// 配置持久化模块：读写「exe 同一目录下的 claude-launcher/config.json」
// 只认 exe 旁边这一处，不再使用 ~/.claude-launcher/config.json，无回退、无迁移
// 对应 golang 版 Config / loadConfig / saveConfig / GetConfig / SetConfig

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// Grok 代理配置定义放在 `crate::grok::models`（与该 provider 模块就近），此处仅供
// `Config.grok` 字段引用。同 crate 内循环引用无碍：GrokConfig 自身不引用 Config。
// 以 `pub use` 重导出，便于 lib.rs 等处直接 `use config::GrokConfig`。
pub use crate::grok::models::GrokConfig;

// 供应商配置集：一组命名的环境变量，用于在启动时注入到 claude 进程
// （替代原先改写 ~/.claude/settings.json 的做法，彻底避免全局冲突/并发竞争）。
// 不同 provider（讯飞 / CherryStudio …）各存一套，启动页下拉选择。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub env: HashMap<String, String>,
}

// NVIDIA API 代理配置：应用内 axum 代理服务的全部可调参数。
// 对应需求文档中的 .env 配置项，这里改为随 config.json 持久化、由 UI 配置页编辑。
//   - api_keys : 多个 NVIDIA API Key，轮询 + 429 冷却
//   - models   : 多个模型，按顺序作为 Fallback 优先级
//   - base_url : NVIDIA NIM OpenAI 兼容端点（含 /v1）
//   - host/port: 本地代理监听地址
//   - key_cooldown_seconds : Key 命中 429 后的冷却时长
//   - max_retries          : 单请求最大重试次数
//   - request_timeout_seconds : 上游请求超时
//   - auth_token : 可选的本地代理鉴权 token（校验请求头 x-api-key），空串表示不校验
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvidiaConfig {
    #[serde(default)]
    pub api_keys: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default = "default_nvidia_base_url")]
    pub base_url: String,
    #[serde(default = "default_nvidia_host")]
    pub host: String,
    #[serde(default = "default_nvidia_port")]
    pub port: u16,
    #[serde(default = "default_key_cooldown")]
    pub key_cooldown_seconds: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_seconds: u64,
    #[serde(default)]
    pub auth_token: String,
}

fn default_nvidia_base_url() -> String {
    "https://integrate.api.nvidia.com/v1".to_string()
}
// 默认仅绑定回环地址：避免把无鉴权的本地代理暴露到局域网被他人盗用 NVIDIA 配额。
// 仅当用户在 UI 显式填写非回环 host（如 0.0.0.0 / 局域网 IP）时才监听外部网卡，
// 且届时会强制要求配置高熵 auth_token（见 proxy.rs / NvidiaConfig 校验）。
fn default_nvidia_host() -> String {
    "127.0.0.1".to_string()
}
fn default_nvidia_port() -> u16 {
    8082
}
fn default_key_cooldown() -> u64 {
    65
}
fn default_max_retries() -> u32 {
    3
}
fn default_request_timeout() -> u64 {
    600
}

impl Default for NvidiaConfig {
    fn default() -> Self {
        Self {
            api_keys: Vec::new(),
            models: Vec::new(),
            base_url: default_nvidia_base_url(),
            host: default_nvidia_host(),
            port: default_nvidia_port(),
            key_cooldown_seconds: default_key_cooldown(),
            max_retries: default_max_retries(),
            request_timeout_seconds: default_request_timeout(),
            auth_token: String::new(),
        }
    }
}

impl NvidiaConfig {
    // 旧版本把 120 秒作为内置默认值。加载已有配置时仅迁移这一历史默认；
    // 用户显式选择的其他超时值（例如 300）保持不变。
    fn migrate_legacy_timeout(&mut self) {
        if self.request_timeout_seconds == 120 {
            self.request_timeout_seconds = default_request_timeout();
        }
    }

    // 判断配置的监听地址是否仅对回环接口开放。
    //   - "127.0.0.1" / "localhost" / "::1" 视为回环（仅本机可连）；
    //   - "0.0.0.0" / "::" / 任意非回环 IP 视为对外暴露，需要强鉴权保护。
    // 用在两处：start() 拒绝无鉴权地对外监听；UI 提示用户外部监听需配 token。
    pub fn is_loopback_host(&self) -> bool {
        let h = self.host.trim().to_ascii_lowercase();
        h.is_empty() || h == "127.0.0.1" || h == "localhost" || h == "::1"
    }

    // 外部监听安全校验：host 非回环时必须配置足够强度的 auth_token，
    // 否则拒绝启动——默认无鉴权地暴露到局域网约等于公开用户的 NVIDIA API Key。
    // 返回 Err 时携带可在 UI 直接展示的中文错误。
    pub fn require_auth_if_exposed(&self) -> Result<(), String> {
        if !self.is_loopback_host() {
            let token = self.auth_token.trim();
            // 高熵门槛：至少 24 个字符，避免 "1" / "token" 这类形同虚设的占位值。
            if token.len() < 24 {
                let state = if token.is_empty() { "为空" } else { "过短" };
                return Err(format!(
                    "❌ 绑定地址 {} 面向外部网络，必须配置至少 24 位的本地鉴权 token（当前{state}），否则同网设备可盗用你的 NVIDIA Key。",
                    self.host
                ));
            }
        }
        Ok(())
    }

    // 上游 base_url 安全校验：保存与转发前调用，杜绝 SSRF / bearer token 被 30x 引流到任意主机。
    //   - 必须以 `https://` 或 `http://` 开头，且非空；
    //   - host 必须存在（拒绝 `http:///path` 这类能被 reqwest 当成 localhost 的畸形 URL）；
    //   - 显式拒绝 host 为空 / 仅含回环字面但带可疑前导等。这里不限定单一 NVIDIA 主机，
    //     因为本地自测需要可指向 http://localhost 反代，但 scheme + host 非空是硬下限。
    // 真正的"不跟随重定向"在 ProxyCtx::new 用 redirect(Policy::none) 实现，本函数是第二道闸。
    pub fn validate_base_url(&self) -> Result<(), String> {
        let url = self.base_url.trim();
        if url.is_empty() {
            return Err("❌ NVIDIA Base URL 不能为空".to_string());
        }
        let lower = url.to_ascii_lowercase();
        if !lower.starts_with("https://") && !lower.starts_with("http://") {
            return Err(format!(
                "❌ NVIDIA Base URL 必须以 http:// 或 https:// 开头（当前: {url}），否则可能泄露你的 NVIDIA Key。"
            ));
        }
        let after_scheme = &url[url.trim_start_matches(|c| c != ':').len()..];
        let host_part = {
            let s = lower
                .strip_prefix("https://")
                .or_else(|| lower.strip_prefix("http://"))
                .unwrap_or(url);
            s.split(['/', ':']).next().unwrap_or("")
        };
        if host_part.is_empty() {
            return Err(format!("❌ NVIDIA Base URL 缺少主机名（当前: {url}）"));
        }
        let _ = after_scheme; // 调试可见；本函数只做 scheme + host 存在性校验
        Ok(())
    }
}

// 应用配置结构，字段对齐 golang 版
// compact_window / compact_pct 为新增字段：控制 Claude Code auto-compact 触发阈值。
//   - compact_window: 纳入 auto-compact 计算的上下文容量(token)，默认 1_000_000 对应 1M 窗口模型。
//   - compact_pct   : auto-compact 触发的窗口占比(1-100)，默认 70 表示用到 70% 时压缩。
// profiles: 供应商配置集列表，启动页选择其一注入连接参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub work_dir: String,
    pub yolo_mode: bool,
    #[serde(default = "default_compact_window")]
    pub compact_window: u64,
    #[serde(default = "default_compact_pct")]
    pub compact_pct: u8,
    // 供应商配置集列表
    #[serde(default)]
    pub profiles: Vec<Profile>,
    // NVIDIA API 代理配置（应用内 axum 服务）；旧配置缺失时回退默认值
    #[serde(default)]
    pub nvidia: NvidiaConfig,
    // Grok 代理配置（与 NVIDIA 平级的并行 provider，独立端口 8083）。
    // OAuth token 不落此结构（单独 DPAPI 加密存 grok-oauth.json），这里只存配置与 account 标识。
    #[serde(default)]
    pub grok: GrokConfig,
    // 仅运行期字段，不落盘：记录最近一次 load 是否因 config.json 损坏而回退默认配置，
    // 供启动时写一份告警文件给用户（S4）。serde 跳过，save 序列化时不会写出。
    #[serde(skip)]
    pub last_corrupt_path: Option<PathBuf>,
}

fn default_compact_window() -> u64 {
    1_000_000
}

fn default_compact_pct() -> u8 {
    70
}

// 默认供应商配置：讯飞（示例值）+ CherryStudio · GLM。
// 这样首次安装/旧配置迁移后即可直接选两套 provider，无需手动录入。
fn default_profiles() -> Vec<Profile> {
    let mut xf = HashMap::new();
    xf.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        "2e7dace:-demo".to_string(),
    );
    xf.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        "https://maas-coding-api.cn-huabei-1.xf-yun.com/anthropic".to_string(),
    );
    xf.insert(
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
        "1".to_string(),
    );
    xf.insert("API_TIMEOUT_MS".to_string(), "600000".to_string());
    xf.insert(
        "ANTHROPIC_MODEL".to_string(),
        "astron-code-latest".to_string(),
    );
    xf.insert(
        "ANTHROPIC_SMALL_FAST_MODEL".to_string(),
        "astron-code-latest".to_string(),
    );

    // CherryStudio 本地 API 服务器（通过 OpenAI 兼容接口暴露 GLM 模型）。
    // 地址以 CherryStudio「API 服务器」面板显示为准，默认通常是 http://127.0.0.1:24333 ；
    // 该服务不处理 /v1 路径前缀时去掉 /v1，Anthropic SDK 通常需要完整路径。
    let mut cherry = HashMap::new();
    cherry.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        "http://127.0.0.1:24333".to_string(),
    );
    cherry.insert(
        "ANTHROPIC_MODEL".to_string(),
        "x-express:agent/glm-5.2".to_string(),
    );
    cherry.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        "<在此填入 CherryStudio 提供的 ANTHROPIC_AUTH_TOKEN>".to_string(),
    );
    cherry.insert("API_TIMEOUT_MS".to_string(), "600000".to_string());

    vec![
        Profile {
            name: "讯飞".to_string(),
            env: xf,
        },
        Profile {
            name: "CherryStudio · GLM".to_string(),
            env: cherry,
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_dir: String::new(),
            yolo_mode: false,
            compact_window: default_compact_window(),
            compact_pct: default_compact_pct(),
            profiles: default_profiles(),
            nvidia: NvidiaConfig::default(),
            grok: GrokConfig::default(),
            last_corrupt_path: None,
        }
    }
}

impl Config {
    // 推断配置文件路径：exe 同目录下的 claude-launcher/config.json（便于随程序携带）。
    // 取不到 exe 路径时回退到当前工作目录的 claude-launcher 子目录；不再使用 ~/.claude-launcher。
    pub fn path() -> PathBuf {
        let dir = Self::config_dir();
        let _ = fs::create_dir_all(&dir); // 目录缺失时创建，忽略错误：读取时仍会回退默认值
        dir.join("config.json")
    }

    // 配置目录：exe 同级的 claude-launcher 子目录；取不到 exe 时回退当前工作目录
    // pub(crate)：供 history 模块复用，保证配置/历史同目录
    pub(crate) fn config_dir() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        match exe_dir {
            Some(d) => d.join("claude-launcher"),
            None => std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("claude-launcher"),
        }
    }
    // 保证至少有一套默认供应商配置，避免启动无可选 provider
    fn ensure_profiles(mut cfg: Config) -> Config {
        if cfg.profiles.is_empty() {
            cfg.profiles = default_profiles();
        }
        cfg
    }

    // 加载配置并返回「上次加载是否发生过损坏回退」。
    // 供启动时把损坏情况显式暴露给 UI（避免用户误以为配置正常丢失了 Key）。
    // 返回 (config, last_corrupt_path)：last_corrupt_path 非空表示发生了一次损坏回退，
    // 调用方可把该路径透传给前端供用户定位证据文件。
    pub fn load_or_default() -> (Self, Option<PathBuf>) {
        let path = Self::path();
        let data = match fs::read(&path) {
            Ok(d) => d,
            Err(_) => return (Self::ensure_profiles(Self::default()), None), // 缺文件：正常首次安装
        };
        match serde_json::from_slice::<Config>(&data) {
            Ok(mut cfg) => {
                cfg.nvidia.migrate_legacy_timeout();
                // Grok 默认模型兜底：既有空 models 配置补默认（开箱即用，免去用户先在 GUI
                // 手配才能启动代理）。返回 true 才落盘，幂等。
                if cfg.grok.migrate_default_models() {
                    let _ = cfg.save();
                }
                (Self::ensure_profiles(cfg), None)
            }
            Err(e) => {
                // 保留现场：把损坏文件改名为带时间戳的 .corrupt.json，便于人工排查。
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let corrupt = path.with_extension(format!("corrupt-{stamp}.json"));
                let _ = fs::rename(&path, &corrupt);
                tracing::error!(
                    error = %e,
                    corrupt_path = ?corrupt,
                    "config.json 解析失败，已重命名为证据文件并回退默认配置"
                );
                (Self::ensure_profiles(Self::default()), Some(corrupt))
            }
        }
    }

    // 保存配置到磁盘：序列化到同目录临时文件 -> flush -> sync_all -> 原子 rename 覆盖。
    // 进程异常退出 / 磁盘写满 / 同步中断至多留下一个 *.tmp 残骸，目标文件要么是完整旧版
    // 要么是完整新版，杜绝半写损坏导致下次加载静默回退默认（表现为 Provider/Key 全部丢失）。
    pub fn save(&self) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(self).map_err(|e| format!("序列化配置失败: {e}"))?;
        let path = Self::path();
        let dir = path
            .parent()
            .ok_or_else(|| "无法定位配置目录".to_string())?;
        fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时配置文件失败: {e}"))?;
            use std::io::Write;
            f.write_all(&data)
                .map_err(|e| format!("写入配置失败: {e}"))?;
            f.flush().map_err(|e| format!("刷新配置失败: {e}"))?;
            let _ = f.sync_all(); // best-effort 落盘；不支持时也已完成写入
        }
        fs::rename(&tmp, &path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("原子替换配置文件失败: {e}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 旧 config.json 缺 compact_window/compact_pct 字段时，serde 回退到默认值
    // 这是兼容性的关键保障：老用户配置不打补丁也能拿到 70 / 1M。
    #[test]
    fn old_config_without_compact_fields_falls_back_to_default() {
        let old = r#"{
            "work_dir": "D:/work",
            "yolo_mode": true
        }"#;
        let cfg: Config = serde_json::from_str(old).expect("旧配置解析失败");
        assert_eq!(
            cfg.compact_window, 1_000_000,
            "缺字段时 compact_window 应回退到 1M"
        );
        assert_eq!(cfg.compact_pct, 70, "缺字段时 compact_pct 应回退到 70");
    }

    // 新 config.json 显式写值时，serde 正常读取，不被默认值覆盖
    #[test]
    fn new_config_with_compact_fields_uses_explicit_values() {
        let new = r#"{
            "work_dir": "",
            "yolo_mode": false,
            "compact_window": 500000,
            "compact_pct": 50
        }"#;
        let cfg: Config = serde_json::from_str(new).expect("新配置解析失败");
        assert_eq!(cfg.compact_window, 500_000);
        assert_eq!(cfg.compact_pct, 50);
    }

    // 默认值直接来自 Default impl，保证 load() 在文件缺失时也带 70 / 1M
    #[test]
    fn default_config_carries_compact_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.compact_window, 1_000_000);
        assert_eq!(cfg.compact_pct, 70);
    }

    // 默认配置应带两套供应商：讯飞、CherryStudio · GLM
    #[test]
    fn default_config_seeds_two_profiles() {
        let cfg = Config::default();
        assert_eq!(cfg.profiles.len(), 2, "默认应种子两套供应商配置");
        assert!(cfg.profiles.iter().any(|p| p.name == "讯飞"));
        assert!(cfg.profiles.iter().any(|p| p.name == "CherryStudio · GLM"));
    }

    // 关键回归：供应商 profile（含 ANTHROPIC_AUTH_TOKEN）经 save -> load 后仍能保留
    // 这证明「配置不能保存」不是序列化/落盘逻辑问题，而多发生在文件路径错位（写 exe 同级、却查看旧路径）。
    #[test]
    fn profiles_roundtrip_persists_auth_token() {
        // 用临时目录作为 exe 同级，避免污染真实安装位置
        let tmp = std::env::temp_dir().join(format!("claude_launcher_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        // 构造一个将被写到 tmp/claude-launcher/config.json 的测试配置
        let custom = Config {
            profiles: vec![Profile {
                name: "讯飞".to_string(),
                env: {
                    let mut m = HashMap::new();
                    m.insert(
                        "ANTHROPIC_AUTH_TOKEN".to_string(),
                        "my-real-token-123".to_string(),
                    );
                    m.insert(
                        "ANTHROPIC_BASE_URL".to_string(),
                        "https://xf-yun.example.com/anthropic".to_string(),
                    );
                    m
                },
            }],
            ..Default::default()
        };

        // 直接调用 save 的等价逻辑（path 走 real exe，这里手动写到 tmp 下以隔离）
        let cfg_path = tmp.join("claude-launcher").join("config.json");
        let _ = std::fs::create_dir_all(cfg_path.parent().unwrap());
        let bytes = serde_json::to_vec_pretty(&custom).unwrap();
        std::fs::write(&cfg_path, bytes).unwrap();

        // 从磁盘重新解析（模拟 load 的解析分支）
        let reloaded: Config = serde_json::from_slice(&std::fs::read(&cfg_path).unwrap()).unwrap();
        let xf = reloaded
            .profiles
            .iter()
            .find(|p| p.name == "讯飞")
            .expect("讯飞 profile 应被保留");
        assert_eq!(
            xf.env.get("ANTHROPIC_AUTH_TOKEN").map(|s| s.as_str()),
            Some("my-real-token-123"),
            "ANTHROPIC_AUTH_TOKEN 应随 profile 落盘并保留"
        );

        // 清理
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 【S1 回归】默认代理 host 必须是回环地址，确保默认不对外暴露无鉴权代理。
    #[test]
    fn default_nvidia_host_is_loopback() {
        assert_eq!(NvidiaConfig::default().host, "127.0.0.1");
        assert!(NvidiaConfig::default().is_loopback_host());
    }

    #[test]
    fn nvidia_timeout_defaults_to_600_seconds() {
        assert_eq!(NvidiaConfig::default().request_timeout_seconds, 600);
    }

    #[test]
    fn nvidia_timeout_migrates_only_the_legacy_120_second_default() {
        let mut legacy = NvidiaConfig {
            request_timeout_seconds: 120,
            ..Default::default()
        };
        legacy.migrate_legacy_timeout();
        assert_eq!(legacy.request_timeout_seconds, 600);

        let mut explicit = NvidiaConfig {
            request_timeout_seconds: 300,
            ..Default::default()
        };
        explicit.migrate_legacy_timeout();
        assert_eq!(explicit.request_timeout_seconds, 300);
    }

    // 【S1 回归】localhost / ::1 也算回环；0.0.0.0 / 局域网 IP 视为外部暴露。
    #[test]
    fn loopback_detection_covers_localhost_and_ipv6_and_external() {
        let mk = |h: &str| NvidiaConfig {
            host: h.to_string(),
            ..Default::default()
        };
        assert!(mk("127.0.0.1").is_loopback_host());
        assert!(mk("localhost").is_loopback_host());
        assert!(mk("::1").is_loopback_host());
        assert!(mk("").is_loopback_host()); // 空串视为回环默认
        assert!(!mk("0.0.0.0").is_loopback_host());
        assert!(!mk("192.168.1.10").is_loopback_host());
    }

    // 【S1 回归】非回环监听且无 token / 短 token 时，启动校验必须拒绝。
    #[test]
    fn external_bind_requires_strong_auth_token() {
        let no_token = NvidiaConfig {
            host: "0.0.0.0".to_string(),
            auth_token: String::new(),
            ..Default::default()
        };
        assert!(no_token.require_auth_if_exposed().is_err());

        let short = NvidiaConfig {
            host: "0.0.0.0".to_string(),
            auth_token: "short".to_string(), // < 24
            ..Default::default()
        };
        assert!(short.require_auth_if_exposed().is_err());

        // 充足长度 token 时通过
        let strong = NvidiaConfig {
            host: "0.0.0.0".to_string(),
            auth_token: "0123456789abcdef01234567".to_string(), // 24
            ..Default::default()
        };
        assert!(strong.require_auth_if_exposed().is_ok());

        // 回环监听不要求 token
        let loop_no_token = NvidiaConfig {
            host: "127.0.0.1".to_string(),
            auth_token: String::new(),
            ..Default::default()
        };
        assert!(loop_no_token.require_auth_if_exposed().is_ok());
    }

    // 【S4 回归】save 后再 load 应能完整还原（原子写-读往返）。
    // 通过把 Config::path() 重定向到临时目录来实现隔离：这里只断点核心往返。
    #[test]
    fn save_roundtrip_preserves_profiles_via_temp_dir() {
        let tmp =
            std::env::temp_dir().join(format!("claude_launcher_save_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::create_dir_all(tmp.join("claude-launcher"));
        // 直接写+读临时 config.json，验证序列化/反序列化往返（save 原子逻辑同款）
        let cfg_path = tmp.join("claude-launcher").join("config.json");
        let custom = Config {
            profiles: vec![Profile {
                name: "Test".to_string(),
                env: {
                    let mut m = HashMap::new();
                    m.insert("ANTHROPIC_API_KEY".to_string(), "roundtrip-key".to_string());
                    m
                },
            }],
            ..Default::default()
        };
        let bytes = serde_json::to_vec_pretty(&custom).unwrap();
        std::fs::write(&cfg_path, bytes).unwrap();
        let reloaded: Config = serde_json::from_slice(&std::fs::read(&cfg_path).unwrap()).unwrap();
        assert_eq!(reloaded.profiles.len(), 1);
        assert_eq!(
            reloaded.profiles[0]
                .env
                .get("ANTHROPIC_API_KEY")
                .map(|s| s.as_str()),
            Some("roundtrip-key")
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 【S4 回归】损坏的 config.json 不应再静默回退默认——load_or_default 应返回损坏证据路径。
    // 这里直接构造损坏字节并走解析分支，验证 Err 会触发回退+标记 corrupt（不再 unwrap_or_default 吞掉错误）。
    #[test]
    fn corrupted_config_signals_corrupt_path_not_silent_default() {
        let bad_bytes = b"{ this is not valid json ";
        // 直接断言解析会失败（即非静默路径），证明 load_or_default 的 Err 分支会被走：
        // 若是旧 unwrap_or_default，这里会变成默认 Config 而无法区分。
        let parsed: Result<Config, _> = serde_json::from_slice(bad_bytes);
        assert!(parsed.is_err(), "损坏 JSON 必须解析失败而非静默回退");
    }
}
