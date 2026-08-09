// 前端共享类型与纯转换函数
// 从 App.tsx 抽出：后端配置结构（对齐 Rust Config/NvidiaConfig）、各页面 props 用到的
// 状态结构、配置页编辑用的可变结构，以及 Profile[] <-> EditProfile[] 转换。
// 全部 export，由 App.tsx 及各页面/组件按需引用。

// 后端配置结构，字段对齐 Rust Config
export interface Config {
  work_dir: string;
  yolo_mode: boolean;
  // auto-compact 触发阈值：compact_pct=窗口占比(0-100)，0 表示关闭注入；
  // compact_window=纳入计算的上下文容量(token)，默认 1_000_000 对应 1M 窗口
  compact_window: number;
  compact_pct: number;
  // 供应商配置集：每组含 name 与 env（注入到 claude 进程的环境变量）
  profiles: Profile[];
  // NVIDIA API 代理配置
  nvidia: NvidiaConfig;
  // Grok 代理配置（OAuth + CLI Chat-Proxy 主线 / API Key 退路）
  grok: GrokConfig;
}

export type UsageRange = "live" | "7d" | "30d" | "all";

export interface UsageAggregate {
  requests: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  failed_requests: number;
  retry_count: number;
  success_rate: number;
  usage_missing_requests: number;
}

export interface UsageTrendPoint {
  label: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  failed_requests: number;
  retry_count: number;
}

export interface UsageModelAggregate extends UsageAggregate {
  provider: string;
  model: string;
}

export interface UsageProviderAggregate {
  provider: string;
  requests: number;
  total_tokens: number;
  failed_requests: number;
  retry_count: number;
}

export interface UsageStatsSnapshot {
  range: UsageRange;
  generated_at: string;
  totals: UsageAggregate;
  trend: UsageTrendPoint[];
  models: UsageModelAggregate[];
  providers: UsageProviderAggregate[];
  history_recovered: boolean;
  history_writable: boolean;
}

// NVIDIA 代理配置，字段对齐 Rust NvidiaConfig
export interface NvidiaConfig {
  api_keys: string[];
  models: string[];
  base_url: string;
  host: string;
  port: number;
  key_cooldown_seconds: number;
  max_retries: number;
  request_timeout_seconds: number;
  auth_token: string;
}

// NVIDIA / Grok 代理运行状态（结构相同：running/url/endpoint）
export interface NvidiaStatus {
  running: boolean;
  url: string;
  endpoint: string;
}
export type GrokStatus = NvidiaStatus;

// —— Grok 代理配置（对齐 Rust src-tauri/src/grok/models.rs）——

// 认证模式：oauth=CLI Chat-Proxy 主线 / api-key=官方 api.x.ai 退路
export type GrokAuthMode = "oauth" | "api-key";

// 模型名映射条目：Anthropic 侧模型名 -> Grok 上游 slug
export interface ModelMapEntry {
  anthropic_model: string;
  grok_model: string;
}

// Grok 代理配置，字段对齐 Rust GrokConfig
export interface GrokConfig {
  auth_mode: GrokAuthMode;
  oauth_base_url: string;
  api_base_url: string;
  api_keys: string[];
  models: string[];
  model_map: ModelMapEntry[];
  host: string;
  port: number;
  cooldown_seconds: number;
  max_retries: number;
  request_timeout_seconds: number;
  auth_token: string;
  oauth_account: string;
}

// OAuth Device Code Flow 前端交互态：start 后由后端轮询 + emit 事件，前端只展示
export interface GrokOAuthState {
  // 后端 grok_oauth_status 返回
  authorized: boolean;
  account: string;
  expires_at: number;
  expired: boolean;
  refreshable: boolean;
  // grok_oauth_start 返回（fresh flow）
  userCode: string | null;
  verificationUri: string | null;
  verificationUriComplete: string | null;
  expires_in: number | null;
  // 本地交互态
  busy: boolean;
  // grok-oauth-error 事件回写
  error: string | null;
}

// Grok 测试面板状态（提升到 App，跨菜单切换保留）。结构与 NvTestState 同形
export interface GrokTestState {
  testBusy: boolean;
  testResult: string | null;
  chatTests: Record<string, { busy: boolean; result: string | null }>;
}

// Key 池单个 Key 的状态
export interface KeyInfo {
  index: number;
  masked: string;
  cooling: boolean;
  cooldown_remaining_secs: number;
}

// Key 池整体状态（Step 2 状态面板）
export interface KeyPoolStatus {
  running: boolean;
  total: number;
  available: number;
  cooling: number;
  keys: KeyInfo[];
}

// 供应商配置集
export interface Profile {
  name: string;
  env: Record<string, string>;
}

// 配置页编辑用的可变结构：env 以键值对数组表示，便于增删行
export type EnvRow = { k: string; v: string };
export type EditProfile = { name: string; env: EnvRow[] };

// Profile[] <-> EditProfile[] 转换
export function toEdit(ps: Profile[]): EditProfile[] {
  return ps.map((p) => ({
    name: p.name,
    env: Object.entries(p.env).map(([k, v]) => ({ k, v })),
  }));
}
export function fromEdit(ps: EditProfile[]): Profile[] {
  return ps.map((p) => ({
    name: p.name,
    env: Object.fromEntries(
      p.env.filter((r) => r.k.trim() !== "").map((r) => [r.k, r.v])
    ),
  }));
}

// 异步加载/查询的状态：DashboardPage 等处探测后端状态时共用此
// 三态联合，避免各页面各自重新声明同一形态的 "loading" | "ready" | "error"
// 字符串联合。带 payload 的加载结果（如 App 的配置加载）不在此列，按需要
// 用独立的判别联合承载，二者职责不同。
export type AsyncStatus = "loading" | "ready" | "error";

// NVIDIA 测试面板状态：提升到 App 持有，避免切换菜单卸载 NvidiaPage 时丢失
// （异步测试完成后写回的是 App 的 state，切回该页可继续看到进行中/最终结果）
export interface NvTestState {
  testBusy: boolean;
  testResult: string | null;
  // 按模型独立的消息测试状态：并发互不影响，key 为模型名
  chatTests: Record<string, { busy: boolean; result: string | null }>;
}

// 配置页「全局参数」编辑态：同样提升到 App，避免切菜单卸载 ConfigPage 时
// 未保存的修改（YOLO / auto-compact）被清空。
export interface CfgGlobals {
  yolo: boolean;
  compactPct: number;
  compactWindow: number;
}
