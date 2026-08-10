//! (a) Phase A §2.3b:模型驱动的真 key 端到端往返 —— `--script --yolo --no-stream` 跑
//! `codeagent` 自己的 exe,喂一句让模型**调 `read_file` 读 temp marker** 的 prompt,截 stdout
//! 断 marker 出现。是 `.e2e/t04` 的 cargo-test 类比 —— 把「真 model API + 真 dispatch_tool +
//! 真 read_file → marker 回灌」整条链罩进 cargo test(有 key 时自跑,无 key 时 skip)。
//!
//! 镜像 §14.3 的关切:refactor `block_on_current`/`Tool::execute` 不该在静默坏运行期路径下过
//! cargo 闸。本测覆盖跟 keystone(`tests/mcp_fake_handshake.rs`)+ `dispatch_tool_read_file_
//! roundtrip_built_in` 不同的维度 —— **模型驱动**:模型先开口要 read_file(真 OpenAI tool_call
//! 协议解码 → dispatch_tool → execute),不只是测试代码直接调 execute。这条链过了 = (a)
//! refactor 没在「模型 → tool_call 字符串 → dispatch_tool → dyn Tool::execute」上遗 stall。
//!
//! 隔离:子进程 cwd 指向本测自建的临时目录,内含本测写的最小隔离 `codeagent.toml`(只
//! deepcopy provider、api_key_env=DEEPSEEK_API_KEY、无 MCP/compaction 段)+ 一份 marker 文件。
//! 子进程读的是这份隔离 toml,不动 crate 根用户真生产 `codeagent.toml`。Key 子进程继承父
//! env(cargo test 已带 `DEEPSEEK_API_KEY`),无需显式注入 —— gate 跑前先从父 env 确认 key 在位。
//!
//! Gate(CODEAGENT_E2E + DEEPSEEK_API_KEY)未就位 → skip(早 return,非 `#[ignore]`),见
//! `common::guard_e2e_or_skip`。无 key 计 CI 永远 skip 留绿;有 key 时 `cargo test` 自跑真链。
//!
//! `flavor = "multi_thread"` **load-bearing** —— 生产 `#[tokio::main]` 多线程,与 keystone 一致;
//! 子进程 spawn + read_to_end + wait 在 awaited body 里(Phase A 现经桥跑,Phase B 删桥后
//! 纯 async)。

mod common;

use std::process::Stdio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// marker 串:随机性低、对手「照写实情」明确(让模型一字不差复述最稳,防它在 stdout 里改写/转述)。
const MARKER: &str = "codeagent-script-e2e-keystone-marker-3c7f-read-this-exact-string-back";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn script_model_drives_read_file_and_returns_marker() {
    // gate:开关 + 真 key 任一不就位 → skip(早 return,pass 但事实 skipped)。
    let Some(_key) = common::guard_e2e_or_skip() else {
        return;
    };

    // —— 搭隔离 cwd ——
    // temp_dir 在不同 OS 下可能含很长的用户名前缀(`C:\Users\<who>\AppData\Local\Temp\`)与
    // 空格(`Local Temp` → `Temp`,但官方 build 环境可能含 `Name WithSpace`)。codeagent 子进程
    // 读 config / write_file 都容空格(对 resolve_under_cwd 无影响),但 cargo test 的并发性要求
    // 每跑独立 cwd —— 用测试函数名 + 后缀拼子目录(无随机源,固定名多次跑覆盖,防残留)。
    let sandbox = std::env::temp_dir().join("codeagent-script-e2e-sandbox");
    std::fs::create_dir_all(&sandbox).expect("建隔离 sandbox 应成功");

    // 写隔离 toml:只 deepcopy 一个 provider,无 [mcp][compaction][approval] 段(走 sane 默认)。
    // 不含真 key(只 api_key_env 指环境变量名),与 crate 根用户真 toml 同形安全可分享。
    let toml = r#"default = "deepseek"
[provider.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
max_context = 1000000
"#;
    std::fs::write(sandbox.join("codeagent.toml"), toml).expect("写隔离 toml 应成功");

    // 写 marker 文件到 sandbox 根(子进程 cwd=此),给 read_file 喂裸名 marker.txt。
    let marker_path = sandbox.join("marker.txt");
    std::fs::write(&marker_path, MARKER).expect("写 marker 文件应成功");

    // —— prompt:让模型用 read_file 读 marker.txt,再把读到的内容原样复述进它的终答(进 stdout) ——
    // 刻意写「一字不差复述」「不要解释/不要改写/不要加引号」—— 模型常把 tool_result 复述成
    // 「读到的内容是: <内容>」之类,marker 串仍会原样出现在 stdout,但加这层提示更稳。
    let prompt = "用 read_file 工具读取当前工作目录下的 marker.txt 文件,然后把读到的内容一字不差、\
         不要加引号、不要解释、不要改写,直接复述进你的终答。";

    // —— 起子进程:--script(管道喂入,无 rustyline) --yolo(读类本不过闸,但显式带保守) --no-stream
    // (非流式走老 chat_completion,输出直接进 stdout 便于一次性 read_to_end 抓全文)。
    let exe = env!("CARGO_BIN_EXE_codeagent");
    let mut child = tokio::process::Command::new(exe)
        .arg("--script")
        .arg("--yolo")
        .arg("--no-stream")
        .current_dir(&sandbox) // cwd=隔离 sandbox,子进程读它下面的 codeagent.toml
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped()) // 不 inherit:失败时单独抓 stderr 进断言信息,不污染测试 stdout
        .kill_on_drop(true) // 测试 panic / 超时退出时 kill 子,防遗孤
        .spawn()
        .expect("起 codeagent --script 子进程应成功");

    // 喂 prompt + 关 stdin → 子进程 read_script 读到 EOF → exit_repl → 子自然退 → 管道 EOF。
    {
        let mut stdin = child.stdin.take().expect("子进程 stdin 应 piped");
        stdin
            .write_all(prompt.as_bytes())
            .await
            .expect("写 prompt 到子进程 stdin 应成功");
        // 显式 drop 触发 EOF(read_script 拿到 0 字节) —— drop 这块作用域结束自动发生,显式表意。
        drop(stdin);
    }

    // 真网络调用 + 模型一轮可能几十秒(DeepSeek 非流式首 token)。设 120s 外层 timeout:
    // 超期 = 真 hang/卡 → FAIL(非静默挂);正常 30-60s 量级远低于此。
    let collect = async {
        let mut stdout_handle = child.stdout.take().expect("子进程 stdout 应 piped");
        let mut stderr_handle = child.stderr.take().expect("子进程 stderr 应 piped");
        let mut out = Vec::new();
        let mut err = Vec::new();
        // 并发抓 stdout + stderr(各 read_to_end 都到 EOF 才退;两路各自独立 EOF)。
        let (r_out, r_err) = tokio::join!(
            stdout_handle.read_to_end(&mut out),
            stderr_handle.read_to_end(&mut err),
        );
        r_out.map_err(|e| anyhow::anyhow!("读子进程 stdout 失败: {e:#}"))?;
        r_err.map_err(|e| anyhow::anyhow!("读子进程 stderr 失败: {e:#}"))?;
        let status = child
            .wait()
            .await
            .map_err(|e| anyhow::anyhow!("等子进程退失败: {e:#}"))?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "codeagent --script 子进程非正常退出({status});stderr:\n{}",
                String::from_utf8_lossy(&err)
            ));
        }
        Ok::<_, anyhow::Error>((out, err))
    };
    let (stdout, stderr) = tokio::time::timeout(std::time::Duration::from_secs(120), collect)
        .await
        .expect("codeagent --script 真链应在 120s 内 resolve;挂/卡 = FAIL")
        .expect("收集子进程 stdout/stderr 应成功");

    let stdout_s = String::from_utf8_lossy(&stdout);
    assert!(
        stdout_s.contains(MARKER),
        "模型驱动 read_file 往返应在 stdout 回灌 marker 串;实得 stdout:\n{stdout_s}\n\
         ── stderr(诊断):\n{}",
        String::from_utf8_lossy(&stderr)
    );

    // 收尾:删隔离 sandbox(连同 toml + marker)。失败也不影响下跑(每跑先 create_dir_all + write 覆盖)。
    let _ = std::fs::remove_dir_all(&sandbox);
}
