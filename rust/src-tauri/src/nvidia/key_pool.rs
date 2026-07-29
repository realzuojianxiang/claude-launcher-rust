// 【Step 2】NVIDIA API Key 池：轮询调度 + 429 冷却。
//
// 设计要点：
//   - 多个 Key 以 round-robin 方式轮询，避免单 Key 被打满；
//   - 命中 429（限流）时，将该 Key 标记为冷却 `cooldown` 秒，期间 pick() 自动跳过，
//     从而把流量导向其他可用 Key；冷却到期后自动恢复；
//   - pick() 在「全部冷却中」时返回 None，由调用方决定是返回聚合错误还是等待。

use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct KeyEntry {
    key: String,
    cooldown_until: Option<Instant>,
    last_used_at: Instant,
}

pub struct KeyPool {
    entries: Vec<KeyEntry>,
    cooldown: Duration,
    cursor: usize,
}

impl KeyPool {
    pub fn new(keys: Vec<String>, cooldown_secs: u64) -> Self {
        let now = Instant::now();
        let entries = keys
            .into_iter()
            .map(|k| KeyEntry {
                key: k,
                cooldown_until: None,
                last_used_at: now,
            })
            .collect();
        KeyPool {
            entries,
            cooldown: Duration::from_secs(cooldown_secs.max(1)),
            cursor: 0,
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // 选一个可用 Key（跳过冷却中的），按 round-robin 推进游标。无可用 Key 返回 None。
    pub fn pick(&mut self) -> Option<String> {
        let n = self.entries.len();
        if n == 0 {
            return None;
        }
        let now = Instant::now();
        let start = self.cursor;
        for i in 0..n {
            let idx = (start + i) % n;
            let cooling = self.entries[idx]
                .cooldown_until
                .map(|t| t > now)
                .unwrap_or(false);
            if !cooling {
                self.cursor = (idx + 1) % n;
                self.entries[idx].last_used_at = now;
                return Some(self.entries[idx].key.clone());
            }
        }
        None
    }

    // 将指定 Key 标记为冷却 `cooldown` 秒（从现在起算）。
    pub fn cooldown(&mut self, key: &str) {
        let until = Instant::now() + self.cooldown;
        for e in &mut self.entries {
            if e.key == key {
                e.cooldown_until = Some(until);
                return;
            }
        }
    }

    // 当前可用 Key 数量（用于状态展示/调试）。
    pub fn available_count(&self) -> usize {
        let now = Instant::now();
        self.entries
            .iter()
            .filter(|e| e.cooldown_until.map(|t| t <= now).unwrap_or(true))
            .count()
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    // 状态快照：返回每个 Key 的脱敏信息 + 冷却剩余秒数，供 UI 展示。
    // 该方法是「Key 池状态面板」的核心数据源（Step 2 配套 UI）。
    pub fn snapshot(&self) -> Vec<Value> {
        let now = Instant::now();
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let cooling = e.cooldown_until.map(|t| t > now).unwrap_or(false);
                let remaining = e
                    .cooldown_until
                    .map(|t| {
                        let d = t.saturating_duration_since(now);
                        d.as_secs_f64().ceil() as u64
                    })
                    .unwrap_or(0);
                json!({
                    "index": i,
                    "masked": mask_key(&e.key),
                    "cooling": cooling,
                    "cooldown_remaining_secs": remaining,
                })
            })
            .collect()
    }
}

// 脱敏显示 Key：保留前 4 后 4，中间用省略号，避免明文泄露凭证。
pub(crate) fn mask_key(k: &str) -> String {
    let k = k.trim();
    if k.len() <= 8 {
        return "*".repeat(k.len());
    }
    let first = &k[..4];
    let last = &k[k.len() - 4..];
    format!("{first}…{last}")
}

// 供 ProxyCtx 在多线程下共享使用的别名。
pub type SharedKeyPool = Mutex<KeyPool>;
