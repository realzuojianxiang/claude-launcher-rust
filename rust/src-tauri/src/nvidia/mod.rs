// NVIDIA API 代理服务模块
//
// 子模块职责：
//   - models    : Anthropic / OpenAI 数据结构
//   - converter : 协议转换（请求映射、响应/SSE 转换）
//   - proxy     : 代理核心（转发、[后续] 重试/Fallback）
//   - server    : axum 路由装配
//   - key_pool  : [Step 2] Key 池轮询与冷却
// 本文件（mod.rs）负责应用内 axum 服务的生命周期管理：启动/停止/状态查询，
// 以 Tauri 托管状态 NvidiaState 持有运行句柄。

pub mod converter;
pub mod key_pool;
pub mod models;
pub mod proxy;
pub mod server;

use crate::config::NvidiaConfig;
use crate::stats::UsageStatsStore;
use proxy::ProxyCtx;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

// 正在运行的代理实例句柄：优雅关闭信号 + 实际监听地址 + 共享上下文（含 Key 池）
struct Running {
    shutdown: tokio::sync::oneshot::Sender<()>,
    addr: String,
    ctx: Arc<ProxyCtx>,
    _stats: Arc<UsageStatsStore>,
}

// Tauri 托管状态：包裹「可选的正在运行实例」
#[derive(Default)]
pub struct NvidiaState {
    inner: Mutex<Option<Running>>,
}

impl NvidiaState {
    pub fn new() -> Self {
        Self::default()
    }

    // 是否正在运行
    pub fn is_running(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }

    // 当前监听地址（未运行则为空串）
    pub fn addr(&self) -> String {
        self.inner
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.addr.clone())
            .unwrap_or_default()
    }

    // 启动代理：绑定监听端口并在独立运行时上启动 axum 服务
    pub fn start(&self, cfg: NvidiaConfig, stats: Arc<UsageStatsStore>) -> Result<String, String> {
        crate::diag_step("start(): entry");
        tracing::info!("NVIDIA 代理启动流程开始");
        {
            // 已在运行则直接返回
            let guard = self.inner.lock().unwrap();
            if guard.is_some() {
                crate::diag_step("start(): already running, return");
                return Ok("⚠️ NVIDIA 代理已经在运行中".to_string());
            }
        }
        crate::diag_step("start(): not-running check passed");

        // 基础校验，尽早给出清晰错误
        if cfg.api_keys.is_empty() {
            return Err("❌ 请先配置至少一个 NVIDIA API Key".to_string());
        }
        if cfg.models.is_empty() {
            return Err("❌ 请先配置至少一个模型".to_string());
        }
        // S1：host 非回环时强制要求高熵 auth_token，拒绝无鉴权对外监听
        cfg.require_auth_if_exposed()?;
        // P2/SSRF 闸：上游 base_url 必须 scheme + host 非空，配合 ProxyCtx 的 redirect(none)
        // 杜绝请求体 + bearer token 被上游 30x 引流到攻击者主机。
        cfg.validate_base_url()?;
        crate::diag_step("start(): config validated");

        let bind_addr = format!("{}:{}", cfg.host, cfg.port);

        // 用标准库同步绑定端口：瞬时完成、立即返回错误（端口占用等），
        // 避免在 Tauri 命令线程上 block_on 一个 async 绑定（会死锁/卡死整个命令）。
        let std_listener = std::net::TcpListener::bind(&bind_addr)
            .map_err(|e| format!("❌ 绑定 {bind_addr} 失败: {e}"))?;
        crate::diag_step("start(): tcp bind ok");
        let _ = std_listener.set_nonblocking(true);
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create NVIDIA proxy runtime: {e}"))?;
        let listener = {
            let _runtime_guard = rt.enter();
            tokio::net::TcpListener::from_std(std_listener)
                .map_err(|e| format!("failed to convert NVIDIA proxy listener: {e}"))?
        };
        crate::diag_step("start(): listener from_std ok");

        let local_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| bind_addr.clone());

        // 构造服务；oneshot 作为优雅关闭信号
        let ctx = ProxyCtx::new(cfg);
        crate::diag_step("start(): ProxyCtx::new ok");
        let app = server::build_router(ctx.clone());
        crate::diag_step("start(): build_router ok");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        // 关键：在「独立的 OS 线程 + 独立的 tokio 运行时」上运行 axum 服务，
        // 与 Tauri 的命令调度运行时彻底隔离。这样：
        //   1) 不需要在命令线程里 block_on（消除死锁/卡死）；
        //   2) 长期运行的代理服务不会与命令处理抢同一组运行时线程，避免 UI 卡顿。
        std::thread::spawn(move || {
            rt.block_on(async move {
                let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                    let _ = rx.await;
                });
                if let Err(e) = server.await {
                    tracing::error!(error = %e, "NVIDIA 代理服务异常退出");
                }
            });
            tracing::info!("NVIDIA 代理后台线程已退出");
        });
        crate::diag_step("start(): server thread spawned");

        *self.inner.lock().unwrap() = Some(Running {
            shutdown: tx,
            addr: local_addr.clone(),
            ctx: ctx.clone(),
            _stats: stats,
        });
        crate::diag_step("start(): inner set");

        tracing::info!(addr = %local_addr, "NVIDIA 代理已启动");
        crate::diag_step("start(): returned Ok");
        Ok(format!("✅ NVIDIA 代理已启动，监听 http://{local_addr}"))
    }

    // 热更新运行中代理的模型优先级列表（无需重启）。
    // 返回 true 表示代理正在运行且已实时应用；false 表示未运行（仅由调用方负责持久化）。
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

    // 停止代理：发送关闭信号触发优雅退出
    pub fn stop(&self) -> Result<String, String> {
        let running = self.inner.lock().unwrap().take();
        match running {
            Some(r) => {
                let _ = r.shutdown.send(());
                tracing::info!("NVIDIA 代理已停止");
                Ok("✅ NVIDIA 代理已停止".to_string())
            }
            None => Ok("⚠️ NVIDIA 代理未在运行".to_string()),
        }
    }

    // 状态查询：返回给前端展示
    pub fn status(&self) -> serde_json::Value {
        let running = self.is_running();
        let addr = self.addr();
        json!({
            "running": running,
            "url": if addr.is_empty() { String::new() } else { format!("http://{addr}") },
            "endpoint": if addr.is_empty() { String::new() } else { format!("http://{addr}/v1/messages") }
        })
    }

    // Key 池状态（Step 2 状态面板数据源）：仅在代理运行时可用，否则返回未运行。
    pub fn key_pool_status(&self) -> Value {
        let guard = self.inner.lock().unwrap();
        match guard.as_ref() {
            Some(r) => {
                let pool = r.ctx.key_pool.lock().unwrap();
                json!({
                    "running": true,
                    "total": pool.total(),
                    "available": pool.available_count(),
                    "cooling": pool.total() - pool.available_count(),
                    "keys": pool.snapshot(),
                })
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
    use super::NvidiaState;
    use crate::config::NvidiaConfig;
    use crate::stats::UsageStatsStore;
    use std::sync::Arc;

    #[test]
    fn start_works_from_plain_os_thread_without_a_tauri_runtime() {
        let result = std::thread::spawn(|| {
            let state = NvidiaState::new();
            let cfg = NvidiaConfig {
                api_keys: vec!["diagnostic-placeholder-key".to_string()],
                models: vec!["diagnostic-placeholder-model".to_string()],
                host: "127.0.0.1".to_string(),
                port: 0,
                ..Default::default()
            };

            let started = state.start(cfg, Arc::new(UsageStatsStore::in_memory()));
            if started.is_ok() {
                let _ = state.stop();
            }
            started
        })
        .join()
        .expect("NVIDIA proxy startup thread must not panic");

        assert!(
            result.is_ok(),
            "NVIDIA proxy must start from a plain OS thread without a Tauri runtime: {result:?}"
        );
    }
}
