// 协议网关模块：8083 通用「Anthropic 入站 ↔ OpenAI 协议上游」转换层。
//
// 8083 端口作为通用协议网关，将任意 OpenAI 协议（Chat Completions）上游大模型
// （deepseek / glm / qwen 等）转换为 Anthropic Messages 协议，供 Claude Code 工具接入。
// 各 provider 挂载到同一套代理重试/冷却/鉴权骨架，仅在「请求体构造」「上游路径」
// 「响应解析」「SSE 流处理」这四个协议差异点分叉（由 UpstreamProtocol trait 封装）。
//
// nvidia 8082 保持独立运行，不碰。
//
// 子模块：
//   - config   : ProviderEntry + GatewayConfig（provider 列表 + active 索引）
//   - auth     : 通用可插拔认证（API Key 轮询 + 429 冷却），对上游零 OAuth 依赖
//   - types    : UpstreamProtocol trait + RequestStatsContext + 共享工具函数
//   - protocol : chat（OpenAI Chat Completions 上游）实现
//   - proxy    : 代理核心（handle_messages）
//   - server   : axum Router 装配
//   - state    : GatewayState 生命周期管理

pub mod auth;
pub mod config;
pub mod protocol;
pub mod proxy;
pub mod server;
pub mod state;
pub mod types;
