// 应用日志模块：统一收集 tracing 日志，提供
//   1) 内存环形缓冲（最近 N 行）——供前端日志页初始回填；
//   2) 文件落盘（logs/ 目录，单文件 >1MB 自动分文件，文件名带日期时间后缀）——持久化；
//   3) Tauri 事件 nvidia-log 实时推送——前端日志页动态展示。
//
// 通过实现 tracing_subscriber::Layer 接入全局 subscriber，不改动业务代码的日志调用。

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;

use chrono::Local;
use serde_json::{json, Value};
use tauri::Emitter;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

// 内存环形缓冲最大行数（页面初始回填取最近这些）
const MAX_BUFFER: usize = 2000;
// 单日志文件滚动阈值（字节）：1MB
const ROTATE_BYTES: u64 = 1_000_000;
// 日志文件名前缀
const FILE_PREFIX: &str = "nvidia-proxy";
// 实时事件推送队列容量：有界 channel，队列满时丢弃最旧事件并累计丢弃计数，
// 避免日志风暴时每条消息 spawn 一个 OS 线程把线程数/句柄数打爆（S5）。
const EVENT_QUEUE_CAP: usize = 1024;

// 全局单例：在 app setup 阶段初始化（拿到 AppHandle 与配置目录）
static INSTANCE: OnceLock<Arc<Logger>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
}

impl LogLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "ERROR" => Some(Self::Error),
            "WARN" => Some(Self::Warn),
            "INFO" => Some(Self::Info),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
        }
    }

    pub fn allows(self, level: &tracing::Level) -> bool {
        let event_level = if *level == tracing::Level::ERROR {
            Self::Error as u8
        } else if *level == tracing::Level::WARN {
            Self::Warn as u8
        } else if *level == tracing::Level::INFO {
            Self::Info as u8
        } else {
            return false;
        };
        event_level <= self as u8
    }
}

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

pub fn init_level_from_env() {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|value| LogLevel::parse(&value))
        .unwrap_or(LogLevel::Info);
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn current_log_level() -> LogLevel {
    match LOG_LEVEL.load(Ordering::Relaxed) {
        1 => LogLevel::Error,
        2 => LogLevel::Warn,
        _ => LogLevel::Info,
    }
}

pub fn set_log_level(value: &str) -> Result<LogLevel, String> {
    let level =
        LogLevel::parse(value).ok_or_else(|| "日志等级必须是 INFO、WARN 或 ERROR".to_string())?;
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
    Ok(level)
}

pub struct Logger {
    // 最近日志环形缓冲（oldest -> newest）
    buffer: Mutex<VecDeque<String>>,
    // 当前活动日志文件句柄（None 表示尚未打开 / 打开失败）
    file: Mutex<Option<File>>,
    // 当前文件已写入字节数（用于判断是否滚动）
    current_size: Mutex<u64>,
    // 日志目录（配置目录下的 logs/）
    logs_dir: Mutex<PathBuf>,
    // AppHandle：用于实时事件推送（setup 后才有）
    app: Mutex<Option<tauri::AppHandle>>,
    // 【S5】实时事件推送队列：所有日志行投递到这里，由单一消费者线程 emit。
    // 队列满时直接丢弃并累计 dropped 计数，不再 spawn OS 线程。
    // 使用 sync_channel 的 SyncSender（有界）以施加背压；存 Option 是因为 init 分两步。
    event_tx: Mutex<Option<std::sync::mpsc::SyncSender<String>>>,
    // 被丢弃的实时事件累计计数（日志风暴时背压指标）
    dropped: std::sync::atomic::AtomicU64,
}

impl Logger {
    // 初始化全局单例（仅一次）：创建 logs 目录并打开首个日志文件。
    pub fn init(app: tauri::AppHandle, config_dir: &std::path::Path) {
        let logs_dir = config_dir.join("logs");
        let _ = fs::create_dir_all(&logs_dir);

        let logger = Arc::new(Logger {
            buffer: Mutex::new(VecDeque::with_capacity(MAX_BUFFER)),
            file: Mutex::new(None),
            current_size: Mutex::new(0),
            logs_dir: Mutex::new(logs_dir),
            app: Mutex::new(Some(app.clone())),
            event_tx: Mutex::new(None),
            dropped: std::sync::atomic::AtomicU64::new(0),
        });
        // 打开第一个日志文件（文件名带启动时间）
        logger.open_new_file();

        // 【S5】启动单一消费者线程：从有界队列取日志行 emit 给前端。
        // 固定一个线程，替代「每条日志 spawn 一个线程」；队列满时生产端直接丢弃并计数，
        // 既阻止线程数爆炸，也不让日志热路径阻塞调用线程。线程退出随进程结束，无需独立关闭。
        let (tx, rx) = mpsc::sync_channel::<String>(EVENT_QUEUE_CAP);
        *logger.event_tx.lock().unwrap() = Some(tx);
        let logger_for_thread = Arc::clone(&logger);
        std::thread::Builder::new()
            .name("nvidia-log-emit".to_string())
            .spawn(move || {
                logger_for_thread.event_consumer(rx);
            })
            .expect("spawn nvidia-log-emit 失败");

        let _ = INSTANCE.set(logger);
    }

    // 事件消费者循环：阻塞接收日志行并 emit。队列空时阻塞等待，不忙轮询。
    // app 被释放（应用退出）后 emit 失败无碍，继续消费直到发送端全部释放后退出。
    fn event_consumer(&self, rx: mpsc::Receiver<String>) {
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(line) => {
                    if let Some(app) = self.app.lock().unwrap().clone() {
                        let _ = app.emit("nvidia-log", line);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // 空闲 1s：附带把累计丢弃计数转成一条提示发出去后计数清零，
                    // 让 UI 能感知「有 N 条日志被背压丢弃」而不是无声丢失。
                    let dropped = self.dropped.swap(0, std::sync::atomic::Ordering::Relaxed);
                    if dropped > 0 {
                        if let Some(app) = self.app.lock().unwrap().clone() {
                            let _ = app.emit(
                                "nvidia-log",
                                format!("[warn] 日志事件队列过载，已丢弃 {dropped} 条实时事件"),
                            );
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break, // 发送端全部释放，退出
            }
        }
    }

    // 取全局单例（未初始化返回 None，避免崩溃）
    pub fn global_opt() -> Option<Arc<Logger>> {
        INSTANCE.get().cloned()
    }

    // 当前本地时间字符串：YYYYMMDD-HHMMSS（用于文件名与每行前缀）
    fn now_string() -> String {
        Local::now().format("%Y%m%d-%H%M%S").to_string()
    }

    // 生成一个当前时间点的日志文件路径（如已存在则追加序号避免覆盖）
    fn new_file_path(&self) -> PathBuf {
        let dir = self.logs_dir.lock().unwrap().clone();
        let stamp = Self::now_string();
        let base = dir.join(format!("{}-{}.log", FILE_PREFIX, stamp));
        if !base.exists() {
            return base;
        }
        let mut i = 1u32;
        loop {
            let p = dir.join(format!("{}-{}-{}.log", FILE_PREFIX, stamp, i));
            if !p.exists() {
                return p;
            }
            i += 1;
        }
    }

    // 关闭旧文件并打开一个新的带时间戳的日志文件
    fn open_new_file(&self) {
        let path = self.new_file_path();
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(f) => {
                *self.file.lock().unwrap() = Some(f);
                *self.current_size.lock().unwrap() = 0;
                // 在文件首行写入启动分隔，便于人工翻阅
                self.write_raw(&format!(
                    "==== {} 日志开始 ({}) ====",
                    FILE_PREFIX,
                    path.display()
                ));
            }
            Err(e) => {
                eprintln!("打开日志文件失败 {}: {e}", path.display());
                *self.file.lock().unwrap() = None;
            }
        }
    }

    // 写一行原始文本到当前文件（不加重载时间戳），用于文件头分隔行
    fn write_raw(&self, line: &str) {
        let mut size = self.current_size.lock().unwrap();
        let mut file = self.file.lock().unwrap();
        if let Some(f) = file.as_mut() {
            let bytes = line.as_bytes();
            if f.write_all(bytes).is_ok() && f.write_all(b"\n").is_ok() {
                let _ = f.flush();
                *size += bytes.len() as u64 + 1;
            }
        }
    }

    // 核心写入：带时间戳，更新环形缓冲 + 文件（按需滚动）+ 实时事件
    pub fn write_line(&self, content: &str) {
        let line = format!("[{}] {}", Self::now_string(), content);

        // 1. 内存环形缓冲
        {
            let mut buf = self.buffer.lock().unwrap();
            if buf.len() >= MAX_BUFFER {
                buf.pop_front();
            }
            buf.push_back(line.clone());
        }

        // 2. 文件落盘（超过阈值则滚动到新文件）
        {
            let needs_rotation = {
                let size = self.current_size.lock().unwrap();
                let file = self.file.lock().unwrap();
                file.is_none() || *size > ROTATE_BYTES
            };
            if needs_rotation {
                self.open_new_file();
            }
            let mut size = self.current_size.lock().unwrap();
            let mut file = self.file.lock().unwrap();
            if let Some(f) = file.as_mut() {
                let bytes = line.as_bytes();
                if f.write_all(bytes).is_ok() && f.write_all(b"\n").is_ok() {
                    let _ = f.flush();
                    *size += bytes.len() as u64 + 1;
                }
            }
        }

        // 3. 实时事件推送：投递到有界队列（由 init 启动的单一消费者线程统一 emit）。
        //    队列满（生产端速度 > emit 消费速度，如上游日志风暴）时 try_send 失败，
        //    直接丢弃该行并累计 dropped 计数（消费者会在下一空闲周把计数转成一条提示）。
        //    不 spawn 线程、不阻塞日志热路径（Tauri 命令线程），原 spawn 线程方案的线程数
        //    无上限风险随之消除。同时不在热路径里拿 app.lock()（避免与事件层互锁）。
        if let Some(tx) = self.event_tx.lock().unwrap().clone() {
            if tx.try_send(line.clone()).is_err() {
                self.dropped
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    // 最近日志（oldest -> newest），供页面初始回填
    pub fn recent(&self) -> Vec<String> {
        self.buffer.lock().unwrap().iter().cloned().collect()
    }

    // 清空日志页使用的内存缓冲；历史日志文件继续保留在磁盘。
    pub fn clear_recent(&self) {
        self.buffer.lock().unwrap().clear();
    }

    // 日志文件列表（按文件名降序：最新在前），每项含 name 与 size 字节
    pub fn list_files(&self) -> Vec<Value> {
        let dir = self.logs_dir.lock().unwrap().clone();
        let mut out: Vec<Value> = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|s| s.to_str()) == Some("log") {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        out.push(json!({ "name": name, "size": size }));
                    }
                }
            }
        }
        // 文件名含时间戳，逆序即最新在前
        out.sort_by(|a, b| {
            b.get("name")
                .and_then(|v| v.as_str())
                .cmp(&a.get("name").and_then(|v| v.as_str()))
        });
        out
    }

    // 读取指定日志文件内容（仅允许 logs/ 下的 .log，禁止路径穿越）
    pub fn read_file(&self, name: &str) -> Result<String, String> {
        if name.is_empty()
            || name.contains("..")
            || name.contains('/')
            || name.contains('\\')
            || !name.ends_with(".log")
        {
            return Err("❌ 非法文件名".to_string());
        }
        let dir = self.logs_dir.lock().unwrap().clone();
        let path = dir.join(name);
        fs::read_to_string(&path).map_err(|e| format!("❌ 读取失败: {e}"))
    }
}

// ===== tracing Layer 实现：把每条 event 格式化后交给 Logger 写入 =====

// 访问器：从 event 中提取 message 字段及其它字段
struct LineVisitor {
    message: String,
    message_set: bool,
    extra: Vec<(String, String)>,
}

impl Visit for LineVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
            self.message_set = true;
        } else {
            self.extra
                .push((field.name().to_string(), value.to_string()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            if !self.message_set {
                self.message = format!("{:?}", value);
                self.message_set = true;
            }
        } else {
            self.extra
                .push((field.name().to_string(), format!("{:?}", value)));
        }
    }
}

// 自定义 Layer：捕获所有 tracing event（不限定 target），统一送 Logger
pub struct NvidiaLogLayer;

impl<S> Layer<S> for NvidiaLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        if !current_log_level().allows(&level) {
            return;
        }

        let logger = match Logger::global_opt() {
            Some(l) => l,
            None => return, // 尚未初始化（setup 之前），丢弃
        };

        let mut visitor = LineVisitor {
            message: String::new(),
            message_set: false,
            extra: Vec::new(),
        };
        event.record(&mut visitor);

        let mut content = visitor.message;
        if !visitor.extra.is_empty() {
            let parts: Vec<String> = visitor
                .extra
                .into_iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            if !content.is_empty() {
                content.push(' ');
            }
            content.push_str(&parts.join(" "));
        }

        let line = format!("{} {}", level, content);
        logger.write_line(&line);
    }
}

#[cfg(test)]
mod log_level_tests {
    use super::{LogLevel, Logger};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[test]
    fn warn_level_allows_warn_and_error_but_rejects_info() {
        assert!(LogLevel::Warn.allows(&tracing::Level::WARN));
        assert!(LogLevel::Warn.allows(&tracing::Level::ERROR));
        assert!(!LogLevel::Warn.allows(&tracing::Level::INFO));
    }

    #[test]
    fn level_names_are_case_insensitive_and_reject_unknown_values() {
        assert_eq!(LogLevel::parse("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::parse("warn"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::parse("Error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::parse("debug"), None);
    }

    #[test]
    fn clearing_recent_logs_empties_only_the_in_memory_buffer() {
        let logger = Logger {
            buffer: Mutex::new(VecDeque::from([
                "first live line".to_string(),
                "second live line".to_string(),
            ])),
            file: Mutex::new(None),
            current_size: Mutex::new(0),
            logs_dir: Mutex::new(std::path::PathBuf::from("unused")),
            app: Mutex::new(None),
            event_tx: Mutex::new(None),
            dropped: std::sync::atomic::AtomicU64::new(0),
        };

        logger.clear_recent();

        assert!(logger.recent().is_empty());
        assert!(logger.file.lock().unwrap().is_none());
    }
}
