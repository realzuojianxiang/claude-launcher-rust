# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 仓库性质

本仓库是 Claude Launcher（快速启动 Claude Code + CLIProxyAPI 的桌面启动器）的**多语言实现集合**，每种语言实现一个独立子目录（当前含 `golang/`，后续会新增如 `rust/` 等，目录名随开发可能增减/变更）。请用语言实现所在的子目录为工作根，进入对应子目录后再看该目录下的 CLAUDE.md 获取构建命令与架构说明：

```bash
cd golang        # 或当前实际存在的语言实现目录
```

不要在仓库根直接构建或运行——根目录只做实现的容器，不含可编译代码。各子目录的 CLAUDE.md 优先级高于本文件。
