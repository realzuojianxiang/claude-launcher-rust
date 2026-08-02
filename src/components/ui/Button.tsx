import { LoaderCircle } from "lucide-react";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

export interface ButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  variant?: ButtonVariant;
  loading?: boolean;
  loadingLabel?: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
}

export function Button({
  variant = "secondary",
  loading = false,
  loadingLabel = "处理中",
  icon,
  children,
  className = "",
  disabled,
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <button
      {...props}
      type={type}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={`ui-button ui-button--${variant} ${className}`.trim()}
    >
      {loading ? <LoaderCircle className="ui-spinner" aria-hidden="true" /> : icon}
      <span>{loading ? loadingLabel : children}</span>
    </button>
  );
}
