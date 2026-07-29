import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DictionaryPage } from "./DictionaryPage";

describe("DictionaryPage study mode", () => {
  beforeEach(() => {
    const values = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
      clear: () => values.clear(),
    });
  });

  it("does not skip the next card after marking an unknown-only card as known", () => {
    render(<DictionaryPage />);

    fireEvent.click(screen.getByRole("button", { name: /背诵/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /只看未掌握/ }));

    expect(screen.getByText("Accomplishing")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /显示答案/ }));
    fireEvent.click(screen.getByRole("button", { name: /^认识$/ }));

    expect(screen.queryByText("Accomplishing")).not.toBeInTheDocument();
    expect(screen.getByText("Actioning")).toBeInTheDocument();
  });
});
