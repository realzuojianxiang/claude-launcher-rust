// OpenAI 透传网关页（本地 8084 通用「OpenAI 入站 ↔ OpenAI 协议上游」统计网关）。
// 不做协议转换，只把 Codex 等 OpenAI 原生客户端的请求透传到「协议网关」页面里已配置好的
// provider（deepseek / glm-5.2 等），并从响应抽取 token 用量写进与 8083 共享的统计面板。
// 主要价值：让 OpenAI 原生客户端（如 Codex）也走统一入口 + Key 池 + 自动统计，而非直连各家。
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Config, KeyPoolStatus, OpenAiGwStatus } from "../types";
import { MessageBanner } from "../components/MessageBanner";

const PORT = 8084;

export function OpenAiGatewayPage({ config }: { config: Config | null }) {
  const [status, setStatus] = useState<OpenAiGwStatus | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [keyPool, setKeyPool] = useState<KeyPoolStatus | null>(null);
  const [kpExpanded, setKpExpanded] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<OpenAiGwStatus>("openai_gw_status"));
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const providers = config?.gateway?.providers || [];
  const running = status?.running ?? false;

  // 起停前先刷新一次（配置可能已在「协议网关」页改动）
  const start = async () => {
    setBusy(true);
    setMsg(null);
    try {
      setMsg(await invoke<string>("openai_gw_start"));
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
      setMsg(await invoke<string>("openai_gw_stop"));
      await refresh();
    } catch (e) {
      setMsg(`❌ ${e}`);
    } finally {
      setBusy(false);
    }
  };

  // Key 池状态轮询：代理运行时每 2s 刷新
  useEffect(() => {
    let timer: number | undefined;
    const tick = async () => {
      try {
        setKeyPool(await invoke<KeyPoolStatus>("openai_gw_pool"));
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

  const icon = status === null ? "❓" : running ? "✅" : "❌";
  const statusText = status === null ? "未知" : running ? "运行中" : "未运行";
  const endpoints = status?.endpoints?.length
    ? status!.endpoints
    : [
        `http://127.0.0.1:${PORT}/v1/chat/completions`,
        `http://127.0.0.1:${PORT}/v1/responses`,
      ];

  return (
    <div className="page">
      <h2 className="page-title">OpenAI 网关</h2>
      <p className="page-desc">
        8084 透传网关：把 Codex 等 OpenAI 原生客户端（Chat Completions / Responses 协议）的请求
        原样转发到「协议网关」页面里已配置的 provider（deepseek / glm-5.2 等），并抽取 token
        用量写进与 8083 共享的统计面板。只透传、不转换协议，对客户端完全透明。
      </p>
      <MessageBanner msg={msg} />

      {/* 复用的 provider 列表 */}
      <div className="card">
        <div className="form-group">
          <label>复用以下 Provider（来自「协议网关」配置）</label>
          {providers.length === 0 ? (
            <p className="form-hint">
              尚未配置任何 provider。请先到「协议网关」页面添加并配置 provider（含 API Key 与模型列表）。
            </p>
          ) : (
            <ul className="provider-list">
              {providers.map((p) => (
                <li key={p.id} className="provider-item">
                  <span className="provider-name">{p.name}</span>
                  <span className="provider-meta">
                    {p.models.length > 0
                      ? p.models.join("、")
                      : "（未配置模型，将无法按 model 路由）"}
                  </span>
                </li>
              ))}
            </ul>
          )}
          <small className="form-hint">
            请求按 <code>model</code> 字段路由：命中某 provider 的模型列表则走该 provider，否则回落到当前
            选中的 provider（model 名原样透传给上游）。
          </small>
        </div>
      </div>

      {/* 状态 + 控制 */}
      <div className="card">
        <div className="gw-status-row">
          <span className="gw-status-icon">{icon}</span>
          <span className="gw-status-text">{statusText}</span>
          <span className="gw-status-url">{status?.url || `http://127.0.0.1:${PORT}`}</span>
          <div className="gw-status-actions">
            <button
              className="btn btn-primary"
              onClick={start}
              disabled={busy || running}
            >
              启动
            </button>
            <button
              className="btn btn-secondary"
              onClick={stop}
              disabled={busy || !running}
            >
              停止
            </button>
            <button className="btn btn-ghost" onClick={refresh} disabled={busy}>
              刷新
            </button>
          </div>
        </div>

        {running && (
          <div className="gw-endpoints">
            <label>客户端接入端点（Codex 等设置 base_url）</label>
            {endpoints.map((e) => (
              <code key={e} className="gw-endpoint">{e}</code>
            ))}
            <small className="form-hint">
              例如 Codex：<code>OPENAI_BASE_URL=http://127.0.0.1:{PORT}/v1</code>
              ，即可享受统一 Key 池与 token 统计。
            </small>
          </div>
        )}
      </div>

      {/* Key 池状态 */}
      {keyPool && (
        <div className="card">
          <div
            className="gw-kp-head"
            onClick={() => setKpExpanded((v) => !v)}
            role="button"
          >
            <label>Key 池状态</label>
            <span className="gw-kp-summary">
              {keyPool.total} 个 Key · 可用 {keyPool.available} · 冷却 {keyPool.cooling}
            </span>
          </div>
          {kpExpanded && (
            <ul className="provider-list">
              {keyPool.keys.map((k) => (
                <li key={k.index} className="provider-item">
                  <span className="provider-name">{k.masked}</span>
                  <span className="provider-meta">
                    {k.cooling ? `冷却中（${k.cooldown_remaining_secs}s）` : "可用"}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
