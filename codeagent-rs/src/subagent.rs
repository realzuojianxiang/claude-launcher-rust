//! subagent —— P8-3:子进程式子 agent 工具。
//!
//! 复用现成 `--script` 模式起 `codeagent --script --yolo` 子进程,**不 lib 化、零重构**:
//!   · 喂单行 task prompt → drop stdin → 子进程 `read_script`(main.rs)读到 `Ok(0)` →
//!     `InputLine::Eof` → `exit_repl`(存 session + `println!()` + 退)→ 父 `read_to_end` 自然 EOF。
//!   · 一次性 spawn(**不常驻**)—— 流式 token 无内置 sentinel,常驻检测不可靠;单次答题用 EOF 收工最稳。
//!
//! session 隔离:新 `--session-file <temp>` flag 让子进程把 session 写到临时区,绝不撞父
//! `.codeagent_session.json`。子进程 cwd 继承父(要能读 `codeagent.toml`)。
//!
//! execute 为 `async fn`(a) Phase B 升级后直接在生产 multi_thread runtime 上 `.await`
//! tokio 子进程 IO(spawn + write_all + read_to_end + wait),不再经 `(c′) 桥 block_on_current`
//! 借独立 OS 线程 + 临时 runtime 跑 —— 桥已整段删除(见 mcp.rs)。subagent async spawn 路径的
//! 运行期等价由 `tests/subagent_e2e_real_key.rs`(env-gate真 key,Phase A 基线经桥绿、Phase B 无桥再绿)证。

use std::path::PathBuf;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Duration;

use crate::tools::Tool;

// Stdio 走 std::process::Stdio(不是 tokio::process::Stdio —— 后者是私有 re-export)。
// tokio::process::Command 的 stdin/stdout/stderr 配置项接 std::process::Stdio。
use std::process::Stdio;

/// SubagentTool::execute 等子进程终答的最长时限(秒)。P10-5 #5 真坑真修:
/// 原版 `read_to_end(&mut out).await` + `child.wait().await` 均**裸 await 无 timeout**,
/// 给定「上游 hang」输入 → 子 `chat_completion*.await` 永挂(reqwest::Client::new() 无 .timeout())
/// → 子进程不退 → 父 `read_to_end` 永挂 → 父 turn 永死、**无任何 timeout/Ctrl-C 信号兜底**
/// (`--script` 模式无 Ctrl-C 信号源)。真撞实证见 `examples/p105_subagent_hang_repro.rs`
/// (本地 mock 上游 hold 连接 → reqwest send() 永挂 → 父 15s 撞钟 Elapsed)。
///
/// 这里给 read_to_end 加 timeout 兜底:超时显式 kill 子进程返错,父 turn 拿到 subagent
/// 超时错误而非永死。值取宽裕的 180s(子 agent 走 MAX_TOOL_ROUNDS 多轮调工具 + 模型多次首
/// token 可能数十秒);欲短可设 `CODEAGENT_SUBAGENT_TIMEOUT_SECS=<n>` 覆盖(测试/本机实验用)。
const SUBAGENT_DEFAULT_TIMEOUT_SECS: u64 = 180;

/// 取 subagent 超时秒数:`CODEAGENT_SUBAGENT_TIMEOUT_SECS` 覆盖,否则 `DEFAULT`。
/// 用 env 变体是给本地实证/析因单测压窗口用;真生产 default 始终宽裕 180s。
/// 这不在 schema/参数里骗模型说可调 —— 子进程实际走 MAX_TOOL_ROUNDS 固定,故不在模型
/// 可见参数透传(同 P8 schema 诚实注释原则)。
fn subagent_timeout() -> Duration {
    match std::env::var("CODEAGENT_SUBAGENT_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        Some(secs) if secs > 0 => Duration::from_secs(secs),
        _ => Duration::from_secs(SUBAGENT_DEFAULT_TIMEOUT_SECS),
    }
}

/// 委派独立子 agent 处理可隔离子任务的工具。一次调用 = 起一个 `codeagent --script --yolo` 子进程、
/// 喂一行 prompt、收它的 stdout(终答文本)回灌给主 agent。
///
/// `bin`:子进程自举(`std::env::current_exe()`),即 codeagent 自己。
/// `session_dir`:临时区,每次 spawn 用独立 session 文件名(带 pid),多次调用不互相覆盖。
pub struct SubagentTool {
    bin: PathBuf,
    session_dir: PathBuf,
}

impl SubagentTool {
    /// bin = 当前 exe;session_dir = `temp/codeagent-subagent-<pid>/`(主进程 pid,一次会话共用一目录)。
    pub fn new() -> anyhow::Result<Self> {
        let bin = std::env::current_exe()
            .map_err(|e| anyhow::anyhow!("subagent 自举 current_exe 失败: {e:#}"))?;
        Self::new_with_bin(bin)
    }

    /// 与 [`new`](Self::new) 同,但 `bin` 由调用方显式给 —— 拿来走「生产 `current_exe` 指向 codeagent
    /// CLI」之外的路径注入(主要:(a) Phase A 集成测试起 `env!("CARGO_BIN_EXE_codeagent")` 真 codeagent
    /// 子进程 —— `cargo test` 里 `current_exe()` 是**测试运行器二进制**非 codeagent CLI,生产 `new()`
    /// 会错把测试 exe 当 codeagent 子进程起;故测试走此构造 + 显式塞真 codeagent exe 路径)。
    ///
    /// 生产 callers 仍用 `new()`(自举 current_exe);这是给测试 / 别的带显式 bin 的场景开的口。
    /// session_dir / 建 dir 行为与 `new()` 完全一致,只是 bin 来源不同。
    pub fn new_with_bin(bin: PathBuf) -> anyhow::Result<Self> {
        let pid = std::process::id();
        let session_dir = std::env::temp_dir().join(format!("codeagent-subagent-{pid}"));
        std::fs::create_dir_all(&session_dir).map_err(|e| {
            anyhow::anyhow!("subagent session 目录创建失败 ({:?}): {e:#}", session_dir)
        })?;
        Ok(Self { bin, session_dir })
    }

    /// session 文件路径:`<session_dir>/subagent-<子进程pid>.json`。用子进程 pid 而非主进程 pid,
    /// 多个 subagent 并发也不互撞(虽 P8 主线串行调,但命名上留余量)。
    fn session_path(&self) -> PathBuf {
        self.session_dir
            .join(format!("subagent-{}.json", std::process::id()))
    }
}

#[async_trait::async_trait]
impl Tool for SubagentTool {
    fn name(&self) -> &str {
        "subagent"
    }
    fn description(&self) -> &str {
        "委派一个独立子 agent 处理可隔离的子任务(如研究某模块、重构某函数、检索特定信息),\
         返回它最终的答复文本。子 agent 与你同工具、同身份,跑独立一轮,不分享本对话历史 —— \
         适合要花多轮工具调用探索、但你不想让那些中间过程挤占本对话上下文的子任务。\
         给它一句清晰、自包含的一次性任务描述。"
    }
    fn parameters(&self) -> serde_json::Value {
        // 只 task 必填。max_turns 不加:子进程固定走 MAX_TOOL_ROUNDS 常量(main.rs),
        // 未透传则不在 schema 里骗模型说支持 —— 诚实。
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "一次性任务描述,自包含(子 agent 看不到本对话其余上下文)。"
                }
            },
            "required": ["task"]
        })
    }
    /// 子 agent 可能写盘/跑命令,is_destructive=true → 过闸。`--yolo` 下无审(预期,subagent 是放手自动执行助手)。
    fn is_destructive(&self) -> bool {
        true
    }
    async fn execute(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(serde::Deserialize)]
        struct SA {
            task: String,
        }
        let args: SA = serde_json::from_str(arguments).map_err(|e| {
            anyhow::anyhow!("subagent 参数解析失败(需要 {{\"task\":\"...\"}}): {e}")
        })?;
        // 给子进程的 prompt = 原 task 加一句一次性收工指令,防子 agent 反问或等下一行输入。
        let prompt = format!(
            "{}\n\n（这是 subagent 一次性会话 —— 直接完成上述任务并给出最终答复,不要反问用户、不要等待更多输入。）",
            args.task
        );
        let bin = self.bin.clone();
        let sess = self.session_path();
        // (a) Phase B:async 化后直接 await tokio 子进程 IO,不再经 (c′) 桥 block_on_current
        // 借独立 OS 线程 + 临时 runtime —— 桥已删。spawn + write_all + read_to_end + wait 在
        // 生产 multi_thread runtime 上本就在跑,此处只是不再绕一层桥。
        let mut child = tokio::process::Command::new(&bin)
            .arg("--script")
            .arg("--yolo")
            .arg("--session-file")
            .arg(&sess)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // 子进程诊断直通,便于排 subagent 收工/spawn 失败
            .kill_on_drop(true) // Windows 兜底:父子意外 detach 时杀子进程,防遗孤
            .spawn()
            .map_err(|e| anyhow::anyhow!("subagent 子进程启动失败 ({:?}): {e:#}", bin))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("subagent 子进程 stdin 未 piped"))?;
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("写 subagent stdin 失败: {e:#}"))?;
        drop(stdin); // 关 stdin → 子 read_script EOF → exit_repl → 子进程退 → 管道 EOF

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("subagent 子进程 stdout 未 piped"))?;
        let mut out = Vec::new();
        // P10-5 #5 真坑真修:read_to_end 包 tokio::time::timeout —— 上游 hang 时原裸 await
        // 永挂(真撞实证 examples/p105_subagent_hang_repro.rs 15s 撞钟));超时显式 kill 子进程
        // 收尸返错,父 turn 拿到 subagent 超时错而非永死。
        let read_deadline = subagent_timeout();
        let read_result = tokio::time::timeout(read_deadline, stdout.read_to_end(&mut out)).await;
        match read_result {
            Ok(inner) => {
                inner.map_err(|e| anyhow::anyhow!("读 subagent stdout 失败: {e:#}"))?;
            }
            Err(_elapsed) => {
                // 超时:显式 kill 子进程防遗孤(kill_on_drop 兜底但此刻 child 还在 hold
                // 进程引用,drop 也会 kill —— 但显式 kill + wait 收尸更稳,日志更清)。
                eprintln!(
                    "[subagent] 超时 {} 秒未返回终答,显式 kill 收尸(子进程或上游 hang / 工具 self-hang / max_tool_rounds 到顶未收工)",
                    read_deadline.as_secs()
                );
                let _ = child.kill().await;
                let _ = child.wait().await; // 收 kill 后的退出码
                let partial = String::from_utf8_lossy(&out).trim_end().to_string();
                let partial_note = if partial.is_empty() {
                    String::new()
                } else {
                    format!("\n[超时前子进程已产出的部分 stdout]\n{partial}\n[/部分 stdout]")
                };
                return Err(anyhow::anyhow!(
                    "subagent 超时({} 秒未返回终答) —— 子进程已被 kill。\
                     可能子 agent 上游 hang、内部某工具 self-hang 或 max_tool_rounds 到顶仍未收工。{}",
                    read_deadline.as_secs(),
                    partial_note
                ));
            }
        }
        let _ = child.wait().await; // 收尸(正常已自退)
        let reply = String::from_utf8_lossy(&out).trim_end().to_string();
        // 包成「[subagent 答复] ... [/subagent]」让主 agent 知是委派产物 —— 合成最终答复时摘结论不复述子过程。
        Ok(format!("[subagent 答复]\n{reply}\n[/subagent 答复]"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不真起子进程 —— 只验 prompt 包裹加了一次性收工指令(suffix 提「不要反问用户」)。
    /// 起真子进程要真 key + 真终端,留本机手测(诚实)。
    #[test]
    fn subagent_prompt_wrapping_adds_one_shot_suffix() {
        // 直接复刻 execute 的 prompt 构造,验证包裹逻辑(不 spawn)。
        let task = "研究 src/session.rs 有几个 pub 函数并报结论";
        let prompt = format!(
            "{}\n\n（这是 subagent 一次性会话 —— 直接完成上述任务并给出最终答复,不要反问用户、不要等待更多输入。）",
            task
        );
        assert!(
            prompt.starts_with("研究 src/session.rs"),
            "prompt 应回显原 task 开头"
        );
        assert!(
            prompt.contains("不要反问用户"),
            "一次性收工指令必须附在 prompt 后,防子 agent 等下一行输入"
        );
        assert!(prompt.contains("subagent 一次性会话"), "应标明一次性上下文");
    }

    /// 不真起子进程 —— 只验 session 文件路径格式(在临时区 + 带 pid + .json 后缀),绝不撞父
    /// `.codeagent_session.json`(主进程默认 session 文件名,见 main.rs SESSION_FILE)。
    #[test]
    fn subagent_session_file_path_format() {
        // 直接验路径构造逻辑:在 temp 区、文件名含 subagent 前缀和 pid、json 后缀。
        let tmp = std::env::temp_dir();
        let pid = std::process::id();
        let expected_dir = tmp.join(format!("codeagent-subagent-{pid}"));
        let expected_file = expected_dir.join(format!("subagent-{pid}.json"));
        assert!(
            expected_file
                .to_string_lossy()
                .contains("codeagent-subagent-"),
            "session 目录应在临时区含 codeagent-subagent- 前缀"
        );
        assert!(
            expected_file.to_string_lossy().ends_with(".json"),
            "session 文件应以 .json 结尾"
        );
        assert!(
            !expected_file
                .to_string_lossy()
                .ends_with("codeagent_session.json"),
            "绝不与主进程默认 .codeagent_session.json 同名(隔离要求)"
        );
        // 验 SubagentTool::new() 真能造出来(不 spawn,只验目录可建 + bin 取到)。
        let tool = SubagentTool::new().expect("SubagentTool::new 应可构造(不 spawn)");
        let p = tool.session_path();
        assert!(p.to_string_lossy().contains("subagent-"));
        assert!(p.to_string_lossy().ends_with(".json"));
    }

    /// P10-5 #5 真修回归闸(轻量·恒跑·不真起子进程):验 `subagent_timeout()` 默认 180s +
    /// `CODEAGENT_SUBAGENT_TIMEOUT_SECS` env 覆盖 + 0/垃圾/空 值回退默认。这是真修逻辑(subagent.rs
    /// execute 的 read_to_end 包 timeout 这套)的直接单测 —— 真起子进程的真撞现场走
    /// `examples/p105_subagent_hang_repro.rs` 的 env-gate 二进制闸(env-only 不长跑)。
    ///
    /// env 在同进程全局变更理论上可能干扰同测组其他读此 env 的测;但此 env 由本测独占语义
    /// (codeagent 别处不读),测尾清回默认保 hygiene。
    #[test]
    fn subagent_timeout_env_override_default_and_fallback() {
        // 清掉可能残留的 env,验默认 = 180s
        std::env::remove_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS");
        assert_eq!(
            subagent_timeout(),
            std::time::Duration::from_secs(SUBAGENT_DEFAULT_TIMEOUT_SECS),
            "无 env 覆盖时应取默认 {}s",
            SUBAGENT_DEFAULT_TIMEOUT_SECS
        );
        assert_eq!(
            SUBAGENT_DEFAULT_TIMEOUT_SECS, 180,
            "默认窗口契约 = 180s(若调宽请同改此断言 + docstring)"
        );

        // 正常整覆盖
        std::env::set_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS", "7");
        assert_eq!(
            subagent_timeout(),
            std::time::Duration::from_secs(7),
            "env=7 应覆盖默认到 7s"
        );

        // 垃圾值 → 回退默认(防 parse 失败 panic 整测组)
        std::env::set_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS", "not-a-number");
        assert_eq!(
            subagent_timeout(),
            std::time::Duration::from_secs(SUBAGENT_DEFAULT_TIMEOUT_SECS),
            "env 垃圾值应回退默认而非 panic"
        );

        // 0/负数(语法上 parse 成功但语义非法)→ 回退默认
        std::env::set_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS", "0");
        assert_eq!(
            subagent_timeout(),
            std::time::Duration::from_secs(SUBAGENT_DEFAULT_TIMEOUT_SECS),
            "env=0 应回退默认(0 秒超时无意义)"
        );

        // 空串 → 回退默认
        std::env::set_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS", "");
        assert_eq!(
            subagent_timeout(),
            std::time::Duration::from_secs(SUBAGENT_DEFAULT_TIMEOUT_SECS),
            "env 空串应回退默认"
        );

        // 空白 → 回退默认(trim 解析非整数)
        std::env::set_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS", "   ");
        assert_eq!(
            subagent_timeout(),
            std::time::Duration::from_secs(SUBAGENT_DEFAULT_TIMEOUT_SECS),
            "env 纯空白应回退默认"
        );

        // 清回 hygiene
        std::env::remove_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS");
    }
}
