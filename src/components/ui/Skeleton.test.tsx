import { describe, expect, test } from "vitest";
import { render, screen } from "@testing-library/react";
import { Skeleton } from "./Skeleton";

describe("Skeleton", () => {
  test("skeleton announces what is loading without exposing decorative bars", () => {
    render(<Skeleton label="正在读取配置" lines={3} />);
    expect(screen.getByRole("status")).toHaveTextContent("正在读取配置");
    expect(screen.getAllByTestId("skeleton-line")).toHaveLength(3);
    expect(screen.getAllByTestId("skeleton-line")[0]).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });

  test("single line renders by default", () => {
    render(<Skeleton label="加载中" />);
    expect(screen.getAllByTestId("skeleton-line")).toHaveLength(1);
  });
});
