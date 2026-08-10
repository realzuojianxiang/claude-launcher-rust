// 协议网关（8083）通用可插拔认证策略。
//
// 网关作为「Anthropic 入站 ↔ OpenAI 协议上游」转换层，对上游的认证只保留最通用的
// API Key 轮询 + 429 冷却策略（复用 nvidia::key_pool 语义）。OAuth（原 grok 专有）
// 已从网关移除——网关只服务于 OpenAI 兼容端点，均走 API Key。
//
// 设计取舍：trait 方法签名尽量窄（不返回具体 token 串，而是返回拼好的 HeaderMap），
// 避免上层暴露凭证；只在 on_401/snapshot 这类语义清晰的场景提供状态可见性。

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};

use crate::nvidia::key_pool::{KeyPool, SharedKeyPool};
use std::sync::Mutex;

/// 一个可上游注入的鉴权头集合（通常含一对 `Authorization: Bearer ...`）。
pub type AuthHeaders = HeaderMap;

/// 认证刷新结果：401 后调用 `on_401` 试图刷新一次。
///   - `Refreshed`：已刷新，下次 `auth_headers` 会拿到新 token，上层应重试一次。
///   - `Unrecoverable`：刷新不可行（API Key 模式本就没有刷新）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    #[allow(dead_code)]
    // 刷新语义由 AuthProvider 契约预留；网关 API Key 通路只产出 Unrecoverable，proxy 仍有匹配分支
    Refreshed,
    Unrecoverable,
}

/// 认证策略插槽。实现者持有当前可用凭证状态，并对 proxy 层暴露三类能力。
///
/// 用 trait 而非 enum：两条通路在「如何拿头」「如何刷新」上差异很大，trait 让
/// proxy 重试逻辑只需面向一组固定方法。
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// 返回注入到上游请求的鉴权头集合。
    /// 调用方拿到后克隆注入；多次调用应返回当前最新状态（API Key 模式即当前选中的 Key）。
    fn auth_headers(&self) -> Result<AuthHeaders, String>;

    /// 收到上游 401 时调用一次：API Key 模式直接返回 `Unrecoverable`（Key 不会过期，
    /// 401 多半是 Key 失效/越权，需用户重新配置）。
    async fn on_401(&self) -> RefreshOutcome;

    /// 返回供 UI 展示的脱敏快照（mode / 是否带 refresh / masked 凭证 / 冷却情况）。
    fn snapshot(&self) -> Value;

    /// 若该策略背后是可轮询的 Key 池（API Key 模式），返回池引用供 proxy 做 429
    /// 冷却与轮换；否则返回 None。
    /// 默认实现返回 None，新增实现无需显式覆写默认行为。
    fn key_pool_opt(&self) -> Option<&SharedKeyPool> {
        None
    }
}

/// 官方 API Key 认证策略。
///
/// 持有一个 round-robin + 429 冷却的 Key 池（复用 nvidia::key_pool 语义）。
/// 401 被判为不可恢复（Key 失效而非过期）——上层应回到 UI 提示用户更换 Key，
/// 而不是反复刷新。proxy 重试循环据此：401 直接终止并返回上游错误。
pub struct ApiKeyAuthProvider {
    inner: SharedKeyPool,
    mode: &'static str,
}

impl ApiKeyAuthProvider {
    /// 由配置的 api_keys 构造。空池在 GatewayState::start 阶段已被拒绝，此处不再校验。
    pub fn new(api_keys: Vec<String>, cooldown_seconds: u64) -> Self {
        let pool = KeyPool::new(api_keys, cooldown_seconds);
        Self {
            inner: Mutex::new(pool),
            mode: "api-key",
        }
    }

    /// 直接访问底层 Key 池（供 proxy 重试循环做 429 冷却 / 轮换）。
    #[allow(dead_code)] // 仅 gateway/auth tests 内调用；cargo check（不含 test）下报未用
    pub fn key_pool(&self) -> &SharedKeyPool {
        &self.inner
    }

    fn build_headers(token: &str) -> Result<AuthHeaders, String> {
        let mut h = HeaderMap::new();
        let v = reqwest::header::HeaderValue::from_str(format!("Bearer {token}").as_str())
            .map_err(|e| format!("❌ API Key 含非法 HTTP 头字符：{e}"))?;
        h.insert(reqwest::header::AUTHORIZATION, v);
        Ok(h)
    }
}

#[async_trait]
impl AuthProvider for ApiKeyAuthProvider {
    fn auth_headers(&self) -> Result<AuthHeaders, String> {
        // pick 返回 None：池空（start 已拦截）或全部冷却中。
        // 冷却中应当由 proxy 重试循环决策（等冷却或切换），这里返回 Err 让上层报错。
        let key = {
            let mut pool = self.inner.lock().unwrap();
            pool.pick()
        }
        .ok_or_else(|| "❌ 所有 API Key 均在冷却中，无可用凭证".to_string())?;
        Self::build_headers(&key)
    }

    async fn on_401(&self) -> RefreshOutcome {
        // API Key 不会"过期"——401 多半是 Key 无效/越权/被吊销，刷新无意义。
        // 上层应终止并把错误回传，让用户在 UI 换 Key。
        RefreshOutcome::Unrecoverable
    }

    fn snapshot(&self) -> Value {
        let pool = self.inner.lock().unwrap();
        json!({
            "mode": self.mode,
            "refreshable": false,
            "total": pool.total(),
            "available": pool.available_count(),
            "cooling": pool.total() - pool.available_count(),
            "keys": pool.snapshot(),
        })
        // 注：mask_key 已在 pool.snapshot 内对每个 key 脱敏，不再二次处理。
    }

    fn key_pool_opt(&self) -> Option<&SharedKeyPool> {
        Some(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_auth_emits_bearer_header() {
        let auth = ApiKeyAuthProvider::new(vec!["sk-AAAAAAAAAAAAAAAAAAAA".to_string()], 60);
        let h = auth.auth_headers().expect("单 Key 应可取头");
        assert_eq!(
            h.get(reqwest::header::AUTHORIZATION).unwrap().to_str().ok(),
            Some("Bearer sk-AAAAAAAAAAAAAAAAAAAA")
        );
    }

    #[test]
    fn api_key_auth_round_robins_across_keys() {
        let auth = ApiKeyAuthProvider::new(vec!["sk-K1".to_string(), "sk-K2".to_string()], 60);
        let a = auth.auth_headers().unwrap();
        let b = auth.auth_headers().unwrap();
        assert_ne!(a, b, "两个 Key 应被轮询到不同值");
    }

    #[test]
    fn api_key_auth_401_is_unrecoverable() {
        let auth = ApiKeyAuthProvider::new(vec!["sk-K".to_string()], 60);
        let outcome = tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(auth.on_401());
        assert_eq!(outcome, RefreshOutcome::Unrecoverable);
    }

    #[test]
    fn api_key_snapshot_masks_keys_and_reports_mode() {
        let auth = ApiKeyAuthProvider::new(vec!["sk-0123456789abcdef".to_string()], 60);
        let snap = auth.snapshot();
        assert_eq!(snap["mode"], "api-key");
        assert_eq!(snap["refreshable"], false);
        assert_eq!(snap["total"], 1);
        assert_eq!(snap["available"], 1);
        let masked = snap["keys"][0]["masked"].as_str().unwrap();
        assert!(masked.contains('…'));
        assert!(!masked.contains("0123456789abcdef"));
    }

    #[test]
    fn api_key_auth_all_cooling_returns_error() {
        let auth = ApiKeyAuthProvider::new(vec!["sk-K1".to_string()], 60);
        auth.key_pool().lock().unwrap().cooldown("sk-K1");
        let r = auth.auth_headers();
        assert!(r.is_err(), "全部冷却中取头应失败而非返回过期凭证");
    }

    #[test]
    fn api_key_headers_reject_illegal_chars() {
        let r = ApiKeyAuthProvider::build_headers("含 空格 的 非法 值");
        assert!(r.is_err());
    }
}
