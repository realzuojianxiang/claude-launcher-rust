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
    /// P6.1 上下文压缩参数。缺省(无 [compaction] 段)= 70%/40% 默认 + 取 provider.max_context
    /// 或兜底默认;向后兼容(老 codeagent.toml 不写这一段仍正常)。
    #[serde(default)]
    pub compaction: Compaction,
    /// P8: MCP stdio server 配置。缺省(无 [mcp] 段)= 空 = 不起任何 MCP server;向后兼容。
    #[serde(default)]
    pub mcp: McpConfig,
}

/// 单个供应商。base_url 用 https,OpenAI 兼容协议(/chat/completions)。
///
/// `max_context` 是该 provider 所用模型的**上下文窗口上限 token 数**(P6.1)。
/// —— 压缩阈值要按模型来(例:DeepSeek 上限约 1M),故放 provider 段而非全局。
/// 缺省走 `Compaction::default_max_context()`(见 compaction 模块),给一个保守值兜底;
/// 用大窗口模型(DeepSeek 1M)时应在配置里显式填,否则压缩会过早触发(浪费 token)。
#[derive(Debug, Deserialize, Clone)]
pub struct Provider {
    pub base_url: String,
    pub model: String,
    /// 真正 api key 的环境变量名(如 "DEEPSEEK_API_KEY")。
    /// 配置里不存真 key,只存「去哪儿拿 key」——安全 + 可分享。
    pub api_key_env: String,
    /// 该 provider 模型的上下文窗口上限(token)。P6.1 压缩按它定阈值。
    /// 缺省走 Compaction 兜底默认;大窗口模型应显式填(见 journey §12)。
    #[serde(default)]
    pub max_context: Option<u64>,
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

/// P8: MCP (Model Context Protocol) stdio 客户端配置。
///
/// codeagent 作为 MCP **client**,起外部 MCP server 子进程、与其 stdin/stdout 走 JSON-RPC 2.0,
/// 把 server 暴露的工具接进自己的工具表(模型看到同形 OpenAI function,按 full name 分派)。
/// 缺整个 `[mcp]` 段 = 空 server 集 = 不起任何子进程,与 P7 行为完全一致(向后兼容)。
///
/// `McpConfig` derive `Default`(HashMap 空 = 默认)—— 没有像 `Compaction` 那样的「段缺 vs 字段缺
/// 默认值分化」陷阱(§12):server 集要么有要么无,字段全必填(见 `McpServerConfig`),无字段级
/// `#[serde(default)]` 取 0 的问题。
#[derive(Debug, Default, Deserialize, Clone)]
pub struct McpConfig {
    /// 键 = server 友好名(用于 prefix 命名与日志),值 = server 进程配置。
    /// `[mcp.server.<name>]` 段,例:
    /// ```toml
    /// [mcp.server.filesystem]
    /// command = "npx"
    /// args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
    /// prefix = "fs"   # 可选,防撞内置 tool 名;
    ///                #   写了则该 server 工具名前缀成 `fs_read_file` 等
    /// ```
    #[serde(default)]
    pub server: HashMap<String, McpServerConfig>,
}

/// 单个 MCP server 子进程的启动配置。
#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    /// 启动命令(如 `npx` / `node` / 完整路径)。
    pub command: String,
    /// 命令参数(不带引号转义,逐元素一条一个 argv)。
    pub args: Vec<String>,
    /// 给该子进程的额外环境变量(可选)。不继承父进程 env 的覆盖场景才填;
    /// 缺省子进程继承 codeagent 的全部环境(含取 api key 的那个变量名)。
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    /// 工具名前缀(可选)。该 server 的所有工具名前缀成 `<名字>_<原名>`,
    /// 防止与内置工具(read_file / bash 等)撞名导致分派歧义。留空则原名直用。
    #[serde(default)]
    pub prefix: Option<String>,
    /// 握手阶段(`initialize` + `tools/list`)每请求的超时秒数(可选)。缺省走
    /// `mcp::HANDSHAKE_TIMEOUT_SECS_DEFAULT`(60s)—— 故意比运行期 `tools/call` 的 30s 宽,
    /// 给 `npx -y <pkg>` 首次冷拉包留余量(P8 实证 npx -y 首拉就占满旧 30s 必超时,
    /// journey §13.6 真坑二)。热包后真握手其实秒回,只是别在冷启动误判成「server 没回」。
    /// 显式填可逐 server 调(慢机器/大包填大些、本地原生二进制 server 填小些)。
    #[serde(default)]
    pub handshake_timeout_secs: Option<u64>,
}

/// P6.1 上下文压缩参数(见 journey §12)。
///
/// 三段参数,都带 sane 默认(老 codeagent.toml 不写 [compaction] 段仍正常):
///   · `compact_at_ratio`:total 达到 `max_context × compact_at_ratio` 即触发压缩。
///     默认 0.7(到 70% 开窗)。
///   · `compact_to_ratio`:压缩目标 —— 把要压的旧消息收掉后,总量降到约
///     `max_context × compact_to_ratio`(默认 0.4,压到 40%)。它决定「保留最近几轮原始、
///     其余摘要」的切点,不是死轮数,而是按 token 量倒推。
///   · `keep_recent_turns`:无论如何最近这 N 个「用户轮」及其后的 assistant/tool 消息
///     保留原始(不压),保证模型对眼下这几轮有全量细节。默认 4。
///
/// `max_context` 不放这里 —— 它按模型来,放 provider 段(DeepSeek ~1M 大窗口)。
/// 这里只放「压缩策略」的可调参数。
///
/// 注意:**不 derive `Default`**,手写 `impl Default` 给 sane 默认(0.7/0.4/4)。
/// 因为 `Config#compaction` 用 `#[serde(default)]` —— 老 toml 不写整个 `[compaction]`
/// 段时,serde 调的就是 `Compaction::default()`。derive 出来的 Default 对 f64/usize 给
/// 全 0,会让压缩阈值 = 0(每轮都触发)、keep_recent_turns = 0(尾段永远空),
/// 与「不写 = 走 sane 默认」的文档承诺相反。手写 impl 与字段级 free fn 默认取值一致,
/// 消除「段缺」与「字段缺」两种路径默认值不同的陷阱(journey §12)。
#[derive(Debug, Deserialize, Clone)]
pub struct Compaction {
    /// 达到 max_context 的此比例触发压缩。默认 0.7。
    #[serde(default = "default_compact_at_ratio")]
    pub compact_at_ratio: f64,
    /// 压缩目标比例。默认 0.4。
    #[serde(default = "default_compact_to_ratio")]
    pub compact_to_ratio: f64,
    /// 保留最近几个「用户轮」原始(不压)。默认 4。
    #[serde(default = "default_keep_recent_turns")]
    pub keep_recent_turns: usize,
}

/// `Compaction` 的 sane 默认集合。`#[serde(default)]`(无参版)在父字段整段缺失时
/// 调它,与字段级自由函数默认取一致值 —— 保证「不写 `[compaction]` 段」与「写了段
/// 但某字段缺」两条路径默认值相同,不再出「段缺 = 全 0」的陷阱。
impl Default for Compaction {
    fn default() -> Self {
        Self {
            compact_at_ratio: default_compact_at_ratio(),
            compact_to_ratio: default_compact_to_ratio(),
            keep_recent_turns: default_keep_recent_turns(),
        }
    }
}

/// Compaction 的 serde 缺省值函数(因 serde(default="fn")要自由函数,不能写在 impl 里)。
/// 取值的 rationale 与 compaction 模块的硬常量一致(journey §12)。
fn default_compact_at_ratio() -> f64 {
    crate::compactor::DEFAULT_COMPACT_AT_RATIO
}
fn default_compact_to_ratio() -> f64 {
    crate::compactor::DEFAULT_COMPACT_TO_RATIO
}
fn default_keep_recent_turns() -> usize {
    crate::compactor::DEFAULT_KEEP_RECENT_TURNS
}

impl Compaction {
    /// provider.max_context 缺省时的兜底上下文上限(保守,小窗口模型假设)。
    /// 用大窗口模型务必在配置里显式填 max_context,否则压缩会过早触发。
    pub const DEFAULT_MAX_CONTEXT: u64 = 32_000;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小可解析 toml:`default` + 一个 provider,无 `[compaction]` 段。
    /// 锁:「不写 `[compaction]` 段 = 走 sane 默认(0.7/0.4/4)」,**不是** derive
    /// `Default` 的全 0。这是 §12 的回归门 —— `#[serde(default)]` 在父字段整段
    /// 缺失时会调 `Compaction::default()`;若 `Default` 是 derive 的,给的是 0.0/0.0/0,
    /// 导致压缩阈值=0(每轮都触发)、keep_recent_turns=0(尾段永空),与文档承诺相反。
    /// 手写 `impl Default`(与字段级 free fn 同取 sane 常量)解之;此处焊死不再回退。
    #[test]
    fn compaction_default_when_section_missing_is_sane() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("最小 toml 必须解析");
        let c = &cfg.compaction;
        assert!(
            (c.compact_at_ratio - 0.7).abs() < 1e-9,
            "无 [compaction] 段时 compact_at_ratio 应为 0.7, 实得 {}",
            c.compact_at_ratio
        );
        assert!(
            (c.compact_to_ratio - 0.4).abs() < 1e-9,
            "无 [compaction] 段时 compact_to_ratio 应为 0.4, 实得 {}",
            c.compact_to_ratio
        );
        assert_eq!(
            c.keep_recent_turns, 4,
            "无 [compaction] 段时 keep_recent_turns 应为 4, 实得 {}",
            c.keep_recent_turns
        );
    }

    /// 锁:写了 `[compaction]` 段、但只填一两个字段 —— 剩下的字段走字段级 free fn 默认,
    /// 也应是 sane(0.7/0.4/4),与上例「整段缺」的结果一致 —— 两条默认路径不分化。
    #[test]
    fn compaction_partial_fields_fall_back_to_sane() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
[compaction]
keep_recent_turns = 8
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("partial compaction 必须解析");
        let c = &cfg.compaction;
        assert_eq!(c.keep_recent_turns, 8, "显式填的字段应原样保留");
        assert!(
            (c.compact_at_ratio - 0.7).abs() < 1e-9,
            "未填字段应回退 sane 0.7, 实得 {}",
            c.compact_at_ratio
        );
        assert!(
            (c.compact_to_ratio - 0.4).abs() < 1e-9,
            "未填字段应回退 sane 0.4, 实得 {}",
            c.compact_to_ratio
        );
    }

    /// 锁:provider.max_context 缺省走 `None`(不是某硬编码兜底);
    /// `Compaction::DEFAULT_MAX_CONTEXT` 兜底值在 main.rs 用 `unwrap_or` 处取,本测只验
    /// 配置层拿到的形态 —— 缺 max_context 字段即 None。
    #[test]
    fn provider_max_context_optional_when_missing() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("必须解析");
        let p = cfg.default_provider().expect("default provider 必须存在");
        assert!(p.max_context.is_none(), "未填 max_context 应为 None");
    }

    /// P8: 不写整个 `[mcp]` 段 → 空 server 集 → 不起任何 MCP 子进程(向后兼容 P7 行为)。
    /// `McpConfig` derive `Default`(HashMap 空 = 默认),无 §12 那种「段缺 vs 字段缺默认分化」陷阱。
    #[test]
    fn mcp_section_missing_yields_empty_servers() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("必须解析");
        assert!(cfg.mcp.server.is_empty(), "无 [mcp] 段应为空 server 集");
    }

    /// P8: 多 server 段正常解析,每个 server 的 command/args 字段原样保留。
    /// 验 `[mcp.server.<name>]` 嵌套 TOML 结构正确映射到 `HashMap<String, McpServerConfig>`。
    #[test]
    fn mcp_parses_multiple_servers() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
[mcp.server.fs]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
[mcp.server.git]
command = "node"
args = ["server-git.js"]
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("多 server 必须解析");
        assert_eq!(cfg.mcp.server.len(), 2, "应解析出 2 个 server");
        let fs = cfg.mcp.server.get("fs").expect("fs server 必须存在");
        assert_eq!(fs.command, "npx");
        assert_eq!(
            fs.args,
            vec!["-y", "@modelcontextprotocol/server-filesystem", "."]
        );
        let git = cfg.mcp.server.get("git").expect("git server 必须存在");
        assert_eq!(git.command, "node");
        assert_eq!(git.args.len(), 1);
    }

    /// P8: `prefix` 字段可选(缺省 None → 工具名前缀成 `<原名>`,即不加前缀)。
    #[test]
    fn mcp_prefix_optional_when_missing() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
[mcp.server.fs]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("必须解析");
        let fs = cfg.mcp.server.get("fs").expect("fs 必须存在");
        assert!(fs.prefix.is_none(), "未填 prefix 应为 None");
        assert!(fs.env.is_none(), "未填 env 应为 None");
    }

    /// P8: `env` 字段可选;填了则原样保留为 HashMap。
    #[test]
    fn mcp_env_optional_when_missing() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
[mcp.server.fs]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
[mcp.server.fs.env]
API_KEY = "sk-test-123"
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("必须解析");
        let fs = cfg.mcp.server.get("fs").expect("fs 必须存在");
        let env = fs.env.as_ref().expect("已填 env 段应不为 None");
        assert_eq!(env.get("API_KEY").map(|s| s.as_str()), Some("sk-test-123"));
    }

    // ── P9-2:握手超时字段 `handshake_timeout_secs` 三条默认路径不分化(§12.7 风格)。──

    /// ① 段缺(`[mcp.server.*]` 里不写 `handshake_timeout_secs` 字段)→ None。
    ///    spawn 时 None → 走 `HANDSHAKE_TIMEOUT_SECS_DEFAULT`(60s)。
    #[test]
    fn mcp_handshake_timeout_optional_when_field_missing() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
[mcp.server.fs]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("必须解析");
        let fs = cfg.mcp.server.get("fs").expect("fs 必须存在");
        assert!(
            fs.handshake_timeout_secs.is_none(),
            "未填 handshake_timeout_secs 应为 None(让 spawn 走 60s 默认)"
        );
        // sane 默认真源在 mcp::HANDSHAKE_TIMEOUT_SECS_DEFAULT;spawn 走它,本测也直接引它,
        // 顺势锁「默认值就是 60」—— 改默认要同时撞这里,防误改。
        assert_eq!(
            crate::mcp::HANDSHAKE_TIMEOUT_SECS_DEFAULT,
            60,
            "握手超时 sane 默认应为 60s(npx -y 首拉余量)"
        );
    }

    /// ② 显式填:`handshake_timeout_secs = 120` 原样保留(None 不回退默认)。
    #[test]
    fn mcp_handshake_timeout_explicit_value_is_kept() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
[mcp.server.fs]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
handshake_timeout_secs = 120
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("必须解析");
        let fs = cfg.mcp.server.get("fs").expect("fs 必须存在");
        assert_eq!(
            fs.handshake_timeout_secs,
            Some(120),
            "显式填的握手超时秒数应原样保留,不被默认覆盖"
        );
    }

    /// ③ 多 server 各自独立:一个 server 填了、另一个不填,互不串味。
    #[test]
    fn mcp_handshake_timeout_per_server_independent() {
        let toml_text = r#"
default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
[mcp.server.slow]
command = "npx"
args = ["-y", "@big/pkg"]
handshake_timeout_secs = 180
[mcp.server.fast]
command = "node"
args = ["native-server.js"]
"#;
        let cfg = toml::from_str::<Config>(toml_text).expect("必须解析");
        let slow = cfg.mcp.server.get("slow").expect("slow 必须存在");
        let fast = cfg.mcp.server.get("fast").expect("fast 必须存在");
        assert_eq!(
            slow.handshake_timeout_secs,
            Some(180),
            "slow server 显式填应保留"
        );
        assert!(
            fast.handshake_timeout_secs.is_none(),
            "fast server 未填应为 None(各 server 超时独立,不互相继承)"
        );
    }
}
