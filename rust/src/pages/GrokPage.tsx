// Grok 代理页（镜像 NvidiaPage 结构 + OAuth 面板）。
// 处理：代理起停/状态、Key 池/会话状态轮询、模型优先级热更新、模型名映射、测试连接/端到端、
// OAuth Device Code Flow 授权（后端轮询 + emit 事件，前端只展示 + 监听）。
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  Config,
  GrokAuthMode,
  GrokConfig,
  GrokStatus,
  GrokTestState,
  GrokOAuthState,
  KeyPoolStatus,
  ModelMapEntry,
} from "../types";
import { MessageBanner } from "../components/MessageBanner";
import { KeyPoolCard } from "./nvidia/KeyPoolCard";
import { GrokConfigForm } from "./grok/GrokConfigForm";
import { GrokStatusCard } from "./grok/GrokStatusCard";
import { GrokTestPanel } from "./grok/GrokTestPanel";
import { GrokOAuthCard } from "./grok/GrokOAuthCard";

const DEFAULT_OAUTH_BASE = "https://cli-chat-proxy.grok.com/v1";
const DEFAULT_API_BASE = "https://api.x.ai/v1";

export function GrokPage({
  config,
  onConfig,
  test,
  onTest,
  oauthState,
  onOauthState,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
  test: GrokTestState;
  onTest: (updater: (prev: GrokTestState) => GrokTestState) => void;
  // OAuth 状态由 App 提升控制：用户在 Device Code Flow 进行中切到别的菜单再回来时，
  // user_code/verification_uri 仍在，不会丢（后端轮询依旧在跑）。
  oauthState: GrokOAuthState;
  onOauthState: (updater: (prev: GrokOAuthState) => GrokOAuthState) => void;
}) {
  const [status, setStatus] = useState<GrokStatus | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [keyPool, setKeyPool] = useState<KeyPoolStatus | null>(null);
  const [kpExpanded, setKpExpanded] = useState(false);
  const oauth = oauthState;
  const setOauth = onOauthState;

  // 测试状态从 App 提升下来（跨菜单切换保留）
  const { testBusy, testResult, chatTests } = test;
  const patchTest = (p: Partial<GrokTestState>) =>
    onTest((prev) => ({ ...prev, ...p }));
  const patchChatTest = (model: string, p: { busy?: boolean; result?: string | null }) =>
    onTest((prev) => {
      const cur = prev.chatTests[model] ?? { busy: false, result: null };
      return {
        ...prev,
        chatTests: { ...prev.chatTests, [model]: { ...cur, ...p } },
      };
    });

  // 配置编辑态
  const [keysText, setKeysText] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [modelMap, setModelMap] = useState<ModelMapEntry[]>([]);
  const [authMode, setAuthMode] = useState<GrokAuthMode>("oauth");
  const [oauthBaseUrl, setOauthBaseUrl] = useState(DEFAULT_OAUTH_BASE);
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_API_BASE);
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState<number>(8083);
  const [cooldown, setCooldown] = useState<number>(600);
  const [retries, setRetries] = useState<number>(3);
  const [timeoutS, setTimeoutS] = useState<number>(600);
  const [authToken, setAuthToken] = useState("");

  // 从 config 回填编辑态
  useEffect(() => {
    const g = config?.grok;
    if (!g) return;
    setKeysText((g.api_keys || []).join("\n"));
    setModels(g.models || []);
    setModelMap(g.model_map || []);
    setAuthMode(g.auth_mode || "oauth");
    setOauthBaseUrl(g.oauth_base_url || DEFAULT_OAUTH_BASE);
    setApiBaseUrl(g.api_base_url || DEFAULT_API_BASE);
    setHost(g.host || "127.0.0.1");
    setPort(g.port ?? 8083);
    setCooldown(g.cooldown_seconds ?? 600);
    setRetries(g.max_retries ?? 3);
    setTimeoutS(g.request_timeout_seconds ?? 600);
    setAuthToken(g.auth_token || "");
  }, [config]);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<GrokStatus>("grok_status"));
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // OAuth 状态拉取（grok_oauth_status）
  const refreshOauth = useCallback(async () => {
    try {
      const r = await invoke<{
        authorized: boolean;
        account: string;
        expires_at: number;
        expired: boolean;
        refreshable: boolean;
      }>("grok_oauth_status");
      setOauth((prev) => ({
        ...prev,
        authorized: r.authorized,
        account: r.account,
        expires_at: r.expires_at,
        expired: r.expired,
        refreshable: r.refreshable,
      }));
    } catch {
      // OAuth 未配置/文件不可读：保持默认未授权态
    }
  }, []);

  useEffect(() => {
    refreshOauth();
  }, [refreshOauth]);

  // 收拢编辑态为 GrokConfig
  const collect = (): GrokConfig => {
    const split = (s: string) =>
      s
        .split(/[\n,]/)
        .map((x) => x.trim())
        .filter((x) => x !== "");
    return {
      auth_mode: authMode,
      oauth_base_url: oauthBaseUrl.trim() || DEFAULT_OAUTH_BASE,
      api_base_url: apiBaseUrl.trim() || DEFAULT_API_BASE,
      api_keys: split(keysText),
      models: models.map((x) => x.trim()).filter((x) => x !== ""),
      model_map: modelMap
        .map((e) => ({
          anthropic_model: e.anthropic_model.trim(),
          grok_model: e.grok_model.trim(),
        }))
        .filter((e) => e.anthropic_model && e.grok_model),
      host: host.trim() || "127.0.0.1",
      port: Math.max(1, Math.min(65535, Number(port) || 8083)),
      cooldown_seconds: Math.max(0, Number(cooldown) || 0),
      max_retries: Math.max(0, Number(retries) || 0),
      request_timeout_seconds: Math.max(1, Number(timeoutS) || 600),
      auth_token: authToken.trim(),
      oauth_account: oauth.account,
    };
  };

  const persistConfig = async (grok: GrokConfig) => {
    await invoke<string>("set_grok_config", { grok });
    if (config) onConfig({ ...config, grok });
  };

  const save = async () => {
    setMsg(null);
    try {
      await persistConfig(collect());
      setMsg("✅ Grok 代理配置已保存");
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  // 模型优先级即时热更新
  const applyModels = useCallback(
    async (list: string[]) => {
      setModels(list);
      if (config) {
        onConfig({ ...config, grok: { ...config.grok, models: list } });
      }
      try {
        const r = await invoke<string>("grok_set_models", { models: list });
        setMsg(r);
      } catch (e) {
        setMsg(`❌ ${e}`);
      }
    },
    [config, onConfig]
  );

  // 模型名映射即时热更新：写本地状态 + 落盘 + 同步给运行中代理
  //（map_model 每次请求现读 cfg，set_grok_config 写 RwLock 即时生效，无需重启）。
  const applyModelMap = useCallback(
    async (next: ModelMapEntry[]) => {
      setModelMap(next);
      const grok = config?.grok;
      if (config && grok) {
        const merged: GrokConfig = { ...grok, model_map: next };
        onConfig({ ...config, grok: merged });
        try {
          await invoke<string>("set_grok_config", { grok: merged });
        } catch (e) {
          setMsg(`❌ ${e}`);
        }
      }
    },
    [config, onConfig]
  );

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
  const addModel = (model: string) => {
    const t = model.trim();
    if (!t) return false;
    if (models.some((m) => m.toLowerCase() === t.toLowerCase())) {
      setMsg("⚠️ 该模型已在列表中");
      return true;
    }
    applyModels([...models, t]);
    return true;
  };

  // 启动前先保存最新配置
  const start = async () => {
    setBusy(true);
    setMsg(null);
    try {
      await persistConfig(collect());
      setMsg(await invoke<string>("grok_start"));
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
      setMsg(await invoke<string>("grok_stop"));
      await refresh();
    } catch (e) {
      setMsg(`❌ ${e}`);
    } finally {
      setBusy(false);
    }
  };

  // 测试连接：直连上游（OAuth 用 access token + CLI Chat-Proxy 头，API Key 用 Bearer key）
  const testConn = async () => {
    patchTest({ testBusy: true, testResult: null });
    try {
      await persistConfig(collect());
      const r = await invoke<string>("grok_test");
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
  const connectHost = host && host !== "0.0.0.0" ? host : "127.0.0.1";
  const endpoint =
    status?.endpoint || `http://${connectHost}:${port}/v1/messages`;

  // Key 池/会话状态轮询：代理运行时每 2s 刷新
  useEffect(() => {
    let timer: number | undefined;
    const tick = async () => {
      try {
        setKeyPool(await invoke<KeyPoolStatus>("grok_pool"));
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

  // 测试面板 curl 示例用稳定的 Anthropic 侧模型名（代理会按映射表改写为 grok slug）。
  const testModel = "claude-sonnet-4";

  // 内置端到端消息测试（走本机代理完整转换链）
  const sendChatTest = async (model: string) => {
    patchChatTest(model, { busy: true, result: null });
    try {
      const r = await invoke<string>("grok_chat_test", { model, prompt: null });
      patchChatTest(model, { busy: false, result: r });
    } catch (e) {
      patchChatTest(model, { busy: false, result: `${e}` });
    }
  };

  return (
    <div className="page">
      <h2 className="page-title">Grok 代理</h2>
      <p className="page-desc">
        本地代理：把 Claude Code 的 Anthropic 请求转换为 Grok 的 OpenAI Responses 协议转发。
        主线用 OAuth（Plus 账号走 CLI Chat-Proxy 消费权益额度），退路用官方 API Key（api.x.ai）。
        代理层按映射表把 claude-* 模型名改写为 grok slug，Claude Code 端不感知。
      </p>
      <MessageBanner msg={msg} />

      <GrokOAuthCard
        state={oauth}
        setState={(updater) => setOauth((prev) => updater(prev))}
        onMessage={setMsg}
      />

      <GrokStatusCard
        icon={icon}
        statusText={statusText}
        endpoint={endpoint}
        busy={busy}
        running={running}
        testBusy={testBusy}
        testResult={testResult}
        onStart={start}
        onStop={stop}
        onRefresh={refresh}
        onTest={testConn}
      />
      <KeyPoolCard
        keyPool={keyPool}
        expanded={kpExpanded}
        cooldown={cooldown}
        onToggle={() => setKpExpanded((value) => !value)}
      />
      <GrokTestPanel port={port} testModel={testModel} onMessage={setMsg} />
      <GrokConfigForm
        keysText={keysText}
        models={models}
        modelMap={modelMap}
        authMode={authMode}
        oauthBaseUrl={oauthBaseUrl}
        apiBaseUrl={apiBaseUrl}
        host={host}
        port={port}
        cooldown={cooldown}
        retries={retries}
        timeout={timeoutS}
        authToken={authToken}
        oauthAccount={oauth.account}
        running={running}
        chatTests={chatTests}
        onKeysText={setKeysText}
        onModelMap={applyModelMap}
        onAuthMode={setAuthMode}
        onOauthBaseUrl={setOauthBaseUrl}
        onApiBaseUrl={setApiBaseUrl}
        onHost={setHost}
        onPort={setPort}
        onCooldown={setCooldown}
        onRetries={setRetries}
        onTimeout={setTimeoutS}
        onAuthToken={setAuthToken}
        onMoveModel={moveModel}
        onMoveTop={moveTop}
        onRemoveModel={removeModel}
        onAddModel={addModel}
        onTestModel={sendChatTest}
        onSave={save}
      />
    </div>
  );
}
