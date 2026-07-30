// axum 路由装配：仅暴露 /v1/messages（与 Anthropic API 路径一致）

use crate::nvidia::proxy::{self, ProxyCtx, MAX_REQUEST_BODY_BYTES};
use axum::{extract::DefaultBodyLimit, routing::post, Router};
use std::sync::Arc;

// 构造 axum Router，注入代理上下文作为共享状态。
//
// P2#4：显式挂 DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES) 作为兜底——
// 当前 handle_messages 直接接收 Request<Body> 并用 axum::body::to_bytes(body, 上限)
// 自行把超限重塑为 Anthropic error（见 proxy::handle_messages）；此 layer 仅用于：
// 若未来有人把 handler 改回 `body: Bytes` 提取器，也不致退回 axum 默认 2MiB 默默 413。
// 主超限路径的 Anthropic 形态重塑在 handle_messages 内完成，因此这里无需 HandleError。
pub fn build_router(ctx: Arc<ProxyCtx>) -> Router {
    Router::new()
        .route("/v1/messages", post(proxy::handle_messages))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(ctx)
}
