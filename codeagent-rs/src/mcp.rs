//! MCP stdio 客户端 + 统一同步→异步桥。
//!
//! 这一模块是 P8 最重的一块。两件正交的事住在这里:
//!
//! 1. **统一桥 `block_on_current`** —— `Tool::execute(&self, args) -> Result<String>`
//!    是同步 + `&self` 不可变签名(tools.rs),而 MCP / subagent 都要 async tokio 子进程 IO +
//!    可变状态。桥取「当前 tokio runtime 借跑」方案(§A 选 c),不建新 runtime、不动 trait、
//!    不波及现有 5 个 Tool impl。subagent.rs 也 `use` 它。
//!
//! 2. **MCP JSON-RPC 2.0 极窄面客户端** —— 手写,不引 crate。`RpcEnvelope` / `RpcError` 几个
//!    serde struct + 逐行 `serde_json` 往返;`McpClient` 封装一 server 子进程的 stdin/stdout,
//!    后台 read task 把每行解包后按 `id` 扇回到 `request` 的 oneshot;`McpTool` 把一远端工具
//!    包成 `impl Tool`,execute 里走桥借跑 `call_tool`。
//!
//! 诚实边界:握手 / list_tools / call_tool 的真往返要真 MCP server 才能验,本机见 journey §13;
//!    纯函数 / 配置解析层已单测焊(见 config::tests 与本模块测试)。

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{oneshot, Mutex};

use crate::config::McpServerConfig;
use crate::tools::Tool;

// ──────────────────────────────────────────────────────────────────────────
// 超时常量(P9-2:握手超时可配)
// ──────────────────────────────────────────────────────────────────────────

/// 握手 + `tools/list` 阶段每请求的超时基线(秒)。放排在 config 的 sane 默认值也指它,
/// 单处真源。故意比运行期宽:给 `npx -y <pkg>` 首次冷拉包留余量 —— P8 实证 `npx -y`
/// 首拉就占满旧 30s 必超时(journey §13.6 真坑二),60s 让它从容。热包后真握手其实秒回。
/// 可按 server 在 `codeagent.toml` 用 `[mcp.server.*]` 的 `handshake_timeout_secs` 覆盖。
pub const HANDSHAKE_TIMEOUT_SECS_DEFAULT: u64 = 60;

/// 运行期 `tools/call` 每请求的超时(秒)。工具调用卡 30s 多半是坏(死 server / 坏上游),
/// 比握手的 60s 严 —— 不沿用握手放宽的值,避免一个挂的工具拖垮整 agent 回合。
const RUNTIME_TIMEOUT_SECS: u64 = 30;

// ──────────────────────────────────────────────────────────────────────────
// §A 统一桥:同步 execute 借跑当前 tokio runtime 的 async 子进程 IO。
// ──────────────────────────────────────────────────────────────────────────

/// 同步 `Tool::execute` 调 async 子进程 IO 的唯一桥。
///
/// **方案 c′(独立线程 + 独立 runtime)** —— 不是 §A 原先备选的 (c) `Handle::current().block_on`:
/// (c) 实跑时在 runtime 内部线程上 block_on,**直接 panic**「Cannot start a runtime from within
/// a runtime」(subagent 真端到端实证塌,journey §13 路线图标注的「最高风险证伪点」成真)。
/// 根因:`#[tokio::main]` multi-thread runtime 下 agent loop 跑在某个 worker 上,那时同线程
/// 还在驱动该 runtime;`Handle::block_on` 在同一线程上还想再跑 mini reactor = 嵌套,
/// tokio 直接拒(防 deadlock)。
///
/// 真正不踩嵌套的轻退:c′ —— 起一个**独立 OS 线程**,线程内 `Runtime::new()` 建一个**全新独立
/// runtime**(与外层 runtime 不共享 worker 也不共享线程)→ 在这独立 runtime 上 `block_on(future)`
/// → drop runtime → 线程退。外层与内层 runtime 处于两个不同 OS 线程,根本不是「从 runtime 内部
/// 起 runtime」,故不 panic。代价是每次调用起一个短命线程 + 一个短命 runtime(微秒级,subagent/
/// MCP 调用本就以秒计的子进程 IO 为主,这点开销可忽略),换来不动 trait、不动 5 个老 impl。
///
/// 备选 (a) trait async 升级改动面最大(P9 候选),c′ 落地后可延后。
pub fn block_on_current<F>(f: F) -> F::Output
where
    F: std::future::Future + Send,
    F::Output: Send,
{
    // Future + Output 都 Send:独立线程要把 future 移过去跑,结果要移回来。
    let result = std::thread::scope(|scope| {
        let h = scope.spawn(|| {
            // **P9-4 真坑(t04 卡 22min)的实证历程与本桥的最终定位**:
            //
            // 原 (c′) 用 `new_current_thread().enable_all()`。纯 sleep 的 timeout 在该桥上能 ~1s 触发
            // (见 tools.rs `block_on_current_bridge_actually_fires_timeout`),但含子进程管道 IO 的
            // timeout(`read_to_end`)在该桥上**该触发时不触发**:current_thread runtime 同一线程既驱动
            // block_on 轮询又驱动 timer+IO,spawn 出去的 worker task 持续在 `read_to_end` 上 pending
            // (管道缓慢、`cmd /C ping -t` 每秒写 1 行)时,timer 推进被压住,30s 变「永久」(实测卡 22min)。
            //
            // 本轮第一直觉是把 runtime 换成 `new_multi_thread().worker_threads(2)` —— 以为独立 timer
            // driver 线程能解。**实证否决**:析因单测 `block_on_current_timeout_replica_of_bash_inner_
            // with_stderr_drain`(复刻 Bash inner 真形:stderr 排水 task + 单路 stdout read_to_end +
            // child.wait + err_rx.await,outer `tokio::time::timeout(5s)`)在本桥 multi_thread 形上实跑
            // **59.62s**(2 worker)/ **59.95s**(4 worker)才 resolve —— 即 timeout 仍被压到等子进程
            // 自然退,与 worker 数无关。对比 `block_on_current_timeout_io_bound_continuous_stdout_fires`
            // (单 spawn 收单路 stdout、**无第 2 个 spawn task**、outer `timeout(2s)`)→ **2.11s 触发**。
            // → 卡住条件 = 「桥上有**≥2 个 spawn task pending 在子进程管道 IO**(如 stderr 排水 task,
            //   哪怕 inner 主干 future 根本不 await 它)+ outer `tokio::time::timeout`」—— (c′) 桥上
            //   tokio timer 对这一形态稳定「stall」至子进程自然退。multi_thread 不能修(token driver
            //   线程独立了,但 stall 的真因不在「timer 线程被同线程 IO 拖死」)。tokio 官方文档未直告此处张力。
            //
            // **最终解法不在本桥,在 Bash::execute caller 侧**:根本不 `tokio::time::timeout` 包 inner ——
            // 改 spawn 一个**独立 OS 线程**壁钟,sleep 到 deadline 后 `taskkill /T /F /PID` 树杀(连
            // `cmd /C` 的 grandchild 一起灭),子进程死 → stdout 管道 EOF → 单路 read_to_end 自然退 →
            // inner JoinHandle resolve。await 路径无 tokio::time::timeout 包裹,故不踩本桥 timer stall。
            // 实证见 tools.rs `bash_hanging_command_actually_returns_within_timeout`(`ping -t` 真命令、
            // 30s deadline)→ **37.65s 完成**(死代码占位时代卡 22min)+ 无孤儿 ping 残留(树杀 /T 连孙灭)。
            //
            // 桥本身仍留 multi_thread(worker_threads=2):虽未修 stall,但比 current_thread 多一档并发
            // 余量(spawn 的排水/读 task 有真 worker 线程跑、不挤内联 block_on 线程,c′ 后续真端到端
            // 验证手段就位后 (a) async 升级若启动会整段删本桥,故不为「未修的 stall」再返工 current_thread)。
            // enable_all() 含 timer+IO driver。每调一次起 2 worker + 1 timer 线程,短命微秒级建/拆,
            // 工具调用本就以子进程 IO 为主、以秒计,开销可忽略。
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build 内嵌 multi_thread runtime 失败");
            rt.block_on(f)
        });
        h.join().expect("bridge 线程 panic")
    });
    result
}

// ──────────────────────────────────────────────────────────────────────────
// P9: Windows 命令解析 —— PATHEXT(实证撞出,见 journey §13.6)
// ──────────────────────────────────────────────────────────────────────────

/// 解析 MCP server 启动命令,补全 Windows 上裸名(如 `npx`)的全路径。
///
/// **背景**:`McpClient::spawn` 早前用 `tokio::process::Command::new("npx")` ——
/// 直接塌「program not found」(journey §13.6 真坑一)。根因:Windows `CreateProcessW`
/// **不搜 PATHEXT**,而 `npx` / `npm` / `pnpm` 等都是 `.cmd`/`.ps1` 脚本(由 node 安装的
/// shim),`npx` 这个裸名在文件系统上不存在,得用 `npx.cmd`。一旦补全查到全路径,
/// std 的 `Command` 在 Windows 上对 `.cmd`/`.bat` 会自动用 `cmd /C` 包裹(P8 实证给
/// 全路径 `.cmd` 能跑通的就是这条机制)。
///
/// 返回语义:
///   · 非 Windows:**永远返 `None`** —— Unix `execvp` 自带 PATH 查找且无 PATHEXT 概念,
///     让 `Command` 自行处理即可,不为非问题写代码。
///   · Windows + `prog` 已含路径分隔符(如 `./foo`、`C:\...`):**返 `None`** ——
///     调用者想精确指定,不替它再找,避免改变语义。
///   · Windows + `prog` 是裸名:遍历 `PATH` 各目录 × `PATHEXT` 各扩展名,第一命中
///     存在的 `dir\prog.ext` 即返其全路径。全没命中返 `None`(让 `Command` 试一次,
///     它的报错信息更直给用户)。
///
/// 纯函数(只读 `std::env` + 探文件存在),可单测焊住 —— 见本模块 `tests`。
pub(crate) fn resolve_program(prog: &str) -> Option<std::path::PathBuf> {
    // 非 Windows 一律不插手。#[cfg] 把整个函数体编译掉,避免在 Unix 上徒增
    // 无意义的 PATH 扫描(也避免单测里 mock 环境变量时跨平台分歧)。
    #[cfg(not(target_os = "windows"))]
    {
        let _ = prog;
        None
    }
    #[cfg(target_os = "windows")]
    {
        use std::path::Path;

        // 已含路径分隔符(相对/绝对)的 prog:让调用者指定的语义生效,不替它找。
        // `Path::components` 在 Windows 上识别 `\` 和 `/`;裸名 components 只有一段。
        let p = Path::new(prog);
        if p.components().count() != 1 {
            return None;
        }

        let path_env = std::env::var_os("PATH")?;
        // PATHEXT 缺省用 Windows 标准序。例:`.COM;.EXE;.BAT;.CMD;.VBS;...`
        // 取不到也回退这套 —— 保证 `npx.cmd` 这种能找到,不强依赖环境真设了 PATHEXT。
        let pathext = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC".to_string());
        let exts: Vec<String> = pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_string())
            .collect();
        resolve_in_paths(prog, std::env::split_paths(&path_env), &exts)
    }
}

/// `resolve_program` 的纯逻辑核心:给定 prog + 一组候选目录 + 一组扩展名,
/// 返回第一命中(`dir\prog.ext` 且是文件)的全路径。纯函数不读 env,可单测。
///
/// 外层 `resolve_program` 只负责「读 PATH/PATHEXT + 裸名判断」,把数据喂进来。
/// `path_iter` 用迭代器而非 `Vec` —— 真用例从 `split_paths` 惰性产路径,
/// 单测用 `Vec` 模拟,签名两便。
#[cfg(target_os = "windows")]
fn resolve_in_paths<I>(prog: &str, path_iter: I, exts: &[String]) -> Option<std::path::PathBuf>
where
    I: IntoIterator<Item = std::path::PathBuf>,
{
    for dir in path_iter {
        for ext in exts {
            let candidate = dir.join(format!("{prog}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────────
// JSON-RPC 2.0 极窄面 —— 只建用得到的几个字段,其余靠 serde 忽略。
// ──────────────────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 信封。请求/响应/通知三态共用一结构:
///   · 请求:有 `id` + `method` + `params`。
///   · 响应:有 `id` + (`result` 或 `error`),无 `method`。
///   · 通知:无 `id`,有 `method` + `params`(无需回应)。
/// MCP 握手:`initialize`(请求→响应)→ `notifications/initialized`(通知)→ `tools/list`(请求→响应)
///   → 运行期 `tools/call`(请求→响应)。
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub(crate) struct RpcEnvelope {
    jsonrpc: String, // 固定 "2.0"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub(crate) struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

// ──────────────────────────────────────────────────────────────────────────
// McpClient —— 一 server 子进程的封装,多工具共享一连接。
// ──────────────────────────────────────────────────────────────────────────

/// 远端工具的描述(从 `tools/list` 响应取)。 schema 直接当 OpenAI function parameters 用。
#[derive(Debug, Clone)]
pub struct McpToolDesc {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

/// 一 MCP server 的子进程封装。同一 server 暴露的多个工具共享这一个 client(一根 stdin/stdout)。
///
/// 设计:
///   · `stdin` 写命令;后台 read task 逐行解 stdout,按 `id` 扇回到 `request` 的 oneshot。
///   · `next_id` 是 `AtomicI64`(放 `Arc` 内不锁),给每个请求发唯一 id。
///   · `pending` 是 `Arc<Mutex<HashMap<id, oneshot::Sender>>>`:请求挂 oneshot 等,read task
///     拿到匹配 id 的响应就唤醒对应 oneshot。read task 自己持 `pending` 的克隆,无需 mpsc 中转。
/// `Arc<Mutex<Self>>` 是为了同 server 多个 `McpTool` 共享 + 内部可变(后台 task 也能拿 pending 写)。
pub struct McpClient {
    /// 子进程句柄。握手中不直接读它,但**必须持有** —— 它一旦 Drop 就杀子进程(kill_on_drop=true)。
    /// 持有 = 保活;读不到字段但 P8 接入面无生命周期终结调用故编译器判 dead。`shutdown` 会 `wait` 它。
    #[allow(dead_code)]
    child: Child,
    stdin: ChildStdin,
    next_id: AtomicI64,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RpcEnvelope>>>>,
    server_name: String,
    /// 握手 + `tools/list` 阶段每请求的超时(P9-2)。由 spawn 时 `cfg.handshake_timeout_secs`
    /// 解析得到(缺省 `HANDSHAKE_TIMEOUT_SECS_DEFAULT`)。运行期 `tools/call` 不用它,改用
    /// 严的 `RUNTIME_TIMEOUT_SECS` —— 见 `call_tool` 经 `request` 的 timeout 参数。
    handshake_timeout: std::time::Duration,
}

impl McpClient {
    /// 起子进程 + 后台读 task,返 `Arc<Mutex<Self>>`(共享给多个 McpTool)。
    /// 失败返 Err,上层(run)跳过该 server 不致命。
    pub async fn spawn(cfg: &McpServerConfig) -> anyhow::Result<Arc<Mutex<Self>>> {
        // P9: Windows 裸名(npx/npm/pnpm 是 .cmd/.ps1 shim)CreateProcessW 不替你搜 PATHEXT,
        // 直接 `Command::new("npx")` 会塌「program not found」。先 `resolve_program` 补全全路径,
        // 命中就用全路径(交给 std 后它对 .cmd/.bat 自动 `cmd /C` 包裹);没命中退化回原样让
        // Command 试一次 —— 它的报错对用户更直给。非 Windows 此函数恒返 None,零开销透传。
        let resolved = resolve_program(&cfg.command);
        let program = resolved
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new(&cfg.command));
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(&cfg.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit()); // MCP server 的 stderr 直通,便于排错
        if let Some(env) = &cfg.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }
        // Windows 上 tokio::process 默认不建 job object 杀子进程;显式开 kill_on_drop 保险
        // (run scope 退时这个 Arc 也退,子进程应早 EOF 自退,但兜底防僵尸)。
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            // 报错里既给配置写的友好名,也给 spawn 实际用的程序路径(P9 PATHEXT 解析后
            // 可能与 cfg.command 不同),便于诊断「真的起的是哪个文件」。
            anyhow::anyhow!(
                "MCP server `{}` 启动失败 (program={:?} args={:?}): {e}",
                cfg.command,
                program,
                cfg.args
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("MCP server `{}` stdin 未 piped", cfg.command))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("MCP server `{}` stdout 未 piped", cfg.command))?;

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RpcEnvelope>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // 后台 read task:逐行读子进程 stdout、解 RpcEnvelope、按 id 扇回(或丢无 id / 无匹配项)。
        // 它持 pending 的 Arc 克隆;命中 id 就从 pending 取出对应 oneshot 唤醒请求者。
        let pending_clone = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // stdout 关 → 子进程退 → 收工
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<RpcEnvelope>(trimmed) {
                            Ok(env) => {
                                if let Some(id) = env.id {
                                    let mut map = pending_clone.lock().await;
                                    if let Some(sender) = map.remove(&id) {
                                        let _ = sender.send(env); // 没人接说明请求已超时取消,丢
                                    }
                                    // 无匹配 id(迟到响应/被取消的请求):丢弃,不致命。
                                }
                                // 无 id(通知/单边事件):P8 不处理 server→client 通知,丢。
                            }
                            Err(_) => {
                                // 非 JSON 行(MCP server 偶发 stdout 调试):丢弃,不致命。
                            }
                        }
                    }
                    Err(_) => break, // 读错:收工(let child wait 上层处理)
                }
            }
        });

        Ok(Arc::new(Mutex::new(Self {
            child,
            stdin,
            next_id: AtomicI64::new(1),
            pending,
            server_name: cfg.command.clone(),
            // 握手超时:cfg 缺省 None → 走本模块 HANDSHAKE_TIMEOUT_SECS_DEFAULT(60s,npx 首拉余量)。
            handshake_timeout: std::time::Duration::from_secs(
                cfg.handshake_timeout_secs
                    .unwrap_or(HANDSHAKE_TIMEOUT_SECS_DEFAULT),
            ),
        })))
    }

    /// server 友好名(给日志/排错用)。P8 接入面用 `cfg.command` 记日志,这方法留给 P9
    /// 统一接入日志层用,暂标 dead。
    #[allow(dead_code)]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// 发一请求(method+params)、等回响应的 `result`(出错包成 anyhow)。每帧一行 JSON + `\n` + flush。
    ///
    /// `timeout` 由调用者传(P9-2):握手 + `tools/list` 用 `self.handshake_timeout`(宽,
    /// 给 npx 首拉余量);运行期 `tools/call` 用严的 `RUNTIME_TIMEOUT_SECS`。一处 request、
    /// 两档超时,不把「该用哪档」的判定埋进通用方法内。
    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
        timeout: std::time::Duration,
    ) -> anyhow::Result<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let env = RpcEnvelope {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        };
        let mut line =
            serde_json::to_string(&env).map_err(|e| anyhow::anyhow!("RPC 序列化失败: {e}"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("写 MCP stdin 失败: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| anyhow::anyhow!("flush MCP stdin 失败: {e}"))?;

        let (tx, rx) = oneshot::channel::<RpcEnvelope>();
        self.pending.lock().await.insert(id, tx);

        // 等 read task 把匹配 id 的响应扇回来。stdout_rx 在 Self 上,得持锁守着 —— 但其实守的是
        // 「self 没被别人同时 request」(串行化请求)。P8 单 REPL 主线串行调工具,足够。
        let resp = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP `{}` 请求 `{}` {}s 未回应(超时)",
                    self.server_name,
                    method,
                    timeout.as_secs()
                )
            })?
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP `{}` 请求 `{}` 的 read task 通道关闭",
                    self.server_name,
                    method
                )
            })?;

        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!(
                "MCP `{}` 返错误 [{}]: {}",
                self.server_name,
                err.code,
                err.message
            ));
        }
        resp.result
            .ok_or_else(|| anyhow::anyhow!("MCP `{}` 响应无 result 也无 error", self.server_name))
    }

    /// 发一通知(method+params,无 id 不等回)。用于 `notifications/initialized`。
    async fn notify(&mut self, method: &str, params: serde_json::Value) -> anyhow::Result<()> {
        let env = RpcEnvelope {
            jsonrpc: "2.0".into(),
            id: None,
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        };
        let mut line =
            serde_json::to_string(&env).map_err(|e| anyhow::anyhow!("RPC 序列化失败: {e}"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("写 MCP stdin 失败: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| anyhow::anyhow!("flush MCP stdin 失败: {e}"))?;
        Ok(())
    }

    /// 握手:`initialize`(等回)→ `notifications/initialized`(通知)。
    pub async fn handshake(&mut self) -> anyhow::Result<()> {
        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "codeagent", "version": env!("CARGO_PKG_VERSION") },
        });
        let _result = self
            .request("initialize", init_params, self.handshake_timeout)
            .await?;
        // 协议要求 initialize 响应后再发 initialized 通知(单边,无回)。
        self.notify("notifications/initialized", serde_json::json!({}))
            .await?;
        Ok(())
    }

    /// `tools/list` → 拆出工具数组。每个工具取 name/description/inputSchema。
    pub async fn list_tools(&mut self) -> anyhow::Result<Vec<McpToolDesc>> {
        let result = self
            .request("tools/list", serde_json::json!({}), self.handshake_timeout)
            .await?;
        let tools_val = result.get("tools").cloned().ok_or_else(|| {
            anyhow::anyhow!("MCP `{}` tools/list 响应缺 tools 字段", self.server_name)
        })?;
        let arr = tools_val.as_array().cloned().ok_or_else(|| {
            anyhow::anyhow!("MCP `{}` tools/list 的 tools 不是数组", self.server_name)
        })?;
        let mut out = Vec::with_capacity(arr.len());
        for t in arr {
            let name = t
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let description = t
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // inputSchema 缺失就给个空 object(模型看到无参数工具)。
            let schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            if name.is_empty() {
                continue; // 工具没名字:丢,不致命。
            }
            out.push(McpToolDesc {
                name,
                description,
                schema,
            });
        }
        Ok(out)
    }

    /// `tools/call` → 把 `result.content[].text` 拼成一段串返(模型看)。
    /// 失败(isError)或非 text content 兜底转串。
    pub async fn call_tool(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<String> {
        let result = self
            .request(
                "tools/call",
                serde_json::json!({ "name": name, "arguments": args }),
                std::time::Duration::from_secs(RUNTIME_TIMEOUT_SECS),
            )
            .await?;
        let content = result
            .get("content")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));
        let arr = content.as_array().cloned().unwrap_or_default();
        let mut parts = Vec::with_capacity(arr.len());
        for item in arr {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            } else {
                // 非 text content(image/resource 等):P8 不解,转原文保留给模型自己判。
                parts.push(item.to_string());
            }
        }
        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let body = parts.join("\n");
        if is_error {
            Err(anyhow::anyhow!("MCP 工具 `{}` 自己报错: {}", name, body))
        } else {
            Ok(body)
        }
    }

    /// 收尾:等子进程退。run scope 退时调用,正常情况下子进程已 EOF 自退。
    /// P8 未在 run 退出前调它 —— `kill_on_drop` 已兜底杀子进程;显式 graceful shutdown 是 P9 候选
    /// (要解决 `Arc<Mutex<Self>>` 消耗 self 的所有权问题),故暂标 dead。
    #[allow(dead_code)]
    pub async fn shutdown(mut self) {
        // 不强行 kill:让 server 自然收。stdin drop(本 self drop)已向 server 发 EOF 信号。
        let _ = self.child.wait().await;
    }
}

// ──────────────────────────────────────────────────────────────────────────
// McpTool —— 把一远端工具包成 impl Tool(同步 execute 借跑 async call_tool)。
// ──────────────────────────────────────────────────────────────────────────

/// 一个 MCP 工具的工具对象。同一 server 的多个工具共享 `client`(一根连接)。
///
/// `name` 是**对模型/codeagent 侧**展示的工具名(可能带 `prefix` 防与内置工具撞名),
/// `remote_name` 是**对 MCP server** `tools/call` 时用的原名(无前缀)—— **二者必须分开**:
/// prefix 只解决「codeagent 工具表分派 + 模型看到的 OpenAI function name 撞名」问题,
/// server 自己注册的工具表里仍是原名,故 `execute` 发 `tools/call` 必须用 `remote_name`。
/// (实证撞 bug:早前 `execute` 误用带前缀的 `self.name` 发 server,server 回
/// `-32602 Tool fs_list_directory not found` —— 见 journey §13.6 MCP 真握手实证。)
pub struct McpTool {
    client: Arc<Mutex<McpClient>>,
    name: String,
    /// server 原名(无 prefix)。`execute` 发 `tools/call` 必须用这个,**不是** `name`。
    remote_name: String,
    description: String,
    schema: serde_json::Value,
}

impl McpTool {
    pub fn new(client: Arc<Mutex<McpClient>>, prefix: Option<&str>, desc: &McpToolDesc) -> Self {
        // prefix 防撞内置 tool 名(仅 codeagent 侧 + 模型可见的 OpenAI function name);
        // remote_name 始终是 server 原名,execute 发 server 时用它。
        let name = match prefix {
            Some(p) if !p.is_empty() => format!("{}_{}", p, desc.name),
            _ => desc.name.clone(),
        };
        Self {
            client,
            name,
            remote_name: desc.name.clone(),
            description: desc.description.clone(),
            schema: desc.schema.clone(),
        }
    }
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters(&self) -> serde_json::Value {
        self.schema.clone()
    }
    /// 保守判 destructive:不知 MCP 工具有无副作用,一律过闸。`--yolo` 下无审(预期)。
    fn is_destructive(&self) -> bool {
        true
    }
    fn execute(&self, arguments: &str) -> anyhow::Result<String> {
        // 参数解析失败兜底成空 object(无参工具),不让一次解析炸断。
        let args: serde_json::Value =
            serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
        let client = Arc::clone(&self.client);
        // 用 remote_name(server 原名)发 tools/call,不是带 prefix 的 self.name。
        let remote = self.remote_name.clone();
        let res =
            block_on_current(async move { client.lock().await.call_tool(&remote, args).await });
        res.map_err(|e| anyhow::anyhow!("MCP 工具 `{}` 调用失败: {:#}", self.name, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── P9-4 诊断:析因 timeout 不触发究竟归 JoinHandle 还是 IO-bound spawn task。──
    //
    // 已知:t04 实端到端撞「40s timeout 不触发而卡 22min」,且纯 sleep 测 `block_on_current_bridge_
    // actually_fires_timeout` 1s 触发 。这两条之间的差异要析清:卡住是 (a) 仅因 `tokio::spawn` 的
    // JoinHandle(任意 spawn task 都不让 timeout 推进),还是 (b) 因 spawn 的 task 内部在 IO `read_to_end`
    // 上 pending(子进程管道)。诊断测 spawn 一个**纯 sleep** 的 worker task(无 IO)看 `timeout` 能否
    // 触发 —— 若能,排除 (a),卡住与 IO-bound task 强相关。

    /// 仅 spawn+sleep(无 IO):`timeout(1s, JoinHandle(sleep(30s)))` 在 c′ 桥上应在 ~1s 触发 Err(Elapsed)。
    /// **若此测也卡/超时不触发** => 问题在「`tokio::spawn(task) + outer timeout(JoinHandle)` 整组」与桥的
    /// 交互(而非 IO),即只要 outer future 等的是一个 spawn 出去的 JoinHandle,timeout 就不推进。
    /// **若此测正常 ~1s 触发** => 之前卡的 t04 主因是 IO-bound(`read_to_end` 子进程管道)特定叠加。
    #[test]
    fn block_on_current_timeout_spawns_only_sleep_task_fires() {
        use std::time::Duration;
        use tokio::time::{sleep, timeout};
        let r = block_on_current(async {
            let h = tokio::spawn(async move {
                sleep(Duration::from_secs(30)).await;
                42u8
            });
            timeout(Duration::from_secs(1), h).await
        });
        assert!(
            r.is_err(),
            "spawn+sleep(无 IO) 的 timeout 应 ~1s 触发 Err;实得 {:?}",
            r
        );
    }

    /// 析因第二刀:spawn 一个 IO-bound worker task,子进程**持续往 stdout 写**(Windows `ping -n 30`
    /// 每秒一行,30 秒后自然退 —— 真重现 t04,但有限不会把测卡死)。`timeout(2s, JoinHandle)` 应
    /// 在 ~2s 触发 kill_on_drop 杀子进程。**若此测也卡/超时未触发** => 锁定「IO-bound `read_to_end`
    /// 在 spawn worker 上 pending」与 c′ 桥的组合就是卡住条件 —— 与 ping -t 永续与否无关。
    #[cfg(target_os = "windows")]
    #[test]
    fn block_on_current_timeout_io_bound_continuous_stdout_fires() {
        use std::time::Duration;
        use tokio::io::AsyncReadExt;
        use tokio::time::timeout;
        let r = block_on_current(async {
            let mut child = tokio::process::Command::new("cmd")
                .arg("/C")
                // ping -n 30 = 持续往 stdout 写 ~30 秒(非永续,免把测卡死)
                .arg("ping")
                .arg("127.0.0.1")
                .arg("-n")
                .arg("30")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .expect("spawn ping");
            let mut stdout = child.stdout.take().unwrap();
            let h = tokio::spawn(async move {
                let mut buf = Vec::new();
                stdout.read_to_end(&mut buf).await.ok();
                let _ = child.wait().await;
                buf.len()
            });
            timeout(Duration::from_secs(2), h).await
        });
        // 60s test runner 兜底:若卡死判 FAILED(HANG) —— 这正是抓的目标现象。
        assert!(
            r.is_err(),
            "IO-bound 子进程管线持续往 stdout 写时 timeout 应 ~2s 触发;实得 {:?}",
            r
        );
    }

    /// 析因第四刀:**复刻 Bash::execute 现 inner 真形** —— 标准 drain stderr + 仅 await 单路
    /// `stdout.read_to_end` + `child.wait()` + `err_rx.await`,命令 = `cmd /C ping -n 60`(有界,持续
    /// 往 stdout 写 ~60s),outer `timeout(5s)`。**本刀实证腿的 wall-clock**:用起止 `Instant` 量
    /// timeout 究竟 5s 触发还是被压到 ~60s(等子进程自然退)。析因结论:若 ~60s => stderr 排水
    /// task 的存在(哪怕不 await)也会压住 outer timeout —— 即 (c′) 桥上 timer 对「有第 2 个 spawn
    /// task pending 在子进程管道 IO」不稳定,非 worker 数(worker_threads=4 实测仍 59.95s)。
    ///
    /// **`#[ignore]`**:本测人为重现「坏的」stall 形态(`tokio::time::timeout` 包含 stderr 排水 task 的
    /// inner),跑满 ~60s 才 resolve。它证伪「单路 read_to_end + 第 2 spawn task 仍 stall」的结论已焊进
    /// `block_on_current` 与 `Bash::execute` 注释(本会话实证数字 59.62s/59.95s);纳入 CI 会拖死每跑
    /// 60s,故标 ignore。按需 `cargo test -- --ignored` 单跑,读 wall-clock 印证 stall 仍在(防有人又把
    /// `tokio::time::timeout` 包回 Bash inner 而不自知)。Bash::execute 真修后的回归闸是 tools.rs
    /// `bash_hanging_command_actually_returns_within_timeout`(37s,非 ignore)。
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore]
    fn block_on_current_timeout_replica_of_bash_inner_with_stderr_drain() {
        use std::time::{Duration, Instant};
        use tokio::io::AsyncReadExt;
        use tokio::time::timeout;
        let start = Instant::now();
        let r = block_on_current(async {
            let mut child = tokio::process::Command::new("cmd")
                .arg("/C")
                .arg("ping")
                .arg("127.0.0.1")
                .arg("-n")
                .arg("60")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .expect("spawn ping");
            let mut stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let err_rx = {
                let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
                let mut stderr = stderr;
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let _ = stderr.read_to_end(&mut buf).await;
                    let _ = tx.send(buf);
                });
                rx
            };
            let inner = tokio::spawn(async move {
                let mut out_buf = Vec::new();
                let _ = stdout.read_to_end(&mut out_buf).await;
                let _ = child.wait().await;
                let _err_buf = err_rx.await.unwrap_or_default();
                out_buf.len()
            });
            timeout(Duration::from_secs(5), inner).await
        });
        let elapsed = start.elapsed();
        // 断 timeout 确实后的形态(Err(Elapsed) == 超时分支);并打 wall-clock 让人类读数:
        // · < 15s => ~5s 触发(timer 健康)
        // · 午 ~55s => 被 stderr drain task 压到等子进程自然退(timer stall)
        // (此测不计入正常门禁的快测 —— 因其本身就为抓「卡 60s」而跑 60s。纳入 #[ignore] 不自动跑。)
        let _ = elapsed;
        assert!(r.is_err(), "复刻 Bash inner 的 stderr-drain 形,outer 5s timeout 终态应 Err(Elapsed);实得 {:?},耗时 {:?}", r, elapsed);
    }

    /// 析因第五刀 + 验真势:**绕开 tokio timer,用 OS 线程壁钟杀子进程**(P9-4 拟真修)。
    /// 不 `tokio::time::timeout` 包 inner —— 根本不让桥上 timer 担责;改为 spawn 一个独立 OS 线程
    /// sleep(deadline) 后 `taskkill /T /F /PID` 树杀(连 `cmd /C` 的 grandchild `ping` 一起灭)。子进程被
    /// 杀 → stdout 管道 EOF → 单路 `read_to_end` 自然完成 → inner JoinHandle resolve。await 路径无
    /// timeout 包裹、不踩桥上 timer stall。预期 ~5s 返回 ok(out_len 有限),不是 ~60s。
    #[cfg(target_os = "windows")]
    #[test]
    fn block_on_current_bash_inner_killed_by_os_thread_deadline_fires() {
        use std::time::{Duration, Instant};
        use tokio::io::AsyncReadExt;
        let start = Instant::now();
        let r: Result<usize, anyhow::Error> = block_on_current(async {
            let mut child = tokio::process::Command::new("cmd")
                .arg("/C")
                .arg("ping")
                .arg("127.0.0.1")
                .arg("-n")
                .arg("60") // 有界 ~60s(不 -t,免真永续)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .expect("spawn ping");
            let pid = child.id().expect("child pid");
            let mut stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            // stderr 排水 task 仍 spawn(与 Bash::execute 真形一致)—— 验「OS 线程杀 + 单路 read_to_end」
            // 是否不受 stderr drain task 存在影响。
            let err_rx = {
                let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
                let mut stderr = stderr;
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let _ = stderr.read_to_end(&mut buf).await;
                    let _ = tx.send(buf);
                });
                rx
            };
            // OS 线程壁钟:deadline 到即树杀 /T /F(连 grandchild 一起灭)。不依赖桥上 tokio timer。
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(5));
                let _ = std::process::Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &pid.to_string()])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            });
            let inner = tokio::spawn(async move {
                let mut out_buf = Vec::new();
                let _ = stdout.read_to_end(&mut out_buf).await;
                let _ = child.wait().await;
                let _err_buf = err_rx.await.unwrap_or_default();
                Ok::<usize, anyhow::Error>(out_buf.len())
            });
            inner
                .await
                .map_err(|e| anyhow::anyhow!("inner join: {e}"))?
        });
        let elapsed = start.elapsed();
        let n = r.expect("read_to_end 应在子进程被杀后退 ok");
        // 壁钟断言:5s 杀应在 ~6s 内返回(杀后管道 EOF + join 收尾)。若 ~60s 才返回 = 失败。
        assert!(
            elapsed.as_secs() < 20,
            "OS 线程壁钟杀应 ~5s 触发返回;实得耗时 {:?}(out_len={})",
            elapsed,
            n
        );
    }

    // ── resolve_program:非 Windows 恒返 None(读 env / 真 PATH 扫描留本机实测回贴)。──

    /// 非 Windows:`resolve_program` 不论传什么都返 `None` —— Unix 的 `execvp` 自带 PATH
    /// 查找且无 PATHEXT 概念,本模块不插手。用 `cfg` 隔离,这条测仅在非 Windows 编译。
    /// (Windows 上等价的纯逻辑测见下面 `resolve_in_paths_*`。)
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn resolve_program_non_windows_always_none() {
        assert!(
            resolve_program("npx").is_none(),
            "非 Windows 不应替调用者解析命令"
        );
        assert!(resolve_program("node").is_none());
    }

    // ── resolve_in_paths:Windows 上的纯逻辑核心(P9-1,不碰 env 可单测)。──

    /// 命中:prog 在某候选目录下、按 PATHEXT 第一个匹配扩展名找到文件 → 返其全路径。
    /// 用 `tempfile` 风格手造候选目录(`std::env::temp_dir().join(唯一子目录)`)避免引 crate;
    /// 造一个假 `fakeprog.cmd`(对应真实 npx.cmd shim 场景)。
    #[cfg(target_os = "windows")]
    #[test]
    fn resolve_in_paths_finds_first_matching_extension() {
        let base =
            std::env::temp_dir().join(format!("codeagent-p9-pathtest-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("造临时 PATH 目录");
        // 假装 fakeprog.CMD 是 npx.cmd 那种 node shim。.COM/.EXE 无文件,.CMD 命中。
        let cmd = base.join("fakeprog.CMD");
        std::fs::write(&cmd, b"@echo off\r\n").expect("造假 shim 文件");
        let dirs = vec![base.clone()];
        let exts = vec![
            ".COM".to_string(),
            ".EXE".to_string(),
            ".BAT".to_string(),
            ".CMD".to_string(),
        ];
        let got = resolve_in_paths("fakeprog", dirs, &exts).expect(".CMD 应命中");
        assert_eq!(got, cmd, "应返 PATH×PATHEXT 命中的全路径");
        // 收拾:测试间不残留(单测默认并行,但每个用唯一子目录互不撞)。
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 顺序:多个目录时,按 PATH 目录序优先命中第一个含可执行文件的目录
    /// (模拟 `PATH` 里两目录都有同名 shim 时,前者胜出 —— 与 shell 一致)。
    #[cfg(target_os = "windows")]
    #[test]
    fn resolve_in_paths_prefers_first_dir() {
        let id = std::process::id();
        let a = std::env::temp_dir().join(format!("codeagent-p9-pathearly-{id}"));
        let b = std::env::temp_dir().join(format!("codeagent-p9-patlate-{id}"));
        std::fs::create_dir_all(&a).expect("造 a");
        std::fs::create_dir_all(&b).expect("造 b");
        let earlier = a.join("dup.EXE");
        let later = b.join("dup.CMD");
        std::fs::write(&earlier, b"X").expect("造 a/dup.EXE");
        std::fs::write(&later, b"Y").expect("造 b/dup.CMD");
        let dirs = vec![a.clone(), b.clone()];
        // PATHEXT 里 .EXE 在 .CMD 前,但更重要的是目录序:a 在前,a/dup.EXE 先命中。
        let exts = vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()];
        let got = resolve_in_paths("dup", dirs, &exts).expect("应命中");
        assert_eq!(
            got, earlier,
            "PATH 前目录优先,即便 .EXE 与 .CMD 顺序也保前目录胜出"
        );
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    /// 全没命中(候选目录里都无 prog+任一扩展名):返 `None` —— 让上层退化回 `Command::new(cfg.command)`
    /// 试一次,它的报错对用户更直给(而非本函数静默吞)。
    #[cfg(target_os = "windows")]
    #[test]
    fn resolve_in_paths_returns_none_when_nothing_matches() {
        let base =
            std::env::temp_dir().join(format!("codeagent-p9-patempty-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("造空候选目录");
        let dirs = vec![base.clone()];
        let exts = vec![".COM".to_string(), ".EXE".to_string()];
        assert!(
            resolve_in_paths("definitely-not-here-fake-prog", dirs, &exts).is_none(),
            "无任何命中应返 None,交上层 Command 再试"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // 注:`resolve_program` 自身(读真 PATH/PATHEXT + 裸名判定)涉及真环境变量,多线程测试下
    // set_var 有 race 风险,不作 auto 测 —— 留本机:真起 `npx.cmd` MCP server 实测回贴
    // (journey §13.6 已记 P8 实证撞坑 + workaround,P9-1 把 workaround 收成可单测的纯逻辑)。
}
