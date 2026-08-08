// config —— P0.5: 把 provider 配置从硬编码抽到 TOML。
//
// 设计目标:「换供应商 = 换配置,不换代码」。
// 一个 provider = { base_url, model, api_key_env } 全部信息。
// 真正的 key 不落 config 文件,只存「去哪个环境变量名取 key」,
// 这样 codeagent.toml 可以入库分享给别人(推广用),真 key 各自设环境变量。
//
// P4: 新增 [approval] 段 —— 把 P3「每次都问」的 y/n 闸做成可配置白名单,
//     命中白名单的 destructive 命令(bash 前缀)自动放行,其余才问;无配置回退 P3 行为(全问)。
//     读类工具本不过闸(is_destructive=false),不需要白名单 —— P3 已对。
//
// 配置文件 codeagent.toml 放在 exe 同级或当前工作目录(后续可扩成 ~/.codeagent/)。

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;

/// 顶层配置:default 指向当前用哪个 provider,provider 一组,[approval] 调审批闸白名单。
#[derive(Debug, Deserialize)]
pub struct Config {
    pub default: String,
    #[serde(default)]
    pub provider: HashMap<String, Provider>,
    /// P4 审批白名单。缺省(无 [approval] 段)= 全问,回退 P3 行为;向后兼容。
    #[serde(default)]
    pub approval: ApprovalConfig,
}

/// 单个供应商。base_url 用 https,OpenAI 兼容协议(/chat/completions)。
#[derive(Debug, Deserialize, Clone)]
pub struct Provider {
    pub base_url: String,
    pub model: String,
    /// 真正 api key 的环境变量名(如 "DEEPSEEK_API_KEY")。
    /// 配置里不存真 key,只存「去哪儿拿 key」——安全 + 可分享。
    pub api_key_env: String,
}

/// P4 审批白名单配置。
/// 命中即自动放行、免 y/n;未命中才走人的 y/n 闸。
/// 这条只管 destructive 工具(bash);读类工具本不过闸(is_destructive=false)。
#[derive(Debug, Default, Deserialize, Clone)]
pub struct ApprovalConfig {
    /// bash 命令前缀白名单:命令以这些串开头则自动放行。
    /// 例:["git status", "ls", "cargo ", "npm ", "pwd", "echo"]
    /// 用「前缀」而非「全等」以容 `cargo build`、`cargo run -- ` 这种带参命令;
    /// 故白名单里 `cargo `(带尾空格) 比 `cargo` 更严 —— 防误放 `cargo-devil`。
    #[serde(default)]
    pub bash_allow_prefix: Vec<String>,
}

impl Config {
    /// 从 path 加载 TOML 配置。
    pub fn load(path: &std::path::Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("解析 TOML 失败: {}", path.display()))?;
        Ok(cfg)
    }

    /// 取 default 指向的 provider,若不存在给出清晰错误。
    pub fn default_provider(&self) -> Result<&Provider> {
        self.provider
            .get(&self.default)
            .with_context(|| format!("default 指向的 provider \"{}\" 不存在", self.default))
    }
}

impl Provider {
    /// 从 api_key_env 指明的环境变量读真 key。
    /// 把「key 是否就位」的失败点集中在这里,P1+ 给 agent 回灌错误时也只这一处。
    pub fn api_key(&self) -> Result<String> {
        std::env::var(&self.api_key_env).with_context(|| {
            format!(
                "缺少环境变量 {} (provider \"{}\" 的 api key)",
                self.api_key_env, self.base_url
            )
        })
    }

    /// Chat Completions 端点 = base_url + "/chat/completions"。
    /// 末尾斜杠统一裁掉,避免 //chat/completions。
    pub fn chat_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }
}
