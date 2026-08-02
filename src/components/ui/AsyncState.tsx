import { useId } from "react";
import { LoaderCircle, WifiOff, AlertCircle } from "lucide-react";

export interface AsyncStateProps {
  title: string;
  detail?: string;
  kind?: "loading" | "error" | "network";
  action?: React.ReactNode;
  compact?: boolean;
}

const icons: Record<Exclude<AsyncStateProps["kind"], undefined>, React.ReactNode> = {
  loading: <LoaderCircle className="ui-spinner" aria-hidden="true" />,
  error: <AlertCircle aria-hidden="true" />,
  network: <WifiOff aria-hidden="true" />,
};

export function AsyncState({
  title,
  detail,
  kind = "loading",
  action,
  compact = false,
}: AsyncStateProps) {
  // 每实例生成唯一 id，避免同页多个 AsyncState/StatusBanner 在
  // aria-labelledby/aria-describedby 上指向同一字面量 id（a11y 冲突）。
  const titleId = useId();
  const detailId = useId();
  const isAlert = kind === "error" || kind === "network";

  return (
    <div
      role={isAlert ? "alert" : "status"}
      aria-live={isAlert ? "assertive" : "polite"}
      aria-labelledby={titleId}
      aria-describedby={detail ? detailId : undefined}
      className={`async-state async-state--${kind} ${compact ? "async-state--compact" : ""}`.trim()}
    >
      <div className="async-state__icon">{icons[kind]}</div>
      <p id={titleId} className="async-state__title">
        {title}
      </p>
      {detail ? (
        <p id={detailId} className="async-state__detail">
          {detail}
        </p>
      ) : null}
      {action ? <div className="async-state__action">{action}</div> : null}
    </div>
  );
}
