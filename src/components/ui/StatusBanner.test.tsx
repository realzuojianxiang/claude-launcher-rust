import { describe, expect, test, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { StatusBanner } from "./StatusBanner";

describe("StatusBanner", () => {
  test("error message is announced immediately and exposes its recovery action", () => {
    render(
      <StatusBanner
        message={{ kind: "error", title: "保存失败", detail: "配置未更改" }}
        action={<button type="button">重试</button>}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("保存失败");
    expect(screen.getByRole("button", { name: "重试" })).toBeEnabled();
  });

  test("success message uses a polite status region", () => {
    render(<StatusBanner message={{ kind: "success", title: "已保存" }} />);
    expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
  });

  test("dismissible message calls onDismiss", () => {
    const onDismiss = vi.fn();
    render(
      <StatusBanner
        message={{ kind: "info", title: "提示" }}
        onDismiss={onDismiss}
      />,
    );
    screen.getByRole("button", { name: "关闭" }).click();
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  test("renders nothing when message is null", () => {
    const { container } = render(<StatusBanner message={null} />);
    expect(container.firstChild).toBeNull();
  });
});
