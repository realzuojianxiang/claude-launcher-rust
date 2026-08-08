---
marp: true
theme: default
paginate: true
size: 16:9
backgroundColor: #0b1220
color: #e6edf3
style: |
  section {
    background: linear-gradient(135deg, #0b1220 0%, #111a2e 100%);
    font-family: "PingFang SC", "Microsoft YaHei", "Helvetica Neue", sans-serif;
    padding: 60px 70px;
  }
  section h1 {
    color: #7fd1ff;
    font-size: 42px;
    border-bottom: 2px solid #2a3f5f;
    padding-bottom: 12px;
    margin-bottom: 28px;
  }
  section h2 {
    color: #9bb7d4;
    font-size: 30px;
    margin-bottom: 18px;
  }
  section h3 {
    color: #c8d4e0;
    font-size: 22px;
    margin-top: 22px;
  }
  section table {
    margin: 18px 0;
    font-size: 19px;
    color: #d7e0ea;
  }
  section table th {
    background: #1d2b45;
    color: #7fd1ff;
    padding: 10px 14px;
  }
  section table td {
    padding: 8px 14px;
    border-bottom: 1px solid #1c2a42;
  }
  .bad { color: #ff8b8b; }
  .good { color: #8be9a8; }
  .muted { color: #7f8fa6; }
  .kw { color: #ffd479; }
  pre {
    background: #0d1626 !important;
    border: 1px solid #1c2a42;
    border-radius: 8px;
    font-size: 16px;
  }
  .lead {
    text-align: center;
    margin-top: 120px;
  }
  .pill {
    display: inline-block;
    background: #1d2b45;
    color: #7fd1ff;
    padding: 4px 12px;
    border-radius: 999px;
    font-size: 18px;
    margin: 0 6px;
  }
---

<!-- _class: lead -->

# 自主可控的 Code Agent 体系

让企业把 AI 写代码的能力，留在自己的机房里。

<br>

<span class="muted">种子轮路演 · 2026</span>

<sub>主体：自建 code agent 体系 · 形态：从桌面 agent 起步，向企业私有部署演进</sub>

---

## 问题：开发者都在用 AI 写代码，但企业用不上

<br>

- 开发者侧爆发：AI 编程已成默认工作方式

- 但**企业私有代码上不了云** —— 合规、数据安全、知识产权三重门

- 主流方案（Cursor / Copilot / Windsurf ...）<span class="bad">全部要求代码出户</span>

- 本应最受惠于 AI 编程的群体 —— 银行、政企、军工、医疗、科研 ——
  <span class="bad">反而被主流生态拒之门外</span>

<br>

> 一句话：<span class="kw">越不能上云的企业，越需要 AI 编程能力 —— 却越没有方案。</span>

---

## 为什么是现在

<br>

| 变化 | 含义 |
|---|---|
| 开源大模型 + 推理成本急速下降 | 企业**私有部署大模型第一次现实可行** |
| 国产模型（深度求索 / NVIDIA NIM 等可在本地） | 「自主可控」从口号变成可工程落地 |
| Agent 范式成熟（ReAct / tool use 协议标准化） | 私有模型也能驱动 agent，不再是云端专利 |
| 客户侧合规收紧（数据出境、信创、等保） | 「不上云」从可选变刚需 |

<br>

<span class="kw">技术拐点 × 合规拐点同时在 2026 汇合 — 窗口期就在当下。</span>

---

<!-- _class: lead -->

## 我们的路线

<div style="text-align:left;">

**今天（已造）**
桌面 agent：自建 code agent 内核跑通，多 provider 配置驱动

**下一步（资金到位）**
企业版：私有模型对接 · 团队协作 · 权限审计 · 数据治理

**终局**
自主可控的企业级 AI 编程平台

</div>

<br>

> 演进型：个人桌面版是<span class="kw">技术能力实证</span>，
> 融资推动我们把它演进为<span class="kw">企业私有部署平台</span>。

---

## 已造的 Code Agent 内核：自主调工具完成真实任务

我们自建了完整的 agent 内核 —— 不是在框架外壳上拼装：

<br>

<table>
<tr><th>能力</th><th>实证</th></tr>
<tr><td>能调通主流大模型</td><td class="good">DeepSeek / NVIDIA NIM 双跑通（一键配置切换，零代码改）</td></tr>
<tr><td>能自主决策调工具</td><td class="good">已实测模型主动 tool call，无需强制指令</td></tr>
<tr><td>多 provider 协议通透</td><td class="good">探针实测两家 tool use 协议差异，落地可移植解析</td></tr>
<tr><td>安全工程实践</td><td class="good">配置/密钥分离 · 原子写 · SSRF 闸 · 重定向禁止 · 命令注入防御</td></tr>
</table>

<br>

<span class="muted">上述每一项已在代码与文档中可复现，附 journey 建造日志与概念教程。</span>

---

## 技术关键：为什么别人抄不走

<br>

- **配置驱动多 provider**：换底层模型只改配置不改代码 —— 模型更替时不会过时

- **协议层通透，不套框架黑盒**：自研 ReAct 循环，tool use 解析经实测定型
  （主流框架在多 provider 协议分岔处首跑即翻，我们提前避雷）

- **安全基线从 day 0 内建**：企业客户的硬门槛，事后补补不出来

- **关键技术横跨纵深**：从启动器、本地代理、密钥治理、到 agent 内核，全栈自研

<br>

> 不靠某个模型或某段 prompt 的奇技，靠的是<span class="kw">把 agent 内核做透</span> —— 这正是框架替你跳过的、最难抄的那一层。

---

## 在生态中的位置

<br>

| 派别 | 代表 | 是否企业私有部署 |
|---|---|---|
| 闭源个人 SaaS | Cursor / Copilot / Windsurf / Cody | 否（代码必须上云）|
| 开源 / 自主可控 agent | Cline / Aider / Continue / 各家国产 | 部分，多为个人向 |
| 大厂企业版编程助手 | 各家云厂「企业 coding 助手」| 是，但绑定其自家电  |
| **我们** | **自建 code agent 体系** | **是 · 不绑定单一模型 · 可对接客户私有模型** |

<br>

<span class="muted">{TODO: 你心里的竞品名单补全后，把这张表对齐你的事实认知 —— 上表为代表分类，最终以你掌握为准}</span>

---

## 市场规模 (TAM / SAM / SOM)

<br>

<div style="font-size:22px;">

- **TAM** 全球 AI 编程工具市场：{TODO: 数字 + 来源}
- **SAM** 其中可私有部署 / 企业采购部分：{TODO: 数字 + 推算}
- **SOM** 国产自主可控 + 我们可达：{TODO: 数字 + 假设}

</div>

<br>

> <span class="muted">{TODO: 这一页所有数字必须可溯源。种子轮可弱数字，但不可编 —— 需你回填真料或标「估算」。</span>
> <br>提示取数方向：AI coding 市场研究（Gartner/IDC/各家咨询）+ 中国信创/等保市场口径 + 国产大模型企业私有部署渗透。}

---

## 商业模式（种子轮方向感）

<br>

<div style="font-size:22px;">

- **授权 / 私有部署费**：企业一次性 + 年维护

- **按席位订阅**：团队协作版按开发者席位

- **模型对接 / 集成交付**：对接客户私有模型的工程服务（高客单）

- **长期演进**：从私有部署切入客户的 agent 平台生态位

</div>

<br>

> <span class="muted">{TODO: 你倾向的主线商业模式。种子轮可以是「方向感」而非完整单位经济，但需要你定下主航道 —— 上面为合理候选，需你确认或改写。}</span>

---

## 团队

<br>

<div style="font-size:24px;">

- **{TODO: 姓名}** —— {TODO: 角色，主导方向}

  {TODO: 一条值得给投资人看的过往背景 / 项目 / 信用背书}

- {TODO: 其他核心成员，或留位「正在补全」}

</div>

<br>

> <span class="bad">路演中团队是重头，这一页必须真料。</span>
> <br><span class="muted">把你真实的姓名 / 过往职务或项目 / 技术信用 / 合作背景给我，我据此润色。
> 若暂不便公开，至少给投资人版本里能站住的「为什么是你们」一句话。</span>

---

## 路线图

<br>

```
已达成:  P0 调通模型 · P0.5 配置驱动多 provider · P1 agent 调工具跑通(实测两家协议)
进行中:  P2 多轮循环 · P3 工具集 · P4 权限审批
近期:    P5 流式 · P6 上下文管理 · P7 会话持久化
企业演进: 私有模型对接 · 团队协作 · 审计治理 · 试点客户
```

<br>

> <span class="muted">技术里程碑已在 journey 日志可追溯；商业里程碑见融资条款页。</span>

---

## 融资条款

<br>

<div style="font-size:24px;">

- **本轮融资**：{TODO: 金额人民币}（种子轮 / 天使）

- **资金用途**：
  - 企业版研发（agent 平台化、权限审计、协作）：{TODO: 占比}
  - 试点客户对接 / 模型集成交付：{TODO: 占比}
  - 团队补全：{TODO: 占比}
  - 运转储备：{TODO: 占比}

- **可讲述里程碑**（12-18 个月）：{TODO: 关键节点 1-3 个}

- **估值期待**：{TODO: 或「希望与正式投资人共同厘定」}

</div>

<br>

> <span class="muted">{TODO: 所有金额粒度需你回填。种子轮对金额留白不致命，但「用钱能给到什么里程碑」一定要想清楚。}</span>

---

<!-- _class: lead -->

# 让企业把 AI 编程能力留在自己的机房里。

<br>

<span class="muted">谢谢 —— 期待与您共同把这条线跑通。</span>

<br><br>

<sub>{TODO: 联系方式 / 二维码 / 邮箱}</sub>
