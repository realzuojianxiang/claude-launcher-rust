# 路演 PPT（种子轮 · Code Agent 体系）

这份是给投资人路的种子轮 PPT 源，用 [Marp](https://marp.app/) 写。**Marp 是 Markdown 写幻灯片**——`---` 分页，VSCode 装 Marp 插件即可一键导出 PDF / PPTX / HTML。

## 主体定位

- **主体**：自建 code agent 体系
- **市场**：企业私有部署（私有模型 + 数据不出户）
- **竞品视角**：闭源 SaaS 派作「痛点放大器」（它们逼企业上云 → 留下私部署空白 → 留给我们），同台对手我们侧重企业级
- **叙事**：演进型（桌面 agent 是技术实证，融资推动演进为企业平台）
- **团队页**：默认个人 / 小团队，需你回填真料

## 本地渲染成 PPT

```powershell
# 方法一（推荐，可视化）：VSCode 装 "Marp for VS Code" 扩展
#   打开 pitch.md，右上角点「Export slide deck」→ 选 PDF / PPTX
#   改完随时再导。最贴合迭代节奏。

# 方法二（命令行）：装 marp-cli 一次性出活
npm install -g @marp-team/marp-cli
cd D:\BaiduSyncdisk\ai-agent\claude-launcher\codeagent-rs\deck
marp pitch.md --pdf            # 出 pitch.pdf
marp pitch.md --pptx           # 出 pitch.pptx（可直接 PowerPoint 打开再微调）
marp pitch.md --html           # 出网页版，浏览器全屏讲
```

## 文件

| 文件 | 作用 |
|---|---|
| `pitch.md` | 11 页路演骨架（Marp 源） |
| `backfill.md` | **待你回填的真料清单**——按它填，骨架就不会塌 |

## 内容 vs 现实的一致性纪律

- 凡我在 PPT 里写了「已造 / 已实测」的，都来自 `codeagent-rs/docs/codeagent-journey.md` 真实记录的 P0/P0.5/P1 进度。你不会被投资人追问到「这个你们真做了吗」时露馅。
- 凡我没把握的（市场数字、竞品事实、团队、融资条款），都打 `{TODO: ...}` 占位 —— **绝不编数字**。种子轮对留白容忍度高，对编造零容忍。

## 待你做的两件事

1. 跟着 `backfill.md` 回填真料（关系到『团队』『竞品』『市场』『商业模式』『融资条款』五页）
2. 本地导出 PDF / PPTX，看整体观感，再回头调文案

回填后把文件给我，我帮你把五页润色贴合你的事实口径。
