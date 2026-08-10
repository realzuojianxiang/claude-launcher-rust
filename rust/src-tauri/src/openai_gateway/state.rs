// 8084 OpenAI 透传网关生命周期管理：start/stop/status。
//
// 与 gateway::state（8083）同源模式：独立 OS 线程 + 独立 tokio Runtime 跑 axum，
// 避免命令线程 block_on 死锁。区别在于 8084 持有整份 GatewayConfig（多 provider），
// 启动时为每个 provider 构造一份 ApiKeyAuthProvider 放进 auth_map，请求时按 model 路由。

use crate::gateway::auth::{ApiKeyAuthProvider, AuthProvider};
use crate::gateway::config::{self, GatewayConfig};
use crate::openai_gateway::proxy::ProxyCtx;
use crate::stats::UsageStatsStore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 8084 固定监听端口（8082=NVIDIA / 8083=协议网关 / 8084=OpenAI 透传）。
pub const OPENAI_GATEWAY_PORT: u16 = 8084;

struct Running {
    shutdown: tokio::sync::oneshot::Sender<()>,
    addr: String,
    ctx: Arc<ProxyCtx>,
}

#[derive(Default)]
pub struct OpenAiGatewayState {
    inner: Mutex<Option<Running>>,
}

impl OpenAiGatewayState {
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

    /// 启动代理：校验 providers → 为每个 provider 构造 AuthProvider → 绑 8084 → 独立线程跑 axum。
    pub fn start(
        &self,
        config: GatewayConfig,
        stats: Arc<UsageStatsStore>,
    ) -> Result<String, String> {
        {
            let guard = self.inner.lock().unwrap();
            if guard.is_some() {
                return Ok("⚠️ OpenAI 透传网关已在运行中".to_string());
            }
        }

        if config.providers.is_empty() {
            return Err(
                "❌ 未配置任何 provider，请先在「协议网关」页面添加并配置 provider".to_string(),
            );
        }
        for p in &config.providers {
            if p.api_keys.iter().all(|k| k.trim().is_empty()) {
                return Err(format!("❌ provider「{}」未配置任何 API Key", p.name));
            }
            crate::shared::validate_base_url(config::effective_base_url(p), &p.name)?;
            crate::shared::require_auth_if_exposed(&p.host, &p.auth_token)?;
        }

        // 为每个 provider 构造独立 Key 池（冷却状态各自独立，跨请求持续）
        let mut auth_map: HashMap<String, Arc<dyn AuthProvider>> = HashMap::new();
        for p in &config.providers {
            auth_map.insert(
                p.id.clone(),
                Arc::new(ApiKeyAuthProvider::new(
                    p.api_keys.clone(),
                    p.cooldown_seconds,
                )),
            );
        }

        let bind_addr = format!("127.0.0.1:{OPENAI_GATEWAY_PORT}");
        let std_listener = std::net::TcpListener::bind(&bind_addr)
            .map_err(|e| format!("❌ 绑定 {bind_addr} 失败: {e}"))?;
        let _ = std_listener.set_nonblocking(true);
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create openai gateway runtime: {e}"))?;
        let listener = {
            let _runtime_guard = rt.enter();
            tokio::net::TcpListener::from_std(std_listener)
                .map_err(|e| format!("failed to convert openai gateway listener: {e}"))?
        };
        let local_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| bind_addr.clone());

        let ctx = ProxyCtx::new(config, auth_map, stats.clone());
        let app = crate::openai_gateway::server::build_router(ctx.clone());
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        std::thread::spawn(move || {
            rt.block_on(async move {
                let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                    let _ = rx.await;
                });
                if let Err(e) = server.await {
                    tracing::error!(error = %e, "OpenAI 透传网关异常退出");
                }
            });
            tracing::info!("OpenAI 透传网关后台线程已退出");
        });

        *self.inner.lock().unwrap() = Some(Running {
            shutdown: tx,
            addr: local_addr.clone(),
            ctx: ctx.clone(),
        });

        tracing::info!(addr = %local_addr, "OpenAI 透传网关已启动");
        Ok(format!(
            "✅ OpenAI 透传网关已启动，监听 http://{local_addr}"
        ))
    }

    pub fn stop(&self) -> Result<String, String> {
        let running = self.inner.lock().unwrap().take();
        match running {
            Some(r) => {
                let _ = r.shutdown.send(());
                tracing::info!("OpenAI 透传网关已停止");
                Ok("✅ OpenAI 透传网关已停止".to_string())
            }
            None => Ok("⚠️ OpenAI 透传网关未在运行".to_string()),
        }
    }

    pub fn status(&self) -> Value {
        let running = self.is_running();
        let addr = self.addr();
        let base = if addr.is_empty() {
            String::new()
        } else {
            format!("http://{addr}")
        };
        json!({
            "running": running,
            "url": base,
            "endpoint": if base.is_empty() { String::new() } else { format!("{base}/v1/chat/completions") },
            "endpoints": if base.is_empty() {
                Vec::<String>::new()
            } else {
                vec![
                    format!("{base}/v1/chat/completions"),
                    format!("{base}/v1/responses"),
                ]
            },
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
