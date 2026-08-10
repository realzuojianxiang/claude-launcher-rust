// 8084 OpenAI 协议透传网关（OpenAI 入站 ↔ OpenAI 协议上游）。
//
// 与 8083（Anthropic 入站 ↔ OpenAI 上游）相对：8084 不做协议转换，只做「透传 + 统计」。
// 入站直接是 OpenAI Chat Completions / Responses 协议，原样转发到 provider 配置的上游，
// 再从响应体抽取 usage 写进与 8083 共享的 UsageStatsStore。
//
// 设计要点：
//   - 复用 gateway 的 ProviderEntry / GatewayConfig（与 8083 同一份 providers 配置）
//   - 复用 gateway::auth::ApiKeyAuthProvider（Key 轮询 + 429 冷却）
//   - 复用 gateway::types::{RequestStatsContext, record_usage_safely, ct_eq, err_response, read_error_body_limited, bearer_key_from}
//   - 按请求体 model 字段路由到对应 provider（命中某 provider 的 models 列表则用该 provider，
//     否则回落到 active provider，model 名原样透传给上游）
//   - 非流式：读 JSON 响应抽 usage；流式：透传 SSE 字节流并扫描 data: 行抽 usage
//   - 为拿到流式 chat/completions 的 usage，自动为流式请求注入 stream_options.include_usage=true
//     （OpenAI 兼容端点支持，不影响语义）

use crate::gateway::auth::AuthProvider;
use crate::gateway::config::{self, GatewayConfig, ProviderEntry};
use crate::gateway::types::{self, RequestStatsContext};
use crate::shared::{split_complete_sse_lines, MAX_REQUEST_BODY_BYTES};
use crate::stats::UsageStatsStore;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{stream::BoxStream, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// 8084 代理运行时上下文：共享配置 + 每 provider 的认证 + 统计 store + HTTP 客户端。
pub struct ProxyCtx {
    pub config: GatewayConfig,
    pub auth_map: HashMap<String, Arc<dyn AuthProvider>>,
    pub stats: Arc<UsageStatsStore>,
    pub client: reqwest::Client,
}

impl ProxyCtx {
    pub fn new(
        config: GatewayConfig,
        auth_map: HashMap<String, Arc<dyn AuthProvider>>,
        stats: Arc<UsageStatsStore>,
    ) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Arc::new(Self {
            config,
            auth_map,
            stats,
            client,
        })
    }

    /// 供 OpenAiGatewayState::pool_status 取脱敏快照：聚合所有 provider 的 Key 池状态。
    pub fn pool_snapshot(&self) -> Value {
        let mut providers = serde_json::Map::new();
        for (id, auth) in &self.auth_map {
            providers.insert(id.clone(), auth.snapshot());
        }
        serde_json::json!({
            "providers": providers,
            "total": self.auth_map.len(),
        })
    }
}

/// 按 model 解析目标 provider：优先命中某 provider 的 models 列表，否则回落 active。
fn resolve_provider<'a>(cfg: &'a GatewayConfig, model: &str) -> Option<&'a ProviderEntry> {
    for p in &cfg.providers {
        if p.models.iter().any(|m| m.eq_ignore_ascii_case(model)) {
            return Some(p);
        }
    }
    cfg.active()
}

/// 从 usage 对象抽取 (input, output) tokens，兼容 chat(prompt/completion) 与
/// responses(input/output) 两套字段命名。
fn extract_usage(usage: Option<&Value>) -> (u64, u64) {
    let Some(u) = usage else { return (0, 0) };
    let in_tok = u
        .get("input_tokens")
        .or_else(|| u.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let out_tok = u
        .get("output_tokens")
        .or_else(|| u.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (in_tok, out_tok)
}

/// 为流式 chat/completions 请求注入 stream_options.include_usage=true，
/// 保证上游在末帧带上 usage（OpenAI 兼容端点支持，Responses API 默认带，不处理）。
fn inject_usage(req: &Value) -> (Vec<u8>, bool) {
    let is_stream = req.get("stream").and_then(Value::as_bool).unwrap_or(false);
    if !is_stream {
        return (serde_json::to_vec(req).unwrap_or_default(), false);
    }
    let mut v = req.clone();
    if v.get("stream_options").is_none() {
        v["stream_options"] = Value::Object(serde_json::Map::new());
    }
    if let Some(so) = v.get_mut("stream_options").and_then(|x| x.as_object_mut()) {
        so.insert("include_usage".to_string(), Value::Bool(true));
    }
    (
        serde_json::to_vec(&v).unwrap_or_else(|_| serde_json::to_vec(req).unwrap_or_default()),
        true,
    )
}

// axum handler：同时服务 /v1/chat/completions 与 /v1/responses（按 path 判定上游后缀）。
pub async fn handle_openai(
    State(ctx): State<Arc<ProxyCtx>>,
    request: axum::http::Request<Body>,
) -> Response {
    let path = request.uri().path().to_string();
    let is_responses = path.contains("/responses");
    let suffix = if is_responses {
        "/responses"
    } else {
        "/chat/completions"
    };

    // 1. 读请求体
    let (_parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            return types::err_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!(
                    "请求体超过 {} MiB 上限，请缩减请求后重试。",
                    MAX_REQUEST_BODY_BYTES / (1024 * 1024)
                ),
            )
        }
    };

    // 2. 解析 JSON 取 model
    let req_val: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            return types::err_response(
                StatusCode::BAD_REQUEST,
                &format!("请求体 JSON 解析失败: {e}"),
            )
        }
    };
    let model = req_val
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if model.trim().is_empty() {
        return types::err_response(StatusCode::BAD_REQUEST, "缺少 model 字段");
    }

    // 3. 解析目标 provider
    let provider = match resolve_provider(&ctx.config, &model) {
        Some(p) => p,
        None => return types::err_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "未找到可服务该 model 的 provider，请检查「协议网关」页面是否已添加含该模型的 provider",
        ),
    };

    // 4. 流式判定 + 注入 usage（仅 chat 流式需要）
    let stream = req_val
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (final_body, final_stream) = if stream && !is_responses {
        inject_usage(&req_val)
    } else {
        (bytes.to_vec(), stream)
    };

    // 5. 上游 URL
    let base = config::effective_base_url(provider).trim_end_matches('/');
    let url = format!("{base}{suffix}");
    let timeout = std::time::Duration::from_secs(provider.request_timeout_seconds);
    let max = provider.max_retries.max(1) as usize;

    let mut attempts = 0usize;
    let mut record_ctx = RequestStatsContext::new(&provider.id, model.clone());
    let mut last_err = "上游未返回可用响应".to_string();

    loop {
        if attempts >= max {
            break;
        }
        attempts += 1;
        record_ctx.begin_attempt(&model, attempts);

        let auth = match ctx.auth_map.get(&provider.id) {
            Some(a) => a,
            None => {
                return types::err_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "内部错误：找不到该 provider 的认证配置",
                )
            }
        };
        let auth_headers = match auth.auth_headers() {
            Ok(h) => h,
            Err(e) => {
                last_err = e;
                break;
            }
        };

        let mut builder = ctx
            .client
            .post(&url)
            .headers(auth_headers.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT_ENCODING, "identity")
            .header(
                header::ACCEPT,
                if final_stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .header(header::CONNECTION, "keep-alive")
            .body(final_body.clone());
        if !final_stream {
            builder = builder.timeout(timeout);
        }

        let resp = match tokio::time::timeout(timeout, builder.send()).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                tracing::warn!(attempt = attempts, error = %e, "上游网络错误，切换重试");
                last_err = format!("上游网络错误: {e}");
                continue;
            }
            Err(_) => {
                tracing::warn!(
                    attempt = attempts,
                    timeout_secs = provider.request_timeout_seconds,
                    "上游响应头超时，切换重试"
                );
                last_err = format!("上游 {} 秒内未返回响应头", provider.request_timeout_seconds);
                continue;
            }
        };

        let status = resp.status();

        // 401：不可恢复，直接报上游错误
        if status == StatusCode::UNAUTHORIZED {
            types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
            let text = types::read_error_body_limited(resp, timeout).await;
            return types::err_response(
                StatusCode::UNAUTHORIZED,
                &format!(
                    "上游 401：凭证无效或已失效，请在「协议网关」页面检查/重新配置 Key。{text}"
                ),
            );
        }

        // 429：冷却当前 Key 并重试
        if status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(pool) = auth.key_pool_opt() {
                if let Some(key) = types::bearer_key_from(&auth_headers) {
                    pool.lock().unwrap().cooldown(&key);
                }
            }
            last_err = "上游 429 限流，已冷却凭证并切换重试".to_string();
            continue;
        }

        // 5xx：重试
        if status.is_server_error() {
            last_err = format!("上游错误 {}", status.as_u16());
            continue;
        }

        // 3xx：禁止跟随以防 bearer 泄漏
        if status.is_redirection() {
            types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
            return types::err_response(
                StatusCode::BAD_GATEWAY,
                "上游返回重定向，已禁止跟随以避免凭证泄漏。请检查 provider Base URL 是否指向官方端点。",
            );
        }

        // 其余 4xx：直传
        if !status.is_success() {
            let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let text = types::read_error_body_limited(resp, timeout).await;
            types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
            return types::err_response(code, &format!("上游错误 {code}: {text}"));
        }

        // 成功：按 stream 分流
        tracing::info!(attempt = attempts, provider = %provider.id, model = %model, "上游成功");
        if final_stream {
            let upstream = resp.bytes_stream().boxed();
            return stream_passthrough(upstream, ctx.stats.clone(), record_ctx);
        } else {
            return non_stream_response(resp, ctx.stats.clone(), record_ctx).await;
        }
    }

    types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
    types::err_response(
        StatusCode::BAD_GATEWAY,
        &format!("共尝试 {attempts} 次仍失败：{last_err}"),
    )
}

/// 非流式：读上游 JSON → 抽 usage → 记录 → 原样返回。
async fn non_stream_response(
    resp: reqwest::Response,
    stats: Arc<UsageStatsStore>,
    record_ctx: RequestStatsContext,
) -> Response {
    let upstream: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            types::record_usage_safely(&stats, record_ctx.failure_record());
            return types::err_response(StatusCode::BAD_GATEWAY, &format!("上游响应解析失败: {e}"));
        }
    };
    let usage = upstream.get("usage");
    let (in_tok, out_tok) = extract_usage(usage);
    types::record_usage_safely(
        &stats,
        record_ctx.success_record(in_tok, out_tok, usage.is_some()),
    );
    axum::Json(upstream).into_response()
}

/// 流式：透传 SSE 字节流，扫描 data: 行抽 usage，流结束后记录统计。
fn stream_passthrough(
    upstream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    stats: Arc<UsageStatsStore>,
    record_ctx: RequestStatsContext,
) -> Response {
    let sse = async_stream::stream! {
        let mut buf: Vec<u8> = Vec::with_capacity(8192);
        let mut in_tok: u64 = 0;
        let mut out_tok: u64 = 0;
        let mut usage_available = false;
        let mut upstream = upstream;

        loop {
            match upstream.next().await {
                Some(Ok(chunk)) => {
                    buf.extend_from_slice(&chunk);
                    for line in split_complete_sse_lines(&mut buf).into_iter().flatten() {
                        let Some(data) = line.strip_prefix("data:") else { continue };
                        let data = data.trim();
                        if data == "[DONE]" {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<Value>(data) {
                            if let Some(u) = v.get("usage") {
                                let (i, o) = extract_usage(Some(u));
                                in_tok = i;
                                out_tok = o;
                                usage_available = true;
                            }
                        }
                    }
                    yield Ok::<Bytes, std::io::Error>(chunk);
                }
                Some(Err(e)) => {
                    tracing::error!(error = %e, "读取上游流失败");
                    break;
                }
                None => break,
            }
        }

        types::record_usage_safely(
            &stats,
            record_ctx.success_record(
                if usage_available { in_tok } else { 0 },
                if usage_available { out_tok } else { 0 },
                usage_available,
            ),
        );
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
