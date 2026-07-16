// 启动 Claude Code 模块
// 对应 golang 版 LaunchClaude：查 claude 命令、备份改写 settings、写临时 bat、cmd /c start 开新窗口、夹还原脚本
// settings 改写可逆：启动失败回滚，Claude 进程结束后由 bat 尾部调用还原脚本

use crate::settings;
use crate::Config;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// 在 PATH 中查找 claude 命令，Windows 上回退尝试 .exe 后缀
fn find_claude() -> Option<String> {
    // 检查 PATH 中是否有 claude / claude.exe
    if which::which("claude.exe").is_ok() {
        return Some("claude.exe".to_string());
    }
    if which::which("claude").is_ok() {
        return Some("claude".to_string());
    }
    None
}

// 临时目录下生成启动批处理文件名
fn launcher_bat() -> PathBuf {
    std::env::temp_dir().join("claude_launcher.bat")
}

// 还原脚本路径
fn restore_bat() -> PathBuf {
    std::env::temp_dir().join("claude_restore_settings.bat")
}

// 写入还原脚本：把备份文件 copy 回 settings.json 并删除备份
fn write_restore_script() -> Result<PathBuf, String> {
    let bak = settings::backup_file();
    let sf = settings::settings_file();
    let script = restore_bat();
    // 注意：路径与字符串都用双引号包裹，避免含空格路径出错
    let content = format!(
        "@echo off\r\necho Restoring Claude settings...\r\ncopy \"{bak}\" \"{sf}\" /Y\r\ndel \"{bak}\"\r\necho Settings restored.\r\n",
        bak = bak.display(),
        sf = sf.display()
    );
    fs::write(&script, content).map_err(|e| format!("创建还原脚本失败: {e}"))?;
    Ok(script)
}

// 启动 Claude Code：返回给前端的状态字符串（对齐 golang 版的 emoji + 文案）
pub fn launch(config: &Config) -> Result<String, String> {
    if config.work_dir.is_empty() {
        return Err("请先选择工作目录".to_string());
    }

    let claude_cmd = find_claude()
        .ok_or_else(|| "未找到 claude 命令，请确保 Claude Code 已安装并添加到 PATH".to_string())?;

    // 1. 备份原 settings.json
    settings::backup().map_err(|e| format!("备份配置失败: {e}"))?;

    // 2. 修改 settings.json，失败则回滚备份
    if let Err(e) = settings::modify(&config.anthropic_url, &config.anthropic_key) {
        // 还原备份，忽略其错误（主错误优先上报）
        let _ = settings::restore();
        return Err(format!("修改配置失败: {e}"));
    }

    // 3. 生成还原脚本
    let restore_script = write_restore_script()?;

    // 4. 拼装启动批处理：cd 到工作目录 -> claude -> 还原脚本 -> pause
    let yolo_flag = if config.yolo_mode {
        " --dangerously-skip-permissions"
    } else {
        ""
    };
    let batch = format!(
        "@echo off\r\necho Starting Claude Code with CLIProxyAPI...\r\necho Settings modified: ANTHROPIC_BASE_URL={url}\r\necho YOLO Mode: {yolo}\r\necho.\r\ncd /d \"{work}\"\r\n{cmd}{yolo_flag}\r\necho.\r\necho Claude Code exited, restoring original settings...\r\ncall \"{restore}\"\r\npause\r\n",
        url = config.anthropic_url,
        yolo = config.yolo_mode,
        work = config.work_dir,
        cmd = claude_cmd,
        yolo_flag = yolo_flag,
        restore = restore_script.display()
    );

    let batch_file = launcher_bat();
    fs::write(&batch_file, batch).map_err(|e| format!("创建启动脚本失败: {e}"))?;

    // 5. 在新终端窗口中启动：cmd /c start "标题" cmd /k <bat>
    let launch = Command::new("cmd")
        .args([
            "/c",
            "start",
            "Claude Code (CLIProxyAPI)",
            "cmd",
            "/k",
            &batch_file.to_string_lossy(),
        ])
        .spawn();
    if let Err(e) = launch {
        // 启动失败，还原备份
        let _ = settings::restore();
        return Err(format!("启动失败: {e}"));
    }

    Ok("✅ Claude Code 已启动（settings.json 已修改，退出后将自动还原）".to_string())
}
