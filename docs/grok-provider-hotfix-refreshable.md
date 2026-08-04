# Grok Provider Hotfix：`grok-oauth-done` 事件漏带 `refreshable` 致「无可续期凭证」红字常驻

> 日期：2026-08-04　分支：`feat/rust-implementation`　触发：Phase 5/6 完成后真机端到端验证阶段发现
> 性质：**纯前端显示 bug**，不影响实际续期能力，不影响代理启动。OAuth 流程与 DPAPI 存储均正确。
>
> 本批同时顺带修了「开箱无默认模型」问题（见 §7），让新装 + 既有空配置自动带 grok-4.5 / grok-3-mini-fast，越过 `grok_status` 的 `models.is_empty()` 闸。

---

## 1. 现象

真机授权 Grok Plus 账号成功后，OAuth 卡片显示：

```
凭证 剩余 5h59min；无可续期凭证
```

红字「无可续期凭证」在授权成功后**常驻不消失**，与「access token 6h 才过期、明显需要续期」的直觉矛盾。

## 2. 排查：先验证 grok-oauth.json 里到底有没有 refresh 字段

`grok-oauth.json` 落盘外壳只有 `{ account, blob }`，`blob` 是 DPAPI 密文（base64），明文 token 字段全部藏在密文里。要确认 `refresh_token` 有无，必须用 DPAPI 解密。

在用户明确授权后，于本机 PowerShell 用 `[System.Security.Cryptography.ProtectedData]::Unprotect($cipher, $null, 'CurrentUser')` 解开 `blob`（与后端 `grok/oauth_store.rs::dpapi::unprotect` 完全同口径：flags=0、无 entropy、CurrentUser 作用域），结果：

| 字段 | 实际值 | 说明 |
| --- | --- | --- |
| `access_token` | len=786，非空 | ✅ |
| `refresh_token` | **len=86，非空** | ✅ x.ai 正常下发 |
| `account` | len=23（zuo.jianxiang@gmail.com） | ✅ |
| `expires_at` | 1785829637（→ 2026-08-04 15:47:17） | 距当时约 3.6h |

**结论：token 包完整，`offline_access` scope 生效，x.ai 确实下发了 refresh_token。**「无可续期凭证」是假象。

> DPAPI 密文头 `AQAAANCMnd8BFdERjHoAwE/`（hex `01 00 00 00 d0 8c 9d df`）是标准 DPAPI blob 头（Masters version 1 + AES-CBC），证明密文合法、CurrentUser 作用域加密，与代码一致。

## 3. 根因

两处协同导致红字常驻：

### 3.1 后端事件 payload 漏字段（`src-tauri/src/lib.rs`，授权成功 emit 处）

原 `grok-oauth-done` 事件只带 `{ account, expires_at }`：

```rust
let _ = app.emit(
    "grok-oauth-done",
    serde_json::json!({
        "account": store.account,
        "expires_at": store.expires_at,
    }),  // ← 缺 refreshable / expired
);
```

而后端另一个命令 `grok_oauth_status`（`lib.rs::grok_oauth_status`）算 `refreshable` 的口径是：

```rust
"refreshable": !t.refresh_token.trim().is_empty(),
```

两条路径口径不一致：事件路径丢了 `refreshable`，前端拿不到真值。

### 3.2 前端事件 handler 沿用陈旧 `refreshable`（`src/pages/grok/GrokOAuthCard.tsx`）

授权成功 handler 用 `prev.refreshable` 兜底：

```ts
listen<{ account: string; expires_at: number }>("grok-oauth-done", (e) => {
  setState((prev) => ({
    ...prev,
    authorized: true,
    account, expires_at,
    expired: false,
    refreshable: prev.refreshable,   // ← 沿用授权前的旧值
    ...
  }));
```

而 `refreshable` 的初值来自 `GrokPage.tsx` 挂载时一次性调的 `grok_oauth_status`（`refreshOauth`，`useEffect[refreshOauth]`）。**授权前 token 文件不存在 → `load()` 返回 None → `refreshable: false`**。这个 false 被事件 handler 原封沿用 → 红字常驻。

且授权成功后**没有任何路径重新 invoke `grok_oauth_status`**——`refreshOauth` 只在挂载调一次，事件 handler 不调它。所以即便后端 `grok_oauth_status` 此刻能返回正确的 `refreshable: true`，前端也再也不去拉。

### 3.3 为什么不影响实际续期

续期逻辑在 `auth.rs::OAuthAuthProvider::on_401`（行 248-270）：401 时用 `token.refresh_token` 调 `oauth::refresh_tokens`，singleflight 串行化；`proxy.rs:272-274` 对 401 refresh 一次重试。**这套逻辑直接读 `TokenStore`，与本 bug 无关**——红字只是 UI 误报，access 过期后代理仍会真续期。

## 4. 修复（修法 A：后端事件带 `refreshable`，前后端单一数据源）

### 4.1 后端 `lib.rs` emit 处补字段

```rust
let now = epoch_secs();
let expired = store.expires_at != 0 && now >= store.expires_at;
let refreshable = !store.refresh_token.trim().is_empty();
let _ = app.emit(
    "grok-oauth-done",
    serde_json::json!({
        "account": store.account,
        "expires_at": store.expires_at,
        "expired": expired,
        "refreshable": refreshable,
    }),
);
```

口径与 `grok_oauth_status` 完全对齐（同一个 `epoch_secs()` + 同一个 `refresh_token.trim().is_empty()` 判空）。

### 4.2 前端 `GrokOAuthCard.tsx` handler 采用回传值

```ts
listen<{ account: string; expires_at: number; expired?: boolean; refreshable?: boolean }>(
  "grok-oauth-done",
  (e) => {
    const { account, expires_at, expired, refreshable } = e.payload;
    setState((prev) => ({
      ...prev,
      authorized: true,
      account, expires_at,
      expired: expired ?? prev.expired,
      refreshable: refreshable ?? prev.refreshable,  // ← 采用回传值，缺字段退回 prev（旧后端兼容）
      ...
    }));
```

`??` 兜底保证对旧后端（不带这些字段的事件）仍兼容，不破坏向后兼容。

## 5. 验证

五道静态门禁全绿（`rust/` 子目录）：

| 门禁 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ green |
| `cargo clippy --all-targets -- -D warnings` | ✅ green |
| `cargo check --tests` | ✅ green（41s）|
| `npm run build`（tsc + vite） | ✅ green |
| `npm test`（vitest + jsdom） | ✅ 46/46 |

> 注：`npm test` 偶发非零退出码是 jsdom 低资源下 "page render failed" 渲染日志噪声 + 偶发 teardown 超时，**用例计数稳定 46/46 全绿**（verbose reporter 复核确认，含 GrokPage 两个用例 + App.test 全过）。门禁以用例结果为准。

生产构建：

- `npm run tauri build`：release cargo 编译 7m15s，NSIS 打包成功。
- 产物：`src-tauri/target/release/bundle/nsis/Claude Launcher_1.0.0_x64-setup.exe`（4.69 MB）。

> 构建 release exe 前须确认无进程占用 `target/release/claude-launcher.exe`（上次曾被运行实例锁住致 `os error 5 拒绝访问`，本热修复前已用 `[IO.File]::Open(.., 'None')` 探针确认 FREE 再 build）。

## 6. 真机回归清单（装新包后）

1. 装新 setup，启动后进 Grok 页。
2. 若此前已授权：先「登出」清凭证，再点「授权 Grok 账号」重新走 Device Code Flow（避免读到旧事件 state）。
3. 授权成功后，OAuth 卡片应显示「凭证 剩余 ~6hxxmin；**过期会自动用 refresh_token 续期**」（绿/中性文字，不再是红字「无可续期凭证」）。
4. 关 app、改系统时间过 access 过期点或等到期，再启动代理发请求：观察 `grok-diag.txt` 应出现 `on_401 refresh` 路径续期、新 access 写回 `grok-oauth.json`、请求成功重试。
5. 极端验证：`grok_oauth_revoke` 登出后，文件被清 → `refreshable` 应回 false 且红字重新出现（这是真·无凭证，符合预期）。

## 7. 顺带修：开箱无默认模型（「请先配置至少一个 Grok 模型」闸挡启动）

同一轮真机验证还撞到第二个问题：授权成功后点「启动 Grok 代理」被挡：

```
❌ 请先配置至少一个 Grok 模型
```

这是 `grok/mod.rs::start()` 的 `cfg.models.is_empty()` 闸（line 123-125）。原设计 `GrokConfig` 的 `models` / `model_map` 默认是空 `Vec`，把「填什么模型」完全交给用户在 GUI 手配——对首次用户不友好，也违背「装上即能用」。

### 7.1 修复：默认模型 + 既有空配置迁移

`grok/models.rs`：

- 新增 `default_models()` → `["grok-4.5", "grok-3-mini-fast"]`，`default_model_map()` → 一条样例 `claude-sonnet-4 → grok-4.5`。
- `models` / `model_map` 字段改用 `#[serde(default = "default_models")]` / `default = "default_model_map"`：反序列化**字段缺失**时填默认（新装用户、老配置没这两字段时受益）。
- `Default::default()` 也用这两函数。
- 新增 `migrate_default_models(&mut self) -> bool`：**既有 config.json 里 `models` 是空数组**（serde 此时不会触发 default 函数，而是用文件里的空值）时，补默认 + 返回 true；已非空则不动、返回 false。幂等。

`ive config.rs::load_or_default()`：解析成功后调 `cfg.grok.migrate_default_models()`，返回 true 才 `save()` 一次。

效果：
- **新装**用户：`Config::default()` 自带默认模型，开箱即用。
- **既有空配置**（如本机这份 `models: []`）：首次启动自动补默认并落盘，下次读出来就是非空。**无需用户手点 GUI**。
- **已配模型**的用户：迁移函数跳过，原样尊重用户改动。

其余 claude-* 的映射由 `map_model` 通配兜底（haiku 系→含 `mini-fast` 的档、其它→`models[0]`），故默认 `model_map` 只放一条样例即可，保持表短可读。

### 7.2 测试

`grok/models.rs` 测试模块新增：

- `default_now_ships_models_and_map`：`GrokConfig::default()` 的 `models` 含 grok-4.5/grok-3-mini-fast、`model_map` 非空、`map_model` 通配正确（sonnet→4.5、haiku→mini-fast、opus→models[0]）。
- `migrate_default_models_is_idempotent_and_respects_user_models`：空→补（true）→再调不动（false，幂等）；用户已配 `["grok-4.3"]` → 不覆盖（false，原样）。
- 原 `map_model_passthrough_when_unconfigured` 改为**显式构造空配置**测透传（`GrokConfig::default()` 现非空，不再代表「未配置」态）。

## 8. 改动文件

| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/lib.rs` | `grok-oauth-done` emit payload 加 `expired` + `refreshable`（与 `grok_oauth_status` 同口径，复用 `epoch_secs()`） |
| `src/pages/grok/GrokOAuthCard.tsx` | done 事件 handler 用 payload 的 `expired`/`refreshable` 覆盖，`??` 兜底旧后端兼容；不再沿用 `prev.refreshable` |
| `src-tauri/src/grok/models.rs` | `models`/`model_map` 改 `serde(default=...)`；新增 `default_models()`/`default_model_map()`；`Default` 用之；新增 `migrate_default_models()` + 3 条测试 |
| `src-tauri/src/config.rs` | `load_or_default` 解析成功后调 `cfg.grok.migrate_default_models()`，true 才 `save()` |

> §1–4 的 refreshable 修复：`grok_oauth_status`、`OAuthAuthProvider::on_401` 续期、`oauth_store` DPAPI 加解密**均未改动**——只补齐事件路径漏掉的字段。
> §7 的默认模型修复：未动 `grok/mod.rs` 的 `models.is_empty()` 闸（保留作最后一道兜底）、未动 `map_model` 通配逻辑，只在配置层补默认。

## 9. 复盘要点

- **两路径口径漂移**：后端既有 `grok_oauth_status`（拉取路径）又有 `grok-oauth-done` 事件（推送路径），两路径返回的授权状态字段集本应对齐却没对齐。教训：凡是「同一信息既 polling 又 push」的地方，push payload 必须 = polling 响应的子集/超集，避免前端两套口径。
- **前端事件 handler 用 `prev.xxx` 兜底是隐藏的陈旧值陷阱**：当被兜底的字段在授权前后会变化时，`prev` 就是过期的。应优先采用 payload 回传值，仅对真正「不变」的字段用 `prev`。
- **DPAPI 解密验证 = 该机调试 token 存储的金标准**：与其猜 token 字段有无，不如用 `[ProtectedData]::Unprotect` 解开看真实 JSON 结构——一次解密定性回答了「是 x.ai 策略还是我们的 bug」，省去反复读 OAuth 流程代码的迂回。
- **「空 Vec 默认」对首次用户是陷阱**：`#[serde(default)]`（空集合）在「字段缺失」时给空，在「字段是 `[]`」时也给空——但首次用户既没字段也没配，二者都该落到「合理默认」而非「空」。凡有「启动闸门判断 `xxx.is_empty()`」的字段，默认就该非空 + 加迁移兜底，否则用户被挡在「请先配 X」的门外才发现要手配。

## 10. 真机验证第二批（2026-08-05）：启动代理白屏 + OAuth 链接不能点开

第二批真机验证又暴露两个非协议层、纯 Tauri 集成层的问题，同批修。

### 10.1 问题：点「启动 Grok 代理」后白屏「页面暂时无法显示」，但代理实际已启动

**根因**：`grok/mod.rs::start()` 里有一段同步 `block_on(oauth::discover(&oauth_client))` 探测 `https://auth.x.ai/.well-known/openid-configuration` 拿 token 端点缓存进 provider。`grok_start` 是**同步 Tauri 命令**，`start()` 在 Tauri 命令线程同步执行——`block_on` 这次网络往返轻则几秒卡顿、重则超时几十秒，期间 webview 的 IPC 长时间得不到回程 → 触发 `PageErrorBoundary` 兜底「页面暂时无法显示」。而代理 axum 是 `start()` 后段 `std::thread::spawn` 起的——discover 块把 `start()` 阻塞 N 秒后才继续，代理线程确实在后台起来了，故「实际已启动」但 GUI 白屏。

**修复**：删掉启动阶段的同步 discover，给 `OAuthAuthProvider` 传**空 token_endpoint**。`oauth::refresh_tokens` 在 token_endpoint 为空时会自行 `discover` 补端点（见 `oauth.rs` refresh 的空 endpoint 分支），且 `on_401` 在代理独立 tokio Runtime 上 async 跑——discover 推迟到运行时，启动立即返回，且不再依赖启动时 auth.x.ai 可达。

> 已知小代价：on_401 每次因 `self.token_endpoint` 是 `String` 不可变、不回写缓存，会 discover 一次。但 401 只在 access 6h 过期才触发（一天数次），每次多一次 discover 网络往返可接受。后续若优化：把 `token_endpoint` 改 `Mutex<String>`，on_401 刷新成功后回写缓存即可。当前不动，保持改动面最小。

`OAuthAuthProvider::new` 的文档注释早已写明「token_endpoint 可空，会在首次 refresh 时补 discover」——设计当初就预留了此退路，本次只是把启动路径对齐到这个已预留的延迟补 endpoint 模式。

### 10.2 问题：带认证码的授权链接要点开用默认浏览器

**根因**：`GrokOAuthCard.tsx` 授权指引里的 `verification_uri` / `verification_uri_complete` 用 `<a href={...} target="_blank">`。Tauri webview 里 `target="_blank"` 默认**不会**把链接抛给系统默认浏览器，行为依赖 webview 自身（常被忽略/拦截），用户只能复制链接到外部浏览器。

**修复**：接入官方 `tauri-plugin-shell` 的 `open(url)` API，点击链接 → `e.preventDefault()` + `open(url)` → 走系统默认浏览器。改动：
- 后端 `Cargo.toml` 加 `tauri-plugin-shell = "2"`；`lib.rs::run()` 注册 `.plugin(tauri_plugin_shell::init())`；`capabilities/default.json` 加 `"shell:allow-open"`（精确，只放行 open，不开其它 shell 命令执行）。
- 前端装 `@tauri-apps/plugin-shell@^2`；`GrokOAuthCard.tsx` 把两处 `<a target="_blank">` 改成 onClick 调 `openExternal(url)`，内部 `open()` 失败时 `window.open` 兜底（shell 插件未就绪的极端情况）。

### 10.3 第二批改动文件

| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/grok/mod.rs` | 删 `start()` 里的同步 `block_on(oauth::discover)` 块，给 provider 传空 token_endpoint（运行时由 refresh_tokens/on_401 延迟补） |
| `src-tauri/Cargo.toml` | 加 `tauri-plugin-shell = "2"` |
| `src-tauri/src/lib.rs` | `.plugin(tauri_plugin_shell::init())` 注册 |
| `src-tauri/capabilities/default.json` | `permissions` 加 `"shell:allow-open"`（仅 open，不放开 shell 执行） |
| `package.json` | 加 `@tauri-apps/plugin-shell@^2` |
| `src/pages/grok/GrokOAuthCard.tsx` | import `open`；新增 `openExternal()`（shell.open + window.open 兜底）；两处验证地址链接改成 onClick 调它，文案改「点击直接用默认浏览器打开登录即可」 |

五道门禁全绿（fmt/clippy -D warnings/check --tests/npm build/npm test 46/46）+ 重出 NSIS 装包。

## 11. 复盘要点（第二批）

- **同步 Tauri 命令里 `block_on` 网络 = webview 白屏定时炸弹**：命令线程被阻塞期间 IPC 无回程、`PageErrorBoundary` 兜底白屏，但后台线程已起来的工作（代理/子进程）照跑 → 用户看到「实际已启动」却白屏。教训：Tauri 命令（尤其非 `async` 的同步命令）里**禁止任何 `block_on` 走外网**；要么命令改 `async`（在 Tauri runtime 上），要么把网络往返推到独立线程/Runtime 异步做。`grok/mod.rs` 注释早写了「避免命令线程 block_on 死锁」，discover 这段是漏网的违反者。
- **Tauri webview 不自动把 `target="_blank"` 抛给系统浏览器**：桌面 webview 的导航策略和真浏览器不同，`<a target=_blank>` 默认被 swallow。任何「打开外部 URL」的桌面需求都应走 `tauri-plugin-shell::open`（跨平台正确调用默认浏览器/邮件/文件管理器）+ capability 精确放行 `shell:allow-open`，而非裸 `<a target=_blank">`。
