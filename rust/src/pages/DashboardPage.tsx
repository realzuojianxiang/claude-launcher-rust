// 仪表盘：状态总览（配置快照），纯展示，无后端探测
// 从 App.tsx 抽出。props：config（全局配置快照）。

import type { Config } from "../types";

export function DashboardPage({ config }: { config: Config | null }) {
  const profileCount = config?.profiles?.length ?? 0;
  const defaultProfile = profileCount > 0 ? config!.profiles[0].name : "无";

  return (
    <div className="page">
      <h2 className="page-title">仪表盘</h2>
      <p className="page-desc">状态总览与快速入口。</p>
      <div className="grid">
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
