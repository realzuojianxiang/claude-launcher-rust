import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { configFixture } from "./test/fixtures";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function clickMenu(label: string) {
  const item = Array.from(document.querySelectorAll<HTMLElement>("nav button")).find(
    (element) => element.textContent?.trim() === label
  );
  expect(item).toBeDefined();
  fireEvent.click(item!);
}

describe("App navigation", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_config") return Promise.resolve(configFixture);
      if (command === "config_path") return Promise.resolve("D:\\config.json");
      if (command === "cliproxyapi_status") {
        return Promise.resolve({ running: false, url: "http://localhost:8317" });
      }
      return Promise.resolve("");
    });
  });

  it("switches between business pages from the sidebar", async () => {
    render(<App />);
    expect(await screen.findByRole("heading", { name: "仪表盘" })).toBeInTheDocument();

    clickMenu("配置");
    expect(await screen.findByRole("heading", { name: "配置" })).toBeInTheDocument();

    clickMenu("关于");
    expect(await screen.findByRole("heading", { name: "关于" })).toBeInTheDocument();
  });

  it("lazily loads the dictionary page and its builtin dictionary on demand", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "仪表盘" });

    clickMenu("单词本");
    // 单词本页面是懒加载 chunk：等待页面标题出现
    expect(
      await screen.findByRole("heading", { name: "单词本" })
    ).toBeInTheDocument();
    // 内置词典数据也是按需加载：随后词条应正常渲染
    expect(await screen.findByText("Accomplishing")).toBeInTheDocument();
  });

  it("keeps unsaved profile edits when the config page is remounted", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "仪表盘" });

    clickMenu("配置");
    const profileName = await waitFor(() => {
      const input = document.querySelector<HTMLInputElement>(".profile-name");
      expect(input).not.toBeNull();
      return input!;
    });
    fireEvent.change(profileName, { target: { value: "未保存供应商" } });

    clickMenu("关于");
    await screen.findByRole("heading", { name: "关于" });
    clickMenu("配置");

    await waitFor(() => {
      expect(document.querySelector<HTMLInputElement>(".profile-name")).toHaveValue(
        "未保存供应商"
      );
    });
  });
});
