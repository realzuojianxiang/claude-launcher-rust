# Grok Provider 接入详细文档（Phase 5 前端 UI + Phase 6 测试门禁/诊断钩子）

> 范围：本文档覆盖 **Phase 5（前端 UI + profile 切换）** 与 **Phase 6（测试门禁强化 + 诊断钩子）**。Phase 1/2/3/4 见各自阶段文档与 `grok-provider-phase3.md`，本文不重复展开其内部实现，只在被前端调用/诊断编排处点名。
>
> 实现基线：`rust/`（React 18 + TypeScript + Vite 8 前端；Tauri v2.1.1 + axum 0.7 + reqwest 0.12 rustls-tls 后端）。本会话末态全五门禁绿：`cargo fmt --check` / `cargo clippy --all-targets -D warnings` / `cargo check --tests` / `npm run build` / `npm test`。
>
> 借鉴说明：协议契约参考 CLIProxyAPI（Go 项目，**只读协议，Rust 从零实现，两项目代码相互独立**）。

---

## 0. 一句话总结

Phase 5 把 Grok provider 的全部前端体验补齐：`GrokPage` 主页（OAuth 授权面板 + 状态卡 + Key 池卡 + 测试面板 + 配置表单）+「🟦 Grok 代理 (本地 8083)」预置 profile（启动页一键选，自动注入 `ANTHROPIC_BASE_URL=http://127.0.0.1:8083` 等环境变量）+ OAuth 状态提升到 App（Device Code Flow 进行中切菜单不丢 `user_code`）。Phase 6 补齐 `GROK_DIAG=1` 独立诊断钩子（写 `grok-diag.txt`，与 NVIDIA 的 `diag.txt` 串台隔离）+ 复核 converter/stream 既有单测已覆盖 text/reasoning/tool_use 三类块 SSE 序列与 truncation/stall 的 error 不伪装 end_turn，全门禁回归绿。

---

## 1. Phase 5 — 前端拓扑

### 1.1 新增/改动文件清单

| 文件 | 状态 | 职责 |
|---|---|---|
| `src/pages/GrokPage.tsx` | 新建 | 主页：组合 OAuth 卡 + 状态卡 + Key 池卡 + 测试面板 + 配置表单。本地态：status/msg/busy/keyPool/kpExpanded + 配置编辑字段；OAuth 态由 App 提升控制（见 1.4） |
| `src/pages/GrokPage.test.tsx` | 新建 | Vitest：mock `invoke`+`listen`，断言 OAuth 卡/状态卡/Key 池/测试面板/认证模式选择器/映射表标题；验证模型添加→`grok_set_models`；验证 Token 生成 `/^[0-9a-f]{64}$/` |
| `src/pages/grok/GrokOAuthCard.tsx` | 已有 | Device Code Flow 前端交互：`listen('grok-oauth-done'/'grok-oauth-error')` + `UnlistenFn` 清理 + `start()`/`revoke()` + 样式化 `user_code` |
| `src/pages/grok/GrokStatusCard.tsx` | 已有 | 纯展示：起停/刷新/测试连接按钮 + endpoint + Anthropic 端点提示文案 |
| `src/pages/grok/GrokTestPanel.tsx` | 已有 | curl 测试命令生成（bash/powershell/cmd 三种）+ 环境变量示例，标题动态 `{port}`（不写死 8082） |
| `src/pages/grok/ModelMapEditor.tsx` | 已有 | claude-*→grok-* 映射表行编辑（增/改/删 + 同名 anthropic 覆盖式去重 + Enter 触发添加） |
| `src/pages/grok/GrokConfigForm.tsx` | 已有 | 适配自 `NvidiaConfigForm`：`auth_mode` select + 双 base URL + `oauthAccount` 只读展示 + API Key 文本框（仅 api-key 模式显示）+ 复用 `ModelPriorityEditor` + 嵌入 `ModelMapEditor` |
| `src/pages/nvidia/ModelPriorityEditor.tsx` | 已有（改） | 加可选 `addPlaceholder` prop（默认保留 NVIDIA 文案），向后兼容；Grok 用时传「添加 Grok 模型，如 grok-4.3」 |
| `src/pages/nvidia/KeyPoolCard.tsx` | 已有（直接复用） | Grok OAuth 模式下 `grok_pool` 返回 not-running 即走「代理未运行」兜底；API Key 退路模式沿用 NVIDIA Key 池语义 |
| `src/App.tsx` | 改 | lazy import `GrokPage` + `grokTest`/`grokOAuth` 状态提升 + `active === "grok"` 渲染分支传 `oauthState`/`onOauthState` |
| `src/types.ts` | 已有（前阶段） | `GrokConfig`/`GrokStatus`/`GrokAuthMode`/`ModelMapEntry`/`GrokOAuthState`/`GrokTestState` + `Config.grok` 字段 |
| `src/test/fixtures.ts` | 已有（前阶段） | `grok` 配置块（auth_mode=oauth、port=8083、models=[grok-4.3, grok-3-mini-fast]、model_map 两行、auth_token） |
| `src/menu.ts` | 已有（前阶段） | `Bot` 图标 + `"grok"` MenuKey + 「Grok 代理」菜单项（顺序：仪表盘→启动→NVIDIA→Grok→日志→配置→单词本→关于） |
| `src/providerEnv.ts` | 已有（前阶段） | `GROK_PROVIDER="🟦 Grok 代理 (本地 8083)"` + 该 profile 的 env 分支（`__grok_isolate__=1`、`ANTHROPIC_BASE_URL=http://127.0.0.1:{port}`、`ANTHROPIC_API_KEY={auth_token 或 sk-grok-local}`、`ANTHROPIC_MODEL=claude-sonnet-4` 由代理映射） |
| `src/pages/LaunchPage.tsx` | 已有（前阶段） | `<option value={GROK_PROVIDER}>` + 该 profile 的 hint（端口/auth_mode/上游来源）+ fallback guard（`profile !== GROK_PROVIDER` 不回退到首个 profile） |

### 1.2 GrokPage 状态与数据流

```
App (提升态)
├─ cfgState: Config            ─ get_config 拉取
├─ grokTest: GrokTestState     ─ 跨菜单保留测试结果（testBusy/testResult/chatTests{model→{busy,result}}）
└─ grokOAuth: GrokOAuthState   ─ 跨菜单保留 Device Code Flow 进行态（userCode/verificationUri/authorized/account/...）
        │
        ▼  props: config, onConfig, test, onTest(updater), oauthState, onOauthState(updater)
GrokPage
├─ 本地态: status(GrokStatus|null), msg, busy, keyPool(KeyPoolStatus|null), kpExpanded
├─ 配置编辑态: keysText, models, modelMap, authMode, oauthBaseUrl, apiBaseUrl,
│              host, port, cooldown, retries, timeoutS, authToken
├─ useEffect[config] → 回填编辑态
├─ refresh() → invoke('grok_status')                      [挂载时一次]
├─ refreshOauth() → invoke('grok_oauth_status')           [挂载时一次：回填凭证态]
├─ collect() → GrokConfig（拆 keys/models、clamp port/cooldown/retries/timeout、oauth_account=oauth.account）
├─ persistConfig(grok) → invoke('set_grok_config',{grok}) + onConfig({...config,grok})
├─ applyModels(list) → grok_set_models（模型优先级即时热更新）
├─ applyModelMap(next) → set_grok_config（映射表即时热更新，map_model 每请求现读 cfg）
├─ start/stop → persistConfig + grok_start/grok_stop + refresh
├─ testConn → persistConfig + grok_test（直连上游自检）
├─ sendChatTest(model) → grok_chat_test（端到端走本机 8083 完整转换链）
└─ useEffect[running] → 每 2s grok_pool（仅运行时）
```

### 1.3 GrokPage 调用的 Tauri 命令（与 lib.rs 注册一一对照）

| 命令 | 触发点 | 返回shape |
|---|---|---|
| `grok_status` | 挂载/刷新/起停后 | `{running, endpoint}` |
| `grok_oauth_status` | 挂载一次回填凭证 | `{authorized, account, expires_at, expired, refreshable}` |
| `set_grok_config` | 保存/启动前/测试前/映射热更 | `string`（结果文案） |
| `grok_set_models` | 模型优先级增删/拖动即时热更 | `string` |
| `grok_start` / `grok_stop` | 状态卡起停 | `string` |
| `grok_test` | 状态卡「测试连接」直连上游 | `string` |
| `grok_chat_test` | 模型优先级行「▶ 测试」端到端 | `string` |
| `grok_pool` | 运行时每 2s 轮询 | `KeyPoolStatus`（OAuth not-running 兜底） |
| `grok_oauth_start` / `grok_oauth_revoke` | OAuth 卡内 | 见 1.5 |

### 1.4 OAuth 状态为何提升到 App

Device Code Flow 是异步多步：用户在 `GrokPage` 点「授权」→ 前端展示 `user_code` + `verification_uri` → 用户去浏览器登 x.ai 同意 → 后端 `tauri::async_runtime::spawn` 轮询 task → 授权落定 emit `grok-oauth-done`。

如果 `user_code`/`verification_uri` 只存在 `GrokPage` 的本地 `useState`，用户在授权进行中切到「日志」或「启动 Claude」菜单再切回来时，`GrokPage` 已被 `lazy + Suspense` 卸载、本地态清空，于是「明明后端还在轮询、授权码却消失了」——体验拧巴。提升到 App 后：

- `grokOAuth: GrokOAuthState` 由 App 持有，`GrokPage` 作为受控组件通过 `oauthState`/`onOauthState` props 接收与更新。
- `GrokPage` 卸载/重挂不影响 OAuth 态；`GrokOAuthCard` 内的 `listen()` 在 `GrokPage` 重挂时重连事件源（Tauri 事件全局，后端 task 仍在推）。
- App 初始态为未授权；`GrokPage` 挂载后 `grok_oauth_status` 回填真实凭证态（已授权/过期/可刷新）。

### 1.5 GrokOAuthCard 事件契约

```
grok_oauth_start
  ├─ 已有非空凭证 → 返回 {already_authorized:true, account, expires_at}
  │                   （前端直接置 authorized，不重复发起，避免覆盖在用 token）
  └─ 否则 → discover + request_device_code，返回给前端的载荷只含展示字段：
            {already_authorized:false, user_code, verification_uri,
             verification_uri_complete?, expires_in?, interval?}
            device_code / token_endpoint 是机密——只后端用，绝不回前端。
            同时 tauri::async_runtime::spawn 起轮询 task，按 interval 唤醒直到授权/过期。

后端轮询 task（成功路径）:
  poll_for_token → oauth_store::save（DPAPI 加密 grok-oauth.json）
                  → 同步 cfg.grok.oauth_account = store.account 落盘 config.json
                  → app.emit('grok-oauth-done', {account, expires_at})
  （任何失败路径）→ app.emit('grok-oauth-error', {message})

前端 GrokOAuthCard useEffect:
  listen('grok-oauth-done', e => setState authorized/account/expires_at + 清 userCode + onMessage)
  listen('grok-oauth-error', e => setState busy=false + error + onMessage)
  两个 .then(u => unlisten) 卸载时调用 —— UnlistenFn 清理（precedent: LogPage.tsx）
```

> 关键：前端不轮询命令。OAuth 进度由后端 task 推 Tauri 事件，与 CLAUDE.md「无前端高频 fetch」一致；前端只展示 + 监听两个事件。

### 1.6 配置编辑即时热更新两轴

- **模型优先级**（`models[]` 顺序即 Fallback 链）：增删/上移/下移/置顶 → `applyModels()` → `grok_set_models`（写后端 RwLock，运行中代理下个请求即用新链，无需重启）+ 本地 useState 同步。
- **模型名映射**（`model_map[]`：claude→grok）：增/改/删 → `applyModelMap()` → `set_grok_config`（落盘 + 写后端 cfg，`map_model` 每次请求现读现用 cfg 取新表）。
- 两者均「保存即生效」，配置表单底部 hint 明示。`start()` 启动按钮也会先 `persistConfig(collect())` 再 `grok_start`，杜绝「改了没保存就启动」困惑。

### 1.7 认证模式切换的退路语义

`GrokConfigForm` 顶部 `<select>`：`oauth`（主线）/ `api-key`（退路）。

- **OAuth 模式**：`OAuthAuthProvider` 走 `cli-chat-proxy.grok.com` + CLI Chat-Proxy 专用头（`X-XAI-Token-Auth: xai-grok-cli` + `x-grok-client-version: 0.2.93`），已授权 token 不因切模式丢失（DPAPI 文件不动）；启动前 `start()` 校验本地有非空凭证，否则报「请先授权」。表单顶部只读展示当前 `oauthAccount` 邮箱。
- **API Key 模式**：`ApiKeyAuthProvider` 走 `api.x.ai` + Bearer key，复用 NVIDIA `key_pool` 多 Key 轮询 + 429 冷却。表单仅此模式显示 API Keys 多行文本框。

「生效上游」hint 实时显示由 `auth_mode` 裁决的 base（`oauthBaseUrl` / `apiBaseUrl`）+ `/responses`，并提示「两端都禁跟随重定向、3xx 一律判 502，防 SSRF 与 token 外泄」（对应 `proxy.rs` 的 `redirect::Policy::none()`）。

---

## 2. Phase 5 — profile 预置（启动页）

`providerEnv.ts` 的 `GROK_PROVIDER` 分支（在 profiles 兜底之前）：

```ts
if (profileName === GROK_PROVIDER) {
  const grok = config?.grok;
  const model = "claude-sonnet-4";            // 代理按 model_map 改写为 grok slug
  const port = grok?.port ?? 8083;
  return {
    __grok_isolate__: "1",                    // 触发 claude.rs provider 并发隔离目录（同地址跨启动复用）
    ANTHROPIC_BASE_URL: `http://127.0.0.1:${port}`,
    ANTHROPIC_API_KEY: (grok?.auth_token && grok.auth_token.trim()) || "sk-grok-local",
    ANTHROPIC_MODEL: model,
    ANTHROPIC_SMALL_FAST_MODEL: model,
  };
}
```

- 不改 `claude.rs` 的 env 白名单——`ANTHROPIC_BASE_URL`/`ANTHROPIC_API_KEY`/`ANTHROPIC_MODEL`/`ANTHROPIC_SMALL_FAST_MODEL` 已在内，`__grok_isolate__` 复用既有 provider 隔离目录机制（同地址跨启动复用插件/MCP/hooks，不同地址隔离）。
- `LaunchPage.tsx` 的 `<option>` 放在 NVIDIA 之后；该 profile 与 NVIDIA 预置一样是硬编码常量（不在 `profiles[]`），故统一加了 `profile !== GROK_PROVIDER` fallback guard，避免被回退到首个 profile。

---

## 3. Phase 6 — 诊断钩子（GROK_DIAG=1）

### 3.1 设计

镜像 `NVIDIA_DIAG=1`：写普通文件、不依赖任何事件/UI，避免引入新死锁。关键差异：**产物分文件**，避免 grok 启动步骤的时间戳与 NVIDIA 串台。

### 3.2 实现（`lib.rs`）

```rust
// 共享总开关（NVIDIA_DIAG 或 GROK_DIAG 任一为 1 即打开）
static DIAG_ENABLED: OnceLock<bool> = OnceLock::new();

// NVIDIA 专用：落盘 diag.txt
pub(crate) fn diag_step(msg: &str) { ... Config::config_dir().join("diag.txt") ... }

// Grok 专用：落盘 grok-diag.txt（同形，分文件，避免串台）
pub(crate) fn grok_diag_step(msg: &str) { ... Config::config_dir().join("grok-diag.txt") ... }
```

setup 钩子（`lib.rs::run()` 的 setup 闭包尾部，紧跟 NVIDIA 段之后）：

```rust
let grok_diag_on = std::env::var("GROK_DIAG").map(|v| v == "1").unwrap_or(false);
if grok_diag_on {
    DIAG_ENABLED.get_or_init(|| true);              // 打开总开关（grok_diag_step 由此生效）
    let cfg = app.state::<Mutex<Config>>().lock().unwrap().grok.clone();
    let diag = Config::config_dir().join("grok-diag.txt");
    let _ = std::fs::write(&diag, "");              // 清空旧内容
    std::thread::spawn(move || {
        let _ = std::fs::write(&diag, "DIAG: calling GrokState::start\n");
        let gstate = GrokState::new();
        let r = gstate.start(cfg);
        let _ = std::fs::write(&diag, format!("DIAG: start returned = {:?}\n", r));
    });
}
```

`grok/mod.rs::start()` 各关键步骤已埋 `grok_diag_step`（沿用既有，仅改函数名）：
`entry` → `tcp bind ok` → `ProxyCtx::new ok` → `build_router ok` → `server thread spawned` → `inner set`。

`grok/proxy.rs::handle_messages()` 增两处：
- 入口 `grok_diag_step("proxy handle_messages(): entry")`
- 上游响应落地 `grok_diag_step(format!("proxy upstream status={} model={} attempt={}", ...))`

### 3.3 排查用法

```bash
# 在 rust/ 子目录
$env:GROK_DIAG="1"
npm run tauri dev
# 复现「启动 8083 卡死」，随后查看配置目录下（exe 同级 claude-launcher/）grok-diag.txt：
#   DIAG: calling GrokState::start
#   [06:42:01.234] grok start(): entry
#   [06:42:01.451] grok start(): tcp bind ok           ← 缺这条 = 卡在 OAuth discovery/token load
#   [06:42:01.612] grok start(): ProxyCtx::new ok
#   ...
#   DIAG: start returned = Ok("✅ Grok 代理已启动，监听 http://127.0.0.1:8083")
# 哪一步时间戳跳变大 / 缺失，即定位到卡的环节。
# 请求阶段：handle_messages(): entry → upstream status=200 model=grok-4.3 attempt=1
#   ← status 缺 = 卡在 token acquire / 转换 / http send；status 是 5xx/429 = 上游/鉴权问题
```

---

## 4. Phase 6 — 测试门禁复核

### 4.1 converter 单测覆盖（`grok/converter.rs:683`，15 项）

| 覆盖点 | 测试 | 断言 |
|---|---|---|
| system string → input[0] developer message | `system_string_becomes_developer_message_not_instructions` | `instructions==""`、`input[0].role==developer`、`input_text` |
| system 文本数组拼接 | `system_text_array_concatenated_into_developer_message` | `A\n\nB` |
| user text → input_text | `user_text_maps_to_input_text` | `input[0].role==user` |
| assistant text → output_text | `assistant_text_maps_to_output_text` | `output_text` |
| tool_use → function_call（arguments 为 JSON 串） | `tool_use_becomes_function_call_with_string_arguments` | `function_call`/`call_id`/`name`/`{"path":"/tmp"}` |
| tool_result → function_call_output（string） | `tool_result_becomes_function_call_output_string` | `function_call_output`/`output` |
| tool_result 含 image（数组保留二进制） | `tool_result_with_image_keeps_image_in_array` | `input_image` 条目不降级 |
| reasoning（encrypted_content 首轮不回传）等其余 | `...`（见源码 788–1010） | reasoning effort 6 级映射、tools/tool_choice/parallel_tool_calls/store 关闭 |

### 4.2 stream 状态机单测覆盖（`grok/stream.rs:611`，10 项）

覆盖 plan 要求的「text/reasoning/tool_use 三类块 SSE 序列」全部命中：

| 三类块 | 测试 | 关键断言 |
|---|---|---|
| reasoning 块生命周期 | `b2_b3_b4_thinking_block_lifecycle` | `content_block_start(thinking)` → `thinking_delta` → part.done 不立刻 stop → content_part.added(output_text) 先收尾 thinking 再开 text |
| text 块 | `b6_b7_b8_text_block` | `content_block_start(text)` → `text_delta` → `content_block_stop` |
| tool_use 块 | `b9_b10_b11_function_call_block` | `tool_use`/`input_json_delta` → arguments.delta → `content_block_stop` |
| 非流式 message 一次性 | `b13_non_streaming_message_one_shot` | 一次 `text_delta`+`content_block_stop` |
| 完成带 usage/cache | `b14_completed_emits_message_delta_and_stop_with_usage` | `message_delta`+`end_turn`+`message_stop`、`cached_tokens` 从 input 扣除进 `cache_read` |
| incomplete → max_tokens | `incomplete_max_output_tokens_maps_to_max_tokens` | `stop_reason==max_tokens` |
| **error 事件发致命 Err（不伪装 end_turn）** | `error_event_returns_fatal` | `feed_line(error..) → Err("rate limited")` |
| **上游早断 finish 收尾** | `finish_emits_default_terminus_when_upstream_closed_early` | `content_block_stop`+`message_delta`+`message_stop` |
| `[DONE]` 标记 | `done_marker_returns_done_flag` | `FeedOutcome::Events(_, true)` |
| created 只发一次 message_start | `b1_response_created_emits_message_start_once` | 第二次 created 不再发 |

### 4.3 proxy.rs 截断/停滞的 error 不伪装 end_turn（`proxy.rs:398` stream_response）

I/O 级流式转发，逐 chunk 读取 + 跨行 UTF-8 切分 + `StreamState` 翻译：

| 异常 | 处置 | 是否发 error 终止（而非伪装 end_turn） |
|---|---|---|
| 读 chunk `Err` | `event: error / type:upstream_error` 然后 `return` | ✅ |
| chunk `None`（上游正常 EOF） | `state.finish()` 收尾串后 `return` | （正常结束，非截断） |
| 持续 idle 到 `timeout` | `event: error / type:idle_timeout` 然后 `return` | ✅ |
| `feed_line Err`（stream_error） | `event: error / type:stream_error` 然后 `return` | ✅ |
| 非 UTF-8 chunk | `event: error / type:stream_error / 上游 SSE 非合法 UTF-8` 然后 `return` | ✅ |

`StreamState.finish()` 本身由既有单测 `finish_emits_default_terminus_when_upstream_closed_early` 验证「上游早断发 content_block_stop + message_delta + message_stop」。

### 4.4 前端测试（`GrokPage.test.tsx`）

```ts
vi.hoisted({ invokeMock, listenMock });
vi.mock("@tauri-apps/api/core", ...);   // invoke 转发 invokeMock
vi.mock("@tauri-apps/api/event", ...);  // listen 转发 listenMock（GrokOAuthCard 挂载即 resolve noop unlisten）

invokeMock 按命令分发：
  grok_status → {running:false, endpoint:...8083/messages}
  grok_pool   → {running:false, total:0, ...}（KeyPoolCard 走兜底文案）
  grok_oauth_status → {authorized:false, ...}
  default → ""

断言：
  1) 渲染：标题「Grok 代理」+ 「Grok 账号授权（OAuth）」+ 「Key 池状态」+ 「本地测试 8083」+ 认证模式选项 + 映射表标题
  2) 模型添加：placeholder「添加 Grok 模型，如 grok-4.3」input → 填 grok-3-mini → 点其所在 .mp-add 行内的「添加」
     → waitFor(invokeMock calledWith 'grok_set_models', {models:[grok-4.3, grok-3-mini-fast, grok-3-mini]})
     （页面有两个「添加」按钮——模型优先级 + 映射表各一，故用 input.closest('.mp-add') 精确定位行内按钮）
  3) Token 生成：点「生成安全 Token」→ placeholder「留空表示不校验 x-api-key」的 input.value 匹配 /^[0-9a-f]{64}$/
```

### 4.5 门禁结果（本会话末态全绿）

| 门禁 | 状态 | 说明 |
|---|---|---|
| `cargo fmt --all -- --check` | ✅ | 含 GROK_DIAG 段 + grok_diag_step |
| `cargo clippy --all-targets -- -D warnings` | ✅ | |
| `cargo check --tests` | ✅ | 编译期门禁（CLAUDE.md：本机 `cargo test --lib` 会 STATUS_ENTRYPOINT_NOT_FOUND cdylib 环境问题，非代码失败，故以 `check --tests` 为准） |
| `npm run build` | ✅ | tsc 类型检查 + vite 生产构建；GrokPage chunk 22.43 kB（gzip 7.79） |
| `npm test` | ✅ | 15 文件 / 46 用例全过（含新增 GrokPage.test 3 项；PageErrorBoundary 故意抛错栈为预期） |

---

## 5. 端到端验证 checklist（需真实 Grok Plus 账号，离线阶段未执行）

> 这些属于真机交互验证，需 `npm run tauri dev` + 真实账号联网。静态门禁全绿后由用户/联调阶段完成。

1. `GROK_DIAG=1 npm run tauri dev`，前端走 OAuth 授权（手机/浏览器用 Plus 账号登 x.ai、输入 `user_code`、同意 scope `grok-cli:access`）。
2. 授权完成 → 收到 `grok-oauth-done` 事件 → OAuth 卡显示账号 + 剩余有效时长。
3. 「启动」Grok 代理 → `grok_status` 显示 `http://127.0.0.1:8083` running。
4. 模型优先级行「▶ 测试」`grok_chat_test` 发一条带推理 + 工具调用的请求，看转换链输出正确 Anthropic SSE（text/reasoning/tool_use 三类块齐全）。
5. `GrokTestPanel` 复制 curl 直接打 8083，验证非流式 + 流式两种。
6. 启动页选「🟦 Grok 代理 (本地 8083)」profile → 启动 Claude Code → 让它实际调一个工具（读文件）→ 确认 `tool_use → function_call → function_call_output → 下一轮` 全链路通。
7. 模拟号被风控（OAuth 401 不可刷新）→ 配置切 `api-key` 模式、填官方 xai-* Key → 一键继续，OAuth token 不丢。
8. 看 `grok-diag.txt` 各步骤时间戳分布正常（无卡死、status 行随请求跳变）。

---

## 6. 安全约束沿用确认（Phase 5/6 未放松任何一条）

- OAuth token DPAPI 加密存 `grok-oauth.json`，绝不明文落 `config.json`；config 只存 `oauth_account` 邮箱标识。
- `device_code` / `token_endpoint` 机密——`grok_oauth_start` 返回前端载荷只含 `user_code`/`verification_uri`/`verification_uri_complete`/`expires_in`/`interval`，机密字段只后端用。
- `redirect::Policy::none()` 三处生效（proxy 转发 / OAuth discover+device+poll+refresh / `test_connection` 探测），3xx 一律判 502，防 SSRF + bearer/refresh 外泄。
- 非回环绑定强制 `auth_token >= 24` 字符（`require_auth_if_exposed`）；前端 GrokConfigForm 在 host 非回环时红字警示。
- 32 MiB 请求体上限（`MAX_REQUEST_BODY_BYTES`），超限返 Anthropic 形态 413。
- `ct_eq` 恒时比较本地 `x-api-key`，防时序侧信道。
- OAuth discovery SSRF 闸：endpoint 必须 https + host 落 x.ai / *.x.ai（拒 `x.ai.attacker.com`）。
- `AuthProvider::snapshot()` 从不暴露原始 token（OAuth 仅返 mode/refreshable/account/expires_at/expired）。
- 429 冷却从已构造好的 `auth_headers` 反解真实发送的 Key，不重新 pick。
