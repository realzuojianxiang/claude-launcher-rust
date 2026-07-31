# UI 设计 Token

> 版本：0.2（已在 Task 2 落地到 `src/styles.css`；`:root`/light/dark 全集 + 语义组件 class）
>
> 建立时间：2026-07-31 13:13:30 +08:00
>
> 设计语言：Apple HIG / macOS Ventura / iOS 17，面向 Windows Tauri 桌面窗口

## 命名原则

- Token 只表达语义，不按具体页面命名。
- 组件不得直接写十六进制颜色、随机圆角、随机阴影或自定义缓动。
- `surface` 表示层级，`text` 表示文字语义，`control` 表示交互状态。
- 浅色为 `:root`，深色由 `@media (prefers-color-scheme: dark)` 覆盖。
- `[data-theme="light"]` / `[data-theme="dark"]` 必须能显式覆盖系统主题；没有属性时默认跟随系统。

## 颜色

### 浅色

| Token | 值 | 用途 |
|---|---:|---|
| `--color-canvas` | `#F2F2F7` | 应用主背景 |
| `--color-surface` | `#FFFFFF` | 主要卡片/分组表面 |
| `--color-surface-secondary` | `#F9F9FB` | 次级分组、只读区域 |
| `--color-surface-elevated` | `#FFFFFF` | Modal/Popover |
| `--color-glass` | `rgba(246, 246, 249, 0.82)` | 顶栏、侧栏 |
| `--color-text-primary` | `#1C1C1E` | 标题、正文 |
| `--color-text-secondary` | `#3A3A3C` | 次级正文 |
| `--color-text-tertiary` | `#636366` | 提示、说明、占位 |
| `--color-border` | `rgba(60, 60, 67, 0.18)` | 普通分隔 |
| `--color-border-strong` | `rgba(60, 60, 67, 0.30)` | 输入/重点边框 |
| `--color-accent` | `#007AFF` | Apple system blue、焦点、选中 |
| `--color-control-primary` | `#0066CC` | 主按钮背景 |
| `--color-on-accent` | `#FFFFFF` | 主按钮文字/图标 |
| `--color-accent-soft` | `rgba(0, 122, 255, 0.10)` | 选中背景 |
| `--color-success` | `#248A3D` | 成功文本 |
| `--color-success-fill` | `#1E7A35` | 成功按钮/大面积状态 |
| `--color-on-success` | `#FFFFFF` | 成功填充上的文字 |
| `--color-warning` | `#9A5B00` | 警告文本 |
| `--color-warning-fill` | `#995700` | 警告按钮/大面积状态 |
| `--color-on-warning` | `#FFFFFF` | 警告填充上的文字 |
| `--color-danger` | `#D70015` | 错误文本/危险操作 |
| `--color-danger-fill` | `#D70015` | 危险按钮背景 |
| `--color-on-danger` | `#FFFFFF` | 危险按钮文字/图标 |
| `--color-code-bg` | `#1C1C1E` | 日志/代码区 |
| `--color-code-fg` | `#F2F2F7` | 日志/代码文字 |

### 深色

| Token | 值 | 用途 |
|---|---:|---|
| `--color-canvas` | `#000000` | 应用主背景 |
| `--color-surface` | `#1C1C1E` | 主要卡片/分组表面 |
| `--color-surface-secondary` | `#2C2C2E` | 次级分组、只读区域 |
| `--color-surface-elevated` | `#2C2C2E` | Modal/Popover |
| `--color-glass` | `rgba(28, 28, 30, 0.84)` | 顶栏、侧栏 |
| `--color-text-primary` | `#F5F5F7` | 标题、正文 |
| `--color-text-secondary` | `#D1D1D6` | 次级正文 |
| `--color-text-tertiary` | `#AEAEB2` | 提示、说明、占位 |
| `--color-border` | `rgba(235, 235, 245, 0.18)` | 普通分隔 |
| `--color-border-strong` | `rgba(235, 235, 245, 0.30)` | 输入/重点边框 |
| `--color-accent` | `#0A84FF` | Apple dark system blue |
| `--color-control-primary` | `#0A84FF` | 深色主按钮 |
| `--color-on-accent` | `#000000` | 深色主按钮文字/图标 |
| `--color-accent-soft` | `rgba(10, 132, 255, 0.18)` | 选中背景 |
| `--color-success` | `#30D158` | 成功 |
| `--color-success-fill` | `#30D158` | 成功按钮/大面积状态 |
| `--color-on-success` | `#000000` | 成功填充上的文字 |
| `--color-warning` | `#FF9F0A` | 警告 |
| `--color-warning-fill` | `#FF9F0A` | 警告按钮/大面积状态 |
| `--color-on-warning` | `#000000` | 警告填充上的文字 |
| `--color-danger` | `#FF453A` | 错误/危险 |
| `--color-danger-fill` | `#FF453A` | 危险按钮背景 |
| `--color-on-danger` | `#000000` | 危险按钮文字/图标 |
| `--color-code-bg` | `#111113` | 日志/代码区 |
| `--color-code-fg` | `#F2F2F7` | 日志/代码文字 |

### 对比度证据

按 WCAG 相对亮度公式计算，正常文字门槛使用 4.5:1：

| 背景 | 前景 | 对比度 |
|---|---|---:|
| Light primary `#0066CC` | `#FFFFFF` | 5.57:1 |
| Dark primary `#0A84FF` | `#000000` | 5.76:1 |
| Light danger `#D70015` | `#FFFFFF` | 5.38:1 |
| Dark danger `#FF453A` | `#000000` | 6.16:1 |
| Light success `#1E7A35` | `#FFFFFF` | 5.40:1 |
| Dark success `#30D158` | `#000000` | 10.39:1 |
| Light warning `#995700` | `#FFFFFF` | 5.65:1 |
| Dark warning `#FF9F0A` | `#000000` | 10.22:1 |
| Light tertiary text `#636366` | `#FFFFFF` | 5.99:1 |
| Dark tertiary text `#AEAEB2` | `#000000` | 9.50:1 |

系统亮色 `#007AFF` 与亮色系统红 `#FF3B30` 仍可用于图形、焦点与非文字强调；正常字号的填充控件使用上述 `control/fill + on-*` 配对。

## 字体与字号

字体栈：

```css
-apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display",
"Segoe UI", "Microsoft YaHei", "PingFang SC", sans-serif
```

等宽字体：

```css
"SF Mono", "SFMono-Regular", Consolas, "Liberation Mono", monospace
```

| Token | 大小/行高 | 用途 |
|---|---|---|
| `--font-size-title-lg` | `28px / 1.2` | 页面大标题 |
| `--font-size-title` | `20px / 1.3` | 卡片/分区标题 |
| `--font-size-body` | `15px / 1.5` | 正文 |
| `--font-size-control` | `14px / 1.3` | 按钮、输入 |
| `--font-size-caption` | `12px / 1.45` | 说明与元数据 |
| `--font-weight-regular` | `400` | 正文 |
| `--font-weight-medium` | `500` | 控件、次级标题 |
| `--font-weight-semibold` | `600` | 标题、强调 |
| `--font-weight-bold` | `700` | 关键数字 |

禁止使用无依据的 `650` 等非标准字重。

## 间距

8pt 基线，允许 4pt 半步：

| Token | 值 |
|---|---:|
| `--space-1` | `4px` |
| `--space-2` | `8px` |
| `--space-3` | `12px` |
| `--space-4` | `16px` |
| `--space-5` | `20px` |
| `--space-6` | `24px` |
| `--space-8` | `32px` |
| `--space-10` | `40px` |
| `--space-12` | `48px` |
| `--space-16` | `64px` |

## 圆角

| Token | 值 | 用途 |
|---|---:|---|
| `--radius-control` | `10px` | 输入、小按钮 |
| `--radius-button` | `12px` | 主要按钮 |
| `--radius-card` | `16px` | 卡片、分组 |
| `--radius-dialog` | `18px` | Modal/Sheet |
| `--radius-pill` | `999px` | Badge/Segment |

不得在组件中新增无来源的 6/7/8/14px 等随机圆角。

## 阴影与层级

| Token | 值 | 用途 |
|---|---|---|
| `--shadow-card` | `0 1px 2px rgba(0,0,0,.04), 0 8px 24px rgba(0,0,0,.06)` | 普通卡片 |
| `--shadow-elevated` | `0 12px 36px rgba(0,0,0,.18), 0 2px 8px rgba(0,0,0,.08)` | Modal/Popover |
| `--shadow-focus` | `0 0 0 3px rgba(0,122,255,.28)` | 键盘焦点 |
| `--z-nav` | `10` | 顶栏/侧栏 |
| `--z-overlay` | `100` | 遮罩 |
| `--z-dialog` | `110` | 对话框 |

静态卡片不因 hover 上浮；只有明确可点击卡片允许轻微背景或边框反馈。

## 动效

| Token | 值 | 用途 |
|---|---:|---|
| `--duration-fast` | `160ms` | hover/focus |
| `--duration-normal` | `240ms` | 展开、状态切换 |
| `--duration-slow` | `320ms` | Modal/页面级过渡 |
| `--ease-standard` | `cubic-bezier(0.25, 0.1, 0.25, 1)` | 常规过渡 |
| `--ease-decelerate` | `cubic-bezier(0.16, 1, 0.3, 1)` | 进入 |

- 禁止常规交互超过 400ms。
- 禁止无限装饰性脉冲。
- `prefers-reduced-motion: reduce` 下取消 transform 与非必要 animation，只保留不超过 160ms 的 opacity 状态反馈。

## 尺寸与响应式

| Token | 值 | 用途 |
|---|---:|---|
| `--control-height` | `44px` | 标准交互目标 |
| `--control-height-compact` | `36px` | 仅视觉紧凑控件；外层点击区仍需 44px |
| `--sidebar-expanded` | `248px` | 展开侧栏 |
| `--sidebar-collapsed` | `72px` | 折叠侧栏 |
| `--content-max` | `1040px` | 页面最大宽度 |

断点：

- `≤ 1024px`：内容边距收紧，仪表盘允许 2 列。
- `≤ 768px`：单列卡片，表单行与工具条可换行，侧栏默认紧凑。
- `≤ 560px`：内容边距 16px，按钮行纵向或占满宽度，Modal 贴近 Sheet。

## 组件状态规范

### Button

- Primary：`accent-strong`，明确主动作。
- Secondary：surface + border。
- Ghost：透明背景，仅用于低层级操作。
- Danger：危险 Token，必须有明确文字或 accessible name。
- Loading：保留原宽度、禁用重复提交、展示 Spinner 与进行中文案、`aria-busy=true`。
- Disabled：降低对比但保持可读；不得仅用 opacity 让文字低于可读阈值。
- Hover/Active/Focus：分别使用背景、1–2px 视觉按压与 focus ring；reduced motion 下移除缩放。

### Input / Select / Textarea

- 高度至少 44px。
- label 必须通过 `htmlFor` 关联。
- hint/error 通过 `aria-describedby` 关联。
- Error 使用边框 + 图标/文字，不只靠红色。
- Readonly 与 Disabled 必须视觉区分。
- 密钥默认 `type=password`，显隐按钮有 `aria-pressed`。

### Card / Group

- 普通 Card 无装饰性 hover。
- 交互 Card 必须是 button/link 语义，具备 hover/focus/active。
- 分组列表优先使用内部 divider，而不是每行独立厚边框。

### Async State

- Loading：明确对象与动作，如“正在读取日志”，优先稳定骨架或 Spinner，避免布局跳动。
- Empty：说明“为什么为空”以及可用的下一步。
- Error：说明失败对象、温和原因、重试/恢复操作。
- Permission：说明需要的权限与可恢复路径。
- Network：区分本地代理未运行、连接失败与未知错误。
- Submitting：锁定重复提交，完成后保留成功/失败反馈。

### Dialog

- `role=dialog`、`aria-modal=true`、可访问标题。
- 打开时移动焦点，Tab/Shift+Tab 圈禁，Esc 关闭。
- 遮罩关闭规则由调用方明确；关闭后恢复触发元素焦点。
- 深色/窄屏/减少透明度均有降级。
