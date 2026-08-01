// 代理管理页：启动/停止/刷新状态，以及 CLIProxyAPI 执行目录的指定。
// 从 App.tsx 抽出。props：config、onConfig。

import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CircleCheck,
  CircleX,
  LoaderCircle,
  RefreshCw,
  Play,
  Square,
} from "lucide-react";
import type { Config, ProxyStatus, AsyncStatus } from "../types";
import {
  Button,
} from "../components/ui/Button";
import {
  StatusBanner,
  type StatusMessage,
} from "../components/ui/StatusBanner";
import { AsyncState } from "../components/ui/AsyncState";

type BusyAction = "refresh" | "start" | "stop" | "pick" | "clear" | null;

export function ProxyPage({
  config,
  onConfig,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
}) {
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [statusAsync, setStatusAsync] = useState<AsyncStatus>("loading");
  const [statusError, setStatusError] = useState<string | null>(null);
  const [msg, setMsg] = useState<StatusMessage | null>(null);
  const [action, setAction] = useState<BusyAction>(null);
  // 执行目录本地态：优先回显配置值，空则提示"未指定（用 exe 所在目录）"
  const [cliDir, setCliDir] = useState("");

  useEffect(() => {
    if (config) setCliDir(config.cliproxyapi_dir || "");
  }, [config]);

  const refresh = useCallback(async () => {
    setStatusAsync("loading");
    try {
      setStatus(await invoke<ProxyStatus>("cliproxyapi_status"));
      setStatusAsync("ready");
      setStatusError(null);
    } catch (e) {
      setStatus(null);
      setStatusAsync("error");
      setStatusError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 选择 CLIProxyAPI 执行目录：后端已在本命令内持久化，空串为取消哨兵，拒绝为权限错误
  const pickCliDir = async () => {
    if (action) return;
    setAction("pick");
    setMsg(null);
    try {
      const dir = await invoke<string>("select_cli_dir");
      if (dir === "") return; // 取消哨兵：静默 no-op
      setCliDir(dir);
      if (config) onConfig({ ...config, cliproxyapi_dir: dir });
      setMsg({ kind: "success", title: `已选择执行目录：${dir}` });
    } catch (e) {
      setMsg({
        kind: "error",
        title: "无法选择 CLIProxyAPI 目录",
        detail: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setAction(null);
    }
  };

  // 清空执行目录：回到"未指定"，持久化空串
  const clearCliDir = async () => {
    if (action || !config) return;
    setAction("clear");
    setMsg(null);
    try {
      await invoke<string>("set_cli_dir", { dir: "" });
      setCliDir("");
      onConfig({ ...config, cliproxyapi_dir: "" });
      setMsg({ kind: "success", title: "已清空执行目录" });
    } catch (e) {
      setMsg({
        kind: "error",
        title: "清空失败",
        detail: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setAction(null);
    }
  };

  const start = async () => {
    if (action) return;
    setAction("start");
    setMsg(null);
    try {
      await invoke<string>("start_cliproxyapi");
      await refresh();
    } catch (e) {
      setMsg({
        kind: "error",
        title: "无法启动 CLIProxyAPI",
        detail: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setAction(null);
    }
  };

  const stop = async () => {
    if (action) return;
    setAction("stop");
    setMsg(null);
    try {
      await invoke<string>("stop_cliproxyapi");
      await refresh();
    } catch (e) {
      setMsg({
        kind: "error",
        title: "无法停止 CLIProxyAPI",
        detail: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setAction(null);
    }
  };

  const running = status?.running ?? false;
  const busy = action !== null;

  return (
    <div className="page">
      <h2 className="page-title">CLIProxyAPI 管理</h2>
      <p className="page-desc">本地代理的启动、停止与状态查询。</p>
      <StatusBanner message={msg} />

      <div className="card status-card">
        {statusAsync === "error" ? (
          <AsyncState
            kind="network"
            title="无法读取 CLIProxyAPI 状态"
            detail={statusError ?? undefined}
            action={
              <Button variant="secondary" onClick={refresh}>
                重新检测
              </Button>
            }
          />
        ) : (
          <>
            <div className="status-header">
              <span className="status-icon" aria-hidden="true">
                {statusAsync === "loading" ? (
                  <LoaderCircle className="ui-spinner" />
                ) : running ? (
                  <CircleCheck />
                ) : (
                  <CircleX />
                )}
              </span>
              <span className="status-text">
                {statusAsync === "loading"
                  ? "检测中…"
                  : running
                  ? status?.message || "运行中"
                  : "未运行"}
              </span>
            </div>
            <div className="status-url">
              {config?.anthropic_url || "http://localhost:8317"}
            </div>
            <div className="status-buttons">
              <Button
                variant="secondary"
                icon={<Play aria-hidden="true" />}
                loading={action === "start"}
                loadingLabel="正在启动"
                disabled={busy || running}
                onClick={start}
              >
                启动
              </Button>
              <Button
                variant="secondary"
                icon={<Square aria-hidden="true" />}
                loading={action === "stop"}
                loadingLabel="正在停止"
                disabled={busy || !running}
                onClick={stop}
              >
                停止
              </Button>
              <Button
                variant="ghost"
                icon={<RefreshCw aria-hidden="true" />}
                loading={action === "refresh"}
                loadingLabel="正在刷新"
                disabled={busy}
                onClick={refresh}
              >
                刷新
              </Button>
            </div>
          </>
        )}
      </div>

      {/* 执行目录：不指定则回退到 cliproxyapi.exe 所在目录 */}
      <div className="card">
        <div className="form-group">
          <label htmlFor="proxy-cli-dir">CLIProxyAPI 执行目录</label>
          <div className="input-row">
            <input
              id="proxy-cli-dir"
              type="text"
              value={cliDir}
              placeholder="未指定（用 cliproxyapi.exe 所在目录）"
              readOnly
            />
            <Button variant="secondary" loading={action === "pick"} onClick={pickCliDir}>
              选择执行目录
            </Button>
            {cliDir && (
              <Button
                variant="ghost"
                loading={action === "clear"}
                onClick={clearCliDir}
              >
                清空
              </Button>
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
