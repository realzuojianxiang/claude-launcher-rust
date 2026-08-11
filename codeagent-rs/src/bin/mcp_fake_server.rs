//! `codeagent-mcp-fake-server` —— (a) trait async 升级的验证 keystone 用极窄 stdio MCP server。
//!
//! Phase A harness（见 plans/replicated-meandering-dawn.md §2.2）：一个无真 key、无 npx、无网络的
//! JSON-RPC 2.0 stdio server，跑真 `McpClient::spawn → handshake → list_tools → call_tool
//! → McpTool::execute → 后台读 task 扇回` 全链。它在当前桥形代码上绿（refactor 前基线）、
//! refactor 无桥后再绿——两边都绿才证 McpTool async 路径运行期等价。
//!
//! 极窄实现：逐行读 stdin，按 method 分派回一帧 response（`\n` 终）；EOF/坏行退。
//! 故意走纯标准库（无 tokio），让 `[[bin]]` 不拖 async 依赖、不复刻任何 runtime 形态——
//! 它只是对端协议回声，被测对象是 McpClient（在主进程的 tokio runtime 里）。

use std::io::{self, BufRead, Write};

fn main() {
    // P12-4(C1)候选析因闸用:env `MCP_FAKE_NO_RESPOND` 非空时,本进程对所有请求**静默不响应**
    // (stdin 仍逐行消费避免管道堵塞,但不写回响应帧)——对端 `McpClient::request` 必撞 timeout。
    // 仅供 timeout 路径 pending 泄漏析因测用,不应影响其他测试(默认不设置此 env)。
    let no_respond = std::env::var_os("MCP_FAKE_NO_RESPOND").is_some();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // stdin EOF/坏:退,让对端读 task 收到 EOF 自然收
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let val: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue, // 坏 JSON:丢,不给回(对端按 id 等会走 timeout,本 server 不诈)
        };
        if no_respond {
            // 仍逐行消费 stdin,但**不写回任何响应帧** —— 对端 `request` 撞 timeout。
            // 不 break:继续读下一行,让对端能继续往 stdin 写(若它退也通过其 stdin drop 让本 loop 撞 EOF 自然退)。
            continue;
        }
        let id = val.get("id").cloned();
        let method = val.get("method").and_then(|v| v.as_str()).unwrap_or("");
        // 通知（无 id）不回。固定忽略 notifications/initialized。
        if id.is_none() {
            continue;
        }
        let id = id.unwrap();
        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": { "name": "codeagent-mcp-fake-server", "version": "0.0.0" },
            }),
            "tools/list" => serde_json::json!({
                "tools": [{
                    "name": "echo",
                    "description": "Echo back the given text.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "text": { "type": "string" } },
                        "required": ["text"],
                    },
                }],
            }),
            "tools/call" => {
                let name = val
                    .pointer("/params/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let text = val
                    .pointer("/params/arguments/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let body = if name == "echo" {
                    format!("echo: {text}")
                } else {
                    format!("unknown tool: {name}")
                };
                serde_json::json!({
                    "content": [{ "type": "text", "text": body }],
                })
            }
            _ => serde_json::json!({}),
        };
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        // 每帧一行 JSON + `\n` + flush —— 对端 read task 逐行解。
        let mut s = serde_json::to_string(&resp).expect("回声帧序列化");
        s.push('\n');
        if stdout.write_all(s.as_bytes()).is_err() || stdout.flush().is_err() {
            break;
        }
    }
}
