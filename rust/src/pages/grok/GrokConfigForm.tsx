// Grok 代理配置表单（适配自 NvidiaConfigForm + Grok 特有字段）。
// 特有项：auth_mode 切换、两端 base url、模型名映射表编辑、OAuth 账号只读展示。
import type { GrokAuthMode, ModelMapEntry, NvTestState } from "../../types";
import { ModelPriorityEditor } from "../nvidia/ModelPriorityEditor";
import { ModelMapEditor } from "./ModelMapEditor";

function generateAuthToken(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

// 模型名映射表编辑器见 ./ModelMapEditor。

export function GrokConfigForm({
  keysText,
  models,
  modelMap,
  authMode,
  oauthBaseUrl,
  apiBaseUrl,
  host,
  port,
  cooldown,
  retries,
  timeout,
  authToken,
  oauthAccount,
  running,
  chatTests,
  onKeysText,
  onModelMap,
  onAuthMode,
  onOauthBaseUrl,
  onApiBaseUrl,
  onHost,
  onPort,
  onCooldown,
  onRetries,
  onTimeout,
  onAuthToken,
  onMoveModel,
  onMoveTop,
  onRemoveModel,
  onAddModel,
  onTestModel,
  onSave,
}: {
  keysText: string;
  models: string[];
  modelMap: ModelMapEntry[];
  authMode: GrokAuthMode;
  oauthBaseUrl: string;
  apiBaseUrl: string;
  host: string;
  port: number;
  cooldown: number;
  retries: number;
  timeout: number;
  authToken: string;
  oauthAccount: string;
  running: boolean;
  chatTests: NvTestState["chatTests"];
  onKeysText: (value: string) => void;
  onModelMap: (next: ModelMapEntry[]) => void;
  onAuthMode: (value: GrokAuthMode) => void;
  onOauthBaseUrl: (value: string) => void;
  onApiBaseUrl: (value: string) => void;
  onHost: (value: string) => void;
  onPort: (value: number) => void;
  onCooldown: (value: number) => void;
  onRetries: (value: number) => void;
  onTimeout: (value: number) => void;
  onAuthToken: (value: string) => void;
  onMoveModel: (index: number, direction: -1 | 1) => void;
  onMoveTop: (index: number) => void;
  onRemoveModel: (index: number) => void;
  onAddModel: (model: string) => boolean;
  onTestModel: (model: string) => void;
  onSave: () => void;
}) {
  const isLocalHost =
    !host.trim() ||
    host.trim() === "127.0.0.1" ||
    host.trim().toLowerCase() === "localhost" ||
    host.trim() === "::1";
  // 当前生效的 base（随 auth_mode 切换），仅用于在表单顶部给一句「当前指向上游」提示。
  const effectiveBase =
    authMode === "oauth"
      ? oauthBaseUrl || "https://cli-chat-proxy.grok.com/v1"
      : apiBaseUrl || "https://api.x.ai/v1";

  return (
    <div className="card">
      <div className="form-group">
        <label>认证模式</label>
        <div className="input-row">
          <select
            className="select-input"
            value={authMode}
            onChange={(e) => onAuthMode(e.target.value as GrokAuthMode)}
          >
            <option value="oauth">OAuth（Plus 账号 → CLI Chat-Proxy，主线）</option>
            <option value="api-key">API Key（官方 api.x.ai，退路）</option>
          </select>
        </div>
        <small className="form-hint">
          OAuth 模式走 cli-chat-proxy.grok.com，消费账号权益额度，需先在上方「授权 Grok 账号」
          完成 Device Code Flow；API Key 模式走 api.x.ai，用官方 xai-* Key，账号被风控/额度
          耗尽时切换即可（OAuth 已授权的 token 不丢）。
        </small>
      </div>

      {oauthAccount && authMode === "oauth" && (
        <div className="form-group">
          <small className="form-hint">
            当前 OAuth 账号：<code>{oauthAccount}</code>
          </small>
        </div>
      )}

      {authMode === "api-key" && (
        <div className="form-group">
          <label>xAI API Keys（每行一个，或逗号分隔；退路模式用）</label>
          <textarea
            className="env-val"
            style={{ minHeight: 88, fontFamily: "'SF Mono', monospace", width: "100%" }}
            value={keysText}
            placeholder={"xai-xxx1\nxai-xxx2"}
            onChange={(event) => onKeysText(event.target.value)}
          />
          <small className="form-hint">
            仅 API Key 模式生效；多 Key 自动轮询 + 429 冷却（复用 NVIDIA Key 池语义）。
          </small>
        </div>
      )}

      <ModelPriorityEditor
        models={models}
        running={running}
        chatTests={chatTests}
        onMove={onMoveModel}
        onMoveTop={onMoveTop}
        onRemove={onRemoveModel}
        onAdd={onAddModel}
        onTest={onTestModel}
        addPlaceholder="添加 Grok 模型，如 grok-4.3"
      />

      <ModelMapEditor modelMap={modelMap} onChange={onModelMap} />

      <div className="form-group">
        <label>OAuth 模式上游 Base URL（CLI Chat-Proxy）</label>
        <input
          type="text"
          value={oauthBaseUrl}
          placeholder="https://cli-chat-proxy.grok.com/v1"
          onChange={(event) => onOauthBaseUrl(event.target.value)}
        />
      </div>
      <div className="form-group">
        <label>API Key 退路模式上游 Base URL（api.x.ai）</label>
        <input
          type="text"
          value={apiBaseUrl}
          placeholder="https://api.x.ai/v1"
          onChange={(event) => onApiBaseUrl(event.target.value)}
        />
      </div>
      <small className="form-hint">
        当前生效上游：<code>{effectiveBase}/responses</code>（按认证模式裁决）。
        两端都禁跟随重定向，3xx 一律判 502，防 SSRF 与 token 外泄。
      </small>

      <div className="input-row">
        <div className="form-group" style={{ flex: 1 }}>
          <label>监听 Host</label>
          <input
            type="text"
            value={host}
            placeholder="127.0.0.1"
            onChange={(event) => onHost(event.target.value)}
          />
          {!isLocalHost && (
            <small style={{ color: "#c0392b", display: "block", marginTop: 4 }}>
              外部监听需配置至少 24 位鉴权 token，否则局域网设备可盗用你的 Grok 权益。
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
            onChange={(event) => onPort(Number(event.target.value))}
          />
        </div>
      </div>

      <div className="input-row">
        <div className="form-group" style={{ flex: 1 }}>
          <label>429 冷却秒数</label>
          <input
            type="number"
            min={0}
            value={cooldown}
            onChange={(event) => onCooldown(Number(event.target.value))}
          />
        </div>
        <div className="form-group" style={{ flex: 1 }}>
          <label>最大重试次数</label>
          <input
            type="number"
            min={0}
            value={retries}
            onChange={(event) => onRetries(Number(event.target.value))}
          />
        </div>
        <div className="form-group" style={{ flex: 1 }}>
          <label>响应/流等待超时(秒)</label>
          <input
            type="number"
            min={1}
            value={timeout}
            onChange={(event) => onTimeout(Number(event.target.value))}
          />
        </div>
      </div>

      <div className="form-group">
        <label>本地代理鉴权 Token（可选，留空则不校验）</label>
        <div className="input-row">
          <input
            type="text"
            value={authToken}
            placeholder="留空表示不校验 x-api-key"
            onChange={(event) => onAuthToken(event.target.value)}
          />
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() => onAuthToken(generateAuthToken())}
          >
            生成安全 Token
          </button>
        </div>
      </div>

      <div className="button-row">
        <button className="btn btn-secondary" onClick={onSave}>
          保存 Grok 配置
        </button>
      </div>
      <small className="form-hint">
        修改后需重启代理才生效（启动按钮会自动先保存当前配置）。模型优先级与映射表保存后即时热更新运行中代理。
      </small>
    </div>
  );
}
