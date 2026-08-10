// 真实端到端冒烟测试（无头）：直接复用网关同一套模块拉起 8083，
// 发一条 Anthropic 格式请求 → 网关转 OpenAI Chat Completions 打到真实上游
// → 响应转回 Anthropic 形态。
//
// 密钥只通过环境变量进入进程，绝不写盘、不在输出里回显：
//   DEEPSEEK_API_KEY=sk-xxx  cargo run --example gateway_smoke -- --nocapture
// 可用 GATEWAY_TEST_BASE_URL / GATEWAY_TEST_MODEL / GATEWAY_TEST_PORT 覆盖默认。

use std::sync::Arc;

use claude_launcher_lib::gateway::config::GatewayConfig;
use claude_launcher_lib::gateway::state::GatewayState;
use claude_launcher_lib::stats::UsageStatsStore;

fn main() {
    let key = match std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("GATEWAY_TEST_KEY").ok())
        .filter(|s| !s.trim().is_empty())
    {
        Some(k) => k,
        None => {
            eprintln!("skip: 设置 DEEPSEEK_API_KEY 以运行真实网关冒烟测试");
            return;
        }
    };

    let base_url = std::env::var("GATEWAY_TEST_BASE_URL")
        .unwrap_or_else(|_| "https://api.deepseek.com".to_string());
    let model =
        std::env::var("GATEWAY_TEST_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".to_string());
    let fallback = std::env::var("GATEWAY_TEST_MODEL_FALLBACK")
        .unwrap_or_else(|_| "deepseek-v4-pro".to_string());
    let port: u16 = std::env::var("GATEWAY_TEST_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8083);
    let anthropic_model = "claude-sonnet-4";

    // ===== UI 契约验证：模拟前端点「保存网关配置」时发出的 payload =====
    // 这段代码等价于 Tauri 在 set_gateway_config(gateway) 内部做的反序列化步骤。
    // JSON 的键名/取值完全复刻 GatewayPage.collect() + persistConfig 的输出
    // （见 src/pages/GatewayPage.tsx）：snake_case 字段、protocol="chat-completions"、
    // auth_mode="api-key"、model_map 用 {anthropic_model, provider_model}。
    // 只要这里能反序列化 + validate_all + active() 解析出 provider，
    // 就证明「UI 配置 provider → 后端接收」这条链路字段契约正确。
    let ui_payload = serde_json::json!({
        "providers": [{
            "id": "deepseek",
            "name": "DeepSeek",
            "protocol": "chat-completions",
            "auth_mode": "api-key",
            "base_url": base_url,
            "api_keys": [key],
            "models": [model, fallback],
            "model_map": [{ "anthropic_model": anthropic_model, "provider_model": model }],
            "host": "127.0.0.1",
            "port": port,
            "cooldown_seconds": 60,
            "max_retries": 2,
            "request_timeout_seconds": 60,
            "auth_token": ""
        }],
        "active_provider": "deepseek"
    });
    let gw: GatewayConfig = serde_json::from_value(ui_payload.clone())
        .expect("UI payload 反序列化失败：前端字段与后端 ProviderEntry 契约不匹配");
    gw.validate_all()
        .expect("validate_all 失败（SSRF/鉴权闸拦截）");
    let provider_from_ui = gw
        .active()
        .expect("active() 未解析出 provider（active_provider 指向无效 id）")
        .clone();
    eprintln!(
        "✅ UI契约校验通过：反序列化 + validate_all + active() => provider={} ({} 模型)",
        provider_from_ui.name,
        provider_from_ui.models.len()
    );

    // 下面复用 UI 配置出的 provider 启动真实代理（等价于 gateway_start 读 cfg.active()）
    let provider = provider_from_ui;

    let state = GatewayState::new();
    let msg = state
        .start(provider, Arc::new(UsageStatsStore::in_memory()))
        .expect("网关启动失败");
    eprintln!("{msg}");

    // 给后台线程一点时间绑端口
    std::thread::sleep(std::time::Duration::from_millis(500));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let port2 = port;
    let exit_code = rt.block_on(async move {
        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "model": anthropic_model,
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "Reply with exactly the word: PONG"}]
        });
        let resp = client
            .post(format!("http://127.0.0.1:{port2}/v1/messages"))
            .json(&body)
            .send()
            .await
            .expect("请求 8083 失败");

        let status = resp.status();
        let text = resp.text().await.expect("读取响应失败");
        // 不回显密钥；仅打印状态码与前若干字符做形态校验
        eprintln!("status={status}");
        eprintln!("resp_head={}", &text.chars().take(600).collect::<String>());

        if status.is_success() && (text.contains("\"content\"") || text.contains("data:")) {
            eprintln!("SMOKE_OK: 网关成功把 Anthropic 请求转成 OpenAI 并转回");
            0
        } else {
            eprintln!("SMOKE_FAIL: 网关响应异常");
            1
        }
    });

    state.stop().ok();
    std::process::exit(exit_code);
}
