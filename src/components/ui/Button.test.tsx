import { describe, expect, test, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Button } from "./Button";

describe("Button", () => {
  test("loading button blocks duplicate activation and exposes busy state", () => {
    const onClick = vi.fn();
    render(
      <Button loading loadingLabel="正在保存" onClick={onClick}>
        保存
      </Button>,
    );

    const button = screen.getByRole("button", { name: "正在保存" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  test("button defaults to type button so it cannot submit a parent form", () => {
    render(<Button>取消</Button>);
    expect(screen.getByRole("button", { name: "取消" })).toHaveAttribute(
      "type",
      "button",
    );
  });

  test("renders its icon and children when not loading", () => {
    render(
      <Button icon={<span data-testid="icon" aria-hidden="true">*</span>}>
        提交
      </Button>,
    );
    expect(screen.getByTestId("icon")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "提交" })).toBeEnabled();
  });
});
