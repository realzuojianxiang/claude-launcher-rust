# 重构前源码基线清单

记录时间：2026-07-31 13:20 +08:00。

Git 基线：

```text
branch: feat/rust-implementation
commit: 50808c5
subject: docs/polish: tighten CSP, add rust/ CLAUDE.md, fix root doc, a11y labels
```

由于应用内浏览器安全策略拒绝本地 URL，本清单用于确保重构前界面可以从确定的源码状态复现；它不能替代运行截图。

| 文件 | SHA-256 |
|---|---|
| `src/App.tsx` | `435cf40a4570919e97e66b3cfabdf240dba92a8cdbc2dd29f345c9668c9b5807` |
| `src/styles.css` | `8c6f07de8606cb59b34107c0354b6f6e5dac7057c367d075f7d973486cc4716a` |
| `src/components/Sidebar.tsx` | `60930e216e1603ee21da07cdd429a30d0a67afab8b5c07c11f7588544dc8bd51` |
| `src/pages/LaunchPage.tsx` | `70fd69ac727fa7915874604cd594b2f7e1e5dd894c1e7220a0405962c10044ac` |
| `src/pages/ProxyPage.tsx` | `afec6acef641694912300be76025d0bdbe5449125f38245eb48b05702bdc9fe4` |
| `src/pages/NvidiaPage.tsx` | `805d611ff1ce5daeb855ac98ed306539fd8dce04e28ce5d4a6433160ad9276a9` |
| `src/pages/LogPage.tsx` | `9858ac0c9400bde1214115fb2ebb58207f31c7e88921611406f2e46df463b682` |
| `src/pages/ConfigPage.tsx` | `f0de62513396bb8eda8a298c148188e6011ea92bd4dc726b11399a49943d429b` |
| `src/pages/DictionaryPage.tsx` | `0b023de345e25bbebabb3617f827c737b4def7092e3fe5ca5fff00fbfc27d4f5` |
| `src/pages/AboutPage.tsx` | `95f212b5b890cf34696612999a184459eb88a326ac77b72b52429783a0dc8c50` |
| `index.html` | `3e6501b8f733360cb457ac7f94c0350fe8af2e7990cbe5ce31e342997c988a3a` |

复现约束：

- 使用 Node ≥ 24。
- 在 `rust/` 目录执行 `npm.cmd run build`。
- 真实 IPC 与桌面窗口必须通过 Tauri dev/build 环境运行。
- 不把当前 `dist/` 当作唯一基线，因为 Vite 配置了 `emptyOutDir: false`。
