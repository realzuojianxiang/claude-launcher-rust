import { describe, expect, test } from "vitest";
import { render, screen } from "@testing-library/react";
import { AsyncState } from "./AsyncState";

describe("AsyncState", () => {
  test("network failure is not rendered as an empty state", () => {
    render(
      <AsyncState
        kind="network"
        title="无法连接本地代理"
        action={<button type="button">重新检测</button>}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("无法连接本地代理");
    expect(screen.getByRole("button", { name: "重新检测" })).toBeEnabled();
  });

  test("loading state announces what is loading", () => {
    render(<AsyncState kind="loading" title="正在读取配置" />);
    expect(screen.getByRole("status")).toHaveTextContent("正在读取配置");
  });

  test("empty state offers a polite explanation", () => {
    render(<AsyncState kind="empty" title="暂无日志" detail="启动代理后可见" />);
    expect(screen.getByRole("status")).toHaveTextContent("暂无日志");
  });

  test("permission state is an alert", () => {
    render(<AsyncState kind="permission" title="需要目录访问权限" />);
    expect(screen.getByRole("alert")).toHaveTextContent("需要目录访问权限");
  });
});
