# P5 流式协议核证摘记

P5 把 Chat Completions 从非流式改成流式(SSE)。下面是实施前用 subagent 核证 OpenAI 官方 SDK + DeepSeek/NVIDIA NIM 偏差得到的「硬约束清单」,代码(`src/tools.rs::StreamAcc`、`src/main.rs::chat_completion_stream`)即照此实现。**只记已核证的事实 + 实施取舍**,不臆测。

## 帧格式

- `data: {json}\n\n`;末帧 `data: [DONE]\n\n`(`[DONE]` 非 JSON,**别 parse**)。
- `Content-Type: text/event-stream`。
- 中间可能夹注释行(`: keep-alive`,以 `:` 开头),解析时跳过、不断流。
- DeepSeek 与 OpenAI 完全一致。

## chunk 字段(精确路径)

帧顶层:`{id, object:"chat.completion.chunk", created, model, choices:[...], usage:null}`
- `object` 恒 `"chat.completion.chunk"`(OpenAI/DeepSeek)。
- `usage`:中间 chunk 全 `null`;末帧才回填(且 OpenAI 要 `stream_options.include_usage=true`,DeepSeek 自始回填;**NIM 多数不回填**——tolerate 始终 null,不据此判错)。

`choices[0]`:`{index, delta:{...}, finish_reason:null|...}`
- 注意有两个 `index`: choices[].index(候选序,n=1 时恒 0)与 delta.tool_calls[].index(工具调用序),别混淆。

### 2.1 正文增量 `delta.content`
- `Option<String>`。
- 首帧常是 `""`(或 null);中间是 token 片段串;末帧 `""`/缺失。
- **`""` 与 `null` 都跳过(空串 append 是 no-op),但绝不把 null 当「流结束」**——结束只看 finish_reason / [DONE]。
- 累积:字符串原样拼接,不 trim。

### 2.2 工具调用增量 `delta.tool_calls`
**这是流式最坑的部分**(OpenAI 官方 SDK 类型定义背书):
- `delta.tool_calls` 是数组,每元素 `{index: int 必填, id: Option, type: Option, function: {name: Option, arguments: Option<String>}}`。
- 增量分布:**首帧带 id + type + function.name + arguments 头一段;后续帧只带 index + function.arguments 片段**(id/type/name 为 null 或缺失)。
- **配对靠 `index`,绝不靠 `id`**(后续帧无 id)。OpenAI 官方类型 `index: int` 必填即明证。
- `function.arguments` 是**字符串拼接**(非 JSON parse 合),流完再 `serde_json::from_str` 一次;模型可能给非法 JSON,parse 失败要兜底。
- **NIM 偏差**:`tool_calls[].index` 听闻偶不回填——我们建成 `Option<i32>`,缺失时按帧内出现序派生,不崩。

### 2.3 finish_reason
- 中间所有 chunk 全 `null`;**只在末帧出现**。
- 取值 `stop | length | tool_calls | content_filter | insufficient_system_resource(DeepSeek 特有) | ...`。
- 我们用 `#[serde(other)]` 枚举兜底(DeepSeek 那个值落 Other,不崩)。

### 2.4 role / reasoning_content
- `role`:首帧 `"assistant"`,后续缺失/null——首次 set 不覆盖。
- `reasoning_content`(DeepSeek 特有,NIM 跑 DeepSeek-distill 类也有):`delta.reasoning_content` 字符串分片,在正式 `content` **之前**先流;单独累积,不进 messages 历史(§3.3 #3)。

## 累积铁律

1. tool_calls 配对**靠 index,绝不靠 id**。
2. arguments 是**字符串拼接**,非 JSON 合。
3. content 也是字符拼接;null/空都跳过,但 null 非流结束。
4. **chunk 层面不做 content/tool_calls 互斥假设**——独立累积,最后按 finish_reason 分类。
5. finish_reason 缺失(某些 proxy 末帧没带)→ 回退看 tool_calls 有无(§3.5 反直觉发现的反面回退)。

## 终止判据

- **两个并用**:见 `[DONE]` 即退;或流自然 EOF(NIM/vLLM 偶不发 `[DONE]` 直接断)。
- 顺序:OpenAI 体系末帧(finish_reason chunk)→ `[DONE]` 相邻而来。
- **错误帧兜底**:某些 proxy 直接发 `{"error":{...}}` 再断流,无 finish_reason 无 [DONE]——parse 失败时再 try error,命中即整体标错,不当空 content 吞掉。

## 两家偏差一句话

- **DeepSeek**:多 `delta.reasoning_content` + 多 `finish_reason: "insufficient_system_resource"`;其余 SSE 框架与 OpenAI 逐字对齐。
- **NIM**:协议级对齐 OpenAI,但 `tool_calls[].index` 偶不稳(防御 Option+派生)、`usage` 末帧多不回填(tolerate null)、EOF 即终态的保底要留着。

## 实施收尾(`src/main.rs::chat_completion_stream`)

- 终止两路(`[DONE]` / 自然 EOF)在循环外**合流到统一 finalize**,不单独构造(避免丢真实 finish_reason)。
- 中途打印:content 与 **reasoning_content 都边来边打**(`print!`+flush),用 `content_started`/`reasoning_opened` 两标志错开视觉。
  - **这条是实测反手改的**(见 journey §7.7):P5 初版只对 content 边打、reasoning 收尾整段打一次——结果 `(思考:)` 落在正文**之后**,违反协议「reasoning 先于 content 流」(§2.4)。改成 reasoning 也边来边打,它自然落在正文之前。收尾不再二次整段打,只 `if reasoning_opened { println!(")") }` 兜底封口。
- tool_calls 增量不边打 —— 它的亮相时机是收尾由审批闸问人(dispatch 触发),边打会先冒半个非法 JSON arguments 扰人。
- 复用非流式 `AssistantReply { content, tool_calls, reasoning_content }` 与 `FinishReason`(含 `#[serde(other)]`),流式只是一层 SSE 累积器在前面焊。

## 资料来源

- OpenAI 官方 Python SDK `chat_completion_chunk.py` —— `index: int` 必填、`id/function/type` 全 Optional、`function.arguments: Optional[str]`。
- DeepSeek 官方 API 文档 `create-chat-completion` —— 逐字 JSON 帧示例、`reasoning_content`、`insufficient_system_resource`。
- LangChain `langchain_openai/chat_models/base.py` `_convert_delta_to_message_chunk` —— index 配对、arguments 碎片 append 印证。
- NVIDIA NIM:官方文档站抽测时多 URL 404/空,偏差部分基于「OpenAI 兼容」自标 + 行业实践,**NIM 实跑再核证**(已在代码里加了防御性 Option index + null usage 容忍,实跑若发现新偏差补回)。
