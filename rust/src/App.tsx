import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

// 菜单项定义：后台管理式左侧导航，可折叠收起
type MenuKey = "dashboard" | "launch" | "proxy" | "config" | "about";

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
  { key: "config", label: "配置", collapsedLabel: "置", icon: "⚙️" },
  { key: "about", label: "关于", collapsedLabel: "于", icon: "ℹ️" },
];

// 后端配置结构，字段对齐 Rust Config
interface Config {
  work_dir: string;
  anthropic_url: string;
  anthropic_key: string;
  cliproxyapi_key: string;
  yolo_mode: boolean;
}

// CLIProxyAPI 状态返回结构
interface ProxyStatus {
  running: boolean;
  url: string;
  status_code?: number;
  message?: string;
  error?: string;
}

// 应用外壳：左侧菜单 + 右侧内容，菜单可左右折叠
export default function App() {
  const [active, setActive] = useState<MenuKey>("dashboard");
  const [collapsed, setCollapsed] = useState(false);
  // 全局配置：启动后从后端加载一次，各页面读写共用
  const [config, setConfig] = useState<Config | null>(null);
  const [loadingCfg, setLoadingCfg] = useState(true);

  useEffect(() => {
    invoke<Config>("get_config")
      .then((c) => setConfig(c))
      .catch((e) => console.error("加载配置失败", e))
      .finally(() => setLoadingCfg(false));
  }, []);

  const activeItem = MENU.find((m) => m.key === active) as MenuItem;

  return (
    <div className={`layout ${collapsed ? "collapsed" : ""}`}>
      {/* 顶部栏：折叠按钮 + 当前页标题 */}
      <header className="topbar">
        <button
          className="toggle-btn"
          onClick={() => setCollapsed((c) => !c)}
          title={collapsed ? "展开菜单" : "折叠菜单"}
          aria-label="切换菜单"
        >
          {collapsed ? "☰" : "✕"}
        </button>
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
              {active === "proxy" && <ProxyPage config={config} />}
              {active === "config" && (
                <ConfigPage config={config} onConfig={setConfig} />
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
  const color = ok ? "#2d9d5f" : warn ? "#d98a2b" : "#d64545";
  return (
    <div style={{ color, fontSize: 13, margin: "0 0 12px", fontWeight: 600 }}>
      {msg}
    </div>
  );
}

// 仪表盘：状态总览，启动时探测一次代理状态与备份标记
function DashboardPage({ config }: { config: Config | null }) {
  const [proxyRunning, setProxyRunning] = useState<boolean | null>(null);
  const [backupExists, setBackupExists] = useState(false);

  const refresh = useCallback(async () => {
    if (!config) return;
    try {
      const s = await invoke<ProxyStatus>("cliproxyapi_status");
      setProxyRunning(s.running);
    } catch {
      setProxyRunning(null);
    }
    try {
      const info = await invoke<{ backup_exists: boolean }>("get_settings_info");
      setBackupExists(info.backup_exists);
    } catch {
      /* 忽略：仅展示用 */
    }
  }, [config]);

  useEffect(() => {
    refresh();
  }, [refresh]);

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
          <div className="card-label">settings.json</div>
          <div className="card-value">{backupExists ? "已改写(有备份)" : "未备份"}</div>
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

// 启动 Claude 页：选择工作目录 + 历史目录 + YOLO 开关 + 启动按钮 + 立即还原
function LaunchPage({
  config,
  onConfig,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
}) {
  const [yolo, setYolo] = useState(false);
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

  // 选用某历史目录：写回配置工作目录，并记入历史（置顶）
  const useDir = async (dir: string) => {
    if (!config) return;
    onConfig({ ...config, work_dir: dir });
    setMsg(`已选用: ${dir}`);
    try {
      await invoke<string[]>("add_recent_dir", { dir });
      await loadRecent();
    } catch {
      /* 记录历史失败不影响选用 */
    }
  };

  const launch = async () => {
    setMsg(null);
    try {
      const r = await invoke<string>("launch_claude");
      setMsg(r);
      // 启动成功后刷新历史（已写入新的置顶目录）
      await loadRecent();
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  const restore = async () => {
    try {
      setMsg(await invoke<string>("restore_now"));
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
      <p className="page-desc">选择工作目录并启动 Claude Code（CLIProxyAPI 接入）。</p>
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
                </li>
              ))}
            </ul>
            {hasMore && (
              <button
                className="btn-toggle-more"
                onClick={() => setExpanded((v) => !v)}
              >
                {expanded
                  ? `⌃ 收起`
                  : `⌄ 展开全部 (${recent.length})`}
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
          <button className="btn btn-secondary" onClick={restore}>
            ↩️ 还原 settings.json
          </button>
        </div>
      </div>
    </div>
  );
}

// 代理管理页：启动/停止/刷新状态
function ProxyPage({ config }: { config: Config | null }) {
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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
    </div>
  );
}

// 配置页：编辑代理地址/密钥并保存
function ConfigPage({
  config,
  onConfig,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
}) {
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");
  const [proxykey, setProxykey] = useState("");
  const [yolo, setYolo] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    if (config) {
      setUrl(config.anthropic_url);
      setKey(config.anthropic_key);
      setProxykey(config.cliproxyapi_key);
      setYolo(config.yolo_mode);
    }
  }, [config]);

  const save = async () => {
    setMsg(null);
    try {
      await invoke<string>("set_config", {
        url,
        key,
        cliproxyKey: proxykey,
        yoloMode: yolo,
      });
      if (config) onConfig({ ...config, anthropic_url: url, anthropic_key: key, cliproxyapi_key: proxykey, yolo_mode: yolo });
      setMsg("✅ 配置已保存");
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  if (!config) return null;

  return (
    <div className="page">
      <h2 className="page-title">配置</h2>
      <p className="page-desc">CLIProxyAPI 接入参数，保存到 ~/.claude-launcher/config.json。</p>
      <MessageBanner msg={msg} />
      <div className="card">
        <div className="form-group">
          <label>CLIProxyAPI 地址</label>
          <input
            type="text"
            value={url}
            placeholder="http://localhost:8317"
            onChange={(e) => setUrl(e.target.value)}
          />
        </div>
        <div className="form-group">
          <label>CLIProxyAPI 密钥</label>
          <input
            type="text"
            value={key}
            placeholder="sk-cliproxy-demo-key-1"
            onChange={(e) => setKey(e.target.value)}
          />
        </div>
        <div className="form-group checkbox-group">
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={yolo}
              onChange={(e) => setYolo(e.target.checked)}
            />
            <span>YOLO 模式（跳过权限确认）</span>
          </label>
        </div>
        <div className="button-row">
          <button className="btn btn-secondary" onClick={save}>
            保存配置
          </button>
        </div>
      </div>
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
        </ul>
      </div>
    </div>
  );
}
