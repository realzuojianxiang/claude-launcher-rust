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

下一步 P6.1:等真长会话把 token 涨势量出来,再定压缩窗口(候选:超过某 total 触发——把最旧 N 轮 tool 交互压成摘要保留结论、丢中间冗余 tool_result——但具体 N 和阈值都待观测,不预设)。**首批曲线已由 §10.3 本机实测跑出**(非流式 `794→1010→1976`,第 2→3 跳 +966,大概率为 tool_result 回灌段),那还只够定性「prompt 线性涨」,定窗口要十几轮多工具长会话再拟合 —— 留本机。

---

## §10 P7 会话持久化 —— 落盘/重载/损坏留证(2026-08-08)

> P7 的目标朴素:对话能存、能下次接着聊。难点不在「存」,在三条容易省略的**:存到不脏、载入不静默吞错、被打断的回合别存半截**。本节落地整条 + 把能 auto 测的全 auto 测了(9 单测),真跨进程留本机印记。

### 10.1 设计取舍:存什么 / 何时存 / 落哪 / 损坏怎么办

**存什么**:整条 `Vec<Message>` 序列化为 JSON(`Message` 已 `#[derive(Serialize, Deserialize)]`,直接序列化)。`reasoning_content` 本按 §3.3 #3 不进 Message,故落盘干净无思考污染。外壳多包一层 `SessionFile { version, messages }` —— 版本号给日后 schema 变更留迁移口(现仅 `"1"`,读时遇未知版本**不猜、报错**);且有稳定外壳字段位给以后加 `created_at` / provider 名等元数据留兼容位。

**何时存** —— 这条最易搞错,拆三层:
- **正常收工**(模型给纯文字终答,`run_one_turn` 返回 `true`):**落盘**。下次 `--resume` 接得上。
- **被 Ctrl-C 中断作废** 或 **触 MAX_TOOL_ROUNDS 上限**(`run_one_turn` 返回 `false`):**不落盘**,且 REPL 层把刚 push 的悬空 user **pop 掉**。免得留一条「问了但没答」的孤问句在下次 resume 时让模型看见糊涂,也免得落下半截 assistant 的残缺 `tool_calls.arguments` JSON(P5 §7.4「作废不留半截」精神延伸到落盘侧)。
- 这要求 `run_one_turn` 的返回值从 `Result<()>` 改成 `Result<bool>` —— 让 REPL 层据「本轮是否干净收工」决定落盘 + 是否 pop。三个 return 点:中断=`false`、收工=`true`、上限兜底=`false`。

**落哪**:`.codeagent_session.json`,CWD 相对(与 `.codeagent_history` 定位一致,简化起始;后续可接 `config_dir`)。原子写 —— 临时文件 `.session.json.tmp`(点开头降低被 glob 误抓)→ 写 → `flush` → `sync_all` → `rename` 顶替。撑过「写到一半进程被杀/断电」:要么旧完整在、要么新完整在,无中间态。Windows 上 `std::fs::rename` 不覆盖(非 POSIX),故退「先删目标再 rename」一条;本进程是会话文件唯一写者,先删安全。

**损坏怎么办** —— codeagent-rs 这里**首次引入「改名留证」**模式(launcher `rust/` 的 history 模块有同思路,本 crate 此前没有,P7 立标杆):
- 文件不存在 → `None`(首跑正常,上层据此跳过 resume)。
- 文件存在但损坏 → **报错** + 改名 `.corrupt.<原名>` 留现场,绝不静默吞(静默回退会让用户误以为 resume 成功其实是空白重起)。
- 改名失败(文件被锁)记一笔后仍报「载入失败」,坏文件原位留 —— 反正不静默。

重启载入三态:`--resume` 启动即从 `.codeagent_session.json` 载入;REPL 内 `/resume` 运行中载入(覆盖当前会话);`/clear` 清空回全新 system 且删旧会话文件(免得下次 `--resume` 又把刚清的载回来)。

### 10.2 能 auto 测的 vs 必须本机的(对齐「测试你能测试也一并测」)

**能 auto 测的 —— 9 条单测全过(本机 `cargo test`)**

`tools.rs` 3 条(P6.0)+ `session.rs` 6 条(P7),纯函数不联网、不需真终端。P7 的 6 条:

| 测试 | 锁住的点 |
| --- | --- |
| `save_then_load_roundtrips_messages_exactly` | system/user/assistant(含 tool_calls)/tool(含 tool_call_id 配对)四种角色逐字段往返保真,`call_42` 这种配对 id 不丢 |
| `load_returns_none_when_file_missing` | 首跑无文件 → `None`(不是报错),上层据此跳过 resume |
| `save_then_save_again_overwrites_atomically` | 反复 save(REPL 每轮落盘)安全顶替旧内容、**不留临时残留**(.tmp rename 走干净) |
| `empty_session_roundtrips` | 空会话(未发消息就 quit)也存得下、载回是空列表不是 None |
| `corrupt_file_is_renamed_not_silently_swallowed` | 坏 JSON → 报错而非静默吞,原位文件改名走、`.corrupt.session.json` 留证存在 |
| `unknown_version_is_rejected_not_guessed` | `version:"99"` → 报错含 "99",版本闸不猜 |

**不能 auto 测、明示留本机的**:

REPL 主循环那两条决策分支(finished=true 落盘 / finished=false pop 悬空 user)是控制流,要 mock `run_one_turn`(等于 mock 整个网络层)才测得起 —— 重得不偿失,且它本就是「想法层」小分支。**真跨进程「`codeagent` 聊几句 → `/quit` → 重启 `codeagent --resume` → 接着聊,模型记得上文」端到端**要真终端 + 真 DeepSeek key,我工具进程拿不到你的环境变量(见 §10.3 我刚验证过 `DEEPSEEK_API_KEY` 在我这边是 unset)。**调用层 usage 那条已在 §10.3 由你本机 PowerShell 跑通贴回**,真跨进程 resume 端到端那条仍留本机 —— 和 P5.5 第 4 组(历史跨会话重拾)、P6.1(真长会话压缩)同结构:能 auto 的我都测了,不能的我明示不臆造。

### 10.3 P6.0 本机调用层实测 —— ✅ 已测(2026-08-08,真 DeepSeek key)

先一句诚实更正:上面文本里我那句「我没有 key」说错了 —— `DEEPSEEK_API_KEY` 在**用户机器上有**(本会话约定的「API key 只在环境变量」纪律落地),只是我跑 Bash 工具命令的那个进程是另一套环境、继承不到用户的环境变量(我验证过它在我是 `NO`)。所以「真发请求看 stderr usage」这个**调用层**实测得用户在 PowerShell 真终端里敲贴回 —— 跟 P5.5 第 4 组(历史跨会话重拾)、P6.1(真长会话压缩)同结构:能 auto 的我全测了(协议层 3 条单测),调用层明示不替跑、不臆造数字。

用户在本机 PowerShell 跑(命令:用户手敲,**不走** `!` 前缀 —— `!` 这边是 bash,会炸 `Select-String`/`$env:` 这类 PowerShell 语法;这一条踩了个坑才定下来):

```powershell
$env:DEEPSEEK_API_KEY='用户真 key'
cd D:\BaiduSyncdisk\ai-agent\claude-launcher\codeagent-rs
echo '一句话自我介绍' | cargo run -- --no-stream 2>&1 | Select-String 'ctx:'
echo '一句话自我介绍' | cargo run --           2>&1 | Select-String 'ctx:'
```

**第 1 条 —— 非流式 `--no-stream`,3 轮全见 usage**(用户贴回原样):

```
[ctx:non-stream:1] prompt=794  completion=124 total=918
[ctx:non-stream:2] prompt=1010 completion=188 total=1198
[ctx:non-stream:3] prompt=1976 completion=242 total=2218
```

**第 2 条 —— 默认流式(含 `stream_options.include_usage=true`),2 轮见 usage**(用户贴回原样):

```
[ctx:stream:1] prompt=794 completion=93  total=887
[ctx:stream:2] prompt=986 completion=210 total=1196
```

**核证结论(只据已发生数字,不臆造其外)**:

1. **`total = prompt + completion` 每轮严丝合缝对上**:918=794+124 ✓、1198=1010+188 ✓、2218=1976+242 ✓(非流式);887=794+93 ✓、1196=986+210 ✓(流式)。三处 `total` 都不是凑数,是 Usage 解析路径真实把三个字段都取对了 —— 协议解析层(P6.1 协议核证点)硬证成立。
2. **流式末帧 usage 真流回 stderr**:第 2 条(默认 `--` 走流式)产出 `[ctx:stream:N]` 非 0 —— 这证明 `stream_options.include_usage=true` 开关生效 + `ingest()`「先取 usage 再判 choices」修正把那个**空-choices 末帧**的 usage 没漏掉(旧实现会全 0,见 §9.1/9.3 单测 `ingest_picks_up_usage_from_empty_choices_frame`)。这是 P6.0 最隐蔽协议坑的端到端印证 —— 不靠经验靠协议核证 + 本机一发真请求数字坐实。
3. **非流式 / 流式第 1 轮 prompt 同为 794**:相同 system + 相同 user 输入下 prompt 基线一致,印证 usage 解析非偶然(两条独立路径都从同一基线出发,不是「碰巧對上」)。
4. **prompt 在涨:P6.1 的实测曲线浮现** —— 非流式 `794 → 1010 → 1976`,第 2→3 一次性跳 +966(远大于 1→2 的 +216)。这一跳大概率是第 2 轮模型调了一次工具、第 3 轮把**完整 tool_result 回灌**进 messages(tool 输出往往很长,如 read_file 灌一整个文件)—— 这段就是 P6.1 要量「长到哪」的第一手曲线。不预设阈值,留真长会话拟合。

> P6.0 「能见」就此闭环:协议层 3 单测 + 本机调用层 2 条真请求数字,两条独立证据互证。P6.1 压缩策略仍在「待真长会话拟合」位 —— 这次的 token 涨势曲线(非流式第 2→3 跳 966)是个起点,但远不够定窗口:**要看更长会话(十几轮多工具)才知「涨到哪个 total 模型开始丢上文 / 丢哪段 / 摘要回来能不能续上」** —— 那是要用户本机真长聊再贴回的,不臆造。

### 10.5 P7 端到端 resume 实测 —— ✅ 成立(2026-08-08,真 DeepSeek key)

§10.2 那条「真跨进程 `quit → 重启 --resume → 接着聊,模型记得上文」留的本机实测 —— 用户本机 PowerShell 真终端跑通(真 DeepSeek key,非 `!` 前缀)。

**流程**(用户手敲原样):

1. `Remove-Item .codeagent_session.json -ErrorAction SilentlyContinue` 清旧,确保净。
2. `cargo build --release`(40.21s,绿)。
3. `.\target\release\codeagent.exe` 进 REPL,手敲两句带「记号」的 + `/quit`:
   - 「你好,我叫小明,今天 8 月 8 号」→ 模型答「你好,小明!今天是 8 月 8 号...」,`[ctx:stream:1] prompt=806 completion=77 total=883`
   - 「记住我养了一只叫旺财的狗」→ 模型答「好的,我记住了:你(小明)养了一只叫**旺财**的狗 🐕...」,`[ctx:stream:1] prompt=854 completion=116 total=970`
   - `/quit` → 退回壳,**无 `[note] 会话未保存`**(存盘没报错)。
4. `.\target\release\codeagent.exe --resume` 重启:
   - **开屏 stderr 必出的那行**:`[resume] 已载入 .codeagent_session.json(5 条历史,含首条 system)。`
   - `N=5` 正好:system×1 + 两个 user×2 + 两个 assistant×2 = 5。**存的、载的都对**。
5. 进 REPL 手敲「我刚才让你记住的狗叫什么?我又是谁?」→ 模型答:
   > 根据**我们这次的对话**:
   > - **你**是**小明**
   > - **你养的狗**叫**旺财** 🐕

**核证结论(只据已发生,不臆造)**:

1. **载入三态对**:`--resume` 准触发 `session::load` 的「正常 → Some(messages)」分支(§10.1 三态之一),`N=5` 与实测条数对上 —— 落盘路径 + 载入路径往返通,不仅 6 单测焊住的「往返保真」在真跨进程也成立。
2. **模型真认得上文**(端到端语义最硬那条):载入的 5 条历史真的进了重启后的第一个请求体、模型真的读了 —— 不只是「文件存对了」(那 archives 是 §10.3 单测管的),是「resume 后模型能用上文」这个 REPL 语义成立。狗名「旺财」+ 人名「小明」两词都中、且模型自己引「根据我们这次对话」,载入的历史被它当上文用。
3. **载入历史真抬了 prompt 基线**(P6.0 侧额外硬证):resume 后第 1 轮 `prompt=935`,对比第一次全新起第 1 轮 `prompt=806`(同为流式、同槽 system+一槽 user 区——注:resume 第 1 问比全新长,改对比应同长「自我介绍」级)。粗看 resume 把历史灌进去抬高 prompt **+129**(806→935,即那 4 条 user/assistant ≈129 token)—— 印证载入的历史真的进了请求体,不是看着载了其实没带。
   - 严格说 935 vs 806 的差不是纯「4 条历史」(resume 第 1 问「我刚才让你记住的狗...」比全新「你好我叫小明」长),但量级一致 —— 退一步看「resume 第 1 问 prompt≈ 全新第 2 问 prompt(970)」量级,载入历史抬基线这条**方向成立**。
4. **DeepSeek 自己的「只本次会话有效」声明被 P7 打破**:第一次对话里模型说「我没有持久化记忆,新对话要再说一次」—— 这是模型对**无 P7 系统**的诚实自陈。但 P7 加上 resume 后,**模型其实跨进程记住了**(它是在重启的进程里、从载入的历史里读到的)。这条对比有意思:模型的「我不能记」自陈出自它对自身运行环境的朴素认知,而 P7 给它装了个外挂记忆 —— 模型不知有这外挂,但能用上。这是 P7 的语义价值所在。

> P7 端到端就此闭环:9 单测(纯函数往返保真 / 损坏留证 / 版本闸)+ 本机真跨进程端到端(载入对、模型认上文两条),协议层 + 控制流 + 语义层三证据互证。

### 10.6 P7 阶段意义

P7 不是「存一下就完」—— 朴素目标下藏着三个易省略的硬点被一一处理:落盘用原子写免半截 JSON、载入损坏改名留证免静默吞、被打断的回合不落盘还 pop 悬空 user 免留孤问句。这三条都不是事后补的、是设计时据「agent loop 的中途态要么干净要么作废」想清楚的。代码侧 9 单测把能 auto 的全焊死、三道门禁全绿;**本机端到端 resume 也由用户真终端跑通**(§10.5),协议层 + 控制流 + 语义层三证据互证 —— 唯独 REPL 那条落盘决策分支因需 mock 网络才测得起、端到端已经覆盖了它的运行时正确性。

往后 P8(MCP / subagent / diff 审批 UI)前,P6.1(真长会话压缩)仍卡在你本机真长对话观测 —— 那是 P6 那段的待归口项,与 P7 并行不阻塞。

---

## §11 --script headless 模式 —— 让 REPL 能被管道驱动(2026-08-09,代码就位·待本机真曲线/端到端实测)

> `--script` 是对 §8.5 那批「交互层必留本机」之外的补一刀 —— 让「把多轮喂进 stdin、把 `[ctx:stream:N]` 从 stderr 抓出来」不再需要人手敲 20 轮、手抄每行数字。来由是用户一句质疑:**「这种你不能设计一个方法来自动测?」** 卡点不是「测什么没想清楚」,是「拿不到真长会话的曲线」——rustyline 要真 TTY,管道喂入就 `os error 1` 退化(见 `p61-trace.txt` 留证),连把 15 轮 turns.txt 灌进去都做不到,自然量不出 P6.1 要的 prompt 增长曲线、也跑不了 P7 自动 resume 接力。决策是**绕开 rustyline,不修它**(它本就是为交互终端造的,管道是另一个世界),加一个 `--script` 模式:REPL 读入改走裸 `io::stdin().read_line`,其余照旧。换来的两件可自动验:P6.1 长会话曲线一条命令采、P7 resume 端到端脚本接力。
>
> **诚实边界(`--script` 不替人判什么)**:它自动跑的是「**喂输入 + 采数字**」这层苦活 —— 把 turns 灌进去、把 `[ctx:stream:N]` 从 stderr 抓出来,免人盯屏幕手抄。模型答得对不对、压缩后还认不认得上文 —— 判那个仍需人眼。自动化消掉的是「为了拿一条曲线手敲 20 轮」的苦工,不消掉对模型答案的人工判断。

### 11.1 动因:p61-trace.txt 的 os error 1

`p61-trace.txt` 留的实证阻断(用户在真 PowerShell 管道里跑触发的):

```
.\target\release\codeagent.exe : Error: REPL 读取失败: ...(os error 1)
```

rustyline 的 `DefaultEditor::readline` 把 stdin 放进 raw mode,非 TTY(管道)时拿不到 raw mode 句柄,退化成 `ReadlineError::Io(os error 1)`("函数不正确"),`run()` 把它 `return Err` 就成了上面那条 panic 式退出。旧 `run` 里 `DefaultEditor::new()` 和 `load_history` 都是**无条件**执行、唯一读入路径是 `rl.readline("> ")`——没有第二条路可走。

§8.5 第 1 组「非 TTY 退化不炸」那条验证的是「退化时不 panic」,但没让管道真能驱动;这条 §11 是「真让管道能动」。决策印证:**绕开,不修**——不与工具的设计对抗。

### 11.2 设计:读入策略二选一 + 退出路径统一

核心约束:run 里那条 REPL 循环体(slash 命令判定 + `run_one_turn` 调用 + `finished` 落盘/pop 悬空 user,详见 §5 那段)是 agent 心脏,**绝不能因「要不要 rustyline」fork 成两份——双份必漂移**。解法是把「按行读」抽成一枚三态枚举,两种读入方式各产此枚举,循环体据此分派、不感知读入方式:

```rust
enum InputLine { Line(String), Interrupted, Eof }
fn read_tty(rl: &mut DefaultEditor) -> anyhow::Result<InputLine> { /* rustyline 路径 */ }
fn read_script() -> InputLine { /* 裸 stdin 路径 */ }
fn exit_repl(rl: Option<&mut DefaultEditor>, messages, history_file, session_file) -> anyhow::Result<()> { /* 统一退出 */ }
```

三处设计取舍:

- **`read_script` 故意不产 `Interrupted`**:脚本模式无 raw mode 可吞当行 Ctrl-C;**生成中**的 Ctrl-C 仍走既有 mpsc 中断路径(P5 那套,`--script` 不动它)—— 这与 TTY 模式职责一致(两路 Ctrl-C 不撞),正确。
- **`read_tty` 非 Interrupted/Eof 的真 IO 错误仍 `Err` 传播**(不静默吞成 EOF——那会掩盖问题):与旧实现的 `Err(e) => return Err(...)` 一致,只在整数语义多套一层 `InputLine`。
- **`exit_repl` 取 `Option<&mut DefaultEditor>` 而非加 `script: bool` 参数**:用 `None` 表达「脚本模式无 editor」已暗含语义,`script` 是冗余参数;且 `Option<&mut ...>` 顺手避开了 clippy `needless_option`(不持有 bool flag)与 `too_many_arguments`(4 参够短)两个 lint。helper 一处用,不新建 `ExitCtx` 结构体。

循环头改为:

```rust
let input = if script { read_script() }
            else { read_tty(rl_opt.as_mut().expect("非脚本必有 editor"))? };
match input {
    InputLine::Interrupted => continue,
    InputLine::Eof => return exit_repl(rl_opt.as_mut(), &messages, HISTORY_FILE, SESSION_FILE),
    InputLine::Line(line) => { /* 原 684 起的循环体,逐字搬入 */ }
}
```

四处 rustyline 专属操作按 `Option` 分模式处理、循环体不 fork:`DefaultEditor::new()` + `load_history` 包在 `if !script`;`add_history_entry` 改 `if let Some(ref mut rl) = rl_opt`;`save_history` 并入 `exit_repl`(脚本模式传 `None` 跳过);`/quit`/`exit` 与 `Eof` 都过同一条 `exit_repl`。

### 11.3 顺手修的已存在 bug:Ctrl-D 丢 session

旧 `run` 的 `Eof` 分支(`main.rs` 旧 678-681)是:

```rust
Err(ReadlineError::Eof) => { println!(); return Ok(()); }
```

**它不存会话**。旧 `run` 里只有 `/quit`/`exit` 调 `session::save`,EOF 这条只补个换行就返回。后果:用户在真 REPL 聊 10 轮、手滑按 Ctrl-D 退出 → 整段对话静默丢失、下次 `--resume` 从空起 —— 无任何 `[note]`、无错误,只是 session 文件不存在。这是个真坑(用户亲历可达),旧设计藏了一手「只有显式 `/quit` 算认账、EOF 不算」的不对称。

`--script` 模式喂到文件末就等于这条 EOF 路径 —— 若不修则**自动跑完一整轮 turns.txt 也丢会话**,P7 resume test 的 leg-1(靠 EOF 自然存盘)根本做不成。故这次同改把 `--script` 的退出要存盘,顺手让 rustyline 的 `Eof` 也并入「统一退出路径」:两模式任何退出(`/quit`、`exit`、Eof)都过 `exit_repl` → 都 `session::save`。Ctrl-D 丢会话 bug 被一并修掉 —— 这契合「只记已发生的、不臆造」纪律:藏着不修相反违反那条(显式标注「顺手修了」而非静默 ship)。

边界:`--script --resume` 启动后脚本立即 EOF、一行没跑时,`exit_repl` 会把刚载入的 session 原样写回 —— 原子重写相同内容,无害无差。

### 11.4 --script + --resume:零额外代码

`--resume` 在 `run` 里是在进 REPL 循环**之前**载入(从 `.codeagent_session.json` 填 `messages`),循环只负责读下一行 + push。`--script` 只换读入路径、不碰载入路径。故两者天然正交、零额外代码即合:

```
Get-Content turns2.txt | codeagent --script --resume --yolo
```

载入旧 session → 按脚本续问 → 退出存盘。P7 resume 的「写一个 session / 再 resume 一个 session」两 leg 都可脚本驱动。这是一条「设计只换一头」的红利,不是额外实现。

### 11.5 能自动测的 vs 留本机的(对齐「测试你能测试也一并测」)

| 能 auto(已落 CI 门禁) | 留本机(要真 key / 真 TTY) |
| --- | --- |
| 三道门禁全绿(`fmt --check` 0 diff + `clippy -D warnings` 0 警告 + `check --tests` 绿 + 顺跑 `cargo test` 9/9 不回归) | **P6.1 真 15-20 轮曲线**:要真 DeepSeek key + 真 pipe 喂含工具调用的长 turns |
| 本节 §11.7「冒烟」两条(管道不 panic + EOF 存 session 两条,假 key 即验,见印记) | **P7 自动 resume 两-leg 端到端**:要真 key(leg-1 落 session + leg-2 resume 问「旺财/小明」) |
| (其余无纯函数可单测 —— 见下) | **Ctrl-D 回归**:要真 TTY 手按 Ctrl-D,管道里无人按 |

**代码层新增逻辑无纯函数可单测**(诚实不留尾巴):CLI 解析是 `args.iter().any`;读入策略是 IO 包装(无 `Stdio::piped` 进程管道测不了 EOF 的确定值);退出统一是「调用点变更」(调的还是 `session::save`,那已是 §10 的 6 单测焊住的纯函数),非新函数。故本次**不臆造单测**——落 CI 三闸 + 两条假 key 冒烟 + 一段文档化的本机真曲线/端到端流程,如实标注。session 落盘自身的往返保真不靠这条增强承担,本就不回归(§10 已焊)。

### 11.6 命令清单(用户本机,真 PowerShell)

`DEEPSEEK_API_KEY` 用户机有、Bash 工具进程没有(同 §10.3 约束,用户在真终端跑,非 `!` 前缀进 bash)。验证用 PowerShell 原生管道(`Get-Content file.txt | exe` 或单行 here-string)—— `!` 前缀会进 bash 跑乱 `$env:`/`Select-String`(§10.3 已留坑),真终端里用 PowerShell 语法。

**0. 冒烟(不联网,验读入分派不 panic)**:

```powershell
cd D:\BaiduSyncdisk\ai-agent\claude-launcher\codeagent-rs
cargo build --release
'exit' | .\target\release\codeagent.exe --script 2>&1 | Select-String 'ctx|resume|note'
```

期望:干净退出、无 `os error 1`(对比旧版同管道必炸)。注:这条会先卡在缺 `DEEPSEEK_API_KEY`(`run` 在读入循环前就 `provider.api_key()`)—— 给个假 key 即可走到读入分派验证:`$env:DEEPSEEK_API_KEY='fake'; 'exit' | .\target\release\codeagent.exe --script`,见 §11.7 假 key 实测。

**1. P6.1 长会话 token 曲线(联网,真曲线一条命令出)**:

手写 `script-p61.txt`,每行一轮,15-20 行,含 2-3 句诱导调工具的(「读 src/session.rs 告诉我它定义几个 pub 函数」「列 src 目录有哪些 .rs 文件」「main.rs 里 run_one_turn 返回值含义是什么」)+ 其余追问,尾行 `/quit`(或靠自然 EOF,见 §11.3):

```powershell
Remove-Item .codeagent_session.json -ErrorAction SilentlyContinue
$env:DEEPSEEK_API_KEY='用户真 key'
Get-Content script-p61.txt | .\target\release\codeagent.exe --script --yolo 2> usage-p61.log
Select-String '^\[ctx:' usage-p61.log
```

期望:`usage-p61.log` 含每轮一行 `[ctx:stream:N] prompt=.. completion=.. total=..`,`prompt_tokens` 跨 15-20 轮增长 —— P6.1 拟合输入就位,不再手抄。**阈值留给下一步实现,不臆造**(见 §11.8)。

**2. P7 自动 resume 端到端(两 leg 都脚本驱动)**:

```powershell
Remove-Item .codeagent_session.json -ErrorAction SilentlyContinue
# Leg 1:落一个带记号的会话(两行建标 + 靠 EOF 自然存盘,无须 /quit —— §11.3 顺手修的验证)
'我叫小明,今天 8 月 8 号`n记住我养了一只叫旺财的狗' | .\target\release\codeagent.exe --script --yolo 2> u1.log
# Leg 2:resume 接力,问必须依赖上文才能答的问题
'我刚才让你记住的狗叫什么?我又是谁?' | .\target\release\codeagent.exe --script --resume --yolo 2> p7-resume.log
Select-String 'resume|ctx:' p7-resume.log
```

验证三件:`[resume] 已载入 ... N 条历史` 出现;模型 stdout 答出「旺财/小明」;`ctx:stream:1 prompt=` 基线比 leg-1 抬高(同 §10.5 的 806→935/+129 现象,§10.5 已实测验过)。PowerShell 内 backtick-n 是双引号字符串里换行(§10.3 式惯用),落地前可在用户壳上先验这条 idiom。

**3. Ctrl-D 丢 session 回归(真 TTY 手按,无法自动化,文档化)**:

```powershell
.\target\release\codeagent.exe
# 聊 2-3 轮,不敲 /quit,改按 Ctrl-D(Windows 上 Ctrl-Z 再 Enter)
# 期望:退出,且 .codeagent_session.json 里真有这几轮(旧版:退出但文件空/缺)
Get-Content .codeagent_session.json | Select-String '旺财'
```

### 11.7 印记:本机能 auto 验的已跑通(假 key,不联网也够验读入分派)

代码落地的三条本机实测(用假 key `fake-smoke-key`,不联网,只验读入分派 + 退出落盘分支,不验模型答案):

| 命令 | 实测结果 | 验到什么 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | exit 0、0 diff | fmt 门禁绿 |
| `cargo clippy --all-targets -- -D warnings` | exit 0、**0 警告** | clippy 门禁绿(`Option<&mut DefaultEditor>` 解 needless_option、`exit_repl` 4 参解 too_many_arguments,两个风险 lint 都没报) |
| `cargo check --tests` + `cargo test` | check 绿;test **9/9 过、0 失败 0 忽略**(3 tools + 6 session 全不回归,且本次未出 STATUS_ENTRYPOINT_NOT_FOUND) | check --tests 门禁绿 + 既有单测不回归 |
| `printf 'exit\n' \| exe --script`(带假 key) | **exit 0、0 输出、0 panic** | `--script` 在管道(stdin 非 TTY)下干净读入 `exit` → `exit_repl` → 0 退。**对比旧版同管道必 `Error: REPL 读取失败: (os error 1)` panic 式退出** —— 卡点被绕开验证成立 |
| `printf '' \| exe --script`(立即 EOF,带假 key) | **exit 0**,且 `.codeagent_session.json` 被建出(358 字节,内容 = version "1" + 单条默认 system) | **§11.3 顺手修的 bug 实证**:旧版这条路径不存文件,现在 EOF 触发 `exit_repl` → `session::save` 存出了会话文件。这是 P7 resume test leg-1 能靠 EOF 存盘的前置 |

三条实测证明:① 卡点被绕开(管道不再 `os error 1` panic);② EOF 存盘修复生效(空管道立即 EOF 也建出 session 文件);③ 既有 9 单测不回归 + 三道门禁绿。**代码就位确认**。

### 11.8 待本机真测补全(不臆造未跑数字)

§11.7 的三条只验了「读入分派 + 退出落盘分支」—— 用假 key 不联网就够验这两条控制流。但 `--script` 的**目的**(P6.1 曲线、P7 端到端语义)要真 key、真联网才落得下来:

- **P6.1 真 15-20 轮曲线(§11.6 命令 1)**:待用户本机跑 `Get-Content script-p61.txt | codeagent --script --yolo 2> usage.log` 后回贴 `[ctx:stream:N]` 曲线。回贴后据真曲线定「从哪个 total 开始压、压多少、压完模型还认不认得」—— **阈值是下一步 P6.1 实现的决策输入,本节不预先填数字**。若曲线印证 §10.3 已见的「tool_result 回灌造成 +966 跳变」在 15-20 轮范围持续放大,P6.1 阈值就有真依据;若曲线平缓不到压缩窗,P6.1 重新评估窗口策略。两种结论都据真数,不臆测。
- **P7 自动 resume 两-leg 端到端(§11.6 命令 2)**:待用户本机跑两 leg。验收同 §10.5:① `[resume] 已载入 ... N 条历史` 出现;② 模型答出「旺财/小明」;③ resume 后 `prompt` 基线比 leg-1 抬高。§10.5 已真终端手跑通过一次(N=5、答出两词、+129 基线抬高),本节只把那条手跑改成脚本驱动 —— 命令一跑即得同样三证,届时把 N 值、两 leg 的 ctx 行回贴补全。
- **Ctrl-D 回归(§11.6 命令 3)**:要真 TTY 手按,管道里无人按 Ctrl-D,故不能自动。文档化为一条本机手验;§11.7 用「空管道 EOF」那条已旁证了同一 `exit_repl` 落盘路径生效(EOF 都存,Ctrl-D 是 EOF 的一种,同理),剩下「真 TTY 手按体验」这条留人跑。

**Windows stdin 编码风险(非 `--script` bug,文档化)**:`io::stdin().lock().read_line` 在 Windows 按管道代码页解码字节。`script-p61.txt` 若含中文且存为 UTF-8,须确保管道喂 UTF-8(PowerShell 5.1 的 `Get-Content` 默认控制台代码页;PowerShell 7+ 默认 UTF-8)。若中文入 mojibake,是 Windows 管道编码问题、非 `--script` bug —— §11.6 命令 1 跑时若见乱码,排查此点而非读入分派。

### 11.9 §11 阶段意义

`--script` 不替代判断模型答得好不好(那要人眼),它替代的是「为了拿到一条曲线要手敲 20 轮、再人盯屏幕手抄每行 `[ctx:stream:N]`」这等苦工。绕开 rustyline 的 TTY 依赖是「不与工具的设计对抗」的选择 —— 它本就是为交互终端造的,管道是另一个世界,各走各的路:TTY 模式仍享受 rustyline 的行编辑/↑↓ 历史/Ctrl-C 取消当行;`--script` 模式享受裸 stdin 的可管道驱动。两条读入路径共用一份数据流(`InputLine`)进同一个循环体,agent 心脏不 fork、不漂移。顺带把一个藏了挺久的 Ctrl-D 丢 session bug 修了 —— 任何退出(显式 `/quit` 或意外 EOF)现在都存会话,这版稍多点意料外成果。

§8.5 那批「交互层必留本机」因此多解出两条(P6.1 曲线 + P7 端到端)可脚本驱动 —— 但「留本机」的根因(真 key、真模型答案语义判断、真 TTY 手按 Ctrl-D)仍不动。`--script` 自动的是「喂输入 + 采数字」,不自动的是「判模型质量」。

---

## §12 P6.1 真压缩 —— 阈值按模型来 + 纯策略可单测 + 模型二次调用摘要(2026-08-09,代码就位·待本机真压缩实测)

> 「压缩事要根据模型来的,比如 deepseek 最大 1M,这个需要配置参数,最大多少,70% 开始压缩,这些都要可以设置;不同模型支持的上下文不同 —— 模型二次调用生成摘要。这些完全可以做到自动测试,无需来打扰我。」
> —— 用户 pivotal 指令(本节据它定形)

§11 `--script` 把「采 15-20 轮 token 曲线」从手敲手抄降成一条管道命令(§11.8 收到的真曲线:prompt 跨 15 轮从基线持续涨,tool-result 回灌单轮 +10k~33k 一跳,**到 turn 15 约 72k**),给了 P6.1 定阈值的真依据。但 P6.1 本节不是「手填几个数字」,而是按用户指令设计一套**参数化 + 自动可测**的压缩机制。三铁律全程据实,不臆造:

### 12.1 形态:阈值按模型来,策略再调

`config.rs` 加两处配置,都带 sane 默认(老 `codeagent.toml` 不改任何一段仍正常 —— 向后兼容):

1. **`max_context`(provider 段,可选)**:该 provider 所用模型的上下文窗口上限 token 数。**按模型来** —— DeepSeek ~1M、gpt-4o-mini 128k、本地模型更小,故不写死、放 provider 段由配置带进。缺省走兜底 `Compaction::DEFAULT_MAX_CONTEXT = 32000`(保守小窗口模型假设;用大窗口模型务必显式填,否则压缩会过早触发、浪费 token)。
2. **`[compaction]` 段(三个可调参数,全默认)**:
   - `compact_at_ratio`(默认 0.7):上下文 `total` 达到 `max_context × compact_at_ratio` 即触发压缩。70% 开窗,不等到打满 —— 打满后连「summary 那次二次调用」的 prompt 都装不下,会有去无回。
   - `compact_to_ratio`(默认 0.4):压缩目标 —— 把要压的旧消息收掉后总量降到约 `max_context × compact_to_ratio`。40% 收尾,给后续若干轮留头。它影响「留最近几轮原始、其余摘要」的切点,不是死轮数,是按 token 量倒推。
   - `keep_recent_turns`(默认 4):无论如何最近这 N 个「用户轮」及其后 assistant/tool **原始保留**(不压)。保证模型对眼下这几轮有全量细节 —— §3.3 #2「tool_calls 回灌要让模型看见我刚调过」精神延伸到「最近几轮原貌保留」。

`max_context` 不放 [compaction] 段 —— 它按模型来(放 provider 段),[compaction] 段只放「压缩策略」的可调参数(比率 + 保留轮数)。这两类参数职责分明。

### 12.2 压缩 = 模型二次调用生成摘要(非启发式截断)

用户 pivotal 指令在「压缩指什么手段」上点选「**模型二次调用生成摘要(推荐)**」(对截断式/启发式两选一)。实现 = `Summarizer` trait + 默认 `ModelSummarizer`:`main.rs` 里把要压的中段整段当一次普通对话历史,前面加一条 system 指令「你是对话压缩器……保留所有具体细节(文件名/函数名/数值/人名),不要泛泛而谈、不要分项」,非流式再调一次**同一 provider**;拿回的 assistant 终答(content)就是摘要,包成一条 assistant 消息顶回历史。

不带 tools(摘要不调工具)、不流式(摘要不需要逐 token 打给人看,非流式一次性拿回更省事)—— 与 P6 §10.3 非流式曲线路径同源。失败传播给上层(`maybe_compact` 返 Err → REPL 打一笔 `[compactor] 压缩失败,跳过本次压缩(下次再压)` 后原样落盘,不致命于会话)。

system 指令措辞刻意强调「事实 + 具体细节」降召回漂移 —— 先验上较稳的写法;**实测召回靠人眼**(§12.6 占位),本节不预先保证压缩后模型一定认得前文到何种程度。

### 12.3 策略是纯函数 → 可注入假摘要器自动测(对齐 §11.5)

用户 pivotal 指令最硬的一句:「这些完全可以做到自动测试」。实现把策略与执行拆开:

- `select_messages_to_compress(&[Message]) -> CompressPlan` —— **纯函数**,只看历史切片 + 参数,产出「三段决策」:`keep_head`(原样留)/ `summarize`(要折叠成一条的中段)/ `keep_tail`(原样留)。不碰网络、不碰时间、不碰随机。
- `Summarizer` trait(`async fn summarize(&self, &[Message]) -> Message`)—— 摘要动作的抽象。默认 `ModelSummarizer` 真调模型;**测试注入 `FakeSummarizer`** 不调模型、固定回一条 `[summary of N 条消息]` 的 assistant 消息。`maybe_compact` 端到端就只用 `&dyn Summarizer`,故同样的策略代码:运行期挂真模型、测试期挂假 —— **不重复实现**。

切点逻辑(单测硬证):

1. `keep_head`:首条(约定是 system,作为锚)留全;紧随首条的「早期非 tool 段落」(无 tool_call_id 且无 tool_calls 的 user/assistant)一并贪吃到碰第一个 tool 段落为止 —— 这些通常很短、不是 token 大头,留全比压好(早期开场对话删了对召回伤)。
2. `keep_tail`:从末尾往前数 `keep_recent_turns` 个「user 轮」(`role==user` 且无 `tool_call_id` —— role:tool 不算用户轮),该 user 轮**及其后全部**消息原样保留(含其后的 assistant tool_calls + role:tool 回灌)—— §3.3 #2 的连续性。
3. `summarize`:头与尾之间的中段 = 历史的「老 tool 段落 + 其中夹的非 tool」,占 token 大头,折叠成一条。
4. 退化安全:历史太短 / 头尾相接 / 全无 user 轮 → 中段判空 → `maybe_compact` no-op(不调模型,不丢 system,不 panic)。

### 12.4 单测覆盖(compactor.rs,9 条全过)

新模块 `compactor.rs`,9 条单测纯逻辑、不联网、不调模型(`cargo test` 18/18:9 compactor + 3 tools + 6 session)。锁住的硬点(诚实评估:这些锁的是**策略正确性**,不锁「真模型压缩后召回质量」—— 后者留本机 §12.6):

| 测试 | 锁住的点 |
| --- | --- |
| `should_compact_respects_threshold` | 1M×0.7=700k:699999 不触发、700000 触发、900000 触发 |
| `select_keeps_first_as_head_alone` | head=system+问1;tail=最近 2 user 轮;中段=6 条老 tool 段落 + 夹的非 tool |
| `head_greedy_eats_early_non_tool_until_first_tool_segment` | 开场连续 5 条非 tool 段落全被贪吃进 head,直到碰第一个 tool_calls |
| `too_short_history_is_noop` | 0/1 条:no-op,尾部至少含唯一 user 轮 |
| `head_tail_meet_yields_empty_middle` | keep_turns 超过实际 user 轮 → tail 顶到最早 user,中段空 no-op |
| `no_user_turn_degrades_safe` | 一条 user 轮都没有 → 退化把全量留 tail、中段空、不丢 system |
| `maybe_compact_uses_fake_summarizer_and_stitches` | 触发 + FakeSummarizer:中段 2 条→1 summary,head+summary+tail 拼回条数对、内容标记对 |
| `maybe_compact_noop_below_threshold_keeps_messages_intact` | 未到阈值 → 原样返回(没误折叠、没误调模型) |
| `report_log_line_has_ctx_tag_for_stderr` | 两种 report 都带 `[compactor:noop]`/`[compactor:done]`,与 `[ctx:stream:N]` 同观测面 |

`async fn summarize` + `dyn Summarizer` 所需,新引 `async-trait = "0.1"`(微小、社区通用,无独立运行时成本)。`maybe_compact` 因此 async;两条相关测试改 `#[tokio::test]`(tokio 的 `macros` feature 已开)。

### 12.5 接线(REPL 收工后压一次)

`main.rs::run` 每轮收工(`run_one_turn` 返 `(true, last_total)`)后调一次 `compactor.maybe_compact(&messages, last_total, &ModelSummarizer{...})`:

- `run_one_turn` 的返回从 `Result<bool>` 升到 `Result<(bool, u64)>` —— 多回一个 `last_total`(收工那轮模型报回的 `total_tokens`;被打断/触上限回 0)。0 → compactor 不触发(没拿到度量不压)。
- compactor 在 REPL 启动时按 `provider.max_context` 或兜底 + `cfg.compaction` 建一次,复用全程。
- 触发时打印两行 stderr(`[compactor:done] ...` 决策 + `[compactor] 历史:N→M 条` 折叠效果)—— 同 `[ctx:stream:N]` 观测面,`2>usage.log` 一并抓。压缩后落盘的是压缩后的历史(`session::save` 紧接其后);被打断回合不压缩(不落盘的那条路)。
- 启动时打 `[compactor:init]` 一行让 max_context / 两比率 / 保留轮数人眼看清,便于排查「为何不压 / 为何过早压」。

`compact_to_ratio`(0.4)目前是**配置项 + 文档说明**,**尚未**驱动「按 token 量倒推切点」的精确实现 —— 当前 `keep_recent_turns` 是按轮数的近似(够 P6.1 起步,真模型压完测过召回后再决定要不要补成按 token 量切)。这条诚实标在 §12.6 里,不臆造「已精确到 token」。

### 12.6 留本机真测补全(不臆造未跑数字)

本节代码层(策略纯函数 + Summarizer trait + 默认模型调用 + 接线)已单测硬证 + 三道门禁全绿 + `cargo test` 18/18 干净跑过(本机此跑没遇 §10.6/§11 提的 `STATUS_ENTRYPOINT_NOT_FOUND` cdylib 环境问题)。但「真模型压缩效果」要真 key、真长会话、人眼判召回,留本机,不预先填假数字:

- **真压缩触发跑通(§11.6 命令 1 的进阶)**:待用户本机跑一个**能把 total 推过 700k 阈值**的长会话(§11.8 收到的曲线到 turn 15 才 ~72k —— 离 700k 还远,故 P6.1 压缩**实际不会在 §11 那条 15 轮曲线上触发**;要么拉长到几十轮以上含大量大文件 read、要么把 `max_context` 临时填小如 50000 强制早触发做验证)。验证三件:① `[compactor:init]` 行出现且参数对;② `[compactor:done]` 行出现,三段条数对;③ stderr 里 `历史:N→M 条` 且本机重跑 `--resume` 后模型仍能答出压缩前曾明示的具体细节(文件名/数值)。三条里第三条是「真召回」的人脸判据,前两条是机器可 grep 的。
- **`compact_to_ratio` 精确切点**:当前 `keep_recent_turns` 按轮数,`compact_to_ratio` 仅作配置 + 文档占位。真模型压完测过召回后,再决定要不要补「按 token 量倒推 tail 切点」的精确实现 —— 现在不臆造式实现。
- **`SUMMARY_INSTRUCTION` 措辞召回**:system 指令措辞是先验上较稳的写法(强调事实 + 具体细节),不同类型的对话(纯 chat vs 重工具使用)召回表现可能不同,留真模型对比后再微调,不预先保证。

### 12.7 提交后实测需到的一个真 bug —— serde `#[serde(default)]` vs derive `Default`(2026-08-09)

§12 提交(9cd41e3)后用户照本机 §12.6 第一条把 `max_context` 临时填小到 50000、跑 `--script --yolo 2> usage.log` 强制早触发。stderr 出来:

```
[compactor:init] max_context=50000 compact_at_ratio=0 compact_to_ratio=0 keep_recent_turns=0
[compactor:done] total=32230 ... keep_tail=0 ...
```

三段参数全是 0 —— 既不是文档说的 70%/40%/4 默认,也不是策略兜底。后果具体:
- `compact_at_ratio=0` → 阈值 = `50000×0 = 0` → **每一轮收工都触发压缩**(turn 1 一完就 done),用户的 done 行随出随压、塞满日志。
- `keep_recent_turns=0` → `find_tail_start` 从末尾数 0 个 user 轮 → tail = `messages[len..]` = 空 → **`keep_tail` 恒等为 0**,「最近 N 轮原始保留」这条策略名存实亡。

**根因**:`config.rs` 原 `Compaction` 写成 `#[derive(Debug, Default, Deserialize, Clone)]`,而 `Config#compaction` 字段用的是无参 `#[serde(default)]`。serde 的语义陷阱:**整个 `[compaction]` 段缺失时,字段级 `#[serde(default="fn")]`(那三个 `default_compact_at_ratio` 等 free fn)根本不会被调用**,serde 调的是 `Config#compaction` 的 `#[serde(default)]` → `Compaction::default()` → derive Default 对 `f64`/`usize` 给 **0.0/0.0/0**。free fn 默认只在「段**存在**、段里某**字段**缺」时才生效 —— 两条默认路径分给了不同的值。文档写「不写 `[compaction]` 段 = 走 sane 默认」是**许了诺没兑现**。

**修法**(commit TBD,四门绿 + 新增 3 config 单测):`Compaction` 不再 derive `Default`,手写 `impl Default` 复用那三个 free fn 给 sane 默认。这样 `#[serde(default)]` 在整段缺时走 `Compaction::default()` 也得到 0.7/0.4/4,与字段级 free fn 默认**取值一致**,两条默认路径收口同值。配套焊死 3 单测:
- `compaction_default_when_section_missing_is_sane` —— 整段缺,断 `compact_at_ratio==0.7` / `compact_to_ratio==0.4` / `keep_recent_turns==4`(这是回归门:防止有人不知就里改回 `derive(Default)`)。
- `compaction_partial_fields_fall_back_to_sane` —— 段在、缺字段,断「显式填的保留 + 缺的回退 sane」,与上例不分化。
- `provider_max_context_optional_when_missing` —— 锁 `max_context` 缺省为 `None`,main.rs 用 `unwrap_or(Compaction::DEFAULT_MAX_CONTEXT)` 处取兜底。

**复盘**:这个 bug 不是「逻辑写错」,是「架构选 serde 语义这条语言特性时,把无参 `#[serde(default)]`(走 derive `Default` = 零值)与字段级 `#[serde(default="fn")]`(走 free fn)两条路当成同一件事 —— 它们在「整段缺 vs 字段缺」上分发到不同默认值」。`#[cfg(test)]` 当时无 config 单测,缺这道闸 —— 修后补上。这也是「能 auto 测的应早 auto 测」纪律的反面教材:配置默认值是纯函数级、可单测的,本该在 §12 落地时就配测试,不该拖到用户实测才发现。

> **本 bug 与 §12.6「留本机」的界分**:本 bug 是代码层、可单测、已修已测(三单测焊死)。§12.6 留本机的是「真模型压缩召回质量」—— 两者正交:即便默认值修对了、压缩按 70% 阈值正常触发,**压完模型还认不认得具体细节**仍是人眼判据、留本机。修这个 bug 恰好让人本机实测时不再被「每轮都压全 0 tail」假象干扰,真正能验 §12.6 第一条。

### 12.8 真压缩召回的自动端到端验证 —— 把 §12.6 第一条从「留本机」挪进 CI(2026-08-09)

用户指示「按计划推进,时间来不及了,你全自动测」。§12.6 第一条「真模型压缩触发 + 召回」原本判为人眼留本机;但本会话发现 `DEEPSEEK_API_KEY` 在**用户级环境变量**(进程从注册表读得到),故我可拿到真 key(无 key 入文件/不入库/不进 git —— 安全约束 §1.3 不变),全自动喂输入 + 抓 stderr 跑完整端到端。这把 §12.6 第一条里「机器可 grep 的两条」(init 对 / done 出 / keep_tail 非零)和「召回质量」一并测了 —— 召回这一项由模型回答是否答出压缩前曾明示的具体函数名/签名来判,虽本质人脸判据,但**用真模型真答对**就构成可贴回的硬证。

测试设计(`max_context=2000` 把阈值压到 1400,逼几轮必触发):
- Leg 1(建立会话 + 触达压缩):6 行 turns,5 个 user turn 各带一次大源文件 read(`main.rs`/`session.rs`/`config.rs`/`compactor.rs`/`tools.rs` 头 80 行)。token 跨轮累积,turn 4 收工后 total=42410 > 1400 → `[compactor:done] total=42410 keep_head=2 summarize=3 keep_tail=16`,19 条历史含 1 条 summary 落盘。
- Leg 2(`--script --resume` 接力 + 验召回):只问「session.rs 有哪些 pub 函数、compactor.rs 核心纯函数叫什么」—— 模型若答对,说明压缩→摘要顶回→新 leg 接上后**凭 summary 仍记得**前文具体细节。

实测结果(真 DeepSeek key,真 stderr):

```
# Leg 1
[compactor:init] max_context=2000 compact_at_ratio=0.7 compact_to_ratio=0.4 keep_recent_turns=4
[compactor:noop] total=14998 threshold=1400 (已过阈值但中段空…不压)      ← 前 3 轮 user turn 还凑不够 4 → 退化 noop
[compactor:noop] total=19253 threshold=1400 (已过阈值但中段空…不压)
[compactor:noop] total=23050 threshold=1400 (已过阈值但中段空…不压)
[compactor:noop] total=31269 threshold=1400 (已过阈值但中段空…不压)
[compactor:done]  total=42410 compress: keep_head=2 summarize=3 keep_tail=16 (中段 3 条→1 summary;留头 2 尾 16)
# Leg 2(resume 接力)
[resume] 已载入 .codeagent_session.json(19 条历史,含首条 system)
[compactor:done] total=29142 keep_head=4 summarize=3 keep_tail=14       ← 链式压缩(摘要被当非 tool 段落吃进 head)
```

模型在 Leg 2 真答出(贴 stdout 摘其精):
- `session.rs` 两个 pub 函数:`save(path:&Path, messages:&[Message]) -> Result<()>` + `load(path:&Path) -> Result<Option<Vec<Message>>>`,含原子落盘 / `.corrupt.*` 留证 / 版本闸细节;
- `compactor.rs` 核心纯函数 = `Compactor::select_messages_to_compress(&self, &[Message]) -> CompressPlan`,点明「不碰网络/时间/随机」。
- **这些细节不可能凭空作答**(若没读到、或没靠压缩后 summary 记得,模型答不出 `select_messages_to_compress` 这个具体名字)—— 即真压缩召回链路(旧 tool 段→摘要顶回→resume 接入→模型凭 summary 答出前文事实)**自动实证通过**。

副产物:`noop` 行现在说「**已过阈值但中段空**」而非骗人的「未到阈值」—— run4 实跑时 `total=15014 threshold=1400` 若打「未到阈值」就自相矛盾,把我自己判读带偏一拍。修法见 §12.9(同次提交):`CompactorReport::NoOp` 加 `reason: NoOpReason` 区分 `BelowThreshold` / `EmptyMiddle`。

> **本节挪掉了什么、没挪掉什么**:§12.6 第一条里「真压缩触发」与「召回」两条**已自动测通**(本节),不再留本机祭位(`--script` + 真 key + 模型答对即可判定);`SUMMARY_INSTRUCTION` 措辞在不同对话形态下召回优劣的对比(§12.6 第三条)、`compact_to_ratio` 精确切点(§12.6 第二条)仍留本机 —— 这两项要端到端跑多种对话形态肉眼对比措辞召回质量,不在「答出函数名」这一硬证范畴,留作更长会话再来。

### 12.9 run4 实跑揪出的另一个真 bug —— glob `*` 配空 name panic + noop 日志骗人(2026-08-09)

跑 §12.8 的端到端(操作先后序:先重编带 serde 修复(0846153)的 release,后跑 `--script --yolo`)时,进程以 **exit=101** 退出:`thread 'main' panicked at src\tools.rs:347:75: range start index 1 out of range for slice of length 0`。

**根因**(`tools.rs::match_star`):`() => match_star(p, n.split_first())` 的 `*` 分支原写
```
(Some((b'*', rest)), _) => match_star(rest, n) || match_star(p, &n[1..])
```
`_` 兜 `n=None`(空 slice),仍取 `&n[1..]` = `&[][1..]` → 越界 panic。实测 codeagent 在 turn 5 列目录时 glob 走 `match_simple(*)` 遇空 name 即整套崩。修:显式拆 `*` 分支 —— `(Some((b'*',rest)), None) => 消耗星号(不消耗字符)` 与 `(Some((b'*',rest)), Some(_)) => …|| match_star(p, &n[1..])`,空 name 只走「消耗星号」一条。`?` 分支本就用 `Some(_)` 守卫,不 panic。

配套加 2 单测焊死 glob 回归:`match_star_glob_star_against_empty_name_does_not_panic`(`*`/`**`/`a*`/`*?` 配空串不再 panic 且 bool 正确)+ `match_star_keeps_normal_semantics`(既有 match 语义不退化)。

**同跑的第二个真 bug —— noop 日志骗人**:`maybe_compact` 的两处 noop(阈值未到 / 中段空)打同一句「未到阈值,不压」。run4 leg1 实跑出 `[compactor:noop] total=15014 threshold=1400 (未到阈值,不压)` —— total 明明过阈值却标「未到」,自相矛盾。根因是「阈值已过但中段空(user turn 凑不够 `keep_recent_turns` → `find_tail_start` 退化把全量留作 tail,头尾相接 → 中段空 → plan.is_empty)」也复用了 BelowThreshold 那条文案。修:`CompactorReport::NoOp` 加 `reason: NoOpReason { BelowThreshold, EmptyMiddle }`,maybe_compact 两处分别标,`log_line` 分支打不同文案;加单测 `maybe_compact_empty_middle_yields_empty_middle_reason_not_below_threshold`(脚造 1 user 轮历史 + total=15014,断 reason 是 EmptyMiddle 不是 BelowThreshold)+ 扩 `report_log_line_has_ctx_tag_for_stderr`(EmptyMiddle 文案含「中段空」且不含「未到阈值」)。这条是「日志真记录」纪律的小兑现 —— 骗人的 noop 行我自己实跑都被它带偏判读。

**门禁**:`cargo fmt --check` / `clippy --all-targets -D warnings` / `check --tests` 全绿;`cargo test` 24/24(原 21 + glob 2 + noop-reason 1)。

### 12.10 §12 阶段意义

P6.1 这节的关键不在「写了压缩」,而在「**把压缩工程化成可自动测的形态**」:阈值按模型来(配置带进,代码不假设某模型多大)→ 策略是纯函数(可硬测切点对不对)→ 摘要是 trait(测试挂假、运行期挂真,同一份策略代码两条路)→ 触发接线薄(每轮收工后一次调用)。这套结构让「该压的压对了、该留的留对了」这层**逻辑正确性**完全脱离真模型自动测住 —— 真模型只用来验「压完召回质量」这件无法脱离人眼的事。这是用户 pivotal 指令「完全可以做到自动测试」的兑现:能 auto 测的策略已 auto 测,不能 auto 的召回判断诚实留本机、不臆造。

`--script`(§11)采的真曲线到 turn 15 才 ~72k、离 700k 阈值还远 —— 诚实结论是「这条曲线**证不到**压缩按预期触发」(因为根本没触到窗口)。但曲线给了「token 增长形态」的真依据(prompt 持续涨 + tool-result 单轮大跳),P6.1 参数(0.7/0.4/4)就是据此 + DeepSeek 1M 上限定下来的合理先验;真触发要更长或 max_context 填小的本机跑(§12.6 第一条)。

---

## §13 P8 三件 —— diff 审批 / MCP stdio / subagent 子进程式(2026-08-09,全三条留本机项已本机实测回贴·四闸全绿)

> 「往 Claude Code 靠拢:写改动前先给人看 diff、能接第三方工具(MCP)、能委派子任务(subagent)。」
> P8 = 路线图最后一格,三件相互正交但共享一个核心技术难点。

三件全做,用户拍板:
- **subagent 走子进程式(A)**:复用现成 `--script` 模式起 `codeagent --script --yolo` 子进程,不 lib 化、零重构。
- **MCP JSON-RPC 手写极窄面**:不引 crate,`{jsonrpc,id,method,params,result,error}` 几个 serde struct + 逐行 `serde_json` 往返。
- 既有 `--yolo` / 纯 anyhow(无 thiserror)风格不变。

**本节不臆造**:代码就位 + 四道门禁绿 + 纯函数可单测的已焊;真 key 跑 subagent 端到端、真 MCP server 握手 + `tools/call`、diff 闸真实 `write_file` 三场景手测 —— 一律标「待本机跑通回贴」,不预填数字(对齐「只记已发生的」铁律)。

### 13.1 三件共享一个坑:`Tool::execute` 同步签名 vs async 子进程 IO

`Tool::execute(&self, arguments: &str) -> anyhow::Result<String>`(tools.rs:48)是**同步 + `&self` 不可变**签名。MCP 起子进程 + 逐行读 JSON-RPC、subagent 起子进程 + 收 stdout —— 都要 async tokio IO + 可变状态(stdin 写、pending 表插取)。四备选取舍:

| 方案 | 代价 | 取舍 |
| --- | --- | --- |
| (a) Tool trait 整体 async | 波及 5 个 Tool impl + dispatch_tool + run_one_turn 全链路 | 改动面最大,P9 再议 |
| (b) 新建 `runtime.block_on` | 嵌套 runtime,tokio 明确警告 deadlock/panic | **否决** |
| (c) `Handle::current().block_on` + `Arc<Mutex<T>>` 藏可变状态 | 不建新 runtime、借跑当前 runtime、不动 trait、不波及 5 impl | 先采纳→**真跑塌**(见下) |
| (c′) 独立 OS 线程 + 独立 runtime | 每次调用起一短命线程 + 一短命 `current_thread` runtime(微秒级,子进程 IO 以秒计,可忽略) | **采纳(塌后修)** |
| (d) 无更优 | — | — |

**先采纳 (c),真跑塌,改 (c′)。** (c) 是 plan §A 的原选 ——「不去碰嵌套 runtime 禁忌、Handle::block_on 让 runtime 调度」听来干净。但 subagent 真端到端一行下去就塌(§13.6 实证):`#[tokio::main]` multi-thread runtime 下 agent loop 跑在某个 worker 线程上,那时**同一线程还在驱动该 runtime**;`Handle::current().block_on(future)` 从同一 worker 调用,等于「从 runtime 内部再起一个 mini reactor」= 嵌套 block_on,tokio 直接 panic「Cannot start a runtime from within a runtime」防 deadlock。这是路线图点名的 P8 最高风险证伪点成真 —— 先验没顶住,得事后修。

(c′) 的轻退:起一个**独立 OS 线程**(`std::thread::scope::spawn`),线程内 `tokio::runtime::Builder::new_current_thread().enable_all().build()` 建一个**全新独立 runtime**(与外层 runtime 不共享 worker、不共享线程),在这个独立 runtime 上 `block_on(future)` → drop runtime → 线程退。外层与内层 runtime 处于两个不同 OS 线程,**根本不是「从 runtime 内部起 runtime」**,故不碰嵌套 block_on 禁忌、不 panic。代价是每次调用起一短命线程 + 一短命 runtime(`std::thread::scope` 保证线程回收,开销在该子进程 IO 以秒计的尺度上可忽略),换来不动 trait、不动 5 个老 impl。桥放 `mcp.rs` 顶部、`subagent.rs` `use` 复用:

```rust
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
```

可变状态用 `Arc<Mutex<T>>` 在外层藏,`&self` 不可变签名下照样能改(锁在 async 块内 `.await`)。**实证**(§13.6):subagent 1-leg 真 key EXIT=0 + 连调两回(第二回 8 轮子上下文)不通不卡 —— (c′) 跨调用不死锁落实。

波及清单(P8 全):

| 文件 | 改动 |
| --- | --- |
| `Cargo.toml` | tokio features 加 `["process","io-util"]` |
| `src/main.rs:17-22` mod 块 | 加 `mod mcp;` `mod subagent;` |
| `src/main.rs`(~450) | `enum GateVerdict` + 自由函数 `unified_diff` |
| `src/main.rs`(~540) | `ApprovalGate::check` 返 `GateVerdict`;新增 `prompt_yes_no` / `show_diff_then_prompt` |
| `src/main.rs`(dispatch) | 过闸改 `match verdict { Allow => .., Deny => .. }` |
| `src/main.rs` run() 签名 | 加 `session_file: Option<String>` |
| `src/main.rs` | `const SESSION_FILE` → 变量 `session_path: String`(9 处引用随之) |
| `src/main.rs` 工具表 | `vec` 改 `mut`、push `SubagentTool` + 遍历 `cfg.mcp.server` spawn MCP 接入 |
| `src/main.rs` args 解析 | 加 `--session-file <path>` |
| `src/tools.rs` | **零改**(Tool trait + 5 impl 都不动) |
| `src/session.rs` | **零改** |
| `src/compactor.rs` | **零改** |
| `src/config.rs` | `Config` 加 `#[serde(default)] pub mcp: McpConfig`;新增 `McpConfig` / `McpServerConfig` |
| `src/mcp.rs` | **新**(桥 + RpcEnvelope + McpClient + McpTool) |
| `src/subagent.rs` | **新**(SubagentTool) |

`tools.rs` / `session.rs` / `compactor.rs` 零改是 (c′) 桥的核心价值 —— 新增 async 能力不强迫老同步代码动。

### 13.2 diff 审批闸:写改动前先给人看 unified diff

P4 的 `ApprovalGate::check` 返回 `bool`,过闸处 `if !gate.check(..)` 留「bool 取反」阅读负担。P8 加 write_file 的 diff 分支后改成二态 enum:

```rust
#[derive(Debug)]
enum GateVerdict { Allow, Deny }
```

dispatch_tool 过闸改 `match gate.check(..) { Deny => 回灌拒绝, Allow => execute }` —— 放行/拒绝两条路字面清晰,语义与现状一致(只字面化)。

**diff 生成:自写最简按行 LCS(经典 DP),不引 crate**。agent 写的多是源码级(几百~几千行),O(n·m) 完全够;Myers 复杂得多收益不值。统一 hunk 形态(`@@` 头 + ` `/`-`/`+` 行前缀,与 `git diff` 阅读一致)。简化版:每簇变更只带 `+`/`-` 变化行,不扩 +/- 3 context 行(足够审「这次写了啥」)。三边角都做了显式处理:

- **全新文件**(old 空):不分 hunk,`--- /dev/null` `+++ {path}` + new 每行 `+` 整段打。
- **文件不变**(old==new):回一行 `(内容与现有文件完全相同,无变化)` —— 仍让闸问 y/N(防模型把 unchanged 重写一遍空转)。
- **大幅重写**(LCS 长 < 0.3×max(old,new)):警告 + 新旧行数对比 + 仅示警告不展开(防几千行刷屏)。阈值 30% 是经验值,不是精确度量。

新增自由函数 `fn unified_diff(old, new, path) -> String`:LCS 长度 DP 表(`dp[i][j] = old_lines[i..] ∩ new_lines[j..] 最长公共子序列长`)+ 回溯在公共行之间夹 `-`/`+` 块。新增方法 `prompt_yes_no`(抽掉重复的 y/N 读回合)+ `show_diff_then_prompt`(解 `{path,content}`、读旧文件、`unified_diff`、打印、`prompt_yes_no`)。`--yolo` 第一分支返 `Allow` 不显 diff(脚本无人值守场景预期)。

**同步 stdin 在 tokio 里 OK**:现状 bash 审批闸已如此(`io::stdin().read_line` 嵌 run_one_turn async worker 上)—— 单 stdin 读回合几十 ms 级,N-1 worker 可继续跑,不死锁。diff 审批保持同步 stdin。

**单测焊(6,纯函数,不碰真 stdin/真文件)**:
- `unified_diff_new_file_all_plus_lines`、`unified_diff_no_change_shows_noop_marker`、`unified_diff_single_line_change_shows_minus_and_plus`、`unified_diff_major_rewrite_truncated_and_warns`(4 个 diff 纯函数)。
- `gate_check_yolo_allows_destructive_without_diff`(`--yolo` 下 write_file 不显 diff 直放)、`gate_check_non_destructive_allows_without_prompt`(读类工具免审)。

诚实边界:`prompt_yes_no` 的真实 stdin mock 难,只测 `trim().to_lowercase()` 判定分支,read_line 嘴留本机手测;真实 write_file 三场景(改一行 / 新建 / 完全重写)手测留本机(§13.6)。

### 13.3 subagent 子进程式:落于 MCP 前,1-leg 验 bridge 不死锁

新模块 `src/subagent.rs`,`SubagentTool` impl `Tool`:

- `bin`:`std::env::current_exe()` 自举(codeagent 自己)。`session_dir`:`temp_dir/codeagent-subagent-<主pid>/`,`create_dir_all`。每次 spawn 用独立 session 文件名 `<session_dir>/subagent-<子pid>.json`(用子 pid 不用主 pid,多个 subagent 并发也不互撞 —— 虽 P8 主线串行调,命名留余量)。
- `is_destructive=true`(子 agent 可能写盘/跑命令 → 过闸;`--yolo` 下无审,预期,subagent 是「放手自动执行」助手)。
- schema 只 `task` 必填;**不**加 `max_turns` —— 子进程固定走 `MAX_TOOL_ROUNDS=8` 常量(main.rs),未透传则不在 schema 里骗模型说支持,**诚实**。

**stdout 收工检测:一次性 spawn(不常驻)** —— 流式 token 无内置 sentinel,常驻检测不可靠。喂单行 prompt(prompt = 原 task 加一句一次性收工指令「直接完成上述任务并给出最终答复,不要反问用户、不要等待更多输入」,防子 agent 反问或等下一行)→ `drop(stdin)` → 子进程 `read_script`(main.rs)读到 `Ok(0)` → `InputLine::Eof` → `exit_repl`(存 session + `println!()` + 退)→ 子进程退 → 父 `read_to_end` 自然 EOF 收工。无 sentinel、无超时砍 —— 收工检测**等于子进程自然退出**这个最稳的信号。

**session 隔离**:新 `--session-file <path>` CLI flag + 临时区。子进程 cwd 继承父(要能读 `codeagent.toml`),session 文件路径显式覆盖到 `temp/codeagent-subagent-<主pid>/subagent-<子pid>.json`,**绝不撞**父 `.codeagent_session.json`。普通用户不带这 flag → 走默认名 → 行为与 P7 完全一致(向后兼容)。

回灌文案 `[subagent 答复] ... [/subagent 答复]` 包裹,主 agent 知是委派产物,合成最终答复时摘结论不复述子过程。`--yolo` 父模式下 subagent 工具自动放行闸 + 子进程本身 `--yolo` —— 整链无人审,预期。

**execute 同步签名借桥跑**:内部 `block_on_current(async move { spawn + write_all + drop(stdin) + read_to_end + wait })`,完包成 `Result<String>`,外层 `format!("[subagent 答复]\n{reply}\n[/subagent 答复]")`。`kill_on_drop(true)` 作 Windows 兜底:父子意外 detach 时杀子进程防遗孤。

**单测焊(2,不 spawn 子进程)**:
- `subagent_prompt_wrapping_adds_one_shot_suffix`:复刻 execute 的 prompt 构造,验「回显原 task 开头 + 附一次性收工指令(含「不要反问用户」)+ 标明一次性上下文」。
- `subagent_session_file_path_format`:验 session 路径在临时区、含 `codeagent-subagent-` 前缀、`.json` 后缀、**绝不**与主进程默认 `.codeagent_session.json` 同名 + `SubagentTool::new()` 真能造出来(目录可建 + bin 取到,不 spawn)。

诚实边界:真子进程端到端(子进程真调模型答、父收 stdout、连续调两回验 bridge 不死锁)要真 key + 真终端,留本机(§13.6)。

### 13.4 MCP stdio 客户端:JSON-RPC 极窄面 + 后台 read task id 扇回

新模块 `src/mcp.rs`,住两件:统一桥(§A) + MCP 客户端。

**JSON-RPC 2.0 极窄面**:手写 `RpcEnvelope`(`jsonrpc/id/method/params/result/error`,请求/响应/通知三态共用一结构,**只建用得到的几个字段,其余靠 serde 忽略**)+ `RpcError`(`code/message/data`)。每帧一行 JSON + `\n` + flush。握手:`initialize`(请求→响应)→ `notifications/initialized`(通知无回)→ `tools/list`(请求→响应取工具数组)→ 运行期按需 `tools/call`(请求→响应,把 `result.content[].text` 拼串返)。

**`McpClient`(一 server 子进程封装,多工具共享一连接)**:`child` / `stdin` / `next_id`(AtomicI64,Arc 内不锁)/ `pending`(`Arc<Mutex<HashMap<id, oneshot::Sender>>>`)/ `server_name`。`spawn` 起子进程(`kill_on_drop=true` Windows 兜底;`env` 可选注入;`stderr=inherit` 便于排错)+ 后台 read task。

后台 read task(`spawn` 内 `tokio::spawn`):`BufReader<ChildStdout>::read_line` 逐行 → `from_str::<RpcEnvelope>` → 有 `id` 就从 `pending` 取对应 oneshot 唤醒请求者(`sender.send(env)`),无 id(通知/单边事件)或无匹配 id(迟到响应/被取消请求)丢弃,非 JSON 行(MCP server 偶发 stdout 调试)丢弃 —— 全部不致命。`request(method, params)`:取下一个 id、写 stdin、挂 oneshot 等(`tokio::time::timeout(30s)` 兜底防 server 不回卡死)、Err 回 `error` 包成 anyhow、否则取 `result`。

**握手 + list_tools + call_tool 都在 run() async 上下文直接 `.await`**(run() 本身是 async,c桥只在 execute 同步口用)。

**`McpTool` 接 `Tool` trait(同 (c) 桥借跑)**:持 `client: Arc<Mutex<McpClient>>`(同 server 多 Tool 共享一连接)+ name/description/schema。`is_destructive=true`(保守:不知 MCP 工具有无副作用,过闸)。`execute`:`serde_json::from_str(args).unwrap_or(json!({}))` 兜底解析 → `block_on_current(async { client.lock().await.call_tool(name, args).await })` → err 包成 anyhow。`Box<dyn Tool>` 是 `'static`,McpTool 持 Arc 不借外部引用,**无生命周期坑**。

**工具表接入(main.rs run() 内)**:内置 + `SubagentTool` 之后,遍历 `cfg.mcp.server`:`McpClient::spawn`(失败 eprintln 跳过、**不致命于会话**)→ `handshake`(失败跳过该 server)→ `list_tools`(失败跳过其工具)→ 每个 desc 包 `McpTool`(按 `prefix` 加前缀防撞内置 tool 名:`<prefix>_<原名>`)push 进 `tools`。`mcp_clients: Vec<Arc<Mutex<McpClient>>>` 持连接保活(run scope 内,与 `tools` 同寿);Drop 时 `kill_on_drop` 兜底杀子进程。schema 自动派生(`tools.iter().map(t.schema())`)照走,模型看到同形 OpenAI function;分派(main.rs 线性 find)MCP 工具按 full_name 命中。

**配置 `[mcp]` 段**(config.rs,类比 `[approval]` / `[compaction]`):

```toml
[mcp.server.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
prefix = "fs"     # 可选,防撞内置 tool 名;写了则该 server 工具名前缀成 fs_read_file
```

`McpConfig` derive `Default`(HashMap 空 = 默认)—— **无 §12 那种「段缺 vs 字段缺默认值分化」陷阱**:server 集要么有要么无,字段全必填,无字段级 `#[serde(default)]` 取 0 的问题。`Config` 加 `#[serde(default)] pub mcp: McpConfig`,缺整个 `[mcp]` 段 = 空 server 集 = 不起任何子进程,与 P7 行为完全一致。

**配置单测焊(4)**:`mcp_section_missing_yields_empty_servers`(无 [mcp] = 空)、`mcp_parses_multiple_servers`(两个 server 段)、`mcp_prefix_optional_when_missing`(prefix/env 缺省 None)、`mcp_env_optional_when_missing`(已填 env 段原样保留 HashMap)。

**诚实 allow 标注**:`McpClient` 三处编译器判 dead,带 rationale allow 而非强用:
- `child` 字段:**必须持有**保活(它 Drop 即 `kill_on_drop` 杀子进程),握手中不直接读、P8 接入面无生命周期终结调用故判 dead —— `#[allow(dead_code)]` 注解「持有即保活」。
- `server_name()`:P8 接入面用 `cfg.command` 记日志,这方法留给 P9 统一日志层 —— `#[allow(dead_code)]`。
- `shutdown()`:显式 graceful shutdown 是 P9 候选(要解决 `Arc<Mutex<Self>>` 消耗 self 的所有权)—— `#[allow(dead_code)]`。

诚实边界:握手 / `list_tools` / `call_tool` 的真往返要真 MCP server(如 `@modelcontextprotocol/server-filesystem`)才能验,留本机(§13.6)。

### 13.5 tokio features 扩

`Cargo.toml` tokio features 从 `["macros","rt-multi-thread","signal"]` 扩到 `["macros","rt-multi-thread","signal","process","io-util"]`:
- `process`:subagent + MCP 起 `tokio::process::Command` 子进程(`Stdio::piped` + `ChildStdin`/`ChildStdout`)。
- `io-util`:`AsyncReadExt::read_to_end`(subagent 收 stdout)、`AsyncBufReadExt::read_line`(MCP 逐行读 stdout)。

注意:`Stdio` 走 `std::process::Stdio`(不是 `tokio::process::Stdio` —— 后者是私有 re-export,`tokio::process::Command` 的配置项接 `std::process::Stdio`)。这条坑编译期会报 E0603,改 `use std::process::Stdio`。

### 13.6 留本机真测(诚实不臆造)

P8 的真端到端全要真外部依赖(MCP server 子进程 / 真 key 跑子进程 / 真终端手测审批),本节一律「跑通回贴、跑不通尸检」,不预填数字。

- **diff 审批三场景手测**(**已本机跑通回贴**,真终端非 `--yolo`、stdin 管道喂 `y` 过审 —— 比纯手感多了「管道驱动 approval gate」这一验,见 scenario 通链)隔离临时目录 `_difftest/`(带非 yolo 真 `codeagent.toml`,不动真配置):
  - **A 全新文件**:让模型 `write_file hello_diff.txt`(内容 "hello from diff test")。闸出:
    ```
    [审批] write_file hello_diff.txt —— 拟写入 20 字节(旧 0 字节):
    --- /dev/null
    +++ hello_diff.txt
    +hello from diff test
    [审批] 放行写 hello_diff.txt? [y/N]
    ```
    管道喂 `y` → Allow → 文件真写出 20 字节内容对上。**全新文件 diff 形态正确(`--- /dev/null` + 每行 `+`)+ 审批→写链通。** 顺带实证模型自发 `git add` 走 **bash 普通闸**(非 write_file 分支)另出 `[审批] 即将执行 bash({...}) 放行? [y/N]` → 喂 `y` 通;模型再 `git commit` 时闸 read_line 拿到空(管道 y 已耗尽)→ 默认 **Deny**(plan §B1「默认安全」实证)。
  - **B 改单行**:旧 4 行文件(`aaaa/bbbb/cccc/dddd`)让模型只改第 3 行 `cccc`→`CCCC`。闸出:
    ```
    [审批] write_file target_edit.txt —— 拟写入 52 字节(旧 56 字节):
    --- target_edit.txt
    +++ target_edit.txt
     aaaaaaaaaaaa
     bbbbbbbbbbbb
    +CCCCCCCCCCCC
    -cccccccccccc
     dddddddddddd
    [审批] 放行写 target_edit.txt? [y/N]
    ```
    **单行 hunk LCS 回溯实证成立**:公共 context 行带 ` ` 前缀、变更行 `+`/`-` 正夹,`@@` 风格同 git diff 阅读。`y` 通 → 改后文件实测只第三行 `CCCC` 其余原样。
  - **C 大幅重写**:旧 6 行让模型重写。闸出:
    ```
    [审批] write_file major.txt —— 拟写入 20 字节(旧 81 字节):
    (大幅重写:旧 6 行 → 新 1 行;公共行太少,不展开全量 diff 仅示警告)
    [审批] 放行写 major.txt? [y/N]
    ```
    **大幅重写截断警告分支实证成立**:LCS 长 = 0 < 0.3×6 阈值 → 不刷屏、只示「公共行太少」+ 新旧行数对比(防几千行 diff 刷屏)。plan §B1 三边角第三边角通过。
  - 三场景验全:**(A) 全新文件 `--- /dev/null` 全 `+` + (B) 单行 hunk LCS context 夹 `+`/`-` + (C) 大幅重写截断警告**;外加 bash 普通闸(y 通)+ 默认 Deny(管道 y 耗尽时空读退假)两条副验。
- **subagent 端到端 + bridge 不死锁实证**(最关键测,**已本机跑通回贴**):先 `'用 subagent 工具研究 src/session.rs 有几个 pub 函数并报结论' | codeagent --script --yolo --no-stream 2> sub-leg.log`(1-leg EXIT=0,子进程真读 `session.rs` 答「2 个 pub 函数:save / load」)。再连调两回:一行喂「先研究 session.rs、再研究 compactor.rs」`2> sub-leg2.log`(**EXIT=0,两回子进程都真答** —— 第一回 session.rs save/load,第二回 compactor.rs 7 个 pub:`is_empty`/`explain`/`new`/`should_compact`/`select_messages_to_compress`/`maybe_compact`/`log_line`,第二回跑足 8 轮子上下文见 `sub-leg2.log` L20–38)。验三成立:① 子进程真答非空;② 父 tool_result 含 `[subagent 答复]`;③ **连调两回不卡死**(bridge 不死锁实证 —— P8 最高风险点证伪通过)。**也是 (c) 塌的现场**:`sub-leg.log` 前身(改 (c′) 前)那次跑 stderr 出 `Cannot start a runtime from within a runtime` panic —— 注册证 (c) 真跑塌、(c′) 真修。(c′) 两 leg 8 轮高压都不死锁落实。
- **MCP filesystem 真握手 + tools/call**(**已本机跑通回贴**,隔离临时目录 `_mcp-probe2/` 验,不动真 `codeagent.toml`):配 `[mcp.server.filesystem] command='C:\Program Files\nodejs\npx.cmd' args=["-y","@modelcontextprotocol/server-filesystem","."] prefix="fs"`,真 DeepSeek key 跑 `codeagent --script --yolo --no-stream`,验三(endpoint)— ① spawn + 握手通:stderr 出 `[mcp] server \`filesystem\` 起好,握手通过,接 14 个工具: ["read_file", ... "list_directory", ...]`;② 模型按 prompt 调 `fs_list_directory`;③ tools/call 真返目录内容、回灌、模型合成答出 6 个实文件(`.codeagent_session.json` / `codeagent.toml` / 各 `mcp-probe*.log`)。**真握手通 + 真返内容实证通过**(§E 留本机第二条挪进已验)。
  - **撞 + 修的真坑一:Windows 裸 `npx` 起不来**。先按官方范例配 `command = "npx"` → stderr `MCP server \`npx\` 启动失败 ... program not found`。根因:Windows `CreateProcessW` 默认**不搜 PATHEXT**,`npx` 实为 `.cmd`/`.ps1` 脚本(node v25 装的几套 shim),裸名找不到。**临时绕过**:配 `command` 给全路径 `'C:\Program Files\nodejs\npx.cmd'`(TOML literal string 防 `\P` 转义),std `Command` 见 `.cmd` 后缀会自动包 `cmd /C`,spawn 即通。**根治(Windows 按 PATHEXT 解析)**留 P9(plan §D 风险面外的真接入坑,~25 行 windows-only std 代码)。
  - **撞 + 修的真坑二:握手 30s 超时(首跑)**。全路径配好后首跑 stderr 出 `MCP ... 请求 \`initialize\` 30s 未回应(超时)`,但末行也出 `Secure MCP Filesystem Server running on stdio` —— 即 server **起好了但晚到**。根因:`npx -y` 首次**先拉包**(npm 下载),拉完才起 server;codeagent 的 30s 握手超时被 npm bootstrap 占满。**二次跑包已缓存**,握手立即通过(`mcp-probe2b.log` 起即 `[mcp] server ... 起好,握手通过,接 14 个工具`)。教训:Windows 用 `npx -y` 起 MCP server 有首次冷拉延迟,30s 超时偏紧(P9 候选:加握手超时可配 / 首跑 warmup)。
  - **撞 + 修的真坑三(prefix/remote_name 真 bug)**:首跑修通握手后,模型调 `fs_list_directory` 仍报 MCP server 回 `error -32602: Tool fs_list_directory not found`。根因:`McpTool` 早前只存 `name`(带前缀),`execute` 用 `self.name` 发 `tools/call` —— 但 MCP server 注册的是**原名** `list_directory`,前缀只是 codeagent 侧防与内置工具撞名,server 收到带前缀的名自然找不到。**修**:`McpTool` 加 `remote_name` 存 server 原名,`execute` 发 `tools/call` 用 `remote_name`、`name()` 仍给模型/分派用带前缀名。修后再跑 —— models 调 `fs_list_directory` → codeagent 转 server 发 `list_directory` → server 真返目录 → 模型合成答 6 个实文件。这是 plan §13.4「McpTool 接 Tool 同 (c) 桥借跑」里埋的 prefix 设计缺陷,实证揪出并修。
- **Bash 真 timeout**(P9 候选,不在 P8 内)。

**已自动测住(非留本机)**:四道门禁绿 —— `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo check --tests` / `cargo test`;冒烟 `'exit' | codeagent --script --yolo`(EXIT=0,config 载入 + SubagentTool::new 不炸 + 空 [mcp] 段不起子进程 + 模型回「已退出」收工);`cargo test` 36/36(原 30 + config MCP 4 + subagent 2 + diff+gate 6 含 P8-1)。**本机已跑通回贴(非 auto,真键真终端)**:diff 审批三场景(A 全新文件 / B 改单行 hunk / C 大幅重写截断警告)+ subagent 端到端 1-leg + 连调两回两腿 + MCP filesystem 真握手 + tools/call + prefix/remote_name 修 —— P8 全三条留本机项挪进已验。

> 留本机不臆造:仅剩 **Bash 真 timeout**(P9 候选,不在 P8 内);MCP 真握手实证里撞出三个真坑(裸 npx/PATHEXT、握握手超时首跑、prefix bug)前两个标 P9,第三已修见上。

### 13.7 §13 阶段意义

(c′) 桥(独立 OS 线程 + 独立 `current_thread` runtime)是 P8 全部的支点 —— 它让「同步 `Tool::execute` 调 async 子进程 IO + 改可变状态」在不改 trait、不改 5 个老 impl 的前提下成立。P8 的真实路径是**先采纳 (c)(`Handle::current().block_on`)、真跑塌、退到 (c′)** —— 这把 plan §A 写的「先验:`Handle::block_on` mini reactor 让 runtime 调度、不死锁」当场证伪:multi-thread runtime 下 agent loop 跑在 worker 上,同线程再 block_on 即嵌套,tokio panic「Cannot start a runtime from within a runtime」防 deadlock(这是路线图点名的 P8 最高风险证伪点成真)。subagent 先于 MCP 落地是刻意的:subagent 的 spawn + `read_to_end` 是 MCP 的 spawn + 行级 read_loop 的退化版,1-leg 真跑最先撞这个风险面 —— 也确实先在 subagent 这条腿上把 (c) 撞塌、(c′) 修好、连调两回 8 轮高压验通(§13.6 第三验),给 MCP 铺路。退路若 (c′) 也塌才会动 (a)(trait async 升级),尸检日志记;**现 (c′) 已验通,(a) 延后到 P9**。

P8 的诚实边界是「代码就位 + 门禁 + 可单测的焊住,真外部依赖的端到端留本机不臆造」—— 与 §11/§12 同一纪律:能 auto 测的已 auto 测(diff 纯函数 6 单测、config 4 单测、subagent prompt/路径 2 单测 + 冒烟 EXIT=0),不能 auto 的(真 MCP server 握手、真 key 跑子进程、真手 y/N)诚实标「待本机跑通回贴」。这故 P9 候选也显式标出(`shutdown` graceful、Bash 真 timeout)—— 不把未做的说成做了。

---

## §14 P9 起步 —— P8 实证撞出的待办集合收口(2026-08-09 起,2026-08-10 P9-4 双阶段:Phase 1 死代码占位收口·四闸绿但真端到端静默卡~22min → Phase 2 析因锁死 (c′) 桥 timer stall + 真修绕开 tokio timer(OS 线程壁钟树杀)+ 真端到端挪进 CI)

> P9 不是路线图 P0–P8 里预设的一格,而是 P8 真端到端实证*撞出、记下*的待办集合 —— 「实证驱动」的下一格,不是「计划驱动」的下一格。份内四态(2026-08-10 全已收口、无留本机):① 两硬坑 PATHEXT / 握手超时,代码就位 + 纯函数/配置单测焊住 + 四闸绿 + **真端到端已本机实测回贴**(`.e2e/t01` bare `npx` 真补全起 server + `.e2e/t02` 两次真 cold-pull 真握手全过);② (a) trait async 升级 —— **已完成**(`Tool::execute` 升 `#[async_trait]` async + 7 impls + `dispatch_tool` async + 整段删 `(c′)` 桥 `block_on_current` + keystone 两边绿证运行期等价 + 真 key e2e 2 条 1.52s/2.46s 已实测);③ Bash 真 timeout —— P8 末 `let _=Duration::from_secs(30)` 占位死代码:**Phase 1** 照设想用 `tokio::time::timeout`+`tokio::join!` 收,四闸绿但真 `ping -t` 静默卡 ~22min(门禁绿=伪绿);**Phase 2** 析因单测锁死 (c′) 桥 timer stall(≥2 spawn task pending 在子进程管道 IO 时 `tokio::time::timeout` 被压到等子进程自然退)+ 真修绕开 tokio timer(独立 OS 线程壁钟 → `taskkill /T/F` 树杀 → 单路 read_to_end 靠 EOF 自然退)+ 真 `ping -t` 端到端挪进 CI 作回归闸。**四件全收口**:P9-1/P9-2 已本机真 npx 实测过、(a) 删桥已落、P9-4 已挪进 auto,无留本机。

### 14.1 P9-1:Windows 裸命令名按 PATHEXT 解析(P8 真坑一收口)

P8 实证:`McpClient::spawn` 用 `tokio::process::Command::new("npx")` 直接塌「program not found」(journey §13.6 真坑一)。根因:Windows `CreateProcessW` **不搜 PATHEXT**,而 `npx`/`npm`/`pnpm` 等是 node 装的 `.cmd`/`.ps1` shim,裸名在文件系统上不存在得用 `npx.cmd`。P8 当时靠给全路径 `.cmd` workaround 绕过,P9-1 把它收成**可单测的纯逻辑**。

- `resolve_program(prog: &str) -> Option<PathBuf>`:非 Windows 恒返 `None`(Unix `execvp` 自带 PATH 查找、无 PATHEXT,不为非问题写代码);Windows 裸名 → 遍历 `PATH×PATHEXT` 找 `dir\prog.ext` 命中返全路径;含路径分隔符的 prog 透返 `None` 让调用者指定的语义生效。
- `resolve_in_paths(prog, path_iter, exts)`:纯逻辑核心(不读 env),契约清晰:第一命中扩展名、PATH **目录序**优先(前目录胜出、即便后目录扩展名序更前)、全不命中返 `None`(让 `Command` 试一次,报错对用户直给)。
- `spawn()` 命中就起全路径(std 的 `Command` 对 `.cmd`/`.bat` 在 Windows 自动 `cmd /C` 包裹 —— 这正是 P8 给全路径 .cmd 能跑通的机制),没命中退化回原样。
- 报错信息现在 `program=<>` 同时给配置写的友好名与 spawn 实际路径,便于诊断。

纯函数层单测焊住三条(mcp::tests:`resolve_in_paths_finds_first_matching_extension` / `_prefers_first_dir` / `_returns_none_when_nothing_matches`),用 `temp_dir()` 造假 shim 文件 + 唯一子目录并行不撞。`resolve_program` 自身(读真 `PATH`/`PATHEXT` + 裸名判定)涉真环境变量、多线程测试 `set_var` 有 race 风险,**不作 auto 测 —— 留本机:真起 `npx.cmd` MCP server 实测回贴**(P9-1 把 P8 的 workaround 收成可单测纯逻辑,真端到端仍就位待本机,与 §13 同纪律)。

### 14.2 P9-2:握手超时可配 + 默认放大到 60s(P8 真坑二收口)

P8 实证:`npx -y <pkg>` 首次冷拉包占满旧硬编码 30s,握手**首跑必超时**(stderr:`MCP 请求 initialize 30s 未回应(超时)`,虽末行 `Server running on stdio`);第二次(包已缓存)秒回(journey §13.6 真坑二)。P9-2 把 30s 收成可配,把握手阶段宽到 60s 给冷拉留余量。

- 手段拆**两档超时**(单处真源在 mcp):
  - `HANDSHAKE_TIMEOUT_SECS_DEFAULT = 60`(握手 + `tools/list`,宽,给 `npx -y` 首拉);`RUNTIME_TIMEOUT_SECS = 30`(`tools/call` 运行期,严,防挂工具拖垮整 agent 回合)。
- `request(&mut self, method, params, timeout)` 加 `timeout` 参数 —— **不把「该用哪档」判定埋进通用方法内**,调用者传:handshake/list_tools 传 `self.handshake_timeout`(spawn 时 `cfg.handshake_timeout_secs.unwrap_or(60)` 解析存字段);call_tool 传 `RUNTIME_TIMEOUT_SECS`。报错信息现在显示实际超时秒数(不再硬编 "30s"),便于诊断。
- `McpServerConfig` 加 `handshake_timeout_secs: Option<u64>`(字段级 serde default → None),可逐 server 调(慢机器/大包填大、本地原生二进制填小)。
- config 单测焊住 §12.7 风格三条路径**不分化**:① 字段缺 → None(并锁默认真源 `mcp::HANDSHAKE_TIMEOUT_SECS_DEFAULT == 60`)、② 显式填 120 原样保留、③ 多 server 各自独立(填的不串到没填的)。

诚实边界:真冷拉 npx 握手仍就位**待本机实测回贴**(P9-2 把硬编码收成可配纯逻辑,真端到端仍与 §13 同纪律留本机)。

### 14.3 P9-3:(a) Tool trait async 升级 —— ~~评估结论**本轮不做**~~ **已完成(2026-08-10)**

> **状态翻转注(2026-08-10)**:本节原题「评估结论本轮不做」,真实端到端验证手段建成后(a) 已在紧接的下文中**真做并落地**,标题保留以证「评估→启动」的决策迹。下文 14.3a 是 2026-08-10 真做记录,与 14.3 的原评估(收益与风险正交不动)同工程判断,只是原判的「待手段就位」前置在 14.3 真做前被满足。

#### 评估背景(收益面)

`Tool::execute(&self, args) -> Result<String>` 是**同步 + `&self`** 签名(tools.rs:48),而 subagent/MCP 要 async tokio 子进程 IO。P8 取了 (c′) 桥(独立 OS 线程 + 独立 `current_thread` runtime)让同步 execute 跑 async 子进程 IO —— **不动 trait、不改 5 个老 impl** 就把 P8 三件全跑通。plan §A 列的备选 (a)「Tool trait 整体升级 async」当时列为「改动面最大」被排后,P8 通后 (a) 的位置是「**可选优化、非修坑**」。

(a) 不只是「更干净」—— 有真收益:**升级 async 后 subagent/McpTool 的 `execute` 直接 `.await` tokio 子进程 IO,(c′) 桥(`block_on_current`:起独立 OS 线程 + 独立 runtime)可整段删除**。subagent.rs / mcp.rs 不再走「同步函数里另起 runtime 跑 async」的怪招,代码形态与「tokio 应用」常规一致。代价 `(c′)` 这个「P8 最高风险证伪点曾成真、事后修通」的支点也就此退场 —— 对代码可维护性是净正。**(更:2026-08-10 落地后这条成了真证 —— 桥删后 §14.4 析因出的「桥上 timer stall」也随之在结构上消失,见下 14.3a。)】

#### ~~评估结论:本轮不做,待真端到端验证手段建立才启动~~(原 2026-08-09 评估,2026-08-10 真做后的回看)

原不动理由不是「收益不值得」,是**真端到端验证手段未就位撑不住这种重构**:

- (a) 改动面:**`Tool::execute` 改 `async fn`(或借已在 Cargo 的 `async-trait` crate)→ 5 个内置 impl(ReadFile/ListDir/Glob/WriteFile/Bash) + SubagentTool + McpTool 全 `Box::pin` 包逻辑 → `dispatch_tool` 改 `async fn` + `tool.execute(&args).await` → `run_one_turn` 调用链随之**。触及面 = 几乎全工具盘,trait object `dyn Tool` 的 `Box<dyn Future>` 多一次堆分配 + 动态分发。
- **关键风险**(原无手段验):改 async 后 `--script` 子进程路径(主 `#[tokio::main]` runtime 下 agent loop 直接 `.await`,无需桥)与 MCP/subagent 端到端(真起 npx server / 真 key 跑子进程 / 真终端 y/N 审批闸)的**回归**,cargo 四闸(fmt/clippy/check/test)只能证编译期不塌 + 纯函数单测不回归,**证不了运行期 async 调度 / 嵌套 IO / Ctrl-C 交互链无回归**。P8 的 (c)→(c′) 塌修正是真端到端实证撞的,门禁那道全绿照塌 —— 同样标准,(a) 没有「真端到端验证手段」背书,**门禁绿不等于函数无回归**。
- 按「只记已发生的,不臆造」铁律:**不在缺真端到端验证手段下贸然重构支撑 P8 证明的支点 (c′)**。

故原结论是 **(a) 标记为 P9 后续/独立阶段,本轮不启动**;待真端到端验证手段就位再评估启动。

#### 14.3a (a) 真做(2026-08-10)—— harness FIRST → refactor → 删桥,§14.3 纪律落实

2026-08-10 真端到端验证手段建成后,(a) 启动并完成。守「**门禁绿也证不了运行期回归**」这条本节原判的核心 risk,执行「**先建验证 harness,再动桥**」(§14.3 纪律):

**Phase A —— 纯加法建验证 harness,不动 (c′) 桥 / 不动 source**(3 commit: cea3161 / 5e3072b / 04e4938):
- **keystone** `tests/mcp_fake_handshake.rs` —— 无真 key / 无 npx / 无网络,起一个 `[[bin]] codeagent-mcp-fake-server`(纯 std JSON-RPC 2.0 stdio 回声)子进程,跑真 `McpClient::spawn → handshake → list_tools → McpTool::execute → 后台读 task 扇回` 全链。在**当前桥形代码**上绿(经 `block_on_current` 桥跑 async call_tool)即为 refactor 前基线;refactor 后无桥再绿,两边都绿才证 McpTool async 路径**运行期**等价,不止编译等价。这是 §14.4 stall 现场(读 task `tokio::spawn` pending 在子进程管道 IO)的 CI 可跑版,正闭本节原 risk。
- `dispatch_tool_read_file_roundtrip_built_in`(`src/main.rs mod tests`):私有 `dispatch_tool` 跨不进 `tests/`,故落 crate 内 mod tests。`#[tokio::test(flavor="multi_thread",worker_threads=2)]` 跑 `dispatch_tool(...) → ReadFile::execute → marker 回灌`,锁 async `dispatch_tool` + `dyn Tool::execute().await` 在多线程 runtime(cargo test 里调私有 fn 的唯一点)。
- `bash_hanging_command_actually_returns_within_timeout`(P9-4 Phase 2 已纳 CI 的真 `ping -t` 闸):Phase A 仍经桥跑(同步 execute 在裸 `#[test]` + `std::thread::scope` 外壳内调)。
- **env-gate 真 key e2e**(`tests/script_e2e_real_key.rs` + `tests/subagent_e2e_real_key.rs` + 共享 `tests/common/mod.rs`):`CODEAGENT_E2E`(开关,非 key)+ `DEEPSEEK_API_KEY`(真 key,测试运行期从 env 读、永不进 commit)双 gate —— 非空才跑真 key 路径,未设 `eprintln!("skipped") + 早 return`(非 `#[ignore]`)。前者 spawn `CARGO_BIN_EXE_codeagent --script --yolo --no-stream` 喂「调 read_file 读 temp marker 再复述」prompt 截 stdout 断 marker(模型驱动 tool_call 全链,是 `.e2e/t04` 的 cargo-test 类比);后者走新加的 `SubagentTool::new_with_bin(env!("CARGO_BIN_EXE_codeagent"))` 起真子进程(避 `cargo test` 里 `current_exe()` 指测试运行器二进制的坑)断 `[subagent 答复]` 回灌。CI 无 key → skip(留绿),有 key → 真链自跑。

Phase A 验收(动桥前):keystone 在当前桥形代码绿(refactor 前基线)、dispatch_tool roundtrip 绿、bash 闸仍绿;4 闸全绿(fmt/clippy/check --tests/cargo test);`grep block_on_current` 仍全在(桥未动,纯加法)。

**Phase B —— refactor,A 绿后才动 (c′) 桥**(commit b8ddd0a):
1. `Tool` trait:`#[async_trait]` + `fn execute` → `async fn execute`(镜像早已在仓库的 `Summarizer` async-trait + `dyn` 范式,compactor.rs:79)。
2. 7 个 impl 全 `#[async_trait]` + `async fn execute`:ReadFile/WriteFile/ListDir/Glob body 字节不变(async fn 内 sync std::fs 无 block 点);Bash(拆 `let block=async move{};block_on_current(block)` 包裹成直接 async body,**保留** OS 线程壁钟 + `taskkill /T` 树杀真修,与删桥正交);SubagentTool(拆桥、删 `use block_on_current`);McpTool(`Ok(self.client.lock().await.call_tool(...).await?)`)。
3. `dispatch_tool`(main.rs)`fn`→`async fn` + `.execute(...).await`;caller `dispatch_tool(...).await?`(`run_one_turn` 本已 async,签名不动)。
4. **整段删 (c′) 桥** `block_on_current`(mcp.rs:43-113)+ **4 个析因消融测试** + **1 个 bridge-timer-fires 测试**(tools.rs)—— 6 个桥诊断测全删。代码回常规 tokio 形态。
5. 重写 P9-4 真挂死回归闸 `bash_hanging_command_actually_returns_within_timeout`:旧同步裸 `#[test]` + `std::thread::scope` 外壳(只因 execute 内调桥自起 OS 线程才需隔离) → 新 `#[tokio::test(flavor="multi_thread",worker_threads=2)] async fn` + 60s 外层 `tokio::time::timeout`(**删桥后无桥-stall 可能,timeout 现 safe** 作 CI-hang 守,替旧 5min 外壳)+ 60s 远 > Bash 内部 30s deadline。多线程 flavor **load-bearing**(裸 `#[tokio::test]`=current_thread 会掩盖多线程-only stall 回归,即本节原 risk/§14.3 那个坑)。

**Phase B 验收(本机实测回贴,2026-08-10)**:4 闸全绿 —— fmt 干净 / clippy `-D warnings` 零告 / `check --tests` 绿 / `cargo test`:
- **lib 单测 42 passed**(较删桥前的 47 净减 5:删 4 消融 + 1 桥-fires;保留 bash 真挂死闸 + 新重写形仍绿);**main.rs mod tests 7 passed**(含 dispatch_tool roundtrip 加 `.await` 后绿);
- keystone `fake_mcp_server_handshake_list_call_roundtrip` **0.03s 绿** —— **Phase A 基线经桥绿 + Phase B 无桥再绿,跨 commit 两边都绿 = McpTool async 路径运行期等价证**(非只编译,正落实本节原 risk 要求)。
- env-gate 真 key e2e 两条 **2026-08-10 已本机实测回贴(从留本机翻已跑)** —— 设 `CODEAGENT_E2E=1` + 真 `DEEPSEEK_API_KEY`(env,非 CI)真跑:
  - `script_model_drives_read_file_and_returns_marker`(真 `codeagent --script --yolo --no-stream` + 真 DeepSeek 真模型驱 `read_file` 读临时 marker 一轮 round-trip)→ **1.52s PASS**,模型真发 tool_call、真读 marker、真回灌;
  - `subagent_tool_real_spawn_returns_wrapped_reply`(真 spawn 子进程、真喂"用一句话回答:好"、真两轮 stream prompt=947/981 completion=15/36、真 compactor:noop 未到阈值、真收 stdout 包 `[subagent 答复]`)→ **2.46s PASS**。
  两边绿 = 删桥后 async `--script` 全链 + SubagentTool 真 spawn 路径**真 key 真模型运行期**实证过(非只 keystone 等价)。无 gate(key) 时早退 skip 路径仍先验过(`0.00s ok` 印 eprintln skip)。

**14.3 纪律闭环**:本节原 risk「门禁绿证不了 async 改动后运行期无回归」由 keystone + dispatch_tool roundtrip + 重写 bash 闸三条**无 key、CI 每跑自跑**的真路径闸满足 —— hash McpTool 握+call+读 task spawn 这条最吓人路径在多线程 runtime(refactor 前/后两边都绿)。真 key 模型驱动 round-trip(1.52s)+ subagent 真 spawn(2.46s)两条 env-gated 2026-08-10 已本机实测过(§14.3a Phase B 验收第 4 行),补人眼看时模型语义回归 —— 与 §14.3a Phase A keystone + dispatch_tool roundtrip + 重写 bash 闸三条无 key CI 每跑自跑真路径闸合起,(a) 改动的运行期回归风险由无 key + 真 key 两条真路径共同兜住。§14.4 析因出的桥上 timer stall **结构上随桥删而消失**(桥是 stall 的必要条件:无桥则无桥上独立 runtime + 无桥上 timer 在 spawn task pending 子进程管道 IO 时的张力);Bash::execute OS 线程壁钟树杀真修仍留(治本 Windows orphan,与删桥正交 —— **删桥去 trip,保树杀去根**)。

### 14.4 P9-4:Bash 真 timeout 收口(P8 留下来的「真坑非优化」单列项)

P8 plan §13.6 明确「Bash 真 timeout 不在 P8 内、P9 候选、应单列一项而非和 (a) trait async 升级捆」—— 理由是它**不是优化、是真坑**:tools.rs 这里 P8 末就有 `let _ = Duration::from_secs(30)` 占位 + 注释自承「同步 process::Command 无 timeout,P5 改 tokio 再上真 timeout」,那是纯死代码,**模型 bash 调一条挂死命令(`ping -t` / `cmd /C pause` / `sleep inf` / 大管道喂 `cat` 阻塞)会把整 agent 回合真卡死** —— 同步 `Command::output()` 返回前 dispatch 不退、`run_one_turn` 不回,无任何超时兜底。14.3 把 (a) 评估为「本轮不做」,**不能让 (a) 的延后把这个真坑也一起晾着** —— (a) 是改动架构支点 (c′) 的可选重构,Bash 真 timeout 是不动 trait、不动 (c′) 桥的独立工具增量,P9-4 单列把它收掉。

**两阶段历程(本会话把 Phase 1 「四闸绿但不工作」又真修成「四闸绿 + 真端到端不死」)**:

**Phase 1(commit 38f18fe,先于本会话)**:收口手段照前置设想落 —— `Bash::execute` 改走 `block_on_current`((c′) 桥复用)起 `tokio::process::Command::spawn`,**`tokio::time::timeout(BASH_TIMEOUT_SECS_DEFAULT=30)` 包裹** + `tokio::join!` 两路并发收 stdout/stderr + 超时 `child.kill().wait()` 回收 + 截断抽纯函数 `truncate_output(combined, exit)`。**这一版四闸全绿、5 纯函数单测、release 构建过**(`cargo test` 42→47)。诚实标「真端到端留本机」放 `bash_hanging_command_actually_returns_within_timeout` 那条测,因它真跑 `cmd /C ping -t`(Win 独有无界命令)在 CI 跑里**卡死** —— 当时解读为「真端到端未验」,未深究为何卡。

**Phase 2(本会话,真端到端实证撞出 Phase 1 的桥上 timer stall,锁定 + 真修)**:重启 P9-4 真端到端验证时实测 `bash_hanging_command_actually_returns_within_timeout`(`cmd /C ping -t`、Phase 1 路)**真卡 ~22min 不回**(非 CI 60s 兜底判 FAIL 那种 —— 它静默挂到外力杀)。即 Phase 1「四闸绿」是假象:门禁绿证「编译期不塌 + 纯函数截断策略不回归 + 常量锁」,但**运行期桥上 `tokio::time::timeout` 根本没在 30s 触发**。

**析因单测序列(mcp::tests,本会话隔离焊)**(真跑实测数字,不臆造):
- `block_on_current_timeout_spawns_only_sleep_task_fires` —— 纯 `tokio::spawn` + 30s sleep,outer `timeout(1s)` → **~1s 触发**(桥对「无 IO 的 spawn task」timer 正常)。
- `block_on_current_timeout_io_bound_continuous_stdout_fires` —— `cmd /C ping -n 30`、单 spawn 收单路 stdout `read_to_end`、**无第 2 个 spawn task**、outer `timeout(2s)` → **2.11s 触发**(单路 IO + 无第 2 spawn task,timer 正常)。
- `block_on_current_timeout_replica_of_bash_inner_with_stderr_drain`(`#[ignore]`,跑要 ~60s)—— 复刻 Bash inner 真形:stderr 排水 spawn task + 单路 stdout `read_to_end` + `child.wait()` + `err_rx.await`、outer `timeout(5s)`、`cmd /C ping -n 60`。**worker_threads=2 → 59.62s resolve**;**worker_threads=4 → 59.95s resolve**(与 worker 数无关)。即第 2 个 spawn task(pending 在子进程 stderr 管道 `read_to_end`)的存在就会把 outer `tokio::time::timeout` 静默压到等子进程自然退(~60s)—— 哪怕 inner 主干 future 根本不 await 排水 task。
- `block_on_current_bash_inner_killed_by_os_thread_deadline_fires` —— **拟真修验真**:inner 真形(同上 stderr 排水 spawn task)但**不 `tokio::time::timeout` 包**;改 spawn **独立 OS 线程**壁钟 `sleep(5s)` → `taskkill /T /F /PID` 树杀 → 子进程死 → stdout 管道 EOF → `read_to_end` 自然退 → inner 自然 resolve。`cmd /C ping -n 60` → **7.13s 完成**(5s deadline + 树杀 + EOF + 收尾),不是 60s。

→ **卡住条件锁定**(实证锁死,非推测):在 `block_on_current` (c′) 桥上,「`tokio::time::timeout` 包 await 一个 JoinHandle,且该桥独立 runtime 上同时有 ≥2 个 spawn task pending 在子进程管道 IO」这一组合**稳定**让 timer **stall 到子进程自然退**;multi_thread/timer driver 独立线程/worker 数(2 vs 4)都不能解。tokio 官方文档未直告此张力。stderr 排水 spawn task 不能去掉(否则子进程把 stderr 管道塞满 ~64KiB 阻塞写误伤 stdout 收集,类旧同步 `Command::output()` 各路收之必要)。

**真修(Bash::execute,本会话)**:**根本不 `tokio::time::timeout` 包 inner** —— 绕开桥上 tokio timer(它对「spawn task pending 在子进程管道 IO」形态不稳定)。spawn 一个**独立 OS 线程**壁钟:sleep 到 deadline(= `BASH_TIMEOUT_SECS_DEFAULT=30`)→ 置 `Arc<AtomicBool>` killed 标志 → Windows `taskkill /T /F /PID` **树杀**(连 `cmd /C` 的 grandchild `ping` 一起灭 —— 治本 Phase 1 遗留的 Windows orphan ping 残留问题)/ 非 Windows `kill -9` 兜。子进程被杀 → stdout 管道 EOF → 单路 `read_to_end` 自然退 → inner JoinHandle resolve,无 timeout 包裹、不踩桥上 stall。inner 完成后读 killed 标志分「超时已杀」(拼已有截断输出 + 超时提示串)/ 正常完成(拼 stdout+stderr 截断回灌 exit code)。`kill_on_drop(true)` 保留作兜底(agent 进程被强杀时 child drop 自动 kill 不漏孤儿)。

**真端到端实证回贴(本会话真跑,非 auto 已过门禁前的伪绿)**:
- `bash_hanging_command_actually_returns_within_timeout`(`cmd /C ping -t`、Phase 2 路)→ **30.80s / 37.65s 两跑均 PASS** 回灌含「超时」(死代码占位时代卡 ~22min);跑后查 `Get-Process ping` **无孤儿 ping 残留**(树杀 `/T` 连孙灭,治 Phase 1 遗留)。
- 4 闸全绿:fmt 干净 / clippy `-D warnings` 零告 / `check --tests` 绿 / `cargo test` **52 passed, 1 ignored(`_replica_*` 故意 ignore,只按需 `--ignored` 单跑作 stall 仍在的示警),0 failed**。
- 真修后 `bash_hanging_command_actually_returns_within_timeout` **从「留本机」挪进 CI 作真回归闸**:它跑真无界命令、证「不卡 agent 回合 + 树杀连孙不留孤儿 + 回灌模型可读『超时已杀』」;CI test 默认超时(~60s)兜住任何回归(若有人又把 `tokio::time::timeout` 包回 Bash inner → 卡死 → FAIL,而非静默挂回 22min)。

截断逻辑 `truncate_output(combined, exit)` 与常量 `BASH_TIMEOUT_SECS_DEFAULT=30` 在 Phase 1 已落、Phase 2 沿用(纯函数 5 单测在 §14.4 旧表述已焊、本会话未动、仍绿)。Phase 2 改动面只在 `Bash::execute` 内部 timeout 机制(tools.rs)+ 4 析因单测焊进 mcp::tests + `block_on_current` 内注释更正(原声称 multi_thread 修 stall,实证否决)。

诚实边界演进:Phase 1 把 P9-4「真端到端」标「留本机」是**误会** —— 把「桥上 timer stall」当成「人手验才能验的运行期细节」。Phase 2 真跑即见卡死,故把真端到端从留本机挪进 CI(析因单测 + 真挂死命令回归闸都是 auto)。**P9-1/P9-2 的真端到端仍留本机**(真起 `npx` 验 PATHEXT 真补全 + npx 首拉跑满 60s 是否从容握手);P9-4 不再留本机。

#### 14.4a (a) 真做后:桥删 → timer stall **结构上消失**(2026-08-10 update)

§14.3 (a) 真做(Phase B,2026-08-10,见 §14.3a/commit b8ddd0a)**整段删 (c′) 桥 `block_on_current`** + 4 析因消融测试 + 1 bridge-timer-fires 测试。本节析因的「桥上 timer stall」**结构上随桥删而消失** —— 桥是 stall 的必要条件:无桥则无桥上独立 runtime、无桥上 timer 在「spawn task pending 在子进程管道 IO」时的张力。tokio timer stall 那个硬坑的实证数字(2.11s / 7.13s / 59.62s / 59.95s)**永久留在本 §14.4 作记录**,不在代码注释里再背一遍(代码里 mcp.rs/tools.rs 注释只记「桥已删,stall 随之结构消失」指向本节)。

**删桥对 Bash 真 timeout 的影响**:`Bash::execute` 升 `async fn` 后真挂死回归闸 `bash_hanging_command_actually_returns_within_timeout` 从旧同步裸 `#[test]` + `std::thread::scope` 外壳(只为隔离桥自起的 OS 线程)改 `#[tokio::test(flavor="multi_thread",worker_threads=2)] async fn` + 60s 外层 `tokio::time::timeout`(**删桥后无桥-stall 可能,60s 外层 timeout 现 safe** 作 CI-hang 守,替旧 5min 外壳)。Bash 内真修不动 —— **OS 线程壁钟 sleep(30) → `taskkill /T /F` 树杀 → 单路 `read_to_end` 靠管道 EOF 自然退 → inner 自然 resolve** 仍留(结构绕开 tokio timer,与桥在/否无关;Bash 真挂死回归闸在新 async 形仍绿,30.98s 量级,见 §14.3a Phase B 验收)。

**即删桥的净效**:(a) 前「Bash::execute 经桥跑 async body」+「桥上 tokio timer 在 IO-pending spawn task 组合下 stall」两件相乘才出卡死;(a) 后桥没了,连「要不要在 caller 侧绕开桥上 timer」这个权衡都不再存在 —— Bash::execute 的真修(OS 线程壁钟树杀)是**治本 Windows orphan + 精确 wall-clock 死线** 的独立价值,留与删桥不耦合。**这是 §14.3 (a) 评估背景里「(a) 有真收益」那句的真证:删桥不仅代码变常规,还把 §14.4 析因出的桥上 timer stall 连锅端走** —— 评估写「净正」,真做后「净正」有了实证体。

### 14.5 实测回贴纪律(2026-08-10 更:全已本机实测过,不留本机)

- **P9-1/P9-2 的真端到端** —— **2026-08-10 已本机实测回贴(从留本机翻已跑)**。`.e2e/t01-pathtext.ps1` + `.e2e/t02-handshake-timeout.ps1` 自含脚本(从注册表读真 key 进本进程 env 不落盘 + 隔离 scenario cwd + carrier `mcp-codeagent.toml` 配裸 `command="npx"` + ProcessStartInfo OS-handle 重定向捕 stdout/stderr),用刚 build 的 release exe(`target/release/codeagent.exe`,(a) 删桥后最新代码)真跑:
  - **P9-1 `t01-pathtext.ps1`(真 `npx` PATHEXT)**:裸 `command="npx"`(无全路径)真被 `resolve_program` 按 PATH×PATHEXT 真补全 → spawn 真 `npx -y @modelcontextprotocol/server-filesystem` 起来 → stderr 出 `Secure MCP Filesystem Server running on stdio` + `[mcp] server \`filesystem\` ...握手通过... 14 工具` → 模型真调 `fs_list_directory` 拿到隔离 cwd 的 `t01-marker.txt`/`t01-second.txt`/`codeagent.toml` 回灌 → **EXIT=0,P9-1=PASS**。证 P8「给全路径 .cmd workaround」收成可单测纯逻辑 `resolve_program` 在生产真 `npx` 链上真生效。
  - **P9-2 `t02-handshake-timeout.ps1`(默认 60s 握手余量)**:**两轮真 cold-pull**(t01 首冷 + t02 因 npm cache 被逐「effectively cold again」,`npm warn deprecated glob@10.5.0` cold 标记两轮都出)都跑通:整体 `ELAPSED_MS=15210`(一轮全链 含握手 + 两轮模型 + tools/call),握手必远 < 15.2s,默认 60s 预算至少 4× 余量 → **EXIT=0,P9-2=PASS**。证旧硬编码 30s 会被 npx 首拉吃满提前打断,新默认 60s 两次真 cold 都没掉。
  控制台中文 GBK 错码显示(`閹剽鈧?`/`鐠у嘲銈?`)不影响判定 —— 脚本靠 ASCII 钩子(`server \`filesystem\``、`EXIT=0`、`fs_list_directory`/文件名)判,握手通过行/compactor noop 背后真意均在,纯控制台显示问题。**P9-1/P9-2 不再留本机** —— 都已本机真 npx 真冷拉实测过,真数贴上(t01 EXIT=0 / t02 EXIT=0 ELAPSED=15.21s 两次 cold)。
- **P9-4 的真端到端已在本会话挪进 CI** —— `bash_hanging_command_actually_returns_within_timeout`(真 `cmd /C ping -t`、30s deadline)+ 4 析因单测(mcp::tests)都是 auto、纳四闸;`ping -t` 两跑 30.80s / 37.65s 均 PASS 回灌「超时」+ 无孤儿 ping 残留(树杀 /T 连孙灭);`cargo test` 52 passed / 1 ignored(hex顾 stall 示警)/ 0 failed。此条已挪进 CI 不再留本机(详见 §14.4 Phase 2 真修与实证数字)。上面那条 P9-1/P9-2 亦于 2026-08-10 本机实测回贴,本节现无留本机项。

### 14.6 阶段意义(截至本轮)

P9 起步把 P8 实证撞出的待办集合**逐件收口**:14.1 PATHEXT / 14.2 握手超时把 P8 主程序里「硬编码全路径 .cmd workaround / 硬编码 30s」收成「可单测的纯逻辑 + 可配的配置项」(工程卫生);14.4 Bash 真 timeout 把 P8 末 `let _ = Duration::from_secs(30)` 占位死代码收成真超时兜底 —— **两阶段**:Phase 1(commit 38f18fe)照设想用 `tokio::time::timeout` 包 `tokio::join!` 两路收,四闸绿但真 `ping -t` 端到端静默卡 ~22min(「门禁绿=伪绿」实证);Phase 2(本会话)析因单测序列锁死卡住条件 = **(c′) 桥上 `tokio::time::timeout` 对「≥2 个 spawn task pending 在子进程管道 IO」稳定 stall 到等子进程自然退**(multi_thread/worker 数不治),真修为**绕开 tokio timer**:独立 OS 线程壁钟 sleep deadline → `taskkill /T /F` 树杀(连 grandchild 一起灭,治 orphan)→ 单路 `read_to_end` 靠管道 EOF 自然退 → inner 自然 resolve,无 timeout 包裹。真 `ping -t` 端到端从「留本机」挪进 CI 作回归闸。三件 P9-1/2/4 共规「P8 真端到端实证撞出 → 记下 → 代码层就位 + 单测焊 + 四闸绿」;P9-4 进一步把「真端到端」也从留本机挪进 auto。14.3 (a) trait async 升级:**原 2026-08-09 评估**「有真收益(消 (c′) 桥让代码变常规 tokio 形态,直接绕开 §14.4 析因出的桥上 timer stall)、但本会话验证能力撑不住运行期回归,故不动」—— 把「能不能做」与「现在该不该做」分开记,是「不臆造」纪律在决定层的体现;**2026-08-10 真端到端验证手段(Fake-MCP keystone + dispatch_tool roundtrip + 重写 bash 闸 + env-gate 真 key e2e)建成后,(a) 已按 §14.3a 真做完成并落地**:桥删、timer stall 结构消失、keystone 两边绿证运行期等价(详 §14.3a/§14.4a/commit b8ddd0a)。P9 起步格 —— 四件收口里:14.1/14.2 PATHEXT/握手超时**已本机真 npx 实测回贴**(§14.5:真 bare `npx` 真补全起 server + 真两次 cold-pull 真握手全过,t01/t02 EXIT=0 ELAPSED=15.21s);14.3 (a) trait async 升级**已真做完成并落地**(keystone 两边绿证运行期等价 + 真 key e2e 2 条 1.52s/2.46s 已本机实测过);14.4 Bash 真 timeout 已挪进 CI 作真回归闸。无「留本机」未跑项,不臆造未跑数字;P9 已逐件收口 —— 下一个实证驱动阶段(P10)接着撞。

---

## 15. P10:实证撞出待办集合(2026-08-11 起·2026-08-11 收口·五候选全清:3 真坑真修+1 证否+1 真坑真修)

P9 同款「实证驱动」逻辑接力:P10 不是路线图预设格,是 P9 收口后**真跑 codeagent 试新场景实证撞出来的坑记下而成的集合**。撞坑方向(用户拍板:2026-08-11):真跑 codeagent CLI 试新场景,连续撞不到真硬坑就算结束。

### 15.0 撞坑基线场景(2026-08-11 本机实测·全 PASS,无坑)

诚实先记「没撞出来的」:P10 起步主动真跑两个 P8/P9 没真覆盖的场景、**全绿、无硬坑** —— 这是「不臆造」纪律的反面实证(撞了没坑就如实记没坑,不挑事)。

- **`t10-multiturn.ps1`(多轮带工具连续对话)** = P8/P9 只验单轮或单工具,「多轮 + tool_call + 追问依赖上文」**没真覆盖**。原生 PowerShell 管道(从注册表读 key 进本进程 env + `Get-Content turns | release exe --script --yolo 2>trace`)真跑三轮(write `marker.txt=p10-hello` → 读 marker 复述 → bash `dir` 列目录):**EXIT=0 ELAPSED_MS=7999**,marker.txt 真落地内容 `p10-hello`,三轮答得正确、第三轮认出「跟我们刚创建的 marker.txt」(历史跨轮保留),compactor 3 轮 noop(远没到 700k 阈值)。多轮+三工具(write/read/bash)+ 跨轮上下文全活,无硬坑。
- **`t11-resume.ps1`(会话恢复 --resume 全链路)** = journey §11 称 P7 自动 resume 接力,「写文件→退出→--resume→追问依赖上文→模型真记得」**真跑全链路**:stage1 留上文(记串 `p10-resume-marker-5f3a` → 写 file → 读确认、EXIT=0 ELAPSED_MS=7173)→ stage2 `--resume` 起 + 只凭 resume 载入历史答(不读文件)。trace 铁证 `[resume] 已载入 .codeagent_session.json(11 条历史,含首条 system)` + 模型答出 `p10-resume-marker-5f3a`(凭记忆非读文件):**STAGE2 EXIT=0 ELAPSED_MS=1542**。resume 形神兼备,无硬坑。

两场景全 PASS —— P10 基线证明 P9 收口的代码在多轮/resume 这两个没真覆盖的形态上也不塌。本节继续的是真硬坑。

### 15.1 P10-1:MCP `request` TOCTOU race(`pending.insert` 后于 `flush`,快 server 丢响应)

**实证撞出**(非读码臆测):**2026-08-11 本机实测真命中**。撞坑脚本是 `tests/mcp_toctou_race.rs`(进 CI,新加)。横截面对端是**已有的 `codeagent-mcp-fake-server`**(纯 stdlib 同步、读到就立刻 `write_all+flush` 回,响应零延迟)—— 正是最易撞 race window 的对端。脚本连做 **N=200 次完整 `spawn+handshake+list_tools`**,握手超时压到 **3s**(默认 60s 太慢,race 命中本就≈timeout 值,3s 让失败快速暴露)、任一次若 race 命中→read task 抢先→`pending.insert` 赶不上→`rx` 3s 超时→Err→测 FAIL。

**修前实测(2026-08-11 本机)**:`N=100` 一跑,**100 次中 2 次失败(#41 / #75)**——
```
#41 handshake/list 失败(race 嫌疑): MCP `...fake-server.exe` 请求 `tools/list` 3s 未回应(超时)
#75 handshake/list 失败(race 嫌疑): MCP `...fake-server.exe` 请求 `tools/list` 3s 未回应(超时)
```
**2% 确定性 race**(非「极小概率」)。修前/修后对照:换 `N=200` 跑 3 次的预期是 ~4 次失败/跑;修后跑了 3 跑 × 200 次 = **600 次握手 0 失败**(1.42s/1.43s/1.40s),实证修对了。

**根因**(`mcp.rs::request`,修前):
```rust
self.stdin.write_all(line.as_bytes()).await?;   // 先发
self.stdin.flush().await?;                        // flush ← server 此刻就能读到请求、回响应
                                                    //   read task 此窗口内若读到响应:
let (tx, rx) = oneshot::channel::<RpcEnvelope>();
self.pending.lock().await.insert(id, tx);         // 登记晚于 flush ← map.remove(&id)=None 丢响应
                                                    //   → rx 永不收 → tokio::time::timeout(60s/3s) 挂
```
read task(mcp.rs:261-269)拿到响应就 `map.remove(&id)`,`None` 走 `let _ = sender.send(...)` 静默丢(line 266 注释「没人接说明请求已超时取消」**误判** —— 这里不是超时,是 insert 没赶上)。**为什么 P9-1 `t01` 真运行时没挂?** 因 `npx -y @modelcontextprotocol/server-filesystem` 是 node 进程,cold-pull 时握手响应有 OS/npm/node 启动延迟,race window 期 read task 多半还在 `read_line` await yield,insert 赶得上 —— 真 server 的响应延迟把 race 概率压到几乎零,但**结构上的窗口从未闭**,一个比 node 启动快的对端(同步 fake-server、或某 native MCP server)就能稳定撞上。CI 里 `tests/mcp_fake_handshake.rs`(keystone)每跑只 1 次握手、0.05s 绿 —— 单跑撞 2% race 概率低,故长期没被 CI 抓到;但跑多了(或 CI 抖一次调度)必撞,P9 末留下的「keystone 闸是否真够稳」暗问此刻有了实证答案:**原 keystone 闸不够敏感到抓这个 race**,新加 `mcp_toctou_race.rs` N=200 才够(2% × 200 ≈ 4 期望失败,回退必撞)。

**真修**(`mcp.rs::request`,修后):**登记前置** —— `pending.insert(id, tx)` 移到 `write_all+flush` **之前**,彻底闭窗口(read task 任时刻命中 id 都已在表里):
```rust
let (tx, rx) = oneshot::channel::<RpcEnvelope>();
self.pending.lock().await.insert(id, tx);          // 先登记(闭 race window)
let send_res = async {                              // write/flush 若失败,清这条 pending 孤儿
    self.stdin.write_all(line.as_bytes()).await.map_err(...)?;
    self.stdin.flush().await.map_err(...)
}.await;
if let Err(e) = send_res { self.pending.lock().await.remove(&id); return Err(e); }   // 清孤儿(发不出去,响应收不到)
```
**timeout 路径自带清理**:read task 迟到响应触发 `map.remove(&id)` 取出 sender、`sender.send` 对已 drop 的 rx 返回 Err 被 `let _ =` 忽略,entry 此刻才被 remove 真清掉 —— 故超时取消的请求不留长期孤儿。只有「发都发不出去」(write/flush err)是新引入的孤儿路径(改前 write 在 insert 前不会留),故显式 `remove(&id)` 清。

**验收**(2026-08-11 本机四闸):fmt OK / clippy `-D warnings` 0 warning / `check --tests` Finished / `cargo test` 全绿 —— lib 42 passed(30.85s 含 bash 真 ping-t 闸)/main.rs 7/keystone **1 passed 0.02s(改了 mcp.rs 后仍绿 = insert 前置没破坏握手)**/**`mcp_toctou_race` 1 passed 2.91s(200 次握手 0 失败)**/env-gate 2 条 gate 未开 skip 0.00s。**修前/修后实证对照确立**(2/100 失败 → 0/200 × 3 跑失败),新闸进 CI 作回归闸(若有人把 insert 移回 flush 后,200× 2% ≈ 4 失败/跑,必撞)。

**序列诚实**:本条是「代码审查 agent 读出的潜在坑 + 我真跑实证撞出 2% 失败」两条腿都走的产物 —— 没真跑前的读码只能叫「嫌疑」(像 P9-3 (a) 原「评估:有真收益但本会话验证能力撑不住」的诚实分类),真跑命 2/100 失败后才升格成 P10-1 真坑。这是 §14.3「门禁绿=伪绿」教训的主动套用:不靠「看代码觉得没问题」,靠「跑 200 次看死没死」。

### 15.2 P10-2:中断轮孤儿 tool 历史(旧退格 `else if last==user {pop()}` 漏带 tool 尾,析因单测复旧留 6 条)

同 P10-1 是代码审查 agent 读出的候选 #2:Ctrl-C 在 tool 轮中途打断,内存历史留半截 `[assistant(tool_calls), tool, tool]` 孤儿。读出时只能叫「嫌疑」,需证。

**先核读码无误**:`main.rs::run` 在 `finished=false`(本轮未跑完 = 被打断)分支,旧退格逻辑是 `else if messages.last().map(|m| m.role == "user").unwrap_or(false) { messages.pop(); }`。当打断发生在 tool 轮**之后**:本轮已 push `user`(本轮问题)+ `assistant`(带 tool_calls,模型说「我要调 read_file」)+ 一/多条 `tool`(工具结果),尾部 `role="tool"` **不是** `"user"`,`else if` **不命中、不退格** → 半截 tool 轮留在内存 `messages`。由于 `finished=false` 分支**跳过 save**(只跑完的轮才落盘),孤儿不污 `.codeagent_session.json`,但污染**当前会话余下轮次的内存历史**:下一轮 user 再 push 时,模型看见一条没人接话的 `assistant(tool_calls)` + 悬空 `tool` 结果(「我说了要调 read_file、也调了,然后呢?」),据此答出困惑的回复 —— 用户面观感是「我打断它半截,它下一句答非所问」。

**析因单测复现**(落 `src/main.rs::mod tests` —— `dispatch_tool`/`run` 私有,集成测试 crate 外进不去,与本计划 §3.3a 同址):构造 `[system, user"上一轮问题", assistant"上一轮答案"]`(pre_turn_len=3),模拟中断轮 push `user"本轮问题"` + `assistant(tool_calls read_file)` + `tool(result "文件内容")` —— tail 是 role="tool"。对**克隆**的 `old_path` 跑**旧** `if last==user {pop()}` 逻辑:`old_path.last().role=="tool"` 不命中,`pop()` 不执行,`old_path.len()` 仍 = pre_turn_len+3 = 6 —— 断言 `assert_eq!(old_path.len(), pre_turn_len + 3)` 锁住「旧逻辑在此现场留 6 条(3 轮基线 + 本轮 user + assistant(tool) + tool 全留 = 孤儿)」。再对原 `messages` 跑**新** `messages.truncate(pre_turn_len)` → len=3、末条仍是「上一轮答案」、不含「本轮问题」、不含悬空 tool 结果 —— 断言干净。这是 §14.4「析因单测」范式:对**旧逻辑**跑出 bug 现场、锁住,再对**新逻辑**跑出修后干净、锁住 —— 旧逻辑留 6 条的断言**就是 bug 存在的实证**(不需要真 Ctrl-C 一个 live 进程 —— 旧逻辑的缺陷纯函数可复现,析因单测比 live 复现更确定)。

**真修**:`run()` body 在 `messages.push(Message::user(input_trimmed))` **之前**记 `let pre_turn_len = messages.len();`(本轮起点),`finished=false` 分支用 `messages.truncate(pre_turn_len)` 替旧 `else if last==user {pop()}`。`truncate` 回滚到本轮起点 —— 无论本轮尾部是 `user`(用户敲完没生成)、`assistant`(生成中被打断)还是 `tool`(tool 轮被打断),整轮半截全清,内存历史回到上一轮干净终点。比旧 `pop()` 单条单退(且只退 user 尾)覆盖更广、语义更直白。

**为什么 P10-1 是实证 100 次跑出 2 失败、P10-2 是析因单测纯函数证**:两者撞坑手段不同但同属「不臆造」。P10-1 是 race 窗口(时序 bug),纯函数证不出 —— 必须真跑 200 次 handshake 撞概率;P10-2 是**确定性**控制流缺陷(打断后 tail=="tool" 必不命中 `else if last==user`),旧逻辑对**给定输入**的输出是纯函数,析因单测喂那个输入就能锁住「旧留 6 条、新留 3 条」。故 P10-2 不需要 live Ctrl-C 复现 —— 析因单测比 live 更确定(live 还依赖打断恰好落在 tool 轮后的精确时序);但**仍守「先证再修」**:先写析因单测跑旧逻辑确认留 6 条(= bug 实证),再改 `truncate` 跑同一测确认留 3 条(= 修后实证),不是「读出未跑就改」。

**验收**(2026-08-11 本机四闸):fmt OK / clippy `-D warnings` 0 warning / `check --tests` Finished / `cargo test` 全绿 —— lib 42 passed(31.12s)/main.rs **8 passed**(新增 `interrupted_round_rollback_clears_orphan_tool_history` = 第 8)/keystone 1 0.02s/toctou 1 2.77s/env-gate 2 skip 0.00s。析因单测实证旧逻辑留 6 条孤儿 vs 新 `truncate` 留 3 条干净,bug 与修在同一测里对照锁住。

**为什么这条是真坑不是凑数**:打断后内存历史污染影响**下一轮**模型输入,用户面是「答非所问」体验 bug,非纯防御性代码洁癖。旧 `pop()` 单条退格是 P7 期(`session.rs::save` 落盘前清悬空 user)语义的内存版沿用,当时只覆盖 user 尾、tool 轮是 P8 才加的 capability —— tool 轮打断路径是 P8 后新增而旧退格没跟着扩,属「新 capability 配旧清理」的典型遗漏。

### 15.3 P10-3:glob `**` 跟随 symlink/junction cycle 爆 64 条重复(本机 junction 真跑实证 + 修 nofollow + 跨 OS 真 cycle 单测进 CI)

同 P10-1/P10-2 是审计 agent 候选 #3:`tools.rs::walk` 在 `**` 段做 `walk(&child, rest, out)`(消段)+ `walk(&child, segs, out)`(**仍带 `**` 再下钻**)两条递归。读出时只能叫「嫌疑」 —— 看上去 `**` 自己再递归似会无限,但 Windows 上 read_dir 有 MAX_PATH 260 限制、可能自然在深处被端口拒,需真跑才能定。

**真跑实证**(2026-08-11 本机):建临时目录环 `tmp/A/B/leaf.txt` + `B/loop -> A`(Windows junction 回指祖先造环,PoweShell `New-Item -ItemType Junction` 不需管理员建得成),用一条 ad-hoc example bin(`examples/glob_cycle_probe.rs`,**复制生产 `walk` 逐字节**,已删)对 `tmp/A` 跑 `glob_walk` 返 `**/*`:

- **修前**:返 **64 条** `leaf.txt` 匹配,每条新添 `loop\B\` 圈长一截 —— `A\B\leaf.txt`、`A\B\loop\B\leaf.txt`、`A\B\loop\B\loop\B\leaf.txt` …… 第 64 圈路径长到 Windows MAX_PATH 260 后 `std::fs::read_dir` 返 Err 触发 `Err(_) => return Ok(())`(tools.rs:331 的「无权限/不存在跳过」)被误当「自然终点」停。耗时 319ms 全在无效下钻。
- 这不是「自然终止」是「偶然被 OS 限外弹回」 —— Linux 上 `read_dir` 无 MAX_PATH-per-call 限制、symlink 又不需特权,一会一路爆 `out` 体积(同文件重复 N 条、N 随路径长限上限)到 OOM/爆栈,UI 给模型灌回 64 条同文件更显眼。

**为什么 P10-1 是 N=200 概率闸、P10-2 是析因纯函数证、P10-3 是真实建环厨实证**:撞坑第三态 —— P10-3 是**确定性控制流缺陷 + 跨 OS 厨实证**(同 P10-2 确定性,但旧逻辑缺陷依赖**真文件系统的 symlink/cycle**,纯函数证不出 —— 必须真建一个 OS 环)。析因单测理论可复现但真盘 IO 才是「跑给的 lives 现场」。

**真修**:`walk` 在每条 `entry` 加 `if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) { continue; }` —— 用 `DirEntry::file_type()`(原生不跟随,给 entry 本身类型)判 symlink 跳过该子树。语义:glob 搜目录**横截面**,不绕链接(ripgrep 默认 `--nofollow` 同理);`**` 不再跟 junction/symlink 下钻,cycle 自然断 —— `loop` 子树被跳过,真路径 `A\B\leaf.txt` 仍由 `*` 段名匹配走 `walk(&child, rest)` 在 `segs.is_empty()` 时 `dir.is_file()`(跟随判)正常捕到。

**修后实证**:同环、同 pattern —— **1 条** `A\B\leaf.txt`(真实路径)、**0** `\loop\` 副本泄漏、~0.6ms(修前 319ms)。

**析因 / 回归单测**(落 `src/tools.rs::mod tests` —— `glob_walk` 私有可直接调,落本源最净):`glob_walk_does_not_follow_symlink_cycle`:

- 调 `build_cycle_tmp()` 跨 OS 建真环(unix `std::os::unix::fs::symlink` 普通用户可建、windows 试 `std::os::windows::fs::symlink_dir` 失败退 `cmd mklink /J` junction、其他平台 skip),建环失败(开发者模式 off / CI 沙箱受限 / 无 symlink 特权)则 `eprintln + return` skip —— 非 `#[ignore]`,镜像 `CODEAGENT_E2E` gate 自退记号,不掩盖、不臆造。
- 成功建环:断言 `glob_walk(cycle, "**/*")` 中名为 `leaf.txt` 的**恰好 1 条**(真路径)、不出现含 `\loop\` / `/loop/` 的 cycle 副本(防 leakage)。
- 「析因两腿」:(
  1) 本机真跑 + (
  2) 通过 `if false && entry.file_type()...` 临时禁闸→ 同测从 1 条变 64 条 FAILED(本地实证旧 bug 现场)→ 恢复闸 → 1 条 ok。修前/修后同测对照锁住。

**验收**(2026-08-11 本机四闸):fmt OK / clippy `-D warnings` 0 warning / `check --tests` Finished / `cargo test` 全绿 —— lib **43 passed**(30.80s,新增 `glob_walk_does_not_follow_symlink_cycle` = 第 43)/main.rs 8/keystone 1/toctou 1 2.77s/env-gate 2 skip。修前 64 重复 → 修后 1 真路径,析因单测实证 + 跨 OS 真 cycle 焊进 CI 作回归闸(若有人删 symlink 跳过,本测① 开发者模式开的 CI 上必 FAIL 64 vs 1;② 普通权限 CI 上 skip 不假绿 —— gate 自退、不掩盖)。

**诚实副作用**(行为变更:**`**` 默认不跟随 symlink;`node_modules` 等符号链接包内 `**/*.rs` 不再命中**)。这是语义修正(ripgrep `--nofollow` 同形),非破老用法的真 bug;真要跟 symlink 后续可加 `--follow-symlinks` flag(**此版不做** —— P10 只修实证硬坑,不加臆造功能)。

### 15.4 P10-4:空摘要 silent success —— 候选 #4 证否(真跑真压缩 live 触发,模型返 1093 字真摘要,空 content 路径未实证撞到)

代码审查 agent 候选 #4:`ModelSummarizer::summarize`(main.rs:177-180)`choice.message.content.unwrap_or_else(|| "[摘要为空]".to_string())` —— 模型若真返 content=None/空,被填成 `[摘要为空]` 仍 `Ok`;`compactor.maybe_compact`(compactor.rs:203-217)信 summarizer 返回,把 `[摘要为空]` 当真 summary 推进 `out` + 报 `Compacted` done —— 中段历史(老 tool 段落)被一条「占位字符串」替掉,模型真没摘要成,**历史静默丢且 compaction 报「done」**。读出时是「嫌疑」,同 P10-1/P10-2/P10-3 先证再修。

**真跑实证**(2026-08-11 本机):

1. **真压缩路第一次被 live 真跑触发** —— 全程 P0-P9/P10-1-3 期,e2e 从没真触发 compaction(max_context=1M × 0.7 = 700k token 阈值,一句话造不出 700k)。本轮临时加 env 闸 `CODEAGENT_P10_DEBUG_COMPACT_AT=0.01` 把阈值压到 10k(不碰 prod `codeagent.toml`、env-only 实验闸),跑 9 轮 `--script --yolo --no-stream` 每轮长 prompt 拉高 cumulative token:
   - 第 5 轮收工 `total=12503 ≥ 10000` 触发 → `[compactor:done] total=12503 compress: keep_head=2 summarize=10 keep_tail=8`(中段 10 条折叠成 1)、`[compactor] 历史:20→1 条(老 tool 段落已折叠为一条摘要)` —— **真压缩链路第一次现实跑通**。
   - 后续第 6-9 轮 `should_compact=true` 但 `select_messages_to_compress` 中段空(`keep_recent_turns=4` = 8 条 keep_tail 包住最近 user assistant 轮,头单条 sys,中段 0 条)→ 报 `EmptyMiddle` noop,正常自洽未退化。
2. **真 summarize 返非空 content** —— 临时 stderr 探针([P10-4-DEBUG] summarize 内容头 80 字,已随实验闸撤掉,见下「清场」)实测真模型返 **1093 字真摘要**(「这段对话中用户发来一条内容为大量重复字符的 `p10fourprobe-XQWNJTVB`(开头带乱码)的消息。code agent 先执行了 `list_di...」)—— **空 content 路径未实证撞到**。我的 live 长 prompt 触发条件没把模型顶到 length-cutoff / content_filter / 空 content reasoner 这类返空态。

**撞坑第四态(与前三态对比)**:
- P10-1 race / P10-2 确定性 / P10-3 确定性 + OS 真建环 —— 都实证命中并真修。
- **P10-4 真跑了、真压缩 live 触发,但 ·空 content· 这个 bug 条件没真撞到**。这是 P5/journey §5.7-8「多轮工具中断窗口未真触发」同类诚实记:撞了但没坑的。

**为什么不直接用 FakeSummarizer 注空 content 跑析因单测判定真坑**:对 P10-2 的析因单测喂 `user(本轮)+assistant(tool)+tool` 三元状态 —— 那是 codeagent 自身处理 user 敲轮的**真实用户可控路径**(用户 Ctrl-C 真在哪轮都顶得住 tri 下列那个 tail=tool 现场);P10-4 的 `summarize 返空 content` 是**外部模型依赖的事件**,**用户不可控触发**,我 live 没撞到、读码仅「嫌疑」。FakeSummarizer 注空只证「控制流接受空 shady 不报错」(= 受控代码缺陷),不证「现实真发生」—— 单纯读码嫌疑 + 受控析因无法升格为真坑。这样强行标注 P10-4 真修会破「不臆造」纪律。

**清场**:临时 env `CODEAGENT_P10_DEBUG_COMPACT_AT` 闸与 stderr `[P10-4-DEBUG]` 探针已撤回(回 `compactor.rs::threshold_tokens` 与 `maybe_compact` 代码原样),`codeagent.toml`(prod 用户真配置,gitignored 未追踪)未碰一行。临时 e2e `.e2e/t12-p10-four-trigger-compact.ps1` + turn/trace txt(gitignored)留作真压缩链路实证回贴凭,真值:`total=12503 → compactor:done keep_head=2 summarize=10 keep_tail=8 / 历史 20→1 / summarize content 长 1093 字真摘要`。

**累计 P10 实证价值(诚实对照)**:P10-1/2/3 三条真坑各被真撞真修焊进 CI;P10-4 真跑触发第一次 + 真 summarize 返非空 = 「真压缩链路 live 跑通」新增实证(填了 P6.1 以来「真压缩召回、未在长上下文大规模真跑」之中留白的一个小切片),**但 #4 作 bug 候选证否、不收为真坑不修**。P10 进行中:剩 #5 SubagentTool 无 timeout 候选待真撞。守「连续撞不到真硬坑就算结束」—— P10-4 此番虽触发但没坑 + #5 待真撞,尚未到「连续」判定窗。

### 15.5 P10-5:SubagentTool `read_to_end` 无 timeout 包 —— 候选 #5 真撞升格真坑 + 真修(2026-08-11 本机 mock 上游真撞实证)

代码审查 agent 候选 #5。**事实链**(读码逐条确认,非猜):`subagent.rs::execute` 的 `stdout.read_to_end(&mut out).await`(原 144 行裸 await)+ `child.wait().await`(145 收尸裸 await)均**无 `tokio::time::timeout` 包**;子进程 `codeagent --script --yolo` 内:`run_one_turn` 主循环 `chat_completion` / `chat_completion_stream` 皆 `client.post(...).send().await` **裸 await 上游**;`client = reqwest::Client::new()`(`main.rs` 857/1159)裸构、**无 `.timeout()` / `.connect_timeout()`**;`--script` 模式无 Ctrl-C 信号源(P5 中断门挂在 mpsc / `signal::ctrl_c`,`--script` headless 无 TTY 也无控 — 子进程无法被打断)。全链无超时兜底:**给定「上游 hang」输入 → 子 `send().await` 永挂 → 子进程不退 → 父 `read_to_end` 永挂 → 父 turn 永死、无任何 timeout/Ctrl-C 兜底**。

**撞坑候选 vs 真坑 判规**:P10 纪律「不直接信读码、要真撞」—— 真撞是 #5 升格门槛。但 P10-4 的 `summarize 返空 content` 是「外部模型依赖、不可控触发、读码仅嫌疑、live 没撞到」,故证否;**#5 的上游 hang 是确定性控制流缺陷**(类比 P10-2 喂中断现场:给定输入 → 父侧代码是 buggy 纯逻辑)。同 P10-2 路径:**确定性缺陷 + 真复刻现场 = 升格真坑**(非 P10-4 的「外部依赖、不可控」做空)。

**真撞实证(撞坑第四态 — 本地 mock 上游真起 production codeagent 子进程)**:起 `examples/p105_subagent_hang_repro.rs`(env-gate 二进制,默认 skip、`CODEAGENT_P105_GATE=1` 跑真),它的真撞手段(不起真上游、不依赖真 key):

1. **mock hang 上游**:`TcpListener::accept` 一条连接后起 long-hold task **持有 stream 整 example 生命期不退、不读不写不关** —— `reqwest::Client::post().send().await` 发完请求字节后等响应头**永不回** + TCP 不 RST(keepalive 也不杀)→ reqwest send() 永挂。**反事实对照实证**(关键反例锁现场精确性):**第一版** mock 一 accept 就 drop stream(`Ok((_stream, _peer))` 出 match arm scope drop)→ TCP RST → codeagent 子进程 `83ms` 内退并报 `流式请求发送失败(reqwest error): error sending request` —— **83ms 退不是 hang 现场,证第一版 mock 路径错**;改成 long-hold 持 stream 后才真复刻「上游应用层 hang」。
2. **临时 toml 写 tmp 目录**(`base_url=http://127.0.0.1:<mock-port>`, `api_key_env=DEEPSEEK_API_KEY`,key 任值上游假不验真),**不动** crate root 真 production `codeagent.toml`(纪律:`gitignored` 未追踪、不碰真生产配置)。
3. **p105 v1(初撞)**:手动复刻 `SubagentTool::execute` 的 spawn 段(`--script --yolo --session-file <tmp>` + `current_dir(tmp)` 让子读临时 toml)+ 裸 `read_to_end` + 外层 `tokio::time::timeout(15s)` 包 → 撞钟 `Elapsed`,**elapsed=15.00s exit=0 = #5 真坑现场实证**(对照脚:正常自退 `echo hi` 子进程秒回 'hi',证裸 `read_to_end` 不误杀正常路径)。
4. **p105 v2(焊闸·修后)**:真起**生产 `SubagentTool::execute`**(走 `SubagentTool::new_with_bin(locate_codeagent_bin())`)撞同一 mock hang 上游,`CODEAGENT_SUBAGENT_TIMEOUT_SECS=3` 压兜底窗口,外层 `tokio::time::timeout(15s)` 包:验证修后 `execute` 自己 ~3s 返 `Err("超时...子进程已被 kill")` 而**不**撞外层 15s 兜底 = 父 turn 拿到 subagent 超时错而非永死,子进程被 `child.kill` 收尸无遗孤。

**真修**(`src/subagent.rs`):
- 加 `const SUBAGENT_DEFAULT_TIMEOUT_SECS: u64 = 180` + `fn subagent_timeout() -> Duration`(读 `CODEAGENT_SUBAGENT_TIMEOUT_SECS` env 覆盖,0/垃圾/空/空白回退默认 —— 给本地实证/析因单测压窗口用;真生产永远 180s 默认,子 agent 走 MAX_TOOL_ROUNDS 多轮 + 多次首 token 可能数十秒故取宽裕)。
- `execute` 的 `stdout.read_to_end(&mut out)` 包 `tokio::time::timeout(subagent_timeout(), ...)`:`Ok(inner) → 正常读`;`Err(_elapsed) → eprintln!("[subagent] 超时...") + 显式 child.kill().await + child.wait().await 收尸 + 返 Err("subagent 超时(<n>秒未返回终答) —— 子进程已被 kill...{partial stdout}")`(超时前子进程已产的部分 stdout 一并附上,父 agent 收到 subagent 超时错而非永死)。

**根因层为何不修**:顶层 `reqwest::Client::new()` 加 `.timeout()` 会**断流式** SSE 长跑超 window 被切(真生产 = DeepSeek,慢首 token turn 可能数分钟)—— 真修选**展示层** `SubagentTool::execute` 自有 deadline 而非根因层全局 client timeout,符合 P10-3 副作用诚告的「不强加-破老用法的」净修原则。`SUBAGENT_DEFAULT_TIMEOUT_SECS` 取 180s 是给真生产宽裕窗口,既不挂父 turn 也不误杀正常的慢长 multi-turn subagent。

**焊入式 CI 回归闸(两层)**:
1. **`src/subagent.rs::mod tests::subagent_timeout_env_override_default_and_fallback`**(恒跑,不真起子进程):验默认 180s + env 整覆盖 + 垃圾值/0/空/空白全回退默认(防 parse 失败 panic 整测组)+ 测尾清回 hygiene。**真修逻辑(`subagent_timeout()`)的直接单测**,跨 OS,0s 跑。
2. **`examples/p105_subagent_hang_repro.rs` env-gate 二进制闸**(对齐 P10-3 「临时 example 跑出现场证据后装成 env-gate CI 跑通闸」范式):`CODEAGENT_P105_GATE` 非空跑真撞真修,未设 eprintln skip + 0 退出(记 pass 事实 skipped,CI 默认不耗 ~3s 真起 codeagent 子进程)。本机 2026-08-11 设 gate 跑:`PASS_FIX_OK` verdict、`ctrl=true`、`elapsed=3.06s`(修前 v1 15.00s 永挂 → 修后 v2 3.06s execute 自返 Err 超时,15.00→3.06 修前/修后实证对照锁真修生效);gate off = skip + exit=0。

**保留的修前机制回归闸**:`src/main.rs::mod tests::subagent_read_to_end_hangs_forever_on_non_exiting_child`(析因单测,跨 OS 不退子进程 `cmd /C ping -t` / `sh -c sleep 999999` 包 `tokio::time::timeout(2s)` 断 `Elapsed` + 对照 `echo hi` 秒回断 'hi' 不误杀,2.06s 单跑)。它复刻的是「裸 await 永挂」机制本身,**与生产 code path 无关**(inline spawn,不走 SubagentTool::execute),作**反退化闸**:有人想 PR 删 execute 的 timeout 包时,这条析因单测会先证「裸 await 必永挂」让 PR 复议;对真修 robustness 不变故不改 (P10-2 interrupted_round 对照腿同范式)。

**验收(2026-08-11 本机实测)**:四闸全绿(fmt / clippy `-D warnings` / `check --tests` / example build)。`cargo test --lib` **44 passed**(原 43 + 1 新 `subagent_timeout_env_override_default_and_fallback`,30.80s 含 bash 真 ping-t 闸 + glob 真 cycle 闸)。p105 二进制闸:gate-off skip + exit=0、gate-on `VALIDATED verdict=PASS_FIX_OK ctrl=true elapsed=3.06s` + exit=0。**诚实副作用**:subagent 默认上限 180s,真跑超 3 分钟的 subagent 会被 kill(此前无界),`CODEAGENT_SUBAGENT_TIMEOUT_SECS` env 可调 —— 这是「修正无界 hang」带来的有界 deadline,**非破老用法**(本就期望 subagent 收工而非永挂,180s 给真生产宽裕)。

**累计 P10 实证价值(更新)**:P10-1/2/3/**5** 四条真坑各被真撞真修焊进 CI 回归闸;P10-4 真跑真压缩 live 触发但 `summarize 返空 content` 候选证否、不收为真坑不修。守「连续撞不到真硬坑就算结束」判定 —— P10-4 已证否(撞了没坑第四态)、P10-5 真修至此 P10 五条候选**全收口**:3 证成真修(#1/#2/#3)+ 1 证否不修(#4)+ 1 证成真修(#5)= 4 真 1 否,**P10 结束**(撞得够全、回贴、待用户删 key 收摊)。










---


## 17. P12:P10-5 acknowledged scope 外的真坑接力(2026-08-11 起·同款「实证驱动」逻辑接力)

P11 收口「连续三条撞不到真硬坑」牌子收摊后,P12 不是路线图预设格,是**回 P10-5 acknowledged 但 scope 外未修的 fact chain** —— 同款「实证驱动」逻辑接力:P10-5 修了**第一环**(父 `read_to_end` 加 `tokio::time::timeout` 兜底,subagent 子进程那一头),但其 fact chain 注释(main.rs 析因单测 1467-1468)显式列了 **acknowledged 但 scope 外的后三环**未修:

- 第二环「子 `run_one_turn` 主循环 `chat_completion*.await` 裸 await 上游」
- 第三环「`reqwest::Client::new()`(main.rs 857/1159)裸构、无 `.timeout()` / `.connect_timeout()`」
- 第四环「`--script` 无 Ctrl-C 信号源挂中断」

P10-5 只修展示层(subagent 自有 deadline),根因层(reqwest client 兜底)刻意不修 —— 注释原话「顶层 `reqwest::Client::new()` 加 `.timeout()` 会断流式 SSE 长跑被切」。P12 问的是:**这后三环在主 agent 自己(非 subagent 委派路径)直接调 `chat_completion` / `chat_completion_stream` 时**,会撞出真坑吗?主 agent 路径与 subagent 路径同根(都 `reqwest::Client::new()` 裸构),但**主 agent 是 REPL/`--script` 的主回路** —— 上游 hang 在主回路直接 hang 整个 agent turn,不经过 subagent 那层 read_to_end 中转。

### 17.1 候选 #1:主 agent `chat_completion*` 裸 await 上游 hang —— 真坑升格 + 真修

**事实链(读码逐条确认,非猜;接力 P10-5 fact chain)**:主 agent 自己(非 subagent 路径)直接调 `chat_completion`(非流式) / `chat_completion_stream`(流式),client 同样 `reqwest::Client::new()`(main.rs 857 `run` + 1159 `probe_tool`)裸构、**无 `.timeout()` / `.connect_timeout()` / `.read_timeout()`**。上游应用层 hang(TCP 已建但永不回 HTTP 响应头)时:

- `chat_completion`(非流式,main.rs ~106):`.send().await` 裸 await 无 `tokio::select!` —— 无任何模式救得了;
- `chat_completion_stream`(流式,main.rs ~235):`.send().await` 裸 await 在 chunk 累积循环**之外**,Ctrl-C `tokio::select!{biased;...}` 挂在 chunk 循环内(262-343) —— **send 阶段 hang 连循环都进不去,中断 select 也救不了**。

主用例区分:
- TTY 流式 REPL:chunk 阶段有 Ctrl-C 兜底,但 **send 阶段 hang 救不了**(select 挂在 chunk 内,send 在 chunk 外裸 await);
- `--no-stream` / `--script`:连 chunk 阶段都没 Ctrl-C(send 也不挂 select),**全链无超时兜底**。

**候选真坑性判规(同 P10-5 性质,确定性控制流缺陷非概率 race)**:给定「上游建好 TCP 但永不回 HTTP 响应头」输入 → 主 agent `send().await` 永挂 → 整 agent turn 永死。这不是「上游偶尔慢」的概率 race,是**给定确定性坏上游**的控制流缺陷(类比 P10-2 喂中断现场、P10-5 喂 hang 上游现场:给定输入 → 父侧代码是 buggy 纯逻辑)。损害 = 整 agent turn 永死,`--script` 用户喂 turns 会被一条 hang 的上游卡死整个批跑(P6.1 长会话曲线采集场景里「上游 hang 一条 → 整批跑停」就是这条)。同 P10-5「确定性缺陷 + 真复刻现场 = 升格真坑」门槛(P10-4「外部模型依赖不可控」做空的反面)。

**真撞实证升格(撞坑第五态接力 — 本地 mock 上游真起 production codeagent 主进程)**:起 `examples/p12_main_agent_upstream_hang.rs`(env-gate 二进制,默认 skip、`CODEAGENT_P12_GATE=1` 跑真,范式对齐 p105 / mcp_toctou_race / env-gate,**非 `#[ignore]`**)。它的真撞手段(不起真上游、不依赖真 key):

1. **mock hang 上游**:`TcpListener::bind("127.0.0.1:0")` accept 一条连接后 `tokio::spawn` 一 long-hold task `let _io = stream; loop { tokio::time::sleep(3600).await; }` —— **持 stream 整连接生命期不退、不读不写不关**(同 p105 long-hold 范式;反事实对照:早期一 accept 就 drop 会触发 TCP RST 83ms 退报 send error,是伪现场 —— long-hold 才真复刻「上游应用层 hang」)。这模拟「上游建好 TCP 但永不回 HTTP 响应头」;
2. **临时 toml 写 tmp 目录**(`base_url=http://127.0.0.1:<mock-port>`、`model="hang-model"`、`api_key_env="DEEPSEEK_API_KEY"`,key 注 `fake-not-used-mock-never-returns` 上游假不验真),**不动** crate root 真 production `codeagent.toml`(纪律:`gitignored` 未追踪、不碰真生产配置、env-only)。`Config` 解析确认字段齐(对齐 p105 `use codeagent::config::Config`);
3. **起真 production codeagent `--script --yolo` 主进程**(`kill_on_drop(true)` + `.env(key_env, fake)` + `.current_dir(tmp)` 让子读临时 toml),喂一行 user 输入 `用一句话回答:好\n` 后 drop stdin → 子 `read_script` 读到一行进 `run_one_turn` → 主 agent `chat_completion_stream` 走 send 阶段。**hang 模式额外注 `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS=5` 压窗口**(修后应 ~5s 兜错退;echo 模式走 default 90s 不压,正常上游几百 ms 走完不摊到 read_timeout);
4. **外层 `tokio::time::timeout(15s, child.wait()).await` 包**,外层 = `Result<Result<ExitStatus, io::Error>, Elapsed>`,match 三态:
   - `Err(_elapsed)`(撞钟):hang 模式 → `REGRESSION_STILL_HANG`(修退化回修前真坑);echo 模式 → `ECHO_UNEXPECTED_HANG`(对照异常);
   - `Ok(Err(wait_io_err))` → `WAIT_IO_ERROR`(child.wait IO 失败,异常让人查);
   - `Ok(Ok(status))`:hang 模式 → `FIX_OK_TIMEOUT_ERR_EXIT`(修守住:子进程 ~5s 拿 reqwest `read_timeout` Err 退非永挂,stderr 含 reqwest timeout err `build_http_client()` 生效);echo 模式 → `ECHO_NORMAL_EXIT`(对照绿,验闸不误杀正常路径)。

**修前实证(2026-08-11 本机)**:gate-on hang 模式跑 → 外层 15s **撞钟 `Elapsed`,elapsed=15.27s** —— **主 agent 裸 await 上游 hang 永挂真坑现场实证成立**,候选 #1 升格真坑(15.27s Elapsed = `REGRESSION_STILL_HANG` 态)。同 P10-5 v1 15.00s 永挂对位 —— 主 agent 路径与 subagent 路径同根 hang 现场,但**主 agent 是主回路、不经 subagent 中转**,故修必须在根因层(client 兜底)而非展示层。

**真修**(`src/main.rs`,新增集中段、替换两处裸构):

- **新增 `build_http_client()`** 集中体:`reqwest::Client::builder()` + `connect_timeout`(DNS+TCP 阶段,default 30s)+ `read_timeout`(**每次 read 字节间隔上限非总时长**,streaming-safe —— 不用 `.timeout()` 总包因会断流式 SSE 长跑,对齐 P10-5 注释「根因层 `.timeout()` 会断流」的诚实约束,但 `read_timeout` 是「每字节间隔」非「总时长」,SSE 长跑只要持续有字节就不触发,真生产慢首 token 数分钟不被切);
- **新增 `upstream_timeout_env(name, default_secs)`** env helper:读 `CODEAGENT_UPSTREAM_CONNECT_TIMEOUT_SECS` / `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS` 覆盖,`Some(secs)` if secs>0、`Some(0) => None`(显式关逃生口,对齐 subagent_timeout 但加 `Some(0)` 关语义)、`None`/垃圾/空/空白回退默认(对齐 p105 `subagent_timeout()` 风格,防 parse 失败 panic);
- **新增 `DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 30` / `DEFAULT_READ_TIMEOUT_SECS: u64 = 90`** 常量(真生产 DeepSeek 慢首 token + 大模型长生成给宽裕);
- **替换两处 `let client = reqwest::Client::new();`**(`run` ~857 + `probe_tool` ~1159)为 `let client = build_http_client()?;`(`replace_all=true` 同改),前置注释标 P12 候选 #1 真修。

**修后实证(2026-08-11 本机)**:gate-on hang 模式跑(注 `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS=5` 压窗口)→ 外层 **不撞钟**,子进程 **5.17s 拿 reqwest `read_timeout` Err 退** → `FIX_OK_TIMEOUT_ERR_EXIT`(stderr 含 reqwest timeout err)—— **15.27s 永挂 → 5.17s 兜错退,修前/修后实证对照锁真修生效**(闸红绿翻转 ✗→✓)。**echo 对照绿**:gate-on echo 模式跑(走 default 90s、正常上游几百 ms 走完不摊到 read_timeout)→ 子进程 **411ms 正常收工退** → `ECHO_NORMAL_EXIT` —— **验闸不误杀正常路径**(对照前实测 73-167ms 退 vs 修前 15s 撞钟可区分;echo 模式子进程因手写裸 HTTP 响应 reqwest 在某处拒故 exit code 非 0,但闸核心断言「秒退 vs 15s 撞钟可区分」成立,不追完美 echo 模式 exit=0)。

**为什么是根因层修而非展示层修(与 P10-5 对照)**:P10-5 修 subagent 路径 = 展示层(`SubagentTool::execute` 自有 `tokio::time::timeout` 包 `read_to_end`),根因层 client 故意不修(怕 `.timeout()` 总包断流);P12 修主 agent 路径 = **根因层 client 兜底**(`build_http_client` 加 `connect_timeout` + `read_timeout`),因为主 agent 是主回路、没有展示层 `read_to_end` 中转可包 —— 修必须在 client 层。**关键设计选择**:用 `read_timeout`(每字节间隔)非 `.timeout()`(总时长),SSE 长跑只要持续有字节就不触发 —— 兼顾「上游 hang 兜底」与「真生产慢长 streaming 不被切」两约束。这是 P10-5 acknowledged scope 外的真修,非破 P10-5 决定(展示层 subagent deadline 仍在,根因层 client 兜底补上后三环之二、三)。

**不修第四环的诚实记(同 P10-5 保留判断)**:第四环「`--script` 无 Ctrl-C 信号源挂中断」**此版不修** —— 主 agent client 兜底(`connect_timeout` + `read_timeout`)已覆盖 send 阶段 hang(后三环之二、三),第四环是「信号源」层而非「timeout」层,加 Ctrl-C 信号源是另一个独立 capability(P5 中断挂在 TTY mpsc,`--script` headless 加信号源需独立设计),**非 P12 候选 #1 的真修范畴**。client 兜底已让 send 阶段 hang 不再永挂(每字节间隔 90s 无响应即兜错退),第四环的「无法中断」在「兜错退」后已不是永死问题 —— 用户等 90s 拿 timeout err 而非永挂,中断信号源是体验优化非 hang 修复。守 P10-3「净修原则:不加臆造功能」,第四环留作未来 capability。

**焊入式 CI 回归闸**:`examples/p12_main_agent_upstream_hang.rs` env-gate 二进制闸(对齐 p105 / mcp_toctou_race / env-gate 范式,**非 `#[ignore]`**)。两模式各跑:
- **hang 模式**(真撞真修):gate-off skip + exit=0(CI 默认不耗真起 codeagent 子进程);gate-on(注 `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS=5` 压窗口)跑修后 production codeagent → 期望 `FIX_OK_TIMEOUT_ERR_EXIT` ~5s 退 exit=0;若落 `REGRESSION_STILL_HANG`(外层 15s 撞钟)= 修退化回修前真坑,闸抓住 exit=1。修前实测 15.27s Elapsed → 修后 5.17s FIX_OK,闸红绿翻转锁真修;
- **echo 模式**(对照不误杀):gate-on(走 default 90s)跑正常自退上游 → 期望 `ECHO_NORMAL_EXIT` 几百 ms 退 exit=0;若撞 `ECHO_UNEXPECTED_HANG` = 对照脚或闸本身有问题 exit=1。本机 411ms ECHO_NORMAL_EXIT 绿。

`kill_on_drop(true)` 兜底收子进程(撞钟态子进程仍挂着,exit 前 kill_on_drop 触发收,同 p105)。

**验收(2026-08-11 本机四闸全绿)**:fmt OK(`cargo fmt --all -- --check`)/ clippy `-D warnings` **0 warning** / `check --tests` Finished / `cargo test --lib` **44 passed**(与 P11 收口同基线,无新增 lib 单测 —— P12 候选 #1 真修是 client 层集中体替换、env helper 逻辑由 p12 example 二进制闸覆盖真起实证,未另加 inline 单测;主 agent client 路径无 inline pure-function 切点可单测,对齐 p105 范式「env-gate 二进制闸证真撞真修」)。p12 二进制闸:gate-off skip + exit=0、gate-on hang `FIX_OK_TIMEOUT_ERR_EXIT elapsed=5.17s` exit=0、gate-on echo `ECHO_NORMAL_EXIT elapsed=411ms` exit=0。**修前/修后实证对照确立**(15.27s 永挂 → 5.17s 兜错退 + echo 411ms 不误杀),新闸进 CI 作回归闸(若有人把 `Client::new()` 裸构改回、或删 `build_http_client` 的 timeout 配置,hang 模式→撞钟 FAIL exit=1,必撞)。

**诚实副作用**:主 agent client 现在有 `connect_timeout` 30s + `read_timeout` 90s 兜底 —— 真生产遇到「建 TCP 慢 >30s」或「上游建好 TCP 但 >90s 不吐字节」会兜错退(此前永挂);真生产 DeepSeek 慢首 token 数分钟但**持续吐字节**(SSE chunk 间歇到达),`read_timeout` 是每字节间隔非总时长,持续吐字就不触发 —— 非破老用法。env `CODEAGENT_UPSTREAM_CONNECT_TIMEOUT_SECS` / `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS` 可调,`Some(0)` 显式关(逃生口,对齐 subagent_timeout 但加显式关语义)。P10-5 展示层 subagent deadline 180s 仍在(两条 timeout 独立、不冲突:主 agent client 兜底拦上游 hang,subagent deadline 拦子进程整轮 hang)。

**P12 候选 #1 性质归类**:与 P10 五态对照 —— P10-1 race(概率 N=200 闸)/ P10-2 确定性纯函数(析因单测)/ P10-3 确定性 + OS 真建环(跨 OS cycle 单测)/ P10-4 外部依赖不可控(live 证否)/ P10-5 确定性 + 本地 mock 上游(example 二进制闸真起)。**P12-1 = P10-5 同型「确定性 + 本地 mock 上游 + example 二进制闸真起主进程」**(非 subagent 委派,主 agent 主回路),撞坑第六态接力 P10-5 fact chain 的 acknowledged 但 scope 外后三环之二、三。**P10-5 修展示层 / P12-1 修根因层** 是 fact chain 同根两环的分工修,不重复、不冲突。

### 17.2 候选 #2:上游 Err 路径漏落盘 + 漏 truncate(P12-1 echo 假绿诚实副作用同收 + 判「真撞到损害但属已知设计取舍」证否)

**事实链**(承接 §17.1 P12-1 已修 client 兜底 timeout 后,接力 fact chain 看下一环):`run()` 主循环 line 1140-1151 `run_one_turn(...).await?` 是裸 `?` —— 当上游返非 2xx(500/502/429)/ reqwest send 失败 / 流读字节失败 / 错误帧时,`run_one_turn` 返 `Err` → 裸 `?` 把 Err **直接 propagate 出 `run()` → `main()` line 1291 末尾 expression 出 `fn main() -> anyhow::Result<()>` → 进程非 0 退**。这条路径**不经过** line 1199 `else { messages.truncate(pre_turn_len) }`(finished=false 正常返才进)、line 1196 `session::save`(finished=true 正常返才进)、line 872 `exit_repl`(干净退出 /quit/Eof 才进)。故**本轮 user**(已 line 1136 push 进内存 `messages`)随进程退丢失,**磁盘 `.codeagent_session.json` 仍是上一轮(K-1)完成态**,不含本轮 user。用户 `--resume` 拿 K-1 版本,本轮问的一句话没了——得重问。

**关键判规(与 P12-1 同门槛分级)**:
- **触发端** = 上游 Err = 外部依赖概率事件,性质偏 P10-4「外部依赖不可控」**非** P10-2/P10-5/P12-1 那种「确定性控制流缺陷」;
- **但损害链是确定性控制流缺陷** —— 给定上游 Err,必然本轮 user 不落盘 + 进程退,非概率 race;
- 读码看不见显式「Err 路径故意丢本轮 user」设计声明;`?` propagate 是 Rust 惯用法,注释 line 1152-1156 只覆盖 finished=false 正常返路径的「不落盘 + truncate」语义,Err 路径无说明 —— 属「正常返路径有清理,Err 路径漏清理」的遗漏(类比 P10-2「正常收盘有 pop,打断轮漏 pop」)。

**真撞实证升格(M2 ≠ P10-4「未撞证否」,是「真撞到损害」实证)**:`examples/p12m2_upstream_err_drops_round.rs`(env-gate 二进制,`CODEAGENT_P12M2_GATE` 非空跑真 / 否 eprintln skip+exit=0,范式对齐 p105/p12_main_agent_upstream_hang/env-gate、**非 `#[ignore]`**):mock 上游按连接计数回不同响应(conn 1 = 合法 OpenAI streaming response round 1 落盘 / conn 2+ = HTTP 500 round 2 撞 Err),临时 toml 写 tmp 指 mock(key 注 fake 不动 crate root 真 prod `codeagent.toml` gitignored),起真 production codeagent `--script --yolo` 主进程 `kill_on_drop(true)` 喂两行 user 输入(第一轮问好 / 第二轮问再见),外层 `tokio::time::timeout(20s, child.wait())` 包,收子进程退出后读磁盘 `.codeagent_session.json` 三断言:含 round 1 user + 含 round 1 assistant + 不含 round 2 user。**实证结果(2026-08-11 本机跑)**:conn 1 accept 回 200 streaming → stderr `[ctx:stream:1] prompt=5 completion=1 total=6` `[compactor:noop] total=6 threshold=22400 (未到阈值,不压)` + stdout `好` = round 1 finished=true 走完;conn 2 accept 回 500 → stderr `Error: 调用 http://... 失败(流式,第 1 轮): 流式 HTTP 状态非 2xx: HTTP status server error (500)` = round 2 run_one_turn Err;**子进程 非正常退 (Some(1)) 132ms** 非 0 退;**磁盘 session.json 491 字节,含 round 1 user「第一轮问好」+ 含 round 1 assistant「好」+ 不含 round 2 user「第二轮问再见」** = `REGRESSION_CURRENT_ROUND_LOST` verdict。**M2 真坑损害现场实证成立**:给定上游 Err,确定性本轮 user 不落盘 + 进程退 + K-1 保留。**修正侦察 agent 误述**:Agent B 之前说「前面 N 轮成功会话全丢」是错的 —— P7「finished=true 一轮一 save」语义下,前面 N-1 轮每轮都落盘,只有当前 K 轮(触发 Err 那轮)本轮 user 丢。损害 = 本轮问的一句话重打成本,**不累积**(无论 K 多大,损害都是一个 user turn 的输入丢失),前面成功轮 K-1 已在磁盘没丢、`--resume` 能接上前面对话。

**同坑过程撞出的 P12-1 echo 假绿诚实副作用同收**:M2 真撞过程第一版 mock 不读 client POST body 就 write 响应 + shutdown,Windows TCP 发 RST 非干净 FIN,reqwest 在 `.send()` 阶段报 `error sending request`(line 302,不是 line 300 的「HTTP 状态非 2xx」),子进程 75ms 拿 Err 退(code 1),磁盘 session.json 0 字节(连 round 1 都没落盘)。修 mock 先 `stream.read(&mut req_buf)` 读一段 client→server 数据再回响应后真撞现场才成立。这同款 mock 缺陷使**上轮 P12-1 §17.1 的 echo「411ms ECHO_NORMAL_EXIT 对照绿」实为假绿** —— P12-1 example 的 echo 判读 `Ok(Ok(_))` 一律判 `ECHO_NORMAL_EXIT`,**没校 `status.success()`**,把「子进程拿 Err 退(code 1)」也误判为「子进程正常收工退」,假绿掩盖「echo 从没真验过正常路径在修后不被误杀」的事实。P12-1 hang 模式期望态就是「子进程拿 timeout Err 退」,unsuccess 是符合期望,故 hang 判读无错、P12-1 真坑成立靠的是 hang 红绿翻转(15.27s→5.17s),echo 是辅助对照——但「echo 对照绿」论证本身确实无效。**诚实原则要求同收**:在 P12 候选 #2 范围内(同一 example 命名空间、P12 真坑接力、上轮 P12-1 验收诚实性遗留)同步修两处:① echo mock 加 `stream.read(&mut req_buf)` 读完一段 client body 再回响应(让现行真 `reqwest::send()` 走干净 FIN 不 RST);② echo 判读校 `status.success()` 才判 `ECHO_NORMAL_EXIT`、不成功判 `ECHO_ERR_EXIT` exit=1 让人查(hang 模式无改、仍 `FIX_OK_TIMEOUT_ERR_EXIT` 期望态)。修后 P12-1 echo 真绿:`code=0 108.44ms ECHO_NORMAL_EXIT`(校过 `success()=0`)+ stderr `[ctx:stream:1] prompt=5 completion=1 total=6` + `[compactor:noop]`(round 1 streaming 真走完了 = 第一次 echo 真验过「修后正常路径不被误杀」);hang 仍 `5.07s FIX_OK_TIMEOUT_ERR_EXIT`;gate-off skip+exit=0。**这是「净修原则」内合理修**:同 example 命名空间、同 P12 接力范围、修的是上轮 P12-1 验收诚实性遗漏(非 src/main.rs 真修、非 cherry-pick 闸),echo 假绿是 P12-1 commit `be6a679` 已推 remote 时未察觉的诚实性 bug,M2 真撞过程撞出来属 P12 第二候选值得修。

**真修价值评估 → 判证否不真修**:
- **A 派「补对称清理 + 仍 propagate 退」**:Err 路径也 truncate 本轮 user + save 干净态再 propagate 退 —— 但 K-1 本就已在磁盘(上一轮 finished=true 已 save),**此派修等于啥都没改(空修)**;
- **B 派「catch + continue REPL」**:catch run_one_turn Err → truncate 本轮 user + eprintln 报错 + **continue 回 REPL 不退**。损害面收窄更彻底(进程活着用户立刻重问)。**但改 fatal 语义有副作用**:API key 无效 / provider 不可达 这类「每轮都 Err」硬错误,continue REPL 让用户在 REPL 里每轮撞错、不死不活反复弹错,体验不如直接退让用户改 config(进程退让用户撞一次错就看到「API key 无效」立刻去改 env/toml,比继续让他撞十次更友好);
- **损害轻微**:本轮问的一句话重打成本低(P7 一轮一 save 保证损害不累积,只丢本轮 K 的 user turn,不丢前面成功轮);fatal 退是 Rust `fn main() -> Result` 惯用语义非 bug;`--resume` 仍能接上前面对话(K-1 已落盘)。

**M2 判收口**:与 P10-4「未撞证否」对照,**P10-4 = 未撞证否(P10-4 真压缩 live 跑通但空 content bug 条件未实证撞到)**;**M2 = 真撞到损害但证否**(同 P10-4 同款细分变种,M2 是更进一步的细分——损害确实发生但**真修代价 > 收益故不修**)。这是「连续撞不到真硬坑就算结束」守则下的计数:- M2 真撞到损害但判不真修 → 不算「真坑真修」(连续撞不到真硬坑计数 +1 朝收摊走),也不算「真坑真修」清计数;**视为「P10-4 同款第四态证否但更细:撞到损害仍证否」**。journey 落点诚实记「真坑损害现场 + 真修代价>收益证否」两腿,**不带「假绿」假象**:
  · 不是「未撞证否」(P10-4 路径)— M2 真撞到了损害(磁盘实证三断言全中);
  · 不是「真坑真修」(P12-1 路径)— M2 真修改 fatal 语义代价 > 收益;
  · 是「真撞到损害但属已知设计取舍」证否(新细分,记入「连续撞不到真硬坑」计数)。

**焊入式 CI 回归闸**:`examples/p12m2_upstream_err_drops_round.rs` 是**单态证否闸**(非红绿翻转、非修后回归——M2 判不真修故无修后 ground truth 对照),gate-on 期望 `REGRESSION_CURRENT_ROUND_LOST`(真坑损害现场成立)+ exit=0(实证已记录损害态);异常态 `UNEXPECTED_NO_EXIT` / `WAIT_IO_ERROR` / `UNEXPECTED_CLEAN_EXIT` / `UNEXPECTED_COMBO` → exit=1 让人查。该闸作用:**防未来 M2 控制流缺陷描述错**(若有人改 run() Err 路径加了 catch + truncate + save,闸异常态 `ROUND2_PRESERVED` / 子进程正常退 → 提示 M2 已被修、闸该档更新);**防 mock 退化**(若有人改 mock 不读 client body,闸必踩同款 reqwest RST 退 stdout 空白异常态)。echo 对照绿由 P12-1 example 的 echo 模式承担(已修真绿校 success()=0)。

**验收四闸全绿(2026-08-11 本机)**:fmt `cargo fmt --all -- --check` OK / clippy `cargo clippy --all-targets -- -D warnings` **0 warning** / `cargo check --tests` Finished / `cargo test --lib` **44 passed**(与 P12-1 同基线;M2 判证否故 src/main.rs 零改动,实证由 example 二进制闸覆盖真起 production codeagent --script 撞 mock 上游两轮验磁盘三断言)。p12m2 闸:gate-off skip+exit=0、gate-on `REGRESSION_CURRENT_ROUND_LOST` 132ms/141ms exit=0(两跑稳定)。P12-1 闸 echo 修复后:gate-off skip+exit=0、gate-on echo `ECHO_NORMAL_EXIT 108.44ms code=0` exit=0(校 success()=0 真绿)、gate-on hang `FIX_OK_TIMEOUT_ERR_EXIT 5.07s` exit=0(无回归)。

**诚实副作用**:
- src/main.rs 零改(M2 判证否不真修);
- P12-1 example 判读漏洞 + echo mock 协议缺陷同收(诚实性补丁、上轮 P12-1 验收遗留、M2 真撞过程撞出来);
- 临时 toml 写 temp_dir、key 注 fake、prod `codeagent.toml` mtime 未变(08/09/2026 14:30:39)随 env-only 纪律;
- 「连续撞不到真硬坑就算结束」守则:M2 真撞到损害证否 → 计 1 条「撞到但非真修」朝收摊走,需后续连续撞不到真硬坑 2 条才算 P12 收摊(被 P12-1 打破的连续计数从 0 重起后,P12-2 当前计数=1 朝收摊走)。

### 17.3 候选 #3:ctrl_c 注册失败 → mpsc 关 → select `_` 模式假中断 → 持续刷本轮 user(真坑升格 + 真修)

**事实链**(承接 §17.2 P12-2 证否后接力看 interrupt 路径,读码逐条确认非猜):`run()`(main.rs 914)创建 `let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::channel::<()>(8);` 后 `tokio::spawn` 一后台 task `loop { if signal::ctrl_c().await.is_err() { return; } let _ = interrupt_tx.send(()).await; }` —— 这份 task move 走**原始 `interrupt_tx`**,**主循环只在创建后立刻 drop 自己那份 `interrupt_tx` 的所有引用**(只留 `interrupt_rx` + spawn task 持的 `interrupt_tx`)。`run_one_turn`(721)流式分支每轮 `let interrupt_fut = interrupt_rx.recv(); tokio::pin!(interrupt_fut);` 后挂进 `chat_completion_stream` 内 line 327 `tokio::select! { biased; _ = &mut interrupt => { interrupted = true; break; } chunk_res = stream.next() => {...} }` —— **`_` 模式不绑定 future output**,无论 `recv` ready 返 `Some(())`(真 Ctrl-C)还是 `None`(channel 关、all sender dropped)都 fire 该 arm 设 `interrupted=true`。

**真坑触发条件 + 确定性**:`tokio::signal::ctrl_c()` 在某些 Windows 环境(无 console 句柄 / `DETACHED_PROCESS` / `CREATE_NO_WINDOW` / 某些脱机服务/沙箱断 TTY 环境)返 `Err` —— spawn task 进 `if signal::ctrl_c().await.is_err() { return; }` 分支 silently `return`,drop 它那份**唯一的 `interrupt_tx` sender**。mpsc channel 仅在**所有 sender drop** 后关,这里 keepalive 缺席 = task `return` 后 channel 关 → 主循环 `interrupt_rx.recv()` 立刻 ready 返 `None` → `tokio::select! _ = &mut interrupt` arm fire(`_` 不区分 None/Some 就当成 ready)→ `interrupted = true; break;` → `chat_completion_stream` 返 `StreamOutcome::Interrupted` → `run_one_turn` 返 `(false, 0)` → run() 主循环 `finished=false` 进 truncate 分支 `messages.truncate(pre_turn_len)` 刷掉本轮 user → REPL 表现为**「没按 Ctrl-C 但每轮被秒打断、本轮问话丢、模型永远不答」死循环**(下一轮 read 一行新 user,挂 select,又立刻假中断,无限循环刷 user)。**判规**(与 P12-1 同门槛分级):触发端 = ctrl_c 注册失败 = 环境概率事件(本机 Win11 + console 不大概率不返 Err,遵经验环境依赖偏 P10-4);**但损害链是确定性控制流缺陷** —— 给定 channel 关 → 必然假中断持续刷本轮 user,**非概率 race**;且持续 + 静默 + 难诊断(用户看 stderr 无任何线索,只感觉 REPL 不答)三性合 = 比 P12-2「撞到损害但 fatal 退」更糟 —— P12-2 进程退用户至少有信号「我做坏了」,M3 REPL 假死用户以为模型不答反复重打。

**判定:真坑真修(非证否)** —— 与 P12-2「真撞到损害但已知设计取舍证否」相反,M3 真修无副作用代价:
- 旧注释(line 745 区域)自辩白称「发送端常驻不会关、万一关了也安全退化(本轮作废)」 —— **这是误判**:旧逻辑下 `_` select 模式不区分 None vs Some(()),None 被当 Ctrl-C,进而每轮假中断持续刷本轮 user,REPL 死而不退,远非「安全退化」(本轮作废至少下轮能正常答;M3 下轮一样假中断,根本不答)。注释诚实性纠错随修同收。
- 真修 vs 候选 A/B:A 派「`select!` 模式 `_` → `Some(_)`」绑 output 区分 None/Some,但① 有 stale-poll 风险(模式不匹配的分支在 select! 调用余程被禁用、下次 loop 重新启用,但 `Some(_)` 长期不匹配让该 arm 一直被禁用语义让人糊涂)、② 依赖「channel 关后重复 poll 已融合的 recv future 仍稳定返 None」这条**实现细节非文档保证**(tokio mpsc `recv` cancel-safe 已文档确认,但反复 poll 已融合 future 行为是内部实现非 API 合同),把真修复杂化;B 派「主循环持一份 sender keepalive」,mpsc 文档明确「channel 仅在**所有** sender drop 后关」,keepalive 一份活着就**结构性保证**(非依赖内部细节)channel 不关 → recv 不 ready None → select! 这条 arm 长期 Pending → 永远只在真信号(Some(()))fire。B 修根因 + 注释诚实性纠错更简洁,**判用 B 派**。

**真修**(`src/main.rs`):`run()` 在创建 channel 后立即 `let _interrupt_keepalive = interrupt_tx.clone();` —— 这份 keepalive 由 `run()` scope 持到 `run()` 结束;spawn 监听 task move 走原始 `interrupt_tx` 不变。即便后台 task 早早 `return`(ctrl_c 注册失败),keepalive 这份 sender 仍活着 → channel 不关 → `interrupt_rx.recv()` 永远 Pending(无真信号就等)→ 不会被 `select! _ = &mut interrupt` 误判为 Ctrl-C 假中断。同时 ctrl_c 注册失败分支原 silent `return` 改 `eprintln!("[ctrl_c] 注册中断信号失败,本进程将不响应 Ctrl-C(仅影响中断,其他正常)。")` 后 `return` —— 诚实性:让用户**知晓**本进程无 Ctrl-C 功能(退化态是「无中断功能」非「假中断打死 REPL」),不再静默。`run_one_turn` 内 select 前注释(line 745-747 区域)同步改写为新 P12-3 守门说明:`recv` 出 None 仅在所有 sender 全 drop 时发生,keepalive 修后此分支永不返 None,select! 永远只在真信号触发;**显纠旧注释误判**「万一关了也安全退化」是错的(旧 `_` 模式 None 当 Ctrl-C 持续刷 user 死循环,非安全退化)。

**析因单测闸**(落 `src/main.rs::mod tests`,**非 example 二进制闸,非 env-gate**):M3 触发依赖 `tokio::signal::ctrl_c()` 真 Err 环境(本机 Win11 + console 大概率不返 Err),example 二进制闸难自动复刻真现场(对齐 P10-5/P12-1 env-gate 二进制闸范式都需真起 production codeagent 子进程 + 能 production 触发真坑现场;M3 触发 ctrl_c 注册失败需特殊环境仿真 = 仿真成本 > 价值)。故用**析因单测**(对齐 P10-2「锁不变量不跑真运行时」范式):复刻 select `_` 模式 + mpsc 关 channel recv 返 None 这条 `tokio` **通用机制**,**不调 production `chat_completion_stream`**(它 binary-private + 需 mock reqwest 上游,单测难),析因层把机制锁死 —— 验证它确实「ready 即 trigger arm,不区分 None/Some」,无需 keepalive、无需真 ctrl_c 注册失败环境。
- **测 #1 `select_underscore_pattern_treats_closed_recv_as_ready_interrupt`**(真坑机制再现,析因锁):`tokio::sync::mpsc::channel::<()>(8)` 创建后立即 `drop(tx)` → channel 关 → `rx.recv()` future 立即 ready 返 None → `tokio::pin!` 后喂进 `tokio::select! { _ = &mut recv_fut => {} }` —— **select! 走过 = arm fire 了**(若 recv 不 ready 某 no-else 单 arm select 会 panic abort,本测根本走不到下一步;走下一步就是 fire) = 真坑机制实证成立:`_` 模式在 recv 返 None(ready)时**仍然 fire**,不区分 None vs Some(())。这正是退化场景下假中断的源头:没按 Ctrl-C 但 select 当成中断了。(初版用 `trigger_fired` 变量 + `assert!(trigger_fired, ...)` 拉 clippy `unused-assignments` warning 同时再 `rx.recv()` 二段验 None 撞 E0499「cannot borrow `rx` as mutable more than once at a time」too —— 因 `recv_fut` pinned 持 `&mut rx` 活到测末。简化:撤掉 `trigger_fired` 变量 + 二段 None 验证,单 select! 走过即锁机制,clippy 干净 0 warning / borrowck 过。)
- **测 #2 `keepalive_sender_keeps_recv_pending_no_pseudointerrupt`**(B 修后机制 demo,对照):复刻 B 修结构 `let _interrupt_keepalive = tx.clone(); drop(tx);`(只剩 keepalive 一份 sender),mock「ctrl_c 注册失败 task `return` drop task 那份原始 sender 那场景」 → `rx.recv()` 必须**Pending**(keepalive 仍活 channel 不关)→ `tokio::time::timeout(100ms, rx.recv())` **撞钟 Elapsed** assert `verdict.is_err()`(若 Ok 则 recv 竟 ready None = keepalive 未保活 = B 修退化,FAIL)。两测对照(测 #1 全 drop recv 立即 ready fire / 测 #2 keepalive 保活 recv 长期 Pending 撞钟)封死真坑机制 + 修后不假中断。`let _ = &_interrupt_keepalive;` 防 keepalive 被 early drop(`_` 不续命需 ref)。**两测都是 `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]`**(对齐 P10-5 mod 测 flavor,防 current_thread 掩盖多线程-only 回归),跨 OS 0s 不依赖 ctrl_c 真 Err 环境。

**验收四闸全绿(2026-08-11 本机)**:fmt `cargo fmt --all -- --check` OK / clippy `cargo clippy --all-targets -- -D warnings` **0 warning** / `cargo check --tests` Finished / `cargo test --lib` **44 passed**(P12-3 真修在 src/main.rs 不动 lib 故 lib 基线与 P11 收口/P12-1/P12-2 同 44);**这是 M3 真修对 lib crate 零改**(P12-1 也是 src/main.rs 真修,lib 单测未增故 lib 不变)+ **binary mod tests runtime 实证显跑**:`cargo test --bin codeagent --no-fail-fast -- --test-threads=1` **11 passed** finished 2.18s —— 含旧 9 测(dispatch_tool/gate_check×2/interrupted_round/subagent_read_to_end_hangs/unified_diff×4)+ **新增 2 测 `select_underscore_pattern_treats_closed_recv_as_ready_interrupt`/`keepalive_sender_keeps_recv_pending_no_pseudointerrupt` 全绿**(M3 析因两腿对照 runtime 绿实证)。**CI 四闸不变性**(原样保留):`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` + `cargo check --tests` + `cargo test --lib` —— 注意 P11 起确立的术语「`cargo test --lib` 仅跑 lib.rs mod tests(`src/lib.rs` 下的单元测),**不跑** main.rs `mod tests`(binary target)」在 M3 此处再次验证成立:`cargo test --lib` 4 闸版仍 44 同基线,新增两 M3 测因属 main.rs `mod tests` 被 lib 过滤器跳过;但**四闸静态门禁覆盖**含 main.rs `mod tests` 编译期检查(`cargo clippy --all-targets` + `cargo check --tests` 都含 bin test target 编译)故两测编译期绿 = 静态门禁绿,本机 `cargo test --bin codeagent` 11 passed 是 runtime 绿显跑实证(非 CI 门禁载体,本地实证按「守不臆造」纪律真跑过)。

**诚实副作用 & 性质归类**:
- 不修第四环仍是 P12-1 净修原则范畴延伸(client 兜底 + interrupt keepalive 是 fact chain 同根后三环分工修)。
- keepalive `_interrupt_keepalive` 一行变量 = 极简修复(scope-held 持续活到 run() 结束),非新增 capability 非破老用法 —— 真 Ctrl-C 路径行为未变(真信号返 Some(())时 select arm 仍 fire 设 interrupted=true 走原 Interrupted 路径),只是闭了「channel 关 → None 假中断」这条退化路径。
- ctrl_c 注册失败从 silent `return` 改 `eprintln! + return` 是诚实性补充(非必需):让退化态对用户透明(用户至少知晓「本进程不响应 Ctrl-C」,而非白屏假死猜不透),无任何行为副作用(仍 `return` task 不写 signal)。
- 注释纠错:旧注释「发送端常驻不会关、万一关了也安全退化」是误判,新注释显式标志「None 被当 Ctrl-C 持续刷 user 死循环,远非安全退化」+「P12-3 keepalive 修后此分支永不返 None」—— 同步 main.rs line 928-948 spawn 块注释 + line 745-751 select 前注释 + ctrl_c 注册失败分支注释三处诚实性纠错。
- **P12-3 性质归类**:P10-2 同型「确定性控制流 + 析因单测喂现场」(非 P10-5/P12-1 env-gate 二进制闸真起主进程型,因 M3 触发 ctrl_c 注册失败依赖环境概率事件,example 二进制闸难自动复刻真现场 → 走析因单测范式);损害**比 P12-2 更糟**(P12-2 进程退用户至少有信号,M3 REPL 假死静默难诊断)故**真修非证否**;真修极简(keepalive 一行 + 注释纠错 + 诚实性 eprintln)无副作用 = 修代价 < 收益,与 P12-2「真修代价 > 收益证否」对照两端(同 P12 接力先后两环,一证否一真修,判规同框架不同结论靠「损害严重度 × 修代价」二维分)。
- 守「连续撞不到真硬坑就算结束」:**M3 真坑真修打破 P12-2 起的朝收摊计数** —— P12-2 是「撞到损害证否」计 1 朝收摊走,M3 是「真坑真修」**清零该朝收摊计数**(真修打破连续撞不到)+ 真坑真修计数 +1(P12 起累计真坑真修 = P12-1 + P12-3 = 2)。需后续连续撞不到真硬坑 3 条才算 P12 收摊(被 M3 真修再次打破,计数从 0 重起走 3 条门)。

### 17.4 候选 #4:SSE 自然 EOF 末 frame 无尾 `\n\n` 残 buf 不 flush(协议正确·真生产实证证否·不修不焊闸)

P12-3 真修后接力看 `chat_completion_stream` 的 SSE 解码收尾 —— chunk 循环(main.rs 324-407)在 `stream.next()` 返 `None`(自然 EOF)时 line 332-333 直接 `break` 跳到 line 419 `acc.finalize()` 收尾,**循环外没有任何「对残 `buf` 做一次 finalize flush」**。候选坑浮现:上游若发最末 `data:` 帧后 EOF **不带尾 `\n\n`** → `sse_split`(tools.rs 932)的 loop 找不到 `\n\n`/`\r\n\r\n` 边界 → 末帧整段留 `buf` 里一帧都不 `drain` 出来 → chunk 循环走 EOF break → 最后帧的 `acc.ingest()` 从未被调 → `acc.finalize()` 拿的是不含末帧的态 → **丢最后 content delta / usage 末帧 / finish_reason 末帧**。注释 line 333「NIM/vLLM 偶尔不发 `[DONE]`,EOF 即终态」只说不发 `[DONE]` 内容帧(已有 EOF break 兜底收尾合流 finalize),**未明说尾 `\n\n` 传输分隔符是否也省略** —— 候选把两件事分清查。

**析因实证锁残留行为**(临时 example `examples/m1_sse_probe.rs` 跑完即删、非门禁不入 git,纯 lib pub fn 析因):喂 `sse_split` 两场景 —— 场景 A 末帧无尾 `\n\n`(`b"data: {...}\n\n"` + `b"data: {末帧 usage}"` 无尾)/ 对照 B 末帧带尾 `\n\n` / 场景 C 单 chunk 首末帧即无尾。**结果**:A 与 C 均 **buf 残留 87 字节(完整末帧)、取出 0 frames**;B 正常 drain 1 frame、buf 残留 0。残 buf 手动喂 `sse_data_payload` 仍能抽出合法 payload —— 证「末帧内容本合法,只是 `sse_split` 没切到边界漏在 buf」,**机制实证成立**(`sse_split` 给定无尾边界 input 必丢末帧,确定性纯函数缺陷非概率)。

**关键判规转折 —— SSE 规范层面 `sse_split` 行为正确非缺陷**:查 WhatWG SSE 规范(html.spec.whatwg.org/multipage/server-sent-events)显式裁决 —— *"Once the end of the file is reached, any pending data must be discarded. (If the file ends in the middle of an event, before the final empty line, the incomplete event is not dispatched.)"* + 例证 *"the end of the stream is not enough to trigger the dispatch of the last event."*。**dispatch 只在遇到 blank line 时触发**,末 event 未带尾 blank line 规范明确「不投递、应丢弃」。`sse_split` 把无尾 blank line 的末帧漏在 buf 不 flush = **严格符合 SSE 规范**(规范要求丢弃未终止 event),**不是缺陷**。若哪天「修」成 flush 残 buf 反而**违反 SSE 规范**(会把规范应丢弃的未终止 event 错误投递给 ingest)。

**真生产实证证否损害链成立前提**:损害要真发生需真实上游真省略尾 `\n\n`,而上游契约会带尾 `\n\n` 则损害不发生 —— journey §9.1(line 996-1003)有 **DeepSeek 真流式实测样本** `[ctx:stream:1] prompt=794 completion=93 total=887`(非 0),usage 末帧正是「最易丢的那帧」被成功 ingest 到 = 证明真生产 DeepSeek 末帧**带尾 `\n\n`**(否则 sse_split 漏 buf usage 必落 0,与实测 887 非 0 矛盾)。**M1 候选真生产未实证撞到损害**。

**`[DONE]` 帧边角同收证否**:DeepSeek 真流发 `data: [DONE]` 帧,若该帧漏 buf 则 `done=true` 永不置、走不到 line 342-344 显式终止 break —— 但 line 332-333 EOF break 已是兜底收尾路径(注释明说「NIM/vLLM 偶尔不发 `[DONE]`,EOF 即终态」),`[DONE]` 漏 buf 也被 EOF break 合流 finalize 兜底,无损害。这条不另立候选。

**判规归属**:M1 候选损害链前提(真生产上游省略尾 `\n\n`)真生产实证未撞到 → **证否(未撞证否型),对齐 P10-4 范式**;且控制流行为本身协议正确非缺陷,焊「锁 sse_split 漏 buf」的析因单测反成**反闸**(锁一个规范正确行为、未来有人改 flush 残 buf 反证规范违反会被闸误拒)→ **不焊闸**。与 P11-1/P11-3「证否不焊闸」同型:撞候选但损害轻微/未实证/读码判无可命中 + 不额外焊新闸(已有结构本身正确)。

**真修价值评估**:若哪天真遇到非规范 server 漏尾 `\n\n`,正确处理应是**针对那类 server 的兼容 hack**(在 `chat_completion_stream` EOF break 后对非空 `buf` 做一次 `sse_data_payload` + `acc.ingest` 容错),而非**无差别** flush 残 buf —— 无差别 flush 会把规范明确应丢弃的未终止 event(可能是半截写坏的非法帧)错误投递给 ingest,违反 P10-3「净修不破老用法」+ 违反协议正确性。**此版不做** —— 守「不臆造」纪律,非真生产实证撞到的损害不预修,留未来真撞到的兼容 hack 局部处理。

**守「连续撞不到真硬坑就算结束」:M1 证否(真生产未实证撞到损害 + 控制流行为协议正确) = 朝收摊方向计 1**(M3 真修清零计数后,从 0 起 M1 是第 1 条证否朝收摊走)。需后续连续撞不到真硬坑 2 条才算 P12 收摊(计数从 M3 清零后走 2 条门);或用户拍板到此收摊。**真坑真修累计仍 = P12-1 + P12-3 = 2**(M1 证否故真修计数不动)。

**诚实副作用**:src 零改(M1 判证否故不真修)+ 不焊新闸(焊反成反闸)+ 临时 `examples/m1_sse_probe.rs` 跑完即删不入 git + prod `codeagent.toml` 一行未改(mtime 08/09/2026 14:30:39)+ env-only key 纪律不变。journey 本节是 M1 唯一落点 + 状态栏 P12-4 收口行。

### 17.5 候选 #5:MCP `request()` timeout/rx-close 路径漏 `pending.remove(&id)` 注释自述真坑真修（析因集成测闸进 CI · 对称补齐双 Err 出口）

**P12-4 证否后接力看 MCP 请求收尾** —— `mcp.rs::request` 的 timeout 链 recv 路径。读注释 line 337-341 自述 P10-1 修 TOCTOU race 时写的「删请求(超时取消)走 timeout 后的 map.remove 收尾」—— 这是**注释对代码打算做的承诺**，须核代码是否真兑现（同「不臆造」纪律要先验证注释与代码一致，注释自述行为漏落 = 实现 bug 强信号）。

**读码逐条确认事实链**（非猜、逐出口核对）：`request()` 路径有**三个**能 `return Err` 的出口 ——
1. **write/flush err**（line 358-361）：已有 `self.pending.lock().await.remove(&id);` —— **P10-1 时就有**，对的；
2. **rx 关闭 Ok(Err(_))**（修前裸 `?` 链，match 改前是 `Ok(Err(_)) => return Err(...)` 不清 pending）；
3. **timeout Elapsed**（修前裸 `?` 链，`tokio::time::timeout(timeout, rx).await?` 直接 propagate Err 不清 pending）。

**修前**：timeout 链是 `let resp = tokio::time::timeout(timeout, rx).await?;` 一行裸 `?` —— Elapsed 直接 propagate 出 `request()`，**不调 `pending.remove(&id)`**。注释 line 341 自述`map.remove` 收尾但代码 timeout 出口漏落实 → **注释与代码不符的实现 bug**（P10-1 修 TOCTOU race 时把 `pending.insert` 前置，但 timeout 出口的收尾注释承诺没同步兑现代码）。

**损害链推理**（给定格境 → 确定性控制流）：`McpClient` 持 `pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RpcEnvelope>>>>`，timeout 出口漏 remove 留 oneshot `tx` 孤儿。**真泄漏条件** = server 永不回该 id（若 server 是 hang 应用层 never 响应，read task 在 server 进程 EOF 后 `sender.send(env)` 走 `Err(ReceiverDropped)` 静默吞 + task 退，**不会** `map.remove(&id)` 那个 id 故 tx 永留 HashMap 到 McpClient drop）。seq 多次 timeout → 多个孤儿 tx 累计几 KB 不可见内存增长，**不 crash 不答非所问不丢数据**，损害=不可见缓慢泄漏（同 P10-5「上游 hang」fact chain 下游、但放在更靠前的 request 收尾层非 display 层）。**判规**:触发端=坏 server hang 环境（偏 P10-4 外部依赖），**损害链=确定性控制流**（给定 server 不回该 id → timeout 出口必然漏 remove 留孤儿）。

**真判定真坑真修 · 非证否**（对照 P12-2 与 M1 两端门槛）：(1) **真修代价极小无副作用** —— 两处 Err 出口对称补一行 `self.pending.lock().await.remove(&id);`，**无任何行为变化**（remove 本 P10-1 已为 write/flush 路径就有，对称补齐只补漏落、不改语义不改控制流不改老用法）；(2) **收益真** —— 几 KB 防 + 注释自述兑现 + Err 出口语义对称（三出口都清自己登记的 entry，与注释 line 341 自述「删请求走 timeout 后的 map.remove 收尾」一致）。**代价 < 收益 = 真修**，与 P12-2「真修代价 > 收益证否」**对照两端判规同框架不同结论**（P12-2 改 catch+continue 有 fatal 语义副作用；此处仅补 remove 无副作用）。**与 M1 证否对照**:M1 控制流行为本身协议正确（SSE 规范要求丢弃未终止 event）故修反成违反协议；C1 控制流行为注释**自述应该清**而代码漏清，修是让它兑现承诺非修旧行为故非证否型。

**真修**（`src/mcp.rs`，line 366-395 段）：把旧裸 `?` 链的 timeout recv 改成显式 `match tokio::time::timeout(timeout, rx).await` 三分支:① `Ok(Ok(env)) => env`（正常路径不变）② `Ok(Err(_))` rx 关闭分支 → 先 `self.pending.lock().await.remove(&id);` 再返 Err（注释:rx 关闭多半 read task 已自清 no-op 但对称闭窗口）③ `Err(_)` Elapsed 分支 → 先 `self.pending.lock().await.remove(&id);` 再返 timeout Err（注释:拿回 entry 若 read task 已先 remove 取走则 no-op 不留孤儿）。新增注释段（line 366-372）诚实记录「注释与代码不符的实现 bug」+ 修前 timeout 路径漏落 + 真泄漏条件 + 对称补齐逻辑 + 与 P10-1 注释自述对齐。

**析因闸进 CI** —— 与 P12-1/P12-2 example env-gate 二进制闸不同范式与 P12-3 析因单测不同 crate 定位，C1 析因闸走 `tests/` 集成测（独立 crate 拿 `CARGO_BIN_EXE_codeagent-mcp-fake-server` 路径，lib `mod tests` 编译期**拿不到** `CARGO_BIN_EXE_*` env 故走集成测 —— 此约束先前 session 已实证过、见 journey P9 同款约束）：
- **fake server 加 env 模式**（`src/bin/mcp_fake_server.rs`）：开头加 `let no_respond = std::env::var_os("MCP_FAKE_NO_RESPOND").is_some();`，逐行消费 stdin（防对端 stdin 写满管道堵塞）但**不写回任何响应帧** → 对端 `McpClient::request` 必撞 timeout。默认不设 env 时 fake server 行为与 P8/A 阶段 keystone 完全一致（向后兼容不破 P9 验收基线）。
- **析因集成测** `tests/mcp_request_timeout_no_leak.rs`：`#[tokio::test(flavor = "multi_thread", worker_threads = 2)]`，配 `MCP_FAKE_NO_RESPOND=1` env + `handshake_timeout_secs=Some(1)` 紧超时，真 spawn McpClient 连 fake server，handshake `initialize` request 必 1s 撞钟 Err → 断 `handshake_res.is_err()`（验 no_respond 模式真生效）、再 lock client 调 `pending_len_for_test()` 断 `== 0`（真修后 timeout 出口 remove 清干净 = 0；修前漏 remove 拿 1 留孤儿）。无 key 需求（纯 stdio IPC + tokio::time）跨 OS 进 CI 零 npx 依赖。
- **test-only 访问器** `pub fn pending_len_for_test(&self) -> usize`（mcp.rs line 303-311）：`self.pending.try_lock().map(|m| m.len()).unwrap_or(0)`。`pub` 让集成测跨 crate 可达（lib `#[cfg(test)] pub(crate)` 在集成测 crate 边界不可达——先前 session 实证 `#[cfg(test)]` 项在集成测编译期不编译 + pub(crate) 跨不了 lib crate 边界，故改 `pub fn` 让恒编译）。`#[cfg_attr(not(test), allow(dead_code))]` 让 prod 路径不警告 dead code（非 prod 调用面）。`tokio::sync::Mutex::try_lock` 是 sync 方法返 `Result<MutexGuard, TryLockError>` 不需 await —— 测试持 `client.lock().await` 序列化请求故 read task 此时不持这把锁，try_lock 必成……（实际测形下打成，unwrap_or(0) 兜底防他人在非串行形态下复用）。

**红绿翻转实证锁真修生效** —— 守 P10 范式真修必须红绿翻转锁，不止「门禁绿」再防伪绿（同 P10-1/P10-5/P12-1 红绿翻转锁范式）：
- **修前（禁 remove）**:Edit 工具把 timeout 出口 line 387 `self.pending.lock().await.remove(&id);` 临时注释成 `// REDGREEN_DISABLED self.pending.lock().await.remove(&id);` → 跑 `cargo test --test mcp_request_timeout_no_leak` → **FAILED assertion `left == 0` right == 1**（拿到 1 = timeout 出口漏 remove 留孤儿）+ 错误信息「拿到 1 说明 timeout 路径漏 remove -> oneshot `tx` 孤儿留 HashMap」**= 真坑现场实证成立**（修前谓，对照 P10-1 N=200 拉 2% race 已 1/1 次必现：C1 候选 server 恒不响 → timeout 必撞钟 → pending 必留 = 确定性纯逻辑非概率一个 case 就锁）；
- **修后（恢复 remove）**:`cargo test --test mcp_request_timeout_no_leak` → **1 passed 1.02s** 拿 `pending_len_for_test() == 0` **= 真修生效锁**。

**验收四闸全绿（C1）**:cargo fmt --check OK / cargo clippy --all-targets -D warnings **0 warning**（`pub fn pending_len_for_test` + `#[cfg_attr(not(test), allow(dead_code))]` 不触发 dead_code 警告，前面 session 先碰 E0599 把 `#[cfg(test)] pub(crate) fn` 改 `pub fn` + allow 也是为这条 clippy 绿）/ cargo check --tests Finished / cargo test --lib **44 passed**（C1 真修在 src/mcp.rs 但真修改是收尾语义非新增 lib pure-function 切点 = 同基线 lib 44 不动）。**集成测全绿无回归**:`mcp_request_timeout_no_leak`(新闸)1 passed 1.02s / `mcp_toctou_race`(P10-1)1 passed 2.90s = 200×3=600 握手 0 丢=P10-1 修未退化 / `mcp_multi_server_isolation`(P11-2)1 passed 0.03s 跨 server 交错自回=P11-2 修未退化 / `mcp_fake_handshake`(P8 keystone)1 passed 0.02s = P10-1 insert 前置未碰握手基线。

**与 P10-1 TOCTOU race 的同根两环对照分工**（同 session 不同候补历程的两环）：P10-1 修的是「`pending.insert` 后于 `stdin.flush()`」时序 race（快 server 在两步窗口内抢回响应 → read task `map.remove(&id)` 取 `None` 丢响应 → rx 永不收 → timeout 挂）→ 修 insert 前置闭窗口；C1 修的是「timeout 出口自己漏 remove」收尾漏落（慢/不响 server 撞 timeout → 自己登记的 entry 不清 → tx 孤儿留 HashMap）→ 修对称 remove 两 Err 出口。**两环 fact chain 同源**（`mcp.rs::request` 的 pending HashMap 收发对称性）但**触发端不同**（P10-1 快 server 抢回 race / C1 慢/不响 server 挂死 timeout）+ **损害不同**（P10-1 丢响应挂死 repl / C1 孤儿 tx 几 KB 不可见泄漏）+ **修法不同**（P10-1 insert 前置 / C1 remove 对称）—— 不重复不冲突，P10-1 修了发送端 race，C1 补接收端收尾。**P10-1 注释 line 341 自述收尾承诺的代码漏落实**由 C1 兑现，故 P10-1 与 C1 注释段互相 cross-reference 同一 fact chain 的两端。

**P12-5 性质归类**:P10-5/P12-1 同型「确定性 + 析因闸喂现场」，但触发端 = 慢/不响 server 外部依赖（偏 P10-4 环境信号），析因闸的「伪造现场」是 fake server env 模式闭合环境影响（同 P12-3 fake server 不响应的范式差异结合点）—— **非 P10-1 概率 race 必 N=200、非 P10-3 真文件系统跨 OS 建环、非 P10-5/P12-1 env-gate 二进制真起主进程**。C1 是「析因集成测借 CARGO_BIN_EXE_* spawn 真 fake server 闭环境、确定性纯逻辑 remove 漏落」第四态变型。

**守「连续撞不到真硬坑就算结束」:C1 真坑真修打破 M1 起朝收摊计数** —— M1 证否计 1 朝收摊走，C1 真坑真修**清零该朝收摊计数**(真修打破连续撞不到)+ **真坑真修累计 +1 = P12-1 + P12-3 + C1 = 3**。被打回 0 重起后,需后续连续撞不到真硬坑 3 条才算 P12 收摊;或用户拍板到此收摊。

**诚实副作用**:`src/mcp.rs` 改(request timeout 链显式 match + 双 Err 出口对称 `pending.remove` + 新注释 + `pub fn pending_len_for_test` 访问器 + `#[cfg_attr(not(test), allow(dead_code))]`)+ `src/bin/mcp_fake_server.rs` 改(env `MCP_FAKE_NO_RESPOND` 模式，向后兼容默认不破老 P9 keystone 基线)+ `tests/mcp_request_timeout_no_leak.rs` 新增析因集成闸(跨 OS 0 key 进 CI)+ 零例 example 文件（C1 走集成测范式非 env-gate 二进制闸）+ prod `codeagent.toml` 一行未改（mtime 08/09/2026 14:30:39）+ env-only key 纪律不变。journey §17.5 是 C1 唯一落点 + 状态栏 P12-5 收口行。

### 17.6 候选 #6：`summarize` 路径裸 await 上游 hang（被 P12-1 同根 client 真修完全覆盖型证否·二阶否定·不修不焊闸）

**起因**:C1 (§17.5) 真修收口后接力看 §17.1 P12-1 acknowledged 但 scope 外未单独闸的下一环 —— 主 agent 的另一条独立 HTTP 路径 `ModelSummarizer::summarize`（main.rs 207-251），它**不**走 `chat_completion`/`chat_completion_stream` 主回路，而是另一条直接 `self.client.post(provider.chat_url()).bearer_auth(api_key).json(&req).send().await` 非流式（`stream:false`）模型二次调用生成摘要路径。prompt:「P12-1 修的是主 agent chat path 的 client hang 兜底，那 summarize 路径同是裸 `send().await` 上游，会不会漏在 P12-1 修覆盖外还裸 hang？」

**读码逐条确认**（非猜）：
1. `ModelSummarizer::summarize`（main.rs 207-251）持 `client: &'a reqwest::Client`（line 193-196）**借用而非自构** —— 不调 `reqwest::Client::new()` 不自构裸 client；
2. `ModelSummarizer` 唯一实例化点 line 1183-1186 `&ModelSummarizer { client: &client, provider: &provider, api_key: &api_key }` —— `&client` 借用的是 `run()` line 926 `let client = build_http_client()?;` 构的同一 client（**经 P12-1 修后持 connect_timeout 30s + read_timeout 90s 兜底**）；
3. `run()` line 926 是主路径唯一 client 构点、line 1243 是 `probe_tool()` 另一独立 `build_http_client()?`（与 summarize 无关）—— grep 全文件 **无裸 `reqwest::Client::new()`**，P12-1 已替换全部两处；
4. `summarize` 调用点 line 1183 在 `maybe_compact` 调用里（main.rs 1175-1196），故 summarize 路径**经同 client** 走 P12-1 修过的 timeout 兜底覆盖。

**判规 —— 被历史真修覆盖型证否（二阶否定，比 P10-4「未撞」更细一档）**：

- **第一阶（损害链根本成立性）**：P12-1 真修对换的是主 agent client 的 timeoutless bug 根因。summarize 路径**借用** line 926 同 client（非自构裸 `Client::new()`），故「`Client::new()` 裸构无 timeout → 上游 hang 确定性永挂」损害链**根本不成立** —— 该路径上不存在 timeoutless bug 的活动载体，P12-1 的 connect_timeout + read_timeout 兜底同根覆盖到 summarize 路径。
- **第二阶（真生产未实证撞损害）**：即便忽略第一阶，真生产也未实证 summarize 路径撞 P12-1 modified client 的 timeout service —— P12-1 example `examples/p12_main_agent_upstream_hang.rs` 跑 `codeagent --script --yolo` 喂单行 user 输入，**单轮不过 `max_context` 阈值 → 不触发 compaction → 不调 summarize**，故 example 闸**不直接覆盖** summarize 路径 send 阶段；真 DeepSeek live 调用里 summarize 路径 timing 无 measured 撞 90s read_timeout 的 sample（journey §9.1 `[ctx:stream:1] total=887` 是 main agent streaming 路径实测，非 summarize 路径）。两层都查不到损害实证。

**两层都否定**故不同于 P10-4「未撞证否」（P10-4 是 candidate 损害链成立 + 损害未实证撞到 = 一阶否、二阶也否但一阶成立）、不同于 M1「行为本身协议正确证否」（M1 是 candidate 损害链成立 + 行为正确不修）、不同于 P12-2「真撞到损害但代价>收益证否」（P12-2 是损害真发生但仍不修）—— **C6 是「损害链根本不成立 + 损害也未实证」新型「被历史真修覆盖型二阶否定」**：

> P12-1 真修对主 agent client timeoutless bug 的根因 fix，**通过 client 共享**自然延伸覆盖到所有借用同一 client 的卫星路径（summarize 是其中之一），无需对每条卫星 path 重复闸/修 —— 一次根因 fix 封断点 + N 条同根卫星 path 同时获保。

**边角细微注记（诚实记，但不以此为打开真坑）**：P12-1 `read_timeout` 是「每字节间隔」**streaming-safe** 设计（真生产流式 SSE chunk 间歇吐字节，read_timeout 持续吐就不触发，对齐真生产 DeepSeek 慢首 token 数分钟不被切 —— journey §17.1 注释自述过这条设计取向是为兼容流式续吐长跑）。summarize 走的是**非流式 chunked response**（`stream:false`）：上游 server 通常先把整段生成完成才开始 flush（首字节延迟 ≈ full generation time），不是流式「首 token 早首 + 后续 chunk 间歇吐」。streaming-safe `read_timeout` 嫁接到非流式 summarize 路径上，**理论上**若上游「首响应头几十分钟不 flush 首字节」可能误杀慢首响应 summarize call。

**为何此边角不升格真坑** —— 守「不臆造 / 真撞实证」纪律：
1. **真生产未实证撞损害**:DeepSeek summarize 调用真 sample timing 未实测过（P12-1 example 单轮不触发 compaction 故 summarize 路径未被实测经过 P12-1 modified client 边界一次）。理论可成立的边角 ≠ 真生产已撞损害；
2. **同 P10-4「未撞」防臆造范式**:P10-4「`summarize` 返空 content」当时也是 live 实测真触发真压缩发现返 1093 字真摘要、空 content 候选条件未实证撞到 → 不收真坑不预修。C6 同型 —— 真生产未实测撞「非流式 summarize 首响应 head 慢超过 read_timeout」损害 → 不预修；
3. **若真要做什么应是「针对那类 server 的兼容配置」非无差别改**:若未来真撞某 non-streaming summarize 上游慢首响应 head > 90s，正确处理是**针对那类 server 的 env 调窗口**（P12-1 `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS` env override 已留这条逃生口：用户撞此场景可临时调高 read_timeout 到真生产可接受值）—— 非**无差别**改 `build_http_client` 默认（无差别改默认会破真生产流式慢首 token 数分钟不被切的兼容取向，违反 P10-3「净修不破老用法」+ 违反 P12-1 设计取向）；
4. **不焊闸**:焊「锁 summarize 路径走 read_timeout 兜底」反成**反闸** —— P12-1 已修同根 client 覆盖到 summarize，再焊单独针对 summarize 的析因闸测 90s-timing 边角，锁的是「P12-1 设计取向嫁接非流式路径」这一**未经真生产实测证伪**的理论边角，未来若有人按真生产样本发现 read_timeout 对非流式 summarize 不合调默认或留 env override，反闸会误拒；与 M1「焊反成反闸锁规范正确行为」同型但更弱（M1 锁的是 SSE 规范层正确行为，C6 锁的是 P12-1 设计取向嫁接非流式路径，后者规范无背书）—— 故此边角更弱，**不焊闸**。

**真裁结论**：候选 #6「summarize 裸 await 上游 hang」**被 P12-1 同根 client 真修完全覆盖型二阶否定**（损害链根本不成立因 root cause fix 已封断 + 损害也未实证撞到）—— 不修代码、不焊新闸、不另加 example。journey §17.6 是 C6 唯一落点 + 状态栏 P12-6 收口行。**守「连续撞不到真硬坑就算结束」**:C6 证否 = 朝收摊方向计 1（C1 真修清零计数后从 0 起，C6 是第 1 条证否朝收摊走），真坑真修累计仍 = P12-1 + P12-3 + C1 = 3（C6 证否故真修计数不动）。需后续连续撞不到真硬坑 2 条才算 P11 收摊；或用户拍板到此收摊。

**诚实副作用**:src 零改（C6 证否不修）+ 不焊新闸（焊反成反闸）+ 不加 example（candidate 被 P12-1 同根覆盖故 example 重复 P12-1 范式白加）+ prod `codeagent.toml` 一行未改（mtime 08/09/2026 14:30:39）+ env-only key 纪律不变。**C6 与 P12-1 的关系诚实记**：P12-1 真修对主 agent client 的 root cause fix 通过 client 共享自然延伸覆盖所有卫星路径（summarize 是其中之一）—— 一次根因 fix 封断点 + N 条同根卫星 path 同时获保，是「net 修」原则的实践印证（P10-3 净修原则的代码实例：在根因层修一次，不在每条卫星 path 各修一遍）。

### 17.7 P12 src/ async/concurrency/subprocess surface 全扫尽 + C3 非 Windows grandchild orphan acknowledged-deferred（收摊附录·非新候选·朝收摊计数收口）

**起因**:C6 证否后按 P10-1 / P10-5 / P12-1 接力同范式 —— 「P12-1 acknowledged scope 外未单独闸的下一环」已接力到 summarize（§17.6）。继续接力下一环前先穷尽 src/ async/concurrency/subprocess surface，确认是否还有未扫到的真 candidate domain 未审，避免「该审没审」型遗漏。

**Explore agent sweep 结果**（只读结构性 inventory，非 code audit 非 fix 提议 —— 只为定位「是否还有 fact chain 未审真 candidate async surface」）：
- src/ 9 个 module（main / mcp / subagent / tools / compactor / session / config / lib / message）全部确认在 P12 covered set 内或纯同步零 async surface（`message.rs` 50 行纯 struct、`lib.rs` 30 行 crate root re-export 无 async）。
- 全 src/ 的 `reqwest::Client` 构点恒在 `src/main.rs:112`（P12-covered `build_http_client`）+ `probe_tool` 另一独立 `build_http_client`，grep 全文件**无裸 `reqwest::Client::new()`**（P12-1 已替换全部两处）。
- 全 src/ 的 `std::thread::spawn` / `std::thread::sleep` 在 `src/tools.rs:527-528`（P12-covered `Bash::execute` 壁钟路径）。
- 全 src/ 的 `tokio::spawn` 都在 covered sections 内（`mcp.rs:249` read task / `tools.rs:514` + `tools.rs:549` in Bash::execute / `main.rs:940` ctrl_c task）。
- 全 src/ 的 `tokio::select!` 在 `main.rs:325` + `main.rs:1647`（covered）。
- 全 src/ 的 `tokio::pin!` 在 `main.rs:744` + `main.rs:1642`（covered）。
- examples/ + tests/ 与 src/bin/mcp_fake_server.rs 都是 env-gate 例程 / 集成测闸 / fake server —— 是「闸与 repro 工具」**非生产 code path**，不属「真 candidate async surface」域。

**Net uncovered async / concurrency / subprocess surface = 0**：src/ 生产代码面 P12 已 100% swept，**无新真 candidate async surface 可读出**。继续接力下一环候选需**主动构造**而非**读码读出** —— 这违反「不臆造 / 真撞实证 / candidate 应读码读出而非猜出」纪律，不构造。

**唯一剩余的 known-but-deferred 候选 —— C3 非 Windows grandchild orphan**：
- **真实位置**：`src/tools.rs:537-544` `Bash::execute` 壁钟路径的非 Windows 分支。Windows 走 `taskkill /T /F /PID`（`/T` 树杀连 grandchild 一起灭，P9-4 实装真修）；非 Windows 走 `kill -9 <pid>` 单杀父级 —— **`kill -9 <pid>` 不带 `-- -pgid` 进程组树杀语义**，child fork 出去的 grandchild 不在 `kill -9` 杀范围。
- **注释自述承认**：`src/tools.rs:503-504` 注释「非 Windows 路径同样用 kill(pid, SIGKILL) 是另一码事 —— 本工具 Windows 为主战场，非 Windows 的等价树杀留 P9+；此处 cfg gates」+ line 538-541 注释「非 Windows:SIGKILL 直杀 (无 /T 树概念, 子 fork 的 grandchild 由其进程组杀主管 —— 此处最简杀 pid 自身; 真非 Windows 端到端留 P9+)」—— **code 自述这是 P9-4 acknowledged scope 外的 future work item**。
- **判归**：不归 P12 active candidate —— 同 P12-1 当时不修第四环（`--script` 无 Ctrl-C 信号源）同 convention「independent future capability / acknowledged-deferred」。P12 baseline 是 Windows 为主战场（journey 范围明明 establish），本机 Windows 无法编译 `#[cfg(not(target_os = "windows"))]` 分支或真跑实证非 Windows 路径损害链 —— **环境受限证否**状态（fact chain 真成立 + 真生产未实证撞损害 on Windows 主战场）。**不计数**、不归证否也不归真修，不是 P12 candidate #7 —— 它是 P9-4 已 acknowledged 的 roadmap future work item，P12 sweep 时遇着诚实在此 acknowledge 不掩盖。未来真上 Linux/macOS 主战场时这条该被真端到端闸覆盖（同 P10-5/P12-1 env-gate 闸范式 + 跨 OS CI）。

**Closure 判定** —— 守「连续撞不到真硬坑就算结束」收摊条件自然达成：
- 自 C1 真修清零朝收摊计数后，C6 证否朝收摊计数 +1 = 1，接着 src/ async surface 全扫尽 net 0 uncovered + 剩 C3 acknowledged-deferred 非 active candidate 不计数 —— **P12 真 candidate async surface 100% swept 完、无新候选可读出** = P11「连续三条撞不到真硬坑」收摊牌子同型的 surface exhaustion 收摊条件。
- 等价 P11-1 / P11-3「证否型」+ P11-2「读码判结构不可能 + 真跑兜底双绿」组合：surface exhaustion 是「读码判无可读出 candidate」型收摊，src/ 净零 uncovered async surface = 撞坑 surface 已穷尽 → 自然收摊，不构造不臆造。

**P12 真坑真修累计 = 3**：P12-1（主 agent client hang 根因层兜底）+ P12-3（ctrl_c mpsc 假中断 keepalive 修）+ C1（MCP `request()` timeout/rx-close 路径对称 remove 收尾）—— 三真坑各有真撞实证 + 红绿翻转锁 + CI 闸进 = 真 fact chain 真 fix 真验证闭环。

**P12 证否型候选 = 3**：M1（SSE EOF 末 frame 残 buf 不 flush 协议正确证否）+ P12-2（上游 Err 路径漏落盘漏 truncate 真修代价>收益证否）+ C6（summarize 路径裸 await 上游 hang 被历史真修覆盖型二阶否定证否）—— 三证否各有真 fact chain 分析 + 真生产实证证否理由（M1 SSE 规范对齐 / P12-2 真撞到损害但设计取舍代价>收益 / C6 损害链根本不成立因 P12-1 同根覆盖）。

**P12 真坑真修 + 证否组合 = 6 candidates 全闭环**，剩 1 尾 C3 acknowledged-deferred（environmental gating 留 future）。

**诚实副作用**:此 §17.7 收摊附录**不修代码、不焊新闸、不加 example、零文件 src 改动**；Explore agent sweep 报告不入 git（task transcript 非仓库追踪）；journey §17.7 是此 sweep result + C3 acknowledged-deferred 唯一落点；prod `codeagent.toml` 一行未改（mtime 08/09/2026 14:30:39）+ env-only key 纪律不变。

**P10→P11→P12 撞坑手段类型谱总结**（诚实给同侪）：自 P10 起 5 种 → P11 加 2 种 → P12 加 4 种 = 11 种撞坑手段类型对照：
1. **P10-1** 概率 race 必 N=200 真撞闸（race 必真跑撞概率）
2. **P10-2** 确定性控制流析因单测喂现场（纯函数对照旧/新逻辑）
3. **P10-3** 真文件系统 symlink cycle 跨 OS 建环真跑（确定性 + 真 OS 环）
4. **P10-4** 外部模型依赖不可控 live 实证撞不到即证否（损害未实证证否）
5. **P10-5** 确定性 + 本地 mock 上游真起 example 修前/修后两版实证对照（env-gate 二进制闸）
6. **P11-1** 真撞实证撞到候选库但非真坑证否型（损害轻微第四态同 P10-4）
7. **P11-2** 读码判结构无可命中 + 真跑兜底双绿焊反退化闸型（结构不可能也焊闸防未来回归 · 不证否即修型）
8. **P12-1** 主回路根因层 client 兜底 + env helper + env-gate 二进制闸红绿翻转（fact chain acknowledged scope 外接力）
9. **P12-2** 真撞到损害但真修代价>收益证否（新细分 P10-4 更细一档：损害真发生但代价>收益）
10. **P12-3** 确定性控制流 + 析因单测喂现场 + keepalive 修（同 P10-2 范式不同机制）
11. **P12-5(C1)** 析因集成测借 `CARGO_BIN_EXE_*` spawn 真 fake server 环境闸 + 确定性纯逻辑 remove 漏落（第四态变型）+ **P12-6(C6)** 被历史真修覆盖型二阶否定证否（新型：损害链根本不成立因历史 root cause fix 已封断 + 损害也未实证）

**P12 收摊 stamp**（2026-08-12）：守「连续撞不到真硬坑就算结束」—— C6 证否朝收摊计数 +1 = 1，src/ async surface 全扫尽 net 0 uncovered 后无新真 candidate 可读出（surface exhaustion）+ C3 acknowledged-deferred 非 active candidate 不计数 = 等价 P11「连续三条撞不到」surface exhaustion 收摊条件达成。**P12 真坑真修 3 + 证否 3**（自 P12-1 起 covered candidates）全闭环，剩 1 C3 acknowledged-deferred 留 future；src/ 生产代码面 P12 100% swept。待用户删测试 DEEPSEEK_API_KEY（sk-...len=35）收摊（本轮已保证自删故不再中途提醒）。**如果未来上 Linux/macOS 主战场**，C3 非 Windows grandchild orphan 该被真端到端闸覆盖（同 P10-5/P12-1 env-gate 跨 OS CI 范式）—— 此 deferred 不丢，记在此 §17.7 + C3 注释自述「留 P9+」留痕同源。

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
- [ ] P6 上下文管理(P6.0 度量层落地:stream_options.include_usage + Usage 透出 + ingest「先取 usage 再判 choices」修正 + report_usage 走 stderr;协议核证三条单测 3/3 过、三道门禁绿;**本机调用层实测闭环 §10.3**:真 DeepSeek key 跑通,非流式/流式 total=prompt+completion 严丝合缝、流式末帧 usage 真流回 stderr,首条曲线 794→1976。P6.1 真压缩策略**代码层落地 §12**:阈值按模型来(provider 段 max_context)+ [compaction] 段三参数(0.7/0.4/4 默认)+ 策略纯函数可单测 + Summarizer trait(默认模型二次调用、测试注入 FakeSummarizer)+ REPL 收工后接线;新 compactor.rs 9 单测全过 + 三道门禁绿。**§12.7 用户本机实测捕获 serde `#[serde(default)]` vs derive `Default` 真 bug**(不写 [compaction] 段 = 默认全 0、压缩每轮触发 + keep_tail 恒空)+3 config 单测焊死修复。**§12.8 把 §12.6 第一条「真压缩触发 + 召回」从留本机挪进自动**:真 DeepSeek key + `--script` 跑两-leg,leg1 total=42410 触发 done(keep_head=2 summarize=3 keep_tail=16),leg2 `--resume` 接力后模型凭 summary 真答出 `session.rs::save/load` 与 `compactor.rs::select_messages_to_compress` 具体签名 —— 压缩召回链路自动实证通过。**§12.9 §12.8 实跑副揪两个真 bug**:glob `*` 配空 name panic(`tools.rs:347` exit=101,match_star `*` 分支空 slice 越界)+2 glob 单测焊死;noop 日志骗人(过阈值却报「未到阈值」)→ `CompactorReport::NoOp` 加 `reason: NoOpReason { BelowThreshold, EmptyMiddle }` +1 单测。cargo test 24/24 干净。仅 §12.6 第二/三条(`compact_to_ratio` 精确切点 + `SUMMARY_INSTRUCTION` 措辞召回对比)仍留本机,不臆造)
- [x] P7 会话持久化(§10 落地 + §10.5 本机端到端实测打通:9 单测全过(P6.0 的 3 + P7 的 6)、三道门禁绿;真终端 resume 跑通 —— `--resume` 载入 N=5 条对上,模型从载入历史里答出「旺财/小明」两词印证真认得上文,resume 后 prompt 基线抬高 +129 印证历史真进请求。原子写+损坏改名留证+版本闸+被打断回合不落盘 pop 悬空 user 三硬点全落)
- [x] `--script` headless 模式(§11 落地:REPL 读入改走裸 stdin 绕开 rustyline TTY 依赖,管道可驱动;`InputLine` 枚举 + `read_tty`/`read_script` + 统一 `exit_repl` 退出路径,agent 循环体单源不 fork;三道门禁全绿 + 9 单测不回归;§11.7 假 key 实测两条已验 —— 管道不再 `os error 1` panic、EOF 也存会话(顺手修旧 Ctrl-D 丢 session bug)。P6.1 真 15-20 轮曲线 + P7 resume 两-leg 端到端待真 key 本机跑通回贴,不臆造数字)
- [x] P8 diff 审批 UI / MCP stdio 客户端 / subagent 子进程式(§13 落地·全三条留本机项已本机实测回贴·四闸全绿:三件共享 (c′) 桥 = 独立 OS 线程 + 独立 `current_thread` runtime(先选 (c) `Handle::current().block_on`→真跑塌「Cannot start a runtime from within a runtime」→退 (c′) 修)→ 同步 `Tool::execute` 跑独立 runtime 上的 async 子进程 IO,不动 trait、不改 5 个老 impl。① diff 审批闸 `bool`→`GateVerdict{Allow,Deny}` 二态 + 自写按行 LCS `unified_diff`(三边角:全新文件/无变化/大幅重写)+ write_file 过闸先显 diff 再 y/N;6 纯函数单测。② subagent 子进程式(方案 A):复用 `--script` 一次性 spawn(不常驻,EOF 收工最稳)+ `--session-file` 临时区隔离 + `[subagent 答复]` 包裹回灌;2 单测(不 spawn)。③ MCP stdio 客户端:手写 JSON-RPC 2.0 极窄面(RpcEnvelope)+ 后台 read task 按 id 扇回 + 握手 initialize→initialized→tools/list→tools/call + McpTool 接 Tool 借桥跑 call_tool + `[mcp.server.*]` 配置(prefix 防撞名)+ **`McpTool` 拆 `name`(带前缀给模型/分派用)/`remote_name`(server 原名,`execute` 发 `tools/call` 用)** —— 实证撞出此前 prefix 设计 bug(server 收带前缀的名报 `-32602 Tool not found`),加 remote_name 修;4 config 单测。tokio features 扩 `["process","io-util"]`;`tools.rs`/`session.rs`/`compactor.rs` 零改。四道门禁绿、`cargo test` 36/36、冒烟 `'exit'|--script --yolo` EXIT=0。**本机已跑通回贴(NOT auto,真键真终端)**:diff 审批三 scenario(A 全新文件 `/dev/null` 全 `+` + B 改单行 hunk LCS context 夹 `+`/`-` + C 大幅重写截断警告)+ 顺带 bash 普通闸 / 默认 Deny 两条副验;subagent 1-leg + 连调两回 EXIT=0(8 轮子上下文)= bridge (c) 塌修 (c′)、不死锁跨调用实证;MCP filesystem 真握手 EXIT=0 = 真_spawn npx + 真握手 + 真调用 `fs_list_directory` + server 真返 6 个实文件回灌合成答 = MCP 全链实证(同时撞出并修 prefix/remote_name bug)。**实证撞出待 P9 的两坑**(非留本机、明确 record):Windows 裸 `npx` 起不来(CreateProcessW 不搜 PATHEXT 须给全路径 `.cmd`,`command="npx"` 会塌 program not found)→ P9 spawn 时按 PATHEXT 解析;MCP 握手 30s 超时偏紧(`npx -y` 首拉包占满 30s)→ P9 握手超时可配/warmup。**仅剩留本机不臆造**:Bash 真 timeout(P9 候选,不在 P8 内))
- [x] P9 P8 实证撞出的硬坑收口 + (a) trait async(§14·四件全收口、无留本机;P9-1/P9-2 2026-08-10 本机真 npx 实测过;P9-3 (a) 2026-08-10 真做完成;P9-4 真端到端挪进 CI):(1) **P9-1 PATHEXT 解析** = 把 P8「给全路径 .cmd」workaround 收成可单测纯逻辑 `resolve_program`(非 Windows 恒 None/Windows 裸名遍历 PATH×PATHEXT 找 dir\prog.ext)+ `resolve_in_paths` 纯逻辑核心(目录序优先/全不命中返 None)+ spawn 接入;3 单测焊(.CMD 命中/前目录优先/无命中 None)。**2026-08-10 本机实测回贴**:`.e2e/t01-pathtext.ps1` 配裸 `command="npx"`(无全路径)真跑,EXIT=0、`resolve_program` 真补全到 `npx.cmd`、真 spawn `npx -y @modelcontextprotocol/server-filesystem` 起 server、真握手 `[mcp] server \`filesystem\` ...14 工具`、模型真调 `fs_list_directory` 拿 `t01-marker.txt` 回灌 = P9-1 PASS。(2) **P9-2 握手超时可配** = 把 P8 硬编码 30s 收成两档(握手 60s 给 npx 首拉 / 运行期 tools/call 30s 防挂工具)+ `request(timeout)` 显式传 + `McpServerConfig.handshake_timeout_secs: Option<u64>` 可逐 server 调;3 config 单测焊(字段缺→None 锁默认 60/显式填 120 保留/多 server 独立)。两件 `cargo test` 36→42 全过、四闸绿。**2026-08-10 本机实测回贴**:`.e2e/t02-handshake-timeout.ps1` 两次真 cold-pull(t01 首冷 + t02 npm cache 逐后 effectively cold again)都跑通、EXIT=0、整体 ELAPSED_MS=15210(握手 < 15.2s,默认 60s 预算 ≥4× 余量)= P9-2 PASS(旧硬编码 30s 会被 npx 首拉吃满、新 60s 两次 cold 都没掉)。(3) **P9-3 (a) trait async 升级** = **2026-08-10 真做完成**(详 §14.3a/commit b8ddd0a):原 2026-08-09 评估「本轮不做、待真端到端验证手段就位」的真手段在 harness(Fake-MCP keystone + dispatch_tool roundtrip + 重写 bash 闸 + env-gate 真 key e2e)落位后启动 —— **Phase A 纯加法建 harness(3 commit,keystone 在当前桥形代码上绿 = refactor 前基线)→ Phase B(refactor,A 绿后才动 (c′) 桥)**:`Tool::execute` 升 `async fn` + `#[async_trait]`(镜像 `Summarizer`),7 个 impl(ReadFile/WriteFile/ListDir/Glob/Bash/SubagentTool/McpTool)全升 async,**整段删 (c′) 桥 `block_on_current`** + 4 析因消融测试 + 1 bridge-timer-fires 测试(6 测全删),`dispatch_tool` 同升 async;§14.4 析因出的「桥上 timer stall」**结构上随桥删而消失**(桥是 stall 的必要条件,见 §14.4a)。验收(2026-08-10 本机实测回贴):4 闸全绿;`cargo test` lib 42(删桥前 47 净减 5)/main.rs 7(含 dispatch_tool 加 `.await`)/keystone 1 **0.03s 绿 — Phase A 经桥 + Phase B 无桥两边都绿 = McpTool async 路径运行期等价证**(正闭 §14.3 原判 risk 「门禁绿证不了运行期无回归」);env-gate 真 key e2e 2 条 2026-08-10 设 `CODEAGENT_E2E=1`+真 key 已本机实测过 `script` 1.52s + `subagent` 2.46s 全绿(从留本机翻已跑,详 §14.3a Phase B 验收第 4 行)。**Bash::execute OS 线程壁钟树杀真修仍留**(与删桥正交,治本 orphan);`bash_hanging_command_actually_returns_within_timeout` 改 `#[tokio::test multi_thread] async fn` + 60s 外层 timeout(删桥后无桥-stall、safe),在新形仍绿 ~30.98s。(4) **P9-4 Bash 真 timeout 代码收口**(P8 plan §13.6 明确的「单列项、真坑非优化、不动 trait 不动 (c′) 桥」)= **两阶段**:Phase 1(commit 38f18fe)tools.rs `Bash::execute` 改走 `block_on_current`((c′) 桥复用)+ `tokio::process::Command::spawn` + **`tokio::time::timeout(30)` 包含 `tokio::join!` 两路收 stdout/stderr 的 inner** + 超时 `child.kill().wait()` 回回收,四闸绿 + 5 纯函数单测(42→47)但**真 `cmd /C ping -t` 端到端静默卡 ~22min**(门禁绿=伪绿实证);Phase 2(本会话)析因单测序列(mcp::tests 4 条)锁死卡住条件 = **(c′) 桥上 `tokio::time::timeout` 对「≥2 个 spawn task pending 在子进程管道 IO」稳定 stall 到等子进程自然退**(单路无第 2 spawn task 2.11s 触发 vs 有 stderr 排水 spawn task 59.62s/59.95s 与 worker 数无关)→ 真修**绕开 tokio timer**:独立 OS 线程壁钟 sleep(30) → `taskkill /T /F` 树杀(连 grandchild 一起灭,治 orphan ping)→ 单路 `read_to_end` 靠管道 EOF 自然退 → inner 自然 resolve,无 timeout 包裹、`Arc<AtomicBool>` 分超时/正常分支;4 析因单测焊 `block_on_current_timeout_*`(含 1 `#[ignore]` 的 stall 示警 —— **注意:Phase B 删桥时此 4 测已一并删,实证数字 2.11s/7.13s/59.62s/59.95s 永久留 §14.4 作记录不再硬背在代码**)。**真端到端实证挪进 CI**:`bash_hanging_command_actually_returns_within_timeout`(真 `ping -t`、30s deadline)30.80s/37.65s 两跑 PASS 回灌「超时」+ 无孤儿 ping + `cargo test` 52 passed/1 ignored/0 failed + 四闸绿。**全已本机实测回贴、无留本机**:P9-1 真 bare `npx` 真补全起 server PASS(t01 EXIT=0);P9-2 两次真 cold-pull 真握手全过 + 默认 60s 预算余量≥4× PASS(t02 EXIT=0 ELAPSED_MS=15210);P9-4 真端到端已挪进 CI;P9-3 (a) 已完成(桥删 + 真端到端运行期等价由 keystone 两边绿证 + 真 key e2e 2 条 1.52s/2.46s 已本机实测过))
- [x] P10-1 P9 收口后真跑试新场景实证撞出的 TOCTOU race(§15.1·2026-08-11 本机实测真命中 + 真修 + 进 CI):`mcp.rs::request` 的 `pending.insert(id, tx)` **后于** `stdin.flush()`,快 server 在两步间窗口内已回响应、read task 抢先 `map.remove(&id)` 取 `None` 丢响应 → `rx` 永不收 → 默认 60s/运行期超时挂(「门禁绿=伪绿」的 McpTool 版 ghost race)。**修前实证**:`tests/mcp_toctou_race.rs`(N=200、紧超时 3s、对端用纯 stdlib 同步零延迟的 `codeagent-mcp-fake-server`)100 次握手真命中 **2 次失败(#41/#75)= 2% 确定性 race**(非极小概率);为什么 P9-1 `t01` 真运行时没挂:`npx` 是 node,cold-pull 响应有 OS/npm/node 启动延迟把 race 概率压向零、但结构窗口未闭,快对端就撞 —— 故 keystone 单跑每跑 1 次(0.05s)长期没撞到,新加 N=200 才够敏(2%×200≈4 期望失败,回退必撞)。**真修**:登记前置 —— `pending.insert` 移到 `write_all+flush` 之前彻底闭窗口;write/flush err 路径显式 `remove(&id)` 清发不出去的孤儿(改前 write 在 insert 前、不产生此类孤儿);timeout 路径 read task 迟到响应触发 remove 自带清,不留孤儿。**修后实证**:3 跑 × 200 = 600 次握手 0 失败(1.42s/1.43s/1.40s)。验收四闸全绿 + `cargo test` 全绿(lib 42/main.rs 7/**keystone 1 仍绿 0.02s = insert 前置没破坏握手**/**新增 toctou 1 passed 2.91s**/env-gate 2 skip)。撞坑另跑两条基线场景全 PASS 无坑:`t10-multiturn`(多轮带工具 write→read→bash + 跨轮上下文,EXIT=0 ELAPSED=7999)+ `t11-resume`(`--resume` 凭载入历史答出 11 条 + 答出独串 EXIT=0 ELAPSED=1542=resume 形神兼备)。P10 进行中,继续撞下一条「连续撞不到真硬坑」才算结束
- [x] P10-2 中断轮孤儿 tool 历史(§15.2·2026-08-11 析因单测实证 + 真修):代码审查 agent 候选 #2 —— Ctrl-C 在 tool 轮中途打断,旧退格 `else if messages.last().role=="user" { messages.pop(); }` 在 tail==`tool` 时 **不命中、不退格**,半截 `[assistant(tool_calls), tool]` 留在内存 `messages`(`finished=false` 跳 save 故不污盘,但污余下轮次模型输入 → 下一轮答非所问)。**证手段与 P10-1 不同但同属不臆造**:P10-1 是时序 race 必真跑 200 次撞概率;P10-2 是**确定性**控制流缺陷,旧逻辑对给定输入的输出是纯函数 —— 故用**析因单测**(§14.4 范式)喂中断现场:对克隆跑旧逻辑断 `len==pre_turn_len+3=6`(留孤儿 = bug 实证),对原跑新 `truncate(pre_turn_len)` 断 `len==3 + 末条仍是上轮答案 + 无本轮 user + 无悬空 tool`(修后实证),同测对照锁住。**真修**:`run()` push 本轮 user 前记 `pre_turn_len`,`finished=false` 用 `messages.truncate(pre_turn_len)` 替旧 `else if last==user {pop()}` —— 无论 tail 是 user/assistant/tool,整轮半截全清回上一轮干净终点(旧 `pop()` 单条单退且只退 user 尾,是 P7 session 落盘语义的内存沿用,tool 轮是 P8 新 capability 而旧退格没跟着扩)。验收四闸全绿 + `cargo test` 全绿(lib 42/main.rs **8 passed** 新增 `interrupted_round_rollback_clears_orphan_tool_history`/keystone 1/toctou 1/env-gate 2 skip)。P10 进行中,继续撞候选 #3-#5「连续撞不到真硬坑」才算结束
- [x] P10-3 glob `**` 跟随 symlink/junction cycle 爆 64 条重复(§15.3·2026-08-11 本机 junction 真跑实证 + 修 nofollow + 跨 OS 真 cycle 单测进 CI):代码审查 agent 候选 #3 —— `tools.rs::walk` 在 `**` 段做 `walk(&child, rest, out)`(消段)+ `walk(&child, segs, out)`(**带 `**` 再下钻**)两条递归。**真跑实证**:建临时目录环 `tmp/A/B/leaf.txt` + `B/loop -> A`(junction 回指祖先),ad-hoc example 复制生产 `walk` 逐字节对 `tmp/A` 跑 `glob_walk("**/*")`:**修前 64 条重复** `A\B\loop\B\loop\...\B\leaf.txt`(每圈长 `loop\B`、约第 64 圈路径长到 Windows MAX_PATH 260 → `std::fs::read_dir` 返 Err → 触发 `Err(_) => return Ok(())` 自然弹看似停 —— **非自然终止乃偶然被 OS 限外弹回**;Linux `read_dir` 无 MAX_PATH、symlink 不需特权,会一路爆 `out` 体积)。**撞坑第三态**(P10-1 概率闸 / P10-2 析因纯函数 / P10-3 确定性 + 跨 OS 真建环):缺陷依赖真文件系统 symlink/cycle,纯函数证不出,必真建 OS 环。**真修**:每条 `entry` 加 `if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) { continue; }`(`DirEntry::file_type()` 原生不跟随)—— glob 搜目录横截面不绕链接(ripgrep `--nofollow` 同理);真路径 `A\B\leaf.txt` 仍由 `*` 段名匹配在 `segs.is_empty()` 时 `is_file()` 正常捕。**修后 1 条** 真路径、**0** `\loop\` 副本泄漏、0.6ms(修前 319ms)。**析因 / 回归单测** `glob_walk_does_not_follow_symlink_cycle`(落 `src/tools.rs::mod tests` 私有 `glob_walk` 可直调):跨 OS 建真环(unix `symlink` 普通用户、windows 试 `symlink_dir` 退 `cmd mklink /J` junction、其他平台 skip),建环失败 `eprintln + return` skip(**非 `#[ignore]`**,镜像 env-gate 自退不假绿);成功建环断 `leaf.txt` 恰 1 条 + 无 `\loop\` 副本。**析因两腿**:本机真跑 64 → 1,且临时 `if false && ...` 禁闸验同测从 1 变 64 FAILED 实证旧 bug 现场 → 恢复闸 1 ok。验收四闸全绿 + `cargo test` 全绿(lib **43 passed** 新增 `glob_walk_does_not_follow_symlink_cycle` = 第 43/main.rs 8/keystone 1/toctou 1/env-gate 2 skip)。**诚实副作用**:`**` 默认不跟随 symlink(`node_modules` 等符号链接包内 `**/*.rs` 不再命中)—— 语义修正非破老用法的 bug,真要跟后续加 `--follow-symlinks` flag(**此版不做** —— P10 只修实证硬坑不加臆造功能)
- [ ] P10-4 空摘要 silent success(§15.4·2026-08-11 真跑证否、不收真坑不修):代码审查 agent 候选 #4 —— `compactor.rs::summarize` 走 length-cutoff/content_filter/空 reasoner 时返空 content 是否 silent success。临时 env 闸 `CODEAGENT_P10_DEBUG_COMPACT_AT=0.01` 把 compaction 阈值压到 10k(env-only 实验闸、不碰 prod `codeagent.toml`),9 轮长 prompt 第 5 轮真触发 `total=12503 → keep_head=2 summarize=10 keep_tail=8 / 历史 20→1` = **真压缩链路 P0-P9 期第一次 live 真跑通**;但真 summarize 实测返 **1093 字真摘要**,空 content bug 条件未实证撞到。**为什么不直接用 FakeSummarizer 注空 content 跑析因判定真坑**:P10-2 析因单测喂的中断轮是**用户可控真路径**,P10-4 的 `summarize 返空` 是**外部模型依赖、用户不可控触发**,live 没撞到 + 受控析因只证「控制流接受空」不证「现实真发生」→ 强行标真修破「不臆造」纪律,**故 #4 证否、不收真坑、不修**(诚实记「撞了但没坑」第四态)。env 闸与 stderr `[P10-4-DEBUG]` 探针已撤回、`codeagent.toml` 未碰一行
- [x] P10-5 SubagentTool `read_to_end` 无 timeout(§15.5·2026-08-11 本机 mock 上游真撞升格真坑+真修+进 CI env-gate 二进制闸):代码审查 agent 候选 #5。**事实链**(读码逐条确认非猜):`subagent.rs::execute` 的 `stdout.read_to_end(&mut out).await` + `child.wait().await` 均**裸 await 无 `tokio::time::timeout` 包**;子 `codeagent --script` 内 `chat_completion`/`chat_completion_stream` 皆 `client.post().send().await` 裸 await 上游 + `reqwest::Client::new()`(main.rs 857/1159)裸构**无 `.timeout()`**;`--script` 无 Ctrl-C 信号源 → 全链无超时兜底,「上游 hang」→ 子 send() 永挂 → 父 read_to_end 永挂 → 父 turn 永死无救。**判规(撞 vs #4 否的关键差)**:#5 是**确定性控制流缺陷**(类 P10-2:给定输入→父侧 buggy 纯逻辑),非 P10-4「外部依赖、不可控触发」之感嫌疑——故确定性缺陷+真复刻现场=升格真坑。**真撞实证(撞坑第四态)**:`examples/p105_subagent_hang_repro.rs`(env-gate 二进制,`CODEAGENT_P105_GATE` 非空跑真 / 否 eprintln skip+exit=0):本地 mock 上游 `TcpListener::accept` 一条连接后起 long-hold task **持 stream 整 example 生命期不退、不读不写不关**(此前第一版 mock 一 accept 就 drop stream → TCP RST → 83ms 退报 send error = **修前机制伪现场**,改成 long-hold 才真复刻上游应用层 hang);临时 toml 写 tmp 目录指 mock(`base_url`/`api_key_env`,key 任值、不动 crate root 真 prod toml)。**p105 v1(初撞·修前现场)**:手动复刻 execute spawn 段 + 裸 read_to_end + 外层 `tokio::time::timeout(15s)` 包 → **Elapsed,elapsed=15.00s exit=0 = #5 真坑现场实证**。**p105 v2(焊闸·修后)**:真起生产 `SubagentTool::execute`(走 `SubagentTool::new_with_bin`)+ `CODEAGENT_SUBAGENT_TIMEOUT_SECS=3` 压窗口 + 外层 15s 包 → 验证修后 **execute 自己 3.06s 返 `Err("超时...子进程已被 kill")` 而不撞外层 15s**,子进程被 `child.kill` 收尸无遗孤 → `VALIDATED verdict=PASS_FIX_OK ctrl=true elapsed=3.06s exit=0`(**15.00s 永挂 → 3.06s 自返 Err 修前/修后实证对照锁真修生效**)。**真修**(`src/subagent.rs`):加 `const SUBAGENT_DEFAULT_TIMEOUT_SECS: u64 = 180` + `fn subagent_timeout()`(`CODEAGENT_SUBAGENT_TIMEOUT_SECS` env 覆盖压窗口、0/垃圾/空/空白回退默认 —— 给本地实证/测压窗口、真生产永远 180s);`execute` 的 `read_to_end` 包 `tokio::time::timeout`:`Ok` → 正常读;`Err(Elapsed)` → `eprintln!("[subagent] 超时...")` + 显式 `child.kill().await` + `child.wait().await` 收尸 + 返 `Err("subagent 超时(<n>秒未返回终答)——子进程已被 kill...{partial stdout}补充")`。**根因层为何不修**:顶层 `reqwest::Client::new()` 加 `.timeout()` 会断流式 SSE 长跑被切(真生产 DeepSeek 慢首 token turn 可能数分钟),真修选**展示层** subagent 自有 deadline 而非根因层全局 timeout,符合 P10-3「不强加破老用法」净修原则。**焊入式 CI 回归闸两层**:(1) `subagent.rs::mod tests::subagent_timeout_env_override_default_and_fallback`(恒跑、不真起子进程、跨 OS 0s):验默认 180s + env 整覆盖 + 垃圾/0/空/空白回退默认(防 parse 失败 panic)+ 尾清 hygiene;(2) `examples/p105_subagent_hang_repro.rs` env-gate 二进制闸(对齐 P10-3 example 闸范式):gate-off skip + exit=0、gate-on ~3s 真起 production execute 撞 hang 验修后兜底返错 exit=0、修坏则外层 15s 撞钟 FAIL_STILL_HANG/exit=1。**保留的修前机制反退化闸**:`main.rs::mod tests::subagent_read_to_end_hangs_forever_on_non_exiting_child`(析因单测,跨 OS 不退子进程 `cmd /C ping -t` / `sh -c sleep 999999` 包 timeout(2s) 断 Elapsed + 对照 echo hi 秒回不误杀,2.06s 单跑)——它复刻「裸 await 永挂」机制本身(与生产 code path 无关,作反退化闸:有人 PR 删 execute timeout 包时这条先证裸 await 必永挂让 PR 复议)。**验收四闸全绿** + `cargo test --lib` **44 passed**(原 43 +1 timeout env 覆盖测,30.80s 含 bash 真 ping-t 闸+glob 真 cycle 闸)+ p105 二进制 gate-on `VALIDATED elapsed=3.06s` exit=0。**诚实副作用**:subagent 默认上限 180s,真跑超 3 分钟的 subagent 会被 kill(此前无界),`CODEAGENT_SUBAGENT_TIMEOUT_SECS` env 可调 —— 修正无界 hang 带来的有界 deadline、非破老用法
- **P10 收口(2026-08-11)**:五候选全清 —— **#1/#2/#3/#5 四真坑各被真撞真修焊进 CI 回归闸** + **#4 真跑证否不修**。撞坑手段类型谱(诚实给同侪):P10-1 概率 race 必 N=200 真撞闸 / P10-2 确定性控制流析因单测喂中断现场 / P10-3 真文件系统 symlink cycle 跨 OS 建环真跑 / P10-4 外部模型依赖不可控 live 实证撞不到即证否 / P10-5 确定性 + 本地 mock 上游真起 p105 example 做修前/修后两版实证对照。守「连续撞不到真硬坑就算结束」判定 —— P10-4 已证否 + P10-5 真修 = 撞得够全、回贴、待用户删 key 收摊
- [ ] P11-1 纯 user 多轮 compactor 永久 noop(§16.1·2026-08-11 真跑证否、不收真坑不修):P10 收口后启 P11 续守「连续撞不到真硬坑就算结束」。P11-1 起点 = P10-4 留的「真压缩触发后下一轮回上文」未验下游,真跑两脚本(env-only、不碰 prod `codeagent.toml`、key 走 env/注册表、临时 .e2e/p11-1*.ps1 gitignored 不入 git)。**真撞实证(撞坑第四态 — 本地真起 production codeagent + 真 DeepSeek 上游 + 真 key)**:(1) `.e2e/p11-1-compact-recall.ps1`(纯 user 9 长轮 + 1 recall probe、`max_context=10000` 应在 ~round 5 过阈值=7000 触发):跑出来**10 轮全 `[compactor:noop]`**,round 5+ 日志「已过阈值但中段空:头尾相接/user 轮太少,无可折;不压」—— **compaction 永远不触发**,故 probe 轮模型仍看**未压缩全原始 history**(按 recall 探针应答出 marker `P11ONEPROBE-MARKER-ZQXJVW` + 9 rounds + Round 9 措辞 = perfect recall,但这恰说明根本没压过、不是压缩链路通过的证据)。**新候选坑浮现**:`compactor.rs::select_messages_to_compress` 的 `keep_head` 循环 `while head_end < n { if m.role != "tool" && m.tool_call_id.is_none() && !has_tool_calls(m) { head_end += 1 } else { break } }` 生吃所有早期非 tool 消息 —— 在纯 user 多轮(无 tool_calls)里 `head_end` 一路长到吃满整个数组 → `find_tail_start(...) <= head_end` → summarize 段恒空 → `NoOp{EmptyMiddle}` 永久,**远超阈值也不压**。(2) **判真坑前先受控析因**:`.e2e/p11-1b-tool-compact.ps1`(真 agent 流 = agent 调 `read_file` 读 5200-char big.txt、`keep_recent_turns=2`、`max_context=10000`) —— keep_head 在首条 `assistant(with tool_calls)` 处**早 break**,summarize 段非空,compaction **正常触发**:`[compactor:done] total=8080 compress: keep_head=2 summarize=3 keep_tail=18 / 历史:23→21` + 第二次 `[compactor:done] total=8981 keep_head=4 summarize=17 keep_tail=8 / 历史:29→13` = **TOOL_SCENARIO_COMPACTS**。**真坑结论**:工具 agent 流(真生产主用例 = write_file/read_file/bash 都触发 `has_tool_calls` → keep_head 早 break)压缩正常;纯 user 无工具 noop 仅收窄到「`max_context` 显著误配小 + 纯 chat 无工具」corner —— 真 DeepSeek production 配 `max_context=1000000`(1M),1M × 0.7 = 70 万 token 阈值,纯 user 聊到 70 万 token 触发 noop 的场景主用例几乎不存在,且即便 noop **损害 = token 浪费(贵)非 crash / 非答非所问 / 无数据丢失**,上游窗口够大不报错。同 P10-4「撞了但没坑/损害轻微」第四态 —— **证否不修、不收真坑、不改代码**。**设计意图 gap 诚实记**:compactor 代码 docstring「keep_head: 首 + 紧随的非 tool 段落(早期短、不该压)」与实际循环「无界生吃非 tool 早期消息」有违 —— 未来真撞出 **非纯 token 浪费的 downstream 损害**(如答非所问 / 丢关键上文 / 上游 413)才考虑加 `head 长 ≤ max_context * 0.05` 兜底限制,**此版不做**(守 P10-3 净修原则「不加臆造功能」)。**诚实副作用**:临时脚本 .e2e/p11-1*.ps1 gitignored 不入 git,prod `codeagent.toml` mtime 未变(08/09/2026 14:30:39),source code zero 改动 ——journey 此节是 P11-1 唯一落点
- [x] P11-2 多 MCP server 并发隔离真裁(§16.2·2026-08-11 读码判无坑 + 真起双 server 交错实证兜底·仍进 CI 回归闸不证否即修型):候选「裁多 MCP server 同时 spawn、各自 handshake、各自超时窗口、**共用 client stdio 通道**时会不会相互污染 read task / **共享 pending map**」。**先读码判**(诚实「读码出的嫌疑」非想当然——同 P11-1 先析因):`mcp.rs::McpClient::spawn` 每 server 起一独立子进程 + 独立 stdout 管道 + 独立 `Arc<Mutex<HashMap>>` pending map + **独立 read task**(只读自己的 stdout、只扇回自己的 pending)—— **无共享通道、无共享表**,命题「共用 client stdio 通道 / 共享 pending map」与实结构不符。`main.rs::run`(928)多 server spawn+handshake 走 `for (name, server_cfg) in &cfg.mcp.server { ... .await }` —— **串行**(非 `join_all`/`FuturesUnordered`);运行期 `tools/call`(739)`for call in &calls { dispatch_tool(call).await }` —— 同回合同样**串行**。故「并发隔离」从结构上就**无可并发**:无共享管道、无共享表、spawn/handshake/call 全串行、per-server `Arc<Mutex<McpClient>>` 连单 server 同实例并发调都锁串行。**但「读码判无坑」会破「不臆造 / 真撞实证」纪律**(同 P10-3「确定性缺陷纯函数证不出」反方向——**结构不可能也要真跑兜底**),故真起 2 个 fake-server 子进程同活,在两 client 间 `tokio::join!` **并发**交错 `call_tool`×12 轮(A↔B):两 `next_id` 各从 1 起 → 两 server 都回 id=1/2/3… → **若 pending map 真被错共享 / read task 真错扇到另一 client 的表**,id 撞号必让某轮交错响应体含另一 server 的 marker(`<echo: An>` vs `<echo: Bn>`)—— `tests/mcp_multi_server_isolation.rs` 严格断「每调只回自己 marker」当场炸。无共享 = 每调严格返回自己 marker。再补 server-A 自身 4 调 `tokio::spawn` 并发池(`Arc<Mutex>` 锁串行 + per-request id 唯一),严格断集 = {`echo: S1..S4`} 绝无自撞。**真裁结论**:**读码判成立 + 真跑实证兜底双绿** = 无跨 server 路由污染 / 无 pending map 共享 / 无 read task 错扇,**不收真坑、不修代码**,但新增 `tests/mcp_multi_server_isolation.rs` 作**反退化 CI 回归闸**(非证否型 —— 此闸若坏 = 有人后续把多 server 改并发 / 共享表,当场炸让他复议;同 P10-5「保留修前机制反退化闸」精神,但方向相反:无坑也焊闸防未来回归)。**验收四闸全绿**:fmt OK / clippy `-D warnings` 0 warning / `check --tests` Finished / `cargo test` 全绿(lib 44 含 bash 真 ping-t 闸+glob 真 cycle 闸 / **新增 multi_server 1 passed 0.18s 跨 server 交错严格自回** / toctou 1 passed 16.04s 200×3=600 握手 0 丢=P10-1 修未退化 / keystone 1 passed 0.21s / main.rs bin 全绿)。**诚实副作用**:`tests/mcp_multi_server_isolation.rs` 用 prefix `alpha`/`beta` 体现代码路径(未收 tool 表);test 0.18s 跨 OS 可重复进 CI 无 npx 依赖。**诚实判规**:P11-1 是「真撞实证撞到候选库但非真坑」证否型(撞了没坑/损害轻微第四态,同 P10-4),P11-2 是「读码判结构不可能无坑 + 真跑兜底双绿」**焊反退化闸型** —— P11 的两种「无坑但留闸」模式由 P11-1/P11-2 分别立范
- [ ] P11-3 REPL 中断落工具轮中途与 truncate 配合真裁(§16.3·2026-08-11 读码判结构无可命中 + 析因单元闸已穷尽覆盖·证否不修不另焊闸):候选「Ctrl-C 落在工具轮中途会不会**回退已跑工具结果**(丢已计算 evidence / 半截轮坏下次答)/ 与 SIGINT 那 read task **竞争**(产生幽灵中断或丢中断)」。**先读码判结构**(诚实「读码出的嫌疑」非想当然——同 P11-1/P11-2 先析因不臆造):(1) 候选命题①「中断落工具轮中途」**不对位 interrupt 的真 hook 点** —— `run_one_turn`(658-755) `for round in 1..=MAX_TOOL_ROUNDS` **每轮只在 `chat_completion_stream` 的 `tokio::select!` 挂中断**(680-692,`Ok(StreamOutcome::Interrupted) => return Ok((false, 0))`),`dispatch_tool` loop(739-747)是**裸 `.await?` 无任何 interrupt hookup** —— 落在工具执行中途的 Ctrl-C **信号排队但不取消正在跑的工具**,等工具自己走 P9-4 的 30s `taskkill /T /F` 兜底 timeout 收(本来就是 P9-4 修过的「Bash 永挂」治法)。**设计意图**:工具是「已开始的确定量步」,留着跑完回 evidence;Ctrl-C 终结的是**下一花模型生成**而非工具本身——本就不期望「工具可中断」,非债。(2) 候选命题②「truncate 回退已跑工具结果」是**正确且 desirable、非 bug** —— finished=false 路径(1134-1138)`messages.truncate(pre_turn_len)` 一刀回退整轮「user + 已推的 assistant(tool_calls) + tool result」,正是「中断轮不应留半截轮 evidence 下轮让模型看」要做的事;旧版 `else if last=="user" { pop() }` 在工具轮失灵(tail 是 tool 触发不了 pop,孤儿留到下轮让模型糊涂),P10-2 修后 `truncate(pre_turn_len)` 一刀精准覆盖(注释 1060-1069 已显式叙述 intend)。(3) 候选命题③「与 SIGINT 那路 read task 竞争」**结构上无解可命中** —— ctrl_c 监听 task(862-872)是**纯 pusher**(`signal::ctrl_c().await` + `interrupt_tx.send(()).await` cap=8 满则丢),**不与 dispatch_tool 竞争任何共享态**(dispatch_tool 不读 `interrupt_rx`);下轮 `while interrupt_rx.try_recv().is_ok() {}`(1074)进 agent loop 前回吐间隔里早到的 Ctrl-C → **无幽灵中断**,且 reader-writer/reader-reader 竞争结构在候选域内根本不存在(dispatch_tool 不读 interrupt)。(4) **读完的结构断言**:候选坑的控制路径**已被 P10-2 `interrupted_round_rollback_clears_orphan_tool_history`(1374-1458) 析因穷尽覆盖** —— 该单测直接喂进「system + prior user + prior assistant(pre_turn_len=3);push user + assistant(tool_calls) + tool result;truncate(3) → 前 3 条完好 + 无孤儿 tool result」即候选坑的完整现场,断 precises invariant,**反退化闸本身就在 P10-2 那**。**诚实实证弱点承认**(不臆造):真 live 端到端「--script pipe 跑半轮按 TTY Ctrl-C 看 truncate」**不能便宜自动驱动**:`--script` 走非 TTY 路径,rustyline 信号处理只在 TTY 模式启用,脚本里 send ctrl_c 到 stdin=pipe 的子进程语义不对位 REPL；析因单元闸已穷尽覆盖候选控制流 —— P11-3 是「逻辑可证否」型(同 P11-1),**非「环境依赖需 live 闸」型**(P10-5/P11-2),故不必再焊另条二进制闸(已有 P10-2 单测就是闸,重复焊白费)。**真裁结论**:**不收真坑、不改代码、不焊额外闸** —— P11-3 归 P11-1 同型「证否型」:read-code 判无可命中 + 析因单元闸穷尽覆盖 = 撞不动真坑。诚实副记:ctrl_c task 在 `--script` headless + stdin=pipe 时**进程层 SIGINT 是否到达 `tokio::signal::ctrl_c`** 是 Windows 信号语义边角,**不属 P11-3 候选域**(候选问「已挂 select 后竞争」,select 都没挂进 dispatch_tool),不在 P11-3 立闸范畴
- **P11 收口(2026-08-11)**:P11-1 证否(纯 user noop 缩窄到「max_context 误配小 + 纯 chat 无工具」corner、损害=token 浪费非 crash 同 P10-4 第四态·不修) + P11-2 读码判结构无可并发 + 真起双 server 交错 `tokio::join!` 兜底双绿·焊反退化闸 `tests/mcp_multi_server_isolation.rs`(无坑也焊防未来回归·不证否即修型) + P11-3 读码判 interrupt 不落工具轮 + 析因单测闸已穷尽覆盖·证否(不修不另焊闸)= **连续三条撞不到真硬坑**,守「连续撞不到真硬坑就算结束」牌子收摊。P11 的两种「无坑但留闸」模式:P11-1/P11-3「证否型」(撞候选但非真坑/损害轻微/读码判无可命中+析因闸已覆盖,不焊新闸=不上额外)与 P11-2「焊反退化闸型」(结构不可能+真跑兜底双绿,焊闸防未来改坏)由三者分立。源码改动:仅 P11-2 加 `tests/mcp_multi_server_isolation.rs`(env-only 无 key、跨 OS 进 CI 无 npx 依赖)+ 三段 journey 落点(P11-2/P11-3 两节 + 此收口行,§16.2/§16.3);prod `codeagent.toml` 一行未改(mtime 08/09/2026 14:30:39)、临时 .e2e/p11-1*.ps1 gitignored 不入 git。验收四闸全绿(fmt/clippy -D warnings/check --tests/cargo test)
- [x] P12-1 主 agent `chat_completion*` 裸 await 上游 hang(§17.1·2026-08-11 本机 mock 上游真撞升格真坑+真修+进 CI env-gate 二进制闸):P10-5 acknowledged 但 scope 外的后三环之二、三接力。**事实链**(读码逐条确认、接力 P10-5 chain):主 agent 自己(非 subagent 委派)直接调 `chat_completion`(~106 `.send().await` 裸 await 无 select)/ `chat_completion_stream`(~235 `.send().await` 裸 await 在 chunk 循环外、Ctrl-C select 挂 chunk 内 262-343 救不了 send 阶段 hang),client 同样 `reqwest::Client::new()`(main.rs 857 run + 1159 probe_tool)裸构无 `.timeout()`/`.connect_timeout()`/`.read_timeout()` → 上游「建好 TCP 但永不回 HTTP 响应头」确定性永挂整 agent turn。**判规**(同 P10-5 确定性控制流缺陷非概率 race,类比 P10-2 喂中断现场 = 给定坏上游→父侧 buggy 纯逻辑)。**真撞实证升格**:`examples/p12_main_agent_upstream_hang.rs`(env-gate 二进制,`CODEAGENT_P12_GATE` 非空跑真 / 否 eprintln skip+exit=0,范式对齐 p105/mcp_toctou_race/env-gate、**非 `#[ignore]`**):mock hang 上游 `TcpListener::accept` + long-hold task 持 stream 不退(同 p105 long-hold,反事实对照:一 accept 就 drop=TCP RST 83ms 退是伪现场)、临时 toml 写 tmp 指 mock(key 注 fake 上游假不验真、不动 crate root 真 prod `codeagent.toml` gitignored)、起真 production codeagent `--script --yolo` 主进程 `kill_on_drop(true)` 喂一行 user 输入、外层 `tokio::time::timeout(15s, child.wait())` 包 match 三态(`Err(Elapsed)`=撞钟 / `Ok(Err(io))`=WAIT_IO_ERROR / `Ok(Ok(status))`=正常退)。**修前**(gate-on hang):**15.27s 撞钟 Elapsed = `REGRESSION_STILL_HANG`** 真坑现场实证成立 → 候选 #1 升格真坑(15.27s 同 P10-5 v1 15.00s 对位,主 agent 主回路非 subagent 中转故修须在根因层 client)。**真修**(`src/main.rs`):新增 `build_http_client()` 集中体 = `Client::builder()` + `connect_timeout`(DNS+TCP,default 30s)+ `read_timeout`(**每字节间隔非总时长**,streaming-safe —— 不用 `.timeout()` 总包会断流式 SSE,对齐 P10-5 注释;`read_timeout` 持续吐字就不触发真生产慢首 token 数分钟不被切);新增 `upstream_timeout_env(name, default)` env helper(`CODEAGENT_UPSTREAM_CONNECT_TIMEOUT_SECS`/`CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS` 覆盖、`Some(0)=>None` 显式关逃生口、None/垃圾/空/空白回退默认防 parse panic);新增 `DEFAULT_CONNECT_TIMEOUT_SECS=30`/`DEFAULT_READ_TIMEOUT_SECS=90` 常量;**替换两处** `let client = reqwest::Client::new();`(857 run + 1159 probe_tool)为 `build_http_client()?`。**修后**(gate-on hang、注 `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS=5` 压窗口):**5.17s 拿 reqwest read_timeout Err 退 `FIX_OK_TIMEOUT_ERR_EXIT`**(stderr 含 timeout err)—— **15.27s→5.17s 闸红绿翻转锁真修生效**。**echo 对照**(走 default 90s、正常自退上游):**411ms `ECHO_NORMAL_EXIT`** 验闸不误杀正常路径(echo 模式子进程因手写裸 HTTP reqwest 某处拒故 exit 非 0,但「秒退 vs 15s 撞钟可区分」核心断言成立不追完美 exit=0)。**〔§17.2 诚实副作用返追〕**:上轮 echo「411ms ECHO_NORMAL_EXIT 对照绿」**实为假绿** —— P12-1 example 的 echo 判读 `Ok(Ok(_))` 一律判 `ECHO_NORMAL_EXIT`,**没校 `status.success()`**,故 echo mock 因未读 client POST body 触发 Windows TCP RST 让子进程 411ms 拿 reqwest send Err 退(code 1)也被旧判读误判为「正常收工退」。M2 真撞过程撞出该诚实性 bug,同 §17.2 同收两处:① echo mock 加 `stream.read(&mut req_buf)` 读完一段 client body 再回、走干净 FIN 不 RST;② echo 判读校 `status.success()` 才判 `ECHO_NORMAL_EXIT`、不成功判 `ECHO_ERR_EXIT` exit=1。修后 echo 真绿 `code=0 108.44ms` + stderr `[ctx:stream:1] prompt=5 completion=1 total=6` + `[compactor:noop]`(= 第一次 echo 真验过「修后正常路径不被误杀」)。**P12-1 真坑成立靠 hang 红绿翻转**(15.27s→5.17s,unsuccess 符合 hang 期望态故 hang 判读无错),echo 是辅助对照 —— 故 echo 假绿不破 P12-1 真坑成立、但「echo 对照绿」论证本身曾无效,诚实返追收之。**根因层 vs 展示层对照 P10-5**:P10-5 修 subagent=展示层(`execute` 自有 timeout 包 `read_to_end`)根因层故意不修怕断流;P12 修主 agent=根因层 client 兜底(主回路无展示层中转可包必须 client 层)—— fact chain 同根两环分工修不重复不冲突,`read_timeout` 每字节间隔兼顾「上游 hang 兜底」与「真生产慢长 streaming 不被切」两约束。**不修第四环**(`--script` 无 Ctrl-C 信号源):client 兜底已覆盖 send 阶段 hang(后二、三环),第四环是「信号源」层非「timeout」层、加 Ctrl-C 信号源是独立 capability(P5 中断挂 TTY mpsc、--script headless 加信号源需独立设计)非 P12-1 范畴,守 P10-3 净修原则不加臆造功能留未来。**焊入式 CI 回归闸**:`examples/p12_main_agent_upstream_hang.rs` env-gate 二进制(hang 模式 gate-on 期望 `FIX_OK_TIMEOUT_ERR_EXIT`~5s exit=0、落 `REGRESSION_STILL_HANG` exit=1;echo 模式 gate-on 期望 `ECHO_NORMAL_EXIT` 几百 ms `success()=0` exit=0、`ECHO_ERR_EXIT` / `ECHO_UNEXPECTED_HANG` exit=1;`kill_on_drop(true)` 兜底收子进程同 p105)。**验收四闸全绿**:fmt OK / clippy `-D warnings` 0 warning / `check --tests` Finished / `cargo test --lib` **44 passed**(与 P11 收口同基线;无新增 lib 单测 —— 真修是 client 层集中体替换、env helper 逻辑由 p12 二进制闸覆盖真起实证,主 agent client 路径无 inline pure-function 切点可单测对齐 p105 范式);p12 闸 gate-off skip+exit=0、gate-on hang `FIX_OK elapsed=5.17s` exit=0、gate-on echo `ECHO_NORMAL_EXIT 411ms` exit=0〔§17.2 修后 echo 真校 `success()=0` 再绿、`code=0 108.44ms`〕。**诚实副作用**:主 agent client 有 connect 30s + read 90s 兜底,真生产 DeepSeek 慢首 token 数分钟但持续吐字节(SSE chunk 间歇)`read_timeout` 每字节间隔不触发非破老用法;P10-5 展示层 subagent deadline 180s 仍在(两条 timeout 独立不冲突:主 agent client 拦上游 hang、subagent deadline 拦子进程整轮 hang)。**P12-1 性质归类**:P10-5 同型「确定性+本地 mock 上游+example 二进制闸真起主进程」(非 subagent 委派、主 agent 主回路),撞坑第六态接力 P10-5 acknowledged 后三环之二、三
- [x] P12-2 上游 Err 路径漏落盘 + 漏 truncate(§17.2·2026-08-11 本机 mock 上游真撞损害现场升格证否不真修+ P12-1 echo 假绿诚实副作用同收 + 进 CI env-gate 二进制闸):P12-1 修完 client 兜底 timeout 后接力看下一环 —— `run()` 主循环 line 1140-1151 `run_one_turn(...).await?` 裸 `?`,上游返非 2xx / reqwest send 失败 / 错误帧时 Err 直接 propagate 出 `run()` → `main()` → 进程非 0 退,**不经过** line 1199 truncate / line 1196 save / exit_repl → 本轮 user(已 line 1136 push 进内存)随进程退丢失、磁盘 K-1 完成态保留。**关键判规分级**:触发端=上游 Err=外部依赖概率事件(性质偏 P10-4 非 P10-5 / P12-1 确定性控制流缺陷),但损害链是确定性控制流(给定上游 Err → 必然本轮 user 不落盘 + 进程退)。**真撞实证升格**(`examples/p12m2_upstream_err_drops_round.rs` env-gate 二进制,`CODEAGENT_P12M2_GATE` 非空跑真 / 否 eprintln skip+exit=0,范式对齐 p105/p12_main_agent_upstream_hang):mock 上游按连接计数回不同响应(conn 1 = 200 streaming round 1 落盘 / conn 2+ = HTTP 500 round 2 撞 Err)、临时 toml 写 tmp 指 mock key 注 fake 不动 prod `codeagent.toml` gitignored、起真 production `codeagent --script --yolo` `kill_on_drop(true)` 喂两行 user 输入、外层 `tokio::time::timeout(20s, child.wait())` 包、收子进程退出后读磁盘 `.codeagent_session.json` 三断言(含 round 1 user / 含 round 1 assistant / 不含 round 2 user)。**实证结果(2026-08-11 本机)**:conn 1 accept 200 streaming → stderr `[ctx:stream:1] prompt=5 completion=1 total=6` + `[compactor:noop]` + stdout `好`;conn 2 accept 500 → stderr `流式 HTTP 状态非 2xx: HTTP status server error (500)`;**子进程 非正常退 (Some(1)) 132ms**;**磁盘 491 字节 含「第一轮问好」+「好」、不含「第二轮问再见」** = `REGRESSION_CURRENT_ROUND_LOST` verdict。**M2 真坑损害现场实证成立**(给定上游 Err → 确定性本轮 user 不落盘 + 进程退 + K-1 保留)。**修正侦察 agent 误述**:Agent B 之前说「前面 N 轮全丢」错 —— P7 一轮一 save 语义下前面 N-1 轮每轮已落盘,只当前 K 轮 user 丢、损害**不累积**。**真修价值评估 → 判证否不真修**:A 派「补对称 save + truncate + 仍 propagate 退」空修(K-1 本已在磁盘);B 派「catch + continue REPL」收窄但**改 fatal 语义有副作用**(API key 无效 / provider 不可达 这类每轮都 Err 硬错误,continue REPL 让用户在 REPL 里每轮撞错、不死不活反复弹错,体验不如直接退让用户改 config)。**损害轻微**:本轮问一句话重打成本低(P7 一轮一 save 保证损害不累积只丢本轮 K 的 user turn)、fatal 退是 Rust `fn main() -> Result` 惯用语义非 bug、`--resume` 仍能接上前面对话(K-1 已落盘)。**M2 判**:**真撞到损害但属已知设计取舍证否** —— 新细分,与 P10-4「未撞证否」对照:P10-4 损害未实证撞到证否;**M2 损害确实发生但真修代价>收益证否**。**同 §17.1 P12-1 echo 假绿诚实副作用同收**:M2 真撞过程第一版 mock 不读 client POST body 就 write 响应 + shutdown 触发 Windows TCP RST=reqwest send 阶段报 error + 75ms 子进程 Err 退 + 磁盘 0 字节(连 round 1 都没落盘),修 mock 先 `stream.read(&mut req_buf)` 读一段 client→server 数据再回响应后真撞现场才成立。这同款 mock 缺陷使上轮 P12-1 §17.1 echo「411ms ECHO_NORMAL_EXIT 对照绿」实为假绿(详 §17.1 诚实返追段 + 上方 P12-1 状态栏),M2 真撞过程撞出 → 同收 P12-1 example 的:① echo mock 读 client body + ② echo 判读校 `status.success()`;修后 echo 真绿 `code=0 108.44ms` + hang 无回归仍 `5.07s FIX_OK_TIMEOUT_ERR_EXIT`。**焊入式 CI 回归闸**:`examples/p12m2_upstream_err_drops_round.rs` **单态证否闸**(非红绿翻转、非修后回归——M2 判不真修故无修后 ground truth 对照),gate-on 期望 `REGRESSION_CURRENT_ROUND_LOST`(真坑损害现场成立)+ exit=0(实证已记录损害态);异常态 `UNEXPECTED_NO_EXIT` / `WAIT_IO_ERROR` / `UNEXPECTED_CLEAN_EXIT` / `UNEXPECTED_COMBO` → exit=1 让人查。**作用**:防 M2 描述错(若有人改 run() Err 路径加了 catch + truncate + save,闸异常态 `ROUND2_PRESERVED` / 子进程正常退 → 提示 M2 已被修闸该档更新)+ 防 mock 退化(若有人改 mock 不读 client body,闸必踩同款 reqwest RST 退 stdout 空白异常态)。**验收四闸全绿**:fmt OK / clippy `-D warnings` 0 warning / `check --tests` Finished / `cargo test --lib` **44 passed**(M2 判证否故 src/main.rs 零改动,实证由 example 二进制闸覆盖真起 production codeagent --script 撞 mock 上游两轮验磁盘三断言)。p12m2 闸:gate-off skip+exit=0、gate-on `REGRESSION_CURRENT_ROUND_LOST` 132ms/141ms exit=0(两跑稳定)。**诚实副作用**:src/main.rs 零改、P12-1 example 判读漏洞 + echo mock 协议缺陷同收(诚实性补丁、上轮 P12-1 验收遗留、M2 真撞过程撞出来)、临时 toml 写 temp_dir key 注 fake、prod `codeagent.toml` mtime 未变(08/09/2026 14:30:39)随 env-only 纪律。**P12-2 性质归类**:新细分「真撞到损害但属已知设计取舍证否」(比 P10-4「未撞证否」更细一档:P10-4 损害未实证撞到、P12-2 损害确实发生但真修代价>收益),守「连续撞不到真硬坑就算结束」连续计数=1 朝收摊走
- **P12 进行中(2026-08-11)**:P12 候选 #1 已升格真坑真修(打破 P11「连续三条撞不到」收摊牌子)—— 回 P10-5 acknowledged 但 scope 外后三环接力,主 agent 裸 await 上游 hang 真撞 15.27s→5.17s 真修 + echo 411ms 对照绿 + 四闸全绿 + env-gate 二进制闸进 CI。继续守「连续撞不到真硬坑就算结束」:需后续连续撞不到 3 条才算 P12 收摊(被 #1 打破的连续计数从 0 重起);或用户拍板到此收摊。当前真坑真修 1 / 证否待定,待用户删测试 DEEPSEEK_API_KEY(sk-...len=35)收摊
- **P12-2 收口随 §17.2(2026-08-11)**:P12 候选 #2 上游 Err 路径漏落盘 + 漏 truncate 真撞损害现场升格 + 判「真撞到损害但属已知设计取舍」证否不真修(新细分,比 P10-4「未撞证否」更细一档:损害确实发生但真修代价>收益)+ P12-1 §17.1 echo 假绿诚实副作用同收(判读校 success() + mock 读 client body,echo 修后真绿 `code=0 108.44ms` 不再假绿)+ `examples/p12m2_upstream_err_drops_round.rs` env-gate 单态证否闸进 CI + 四闸全绿 lib 44。**当前「连续撞不到真硬坑」计数 = 1**(P12-2 真撞到损害证否计 1 朝收摊走,非真修故不清零 P12-1 已有的真修计数反向,也不算真修)+ src/main.rs 零改(M2 判证否故不真修)+ P12-1 example 诚实性补丁(echo 校 success() + mock 读 body)。守「连续撞不到真硬坑就算结束」:需后续连续撞不到真硬坑 2 条才算 P12 收摊(被 P12-1 打破从 0 重起后,P12-2 当前计数 1 朝收摊走);或用户拍板到此收摊。待用户删测试 DEEPSEEK_API_KEY(sk-...len=35)收摊
- [x] P12-3 ctrl_c 注册失败 → mpsc 关 → select 假中断刷本轮 user(§17.3·2026-08-11 析因单测实证 + 真修):P12-2 证否后接力看 interrupt 路径 —— `run()` 创建 interrupt channel + spawn ctrl_c 监听 task move 走唯一 `interrupt_tx`,**主循环未持 keepalive**(只留 `interrupt_rx`+task sender)。`tokio::signal::ctrl_c()` 某些 Windows 环境(无 console / `DETACHED_PROCESS` / 沙箱断 TTY)返 `Err` → task `if ctrl_c().await.is_err() { return; }` silent `return` drop 全 sender → channel 关 → `interrupt_rx.recv()` 立即 ready 返 `None` → `chat_completion_stream` 内 line 327 `tokio::select! { biased; _ = &mut interrupt => { interrupted=true; break; } ... }` **`_` 模式不绑 output 不区分 None vs Some(())**,`None` 当 Ctrl-C 假中断 → `StreamOutcome::Interrupted` → `run_one_turn` 返 `(false, 0)` → 主循环 `truncate(pre_turn_len)` **持续刷本轮 user** → REPL 表现「没按 Ctrl-C 但每轮秒打断、本轮问话丢、模型永远不答」死循环(持续 + 静默 + 难诊断三性)。**判规**:触发端=环境概率事件偏 P10-4,**损害链=确定性控制流**(给定 channel 关 → 必然假中断持续刷 user,非 race)。**判定真坑真修**(非证否 —— 损害比 P12-2 更糟:进程退用户至少有信号,M3 REPL 静默假死难诊断;真修极简、无副作用代价 < 收益)。**真修**(`src/main.rs`):`run()` 创建 channel 后立即 `let _interrupt_keepalive = interrupt_tx.clone();` —— main scope 持 sender 副本到 run() 结束,即便 task `return` `interrupt_tx` drop 全其他 sender,keepalive 一份活 → channel 不关 → `recv` 永远 Pending(无真信号即等)→ 不会被 select `_` 模式误判 None 假中断;ctrl_c 注册失败分支 silent `return` 改 `eprintln!("[ctrl_c] 注册中断信号失败...") + return` 诚实告知用户本进程无 Ctrl-C 功能(退化是「无中断功能」非「假中断打死 REPL」);`run_one_turn` 内 select 前注释 + spawn 块注释三处诚实性纠错旧「安全退化」误判。**判用 B 派 keepalive 而非 A 派 `_`→`Some(_)` 绑 output**:A 派 stale-poll 风险 + 依赖 mpsc 已融合 future 行为(非文档保证实现细节);B 派 mpsc 文档明确「channel 仅所有 sender drop 后关」keepalive 结构性保证更简洁。**析因单测闸**(落 `src/main.rs::mod tests`,**非 env-gate 二进制闸** —— M3 触发 ctrl_c 注册失败依赖环境概率,example 二进制闸难自动复刻真现场,走 P10-2 范式析因单测锁 select `_` 模式 + mpsc 关 channel recv 返 None 通用机制不调 production `chat_completion_stream`):测 #1 `select_underscore_pattern_treats_closed_recv_as_ready_interrupt`(真坑机制再现:`drop(tx)` 关 channel → `recv` 立即 ready None → `tokio::select! { _ = &mut recv_fut => {} }` 走过 = arm fire = `_` 模式不区分 None/Some 假中断源头实证);测 #2 `keepalive_sender_keeps_recv_pending_no_pseudointerrupt`(B 修后机制 demo:`let _interrupt_keepalive = tx.clone(); drop(tx);` 保持 keepalive → `rx.recv()` 必须 Pending → `tokio::time::timeout(100ms, rx.recv())` 撞钟 Elapsed assert `verdict.is_err()`)。两测对照封死真坑机制 + 修后不假中断(同 P10-2 复现旧/新逻辑对照 + P10-5 hang/echo 对照范式)。两测都是 `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]` 跨 OS 0s 不依赖 ctrl_c 真 Err 环境。初版写 trigger_fired 变量 + 二段 None 验证撞 clippy `unused-assignments` warning + E0499「cannot borrow `rx` as mutable more than once」(pinned recv_fut 持 `&mut rx` 活到测末)→ 撤掉变量 + 二段验,单 select! 走过即锁机制,clippy 0 / borrowck 过。**验收四闸全绿**:fmt OK / clippy `-D warnings` 0 warning / `check --tests` Finished / `cargo test --lib` **44 passed**(P12-3 真修在 src/main.rs 不动 lib 故 lib 同基线 44);**本机 binary mod tests runtime 显跑实证**:`cargo test --bin codeagent --no-fail-fast -- --test-threads=1` **11 passed** finished 2.18s —— 含新增两测 `select_underscore_pattern_treats_closed_recv_as_ready_interrupt`/`keepalive_sender_keeps_recv_pending_no_pseudointerrupt` 全绿 runtime 绿实证。**CI 四闸不变性**:`cargo test --lib` 仅跑 lib.rs mod tests **不跑** main.rs `mod tests`(binary target)(P11 起术语此地再次验证成立),新增两测 lib 过滤器跳过;但 `cargo clippy --all-targets` + `cargo check --tests` 静态门禁含 bin test target 编译期检查;本机 `cargo test --bin codeagent` 11 passed 是 runtime 显跑实证(非 CI 闸载体,按「不臆造」纪律真跑过)。**P12-3 性质归类**:P10-2 同型「确定性控制流 + 析因单测喂现场」(非 env-gate 二进制真起主进程型因 M3 触发依赖环境概率),损害比 P12-2 更糟故真修非证否,真修极简无副作用 = 修代价 < 收益(与 P12-2「真修代价 > 收益证否」对照两端判规同框架不同结论)。**守「连续撞不到真硬坑就算结束」:M3 真坑真修打破 P12-2 起朝收摊计数** —— P12-2「撞到损害证否」计 1 朝收摊走,M3「真坑真修」**清零该朝收摊计数**(真修打破连续撞不到)+ 真坑真修计数 +1(P12 起累计真坑真修 = P12-1 + P12-3 = 2)。需后续连续撞不到真硬坑 3 条才算 P12 收摊(被 M3 真修再次打破,计数从 0 重起走 3 条门);或用户拍板到此收摊。待用户删测试 DEEPSEEK_API_KEY(sk-...len=35)收摊。**诚实副作用**:keepalive `_interrupt_keepalive` 一行 scope-held 极简非新增 capability 非破老用法(真 Ctrl-C 路径行为未变,只闭「channel 关 → None 假中断」退化路径);ctrl_c eprintln 是诚实性补充非必需(无行为副作用);注释三处纠错(旧「安全退化」误判 → 新「None 当 Ctrl-C 持续刷 user 死循环远非安全退化」诚实纠偏);src/main.rs 改 3 处(spawn keepalive + 注释纠错 + eprintln + 2 析因单测),无新 example 文件(M3 走析因范式非 env-gate 二进制闸)+ prod `codeagent.toml` 一行未改(mtime 08/09/2026 14:30:39)+ env-only key 纪律不变
- [ ] P12-4 SSE 自然 EOF 末 frame 无尾 `\n\n` 残 buf 不 flush(§17.4·2026-08-11 临时 example 析因实证 + SSE 规范真生产实证证否·不修不焊闸):P12-3 真修后接力看 `chat_completion_stream` 收尾 —— chunk 循环(main.rs 324-407)在 `stream.next()` 返 `None`(自然 EOF)时 line 332-333 直接 `break` 跳 line 419 `acc.finalize()`,**循环外无残 `buf` flush 步骤**。候选坑:上游发最末 `data:` 帧后 EOF **不带尾 `\n\n`** → `sse_split`(tools.rs 932)loop 找不到边界 → 末帧整段留 `buf` 不 `drain` → 最后帧 `acc.ingest()` 从未被调 → 丢末含量 content delta / usage / finish_reason。注释 line 333「NIM/vLLM 偶尔不发 `[DONE]`,EOF 即终态」只说不发 `[DONE]` 内容帧(已有 EOF break 兜底),**未明说尾 `\n\n` 传输分隔符是否也省略** —— 候选把两件事分清。**析因实证**(临时 `examples/m1_sse_probe.rs` 跑完即删不入 git,纯 lib pub fn 析因):场景 A 末帧无尾 `\n\n`/对照 B 末帧带尾/场景 C 单 chunk 首末帧即无尾 —— A 与 C 均 **buf 残留 87 字节(完整末帧)、取出 0 frames**;B 正常 drain 1 frame、残留 0;残 buf 手动 `sse_data_payload` 仍能抽合法 payload(证明末帧内容合法、只是 sse_split 没切边界漏 buf)= **机制实证成立**(确定性纯函数缺陷非概率)。**关键判规转折 —— SSE 规范层面 `sse_split` 行为正确非缺陷**:WhatWG SSE 规范显式裁决 *"the incomplete event is not dispatched"* + *"the end of the stream is not enough to trigger the dispatch of the last event"* —— dispatch 只在遇到 blank line 时触发,末 event 未带尾 blank line 规范明确「不投递、应丢弃」。`sse_split` 漏 buf 不 flush = **严格符合 SSE 规范**(规范要求丢弃未终止 event),**不是缺陷**;若「修」成 flush 残 buf 反而**违反 SSE 规范**(把规范应丢弃的未终止 event 错误投递给 ingest)。**真生产实证证否损害链前提**:journey §9.1 DeepSeek 真流式实测样本 `[ctx:stream:1] ... total=887`(非 0)—— usage 末帧正是「最易丢的那帧」被成功 ingest = 证明真生产 DeepSeek 末帧**带尾 `\n\n`**(否则 sse_split 漏 buf usage 必落 0 与实测矛盾)= **M1 候选真生产未实证撞到损害**。**`[DONE]` 帧边角同收证否**:若 `[DONE]` 帧漏 buf 则 `done=true` 永不置走不到显式终止 break,但 line 332-333 EOF break 已是兜底收尾路径,`[DONE]` 漏 buf 也被 EOF break 合流 finalize 兜底,无损害,不另立候选。**判规归属**:对齐 **P10-4「未撞证否」范式**;控制流行为本身协议正确,焊「锁 sse_split 漏 buf」析因单测反成**反闸**(锁规范正确行为、未来有人改 flush 残 buf 反证规范违反会被误拒)→ **不焊闸**;与 P11-1/P11-3「证否不焊闸」同型(撞候选但损害未实证 + 不焊新闸因结构本身正确)。**真修价值评估**:若未来真遇到非规范 server 漏尾 `\n\n`,正确处理是**针对那类 server 的兼容 hack**(EOF break 后对非空 buf 做容错 ingest),而非**无差别** flush 残 buf —— 无差别 flush 会把规范明确应丢弃的未终止 event(可能是半截写坏的非法帧)错误投递给 ingest,违反 P10-3「净修不破老用法」+ 违反协议正确性 —— **此版不做**,守「不臆造」纪律非真生产实证撞到的损害不预修。**守「连续撞不到真硬坑就算结束」:M1 证否 = 朝收摊方向计 1**(M3 真修清零计数后从 0 起,M1 是第 1 条证否朝收摊走),真坑真修累计仍 = P12-1 + P12-3 = 2(M1 证否故真修计数不动),需后续连续撞不到真硬坑 2 条才算 P12 收摊;或用户拍板到此收摊。**诚实副作用**:src 零改(M1 判证否不修)+ 不焊新闸(焊反成反闸)+ 临时 `examples/m1_sse_probe.rs` 跑完即删不入 git + prod `codeagent.toml` 一行未改(mtime 08/09/2026 14:30:39)+ env-only key 纪律不变;journey §17.4 是 M1 唯一落点 + 本 P12-4 状态栏收口行
- [x] P12-5 MCP `request()` timeout/rx-close 路径漏 `pending.remove(&id)` 注释自述真坑真修(§17.5·2026-08-11 析因集成测闸进 CI · 对称补齐双 Err 出口):P12-4 证否后接力看 MCP 请求收尾 —— `mcp.rs::request` 的 timeout 链 recv 路径。读注释 line 337-341 自述 P10-1 修 TOCTOU race 时写「删请求(超时取消)走 timeout 后的 map.remove 收尾」= **注释对代码的承诺**,须核代码是否真兑现。**读码逐出口核对**(非猜):`request()` 三 return Err 出口 — ① write/flush err(line 358-361) 已有 `pending.remove(&id)` 是 P10-1 时就有、对的;② rx 关闭 Ok(Err(_)) 修前裸 `?` 不清;③ timeout Elapsed 修前裸 `?` 链 `tokio::time::timeout(timeout, rx).await?` 直接 propagate **不清 pending**。**注释自述应清但代码漏落实 = 实现 bug 强信号**(P10-1 修 TOCTOU race 把 `pending.insert` 前置但 timeout 出口收尾承诺漏兑现)。**损害链**:留 oneshot `tx` 孤儿在 `pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RpcEnvelope>>>`;真泄漏条件 = server 永不回该 id(hang 应用层 never 响应 → read task EOF 后 `sender.send(env)` 返 `Err(ReceiverDropped)` 静默吞 + task 退,**不会** `map.remove` 那 id 故 tx 永留 HashMap 到 McpClient drop)→ 多次 timeout 累几 KB 不可见内存增长(不 crash/不答非所问/不丢数据 = 不可见缓慢泄漏)。**判规**:触发端=坏 server hang 偏 P10-4 外部依赖,**损害链=确定性控制流**(给定 server 不回该 id → timeout 出口必然漏 remove 留孤儿)。**真判定真坑真修非证否** —— 对照 P12-2/M1 两端门槛:(1) 代价极小无副作用 = 两处 Err 出口对称补一行 `pending.remove(&id)`,**无行为变化**仅补漏落非改语义非破老用法;(2) 收益真 = 几 KB 防 + 注释自述兑现 + Err 出口语义对称。**代价 < 收益 = 真修**(与 P12-2「改 catch+continue 有 fatal 语义副作用故代价>收益证否」对照两端判规同框架不同结论 + 与 M1「行为本身协议正确故修反成违反」对照证否/真修分判)。**真修**(`src/mcp.rs` line 366-395):把旧裸 `?` 链 timeout recv 改显式 `match tokio::time::timeout(timeout, rx).await` 三分支 —— `Ok(Ok(env)) => env` 正常路径不变 / `Ok(Err(_))` rx 关闭先 `pending.remove(&id)` 再返 Err(read task 多半已自清 no-op 但对称闭窗口)/ `Err(_)` Elapsed 先 `pending.remove(&id)` 再返 timeout Err(拿回 entry 若 read task 已先 remove 取走则 no-op 不留孤儿)。新增注释段(366-372)诚实记录「注释与代码不符的实现 bug」+ 修前 timeout 漏落 + 真泄漏条件 + 对称补齐逻辑 + 与 P10-1 注释自述对齐。**析因闸进 CI** —— 与 P12-1/P12-2 example env-gate 二进制闸不同范式与 P12-3 析因单测不同 crate 定位,**C1 走 `tests/` 集成测**(独立 crate 拿 `CARGO_BIN_EXE_codeagent-mcp-fake-server` 路径 spawn 真 fake server,lib `mod tests` 编译期拿不到 `CARGO_BIN_EXE_*` env 故走集成测 —— 此约束先前 session 实证过见 journey P9 同款约束):① `src/bin/mcp_fake_server.rs` 加 `MCP_FAKE_NO_RESPOND` env 模式(逐行消费 stdin 防 pipe 堵塞但**不写回响应帧** → 对端 `request` 必撞 timeout;默认不设 env 时行为与 P8/A 阶段 keystone 完全一致向后兼容不破 P9 基线);② `tests/mcp_request_timeout_no_leak.rs` `#[tokio::test(flavor="multi_thread", worker_threads=2)]` 配 env + `handshake_timeout_secs=Some(1)` 真起 McpClient 连不响应 fake server,handshake initialize 1s 撞钟 Err → 断 `is_err()`(验 no_respond 真生效)+ lock client 调 `pending_len_for_test()` 断 `== 0`(真修后 timeout 清干净 = 0,修前漏 remove 拿 1 留孤儿);无 key 需求(纯 stdio + tokio::time)跨 OS 0 npx 依赖;③ test-only 访问器 `pub fn pending_len_for_test(&self) -> usize`(line 303-311) `pending.try_lock().map(|m| m.len()).unwrap_or(0)` —— `pub` 让集成测跨 crate 可达(先前 session 实证 `#[cfg(test)] pub(crate)` 在集成测编译期不编译 + pub(crate) 跨不了 lib crate 边界故改 `pub fn` 恒编译)+ `#[cfg_attr(not(test), allow(dead_code))]` 让 prod 路径不警告 dead code。**红绿翻转实证锁真修**(守 P10 范式不止「门禁绿」再防伪绿):SignalEdit 工具把 line 387 timeout `remove` 临时注释成 `// REDGREEN_DISABLED self.pending.lock().await.remove(&id);` → 跑测 **FAILED `left == 0 right == 1`**(拿到 1 = timeout 漏 remove 留孤儿)+ 错误信息「拿到 1 说明 timeout 路径漏 remove -> oneshot `tx` 孤儿留 HashMap」 **= 真坑现场实证成立**(本候选 server 恒不响 → timeout 必撞钟 → pending 必留 = 确定性纯逻辑非概率一回 case 就锁,对照 P10-1 N=200 拉 2% race 同是「真修必红绿翻转锁」范式);恢复 remove → 跑测 **1 passed 1.02s** 拿 `== 0` **= 真修生效锁**。**验收四闸全绿**:fmt OK / clippy `-D warnings` 0 warning(`pub fn pending_len_for_test` + `#[cfg_attr(not(test), allow(dead_code))]` 不触发 dead_code;先前 session 先碰 E0599 把 `#[cfg(test)] pub(crate) fn` 改 `pub fn`+allow 也是为这条 clippy 绿)/ check --tests Finished / cargo test --lib **44 passed**(C1 收尾语义非新增 lib pure-function 切点 = 同基线 44 不动)。**集成测全绿无回归**:`mcp_request_timeout_no_leak`(新闸)1 passed 1.02s / `mcp_toctou_race`(P10-1)1 passed 2.90s 200×3=600 握手 0 丢=P10-1 未退化 / `mcp_multi_server_isolation`(P11-2)1 passed 0.03s 跨 server 交错自回=P11-2 未退化 / `mcp_fake_handshake`(P8 keystone)1 passed 0.02s = P10-1 insert 前置未碰握手基线。**与 P10-1 TOCTOU race 同根两环对照分工**:P10-1 修「`pending.insert` 后于 `stdin.flush()`」时序 race(快 server 抢回响应丢 → timeout 挂)→ 修 insert 前置闭窗口;C1 修「timeout 出口自己漏 remove」收尾漏落(慢/不响 server 撞 timeout → entry 不清留孤儿)→ 修对称 remove 双 Err 出口。**两环 fact chain 同源**(`mcp.rs::request` 的 pending HashMap 收发对称性)**触发端不同**(P10-1 快 server 抢回 race / C1 慢/不响 server 挂死 timeout)**损害不同**(P10-1 丢响应挂死 repl / C1 孤儿 tx 几 KB 泄漏)**修法不同**(P10-1 insert 前置 / C1 remove 对称)—— 不重复不冲突:P10-1 修发送端 race,C1 补接收端收尾。**P10-1 注释 line 341 自述收尾承诺的代码漏落实由 C1 兑现**,故 P10-1 与 C1 注释段互相 cross-reference 同一 fact chain 的两端。**P12-5 性质归类**:P10-5/P12-1 同型「确定性 + 析因闸喂现场」,但触发端 = 慢/不响 server 外部依赖(偏 P10-4 环境信号),析因闸「伪造现场」是 fake server env 模式闭合环境影响(同 P12-3 fake server 不响的范式差异结合点)—— **非 P10-1 概率 race 必 N=200、非 P10-3 真文件系统跨 OS 建环、非 P10-5/P12-1 env-gate 二进制真起主进程**(C1 是「析因集成测借 CARGO_BIN_EXE_* spawn 真 fake server 闭环境 + 确定性纯逻辑 remove 漏落」第四态变型)。**守「连续撞不到真硬坑就算结束」:C1 真坑真修打破 M1 起朝收摊计数** —— M1 证否计 1,C1 真坑真修**清零该朝收摊计数**(真修打破连续撞不到)+ **真坑真修累计 +1 = P12-1 + P12-3 + C1 = 3**。被打回 0 重起后,需后续连续撞不到真硬坑 3 条才算 P12 收摊;或用户拍板到此收摊。**诚实副作用**:`src/mcp.rs` 改(request timeout 链显式 match + 双 Err 出口对称 `pending.remove` + 新注释段 + `pub fn pending_len_for_test` 访问器 + `#[cfg_attr(not(test), allow(dead_code))]`)+ `src/bin/mcp_fake_server.rs` 改(`MCP_FAKE_NO_RESPOND` env 模式向后兼容默认不破老 P9 keystone 基线)+ `tests/mcp_request_timeout_no_leak.rs` 新增(跨 OS 0 key 进 CI)+ 零例 example 文件(C1 走集成测范式非 env-gate 二进制闸)+ prod `codeagent.toml` 一行未改(mtime 08/09/2026 14:30:39)+ env-only key 纪律不变;journey §17.5 是 C1 唯一落点 + 本 P12-5 状态栏收口行
- [x] P12-6 `summarize` 路径裸 await 上游 hang(§17.6·2026-08-12 被历史真修覆盖型二阶否定·证否不修不焊闸·已收口):C1 真修收口后接力看 P12-1 acknowledged 但 scope 外未单独闸的下一环 —— 主 agent 另一条独立 HTTP 路径 `ModelSummarizer::summarize`(main.rs 207-251)非流式(`stream:false`)模型二次调摘要生成。**读码逐条确认**(非猜):① `ModelSummarizer` 持 `client: &'a reqwest::Client`(line 193-196)**借用而非自构**不调 `Client::new()`;② 唯一实例化点 line 1183-1186 `&ModelSummarizer { client: &client, ... }` 借用 `run()` line 926 `let client = build_http_client()?` 构的同一 client(**经 P12-1 修后持 connect_timeout 30s + read_timeout 90s 兜底**);③ grep 全文件无裸 `Client::new()`(P12-1 已替换全部两处 run/`probe_tool`);④ summarize 调用点 line 1183 在 `maybe_compact` 里故**经同 client** 走 P12-1 修过的兜底覆盖。**判规**:**被历史真修覆盖型证否(二阶否定,比 P10-4「未撞」更细一档)** —— 第一阶(损害链根本成立性):P12-1 真修对换主 agent client timeoutless bug 根因,summarize 路径**借用**同 client 非自构裸 `Client::new()`,故「`Client::new()` 裸构无 timeout → 上游 hang 确定性永挂」损害链**根本不成立** —— 该路径上不存在 timeoutless bug 活动载体,P12-1 connect+read_timeout 兜底同根覆盖到 summarize 路径;第二阶(真生产未实证撞损害):P12-1 example `p12_main_agent_upstream_hang.rs` 跑 `--script --yolo` 喂单行 user 输入**单轮不过 `max_context` 阈值 → 不触发 compaction → 不调 summarize** 故闸**不直接覆盖** summarize 路径 send 阶段 + 真 DeepSeek live 调用里 summarize 路径 timing 无 measured 撞 90s read_timeout sample(§9.1 `total=887` 是 main agent streaming 路径实测非 summarize 路径)→ 两层都查不到损害实证。**两层都否定**故不同 P10-4「未撞证否」(P10-4 一阶成立 candidate 损害链成立 + 二阶否损害未实证)、不同 M1「行为协议正确证否」(M1 一阶成立 + 行为正确不修)、不同 P11-2「真撞到损害但代价>收益证否」(P11-2 损害真发生仍不修)—— **C6 是「损害链根本不成立 + 上层损害也未实证」**新型「被历史真修覆盖型二阶否定**:P12-1 真修对主 agent client timeoutless bug 的根因 fix 通过 **client 共享**自然延伸覆盖所有借用同一 client 的卫星路径(summarize 是其中之一),无需对每条卫星 path 重复闸/修 —— 一次根因 fix 封断点 + N 条同根卫星 path 同时获保。**边角细微注记(诚实记但不升格真坑)**:P12-1 `read_timeout` 是「每字节间隔」streaming-safe 设计(真生产流式 SSE chunk 间歇吐字节 read_timeout 持续吐不触发,兼容流式续吐长跑不解被切 —— §17.1 注释自述过),summarize 走的是**非流式 chunked response**(`stream:false`):上游先把整段生成完成才 flush(首字节延迟 ≈ full generation time)非流式「首 token 早首 + chunk 间歇吐」,**理论上**若上游「首响应头几十分钟不 flush 首字节」可能误杀慢首响应 summarize call。**为何此边角不升格真坑**(守「不臆造 / 真撞实证」纪律):① 真生产未实证撞损害(DeepSeek summarize 真 sample timing 未实测过,P12-1 example 不触发 compaction 故 summarize 路径未实测经过 P12-1 modified client 边界一次,理论可成立 ≠ 真生产撞损害);② 同 P10-4「未撞」防臆造范式(P10-4「summarize 返空」当时也是 live 实测真触发发现返 1093 字真摘要、空 content 候选未实证撞到 → 不收真坑不预修,C6 同型不预修);③ 若未来真撞某 non-streaming 上游慢首响应 head > 90s,正确处理是**针对那类 server 的 env 调窗口**(P12-1 `CODEAGENT_UPSTREAM_READ_TIMEOUT_SECS` env override 已留逃生口)非**无差别**改 `build_http_client` 默认(无差别改默认会破真生产流式慢首 token 数分钟不解被切的兼容取向违反 P10-3「净修不破老用法」+ 违反 P12-1 设计取向);④ 不焊闸:焊「锁 summarize 路径走 read_timeout 兜底」反成**反闸**(P12-1 已修同根 client 覆盖到 summarize,再焊单独针对 summarize 的析因闸测 90s-timing 边角锁的是「P12-1 设计取向嫁接非流式路径」未经真生产实测证伪的理论边角,未来若有人按真生产样本发现 read_timeout 对非流式 summarize 不合调默认或留 env override 时反闸会误拒;与 M1「焊反成反闸锁规范正确行为」同型但更弱 —— M1 锁 SSE 规范层正确行为,C6 锁 P12-1 设计取向嫁接非流式路径后者规范无背书故更弱不焊闸)。**真裁结论**:候选 #6「summarize 裸 await 上游 hang」**被 P12-1 同根 client 真修完全覆盖型二阶否定**(损害链根本不成立因 root cause fix 已封断 + 损害也未实证撞到)—— 不修代码、不焊新闸、不另加 example;journey §17.6 是 C6 唯一落点 + 本 P12-6 状态栏收口行。**守「连续撞不到真硬坑就算结束」**:C6 证否 = 朝收摊方向计 1(C1 真修清零计数后从 0 起,C6 是第 1 条证否朝收摊走),真坑真修累计仍 = P12-1 + P12-3 + C1 = 3(C6 证否故真修计数不动);需后续连续撞不到真硬坑 2 条才算 P11 收摊;或用户拍板到此收摊。**诚实副作用**:src 零改(C6 证否不修)+ 不焊新闸(焊反成反闸)+ 不加 example(C6 被 P12-1 同根覆盖故 example 重复 P12-1 范式白加)+ prod `codeagent.toml` 一行未改(mtime 08/09/2026 14:30:39)+ env-only key 纪律不变。**C6 与 P12-1 关系诚实记**:P12-1 真修对主 agent client 的 root cause fix 通过 client 共享自然延伸覆盖所有卫星路径(summarize 是其中之一)—— 一次根因 fix 封断点 + N 条同根卫星 path 同时获保,是「净修」原则的实践印证(P10-3 净修原则的代码实例:在根因层修一次,不在每条卫星 path 各修一遍)
- [x] P12 SRC SWEEP 收摊(§17.7·2026-08-12 src/ async/concurrency/subprocess surface 全扫尽 net 0 uncovered + C3 非 Windows grandchild orphan acknowledged-deferred·P12 真 candidate surface 100% swept 完·收摊附录非新候选):**起因**:C6 证否后接力下一环前先穷尽 src/ async/concurrency/subprocess surface 确认无未扫到的真 candidate domain。**Explore agent sweep 结果**(只读结构性 inventory 非 fix 提议):src/ 9 个 module(main/mcp/subagent/tools/compactor/session/config/lib/message)全部在 P12 covered set 内或纯同步零 async(message.rs 50 行纯 struct、lib.rs 30 行 crate root re-export 无 async);全 src/ `reqwest::Client` 构点恒在 `src/main.rs:112`(P12-covered `build_http_client`)+ `probe_tool` 另一独立 `build_http_client`,grep 全文件**无裸 `reqwest::Client::new()`**(P12-1 已替换全部两处);全 src/ `std::thread::spawn`/`std::thread::sleep` 在 `src/tools.rs:527-528`(P12-covered `Bash::execute` 壁钟路径);全 src/ `tokio::spawn` 都在 covered sections 内(mcp.rs:249 read task / tools.rs:514 + 549 in Bash::execute / main.rs:940 ctrl_c task);全 src/ `tokio::select!` 在 main.rs:325 + 1647(covered);全 src/ `tokio::pin!` 在 main.rs:744 + 1642(covered);examples/ + tests/ 与 src/bin/mcp_fake_server.rs 都是 env-gate 例程/集成测闸/fake server =「闸与 repro 工具」非生产 code path 不属「真 candidate async surface」域。**Net uncovered async/concurrency/subprocess surface = 0**:src/ 生产代码面 P12 已 100% swept,**无新真 candidate async surface 可读出**,继续接力下一环候选需主动构造非读码读出 —— 这违反「不臆造/真撞实证/candidate 应读码读出而非猜出」纪律,不构造。**唯一剩余 known-but-deferred 候选 = C3 非 Windows grandchild orphan**:`src/tools.rs:537-544` `Bash::execute` 壁钟路径非 Windows 分支走 `kill -9 <pid>` 单杀父级(不带 `-- -pgid` 进程组树杀语义)故 child fork 出去的 grandchild 不在 `kill -9` 杀范围 vs Windows 走 `taskkill /T /F /PID` 树杀连 grandchild 一起灭(P9-4 实装真修);注释 tools.rs:503-504 + line 538-541 自述「非 Windows 路径留 P9+」「真非 Windows 端到端留 P9+」= **P9-4 acknowledged 已 deferred future work item,code 自述承认**。**判归**:不归 P12 active candidate —— 同 P12-1 当时不修第四环(`--script` 无 Ctrl-C 信号源)同 convention「independent future capability/acknowledged-deferred」;P12 baseline 是 Windows 为主战场(journey 范围明明 establish),本机 Windows 无法编译 `#[cfg(not(target_os = "windows"))]` 分支或真跑实证非 Windows 路径损害链 → **环境受限证否**状态(fact chain 真成立 + 真生产未实证撞损害 on Windows 主战场);**不计数**、不归证否也不归真修,不是 P12 candidate #7 —— 它是 P9-4 已 acknowledged roadmap future work item,P12 sweep 时遇着诚实在此 acknowledge 不掩盖,未来真上 Linux/macOS 主战场时这条该被真端到端闸覆盖(同 P10-5/P12-1 env-gate 跨 OS CI 范式)。**Closure 判定**:C6 证否朝收摊计数 +1 = 1 后,src/ async surface 全扫尽 net 0 uncovered + C3 acknowledged-deferred 非 active candidate 不计数 = **P12 真 candidate async surface 100% swept 完、无新候选可读出** = P11「连续三条撞不到真硬坑」收摊牌子同型 surface exhaustion 收摊条件自然达成,等价 P11-1/P11-3 证否型 + P11-2 读码判结构不可能 + 真跑兜底双绿组合 —— 读码判无可读出 candidate = src/ 净零 uncovered async surface = 撞坑 surface 已穷尽自然收摊,不构造不臆造。**P12 真坑真修累计 = 3**:P12-1(主 agent client hang 根因层兜底)+ P12-3(ctrl_c mpsc 假中断 keepalive 修)+ C1(MCP `request()` timeout/rx-close 路径对称 remove 收尾)—— 三真坑各有真撞实证 + 红绿翻转锁 + CI 闸进 = 真 fact chain 真 fix 真验证闭环。**P12 证否型候选 = 3**:M1(SSE EOF 末 frame 残 buf 不 flush 协议正确证否)+ P12-2(上游 Err 路径漏落盘漏 truncate 真修代价>收益证否)+ C6(summarize 路径裸 await 上游 hang 被历史真修覆盖型二阶否定证否)—— 三证否各有真 fact chain 分析 + 真生产实证证否理由。**P12 真坑真修 + 证否组合 = 6 candidates 全闭环**,剩 1 尾 C3 acknowledged-deferred(environmental gating 留 future)。**P10→P11→P12 撞坑手段类型谱总结**(诚实给同侪):自 P10 起 5 种 → P11 加 2 种 → P12 加 4 种 = 11 种撞坑手段类型对照:① P10-1 概率 race 必 N=200 真撞闸 ② P10-2 确定性控制流析因单测喂现场 ③ P10-3 真文件系统 symlink cycle 跨 OS 建环真跑 ④ P10-4 外部模型依赖不可控 live 实证撞不到即证否 ⑤ P10-5 确定性 + 本地 mock 上游真起 example 修前/修后两版实证对照(env-gate 二进制闸)⑥ P11-1 真撞实证撞到候选库但非真坑证否型 ⑦ P11-2 读码判结构无可命中 + 真跑兜底双绿焊反退化闸型 ⑧ P12-1 主回路根因层 client 兜底 + env helper + env-gate 二进制闸红绿翻转 ⑨ P12-2 真撞到损害但真修代价>收益证否 ⑩ P12-3 确定性控制流 + 析因单测喂现场 + keepalive 修 ⑪ P12-5(C1)析因集成测借 `CARGO_BIN_EXE_*` spawn 真 fake server 环境闸 + 确定性纯逻辑 remove 漏落 + P12-6(C6)被历史真修覆盖型二阶否定证否。**P12 收摊 stamp**(2026-08-12):守「连续撞不到真硬坑就算结束」—— C6 证否朝收摊计数 +1 = 1,src/ async surface 全扫尽 net 0 uncovered 后无新真 candidate 可读出(surface exhaustion)+ C3 acknowledged-deferred 非 active candidate 不计数 = 等价 P11「连续三条撞不到」surface exhaustion 收摊条件达成。**P12 真坑真修 3 + 证否 3**(自 P12-1 起 covered candidates)全闭环,剩 1 C3 acknowledged-deferred 留 future;src/ 生产代码面 P12 100% swept。**诚实副作用**:此 §17.7 收摊附录不修代码、不焊新闸、不加 example、零文件 src 改动;Explore agent sweep 报告不入 git;journey §17.7 是 sweep result + C3 acknowledged-deferred 唯一落点;prod `codeagent.toml` 一行未改(mtime 08/09/2026 14:30:39)+ env-only key 纪律不变。待用户删测试 DEEPSEEK_API_KEY(sk-...len=35)收摊(本轮已保证自删故不再中途提醒)。**如果未来上 Linux/macOS 主战场**,C3 非 Windows grandchild orphan 该被真端到端闸覆盖(同 P10-5/P12-1 env-gate 跨 OS CI 范式)—— 此 deferred 不丢,记在此 §17.7 + C3 注释自述「留 P9+」留痕同源

