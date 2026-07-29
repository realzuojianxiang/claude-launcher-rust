// 代理管理页：启动/停止/刷新状态，以及 CLIProxyAPI 执行目录的指定。
// 从 App.tsx 抽出。props：config、onConfig。

import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Config, ProxyStatus } from "../types";
import { MessageBanner } from "../components/MessageBanner";

export function ProxyPage({
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
