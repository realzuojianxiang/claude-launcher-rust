import type { UsageRange, UsageTrendPoint } from "../../types";
import { formatTokenCount } from "./usageStats";

const CHART_HEIGHT = 220;
const CHART_WIDTH = 640;
const CHART_PADDING_TOP = 16;
const CHART_PADDING_BOTTOM = 40;
const CHART_BAR_WIDTH = 18;
const CHART_BAR_GAP = 10;

export function UsageTrendChart({
  points,
  range,
}: {
  points: UsageTrendPoint[];
  range: UsageRange;
}) {
  const totalTokens = points.reduce((sum, point) => sum + point.total_tokens, 0);
  const totalRequests = points.reduce((sum, point) => sum + point.requests, 0);
  const maxValue = Math.max(
    0,
    ...points.flatMap((point) => [point.input_tokens, point.output_tokens])
  );
  const hasUsage = maxValue > 0;
  const chartInnerHeight = CHART_HEIGHT - CHART_PADDING_TOP - CHART_PADDING_BOTTOM;
  const slotWidth = Math.max(64, CHART_WIDTH / Math.max(points.length, 1));
  const summaryLabel = `${points.length} points | ${formatTokenCount(totalTokens)} total tokens | ${totalRequests} requests`;
  const ariaLabel = `Usage trend for ${range}. ${points.length} points. ${totalTokens} total tokens.`;

  return (
    <section className="card usage-chart-card" aria-labelledby="usage-trend-title">
      <div className="usage-card-header">
        <div>
          <h3 id="usage-trend-title" className="usage-section-title">
            Usage Trend
          </h3>
          <p className="usage-section-subtitle">{summaryLabel}</p>
        </div>
        <ul className="usage-chart-legend" aria-label="Trend legend">
          <li>
            <span className="usage-chart-legend__swatch usage-chart-legend__swatch--input" />
            Input tokens
          </li>
          <li>
            <span className="usage-chart-legend__swatch usage-chart-legend__swatch--output" />
            Output tokens
          </li>
        </ul>
      </div>

      {hasUsage ? (
        <div className="usage-chart">
          <svg
            viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT}`}
            className="usage-chart__svg"
            role="img"
            aria-label={ariaLabel}
            preserveAspectRatio="none"
          >
            <line
              x1="0"
              x2={CHART_WIDTH}
              y1={CHART_HEIGHT - CHART_PADDING_BOTTOM}
              y2={CHART_HEIGHT - CHART_PADDING_BOTTOM}
              className="usage-chart__baseline"
            />
            {points.map((point, index) => {
              const x =
                index * slotWidth +
                slotWidth / 2 -
                CHART_BAR_WIDTH -
                CHART_BAR_GAP / 2;
              const inputHeight =
                maxValue === 0 ? 0 : (point.input_tokens / maxValue) * chartInnerHeight;
              const outputHeight =
                maxValue === 0 ? 0 : (point.output_tokens / maxValue) * chartInnerHeight;
              const labelX = index * slotWidth + slotWidth / 2;

              return (
                <g key={`${point.label}-${index}`}>
                  <rect
                    x={x}
                    y={CHART_HEIGHT - CHART_PADDING_BOTTOM - inputHeight}
                    width={CHART_BAR_WIDTH}
                    height={inputHeight}
                    rx="6"
                    className="usage-chart__bar usage-chart__bar--input"
                  />
                  <rect
                    x={x + CHART_BAR_WIDTH + CHART_BAR_GAP}
                    y={CHART_HEIGHT - CHART_PADDING_BOTTOM - outputHeight}
                    width={CHART_BAR_WIDTH}
                    height={outputHeight}
                    rx="6"
                    className="usage-chart__bar usage-chart__bar--output"
                  />
                  <text
                    x={labelX}
                    y={CHART_HEIGHT - 12}
                    textAnchor="middle"
                    className="usage-chart__label"
                  >
                    {point.label}
                  </text>
                </g>
              );
            })}
          </svg>

          <ol className="usage-chart__summary-list">
            {points.map((point) => (
              <li key={point.label}>
                <span>{point.label}</span>
                <span>{formatTokenCount(point.total_tokens)} total</span>
                <span>{point.requests} requests</span>
              </li>
            ))}
          </ol>
        </div>
      ) : (
        <div className="usage-chart usage-chart--empty" role="img" aria-label={ariaLabel}>
          <p className="usage-chart__empty-title">No usage yet for this range.</p>
          <p className="usage-chart__empty-detail">
            Polling continues every five seconds. Switch ranges to inspect
            history once data lands.
          </p>
        </div>
      )}
    </section>
  );
}
