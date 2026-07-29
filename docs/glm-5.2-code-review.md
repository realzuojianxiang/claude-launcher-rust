# Code Review — `feat/rust-implementation` vs `master` (704a07f)

**Generated:** 2026-07-27
**Fixed point:** `master` (704a07f, "Initial commit: Claude Launcher v1.0.0")
**Diff:** 4 commits adding the entire `rust/` Tauri v2 + React implementation — `git diff master...HEAD` (three-dot).
**Commits in range:**
- `720378f` feat(rust): 新增 Tauri v2 + React 实现
- `7201ba1` chore(rust): 替换 Tauri 占位图标为仓库真实图标
- `8d322a6` chore: 纳入仓库根目录共享图标资源
- `b9c2e03` feat(rust): 启动页新增历史目录记录功能

> **Note:** `git status` shows uncommitted working-tree changes (`settings.rs` deleted, new `logger.rs` + `nvidia/`, several `.rs` files modified). Those are **not part of the committed diff under review** per this skill's three-dot committed-diff scope.

---

## Standards

### (a) Documented-standard breaches ( HARD )

The `rust/` tree **complies** with the only documented standard — the root `CLAUDE.md` convention ("每种语言实现一个独立子目录"; "不要在仓库根直接构建或运行"). It's a self-contained subdir (own `Cargo.toml`, `package.json`, `src-tauri/`, `src/`), no root-level build/run files leak. **No HARD breaches.**

One cross-file inconsistency worth noting: `src-tauri/Cargo.toml` declares `name = "claude-launcher"` while `package.json` declares `claude-launcher-rust`. The repo standard doesn't require consistent naming across language impls, so this is a smell (below), not a hard breach.

### (b) Baseline smells ( JUDGEMENT-CALL, labelled "possible" )

**possible Duplicated Code — config-read shape.** `config.rs:2254-2259`, `history.rs:2301-2306`, and `settings.rs:2737-2740` all repeat the same idiom:
```rust
match fs::read(&path) {
    Ok(data) => serde_json::from_slice(&data).unwrap_or_default(),
    Err(_) => Self::default(),
}
```
→ one `fn load_json_or_default::<T: DeserializeOwned>(path) -> T`.

**possible Duplicated Code — `~/.claude-launcher` dir resolution.** `config.rs:2245-2250` and `history.rs:2293-2298` both do `dirs::home_dir().expect(...).join(".claude-launcher")` + `create_dir_all`. (`settings.rs` computes a different dir `~/.claude`, less shareable.)

**possible Primitive Obsession.** `Config::work_dir: String`, `anthropic_url: String` — path and URL as bare primitives that each have structure the code keeps re-parsing. `proxy.rs:2601` does `url.trim_end_matches('/')`, `proxy.rs` GETs against `{url}/v1/models`; a `ProxyUrl`/`WorkDir` newtype localizes the `/v1/models` build + validation.

**possible Repeated Switches — status-flavour in three sites.** Backend `proxy.rs:2582-2624` (running/message/error) is mirrored on the frontend `App.tsx:3239-3246` (`status === null ? ❓ : running ? ✅ : ❌`) and the `MessageBanner` emoji-prefix switch `App.tsx:2977-2979` (`startsWith("✅") / "⚠"`). One typed enum mapped to both ends removes all three.

**possible Divergent Change — `proxy.rs`.** Mixes three unrelated reasons to change: exe discovery (`find_proxy_exe`, hardcoded `PROBE_PATHS:2547-2550`), terminal launch via `cmd /c start`, and `/v1/models` reqwest HTTP probing.

**possible Middle Man — Tauri command wrappers in `lib.rs`.** `stop_cliproxyapi`, `get_recent_dirs`, `restore_now`, `get_settings_info`, `cliproxyapi_status` (osmium-call-return in `lib.rs`) — acceptable as Tauri seams but bordering on pure delegation. `add_recent_dir` returns `Vec<String>` from `history::add(...).dirs` that the frontend ignores at `App.tsx:3089` — an unused delegated passthrough.

**possible Shotgun Surgery / God Component — `App.tsx` (539 lines).** One file defines `App`, `MessageBanner`, `DashboardPage`, `LaunchPage`, `ProxyPage`, `ConfigPage`, `AboutPage` plus `MENU`/`Config`/`ProxyStatus` types and a duplicated `yolo` field (`LaunchPage:3049` vs `ConfigPage:3286` — same concept synced only via a Config round-trip). Adding a page or changing the message-banner contract forces edits here → split to `src/pages/*`.

**possible Message Chains — frontend invoke + state threading.** Each page independently invokes `cliproxyapi_status` / `get_recent_dirs` (`DashboardPage:2995`, `ProxyPage:3203`, `LaunchPage:3057`) instead of loading once and pushing. A small `useConfig` hook hides the walk.

**possible Speculative Generality — `PROBE_PATHS` case-pair.** Lists the same absolute path twice, only `d:` vs `D:` differing, to handle Windows case-insensitivity the filesystem already equalizes.

---

## Spec

**N/A — no spec available.** No commits cite issue numbers (`#123`, `!67`), no `docs/` / `specs/` / `.scratch/` directory, and the user opted to skip the Spec axis. `docs/agents/issue-tracker.md` is not present (`/setup-matt-pocock-skills` not run), so there was nothing to fetch a spec against. The Spec sub-agent per the user's choice did not run.

---

## Summary

- **Standards axis:** 1 hard (compliant), 10 judgement-call baseline smells. **Worst issue in this axis:** the Shotgun Surgery / God Component smell in `App.tsx` (539 lines) — it touches 3 other smells (duplicated `yolo` state, the frontend status-switch mirror, the message chain in the frontend) and is the highest-leverage refactor target (split to `src/pages/*`).
- **Spec axis:** 0 / 0 / 0 — no spec available; no findings reported. **Worst issue in this axis:** —.

The axes are kept separate by design: the Spec axis's absence casts no verdict on the Standards axis and vice-versa — no cross-axis ranking occurs since the first axis had no spec to check against.
