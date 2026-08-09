import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { UsageTrendPoint } from "../../types";
import { UsageTrendChart } from "./UsageTrendChart";

const points: UsageTrendPoint[] = [
  {
    label: "Mon",
    requests: 18,
    input_tokens: 120_000,
    output_tokens: 40_000,
    total_tokens: 160_000,
    failed_requests: 1,
    retry_count: 3,
  },
  {
    label: "Tue",
    requests: 22,
    input_tokens: 150_000,
    output_tokens: 60_000,
    total_tokens: 210_000,
    failed_requests: 0,
    retry_count: 1,
  },
];

describe("UsageTrendChart", () => {
  it("renders the chart title, legend, summary, and point labels", () => {
    render(<UsageTrendChart points={points} range="7d" />);

    expect(screen.getByText("Usage Trend")).toBeInTheDocument();
    expect(screen.getByText("Input tokens")).toBeInTheDocument();
    expect(screen.getByText("Output tokens")).toBeInTheDocument();
    expect(screen.getAllByText("Mon")).toHaveLength(2);
    expect(
      screen.getByLabelText("Usage trend for 7d. 2 points. 370000 total tokens.")
    ).toBeInTheDocument();
    expect(
      screen.getByText("2 points | 370K total tokens | 40 requests")
    ).toBeInTheDocument();
  });

  it("renders an accessible empty state when all points are zero", () => {
    render(
      <UsageTrendChart
        points={[
          {
            label: "Today",
            requests: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            failed_requests: 0,
            retry_count: 0,
          },
        ]}
        range="live"
      />
    );

    expect(screen.getByText("No usage yet for this range.")).toBeInTheDocument();
  });
});
