// 防止 Windows release 构建弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    claude_launcher_lib::run()
}
