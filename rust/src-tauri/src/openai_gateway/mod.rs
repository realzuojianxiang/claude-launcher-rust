// 8084 OpenAI 协议透传网关（OpenAI 入站 ↔ OpenAI 协议上游）。
//
// 与 gateway（8083，Anthropic 入站 ↔ OpenAI 上游）互补：8084 不做协议转换，只透传 +
// 统计。入站直接是 OpenAI Chat Completions / Responses 协议，原样转发到 provider 配置的上游，
// 并从响应抽 usage 写进与 8083 共享的 UsageStatsStore。复用 gateway 的 ProviderEntry /
// GatewayConfig / ApiKeyAuthProvider，因此 8083 页面里添加的 deepseek / glm-5.2 等 provider
// 无需重复配置即可被 8084 复用。
//
// 子模块：
//   - config  : 直接复用 gateway::config（ProviderEntry / GatewayConfig）
//   - proxy   : handle_openai（按 model 路由 + 透传 + 抽 usage）
//   - server  : axum Router 装配（/v1/chat/completions + /v1/responses）
//   - state   : OpenAiGatewayState 生命周期管理

pub mod proxy;
pub mod server;
pub mod state;
