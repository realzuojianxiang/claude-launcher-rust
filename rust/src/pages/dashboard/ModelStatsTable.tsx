import { useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { UsageModelAggregate } from "../../types";
import {
  type ModelStatsSortKey,
  type SortDirection,
  formatTokenCount,
  sortModelStats,
} from "./usageStats";

const columns: Array<{
  key: ModelStatsSortKey;
  label: string;
  buttonLabel: string;
  render: (row: UsageModelAggregate) => ReactNode;
}> = [
  {
    key: "model",
    label: "Model",
    buttonLabel: "Sort by model",
    render: (row) => (
      <div className="usage-table__model">
        <span className="usage-table__model-name">{row.model}</span>
        <span className="usage-table__provider">Provider: {row.provider}</span>
      </div>
    ),
  },
  {
    key: "requests",
    label: "Requests",
    buttonLabel: "Sort by requests",
    render: (row) => row.requests,
  },
  {
    key: "total_tokens",
    label: "Tokens",
    buttonLabel: "Sort by tokens",
    render: (row) => formatTokenCount(row.total_tokens),
  },
  {
    key: "failed_requests",
    label: "Failures",
    buttonLabel: "Sort by failures",
    render: (row) => row.failed_requests,
  },
  {
    key: "retry_count",
    label: "Retries",
    buttonLabel: "Sort by retries",
    render: (row) => row.retry_count,
  },
  {
    key: "success_rate",
    label: "Success",
    buttonLabel: "Sort by success rate",
    render: (row) =>
      `${(row.success_rate * 100).toFixed(1).replace(/\.0$/, "")}%`,
  },
  {
    key: "usage_missing_requests",
    label: "Usage gaps",
    buttonLabel: "Sort by usage gaps",
    render: (row) =>
      row.usage_missing_requests > 0 ? (
        <span className="usage-table__hint">Usage incomplete</span>
      ) : (
        row.usage_missing_requests
      ),
  },
];

export function ModelStatsTable({ rows }: { rows: UsageModelAggregate[] }) {
  const [sortKey, setSortKey] = useState<ModelStatsSortKey>("total_tokens");
  const [direction, setDirection] = useState<SortDirection>("desc");

  const sortedRows = useMemo(
    () => sortModelStats(rows, sortKey, direction),
    [direction, rows, sortKey]
  );

  function toggleSort(nextKey: ModelStatsSortKey) {
    setDirection((currentDirection) =>
      nextKey === sortKey
        ? currentDirection === "desc"
          ? "asc"
          : "desc"
        : "desc"
    );
    setSortKey(nextKey);
  }

  return (
    <section className="card usage-table-card">
      <div className="usage-table__scroller">
        <table className="usage-table">
          <caption className="usage-section-title usage-table__caption">
            Model breakdown
          </caption>
          <thead>
            <tr>
              {columns.map((column) => {
                const isActive = column.key === sortKey;
                const ariaSort =
                  isActive && direction === "asc"
                    ? "ascending"
                    : isActive && direction === "desc"
                      ? "descending"
                      : "none";

                return (
                  <th key={column.key} scope="col" aria-sort={ariaSort}>
                    <button
                      type="button"
                      className={`usage-table__sort ${
                        isActive ? "usage-table__sort--active" : ""
                      }`}
                      aria-label={column.buttonLabel}
                      onClick={() => toggleSort(column.key)}
                    >
                      {column.label}
                    </button>
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {sortedRows.length > 0 ? (
              sortedRows.map((row) => (
                <tr key={`${row.provider}-${row.model}`}>
                  {columns.map((column) => (
                    <td key={column.key}>{column.render(row)}</td>
                  ))}
                </tr>
              ))
            ) : (
              <tr>
                <td colSpan={columns.length} className="usage-table__empty">
                  No model data yet.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
