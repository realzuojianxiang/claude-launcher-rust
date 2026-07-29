// 关于页：实现语言 / 版本 / 平台 / 连接注入方式说明。从 App.tsx 抽出，无依赖。

export function AboutPage() {
  return (
    <div className="page">
      <h2 className="page-title">关于</h2>
      <div className="card">
        <p>
          <strong>Claude Launcher</strong> (Rust 实现)
        </p>
        <p className="muted">快速启动 Claude Code + CLIProxyAPI 的桌面启动器。</p>
        <ul className="kv-list">
          <li>
            <span>实现语言</span>
            <span>Rust (Tauri v2 + React)</span>
          </li>
          <li>
            <span>版本</span>
            <span>1.0.0</span>
          </li>
          <li>
            <span>平台</span>
            <span>Windows</span>
          </li>
          <li>
            <span>连接参数注入</span>
            <span>进程环境变量（不改动 settings.json）</span>
          </li>
        </ul>
      </div>
    </div>
  );
}
