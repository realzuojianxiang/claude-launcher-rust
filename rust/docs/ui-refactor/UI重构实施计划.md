# Apple 风格 UI 重构实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不改变 Claude Launcher 业务流程、IPC 契约和数据口径的前提下，将全部前端页面重构为统一、可访问、响应式且支持浅色/深色模式的 Apple HIG 风格生产界面。

**Architecture:** 保留 `App` 条件渲染和现有页面卸载语义，以语义 CSS Token 和小型无依赖 UI primitives 作为基础层；页面继续拥有各自 IPC 与业务状态，只重构展示、异步状态和交互防护。所有业务行为先由 Vitest 回归测试锁定，再按基础组件 → AppShell → 核心页面 → 高风险页面 → 视觉收口的顺序迁移。

**Tech Stack:** React 18、TypeScript 5.6 strict、Vite 8、Vitest 4、Testing Library、Tauri v2 IPC、lucide-react、原生 CSS。

## Global Constraints

- 唯一工作区是 `D:\BaiduSyncdisk\ai-agent\claude-launcher\rust`；所有命令在此目录执行。
- 当前分支是 `feat/rust-implementation`，不是 main/master；遵守用户指定工作区，不创建额外 worktree。
- 不修改现有业务流程、接口契约、数据口径、路由语义、权限逻辑和用户实际使用方式。
- 不修改 `src-tauri/src/**`、Tauri IPC 注册、CSP、capability、Rust 配置结构或数据库对象。
- 不引入第三方 UI 框架；只使用现有 React、lucide-react、CSS 与测试依赖。
- 使用 `UI设计Token.md` 中的系统字体、语义颜色、8pt 间距、统一圆角/阴影和 160–320ms 动效。
- 支持浅色/深色、`prefers-reduced-motion`、`prefers-reduced-transparency`。
- 所有可点击控件的实际交互目标至少 44×44px，并具有可见 `:focus-visible`。
- Loading、Empty、Error、Disabled、Hover、Focus、Active、Submitting、校验失败、无权限和网络失败必须有可恢复表达；不把失败误报为空。
- 不使用紫蓝渐变、教程式装饰、Lorem Ipsum、`Item 1`、无目的无限动画或只覆盖理想状态的分支。
- 每个行为变更必须先写失败测试并确认因缺失行为而失败；CSS/文档通过构建、静态审计和运行视觉证据验证。
- 静态 `rg` 禁止项检查中，退出码 1 表示“无匹配，门禁通过”，只有退出码大于 1 才表示命令错误。
- 每个任务完成后立即更新 `UI重构记录.md`、`UI审计清单.md` 和需要变更的 `UI设计Token.md`。
- 不把账号、密码、API Key、Token、真实日志内容或其他敏感数据写入测试、快照、文档和日志。
- 本轮 UI 对比基线固定为 `UI_BASE_SHA=50808c5`；最终 diff、审查和契约核对必须同时检查 `50808c5..HEAD` 与未提交工作树。
- 每次提交前先运行 `git status --short`、`git diff --name-only`，只暂存当前 Task 的精确文件清单；暂存后运行 `git diff --cached --name-only` 和 `git diff --cached --check`，发现清单外文件即停止提交并排除该文件。

## 阶段 0 文档基线检查点

在视觉 Gate 等待期间，先用独立提交保存计划、审计、Token、验收矩阵和可复现基线。精确清单为：

```powershell
git status --short
git add -- docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI设计Token.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/baseline-build.md docs/ui-refactor/evidence/baseline-ipc.md docs/ui-refactor/evidence/before-source-manifest.md
git diff --cached --name-only
git diff --cached --check
git commit -m "docs(ui): establish refactor baseline and plan"
git status --short
```

`UI重构Goal.md` 是用户所有的未跟踪输入文件，不在该提交内。

## Pre-implementation Gate：重构前视觉基线

Goal 明确要求在修改视觉代码前保存重构前截图。当前应用内浏览器因安全策略不能访问本地 Vite URL，因此 Task 1–9 暂不得开始，直到以下 Gate 全部满足：

- [x] 在获准的 Tauri 原生窗口或能合法访问 localhost 的受支持浏览器中打开 commit `50808c5` 对应界面。
- [x] 以同一应用窗口尺寸保存仪表盘、启动 Claude、CLIProxyAPI、NVIDIA、日志、配置、单词本、关于 8 个默认页面截图。
- [x] 额外保存日志历史弹窗、配置未保存提示、至少一个 Loading/Empty/Error 场景；截图必须使用脱敏数据。
- [x] 将文件保存到 `docs/ui-refactor/evidence/before/`，并在 `evidence/README.md` 记录视口、主题、数据状态和文件名。
- [x] 校验图片可打开且不含 API Key、Token、真实日志、敏感路径或其他凭证。

若 Gate 仍受外部策略阻止，只能继续完善计划和文档，不能开始大范围视觉修改，也不能把最终“受阻”写成 Goal 已完成。

Gate 使用应用默认窗口 `1100×720` 保存以下固定文件名，避免后续用目录/glob 暂存未知文件：

```text
docs/ui-refactor/evidence/before/README.md
docs/ui-refactor/evidence/before/dashboard-default-light-1100x720.png
docs/ui-refactor/evidence/before/launch-default-light-1100x720.png
docs/ui-refactor/evidence/before/cliproxy-default-light-1100x720.png
docs/ui-refactor/evidence/before/nvidia-default-light-1100x720.png
docs/ui-refactor/evidence/before/logs-default-light-1100x720.png
docs/ui-refactor/evidence/before/config-default-light-1100x720.png
docs/ui-refactor/evidence/before/dictionary-default-light-1100x720.png
docs/ui-refactor/evidence/before/about-default-light-1100x720.png
docs/ui-refactor/evidence/before/logs-dialog-light-1100x720.png
docs/ui-refactor/evidence/before/config-dirty-light-1100x720.png
docs/ui-refactor/evidence/before/state-loading-light-1100x720.png
docs/ui-refactor/evidence/before/state-empty-light-1100x720.png
docs/ui-refactor/evidence/before/state-error-light-1100x720.png
```

全部图片逐张检查后，使用一条显式 `git add -- <上列每个实际文件> docs/ui-refactor/evidence/README.md docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI重构实施计划.md` 命令；必须把尖括号说明替换成上列真实路径，不得原样运行、不得暂存整个目录。随后执行 cached name-only/check，并以 `docs(ui): capture pre-refactor visual baseline` 提交。Gate commit 前 `git diff --cached --name-only` 必须只包含上述证据和四个被更新的索引/记录文件。

### 批次视觉回归协议（Gate 通过后）

视觉证据按用户可见批次保存到 `docs/ui-refactor/evidence/after/task-XX/`。每个批次必须复用 Gate 的固定窗口尺寸、脱敏数据和对应 before 页面，记录主题、宽度、状态、文件名、人工结论及差异；发现布局遮挡、信息丢失、焦点不可见或业务状态误报时，先修复并重拍，再提交该批次。

| 批次 | 页面/状态 | 最小视觉证据 |
|---|---|---|
| Task 1 | 尚未被页面消费的 primitives | 明确记录“无可达 UI 变化，截图 N/A”；在 Task 2 首次消费时一并验证 |
| Task 2 | AppShell、Sidebar、启动 Loading/Error、导航 Active/Focus | Light 1280、Dark 1280、Light 560 |
| Task 3 | Dashboard、Launch、CLIProxy | 默认、Loading、Error、Submitting，至少 Light 1280 与 560 |
| Task 4 | Config、Confirm、secret input | 默认、Dirty、校验失败、Submitting、Error，Light/Dark 1280 |
| Task 5 | NVIDIA | 默认、Status/Key pool Error、Submitting、secret 隐藏、外部 host 警告，Light/Dark 1280 与 560 |
| Task 6 | Logs/Dialog | live/history Empty/Error、打开 Dialog、键盘焦点，Light/Dark 1280 与 560 |
| Task 7 | Dictionary | Loading/Error、tab、搜索 Empty、学习完成、导入 Error，Light/Dark 1280 与 560 |
| Task 8 | 全部 8 页 | 1280/768/560、Light/Dark、reduced-motion 最终矩阵 |

如果 Gate 通过后视觉访问再次失效，停止后续用户可见批次，在记录中写明最后一个已验证批次和阻断证据；不得用静态源码或单测冒充截图回归。

Tauri 原生窗口配置的最小宽度是 800px，且该配置属于保护范围。560px 证据必须来自获准的浏览器或 WebView DevTools 响应式模拟，不得为了截图修改 `src-tauri/tauri.conf.json`。

---

### Task 1: 建立 UI primitives 与结构化状态

**Files:**

- Create: `src/components/ui/Button.tsx`
- Create: `src/components/ui/Button.test.tsx`
- Create: `src/components/ui/StatusBanner.tsx`
- Create: `src/components/ui/StatusBanner.test.tsx`
- Create: `src/components/ui/AsyncState.tsx`
- Create: `src/components/ui/AsyncState.test.tsx`
- Create: `src/components/ui/Skeleton.tsx`
- Create: `src/components/ui/Skeleton.test.tsx`
- Create: `src/components/ui/FormField.tsx`
- Create: `src/components/ui/FormField.test.tsx`
- Modify: `src/styles.css`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Create: `docs/ui-refactor/evidence/after/task-01/README.md`

**Interfaces:**

- Produces:

```ts
export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

export interface ButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  variant?: ButtonVariant;
  loading?: boolean;
  loadingLabel?: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
}

export type StatusKind = "success" | "warning" | "error" | "info";

export interface StatusMessage {
  kind: StatusKind;
  title: string;
  detail?: string;
}

export interface StatusBannerProps {
  message: StatusMessage | null;
  action?: React.ReactNode;
  onDismiss?: () => void;
}

export interface AsyncStateProps {
  title: string;
  detail?: string;
  kind?: "loading" | "empty" | "error" | "permission" | "network";
  action?: React.ReactNode;
  compact?: boolean;
}

export interface SkeletonProps {
  lines?: number;
  label: string;
}

export interface FormFieldProps {
  id: string;
  label: string;
  hint?: string;
  error?: string;
  success?: string;
  status?: "default" | "success" | "error";
  required?: boolean;
  children: (describedBy: string | undefined) => React.ReactNode;
}
```

- Later tasks consume these components without changing their signatures.

- [x] **Step 1: Write failing Button tests**

Create tests that name the user-visible breaks:

```tsx
test("loading button blocks duplicate activation and exposes busy state", () => {
  const onClick = vi.fn();
  render(
    <Button loading loadingLabel="正在保存" onClick={onClick}>
      保存
    </Button>,
  );

  const button = screen.getByRole("button", { name: "正在保存" });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("aria-busy", "true");
  fireEvent.click(button);
  expect(onClick).not.toHaveBeenCalled();
});

test("button defaults to type button so it cannot submit a parent form", () => {
  render(<Button>取消</Button>);
  expect(screen.getByRole("button", { name: "取消" })).toHaveAttribute(
    "type",
    "button",
  );
});
```

- [x] **Step 2: Run Button tests and verify RED**

Run:

```powershell
npx.cmd vitest run src/components/ui/Button.test.tsx
```

Expected: FAIL because `Button.tsx` does not exist.

- [x] **Step 3: Implement Button**

Implement the exact state contract:

```tsx
export function Button({
  variant = "secondary",
  loading = false,
  loadingLabel = "处理中",
  icon,
  children,
  className = "",
  disabled,
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <button
      {...props}
      type={type}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={`ui-button ui-button--${variant} ${className}`.trim()}
    >
      {loading ? <LoaderCircle className="ui-spinner" aria-hidden="true" /> : icon}
      <span>{loading ? loadingLabel : children}</span>
    </button>
  );
}
```

Use `LoaderCircle` from `lucide-react`; do not use emoji or inline colors.

- [x] **Step 4: Write failing StatusBanner, AsyncState, Skeleton and FormField tests**

```tsx
test("error message is announced immediately and exposes its recovery action", () => {
  render(
    <StatusBanner
      message={{ kind: "error", title: "保存失败", detail: "配置未更改" }}
      action={<button type="button">重试</button>}
    />,
  );
  expect(screen.getByRole("alert")).toHaveTextContent("保存失败");
  expect(screen.getByRole("button", { name: "重试" })).toBeEnabled();
});

test("success message uses a polite status region", () => {
  render(
    <StatusBanner message={{ kind: "success", title: "已保存" }} />,
  );
  expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
});

test("network failure is not rendered as an empty state", () => {
  render(
    <AsyncState
      kind="network"
      title="无法连接本地代理"
      action={<button type="button">重新检测</button>}
    />,
  );
  expect(screen.getByRole("alert")).toHaveTextContent("无法连接本地代理");
  expect(screen.getByRole("button", { name: "重新检测" })).toBeEnabled();
});

test("form field connects labels, help and validation state to its control", () => {
  render(
    <FormField
      id="port"
      label="监听端口"
      hint="范围 1 到 65535"
      error="端口超出范围"
      status="error"
    >
      {(describedBy) => (
        <input id="port" aria-describedby={describedBy} aria-invalid="true" />
      )}
    </FormField>,
  );
  expect(screen.getByRole("textbox", { name: "监听端口" })).toHaveAttribute(
    "aria-describedby",
    "port-hint port-error",
  );
  expect(screen.getByRole("alert")).toHaveTextContent("端口超出范围");
});

test("skeleton announces what is loading without exposing decorative bars", () => {
  render(<Skeleton label="正在读取配置" lines={3} />);
  expect(screen.getByRole("status")).toHaveTextContent("正在读取配置");
  expect(screen.getAllByTestId("skeleton-line")).toHaveLength(3);
  expect(screen.getAllByTestId("skeleton-line")[0]).toHaveAttribute(
    "aria-hidden",
    "true",
  );
});
```

- [x] **Step 5: Run state tests and verify RED**

Run:

```powershell
npx.cmd vitest run src/components/ui/StatusBanner.test.tsx src/components/ui/AsyncState.test.tsx src/components/ui/Skeleton.test.tsx src/components/ui/FormField.test.tsx
```

Expected: FAIL because the components do not exist.

- [x] **Step 6: Implement StatusBanner, AsyncState, Skeleton and FormField**

Rules:

- Error/permission/network use `role="alert"`.
- Success/warning/info use `role="status"` and `aria-live="polite"`.
- Every alert/status container is named with `aria-labelledby` pointing at its visible title.
- Status icon comes from lucide and has `aria-hidden="true"`.
- `Skeleton` reserves a stable block, exposes one off-screen loading label through `role="status"`, and marks visual bars `aria-hidden="true"`.
- `FormField` renders `<label htmlFor={id}>`; hint id is `${id}-hint`, error id is `${id}-error`, success id is `${id}-success`, and the callback receives the applicable ids joined by one space.
- Error text uses `role="alert"` and never relies on color alone.
- Success text uses `role="status"`; default/success/error state is reflected by semantic classes and icon + text, not color alone.

- [x] **Step 7: Add primitive styles**

Add `.ui-button`, four variants, `.ui-spinner`, `.ui-skeleton`, `.status-banner`, `.async-state`, `.form-field`, `.field-hint`, `.field-error` using existing variables temporarily. Every button has `min-height: 44px`; focus uses `:focus-visible`; Skeleton uses a restrained shimmer only in normal-motion mode, and Spinner/shimmer stop under reduced motion.

- [x] **Step 8: Verify GREEN and regression**

Run:

```powershell
npx.cmd vitest run src/components/ui/Button.test.tsx src/components/ui/StatusBanner.test.tsx src/components/ui/AsyncState.test.tsx src/components/ui/Skeleton.test.tsx src/components/ui/FormField.test.tsx
npm.cmd test
npm.cmd run build
```

Expected: all new tests and the 13 baseline tests pass; build passes.

视觉批次记录：本 Task 尚无应用内可达调用点，因此在 `evidence/after/task-01/README.md` 记录截图 N/A；不得为制造截图而把测试夹具暴露到生产应用。Task 2 首次消费 primitives 时完成实际视觉回归。

- [x] **Step 9: Update documents and commit**

Record Task 1 files, tests, remaining legacy `MessageBanner` call sites, and rollback commit.

```powershell
git status --short
git diff --name-only
git add -- src/components/ui/Button.tsx src/components/ui/Button.test.tsx src/components/ui/StatusBanner.tsx src/components/ui/StatusBanner.test.tsx src/components/ui/AsyncState.tsx src/components/ui/AsyncState.test.tsx src/components/ui/Skeleton.tsx src/components/ui/Skeleton.test.tsx src/components/ui/FormField.tsx src/components/ui/FormField.test.tsx src/styles.css docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-01/README.md
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): add accessible state primitives"
```

---

### Task 2: Tokenize the AppShell and make startup recoverable

**Files:**

- Create: `src/components/PageErrorBoundary.tsx`
- Create: `src/components/PageErrorBoundary.test.tsx`
- Create: `src/components/Sidebar.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/components/Sidebar.tsx`
- Modify: `src/styles.css`
- Modify: `index.html`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Modify: `docs/ui-refactor/UI设计Token.md`
- Create: `docs/ui-refactor/evidence/after/task-02/README.md` and the six fixed-name PNGs in Step 10

**Interfaces:**

- Consumes: `Button`, `AsyncState`.
- Produces:

```ts
type ConfigLoadState =
  | { status: "loading" }
  | { status: "ready"; config: Config }
  | { status: "error"; detail: string };

export interface PageErrorBoundaryProps {
  resetKey: string;
  onReload?: () => void;
  children: React.ReactNode;
}
```

- App continues to expose the same page props and preserve `cfgProfiles`, `cfgGlobals`, and `nvTest`.

- [ ] **Step 1: Add failing App startup failure/retry test**

Extend `src/App.test.tsx`:

```tsx
test("configuration load failure is visible and retry restores the dashboard", async () => {
  invokeMock
    .mockRejectedValueOnce(new Error("config damaged"))
    .mockResolvedValueOnce(configFixture);

  render(<App />);

  expect(
    await screen.findByRole("alert", { name: "无法读取应用配置" }),
  ).toHaveTextContent("config damaged");
  fireEvent.click(screen.getByRole("button", { name: "重新读取配置" }));

  expect(await screen.findByRole("heading", { name: "仪表盘" })).toBeVisible();
  const configCalls = invokeMock.mock.calls.filter(
    ([command]) => command === "get_config",
  );
  expect(configCalls).toHaveLength(2);
});
```

Ensure the invoke mock returns complete `Config`, `ProxyStatus`, and other structures used after the retry.

- [ ] **Step 2: Run App test and verify RED**

```powershell
npx.cmd vitest run src/App.test.tsx
```

Expected: FAIL because startup failure has no visible alert or retry.

- [ ] **Step 3: Write and run PageErrorBoundary RED test**

Use a child that throws while rendering and pass `onReload={reload}`. Assert the visible named alert and that clicking “重新载入应用” calls the injected reload function exactly once.

```powershell
npx.cmd vitest run src/components/PageErrorBoundary.test.tsx
```

Expected: FAIL because `PageErrorBoundary.tsx` does not exist.

- [ ] **Step 4: Implement recoverable startup and page boundary**

- Replace `loadingCfg` with `ConfigLoadState`.
- Put `invoke("get_config")` in a stable `loadConfig` callback.
- Render `AsyncState kind="error"` with `Button` action when loading fails.
- Preserve the one-time profile draft initialization guard.
- Wrap the `Suspense` page area in `PageErrorBoundary resetKey={active}`.
- `PageErrorBoundary` renders “页面暂时无法显示” and “重新载入应用”; the action calls injected `onReload` or defaults to `window.location.reload()`.
- Full reload is intentional: React.lazy caches a rejected dynamic-import Promise, so merely clearing boundary state would immediately rethrow and falsely advertise recovery.

- [ ] **Step 5: Write Sidebar behavior tests**

Assert:

- the navigation landmark is named “主导航”;
- the active item has `aria-current="page"`;
- collapsed items retain accessible names;
- collapse control name changes between “折叠菜单”和“展开菜单”;
- selecting a menu item calls its exact key.

Run before changing Sidebar:

```powershell
npx.cmd vitest run src/components/Sidebar.test.tsx
```

Expected: FAIL because the current navigation landmark has no accessible name.

- [ ] **Step 6: Tokenize global CSS**

Replace the current root variables with the exact light/dark variables from `UI设计Token.md`, including typography, spacing, radii, shadows, timing, z-index, control height and content width.

Required CSS structure:

```css
:root,
[data-theme="light"] {
  color-scheme: light;
  --color-canvas: #f2f2f7;
  --color-surface: #ffffff;
  --color-text-primary: #1c1c1e;
  --color-accent: #007aff;
  --color-control-primary: #0066cc;
  --color-on-accent: #ffffff;
  --control-height: 44px;
  --radius-card: 16px;
  --duration-normal: 240ms;
  --ease-standard: cubic-bezier(0.25, 0.1, 0.25, 1);
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    color-scheme: dark;
    --color-canvas: #000000;
    --color-surface: #1c1c1e;
    --color-text-primary: #f5f5f7;
    --color-accent: #0a84ff;
    --color-control-primary: #0a84ff;
    --color-on-accent: #000000;
  }
}

[data-theme="dark"] {
  color-scheme: dark;
  --color-canvas: #000000;
  --color-surface: #1c1c1e;
  --color-text-primary: #f5f5f7;
  --color-accent: #0a84ff;
  --color-control-primary: #0a84ff;
  --color-on-accent: #000000;
}
```

Carry every value from the Token document; do not create page-specific color variables.

- [ ] **Step 7: Refactor Sidebar and AppShell**

- Replace Tailwind color/gradient strings with stable semantic classes.
- Brand mark uses one accent color and the existing `Sparkles` icon.
- Navigation remains an `<aside>` containing `<nav aria-label="主导航">`.
- Page title icon remains decorative with `aria-hidden`.
- Static cards no longer raise on hover.
- Provide `.card`, `.card--grouped`, `.card--glass`, `.card--interactive`, and `.card--elevated`; only the interactive variant responds to hover/active.
- Content max width becomes 1040px; grouped layout uses whitespace instead of title underline decoration.

- [ ] **Step 8: Update boot screen**

Use the same light/dark system colors and font stack in `index.html`. Remove the rocket emoji. Render the real existing asset `src-tauri/icons/32x32.png` with alt text “Claude Launcher”; do not draw a replacement logo with CSS or inline SVG.

- [ ] **Step 9: Verify and static-audit**

```powershell
npx.cmd vitest run src/App.test.tsx src/components/PageErrorBoundary.test.tsx src/components/Sidebar.test.tsx
npm.cmd test
npm.cmd run build
rg -n 'from-blue|to-indigo|bg-gradient|linear-gradient' src index.html
```

Expected:

- tests/build pass;
- gradient search has no matches;
- no `console.error("加载配置失败"` branch remains.

完成视觉批次 A：按“批次视觉回归协议”保存 AppShell/Sidebar、启动 Loading/Error 和导航状态对比；Task 1 primitives 的首次实际消费一并验收。

- [ ] **Step 10: Update documents and commit**

```powershell
git status --short
git diff --name-only
git add -- index.html src/App.tsx src/App.test.tsx src/components/PageErrorBoundary.tsx src/components/PageErrorBoundary.test.tsx src/components/Sidebar.tsx src/components/Sidebar.test.tsx src/styles.css docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI设计Token.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-02/README.md docs/ui-refactor/evidence/after/task-02/appshell-default-light-1280x800.png docs/ui-refactor/evidence/after/task-02/appshell-default-dark-1280x800.png docs/ui-refactor/evidence/after/task-02/appshell-narrow-light-560x800.png docs/ui-refactor/evidence/after/task-02/startup-loading-light-1280x800.png docs/ui-refactor/evidence/after/task-02/startup-error-light-1280x800.png docs/ui-refactor/evidence/after/task-02/navigation-focus-light-1280x800.png
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): rebuild the application shell"
```

---

### Task 3: Harden Dashboard, Launch and CLIProxy workflows

**Files:**

- Create: `src/pages/LaunchPage.test.tsx`
- Create: `src/pages/ProxyPage.test.tsx`
- Create: `src/pages/DashboardPage.test.tsx`
- Modify: `src/pages/DashboardPage.tsx`
- Modify: `src/pages/LaunchPage.tsx`
- Modify: `src/pages/ProxyPage.tsx`
- Modify: `src/styles.css`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Create: `docs/ui-refactor/evidence/after/task-03/README.md` and the eight fixed-name PNGs in Step 9

**Interfaces:**

- Consumes: `Button`, `StatusBanner`, `AsyncState`, `FormField`.
- IPC command names and payloads remain exactly:

```ts
invoke("select_directory");
invoke("get_recent_dirs");
invoke("set_work_dir", { dir });
invoke("add_recent_dir", { dir });
invoke("remove_recent_dir", { dir });
invoke("launch_claude", { yolo, env });
invoke("cliproxyapi_status");
invoke("select_cli_dir");
invoke("set_cli_dir", { dir });
invoke("start_cliproxyapi");
invoke("stop_cliproxyapi");
```

- [ ] **Step 1: Write LaunchPage RED tests**

Cover six observable behaviors:

1. Pending `launch_claude` disables the action, changes its accessible name to “正在启动 Claude Code”, and a second click cannot invoke again.
2. Failure renders a `role="alert"` with a recovery-capable action area; the button becomes enabled again.
3. Clicking a recent directory calls `set_work_dir` then `add_recent_dir` with the exact directory, while its delete control does not select the row.
4. `select_directory` resolving to the baseline cancellation sentinel `""` is a no-op: it does not call `onConfig`, does not reload history and does not render an error.
5. `select_directory` rejection renders a visible “无法选择工作目录” permission/recovery alert and leaves the chooser enabled for retry.
6. When an older deferred `get_recent_dirs` resolves after a newer refresh, the newer list remains visible; the stale response is ignored.

Use a deferred promise for the pending test and a complete `Config` fixture.

- [ ] **Step 2: Run LaunchPage test and verify RED**

```powershell
npx.cmd vitest run src/pages/LaunchPage.test.tsx
```

Expected: FAIL because launch is not locked and recent rows are not native buttons.

- [ ] **Step 3: Implement LaunchPage states**

- Add `launchBusy`, `recentState`, and structured `StatusMessage`.
- Use `Button loading loadingLabel="正在启动 Claude Code"`.
- Make the recent-directory selection an actual button inside each list item; keep `ConfirmButton` as its sibling so nested buttons never occur.
- Replace folder/rocket/cross emoji with lucide `Folder`, `Rocket`, `Trash2`, `ChevronDown`, `ChevronUp`.
- Add associated labels and hints.
- Empty recent history is not shown as an error; load failure displays a compact retry state.
- Guard async history refreshes with a monotonically increasing request id so a stale response cannot replace a newer list; Tauri invoke itself is not cancellable, and the UI must not claim that it cancelled backend work.
- Treat the backend cancellation sentinel `""` as a silent no-op. A rejected chooser is a permission/error state with retry; do not call `set_work_dir`/`add_recent_dir` after `select_directory`, because that backend command already persists the selected directory and history in the baseline contract.

- [ ] **Step 4: Write ProxyPage RED tests**

Assert:

- status refresh rejection renders “无法读取 CLIProxyAPI 状态” rather than only “未知”;
- start pending sets `aria-busy`, prevents stop/refresh duplication, and says “正在启动”;
- failed start restores all valid controls and renders `role="alert"`;
- clearing the directory sends `{ dir: "" }`;
- `select_cli_dir` resolving `""` is a silent no-op with no config mutation;
- a rejected directory chooser renders a recoverable “无法选择 CLIProxyAPI 目录” alert and can be retried.

- [ ] **Step 5: Run ProxyPage test and verify RED**

```powershell
npx.cmd vitest run src/pages/ProxyPage.test.tsx
```

Expected: FAIL on visible error and action-specific busy labels.

- [ ] **Step 6: Write and run Dashboard RED tests**

Assert that:

- unresolved status renders a loading state;
- rejected `cliproxyapi_status` renders “无法读取 CLIProxyAPI 状态” with a “重新检测” button;
- successful retry renders “运行中” or “未运行” from the returned value;
- profile count and work directory remain unchanged.

```powershell
npx.cmd vitest run src/pages/DashboardPage.test.tsx
```

Expected: FAIL because the current Dashboard collapses loading and failure into the same `null` state and has no retry control.

- [ ] **Step 7: Implement ProxyPage and Dashboard**

- Model async action as `"idle" | "refresh" | "start" | "stop" | "pick" | "clear"`.
- Use `AsyncState kind="network"` for failed status and a retry Button.
- Keep `running` and command enablement semantics unchanged.
- Dashboard distinguishes “检测中”, “未运行”, “运行中”, and “检测失败”; provide a retry button for failure.
- Replace status emoji with lucide `CircleCheck`, `CircleX`, `CircleHelp`, `RefreshCw`, `Play`, `Square`.
- Preserve `select_cli_dir` baseline semantics: empty string means cancellation, while rejection is a visible permission/error state. Do not append `set_cli_dir` after a successful selection because the backend chooser already persists it.

- [ ] **Step 8: Verify and document**

```powershell
npx.cmd vitest run src/pages/LaunchPage.test.tsx src/pages/ProxyPage.test.tsx src/pages/DashboardPage.test.tsx
npm.cmd test
npm.cmd run build
```

Expected: all pass.

完成视觉批次 B：保存 Dashboard、Launch、CLIProxy 的默认、Loading、Error、Submitting 对比；在 560px 验证目录行和操作按钮无溢出。

- [ ] **Step 9: Commit**

```powershell
git status --short
git diff --name-only
git add -- src/pages/DashboardPage.tsx src/pages/DashboardPage.test.tsx src/pages/LaunchPage.tsx src/pages/LaunchPage.test.tsx src/pages/ProxyPage.tsx src/pages/ProxyPage.test.tsx src/styles.css docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-03/README.md docs/ui-refactor/evidence/after/task-03/dashboard-default-light-1280x800.png docs/ui-refactor/evidence/after/task-03/dashboard-error-light-1280x800.png docs/ui-refactor/evidence/after/task-03/launch-default-light-1280x800.png docs/ui-refactor/evidence/after/task-03/launch-submitting-light-1280x800.png docs/ui-refactor/evidence/after/task-03/launch-error-light-560x800.png docs/ui-refactor/evidence/after/task-03/cliproxy-default-light-1280x800.png docs/ui-refactor/evidence/after/task-03/cliproxy-loading-light-1280x800.png docs/ui-refactor/evidence/after/task-03/cliproxy-error-light-560x800.png
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): harden launcher and proxy states"
```

---

### Task 4: Make configuration forms safe, labeled and submission-aware

**Files:**

- Create: `src/components/ConfirmButton.test.tsx`
- Create: `src/components/EnvValueInput.test.tsx`
- Create: `src/pages/ConfigPage.test.tsx`
- Modify: `src/components/ConfirmButton.tsx`
- Modify: `src/components/EnvValueInput.tsx`
- Modify: `src/pages/ConfigPage.tsx`
- Modify: `src/styles.css`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Create: `docs/ui-refactor/evidence/after/task-04/README.md` and the five fixed-name PNGs in Step 7

**Interfaces:**

- Consumes: `Button`, `StatusBanner`, `FormField`.
- Produces an extended secret input:

```ts
export interface EnvValueInputProps {
  id: string;
  value: string;
  secret: boolean;
  label: string;
  placeholder?: string;
  describedBy?: string;
  onChange: (value: string) => void;
}
```

- `set_config` and `set_profiles` payloads remain byte-for-byte compatible in field names and normalized values.

- [ ] **Step 1: Write ConfirmButton and secret input RED tests**

ConfirmButton:

- defaults to `type="button"`;
- first click changes its accessible name to “再次确认删除”;
- second click within three seconds invokes once;
- timeout restores the original name;
- `aria-live` announces the armed state.

EnvValueInput:

- secret values default to `type="password"`;
- label is available through the supplied accessible name;
- show/hide toggles `aria-pressed` without changing `value`.

- [ ] **Step 2: Run component tests and verify RED**

```powershell
npx.cmd vitest run src/components/ConfirmButton.test.tsx src/components/EnvValueInput.test.tsx
```

Expected: FAIL on button type/announcement and missing accessible input label.

- [ ] **Step 3: Implement the component contracts**

Use lucide `Eye` / `EyeOff`; preserve `e.stopPropagation()` and the three-second confirmation window. Never write a masked placeholder back into `value`.

- [ ] **Step 4: Write ConfigPage RED tests**

Cover:

1. `set_profiles` receives `fromEdit(profiles)` with empty keys removed and real secret values unchanged.
2. Saving profiles or globals shows an action-specific loading label and blocks repeat clicks.
3. `set_config` receives exact fields:

```ts
{
  url,
  key: apiKey,
  cliproxyKey: config.cliproxyapi_key,
  yoloMode: yolo,
  compactWindow: Math.max(0, Math.round(compactWindow)),
  compactPct: Math.max(0, Math.min(100, compactPct)),
  cliproxyapiDir: config.cliproxyapi_dir,
}
```

4. Failure renders an alert and leaves unsaved indicators present.
5. Inputs can be found by their visible labels.
6. A percentage outside 0–100 is marked `aria-invalid` with a visible message while the existing save normalization still clamps to 0–100.
7. `config_path` rejection renders “无法读取配置文件位置” with a retry action instead of silently displaying an empty path.
8. Retrying `config_path` after a permission failure invokes it once more and replaces the error with the recovered path; pending retry cannot be duplicated.

- [ ] **Step 5: Run ConfigPage test and verify RED**

```powershell
npx.cmd vitest run src/pages/ConfigPage.test.tsx
```

Expected: FAIL because saves have no pending lock and labels are not associated.

- [ ] **Step 6: Refactor ConfigPage**

- Use separate `profileSaveBusy` and `globalSaveBusy`.
- Use structured messages, `FormField`, semantic fieldset/legend for profiles and global settings.
- Keep `url` and `apiKey` pass-through even though they have no visible editors.
- Replace warning emoji with `TriangleAlert`.
- Give each dynamic field deterministic ids based on profile/row indices and meaningful accessible labels.
- Preserve dirty comparison and cross-menu draft ownership.
- Migrate the Config call site away from legacy `MessageBanner`; NVIDIA remains the only temporary legacy call site until Task 5.
- Give `config_path` its own loading/ready/error state. Permission rejection is visible and recoverable; retry is action-locked and does not alter either configuration draft.

- [ ] **Step 7: Verify and commit**

```powershell
npx.cmd vitest run src/components/ConfirmButton.test.tsx src/components/EnvValueInput.test.tsx src/pages/ConfigPage.test.tsx
npm.cmd test
npm.cmd run build
rg -n 'MessageBanner' src/pages/ConfigPage.tsx
```

Expected: tests/build pass and the ConfigPage search has no matches.

完成视觉批次 C：保存 Config 默认、Dirty、校验失败、Submitting、Error 以及 secret 显隐状态的 Light/Dark 对比。

```powershell
git status --short
git diff --name-only
git add -- src/components/ConfirmButton.tsx src/components/ConfirmButton.test.tsx src/components/EnvValueInput.tsx src/components/EnvValueInput.test.tsx src/pages/ConfigPage.tsx src/pages/ConfigPage.test.tsx src/styles.css docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-04/README.md docs/ui-refactor/evidence/after/task-04/config-default-light-1280x800.png docs/ui-refactor/evidence/after/task-04/config-dirty-dark-1280x800.png docs/ui-refactor/evidence/after/task-04/config-validation-light-1280x800.png docs/ui-refactor/evidence/after/task-04/config-submitting-dark-1280x800.png docs/ui-refactor/evidence/after/task-04/config-error-light-1280x800.png
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): make configuration forms resilient"
```

---

### Task 5: Refactor NVIDIA without changing hot-update semantics

**Files:**

- Modify: `src/pages/NvidiaPage.tsx`
- Modify: `src/pages/NvidiaPage.test.tsx`
- Modify: `src/pages/nvidia/NvidiaStatusCard.tsx`
- Modify: `src/pages/nvidia/NvidiaConfigForm.tsx`
- Modify: `src/pages/nvidia/ModelPriorityEditor.tsx`
- Modify: `src/pages/nvidia/KeyPoolCard.tsx`
- Modify: `src/pages/nvidia/NvidiaTestPanel.tsx`
- Create: `src/components/ui/SecretTextarea.tsx`
- Create: `src/components/ui/SecretTextarea.test.tsx`
- Delete: `src/components/MessageBanner.tsx`
- Modify: `src/styles.css`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Create: `docs/ui-refactor/evidence/after/task-05/README.md` and the seven fixed-name PNGs in Step 5

**Interfaces:**

- Consumes: `Button`, `StatusBanner`, `AsyncState`, `FormField`, `EnvValueInput`, `ConfirmButton`.
- Preserve every existing NVIDIA invoke command and payload.
- Model list order remains highest priority first; add/move/top/delete still call `nvidia_set_models` immediately.

- [ ] **Step 1: Add NVIDIA and SecretTextarea RED tests**

Extend `NvidiaPage.test.tsx` with:

- Render through a stateful parent harness that owns `config` and implements `onConfig={setConfig}`; edit Base URL to `https://example.test/v1`, then move a model. The input must still contain that unsaved value after `nvidia_set_models` resolves and the parent rerenders. A bare `onConfig={vi.fn()}` is insufficient because it cannot reproduce the current overwrite bug.
- The command receives the exact reordered model array.
- Pending start says “正在启动代理”, is busy, and calls `set_nvidia_config` before `nvidia_start`.
- API Keys stay in one multiline textarea whose visual content is masked by default; auth token stays a password input. Both can be revealed without value mutation.
- Port `9090` renders “本地测试 9090” and URLs containing `:9090`.
- Model controls have names including model and action, such as “置顶 model-b”.
- External host with an auth token shorter than 24 characters is marked `aria-invalid` and reports the same security requirement as the backend, but ordinary Save still calls `set_nvidia_config` with the exact draft.
- Starting that invalid external-host draft still calls `set_nvidia_config` before `nvidia_start`; a mocked backend rejection is rendered visibly. The UI must not pre-emptively block Save or Start and thereby change the baseline persistence/call-order contract.
- A rejected `nvidia_status` shows a retry control. If an older deferred status resolves after a newer manual refresh, it cannot overwrite the newer status.
- A rejected `nvidia_key_pool` renders a recoverable error rather than an empty pool. Fake-timer tests prove exactly one 2-second interval while running, cleanup on unmount, and no state application from a deferred result after unmount.

Create `SecretTextarea.test.tsx` and assert the textarea keeps a multiline literal value, is labeled “NVIDIA API Keys”, starts with class `secret-textarea--masked`, and toggling the unique “显示 NVIDIA API Keys” button changes `aria-pressed` and the mask class without changing the value.

- [ ] **Step 2: Run NVIDIA tests and verify RED**

```powershell
npx.cmd vitest run src/components/ui/SecretTextarea.test.tsx src/pages/NvidiaPage.test.tsx
```

Expected: FAIL on draft preservation, secret masking contract, dynamic title and accessible action names.

- [ ] **Step 3: Isolate draft synchronization**

Implement a single draft object:

```ts
interface NvidiaDraft {
  keysText: string;
  models: string[];
  baseUrl: string;
  host: string;
  port: number;
  cooldown: number;
  retries: number;
  timeout: number;
  authToken: string;
}
```

Rules:

- Full draft initializes from a newly loaded backend config.
- Local model hot updates patch only `models`; they do not replace other draft fields.
- `onConfig` receives the committed model list so Launch page sees it.
- `collectNvidiaConfig(draft)` performs the existing split/trim/clamp logic exactly.
- start and test continue to save the entire current draft before invoking their action.
- Status refreshes use a monotonically increasing request id; status and Key-pool async completions check a disposed/generation guard before applying state.

- [ ] **Step 4: Refactor NVIDIA presentation**

- Use action-specific busy states for start/stop/refresh/save/test.
- Status failures show network error + retry.
- Key-pool polling failure never claims the pool is empty; show a compact recoverable state.
- Implement `SecretTextarea` as a native multiline `<textarea>` plus a 44px show/hide Button. The masked state uses class `secret-textarea--masked` with `-webkit-text-security: disc`; the textarea keeps its real multiline value, accessible label, selection and change behavior. The reveal control uses `aria-pressed`. Use `EnvValueInput` for the single-line auth token.
- Use lucide icons and semantic classes; remove inline colors/backgrounds/padding/radii.
- Keep the external-host warning text and backend security meaning unchanged.
- Real-time validation is advisory for the baseline external-host/token rule: it sets associated error text and `aria-invalid`, but does not block ordinary Save. Start still saves first and delegates final rejection to `nvidia_start`; test still preserves its existing save-then-test sequence.
- Make test panel segmented terminal choices buttons with `aria-pressed`.
- Migrate the final legacy `MessageBanner` call site to `StatusBanner`, delete `src/components/MessageBanner.tsx`, and verify no emoji-prefix inference remains.

- [ ] **Step 5: Verify and commit**

```powershell
npx.cmd vitest run src/components/ui/SecretTextarea.test.tsx src/pages/NvidiaPage.test.tsx
npm.cmd test
npm.cmd run build
rg -n 'MessageBanner|startsWith' src
```

Expected: tests/build pass and the search has no matches.

完成视觉批次 D：保存 NVIDIA 默认、Status/Key pool Error、Submitting、secret 隐藏、外部 host 警告，以及 560px 窄屏对比。

```powershell
git status --short
git diff --name-only
git add -- src/components/MessageBanner.tsx src/components/ui/SecretTextarea.tsx src/components/ui/SecretTextarea.test.tsx src/pages/NvidiaPage.tsx src/pages/NvidiaPage.test.tsx src/pages/nvidia/NvidiaStatusCard.tsx src/pages/nvidia/NvidiaConfigForm.tsx src/pages/nvidia/ModelPriorityEditor.tsx src/pages/nvidia/KeyPoolCard.tsx src/pages/nvidia/NvidiaTestPanel.tsx src/styles.css docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-05/README.md docs/ui-refactor/evidence/after/task-05/nvidia-default-light-1280x800.png docs/ui-refactor/evidence/after/task-05/nvidia-status-error-dark-1280x800.png docs/ui-refactor/evidence/after/task-05/nvidia-key-pool-error-light-1280x800.png docs/ui-refactor/evidence/after/task-05/nvidia-submitting-dark-1280x800.png docs/ui-refactor/evidence/after/task-05/nvidia-secret-hidden-light-1280x800.png docs/ui-refactor/evidence/after/task-05/nvidia-external-host-warning-light-1280x800.png docs/ui-refactor/evidence/after/task-05/nvidia-narrow-dark-560x800.png
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): preserve nvidia drafts and states"
```

---

### Task 6: Build an accessible log viewer and Dialog

**Files:**

- Create: `src/components/ui/Dialog.tsx`
- Create: `src/components/ui/Dialog.test.tsx`
- Modify: `src/pages/LogPage.tsx`
- Modify: `src/pages/LogPage.test.tsx`
- Modify: `src/styles.css`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Create: `docs/ui-refactor/evidence/after/task-06/README.md` and the four fixed-name PNGs in Step 7

**Interfaces:**

- Produces:

```ts
export interface DialogProps {
  open: boolean;
  title: string;
  description?: string;
  onClose: () => void;
  closeOnBackdrop?: boolean;
  children: React.ReactNode;
}
```

- Log commands/event names remain unchanged.

- [ ] **Step 1: Write Dialog RED tests**

Assert:

- `role="dialog"`, `aria-modal="true"` and accessible title;
- initial focus moves to close button;
- Escape calls `onClose`;
- Tab and Shift+Tab stay within the dialog;
- closing restores focus to the trigger.

- [ ] **Step 2: Run Dialog test and verify RED**

```powershell
npx.cmd vitest run src/components/ui/Dialog.test.tsx
```

Expected: FAIL because Dialog does not exist.

- [ ] **Step 3: Implement Dialog**

Use a ref to the dialog panel, store `document.activeElement` on open, query only focusable native controls inside the panel, and restore focus during cleanup. Backdrop click closes only when `closeOnBackdrop` is true; panel click never bubbles to close.

- [ ] **Step 4: Extend LogPage RED tests**

Cover:

- a history file is a button and Enter opens an accessible dialog;
- Escape closes and focus returns to that history button;
- `list_log_files` failure displays “无法读取历史日志” with retry, not “暂无文件”;
- `get_logs` failure displays a visible warning while the event listener can still append future lines;
- `clear_logs` failure renders alert and preserves displayed lines;
- clear still never calls any disk-delete command.
- if the component unmounts before the asynchronous `listen()` registration resolves, the eventual `UnlistenFn` is called immediately instead of leaking the listener;
- if a live event arrives before a slow `get_logs` snapshot resolves, resolving the snapshot preserves that live line and avoids duplicating an identical trailing line.

- [ ] **Step 5: Run LogPage test and verify RED**

```powershell
npx.cmd vitest run src/pages/LogPage.test.tsx
```

Expected: FAIL on native button/dialog/error behavior.

- [ ] **Step 6: Refactor LogPage**

- Use explicit load states for live buffer, log level and file list.
- Replace inline dark viewer and modal styles with `.log-viewer`, `.log-file-button`, and Dialog classes.
- Keep 2,000-line truncation and auto-scroll behavior.
- Track `disposed` during listener registration; if `listen()` resolves after cleanup, invoke the returned unlisten function immediately.
- Merge the initial snapshot with live lines received after mount in timestamp/order of arrival, deduplicating only exact adjacent duplicates; never replace the live buffer wholesale with a late snapshot.
- Add `aria-live="polite"` only to status summaries, not the high-frequency entire log content.
- Use `RefreshCw`, `Trash2`, `FileText`, `ScrollText`.

- [ ] **Step 7: Verify and commit**

```powershell
npx.cmd vitest run src/components/ui/Dialog.test.tsx src/pages/LogPage.test.tsx
npm.cmd test
npm.cmd run build
```

Expected: pass.

完成视觉批次 E：保存实时/历史 Empty 与 Error、历史 Dialog、键盘焦点恢复，以及 Light/Dark 和 560px 对比。

```powershell
git status --short
git diff --name-only
git add -- src/components/ui/Dialog.tsx src/components/ui/Dialog.test.tsx src/pages/LogPage.tsx src/pages/LogPage.test.tsx src/styles.css docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-06/README.md docs/ui-refactor/evidence/after/task-06/logs-live-empty-light-1280x800.png docs/ui-refactor/evidence/after/task-06/logs-history-error-dark-1280x800.png docs/ui-refactor/evidence/after/task-06/logs-dialog-light-1280x800.png docs/ui-refactor/evidence/after/task-06/logs-dialog-focus-dark-560x800.png
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): make log viewing keyboard accessible"
```

---

### Task 7: Make Dictionary loading, tabs and import errors recoverable

**Files:**

- Modify: `src/pages/DictionaryPage.tsx`
- Modify: `src/pages/DictionaryPage.test.tsx`
- Modify: `src/styles.css`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Create: `docs/ui-refactor/evidence/after/task-07/README.md` and the seven fixed-name PNGs in Step 5

**Interfaces:**

- Consumes: `Button`, `StatusBanner`, `AsyncState`, `ConfirmButton`.
- Preserve localStorage keys:

```ts
const LS_IMPORTED = "claude-launcher:dict:imported";
const knownKey = (id: string) => `claude-launcher:dict:known:${id}`;
```

- Preserve imported JSON shapes and existing browse/study/shortcut semantics.

- [ ] **Step 1: Add Dictionary RED tests**

Cover:

1. Built-in dictionary load rejection renders “词典加载失败” and a retry button; it never remains at “词典加载中”.
2. The selector is a tablist; each dictionary is a tab with `aria-selected`.
3. ArrowRight and ArrowLeft move selection, Home selects the first, End selects the last.
4. Removing an imported dictionary is a separate button and never creates nested interactive elements.
5. Invalid JSON displays an in-app alert containing the filename and format guidance; `window.alert` is not called.
6. Existing `onlyUnknown` behavior remains covered.

- [ ] **Step 2: Run Dictionary test and verify RED**

```powershell
npx.cmd vitest run src/pages/DictionaryPage.test.tsx
```

Expected: FAIL on load error, tabs and import alert behavior.

- [ ] **Step 3: Implement dictionary async state and tabs**

- Track per-active-id load state as loading/ready/error.
- Retry calls the same registry loader without changing ids or cache behavior.
- Use `role="tablist"` and native `button role="tab"` with roving `tabIndex`.
- Keep the imported dictionary delete button outside the tab button in a shared wrapper.
- On keyboard selection, reset the same state that mouse selection resets.

- [ ] **Step 4: Replace native alert and refine states**

- Store import error as `StatusMessage`.
- Include a concise valid shape:

```json
{
  "name": "我的词典",
  "words": [{ "en": "Apple", "zh": "苹果", "category": "水果" }]
}
```

- Search-empty, study-complete and no-unmastered remain distinct empty/success states.
- Replace celebratory/status emoji with lucide icons where the icon conveys function; memory/source text may retain its literal content marker only if accompanying text remains complete.

- [ ] **Step 5: Verify and commit**

```powershell
npx.cmd vitest run src/pages/DictionaryPage.test.tsx
npm.cmd test
npm.cmd run build
```

Expected: pass.

完成视觉批次 F：保存 Dictionary Loading/Error、tab、搜索 Empty、学习完成、导入 Error，以及 Light/Dark 和 560px 对比。

```powershell
git status --short
git diff --name-only
git add -- src/pages/DictionaryPage.tsx src/pages/DictionaryPage.test.tsx src/styles.css docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-07/README.md docs/ui-refactor/evidence/after/task-07/dictionary-loading-light-1280x800.png docs/ui-refactor/evidence/after/task-07/dictionary-error-dark-1280x800.png docs/ui-refactor/evidence/after/task-07/dictionary-tabs-light-1280x800.png docs/ui-refactor/evidence/after/task-07/dictionary-search-empty-dark-1280x800.png docs/ui-refactor/evidence/after/task-07/dictionary-study-complete-light-1280x800.png docs/ui-refactor/evidence/after/task-07/dictionary-import-error-light-1280x800.png docs/ui-refactor/evidence/after/task-07/dictionary-narrow-dark-560x800.png
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): make dictionary states recoverable"
```

---

### Task 8: Complete responsive, dark-mode and interaction-state migration

**Files:**

- Modify: `src/pages/AboutPage.tsx`
- Modify: `src/styles.css`
- Modify: `src/tailwind.css`
- Modify: `docs/ui-refactor/UI设计Token.md`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Modify: `docs/ui-refactor/UI验收矩阵.md`
- Create: `docs/ui-refactor/evidence/after/task-08/README.md` and the 24 fixed-name PNGs in Step 8

**Interfaces:**

- No business or IPC interface changes.
- All pages consume the finalized token variables from Task 2.
- Presentation leaks in files owned by Tasks 2–7 return to that task's implementer/review loop before Task 8 begins; Task 8 itself changes only the explicit files listed above.

- [ ] **Step 1: Inventory remaining presentation leaks**

Run:

```powershell
rg -n 'style=\{\{|#[0-9a-fA-F]{3,8}|rgba?\(|linear-gradient|bg-gradient|from-(blue|indigo|purple)|to-(blue|indigo|purple)' src index.html
rg -n '🚀|✅|❌|⚠|🔄|▶|⏹|📁|📄|🧹|🔑|🧪|👁|🙈' src index.html
```

Classify every match:

- CSS Token definition: allowed.
- Literal content with full text meaning: document the exception.
- Component presentation: migrate.

- [ ] **Step 2: Migrate the remaining About/global presentation**

- Move remaining About/global layout, color, typography, padding and radius styles into semantic CSS classes.
- Inline values derived from runtime data, such as progress width, remain inline.
- Replace functional emoji with lucide icons; every icon-only button receives a unique accessible name.
- If Step 1 reports a leak in another page/component, do not edit it here; reopen that file's owning Task 2–7 review loop so its focused tests and report remain authoritative.

- [ ] **Step 3: Audit high-frequency work before adding rate limits**

Inspect search/filter, resize, scroll, polling and event listeners:

- Dictionary search is an in-memory `useMemo` over the active dictionary; keep it immediate unless measured input latency requires a 150ms debounce.
- NVIDIA Key-pool polling remains exactly one 2-second interval while the page is mounted and running.
- Log auto-scroll performs one scroll update per committed line batch and unregisters its listener on unmount.
- No window resize listener is added; responsive behavior stays in CSS.

Record the decision and evidence in `UI重构记录.md`. Do not add decorative debounce/throttle wrappers where there is no high-frequency external side effect.

- [ ] **Step 4: Implement responsive contracts**

Add:

```css
@media (max-width: 1024px) {
  .dashboard-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}

@media (max-width: 768px) {
  .content { padding: var(--space-5); }
  .dashboard-grid { grid-template-columns: 1fr; }
  .input-row,
  .status-buttons,
  .logbar-controls { flex-wrap: wrap; }
}

@media (max-width: 560px) {
  .content { padding: var(--space-4); }
  .input-row,
  .stack-actions-mobile { flex-direction: column; align-items: stretch; }
  .stack-actions-mobile > .ui-button { width: 100%; }
  .dialog-panel { width: calc(100vw - 24px); max-height: calc(100vh - 24px); }
}
```

Apply `.stack-actions-mobile` only to page-level action groups that are intentionally vertical at 560px. Dialog close buttons, StatusBanner actions, secret reveal controls, segmented controls and icon buttons keep their 44px hit area and intrinsic width. Add page-specific rules only when they express layout, never new color or motion values.

- [ ] **Step 5: Complete global interaction states**

- `button`, `a`, `input`, `select`, `textarea`, `[role="tab"]` use the same `:focus-visible` ring.
- All controls have Hover, Active, Disabled and Focus without relying on color alone.
- Minimum interactive size is 44px; dense icon controls use padding/hit-area instead of enlarging glyphs.
- `@media (prefers-reduced-motion: reduce)` disables transform and animation.
- `@media (prefers-reduced-transparency: reduce)` replaces glass with opaque surface in both themes.

- [ ] **Step 6: Verify static acceptance gates**

```powershell
npm.cmd test
npm.cmd run build
rg -n 'linear-gradient|bg-gradient|from-(blue|indigo|purple)|to-(blue|indigo|purple)' src index.html
rg -n 'Lorem Ipsum|Item 1' src index.html
rg -n 'animation:.*infinite' src/styles.css
```

Expected:

- tests/build pass;
- no gradient, placeholder copy or infinite animation matches;
- any remaining inline style match is limited to runtime-derived values and recorded.

完成视觉批次 G：按最终矩阵保存全部 8 页在 1280/768/560、Light/Dark 与 reduced-motion 下的对比；逐项检查无横向滚动、遮挡或误撑满的 inline/icon control。

- [ ] **Step 7: Update Token and audit documents**

Change Token version to `1.0（已落地）`; record any deviations with the exact component and rationale. Mark V-01 through V-13 only when their matching evidence exists.

- [ ] **Step 8: Commit**

```powershell
git status --short
git diff --name-only
git add -- src/pages/AboutPage.tsx src/styles.css src/tailwind.css docs/ui-refactor/UI设计Token.md docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md docs/ui-refactor/evidence/after/task-08/README.md docs/ui-refactor/evidence/after/task-08/final-dashboard-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-dashboard-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-launch-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-launch-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-cliproxy-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-cliproxy-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-nvidia-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-nvidia-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-logs-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-logs-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-config-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-config-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-dictionary-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-dictionary-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-about-light-1280x800.png docs/ui-refactor/evidence/after/task-08/final-about-dark-1280x800.png docs/ui-refactor/evidence/after/task-08/final-navigation-light-768x800.png docs/ui-refactor/evidence/after/task-08/final-config-light-768x800.png docs/ui-refactor/evidence/after/task-08/final-logs-dark-768x800.png docs/ui-refactor/evidence/after/task-08/final-dictionary-dark-768x800.png docs/ui-refactor/evidence/after/task-08/final-navigation-dark-560x800.png docs/ui-refactor/evidence/after/task-08/final-nvidia-dark-560x800.png docs/ui-refactor/evidence/after/task-08/final-logs-dialog-dark-560x800.png docs/ui-refactor/evidence/after/task-08/final-dictionary-flashcard-dark-560x800.png
git diff --cached --name-only
git diff --cached --check
git commit -m "refactor(ui): finish responsive theme migration"
```

---

### Task 9: Full regression, review and delivery

**Files:**

- Create: `docs/ui-refactor/UI重构总结.md`
- Modify: `docs/ui-refactor/UI重构记录.md`
- Modify: `docs/ui-refactor/UI审计清单.md`
- Modify: `docs/ui-refactor/UI验收矩阵.md`
- Modify: `docs/ui-refactor/evidence/README.md`
- Modify: any frontend file required by final review findings

**Interfaces:**

- Consumes every prior task output.
- Produces final evidence mapped one-to-one to `UI重构Goal.md` completion conditions.

- [ ] **Step 1: Run the complete automated gate**

```powershell
npm.cmd test
npx.cmd tsc --noEmit
npm.cmd run build
git diff --check 50808c5..HEAD
git diff --check
git diff --cached --check
git diff --cached --name-only
```

Record exact test counts, duration, build asset summary and warnings.

Also compare final build asset sizes with the baseline recorded in `UI重构记录.md`; investigate unexpected main-bundle growth greater than 10%.

- [ ] **Step 2: Confirm backend and contract boundaries**

```powershell
git diff --name-only 50808c5..HEAD -- src-tauri
git diff --name-only -- src-tauri
git diff --cached --name-only -- src-tauri
git diff -U3 50808c5..HEAD -- src/providerEnv.ts src/types.ts
git diff -U3 -- src/providerEnv.ts src/types.ts
git diff --cached -U3 -- src/providerEnv.ts src/types.ts
git diff -U3 50808c5..HEAD -- src/App.tsx src/pages src/components
git diff -U3 -- src/App.tsx src/pages src/components
git diff --cached -U3 -- src/App.tsx src/pages src/components
rg -n 'invoke<|invoke\(' src/App.tsx src/pages src/components
rg -n 'listen<' src/App.tsx src/pages src/components
Get-Content -LiteralPath 'docs/ui-refactor/evidence/baseline-ipc.md' -Encoding UTF8
```

Expected:

- no committed or uncommitted `src-tauri` modifications;
- `providerEnv.ts` and shared Rust-aligned types unchanged unless a reviewed front-end-only type addition is present;
- compare the committed diff, unstaged diff, staged diff and current static inventory
  against `evidence/baseline-ipc.md` row by row;
- all 29 baseline `invoke` commands and the `nvidia-log` listener retain their exact
  command/event names, payload keys and documented critical call order;
- any added/removed command, payload-key change, reordered critical sequence, polling
  change or listener lifecycle change is a contract difference and must be explained;
  stop for explicit user confirmation unless it is a tested restoration of baseline
  behavior.

- [ ] **Step 3: Run functional regression scenarios**

With Tauri dev when the environment permits, record actual/expected for:

1. startup config load and retry failure;
2. menu navigation and draft preservation;
3. working-directory selection/history/delete;
4. Claude launch success/failure/pending;
5. CLIProxy refresh/start/stop/directory;
6. Config profile/global save, failure and dirty state;
7. NVIDIA save/start/stop/test/model hot update/Key pool;
8. logs live append/filter/clear/history dialog;
9. dictionary browse/import/study/keyboard/progress;
10. About page.

Use only non-sensitive fixtures and never invoke a destructive external action.

During the same pass, inspect the developer console for new errors/warnings, verify that one user action produces one IPC request unless polling is explicitly documented, and record visible layout shifts or scroll jumps.

Update `UI验收矩阵.md` row by row. Every Goal requirement must point to a test, command output, screenshot or an exact N/A reason. In particular:

- About page has no async request, so request loading/error/cancel is N/A while its responsive/theme/keyboard evidence remains required.
- Tauri invoke is not cancellable; pages with overlapping requests must ignore stale responses and listener/polling cleanup must be directly tested.
- Permission rejection from directory dialogs/config access must produce a visible recoverable error; user cancellation of a directory dialog remains a no-op, not an error.
- No page may receive a global “all states complete” claim from the existence of the generic `AsyncState` component alone.

- [ ] **Step 4: Run visual and accessibility matrix**

Required states:

| Theme | Width | Pages/states |
|---|---:|---|
| Light | 1280 | all 8 pages; loading/empty/error/disabled/submitting |
| Dark | 1280 | all 8 pages; modal/code/input/status |
| Light | 768 | navigation, forms, logs, dictionary |
| Dark | 560 | navigation, NVIDIA, logs modal, dictionary flashcard |
| Either | keyboard only | navigation, forms, log dialog, dictionary tabs |
| Either | reduced motion | pending, dialog, confirm, dictionary answer |

Save screenshots with the naming convention in `evidence/README.md`. If browser/Tauri visual access remains blocked, mark each screenshot requirement “受阻” with the exact policy/tool evidence and do not claim visual completion.

- [ ] **Step 5: Request whole-branch code review**

Use the `superpowers:requesting-code-review` workflow over `50808c5..HEAD` plus any uncommitted diff. The review must check:

- business/IPC behavior preservation;
- async race and duplicate submission;
- accessibility and focus;
- secret handling;
- responsive/theme token consistency;
- test quality and missing goal requirements.

Fix all Critical/Important findings, rerun their focused tests, and perform one scoped re-review.

- [ ] **Step 5b: Commit final-review fixes without broad staging**

If Step 5 changes any production or test file, make a dedicated `fix(ui): address final review findings` commit before producing the final summary:

1. Run `git status --short` and `git diff --name-only`.
2. Copy the exact reviewed frontend/test/doc paths into `UI重构记录.md`.
3. Construct `git add --` with those literal paths only; no directory, glob, variable expansion or `-A`.
4. Run `git diff --cached --name-only` and compare it with the recorded manifest.
5. Run `git diff --cached --check`, the focused tests, `npm.cmd test`, and `npm.cmd run build`.
6. Commit only after the scoped re-review reports no Critical/Important findings.

If no files change, record “review-fix commit N/A” with the review result. Final delivery must not leave review fixes unstaged or uncommitted.

- [ ] **Step 6: Produce final summary**

`UI重构总结.md` must contain:

- before/after description and evidence links;
- changed files grouped by foundation/page/docs;
- core business regression table;
- light/dark/responsive/keyboard/error-state matrix;
- exact automated test/build results;
- known warnings and evidence limitations;
- incomplete items and risk level;
- rollback commits;
- next recommendations.

- [ ] **Step 7: Final completion audit and commit**

Check every checkbox in `UI重构Goal.md` against direct evidence. Unproven items remain incomplete.

```powershell
git status --short
git diff --name-only
git add -- docs/ui-refactor/UI重构总结.md docs/ui-refactor/UI重构记录.md docs/ui-refactor/UI审计清单.md docs/ui-refactor/UI验收矩阵.md docs/ui-refactor/UI重构实施计划.md docs/ui-refactor/evidence/README.md
git diff --cached --name-only
git diff --cached --check
git commit -m "docs(ui): deliver refactor verification summary"
git status --short
git status --short -- docs/ui-refactor
```

Expected: `git status --short -- docs/ui-refactor` is empty, proving every plan, baseline and before/after evidence file is tracked and committed. The repository-level status may still show the user-owned untracked `UI重构Goal.md`; do not stage it unless the user later explicitly asks to include it.
