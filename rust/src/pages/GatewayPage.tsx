// 协议网关页（本地 8083 通用「Anthropic 入站 ↔ OpenAI 协议上游」转换层）。
// 处理：网关起停/状态、Key 池/会话状态轮询、模型优先级热更新、模型名映射、测试连接/端到端。
// 仅支持 OpenAI Chat Completions 协议 + API Key 认证（deepseek / glm / qwen 等 OpenAI 兼容端点）。
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  Config,
  GatewayStatus,
  GatewayTestState,
  KeyPoolStatus,
  ProviderConfig,
  ProviderModelMapEntry,
} from "../types";
import { MessageBanner } from "../components/MessageBanner";
import { KeyPoolCard } from "./nvidia/KeyPoolCard";
import { GatewayConfigForm } from "./gateway/GatewayConfigForm";
import { GatewayStatusCard } from "./gateway/GatewayStatusCard";
import { GatewayTestPanel } from "./gateway/GatewayTestPanel";

const DEFAULT_BASE_URL = "https://api.openai.com/v1";

export function GatewayPage({
  config,
  onConfig,
  test,
  onTest,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
  test: GatewayTestState;
  onTest: (updater: (prev: GatewayTestState) => GatewayTestState) => void;
}) {
  const [status, setStatus] = useState<GatewayStatus | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [keyPool, setKeyPool] = useState<KeyPoolStatus | null>(null);
  const [kpExpanded, setKpExpanded] = useState(false);

  // 测试状态从 App 提升下来（跨菜单切换保留）
  const { testBusy, testResult, chatTests } = test;
  const patchTest = (p: Partial<GatewayTestState>) =>
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
  const [providerName, setProviderName] = useState("OpenAI 兼容");
  const [keysText, setKeysText] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [modelMap, setModelMap] = useState<ProviderModelMapEntry[]>([]);
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_BASE_URL);
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState<number>(8083);
  const [cooldown, setCooldown] = useState<number>(600);
  const [retries, setRetries] = useState<number>(3);
  const [timeoutS, setTimeoutS] = useState<number>(600);
  const [authToken, setAuthToken] = useState("");

  // 当前 active provider（从 gateway config 读取）
  const activeProvider: ProviderConfig | undefined = (() => {
    const gw = config?.gateway;
    if (!gw || gw.providers.length === 0) return undefined;
    return gw.providers.find((p) => p.id === gw.active_provider) || gw.providers[0];
  })();

  // 从 config.gateway.active_provider 回填编辑态
  useEffect(() => {
    const p = activeProvider;
    if (!p) return;
    setProviderName(p.name || "未命名 Provider");
    setKeysText((p.api_keys || []).join("\n"));
    setModels(p.models || []);
    setModelMap(p.model_map || []);
    // ProviderConfig 用 base_url（上游 OpenAI 兼容端点）
    setApiBaseUrl(p.base_url || DEFAULT_BASE_URL);
    setHost(p.host || "127.0.0.1");
    setPort(p.port ?? 8083);
    setCooldown(p.cooldown_seconds ?? 600);
    setRetries(p.max_retries ?? 3);
    setTimeoutS(p.request_timeout_seconds ?? 600);
    setAuthToken(p.auth_token || "");
  }, [activeProvider]);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<GatewayStatus>("gateway_status"));
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 收拢编辑态为 ProviderConfig（写入 gateway.providers 对应条目）
  const collect = (): ProviderConfig | null => {
    if (!activeProvider) return null;
    const split = (s: string) =>
      s
        .split(/[\n,]/)
        .map((x) => x.trim())
        .filter((x) => x !== "");
    return {
      ...activeProvider,
      name: providerName.trim() || "未命名 Provider",
      protocol: "chat-completions",
      auth_mode: "api-key",
      base_url: apiBaseUrl.trim() || DEFAULT_BASE_URL,
      api_keys: split(keysText),
      models: models.map((x) => x.trim()).filter((x) => x !== ""),
      model_map: modelMap.filter((e) => e.anthropic_model && e.provider_model),
      host: host.trim() || "127.0.0.1",
      port: Math.max(1, Math.min(65535, Number(port) || 8083)),
      cooldown_seconds: Math.max(0, Number(cooldown) || 0),
      max_retries: Math.max(0, Number(retries) || 0),
      request_timeout_seconds: Math.max(1, Number(timeoutS) || 600),
      auth_token: authToken.trim(),
    };
  };

  const persistConfig = async (provider: ProviderConfig) => {
    const gw = config?.gateway;
    if (!gw) return;
    const providers = gw.providers.map((p) =>
      p.id === provider.id ? provider : p
    );
    const gateway = { providers, active_provider: gw.active_provider };
    await invoke<string>("set_gateway_config", { gateway });
    if (config) onConfig({ ...config, gateway });
  };

  const save = async () => {
    setMsg(null);
    try {
      const p = collect();
      if (!p) { setMsg("❌ 无可用 provider"); return; }
      await persistConfig(p);
      setMsg("✅ 协议网关配置已保存");
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  // 模型优先级即时热更新
  const applyModels = useCallback(
    async (list: string[]) => {
      setModels(list);
      if (config && activeProvider) {
        const providers = config.gateway.providers.map((p) =>
          p.id === activeProvider.id ? { ...p, models: list } : p
        );
        onConfig({ ...config, gateway: { ...config.gateway, providers } });
      }
      try {
        const r = await invoke<string>("gateway_set_models", { models: list });
        setMsg(r);
      } catch (e) {
        setMsg(`❌ ${e}`);
      }
    },
    [config, onConfig, activeProvider]
  );

  // 模型名映射即时热更新：写本地状态 + 落盘 + 同步给运行中代理
  //（map_model 每次请求现读 cfg，set_gateway_config 写 RwLock 即时生效，无需重启）。
  const applyModelMap = useCallback(
    async (next: ProviderModelMapEntry[]) => {
      setModelMap(next);
      if (config && activeProvider) {
        const providers = config.gateway.providers.map((p) =>
          p.id === activeProvider.id ? { ...p, model_map: next } : p
        );
        const gateway = { ...config.gateway, providers };
        onConfig({ ...config, gateway });
        try {
          await invoke<string>("set_gateway_config", { gateway });
        } catch (e) {
          setMsg(`❌ ${e}`);
        }
      }
    },
    [config, onConfig, activeProvider]
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
      const p = collect();
      if (p) await persistConfig(p);
      setMsg(await invoke<string>("gateway_start"));
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
      setMsg(await invoke<string>("gateway_stop"));
      await refresh();
    } catch (e) {
      setMsg(`❌ ${e}`);
    } finally {
      setBusy(false);
    }
  };

  // 测试连接：直连上游（API Key 用 Bearer key）
  const testConn = async () => {
    patchTest({ testBusy: true, testResult: null });
    try {
      const p = collect();
      if (p) await persistConfig(p);
      const r = await invoke<string>("gateway_test");
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
        setKeyPool(await invoke<KeyPoolStatus>("gateway_pool"));
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

  // 测试面板 curl 示例用稳定的 Anthropic 侧模型名（代理会按映射表改写为上游 slug）。
  const testModel = "claude-sonnet-4";

  // 内置端到端消息测试（走本机代理完整转换链）
  const sendChatTest = async (model: string) => {
    patchChatTest(model, { busy: true, result: null });
    try {
      const r = await invoke<string>("gateway_chat_test", { model, prompt: null });
      patchChatTest(model, { busy: false, result: r });
    } catch (e) {
      patchChatTest(model, { busy: false, result: `${e}` });
    }
  };

  // Provider 切换处理
  const handleProviderChange = (newId: string) => {
    if (!config) return;
    const gateway = { ...config.gateway, active_provider: newId };
    onConfig({ ...config, gateway });
  };

  // 添加新 Provider：构造一个默认的 Chat Completions provider，追加到 providers 并切换为 active
  const handleAddProvider = () => {
    if (!config) return;
    const id = `provider-${Date.now()}`;
    const newProvider: ProviderConfig = {
      id,
      name: "新 Provider",
      protocol: "chat-completions",
      auth_mode: "api-key",
      base_url: DEFAULT_BASE_URL,
      api_keys: [],
      models: [],
      model_map: [],
      host: "127.0.0.1",
      port: 8083,
      cooldown_seconds: 600,
      max_retries: 3,
      request_timeout_seconds: 600,
      auth_token: "",
    };
    const gateway = {
      providers: [...config.gateway.providers, newProvider],
      active_provider: id,
    };
    onConfig({ ...config, gateway });
    setMsg(`已添加新 Provider，请填写配置后保存。`);
  };

  // 删除当前 Provider（至少保留一个）
  const handleDeleteProvider = () => {
    if (!config || !activeProvider) return;
    if (providers.length <= 1) {
      setMsg("⚠️ 至少需要保留一个 Provider");
      return;
    }
    const remaining = providers.filter((p) => p.id !== activeProvider.id);
    const gateway = {
      providers: remaining,
      active_provider: remaining[0]?.id || "",
    };
    onConfig({ ...config, gateway });
    setMsg(`已删除 Provider「${activeProvider.name}」`);
  };

  const providers = config?.gateway?.providers || [];

  return (
    <div className="page">
      <h2 className="page-title">协议网关</h2>
      <p className="page-desc">
        8083 通用协议转换层：把 Claude Code 的 Anthropic 请求转换为上游 OpenAI Chat
        Completions 协议转发（deepseek / glm / qwen 等 OpenAI 兼容端点均可接入）。
        代理层按映射表改写模型名，Claude Code 端不感知。
      </p>
      <MessageBanner msg={msg} />

      <div className="card">
        <div className="form-group">
          <label>当前 Provider</label>
          <div className="input-row" style={{ gap: "0.5rem" }}>
            <select
              className="select-input"
              value={activeProvider?.id || ""}
              onChange={(e) => handleProviderChange(e.target.value)}
            >
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
            <button
              className="btn btn-secondary"
              onClick={handleAddProvider}
              title="添加一个新的 Provider（默认 Chat Completions 协议）"
            >
              + 添加
            </button>
            {providers.length > 1 && (
              <button
                className="btn btn-secondary"
                onClick={handleDeleteProvider}
                title="删除当前选中的 Provider"
              >
                删除
              </button>
            )}
          </div>
          <small className="form-hint">
            选择要编辑的 Provider。新增的 Provider 默认走 Chat Completions 协议（适合 deepseek/glm 等 OpenAI 兼容端点）。
            切换 Provider 后编辑态会自动回填对应配置。
          </small>
        </div>
      </div>

      <GatewayStatusCard
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
      <GatewayTestPanel port={port} testModel={testModel} onMessage={setMsg} />
      <GatewayConfigForm
        providerName={providerName}
        keysText={keysText}
        models={models}
        modelMap={modelMap}
        apiBaseUrl={apiBaseUrl}
        host={host}
        port={port}
        cooldown={cooldown}
        retries={retries}
        timeout={timeoutS}
        authToken={authToken}
        running={running}
        chatTests={chatTests}
        onProviderName={setProviderName}
        onKeysText={setKeysText}
        onModelMap={applyModelMap}
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
