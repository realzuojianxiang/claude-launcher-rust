import type {
  UsageAggregate,
  UsageModelAggregate,
  UsageRange,
  UsageStatsSnapshot,
} from "../../types";

export type ModelStatsSortKey =
  | "provider"
  | "model"
  | "requests"
  | "input_tokens"
  | "output_tokens"
  | "total_tokens"
  | "failed_requests"
  | "retry_count"
  | "success_rate"
  | "usage_missing_requests";

export type SortDirection = "asc" | "desc";

const EMPTY_USAGE_AGGREGATE: UsageAggregate = {
  requests: 0,
  input_tokens: 0,
  output_tokens: 0,
  total_tokens: 0,
  failed_requests: 0,
  retry_count: 0,
  success_rate: 1,
  usage_missing_requests: 0,
};

export function formatTokenCount(value: number): string {
  const absoluteValue = Math.abs(value);

  if (absoluteValue < 1_000) {
    return String(value);
  }

  if (absoluteValue < 1_000_000) {
    return formatCompact(value, 1_000, "K");
  }

  if (absoluteValue < 1_000_000_000) {
    return formatCompact(value, 1_000_000, "M");
  }

  return formatCompact(value, 1_000_000_000, "B");
}

export function getUsageTotal(snapshot: UsageStatsSnapshot): number {
  return snapshot.totals.total_tokens;
}

export function successRateLabel(snapshot: UsageStatsSnapshot): string {
  return `${trimTrailingZeros((snapshot.totals.success_rate * 100).toFixed(1))}%`;
}

export function emptyUsageStatsSnapshot(range: UsageRange): UsageStatsSnapshot {
  return {
    range,
    generated_at: "",
    totals: { ...EMPTY_USAGE_AGGREGATE },
    trend: [],
    models: [],
    providers: [],
    history_recovered: false,
    history_writable: true,
  };
}

export function sortModelStats(
  rows: UsageModelAggregate[],
  sortKey: ModelStatsSortKey,
  direction: SortDirection
): UsageModelAggregate[] {
  const multiplier = direction === "asc" ? 1 : -1;

  return [...rows].sort((left, right) => {
    const leftValue = left[sortKey];
    const rightValue = right[sortKey];

    if (leftValue < rightValue) {
      return -1 * multiplier;
    }

    if (leftValue > rightValue) {
      return 1 * multiplier;
    }

    const providerComparison = left.provider.localeCompare(right.provider);
    if (providerComparison !== 0) {
      return providerComparison;
    }

    return left.model.localeCompare(right.model);
  });
}

function formatCompact(value: number, divisor: number, suffix: "K" | "M" | "B"): string {
  return `${trimTrailingZeros((value / divisor).toFixed(2))}${suffix}`;
}

function trimTrailingZeros(value: string): string {
  return value.replace(/\.0+$|(\.\d*?)0+$/, "$1");
}
