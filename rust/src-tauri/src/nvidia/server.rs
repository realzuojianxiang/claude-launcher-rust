// axum 路由装配：仅暴露 /v1/messages（与 Anthropic API 路径一致）

use crate::nvidia::proxy::{self, ProxyCtx};
use axum::{routing::post, Router};
use std::sync::Arc;

// 构造 axum Router，注入代理上下文作为共享状态
pub fn build_router(ctx: Arc<ProxyCtx>) -> Router {
    Router::new()
        .route("/v1/messages", post(proxy::handle_messages))
        .with_state(ctx)
}
