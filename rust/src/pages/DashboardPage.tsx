// 仪表盘：状态总览，启动时探测一次代理状态
// 从 App.tsx 抽出。props：config（全局配置快照）。

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Config, ProxyStatus } from "../types";

export function DashboardPage({ config }: { config: Config | null }) {
  const [proxyRunning, setProxyRunning] = useState<boolean | null>(null);

  const refresh = useCallback(async () => {
    if (!config) return;
    try {
      const s = await invoke<ProxyStatus>("cliproxyapi_status");
      setProxyRunning(s.running);
    } catch {
      setProxyRunning(null);
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
          <div className="card-value">
            {proxyRunning === null ? "检测中…" : proxyRunning ? "运行中" : "未运行"}
          </div>
        </div>
        <div className="card stat-card">
          <div className="card-label">供应商配置</div>
          <div className="card-value" style={{ fontSize: 13 }}>
            {profileCount} 套（默认 {defaultProfile}）
          </div>
        </div>
        <div className="card stat-card">
          <div className="card-label">工作目录</div>
          <div className="card-value" style={{ fontSize: 13, wordBreak: "break-all" }}>
            {config?.work_dir || "未选择"}
          </div>
        </div>
      </div>
    </div>
  );
}
