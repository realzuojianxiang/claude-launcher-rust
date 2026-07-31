# Task 2 视觉批次记录

- 时间：2026-07-31 16:27 +08:00
- 批次：AppShell 令牌化 + 启动可恢复（配置 loading/error + 重试、PageErrorBoundary、Sidebar 语义化与令牌化、启动屏去 emoji、卡片变体、响应式与 reduced-motion/transparency）。
- 采集方式：复用 Gate 的零网络 Playwright 路由拦截，从当前 `dist/` 直接喂文件；脱敏夹具数据，无真实账号/密钥/日志。

## 文件清单（39 张 PNG，1100×720）

每个场景在 `light-1100x720/`、`dark-1100x720/`、`light-560x720/` 三个子目录各一份，文件名形如 `<scene>-<theme><W>x<H>.png`：

| 场景 | 子目录 | 说明 |
|---|---|---|
| dashboard-default | light/dark/560 | 仪表盘默认 |
| launch-default | light/dark/560 | 启动 Claude 默认 |
| cliproxy-default | light/dark/560 | CLIProxyAPI 默认 |
| nvidia-default | light/dark/560 | NVIDIA 代理默认 |
| logs-default | light/dark/560 | 日志默认 |
| logs-dialog | light/dark/560 | 日志历史弹窗 |
| config-default | light/dark/560 | 配置默认 |
| config-dirty | light/dark/560 | 配置未保存（"未保存供应商" 草稿徽标） |
| dictionary-default | light/dark/560 | 单词本默认 |
| about-default | light/dark/560 | 关于默认 |
| state-loading | light/dark/560 | App 启动加载（get_config 不返回） |
| state-empty | light/dark/560 | 日志空态（暂无日志） |
| state-error | light/dark/560 | 启动 Claude 失败（连接超时横幅） |

## 与 before/ 的可比性

`before/` 基线亦为 1100×720 浅色同视口同场景（Gate 规定窗口）。`light-1100x720/` 与其逐张同名场景可直接像素对比，验证以下 Task 2 变化：

- Sidebar 蓝紫渐变 → 语义令牌表面；激活项使用系统蓝强调而非渐变。
- 启动屏 emoji logo → 真实应用图标（32×32 PNG），跟随主题色。
- 启动加载/错误使用 `AsyncState` 结构化状态（"加载配置中…"、错误 + "重新读取配置" 重试按钮）。
- 卡片新增 grouped/glass/elevated/interactive 变体，静态卡片无 hover 上浮；全局 `:focus-visible` 焦点环。
- 深色（dark-1100x720）与窄屏（light-560x720）验证令牌系统在主题/响应式下的降级与可读性。

## 人工结论

- 13 个场景 × 3 组 = 39 张截图全部生成，0 控制台错误、0 页面错误（Playwright `pageerror`/`console.error` 监听为空）。
- 浅/深/窄屏下布局完整、无横向溢出遮挡；焦点环在 Tab 遍历时可见。
- 业务状态（未保存徽标、加载中、空态、错误+重试）均按语义渲染，未把取消误报为错误。
- 全站 `rg` 已无 `from-blue`/`to-indigo`/`bg-gradient`；剩余 2 处 `linear-gradient` 均为功能性（成功徽标纯色渐变、骨架 shimmer），不属于被禁的蓝紫装饰渐变。

## 已知局限

- 本批次未单独采集 1280×720；before 与本次均以 1100×720 为基准以保证同窗口对比。完整 1280/768/560 × Light/Dark 矩阵在 Task 8 最终回归统一采集。
- 深色/窄屏结论基于渲染截图与 CSS 令牌审核，非真实 Tauri 原生窗口；Tauri 原生视觉以 Task 8 或获准原生窗口为准。
