# 重构前 production build 基线

记录时间：2026-07-31 13:08 +08:00。

命令：

```powershell
npm.cmd run build
```

结果：

- `tsc` 通过。
- Vite 8.1.5 通过。
- 1,811 个模块转换。
- 构建耗时 3.50s。
- Vite 配置 `emptyOutDir: false`，下表只记录本次构建输出，不证明 `dist/` 没有其他历史文件。

| 产物 | 原始大小 | gzip |
|---|---:|---:|
| `dist/index.html` | 1.47 kB | 0.66 kB |
| `dist/assets/index-CNmfKhq5.css` | 34.86 kB | 7.71 kB |
| `dist/assets/MessageBanner-DnfxQHi9.js` | 0.28 kB | 0.24 kB |
| `dist/assets/ConfirmButton-iAcuRP7k.js` | 0.60 kB | 0.44 kB |
| `dist/assets/AboutPage-CXA_OzGV.js` | 0.99 kB | 0.46 kB |
| `dist/assets/DashboardPage-ByZzPZ6F.js` | 1.29 kB | 0.65 kB |
| `dist/assets/ProxyPage-CKQDmHVC.js` | 2.76 kB | 1.22 kB |
| `dist/assets/LaunchPage-MjOh77N3.js` | 4.56 kB | 2.03 kB |
| `dist/assets/LogPage-BYUKs9Sn.js` | 6.36 kB | 2.67 kB |
| `dist/assets/ConfigPage-CTyYyPYC.js` | 6.64 kB | 2.66 kB |
| `dist/assets/DictionaryPage-huMzKQM7.js` | 12.79 kB | 4.94 kB |
| `dist/assets/NvidiaPage-CPFY0QAh.js` | 16.28 kB | 5.92 kB |
| `dist/assets/spinner-verbs-Dn1rJ_fM.js` | 18.92 kB | 6.94 kB |
| `dist/assets/index-C7oWCX61.js` | 153.73 kB | 50.85 kB |
| `dist/assets/ielts-Db4PC6eF.js` | 467.26 kB | 163.86 kB |

最终性能 Gate：

- 对比最终 main JS、CSS 和各业务 chunk 的 gzip 大小。
- main JS 或 CSS 增长超过 10% 时必须解释并检查是否意外引入重复依赖或把懒加载数据拉入主包。
- 字典数据 chunk 的内容变化不属于本次 UI 重构范围。
