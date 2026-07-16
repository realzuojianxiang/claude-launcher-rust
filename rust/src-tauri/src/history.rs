// 历史目录持久化模块：读写 ~/.claude-launcher/history.json
// 记录用户打开过的工作目录，去重、最近优先，上限 200 条
// 前端仅展示最近 3 条，更多项由前端折叠控制；后端始终维护全量上限

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_ENTRIES: usize = 200;

// 历史目录集合，按最近使用在前排序
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub dirs: Vec<String>,
}

// 历史文件路径：~/.claude-launcher/history.json，目录缺失则创建
pub fn path() -> PathBuf {
    let home = dirs::home_dir().expect("无法获取用户主目录");
    let dir = home.join(".claude-launcher");
    let _ = fs::create_dir_all(&dir);
    dir.join("history.json")
}

// 加载历史：文件缺失或解析失败时返回空历史
pub fn load() -> History {
    let path = path();
    match fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or_default(),
        Err(_) => History::default(),
    }
}

// 保存历史到磁盘，序列化或写入失败时返回错误
fn save(history: &History) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(history)
        .map_err(|e| format!("序列化历史失败: {e}"))?;
    fs::write(path(), data).map_err(|e| format!("写入历史失败: {e}"))
}

// 记录一个目录：去重后置顶（最近在前），超过上限截断尾部
// 空字符串忽略不计
pub fn add(dir: &str) -> Result<History, String> {
    if dir.trim().is_empty() {
        return Ok(load());
    }
    let mut history = load();
    // 去重：移除已存在的同名目录，再置顶
    history.dirs.retain(|d| d != dir);
    history.dirs.insert(0, dir.to_string());
    // 上限截断
    history.dirs.truncate(MAX_ENTRIES);
    save(&history)?;
    Ok(history)
}

// 获取当前历史目录列表（最近在前）
pub fn list() -> Vec<String> {
    load().dirs
}
