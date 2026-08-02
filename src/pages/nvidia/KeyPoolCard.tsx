import type { KeyPoolStatus } from "../../types";

export function KeyPoolCard({
  keyPool,
  expanded,
  cooldown,
  onToggle,
}: {
  keyPool: KeyPoolStatus | null;
  expanded: boolean;
  cooldown: number;
  onToggle: () => void;
}) {
  return (
    <div className="card">
      <div className="status-header">
        <span className="status-icon">🔑</span>
        <span className="status-text">Key 池状态</span>
      </div>
      {!keyPool || !keyPool.running ? (
        <p className="form-hint">代理未运行，启动后即可查看 Key 可用与冷却情况。</p>
      ) : (
        <>
          <div className="kp-summary">
            <span className="kp-badge kp-ok">
              可用 {keyPool.available}/{keyPool.total}
            </span>
            {keyPool.cooling > 0 && (
              <span className="kp-badge kp-cool">冷却中 {keyPool.cooling}</span>
            )}
          </div>
          <ul className="kp-list">
            {(expanded ? keyPool.keys : keyPool.keys.slice(0, 3)).map((key) => (
              <li
                key={key.index}
                className={key.cooling ? "kp-item kp-item-cool" : "kp-item"}
              >
                <span className="kp-idx">#{key.index + 1}</span>
                <span className="kp-masked">{key.masked}</span>
                {key.cooling ? (
                  <span className="kp-state kp-cool">
                    冷却 {key.cooldown_remaining_secs}s
                  </span>
                ) : (
                  <span className="kp-state kp-ok">可用</span>
                )}
              </li>
            ))}
          </ul>
          {keyPool.keys.length > 3 && (
            <button className="kp-toggle" onClick={onToggle}>
              {expanded ? "收起" : `展开其余 ${keyPool.keys.length - 3} 个`}
              <span className={expanded ? "kp-toggle-arrow up" : "kp-toggle-arrow"}>
                ▾
              </span>
            </button>
          )}
          <small className="form-hint">
            命中 429 的 Key 会被冷却 {cooldown}s 后自动恢复，期间流量自动导向其他可用 Key。
          </small>
        </>
      )}
    </div>
  );
}
