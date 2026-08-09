import { describe, expect, it } from "vitest";
import type { UsageModelAggregate, UsageStatsSnapshot } from "../../types";
import {
  emptyUsageStatsSnapshot,
  formatTokenCount,
  getUsageTotal,
  sortModelStats,
  successRateLabel,
} from "./usageStats";

const snapshotFixture: UsageStatsSnapshot = {
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
      label: "today",
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
    {
      provider: "nvidia",
      model: "nemotron",
      requests: 116,
      input_tokens: 380_000,
      output_tokens: 154_000,
      total_tokens: 534_000,
      failed_requests: 6,
      retry_count: 9,
      usage_missing_requests: 1,
      success_rate: 0.948,
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
    {
      provider: "nvidia",
      requests: 116,
      total_tokens: 534_000,
      failed_requests: 6,
      retry_count: 9,
    },
  ],
  history_recovered: false,
  history_writable: true,
};

describe("usageStats helpers", () => {
  it("formats token counts compactly without hiding small values", () => {
    expect(formatTokenCount(0)).toBe("0");
    expect(formatTokenCount(999)).toBe("999");
    expect(formatTokenCount(1_280_000)).toBe("1.28M");
  });

  it("normalizes rounded values that cross unit boundaries", () => {
    expect(formatTokenCount(999_999)).toBe("1M");
    expect(formatTokenCount(999_999_999)).toBe("1B");
  });

  it("normalizes signed rounded values that cross unit boundaries", () => {
    expect(formatTokenCount(-999_999)).toBe("-1M");
    expect(formatTokenCount(-999_999_999)).toBe("-1B");
  });

  it("uses the backend total when rendering the KPI", () => {
    expect(getUsageTotal(snapshotFixture)).toBe(1_280_000);
    expect(successRateLabel(snapshotFixture)).toBe("96.8%");
  });

  it("creates an empty snapshot fallback with stable defaults", () => {
    expect(emptyUsageStatsSnapshot("live")).toEqual({
      range: "live",
      generated_at: "",
      totals: {
        requests: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        failed_requests: 0,
        retry_count: 0,
        success_rate: 1,
        usage_missing_requests: 0,
      },
      trend: [],
      models: [],
      providers: [],
      history_recovered: false,
      history_writable: true,
    });
  });

  it("sorts model rows deterministically with provider/model tie breaks", () => {
    const rows: UsageModelAggregate[] = [
      {
        provider: "zeta",
        model: "same",
        requests: 1,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 10,
        failed_requests: 0,
        retry_count: 0,
        usage_missing_requests: 0,
        success_rate: 1,
      },
      {
        provider: "alpha",
        model: "same",
        requests: 1,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 10,
        failed_requests: 0,
        retry_count: 0,
        usage_missing_requests: 0,
        success_rate: 1,
      },
      {
        provider: "alpha",
        model: "beta",
        requests: 1,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 10,
        failed_requests: 0,
        retry_count: 0,
        usage_missing_requests: 0,
        success_rate: 1,
      },
    ];

    expect(
      sortModelStats(rows, "total_tokens", "desc").map(
        ({ provider, model }) => `${provider}/${model}`
      )
    ).toEqual(["alpha/beta", "alpha/same", "zeta/same"]);
  });
});
