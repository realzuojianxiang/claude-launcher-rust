import { act, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UsageStatsSnapshot } from "../types";
import { configFixture } from "../test/fixtures";
import { DashboardPage } from "./DashboardPage";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const usageSnapshotFixture: UsageStatsSnapshot = {
  range: "7d",
  generated_at: "2026-08-09T12:00:00+08:00",
  totals: {
    requests: 248,
    input_tokens: 900_000,
    output_tokens: 380_000,
    total_tokens: 1_280_000,
    failed_requests: 8,
    retry_count: 17,
    success_rate: 0.968,
    usage_missing_requests: 1,
  },
  trend: [
    {
      label: "Today",
      requests: 32,
      input_tokens: 120_000,
      output_tokens: 50_000,
      total_tokens: 170_000,
      failed_requests: 1,
      retry_count: 2,
    },
  ],
  models: [
    {
      provider: "grok",
      model: "grok-4.5",
      requests: 132,
      input_tokens: 520_000,
      output_tokens: 226_000,
      total_tokens: 746_000,
      failed_requests: 2,
      retry_count: 8,
      usage_missing_requests: 0,
      success_rate: 0.985,
    },
  ],
  providers: [
    {
      provider: "grok",
      requests: 132,
      total_tokens: 746_000,
      failed_requests: 2,
      retry_count: 8,
    },
  ],
  history_recovered: false,
  history_writable: true,
};

describe("DashboardPage", () => {
  beforeEach(() => {
    vi.useRealTimers();
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(usageSnapshotFixture);
  });

  it("loads and renders usage metrics beside the existing config summary", async () => {
    render(<DashboardPage config={configFixture} />);

    expect(await screen.findByText("Total Tokens")).toBeInTheDocument();
    expect(screen.getByText("1.28M")).toBeInTheDocument();
    expect(screen.getByText("grok-4.5")).toBeInTheDocument();
    expect(screen.getByText("D:\\work")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("get_usage_stats", { range: "7d" });
  });

  it("polls every five seconds and shows the refreshing state", async () => {
    vi.useFakeTimers();
    const secondRequest = deferred<UsageStatsSnapshot>();
    invokeMock
      .mockResolvedValueOnce(usageSnapshotFixture)
      .mockImplementationOnce(() => secondRequest.promise);

    render(<DashboardPage config={configFixture} />);

    await act(async () => {
      await Promise.resolve();
    });

    act(() => {
      vi.advanceTimersByTime(5_000);
    });

    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Refreshing...")).toBeInTheDocument();

    secondRequest.resolve(usageSnapshotFixture);
    await act(async () => {
      await secondRequest.promise;
    });
    vi.useRealTimers();
  });

  it("shows a non-blocking error banner while keeping the provider summary cards", async () => {
    invokeMock.mockRejectedValueOnce(new Error("backend unavailable"));

    render(<DashboardPage config={null} />);

    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("Usage stats unavailable")).toBeInTheDocument();
    expect(within(alert).getByText("backend unavailable")).toBeInTheDocument();
    expect(screen.getByText("0 profiles | default None")).toBeInTheDocument();
    expect(screen.getByText("Not selected")).toBeInTheDocument();
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}
