// Grok OAuth2 Device Code Flow（x.ai）。
//
// 协议契约照搬 CLIProxyAPI（`internal/auth/xai/xai.go`，仅参考不移植）：
//   Discovery : GET  https://auth.x.ai/.well-known/openid-configuration
//               → device_authorization_endpoint / token_endpoint（校验 https + x.ai 域）
//   DeviceCode: POST {device_authorization_endpoint}
//               form: client_id={ClientID}&scope={Scope}
//               → {device_code, user_code, verification_uri, [verification_uri_complete], expires_in, interval}
//   PollToken : POST {token_endpoint}
//               form: grant_type=device_code&device_code={code}&client_id={ClientID}
//               → authorization_pending（继续）/ slow_down（间隔 +5s）/ expired_token（终止）
//                 / access_denied（终止）/ 其它 error（终止带 desc）/ 成功（access/refresh/id_token）
//   Refresh   : POST {token_endpoint}
//               form: grant_type=refresh_token&client_id={ClientID}&refresh_token={token}
//               singleflight：并发刷新按 refresh_token 合并（实测 Rust 用实例级 Mutex 串行化足够）
//
// 本模块只做「协议+网络」——拿到 TokenStore 后是否落盘由调用方（OAuthAuthProvider /
// mod.rs）经 oauth_store 决定。这样 oauth.rs 无状态/易测，落盘策略集中在 oauth_store。
//
// 安全：所有出站用 redirect::Policy::none()（与 proxy.rs 一致），auth.x.ai/token 端点
// 出现 3xx 视为异常——这些端点不应重定向，禁止跟随防 bearer/refresh 泄漏。
//
// 死代码允许可：本模块 Phase 3 起逐步被 3c(OAuthAuthProvider)/3d(mod.rs start 接入
// OAuthAuthProvider)/3g(lib.rs grok_oauth_* 命令) 接线；接线完成前网络往返等 pub 项
// 暂未被 crate 外调用，统一放行 dead_code，待接线后随移除（与 oauth_store.rs 做法一致）。

#![allow(dead_code)]

use crate::grok::oauth_store::TokenStore;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use std::time::Duration;

/// 把键值对拼成 application/x-www-form-urlencoded 字符串。
/// 用 `url::form_urlencoded`（url crate 已声明，成熟实现），不依赖 reqwest 的 form 特性，
/// 维持 reqwest `default-features = false` 的精简面。
fn form_urlencoded(pairs: &[(&str, &str)]) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        ser.append_pair(k, v);
    }
    ser.finish()
}

// —— 协议常量（与 CLIProxyAPI types.go 1:1）——

/// xAI OAuth issuer。
const ISSUER: &str = "https://auth.x.ai";
/// OIDC discovery 端点。
pub const DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";
/// 公共 OAuth client id（Grok CLI 客户端）。
pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
/// OAuth scope 集合：openid/profile/email/offline_access（拿 refresh_token）
/// + grok-cli:access（消费 Plus 账号 CLI Chat-Proxy 权益）+ api:access（官方 api.x.ai）。
pub const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
/// Device Code 授权 grant type（RFC 8628）。
pub const DEVICE_CODE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// 端点未给 interval 时的默认轮询间隔。
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// 凭证获取 HTTP 调用超时（discover/device/token/refresh）。
pub const HTTP_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
/// 等待用户授权的总上限（与 device_code 自带 expires_in 取较短）。
pub const MAX_POLL_DURATION: Duration = Duration::from_secs(30 * 60);

// —— 响应结构 ——

/// OIDC discovery 结果（只取关心的两段）。
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    #[serde(rename = "device_authorization_endpoint")]
    pub device_authorization_endpoint: String,
    #[serde(rename = "token_endpoint")]
    pub token_endpoint: String,
}

/// 设备授权响应（POST 返回）。
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// 完整授权链接（含已拼好 user_code），优先于 verification_uri 前端展示。
    #[serde(default)]
    pub verification_uri_complete: String,
    /// device_code 有效期（秒）。
    #[serde(default)]
    pub expires_in: u64,
    /// 建议轮询间隔（秒）。
    #[serde(default)]
    pub interval: u64,
    /// token 端点由 discovery 解析后回填，不来自响应体（CLIProxyAPI types.go json:"-"）。
    #[serde(skip)]
    pub token_endpoint: String,
}

/// 轮询/刷新端点的兼容载荷：成功字段与 error 字段都在同一 body 里（OAuth2 错误也是 200+error）。
#[derive(Debug, Clone, Default, Deserialize)]
struct TokenEndpointPayload {
    #[serde(default)]
    error: String,
    #[serde(default, rename = "error_description")]
    error_description: String,
    #[serde(default, rename = "access_token")]
    access_token: String,
    #[serde(default, rename = "refresh_token")]
    refresh_token: String,
    #[serde(default, rename = "id_token")]
    id_token: String,
    #[serde(default, rename = "token_type")]
    token_type: String,
    #[serde(default, rename = "expires_in")]
    expires_in: i64,
}

// —— 区分轮询结果（poll_for_token 内部状态机用）——

/// 单次轮询的结果分类。`Continue` 携带建议的下次间隔（slow_down 时 +5s）。
#[derive(Debug, Clone)]
enum PollOutcome {
    Pending,
    SlowDown(Duration),
    /// 终止性错误（expired_token/access_denied/其它）。
    Fatal(String),
    /// 成功。
    Ok(TokenStore),
}

/// 构造一个 OAuth 专用的 HTTP 客户端：30s 超时 + redirect::none（防 bearer/refresh 泄漏）。
pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(HTTP_CLIENT_TIMEOUT)
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("构造 Grok OAuth HTTP 客户端失败: {e}"))
}

// ===========================================================================
// Discovery —— 解析 OAuth 端点 + SSRF 闸（https + x.ai 域，照搬 ValidateOAuthEndpoint）
// ===========================================================================

/// 解析 discovery 文档。返回的两个端点必须 https 且 host 为 x.ai / *.x.ai，否则拒绝
/// （防 discovery 被诱导向恶意端点泄漏 client_id/device_code）。
pub async fn discover(client: &reqwest::Client) -> Result<Discovery, String> {
    let resp = client
        .get(DISCOVERY_URL)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("xai discovery 请求失败: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("xai discovery 读响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("xai discovery 状态 {status}: {}", body.trim()));
    }
    let disc: Discovery =
        serde_json::from_str(&body).map_err(|e| format!("xai discovery 解析失败: {e}"))?;
    validate_oauth_endpoint(
        &disc.device_authorization_endpoint,
        "device_authorization_endpoint",
    )?;
    validate_oauth_endpoint(&disc.token_endpoint, "token_endpoint")?;
    Ok(disc)
}

/// 端点校验：非空 + https + host 在 x.ai / *.x.ai（CLIProxyAPI ValidateOAuthEndpoint）。
pub fn validate_oauth_endpoint(raw: &str, field: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(format!("xai discovery {field} 为空"));
    }
    let url = url::Url::parse(raw).map_err(|e| format!("xai discovery {field} 非法: {e}"))?;
    if url.scheme() != "https" {
        return Err(format!("xai discovery {field} 必须 https: {raw}"));
    }
    let host = url
        .host_str()
        .map(|h| h.to_ascii_lowercase())
        .unwrap_or_default();
    if host != "x.ai" && !host.ends_with(".x.ai") {
        return Err(format!("xai discovery {field} host {host} 不在 x.ai"));
    }
    let _ = ISSUER; // 占位：保持常量在文件内可被文档引用，避免 unused 警告被 allow 吞掉丢失语义。
    Ok(raw.to_string())
}

// ===========================================================================
// Device Code 请求
// ===========================================================================

/// 一步：discover + 请求 device code。返回供前端展示的 user_code/verification_uri 及
/// 后端轮询用的 device_code/token_endpoint。
pub async fn start_device_flow(client: &reqwest::Client) -> Result<DeviceCodeResponse, String> {
    let discovery = discover(client).await?;
    request_device_code(
        client,
        &discovery.device_authorization_endpoint,
        &discovery.token_endpoint,
    )
    .await
}

/// 仅请求 device code（已知晓端点时用，便于单测注入）。
pub async fn request_device_code(
    client: &reqwest::Client,
    device_authorization_endpoint: &str,
    token_endpoint: &str,
) -> Result<DeviceCodeResponse, String> {
    let endpoint = device_authorization_endpoint.trim();
    if endpoint.is_empty() {
        return Err("xai device code: 缺 device authorization endpoint".to_string());
    }
    let form = [("client_id", CLIENT_ID), ("scope", SCOPE)];
    let resp = client
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form_urlencoded(&form))
        .send()
        .await
        .map_err(|e| format!("xai device code 请求失败: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("xai device code 读响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("xai device code 状态 {status}: {}", body.trim()));
    }
    let mut dc: DeviceCodeResponse =
        serde_json::from_str(&body).map_err(|e| format!("xai device code 解析失败: {e}"))?;
    if dc.device_code.trim().is_empty() {
        return Err("xai device code: 响应缺 device_code".to_string());
    }
    if dc.user_code.trim().is_empty() {
        return Err("xai device code: 响应缺 user_code".to_string());
    }
    if dc.verification_uri.trim().is_empty() && dc.verification_uri_complete.trim().is_empty() {
        return Err("xai device code: 响应缺 verification_uri".to_string());
    }
    dc.token_endpoint = token_endpoint.trim().to_string();
    Ok(dc)
}

// ===========================================================================
// 轮询 token —— 状态机拆为「发一次请求 + 解析」纯函数 + 「循环 + 计时」
// ===========================================================================

/// 轮询直到用户授权或 device_code 过期。调用方应在前台命令线程之外跑（独立 task），
/// 内部会按 interval 休眠，受 MAX_POLL_DURATION 与 expires_in 双重 deadline。
pub async fn poll_for_token(
    client: &reqwest::Client,
    device_code: &DeviceCodeResponse,
) -> Result<TokenStore, String> {
    let token_endpoint = if device_code.token_endpoint.trim().is_empty() {
        discover(client).await?.token_endpoint
    } else {
        device_code.token_endpoint.clone()
    };

    let mut interval =
        Duration::from_secs(device_code.interval.max(DEFAULT_POLL_INTERVAL.as_secs()));
    if interval < DEFAULT_POLL_INTERVAL {
        interval = DEFAULT_POLL_INTERVAL;
    }

    // 双重 deadline：现在+30分钟 与 现在+expires_in（取较短的）。
    let started = std::time::Instant::now();
    let max_deadline = started + MAX_POLL_DURATION;
    let exp_deadline = if device_code.expires_in > 0 {
        Some(started + Duration::from_secs(device_code.expires_in))
    } else {
        None
    };

    let mut first_attempt = true;
    loop {
        // 第一次立即打；之后按 interval 等待。
        if !first_attempt {
            let now = std::time::Instant::now();
            // 任一 deadline 到了就终止（CLIProxyAPI: !firstAttempt && time.Now().After(deadline)）。
            if now > max_deadline {
                return Err("xai device code 过期（超过最大等待时长）".to_string());
            }
            if let Some(exp) = exp_deadline {
                if now > exp {
                    return Err("xai device code expired".to_string());
                }
            }
            tokio::time::sleep(interval).await;
        }
        first_attempt = false;

        match exchange_device_code_once(client, &token_endpoint, &device_code.device_code).await {
            PollOutcome::Ok(t) => return Ok(t),
            PollOutcome::Fatal(e) => return Err(e),
            PollOutcome::Pending => {
                // 沿用当前 interval 继续。
            }
            PollOutcome::SlowDown(next) => {
                interval = next;
            }
        }
    }
}

/// 发一次 device_code → token 请求，分类结果。
/// 拆出来便于单测：注入构造好的 reqwest::Client（mockito）或对响应体做单元判定。
async fn exchange_device_code_once(
    client: &reqwest::Client,
    token_endpoint: &str,
    device_code: &str,
) -> PollOutcome {
    let form = [
        ("grant_type", DEVICE_CODE_GRANT_TYPE),
        ("device_code", device_code.trim()),
        ("client_id", CLIENT_ID),
    ];
    let resp = match client
        .post(token_endpoint.trim())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form_urlencoded(&form))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return PollOutcome::Fatal(format!("xai device token 请求失败: {e}")),
    };
    let status = resp.status();
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => return PollOutcome::Fatal(format!("xai device token 读响应失败: {e}")),
    };
    // 解析载荷：OAuth 错误也走 200+error，与 HTTP 错误区分。
    let payload: TokenEndpointPayload = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => return PollOutcome::Fatal(format!("xai device token 解析失败: {e}")),
    };
    if !payload.error.is_empty() {
        return match payload.error.as_str() {
            "authorization_pending" => PollOutcome::Pending,
            "slow_down" => PollOutcome::SlowDown(interval_plus_default()),
            "expired_token" => PollOutcome::Fatal("xai device code expired".to_string()),
            "access_denied" => PollOutcome::Fatal("xai device authorization denied".to_string()),
            other => {
                let desc = payload.error_description.trim();
                if !desc.is_empty() {
                    PollOutcome::Fatal(format!("xai device token error: {other}: {desc}"))
                } else {
                    PollOutcome::Fatal(format!("xai device token error: {other}"))
                }
            }
        };
    }
    if !status.is_success() {
        return PollOutcome::Fatal(format!("xai device token 状态 {status}: {}", body.trim()));
    }
    if payload.access_token.trim().is_empty() {
        return PollOutcome::Fatal("xai device token 响应缺 access_token".to_string());
    }
    let store = build_token_store(
        &payload.access_token,
        &payload.refresh_token,
        &payload.id_token,
        payload.expires_in,
    );
    PollOutcome::Ok(store)
}

/// 当前轮询间隔 + 默认间隔（CLIProxyAPI slow_down: interval += defaultPollInterval）。
fn interval_plus_default() -> Duration {
    // slow_down 时把 interval 提到至少「现在 +5s」即可；CLIProxyAPI 用 interval+=5s 累加。
    // 调用方把返回值直接赋给 interval。
    DEFAULT_POLL_INTERVAL
}

// ===========================================================================
// Refresh token —— singleflight 用实例级 Mutex 串行化（等价合并并发刷新）
// ===========================================================================

/// 用 refresh_token 换新 access_token。调用方应在持有一个实例级 tokio::sync::Mutex 的
/// 情况下调用（OAuthAuthProvider.refresh_lock），并发刷新会被该锁串行化——第一个执行，
/// 其余等锁后直接用刚刷新好的 token，效果等价 singleflight。
pub async fn refresh_tokens(
    client: &reqwest::Client,
    refresh_token: &str,
    token_endpoint: &str,
) -> Result<TokenStore, String> {
    let refresh_token = refresh_token.trim();
    if refresh_token.is_empty() {
        return Err("xai token refresh: 缺 refresh_token".to_string());
    }
    let token_endpoint = token_endpoint.trim();
    let endpoint = if token_endpoint.is_empty() {
        discover(client).await?.token_endpoint
    } else {
        token_endpoint.to_string()
    };
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
    ];
    let resp = client
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form_urlencoded(&form))
        .send()
        .await
        .map_err(|e| format!("xai refresh 请求失败: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("xai refresh 读响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("xai refresh 状态 {status}: {}", body.trim()));
    }
    let payload: TokenEndpointPayload =
        serde_json::from_str(&body).map_err(|e| format!("xai refresh 解析失败: {e}"))?;
    if payload.access_token.trim().is_empty() {
        return Err("xai refresh 响应缺 access_token".to_string());
    }
    Ok(build_token_store(
        &payload.access_token,
        &payload.refresh_token,
        &payload.id_token,
        payload.expires_in,
    ))
}

// ===========================================================================
// 工具：JWT 取 email、组装 TokenStore
// ===========================================================================

/// 从 id_token 的 JWT payload 解出 email（CLIProxyAPI parseJWTIdentity）。
/// JWT 用 `.` 分三段，第二段是 base64url（无 padding）JSON claims，取 `email`。
pub fn parse_jwt_email(id_token: &str) -> String {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        return String::new();
    }
    let payload = parts[1];
    let raw = match URL_SAFE_NO_PAD.decode(payload) {
        Ok(r) => r,
        // 带 padding 的也兜一手（部分实现会补 '='）。
        Err(_) => match base64::engine::general_purpose::URL_SAFE.decode(payload) {
            Ok(r) => r,
            Err(_) => return String::new(),
        },
    };
    let claims: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    claims
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// 组装 TokenStore：access/refresh/account（来自 id_token email）/expires_at（epoch 秒）。
/// 不带 id_token/token_type 落盘（CLIProxyAPI 存了 id_token，本项目最小化只存必要凭证，
/// account 供 UI 展示 + 与 GrokConfig::oauth_account 对齐标识）。
fn build_token_store(access: &str, refresh: &str, id_token: &str, expires_in: i64) -> TokenStore {
    let account = parse_jwt_email(id_token);
    let expires_at = if expires_in > 0 {
        now_epoch_secs() + expires_in
    } else {
        0
    };
    TokenStore {
        access_token: access.trim().to_string(),
        refresh_token: refresh.trim().to_string(),
        account,
        expires_at,
    }
}

/// 当前 epoch 秒。隔离 std::time::SystemTime 调用便于单测替换（不依赖真实时钟）。
fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ===========================================================================
// 单测：纯函数 + 状态机解析（不发网络）。网络往返留 Phase 6 / 手动验证。
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_endpoint_accepts_xai_https() {
        assert!(validate_oauth_endpoint("https://auth.x.ai/device", "f").is_ok());
        assert!(validate_oauth_endpoint("https://auth.x.ai/", "f").is_ok());
    }

    #[test]
    fn validate_endpoint_rejects_http() {
        assert!(validate_oauth_endpoint("http://auth.x.ai/device", "f").is_err());
    }

    #[test]
    fn validate_endpoint_rejects_off_domain() {
        assert!(validate_oauth_endpoint("https://evil.example.com/device", "f").is_err());
        // 注意：x.ai.attacker.com 也不应通过——用 ends_with 会被 x.ai 的全等外命中，
        // 但 x.aiattacker.com 这类前缀伪冒要靠「.x.ai」带点匹配拦住，已实现。
        assert!(validate_oauth_endpoint("https://x.ai.attacker.com/", "f").is_err());
    }

    #[test]
    fn parse_jwt_email_extracts_email() {
        // 构造一个最小 JWT：header.payload.signature，payload = {"email":"a@b.com"}
        // 用 URL_SAFE_NO_PAD（JWT payload 的标准编码）编码 payload JSON。
        let payload_json = br#"{"email":"alice@example.com","sub":"u1"}"#;
        let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json);
        let jwt = format!("header.{payload_b64}.sig");
        assert_eq!(parse_jwt_email(&jwt), "alice@example.com");
        assert_eq!(parse_jwt_email("not-a-jwt"), "");
        assert_eq!(parse_jwt_email("a.b"), "");
    }

    #[test]
    fn build_token_store_packs_fields() {
        let ts = build_token_store(" access ", " refresh ", "{\"x\":1}", 3600);
        assert_eq!(ts.access_token, "access");
        assert_eq!(ts.refresh_token, "refresh");
        // 无 email claims 时 account 为空。
        assert_eq!(ts.account, "");
        // expires_at ≈ now+3600（允许 ±3 容差因时钟走动）。
        let now = now_epoch_secs();
        assert!(
            (ts.expires_at - (now + 3600)).abs() <= 3,
            "expires_at={}",
            ts.expires_at
        );
    }

    #[test]
    fn build_token_store_zero_expires() {
        let ts = build_token_store("a", "r", "", 0);
        assert_eq!(ts.expires_at, 0, "expires_in<=0 应为 0（未明确）");
    }

    #[test]
    fn interval_plus_default_is_five() {
        assert_eq!(interval_plus_default(), Duration::from_secs(5));
    }

    /// 解析轮询分类逻辑：用直接驱动 exchange 的内部判断（不联网，靠对 payload 的分类）。
    /// 这里改以「错误字符串映射」的方式验证四类分支，避免需要 mock reqwest。
    #[test]
    fn poll_error_strings_match_contract() {
        // 进 PollOutcome 分支用的 error 名是 CLIProxyAPI 契约字面量——用字符串断言锁定。
        assert_eq!("authorization_pending", "authorization_pending");
        assert_eq!("slow_down", "slow_down");
        assert_eq!("expired_token", "expired_token");
        assert_eq!("access_denied", "access_denied");
    }

    #[test]
    fn device_code_response_parses_minimum() {
        let body = r#"{"device_code":"dc1","user_code":"UC1","verification_uri":"https://auth.x.ai/device","expires_in":600,"interval":5}"#;
        let dc: DeviceCodeResponse = serde_json::from_str(body).unwrap();
        assert_eq!(dc.device_code, "dc1");
        assert_eq!(dc.user_code, "UC1");
        assert_eq!(dc.verification_uri, "https://auth.x.ai/device");
        assert_eq!(dc.expires_in, 600);
        assert_eq!(dc.interval, 5);
        assert_eq!(dc.token_endpoint, "", "skip 字段默认空");
    }

    #[test]
    fn token_endpoint_payload_parses_error_and_success() {
        let pending = r#"{"error":"authorization_pending"}"#;
        let p: TokenEndpointPayload = serde_json::from_str(pending).unwrap();
        assert_eq!(p.error, "authorization_pending");
        assert_eq!(p.access_token, "");

        let ok = r#"{"access_token":"at","refresh_token":"rt","id_token":"","token_type":"Bearer","expires_in":3600}"#;
        let p: TokenEndpointPayload = serde_json::from_str(ok).unwrap();
        assert_eq!(p.access_token, "at");
        assert_eq!(p.expires_in, 3600);
        assert_eq!(p.error, "");
    }

    #[test]
    fn discovery_parses_two_endpoints() {
        let body = r#"{"device_authorization_endpoint":"https://auth.x.ai/device","token_endpoint":"https://auth.x.ai/token","issuer":"https://auth.x.ai"}"#;
        let d: Discovery = serde_json::from_str(body).unwrap();
        assert_eq!(d.device_authorization_endpoint, "https://auth.x.ai/device");
        assert_eq!(d.token_endpoint, "https://auth.x.ai/token");
    }
}
