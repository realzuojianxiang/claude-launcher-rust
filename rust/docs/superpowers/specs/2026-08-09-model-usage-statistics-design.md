# 模型使用统计与历史趋势设计

## 1. 目标

在现有 Claude Launcher Dashboard 中增加专业、可读的模型使用统计，覆盖：

- 输入 Token、输出 Token 与总 Token
- 逻辑请求数
- 最终失败请求数
- 额外重试次数
- 按 Provider、模型、时间范围查看趋势与排行

统计同时支持当前进程实时数据和跨重启历史数据。实现应保持现有 NVIDIA/Grok 代理的请求语义，不让统计故障影响代理请求。

## 2. 已确认的统计口径

### 2.1 逻辑请求

一次进入本地代理 `/v1/messages` 的请求视为一次逻辑请求。代理内部因 Key、模型、网络或流式前置失败产生的多次上游尝试，仍归属于同一个逻辑请求。

- 第一次上游尝试不计为重试。
- `retry_count` 是额外上游尝试次数。
- 最终成功：`failed = false`，无论中途是否失败。
- 最终失败：`failed = true`，失败请求数只增加 1。
- 最终成功的模型归属该请求；最终失败的请求归属最后一次尝试的模型。
- Token 只采信上游响应的 `usage` 字段，不用字符数或 SSE 帧数伪造精确 Token。
- 某次响应没有 usage 时，该次 Token 计为 0，并增加 `usage_missing_requests`，供界面提示用量可能不完整。

直接连接探测（`test_connection`）不经过本地 `/v1/messages`，不计入模型使用统计；通过本地代理执行的聊天测试属于真实代理请求，会正常计入。

### 2.2 时间范围

Dashboard 提供四个范围：

- `实时`：仅当前应用进程启动以来的数据；应用重启后清零。
- `近 7 天`：最近 7 个自然日的持久化数据，加当前进程数据。
- `近 30 天`：最近 30 个自然日的持久化数据，加当前进程数据。
- `全部`：统计文件中的全部历史数据，加当前进程数据。

默认打开 Dashboard 时显示 `近 7 天`，顶部实时状态与更新时间持续刷新。实时范围的趋势以当前进程的 15 分钟桶呈现；历史范围的趋势以自然日桶呈现。

## 3. 后端架构

### 3.1 独立统计模块

新增 `src-tauri/src/stats.rs`，不把聚合责任塞进 `NvidiaState` 或 `GrokState`。应用启动时创建一个共享的 `UsageStatsStore`，并通过 `Arc` 注入两个 Provider 的 `ProxyCtx`。

主要职责：

- 维护当前进程的实时聚合和 15 分钟趋势桶。
- 读取、合并并持久化历史日桶。
- 在一次逻辑请求结束时接收唯一的 `UsageRecord`。
- 按时间范围、Provider、模型生成前端快照。
- 将持久化错误与数据恢复状态暴露为可读的健康状态。

建议的核心结构：

```rust
struct UsageRecord {
    provider: String,
    requested_model: String,
    final_model: String,
    input_tokens: u64,
    output_tokens: u64,
    usage_available: bool,
    retry_count: u32,
    failed: bool,
    at: DateTime<Local>,
}

struct UsageStatsStore {
    inner: Mutex<StatsState>,
}
```

`StatsState` 包含当前进程聚合、当前进程 15 分钟桶和持久化日桶。聚合键至少包含 `provider + final_model`，避免同名模型在不同 Provider 间串台。

### 3.2 持久化

统计文件放在现有 `Config::config_dir()` 下，命名为 `usage-stats.json`。结构带 `version` 字段，按自然日保存模型级汇总，内容仅包含计数和 Token 数，不保存 prompt、响应正文、Key 或鉴权信息。

每次逻辑请求完成后更新内存并执行原子持久化：写入同目录临时文件、flush、sync、rename，沿用现有 `config.rs` / `history.rs` 的安全写盘模式。统计量规模按“日期 + Provider + 模型”增长，保持轻量。

历史文件损坏时：

1. 将原文件重命名为带时间戳的证据文件。
2. 以空历史启动，当前实时统计仍可用。
3. 记录 warning，并在 `get_usage_stats` 响应中返回 `history_recovered = true`。

如果写盘失败，保留内存结果并返回 `history_writable = false`；该错误不改变代理的 HTTP/SSE 响应。

### 3.3 请求收口

NVIDIA 与 Grok 的请求循环分别维护同一个逻辑请求上下文：

- 进入 `handle_messages` 时创建上下文。
- 每次上游尝试递增 attempt，并更新最后尝试模型。
- 非流式响应解析出 usage 后，在返回前记录一次。
- 流式响应由 stream 结束/异常收口函数记录一次，避免在每个 chunk 重复记录。
- 任何最终错误路径都只记录一次 `failed = true`。

流式路径必须把记录动作绑定到实际响应结束，不提前把“收到响应头”当作成功。这样既能正确统计最终失败，也不会因前置重试与流式交接重复计数。

### 3.4 Tauri 查询命令

新增 `get_usage_stats` 命令，输入时间范围，返回序列化快照。建议响应包含：

```ts
type UsageRange = "live" | "7d" | "30d" | "all";

interface UsageStatsSnapshot {
  range: UsageRange;
  generated_at: string;
  totals: {
    requests: number;
    input_tokens: number;
    output_tokens: number;
    total_tokens: number;
    failed_requests: number;
    retry_count: number;
    success_rate: number;
    usage_missing_requests: number;
  };
  trend: Array<{
    label: string;
    requests: number;
    input_tokens: number;
    output_tokens: number;
    total_tokens: number;
    failed_requests: number;
    retry_count: number;
  }>;
  models: Array<{
    provider: string;
    model: string;
    requests: number;
    input_tokens: number;
    output_tokens: number;
    total_tokens: number;
    failed_requests: number;
    retry_count: number;
    usage_missing_requests: number;
    success_rate: number;
  }>;
  providers: Array<{
    provider: string;
    requests: number;
    total_tokens: number;
    failed_requests: number;
    retry_count: number;
  }>;
  history_recovered: boolean;
  history_writable: boolean;
}
```

前端只依赖该快照，不直接读取统计文件。

## 4. Dashboard 视觉与交互

采用已确认的 A「专业数据台」方向，沿用现有 Apple/macOS 视觉 token、卡片、边框、暗色模式和响应式断点。

### 4.1 页面层级

1. 标题区：`使用统计`、一句说明、实时状态点、最近更新时间、刷新按钮。
2. 时间范围切换：`实时 / 近 7 天 / 近 30 天 / 全部`。
3. 四张指标卡：总 Token、请求数、失败请求、重试次数。
4. Token 趋势卡：输入/输出区分、按范围显示 15 分钟或日趋势、可悬浮查看详情。
5. 模型排行卡：按 Token 默认降序，支持按请求数、失败、重试和成功率排序。
6. Provider 摘要：NVIDIA/Grok 的请求量、Token 占比和失败/重试概况。

### 4.2 交互规则

- 首次打开立即调用 `get_usage_stats("7d")`。
- Dashboard 激活期间每 5 秒刷新；离开页面时停止定时器。
- 切换范围时立即重新查询，保留当前页面位置与排序偏好。
- 手动刷新按钮显示短暂忙碌态，重复点击被禁用。
- Token 使用 `K/M/B` 紧凑格式，悬浮或辅助文本提供完整数字。
- 缺失 usage 的模型行显示轻量提示，不使用红色错误样式误导为请求失败。
- 历史恢复或写盘失败显示非阻塞状态 Banner；实时统计仍继续展示。
- 没有数据时显示带说明的空状态，不用一组全是 0 的卡片制造噪声。

### 4.3 可访问性与响应式

- 图表同时提供文本摘要，颜色不是唯一语义来源。
- 时间筛选、刷新、排序均使用语义按钮并支持键盘操作。
- 1024px 以下指标卡降为两列，768px 以下降为单列。
- 模型表在窄屏保持内容完整并允许横向滚动。
- 遵循现有 `prefers-reduced-motion` 规则。

## 5. 测试策略

### 5.1 Rust 单元测试

- 成功请求正确累加输入/输出/总 Token。
- 最终失败只增加一个失败请求。
- 中间失败后最终成功只增加重试，不增加最终失败。
- 多模型 fallback 将记录归到最终成功模型；最终失败归到最后尝试模型。
- 实时快照只包含当前进程；7 天、30 天和全部范围正确合并历史与实时。
- 持久化文件 round-trip 保留聚合结果。
- 损坏文件恢复为证据文件并不阻塞实时快照。
- 写盘失败不会让记录动作返回代理错误。
- NVIDIA/Grok 非流式和流式 usage 都能正确收口。

### 5.2 前端测试

- 正常数据渲染四张指标卡、趋势和模型表。
- 空数据、加载中、统计读取失败、历史恢复、usage 缺失均有可见状态。
- 时间范围切换传入正确参数。
- 刷新按钮状态与 5 秒轮询生命周期正确。
- Token 格式化、成功率和排序结果正确。
- Dashboard 原有配置快照测试继续通过。

### 5.3 交付验证

```text
npm test
npm run build
cargo fmt --all -- --check
cargo check --tests
cargo clippy --all-targets -- -D warnings
```

## 6. 范围与非目标

本次不实现：

- prompt、响应内容或 API Key 的明细存储
- 费用估算或货币换算
- 手动清空统计历史的设置项
- 从旧日志反向重建历史统计
- 独立的请求明细审计页

后续如果需要费用估算或审计明细，可以在当前 `UsageRecord` 与版本化存储结构上扩展，而不改变 Dashboard 的基础查询接口。
