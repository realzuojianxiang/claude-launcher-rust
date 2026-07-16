// settings.json 可逆改写模块
// 对应 golang 版 backupSettings / modifySettings / restoreSettings / GetSettingsInfo / RestoreNow
// 关键且可逆：启动 Claude 时注入 CLIProxyAPI 接入参数，退出/失败时还原原始文件

use std::fs;
use std::path::PathBuf;

// 推断 settings 文件路径：~/.claude/settings.json
pub fn settings_file() -> PathBuf {
    let home = dirs::home_dir().expect("无法获取用户主目录");
    home.join(".claude").join("settings.json")
}

// 推断备份文件路径：~/.claude/settings.json.launcher_backup
pub fn backup_file() -> PathBuf {
    let home = dirs::home_dir().expect("无法获取用户主目录");
    home.join(".claude").join("settings.json.launcher_backup")
}

// 备份原 settings.json：原文件不存在则写入空对象标记，存在则整文件拷贝
pub fn backup() -> Result<(), String> {
    let src = settings_file();
    let dst = backup_file();
    if !src.exists() {
        // 原文件不存在，创建一个空备份标记
        fs::write(&dst, "{}").map_err(|e| format!("创建备份标记失败: {e}"))?;
        return Ok(());
    }
    fs::copy(&src, &dst).map_err(|e| format!("备份 settings 失败: {e}"))?;
    Ok(())
}

// 还原备份的 settings.json：无备份则跳过；还原后删除备份文件
pub fn restore() -> Result<(), String> {
    let bak = backup_file();
    if !bak.exists() {
        return Ok(()); // 没有备份，无需还原
    }
    let data = fs::read(&bak).map_err(|e| format!("读取备份失败: {e}"))?;
    fs::write(settings_file(), data).map_err(|e| format!("还原 settings 失败: {e}"))?;
    fs::remove_file(&bak).map_err(|e| format!("删除备份失败: {e}"))?;
    Ok(())
}

// 修改 settings.json 为使用 CLIProxyAPI：注入 base_url/api_key，清掉冲突变量，置 apiProvider=anthropic
// 使用 serde_json::Value 解析整文件，保留所有未声明的顶层字段
pub fn modify(url: &str, key: &str) -> Result<(), String> {
    let path = settings_file();
    // 确保 .claude 目录存在
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // 读取现有配置，缺省时用空对象
    let mut settings: serde_json::Value = match fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or(serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    };
    if !settings.is_object() {
        settings = serde_json::json!({});
    }
    let obj = settings.as_object_mut().unwrap();

    // 取出或新建 env 子对象
    let env = obj
        .entry("env".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !env.is_object() {
        *env = serde_json::json!({});
    }
    let env_obj = env.as_object_mut().unwrap();

    // 注入 CLIProxyAPI 接入参数
    env_obj.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        serde_json::Value::String(url.to_string()),
    );
    env_obj.insert(
        "ANTHROPIC_API_KEY".to_string(),
        serde_json::Value::String(key.to_string()),
    );

    // 清除可能冲突的变量
    env_obj.remove("ANTHROPIC_AUTH_TOKEN");
    env_obj.remove("ANTHROPIC_MODEL");
    env_obj.remove("ANTHROPIC_SMALL_FAST_MODEL");

    // 置 apiProvider=anthropic
    obj.insert(
        "apiProvider".to_string(),
        serde_json::Value::String("anthropic".to_string()),
    );

    let data = serde_json::to_vec_pretty(&settings)
        .map_err(|e| format!("序列化 settings 失败: {e}"))?;
    fs::write(&path, data).map_err(|e| format!("写入 settings 失败: {e}"))
}

// settings.json 当前状态信息（路径、内容、备份是否存在），返回扁平结构供前端展示
pub fn info() -> serde_json::Value {
    let sf = settings_file();
    let bf = backup_file();

    let content = match fs::read_to_string(&sf) {
        Ok(s) => s,
        Err(_) => "文件不存在".to_string(),
    };

    serde_json::json!({
        "settings_file": sf.to_string_lossy(),
        "backup_file": bf.to_string_lossy(),
        "settings_content": content,
        "backup_exists": bf.exists(),
    })
}
