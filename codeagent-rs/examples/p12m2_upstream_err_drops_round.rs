//! P12 候选 #2 析因实证 —— 主 agent `run_one_turn` 上游 Err 路径**漏落盘 + 漏 truncate**
//! (M2 候选,真撞实证 / 不臆造)。
//!
//! **背景(承接 §17.1 P12-1)**:P12-1 修了主 agent client 兜底 timeout(根因层),
//! 但 `run()` 主循环 line 1140-1151 `run_one_turn(...).await?` 是裸 `?` ——
//! 当上游返非 2xx(500/502/429)/reqwest send 失败/流读字节失败/错误帧时,
//! `run_one_turn` 返 `Err` → 裸 `?` 把 Err **直接 propagate 出 run() → main → 进程非 0 退**。
//! 这条路径**不经过**:
//!   · line 1199 `else { messages.truncate(pre_turn_len) }`(finished=false 才进)
//!   · line 1196 `session::save`(finished=true 才进)
//!   · line 872 `exit_repl`(干净退出 /quit/Eof 才进)
//! 故**本轮 user**(已 line 1136 push 进内存 `messages`)随进程退丢失,**磁盘 session.json
//! 仍是上一轮(K-1)完成态**,不含本轮 user。用户 `--resume` 拿 K-1 版本,本轮问的一句话
//! 没了——得重问。
//!
//! **关键判规(与 P12-1 同门槛)**:
//!   · 触发端 = 上游 Err(外部依赖概率事件,性质偏 P10-4「外部依赖不可控」非 P10-2/P10-5
//!     「确定性控制流缺陷」);
//!   · 但损害链是**确定性**控制流缺陷——给定上游 Err,必然本轮 user 不落盘 + 进程退,
//!     非概率 race;
//!   · 读码看不见显式「Err 路径故意丢本轮 user」设计声明;`?` propagate 是 Rust 惯用法,
//!     注释只覆盖 finished=false 正常返路径(line 1152-1156)的 truncate 语义,Err 路径无说明。
//!     属「正常返路径有清理,Err 路径漏清理」的遗漏(类比 P10-2「正常收盘有 pop,打断轮漏 pop」)。
//!
//! **真修价值评估(B 派 vs A 派)**:
//!   · A 派「补对称清理 + 仍 propagate 退」:Err 路径也 truncate + save 干净态再退 ——
//!     但 K-1 本就已在磁盘(上一轮 finished=true 已 save),此派修等于啥都没改(空修);
//!   · B 派「catch + continue REPL」:catch run_one_turn Err → truncate 本轮 user + eprintln
//!     + **continue 回 REPL** 不退。损害面收窄更彻底(进程活着用户立刻重问)。
//!     **但改 fatal 语义有副作用**:API key 无效 / provider 不可达 这类「每轮都 Err」硬错误,
//!     continue REPL 让用户在 REPL 里每轮撞错、不死不活反复弹错,体验不如直接退让用户改 config。
//!
//! **真修代价 > 收益** → 判「真撞到损害但属已知设计取舍」证否不修(同 P10-4「真撞到但证否」
//! 同款细分变种:P10-4 = 未撞证否,M2 = 撞到证否)。损害轻微(本轮问一句话重打成本低),
//! fatal 退是 Rust `fn main() -> Result` 惯用语义非 bug,前面成功轮 K-1 已落盘没丢。
//!
//! **本闸的活**:真起 production codeagent --script 进程对 mock 上游两轮:
//!   · round 1 mock 回合法 OpenAI streaming response(stop + [DONE])→ finished=true → 落盘 round 1;
//!   · round 2 mock 回 HTTP 500 → run_one_turn Err → 裸 ? propagate → 进程非 0 退;
//! 验磁盘 `.codeagent_session.json`:
//!   · **含** round 1 user "第一轮问好" + assistant "好"(证前面成功轮没丢——修正侦察 agent 误述
//!     「前面 N 轮全丢」,P7 一轮一 save 语义下 K-1 落盘保留);
//!   · **不含** round 2 user "第二轮问再见"(证本轮 user 丢失 = 真坑损害现场)。
//!
//! **判读(单态证否闸,非红绿翻转)**:
//!   · `REGRESSION_CURRENT_ROUND_LOST`(真坑现场):磁盘含 round 1 user + 不含 round 2 user +
//!     进程非 0 退 = M2 控制流缺陷确实发生 → 证「真撞到损害」(下一步判真修 vs 证否,本闸判证否);
//!   · 若子进程**没退**(timeout 撞钟)→ M2 描述错(上游 Err 被某层兜了)或 mock 500 没触发 →
//!     `UNEXPECTED_NO_EXIT` 标异常让人查(可能真坑性塌,记为未撞证否);
//!   · 若磁盘**含** round 2 user → 描述错(Err 路径竟落盘了)→ `ROUND2_PRESERVED` 标真坑证否
//!     (损害未发生,P10-4 同款未撞证否)。
//!
//! **Gate 语义**(对齐 p12_main_agent_upstream_hang / p105 / env-gate 范式,**非 `#[ignore]`**):
//!   · `CODEAGENT_P12M2_GATE` 非空 = 跑真撞闸;
//!   · 未设 = eprintln skip + 0 退出(CI 默认不耗真起 codeagent 子进程)。
//!
//! 跑法:
//!   · 默认 skip:`cargo run --example p12m2_upstream_err_drops_round`(0s skip)
//!   · 开闸:`CODEAGENT_P12M2_GATE=1 cargo run --example p12m2_upstream_err_drops_round`
//!     (~5-10s 真起两轮 codeagent --script → round 2 撞 500 → 进程退 → 验磁盘)

use codeagent::config::Config; // 确认临时 toml 能被生产 Config 解析,锁字段齐(对齐 p105/p12)
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
    let on = !std::env::var("CODEAGENT_P12M2_GATE")
        .unwrap_or_default()
        .is_empty();
    if !on {
        eprintln!(
            "[p12m2] skipped: set CODEAGENT_P12M2_GATE=1 to run the upstream-err-drops-round \
             factoring test (real codeagent subprocess; ~5-10s)."
        );
    }
    on
}

/// 合法 OpenAI Chat Completions streaming response(同 p12 echo 模式 body),让 round 1
/// 走完 send→chunk→finalize→finished=true → 落盘 round 1。Content-Length 让 reqwest
/// send 在读完指定字节后确认响应头解析完成(避免裸 Connection: close 让 reqwest 等
/// chunked terminator 报错退造成 round 1 误判)。
fn legal_streaming_response() -> String {
    let body = concat!(
        // content 增量帧
        "data: {\"choices\":[{\"delta\":{\"content\":\"好\"},\"index\":0}]}\n\n",
        // stop 收尾帧
        "data: {\"choices\":[{\"delta\":{},\"index\":0,\"finish_reason\":\"stop\"}]}\n\n",
        // usage 帧(stream_options.include_usage=true 触发,见 main.rs:289)
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\n",
        // 显式 DONE
        "data: [DONE]\n\n",
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body,
    )
}

/// HTTP 500 响应(触发 reqwest error_for_status Err 路径,流式路径 main.rs:301-302)。
fn http_500_response() -> String {
    let body =
        "{\"error\":{\"message\":\"mock upstream 500 for round 2\",\"type\":\"server_error\"}}";
    format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body,
    )
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if !gate_on() {
        return;
    }
    eprintln!(
        "[p12m2] === P12 候选 #2 析因实证:run_one_turn 上游 Err → 本轮 user 不落盘 + 进程退 ==="
    );

    // 1. mock 上游:按连接计数回不同响应。
    //    · conn 1(round 1)→ 合法 OpenAI streaming response → finished=true → 落盘 round 1;
    //    · conn 2+(round 2)→ HTTP 500 → error_for_status Err → 裸 ? propagate → 进程非 0 退。
    //    计数从 0 起,第 0+1 条 conn = round 1(cargo 流式默认 stream=true 走 chat_completion_stream)。
    let conn_count = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    eprintln!(
        "[p12m2][mock] 上游听在 {upstream_addr}:conn 1 回合法 streaming response(round 1 落盘),conn 2 回 HTTP 500(round 2 撞 Err)"
    );
    let conn_count_clone = conn_count.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    let idx = conn_count_clone.fetch_add(1, Ordering::SeqCst);
                    eprintln!("[p12m2][mock] accept conn #{idx} (0 起)",);
                    tokio::spawn(async move {
                        let mut stream = stream;
                        // HTTP server 标准行为:先读完 client 发来的 POST 请求(行 + 头 + body),
                        // 再回响应。若不读 body 就 write 响应 + shutdown,Windows TCP 可能发 RST
                        // 而非干净 FIN,reqwest 把它判成 `error sending request`(send 阶段错)——
                        // 这是 mock 协议缺陷非 M2 真坑,P12-1 echo 同款踩坑致「411ms 对照绿」假绿。
                        // 读够缓冲放过整个 request 行即可(Content-Length body 可能很大但不读也行 ——
                        // 关键是别在 client 写 request 阶段就关连接发 RST;读一段 client→server 数据
                        // 表示 server 已 recv,后续 shutdown 走干净 FIN 路径)。
                        let mut req_buf = [0u8; 4096];
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_millis(500),
                            stream.read(&mut req_buf),
                        )
                        .await;
                        // round 1(idx 0)→ 合法 streaming response; round 2+(idx>=1)→ HTTP 500。
                        let resp = if idx == 0 {
                            eprintln!("[p12m2][mock] conn #{idx}: 回 200 streaming response(round 1 正常收工)");
                            legal_streaming_response()
                        } else {
                            eprintln!(
                                "[p12m2][mock] conn #{idx}: 回 HTTP 500(round 2 撞 Err 路径)"
                            );
                            // 短暂 sleep 让 500 不太快回(确定 reqwest 已建好连接后拿到响应行)
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            http_500_response()
                        };
                        let _ = stream.write_all(resp.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    });
                }
                Err(e) => {
                    eprintln!("[p12m2][mock] accept err: {e}");
                    break;
                }
            }
        }
    });

    // 2. 临时 toml 写临时目录,base_url 指向 mock,key env 注任意值(mock 不验真)
    let tmp = std::env::temp_dir().join(format!("p12m2-err-round-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let toml_path = tmp.join("codeagent.toml");
    let key_env = "DEEPSEEK_API_KEY";
    let toml_content = format!(
        r#"default = "mock"

[provider.mock]
base_url = "http://{upstream_addr}"
model = "mock-model"
api_key_env = "{key_env}"
"#
    );
    std::fs::write(&toml_path, &toml_content).unwrap();
    // 确认临 toml 能被生产 Config 解析
    let cfg_src = std::fs::read_to_string(&toml_path).unwrap();
    let cfg: Config = toml::from_str(&cfg_src).expect("临时 toml 应能被生产 Config 解析");
    let p = cfg.default_provider().expect("mock provider 必存在");
    eprintln!(
        "[p12m2] 临时 toml 写于 {} | 解析 OK: base_url={} model={} api_key_env={}",
        toml_path.display(),
        p.base_url,
        p.model,
        p.api_key_env
    );

    // 3. 起真 production codeagent --script 进程,喂两行 user 输入:
    //    round 1 「第一轮问好」→ mock 回 streaming response → finished=true → 落盘 round 1;
    //    round 2 「第二轮问再见」→ mock 回 500 → run_one_turn Err → 裸 ? propagate → 进程非 0 退。
    let bin = locate_codeagent_bin();
    eprintln!("[p12m2] bin = {bin:?}");
    let t0 = std::time::Instant::now();
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("--script")
        .arg("--yolo")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .env(key_env, "fake-not-used-mock-returns-500-on-conn-2")
        .current_dir(&tmp);
    // 不压 read_timeout 窗口:本闸撞的不是 hang(timeout 兜底),而是上游 500 立刻 Err 退。
    // 默认 read_timeout=90s 不会兜到(mock 立刻回 500),进程应在拿到 500 后很快(~秒级)退。
    // 注:不显式设 CODEAGENT_UPSTREAM_* 任何值,让 build_http_client() 走默认值验「默认配置下 M2 真坑发生」。
    let mut child = cmd.spawn().expect("spawn codeagent --script 应成功");
    eprintln!("[p12m2] 真起 production codeagent --script 子进程 (cwd=tmp,两轮撞 mock 上游)");

    // 喂两行 user 输入后 drop stdin → 子 read_script 按行读两轮,各触发一轮 run_one_turn。
    // · round 1:chat_completion_stream 拿 200 → 收 SSE → finish_reason:stop → finished=true → 落盘 round 1
    //   → run() 循环 continue 回 read_script 读第二行;
    // · round 2:chat_completion_stream 拿 500 → error_for_status Err → run_one_turn Err →
    //   run() line 1151 裸 ? propagate → main 退(根本到不了读下一行,也到不了 exit_repl 存盘)。
    let mut stdin = child.stdin.take().expect("stdin piped");
    stdin
        .write_all("第一轮问好\n第二轮问再见\n".as_bytes())
        .await
        .expect("write stdin");
    drop(stdin);

    // 4. 外层 tokio::time::timeout 包 child.wait().await (收子进程退出):
    //    · M2 真坑现场:round 2 拿 500 → 进程非 0 退(几秒内);
    //    · 若撞钟(20s 不退)= 异常 —— M2 描述错或 mock 500 没触发该层 Err。
    eprintln!(
        "[p12m2] 外层 20s timeout 包 child.wait() —— round 2 拿 500 后主 agent 应在几秒内非 0 退"
    );
    let outer = tokio::time::timeout(std::time::Duration::from_secs(20), child.wait()).await;
    let elapsed = t0.elapsed();

    // 收子进程残留 stderr(看是否含「调用 ... 失败(流式,第 1 轮)」Err 出口)
    let mut stderr_buf = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            stderr.read_to_end(&mut stderr_buf),
        )
        .await;
    }
    eprintln!(
        "[p12m2] stderr 残留(若有): {}",
        String::from_utf8_lossy(&stderr_buf).trim_end()
    );
    // 收子进程 stdout —— 看 round 1 流式是否真吐了 assistant 正文(content 增量 print! 走 stdout)。
    // · 若 stdout 含「好」字 = round 1 流式成功吐字了 = chat_completion_stream 走到 chunk 阶段;
    // · 若 stdout 空白 = round 1 甚至没进 chunk 就在 send 阶段挂了 = mock 不可用。
    let mut stdout_buf = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            stdout.read_to_end(&mut stdout_buf),
        )
        .await;
    }
    eprintln!(
        "[p12m2] stdout 残留(若有): {}",
        String::from_utf8_lossy(&stdout_buf).trim_end()
    );

    // 5. 判进程退态:M2 真坑现场是「进程非 0 退」,不是挂。
    let exit_status = match &outer {
        Err(_elapsed) => {
            eprintln!(
                "[p12m2] 异常: 20s timeout 撞钟 ({elapsed:.2?}) —— round 2 上游 500 似乎没触发进程退,\
                 M2 描述错或 mock 行为异常需人查。撞钟态子进程已 kill_on_drop 收。"
            );
            eprintln!("[p12m2] === 终判: UNEXPECTED_NO_EXIT (elapsed={elapsed:.2?}) ===");
            std::process::exit(1);
        }
        Ok(Err(wait_io_err)) => {
            eprintln!(
                "[p12m2] 异常: child.wait() IO 失败 ({wait_io_err:?}, {elapsed:.2?}) —— 子进程态判不清,需人查"
            );
            eprintln!("[p12m2] === 终判: WAIT_IO_ERROR (elapsed={elapsed:.2?}) ===");
            std::process::exit(1);
        }
        Ok(Ok(status)) => {
            eprintln!(
                "[p12m2] 子进程退出 ({:?}, {elapsed:.2?}) —— round 2 上游 500 触发进程退(M2 真坑现场)",
                status.code()
            );
            status
        }
    };

    // 6. 关键验收:读磁盘 .codeagent_session.json,验 round 1 user 在 + round 2 user 不在。
    let session_path = tmp.join(".codeagent_session.json");
    let session_json = std::fs::read_to_string(&session_path).unwrap_or_default();
    let has_round1_user = session_json.contains("第一轮问好");
    let has_round2_user = session_json.contains("第二轮问再见");
    let has_round1_assistant = session_json.contains("好");

    eprintln!(
        "[p12m2] 磁盘 session.json 路径: {} | 总长 {} 字节",
        session_path.display(),
        session_json.len()
    );
    eprintln!(
        "[p12m2] 磁盘含 round 1 user \"第一轮问好\": {has_round1_user}(期望 true: P7 一轮一 save 语义下 K-1 应落盘保留)"
    );
    eprintln!(
        "[p12m2] 磁盘含 round 1 assistant \"好\": {has_round1_assistant}(期望 true: round 1 finished=true 落盘)"
    );
    eprintln!(
        "[p12m2] 磁盘含 round 2 user \"第二轮问再见\": {has_round2_user}(期望 false: 本轮 user 已 push 进内存但 Err 路径漏 save → 丢失)"
    );

    // 7. M2 真坑现场判读(单态证否闸,非红绿翻转):
    //   · 期望态 `REGRESSION_CURRENT_ROUND_LOST`(真坑损害确实发生):
    //     round1 user 在 + round1 assistant 在 + round2 user 不在 + 进程非 0 退
    //     = M2 控制流缺陷「Err 路径漏落盘 + 漏 truncate → 本轮 user 丢 + 进程退」实证成立。
    //     记 journey §17.2 为「真撞到损害但属已知设计取舍」证否不修(真修代价 > 收益)。
    //   · 异常态 `ROUND2_PRESERVED`:磁盘竟含 round2 user → M2 描述错(Err 路径竟落盘了)→
    //     真坑证否(损害未发生,P10-4 同款未撞证否)。需人查落盘路径。
    //   · 进程正常退(0)而非非 0 退 → 描述错(上游 Err 竟没让进程退)→ `UNEXPECTED_CLEAN_EXIT` 让人查。
    let verdict = if has_round1_user
        && has_round1_assistant
        && !has_round2_user
        && !exit_status.success()
    {
        "REGRESSION_CURRENT_ROUND_LOST"
    } else if has_round2_user {
        // 磁盘竟含 round2 user → Err 路径竟落盘了 → M2 描述错,真坑塌(损害未发生)
        eprintln!(
            "[p12m2] 磁盘竟含 round 2 user —— Err 路径竟落盘了,M2 损害未发生(描述错)。\
             这等同 P10-4 同款「未撞证否」:真坑塌,记 journey §17.2。"
        );
        "ROUND2_PRESERVED"
    } else if exit_status.success() {
        eprintln!(
            "[p12m2] 子进程竟正常退(0) —— 上游 Err 竟没让进程退,M2 控制流描述错。\
             exit_status.success()=true 即非 M2 预期态。标异常让人查上游 Err 是否被某层兜了。"
        );
        "UNEXPECTED_CLEAN_EXIT"
    } else {
        // 其他组合态(如 round1 user 不在但 round2 不在 = round 1 没落盘?)
        eprintln!(
            "[p12m2] 其他异常组合态: round1_user={has_round1_user} round1_assistant={has_round1_assistant} \
             round2_user={has_round2_user} exit_success={} —— 需人查",
            exit_status.success()
        );
        "UNEXPECTED_COMBO"
    };

    eprintln!("[p12m2] === 终判: {verdict} (elapsed={elapsed:.2?}) ===");
    eprintln!(
        "[p12m2] 闸逻辑:REGRESSION_CURRENT_ROUND_LOST = M2 真坑损害确实发生(实证\"真撞到\")。\
         本闸判证否不真修(真修代价 > 收益:损害轻微、fatal 退是惯用语义、前面轮 K-1 没丢)。\
         ROUND2_PRESERVED = 损害未发生,M2 描述错,真坑塌记未撞证否。两者都不红绿翻转(无修后对照)。"
    );

    // 判读:REGRESSION_CURRENT_ROUND_LOST = 真"撞到损害" 但本闸判证否不真修 → exit 0(实证已记录);
    //       ROUND2_PRESERVED = 损害未发生(真坑塌)→ exit 0(证否成立);
    //       UNEXPECTED_* / WAIT_IO_ERROR = 异常态需人查 → exit 1。
    // 注意:这个闸是「证否实证闸」非「修后回归闸」。M2 判不真修故无红绿翻转。
    // exit 0 表示「实证已记录损害态(无论真坑成立或塌)」;exit 1 只在「闸本身异常需人查」时用。
    if matches!(
        verdict,
        "REGRESSION_CURRENT_ROUND_LOST" | "ROUND2_PRESERVED"
    ) {
        std::process::exit(0);
    }
    std::process::exit(1);
}
