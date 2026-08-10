// 无头端到端冒烟：验证「8084（OpenAI 透传网关）的请求 → UsageStatsStore → UI 面板快照」
// 这条统计链路真的通，且 chat/completions 与 responses 两种路由、stream/非 stream 两种模式、
// 多个 provider（deepseek / glm-5.2）路由都正常。
//
// 不需要真实 provider Key：本地拉起一个 mock OpenAI 上游（同时支持 /v1/chat/completions 与
// /v1/responses，stream 与 非 stream），把两个 provider 的 base_url 都指向它；8084 按请求体
// model 字段路由到对应 provider 并转发，最后读 UsageStatsStore::snapshot —— 这正是 Tauri 命令
// get_usage_stats 返回给前端 DashboardPage 的同一份数据。
//
// 运行：cargo run --example openai_gateway_smoke
// （本沙箱 `cargo test --lib` 的 libtest harness 会 0xc0000139 崩溃，故 e2e 走 examples。）

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    http::header,
    response::Response,
    routing::{post, Router},
    Json,
};
use claude_launcher_lib::gateway::config::{AuthMode, GatewayConfig, ProtocolKind, ProviderEntry};
use claude_launcher_lib::openai_gateway::state::OpenAiGatewayState;
use claude_launcher_lib::stats::{UsageRange, UsageStatsStore};
use serde_json::{json, Value};

// 非流式用量：prompt 12 + completion 5 = 17
const NON_STREAM_INPUT: u64 = 12;
const NON_STREAM_OUTPUT: u64 = 5;
// 流式用量：prompt 120 + completion 50 = 170
const STREAM_INPUT: u64 = 120;
const STREAM_OUTPUT: u64 = 50;

const DEEPSEEK_MODEL: &str = "deepseek-chat";
const GLM_MODEL: &str = "glm-5.2";

// mock 上游：依 path 区分 chat/completions 与 responses，依 body.stream 区分流式。
async fn mock_handler(Json(body): Json<Value>) -> Response {
    let path = body
        .get("__mock_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let is_responses = path.contains("/responses");
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    if is_responses {
        if stream {
            // Responses API 流式：event: response.completed 的 data 带 usage（input/output）
            let created = json!({
                "type": "response.created",
                "usage": { "input_tokens": 0, "output_tokens": 0 }
            })
            .to_string();
            let completed = json!({
                "type": "response.completed",
                "usage": { "input_tokens": STREAM_INPUT, "output_tokens": STREAM_OUTPUT }
            })
            .to_string();
            let payload = format!(
                "event: response.created\ndata: {created}\n\nevent: response.completed\ndata: {completed}\n\n"
            );
            return Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(payload))
                .unwrap();
        }
        let payload = json!({
            "id": "resp_mock",
            "output": [{ "type": "message", "content": [{ "type": "output_text", "text": "hi from responses" }] }],
            "usage": { "input_tokens": NON_STREAM_INPUT, "output_tokens": NON_STREAM_OUTPUT }
        })
        .to_string();
        return Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload))
            .unwrap();
    }

    // chat/completions
    if stream {
        let chunk1 = json!({
            "choices": [{ "delta": { "content": "hi" }, "finish_reason": null }]
        })
        .to_string();
        let chunk2 = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": STREAM_INPUT, "completion_tokens": STREAM_OUTPUT }
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
            "message": { "role": "assistant", "content": "hello" }
        }],
        "usage": { "prompt_tokens": NON_STREAM_INPUT, "completion_tokens": NON_STREAM_OUTPUT }
    })
    .to_string();
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .unwrap()
}

fn check(label: &str, ok: bool, detail: String) -> bool {
    println!("{} {label}: {detail}", if ok { "PASS" } else { "FAIL" });
    ok
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

async fn run() -> bool {
    // 1. mock 上游（随机空闲端口）
    let mock = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let mock_addr = mock.local_addr().unwrap();
    // 用 axum 中间件把 path 带进 handler（handler 通过 body.__mock_path 判断路由）
    let mock_app = Router::new()
        .route(
            "/v1/chat/completions",
            post(|body: Json<Value>| async move {
                let mut v = body.0;
                v["__mock_path"] = json!("/v1/chat/completions");
                mock_handler(Json(v)).await
            }),
        )
        .route(
            "/v1/responses",
            post(|body: Json<Value>| async move {
                let mut v = body.0;
                v["__mock_path"] = json!("/v1/responses");
                mock_handler(Json(v)).await
            }),
        );
    tokio::spawn(async move {
        let _ = axum::serve(mock, mock_app).await;
    });
    println!("mock 上游: http://{mock_addr}/v1");

    // 2. 独立统计文件（临时目录）
    let dir = std::env::temp_dir().join(format!(
        "claude-launcher-openai-gw-smoke-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let stats_path = dir.join("usage-stats.json");
    let store = Arc::new(UsageStatsStore::from_path(stats_path.clone()).expect("stats store"));

    // 3. 构造两个 provider，base_url 都指向 mock（base 含 /v1，8084 会拼出 /v1/chat/completions）
    let mk_provider = |id: &str, name: &str, models: Vec<&str>| ProviderEntry {
        id: id.to_string(),
        name: name.to_string(),
        protocol: ProtocolKind::ChatCompletions,
        auth_mode: AuthMode::ApiKey,
        base_url: format!("http://{mock_addr}/v1"),
        api_keys: vec!["sk-mock".to_string()],
        models: models.into_iter().map(|s| s.to_string()).collect(),
        model_map: vec![],
        host: "127.0.0.1".to_string(),
        port: 8084,
        cooldown_seconds: 600,
        max_retries: 3,
        request_timeout_seconds: 30,
        auth_token: String::new(),
    };
    let config = GatewayConfig {
        providers: vec![
            mk_provider("deepseek", "DeepSeek", vec![DEEPSEEK_MODEL]),
            mk_provider("glm", "GLM", vec![GLM_MODEL]),
        ],
        active_provider: "deepseek".to_string(),
    };

    // 4. 启动 8084 网关
    let state = OpenAiGatewayState::new();
    match state.start(config, Arc::clone(&store)) {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            println!("FAIL 启动 8084 网关: {e}");
            return false;
        }
    }
    wait_ready(8084).await;
    println!("8084 网关就绪: http://127.0.0.1:8084");

    let base = "http://127.0.0.1:8084";
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("client");

    let mut all_ok = true;

    // 5a. chat/completions 非流式（model=deepseek-chat → 路由到 deepseek provider）
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": DEEPSEEK_MODEL,
            "messages": [{ "role": "user", "content": "ping" }],
            "stream": false
        }))
        .send()
        .await
        .expect("chat 非流式发送失败");
    let status = resp.status();
    let body: Value = resp.json().await.expect("chat 非流式响应不是 JSON");
    all_ok &= check(
        "chat/completions 非流式返回 200",
        status.as_u16() == 200 && body.get("choices").is_some(),
        format!("status={status}"),
    );

    // 5b. chat/completions 流式（model=deepseek-chat → deepseek provider；8084 注入 include_usage）
    let resp = client
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": DEEPSEEK_MODEL,
            "messages": [{ "role": "user", "content": "ping" }],
            "stream": true
        }))
        .send()
        .await
        .expect("chat 流式发送失败");
    let status = resp.status();
    let text = resp.text().await.expect("读 chat SSE 失败");
    all_ok &= check(
        "chat/completions 流式返回 200 且含 [DONE]",
        status.as_u16() == 200 && text.contains("[DONE]"),
        format!("status={status}, bytes={}", text.len()),
    );

    // 5c. responses 非流式（model=glm-5.2 → 路由到 glm provider）
    let resp = client
        .post(format!("{base}/v1/responses"))
        .json(&json!({
            "model": GLM_MODEL,
            "input": "ping",
            "stream": false
        }))
        .send()
        .await
        .expect("responses 非流式发送失败");
    let status = resp.status();
    let body: Value = resp.json().await.expect("responses 非流式响应不是 JSON");
    all_ok &= check(
        "responses 非流式返回 200",
        status.as_u16() == 200 && body.get("usage").is_some(),
        format!("status={status}"),
    );

    // 5d. responses 流式（model=glm-5.2 → glm provider）
    let resp = client
        .post(format!("{base}/v1/responses"))
        .json(&json!({
            "model": GLM_MODEL,
            "input": "ping",
            "stream": true
        }))
        .send()
        .await
        .expect("responses 流式发送失败");
    let status = resp.status();
    let text = resp.text().await.expect("读 responses SSE 失败");
    all_ok &= check(
        "responses 流式返回 200 且含 response.completed",
        status.as_u16() == 200 && text.contains("response.completed"),
        format!("status={status}, bytes={}", text.len()),
    );

    // 6. 读 UI 面板同源快照
    let per_provider = NON_STREAM_INPUT + NON_STREAM_OUTPUT + STREAM_INPUT + STREAM_OUTPUT; // 187
    let expect_total = per_provider * 2; // deepseek + glm

    for (label, range) in [
        ("live", UsageRange::Live),
        ("7d", UsageRange::Days7),
        ("all", UsageRange::All),
    ] {
        let snap = store.snapshot(range);

        let ds = snap.providers.iter().find(|p| p.provider == "deepseek");
        let glm = snap.providers.iter().find(|p| p.provider == "glm");
        all_ok &= check(
            &format!("[{label}] providers 含 deepseek 与 glm"),
            ds.is_some() && glm.is_some(),
            format!(
                "providers={:?}",
                snap.providers
                    .iter()
                    .map(|p| p.provider.as_str())
                    .collect::<Vec<_>>()
            ),
        );

        if let Some(p) = ds {
            all_ok &= check(
                &format!("[{label}] deepseek 请求数=2 且 token={per_provider}"),
                p.requests == 2 && p.total_tokens == per_provider,
                format!("requests={}, total={}", p.requests, p.total_tokens),
            );
        }
        if let Some(p) = glm {
            all_ok &= check(
                &format!("[{label}] glm 请求数=2 且 token={per_provider}"),
                p.requests == 2 && p.total_tokens == per_provider,
                format!("requests={}, total={}", p.requests, p.total_tokens),
            );
        }

        all_ok &= check(
            &format!("[{label}] totals: 4 请求 / 0 失败 / 0 缺失"),
            snap.totals.requests == 4
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

        let trend_total: u64 = snap.trend.iter().map(|t| t.total_tokens).sum();
        all_ok &= check(
            &format!("[{label}] 趋势图有数据点且合计={expect_total}"),
            !snap.trend.is_empty() && trend_total == expect_total,
            format!("points={}, sum={trend_total}", snap.trend.len()),
        );
    }

    // 7. 落盘验证
    let on_disk = std::fs::read_to_string(&stats_path).unwrap_or_default();
    all_ok &= check(
        "usage-stats.json 已落盘且含 deepseek 与 glm",
        on_disk.contains("deepseek") && on_disk.contains("glm"),
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
        "\n==== 8084 OpenAI 透传网关统计链路 {} ====",
        if ok { "全部通过" } else { "存在失败" }
    );
    if !ok {
        std::process::exit(1);
    }
}
