# Task 3 视觉证据（批次 B）

 hardening Dashboard / Launch / CLIProxy 工作流状态与可恢复性。

捕获方式：零网络 Playwright（路由拦截从 `dist/` 喂文件）+ 注入 Tauri `__TAURI_INTERNALS__` mock，通过 `?mode=` 注入状态场景。详见 `capture-task-03.mjs`（工作区 `node` 脚本目录）。

| 文件 | 场景 | 关键验证点 |
|---|---|---|
| `light-1280x800/dashboard-default-light-1280x800.png` | 仪表盘默认（CLIProxyAPI 未运行） | 统计卡 + 供应商/工作目录不变 |
| `light-1280x800/dashboard-error-light-1280x800.png` | `cliproxyapi_status` 拒绝 | `AsyncState` 网络错误 + “重新检测” 按钮 |
| `light-1280x800/launch-default-light-1280x800.png` | 启动页默认（含历史目录） | 历史目录为独立 button，删除为同级 ConfirmButton |
| `light-1280x800/launch-submitting-light-1280x800.png` | 点击启动、`launch_claude` 挂起 | 按钮锁定为“正在启动 Claude Code”、`aria-busy` |
| `light-1280x800/cliproxy-default-light-1280x800.png` | 代理页默认 | 启动/停止/刷新 + 执行目录 |
| `light-1280x800/cliproxy-loading-light-1280x800.png` | `cliproxyapi_status` 挂起 | 状态卡显示“检测中…” |
| `light-560x800/launch-error-light-560x800.png` | 点击启动、`launch_claude` 拒绝（窄屏） | 可恢复 `alert` + 启动按钮恢复可用；无溢出 |
| `light-560x800/cliproxy-error-light-560x800.png` | `cliproxyapi_status` 拒绝（窄屏） | 可恢复 `alert` + 重试；无溢出 |

> 窄屏 560px 验证：目录行与操作按钮均无横向溢出。
> 脱敏数据：工作目录/路径均为本地占位值（`D:\work` 等），无真实凭据。

对照基线：`docs/ui-refactor/evidence/before/`（Gate 批次）。
