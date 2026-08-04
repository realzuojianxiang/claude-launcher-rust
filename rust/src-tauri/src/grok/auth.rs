// Grok provider 可插拔认证策略。
//
// grok 代理对上游的认证有两条互斥通路，分别对应两种账号形态：
//   - OAuth（默认，Phase 3 接入）：用 Plus 同一个 x.ai 账号走 Device Code Flow 拿
//     access token，调官方 CLI Chat-Proxy（cli-chat-proxy.grok.com）。消费的是账号
//     权益额度，协议标准稳定。
//   - API Key 退路（Phase 2 先打通）：用官方 xai- API Key 调 api.x.ai，作号被风控/
//     额度耗尽时的退路。Key 轮询与 429 冷却复用 nvidia::key_pool。
//
// 两条通路对 proxy 层暴露同一组方法（见 `AuthProvider` trait）：取一个可注入到
// 上游请求的 Authorization 头集合、在收到 401 时尝试刷新一次、返回供 UI 展示的
// 脱敏快照。proxy 的重试循环据此决定「401→刷一次再重试」「429→cooldown 切 Key」。
//
// 设计取舍：
//   - trait 方法签名尽量窄（不返回具体 token 串，而是返回拼好的 HeaderMap），避免
//     上层暴露凭证；只在 on_401/snapshot 这类语义清晰的场景提供状态可见性。
//   - API Key 模式与 OAuth 模式上游路径不同（api.x.ai vs cli-chat-proxy），故 trait
//     不替上层决定 base_url；base 用哪个由 GrokConfig::effective_base_url 统一裁决，
//     auth provider 只负责「鉴权头 + 刷新 + 快照」。
//   - OAuth 实现 Phase 3 才真正接入，此处先放 stub 占位，保证 proxy 依赖的 trait
//     形状稳定，Phase 3 不用改 proxy。

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};

/// 一个可上游注入的鉴权头集合（通常含一对 `Authorization: Bearer ...`，
/// OAuth 模式额外带 CLI Chat-Proxy 专用头）。
///
/// 返回的 HeaderMap 由调用方克隆后注入到 reqwest 请求构建器；
/// OAuth 模式还会带 `X-XAI-Token-Auth` / `x-grok-client-version`，见 OAuthAuthProvider。
pub type AuthHeaders = HeaderMap;

/// 认证刷新结果：401 后调用 `on_401` 试图刷新一次。
///   - `Refreshed`：已刷新，下次 `auth_headers` 会拿到新 token，上层应重试一次。
///   - `Unrecoverable`：刷新不可行（API Key 模式本就没有刷新；OAuth 模式 refresh
///     也失败/无 refresh_token），上层应终止并提示重新授权。
#[allow(dead_code)] // Phase 2 proxy 重试 match 覆盖 Refreshed；OAuth 实现 Phase 3 产出
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    Refreshed,
    Unrecoverable,
}

/// 认证策略插槽。实现者持有当前可用凭证状态，并对 proxy 层暴露三类能力。
//
// 用 trait 而非 enum：两条通路在「如何拿头」「如何刷新」上差异很大，trait 让
// proxy 重试逻辑只需面向一组固定方法；OAuth/APIKey 各自封装内部状态。
#[allow(dead_code)] // Phase 2 proxy.rs 接入
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// 返回注入到上游请求的鉴权头集合。
    /// 调用方拿到后克隆注入；多次调用应返回当前最新状态（OAuth 刷新后变化）。
    fn auth_headers(&self) -> Result<AuthHeaders, String>;

    /// 收到上游 401 时调用一次：API Key 模式直接返回 `Unrecoverable`（Key 不会过期，
    /// 401 多半是 Key 失效/越权，需用户重新配置）；OAuth 模式尝试用 refresh_token
    /// 换新 access_token，成功返回 `Refreshed`。
    ///
    /// 异步：OAuth 刷新走 oauth::refresh_tokens（网络往返）；API Key 模式虽无需异步，
    /// 但为统一 trait 形状用 async_trait。proxy 重试循环以 `.await` 调用，dyn-safe 由
    /// async_trait 的 Pin<Box<Future>> 包装保证。
    async fn on_401(&self) -> RefreshOutcome;

    /// 返回供 UI 展示的脱敏快照（mode / 是否带 refresh / masked 凭证 / 冷却情况）。
    fn snapshot(&self) -> Value;

    /// 若该策略背后是可轮询的 Key 池（API Key 退路模式），返回池引用供 proxy 做 429
    /// 冷却与轮换；OAuth 模式返回 None（不存在多 Key 轮换语义）。
    /// 默认实现返回 None，新增实现无需显式覆写默认行为。
    fn key_pool_opt(&self) -> Option<&SharedKeyPool> {
        None
    }
}

// =====================================================================
// API Key 退路策略
// =====================================================================

use crate::nvidia::key_pool::{KeyPool, SharedKeyPool};
use std::sync::Mutex;

/// 官方 API Key 退路认证策略。
///
/// 持有一个 round-robin + 429 冷却的 Key 池（复用 nvidia::key_pool 语义）。
/// 401 被判为不可恢复（Key 失效而非过期）——上层应回到 UI 提示用户更换 Key，
/// 而不是反复刷新。proxy 重试循环据此：401 直接终止并返回上游错误。
///
/// 与 OAuth 模式不同，API Key 模式不带 `X-XAI-Token-Auth`/`x-grok-client-version`
/// 等 CLI Chat-Proxy 专用头——这些头只对 cli-chat-proxy 有意义，api.x.ai 不认。
/// 因此 auth_headers 只产出 Authorization 一对头。
#[allow(dead_code)] // Phase 2 proxy.rs 接入构造 + auth_headers/on_401/snapshot
pub struct ApiKeyAuthProvider {
    inner: SharedKeyPool,
    // 当前用于「正在处理的单次请求」的 Key，on_401 不可恢复时也不轮换
    // （Key 失效需用户介入，不是冷却能解决的）。仅用于 snapshot 展示。
    mode: &'static str,
}

#[allow(dead_code)] // Phase 2 proxy.rs 接入：new/key_pool/build_headers
impl ApiKeyAuthProvider {
    /// 由配置的 api_keys 构造。空池在 GrokState::start 阶段已被拒绝，此处不再校验。
    pub fn new(api_keys: Vec<String>, cooldown_seconds: u64) -> Self {
        let pool = KeyPool::new(api_keys, cooldown_seconds);
        Self {
            inner: Mutex::new(pool),
            mode: "api-key",
        }
    }

    /// 直接访问底层 Key 池（供 proxy 重试循环做 429 冷却 / 轮换）。
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

#[allow(dead_code)] // Phase 2 proxy.rs 接入：auth_headers/on_401/snapshot 三个 trait 方法
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

// =====================================================================
// OAuth 策略（CLI Chat-Proxy + Plus 账号）
// =====================================================================

use crate::grok::oauth;
use crate::grok::oauth_store::{self, TokenStore};

/// CLI Chat-Proxy 专用身份头（仅在 OAuth 走 cli-chat-proxy 时附加，api.x.ai 不加）。
/// 取自 CLIProxyAPI xai_executor.go: `xaiTokenAuthValue = "xai-grok-cli"`。
const XAI_TOKEN_AUTH_VALUE: &str = "xai-grok-cli";
/// Grok CLI 客户端版本，chat-proxy 期望的版本号。
/// 取自 CLIProxyAPI xai_executor.go: `xaiClientVersionValue = "0.2.93"`，随上游升级同步。
const XAI_CLIENT_VERSION_VALUE: &str = "0.2.93";

/// OAuth Device Code Flow 认证策略（默认主线）。
///
/// 持有当前 TokenStore（access/refresh/account/expires_at）+ token 端点 + 共享 HTTP 客户端
/// + 实例级刷新锁。`auth_headers` 拼出 `Authorization: Bearer {access}` 加两条 CLI Chat-Proxy 身份头（`X-XAI-Token-Auth` / `x-grok-client-version`）。`on_401` 异步刷新采用 singleflight 等价语义：并发 401 全部 `lock().await` 排队，首个调用方执行 `refresh_tokens`，后续调用方拿到锁后对比 access 快照，已变则直接 Refreshed，避免重复刷。
///
/// 刷新成功后写盘（oauth_store::save），刷新失败返回 `Unrecoverable`——上层应终止并把
/// 上游错误回传，由前端引导重新走 Device Code Flow（refresh_token 也失效属正常授权过期）。
///
/// base_url 由 GrokConfig::effective_base_url 裁决（OAuth 模式 = oauth_base_url，
/// 即 cli-chat-proxy.grok.com），本策略不持有 base——只负责「鉴权头 + 刷新 + 快照」。
#[allow(dead_code)]
pub struct OAuthAuthProvider {
    /// 当前 token。std 锁（同步取头用）；刷新时短暂持锁更新。
    token: std::sync::Mutex<TokenStore>,
    /// token 端点（discovery 拿到后缓存，refresh 直接用，免去每轮再 discover）。
    token_endpoint: String,
    /// OAuth 刷新用的 HTTP 客户端（与 proxy 上游转发客户端可不同实例，互不干扰；
    /// 此处复用 oauth::http_client 风格的 redirect::none 客户端）。
    client: reqwest::Client,
    /// 刷新串行锁：singleflight 等价。并发 401 只让一个去刷。
    refresh_lock: tokio::sync::Mutex<()>,
}

#[allow(dead_code)]
impl OAuthAuthProvider {
    /// 由已授权的 TokenStore + token 端点构造。client 由调用方传入（一般用
    /// oauth::http_client()）；token_endpoint 可空，会在首次 refresh 时补 discover。
    pub fn new(token: TokenStore, token_endpoint: String, client: reqwest::Client) -> Self {
        Self {
            token: std::sync::Mutex::new(token),
            token_endpoint,
            client,
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// 当前 token 的 account（供 mod.rs start 写回 GrokConfig::oauth_account 用）。
    pub fn account(&self) -> String {
        self.token.lock().unwrap().account.clone()
    }

    fn build_headers(access: &str) -> Result<AuthHeaders, String> {
        let mut h = HeaderMap::new();
        let bearer = reqwest::header::HeaderValue::from_str(format!("Bearer {access}").as_str())
            .map_err(|e| format!("❌ access token 含非法 HTTP 头字符：{e}"))?;
        h.insert(reqwest::header::AUTHORIZATION, bearer);
        // CLI Chat-Proxy 身份头（api.x.ai 不需要这两个，故 ApiKey 路径不加）。
        let ta = reqwest::header::HeaderValue::from_static(XAI_TOKEN_AUTH_VALUE);
        h.insert("X-XAI-Token-Auth", ta);
        let cv = reqwest::header::HeaderValue::from_static(XAI_CLIENT_VERSION_VALUE);
        h.insert("x-grok-client-version", cv);
        Ok(h)
    }
}

#[allow(dead_code)]
#[async_trait]
impl AuthProvider for OAuthAuthProvider {
    fn auth_headers(&self) -> Result<AuthHeaders, String> {
        let token = self.token.lock().unwrap();
        if token.access_token.trim().is_empty() {
            return Err("❌ OAuth 未授权（无 access_token），请先完成 Grok 账号授权".to_string());
        }
        Self::build_headers(&token.access_token)
    }

    async fn on_401(&self) -> RefreshOutcome {
        // singleflight 等价：抢刷新锁。这里用阻塞式 `lock().await` 而非 try_lock——
        // 让并发 401 全部排队等首个刷新完成，再各自比对 access 快照：变了说明已被首刷
        // 刷新过，直接 Refreshed；没变（首刷失败也会推进到 return Unrecoverable 分支）
        // 说明首刷已自救过且失败，本调用方也按不可恢复处理。
        let _guard = self.refresh_lock.lock().await;

        // 加锁前的 access 快照——加锁后对比用。若已被别的并发 401 刷过，这里就会不同。
        let pre_access = self.token.lock().unwrap().access_token.clone();
        if pre_access.trim().is_empty() {
            // 没有 access 本就不该走到这（auth_headers 已拦），保守判不可恢复。
            return RefreshOutcome::Unrecoverable;
        }

        // 拿到锁后再看一次：可能与 pre 不同（前一个持锁者刚刷完）。
        let cur_access = self.token.lock().unwrap().access_token.clone();
        if cur_access != pre_access {
            // 并发刷新已成功，本调用方刷到新 token，重试一次即可。
            return RefreshOutcome::Refreshed;
        }

        let refresh_token = self.token.lock().unwrap().refresh_token.clone();
        if refresh_token.trim().is_empty() {
            tracing::warn!("OAuth 401 但无 refresh_token，需重新授权");
            return RefreshOutcome::Unrecoverable;
        }

        match oauth::refresh_tokens(&self.client, &refresh_token, &self.token_endpoint).await {
            Ok(new_store) => {
                // 更新共享 token + 落盘。
                {
                    let mut t = self.token.lock().unwrap();
                    *t = new_store.clone();
                }
                if let Err(e) = oauth_store::save(&new_store) {
                    tracing::warn!(error = %e, "OAuth 刷新成功但写盘失败（内存已更新，下次重启将回退旧 token）");
                }
                tracing::info!("OAuth 401 已刷新 access_token 并落盘");
                RefreshOutcome::Refreshed
            }
            Err(e) => {
                tracing::warn!(error = %e, "OAuth 401 刷新失败，需重新授权");
                RefreshOutcome::Unrecoverable
            }
        }
    }

    fn snapshot(&self) -> Value {
        let t = self.token.lock().unwrap();
        // 不暴露 token 本体；account 是 email（非凭证）可显式展示，与 GrokConfig::oauth_account 对齐。
        let now = epoch_secs();
        json!({
            "mode": "oauth",
            "refreshable": !t.refresh_token.trim().is_empty(),
            "account": t.account,
            "expires_at": t.expires_at,
            "expired": t.expires_at != 0 && now >= t.expires_at,
        })
    }

    fn key_pool_opt(&self) -> Option<&SharedKeyPool> {
        // OAuth 语义上无 Key 池轮换，返回 None。
        None
    }
}

/// 当前 epoch 秒（OAuthAuthProvider::snapshot 判 expired 用）。
fn epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// 给单测用的小工具：把 HeaderMap 里的 Authorization 取回来做断言（脱敏前）。
#[cfg(test)]
fn auth_bearer(headers: &AuthHeaders) -> Option<String> {
    headers
        .get(reqwest::header::AUTHORIZATION)?
        .to_str()
        .ok()
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_auth_emits_bearer_header() {
        let auth = ApiKeyAuthProvider::new(vec!["xai-AAAAAAAAAAAAAAAAAAAA".to_string()], 60);
        let h = auth.auth_headers().expect("单 Key 应可取头");
        assert_eq!(
            auth_bearer(&h).as_deref(),
            Some("Bearer xai-AAAAAAAAAAAAAAAAAAAA")
        );
    }

    #[test]
    fn api_key_auth_round_robins_across_keys() {
        let auth = ApiKeyAuthProvider::new(vec!["xai-K1".to_string(), "xai-K2".to_string()], 60);
        let a = auth_bearer(&auth.auth_headers().unwrap()).unwrap();
        let b = auth_bearer(&auth.auth_headers().unwrap()).unwrap();
        assert_ne!(a, b, "两个 Key 应被轮询到不同值");
    }

    #[test]
    fn api_key_auth_401_is_unrecoverable() {
        let auth = ApiKeyAuthProvider::new(vec!["xai-K".to_string()], 60);
        // on_401 在 async_trait 下返回 Pin<Box<Future>>；测试用 block_on 取值。
        let outcome = tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(auth.on_401());
        assert_eq!(outcome, RefreshOutcome::Unrecoverable);
    }

    #[test]
    fn api_key_snapshot_masks_keys_and_reports_mode() {
        let auth = ApiKeyAuthProvider::new(vec!["xai-0123456789abcdef".to_string()], 60);
        let snap = auth.snapshot();
        assert_eq!(snap["mode"], "api-key");
        assert_eq!(snap["refreshable"], false);
        assert_eq!(snap["total"], 1);
        assert_eq!(snap["available"], 1);
        // snapshot 内的 key 已被 mask_key 脱敏（前4后4中间省略）
        let masked = snap["keys"][0]["masked"].as_str().unwrap();
        assert!(masked.contains('…'));
        assert!(!masked.contains("0123456789abcdef"));
    }

    #[test]
    fn api_key_auth_all_cooling_returns_error() {
        let auth = ApiKeyAuthProvider::new(vec!["xai-K1".to_string()], 60);
        // 把唯一 Key 置冷却
        auth.key_pool().lock().unwrap().cooldown("xai-K1");
        let r = auth.auth_headers();
        assert!(r.is_err(), "全部冷却中取头应失败而非返回过期凭证");
    }

    #[test]
    fn api_key_headers_reject_illegal_chars() {
        // 构造含非法 HTTP 头字符的 Key：from_str 在 build_headers 里直接报错
        let r = ApiKeyAuthProvider::build_headers("含 空格 的 非法 值");
        assert!(r.is_err());
    }
}
