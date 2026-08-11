// P12-4(C1)候选 #5 析因闸:MCP `request()` timeout err 路径漏 `pending.remove(&id)` ——
// P10-1 修 TOCTOU race 时注释 line 331 自述「删请求(超时取消)走 timeout 后的 map.remove 收尾」
// 但代码 timeout Elapsed 路径只构造 err 串 `?` propagate 未 remove -> oneshot `tx` 孤儿留
// `pending` HashMap(若 read task 永不再回该 id 则永留到 McpClient drop = 几 KB 不可见内存增长)。
//
// 本测对生产 `McpClient::request` 走真 spawn 析因:连 fake-server 的「不响应」模式
// (env `MCP_FAKE_NO_RESPOND` 非空时对端静默不回),紧 timeout 50ms 让 `handshake()` 内
// `request("initialize", ...)` 撞钟 Err。修前(漏 remove):撞钟后 `pending` 仍留该 id;
// 修后(对称补充):任 Err 出口都 `pending.remove(&id)` 收尾,撞钟后 `pending` 空。
//
// gate:无 key 需求(纯 stdio IPC + tokio::time),CI-runnable、跨 OS、不依赖 npx。
// flavor=multi_thread 匹配生产。

use codeagent::config::McpServerConfig;
use codeagent::mcp::McpClient;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_request_timeout_err_path_clears_pending_no_orphan_sender() {
    // fake server「不响应」模式:env `MCP_FAKE_NO_RESPOND` 非空让 server 静默消费 stdin 但不写回响应。
    // 紧 `handshake_timeout_secs=1` 让 `initialize` request 1s 撞钟(可能 N=1 单跑 < spawn 启动几秒遇撞钟快不了);
    // 实际 spawn 一进程在 Windows ~10-50ms,1s 后撞钟留余地(防 spawn 慢 OS 把 baseline 拉慢)。
    let mut cmd_env = std::collections::HashMap::new();
    cmd_env.insert("MCP_FAKE_NO_RESPOND".to_string(), "1".to_string());
    let cfg = McpServerConfig {
        command: env!("CARGO_BIN_EXE_codeagent-mcp-fake-server").into(),
        args: vec![],
        env: Some(cmd_env),
        prefix: None,
        handshake_timeout_secs: Some(1), // 1s 紧超时让 timeout 路径快速撞钟。
    };

    let client = McpClient::spawn(&cfg)
        .await
        .expect("spawn fake-server 不响应模式应成立(只是握手会撞钟,不是 spawn 失败)");

    // handshake() 内调 request("initialize", ..., handshake_timeout) + request("tools/list", ..., handshake_timeout)。
    // fake server 不响应任何 method -> initialize 即 1s 撞钟 Err。
    let handshake_res = {
        let mut c = client.lock().await;
        c.handshake().await
    };
    assert!(
        handshake_res.is_err(),
        "fake server 不响应模式下 handshake() 必撞 timeout Err —— \
         拿到 Ok 说明 no_respond 模式未真生效(查 MCP_FAKE_NO_RESPOND env 传递), \
         或 timeout 设得过宽;握手结果: {:#?}",
        handshake_res.ok()
    );

    // 真坑实证(修前谓):撞钟后读 `pending` HashMap 仍留撞钟那帧的 id -> 待会慢慢累积漏。
    // 修后(对称 remove):Err 出口清自己登记的 entry -> `pending_len_for_test` == 0。
    let pending_after_timeout = {
        let c = client.lock().await;
        c.pending_len_for_test()
    };
    assert_eq!(
        pending_after_timeout, 0,
        "P12-4(C1) 真修:timeout Err 出口 `pending.remove(&id)` 收尾后 `pending` 应空; \
         拿到 {} 说明 timeout 路径漏 remove -> oneshot `tx` 孤儿留 HashMap(注释 line 331 自述 \
         该收尾但代码漏落),修复应在该 err 出口对称补 `pending.remove(&id)`。",
        pending_after_timeout
    );

    // 收尾:drop 客户端让 stdin 关 -> fake server 读 EOF 退 -> child drop 收尸 kill_on_drop。
    drop(client);
    tokio::task::yield_now().await;
}
