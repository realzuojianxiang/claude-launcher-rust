// 协议网关核心抽象：UpstreamProtocol trait。
//
// 8083 端口作为通用「Anthropic 入站 ↔ OpenAI 协议上游」网关，当前支持 Chat
// Completions 一种上游协议。deepseek / glm / qwen / 任意 OpenAI 兼容端点各自挂载到
// 该协议，共享同一套代理重试/冷却/鉴权骨架，仅在「请求体构造」「上游路径」
// 「响应解析」「SSE 流处理」这四个协议差异点分叉——本 trait 封装这四个差异点。
//
// 设计取舍：trait 在更上层把整条「上游响应 → Anthropic SSE 字节流」的处理流水线
// （含 fallback 首段探测）包成 trait 方法。Chat 协议的复杂首段探测逻辑留在自己
// 实现里，协议差异点互不污染。

use crate::nvidia::models::AnthropicRequest;
use axum::body::Bytes;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{stream::BoxStream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::stats::{UsageRecord, UsageStatsStore};

// ===== 协议差异点 trait =====

/// 一个上游协议的完整实现：四个差异点的封装。
///
/// 实现者负责：
///   1. `upstream_path`：上游端点路径（当前为 `/chat/completions`）
///   2. `build_request`：Anthropic Messages 请求体 → 上游协议请求体
///   3. `parse_non_stream`：上游非流式 JSON 响应 → Anthropic Messages JSON
///   4. `stream_response`：上游 SSE 字节流 → Anthropic SSE Response（含 fallback 探测）
///
/// 生命周期：trait 对象 + Send + Sync，在 ProxyCtx 构造时由 ProviderEntry 决定具体实现，
/// 整个代理运行期不变。
pub trait UpstreamProtocol: Send + Sync {
    /// 上游端点路径（不含 base_url 的 host 部分）。
    /// Chat Completions 协议返回 `/chat/completions`。
    fn upstream_path(&self) -> &'static str;

    /// 把 Anthropic Messages 请求体转换成上游协议请求体。
    /// `model` 为已映射好的上游模型 slug；`stream` 透传流式标志。
    fn build_request(&self, req: &AnthropicRequest, model: &str, stream: bool) -> Value;

    /// 把上游非流式 JSON 响应转换成 Anthropic Messages JSON。
    /// `model` 为对外展示的模型名（Anthropic 侧入站 model）；`msg_id` 为本地生成的 msg_ id。
    fn parse_non_stream(&self, upstream: &Value, model: &str, msg_id: &str) -> Value;

    /// 把上游 SSE 字节流转换成 Anthropic SSE Response，返回给客户端。
    ///
    /// 各协议的流式处理流水线差异极大（Chat 协议有「等首段有效输出才决定 fallback」机制，
    /// 见 needs_stream_start_probe），故整条流水线由实现者各自封装，trait 只定入口签名。
    ///
    /// 入参：
    ///   - `upstream`：上游响应的字节流（已从 reqwest::Response 取出 bytes_stream）
    ///   - `model`：对外展示的模型名
    ///   - `idle_timeout`：逐 chunk 空闲超时
    ///   - `stats`：用量统计存储
    ///   - `record_ctx`：本请求的统计上下文（含 requested_model / attempts）
    ///   - `tool_map`：工具名大小写还原映射（从 Anthropic 请求的 tools 提取）
    ///
    /// 返回：构造好的 axum Response（SSE text/event-stream）。
    ///
    /// 注意：部分协议实现（如 nvidia Chat）需要额外的 `buffered_chunks`（首段探测已
    /// 消费但需重放的 chunk）。本 trait 方法不接收 buffered_chunks——那些协议在代理层
    /// 的 `handle_messages` 重试循环里自行处理首段探测后再调用本方法。实现者应在内部
    /// 完整管理自己的流处理状态机。
    fn stream_response(
        &self,
        upstream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
        model: &str,
        idle_timeout: std::time::Duration,
        stats: Arc<UsageStatsStore>,
        record_ctx: RequestStatsContext,
        tool_map: HashMap<String, String>,
    ) -> Response;

    /// 该协议是否需要「等首段有效输出才决定 fallback」的探测机制。
    /// Chat Completions 协议返回 true（首段无输出可切模型重试）。
    /// 代理层的重试循环据此决定是否在 stream 分流前插入首段探测。
    fn needs_stream_start_probe(&self) -> bool {
        false
    }
}

// ===== 请求统计上下文（provider 字段改为 name） =====

/// 单次请求的统计上下文：跟踪 requested_model / last_model / attempts，
/// 成功或失败时构造对应的 UsageRecord。
#[derive(Clone, Debug)]
pub struct RequestStatsContext {
    pub provider: String,
    pub requested_model: String,
    pub last_model: String,
    pub attempts: usize,
}

impl RequestStatsContext {
    pub fn new(provider: &str, requested_model: String) -> Self {
        Self {
            provider: provider.to_string(),
            last_model: requested_model.clone(),
            requested_model,
            attempts: 0,
        }
    }

    pub fn begin_attempt(&mut self, model: &str, attempts: usize) {
        self.last_model = model.to_string();
        self.attempts = attempts;
    }

    pub fn success_record(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        usage_available: bool,
    ) -> UsageRecord {
        UsageRecord {
            provider: self.provider.clone(),
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

    pub fn failure_record(&self) -> UsageRecord {
        UsageRecord {
            provider: self.provider.clone(),
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

/// 安全记录用量：失败仅 warn 不影响主流程。
pub fn record_usage_safely(store: &UsageStatsStore, record: UsageRecord) {
    if let Err(error) = store.record(record) {
        tracing::warn!(error = %error, "failed to persist usage statistics");
    }
}

/// 恒时字节比较（constant-time compare）：避免计时侧信道下的 token 逐字节泄露。
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 构造 Anthropic 风格错误响应（各协议共用）。
pub fn err_response(status: StatusCode, msg: &str) -> Response {
    let body = json!({
        "type": "error",
        "error": { "type": "proxy_error", "message": msg }
    });
    (status, axum::Json(body)).into_response()
}

/// 读取上游错误响应体（带上限与超时，各协议共用）。
pub async fn read_error_body_limited(
    resp: reqwest::Response,
    timeout: std::time::Duration,
) -> String {
    const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
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

/// 从「已选好、即随请求发出」的鉴权头集里反解本轮实际用的 Bearer key/token。
/// 用于 429 冷却——必须从这份已发送头取，而不是再调 auth_headers 触发一次 pick。
pub fn bearer_key_from(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let bearer = headers.get(reqwest::header::AUTHORIZATION)?.to_str().ok()?;
    bearer.strip_prefix("Bearer ").map(|s| s.to_string())
}

// 让 `header::ACCEPT` 等常量在使用了本模块的文件里可用：
// 上面已 `use axum::http::{header, StatusCode}`，使用本模块的文件应直接
// `use axum::http::header` 或通过完整路径引用，这里不再重复 re-export。
