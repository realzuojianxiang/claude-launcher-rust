# NVIDIA Stream Stall Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or execute inline task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raise the NVIDIA response timeout default to 600 seconds and fall back to the next configured model when a stream produces no meaningful response within that window, while reusing the same NVIDIA API Key.

**Architecture:** Treat “response headers received” separately from “meaningful stream output received.” Before returning the SSE response to Claude Code, buffer upstream chunks until substantive content, a tool call, or a completion marker arrives; if that pre-output phase times out, retry the next model with the same key. Once substantive content has been exposed, retain the existing honest-error behavior on later stalls because stitching a second model into a partial answer is unsafe.

**Tech Stack:** Rust 2021, Tokio, reqwest streaming, Axum SSE, React 18, TypeScript.

## Global Constraints

- Default and legacy-default `request_timeout_seconds = 120` migrate to `600`.
- User-selected timeout values other than the legacy default remain configurable.
- A pre-output stream timeout advances the model fallback chain without rotating the API Key.
- A stream that has emitted substantive content must never be continued with a different model.
- No database objects are modified.

---

### Task 1: Upgrade the timeout default and legacy value

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src/pages/NvidiaPage.tsx`
- Modify: `src/test/fixtures.ts`

**Interfaces:**
- `NvidiaConfig::default().request_timeout_seconds == 600`
- `Config::load_or_default()` upgrades the legacy default value `120` to `600`.

- [x] **Step 1: Add failing Rust tests**

Add tests asserting the new default is `600` and a loaded configuration containing the legacy default `120` is normalized to `600`, while an explicit non-legacy value is preserved.

- [x] **Step 2: Run focused tests and verify RED**

Run: `cargo test config::tests::nvidia_timeout`

Expected: assertions receive `120`.

- [x] **Step 3: Implement the default and migration**

Change `default_request_timeout()` to `600` and normalize only the legacy value `120` during config loading. Change frontend empty/fallback values and fixtures to `600`.

- [x] **Step 4: Verify GREEN**

Run: `cargo test config::tests::nvidia_timeout`

Expected: all focused tests pass.

### Task 2: Fall back models before meaningful output

**Files:**
- Modify: `src-tauri/src/nvidia/proxy.rs`
- Test: `src-tauri/src/nvidia/proxy.rs`

**Interfaces:**
- `wait_for_meaningful_stream_start` buffers upstream chunks until meaningful SSE output or completion.
- A timeout outcome is handled inside the outer request loop by advancing `model_idx` and retaining the selected key for the next attempt.
- `stream_response` consumes the buffered prefix followed by the remaining upstream stream.

- [x] **Step 1: Add a failing integration test**

Start a local Axum mock upstream with two models and two keys. The first model returns SSE headers plus a non-substantive role chunk and then stalls; the second model completes normally. Assert that the proxy returns the second model’s output and both upstream calls carry the same bearer token.

- [x] **Step 2: Run the focused test and verify RED**

Run: `cargo test stream_stall_fallback_tests::pre_output_stall_falls_back_model_with_same_key -- --nocapture`

Expected: test-level timeout because the current implementation exposes the first stream immediately and waits for the 30-second idle guard.

- [x] **Step 3: Implement minimal buffered preflight and sticky-key fallback**

Buffer the pre-output SSE prefix under `request_timeout_seconds`. On timeout/EOF before meaningful output, retry the next model with the same key. Return the buffered prefix and remaining stream only after meaningful output or valid completion is observed.

- [x] **Step 4: Verify GREEN and protect the partial-output boundary**

Run the focused integration test, then add/execute a test showing content-bearing streams are returned normally and are not replaced by another model.

### Task 3: Document and verify

**Files:**
- Modify: `CODE_REVIEW_2026-07-28.md`
- Modify: this plan

- [x] **Step 1: Record the new stream-stall behavior and limitation**

Document that pre-output stalls fall back by model with a sticky key, while post-output stalls remain errors to avoid mixed-model responses.

- [x] **Step 2: Run complete verification**

Run:

```text
npm test
npm run build
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
git diff --check
```

- [x] **Step 3: Request independent code review and commit**

Review the complete diff, fix Critical/Important findings, then commit the verified change.

## Self-Review

- Spec coverage: 600-second default/migration, same-key model fallback, and the partial-output safety boundary are explicit.
- Placeholder scan: no deferred implementation placeholders.
- Type consistency: preflight owns only buffered bytes and a borrowed stream; the existing converter remains the single protocol conversion path.
