// axum 路由装配：暴露 OpenAI 协议端点（与 OpenAI SDK 默认路径一致）。
//
// Codex / OpenAI SDK 设置 base_url=http://127.0.0.1:8084/v1 后，会请求
// /v1/chat/completions 与 /v1/responses；两条路由都交给同一个 handler，
// 由 handler 按 path 决定上游后缀，保证 8084 对客户端完全透明。

use crate::openai_gateway::proxy::{self, ProxyCtx};
use crate::shared::MAX_REQUEST_BODY_BYTES;
use axum::{extract::DefaultBodyLimit, routing::post, Router};
use std::sync::Arc;

pub fn build_router(ctx: Arc<ProxyCtx>) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(proxy::handle_openai))
        .route("/v1/responses", post(proxy::handle_openai))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(ctx)
}
