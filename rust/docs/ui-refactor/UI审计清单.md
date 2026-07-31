# UI 审计清单

> 建立时间：2026-07-31 13:13:30 +08:00
>
> 状态：阶段 1 审计完成，实施与验证持续更新

状态约定：`待处理`、`进行中`、`已完成`、`受阻`、`不适用`。

## 阻断与功能风险

| ID | 优先级 | 原问题 | 文件/组件位置 | 重构方案 | 状态 | 验证证据 |
|---|---|---|---|---|---|---|
| A-01 | 阻断 | 首次 `get_config` 失败只写 console，最终仍渲染空页面 | `src/App.tsx` | 建立可见 Error + Retry；区分 loading/success/error | 待处理 | 新增 App 失败/重试测试；真实 IPC 失败场景 |
| A-02 | 阻断 | React.lazy chunk 失败没有 Error Boundary | `src/App.tsx` | 增加页面级错误边界与恢复操作 | 已完成 | `src/components/PageErrorBoundary.test.tsx` 2 项测试通过；App 已包裹 lazy 页面 |
| A-03 | 阻断 | 内置词典加载失败后可能永久显示“加载中” | `src/pages/DictionaryPage.tsx` | 维护 loading/error 状态和重试 | 待处理 | 模拟 `meta.load()` 拒绝，出现 Error + Retry |
| A-04 | 阻断 | 历史日志项用带 `onClick` 的 `li`，键盘不可操作 | `src/pages/LogPage.tsx` | 列表项内改为原生 button，保留选中语义 | 待处理 | Tab + Enter/Space 打开；组件测试 |
| A-05 | 阻断 | 历史日志弹窗无 dialog 语义、焦点圈禁、Esc、焦点恢复 | `src/pages/LogPage.tsx` | 抽取可复用 Dialog；遮罩、Esc、初始焦点、恢复焦点 | 待处理 | Dialog 单测 + 日志页回归 |
| A-06 | 阻断 | 词典选择器用可点击 `div`，没有 tab 语义与键盘导航 | `src/pages/DictionaryPage.tsx` | 原生 button + tablist/tab；方向键/Home/End | 待处理 | 键盘测试和 `aria-selected` |
| A-07 | 功能风险 | 启动 Claude 没有 pending 锁，可能重复提交 | `src/pages/LaunchPage.tsx` | 提交中禁用并显示“启动中”，失败后恢复 | 已完成 | `src/pages/LaunchPage.test.tsx` 第 1 项：延迟 promise 下只触发一次 `launch_claude`，按钮锁定为“正在启动 Claude Code” |
| A-08 | 功能风险 | Proxy refresh 失败被折叠成“未知”，无可恢复说明 | `src/pages/ProxyPage.tsx` | 细分初始、刷新中、失败、重试 | 已完成 | `src/pages/ProxyPage.test.tsx` 第 1 项：拒绝渲染可重试 `AsyncState`；第 2 项 start pending 锁 |
| A-09 | 功能风险 | 日志文件列表、等级设置、清除失败只清空或写 console | `src/pages/LogPage.tsx` | 结构化错误消息与重试；不把失败误报为空 | 待处理 | 模拟三个失败分支 |
| A-10 | 功能风险 | MessageBanner 依赖 emoji 首字符推断语义 | `src/components/MessageBanner.tsx` 及调用方 | 改为结构化 `kind/title/detail/action`，正确使用 status/alert | 进行中 | StatusBanner 组件与测试已完成；调用方迁移在 Task 4/5 完成 |
| A-11 | 功能风险 | NVIDIA 测试卡标题写死 8082 | `src/pages/nvidia/NvidiaTestPanel.tsx` | 标题使用实际 `port` | 待处理 | 端口变更后标题、URL、复制内容一致 |
| A-12 | 功能风险 | NVIDIA 模型即时更新可能触发 config 回填并覆盖未保存草稿 | `src/pages/NvidiaPage.tsx` | 明确草稿初始化/同步边界，模型仍即时持久化 | 待处理 | 编辑 Base URL 后移动模型，草稿保持；命令仍发送 |
| A-13 | 功能风险 | 启动/停止/刷新只通过 disabled 暗示处理中 | Proxy/NVIDIA 状态卡 | 按动作显示 pending 文案/Spinner，`aria-busy` | 已完成 | `src/pages/ProxyPage.test.tsx` 第 2 项：`aria-busy`、stop/refresh 禁用且不可重复触发 |
| A-14 | 功能风险 | 重要输入 label 未绑定字段 | Config/NVIDIA/Launch/Proxy | 使用稳定 id、`htmlFor`、`aria-describedby`、错误关联 | 进行中 | `FormField` 组件与测试已完成（Task 1）；页面 wiring 在 Task 4/5 |
| A-15 | 功能风险 | NVIDIA Key 与 auth token 默认明文 | `NvidiaConfigForm.tsx` | 复用可访问的 SecretInput 显隐控件 | 待处理 | 默认 password，显隐按钮 `aria-pressed` |
| A-16 | 功能风险 | 导入词典失败使用原生 `alert()` | `DictionaryPage.tsx` | 应用内 Error Banner/Dialog，保留格式示例与重试路径 | 待处理 | 非法 JSON 不触发原生 alert |
| A-17 | 功能风险 | ConfirmButton 没有 `type=button`，确认态不播报 | `ConfirmButton.tsx` | 明确 type、`aria-live`、动态 accessible name | 待处理 | 首次点击播报待确认，超时恢复 |
| A-18 | 功能风险 | 图标型模型操作仅依赖 title | `ModelPriorityEditor.tsx` | 为每个模型/位置提供唯一 aria-label | 待处理 | Testing Library 按名称定位全部操作 |
| A-19 | 功能风险 | Launch history、NVIDIA status/Key pool 的较旧异步响应可能覆盖较新状态，卸载后 deferred 结果仍可能落地 | `LaunchPage.tsx`、`NvidiaPage.tsx` | request id / generation + disposed guard；轮询和监听卸载清理 | 已完成 | `src/pages/LaunchPage.test.tsx` 第 6 项：单调递增 reqId 守卫，过期 `get_recent_dirs` 响应被忽略 |
| A-20 | 功能风险 | 目录对话框取消与权限拒绝缺少明确区分，配置路径读取失败被静默吞掉 | Launch/Proxy/Config | 空串取消保持 no-op；reject 显示可恢复错误；重试动作防重复 | 已完成 | `LaunchPage.test.tsx` 第 4/5 项、`ProxyPage.test.tsx` 第 5/6 项：空串 no-op、reject 可恢复 alert |
| A-21 | 功能风险 | NVIDIA 外部 host/token 的前端实时校验若阻止保存，会改变“允许保存、启动时后端拒绝”的既有契约 | `NvidiaPage.tsx` | `aria-invalid` 只作提示；普通 Save 仍持久化；Start 保持 save → start 顺序 | 待处理 | invalid draft 的 Save/Start 精确调用顺序测试 |

## 视觉、响应式与可访问性

| ID | 优先级 | 原问题 | 文件/组件位置 | 重构方案 | 状态 | 验证证据 |
|---|---|---|---|---|---|---|
| V-01 | 高 | 没有深色模式，背景和表面大量写死 | `styles.css`、`index.html`、多个 TSX inline style | 语义 Token + `prefers-color-scheme`；启动屏同步 | 已完成 | 浅/深 1100×720 同场景截图（`evidence/after/task-02/`）；令牌系统已落地 `styles.css` |
| V-02 | 高 | 没有宽度断点，三栏和多列输入窄屏溢出 | `styles.css` | 768px/560px 响应式布局；工具条换行；安全边距 | 待处理 | 360/480/768/1024px 无横向滚动 |
| V-03 | 高 | 多个控件实际点击区仅 24–36px | recent remove、模型按钮、secret reveal、词典删除 | 交互区域最小 44×44px，视觉图标可保持 16–20px | 待处理 | 盒模型测量与键盘测试 |
| V-04 | 高 | 全站 `:focus-visible` 不统一 | Sidebar、按钮、tab、列表、输入 | 全局可见 focus ring；不以 hover 替代 | 已完成 | 全局 `:focus-visible` 令牌化；Tab 遍历截图验证 |
| V-05 | 高 | 侧栏蓝紫渐变、进度条渐变违反目标 | `Sidebar.tsx`、`.dict-progress-fill` | 使用单一系统色和语义状态色 | 已完成 | `rg` 无 `from-blue/to-indigo/bg-gradient`；Sidebar 改用语义表面令牌 |
| V-06 | 高 | CSS、Tailwind、inline style 三套样式漂移 | Sidebar、Log、NVIDIA form/test、Dashboard | 迁移到语义 class 和 Token；保留 Tailwind 仅最低限度 | 待处理 | `rg` 检查 inline color/background/radius |
| V-07 | 中 | 小号 muted 文字和白字蓝底对比不足 | 全局 Token、侧栏版本、提示 | 使用可达 AA 的文本/强调 Token；状态不只靠颜色 | 待处理 | 对比度计算 + 人工检查 |
| V-08 | 中 | 所有普通卡片 hover 上浮/加阴影，装饰性较强 | `.card`、`.stat-card` | 仅交互卡片响应 hover；静态卡片保持稳定 | 待处理 | CSS 检查 + reduced-motion |
| V-09 | 中 | 未保存、测试中、确认删除使用无限脉冲 | `styles.css` | 改为一次/静态状态；reduced-motion 完整降级 | 待处理 | `prefers-reduced-motion` 测试与源码检查 |
| V-10 | 中 | Emoji 充当大量功能图标与状态唯一信号 | 多页面 | 使用 lucide 图标 + 文字/ARIA；保留非功能性内容时不得作唯一信号 | 待处理 | 图标按钮均有文本或 aria-label |
| V-11 | 中 | 页面标题装饰线和教程式大卡片层级偏模板化 | `.page-title`、页面卡片 | 采用原生大标题/section header/grouped surface 层级 | 待处理 | 同视口视觉检查 |
| V-12 | 中 | 毛玻璃无统一降级主题 | topbar/sidebar/modal | 仅导航与模态遮罩使用 glass Token，支持 reduced-transparency | 已完成 | `--color-glass` 令牌 + `prefers-reduced-transparency` 降级；浅/深截图 |
| V-13 | 中 | 启动屏用 emoji logo，颜色写死 | `index.html` | 使用真实应用图标或纯文字系统启动状态，跟随主题 | 已完成 | 启动屏改用 `32x32.png` 应用图标，跟随主题色 |
| V-14 | 中 | 窄屏若全局设置 `.ui-button { width: 100% }` 会误撑满 Dialog、显隐、分段和图标按钮 | 响应式实施计划 | 只对显式 `.stack-actions-mobile > .ui-button` 全宽；inline/icon 保持固有宽度与 44px hit area | 已完成 | `UI重构实施计划.md` Task 8；最终 560px 截图复核 |

## 状态、测试与证据

| ID | 优先级 | 原问题 | 文件/组件位置 | 重构方案 | 状态 | 验证证据 |
|---|---|---|---|---|---|---|
| T-01 | 高 | Loading/Empty/Error/Permission/Network/Submit 状态没有统一模型 | App 与各页面 | 建立 AsyncState / StatusBanner / Button loading 基础能力 | 已完成 | `src/components/ui/{Button,StatusBanner,AsyncState,Skeleton,FormField}.test.tsx` 共 16 项测试通过 |
| T-02 | 高 | Launch、Proxy、Config 缺少关键流程测试 | `src/pages` | 按 TDD 增加 pending、失败、恢复、参数回归 | 进行中 | `LaunchPage.test.tsx`(6)、`ProxyPage.test.tsx`(6) 已通过；Config 在 Task 4 |
| T-03 | 高 | 真实 IPC 契约仅靠人工对照 | 前端 invoke/listen 与 `src-tauri/src/lib.rs` | 固定基线 29 个 invoke、1 个 event、payload 和关键顺序；最终逐项核对 committed/staged/unstaged diff | 进行中 | `evidence/baseline-ipc.md`；Task 9 最终报告待执行 |
| T-04 | 中 | 无 lint 脚本 | `package.json` | 不擅自引入框架；以 TS strict、测试、build 和静态搜索补位 | 不适用 | 记录“未配置”，不伪报通过 |
| T-05 | 中 | 无 E2E/视觉回归框架 | 仓库级 | 本轮不引入重量依赖；使用受支持浏览器/原生窗口证据 | 受阻 | 本地 URL 被应用内浏览器策略拒绝 |
| T-06 | 中 | Vitest 有 localstorage 参数警告 | 测试运行环境 | 确认是否由宿主注入；不影响通过，避免掩盖新 warning | 待处理 | 最终测试输出 |
| T-07 | 已完成 | 应用内浏览器安全策略拒绝 localhost，缺少修改前运行截图 | `docs/ui-refactor/evidence/before/` | 使用 Playwright 零网络路由拦截直接从 `dist/` 喂文件，生成 13 张 1100×720 脱敏截图 | 已完成 | `evidence/before/*-light-1100x720.png`；`capture-baseline.mjs` 脚本在工作区 |
| T-08 | 高 | 已提交批次会让无基线 `git diff` 漏审，merge-base 又会混入旧分支变更 | 最终审查流程 | 固定 `UI_BASE_SHA=50808c5`，同时检查 committed range 与 worktree | 已完成 | `UI重构实施计划.md` Global Constraints / Task 9 |
| T-09 | 高 | 目录级 `git add` 会混入用户或并行任务改动，反之遗漏 evidence/计划又会造成不完整交付 | 阶段 0 与 Tasks 1–9 原计划 | 初始文档独立精确提交；每个 Task 逐文件暂存源码、文档和登记截图；暂存前后核对 name-only/cached check | 受阻 | 精确清单已写入计划；2026-07-31 13:58 `git add` 因父级 `.git` 写权限/审批额度被拒，尚未暂存 |
| T-10 | 高 | 仅在最终阶段截图不能满足“每批次可用视觉回归” | Tasks 1–8 | 建立 A–G 批次证据协议；不可达 primitives 明确 N/A 并在首次消费时补验 | 待处理 | `UI重构实施计划.md` 批次视觉回归协议；Gate 仍受阻 |
