# UI 重构验收矩阵

> 建立时间：2026-07-31 13:27 +08:00
>
> 基线：`UI_BASE_SHA=50808c5`
>
> 用途：把 `UI重构Goal.md` 的每项要求映射到直接证据；通用组件存在本身不能证明每个页面已完成。

状态约定：`已证明`、`待实施`、`待验证`、`受阻`、`不适用`。

| ID | Goal 要求 | 适用范围 / N/A 理由 | 直接证据 | 当前状态 |
|---|---|---|---|---|
| G-01 | 技术栈、命令、页面、组件、表单、弹窗、列表、导航盘点 | 全项目 | `UI重构记录.md` 阶段 0；`UI审计清单.md` | 已证明 |
| G-02 | 重构前测试/类型/build 基线 | 全项目 | `npm.cmd test` 13/13；`npm.cmd run build`；`evidence/baseline-build.md` | 已证明 |
| G-03 | 重构前关键页面截图 | 8 个页面 + 日志弹窗 + 未保存/异步状态 | `evidence/README.md` 记录应用内浏览器安全策略拒绝 localhost | 受阻 |
| G-04 | 可复现的重构前源码状态 | 全项目 | commit `50808c5`；`evidence/before-source-manifest.md` | 已证明 |
| G-05 | 语义颜色、字体、字号、间距、圆角、阴影、动效 Token | 全局 | `UI设计Token.md`；最终 CSS diff | 待实施 |
| G-06 | 浅色/深色主题与系统色映射 | 全局、启动屏、全部页面 | 主题 CSS + 8 页 light/dark 截图 | 待实施 |
| G-07 | 毛玻璃降级、reduced transparency | 仅顶栏、侧栏、Dialog 遮罩 | CSS media query + 截图 | 待实施 |
| G-08 | reduced motion | Spinner、Skeleton、Dialog、确认、学习卡片 | CSS media query + 动效状态检查 | 待实施 |
| G-09 | Button 状态矩阵 | 全部主要/次要/ghost/danger 操作 | `Button.test.tsx` + 页面 pending 测试 + 截图 | 待实施 |
| G-10 | Card 状态矩阵 | 普通、grouped、glass、interactive、elevated | CSS 审计 + 页面截图；无独立业务行为则不写源码文本测试 | 待实施 |
| G-11 | Input/Select/Textarea 状态 | Config、Launch、Proxy、NVIDIA、Logs、Dictionary | FormField/secret tests + 页面验证 | 待实施 |
| G-12 | Loading/骨架/Spinner | App、lazy page、词典、日志、状态刷新、提交 | AsyncState/Skeleton tests + 页面测试 | 待实施 |
| G-13 | Empty | 最近目录、Key 池、日志、词典搜索/学习 | 页面测试 + 状态截图 | 待实施 |
| G-14 | Error/网络失败/重试 | App、Dashboard、Proxy、NVIDIA、Logs、Dictionary | 页面失败/重试测试 | 待实施 |
| G-15 | 无权限 | 目录选择、配置读取等 Tauri 拒绝 | mock permission rejection + 可见恢复路径；静态 About 不适用 | 待实施 |
| G-16 | Disabled/Active/Hover/Focus | 全部交互控件 | 组件测试、CSS、键盘遍历、截图 | 待实施 |
| G-17 | Submitting/重复提交 | Launch、Proxy、Config、NVIDIA、日志清除 | deferred-promise 页面测试 | 待实施 |
| G-18 | 实时校验/校验失败 | Config 数值、NVIDIA host/port/token、导入词典 | `aria-invalid`、错误关联、边界测试 | 待实施 |
| G-19 | 请求取消/过期结果 | Tauri invoke 没有取消 API；必须忽略 stale response，卸载时清监听/轮询 | Launch/NVIDIA request-id、Log deferred-listen/合并、NVIDIA interval/deferred cleanup tests | 待实施 |
| G-20 | 防抖/节流 | Dictionary 搜索是本地 memo；NVIDIA 只有既定 2s polling；无 resize listener | Task 8 高频事件审计及明确“不需要”理由 | 待验证 |
| G-21 | Dialog 焦点/键盘/遮罩/Esc/恢复 | 当前唯一历史日志 Modal；没有 Sheet/Popover 业务组件 | `Dialog.test.tsx` + LogPage test | 待实施 |
| G-22 | List/Form Group/Table | recent/log/model/key/dictionary/profile 列表与表单；项目没有数据 Table | 页面测试；Table 标记不适用 | 待实施 |
| G-23 | 导航/响应式/安全区域 | AppShell、Sidebar、全部页面 | Sidebar test + 1280/768/560 matrix | 待实施 |
| G-24 | 键盘与至少 44×44px | 导航、表单、日志、词典、图标操作 | DOM/样式检查 + 键盘遍历 | 待实施 |
| G-25 | 不再有紫蓝渐变、占位文案、随机视觉值 | `src`、`index.html` | `rg` 禁止项检查 + Token audit | 待实施 |
| G-26 | 保持 IPC 命令、payload、路由、权限与业务流程 | 所有业务页面 | `evidence/baseline-ipc.md`（29 invoke + 1 event）逐项对照 committed/staged/unstaged diff + 页面回归表 | 待验证 |
| G-27 | 不修改后端/数据库 | `src-tauri/**`、数据库对象 | committed + worktree diff 均为空；本任务无数据库操作 | 待验证 |
| G-28 | 控制台错误、重复请求、布局抖动、性能回退 | 全部核心流程 | Tauri dev 控制台/IPC观察；bundle 与 `baseline-build.md` 对比 | 待验证 |
| G-29 | lint/typecheck/unit/integration/build | 当前无 lint/E2E/integration 脚本；不得伪报 | `npm test`、`tsc --noEmit`、build；缺失项明确 N/A | 待验证 |
| G-30 | 每批次文档、视觉回归与回滚方式 | Tasks 1–9 | `UI重构记录.md` 时间线、`evidence/after/task-XX/`、审计状态、commit；不可达 Task 1 记录 N/A 并随 Task 2 补验 | 待实施 |
| G-31 | 最终前后对比、测试、风险、未完成项和建议 | 全项目 | `UI重构总结.md` + evidence links | 待实施 |

## 页面状态责任

| 页面 | Loading | Empty | Error/Retry | Disabled/Submitting | Permission/Cancel |
|---|---|---|---|---|---|
| AppShell | 配置、lazy chunk | 不适用 | 配置重试、chunk 重新载入 | 导航按状态保持 | 配置拒绝可见；reload 恢复 |
| Dashboard | Proxy 检测 | 无 profiles/work dir 为空 | Proxy 重试 | refresh pending | 不适用 |
| Launch | history、launch | 无 history | select/history/launch 错误 | 启动中锁定 | dialog cancel=no-op；拒绝=error |
| CLIProxy | status/action | 执行目录为空是合法默认 | status/start/stop/dir error | action-specific busy | dialog cancel=no-op；拒绝=error |
| NVIDIA | status/key pool/test | Key/model empty | network/save/test error | start/stop/save/test busy | invoke 不可取消；卸载忽略 stale |
| Logs | buffer/files/history | 无日志/无文件/空文件 | 各资源独立 retry | clear/read busy | listener unmount cleanup |
| Config | path/read/save | profiles 可为空 | save/path error | 两类 save 独立 busy | config access rejection |
| Dictionary | chunk/import | search/no cards/completed | load/import retry | import/loading disabled | file chooser cancel=no-op |
| About | 不适用：静态页面 | 不适用 | 不适用 | 不适用 | 不适用 |
