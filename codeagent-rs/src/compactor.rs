// compactor —— P6.1 真压缩策略:当上下文逼近模型窗口上限时,把老的 tool_result 段落
// 用模型「二次调用」生成摘要替回去,腾出 token、延展长会话寿命。
//
// 设计取自 journey §12,三条铁律呼应「只记已发生的,不臆造」:
//
//   1. **阈值按模型来**:DeepSeek 上限约 1M、gpt-4o-mini 128k、本地模型更小 —— 故窗口上限
//      `max_context` 放 provider 段(config.rs::Provider),压缩策略参数放 [compaction] 段。
//      不写死任何「某模型该多大」—— 由配置带进来,代码不假设。
//   2. **压缩 = 模型再调一次**(非启发式截断):把「要压的旧消息」整段喂给同一 provider,
//      请它输出「用一段话总结上面这段对话的事实」,把这条 summary 顶回历史当 assistant
//      一条。用户的 pivotal 指令原话:「模型二次调用生成摘要(推荐)」。
//   3. **策略是一个纯函数**:`select_messages_to_compress` 只看 `&[Message]` 与参数,产出
//      「哪几条原始保留、哪几条整段换成一条 summary」的切片决策 —— 不碰网络、不碰时间、
//      不碰随机。故可注入 `FakeSummarizer` 硬测「该压的压对了、该留的留对了」,**自动可测**,
//      无需人盯真模型(journey §11.5「能 auto 测 vs 留本机」对齐)。
//
// 真模型压缩后「模型还认不认得前文」要真终端、真 key、人眼判召回 —— 留本机实测(journey §12.6
// 占位,不预先填假数字)。本模块锁住的是**策略正确性**:给定一个 messages 切片,该压谁、压完
// 切点对不对 —— 这层是纯逻辑,单测硬证。

use crate::config::Compaction;
use crate::Message;

// async-trait:summarize 是 async(要 await 非流式 chat_completion),而我们要用
// `dyn Summarizer`(测试注入 FakeSummarizer vs 运行期 ModelSummarizer 共一接口),故靠它。
use async_trait::async_trait;

// ─── 默认常量(被 config.rs 的 serde 缺省函数引用,故 pub)────────────────────────────
// rationale:70% 开窗别等打满再压(打满后连 summary 那次调用的 prompt 都装不下会有去无回);
// 40% 收尾给后续若干轮留头;保留最近 4 个用户轮原始让模型对眼下几轮有全量细节(§3.3 #2 的
// 「tool_calls 回灌要让模型看见我刚调过」精神延伸到「最近几轮原貌保留」)。
pub const DEFAULT_COMPACT_AT_RATIO: f64 = 0.7;
pub const DEFAULT_COMPACT_TO_RATIO: f64 = 0.4;
pub const DEFAULT_KEEP_RECENT_TURNS: usize = 4;

/// 压缩决策 —— `select_messages_to_compress` 的纯输出。描述「整段历史切成三块」:
///
/// ```text
///   [keep_head]    | [summarize: 这些被换成一条 summary] | [keep_tail: 最近几轮原始保留]
///   system + 首批   |  老的 tool 段落(占大头)             |  最近 N 用户轮及其后 assistant/tool
/// ```
///
/// `keep_head` 与 `keep_tail` 的消息**原样**保留;`summarize` 这段被折叠成一条 assistant
/// summary 消息(`Summarizer::summarize` 产出),插回 head 与 tail 之间。
#[derive(Debug)]
pub struct CompressPlan {
    /// 原样保留的「头」段(通常是 system + 头几条早期 user/assistant)。
    pub keep_head: Vec<Message>,
    /// 要被折叠成一条 summary 的「中段」(老的、占 token 大头的 tool_result 段落)。
    pub summarize: Vec<Message>,
    /// 原样保留的「尾」段(最近 N 个用户轮及其后全部 assistant/tool 消息,全量细节)。
    pub keep_tail: Vec<Message>,
}

impl CompressPlan {
    /// 没什么要压(任何一段空、或总量没超阈值)—— `maybe_compress` 据此 no-op。
    pub fn is_empty(&self) -> bool {
        self.summarize.is_empty()
    }

    /// 张贴出决策概要给人看(stderr / journey 日志)。不进 messages 历史。
    pub fn explain(&self) -> String {
        format!(
            "compress: keep_head={} summarize={} keep_tail={}",
            self.keep_head.len(),
            self.summarize.len(),
            self.keep_tail.len()
        )
    }
}

/// 压缩动作的抽象 —— 把中段要压的 `&[Message]` 总结成一条 assistant 消息。
///
/// 默认实现 `ModelSummarizer` 用同一 provider 非流式再调一次模型(用户的 pivotal 指令);
/// 测试注入 `FakeSummarizer` 产出可断言的固定 summary —— 故策略纯函数自动可测。
///
/// trait 持 `&self` 而非 `&mut self`:summarize 逻辑上无累计可变状态,借用更宽。
/// `#[async_trait]` 让 `summarize` 可 async + 用 `dyn Summarizer`(见上方 use)。
#[async_trait]
pub trait Summarizer {
    async fn summarize(&self, to_compress: &[Message]) -> anyhow::Result<Message>;
}

/// 给一次压缩决策的所有量戴上「都从哪儿来」的眼睛,避免散传 5 个裸参被 clippy 唠叨。
/// `max_context` 与 `Compaction`(比率/保留轮数)聚合在此,`select_messages_to_compress` 只读。
pub struct Compactor {
    /// 该 provider 模型的窗口上限(token)。来自 provider.max_context 或 Compaction 兜底。
    pub max_context: u64,
    /// 压缩策略参数(比率 + 保留轮数)。来自 [compaction] 段或其 default。
    pub params: Compaction,
}

impl Compactor {
    pub fn new(max_context: u64, params: Compaction) -> Self {
        Self {
            max_context,
            params,
        }
    }

    /// 当前 total 是否过了「开窗」阈值?P6.1 的触发闸 —— REPL 每轮收工后用最近 usage.total 调它。
    ///
    /// 取上次模型回的 `total_tokens` 作上下文当前体积的近似度量(不是逐 token 精算,
    /// 是模型/代理**报回来的**上下文总量 —— P6 §10.3 已用它画 token 曲线,稳定可用)。
    pub fn should_compact(&self, last_total: u64) -> bool {
        let threshold = threshold_tokens(self.max_context, self.params.compact_at_ratio);
        last_total >= threshold
    }

    /// 纯策略:给定历史切片 + 参数,产出「头/中段折叠/尾保留」三段决策。不碰网络、
    /// 不碰时间、不碰随机 —— 这条是单测硬证「该压对、该留对」的承重函数。
    ///
    /// 切点逻辑(见 journey §12.3):
    ///   1. `keep_head`:始终保留首条消息(约定是 system;若首条不是 system 则仍保留首条作为锚)。
    ///      另外若紧随首条有「无 tool_call_id 的早期 user/assistant」(非 tool 段落),一并留头
    ///      —— 这些通常很短、不是 token 大头,留全比压好(早期开场对话删了对召回伤)。
    ///   2. `keep_tail`:从末尾往前数,数够 `keep_recent_turns` 个「user 轮」(role==user 且无
    ///      tool_call_id —— 把 tool_result 灌进来的 role:tool 不算用户轮),该 user 轮及其后**全部**
    ///      消息原样保留(含其后的 assistant tool_calls + role:tool 回灌)—— §3.3 #2 的连续性。
    ///   3. `summarize`:头与尾之间的中段 = 历史的「中段老 tool 段落」,占 token 大头,折叠成一条。
    ///   4. 若头尾相接 / 中段空 —— `is_empty` 为真,调用方 no-op(`maybe_compress` 据此不调模型)。
    pub fn select_messages_to_compress(&self, messages: &[Message]) -> CompressPlan {
        let n = messages.len();
        if n <= 1 {
            // 0/1 条:留着,谈不上压。
            return CompressPlan {
                keep_head: messages.to_vec(),
                summarize: vec![],
                keep_tail: vec![],
            };
        }

        // 1) keep_head:首条(锚)+ 紧随的非 tool 段落早期消息(短、不该压)。
        let mut head_end = 1usize; // 首条已纳入 head
        while head_end < n {
            let m = &messages[head_end];
            let is_early_non_tool =
                m.role != "tool" && m.tool_call_id.is_none() && !has_tool_calls(m);
            if is_early_non_tool {
                head_end += 1;
            } else {
                break; // 碰到第一个 tool 段落 / assistant 带 tool_calls —— head 到此为止。
            }
        }

        // 2) keep_tail:从末尾往前数 keep_recent_turns 个 user 轮,切点 = 该 user 轮下标。
        let tail_start = find_tail_start(messages, self.params.keep_recent_turns);

        // 校正:tail 不能伸进 head 里(否则中段空)。保持 tail_start >= head_end,
        // 否则把中段判空(no-op)。
        if tail_start <= head_end {
            return CompressPlan {
                keep_head: messages[..tail_start].to_vec(),
                summarize: vec![],
                keep_tail: messages[tail_start..].to_vec(),
            };
        }

        CompressPlan {
            keep_head: messages[..head_end].to_vec(),
            summarize: messages[head_end..tail_start].to_vec(),
            keep_tail: messages[tail_start..].to_vec(),
        }
    }

    /// 端到端执行一次压缩:若 `should_compact` 且 `select` 出非空中段,调 `Summarizer`
    /// 生成 summary,粘回成新的历史;否则原样返回。REPL 收工后调它。
    /// `report` 回收一条给 stderr 的人话决策日志(含没触发时的「no-op」),供 journey 观测。
    /// async 因 Summarizer::summarize async(默认实现要 await 模型调用);FakeSummarizer 立即返回。
    pub async fn maybe_compact(
        &self,
        messages: &[Message],
        last_total: u64,
        summarizer: &dyn Summarizer,
    ) -> anyhow::Result<(Vec<Message>, CompactorReport)> {
        if !self.should_compact(last_total) {
            return Ok((
                messages.to_vec(),
                CompactorReport::NoOp {
                    last_total,
                    threshold: threshold_tokens(self.max_context, self.params.compact_at_ratio),
                },
            ));
        }
        let plan = self.select_messages_to_compress(messages);
        if plan.is_empty() {
            return Ok((
                messages.to_vec(),
                CompactorReport::NoOp {
                    last_total,
                    threshold: threshold_tokens(self.max_context, self.params.compact_at_ratio),
                },
            ));
        }
        let explain = plan.explain();
        let (summarize_count, head_count, tail_count) = (
            plan.summarize.len(),
            plan.keep_head.len(),
            plan.keep_tail.len(),
        );
        let summary_msg = summarizer.summarize(&plan.summarize).await?;
        let mut out = Vec::with_capacity(plan.keep_head.len() + 1 + plan.keep_tail.len());
        out.extend(plan.keep_head);
        out.push(summary_msg);
        out.extend(plan.keep_tail);
        Ok((
            out,
            CompactorReport::Compacted {
                last_total,
                head_count,
                summarize_count,
                tail_count,
                explain,
            },
        ))
    }
}

/// maybe_compact 的决策回单 —— 写 stderr(journey §12 观测面),不进 messages。
#[derive(Debug)]
pub enum CompactorReport {
    /// 没触发:total 还没到阈值,或选中段空。带 last_total 与当时阈值便于看「离压缩还多远」。
    NoOp { last_total: u64, threshold: u64 },
    /// 触发并已折叠:带三段条数 + 决策概要。
    Compacted {
        last_total: u64,
        head_count: usize,
        summarize_count: usize,
        tail_count: usize,
        explain: String,
    },
}

impl CompactorReport {
    /// 一行决策日志(写 stderr,与 report_usage 的 [ctx:...] 同观测面)。
    pub fn log_line(&self) -> String {
        match self {
            CompactorReport::NoOp {
                last_total,
                threshold,
            } => format!(
                "[compactor:noop] total={last_total} threshold={threshold} (未到阈值,不压)"
            ),
            CompactorReport::Compacted {
                last_total,
                head_count,
                summarize_count,
                tail_count,
                explain,
            } => format!(
                "[compactor:done] total={last_total} {explain} (中段 {summarize_count} 条→1 summary;留头 {head_count} 尾 {tail_count})"
            ),
        }
    }
}

/// 触发阈值 token 数 = max_context × compact_at_ratio,夹到至少 1 免 0 退化。
fn threshold_tokens(max_context: u64, ratio: f64) -> u64 {
    let t = (max_context as f64) * ratio;
    if t.is_finite() && t >= 1.0 {
        t as u64
    } else {
        1
    }
}

/// 是否带 tool_calls(assistant 专有)。包成函数,避免在散处手写同一段条件。
fn has_tool_calls(m: &Message) -> bool {
    m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
}

/// 从末尾往前数 `keep_turns` 个 user 轮(role==user 且无 tool_call_id),
/// 返回「该 user 轮的下标」即 tail 切点。找不到够数(历史太短)则 tail 从最早一个 user 轮起;
/// 一条 user 轮都没(全 system 或纯 tool)则 tail = n(不保留尾部,中段全压 —— 退化但安全)。
fn find_tail_start(messages: &[Message], keep_turns: usize) -> usize {
    let n = messages.len();
    if n == 0 || keep_turns == 0 {
        return n; // 不留尾 = 全部进中段(仅当 keep_turns=0 退化时;正常不会传 0)。
    }
    let mut seen_user = 0usize;
    let mut cut = n;
    // 从后往前找 user 轮边界。
    for (i, m) in messages.iter().enumerate().rev() {
        let is_user_turn = m.role == "user" && m.tool_call_id.is_none();
        if is_user_turn {
            seen_user += 1;
            cut = i; // 记最后一次(即第 keep_turns 个 user 轮)的起点
            if seen_user >= keep_turns {
                break;
            }
        }
    }
    // 没数够 keep_turns 个 user 轮 —— 退化为「从最早那条 user 轮起全留尾」(cut 已是它);
    // 一条都没有 —— cut 仍 = n,即 tail 全空(中段压全部),退化但安全,不丢 system。
    if seen_user == 0 {
        // 兜底:整个 messages 都不是 user 轮(料想极少)—— 不强行压 system,把全量留作 tail,中段空。
        // 判空会兜住调用方 no-op,等模型真的回了含 user 的历史再来。
        return 0;
    }
    cut
}

// ─── 默认 Summarizer:用同一 provider 非流式再调一次模型 ───
//
// 这是 user pivotal 指令的实现:「模型二次调用生成摘要(推荐)」。把要压的中段整段
// 当 user 消息包成「请总结」喂回同一 provider,拿回的 assistant 终答当 summary 消息顶回。
//
// 注:summarize 调用**走非流式**(`chat_completion` 那条)—— 不用流式,因为 summary 不需要
// 逐 token 打给人看,非流式一次性拿回更省事、且与 P6 §10.3 非流式曲线路径同源。
// 具体调用的是 main.rs 的 chat_completion;此处只在(main.rs 的)压缩接线处实例化它 + 注入。
// 本模块**不**直接依赖 reqwest —— 把「真调模型」的细节留给 main.rs(ModelSummarizer
// 定义在 main.rs,这里只给 trait),这样 compactor.rs 保持纯逻辑 + trait,可独立单测。

#[cfg(test)]
mod tests {
    //! P6.1 压缩策略单测 —— 纯逻辑,不联网、不调模型。注入 FakeSummarizer,断:
    //!   · 该压的中段(老 tool_result 段落)选对了;
    //!   · 该留的头(system + 早期非 tool)与尾(最近 N user 轮 + 其后 assistant/tool)都原样留;
    //!   · should_compact 阈值按 max_context × compact_at_ratio 算对;
    //!   · 退化(历史太短 / 全无 user 轮 / 头尾相接)安全 no-op,不丢 system、不 panic。
    use super::*;
    use crate::tools::ToolCall;

    /// 假摘要器:不管喂什么,回一条 content="(summary of N 条消息)" 的 assistant 消息。
    /// 测的是**策略切点**对不对,不是摘要文本质量 —— 后者要真模型,留本机(journey §12.6)。
    struct FakeSummarizer;
    #[async_trait]
    impl Summarizer for FakeSummarizer {
        async fn summarize(&self, to_compress: &[Message]) -> anyhow::Result<Message> {
            Ok(Message {
                role: "assistant".into(),
                content: format!("[summary of {} 条消息]", to_compress.len()),
                tool_calls: None,
                tool_call_id: None,
            })
        }
    }

    /// 帮手:f64 ratio 配 u64 max → Compactor。
    fn compactor(max_context: u64, at: f64, to: f64, keep: usize) -> Compactor {
        Compactor::new(
            max_context,
            Compaction {
                compact_at_ratio: at,
                compact_to_ratio: to,
                keep_recent_turns: keep,
            },
        )
    }

    /// 构一条任意消息的帮手(role 暴露给测试,**不**泄漏任何会臆造的语义)。
    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
    /// 一条 assistant 带 tool_calls 的帮手(验证它判作 head/tail 边界、不进中段误判)。
    fn assistant_with_tools(content: &str, call_id: &str) -> Message {
        Message {
            role: "assistant".into(),
            content: content.into(),
            tool_calls: Some(vec![ToolCall {
                id: call_id.into(),
                r#type: "function".into(),
                function: crate::tools::ToolCallFunction {
                    name: "read_file".into(),
                    arguments: "{}".into(),
                },
            }]),
            tool_call_id: None,
        }
    }
    /// 一条 role:tool 回灌(tool_call_id 配对)。
    fn tool_result(call_id: &str, content: &str) -> Message {
        Message {
            role: "tool".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
        }
    }

    #[test]
    fn should_compact_respects_threshold() {
        let c = compactor(1_000_000, 0.7, 0.4, 4); // DeepSeek 1M × 0.7 = 700k
        assert!(!c.should_compact(699_999), "阈值线之下不应触发");
        assert!(c.should_compact(700_000), "恰好到阈值应触发");
        assert!(c.should_compact(900_000), "超过阈值必触发");
    }

    #[test]
    fn select_keeps_first_as_head_alone() {
        // system + 早期非 tool 段落全留头;中段是老 tool 段落;尾 = 最近 N user 轮。
        let c = compactor(1_000_000, 0.7, 0.4, 2);
        let msgs = vec![
            msg("system", "你是 code agent"),
            msg("user", "问1"),
            assistant_with_tools("我去读文件", "c1"),
            tool_result("c1", "大段文件内容..."),
            msg("assistant", "答1"), // 中段里一条非 tool 也可能(早期后),属中段
            msg("user", "问2"),
            assistant_with_tools("我再读", "c2"),
            tool_result("c2", "更大段内容..."),
            msg("user", "问3"),
            msg("user", "问4"),
        ];
        let plan = c.select_messages_to_compress(&msgs);
        // head = system + "问1"(紧随首条的非 tool 早期消息)。
        // 注意:assistant_with_tools 不算 is_early_non_tool(有 tool_calls)→ head 纳到 "问1" 为止。
        assert_eq!(plan.keep_head.len(), 2);
        assert_eq!(plan.keep_head[0].role, "system");
        assert_eq!(plan.keep_head[1].content, "问1");
        // tail = 最近 2 个 user 轮("问3"、"问4")起 → 从末尾数 2 个 user:问4(idx8) + 问3(idx7),
        // 但 idx7=tool_result(c2)(非 user),idx8=user问3,idx9=user问4 → tail_start = 8。
        // tail = idx8..=idx9 = [user问3, user问4] = 2 条。
        assert_eq!(plan.keep_tail.len(), 2);
        assert_eq!(plan.keep_tail[0].content, "问3");
        assert_eq!(plan.keep_tail[1].content, "问4");
        // 中段 = idx2..8 = assistant_with_tools + tool_result + assistant答1 + user问2
        //        + assistant_with_tools + tool_result = 6 条(老 tool 段落 + 其中夹的非 tool)。
        assert_eq!(plan.summarize.len(), 6);
        assert!(!plan.is_empty());
    }

    #[test]
    fn head_greedy_eats_early_non_tool_until_first_tool_segment() {
        // 首条后若是连续多条非 tool user/assistant(开场散文),head 一并吞,直到碰 tool 段落。
        let c = compactor(1_000_000, 0.7, 0.4, 1);
        let msgs = vec![
            msg("system", "sys"),
            msg("user", "开场1"),
            msg("assistant", "答开场1"),
            msg("user", "开场2"),
            msg("assistant", "答开场2"),
            assistant_with_tools("开始调工具", "c1"), // ← head 到此为止
            tool_result("c1", "内容"),
            msg("user", "最近一问"),
        ];
        let plan = c.select_messages_to_compress(&msgs);
        // head = system + 开场1 + 答开场1 + 开场2 + 答开场2 = 5 条(全是非 tool 早期链,被贪吃)。
        assert_eq!(plan.keep_head.len(), 5);
        // tail = 最近 1 个 user 轮起 = [user最近一问] = 1 条(idx7)。
        assert_eq!(plan.keep_tail.len(), 1);
        assert_eq!(plan.keep_tail[0].content, "最近一问");
        // 中段 = idx5..7 = [assistant_with_tools(调工具), tool_result(内容)] = 2 条。
        assert_eq!(plan.summarize.len(), 2);
    }

    #[test]
    fn too_short_history_is_noop() {
        let c = compactor(1_000_000, 0.7, 0.4, 4);
        let msgs = vec![msg("system", "sys"), msg("user", "只问一句")];
        let plan = c.select_messages_to_compress(&msgs);
        assert!(plan.is_empty(), "历史太短:中段空,no-op");
        assert_eq!(plan.keep_tail.len(), 1, "尾部至少含那唯一 user 轮");
    }

    #[test]
    fn head_tail_meet_yields_empty_middle() {
        // 开场全是 user/assistant 非 tool 段落 + 尾部需求覆盖掉全部 → 中段空 no-op。
        let c = compactor(1_000_000, 0.7, 0.4, 10); // 要留 10 个 user 轮,但只有 1 个
        let msgs = vec![
            msg("system", "sys"),
            msg("user", "唯一一句"),
            msg("assistant", "唯一一答"),
        ];
        let plan = c.select_messages_to_compress(&msgs);
        assert!(
            plan.is_empty(),
            "keep_turns 超过实际 user 轮 → tail 顶到最早 user,中段空"
        );
        assert_eq!(plan.keep_tail[0].content, "唯一一句");
    }

    #[test]
    fn no_user_turn_degrades_safe() {
        // 退极端:历史只有 system + 一串 tool 段落,一条 user 轮都没有。
        let c = compactor(1_000_000, 0.7, 0.4, 2);
        let msgs = vec![
            msg("system", "sys"),
            assistant_with_tools("t1", "c1"),
            tool_result("c1", "x"),
            assistant_with_tools("t2", "c2"),
            tool_result("c2", "y"),
        ];
        let plan = c.select_messages_to_compress(&msgs);
        assert!(
            plan.is_empty(),
            "一条 user 轮都没有 → tail 退化把全量留作 tail,中段空 no-op,不丢 system"
        );
        assert_eq!(plan.keep_tail.len(), msgs.len());
        assert_eq!(plan.keep_head.len(), 0);
    }

    #[tokio::test]
    async fn maybe_compact_uses_fake_summarizer_and_stitches() {
        // 触发 + 注入 FakeSummarizer:中段折叠成 1 条(标 [summary of N 条消息]),头尾原样拼回。
        let c = compactor(100, 0.5, 0.4, 1); // 极小窗口便于触发:max=100, 阈值=50
        let msgs = vec![
            msg("system", "sys"),
            msg("user", "开场"),
            assistant_with_tools("调工具", "c1"),
            tool_result("c1", "大段"),
            msg("user", "最近问"),
        ];
        let total = 60u64; // > 50 阈值 → 触发
        let (out, report) = c
            .maybe_compact(&msgs, total, &FakeSummarizer)
            .await
            .unwrap();
        match report {
            CompactorReport::Compacted {
                summarize_count, ..
            } => assert_eq!(
                summarize_count, 2,
                "中段 = assistant_with_tools + tool_result"
            ),
            CompactorReport::NoOp { .. } => panic!("total 超阈值应触发 Compacted,不是 NoOp"),
        }
        // head(sys + 开场) + 1 summary + tail(最近问) = 4 条(原 5 条少 1,因 2 折 1)。
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].role, "system");
        assert_eq!(out[1].content, "开场");
        assert_eq!(out[2].role, "assistant");
        assert!(
            out[2].content.starts_with("[summary of"),
            "中段折叠点应是 FakeSummarizer 输出"
        );
        assert_eq!(out[2].content, "[summary of 2 条消息]");
        assert_eq!(out[3].content, "最近问");
    }

    #[tokio::test]
    async fn maybe_compact_noop_below_threshold_keeps_messages_intact() {
        let c = compactor(1_000_000, 0.7, 0.4, 2);
        let msgs = vec![
            msg("system", "sys"),
            msg("user", "问1"),
            assistant_with_tools("t", "c1"),
            tool_result("c1", "x"),
            msg("user", "问2"),
        ];
        let total = 1000u64; // 远低于 700k 阈值
        let (out, report) = c
            .maybe_compact(&msgs, total, &FakeSummarizer)
            .await
            .unwrap();
        assert!(matches!(report, CompactorReport::NoOp { .. }));
        // NoOp 必须原样返回,逐条相等(没误折叠、没误调模型)。
        assert_eq!(out.len(), msgs.len());
        for (a, b) in msgs.iter().zip(out.iter()) {
            assert_eq!(a.content, b.content);
        }
    }

    #[test]
    fn report_log_line_has_ctx_tag_for_stderr() {
        // 两种 report 的 log_line 都应带 [compactor:...] 前缀,与 [ctx:stream:N] 同观测面。
        let noop = CompactorReport::NoOp {
            last_total: 10,
            threshold: 50,
        };
        assert!(noop.log_line().starts_with("[compactor:noop]"));
        let done = CompactorReport::Compacted {
            last_total: 100,
            head_count: 1,
            summarize_count: 3,
            tail_count: 2,
            explain: "compress: keep_head=1 summarize=3 keep_tail=2".into(),
        };
        assert!(done.log_line().starts_with("[compactor:done]"));
    }
}
