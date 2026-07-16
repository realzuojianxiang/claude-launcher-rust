// 配置持久化模块：读写 ~/.claude-launcher/config.json
// 对应 golang 版 Config / loadConfig / saveConfig / GetConfig / SetConfig

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// 应用配置结构，字段对齐 golang 版
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub work_dir: String,
    pub anthropic_url: String,
    pub anthropic_key: String,
    pub cliproxyapi_key: String,
    pub yolo_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_dir: String::new(),
            // 默认接入地址与示例密钥，对齐 golang 版
            anthropic_url: "http://localhost:8317".to_string(),
            anthropic_key: "sk-cliproxy-demo-key-1".to_string(),
            cliproxyapi_key: String::new(),
            yolo_mode: false,
        }
    }
}

impl Config {
    // 推断配置文件路径：~/.claude-launcher/config.json，目录不存在则创建
    pub fn path() -> PathBuf {
        let home = dirs::home_dir().expect("无法获取用户主目录");
        let dir = home.join(".claude-launcher");
        // 配置目录缺失时创建，忽略错误：读取时仍会回退默认值
        let _ = fs::create_dir_all(&dir);
        dir.join("config.json")
    }

    // 加载配置：文件缺失或解析失败时回退默认值（对齐 golang 版静默回退行为）
    pub fn load() -> Self {
        let path = Self::path();
        match fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    // 保存配置到磁盘，序列化失败或写入失败时返回错误
    pub fn save(&self) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("序列化配置失败: {e}"))?;
        let path = Self::path();
        fs::write(&path, data).map_err(|e| format!("写入配置失败: {e}"))
    }
}
