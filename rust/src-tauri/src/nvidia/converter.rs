// 协议转换逻辑：Anthropic <-> OpenAI
//
// 请求方向（Anthropic -> OpenAI）：
//   - system（字符串或 block 数组）-> OpenAI system 消息（数组支持 cache_control）
//   - messages[].content 数组逐块展开：
//       text         -> OpenAI text 段（或拼为字符串）
//       image       -> OpenAI image_url 段（base64 转 data URI / 直传 url）
//       document    -> OpenAI file 段（base64 转 data URI）
//       thinking     -> OpenAI reasoning_content（仅 assistant 角色，防注入）
//       tool_use    -> OpenAI tool_calls
//       tool_result -> 拆为独立的 role=tool 文本消息；图片/文档降级为说明文字
//   - tools: Anthropic {name, description, input_schema} -> OpenAI {type:"function", function:{...}}（复制 cache_control）
//   - thinking 配置 -> OpenAI reasoning_effort
//   - max_tokens / temperature / top_p / stream 原样透传；流式请求显式申请末帧 usage
//   - stop_sequences -> OpenAI 的 stop
//
// 响应方向（OpenAI -> Anthropic）：
//   - 非流式：choices[].message.content/reasoning_content/tool_calls -> text/thinking/tool_use 块
//   - 流式：StreamState 管理动态 content block 索引，正确发出
//       text / thinking / tool_use 块的 start/delta/stop 事件序列
//   - Token 用量：cache_read_input_tokens（来自 prompt_tokens_details.cached_tokens），
//       input_tokens 扣除缓存部分；工具名按原始大小写还原（ToolNameMap）

use crate::nvidia::models::AnthropicRequest;
use serde_json::{json, Value};
use std::collections::HashMap;

// ===== 请求转换 =====

// 从 Anthropic 的 content（字符串或 block 数组）提取纯文本。
// 用于需要将 content 降级为纯文本的场景（如兜底）。
pub fn content_to_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let mut out = String::new();
            for block in arr {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                } else if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                    if let Some(c) = block.get("content") {
                        out.push_str(&content_to_text(c));
                    }
                }
            }
            out
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// 将 Anthropic content block（text/image/document）转为一个或多个 OpenAI content 段。
// 返回空 Vec 表示该块被忽略。
fn claude_block_to_openai_parts(block: &Value) -> Vec<Value> {
    let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match btype {
        "text" => {
            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                if !t.trim().is_empty() {
                    return vec![json!({ "type": "text", "text": t })];
                }
            }
            vec![]
        }
        "image" => {
            if let Some(url) = claude_image_to_openai_url(block) {
                return vec![json!({ "type": "image_url", "image_url": { "url": url } })];
            }
            vec![]
        }
        "document" => {
            if let Some(data) = claude_document_to_openai_filedata(block) {
                return vec![json!({ "type": "file", "file": { "file_data": data } })];
            }
            vec![]
        }
        _ => vec![],
    }
}

// Anthropic image 块 -> OpenAI image_url 的 url（data URI 或直链）
fn claude_image_to_openai_url(block: &Value) -> Option<String> {
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
            Some(format!("data:{};base64,{}", media, data))
        }
        Some("url") => {
            let url = source.get("url").and_then(|v| v.as_str())?;
            if url.is_empty() {
                return None;
            }
            Some(url.to_string())
        }
        _ => None,
    }
}

// Anthropic document 块 -> OpenAI file 的 file_data（仅支持 base64）
fn claude_document_to_openai_filedata(block: &Value) -> Option<String> {
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
            Some(format!("data:{};base64,{}", media, data))
        }
        _ => None,
    }
}

// 把 source 上的 cache_control 对象复制到 dest（不存在或非对象则原样返回）。
// 用于把 Anthropic 的缓存控制透传到 OpenAI 结构。
fn attach_cache_control(mut dest: Value, src: &Value) -> Value {
    if let Some(cc) = src.get("cache_control") {
        if cc.is_object() {
            if let Some(obj) = dest.as_object_mut() {
                obj.insert("cache_control".to_string(), cc.clone());
            }
        }
    }
    dest
}

// 合并 OpenAI content 段：全为 text 则拼成字符串，否则保留数组（含图片/文件）。
fn finalize_content(parts: &[Value]) -> Value {
    if parts.is_empty() {
        return json!("");
    }
    let all_text = parts
        .iter()
        .all(|p| p.get("type").and_then(|v| v.as_str()) == Some("text"));
    if all_text {
        let joined: String = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("");
        json!(joined)
    } else {
        json!(parts)
    }
}

// Anthropic tool_result.content（字符串/数组）-> OpenAI tool 消息的文本 content。
// OpenAI/NVIDIA 的 tool 消息不接受 image_url/file 内容段；媒体必须降级为简短说明，
// 否则 NVIDIA 会以 ChatCompletionRequestToolMessageContent 反序列化失败返回 400。
fn convert_tool_result_content(content: &Value) -> Value {
    match content {
        Value::String(s) => json!(s.clone()),
        Value::Array(arr) => {
            let mut parts: Vec<String> = Vec::new();
            for item in arr {
                if let Some(s) = item.as_str() {
                    parts.push(s.to_string());
                } else if let Some(t) = item.get("type").and_then(|v| v.as_str()) {
                    match t {
                        "text" => {
                            if let Some(s) = item.get("text").and_then(|v| v.as_str()) {
                                parts.push(s.to_string());
                            }
                        }
                        "image" => {
                            let media_type = item
                                .pointer("/source/media_type")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            parts.push(format!("[image omitted: {media_type}]"));
                        }
                        "document" => {
                            let media_type = item
                                .pointer("/source/media_type")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            parts.push(format!("[document omitted: {media_type}]"));
                        }
                        _ => {}
                    }
                }
            }
            let joined = parts.join("\n\n");
            if joined.trim().is_empty() {
                json!("")
            } else {
                json!(joined)
            }
        }
        other => json!(other.to_string()),
    }
}

// Anthropic tools -> OpenAI tools 格式
// Anthropic: {name, description, input_schema}
// OpenAI:    {type:"function", function:{name, description, parameters}}
fn anthropic_tools_to_openai(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name").and_then(|v| v.as_str())?;
            let description = t.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let input_schema = t
                .get("input_schema")
                .cloned()
                .unwrap_or(json!({ "type": "object", "properties": {} }));
            let mut openai_tool = json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": input_schema
                }
            });
            // 透传 cache_control（若工具定义上携带）
            openai_tool = attach_cache_control(openai_tool, t);
            Some(openai_tool)
        })
        .collect()
}

// Anthropic tool_choice -> OpenAI tool_choice
// {type:"auto"}  -> "auto"
// {type:"any"}   -> "required"
// {type:"tool", name:"x"} -> {"type":"function","function":{"name":"x"}}
fn anthropic_tool_choice_to_openai(tc: &Value) -> Value {
    let ttype = tc.get("type").and_then(|v| v.as_str()).unwrap_or("auto");
    match ttype {
        "any" => json!("required"),
        "tool" => {
            let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("");
            json!({ "type": "function", "function": { "name": name } })
        }
        _ => json!("auto"),
    }
}

// Anthropic thinking 配置 -> OpenAI reasoning_effort
// 仅当显式开启（enabled/adaptive/auto）时返回 Some；disabled 或无配置返回 None，
// 避免给普通模型发送不支持的参数。
fn map_thinking_to_reasoning_effort(thinking: &Value) -> Option<String> {
    let ttype = thinking.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match ttype {
        "disabled" => None,
        "enabled" => {
            let budget = thinking
                .get("budget_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if budget == 0 {
                None
            } else if budget < 2048 {
                Some("low".to_string())
            } else if budget < 8192 {
                Some("medium".to_string())
            } else {
                Some("high".to_string())
            }
        }
        "adaptive" | "auto" => Some("high".to_string()),
        _ => None,
    }
}

// 将一条 Anthropic 消息转换为一条或多条 OpenAI 消息。
// - 纯字符串 content -> 1 条 {role, content}
// - 数组 content 中的 text/image/document 块 -> 拼为 content（字符串或段数组）
// - 数组 content 中的 thinking 块 -> assistant 消息的 reasoning_content
// - 数组 content 中的 tool_use 块 -> assistant 消息的 tool_calls
// - 数组 content 中的 tool_result 块 -> 拆成独立的 role=tool 文本消息
fn convert_anthropic_message(msg: &Value) -> Vec<Value> {
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let content = msg.get("content").cloned().unwrap_or(Value::Null);
    let is_assistant = role == "assistant";

    match &content {
        Value::String(s) => vec![json!({ "role": role, "content": s })],
        Value::Array(arr) => {
            let mut messages: Vec<Value> = Vec::new();
            let mut content_parts: Vec<Value> = Vec::new();
            let mut reasoning_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();

            for block in arr {
                let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match btype {
                    "text" => {
                        if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                            if !t.trim().is_empty() {
                                let mut part = json!({ "type": "text", "text": t });
                                part = attach_cache_control(part, block);
                                content_parts.push(part);
                            }
                        }
                    }
                    "image" | "document" => {
                        for p in claude_block_to_openai_parts(block) {
                            content_parts.push(p);
                        }
                    }
                    "thinking" if is_assistant => {
                        // 仅 assistant 角色的思考块映射为 reasoning_content（防注入）
                        if let Some(t) = block.get("thinking").and_then(|v| v.as_str()) {
                            if !t.trim().is_empty() {
                                reasoning_parts.push(t.to_string());
                            }
                        }
                    }
                    "tool_use" if is_assistant => {
                        let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let input = block.get("input").cloned().unwrap_or(json!({}));
                        let args_str =
                            serde_json::to_string(&input).unwrap_or_else(|_| "{}".into());
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": args_str }
                        }));
                    }
                    "tool_result" => {
                        // 非 assistant 消息遇到 tool_result：先刷出已累积的 content，
                        // 再插入 role=tool 消息（保证它紧跟上一轮 assistant 的 tool_calls）。
                        if !is_assistant && !content_parts.is_empty() {
                            let m = json!({ "role": role, "content": finalize_content(&content_parts) });
                            messages.push(m);
                            content_parts.clear();
                        }
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let result_content = block
                            .get("content")
                            .map(convert_tool_result_content)
                            .unwrap_or(json!(""));
                        let mut tm = json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": result_content
                        });
                        tm = attach_cache_control(tm, block);
                        messages.push(tm);
                    }
                    _ => {}
                }
            }

            if is_assistant {
                // assistant：一条消息同时承载 content + reasoning_content + tool_calls
                if !content_parts.is_empty()
                    || !reasoning_parts.is_empty()
                    || !tool_calls.is_empty()
                {
                    let mut m = json!({ "role": "assistant" });
                    if !content_parts.is_empty() {
                        m["content"] = finalize_content(&content_parts);
                    } else {
                        m["content"] = json!("");
                    }
                    if !reasoning_parts.is_empty() {
                        m["reasoning_content"] = json!(reasoning_parts.join("\n\n"));
                    }
                    if !tool_calls.is_empty() {
                        m["tool_calls"] = json!(tool_calls);
                    }
                    messages.push(m);
                }
            } else if !content_parts.is_empty() {
                let m = json!({ "role": role, "content": finalize_content(&content_parts) });
                messages.push(m);
            }

            if messages.is_empty() {
                vec![json!({ "role": role, "content": "" })]
            } else {
                messages
            }
        }
        _ => vec![json!({ "role": role, "content": content_to_text(&content) })],
    }
}

// 构造 OpenAI Chat Completions 请求体。
// model / stream 由调用方（可能因 Fallback 而变化）传入，覆盖请求里的 model。
pub fn build_openai_request(req: &AnthropicRequest, model: &str, stream: bool) -> Value {
    let mut messages: Vec<Value> = Vec::new();

    // 1. system 放入 messages 首位（数组支持 cache_control，否则原样字符串）
    if let Some(sys) = &req.system {
        match sys {
            Value::String(s) => {
                if !s.is_empty() {
                    messages.push(json!({ "role": "system", "content": s }));
                }
            }
            Value::Array(arr) => {
                let mut parts: Vec<Value> = Vec::new();
                for blk in arr {
                    if let Some(t) = blk.get("text").and_then(|v| v.as_str()) {
                        if !t.trim().is_empty() {
                            let mut p = json!({ "type": "text", "text": t });
                            p = attach_cache_control(p, blk);
                            parts.push(p);
                        }
                    }
                }
                if !parts.is_empty() {
                    messages.push(json!({ "role": "system", "content": parts }));
                }
            }
            _ => {
                let t = content_to_text(sys);
                if !t.is_empty() {
                    messages.push(json!({ "role": "system", "content": t }));
                }
            }
        }
    }

    // 2. 逐条转换 messages（单条 Anthropic 消息可能拆成多条 OpenAI 消息）
    for m in &req.messages {
        messages.extend(convert_anthropic_message(m));
    }

    // 3. 组装请求体，透传采样参数
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });
    let obj = body.as_object_mut().unwrap();
    if stream {
        obj.insert("stream_options".into(), json!({ "include_usage": true }));
    }
    if let Some(mt) = req.max_tokens {
        obj.insert("max_tokens".into(), json!(mt));
    }
    if let Some(t) = req.temperature {
        obj.insert("temperature".into(), json!(t));
    }
    if let Some(p) = req.top_p {
        obj.insert("top_p".into(), json!(p));
    }
    if let Some(stop) = &req.stop_sequences {
        if !stop.is_empty() {
            obj.insert("stop".into(), json!(stop));
        }
    }
    // 4. 扩展思考配置 -> OpenAI reasoning_effort
    if let Some(thinking) = &req.thinking {
        if let Some(effort) = map_thinking_to_reasoning_effort(thinking) {
            obj.insert("reasoning_effort".into(), json!(effort));
        }
    }
    // 5. 工具定义
    if let Some(tools) = &req.tools {
        let openai_tools = anthropic_tools_to_openai(tools);
        if !openai_tools.is_empty() {
            obj.insert("tools".into(), json!(openai_tools));
        }
    }
    // 6. 工具选择策略
    if let Some(tc) = &req.tool_choice {
        obj.insert("tool_choice".into(), anthropic_tool_choice_to_openai(tc));
    }
    body
}

// ===== 响应转换 =====

// 从 OpenAI reasoning_content 节点收集文本（兼容 string / string[] / {text}[]）。
pub fn collect_reasoning_texts(node: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match node {
        Value::String(s) => {
            if !s.is_empty() {
                out.push(s.clone());
            }
        }
        Value::Array(arr) => {
            for v in arr {
                out.extend(collect_reasoning_texts(v));
            }
        }
        Value::Object(_) => {
            if let Some(t) = node.get("text").and_then(|v| v.as_str()) {
                if !t.is_empty() {
                    out.push(t.to_string());
                }
            }
        }
        _ => {}
    }
    out
}

// OpenAI image_url 段 -> Anthropic image 块
fn openai_image_url_to_anthropic(url: &str) -> Value {
    if let Some(stripped) = url.strip_prefix("data:") {
        // data:<media_type>;base64,<data>
        if let Some((media, data)) = stripped.split_once(";base64,") {
            return json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media, "data": data }
            });
        }
    }
    json!({ "type": "image", "source": { "type": "url", "url": url } })
}

// OpenAI file 段 -> Anthropic document 块
fn openai_filedata_to_anthropic(filedata: &str) -> Value {
    if let Some(stripped) = filedata.strip_prefix("data:") {
        if let Some((media, data)) = stripped.split_once(";base64,") {
            return json!({
                "type": "document",
                "source": { "type": "base64", "media_type": media, "data": data }
            });
        }
    }
    json!({
        "type": "document",
        "source": { "type": "base64", "media_type": "application/octet-stream", "data": filedata }
    })
}

// 从 OpenAI usage 提取 token 用量。
// 返回 (input_tokens 计费部分, output_tokens, cache_read_input_tokens)。
// OpenAI 的 prompt_tokens 含缓存命中部分，扣除 cached_tokens 作为计费 input。
pub fn extract_openai_usage(usage: &Value) -> (u64, u64, u64) {
    if !usage.is_object() {
        return (0, 0, 0);
    }
    let input = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let input_billable = if cached > 0 {
        input.saturating_sub(cached)
    } else {
        input
    };
    (input_billable, output, cached)
}

// OpenAI finish_reason -> Anthropic stop_reason
pub fn map_stop_reason(finish: Option<&str>) -> &'static str {
    match finish {
        Some("length") => "max_tokens",
        Some("tool_calls") | Some("function_call") => "tool_use",
        Some("content_filter") => "end_turn",
        _ => "end_turn",
    }
}

// 考虑是否出现过工具调用的 stop_reason 映射
pub fn map_stop_reason_with_tool(saw_tool: bool, finish: Option<&str>) -> &'static str {
    if saw_tool {
        return "tool_use";
    }
    map_stop_reason(finish)
}

// 清理 tool ID：只保留字母、数字、下划线、连字符
pub fn sanitize_tool_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("toolu_{nanos:x}")
    } else {
        cleaned
    }
}

// 生成一个 Anthropic 风格的消息 id（msg_ + 纳秒时间戳），无需引入 uuid 依赖
pub fn gen_message_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("msg_{nanos:x}")
}

// 规范化的工具名（小写、去首尾空白、去除前导下划线），用于大小写还原的 key。
fn canonical_tool_name(name: &str) -> String {
    let trimmed = name.trim().trim_start_matches('_');
    trimmed.to_lowercase()
}

// 从原始 Anthropic 请求的 tools 构建 canonical -> 原始大小写的映射。
pub fn build_tool_name_map(tools: &Option<Vec<Value>>) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    if let Some(tools) = tools {
        for t in tools {
            let name = match t.get("name").and_then(|v| v.as_str()) {
                Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                _ => continue,
            };
            let key = canonical_tool_name(&name);
            if key.is_empty() {
                continue;
            }
            map.entry(key).or_insert(name);
        }
    }
    map
}

// 按 ToolNameMap 还原工具名大小写（找不到映射时原样返回）。
pub fn map_tool_name(tool_map: &HashMap<String, String>, name: &str) -> String {
    if name.is_empty() || tool_map.is_empty() {
        return name.to_string();
    }
    let key = canonical_tool_name(name);
    match tool_map.get(&key) {
        Some(mapped) if !mapped.is_empty() => mapped.clone(),
        _ => name.to_string(),
    }
}

// 非流式：OpenAI 响应 JSON -> Anthropic messages 响应 JSON
// 处理 text / thinking / tool_calls 块与 token 用量（含 cache_read）。
pub fn openai_response_to_anthropic(
    openai: &Value,
    model: &str,
    tool_map: &HashMap<String, String>,
) -> Value {
    let choice0 = openai
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let message = choice0.and_then(|c| c.get("message"));

    let mut content: Vec<Value> = Vec::new();

    // 1) thinking 块（必须排在 text/tool_use 之前，符合 Anthropic 顺序约定）
    if let Some(m) = message {
        if let Some(rc) = m.get("reasoning_content") {
            for t in collect_reasoning_texts(rc) {
                if !t.trim().is_empty() {
                    content.push(json!({ "type": "thinking", "thinking": t }));
                }
            }
        }
    }

    // 2) text 或 content 数组（含图片/文档）
    if let Some(m) = message {
        if let Some(s) = m.get("content").and_then(|v| v.as_str()) {
            if !s.is_empty() {
                content.push(json!({ "type": "text", "text": s }));
            }
        } else if let Some(arr) = m.get("content").and_then(|v| v.as_array()) {
            for part in arr {
                let ptype = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ptype {
                    "text" => {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            if !t.is_empty() {
                                content.push(json!({ "type": "text", "text": t }));
                            }
                        }
                    }
                    "image_url" => {
                        if let Some(u) = part
                            .get("image_url")
                            .and_then(|v| v.get("url"))
                            .and_then(|v| v.as_str())
                        {
                            content.push(openai_image_url_to_anthropic(u));
                        }
                    }
                    "file" => {
                        if let Some(fd) = part
                            .get("file")
                            .and_then(|v| v.get("file_data"))
                            .and_then(|v| v.as_str())
                        {
                            content.push(openai_filedata_to_anthropic(fd));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // 3) tool_calls -> tool_use 块（还原大小写）
    let tool_calls: Vec<Value> = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .map(|tc| {
                    let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let raw_name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let name = map_tool_name(tool_map, raw_name);
                    let args_str = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");
                    let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                    json!({
                        "type": "tool_use",
                        "id": sanitize_tool_id(id),
                        "name": name,
                        "input": input
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let has_tool_calls = !tool_calls.is_empty();
    content.extend(tool_calls);

    let finish = choice0
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());
    let stop_reason = if has_tool_calls {
        "tool_use"
    } else {
        map_stop_reason(finish)
    };

    // 4) token 用量
    let usage = openai.get("usage");
    let (input_tokens, output_tokens, cache_read) = match usage {
        Some(u) => extract_openai_usage(u),
        None => (0, 0, 0),
    };
    let mut usage_obj = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });
    if cache_read > 0 {
        usage_obj["cache_read_input_tokens"] = json!(cache_read);
    }

    let id = openai
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(gen_message_id);

    if content.is_empty() {
        content.push(json!({ "type": "text", "text": "" }));
    }

    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage_obj
    })
}

// ===== 流式：Anthropic SSE 事件构造 =====

// 拼装一条 SSE：event: <type>\n data: <json>\n\n
pub fn sse_event(event: &str, data: &Value) -> String {
    format!("event: {}\ndata: {}\n\n", event, data)
}

// message_start 事件的 data
pub fn ev_message_start(id: &str, model: &str) -> Value {
    json!({
        "type": "message_start",
        "message": {
            "id": id,
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [],
            "stop_reason": null,
            "stop_sequence": null,
            "usage": { "input_tokens": 0, "output_tokens": 0 }
        }
    })
}

// content_block_start: 文本块（带动态 index）
pub fn ev_content_block_start_text(index: i32) -> Value {
    json!({
        "type": "content_block_start",
        "index": index,
        "content_block": { "type": "text", "text": "" }
    })
}

// content_block_start: 思考块（带动态 index）
pub fn ev_content_block_start_thinking(index: i32) -> Value {
    json!({
        "type": "content_block_start",
        "index": index,
        "content_block": { "type": "thinking", "thinking": "" }
    })
}

// content_block_start: 工具使用块
pub fn ev_content_block_start_tool_use(index: i32, id: &str, name: &str) -> Value {
    json!({
        "type": "content_block_start",
        "index": index,
        "content_block": {
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": {}
        }
    })
}

// content_block_delta: 文本增量
pub fn ev_content_block_delta_text(index: i32, text: &str) -> Value {
    json!({
        "type": "content_block_delta",
        "index": index,
        "delta": { "type": "text_delta", "text": text }
    })
}

// content_block_delta: 思考增量
pub fn ev_content_block_delta_thinking(index: i32, text: &str) -> Value {
    json!({
        "type": "content_block_delta",
        "index": index,
        "delta": { "type": "thinking_delta", "thinking": text }
    })
}

// content_block_delta: 工具参数 JSON 增量
pub fn ev_content_block_delta_input_json(index: i32, partial_json: &str) -> Value {
    json!({
        "type": "content_block_delta",
        "index": index,
        "delta": { "type": "input_json_delta", "partial_json": partial_json }
    })
}

// content_block_stop（带动态 index）
pub fn ev_content_block_stop(index: i32) -> Value {
    json!({ "type": "content_block_stop", "index": index })
}

// message_delta 事件的 data（携带 stop_reason 与累计 token，含 cache_read）
pub fn ev_message_delta(
    stop_reason: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
) -> Value {
    let mut usage = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });
    if cache_read > 0 {
        usage["cache_read_input_tokens"] = json!(cache_read);
    }
    json!({
        "type": "message_delta",
        "delta": { "stop_reason": stop_reason, "stop_sequence": null },
        "usage": usage
    })
}

// message_stop 事件的 data
pub fn ev_message_stop() -> Value {
    json!({ "type": "message_stop" })
}

// ===== 流式状态管理 =====

// 工具调用累积器：按 OpenAI 的 tool_calls[].index 键控
#[derive(Default)]
struct ToolAccumulator {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

// 流式转换状态：管理动态 content block 索引、文本/思考/工具块的生命周期
pub struct StreamState {
    text_started: bool,
    text_index: i32,
    thinking_started: bool,
    thinking_index: i32,
    next_index: i32,
    tools: HashMap<i32, ToolAccumulator>,
    tool_block_idx: HashMap<i32, i32>,
    tool_name_map: HashMap<String, String>,
    saw_tool: bool,
    blocks_stopped: bool,
}

impl StreamState {
    pub fn new(tool_name_map: HashMap<String, String>) -> Self {
        Self {
            text_started: false,
            text_index: -1,
            thinking_started: false,
            thinking_index: -1,
            next_index: 0,
            tools: HashMap::new(),
            tool_block_idx: HashMap::new(),
            tool_name_map,
            saw_tool: false,
            blocks_stopped: false,
        }
    }

    // 确保文本块已 start，返回需要发出的 SSE 事件
    fn ensure_text_started(&mut self) -> Option<String> {
        if self.text_started || self.blocks_stopped {
            return None;
        }
        self.text_index = self.next_index;
        self.next_index += 1;
        self.text_started = true;
        Some(sse_event(
            "content_block_start",
            &ev_content_block_start_text(self.text_index),
        ))
    }

    // 确保思考块已 start
    fn ensure_thinking_started(&mut self) -> Option<String> {
        if self.thinking_started || self.blocks_stopped {
            return None;
        }
        self.thinking_index = self.next_index;
        self.next_index += 1;
        self.thinking_started = true;
        Some(sse_event(
            "content_block_start",
            &ev_content_block_start_thinking(self.thinking_index),
        ))
    }

    // 关闭文本块（如果已开启）
    fn stop_text(&mut self) -> Option<String> {
        if !self.text_started {
            return None;
        }
        self.text_started = false;
        let idx = self.text_index;
        self.text_index = -1;
        Some(sse_event("content_block_stop", &ev_content_block_stop(idx)))
    }

    // 关闭思考块（如果已开启）
    fn stop_thinking(&mut self) -> Option<String> {
        if !self.thinking_started {
            return None;
        }
        self.thinking_started = false;
        let idx = self.thinking_index;
        self.thinking_index = -1;
        Some(sse_event("content_block_stop", &ev_content_block_stop(idx)))
    }

    // 处理文本增量：确保文本块已 start（切换时先关思考块），返回 delta 事件
    pub fn handle_text(&mut self, text: &str) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(s) = self.stop_thinking() {
            events.push(s);
        }
        if let Some(s) = self.ensure_text_started() {
            events.push(s);
        }
        events.push(sse_event(
            "content_block_delta",
            &ev_content_block_delta_text(self.text_index, text),
        ));
        events
    }

    // 处理思考增量：确保思考块已 start（切换时先关文本块），返回 delta 事件
    pub fn handle_thinking(&mut self, text: &str) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(s) = self.stop_text() {
            events.push(s);
        }
        if let Some(s) = self.ensure_thinking_started() {
            events.push(s);
        }
        events.push(sse_event(
            "content_block_delta",
            &ev_content_block_delta_thinking(self.thinking_index, text),
        ));
        events
    }

    // 处理 tool_calls delta 中的一个条目
    pub fn handle_tool_call(&mut self, tc: &Value) -> Vec<String> {
        let mut events = Vec::new();
        let index = tc.get("index").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

        // 切换时先关文本/思考块
        if let Some(s) = self.stop_text() {
            events.push(s);
        }
        if let Some(s) = self.stop_thinking() {
            events.push(s);
        }

        // 从 delta 中提取字段（不持有 self 的借用）
        let id_val = tc
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let name_val = tc
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let args_fragment = tc
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(|a| a.as_str())
            .unwrap_or("");

        // Phase 1: 更新累积器并判断是否需要 start（借用范围限于此块）
        let need_start = {
            let acc = self.tools.entry(index).or_default();
            if let Some(id) = id_val {
                acc.id = id.to_string();
            }
            if let Some(name) = name_val {
                if !acc.started {
                    acc.name = map_tool_name(&self.tool_name_map, name);
                }
            }
            !acc.started && !acc.name.is_empty() && !acc.id.is_empty() && !self.blocks_stopped
        };
        // tools 的借用在此结束

        // Phase 2: 如果需要 start，先开工具块
        if need_start {
            let block_idx = self.next_index;
            self.next_index += 1;
            self.tool_block_idx.insert(index, block_idx);

            let acc = self.tools.get_mut(&index).unwrap();
            let tool_id = sanitize_tool_id(&acc.id);
            let tool_name = acc.name.clone();
            events.push(sse_event(
                "content_block_start",
                &ev_content_block_start_tool_use(block_idx, &tool_id, &tool_name),
            ));
            acc.started = true;
            self.saw_tool = true;
        }

        // Phase 3: 处理参数片段——累积并流式发出
        if !args_fragment.is_empty() {
            let (started, block_idx) = {
                let acc = self.tools.get_mut(&index).unwrap();
                acc.arguments.push_str(args_fragment);
                (
                    acc.started,
                    self.tool_block_idx.get(&index).copied().unwrap_or(0),
                )
            };
            if started {
                events.push(sse_event(
                    "content_block_delta",
                    &ev_content_block_delta_input_json(block_idx, args_fragment),
                ));
            }
        }

        events
    }

    // 关闭所有内容块（文本 + 思考 + 工具），在 finish_reason / [DONE] 时调用
    pub fn stop_all(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        if self.blocks_stopped {
            return events;
        }

        // 依次关闭思考块、文本块、工具块
        if let Some(s) = self.stop_thinking() {
            events.push(s);
        }
        if let Some(s) = self.stop_text() {
            events.push(s);
        }

        // 按 index 排序，依次关闭工具块
        let mut indexes: Vec<i32> = self.tools.keys().copied().collect();
        indexes.sort();

        for idx in indexes {
            // 提取累积器状态（借用范围限于此块）
            let (need_belated, tool_id, tool_name, accumulated_args) = {
                let acc = self.tools.get(&idx).unwrap();
                if acc.started {
                    (false, String::new(), String::new(), String::new())
                } else if acc.name.is_empty() {
                    continue;
                } else {
                    (
                        true,
                        sanitize_tool_id(&acc.id),
                        acc.name.clone(),
                        acc.arguments.clone(),
                    )
                }
            };
            // tools 的借用在此结束

            // 延迟 start：有 name 但从未发出过 start
            if need_belated {
                let block_idx = self.next_index;
                self.next_index += 1;
                self.tool_block_idx.insert(idx, block_idx);
                events.push(sse_event(
                    "content_block_start",
                    &ev_content_block_start_tool_use(block_idx, &tool_id, &tool_name),
                ));
                self.tools.get_mut(&idx).unwrap().started = true;
                self.saw_tool = true;

                // 发出累积的参数
                if !accumulated_args.is_empty() {
                    events.push(sse_event(
                        "content_block_delta",
                        &ev_content_block_delta_input_json(block_idx, &accumulated_args),
                    ));
                }
            }

            let block_idx = self.tool_block_idx.get(&idx).copied().unwrap_or(0);
            events.push(sse_event(
                "content_block_stop",
                &ev_content_block_stop(block_idx),
            ));
        }

        self.blocks_stopped = true;
        events
    }

    pub fn saw_tool(&self) -> bool {
        self.saw_tool
    }
}

#[cfg(test)]
mod stream_usage_request_tests {
    use super::build_openai_request;
    use crate::nvidia::models::AnthropicRequest;
    use serde_json::json;

    #[test]
    fn streaming_request_asks_nvidia_for_terminal_usage_chunk() {
        let request: AnthropicRequest = serde_json::from_value(json!({
            "model": "z-ai/glm-5.2",
            "messages": [{ "role": "user", "content": "hello" }],
            "stream": true
        }))
        .unwrap();

        let openai = build_openai_request(&request, "z-ai/glm-5.2", true);

        assert_eq!(
            openai.pointer("/stream_options/include_usage"),
            Some(&json!(true)),
            "NVIDIA 流式响应必须返回 prompt_tokens，Claude Code 才能判断 auto-compact 阈值"
        );
    }

    #[test]
    fn non_streaming_request_does_not_send_stream_options() {
        let request: AnthropicRequest = serde_json::from_value(json!({
            "model": "z-ai/glm-5.2",
            "messages": [{ "role": "user", "content": "hello" }]
        }))
        .unwrap();

        let openai = build_openai_request(&request, "z-ai/glm-5.2", false);

        assert!(
            openai.get("stream_options").is_none(),
            "非流式请求不应携带仅适用于 SSE 的 stream_options"
        );
    }

    #[test]
    fn tool_result_with_image_is_downgraded_to_text_for_nvidia() {
        let request: AnthropicRequest = serde_json::from_value(json!({
            "model": "z-ai/glm-5.2",
            "messages": [
                {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "tool-1",
                        "name": "capture",
                        "input": {}
                    }]
                },
                {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "tool-1",
                        "content": [
                            { "type": "text", "text": "capture completed" },
                            {
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": "image/png",
                                    "data": "sensitive-base64-payload"
                                }
                            }
                        ]
                    }]
                }
            ]
        }))
        .unwrap();

        let openai = build_openai_request(&request, "z-ai/glm-5.2", true);
        let tool_content = openai.pointer("/messages/1/content").unwrap();

        assert!(
            tool_content.is_string(),
            "NVIDIA tool 消息只接受文本 content，不能发送 image_url/file 数组"
        );
        assert_eq!(
            tool_content,
            &json!("capture completed\n\n[image omitted: image/png]")
        );
        assert!(
            !openai.to_string().contains("sensitive-base64-payload"),
            "不兼容的媒体数据不应继续进入 NVIDIA 请求"
        );
    }
}
