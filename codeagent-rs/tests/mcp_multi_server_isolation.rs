// P11-2 真裁:MCP 多 server 并发隔离 —— 审计候选「裁多 MCP server 同时 spawn、各自 handshake、
// 各自超时窗口、共用 client stdio 通道时会不会相互污染 read task / 共享 pending map」。
//
// 真裁前先读码(诚实「读码出的嫌疑」非想当然):
//   · 每个 `McpClient::spawn` 起一独立子进程 + 独立 stdout 管道 + 独立 `Arc<Mutex<HashMap>>`
//     pending map + 独立 read task(只读自己的 stdout、只扇回自己的 pending)—— **无共享通道、
//     无共享表**。命题「共用 client stdio 通道 / 共享 pending map」与实结构不符。
//   · `main.rs::run`(行 928)多 server spawn+handshake 走 `for ... .await`(串行,非 join_all)。
//   · 运行期 `tools/call`(行 739)`for call in &calls { dispatch_tool(...).await }` 同回合同样串行。
//   · per-server `Arc<Mutex<McpClient>>` 连单 server 同实例并发调都锁串行。
//   故「并发隔离」从结构上就**无可并发**:无共享管道、无共享表、spawn/handshake/call 全串行。
//
// 但「读码判无坑」会破「不臆造 / 真撞实证」纪律 —— 故真起 2 个 fake-server 子进程同活、两个
// read task 同时在场,在两 client 间交错 `call_tool`。关键是 **id 撞号**:两 client 各自 `next_id`
// 都从 1 起 → 两 server 都会在各自的 pending 表里见 id=1/2/3…;**若**结构上其实有共享(或读 task
// 错扇到另一 client 的表),id 撞号必然打破隔离 —— 在 A 上 `echo A7`(期望 `echo: A7`)若被错的
// read task 扇回 B 的 id=7 响应 `echo: B7`,断言当场炸。无共享 = 每调都严格返回自己的 marker。
//
// 测试故意施加**超出生产的并发压力**(`tokio::join!` 同时对两 client 发 call,A↔B ×12 轮;再对单
// client 上 `tokio::spawn` 4 任务并发池)—— 生产是串行 for-await,本测并发跑只为把「即便硬塞并发,
// 隔离仍稳」也兜住。两 client 是不同 `Arc<Mutex>`、互不阻塞,join! 跨 client 不死锁。

use std::sync::Arc;

use codeagent::config::McpServerConfig;
use codeagent::mcp::McpClient;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_mcp_servers_call_interleave_never_cross_routes() {
    let exe = env!("CARGO_BIN_EXE_codeagent-mcp-fake-server");

    // 两个独立 server 子进程(各自 cwd/进程/管道/pending map/read task)。
    let cfg_a = McpServerConfig {
        command: exe.to_string(),
        args: vec![],
        env: None,
        prefix: Some("alpha".into()), // 前缀防 codeagent 侧撞名(本测不收入 tool 表,仅体现代码路径)
        handshake_timeout_secs: Some(15),
    };
    let cfg_b = McpServerConfig {
        command: exe.to_string(),
        args: vec![],
        env: None,
        prefix: Some("beta".into()),
        handshake_timeout_secs: Some(15),
    };

    // spawn + handshake 两 server(同 main.rs 的 for-await 串行起两个)。两子进程同时存活、
    // 两个 read task 同时在场 —— 后面交错 call_tool 的跨 server 路由压力界就此建好。
    let client_a = McpClient::spawn(&cfg_a)
        .await
        .expect("server A spawn 应成功");
    {
        let mut g = client_a.lock().await;
        g.handshake().await.expect("server A 握手应成功");
        let descs = g.list_tools().await.expect("server A tools/list 应成功");
        assert_eq!(descs.len(), 1, "fake-server 只暴露一个 echo");
        assert_eq!(descs[0].name, "echo");
    }
    let client_b = McpClient::spawn(&cfg_b)
        .await
        .expect("server B spawn 应成功");
    {
        let mut g = client_b.lock().await;
        g.handshake().await.expect("server B 握手应成功");
        let descs = g.list_tools().await.expect("server B tools/list 应成功");
        assert_eq!(descs.len(), 1, "fake-server 只暴露一个 echo");
        assert_eq!(descs[0].name, "echo");
    }

    // 交错压力A↔B ×12 轮:`tokio::join!` 同时发两条 call(两 client 不同 mutex 互不阻塞)。
    // 两 client 各自 next_id=1/2/3… → 两 server 各自看到 id {1..12}。id 撞号下若 read task
    // 错扇 / pending 共享,A 的某轮响应体必含 B 的 marker,断言炸。无共享 = 每调严格自身 marker。
    for i in 1..=12 {
        let marker_a = format!("A{i}");
        let marker_b = format!("B{i}");
        let (resp_a, resp_b) = tokio::join!(
            async {
                let mut g = client_a.lock().await;
                g.call_tool("echo", serde_json::json!({ "text": marker_a }))
                    .await
            },
            async {
                let mut g = client_b.lock().await;
                g.call_tool("echo", serde_json::json!({ "text": marker_b }))
                    .await
            }
        );
        let resp_a = resp_a.expect("server A echo 应 Ok");
        let resp_b = resp_b.expect("server B echo 应 Ok");
        // 严格断「响应体只含本调自己的 marker」—— 任何含对方 marker 都意味着跨 server 路由污染。
        assert_eq!(
            resp_a,
            format!("echo: {marker_a}"),
            "交错 #{}:server A 应只回 `echo: {marker_a}`,实得 `{resp_a}` —— 若含 B marker 则 read task 错扇致跨 server 路由污染(P11-2 候选坑)",
            i
        );
        assert_eq!(
            resp_b,
            format!("echo: {marker_b}"),
            "交错 #{}:server B 应只回 `echo: {marker_b}`,实得 `{resp_b}` —— 若含 A marker 则 read task 错扇致跨 server 路由污染(P11-2 候选坑)",
            i
        );
    }

    // 同 server 单实例 4 调并发池:`Arc<Mutex<McpClient>>` 锁串行 + per-request id 唯一 + 同表按
    // id 取 —— 即便硬塞并发,每调也扇回正确自己。集严格 = {`echo: S1..S4`}(顺序因调度可变,sort 后比)。
    // 这是「per-server 自身隔离」侧验,与上面的「跨 server 隔离」主验互补。
    let mut same_server_futs = Vec::new();
    for i in 1..=4 {
        let c = Arc::clone(&client_a);
        same_server_futs.push(tokio::spawn(async move {
            let mut g = c.lock().await;
            g.call_tool("echo", serde_json::json!({ "text": format!("S{i}") }))
                .await
                .expect("server A same-server 并发 echo 应 Ok")
        }));
    }
    let mut same_results = Vec::new();
    for f in same_server_futs {
        same_results.push(f.await.expect("spawn task 不该 panic"));
    }
    same_results.sort();
    let mut want = vec!["echo: S1", "echo: S2", "echo: S3", "echo: S4"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    want.sort();
    assert_eq!(
        same_results, want,
        "server A 自身并发 4 调应严格回 S1..S4(每调自己的 marker,绝无自撞错扇)"
    );

    // 收尾:drop client 让 kill_on_drop 收子进程,防孤儿漂移。
    drop(client_a);
    drop(client_b);
    tokio::task::yield_now().await;
}
