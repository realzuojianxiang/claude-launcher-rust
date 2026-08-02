// Anthropic 与 OpenAI 的数据结构定义
//
// 设计取舍：Claude Code 发来的 Anthropic /v1/messages 请求体字段丰富且 content 可能是
// 字符串或 block 数组（text / tool_use / tool_result / image ...）。为兼顾健壮性与可读性，
// 顶层字段用强类型承接，content 用 serde_json::Value 承接、在 converter 里再做展开。
// 未知字段一律忽略（不使用 deny_unknown_fields），避免上游/客户端新增字段导致解析失败。

use serde::Deserialize;
use serde_json::Value;

// Anthropic /v1/messages 请求体（仅取我们需要映射的字段，其余忽略）
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicRequest {
    // Claude Code 会带上模型名；优先透传它（Launch 页所选 NVIDIA 模型），
    // 为空时回退到配置的 models[0]（见 proxy.rs）。
    #[serde(default)]
    #[allow(dead_code)]
    pub model: Option<String>,
    // system 可能是字符串，也可能是 [{type:"text",text:"..."}] 数组
    #[serde(default)]
    pub system: Option<Value>,
    #[serde(default)]
    pub messages: Vec<Value>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    // 工具定义（Anthropic 格式：{name, description, input_schema}）
    #[serde(default)]
    pub tools: Option<Vec<Value>>,
    // 工具选择策略（可选：{type:"auto"} / {type:"any"} / {type:"tool", name:"xxx"}）
    #[serde(default)]
    pub tool_choice: Option<Value>,
    // 扩展思考配置（可选：{type:"enabled", budget_tokens:N} / {type:"adaptive"} / {type:"disabled"}）
    // 用于映射到 OpenAI 的 reasoning_effort（仅当显式开启思考时透传，避免影响普通模型）。
    #[serde(default)]
    pub thinking: Option<Value>,
}

impl AnthropicRequest {
    // 是否流式：缺省视为 false（与 Anthropic 一致）
    pub fn is_stream(&self) -> bool {
        self.stream.unwrap_or(false)
    }
}
