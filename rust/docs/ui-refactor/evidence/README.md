# UI 重构证据目录

建立时间：2026-07-31 13:13:30 +08:00。

本目录用于保存重构前后截图、场景说明和验证输出索引，不保存账号、密码、API Key、Token、真实日志内容或其他敏感信息。

计划目录：

- `before/`：重构前同视口截图
- `after/task-XX/`：每个用户可见重构批次的同视口对比与场景说明
- `states/`：Loading、Empty、Error、Disabled、Submitting 等脱敏状态

文件命名使用稳定、可精确暂存的场景名；采集时间写入相邻 README 元数据，不写入文件名：

```text
<page>-<state>-<theme>-<width>x<height>.png
```

每个 `before/` 或 `after/task-XX/` 目录只能包含该批次 README 明确登记且已脱敏复核的文件。Git 暂存必须逐文件列名，禁止用目录、glob 或 `git add -A`。

## 当前证据状态

| 日期时间 | 场景 | 结果 |
|---|---|---|
| 2026-07-31 13:09–13:11 +08:00 | 应用内浏览器访问 `http://127.0.0.1:1420/` | Vite 已监听，但本地 URL 被浏览器安全策略拒绝；未生成截图 |
| 2026-07-31 13:07 +08:00 | 前端单测基线 | 5 文件 / 13 测试通过 |
| 2026-07-31 13:08 +08:00 | TypeScript + production build | 通过 |
| 2026-07-31 13:20 +08:00 | 重构前可复现源码基线 | Git commit `50808c5` + 11 个关键界面文件 SHA-256；见 [`before-source-manifest.md`](before-source-manifest.md) |
| 2026-07-31 13:08 +08:00 | Production bundle | 完整当前产物大小与 gzip 清单；见 [`baseline-build.md`](baseline-build.md) |
| 2026-07-31 13:44 +08:00 | 前端 IPC / Event 契约 | 固定 commit 中 29 个唯一 invoke、1 个 event、payload 与关键顺序；见 [`baseline-ipc.md`](baseline-ipc.md) |
| 2026-07-31 14:50 +08:00 | 重构前基线截图 | 使用 Playwright 零网络路由拦截生成 13 张 1100×720 脱敏 PNG；见 [`before/`](before/) 与 [`before-source-manifest.md`](before-source-manifest.md) |
| 2026-07-31 15:30 +08:00 | Task 1 primitives 证据 | 无可达 UI 变化，记录 N/A；见 [`after/task-01/README.md`](after/task-01/README.md) |

基线截图已获得：13 张 PNG 覆盖 8 个默认页面、日志历史弹窗、配置未保存状态、Loading/Empty/Error 场景，均使用 1100×720 视口与脱敏数据。
