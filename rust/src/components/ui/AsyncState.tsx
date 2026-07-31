import { LoaderCircle, WifiOff, ShieldAlert, FolderOpen, AlertCircle } from "lucide-react";

export interface AsyncStateProps {
  title: string;
  detail?: string;
  kind?: "loading" | "empty" | "error" | "permission" | "network";
  action?: React.ReactNode;
  compact?: boolean;
}

const icons: Record<Exclude<AsyncStateProps["kind"], undefined>, React.ReactNode> = {
  loading: <LoaderCircle className="ui-spinner" aria-hidden="true" />,
  empty: <FolderOpen aria-hidden="true" />,
  error: <AlertCircle aria-hidden="true" />,
  permission: <ShieldAlert aria-hidden="true" />,
  network: <WifiOff aria-hidden="true" />,
};

export function AsyncState({
  title,
  detail,
  kind = "loading",
  action,
  compact = false,
}: AsyncStateProps) {
  const isAlert = kind === "error" || kind === "permission" || kind === "network";
  const titleId = "async-state-title";
  const detailId = "async-state-detail";

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
