// P10 撞坑 #1 实证:MCP request TOCTOU race —— 审计读 mcp.rs:326/330 flush 先于 :336 pending.insert,
// read task 在 flush 后 insert 前若读到 server 响应,map.remove(&id)=None 丢响应 -> rx 永不收 -> 30-60s hang。
// fake-server 是纯 stdlib 同步零延迟响应(loop: 读一行立刻 write_all+flush 回一行),正是撞 race 的理想对端。
// 连跑 N 次握手:list_tools 在 FLUSH 与 INSERT race window 之间能被 read task 抢先读到响应的话,
// 那一次握手会超时挂住(默认 60s 太慢,本测把 handshake_timeout_secs 压到 3s 让 race 失败快速暴露)。
// 若 N 次全绿 = race 在实测中没稳定撞上(可能是 tokio 调度让 read task 在 read_line await yield 不会抢先);
// 若任一次超时 = 实证撞到 race,P10 成立。

// gate: 无 key 需求(纯 IPC),但守 CODEAGENT_E2E 风格,本测默认跑(CI-runnable)。
// flavor=multi_thread 匹配生产,与 keystone 一致。

use codeagent::config::McpServerConfig;
use codeagent::mcp::McpClient;
use std::time::Duration;

const N: usize = 200;
const TIGHT_TIMEOUT: u64 = 3; // 紧超时:race 命中即 ~3s 暴露,而非默认 60s。
                              // 200 次:修前实测 ~2% race(100 次丢 2 次),200 次期望 ~4 次失败 —— 若 race 回归必撞到。
                              // 单跑 ~3s 整,跑 200 次 200x spawn,~2-4s。CI 闸够敏感又不慢。

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_request_toctou_race_repeated_handshake_never_loses_response() {
    // fake-server 是 stdlib 同步、读到就立刻 flush 回,响应零延迟 = 最易撞 race window 的对端。
    // 我们连做 N 次完整 spawn+handshake+list_tools,任一次若 race 命中 -> read task 抢先 ->
    // pending.insert 赶不上 -> rx 3s 超时 -> Err -> 测 FAIL(暴露 race)。
    let mut failures = Vec::new();
    for i in 0..N {
        let cfg = McpServerConfig {
            command: env!("CARGO_BIN_EXE_codeagent-mcp-fake-server").into(),
            args: vec![],
            env: None,
            prefix: None,
            // 紧超时让 race 暴露快(默认 60s 慢,但 race 命中本就≈timeout 值,3s 足够判)
            handshake_timeout_secs: Some(TIGHT_TIMEOUT),
        };
        let client = match McpClient::spawn(&cfg).await {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("#{} spawn 失败: {:#}", i, e));
                continue;
            }
        };
        // handshake + list_tools 全在一个 lock 段,期间 FLUSH->INSERT race 在每帧都有窗口
        let res = async {
            let mut c = client.lock().await;
            c.handshake().await?;
            c.list_tools().await
        };
        match tokio::time::timeout(Duration::from_secs(TIGHT_TIMEOUT * 2 + 1), res).await {
            Ok(Ok(_tools)) => {}
            Ok(Err(e)) => failures.push(format!("#{} handshake/list 失败(race 嫌疑): {:#}", i, e)),
            Err(_) => failures.push(format!("#{} 2x timeout 仍未回(强 race 嫌疑)", i)),
        }
        // 收尾:关 fake server(EOF 让它退)+ 等收尸,避免子进程泄漏漂移下一轮
        drop(client);
        tokio::task::yield_now().await;
    }
    assert!(
        failures.is_empty(),
        "P10 #1 TOCTOU race 实证命中: {} 次中 {} 次响应丢失/hang:\n{}",
        N,
        failures.len(),
        failures.join("\n")
    );
}
