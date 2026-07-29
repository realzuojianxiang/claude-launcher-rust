import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LogPage } from "./LogPage";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

describe("LogPage", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_logs") {
        return Promise.resolve([
          "[20260728-120000] INFO first line",
          "[20260728-120001] ERROR second line",
        ]);
      }
      if (command === "get_log_level") return Promise.resolve("INFO");
      if (command === "list_log_files") return Promise.resolve([]);
      if (command === "clear_logs") return Promise.resolve(undefined);
      return Promise.resolve("");
    });
    listenMock.mockResolvedValue(vi.fn());
  });

  it("clears the backend live-log buffer and removes the displayed lines", async () => {
    render(<LogPage />);

    expect(await screen.findByText("[20260728-120000] INFO first line")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "清除实时日志" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("clear_logs");
      expect(screen.queryByText("[20260728-120000] INFO first line")).not.toBeInTheDocument();
      expect(screen.queryByText("[20260728-120001] ERROR second line")).not.toBeInTheDocument();
    });
  });
});
