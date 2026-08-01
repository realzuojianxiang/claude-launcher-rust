// 仪表盘：状态总览，启动时探测一次代理状态，失败可重试
// 从 App.tsx 抽出。props：config（全局配置快照）。

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CircleCheck, CircleX, LoaderCircle } from "lucide-react";
import type { Config, ProxyStatus, AsyncStatus } from "../types";
import { Button } from "../components/ui/Button";
import { AsyncState } from "../components/ui/AsyncState";

export function DashboardPage({ config }: { config: Config | null }) {
  const [proxyStatus, setProxyStatus] = useState<AsyncStatus>("loading");
  const [proxyRunning, setProxyRunning] = useState<boolean | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!config) return;
    setProxyStatus("loading");
    try {
      const s = await invoke<ProxyStatus>("cliproxyapi_status");
      setProxyRunning(s.running);
      setProxyStatus("ready");
      setStatusError(null);
    } catch (e) {
      setProxyStatus("error");
      setStatusError(e instanceof Error ? e.message : String(e));
    }
  }, [config]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const profileCount = config?.profiles?.length ?? 0;
  const defaultProfile = profileCount > 0 ? config!.profiles[0].name : "无";

  return (
    <div className="page">
      <h2 className="page-title">仪表盘</h2>
      <p className="page-desc">状态总览与快速入口。</p>
      <div className="grid">
        <div className="card stat-card">
          <div className="card-label">CLIProxyAPI</div>
          {proxyStatus === "error" ? (
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
            <div
              className="card-value"
              role={proxyStatus === "loading" ? "status" : undefined}
            >
              <span className="status-icon" aria-hidden="true">
                {proxyStatus === "loading" ? (
                  <LoaderCircle className="ui-spinner" />
                ) : proxyRunning ? (
                  <CircleCheck />
                ) : (
                  <CircleX />
                )}
              </span>{" "}
              {proxyStatus === "loading"
                ? "检测中…"
                : proxyRunning
                ? "运行中"
                : "未运行"}
            </div>
          )}
        </div>
        <div className="card stat-card">
          <div className="card-label">供应商配置</div>
          <div className="card-value card-value--wrap">
            {profileCount} 套（默认 {defaultProfile}）
          </div>
        </div>
        <div className="card stat-card">
          <div className="card-label">工作目录</div>
          <div className="card-value card-value--wrap">
            {config?.work_dir || "未选择"}
          </div>
        </div>
      </div>
    </div>
  );
}
