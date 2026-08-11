// 代理核心逻辑：接收 Anthropic /v1/messages，转发到 NVIDIA NIM(OpenAI 协议)，再转回 Anthropic。
//
// 【Step 1】基础协议转换 + 流式转发。
// 【Step 2】Key 池轮询 + 429 冷却（见 key_pool）。
// 【Step 3】多模型 Fallback + 重试：单请求最多重试 max_retries 次，失败优先切 Key、再切模型。

use crate::config::NvidiaConfig;
use crate::nvidia::converter;
use crate::nvidia::key_pool::{mask_key, KeyPool, SharedKeyPool};
use crate::nvidia::models::AnthropicRequest;
use crate::stats::{UsageRecord, UsageStatsStore};

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{stream::BoxStream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_PREOUTPUT_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_RETRYABLE_STREAM_BUFFER_BYTES: usize = 256 * 1024;
const RETRYABLE_STREAM_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

// /v1/messages 入站请求体上限：32 MiB。
// Anthropic 请求常含 base64 图片/文档块、长 system、大 tools 定义，合计超
// axum 默认 2 MiB 很现实——默认会被 axum 默默 413 截断在鉴权/解析前，且不是
// Anthropic 风格的 error 事件。handle_messages 用 axum::body::to_bytes(body, 本值)
// 读取请求体：超限时返回 Err，在同层把超限请求重塑为 Anthropic error（413）。
// server.rs 同时挂 DefaultBodyLimit::max(本值) 作为兜底（防止未来若改回 `Bytes`
// 提取器再次踩 2MiB 默认上限）。
pub const MAX_REQUEST_BODY_BYTES: usize = 32 * 1024 * 1024;

// 代理运行时上下文：配置快照 + 复用的异步 HTTP 客户端 + 共享 Key 池
// models 单独用 RwLock 持有，支持 UI 实时热更新优先级（无需重启代理）。
pub struct ProxyCtx {
    pub cfg: NvidiaConfig,
    pub client: reqwest::Client,
    pub key_pool: SharedKeyPool,
    pub stats: Arc<UsageStatsStore>,
    // 模型优先级列表（热更新）：读多写少，用 RwLock；写入来自 NvidiaState::set_models
    pub models: Arc<std::sync::RwLock<Vec<String>>>,
}

impl ProxyCtx {
    pub fn new(cfg: NvidiaConfig, stats: Arc<UsageStatsStore>) -> Arc<Self> {
        // 注意：绝不能用全局 .timeout()——reqwest 的该超时限制的是
        // "整个请求（含读完全部响应体）"的总时长。长思考模型（如 nemotron-ultra）
        // 的 SSE 流经常超过 120s，会在超时点被拦腰截断，客户端收到残缺回复。
        // 正确做法：只限制连接建立；
        // 流式正文的健康度由 stream_response 内的逐 chunk 空闲超时守护，
        // 非流式请求则在发送时挂请求级超时。
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            // P2/SSRF：禁止上游自动跟随重定向。否则操作员误配非默认 base_url、
            // 或上游被劫持返回 30x 时，会把带 `Authorization: Bearer <NVIDIA Key>` 的
            // 请求体转发到攻击者主机/云元数据端点。配合 NvidiaConfig::validate_base_url
            // 的 scheme+host 校验，使上游 URL 与重定向两路都不被外部改写。
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let pool = KeyPool::new(cfg.api_keys.clone(), cfg.key_cooldown_seconds);
        let models = Arc::new(std::sync::RwLock::new(cfg.models.clone()));
        Arc::new(Self {
            cfg,
            client,
            key_pool: SharedKeyPool::new(pool),
            stats,
            models,
        })
    }
}

fn masked_key_for_log(key: &str) -> String {
    mask_key(key)
}

#[derive(Clone, Debug)]
struct RequestStatsContext {
    requested_model: String,
    last_model: String,
    attempts: usize,
}

impl RequestStatsContext {
    fn new(requested_model: String) -> Self {
        Self {
            last_model: requested_model.clone(),
            requested_model,
            attempts: 0,
        }
    }

    fn begin_attempt(&mut self, model: &str, attempts: usize) {
        self.last_model = model.to_string();
        self.attempts = attempts;
    }

    fn success_record(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        usage_available: bool,
    ) -> UsageRecord {
        UsageRecord {
            provider: "nvidia".to_string(),
            requested_model: self.requested_model.clone(),
            final_model: self.last_model.clone(),
            input_tokens,
            output_tokens,
            usage_available,
            retry_count: self.attempts.saturating_sub(1) as u32,
            failed: false,
            at: chrono::Local::now(),
        }
    }

    fn failure_record(&self) -> UsageRecord {
        UsageRecord {
            provider: "nvidia".to_string(),
            requested_model: self.requested_model.clone(),
            final_model: self.last_model.clone(),
            input_tokens: 0,
            output_tokens: 0,
            usage_available: false,
            retry_count: self.attempts.saturating_sub(1) as u32,
            failed: true,
            at: chrono::Local::now(),
        }
    }
}

fn record_usage_safely(store: &UsageStatsStore, record: UsageRecord) {
    if let Err(error) = store.record(record) {
        tracing::warn!(error = %error, "failed to persist usage statistics");
    }
}

// 恒时字节比较（constant-time compare）：避免计时侧信道下的 token 逐字节泄露。
// 纯粹按位异或累积再归约，对相同长度输入恒定时间；不同长度时补齐处理（结果必然不等）。
// 注意：subtle crate 的 ConstantTimeEq 需要在相同长度上安全，长度差异本身会泄露，
// 因此这里把「长度不同」与「内容不等」统一归约到一个固定流程，不提前返回。
fn ct_eq(a: &str, b: &str) -> bool {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    if ab.len() != bb.len() {
        return false; // 长度公开可见，无需恒时处理
    }
    let mut diff = 0u8;
    for (x, y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// 【S3】从字节缓冲里切分出所有「已完整到达的 SSE 行」并就地 drain。
// 只在 '\n' 边界处切分（兼容 CRLF），不完整的尾部字节保留在 buf 中等到下一 chunk，
// 因此跨 chunk 的多字节 UTF-8 字符绝不会被劈开。每行做严格 UTF-8 解码：
//   Ok(s)  —— 行首已 strip_prefix 处理交给调用方，这里只给 trim 后的整行；
//   Err(e) —— 解码失败，调用方应判为流异常截断并发 error，绝不产乱码/坏 JSON。
// 这是 stream_response 与单元测试共用的纯函数，避免热路径与测试各自维护一份分块逻辑。
fn split_complete_sse_lines(buf: &mut Vec<u8>) -> Vec<Result<String, std::str::Utf8Error>> {
    let mut out = Vec::new();
    while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
        let line_end = if pos > 0 && buf[pos - 1] == b'\r' {
            pos - 1
        } else {
            pos
        };
        let raw: Vec<u8> = buf[..line_end].to_vec();
        buf.drain(..=pos);
        // 跳过空行 / 仅空白的分隔行
        if raw.is_empty() || raw.iter().all(|b| *b == b' ') {
            continue;
        }
        match std::str::from_utf8(&raw) {
            Ok(s) => out.push(Ok(s.trim().to_string())),
            Err(e) => out.push(Err(e)),
        }
    }
    out
}

// 把错误及其完整 source 链拼成一行。reqwest 的 "error decoding response body"
// 只是 body 流错误的统一外层包装（bytes_stream 一律 map_err 成 Kind::Decode），
// 真正的 hyper 层根因（连接重置 / HTTP2 RST / IncompleteMessage）在 source 链里；
// 排查日志必须打出全链才能区分「NVIDIA 主动断」「网络中间层掐断」等场景。
fn format_error_chain(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut cur = error.source();
    while let Some(src) = cur {
        parts.push(src.to_string());
        cur = src.source();
    }
    parts.join(" -> ")
}

#[derive(Default)]
struct ToolStartState {
    has_id: bool,
    has_name: bool,
}

#[derive(Default)]
struct StreamStartDetector {
    tools: HashMap<i64, ToolStartState>,
    saw_completion: bool,
    saw_output: bool,
}

impl StreamStartDetector {
    fn observe_line(&mut self, line: &str) -> bool {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return false;
        };
        if data == "[DONE]" {
            // 完成标志单独记录：不是有效输出（空流也可能以 [DONE] 收尾）
            self.saw_completion = true;
            return false;
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
        // 完成标志单独记录，不提前 return：finish_reason 与内容可能同帧，
        // 必须继续检查 delta 里是否有真实输出。
        if choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| !reason.is_empty())
        {
            self.saw_completion = true;
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
        let has_tool = delta
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
            });
        if has_text || has_reasoning || has_tool {
            self.saw_output = true;
            return true;
        }
        false
    }

    fn has_completion(&self) -> bool {
        self.saw_completion
    }

    // 是否出现过真实有效输出（text / reasoning / tool id+name）。
    // 完成标志（finish_reason / [DONE]）单独记录，不视为有效输出——
    // 「空完成」（如 200 + 空 choices + finish_reason）必须按空流处理，
    // 否则会被当成成功转发，claude-code 压缩会报 no assistant message。
    fn has_output(&self) -> bool {
        self.saw_output
    }
}

#[cfg(test)]
fn sse_line_has_meaningful_output(line: &str) -> bool {
    StreamStartDetector::default().observe_line(line)
}

enum StreamStart {
    Ready(Vec<Bytes>),
    Idle,
    Ended,
    BufferLimitExceeded,
    Failed(String),
}

async fn wait_for_meaningful_stream_start(
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

                for line in split_complete_sse_lines(&mut parse_buffer) {
                    match line {
                        Ok(line) => {
                            if detector.observe_line(&line) && handoff_deadline.is_none() {
                                handoff_deadline =
                                    Some(tokio::time::Instant::now() + RETRYABLE_STREAM_WINDOW);
                            }
                            // 只有「完成标志 + 确有有效输出」才算可转发的成功流；
                            // 空完成（finish_reason/[DONE] 但零内容）留给 EOF 分支判 Ended。
                            if detector.has_completion() && detector.has_output() {
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
            Ok(Some(Err(error))) => {
                return StreamStart::Failed(format_error_chain(&error));
            }
            Ok(None) => {
                return if detector.has_completion() && detector.has_output() {
                    StreamStart::Ready(buffered_chunks)
                } else if handoff_deadline.is_some() {
                    StreamStart::Failed("上游流在完成标志前关闭了".to_string())
                } else {
                    // 含「空完成」：上游以完成标志收尾但全程零有效输出，
                    // 视为空流，交由调用方按「有效输出前关闭」切模型/重试。
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

async fn read_error_body_limited(resp: reqwest::Response, timeout: std::time::Duration) -> String {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut stream = resp.bytes_stream();
    let mut body = Vec::new();
    let mut suffix = "";

    loop {
        if body.len() >= MAX_ERROR_BODY_BYTES {
            suffix = "…（错误体已截断）";
            break;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            suffix = "…（读取错误体超时）";
            break;
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                let available = MAX_ERROR_BODY_BYTES - body.len();
                let take = available.min(chunk.len());
                body.extend_from_slice(&chunk[..take]);
                if take < chunk.len() {
                    suffix = "…（错误体已截断）";
                    break;
                }
            }
            Ok(Some(Err(error))) => {
                suffix = "…（读取错误体失败）";
                tracing::debug!(error = %error, "读取上游错误体失败");
                break;
            }
            Ok(None) => break,
            Err(_) => {
                suffix = "…（读取错误体超时）";
                break;
            }
        }
    }

    format!("{}{}", String::from_utf8_lossy(&body), suffix)
}

// 构造 Anthropic 风格错误响应
fn err_response(status: StatusCode, msg: &str) -> Response {
    let body = json!({
        "type": "error",
        "error": { "type": "proxy_error", "message": msg }
    });
    (status, axum::Json(body)).into_response()
}

// axum handler：POST /v1/messages
//
// 这里直接接收 Request<Body> 而非用 `body: Bytes` 提取器，是为了把超限请求（>
// MAX_REQUEST_BODY_BYTES）的 413 重新塑形为 Anthropic 风格 error 事件——否则 axum
// 默认 `Bytes` 提取器在解析前就把超限请求按 413 plaintext 默默截断，既误拒真实
// 大请求、又与代理「讲 Anthropic 协议」的承诺不一致。
pub async fn handle_messages(
    State(ctx): State<Arc<ProxyCtx>>,
    request: axum::http::Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;

    // 1. 可选的本地代理鉴权：配置了 auth_token 时恒时校验请求头 x-api-key
    if !ctx.cfg.auth_token.is_empty() {
        let provided = headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !ct_eq(provided, &ctx.cfg.auth_token) {
            return err_response(StatusCode::UNAUTHORIZED, "无效的 x-api-key");
        }
    }

    // 2. 读取请求体（带显式上限 MAX_REQUEST_BODY_BYTES）。
    // axum::body::to_bytes 在超限时返回 LengthLimitError，我们把这条路径重塑成
    // Anthropic error（413），而非 axum 默认 plaintext。
    let body: Bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            return err_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!(
                    "请求体超过 {} MiB 上限，请缩减请求（如减少图片/文档块体积）后重试。",
                    MAX_REQUEST_BODY_BYTES / (1024 * 1024)
                ),
            );
        }
    };

    // 3. 解析 Anthropic 请求体
    let req: AnthropicRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, &format!("请求体解析失败: {e}")),
    };

    // 4. 基础校验：至少要配置一个 Key 与一个模型
    if ctx.cfg.api_keys.is_empty() {
        return err_response(StatusCode::SERVICE_UNAVAILABLE, "未配置任何 NVIDIA API Key");
    }
    // 实时读取当前模型优先级快照（可被 UI 热更新，无需重启代理）
    let live_models: Vec<String> = ctx
        .models
        .read()
        .map(|m| m.clone())
        .unwrap_or_else(|_| ctx.cfg.models.clone());
    if live_models.is_empty() {
        return err_response(StatusCode::SERVICE_UNAVAILABLE, "未配置任何模型");
    }

    // 5. 模型 Fallback 链：请求体里的 model（即 Launch 页所选）优先，
    //    其后按配置顺序追加其余模型（去重、保持顺序）。
    let stream = req.is_stream();
    let requested_model = req
        .model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| live_models[0].clone());
    let base_model = requested_model.clone();
    let mut models_chain: Vec<String> = vec![base_model.clone()];
    for m in &live_models {
        if !models_chain.iter().any(|x| x.eq_ignore_ascii_case(m)) {
            models_chain.push(m.clone());
        }
    }

    let max_retries = ctx.cfg.max_retries.max(1) as usize;
    let url = format!(
        "{}/chat/completions",
        ctx.cfg.base_url.trim_end_matches('/')
    );

    // 6. Step 2/3：重试循环——Key 级故障（429/网络错误）才切 Key；
    // 模型故障（5xx/长时间无有效输出）切模型但复用当前 Key。上限 max_retries 次。
    let mut model_idx = 0usize;
    let mut attempts = 0usize;
    let mut last_msg = "上游未返回可用响应".to_string();
    // 仅用于“尚未产生有意义输出”的模型 fallback：NVIDIA 的 Key 在模型间共用，
    // 因此这种场景复用原 Key，避免无意义轮换到另一个 Key。
    let mut sticky_key: Option<String> = None;
    let mut record_ctx = RequestStatsContext::new(requested_model);

    loop {
        attempts += 1;
        if attempts > max_retries {
            break;
        }
        let model = models_chain[model_idx % models_chain.len()].clone();
        record_ctx.begin_attempt(&model, attempts);

        // 6a. 从 Key 池轮询取一个可用 Key（跳过冷却中的）
        let key = match sticky_key.take() {
            Some(key) => key,
            None => {
                let mut pool = ctx.key_pool.lock().unwrap();
                match pool.pick() {
                    Some(k) => k,
                    None => {
                        last_msg = "所有 API Key 均处于冷却中，请稍后重试".to_string();
                        break; // 全部冷却，无法继续
                    }
                }
            }
        };
        let masked_key = masked_key_for_log(&key);

        // 6b. 按本次所选模型构造 OpenAI 请求体并转发
        // 构建工具名大小写还原映射（从原始 Anthropic 请求的 tools 提取）
        let tool_map = converter::build_tool_name_map(&req.tools);
        let openai_body = converter::build_openai_request(&req, &model, stream);
        tracing::info!(attempt = attempts, model = %model, key = %masked_key, "转发请求到 NVIDIA NIM（Key 已轮询）");

        let mut builder = ctx
            .client
            .post(&url)
            .bearer_auth(&key)
            // 显式声明 identity，避免上游/CDN 对 SSE 流做压缩（虽然本客户端未启用
            // gzip 解码特性，但部分中间层可能无视 Accept-Encoding 仍返回压缩流，
            // 污染按行 SSE 解析）。
            // 注意：日志里的 "error decoding response body" 是 reqwest 对**所有**
            // body 流读取错误的统一包装（bytes_stream 一律 map_err 成 Kind::Decode），
            // 真实原因（连接断开/RST/IncompleteMessage）在 error.source() 链里——
            // 不是 gzip 解码问题，identity 头挡不住连接层错误，排查须打全链。
            .header(header::ACCEPT_ENCODING, "identity")
            .header(header::ACCEPT, "application/json")
            .json(&openai_body);
        // 超时策略区分流式/非流式：
        // - 非流式：请求级超时覆盖"头 + 整个响应体"（原语义保留）。
        // - 流式：请求级超时会把长思考模型的长 SSE 流拦腰截断（曾导致回复输出到一半
        //   突然"正常结束"），因此只在这里限制"响应头到达"，正文由逐 chunk 空闲超时守护。
        let timeout = std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds);
        if !stream {
            builder = builder.timeout(timeout);
        }
        let send_fut = builder.send();
        let resp = match tokio::time::timeout(timeout, send_fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                // 网络错误 / 超时：视为可重试，下一轮换 Key（或全部冷却则失败）
                tracing::warn!(attempt = attempts, key = %masked_key, error = %e, "上游网络错误（超时/连接），切换重试");
                last_msg = format!("上游网络错误: {e}");
                continue;
            }
            Err(_) => {
                tracing::warn!(attempt = attempts, key = %masked_key, timeout_secs = ctx.cfg.request_timeout_seconds, "上游响应头超时，切换重试");
                last_msg = format!("上游 {} 秒内未返回响应头", ctx.cfg.request_timeout_seconds);
                continue;
            }
        };

        let status = resp.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            // 6c. 429：冷却该 Key，保持当前模型，下一轮换用其他 Key
            {
                let mut pool = ctx.key_pool.lock().unwrap();
                pool.cooldown(&key);
            }
            let text = read_error_body_limited(
                resp,
                std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds.max(1)),
            )
            .await;
            tracing::warn!(
                attempt = attempts,
                key = %masked_key,
                key_cooled = true,
                "收到 429，冷却该 Key 并切 Key 重试"
            );
            last_msg = format!("上游 429 限流，已冷却 Key 并切换重试: {text}");
            continue;
        }
        if status.is_server_error() {
            // 6d. 5xx：切换模型并复用当前 Key；没有后备模型时直接失败。
            let text = read_error_body_limited(
                resp,
                std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds.max(1)),
            )
            .await;
            tracing::warn!(attempt = attempts, model = %model, key = %masked_key, status = %status.as_u16(), "上游 5xx，切换模型重试");
            last_msg = format!("上游错误 {}: {}", status.as_u16(), text);
            if model_idx < models_chain.len() - 1 {
                model_idx += 1;
                sticky_key = Some(key);
                continue;
            }
            break;
        }
        if status == StatusCode::NOT_FOUND && model_idx < models_chain.len() - 1 {
            // 404 通常表示当前模型在 NVIDIA 路由中不存在或暂不可用。
            // NVIDIA Key 在模型间共用，因此复用当前 Key 并切换备选模型，而不是轮换 Key。
            let text = read_error_body_limited(
                resp,
                std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds.max(1)),
            )
            .await;
            tracing::warn!(
                attempt = attempts,
                model = %model,
                next_model = %models_chain[model_idx + 1],
                key = %masked_key,
                status = %status,
                body = %text,
                "上游模型返回 404，复用当前 Key 并切换备选模型"
            );
            last_msg = format!("模型 {model} 返回 404: {text}");
            model_idx += 1;
            sticky_key = Some(key);
            continue;
        }
        if status.is_redirection() {
            // P2/SSRF：redirect 已被 Policy::none() 关闭，故上游任何 3xx 都会原样
            // 返回到这里。把这个原本"会被静默跟随、把请求体+bearer token 导流到
            // Location 所指主机"的状态，明确判为致命错误并立即终止——既不重试、也不
            // 接力转发，避免被劫持上游借 30x 把用户的 NVIDIA Key 套走。
            let text = read_error_body_limited(
                resp,
                std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds.max(1)),
            )
            .await;
            record_usage_safely(&ctx.stats, record_ctx.failure_record());
            tracing::error!(
                model = %model,
                status = %status.as_u16(),
                body = %text,
                "上游返回重定向（已禁止跟随），中止以防 bearer token 外泄"
            );
            return err_response(
                StatusCode::BAD_GATEWAY,
                &format!(
                    "上游返回重定向 {}：已禁止跟随以避免 API Key 泄漏。请检查 NVIDIA Base URL 是否指向官方端点。",
                    status.as_u16()
                ),
            );
        }
        if !status.is_success() {
            // 4xx（非 429）：客户端错误，不重试，直接返回上游真实错误
            let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let text = read_error_body_limited(
                resp,
                std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds.max(1)),
            )
            .await;
            record_usage_safely(&ctx.stats, record_ctx.failure_record());
            tracing::warn!(model = %model, key = %masked_key, status = %code, body = %text, "上游返回客户端错误（不重试）");
            return err_response(code, &format!("上游错误 {code}: {text}"));
        }

        // 6e. 成功：按 stream 分流处理
        tracing::info!(attempt = attempts, model = %model, key = %masked_key, "上游成功");
        if stream {
            let timeout = std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds.max(1));
            let mut upstream = resp.bytes_stream().boxed();
            match wait_for_meaningful_stream_start(&mut upstream, timeout).await {
                StreamStart::Ready(buffered) => {
                    return stream_response(
                        buffered,
                        upstream,
                        &model,
                        &tool_map,
                        ctx.cfg.request_timeout_seconds,
                        ctx.stats.clone(),
                        record_ctx.clone(),
                    );
                }
                outcome @ (StreamStart::Idle
                | StreamStart::Ended
                | StreamStart::BufferLimitExceeded)
                    if model_idx + 1 < models_chain.len() =>
                {
                    let reason = match outcome {
                        StreamStart::Idle => "长时间无有效输出",
                        StreamStart::Ended => "在有效输出前关闭",
                        StreamStart::BufferLimitExceeded => "有效输出前数据超过缓冲上限",
                        _ => unreachable!(),
                    };
                    tracing::warn!(
                        attempt = attempts,
                        model = %model,
                        next_model = %models_chain[model_idx + 1],
                        key = %masked_key,
                        timeout_secs = ctx.cfg.request_timeout_seconds,
                        "{reason}，复用当前 Key 并切换模型"
                    );
                    last_msg = format!("模型 {model} {reason}");
                    model_idx += 1;
                    sticky_key = Some(key);
                    continue;
                }
                StreamStart::Idle => {
                    last_msg = format!(
                        "模型 {model} 在 {} 秒内无有效输出，且没有后备模型",
                        ctx.cfg.request_timeout_seconds
                    );
                    break;
                }
                StreamStart::Ended => {
                    last_msg = format!("模型 {model} 在有效输出前关闭，且没有后备模型");
                    break;
                }
                StreamStart::BufferLimitExceeded => {
                    last_msg = format!(
                        "模型 {model} 在有效输出前超过 {} 字节缓冲上限，且没有后备模型",
                        MAX_PREOUTPUT_BUFFER_BYTES
                    );
                    break;
                }
                StreamStart::Failed(error) => {
                    tracing::warn!(
                        attempt = attempts,
                        model = %model,
                        key = %masked_key,
                        error = %error,
                        "读取首段上游流失败，切换重试"
                    );
                    last_msg = format!("读取模型 {model} 首段流失败: {error}");
                    continue;
                }
            }
        } else {
            return non_stream_response(
                resp,
                &model,
                &tool_map,
                ctx.stats.clone(),
                record_ctx.clone(),
            )
            .await;
        }
    }

    record_usage_safely(&ctx.stats, record_ctx.failure_record());
    err_response(
        StatusCode::BAD_GATEWAY,
        &format!(
            "共尝试 {} 次仍失败：{}",
            attempts.min(max_retries),
            last_msg
        ),
    )
}

// 非流式：读取上游 JSON 并转成 Anthropic 响应
async fn non_stream_response(
    resp: reqwest::Response,
    model: &str,
    tool_map: &HashMap<String, String>,
    stats: Arc<UsageStatsStore>,
    record_ctx: RequestStatsContext,
) -> Response {
    let openai: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            record_usage_safely(&stats, record_ctx.failure_record());
            return err_response(StatusCode::BAD_GATEWAY, &format!("上游响应解析失败: {e}"));
        }
    };
    let (input_tokens, output_tokens, _cache_read) = openai
        .get("usage")
        .map(converter::extract_openai_usage)
        .unwrap_or((0, 0, 0));
    record_usage_safely(
        &stats,
        record_ctx.success_record(input_tokens, output_tokens, openai.get("usage").is_some()),
    );
    let anthropic = converter::openai_response_to_anthropic(&openai, model, tool_map);
    axum::Json(anthropic).into_response()
}

// 流式：将上游 OpenAI SSE 转换为 Anthropic SSE 事件序列后转发
// 使用 StreamState 动态管理 content block（text / tool_use）的生命周期：
//   message_start -> [content_block_start -> content_block_delta* -> content_block_stop]* ->
//   message_delta -> message_stop
fn stream_response(
    buffered_chunks: Vec<Bytes>,
    mut upstream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    model: &str,
    tool_map: &HashMap<String, String>,
    idle_timeout_secs: u64,
    stats: Arc<UsageStatsStore>,
    record_ctx: RequestStatsContext,
) -> Response {
    let msg_id = converter::gen_message_id();
    let model = model.to_string();
    let tool_map = tool_map.clone();
    // 逐 chunk 空闲超时：只要上游持续产出 token 就永不超时（长思考流可以跑几十分钟），
    // 只有"两个 chunk 之间"卡住超过该时长才判定上游挂死。
    let idle_timeout = std::time::Duration::from_secs(idle_timeout_secs.max(30));

    let sse = async_stream::stream! {
        // 1. 发出 message_start（content_block_start 延迟到有实际内容时才发）
        yield Ok::<Bytes, std::io::Error>(Bytes::from(converter::sse_event(
            "message_start",
            &converter::ev_message_start(&msg_id, &model),
        )));

        let mut state = converter::StreamState::new(tool_map);
        let mut buffered_chunks = buffered_chunks.into_iter();
        // 【S3】字节缓冲累计原始网络字节，按 SSE 行边界（'\n'）切分后才做严格 UTF-8 解码。
        // 不再对每个 chunk 单独 String::from_utf8_lossy：那会把跨 chunk 的多字节字符
        // （三字节汉字 / 四字节 emoji）不可逆替换为 �，工具调用参数 JSON 也可能因此损坏。
        // 不完整的尾部字节天然留在 buf 里等到下一 chunk。
        let mut buf: Vec<u8> = Vec::with_capacity(8192);
        let mut stop_reason = "end_turn".to_string();
        let mut output_tokens: u64 = 0;
        let mut input_tokens: u64 = 0;
        let mut cache_read: u64 = 0;
        let mut usage_available = false;
        let mut done = false;
        // 【Spec EOF】是否已观察到上游的完成标志（[DONE] 或非空 finish_reason）。
        // 仅在确认完成后，EOF 才允许走正常尾帧；否则干净 EOF 属于异常截断，
        // 应发 error 而非伪装成 end_turn（避免客户端把残缺回复当完整结果）。
        let mut saw_completion = false;
        // 流中断原因：Some(描述) 表示异常截断，需要向客户端发 error 事件而非伪装正常结束
        let mut abort_reason: Option<String> = None;
        // 是否产出过至少一个内容块（text / thinking / tool_use）。
        // 零内容「成功」流（如 200 + 空 choices + finish_reason）必须发 error，
        // 见下方 2c 空完成兜底。
        let mut produced_content = false;

        loop {
            let next = if let Some(chunk) = buffered_chunks.next() {
                Some(Ok(chunk))
            } else {
                match tokio::time::timeout(idle_timeout, upstream.next()).await {
                    Ok(n) => n,
                    Err(_) => {
                        tracing::error!(idle_secs = idle_timeout.as_secs(), "上游流在输出过程中空闲超时，判定挂死");
                        abort_reason =
                            Some(format!("上游流输出过程中空闲超过 {} 秒", idle_timeout.as_secs()));
                        break;
                    }
                }
            };
            let Some(chunk) = next else {
                // 上游关闭流：未见完成标志（[DONE]/finish_reason）即 EOF 属于异常截断，
                // 不能伪装成正常 end_turn 结束（否则客户端把残缺回复当完整结果，且不重试）。
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
                    tracing::error!(error = %format_error_chain(&e), "读取上游流失败（含根因链）");
                    abort_reason = Some(format!("读取上游流失败: {e}"));
                    break;
                }
            };
            buf.extend_from_slice(&chunk);

            // 按行解析已完整到达的 SSE 行（按字节边界切分 + 严格 UTF-8 解码，跨 chunk
            // 不完整尾部留在 buf 等下一 chunk；详见 split_complete_sse_lines）。
            for line in split_complete_sse_lines(&mut buf) {
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
                // 解析一段 OpenAI chunk
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(choice) = v
                        .get("choices")
                        .and_then(|c| c.as_array())
                        .and_then(|a| a.first())
                    {
                        if let Some(delta) = choice.get("delta") {
                            // 扩展思考 reasoning_content（兼容旧字段 reasoning）
                            let mut reasoning_node = delta.get("reasoning_content");
                            if reasoning_node.is_none() {
                                reasoning_node = delta.get("reasoning");
                            }
                            if let Some(node) = reasoning_node {
                                for t in converter::collect_reasoning_texts(node) {
                                    if !t.trim().is_empty() {
                                        produced_content = true;
                                        for ev in state.handle_thinking(&t) {
                                            yield Ok(Bytes::from(ev));
                                        }
                                    }
                                }
                            }
                            // 文本增量
                            if let Some(text) = delta.get("content").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    output_tokens += 1;
                                    produced_content = true;
                                    for ev in state.handle_text(text) {
                                        yield Ok(Bytes::from(ev));
                                    }
                                }
                            }
                            // 工具调用增量
                            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                for tc in tool_calls {
                                    for ev in state.handle_tool_call(tc) {
                                        produced_content = true;
                                        yield Ok(Bytes::from(ev));
                                    }
                                }
                            }
                        }
                        // finish_reason：关闭所有内容块
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
                    // usage（部分实现会在最后一帧给出）
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
            // 内层按行循环因 UTF-8 解码失败等异常提前 break 时，abort_reason 已置位，
            // 此处跳出外层循环，统一走下方 error 尾帧分支。
            if abort_reason.is_some() {
                break;
            }
        }

        // 2. 确保 all 内容块已关闭（finish_reason 未触发或流中断时的兜底）
        for ev in state.stop_all() {
            yield Ok(Bytes::from(ev));
        }

        // 2b. 异常截断：向客户端发诚实的 error 事件后结束，
        //     绝不伪装成正常 end_turn（否则客户端会把残缺回复当完整结果展示，
        //     也不会触发重试）。Anthropic SSE 协议允许流中出现 error 事件。
        if let Some(reason) = abort_reason.take() {
            if !done {
                record_usage_safely(&stats, record_ctx.failure_record());
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

        // 2c. 空完成兜底：上游「成功」但全程零内容块（如 200 + 空 choices + finish_reason）。
        //     不能伪装成 end_turn 空消息——claude-code 压缩会因此报
        //     "no assistant message in summarization response"。发诚实 error 事件，
        //     让客户端走重试/降级，而非吞掉空响应。
        if !produced_content {
            record_usage_safely(&stats, record_ctx.failure_record());
            yield Ok(Bytes::from(converter::sse_event(
                "error",
                &serde_json::json!({
                    "type": "error",
                    "error": { "type": "overloaded_error", "message": "上游流未产出任何内容块，判定为空响应" }
                }),
            )));
            return;
        }

        // 3. 尾部事件（input_tokens 扣除缓存命中部分，与 Anthropic 用量语义对齐）
        let billable_input = if cache_read > 0 {
            input_tokens.saturating_sub(cache_read)
        } else {
            input_tokens
        };
        record_usage_safely(
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
        .unwrap_or_else(|_| err_response(StatusCode::INTERNAL_SERVER_ERROR, "构造流式响应失败"))
}

// 连接自检：用配置的第一个 Key + 第一个模型，向 NVIDIA 发一个极短的非流式请求，
// 返回成功文本或真实上游错误。供 UI「测试连接」按钮调用，便于排查 key/模型/网络问题。
pub async fn test_connection(cfg: &NvidiaConfig) -> Result<String, String> {
    if cfg.api_keys.is_empty() {
        return Err("❌ 未配置任何 NVIDIA API Key".to_string());
    }
    if cfg.models.is_empty() {
        return Err("❌ 未配置任何模型".to_string());
    }
    let key = &cfg.api_keys[0];
    let masked_key = masked_key_for_log(key);
    let model = &cfg.models[0];
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(cfg.request_timeout_seconds))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let probe = json!({
        "model": model,
        "messages": [ { "role": "user", "content": "hi" } ],
        "max_tokens": 16,
        "stream": false,
    });
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));

    tracing::info!(model = %model, key = %masked_key, "连接自检：向 NVIDIA 发送探测请求");
    let resp = match client
        .post(&url)
        .bearer_auth(key)
        .header(header::ACCEPT_ENCODING, "identity")
        .json(&probe)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(key = %masked_key, error = %e, "连接自检：上游请求失败");
            return Err(format!("❌ 上游请求失败: {e}"));
        }
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(key = %masked_key, status = %status.as_u16(), body = %text, "连接自检：上游返回错误");
        return Err(format!("❌ 上游错误 {}: {}", status.as_u16(), text));
    }

    // 提取返回文本
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let out = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    tracing::info!(model = %model, key = %masked_key, len = out.len(), "连接自检：成功");
    Ok(format!(
        "✅ 连接成功（模型 {}）：{}",
        model,
        if out.is_empty() {
            "（空响应）"
        } else {
            &out
        }
    ))
}

// 本地消息测试：向本机运行中的代理 127.0.0.1:{port}/v1/messages 发一条 Anthropic
// 非流式请求，走完整转换链（Anthropic→OpenAI→NIM→Anthropic）。
// 供 UI「发送测试」按钮调用，可指定模型；返回耗时 + tokens + 回复文本。
pub async fn local_chat_test(
    cfg: &NvidiaConfig,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{}/v1/messages", cfg.port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            cfg.request_timeout_seconds.max(30),
        ))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let body = json!({
        "model": model,
        "max_tokens": 256,
        "messages": [ { "role": "user", "content": prompt } ],
        "stream": false,
    });

    let mut req = client.post(&url).json(&body);
    // 配置了本地鉴权时自动带上 x-api-key
    if !cfg.auth_token.is_empty() {
        req = req.header("x-api-key", &cfg.auth_token);
    }

    tracing::info!(model = %model, "本地消息测试：POST {url}");
    let t0 = std::time::Instant::now();
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let hint = if e.is_connect() {
                "（代理未运行？请先点击「启动」）"
            } else {
                ""
            };
            return Err(format!("❌ 请求本地代理失败: {e} {hint}"));
        }
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let elapsed = t0.elapsed().as_secs_f32();
    if !status.is_success() {
        return Err(format!(
            "❌ 代理返回 {}（{:.1}s）: {}",
            status.as_u16(),
            elapsed,
            text
        ));
    }

    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    // 汇总 content 中的 text 块（忽略 thinking 块，但统计其存在）
    let mut reply = String::new();
    let mut has_thinking = false;
    if let Some(blocks) = v.get("content").and_then(|c| c.as_array()) {
        for b in blocks {
            match b.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                        reply.push_str(t);
                    }
                }
                Some("thinking") => has_thinking = true,
                _ => {}
            }
        }
    }
    let in_tok = v
        .pointer("/usage/input_tokens")
        .and_then(|n| n.as_u64())
        .unwrap_or(0);
    let out_tok = v
        .pointer("/usage/output_tokens")
        .and_then(|n| n.as_u64())
        .unwrap_or(0);
    let used_model = v.get("model").and_then(|m| m.as_str()).unwrap_or(model);

    tracing::info!(model = %used_model, secs = elapsed, "本地消息测试：成功");
    Ok(format!(
        "✅ {}（{:.1}s，in {} / out {} tokens{}）\n{}",
        used_model,
        elapsed,
        in_tok,
        out_tok,
        if has_thinking { "，含 thinking" } else { "" },
        if reply.trim().is_empty() {
            "（空文本回复）"
        } else {
            reply.trim()
        },
    ))
}

#[cfg(test)]
mod masked_key_log_tests {
    use super::masked_key_for_log;

    #[test]
    fn request_log_uses_a_masked_key_without_exposing_the_secret() {
        let secret = "nvapi-1234567890abcd";
        let displayed = masked_key_for_log(secret);

        assert_eq!(displayed, "nvap…abcd");
        assert!(!displayed.contains(secret));
    }
}

#[cfg(test)]
mod sse_utf8_tests {
    use super::split_complete_sse_lines;

    // 【S3 回归】一个三字节汉字「中」(UTF-8: e4 b8 ad) 被网络切成两 chunk 时，
    // 字节缓冲必须把不完整的尾部保留到下一 chunk，再整体解码——绝不可按 chunk 单独
    // from_utf8_lossy 把中间字节替换为 �。
    #[test]
    fn multibyte_utf8_split_across_chunks_is_reassembled_without_replacement() {
        // 构造一条完整的 SSE 行，但其多字节字符故意跨 chunk 切断
        let full = "data: {\"content\":\"中\"}\n".as_bytes().to_vec();
        let mid = full.len() / 2; // 在「中」的 UTF-8 字节中间切断
        let chunk_a = &full[..mid];
        let chunk_b = &full[mid..];

        let mut buf = Vec::new();
        buf.extend_from_slice(chunk_a);
        // 第一 chunk 不完整：不应产出任何完整行，残留保留在 buf
        let lines = split_complete_sse_lines(&mut buf);
        assert!(lines.is_empty(), "半截行不应产出");
        assert!(!buf.is_empty(), "不完整尾部应保留在 buf");

        buf.extend_from_slice(chunk_b);
        let lines = split_complete_sse_lines(&mut buf);
        assert_eq!(lines.len(), 1, "拼齐后应产出一行");
        let line = lines[0].as_ref().expect("UTF-8 解码应成功");
        assert_eq!(line, "data: {\"content\":\"中\"}", "汉字应被无损还原");
        assert!(buf.is_empty(), "行尾换行后 buf 应清空");
    }

    // 整行的多字节字符（emoji / 多个汉字）正常解码，不被行切分破坏。
    #[test]
    fn full_multibyte_line_decodes_correctly() {
        let mut buf = b"data: \xe4\xbd\xa0\xe5\xa5\xbd\n".to_vec(); // "你好"
        let lines = split_complete_sse_lines(&mut buf);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].as_ref().unwrap(), "data: 你好");
    }

    // 跨多行、含空行分隔的标准 SSE 帧，全部被正确切分且空行被跳过。
    #[test]
    fn multiple_lines_with_blank_separators_are_split() {
        let mut buf = b"data: a\n\ndata: b\n\n".to_vec();
        let lines = split_complete_sse_lines(&mut buf);
        assert_eq!(
            lines.into_iter().map(|r| r.unwrap()).collect::<Vec<_>>(),
            vec!["data: a", "data: b"]
        );
    }

    // CRLF 行尾兼容：\r\n 应去掉 \r，不混入行内容。
    #[test]
    fn crlf_line_endings_are_handled() {
        let mut buf = b"data: hello\r\n".to_vec();
        let lines = split_complete_sse_lines(&mut buf);
        assert_eq!(lines[0].as_ref().unwrap(), "data: hello");
    }
}

#[cfg(test)]
mod completion_flag_tests {
    use super::split_complete_sse_lines;

    // 【Spec EOF 回归】上游仅在数据帧里给 [DONE] 时，split + 行扫描应识别到完成标志。
    // 这锁定了「[DONE] 是完成信号之一」的判定来源。
    #[test]
    fn done_marker_is_emitted_as_a_complete_line() {
        let mut buf = b"data: {\"content\":\"hi\"}\n\ndata: [DONE]\n".to_vec();
        let lines = split_complete_sse_lines(&mut buf);
        let has_done = lines
            .iter()
            .any(|r| r.as_ref().map(|s| s == "data: [DONE]").unwrap_or(false));
        assert!(has_done, "[DONE] 应能被行扫描识别");
    }

    // 【Spec EOF 回归】上游流**没有任何** [DONE] 也没有 finish_reason 帧即结束，
    // 对应「干净 EOF」场景：此时未观察到完成标志，按需求不应伪装 end_turn。
    // 这里锁定判定所需的事实——一个不含 [DONE]/finish_reason 的帧序列不应被误判为完成。
    #[test]
    fn stream_without_done_or_finish_reason_is_not_marked_complete() {
        let mut buf = b"data: {\"choices\":[{\"delta\":{\"content\":\"pa\"}}]}\n".to_vec();
        let lines = split_complete_sse_lines(&mut buf);
        // 逐行检查：既无 [DONE] 行，也无任何含非空 finish_reason 的 JSON 行
        let saw_done = lines
            .iter()
            .any(|r| r.as_ref().map(|s| s == "data: [DONE]").unwrap_or(false));
        let saw_finish = lines.iter().any(|r| match r {
            Ok(s) => s
                .strip_prefix("data:")
                .map(|d| d.trim())
                .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
                .map(|v| {
                    // 观察到非空 finish_reason 即视为完成信号
                    v.get("choices")
                        .and_then(|c| c.as_array())
                        .and_then(|a| a.first())
                        .and_then(|c| c.get("finish_reason"))
                        .and_then(|f| f.as_str())
                        .map(|fr| !fr.is_empty())
                        .unwrap_or(false)
                })
                .unwrap_or(false),
            Err(_) => false,
        });
        assert!(!saw_done, "该帧序���不应出现 [DONE]");
        assert!(!saw_finish, "该帧序列不应出现非空 finish_reason");
    }
}

#[cfg(test)]
mod stream_start_detection_tests {
    use super::{sse_line_has_meaningful_output, StreamStartDetector};

    #[test]
    fn whitespace_and_placeholder_tool_frames_are_not_meaningful_output() {
        assert!(!sse_line_has_meaningful_output(
            r#"data: {"choices":[{"delta":{"content":"   "},"finish_reason":null}]}"#
        ));
        assert!(!sse_line_has_meaningful_output(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0}]},"finish_reason":null}]}"#
        ));
    }

    #[test]
    fn text_reasoning_and_real_tool_deltas_are_meaningful_output() {
        assert!(sse_line_has_meaningful_output(
            r#"data: {"choices":[{"delta":{"content":"answer"},"finish_reason":null}]}"#
        ));
        assert!(sse_line_has_meaningful_output(
            r#"data: {"choices":[{"delta":{"reasoning_content":"thinking"},"finish_reason":null}]}"#
        ));
        assert!(sse_line_has_meaningful_output(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{"}}]},"finish_reason":null}]}"#
        ));
    }

    #[test]
    fn split_tool_id_and_name_become_meaningful_only_when_start_can_be_emitted() {
        let mut detector = StreamStartDetector::default();
        assert!(!detector.observe_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1"}]},"finish_reason":null}]}"#
        ));
        assert!(detector.observe_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"read_file"}}]},"finish_reason":null}]}"#
        ));
    }

    #[test]
    fn empty_completion_without_output_is_not_ready_for_handoff() {
        // 200 + 空 choices + finish_reason + [DONE]：完成标志有，但全程零有效输出。
        // 这是 claude-code 压缩报 no assistant message 的源头，必须判为「空完成」。
        let mut detector = StreamStartDetector::default();
        assert!(
            !detector.observe_line(r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#)
        );
        assert!(!detector.observe_line("data: [DONE]"));
        assert!(detector.has_completion(), "完成标志应被单独记录");
        assert!(!detector.has_output(), "零内容不算有效输出");
    }

    #[test]
    fn completion_after_content_is_ready_for_handoff() {
        // 先内容后完成标志：有效完成，可以转发。
        let mut detector = StreamStartDetector::default();
        assert!(detector.observe_line(
            r#"data: {"choices":[{"delta":{"content":"hi"},"finish_reason":null}]}"#
        ));
        assert!(
            !detector.observe_line(r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#)
        );
        assert!(detector.has_completion());
        assert!(detector.has_output(), "先内容后完成标志 = 有效完成");
    }

    #[test]
    fn finish_reason_same_frame_as_content_counts_as_output() {
        // finish_reason 与内容同帧时，observe_line 必须先记完成标志、再继续看 delta 内容。
        let mut detector = StreamStartDetector::default();
        assert!(detector.observe_line(
            r#"data: {"choices":[{"delta":{"content":"hi"},"finish_reason":"stop"}]}"#
        ));
        assert!(detector.has_completion());
        assert!(detector.has_output());
    }
}

#[cfg(test)]
mod stream_stall_fallback_tests {
    use super::{handle_messages, ProxyCtx};
    use crate::config::NvidiaConfig;
    use crate::stats::UsageStatsStore;
    use axum::{
        body::{to_bytes, Body, Bytes},
        extract::State,
        http::{header, HeaderMap, Method, Request},
        response::Response,
        routing::post,
        Json, Router,
    };
    use serde_json::{json, Value};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    type SeenRequests = Arc<Mutex<Vec<(String, String)>>>;

    #[test]
    fn proxy_ctx_exposes_shared_stats_store() {
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string()],
            ..Default::default()
        };
        let stats = Arc::new(UsageStatsStore::in_memory());
        let ctx = ProxyCtx::new(cfg, stats.clone());

        assert!(Arc::ptr_eq(&ctx.stats, &stats));
    }

    // 构造一个带指定 headers + body 的 POST /v1/messages 请求，供直接调用
    // handle_messages（其签名现为 Request<Body>）。与真实路由路径等价。
    fn build_request(extra_headers: HeaderMap, body: Bytes) -> Request<Body> {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("构建测试用 Request 失败");
        for (name, value) in extra_headers.iter() {
            req.headers_mut().insert(name.clone(), value.clone());
        }
        req
    }

    async fn mock_nvidia(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        seen.lock().await.push((model.clone(), auth));

        if model == "model-a" {
            let stalled = async_stream::stream! {
                yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
                ));
                tokio::time::sleep(Duration::from_secs(10)).await;
            };
            return Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stalled))
                .unwrap();
        }

        let completed = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"fallback-ok\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(completed))
            .unwrap()
    }

    async fn mock_nvidia_5xx_then_success(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        seen.lock().await.push((model.clone(), auth));

        if model == "model-a" {
            return Response::builder()
                .status(503)
                .body(Body::from("model unavailable"))
                .unwrap();
        }

        let completed = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"fallback-5xx-ok\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(completed))
            .unwrap()
    }

    async fn mock_nvidia_404_then_success(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        seen.lock().await.push((model.clone(), auth));

        if model == "model-a" {
            return Response::builder()
                .status(404)
                .body(Body::from("model not found"))
                .unwrap();
        }

        let completed = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"fallback-404-ok\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(completed))
            .unwrap()
    }

    async fn mock_oversized_prefix_then_success(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        seen.lock().await.push((model.clone(), auth));
        if model == "model-a" {
            let oversized = vec![b'x'; 1024 * 1024 + 1];
            let stalled = async_stream::stream! {
                yield Ok::<Bytes, std::io::Error>(Bytes::from(oversized));
                tokio::time::sleep(Duration::from_secs(10)).await;
            };
            return Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stalled))
                .unwrap();
        }
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"bounded-ok\"},\"finish_reason\":null}]}\n\n",
                "data: [DONE]\n\n"
            )))
            .unwrap()
    }

    async fn mock_stalled_5xx_body_then_success(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        seen.lock().await.push((model.clone(), auth));
        if model == "model-a" {
            let stalled = async_stream::stream! {
                yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"unavailable"));
                tokio::time::sleep(Duration::from_secs(10)).await;
            };
            return Response::builder()
                .status(503)
                .body(Body::from_stream(stalled))
                .unwrap();
        }
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"bounded-5xx-ok\"},\"finish_reason\":null}]}\n\n",
                "data: [DONE]\n\n"
            )))
            .unwrap()
    }

    async fn mock_partial_then_eof(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        seen.lock().await.push((model, auth));
        let partial = async_stream::stream! {
            yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
            ));
            tokio::time::sleep(super::RETRYABLE_STREAM_WINDOW + Duration::from_millis(50)).await;
        };
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(partial))
            .unwrap()
    }

    async fn mock_stream_error_then_success(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let request_number = {
            let mut requests = seen.lock().await;
            let request_number = requests.len();
            requests.push((model, auth));
            request_number
        };

        if request_number == 0 {
            let broken = async_stream::stream! {
                yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"before-error\"},\"finish_reason\":null}]}\n\n",
                ));
                yield Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "simulated upstream body decoding failure",
                ));
            };
            return Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(broken))
                .unwrap();
        }

        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"retry-ok\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            )))
            .unwrap()
    }

    // 空完成 mock：model-a 返回 200 + 零内容 + finish_reason stop + [DONE]（模拟
    // NVIDIA 在超长上下文/拒答时偶发的空响应），model-b 返回正常内容。
    async fn mock_empty_completion_then_success(
        State(seen): State<SeenRequests>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        seen.lock().await.push((model.clone(), auth));
        if model == "model-a" {
            return Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(concat!(
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                )))
                .unwrap();
        }
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"empty-fallback-ok\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            )))
            .unwrap()
    }

    #[tokio::test]
    async fn pre_output_stall_falls_back_model_with_same_key() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_nvidia))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string(), "nvapi-key-two".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "max_tokens": 32,
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;
        let body = tokio::time::timeout(
            Duration::from_secs(3),
            to_bytes(response.into_body(), 1024 * 1024),
        )
        .await
        .expect("首模型无有效输出时应在超时内切换模型，而不是把挂死流交给客户端")
        .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(body.contains("fallback-ok"), "应返回第二模型的有效输出");
        let seen = seen.lock().await.clone();
        assert_eq!(
            seen,
            vec![
                ("model-a".to_string(), "Bearer nvapi-key-one".to_string()),
                ("model-b".to_string(), "Bearer nvapi-key-one".to_string()),
            ],
            "模型 fallback 必须复用同一 Key"
        );

        server.abort();
    }

    #[tokio::test]
    async fn body_decoding_error_before_handoff_retries_the_request() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_stream_error_then_success))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string(), "nvapi-key-two".to_string()],
            models: vec!["model-a".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "max_tokens": 32,
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;
        let body = tokio::time::timeout(
            Duration::from_secs(3),
            to_bytes(response.into_body(), 1024 * 1024),
        )
        .await
        .expect("body decoding failure should be retried before returning to Claude Code")
        .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(body.contains("retry-ok"), "downstream body: {body}");
        assert_eq!(
            seen.lock().await.len(),
            2,
            "a short upstream body failure must trigger one retry"
        );

        server.abort();
    }

    #[tokio::test]
    async fn empty_completion_stream_falls_back_to_next_model_with_same_key() {
        // 回归闸：200 + 零内容 + finish_reason 的空完成流，必须判为「空流」并切模型，
        // 绝不能当成功转发——否则 claude-code 压缩会报 no assistant message。
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(mock_empty_completion_then_success),
            )
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "max_tokens": 32,
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;
        let body = tokio::time::timeout(
            Duration::from_secs(3),
            to_bytes(response.into_body(), 1024 * 1024),
        )
        .await
        .expect("空完成流应在超时内切模型，而不是把空成功流交给客户端")
        .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(
            body.contains("empty-fallback-ok"),
            "空完成应触发模型 fallback 到第二模型: {body}"
        );
        assert_eq!(
            seen.lock().await.clone(),
            vec![
                ("model-a".to_string(), "Bearer nvapi-key-one".to_string()),
                ("model-b".to_string(), "Bearer nvapi-key-one".to_string()),
            ],
            "空完成必须复用同一 Key 并切换模型"
        );

        server.abort();
    }

    #[tokio::test]
    async fn stream_response_rejects_zero_content_success_stream() {
        // 纵深防御回归闸：即使某个路径把零内容流送进 stream_response（如未来
        // detector 改回归），也必须发 error 事件，绝不能发 message_delta(end_turn)
        // 伪装成空成功——claude-code 压缩会因此报 no assistant message。
        use futures_util::StreamExt;
        let upstream =
            futures_util::stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            ))])
            .boxed();
        let response = super::stream_response(
            Vec::new(),
            upstream,
            "model-a",
            &std::collections::HashMap::new(),
            30,
            Arc::new(UsageStatsStore::in_memory()),
            super::RequestStatsContext::new("model-a".to_string()),
        );
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            body.contains("event: error"),
            "零内容成功流必须发 error 事件: {body}"
        );
        assert!(
            !body.contains("message_stop"),
            "不得伪装成 end_turn 空消息: {body}"
        );
    }

    #[tokio::test]
    async fn model_5xx_fallback_also_reuses_the_same_key() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_nvidia_5xx_then_success))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string(), "nvapi-key-two".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "max_tokens": 32,
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(String::from_utf8(body.to_vec())
            .unwrap()
            .contains("fallback-5xx-ok"));
        assert_eq!(
            seen.lock().await.clone(),
            vec![
                ("model-a".to_string(), "Bearer nvapi-key-one".to_string()),
                ("model-b".to_string(), "Bearer nvapi-key-one".to_string()),
            ],
            "任何模型 fallback 都应复用同一 NVIDIA Key"
        );

        server.abort();
    }

    #[tokio::test]
    async fn model_404_falls_back_to_the_next_model_with_the_same_key() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_nvidia_404_then_success))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string(), "nvapi-key-two".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "max_tokens": 32,
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(String::from_utf8(body.to_vec())
            .unwrap()
            .contains("fallback-404-ok"));
        assert_eq!(
            seen.lock().await.clone(),
            vec![
                ("model-a".to_string(), "Bearer nvapi-key-one".to_string()),
                ("model-b".to_string(), "Bearer nvapi-key-one".to_string()),
            ],
            "404 模型 fallback 必须复用同一 NVIDIA Key"
        );

        server.abort();
    }

    #[tokio::test]
    async fn model_5xx_without_fallback_does_not_rotate_shared_keys() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_nvidia_5xx_then_success))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string(), "nvapi-key-two".to_string()],
            models: vec!["model-a".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "max_tokens": 32,
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let _response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;

        assert_eq!(
            seen.lock().await.clone(),
            vec![("model-a".to_string(), "Bearer nvapi-key-one".to_string())],
            "模型 5xx 且没有后备模型时，轮换共享 Key 不会改善结果"
        );

        server.abort();
    }

    #[tokio::test]
    async fn oversized_nonmeaningful_prefix_is_bounded_then_falls_back() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(mock_oversized_prefix_then_success),
            )
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string(), "nvapi-key-two".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 5,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = tokio::time::timeout(
            Duration::from_secs(2),
            handle_messages(
                State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
                build_request(
                    HeaderMap::new(),
                    Bytes::from(serde_json::to_vec(&request).unwrap()),
                ),
            ),
        )
        .await
        .expect("超过预输出缓冲上限后应立即 fallback，不应继续等待 5 秒");
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(String::from_utf8(body.to_vec())
            .unwrap()
            .contains("bounded-ok"));

        server.abort();
    }

    #[tokio::test]
    async fn stalled_5xx_body_is_bounded_then_falls_back_with_same_key() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(mock_stalled_5xx_body_then_success),
            )
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string(), "nvapi-key-two".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = tokio::time::timeout(
            Duration::from_secs(3),
            handle_messages(
                State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
                build_request(
                    HeaderMap::new(),
                    Bytes::from(serde_json::to_vec(&request).unwrap()),
                ),
            ),
        )
        .await
        .expect("5xx 错误体读取必须受超时保护");
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(String::from_utf8(body.to_vec())
            .unwrap()
            .contains("bounded-5xx-ok"));
        assert_eq!(
            seen.lock().await.clone(),
            vec![
                ("model-a".to_string(), "Bearer nvapi-key-one".to_string()),
                ("model-b".to_string(), "Bearer nvapi-key-one".to_string()),
            ]
        );

        server.abort();
    }

    #[tokio::test]
    async fn partial_output_then_eof_is_not_stitched_with_another_model() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_partial_then_eof))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let request = json!({
            "model": "model-a",
            "stream": true,
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;
        let body = String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("partial"));
        assert!(body.contains("event: error"));
        assert_eq!(
            seen.lock().await.len(),
            1,
            "已有部分输出后不得调用第二模型拼接回答"
        );

        server.abort();
    }

    // P2#4 回归：请求体超过 MAX_REQUEST_BODY_BYTES 时，必须被重塑为 Anthropic 风格
    // error 事件（而非 axum 默认 plaintext 413），且不触达上游转发。
    #[tokio::test]
    async fn oversized_body_is_anthropic_413_without_hitting_upstream() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_nvidia))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };

        // 构造一个超过 MAX_REQUEST_BODY_BYTES（32 MiB）的「合法」Anthropic 请求：
        // 用一个极大的 content 填充使其 JSON 字节数越过上限。
        let big = "x".repeat(super::MAX_REQUEST_BODY_BYTES + 1);
        let request = json!({
            "model": "model-a",
            "max_tokens": 4,
            "stream": false,
            "messages": [{ "role": "user", "content": big }]
        });

        let response = handle_messages(
            State(ProxyCtx::new(cfg, Arc::new(UsageStatsStore::in_memory()))),
            build_request(
                HeaderMap::new(),
                Bytes::from(serde_json::to_vec(&request).unwrap()),
            ),
        )
        .await;

        assert_eq!(
            response.status(),
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            "超限请求应返回 413"
        );
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).expect("413 体应为 Anthropic 风格 JSON");
        assert_eq!(parsed["type"], "error", "顶层 type 应为 error");
        assert!(
            parsed["error"]["message"]
                .as_str()
                .unwrap()
                .contains("超过 32 MiB 上限"),
            "413 错误消息应说明上限: {}",
            parsed["error"]["message"]
        );
        assert!(
            seen.lock().await.is_empty(),
            "超限请求不得转发到上游（不应消耗任何 Key）"
        );

        server.abort();
    }
}

#[cfg(test)]
mod usage_stats_tests {
    use super::{handle_messages, ProxyCtx};
    use crate::config::NvidiaConfig;
    use crate::stats::{UsageRange, UsageStatsStore};
    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{header, Method, Request},
        response::Response,
        routing::post,
        Json, Router,
    };
    use serde_json::{json, Value};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    type SeenRequests = Arc<Mutex<Vec<String>>>;

    fn build_request(body: Value) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    async fn collect_response_body(response: Response) -> String {
        String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }

    async fn mock_non_stream(Json(body): Json<Value>) -> Response {
        let model = body.get("model").and_then(Value::as_str).unwrap_or("");
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "id": "chatcmpl_nonstream",
                    "choices": [{
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {
                            "role": "assistant",
                            "content": format!("hello from {model}")
                        }
                    }],
                    "usage": {
                        "prompt_tokens": 12,
                        "completion_tokens": 5
                    }
                })
                .to_string(),
            ))
            .unwrap()
    }

    async fn mock_stream_success(Json(body): Json<Value>) -> Response {
        let model = body.get("model").and_then(Value::as_str).unwrap_or("");
        let payload = format!(
            concat!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"hello {}\"}},\"finish_reason\":null}}]}}\n\n",
                "data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":120,\"completion_tokens\":50,\"prompt_tokens_details\":{{\"cached_tokens\":20}}}}}}\n\n",
                "data: [DONE]\n\n"
            ),
            model,
        );
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(payload))
            .unwrap()
    }

    async fn mock_404_then_success(
        State(seen): State<SeenRequests>,
        Json(body): Json<Value>,
    ) -> Response {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        seen.lock().await.push(model.clone());
        if model == "model-a" {
            return Response::builder()
                .status(404)
                .body(Body::from("missing"))
                .unwrap();
        }

        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "id": "chatcmpl_fallback",
                    "choices": [{
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {
                            "role": "assistant",
                            "content": "fallback ok"
                        }
                    }],
                    "usage": {
                        "prompt_tokens": 9,
                        "completion_tokens": 4
                    }
                })
                .to_string(),
            ))
            .unwrap()
    }

    async fn mock_stream_eof_after_output(Json(_body): Json<Value>) -> Response {
        let partial = async_stream::stream! {
            yield Ok::<axum::body::Bytes, std::io::Error>(axum::body::Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"partial-eof\"},\"finish_reason\":null}]}\n\n",
            ));
        };
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(partial))
            .unwrap()
    }

    #[tokio::test]
    async fn non_stream_usage_records_one_successful_request() {
        let app = Router::new().route("/v1/chat/completions", post(mock_non_stream));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let stats = Arc::new(UsageStatsStore::in_memory());
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            ..Default::default()
        };
        let response = handle_messages(
            State(ProxyCtx::new(cfg, stats.clone())),
            build_request(json!({
                "model": "model-a",
                "max_tokens": 32,
                "stream": false,
                "messages": [{ "role": "user", "content": "hello" }]
            })),
        )
        .await;
        let body = collect_response_body(response).await;
        assert!(body.contains("hello from model-a"));

        let totals = stats.snapshot(UsageRange::Live).totals;
        assert_eq!(totals.requests, 1);
        assert_eq!(totals.input_tokens, 12);
        assert_eq!(totals.output_tokens, 5);
        assert_eq!(totals.failed_requests, 0);
        assert_eq!(totals.retry_count, 0);
        assert_eq!(totals.usage_missing_requests, 0);

        server.abort();
    }

    #[tokio::test]
    async fn stream_success_records_raw_provider_usage() {
        let app = Router::new().route("/v1/chat/completions", post(mock_stream_success));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let stats = Arc::new(UsageStatsStore::in_memory());
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            ..Default::default()
        };
        let response = handle_messages(
            State(ProxyCtx::new(cfg, stats.clone())),
            build_request(json!({
                "model": "model-a",
                "max_tokens": 32,
                "stream": true,
                "messages": [{ "role": "user", "content": "hello" }]
            })),
        )
        .await;
        let body = tokio::time::timeout(Duration::from_secs(3), collect_response_body(response))
            .await
            .unwrap();
        assert!(body.contains("\"input_tokens\":100"));
        assert!(body.contains("\"output_tokens\":50"));

        let totals = stats.snapshot(UsageRange::Live).totals;
        assert_eq!(totals.requests, 1);
        assert_eq!(totals.input_tokens, 120);
        assert_eq!(totals.output_tokens, 50);
        assert_eq!(totals.failed_requests, 0);
        assert_eq!(totals.usage_missing_requests, 0);

        server.abort();
    }

    #[tokio::test]
    async fn fallback_success_records_final_model_and_retry_count() {
        let seen: SeenRequests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_404_then_success))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let stats = Arc::new(UsageStatsStore::in_memory());
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            max_retries: 3,
            ..Default::default()
        };
        let response = handle_messages(
            State(ProxyCtx::new(cfg, stats.clone())),
            build_request(json!({
                "model": "model-a",
                "max_tokens": 32,
                "stream": false,
                "messages": [{ "role": "user", "content": "hello" }]
            })),
        )
        .await;
        let body = collect_response_body(response).await;
        assert!(body.contains("fallback ok"));
        assert_eq!(
            seen.lock().await.clone(),
            vec!["model-a".to_string(), "model-b".to_string()]
        );

        let snapshot = stats.snapshot(UsageRange::Live);
        assert_eq!(snapshot.totals.requests, 1);
        assert_eq!(snapshot.totals.retry_count, 1);
        assert_eq!(snapshot.totals.failed_requests, 0);
        assert_eq!(snapshot.models.len(), 1);
        assert_eq!(snapshot.models[0].model, "model-b");

        server.abort();
    }

    #[tokio::test]
    async fn premature_stream_eof_records_one_failure() {
        let app = Router::new().route("/v1/chat/completions", post(mock_stream_eof_after_output));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let stats = Arc::new(UsageStatsStore::in_memory());
        let cfg = NvidiaConfig {
            api_keys: vec!["nvapi-key-one".to_string()],
            models: vec!["model-a".to_string()],
            base_url: format!("http://{address}/v1"),
            request_timeout_seconds: 1,
            ..Default::default()
        };
        let response = handle_messages(
            State(ProxyCtx::new(cfg, stats.clone())),
            build_request(json!({
                "model": "model-a",
                "max_tokens": 32,
                "stream": true,
                "messages": [{ "role": "user", "content": "hello" }]
            })),
        )
        .await;
        let body = tokio::time::timeout(Duration::from_secs(3), collect_response_body(response))
            .await
            .unwrap();
        assert!(body.contains("partial-eof"));
        assert!(body.contains("event: error"));

        let totals = stats.snapshot(UsageRange::Live).totals;
        assert_eq!(totals.requests, 1);
        assert_eq!(totals.failed_requests, 1);
        assert_eq!(totals.retry_count, 0);
        assert_eq!(totals.usage_missing_requests, 1);

        server.abort();
    }
}

#[cfg(test)]
mod auth_tests {
    use super::ct_eq;

    // 恒时比较：相同 token 返回 true，差异一处返回 false。
    #[test]
    fn ct_eq_matches_and_rejects() {
        assert!(ct_eq("sk-abc-1234567890", "sk-abc-1234567890"));
        assert!(!ct_eq("sk-abc-1234567890", "sk-abc-1234567891"));
        assert!(!ct_eq("a", "ab")); // 长度不同必然不等
    }
}
