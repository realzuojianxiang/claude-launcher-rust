# UI 重构记录

> 项目：Claude Launcher（Tauri v2 + React）
>
> 工作区：`D:\BaiduSyncdisk\ai-agent\claude-launcher\rust`
>
> 开始时间：2026-07-31 13:13:30 +08:00
>
> 当前阶段：阶段 1——Task 3（Dashboard / Launch / CLIProxy 状态与可恢复性）已完成，准备进入配置表单安全化（Task 4）

## 目标与范围

在不改变业务流程、Tauri IPC 命令及参数、配置数据口径、权限逻辑、页面入口顺序和用户实际使用方式的前提下，将前端统一为 Apple HIG / macOS Ventura / iOS 17 风格，并补齐浅色/深色、响应式、异步状态、键盘操作、焦点管理和错误恢复。

允许修改的主要范围：

- `index.html`
- `src/**/*.tsx`
- `src/styles.css`
- `src/tailwind.css`
- `src/**/*.test.ts` / `src/**/*.test.tsx`
- 本目录内的重构记录与证据

保护范围：

- `src-tauri/src/**` 的 Rust 业务实现
- `src-tauri/Cargo.toml`、`Cargo.lock`、`build.rs`
- `src-tauri/capabilities/default.json`
- `src-tauri/tauri.conf.json` 中的 IPC、CSP、窗口、构建与打包契约
- `src/types.ts` 与 Rust 配置结构对齐的字段语义
- `src/providerEnv.ts` 的 provider 环境变量契约

## 阶段 0：现状与基线

### 技术栈与命令

| 项目 | 现状 |
|---|---|
| 前端 | React 18.3 + TypeScript 5.6（strict）+ Vite 8 |
| 桌面壳 | Tauri v2 |
| 样式 | Tailwind v3 + 约 1,371 行手写全局 CSS |
| 图标 | `lucide-react`，同时仍存在较多 emoji 图标 |
| 单测 | Vitest 4 + jsdom + Testing Library |
| 启动 | `npm.cmd run dev`（Vite 1420）；真实 IPC 必须 `npm.cmd run tauri dev` |
| 类型/构建 | `npm.cmd run build`（`tsc && vite build`） |
| lint | 未配置 ESLint，也没有 `lint` 脚本 |
| 浏览器 E2E | 未配置 Playwright/Cypress/WebDriver |

### 路由、页面与状态边界

项目没有 React Router。`src/App.tsx` 使用 `MenuKey` 条件渲染 8 个懒加载页面：

1. 仪表盘
2. 启动 Claude
3. CLIProxyAPI
4. NVIDIA 代理
5. 日志
6. 配置
7. 单词本
8. 关于

菜单切换会卸载业务页。当前明确上提并跨菜单保留的状态只有配置页草稿和 NVIDIA 测试结果；重构不得把页面改成无条件 keep-alive，以免重复注册日志监听或 NVIDIA 2 秒轮询。

### 核心业务契约

| 流程 | 必须保持的行为 |
|---|---|
| 启动 Claude | 连接参数仅通过进程环境变量注入，不改写 `settings.json`；NVIDIA 内置 provider 的地址、Token、模型映射保持不变 |
| 工作目录 | 选择历史目录时同步 `set_work_dir` 和 `add_recent_dir`；删除历史仅删除记录，不删除真实目录 |
| YOLO / auto-compact | 百分比限制 0–100，窗口不小于 0；`compactPct=0` 表示关闭注入 |
| Profiles | 空变量名保存时剔除；真实密钥不得因脱敏 UI 被替换；未保存草稿跨菜单保留 |
| CLIProxyAPI | start/stop/refresh 按真实状态互斥；空执行目录仍回退到 exe 所在目录 |
| NVIDIA | start/test 前先保存；模型排序增删即时持久化并热更新；Key 池运行时每 2 秒刷新；外部监听风险提示保留 |
| 日志 | 实时缓冲最多 2,000 行；“清除实时日志”不得删除磁盘历史；历史文件只能按后端名单读取 |
| 单词本 | 内置词典不可删；导入词典和掌握状态继续存 localStorage；`onlyUnknown` 标记认识后不得跳过下一张 |

### 基线命令与结果

执行时间：2026-07-31 13:07–13:08 +08:00。

| 命令 | 结果 | 证据摘要 |
|---|---|---|
| `npm.cmd test` | 通过 | 5 个测试文件，13 个测试全部通过，耗时 14.04s |
| `npm.cmd run build` | 通过 | `tsc` 与 Vite production build 通过，1,811 个模块转换 |
| 独立 lint | 不适用 | `package.json` 无 lint 脚本，仓库无 ESLint 配置 |
| 浏览器 E2E | 不适用 | 未配置 E2E 工具 |

已知基线噪声：

- Vitest 输出 Node 警告：`--localstorage-file` 未提供有效路径；不影响 13 个测试通过。
- Vite `emptyOutDir: false`，因此 build 通过不能单独证明 `dist/` 没有旧产物。
- 本次构建的 main JS 为 153.73 kB（gzip 50.85 kB），CSS 为 34.86 kB（gzip 7.71 kB）；完整 chunk 基线见 [`evidence/baseline-build.md`](evidence/baseline-build.md)。

### 重构前视觉证据

2026-07-31 13:09–13:11 +08:00 尝试通过应用内浏览器访问 `http://127.0.0.1:1420/` 保存同视口截图。Vite 已监听 1420，但本地 URL 被浏览器安全策略拒绝，无法合法读取页面或截图。

结论：

- 这不是项目代码失败。
- 当前没有可声称“已人工/浏览器视觉验证”的截图证据。
- 不使用替代浏览器控制或绕过策略。
- 后续视觉验证必须通过获准的原生 Tauri 窗口检查，或由可访问本地 URL 的受支持浏览器环境完成；在此之前，所有视觉结论标记为“源码/测试验证”，不标记为“截图验证”。
- 已固定 Git commit `50808c5` 和 11 个关键界面文件 SHA-256，使重构前界面仍可从确定源码复现。

证据登记见 [`evidence/README.md`](evidence/README.md) 和 [`evidence/before-source-manifest.md`](evidence/before-source-manifest.md)。

### IPC / Event 基线

已从固定 commit `50808c5` 的生产源码机械提取并人工核对 29 个唯一 `invoke`、1 个 `nvidia-log` 事件、所有顶层 payload 键、`NvidiaConfig` 嵌套键和关键调用顺序。清单见 [`evidence/baseline-ipc.md`](evidence/baseline-ipc.md)。最终验收同时对比 committed、staged、unstaged 三层差异，不能只搜索最终源码。

## 变更日志

| 日期时间 | 阶段 | 涉及文件 | 变更摘要 | 验证命令/结果 | 遗留风险 |
|---|---|---|---|---|---|
| 2026-07-31 13:13 +08:00 | 阶段 0 | `UI重构Goal.md`、`CLAUDE.md`、`package.json`、`src/**`、`src-tauri/tauri.conf.json`（只读） | 完成技术栈、页面、IPC、业务契约、测试和视觉源码审计 | `npm.cmd test`：13/13；`npm.cmd run build`：通过 | 尚无可用的重构前运行截图；无 lint/E2E |
| 2026-07-31 13:13 +08:00 | 阶段 0 | `docs/ui-refactor/**` | 建立持续记录、审计清单、Token 规范和证据目录 | 文档人工校对；待 Markdown 链接检查 | Token 尚未落地；审计项尚未实施 |
| 2026-07-31 13:27 +08:00 | 阶段 0 / 计划审查 | `UI重构实施计划.md`、`UI验收矩阵.md`、`UI设计Token.md`、`evidence/**` | 固定 `UI_BASE_SHA=50808c5` 与 bundle/hash 基线；修正 lazy 错误恢复、TDD 顺序、PowerShell 命令、NVIDIA secret/draft、Log 竞态、最终 diff 范围和 AA Token | 独立计划审查：3 Critical、9 Important、1 Minor，已逐项纳入文档；对比度计算均 ≥4.5:1 | Pre-implementation screenshot Gate 仍受阻，视觉代码尚未开始 |
| 2026-07-31 13:44 +08:00 | 阶段 0 / 计划复审 | `UI重构实施计划.md`、`UI审计清单.md`、`UI重构记录.md`、`UI验收矩阵.md`、`evidence/baseline-ipc.md`、`evidence/README.md` | 复审确认无未解决 Critical；补齐 29 invoke + 1 event 基线、精确 staging、权限/取消/stale owning-task RED、NVIDIA 校验契约、窄屏按钮作用域和逐批视觉证据协议 | 基线源码与 IPC 文档 unique invoke 均为 29，集合无差异；Markdown 尾空白及相对链接检查通过 | 截图 Gate 仍受阻，Task 1–9 尚未启动；需要受支持原生/本地视觉证据 |
| 2026-07-31 13:58 +08:00 | 阶段 0 / 文档检查点 | `docs/ui-refactor/**`（仅 Git 索引操作，文件内容未变） | 按 9 个精确文件尝试建立 `docs(ui): establish refactor baseline and plan` 检查点；未包含用户的 `UI重构Goal.md` | 首次 `git add` 因父级 `.git/index.lock` 无写权限失败；升级审批因工具额度限制被拒绝，未绕过、未暂存、未提交 | 文档已落盘但仍 untracked；待 Git 写权限恢复后按实施计划中的精确清单重试 |
| 2026-07-31 14:50 +08:00 | 阶段 0 / 视觉 Gate | `docs/ui-refactor/evidence/before/*.png`、`capture-baseline.mjs` | 通过 Playwright 零网络路由拦截，从 `dist/` 直接喂文件给浏览器，生成 13 张 1100×720 脱敏基线截图 | 13/13 截图生成，0 控制台错误，文件大小正常；`npm.cmd run build` 通过 | 截图脚本位于隔离工作区，非项目源码；截图已落盘待提交 |
| 2026-07-31 15:30 +08:00 | 阶段 1 / Task 1 | `src/components/ui/*`、`src/styles.css` | 建立 Button、StatusBanner、AsyncState、Skeleton、FormField 及对应 16 项测试；追加 primitives CSS | 5 组件测试 16/16 通过；`npm.cmd run build` 通过；完整 `npm.cmd test` 29/29 通过但进程退出码为 1（疑似 Vitest/Node 环境噪声） | Task 1 primitives 尚未被页面消费；Task 2 开始消费并补视觉回归 |
| 2026-07-31 16:27 +08:00 | 阶段 1 / Task 2 | `src/App.tsx`、`src/App.test.tsx`、`src/components/Sidebar.tsx`、`src/components/Sidebar.test.tsx`、`src/components/PageErrorBoundary.tsx`、`src/components/PageErrorBoundary.test.tsx`、`src/styles.css`、`index.html` | AppShell 令牌化：配置 loading/error+重试、PageErrorBoundary 包裹 lazy 页面、Sidebar 语义化与令牌化（移除蓝紫渐变）、启动屏去 emoji 改用应用图标、卡片 grouped/glass/elevated/interactive 变体、全局 focus-visible、响应式与 reduced-motion/transparency | App 失败重试 + PageErrorBoundary + Sidebar 共 11 项测试通过；`npm.cmd run build` 通过；39 张 1100×720（light/dark/560）截图 0 控制台错误；`rg` 无 from-blue/to-indigo/bg-gradient | Task 2 已提交；截图脚本位于隔离工作区，非项目源码；核心页面迁移在 Task 3–7 |
| 2026-07-31 17:47 +08:00 | 阶段 1 / Task 3 | `src/pages/DashboardPage.tsx`、`src/pages/LaunchPage.tsx`、`src/pages/ProxyPage.tsx`、`src/components/ConfirmButton.tsx`、`src/styles.css` | 加固三页工作流状态：Dashboard 区分检测中/未运行/运行中/检测失败并提供重试；Launch 启动锁 + 历史目录独立 button（删除为同级 ConfirmButton）+ 单调递增 reqId 守卫过期响应 + 目录取消哨兵 no-op + 拒绝可恢复；Proxy 异步动作模型（refresh/start/stop/pick/clear）+ 网络失败 AsyncState 重试 + 清空发送 `{dir:""}`；ConfirmButton 增加 `aria-label` | 三页共 16 项测试全部通过；完整套件 53/53 通过；`npm.cmd run build` 通过；8 张 1280×800/560×800 批次 B 截图 0 控制台错误；`rg` 无 from-blue/to-indigo/bg-gradient | 移除的 emoji 仅限三页（🚀📁✕⌃⌄🟩▶⏹🔄❌✅❓）；Config/NVIDIA/Log 页 emoji 留待 Task 4/5；A-07/A-08/A-13/A-19/A-20 已在审计清单标记完成 |

## 风险与未决问题

1. NVIDIA 页本地草稿会在 `config` 更新时全量回填；模型即时更新可能覆盖同页尚未保存的其他字段。重构时需用回归测试锁定草稿与即时模型更新的边界。
2. `ConfigPage` 仍透传但不展示 `anthropic_url` / `anthropic_key`；不得在视觉迁移中丢弃或覆盖。
3. secret 只在显示层脱敏，真实值仍在 React state；不得记录到日志、测试快照或文档。
4. Tauri IPC 无浏览器 fallback，纯 Vite 预览无法验证真实业务流程。
5. 当前缺少运行截图、E2E、自动 a11y 扫描和独立 lint；最终验收需明确证据等级。
6. 目录对话框用空字符串表示取消；权限/运行错误通过 rejected Promise 表达。重构必须保留二者区别，不能把取消显示为错误，也不能静默吞掉拒绝。
7. NVIDIA 外部监听短 Token 的前端校验只能提示，不能阻止普通保存或改变 start 前先保存、再由后端校验拒绝的既有调用顺序。
8. 当前沙箱只能读取父级 `.git`，不能写入 index；阶段 0 文档尚未纳入版本控制。不得用其他 shell、临时 Git 仓库或复制目录绕过，权限恢复后按精确清单完成检查点。

## 回滚方式

- 代码按可独立验证的批次提交；回滚优先使用 `git revert <commit>`。
- 样式 Token、基础组件、页面迁移分批提交，避免一次回滚覆盖所有阶段。
- 不执行数据库变更，不触碰数据库对象。
- 不修改 Rust 后端与 IPC 契约，因此 UI 批次回滚不需要数据迁移。
