# 造自己的 Code Agent — 学习建造日志

> 这份文档是我们一步步造一个 code agent 的**建造日志**:每次探讨、每个决定、调研结论、踩到的坑,都追加在末尾。它随项目长 —— 对话即文档,文档即项目记忆。
>
> 项目目标:**学习、逐步深入、造自己的工具、推广造福人类。**

---

## 0. 缘起(2026-08-06)

当前 `claude-launcher` 是「调用 Claude Code 的桌面启动器」。自然想到:**不能自己开发一个类似 Claude Code 的 code agent 吗?** 一步一步来,从简单到复杂,共同学习。于是开 `codeagent-rs/` 子项目,与 launcher 完全独立、互不依赖,纯做 agent 内核探索。

### 0.1 拆解:一个 code agent 到底是三层

| 层 | 名字 | 做什么 | P0~P8 对应 |
|---|---|---|---|
| L1 | Agent loop | LLM ↔ 工具 ↔ 环境 的循环(ReAct / tool-use loop)。这是 code agent 的本质 | P1~P2 |
| L2 | Tools | 读/写文件、跑命令、搜索 —— agent 的手脚 | P3 |
| L3 | Harness | 交互 UI、权限审批、会话管理、上下文压缩、配置 | P4~P8 |

Claude Code 真正的壁垒不在某一层,而在**三层的耦合质量**:工具输出格式如何让模型不易犯错、上下文满了怎么压、错误怎么反馈让模型自我纠正。这些是迭代出来的,不是设计出来的 —— 所以路线定为「能跑的最小骨架 → 每步加一个使其更"像 agent"的能力」,每步可独立 demo。

### 0.2 决定记录(各方拍板)

| 决定点 | 选择 | 理由 |
|---|---|---|
| 语言/栈 | **Rust** | 与现有 launcher 同栈,可复用 reqwest/tokio,最终能塞进 Tauri 做桌面 agent。学习曲线稍陡但与现有积累一致 |
| 模型源 | **DeepSeek**(正式账号+key);NVIDIA 代理**先不动** | 有正式 key;起步阶段不动现有 launcher 的代理,隔离开发 |
| 第一里程碑深度 | **理解 agent 内核** | 重点放 tool loop / 上下文管理 / 错误回灌,UI 用最简 CLI。先做对的「大脑」 |
| 本次做到哪步 | **先搭 P0 骨架并跑通** | 一切的地基 |
| API key 用法 | **环境变量 `DEEPSEEK_API_KEY`(最安全)** | key 永远不落进项目目录、不入 git、不进对话 |
| API 协议 | **OpenAI 兼容**(`https://api.deepseek.com`) | 行业事实标准,学完可平移到本地 ollama/vLLM/各家国产模型;Anthropic 格式只有 Claude 系用。即便今后要接 Anthropic,在 OpenAI 格式上学到的 agent 内核也能平移 |

### 0.3 迭代路线图(会随进展修订)

| 阶段 | 目标 | 能演示什么 |
|---|---|---|
| **P0** ✅ 本次 | CLI 读一行 → 调 DeepSeek 非流式 → 打印回复 | 一个能聊天的客户端 |
| P1 | 加第一个工具 `read_file` | **第一次出现 agent 行为** —— 模型决定调工具 → 执行 → 回灌 → 回答 |
| P2 | 多轮 tool loop | 真正的 agentic loop(读→改→读确认,连续多步) |
| P3 | 扩工具集:write / bash / grep / glob | 能改代码了 |
| P4 | 权限审批(写/执行前 y/n) | 不会乱删文件 |
| P5 | 流式输出 + 中断 | 体验接近真 agent |
| P6 | 上下文管理(长对话压缩/摘要) | 不爆 context |
| P7 | 会话持久化(保存/恢复) | 可断点续做 |
| P8 | MCP / subagent / diff 审批 UI | 往 Claude Code 靠拢 |

### 0.4 技术调研:DeepSeek API(2026-08-06)

来源:官方文档 https://api-docs.deepseek.com/

- **两个 base_url**:
  - `https://api.deepseek.com` —— OpenAI 兼容格式(`/chat/completions`,标准 OpenAI messages 结构,`Authorization: Bearer`)
  - `https://api.deepseek.com/anthropic` —— Anthropic 兼容格式
- 支持 **tool use / function calling**(P1+ 命根子)。基础页未含细节,接进 P1 时再去 `/guides/tool_calls` 细查。
- 鉴权:`"Authorization: Bearer ${DEEPSEEK_API_KEY}"`
- 模型名:`deepseek-v4-flash` / `deepseek-v4-pro`(本 P0 用 flash)。文档注明「直接用 `deepseek-v4-flash` 即拿到最新版本」。
- 选 OpenAI 兼容而非 Anthropic 兼容,理由见 0.2「API 协议」行。

---

## 1. P0 完成(2026-08-06)

### 1.1 文件

- `codeagent-rs/Cargo.toml` —— 依赖刻意极小:`tokio`(macros+rt) + `reqwest`(json+rustls-tls) + `serde` + `serde_json` + `anyhow`。
- `codeagent-rs/src/main.rs` —— P0 单文件骨架。
- `codeagent-rs/.gitignore` —— 忽略 `target/`、`.env`、`*.key`、`code-agent-apikey.txt`(双保险)。
- `codeagent-rs/.env.example` —— 仅格式示意,真实 key 不入此文件。

### 1.2 代码设计意图(为后续铺路,关键)

P0 虽只 ~80 行,刻意留了三个让 P1+ 顺接的「生长点」:

1. **`messages: Vec<Message>` 抽出来**:P0 只塞 system + user 两条,但 P2 多轮循环时直接 `.push()` 即可,主结构不动。`Message` 的 `role` 字段已按 OpenAI 格式备齐(system/user/assistant/tool),P1 用到 `"tool"` 角色无需改结构。
2. **API 调用封进 `chat_completion()`**:agent loop 每一轮都复用它。P1 加工具时,只要在「拿到 `reply` 之后」加一个「reply 里有没有 tool_call?」的分支 —— 不重写。
3. **`run()` 用 `loop` 框起(虽然 P0 只跑一轮)**:P2 多轮 tool loop 时改循环体即可,把单轮体换成「模型输出 tool_call?执行→回灌→再问」。

### 1.3 三个可深挖的「为什么」

- **为什么用 `anyhow` 而非 `Box<dyn Error>`**:agent loop 错误源多(网络错、JSON 解析错、工具执行错),anyhow 的 `?` + `.context()` 注入调试上下文,P1+ 反复有用,值得现在引入。
- **为什么 P0 选非流式**:先把「能调通、能解析」跑通,P5 再上 SSE 流式(届时 `stream: true` + 按行解码,和你 launcher NVIDIA 代理踩过的 SSE 跨 chunk UTF-8 坑同源 —— 正好教学)。
- **为什么坚持 key 不读进对话/key 文件不进 git**:已有人因此 key 被永久暴露在 git 历史里。本约定:真 key 只在环境变量,`code-agent-apikey.txt` 仅作本地暂存且被 .gitignore 双保险忽略。

### 1.4 跑法

**首次配置(一次性,永久生效)**——把真 key 写进 Windows 用户环境变量,不入库不入文件:

```powershell
# 去 DeepSeek 控制台复制 sk- 开头那串 key
[Environment]::SetEnvironmentVariable("DEEPSEEK_API_KEY", "你的真实key", "User")
# 当前窗口补注入(写注册表只对新窗口生效,当前窗口要手动注一次):
$env:DEEPSEEK_API_KEY = [Environment]::GetEnvironmentVariable("DEEPSEEK_API_KEY", "User")
```

**日常跑**——新开 PowerShell 直接:

```powershell
cd D:\BaiduSyncdisk\ai-agent\claude-launcher\codeagent-rs
cargo run
# > 介绍一下你的模型   ← 看到提示符后手敲输入,回车
# (模型回复)
```

> ⚠️ 两个坑已踩过,别再踩:
> 1. **别用 `"内容" | cargo run` 管道喂 stdin**——PowerShell 管道把字符串当对象传,模型收不到,会回「内容不完整」。必须 `cargo run` 后手敲。
> 2. **别把 key 存进 `code-agent-apikey.txt`**——我们曾误把文档片段当 key,含空格/制表符,reqwest 拼 Authorization 头报 `builder error`。真 key 只走环境变量。

### 1.5 验证结论

**编译门禁 ✅**:`cargo check` 全绿,零警告零错误,25 秒编完(含依赖)。P0 骨架的类型/编译验证通过。

**联通验证 ⏳(卡在 key,非代码问题)**:

- `cargo run` 实跑报 `Error: 调用 DeepSeek 失败: 请求发送失败(reqwest error): builder error`,exit code 1。
- 诊断经过(本身是 agent 开发教学点):reqwest 默认 `?` 丢上下文,改用 `{e:#}` + `.context()` 后,`builder error` 后面**仍无 cause chain** —— 说明是「构造请求阶段」失败且错误里没带根因。靠外部诊断打印 key 长度/前缀 + 读 key 文件内容才定位。
- **根因**:`code-agent-apikey.txt` 里存的不是真 key,是 DeepSeek 文档片段(`base_url (OpenAI)\tht...`、`base_url (Anth`),长度 190、含制表符/括号/空格。`.bearer_auth()` 拿到这种含非法字符的串,reqwest 拒绝拼进 HTTP header → `builder error`。与 TLS/网络/代码无关。
- **修复**:用 DeepSeek 控制台真 key(`sk-...` 开头,无空格无制表符)直接 `SetEnvironmentVariable` 写入,不读那个错文件。待用户拿到真 key 复跑后填入实际回复。

**教学副产物**:这一坑直接印证文档「三个为什么」——agent 工具的报错常是「半截」,需自己造诊断(打印 key 长度/前缀、文件内容)才能定位深层原因。P3 加 bash 工具后,如果把这种错误原样回灌给模型,模型能否自纠正「key 不对」将成为 agent 自愈能力的首个考察点。


---

## 2. P0.5 完成(2026-08-06):多 provider 配置(TOML)

### 2.1 起因

P0 里 `BASE_URL`/`MODEL` 是硬编码常量,key 走环境变量 —— 这套只能跑 DeepSeek 一家。但「多个供应商」的本质是**同一份 OpenAI 兼容协议、不同的 base_url + key + 模型名**。该抽的不是「key 单独放配置」,而是「**一个 provider = {base_url, model, api_key_env} 整体放配置**」。在 P1 加工具前先扶正地基,后续 agent 在不同 provider 间切换是切配置不改代码。

### 2.2 设计决定

| 决定点 | 选择 | 理由 |
|---|---|---|
| 配置格式 | **TOML** | 人类好读好改;serde + toml 顺手;和 launcher config.rs 原子落盘同源 |
| key 在配置里怎么存 | **只存 env 名**(`api_key_env`) | 真 key 不入配置 → 配置可入库分享(推广);各自环境变量各自管。最安全 |
| P0.5 vs P1 顺序 | **P0.5 先做配置** | 地基扶正后 P1 只管 agent 逻辑,不管 provider 鉴权,职责清晰 |

### 2.3 文件与结构

- `Cargo.toml` — 加 `toml = "0.8"`(serde 解析)。
- `src/config.rs` — 新模块:
  - `Config { default, provider: HashMap<String, Provider> }`
  - `Provider { base_url, model, api_key_env }`
  - `Config::load(path)` 读 TOML;`default_provider()` 取 default 指向者;`Provider::api_key()` 从 env 名取真 key;`Provider::chat_url()` 拼 `base_url + /chat/completions`(裁末尾斜杠)。
- `src/main.rs` — 改造:`mod config;` 引入;去掉 `BASE_URL`/`MODEL` 常量;`run()` 先 `Config::load("codeagent.toml")` → `default_provider()` → `api_key()`;`chat_completion()` 改接 `&Provider` 参数,用 `provider.chat_url()` 和 `provider.model`。
- `codeagent.toml.example` — 入库范例,含 DeepSeek/OpenAI/ollama/NVIDIA 代理四段(后三段注释),说明换 provider = 改 default + 加段。
- `codeagent.toml` — 本机真实(deepseek 段启用),已 gitignore,不入库。
- `.gitignore` — 加 `codeagent.toml` 与 `code-agent-apikey.txt` 双保险。

### 2.4 三个设计意图(为 P1+ 铺路)

1. **key 失败点集中**:`Provider::api_key()` 是唯一从 env 读 key 的地方,P1+ 给 agent 回灌错误时,「key 缺」这一类失败只从这一处冒,错误消息带 provider base_url,模型/用户一看就懂。
2. **`chat_completion()` 接 `&Provider` 不接散参**:P1 加 tool use 时,请求体里要塞 `tools` 字段,此时 `Provider` 顺带可以挂「该 provider 支不支持 tool use」之类元数据,agent loop 据此决定要不要发 tools。现在用 `&Provider` 是为那时留口子。
3. **配置查找当前只认当前目录的 `codeagent.toml`**:后续可扩成 `~/.codeagent/config.toml` 之类分布式查找,但起步阶段单文件最简,不引入路径策略复杂度。

### 2.5 验证结论

- **编译门禁 ✅**:`cargo check` 全绿(中途踩了一个 Rust 字符串语法坑:`format!` 里未转义的 `"` 嵌套引号报 `expected `,`, found `{``,改用 `\"` 转义修复;记下来——这是 Rust 新手在中文注释夹带引号场景的常见坑)。
- **联通验证 ✅**:`cargo run` 实跑成功 —— 编译 toml 依赖链(8s)后程序启动,从 `codeagent.toml` 读到 deepseek provider,`chat_url()` 拼出 `https://api.deepseek.com/chat/completions`,模型正常回复。配置驱动跑通,「换 provider = 改配置不改代码」成立。
- **顺带坑(PowerShell 管道喂 stdin)**:`"内容" | cargo run` 在 PowerShell 下模型回复「内容不完整」——PowerShell 管道把字符串当对象传,非纯字节流,cargo 子进程的 stdin 拿到的不是预期输入。根因是 PowerShell 原生管道 + 外部进程的已知特性。**正确验法是直接 `cargo run` 后手敲输入**。P5 上 readline REPL 后此坑自然消失。

### 2.6 加 NVIDIA provider(直连官方 NIM,2026-08-06)

**起因**:加第二个 provider 实证「换供应商 = 改配置不改代码」。

**协议卡点(关键决策)**:你 launcher 的 NVIDIA 代理(`127.0.0.1:8082`)暴露的是 `/v1/messages`(**Anthropic 形态**),而 codeagent 打的是 `/chat/completions`(**OpenAI 形态**),协议对不上。三方案:

| 方案 | 做法 | 评估 |
|---|---|---|
| A 直连 NVIDIA 官方 NIM | `base_url = https://integrate.api.nvidia.com/v1`(NVIDIA 官方,原生 OpenAI 兼容),用 `nvapi-` 开头的官方 key | 零代码改,纯配置 —— 最符合 P0.5 精神。**采用** |
| B 给 8082 代理加 OpenAI 端点 | 改 `rust/src-tauri/nvidia/server.rs`,在 `/v1/messages` 外加 `/openai/v1/chat/completions` 路由 | 中等代码量;但与「代理先不动」冲突 |
| C codeagent 加 Anthropic 协议 | codeagent 侧多套 `/v1/messages` 路径,provider 配 `protocol=anthropic` 走它 | 大:同时维护两套协议,过早 |

**决定**:A。不动 launcher 代理,纯配置加 NVIDIA 官方 NIM 段。代价是要一把 NVIDIA 官方 key(环境变量 `NVIDIA_NIM_API_KEY`)和**校准模型名**——NIM 模型名按账号/版本变,我标成 `meta/llama-3.3-70b-instruct` 占位,404 model not found 时回 build.nvidia.com 复制准确名替换。

**落地**:
- `codeagent.toml.example` — NVIDIA 段从「走代理」改为「直连官方 NIM」,附三步启用说明和模型名校准提醒。
- `codeagent.toml`(本机) — NVIDIA 段以**注释形式**加进去,`default` 仍为 `deepseek`(未拿到 NIM key 前不切走,免得跑不通)。拿到 key 后取消注释 + 改 default 即可,代码零改动。
- `cargo check` 仍全绿(注释段不影响 TOML 解析)。

**验证 ✅(2026-08-06)**:用户指定模型 `nvidia/nemotron-3-ultra-550b-a55b`,设 `NVIDIA_NIM_API_KEY` 环境变量后,仅改 `codeagent.toml`(`default = "nvidia"` + 取消 NVIDIA 段注释),`cargo run` 直接拿到 NVIDIA 模型回复 `我是由 NVIDIA 的研究人员创建的。`。**代码、Cargo.toml、main.rs、config.rs 全未动** —— 「换 provider = 改配置不改代码」彻底实证。两家 provider(deepseek / nvidia)在骨架里已是平权可配置公民,再接几家只是 TOML 加段的同款操作。

**顺带记一笔(TOML 段头前导空格)**:本机 `codeagent.toml` 里 `[provider.nvidia]` 行有个前导空格(` [provider.nvidia]`),`toml` crate 容错了没报错才跑通——属隐患,严格 TOML 段头应顶格。跑通不回退,但后续严格化时统一格式。

---

## 3. P1 起步:tool use 探针实测两家协议(2026-08-06)

### 3.1 做了什么

进 P1(加第一个工具 `read_file`、跑通一轮 tool loop)前,按概念文档 §7 / §7.5 的纪律——**不空猜协议,先发探针实测**。做法:给 `ChatRequest` 加 `tools` 可选字段(`#[serde(skip_serializing_if = "Option::is_none")]`,无工具时序列化结果与 P0 一字节不差,不破坏向后兼容),写 `probe_tool()` 发一个带 `read_file` 工具定义的请求、把**原始响应 JSON** 美化打印出来不解析;`main()` 加 `probe` 分支(`cargo run -- probe`)。`cargo check` 全绿。

探针的请求体:system 教模型「需要看文件就调 read_file」,user 问「这个项目用了哪些第三方 crate?请实际查看 Cargo.toml 后再回答」——刻意诱导模型用工具。

### 3.2 两家实测原始响应对照(关键)

| 维度 | DeepSeek (v4-flash) | NVIDIA NIM (nemotron-3-ultra) | 解析能否共用一套 |
|---|---|---|---|
| `finish_reason` | `"tool_calls"` | `"tool_calls"` | ✅ 一致 |
| `tool_calls[].function.name` | `"read_file"` | `"read_file"` | ✅ 一致 |
| `arguments` | `"{\"path\": \"Cargo.toml\"}"` 字符串JSON | `"{\"path\":\"Cargo.toml\"}"` 字符串JSON | ✅ 都是字符串,都要 `serde_json::from_str` 再解一层 |
| `tool_calls[].id` | `call_00_XCU6LtAZiIeAthQ1f8cE4620` | `call-0e0f0ce1-88e5-4014-9987-72b8b0b5a250` | ✅ 格式不同但都当不透明字符串,只用来配 role:tool 回灌 |
| `tool_calls[].index` | 有(`"index": 0`) | **没有** | ⚠️ 不能靠 index 排序,改用数组顺序 + id 配对 |
| `content` | `""`(空串) | `null` | ⚠️ 两家对「调工具时 content」取值不同,解析必须 `Option<String>` 兼容两者 |
| `reasoning_content`(非标准) | 有,英文思考 | 有,中文思考 | ⚠️ 都不在标准协议里,解析层别建模它(别家可能没有);只在给用户打印时顺手挖出 |
| 是否主动调工具 | ✅ 会,无需 `tool_choice=required` 逼 | ✅ 会 | ✅ 两家都自然 tool call |
| 厂家自留噪声字段 | `prompt_cache_hit_tokens` / `..._miss_tokens` | `nvext`(吞吐/调度快照)、`service_tier`、`system_fingerprint: null` | serde 默认忽略未知字段即可 |

### 3.3 对 P1-3 / P1-4 的三个实质结论(写解析的依据)

1. **`index` 字段不能依赖**:NVIDIA 的 tool_calls 根本没 `index`。多工具时一律按**数组顺序 + id 唯一**配对 role:tool 回灌,不用 index 排序。
2. **`content` 必须 `Option<String>` 且容忍 null 和空串**:DeepSeek 给 `""`、NVIDIA 给 `null`,解析结构定为 `Option<String>`,两种都容。
3. **`reasoning_content` 不进 messages**:`finish_reason=tool_calls` 时 content 是空,但 `reasoning_content` 有「思考」——这是给人看的,不是对话内容,**绝不**回灌进 messages 历史里(否则模型会把「自己的思考」当对话材料反复卷)。P1-3 只在打印时可选展示,解析结构不主建模它。

### 3.4 一个更深的教训(印证概念文档)

概念文档 §7 / §7.5 讲「各家 OpenAI 兼容实现有微妙差异,别空猜、实测最快」。这次实测第一次给上述说法**端上实锤**:同样是「OpenAI 兼容」,两家在 `index`(有/无)、`content`(空串/null)、`reasoning_content`(英文/中文)三处都有差异。如果照一家写死,在另一家跑就翻。**探针先行的纪律立竿见影**,否则 P1-3 解析在 NVIDIA 上首跑大概率炸。

### 3.5 一个反直觉发现:`finish_reason` 让循环判定更稳

概念文档 §4 说 agent loop 结束条件是「模型不再用工具」。实测揭示 OpenAI 兼容协议给了**更直接的信号**:`finish_reason` 字段。`"tool_calls"` = 这轮要调工具(该执行+回灌+再循环),`"stop"` = 正常结束(打印纯文字收工)。P1-4 的 loop 判定可两路看:`finish_reason == "tool_calls"` → 执行工具;`"stop"` → 结束。比光看「有没有 tool_calls 数组」更稳——比容错两家 content 举棋不定更干净。

### 3.6 P1-3 落地:Tool trait + ReadFile + 协议解析结构(2026-08-06)

按 §3.3 三结论把解析写实。拆出 `src/tools.rs` 专放「工具侧」,与 `main.rs` 的「对话/循环侧」分文件——加工具不动 main,改循环不动 tools,文件即职责边界。

**`tools.rs` 三块:**

1. **`Tool` trait(窄接口)**:
   ```rust
   pub trait Tool {
       fn name(&self) -> &str;                      // 对模型可见
       fn description(&self) -> &str;               // 给模型看的 prompt(§3.1 工程要点)
       fn parameters(&self) -> serde_json::Value;   // OpenAI function 的 JSON Schema
       fn schema(&self) -> serde_json::Value {       // 包一层 {type:function, function:{...}}
           serde_json::json!({ "type":"function", "function":{ /* name/desc/params */ } })
       }
       fn execute(&self, arguments: &str) -> anyhow::Result<String>;  // args 是 string JSON
   }
   ```
   设计刻意窄(见 concepts §7.5):**加工具 = impl Tool**,与 P0.5「加 provider = TOML 加段」同构 —— 都是一处 trait + 一份实现,扩展点单一。

2. **`ReadFile` 第一个工具**:name=`read_file`,parameters 是单字段 `{path}` object。`execute` 先把 string 形 arguments `serde_json::from_str` 解一层拿 `path`,再 `std::fs::read_to_string`。**错误回灌按 §5「丙(清楚)」写**:不只回栈,而给模型「路径不存在或不可读…建议先用绝对路径或在调用前列目录确认」这种可操作暗示,让模型自纠正 —— 错误信息是给模型看的 prompt,不是给人看的栈。

3. **协议解析结构(严格按 §3.3 三结论)**:
   - `ToolCall` / `ToolCallFunction`:**不建模 `index`**(§3.3 #1),靠数组顺序 + `id`(配 role:tool 回灌)。`arguments` 定为 `String` 不是 object(§3.2 实测:OpenAI 协议 arguments 一直是字符串 JSON)。
   - `FinishReason` enum(`rename_all="snake_case"`):`ToolCalls` / `Stop` / `#[serde(other)] Other` 兜底未知值 —— §3.5 的循环判定主信号。
   - `AssistantReply`:`content: Option<String>` + `#[serde(default)]`(§3.3 #2 容 `""` 与 `null`);`tool_calls: Option<Vec<ToolCall>>`;`reasoning_content` 建了但 `#[allow(dead_code)]` + 不进 messages(§3.3 #3)。`wants_tool(&finish)` = 「finish 是 ToolCalls **或** tool_calls 非空」两路容错。
   - `ToolResultMessage`:`{role:"tool", tool_call_id, content}` —— role:tool 回灌时 `tool_call_id` 必须配上 `ToolCall.id`(§3.2 配对要求)。

**踩一个编译坑(记录给后来人)**:`Message`(main.rs)有 `tool_calls: Option<Vec<ToolCall>>` 字段且 `#[serde(skip_serializing_if=...)]` → 要能把含 tool_calls 的 assistant 消息**序列化**回灌进请求体,故 `ToolCall` / `ToolCallFunction` 必须 `derive(Serialize)`。初版只 derive 了 `Deserialize`(从响应解析用),`cargo check` 报 `the trait bound Vec<ToolCall>: Serialize is not satisfied`。补 `Serialize` 即解。教训:解析结构常「既要从响应反序列化、又要作为请求体序列化」,两 derive 一起给最省心。

**门禁 ✅**:`cargo fmt --check` + `cargo clippy --all-targets -D warnings`(零警告)+ `cargo check --tests` 全绿(P1-3 编译期完成)。

### 3.7 P1-4 落地:main 里跑通一轮 tool loop(2026-08-06,代码就位·待本机实测)

把 `main.rs` 从 P0「调一次打印一次」升级为**真正的 agent loop**。核心新增四块:

1. **`Message` 扩成能承三轮色**:加 `tool_calls: Option<Vec<ToolCall>>`(assistant 回灌时带)、`tool_call_id: Option<String>`(role:tool 时带),都 `skip_serializing_if="Option::is_none"` + `#[serde(default)]` —— 无关角色不序列化这些字段,请求体保持每种角色只发该发的字节。`Message::system` / `Message::user` 构造器把新字段填 None。

2. **`chat_completion(...)` 返回 `(AssistantReply, FinishReason)`**:不止取 `message`,把 `finish_reason` 一并带回—— §3.5 的循环判定要它。

3. **`assistant_message_from_reply(reply)`**:把模型本轮含 `tool_calls` 的回复**先压进历史**。关键:这条 assistant 消息必须进历史,否则模型下一轮看不到「我刚才调过啥」会原地打转(concepts §4 要点 1)。

4. **`run()` 主循环**:
   ```
   读配置/provider/key → system+user 起历史 → 工具集=[ReadFile.schema()]
   for round in 1..=MAX_TOOL_ROUNDS(=8 兜底硬上限):
       chat_completion(...) 拿 (reply, finish)
       if !reply.wants_tool(&finish): print_reply → 收工
       else: 把含 tool_calls 的 assistant 消息压历史
             逐个 call: dispatch_tool → role:tool 消息(带 tool_call_id)压历史
             回到 loop 顶
   触上限:打印明确告知,不静默退出
   ```
   `dispatch_tool(call)` 当前固定路由 `read_file`,P3 扩工具集时这里变「按 name 分发」。工具失败也回灌(`format!("[工具执行失败] {}", e)`)——让模型看到错误有机会自纠正,而不是程序崩溃把整轮对话作废。`print_reply` 顺手把 `reasoning_content`(若有)在正文前打 `(思考: …)`,但 reasoning 绝不进 messages(§3.3 #3)。

**门禁 ✅**:`cargo fmt --check` + `cargo clippy --all-targets -D warnings` + `cargo check --tests` 全绿。**代码面 P1-4 完成**。

**P1-4 的「实测」留给你本机**(自律:文档只记已发生的,不伪造运行输出):跑 `cargo run`,问一句诱导模型看文件的(如「`codeagent.toml` 里 default 是哪个 provider?请实际查看后再答」),观察是否出现真正的 agent 行为——模型主动 tool call → 程序读文件回灌 → 模型据文件内容给答案。实测后把真实输出去敏回贴,我补进本节作「实测印记」,与 §3.2 探针印记对齐。

### 3.8 P1-4 实测印记:真正的 agent 闭环打通(2026-08-07)

本机 `cargo run`,问「`codeagent.toml` 里 default 是哪个 provider?请实际查看后再回答。」——**真 agent 行为出现**,完整闭环:

```
> codeagent.toml 里 default 是哪个 provider?请实际查看后再回答。

(思考: The file clearly shows the default provider is "nvidia" as specified in the first line: `default = "nvidia"`)

`default = "nvidia"`
```

逐条印证设计:

| 现象 | 印证哪条设计 |
|---|---|
| 模型**主动** tool call,未用 `tool_choice=required` 逼 | §3.2 两家都「自然 tool call」实测;概念 §4「模型自己决定调工具」 |
| `(思考: …)` 被打印给人看 | §3.3 #3 / §3.6 `print_reply` 顺手挖 `reasoning_content` |
| 答案是文件里的真实内容 `` `default = "nvidia"` `` | role:tool 回灌成功 + `tool_call_id` 配对成功(concepts §3.2);模型确实看到了文件内容再答,不是瞎编 |
| 只调了一轮工具就给答案收工 | §3.5 `finish_reason=stop` 判定生效,loop 干净退出 |

**顺带观察(P2 动因)**:你输出的末尾有个孤立的 `y`——那不是模型答的,是模型收工后 PowerShell 又吃了一行用户输入。现在 `run()` **读一行就退出程序**,没有「连续对话」概念,那个 `y` 被悬空了。这正是 P2 要解决的:把循环从「读一行跑一轮」改成「读完 → 跑 loop → 答完 → **再读下一行**」的 REPL,悬空的 `y` 自然就被当成下一句输入处理。

> 至此 **P1 整章收口**:探针(§3.1-3.2)→ 解析结论(§3.3)→ 循环信号(§3.5)→ 工具侧落地(§3.6)→ 循环侧落地(§3.7)→ 实测印记(§3.8)。code agent 三层里的 L1「Agent loop」**跑通且实测**,L2「Tools」开了第一个口子(ReadFile)。下一站 P2 把 `run()` 的「读一行跑一轮」扩成**连续多轮 REPL**,真正的 agentic loop 才算活。

---

## 4. P2 落地:连续多轮 REPL —— 真正的 agentic loop(2026-08-07)

### 4.1 动因

P1-4 实测里那个孤立的 `y` 是直接动因:当时 `run()` **读一行跑一轮就退出程序**,PowerShell 又吃了一行用户输入,那个 `y` 悬空。P1 虽把「单问单答 + 工具闭环」跑通了,但它**还不是 REPL**:你没法追问、没法带上下文、没法像用真 agent 那样连贯工作。P2 要做的就是把这个外层循环补上 —— 让 agent 从「一次性命令」变成「常驻陪聊的同事」。

### 4.2 拆 `run_one_turn` vs 外层 REPL

把 P1 全塞在一个 `run()` 里的逻辑**按职责拆两层**:

- **内层 `run_one_turn(client, provider, api_key, &mut messages, tools_slice)`**:"把一个问题问到答完"。进入时 `messages` 已含本轮 user 输入(和之前全部历史),跑 agent loop 到模型给纯文字答案,把这条 assistant 终答**也压回 `messages`** 后返回。**不读输入、不打印提示符** —— 它只管「一轮问答的内部循环」。
- **外层 `run()` REPL**:"把一行行输入喂进内层"。`messages` 提到外层、**跨整轮对话共享**(system 在前,user/assistant/tool 顺序追加)。空行跳过(不浪费一次模型调用;P1 那版「空就退出」在 REPL 语义下不对了)、`/quit` / `exit` 退出、EOF(PowerShell Ctrl-Z 回车 / Unix Ctrl-D,靠 `read_line` 返回 0 字节判定)退出。

这一个拆分的真正价值:**循环信号分层**。内层那层是「模型决定调不调工具」(ReAct),外层这层是「人决定下一条问啥」。两层不混淆,混在一起(P1 那版)就是「问一次程序就死」。

### 4.3 历史跨轮保留 —— P2 真正的「活」点

`messages` 提到外层、不重置 = **对话历史跨多个用户输入保留**。这带来 P2 的实质能力:

- 你能问「`codeagent.toml` 里的 default 是哪个 provider?」(模型调工具读了文件)→ 答完 →
- 接着追问「那里面配置的 base_url 是哪个?」—— 模型**不必再调工具**,因为上一轮它已经读过文件、内容在历史里,直接从上下文答。
- 或者「把刚才读到的 `[provider.nvidia]` 段念给我」—— 依赖上文。

P1 里 `messages` 每次新建,第二句就把第一句忘了(其实 P1 根本跑不到第二句)。P2 这才算把 **L1 Agent loop 从「单次交易」做成「有记忆的会话」**。当然有代价:历史会一直长,P6(上下文管理)就是来解决「长生不老的历史终会爆 context」的;P2 先让它活起来,记多久 P6 再裁。

### 4.4 退出语义的取舍

三种退出都留:

| 退出方式 | 语义 | 为啥留 |
|---|---|---|
| `Ctrl-Z`/`Ctrl-D`(EOF) | `read_line` 返回 0 字节 | 习惯 Unix/Terminal 的人本能动作;PowerShell 的 `Ctrl-Z 回车` 也走这条 |
| `/quit` 或 `exit` 文本 | 显式退出 | PowerShell 下 Ctrl-Z 体验差(要先回车),给个文本兜底 |
| 空行 `continue` | **不退**,跳过这一轮 | P1 那版「空就退」在 REPL 语义下是 bug:手滑回车不该整个程序死掉 |

这份取舍里有个小决心:**P1 的「空输入就 return Ok」被改成了「空输入 continue」**。因为语义变了:P1 是「命令行工具跑一次」,空输入 = 没事干 = 退;P2 是「常驻 REPL」,空输入 = 这一轮没问 = 等下一句 = 接着转。同一份 `read_line` 判空,在两种顶层语义下该有不同动作,这是这次重构里最容易想错的一处。

### 4.5 门禁 ✅ + 留给后续的窗口

`cargo fmt --check` + `cargo clippy --all-targets -D warnings`(零警告)+ `cargo check --tests` 全绿。P2 代码面完成。

代码里留了两个**显式后向指路注释**,为后续阶段铺路:

- `run()` REPL 注释:`P5 上 readline(lineppy/rustyline)后提示符/历史/编辑会变好,现在用裸 stdin` —— 现在的 `> ` 提示符 + 裸 `read_line` 没有上下方向键历史、没有行编辑,PowerShell 卡能用但糙。P5 流式那段时顺手换 rustyline。
- `run_one_turn` 触上限注释:`本回合作废但 REPL 继续` —— P1 那版触硬上限直接退程序,P2 改成「这一轮答不上来,但 REPL 接着转,你换句话问」。这俩「接着转」的语义对齐了。

### 4.6 P2 实测印记(待本机)

代码面 P2 完成,但「连续多轮 + 跨轮上下文」这个**质变**要本机跑一轮多问才显形。建议你这样测:

```
cargo run
> codeagent.toml 里的 default 是哪个 provider?先实际查看文件再回答。
（模型答 nvidia —— 这轮调了 read_file）
> 刚才那个文件里配置了哪几个 provider?
（模型应能从上一轮读到的文件内容直接答,不必再调工具 —— 验证历史跨轮保留）
> /quit
```

第二条是不是「不调工具、直接答」就是 P2 的成败判据:它印证 `messages` 跨轮保留成立。跑完把真实输出去敏贴回来,我补本节作「实测印记」,把 P2 关上。

### 4.7 P2 实测印记:REPL + 跨轮记忆打通(2026-08-07)

三连问,caret 旁是观察点:

```
> codeagent.toml 里的 default 是哪个 provider?先实际查看文件再回答
`default = "deepseek"`,所以默认 provider 是 deepseek… 文件中还定义了一个备用 provider nvidia,但默认未启用。
> 刚才那个文件里配置了哪几个提供商?
(思考: …我已经读过了,直接回答。)
根据刚才读到的 codeagent.toml,一共配置了 2 个提供商:1. deepseek(当前默认) 2. nvidia(目前处于注释状态)…
> 这轮对话 你调用了几次工具
这取决于你指的范围:
- 当前这一轮:我还没有调用任何工具。
- 整个对话到目前为止:一共调用了 1 次工具,即第一次回答时读取 codeagent.toml 的 read_file;第二次回答直接基于已有信息,没有调用工具。
```

逐条印证 P2 的三个质变:

| 观察 | 印证的 P2 设计 |
|---|---|
| 第一问调 `read_file` 答出 deepseek | §4.3 第一条 user+tool+assistant 进历史 |
| 第二问「**我已经读过了，直接回答**」(思考),**不调工具**就答出两家 | §4.3 `messages` 跨轮保留 —— **P2 成败判据通了**:模型能复用上一轮读到的文件,而非重读 |
| 第三问模型回顾三轮调用史,给出「第一轮 1 次、第二轮 0 次、本轮 0 次」精确区分 | 历史不只是「记得上文」,模型还能**元认知**自己的工具调用史 |

**第三问是个意外彩蛋**:它证明 agent 不只「有上文记忆」,还能**对自己过去的动作做审计**(「我刚才调过几次工具」)。这是 Claude Code 那种 agent 才有的特质 —— 在「反复回灌」的机制里,模型自然长出对自身操作的自我认知。一个最小 agent 内核(到此 ~230 行)能涌现这种行为,印证 concepts §4 那句「agent loop 的本质不在某一轮,而在轮与轮的累积」。P2 把这层质变端出来了。

> **P2 整章收口**。L1 Agent loop 从「单次交易」做成「有记忆的会话」且实测打通;代价(历史只会更长)留给 P6 上下文管理裁。下一站 **P3 扩工具集** —— 让 agent 真正能动手(写文件 / 跑命令 / 搜索),从「只能看」变「能改」。

---

## 5. P3 落地:扩工具集 + path 钳 + 审批闸(2026-08-07,代码就位·待本机实测)

### 5.1 加了什么(4 个工具)

P1 只一个 `ReadFile`,P2 让循环活起来,P3 让 agent **真正能动手**:从「只能看」变「能写、能跑、能搜」。在 `tools.rs` 加 4 个工具 + 一个跨工具共用的 path 安全钳:

| 工具 | 作用 | 危险度 | 备注 |
|---|---|---|---|
| `write_file` | 写文件(可覆盖),父目录不存在自动建 | destructive | 含完整 content 参数;返回落点绝对路径 |
| `list_dir` | 列目录条目(文件名 + 是不是目录) | 只读 | path 可选 —— 补 ReadFile 的「读之前先看这有什么」 |
| `glob` | 按 glob 规则搜文件名(`**/*.rs`) | 只读 | 自己实现的极简 glob,**不引第三方 crate**;支持 `*`/`?`/`**`,不支持 `[...]` |
| `bash` | 跑命令(Windows `cmd /C`、Unix `sh -c`) | destructive | stdout+stderr 合流;输出超 5000 字符截断头4000/尾1000 |

### 5.2 path 安全钳 `resolve_under_cwd`(防 `../` 越狱)

P3 起模型能写文件能跑命令了,**第一条要守的线就是路径不能跑出工作目录**。`resolve_under_cwd(raw)` 把模型给的路径归一到「cwd subtree 内」的规范绝对路径:

1. 绝对路径直用、相对 join cwd
2. 手做 normalize(遍历 `Components`,`.` 跳过、`..` pop 一层)—— **不**用 `canonicalize`(它要求路径已存在,容不了「写不存在的新文件」)
3. 钳:`normalized.starts_with(cwd)` 不成立就 `Err` —— 防 `../../../etc/passwd` 越狱。错误串回灌给模型,让它换路径

所有文件类工具(ReadFile 起也统走它了)都过这道钳。**读要防,写更要防** —— 写的外不防,模型一句 `write_file path="../../.ssh/authorized_keys"` 就出大事。

> 刻意「自己实现 normalize」而非引第三方:就这一处钳,逻辑不到 30 行,引个 crate 反而把安全审计面拉宽。概念文档 §7.5「窄接口」在这里同样适用 —— **核心路径上的安全逻辑,自己写、自己读、自己审**。

### 5.3 dispatch 从「固定路由」改成「按 name 分发」

P1 时 `dispatch_tool` 是 `if name == "read_file" { ReadFile.execute() }` 硬编码。P3 加 4 个工具就改不成了。改成**工具表 + 按 name 查**:

```rust
let tools: Vec<Box<dyn Tool>> = vec![
    Box::new(ReadFile), Box::new(ListDir), Box::new(Glob),
    Box::new(WriteFile), Box::new(Bash),
];
// schema 从这表派生 —— 避免两处维护(schema 列表 + 工具列表)对不上。
let tools_schemas = tools.iter().map(|t| t.schema()).collect();
```

`dispatch_tool` 在表里 `find(|t| t.name() == call.function.name)`,没找到就回灌「可用工具: …」给模型。**加工具从改两处(if 分支 + schema 列表)降到改一处(vec 里 push 一个)** —— 与 P0.5「加 provider = TOML 加段」、P1-3「加工具 = impl Tool」三个扩展点彻底同构。

### 5.4 审批闸 `ApprovalGate`(P4 雏形)

`bash` / `write_file` 这种 destructive 工具,没闸就跑模型给的任意命令 —— 危险度等于把 shell 让给 LLM。P3 起加一道**最简 y/n 闸**(P4 会做成可配置白名单):

- `Tool::is_destructive()` 默认 `false`;`write_file` / `bash` 标 `true`。**闸在 `dispatch` 处统一拦**,不塞进 `Tool::execute` —— `execute` 在 trait 形态里是纯函数约定,没 stdin 通道;闸要 stdin,放 dispatch 那层(那里有 stdin)正合适。trait 形态最小改动。
- 命中 destructive → 打印 `[审批] 即将执行 write_file({...}) —— 放行? [y/N]` + 等 stdin。**非 y 一律拒绝** —— 默认安全。
- 拒绝**也回灌**:「[用户拒绝执行] … 被用户否决,请换一种不修改磁盘的方式继续」—— 让模型换条路,而非卡死。
- `--yolo` 开关跳过闸(致敬 launcher 的 yolo 概念):`cargo run -- --yolo`。实测想一连跑多步不被卡时开,平时关。

> 这个闸是 P4「权限审批层」的雏形 —— P4 会把「哪些工具要审、哪些命令前缀放行」做成可配置,不再硬编码 y/n。P3 先把闸位留好,逻辑在那里,P4 直接换实现。

### 5.5 踩的两个坑(给后来人)

1. **glob 自己写踩的 borrow 坑**:`entry.file_name().to_string_lossy()` 返回 `Cow` 临时值,`let name = …; match(seg, &name)` 时临时值在语句尾释放、`&name` 跨语句悬空 → `E0716 temporary value dropped while borrowed`。修:`.into_owned()` 持 String 所有权。教训:`to_string_lossy` 的 Cow 别想当然当长命值用。

2. **glob 第一版手抖打了个乱码注释**(`// 用 p؈ 表 …`),连 Edit 的 old_string 都匹配不上(乱码字符不可逆)。最终用 `awk NR<=282 || NR>=368` 按行号整段删重建。教训:**乱码注释不止难看,会把后续的机械化重构(字符串匹配)也卡住**;非 ASCII 字符在代码注释里要克制,尤其含组合字符的 emoji。

### 5.6 门禁 ✅

`cargo fmt --check` + `cargo clippy --all-targets -D warnings`(零警告)+ `cargo check --tests` 全绿。P3 代码面完成。system 提示词同步更新为「可用工具:read_file/list_dir/glob/write_file(会问人)/bash(会问人)…」,让模型知道节奏。

### 5.7 P3 实测印记(待本机)

代码面 P3 完成,实测留给你本机。建议这样验:开 `--yolo` 顺跑多步(看 agent 真能写文件),再关 `--yolo` 看 destructive 闸:

```
cargo run -- --yolo
> 在当前目录建个 hello.txt,内容写「codeagent P3 生效」
（应看到 write_file 被调、回灌「已写入 …」）
> 列一下当前目录,确认 hello.txt 在
（应看到 list_dir 调 + 条目里有 hello.txt）
> 用 bash 跑 git status,看当前有啥改动
（应看到 bash 调 + git status 输出回灌）
> /quit
```

再不开 yolo 重跑一次首条,应看到 `[审批] 即将执行 write_file(…) —— 放行? [y/N]`,回车(非 y)应被拒、模型回灌换条路。把去敏真实输出贴回,我补本节记印记,把 P3 关上。

> **P3 阶段意义**:L2 Tools 层从「1 个」变「5 个」,agent 第一次能改磁盘。代价是**安全面打开了**,所以 P3 同步把第一道闸(path 钳 + y/n 闸)立起来了 —— 这正是 P4 要做完整权限审批的直接前驱。

### 5.8 P3 实测印记:工具五件套 + 审批闸两度生效(2026-08-07)

不开 `--yolo` 跑实测(故意测审批闸真生效的本机),四连问:

```
> 在当前目录建个 hello.txt,内容写「codeagent P3 生效」
[审批] 即将执行 write_file({"path": "hello.txt", "content": "codeagent P3 生效"}) —— 放行? [y/N]
> y
已创建 hello.txt,内容为 codeagent P3 生效,文件位于当前目录。

> 列一下当前目录,确认 hello.txt 在
已确认,当前目录下存在 hello.txt,已创建成功。

> 用工具检查
已通过 read_file 工具检查,hello.txt 内容为 codeagent P3 生效,与写入时一致,确认无误。

> 用 bash 跑 git status,看当前有啥改动
[审批] 即将执行 bash({"command": "git status"}) —— 放行? [y/N]
> y
git status 结果如下:当前分支 feat/rust-implementation…未跟踪文件 ./、../config.json、../logs/、../push-rust-to-github.ps1…
```

逐条印证 P3 设计:

| 观察 | 印证哪条 P3 设计 |
|---|---|
| `write_file` 调用前先 `[审批]` 张 + 答 y 才写 | §5.4 审批闸对 destructive 工具生效;闸在 dispatch 处拦、没塞进 execute |
| `list_dir` 调了 + 答出 hello.txt 在 | §5.1 list_dir 工具可用,补 ReadFile 的「读之前先看」 |
| 第三句「用工具检查」→ 模型**自选 `read_file`** 验内容一致 | dispatch 按 name 分发已在多工具间正确路由;模型能在 5 工具里自主挑对的那个 |
| `bash git status` 也各自 `[审批]` + y 才跑 | 闸对**每一种** destructive 工具都拦(write_file + bash 都拦),不是只拦某一个 |
| 写成功后路径钳「codeagent-rs 目录下」 | §5.2 `resolve_under_cwd` 把 hello.txt 钳在 cwd subtree 内,没越界 |
| git status 答完定格在 `> ` | §4 REPL 正常接着转,没因 destructive 工具而崩或卡 |

**git status 里的一个观察(留记)**:模型注意到当前目录在 git 里显示为 `./`(整个 codeagent-rs 未被跟踪)。印证 `codeagent-rs` 是仓库根 `claude-launcher` 下的**独立子项目**,与 `rust/` launcher 互不依赖(本仓库根 CLAUDE.md 的设计)—— git 在仓库根视角看 codeagent-rs 整个是新东西。这与 §0「开 codeagent-rs/ 子项目、与 launcher 完全独立」一致;也提示后续 git commit 时 codeagent-rs 的内容会作为新增进仓库历史的部分。

> **P3 整章收口**。agent 真正能动手(写+跑+搜),有安全性闸(path 钳 + y/n),实测通过。P3 这道 y/n 闸是「每次都问」的硬闸 —— 实测体验也对:你逐条 y/n 其实是被模型「报信」逐项过审,透明但稍累。**下一站 P4**:把这道闸做成**可配置白名单**(读类工具全自动放行、白名单命令前缀跑类自动放行、其余问),既保留安全又减摩擦 —— 这正是 P3 闸手上攒出来的实战需求。

---

## 6. P4 落地:可配置白名单审批层(2026-08-07,代码就位·待本机实测)

### 6.1 动因:从「逐条问」到「放心自动、可疑才问」

P3 的 `[审批] … 放行? [y/N]` 闸是「每次都问」的硬闸。 §5.8 实测里,你建个 `hello.txt` 要 y、跑个 `git status` 也要 y —— 透明是真透明,但**摩擦过重**:`git status` / `ls` / `cargo build` 这种显然安全的命令,人每次手 y 是浪费。P4 把闸做成**可配置白名单**:

- 命中白名单的 destructive 命令 → **自动放行**,免 y/n
- 未命中 → 才走 y/n 问人
- 读类工具本不过闸(`is_destructive=false`,P3 已对)→ 不需要配白名单

哲学:**安全的交给规则、可疑的交给人**。规则越贴心,人工审越精纯、只盯真正要拍板的那些(写新文件、`rm`、`git push` 这种)。

### 6.2 落地:config 加 `[approval]` 段

顶层 `Config` 加可选 `[approval]` 段,缺省回退 P3 行为(全问),向后兼容老 `codeagent.toml`:

```toml
[approval]
bash_allow_prefix = [
    "git status", "git diff", "git log",   # 只读 git
    "ls", "pwd", "echo",
    "cargo ", "npm ", "node ",             # 构建链(注意尾空格)
]
```

`ApprovalConfig { bash_allow_prefix: Vec<String> }`,`#[serde(default)]` 空集 = 全问。

### 6.3 一个反直觉的「前缀」取舍

白名单用 **「前缀」而非「全等」** —— 因为 `cargo build`、`cargo run -- probe` 这种带参命令按全等配白名单要列无穷多条。用前缀,一条 `cargo ` 就把 `cargo build`、`cargo test`、…都覆盖。

但前缀有副作用:`cargo` 当前缀会把 `cargo-devil`(假如有的话)也放行。故约定 **「放心词带尾空格」**:`cargo `(尾带空格)更严 —— 它要求命令在 `cargo` 后立即是空格(正是带参命令的样子),不会误放 `cargo-devil`。这是前缀白名单的细节心法,**写进 toml.example 注释**(§6.2 那段尾空格注),让你回看时一眼记起。

### 6.4 ApprovalGate 携配置、判定四档

`ApprovalGate` 从 `{ yolo }` 扩成 `{ yolo, allow: ApprovalConfig }`,判定四档分明(见 main.rs doc):

1. 非 destructive 工具 → 不过闸直放行
2. `yolo=true` → 全放行(深度逃逸,最高优先级兜底)
3. destructive 但 bash 命令命中 `bash_allow_prefix` → 自动放行
4. 命不中 → y/n 问人,非 y 一律拒

bash 白名单命中要先从 arguments 解出 `command` 字段(`extract_bash_command`)。**解出失败 → 退回 y/n**,不因配置解析炸而误放行 —— 这条「拿不准就往严了走」是安全代码的肌肉记忆。

### 6.5 保守口径:写文件暂不做路径白名单

P4 暂**没**给 `write_file` 加路径白名单(让某些路径写也不审)。理由:**写比读更危险**,而前缀白名单对路径意义弱(文件名千变,列 prefix 不如列坏更精准)。保守口径是「所有 `write_file` 一律问人」,让 y/n 闸专门守「写」这件最该人拍板的事。未来若要减写摩擦,做的事应该是**可配置 yolo-by-tool**(而非路径前缀白名单),那是 P4.2 的桥。

### 6.6 门禁 ✅

`cargo fmt --check` + `cargo clippy --all-targets -D warnings`(零警告)+ `cargo check --tests` 全绿。中间 clippy 抓了一处 `doc_lazy_continuation`(doc-comment 第 4 条后那行没缩进,被当成 list 懒续)——缩进进 list 项即解。toml.example 同步加了被注释的 `[approval]` 范例段。

### 6.7 P4 实测印记(本机跑通 2026-08-07)

配 `bash_allow_prefix = ["git status", "ls"]`(其余破坏性命令仍走 y/n),实测见证三档闸分流 + 一项反直觉发现:

```
> 用 bash 跑 git status
（直接出结果:branch feat/rust-implementation、改动列表…  ← 命中白名单,免审直放行）

> 用 bash 跑 git log
[审批] 即将执行 bash({"command": "git log --oneline"}) —— 放行? [y/N]
> N                                                      ← 用户拒绝这一次
[审批] 即将执行 bash({"command": "git log"}) —— 放行? [y/N]
>                                                        ← 模型被拒后换写法再试
```

三件可读出来:

1. **白名单命中免审**:`git status` 前缀在白名单 → 不弹 `[审批]`、直接出 `exit=0\n…branch…` 输出,摩擦降到零。
2. **白名单未命中仍问**:`git log` 不在前缀表 → 弹 `[审批] … 放行? [y/N]`,挡住等你拍板。
3. **拒绝的真实形状**(反直觉、最值一记):**N 不是「堵死这条路」**。被拒的那条 `git log --oneline` 命令**绝对没跑**(闸 return false → `tool.execute` 那行根本到不了,磁盘零改动),但模型**没退出 REPL、也不死**——它把拒绝回灌 `[用户拒绝执行]…请换一种不修改磁盘的方式继续` 当成一种信号,**自己换写法**(去掉 `--oneline`)再调一次 bash,又触发一道闸。也就是说:**闸拦的是每一刀(单条命令),不拦整个意图**。被拒一刀后模型有权改路继续问闸,直到它觉着换不动了去改用 read_file / 或直接告诉你「需要放行才能跑」。

这条「**拒绝≠退出、闸拦刀不拦意图**」是白名单 + 手工确认式闸的固有语义,也恰恰是它的好处和它的局限同一处:它能保证**每一条破坏性命令都过人或过白名单**,但它**不替你决策「这一整段意图要不要」**。模型若执意变招试,会连弹几次闸——这时人在循环里逐条 N 即可,效果是「这意图不前进」而非「agent 崩」。实测中模型一次换写法、就停下改路,体验可接受;若将来出现连试不停,P4.x 可加「**同一意图被连拒 N 次自动停**」计数闸(暂缓,先看是否真需要)。

> 印记落定:P4 落地与实测两条都见绿,可关章。



> **P4 阶段意义**:agent 的「安全-摩擦」曲线头一次有了用户可调旋钮。读全免、命中白名单免、可疑才人审、--yolo 兜底 —— 四档构成实际可用的审批体验。写文件这条保守守牢、命令前缀放行减摩擦。这条闸后续 P4.x 可继续调(比如按工具配 yolo、或命令黑名单),但 P4 把可配置这件事竖起来了 —— 安全不再是写死的假设,是可分享到 toml 里、可随团队调整的策略。

---

## 7. P5 落地:流式输出 + Ctrl-C 中断(2026-08-07,代码就位·待本机实测)

### 7.1 动因:从「干等到全好」到「逐 token + 一拍即停」

P2-P4 的 REPL 想问个稍复杂的事,模型脑子里一片、磁盘上一动不动,屏幕僵在那儿几十秒才「啪」一下整条答案蹦出来——非流式的硬伤:**你不知道它还在转、转得成不成、想法走到哪了**。这是从「能用」到「顺手」最扎实的一跳:

- **流式输出(SSE)**:网络来一片 token 就 `print!` 一片+flush,跟 ChatGPT 那种逐字吐字的体验一样。长答案不再「等待→爆炸式蹦出」,而是边想边说,你提前能看出它想歪了。
- **中断(Ctrl-C)**:生成过程中随时 Ctrl-C 一拍即停,不必等模型把啰嗦完。中断后历史**不留半截**(下文 §7.4 讲为何不能留),回 REPL 顶等下一句。

(原计划的第三件 **rustyline REPL 行编辑/历史** 拆到 §7 后的 **P5.5** 单独做——它 native 依赖 + Windows 编译环境 + 接管 stdin 后 Ctrl-C 路径要单独验证,跟流式/中断这俩精髓不强耦合,卡进来反而让 P5 的本机实测重心模糊。先把「逐 token + 一拍即停」两件最值本机见的事做完收口,行编辑锦上添花紧接其后。)

### 7.2 落地前先核证协议(不凭印象赌)

OpenAI 兼容流式的 `tool_calls` 增量配对是这章最容易写炸的点——后续 chunk **不带 id,只带 function.arguments 字符片段**,凭印象写成「靠 id 配对」就崩。所以开工前用 subagent 核证了 OpenAI 官方 Python SDK + DeepSeek 文档 + LangChain 累积器实现,结论存档 `docs/P5-streaming-protocol-notes.md`。三条铁律沉淀进代码:

1. **tool_calls 配对靠 `index`,绝不靠 `id`** —— OpenAI 官方类型 `index: int` 必填即明证;后续 chunk 无 id,只 arguments 片段。
2. **`function.arguments` 是字符串拼接,非 JSON 合** —— 流完再 `serde_json::from_str` 一次。
3. **chunk 层面不做 content/tool_calls 互斥假设** —— 独立累积,末尾按 finish_reason 分类。

两家偏差也照核证处理:DeepSeek 多 `delta.reasoning_content`(分片,在 content 之前)+ `finish_reason:"insufficient_system_resource"`(复用现有 `#[serde(other)]` 兜底落 Other,不崩);NIM 听闻 `tool_calls[].index` 偶不回填——建成 `Option<i32>`,缺失按帧内出现序派生,不崩。`usage` 末帧可能不回填——tolerate 始终 null。

### 7.3 落地:SSE 累积器焊成非流式同形

`tools.rs` 加 `StreamAcc`——一个 SSE 增量累积器,把无数 delta 片段焊成非流式 `AssistantReply` 同形;`main.rs::chat_completion_stream` 跑字节流循环。**关键**:流式只是一层累积器在前面焊,后面的 `run_one_turn` 主循环结构、`AssistantReply`/`ToolCall`/`FinishReason` 三个结构**原样复用**——这是从 P1-3 就把「解析结构」设计得与流式/非流式无关的红利,P5 改动面被压到最小。

中止判据**两个并用**(协议笔记 §终止判据):显式 `[DONE]` 或 流自然 EOF(NIM/vLLM 偶尔不发 `[DONE]` 直接断)。两路在循环外**合流到统一 finalize**,不各自构造——避免 `[DONE]` 分支丢掉真实 finish_reason 的 bug(实施途中真踩过这个坑、专门修平)。错误帧(`{"error":{...}}`)parse 失败时再 try error,命中即整体标错,不当空 content 吞。

打印策略:**content 增量即时 `print!`+flush**(真流式);`reasoning_content` 收尾单独提示(边来边刷屏扰人,沿用 §3.3 #3「不进历史只给人看」语义);tool_calls 不边打(收尾由审批闸问人才亮相)。

### 7.4 中断:作废而非留半截,且全程每轮可中断

中断语义最该想清楚的点:**中断时绝不把半截 assistant 消息压回历史**。半截 `tool_calls.arguments` 是残缺 JSON(流到一半被掐),回灌进 history 会让下一轮模型看见「我刚才调过工具但参数是半截乱码」、原地打转。所以定为**本轮作废**:Ctrl-C → 取消流 → 打印 `[已中断 —— 本轮作废,对话历史不保留半截。]` → 回 REPL 顶等下一句。安全且干净,对话历史里只有完整轮次。

机制:`run()` 启一个后台 task 装 `tokio::signal::ctrl_c()`,经 **mpsc 通道**(非 oneshot)把信号推给主循环;`run_one_turn` 每个工作循环轮 `interrupt_rx.recv()` 现取一个 future 挂进 `select!`。**选 mpsc 是因为 oneshot 一次性、多轮工具调用就废了**——mpsc 让每一轮模型生成都能被 Ctrl-C 中断,不止第一轮。`tokio::select!` 用 `biased` 让中断优先,哪怕 chunk 正在来也尽快响应。

还有个易忽略的「间隔期早到信号」坑:用户在 REPL 等待期(不是生成期)连按了 Ctrl-C,信号会囤在通道里、下一轮 agent loop 一进去就被秒中断。 REPL 顶每轮 `try_recv` 清一遍囤积信号——只让**本轮生成期间**按下的 Ctrl-C 生效。

### 7.5 旁路口:`--no-stream` 回退非流式

P5 流式刚上,留个 `--no-stream` CLI 旗:走老非流式 `chat_completion`(不接中断)。流式真出问题时(某 provider SSE 形态偏差、累积器有 bug)一旗回退老路径排错,不至于黑屏。——这是 `docs/codeagent-concepts.md` 里「内省/旁路」原则在工程上的具体落地:永远给主路径留一条可对比的回退路。

### 7.6 门禁 ✅

`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`(零警告)+ `cargo check --tests` 全绿。途中过了几道:
- `reqwest` 默认 `default-features=false` 关掉了 `bytes_stream()` 所需的 `stream` feature → 加 `features=["stream"]`(并显式引 `futures-util` 给 `StreamExt::next`)。
- select 里 `&mut interrupt` 要参数 `mut`;F 的 `Output` 是 `Option<()>`(mpsc recv 返 `Option<()>`,非 oneshot 的 `()`),约束按 `Option<()>` 写才对。
- `println()` 漏写感叹号(`println!()`)——一个真笔误,E0423 「expected function, found macro」精准抓出。
- clippy `too_many_arguments`:`run_one_turn` 9 参 → 加 `#[allow(clippy::too_many_arguments)]` 并注说明(参数各有来路,捏 `LoopCtx` struct 反而要解构重新借,绕一圈不更清楚)。
- clippy「private type 比 pub field 更窄」:`StreamTcDelta`/`StreamTcFunc` 跟 `StreamDelta.tool_calls` 一起改 pub。

`tokio` 加了 `signal` feature(Ctrl-C)。

### 7.7 P5 实测印记(本机,2026-08)

三道闸绿后 cargo build 出二进制,本机跑了建议的四组(默认流式开,DeepSeek)。下面是去敏后的真行为,**只记已发生的**——一处反馈还反手揪出了这版实现的一个提示位置 bug,当场修了,留印记。

#### 第 1 组:流式逐 token 真来了

```
> 用中文写一段 200 字左右介绍 Rust 的,分两三段。

[Rust 介绍正文,三段,以「……的首选」收尾,逐字吐出]
(思考: The user wants a Chinese introduction to Rust, about 200 characters, in 2-3 paragraphs. This is a simple writing task, no tools needed.)
```

- 正文**逐字吐**(对比 P2-P4「干等到全好」一整段蹦),流式体感成立 ✅。
- **⚠️ 揪到一个 bug**:`(思考: …)` 落在了正文**之后**,而协议(`P5-streaming-protocol-notes.md` §2.4)约定 DeepSeek 的 `reasoning_content` **先于** `content` 流。这是**提示位置 bug,不是累积 bug**——累积层 `StreamAcc` 把 reasoning 单独累积是对的,问题在打针:P5 初版只在循环外收尾时把整段 reasoning `println!` 一次,而那时 content 早边来边打完了,顺序回不去。
- **修复**:把 `reasoning_content` 也改成**边来边打**(像 content 一样 `print!`+flush,前后用 `reasoning_opened`/`content_started` 两个标志错开视觉),收尾只兜底封 `)`、不再二次整段打。这样 reasoning 自然落在正文之前(协议保证其先流)。详见 `codeagent-journey.md` 本节末「思考位置 bug 修复」。

#### 第 2 组:中断作废、历史不保留半截(完美印证 §7.4 承诺)

```
> 再详细写 500 字关于所有权和借用的。

[正文流到「……在编译阶段」]
[已中断 —— 本轮作废,对话历史不保留半截。]
> 刚才你写到哪里了
(思考: [模型先思考要不要查文件确认……])
[模型答:「刚才我写到 Rust 整体介绍的第二段末尾——提到 Rust 被应用于 Tauri、Ripgrep、Deno 等知名项目,并说它"成为越来越多开发者与大型项目在工程可靠性与性能之间的首选",还没有展开讲所有权与借用。」
 然后继续详细写所有权与借用 300+ 字]
```

- `[已中断…` 立即打出、立即回 `>` 提示符 ✅。
- 关键印证:问「刚才你写到哪里了」,模型答到的是**上一轮(P5 之前那次 Rust 介绍)的收尾「首选」**,而**不是**这轮半截的「……在编译阶段」——说明中断那轮的半截 `tool_calls.arguments` 或半截 content **没进历史**(§7.4「本轮作废不留半截」兑现)。模型甚至思考「要不要先查文件」,reverse-proof 历史**干净**。
- 这正是中断语义的设计命门:半截的 content/arguments(尤其中断在 tool_calls 流到一半时 arguments 是非法 JSON)若灌回历史,模型下一轮看到「我自己调过一坨坏 JSON」会原地打转。作废、不压、回 REPL = 干净。

#### 第 3 组:`--no-stream` 旁路回退

```
cargo run -- --no-stream
> 用一句话说 Rust 是什么。
(思考: The user asks a simple knowledge question. No tools needed. Answer in one sentence in Chinese.)
[一句话回答]
```

- 回到非流式整条一次性蹦 ✅。`--no-stream` 旁路保留(P5 拆出的第一件:出问题时回退到非流式 = 退路)。
- 注意:非流式分支的 `(思考:)` 因 `chat_completion`(非流式)一次性返回整 reasoning_content,收尾打一次位置是对的——这版修复只动流式分支,不动非流式。

#### 第 4 组:多轮工具调用里的中断窗口(未真触发,留坑)

- `先读 codeagent.toml 然后告诉我里面有几个 provider`:模型**一轮就调完 read_file** 并据结果答「2 个 provider(deepseek 默认 + nvidia)」,没赶上需要中断的「多轮 model 生成」窗口。这组的本意是印证 `mpsc` 全程可中断,但本轮模型没给机会触发——**记为已跑、未印证**,不臆造。留待后续有连续多轮工具调用的场景再实测。

#### 思考位置 bug 修复(代码层,留印记)

- 病灶:`src/main.rs::chat_completion_stream` 收尾区原写的是 `if let Some(r) = reasoning.as_deref().filter(!empty) { println!("(思考: {})", r); }`——一次性整段打,但此时 content 已流式打完,顺序回不去。
- 改法:循环内 reasoning_content 与 content **都边来边打**,各用 `reasoning_opened`/`content_started` 错开:
  - reasoning 起时(正文还没冒)→ 先 `println!()` 隔行,打 `(思考: ` 开头,增量 `print!`+flush;
  - 正文起时若 reasoning 还没收完 → 先 `println!(")")` 封掉思考段再起新行;
  - 收尾若思考段还开着口(正文始终没接上的反常情况)→ 兜底 `println!(")")`。
- 收尾区那行二次整段 `println!("(思考: …)")` 删掉,改 `if reasoning_opened { println!(")"); }`。
- 三闸重过全绿(fmt `--check` 0 diff、clippy `-D warnings` 0、`check --tests` 0)。
- 一个 clippy 小插曲:封口初版写 `print!(")\n")` 触 `print_with_newline` lint,改 `println!(")")` 即过——`println!` 就是 `print!` + `\n`,lint 教后者用前者。

> **P5 阶段意义**:agent 的体感头一次像「真在跟一个会边想边说的东西对话」——逐 token 让长答案不再是黑盒倒计时,Ctrl-C 让你拿回控制权不必干等。这两件加在一起是 REPL 从「能跑」到「痛快」的最大一跳。协议层的硬约束(tool_calls 靠 index 配对 / arguments 字符串拼接 / 终止双判据 / 错误帧识别)不是为了好看,是流式真跑起来绕不开的——一条错累积逻辑会让流式时高时崩;一条漏判 `[DONE]` 之外的 EOF 会让 NIM 卡死。本机实测还反手揪出了思考提示的位置 bug(收尾才打 → 落在正文后,违反协议先流序)并当场修掉——这正是「实测驱动实现完善」的写照:协议核证挡住了累积层炸,但提示层的次序得真跑一遍才看得见。

---

## 8. P5.5 落地:rustyline REPL —— 行编辑 + 命令历史 ↑↓(2026-08-08,代码就位·待本机实测)

### 8.1 动因:P5 的 REPL 还是「裸 stdin」

P5 把「逐 token + 一拍即停」做完了,但 REPL 读入那头还是 `io::stdin().read_line` 裸读——没法光标回退修前面打错的字、↑↓ 翻不出上一句重发、`Ctrl-C` 在 readline 窗口会被读成逐字符而非「取消当行」。这是 P5 当初拆出来、说「native 依赖 + Windows 编译环境 + 接管 stdin 后 Ctrl-C 路径要单独验证」的第三件。现在做它。

`rustyline`(readline 在 Rust 里的现成实现,基于 Antirez 的 Linenoise)正解三件:行内光标移动/删除、文件持久化的↑↓ 历史、Ctrl-C 在 raw mode 下转成可识别的 `ReadlineError::Interrupted`(取消当行、不退出)。一个 crate 把 REPL 从「能跑」补到「顺手」。

### 8.2 关键设计:Ctrl-C 两路职责不撞 —— readline 里归 rustyline,生成里归 mpsc

这是 P5.5 唯一值得单独讲的设计点,因为它和 P5 那套 `mpsc + tokio::select!` 的中断机制直接相邻,搞不好就打架:

- **P5 的中断**(已就位):后台 task 装一个 `tokio::signal::ctrl_c()` 监听器,经 mpsc 通道在**模型生成期间**(流式循环里 `tokio::select!` 挂着)取一条中断中流式。
- **P5.5 的 readline**:rustyline 在 `readline()` 期间进 raw mode,Console 的 Ctrl-C 信号**在它那层就被吃掉**、转成 `Err(ReadlineError::Interrupted)` 返回——不会往上冒到 P5 那个 `tokio::signal::ctrl_c` 监听器。

这两路什么时候各管各?**它们天然不重叠**:readline 时根本没在生成(还在等用户输完这行),生成时根本没在 readline(流式循环占着 stdout/stdin 的注意力)。所以:

- readline 期间按 Ctrl-C → `Interrupted` → `continue`(取消当行、重打提示符,不退出不喂 mpsc)。语义对:你刚敲一串想作废重来,不是想杀正在跑的东西(根本没东西在跑)。
- 生成期间按 Ctrl-C → 走 P5 老路(mpsc 中断、本轮作废不留半截)。语义对:正在逐 token 吐,你嫌慢想停,这正是中断该干的。

唯一要小心的是「REPL 等待期连按了 Ctrl-C」——P5 已有 `while interrupt_rx.try_recv().is_ok() {}` 在每轮 `run_one_turn` 前清早到的信号(readline 期间产生的残信号会积在通道里,等下一轮真生成时被秒中)。但既然 P5.5 把 readline 期间的 Ctrl-C 交给 rustyline 吞了(不再进 mpsc),那个清残信号在 P5.5 后事实上更不容易攒到——留它是不伤的兜底,P5 的清法继续在。

**审批闸那处(`ApprovalGate::check` 里的 y/N `read_line`)不上 rustyline**:它是单字符一次性确认、嵌在 agent loop 中途(正打断输出流式),上塞尔维亚模式(line editing raw mode)的干预太重、收益为零。保持裸 `io::stdin()`,职责单一:只读一行确认。

### 8.3 落地:三处改动

1. **`Cargo.toml`**:加 `rustyline = "18"`(写明引它的理由 + Ctrl-C 与 mpsc 职责分工的注释)。
2. **`src/main.rs` use 区**:引 `rustyline::error::ReadlineError` 与 `rustyline::DefaultEditor`。一个坑:首版顺手把 `rustyline::History` 也 use 了(以为 `load_history`/`save_history` 要 trait 在作用域),`cargo check` 报 `E0603 trait History is private`——这俩方法是 `DefaultEditor` 自带的(借 `DefaultHistory` 实现),不需要显式引 `History`,删 use 即过。
3. **`src/main.rs::run` 主 REPL 循环**:把
   ```rust
   print!("> "); stdout flush; read_line(&mut input); n==0→EOF 退
   ```
   换成
   ```rust
   let mut rl = DefaultEditor::new()?;
   rl.load_history(".codeagent_history") // 不存在 → 静默(first run 正常)
   loop { match rl.readline("> ") {
       Ok(line) => …
       Interrupted => continue,        // Ctrl-C:取消当行重来
       Eof => { println!(); return Ok(()) }  // Ctrl-Z/D:退 REPL
   }}
   // 非空非 /quit/exit → add_history_entry + 进历史
   // /quit/exit → save_history 后退(失败不挡退,记 stderr)
   ```
   历史文件落 **CWD 相对 `.codeagent_history`**(简化起始——后续可接 `Config::config_dir` 等价定位,与 launcher 习惯对齐)。`load_history` 找不到文件静默(首跑正常),`save_history` 失败只 `eprintln!` 不挡退。

### 8.4 门禁 ✅

三闸绿。途中过了几道:
- `E0603 trait History is private`:见上,删 `use History` 即过——`load_history` 等是 `DefaultEditor` 自带,不需 trait 在作用域。
- fmt `--check`:两处行长超限(`DefaultEditor::new(...).map_err(...)` 那行、`matches!(...)` 那 if 守卫行)→ `cargo fmt --all` 自动折成多行。
- clippy 与 check 干净通过,无遗留警告。

### 8.5 P5.5 实测印记(待本机)

代码面完成、门禁绿。代码层能自动验的已验到边界,再往下是交互层、留本机。

#### 代码层已验(非 TTY 退化不炸)

rustyline 在非终端环境(stdin 是管道、非 TTY)的退化路径已排硬伤:新二进制 `printf 'exit\n' | codeagent.exe` → 非交互收行后退,**不 panic**(exit code 2、无 traceback)。这条排了 rustyline 在自动化/CI/管道喂入场景下「直接崩」这类设计前最该先排的坑。

但**这就到边界了**:rustyline 检测到非 TTY 时退回非行编辑模式,↑↓ / 光标移动 / Ctrl-C 转 `Interrupted` 这些 raw-mode 行为**根本不发生**——它们只在真交互终端里才有。用管道自动化自检只能给人「测到了」的假象(实际没进行编辑路径),违「只记已发生的、不臆测」。所以下面五组**必须在真实终端里手敲**,留本机。

#### 五组实测步骤(都 `cargo run`,默认流式开)

1. **行编辑**:敲半行字、用 ← 移回去改中间一个字、回车发——印证光标行内移动生效(裸 stdin 时代改不了前面)。
2. **↑↓ 历史**:发一句话、答完、按 ↑——应吐出上一句话可重发;再 ↑ 翻更早。
3. **Ctrl-C 取消当行**:敲一半字按 Ctrl-C——应见提示符另起一行(当行作废),不是退出,也不是被当成中断喂给正在生成(本来就没在生成)。
4. **历史落盘**:发几句 → `/quit` 退 → 重新 `cargo run` → 按 ↑ —— 应能翻出上一会话发过的句子(印证 `.codeagent_history` 落了)。
5. **回归**:Ctrl-C 中断**生成中**的流式仍成立(走 P5 老路,不被 P5.5 影响)——发个长答案、生成中途按 Ctrl-C,见 `[已中断…]` 回 REPL。

把去敏真输出贴回,我补 §8.5 交互层印记、关 P5.5 章;然后接 P6(上下文管理/压缩)。

#### 交互层真印记(本机,2026-08,逐组回补,只记已发生的)

**第 1 组 行编辑 —— ✅ 通过**

实测现象:敲「你好世界」→ 按 `←` 移光标到「好」之后 → 在中间插入「啊」 → 回车发「你好啊世界」。终端实时显示插字过程(只贴了最终态)。模型收到「你好啊世界」、按普通中文问候回应,未触发任何工具,问「有什么想让我做的」。

- 印证:**rustyline 已接管 stdin 的 raw mode**(裸 `io::stdin().read_line` 不可能让你在行内移动光标)。光标行内移动 + 中间插入编辑生效。✅
- 这条是 P5.5 的本钱——没有它,后面四组没意义(rustyline 根本没接管)。

其余四组(↑↓ 历史 / Ctrl-C 取消当行 / 历史落盘 / 回归中断生成)本机继续,贴回后逐组回补。

**第 2-5 组 —— ✅ 逐组通过(实测确认式,非逐字现象 transcript)**

> 注:这一批回执为「用户逐组确认行为对得上预期」式,而非像第 1 组那样带终端画面的逐字 transcript。据「只记已发生的」纪律,下面只记**用户实测确认成立的结论**与**对照关系成立的判据**,不臆造我未亲见的终端逐字输出。

- **第 2 组 ↑↓ 历史 ✅**:发一句、答完、按 `↑` 调出上一句可再发、再 `↑` 翻更早 —— 用户确认「可以」。印证 rustyline `DefaultEditor` 的 history 机制在本会话内会话内存生效。
- **第 3 组 Ctrl-C 取消当行 ✅**:敲半行字、按 `Ctrl-C` —— 用户确认「可以」。印证键盘断流期间(没在生成)的 `Ctrl-C` 落到 rustyline 转成 `ReadlineError::Interrupted`、走 `continue` 另起提示符,**不是退出、也不是 `[已中断…]`**。
- **第 4 组 历史落盘 ✅**:`/quit` 退 → 重新 `cargo run` → 按 `↑` 翻出**上一会话**发过的句子 —— 用户确认「正常」。印证 `.codeagent_history` 落盘 + 跨会话重拾生效(`save_history` 在退前落盘、`load_history` 在起后载入、首跑无文件静默不崩)。
- **第 5 组 回归:生成中 Ctrl-C 仍管用 ✅**:长答案生成中途按 `Ctrl-C` —— 用户确认「正常」,即见 `[已中断 —— 本轮作废,对话历史不保留半截。]` 回 `> `。印证 P5 那套 `mpsc + tokio::select!` 中断**没被 P5.5 弄坏**、仍走作废语义(§7.4 不留半截)。

**关键对照:第 3 组 vs 第 5 组 —— Ctrl-C 两路职责真分开(已实测印证)**

这是 P5.5 唯一带独立设计风险的一条,实测合龙:

| 时刻 | Ctrl-C 到哪 | 表现 | 实测组 |
| --- | --- | --- | --- |
| 读入期间(没在生成) | rustyline raw mode 吃掉 → `Interrupted` | 另起提示符、不退、不中断 | 第 3 组 |
| 生成期间(流式 `tokio::select!` 挂着) | mpsc `interrupt_tx` → `select!` 取消流 | `[已中断…]` 本轮作废不留半截 | 第 5 组 |

两种表现**截然不同**(「取消当行」vs「中断作废生成」),且按 §8.2 设计在两边各走各的职责路径、互不喂错 —— 两路天然不重叠的判断**被本机实测背书**。这是 P5.5 落地的最关键一条:不是「加上去就好」的幸事,是「想清楚 + 实测验证」才敢落的设计判断。

> **P5.5 阶段意义**:这是 REPL 体感的最后一公里——逐 token 让输出不再憋、Ctrl-C 让生成能停、rustyline 让**输入**也能修能翻历史。三件凑齐,REPL 才从「能用」真正变「顺手」。最值得记的是 Ctrl-C 两路职责的设计判断:rustyline 读期间吃掉 Ctrl-C 转成当行取消、P5 的 mpsc 只管生成期间——它们天然不重叠,所以没真打架;但这条得**想清楚 + 实测验证**才敢落,不是「加上去就好」这种幸事。这版只动主 REPL 那处读入,审批闸的 y/N 明确不上 rustyline——职责单一处不引新依赖、不必给嵌在中途的读入掺一份 raw mode 干预。

---

## §9 P6 上下文管理 —— 先「能见」再谈压缩(2026-08-08)

> 进入 P6 前,一个判断要先立住:**压缩是有损的**(摘要要丢原文、要重排序、要赌模型的「记得住」)。没真量到「长到要压了」之前就臆造一个压缩阈值/策略,是拿困惑度开盲盒。所以 P6 拆两步:**P6.0 先把上下文用量「能看见」** —— 每轮把 token 数如实打到 stderr;**P6.1 等真长会话观测到 token 在涨、且涨到哪儿开始伤模型表现,再针对性定压缩策略**。本节是 P6.0 —— 度量层落地,还没动手压。

### 9.1 协议核证:token 数从哪来(关键坑)

要做「度量」先得有度量值。OpenAI/DeepSeek 的 Chat Completions 协议里,usage 不是随便拿的:

**非流式响应** —— 顶层直接带 `usage` 对象:

```json
{ "choices": [...], "usage": { "prompt_tokens": 12, "completion_tokens": 34, "total_tokens": 46 } }
```

DeepSeek 在此之上还多 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` / `completion_tokens_details.reasoning_tokens`(Cache 命中拆分 + 思考 token 明细)。这几个本版本先不消费,`serde` 默认忽略即可。

**流式响应** —— 坑在这。usage **不在每一帧**,只在**最后一帧**(发 `data: [DONE]` 之前那帧),且:

1. 那帧 `choices` 往往是**空数组** `[]`(只带 usage、没有 delta)。
2. 中间所有帧 `usage` 字段为 `null`。
3. **必须**在请求体里带 `stream_options: { "include_usage": true }` —— 不带,末帧那帧根本不发,你一个 `prompt_tokens` 都拿不到。

第 3 条是协议的硬开关,核证自 DeepSeek 官方文档("usage 末帧"段,印证件存 `docs/P5-streaming-protocol-notes.md`)。这一条不查文档、光看别人示例很容易漏 —— 因为非流式根本没这个开关,惯性思维会以为流式也顶层自带。

### 9.2 P6.0 设计:三处改动让 usage 流到观测面

落点全部对齐「最小侵入、不动 agent 内核」:

**① 请求层**(`main.rs::ChatRequest`)
- 加 `stream_options: Option<StreamOptions>` 字段(`#[serde(skip_serializing_if = "Option::is_none")]` —— 非流式不带它,免得给非流式请求多发一个无意义字段)。
- 新建 `struct StreamOptions { include_usage: bool }`(单 bool 为何单独建结构体:协议要的是嵌套对象 `{"include_usage":true}` 形态,不是平铺 bool —— 单独 struct 让 serde 自然序列化成那个形态,比手拼 JSON 干净)。
- 流式调用(`chat_completion_stream`)请求里填 `stream_options: Some(StreamOptions { include_usage: true })`;非流式(`chat_completion`)和 probe 走 `None`。

**② 解析层**(`tools.rs`)
- `StreamChunk` 加 `#[serde(default)] pub usage: Option<Usage>` —— 流式末帧才填,中间全 null,故 `default`。
- `ChatResponse`(非流式)加 `#[serde(default)] usage: Usage` —— 防御性,某些代理可能不回 usage。
- 新建 `pub struct Usage { prompt_tokens, completion_tokens, total_tokens }`(跨流式末帧与非流式顶层共用一套),`#[derive(Default,...)]` 让缺字段退 0。
- `StreamAcc`(流式累积器)加 `usage: Option<Usage>` 字段。
- **ingest() 的关键修正**:旧实现是

  ```rust
  let Some(choice) = ch.choices.into_iter().next() else { return };
  ```

  一旦 `choices` 空(正是 P6 usage 末帧的常态)就直接 return,**在判 choices 之前根本没看 usage** —— usage 永远拿不到。改成「先取 usage 再判 choices」:

  ```rust
  pub fn ingest(&mut self, ch: StreamChunk) {
      if let Some(u) = ch.usage { self.usage = Some(u); }  // 先取 usage
      let Some(choice) = ch.choices.into_iter().next() else { return };
      // ... 原 choices 处理
  }
  ```

  这一行顺序调换是 P6.0 最隐蔽的一条 —— 旧代码对「中间帧 choices 有料」场景完全正确(中间帧也就没 usage),只有 usage 末帧这个空-choices 帧才会把它踩穿。这条不靠经验、靠协议核证 + 测试。

- `finalize()` 返回值从 5 元组改为 named struct `FinalizedReply { content, reasoning, tool_calls, finish_reason, usage }` —— 既过 clippy `type_complexity` 门禁(超过 4 元组的返回报警),也让调用端可读(原来调端要 `let (content, reasoning, ...) = ...` 数位置,现在 `fr.content` / `fr.usage` 语义直白)。

**③ 观测层**(`main.rs::run_one_turn`)
- 流式分支收完(`StreamOutcome::Completed(r, f, u)`)调 `report_usage("stream", round, &u)`;非流式分支在 `.map` 里调 `report_usage("non-stream", round, &u)`。两条路径对称走同一个 report。
- `report_usage` 写 **stderr 不是 stdout**:

  ```rust
  fn report_usage(kind: &str, round: usize, u: &Usage) {
      eprintln!("[ctx:{kind}:{round}] prompt={} completion={} total={}",
          u.prompt_tokens, u.completion_tokens, u.total_tokens);
  }
  ```

  走 stderr 是有意的:stdout 是给模型生成内容 + REPL 提示符的主对话流,usage 是**开发观测面**。混进 stdout 会污染 `printf '...\n' | codeagent` 之类的管道用法,也让人读对话时多一行噪声。stderr 给到「想看就看、不看不扰」的正确分面。

全 0 的 usage 也照打 —— 「这轮拿不到度量」本身就是观测信号(能让人看出 include_usage 没生效 / 上游不回),不是该藏的失败。

### 9.3 我能测的 vs 必须本机实测的(对齐用户「测试你能测试也一并测」)

P6.0 的可测性分两半,诚实拆开:

**能自动测的 —— 已用单测焊死(本机 `cargo test` 3/3 过)**

`tools.rs` 新增 `#[cfg(test)] mod tests`,三条不联网、纯函数、可执行断言锁协议核证:

| 测试 | 锁住的协议点 |
| --- | --- |
| `ingest_picks_up_usage_from_empty_choices_frame` | usage 末帧 `choices` 是空数组,旧 ingest 会因 `choices.into_iter().next() = None` 早 return、**在判 choices 之前没取 usage** → 漏。现实现「先取 usage 再判 choices」,这条断言那个顺序。 |
| `finalize_carries_both_content_and_usage_through_a_minimal_stream` | 端到端:content 帧 → finish 帧 → usage 末帧,`finalize()` 同时透出 content="hello"、finish="stop"、usage.total=46。防重构时 content / usage 之一被错顺位的代码吃掉。 |
| `finalize_usage_is_none_when_never_seen` | 反向钉:从不带 usage 的流(代理不回 / 没开 include_usage)`finalize()` 给 `usage: None`(上层 `unwrap_or_default()` 退全 0)。锁住「别把 None 默默填假 0 误导度量」。 |

> 本机这次 `cargo test` 居然干净跑过(没撞 `STATUS_ENTRYPOINT_NOT_FOUND` 那个 cdylib 运行时 DLL 加载的环境问题 —— 按 `ci-gates-windows-cdylib` 记忆,它是环境偶发不是代码,`cargo check --tests` 绿即编译期门禁绿;但这次 test 二进制也真跑起来了,3/3 ok,实测链路一个证据多一层)。

**不能自动测、留本机实测的 —— P6.1 压缩策略**

「压缩」这一步要观测**真长会话**:token 数怎么随轮次涨、涨到哪个 total 模型开始丢上文、丢了哪一段、摘要回来后模型是否还能续上。这些只能真跑长对话,我无法在本机无 key、无长历史的条件里 auto-run 出来 —— 硬造一个压缩阈值是臆造,违背「只记已发生的」。所以 P6.0 只做「能见」,把度量台子搭好;**真正压缩策略(P6.1)留真长会话观测后再定、并记本机实测印记**。这是对用户「测试你能测试也一并测 就不要让我手动测了」的诚实拆分:**能测的我测了(协议层三条硬证 + 三道门禁绿);真长会话压缩不能 auto-run,明示不臆造、留本机**。

三道 CI 门禁(fmt / clippy `-D warnings` / `check --tests`)本轮全绿。

### 9.4 P6.0 阶段意义

P6.0 不是「实现了上下文管理」—— 它实现的是「上下文管理的前置条件:能量」。没有 token 数,后面所有压缩/截断/摘要策略都是空中楼阁:砍多少、从哪砍、砍完模型还认不认得 —— 全要拿 token 增长曲线当输入。这一步把量引出来、走 stderr 不扰人、并把"末帧空-choices 帧"这个最隐蔽的协议坑用 3 条单测焊死,**不靠经验靠核证**。

下一步 P6.1:等真长会话把 token 涨势量出来,再定压缩窗口(候选:超过某 total 触发——把最旧 N 轮 tool 交互压成摘要保留结论、丢中间冗余 tool_result——但具体 N 和阈值都待观测,不预设)。

---

## 路线图状态栏

- [x] P0 单轮问答骨架(deepseek 联通)
- [x] P0.5 多 provider 配置(TOML,Provider 抽象)
- [x] P0.5+ NVIDIA provider 实证(nemotron-3-ultra-550b-a55b,配置切换不改代码)
- [x] P1-1 探针(`cargo run -- probe`,打印 tool use 原始响应)
- [x] P1-2 两家实测对照(deepseek vs nvidia,index/content/reasoning 三处差异)
- [x] P1-3 Tool trait + ReadFile + 协议解析结构(据 §3.3 三结论,tools.rs 落地)
- [x] P1-4 main 里跑通一轮 tool loop(agent loop 实测打通,§3.8 印记:模型主动 tool call→读文件回灌→据文件内容答 nvidia,闭环真 agent 行为)
- [x] P2 连续多轮 REPL(实测打通,§4.7 印记:跨轮不重读 + 模型自审计工具调用史,有记忆会话活起来了)
- [x] P3 扩工具集(实测打通,§5.8 印记:write_file/bash 两度过审闸 + 模型自主连用 list/read_file 验结果)
- [x] P4 权限审批(实测打通,§6 落地:可配置白名单闸 ApprovalConfig/ApprovalGate —— 读全免/命中前缀免/可疑才问/--yolo 兜底;§6.7 印记:git status 命中白名单免审、git log 未命中弹闸、N 后模型换写法再试 —— 闸拦刀不拦意图)
- [x] P5 流式输出 + Ctrl-C 中断(本机实测打通,§7.7 印记:逐 token 真来了 + 中断作废不留半截完美印证 + --no-stream 旁路对得上 + 多轮工具中断窗口未真触发留坑;实测反手揪出思考提示位置 bug「收尾才打落在正文后」并当场修复,改 reasoning 边来边打、收尾只兜底封口)
- [x] P5.5 rustyline REPL(本机实测打通,§8.5 印记:行编辑光标中间插字成立(证明 rustyline 已接管 stdin raw mode)+ ↑↓ 历史 + .codeagent_history 跨会话重拾 + Ctrl-C 取消当行 + 生成中 Ctrl-C 仍走 P5 作废语义 —— 第3组vs第5组对照实测印证 Ctrl-C 两路职责真分开;第2-5组为实测确认式非逐字 transcript)
- [ ] P6 上下文管理(P6.0 度量层落地:stream_options.include_usage + Usage 透出 + ingest「先取 usage 再判 choices」修正 + report_usage 走 stderr;协议核证三条单测 3/3 过、三道门禁绿。P6.1 真压缩策略留真长会话观测后定,不臆造)
- [ ] P7 会话持久化
- [ ] P8 MCP / subagent
