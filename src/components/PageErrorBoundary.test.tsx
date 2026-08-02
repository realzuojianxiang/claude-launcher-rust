import { describe, expect, test, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { PageErrorBoundary } from "./PageErrorBoundary";

function Thrower({ shouldThrow }: { shouldThrow: boolean }) {
  if (shouldThrow) {
    throw new Error("page render failed");
  }
  return <div data-testid="ok">正常内容</div>;
}

describe("PageErrorBoundary", () => {
  test("renders a named alert and reload action when child throws", () => {
    const reload = vi.fn();
    render(
      <PageErrorBoundary resetKey="once" onReload={reload}>
        <Thrower shouldThrow={true} />
      </PageErrorBoundary>,
    );

    expect(screen.getByRole("alert", { name: "页面暂时无法显示" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重新载入应用" }));
    expect(reload).toHaveBeenCalledTimes(1);
  });

  test("renders children when there is no error", () => {
    render(
      <PageErrorBoundary resetKey="fine">
        <Thrower shouldThrow={false} />
      </PageErrorBoundary>,
    );
    expect(screen.getByTestId("ok")).toBeInTheDocument();
  });
});
