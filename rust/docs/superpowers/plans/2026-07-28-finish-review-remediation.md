# Finish Review Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or execute inline task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish extracting the four remaining business pages from `App.tsx` and add a real two-provider isolation regression test.

**Architecture:** Keep `App.tsx` as the application shell and owner of cross-page state; each page remains responsible for its existing Tauri commands and local UI state. Move isolated-config preparation behind a filesystem-oriented helper so the launch path and a real temporary-directory test exercise the same behavior.

**Tech Stack:** React 18, TypeScript 5.6, Tauri 2, Rust 2021, built-in Rust test framework.

## Global Constraints

- Preserve all existing user-visible behavior and Tauri command names.
- Preserve the already lifted `nvTest`, provider-edit, and global-config state in `App`.
- Do not add frontend or Rust dependencies.
- Run edits from the `rust/` implementation directory.
- Do not modify database objects.

---

### Task 1: Extract the remaining business pages

**Files:**
- Modify: `src/App.tsx`
- Create/finish: `src/pages/LaunchPage.tsx`
- Create/finish: `src/pages/ProxyPage.tsx`
- Create/finish: `src/pages/ConfigPage.tsx`
- Create: `src/pages/NvidiaPage.tsx`

**Interfaces:**
- `LaunchPage({ config, onConfig })`
- `ProxyPage({ config, onConfig })`
- `ConfigPage({ config, onConfig, profiles, setProfiles, globals, setGlobals })`
- `NvidiaPage({ config, onConfig, test, onTest })`

- [ ] **Step 1: Wire the three existing extracted files into `App.tsx`**

```ts
import { LaunchPage } from "./pages/LaunchPage";
import { ProxyPage } from "./pages/ProxyPage";
import { ConfigPage } from "./pages/ConfigPage";
```

- [ ] **Step 2: Run the TypeScript build and verify RED**

Run: `npm run build`

Expected: FAIL because imported page names still collide with the inline declarations.

- [ ] **Step 3: Remove the three corresponding inline page implementations and their now-unused imports**

Keep the shell render calls and shared state unchanged.

- [ ] **Step 4: Run the build and verify GREEN**

Run: `npm run build`

Expected: PASS.

- [ ] **Step 5: Start the NVIDIA extraction with a missing import and verify RED**

```ts
import { NvidiaPage } from "./pages/NvidiaPage";
```

Run: `npm run build`

Expected: FAIL because `src/pages/NvidiaPage.tsx` does not exist and the inline name collides.

- [ ] **Step 6: Move the inline NVIDIA page unchanged**

`src/pages/NvidiaPage.tsx` imports its own React hooks, `invoke`, shared types, `MessageBanner`, and `ConfirmButton`, then exports:

```ts
export function NvidiaPage({
  config,
  onConfig,
  test,
  onTest,
}: NvidiaPageProps) {
  // existing body unchanged
}
```

Delete the inline implementation and remove unused imports from `App.tsx`.

- [ ] **Step 7: Run the build and verify GREEN**

Run: `npm run build`

Expected: PASS with `App.tsx` containing only the application shell and cross-page state.

### Task 2: Test real isolation for two providers

**Files:**
- Modify: `src-tauri/src/claude.rs`

**Interfaces:**
- Produces: `prepare_isolated_config_in(base_dir: &Path, profile_env: &HashMap<String, String>) -> Result<PathBuf, String>`
- `launch` calls the same helper with `Config::config_dir()`.

- [ ] **Step 1: Write the failing behavior test**

```rust
#[test]
fn two_providers_write_independent_settings() {
    let root = unique_test_dir();
    let first = provider_env("http://provider-one.test", "token-one");
    let second = provider_env("http://provider-two.test", "token-two");

    let first_dir = prepare_isolated_config_in(&root, &first).unwrap();
    let second_dir = prepare_isolated_config_in(&root, &second).unwrap();

    assert_ne!(first_dir, second_dir);
    assert_eq!(read_env(&first_dir)["ANTHROPIC_BASE_URL"], "http://provider-one.test");
    assert_eq!(read_env(&first_dir)["ANTHROPIC_AUTH_TOKEN"], "token-one");
    assert_eq!(read_env(&second_dir)["ANTHROPIC_BASE_URL"], "http://provider-two.test");
    assert_eq!(read_env(&second_dir)["ANTHROPIC_AUTH_TOKEN"], "token-two");
}
```

The production mutation caught by this test is reverting to a shared fixed directory or writing one provider's environment into both settings files.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test --lib two_providers_write_independent_settings`

Expected: compile failure because `prepare_isolated_config_in` is not defined.

- [ ] **Step 3: Implement the minimal shared preparation helper**

The helper creates a unique child under `<base_dir>/claude-isolated`, filters `ANTHROPIC_` and `CLAUDE_` variables, atomically writes `settings.json`, writes onboarding markers, and returns the created directory. Replace the duplicated preparation block in `launch` with this helper.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run: `cargo test --lib two_providers_write_independent_settings`

Expected: PASS.

- [ ] **Step 5: Run full verification**

Run:

```text
npm run build
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Expected: all commands exit 0 with no test failures or warnings.

## Self-Review

- Spec coverage: all four remaining pages and the two-provider filesystem behavior are covered.
- Placeholder scan: no deferred implementation placeholders.
- Type consistency: page props match existing `App.tsx` render calls; the isolation helper consumes the existing `HashMap<String, String>` environment shape.
