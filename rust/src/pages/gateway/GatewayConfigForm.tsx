// 协议网关配置表单（适配自 NvidiaConfigForm）。
// 协议网关仅支持 OpenAI Chat Completions（/v1/chat/completions）+ API Key 认证，
// 故仅保留通用字段：API Keys、模型优先级、模型名映射、上游 Base URL、监听参数、本地鉴权 Token。
import type { NvTestState, ProviderModelMapEntry } from "../../types";
import { ModelPriorityEditor } from "../nvidia/ModelPriorityEditor";
import { GatewayModelMapEditor } from "./GatewayModelMapEditor";

function generateAuthToken(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function GatewayConfigForm({
  providerName,
  keysText,
  models,
  modelMap,
  apiBaseUrl,
  host,
  port,
  cooldown,
  retries,
  timeout,
  authToken,
  running,
  chatTests,
  onProviderName,
  onKeysText,
  onModelMap,
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
  providerName: string;
  keysText: string;
  models: string[];
  modelMap: ProviderModelMapEntry[];
  apiBaseUrl: string;
  host: string;
  port: number;
  cooldown: number;
  retries: number;
  timeout: number;
  authToken: string;
  running: boolean;
  chatTests: NvTestState["chatTests"];
  onProviderName: (value: string) => void;
  onKeysText: (value: string) => void;
  onModelMap: (next: ProviderModelMapEntry[]) => void;
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
  const upstreamPath = "/chat/completions";

  return (
    <div className="card">
      {/* Provider 基本信息：名字 */}
      <div className="input-row">
        <div className="form-group" style={{ flex: 2 }}>
          <label>Provider 名称</label>
          <input
            type="text"
            value={providerName}
            placeholder="如 DeepSeek / GLM / Qwen"
            onChange={(event) => onProviderName(event.target.value)}
          />
        </div>
      </div>
      <small className="form-hint">
        协议网关当前仅支持 OpenAI Chat Completions 协议（/v1/chat/completions），
        适合绝大多数 OpenAI 兼容端点（deepseek / glm / qwen 等）。
      </small>

      {/* API Keys */}
      <div className="form-group">
        <label>API Keys（每行一个，或逗号分隔）</label>
        <textarea
          className="env-val"
          style={{ minHeight: 88, fontFamily: "'SF Mono', monospace", width: "100%" }}
          value={keysText}
          placeholder={"sk-xxx1\nsk-xxx2"}
          onChange={(event) => onKeysText(event.target.value)}
        />
        <small className="form-hint">
          多 Key 自动轮询 + 429 冷却。
        </small>
      </div>

      <ModelPriorityEditor
        models={models}
        running={running}
        chatTests={chatTests}
        onMove={onMoveModel}
        onMoveTop={onMoveTop}
        onRemove={onRemoveModel}
        onAdd={onAddModel}
        onTest={onTestModel}
        addPlaceholder="添加模型，如 gpt-4.1"
      />

      <GatewayModelMapEditor modelMap={modelMap} onChange={onModelMap} />

      <div className="form-group">
        <label>上游 Base URL（含 /v1）</label>
        <input
          type="text"
          value={apiBaseUrl}
          placeholder="https://api.openai.com/v1"
          onChange={(event) => onApiBaseUrl(event.target.value)}
        />
      </div>
      <small className="form-hint">
        当前生效上游：<code>{apiBaseUrl.trim() || "https://api.openai.com/v1"}{upstreamPath}</code>。
        禁跟随重定向，3xx 一律判 502，防 SSRF 与 token 外泄。
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
              外部监听需配置至少 24 位鉴权 token，否则局域网设备可盗用你的上游凭证。
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
          保存网关配置
        </button>
      </div>
      <small className="form-hint">
        修改后需重启代理才生效（启动按钮会自动先保存当前配置）。模型优先级与映射表保存后即时热更新运行中代理。
      </small>
    </div>
  );
}
