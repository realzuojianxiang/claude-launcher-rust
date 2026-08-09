// Responses SSE -> Anthropic Messages SSE 状态机（grok 上游响应侧）。
//
// 上游（cli-chat-proxy.grok.com / api.x.ai 的 /v1/responses 流式端点）吐的是
// OpenAI Responses 协议的 SSE 事件流，Claude Code 端只认 Anthropic Messages SSE。
// 本模块把前者逐事件喂进一个状态机，吐出后者的事件序列串，由 proxy.rs 拼成字节流转发。
//
// 协议契约来自 CLIProxyAPI `codex_claude_response.go`（Responses -> Anthropic 状态机），
// 在此从零 Rust 实现，两项目独立。事件级映射（B1-B19）：
//
//   response.created                         -> message_start（仅一次）
//   response.reasoning_summary_part.added    -> content_block_start(thinking)
//                                              （开新 text/thinking 前先 finalize 上一个 pending）
//   response.reasoning_summary_text.delta    -> content_block_delta(thinking_delta)
//   response.reasoning_text.delta            -> 归一化为 reasoning_summary_text.delta 后同上
//   response.reasoning_summary_part.done     -> 置 ThinkingStopPending（**不立即发 stop**，
//                                              等 reasoning done 的 signature 或下一个块开
//                                              始时收尾，避免空 thinking 块 / 顺序错乱）
//   response.content_part.added(output_text) -> content_block_start(text)（先 finalize thinking）
//   response.output_text.delta               -> content_block_delta(text_delta)
//                                              （先 finalize thinking + start text）
//   response.content_part.done(output_text)  -> content_block_stop(text)
//   response.output_item.added(function_call)-> content_block_start(tool_use) + 初始空 input_json_delta
//   response.function_call_arguments.delta   -> content_block_delta(input_json_delta, partial_json)
//   response.output_item.done(function_call) -> content_block_stop(tool_use)
//   response.output_item.done(reasoning)     -> signature_delta + content_block_stop(thinking)
//   response.output_item.done(message,非流式)-> 整段 text 一次性发 text_delta + content_block_stop
//   response.completed / response.incomplete -> message_delta(stop_reason + usage) + message_stop
//   keepalive / response.web_search_call.*   -> 丢弃
//   error                                    -> Anthropic error 事件
//
// 解析约定：本状态机只吃「已经按 \\n 切好、trim 好的整行」（调用方用
// shared::split_complete_sse_lines 处理跨 chunk UTF-8 边界）。对每行：
//   - 仅取 `data:` 行；空行 / `event:` 行 / 注释行忽略（事件类型就在 data JSON 的 `type`）。
//   - `data: [DONE]` -> 终止信号。
//
// Phase 2 阶段：本模块已可被 proxy.rs 流式分支接线；当前尚未接线，整模块放行 dead_code。
#![allow(dead_code)]

use crate::nvidia::converter::{
    ev_content_block_delta_input_json, ev_content_block_delta_text,
    ev_content_block_delta_thinking, ev_content_block_start_text, ev_content_block_start_thinking,
    ev_content_block_start_tool_use, ev_content_block_stop, ev_message_delta, ev_message_start,
    ev_message_stop, sse_event,
};
use serde_json::{json, Value};

/// 流式响应里 Claude Code 期望的 message id（Anthropic 形如 `msg_<22 hex>`）。
/// 上游 Responses 的 response.id 形如 `resp_<...>`，不能直接给——这里生成一个稳定的
/// msg_ 前缀 id。本无状态实现不依赖随机源，故用一个确定的前缀 + 序号（调用方传入）。
const MSG_ID_PREFIX: &str = "msg_grok_";

/// 思考块（thinking）的生命周期要点：
///   - reasoning_summary_part.added 开启 thinking；
///   - reasoning_summary_part.done 不立即 stop，置 pending（reasoning_text.done 亦归一化到这）；
///   - 直到「reasoning done 带 signature」或「下一个 block 开始」或「完成」时再 stop。
///
/// 这样可以避免空 thinking 块，并让 signature_delta 在 stop 前发出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenBlock {
    None,
    Thinking,
    Text,
    ToolUse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalOutcome {
    Pending,
    Success,
    Failure,
}

/// 状态机内部状态。一次请求对应一次 reset + 逐行喂入 + take_events 取串。
#[derive(Debug)]
pub struct StreamState {
    /// 上游 response.id（来自 response.created），用于诊断；Anthropic 端用本地生成的 msg_id。
    upstream_response_id: String,
    /// 本地生成的 Anthropic message id。
    msg_id: String,
    /// 对外展示的模型名（Anthropic 侧的入站 model，即 Claude Code 看到的那个）。
    model: String,
    /// message_start 是否已发（response.created 到来时发一次）。
    started: bool,
    /// 已完结（completed/incomplete）——之后再喂忽略，防止重复发 message_stop。
    finished: bool,
    /// 上一个未关闭的 block 类型。
    open: OpenBlock,
    /// 下一个 block 的 index（每次 stop 后自增）。
    block_index: i32,
    /// 思考块等待收尾（part.done 已到、但 stop 还没发）。
    thinking_stop_pending: bool,
    /// 累计的 usage（completed/incomplete 时上报）。
    input_tokens: u64,
    raw_input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
    usage_available: bool,
    terminal_outcome: TerminalOutcome,
    /// 最终 stop_reason（completed/incomplete 里取，或默认 end_turn）。
    stop_reason: String,
}

impl StreamState {
    /// 新建状态机。`model` 为对外（Anthropic 侧）展示的模型名，通常是入站请求里的 model 字段。
    pub fn new(model: &str, seq: usize) -> Self {
        Self {
            upstream_response_id: String::new(),
            msg_id: format!("{MSG_ID_PREFIX}{seq}"),
            model: model.to_string(),
            started: false,
            finished: false,
            open: OpenBlock::None,
            block_index: 0,
            thinking_stop_pending: false,
            input_tokens: 0,
            raw_input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            usage_available: false,
            terminal_outcome: TerminalOutcome::Pending,
            stop_reason: "end_turn".to_string(),
        }
    }

    pub fn usage_snapshot(&self) -> (u64, u64, bool) {
        (
            self.raw_input_tokens,
            self.output_tokens,
            self.usage_available,
        )
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn terminal_outcome(&self) -> TerminalOutcome {
        self.terminal_outcome
    }

    /// 喂一行 SSE（已 trim）。返回应当立即向下游发送的事件串（可能为空）。
    /// 第一个 `Ok(true)` 表示遇到 `[DONE]`，调用方应在 flush 之后结束流。
    /// `Err(message)` 表示致命解码/协议错误，调用方应发 error 事件并结束流。
    pub fn feed_line(&mut self, line: &str) -> Result<FeedOutcome, String> {
        if self.finished {
            return Ok(FeedOutcome::events(String::new(), false));
        }
        // 只认 data: 行
        let Some(payload) = line.strip_prefix("data:").map(str::trim) else {
            // event: / 注释 / 空行：忽略（事件类型在 data JSON 的 type 字段）
            return Ok(FeedOutcome::events(String::new(), false));
        };
        if payload == "[DONE]" {
            return Ok(FeedOutcome::events(String::new(), true));
        }
        let ev: Value =
            serde_json::from_str(payload).map_err(|e| format!("上游 SSE data 非合法 JSON: {e}"))?;
        let ty = ev
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut out = String::new();
        self.dispatch(&ty, &ev, &mut out)?;
        Ok(FeedOutcome::events(out, false))
    }

    /// 流结束（上游正常断开但没发 completed）时调用：补一个带默认 stop_reason 的收尾。
    /// 若已经 finished 则什么都不做。
    pub fn finish(&mut self) -> String {
        if self.finished {
            return String::new();
        }
        let mut out = String::new();
        self.finalize_open(&mut out);
        self.emit_message_delta_and_stop(&mut out);
        out
    }

    // ===== 内部分发 =====

    fn dispatch(&mut self, ty: &str, ev: &Value, out: &mut String) -> Result<(), String> {
        match ty {
            "response.created" => self.on_response_created(ev, out),
            "response.reasoning_summary_part.added" => self.on_reasoning_part_added(ev, out),
            "response.reasoning_text.added" => self.on_reasoning_part_added(ev, out),
            "response.reasoning_summary_text.delta" => self.on_reasoning_text_delta(ev, out),
            "response.reasoning_text.delta" => self.on_reasoning_text_delta(ev, out),
            "response.reasoning_summary_part.done" => self.on_reasoning_part_done(),
            "response.reasoning_text.done" => self.on_reasoning_part_done(),
            "response.content_part.added" => self.on_content_part_added(ev, out),
            "response.output_text.delta" => self.on_output_text_delta(ev, out),
            "response.content_part.done" => self.on_content_part_done(ev, out),
            "response.output_item.added" => self.on_output_item_added(ev, out),
            "response.function_call_arguments.delta" => {
                self.on_function_call_arguments_delta(ev, out)
            }
            "response.output_item.done" => self.on_output_item_done(ev, out),
            "response.completed" => self.on_completed(ev, out),
            "response.incomplete" => self.on_completed(ev, out),
            "response.failed" => self.on_failed(ev, out),
            "error" => self.on_error_event(ev),
            // keepalive / web_search_call.* / 其余未知事件：忽略
            _ => Ok(()),
        }
    }

    // --- B1 ---
    fn on_response_created(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        if !self.started {
            if let Some(id) = ev
                .get("response")
                .and_then(|r| r.get("id"))
                .and_then(Value::as_str)
            {
                self.upstream_response_id = id.to_string();
            }
            out.push_str(&sse_event(
                "message_start",
                &ev_message_start(&self.msg_id, &self.model),
            ));
            self.started = true;
        }
        Ok(())
    }

    // --- B2 ---
    fn on_reasoning_part_added(&mut self, _ev: &Value, out: &mut String) -> Result<(), String> {
        self.finalize_pending_thinking(out);
        if self.open != OpenBlock::Thinking {
            self.finalize_open(out); // 若正开着 text/tool，先关掉（防御性，正常不应有）
            out.push_str(&sse_event(
                "content_block_start",
                &ev_content_block_start_thinking(self.block_index),
            ));
            self.open = OpenBlock::Thinking;
        }
        Ok(())
    }

    // --- B3 ---
    fn on_reasoning_text_delta(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        if self.open != OpenBlock::Thinking {
            // 上游先发 delta 再发 part.added 的兜底：开一个 thinking 块
            self.finalize_pending_thinking(out);
            self.finalize_open(out);
            out.push_str(&sse_event(
                "content_block_start",
                &ev_content_block_start_thinking(self.block_index),
            ));
            self.open = OpenBlock::Thinking;
        }
        if let Some(text) = pick_summary_text_delta(ev) {
            if !text.is_empty() {
                out.push_str(&sse_event(
                    "content_block_delta",
                    &ev_content_block_delta_thinking(self.block_index, &text),
                ));
            }
        }
        Ok(())
    }

    // --- B4/B5 ---
    fn on_reasoning_part_done(&mut self) -> Result<(), String> {
        if self.open == OpenBlock::Thinking {
            self.thinking_stop_pending = true;
        }
        Ok(())
    }

    // --- B6 ---
    fn on_content_part_added(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        let part_type = ev
            .get("part")
            .and_then(|p| p.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if part_type != "output_text" {
            // 非 output_text 的 content_part（如 refusal 等）暂不转发
            return Ok(());
        }
        // 先收尾 thinking（含 pending）
        self.finalize_pending_thinking(out);
        self.finalize_open(out);
        out.push_str(&sse_event(
            "content_block_start",
            &ev_content_block_start_text(self.block_index),
        ));
        self.open = OpenBlock::Text;
        Ok(())
    }

    // --- B7 ---
    fn on_output_text_delta(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        // 先收尾 thinking（可能上一段是推理，这一段直接进文本而非走 content_part.added）
        self.finalize_pending_thinking(out);
        if self.open != OpenBlock::Text {
            self.finalize_open(out);
            out.push_str(&sse_event(
                "content_block_start",
                &ev_content_block_start_text(self.block_index),
            ));
            self.open = OpenBlock::Text;
        }
        if let Some(text) = ev.get("delta").and_then(Value::as_str) {
            if !text.is_empty() {
                out.push_str(&sse_event(
                    "content_block_delta",
                    &ev_content_block_delta_text(self.block_index, text),
                ));
            }
        }
        Ok(())
    }

    // --- B8 ---
    fn on_content_part_done(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        let part_type = ev
            .get("part")
            .and_then(|p| p.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if part_type != "output_text" {
            return Ok(());
        }
        if self.open == OpenBlock::Text {
            out.push_str(&sse_event(
                "content_block_stop",
                &ev_content_block_stop(self.block_index),
            ));
            self.open = OpenBlock::None;
            self.block_index += 1;
        }
        Ok(())
    }

    // --- B9 ---
    fn on_output_item_added(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        let item_type = ev
            .get("item")
            .and_then(|i| i.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if item_type != "function_call" {
            // reasoning/message 项的 added 不在这里开块（reasoning 由 part.added 开，message 整体由 output_item.done 一次发）
            return Ok(());
        }
        self.finalize_pending_thinking(out);
        self.finalize_open(out);
        let item = ev.get("item").cloned().unwrap_or(Value::Null);
        let id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        out.push_str(&sse_event(
            "content_block_start",
            &ev_content_block_start_tool_use(self.block_index, id, name),
        ));
        // 初始空 input_json_delta：Anthropic 端工具调用块的 input 必须以 input_json_delta 流式累积，
        // 上游首轮可能不带 arguments，先发一个空增量让下游进 tool_use 状态。
        out.push_str(&sse_event(
            "content_block_delta",
            &ev_content_block_delta_input_json(self.block_index, ""),
        ));
        self.open = OpenBlock::ToolUse;
        Ok(())
    }

    // --- B10 ---
    fn on_function_call_arguments_delta(
        &mut self,
        ev: &Value,
        out: &mut String,
    ) -> Result<(), String> {
        if self.open != OpenBlock::ToolUse {
            return Ok(()); // 没有 pending tool_use 块时，丢弃增量（防御性）
        }
        if let Some(delta) = ev.get("delta").and_then(Value::as_str) {
            if !delta.is_empty() {
                out.push_str(&sse_event(
                    "content_block_delta",
                    &ev_content_block_delta_input_json(self.block_index, delta),
                ));
            }
        }
        Ok(())
    }

    // --- B11/B12/B13 ---
    fn on_output_item_done(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        let item_type = ev
            .get("item")
            .and_then(|i| i.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        match item_type {
            "function_call" => {
                if self.open == OpenBlock::ToolUse {
                    out.push_str(&sse_event(
                        "content_block_stop",
                        &ev_content_block_stop(self.block_index),
                    ));
                    self.open = OpenBlock::None;
                    self.block_index += 1;
                }
            }
            "reasoning" => {
                // 标准做法：reasoning done 时若有 signature 则发 signature_delta，
                // 然后关闭 thinking 块。signature 在 encrypted_content 字段。
                if self.open == OpenBlock::Thinking || self.thinking_stop_pending {
                    if let Some(sig) = ev
                        .get("item")
                        .and_then(|i| i.get("encrypted_content"))
                        .and_then(Value::as_str)
                    {
                        if !sig.is_empty() {
                            // signature_delta：Anthropic 端 thinking 块的签名增量
                            out.push_str(&sse_event(
                                "content_block_delta",
                                &json!({
                                    "type": "content_block_delta",
                                    "index": self.block_index,
                                    "delta": { "type": "signature_delta", "signature": sig }
                                }),
                            ));
                        }
                    }
                    self.finalize_pending_thinking(out);
                    self.finalize_open(out);
                }
            }
            "message" => {
                // 非流式 message 项：上游把整段文本一次性放在 item.content[].text 里。
                // 整段转成一次 text_delta + content_block_stop。
                self.finalize_pending_thinking(out);
                self.finalize_open(out);
                let texts = collect_message_text(ev.get("item"));
                if !texts.is_empty() {
                    out.push_str(&sse_event(
                        "content_block_start",
                        &ev_content_block_start_text(self.block_index),
                    ));
                    out.push_str(&sse_event(
                        "content_block_delta",
                        &ev_content_block_delta_text(self.block_index, &texts),
                    ));
                    out.push_str(&sse_event(
                        "content_block_stop",
                        &ev_content_block_stop(self.block_index),
                    ));
                    self.block_index += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // --- B14/B15 ---
    fn on_completed(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        self.finalize_pending_thinking(out);
        self.finalize_open(out);
        // stop_reason 取上游 response.status 或 response.incomplete_details.reason
        self.stop_reason = map_stop_reason(ev);
        // usage
        if let Some(u) = ev.get("response").and_then(|r| r.get("usage")) {
            self.usage_available = true;
            self.input_tokens = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
            self.raw_input_tokens = self.input_tokens;
            self.output_tokens = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
            // Responses usage 的 input_tokens 已含 cached_tokens；Anthropic 端要分列，
            // cached 部分从 input 里扣出来放到 cache_read（避免重复计数）。
            let cached = u
                .get("input_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if cached > 0 && cached <= self.input_tokens {
                self.cache_read = cached;
                self.input_tokens -= cached;
            }
        }
        self.emit_message_delta_and_stop(out);
        Ok(())
    }

    fn on_failed(&mut self, ev: &Value, out: &mut String) -> Result<(), String> {
        // response.failed：升级为错误事件
        let msg = ev
            .get("response")
            .and_then(|r| r.get("error"))
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("上游 response.failed");
        self.finished = true;
        self.terminal_outcome = TerminalOutcome::Failure;
        self.finalize_open(out);
        out.push_str(&sse_event(
            "error",
            &json!({
                "type": "error",
                "error": { "type": "upstream_error", "message": msg }
            }),
        ));
        Ok(())
    }

    fn on_error_event(&mut self, ev: &Value) -> Result<(), String> {
        let msg = ev
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| ev.get("error").and_then(Value::as_str))
            .unwrap_or("上游返回 error 事件");
        self.finished = true;
        self.terminal_outcome = TerminalOutcome::Failure;
        Err(msg.to_string())
    }

    // ===== 收尾工具 =====

    /// 关闭「等待 stop 的 thinking 块」：若 thinking_stop_pending 且当前正开着 thinking，
    /// 发 content_block_stop 并清状态。不立刻关：必须等到真正原因（sig/next/completed）。
    fn finalize_pending_thinking(&mut self, out: &mut String) {
        if self.thinking_stop_pending && self.open == OpenBlock::Thinking {
            out.push_str(&sse_event(
                "content_block_stop",
                &ev_content_block_stop(self.block_index),
            ));
            self.open = OpenBlock::None;
            self.block_index += 1;
            self.thinking_stop_pending = false;
        }
    }

    /// 关闭任意非 None 的打开块（text/tool_use 直接关；thinking 也直接关并清 pending）。
    fn finalize_open(&mut self, out: &mut String) {
        match self.open {
            OpenBlock::None => {}
            _ => {
                out.push_str(&sse_event(
                    "content_block_stop",
                    &ev_content_block_stop(self.block_index),
                ));
                self.open = OpenBlock::None;
                self.block_index += 1;
                self.thinking_stop_pending = false;
            }
        }
    }

    fn emit_message_delta_and_stop(&mut self, out: &mut String) {
        out.push_str(&sse_event(
            "message_delta",
            &ev_message_delta(
                &self.stop_reason,
                self.input_tokens,
                self.output_tokens,
                self.cache_read,
            ),
        ));
        out.push_str(&sse_event("message_stop", &ev_message_stop()));
        self.finished = true;
        self.terminal_outcome = TerminalOutcome::Success;
    }
}

/// 喂一行的结果。
#[derive(Debug)]
pub enum FeedOutcome {
    /// 应发送的事件串 + 是否遇到 [DONE]。
    Events(String, bool),
}

impl FeedOutcome {
    fn events(s: String, done: bool) -> Self {
        FeedOutcome::Events(s, done)
    }
}

// ===== 纯函数工具 =====

/// 从 reasoning_summary_text.delta / reasoning_text.delta 事件里取文本。
/// 上游两种写法：`{delta:"..."}` 或 `{summary_text:"..."}` / `{text:"..."}`，都兼容。
fn pick_summary_text_delta(ev: &Value) -> Option<String> {
    if let Some(s) = ev.get("delta").and_then(Value::as_str) {
        return Some(s.to_string());
    }
    if let Some(s) = ev.get("summary_text").and_then(Value::as_str) {
        return Some(s.to_string());
    }
    if let Some(s) = ev.get("text").and_then(Value::as_str) {
        return Some(s.to_string());
    }
    None
}

/// 取 message 项（非流式 output_item.done）里所有 output_text 文本拼起来。
fn collect_message_text(item: Option<&Value>) -> String {
    let Some(item) = item else {
        return String::new();
    };
    let Some(content) = item.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    let mut buf = String::new();
    for part in content {
        let ty = part.get("type").and_then(Value::as_str).unwrap_or("");
        if ty == "output_text" {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                buf.push_str(t);
            }
        }
    }
    buf
}

/// response.completed/incomplete -> Anthropic stop_reason 映射。
fn map_stop_reason(ev: &Value) -> String {
    let resp = ev.get("response");
    let status = resp.and_then(|r| r.get("status")).and_then(Value::as_str);
    match status {
        Some("completed") => "end_turn".to_string(),
        Some("incomplete") => {
            // incomplete 的 reason 更细：max_output_tokens / content_filter / ...
            let reason = resp
                .and_then(|r| r.get("incomplete_details"))
                .and_then(|d| d.get("reason"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match reason {
                "max_output_tokens" => "max_tokens".to_string(),
                "content_filter" => "content_filter".to_string(),
                _ => "max_tokens".to_string(),
            }
        }
        _ => "end_turn".to_string(),
    }
}

// =====================================================================
// 单元测试：覆盖 B1-B19 主路径
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn line(data: &str) -> String {
        format!("data: {data}")
    }

    fn feed(state: &mut StreamState, data: &str) -> String {
        match state.feed_line(&line(data)).unwrap() {
            FeedOutcome::Events(s, _) => s,
        }
    }

    #[test]
    fn b1_response_created_emits_message_start_once() {
        let mut s = StreamState::new("claude-sonnet-4", 1);
        // created 之前喂空行不应发 message_start
        let empty = s.feed_line("event: response.created").unwrap();
        assert!(empty.events_or_empty().is_empty());
        let out = feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"resp_abc"}}"#,
        );
        assert!(out.contains("event: message_start"));
        assert!(out.contains("msg_grok_1"));
        // 再来一次 created 不再发
        let out2 = feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"resp_def"}}"#,
        );
        assert!(!out2.contains("message_start"));
    }

    #[test]
    fn b2_b3_b4_thinking_block_lifecycle() {
        let mut s = StreamState::new("m", 0);
        feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"r"}}"#,
        );
        let out = feed(
            &mut s,
            r#"{"type":"response.reasoning_summary_part.added","summary_index":0}"#,
        );
        assert!(out.contains("content_block_start"));
        assert!(out.contains("\"type\":\"thinking\""));
        let out = feed(
            &mut s,
            r#"{"type":"response.reasoning_summary_text.delta","delta":"思考中"}"#,
        );
        assert!(out.contains("thinking_delta"));
        assert!(out.contains("思考中"));
        // part.done 不立刻 stop
        let out = feed(&mut s, r#"{"type":"response.reasoning_summary_part.done"}"#);
        assert!(!out.contains("content_block_stop"));
        // content_part.added(output_text) 先收尾 thinking 再开 text
        let out = feed(
            &mut s,
            r#"{"type":"response.content_part.added","part":{"type":"output_text"}}"#,
        );
        assert!(out.contains("content_block_stop"), "应先关 thinking");
        assert!(out.contains("\"type\":\"text\""));
    }

    #[test]
    fn b6_b7_b8_text_block() {
        let mut s = StreamState::new("m", 0);
        feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"r"}}"#,
        );
        let out = feed(
            &mut s,
            r#"{"type":"response.content_part.added","part":{"type":"output_text"}}"#,
        );
        assert!(out.contains("\"type\":\"text\""));
        let out = feed(
            &mut s,
            r#"{"type":"response.output_text.delta","delta":"Hi"}"#,
        );
        assert!(out.contains("text_delta") && out.contains("Hi"));
        let out = feed(
            &mut s,
            r#"{"type":"response.content_part.done","part":{"type":"output_text"}}"#,
        );
        assert!(out.contains("content_block_stop"));
    }

    #[test]
    fn b9_b10_b11_function_call_block() {
        let mut s = StreamState::new("m", 0);
        feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"r"}}"#,
        );
        let out = feed(
            &mut s,
            r#"{"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_1","name":"get_weather"}}"#,
        );
        assert!(out.contains("tool_use"));
        assert!(out.contains("get_weather"));
        assert!(out.contains("input_json_delta"));
        let out = feed(
            &mut s,
            r#"{"type":"response.function_call_arguments.delta","delta":"{\"city\"" }"#,
        );
        assert!(out.contains("input_json_delta"));
        let out = feed(
            &mut s,
            r#"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_1"}}"#,
        );
        assert!(out.contains("content_block_stop"));
    }

    #[test]
    fn b13_non_streaming_message_one_shot() {
        let mut s = StreamState::new("m", 0);
        feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"r"}}"#,
        );
        let out = feed(
            &mut s,
            r#"{"type":"response.output_item.done","item":{"type":"message","content":[{"type":"output_text","text":"整段回答"}]}}"#,
        );
        // 一次 text_delta + content_block_stop
        assert!(out.contains("整段回答"));
        assert!(out.contains("content_block_stop"));
    }

    #[test]
    fn b14_completed_emits_message_delta_and_stop_with_usage() {
        let mut s = StreamState::new("m", 0);
        feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"r"}}"#,
        );
        let out = feed(
            &mut s,
            r#"{"type":"response.completed","response":{"id":"resp_1","status":"completed","usage":{"input_tokens":120,"output_tokens":50,"input_tokens_details":{"cached_tokens":20}}}}"#,
        );
        assert!(out.contains("message_delta"));
        assert!(out.contains("\"stop_reason\":\"end_turn\""));
        assert!(out.contains("message_stop"));
        // cached_tokens 从 input 扣除进 cache_read
        assert!(out.contains("\"input_tokens\":100"));
        assert!(out.contains("\"cache_read_input_tokens\":20"));
        // 已经 finished，再喂忽略
        let out2 = feed(
            &mut s,
            r#"{"type":"response.completed","response":{"id":"r2"}}"#,
        );
        assert!(out2.is_empty());
    }

    #[test]
    fn incomplete_max_output_tokens_maps_to_max_tokens() {
        let mut s = StreamState::new("m", 0);
        feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"r"}}"#,
        );
        let out = feed(
            &mut s,
            r#"{"type":"response.incomplete","response":{"id":"r","status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}}"#,
        );
        assert!(out.contains("\"stop_reason\":\"max_tokens\""));
    }

    #[test]
    fn error_event_returns_fatal() {
        let mut s = StreamState::new("m", 0);
        let r = s.feed_line(&line(r#"{"type":"error","message":"rate limited"}"#));
        match r {
            Err(msg) => assert_eq!(msg, "rate limited"),
            Ok(_) => panic!("error 事件应以 Err 返回致命错误"),
        }
    }

    #[test]
    fn done_marker_returns_done_flag() {
        let mut s = StreamState::new("m", 0);
        match s.feed_line("data: [DONE]").unwrap() {
            FeedOutcome::Events(_, true) => {}
            _ => panic!("应返回 done=true"),
        }
    }

    #[test]
    fn finish_emits_default_terminus_when_upstream_closed_early() {
        let mut s = StreamState::new("m", 0);
        feed(
            &mut s,
            r#"{"type":"response.created","response":{"id":"r"}}"#,
        );
        // 开一个 text 块但不发 completed 就断
        feed(
            &mut s,
            r#"{"type":"response.content_part.added","part":{"type":"output_text"}}"#,
        );
        let out = s.finish();
        assert!(out.contains("content_block_stop"));
        assert!(out.contains("message_delta") && out.contains("message_stop"));
    }

    // 小工具：把 FeedOutcome 的串取出来便于上面空行用例
    impl FeedOutcome {
        fn events_or_empty(&self) -> String {
            match self {
                FeedOutcome::Events(s, _) => s.clone(),
            }
        }
    }
}
