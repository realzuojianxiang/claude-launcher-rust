// CLIProxyAPI 进程管理模块（仅 Windows）
// 对应 golang 版 StartCLIProxyAPI / StopCLIProxyAPI / CLIProxyAPIStatus
// 启动靠 cmd /c start 开新窗口；停止靠 taskkill /IM cliproxyapi.exe /F；
// 状态用 2 秒超时的 HTTP GET /v1/models 判断——只要能连上（即便 401）即视为运行中

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

// CLIProxyAPI 可执行文件的探测路径（对齐 golang 版硬编码，大小写两份）
const PROBE_PATHS: [&str; 2] = [
    "d:\\BaiduSyncdisk\\ai-agent\\CLIProxyAPI\\cliproxyapi.exe",
    "D:\\BaiduSyncdisk\\ai-agent\\CLIProxyAPI\\cliproxyapi.exe",
];

// 查找 cliproxyapi 可执行文件：先试探测路径，再回退 PATH 查找
fn find_proxy_exe() -> Option<String> {
    for p in PROBE_PATHS {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    if which::which("cliproxyapi.exe").is_ok() {
        return Some("cliproxyapi.exe".to_string());
    }
    if which::which("cliproxyapi").is_ok() {
        return Some("cliproxyapi".to_string());
    }
    None
}

// 推断代理进程的工作目录：绝对路径取其父目录，PATH 命名取当前目录
fn proxy_work_dir(exe_path: &str) -> PathBuf {
    let p = std::path::Path::new(exe_path);
    if p.is_absolute() {
        p.parent().map(|x| x.to_path_buf()).unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        })
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

// 状态查询：HTTP GET {url}/v1/models，2 秒超时，连上即运行中
// 返回扁平 JSON 供前端展示，字段对齐 golang 版（running/url/status_code/message/error）
pub fn status(url: &str) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    result.insert("running".into(), serde_json::Value::Bool(false));
    result.insert(
        "url".into(),
        serde_json::Value::String(url.to_string()),
    );

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            result.insert("error".into(), serde_json::Value::String(e.to_string()));
            return serde_json::Value::Object(result);
        }
    };

    let target = format!("{}/v1/models", url.trim_end_matches('/'));
    match client.get(&target).send() {
        Err(e) => {
            result.insert("error".into(), serde_json::Value::String(e.to_string()));
            serde_json::Value::Object(result)
        }
        Ok(resp) => {
            // 只要能连上即认为在运行（即便 401）
            let code = resp.status().as_u16();
            result.insert("running".into(), serde_json::Value::Bool(true));
            result.insert(
                "status_code".into(),
                serde_json::Value::Number(code.into()),
            );
            if resp.status().as_u16() == 401 {
                result.insert(
                    "message".into(),
                    serde_json::Value::String("服务运行中，需要 API key".to_string()),
                );
            }
            serde_json::Value::Object(result)
        }
    }
}

// 启动 CLIProxyAPI：已在运行则跳过；找到 exe 后开新终端窗口，等待 2 秒再探测
pub fn start(base_url: &str) -> Result<String, String> {
    // 先检查是否已经在运行
    if status(base_url).get("running") == Some(&serde_json::Value::Bool(true)) {
        return Ok("⚠️ CLIProxyAPI 已经在运行中".to_string());
    }

    let exe_path = find_proxy_exe()
        .ok_or_else(|| "❌ 未找到 cliproxyapi.exe，请检查安装路径".to_string())?;

    let work_dir = proxy_work_dir(&exe_path);

    // cmd /c start "CLIProxyAPI" cmd /k <exe>，在新终端窗口前台运行
    let mut cmd = Command::new("cmd");
    cmd.args([
        "/c",
        "start",
        "CLIProxyAPI",
        "cmd",
        "/k",
        &exe_path,
    ])
    .current_dir(&work_dir);

    cmd.spawn().map_err(|e| format!("❌ 启动失败: {e}"))?;

    // 等待并复探状态
    std::thread::sleep(Duration::from_secs(2));
    if status(base_url).get("running") == Some(&serde_json::Value::Bool(true)) {
        return Ok("✅ CLIProxyAPI 已启动".to_string());
    }
    Ok("⚠️ 启动命令已执行，但服务可能未成功启动，请检查终端窗口".to_string())
}

// 停止 CLIProxyAPI：taskkill /IM cliproxyapi.exe /F
pub fn stop() -> Result<String, String> {
    let output = Command::new("taskkill")
        .args(["/IM", "cliproxyapi.exe", "/F"])
        .output()
        .map_err(|e| format!("❌ 停止失败: {e}"))?;
    if output.status.success() {
        return Ok("✅ CLIProxyAPI 已停止".to_string());
    }
    // 进程不存在的情形：检查输出是否含相关提示
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    if text.contains("not found") || text.contains("找不到") {
        return Ok("⚠️ CLIProxyAPI 未在运行".to_string());
    }
    Err(format!("❌ 停止失败: {text}"))
}
