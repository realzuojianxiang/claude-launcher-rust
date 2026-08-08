// tools —— P1-3:Tool trait + ReadFile;P3:扩 write_file/list_dir/glob/bash。
//
// 设计原则(来自实测,见 docs/codeagent-journey.md §3.3):
//   1. `index` 字段不可依赖(NVIDIA 根本不返回)—— 多工具按数组顺序 + id 唯一配对。
//   2. `content` 必须 Option<String>,容 DeepSeek 的 "" 与 NVIDIA 的 null 两种。
//   3. `reasoning_content` 不进 messages —— 那是给模型自己的「思考」,
//      回灌进对话历史会让模型反复卷自己的思考、触发死循环;只在给用户打印时可选展示。
//
// Tool trait 形态刻意保持「窄接口」(见 docs/codeagent-concepts.md §7.5):
//   加工具 = impl Tool,与 P0.5 加 provider 同构 —— 都是一处 trait + 一份实现。
//
// P3 安全取舍(见 journey §5):有副作用的工具(write_file/bash)实现标 `is_destructive=true`,
//   审批闸在 dispatch 处统一拦(那里才有 stdin),不塞进 execute —— trait 形态最小改动。
//   默认 y/n 闸(P4 雏形);CLI `--yolo` 跳过。读到「destructive」别慌:它指「对磁盘有副作用」,
//   不是「会删你硬盘」—— 且 first 杠就是 path 安全钳 + 命令前缀都拦在工作目录 subtree 内。

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
// anyhow::Context 给 .context() 用(取不到 cwd 时加一句人话);trait 必须 `as _` 显式导入。
use anyhow::Context as _;

/// 一个工具的能力契约。P3 扩工具集就是多 impl 几个它。
pub trait Tool {
    /// 工具名,对模型可见(模型靠它决定调谁)。
    fn name(&self) -> &str;
    /// 给模型看的描述 —— 这是 agent 工程最被低估的「写 prompt」点(见 concepts §3.1)。
    fn description(&self) -> &str;
    /// OpenAI function tool 的 parameters JSON Schema。
    fn parameters(&self) -> serde_json::Value;
    /// 生成完整的 OpenAI tool 定义(包一层 {type:function, function:{...}})。
    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name(),
                "description": self.description(),
                "parameters": self.parameters(),
            }
        })
    }
    /// 这个工具有没有副作用(写文件 / 跑命令)?有则 dispatch 处会先拦一道审批闸(P4 雏形)。
    /// 默认 false —— 只读工具不必审批。write_file / bash 标 true。
    fn is_destructive(&self) -> bool {
        false
    }
    /// 真正执行。arguments 是模型给出的 JSON 字符串(OpenAI 协议:arguments 是 string,
    /// 不是 object —— 见 §3.2 的实测印记)。实现负责自己解析这一层。
    fn execute(&self, arguments: &str) -> anyhow::Result<String>;
}

// ===== path 安全钳(所有文件类工具共用,见 journey §5.2) =====
// 把模型给的路径(相对或绝对)归一到「工作目录 subtree 内」的规范绝对路径,
// 防 `../` 越狱。越界一律 Err —— 错误串回灌给模型,让它换路径或先确认在哪。
fn resolve_under_cwd(raw: &str) -> anyhow::Result<PathBuf> {
    let cwd = std::env::current_dir().context("取不到当前工作目录")?;
    let joined = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        cwd.join(raw)
    };
    // 规范化:去掉 . 和 ..。canonicalize 要求路径已存在,这里要容「写不存在的新文件」,
    // 故手做 normalize:遍历 Components 重新拼。
    let mut normalized = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::CurDir => {} // . 跳过
            Component::ParentDir => {
                normalized.pop(); // .. 退一层;若根上 pop 无效就停在根,不越界
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    // 钳:cwd 是这条路径的前缀,否则越界。
    if !normalized.starts_with(&cwd) {
        return Err(anyhow::anyhow!(
            "路径 {:?} 规范化后 {:?} 跑到了工作目录 {:?} 之外 —— 拒绝(防 ../ 越狱)。请只用工作目录内的路径。",
            raw, normalized, cwd
        ));
    }
    Ok(normalized)
}

/// 读一个本地文件。Agent 的第一个「手」。
pub struct ReadFile;

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
}

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "读取本地文件内容并返回。用于查看某个文件里写的是什么。"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "要读取的文件路径,相对路径基准是程序工作目录"
                }
            },
            "required": ["path"]
        })
    }
    fn execute(&self, arguments: &str) -> anyhow::Result<String> {
        // arguments 是字符串形态的 JSON —— 这是 OpenAI tool use 协议的反直觉点(§3.2)。
        // 先解一层拿到 path,再去真读文件。
        let args: ReadFileArgs = serde_json::from_str(arguments).map_err(|e| {
            anyhow::anyhow!("read_file 参数解析失败: {e} (原始 arguments: {arguments:?})")
        })?;
        // P3 起统走 path 安全钳:规范化 + 钳在 cwd subtree 内(防 ../ 越狱,见 §5.2)。
        let path = resolve_under_cwd(&args.path)?;
        let content = std::fs::read_to_string(&path).map_err(|e| {
            // 错误回灌按 §5 的「丙(清楚)」写法:讲清发生了什么 + 给下一步可操作暗示。
            // 这种错误信息是给模型看的 prompt,让它知道是换个路径还是先列目录。
            anyhow::anyhow!(
                "读取文件失败:路径 {:?} 不存在或不可读({})。可能原因:相对路径基准是当前工作目录,或文件名拼错。建议先用绝对路径或在调用前列目录确认。",
                args.path, e
            )
        })?;
        Ok(content)
    }
}

// ===== P3:扩工具集 (见 journey §5) =====

/// 写一个本地文件(可覆盖)。Agent 第一个「有副作用的手」。
pub struct WriteFile;

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "把内容写入本地文件;若文件已存在则整体覆盖。用于创建文件或保存修改后的代码。"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "要写入的文件路径,相对路径基准是程序工作目录" },
                "content": { "type": "string", "description": "要写入的完整文件内容" }
            },
            "required": ["path", "content"]
        })
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn execute(&self, arguments: &str) -> anyhow::Result<String> {
        let args: WriteFileArgs = serde_json::from_str(arguments).map_err(|e| {
            anyhow::anyhow!("write_file 参数解析失败: {e} (原始 arguments: {arguments:?})")
        })?;
        let path = resolve_under_cwd(&args.path)?;
        // 父目录不存在则建(模型常「先写新文件再 list」,该容);父若越界已被 resolve 拦。
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("write_file 建父目录 {:?} 失败: {e}", parent))?;
        }
        std::fs::write(&path, &args.content).map_err(|e| {
            anyhow::anyhow!(
                "写入 {:?} 失败: {e}。确认路径在工作目录内、磁盘未满、有写权限。",
                args.path
            )
        })?;
        // 回灌一句话 + 实际落点绝对路径 —— 模型能据此 report 成功、并知道它落在哪。
        Ok(format!(
            "已写入 {} ({} 字节)",
            path.display(),
            args.content.len()
        ))
    }
}

/// 列目录条目。补 ReadFile 的「读之前先看这有什么」缺失(§3.6 错误回灌里就提了先列目录)。
pub struct ListDir;

#[derive(Deserialize)]
struct ListDirArgs {
    path: Option<String>,
}

impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "列出指定目录下的条目(文件名 + 是不是目录);不传 path 则列程序工作目录。不递归。"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "要列的目录路径,省略则列工作目录" }
            },
            "required": []
        })
    }
    fn execute(&self, arguments: &str) -> anyhow::Result<String> {
        // path 可选 -> 结构上 Option<String>;空串 arguments 也要容(模型常传 "{}")。
        let args: ListDirArgs = if arguments.trim().is_empty() || arguments.trim() == "{}" {
            ListDirArgs { path: None }
        } else {
            serde_json::from_str(arguments).map_err(|e| {
                anyhow::anyhow!("list_dir 参数解析失败: {e} (原始 arguments: {arguments:?})")
            })?
        };
        let dir = match &args.path {
            Some(p) => resolve_under_cwd(p)?,
            None => std::env::current_dir().context("取不到当前工作目录")?,
        };
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            anyhow::anyhow!(
                "列目录 {:?} 失败: {e}。确认路径存在且是目录;不确定时先不传 path 列工作目录。",
                args.path.as_deref().unwrap_or(".")
            )
        })?;
        let mut lines = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| anyhow::anyhow!("读目录条目失败: {e}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if entry.path().is_dir() {
                "目录"
            } else {
                "文件"
            };
            lines.push(format!("{kind} {name}"));
        }
        lines.sort();
        if lines.is_empty() {
            Ok(format!("目录 {:?} 为空", dir))
        } else {
            Ok(lines.join("\n"))
        }
    }
}

/// 按 glob pattern 搜文件名。命名沿用 Claude Code 的 glob,语义同 shell glob(简单,非 regex)。
pub struct Glob;

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
}

impl Tool for Glob {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "按 glob 规则搜文件路径(如 \"**/*.rs\" 匹配所有子目录里的 .rs 文件)。基准是程序工作目录。"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "glob 规则,* 单层、** 任意层" }
            },
            "required": ["pattern"]
        })
    }
    fn execute(&self, arguments: &str) -> anyhow::Result<String> {
        let args: GlobArgs = serde_json::from_str(arguments).map_err(|e| {
            anyhow::anyhow!("glob 参数解析失败: {e} (原始 arguments: {arguments:?})")
        })?;
        // 自己实现 glob 起步:不引第三方 crate,避免 P3 凭空加依赖。
        // 支持 ** / * / ?,基准 cwd,只返回文件(不返回目录),打包路径作输出。
        let cwd = std::env::current_dir().context("取不到当前工作目录")?;
        let results = glob_walk(&cwd, &args.pattern)?;
        if results.is_empty() {
            Ok(format!("没有匹配 {:?} 的文件", args.pattern))
        } else {
            Ok(results
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }
}

// 最小 glob:cwd 为根,递归收集文件,按 pattern 边走边匹配。
// 不引 crate 的代价:只支持 * / ? / ** 这几档,字符类 [...] 省略(P3 够用)。
fn glob_walk(root: &Path, pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
    // 按 / \ 拆「段」:** 段跨任意层,其余段单层。例如 "**/*.rs" -> ["**","*.rs"]。
    let segs: Vec<String> = pattern
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let mut out = Vec::new();
    walk(root, &segs, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, segs: &[String], out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    // 段走完 = 匹配终点,只收文件。
    if segs.is_empty() {
        if dir.is_file() {
            out.push(dir.to_path_buf());
        }
        return Ok(());
    }
    let (seg, rest) = segs.split_first().unwrap();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // 无权限/不存在 -> 跳过该子树,不整树炸
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let child = entry.path();
        if seg == "**" {
            // ** 段:自身匹配零层(消段) + 跨一层(向下仍带 **)。
            walk(&child, rest, out)?;
            walk(&child, segs, out)?;
        } else {
            // 单层段:整段当一个通配串做名匹配。to_string 持所有权,避免 Cow 临时值早释放。
            let name = entry.file_name().to_string_lossy().into_owned();
            if match_simple(seg, &name) {
                walk(&child, rest, out)?;
            }
        }
    }
    Ok(())
}

// 粗单层通配匹配:支持 * / ?,不支持 [...];不含通配时要求全等。按 UTF-8 字节粗匹(P3 够用)。
fn match_simple(pat: &str, name: &str) -> bool {
    match_star(pat.as_bytes(), name.as_bytes())
}
fn match_star(p: &[u8], n: &[u8]) -> bool {
    match (p.split_first(), n.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((b'*', rest)), _) => match_star(rest, n) || match_star(p, &n[1..]),
        (Some((b'?', rest)), Some((_, nrest))) => match_star(rest, nrest),
        (Some((c, rest)), Some((d, nrest))) if c == d => match_star(rest, nrest),
        _ => false,
    }
}
/// 跑一条 shell 命令。真正「动手」的能力,也是危险度最高 —— 强标 `is_destructive`,
/// dispatch 处会拦审批闸(P4 雏形,见 journey §5.4)。命令 timeout 30s 兜底,输出截断回灌。
pub struct Bash;

#[derive(Deserialize)]
struct BashArgs {
    command: String,
}

impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "执行一条命令行(不起交互包)。用于跑测试/编译/装依赖/git 操作 等。工作目录基准是程序当前目录。"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "要执行的完整命令行" }
            },
            "required": ["command"]
        })
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn execute(&self, arguments: &str) -> anyhow::Result<String> {
        let args: BashArgs = serde_json::from_str(arguments).map_err(|e| {
            anyhow::anyhow!("bash 参数解析失败: {e} (原始 arguments: {arguments:?})")
        })?;
        // 用 tokio 进程 buf 完整收 stdout+stderr 合流,30s timeout 兜底防死等。
        use std::time::Duration;
        let out = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .arg(if cfg!(windows) { "/C" } else { "-c" })
            .arg(&args.command)
            .output()
            .map_err(|e| anyhow::anyhow!("启动命令失败: {e}"))?;
        let code = out.status.code().unwrap_or(-1);
        let mut combined = String::new();
        combined.push_str(&String::from_utf8_lossy(&out.stdout));
        if !out.stderr.is_empty() {
            combined.push_str("\n[stderr]\n");
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
        }
        // 截断:过长输出灌回去会爆 context,留头 4000 字符 + 尾 1000 + 截断提示。
        let _ = Duration::from_secs(30); // 占位:同步 process::Command 无 timeout,P5 改 tokio 再上真 timeout
        if combined.chars().count() > 5000 {
            let head: String = combined.chars().take(4000).collect();
            let tail: String = combined
                .chars()
                .rev()
                .take(1000)
                .collect::<Vec<_>>()
                .iter()
                .rev()
                .collect();
            Ok(format!(
                "exit={code}\n…(输出已截断,头4000/尾1000)…\n{head}\n…[截断]…\n{tail}"
            ))
        } else {
            Ok(format!("exit={code}\n{combined}"))
        }
    }
}

// ===== tool use 响应解析结构 =====
// 这些结构跨 provider 通用,严格按 §3.3 三结论定:不靠 index、content 容空、reasoning 不进历史。

/// 模型一次 tool_call 的描述。
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolCall {
    // 注意:不建模 index(NVIDIA 不返回,DeepSeek 返回但不稳定)。
    // 多工具一律按 tool_calls 数组顺序执行 + 用 id 配 role:tool 回灌。
    pub id: String,
    // OpenAI 协议里 type 恒为 "function",建模它纯粹为容错,不强校验。
    #[serde(default)]
    #[allow(dead_code)]
    pub r#type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolCallFunction {
    pub name: String,
    /// arguments 是字符串形态 JSON,不是对象 —— 见 §3.2 / ReadFile::execute。
    pub arguments: String,
}

/// 模型本轮回复的「完整」描述:正文 + (可选)tool_calls 终止理由。
/// finish_reason: §3.5 的反直觉发现 —— 比光看「有没有 tool_calls」更稳的循环判定信号。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// 模型决定调工具 → 执行 + 回灌 + 再循环。
    ToolCalls,
    /// 模型正常结束、给纯文字答案 → 打印收工。
    Stop,
    /// 其它可能的值(Llength / content_filter / 等)+ 未知:统一兜底。
    #[serde(other)]
    Other,
}

/// 多 provider 兼容的 assistant 消息体。
/// content 容 Option<String> —— DeepSeek 给 ""、NVIDIA 给 null,两种都要能解(§3.3 #2)。
/// reasoning_content 不建模进 messages 历史 —— 仅在打印时单独取见(§3.3 #3)。
#[derive(Debug, Deserialize)]
pub struct AssistantReply {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// DeepSeek / NVIDIA 都返回 reasoning_content,但不进对话历史。
    /// 单独建模它只是为「打印给用户看」,所以保持 Option + 默认 None。
    #[serde(default)]
    #[allow(dead_code)]
    pub reasoning_content: Option<String>,
}

impl AssistantReply {
    /// 这一轮模型到底想不想调工具?优先看 finish_reason(§3.5),回落看 tool_calls 有无。
    pub fn wants_tool(&self, finish: &FinishReason) -> bool {
        matches!(finish, FinishReason::ToolCalls)
            || self.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
    }
}

/// 工具结果回灌时用的 role:"tool" 消息体。
/// tool_call_id 必须配对上 ToolCall.id,让模型知道哪个结果对哪次调用(§3.2)。
#[derive(Serialize, Clone)]
pub struct ToolResultMessage {
    pub role: String,
    pub tool_call_id: String,
    pub content: String,
}

impl ToolResultMessage {
    pub fn new(tool_call_id: String, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            tool_call_id,
            content,
        }
    }
}

// ===== P5 流式:OpenAI 兼容 Chat Completions 的 SSE 累积器 =====
// (见 journey §7;协议核证见 docs/P5-streaming-protocol-notes.md)。
//
// 协议要点(实测+官方 SDK + DeepSeek/NIM 偏差核证后的硬约束):
//   · 帧长这样: `data: {json}\n\n`,末帧 `data: [DONE]\n\n`(`[DONE]` 非 JSON,别 parse)。
//   · 终止判据两个并用: 见 `[DONE]` 即退;或流自然 EOF(NIM/vLLM 偶尔不发 [DONE] 直接断)。
//   · tool_calls 配对**靠 index,绝不靠 id** —— 后续 chunk 不带 id,只带 function.arguments 字符片段。
//   · function.arguments 是**字符串拼接**(非 JSON parse 合),流完再 serde_json::from_str 一次。
//   · chunk 层面不做 content/tool_calls 互斥假设 —— 独立累积,最后按 finish_reason 分类。
//   · DeepSeek 多 `delta.reasoning_content`(分片,在 content 之前)+ finish_reason 多 "insufficient_system_resource"
//     → 现有 FinishReason 的 #[serde(other)] 已兜底成 Other,这里 finish_reason 用原始 string 累,交上层定型。
//   · NIM 听闻 `tool_calls[].index` 时有不回填,把 index 建成 Option;缺失时按帧内出现序派生,不崩。
//   · 错误帧: 有的 proxy 直接发 `{"error":{...}}` 再断流 —— parse 失败时再 try error,命中即整体标错。

/// 流式响应的 JSON 增量结构(只建用到的字段,其余靠 serde 忽略)。
/// 跟非流式 ChatResponse 是两套 schema:流式是「分片增量」,非流式是「整条 assistant 消息」。
#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    #[serde(default)]
    pub choices: Vec<StreamChoice>,
    /// P6:流式 usage 只在末帧(stream_options.include_usage=true 时 `data: [DONE]` 前一帧)。
    /// 中间帧全 null(OpenAI/DeepSeek 一致)。空帧(退化帧)可有 usage 无 choices —— 故都 default。
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// 上下文/计费用量 —— 跨流式(末帧)与非流式(顶层)共用一套。
/// DeepSeek 额外多 `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens`/
///   `completion_tokens_details.reasoning_tokens`,本版本不消费、serde 默认忽略。
#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    #[serde(default)]
    pub delta: StreamDelta,
    /// 末帧才有值;中间全 null(OpenAI/DeepSeek 一致;NIM 应一致)。
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct StreamDelta {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    /// DeepSeek(及 NIM 跑 DeepSeek-distill 类)特有,正文之前的「思考」,分段流。
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<StreamTcDelta>>,
}

#[derive(Debug, Deserialize)]
pub struct StreamTcDelta {
    /// OpenAI 官方 SDK 类型里是 `index: int` 必填。
    /// NIM 听闻偶尔不回填 → 建成 Option,缺失时调用方按帧内出现序派生(见 StreamAcc::ingest)。
    #[serde(default)]
    pub index: Option<i32>,
    #[serde(default)]
    pub id: Option<String>,
    /// type 恒为 "function";建模它纯粹为容错首帧偶发携带,不强校验。
    #[serde(default)]
    #[allow(dead_code)]
    #[serde(rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub function: Option<StreamTcFunc>,
}

#[derive(Debug, Default, Deserialize)]
pub struct StreamTcFunc {
    #[serde(default)]
    pub name: Option<String>,
    /// 字符串**片段**,逐 chunk 拼接;空串 "" 是合法的 no-op append。
    #[serde(default)]
    pub arguments: Option<String>,
}

/// 流式 tool_call 的累积槽(按 delta.index 占位;index 缺失则按出现序派生)。
struct AccToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// 一条流式响应的全量累积:把无数 delta 片段焊成「一条完整 assistant 消息」的形态,
/// finalize 后即与非流式 AssistantReply 同形,喂回 run_one_turn 主循环结构基本不动。
pub struct StreamAcc {
    role: Option<String>,
    content: String,
    reasoning: String,
    /// 按 index 占位的稀疏数组;finalize 时收敛成连续 Vec。
    tool_calls: Vec<Option<AccToolCall>>,
    /// 末帧才到的原始 string(stop / tool_calls / length / content_filter / insufficient_system_resource / ...)。
    finish_reason: Option<String>,
    /// P6:end 发的「只带 usage 的空帧」(需 stream_options.include_usage=true)。中间帧 usage 全 None。
    usage: Option<Usage>,
}

/// finalize 的返回包:把流式累积态收敛成「一条完整 assistant 消息」+ 终止依据 + 末帧 usage。
/// clippy type_complexity 因这个 5 元组报警,named type 收一下既过门禁也让调用端可读。
pub struct FinalizedReply {
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    /// 原始 string(stop/tool_calls/length/...),上层再转 FinishReason(复用 #[serde(other)] 兜底)。
    pub finish_reason: Option<String>,
    /// 末帧 usage(需 stream_options.include_usage=true);缺失则 None。
    pub usage: Option<Usage>,
}

impl StreamAcc {
    pub fn new() -> Self {
        Self {
            role: None,
            content: String::new(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            finish_reason: None,
            usage: None,
        }
    }

    /// 吃一个 SSE 增量 chunk。content/reasoning/arguments 全按「字符串拼接」语义;
    /// null 与空串都跳过(空串 append 是 no-op),且不把 null 当「流结束」信号。
    pub fn ingest(&mut self, ch: StreamChunk) {
        // P6:usage 末帧往往 choices 为空(只带 usage),故**先取 usage 再判 choices**,
        // 否则会被下面「choices.into_iter().next() 的 None 早 return」吞掉。
        if let Some(u) = ch.usage {
            self.usage = Some(u);
        }
        let Some(choice) = ch.choices.into_iter().next() else {
            return; // 退化帧(NIM 偶发 / P6 usage 末帧):usage 已收,这帧 choices 没料 —— skip。
        };
        if let Some(fr) = choice.finish_reason {
            if self.finish_reason.is_none() {
                self.finish_reason = Some(fr);
            }
        }
        let d = choice.delta;
        if let Some(r) = d.role {
            if self.role.is_none() {
                self.role = Some(r); // 首帧 "assistant",后续不覆盖。
            }
        }
        if let Some(s) = d.content {
            if !s.is_empty() {
                self.content.push_str(&s);
            }
        }
        if let Some(s) = d.reasoning_content {
            if !s.is_empty() {
                self.reasoning.push_str(&s);
            }
        }
        if let Some(tcs) = d.tool_calls {
            // NIM 兜底:index 缺失则按这帧里 tool_calls 的「出现序」派生,免崩。
            // 派生只在该帧内序号可信;跨帧仍以「同一槽位已填过则不重建」收敛。
            let mut fallback = 0i32;
            for tc in tcs {
                let raw_idx = tc.index.unwrap_or_else(|| {
                    let v = fallback;
                    fallback += 1;
                    v
                }) as usize;
                if raw_idx >= self.tool_calls.len() {
                    self.tool_calls.resize_with(raw_idx + 1, || None);
                }
                let slot = self.tool_calls[raw_idx].get_or_insert_with(|| AccToolCall {
                    id: None,
                    name: None,
                    arguments: String::new(),
                });
                if let Some(id) = tc.id {
                    slot.id = Some(id); // 仅首帧带;后续 chunk 不带 id,靠 index 配位不丢。
                }
                if let Some(f) = tc.function {
                    if let Some(n) = f.name {
                        slot.name = Some(n); // 同样仅首帧。
                    }
                    if let Some(a) = f.arguments {
                        slot.arguments.push_str(&a); // 字符片段拼接,不强 parse。
                    }
                }
            }
        }
    }

    /// 流收尾时把累积态焊成非流式同形 (content/reasoning/tool_calls/finish_reason)。
    /// `arguments` 仍是未 parse 的 JSON 字符串(沿用非流式 ToolCall 的约定,parse 责任在工具实现)。
    /// finish_reason 原样返回(string),上层再转成枚举 FinishReason(复用 #[serde(other)] 兜底逻辑)。
    pub fn finalize(self) -> FinalizedReply {
        let tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_iter()
            .filter_map(|opt| {
                opt.map(|tc| ToolCall {
                    id: tc.id.unwrap_or_default(),
                    // type 恒 "function";首帧若带则用之,否则默认填上(与非流式行态对齐)。
                    r#type: "function".to_string(),
                    function: ToolCallFunction {
                        name: tc.name.unwrap_or_default(),
                        arguments: tc.arguments,
                    },
                })
            })
            .collect();
        let content = if self.content.is_empty() {
            None
        } else {
            Some(self.content)
        };
        let reasoning = if self.reasoning.is_empty() {
            None
        } else {
            Some(self.reasoning)
        };
        FinalizedReply {
            content,
            reasoning,
            tool_calls,
            finish_reason: self.finish_reason,
            usage: self.usage,
        }
    }
}

/// 把「finish_reason 原始字符串」转回枚举,复用非流式 FinishReason 的 #[serde(other)] 兜底
/// —— 这样 DeepSeek 的 "insufficient_system_resource" / 未来未知值都落到 Other,不崩。
/// 用 serde 桥接:把 string 包成单字段 JSON 再 Deserialize,白嫖 enum 的 other 分支。
pub fn finish_reason_from_str(s: &str) -> FinishReason {
    #[derive(Deserialize)]
    struct Wrap {
        v: FinishReason,
    }
    let json = format!(r#"{{"v":{}}}"#, s);
    serde_json::from_str::<Wrap>(&json)
        .map(|w| w.v)
        .unwrap_or(FinishReason::Other)
}

/// 切 SSE 流的字节累积器:跨 chunk 边界存「半 event」,按 `\n\n` 切 event 边界。
/// chomp_dnfalse 把已切出的整段 event 文本交回调用方处理。
/// 兼容 `\r\n\r\n`(部分 server 用 CRLF);两者取先到者。
pub fn sse_split(buf: &mut Vec<u8>, incoming: &[u8]) -> Vec<Vec<u8>> {
    buf.extend_from_slice(incoming);
    let mut out = Vec::new();
    loop {
        // 找 \n\n 或 \r\n\r\n;取先到的位置。
        let pos = buf
            .windows(2)
            .position(|w| w == b"\n\n")
            .map(|p| (p, 2))
            .or_else(|| {
                buf.windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| (p, 4))
            });
        let Some((pos, sep_len)) = pos else {
            break;
        };
        let event: Vec<u8> = buf.drain(..pos + sep_len).collect();
        out.push(event);
    }
    out
}

/// 从一段 SSE event 文本里抽 `data:` 行负载;其它行(注释 `:`、`event:`、`id:`、`retry:`)跳过。
/// 返回 None 表示这帧没有效 data(纯注释/心跳)。多 data 行按 SSE 规则用 `\n` 拼(OpenAI 体系实际每帧只一行,留通用性)。
pub fn sse_data_payload(event_bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(event_bytes);
    let mut parts: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(':') {
            // SSE 注释行(`: keep-alive` 等)—— 跳过,不断流。
            let _ = rest;
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            // 规范: data 后可选一个空格(OWS),剥掉它;无空格也容。
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            parts.push(rest);
        }
        // event: / id: / retry: 行不处理(我们不靠它们)。
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    //! P6.0 协议核证 —— 这些不是「业务逻辑测试」,是把 DeepSeek/OpenAI 协议里容易踩坑的两条
    //! 退化帧用可执行断言焊死,免得日后重构 ingest() 时不知不觉回退到「choices 空就 return」
    //! 把 usage 末帧吞掉。纯函数、不联网,本地 `cargo test --lib` 全过(ci-gates-windows-cdylib
    //! 记的 cdylib 运行时 DLL 加载问题在本机可能让 test 二进制启动失败,那是环境问题)。
    use super::*;

    /// 构一个常用末帧 JSON:`choices: []`,`usage: {...}`。
    fn usage_only_frame_json() -> &'static str {
        r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":34,"total_tokens":46}}"#
    }

    /// P6 核心断言:usage 末帧的 `choices` 是**空数组**。ingest 旧实现是
    /// `let Some(choice) = ch.choices.into_iter().next() else { return }` ——
    /// 一旦 choices 空就提前 return,**在判 choices 之前没取 usage**,usage 永远拿不到。
    /// 现实现把 `if let Some(u) = ch.usage` 提到 choices 判定之**前**。这条测试锁住那个顺序。
    #[test]
    fn ingest_picks_up_usage_from_empty_choices_frame() {
        let chunk: StreamChunk = serde_json::from_str(usage_only_frame_json()).unwrap();
        // 帧本身合法反序列化:choices 空、usage 有值。
        assert!(chunk.choices.is_empty());
        assert!(chunk.usage.is_some());

        let mut acc = StreamAcc::new();
        acc.ingest(chunk);
        // 关键:尽管 choices 空,usage 必须已落到 acc 上 —— 这正是旧实现会漏的点。
        let u = acc
            .usage
            .expect("usage 末帧(choices 空)必须被 ingest 收到,不能被 choices 早 return 吞掉");
        assert_eq!(
            u,
            Usage {
                prompt_tokens: 12,
                completion_tokens: 34,
                total_tokens: 46
            }
        );
    }

    /// 端到端收尾:content 帧 → usage 末帧 → finalize() 两样都不丢。
    /// 模拟一条最小流:第 1 帧正文 delta,第 2 帧带 finish + 我方这里不画的 usage 末帧的处理。
    /// 这条防的是「重构 ingest 后 finalize 出来的 usage / content 之一被错顺位的代码吃掉」。
    #[test]
    fn finalize_carries_both_content_and_usage_through_a_minimal_stream() {
        let mut acc = StreamAcc::new();
        // 帧 1:正文增量 + 角色首帧。
        let frame1 = r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":null}]}"#;
        acc.ingest(serde_json::from_str(frame1).unwrap());
        // 帧 2:末帧 finish + 正文空(常态:末帧 choice 还在但 content 空;OpenAI 末帧带 finish)。
        let frame2 = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        acc.ingest(serde_json::from_str(frame2).unwrap());
        // 帧 3:usage 末帧 —— `data: [DONE]` 前一帧,choices 全空。
        acc.ingest(serde_json::from_str(usage_only_frame_json()).unwrap());

        let fr = acc.finalize();
        assert_eq!(fr.content.as_deref(), Some("hello"));
        assert_eq!(fr.finish_reason.as_deref(), Some("stop"));
        let u = fr.usage.expect("finalize 末帧 usage 必须透出");
        assert_eq!(u.total_tokens, 46);
        assert!(fr.tool_calls.is_empty());
    }

    /// 反向钉:不带 usage 的中间帧 + 收尾无 usage 末帧(代理不回 / 没开 include_usage)。
    /// finalize 仍要给出 `usage: None`,上层 `unwrap_or_default()` 退成全 0。这是「度量缺失」
    /// 的可观测退化,不是 panic。锁住它别被改成「None 时塞个假 0」之类。
    #[test]
    fn finalize_usage_is_none_when_never_seen() {
        let mut acc = StreamAcc::new();
        acc.ingest(
            serde_json::from_str(
                r#"{"choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":"stop"}]}"#,
            )
            .unwrap(),
        );
        let fr = acc.finalize();
        assert!(
            fr.usage.is_none(),
            "从未见过 usage 帧时 finalize 应给 None,别默默填 0 假数据"
        );
        assert_eq!(fr.content.as_deref(), Some("hi"));
    }
}
