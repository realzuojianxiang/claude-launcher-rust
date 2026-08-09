# Model Usage Statistics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add accurate real-time and persisted historical model usage statistics for Token consumption, final failures, and extra retries to the Dashboard for both NVIDIA and Grok.

**Architecture:** Add one app-owned `UsageStatsState` backed by a thread-safe `UsageStatsStore`. Inject the shared store into both provider proxy contexts and record exactly one `UsageRecord` when each logical `/v1/messages` request reaches its final outcome. Expose one Tauri snapshot command; React renders the snapshot with a dense professional Dashboard made from focused chart/table components.

**Tech Stack:** Rust 2021, Tauri 2, Axum 0.7, Tokio, Serde/JSON, Chrono; React 18, TypeScript, Vitest, Testing Library, existing CSS tokens and Lucide icons. No new runtime dependency.

## Global Constraints

- Count failures at logical-request level: only a request whose final outcome fails increments `failed_requests`.
- Count retries as extra upstream attempts; the first upstream attempt is not a retry.
- Count only provider `usage` fields; missing usage contributes zero Token and increments `usage_missing_requests`.
- `live` means current process only; `7d`, `30d`, and `all` merge persisted daily data with current-process data.
- Persist only aggregate counts; never persist prompts, response bodies, API keys, or auth tokens.
- Statistics failures must not change the provider HTTP/SSE response.
- Keep existing provider retry, fallback, security, and streaming semantics unchanged.
- Do not add a chart library; use an accessible SVG chart and existing CSS.
- Preserve unrelated working-tree changes; stage and commit only files belonging to this feature.

---

## File Map

Create:

- `src-tauri/src/stats.rs` — usage record, aggregation, persistence, range snapshots, and Rust tests.
- `src/pages/dashboard/UsageTrendChart.tsx` — accessible SVG trend visualization.
- `src/pages/dashboard/ModelStatsTable.tsx` — sortable model/provider statistics table.
- `src/pages/dashboard/usageStats.ts` — frontend range types, formatting, and snapshot helpers.
- `src/pages/dashboard/usageStats.test.ts` — frontend formatting and derived-value tests.
- `src/pages/dashboard/UsageTrendChart.test.tsx` — chart rendering and text-summary tests.
- `src/pages/dashboard/ModelStatsTable.test.tsx` — table rendering and sorting tests.

Modify:

- `src-tauri/src/lib.rs` — register `stats` module/state, `get_usage_stats`, and pass the shared store into provider starts.
- `src-tauri/src/nvidia/mod.rs` — pass the shared store into `NvidiaState::start` and `ProxyCtx`.
- `src-tauri/src/nvidia/proxy.rs` — create logical request metrics and record final non-stream/stream outcomes.
- `src-tauri/src/grok/mod.rs` — pass the shared store into `GrokState::start` and `ProxyCtx`.
- `src-tauri/src/grok/proxy.rs` — create logical request metrics and record final non-stream/stream outcomes.
- `src-tauri/src/grok/stream.rs` — expose whether usage was observed and the final usage totals.
- `src/types.ts` — add the TypeScript snapshot contract and range type.
- `src/pages/DashboardPage.tsx` — load, poll, filter, and compose the statistics Dashboard.
- `src/pages/DashboardPage.test.tsx` — replace static-only assertions with configuration and statistics coverage.
- `src/styles.css` — add Dashboard statistics layout, cards, chart, table, state banner, and responsive rules.

---

## Task 1: Build the Rust statistics domain and persistence layer

**Files:**

- Create: `src-tauri/src/stats.rs`
- Modify: `src-tauri/src/lib.rs:1-20` to declare `mod stats;` after the existing modules.

**Interfaces:**

- Produces `UsageRecord`, `UsageRange`, `UsageStatsSnapshot`, `UsageStatsState`, and `UsageStatsStore` for provider wiring and the Tauri command.
- `UsageStatsStore::record(&self, record: UsageRecord)` must update live and daily aggregates and return `Result<(), StatsWriteError>` without losing the in-memory update when persistence fails.
- `UsageStatsStore::snapshot(&self, range: UsageRange) -> UsageStatsSnapshot` must be read-only and safe during concurrent provider updates.
- `UsageStatsState::new() -> Self` loads `usage-stats.json` from `Config::config_dir()` and exposes an `Arc<UsageStatsStore>` for `NvidiaState` and `GrokState`.

### Step 1: Write the failing domain tests

Add `#[cfg(test)] mod tests` in the new module. Use a unique directory under `std::env::temp_dir()` and remove it at the end of each persistence test; do not add `tempfile` just for tests. Start with these behaviors:

```rust
#[test]
fn record_success_aggregates_tokens_and_zero_failures() {
    let store = UsageStatsStore::in_memory();
    store.record(UsageRecord::success("nvidia", "nvidia/a", 120, 30, 0, true));

    let snapshot = store.snapshot(UsageRange::Live);
    assert_eq!(snapshot.totals.requests, 1);
    assert_eq!(snapshot.totals.input_tokens, 120);
    assert_eq!(snapshot.totals.output_tokens, 30);
    assert_eq!(snapshot.totals.total_tokens, 150);
    assert_eq!(snapshot.totals.failed_requests, 0);
    assert_eq!(snapshot.totals.retry_count, 0);
}

#[test]
fn final_failure_counts_once_and_keeps_extra_attempts_as_retries() {
    let store = UsageStatsStore::in_memory();
    store.record(UsageRecord::failure("grok", "grok-4.5", 2, false));

    let totals = store.snapshot(UsageRange::Live).totals;
    assert_eq!(totals.requests, 1);
    assert_eq!(totals.failed_requests, 1);
    assert_eq!(totals.retry_count, 2);
}

#[test]
fn persisted_daily_data_round_trips_and_range_merge_includes_live_data() {
    let dir = unique_test_dir("usage-stats-roundtrip");
    let path = dir.join("usage-stats.json");
    let first = UsageStatsStore::from_path(path.clone()).unwrap();
    first.record_at(UsageRecord::success("grok", "grok-4.5", 100, 50, 1, true), day("2026-08-08"));
    drop(first);

    let second = UsageStatsStore::from_path(path).unwrap();
    second.record(UsageRecord::success("nvidia", "nvidia/a", 20, 10, 0, true));
    let totals = second.snapshot(UsageRange::All).totals;
    assert_eq!(totals.requests, 2);
    assert_eq!(totals.total_tokens, 180);
    remove_test_dir(dir);
}
```

The helper constructors used above are test-friendly wrappers around the public record shape; `record_at` is only needed to make date-range tests deterministic.

### Step 2: Run the focused Rust tests and verify they fail

Run from `src-tauri`:

```text
cargo test --lib stats::tests
```

Expected: FAIL because `stats` and its public types do not exist yet.

### Step 3: Implement the minimal statistics store

Implement these concrete types and rules:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum UsageRange { Live, Days7, Days30, All }

#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub provider: String,
    pub requested_model: String,
    pub final_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usage_available: bool,
    pub retry_count: u32,
    pub failed: bool,
    pub at: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageStatsSnapshot {
    pub range: UsageRange,
    pub generated_at: String,
    pub totals: Aggregate,
    pub trend: Vec<TrendPoint>,
    pub models: Vec<ModelAggregate>,
    pub providers: Vec<ProviderAggregate>,
    pub history_recovered: bool,
    pub history_writable: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Aggregate {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
    pub success_rate: f64,
    pub usage_missing_requests: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrendPoint {
    pub label: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelAggregate {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
    pub usage_missing_requests: u64,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAggregate {
    pub provider: String,
    pub requests: u64,
    pub total_tokens: u64,
    pub failed_requests: u64,
    pub retry_count: u64,
}

pub struct UsageStatsState { store: Arc<UsageStatsStore> }
```

Use a `StatsDocument { version: u32, days: BTreeMap<String, DailyBucket> }` persisted as JSON. Each `DailyBucket` contains provider/model aggregates and a daily trend point. Keep a separate in-memory `live` aggregate and 15-minute buckets. Aggregate keys must include provider and final model. Define `StatsWriteError` as an internal error containing only the file path and I/O message; never include prompt, body, or credential values in its display text.

Implement `record` as:

1. Lock the state.
2. Update the current-process aggregate and current 15-minute bucket.
3. Update the local-date daily bucket.
4. Clone the serializable document while holding the lock.
5. Release the lock and atomically write the cloned document.
6. Return a write error only for observability; keep the in-memory update.

`snapshot` must merge the selected persisted buckets and live aggregate, calculate `total_tokens = input_tokens + output_tokens`, and calculate `success_rate = (requests - failed_requests) / requests`, returning `1.0` for an empty set. Sort models by `total_tokens` descending by default.

Use `Config::config_dir().join("usage-stats.json")` for the app store. Reuse the existing atomic write pattern and corrupt-file evidence naming from `config.rs`/`history.rs`; do not log sensitive fields.

### Step 4: Run the focused tests and verify they pass

```text
cargo test --lib stats::tests
```

Expected: PASS for aggregation, retry/failure semantics, persistence, range merge, empty snapshots, and corrupt-file recovery.

### Step 5: Commit the self-contained domain layer

```text
git add src-tauri/src/stats.rs src-tauri/src/lib.rs
git commit -m "feat: add persisted usage statistics store"
```

---

## Task 2: Wire the shared store into provider lifecycle and commands

**Files:**

- Modify: `src-tauri/src/stats.rs:UsageRange` to add `parse_usage_range` and its tests.
- Modify: `src-tauri/src/lib.rs:1-20, nvidia_start, grok_start, run().manage/invoke_handler`
- Modify: `src-tauri/src/nvidia/mod.rs:Running, NvidiaState::start`
- Modify: `src-tauri/src/grok/mod.rs:Running, GrokState::start`

**Interfaces:**

- `NvidiaState::start(&self, cfg: NvidiaConfig, stats: Arc<UsageStatsStore>)`.
- `GrokState::start(&self, cfg: GrokConfig, stats: Arc<UsageStatsStore>)`.
- `get_usage_stats(range: String, stats: State<'_, UsageStatsState>) -> UsageStatsSnapshot`.

### Step 1: Write the failing wiring tests/checks

Add `parse_usage_range(&str) -> UsageRange` to `src-tauri/src/stats.rs` and cover it in that module's test block. Also add `UsageStatsState::shared_store_is_cloneable`, which clones the store twice and records through one clone before snapshotting through the other; this proves both provider contexts can share one store.

```rust
#[test]
fn usage_range_command_defaults_unknown_values_to_seven_days() {
    assert_eq!(parse_usage_range("unexpected"), UsageRange::Days7);
    assert_eq!(parse_usage_range("live"), UsageRange::Live);
}

#[test]
fn shared_state_returns_the_same_store_to_both_providers() {
    let state = UsageStatsState::in_memory();
    let first = state.store();
    let second = state.store();
    first.record(UsageRecord::success("nvidia", "a", 1, 1, 0, true));
    assert_eq!(second.snapshot(UsageRange::Live).totals.requests, 1);
}
```

### Step 2: Run the focused test and verify it fails

```text
cargo test --lib parse_usage_range
```

Expected: FAIL because the command parser and shared state are not present.

### Step 3: Implement app-state wiring

- Import `UsageStatsState`, `UsageStatsStore`, and `UsageRange` in `lib.rs`.
- Register `.manage(UsageStatsState::new())` before provider states.
- Add `get_usage_stats` to `tauri::generate_handler!`.
- Inject `stats.store()` into `nvidia_start` and `grok_start`.
- Update both provider `start` methods to pass the same store into `ProxyCtx::new`.
- Keep diagnostic-only `NvidiaState::new().start(cfg)` and `GrokState::new().start(cfg)` call sites compiling by creating an in-memory store in those diagnostic branches.
- Return a snapshot even when persistence is unwritable; use snapshot health flags for the UI.

### Step 4: Run tests and compile checks

```text
cargo test --lib parse_usage_range stats::tests
cargo check --tests
```

Expected: PASS and no type errors from Tauri state registration or provider start signatures.

### Step 5: Commit the wiring layer

```text
git add src-tauri/src/lib.rs src-tauri/src/nvidia/mod.rs src-tauri/src/grok/mod.rs
git commit -m "feat: expose usage statistics through Tauri state"
```

---

## Task 3: Record final usage from NVIDIA and Grok requests

**Files:**

- Modify: `src-tauri/src/nvidia/proxy.rs:ProxyCtx, handle_messages, non_stream_response, stream_response`
- Modify: `src-tauri/src/grok/proxy.rs:ProxyCtx, handle_messages, non_stream_response, stream_response`
- Modify: `src-tauri/src/grok/stream.rs:StreamState`
- Test: existing provider proxy test modules in `src-tauri/src/nvidia/proxy.rs` and `src-tauri/src/grok/proxy.rs`

**Interfaces:**

- `ProxyCtx` gains `pub stats: Arc<UsageStatsStore>` and `ProxyCtx::new` receives the shared store.
- Provider handlers create one logical request context containing requested model, last model, attempts, and finalization guard.
- `UsageStatsStore::record` is called exactly once for every request that passes local authentication/body parsing and enters upstream attempt handling.

### Step 1: Write failing provider instrumentation tests

Extend the existing local Axum upstream tests with a store assertion after the downstream response is consumed:

```rust
let stats = UsageStatsStore::in_memory();
let ctx = ProxyCtx::new(test_config(), stats.clone());
let response = handle_messages(State(ctx), request_with_model("grok-4.5")).await;
let _body = collect_response_body(response).await;

let snapshot = stats.snapshot(UsageRange::Live);
assert_eq!(snapshot.totals.requests, 1);
assert_eq!(snapshot.totals.failed_requests, 0);
assert_eq!(snapshot.totals.retry_count, 1);
assert_eq!(snapshot.models[0].model, "grok-4.5");
```

Add cases for:

- non-stream JSON with `usage.prompt_tokens/completion_tokens` (NVIDIA) or `usage.input_tokens/output_tokens` (Grok);
- completed stream with usage;
- final upstream error after one retry;
- fallback from model A to model B where the final successful row is model B;
- stream error after output, which must record final failure once.

### Step 2: Run the provider tests and verify they fail

```text
cargo test --lib nvidia::proxy::tests grok::proxy::tests
```

Expected: FAIL to compile because `ProxyCtx::new` has no store parameter and no record is emitted.

### Step 3: Add provider request finalization

For both providers:

- Capture the inbound requested model before fallback changes it.
- Keep `attempts` as the authoritative count and pass `attempts.saturating_sub(1)` to the record.
- Update `last_model` before each upstream attempt.
- On successful non-stream response, parse usage from the upstream JSON and record before returning the converted response.
- On final non-stream error, record `failed = true`, zero usage, and the last attempted model.
- For streaming responses, move a clone of the store and a record context into the stream closure. Record only on the single normal-completion or error-finalization path; use a boolean/guard so `return` branches cannot double-record.
- Treat a missing `usage` object as `usage_available = false`, not as a failure.

NVIDIA usage extraction must use the already converted OpenAI response fields (`prompt_tokens`, `completion_tokens`, or the existing pointer helpers). Grok non-stream usage must read Responses `usage.input_tokens` and `usage.output_tokens`; its stream path must use the final `StreamState` usage.

In `grok/stream.rs`, add a small public method such as:

```rust
pub fn usage_snapshot(&self) -> (u64, u64, bool) {
    (self.input_tokens, self.output_tokens, self.usage_available)
}
```

Set `usage_available = true` whenever a valid upstream usage object is observed. Do not change Anthropic SSE event output.

### Step 4: Run provider tests and regression checks

```text
cargo test --lib nvidia::proxy::tests grok::proxy::tests grok::stream::tests
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: all existing proxy/stream tests plus new stats assertions PASS, with no new warnings.

### Step 5: Commit provider instrumentation

```text
git add src-tauri/src/nvidia/proxy.rs src-tauri/src/grok/proxy.rs src-tauri/src/grok/stream.rs
git commit -m "feat: record provider usage and retry outcomes"
```

---

## Task 4: Add TypeScript contract and Dashboard data helpers

**Files:**

- Modify: `src/types.ts` after the existing config types.
- Create: `src/pages/dashboard/usageStats.ts`
- Create: `src/pages/dashboard/usageStats.test.ts`

**Interfaces:**

- `UsageRange = "live" | "7d" | "30d" | "all"`.
- `UsageStatsSnapshot` mirrors the Tauri JSON snapshot fields exactly in snake_case.
- `formatTokenCount(value: number): string` returns compact `K/M/B` output while preserving exact values for small counts.
- `getUsageTotal(snapshot): number` and `successRateLabel(snapshot): string` provide testable derived display values.

### Step 1: Write failing frontend helper tests

```ts
it("formats token counts compactly without hiding small values", () => {
  expect(formatTokenCount(0)).toBe("0");
  expect(formatTokenCount(999)).toBe("999");
  expect(formatTokenCount(1_280_000)).toBe("1.28M");
});

it("uses the backend total when rendering the KPI", () => {
  expect(getUsageTotal(snapshotFixture)).toBe(1_280_000);
  expect(successRateLabel(snapshotFixture)).toBe("96.8%");
});
```

### Step 2: Run the focused frontend tests and verify they fail

```text
npm test -- src/pages/dashboard/usageStats.test.ts
```

Expected: FAIL because the helpers and snapshot fixture do not exist.

### Step 3: Add the exact TypeScript contract and helpers

Add this exact contract to `src/types.ts`; keep backend field names snake_case so `invoke` results do not need a lossy transformation:

```ts
export type UsageRange = "live" | "7d" | "30d" | "all";

export interface UsageAggregate {
  requests: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  failed_requests: number;
  retry_count: number;
  success_rate: number;
  usage_missing_requests: number;
}

export interface UsageTrendPoint {
  label: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  failed_requests: number;
  retry_count: number;
}

export interface UsageModelAggregate extends UsageAggregate {
  provider: string;
  model: string;
}

export interface UsageProviderAggregate {
  provider: string;
  requests: number;
  total_tokens: number;
  failed_requests: number;
  retry_count: number;
}

export interface UsageStatsSnapshot {
  range: UsageRange;
  generated_at: string;
  totals: UsageAggregate;
  trend: UsageTrendPoint[];
  models: UsageModelAggregate[];
  providers: UsageProviderAggregate[];
  history_recovered: boolean;
  history_writable: boolean;
}
```

Add deterministic formatters with no locale-dependent output.

Create a small `emptyUsageStatsSnapshot(range)` helper for the Dashboard loading/error fallback and a `sortModelStats(rows, sortKey, direction)` helper with stable tie-breaking by provider then model.

### Step 4: Run the tests and build

```text
npm test -- src/pages/dashboard/usageStats.test.ts
npm run build
```

Expected: PASS and TypeScript compilation succeeds.

### Step 5: Commit the frontend contract

```text
git add src/types.ts src/pages/dashboard/usageStats.ts src/pages/dashboard/usageStats.test.ts
git commit -m "feat: add usage statistics frontend contract"
```

---

## Task 5: Build the professional Dashboard visualizations

**Files:**

- Create: `src/pages/dashboard/UsageTrendChart.tsx`
- Create: `src/pages/dashboard/UsageTrendChart.test.tsx`
- Create: `src/pages/dashboard/ModelStatsTable.tsx`
- Create: `src/pages/dashboard/ModelStatsTable.test.tsx`
- Modify: `src/pages/DashboardPage.tsx`
- Modify: `src/pages/DashboardPage.test.tsx`
- Modify: `src/styles.css` near existing `.stat-card`/Dashboard responsive rules.

**Interfaces:**

- `UsageTrendChart({ points, range })` renders an SVG chart plus a visible text summary and legend.
- `ModelStatsTable({ rows })` renders sortable model rows with provider, Token, failure, retry, success rate, and missing-usage hint.
- `DashboardPage` owns `range`, `snapshot`, `loading`, `error`, `refreshing`, and the 5-second polling timer.

### Step 1: Write failing component tests

`UsageTrendChart.test.tsx` must assert the chart title, legend, accessible summary, and a point label from the fixture. `ModelStatsTable.test.tsx` must assert model/provider names, formatted Token, failure/retry counts, and that clicking the `重试` sort control changes row order. Extend `DashboardPage.test.tsx` with a mocked `invoke` implementation:

```tsx
const usageSnapshotFixture: UsageStatsSnapshot = {
  range: "7d",
  generated_at: "2026-08-09T12:00:00+08:00",
  totals: {
    requests: 248, input_tokens: 900000, output_tokens: 380000,
    total_tokens: 1280000, failed_requests: 8, retry_count: 17,
    success_rate: 0.968, usage_missing_requests: 1,
  },
  trend: [{ label: "今天", requests: 32, input_tokens: 120000, output_tokens: 50000,
    total_tokens: 170000, failed_requests: 1, retry_count: 2 }],
  models: [{ provider: "grok", model: "grok-4.5", requests: 132,
    input_tokens: 520000, output_tokens: 226000, total_tokens: 746000,
    failed_requests: 2, retry_count: 8, usage_missing_requests: 0, success_rate: 0.985 }],
  providers: [{ provider: "grok", requests: 132, total_tokens: 746000,
    failed_requests: 2, retry_count: 8 }],
  history_recovered: false,
  history_writable: true,
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(usageSnapshotFixture),
}));

it("loads and renders usage metrics beside the existing config summary", async () => {
  render(<DashboardPage config={configFixture} />);
  expect(await screen.findByText("总 Token")).toBeInTheDocument();
  expect(screen.getByText("1.28M")).toBeInTheDocument();
  expect(screen.getByText("grok-4.5")).toBeInTheDocument();
});
```

### Step 2: Run the focused tests and verify they fail

```text
npm test -- src/pages/DashboardPage.test.tsx src/pages/dashboard
```

Expected: FAIL because the new components and usage sections do not exist.

### Step 3: Implement the chart and table

For `UsageTrendChart`:

- Normalize values against the maximum point to calculate SVG heights.
- Render separate input/output bars or paths using existing primary/success colors.
- Add `role="img"`, an `aria-label` with the range and totals, and a visible text summary for reduced-motion/screen-reader use.
- Render an empty state when every point is zero.

For `ModelStatsTable`:

- Keep sort state local with default `total_tokens` descending.
- Use semantic `<table>`, `<caption>`, `<thead>`, `<tbody>`, and `<button>` sort controls.
- Show a muted `用量不完整` hint when `usage_missing_requests > 0`.
- Keep the Provider label visible so identical model names cannot be confused.

### Step 4: Compose DashboardPage and style it

Implement this state flow:

```tsx
const [range, setRange] = useState<UsageRange>("7d");
const [snapshot, setSnapshot] = useState<UsageStatsSnapshot | null>(null);
const [loading, setLoading] = useState(true);
const [refreshing, setRefreshing] = useState(false);
const [error, setError] = useState<string | null>(null);

const loadStats = useCallback(async (nextRange = range) => {
  setRefreshing(true);
  try {
    const next = await invoke<UsageStatsSnapshot>("get_usage_stats", { range: nextRange });
    setSnapshot(next);
    setError(null);
  } catch (cause) {
    setError(cause instanceof Error ? cause.message : String(cause));
  } finally {
    setLoading(false);
    setRefreshing(false);
  }
}, [range]);

useEffect(() => {
  void loadStats(range);
  const timer = window.setInterval(() => void loadStats(range), 5000);
  return () => window.clearInterval(timer);
}, [loadStats, range]);
```

The page must retain the existing provider-configuration cards and add the approved dense layout: title/status row, four KPI cards, range tabs, trend card, provider summary, model table, and non-blocking health banners. Use existing `card`, `stat-card`, `grid`, `status-banner`, and CSS variables where possible; add focused class names such as `.usage-dashboard`, `.usage-metrics`, `.usage-chart`, and `.usage-table` rather than broad global overrides.

Add responsive rules at the existing 1024px, 768px, and 560px breakpoints. Respect `prefers-reduced-motion` and keep all interactive controls keyboard reachable.

### Step 5: Run tests and inspect the rendered result

```text
npm test -- src/pages/DashboardPage.test.tsx src/pages/dashboard
npm run build
```

Expected: all Dashboard/component tests PASS and the production build succeeds. Start the desktop preview with `npm run tauri dev`, then verify the four cards, trend chart, table, empty state, and narrow-window layout against the approved A mockup before committing.

### Step 6: Commit the Dashboard UI

```text
git add src/pages/DashboardPage.tsx src/pages/DashboardPage.test.tsx src/pages/dashboard src/styles.css
git commit -m "feat: add model usage statistics dashboard"
```

---

## Task 6: Full verification and handoff

**Files:**

- Modify only if verification exposes a feature regression: the specific file under test.

### Step 1: Run the complete frontend suite

```text
npm test
npm run build
```

Expected: all Vitest tests PASS and `dist/` builds without TypeScript errors.

### Step 2: Run the complete Rust checks

```text
cd src-tauri
cargo fmt --all -- --check
cargo check --tests
cargo clippy --all-targets -- -D warnings
cargo test --lib
```

Expected: formatting, compile, lint, and library tests all PASS. If Windows cannot execute the Rust test binary because of the documented DLL loader issue, report that exact limitation and retain the successful `cargo check --tests` evidence.

### Step 3: Inspect the feature diff and working tree

```text
git diff --check HEAD~5..HEAD
git status --short
git log --oneline -6
```

Confirm the feature commits contain only the planned files and unrelated existing edits remain untouched.

### Step 4: Final handoff

Report:

- the new Tauri command and persisted file name;
- the Dashboard ranges and request-level counting semantics;
- test/build commands and their actual results;
- any environment-only verification limitation;
- links to the changed Dashboard and design/plan documents.
