//! 对话历史中的一条消息 —— 整个 crate 共享的核心传输类型。
//!
//! 历史上住 `src/main.rs`(二进制 crate 根),(a) trait async 升级 Phase A 起 crate root
//! 搬到 `src/lib.rs`(各模块在那 `pub mod`),`Message` 被 `compactor`/`session` 跨模块 `use
//! crate::Message` 引——它在 crate 根才能这样被裸引,故单独挪到这个 `pub mod message`。
//! `main.rs` 仍用它,改走 `use crate::message::Message;`(lib 内 = `use codeagent::message::Message;`)。
//!
//! 类型字节未改、语义未改;仅是把 crate-root 的一项放到独立的 pub mod 里供 lib/bin 共见。

use serde::{Deserialize, Serialize};

/// 对话历史中的一条消息。
/// 一个 Message 承载三种角色(system/user/assistant/tool),靠 role 字段区分;
/// tool_calls(assistant 用)与 tool_call_id(tool 用)都设可选 + skip,
/// 无关角色不序列化这些字段 —— 请求体保持每种角色只发该发的字段。
/// P7:字段对子模块 session 可见 —— save/load 接 `&[Message]` 跨模块要它的类型;
///   单测往返比较需逐字段读,故字段也 pub(crate 内传阅对象)。
/// derive Debug:测试里 unwrap_err()(Ok 变体要 Debug)与失败断言打印需它。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// assistant 回复含的工具调用 —— 回灌进历史时模型要能看见「我刚才调过」(concepts §4 要点 1)。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub tool_calls: Option<Vec<crate::tools::ToolCall>>,
    /// role:tool 时配对的 tool_call_id(concepts §3.2 配对要求)。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}
