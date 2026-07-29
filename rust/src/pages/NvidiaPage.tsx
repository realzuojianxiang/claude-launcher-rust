import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  Config,
  NvidiaConfig,
  NvidiaStatus,
  KeyPoolStatus,
  NvTestState,
} from "../types";
import { MessageBanner } from "../components/MessageBanner";
import { KeyPoolCard } from "./nvidia/KeyPoolCard";
import { NvidiaConfigForm } from "./nvidia/NvidiaConfigForm";
import { NvidiaStatusCard } from "./nvidia/NvidiaStatusCard";
import { NvidiaTestPanel } from "./nvidia/NvidiaTestPanel";

export function NvidiaPage({
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
  // 配置编辑态（keys 用多行文本；models 用有序数组，顺序即优先级）
  const [keysText, setKeysText] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [baseUrl, setBaseUrl] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState<number>(8082);
  const [cooldown, setCooldown] = useState<number>(65);
  const [retries, setRetries] = useState<number>(3);
  const [timeout, setTimeoutS] = useState<number>(600);
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
    setTimeoutS(n.request_timeout_seconds ?? 600);
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
      request_timeout_seconds: Math.max(1, Number(timeout) || 600),
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

  return (
    <div className="page">
      <h2 className="page-title">NVIDIA 代理</h2>
      <p className="page-desc">
        本地代理：将 Claude Code 的 Anthropic 请求转换为 NVIDIA NIM 的 OpenAI 协议，
        支持多 Key 轮询/冷却与多模型 Fallback。
      </p>
      <MessageBanner msg={msg} />

      <NvidiaStatusCard
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
      <NvidiaTestPanel port={port} testModel={testModel} onMessage={setMsg} />
      <NvidiaConfigForm
        keysText={keysText}
        models={models}
        baseUrl={baseUrl}
        host={host}
        port={port}
        cooldown={cooldown}
        retries={retries}
        timeout={timeout}
        authToken={authToken}
        running={running}
        chatTests={chatTests}
        onKeysText={setKeysText}
        onBaseUrl={setBaseUrl}
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
