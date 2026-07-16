// Claude Launcher - Rust 实现后端入口
// 模块按职责拆分：config / settings / claude / proxy
// 所有前端可调用方法以 #[tauri::command] 暴露，集中注册于 run()

mod claude;
mod config;
mod proxy;
mod settings;

use config::Config;
use serde::Serialize;

// 退出兜底还原 settings.json：窗口关闭时触发，对齐 golang 版 beforeClose
fn restore_on_exit() {
    let _ = settings::restore();
}

// GetConfig：返回当前配置，前端据此回填表单
#[tauri::command]
fn get_config(state: tauri::State<'_, std::sync::Mutex<Config>>) -> Config {
    state.lock().unwrap().clone()
}

// SetConfig：更新代理地址/密钥/yolo 模式并持久化；成功返回提示，失败返回错误
#[tauri::command]
fn set_config(
    state: tauri::State<'_, std::sync::Mutex<Config>>,
    url: String,
    key: String,
    cliproxy_key: String,
    yolo_mode: bool,
) -> Result<String, String> {
    let mut cfg = state.lock().unwrap();
    cfg.anthropic_url = url;
    cfg.anthropic_key = key;
    cfg.cliproxyapi_key = cliproxy_key;
    cfg.yolo_mode = yolo_mode;
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
            }
            Ok(s)
        }
        None => Ok(String::new()),
    }
}

// GetSettingsInfo：返回 settings.json 当前状态（路径/内容/备份是否存在）
#[tauri::command]
fn get_settings_info() -> serde_json::Value {
    settings::info()
}

// RestoreNow：立即还原 settings.json
#[tauri::command]
fn restore_now() -> Result<String, String> {
    settings::restore()?;
    Ok("✅ settings.json 已还原".to_string())
}

// LaunchClaude：启动 Claude Code（含 settings 改写与还原机制）
#[tauri::command]
fn launch_claude(state: tauri::State<'_, std::sync::Mutex<Config>>) -> Result<String, String> {
    let cfg = state.lock().unwrap().clone();
    claude::launch(&cfg)
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
#[tauri::command]
fn start_cliproxyapi(state: tauri::State<'_, std::sync::Mutex<Config>>) -> Result<String, String> {
    let url = state.lock().unwrap().anthropic_url.clone();
    proxy::start(&url)
}

// StopCLIProxyAPI：taskkill 终止代理进程
#[tauri::command]
fn stop_cliproxyapi() -> Result<String, String> {
    proxy::stop()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(std::sync::Mutex::new(Config::load()))
        .on_window_event(|window, event| {
            // 主窗口关闭时兜底还原 settings.json
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    restore_on_exit();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            select_directory,
            get_settings_info,
            restore_now,
            launch_claude,
            get_system_info,
            cliproxyapi_status,
            start_cliproxyapi,
            stop_cliproxyapi,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
