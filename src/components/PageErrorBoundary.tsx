import { Component, type ReactNode } from "react";
import { AlertCircle } from "lucide-react";
import { Button } from "./ui/Button";

export interface PageErrorBoundaryProps {
  resetKey: string;
  onReload?: () => void;
  children: ReactNode;
}

interface State {
  hasError: boolean;
}

export class PageErrorBoundary extends Component<PageErrorBoundaryProps, State> {
  state: State = { hasError: false };

  static getDerivedStateFromError(): State {
    return { hasError: true };
  }

  componentDidUpdate(prevProps: PageErrorBoundaryProps) {
    if (prevProps.resetKey !== this.props.resetKey && this.state.hasError) {
      this.setState({ hasError: false });
    }
  }

  render() {
    if (this.state.hasError) {
      // 以 resetKey 命名空间的稳定 id 替代字面量常量，避免同页多个错误区域
      // 与 AsyncState/StatusBanner 在 aria-labelledby 上指向同一 id。
      const titleId = `page-error-title-${this.props.resetKey}`;
      return (
        <div
          className="async-state async-state--error"
          role="alert"
          aria-labelledby={titleId}
        >
          <div className="async-state__icon">
            <AlertCircle aria-hidden="true" />
          </div>
          <p id={titleId} className="async-state__title">
            页面暂时无法显示
          </p>
          <p className="async-state__detail">
            加载该页面时发生错误。重新载入应用通常可恢复。
          </p>
          <div className="async-state__action">
            <Button
              variant="primary"
              onClick={() => {
                if (this.props.onReload) {
                  this.props.onReload();
                } else {
                  window.location.reload();
                }
              }}
            >
              重新载入应用
            </Button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
