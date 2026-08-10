import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Config } from "../types";
import { configFixture } from "../test/fixtures";
import { GatewayPage } from "./GatewayPage";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe("GatewayPage", () => {
  beforeEach(() => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "gateway_status") {
        return Promise.resolve({
          running: false,
          endpoint: "http://127.0.0.1:8083/v1/messages",
        });
      }
      if (command === "gateway_pool") {
        // KeyPoolCard 在 !running 时只显示"代理未运行"，形状给齐即可
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

  it("renders the status, key-pool, test, and config sections (no OAuth card)", async () => {
    render(
      <GatewayPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
      />
    );

    expect(screen.getByRole("heading", { name: "协议网关" })).toBeInTheDocument();
    // OAuth 卡不应存在（协议网关仅 API Key，无 OAuth）
    expect(screen.queryByText("Grok 账号授权（OAuth）")).not.toBeInTheDocument();
    expect(screen.getByText("Key 池状态")).toBeInTheDocument();
    expect(screen.getByText("本地测试 8083")).toBeInTheDocument();
    // 认证模式选择器不应存在（仅 API Key 一种模式）
    expect(
      screen.queryByText("OAuth（Plus 账号 → CLI Chat-Proxy，主线）")
    ).not.toBeInTheDocument();
    // 模型名映射编辑器标题（新文案，上游 provider slug）
    expect(
      screen.getByText(
        "模型名映射（Anthropic 侧 → 上游 provider slug，未命中回退 models[0] / 通配前缀）"
      )
    ).toBeInTheDocument();
  });

  it("wires model additions to the live gateway_set_models command", async () => {
    render(
      <GatewayPage
        config={configFixture}
        onConfig={vi.fn()}
        test={{ testBusy: false, testResult: null, chatTests: {} }}
        onTest={vi.fn()}
      />
    );

    // 页面上有两个「添加」按钮（模型优先级 + 模型名映射各一个），需按输入框定位
    // 模型优先级那个输入框，再 click 它所在 mp-add 行内的「添加」。
    const input = screen.getByPlaceholderText("添加模型，如 gpt-4.1");
    fireEvent.change(input, { target: { value: "gpt-4.1-nano" } });
    const addRow = input.closest(".mp-add");
    const addBtn = addRow!.querySelector<HTMLButtonElement>("button")!;
    fireEvent.click(addBtn);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("gateway_set_models", {
        models: ["gpt-4.1", "gpt-4.1-mini", "gpt-4.1-nano"],
      });
    });
  });

  it("generates a strong local proxy auth token into the input", async () => {
    render(
      <GatewayPage
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

  it("adds a new provider via + 添加 and persists it through 保存网关配置", async () => {
    // 模拟首次使用：网关尚无任何 provider，用户点「+ 添加」新建
    const initial: Config = {
      ...configFixture,
      gateway: { providers: [], active_provider: "" },
    };

    function Harness() {
      const [cfg, setCfg] = useState(initial);
      return (
        <GatewayPage
          config={cfg}
          onConfig={setCfg}
          test={{ testBusy: false, testResult: null, chatTests: {} }}
          onTest={vi.fn()}
        />
      );
    }
    render(<Harness />);

    // 初始：下拉里没有 provider（无可用条目）
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.options.length).toBe(0);

    // 点击「+ 添加」：构造默认 Chat Completions provider 并设为 active
    fireEvent.click(screen.getByRole("button", { name: "+ 添加" }));

    // 新 provider 出现、成为 active、名字回填「新 Provider」
    await waitFor(() => {
      expect((screen.getByRole("combobox") as HTMLSelectElement).options.length).toBe(1);
    });
    // 用名称输入框专属 placeholder 精确定位，避免与下拉选项文本歧义
    const nameInput = screen.getByPlaceholderText(
      "如 DeepSeek / GLM / Qwen"
    ) as HTMLInputElement;
    expect(nameInput.value).toBe("新 Provider");

    // 填入 API Key（placeholder 含换行，用正则模糊匹配避免空白归一化问题）
    const keysArea = screen.getByPlaceholderText(/sk-xxx1/) as HTMLTextAreaElement;
    fireEvent.change(keysArea, { target: { value: "sk-newprovider-key" } });

    // 点「保存网关配置」→ 应调 set_gateway_config 落盘新 provider
    fireEvent.click(screen.getByRole("button", { name: "保存网关配置" }));

    await waitFor(() => {
      const call = invokeMock.mock.calls.find((c) => c[0] === "set_gateway_config");
      expect(call).toBeDefined();
      const gateway = (call as unknown as [string, { gateway: Config["gateway"] }])[1]
        .gateway;
      expect(gateway.providers.length).toBe(1);
      const added = gateway.providers[0];
      expect(added.name).toBe("新 Provider");
      expect(added.api_keys).toEqual(["sk-newprovider-key"]);
      // 默认协议/认证模式正确（与 collect() 硬编码一致）
      expect(added.protocol).toBe("chat-completions");
      expect(added.auth_mode).toBe("api-key");
      // active_provider 指向新 provider
      expect(gateway.active_provider).toBe(added.id);
    });
  });
});
