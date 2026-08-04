// axum 路由装配：仅暴露 /v1/messages（与 Anthropic API 路径一致）
//
// 与 nvidia/server.rs 同结构：注入 ProxyCtx 共享状态 + DefaultBodyLimit 兜底。
// 主超限路径的 Anthropic 形态重塑在 proxy::handle_messages 内完成，这里 layer 仅作
// 防御（避免未来若改回 `body: Bytes` 提取器退回 axum 默认 2MiB 默默 413）。

use crate::grok::proxy::{self, ProxyCtx};
use crate::shared::MAX_REQUEST_BODY_BYTES;
use axum::{extract::DefaultBodyLimit, routing::post, Router};
use std::sync::Arc;

pub fn build_router(ctx: Arc<ProxyCtx>) -> Router {
    Router::new()
        .route("/v1/messages", post(proxy::handle_messages))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(ctx)
}
