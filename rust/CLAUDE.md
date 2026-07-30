# CLAUDE.md — rust/ 实现

本子目录是 Claude Launcher 的 Rust（Tauri v2）桌面实现。仓库根 CLAUDE.md 已说明：**不要在仓库根直接构建或运行**，所有命令都在本子目录（`rust/`）内执行。

## 技术栈

- **桌面壳**：Tauri v2（`src-tauri/`，Rust 后端）+ React 18 + TypeScript 前端（`src/`）
- **前端构建**：Vite 8（Oxc minify）+ Tailwind v3 + lucide-react；测试用 Vitest + jsdom
- **Rust 后端**：axum 0.7（应用内 NVIDIA 代理服务）+ reqwest（rustls-tls，不依赖系统 OpenSSL）+ tokio + tracing
- 图标：仓库根 `icons/`（共享资源，已通过 tauri.conf.json `bundle.icon` 引用）

## 常用命令

> 全部在 `rust/` 子目录下执行。Node ≥ 24，Rust 稳定工具链。

```bash
# —— 前端 ——
npm install            # 安装前端依赖
npm run dev            # 仅起 Vite dev server（http://localhost:1420）
npm run build          # tsc 类型检查 + vite 生产构建 -> dist/
npm test               # vitest run（单次，前端单测）
npm run test:watch     # vitest 监听模式

# —— Tauri（前端+Rust 一体）——
npm run tauri dev      # 开发：先 npm run dev 起 Vite，再 cargo build 跑 Rust GUI
npm run tauri build    # 生产打包：先 npm run build，再 cargo build --release 产出安装包

# —— 仅 Rust 后端（在 src-tauri/ 内）——
cd src-tauri
cargo check --tests                  # 快速类型/编译检查（含测试）
cargo fmt --all -- --check           # 格式自检（CI 门禁）
cargo clippy --all-targets -- -D warnings   # lint 门禁（CI 视警告为错误）
cargo test --lib                     # 跑库单测（见下方「测试」说明）
```

前端与 Tauri 通过 `invoke()` 双向 IPC，**没有** webview 侧的 `fetch`/WebSocket/EventSource——本地端口（8317 CLIProxyAPI、127.0.0.1:8082 NVIDIA 代理）都是以环境变量注入 `claude` 子进程，不是 webview 发起的请求。因此 `tauri.conf.json` 的 CSP 设得很严（`script-src 'self'`，仅放行 Tauri IPC 的 `ipc:`）。

## 架构速览

入口 `src-tauri/src/lib.rs::run()`：注册 Tauri 命令、托盘菜单、窗口关闭转隐藏、日志层。所有前端可调用方法以 `#[tauri::command]` 暴露，集中在 `run()` 的 `invoke_handler!` 注册。

模块职责：

| 模块 | 职责 |
| --- | --- |
| `lib.rs` | Tauri 入口、命令注册、托盘/窗口策略、S4 损坏配置告警、诊断钩子（`NVIDIA_DIAG=1`） |
| `config.rs` | `Config`（anthropic_url/key、cliproxyapi_dir、yolo、compact 阈值、`profiles[]`、`nvidia`）读写；原子落盘（临时文件→flush→sync_all→rename）；损坏文件改名留证 + 非静默回退；`NvidiaConfig::validate_base_url`（SSRF 闸：scheme+host） |
| `claude.rs` | 启动 `claude` 子进程；**provider 并发隔离**：按完整 base URL 的稳定哈希派生**持久**目录 `claude-profiles/<host-slug>__<hash:016x>`，同地址跨启动复用（保留插件/MCP/hooks），不同地址隔离；`settings.json` 原子合并写；`validate_work_dir` 拒绝 cmd 元字符/不存在/非绝对路径，杜绝 .bat 注入 |
| `proxy.rs` | CLIProxyAPI 子进程的启动/停止/状态探测（`reqwest::blocking`） |
| `history.rs` | 最近工作目录持久化（`history.json`，上限 200）；原子写 + 损坏改名留证，与 `config.rs` 对齐 |
| `logger.rs` | 自定义 tracing 层：落盘 `logs/`（1MB 滚动、带时间戳）+ `nvidia-log` 事件推前端。**关键坑**：GUI 命令线程里向 stdout 写会死锁，绝不能加 stdout 层 |
| `nvidia/mod.rs` | `NvidiaState`：代理服务运行状态、start/stop、模型热更新、Key 池状态查询 |
| `nvidia/server.rs` | axum Router 装配，仅暴露 `/v1/messages`，挂 `DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES)` 兜底 |
| `nvidia/proxy.rs` | NVIDIA 代理核心：本地鉴权、**重定向禁止跟随**（`redirect::Policy::none()`，防 bearer 外泄）、上游响应流的 Anthropic↔Chat 转换转发、Key 池轮询、429 冷却、SSE 严格按行累积解码（防跨 chunk UTF-8 损坏） |
| `nvidia/converter.rs` | Anthropic Messages ↔ 上游协议的请求/响应体转换 |
| `nvidia/key_pool.rs` | 多 API Key 轮询与 429 冷却状态机 |
| `nvidia/models.rs` | 模型优先级列表 |

安全要点（详见 `CODE_REVIEW_2026-07-28.md`）：命令注入（work_dir→.bat）、SSRF + bearer 泄漏（重定向禁止 + 保存期校验 base_url）、配置/历史原子写、代理请求体上限（`MAX_REQUEST_BODY_BYTES = 32 MiB`，超限返回 Anthropic 形态 413）、默认绑回环 + 非回环强鉴权。

## 测试说明

- 前端：`npm test`（Vitest，前端纯逻辑/组件单测）。
- Rust：`cargo test --lib`。在本机直接跑库单测时，可能遇到 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`——这是 **Windows cdylib 运行时 DLL 加载的环境问题**（测试二进制能干净编译），不是代码/断言失败。`cargo check --tests` 通过即视为编译期门禁绿。
- CI 静态门禁（须全绿）：`cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo check --tests` + `npm run build` + `npm test`。

## 配置/数据落点

运行时数据都在「exe 同级 `claude-launcher/` 目录」下（`Config::config_dir()`）：`config.json`、`history.json`、`diag.txt`、`config.corrupt.notice.txt`、`logs/`、`claude-profiles/`（各 provider 隔离目录）。调试「启动 8082 卡死」可设环境变量 `NVIDIA_DIAG=1`，启动会自动走代理并把每步时间戳写入 `diag.txt`。
