//! (a) trait async 升级 Phase A keystone —— 无真 key、无 npx、无网络的 MCP 握手往返测试。
//!
//! 起一个真的 `codeagent-mcp-fake-server` 子进程(`[[bin]]` in `Cargo.toml`),对它跑真
//! `McpClient::spawn → handshake → list_tools → McpTool::new → execute → 后台读 task 扇回` 全链。
//! 在**当前桥形代码**上绿(经 `block_on_current` 桥跑 async call_tool)是 refactor 前基线;Phase B
//! 删桥后 `Tool::execute` 升 async,本测试的 `tool.execute(...)` 行加 `.await` 再绿 —— 两边都绿
//! 才证 McpTool async 路径**运行期**等价,不止编译等价。这是 §14.4 stall 现场(读 task
//! `tokio::spawn` pending 在子进程管道 IO)的 CI 可跑版,套住 §14.3「refactor 桥不该在静默坏
//! 运行期路径下过 cargo 闸」。
//!
//! 为何走 `tests/` 集成测试而非 `src/mcp.rs mod tests`:crate root 已搬到 lib(`src/lib.rs`
//! `pub mod mcp`),集成测试可达 `codeagent::mcp::McpClient`(只走 pub 的 spawn/handshake/
//! list_tools + McpTool::new/execute;私有的 call_tool 仍私有,不碰);且 cargo 只给 `tests/`
//! 集成测试设 `CARGO_BIN_EXE_<name>`(src 内 `#[test]` 不设),fake-server 必须经 `tests/` 才能用
//! `env!("CARGO_BIN_EXE_codeagent_mcp_fake_server")` 拿到 exe 绝对路径。
//!
//! `flavor = "multi_thread"` **load-bearing**:生产 `#[tokio::main]` 多线程,§14.4 stall 特指
//! 多线程 runtime 上「≥2 个 spawn task pending 子进程管道 IO」。裸 `#[tokio::test]` = current_thread,
//! 会掩盖此 stall 回归 —— 正复刻 §14.3「门禁绿=伪绿」坑。故强配 `multi_thread` 真行使。

use std::sync::Arc;

use codeagent::config::McpServerConfig;
use codeagent::mcp::{McpClient, McpTool};
use codeagent::tools::Tool; // trait 在作用域才能调 McpTool::execute(impl Tool 方法)

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fake_mcp_server_handshake_list_call_roundtrip() {
    // fake-server exe:cargo 给集成测试设的绝对路径(`[[bin]] codeagent-mcp-fake-server`)。
    // 注意 env 名按二进制原名保留连字符(CARGO_BIN_EXE_<name> 不把连字符转下划线,见 cargo 文档)。
    let exe = env!("CARGO_BIN_EXE_codeagent-mcp-fake-server");
    // 握手超时给 15s。fake-server 是纯 stdio 回声无 npx 冷拉,实测毫秒级;但 `cargo test` 在
    // 首拉编译/缓存写入抖动下偶现抖动,15s 给数 σ 余量防 flake —— 真卡死会超期 FAIL 不静默挂。
    let cfg = McpServerConfig {
        command: exe.to_string(),
        args: vec![],
        env: None,
        prefix: None,
        handshake_timeout_secs: Some(15),
    };

    // 起 server 子进程 + 后台读 task。kill_on_drop=true:测试退出自动收子进程,无孤儿。
    let client = McpClient::spawn(&cfg)
        .await
        .expect("spawn fake-mcp-server 子进程应成功");

    // 握手:initialize(等回)+ notifications/initialized(通知)。
    client
        .lock()
        .await
        .handshake()
        .await
        .expect("fake-mcp-server 握手应成功");

    // tools/list:回一个 echo 工具。断言表含 echo。
    let descs = {
        let mut g = client.lock().await;
        g.list_tools().await.expect("tools/list 应成功")
    };
    let echo_desc = descs
        .iter()
        .find(|d| d.name == "echo")
        .expect("fake-mcp-server 应暴露一个名为 echo 的工具");
    assert!(
        echo_desc.schema.get("properties").is_some(),
        "echo 工具应有 inputSchema.properties"
    );

    // McpTool 把远端 echo 包成 Tool,execute 发 tools/call(用 server 原名 echo)并把
    // result.content[].text 拼回。Phase B:execute 升 async、不经 (c′) 桥,直接 `.await`
    // call_tool —— Phase A 基线经桥绿 + Phase B 无桥再绿 才证 McpTool async 路径运行期等价。
    let tool = McpTool::new(Arc::clone(&client), None, echo_desc);
    let out = tool
        .execute(r#"{"text":"hi"}"#)
        .await
        .expect("McpTool::execute(echo) 应 Ok");
    assert!(
        out.contains("echo: hi"),
        "echo 往返应回灌「echo: hi」;实得: {out}"
    );
}
