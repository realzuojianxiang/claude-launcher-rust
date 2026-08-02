import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { configFixture } from "../test/fixtures";
import { NvidiaPage } from "./NvidiaPage";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe("NvidiaPage", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "nvidia_status") {
        return Promise.resolve({
          running: false,
          endpoint: "http://127.0.0.1:8082/v1/messages",
        });
      }
      if (command === "nvidia_key_pool") {
        return Promise.resolve({
          running: false,
          total: 0,
          available: 0,
          cooling: 0,
          keys: [],
        });
      }
      return Promise.resolve("");
    });
  });

  it("renders the status, test, key-pool, and configuration sections", async () => {
    render(
      <NvidiaPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
      />
    );

    expect(screen.getByRole("heading", { name: "NVIDIA 代理" })).toBeInTheDocument();
    expect(screen.getByText("Key 池状态")).toBeInTheDocument();
    expect(screen.getByText("本地测试 8082")).toBeInTheDocument();
    expect(
      screen.getByText("NVIDIA API Keys（每行一个，或逗号分隔）")
    ).toBeInTheDocument();
    expect(
      screen.getByText("模型优先级（顺序即优先级，第 1 个为默认/最高，其余依次 Fallback）")
    ).toBeInTheDocument();
  });

  it("wires model additions to the live model-priority command", async () => {
    render(
      <NvidiaPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
      />
    );

    const input = screen.getByPlaceholderText(
      "添加模型，如 nvidia/nemotron-3-ultra-550b-a55b"
    );
    fireEvent.change(input, { target: { value: "nvidia/second-model" } });
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("nvidia_set_models", {
        models: ["nvidia/test-model", "nvidia/second-model"],
      });
    });
  });

  it("generates a strong local proxy auth token into the input", async () => {
    render(
      <NvidiaPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "生成安全 Token" }));

    expect(
      (screen.getByPlaceholderText("留空表示不校验 x-api-key") as HTMLInputElement)
        .value
    ).toMatch(/^[0-9a-f]{64}$/);
  });
});
