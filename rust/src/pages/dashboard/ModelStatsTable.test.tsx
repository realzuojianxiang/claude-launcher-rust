import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { UsageModelAggregate } from "../../types";
import { ModelStatsTable } from "./ModelStatsTable";

const rows: UsageModelAggregate[] = [
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
    requests: 240,
    input_tokens: 380_000,
    output_tokens: 154_000,
    total_tokens: 534_000,
    failed_requests: 6,
    retry_count: 9,
    usage_missing_requests: 1,
    success_rate: 0.948,
  },
];

describe("ModelStatsTable", () => {
  it("renders provider/model metrics and missing-usage hints", () => {
    render(<ModelStatsTable rows={rows} />);

    expect(screen.getByText("Model breakdown")).toBeInTheDocument();
    expect(screen.getByText("grok-4.5")).toBeInTheDocument();
    expect(screen.getByText("Provider: grok")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sort by requests" })).toBeInTheDocument();
    expect(screen.getByText("240")).toBeInTheDocument();
    expect(screen.getByText("746K")).toBeInTheDocument();
    expect(screen.getByText("9")).toBeInTheDocument();
    expect(screen.getByText("Usage incomplete")).toBeInTheDocument();
  });

  it("sorts rows when the requests header button is clicked", () => {
    render(<ModelStatsTable rows={rows} />);

    const dataRows = screen.getAllByRole("row").slice(1);
    expect(within(dataRows[0]).getByText("grok-4.5")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Sort by requests" }));

    const sortedRows = screen.getAllByRole("row").slice(1);
    expect(within(sortedRows[0]).getByText("nemotron")).toBeInTheDocument();
  });
});
