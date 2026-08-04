// Anthropic Messages ↔ OpenAI Responses 协议转换（grok 上游专用）。
//
// 与 nvidia/converter 不同：grok 上游（cli-chat-proxy.grok.com 与 api.x.ai）
// 走的是 **OpenAI Responses API**（POST /v1/responses），不是 Chat Completions。
// Responses 的输入是线性的 `input[]`（含 message / reasoning / function_call /
// function_call_output 等条目），而非 Chat 的 `messages[]`。因此转换逻辑独立成这
// 份文件，不复用 nvidia 的 Chat Completions converter——两套 schema 差异够大，
// 强行复用反而难读。
//
// 本 Phase 2 先实现**请求侧**（Anthropic Messages -> Responses 请求体），让 API
// Key 退路（api.x.ai）能发出一个被上游接受的非流式 / 流式请求。响应侧 SSE 状态机
// （Responses SSE -> Anthropic SSE）见 stream.rs，紧随其后接入。
//
// 下方 `pub fn` 在 Phase 2 内被 proxy.rs/server.rs 接线调用；当前尚未接线，整模块
// 先放行 dead_code 以过 clippy -D warnings 门禁，接线后无需移除（allow 无副作用）。
#![allow(dead_code)]
//
// 字段映射依据 CLIProxyAPI `codex_claude_request.go`（ConvertClaudeRequestToCodex）：
//   - system（字符串或 [{type:text}] 数组）-> `input[]` 首条 `{type:message,role:developer,
//     content:[{type:input_text,text}]}`；顶层 `instructions` 保留空串占位（xAI executor
//     的 normalizeCodexInstructions 要求非 null，保留空串即可）。
//   - messages[].role=user 文本块 -> input message role=user，
//     content:{type:input_text,text}
//   - assistant 文本块 -> role=assistant，content:{type:output_text,text}
//   - assistant thinking 块 -> 独立 `{type:reasoning,summary:[],content:null,
//     encrypted_content:<signature>}` 条目（**丢弃 thinking 文本本身**，只回传 signature
//     作 encrypted_content；首轮无 thinking 块，故首轮请求无 encrypted_content）。
//   - tool_use 块 -> {type:function_call,call_id,name,arguments(JSON 字符串)}；
//     arguments 取 Anthropic `input` 原始 JSON 文本（对象经序列化等价，纯字符串原样）。
//   - tool_result 块 -> {type:function_call_output,call_id,output}：content 是数组时，
//     text 块->input_text、image 块->input_image 拼成数组，**output 取数组**（保留二进制）；
//     无可用条目或 content 是字符串时 output 取字符串值。
//   - image 块 -> content:{type:input_image,image_url:data URI}（base64 拼成 data URI）。
//   - tools[] -> {type:function,name,description,parameters,strict:false}；
//     parameters 经规整（缺则补 {type:object,properties:{}}）。
//   - tool_choice -> auto / required / none / {type:"function",name}。
//   - thinking 配置 -> reasoning.effort（minimal/low/medium/high/xhigh +2 特例 none/auto）
//     + reasoning.summary="auto" + include:["reasoning.encrypted_content"]。
//   - parallel_tool_calls：默认 true，tool_choice.disable_parallel_tool_use=true 时改 false。
//   - max_tokens -> max_output_tokens；temperature/top_p 透传；stream 透传。
//     （CLIProxyAPI 在 Claude->Codex 路径丢弃这三项，是因 codex CLI 恒流式且不传采样；
//      本通用 Anthropic 代理透传更正确，**有意偏离参考实现对齐 Responses API 标准**。）
//   - 剥离上游不认的字段：stream_options、prompt_cache_retention、safety_identifier、
//     previous_response_id（无状态每轮重发 input，不用 response id 链）。
//   - store:false（无状态语义，对齐 CLIProxyAPI）。

use crate::nvidia::models::AnthropicRequest;
use serde_json::{json, Value};

// ===== 请求转换：Anthropic Messages -> Responses 请求体 =====

/// 把一个 Anthropic `/v1/messages` 请求体转换成 grok 上游 `/v1/responses` 请求体。
///
/// `target_model` 为已映射好的 grok slug（由调用方用 GrokConfig::map_model 决定）。
/// 入参 `req` 的未知字段一律忽略（AnthropicRequest 不使用 deny_unknown_fields）。
///
/// 返回的 Value 直接作为 reqwest 请求体序列化发送。
pub fn build_responses_request(req: &AnthropicRequest, target_model: &str) -> Value {
    // 模板白名单制（与 CLIProxyAPI 对齐）：顶层只放这些字段，其余全部不出现，
    // 杜绝 Anthropic 侧的 stream_options/prompt_cache_retention/safety_identifier
    // /previous_response_id 被透传到上游。
    let mut out = json!({
        "model": target_model,
        "instructions": "",
        "stream": req.is_stream(),
        // 无状态：不启用 Responses 的服务端会话存储，每轮靠 input[] 全量重发上下文。
        "store": false,
    });

    // input[]：先放 system -> developer message，再线性化对话/工具调用序列。
    let mut input = Vec::new();
    if let Some(dev) = build_developer_message(&req.system) {
        input.push(dev);
    }
    input.extend(build_input(req));
    if !input.is_empty() {
        out["input"] = Value::Array(input);
    }

    // tools[]：Anthropic {name,description,input_schema} -> Responses {type:function,...} （parameters 规整）
    if let Some(tools) = build_tools(&req.tools) {
        out["tools"] = tools;
    }

    // tool_choice：auto/any/tool -> auto/required/none/{type:function,name}
    if let Some(tc) = build_tool_choice(&req.tool_choice) {
        out["tool_choice"] = tc;
    }

    // parallel_tool_calls：默认 true；Anthropic tool_choice.disable_parallel_tool_use
    // 是反向布尔（true=禁用并行），取反为正向。仅在定义了 tools 时才有意义。
    let has_tools = req.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
    if has_tools {
        let parallel = !req
            .tool_choice
            .as_ref()
            .and_then(|tc| tc.get("disable_parallel_tool_use"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out["parallel_tool_calls"] = json!(parallel);
    }

    // reasoning：thinking 配置 -> reasoning.effort + reasoning.summary="auto"。
    // 同时请求服务端返回加密 reasoning（include），供多轮回传 encrypted_content。
    if let Some(reasoning) = build_reasoning(&req.thinking) {
        out["reasoning"] = reasoning;
        // CLIProxyAPI 始终带上 include；xAI executor 会按模型支持情况剥离。
        // 本实现跟随：只要带了 reasoning 字段就请求 encrypted_content 回传。
        out["include"] = json!(["reasoning.encrypted_content"]);
    }

    // 透传 / 重命名字段。
    if let Some(max) = req.max_tokens {
        out["max_output_tokens"] = json!(max);
    }
    if let Some(t) = req.temperature {
        out["temperature"] = json!(t);
    }
    if let Some(p) = req.top_p {
        out["top_p"] = json!(p);
    }
    if let Some(stops) = req.stop_sequences.as_ref() {
        if !stops.is_empty() {
            // xAI Responses 接受 stop 透传（OpenAI 标准）；不强行剥离。
            out["stop"] = Value::Array(stops.iter().cloned().map(Value::String).collect());
        }
    }

    out
}

/// 把 Anthropic `system`（字符串 / [{type:"text",text:...}] 数组）转换为 Responses
/// `input[]` 的首条 developer message 条目：`{type:message,role:developer,
/// content:[{type:input_text,text}]}`（CLIProxyAPI 的真实做法，system 不进顶层
/// `instructions` 字段）。null / 空 text -> None（不发该条目）。
pub fn build_developer_message(system: &Option<Value>) -> Option<Value> {
    let s = system.as_ref()?;
    let text = match s {
        Value::String(t) => t.clone(),
        Value::Array(blocks) => {
            let mut buf = String::new();
            for b in blocks {
                if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                        if !buf.is_empty() {
                            buf.push_str("\n\n");
                        }
                        buf.push_str(t);
                    }
                }
            }
            buf
        }
        _ => return None,
    };
    if text.trim().is_empty() {
        None
    } else {
        Some(json!({
            "type": "message",
            "role": "developer",
            "content": [{ "type": "input_text", "text": text }],
        }))
    }
}

/// 把 Anthropic messages[] 线性化为 Responses `input[]`。
///
/// 单个 assistant message 若含 {text, tool_use} 两块，会产出一条 assistant
/// message 条目 + 一条 function_call 条目，顺序保持原 block 顺序。
///
/// 拆分规则（每条 Anthropic message 的 content 数组被展开）：
/// - user 文本/图片 -> role=user 的 input_text/input_image content；
/// - assistant 文本 -> role=assistant 的 output_text content；
/// - thinking / tool_use / tool_result -> 各自独立的顶层条目（reasoning /
///   function_call / function_call_output），不绑在 message content 里。
pub fn build_input(req: &AnthropicRequest) -> Vec<Value> {
    let mut out = Vec::new();
    for msg in &req.messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content");
        match content {
            Some(Value::String(t)) => {
                // 整条 message 是纯文本：按 role 映射 input_text / output_text。
                let block_type = if role == "assistant" {
                    "output_text"
                } else {
                    "input_text"
                };
                out.push(json!({
                    "type": "message",
                    "role": role,
                    "content": [{ "type": block_type, "text": t }],
                }));
            }
            Some(Value::Array(blocks)) => {
                // 先把同 message 的 text/image 块合并成一条 message 条目，
                // thinking/tool_use/tool_result 则拆成独立顶层条目（夹在 message 之间，
                // 保持 block 原顺序）。
                let mut msg_parts: Vec<Value> = Vec::new();
                let mut had_msg_part = false;
                for block in blocks {
                    let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match btype {
                        "text" => {
                            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                if !t.trim().is_empty() {
                                    let bt = if role == "assistant" {
                                        "output_text"
                                    } else {
                                        "input_text"
                                    };
                                    msg_parts.push(json!({ "type": bt, "text": t }));
                                    had_msg_part = true;
                                }
                            }
                        }
                        "image" => {
                            // 仅 user 角色发图；assistant 图片上传忽略。
                            if role == "user" {
                                if let Some(img) = claude_image_to_responses(block) {
                                    msg_parts.push(img);
                                    had_msg_part = true;
                                }
                            }
                        }
                        "thinking" => {
                            // thinking 块 -> 独立 reasoning 条目。先把已累积的 message
                            // parts flush，再发 reasoning，保证顺序正确。
                            flush_message(&mut out, role, &mut msg_parts, had_msg_part);
                            if let Some(r) = claude_thinking_to_reasoning(block) {
                                out.push(r);
                            }
                            had_msg_part = false;
                        }
                        "tool_use" => {
                            flush_message(&mut out, role, &mut msg_parts, had_msg_part);
                            if let Some(fc) = claude_tool_use_to_function_call(block) {
                                out.push(fc);
                            }
                            had_msg_part = false;
                        }
                        "tool_result" => {
                            // tool_result 永远独立成 function_call_output（不进 message content）。
                            flush_message(&mut out, role, &mut msg_parts, had_msg_part);
                            if let Some(fco) = claude_tool_result_to_call_output(block) {
                                out.push(fco);
                            }
                            had_msg_part = false;
                        }
                        _ => { /* 未知块忽略 */ }
                    }
                }
                flush_message(&mut out, role, &mut msg_parts, had_msg_part);
            }
            _ => { /* content 缺省 -> 跳过该条 message */ }
        }
    }
    out
}

/// 把已累积的 message parts 收尾成一条 input message 条目。
/// 若无 part（thinking/tool_use 等已 flush 走的空 message），则不发空条目。
fn flush_message(out: &mut Vec<Value>, role: &str, parts: &mut Vec<Value>, had: bool) {
    if had && !parts.is_empty() {
        out.push(json!({
            "type": "message",
            "role": role,
            "content": std::mem::take(parts),
        }));
    } else {
        parts.clear();
    }
}

/// Anthropic image 块 -> Responses input_image content 段。
/// base64 源转 data URI（同 nvidia converter 思路），url 源直传。
fn claude_image_to_responses(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    match source.get("type").and_then(|v| v.as_str()) {
        Some("base64") => {
            let media = source
                .get("media_type")
                .and_then(|v| v.as_str())
                .unwrap_or("application/octet-stream");
            let data = source.get("data").and_then(|v| v.as_str())?;
            if data.is_empty() {
                return None;
            }
            Some(json!({
                "type": "input_image",
                "image_url": format!("data:{media};base64,{data}"),
            }))
        }
        Some("url") => {
            let url = source.get("url").and_then(|v| v.as_str())?;
            if url.is_empty() {
                return None;
            }
            Some(json!({ "type": "input_image", "image_url": url }))
        }
        _ => None,
    }
}

/// Anthropic thinking 块 -> Responses 独立 reasoning 条目。
///
/// 与 CLIProxyAPI 对齐：**丢弃 thinking 文本本身**（reasoning 项的 `summary` 为空数组、
/// `content` 为 null），只把 Anthropic 的 `signature` 作为 `encrypted_content` 回传——
/// 上游见 `encrypted_content` 即复用上一轮推理状态，无需重建。
///
/// 首轮请求无 assistant thinking 块，故无 encrypted_content；多轮时上一轮的
/// signature_delta 经 Anthropic 客户端保存为 thinking 块的 `signature`，回到此函数回传。
///
/// 注：CLIProxyAPI 在 Claude↔GPT 路径额外做 `CompatibleSignatureForProvider` 仅放行
/// `gAAAA` 前缀的 GPT 格式 signature；那是跨 provider 互转的特殊处理。本代理
/// grok↔grok 的 signature 上游原生格式（unpadded base64、非 gAAAA），无需兼容过滤，
/// 有即透传，无即整块丢弃（与上游无 reasoning 状态可复用等价）。
pub fn claude_thinking_to_reasoning(block: &Value) -> Option<Value> {
    let signature = block.get("signature").and_then(|v| v.as_str())?;
    if signature.trim().is_empty() {
        // 无 signature：无加密推理可回传，丢弃该块（首轮通常如此）。
        return None;
    }
    Some(json!({
        "type": "reasoning",
        "summary": [],
        "content": Value::Null,
        "encrypted_content": signature,
    }))
}

/// Anthropic tool_use 块 -> Responses function_call 条目。
/// `arguments` 序列化为 JSON 字符串（Responses 要求字符串，非对象）。
fn claude_tool_use_to_function_call(block: &Value) -> Option<Value> {
    let call_id = block.get("id").and_then(|v| v.as_str())?.to_string();
    let name = block.get("name").and_then(|v| v.as_str())?.to_string();
    let input = block.get("input");
    // arguments：input 缺省视为 "{}"，对象/数组序列化为字符串，纯字符串原样。
    let arguments = match input {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => "{}".to_string(),
    };
    Some(json!({
        "type": "function_call",
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
    }))
}

/// Anthropic tool_result 块 -> Responses function_call_output 条目。
///
/// 文档块（type=document）在 CLIProxyAPI 的 Responses 路径不处理（无 document
/// case），本实现也跳过。
///
/// `output` 规则（与 CLIProxyAPI 对齐）：
/// - content 是数组：遍历逐块，text -> input_text、image -> input_image（保留二进制，
///   不降级文字），若有可用项则 output 取**数组**；无可用项回退 content 的字符串值。
/// - content 不是数组：output 取 content 的字符串值。
pub fn claude_tool_result_to_call_output(block: &Value) -> Option<Value> {
    let call_id = block
        .get("tool_use_id")
        .and_then(|v| v.as_str())?
        .to_string();
    let output = match block.get("content") {
        Some(Value::Array(blocks)) => {
            let mut parts: Vec<Value> = Vec::new();
            for b in blocks {
                match b.get("type").and_then(|v| v.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                            parts.push(json!({ "type": "input_text", "text": t }));
                        }
                    }
                    Some("image") => {
                        if let Some(img) = claude_image_to_responses(b) {
                            parts.push(img);
                        }
                    }
                    _ => {}
                }
            }
            if parts.is_empty() {
                Value::String(String::new())
            } else {
                Value::Array(parts)
            }
        }
        None => Value::String(String::new()),
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(other) => Value::String(other.to_string()),
    };
    Some(json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output,
    }))
}

/// Anthropic tools[] -> Responses tools[]（每个 {type:function,name,description,
/// parameters,strict:false}）。input_schema 经规整后作为 parameters（与 CLIProxyAPI
/// 的 normalizeToolParameters 对齐：保证有 type:object 和 properties）。
pub fn build_tools(tools: &Option<Vec<Value>>) -> Option<Value> {
    let t = tools.as_ref()?;
    if t.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(t.len());
    for tool in t {
        let name = tool.get("name").and_then(|v| v.as_str());
        if name.is_none() {
            continue;
        }
        let parameters = tool
            .get("input_schema")
            .cloned()
            .map(normalize_tool_parameters)
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        let mut entry = json!({
            "type": "function",
            "name": name.unwrap(),
            "parameters": parameters,
            "strict": false,
        });
        if let Some(desc) = tool.get("description").and_then(|v| v.as_str()) {
            entry["description"] = Value::String(desc.to_string());
        }
        out.push(entry);
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Array(out))
    }
}

/// 规整工具 parameters（CLIProxyAPI normalizeToolParameters 等价）：
/// - null/空对象 -> {type:object,properties:{}}；
/// - 缺 type -> 补 type:object；
/// - type 是 object 但缺 properties -> 补空 properties:{}；
/// - 其余字段原样保留。
fn normalize_tool_parameters(mut schema: Value) -> Value {
    if !schema.is_object() {
        return json!({ "type": "object", "properties": {} });
    }
    let needs_type = schema.get("type").and_then(|v| v.as_str()).is_none();
    if needs_type {
        schema["type"] = Value::String("object".to_string());
    }
    let is_object = schema
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s == "object")
        .unwrap_or(false);
    if is_object && schema.get("properties").is_none() {
        schema["properties"] = json!({});
    }
    schema
}

/// Anthropic tool_choice -> Responses tool_choice。
///   auto -> "auto"；any -> "required"；none/logic缺省无 tools 时不发；
///   {type:"tool",name:"x"} -> {type:"function",name:"x"}。
pub fn build_tool_choice(tool_choice: &Option<Value>) -> Option<Value> {
    let tc = tool_choice.as_ref()?;
    let ty = tc.get("type").and_then(|v| v.as_str()).unwrap_or("auto");
    match ty {
        "auto" => Some(Value::String("auto".to_string())),
        "any" => Some(Value::String("required".to_string())),
        "none" => Some(Value::String("none".to_string())),
        "tool" => {
            let name = tc.get("name").and_then(|v| v.as_str())?;
            Some(json!({ "type": "function", "name": name }))
        }
        _ => Some(Value::String("auto".to_string())),
    }
}

/// Anthropic thinking 配置 -> Responses `reasoning` 字段（与 CLIProxyAPI 的
/// `ConvertBudgetToLevel` 阈值对齐）。
/// - enabled + budget_tokens -> effort，分档：
///   budget=-1 -> "auto"（CLIProxyAPI 特例；Anthropic 无 -1，但留映射）；
///   budget=0 -> "none"；≤512 -> "minimal"；≤1024 -> "low"；≤8192 -> "medium"；
///   ≤24576 -> "high"；≥24577 -> "xhigh"。
///   无 budget_tokens 的 enabled -> "medium"（CLIProxyAPI 默认）。
/// - adaptive / auto -> "xhigh"（CLIProxyAPI LevelXHigh）。
/// - disabled -> "none"。
/// - 完全没传 thinking 字段 -> None（不发 reasoning 字段，沿用上游默认，与上面
///   "enabled 默认 medium" 区别开：用户显式开启才做事）。
///
/// 同时附带 `reasoning.summary = "auto"`（CLIProxyAPI 固定值，要求上游自动产 reasoning
/// summary 以喂给 SSE 的 reasoning_summary_* 事件）。
pub fn build_reasoning(thinking: &Option<Value>) -> Option<Value> {
    let t = thinking.as_ref()?;
    let ty = t.get("type").and_then(|v| v.as_str())?;
    // 完全无 thinking 配置（type 缺省）-> 不发 reasoning。
    let effort = match ty {
        "enabled" => {
            let budget = t
                .get("budget_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(i64::MAX); // 缺省 budget 视为「足够大」-> medium 默认档
            budget_to_effort(budget)
        }
        "adaptive" | "auto" => "xhigh".to_string(),
        "disabled" => {
            // disabled 显式禁用推理 -> none（而非不发字段，让上游明确关推理）。
            "none".to_string()
        }
        _ => return None, // 未知 type -> 不发 reasoning
    };
    Some(json!({ "effort": effort, "summary": "auto" }))
}

/// budget_tokens -> reasoning.effort，与 CLIProxyAPI `internal/thinking/convert.go`
/// 的 ConvertBudgetToLevel 阈值一致。
fn budget_to_effort(budget: i64) -> String {
    match budget {
        -1 => "auto".to_string(),
        0 => "none".to_string(),
        b if b <= 512 => "minimal".to_string(),
        b if b <= 1024 => "low".to_string(),
        b if b <= 8192 => "medium".to_string(),
        b if b <= 24576 => "high".to_string(),
        _ => "xhigh".to_string(),
    }
}

// =====================================================================
// 端到端构建：从 Anthropic 请求 bytes -> Responses 请求 bytes（便利函数）
// =====================================================================

/// 从 Anthropic `/v1/messages` 的原始 JSON 字节解析并构造 grok Responses 请求体，
/// 失败时返回可展示给前端的中文错误。proxy.rs 入口直接调用此函数。
pub fn convert_request_body(anthropic_bytes: &[u8], target_model: &str) -> Result<Value, String> {
    let req: AnthropicRequest = serde_json::from_slice(anthropic_bytes)
        .map_err(|e| format!("❌ 解析 Anthropic 请求失败: {e}"))?;
    Ok(build_responses_request(&req, target_model))
}

// =====================================================================
// 响应转换：Responses 非流式 JSON -> Anthropic Messages JSON
// =====================================================================
//
// 上游 /v1/responses 非流式响应体形如：
//   { "id":"resp_...", "object":"response", "status":"completed",
//     "output":[ {"type":"reasoning",...}?, {"type":"message","content":[{output_text}]},
//                {"type":"function_call","call_id","name","arguments"} ... ],
//     "usage":{"input_tokens","output_tokens","input_tokens_details":{"cached_tokens"}} }
//
// 本函数把它一次性聚合成 Anthropic /v1/messages 非流式响应体：
//   { id, type:"message", role:"assistant", model, content[], stop_reason, stop_sequence:null, usage }
// content[] 顺序与 output[] 出现顺序对齐：reasoning->thinking 块、message->text 块、
// function_call->tool_use 块。thinking 块带 signature（取 encrypted_content）。

/// 把上游 Responses 非流式 JSON 转成 Anthropic Messages 非流式 JSON。
/// `model` 为对外（Anthropic 侧）展示的模型名；`msg_id` 为本地生成的 message id。
pub fn responses_json_to_anthropic(resp: &Value, model: &str, msg_id: &str) -> Value {
    let mut content: Vec<Value> = Vec::new();
    if let Some(output) = resp.get("output").and_then(Value::as_array) {
        for item in output {
            let ty = item.get("type").and_then(Value::as_str).unwrap_or("");
            match ty {
                "reasoning" => {
                    let sig = item
                        .get("encrypted_content")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    content.push(json!({
                        "type": "thinking",
                        "thinking": "",
                        "signature": sig
                    }));
                }
                "message" => {
                    let texts = collect_message_output_text(item);
                    if !texts.is_empty() {
                        content.push(json!({ "type": "text", "text": texts }));
                    }
                }
                "function_call" => {
                    let id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    let args_str = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input = serde_json::from_str::<Value>(args_str).unwrap_or(json!({}));
                    content.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input
                    }));
                }
                _ => {}
            }
        }
    }

    let stop_reason = map_nonstream_stop_reason(resp);
    let (input_tokens, output_tokens, cache_read) = map_usage(resp);

    let mut usage = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });
    if cache_read > 0 {
        usage["cache_read_input_tokens"] = json!(cache_read);
    }

    json!({
        "id": msg_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage
    })
}

/// 取 message 项（非流式）里所有 output_text 文本拼起来。proxy test_connection 复用。
pub fn collect_message_output_text(item: &Value) -> String {
    let Some(content) = item.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    let mut buf = String::new();
    for part in content {
        if part.get("type").and_then(Value::as_str) == Some("output_text") {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                buf.push_str(t);
            }
        }
    }
    buf
}

/// 非流式 Responses JSON -> Anthropic stop_reason（completed/incomplete/failed 等）。
fn map_nonstream_stop_reason(resp: &Value) -> String {
    let status = resp.get("status").and_then(Value::as_str);
    match status {
        Some("completed") => "end_turn".to_string(),
        Some("incomplete") => {
            let reason = resp
                .get("incomplete_details")
                .and_then(|d| d.get("reason"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match reason {
                "max_output_tokens" => "max_tokens".to_string(),
                "content_filter" => "content_filter".to_string(),
                _ => "max_tokens".to_string(),
            }
        }
        // failed / 其它：用 incomplete 语义兜底，前端能识别 stop_reason 非空即非异常
        _ => "end_turn".to_string(),
    }
}

/// 非流式 Responses usage -> (input_tokens, output_tokens, cache_read)。
fn map_usage(resp: &Value) -> (u64, u64, u64) {
    let Some(u) = resp.get("usage") else {
        return (0, 0, 0);
    };
    let mut input = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let output = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
    let cached = u
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if cached > 0 && cached <= input {
        input -= cached;
    }
    (input, output, cached)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req_of(value: Value) -> AnthropicRequest {
        serde_json::from_value(value).expect("test fixture 解析失败")
    }

    #[test]
    fn system_string_becomes_developer_message_not_instructions() {
        let req = req_of(json!({
            "model": "claude-sonnet-4",
            "system": "你是助手",
            "messages": [{"role":"user","content":"hi"}],
            "max_tokens": 100,
            "stream": true,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        // 顶层 instructions 保留空串占位（xAI executor 要求非 null）
        assert_eq!(out["instructions"], "");
        // system 进 input[0] 作为 developer message
        assert_eq!(out["input"][0]["type"], "message");
        assert_eq!(out["input"][0]["role"], "developer");
        assert_eq!(out["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(out["input"][0]["content"][0]["text"], "你是助手");
        assert_eq!(out["input"][1]["role"], "user");
        assert_eq!(out["model"], "grok-4.3");
        assert_eq!(out["stream"], true);
        assert_eq!(out["store"], false);
        assert_eq!(out["max_output_tokens"], 100);
    }

    #[test]
    fn system_text_array_concatenated_into_developer_message() {
        let req = req_of(json!({
            "system": [{"type":"text","text":"A"},{"type":"text","text":"B"}],
            "messages": [],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        assert_eq!(out["input"][0]["role"], "developer");
        assert_eq!(out["input"][0]["content"][0]["text"], "A\n\nB");
    }

    #[test]
    fn user_text_maps_to_input_text() {
        let req = req_of(json!({
            "messages": [{"role":"user","content":"hello"}],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let input = out["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "hello");
    }

    #[test]
    fn assistant_text_maps_to_output_text() {
        let req = req_of(json!({
            "messages": [{"role":"assistant","content":"ok"}],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        assert_eq!(out["input"][0]["role"], "assistant");
        assert_eq!(out["input"][0]["content"][0]["type"], "output_text");
    }

    #[test]
    fn tool_use_becomes_function_call_with_string_arguments() {
        let req = req_of(json!({
            "messages": [{
                "role": "assistant",
                "content": [{"type":"tool_use","id":"call_1","name":"read","input":{"path":"/tmp"}}]
            }],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let fc = &out["input"][0];
        assert_eq!(fc["type"], "function_call");
        assert_eq!(fc["call_id"], "call_1");
        assert_eq!(fc["name"], "read");
        assert_eq!(
            fc["arguments"], r#"{"path":"/tmp"}"#,
            "arguments 必须是 JSON 字符串"
        );
    }

    #[test]
    fn tool_result_becomes_function_call_output_string() {
        let req = req_of(json!({
            "messages": [{
                "role": "user",
                "content": [{"type":"tool_result","tool_use_id":"call_1","content":"文件内容"}]
            }],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let fco = &out["input"][0];
        assert_eq!(fco["type"], "function_call_output");
        assert_eq!(fco["call_id"], "call_1");
        assert_eq!(fco["output"], "文件内容");
    }

    #[test]
    fn tool_result_with_image_keeps_image_in_array() {
        // 与 CLIProxyAPI 对齐：tool_result 数组里有 image 块时，output 取数组
        // （保留二进制为 input_image 条目，不降级文字）。
        let req = req_of(json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type":"tool_result","tool_use_id":"c1",
                    "content":[{"type":"text","text":"部分文本"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}}]
                }]
            }],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let fco = &out["input"][0];
        assert_eq!(fco["type"], "function_call_output");
        assert_eq!(fco["call_id"], "c1");
        // output 是数组：[input_text, input_image]
        let output = fco["output"].as_array().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "input_text");
        assert_eq!(output[0]["text"], "部分文本");
        assert_eq!(output[1]["type"], "input_image");
        assert_eq!(output[1]["image_url"], "data:image/png;base64,AAAA");
    }

    #[test]
    fn thinking_becomes_reasoning_with_encrypted_content_and_empty_summary() {
        let req = req_of(json!({
            "messages": [{"role":"assistant","content":[
                {"type":"thinking","thinking":"我先想想","signature":"c2lnbmF0dXJl"},
                {"type":"text","text":"答"}
            ]}],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let input = out["input"].as_array().unwrap();
        // reasoning 条目在 assistant message 之前（保留块顺序）
        assert_eq!(input[0]["type"], "reasoning");
        // summary 是空数组（thinking 文本被丢弃），content 为 null
        assert_eq!(input[0]["summary"].as_array().unwrap().len(), 0);
        assert!(input[0]["content"].is_null());
        // encrypted_content 来自 thinking.signature
        assert_eq!(input[0]["encrypted_content"], "c2lnbmF0dXJl");
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
    }

    #[test]
    fn thinking_without_signature_is_dropped() {
        // 无 signature 的 thinking 块（首轮常见）-> 丢弃，不进 input。
        let req = req_of(json!({
            "messages": [{"role":"assistant","content":[
                {"type":"thinking","thinking":"想但没签名"},
                {"type":"text","text":"答"}
            ]}],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let input = out["input"].as_array().unwrap();
        // 只剩 assistant message，无 reasoning 条目
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "message");
    }

    #[test]
    fn tools_map_to_function_definitions_with_strict_false() {
        let req = req_of(json!({
            "messages": [{"role":"user","content":"go"}],
            "max_tokens": 10,
            "tools": [{"name":"read","description":"读文件","input_schema":{"type":"object","properties":{"path":{"type":"string"}}}}],
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "read");
        assert_eq!(tools[0]["description"], "读文件");
        assert_eq!(tools[0]["strict"], false);
        assert_eq!(tools[0]["parameters"]["type"], "object");
        // 配了 tools 时默认开启并行工具调用
        assert_eq!(out["parallel_tool_calls"], true);
    }

    #[test]
    fn tool_choice_mappings() {
        assert_eq!(
            build_tool_choice(&Some(json!({"type":"auto"}))),
            Some(Value::String("auto".into()))
        );
        assert_eq!(
            build_tool_choice(&Some(json!({"type":"any"}))),
            Some(Value::String("required".into()))
        );
        assert_eq!(
            build_tool_choice(&Some(json!({"type":"none"}))),
            Some(Value::String("none".into()))
        );
        assert_eq!(
            build_tool_choice(&Some(json!({"type":"tool","name":"read"}))),
            Some(json!({"type":"function","name":"read"}))
        );
    }

    #[test]
    fn reasoning_effort_budget_mapping_six_levels() {
        // 与 CLIProxyAPI ConvertBudgetToLevel 阈值对齐：
        assert_eq!(
            build_reasoning(&Some(json!({"type":"enabled","budget_tokens":256}))),
            Some(json!({"effort":"minimal","summary":"auto"}))
        );
        assert_eq!(
            build_reasoning(&Some(json!({"type":"enabled","budget_tokens":700}))),
            Some(json!({"effort":"low","summary":"auto"}))
        );
        assert_eq!(
            build_reasoning(&Some(json!({"type":"enabled","budget_tokens":5000}))),
            Some(json!({"effort":"medium","summary":"auto"}))
        );
        assert_eq!(
            build_reasoning(&Some(json!({"type":"enabled","budget_tokens":20000}))),
            Some(json!({"effort":"high","summary":"auto"}))
        );
        assert_eq!(
            build_reasoning(&Some(json!({"type":"enabled","budget_tokens":50000}))),
            Some(json!({"effort":"xhigh","summary":"auto"}))
        );
        // 预算 0（虽 enabled）-> none；负 1 -> auto（CLIProxyAPI 特例）
        assert_eq!(
            build_reasoning(&Some(json!({"type":"enabled","budget_tokens":0}))),
            Some(json!({"effort":"none","summary":"auto"}))
        );
        // enabled 但无 budget_tokens -> 默认 medium
        assert_eq!(
            build_reasoning(&Some(json!({"type":"enabled"}))),
            Some(json!({"effort":"medium","summary":"auto"}))
        );
        // adaptive / auto -> xhigh；disabled -> none
        assert_eq!(
            build_reasoning(&Some(json!({"type":"adaptive"}))),
            Some(json!({"effort":"xhigh","summary":"auto"}))
        );
        assert_eq!(
            build_reasoning(&Some(json!({"type":"disabled"}))),
            Some(json!({"effort":"none","summary":"auto"}))
        );
        // 完全无 thinking 配置 -> 不发 reasoning 字段
        assert_eq!(build_reasoning(&None), None);
    }

    #[test]
    fn parallel_tool_calls_depends_on_disable_flag() {
        // 默认（无 disable_parallel_tool_use）+ 有 tools -> true
        let req = req_of(json!({
            "messages": [{"role":"user","content":"go"}],
            "max_tokens": 10,
            "tools": [{"name":"read","input_schema":{"type":"object"}}],
        }));
        assert_eq!(
            build_responses_request(&req, "grok-4.3")["parallel_tool_calls"],
            true
        );
        // disable_parallel_tool_use=true -> false
        let req2 = req_of(json!({
            "messages": [{"role":"user","content":"go"}],
            "max_tokens": 10,
            "tools": [{"name":"read","input_schema":{"type":"object"}}],
            "tool_choice": {"type":"any","disable_parallel_tool_use": true},
        }));
        assert_eq!(
            build_responses_request(&req2, "grok-4.3")["parallel_tool_calls"],
            false
        );
        // 无 tools 时不发 parallel_tool_calls 字段
        let req3 = req_of(json!({"messages":[{"role":"user","content":"hi"}],"max_tokens":10}));
        assert!(build_responses_request(&req3, "grok-4.3")
            .get("parallel_tool_calls")
            .is_none());
    }

    #[test]
    fn reasoning_emit_triggers_include_field() {
        let req = req_of(json!({
            "messages": [{"role":"user","content":"hi"}],
            "max_tokens": 10,
            "thinking": {"type":"enabled","budget_tokens":5000},
        }));
        let out = build_responses_request(&req, "grok-4.3");
        assert_eq!(out["reasoning"]["effort"], "medium");
        assert_eq!(out["reasoning"]["summary"], "auto");
        assert_eq!(out["include"][0], "reasoning.encrypted_content");
    }

    #[test]
    fn image_block_user_only_becomes_input_image() {
        let req = req_of(json!({
            "messages": [{
                "role":"user",
                "content":[
                    {"type":"image","source":{"type":"base64","media_type":"image/png","data":"Qk=="}},
                    {"type":"text","text":"看图"}
                ]
            }],
            "max_tokens": 10,
        }));
        let out = build_responses_request(&req, "grok-4.3");
        let content = &out["input"][0]["content"];
        assert_eq!(content[0]["type"], "input_image");
        assert_eq!(content[0]["image_url"], "data:image/png;base64,Qk==");
        assert_eq!(content[1]["type"], "input_text");
        assert_eq!(content[1]["text"], "看图");
    }

    #[test]
    fn convert_request_body_from_bytes_roundtrip() {
        let bytes = br#"{"model":"claude-sonnet-4","messages":[{"role":"user","content":"hi"}],"max_tokens":10}"#;
        let out = convert_request_body(bytes, "grok-4.3").unwrap();
        assert_eq!(out["model"], "grok-4.3");
        assert_eq!(out["input"][0]["content"][0]["text"], "hi");
    }

    #[test]
    fn stream_options_and_unknown_fields_are_dropped() {
        // AnthropicRequest 不解析 stream_options/未知字段，转换后不会带过去。
        let req = req_of(json!({
            "messages": [{"role":"user","content":"hi"}],
            "max_tokens": 10,
            "stream_options": {"include_usage": true},
            "safety_identifier": "abc",
        }));
        let out = build_responses_request(&req, "grok-4.3");
        assert!(out.get("stream_options").is_none());
        assert!(out.get("safety_identifier").is_none());
    }
}
