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
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build 内嵌 runtime 失败");
            rt.block_on(f)
        });
        h.join().expect("bridge 线程 panic")
    });
    result
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
}

impl McpClient {
    /// 起子进程 + 后台读 task,返 `Arc<Mutex<Self>>`(共享给多个 McpTool)。
    /// 失败返 Err,上层(run)跳过该 server 不致命。
    pub async fn spawn(cfg: &McpServerConfig) -> anyhow::Result<Arc<Mutex<Self>>> {
        let mut cmd = tokio::process::Command::new(&cfg.command);
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
            anyhow::anyhow!(
                "MCP server `{}` 启动失败 ({:?} {:?}): {e}",
                cfg.command,
                cfg.command,
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
        })))
    }

    /// server 友好名(给日志/排错用)。P8 接入面用 `cfg.command` 记日志,这方法留给 P9
    /// 统一接入日志层用,暂标 dead。
    #[allow(dead_code)]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// 发一请求(method+params)、等回响应的 `result`(出错包成 anyhow)。每帧一行 JSON + `\n` + flush。
    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
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
        let resp = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP `{}` 请求 `{}` 30s 未回应(超时)",
                    self.server_name,
                    method
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
        let _result = self.request("initialize", init_params).await?;
        // 协议要求 initialize 响应后再发 initialized 通知(单边,无回)。
        self.notify("notifications/initialized", serde_json::json!({}))
            .await?;
        Ok(())
    }

    /// `tools/list` → 拆出工具数组。每个工具取 name/description/inputSchema。
    pub async fn list_tools(&mut self) -> anyhow::Result<Vec<McpToolDesc>> {
        let result = self.request("tools/list", serde_json::json!({})).await?;
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
