//! codeagent — 造自己的 code agent(codeagent-rs 实现)。
//!
//! crate root 历史上住 `src/main.rs`(纯 binary crate,无 lib target),所有模块在其下私有声明、
//! 共享类型(`Message`)挂在 crate 根。(a) trait async 升级 Phase A 起,为在 `tests/` 集成测试
//! 里调真 `McpClient`/`McpTool`(fake-MCP-server keystone,journey §14.3 / plans/replicated-
//! meandering-dawn.md §2.2),需 lib target 供集成测试 `use codeagent::...` + 需 `CARGO_BIN_EXE_*`
//! (cargo 只给 `tests/` 集成测试设,src 内 `#[test]` 不设)。故把 crate root 搬到这里、各模块在
//! 这 `pub mod`,被跨模块裸引的 `Message` 单独放 `pub mod message`。
//!
//! 这一步是结构搬位、不动任何业务逻辑、不碰 (c′) 桥 `block_on_current`:模块声明从 main.rs 的
//! `mod x;`(私有)搬到这里的 `pub mod x;`(pub 给 lib/tests/bin 共见),被跨模块用到的 `Message`
//! 从挂在 crate 根搬到独立 `pub mod message`。`main.rs` 瘦成 thin bin(`use codeagent::...` /
//! `use crate::message::Message`)。内部实现字节未改、语义未改 —— `pub(crate)` 改 `pub` 仅是
//! crate 外可见性放宽(原本 `pub(crate)` 作「crate 内传阅、不开 crate 外」护栏;此处 lib 是
//! 单 binary 的内部辅助 lib,非对外发布面,可见性放宽无对外影响),不影响行为也不被模型/网络面
//! 暴露(本就在 serde 传输边界上)。

pub mod compactor;
pub mod config;
pub mod mcp;
pub mod message;
pub mod session;
pub mod subagent;
pub mod tools;

// `Message` 历史上挂在 crate 根(二进制 crate 里 `crate::Message` 直接引),(a) Phase A 搬
// root 后挪到 `pub mod message`。在 lib 根 `pub use` 一道 —— 让 `compactor`/`session` 现有
// `use crate::Message;` 不动直接解析(lib crate 里 `crate::` 指本根,见 Message),保持「跨模块
// 裸引」旧用法兼容、零编辑那两文件。bin main.rs 用 `use codeagent::Message`(由 lib 根 pub use 出)。
pub use crate::message::Message;
