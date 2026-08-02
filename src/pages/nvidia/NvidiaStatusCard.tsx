export function NvidiaStatusCard({
  icon,
  statusText,
  endpoint,
  busy,
  running,
  testBusy,
  testResult,
  onStart,
  onStop,
  onRefresh,
  onTest,
}: {
  icon: string;
  statusText: string;
  endpoint: string;
  busy: boolean;
  running: boolean;
  testBusy: boolean;
  testResult: string | null;
  onStart: () => void;
  onStop: () => void;
  onRefresh: () => void;
  onTest: () => void;
}) {
  return (
    <div className="card status-card">
      <div className="status-header">
        <span className="status-icon">{icon}</span>
        <span className="status-text">{statusText}</span>
      </div>
      <div className="status-url">{endpoint}</div>
      <div className="status-buttons">
        <button className="btn btn-start" onClick={onStart} disabled={busy || running}>
          ▶ 启动
        </button>
        <button className="btn btn-stop" onClick={onStop} disabled={busy || !running}>
          ⏹ 停止
        </button>
        <button className="btn btn-refresh" onClick={onRefresh} disabled={busy}>
          🔄 刷新
        </button>
        <button className="btn btn-test" onClick={onTest} disabled={testBusy}>
          {testBusy ? "测试中…" : "🔌 测试连接"}
        </button>
      </div>
      {testResult && (
        <div className="test-result">
          <code>{testResult}</code>
        </div>
      )}
      <small className="form-hint">
        在 Claude Code 中把 ANTHROPIC_BASE_URL 指向上面的地址（去掉 /v1/messages），
        即可通过本代理访问 NVIDIA NIM。
      </small>
    </div>
  );
}
