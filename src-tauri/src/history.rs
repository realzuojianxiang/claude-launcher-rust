// 历史目录持久化模块：读写「exe 同级 claude-launcher/history.json」
// 与 config.json 同目录（用户要求配置都放 exe 同级），记录用户打开过的工作目录，
// 去重、最近优先，上限 200 条。前端仅展示最近 3 条，更多项由前端折叠控制。

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_ENTRIES: usize = 200;

// 历史目录集合，按最近使用在前排序
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub dirs: Vec<String>,
}

// 历史文件路径：exe 同级的 claude-launcher/history.json，目录缺失则创建
// 复用 Config::config_dir()，保证与 config.json 同处一目录
pub fn path() -> PathBuf {
    let dir = Config::config_dir();
    let _ = fs::create_dir_all(&dir);
    dir.join("history.json")
}

// 加载历史：文件缺失返回空历史（首次安装属正常）；解析失败不再静默丢空，
// 而是把损坏文件改名为带时间戳的证据文件并记日志，避免"最近目录列表悄悄清空"
// 却无人知晓——同类缺陷已在 config.json（S4）修过，这里对齐。
pub fn load() -> History {
    let path = path();
    let data = match fs::read(&path) {
        Ok(d) => d,
        Err(_) => return History::default(), // 缺文件：首次安装，正常
    };
    match serde_json::from_slice::<History>(&data) {
        Ok(h) => h,
        Err(e) => {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let corrupt = path.with_extension(format!("corrupt-{stamp}.json"));
            let _ = fs::rename(&path, &corrupt);
            tracing::error!(
                error = %e,
                corrupt_path = ?corrupt,
                "history.json 解析失败，已重命名为证据文件并回退空历史"
            );
            History::default()
        }
    }
}

// 保存历史到磁盘：序列化到同目录临时文件 -> flush -> sync_all -> 原子 rename 覆盖。
// 与 Config::save 同款原子写：进程异常退出 / 磁盘写满 / 同步中断至多留下一个残骸，
// 目标文件要么是完整旧版要么是完整新版，杜绝裸 fs::write 半写损坏导致下次加载
// 静默回退空历史（表现为最近目录列表突然清空）。
fn save(history: &History) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(history).map_err(|e| format!("序列化历史失败: {e}"))?;
    let path = path();
    let dir = path
        .parent()
        .ok_or_else(|| "无法定位历史目录".to_string())?;
    fs::create_dir_all(dir).map_err(|e| format!("创建历史目录失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时历史文件失败: {e}"))?;
        f.write_all(&data)
            .map_err(|e| format!("写入历史失败: {e}"))?;
        f.flush().map_err(|e| format!("刷新历史失败: {e}"))?;
        let _ = f.sync_all();
    }
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("原子替换历史文件失败: {e}"));
    }
    Ok(())
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

// 删除一个历史目录：按精确路径匹配移除，不存在的目录视为成功（幂等）
pub fn remove(dir: &str) -> Result<History, String> {
    let mut history = load();
    history.dirs.retain(|d| d != dir);
    save(&history)?;
    Ok(history)
}

// 获取当前历史目录列表（最近在前）
pub fn list() -> Vec<String> {
    load().dirs
}
