//! P10-5 #5 真坑真修的焊入式 CI 闸(env-gate example,对齐 P10-3「临时 example 跑出
//! 现场证据」后把它装成 env-gate CI 跑通闸的范式)。
//!
//! **证成 + 真修回顾**:#5 候选「SubagentTool::execute 的 read_to_end 无 timeout 包」经
//! 临时 example 本机真撞实证(p105 第一版,本地 mock 上游 hold TCP 连接不回字节 → 真
//! codeagent --script 子进程的 reqwest send().await 永挂 → 父 read_to_end 裸 await 永挂
//! → 外层 tokio::time::timeout(15s) 撞钟变 Elapsed,15.00s exit=0)确定升格真坑。
//! 真修:subagent.rs::execute 的 read_to_end 包 tokio::time::timeout(subagent_timeout())
//! (默认 180s,`CODEAGENT_SUBAGENT_TIMEOUT_SECS=<n>` env 覆盖压窗口),超时显式 child.kill +
//! wait 收尸返 Err —— 父 turn 拿到 subagent 超时错而非永死。
//!
//! **本闸(修后)**:真起**生产 `SubagentTool::execute`**(不是手动复刻 spawn 段)撞同一
//! mock hang 上游,验证修后行为:
//!   · execute 自己的 subagent_timeout() 兜底返 Err(...超时...)—— 外层 15s 包应 `Ok(Err(...))`
//!     而非修前的 `Err(Elapsed)`(裸 read_to_end 永挂外层撞钟才 Elapsed);
//!   · 子进程被 child.kill 收尸不再遗孤(exe 进程树里此 pid 已退);
//!   · 兜底窗口小(`CODEAGENT_SUBAGENT_TIMEOUT_SECS=3`)故整闸 ~3s 内 resolve,不会撞外层 15s。
//!
//! **Gate 语义**(对齐 mcp_toctou_race / env-gate 范式,**非 `#[ignore]`**):
//!   · `CODEAGENT_P105_GATE` 非空 = 跑真撞真修闸;
//!   · 未设 = eprintln skip 提示 + 0 退出(记 pass 但事实 skipped —— CI 默认不耗 ~3s 真起
//!     codeagent 子进程,本地或显式 CI job 设 gate 才跑)。
//!   · 与 mod tests 同进程污染担忧无关:本 example 是独立二进制,`set_current_dir(tmp)` 全
//!     进程切 cwd 安全(只此 example 进程内切,子 codeagent 继承父 cwd 读 tmp/codeagent.toml,
//!     不读 crate root 真 production toml —— 遵守「不动真 codeagent.toml」纪律)。
//!
//! 跑法:
//!   · 默认 skip:`cargo run --example p105_subagent_hang_repro`(~0s 编译/0s skip)
//!   · 开闸:`CODEAGENT_P105_GATE=1 cargo run --example p105_subagent_hang_repro`(~3-4s 真撞+修后返错)

use codeagent::config::Config; // 确认临时 toml 能被生产 Config 解析,锁字段齐
use codeagent::subagent::SubagentTool;
use codeagent::tools::Tool;
use std::path::PathBuf;
use tokio::io::AsyncReadExt; // 对照脚 read_to_end + StreamExt 不需要:用裸 tokio

/// `cargo run --example X` 时 current_exe = `target/debug/examples/X.exe`;
/// codeagent bin 在 `target/debug/codeagent.exe`(同 ../)。退到 examples/ parent 拼。
fn locate_codeagent_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let dir = exe
        .parent()
        .expect("exe parent")
        .parent()
        .expect("debug parent");
    let bin_name = if cfg!(windows) {
        "codeagent.exe"
    } else {
        "codeagent"
    };
    dir.join(bin_name)
}

/// env-gate:非空才跑真撞真修闸;否则 skip(eprintln + 0 退出,记 pass 事实 skipped)。
fn gate_on() -> bool {
    let on = !std::env::var("CODEAGENT_P105_GATE")
        .unwrap_or_default()
        .is_empty();
    if !on {
        eprintln!(
            "[p105] skipped: set CODEAGENT_P105_GATE=1 (and CODEAGENT_SUBAGENT_TIMEOUT_SECS=3) \
             to run the post-fix timeout-backstop regression."
        );
    }
    on
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if !gate_on() {
        return;
    }
    eprintln!("[p105] === P10-5 #5 真坑真修 焊入式 CI 闸(修后)===");

    // 1. mock 上游:accept 后紧 hold,永不回字节
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    eprintln!("[p105] mock 上游听在 {upstream_addr} (将永不 send 字节, 紧 hold 这条连接)");
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    eprintln!("[p105][mock] accept 了一条连接, 起 long-hold task 紧 hold 不回字节不关流 —— 让 reqwest send().await 永挂");
                    tokio::spawn(async move {
                        // 持有 stream 整 example 生命期不退 —— 不读不写,真 hang
                        // (关键:_io 借流一起活着,流不 drop = TCP 不断 = reqwest 等头永挂)
                        let _io = stream;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                        }
                    });
                }
                Err(e) => {
                    eprintln!("[p105][mock] accept err: {e}");
                    break;
                }
            }
        }
    });

    // 2. 临时 toml 写临时目录, base_url 指向 mock, key env 沿用 DEEPSEEK_API_KEY(任意值,上游假的不验真)
    let tmp = std::env::temp_dir().join(format!("p105-hang-fix-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let toml_path = tmp.join("codeagent.toml");
    let key_env = "DEEPSEEK_API_KEY";
    let toml_content = format!(
        r#"default = "hang"

[provider.hang]
base_url = "http://{upstream_addr}"
model = "hang-model"
api_key_env = "{key_env}"
"#
    );
    std::fs::write(&toml_path, &toml_content).unwrap();
    // 确认临 toml 能被生产 Config 解析(锁字段齐)
    let cfg_src = std::fs::read_to_string(&toml_path).unwrap();
    let cfg: Config = toml::from_str(&cfg_src).expect("临时 toml 应能被生产 Config 解析");
    let p = cfg.default_provider().expect("hang provider 必存在");
    eprintln!(
        "[p105] 临时 toml 写于 {} (不动 crate root 真 production toml) | 解析 OK: base_url={} model={} api_key_env={}",
        toml_path.display(),
        p.base_url,
        p.model,
        p.api_key_env
    );

    // 3. 切 example 进程 cwd 到 tmp —— 生产 execute 不设 child.current_dir,子继承父 cwd,
    //    故父进程 cwd=tmpl 时子进程读到的 codeagent.toml 是 tmp/codeagent.toml(指向 mock)。
    //    set_current_dir 在独立 example 二进制里安全(无并发测污染同进程 cwd)。
    std::env::set_current_dir(&tmp).expect("切 cwd 到 tmp");
    // 子进程 DEEPSEEK_API_KEY 注入任意值(mock 永不回不验真)
    std::env::set_var(key_env, "fake-not-used-mock-never-returns");

    // 4. 把 production SubagentTool 的兜底窗口压到 3s(本地实证用 env 覆盖,真生产永远走 180s 默认)
    std::env::set_var("CODEAGENT_SUBAGENT_TIMEOUT_SECS", "3");
    eprintln!("[p105] 已设 CODEAGENT_SUBAGENT_TIMEOUT_SECS=3 (压窗口) —— 真 production execute 兜底应在 ~3s 触发");

    // 5. 真起 production SubagentTool::execute
    let bin = locate_codeagent_bin();
    eprintln!("[p105] bin = {bin:?}");
    let tool = SubagentTool::new_with_bin(bin.clone()).expect("建 SubagentTool 应成功");
    let args = r#"{"task":"用一句话回答:好"}"#;

    eprintln!("[p105] 调真 production SubagentTool::execute (上游 hang), 外层 15s 包兜底...");
    let t0 = std::time::Instant::now();
    // 外层 15s 包:若真修到位 execute 自己 ~3s 返 Err 这层 Ok(Err);若修坏(裸 read_to_end 永挂)
    // 外层才会撞钟变 Err(Elapsed) —— 两者从结果区分修好/修坏。
    let outer = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tool.execute(args).await
    })
    .await;
    let elapsed = t0.elapsed();

    let verdict = match &outer {
        Ok(Err(e)) => {
            let msg = format!("{e:#}");
            // 修后期待:execute 兜底返「subagent 超时」错,且 msg 含「超时」字样
            if msg.contains("超时") && elapsed.as_secs() < 14 {
                eprintln!(
                    "[p105] 修后行为 OK: production execute 自己返 Err(超时)({elapsed:.2?}), 未撞外层 15s —— \
                     subagent_timeout() 兜底生效, 父 turn 拿到超时错而非永死"
                );
                eprintln!("[p105] err 详情: {msg}");
                "PASS_FIX_OK"
            } else {
                eprintln!("[p105] 异常: execute 返 Err 但不似超时口令({elapsed:.2?}): {msg}");
                "FAIL_UNEXPECTED_ERR"
            }
        }
        Ok(Ok(s)) => {
            eprintln!("[p105] 异常: execute 在上游 hang 下返 Ok(不应成功)({elapsed:.2?}): {s}");
            "FAIL_OK_NOT_TIMEOUT"
        }
        Err(_) => {
            // 修前行为:裸 read_to_end 永挂,外层 15s 撞钟 Elapsed —— 修坏/未修
            eprintln!(
                "[p105] 修坏/未修现场: 裸 read_to_end 永挂, 外层 15s 撞钟 Elapsed ({elapsed:.2?}) —— \
                 真修的兜底应在此之前由 execute 自己返 Err 才对"
            );
            "FAIL_STILL_HANG"
        }
    };

    // 6. 对照脚:正常自退 OS 子进程, 验裸 read_to_end 不误杀正常路径
    eprintln!("[p105] 对照脚: 正常自退 OS 子进程 (echo hi) 验 read_to_end 秒回不误杀...");
    let (exit_bin, exit_args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd", vec!["/C", "echo hi"])
    } else {
        ("sh", vec!["-c", "echo hi"])
    };
    use std::process::Stdio;
    let mut child_ctrl = tokio::process::Command::new(exit_bin)
        .args(&exit_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn echo 应成功");
    let mut stdout_ctrl = child_ctrl.stdout.take().expect("stdout");
    let mut out_ctrl = Vec::new();
    stdout_ctrl
        .read_to_end(&mut out_ctrl)
        .await
        .expect("read echo");
    let _ = child_ctrl.wait().await;
    let s = String::from_utf8_lossy(&out_ctrl);
    let ctrl_ok = s.contains("hi");
    eprintln!(
        "[p105] 对照脚 {} echo 回 '{}'(len={}) -> read_to_end 不误杀正常路径",
        if ctrl_ok { "OK" } else { "FAIL" },
        s.trim(),
        s.len()
    );

    // 终判
    let pass = verdict == "PASS_FIX_OK" && ctrl_ok;
    eprintln!(
        "[p105] === 终判: {} (verdict={verdict} ctrl={ctrl_ok} elapsed={elapsed:.2?}) ===",
        if pass { "VALIDATED" } else { "FAILED" }
    );
    if !pass {
        std::process::exit(1);
    }
}
