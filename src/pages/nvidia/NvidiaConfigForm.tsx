import type { NvTestState } from "../../types";
import { ModelPriorityEditor } from "./ModelPriorityEditor";

function generateAuthToken(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function NvidiaConfigForm({
  keysText,
  models,
  baseUrl,
  host,
  port,
  cooldown,
  retries,
  timeout,
  authToken,
  running,
  chatTests,
  onKeysText,
  onBaseUrl,
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
  baseUrl: string;
  host: string;
  port: number;
  cooldown: number;
  retries: number;
  timeout: number;
  authToken: string;
  running: boolean;
  chatTests: NvTestState["chatTests"];
  onKeysText: (value: string) => void;
  onBaseUrl: (value: string) => void;
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

  return (
    <div className="card">
      <div className="form-group">
        <label>NVIDIA API Keys（每行一个，或逗号分隔）</label>
        <textarea
          className="env-val"
          style={{ minHeight: 88, fontFamily: "'SF Mono', monospace", width: "100%" }}
          value={keysText}
          placeholder={"nvapi-xxx1\nnvapi-xxx2"}
          onChange={(event) => onKeysText(event.target.value)}
        />
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
      />
      <div className="form-group">
        <label>NVIDIA Base URL</label>
        <input
          type="text"
          value={baseUrl}
          placeholder="https://integrate.api.nvidia.com/v1"
          onChange={(event) => onBaseUrl(event.target.value)}
        />
      </div>
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
            onChange={(event) => onPort(Number(event.target.value))}
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
          <small className="form-hint">
            默认 600 秒；首个有效输出超时会复用当前 Key 并尝试下一模型。
          </small>
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
          保存 NVIDIA 配置
        </button>
      </div>
      <small className="form-hint">
        修改后需重启代理才生效（启动按钮会自动先保存当前配置）。
      </small>
    </div>
  );
}
