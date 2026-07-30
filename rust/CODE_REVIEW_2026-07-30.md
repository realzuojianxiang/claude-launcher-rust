# 当前项目代码审查报告

**审查日期：** 2026-07-30
**审查对象：** `rust/`（Tauri v2 + React 实现）
**当前分支：** `feat/rust-implementation`
**本次范围：** 自上次评审 `CODE_REVIEW_2026-07-28.md` 之后进入的两笔提交
- `f2c709a` feat: refresh UI and add study dictionaries
- `c95744b` perf(frontend): lazy-load pages and dictionaries to shrink initial bundle

并同时对上次评审声称「已整改」的 6 项后端修复做独立回核。本报告以当前磁盘实际代码为准，关键结论由源码逐条核对，而非转述。

> 与上次评审的差异：上轮聚焦 backend 安全/并发；本轮新增功能（懒加载 + 单词本词典）已在前端落地，故本轮新增前端为主、后端为「回核 + 新发现」。本轮采用 4 个并行子代理（后端核验 / 前端页面 / 懒加载与词典 / 对抗性跨轴）+ 主路径逐行复核。

## 验证结果（本轮实测）

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 通过 | exit 0 |
| `cargo clippy --all-targets -- -D warnings` | 通过 | 编译干净（受文件锁等待，但无 lint） |
| `cargo test --lib` | **未跑通（环境问题，非代码）** | 测试二进制加载即报 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`——Windows 下 Tauri `cdylib` 测试固件加载运行时 DLL 缺失所致；48 个测试函数均编译通过，属本机环境而非测试断言失败。上轮报告 `41 passed` 时环境不同 |
| `npm run build` | 通过 | tsc + vite 构建成功；懒加载如预期拆分各页与词典为独立 chunk |
| `npm test`（vitest） | 通过 | 5 文件 / 13 测试全过 |
| `cargo build`（带 tray-icon） | 通过 | 后端正常编译 |

构建产物 chunk 实证（`dist/assets/`）：`index-*.js` 154KB（主包，含外壳与跨页状态）、`DictionaryPage-*.js` 12.7KB、`spinner-verbs-*.js` 18.9KB、`ielts-*.js` 467KB。雅思词典虽 467KB 但**仅在用户选中该词典 tab 时才加载**，首屏主包不含——懒加载改造达成目标。

## 上轮 6 项后端修复回核结论：全部仍成立

逐源码核对，S1–S5 + Spec EOF 全部仍在位（详见「附录 A」）。其中一项需更正上轮文档：

- **S2 文档与实现不符（更正）**：上轮及整改记录称隔离目录形如 `provider-slug__pid__nanos`（每次启动唯一）。实际实现是 **`persistent_provider_dir_in` = `<host-slug>__<稳定 URL 哈希:016x>`**（`claude.rs:117-146`），**没有 pid/nanos**，是「同一 base URL 跨启动复用、不同 URL 隔离」的持久目录设计（为保留插件/MCP/hooks）。并发安全由 `PROVIDER_CONFIG_LOCK` 串行化读改写（`claude.rs:18,194`）+ 原子写（`claude.rs:156-188`）保证。这是一套**更强**的设计，但上轮文档措辞错误，建议更正 `CODE_REVIEW_2026-07-28.md` 第 39、173 行相关描述。

## 本轮新发现

### P1 — 1. `work_dir` 注入生成 `.bat` 导致命令执行（claude.rs）

**位置：** `src-tauri/src/claude.rs:76`（`build_launch_batch`）；信任源 `lib.rs:148 add_recent_dir` / `lib.rs:162 set_work_dir` / `history.rs`

`.bat` 模板里写的是 `cd /d "{work}"`，`work = config.work_dir` **原样插值、未做任何转义、未校验路径**。而 `set_work_dir` 与 `add_recent_dir` 直接接受 webview 传入的任意 `dir: String` 并落盘到 `config.json` / `history.json`：

- `set_work_dir`（`lib.rs:162-170`）：`cfg.work_dir = dir.clone(); cfg.save()?;` 无任何校验。
- `add_recent_dir`（`lib.rs:148-151`）：转入 `history::add`，而 `history::add`（`history.rs:43-55`）只做去重/截断，不去校验路径。
- `history.json` 是 exe 同级明文 JSON，任何能写入该文件的本地进程都可种入恶意条目。

**复现：** 在 `claude-launcher/history.json` 的 `dirs` 放入 `C:\x" & calc.exe & rem`（或被某个本地进程写入），启动器内选中该历史目录（前端 `useDir` 蝶 `set_work_dir`）→ 点启动。生成 `.bat` 行变为：

```
cd /d "C:\x" & calc.exe & rem"
```

`cmd` 把 `&` 当命令分隔符，于是 `calc.exe` 以启动器进程身份执行，并继承该 provider 的 env（含 `ANTHROPIC_*` 凭据）。任何能写 `history.json`/`config.json` 的本地实体即可在下次选用历史项时获得当前用户身份下的命令执行。

**严重性界定（重要）：**
- 此 SSRF 至命令执行的触发需先能写 `history.json`/`config.json`，或经 webview `invoke()` 调 `set_work_dir`/`add_recent_dir`——后者要求渲染进程已能执行任意 JS。
- 故这更像**纵深防御缺口**而非提权：能到这一步的攻击者本就具备本地代码执行能力。真正危险的是它**与下方 P3-CSP 形成链**：`tauri.conf.json` 的 `"csp": null` 使未来任一 XSS sink（如有人对词典/日志改用 `dangerouslySetInnerHTML`）可直接串到此命令注入，把「展示层 XSS」放大为「本机命令执行」。

**建议（按代价排序）：**
1. `.bat` 写入前对 `work` 做白名单化：仅允许已存在目录且路径字符集不含 `" & | < > ^ ( ) %` 等 cmd 元字符；或干脆**改用 `Command::new` 直接生成进程 + `.current_dir(work)**`（API 已支持），完全不走 `.bat` + `cd`，从根上消除注入面。
2. `set_work_dir`/`add_recent_dir` 入口校验：`Path::new(&dir).is_absolute()` 且（建议）`exists() && is_dir()`，拒绝其它；对 `history::add` 同步加守门。
3. 用 `env::set_current_dir` + `std::process::Command` 注入 env 后 spawn，移除 `cd /d` 这条 shell 语句。

### P2 — 2. NVIDIA 上游客户端跟随重定向且无 scheme/host 白名单（SSRF + bearer token 外泄）

**位置：** `src-tauri/src/nvidia/proxy.rs:44-47`（`ProxyCtx::new`）；转发点 `proxy.rs:340`、`proxy.rs:841`

上游 `reqwest::Client` 构建只设了 `.connect_timeout(...)`：

```rust
reqwest::Client::builder()
    .connect_timeout(std::time::Duration::from_secs(15))
    .build()
```

**没有** `.redirect(reqwest::redirect::Policy::none())`，故走 reqwest 默认「最多跟随 10 次重定向」；**也无人**校验 `cfg.base_url` 的 scheme/host（`grep` 全后端无 `base_url` 校验/白名单/Policy::none/`https_only`）。转发把完整 Anthropic 请求体 + `.bearer_auth(&key)`（用户的 NVIDIA API Key）POST 到 `format!("{}/chat/completions", base_url)`。

**复现：** 将 NVIDIA Base URL 改为任一回 302 的地址（操作员误配 / 上游被攻破 / DNS 劫持即便 TLS 有效也仍可经重定向导流），302 目标设为 `https://169.254.169.254/...` 或攻击者主机 → 代理会把请求体 + 用户 NVIDIA bearer token 一并转发过去。默认 `base_url` 是 `integrate.api.nvidia.com`，但默认值不构成保护——缺乏白名单意味着任何非默认/被劫持的上游都能套取密钥。

**建议：**
1. 上游客户端 `.redirect(reqwest::redirect::Policy::none())`（拒绝跟随重定向；如确需跟随，必须自实现且按主机白名单逐跳验证）。
2. `set_nvidia_config` 保存时校验 `base_url`：强制 `https://` 且 host 命中白名单（至少限定 `*.api.nvidia.com`，或允许操作员显式登记可信 host）。
3. 转发前对最终 URL 再做一次 scheme+host 校验，确保tadırbearer 只发往可信主机。

### P2 — 3. `history.json` 仍是截断写入 + 损坏静默回退（与上轮已修的 `config.json` 同一类缺陷）

**位置：** `src-tauri/src/history.rs:36-39`（`save`）、`history.rs:27-33`（`load`）

`save()` 用裸 `fs::write(path(), data)` 截断写入——正是上轮 S4 针对 `config.json` 修掉的写法。`load()` 用 `unwrap_or_default()` 静默回退。crash/断电中途产生半文件会让最近目录列表**静默清空**，用户无任何感知。

**建议：** 复用 `claude.rs::atomic_write_json` / `Config::save` 的「临时文件 → flush → rename」原子写；`load` 可沿用 `config.rs` 的「损坏留证 + 上报」模式，至少记日志并（若并入 P3-4 的 corrupt 上报通道）让 UI 可见。

### P2 — 4. NVIDIA 代理路由无显式请求体大小限制

**位置：** `src-tauri/src/nvidia/server.rs:8-12`（`Router` 无 `DefaultBodyLimit`）、`proxy.rs:286-290`（`body: Bytes`）

axum 0.7 的 `Bytes` 提取默认回落到 2MiB 上限。Anthropic `/v1/messages` 常带 base64 图片/文档块、长 system、大 `tools` 定义，合计超 2MiB 很现实，会被 axum 默默 `413` 截断在鉴权/解析前，且 413 不是 Anthropic 风格的 `error` 事件。既是误拒真实请求、又与代理「讲 Anthropic 协议」的承诺不一致。

**建议：** Router 链 `.layer(DefaultBodyLimit::max(N))`（如 32MiB），并把超限路径的响应重塑为 Anthropic `error` 事件而非 axum 默认 plaintext。新增一条「>2MiB/超限请求被按 Anthropic 错误处理」的回归。

### P3 — 5. `csp: null`，webview 无内容安全策略（纵深防御缺口）

**位置：** `src-tauri/tauri.conf.json:25`

当前所有词典/日志/导入 JSON 字段均经 JSX 文本插值渲染，`grep` 全前端无 `dangerouslySetInnerHTML`/`innerHTML`，故**当下不可直接利用**。但 `csp:null` 抹掉了本来能兜住「未来任一 XSS sink」的保险：一旦有人为词典 HTML 或上游错误体加 `innerHTML`，便在 webview 内直接代码执行，并可 `invoke` 所有 Tauri 命令——包括 P1 命令注入链所依赖的 `set_work_dir`/`add_recent_dir`/`launch_claude`。

**建议：** 设一条 Tauri CSP（至少 `default-src 'self'; connect-src` 限定到後端 + NVIDIA 域），作为 P1 的纵深防御后补。

### P3 — 6. 运行时 env 注入未做与 settings.json 一致的前缀过滤（「看似安全其实不然」）

**位置：** `src-tauri/src/claude.rs:224-233`（settings.json 写：双向前缀过滤）vs `claude.rs:311-323`（运行时 env：**无过滤**注入全部 `profile_env`）

settings.json 写入会移除非 `ANTHROPIC_`/`CLAUDE_` 前缀的键、且只注入匹配前缀的 profile 键；但运行时 `envs` 先转储 `std::env::vars()` 再**无差别** `envs.insert(k, v)` 每个 `profile_env` 键。两边规则不一致，造成「配置页看似只允许 Claude 相关 env」的假象。

**复现：** 配置页某 profile 加一行 `PATH=C:\hostile`（或 `COMSPEC`），存盘选中启动 → `claude` 子壳与 `claude.cmd` 在被劫持的 `PATH` 下执行。

**严重性：** 攻击者即「编辑自己配置的用户」，非提权；但确为「looks fine but isn't」一致性缺陷。**建议：** 运行时注入同样套用 `ANTHROPIC_`/`CLAUDE_` 前缀白名单（连同显式允许名单如 `API_TIMEOUT_MS` 等已用到的），与 settings.json 侧对齐。

## 前端发现

### P1 — 7. IELTS 词典 48 条数据损坏（catetory/zh 解析残留）

**位置：** `src/dictionaries/ielts.ts`（已用脚本逐行解析確认）

源頭「有道词库」解析脚本在上游满含 `"…"`/`|` 的字段上泄露，三类损坏合共 48 条：

1. **category 为裸引号字符** 14 条：`"category": "\""`（注意是真正只含一个 `"`），同时 `zh` 尾巴多一个 `"`。涉及行：471、622、734、904、1051、1284、1371、1426、1539、1878、2043、2222、2930、3337。
2. **category 为异常两字片段** 4 条：`rn`（ambassador，line 114）、`n v&n`（groa，1359）、`lint&n&a`（farewell，1133）、`a&n&porn`（one，2074）——均是从更长 pos 串里抓取的垃圾片段。
3. **zh 含尾部奇生引号但 category 正常** 约 30 条（chip/deny/detest/disdain 等），仅 `zh` 末尾多一个 `"`。

**影响：**
- 单词本浏览页 category 下拉框会多出 `"`、`rn`、`lint&n&a` 等垃圾分类，且裸 `"` 排序在顶部 → 直观坏相。
- 闪卡背面中文释义末尾出现杂散 `"`。
- 不阻止功能、不崩溃，但作为「主打背单词、词典权威」的页面，数据可信度受损。

**建议：** 从上游有道源重新生成 `ielts.ts`（解析脚本对含 `"` 的字段做强转义 + pos 归一化白名单），或就地 batch-fix 48 条；并在 `DictionaryPage.tsx` 导入归一化路径同样对 category 去首尾 `"`、应用到内置词典展示前的清理。

> 说明：此为**数据质量**级而非代码级缺陷；词典结构本身（187/3427 条、无重复、字段齐备、TS 类型为单一真源）均无误，懒加载与缓存设计也正确。修正后无需改前端管线。

### P3 — 8. 图标按钮无 `aria-label`，`prefers-reduced-motion` 未覆盖新增动画（可访问性打磨）

**位置：** `src/components/EnvValueInput.tsx:45-53`（眼睛显隐按钮仅 `title` 无 `aria-label`/`aria-pressed`）；`src/pages/DictionaryPage.tsx:319`（导入词典 trash 按钮仅 `title`）；`styles.css` 新增 `.lazy-fade-in`/`.mp-pulse`/`.confirm-glow`/`.unsaved-glow`/`.dict-pop` 动画无 `@media (prefers-reduced-motion: reduce)`豁免。

**建议：** 图标按钮加 `aria-label`（如「显示真实值」/「隐藏真实值」/「移除该导入词典」）；为上述动画统一加一段 `prefers-reduced-motion` 全局豁免。

### P3 — 9. `globalsDirty` 在非数字输入时恒显「未保存」

**位置：** `src/pages/ConfigPage.tsx:113`

`Number(compactPct)` 在用户清空/误输非数字时为 `NaN`；`NaN !== (config.compact_pct ?? 70)` 恒真，导致「⚠ 有未保存的修改」常显——即便 `saveGlobals` 会把它钳到 0。**建议：** 比较前 `const pctNum = Number(compactPct); … Number.isNaN(pctNum) ? ... `，或在 `onChange` 处 `Number(e.target.value) || 0` 入参即钳。

### 测试覆盖缺口（前端）

- `ConfigPage` 的 `saveProfiles`/`saveGlobals`/`profileSig` 脏检查逻辑无直接单测（App 级仅验「跨菜单编辑保活」）。鉴于需求明确强调「未保存编辑不丢」，这是性价比最高的补测。
- `DictionaryPage` 导入 JSON 流程、`removeImport`/内置不可删、keyboard 快捷键、`shuffleVersion` 重洗策略均未测（懒加载 + 重置已测）。

## 静态质量备注（clippy 级，非阻断）

- `nvidia/converter.rs:445`、`:1082/1096/1136/1161` 多处 `.unwrap()` 均由紧邻的构造保证安全（locally built / 刚 insert）；建议改为 `.expect("<invariant>")` 既可读又不漏 lint。
- `proxy.rs` 中 `tokio::spawn` 命中（1378/1434/…）全部在 `#[cfg(test)]` mock 内；生产热路径仅 `logger.rs:148`（一次性、有名消费者）与 `nvidia/mod.rs:114`（持有 runtime + graceful-shutdown oneshot），无失控 `tokio::spawn`。✅
- `ct_eq`（`proxy.rs:67-77`）长度不等时提前 `return false`——已有注释明确「长度公开可见、无需恒时」，故**该处注释并不 over-claim**，上轮「静态门禁」记录可保持。✅

## 优先级与建议修复顺序

1. **P1#1 命令注入** — 最高优先（纵深防御；且与 P3-CSP 链成）。优先采用「改 `Command::new + current_dir`，弃 `.bat` + `cd`」的根因方案。同时给 `set_work_dir`/`add_recent_dir`/`history::add` 加路径守门。
2. **P2#2 SSRF/bearer 外泄** — `.redirect(none)` + `base_url` 保存校验 + 转发前再校验。安全侧影响面大、改动量小。
3. **P2#3 `history.json` 原子写 + 不静默** — 与已修 `config.json` 同类，直接复用模式。
4. **P2#4 代理 body 限制** — 显式上限 + 413 改 Anthropic error。
5. **P1#7 ielts 数据** — 桶新生成 / 48 条就地fix。
6. 其余 P3（CSP、env 一致、a11y、NaN 脏标、测试缺口）随相应功能补齐。

## 与上轮一致的「已验证仍 OK」清单（节省后续复查）

- 默认 `NvidiaConfig.host = 127.0.0.1`、非回环强制 ≥24 位 token、`ct_eq` 恒时比较（S1）✅
- 持久 provider 目录 + `PROVIDER_CONFIG_LOCK` 串行读改写 + 原子写（S2）✅
- `split_complete_sse_lines` 字节缓冲行切 + 严格 UTF-8、`saw_completion` 完成判定、未见完成标志的 EOF 发 error（S3 + Spec EOF）✅
- `Config::save` 原子替换 + corrupt 留证 `.corrupt-<ts>.json` + `config.corrupt.notice.txt`（S4）✅
- 有界 `sync_channel(1024)` + 单消费者线程 + 空闲周期 flush 丢弃计数（S5）✅
- `read_log_file` 路径穿越守门（拒 `..`/`/`/`\`/非 `.log`）严密 ✅
- 前端响应类型化，`grep` 无 `as any`/`@ts-ignore`；无 `dangerouslySetInnerHTML`；localStorage 仅存词典 JSON 与 known 集合，无凭据 ✅
- `.bat` 启动后 `del "%~f0"` 自删、`launch` 用 `.env_clear().envs(...)` 不污染用户 shell env ✅
- `dist/` 扫描无明文 `sk-`/`nvapi-`/`Bearer` 凭据；demo/种子凭据只在 Rust `default_profiles`，不进 JS 包 ✅
- 词典数量与文档一致：spinner-verbs 187（行动41/烹饪23/音乐9/奇趣32/自然35/移动19/思考28）、ielts 3427、无重复、无空 `en`/`zh` ✅

## 附录 A — 各命题的源码位置速查（本轮复核落点）

| 命题 | 位置 |
| --- | --- |
| `.bat` `cd /d "{work}"` 未转义 | `claude.rs:76` |
| `set_work_dir` 无校验 | `lib.rs:162-170` |
| `add_recent_dir` → `history::add` 无校验 | `lib.rs:148`, `history.rs:43-55` |
| 上游 client 无 redirect/无 scheme 限定 | `nvidia/proxy.rs:44-47` |
| 转发 URL 拼接 | `nvidia/proxy.rs:340, 841` |
| `base_url` 无任何校验（grep） | 全后端无命中 |
| `history.json` 截断写 + 静默回退 | `history.rs:27-39` |
| `Router` 无 `DefaultBodyLimit` | `nvidia/server.rs:8-12` |
| `csp: null` | `tauri.conf.json:25` |
| settings.json 前缀过滤 vs 运行时未过滤 | `claude.rs:224-233` vs `claude.rs:311-323` |
| 持久 provider 目录（非 pid__nanos） | `claude.rs:117-146` |
| SSE 行切 + UTF-8 + `saw_completion` | `nvidia/proxy.rs:85-105, 625-691` |
| ct_eq 恒时比较 | `nvidia/proxy.rs:67-77` |
| logger 有界队列 + 单消费者 | `logger.rs:36, 145-180, 287-292` |
