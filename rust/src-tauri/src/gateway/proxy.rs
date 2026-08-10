// 协议网关代理核心：接收 Anthropic /v1/messages，按 provider 的 protocol 类型派发到
// 对应的 UpstreamProtocol 实现，转发到上游后响应再转回 Anthropic Messages。
//
// 与上游代理同源的设计约束（现已独立维护，不再依赖 grok 模块）：
//   - redirect::Policy::none() + 3xx 判 502（防 SSRF + bearer 外泄）
//   - 429 冷却切换 Key（auth_provider.key_pool_opt）
//   - 5xx / 404 切模型；网络错误 / 超时同模型重试
//   - 4xx 直传上游错误
//   - 32 MiB 请求体上限重塑为 Anthropic error
//   - 非回环强鉴权、请求头身份恒时比较
//
// 泛化点：
//   - ProviderEntry 携带 protocol 字段，运行时构造对应 UpstreamProtocol 实现
//   - 上游 URL：由 protocol.upstream_path() 动态取（当前仅 /chat/completions）
//   - 请求体转换：protocol.build_request()
//   - 响应解析：protocol.parse_non_stream()
//   - 流式分流：直接调 protocol.stream_response（Chat 协议内部含首段探测）
//   - 鉴权头：统一用 API Key 实现（gateway::auth）
//   - 模型映射 / base_url：config::map_model() / config::effective_base_url()

use crate::gateway::auth::{AuthProvider, RefreshOutcome};
use crate::gateway::config::{self, ProviderEntry};
use crate::gateway::protocol::chat::{
    wait_for_meaningful_stream_start, ChatCompletionsProtocol, StreamStart,
};
use crate::gateway::types::{self, RequestStatsContext, UpstreamProtocol};
use crate::nvidia::converter;
use crate::nvidia::models::AnthropicRequest;
use crate::shared::MAX_REQUEST_BODY_BYTES;
use crate::stats::UsageStatsStore;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;

/// 代理运行时上下文：provider 配置 + 复用异步 HTTP 客户端 + 可插拔认证 + 热更新模型。
pub struct ProxyCtx {
    pub provider: ProviderEntry,
    pub client: reqwest::Client,
    pub stats: Arc<UsageStatsStore>,
    pub auth_provider: Arc<dyn AuthProvider>,
    pub models: Arc<std::sync::RwLock<Vec<String>>>,
    pub protocol: Arc<dyn UpstreamProtocol>,
}

impl ProxyCtx {
    pub fn new(
        provider: ProviderEntry,
        auth_provider: Arc<dyn AuthProvider>,
        stats: Arc<UsageStatsStore>,
    ) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        // 当前仅支持 Chat Completions 协议，直接构造对应实现。
        let protocol: Arc<dyn UpstreamProtocol> = Arc::new(ChatCompletionsProtocol);
        let models = Arc::new(std::sync::RwLock::new(provider.models.clone()));
        Arc::new(Self {
            provider,
            client,
            stats,
            auth_provider,
            models,
            protocol,
        })
    }

    /// 供 GatewayState::pool_status 取脱敏快照。
    pub fn pool_snapshot(&self) -> Value {
        self.auth_provider.snapshot()
    }
}

// axum handler：POST /v1/messages
pub async fn handle_messages(
    State(ctx): State<Arc<ProxyCtx>>,
    request: axum::http::Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;

    // 1. 本地代理鉴权
    if !ctx.provider.auth_token.is_empty() {
        let provided = headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !types::ct_eq(provided, &ctx.provider.auth_token) {
            return types::err_response(StatusCode::UNAUTHORIZED, "无效的 x-api-key");
        }
    }

    // 2. 读请求体
    let body: Bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            return types::err_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!(
                    "请求体超过 {} MiB 上限，请缩减请求后重试。",
                    MAX_REQUEST_BODY_BYTES / (1024 * 1024)
                ),
            );
        }
    };

    // 3. 解析 Anthropic 请求体
    let req: AnthropicRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return types::err_response(StatusCode::BAD_REQUEST, &format!("请求体解析失败: {e}"))
        }
    };

    // 4. 模型优先级快照
    let live_models: Vec<String> = ctx
        .models
        .read()
        .map(|m| m.clone())
        .unwrap_or_else(|_| ctx.provider.models.clone());
    if live_models.is_empty() {
        return types::err_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &format!("未配置任何 {} 模型", ctx.provider.name),
        );
    }

    // 5. 模型映射 + Fallback 链
    let requested_model = req
        .model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(&live_models[0])
        .to_string();
    let stream = req.is_stream();
    let base_model = config::map_model(
        &ctx.provider,
        req.model
            .as_deref()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(&live_models[0]),
    );
    let mut models_chain: Vec<String> = vec![base_model.clone()];
    for m in &live_models {
        if !models_chain.iter().any(|x| x.eq_ignore_ascii_case(m)) {
            models_chain.push(m.clone());
        }
    }

    // 6. 重试循环
    //
    // 预算 = max_retries × 模型数：保证即使 provider 模型数 > max_retries，末尾模型也能被轮到。
    // 5xx / 404 触发切模型；网络错误 / 超时 / 429 在同模型内重试，由预算上限兜底。
    let model_count = models_chain.len();
    let max_retries = ctx.provider.max_retries.max(1) as usize;
    let budget = (max_retries * model_count).max(1);
    let url = format!(
        "{}{}",
        config::effective_base_url(&ctx.provider).trim_end_matches('/'),
        ctx.protocol.upstream_path()
    );
    let timeout = std::time::Duration::from_secs(ctx.provider.request_timeout_seconds);
    let display_model = req.model.clone().unwrap_or_else(|| base_model.clone());

    let mut model_idx = 0usize;
    let mut attempts = 0usize;
    let mut last_msg = "上游未返回可用响应".to_string();
    let mut refreshed_once = false;
    let mut record_ctx = RequestStatsContext::new(&ctx.provider.id, requested_model.clone());

    loop {
        if attempts >= budget {
            break;
        }
        attempts += 1;
        let model = models_chain[model_idx % model_count].clone();
        record_ctx.begin_attempt(&model, attempts);

        // 6a. 取鉴权头
        let auth_headers = match ctx.auth_provider.auth_headers() {
            Ok(h) => h,
            Err(e) => {
                last_msg = e;
                break;
            }
        };

        // 6b. 请求体转换：按协议派发
        let upstream_body = ctx.protocol.build_request(&req, &model, stream);

        tracing::info!(
            attempt = attempts,
            model = %model,
            display_model = %display_model,
            protocol = ?ctx.provider.protocol,
            "转发请求到 {} 上游 {}",
            ctx.provider.name,
            ctx.protocol.upstream_path()
        );

        let mut builder = ctx
            .client
            .post(&url)
            .headers(auth_headers.clone())
            .header(header::ACCEPT_ENCODING, "identity")
            .header(
                header::ACCEPT,
                if stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .header(header::CONNECTION, "keep-alive")
            .json(&upstream_body);
        if !stream {
            builder = builder.timeout(timeout);
        }

        let send_fut = builder.send();
        let resp = match tokio::time::timeout(timeout, send_fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                tracing::warn!(attempt = attempts, error = %e, "上游网络错误，切换重试");
                last_msg = format!("上游网络错误: {e}");
                continue;
            }
            Err(_) => {
                tracing::warn!(
                    attempt = attempts,
                    timeout_secs = ctx.provider.request_timeout_seconds,
                    "上游响应头超时，切换重试"
                );
                last_msg = format!(
                    "上游 {} 秒内未返回响应头",
                    ctx.provider.request_timeout_seconds
                );
                continue;
            }
        };

        let status = resp.status();

        // 6c. 401：refresh 一次重试
        if status == StatusCode::UNAUTHORIZED && !refreshed_once {
            refreshed_once = true;
            match ctx.auth_provider.on_401().await {
                RefreshOutcome::Refreshed => {
                    tracing::warn!(attempt = attempts, "上游 401，已刷新凭证重试");
                    last_msg = "上游 401，已刷新凭证重试".to_string();
                    continue;
                }
                RefreshOutcome::Unrecoverable => {
                    let text = types::read_error_body_limited(resp, timeout).await;
                    types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
                    tracing::warn!(attempt = attempts, body = %text, "上游 401 不可恢复");
                    return types::err_response(
                        StatusCode::UNAUTHORIZED,
                        &format!("上游 401：凭证无效或已失效，请在设置中检查/重新授权。{text}"),
                    );
                }
            }
        }

        // 6d. 429：冷却 Key
        if status == StatusCode::TOO_MANY_REQUESTS {
            {
                if let Some(pool) = ctx.auth_provider.key_pool_opt() {
                    if let Some(key) = types::bearer_key_from(&auth_headers) {
                        let mut pool = pool.lock().unwrap();
                        pool.cooldown(&key);
                    }
                }
            }
            let text = types::read_error_body_limited(resp, timeout).await;
            tracing::warn!(attempt = attempts, status = 429, "收到 429，冷却凭证并重试");
            last_msg = format!("上游 429 限流，已冷却凭证并切换重试: {text}");
            continue;
        }

        // 6e. 5xx：切模型（末尾模型则同模型重试，由预算兜底）
        if status.is_server_error() {
            let text = types::read_error_body_limited(resp, timeout).await;
            tracing::warn!(attempt = attempts, model = %model, status = %status.as_u16(), "上游 5xx，切换模型重试");
            last_msg = format!("上游错误 {}: {}", status.as_u16(), text);
            if model_idx < model_count - 1 {
                model_idx += 1;
            }
            // 末尾模型：停留在原地，下一轮同模型重试（预算上限兜底）
            continue;
        }

        // 6f. 404：切模型（末尾模型则终止，404 重试无意义）
        if status == StatusCode::NOT_FOUND && model_idx < model_count - 1 {
            let text = types::read_error_body_limited(resp, timeout).await;
            tracing::warn!(attempt = attempts, model = %model, status = 404, "上游 404，切换备选模型");
            last_msg = format!("模型 {model} 返回 404: {text}");
            model_idx += 1;
            continue;
        }

        // 6g. 3xx：判 502
        if status.is_redirection() {
            let text = types::read_error_body_limited(resp, timeout).await;
            types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
            tracing::error!(model = %model, status = %status.as_u16(), body = %text, "上游返回重定向，中止以防 bearer 泄漏");
            return types::err_response(
                StatusCode::BAD_GATEWAY,
                &format!(
                    "上游返回重定向 {}：已禁止跟随以避免凭证泄漏。请检查 {} Base URL 是否指向官方端点。",
                    status.as_u16(),
                    ctx.provider.name
                ),
            );
        }

        // 6h. 其余 4xx：直传
        if !status.is_success() {
            let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let text = types::read_error_body_limited(resp, timeout).await;
            types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
            tracing::warn!(model = %model, status = %code, body = %text, "上游返回客户端错误");
            return types::err_response(code, &format!("上游错误 {code}: {text}"));
        }

        // 6i. 成功：按 stream 分流
        tracing::info!(attempt = attempts, model = %model, "上游成功");
        if stream {
            let mut upstream = resp.bytes_stream().boxed();
            if ctx.protocol.needs_stream_start_probe() {
                match wait_for_meaningful_stream_start(&mut upstream, timeout).await {
                    StreamStart::Ready(buffered) => {
                        let replayed: futures_util::stream::BoxStream<
                            'static,
                            Result<Bytes, reqwest::Error>,
                        > = futures_util::stream::iter(
                            buffered
                                .into_iter()
                                .map(|b: Bytes| Ok::<Bytes, reqwest::Error>(b)),
                        )
                        .chain(upstream)
                        .boxed();
                        let tool_map = converter::build_tool_name_map(&req.tools);
                        return ctx.protocol.stream_response(
                            replayed,
                            &display_model,
                            timeout,
                            ctx.stats.clone(),
                            record_ctx,
                            tool_map,
                        );
                    }
                    StreamStart::Idle => {
                        last_msg = "流式首段空闲超时（上游未在规定时间内产出有效输出）".to_string();
                    }
                    StreamStart::Ended => {
                        last_msg = "上游流在产出有效输出前关闭".to_string();
                    }
                    StreamStart::Failed(e) => {
                        last_msg = format!("读取上游流失败: {e}");
                    }
                    StreamStart::BufferLimitExceeded => {
                        last_msg = "流式首段缓冲超过上限（上游持续无有效输出）".to_string();
                    }
                }
                tracing::warn!(
                    attempt = attempts,
                    "流式首段探测失败，按可重试错误处理：{last_msg}"
                );
                if model_idx < model_count - 1 {
                    model_idx += 1;
                }
                // 末尾模型：停留原地，由预算上限兜底重试
                continue;
            } else {
                let tool_map = converter::build_tool_name_map(&req.tools);
                return ctx.protocol.stream_response(
                    upstream,
                    &display_model,
                    timeout,
                    ctx.stats.clone(),
                    record_ctx,
                    tool_map,
                );
            }
        } else {
            return non_stream_response(
                resp,
                &display_model,
                ctx.stats.clone(),
                record_ctx,
                &ctx.protocol,
            )
            .await;
        }
    }

    types::record_usage_safely(&ctx.stats, record_ctx.failure_record());
    types::err_response(
        StatusCode::BAD_GATEWAY,
        &format!("共尝试 {} 次仍失败：{}", attempts.min(budget), last_msg),
    )
}

/// 非流式：读上游 JSON → protocol.parse_non_stream → Anthropic JSON
async fn non_stream_response(
    resp: reqwest::Response,
    display_model: &str,
    stats: Arc<UsageStatsStore>,
    record_ctx: RequestStatsContext,
    protocol: &Arc<dyn UpstreamProtocol>,
) -> Response {
    let upstream: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            types::record_usage_safely(&stats, record_ctx.failure_record());
            return types::err_response(StatusCode::BAD_GATEWAY, &format!("上游响应解析失败: {e}"));
        }
    };
    let resp_id = upstream
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("anon")
        .replace("resp_", "");
    let msg_id = format!("msg_gw_{resp_id}");
    let usage = upstream.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("input_tokens").or_else(|| u.get("prompt_tokens")))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| {
            u.get("output_tokens")
                .or_else(|| u.get("completion_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0);
    types::record_usage_safely(
        &stats,
        record_ctx.success_record(input_tokens, output_tokens, usage.is_some()),
    );
    let anthropic = protocol.parse_non_stream(&upstream, display_model, &msg_id);
    axum::Json(anthropic).into_response()
}

// ===== 连接自检 =====

/// 连接自检：用配置的 first Key/Model 向上游发一次极简探针。
pub async fn test_connection(provider: &ProviderEntry) -> Result<String, String> {
    if provider.api_keys.is_empty() {
        return Err(format!("❌ 未配置任何 {} API Key", provider.name));
    }
    if provider.models.is_empty() {
        return Err(format!("❌ 未配置任何 {} 模型", provider.name));
    }
    let model = provider.models[0].clone();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            provider.request_timeout_seconds,
        ))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 构造极简 Anthropic 探针请求：一条 "hi" 消息，非流式，max_tokens=16。
    let probe_req = AnthropicRequest {
        model: Some(model.clone()),
        system: None,
        messages: vec![serde_json::json!({"role":"user","content":"hi"})],
        max_tokens: Some(16),
        temperature: None,
        top_p: None,
        stream: Some(false),
        stop_sequences: None,
        tools: None,
        tool_choice: None,
        thinking: None,
    };
    let protocol: Arc<dyn UpstreamProtocol> = Arc::new(ChatCompletionsProtocol);
    let probe_body = protocol.build_request(&probe_req, &model, false);
    let base = config::effective_base_url(provider).trim_end_matches('/');
    let url = format!("{base}{}", protocol.upstream_path());

    let req = client
        .post(&url)
        .header(header::ACCEPT_ENCODING, "identity")
        .bearer_auth(&provider.api_keys[0]);

    tracing::info!(provider = %provider.id, model = %model, "连接自检：向上游发送探测请求");
    let resp = match req.json(&probe_body).send().await {
        Ok(r) => r,
        Err(e) => {
            return Err(format!("❌ 上游请求失败: {e}"));
        }
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("❌ 上游错误 {}: {}", status.as_u16(), text));
    }

    // 简单提取回复文本（两种协议都从 choices[0].message.content 或 output_text 取）
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let out = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .or_else(|| {
            v.get("output")
                .and_then(|o| o.as_array())
                .and_then(|a| a.first())
                .and_then(|i| i.get("content"))
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
        })
        .unwrap_or("")
        .to_string();
    Ok(format!(
        "✅ 连接成功（{} 模型 {}）：{}",
        provider.name,
        model,
        if out.is_empty() {
            "（空响应）"
        } else {
            &out
        }
    ))
}

/// 本地消息测试：向本机运行中的代理发一条真实 Anthropic 消息。
pub async fn local_chat_test(
    provider: &ProviderEntry,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{}/v1/messages", provider.port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            provider.request_timeout_seconds.max(30),
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
    if !provider.auth_token.is_empty() {
        req = req.header("x-api-key", &provider.auth_token);
    }

    tracing::info!(provider = %provider.id, model = %model, "本地消息测试：POST {url}");
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
