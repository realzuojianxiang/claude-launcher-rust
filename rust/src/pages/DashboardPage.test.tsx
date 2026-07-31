import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DashboardPage } from "./DashboardPage";
import { configFixture } from "../test/fixtures";

const { invokeMock, deferred } = vi.hoisted(() => {
  const invokeMock = vi.fn();
  const deferred: { promise: Promise<unknown>; resolve: (v: unknown) => void } = {
    promise: Promise.resolve(),
    resolve: () => {},
  };
  return { invokeMock, deferred };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe("DashboardPage", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status")
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      return Promise.resolve("");
    });
  });

  it("renders a loading state while the status is unresolved", async () => {
    deferred.promise = new Promise((res) => {
      // never resolve within the assertion window
      setTimeout(res, 5000);
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status") return deferred.promise;
      return Promise.resolve("");
    });

    render(<DashboardPage config={configFixture} />);
    expect(await screen.findByText("检测中…")).toBeInTheDocument();
  });

  it("renders an alert with a retry button when status rejects", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status") return Promise.reject(new Error("offline"));
      return Promise.resolve("");
    });

    render(<DashboardPage config={configFixture} />);
    const alert = await screen.findByRole("alert", {
      name: "无法读取 CLIProxyAPI 状态",
    });
    expect(alert).toHaveTextContent("offline");
    expect(
      screen.getByRole("button", { name: "重新检测" })
    ).toBeInTheDocument();
  });

  it("renders the running state from a successful retry", async () => {
    let call = 0;
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status") {
        call += 1;
        if (call === 1) return Promise.reject(new Error("offline"));
        return Promise.resolve({ running: true, url: "http://localhost:8317" });
      }
      return Promise.resolve("");
    });

    render(<DashboardPage config={configFixture} />);
    await screen.findByRole("alert", { name: "无法读取 CLIProxyAPI 状态" });
    fireEvent.click(screen.getByRole("button", { name: "重新检测" }));

    expect(await screen.findByText("运行中")).toBeInTheDocument();
  });

  it("keeps profile count and work directory unchanged across a retry", async () => {
    let call = 0;
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status") {
        call += 1;
        if (call === 1) return Promise.reject(new Error("offline"));
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      }
      return Promise.resolve("");
    });

    render(<DashboardPage config={configFixture} />);
    expect(screen.getByText("1 套（默认 CLIProxyAPI）")).toBeInTheDocument();
    expect(screen.getByText("D:\\work")).toBeInTheDocument();

    await screen.findByRole("alert", { name: "无法读取 CLIProxyAPI 状态" });
    fireEvent.click(screen.getByRole("button", { name: "重新检测" }));
    await screen.findByText("未运行");

    expect(screen.getByText("1 套（默认 CLIProxyAPI）")).toBeInTheDocument();
    expect(screen.getByText("D:\\work")).toBeInTheDocument();
  });
});
