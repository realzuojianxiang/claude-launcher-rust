import { describe, expect, it } from "vitest";
import { configFixture } from "./test/fixtures";
import { buildProviderEnv } from "./providerEnv";

describe("buildProviderEnv", () => {
  it("returns the selected named profile environment unchanged", () => {
    expect(buildProviderEnv(configFixture, "CLIProxyAPI")).toEqual({
      ANTHROPIC_BASE_URL: "http://localhost:8317",
      ANTHROPIC_API_KEY: "profile-key",
    });
  });

  it("builds the local NVIDIA environment from the current NVIDIA config", () => {
    expect(
      buildProviderEnv(configFixture, "🟩 NVIDIA 代理 (本地 8082)")
    ).toEqual({
      __nvidia_isolate__: "1",
      ANTHROPIC_BASE_URL: "http://127.0.0.1:8082",
      ANTHROPIC_API_KEY: "local-auth-token",
      ANTHROPIC_MODEL: "nvidia/test-model",
      ANTHROPIC_SMALL_FAST_MODEL: "nvidia/test-model",
    });
  });

  it("uses safe local defaults before configuration has loaded", () => {
    expect(buildProviderEnv(null, "🟩 NVIDIA 代理 (本地 8082)")).toEqual({
      __nvidia_isolate__: "1",
      ANTHROPIC_BASE_URL: "http://127.0.0.1:8082",
      ANTHROPIC_API_KEY: "sk-nvidia-local",
      ANTHROPIC_MODEL: "",
      ANTHROPIC_SMALL_FAST_MODEL: "",
    });
  });
});
