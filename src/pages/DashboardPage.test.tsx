import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DashboardPage } from "./DashboardPage";
import { configFixture } from "../test/fixtures";

describe("DashboardPage", () => {
  it("shows the profile count and default profile name", () => {
    render(<DashboardPage config={configFixture} />);
    expect(screen.getByText("1 套（默认 讯飞）")).toBeInTheDocument();
    expect(screen.getByText("D:\\work")).toBeInTheDocument();
  });

  it("shows fallbacks when config is null", () => {
    render(<DashboardPage config={null} />);
    expect(screen.getByText("0 套（默认 无）")).toBeInTheDocument();
    expect(screen.getByText("未选择")).toBeInTheDocument();
  });
});
