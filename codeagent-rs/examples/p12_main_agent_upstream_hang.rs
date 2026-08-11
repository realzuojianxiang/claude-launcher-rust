//! P12 候选 #1 析因实证 —— 主 agent `chat_completion*` 裸 await 上游 hang(P10-5
//! acknowledged scope 外的第二 + 第三环,真撞实证 / 不臆造)。
//!
//! **背景**:P10-5 #5 析因单测的旁证链(main.rs::subagent_read_to_end_hangs_forever_on_non_exiting_child
//! 注释 1467-1468)已把事实链列清:
//!   [父 read_to_end 无 timeout] ⊕ [子 run_one_turn 主循环 chat_completion*.await 裸 await 上游]
//!   ⊕ [reqwest::Client::new() 裸构无 .timeout()] ⊕ [--script 无 Ctrl-C 信号源挂中断]
//!   → 全链无超时兜底。
//! P10-5 修了**第一环**(父 read_to_end 加 tokio::time::timeout 兜底,subagent 子进程那一头)。
//! **后三环是 acknowledged 但 scope 外未修**:第二环「子 run_one_turn 主循环
//! chat_completion*.await 裸 await 上游」、第三环「reqwest::Client::new() 裸构无 .timeout()」、
//! 第四环「--script 无 Ctrl-C 信号源挂中断」。
//!
//! 主 agent 自己(非 subagent 路径)直接调 `chat_completion` / `chat_completion_stream`,client
//! 同样 `reqwest::Client::new()`(main.rs:857)裸构无 `.timeout()`。上游 hang 时:
//!   · `chat_completion`:`.send().await`(main.rs:106)裸 await 无 select —— 无任何模式救得了;
//!   · `chat_completion_stream`:`.send().await`(main.rs:235)裸 await 在 chunk 循环之外 ——
//!     Ctrl-C select 挂在 chunk 内(262-343),send 阶段 hang 连循环都进不去,interrupt 也救不了。
//! 主用例区分:
//!   · TTY 流式:chunk 阶段有 Ctrl-C 兜底,但 send 阶段 hang 救不了;
//!   · `--no-stream` / `--script`:连 chunk 阶段都没 Ctrl-C(send 也不挂),全链无超时兜底。
//!
//! **候选真坑性**:确定性控制流缺陷(同 P10-5 性质)—— 裸 await 上游 + 无 timeout 包 + 无
//! interrupt 挂点(send 阶段)= 上游 hang 确定性永挂,非概率 race。损害 = 整 agent turn 永死,
//! `--script` 用户喂 turns 会被一条 hang 的上游卡死整个批跑(P6.1 长会话曲线采集场景)。
//!
//! **本闸的活**:真起 production codeagent 进程对 mock hang 上游(TCP listener accept 后紧 hold
//! 不回字节,与 p105 同范式),外层 `tokio::time::timeout` 包,断**主 agent 裸 await 上游 hang 永挂**
//! → 外层撞钟变 Elapsed。对照脚:正常自退上游(echo)验不误杀正常路径。
//! 但「真起整 codeagent --script 进程」要真 spawn 子进程 + 注入 cwd + env codeagent.toml——p105
//! 已证此范式可行。本闸复刻之,撞**主 agent 路径**(非 subagent)。
//!
//! **Gate 语义**(对齐 p105 / mcp_toctou_race / env-gate 范式,**非 `#[ignore]`**):
//!   · `CODEAGENT_P12_GATE` 非空 = 跑真撞闸;
//!   · 未设 = eprintln skip 提示 + 0 退出(记 pass 事实 skipped —— CI 默认不耗真起 codeagent
//!     子进程,本地或显式 CI job 设 gate 才跑)。
//!
//! 跑法:
//!   · 默认 skip:`cargo run --example p12_main_agent_upstream_hang`(0s skip)
//!   · 开闸:`CODEAGENT_P12_GATE=1 cargo run --example p12_main_agent_upstream_hang`
//!     (~~15-20s 真起 codeagent --script 子进程撞 mock hang 上游 → 主 agent send().await 永挂)
//!
//! **判读**:**修后回归闸**(原析因实证闸升格,析因证已落见下方「背景」段里的真撞结论)。
//! hang 模式注入 `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS=5` 压窗口,跑修后 production codeagent:
//!   · 修守住(`FIX_OK_TIMEOUT_ERR_EXIT`):子进程在 ~5s 拿 reqwest read_timeout Err 退 —— 外层
//!     15s **不**撞钟,Ok(Ok(_)) ~5s 内,stderr 含 reqwest timeout err(`build_http_client()` 生效);
//!   · 回归(`REGRESSION_STILL_HANG`):外层 15s 撞钟 Elapsed = 修退化回修前真坑,闸抓住 → 闸红。
//! echo 模式(对照):正常自退上游(立即回合法 OpenAI streaming response + [DONE]),子进程几百 ms
//! 收工退 → `ECHO_NORMAL_EXIT`,验闸不误杀正常路径(对照前实测 73-167ms 退 vs 修前 15s 撞钟可区分)。

use codeagent::config::Config; // 确认临时 toml 能被生产 Config 解析,锁字段齐(对齐 p105)
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

/// env-gate:非空才跑真撞闸;否则 skip(eprintln + 0 退出)。
fn gate_on() -> bool {
    let on = !std::env::var("CODEAGENT_P12_GATE")
        .unwrap_or_default()
        .is_empty();
    if !on {
        eprintln!(
            "[p12] skipped: set CODEAGENT_P12_GATE=1 to run the main-agent-upstream-hang \
             factoring regression (real codeagent subprocess; ~15-20s)."
        );
    }
    on
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if !gate_on() {
        return;
    }
    // mode 分流:hang(默认,真撞) vs echo(对照,验不误杀正常路径)
    // · hang: mock 上游 accept 后紧 hold 不回字节 → reqwest send().await 永挂 → 外层撞钟 Elapsed
    // · echo: mock 上游 accept 后立即回一个合法 OpenAI streaming response + [DONE] + 关连接
    //         → 子进程正常收工退 → 外层 Ok(Ok(0)) 远在 15s 内 —— 证明闸能区分「真 hang」与「正常慢回」
    let mode = std::env::var("CODEAGENT_P12_MODE").unwrap_or_else(|_| "hang".to_string());
    let is_hang = match mode.as_str() {
        "hang" => true,
        "echo" => false,
        other => {
            eprintln!("[p12] 未知 CODEAGENT_P12_MODE={other:?}(合法:hang|echo),default hang");
            true
        }
    };
    eprintln!(
        "[p12] === P12 候选 #1 析因实证:主 agent chat_completion* 裸 await上游 {} (mode={mode}) ===",
        if is_hang { "hang" } else { "对照 echo" }
    );

    // 1. mock 上游:hang 模式 accept 后紧 hold 不回字节(让 reqwest send().await 永挂,与 p105 同范式);
    //    echo 模式 accept 后立即回合法 OpenAI streaming response(含 content + [DONE] + usage 帧)
    //    让子进程正常收工,验闸不误杀正常路径。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    eprintln!(
        "[p12][mock] 上游听在 {upstream_addr} (mode={mode}: {})",
        if is_hang {
            "accept 后紧 hold 不回字节 —— 让主 agent reqwest send().await 永挂"
        } else {
            "accept 后立即回合法 OpenAI streaming response + [DONE] —— 子进程应正常收工退"
        }
    );
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    eprintln!(
                        "[p12][mock] accept 一条连接 —— {}",
                        if is_hang {
                            "紧 hold 不回字节不关流 —— reqwest 等响应头永挂"
                        } else {
                            "回合法 OpenAI streaming response 后关连接"
                        }
                    );
                    tokio::spawn(async move {
                        if !is_hang {
                            // echo 模式:回合法 OpenAI Chat Completions streaming response。
                            // 协议见 src/main.rs ChatRequest / StreamChunk + docs P5-streaming-protocol-notes:
                            //   响应头 Content-Type: text/event-stream;SSE 帧 `data: {...}\n\n` 累积,
                            //   末帧 choices[].finish_reason="stop" + usage 帧 + `data: [DONE]`。
                            // 这条流让主 agent chat_completion_stream 走完 send→chunk→finalize→收工。
                            // Content-Length 让 reqwest send 在读完指定字节后确认响应头解析完成(避免
                            // 裸 Connection: close 让 reqwest 等 chunked terminator 报错退造成对照误判)。
                            //
                            // **M2 撞坑实证修的 mock 协议缺陷**:必须先读掉 client 发来的 POST 请求
                            // (行+头+body)再回响应。若 mock 不读就 write 响应 + shutdown,Windows TCP
                            // 可能发 RST 而非干净 FIN,reqwest 把它判成 `error sending request`(send 阶段错)
                            // —— 子进程 411ms 拿 Err 退,旧闸判读只看「子进程在 15s 内退」不校 success(),
                            // 误判为 ECHO_NORMAL_EXIT「对照绿」。实为假绿:echo 从没真验过正常路径不被误杀。
                            // (M2 真坑接力里同步修两处:① echo mock 读 client body;② echo 判读校 success()=0。)
                            use tokio::io::AsyncWriteExt;
                            let mut stream = stream;
                            // 先读一段 client→server 数据(行+头+部分 body)表示 server 已 recv,
                            // 后续 shutdown 走干净 FIN 路径而非 RST。读够缓冲放过整个 request 行即可。
                            let mut req_buf = [0u8; 4096];
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_millis(500),
                                stream.read(&mut req_buf),
                            )
                            .await;
                            let body = concat!(
                                // content 增量帧
                                "data: {\"choices\":[{\"delta\":{\"content\":\"好\"},\"index\":0}]}\n\n",
                                // stop 收尾帧
                                "data: {\"choices\":[{\"delta\":{},\"index\":0,\"finish_reason\":\"stop\"}]}\n\n",
                                // usage 帧(stream_options.include_usage=true 触发,见 main.rs:226)
                                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\n",
                                // 显式 DONE
                                "data: [DONE]\n\n",
                            );
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                                body.len(),
                                body,
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.shutdown().await;
                            return;
                        }
                        // hang 模式:紧 hold 不回字节不关流
                        let _io = stream;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                        }
                    });
                }
                Err(e) => {
                    eprintln!("[p12][mock] accept err: {e}");
                    break;
                }
            }
        }
    });

    // 2. 临时 toml 写临时目录,base_url 指向 mock,key env 注任意值(mock 不验真)
    let tmp = std::env::temp_dir().join(format!("p12-hang-{}", std::process::id()));
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
    // 确认临 toml 能被生产 Config 解析
    let cfg_src = std::fs::read_to_string(&toml_path).unwrap();
    let cfg: Config = toml::from_str(&cfg_src).expect("临时 toml 应能被生产 Config 解析");
    let p = cfg.default_provider().expect("hang provider 必存在");
    eprintln!(
        "[p12] 临时 toml 写于 {} | 解析 OK: base_url={} model={} api_key_env={}",
        toml_path.display(),
        p.base_url,
        p.model,
        p.api_key_env
    );

    // 3. 起真 production codeagent --script 进程,喂一行 user 输入,撞 mock 上游
    //    (走主 agent run_one_turn → chat_completion_stream 的 send().await 裸 await 路径,
    //     不是 subagent 子进程式委派 —— 这是 P12 候选 #1 直接撞主代理主回路)
    let bin = locate_codeagent_bin();
    eprintln!("[p12] bin = {bin:?}");
    let t0 = std::time::Instant::now();
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("--script")
        .arg("--yolo")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .env(key_env, "fake-not-used-mock-never-returns")
        .current_dir(&tmp);
    // hang 模式:压小读超时窗口到 5s(`CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS=5`),让修后子 agent
    // 的 build_http_client() read_timeout 在 ~5s 兜到 reqwest Err 退 —— 而非修前裸 Client::new()
    // 不读 env、上游 hang 永挂(外层 15s 撞钟 Elapsed)。echo 模式不压窗口(走 default 90s,正常上游
    // 几百 ms 走完不摊到 read_timeout,故 default 即可)。
    if is_hang {
        cmd.env("CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS", "5");
        eprintln!(
            "[p12] hang 模式:注入 CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS=5 压窗口(修后应 ~5s 兜错退)"
        );
    }
    let mut child = cmd.spawn().expect("spawn codeagent --script 应成功");
    eprintln!("[p12] 真起 production codeagent --script 子进程 (cwd=tmp,撞 mock 上游)");

    // 喂一行 user 输入后 drop stdin → 子 read_script 读到一行就进 run_one_turn。
    // · hang 模式:第一轮 chat_completion_stream send().await 就永挂,根本到不了 EOF 收工;
    // · echo 模式:上游正常回,子进程正常收工退(EOF 后 REPL 也退)。
    let mut stdin = child.stdin.take().expect("stdin piped");
    stdin
        .write_all("用一句话回答:好\n".as_bytes())
        .await
        .expect("write stdin");
    drop(stdin);

    // 4. 外层 tokio::time::timeout 包 child.wait().await (收子进程退出):
    //    · hang 模式候选真坑(self send().await 永挂) → 子进程永不退 → 外层撞钟变 Err(Elapsed);
    //    · echo 模式正常收工 → 子进程几秒退 → 外层 Ok(Ok(0)) 远在 15s 内 = 验闸不误杀正常路径;
    //    · hang 模式若意外证否(reqwest 内部兜底返错 / 上游真断) → 子进程很快返错退。
    //    timeout 取 15s:远大于正常路径(echo 模式几秒回),给真 hang 充分撞钟。
    eprintln!(
        "[p12] 外层 15s timeout 包 child.wait() —— {}",
        if is_hang {
            "主 agent 若裸 await 永挂 → 撞钟 Elapsed"
        } else {
            "正常上游应几秒收工退 → Ok(Ok(0)) 远在 15s 内 = 闸不误杀正常路径"
        }
    );
    let outer = tokio::time::timeout(std::time::Duration::from_secs(15), child.wait()).await;
    let elapsed = t0.elapsed();

    // 不管判定哪态,先收子进程残留 stderr 给人看(若撞钟则子进程还挂着,读 stderr 提示)
    let mut stderr_buf = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        // 非阻塞尽量读已产出的 stderr(撞钟态子进程还在 hold,读可能 block,加短 timeout)
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            stderr.read_to_end(&mut stderr_buf),
        )
        .await;
    }

    let verdict = match &outer {
        Err(_elapsed) => {
            if is_hang {
                // 修后回归闸里:hang 模式不应撞钟 —— 子进程的 build_http_client() 应在 ~5s 拿到
                // reqwest read_timeout Err 退(外层 15s 不该 Elapsed)。若到此态 = 回归(修前真坑现场:
                // 裸 Client::new() 不读 CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS env,上游 hang 永挂)。
                eprintln!(
                    "[p12] 回归(修前态): hang 模式外层 15s 撞钟 Elapsed ({elapsed:.2?}) —— \
                     build_http_client() read_timeout 兜底未生效,上游 hang 永挂。修前真坑现场复现。"
                );
                "REGRESSION_STILL_HANG"
            } else {
                // echo 模式不应撞钟:正常上游居然也挂 = 闸或对照脚本身有问题,标异常让人查
                eprintln!(
                    "[p12] 异常(echo 模式不应撞钟): echo 正常上游也挂到 15s Elapsed \
                     ({elapsed:.2?}) —— 对照脚或闸本身有问题,需人查"
                );
                "ECHO_UNEXPECTED_HANG"
            }
        }
        Ok(Err(wait_io_err)) => {
            // child.wait() 自己 IO 失败(ENOTTY 等)—— 子进程态判不清,标异常让人查
            eprintln!(
                "[p12] 异常: child.wait() IO 失败 ({wait_io_err:?}, {elapsed:.2?}) —— \
                 子进程态判不清,需人查"
            );
            "WAIT_IO_ERROR"
        }
        Ok(Ok(status)) => {
            if is_hang {
                // 修后回归闸期望态:hang 模式子进程在 ~5s 内(reqwest read_timeout 兜底)拿 Err 退 ——
                // 非永挂、外层 15s 不撞钟。stderr 应含 reqwest read_timeout err(流式请求发送失败 / operation timed out)。
                eprintln!(
                    "[p12] 修后绿(hang 模式): 子进程 {} ({elapsed:.2?}) —— \
                     build_http_client() read_timeout 兜底生效,上游 hang 在 ~5s 兜错退非永挂。\
                     stderr 应含 reqwest timeout err(见下)",
                    if status.success() {
                        "正常退 (0)".to_string()
                    } else {
                        format!("非正常退 ({:?})", status.code())
                    }
                );
                "FIX_OK_TIMEOUT_ERR_EXIT"
            } else {
                // echo 判读必须校 status.success()=0——子进程拿上游合法响应正常收工才真绿。
                // 旧版只 match Ok(Ok(_)) 一律判 ECHO_NORMAL_EXIT,把「子进程拿 Err 退(code 1)」也
                // 当正常收工误判为「对照绿」——这是诚实性 bug(M2 真撞过程撞出来):echo 模式若 mock
                // 协议有缺陷(reqwest send 阶段撞 RST 退),子进程拿 Err 退(code 1)被旧判读当绿,
                // 假绿掩盖了「echo 从没真验过正常路径」。
                if status.success() {
                    eprintln!(
                        "[p12] 对照绿(echo 模式): 子进程正常收工退 (code=0, {elapsed:.2?}) —— \
                         闸不误杀正常路径确认(校 success()=0)"
                    );
                    "ECHO_NORMAL_EXIT"
                } else {
                    eprintln!(
                        "[p12] 对照红(echo 模式): 子进程非正常退 (code={:?}, {elapsed:.2?}) —— \
                         echo 期望态是 normal 收工退(0);非 0 退 = mock 协议有缺陷(reqwest send 阶段撞 RST) \
                         或 codeagent 路径有真坑。M2 撞坑过程已证 root cause 多为 mock 不读 client body \
                         就 shutdown 致 reqwest RST;若 echo mock 已修读 body 仍非 0 退 = codeagent 真坑需人查。",
                        status.code()
                    );
                    "ECHO_ERR_EXIT"
                }
            }
        }
    };
    eprintln!(
        "[p12] stderr 残留(若有): {}",
        String::from_utf8_lossy(&stderr_buf).trim_end()
    );

    eprintln!("[p12] === 终判: {verdict} (elapsed={elapsed:.2?}) ===");
    // 修后回归闸判读(两模式各自期望态):
    // · hang 模式期望 FIX_OK_TIMEOUT_ERR_EXIT(修守住:子进程 ~5s 拿 reqwest read_timeout Err 退非永挂);
    //   若落 REGRESSION_STILL_HANG(外层 15s 撞钟)= 修退化回修前真坑,闸抓住 → exit 1。
    // · echo 模式期望 ECHO_NORMAL_EXIT(对照绿,验闸不误杀正常路径)。
    // 期望态 = pass(exit 0);REGRESSION_WAY / WAIT_IO_ERROR / ECHO_UNEXPECTED_HANG / 异态 = exit 1 让人查。
    // kill_on_drop 兜底:撞钟态子进程仍挂着,exit 前会 kill_on_drop 触发收子(同 p105)。
    if matches!(verdict, "FIX_OK_TIMEOUT_ERR_EXIT" | "ECHO_NORMAL_EXIT") {
        std::process::exit(0);
    }
    std::process::exit(1);
}
