# 当前项目代码审查报告（双轴：Standards / Spec）

**审查日期：** 2026-08-01
**审查对象：** `rust/`（Tauri v2 + React 实现）
**当前分支：** `feat/rust-implementation`
**本次范围：** UI 重构系列 + 1 笔后端修复，共 6 笔提交（固定点为 `ffe28dc^`）

- `6a69ca7` docs(ui): capture pre-refactor visual baseline
- `3b3f827` refactor(ui): add accessible state primitives
- `54bcc84` refactor(ui): tokenize AppShell, add startup recovery and error boundary (Task 2)
- `7baf4cb` docs(ui): mark Task 2 steps complete in implementation plan
- `0c6a29e` refactor(ui): harden Dashboard, Launch and CLIProxy states (Task 3)
- `fc1b001` fix(claude): strip Windows `\\?\` prefix from launch work dir

一名为「重新全面审核代码」之请求，对 `ffe28dc...HEAD` 范围同时做两项独立审核：**Standards**（是否符合仓库既定的安全/编码约定 + Fowler 嗅味基线）与 **Spec**（是否忠实实现 `UI重构Goal.md` 起源需求）。两轴各由一个并行子代理产出，主路径不合并、不重排两轴结论，仅分别汇总。

> Diff 规模：~95 文件，+2427/−310。最大改动为 `src/styles.css`（+~826）、`src/pages/ProxyPage.tsx`、`src/pages/LaunchPage.tsx`、`src/components/Sidebar.tsx`、新增 `src/components/ui/*.tsx` 基元（AsyncState/Button/FormField/Skeleton/StatusBanner）及其 `.test.tsx`，以及后端 `src-tauri/src/claude.rs`（Windows `\\?\` 前缀剥离）。

---

## Standards

### 硬性违反（已记录的文档标准）

**后端安全闸门无硬性违反。** 安全相关模块中仅 `src-tauri/src/claude.rs` 被改动；`logger.rs`、`nvidia/*`、`config.rs`、`history.rs`、`lib.rs`、`tauri.conf.json` 均未触。逐源码核对 `validate_work_dir` 的安全顺序仍正确:cmd 元字符/`..`/绝对路径/不存在性拒绝发生在 `canonicalize` **之前**，新增的 `\\?\` 剥离发生于**已校验的 canonical 路径之后**（其字符已被校验为安全），故不可能在此重新引入注入向量。

「webview 禁用 fetch/WebSocket/EventSource」约定成立——对全 diff 搜索 `fetch(`/`WebSocket`/`EventSource`/`XMLHttpRequest` 均无命中。CSP 未放松（`tauri.conf.json` 未改）。错误边界的 `window.location.reload()` 是导航而非网络调用，仍受 `script-src 'self'` 约束。

`\\?\` 剥离是正当修复，契合 `validate_work_dir` 记录在案的职责;剥离逻辑本身正确（`\\?\UNC\`→`\\`、`\\?\`→裸路径），并附带回归测试 `validate_work_dir_strips_verbatim_prefix` / `validate_work_dir_rejects_missing_dir`。无已记录标准之破坏。

### 判断性嗅味（基线嗅味，均为判断性，非硬性违规）

| # | 嗅味 | 说明 | 文件/位置 |
| --- | --- | --- | --- |
| S1 | Duplicated Code | `AsyncState` 与 `StatusBanner` 硬编码一致形态的 DOM id 常量;同页两实例会令 `aria-labelledby`/`aria-describedby` 的 id 冲突。改用 `React.useId()` 按实例生成。 | `src/components/ui/AsyncState.tsx:27-28`、`StatusBanner.tsx:28-29` |
| S2 | Speculative Generality | `AsyncState` `kind` 有 5 个变体;页面调用点只用到 `error`/`network`/`loading`，`empty`/`permission` 有图标+aria 接线+单测却无真实消费方。删除或落地消费方。 | `src/components/ui/AsyncState.tsx` |
| S3 | Primitive Obsession / Repeated Switches | `loading \| ready \| error` 三态在每页重新声明为本页局部字符串联合（`ProxyState`/`StatusState`/`ConfigLoadState`）。一份共享 `AsyncStatus<T>` 判别联合（且 `AsyncState` 基元已为此存在）即可折叠重复，亦能消解 `ProxyPage` 的 `BusyAction` 联合。 | `DashboardPage.tsx`、`ProxyPage.tsx:12`、`App.tsx` |
| S4 | Divergent Change | `ProxyPage.tsx` 用一个 `BusyAction` 字符串联合 + 六处 `if (action) return` 守卫并行化六个异步动作（`"refresh"\|"start"\|"stop"\|"pick"\|"clear"`）。新增第七个动作需改联合且改每处守卫。改为 `Busy` 集合或「动作→禁用」映射以局部化变更。 | `src/pages/ProxyPage.tsx` |
| S5 | Mysterious Name | 同一 `loading\|ready\|error` 概念在三文件分别叫 `proxyState`/`statusState`/`cfgState`;`recentFailed`（布尔）又用另一形态影子化他处建模为三态的「加载失败」概念。命名不一致。 | `DashboardPage.tsx`、`ProxyPage.tsx`、`App.tsx` |
| S6 | （冗余噪音） | `Button` 的 `variant={"secondary" as ButtonVariant}` 中 `variant` 已类型化为 `ButtonVariant`，强转多余，删去。 | `src/pages/ProxyPage.tsx` |

无显著 `Middle Man` / `Refused Bequest` / `Message Chains`;`PageErrorBoundary` 是合理的组合包装;`FormField` 的 render-prop `children(describedBy)` 是干净接缝。

---

## Spec

起源规范:`rust/UI重构Goal.md`（「Apple 风格前端 UI 重构 Goal 与实施清单」）。进度跟踪文档位于 `rust/docs/ui-refactor/`（`UI重构实施计划.md`/`UI审计清单.md`/`UI设计Token.md`/`UI验收矩阵.md`/`UI重构记录.md`/`evidence/`）。规范自注个别数值已损坏/冲突（Modal 圆角、字体字号、动效曲线），「执行时以实际平台、现有组件约束和可用性验证为准」——故规范**数值**作引导;但**约束与完整性要求**（无渐变/占位、状态全覆盖、不改业务流程、不引新框架、reduced-motion）**不软化**，从严核查。

### (a) 缺失 / 不完整

1. **状态覆盖仅部分。** 规范:*「必须补齐 Loading、Empty、Error、Disabled、Hover、Focus、Active、提交中、校验失败、无权限和网络失败」*。diff 只触 Dashboard/Launch/Proxy + App 外壳;Config/Logs/NVIDIA/Dictionary/About 仍输出 emoji 状态串（`✅`/`❌`/`❓`/`🔄`/`▶`/`⏹`/`⏳`）并用旧 `MessageBanner`，无 loading/error/retry。`UI验收矩阵.md` G-11/G-15/G-21 已如实标 待实施。
2. **紫蓝渐变移除在 Sidebar 之外尚未完成。** 规范:*「彻底移除紫蓝渐变」*。Sidebar 的 `from-blue-500 to-indigo-500` 已移除（正确），但 `src/styles.css:1473` 仍含 `linear-gradient(90deg, var(--success), #30d158)`。审计清单 G-25（「不再有紫蓝渐变」）仍为 进行中。文档将其界定为「功能性 success-badge 渐变、非装饰性」——属规范的 fallback 规则可容忍的判断性取舍，但并非严格移除。
3. **未执行防抖/节流审计（G-20，阶段 5）**——规范:*「对搜索、筛选、窗口 resize…高频事件增加合理防抖或节流」*。计划延至 Task 8，本次 diff 未含。

### (b) 越界（scope creep）—— 一处后端改动

4. **`src-tauri/src/claude.rs`（`fc1b001`）** 改动 `validate_work_dir` 以剥离 Windows `\\?\` 前缀。规范约束 1:*「前端 UI 重构，不得改变…接口契约…用户实际使用方式」*;实施计划:*「不修改 `src-tauri/src/**`」*。此为对后端路径规范化的真实行为变更（+2 测试），且恰处于 `validate_work_dir` 所守护的安全相邻面。可辩称为既往 bug 修复，亦确实改善用户主流程，但**显式落在规范的 UI-only 范围之外**，违背计划的「不触 `src-tauri/`」规则。剥离逻辑本身正确（`\\?\UNC\`→`\\`、`\\?\`→裸）。前端无 API 契约/路由/权限/payload 变更——Launch/Proxy/App 的 IPC 名称与载荷逐字保留，仅错误展示包装变化。

### (c) 看似有误

5. **硬编码 DOM id 冲突。** `AsyncState.tsx`、`StatusBanner.tsx`、`PageErrorBoundary.tsx` 用字面量 id（`"async-state-title"`、`"status-banner-title"`、`"page-error-title"`）。同页两个 banner/state（如 App 错误 + 页级 StatusBanner）会导致 `aria-labelledby` 指向非唯一 id，违背规范*「错误提示应温和、可理解且可恢复」*及 G-16（待实施）。页面测试仅挂载单实例，故未捕获。
6. **`DashboardPage.tsx` spinner 误用**——`<CircleHelp className="ui-spinner" />` 把 spin 关键帧加在「帮助」问号图标上（一个旋转的「?」），与规范*「动效必须有目的」*冲突。应为 `LoaderCircle`。

### 跟踪文档声明 vs diff

`task-02/README.md` 称「全站 `rg` 已无 `from-blue`/`to-indigo`」——对源 `.tsx` 经核为真（仅余一处功能性 `linear-gradient`，已正确注明），未越界声明页面完成。`UI验收矩阵.md` 如实将 G-11/15/16/20-24/28-31 标为 待实施/待验证，已标「已完成」者 diff 均能佐证。**未发现虚高的完成度声明。**

---

## 汇总（按轴，不跨轴择优）

- **Standards**:0 硬性、6 项判断性嗅味。轴内最重 = 跨 `AsyncState`/`StatusBanner`/`PageErrorBoundary` 硬编码的 `aria-*` id（Duplicated Code，叠加真实页面可复现的 a11y 冲突）。
- **Spec**:6 项发现。轴内最重 = `claude.rs` 后端编辑，违背规范的 UI-only 约束与计划「不触 `src-tauri/`」规则（修复本身正确）。
