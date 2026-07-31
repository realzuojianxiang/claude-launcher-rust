import { describe, expect, test, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { LayoutDashboard, Rocket, Settings } from "lucide-react";
import Sidebar, { type SidebarItem } from "./Sidebar";

const items: SidebarItem[] = [
  { key: "dashboard", label: "仪表盘", icon: LayoutDashboard },
  { key: "launch", label: "启动 Claude", icon: Rocket },
  { key: "config", label: "配置", icon: Settings },
];

describe("Sidebar", () => {
  test("navigation landmark is named 主导航", () => {
    render(
      <Sidebar
        items={items}
        activeKey="dashboard"
        onSelect={vi.fn()}
        collapsed={false}
        onToggleCollapse={vi.fn()}
      />,
    );
    expect(screen.getByRole("navigation", { name: "主导航" })).toBeInTheDocument();
  });

  test("active item exposes aria-current=page", () => {
    render(
      <Sidebar
        items={items}
        activeKey="launch"
        onSelect={vi.fn()}
        collapsed={false}
        onToggleCollapse={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "启动 Claude" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("button", { name: "仪表盘" })).not.toHaveAttribute(
      "aria-current",
    );
  });

  test("collapsed items retain accessible names", () => {
    render(
      <Sidebar
        items={items}
        activeKey="dashboard"
        onSelect={vi.fn()}
        collapsed={true}
        onToggleCollapse={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "仪表盘" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "启动 Claude" })).toBeInTheDocument();
  });

  test("collapse control toggles its accessible name", () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <Sidebar
        items={items}
        activeKey="dashboard"
        onSelect={vi.fn()}
        collapsed={false}
        onToggleCollapse={onToggle}
      />,
    );
    expect(screen.getByRole("button", { name: "折叠菜单" })).toBeInTheDocument();

    rerender(
      <Sidebar
        items={items}
        activeKey="dashboard"
        onSelect={vi.fn()}
        collapsed={true}
        onToggleCollapse={onToggle}
      />,
    );
    expect(screen.getByRole("button", { name: "展开菜单" })).toBeInTheDocument();
  });

  test("selecting a menu item calls its exact key", () => {
    const onSelect = vi.fn();
    render(
      <Sidebar
        items={items}
        activeKey="dashboard"
        onSelect={onSelect}
        collapsed={false}
        onToggleCollapse={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "配置" }));
    expect(onSelect).toHaveBeenCalledWith("config");
  });
});
