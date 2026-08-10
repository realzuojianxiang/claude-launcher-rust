// OpenAI Chat Completions 协议实现（/v1/chat/completions，messages[] 数组）。
//
// 薄封装复用 nvidia::converter 的 pub fn（build_openai_request / openai_response_to_anthropic
// / StreamState / ev_* / sse_event 等），不拷贝 1300 行转换逻辑。nvidia 8082 保持独立运行，
// 本实现只是把 nvidia 已稳定的转换能力作为 trait 实现挂到 8083 通用网关上。
//
// 流式处理流水线从 nvidia/proxy.rs 迁移而来，含「等首段有效输出才决定 fallback」探测机制：
// 先缓冲上游 chunk 探测首段是否有有效输出（text/reasoning/tool_call），
// 探测到有效输出才把缓冲的 chunk + 后续流交给 StreamState 翻译；
// 探测失败（超时空闲/EOF/缓冲超限）则返回特殊 StreamStart 让代理层决定是否切模型重试。
//
// 首段探测已接线：proxy.rs 在流式首段缓冲期间调用 wait_for_meaningful_stream_start，
// 探测到有效输出才把缓冲 chunk + 后续流交给 stream_response 翻译；探测失败则返回
// StreamStart 让代理层决定切模型重试。相关导出项现均被 proxy.rs 消费，不再需要 dead_code 放行。

use crate::gateway::types::{self, RequestStatsContext};
use crate::nvidia::converter;
use crate::nvidia::models::AnthropicRequest;
use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use futures_util::{stream::BoxStream, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::stats::UsageStatsStore;

const MAX_PREOUTPUT_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_RETRYABLE_STREAM_BUFFER_BYTES: usize = 256 * 1024;
const RETRYABLE_STREAM_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);

pub struct ChatCompletionsProtocol;

#[derive(Default)]
struct ToolStartState {
    has_id: bool,
    has_name: bool,
}

#[derive(Default)]
struct StreamStartDetector {
    tools: HashMap<i64, ToolStartState>,
    saw_completion: bool,
}

impl StreamStartDetector {
    fn observe_line(&mut self, line: &str) -> bool {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return false;
        };
        if data == "[DONE]" {
            self.saw_completion = true;
            return true;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return false;
        };
        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return false;
        };
        if choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| !reason.is_empty())
        {
            self.saw_completion = true;
            return true;
        }
        let Some(delta) = choice.get("delta") else {
            return false;
        };
        let has_text = delta
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty());
        let has_reasoning = ["reasoning_content", "reasoning"].iter().any(|field| {
            delta
                .get(field)
                .map(converter::collect_reasoning_texts)
                .is_some_and(|parts| parts.iter().any(|part| !part.trim().is_empty()))
        });
        if has_text || has_reasoning {
            return true;
        }
        delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| {
                calls.iter().any(|call| {
                    let index = call.get("index").and_then(Value::as_i64).unwrap_or(0);
                    let state = self.tools.entry(index).or_default();
                    state.has_id |= call
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| !id.is_empty());
                    state.has_name |= call
                        .get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .is_some_and(|name| !name.is_empty());
                    state.has_id && state.has_name
                })
            })
    }

    fn has_completion(&self) -> bool {
        self.saw_completion
    }
}

/// 首段探测结果：代理层据此决定是否切模型重试。
pub enum StreamStart {
    /// 探测到有效输出，返回已缓冲的 chunk 供后续重放。
    Ready(Vec<Bytes>),
    /// 超时但未探测到有效输出（可重试）。
    Idle,
    /// 上游流在探测到有效输出前关闭（可重试）。
    Ended,
    /// 缓冲超过上限（可重试）。
    BufferLimitExceeded,
    /// 读取失败（可重试）。
    Failed(String),
}

pub async fn wait_for_meaningful_stream_start(
    upstream: &mut BoxStream<'static, Result<Bytes, reqwest::Error>>,
    timeout: std::time::Duration,
) -> StreamStart {
    let mut buffered_chunks = Vec::new();
    let mut buffered_bytes = 0usize;
    let mut parse_buffer = Vec::new();
    let mut detector = StreamStartDetector::default();
    let first_output_deadline = tokio::time::Instant::now() + timeout;
    let mut handoff_deadline = None;

    loop {
        let deadline = handoff_deadline.unwrap_or(first_output_deadline);
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return if handoff_deadline.is_some() {
                StreamStart::Ready(buffered_chunks)
            } else {
                StreamStart::Idle
            };
        }

        match tokio::time::timeout(remaining, upstream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buffered_bytes = buffered_bytes.saturating_add(chunk.len());
                parse_buffer.extend_from_slice(&chunk);
                buffered_chunks.push(chunk);

                for line in crate::shared::split_complete_sse_lines(&mut parse_buffer) {
                    match line {
                        Ok(line) => {
                            if detector.observe_line(&line) && handoff_deadline.is_none() {
                                handoff_deadline =
                                    Some(tokio::time::Instant::now() + RETRYABLE_STREAM_WINDOW);
                            }
                            if detector.has_completion() {
                                return StreamStart::Ready(buffered_chunks);
                            }
                        }
                        Err(error) => return StreamStart::Failed(error.to_string()),
                    }
                }

                if handoff_deadline.is_none() && buffered_bytes > MAX_PREOUTPUT_BUFFER_BYTES {
                    return StreamStart::BufferLimitExceeded;
                }
                if handoff_deadline.is_some() && buffered_bytes >= MAX_RETRYABLE_STREAM_BUFFER_BYTES
                {
                    return StreamStart::Ready(buffered_chunks);
                }
            }
            Ok(Some(Err(error))) => return StreamStart::Failed(error.to_string()),
            Ok(None) => {
                return if detector.has_completion() {
                    StreamStart::Ready(buffered_chunks)
                } else if handoff_deadline.is_some() {
                    StreamStart::Failed("上游流在完成标志前关闭了".to_string())
                } else {
                    StreamStart::Ended
                };
            }
            Err(_) => {
                return if handoff_deadline.is_some() {
                    StreamStart::Ready(buffered_chunks)
                } else {
                    StreamStart::Idle
                };
            }
        }
    }
}

impl types::UpstreamProtocol for ChatCompletionsProtocol {
    fn upstream_path(&self) -> &'static str {
        "/chat/completions"
    }

    fn build_request(&self, req: &AnthropicRequest, model: &str, stream: bool) -> Value {
        converter::build_openai_request(req, model, stream)
    }

    fn parse_non_stream(&self, upstream: &Value, model: &str, msg_id: &str) -> Value {
        // nvidia::converter::openai_response_to_anthropic 签名是 (&Value, &str, &HashMap)，
        // 不接收 msg_id（它内部用 gen_message_id()）。本 trait 方法收 msg_id 但这里不使用——
        // nvidia converter 自行生成 id。保留参数仅为对齐 UpstreamProtocol trait 签名。
        let _ = msg_id;
        let tool_map = HashMap::new();
        converter::openai_response_to_anthropic(upstream, model, &tool_map)
    }

    fn needs_stream_start_probe(&self) -> bool {
        true
    }

    fn stream_response(
        &self,
        mut upstream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
        model: &str,
        idle_timeout: std::time::Duration,
        stats: Arc<UsageStatsStore>,
        record_ctx: RequestStatsContext,
        tool_map: HashMap<String, String>,
    ) -> Response {
        let msg_id = converter::gen_message_id();
        let model = model.to_string();
        let tool_map = tool_map.clone();
        let idle_timeout = std::time::Duration::from_secs(idle_timeout.as_secs().max(30));

        let sse = async_stream::stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from(converter::sse_event(
                "message_start",
                &converter::ev_message_start(&msg_id, &model),
            )));

            let mut state = converter::StreamState::new(tool_map);
            let mut buf: Vec<u8> = Vec::with_capacity(8192);
            let mut stop_reason = "end_turn".to_string();
            let mut output_tokens: u64 = 0;
            let mut input_tokens: u64 = 0;
            let mut cache_read: u64 = 0;
            let mut usage_available = false;
            let mut done = false;
            let mut saw_completion = false;
            let mut abort_reason: Option<String> = None;

            loop {
                let next = match tokio::time::timeout(idle_timeout, upstream.next()).await {
                    Ok(n) => n,
                    Err(_) => {
                        tracing::error!(idle_secs = idle_timeout.as_secs(), "上游流在输出过程中空闲超时，判定挂死");
                        abort_reason =
                            Some(format!("上游流输出过程中空闲超过 {} 秒", idle_timeout.as_secs()));
                        break;
                    }
                };
                let Some(chunk) = next else {
                    if !saw_completion {
                        tracing::error!("上游流在未发出完成标志前正常关闭，判定为异常截断");
                        abort_reason =
                            Some("上游流在未发出完成标志前关闭".to_string());
                    }
                    break;
                };
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(error = %e, "读取上游流失败");
                        abort_reason = Some(format!("读取上游流失败: {e}"));
                        break;
                    }
                };
                buf.extend_from_slice(&chunk);

                for line in crate::shared::split_complete_sse_lines(&mut buf) {
                    let line = match line {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(error = %e, "SSE 行解码为 UTF-8 失败，判定为异常截断");
                            abort_reason = Some(format!("SSE 行 UTF-8 解码失败: {e}"));
                            break;
                        }
                    };
                    let data = match line.strip_prefix("data:") {
                        Some(d) => d.trim(),
                        None => continue,
                    };
                    if data == "[DONE]" {
                        done = true;
                        saw_completion = true;
                        break;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(data) {
                        if let Some(choice) = v
                            .get("choices")
                            .and_then(|c| c.as_array())
                            .and_then(|a| a.first())
                        {
                            if let Some(delta) = choice.get("delta") {
                                let mut reasoning_node = delta.get("reasoning_content");
                                if reasoning_node.is_none() {
                                    reasoning_node = delta.get("reasoning");
                                }
                                if let Some(node) = reasoning_node {
                                    for t in converter::collect_reasoning_texts(node) {
                                        if !t.trim().is_empty() {
                                            for ev in state.handle_thinking(&t) {
                                                yield Ok(Bytes::from(ev));
                                            }
                                        }
                                    }
                                }
                                if let Some(text) = delta.get("content").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        output_tokens += 1;
                                        for ev in state.handle_text(text) {
                                            yield Ok(Bytes::from(ev));
                                        }
                                    }
                                }
                                if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                    for tc in tool_calls {
                                        for ev in state.handle_tool_call(tc) {
                                            yield Ok(Bytes::from(ev));
                                        }
                                    }
                                }
                            }
                            if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                if !fr.is_empty() {
                                    saw_completion = true;
                                    stop_reason = converter::map_stop_reason_with_tool(
                                        state.saw_tool(),
                                        Some(fr),
                                    )
                                    .to_string();
                                    for ev in state.stop_all() {
                                        yield Ok(Bytes::from(ev));
                                    }
                                }
                            }
                        }
                        if let Some(u) = v.get("usage") {
                            if let Some(ct) = u
                                .get("completion_tokens")
                                .and_then(|n| n.as_u64())
                            {
                                output_tokens = ct;
                            }
                            if let Some(pt) = u
                                .get("prompt_tokens")
                                .and_then(|n| n.as_u64())
                            {
                                input_tokens = pt;
                            }
                            if let Some(cd) = u
                                .get("prompt_tokens_details")
                                .and_then(|d| d.get("cached_tokens"))
                                .and_then(|n| n.as_u64())
                            {
                                cache_read = cd;
                            }
                            usage_available = true;
                        }
                    }
                }
                if done {
                    break;
                }
                if abort_reason.is_some() {
                    break;
                }
            }

            for ev in state.stop_all() {
                yield Ok(Bytes::from(ev));
            }

            if let Some(reason) = abort_reason.take() {
                if !done {
                    types::record_usage_safely(&stats, record_ctx.failure_record());
                    yield Ok(Bytes::from(converter::sse_event(
                        "error",
                        &serde_json::json!({
                            "type": "error",
                            "error": { "type": "overloaded_error", "message": format!("上游流异常中断: {reason}") }
                        }),
                    )));
                    return;
                }
            }

            let billable_input = if cache_read > 0 {
                input_tokens.saturating_sub(cache_read)
            } else {
                input_tokens
            };
            types::record_usage_safely(
                &stats,
                record_ctx.success_record(
                    if usage_available { input_tokens } else { 0 },
                    if usage_available { output_tokens } else { 0 },
                    usage_available,
                ),
            );
            yield Ok(Bytes::from(converter::sse_event(
                "message_delta",
                &converter::ev_message_delta(&stop_reason, billable_input, output_tokens, cache_read),
            )));
            yield Ok(Bytes::from(converter::sse_event(
                "message_stop",
                &converter::ev_message_stop(),
            )));
        };

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header("connection", "keep-alive")
            .body(Body::from_stream(sse))
            .unwrap_or_else(|_| {
                types::err_response(StatusCode::INTERNAL_SERVER_ERROR, "构造流式响应失败")
            })
    }
}

// 暴露首段探测函数供代理层调用（代理层重试循环在 needs_stream_start_probe=true 时
// 先调用它探测，再决定是否切模型重试或把缓冲交给 stream_response）。
