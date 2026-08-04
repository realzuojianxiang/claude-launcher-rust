import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { configFixture } from "../test/fixtures";
import { GrokPage } from "./GrokPage";
import type { GrokOAuthState } from "../types";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// GrokOAuthCard 在挂载时 listen() 两个 Tauri 事件；JSDOM 下没有真实事件层，
// 这里把 listen mock 成返回一个 noop unlisten，挂载即 resolve 即可。
const { listenMock } = vi.hoisted(() => ({
  listenMock: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

const OAUTH_INIT: GrokOAuthState = {
  authorized: false,
  account: "",
  expires_at: 0,
  expired: false,
  refreshable: false,
  userCode: null,
  verificationUri: null,
  verificationUriComplete: null,
  expires_in: null,
  busy: false,
  error: null,
};

describe("GrokPage", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "grok_status") {
        return Promise.resolve({
          running: false,
          endpoint: "http://127.0.0.1:8083/v1/messages",
        });
      }
      if (command === "grok_pool") {
        // KeyPoolCard 在 !running 时只显示"代理未运行"，形状给齐即可
        return Promise.resolve({
          running: false,
          total: 0,
          available: 0,
          cooling: 0,
          keys: [],
        });
      }
      if (command === "grok_oauth_status") {
        return Promise.resolve({
          authorized: false,
          account: "",
          expires_at: 0,
          expired: false,
          refreshable: false,
        });
      }
      return Promise.resolve("");
    });
    listenMock.mockResolvedValue(() => undefined);
  });

  it("renders the OAuth authorization, status, key-pool, test, and config sections", async () => {
    render(
      <GrokPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
        oauthState={OAUTH_INIT}
        onOauthState={vi.fn()}
      />
    );

    expect(screen.getByRole("heading", { name: "Grok 代理" })).toBeInTheDocument();
    expect(
      screen.getByText("Grok 账号授权（OAuth）")
    ).toBeInTheDocument();
    expect(screen.getByText("Key 池状态")).toBeInTheDocument();
    expect(screen.getByText("本地测试 8083")).toBeInTheDocument();
    // 认证模式选择器（OAuth / API Key）存在
    expect(
      screen.getByText("OAuth（Plus 账号 → CLI Chat-Proxy，主线）")
    ).toBeInTheDocument();
    // 模型名映射编辑器标题
    expect(
      screen.getByText(
        "模型名映射（Anthropic 侧 → Grok 上游 slug，未命中回退 models[0] / 通配前缀）"
      )
    ).toBeInTheDocument();
  });

  it("wires model additions to the live grok_set_models command", async () => {
    render(
      <GrokPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
        oauthState={OAUTH_INIT}
        onOauthState={vi.fn()}
      />
    );

    // 页面上有两个「添加」按钮（模型优先级 + 模型名映射各一个），需按输入框定位
    // 模型优先级那个输入框，再 click 它所在 mp-add 行内的「添加」。
    const input = screen.getByPlaceholderText("添加 Grok 模型，如 grok-4.3");
    fireEvent.change(input, { target: { value: "grok-3-mini" } });
    const addRow = input.closest(".mp-add");
    const addBtn = addRow!.querySelector<HTMLButtonElement>("button")!;
    fireEvent.click(addBtn);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("grok_set_models", {
        models: ["grok-4.3", "grok-3-mini-fast", "grok-3-mini"],
      });
    });
  });

  it("generates a strong local proxy auth token into the input", async () => {
    render(
      <GrokPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
        oauthState={OAUTH_INIT}
        onOauthState={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "生成安全 Token" }));

    expect(
      (screen.getByPlaceholderText("留空表示不校验 x-api-key") as HTMLInputElement)
        .value
    ).toMatch(/^[0-9a-f]{64}$/);
  });
});
