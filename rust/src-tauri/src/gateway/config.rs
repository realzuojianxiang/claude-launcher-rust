// 协议网关配置：ProviderEntry + GatewayConfig。
//
// 8083 端口作为通用「Anthropic 入站 ↔ OpenAI 协议上游」转换层，支持多个 provider 并存。
// 每个 provider 挂载到 Chat Completions 协议（OpenAI 兼容 /v1/chat/completions），
// 共享同一套代理重试/冷却/鉴权骨架。providers 是一个 Vec，前端用下拉/tab 切换；
// active_provider 指向当前选中的。

use serde::{Deserialize, Serialize};

/// 上游协议类型。当前仅支持 OpenAI Chat Completions（/v1/chat/completions），
/// 即绝大多数 OpenAI 兼容端点（deepseek / glm / qwen 等）使用的协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProtocolKind {
    /// OpenAI Chat Completions（/v1/chat/completions，messages[] 数组）
    #[default]
    ChatCompletions,
}

/// 认证模式：当前仅支持 API Key 轮询（多数 OpenAI 兼容端点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    #[default]
    ApiKey,
}

/// 单个 provider 配置。
///
/// 通用字段（所有 provider 都有）：name / protocol / base_url / api_keys / models /
/// host / port / cooldown / retries / timeout / auth_token / model_map。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    /// 唯一标识（如 "deepseek" / "glm"）。前端用它做 provider 切换 key。
    pub id: String,
    /// 展示名（如 "DeepSeek" / "GLM"）。
    pub name: String,
    /// 上游协议类型。
    #[serde(default)]
    pub protocol: ProtocolKind,
    /// 认证模式。
    #[serde(default)]
    pub auth_mode: AuthMode,
    /// 上游 base URL（含 /v1）。
    #[serde(default)]
    pub base_url: String,
    /// API Key 列表（多条可轮询 + 429 冷却）。
    #[serde(default)]
    pub api_keys: Vec<String>,
    /// 模型优先级列表（[0] 为默认/最高优先级，其后为 Fallback）。
    #[serde(default)]
    pub models: Vec<String>,
    /// 模型名映射：Anthropic 侧模型名 → 上游 slug。可选；缺省时回落到 models[0]。
    #[serde(default)]
    pub model_map: Vec<ModelMapEntry>,
    /// 本地代理监听 host（默认回环）。
    #[serde(default = "default_host")]
    pub host: String,
    /// 本地代理监听端口（8083 通用网关）。
    #[serde(default = "default_port")]
    pub port: u16,
    /// 命中 429 后的 Key 冷却时长（秒）。
    #[serde(default = "default_cooldown")]
    pub cooldown_seconds: u64,
    /// 单请求最大重试次数。
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// 上游请求超时（秒）。
    #[serde(default = "default_request_timeout")]
    pub request_timeout_seconds: u64,
    /// 本地代理鉴权 token（空串表示不校验，仅回环允许）。
    #[serde(default)]
    pub auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMapEntry {
    pub anthropic_model: String,
    pub provider_model: String,
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

/// 协议网关配置：provider 列表 + 当前选中。
///
/// 随 config.json 的 `Config.gateway` 字段落盘。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayConfig {
    /// 所有 provider 配置。第一个是默认 provider。
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
    /// 当前选中的 provider id（前端下拉切换后写回）。空则用 providers[0].id。
    #[serde(default)]
    pub active_provider: String,
}

impl GatewayConfig {
    /// 取当前选中的 provider。无 provider 时返回 None。
    pub fn active(&self) -> Option<&ProviderEntry> {
        if self.providers.is_empty() {
            return None;
        }
        if self.active_provider.is_empty() {
            return Some(&self.providers[0]);
        }
        self.providers
            .iter()
            .find(|p| p.id == self.active_provider)
            .or_else(|| Some(&self.providers[0]))
    }

    /// 取当前选中的 provider 的可变引用。
    pub fn active_mut(&mut self) -> Option<&mut ProviderEntry> {
        if self.providers.is_empty() {
            return None;
        }
        let id = if self.active_provider.is_empty() {
            self.providers[0].id.clone()
        } else {
            self.active_provider.clone()
        };
        self.providers.iter_mut().find(|p| p.id == id)
    }

    /// 按 id 取 provider。
    #[allow(dead_code)] // 为前端按 id 查询预留
    pub fn get(&self, id: &str) -> Option<&ProviderEntry> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// 安全校验：所有 provider 的 base_url 都过 SSRF 闸。
    /// 非回环 host 的 provider 强制要求高熵 auth_token。
    pub fn validate_all(&self) -> Result<(), String> {
        for p in &self.providers {
            crate::shared::validate_base_url(&p.base_url, &p.name)?;
            crate::shared::require_auth_if_exposed(&p.host, &p.auth_token)?;
        }
        Ok(())
    }
}

/// 模型名映射：入站 claude-* 按表改写为上游 slug。
/// 未命中时回退到 models[0]（或按模型名做轻量回落规则）。
pub fn map_model(provider: &ProviderEntry, anthropic_model: &str) -> String {
    for e in &provider.model_map {
        if e.anthropic_model.eq_ignore_ascii_case(anthropic_model)
            && !e.provider_model.trim().is_empty()
        {
            return e.provider_model.trim().to_string();
        }
    }
    let lower = anthropic_model.to_ascii_lowercase();
    let mapped = match () {
        _ if lower.starts_with("claude-haiku") || lower.starts_with("claude-3-haiku") => provider
            .models
            .iter()
            .find(|m| {
                let ml = m.to_ascii_lowercase();
                ml.contains("mini-fast") || ml.contains("mini")
            })
            .or_else(|| provider.models.first()),
        _ => provider.models.first(),
    };
    mapped
        .map(|m| m.trim().to_string())
        .unwrap_or_else(|| anthropic_model.to_string())
}

/// 有效上游 base URL（当前仅 base_url 一路，无 OAuth 分支）。
pub fn effective_base_url(provider: &ProviderEntry) -> &str {
    &provider.base_url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_provider(id: &str) -> ProviderEntry {
        ProviderEntry {
            id: id.to_string(),
            name: id.to_string(),
            protocol: ProtocolKind::ChatCompletions,
            auth_mode: AuthMode::ApiKey,
            base_url: "https://api.example.com/v1".to_string(),
            api_keys: vec!["sk-test".to_string()],
            models: vec!["model-a".to_string()],
            model_map: vec![],
            host: "127.0.0.1".to_string(),
            port: 8083,
            cooldown_seconds: 60,
            max_retries: 3,
            request_timeout_seconds: 600,
            auth_token: String::new(),
        }
    }

    #[test]
    fn active_returns_first_when_no_active_set() {
        let gw = GatewayConfig {
            providers: vec![sample_provider("a"), sample_provider("b")],
            active_provider: String::new(),
        };
        assert_eq!(gw.active().unwrap().id, "a");
    }

    #[test]
    fn active_returns_selected() {
        let gw = GatewayConfig {
            providers: vec![sample_provider("a"), sample_provider("b")],
            active_provider: "b".to_string(),
        };
        assert_eq!(gw.active().unwrap().id, "b");
    }

    #[test]
    fn active_falls_back_to_first_when_id_not_found() {
        let gw = GatewayConfig {
            providers: vec![sample_provider("a")],
            active_provider: "nonexistent".to_string(),
        };
        assert_eq!(gw.active().unwrap().id, "a");
    }

    #[test]
    fn map_model_uses_explicit_table() {
        let p = ProviderEntry {
            models: vec!["gpt-4.1".to_string(), "gpt-4.1-mini".to_string()],
            model_map: vec![ModelMapEntry {
                anthropic_model: "claude-sonnet-4".to_string(),
                provider_model: "gpt-4.1".to_string(),
            }],
            ..sample_provider("acme")
        };
        assert_eq!(map_model(&p, "claude-sonnet-4"), "gpt-4.1");
        assert_eq!(map_model(&p, "claude-haiku-4-5"), "gpt-4.1-mini");
        assert_eq!(map_model(&p, "claude-opus-5"), "gpt-4.1");
    }

    #[test]
    fn effective_base_url_returns_base_url() {
        let p = ProviderEntry {
            base_url: "https://api.acme.com/v1".to_string(),
            ..sample_provider("acme")
        };
        assert_eq!(effective_base_url(&p), "https://api.acme.com/v1");
    }

    #[test]
    fn validate_all_rejects_bad_base_url() {
        let gw = GatewayConfig {
            providers: vec![ProviderEntry {
                base_url: "ftp://bad".to_string(),
                ..sample_provider("bad")
            }],
            active_provider: "bad".to_string(),
        };
        assert!(gw.validate_all().is_err());
    }
}
