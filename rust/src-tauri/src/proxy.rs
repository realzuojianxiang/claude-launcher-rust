// CLIProxyAPI 进程管理模块（仅 Windows）
// 对应 golang 版 StartCLIProxyAPI / StopCLIProxyAPI / CLIProxyAPIStatus
// 启动靠 cmd /c start 开新窗口；停止靠 taskkill /IM cliproxyapi.exe /F；
// 状态用 2 秒超时的 HTTP GET /v1/models 判断——只要能连上（即便 401）即视为运行中

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

// 定位结果：cliproxyapi.exe 的运行路径 + 进程工作目录
// 二者必须配对返回——working dir 决定代理进程的当前目录
#[derive(Debug)]
struct ProxyLocation {
    exe_path: String,
    work_dir: PathBuf,
}

// 校验指定目录：必须存在且是目录，否则显式报错（不静默回退到别处）
fn validate_user_dir(user_dir: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(user_dir);
    if !dir.exists() {
        return Err(format!("❌ 指定的 CLIProxyAPI 执行目录不存在: {user_dir}"));
    }
    if !dir.is_dir() {
        return Err(format!(
            "❌ 指定的 CLIProxyAPI 执行目录不是目录: {user_dir}"
        ));
    }
    Ok(dir)
}

// 查找 cliproxyapi.exe 并解析其工作目录：
//   1. 用户指定了目录(use_dir) → 优先在该目录内找 cliproxyapi.exe，working dir 即该目录
//      （指明确意图：exe 就放这里；找不到不回退到别处，避免在错误位置启动）
//   2. 未指定 → 沿用原逻辑：本程序同目录 → PATH 查找
// PATH 命名命中时 working dir 取当前目录
fn locate_proxy(use_dir: Option<&str>) -> Result<ProxyLocation, String> {
    const EXE_NAME: &str = "cliproxyapi.exe";

    // 1. 用户指定目录：先校验目录本身，再在其中找 exe
    if let Some(dir) = use_dir.filter(|s| !s.trim().is_empty()) {
        let dir = validate_user_dir(dir)?;
        let candidate = dir.join(EXE_NAME);
        if !candidate.exists() {
            // 明确告知指定的目录里没有 exe，不静默回退，避免在错误位置启动
            return Err(format!(
                "❌ 指定目录下未找到 {EXE_NAME}：{}（请把 cliproxyapi.exe 放入该目录）",
                dir.display()
            ));
        }
        return Ok(ProxyLocation {
            exe_path: candidate.to_string_lossy().into_owned(),
            work_dir: dir,
        });
    }

    // 2. 回退路径 A：当前可执行文件所在目录
    if let Ok(self_path) = std::env::current_exe() {
        if let Some(self_dir) = self_path.parent() {
            let candidate = self_dir.join(EXE_NAME);
            if candidate.exists() {
                return Ok(ProxyLocation {
                    exe_path: candidate.to_string_lossy().into_owned(),
                    work_dir: self_dir.to_path_buf(),
                });
            }
        }
    }

    // 3. 回退路径 B：PATH 中查找，working dir 取当前目录
    if which::which(EXE_NAME).is_ok() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        return Ok(ProxyLocation {
            exe_path: EXE_NAME.to_string(),
            work_dir: cwd,
        });
    }

    Err(
        "❌ 未找到 cliproxyapi.exe，请将其放到本程序同目录下、放入指定目录，或添加到系统 PATH"
            .to_string(),
    )
}

// 状态查询：HTTP GET {url}/v1/models，2 秒超时，连上即运行中
// 返回扁平 JSON 供前端展示，字段对齐 golang 版（running/url/status_code/message/error）
pub fn status(url: &str) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    result.insert("running".into(), serde_json::Value::Bool(false));
    result.insert("url".into(), serde_json::Value::String(url.to_string()));

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
            result.insert("status_code".into(), serde_json::Value::Number(code.into()));
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

// 启动 CLIProxyAPI：已在运行则跳过；定位 exe 后开新终端窗口，等待 2 秒再探测
// exec_dir 指定执行目录：非空时优先在该目录内找 cliproxyapi.exe 并以其为 working dir；
//   None/空串表示未指定，回退到本程序同目录或 PATH 查找
pub fn start(base_url: &str, exec_dir: Option<&str>) -> Result<String, String> {
    // 先检查是否已经在运行
    if status(base_url).get("running") == Some(&serde_json::Value::Bool(true)) {
        return Ok("⚠️ CLIProxyAPI 已经在运行中".to_string());
    }

    // 定位 exe 与工作目录（指定目录校验失败/exe 找不到在此统一报错）
    let loc = locate_proxy(exec_dir)?;
    let exe_path = &loc.exe_path;
    let work_dir = &loc.work_dir;

    // cmd /c start "CLIProxyAPI" cmd /k <exe>，在新终端窗口前台运行
    let mut cmd = Command::new("cmd");
    cmd.args(["/c", "start", "CLIProxyAPI", "cmd", "/k", exe_path])
        .current_dir(work_dir);

    cmd.spawn().map_err(|e| format!("❌ 启动失败: {e}"))?;

    // 等待并复探状态
    std::thread::sleep(Duration::from_secs(2));
    if status(base_url).get("running") == Some(&serde_json::Value::Bool(true)) {
        return Ok("✅ CLIProxyAPI 已启动".to_string());
    }
    Ok("⚠️ 启动命令已执行，但服务可能未成功启动，请检查终端窗口".to_string())
}

// 停止 CLIProxyAPI：taskkill /F /IM cliproxyapi.exe
pub fn stop() -> Result<String, String> {
    let output = Command::new("taskkill")
        .args(["/F", "/IM", "cliproxyapi.exe"])
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

#[cfg(test)]
mod tests {
    use super::*;

    // validate_user_dir：指定不存在的目录时显式报错，不静默回退
    #[test]
    fn validate_user_dir_errors_when_missing() {
        let res = validate_user_dir(r"C:\definitely\not\here\nope");
        assert!(res.is_err(), "不存在的目录应报错");
        let err = res.unwrap_err();
        assert!(err.contains("不存在"), "错误信息应点明原因: {err}");
    }

    // locate_proxy：指定目录存在但里面没有 exe 时，显式告知未找到，不回退到别处
    // 通过临时目录模拟：建空目录，期望报错且提示未找到 exe
    #[test]
    fn locate_proxy_errors_when_dir_has_no_exe() {
        let tmp =
            std::env::temp_dir().join(format!("claude-launcher-test-{:?}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("建临时目录失败");
        let res = locate_proxy(Some(tmp.to_str().unwrap()));
        // 清理
        let _ = std::fs::remove_dir(&tmp);

        assert!(res.is_err(), "目录内无 exe 应报错");
        let err = res.unwrap_err();
        assert!(
            err.contains("未找到") || err.contains("cliproxyapi.exe"),
            "错误应指向目录内缺 exe: {err}"
        );
    }

    // locate_proxy：未指定目录(None)时不应因缺 exe 校验逻辑而 panic，
    // 走回退路径：本程序同目录通常也没有 exe，结果可能是 Err——这里只验证不 panic
    #[test]
    fn locate_proxy_none_does_not_panic() {
        // 不关心结果（环境里可能就是没有 exe），只要不 panic 即可
        let _ = locate_proxy(None);
    }
}
