import { fireEvent, render, screen, waitFor, act } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyPage } from "./ProxyPage";
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

describe("ProxyPage", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status")
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      if (command === "select_cli_dir") return Promise.resolve("C:/cli");
      if (command === "start_cliproxyapi") return Promise.resolve("started");
      if (command === "stop_cliproxyapi") return Promise.resolve("stopped");
      return Promise.resolve("");
    });
  });

  it("renders a readable alert when status refresh rejects", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status") return Promise.reject(new Error("offline"));
      return Promise.resolve("");
    });
    render(<ProxyPage config={configFixture} onConfig={vi.fn()} />);
    const alert = await screen.findByRole("alert", {
      name: "无法读取 CLIProxyAPI 状态",
    });
    expect(alert).toHaveTextContent("offline");
  });

  it("sets aria-busy on start, disables stop/refresh, and prevents duplication", async () => {
    let resolveStart!: (v: unknown) => void;
    deferred.promise = new Promise((res) => {
      resolveStart = res;
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status")
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      if (command === "start_cliproxyapi") return deferred.promise;
      return Promise.resolve("");
    });

    render(<ProxyPage config={configFixture} onConfig={vi.fn()} />);
    const startBtn = await screen.findByRole("button", { name: "启动" });
    fireEvent.click(startBtn);

    const pending = await screen.findByRole("button", { name: "正在启动" });
    expect(pending).toHaveAttribute("aria-busy", "true");
    const stopBtn = screen.getByRole("button", { name: "停止" });
    const refreshBtn = screen.getByRole("button", { name: "刷新" });
    expect(stopBtn).toBeDisabled();
    expect(refreshBtn).toBeDisabled();

    // extra clicks must not duplicate start / stop / refresh
    const statusBefore = invokeMock.mock.calls.filter(
      ([c]) => c === "cliproxyapi_status"
    ).length; // mount already refreshed once
    fireEvent.click(pending);
    fireEvent.click(stopBtn);
    fireEvent.click(refreshBtn);
    const startCalls = invokeMock.mock.calls.filter(([c]) => c === "start_cliproxyapi");
    const stopCalls = invokeMock.mock.calls.filter(([c]) => c === "stop_cliproxyapi");
    const statusCalls = invokeMock.mock.calls.filter(([c]) => c === "cliproxyapi_status");
    expect(startCalls).toHaveLength(1);
    expect(stopCalls).toHaveLength(0);
    expect(statusCalls.length).toBe(statusBefore); // no extra refresh while busy

    await act(async () => {
      resolveStart("started");
    });
  });

  it("restores controls and shows an alert when start fails", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status")
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      if (command === "start_cliproxyapi") return Promise.reject(new Error("port busy"));
      return Promise.resolve("");
    });

    render(<ProxyPage config={configFixture} onConfig={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "启动" }));

    const alert = await screen.findByRole("alert", { name: "无法启动 CLIProxyAPI" });
    expect(alert).toHaveTextContent("port busy");
    expect(screen.getByRole("button", { name: "启动" })).toBeEnabled();
  });

  it("clears the directory by sending an empty string", async () => {
    const cfg = { ...configFixture, cliproxyapi_dir: "C:/cli" };
    render(<ProxyPage config={cfg} onConfig={vi.fn()} />);
    const clearBtn = await screen.findByRole("button", { name: "清空" });
    fireEvent.click(clearBtn);

    await waitFor(() => {
      const calls = invokeMock.mock.calls.filter(([c]) => c === "set_cli_dir");
      expect(calls).toEqual([["set_cli_dir", { dir: "" }]]);
    });
  });

  it("treats the empty select_cli_dir sentinel as a silent no-op", async () => {
    const onConfig = vi.fn();
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status")
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      if (command === "select_cli_dir") return Promise.resolve("");
      return Promise.resolve("");
    });
    render(<ProxyPage config={configFixture} onConfig={onConfig} />);

    const pickBtn = screen.getByRole("button", { name: /选择.*目录/ });
    fireEvent.click(pickBtn);

    await waitFor(() => {
      expect(onConfig).not.toHaveBeenCalled();
    });
    expect(invokeMock.mock.calls.filter(([c]) => c === "set_cli_dir")).toHaveLength(0);
  });

  it("renders a recoverable alert when select_cli_dir rejects", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "cliproxyapi_status")
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      if (command === "select_cli_dir")
        return Promise.reject(new Error("permission denied"));
      return Promise.resolve("");
    });
    render(<ProxyPage config={configFixture} onConfig={vi.fn()} />);

    const pickBtn = screen.getByRole("button", { name: /选择.*目录/ });
    fireEvent.click(pickBtn);

    const alert = await screen.findByRole("alert", {
      name: "无法选择 CLIProxyAPI 目录",
    });
    expect(alert).toHaveTextContent("permission denied");
    expect(pickBtn).toBeEnabled();
  });
});
