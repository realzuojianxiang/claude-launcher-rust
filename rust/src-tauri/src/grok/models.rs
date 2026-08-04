// Grok provider 的入站请求结构 + 配置定义。
//
// Grok 代理对外暴露标准的 Anthropic Messages 端点（POST /v1/messages），
// 入站请求结构与 NVIDIA 侧一致，因此直接复用 `crate::nvidia::models::AnthropicRequest`，
// 不在本模块重新定义。本文件只承载 Grok 代理自身的运行/认证配置。

use serde::{Deserialize, Serialize};

// 与 CLIProxyAPI `internal/auth/xai/types.go` 一致的默认上游地址：
//   - OAuth 模式（默认）：官方 Grok CLI Chat-Proxy，OpenAI Responses 格式，Bearer token
//   - API Key 退路模式：官方 api.x.ai，OpenAI Chat Completions/Responses 兼容，Bearer api key
// 两个 base 用同一组 converter 不行——OAuth 走 /responses（Responses 协议），
// API Key 也走 /responses（xAI 官方同样支持 Responses 端点），因此 converter 可共用 Responses 协议；
// 仅认证头与是否加 CLI Chat-Proxy 专用头（X-XAI-Token-Auth / x-grok-client-version）不同。

/// 认证模式：OAuth（默认，走 CLI Chat-Proxy 用 Plus 账号额度）或 API Key（退路，api.x.ai）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrokAuthMode {
    /// OAuth2 Device Code Flow，token 加密落 grok-oauth.json，调 cli-chat-proxy.grok.com
    #[default]
    Oauth,
    /// 官方 API Key（xai-...），调 api.x.ai，作号被风控时的退路
    ApiKey,
}

/// Grok 代理配置。
///
/// 与 `NvidiaConfig` 平级、独立端口（默认 8083，避开 NVIDIA 8082）。
/// 随 `config.json` 的 `Config.grok` 字段落盘；**OAuth token 不落此结构**，
/// 单独 DPAPI 加密存 `grok-oauth.json`，此处的 `oauth_account` 只存授权账号的 email 标识。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrokConfig {
    /// 认证模式：oauth / api-key
    #[serde(default)]
    pub auth_mode: GrokAuthMode,

    /// OAuth 模式上游 base（默认 CLI Chat-Proxy）；api-key 模式忽略，用 api_base_url。
    #[serde(default = "default_oauth_base_url")]
    pub oauth_base_url: String,

    /// API Key 退路模式上游 base（默认官方 api.x.ai）。
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,

    /// 官方 API Key 退路模式用（多条可轮询，复用 nvidia::key_pool 语义）。
    #[serde(default)]
    pub api_keys: Vec<String>,

    /// grok 模型优先级列表（grok-4.3 / grok-3-mini …），用于 fallback 与默认。
    #[serde(default)]
    pub models: Vec<String>,

    /// 模型名映射：claude-* → grok-*。入站携带 claude 模型名时按此表改写为 grok slug。
    /// 未命中时回退到 `models` 的第一项（或映射默认规则）。
    #[serde(default)]
    pub model_map: Vec<ModelMapEntry>,

    /// 本地代理监听 host（默认回环，避免对外暴露无鉴权代理）。
    #[serde(default = "default_host")]
    pub host: String,

    /// 本地代理监听端口（默认 8083，避开 NVIDIA 8082）。
    #[serde(default = "default_port")]
    pub port: u16,

    /// 命中 429 / free-usage-exhausted 后的会话/Key 冷却时长（秒）。
    #[serde(default = "default_cooldown")]
    pub cooldown_seconds: u64,

    /// 单请求最大重试次数（切 Key / 切模型 / token refresh 后重试）。
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// 上游请求超时（秒）。流式响应另行用逐 chunk 空闲超时守护。
    #[serde(default = "default_request_timeout")]
    pub request_timeout_seconds: u64,

    /// 本地代理鉴权 token（校验请求头 x-api-key/auth_token），空串表示不校验（仅回环可）。
    /// 注入到 claude 子进程后，子进程通过该 token 访问本地代理。
    #[serde(default)]
    pub auth_token: String,

    /// 当前已授权的 OAuth 账号 email（仅作 UI 展示 + 标识 token 文件，不含 token 本身）。
    #[serde(default)]
    pub oauth_account: String,
}

/// 模型映射表条目：Anthropic 侧模型名 → Grok 上游模型 slug。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMapEntry {
    pub anthropic_model: String,
    pub grok_model: String,
}

fn default_oauth_base_url() -> String {
    "https://cli-chat-proxy.grok.com/v1".to_string()
}
fn default_api_base_url() -> String {
    "https://api.x.ai/v1".to_string()
}
fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    8083
}
fn default_cooldown() -> u64 {
    600
}
fn default_max_retries() -> u32 {
    3
}
fn default_request_timeout() -> u64 {
    600
}

impl Default for GrokConfig {
    fn default() -> Self {
        Self {
            auth_mode: GrokAuthMode::default(),
            oauth_base_url: default_oauth_base_url(),
            api_base_url: default_api_base_url(),
            api_keys: Vec::new(),
            models: Vec::new(),
            model_map: Vec::new(),
            host: default_host(),
            port: default_port(),
            cooldown_seconds: default_cooldown(),
            max_retries: default_max_retries(),
            request_timeout_seconds: default_request_timeout(),
            auth_token: String::new(),
            oauth_account: String::new(),
        }
    }
}

impl GrokConfig {
    /// 当前认证模式对应的上游 base URL。
    pub fn effective_base_url(&self) -> &str {
        match self.auth_mode {
            GrokAuthMode::Oauth => &self.oauth_base_url,
            GrokAuthMode::ApiKey => &self.api_base_url,
        }
    }

    /// 上游 base_url 安全校验（SSRF 闸）：scheme + host 非空。
    /// 委托给 `shared::validate_base_url`，与 nvidia 侧保持一致约束。
    pub fn validate_base_url(&self) -> Result<(), String> {
        crate::shared::validate_base_url(self.effective_base_url(), "Grok")
    }

    /// 非回环绑定强制高熵 auth_token。
    pub fn require_auth_if_exposed(&self) -> Result<(), String> {
        crate::shared::require_auth_if_exposed(&self.host, &self.auth_token)
    }

    /// 把入站的 Anthropic 模型名映射为 Grok 上游 slug。
    ///
    /// 优先在 `model_map` 中按大小写不敏感匹配 `anthropic_model`；
    /// 命中则返回对应的 grok slug。未命中时：若 `models` 非空，返回 `models[0]`
    /// （最高优先级默认模型）；否则返回原入站名（让上游报错，便于发现未配置）。
    pub fn map_model(&self, anthropic_model: &str) -> String {
        for e in &self.model_map {
            if e.anthropic_model.eq_ignore_ascii_case(anthropic_model)
                && !e.grok_model.trim().is_empty()
            {
                return e.grok_model.trim().to_string();
            }
        }
        // 通配：claude-sonnet-* / claude-haiku-* 之类给个合理默认，避免要求用户逐个配。
        let lower = anthropic_model.to_ascii_lowercase();
        let mapped_from_prefix = match () {
            _ if lower.starts_with("claude-haiku") || lower.starts_with("claude-3-haiku") => self
                .first_model_matching(|m| {
                    let ml = m.to_ascii_lowercase();
                    ml.contains("mini-fast") || ml.contains("mini")
                }),
            _ => self.first_model_matching(|_| true),
        };
        mapped_from_prefix.unwrap_or_else(|| anthropic_model.to_string())
    }

    fn first_model_matching(&self, predicate: impl Fn(&str) -> bool) -> Option<String> {
        self.models
            .iter()
            .find(|m| {
                let mt = m.trim();
                !mt.is_empty() && predicate(mt)
            })
            .map(|m| m.trim().to_string())
            .or_else(|| self.models.first().map(|m| m.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_cli_chat_proxy_and_port_8083() {
        let c = GrokConfig::default();
        assert_eq!(c.auth_mode, GrokAuthMode::Oauth);
        assert_eq!(c.oauth_base_url, "https://cli-chat-proxy.grok.com/v1");
        assert_eq!(c.api_base_url, "https://api.x.ai/v1");
        assert_eq!(c.host, "127.0.0.1");
        assert_eq!(c.port, 8083);
        assert_eq!(c.effective_base_url(), "https://cli-chat-proxy.grok.com/v1");
    }

    #[test]
    fn api_key_mode_uses_api_base_url() {
        let c = GrokConfig {
            auth_mode: GrokAuthMode::ApiKey,
            ..Default::default()
        };
        assert_eq!(c.effective_base_url(), "https://api.x.ai/v1");
    }

    #[test]
    fn validate_base_url_runs_ssrf_gate() {
        assert!(GrokConfig::default().validate_base_url().is_ok());
        let bad = GrokConfig {
            oauth_base_url: "ftp://x".to_string(),
            ..Default::default()
        };
        assert!(bad.validate_base_url().is_err());
    }

    #[test]
    fn external_bind_requires_strong_token() {
        let mut c = GrokConfig::default();
        assert!(c.require_auth_if_exposed().is_ok()); // 回环不要求
        c.host = "0.0.0.0".to_string();
        assert!(c.require_auth_if_exposed().is_err()); // 对外但无 token
        c.auth_token = "0123456789abcdef01234567".to_string();
        assert!(c.require_auth_if_exposed().is_ok());
    }

    #[test]
    fn map_model_uses_explicit_table_first() {
        let c = GrokConfig {
            models: vec!["grok-4.3".to_string(), "grok-3-mini-fast".to_string()],
            model_map: vec![ModelMapEntry {
                anthropic_model: "claude-sonnet-4".to_string(),
                grok_model: "grok-4.3".to_string(),
            }],
            ..Default::default()
        };
        assert_eq!(c.map_model("claude-sonnet-4"), "grok-4.3");
        assert_eq!(
            c.map_model("claude-haiku-4-5"),
            "grok-3-mini-fast",
            "haiku 系列默认回退到 mini-fast"
        );
        assert_eq!(
            c.map_model("claude-opus-5"),
            "grok-4.3",
            "未命中表时回退到 models[0]"
        );
    }

    #[test]
    fn map_model_passthrough_when_unconfigured() {
        let c = GrokConfig::default(); // 无 models 无 map
        assert_eq!(c.map_model("anything"), "anything");
    }
}
