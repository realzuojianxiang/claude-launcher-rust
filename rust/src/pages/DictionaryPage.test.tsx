import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DictionaryPage } from "./DictionaryPage";

describe("DictionaryPage", () => {
  beforeEach(() => {
    const values = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
      clear: () => values.clear(),
    });
  });

  it("loads the default builtin dictionary lazily and lists its words", async () => {
    render(<DictionaryPage />);

    // 内置词典按需动态加载：等待默认词典（Spinner Verbs）词条渲染出来
    expect(await screen.findByText("Accomplishing")).toBeInTheDocument();
    // 掌握进度基于加载后的词条正常统计
    expect(screen.getByText(/已掌握 0\/187/)).toBeInTheDocument();
  });

  it(
    "loads the IELTS dictionary only after it is selected",
    async () => {
      render(<DictionaryPage />);
      await screen.findByText("Accomplishing");

      // 切到雅思词典 tab，触发该词典 chunk 的动态加载。
      // 3427 条词在 jsdom 中渲染较慢，放宽查找与用例超时。
      fireEvent.click(screen.getByText("雅思词汇（有道词库）"));

      expect(
        await screen.findByText("abandon", {}, { timeout: 20000 })
      ).toBeInTheDocument();
      expect(screen.getByText(/已掌握 0\/3427/)).toBeInTheDocument();
    },
    40000
  );

  it("does not skip the next card after marking an unknown-only card as known", async () => {
    render(<DictionaryPage />);
    await screen.findByText("Accomplishing");

    fireEvent.click(screen.getByRole("button", { name: /背诵/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /只看未掌握/ }));

    expect(screen.getByText("Accomplishing")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /显示答案/ }));
    fireEvent.click(screen.getByRole("button", { name: /^认识$/ }));

    expect(screen.queryByText("Accomplishing")).not.toBeInTheDocument();
    expect(screen.getByText("Actioning")).toBeInTheDocument();
  });
});
