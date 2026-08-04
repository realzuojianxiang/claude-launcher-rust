// Grok 代理核心：接收 Anthropic /v1/messages，转换为 OpenAI Responses 请求转发到
// grok 上游（cli-chat-proxy.grok.com 或 api.x.ai），响应再转回 Anthropic Messages
// （SSE 流式经 stream.rs 状态机，非流式经 converter::responses_json_to_anthropic）。
//
// 与 nvidia/proxy.rs 共用的硬约束：redirect::Policy::none() + 3xx 判 502（防 SSRF +
// bearer 外泄）、3xx/3xx 处理、429 冷却切换 Key、5xx 切模型复用 Key、4xx 直传上游错误、
// 32 MiB 请求体上限重塑为 Anthropic error、非回环强鉴权、请求头身份恒时比较。
//
//grok 特有：
//   - AuthProvider 可插拔：取上游鉴权头由 auth_provider 决定（API Key 走 Bearer key，
//     OAuth Phase 3 走 Bearer token + CLI Chat-Proxy 专用头）。
//   - 上游路径是 /responses（Responses 协议），不是 /chat/completions。
//   - 模型名映射：入站 claude-* 按 GrokConfig::map_model 改写为 grok slug。
//   - 响应非 Chat 增量而是 Responses 事件流，由 stream::StreamState 翻译。
//
// Phase 2：API Key 退路通路打通（OAuth Phase 3 接入）。admin provider 由 GrokState::start
// 按 auth_mode 构造传入 ProxyCtx::new。

// 本模块 pub fn（handle_messages/test_connection/local_chat_test 等）尚未被
// server.rs/GrokState::start/lib.rs 接线，整模块放行 dead_code 以过 -D warnings 门禁；
// 接线后 allow 无副作用保留。
#![allow(dead_code)]

use crate::config::GrokConfig;
use crate::grok::auth::{AuthProvider, RefreshOutcome};
use crate::grok::converter;
use crate::grok::models::GrokAuthMode;
use crate::grok::oauth_store;
use crate::grok::stream::{FeedOutcome, StreamState};
use crate::nvidia::models::AnthropicRequest;
use crate::shared::{self, MAX_REQUEST_BODY_BYTES};

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{stream::BoxStream, StreamExt};
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use std::sync::Arc;

const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// 代理运行时上下文：配置快照 + 复用异步 HTTP 客户端 + 可插拔认证 + 热更新模型。
pub struct ProxyCtx {
    pub cfg: GrokConfig,
    pub client: reqwest::Client,
    /// 上游认证策略。Phase 2 唯一实现为 ApiKeyAuthProvider；Phase 3 加 OAuthAuthProvider。
    pub auth_provider: Arc<dyn AuthProvider>,
    /// 模型优先级列表（热更新）：读多写少，用 RwLock；写入来自 GrokState::set_models。
    pub models: Arc<std::sync::RwLock<Vec<String>>>,
}

impl ProxyCtx {
    /// 由 GrokState::start 按 auth_mode 构造对应 AuthProvider 后传入。
    /// grok 的 client 取与 nvidia 同样的 redirect::none + connect_timeout 守护。
    pub fn new(cfg: GrokConfig, auth_provider: Arc<dyn AuthProvider>) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            // P2/SSRF：禁止上游自动跟随重定向。否则误配非默认 base_url 或上游被劫持
            // 返回 30x 时，会把带 Authorization: Bearer <token/key> 的请求体转到攻击者
            // 主机/云元数据端点。配合 GrokConfig::validate_base_url 的 scheme+host 校验，
            // 上游 URL 与重定向两路都不被外部改写。
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let models = Arc::new(std::sync::RwLock::new(cfg.models.clone()));
        Arc::new(Self {
            cfg,
            client,
            auth_provider,
            models,
        })
    }

    /// 供 GrokState::pool_status 取脱敏快照（OAuth Phase 3 另实现）。
    pub fn pool_snapshot(&self) -> Value {
        self.auth_provider.snapshot()
    }
}

// 恒时字节比较（constant-time compare）：避免计时侧信道下的 token 逐字节泄露。
// 与 nvidia/proxy.rs::ct_eq 同实现，grok 侧自留一份避免跨模块耦合式调用。
fn ct_eq(a: &str, b: &str) -> bool {
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

// 构造 Anthropic 风格错误响应
fn err_response(status: StatusCode, msg: &str) -> Response {
    let body = json!({
        "type": "error",
        "error": { "type": "proxy_error", "message": msg }
    });
    (status, axum::Json(body)).into_response()
}

// axum handler：POST /v1/messages
pub async fn handle_messages(
    State(ctx): State<Arc<ProxyCtx>>,
    request: axum::http::Request<Body>,
) -> Response {
    crate::grok_diag_step("proxy handle_messages(): entry");
    let (parts, body) = request.into_parts();
    let headers = parts.headers;

    // 1. 本地代理鉴权：配置 auth_token 时恒时校验 x-api-key
    if !ctx.cfg.auth_token.is_empty() {
        let provided = headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !ct_eq(provided, &ctx.cfg.auth_token) {
            return err_response(StatusCode::UNAUTHORIZED, "无效的 x-api-key");
        }
    }

    // 2. 读请求体（带显式上限 MAX_REQUEST_BODY_BYTES），超限重塑为 Anthropic 413。
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

    // 3. 解析 Anthropic 请求体（未知字段一律忽略）
    let req: AnthropicRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, &format!("请求体解析失败: {e}")),
    };

    // 4. 实时读取模型优先级快照（可被 UI 热更新）
    let live_models: Vec<String> = ctx
        .models
        .read()
        .map(|m| m.clone())
        .unwrap_or_else(|_| ctx.cfg.models.clone());
    if live_models.is_empty() {
        return err_response(StatusCode::SERVICE_UNAVAILABLE, "未配置任何 Grok 模型");
    }

    // 5. 模型映射 + Fallback 链：入站 model 按 map_model 改写为 grok slug，其后按优先级
    //    追加其余模型（去重保序）。Fallback 用于 5xx/404 时切模型。
    let stream = req.is_stream();
    let base_model = ctx.cfg.map_model(
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

    // 6. 重试循环：Key 级故障（429 全冷却 / 网络）换 Key；模型级故障（5xx/404）切模型复用 Key。
    let max_retries = ctx.cfg.max_retries.max(1) as usize;
    let url = format!(
        "{}/responses",
        ctx.cfg.effective_base_url().trim_end_matches('/')
    );
    let timeout = std::time::Duration::from_secs(ctx.cfg.request_timeout_seconds);

    // 对外展示的模型名：用入站 Anthropic 侧的 model（Claude Code 看到的那个）；
    // 上游实际打哪个 grok slug 仅用于日志。
    let display_model = req.model.clone().unwrap_or_else(|| base_model.clone());

    let mut model_idx = 0usize;
    let mut attempts = 0usize;
    let mut last_msg = "上游未返回可用响应".to_string();
    // OAuth 刷新标记：一次请求对 401 只 refresh 一次（防 refresh 死循环）。
    let mut refreshed_once = false;

    loop {
        attempts += 1;
        if attempts > max_retries {
            break;
        }
        let model = models_chain[model_idx % models_chain.len()].clone();

        // 6a. 取鉴权头。OAuth 模式 auth_headers 可能随刷新变化；API Key 模式从池里 pick。
        // 拿不到头（全部 Key 冷却 / OAuth 无可用 token）即不可恢复——本请求无法重试，
        // 直接终止并把可读错误回传给 Claude Code。
        let auth_headers = match ctx.auth_provider.auth_headers() {
            Ok(h) => h,
            Err(e) => {
                last_msg = e;
                break;
            }
        };

        // 6b. 响应侧转换：Anthropic 请求 -> Responses 请求体
        let responses_body = converter::convert_request_body(&body, &model);
        let responses_body = match responses_body {
            Ok(v) => v,
            Err(e) => return err_response(StatusCode::BAD_REQUEST, &e),
        };

        tracing::info!(
            attempt = attempts,
            model = %model,
            display_model = %display_model,
            auth_mode = ?ctx.cfg.auth_mode,
            "转发请求到 Grok 上游 /v1/responses"
        );

        let mut builder = ctx
            .client
            .post(&url)
            // clone：auth_headers 在 429 冷却路径还需反解 Bearer，故不在 .headers 里 move。
            .headers(auth_headers.clone())
            // SSE 流必须禁压缩（同 nvidia 理由：分块 chunked gzip 解码易失败）
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
            .json(&responses_body);
        if !stream {
            builder = builder.timeout(timeout);
        }

        let send_fut = builder.send();
        let resp = match tokio::time::timeout(timeout, send_fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                tracing::warn!(attempt = attempts, error = %e, "上游网络错误（超时/连接），切换重试");
                last_msg = format!("上游网络错误: {e}");
                continue;
            }
            Err(_) => {
                tracing::warn!(
                    attempt = attempts,
                    timeout_secs = ctx.cfg.request_timeout_seconds,
                    "上游响应头超时，切换重试"
                );
                last_msg = format!("上游 {} 秒内未返回响应头", ctx.cfg.request_timeout_seconds);
                continue;
            }
        };

        let status = resp.status();
        crate::grok_diag_step(&format!(
            "proxy upstream status={} model={} attempt={}",
            status.as_u16(),
            model,
            attempts
        ));

        // 6c. 401：refresh 一次重试，再失败则终止。
        if status == StatusCode::UNAUTHORIZED && !refreshed_once {
            refreshed_once = true;
            match ctx.auth_provider.on_401().await {
                RefreshOutcome::Refreshed => {
                    tracing::warn!(attempt = attempts, "上游 401，已刷新凭证重试");
                    last_msg = "上游 401，已刷新凭证重试".to_string();
                    continue;
                }
                RefreshOutcome::Unrecoverable => {
                    let text = read_error_body_limited(resp, timeout).await;
                    tracing::warn!(attempt = attempts, body = %text, "上游 401 不可恢复（凭证失效）");
                    return err_response(
                        StatusCode::UNAUTHORIZED,
                        &format!("上游 401：凭证无效或已失效，请在设置中检查/重新授权。{text}"),
                    );
                }
            }
        }

        if status == StatusCode::TOO_MANY_REQUESTS {
            // 6d. 429：冷却本轮实际发送用的 Key（从已选 auth_headers 反解 Bearer，
            // 不重新 pick——避免轮换偏移），保持当前模型，下一轮换 Key。
            {
                if let Some(pool) = ctx.auth_provider.key_pool_opt() {
                    if let Some(key) = bearer_key_from(&auth_headers) {
                        let mut pool = pool.lock().unwrap();
                        pool.cooldown(&key);
                    }
                }
            }
            let text = read_error_body_limited(resp, timeout).await;
            tracing::warn!(
                attempt = attempts,
                key_cooled = true,
                status = 429,
                "收到 429，冷却该 Key 并切 Key 重试"
            );
            last_msg = format!("上游 429 限流，已冷却凭证并切换重试: {text}");
            continue;
        }
        if status.is_server_error() {
            // 6e. 5xx：切模型，复用当前 Key
            let text = read_error_body_limited(resp, timeout).await;
            tracing::warn!(attempt = attempts, model = %model, status = %status.as_u16(), "上游 5xx，切换模型重试");
            last_msg = format!("上游错误 {}: {}", status.as_u16(), text);
            if model_idx < models_chain.len() - 1 {
                model_idx += 1;
                continue;
            }
            break;
        }
        if status == StatusCode::NOT_FOUND && model_idx < models_chain.len() - 1 {
            // 6f. 404：模型不在上游路由，切备选模型复用 Key
            let text = read_error_body_limited(resp, timeout).await;
            tracing::warn!(attempt = attempts, model = %model, status = 404, "上游 404，切换备选模型");
            last_msg = format!("模型 {model} 返回 404: {text}");
            model_idx += 1;
            continue;
        }
        if status.is_redirection() {
            // 6g. 3xx：redirect 已被 Policy::none() 关闭，原样到此——判为致命 502，
            // 不重试/不接力，避免被劫持上游借 30x 把 bearer 套走。
            let text = read_error_body_limited(resp, timeout).await;
            tracing::error!(model = %model, status = %status.as_u16(), body = %text, "上游返回重定向（已禁止跟随），中止以防 bearer 泄漏");
            return err_response(
                StatusCode::BAD_GATEWAY,
                &format!(
                    "上游返回重定向 {}：已禁止跟随以避免凭证泄漏。请检查 Grok Base URL 是否指向官方端点。",
                    status.as_u16()
                ),
            );
        }
        if !status.is_success() {
            // 6h. 其余 4xx：客户端错误，不重试，直传上游错误
            let code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let text = read_error_body_limited(resp, timeout).await;
            tracing::warn!(model = %model, status = %code, body = %text, "上游返回客户端错误（不重试）");
            return err_response(code, &format!("上游错误 {code}: {text}"));
        }

        // 6i. 成功：按 stream 分流
        tracing::info!(attempt = attempts, model = %model, "上游成功");
        if stream {
            return stream_response(resp, &display_model, timeout).await;
        } else {
            return non_stream_response(resp, &display_model).await;
        }
    }

    err_response(
        StatusCode::BAD_GATEWAY,
        &format!(
            "共尝试 {} 次仍失败：{}",
            attempts.min(max_retries),
            last_msg
        ),
    )
}

// 从「已选好、即随请求发出」的鉴权头集里反解本轮实际用的 Bearer key/token。
// 用于 429 冷却——必须从这份已发送头取，而不是再调 auth_headers 触发一次 pick（会让
// 池指针偏移到另一条 key 上去）。OAuth token 同理反解（冷却对 token 无意义，但若
// 未命中 key_pool_opt 分支也不会走到这里）。
fn bearer_key_from(headers: &HeaderMap) -> Option<String> {
    let bearer = headers.get(reqwest::header::AUTHORIZATION)?.to_str().ok()?;
    bearer.strip_prefix("Bearer ").map(|s| s.to_string())
}

// 非流式：reads 上游 Responses JSON -> Anthropic Messages JSON
async fn non_stream_response(resp: reqwest::Response, display_model: &str) -> Response {
    let upstream: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return err_response(StatusCode::BAD_GATEWAY, &format!("上游响应解析失败: {e}")),
    };
    // msg_id：用 response.id 兜底包一下形如 msg_grok_resp_xxx
    let resp_id = upstream
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("anon")
        .replace("resp_", "");
    let msg_id = format!("msg_grok_{resp_id}");
    let anthropic = converter::responses_json_to_anthropic(&upstream, display_model, &msg_id);
    axum::Json(anthropic).into_response()
}

// 流式：上游 Responses SSE -> StreamState 翻译 -> Anthropic SSE 字节流转发
async fn stream_response(
    resp: reqwest::Response,
    display_model: &str,
    timeout: std::time::Duration,
) -> Response {
    // async_stream 闭包要捕获 display_model 并要求 'static，先把 &str 拥有权化。
    let display_model = display_model.to_string();
    let mut upstream: BoxStream<'static, Result<Bytes, reqwest::Error>> =
        resp.bytes_stream().boxed();

    // 用 async_stream 构造一个把上游 chunk 喂进 StreamState、产出 Anthropic SSE 串的流。
    // 用固定 seq=0 起的消息 id（无状态实现，每次请求新建 state，无需跨请求递增）。
    let stream = async_stream::stream! {
        let mut state = StreamState::new(&display_model, 0);
        let mut buf: Vec<u8> = Vec::new();
        let mut last_idle = tokio::time::Instant::now();
        loop {
            // 逐 chunk 读取：读取超时即下一个空闲 chunk（持续空闲 timeout 则发 error 终止）
            let chunk = match tokio::time::timeout(timeout, upstream.next()).await {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    let msg = format!("event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"upstream_error\",\"message\":\"读取上游流失败: {e}\"}}}}\n\n");
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(msg));
                    return;
                }
                Ok(None) => {
                    // 上游正常结束：发剩余 finish 串
                    let tail = state.finish();
                    if !tail.is_empty() {
                        yield Ok(Bytes::from(tail));
                    }
                    return;
                }
                Err(_) => {
                    if last_idle.elapsed() >= timeout {
                        let msg = format!("event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"idle_timeout\",\"message\":\"上游 {} 秒内无数据\"}}}}\n\n", timeout.as_secs());
                        yield Ok(Bytes::from(msg));
                        return;
                    }
                    continue;
                }
            };
            last_idle = tokio::time::Instant::now();
            buf.extend_from_slice(&chunk);
            // 跨 chunk 按行切分（严格 UTF-8 边界保护）
            let lines = shared::split_complete_sse_lines(&mut buf);
            for line in lines {
                match line {
                    Ok(text) => match state.feed_line(&text) {
                        Ok(FeedOutcome::Events(events, done)) => {
                            if !events.is_empty() {
                                yield Ok(Bytes::from(events));
                            }
                            if done {
                                let tail = state.finish();
                                if !tail.is_empty() {
                                    yield Ok(Bytes::from(tail));
                                }
                                return;
                            }
                        }
                        Err(e) => {
                            let msg = format!("event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"stream_error\",\"message\":\"{e}\"}}}}\n\n");
                            yield Ok(Bytes::from(msg));
                            return;
                        }
                    },
                    Err(_) => {
                        let msg = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"stream_error\",\"message\":\"上游 SSE 非合法 UTF-8\"}}\n\n";
                        yield Ok(Bytes::from(msg));
                        return;
                    }
                }
            }
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("connection", "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| err_response(StatusCode::INTERNAL_SERVER_ERROR, "构造流式响应失败"))
}

// =====================================================================
// 连接自检 test_connection：直连 grok 上游发一次极简 Responses 请求
// =====================================================================

pub async fn test_connection(cfg: &GrokConfig) -> Result<String, String> {
    // 校验：API Key 模式要至少一个 key；OAuth 模式要本地凭证 + 邮箱。
    match cfg.auth_mode {
        GrokAuthMode::ApiKey => {
            if cfg.api_keys.is_empty() {
                return Err("❌ 未配置任何 Grok API Key".to_string());
            }
        }
        GrokAuthMode::Oauth => {
            // OAuth 自检（Phase 3）：从 DPAPI 落盘读 token；缺凭证即未授权。
            if oauth_store::load().is_none() {
                return Err(
                    "❌ OAuth 模式尚未授权（无本地凭证），请先点「授权 Grok 账号」".to_string(),
                );
            }
        }
    }
    if cfg.models.is_empty() {
        return Err("❌ 未配置任何模型".to_string());
    }
    let model = cfg.models[0].clone();
    // 探测客户端：redirect::none 是不可妥协安全闸（防 SSRF + bearer 外泄）。
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(cfg.request_timeout_seconds))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let probe = json!({
        "model": model,
        "instructions": "",
        "input": [ { "type":"message","role":"user","content":[{"type":"input_text","text":"hi"}] } ],
        "stream": false,
        "max_output_tokens": 16,
        "store": false,
    });
    let base = cfg.effective_base_url().trim_end_matches('/');
    let url = format!("{base}/responses");

    // 按模式拼鉴权头（与代理转发共用同一组头策略，避免自检与实跑行为分叉）。
    let mut req = client
        .post(&url)
        .header(header::ACCEPT_ENCODING, "identity");
    match cfg.auth_mode {
        GrokAuthMode::ApiKey => {
            req = req.bearer_auth(&cfg.api_keys[0]);
        }
        GrokAuthMode::Oauth => {
            // 取本地 token；access 拼进 Authorization，再加两条 CLI Chat-Proxy 身份头。
            // 与 OAuthAuthProvider::build_headers 字段一致，确保自检与代理转发上游行为同构。
            let token = oauth_store::load().ok_or_else(|| "❌ OAuth 凭证读取失败".to_string())?;
            if token.access_token.trim().is_empty() {
                return Err("❌ OAuth token 损坏（access_token 为空）".to_string());
            }
            req = req
                .bearer_auth(&token.access_token)
                .header("X-XAI-Token-Auth", "xai-grok-cli")
                .header("x-grok-client-version", "0.2.93");
        }
    }

    tracing::info!(mode = ?cfg.auth_mode, model = %model, "Grok 连接自检：向上游发探测请求");
    let resp = match req.json(&probe).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "Grok 连接自检：上游请求失败");
            return Err(format!("❌ 上游请求失败: {e}"));
        }
    };

    let status = resp.status();
    // 3xx 在 redirect::none 下不会跟随——判为上游配置异常，向外供 502 语义不做软继续。
    if status.is_redirection() {
        return Err(format!(
            "❌ 上游返回 {} 重定向（{base} 不应重定向，代理禁止跟随）",
            status.as_u16()
        ));
    }
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(status = %status.as_u16(), body = %text, "Grok 连接自检：上游返回错误");
        return Err(format!("❌ 上游错误 {}: {}", status.as_u16(), text));
    }

    // 提取返回文本（Responses 非流式 output[].message.content[].output_text.text）
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let out = extract_responses_text(&v);
    tracing::info!(model = %model, len = out.len(), "Grok 连接自检：成功");
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

/// 从 Responses 非流式 JSON 提取首个 message 项的 output_text 拼起来。
fn extract_responses_text(v: &Value) -> String {
    let Some(output) = v.get("output").and_then(Value::as_array) else {
        return String::new();
    };
    for item in output {
        if item.get("type").and_then(Value::as_str) == Some("message") {
            return converter::collect_message_output_text(item);
        }
    }
    String::new()
}

// 本地消息测试：向本机运行中的代理发一条真实 Anthropic 消息（走完整转换链）
pub async fn local_chat_test(
    cfg: &GrokConfig,
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
    if !cfg.auth_token.is_empty() {
        req = req.header("x-api-key", &cfg.auth_token);
    }

    tracing::info!(model = %model, "Grok 本地消息测试：POST {url}");
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
    let reply = v
        .get("content")
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter()
                .filter_map(|b| {
                    if b.get("type").and_then(Value::as_str) == Some("text") {
                        b.get("text").and_then(Value::as_str).map(String::from)
                    } else {
                        None
                    }
                })
                .next()
        })
        .unwrap_or_default();
    let usage = v.get("usage").cloned().unwrap_or(Value::Null);
    Ok(format!(
        "✅ 成功（{:.1}s）模型 {}：{}\n用量：{}",
        elapsed, model, reply, usage
    ))
}

// 读取上游错误体（带超时 + 64KB 上限，避免大错误响应吃满内存）
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
            Ok(Some(Err(_))) => {
                suffix = "…（读取错误体失败）";
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
