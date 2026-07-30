// Claude Launcher - Rust 实现后端入口
// 模块按职责拆分：config / claude / proxy / history
// 所有前端可调用方法以 #[tauri::command] 暴露，集中注册于 run()

mod claude;
mod config;
mod history;
mod logger;
mod nvidia;
mod proxy;

use config::{Config, NvidiaConfig, Profile};
use nvidia::NvidiaState;
use serde::Serialize;
use std::io::Write;
use std::sync::OnceLock;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;
use tracing_subscriber::prelude::*;

// === 诊断辅助（仅 NVIDIA_DIAG=1 时启用）===
// 向配置目录下的 diag.txt 追加「关键步骤」时间戳，用于离线定位
// 「启动 8082 卡死」到底卡在哪一步。生产环境（DIAG 关闭）下 diag_step 是空操作，
// 不影响性能，也不依赖任何事件/UI，避免引入新的死锁。
static DIAG_ENABLED: OnceLock<bool> = OnceLock::new();

pub(crate) fn diag_step(msg: &str) {
    if DIAG_ENABLED.get().copied().unwrap_or(false) {
        let path = Config::config_dir().join("diag.txt");
        let stamp = chrono::Local::now().format("%H:%M:%S%.3f");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(format!("[{}] {}\n", stamp, msg).as_bytes()));
    }
}

// GetConfig：返回当前配置，前端据此回填表单
#[tauri::command]
fn get_config(state: tauri::State<'_, std::sync::Mutex<Config>>) -> Config {
    state.lock().unwrap().clone()
}

// ConfigPath：返回配置文件的绝对路径，便于前端展示「配置保存位置」，
// 避免用户误以为没保存（实际写到了 exe 同级 claude-launcher/config.json，而非旧路径）
#[tauri::command]
fn config_path() -> String {
    Config::path().to_string_lossy().to_string()
}

// SetConfig：更新代理地址/密钥/yolo 模式/auto-compact 阈值并持久化
// compact_window/compact_pct 经 Tauri 命令入参(camelCase)映射到 snake_case：
//   前端 compactWindow -> compact_window, compactPct -> compact_pct
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_config(
    state: tauri::State<'_, std::sync::Mutex<Config>>,
    url: String,
    key: String,
    cliproxy_key: String,
    yolo_mode: bool,
    compact_window: u64,
    compact_pct: u8,
    cliproxyapi_dir: String,
) -> Result<String, String> {
    let mut cfg = state.lock().unwrap();
    cfg.anthropic_url = url;
    cfg.anthropic_key = key;
    cfg.cliproxyapi_key = cliproxy_key;
    cfg.yolo_mode = yolo_mode;
    cfg.compact_window = compact_window;
    cfg.compact_pct = compact_pct;
    cfg.cliproxyapi_dir = cliproxyapi_dir;
    cfg.save()?;
    Ok("✅ 配置已保存".to_string())
}

// SelectDirectory：打开目录选择对话框，选中则写入配置 work_dir 并持久化
#[tauri::command]
fn select_directory(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<Config>>,
) -> Result<String, String> {
    // 借助 tauri-plugin-dialog 的 Rust API 直接弹出目录选择框
    use tauri_plugin_dialog::DialogExt;
    let dir = app
        .dialog()
        .file()
        .set_title("选择工作目录")
        .blocking_pick_folder();
    match dir {
        Some(path) => {
            let s = path.to_string();
            if !s.is_empty() {
                let mut cfg = state.lock().unwrap();
                cfg.work_dir = s.clone();
                // 持久化失败时仍把已选目录返回给前端展示，但上报保存错误
                cfg.save()?;
                // 记入历史目录（去重复近优先，上限 200）；失败不影响主流程
                let _ = history::add(&s);
            }
            Ok(s)
        }
        None => Ok(String::new()),
    }
}

// LaunchClaude：启动 Claude Code（连接参数以进程环境变量注入，不改动 settings.json）
// yolo 入参：启动页的 YOLO 勾选通过它覆盖 config 默认值；不传则回退 config.yolo_mode
// env 入参：所选供应商配置集的环境变量，注入到 claude 进程
#[tauri::command]
fn launch_claude(
    state: tauri::State<'_, std::sync::Mutex<Config>>,
    yolo: Option<bool>,
    env: std::collections::HashMap<String, String>,
) -> Result<String, String> {
    let cfg = state.lock().unwrap().clone();
    let result = claude::launch(&cfg, yolo.unwrap_or(cfg.yolo_mode), &env);
    // 启动成功时把工作目录记入历史，失败则不记
    if result.is_ok() && !cfg.work_dir.is_empty() {
        let _ = history::add(&cfg.work_dir);
    }
    result
}

// SetProfiles：整体替换供应商配置集并持久化（前端配置页编辑后调用）
#[tauri::command]
fn set_profiles(
    state: tauri::State<'_, std::sync::Mutex<Config>>,
    profiles: Vec<Profile>,
) -> Result<String, String> {
    let mut cfg = state.lock().unwrap();
    cfg.profiles = profiles;
    cfg.save()?;
    Ok("✅ 供应商配置已保存".to_string())
}

// GetRecentDirs：返回历史目录列表（最近在前），前端据此渲染最近 3 个 + 折叠展开
#[tauri::command]
fn get_recent_dirs() -> Vec<String> {
    history::list()
}

// AddRecentDir：手动追加一个历史目录（供前端选用历史项时同步记录）
// 入口校验同 set_work_dir：拒绝非法路径写入历史，避免下次选用时触发 .bat 注入。
#[tauri::command]
fn add_recent_dir(dir: String) -> Result<Vec<String>, String> {
    let canonical = crate::claude::validate_work_dir(&dir)?;
    let history = history::add(&canonical.to_string_lossy())?;
    Ok(history.dirs)
}

// RemoveRecentDir：删除一个历史目录，返回删除后的列表供前端刷新
#[tauri::command]
fn remove_recent_dir(dir: String) -> Result<Vec<String>, String> {
    let history = history::remove(&dir)?;
    Ok(history.dirs)
}

// SetWorkDir：仅更新工作目录并持久化（供前端选用历史条目时同步后端状态）
// 入口校验：拒绝 cmd 元字符 / 不存在 / 非绝对路径，使后续 .bat 的 `cd /d "{work}"`
// 输入恒可信，杜绝 work_dir 注入命令执行（详见 claude::validate_work_dir）。
#[tauri::command]
fn set_work_dir(
    state: tauri::State<'_, std::sync::Mutex<Config>>,
    dir: String,
) -> Result<String, String> {
    let canonical = crate::claude::validate_work_dir(&dir)?;
    let mut cfg = state.lock().unwrap();
    cfg.work_dir = canonical.to_string_lossy().to_string();
    cfg.save()?;
    Ok(dir)
}

// GetSystemInfo：返回系统信息（平台写死 Windows，对齐 golang 版）
#[tauri::command]
fn get_system_info() -> SystemInfo {
    SystemInfo {
        os: "windows".to_string(),
        arch: "amd64".to_string(),
        version: "1.0.0".to_string(),
    }
}

#[derive(Serialize)]
struct SystemInfo {
    os: String,
    arch: String,
    version: String,
}

// CLIProxyAPIStatus：返回代理运行状态（连上即运行中）
#[tauri::command]
fn cliproxyapi_status(state: tauri::State<'_, std::sync::Mutex<Config>>) -> serde_json::Value {
    let url = state.lock().unwrap().anthropic_url.clone();
    proxy::status(&url)
}

// StartCLIProxyAPI：在新窗口启动代理进程
// 若配置了 cliproxyapi_dir 则以其为执行目录，否则回退到 exe 所在目录
#[tauri::command]
fn start_cliproxyapi(state: tauri::State<'_, std::sync::Mutex<Config>>) -> Result<String, String> {
    let cfg = state.lock().unwrap().clone();
    let exec_dir = if cfg.cliproxyapi_dir.trim().is_empty() {
        None
    } else {
        Some(cfg.cliproxyapi_dir.as_str())
    };
    proxy::start(&cfg.anthropic_url, exec_dir)
}

// SelectCliDir：打开目录选择对话框选 CLIProxyAPI 执行目录并持久化
// 空串（用户取消）保留原配置不动；非空则写入 cliproxyapi_dir
#[tauri::command]
fn select_cli_dir(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<Config>>,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let dir = app
        .dialog()
        .file()
        .set_title("选择 CLIProxyAPI 执行目录")
        .blocking_pick_folder();
    match dir {
        Some(path) => {
            let s = path.to_string();
            if !s.is_empty() {
                let mut cfg = state.lock().unwrap();
                cfg.cliproxyapi_dir = s.clone();
                cfg.save()?;
            }
            Ok(s)
        }
        None => Ok(String::new()),
    }
}

// SetCliDir：仅更新 CLIProxyAPI 执行目录并持久化（前端清空等场景用，空串=未指定）
#[tauri::command]
fn set_cli_dir(
    state: tauri::State<'_, std::sync::Mutex<Config>>,
    dir: String,
) -> Result<String, String> {
    let mut cfg = state.lock().unwrap();
    cfg.cliproxyapi_dir = dir.clone();
    cfg.save()?;
    Ok(dir)
}

// StopCLIProxyAPI：taskkill 终止代理进程
#[tauri::command]
fn stop_cliproxyapi() -> Result<String, String> {
    proxy::stop()
}

// ===== NVIDIA API 代理服务命令 =====

// SetNvidiaConfig：整体替换 NVIDIA 代理配置并持久化（前端配置页保存时调用）
//
// P2/SSRF 闸：保存时即校验 base_url（scheme + host 非空）。
// 让"误配上游 / 上游被劫持成 30x"在用户保存配置时就被拒绝，而不是等到下次 start()
// 才报错——避免"配了能存、跑起来才炸"的窗口。host 非回环时的 auth_token 闸由
// require_auth_if_exposed 守门，与 start() 入口保持一致。
#[tauri::command]
fn set_nvidia_config(
    state: tauri::State<'_, std::sync::Mutex<Config>>,
    nvidia: NvidiaConfig,
) -> Result<String, String> {
    let mut cfg = state.lock().unwrap();
    nvidia.validate_base_url()?;
    cfg.nvidia = nvidia;
    cfg.save()?;
    Ok("✅ NVIDIA 代理配置已保存".to_string())
}

// NvidiaSetModels：热更新模型优先级列表。
// 先持久化到 config.json，再实时应用到运行中的代理（无需重启）。
// 列表顺序即优先级：models[0] 为默认/最高优先级，其后依次为 Fallback。
#[tauri::command]
fn nvidia_set_models(
    nstate: tauri::State<'_, NvidiaState>,
    cstate: tauri::State<'_, std::sync::Mutex<Config>>,
    models: Vec<String>,
) -> Result<String, String> {
    // 去空白、去空项、按顺序去重（保持首次出现顺序）
    let mut cleaned: Vec<String> = Vec::new();
    for m in models {
        let t = m.trim().to_string();
        if !t.is_empty() && !cleaned.iter().any(|x: &String| x.eq_ignore_ascii_case(&t)) {
            cleaned.push(t);
        }
    }
    {
        let mut cfg = cstate.lock().unwrap();
        cfg.nvidia.models = cleaned.clone();
        cfg.save()?;
    }
    let applied = nstate.set_models(cleaned);
    if applied {
        Ok("✅ 优先级已实时生效（代理运行中，无需重启）".to_string())
    } else {
        Ok("✅ 优先级已保存（代理未运行，下次启动生效）".to_string())
    }
}

// NvidiaStatus：返回 NVIDIA 代理运行状态
#[tauri::command]
fn nvidia_status(nstate: tauri::State<'_, NvidiaState>) -> serde_json::Value {
    nstate.status()
}

// NvidiaKeyPool：返回 Key 池实时状态（X/Y 可用 + 每个 Key 冷却剩余秒数），
// 供 NVIDIA 代理页展示多 Key 轮询/冷却情况。
#[tauri::command]
fn nvidia_key_pool(nstate: tauri::State<'_, NvidiaState>) -> serde_json::Value {
    nstate.key_pool_status()
}

// NvidiaStart：以当前配置启动应用内 NVIDIA 代理服务
#[tauri::command]
fn nvidia_start(
    nstate: tauri::State<'_, NvidiaState>,
    cstate: tauri::State<'_, std::sync::Mutex<Config>>,
) -> Result<String, String> {
    let cfg = cstate.lock().unwrap().nvidia.clone();
    nstate.start(cfg)
}

// NvidiaStop：停止 NVIDIA 代理服务
#[tauri::command]
fn nvidia_stop(nstate: tauri::State<'_, NvidiaState>) -> Result<String, String> {
    nstate.stop()
}

// NvidiaTest：用第一个 Key + 第一个模型向 NVIDIA 发一次极短请求，验证连通性/Key/模型
#[tauri::command]
async fn nvidia_test(cstate: tauri::State<'_, std::sync::Mutex<Config>>) -> Result<String, String> {
    let cfg = cstate.lock().unwrap().nvidia.clone();
    nvidia::proxy::test_connection(&cfg).await
}

// NvidiaChatTest：向本机运行中的代理发一条真实 Anthropic 消息（可指定模型），
// 走完整转换链，用于在 UI 内直接测试 8082，免去手动 curl。
#[tauri::command]
async fn nvidia_chat_test(
    cstate: tauri::State<'_, std::sync::Mutex<Config>>,
    model: String,
    prompt: Option<String>,
) -> Result<String, String> {
    let cfg = cstate.lock().unwrap().nvidia.clone();
    let prompt = prompt
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "用一句话介绍你自己".to_string());
    nvidia::proxy::local_chat_test(&cfg, &model, &prompt).await
}

// ===== 日志页命令 =====

// GetLogs：返回内存环形缓冲里的最近日志（oldest -> newest），供日志页初始回填
#[tauri::command]
fn get_logs() -> Vec<String> {
    logger::Logger::global_opt()
        .map(|l| l.recent())
        .unwrap_or_default()
}

#[tauri::command]
fn get_log_level() -> String {
    logger::current_log_level().as_str().to_string()
}

#[tauri::command]
fn set_log_level(level: String) -> Result<String, String> {
    logger::set_log_level(&level).map(|value| value.as_str().to_string())
}

#[tauri::command]
fn clear_logs() -> Result<(), String> {
    let logger = logger::Logger::global_opt().ok_or_else(|| "❌ 日志模块未初始化".to_string())?;
    logger.clear_recent();
    Ok(())
}

// ListLogFiles：列出 logs/ 目录下历史日志文件（最新在前），含名称与大小
#[tauri::command]
fn list_log_files() -> Vec<serde_json::Value> {
    logger::Logger::global_opt()
        .map(|l| l.list_files())
        .unwrap_or_default()
}

// ReadLogFile：读取某个历史日志文件内容（仅限 logs/ 下的 .log，防路径穿越）
#[tauri::command]
fn read_log_file(name: String) -> Result<String, String> {
    logger::Logger::global_opt()
        .map(|l| l.read_file(&name))
        .unwrap_or_else(|| Err("❌ 日志模块未初始化".to_string()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 初始化结构化日志：默认 info 级，可用 RUST_LOG 覆盖。
    // 仅用自定义 NvidiaLogLayer：捕获全部 tracing 事件，写入 logs/ 目录（1MB 滚动、
    // 文件名带时间戳），并通过 Tauri 事件 nvidia-log 实时推送前端日志页。
    //
    // 【关键坑·曾致 8082 卡死】绝不能加 stdout 层（fmt::layer().with_writer(std::io::stdout)）。
    // 在 GUI 程序的 Tauri 命令线程（tokio worker）里向 stdout 写会死锁：tracing 分发在
    // stdout_layer.on_event 处阻塞，既导致 tracing::info! 永不返回（命令卡死、UI 转圈），
    // 又使我们的文件层根本收不到事件（日志文件空白）。所有日志统一走文件 + 事件推送即可。
    logger::init_level_from_env();

    // 应用自定义日志层：落盘 logs/ + 实时事件
    let app_layer = logger::NvidiaLogLayer;

    let _ = tracing_subscriber::registry().with(app_layer).try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            // 点标题栏 ×：阻止真正关闭，改为隐藏到托盘（进程继续后台运行）
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            // 初始化日志模块：拿到 AppHandle 与配置目录，创建 logs/ 并打开首个日志文件
            logger::Logger::init(app.handle().clone(), &Config::config_dir());

            // 系统托盘：关闭窗口时不退出，仅缩到右下角托盘区；右键托盘图标弹出菜单，
            // 只有点「退出」才真正结束进程。复用项目自带的应用图标，无需新增资源。
            let show_item = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .items(&[&show_item, &quit_item])
                .build()?;
            let tray_icon = app
                .default_window_icon()
                .cloned()
                .expect("应用图标缺失，无法创建托盘");
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .tooltip("Claude Launcher")
                // 仅在右键弹出菜单；左键用于还原窗口
                .show_menu_on_left_click(false)
                .menu(&tray_menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // 左键单击：还原并聚焦已隐藏的窗口
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            // S4：启动加载配置若检测到 config.json 损坏回退，写一份告警文件让前台/用户可见，
            // 避免静默丢失所有 Provider/Key 而无人知晓。
            if let Some(corrupt) = app
                .state::<std::sync::Mutex<Config>>()
                .lock()
                .unwrap()
                .last_corrupt_path
                .as_ref()
            {
                let _ = std::fs::write(
                    Config::config_dir().join("config.corrupt.notice.txt"),
                    format!(
                        "检测到 config.json 损坏，已重命名为证据文件：{}\n本次启动回退到默认配置。\n",
                        corrupt.display()
                    ),
                );
            }

            // 诊断钩子（NVIDIA_DIAG=1）：自动启动代理并把结果写入 diag.txt，
            // 便于离线排查「启动 8082 卡死」。写普通文件、不依赖任何事件/UI。
            let diag_on = std::env::var("NVIDIA_DIAG")
                .map(|v| v == "1")
                .unwrap_or(false);
            if diag_on {
                // 打开 diag 开关，使 start() 内部的 diag_step 开始记录
                DIAG_ENABLED.get_or_init(|| true);
                let cfg = app
                    .state::<std::sync::Mutex<Config>>()
                    .lock()
                    .unwrap()
                    .nvidia
                    .clone();
                let diag = Config::config_dir().join("diag.txt");
                let _ = std::fs::write(&diag, ""); // 清空旧内容
                std::thread::spawn(move || {
                    let _ = std::fs::write(&diag, "DIAG: calling NvidiaState::start\n");
                    let nstate = NvidiaState::new();
                    let r = nstate.start(cfg);
                    let _ = std::fs::write(&diag, format!("DIAG: start returned = {:?}\n", r));
                });
            }
            Ok(())
        })
        .manage({
            let (cfg, corrupt) = Config::load_or_default();
            // 把损坏证据塞回 Config 供 setup 写告警文件（此字段 serde skip，不落盘）
            let mut managed = cfg;
            managed.last_corrupt_path = corrupt;
            std::sync::Mutex::new(managed)
        })
        .manage(NvidiaState::new())
        .invoke_handler(tauri::generate_handler![
            get_config,
            config_path,
            set_config,
            select_directory,
            get_recent_dirs,
            add_recent_dir,
            remove_recent_dir,
            set_work_dir,
            launch_claude,
            set_profiles,
            get_system_info,
            cliproxyapi_status,
            start_cliproxyapi,
            stop_cliproxyapi,
            select_cli_dir,
            set_cli_dir,
            set_nvidia_config,
            nvidia_set_models,
            nvidia_status,
            nvidia_key_pool,
            nvidia_start,
            nvidia_stop,
            nvidia_test,
            nvidia_chat_test,
            get_logs,
            get_log_level,
            set_log_level,
            clear_logs,
            list_log_files,
            read_log_file,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
