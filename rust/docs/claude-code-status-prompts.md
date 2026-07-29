# Claude Code 状态提示与提示词全集（v2.1.220）

> 采集来源：本机安装的 Claude Code 原生二进制 `@anthropic-ai/claude-code@2.1.220`
> （`%APPDATA%\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe`，265MB 编译产物）。
>
> 说明：
> - 二进制内除 UI 文案外，还包含大量 V8 引擎 / WASM / 网络库的内部报错字符串，**本文仅收录面向用户的状态提示与交互提示**，已人工剔除引擎内部文案。
> - 标 `（已提取）` 的条目为直接从二进制中扫描到的真实字符串；标 `（常见提示）` 的为 Claude Code 已知行为但未在二进制中单独命中的整句，作补充。
> - 含 `[...]` 的条目为运行时动态拼接（工具名 / 文件路径 / 命令 / URL / token 数等占位）。

---

## 1. 权限 / 工具调用确认提示（Permission prompts）

- Claude wants to call a tool （已提取）
- Claude wants to enter plan mode to explore and design an implementation （已提取）
- Claude wants to exit plan mode （已提取）
- Claude wants to fetch content from [URL] （已提取）
- Claude wants to fetch content from this URL （已提取）
- Claude wants to guide you through [流程] （已提取）
- Claude wants to list artifacts other people shared with you （已提取）
- Claude wants to list artifacts published by you or shared with you （已提取）
- Claude wants to list your published artifacts （已提取）
- Claude wants to publish [artifact] （已提取）
- Claude wants to read artifacts （已提取）
- Claude wants to search the web for [query] （已提取）
- Claude wants to use your browser （已提取）
- Claude wants to watch an artifact for live updates （已提取）
- Claude needs your permission （已提取）
- Claude needs your input （已提取）
- Claude needs to execute shell commands （已提取）
- Claude needs to read or edit files （已提取）
- Claude needs to interact with GUIs （已提取）
- Claude needs to run code in a sandbox （已提取）
- Claude needs your approval for a review artifact （已提取）
- Do you want to allow Claude to fetch this content? （已提取）
- Do you want to allow this connection? （已提取）
- Do you want to proceed? （已提取）
- Do you want to use this API key? （已提取）

### 权限拒绝 / 权限模式（Permission denied / mode）

- Permission denied （已提取）
- Permission denied by user （已提取）
- Permission denied by hook （已提取）
- Permission denied by PermissionRequest hook （已提取）
- Permission denied for this action （已提取）
- Permission for this tool use was denied （已提取）
- Permission for this action was denied by the Claude Code [permission system] （已提取）
- Permission dialog failed （已提取）
- Permission mode [changed / downgraded to default / active when this message was sent / for spawned sessions / forced to default] （已提取）
- Permission mode override over the control channel （已提取）
- Permission Grant / Permission Grant and Protected （已提取）

## 2. 权限确认的选项标签（Permission option labels）

出现在交互选择菜单中（已提取的变体）：

- Yes （常见提示）
- No （常见提示）
- Yes, and don't ask again （已提取）
- Yes, and allow [access to ...] （已提取）
- Yes, and always allow access to [path] （已提取）
- Yes, and bypass permissions （已提取）
- Yes, and remember this directory （已提取）
- Yes, and switch to auto mode / Yes, and use auto mode （已提取）
- Always allow [access to ...] （常见提示）
- No, and tell Claude what to do （常见提示）

## 3. 计划模式（Plan mode）

- Claude wants to enter plan mode to explore and design an implementation （已提取）
- Claude wants to exit plan mode （已提取）
- Exit plan mode / Exit plan mode with [note] （已提取）
- Review the plan （已提取）
- Review the plan in Claude Code on the web （已提取）
- Plan mode is active （常见提示）
- Claude is in plan mode （常见提示）

## 4. 认证 / 登录（Auth / Login）

- Login blocked （已提取）
- Auth error （已提取）
- Auth token expired or invalid （已提取）
- Auth code was successfully acquired （已提取）
- Auth method （已提取）
- Credential authentication failed （已提取）
- Credential archived （已提取）
- Credential deleted （已提取）
- Do you want to use this API key? （已提取）
- Trust this directory （已提取）
- Trust not accepted for current directory （已提取）
- Trust but verify （已提取）

## 5. 会话 / 退出（Session / Exit）

- Exit anyway （已提取）
- Exit and fix manually （已提取）
- Exit the CLI （已提取）
- Exit the REPL （已提取）
- Exit when Claude stops asking for tools （已提取）
- Exit with Ctrl-[C] （已提取）
- Exit without making changes （已提取）
- Interrupt a running session / Interrupt a Running Session （已提取）
- Interrupt often or let Claude run （已提取）
- Session busy （已提取）
- Session at any time （已提取）
- Session becomes [state] （已提取）
- Are you sure you want to exit? (Y/n) （常见提示，二进制命中 "Are you sure you want to delete this permission rule" 同族）

## 6. 更新 / 版本（Update / Version）

- Newer version available （已提取）
- Update available （已提取）
- Update Claude Code and try again （已提取）
- Update Claude Code to use it （已提取）
- Update Claude Code using your organization （已提取）
- Deprecated [Models / alias] （已提取）

## 7. 错误 / 网络 / 限流（Error / Network / Rate limit）

- Connection error （已提取）
- Connection closed while thinking （已提取）
- Connection interrupted by system sleep （已提取）
- Connection dropped / Connection dropped by [keepalive / timeout] （已提取）
- Connection refused （已提取）
- Connection blocked by network allowlist （已提取）
- Connection attempt timed out after [n] （已提取）
- Connection lost （已提取）
- Rate limited / Rate limit [info] （已提取）
- Rate limits on API calls （已提取）
- API Error [message] （常见提示）
- Error [context] （通用错误前缀；二进制中大量 "Error ..." 为引擎内部，用户可见的通常为 API/Connection 类）

## 8. 上下文 / Token 限制（Context / Token limits）

- Context limit reached （已提取）
- Context compaction （已提取）
- Context editing and compaction operate within a session （已提取）
- Context exceeds the [limit] （已提取）
- Context grows stale over many turns （已提取）
- Context low （已提取）
- Context window [相关说明] （已提取）
- Token limits （已提取）
- Compacting conversation...（自动压缩提示，常见提示）
- Auto-compact enabled（常见提示）

## 9. 状态栏 / 进度 / 思考（Status / Progress）

- "Claude is thinking..." / is thinking （已提取 `is thinking`）
- Esc to interrupt（由 `was interrupted` 族可知支持中断）
- was interrupted （已提取）
- was interrupted and ended with [state] （已提取）
- was interrupted before completion （已提取）
- was interrupted while unattended （已提取）
- Press Enter to retry （已提取）
- Press Enter to connect （已提取）
- Press Enter to try again （已提取）
- Press Enter when ready to verify （已提取）
- Press any key to continue （已提取）
- Press any key to exit （已提取）
- Press Esc to cancel （已提取）
- Press Esc to go back （已提取）
- Press return to submit （已提取）
- Cost of the Claude Code session （已提取）
- Cost and usage accumulated by the current session （已提取）
- Cost ceiling hit during this run （已提取）

## 10. 跳过权限模式（YOLO / `--dangerously-skip-permissions`）

- Yes, and bypass permissions （已提取）
- Skipping permission check（常见提示；二进制中 "Skipping ..." 多为内部日志）
- 启动器侧提示：「YOLO 模式（跳过权限确认）」（见本项目 `ConfigPage.tsx` / `LaunchPage.tsx`）

## 11. 结构化消息协议（SDK streaming JSON）

Claude Code 通过 stdout 以 JSON 行（每行一条消息）流式返回，类型与 `subtype` 如下：

- `system`：会话初始化，含 `tools` / `model` / `permissionMode` / `tools_version` 等
- `assistant`：模型回复，`content` 含 `text` / `tool_use` / `thinking`
- `user`：用户输入或工具结果（`tool_result`）
- `result`：会话结束，含 `subtype`：
  - `success`：正常结束
  - `error` / `api_error`：API 错误
  - `max_tokens` / `out_of_tokens`：超出 token 上限
  - `expired`：会话过期
  - `interrupted`：被用户中断（对应第 9 节 "was interrupted"）
  - `permission_error`：权限错误
  - `end_turn`：回合结束
  - `payment_required`：需要付费
- 流式事件（SDK `Query`）：`system` / `assistant` / `user` / `result` / `stream_event`
  （`stream_event` 内增量类型含 `thinking` / `text` / `tool_use` / `tool_result` / `input` / `signature`）

## 12. 通用交互控件标签（按钮 / 选项）

- Yes / No
- Allow / Deny
- Accept / Reject
- Continue / Cancel
- Retry
- Skip
- Save / Discard
- Proceed （已提取 "Proceed" / "Proceed without asking only when the [user requested]"）

---

### 备注

- 以上为 v2.1.220 二进制提取 + 已知协议整理，版本升级后文案可能变化。
- 动态提示中的 `[...]` 为运行时填充：工具名（如 `Bash` / `Edit` / `Write`）、文件路径、命令、URL、token 数等。
- 二进制内同时包含大量非 UI 内部字符串（V8 引擎报错、WASM / 网络库错误），已排除，不计入本文。
