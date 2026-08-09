// codeagent — 造自己的 code agent。
// P0:单轮问答骨架 → P0.5:多 provider 配置(TOML) → P1:tool use 跑通一轮。
//
// 这一版做的事:
//   1. 从 codeagent.toml 读配置,按 default 选 provider(base_url/model 从配置来)
//   2. 从该 provider.api_key_env 指明的环境变量读真 key
//   3. 从 stdin 读一行用户输入
//   4. 带 read_file 工具跑 agent loop:模型决定调工具 → 执行 → role:tool 回灌 → 再调,
//      直到模型不再用工具给纯文字答案 → 打印收工(单圈,P2 再做多轮连续)
//
// 刻意为之的设计(见 codeagent-rs/docs/codeagent-journey.md 与 codeagent-concepts.md):
//   · Tool trait 窄接口 + ReadFile impl —— 加工具 = impl Tool,与加 provider 同构。
//   · 解析结构按实测三结论(§3.3):不靠 index、content 容空、reasoning_content 不进历史。
//   · 循环判定优先看 finish_reason(§3.5 反直觉发现),比光看「有没有 tool_calls」稳。
//   · 工具错误按 §5「丙(清楚)」回灌 —— 是给模型看的 prompt,不是给人看的栈。

mod compactor;
mod config;
mod mcp;
mod session;
mod subagent;
mod tools;

use std::io::{self, Write};

use serde::{Deserialize, Serialize};

use crate::compactor::{Compactor, CompactorReport, Summarizer};
use crate::config::Provider;
use crate::tools::{
    finish_reason_from_str, sse_data_payload, sse_split, AssistantReply, Bash, FinishReason, Glob,
    ListDir, ReadFile, StreamAcc, StreamChunk, Tool, ToolResultMessage, Usage, WriteFile,
};

// P5.5 REPL 行编辑:rustyline(↑↓ 历史、光标行内移动、Ctrl-C 取消当行、EOF 退 REPL)。
// 历史文件落 exe 同级(简化:CWD 相对 .codeagent_history,后续可接 Config::config_dir 等价定位)。
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

/// 对话历史中的一条消息。
/// 一个 Message 承载三种角色(system/user/assistant/tool),靠 role 字段区分;
/// tool_calls(assistant 用)与 tool_call_id(tool 用)都设可选 + skip,
/// 无关角色不序列化这些字段 —— 请求体保持每种角色只发该发的字段。
/// P7:字段对子模块 session(pub(crate))可见 —— save/load 接 `&[Message]` 跨模块要它的类型;
///   单测往返比较需逐字段读,故字段也 pub(crate)(crate 内传阅对象,不开 crate 外可见)。
/// derive Debug:测试里 unwrap_err()(Ok 变体要 Debug)与失败断言打印需它。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Message {
    pub(crate) role: String,
    pub(crate) content: String,
    /// assistant 回复含的工具调用 —— 回灌进历史时模型要能看见「我刚才调过」(concepts §4 要点 1)。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub(crate) tool_calls: Option<Vec<crate::tools::ToolCall>>,
    /// role:tool 时配对的 tool_call_id(concepts §3.2 配对要求)。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub(crate) tool_call_id: Option<String>,
}

impl Message {
    pub(crate) fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
    pub(crate) fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

/// OpenAI 兼容的 Chat Completions 请求体。
#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    /// P6:流式下要末帧回 usage(DeepSeek/OpenAI 协议:`stream_options.include_usage=true`
    /// 才会在 `data: [DONE]` 前补一帧只带 usage 的 chunk)—— 否则拿不到 prompt_tokens,
    /// 上下文监控/压缩的「度量」根本无处取。非流式响应顶层 usage 自始回填,无需此开关。
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

/// `stream_options.include_usage` —— 唯一用到的一个布尔。
/// 单独建结构体是因为协议要求 `{"include_usage": true}` 这个嵌套对象形态,不是平铺 bool。
#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// Chat Completions 响应体(只建模用得到的字段,其余靠 serde 忽略)。
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    /// P6:非流式顶层 usage 自始回填;可选(防御性:某些代理可能不回)。
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    message: crate::tools::AssistantReply,
    finish_reason: FinishReason,
}

/// 调一次模型(非流式,`--no-stream` 回退口)。base_url/model 从 provider 来。
/// 返回 (本轮 assistant 回复, 终止理由) —— agent loop 据理由分支(§3.5)。
async fn chat_completion(
    client: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
) -> anyhow::Result<(crate::tools::AssistantReply, FinishReason, Usage)> {
    let req = ChatRequest {
        model: provider.model.clone(),
        messages: messages.to_vec(),
        stream: false,
        tools: tools.map(|t| t.to_vec()),
        // 非流式:usage 顶层自始回填,无需 stream_options(它只在 stream:true 时有意义)。
        stream_options: None,
    };

    let resp = client
        .post(provider.chat_url())
        .bearer_auth(api_key)
        .json(&req)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("请求发送失败(reqwest error): {e:#}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("HTTP 状态非 2xx: {e:#}"))?;

    let body: ChatResponse = resp.json().await?;
    let choice = body
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("响应 choices 为空"))?;
    Ok((choice.message, choice.finish_reason, body.usage))
}

/// P6.1 默认 `Summarizer` 实现:**模型二次调用生成摘要**(见 compactor.rs / journey §12)。
///
/// 把要压的中段(`to_compress`)整段当一次普通对话历史,加一句 system 指令「用一段话总结
/// 上面这段对话的事实」,非流式再调一次同一 provider;拿回的 assistant 终答(content)就是摘要,
/// 包成一条 assistant 消息顶回历史。
///
/// 不带 tools(摘要不需要调工具)、不流式(不需要逐 token 打给人看)—— 与 P6 §10.3 非流式
/// 曲线路径同源。失败传播给上层(maybe_compact 返 Err → REPL 打一笔后继续,不致命于会话)。
struct ModelSummarizer<'a> {
    client: &'a reqwest::Client,
    provider: &'a Provider,
    api_key: &'a str,
}

/// 给摘要调用的 system 指令。措辞刻意强调「事实 + 具体细节」,降低压缩后召回漂移
/// (journey §12.6 占位 —— 实测召回靠人眼,本措辞是先验上较稳的写法)。
const SUMMARY_INSTRUCTION: &str = "\
你是对话压缩器。下面是用户和一位 code agent 之间较早的一段对话历史(含工具调用与返回)。\
请用一段话总结这段对话里出现的关键事实:用户问过什么、agent 用工具读了/写了什么、得出什么结论、\
达成什么约定。**保留所有具体细节**(文件名、函数名、数值、人名等),不要泛泛而谈;\
直接输出总结内容,不要寒暄、不要分项、不要复述本指令。";

#[async_trait::async_trait]
impl Summarizer for ModelSummarizer<'_> {
    async fn summarize(&self, to_compress: &[Message]) -> anyhow::Result<Message> {
        // 摘要请求 = 一条 instruction system + 整段中段历史。不带 tools(stream/非流都不需要)。
        let mut req_msgs = Vec::with_capacity(1 + to_compress.len());
        req_msgs.push(Message::system(SUMMARY_INSTRUCTION));
        req_msgs.extend(to_compress.iter().cloned());

        // 非流式调用 —— 同 chat_completion 路径,但不传 tools(摘要不调工具)。
        // 这里手动建请求(reuse chat_completion 会塞 tools_slice,故不复用而照同一格式发一次)。
        let req = ChatRequest {
            model: self.provider.model.clone(),
            messages: req_msgs,
            stream: false,
            tools: None,
            stream_options: None,
        };
        let resp = self
            .client
            .post(self.provider.chat_url())
            .bearer_auth(self.api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("摘要调用发送失败: {e:#}"))?
            .error_for_status()
            .map_err(|e| anyhow::anyhow!("摘要调用 HTTP 非 2xx: {e:#}"))?;
        let body: ChatResponse = resp.json().await?;
        let choice = body
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("摘要响应 choices 为空"))?;
        // 摘要回的是纯文字终答(content);tool_calls 理论上不该有(没给 tools),保险压平为空填回。
        let content = choice
            .message
            .content
            .unwrap_or_else(|| "[摘要为空]".to_string());
        Ok(Message {
            role: "assistant".into(),
            content,
            tool_calls: None,
            tool_call_id: None,
        })
    }
}

/// P5 流式调用的结局:正常收完 vs 被用户 Ctrl-C 中断。
/// 中断时**不**把半截 assistant 消息压回历史 —— 半截 tool_calls.arguments 可能是
/// 残缺 JSON,回灌会让模型糊涂;本轮作废、回 REPL 顶等下一句,最干净。
enum StreamOutcome {
    Completed(crate::tools::AssistantReply, FinishReason, Usage),
    Interrupted,
}

/// P5 流式调用:发 `stream:true` 请求,逐 SSE chunk 收 → 边打印 token 边累积 → 收完焊成 AssistantReply。
/// 用 `tokio::select!` 让 Ctrl-C 能在流的中途一拍中止:``interrupt` future 就绪即取消流、返回 Interrupted。
/// `interrupt` 是个返回 `()` 的 future(oneshot/Recv 都兼容),由调用方每轮新建、单轮消费一次。
/// 流式协议的硬约束见 tools.rs `StreamAcc` 的注释和 docs(P5-streaming-protocol-notes)。
///
/// 打印策略:content 增量即时 print(flush 每片,真流式体验);reasoning_content(DeepSeek 思考)
///   先单独攒,收尾时按 P3 §3.3 的「打印给人看但不进历史」语义提示一句 —— 流式时也只在收尾展示,
///   避免思考 token 一边来一边刷屏扰人。tool_calls 不边打边打(它在收尾后由 dispatch 触发审批闸问人)。
async fn chat_completion_stream<F>(
    client: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
    mut interrupt: F,
) -> anyhow::Result<StreamOutcome>
where
    F: std::future::Future<Output = Option<()>> + Unpin + Send,
{
    use futures_util::StreamExt;

    let req = ChatRequest {
        model: provider.model.clone(),
        messages: messages.to_vec(),
        stream: true,
        tools: tools.map(|t| t.to_vec()),
        // P6:流式不开 include_usage → 末帧不会补 usage 帧 → 拿不到 prompt_tokens。
        // DeepSeek/OpenAI 协议核证见 P5-streaming-protocol-notes.md「usage 末帧」段。
        stream_options: Some(StreamOptions {
            include_usage: true,
        }),
    };

    let resp = client
        .post(provider.chat_url())
        .bearer_auth(api_key)
        .json(&req)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("流式请求发送失败(reqwest error): {e:#}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("流式 HTTP 状态非 2xx: {e:#}"))?;

    // text/event-stream 增量字节流。
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new(); // 跨 chunk 的半 event 累积器(SSE event 边界跨 TCP chunk 是常态)。
    let mut acc = StreamAcc::new();
    // 正文增量即时 print 出去(真流式体验:每片 token 一来就 flush,不等收完)。
    // 思考 reasoning_content 也边来边打(DeepSeek「思考」,协议约定先于正文流),给它单独行视觉,
    //   收尾不再二次提示 —— 它本就「给人看、不进历史」(§3.3 #3),边来边打正好。
    // tool_calls 增量不边打 —— 收尾由 dispatch 触发审批闸问人才是它的亮相时机。
    let mut out = std::io::stdout();
    let mut content_started = false;
    // 思考段单独追踪:正文之前(DS 默认)先冒「(思考: …」增量,流完封半个括号回。若 content
    // 先于 reasoning(某些模型同帧乱序),已打正文不动;reasoning 起时补换行隔离,两段不混并。
    let mut reasoning_opened = false;

    // 中断 select:每次拉下一个 chunk 前 tokio::select! 让出给 interrupt future 一拍。
    // 收到信号 → 取消流(省得后台继续拉到 EOF),返回 Interrupted。
    // interrupt 是个返回 () 的未来,select 消费它一次。
    let mut interrupted: bool = false;
    let mut done: bool = false; // 显式 [DONE] 标志(与自然 EOF 区分,仅用于注释义;两条收尾路径合流统一 finalize)。

    loop {
        tokio::select! {
            biased; // interrupt 优先 —— 用户按 Ctrl-C 时哪怕 chunk 正在来也要尽快响应。
            _ = &mut interrupt => {
                interrupted = true;
                break;
            }
            chunk_res = stream.next() => {
                let Some(chunk_res) = chunk_res else {
                    break; // 流自然 EOF(NIM/vLLM 偶尔不发 [DONE],EOF 即终态)。
                };
                let chunk_bytes = chunk_res.map_err(|e| {
                    anyhow::anyhow!("流式读字节失败(reqwest stream error): {e:#}")
                })?;
                for event_bytes in sse_split(&mut buf, &chunk_bytes) {
                    let Some(payload) = sse_data_payload(&event_bytes) else {
                        continue; // 注释 / 心跳 / 无 data 行 —— 跳过,不断流。
                    };
                    if payload == "[DONE]" {
                        done = true;
                        break; // 显式终止:与自然 EOF 合流到循环外的统一 finalize。
                    }
                    match serde_json::from_str::<StreamChunk>(&payload) {
                        Ok(c) => {
                            // 即时打印 + 累积:先看这帧的 content / reasoning 增量(边打边),
                            // 再 ingest。两段用 content_started / reasoning_opened 错开视觉:
                            //   · reasoning 起时若正文还没冒 → 新起一行 `(思考: ` 开头;
                            //   · 正文起时若 reasoning 没收完(同帧既有又有)也别纠结,各自走各自,
                            //     DeepSeek 默认 reasoning 先于 content 流,两段基本不会撞。
                            if let Some(ch0) = c.choices.first() {
                                if let Some(rc) = ch0.delta.reasoning_content.as_deref() {
                                    if !rc.is_empty() {
                                        if !reasoning_opened {
                                            println!();
                                            print!("(思考: ");
                                            let _ = out.flush();
                                            reasoning_opened = true;
                                        }
                                        print!("{}", rc);
                                        let _ = out.flush();
                                    }
                                }
                                if let Some(delta) = ch0.delta.content.as_deref() {
                                    if !delta.is_empty() {
                                        // 正文起前若思考段没封口 → 先封掉 `)` 再起新行给正文(协议 DS 是先思考后正文,
                                        // 此处兜的是「两段同大地交错」的反常情形,常态不会进这条)。
                                        if reasoning_opened && !content_started {
                                            println!(")");
                                            let _ = out.flush();
                                            reasoning_opened = false; // 封口后归位,避免正文完再封一次
                                        }
                                        if !content_started {
                                            // 首片正文前补个换行,把它和「> 提示符」那行分开。
                                            println!();
                                            content_started = true;
                                        }
                                        print!("{}", delta);
                                        let _ = out.flush();
                                    }
                                }
                            }
                            acc.ingest(c);
                        }
                        Err(_) => {
                            // 可能是错误帧(部分 proxy 直接发 {"error":{...}} 再断)。
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) {
                                if v.get("error").is_some() {
                                    return Err(anyhow::anyhow!(
                                        "上游流中返回错误帧: {}",
                                        payload
                                    ));
                                }
                            }
                            // 既非合法 chunk 也非 error 帧 —— 记到 stderr 不致命,流继续。
                            eprintln!("[warn] 不可解析的 SSE chunk 已跳过: {}", payload);
                        }
                    }
                }
                if done {
                    break;
                }
            }
        }
    }

    if interrupted {
        println!("\n[已中断 —— 本轮作废,对话历史不保留半截。]");
        let _ = out.flush();
        return Ok(StreamOutcome::Interrupted);
    }

    let _ = done; // [DONE] 与自然 EOF 两路在此合流:统一从 acc 取最终态。

    // 收尾:把累积态焊成非流式同形 AssistantReply + 枚举 finish_reason。
    // finish_reason 末帧没带(某些 proxy)时,回退看 tool_calls 有无(§3.5 反直觉发现的反面回退)。
    let fr = acc.finalize();
    let usage = fr.usage.unwrap_or_default();
    let finish = match fr.finish_reason.as_deref() {
        Some(s) => finish_reason_from_str(s),
        None => {
            if !fr.tool_calls.is_empty() {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
    };
    // 正文收尾补换行(若流式已即时打过,这里给个干净的行尾;纯 tool_calls 轮 content 为空则不补)。
    if content_started {
        println!();
    }
    // 思考段若还开着口(个别情况下正文从未接上、reasoning 段没收尾封 `)`)→ 补封。
    // reasoning 已边来边打(见上面循环),这里不再二次整段提示 —— 那是 P5 初版的的位置 bug
    //   (收尾才打会让它落在正文之后,而协议约定它先于正文流)。
    if reasoning_opened {
        println!(")");
    }
    let reply = AssistantReply {
        content: fr.content,
        tool_calls: if fr.tool_calls.is_empty() {
            None
        } else {
            Some(fr.tool_calls)
        },
        reasoning_content: fr.reasoning,
    };
    Ok(StreamOutcome::Completed(reply, finish, usage))
}

/// 把一个 AssistantReply 压进历史(含它本轮的 tool_calls)。
/// 关键:这条 assistant 消息必须进历史,否则模型看不到「我刚才调过啥」会原地打转。
fn assistant_message_from_reply(reply: &crate::tools::AssistantReply) -> Message {
    Message {
        role: "assistant".to_string(),
        content: reply.content.clone().unwrap_or_default(),
        tool_calls: reply.tool_calls.clone(),
        tool_call_id: None,
    }
}

/// 审批闸(P4:可配置白名单,见 journey §6;P3 时它还是「每次都问」硬闸)。
/// destructive 工具(write_file/bash)执行前的判定:
///   1. 非 destructive 工具 → 不过闸,直放行(读类工具本如此)。
///   2. `yolo=true` → 全放行(深度逃逸,致敬 launcher 的 yolo 概念)。
///   3. destructive 但命中配置白名单(bash 命令以白名单前缀开头)→ 自动放行,免 y/n。
///   4. 命不中 → 才走 y/n 问人;非 y 一律拒绝(默认安全)。
///
/// 拒绝都回灌给模型「换条路」,见 dispatch_tool。
struct ApprovalGate {
    yolo: bool,
    allow: crate::config::ApprovalConfig,
}

/// 审批闸的二态裁决。dispatch_tool 据 Allow→执行、Deny→回灌拒绝理由给模型换条路。
/// 比 `bool` 字面清晰:过闸分支写成 `match verdict { Allow => .., Deny => .. }`,不再 `if !check(..)`
/// 留「bool 取反」的阅读负担。引入它是 P8 为了加 write_file 的 diff 分支后让放行/拒绝两条路显式。
#[derive(Debug)]
enum GateVerdict {
    Allow,
    Deny,
}

/// 按行 LCS(经典 DP)生成最简 unified diff 串。不引 crate —— codeagent 写的多是源码级
/// (几百~几千行),O(n·m) 完全够;Myers 复杂得多收益不值。简化版:每簇变更只带 `+`/`-` 变化行,
/// 不扩 +/- 3 context 行(足够审「这次写了啥」)。
///
/// 三边角(都做了显式处理,不靠「极致通用算法自动表现」):
///   · 全新文件(old 为空):不分 hunk,把 new 每行加 `+` 整段打,头标 `--- /dev/null`。
///   · 文件不变(old==new):回一行 `(内容与现有文件完全相同,无变化)` —— 仍让闸问 y/N
///     (防模型把 unchanged 重写一遍空转)。
///   · 大幅重写(LCS 长 < 0.3×最大行数):警告 + 新旧行数对比 + 仅显示头 ~40 行(防几千行刷屏)。
fn unified_diff(old: &str, new: &str, path: &str) -> String {
    // 文件不变:显式 no-op 标记。
    if old == new {
        return "(内容与现有文件完全相同,无变化)".to_string();
    }
    // 按行切(保留行尾判定:split 末尾空串决定最后有无换行;这里统一用 lines(),丢尾换行不影响视觉)。
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // 全新文件:直接每行 `+`。
    if old.is_empty() {
        let mut s = String::new();
        s.push_str(&format!("--- /dev/null\n+++ {path}\n"));
        for l in &new_lines {
            s.push_str(&format!("+{l}\n"));
        }
        return s;
    }

    // LCS 长度 DP 表:dp[i][j] = old_lines[i..] 与 new_lines[j..] 的最长公共子序列长。
    let n = old_lines.len();
    let m = new_lines.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_lines[i] == new_lines[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let lcs_len = dp[0][0];
    let max_len = n.max(m);

    // 大幅重写(LCS 太短):警告 + 截断,不全打(防刷屏)。阈值 30% 是经验值,不是精确度量。
    if max_len > 0 && lcs_len < (max_len as f64 * 0.3) as usize {
        return format!("(大幅重写:旧 {n} 行 → 新 {m} 行;公共行太少,不展开全量 diff 仅示警告)");
    }

    // 回溯 LCS,在公共行之间夹 `-`/`+` 块。按 unified 风格:每个公共行打断的变更簇各成一段。
    let mut s = String::new();
    s.push_str(&format!("--- {path}\n+++ {path}\n"));
    let (mut i, mut j) = (0usize, 0usize);
    let mut hunk = String::new();
    let mut hunk_nonempty = false;
    while i < n || j < m {
        if i < n && j < m && old_lines[i] == new_lines[j] {
            // 公共行:把累积的 hunk flush 出去(若有),再打这一行作 context(以 ` ` 前缀)。
            if hunk_nonempty {
                s.push_str(&std::mem::take(&mut hunk));
                hunk_nonempty = false;
            }
            s.push_str(&format!(" {}\n", old_lines[i]));
            i += 1;
            j += 1;
        } else if j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j]) {
            // 新增行(取右边更长 LCS 路径):`+`。
            hunk.push_str(&format!("+{}\n", new_lines[j]));
            hunk_nonempty = true;
            j += 1;
        } else {
            // 删除行:`-`。
            hunk.push_str(&format!("-{}\n", old_lines[i]));
            hunk_nonempty = true;
            i += 1;
        }
    }
    if hunk_nonempty {
        s.push_str(&hunk);
    }
    s
}

impl ApprovalGate {
    /// 拦一道:Allow 放行、Deny 拒绝(P8 起 write_file 走 diff 分支,见 show_diff_then_prompt)。
    fn check(&mut self, tool: &dyn Tool, args: &str) -> GateVerdict {
        // --yolo:跳一切闸(含 write_file 的 diff)。脚本无人值守场景专用,预期。
        if !tool.is_destructive() || self.yolo {
            return GateVerdict::Allow;
        }
        // 仅 bash 走前缀白名单(write_file 暂无路径白名单 —— 保守口径:所有写一律审)。
        // 拿不到 command(解析失败)就退回 y/n —— 默认安全,不因配置解析炸而误放行。
        if tool.name() == "bash" {
            if let Some(cmd) = extract_bash_command(args) {
                if self
                    .allow
                    .bash_allow_prefix
                    .iter()
                    .any(|p| cmd.starts_with(p.as_str()))
                {
                    return GateVerdict::Allow; // 白名单命中:免审直放行。
                }
            }
        }
        // P8:write_file 专设分支 —— 解出 path+content、读旧文件、生成 unified diff 打印、再 y/N。
        // 这把「所有写一律问」升级成「先看要写啥再决定」,与 git 审 commit 同阅读习惯。
        if tool.name() == "write_file" {
            return if self.show_diff_then_prompt(args) {
                GateVerdict::Allow
            } else {
                GateVerdict::Deny
            };
        }
        // 其余 destructive(预留:未来新工具)走老 y/N。
        let brief: String = args.chars().take(200).collect();
        if self.prompt_yes_no(&format!("即将执行 {}({}) —— 放行?", tool.name(), brief)) {
            GateVerdict::Allow
        } else {
            GateVerdict::Deny
        }
    }

    /// 打 prompt 问 y/N,默认安全(读失败或非 y/yes → false)。抽出来给 bash 老分支与 diff 分支共用。
    fn prompt_yes_no(&mut self, prompt: &str) -> bool {
        print!("\n[审批] {prompt}? [y/N]\n> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        let line = line.trim().to_lowercase();
        line == "y" || line == "yes"
    }

    /// write_file 专享:解 {path,content}、读旧文件(不存在当空)、生成 unified diff 打印、再 y/N。
    /// 参数解析失败退回老 y/N(默认安全,不因 JSON 炸而误放行)。
    fn show_diff_then_prompt(&mut self, args: &str) -> bool {
        #[derive(serde::Deserialize)]
        struct WriteArgs {
            path: String,
            content: String,
        }
        let Ok(p) = serde_json::from_str::<WriteArgs>(args) else {
            return self.prompt_yes_no("write_file 参数解析失败,仍要写入");
        };
        let old = std::fs::read_to_string(&p.path).unwrap_or_default(); // 不存在 = 空(全新文件)
        let diff = unified_diff(&old, &p.content, &p.path);
        println!(
            "\n[审批] write_file {} —— 拟写入 {} 字节(旧 {} 字节):\n{}",
            p.path,
            p.content.len(),
            old.len(),
            diff
        );
        self.prompt_yes_no(&format!("放行写 {}", p.path))
    }
}

/// 从 bash 工具的 arguments JSON 里解出 command 字段返回。
/// 失败返回 None(调用方退回 y/n 闸,默认安全)。
fn extract_bash_command(args: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct BashCommand {
        command: String,
    }
    serde_json::from_str::<BashCommand>(args)
        .ok()
        .map(|c| c.command)
}

/// 调度一个 tool_call 到已注册的工具表。P3 起按 name 分发(替固定路由) + destructive 走审批闸。
fn dispatch_tool(
    call: &crate::tools::ToolCall,
    tools: &[Box<dyn Tool>],
    gate: &mut ApprovalGate,
) -> anyhow::Result<ToolResultMessage> {
    let Some(tool) = tools.iter().find(|t| t.name() == call.function.name) else {
        return Ok(ToolResultMessage::new(
            call.id.clone(),
            format!(
                "[未知工具] 没有名为 {} 的工具,可用工具: {}。请改用可用工具。",
                call.function.name,
                tools
                    .iter()
                    .map(|t| t.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    };
    // destructive 工具先过闸。拒绝就回灌拒绝理由,模型换条路;放行才执行。
    match gate.check(tool.as_ref(), &call.function.arguments) {
        GateVerdict::Deny => {
            return Ok(ToolResultMessage::new(
                call.id.clone(),
                format!(
                    "[用户拒绝执行] 工具 {} 的这次调用被用户否决,未执行。请换一种不修改磁盘的方式继续,或先与用户确认。",
                    tool.name()
                ),
            ));
        }
        GateVerdict::Allow => {}
    }
    let content = match tool.execute(&call.function.arguments) {
        Ok(out) => out,
        // 工具失败也回灌 —— 「丙(清楚)」写法的错误串给模型看,让它自纠正(§5)。
        Err(e) => format!("[工具执行失败] {}", e),
    };
    Ok(ToolResultMessage::new(call.id.clone(), content))
}

/// P6:把本轮用法打 stderr(不污染 stdout 对话流)。全 0 = 这轮没回 usage(末帧缺、或代理不回)
/// —— 也照打,因为「这轮拿不到度量」本身就是观测信号,能让人看出 include_usage 没生效 / 上游不回。
/// 写 stderr 而非 stdout:stdout 是给模型生成内容与 REPL 提示符的主对话流,usage 是开发观测面。
fn report_usage(kind: &str, round: usize, u: &Usage) {
    eprintln!(
        "[ctx:{kind}:{round}] prompt={} completion={} total={}",
        u.prompt_tokens, u.completion_tokens, u.total_tokens,
    );
}

/// P1 单圈内层的 agent loop —— 拆出来给 P2 外层 REPL 复用。
/// 约定:进入时 messages 已经包含了本轮 user 输入(以及之前全部历史),
/// 跑完到模型给纯文字答案为止,把这条 assistant 终答压回 messages 后返回。
/// 不在这里读输入也不在这里 print 提示符 —— 它只管「把一个问题问到答完」。
///
/// P5:`stream=true` 走流式 `chat_completion_stream`(逐 token 打印 + 中断),
///   被中断时本轮作废(不压半截历史)、回 REPL 等下一句。
///   `stream=false`(`--no-stream`)走老非流式 `chat_completion`(debug 回退口)。
///   `interrupt_rx` 是共享的 Ctrl-C 信号通道;每个工具循环轮从它 recv 出一个 future,
///   挂进流式 select。多轮工具调用因此每轮都新挂信号 —— 全程可被中断,不只在第一轮。
#[allow(clippy::too_many_arguments)] // 参数各有来路,捏 LoopCtx struct 反而要解构重新借,绕一圈;故 allow。
async fn run_one_turn(
    client: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    messages: &mut Vec<Message>,
    tools_slice: &[serde_json::Value],
    tools: &[Box<dyn Tool>],
    gate: &mut ApprovalGate,
    interrupt_rx: &mut tokio::sync::mpsc::Receiver<()>,
    stream: bool,
) -> anyhow::Result<(bool, u64)> {
    // 返回 (true, total)  = 本回合正常收工,history 已压回;total = 收工那轮模型回的 total_tokens。
    //                       total=0 表示这轮没拿到 usage(末帧缺/代理不回),上层 compactor 见 0 不触发。
    // 返回 (false, _)     = 被打断/异常收尾(用户中断作废 / 触 MAX_TOOL_ROUNDS 硬上限),本回合**不应落盘**
    //                       且调用方要把当下悬空 user(无 assistant 跟答)从 messages pop 掉,
    //                       免得留一条孤问句污染历史。total 这一路不消费(上层只在 finished=true 时压一次)。
    const MAX_TOOL_ROUNDS: usize = 8;
    let mut last_total: u64 = 0; // P6.1:每轮报到 total_tokens 取最大(通常最后一轮最大)喂给 compactor。
    for round in 1..=MAX_TOOL_ROUNDS {
        // 流式分支:每轮从共享 interrupt_rx 现拿一个「中断 future」挂进 select。
        // 非流式分支不读 interrupt(老路径不接中断;留它纯调用最简)。
        let (reply, finish) = if stream {
            let interrupt_fut = interrupt_rx.recv();
            tokio::pin!(interrupt_fut);
            // recv 出 None(发送端全关即 ctrl_c task 结束)也容:future 立即返回 (),
            // 会当下你没有按 Ctrl-C 也被当成中断 —— 此采取:发送端常驻(REPL 全程),
            // 不到进程结束不会关。但万一关了,本轮作废、继续 REPL 也是安全退化,可接受。
            match chat_completion_stream(
                client,
                provider,
                api_key,
                messages,
                Some(tools_slice),
                interrupt_fut,
            )
            .await
            {
                Ok(StreamOutcome::Completed(r, f, u)) => {
                    report_usage("stream", round, &u);
                    last_total = u.total_tokens.max(last_total);
                    (r, f)
                }
                Ok(StreamOutcome::Interrupted) => {
                    // 用户 Ctrl-C 中断 —— 本回合作废,RePL 继续等下一句。
                    return Ok((false, 0));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "调用 {} 失败(流式,第 {round} 轮): {e}",
                        provider.base_url
                    ));
                }
            }
        } else {
            chat_completion(client, provider, api_key, messages, Some(tools_slice))
                .await
                .map(|(r, f, u)| {
                    report_usage("non-stream", round, &u);
                    last_total = u.total_tokens.max(last_total);
                    (r, f)
                })
                .map_err(|e| {
                    anyhow::anyhow!("调用 {} 失败(非流式,第 {round} 轮): {e}", provider.base_url)
                })?
        };

        if !reply.wants_tool(&finish) {
            // 收工 —— 模型给纯文字答案。把终答压回历史,下一轮 user 追问才有上下文。
            // 流式分支已自行边打边 print + 收尾换行 + 思考提示;非流式分支走 print_reply 一次打完。
            if !stream {
                print_reply(&reply);
            }
            messages.push(assistant_message_from_reply(&reply));
            return Ok((true, last_total));
        }

        // 模型要调工具。先把这条含 tool_calls 的 assistant 消息压进历史。
        let calls = reply.tool_calls.clone().unwrap_or_default();
        messages.push(assistant_message_from_reply(&reply));

        // 逐个执行 + 逐个回灌 role:tool(带 tool_call_id 配对)。
        for call in &calls {
            let result = dispatch_tool(call, tools, gate)?;
            messages.push(Message {
                role: "tool".to_string(),
                content: result.content,
                tool_calls: None,
                tool_call_id: Some(result.tool_call_id),
            });
        }
        // 回到 loop 顶 —— 模型会看到这些工具结果,继续(可能再调,可能收工)。
        // 下一轮的 interrupt 通道在 loop 顶现取,所以多轮工具调用每轮都能被 Ctrl-C 中断。
    }

    // 触发硬上限:多轮仍不收工,给个明确告知而非静默退出。本回合作废但 REPL 继续。
    println!("[agent 达到工具调用轮数上限 {MAX_TOOL_ROUNDS},本回合主动停止。可在配置调高上限。]");
    Ok((false, 0))
}

// ─── REPL 读入策略 + 退出统一(#22 --script headless 模式)──────────────────────────────
// 为什么单独抽:run 的 REPL 循环体(slash 命令判定 + run_one_turn + finished 落盘/pop 悬空 user)
// 是 agent 的心脏,绝不能因「要不要 rustyline」fork 成两份——双份必漂移。故把「按行读」抽成一枚
// 三态枚举,两种读入方式各产此枚举;循环体据枚举分派、不感知读入方式。退出同理:把 /quit、exit、
// Eof(rustyline EOF 与 --script EOF 共两个源)汇成一条 exit_repl,顺手修掉旧「Ctrl-D 不存会话」bug。
//
// 不变量:run_one_turn / report_usage(写 stderr)/ interrupt mpsc / session::save 全逐字节不变——
//   --script 只换「读入」这一头,其余照旧。report_usage 落 stderr 是 P6.1 曲线靠 `2>usage.log` 抓的前置。

/// REPL 一步读入结果。两种模式共用此枚举,循环体据此分派、不 fork。
enum InputLine {
    /// 一行真实文本。尾随换行(若存在)由循环体 trim(与旧 rustyline 路径一致)。
    Line(String),
    /// 仅 rustyline 模式可达:raw mode 把 Ctrl-C 转成 Interrupted —— 取消当行,continue。
    /// --script 模式不产此态(无 raw mode 吞当行 Ctrl-C);生成中的 Ctrl-C 仍走 mpsc 中断路径。
    Interrupted,
    /// EOF / 脚本读完。视为请求干净退出(会存 session,见 exit_repl)。
    Eof,
}

/// rustyline 读路径(TTY 模式)。把几种 `ReadlineError` 映射成 `InputLine`。
/// 非 Interrupted/Eof 的真 IO 错误仍 `Err` 传播(不静默吞成 EOF——那会掩盖问题),与旧实现一致。
fn read_tty(rl: &mut DefaultEditor) -> anyhow::Result<InputLine> {
    match rl.readline("> ") {
        Ok(line) => Ok(InputLine::Line(line)),
        Err(ReadlineError::Interrupted) => Ok(InputLine::Interrupted),
        Err(ReadlineError::Eof) => Ok(InputLine::Eof),
        Err(e) => Err(anyhow::anyhow!("REPL 读取失败: {e:#}")),
    }
}

/// `--script` 读路径:裸 stdin,无 rustyline。刻意跳过行编辑/历史 —— 那都要真 TTY,
/// 管道喂入会让 rustyline `readline` 报 `os error 1`(函数不正确)。EOF = 脚本读完 = 干净退出。
/// 不产 `Interrupted`:脚本模式无当行 Ctrl-C 取消的概念(与 TTY 模式职责区分,见 run 注释)。
fn read_script() -> InputLine {
    use std::io::BufRead;
    let mut buf = String::new();
    match io::stdin().lock().read_line(&mut buf) {
        Ok(0) => InputLine::Eof,
        Ok(_) => InputLine::Line(buf),
        Err(e) => {
            eprintln!("[note] 脚本读入失败,按 EOF 退出: {e:#}");
            InputLine::Eof
        }
    }
}

/// 统一 REPL 退出:存历史(仅 rustyline 模式)+ 存会话(两模式都存)+ return。
/// 汇三处退出:`/quit`、`exit`、`Eof`(分别来自 read_tty 与 read_script 的 EOF)。
/// 顺手修旧 bug:旧的 `ReadlineError::Eof` 分支只 `println!(); return`,**不存会话**——
///   手滑 Ctrl-D 会丢整段对话、`--resume` 接不上。现在两模式任何退出都过这条,都存会话。
/// `rl: Option<&mut DefaultEditor>` —— --script 模式无 editor,传 None 跳过存历史(避开 clippy needless_option)。
fn exit_repl(
    rl: Option<&mut DefaultEditor>,
    messages: &[Message],
    history_file: &str,
    session_file: &str,
) -> anyhow::Result<()> {
    if let Some(rl) = rl {
        // 历史非关键,失败静默(与旧 /quit 分支的 `let _ =` 一致,不增 eprintln 噪声)。
        let _ = rl.save_history(history_file);
    }
    if let Err(e) = session::save(std::path::Path::new(session_file), messages) {
        eprintln!("[note] 会话未保存: {e:#}");
    }
    println!();
    Ok(())
}

/// agent 主入口(P2:连续多轮 REPL;P3:扩工具集 + 审批闸;P5:流式 + Ctrl-C 中断)。
/// 外层 REPL:读一行 user 输入 → 跑内层 agent loop(可能多轮工具)→ 答完 → 再读下一行。
/// messages 跨轮**保留**(共享对话历史),所以你能追问「刚才那个文件第一行写了啥」之类
/// 依赖上文的问题 —— 这是 P2 把「跑一轮」升级为「真 agentic loop」的关键。
/// 退出:空行直接跳过(不浪费一次调用)、`/quit` 或 `exit` 退出、EOF(Ctrl-Z/Ctrl-D)退出。
/// `yolo=true`:destructive 工具(write_file/bash)不拦审批闸,顺跑(实测不卡时开)。
/// `no_stream=true`(CLI `--no-stream`):走老非流式调用(P5 流式 debug 回退口,可对照排查)。
/// `script=true`(CLI `--script`):headless 模式 —— REPL 读入不走 rustyline(它要真 TTY,
///   管道喂入会 `os error 1`),改用裸 `io::stdin().read_line`,故可被管道驱动:
///   `Get-Content turns.txt | codeagent --script --yolo 2> usage.log`。
///   解锁 P6.1 长会话曲线采集(把 `[ctx:stream:N]` 从 stderr 一条命令采全,免手敲 20 轮手抄)
///   与 P7 自动 resume 接力(`--script --resume < turns2.txt`)。**不替人判模型答得好不好**,
///   只自动「喂输入 + 采数字」这层苦活。详见 journey §11。
/// --
/// 注意:`--script` 不带 `--yolo` 时,destructive 工具(write_file/bash)的 y/N 审批闸会卡在
///   管道 EOF(裸 stdin 读 y/N 返拒) → 模型可能改投它路、甚至循环。故脚本驱动一般配 `--yolo`。
///
/// P5 中断机制:启一个后台 task 装一个 `tokio::signal::ctrl_c()` 监听器,经 mpsc 通道把
///   「该中断当前生成」的信号发给主循环;每轮 agent 生成前从该通道取一条挂在 select 里。
///   用 mpsc(非 oneshot)是因为 single run_one_turn 内部 for 循环可能多轮工具调用,
///   每轮都要新挂一个信号接收器 —— oneshot 一次性,多轮就废了;mpsc 可多次取发。
async fn run(
    yolo: bool,
    no_stream: bool,
    resume: bool,
    script: bool,
    session_file: Option<String>,
) -> anyhow::Result<()> {
    let cfg = config::Config::load(std::path::Path::new("codeagent.toml"))?;
    let provider = cfg.default_provider()?.clone();
    let api_key = provider.api_key()?;
    let client = reqwest::Client::new();

    // Ctrl-C 监听后台 task:每收到一次 Ctrl-C,往 interrupt_tx 推一个 () 。
    // interrupt_rx 留在主循环(run_one_turn 每轮取一条挂进 select)。
    let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::channel::<()>(8);
    tokio::spawn(async move {
        use tokio::signal;
        loop {
            if signal::ctrl_c().await.is_err() {
                // 某些平台初次注册会报「未安装」,直接退出监听即可(REPL 仍能跑,只是无中断)。
                return;
            }
            // 通道满了(用户连按 Ctrl-C 比消费还快)也无所谓 —— 那几条会丢,但反应不更慢。
            let _ = interrupt_tx.send(()).await;
        }
    });

    // 跨整轮对话共享的历史:system 在最前,user/assistant/tool 顺序追加。
    // P3:system 提示词点明全部可用工具 + 「写/执行会先问人」,让模型知道节奏。
    // P7:`--resume` 启动时上层传 resume=true → 从 session_path 载入(含当时的 system);
    //   载入失败/损坏:不静默吞(session::load 已把坏文件改名留证),退回全新 system 重起 ——
    //   但 eprintln 一行让人知晓,免得以为「resume 成功了其实是新建」。
    // P8 subagent 隔离:`--session-file <path>` 显式覆盖(子进程用它,绝不撞父 .codeagent_session.json);
    // 普通用户不带这 flag 走默认名 —— 行为与 P7 完全一致(向后兼容)。
    let session_path: String = session_file
        .as_deref()
        .unwrap_or(".codeagent_session.json")
        .to_string();
    let default_messages = vec![Message::system(
        "你是一个简洁的 code agent。可用工具:read_file(读文件)、list_dir(列目录)、glob(按规则搜文件名)、write_file(写文件,会问人)、bash(跑命令,会问人)。需要时调相应工具,拿到结果后用中文直接回答用户问题。",
    )];
    let mut messages = if resume {
        match session::load(std::path::Path::new(&session_path)) {
            Ok(Some(msgs)) => {
                let n = msgs.len();
                eprintln!("[resume] 已载入 {session_path}({n} 条历史,含首条 system)。");
                msgs
            }
            Ok(None) => {
                eprintln!("[resume] 没找到 {session_path},从空会话重起。");
                default_messages.clone()
            }
            Err(e) => {
                eprintln!("[resume] 载入失败,从空会话重起: {e:#}");
                default_messages.clone()
            }
        }
    } else {
        default_messages.clone()
    };

    // 工具表:加工具 = 在这里 Box::new 一个 impl Tool,与 P1 的「固定路由」彻底解耦。
    // schema 直接从这表派生,避免「schema 列表」和「工具列表」两处各自维护、容易对不上。
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadFile),
        Box::new(ListDir),
        Box::new(Glob),
        Box::new(WriteFile),
        Box::new(Bash),
    ];

    // P8-3 subagent 工具:子进程式委派。失败不致命(降级为无 subagent 工具)。
    match subagent::SubagentTool::new() {
        Ok(t) => tools.push(Box::new(t)),
        Err(e) => eprintln!("[note] subagent 工具未启用(自举/临时目录失败): {e:#}"),
    }

    // P8-4 MCP stdio 客户端:遍历 [mcp.server.*],spawn 子进程 + 握手 + list_tools,
    // 每个远端工具包成 McpTool push 进表。任一 server 失败 eprintln 跳过(不致命于会话)。
    // mcp_clients 持连接保活 —— 与 tools 同寿(run scope 内);Drop 时 kill_on_drop 兜底杀子进程。
    let mut mcp_clients: Vec<std::sync::Arc<tokio::sync::Mutex<mcp::McpClient>>> = Vec::new();
    for (name, server_cfg) in &cfg.mcp.server {
        match mcp::McpClient::spawn(server_cfg).await {
            Ok(client) => {
                let mut guard = client.lock().await;
                match guard.handshake().await {
                    Ok(()) => match guard.list_tools().await {
                        Ok(descs) => {
                            eprintln!(
                                "[mcp] server `{name}` 起好,握手通过,接 {} 个工具: {:?}",
                                descs.len(),
                                descs.iter().map(|d| &d.name).collect::<Vec<_>>()
                            );
                            for d in &descs {
                                tools.push(Box::new(mcp::McpTool::new(
                                    std::sync::Arc::clone(&client),
                                    server_cfg.prefix.as_deref(),
                                    d,
                                )));
                            }
                            drop(guard);
                            mcp_clients.push(client);
                        }
                        Err(e) => eprintln!(
                            "[note] MCP server `{name}` list_tools 失败,跳过其工具: {e:#}"
                        ),
                    },
                    Err(e) => eprintln!("[note] MCP server `{name}` 握手失败,跳过: {e:#}"),
                }
            }
            Err(e) => eprintln!("[note] MCP server `{name}` 启动失败,跳过: {e:#}"),
        }
    }

    let tools_schemas: Vec<serde_json::Value> = tools.iter().map(|t| t.schema()).collect();
    let tools_slice: &[serde_json::Value] = &tools_schemas;
    // mcp_clients 在此 scope 持有,防子进程连接被 Drop —— 故这里显式标注「用到」它下文不再动。
    // (若编译器仍报 unused,改成只留 drop;当前通过运行期 mut 引用使编译器认得。)
    let _mcp_clients_ref = &mcp_clients;
    let mut gate = ApprovalGate {
        yolo,
        allow: cfg.approval.clone(),
    };

    // P6.1 压缩器:阈值按当前 provider 模型上限来(provider.max_context 或兜底)+ [compaction] 段参数。
    // 一次性建好复用 —— 每回合收工后用最后一次报回来的 total_tokens 调 should_compact / maybe_compact。
    // max_context 取 provider 段填的;缺省退 Compaction::DEFAULT_MAX_CONTEXT(保守小窗口模型假设,
    //   用大窗口模型一定在配置里显式填 max_context,否则压缩会过早触发,见 compactor.rs 注释)。
    let max_context = provider
        .max_context
        .unwrap_or(crate::config::Compaction::DEFAULT_MAX_CONTEXT);
    let compactor = Compactor::new(max_context, cfg.compaction.clone());
    eprintln!(
        "[compactor:init] max_context={max_context} compact_at_ratio={} compact_to_ratio={} keep_recent_turns={}",
        compactor.params.compact_at_ratio,
        compactor.params.compact_to_ratio,
        compactor.params.keep_recent_turns,
    );

    // P5.5 REPL 读入:TTY 模式用 rustyline(行编辑/↑↓ 历史/Ctrl-C 取消当行/EOF 退);--script 模式跳过。
    // 关键:rustyline readline 期间按 Ctrl-C → ReadlineError::Interrupted(它吞了 raw mode 下的 Ctrl-C,
    // 不再到达 P5 的 tokio::signal::ctrl_c 那个监听 task) —— 这正好对:readline 时没在生成,生成时没在
    // readline,两路 Ctrl-C 职责不撞:readline 里的 Ctrl-C = 取消这行重来(continue),不动 mpsc 中断流。
    // --script 模式:rustyline 要真 TTY,管道喂入会 `os error 1`,故不开 editor;读入走 read_script 的裸 stdin。
    //   生成中的 Ctrl-C 仍由上面的 ctrl_c 监听 task 接管(mpsc 中断流,与 TTY 模式共用),正确。
    const HISTORY_FILE: &str = ".codeagent_history";
    let mut rl_opt = if script {
        None
    } else {
        let mut rl =
            DefaultEditor::new().map_err(|e| anyhow::anyhow!("rustyline 初始化失败: {e:#}"))?;
        if let Err(e) = rl.load_history(HISTORY_FILE) {
            // 首次跑没文件属正常,仅其它错误记一笔(stderr,不扰 REPL)。
            if !matches!(e, ReadlineError::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::NotFound)
            {
                eprintln!("[note] 历史文件读取跳过: {e}");
            }
        }
        Some(rl)
    };
    loop {
        // 读入策略分派(#22):--script 走裸 stdin(read_script),否则走 rustyline(read_tty)。
        // 两路都产 InputLine,循环体据此分派、不感知读入方式 —— agent 心脏单源不 fork。
        let input = if script {
            read_script()
        } else {
            read_tty(rl_opt.as_mut().expect("非 --script 模式必有 editor"))?
        };
        let input = match input {
            // Ctrl-C:仅 rustyline 模式可达(raw mode 把它转成 Interrupted)—— 取消当行、继续 REPL,
            // 不退出、也不喂给 interrupt_rx(那时不在生成,根本没轮到中断流式)。--script 模式不产此态。
            InputLine::Interrupted => continue,
            // EOF(Ctrl-Z/Ctrl-D,或 --script 脚本读完):统一走 exit_repl —— 存历史(仅 TTY)+ 存会话两模式都存。
            // 旧实现这条分支只 println!+return **不存会话**(手滑 Ctrl-D 丢整段对话),现并入统一退出顺带修掉。
            InputLine::Eof => {
                return exit_repl(rl_opt.as_mut(), &messages, HISTORY_FILE, &session_path);
            }
            InputLine::Line(line) => line,
        };
        let input_trimmed = input.trim();
        if input_trimmed.is_empty() {
            continue; // 空行跳过 —— 不浪费一次模型调用(P1 那版的「空就退出」在 REPL 语义下不对了)。
        }
        if input_trimmed == "/quit" || input_trimmed == "exit" {
            // 统一退出:与上方 Eof 同一助手术语,都过「存历史(仅 TTY)+ 存会话(两模式)+ return」。
            return exit_repl(rl_opt.as_mut(), &messages, HISTORY_FILE, &session_path);
        }
        // P7:`/resume` 运行中载入(覆盖当前会话);`/clear` 清空回全新 system。
        if input_trimmed == "/resume" {
            match session::load(std::path::Path::new(&session_path)) {
                Ok(Some(msgs)) => {
                    let n = msgs.len();
                    messages = msgs;
                    println!("[已载入 {n} 条历史(含 system)。]");
                }
                Ok(None) => println!("[没找到 {session_path} —— 无可载入。]"),
                Err(e) => println!("[载入失败:{e:#}]"),
            }
            continue;
        }
        if input_trimmed == "/clear" {
            messages = default_messages.clone();
            println!("[已清空,回到全新 system。]");
            // 清空后顺手删旧会话文件,免得下次 --resume 又把刚清掉的载回来。
            let _ = std::fs::remove_file(&session_path);
            continue;
        }
        // 非空非退出 → 进历史(↑↓ 可重拾;rustyline 自去重最大长度,默认行为够用)。
        // --script 模式无 editor,跳过(脚本的「历史」就是 turns 文件本身,无须行编辑历史)。
        if let Some(ref mut rl) = rl_opt {
            let _ = rl.add_history_entry(&input);
        }

        messages.push(Message::user(input_trimmed));
        // 清掉这轮间隔里早到的 Ctrl-C 信号(用户在 REPL 等待期连按了),免得本轮一进 agent loop
        // 就被秒中断 —— 只让「本轮生成期间」按下的 Ctrl-C 生效。
        while interrupt_rx.try_recv().is_ok() {}
        let (finished, last_total) = run_one_turn(
            &client,
            &provider,
            &api_key,
            &mut messages,
            tools_slice,
            &tools,
            &mut gate,
            &mut interrupt_rx,
            !no_stream,
        )
        .await?;
        // P7 落盘语义:正常收工(finished=true)→ 落盘,下次 --resume 接得上;
        //   被打断/触上限(finished=false)→ **不落盘**,且把刚 push 的悬空 user pop 掉,
        //   免得留一条「问了但没答」的孤问句在下次 resume 时让模型看见会糊涂。
        //   半截 assistant(被中断时可能已 push 进 agent loop 的若干 tool 轮)同理会被丢 ——
        //   因为我们因 finished=false 整体不落盘,存的还是上一回合收工时的干净态。
        if finished {
            // P6.1 上下文压缩:收工后用这轮报回来 total_tokens 判是否过阈值;过了就把老 tool 段落
            //   折叠成一条摘要(模型二次调用生成),换回更短的历史,再落盘。压缩失败不致命于会话 ——
            //   记一笔后原样落盘(下次再压),不动走 REPL。last_total=0(这轮没拿到 usage)→ 不触发。
            if last_total > 0 {
                match compactor
                    .maybe_compact(
                        &messages,
                        last_total,
                        &ModelSummarizer {
                            client: &client,
                            provider: &provider,
                            api_key: &api_key,
                        },
                    )
                    .await
                {
                    Ok((new_msgs, report)) => {
                        // 同观测面:与 [ctx:stream:N] 一样写 stderr(journey §12 观测面),不扰 stdout 对话流。
                        eprintln!("{}", report.log_line());
                        if let CompactorReport::Compacted { .. } = report {
                            // 真发生了折叠 —— 换上压缩后的历史供落盘与下轮:
                            //   注意只在「真的折叠了」时换(to_vec 已在 maybe_compact 内 clone 过,这里直接赋)。
                            //   NoOp 时 maybe_compact 返回的就是原样 clone,赋上也等价;但省一次 Vec 重建仍走下面。
                            eprintln!(
                                "[compactor] 历史:{}→{} 条(老 tool 段落已折叠为一条摘要)",
                                messages.len(),
                                new_msgs.len()
                            );
                            messages = new_msgs;
                        } else {
                            // NoOp:maybe_compact 返回的就是原样,无变化;不替换 messages(省一次 Vec 重建)。
                        }
                    }
                    Err(e) => {
                        eprintln!("[compactor] 压缩失败,跳过本次压缩(下次再压):{e:#}");
                    }
                }
            }
            if let Err(e) = session::save(std::path::Path::new(&session_path), &messages) {
                eprintln!("[note] 会话本次写盘失败:{e:#}");
            }
        } else if messages.last().map(|m| m.role == "user").unwrap_or(false) {
            messages.pop();
        }
    }
}

/// 把 assistant 回复打印给人看。reasoning_content(若存在)顺手在正文前提示一句,
/// 但它绝不在 messages 历史里 —— §3.3 #3。
fn print_reply(reply: &crate::tools::AssistantReply) {
    if let Some(r) = reply.reasoning_content.as_deref().filter(|s| !s.is_empty()) {
        println!("\n(思考: {})", r);
    }
    println!("\n{}", reply.content.clone().unwrap_or_default());
}

/// P1-1 探针:发一个带 read_file 工具的请求,把**原始响应 JSON** 打印出来,不解析。
/// 目的:实测各家 tool_calls 字段到底长什么样,再据实写解析(已用完,保留作调试入口)。
/// 跑法:`cargo run -- probe`
async fn probe_tool() -> anyhow::Result<()> {
    let cfg = config::Config::load(std::path::Path::new("codeagent.toml"))?;
    let provider = cfg.default_provider()?.clone();
    let api_key = provider.api_key()?;
    let client = reqwest::Client::new();

    // P1-3 起:工具定义改由 ReadFile::schema() 生成,不再手写 JSON。
    let tools = vec![ReadFile.schema()];

    let messages = vec![
        Message::system("你是一个简洁的助手。需要看文件内容时,调用 read_file 工具。"),
        Message::user("这个项目用到了哪些第三方 crate?请实际查看 Cargo.toml 后再回答。"),
    ];

    let req = ChatRequest {
        model: provider.model.clone(),
        messages,
        stream: false,
        tools: Some(tools),
        stream_options: None, // 非流式探针:顶层 usage 自始回填,无需此开关。
    };

    // 关键:不解析,直接拿原始文本打出来 —— 看真实 tool_calls 字段结构。
    let resp_text = client
        .post(provider.chat_url())
        .bearer_auth(&api_key)
        .json(&req)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("探针请求发送失败: {e:#}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("探针 HTTP 非 2xx: {e:#}"))?
        .text()
        .await?;

    println!("=== provider: {} ({}) ===", cfg.default, provider.base_url);
    println!("=== 原始响应(未解析) ===");
    let pretty = serde_json::from_str::<serde_json::Value>(&resp_text)
        .ok()
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or(resp_text.clone()))
        .unwrap_or(resp_text);
    println!("{pretty}");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `cargo run -- probe` 走探针(打印 tool use 原始响应),仍保留作调试入口。
    // `cargo run -- --yolo` 跳过 destructive 工具的审批闸(实测不卡时开)。
    // `cargo run -- --no-stream` 关 P5 流式,走老非流式调用(流式出问题时 debug 回退口)。
    // `cargo run -- --resume` P7:启动即从 .codeagent_session.json 载入上次会话接着聊。
    // `cargo run -- --script` #22:headless 模式,REPL 读入不走 rustyline(纯 stdin),可被管道驱动
    //   (`Get-Content turns.txt | codeagent --script --yolo 2> usage.log`)。解锁 P6.1 曲线自动采集 + P7 自动 resume。
    // `cargo run -- --session-file <path>` P8:自定义会话文件路径(subagent 子进程隔离用;
    //   普通用户不带,走默认 .codeagent_session.json —— 向后兼容)。
    // 其余走 run()。args 可叠用(如 `--yolo --no-stream --resume`)。
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("probe") {
        probe_tool().await
    } else {
        let yolo = args.iter().any(|a| a == "--yolo");
        let no_stream = args.iter().any(|a| a == "--no-stream");
        let resume = args.iter().any(|a| a == "--resume");
        let script = args.iter().any(|a| a == "--script");
        // `--session-file <path>`:取其后一个 argv 作路径;无 flag → None(走默认 .codeagent_session.json)。
        let session_file = args
            .iter()
            .position(|a| a == "--session-file")
            .and_then(|i| args.get(i + 1).cloned());
        run(yolo, no_stream, resume, script, session_file).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ReadFile, WriteFile};

    /// 锁:全新文件(old 空)→ 每行 `+`、带 `--- /dev/null`、无 `-`。
    #[test]
    fn unified_diff_new_file_all_plus_lines() {
        let d = unified_diff("", "line1\nline2\n", "new.rs");
        assert!(
            d.contains("--- /dev/null"),
            "全新文件应有 /dev/null 旧侧: {d}"
        );
        assert!(d.contains("+++ new.rs"), "新侧应标目标路径: {d}");
        assert!(
            d.contains("+line1\n") && d.contains("+line2\n"),
            "每行应 +: {d}"
        );
        assert!(!d.contains("-line"), "全新文件不应有删除行: {d}");
    }

    /// 锁:内容不变 → 文案「无变化」(提醒闸仍问,防模型空转重写)。
    #[test]
    fn unified_diff_no_change_shows_noop_marker() {
        let same = "fn a() {}\nfn b() {}\n";
        let d = unified_diff(same, same, "x.rs");
        assert!(d.contains("无变化"), "不变应显式标无变化: {d}");
    }

    /// 锁:单行改 → 含该行 `-` 和对应 `+`。
    #[test]
    fn unified_diff_single_line_change_shows_minus_and_plus() {
        let old = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let new = "fn a() {}\nfn B() {}\nfn c() {}\n";
        let d = unified_diff(old, new, "x.rs");
        assert!(d.contains("-fn b() {}"), "应含旧的删除行: {d}");
        assert!(d.contains("+fn B() {}"), "应含新的增加行: {d}");
        // 公共行以 ` ` 前缀作 context。
        assert!(d.contains(" fn a() {}"), "公共行应作 context: {d}");
        assert!(d.contains(" fn c() {}"), "末尾公共行也应作 context: {d}");
    }

    /// 锁:大幅重写(LCS < 30%)→ 警告 + 行数对比,不展开全量(防刷屏)。
    #[test]
    fn unified_diff_major_rewrite_truncated_and_warns() {
        let old = "alpha\nbeta\ngamma\ndelta\n".repeat(3);
        let new = "完全\n不同\n的内容\n完全无关\n".repeat(3);
        let d = unified_diff(&old, &new, "x.rs");
        assert!(d.contains("大幅重写"), "应标大幅重写警告: {d}");
        assert!(!d.contains("+完全\n"), "大幅重写不应展开全量 diff: {d}");
    }

    /// 锁:yolo=true 时 write_file 直 Allow,不进 diff 分支(不显示 diff / 不问)。
    #[test]
    fn gate_check_yolo_allows_destructive_without_diff() {
        let mut gate = ApprovalGate {
            yolo: true,
            allow: crate::config::ApprovalConfig::default(),
        };
        // write_file 但 yolo —— 应直接 Allow,不该卡 stdin。
        let args = serde_json::json!({"path":"any.rs","content":"x"}).to_string();
        let v = gate.check(&WriteFile as &dyn Tool, &args);
        assert!(
            matches!(v, GateVerdict::Allow),
            "yolo 时 write_file 应直放: {v:?}"
        );
    }

    /// 锁:非 destructive(ReadFile)→ 直接 Allow,不过任何分支(读取本就免审)。
    #[test]
    fn gate_check_non_destructive_allows_without_prompt() {
        let mut gate = ApprovalGate {
            yolo: false,
            allow: crate::config::ApprovalConfig::default(),
        };
        let v = gate.check(&ReadFile as &dyn Tool, r#"{"path":"a.rs"}"#);
        assert!(matches!(v, GateVerdict::Allow), "读类工具应免审直放: {v:?}");
    }
}
