# Post-Review Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or execute inline task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove sensitive per-launch artifacts after Claude exits, record the remediation status, establish frontend regression tests, and split the oversized NVIDIA page into focused components.

**Architecture:** Generate the Windows launch batch through a pure builder that can be regression-tested, and make the batch own normal-exit cleanup of its unique `CLAUDE_CONFIG_DIR` and itself. Add Vitest + Testing Library as the frontend test boundary, extract provider environment construction as a pure function, then decompose `NvidiaPage` into presentational/interaction sections while keeping orchestration state in the page.

**Tech Stack:** Rust 2021, Windows batch, Tauri 2, React 18, TypeScript 5.6, Vite 8, Vitest, Testing Library, jsdom.

## Global Constraints

- Preserve existing Tauri command names and user-visible behavior.
- Do not modify database objects.
- Do not persist provider secrets outside the existing application config and a live launch instance.
- Do not delete an isolation directory until the associated Claude process has exited.
- Keep `NvidiaPage` as the owner of backend orchestration and shared editable state; extracted components receive typed props.
- Every behavior change follows RED → GREEN; structural moves must keep frontend tests and production build green.

---

### Task 1: Clean sensitive launch artifacts on normal exit

**Files:**
- Modify: `src-tauri/src/claude.rs`
- Test: `src-tauri/src/claude.rs`

**Interfaces:**
- Produces: `build_launch_batch(work: &str, claude_cmd: &str, yolo: bool) -> String`
- `launch` writes the returned batch and starts it via `cmd /c`.

- [x] **Step 1: Write a failing batch lifecycle test**

```rust
#[test]
fn launch_batch_removes_isolated_config_and_itself_after_claude_exits() {
    let batch = build_launch_batch("D:\\work", "claude.cmd", false);
    let claude = batch.find("claude.cmd").unwrap();
    let remove_config = batch
        .find("rmdir /s /q \"%CLAUDE_CONFIG_DIR%\"")
        .unwrap();
    let remove_script = batch.find("del \"%~f0\"").unwrap();

    assert!(claude < remove_config);
    assert!(remove_config < remove_script);
}
```

This catches removal of either cleanup command or cleanup occurring before Claude exits.

- [x] **Step 2: Run focused test and verify RED**

Run: `cargo test --lib launch_batch_removes_isolated_config_and_itself_after_claude_exits`

Expected: compile failure because `build_launch_batch` does not exist.

- [x] **Step 3: Implement the minimal batch builder**

The generated batch runs Claude, removes `%CLAUDE_CONFIG_DIR%` only after Claude returns, pauses for output inspection, then deletes `%~f0`. Change the child shell from `/k` to `/c` so the terminal process and its secret-bearing environment end after the batch finishes.

- [x] **Step 4: Verify GREEN and all Rust checks**

Run:

```text
cargo test --lib launch_batch_removes_isolated_config_and_itself_after_claude_exits
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: all commands exit 0.

### Task 2: Update the review report

**Files:**
- Modify: `CODE_REVIEW_2026-07-28.md`

- [x] **Step 1: Add a remediation status section**

Record commits `603118c` and `65b2636`, map S1–S5 and Spec findings to their regression tests, and state that the original findings describe the pre-remediation baseline.

- [x] **Step 2: Record launch-artifact cleanup**

Document normal-exit deletion of the unique isolation directory and temporary batch; explicitly note that forced OS termination can still leave artifacts for a future retention-policy task.

- [x] **Step 3: Commit the security/report task**

```text
git add src-tauri/src/claude.rs CODE_REVIEW_2026-07-28.md docs/superpowers/plans/2026-07-28-post-review-hardening.md
git commit -m "fix(rust): clean sensitive launch artifacts"
```

### Task 3: Establish frontend regression testing

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `vite.config.ts`
- Create: `src/test/setup.ts`
- Create: `src/test/fixtures.ts`
- Create: `src/providerEnv.test.ts`
- Create: `src/App.test.tsx`
- Create: `src/providerEnv.ts`
- Modify: `src/pages/LaunchPage.tsx`

**Interfaces:**
- Produces: `buildProviderEnv(config: Config, profileName: string) -> Record<string, string>`

- [x] **Step 1: Install the test runtime**

Run:

```text
npm install --save-dev vitest jsdom @testing-library/react @testing-library/jest-dom
```

Add scripts:

```json
"test": "vitest run",
"test:watch": "vitest"
```

Configure jsdom and `src/test/setup.ts`.

- [x] **Step 2: Write provider-environment tests and verify RED**

Tests use literal expected maps for both a named profile and the built-in NVIDIA provider. Run `npm test -- src/providerEnv.test.ts`; expect module-not-found for `providerEnv`.

- [x] **Step 3: Extract the minimal pure provider builder and verify GREEN**

Move the existing environment-map branches from `LaunchPage.launch` into `buildProviderEnv`, keeping the exact NVIDIA defaults and profile behavior.

- [x] **Step 4: Add App behavior coverage**

Mock only the Tauri `invoke` boundary with a complete `Config` fixture. Verify menu navigation renders the selected page and an unsaved profile-name edit survives navigation away from and back to Config.

- [x] **Step 5: Run frontend tests and build**

Run:

```text
npm test
npm run build
```

Expected: all tests and production build pass.

### Task 4: Split the NVIDIA page under test protection

**Files:**
- Modify: `src/pages/NvidiaPage.tsx`
- Create: `src/pages/nvidia/NvidiaStatusCard.tsx`
- Create: `src/pages/nvidia/KeyPoolCard.tsx`
- Create: `src/pages/nvidia/NvidiaTestPanel.tsx`
- Create: `src/pages/nvidia/ModelPriorityEditor.tsx`
- Create: `src/pages/nvidia/NvidiaConfigForm.tsx`

**Interfaces:**
- `NvidiaStatusCard`: status text, endpoint, busy flags, test result, and action callbacks.
- `KeyPoolCard`: key-pool snapshot, expanded state, cooldown, and toggle callback.
- `NvidiaTestPanel`: port, first model, and message callback; owns shell selection/copy formatting.
- `ModelPriorityEditor`: models, running state, per-model test state, reorder/add/remove/test callbacks.
- `NvidiaConfigForm`: editable connection/key values and save callback.

- [x] **Step 1: Move one JSX section at a time**

After each extraction, run `npm test` and `npm run build`; keep `NvidiaPage` orchestration and Tauri calls unchanged.

- [x] **Step 2: Remove imports/state moved into child components**

Require TypeScript `noUnusedLocals` to remain clean.

- [x] **Step 3: Run complete verification**

```text
npm test
npm run build
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
git diff --check
```

- [x] **Step 4: Commit frontend tests and decomposition**

```text
git add package.json package-lock.json vite.config.ts src
git commit -m "test(frontend): cover navigation and split NVIDIA page"
```

## Self-Review

- Spec coverage: sensitive artifact cleanup, report status, frontend behavior coverage, and NVIDIA decomposition all have explicit tasks.
- Placeholder scan: no deferred implementation placeholders.
- Type consistency: the provider builder consumes the existing `Config`; NVIDIA child component contracts keep orchestration in `NvidiaPage`.
