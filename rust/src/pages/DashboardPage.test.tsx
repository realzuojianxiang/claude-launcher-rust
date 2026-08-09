import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UsageStatsSnapshot } from "../types";
import { configFixture } from "../test/fixtures";
import { DashboardPage } from "./DashboardPage";

const { getUsageStatsMock } = vi.hoisted(() => ({
  getUsageStatsMock: vi.fn(),
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

const historySnapshotFixture: UsageStatsSnapshot = {
  range: "30d",
  generated_at: "2026-08-09T12:05:00+08:00",
  totals: {
    requests: 512,
    input_tokens: 1_800_000,
    output_tokens: 900_000,
    total_tokens: 2_700_000,
    failed_requests: 9,
    retry_count: 21,
    success_rate: 0.982,
    usage_missing_requests: 0,
  },
  trend: [
    {
      label: "This week",
      requests: 84,
      input_tokens: 300_000,
      output_tokens: 160_000,
      total_tokens: 460_000,
      failed_requests: 1,
      retry_count: 4,
    },
  ],
  models: [
    {
      provider: "nvidia",
      model: "nemotron-ultra",
      requests: 240,
      input_tokens: 900_000,
      output_tokens: 430_000,
      total_tokens: 1_330_000,
      failed_requests: 3,
      retry_count: 11,
      usage_missing_requests: 0,
      success_rate: 0.988,
    },
  ],
  providers: [
    {
      provider: "nvidia",
      requests: 240,
      total_tokens: 1_330_000,
      failed_requests: 3,
      retry_count: 11,
    },
  ],
  history_recovered: true,
  history_writable: true,
};

describe("DashboardPage", () => {
  beforeEach(() => {
    vi.useRealTimers();
    getUsageStatsMock.mockReset();
    getUsageStatsMock.mockResolvedValue(usageSnapshotFixture);
  });

  it("loads and renders usage metrics beside the existing config summary", async () => {
    render(<DashboardPage config={configFixture} getUsageStats={getUsageStatsMock} />);

    expect(await screen.findByText("1.28M")).toBeInTheDocument();
    expect(screen.getByText("Total Tokens")).toBeInTheDocument();
    expect(screen.getByText("grok-4.5")).toBeInTheDocument();
    expect(screen.getByText("D:\\work")).toBeInTheDocument();
    expect(getUsageStatsMock).toHaveBeenCalledWith("7d");
  });

  it("polls every five seconds and shows the refreshing state", async () => {
    vi.useFakeTimers();
    const secondRequest = deferred<UsageStatsSnapshot>();
    getUsageStatsMock
      .mockResolvedValueOnce(usageSnapshotFixture)
      .mockImplementationOnce(() => secondRequest.promise);

    render(<DashboardPage config={configFixture} getUsageStats={getUsageStatsMock} />);

    await act(async () => {
      await Promise.resolve();
    });

    act(() => {
      vi.advanceTimersByTime(5_000);
    });

    expect(getUsageStatsMock).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Refreshing...")).toBeInTheDocument();

    secondRequest.resolve(usageSnapshotFixture);
    await act(async () => {
      await secondRequest.promise;
    });
    vi.useRealTimers();
  });

  it("shows a non-blocking error banner while keeping the provider summary cards", async () => {
    getUsageStatsMock.mockRejectedValueOnce(new Error("backend unavailable"));

    render(<DashboardPage config={null} getUsageStats={getUsageStatsMock} />);

    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("Usage stats unavailable")).toBeInTheDocument();
    expect(within(alert).getByText("backend unavailable")).toBeInTheDocument();
    expect(screen.getByText("0 profiles | default None")).toBeInTheDocument();
    expect(screen.getByText("Not selected")).toBeInTheDocument();
  });

  it("treats malformed usage stats payloads as a handled error and keeps the empty fallback renderable", async () => {
    getUsageStatsMock.mockResolvedValueOnce("");

    render(<DashboardPage config={null} getUsageStats={getUsageStatsMock} />);

    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("Usage stats unavailable")).toBeInTheDocument();
    expect(within(alert).getByText("Malformed usage stats response")).toBeInTheDocument();
    expect(screen.getByText("Total Tokens")).toBeInTheDocument();
    expect(screen.getByText("100%")).toBeInTheDocument();
    expect(screen.getByText("No provider usage recorded yet.")).toBeInTheDocument();
  });

  it("ignores stale range responses that resolve after a newer selection", async () => {
    const firstRequest = deferred<UsageStatsSnapshot>();
    const secondRequest = deferred<UsageStatsSnapshot>();
    getUsageStatsMock.mockImplementation((range: string) => {
      if (range === "7d") {
        return firstRequest.promise;
      }

      if (range === "30d") {
        return secondRequest.promise;
      }

      return Promise.reject(new Error(`unexpected range ${range ?? "unknown"}`));
    });

    render(<DashboardPage config={configFixture} getUsageStats={getUsageStatsMock} />);

    fireEvent.click(screen.getByRole("tab", { name: "30D" }));

    expect(getUsageStatsMock).toHaveBeenNthCalledWith(1, "7d");
    expect(getUsageStatsMock).toHaveBeenNthCalledWith(2, "30d");

    secondRequest.resolve(historySnapshotFixture);
    await act(async () => {
      await secondRequest.promise;
    });

    expect(await screen.findByText("nemotron-ultra")).toBeInTheDocument();
    expect(screen.getByText("2.7M")).toBeInTheDocument();

    firstRequest.resolve(usageSnapshotFixture);
    await act(async () => {
      await firstRequest.promise;
    });

    expect(screen.getByText("nemotron-ultra")).toBeInTheDocument();
    expect(screen.queryByText("grok-4.5")).not.toBeInTheDocument();
    expect(screen.getByText("2.7M")).toBeInTheDocument();
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}
