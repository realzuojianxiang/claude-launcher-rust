# Task 2 Report — Model Usage Statistics Tauri Wiring

- status: DONE_WITH_CONCERNS

## Files changed

- `rust/src-tauri/src/lib.rs`
- `rust/src-tauri/src/stats.rs`
- `rust/src-tauri/src/nvidia/mod.rs`
- `rust/src-tauri/src/grok/mod.rs`

## Exact tests/checks run and results

1. Red-phase focused command from the brief
   - Command:
     - `cargo test --lib parse_usage_range`
   - Working directory:
     - `rust/src-tauri`
   - Result before implementation:
     - Failed as expected at compile time because `parse_usage_range` and `UsageStatsState::in_memory` did not exist.

2. Brief Step 4 command as written
   - Command:
     - `cargo test --lib parse_usage_range stats::tests`
   - Working directory:
     - `rust/src-tauri`
   - Result:
     - Cargo rejected the command because it is not valid syntax:
       - `error: unexpected argument 'stats::tests' found`
     - I therefore ran the two equivalent focused test targets separately below.

3. Fresh focused parser test on the final Task 2 code state
   - Command:
     - `cargo test --lib parse_usage_range`
   - Working directory:
     - `rust/src-tauri`
   - Result:
     - Rust compiled successfully.
     - The unit test executable then crashed before test bodies ran:
       - `exit code: 0xc0000139`
       - `STATUS_ENTRYPOINT_NOT_FOUND`

4. Fresh focused stats test block on the final Task 2 code state
   - Command:
     - `cargo test --lib stats::tests`
   - Working directory:
     - `rust/src-tauri`
   - Result:
     - Rust compiled successfully.
     - The same unit test executable crash occurred before tests ran:
       - `exit code: 0xc0000139`
       - `STATUS_ENTRYPOINT_NOT_FOUND`

5. Compile verification on the final Task 2 code state
   - Command:
     - `cargo check --tests`
   - Working directory:
     - `rust/src-tauri`
   - Result:
     - Succeeded.
     - No type errors remained from Tauri state registration or the provider `start(..., stats)` signature changes.
     - Existing warnings remain because the Task 1/Task 2 statistics store is only partially wired until Task 3 starts recording provider usage.

## Self-review findings

- Added `parse_usage_range(&str) -> UsageRange` with command-oriented defaults:
  - `live` → `UsageRange::Live`
  - `7d` → `UsageRange::Days7`
  - `30d` → `UsageRange::Days30`
  - `all` → `UsageRange::All`
  - unknown values default to `UsageRange::Days7`
- Added a focused shared-state test proving multiple cloned handles from `UsageStatsState` point at the same underlying store.
- Registered one app-owned `UsageStatsState` in Tauri and exposed `get_usage_stats`.
- Wired `nvidia_start` and `grok_start` to receive the shared store from Tauri state.
- Updated both provider lifecycle states to accept and retain the shared store for later proxy-level recording work in Task 3.
- Kept the diagnostic startup branches compiling by creating an in-memory stats store there.

## Concerns

- The existing Windows Rust test-runtime blocker remains: library test executables compile but crash before running test bodies with `STATUS_ENTRYPOINT_NOT_FOUND` (`0xc0000139`).
- The exact combined test command written in the brief is not valid `cargo test` syntax, so equivalent separate focused commands were used instead.

---

## Fix report append — review wiring follow-up (2026-08-09)

- status: DONE_WITH_CONCERNS

### Review item addressed

1. The shared usage-stats store is now carried by both proxy contexts instead of being parked only in provider lifecycle state.
   - `nvidia::proxy::ProxyCtx` now has `pub stats: Arc<UsageStatsStore>` and `ProxyCtx::new(cfg, stats)`.
   - `grok::proxy::ProxyCtx` now has `pub stats: Arc<UsageStatsStore>` and `ProxyCtx::new(cfg, auth_provider, stats)`.
   - `NvidiaState::start` and `GrokState::start` now pass the same shared store into the corresponding proxy context.
   - The temporary `Running._stats` parking fields were removed because the proxy now owns the shared store reference directly.
   - Added focused proxy-constructor tests asserting the proxy keeps the exact shared `Arc`.

### Files changed for the fix

- `rust/src-tauri/src/nvidia/mod.rs`
- `rust/src-tauri/src/grok/mod.rs`
- `rust/src-tauri/src/nvidia/proxy.rs`
- `rust/src-tauri/src/grok/proxy.rs`

### Exact commands run and results for the fix

1. Red-phase compile check after adding the focused proxy-store assertion and before finishing the wiring
   - Command:
     - `cargo check --tests`
   - Working directory:
     - `rust/src-tauri`
   - Result:
     - Failed as expected because provider lifecycle and proxy constructors no longer matched yet.
     - Representative errors:
       - `src/nvidia/mod.rs`: `ProxyCtx::new(cfg)` missing `Arc<UsageStatsStore>`
       - `src/grok/mod.rs`: `ProxyCtx::new(cfg, auth_provider)` missing `Arc<UsageStatsStore>`

2. Final compile verification on the completed fix
   - Command:
     - `cargo check --tests`
   - Working directory:
     - `rust/src-tauri`
   - Result:
     - Succeeded.
     - Existing warnings remain from the partially wired stats layer not being consumed for recording until Task 3.

3. Final focused parser test on the completed fix
   - Command:
     - `cargo test --lib parse_usage_range`
   - Working directory:
     - `rust/src-tauri`
   - Result:
     - Rust compiled successfully.
     - The unit test executable then crashed before test bodies ran:
       - `exit code: 0xc0000139`
       - `STATUS_ENTRYPOINT_NOT_FOUND`

### Concerns for the fix

- The same Windows Rust test-runtime issue remains unchanged: the focused library test target compiles but the produced test executable crashes before running tests.
- `cargo check --tests` is therefore the strongest verification evidence available in this environment for this wiring-only fix round.
