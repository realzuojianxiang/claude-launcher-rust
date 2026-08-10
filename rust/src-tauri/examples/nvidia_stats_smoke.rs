// 无头端到端冒烟：验证「8082（NVIDIA 代理）的请求 → UsageStatsStore → UI 面板快照」
// 这条统计链路真的通。
//
// 不需要真实 NVIDIA Key：本地拉起一个 mock OpenAI 上游（/v1/chat/completions），
// 把 NvidiaConfig.base_url 指向它，然后经真实监听端口发两条 Anthropic 请求
// （一条非流式、一条流式），最后读 UsageStatsStore::snapshot —— 这正是 Tauri 命令
// get_usage_stats 返回给前端 DashboardPage 的同一份数据。
//
// 运行：cargo run --example nvidia_stats_smoke
// （本沙箱 `cargo test --lib` 的 libtest harness 会 0xc0000139 崩溃，故 e2e 走 examples。）

use std::sync::Arc;
use std::time::Duration;

use axum::{body::Body, http::header, response::Response, routing::post, Json, Router};
use claude_launcher_lib::config::NvidiaConfig;
use claude_launcher_lib::nvidia::NvidiaState;
use claude_launcher_lib::stats::{UsageRange, UsageStatsStore};
use serde_json::{json, Value};

// 非流式用量：prompt 12 + completion 5 = 17
const NON_STREAM_INPUT: u64 = 12;
const NON_STREAM_OUTPUT: u64 = 5;
// 流式用量：prompt 120 + completion 50 = 170（其中 cached 20，只影响回给客户端的
// billable_input，统计记录的是上游原始 prompt_tokens）
const STREAM_INPUT: u64 = 120;
const STREAM_OUTPUT: u64 = 50;

const MODEL: &str = "mock-model";

async fn mock_chat_completions(Json(body): Json<Value>) -> Response {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    if stream {
        let chunk1 = json!({
            "choices": [{ "delta": { "content": format!("hi {model}") }, "finish_reason": null }]
        })
        .to_string();
        let chunk2 = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": {
                "prompt_tokens": STREAM_INPUT,
                "completion_tokens": STREAM_OUTPUT,
                "prompt_tokens_details": { "cached_tokens": 20 }
            }
        })
        .to_string();
        let payload = format!("data: {chunk1}\n\ndata: {chunk2}\n\ndata: [DONE]\n\n");
        return Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(payload))
            .unwrap();
    }

    let payload = json!({
        "id": "chatcmpl_mock",
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": { "role": "assistant", "content": format!("hello from {model}") }
        }],
        "usage": {
            "prompt_tokens": NON_STREAM_INPUT,
            "completion_tokens": NON_STREAM_OUTPUT
        }
    })
    .to_string();
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap()
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

async fn wait_ready(port: u16) {
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("代理端口 {port} 未在 5s 内就绪");
}

fn check(label: &str, ok: bool, detail: String) -> bool {
    println!("{} {label}: {detail}", if ok { "PASS" } else { "FAIL" });
    ok
}

async fn run() -> bool {
    // 1. mock 上游
    let mock = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let mock_addr = mock.local_addr().unwrap();
    let mock_app = Router::new().route("/v1/chat/completions", post(mock_chat_completions));
    tokio::spawn(async move {
        let _ = axum::serve(mock, mock_app).await;
    });
    println!("mock 上游: http://{mock_addr}/v1");

    // 2. 独立的统计文件（临时目录），既验证内存聚合也验证落盘
    let dir = std::env::temp_dir().join(format!(
        "claude-launcher-stats-smoke-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let stats_path = dir.join("usage-stats.json");
    let store = Arc::new(UsageStatsStore::from_path(stats_path.clone()).expect("stats store"));

    // 3. 拉起 8082 代理（用随机空闲端口，避免和用户真实运行中的 8082 打架）
    let port = free_port();
    let cfg = NvidiaConfig {
        api_keys: vec!["mock-key".to_string()],
        models: vec![MODEL.to_string()],
        base_url: format!("http://{mock_addr}/v1"),
        host: "127.0.0.1".to_string(),
        port,
        key_cooldown_seconds: 65,
        max_retries: 3,
        request_timeout_seconds: 30,
        auth_token: String::new(),
    };
    let state = NvidiaState::new();
    match state.start(cfg, Arc::clone(&store)) {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            println!("FAIL 启动代理: {e}");
            return false;
        }
    }
    wait_ready(port).await;

    let endpoint = format!("http://127.0.0.1:{port}/v1/messages");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("client");

    let mut all_ok = true;

    // 4a. 非流式请求
    let resp = client
        .post(&endpoint)
        .json(&json!({
            "model": MODEL,
            "max_tokens": 128,
            "messages": [{ "role": "user", "content": "ping" }],
            "stream": false
        }))
        .send()
        .await
        .expect("非流式请求发送失败");
    let status = resp.status();
    let body: Value = resp.json().await.expect("非流式响应不是 JSON");
    all_ok &= check(
        "非流式 /v1/messages 返回 200",
        status.as_u16() == 200,
        format!("status={status}, type={:?}", body.get("type")),
    );

    // 4b. 流式请求
    let resp = client
        .post(&endpoint)
        .json(&json!({
            "model": MODEL,
            "max_tokens": 128,
            "messages": [{ "role": "user", "content": "ping" }],
            "stream": true
        }))
        .send()
        .await
        .expect("流式请求发送失败");
    let status = resp.status();
    let text = resp.text().await.expect("读取 SSE 失败");
    all_ok &= check(
        "流式 /v1/messages 返回 200 且含 message_stop",
        status.as_u16() == 200 && text.contains("message_stop"),
        format!("status={status}, bytes={}", text.len()),
    );

    // 5. 读 UI 面板同源快照
    let expect_total = NON_STREAM_INPUT + NON_STREAM_OUTPUT + STREAM_INPUT + STREAM_OUTPUT;

    for (label, range) in [
        ("live", UsageRange::Live),
        ("7d", UsageRange::Days7),
        ("all", UsageRange::All),
    ] {
        let snap = store.snapshot(range);
        let nvidia = snap.providers.iter().find(|p| p.provider == "nvidia");
        all_ok &= check(
            &format!("[{label}] providers 含 nvidia"),
            nvidia.is_some(),
            format!(
                "providers={:?}",
                snap.providers
                    .iter()
                    .map(|p| p.provider.as_str())
                    .collect::<Vec<_>>()
            ),
        );
        if let Some(p) = nvidia {
            all_ok &= check(
                &format!("[{label}] nvidia 请求数=2"),
                p.requests == 2,
                format!("requests={}", p.requests),
            );
            all_ok &= check(
                &format!("[{label}] nvidia token 合计={expect_total}"),
                p.total_tokens == expect_total,
                format!("total_tokens={}", p.total_tokens),
            );
        }

        all_ok &= check(
            &format!("[{label}] totals 无失败/无缺失用量"),
            snap.totals.requests == 2
                && snap.totals.failed_requests == 0
                && snap.totals.usage_missing_requests == 0,
            format!(
                "requests={}, failed={}, usage_missing={}, in={}, out={}",
                snap.totals.requests,
                snap.totals.failed_requests,
                snap.totals.usage_missing_requests,
                snap.totals.input_tokens,
                snap.totals.output_tokens
            ),
        );

        let model_row = snap
            .models
            .iter()
            .find(|m| m.provider == "nvidia" && m.model == MODEL);
        all_ok &= check(
            &format!("[{label}] 模型明细含 nvidia/{MODEL}"),
            model_row.map(|m| m.total_tokens) == Some(expect_total),
            format!(
                "rows={:?}",
                snap.models
                    .iter()
                    .map(|m| format!("{}/{}={}", m.provider, m.model, m.total_tokens))
                    .collect::<Vec<_>>()
            ),
        );

        let trend_total: u64 = snap.trend.iter().map(|t| t.total_tokens).sum();
        all_ok &= check(
            &format!("[{label}] 趋势图有数据点"),
            !snap.trend.is_empty() && trend_total == expect_total,
            format!("points={}, sum={trend_total}", snap.trend.len()),
        );
    }

    // 6. 落盘验证（重启 app 后 7d/30d/all 还能看到）
    let on_disk = std::fs::read_to_string(&stats_path).unwrap_or_default();
    all_ok &= check(
        "usage-stats.json 已落盘且含 nvidia",
        on_disk.contains("nvidia") && on_disk.contains(MODEL),
        format!("{} bytes @ {}", on_disk.len(), stats_path.display()),
    );

    let _ = state.stop();
    let _ = std::fs::remove_dir_all(&dir);
    all_ok
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let ok = rt.block_on(run());
    println!(
        "\n==== 8082 用量统计链路 {} ====",
        if ok { "全部通过" } else { "存在失败" }
    );
    if !ok {
        std::process::exit(1);
    }
}
