import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { StatusBanner, type StatusMessage } from "../components/ui/StatusBanner";
import type { Config, UsageRange, UsageStatsSnapshot } from "../types";
import { ModelStatsTable } from "./dashboard/ModelStatsTable";
import { UsageTrendChart } from "./dashboard/UsageTrendChart";
import {
  emptyUsageStatsSnapshot,
  formatTokenCount,
  successRateLabel,
} from "./dashboard/usageStats";

const rangeOptions: Array<{ value: UsageRange; label: string }> = [
  { value: "live", label: "Realtime" },
  { value: "7d", label: "7D" },
  { value: "30d", label: "30D" },
  { value: "all", label: "History" },
];

const usageRanges = new Set<UsageRange>(["live", "7d", "30d", "all"]);

interface DashboardPageProps {
  config: Config | null;
  getUsageStats?: (range: UsageRange) => Promise<unknown>;
}

export function DashboardPage({
  config,
  getUsageStats = defaultGetUsageStats,
}: DashboardPageProps) {
  const profileCount = config?.profiles?.length ?? 0;
  const defaultProfile = profileCount > 0 ? config!.profiles[0].name : "None";
  const [range, setRange] = useState<UsageRange>("7d");
  const [snapshot, setSnapshot] = useState<UsageStatsSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestGenerationRef = useRef(0);
  const isMountedRef = useRef(true);

  useEffect(() => {
    return () => {
      isMountedRef.current = false;
      requestGenerationRef.current += 1;
    };
  }, []);

  const loadStats = useCallback(
    async (nextRange: UsageRange = range) => {
      const requestGeneration = ++requestGenerationRef.current;
      setRefreshing(true);

      function isCurrentRequest() {
        return isMountedRef.current && requestGeneration === requestGenerationRef.current;
      }

      try {
        const next = parseUsageStatsSnapshot(
          await getUsageStats(nextRange)
        );

        if (next.range !== nextRange) {
          throw new Error("Malformed usage stats response");
        }

        if (!isCurrentRequest()) {
          return;
        }

        setSnapshot(next);
        setError(null);
      } catch (cause) {
        if (!isCurrentRequest()) {
          return;
        }

        setSnapshot(null);
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        if (!isCurrentRequest()) {
          return;
        }

        setLoading(false);
        setRefreshing(false);
      }
    },
    [getUsageStats, range]
  );

  useEffect(() => {
    void loadStats(range);
    const timer = window.setInterval(() => {
      void loadStats(range);
    }, 5_000);

    return () => window.clearInterval(timer);
  }, [loadStats, range]);

  const displaySnapshot = snapshot ?? emptyUsageStatsSnapshot(range);
  const totals = displaySnapshot.totals;
  const healthLabel = error
    ? "Error"
    : loading
      ? "Loading..."
      : refreshing
        ? "Refreshing..."
        : "Up to date";
  const healthTone = error
    ? "usage-dashboard__health-pill--error"
    : loading || refreshing
      ? "usage-dashboard__health-pill--busy"
      : "usage-dashboard__health-pill--ok";

  const bannerMessages = useMemo(() => {
    const messages: StatusMessage[] = [];

    if (error) {
      messages.push({
        kind: "error",
        title: "Usage stats unavailable",
        detail: error,
      });
    }

    if (snapshot && !snapshot.history_writable) {
      messages.push({
        kind: "warning",
        title: "History persistence is read-only",
        detail:
          "Live usage still updates, but historical aggregation cannot be written right now.",
      });
    }

    if (snapshot?.history_recovered) {
      messages.push({
        kind: "info",
        title: "Recovered historical data",
        detail:
          "Dashboard loaded previously persisted usage statistics alongside the current process snapshot.",
      });
    }

    return messages;
  }, [error, snapshot]);

  return (
    <div className="page usage-dashboard">
      <div className="usage-dashboard__header">
        <div>
          <h2 className="page-title">仪表盘</h2>
          <p className="page-desc">
            Professional model usage monitoring with live polling and persisted
            history.
          </p>
        </div>
        <div className={`usage-dashboard__health-pill ${healthTone}`}>
          {healthLabel}
        </div>
      </div>

      <div className="usage-dashboard__banners">
        {bannerMessages.map((message, index) => (
          <StatusBanner key={`${message.kind}-${index}`} message={message} />
        ))}
      </div>

      <section className="usage-dashboard__section">
        <div
          className="usage-dashboard__toolbar"
          role="tablist"
          aria-label="Usage ranges"
        >
          {rangeOptions.map((option) => (
            <button
              key={option.value}
              type="button"
              role="tab"
              aria-selected={range === option.value}
              className={`usage-dashboard__range-tab ${
                range === option.value
                  ? "usage-dashboard__range-tab--active"
                  : ""
              }`}
              onClick={() => setRange(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>

        <div className="grid usage-metrics">
          <div className="card stat-card">
            <div className="card-label">Total Tokens</div>
            <div className="card-value">{formatTokenCount(totals.total_tokens)}</div>
            <p className="usage-metrics__meta">
              {formatTokenCount(totals.input_tokens)} in |{" "}
              {formatTokenCount(totals.output_tokens)} out
            </p>
          </div>

          <div className="card stat-card">
            <div className="card-label">Requests</div>
            <div className="card-value">{totals.requests}</div>
            <p className="usage-metrics__meta">
              {totals.failed_requests} failed logical requests
            </p>
          </div>

          <div className="card stat-card">
            <div className="card-label">Success Rate</div>
            <div className="card-value">{successRateLabel(displaySnapshot)}</div>
            <p className="usage-metrics__meta">
              {totals.usage_missing_requests} requests missing usage
            </p>
          </div>

          <div className="card stat-card">
            <div className="card-label">Retries</div>
            <div className="card-value">{totals.retry_count}</div>
            <p className="usage-metrics__meta">
              Extra upstream attempts beyond the first try
            </p>
          </div>
        </div>
      </section>

      <div className="usage-dashboard__main-grid">
        <UsageTrendChart points={displaySnapshot.trend} range={range} />

        <section className="card usage-provider-card">
          <div className="usage-card-header">
            <div>
              <h3 className="usage-section-title">Provider summary</h3>
              <p className="usage-section-subtitle">
                Requests, token load, and retry pressure by provider
              </p>
            </div>
          </div>

          <div className="usage-provider-list">
            {displaySnapshot.providers.length > 0 ? (
              displaySnapshot.providers.map((provider) => (
                <article
                  key={provider.provider}
                  className="usage-provider-list__item"
                >
                  <div>
                    <h4>{provider.provider}</h4>
                    <p>{provider.requests} requests</p>
                  </div>
                  <dl>
                    <div>
                      <dt>Tokens</dt>
                      <dd>{formatTokenCount(provider.total_tokens)}</dd>
                    </div>
                    <div>
                      <dt>Failures</dt>
                      <dd>{provider.failed_requests}</dd>
                    </div>
                    <div>
                      <dt>Retries</dt>
                      <dd>{provider.retry_count}</dd>
                    </div>
                  </dl>
                </article>
              ))
            ) : (
              <p className="usage-provider-list__empty">
                No provider usage recorded yet.
              </p>
            )}
          </div>
        </section>
      </div>

      <ModelStatsTable rows={displaySnapshot.models} />

      <section className="usage-dashboard__section">
        <div className="usage-card-header">
          <div>
            <h3 className="usage-section-title">Provider configuration</h3>
            <p className="usage-section-subtitle">
              Existing local configuration remains visible alongside the new
              usage dashboard.
            </p>
          </div>
        </div>

        <div className="grid usage-dashboard__config-grid">
          <div className="card stat-card">
            <div className="card-label">Provider profiles</div>
            <div className="card-value card-value--wrap">
              {profileCount} profiles | default {defaultProfile}
            </div>
          </div>

          <div className="card stat-card">
            <div className="card-label">Working directory</div>
            <div className="card-value card-value--wrap">
              {config?.work_dir || "Not selected"}
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

function parseUsageStatsSnapshot(value: unknown): UsageStatsSnapshot {
  if (!isUsageStatsSnapshot(value)) {
    throw new Error("Malformed usage stats response");
  }

  return value;
}

function isUsageStatsSnapshot(value: unknown): value is UsageStatsSnapshot {
  if (!isRecord(value)) {
    return false;
  }

  return (
    isUsageRange(value.range) &&
    typeof value.generated_at === "string" &&
    isUsageAggregate(value.totals) &&
    isUsageTrendPointArray(value.trend) &&
    isUsageModelAggregateArray(value.models) &&
    isUsageProviderAggregateArray(value.providers) &&
    typeof value.history_recovered === "boolean" &&
    typeof value.history_writable === "boolean"
  );
}

function isUsageRange(value: unknown): value is UsageRange {
  return typeof value === "string" && usageRanges.has(value as UsageRange);
}

function isUsageAggregate(value: unknown): boolean {
  return (
    isRecord(value) &&
    isNumber(value.requests) &&
    isNumber(value.input_tokens) &&
    isNumber(value.output_tokens) &&
    isNumber(value.total_tokens) &&
    isNumber(value.failed_requests) &&
    isNumber(value.retry_count) &&
    isNumber(value.success_rate) &&
    isNumber(value.usage_missing_requests)
  );
}

function isUsageTrendPointArray(value: unknown): boolean {
  return (
    Array.isArray(value) &&
    value.every(
      (point) =>
        isRecord(point) &&
        typeof point.label === "string" &&
        isNumber(point.requests) &&
        isNumber(point.input_tokens) &&
        isNumber(point.output_tokens) &&
        isNumber(point.total_tokens) &&
        isNumber(point.failed_requests) &&
        isNumber(point.retry_count)
    )
  );
}

function isUsageModelAggregateArray(value: unknown): boolean {
  return (
    Array.isArray(value) &&
    value.every(
      (row) =>
        isRecord(row) &&
        typeof row.provider === "string" &&
        typeof row.model === "string" &&
        isUsageAggregate(row)
    )
  );
}

function isUsageProviderAggregateArray(value: unknown): boolean {
  return (
    Array.isArray(value) &&
    value.every(
      (row) =>
        isRecord(row) &&
        typeof row.provider === "string" &&
        isNumber(row.requests) &&
        isNumber(row.total_tokens) &&
        isNumber(row.failed_requests) &&
        isNumber(row.retry_count)
    )
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

async function defaultGetUsageStats(range: UsageRange): Promise<unknown> {
  return invoke<unknown>("get_usage_stats", { range });
}
