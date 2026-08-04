# Grok Provider 接入详细文档（Phase 3：OAuth + CLI Chat-Proxy + Responses 协议）

> 范围：本文档只覆盖 **Phase 3**（OAuth Device Code Flow 授权 + 官方 CLI Chat-Proxy + OpenAI Responses 协议转换）。Phase 1（共享层 + GrokConfig 骨架）、Phase 2（代理骨架 + AuthProvider trait + API Key 退路打通）已在各自阶段完成，本文不重复展开其内部实现，只在被 Phase 3 复用处点名。
>
> 实现基线：`rust/src-tauri/`（Tauri v2.1.1 + axum 0.7 + reqwest 0.12 rustls-tls）。本机 `cargo fmt --check` / `cargo check --tests` / `cargo clippy --all-targets -- -D warnings` 三门禁绿。
>
> 借鉴说明：协议契约参考 CLIProxyAPI（Go 项目，`internal/auth/xai/*`、`internal/runtime/executor/xai_executor.go`、`internal/translator/codex/claude/codex_claude_*.go`），**仅读其协议规约，Rust 从零实现，两项目代码相互独立**。

---

## 0. 一句话总结

用一个 **Grok Plus 会员的 x.ai 账号**走 OAuth2 Device Code Flow 拿 access token，调官方 **Grok CLI Chat-Proxy**（`https://cli-chat-proxy.grok.com/v1/responses`，标准 OpenAI Responses API），消费的是账号权益额度。本地起一个**并行 provider 代理**（独立端口 8083，避开 NVIDIA 的 8082），对 Claude Code 暴露标准 Anthropic Messages 端点 `POST /v1/messages`，代理层做 protocol 翻译 + 模型名映射，Claude Code 端完全不感知它接的是 Grok。官方 API Key（`https://api.x.ai/v1`）保留了作为号被风控时的退路 AuthProvider，两种模式共用同一套 Responses 协议 converter。

---

## 1. 模块拓扑

新增平级 `src-tauri/src/grok/`（不重构 `nvidia/`，只复用其 `shared::` 抽取物）：

| 文件 | 职责 |
|---|---|
| `grok/mod.rs` | `GrokState` 生命周期（`start/stop/status/set_models/pool_status`），独立 OS 线程 + 独立 tokio Runtime 跑 axum（参照 `nvidia/mod.rs`），`start()` 按 `auth_mode` 构造对应 `AuthProvider` |
| `grok/server.rs` | axum Router 装配，仅暴露 `POST /v1/messages`，挂 `DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES)` |
| `grok/proxy.rs` | 代理核心：`redirect::Policy::none()` + 3xx 判 502、429 冷却切 Key、5xx 切模型、4xx 直传、32 MiB 体上限、非回环强鉴权、ct_eq 恒时比较、`AuthProvider` 接线、Responses 请求构造 + 响应流转译派发；含 `test_connection` / `local_chat_test` |
| `grok/auth.rs` | `AuthProvider` trait + 两实现：`ApiKeyAuthProvider`（退路，复用 `nvidia::key_pool`）、`OAuthAuthProvider`（Phase 3 主线，CLI Chat-Proxy 专用身份头 + on_401 singleflight 刷新） |
| `grok/oauth.rs` | Device Code Flow：`discover` / `start_device_flow` / `poll_for_token` / `refresh_tokens`，处理 pending/slow_down/expired/denied；discovery SSRF 闸 |
| `grok/oauth_store.rs` | token DPAPI 加密存储 `grok-oauth.json`（access/refresh 绝不明文落盘，config.json 只存 email 标识） |
| `grok/converter.rs` | Anthropic Messages 请求 → Responses `input[]` 请求体；Responses SSE → Anthropic SSE 的字段级映射 |
| `grok/stream.rs` | Responses SSE → Anthropic SSE 状态机（`StreamState` + 19 条输入事件分发） |
| `grok/models.rs` | `GrokConfig` + `GrokAuthMode` + 模型优先级 / `map_model` 映射表占位 |

Tauri 命令注册于 `lib.rs::run()` 的 `invoke_handler!`（紧跟 `grok_chat_test` 之后）。共享层 `shared::security`（`is_loopback_host`/`require_auth_if_exposed`/`validate_base_url`）与 `shared::sse`（`split_complete_sse_lines` + `MAX_REQUEST_BODY_BYTES`）在 Phase 1 已提取，nvidia/grok 共用。

---

## 2. 协议契约（与 CLIProxyAPI 对齐点）

### 2.1 端点与请求头

| 模式 | base | 鉴权头 | 额外身份头 |
|---|---|---|---|
| OAuth（主线） | `https://cli-chat-proxy.grok.com/v1` | `Authorization: Bearer {access_token}` | `X-XAI-Token-Auth: xai-grok-cli` + `x-grok-client-version: 0.2.93` |
| API Key（退路） | `https://api.x.ai/v1` | `Authorization: Bearer {api_key}` | 无（这两条只对 cli-chat-proxy 有意义） |

请求路径统一 `POST {base}/responses`（流式/非流式同一端点，由 body 内 `stream` 字段控制）。
`Accept`：流式 `text/event-stream`、非流式 `application/json`。

> CLI Chat-Proxy 专用头的取值来自 CLIProxyAPI `xai_executor.go`：`xaiTokenAuthValue = "xai-grok-cli"`、`xaiClientVersionValue = "0.2.93"`。这两个值随上游升级同步，集中在 `auth.rs` 顶部常量。

### 2.2 OAuth2 Device Code Flow（`grok/oauth.rs`）

```
Discovery : GET  https://auth.x.ai/.well-known/openid-configuration
            → device_authorization_endpoint / token_endpoint
            （校验 scheme=https + host 落在 x.ai / *.x.ai，拒绝 x.ai.attacker.com 类伪域名）
DeviceCode : POST {device_authorization_endpoint}
             form: client_id={CLIENT_ID}&scope={SCOPE}
             → {device_code, user_code, verification_uri, [verification_uri_complete], expires_in, interval}
PollToken   : POST {token_endpoint}
             form: grant_type=urn:ietf:params:oauth:grant-type:device_code
                   &device_code={device_code}&client_id={CLIENT_ID}
             → authorization_pending（继续）/ slow_down（间隔 +5s 再续）
               / expired_token（终止）/ access_denied（终止）
               / 其它 error（终止带 desc）/ 成功（access/refresh/id_token/expires_in）
Refresh     : POST {token_endpoint}
             form: grant_type=refresh_token&client_id={CLIENT_ID}&refresh_token={...}
```

常量（与 CLIProxyAPI `types.go` 1:1）：

| 常量 | 值 |
|---|---|
| `CLIENT_ID` | `b1a00492-073a-47ea-816f-4c329264a828` |
| `SCOPE` | `openid profile email offline_access grok-cli:access api:access` |
| `ISSUER` | `https://auth.x.ai` |
| `DISCOVERY_URL` | `https://auth.x.ai/.well-known/openid-configuration` |
| `DEVICE_CODE_GRANT_TYPE` | `urn:ietf:params:oauth:grant-type:device_code` |
| `DEFAULT_POLL_INTERVAL` | 5s（端点未给 interval 时） |

`offline_access` 拿 refresh_token；`grok-cli:access` 消费 Plus 账号的 CLI Chat-Proxy 权益额度；`api:access` 同时保留官方 `api.x.ai` 路径权限（这给了"切退路不用重新授权"的能力）。

form 编码用 `url::form_urlencoded` 手工拼，**不开** reqwest 的 form 特性，维持 `default-features = false` 精简面（见 `Cargo.toml` 注释）。所有出站 `reqwest` 客户端一律 `redirect::Policy::none()`，auth.x.ai 端点出现 3xx 视为异常——这些端点本就不应重定向，禁止跟随防 bearer/refresh 泄漏。

### 2.3 请求体（Anthropic → Responses，`grok/converter.rs`）

Responses 的输入是线性 `input[]`（含 message/reasoning/function_call/function_call_output 条目），不是 Chat 的 `messages[]`。字段映射依据 CLIProxyAPI `codex_claude_request.go`：

| Anthropic Messages | Responses input[] |
|---|---|
| `system`（字符串或 `[{type:text}]`） | 首条 `{type:message, role:developer, content:[{type:input_text, text}]}`；顶层 `instructions` 保留空串占位（xAI executor 的 `normalizeCodexInstructions` 要求非 null） |
| user 文本块 | message role=user，`content:{type:input_text,text}` |
| assistant 文本块 | message role=assistant，`content:{type:output_text,text}` |
| thinking 块 | 独立 `{type:reasoning, summary:[], content:null, encrypted_content:<signature>}`（**丢弃 thinking 文本本身**，只回传 signature；首轮无 thinking 块，故首轮请求无 encrypted_content） |
| tool_use 块 | `{type:function_call, call_id, name, arguments}`（arguments 为 Anthropic `input` 序列化后的 JSON 字符串） |
| tool_result 块 | `{type:function_call_output, call_id, output}`；数组型 content 保留二进制 image 块（→ `input_image` 拼数组），无可用条目则 output 取字符串值 |
| image 块 | `content:{type:input_image, image_url: <data URI>}` |
| `tools[]` | `{type:function, name, description, parameters, strict:false}`（parameters 缺则补 `{type:object,properties:{}}`） |
| `tool_choice` | `auto` / `required` / `none` / `{type:"function",name}` |
| `thinking` 配置 | `reasoning.effort`（minimal/low/medium/high/xhigh 两特例 + `none`/`auto`）+ `reasoning.summary="auto"` + `include:["reasoning.encrypted_content"]` |
| `parallel_tool_calls` | 默认 true；`tool_choice.disable_parallel_tool_use=true` 时改 false |
| `max_tokens` | `max_output_tokens` |
| `stream` / `temperature` / `top_p` | 透传 |

**丢弃**上游不认的字段：`prompt_cache_retention`、`safety_identifier`、`stream_options`。`previous_response_id` 链式暂不启用（按无状态模式每轮重发 `input[]`，降低复杂度，后续优化再开）。

reasoning effort 6 级 budget 映射：把 Anthropic thinking budget_tokens 落到 low/medium/high 等 discrete effort 档位（与 CLIProxyAPI 一致）。

### 2.4 SSE 响应（Responses → Anthropic，`grok/stream.rs`）

`StreamState` 状态机，输入事件按 OpenBlock（None/Thinking/Text/ToolUse）分发：

| Responses 事件 | Anthropic 事件 |
|---|---|
| `response.created` | `message_start` |
| `response.reasoning_summary_part.added` | `content_block_start(thinking)` |
| `response.reasoning_summary_text.delta`（含 `reasoning_text.delta` 归一化） | `content_block_delta(thinking_delta)` |
| `response.content_part.added(output_text)` | `content_block_start(text)` |
| `response.output_text.delta` | `content_block_delta(text_delta)` |
| `response.content_part.done` | `content_block_stop` |
| `response.output_item.added(function_call)` | `content_block_start(tool_use)` + `content_block_delta(input_json_delta)` |
| `response.function_call_arguments.delta` | `content_block_delta(input_json_delta)` |
| `response.output_item.done(function_call)` | `content_block_stop` |
| `response.output_item.done(reasoning)` | 先 `signature_delta` 再 `content_block_stop` |
| `response.completed` / `response.incomplete` | `message_delta(stop_reason+usage)` + `message_stop` |
| `keepalive` | 忽略 |
| `error` | error 事件 |
| `[DONE]` | 收束 |

**reasoning 归一化**：上游可能发 `reasoning_text.*`，一律归一为 `reasoning_summary_*`（`content_index`→`summary_index`）。
**thinking_stop_pending**：reasoning 流未自然 stop 而下一块开始时，延迟补 stop。
**finish()**：流提前关闭时补发默认终止符，避免前端悬挂。
**usage 累积**：`response.completed` 的 usage 含 `cached_tokens`，转写时从 input 扣除并搬到 `cache_read_input_tokens`（与 nvidia/converter 输出侧同策略）。

### 2.5 非流式响应（`grok/converter.rs::responses_json_to_anthropic`）

`responses_json_to_anthropic` + `map_nonstream_stop_reason` + `map_usage`。stop_reason 与 usage 映射与流式侧收束逻辑一致，集中在此复用。

---

## 3. 认证策略可插拔（`grok/auth.rs`）

### 3.1 `AuthProvider` trait

```rust
#[async_trait]
pub trait AuthProvider: Send + Sync {
    fn auth_headers(&self) -> Result<AuthHeaders, String>;
    async fn on_401(&self) -> RefreshOutcome;
    fn snapshot(&self) -> Value;
    fn key_pool_opt(&self) -> Option<&SharedKeyPool> { None }
}
```

- `auth_headers()` 返回拼好的 `HeaderMap`（调用方克隆注入），不暴露原始 token 串。
- `on_401()` 异步——OAuth 模式要走网络刷新；API Key 模式无需异步但为统一 trait 形状用 `async_trait` 包成 `Pin<Box<Future>>`，从而 `Arc<dyn AuthProvider>` dyn-safe。
- `RefreshOutcome::{Refreshed, Unrecoverable}`：proxy 重试循环据此决定"刷一次再重试"还是"终止 + 透传上游错误 + 引导重授权"。
- `key_pool_opt()`：API Key 退路返回共享池引用供 proxy 做 429 冷却轮换；OAuth 返回 None（无多 Key 语义）。

trait 故意不替上层决定 base_url——两种模式 base 不同（cli-chat-proxy vs api.x.ai），由 `GrokConfig::effective_base_url` 统一裁决：OAuth→`oauth_base_url`、ApiKey→`api_base_url`。auth provider 只管"鉴权头 + 刷新 + 快照"。

### 3.2 `OAuthAuthProvider`（Phase 3 主线）

持有 `token: std::sync::Mutex<TokenStore>` + `token_endpoint: String` + `client: reqwest::Client`（redirect::none）+ `refresh_lock: tokio::sync::Mutex<()>`。

`auth_headers()`：access 空 → Err；否则拼 `Authorization: Bearer {access}` + `X-XAI-Token-Auth: xai-grok-cli` + `x-grok-client-version: 0.2.93`。

`on_401()` **singleflight 等价**语义——这是 Phase 3 最细的并发正确性问题点：

```rust
async fn on_401(&self) -> RefreshOutcome {
    let _guard = self.refresh_lock.lock().await;     // 阻塞锁，并发 401 全部排队
    let pre_access = self.token.lock().unwrap().access_token.clone();  // 加锁前快照
    if pre_access.trim().is_empty() { return RefreshOutcome::Unrecoverable; }
    let cur_access = self.token.lock().unwrap().access_token.clone();  // 拿到锁后再看
    if cur_access != pre_access {
        return RefreshOutcome::Refreshed;           // 已被前一个持锁者刷过，无需再刷
    }
    let refresh_token = self.token.lock().unwrap().refresh_token.clone();
    if refresh_token.trim().is_empty() { /* 需重授权 */ return RefreshOutcome::Unrecoverable; }
    match oauth::refresh_tokens(&self.client, &refresh_token, &self.token_endpoint).await {
        Ok(new_store) => {
            { let mut t = self.token.lock().unwrap(); *t = new_store.clone(); }
            if let Err(e) = oauth_store::save(&new_store) { /* 写盘失败 warn，内存已更新 */ }
            RefreshOutcome::Refreshed
        }
        Err(_) => RefreshOutcome::Unrecoverable,
    }
}
```

设计要点：
- 用**阻塞式 `lock().await`** 而非 `try_lock` 乐观派：让并发 401 全部排队，首个调用方执行刷新，后续调用方拿到锁后比对 access 快照——变了说明已刷过，直接 `Refreshed`；没变说明首刷已自救过且失败，本调用方也按不可恢复处理。这避免了"第一个刷新在途时其它并发调用方误判已刷完"的窗口。
- **刷新成功后落盘**（`oauth_store::save`），写盘失败只 warn 不阻断：内存已更新，下次重启才会回退旧 token。
- 刷新失败 / 无 refresh_token → `Unrecoverable`，上层终止 + 透传上游错误 + 前端引导重走 Device Code Flow（refresh 也失效属正常授权过期）。

`snapshot()` 脱敏：只返回 `{mode:"oauth", refreshable, account, expires_at, expired}`——**绝不暴露 token 本体**。account 是 email（非凭证），与 `GrokConfig::oauth_account` 对齐供 UI 展示"已授权 xxx@example.com"。

### 3.3 `ApiKeyAuthProvider`（退路，Phase 2 打通）

`nvidia::key_pool::KeyPool` 轮询 + 429 冷却；401 判不可恢复（Key 失效非过期，刷新无意义）；`snapshot` 对每个 key `mask_key` 脱敏。不复述。

---

## 4. Token 加密落盘（`grok/oauth_store.rs`）

设计目标：access/refresh **绝不以明文落盘**。

- 文件 `grok-oauth.json` 只含 `{ account, blob }`，`blob` 是 DPAPI（`CryptProtectData`）对"JSON 序列化的 `TokenStore`"加密后的密文（base64 标准字母 + padding）。
- 解密需当前 Windows 用户态（DPAPI `CryptUnprotectData`），跨用户/跨机器不可解——既防偷文件直接用，也天然绑定本机账号。
- `TokenStore` = `{ access_token, refresh_token, account, expires_at(epoch秒) }`。明文本体只在内存/DPAPI 密文中存在，不直接落盘。
- DPAPI 调用走 `crypt32.dll` 直接 FFI（不经第三方 crate，最小依赖）。
- 落盘走 `history.rs`/`config.rs` 同款**原子写**（临时文件→flush→sync_all→rename），损坏文件改名留证 + 回退 None，与 config/history 的"非静默回退"对齐。
- `config.json` 只存 `GrokConfig::oauth_account`（email 标识），**不含 token 本体**。
- 平台：DPAPI 仅 Windows；非 Windows（CI 上的 `cargo check --tests`）走 fallback 分支直接返回 Err——编译过但不提供功能，CI 门禁绿且不给非目标平台制造意外行为。

---

## 5. 生命周期与 Tauri 命令接线

### 5.1 `GrokState::start(cfg)`（`grok/mod.rs`）

参照 `nvidia/mod.rs`：独立 OS 线程 + 独立 tokio Runtime 跑 axum，避免命令线程 `block_on` 死锁。OAuth 分支（Phase 3 接线点）：

```rust
models::GrokAuthMode::Oauth => {
    let token = oauth_store::load().ok_or_else(|| "❌ OAuth 模式尚未授权：本地无凭证（grok-oauth.json）...".to_string())?;
    if token.access_token.trim().is_empty() { return Err("❌ OAuth token 损坏...".to_string()); }
    let oauth_client = oauth::http_client()?;
    // 探测一次 discovery 拿 token 端点并缓存到 provider，后续 on_401 刷新直接复用，不必每轮再 discover。
    // discovery 失败不致命：refresh_tokens 内部对空 endpoint 会自行补一次 discover。
    let token_endpoint = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(oauth::discover(&oauth_client)).map(|d| d.token_endpoint).unwrap_or_default(),
        Err(_) => String::new(),
    };
    let provider = OAuthAuthProvider::new(token, token_endpoint, oauth_client);
    tracing::info!(account = %provider.account(), "Grok OAuth 已加载凭证，准备启动代理");
    Arc::new(provider) as Arc<dyn AuthProvider>
}
```

授权判据是 `oauth_store::load()` 取回非空 access_token（**token 驱动，非 account 驱动**）。account 空与无 token 走同一未授权错误——不再单独要求 account 非空。

### 5.2 OAuth Tauri 命令（`lib.rs`，紧跟 `grok_chat_test` 之后）

| 命令 | 作用 |
|---|---|
| `grok_oauth_start(app)` | 发起 Device Code Flow。短路：已授权则 `{already_authorized:true}`。否则 `oauth::start_device_flow()`，**只把可公开字段回前端**（`user_code`/`verification_uri`/`verification_uri_complete`/`expires_in`/`interval`；`device_code`/`token_endpoint` 留服务端——机密绝不外泄）。随即 `tauri::async_runtime::spawn` 后台轮询 task：用仅含 poll 所需字段的 `DeviceCodeResponse` 调 `poll_for_token`，成功 → `oauth_store::save` + 写 `config.grok.oauth_account` + `cfg.save()` + emit `grok-oauth-done`（payload `{account, expires_at}`）；失败 emit `grok-oauth-error`（payload `{message}`） |
| `grok_oauth_status()` | 返回 `{authorized, account, expires_at, expired, refreshable}`（`oauth_store::load()` 无则全 false/0） |
| `grok_oauth_revoke(cstate)` | `oauth_store::clear()` + 清 `cfg.grok.oauth_account` + `cfg.save()` |

**轮询模型**：前端只调用一次 `grok_oauth_start`，后端 task 轮询并 emit 事件，**前端不高频 poll 命令**——避免前端高频轮询的事件往返开销。`tauri::Emitter` trait 必须在 `lib.rs` 作用域内（Tauri v2 把 emit 从 Manager 移到 Emitter；故 `use tauri::{Emitter, Manager}`）。后台 task 通过 `tauri::Manager::try_state::<std::sync::Mutex<Config>>(&app)` 访问托管 Config 状态写回 account。

完整 grok_* 命令矩阵（Phase 2 起，Phase 3 补完了 OAuth 三件 + `test_connection` OAuth 分支）：

`grok_oauth_start` · `grok_oauth_status` · `grok_oauth_revoke` · `set_grok_config` · `grok_set_models` · `grok_status` · `grok_pool` · `grok_start` · `grok_stop` · `grok_test`（直连上游自检）· `grok_chat_test`（端到端 POST 本机 8083 `/v1/messages`）。

### 5.3 `proxy.rs::test_connection`

OAuth 分支经 `oauth_store::load()` 取 token，拼 `Authorization: Bearer {access}` + 两条 CLI Chat-Proxy 身份头，base 走 `cfg.effective_base_url()`。probe body：`{model, instructions:"", input:[message/user "hi"], stream:false, max_output_tokens:16, store:false}`。两模式共享同样的 probe body，只差鉴权头与 base。

**Phase 3 顺带关掉的一个真实安全口子**：原来 probe 客户端是 `reqwest::Client::new()`（会跟重定向），上游被劫持成 3xx 时会把 bearer 顺着重定向泄漏。现改为 `redirect::Policy::none()` + `connect_timeout(15s)`，3xx → "上游返回 N 重定向（... 不应重定向，代理禁止跟随）"明确报错。与代理主转发路径的 redirect::none 策略对齐。

---

## 6. 安全约束清单（Phase 3 在 Phase 2 基础上维持 / 加固）

| 约束 | 实现 |
|---|---|
| 上游重定向禁止跟随 | `redirect::Policy::none()` 全路径（proxy 转发 + OAuth discover/device/token/refresh + test_connection probe）；3xx 判 502 fatal |
| SSRF | `GrokConfig::validate_base_url`（scheme+host 非空）保存期 + start 入口双调用；OAuth discovery SSRF 闸：endpoint 必须 https + host 落 x.ai / *.x.ai（拒 `x.ai.attacker.com`） |
| 凭证不外泄 | OAuth token DPAPI 加密 `grok-oauth.json`，config.json 只存 email；`device_code`/`token_endpoint` 留服务端，`grok_oauth_start` 不回传；`AuthProvider::snapshot` 脱敏（token 本体永不出现） |
| 鉴权头恒时比较 | token 校验走 `ct_eq` 避免计时侧信道逐字节泄露（与 nvidia/proxy.rs 同实现，grok 侧自留一份避免跨模块耦合） |
| 非回环强鉴权 | 非回环绑定时强制 `auth_token >= 24` 字符（`require_auth_if_exposed`，shared 层共用） |
| 请求体上限 | `MAX_REQUEST_BODY_BYTES = 32 MiB`，超限重塑成 Anthropic 形态 413 |
| 刷新串行化 | `refresh_lock: tokio::sync::Mutex` singleflight，防并发 401 重复 RefreshToken / 触发上游限流 |

`grok-oauth.json` 用标准 base64 alphabet + padding 包裹 DPAPI 密文（纯 Rust、小、零平台依赖）。OAuth `{grant_type, device_code/refresh_token, client_id}` 走 `url::form_urlencoded` 手工拼，不开 reqwest form 特性，维持 `default-features = false`。

---

## 7. 测试与门禁

### 7.1 后端单测要点

- `auth.rs`：`api_key_*` 一组（Bearer 头/轮询/401 不可恢复/snapshot 脱敏/全冷却报错/含非法字符报错）；OAuth 经 `block_on` 跑 `on_401`。
- `mod.rs`：`oauth_mode_without_token_is_unauthorized`、`start_requires_models`、`oauth_mode_token_driven_not_account_driven`、`apikey_mode_requires_keys`、`apikey_mode_starts_proxy_on_unique_port`。
  - **hermeticity**：`TokenRestore` save-and-restore guard——测试前若有真实 `grok-oauth.json` 就读出字节入内存 + 删文件重现"无凭证"环境，Drop 时无论成败写回。**不毁用户真实 token**。
- `converter.rs` / `stream.rs`：~20 个单测覆盖 text / reasoning / tool_use 三类块的 SSE 序列、B1-B19 输入事件分发、reasoning 归一化、function_call arguments 拆分、usage cached_tokens 搬运。
- `proxy.rs::test_connection` 加 redirect::none 后，3xx 路径有明确错误断言。

### 7.2 门禁（CI 静态三绿）

```
cargo fmt --all -- --check
cargo check --tests
cargo clippy --all-targets -- -D warnings
```

`cargo test --lib` 在本机 Windows 会偶发 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` —— 这是 cdylib 运行时 DLL 加载的环境问题（测试二进制能干净编译），**不是代码/断言失败**；`cargo check --tests` 绿即编译期门禁绿（详见 `rust/CLAUDE.md`「测试说明」）。

---

## 8. 端到端验证路径（Phase 6 的目标，前置列出供对照）

1. `GROK_DIAG=1 npm run tauri dev`，前端走 OAuth 授权（手机/浏览器用 Plus 账号登入 x.ai，输入 user_code）。
2. 听到 `grok-oauth-done` 事件后刷新授权面板状态。
3. 前端启动 Grok 代理，`grok_status` 显示 `http://127.0.0.1:8083` running，`grok_pool` 显示 OAuth 脱敏快照。
4. `grok_test` 直连 cli-chat-proxy 自检通；`grok_chat_test` 端到端发一条带 reasoning + tool_use 的请求，看转换链输出正确 Anthropic SSE。
5. 选「Grok 本地代理」profile（env `ANTHROPIC_BASE_URL=http://127.0.0.1:8083`、`ANTHROPIC_AUTH_TOKEN=<本地代理 auth_token>`、`ANTHROPIC_MODEL=claude-sonnet-*`），启动 Claude Code 实跑一轮对话 + 一次工具调用，确认 `tool_use → function_call → function_call_output → 下一轮` 全链路通。
6. 401 触发刷新验证：手动让 access_token 失效（删 `grok-oauth.json` 的 expires_at 或等过期），下一请求应自动 refresh + 落盘 + 继续成功。

---

## 9. 后续未接阶段（仅标注，非 Phase 3 范围）

- **Phase 4（模型名映射）**：`map_model` 已在 proxy 接线基本就位，待前端 `grok_set_models`/映射表 UI 接通与真实 Grok 上游日志对照验证。
- **Phase 5（前端 UI + profile）**：`src/pages/grok/*` 镜像 NVIDIA UI 结构——OAuth 授权面板（显示 user_code/verification_uri + 监听 `grok-oauth-done` 事件）、配置编辑、模型映射表、起停测试按钮、"Grok 本地代理" profile 预置（env 指向 8083）。
- **Phase 6（测试门禁 + 诊断钩子）**：`GROK_DIAG=1` 诊断钩子（复刻 `lib.rs::diag_step` 模式写 `grok-diag.txt`）、converter 单测覆盖 truncation/stall 发 error 不伪装 end_turn、全门禁实跑。

---

## 附：Phase 3 关键文件清单

| 文件 | 关键符号 |
|---|---|
| `src-tauri/src/grok/auth.rs` | `AuthProvider` trait · `ApiKeyAuthProvider` · `OAuthAuthProvider` · `RefreshOutcome` · `XAI_TOKEN_AUTH_VALUE`/`XAI_CLIENT_VERSION_VALUE` · `epoch_secs` |
| `src-tauri/src/grok/oauth.rs` | `discover` · `start_device_flow` · `poll_for_token` · `refresh_tokens` · `http_client` · `validate_oauth_endpoint` · `CLIENT_ID`/`SCOPE`/`DISCOVERY_URL` |
| `src-tauri/src/grok/oauth_store.rs` | `TokenStore` · `load` · `save` · `clear` · `path` |
| `src-tauri/src/grok/converter.rs` | Anthropic→Responses 请求体构造 · `responses_json_to_anthropic` · `map_nonstream_stop_reason` · `map_usage` |
| `src-tauri/src/grok/stream.rs` | `StreamState` · `FeedOutcome` · B1-B19 输入事件派发 |
| `src-tauri/src/grok/proxy.rs` | `ProxyCtx` · `handle_messages` · `test_connection`(OAuth 分支 + redirect::none probe) · `local_chat_test` · `ct_eq` |
| `src-tauri/src/grok/mod.rs` | `GrokState` · `start`(OAuth 分支) · `stop` · `status` · `set_models` · `pool_status` · `TokenRestore` |
| `src-tauri/src/grok/models.rs` | `GrokConfig` · `GrokAuthMode` · `effective_base_url` · `map_model` · `require_auth_if_exposed` |
| `src-tauri/src/lib.rs` | `grok_oauth_start` · `grok_oauth_status` · `grok_oauth_revoke` · `epoch_secs` · invoke_handler 注册 · `use tauri::{Emitter, Manager}` |
| `src-tauri/Cargo.toml` | `base64=0.22` · `url=2` · `async-trait=0.1` · reqwest `default-features=false + json+stream+rustls-tls` |
