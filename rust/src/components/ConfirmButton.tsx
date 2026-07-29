// 二次确认删除按钮：第一次点击进入「确认」状态（红色高亮），3 秒内再点一次才真正执行删除；
// 超时自动还原。所有删除入口统一使用，防止误删。自动阻止事件冒泡（如历史目录行的点击选中）。
// 从 App.tsx 抽出，自带 React 导入。

import {
  type ReactNode,
  type MouseEvent as ReactMouseEvent,
  useEffect,
  useRef,
  useState,
} from "react";

export function ConfirmButton({
  className,
  title,
  confirmLabel = "确认?",
  onConfirm,
  children,
}: {
  className: string;
  title: string;
  confirmLabel?: string;
  onConfirm: () => void;
  children: ReactNode;
}) {
  const [armed, setArmed] = useState(false);
  const timer = useRef<number | null>(null);
  // 组件卸载时清掉计时器，避免泄漏
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    []
  );
  const handleClick = (e: ReactMouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    if (!armed) {
      setArmed(true);
      timer.current = window.setTimeout(() => setArmed(false), 3000);
    } else {
      if (timer.current !== null) window.clearTimeout(timer.current);
      setArmed(false);
      onConfirm();
    }
  };
  return (
    <button
      className={`${className}${armed ? " confirm-armed" : ""}`}
      title={armed ? "再点一次确认删除（3 秒后自动取消）" : title}
      onClick={handleClick}
    >
      {armed ? confirmLabel : children}
    </button>
  );
}
