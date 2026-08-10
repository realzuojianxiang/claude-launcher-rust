// axum 路由装配：仅暴露 /v1/messages（与 Anthropic API 路径一致）
//
// 注入 ProxyCtx 共享状态 + DefaultBodyLimit 兜底。

use crate::gateway::proxy::{self, ProxyCtx};
use crate::shared::MAX_REQUEST_BODY_BYTES;
use axum::{extract::DefaultBodyLimit, routing::post, Router};
use std::sync::Arc;

pub fn build_router(ctx: Arc<ProxyCtx>) -> Router {
    Router::new()
        .route("/v1/messages", post(proxy::handle_messages))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(ctx)
}
