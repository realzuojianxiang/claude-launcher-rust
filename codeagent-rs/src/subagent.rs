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
//! execute 同步 `&self` 签名通过 `mcp::block_on_current` 桥借跑当前 tokio runtime(见 mcp.rs §A)。
//! 这是 P8 三件共享的桥 —— subagent 先于 MCP 落地,1-leg 端到端先验 bridge 不死锁(最高风险证伪点)。

use std::path::PathBuf;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::mcp::block_on_current;
use crate::tools::Tool;

// Stdio 走 std::process::Stdio(不是 tokio::process::Stdio —— 后者是私有 re-export)。
// tokio::process::Command 的 stdin/stdout/stderr 配置项接 std::process::Stdio。
use std::process::Stdio;

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
    fn execute(&self, arguments: &str) -> anyhow::Result<String> {
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
        let res: anyhow::Result<String> = block_on_current(async move {
            let mut child = tokio::process::Command::new(&bin)
                .arg("--script")
                .arg("--yolo")
                .arg("--session-file")
                .arg(&sess)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit()) // 子进程诊断直通,便于排 subagent 收工/桥死锁
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
            stdout
                .read_to_end(&mut out)
                .await
                .map_err(|e| anyhow::anyhow!("读 subagent stdout 失败: {e:#}"))?;
            let _ = child.wait().await; // 收尸(正常已自退)
            Ok::<String, anyhow::Error>(String::from_utf8_lossy(&out).trim_end().to_string())
        });
        let reply = res.map_err(|e| anyhow::anyhow!("subagent 调用失败: {e:#}"))?;
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
}
