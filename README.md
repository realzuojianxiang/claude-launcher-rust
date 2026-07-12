# Claude Launcher

一个用于快速启动 Claude Code 并自动配置 CLIProxyAPI 的桌面启动器。

## 功能

- 🚀 一键启动 Claude Code
- ⚙️ 自动修改 `~/.claude/settings.json` 配置
- 🔄 退出时自动还原原始配置
- 💾 配置持久化保存

## 构建

需要安装 [Wails](https://wails.io/) 和 Go。

```bash
# 安装 Wails
go install github.com/wailsapp/wails/v2/cmd/wails@latest

# 开发模式
wails dev

# 构建生产版本
wails build
```

构建后的可执行文件位于 `build/bin/claude-launcher.exe`。

## 配置

应用配置保存在 `~/.claude-launcher/config.json`。

Claude Code 的 settings.json 在启动时会被修改，退出时自动还原。

## CLIProxyAPI

需要配合 [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) 使用，用于将 Claude API 请求转发到其他 LLM 服务。

## License

MIT