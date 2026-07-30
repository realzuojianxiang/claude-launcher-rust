import {
  useEffect,
  useState,
  useRef,
  lazy,
  Suspense,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  type Config,
  type EditProfile,
  type NvTestState,
  type CfgGlobals,
  toEdit,
} from "./types";
import { MENU, type MenuKey, type MenuItem } from "./menu";
import Sidebar from "./components/Sidebar";
import { Sparkles } from "lucide-react";

// 业务页面全部按需加载（React.lazy + 动态 import），Vite 会为每页拆独立
// chunk：首屏主包不再包含所有页面代码，尤其是单词本页及其词典数据。
const DashboardPage = lazy(() =>
  import("./pages/DashboardPage").then((m) => ({ default: m.DashboardPage }))
);
const LaunchPage = lazy(() =>
  import("./pages/LaunchPage").then((m) => ({ default: m.LaunchPage }))
);
const ProxyPage = lazy(() =>
  import("./pages/ProxyPage").then((m) => ({ default: m.ProxyPage }))
);
const ConfigPage = lazy(() =>
  import("./pages/ConfigPage").then((m) => ({ default: m.ConfigPage }))
);
const NvidiaPage = lazy(() =>
  import("./pages/NvidiaPage").then((m) => ({ default: m.NvidiaPage }))
);
const LogPage = lazy(() =>
  import("./pages/LogPage").then((m) => ({ default: m.LogPage }))
);
const AboutPage = lazy(() =>
  import("./pages/AboutPage").then((m) => ({ default: m.AboutPage }))
);
const DictionaryPage = lazy(() =>
  import("./pages/DictionaryPage").then((m) => ({ default: m.DictionaryPage }))
);

// 懒加载页面的轻量占位：样式与页面容器一致，文字延迟淡入（CSS 控制），
// 本地 chunk 通常毫秒级加载完成，肉眼几乎看不到占位，避免闪烁。
function PageFallback() {
  return (
    <div className="page">
      <div className="lazy-loading" role="status">
        页面加载中…
      </div>
    </div>
  );
}

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
  const ActiveIcon = activeItem.icon;

  return (
    <div className="layout">
      {/* 顶部栏：当前页标题 */}
      <header className="topbar">
        <span className="topbar-title flex items-center gap-2">
          <ActiveIcon size={18} strokeWidth={2.2} className="text-slate-700" />
          {activeItem.label}
        </span>
      </header>

      <div className="body">
        {/* 左侧菜单（受控折叠 / 选中） */}
        <Sidebar
          items={MENU}
          activeKey={active}
          onSelect={(k) => setActive(k as MenuKey)}
          collapsed={collapsed}
          onToggleCollapse={() => setCollapsed((c) => !c)}
          brand="Claude Launcher"
          brandIcon={Sparkles}
          version="v1.0.0 (Rust)"
        />

        {/* 右侧内容区 */}
        <main className="content">
          {loadingCfg ? (
            <div className="page">
              <p className="page-desc">加载配置中…</p>
            </div>
          ) : (
            <Suspense fallback={<PageFallback />}>
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
              {active === "dictionary" && <DictionaryPage />}
            </Suspense>
          )}
        </main>
      </div>
    </div>
  );
}
