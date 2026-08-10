//! (a) Phase A §2.1:集成测试共享的 env-gate 助手。
//!
//! 两个集成测试文件(`tests/script_e2e_real_key.rs` / `tests/subagent_e2e_real_key.rs`)
//! 都要同一套 gate 语义(`CODEAGENT_E2E` 开关 + `DEEPSEEK_API_KEY` 真 key),但集成测试在
//! crate 外、不能 `use crate::` 引 mcp.rs 的 `#[cfg(test)]` 助手;故在 `tests/common/` 放
//! 一份独立复本(最干净,无 crate 内可见性耦合)。两文件 `mod common;` 引之。
//!
//! Gate 语义镜像 `bash_hanging_command_actually_returns_within_timeout` 的 cfg! 自跑模式
//! (条件成立自跑、否则早退),**非 `#[ignore]`**:
//!   · `CODEAGENT_E2E` 非空 = 跑真 key 路径;
//!   · 未设 = `eprintln!("skipped: set CODEAGENT_E2E=1 (and DEEPSEEK_API_KEY ...)")`
//!     + 早 return(测试函数返回 Ok,记 pass 但事实 skipped —— CI 无 key 时数全绿、不
//!     灰成 #ignored 计数,也不挂记 `#[ignore]` 那串要手动跑的债)。
//!
//! 真 key 从 `DEEPSEEK_API_KEY` 在测试**运行期**读(本进程 env,cargo test 继承自启动
//! shell 的 env),**永不进 commit**。`CODEAGENT_E2E` 是开关、非 key。

/// 真 DeepSeek api key 的环境变量名 —— 与 codeagent.toml / config.rs 一致。
pub const KEY_ENV: &str = "DEEPSEEK_API_KEY";

/// 开关环境变量名:非空才跑真 key 端到端;未设则 skip。
pub const E2E_FLAG_ENV: &str = "CODEAGENT_E2E";

/// 在测试函数开头调:返回 `Some(secret_key)` 表示「跑」;返回 `None` 表示「skip」
/// (函数内 `eprintln!` 提示后早 return)。区别于 `#[ignore]`:skip 记 pass、不进 ignored 计数。
///
/// 用法:
/// ```ignore
/// let Some(_key) = common::guard_e2e_or_skip() else { return; };
/// // 真 key 路径(子进程自带 env 继承父,无需显式传 key)
/// ```
///
/// 注意:本 helper **不**返回 key 内容给调用方充 env —— 真实路径是「子进程继承父 env」,
/// 父(cargo test)已 set `DEEPSEEK_API_KEY`(注册表 User 级 + 当前 shell 同步),子进程
/// 默认继承,无需显式注入。这里读一次仅为 gate(确认 key 就位才跑,否则给清晰 skip 提示)。
pub fn guard_e2e_or_skip() -> Option<String> {
    if std::env::var(E2E_FLAG_ENV).unwrap_or_default().is_empty() {
        eprintln!(
            "skipped: set {}=1 (and {} for real-key tests) to run real-key e2e",
            E2E_FLAG_ENV, KEY_ENV
        );
        return None;
    }
    match std::env::var(KEY_ENV) {
        Ok(k) if !k.is_empty() => Some(k),
        Ok(_) => {
            eprintln!(
                "skipped: {} set but empty (need non-empty {})",
                E2E_FLAG_ENV, KEY_ENV
            );
            None
        }
        Err(_) => {
            eprintln!("skipped: {} set but {} not in env", E2E_FLAG_ENV, KEY_ENV);
            None
        }
    }
}
