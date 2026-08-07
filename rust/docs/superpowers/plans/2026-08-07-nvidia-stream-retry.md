# NVIDIA stream retry hybrid Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task with verification checkpoints.

**Goal:** Make short NVIDIA streaming responses retryable when reqwest fails while decoding the upstream response body, without concatenating a second response after downstream streaming has started.

**Architecture:** Extend the existing `wait_for_meaningful_stream_start` prefetch boundary in `src-tauri/src/nvidia/proxy.rs`. It will buffer a short stream until completion, an error, a 256 KiB handoff limit, or a one-second post-first-output streaming handoff window. Prefetch errors remain inside `handle_messages` and use its existing retry loop; after handoff, `stream_response` retains the current Anthropic error-event behavior.

**Tech Stack:** Rust 2021, Tokio, reqwest 0.12, Axum 0.7, `async_stream`, existing `cargo test --lib` proxy tests.

## Global Constraints

- Preserve the complete `AnthropicRequest` on every retry; do not mutate or truncate messages, tools, system blocks, or token limits.
- Keep `Accept-Encoding: identity` on NVIDIA requests.
- Do not retry after downstream content has been handed to Claude Code.
- Do not add dependencies or expose API keys in logs.
- Use test-first development: the new regression test must fail before production code changes.

---

### Task 1: Add the failing body-decoding regression test

**Files:**
- Modify: `src-tauri/src/nvidia/proxy.rs` in `stream_stall_fallback_tests`

**Interfaces:**
- Reuse `handle_messages`, `build_request`, `SeenRequests`, and the existing local Axum upstream test harness.
- Add a mock upstream handler that emits one meaningful SSE chunk followed by a stream error on the first request, then emits a completed SSE response on the next request.

- [ ] **Step 1: Add `mock_stream_error_then_success`**

Use the request count to select the response. The first response must produce a meaningful OpenAI SSE delta and then `Err(std::io::Error)`. Later responses must produce:

```text
data: {"choices":[{"delta":{"content":"retry-ok"},"finish_reason":null}]}

data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

- [ ] **Step 2: Add `body_decoding_error_before_handoff_retries_the_request`**

Configure two keys, one model, `request_timeout_seconds: 1`, and `max_retries: 3`. Call `handle_messages`, collect the downstream body, and assert:

```rust
assert!(body.contains("retry-ok"));
assert_eq!(seen.lock().await.len(), 2);
```

The test describes the required behavior: a body error before the hybrid handoff must not be visible as the final response.

### Task 2: Verify the regression test fails before the fix

**Files:**
- None

- [ ] **Step 1: Run the focused test**

Run from `src-tauri`:

```bash
cargo test --lib nvidia::proxy::stream_stall_fallback_tests::body_decoding_error_before_handoff_retries_the_request -- --nocapture
```

Expected result before the production change: `FAIL`, with only one upstream request and no `retry-ok` content because the current code returns the response immediately after the first meaningful chunk.

### Task 3: Implement hybrid prefetch and handoff

**Files:**
- Modify: `src-tauri/src/nvidia/proxy.rs` around `StreamStart`, `StreamStartDetector`, and `wait_for_meaningful_stream_start`

**Interfaces:**
- Keep `handle_messages` and `stream_response` call shapes stable.
- Add a private `MAX_RETRYABLE_STREAM_BUFFER_BYTES: usize = 256 * 1024` constant.
- Add a private `RETRYABLE_STREAM_WINDOW: Duration = Duration::from_secs(1)` constant.

- [ ] **Step 1: Track completion separately from meaningful output**

Extend `StreamStartDetector` with a completion flag. Set it for `[DONE]` and non-empty `finish_reason`; leave text, reasoning, and valid tool starts as meaningful output only. Add a private accessor used by the prefetch loop.

- [ ] **Step 2: Change `wait_for_meaningful_stream_start` into a bounded prefetch**

Preserve the existing first-output timeout and the 1 MiB no-meaningful-output safety limit. After the first meaningful output, start a one-second handoff deadline. During that window:

```rust
if upstream_body_error_before_handoff {
    return StreamStart::Failed(error.to_string());
}
if incomplete_eof_before_completion {
    return StreamStart::Failed("...".to_string());
}
if completion_marker_seen {
    return StreamStart::Ready(buffered_chunks);
}
if buffered_bytes >= MAX_RETRYABLE_STREAM_BUFFER_BYTES {
    return StreamStart::Ready(buffered_chunks);
}
if handoff_deadline_elapsed {
    return StreamStart::Ready(buffered_chunks);
}
```

Use a first-output deadline before meaningful output and the post-output handoff deadline afterward; do not apply the first-output timeout to the entire generation. A stream ending without meaningful output remains `Ended`, `Idle`, or `BufferLimitExceeded` as before so existing model fallback behavior is preserved.

- [ ] **Step 3: Keep the existing retry and post-handoff behavior**

The existing `StreamStart::Failed` branch in `handle_messages` should remain the retry path. `StreamStart::Ready` continues to call `stream_response` with buffered chunks and the still-live upstream stream. Do not add retry logic inside `stream_response`.

### Task 4: Lock the no-stitching boundary

**Files:**
- Modify: `src-tauri/src/nvidia/proxy.rs` in `stream_stall_fallback_tests`

- [ ] **Step 1: Make the partial-output fixture cross the handoff window**

Update `mock_partial_then_eof` to delay long enough after its meaningful chunk to force the hybrid prefetch to hand off before EOF. Use `RETRYABLE_STREAM_WINDOW + Duration::from_millis(50)` so the test remains tied to the implementation boundary without a long sleep.

- [ ] **Step 2: Preserve the safety assertions**

Keep assertions that the body contains the partial text and `event: error`, and that exactly one upstream request was made. This proves the proxy never appends a second model response after downstream output begins.

### Task 5: Run focused and full verification

**Files:**
- None

- [ ] **Step 1: Run the focused regression test and existing proxy tests**

```bash
cargo test --lib nvidia::proxy::stream_stall_fallback_tests -- --nocapture
```

Expected result: all tests in the module pass, including the new retry test and the no-stitching test.

- [ ] **Step 2: Run Rust formatting and compile checks**

```bash
cargo fmt --all -- --check
cargo check --tests
```

Expected result: no formatting changes required and no Rust compile errors.

- [ ] **Step 3: Run the complete Rust unit suite**

```bash
cargo test --lib
```

Expected result: all library tests pass. If Windows DLL loading prevents test execution, record the exact environment failure separately from compile/check results.

- [ ] **Step 4: Review the final diff**

```bash
git diff --check
git diff -- src-tauri/src/nvidia/proxy.rs
```

Confirm that only the intended prefetch boundary, tests, and plan/spec documentation changed.

### Task 6: Commit the implementation

**Files:**
- Modify: `src-tauri/src/nvidia/proxy.rs`
- Modify: `docs/superpowers/plans/2026-08-07-nvidia-stream-retry.md`

- [ ] **Step 1: Create the implementation commit**

```bash
git add src-tauri/src/nvidia/proxy.rs docs/superpowers/plans/2026-08-07-nvidia-stream-retry.md
git commit -m "fix(nvidia): retry short stream body failures"
```

