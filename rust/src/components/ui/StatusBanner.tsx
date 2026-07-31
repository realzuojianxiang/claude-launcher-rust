import { X, CheckCircle, AlertTriangle, AlertCircle, Info } from "lucide-react";

export type StatusKind = "success" | "warning" | "error" | "info";

export interface StatusMessage {
  kind: StatusKind;
  title: string;
  detail?: string;
}

export interface StatusBannerProps {
  message: StatusMessage | null;
  action?: React.ReactNode;
  onDismiss?: () => void;
}

const icons: Record<StatusKind, React.ReactNode> = {
  success: <CheckCircle aria-hidden="true" />,
  warning: <AlertTriangle aria-hidden="true" />,
  error: <AlertCircle aria-hidden="true" />,
  info: <Info aria-hidden="true" />,
};

export function StatusBanner({ message, action, onDismiss }: StatusBannerProps) {
  if (!message) return null;

  const isAlert = message.kind === "error";
  const titleId = "status-banner-title";
  const detailId = "status-banner-detail";

  return (
    <div
      role={isAlert ? "alert" : "status"}
      aria-live={isAlert ? "assertive" : "polite"}
      aria-labelledby={titleId}
      aria-describedby={message.detail ? detailId : undefined}
      className={`status-banner status-banner--${message.kind}`}
    >
      <span className="status-banner__icon">{icons[message.kind]}</span>
      <div className="status-banner__body">
        <p id={titleId} className="status-banner__title">
          {message.title}
        </p>
        {message.detail ? (
          <p id={detailId} className="status-banner__detail">
            {message.detail}
          </p>
        ) : null}
      </div>
      {action ? <div className="status-banner__action">{action}</div> : null}
      {onDismiss ? (
        <button
          type="button"
          onClick={onDismiss}
          className="status-banner__dismiss"
          aria-label="关闭"
        >
          <X aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}
