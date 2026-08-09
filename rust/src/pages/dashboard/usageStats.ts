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
    return formatCompact(value, 1);
  }

  if (absoluteValue < 1_000_000_000) {
    return formatCompact(value, 2);
  }

  return formatCompact(value, 3);
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

const COMPACT_UNITS = [
  { divisor: 1, suffix: "" },
  { divisor: 1_000, suffix: "K" },
  { divisor: 1_000_000, suffix: "M" },
  { divisor: 1_000_000_000, suffix: "B" },
] as const;

function formatCompact(value: number, unitIndex: number): string {
  const unit = COMPACT_UNITS[unitIndex];
  const scaledValue = value / unit.divisor;

  if (unit.suffix === "") {
    return String(value);
  }

  const roundedValue = Number(scaledValue.toFixed(2));
  if (Math.abs(roundedValue) >= 1000 && unitIndex < COMPACT_UNITS.length - 1) {
    return formatCompact(value, unitIndex + 1);
  }

  return `${trimTrailingZeros(roundedValue.toFixed(2))}${unit.suffix}`;
}

function trimTrailingZeros(value: string): string {
  return value.replace(/\.0+$|(\.\d*?)0+$/, "$1");
}
