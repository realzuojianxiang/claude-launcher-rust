// 协议网关代理生命周期管理：start/stop/status。
//
// 独立 OS 线程 + 独立 tokio Runtime 上跑 axum，避免命令线程 block_on 死锁
// （生命周期模式参照 nvidia/mod.rs）。
//
// 设计要点：
//   - 入参 ProviderEntry（含 protocol / auth 等字段）
//   - 构造 AuthProvider：统一用 API Key 实现（网关仅支持 OpenAI 兼容端点的 API Key 认证）
//   - 校验：provider 自身的 validate（config::validate_all 已在 set_gateway_config 做过）

use crate::gateway::auth::{ApiKeyAuthProvider, AuthProvider};
use crate::gateway::config::ProviderEntry;
use crate::gateway::proxy::ProxyCtx;
use crate::stats::UsageStatsStore;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct Running {
    shutdown: tokio::sync::oneshot::Sender<()>,
    addr: String,
    ctx: Arc<ProxyCtx>,
}

#[derive(Default)]
pub struct GatewayState {
    inner: Mutex<Option<Running>>,
}

impl GatewayState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }

    pub fn addr(&self) -> String {
        self.inner
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.addr.clone())
            .unwrap_or_default()
    }

    /// 启动代理：校验 → 构造 AuthProvider → ProxyCtx → 绑端口 → 独立线程跑 axum。
    pub fn start(
        &self,
        provider: ProviderEntry,
        stats: Arc<UsageStatsStore>,
    ) -> Result<String, String> {
        {
            let guard = self.inner.lock().unwrap();
            if guard.is_some() {
                return Ok("⚠️ 协议网关已经在运行中".to_string());
            }
        }

        // 仅支持 API Key 模式：至少一个 Key。
        if provider.api_keys.iter().all(|k| k.trim().is_empty()) {
            return Err(format!(
                "❌ API Key 模式请先配置至少一个 {} API Key",
                provider.name
            ));
        }
        let auth_provider: Arc<dyn AuthProvider> = Arc::new(ApiKeyAuthProvider::new(
            provider.api_keys.clone(),
            provider.cooldown_seconds,
        ));

        if provider.models.is_empty() {
            return Err(format!("❌ 请先配置至少一个 {} 模型", provider.name));
        }
        crate::shared::require_auth_if_exposed(&provider.host, &provider.auth_token)?;
        crate::shared::validate_base_url(
            crate::gateway::config::effective_base_url(&provider),
            &provider.name,
        )?;

        let bind_addr = format!("{}:{}", provider.host, provider.port);
        let std_listener = std::net::TcpListener::bind(&bind_addr)
            .map_err(|e| format!("❌ 绑定 {bind_addr} 失败: {e}"))?;
        let _ = std_listener.set_nonblocking(true);
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create gateway proxy runtime: {e}"))?;
        let listener = {
            let _runtime_guard = rt.enter();
            tokio::net::TcpListener::from_std(std_listener)
                .map_err(|e| format!("failed to convert gateway proxy listener: {e}"))?
        };
        let local_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| bind_addr.clone());

        let ctx = ProxyCtx::new(provider, auth_provider, stats.clone());
        let app = crate::gateway::server::build_router(ctx.clone());
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        std::thread::spawn(move || {
            rt.block_on(async move {
                let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                    let _ = rx.await;
                });
                if let Err(e) = server.await {
                    tracing::error!(error = %e, "协议网关服务异常退出");
                }
            });
            tracing::info!("协议网关后台线程已退出");
        });

        *self.inner.lock().unwrap() = Some(Running {
            shutdown: tx,
            addr: local_addr.clone(),
            ctx: ctx.clone(),
        });

        tracing::info!(addr = %local_addr, "协议网关已启动");
        Ok(format!("✅ 协议网关已启动，监听 http://{local_addr}"))
    }

    /// 热更新运行中代理的模型优先级列表。
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

    pub fn stop(&self) -> Result<String, String> {
        let running = self.inner.lock().unwrap().take();
        match running {
            Some(r) => {
                let _ = r.shutdown.send(());
                tracing::info!("协议网关已停止");
                Ok("✅ 协议网关已停止".to_string())
            }
            None => Ok("⚠️ 协议网关未在运行".to_string()),
        }
    }

    pub fn status(&self) -> Value {
        let running = self.is_running();
        let addr = self.addr();
        json!({
            "running": running,
            "url": if addr.is_empty() { String::new() } else { format!("http://{addr}") },
            "endpoint": if addr.is_empty() { String::new() } else { format!("http://{addr}/v1/messages") }
        })
    }

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
