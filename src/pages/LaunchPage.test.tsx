import { fireEvent, render, screen, waitFor, act } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LaunchPage } from "./LaunchPage";
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

function launchClaudeButton() {
  return screen.getByRole("button", { name: /启动 Claude Code/ });
}

describe("LaunchPage", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_recent_dirs") return Promise.resolve([]);
      if (command === "select_directory") return Promise.resolve("C:/picked");
      if (command === "launch_claude") return Promise.resolve("ok");
      return Promise.resolve("");
    });
  });

  it("locks the launch action while pending and prevents a second invocation", async () => {
    let resolveLaunch!: (v: unknown) => void;
    deferred.promise = new Promise((res) => {
      resolveLaunch = res;
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_recent_dirs") return Promise.resolve([]);
      if (command === "launch_claude") return deferred.promise;
      return Promise.resolve("");
    });

    render(<LaunchPage config={configFixture} onConfig={vi.fn()} />);
    const btn = launchClaudeButton();
    fireEvent.click(btn);

    const pending = await screen.findByRole("button", {
      name: "正在启动 Claude Code",
    });
    expect(pending).toBeDisabled();

    // a second click must not trigger another launch
    fireEvent.click(pending);
    const launchCalls = invokeMock.mock.calls.filter(([c]) => c === "launch_claude");
    expect(launchCalls).toHaveLength(1);

    await act(async () => {
      resolveLaunch("ok");
    });
  });

  it("renders a recoverable alert on launch failure and re-enables the action", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_recent_dirs") return Promise.resolve([]);
      if (command === "launch_claude") return Promise.reject(new Error("boom"));
      return Promise.resolve("");
    });

    render(<LaunchPage config={configFixture} onConfig={vi.fn()} />);
    fireEvent.click(launchClaudeButton());

    const alert = await screen.findByRole("alert", { name: "无法启动 Claude Code" });
    expect(alert).toHaveTextContent("boom");
    expect(launchClaudeButton()).toBeEnabled();
  });

  it("uses a recent directory via set_work_dir + add_recent_dir, and delete does not select it", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_recent_dirs") return Promise.resolve(["C:/proj-a"]);
      if (command === "select_directory") return Promise.resolve("C:/picked");
      if (command === "launch_claude") return Promise.resolve("ok");
      return Promise.resolve("");
    });

    render(<LaunchPage config={configFixture} onConfig={vi.fn()} />);
    const useBtn = await screen.findByRole("button", { name: "C:/proj-a" });
    fireEvent.click(useBtn);

    await waitFor(() => {
      const calls = invokeMock.mock.calls.filter(([c]) => c === "set_work_dir");
      expect(calls).toHaveLength(1);
      expect(calls[0]).toEqual(["set_work_dir", { dir: "C:/proj-a" }]);
    });
    expect(invokeMock.mock.calls.filter(([c]) => c === "add_recent_dir")).toEqual([
      ["add_recent_dir", { dir: "C:/proj-a" }],
    ]);

    const setWorkDirBefore = invokeMock.mock.calls.filter(
      ([c]) => c === "set_work_dir"
    ).length;
    const deleteBtn = screen.getByRole("button", { name: "删除该历史目录" });
    fireEvent.click(deleteBtn); // arm
    fireEvent.click(deleteBtn); // confirm
    await waitFor(() => {
      const setWorkDirAfter = invokeMock.mock.calls.filter(
        ([c]) => c === "set_work_dir"
      ).length;
      expect(setWorkDirAfter).toBe(setWorkDirBefore);
    });
  });

  it("treats the empty select_directory sentinel as a silent no-op", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_recent_dirs") return Promise.resolve([]);
      if (command === "select_directory") return Promise.resolve(""); // cancellation sentinel
      return Promise.resolve("");
    });
    const onConfig = vi.fn();

    render(<LaunchPage config={configFixture} onConfig={onConfig} />);
    const callsBefore = invokeMock.mock.calls.filter(
      (c) => c[0] === "get_recent_dirs"
    ).length;

    const pickBtn = screen.getByRole("button", { name: /选择.*目录/ });
    fireEvent.click(pickBtn);

    await waitFor(() => {
      expect(onConfig).not.toHaveBeenCalled();
    });
    const callsAfter = invokeMock.mock.calls.filter(
      (c) => c[0] === "get_recent_dirs"
    ).length;
    expect(callsAfter).toBe(callsBefore);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders a recoverable alert and keeps the chooser enabled when select_directory rejects", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_recent_dirs") return Promise.resolve([]);
      if (command === "select_directory")
        return Promise.reject(new Error("permission denied"));
      return Promise.resolve("");
    });

    render(<LaunchPage config={configFixture} onConfig={vi.fn()} />);
    const pickBtn = screen.getByRole("button", { name: /选择.*目录/ });
    fireEvent.click(pickBtn);

    const alert = await screen.findByRole("alert", {
      name: "无法选择工作目录",
    });
    expect(alert).toHaveTextContent("permission denied");
    expect(pickBtn).toBeEnabled();
  });

  it("ignores a stale get_recent_dirs response when a newer refresh resolves first", async () => {
    let resolveStale!: (value: string[]) => void;
    let callCount = 0;
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_recent_dirs") {
        callCount += 1;
        if (callCount === 1) {
          return new Promise<string[]>((res) => {
            resolveStale = res;
          });
        }
        return Promise.resolve(["FRESH-1"]);
      }
      if (command === "launch_claude") return Promise.resolve("ok");
      return Promise.resolve("");
    });

    render(<LaunchPage config={configFixture} onConfig={vi.fn()} />);
    // initial load is pending; trigger a newer refresh via launch success
    fireEvent.click(launchClaudeButton());
    expect(await screen.findByText("FRESH-1")).toBeInTheDocument();

    await act(async () => {
      resolveStale(["STALE-1", "STALE-2"]);
    });
    expect(screen.queryByText("STALE-1")).not.toBeInTheDocument();
  });
});
