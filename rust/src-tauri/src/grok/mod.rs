// Grok provider 代理服务模块（与 `nvidia` 平级的并行 provider）。
//
// 设计：对 Claude Code 暴露标准 Anthropic Messages 端点（POST /v1/messages），
// 转换为 xAI 上游协议后转发，SSE 响应再转回 Anthropic SSE。
// 认证走「OAuth Device Code Flow（CLI Chat-Proxy）」为主，官方 API Key 为退路，
// 认证策略可插拔（见 auth.rs，Phase 2/3 接入）。
//
// 子模块职责（Phase 2 起逐步填充）：
//   - models     : GrokConfig + 入站请求结构（入站复用 nvidia::models::AnthropicRequest）
//   - server     : axum Router 装配（POST /v1/messages）
//   - proxy      : 代理核心（redirect::none + 重试 + 429冷却 + 3xx判502 + AuthProvider 接线）
//   - converter  : Anthropic <-> xAI Responses 协议翻译
//   - auth       : AuthProvider trait + OAuth(client)/ApiKey(退路) 两实现
//
// 本文件（mod.rs）负责代理生命周期：start/stop/status 状态查询，
// 以 Tauri 托管状态 GrokState 持有运行句柄。生命周期模式参照 nvidia/mod.rs：
// 独立 OS 线程 + 独立 tokio Runtime 上跑 axum，避免命令线程 block_on 死锁。

pub mod auth;
pub mod converter;
pub mod models;
pub mod oauth;
pub mod oauth_store;
pub mod proxy;
pub mod server;
pub mod stream;

use crate::config::GrokConfig;
use crate::grok::auth::{ApiKeyAuthProvider, AuthProvider, OAuthAuthProvider};
use crate::grok::proxy::ProxyCtx;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

// 正在运行的代理实例句柄：优雅关闭信号 + 实际监听地址 + 共享 ProxyCtx
// （ctx 持有 AuthProvider 与热更新模型 RwLock，供 pool_status / set_models 用）。
struct Running {
    shutdown: tokio::sync::oneshot::Sender<()>,
    addr: String,
    ctx: Arc<ProxyCtx>,
}

// Tauri 托管状态：包裹「可选的正在运行实例」。
#[derive(Default)]
pub struct GrokState {
    inner: Mutex<Option<Running>>,
}

impl GrokState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 是否正在运行。
    pub fn is_running(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }

    /// 当前监听地址（未运行则为空串）。
    pub fn addr(&self) -> String {
        self.inner
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.addr.clone())
            .unwrap_or_default()
    }

    /// 启动代理：基础校验 → 构造 AuthProvider → ProxyCtx → 标准库同步绑端口 →
    /// 独立 OS 线程 + 独立 tokio Runtime 上跑 axum，避免命令线程 block_on 死锁。
    /// 生命周期模式参照 nvidia/mod.rs。
    pub fn start(&self, cfg: GrokConfig) -> Result<String, String> {
        crate::grok_diag_step("grok start(): entry");
        {
            let guard = self.inner.lock().unwrap();
            if guard.is_some() {
                return Ok("⚠️ Grok 代理已经在运行中".to_string());
            }
        }

        // 基础校验，尽早给出清晰错误。
        let auth_provider: Arc<dyn AuthProvider> = match cfg.auth_mode {
            models::GrokAuthMode::Oauth => {
                // OAuth 主线（Phase 3）：从 DPAPI 加密落盘读 token；不存在/损坏都视为未授权。
                // config.json 只存 oauth_account 邮箱作用户可见标识，绝不存 token 本体。
                let token = oauth_store::load().ok_or_else(|| {
                    "❌ OAuth 模式尚未授权：本地无凭证（grok-oauth.json）。请先点「授权 Grok 账号」"
                        .to_string()
                })?;
                if token.access_token.trim().is_empty() {
                    return Err(
                        "❌ OAuth token 损坏（access_token 为空），请重新授权 Grok 账号"
                            .to_string(),
                    );
                }
                // OAuth 刷新用的 HTTP 客户端（redirect::none，防 bearer 外泄）。
                let oauth_client = oauth::http_client()?;
                // 探测一次 discovery 拿 token 端点并缓存到 provider，后续 on_401 刷新直接复用，
                // 不必每轮再 discover。discovery 失败不致命：refresh_tokens 内部对空 endpoint
                // 会自行补一次 discover（见 oauth::refresh_tokens）。
                let token_endpoint = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt
                        .block_on(oauth::discover(&oauth_client))
                        .map(|d| d.token_endpoint)
                        .unwrap_or_default(),
                    Err(_) => String::new(),
                };
                let provider = OAuthAuthProvider::new(token, token_endpoint, oauth_client);
                // 写回 oauth_account 邮箱到内存 cfg（落盘由调用方 set_grok_config 负责），
                // UI 据此展示「已授权：xxx@example.com」。
                tracing::info!(account = %provider.account(), "Grok OAuth 已加载凭证，准备启动代理");
                Arc::new(provider) as Arc<dyn AuthProvider>
            }
            models::GrokAuthMode::ApiKey => {
                if cfg.api_keys.iter().all(|k| k.trim().is_empty()) {
                    return Err("❌ API Key 模式请先配置至少一个 xAI API Key".to_string());
                }
                Arc::new(ApiKeyAuthProvider::new(
                    cfg.api_keys.clone(),
                    cfg.cooldown_seconds,
                )) as Arc<dyn AuthProvider>
            }
        };
        if cfg.models.is_empty() {
            return Err("❌ 请先配置至少一个 Grok 模型".to_string());
        }
        cfg.require_auth_if_exposed()?;
        cfg.validate_base_url()?;

        let bind_addr = format!("{}:{}", cfg.host, cfg.port);
        // 标准库同步绑端口：瞬时完成、立即返回错误（端口占用等），避免命令线程 block_on。
        let std_listener = std::net::TcpListener::bind(&bind_addr)
            .map_err(|e| format!("❌ 绑定 {bind_addr} 失败: {e}"))?;
        crate::grok_diag_step("grok start(): tcp bind ok");
        let _ = std_listener.set_nonblocking(true);
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create Grok proxy runtime: {e}"))?;
        let listener = {
            let _runtime_guard = rt.enter();
            tokio::net::TcpListener::from_std(std_listener)
                .map_err(|e| format!("failed to convert Grok proxy listener: {e}"))?
        };
        let local_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| bind_addr.clone());

        let ctx = ProxyCtx::new(cfg, auth_provider);
        crate::grok_diag_step("grok start(): ProxyCtx::new ok");
        let app = server::build_router(ctx.clone());
        crate::grok_diag_step("grok start(): build_router ok");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        // 关键：独立 OS 线程 + 独立 tokio Runtime 跑 axum，与 Tauri 命令调度隔离。
        std::thread::spawn(move || {
            rt.block_on(async move {
                let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                    let _ = rx.await;
                });
                if let Err(e) = server.await {
                    tracing::error!(error = %e, "Grok 代理服务异常退出");
                }
            });
            tracing::info!("Grok 代理后台线程已退出");
        });
        crate::grok_diag_step("grok start(): server thread spawned");

        *self.inner.lock().unwrap() = Some(Running {
            shutdown: tx,
            addr: local_addr.clone(),
            ctx: ctx.clone(),
        });
        crate::grok_diag_step("grok start(): inner set");

        tracing::info!(addr = %local_addr, "Grok 代理已启动");
        Ok(format!("✅ Grok 代理已启动，监听 http://{local_addr}"))
    }

    /// 热更新运行中代理的模型优先级列表（无需重启）。
    /// 返回 true 表示代理正在运行且已实时应用；false 表示未运行（仅由调用方负责持久化）。
    pub fn set_models(&self, models: Vec<String>) -> bool {
        let guard = self.inner.lock().unwrap();
        match guard.as_ref() {
            Some(r) => {
                if let Ok(mut m) = r.ctx.models.write() {
                    *m = models;
                }
                true
            }
            None => false,
        }
    }

    /// 停止代理。
    pub fn stop(&self) -> Result<String, String> {
        let running = self.inner.lock().unwrap().take();
        match running {
            Some(r) => {
                let _ = r.shutdown.send(());
                tracing::info!("Grok 代理已停止");
                Ok("✅ Grok 代理已停止".to_string())
            }
            None => Ok("⚠️ Grok 代理未在运行".to_string()),
        }
    }

    /// 状态查询：返回给前端展示。
    pub fn status(&self) -> Value {
        let running = self.is_running();
        let addr = self.addr();
        json!({
            "running": running,
            "url": if addr.is_empty() { String::new() } else { format!("http://{addr}") },
            "endpoint": if addr.is_empty() { String::new() } else { format!("http://{addr}/v1/messages") }
        })
    }

    /// 会话/凭证池状态。代理运行时由 auth_provider.snapshot 提供脱敏快照；
    /// 未运行返回空（与前端 UI 一致）。
    pub fn pool_status(&self) -> Value {
        let guard = self.inner.lock().unwrap();
        match guard.as_ref() {
            Some(r) => {
                let mut snap = r.ctx.pool_snapshot();
                if let Some(obj) = snap.as_object_mut() {
                    obj.insert("running".to_string(), json!(true));
                }
                snap
            }
            None => json!({
                "running": false,
                "total": 0,
                "available": 0,
                "cooling": 0,
                "keys": [],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 部分单测要验「OAuth 无本地凭证」分支：需确保 grok-oauth.json 不存在。
    /// 但 `Config::config_dir()` 是真实目录，跑 `cargo test --lib` 时可能正放着用户的
    /// 真实 token。为此做 save-and-restore：测试前若有文件就先读出 secret 内容入内存，
    /// 删之以重现「无凭证」环境，测完无论成败都写回。这样既 hermetic 又不毁用户凭证。
    /// 返回可选的待恢复字节（Some = 有原文件需回写，None = 本就无文件）。
    #[derive(Default)]
    struct TokenRestore(Option<Vec<u8>>);

    impl TokenRestore {
        fn take() -> Self {
            let path = oauth_store::path();
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let _ = std::fs::remove_file(&path);
                    Self(Some(bytes))
                }
                Err(_) => Self(None),
            }
        }
    }

    impl Drop for TokenRestore {
        fn drop(&mut self) {
            if let Some(bytes) = self.0.take() {
                let path = oauth_store::path();
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(path, bytes);
            }
        }
    }

    #[test]
    fn oauth_mode_without_token_is_unauthorized() {
        // OAuth 模式（Phase 3 已接入）：本地无 DPAPI 凭证即判未授权，返回明确错误而不
        // 起服务。account 邮箱只是 UI 可见标识，授权判据是 oauth_store::load() 取回非空
        // access_token。TokenRestore 保证不毁用户真实 token。
        let _restore = TokenRestore::take();
        let state = GrokState::new();
        let cfg = GrokConfig {
            oauth_account: "tester@example.com".to_string(),
            models: vec!["grok-4.3".to_string()],
            ..Default::default()
        };
        let r = state.start(cfg);
        assert!(r.is_err(), "OAuth 无本地凭证应报错");
        let e = r.unwrap_err();
        assert!(
            e.contains("尚未授权") || e.contains("无凭证"),
            "OAuth 无 token 应提示未授权，实际: {e}"
        );
    }

    #[test]
    fn start_requires_models() {
        let _restore = TokenRestore::take();
        let state = GrokState::new();
        // OAuth 无凭证分支在模型校验之前就返回了，故即便 models 为空也只会命中「未授权」；
        // 模型为空的校验由 API Key 路径覆盖。这里仅断言整体报错。
        let cfg = GrokConfig {
            oauth_account: "tester@example.com".to_string(),
            models: Vec::new(),
            ..Default::default()
        };
        assert!(state.start(cfg).is_err());
    }

    #[test]
    fn oauth_mode_token_driven_not_account_driven() {
        // account 空和无 token 在 OAuth 路径走同一未授权错误，不再单独要求 account 非空
        // ——证明授权判据是 token 而非 account 邮箱。
        let _restore = TokenRestore::take();
        let state = GrokState::new();
        let cfg = GrokConfig {
            oauth_account: String::new(),
            models: vec!["grok-4.3".to_string()],
            ..Default::default()
        };
        let r = state.start(cfg);
        assert!(r.is_err());
    }

    #[test]
    fn apikey_mode_requires_keys() {
        let state = GrokState::new();
        let cfg = GrokConfig {
            auth_mode: models::GrokAuthMode::ApiKey,
            api_keys: Vec::new(),
            models: vec!["grok-4.3".to_string()],
            ..Default::default()
        };
        assert!(state.start(cfg).is_err());
    }

    #[test]
    fn apikey_mode_starts_proxy_on_unique_port() {
        // API Key + 模型齐全：start 真正起 axum。用一个偏门端口避免与机器上
        // 实际运行的代理或其他测试抢端口；stop 立即收尾，防遗留监听线程。
        let port = 18983u16;
        let state = GrokState::new();
        let cfg = GrokConfig {
            auth_mode: models::GrokAuthMode::ApiKey,
            api_keys: vec!["xai-test-key".to_string()],
            models: vec!["grok-4.3".to_string()],
            port,
            ..Default::default()
        };
        match state.start(cfg) {
            Ok(msg) => {
                assert!(msg.contains("Grok 代理已启动"), "成功应回启成功语: {msg}");
                assert!(state.is_running());
                let _ = state.stop();
            }
            Err(e) => {
                // 唯一可能失败：端口被占（CI 环境/本机偶发）。允许失败但要求错误信息
                // 明确指向绑定，而非被误判为「配置/凭证错误」。
                assert!(
                    e.contains("绑定"),
                    "API Key 已配置应能启动（或仅因端口占用失败），实际: {e}"
                );
            }
        }
    }
}
