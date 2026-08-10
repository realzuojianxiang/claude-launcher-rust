// 协议网关的上游协议实现集。
//
// 上游协议挂载到 UpstreamProtocol trait：
//   - chat : OpenAI Chat Completions（/v1/chat/completions，messages[] 数组）
//            —— deepseek / glm / qwen / 任意 OpenAI 兼容端点可用

pub mod chat;
