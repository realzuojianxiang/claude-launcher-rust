// 启动 Claude Code 模块
// 重构：不再改写 ~/.claude/settings.json，改为把所选供应商的连接参数作为
// 进程环境变量直接注入 claude 进程。每个启动自带环境，互不干扰，天然并发安全、
// 支持任意 provider，且彻底消除 settings.json 的备份/还原竞争。

use crate::Config;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// 同一个启动器进程内可能同时启动多个 Claude 窗口。串行化 settings.json 的
// 读-改-写，避免两个启动请求各自基于旧快照写回，覆盖插件刚保留的配置字段。
static PROVIDER_CONFIG_LOCK: Mutex<()> = Mutex::new(());

// 在 PATH 中查找 claude 命令
// Windows 上 npm 装的 claude 通常是 claude.cmd 批处理（而非 .exe）；
// 优先取 .cmd/.exe 带扩展名的形态写进启动 bat，避免裸 "claude" 在 cmd 里
// 因无扩展名 shim 解析失败而报"文件名、目录名或卷标语法不正确"。
fn find_claude() -> Option<String> {
    // Windows 优先：claude.cmd（npm shim 标准）-> claude.exe
    if cfg!(target_os = "windows") {
        if which::which("claude.cmd").is_ok() {
            return Some("claude.cmd".to_string());
        }
        if which::which("claude.exe").is_ok() {
            return Some("claude.exe".to_string());
        }
    }
    // 兜底：无扩展名的 claude（非 Windows 或 PATH 未提供 .cmd/.exe 时）
    if which::which("claude").is_ok() {
        return Some("claude".to_string());
    }
    None
}

// 临时目录下生成启动批处理文件名（每次启动唯一，避免并发互相覆盖）
fn launcher_bat() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("claude_launcher_{nanos}_{pid}.bat"))
}

// 以 GBK 编码写入 bat 文件（中文 Windows cmd 默认 OEM 代码页 936/GBK）。
// 关键：cmd 解析 bat 时按字节读，UTF-8 中文路径在此会成乱码导致 cd 失败。
// 失败信息保留原写入路径便于排查。
fn write_bat_gbk(path: &PathBuf, content: &str) -> Result<(), String> {
    let (encoded, _, _) = encoding_rs::GBK.encode(content);
    fs::write(path, encoded).map_err(|e| format!("写入 bat 失败 {path:?}: {e}"))
}

// 校验工作目录：必须是真实存在的绝对路径目录，且路径字符集不含 cmd 元字符。
// 这里的 .bat 用 `cd /d "{work}"` 插值，cmd 引号不能完全防住 ` & | < > ^ ( ) %`
// 等元字符（它们在双引号内/边界仍可能改变命令边界）。与其在 .bat 里转义，
// 不如在入口就把不可信来源（history.json / webview invoke）挡在门外，使 cd 的输入恒可信。
// 设计时只允许「目录真实存在」的路径：`select_directory` 走原生目录对话框选出来的路径必然通过；
// 手贴/历史里残留的注入串（含元字符或指向不存在路径）会被拒绝。
pub fn validate_work_dir(dir: &str) -> Result<PathBuf, String> {
    let dir = dir.trim();
    if dir.is_empty() {
        return Err("工作目录为空".to_string());
    }
    let path = Path::new(dir);
    if !path.is_absolute() {
        return Err(format!("工作目录必须是绝对路径: {dir}"));
    }
    // 拒绝 cmd 元字符：即便在双引号内，`& | < > ^ ( ) %` 也可能切断命令边界造成注入。
    // canonicalize 已解析软链并消除 `..`，这里显式挡 `..` 双保险（防止 canonicalize 失败的边界）。
    if dir.contains("..") {
        return Err(format!("工作目录不允许包含 .. : {dir}"));
    }
    for ch in ['&', '|', '<', '>', '^', '(', ')', '%', '`'] {
        if dir.contains(ch) {
            return Err(format!("工作目录含非法字符 `{ch}`: {dir}"));
        }
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("工作目录不存在或不可访问: {dir} ({e})"))?;
    if !canonical.is_dir() {
        return Err(format!("工作目录不是目录: {dir}"));
    }
    // 去掉 Windows 的 `\\?\` 扩展长度前缀（如 `\\?\D:\BaiduSyncdisk\...`）。
    // 该前缀会被 cmd.exe 当作 UNC 路径，导致后续 .bat 里的 `cd /d "{work}"` 报
    // “CMD 不支持将 UNC 路径作为当前目录”。非 Windows 路径不含此前缀，strip 为 no-op。
    let canonical = {
        let s = canonical.to_string_lossy();
        let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            format!("\\\\{rest}")
        } else if let Some(rest) = s.strip_prefix(r"\\?\") {
            rest.to_string()
        } else {
            s.into_owned()
        };
        PathBuf::from(stripped)
    };
    Ok(canonical)
}

fn build_launch_batch(work: &str, claude_cmd: &str, yolo: bool) -> String {
    let yolo_flag = if yolo {
        " --dangerously-skip-permissions"
    } else {
        ""
    };
    format!(
        "@echo off\r\n\
echo Starting Claude Code...\r\n\
echo Provider env injected via process environment (no settings.json modified).\r\n\
echo YOLO Mode: {yolo}\r\n\
echo ANTHROPIC_BASE_URL=%ANTHROPIC_BASE_URL%\r\n\
echo ANTHROPIC_MODEL=%ANTHROPIC_MODEL%\r\n\
echo ANTHROPIC_SMALL_FAST_MODEL=%ANTHROPIC_SMALL_FAST_MODEL%\r\n\
echo CLAUDE_CONFIG_DIR=%CLAUDE_CONFIG_DIR%\r\n\
if defined ANTHROPIC_AUTH_TOKEN (echo AUTH_MODE=token) else (echo AUTH_MODE=api_key)\r\n\
echo.\r\n\
cd /d \"{work}\"\r\n\
{claude_cmd}{yolo_flag}\r\n\
echo.\r\n\
echo Claude Code exited. Provider configuration retained for future launches.\r\n\
pause\r\n\
del \"%~f0\"\r\n"
    )
}

// 我们管理的 canonical Claude 环境变量：注入 provider 前先全部清除，
// 避免上一次/另一个 provider 的残留变量串台（例如 CLIProxyAPI 的 API_KEY
// 泄漏到讯飞，或讯飞的 MODEL 残留到其它 provider）。
fn canonical_claude_env() -> [&'static str; 12] {
    [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        "API_TIMEOUT_MS",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
        "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        "CLAUDE_CONFIG_DIR",
    ]
}

// 「允许注入 Claude 的环境变量键」判定：settings.json 写入与运行时 env 注入两处共用同一真源。
// 语义：只接受 Anthropic/Claude 专有前缀，外加 Claude Code 明确读取的若干「无前缀」变量
// （canonical_claude_env 中的 API_TIMEOUT_MS）。既阻止配置页里随手填的
// PATH/COMSPEC/SYSTEMROOT 等 hijack spawned shell，又与 Claude Code 实际可识别的键集对齐。
// 两处用同一函数，确保「写进 settings.json 的 env 段」与「注入进程环境的 env」永不漂移。
fn is_allowed_env_key(key: &str) -> bool {
    if key.starts_with("ANTHROPIC_") || key.starts_with("CLAUDE_") {
        return true;
    }
    canonical_claude_env()
        .iter()
        .any(|c| c.eq_ignore_ascii_case(key))
}

fn stable_provider_hash(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

// 每个 provider 使用固定、持久的配置目录：同一地址跨启动复用插件/MCP/hooks，
// 不同地址通过 host 前缀 + 稳定哈希隔离。哈希纳入完整 base URL，避免同 host 不同端口
// （例如 127.0.0.1:8082 与 127.0.0.1:8317）发生目录碰撞。
fn persistent_provider_dir_in(base_dir: &Path, base_url: Option<&str>) -> PathBuf {
    let identity = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("provider")
        .trim_end_matches('/');
    // 取 base_url 的 host 部分作为目录名前缀，便于人工排查时一眼看出 provider 归属。
    let raw_host = match base_url {
        Some(url) => {
            let u = url.trim();
            let after_scheme = u.split("://").nth(1).unwrap_or(u);
            after_scheme.split(['/', ':']).next().unwrap_or("")
        }
        None => "",
    };
    // 仅保留 DNS host 合法字符，避免路径穿越/非法文件名
    let mut slug: String = raw_host
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
        .take(32)
        .collect();
    if slug.is_empty() {
        slug = "provider".to_string();
    }

    let provider_hash = stable_provider_hash(identity);
    base_dir
        .join("claude-profiles")
        .join(format!("{slug}__{provider_hash:016x}"))
}

#[cfg(test)]
fn persistent_provider_dir(base_url: Option<&str>) -> PathBuf {
    persistent_provider_dir_in(&Config::config_dir(), base_url)
}

// 原子写入 JSON：序列化到同目录临时文件 -> flush -> rename 覆盖目标。
// 进程被杀、磁盘写满、同步中断都至多留下一个 .tmp 残骸，目标文件要么是旧版要么是完整新版，
// 不会半写损坏导致下一个 claude 进程读到空 env 段。目标与临时同目录保证 rename 是原子操作。
fn atomic_write_json(path: PathBuf, value: &serde_json::Value) -> Result<(), std::io::Error> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.json");
    let tmp = path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nanos));
    let bytes = serde_json::to_vec_pretty(value)?;
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let _ = f.as_raw_fd();
            // best-effort fsync；不支持时忽略
            let _ = f.sync_all();
        }
        #[cfg(not(unix))]
        {
            let _ = f.sync_all();
        }
    }
    if let Err(error) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

fn prepare_provider_config_in(
    base_dir: &Path,
    profile_env: &HashMap<String, String>,
) -> Result<PathBuf, String> {
    let _config_guard = PROVIDER_CONFIG_LOCK
        .lock()
        .map_err(|_| "provider 配置写入锁已损坏".to_string())?;
    let dir = persistent_provider_dir_in(
        base_dir,
        profile_env.get("ANTHROPIC_BASE_URL").map(String::as_str),
    );
    fs::create_dir_all(&dir).map_err(|e| format!("创建 provider 配置目录失败: {e}"))?;

    // 合并更新 env，保留 Claude Code/插件写入 settings.json 的其它字段。
    // 先清掉旧的 provider 连接变量，再写入当前 profile，防止改 Key/模型后残留串台。
    let settings_path = dir.join("settings.json");
    let settings_existed = settings_path.exists();
    let mut settings = if settings_existed {
        let bytes = fs::read(&settings_path)
            .map_err(|e| format!("读取 provider settings.json 失败: {e}"))?;
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|e| format!("解析 provider settings.json 失败: {e}"))?
    } else {
        json!({})
    };
    let original_settings = settings.clone();
    if !settings.is_object() {
        return Err("provider settings.json 顶层必须是 JSON 对象".to_string());
    }
    let settings_obj = settings.as_object_mut().unwrap();
    let mut env_section = settings_obj
        .remove("env")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    env_section.retain(|key, _| {
        let upper = key.to_ascii_uppercase();
        !upper.starts_with("ANTHROPIC_") && !upper.starts_with("CLAUDE_")
    });
    for (key, value) in profile_env
        .iter()
        .filter(|(key, _)| key.starts_with("ANTHROPIC_") || key.starts_with("CLAUDE_"))
    {
        env_section.insert(key.clone(), json!(value));
    }
    settings_obj.insert("env".to_string(), serde_json::Value::Object(env_section));
    // profile 未变化时不触碰文件，缩短与 Claude Code/插件自身写配置的竞争窗口。
    if !settings_existed || settings != original_settings {
        atomic_write_json(settings_path, &settings)
            .map_err(|e| format!("写入 provider settings.json 失败: {e}"))?;
    }

    // 仅首次创建 onboarding 标记；后续绝不覆盖 Claude Code 已写入的持久状态。
    let onboarding = serde_json::to_vec_pretty(&json!({ "hasCompletedOnboarding": true }))
        .map_err(|e| format!("生成 onboarding 配置失败: {e}"))?;
    for file_name in ["claude.json", ".claude.json"] {
        let path = dir.join(file_name);
        if !path.exists() {
            fs::write(path, &onboarding)
                .map_err(|e| format!("写入 provider onboarding 配置失败: {e}"))?;
        }
    }

    Ok(dir)
}

// 启动 Claude Code：返回给前端的状态字符串（对齐 golang 版的 emoji + 文案）
// - yolo: 是否加 --dangerously-skip-permissions
// - profile_env: 所选供应商配置集的环境变量，注入到 claude 进程
pub fn launch(
    config: &Config,
    yolo: bool,
    profile_env: &HashMap<String, String>,
) -> Result<String, String> {
    if config.work_dir.is_empty() {
        return Err("请先选择工作目录".to_string());
    }
    // 入口校验工作目录：拒绝 cmd 元字符 / 不存在路径 / 非绝对路径，
    // 使下方 `cd /d "{work}"` 的输入恒可信（见 validate_work_dir）。
    let work_dir = validate_work_dir(&config.work_dir)?;
    // 已校验为绝对、存在、无 cmd 元字符；validate_work_dir 已规范化并去掉 `\\?\`
    // 前缀（否则 cmd 的 `cd /d` 会把 `\\?\C:\...` 当 UNC 拒绝），用干净路径生成 .bat。
    let work = work_dir.to_string_lossy();

    let claude_cmd = find_claude()
        .ok_or_else(|| "未找到 claude 命令，请确保 Claude Code 已安装并添加到 PATH".to_string())?;

    // 1. 组装进程环境变量：以当前进程环境为基底，但**过滤掉**我们管理的 canonical 变量
    //    （大小写不敏感匹配），避免从启动器/系统环境继承来的 ANTHROPIC_API_KEY 等残留
    //    串台——这正是用讯飞（只设 AUTH_TOKEN）却仍报"both AUTH_TOKEN and API_KEY set"的根因。
    //    随后应用所选 provider 的 env，再叠加全局 auto-compact 变量。
    // 检测隔离配置目录哨兵（NVIDIA 代理等第三方端点使用）：
    // 全局 ~/.claude/settings.json 的 env 段优先级高于进程环境变量，会覆盖我们注入的
    // ANTHROPIC_BASE_URL；OAuth 登录态也会强制走官方网关。改用 CLAUDE_CONFIG_DIR 指向
    // 独立目录后，claude 完全不读全局 settings.json 与 OAuth 凭证，只认本目录配置。
    // 所有 provider 统一走隔离启动：把连接参数写进独立的 CLAUDE_CONFIG_DIR，
    // 彻底不读全局 ~/.claude/settings.json，避免任何 provider 被全局 env（如残留的
    // ANTHROPIC_API_KEY / 错误网关）串台——讯飞此前报 "Both ... set / ConnectionRefused"
    // 正是全局 settings.json 覆盖所致。NVIDIA/讯飞/GLM/CLIProxyAPI… 全部一致隔离。
    let isolate = true;
    let mut profile_env = profile_env.clone();
    profile_env.remove("__nvidia_isolate__");

    // 单鉴权模式：Claude Code 不允许 ANTHROPIC_AUTH_TOKEN 与 ANTHROPIC_API_KEY 同时出现，
    // 否则报 "Both ... set · auth may not work as expected" 并连接失败（ConnectionRefused）。
    // provider 一旦用 token 鉴权，就清掉可能从系统环境/其它 provider 残留或误配的 API_KEY；
    // 反之若只用 API_KEY，也清掉可能误配的 AUTH_TOKEN。二者都在时以 token 为准。
    if profile_env.contains_key("ANTHROPIC_AUTH_TOKEN") {
        profile_env.remove("ANTHROPIC_API_KEY");
    } else if profile_env.contains_key("ANTHROPIC_API_KEY") {
        profile_env.remove("ANTHROPIC_AUTH_TOKEN");
    }

    // 准备持久 Provider 目录：合并 provider env，并预置首次 onboarding 标记。
    // Claude 不读取全局 ~/.claude/settings.json，从而避开其中残留的 ANTHROPIC_API_KEY /
    // 错误网关；同一 Provider 的插件、MCP 和 hooks 则保存在本目录供后续启动复用。
    //
    // 【持久 Provider 隔离】同一 provider 复用固定目录，让插件/MCP/hooks 跨启动保留；
    // 不同 base URL 使用不同目录，避免连接参数串台。settings.json 采用保留其它字段的
    // 原子合并更新，防止覆盖 Claude Code/插件写入的配置。
    let mut provider_config_dir: Option<PathBuf> = None;
    if isolate {
        provider_config_dir = Some(prepare_provider_config_in(
            &Config::config_dir(),
            &profile_env,
        )?);
    }

    let mut envs: HashMap<String, String> = HashMap::new();
    for (k, v) in std::env::vars() {
        if canonical_claude_env()
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&k))
        {
            continue; // 丢弃继承来的 canonical 变量，稍后仅以 profile 为准重新注入
        }
        envs.insert(k, v);
    }
    // 仅注入「Claude 相关」env：与 settings.json 写入用同一白名单 is_allowed_env_key，
    // 阻止配置页里随手填的 PATH/COMSPEC/SYSTEMROOT 等 hijack spawned shell，
    // 同时保留 API_TIMEOUT_MS 这类无前缀但 Claude Code 实际读取的键。
    for (k, v) in profile_env
        .iter()
        .filter(|(key, _)| is_allowed_env_key(key))
    {
        envs.insert(k.clone(), v.clone());
    }

    if let Some(ref dir) = provider_config_dir {
        envs.insert(
            "CLAUDE_CONFIG_DIR".to_string(),
            dir.to_string_lossy().to_string(),
        );
    }
    if config.compact_pct > 0 {
        if config.compact_window > 0 {
            envs.insert(
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW".to_string(),
                config.compact_window.to_string(),
            );
        }
        envs.insert(
            "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE".to_string(),
            config.compact_pct.to_string(),
        );
    }

    // 2. 拼装启动批处理：保留 provider 配置目录供后续启动复用，仅删除临时批处理。
    //    work 已经过 validate_work_dir 校验（绝对、存在、无 cmd 元字符）。
    let batch = build_launch_batch(&work, &claude_cmd, yolo);

    let batch_file = launcher_bat();
    write_bat_gbk(&batch_file, &batch).map_err(|e| format!("创建启动脚本失败: {e}"))?;

    // 3. 在新终端窗口中启动：cmd /c start 开新窗口 -> cmd /c 跑 bat。
    //    环境变量设置在外层 cmd 上，会被 start 派生的子进程继承到 claude。
    //    内层用 /c：pause 结束后关闭终端，避免 secret-bearing 环境在空闲 shell 中残留。
    let child = Command::new("cmd")
        .args([
            "/c",
            "start",
            "Claude Code",
            "cmd",
            "/c",
            &batch_file.to_string_lossy(),
        ])
        .env_clear() // 以 envs 为唯一环境，彻底杜绝继承泄漏（如系统里的 ANTHROPIC_API_KEY）
        .envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .spawn();

    if let Err(e) = child {
        // 启动失败，清理临时 bat
        let _ = fs::remove_file(&batch_file);
        return Err(format!("启动失败: {e}"));
    }

    Ok("✅ Claude Code 已启动（连接参数通过环境变量注入，未改动 settings.json）".to_string())
}

#[cfg(test)]
mod isolation_tests {
    use super::{build_launch_batch, persistent_provider_dir, prepare_provider_config_in};
    use crate::claude::validate_work_dir;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            Self(std::env::temp_dir().join(format!(
                "claude-launcher-provider-isolation-{}-{nanos}",
                std::process::id()
            )))
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn provider_env(base_url: &str, token: &str) -> HashMap<String, String> {
        HashMap::from([
            ("ANTHROPIC_BASE_URL".to_string(), base_url.to_string()),
            ("ANTHROPIC_AUTH_TOKEN".to_string(), token.to_string()),
        ])
    }

    fn read_settings_env(dir: &Path) -> Value {
        let bytes = fs::read(dir.join("settings.json")).expect("应写出 settings.json");
        serde_json::from_slice::<Value>(&bytes)
            .expect("settings.json 应为合法 JSON")
            .get("env")
            .cloned()
            .expect("settings.json 应包含 env")
    }

    // 同一 provider 必须复用固定配置目录，插件/MCP/hooks 才能跨启动保留。
    #[test]
    fn consecutive_launches_reuse_the_same_provider_dir() {
        let d1 = persistent_provider_dir(Some("http://localhost:8317"));
        let d2 = persistent_provider_dir(Some("http://localhost:8317"));
        assert_eq!(d1, d2, "同一 provider 连续启动必须复用配置目录");
        assert!(
            d1.to_string_lossy().contains("claude-profiles"),
            "持久目录应位于 claude-profiles/ 之下"
        );
    }

    // 不同 provider 的目录前缀应反映各自 host，便于人工排查归属。
    #[test]
    fn isolated_dir_prefix_reflects_provider_host() {
        let d_cliproxy = persistent_provider_dir(Some("http://localhost:8317"));
        // host 较短时整个 host 进入 slug（含点号，证明 host 合法字符被保留）
        let d_short = persistent_provider_dir(Some("http://my-proxy.local:9000"));
        assert!(d_cliproxy.to_string_lossy().contains("localhost"));
        assert!(d_short.to_string_lossy().contains("my-proxy.local"));
    }

    // base_url 缺失时回退到 provider 占位前缀，不应 panic。
    #[test]
    fn missing_base_url_falls_back_to_provider_slug() {
        let d = persistent_provider_dir(None);
        assert!(d.to_string_lossy().contains("provider"));
    }

    // 两个 provider 的 settings 必须分别落到独立目录，不能互相覆盖。
    #[test]
    fn two_providers_write_independent_settings() {
        let root = TestDir::new();
        let first = provider_env("http://provider-one.test", "token-one");
        let second = provider_env("http://provider-two.test", "token-two");

        let first_dir =
            prepare_provider_config_in(&root.0, &first).expect("第一个 provider 应写入成功");
        let second_dir =
            prepare_provider_config_in(&root.0, &second).expect("第二个 provider 应写入成功");

        assert_ne!(first_dir, second_dir, "两个 provider 必须使用不同隔离目录");

        let first_env = read_settings_env(&first_dir);
        let second_env = read_settings_env(&second_dir);
        assert_eq!(first_env["ANTHROPIC_BASE_URL"], "http://provider-one.test");
        assert_eq!(first_env["ANTHROPIC_AUTH_TOKEN"], "token-one");
        assert_eq!(second_env["ANTHROPIC_BASE_URL"], "http://provider-two.test");
        assert_eq!(second_env["ANTHROPIC_AUTH_TOKEN"], "token-two");

        // 第二次写入后重读第一份，确保没有被覆盖。
        assert_eq!(
            read_settings_env(&first_dir)["ANTHROPIC_AUTH_TOKEN"],
            "token-one"
        );
    }

    #[test]
    fn repeated_launch_preserves_provider_plugins_and_non_env_settings() {
        let root = TestDir::new();
        let first = provider_env("http://provider-one.test:8082", "token-one");
        let dir =
            prepare_provider_config_in(&root.0, &first).expect("首次启动应创建 provider 目录");

        fs::create_dir_all(dir.join("plugins")).unwrap();
        fs::write(dir.join("plugins").join("installed.marker"), "installed").unwrap();

        let mut settings: Value =
            serde_json::from_slice(&fs::read(dir.join("settings.json")).unwrap()).unwrap();
        settings["enabledPlugins"] = json!({ "example-plugin": true });
        fs::write(
            dir.join("settings.json"),
            serde_json::to_vec_pretty(&settings).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join(".claude.json"),
            serde_json::to_vec_pretty(&json!({
                "hasCompletedOnboarding": true,
                "persistedState": "keep-me"
            }))
            .unwrap(),
        )
        .unwrap();

        let second = provider_env("http://provider-one.test:8082", "token-two");
        let second_dir =
            prepare_provider_config_in(&root.0, &second).expect("再次启动应复用 provider 目录");

        assert_eq!(second_dir, dir);
        assert_eq!(
            fs::read_to_string(dir.join("plugins").join("installed.marker")).unwrap(),
            "installed"
        );
        let updated: Value =
            serde_json::from_slice(&fs::read(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(updated["enabledPlugins"]["example-plugin"], true);
        assert_eq!(updated["env"]["ANTHROPIC_AUTH_TOKEN"], "token-two");

        let claude_state: Value =
            serde_json::from_slice(&fs::read(dir.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(claude_state["persistedState"], "keep-me");
    }

    #[test]
    fn launch_batch_keeps_provider_config_after_claude_exits() {
        let batch = build_launch_batch(r"D:\work", "claude.cmd", false);
        let claude = batch.find("claude.cmd").expect("批处理应启动 Claude");
        let remove_script = batch.find(r#"del "%~f0""#).expect("批处理结束时应删除自身");

        assert!(!batch.contains(r#"rmdir /s /q "%CLAUDE_CONFIG_DIR%""#));
        assert!(batch.contains("Provider configuration retained for future launches."));
        assert!(claude < remove_script, "只能在 Claude 退出后删除临时批处理");
    }

    #[test]
    fn concurrent_launches_prepare_one_valid_shared_provider_config() {
        let root = TestDir::new();
        let initial = provider_env("http://shared-provider.test:8082", "shared-token");
        let initial_dir =
            prepare_provider_config_in(&root.0, &initial).expect("应先创建共享 provider 配置");
        let mut initial_settings: Value =
            serde_json::from_slice(&fs::read(initial_dir.join("settings.json")).unwrap()).unwrap();
        initial_settings["enabledPlugins"] = json!({ "shared-plugin": true });
        fs::write(
            initial_dir.join("settings.json"),
            serde_json::to_vec_pretty(&initial_settings).unwrap(),
        )
        .unwrap();

        let root_path = Arc::new(root.0.clone());
        let workers = 12;
        let barrier = Arc::new(Barrier::new(workers));
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let root_path = root_path.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    let env = provider_env("http://shared-provider.test:8082", "shared-token");
                    barrier.wait();
                    prepare_provider_config_in(&root_path, &env)
                })
            })
            .collect();

        let dirs: Vec<PathBuf> = handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .expect("并发配置线程不应 panic")
                    .expect("并发启动不应争用 settings.json 临时文件")
            })
            .collect();

        assert!(dirs.windows(2).all(|pair| pair[0] == pair[1]));
        let settings: Value =
            serde_json::from_slice(&fs::read(dirs[0].join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "shared-token");
        assert_eq!(settings["enabledPlugins"]["shared-plugin"], true);
    }

    // 回归测试：validate_work_dir 必须剥离 Windows 的 `\\?\` 扩展长度前缀。
    // 否则 .bat 里的 `cd /d "{work}"` 会被 cmd 当作 UNC 路径拒绝，报错
    // “CMD 不支持将 UNC 路径作为当前目录”。
    #[test]
    fn validate_work_dir_strips_verbatim_prefix() {
        let dir = TestDir::new();
        fs::create_dir_all(&dir.0).expect("应创建临时目录");
        let p = dir.0.to_string_lossy().to_string();
        let got = validate_work_dir(&p).expect("存在的绝对目录应通过校验");
        let s = got.to_string_lossy();
        assert!(
            !s.starts_with("\\\\?\\"),
            "校验后的工作目录不应带 Windows `\\\\?\\` 前缀，实际: {s}"
        );
        assert!(got.is_dir(), "校验后的工作目录仍应是目录");
    }

    // 回归测试：不存在的目录必须返回 Err（canonicalize 失败路径保留）。
    #[test]
    fn validate_work_dir_rejects_missing_dir() {
        let missing = std::env::temp_dir().join(format!(
            "claude-launcher-missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let p = missing.to_string_lossy().to_string();
        assert!(validate_work_dir(&p).is_err(), "不存在的目录应被拒绝");
    }
}
