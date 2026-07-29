# 代码审查 — `feat/rust-implementation` 相对 `704a07f`

**审查时间:** 2026-07-28
**固定点 (fixed point):** `704a07f` (Initial commit: Claude Launcher v1.0.0)
**Diff 命令:** `git diff 704a07f...HEAD` —— 整个 `rust/` Tauri v2 + React 实现均为新增，78 个文件 / 约 8400 行插入（纯新增，无删除）。
**范围内提交 (`git log 704a07f..HEAD --oneline`):**
- `720378f` feat(rust): 新增 Tauri v2 + React 实现
- `7201ba1` chore(rust): 替换 Tauri 占位图标为仓库真实图标
- `8d322a6` chore: 纳入仓库根目录共享图标资源
- `b9c2e03` feat(rust): 启动页新增历史目录记录功能

> **说明：** 本次审查的 spec 来源是已弃用的 golang 实现（`../golang-弃用/` 下的 `README.md` + `CLAUDE.md`），即 rust 移植版所参照的「规格」。Standards 来源较薄（仓库内仅有根目录 `CLAUDE.md`），因此 Standards 轴主要由 Fowler 代码坏味基线（_Refactoring_ 第 3 章）承担。两条轴故意分离，互不合并、互不重排。

---

## Standards（规范轴）

### (a) 文档化规范违例 — 硬违例

`rust/` 代码树**符合**仓库内唯一成文的规范（根目录 `CLAUDE.md`：每种语言实现一个自包含子目录；不要在仓库根直接构建或运行；子目录的 `CLAUDE.md` 优先级高于根文件 —— `rust/` 下并无 CLAUDE.md）。它是一个自包含子目录，自带 `Cargo.toml`/`package.json`/`src-tauri/`/`src/`，无根级构建/运行文件泄漏。**无硬违例。**

（git status 中 `settings.rs` 显示为删除，仅是固定点之后的工作区改动，不属于 `704a07f...HEAD` 这段已提交 diff；`settings.rs` 在该 diff 中是完整新增的。）

### (b) 坏味基线 — 判断项（均标注「可能」）

- **可能 Duplicated Code（重复代码）** —— `proxy.rs:8-13` 与 `claude.rs:11-18` 重复了「先探测硬编码绝对路径，再用 `which` 回退 `.exe` / 裸名」的结构。→ 抽出公共 helper `find_exe(probes: &[&str], bare: &str)`。
- **可能 Duplicated Code** —— `config.rs:38-45` 与 `history.rs:26-31` 完全重复了 `home_dir()?; let _ = create_dir_all(home.join(".claude-launcher")); return path` 这句式，仅末尾文件名不同。→ 一个 `app_dir()` helper 即可去重。
- **可能 Shotgun Surgery（霰弹式修改）** —— 历史目录功能 (`b9c2e03`) 同时动了 `history.rs`（新增）+ `lib.rs`（新增命令 `get_recent_dirs`/`add_recent_dir` + `invoke_handler!` 注册）+ `App.tsx`（`loadRecent`/`useDir`/状态扩展）。截断上限这一概念被拆在两个文件里：`MAX_ENTRIES=200`（`history.rs:8`）与显示上限 `RECENT_VISIBLE=3`（`App.tsx:191`）—— 一个概念两处常量。→ 把两处上限都收拢到历史模块附近。
- **可能 Primitive Obsession（基本类型偏执）** —— `anthropic_url: String` 反复被就地归一化：`proxy.rs:65` 做 `url.trim_end_matches('/')`，`claude.rs` 又原样拼进批处理字符串。→ 引入 `BaseUrl` 新类型并提供 `join(path)`，集中处理斜杠裁剪，避免每个调用点各自推导约定。
- **可能 Middle Man（中间人，权重低）** —— `lib.rs:107-117` 的 `add_recent_dir`/`get_recent_dirs` 只是 `history::add`/`history::list` 的透传，单纯再包成 `Vec<String>`；`restore_now`/`stop_cliproxyapi` 类似。Tauri 命令无法直接转发，故多为框架所迫，仅边际可议。
- **可能 Speculative Generality / 可移植性** —— 开发机路径 `d:\BaiduSyncdisk\ai-agent\CLIProxyAPI\cliproxyapi.exe` 被烤进发行二进制（`proxy.rs:8-11`，大小写两分支）。→ 依赖 PATH/配置，删掉这对硬编码探测。
- **可能 Mysterious Name（晦涩命名）** —— `lib.rs:118` `get_system_info` 返回 `SystemInfo { os: "windows", arch: "amd64", version: "1.0.0" }`。全是字面量，几乎算不上探测；命名为 `get_app_info`/`AppInfo` 更诚实。
- **可能 Refused Bequest（拒绝继承）** —— `Cargo.toml` 将 lib crate 声明为 `["staticlib","cdylib","rlib"]`，并在 `run()` 上挂 `#[cfg_attr(mobile, tauri::mobile_entry_point)]`（`lib.rs:147`），但本应用是仅 Windows 的 `cmd`/`taskkill` 胶水代码，全无移动端目标。`staticlib` 与移动入口点均未被实际触达。→ 删除未用的继承/入口。

---

## Spec（规格轴）

### (a) 缺失 / 不完整 —— 规格要求却未实现

1. **Claude settings.json 改写完全缺失。** 规格（golang CLAUDE.md）：启动时须 `backupSettings()` 把原文件备份到 `~/.claude/settings.json.launcher_backup`，再 `modifySettings()` 注入 `ANTHROPIC_BASE_URL`/`ANTHROPIC_API_KEY` + `apiProvider=anthropic`，并删除冲突项 `ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_MODEL`/`ANTHROPIC_SMALL_FAST_MODEL`。rust 版有意不做此事 —— `claude.rs:1-4` 注明「不再改写 ~/.claude/settings.json」，改为以进程环境变量注入 `profile_env`（`claude.rs:76-210`）。无备份、无修改、根本不碰 settings.json。对核心的「可逆改写」要求是整体性偏离，而非局部缺口。
2. **`restoreSettings()` 及全部三处触发点均消失。** 规格：`restoreSettings()` 须在 启动失败回滚、应用退出 `beforeClose`、`RestoreNow` 手动按钮 三处触发。grep `src-tauri/src` 找不到 `beforeClose`、`RunEvent`、`restoreSettings`、`RestoreNow`。`lib.rs` 也无 `on_window_event`/退出 hook —— 唯一的 `.run()` 是裸的 `tauri::Builder::default().run(...)`（`lib.rs:463`）。GUI 关闭时不做任何还原。
3. **临时批处理末尾的还原调用缺失。** 规格：启动批处理须以 `call claude_restore_settings.bat` 收尾。rust `claude.rs:177-183` 构造的批处理仅以 `pause` 结尾，无还原调用。
4. **硬编码探测路径被删除。** 规格：先探测 `d:\BaiduSyncdisk\...\cliproxyapi.exe` 再回退 PATH。`proxy.rs:37-83` 的 `locate_proxy` 改为 用户指定目录 → exe 同目录 → PATH；BaiduSyncdisk 硬编码探测被移除（由配置项 `cliproxyapi_dir` 取代）。
5. **`apiProvider=anthropic` 注入缺失** —— 因 settings.json 未被触碰，该字段从未被设置。

### (b) 范围蔓延（scope creep）—— golang 规格未要求

- **历史目录功能（`b9c2e03`）** —— 整个 `history.rs`（`history.json`、增删查、`MAX_ENTRIES=200`），`get_recent_dirs`/`add_recent_dir`/`remove_recent_dir`/`set_work_dir`（`lib.rs:138-167`），LaunchPage「最近打开」UI（`App.tsx:579-613`）。规格从未提及目录历史，属新增功能。
- **NVIDIA NIM 代理**（`lib.rs:251-329`、`nvidia/` 模块、axum server、Key 池、chat 测试）—— 一个完整的 Anthropic→OpenAI 协议转换代理。规格无此要求。
- **Profiles / 多 provider**（`config.rs:13-185`，默认种子 讯飞 + CherryStudio·GLM）—— 规格只有一个 CLIProxyAPI provider；移植版加了多 provider 系统。
- **应用日志子系统**（`logger.rs` 约 378 行、LogPage UI）—— 未被要求。
- **auto-compact 旋钮**（`compact_window`/`compact_pct`，`config.rs:99-102`、`claude.rs:158-169`）—— 无规格依据。
- **`CLAUDE_CONFIG_DIR` 隔离**（`claude.rs:117-157`）—— 写入独立的 `claude-isolated/settings.json` 以绕开全局 settings，是取代规格「备份/还原」机制的新增机制。

### (c) 实现有误（看似实现、实则不对）

golang 规格的核心契约 —— 「settings.json 改写（关键且可逆）」，即在三处触发点上备份/还原 —— 被替换为环境变量注入设计（`claude.rs:1-4`）。移植版内部自洽，但它实现的���**另一套**契约：README 的「退出时自动还原原始配置」并未被满足 —— 因为从未修改过任何东西，也就无从还原。`CLAUDE_CONFIG_DIR` 隔离是移植版所选的替代方案。

关键文件：`src-tauri/src/claude.rs`、`config.rs`、`proxy.rs`、`history.rs`、`lib.rs`。

---

## 总结（按轴汇总，不做跨轴重排）

- **Standards 轴：** 约 8 个判断项坏味，**0 个硬违例。** 最严重：历史功能里的 **「霰弹式拆分常量」**（`MAX_ENTRIES=200` 与 `RECENT_VISIBLE=3` 一个概念落在两个文件）；以及 `proxy.rs`+`claude.rs` 之间 **Duplicated Code 的可执行文件探测** 逻辑。
- **Spec 轴：** 5 项缺失/不完整 + 6 项范围蔓延 + 1 项实现有误。最严重：**核心的「可逆 settings 改写」契约完全未实现** —— 移植版有意改用环境变量注入（`claude.rs:1-4`「不再改写...」），导致 `modifySettings()` 与全部三处 `restoreSettings()` 触发点悉数缺失，README 的「退出时自动还原」落空。

最值得提请注意的一点：这是一次 **「Standards 通过、Spec 偏离」** 的改动。代码本身按坏味基线还算干净，但它实现的是一套与 golang 规格不同的启动机制 —— 二选一：要么更新规格（环境变量注入 + `CLAUDE_CONFIG_DIR` 隔离很可能是一次有意的、更安全的设计重构）；要么把「备份 / 修改 / 三处触发还原」的契约补回来。
