# 重构前前端 IPC / Event 基线

记录时间：2026-07-31 +08:00。

固定基线：

```text
UI_BASE_SHA=50808c5
branch=feat/rust-implementation
```

本清单只记录 commit `50808c5` 中前端生产代码实际调用的 Tauri
`invoke` 和 `listen` 契约，不以当前工作树为依据。测试 mock 不计入调用清单。

## 汇总

- 前端使用 29 个唯一 `invoke` 命令。
- 前端订阅 1 个事件：`nvidia-log`。
- `src-tauri/src/lib.rs` 在该基线注册但前端未调用的命令：
  `get_system_info`。
- payload 键名区分大小写；没有 payload 的命令不得为“统一接口”额外发送空对象。

## Invoke 清单

| 页面 / 文件（基线行） | 命令 | 顶层 payload 键 | 返回值 / 前端用途 |
|---|---|---|---|
| `src/App.tsx:87` | `get_config` | 无 | `Config`；App 初始化完成后才渲染业务页 |
| `src/pages/ConfigPage.tsx:48` | `config_path` | 无 | `string`；展示配置文件路径 |
| `src/pages/ConfigPage.tsx:59` | `set_config` | `url`, `key`, `cliproxyKey`, `yoloMode`, `compactWindow`, `compactPct`, `cliproxyapiDir` | `string`；保存全局参数 |
| `src/pages/ConfigPage.tsx:91` | `set_profiles` | `profiles` | `string`；保存清理后的 Profile 列表 |
| `src/pages/DashboardPage.tsx:14` | `cliproxyapi_status` | 无 | `ProxyStatus`；仪表盘状态探测 |
| `src/pages/LaunchPage.tsx:31` | `get_recent_dirs` | 无 | `string[]`；读取最近目录 |
| `src/pages/LaunchPage.tsx:58` | `select_directory` | 无 | `string`；空串表示用户取消 |
| `src/pages/LaunchPage.tsx:75` | `set_work_dir` | `dir` | `string`；选用历史目录时同步后端工作目录 |
| `src/pages/LaunchPage.tsx:76` | `add_recent_dir` | `dir` | `string[]`；选用历史目录后置顶历史 |
| `src/pages/LaunchPage.tsx:87` | `remove_recent_dir` | `dir` | `string[]`；只删除历史记录，不删除真实目录 |
| `src/pages/LaunchPage.tsx:98` | `launch_claude` | `yolo`, `env` | `string`；`env` 为本次进程环境变量映射 |
| `src/pages/ProxyPage.tsx:28` | `cliproxyapi_status` | 无 | `ProxyStatus`；CLIProxy 页面状态探测/刷新 |
| `src/pages/ProxyPage.tsx:42` | `select_cli_dir` | 无 | `string`；空串表示用户取消 |
| `src/pages/ProxyPage.tsx:56` | `set_cli_dir` | `dir` | `string`；清空时明确发送 `{ dir: "" }` |
| `src/pages/ProxyPage.tsx:69` | `start_cliproxyapi` | 无 | `string`；启动 CLIProxyAPI |
| `src/pages/ProxyPage.tsx:82` | `stop_cliproxyapi` | 无 | `string`；停止 CLIProxyAPI |
| `src/pages/NvidiaPage.tsx:75` | `nvidia_status` | 无 | `NvidiaStatus`；状态探测/刷新 |
| `src/pages/NvidiaPage.tsx:109,171,201` | `set_nvidia_config` | `nvidia` | `string`；保存、启动前保存、连接测试前保存 |
| `src/pages/NvidiaPage.tsx:125` | `nvidia_set_models` | `models` | `string`；模型增删/移动后立即持久化并热更新 |
| `src/pages/NvidiaPage.tsx:173` | `nvidia_start` | 无 | `string`；启动 NVIDIA 代理 |
| `src/pages/NvidiaPage.tsx:186` | `nvidia_stop` | 无 | `string`；停止 NVIDIA 代理 |
| `src/pages/NvidiaPage.tsx:203` | `nvidia_test` | 无 | `string`；测试保存后的当前配置 |
| `src/pages/NvidiaPage.tsx:226` | `nvidia_key_pool` | 无 | `KeyPoolStatus`；Key 池初始读取及运行时轮询 |
| `src/pages/NvidiaPage.tsx:250` | `nvidia_chat_test` | `model`, `prompt` | `string`；基线固定发送 `prompt: null` |
| `src/pages/LogPage.tsx:44` | `list_log_files` | 无 | `{ name, size }[]`；后端白名单内的历史日志 |
| `src/pages/LogPage.tsx:54` | `get_logs` | 无 | `string[]`；读取实时内存缓冲 |
| `src/pages/LogPage.tsx:57` | `get_log_level` | 无 | `INFO \| WARN \| ERROR` |
| `src/pages/LogPage.tsx:80` | `set_log_level` | `level` | `INFO \| WARN \| ERROR`；以后端返回值更新 UI |
| `src/pages/LogPage.tsx:90` | `clear_logs` | 无 | 无业务 payload；只清实时缓冲 |
| `src/pages/LogPage.tsx:109` | `read_log_file` | `name` | `string`；文件名必须来自后端列表 |

`set_nvidia_config` 的顶层 payload 只有 `nvidia`。基线中的嵌套
`NvidiaConfig` 字段为：

```text
api_keys
models
base_url
host
port
key_cooldown_seconds
max_retries
request_timeout_seconds
auth_token
```

`set_profiles` 中每个 Profile 保持 `{ name, env }` 结构；保存前会过滤
空变量名，但不会把密钥显示层的掩码写回真实值。

## Event 清单

| 文件（基线行） | API | 事件名 | payload | 生命周期 |
|---|---|---|---|---|
| `src/pages/LogPage.tsx:62` | `listen<string>` | `nvidia-log` | 单条日志字符串 | Log 页挂载时订阅；卸载时调用返回的 `UnlistenFn` |

不允许把整个高频日志内容设置为 live region；事件仍只负责追加实时日志，
最多保留 2,000 行。

## 关键调用顺序

以下顺序属于业务契约，UI 重构不得因抽取组件、统一 busy state 或错误处理而改变：

1. **App 初始化**
   - `get_config`
   - 成功后初始化跨菜单草稿并渲染业务页面。

2. **选用历史工作目录**
   - 前端先更新当前 `Config.work_dir` 展示；
   - `set_work_dir({ dir })`
   - `add_recent_dir({ dir })`
   - `get_recent_dirs`
   - 删除历史只调用 `remove_recent_dir({ dir })`，不得调用任何磁盘删除命令。

3. **选择新的工作目录**
   - `select_directory`
   - 返回非空目录后更新当前 `Config.work_dir` 展示；
   - `get_recent_dirs`
   - 基线不在前端追加 `set_work_dir` / `add_recent_dir`，因为
     `select_directory` 后端命令已经同步配置与历史；空串取消保持 no-op。

4. **启动 Claude**
   - `launch_claude({ yolo, env })`
   - 仅成功后 `get_recent_dirs` 刷新历史。
   - 连接参数只通过 `env` 注入本次进程，不改写 `settings.json`。

5. **CLIProxyAPI**
   - 页面挂载/手动刷新：`cliproxyapi_status`。
   - 启动：`start_cliproxyapi`，成功后 `cliproxyapi_status`。
   - 停止：`stop_cliproxyapi`，成功后 `cliproxyapi_status`。
   - 选择执行目录：`select_cli_dir`；返回非空目录后直接更新 UI/App
     配置，基线不再追加 `set_cli_dir`，因为选择命令已经持久化。
   - 清空执行目录：`set_cli_dir({ dir: "" })`。

6. **保存配置**
   - 全局配置：`set_config(...)` 成功后才更新 App 中已提交的 `Config`。
   - Profiles：`set_profiles({ profiles })` 成功后才更新 App 中已提交的
     `Config.profiles`。

7. **NVIDIA 配置与启动**
   - 普通保存：`set_nvidia_config({ nvidia })`。
   - 启动：`set_nvidia_config({ nvidia })` 成功后，
     `nvidia_start`，随后 `nvidia_status`。
   - 停止：`nvidia_stop`，随后 `nvidia_status`。
   - 连接测试：`set_nvidia_config({ nvidia })` 成功后，
     `nvidia_test`。
   - 模型排序/增删：先更新本地模型与 App 配置快照，再立即
     `nvidia_set_models({ models })`；不得改成等待普通“保存”。
   - 单模型测试：`nvidia_chat_test({ model, prompt: null })`。

8. **NVIDIA Key 池**
   - 页面挂载时读取一次 `nvidia_key_pool`。
   - 代理运行时保持一个 2 秒轮询；停止运行或页面卸载时清理 interval。

9. **日志**
   - 挂载时发起 `get_logs`、`get_log_level`、`list_log_files` 并注册
     `listen("nvidia-log")`；这些初始化工作互不串行等待。
   - `set_log_level({ level })` 成功后使用后端返回的等级更新 UI。
   - `clear_logs` 成功后才清空当前显示；不得删除磁盘历史文件。
   - `read_log_file({ name })` 的 `name` 必须来自 `list_log_files` 结果。

## 最终核对方法

Task 9 必须同时检查已提交范围和未提交工作树：

```powershell
git diff -U3 50808c5..HEAD -- src/App.tsx src/pages src/components
git diff -U3 -- src/App.tsx src/pages src/components
git diff --cached -U3 -- src/App.tsx src/pages src/components
rg -n 'invoke<|invoke\(|listen<' src/App.tsx src/pages src/components
```

逐行核对本文件的 29 个 invoke、1 个 event、payload 键及关键顺序。
新增、删除、改名、改变 payload、改变顺序或新增轮询/监听，均必须视为业务
契约差异；若不是恢复基线行为的修复，应停止并取得用户明确确认。
