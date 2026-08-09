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
