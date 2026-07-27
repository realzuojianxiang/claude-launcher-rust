import {
  useEffect,
  useState,
  useCallback,
  useRef,
  type Dispatch,
  type SetStateAction,
  type ReactNode,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  type Config,
  type NvidiaConfig,
  type NvidiaStatus,
  type KeyPoolStatus,
  type Profile,
  type EditProfile,
  type ProxyStatus,
  type NvTestState,
  type CfgGlobals,
  toEdit,
  fromEdit,
} from "./types";

// 菜单项定义：后台管理式左侧导航，可折叠收起
type MenuKey =
  | "dashboard"
  | "launch"
  | "proxy"
  | "nvidia"
  | "logs"
  | "config"
  | "about";

interface MenuItem {
  key: MenuKey;
  label: string;
  icon: string;
  collapsedLabel: string;
}

const MENU: MenuItem[] = [
  { key: "dashboard", label: "仪表盘", collapsedLabel: "表", icon: "📊" },
  { key: "launch", label: "启动 Claude", collapsedLabel: "启", icon: "🚀" },
  { key: "proxy", label: "CLIProxyAPI", collapsedLabel: "代", icon: "🔗" },
  { key: "nvidia", label: "NVIDIA 代理", collapsedLabel: "N", icon: "🟩" },
  { key: "logs", label: "日志", collapsedLabel: "志", icon: "📜" },
  { key: "config", label: "配置", collapsedLabel: "置", icon: "⚙️" },
  { key: "about", label: "关于", collapsedLabel: "于", icon: "ℹ️" },
];

// 应用外壳：左侧菜单 + 右侧内容，菜单可左右折叠
export default function App() {
  const [active, setActive] = useState<MenuKey>("dashboard");
  const [collapsed, setCollapsed] = useState(false);
  // 全局配置：启动后从后端加载一次，各页面读写共用
  const [config, setConfig] = useState<Config | null>(null);
  const [loadingCfg, setLoadingCfg] = useState(true);
  // NVIDIA 测试状态（提升至此以跨菜单切换保留）
  const [nvTest, setNvTest] = useState<NvTestState>({
    testBusy: false,
    testResult: null,
    chatTests: {},
  });
  // 配置页「供应商配置集」编辑态：提升到 App，避免切菜单（如去「启动 Claude」测试）
  // 后再回来时本地 useState 被卸载清空、未保存的编辑（如讯飞 ANTHROPIC_AUTH_TOKEN）丢失。
  // 与 nvTest 同一思路：编辑态不随页面卸载而消失。
  const [cfgProfiles, setCfgProfiles] = useState<EditProfile[]>([]);
  // 配置页「全局参数」编辑态：与 cfgProfiles 同理，跨菜单保活
  const [cfgGlobals, setCfgGlobals] = useState<CfgGlobals>({
    url: "",
    apiKey: "",
    yolo: false,
    compactPct: 70,
    compactWindow: 1_000_000,
  });
  const cfgProfilesInited = useRef(false);

  useEffect(() => {
    invoke<Config>("get_config")
      .then((c) => setConfig(c))
      .catch((e) => console.error("加载配置失败", e))
      .finally(() => setLoadingCfg(false));
  }, []);
  // 配置首次加载完成后，用后端数据初始化编辑态一次；之后完全由用户编辑/保存驱动，
  // 不再随 config 变化复位（否则会冲掉未保存的编辑）。
  useEffect(() => {
    if (config && !cfgProfilesInited.current) {
      setCfgProfiles(toEdit(config.profiles));
      setCfgGlobals({
        url: config.anthropic_url,
        apiKey: config.anthropic_key,
        yolo: config.yolo_mode,
        compactPct: config.compact_pct ?? 70,
        compactWindow: config.compact_window ?? 1_000_000,
      });
      cfgProfilesInited.current = true;
    }
  }, [config]);

  const activeItem = MENU.find((m) => m.key === active) as MenuItem;

  return (
    <div className={`layout ${collapsed ? "collapsed" : ""}`}>
      {/* 顶部栏：折叠按钮 + 当前页标题 */}
      <header className="topbar">
        <span className="topbar-title">
          {activeItem.icon} {activeItem.label}
        </span>
      </header>

      <div className="body">
        {/* 左侧菜单 */}
        <nav className="sidebar">
          <div className="sidebar-header">
            <span className="brand-logo">🚀</span>
            {!collapsed && <span className="brand-text">Claude Launcher</span>}
          </div>

          <ul className="menu">
            {MENU.map((m) => (
              <li
                key={m.key}
                className={`menu-item ${active === m.key ? "active" : ""}`}
                onClick={() => setActive(m.key)}
                title={collapsed ? m.label : ""}
              >
                <span className="menu-icon">{m.icon}</span>
                {!collapsed && <span className="menu-label">{m.label}</span>}
                {collapsed && <span className="menu-tip">{m.collapsedLabel}</span>}
              </li>
            ))}
          </ul>

          <div className="sidebar-footer">
            {!collapsed && <span className="version">v1.0.0 (Rust)</span>}
          </div>
        </nav>

        {/* 折叠手柄：放在左右面板分割线中间，用《/》优雅符号 */}
        <div
          className="sidebar-divider"
          onClick={() => setCollapsed((c) => !c)}
          title={collapsed ? "展开菜单" : "折叠菜单"}
          aria-label="切换菜单"
        >
          <span className="divider-handle" aria-hidden="true">
            <span className="chevron" />
          </span>
        </div>

        {/* 右侧内容区 */}
        <main className="content">
          {loadingCfg ? (
            <div className="page">
              <p className="page-desc">加载配置中…</p>
            </div>
          ) : (
            <>
              {active === "dashboard" && <DashboardPage config={config} />}
              {active === "launch" && (
                <LaunchPage config={config} onConfig={setConfig} />
              )}
              {active === "proxy" && (
                <ProxyPage config={config} onConfig={setConfig} />
              )}
              {active === "nvidia" && (
                <NvidiaPage
                  config={config}
                  onConfig={setConfig}
                  test={nvTest}
                  onTest={setNvTest}
                />
              )}
              {active === "logs" && <LogPage />}
              {active === "config" && (
                <ConfigPage
                  config={config}
                  onConfig={setConfig}
                  profiles={cfgProfiles}
                  setProfiles={setCfgProfiles}
                  globals={cfgGlobals}
                  setGlobals={setCfgGlobals}
                />
              )}
              {active === "about" && <AboutPage />}
            </>
          )}
        </main>
      </div>
    </div>
  );
}

// 顶部状态横幅：展示后端返回的成功/错误消息
function MessageBanner({ msg }: { msg: string | null }) {
  if (!msg) return null;
  const ok = msg.startsWith("✅");
  const warn = msg.startsWith("⚠");
  const color = ok ? "#34c759" : warn ? "#ff9500" : "#ff3b30";
  return (
    <div style={{ color, fontSize: 13, margin: "0 0 12px", fontWeight: 600 }}>
      {msg}
    </div>
  );
}

// 二次确认删除按钮：第一次点击进入「确认」状态（红色高亮），3 秒内再点一次才真正执行删除；
// 超时自动还原。所有删除入口统一使用，防止误删。自动阻止事件冒泡（如历史目录行的点击选中）。
function ConfirmButton({
  className,
  title,
  confirmLabel = "确认?",
  onConfirm,
  children,
}: {
  className: string;
  title: string;
  confirmLabel?: string;
  onConfirm: () => void;
  children: ReactNode;
}) {
  const [armed, setArmed] = useState(false);
  const timer = useRef<number | null>(null);
  // 组件卸载时清掉计时器，避免泄漏
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    []
  );
  const handleClick = (e: ReactMouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    if (!armed) {
      setArmed(true);
      timer.current = window.setTimeout(() => setArmed(false), 3000);
    } else {
      if (timer.current !== null) window.clearTimeout(timer.current);
      setArmed(false);
      onConfirm();
    }
  };
  return (
    <button
      className={`${className}${armed ? " confirm-armed" : ""}`}
      title={armed ? "再点一次确认删除（3 秒后自动取消）" : title}
      onClick={handleClick}
    >
      {armed ? confirmLabel : children}
    </button>
  );
}

// 仪表盘：状态总览，启动时探测一次代理状态
function DashboardPage({ config }: { config: Config | null }) {
  const [proxyRunning, setProxyRunning] = useState<boolean | null>(null);

  const refresh = useCallback(async () => {
    if (!config) return;
    try {
      const s = await invoke<ProxyStatus>("cliproxyapi_status");
      setProxyRunning(s.running);
    } catch {
      setProxyRunning(null);
    }
  }, [config]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const profileCount = config?.profiles?.length ?? 0;
  const defaultProfile = profileCount > 0 ? config!.profiles[0].name : "无";

  return (
    <div className="page">
      <h2 className="page-title">仪表盘</h2>
      <p className="page-desc">状态总览与快速入口。</p>
      <div className="grid">
        <div className="card stat-card">
          <div className="card-label">CLIProxyAPI</div>
          <div className="card-value">
            {proxyRunning === null ? "检测中…" : proxyRunning ? "运行中" : "未运行"}
          </div>
        </div>
        <div className="card stat-card">
          <div className="card-label">供应商配置</div>
          <div className="card-value" style={{ fontSize: 13 }}>
            {profileCount} 套（默认 {defaultProfile}）
          </div>
        </div>
        <div className="card stat-card">
          <div className="card-label">工作目录</div>
          <div className="card-value" style={{ fontSize: 13, wordBreak: "break-all" }}>
            {config?.work_dir || "未选择"}
          </div>
        </div>
      </div>
    </div>
  );
}

// 最近历史目录的默认展示条数；超过则折叠展开
const RECENT_VISIBLE = 3;

// 启动 Claude 页：选择工作目录 + 供应商 + 历史目录 + YOLO 开关 + 启动按钮
function LaunchPage({
  config,
  onConfig,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
}) {
  // Launch 页内置的「NVIDIA 代理」选项：不走 profiles，改用 config.nvidia 注入 8082
  const NVIDIA_PROVIDER = "🟩 NVIDIA 代理 (本地 8082)";
  const [yolo, setYolo] = useState(false);
  // 默认选中内置的 NVIDIA 代理（本地 8082）：用户大多数时间用它，并手动在菜单启动 8082
  const [profile, setProfile] = useState(NVIDIA_PROVIDER);
  const [msg, setMsg] = useState<string | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);

  // 加载历史目录（最近在前）
  const loadRecent = useCallback(async () => {
    try {
      setRecent(await invoke<string[]>("get_recent_dirs"));
    } catch {
      setRecent([]);
    }
  }, []);

  useEffect(() => {
    loadRecent();
  }, [loadRecent]);

  useEffect(() => {
    if (config) setYolo(config.yolo_mode);
  }, [config]);

  // 供应商选择：config 变化或当前选择失效时回退到第一套
  // （NVIDIA_PROVIDER 是内置项、不在 profiles 里，需保留不回退）
  useEffect(() => {
    if (config) {
      const names = config.profiles.map((p) => p.name);
      if (profile !== NVIDIA_PROVIDER && !names.includes(profile)) {
        if (names.length > 0) setProfile(names[0]);
      }
    }
  }, [config, profile]);

  const pickDir = async () => {
    try {
      const dir = await invoke<string>("select_directory");
      if (dir && config) {
        onConfig({ ...config, work_dir: dir });
        await loadRecent();
      }
    } catch (e) {
      setMsg(`❌ 选择目录失败: ${e}`);
    }
  };

  // 选用某历史目录：写回配置工作目录，同步后端状态，并记入历史（置顶）
  const useDir = async (dir: string) => {
    if (!config) return;
    onConfig({ ...config, work_dir: dir });
    setMsg(`已选用: ${dir}`);
    try {
      // 关键：同步更新后端 Config 中的 work_dir，确保启动时使用正确的目录
      await invoke<string>("set_work_dir", { dir });
      await invoke<string[]>("add_recent_dir", { dir });
      await loadRecent();
    } catch {
      /* 记录历史失败不影响选用 */
    }
  };

  // 删除某历史目录：点击删除按钮时触发，阻止冒泡以免触发 useDir
  const removeDir = async (dir: string) => {
    try {
      // 后端返回删除后的全量列表，直接用于刷新，省一次查询
      setRecent(await invoke<string[]>("remove_recent_dir", { dir }));
    } catch (e) {
      setMsg(`❌ 删除失败: ${e}`);
    }
  };

  const launch = async () => {
    setMsg(null);
    try {
      // 取注入 claude 的进程环境变量（不改动 settings.json）
      let env: Record<string, string> = {};
      if (profile === NVIDIA_PROVIDER) {
        // NVIDIA 代理：把 Claude Code 指向本地 8082，模型用配置里的（selected_model 或 models[0]）
        const nv = config?.nvidia;
        const model = nv?.models?.[0] || "";
        const port = nv?.port ?? 8082;
        env = {
          // 哨兵：触发后端用 CLAUDE_CONFIG_DIR 隔离目录启动 claude，
          // 绕开全局 ~/.claude/settings.json 的 env 段对注入变量的覆盖
          __nvidia_isolate__: "1",
          ANTHROPIC_BASE_URL: `http://127.0.0.1:${port}`,
          ANTHROPIC_API_KEY:
            (nv?.auth_token && nv.auth_token.trim()) || "sk-nvidia-local",
          ANTHROPIC_MODEL: model,
          ANTHROPIC_SMALL_FAST_MODEL: model,
        };
      } else {
        const p = config?.profiles.find((x) => x.name === profile);
        env = p ? p.env : {};
      }
      const r = await invoke<string>("launch_claude", { yolo, env });
      setMsg(r);
      // 启动成功后刷新历史（已写入新的置顶目录）
      await loadRecent();
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  // 折叠展示：默认最近 3 个，展开后全部
  const visible = expanded ? recent : recent.slice(0, RECENT_VISIBLE);
  const hasMore = recent.length > RECENT_VISIBLE;

  return (
    <div className="page">
      <h2 className="page-title">启动 Claude Code</h2>
      <p className="page-desc">
        选择工作目录与供应商，启动 Claude Code（连接参数以环境变量注入，不改动 settings.json）。
      </p>
      <MessageBanner msg={msg} />
      <div className="card">
        <div className="form-group">
          <label>工作目录</label>
          <div className="input-row">
            <input
              type="text"
              value={config?.work_dir || ""}
              placeholder="选择 Claude Code 工作目录"
              readOnly
            />
            <button className="btn btn-secondary" onClick={pickDir}>
              选择目录
            </button>
          </div>
        </div>

        <div className="form-group">
          <label>供应商 / Provider</label>
          <div className="input-row">
            <select
              className="select-input"
              value={profile}
              onChange={(e) => setProfile(e.target.value)}
            >
              {(config?.profiles ?? []).map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
              <option value={NVIDIA_PROVIDER}>{NVIDIA_PROVIDER}</option>
            </select>
          </div>
          {profile === NVIDIA_PROVIDER ? (
            <small className="form-hint">
              将把 Claude Code 指向本地{" "}
              <code>http://127.0.0.1:{config?.nvidia?.port ?? 8082}</code>，使用配置里的
              NVIDIA 模型（{config?.nvidia?.models?.join("、") || "未配置"}）。
              请先在「🟩 NVIDIA 代理」页手动启动 8082 代理。
            </small>
          ) : (
            <small className="form-hint">
              不同 provider 的连接参数（ANTHROPIC_BASE_URL / KEY / AUTH_TOKEN / MODEL …）各自独立，
              启动时仅注入到本次进程，互不干扰、可并发多开。
            </small>
          )}
        </div>

        {recent.length > 0 && (
          <div className="form-group">
            <label>最近打开</label>
            <ul className="recent-list">
              {visible.map((d) => (
                <li
                  key={d}
                  className={`recent-item ${
                    config?.work_dir === d ? "active" : ""
                  }`}
                  title={d}
                  onClick={() => useDir(d)}
                >
                  <span className="recent-folder">📁</span>
                  <span className="recent-path">{d}</span>
                  <ConfirmButton
                    className="recent-remove"
                    title="删除该历史目录"
                    onConfirm={() => removeDir(d)}
                  >
                    ✕
                  </ConfirmButton>
                </li>
              ))}
            </ul>
            {hasMore && (
              <button
                className="btn-toggle-more"
                onClick={() => setExpanded((v) => !v)}
              >
                {expanded ? `⌃ 收起` : `⌄ 展开全部 (${recent.length})`}
              </button>
            )}
          </div>
        )}

        <div className="form-group checkbox-group">
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={yolo}
              onChange={(e) => setYolo(e.target.checked)}
            />
            <span>🚀 YOLO 模式 (跳过所有权限确认)</span>
          </label>
        </div>
        <div className="button-row">
          <button className="btn btn-primary" onClick={launch}>
            🚀 启动 Claude Code
          </button>
        </div>
        <small className="form-hint">
          启动时不改写全局 ~/.claude/settings.json：连接参数通过进程环境变量注入，
          每个启动独立、可同时开多个工作目录 / 不同 provider。
        </small>
      </div>
    </div>
  );
}

// 代理管理页：启动/停止/刷新状态，以及 CLIProxyAPI 执行目录的指定
function ProxyPage({
  config,
  onConfig,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
}) {
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // 执行目录本地态：优先回显配置值，空则提示"未指定（用 exe 所在目录）"
  const [cliDir, setCliDir] = useState("");

  useEffect(() => {
    if (config) setCliDir(config.cliproxyapi_dir || "");
  }, [config]);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<ProxyStatus>("cliproxyapi_status"));
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 选择 CLIProxyAPI 执行目录：调目录对话框并持久化
  const pickCliDir = async () => {
    setMsg(null);
    try {
      const dir = await invoke<string>("select_cli_dir");
      if (dir && config) {
        setCliDir(dir);
        onConfig({ ...config, cliproxyapi_dir: dir });
      }
    } catch (e) {
      setMsg(`❌ 选择执行目录失败: ${e}`);
    }
  };

  // 清空执行目录：回到"未指定"，持久化空串
  const clearCliDir = async () => {
    if (!config) return;
    try {
      await invoke<string>("set_cli_dir", { dir: "" });
      setCliDir("");
      onConfig({ ...config, cliproxyapi_dir: "" });
      setMsg("已清空执行目录：将回退到 cliproxyapi.exe 所在目录");
    } catch (e) {
      setMsg(`❌ 清空失败: ${e}`);
    }
  };

  const start = async () => {
    setBusy(true);
    setMsg(null);
    try {
      setMsg(await invoke<string>("start_cliproxyapi"));
      await refresh();
    } catch (e) {
      setMsg(`❌ ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    setMsg(null);
    try {
      setMsg(await invoke<string>("stop_cliproxyapi"));
      await refresh();
    } catch (e) {
      setMsg(`❌ ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const running = status?.running ?? false;
  const icon = status === null ? "❓" : running ? "✅" : "❌";
  const statusText =
    status === null
      ? "未知"
      : running
      ? status.message || "运行中"
      : "未运行";

  return (
    <div className="page">
      <h2 className="page-title">CLIProxyAPI 管理</h2>
      <p className="page-desc">本地代理的启动、停止与状态查询。</p>
      <MessageBanner msg={msg} />
      <div className="card status-card">
        <div className="status-header">
          <span className="status-icon">{icon}</span>
          <span className="status-text">{statusText}</span>
        </div>
        <div className="status-url">{config?.anthropic_url || "http://localhost:8317"}</div>
        <div className="status-buttons">
          <button className="btn btn-start" onClick={start} disabled={busy || running}>
            ▶ 启动
          </button>
          <button className="btn btn-stop" onClick={stop} disabled={busy || !running}>
            ⏹ 停止
          </button>
          <button className="btn btn-refresh" onClick={refresh} disabled={busy}>
            🔄 刷新
          </button>
        </div>
      </div>

      {/* 执行目录：不指定则回退到 cliproxyapi.exe 所在目录 */}
      <div className="card">
        <div className="form-group">
          <label>CLIProxyAPI 执行目录</label>
          <div className="input-row">
            <input
              type="text"
              value={cliDir}
              placeholder="未指定（用 cliproxyapi.exe 所在目录）"
              readOnly
            />
            <button className="btn btn-secondary" onClick={pickCliDir}>
              选择目录
            </button>
            {cliDir && (
              <button className="btn btn-secondary" onClick={clearCliDir}>
                清空
              </button>
            )}
          </div>
          <small className="form-hint">
            指定后 CLIProxyAPI 将在该目录下运行（读取其配置/相对路径）；
            留空则沿用此前行为——使用被找到的 cliproxyapi.exe 所在目录。
          </small>
        </div>
      </div>
    </div>
  );
}

// 判断环境变量名是否为敏感字段（密钥 / token）：命中则在前端脱敏显示。
// 注意：脱敏只发生在「显示层」（input type=password + 👁 切换），
// React state 中始终保存真实值，保存时原样落盘——绝不把脱敏串写进配置。
const isSecretKey = (k: string): boolean =>
  /AUTH_TOKEN$|API_KEY$|SECRET|PASSWORD|PRIVATE_KEY/i.test(k);

// 变量值输入框：敏感字段默认以 password 形态脱敏（带 👁 切换显隐），
// 真实值始终留在 value 中，保存时透传真实值。
function EnvValueInput({
  value,
  secret,
  placeholder,
  onChange,
}: {
  value: string;
  secret: boolean;
  placeholder?: string;
  onChange: (v: string) => void;
}) {
  const [reveal, setReveal] = useState(false);
  if (!secret) {
    return (
      <input
        type="text"
        className="env-val"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    );
  }
  return (
    <span className="env-val-secret">
      <input
        type={reveal ? "text" : "password"}
        className="env-val"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
      <button
        type="button"
        className="env-reveal"
        title={reveal ? "隐藏" : "显示真实值"}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setReveal((r) => !r)}
      >
        {reveal ? "🙈" : "👁"}
      </button>
    </span>
  );
}

// 配置页：编辑全局参数（YOLO / auto-compact / 执行目录）+ 供应商配置集管理
function ConfigPage({
  config,
  onConfig,
  profiles,
  setProfiles,
  globals,
  setGlobals,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
  profiles: EditProfile[];
  setProfiles: Dispatch<SetStateAction<EditProfile[]>>;
  globals: CfgGlobals;
  setGlobals: Dispatch<SetStateAction<CfgGlobals>>;
}) {
  // 全局参数编辑态已提升到 App（globals/setGlobals 由 props 注入），跨菜单保活
  // url/apiKey 页面上已无编辑入口，仅在保存时透传（迁移保留字段）
  const { url, apiKey, yolo, compactPct, compactWindow } = globals;
  const setYolo = (yolo: boolean) => setGlobals((g) => ({ ...g, yolo }));
  const setCompactPct = (compactPct: number) =>
    setGlobals((g) => ({ ...g, compactPct }));
  const setCompactWindow = (compactWindow: number) =>
    setGlobals((g) => ({ ...g, compactWindow }));
  // 供应商配置集（编辑态）已提升到 App（profiles/setProfiles 由 props 注入），跨菜单保活
  const [msg, setMsg] = useState<string | null>(null);
  // 配置文件实际保存路径：展示给用户，避免误以为「没保存」
  const [cfgPath, setCfgPath] = useState("");

  useEffect(() => {
    // 加载配置文件路径（与 load 无关，单独取一次即可）
    invoke<string>("config_path")
      .then(setCfgPath)
      .catch(() => setCfgPath(""));
  }, []);

  // 保存全局参数（url/key 仅作迁移保留，实际连接参数走供应商配置集）
  const saveGlobals = async () => {
    setMsg(null);
    try {
      const pct = Math.max(0, Math.min(100, Number(compactPct) || 0));
      const win = Math.max(0, Math.round(Number(compactWindow) || 0));
      await invoke<string>("set_config", {
        url,
        key: apiKey,
        cliproxyKey: config?.cliproxyapi_key || "",
        yoloMode: yolo,
        compactWindow: win,
        compactPct: pct,
        cliproxyapiDir: config?.cliproxyapi_dir || "",
      });
      if (config) {
        onConfig({
          ...config,
          anthropic_url: url,
          anthropic_key: apiKey,
          yolo_mode: yolo,
          compact_window: win,
          compact_pct: pct,
        });
      }
      setMsg(
        `✅ 全局配置已保存${pct === 0 ? "（已关闭 auto-compact 注入）" : `（阈值 ${pct}% @ ${win} token）`}`
      );
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  // 保存供应商配置集
  const saveProfiles = async () => {
    setMsg(null);
    try {
      const cleaned = fromEdit(profiles);
      await invoke<string>("set_profiles", { profiles: cleaned });
      if (config) onConfig({ ...config, profiles: cleaned });
      setMsg("✅ 供应商配置已保存");
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  if (!config) return null;

  // —— 未保存修改检测：编辑态与已保存的 config 比对，有差异时保存按钮变警告色 ——
  // env 以排序后的键值对生成签名，避免对象键顺序差异造成误报
  const profileSig = (ps: Profile[]) =>
    JSON.stringify(
      ps.map((p) => ({
        name: p.name,
        env: Object.entries(p.env).sort((a, b) => a[0].localeCompare(b[0])),
      }))
    );
  const profilesDirty =
    profileSig(fromEdit(profiles)) !== profileSig(config.profiles);
  const globalsDirty =
    yolo !== config.yolo_mode ||
    Number(compactPct) !== (config.compact_pct ?? 70) ||
    Number(compactWindow) !== (config.compact_window ?? 1_000_000);

  // 供应商配置集的增删改处理函数
  const setName = (i: number, name: string) =>
    setProfiles((ps) => ps.map((p, idx) => (idx === i ? { ...p, name } : p)));
  const setKey = (i: number, j: number, k: string) =>
    setProfiles((ps) =>
      ps.map((p, idx) =>
        idx === i ? { ...p, env: p.env.map((r, jj) => (jj === j ? { ...r, k } : r)) } : p
      )
    );
  const setVal = (i: number, j: number, v: string) =>
    setProfiles((ps) =>
      ps.map((p, idx) =>
        idx === i ? { ...p, env: p.env.map((r, jj) => (jj === j ? { ...r, v } : r)) } : p
      )
    );
  const addRow = (i: number) =>
    setProfiles((ps) =>
      ps.map((p, idx) => (idx === i ? { ...p, env: [...p.env, { k: "", v: "" }] } : p))
    );
  const delRow = (i: number, j: number) =>
    setProfiles((ps) =>
      ps.map((p, idx) => (idx === i ? { ...p, env: p.env.filter((_, jj) => jj !== j) } : p))
    );
  const addProfile = () =>
    setProfiles((ps) => [
      ...ps,
      { name: `供应商${ps.length + 1}`, env: [{ k: "ANTHROPIC_BASE_URL", v: "" }] },
    ]);
  const delProfile = (i: number) =>
    setProfiles((ps) => ps.filter((_, idx) => idx !== i));

  return (
    <div className="page">
      <h2 className="page-title">配置</h2>
      <p className="page-desc">
        全局参数与供应商配置集。供应商配置集在启动页下拉选择，连接参数注入到 Claude 进程。
      </p>
      <MessageBanner msg={msg} />

      {/* 供应商配置集管理 */}
      <div className="card">
        <div className="form-group">
          <label>供应商配置集 (Profiles)</label>
          <small className="form-hint">
            每套含名称与一组环境变量。启动页选择后，这些变量会作为进程环境变量注入 claude。
          </small>
          {profiles.map((p, i) => (
            <div className="profile-block" key={i}>
              <div className="input-row">
                <input
                  type="text"
                  className="profile-name"
                  value={p.name}
                  placeholder="供应商名称"
                  onChange={(e) => setName(i, e.target.value)}
                />
                <ConfirmButton
                  className="btn btn-danger-sm"
                  title="删除该供应商"
                  confirmLabel="确认删除?"
                  onConfirm={() => delProfile(i)}
                >
                  删除
                </ConfirmButton>
              </div>
              {p.env.map((row, j) => (
                <div className="env-row" key={j}>
                  <input
                    type="text"
                    className="env-key"
                    value={row.k}
                    placeholder="变量名，如 ANTHROPIC_BASE_URL"
                    onChange={(e) => setKey(i, j, e.target.value)}
                  />
                  <EnvValueInput
                    value={row.v}
                    secret={isSecretKey(row.k)}
                    placeholder={
                      isSecretKey(row.k) ? "敏感值（默认脱敏显示）" : "变量值"
                    }
                    onChange={(v) => setVal(i, j, v)}
                  />
                  <ConfirmButton
                    className="recent-remove"
                    title="删除该行"
                    onConfirm={() => delRow(i, j)}
                  >
                    ✕
                  </ConfirmButton>
                </div>
              ))}
              <button className="btn-toggle-more" onClick={() => addRow(i)}>
                + 添加变量
              </button>
            </div>
          ))}
          <button className="btn btn-secondary" onClick={addProfile}>
            + 新增供应商
          </button>
        </div>
        <div className="button-row">
          <button
            className={`btn btn-secondary ${profilesDirty ? "btn-unsaved" : ""}`}
            onClick={saveProfiles}
          >
            保存供应商配置
            {profilesDirty && <span className="unsaved-badge">未保存</span>}
          </button>
          {profilesDirty && (
            <span className="unsaved-hint">⚠ 有未保存的修改，关闭应用将丢失</span>
          )}
        </div>
      </div>

      {/* 全局参数 */}
      <div className="card">
        <div className="form-group checkbox-group">
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={yolo}
              onChange={(e) => setYolo(e.target.checked)}
            />
            <span>YOLO 模式（跳过权限确认，作为启动页未指定时的默认）</span>
          </label>
        </div>

        <div className="form-group">
          <label>Auto-compact 触发阈值 (%)</label>
          <input
            type="number"
            min={0}
            max={100}
            value={compactPct}
            placeholder="70"
            onChange={(e) => setCompactPct(Number(e.target.value))}
          />
          <small className="form-hint">
            上下文用到该比例时自动压缩；70 = 70%。填 0 则不注入该配置。
          </small>
        </div>
        <div className="form-group">
          <label>Auto-compact 窗口 (token)</label>
          <input
            type="number"
            min={0}
            step={100000}
            value={compactWindow}
            placeholder="1000000"
            onChange={(e) => setCompactWindow(Number(e.target.value))}
          />
          <small className="form-hint">
            纳入压缩计算的上下文容量，1M 窗口填 1000000。
          </small>
        </div>
        <div className="button-row">
          <button
            className={`btn btn-secondary ${globalsDirty ? "btn-unsaved" : ""}`}
            onClick={saveGlobals}
          >
            保存全局配置
            {globalsDirty && <span className="unsaved-badge">未保存</span>}
          </button>
          {globalsDirty && (
            <span className="unsaved-hint">⚠ 有未保存的修改，关闭应用将丢失</span>
          )}
        </div>
      </div>

      {/* 配置保存位置：明确告知用户文件落在 exe 同级的 claude-launcher/config.json，
          避免误以为「没保存」（旧路径 ~/.claude-launcher 已不再使用） */}
      <div className="card cfg-path-card">
        <div className="form-group">
          <label>配置保存位置</label>
          <code className="cfg-path">{cfgPath || "（加载中…）"}</code>
          <small className="form-hint">
            所有配置（含供应商 env、历史目录）均保存于此文件；旧路径
            <code>~/.claude-launcher</code> 已废弃，请直接查看上面的地址。
          </small>
        </div>
      </div>
    </div>
  );
}

// NVIDIA 代理页：状态启停 + 配置编辑（Anthropic→OpenAI 协议转换代理）
function NvidiaPage({
  config,
  onConfig,
  test,
  onTest,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
  test: NvTestState;
  onTest: (updater: (prev: NvTestState) => NvTestState) => void;
}) {
  const [status, setStatus] = useState<NvidiaStatus | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [keyPool, setKeyPool] = useState<KeyPoolStatus | null>(null);
  // Key 池默认只展开前 3 个，其余折叠
  const [kpExpanded, setKpExpanded] = useState(false);

  // 测试状态从 App 提升下来（跨菜单切换保留，避免卸载丢结果）
  const { testBusy, testResult, chatTests } = test;
  const patchTest = (p: Partial<NvTestState>) =>
    onTest((prev) => ({ ...prev, ...p }));
  // 按模型写入独立的测试状态（并发互不影响）
  const patchChatTest = (model: string, p: { busy?: boolean; result?: string | null }) =>
    onTest((prev) => {
      const cur = prev.chatTests[model] ?? { busy: false, result: null };
      return {
        ...prev,
        chatTests: { ...prev.chatTests, [model]: { ...cur, ...p } },
      };
    });
  // curl 命令的 Shell 形式（不同 Shell 引号规则不同）
  const [shellKind, setShellKind] = useState<"bash" | "powershell" | "cmd">("bash");

  // 配置编辑态（keys 用多行文本；models 用有序数组，顺序即优先级）
  const [keysText, setKeysText] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [newModel, setNewModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState<number>(8082);
  const [cooldown, setCooldown] = useState<number>(65);
  const [retries, setRetries] = useState<number>(3);
  const [timeout, setTimeoutS] = useState<number>(120);
  const [authToken, setAuthToken] = useState("");

  // 从 config 回填编辑态
  useEffect(() => {
    const n = config?.nvidia;
    if (!n) return;
    setKeysText((n.api_keys || []).join("\n"));
    setModels(n.models || []);
    setBaseUrl(n.base_url || "https://integrate.api.nvidia.com/v1");
    setHost(n.host || "127.0.0.1");
    setPort(n.port ?? 8082);
    setCooldown(n.key_cooldown_seconds ?? 65);
    setRetries(n.max_retries ?? 3);
    setTimeoutS(n.request_timeout_seconds ?? 120);
    setAuthToken(n.auth_token || "");
  }, [config]);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<NvidiaStatus>("nvidia_status"));
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 把编辑态收拢成 NvidiaConfig（keys/models 按行或逗号切分、去空白去空项）
  const collect = (): NvidiaConfig => {
    const split = (s: string) =>
      s
        .split(/[\n,]/)
        .map((x) => x.trim())
        .filter((x) => x !== "");
    return {
      api_keys: split(keysText),
      models: models.map((x) => x.trim()).filter((x) => x !== ""),
      base_url: baseUrl.trim(),
      host: host.trim() || "127.0.0.1",
      port: Math.max(1, Math.min(65535, Number(port) || 8082)),
      key_cooldown_seconds: Math.max(0, Number(cooldown) || 0),
      max_retries: Math.max(0, Number(retries) || 0),
      request_timeout_seconds: Math.max(1, Number(timeout) || 120),
      auth_token: authToken.trim(),
    };
  };

  const save = async () => {
    setMsg(null);
    try {
      const nvidia = collect();
      await invoke<string>("set_nvidia_config", { nvidia });
      if (config) onConfig({ ...config, nvidia });
      setMsg("✅ NVIDIA 代理配置已保存");
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  // 实时应用模型优先级：即时持久化 + 热更新运行中的代理（无需重启）
  const applyModels = useCallback(
    async (list: string[]) => {
      setModels(list);
      if (config) {
        onConfig({ ...config, nvidia: { ...config.nvidia, models: list } });
      }
      try {
        const r = await invoke<string>("nvidia_set_models", { models: list });
        setMsg(r);
      } catch (e) {
        setMsg(`❌ ${e}`);
      }
    },
    [config, onConfig]
  );

  // 上移 / 下移 / 置顶 / 删除，均即时生效
  const moveModel = (idx: number, dir: -1 | 1) => {
    const j = idx + dir;
    if (j < 0 || j >= models.length) return;
    const next = models.slice();
    [next[idx], next[j]] = [next[j], next[idx]];
    applyModels(next);
  };
  const moveTop = (idx: number) => {
    if (idx <= 0) return;
    const next = models.slice();
    const [m] = next.splice(idx, 1);
    next.unshift(m);
    applyModels(next);
  };
  const removeModel = (idx: number) => {
    const next = models.slice();
    next.splice(idx, 1);
    applyModels(next);
  };
  const addModel = () => {
    const t = newModel.trim();
    if (!t) return;
    if (models.some((m) => m.toLowerCase() === t.toLowerCase())) {
      setMsg("⚠️ 该模型已在列表中");
      setNewModel("");
      return;
    }
    applyModels([...models, t]);
    setNewModel("");
  };

  // 启动前先保存最新配置，避免"改了没保存就启动"的困惑
  const start = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const nvidia = collect();
      await invoke<string>("set_nvidia_config", { nvidia });
      if (config) onConfig({ ...config, nvidia });
      setMsg(await invoke<string>("nvidia_start"));
      await refresh();
    } catch (e) {
      setMsg(`❌ ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    setMsg(null);
    try {
      setMsg(await invoke<string>("nvidia_stop"));
      await refresh();
    } catch (e) {
      setMsg(`❌ ${e}`);
    } finally {
      setBusy(false);
    }
  };

  // 测试连接：用第一个 Key + 第一个模型真实打一次 NVIDIA，返回成功内容或真实错误
  const testConn = async () => {
    patchTest({ testBusy: true, testResult: null });
    try {
      // 先保存最新配置，确保探测用的是当前编辑的值
      const nvidia = collect();
      await invoke<string>("set_nvidia_config", { nvidia });
      if (config) onConfig({ ...config, nvidia });
      const r = await invoke<string>("nvidia_test");
      patchTest({ testResult: r });
    } catch (e) {
      patchTest({ testResult: `❌ ${e}` });
    } finally {
      patchTest({ testBusy: false });
    }
  };

  const running = status?.running ?? false;
  const icon = status === null ? "❓" : running ? "✅" : "❌";
  const statusText = status === null ? "未知" : running ? "运行中" : "未运行";
  // 状态卡显示的地址：host 为 0.0.0.0（绑定所有网卡）时，连得上的连接地址用 127.0.0.1
  const connectHost = host && host !== "0.0.0.0" ? host : "127.0.0.1";
  const endpoint =
    status?.endpoint || `http://${connectHost}:${port}/v1/messages`;

  // Key 池状态轮询：代理运行时每 2 秒刷新一次，展示 Key 可用/冷却情况；
  // 未运行时置空，避免显示过期数据。组件卸载时清理定时器。
  useEffect(() => {
    let timer: number | undefined;
    const tick = async () => {
      try {
        const r = await invoke<KeyPoolStatus>("nvidia_key_pool");
        setKeyPool(r);
      } catch {
        setKeyPool(null);
      }
    };
    tick();
    if (running) {
      timer = window.setInterval(tick, 2000);
    }
    return () => {
      if (timer) window.clearInterval(timer);
    };
  }, [running]);

  // curl 示例用配置里的第一个真实模型
  const modelOptions = models.map((x) => x.trim()).filter(Boolean);
  const testModel = modelOptions[0] || "z-ai/glm-5.2";

  // 内置消息测试：由后端直接请求本机代理，免去复制 curl 到终端。
  // 按模型独立并发：同时点多个模型互不影响，结果分别显示在各自行下。
  const sendChatTest = async (model: string) => {
    patchChatTest(model, { busy: true, result: null });
    try {
      const r = await invoke<string>("nvidia_chat_test", { model, prompt: null });
      patchChatTest(model, { busy: false, result: r });
    } catch (e) {
      patchChatTest(model, { busy: false, result: `${e}` });
    }
  };

  // 按 Shell 生成引号正确的 curl 命令（CMD 不认单引号；PowerShell 里 curl 是别名需用 curl.exe）
  const testUrl = `http://127.0.0.1:${port}/v1/messages`;
  const testBodyObj = {
    model: testModel,
    max_tokens: 100,
    messages: [{ role: "user", content: "Introduce yourself in one sentence." }],
  };
  const bodyJson = JSON.stringify(testBodyObj);
  const curlCmd =
    shellKind === "bash"
      ? `curl ${testUrl} -H 'content-type: application/json' -d '${bodyJson}'`
      : shellKind === "powershell"
      ? `curl.exe ${testUrl} -H "content-type: application/json" -d '${bodyJson}'`
      : `curl ${testUrl} -H "content-type: application/json" -d "${bodyJson.replace(/"/g, '\\"')}"`;
  const copyText = (t: string) => {
    navigator.clipboard
      ?.writeText(t)
      .then(
        () => setMsg("✅ 已复制到剪贴板"),
        () => setMsg("❌ 复制失败，请手动选择文本")
      );
  };

  return (
    <div className="page">
      <h2 className="page-title">NVIDIA 代理</h2>
      <p className="page-desc">
        本地代理：将 Claude Code 的 Anthropic 请求转换为 NVIDIA NIM 的 OpenAI 协议，
        支持多 Key 轮询/冷却与多模型 Fallback。
      </p>
      <MessageBanner msg={msg} />

      {/* 状态与启停 */}
      <div className="card status-card">
        <div className="status-header">
          <span className="status-icon">{icon}</span>
          <span className="status-text">{statusText}</span>
        </div>
        <div className="status-url">{endpoint}</div>
        <div className="status-buttons">
          <button className="btn btn-start" onClick={start} disabled={busy || running}>
            ▶ 启动
          </button>
          <button className="btn btn-stop" onClick={stop} disabled={busy || !running}>
            ⏹ 停止
          </button>
          <button className="btn btn-refresh" onClick={refresh} disabled={busy}>
            🔄 刷新
          </button>
          <button className="btn btn-test" onClick={testConn} disabled={testBusy}>
            {testBusy ? "测试中…" : "🔌 测试连接"}
          </button>
        </div>
        {testResult && (
          <div className="test-result">
            <code>{testResult}</code>
          </div>
        )}
        <small className="form-hint">
          在 Claude Code 中把 ANTHROPIC_BASE_URL 指向上面的地址（去掉 /v1/messages），
          即可通过本代理访问 NVIDIA NIM。
        </small>
      </div>

      {/* Key 池状态（Step 2：多 Key 轮询 + 429 冷却实时展示） */}
      <div className="card">
        <div className="status-header">
          <span className="status-icon">🔑</span>
          <span className="status-text">Key 池状态</span>
        </div>
        {!keyPool || !keyPool.running ? (
          <p className="form-hint">代理未运行，启动后即可查看 Key 可用与冷却情况。</p>
        ) : (
          <>
            <div className="kp-summary">
              <span className="kp-badge kp-ok">可用 {keyPool.available}/{keyPool.total}</span>
              {keyPool.cooling > 0 && (
                <span className="kp-badge kp-cool">冷却中 {keyPool.cooling}</span>
              )}
            </div>
            <ul className="kp-list">
              {(kpExpanded ? keyPool.keys : keyPool.keys.slice(0, 3)).map((k) => (
                <li key={k.index} className={k.cooling ? "kp-item kp-item-cool" : "kp-item"}>
                  <span className="kp-idx">#{k.index + 1}</span>
                  <span className="kp-masked">{k.masked}</span>
                  {k.cooling ? (
                    <span className="kp-state kp-cool">冷却 {k.cooldown_remaining_secs}s</span>
                  ) : (
                    <span className="kp-state kp-ok">可用</span>
                  )}
                </li>
              ))}
            </ul>
            {keyPool.keys.length > 3 && (
              <button
                className="kp-toggle"
                onClick={() => setKpExpanded((v) => !v)}
              >
                {kpExpanded
                  ? "收起"
                  : `展开其余 ${keyPool.keys.length - 3} 个`}
                <span className={kpExpanded ? "kp-toggle-arrow up" : "kp-toggle-arrow"}>▾</span>
              </button>
            )}
            <small className="form-hint">
              命中 429 的 Key 会被冷却 {cooldown}s 后自动恢复，期间流量自动导向其他可用 Key。
            </small>
          </>
        )}
      </div>

      {/* 测试面板：直接给可复制的 8082 测试地址与命令，免去手敲 */}
      <div className="card">
        <div className="status-header">
          <span className="status-icon">🧪</span>
          <span className="status-text">本地测试 8082</span>
        </div>
        <p className="form-hint">
          代理启动后，用下面的地址/命令即可直接测 {port} 端口（连接地址用 127.0.0.1，而非绑定的 0.0.0.0）。
        </p>
        <div className="form-group">
          <label>测试地址（连得上的连接地址）</label>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <code
              style={{
                flex: 1,
                background: "rgba(0,0,0,0.03)",
                padding: "7px 10px",
                borderRadius: 8,
                fontFamily: "'SF Mono', monospace",
                fontSize: 12,
                overflowX: "auto",
              }}
            >
              http://127.0.0.1:{port}/v1/messages
            </code>
            <button
              className="btn"
              style={{ padding: "4px 10px", fontSize: 12 }}
              onClick={() => copyText(`http://127.0.0.1:${port}/v1/messages`)}
            >
              复制
            </button>
          </div>
        </div>
        <div className="form-group">
          <label>curl 非流式测试（复制到对应终端运行）</label>
          <div style={{ display: "flex", gap: 6, marginBottom: 6 }}>
            {(
              [
                ["bash", "Git Bash / WSL"],
                ["powershell", "PowerShell"],
                ["cmd", "CMD"],
              ] as const
            ).map(([k, label]) => (
              <button
                key={k}
                className="btn"
                style={{
                  padding: "3px 10px",
                  fontSize: 12,
                  background: shellKind === k ? "var(--primary)" : undefined,
                  color: shellKind === k ? "#fff" : undefined,
                }}
                onClick={() => setShellKind(k)}
              >
                {label}
              </button>
            ))}
          </div>
          <textarea
            className="env-val"
            readOnly
            style={{ minHeight: 66, fontFamily: "'SF Mono', monospace", width: "100%" }}
            value={curlCmd}
          />
          <button
            className="btn"
            style={{ padding: "4px 10px", fontSize: 12 }}
            onClick={() => copyText(curlCmd)}
          >
            复制 curl
          </button>
          <small className="form-hint">
            CMD 不支持单引号；PowerShell 中 curl 是 Invoke-WebRequest 别名，需用 curl.exe——已按所选终端生成正确写法。中文内容在 Windows 终端可能按 GBK 发送导致编码错误，示例改用英文提示词。
          </small>
        </div>
        <div className="form-group">
          <label>Claude Code 客户端要设的环境变量</label>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <code
              style={{
                flex: 1,
                background: "rgba(0,0,0,0.03)",
                padding: "7px 10px",
                borderRadius: 8,
                fontFamily: "'SF Mono', monospace",
                fontSize: 12,
                overflowX: "auto",
              }}
            >
              $env:ANTHROPIC_BASE_URL="http://127.0.0.1:{port}"
            </code>
            <button
              className="btn"
              style={{ padding: "4px 10px", fontSize: 12 }}
              onClick={() => copyText(`$env:ANTHROPIC_BASE_URL="http://127.0.0.1:${port}"`)}
            >
              复制
            </button>
          </div>
          <small className="form-hint">
            在启动 Claude Code 的终端里先执行这一行（PowerShell）；cmd 用{" "}
            <code>set ANTHROPIC_BASE_URL=http://127.0.0.1:{port}</code>。
          </small>
        </div>
      </div>

      {/* 配置编辑 */}
      <div className="card">
        <div className="form-group">
          <label>NVIDIA API Keys（每行一个，或逗号分隔）</label>
          <textarea
            className="env-val"
            style={{ minHeight: 88, fontFamily: "'SF Mono', monospace", width: "100%" }}
            value={keysText}
            placeholder={"nvapi-xxx1\nnvapi-xxx2"}
            onChange={(e) => setKeysText(e.target.value)}
          />
        </div>
        <div className="form-group">
          <label>模型优先级（顺序即优先级，第 1 个为默认/最高，其余依次 Fallback）</label>
          <div className="model-prio">
            {models.length === 0 ? (
              <p className="form-hint" style={{ margin: "6px 0" }}>
                暂无模型，请在下方输入框添加。
              </p>
            ) : (
              <ul className="mp-list">
                {models.map((m, i) => {
                  const ct = chatTests[m] || { busy: false, result: null };
                  return (
                    <li key={m} className={i === 0 ? "mp-item mp-item-top" : "mp-item"}>
                      <div className="mp-row">
                        <span className="mp-rank">{i === 0 ? "★" : i + 1}</span>
                        <span className="mp-name" title={m}>
                          {m}
                        </span>
                        {i === 0 && <span className="mp-tag">最高优先级</span>}
                        <span className="mp-actions">
                          <button
                            className="mp-btn mp-btn-test"
                            title={
                              running
                                ? "向本机代理发送一条非流式测试消息（可多个模型同时测试）"
                                : "请先启动代理"
                            }
                            disabled={ct.busy || !running}
                            onClick={() => sendChatTest(m)}
                          >
                            {ct.busy ? "⏳" : "▶ 测试"}
                          </button>
                          <button
                            className="mp-btn"
                            title="置顶"
                            disabled={i === 0}
                            onClick={() => moveTop(i)}
                          >
                            ⤒
                          </button>
                          <button
                            className="mp-btn"
                            title="上移"
                            disabled={i === 0}
                            onClick={() => moveModel(i, -1)}
                          >
                            ↑
                          </button>
                          <button
                            className="mp-btn"
                            title="下移"
                            disabled={i === models.length - 1}
                            onClick={() => moveModel(i, 1)}
                          >
                            ↓
                          </button>
                          <ConfirmButton
                            className="mp-btn mp-btn-del"
                            title="删除"
                            onConfirm={() => removeModel(i)}
                          >
                            ✕
                          </ConfirmButton>
                        </span>
                      </div>
                      {(ct.busy || ct.result) && (
                        <div className="mp-test-result">
                          {ct.busy ? (
                            <code className="mp-testing">测试中…（可继续测试其他模型）</code>
                          ) : (
                            <code style={{ whiteSpace: "pre-wrap" }}>{ct.result}</code>
                          )}
                        </div>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
            <div className="mp-add">
              <input
                type="text"
                value={newModel}
                placeholder="添加模型，如 nvidia/nemotron-3-ultra-550b-a55b"
                onChange={(e) => setNewModel(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    addModel();
                  }
                }}
              />
              <button className="btn" onClick={addModel}>
                添加
              </button>
            </div>
            <small className="form-hint">
              ↑/↓/⤒ 调整顺序会 <b>即时生效</b>：自动保存并热更新运行中的代理，无需重启。
              ▶ 测试 = 应用内直发一条非流式消息（走完整转换链），可<b>多个模型同时测试</b>、互不影响
              {running ? "" : "（需先启动代理）"}。
            </small>
          </div>
        </div>
        <div className="form-group">
          <label>NVIDIA Base URL</label>
          <input
            type="text"
            value={baseUrl}
            placeholder="https://integrate.api.nvidia.com/v1"
            onChange={(e) => setBaseUrl(e.target.value)}
          />
        </div>
        <div className="input-row">
          <div className="form-group" style={{ flex: 1 }}>
            <label>监听 Host</label>
            <input
              type="text"
              value={host}
              placeholder="127.0.0.1"
              onChange={(e) => setHost(e.target.value)}
            />
            {!(
              !host.trim() ||
              host.trim() === "127.0.0.1" ||
              host.trim().toLowerCase() === "localhost" ||
              host.trim() === "::1"
            ) && (
              <small style={{ color: "#c0392b", display: "block", marginTop: 4 }}>
                外部监听需配置至少 24 位鉴权 token，否则局域网设备可盗用你的 NVIDIA Key。
              </small>
            )}
          </div>
          <div className="form-group" style={{ flex: 1 }}>
            <label>监听端口</label>
            <input
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={(e) => setPort(Number(e.target.value))}
            />
          </div>
        </div>
        <div className="input-row">
          <div className="form-group" style={{ flex: 1 }}>
            <label>Key 冷却秒数</label>
            <input
              type="number"
              min={0}
              value={cooldown}
              onChange={(e) => setCooldown(Number(e.target.value))}
            />
          </div>
          <div className="form-group" style={{ flex: 1 }}>
            <label>最大重试次数</label>
            <input
              type="number"
              min={0}
              value={retries}
              onChange={(e) => setRetries(Number(e.target.value))}
            />
          </div>
          <div className="form-group" style={{ flex: 1 }}>
            <label>请求超时(秒)</label>
            <input
              type="number"
              min={1}
              value={timeout}
              onChange={(e) => setTimeoutS(Number(e.target.value))}
            />
          </div>
        </div>
        <div className="form-group">
          <label>本地代理鉴权 Token（可选，留空则不校验）</label>
          <input
            type="text"
            value={authToken}
            placeholder="留空表示不校验 x-api-key"
            onChange={(e) => setAuthToken(e.target.value)}
          />
        </div>
        <div className="button-row">
          <button className="btn btn-secondary" onClick={save}>
            保存 NVIDIA 配置
          </button>
        </div>
        <small className="form-hint">
          修改后需重启代理才生效（启动按钮会自动先保存当前配置）。
        </small>
      </div>
    </div>
  );
}

// 日志页：实时动态显示应用日志（含 NVIDIA 代理活动）。
// - 挂载时拉取内存环形缓冲的最近日志回填；
// - 监听后端 nvidia-log 事件，实时追加并自动滚到底；
// - 列出 logs/ 历史文件，点击可查看某个分卷的完整内容。
type LogLevel = "INFO" | "WARN" | "ERROR";

const LOG_LEVEL_RANK: Record<LogLevel, number> = {
  INFO: 1,
  WARN: 2,
  ERROR: 3,
};

const getLineLogLevel = (line: string): LogLevel | null => {
  const match = line.match(/\b(INFO|WARN|ERROR)\b/);
  return match ? (match[1] as LogLevel) : null;
};

function LogPage() {
  const [lines, setLines] = useState<string[]>([]);
  const [files, setFiles] = useState<{ name: string; size: number }[]>([]);
  const [autoScroll, setAutoScroll] = useState(true);
  const [logLevel, setLogLevelState] = useState<LogLevel>("INFO");
  const [histName, setHistName] = useState<string | null>(null);
  const [histContent, setHistContent] = useState<string>("");
  const [histBusy, setHistBusy] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  // 自动滚到底
  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [lines, logLevel, autoScroll]);

  const refreshFiles = useCallback(async () => {
    try {
      const fs = await invoke<{ name: string; size: number }[]>("list_log_files");
      setFiles(fs);
    } catch {
      setFiles([]);
    }
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    // 初始回填
    invoke<string[]>("get_logs")
      .then((ls) => setLines(ls))
      .catch(() => setLines([]));
    invoke<LogLevel>("get_log_level")
      .then((level) => setLogLevelState(level))
      .catch(() => setLogLevelState("INFO"));
    refreshFiles();
    // 实时订阅
    listen<string>("nvidia-log", (e) => {
      setLines((prev) => {
        const next = prev.length >= 2000 ? prev.slice(prev.length - 1999) : prev.slice();
        next.push(e.payload);
        return next;
      });
    })
      .then((u) => {
        unlisten = u;
      })
      .catch(() => {});
    return () => {
      if (unlisten) unlisten();
    };
  }, [refreshFiles]);

  const changeLogLevel = async (level: LogLevel) => {
    try {
      const applied = await invoke<LogLevel>("set_log_level", { level });
      setLogLevelState(applied);
    } catch (e) {
      console.error("设置日志等级失败", e);
    }
  };

  const visibleLines = lines.filter((line) => {
    const level = getLineLogLevel(line);
    return level === null || LOG_LEVEL_RANK[level] >= LOG_LEVEL_RANK[logLevel];
  });

  const openHist = async (name: string) => {
    setHistBusy(true);
    setHistName(name);
    setHistContent("");
    try {
      const c = await invoke<string>("read_log_file", { name });
      setHistContent(c);
    } catch (e) {
      setHistContent(`❌ ${e}`);
    } finally {
      setHistBusy(false);
    }
  };

  const fmtSize = (n: number) => {
    if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(2)} MB`;
    if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${n} B`;
  };

  return (
    <div className="page">
      <h2 className="page-title">日志</h2>
      <p className="page-desc">
        实时显示应用与 NVIDIA 代理运行日志；日志同时持久化到{" "}
        <code>logs/</code> 目录（单文件超 1MB 自动分卷，文件名带日期时间）。
      </p>

      <div className="card">
        <div className="status-header">
          <span className="status-icon">📜</span>
          <span className="status-text">实时日志（{visibleLines.length}/{lines.length} 行）</span>
          <label className="checkbox-label" style={{ marginLeft: "auto" }}>
            <span>等级</span>
            <select
              className="select-input"
              value={logLevel}
              onChange={(e) => changeLogLevel(e.target.value as LogLevel)}
              style={{ width: 96, padding: "5px 8px" }}
            >
              <option value="INFO">INFO</option>
              <option value="WARN">WARN</option>
              <option value="ERROR">ERROR</option>
            </select>
          </label>
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={(e) => setAutoScroll(e.target.checked)}
            />
            <span>自动滚动</span>
          </label>
        </div>
        <div
          ref={scrollRef}
          className="log-viewer"
          style={{
            height: 360,
            overflowY: "auto",
            background: "#1c1c1e",
            color: "#d4d4d4",
            fontFamily: "'SF Mono', 'SFMono-Regular', Consolas, monospace",
            fontSize: 12,
            padding: 12,
            borderRadius: 10,
            whiteSpace: "pre-wrap",
            wordBreak: "break-all",
          }}
        >
          {visibleLines.length === 0 ? (
            <span style={{ color: "#888" }}>（暂无日志，启动 NVIDIA 代理或操作后将出现）</span>
          ) : (
            visibleLines.map((l, i) => (
              <div key={i} className="log-line">
                {l}
              </div>
            ))
          )}
        </div>
      </div>

      <div className="card">
        <div className="status-header">
          <span className="status-icon">🗂️</span>
          <span className="status-text">历史日志文件（{files.length}）</span>
          <button className="btn btn-refresh" onClick={refreshFiles}>
            🔄 刷新
          </button>
        </div>
        {files.length === 0 ? (
          <p className="form-hint">（logs/ 目录暂无文件）</p>
        ) : (
          <ul className="recent-list">
            {files.map((f) => (
              <li
                key={f.name}
                className={`recent-item ${histName === f.name ? "active" : ""}`}
                onClick={() => openHist(f.name)}
                title={f.name}
              >
                <span className="recent-folder">📄</span>
                <span className="recent-path">{f.name}</span>
                <span style={{ marginLeft: "auto", color: "#888", fontSize: 12 }}>
                  {fmtSize(f.size)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* 历史日志文件内容弹层 */}
      {histName && (
        <div
          className="modal-mask"
          onClick={() => setHistName(null)}
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.3)",
            backdropFilter: "blur(8px)",
            WebkitBackdropFilter: "blur(8px)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 50,
          }}
        >
          <div
            className="modal-box"
            onClick={(e) => e.stopPropagation()}
            style={{
              background: "#fff",
              borderRadius: 14,
              maxWidth: 820,
              width: "90%",
              maxHeight: "80%",
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
              boxShadow: "0 20px 60px -10px rgba(0,0,0,0.3)",
            }}
          >
            <div
              className="modal-head"
              style={{
                display: "flex",
                alignItems: "center",
                padding: "12px 16px",
                borderBottom: "0.5px solid rgba(0,0,0,0.08)",
              }}
            >
              <strong style={{ fontSize: 13 }}>{histName}</strong>
              <button
                className="btn"
                style={{ marginLeft: "auto", padding: "2px 10px", fontSize: 12 }}
                onClick={() => setHistName(null)}
              >
                关闭
              </button>
            </div>
            <div
              className="log-viewer"
              style={{
                flex: 1,
                overflowY: "auto",
                background: "#1c1c1e",
                color: "#d4d4d4",
                fontFamily: "'SF Mono', 'SFMono-Regular', Consolas, monospace",
                fontSize: 12,
                padding: 12,
                whiteSpace: "pre-wrap",
                wordBreak: "break-all",
              }}
            >
              {histBusy ? "读取中…" : histContent || "（空文件）"}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function AboutPage() {
  return (
    <div className="page">
      <h2 className="page-title">关于</h2>
      <div className="card">
        <p>
          <strong>Claude Launcher</strong> (Rust 实现)
        </p>
        <p className="muted">快速启动 Claude Code + CLIProxyAPI 的桌面启动器。</p>
        <ul className="kv-list">
          <li>
            <span>实现语言</span>
            <span>Rust (Tauri v2 + React)</span>
          </li>
          <li>
            <span>版本</span>
            <span>1.0.0</span>
          </li>
          <li>
            <span>平台</span>
            <span>Windows</span>
          </li>
          <li>
            <span>连接参数注入</span>
            <span>进程环境变量（不改动 settings.json）</span>
          </li>
        </ul>
      </div>
    </div>
  );
}
