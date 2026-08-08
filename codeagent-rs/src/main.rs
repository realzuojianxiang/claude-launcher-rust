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

mod config;
mod tools;

use std::io::{self, Write};

use serde::{Deserialize, Serialize};

use crate::config::Provider;
use crate::tools::{
    finish_reason_from_str, sse_data_payload, sse_split, AssistantReply, Bash, FinishReason, Glob,
    ListDir, ReadFile, StreamAcc, StreamChunk, Tool, ToolResultMessage, WriteFile,
};

/// 对话历史中的一条消息。
/// 一个 Message 承载三种角色(system/user/assistant/tool),靠 role 字段区分;
/// tool_calls(assistant 用)与 tool_call_id(tool 用)都设可选 + skip,
/// 无关角色不序列化这些字段 —— 请求体保持每种角色只发该发的字段。
#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: String,
    /// assistant 回复含的工具调用 —— 回灌进历史时模型要能看见「我刚才调过」(concepts §4 要点 1)。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    tool_calls: Option<Vec<crate::tools::ToolCall>>,
    /// role:tool 时配对的 tool_call_id(concepts §3.2 配对要求)。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    tool_call_id: Option<String>,
}

impl Message {
    fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
    fn user(content: impl Into<String>) -> Self {
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
}

/// Chat Completions 响应体(只建模用得到的字段,其余靠 serde 忽略)。
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
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
) -> anyhow::Result<(crate::tools::AssistantReply, FinishReason)> {
    let req = ChatRequest {
        model: provider.model.clone(),
        messages: messages.to_vec(),
        stream: false,
        tools: tools.map(|t| t.to_vec()),
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
    Ok((choice.message, choice.finish_reason))
}

/// P5 流式调用的结局:正常收完 vs 被用户 Ctrl-C 中断。
/// 中断时**不**把半截 assistant 消息压回历史 —— 半截 tool_calls.arguments 可能是
/// 残缺 JSON,回灌会让模型糊涂;本轮作废、回 REPL 顶等下一句,最干净。
enum StreamOutcome {
    Completed(crate::tools::AssistantReply, FinishReason),
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
    let (content, reasoning, tool_calls, finish_raw) = acc.finalize();
    let finish = match finish_raw.as_deref() {
        Some(s) => finish_reason_from_str(s),
        None => {
            if !tool_calls.is_empty() {
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
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        reasoning_content: reasoning,
    };
    Ok(StreamOutcome::Completed(reply, finish))
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

impl ApprovalGate {
    /// 拦一道:返回 true 放行,false 拒绝。
    fn check(&mut self, tool: &dyn Tool, args: &str) -> bool {
        if !tool.is_destructive() || self.yolo {
            return true;
        }
        // 仅 bash 走前缀白名单(write_file 暂无路径白名单 —— 保守口径:所有写一律问)。
        // 拿不到 command(解析失败)就退回 y/n —— 默认安全,不因配置解析炸而误放行。
        if tool.name() == "bash" {
            if let Some(cmd) = extract_bash_command(args) {
                if self
                    .allow
                    .bash_allow_prefix
                    .iter()
                    .any(|p| cmd.starts_with(p.as_str()))
                {
                    return true; // 白名单命中:免审直放行。
                }
            }
        }
        // 给人看个一眼摘要:工具名 + arguments 头 200 字符(多了刷屏)。
        let brief: String = args.chars().take(200).collect();
        println!(
            "\n[审批] 即将执行 {}({}) —— 放行? [y/N]",
            tool.name(),
            brief
        );
        print!("> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        // 读取失败或非 y 一律视为拒绝 —— 默认安全。
        if io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        let line = line.trim().to_lowercase();
        line == "y" || line == "yes"
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
    if !gate.check(tool.as_ref(), &call.function.arguments) {
        return Ok(ToolResultMessage::new(
            call.id.clone(),
            format!(
                "[用户拒绝执行] 工具 {} 的这次调用被用户否决,未执行。请换一种不修改磁盘的方式继续,或先与用户确认。",
                tool.name()
            ),
        ));
    }
    let content = match tool.execute(&call.function.arguments) {
        Ok(out) => out,
        // 工具失败也回灌 —— 「丙(清楚)」写法的错误串给模型看,让它自纠正(§5)。
        Err(e) => format!("[工具执行失败] {}", e),
    };
    Ok(ToolResultMessage::new(call.id.clone(), content))
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
) -> anyhow::Result<()> {
    const MAX_TOOL_ROUNDS: usize = 8;
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
                Ok(StreamOutcome::Completed(r, f)) => (r, f),
                Ok(StreamOutcome::Interrupted) => {
                    // 用户 Ctrl-C 中断 —— 本回合作废,RePL 继续等下一句。
                    return Ok(());
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
            return Ok(());
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
    Ok(())
}

/// agent 主入口(P2:连续多轮 REPL;P3:扩工具集 + 审批闸;P5:流式 + Ctrl-C 中断)。
/// 外层 REPL:读一行 user 输入 → 跑内层 agent loop(可能多轮工具)→ 答完 → 再读下一行。
/// messages 跨轮**保留**(共享对话历史),所以你能追问「刚才那个文件第一行写了啥」之类
/// 依赖上文的问题 —— 这是 P2 把「跑一轮」升级为「真 agentic loop」的关键。
/// 退出:空行直接跳过(不浪费一次调用)、`/quit` 或 `exit` 退出、EOF(Ctrl-Z/Ctrl-D)退出。
/// `yolo=true`:destructive 工具(write_file/bash)不拦审批闸,顺跑(实测不卡时开)。
/// `no_stream=true`(CLI `--no-stream`):走老非流式调用(P5 流式 debug 回退口,可对照排查)。
///
/// P5 中断机制:启一个后台 task 装一个 `tokio::signal::ctrl_c()` 监听器,经 mpsc 通道把
///   「该中断当前生成」的信号发给主循环;每轮 agent 生成前从该通道取一条挂在 select 里。
///   用 mpsc(非 oneshot)是因为 single run_one_turn 内部 for 循环可能多轮工具调用,
///   每轮都要新挂一个信号接收器 —— oneshot 一次性,多轮就废了;mpsc 可多次取发。
async fn run(yolo: bool, no_stream: bool) -> anyhow::Result<()> {
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
    let mut messages = vec![Message::system(
        "你是一个简洁的 code agent。可用工具:read_file(读文件)、list_dir(列目录)、glob(按规则搜文件名)、write_file(写文件,会问人)、bash(跑命令,会问人)。需要时调相应工具,拿到结果后用中文直接回答用户问题。",
    )];

    // 工具表:加工具 = 在这里 Box::new 一个 impl Tool,与 P1 的「固定路由」彻底解耦。
    // schema 直接从这表派生,避免「schema 列表」和「工具列表」两处各自维护、容易对不上。
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadFile),
        Box::new(ListDir),
        Box::new(Glob),
        Box::new(WriteFile),
        Box::new(Bash),
    ];
    let tools_schemas: Vec<serde_json::Value> = tools.iter().map(|t| t.schema()).collect();
    let tools_slice: &[serde_json::Value] = &tools_schemas;
    let mut gate = ApprovalGate {
        yolo,
        allow: cfg.approval.clone(),
    };

    // REPL 外层。P5 流式已让正文逐 token 显示;P5.x 上 rustyline 后历史/编辑会更好,现仍裸 stdin。
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut input = String::new();
        // read_line 返回读到的字节数;0 表示 EOF(PowerShell Ctrl-Z 回车 / Unix Ctrl-D)。
        let n = io::stdin().read_line(&mut input)?;
        if n == 0 {
            println!(); // EOF 前补个换行,免得提示符贴着下一行。
            return Ok(());
        }
        let input = input.trim();
        if input.is_empty() {
            continue; // 空行跳过 —— 不浪费一次模型调用(P1 那版的「空就退出」在 REPL 语义下不对了)。
        }
        if input == "/quit" || input == "exit" {
            return Ok(());
        }

        messages.push(Message::user(input));
        // 清掉这轮间隔里早到的 Ctrl-C 信号(用户在 REPL 等待期连按了),免得本轮一进 agent loop
        // 就被秒中断 —— 只让「本轮生成期间」按下的 Ctrl-C 生效。
        while interrupt_rx.try_recv().is_ok() {}
        run_one_turn(
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
    // 其余走 run()。args 可叠用(如 `--yolo --no-stream`)。
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("probe") {
        probe_tool().await
    } else {
        let yolo = args.iter().any(|a| a == "--yolo");
        let no_stream = args.iter().any(|a| a == "--no-stream");
        run(yolo, no_stream).await
    }
}
