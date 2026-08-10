import type { Config } from "./types";

export const NVIDIA_PROVIDER = "🟩 NVIDIA 代理 (本地 8082)";
export const GATEWAY_PROVIDER = "🟦 协议网关 (本地 8083)";

export function buildProviderEnv(
  config: Config | null,
  profileName: string
): Record<string, string> {
  if (profileName === NVIDIA_PROVIDER) {
    const nvidia = config?.nvidia;
    const model = nvidia?.models?.[0] || "";
    const port = nvidia?.port ?? 8082;
    return {
      __nvidia_isolate__: "1",
      ANTHROPIC_BASE_URL: `http://127.0.0.1:${port}`,
      ANTHROPIC_API_KEY:
        (nvidia?.auth_token && nvidia.auth_token.trim()) || "sk-nvidia-local",
      ANTHROPIC_MODEL: model,
      ANTHROPIC_SMALL_FAST_MODEL: model,
    };
  }

  if (profileName === GATEWAY_PROVIDER) {
    const gateway = config?.gateway;
    const provider = gateway?.providers?.find(
      (p) => p.id === gateway.active_provider
    ) || gateway?.providers?.[0];
    const port = provider?.port ?? 8083;
    // 代理层会按 model_map 把入站 claude-* 映射成上游 slug，故这里只需传一个
    // 能被 map_model 命中默认回退的 Anthropic 侧模型名（让 Claude Code 自洽）。
    const model = "claude-sonnet-4";
    return {
      __gateway_isolate__: "1",
      ANTHROPIC_BASE_URL: `http://127.0.0.1:${port}`,
      ANTHROPIC_API_KEY:
        (provider?.auth_token && provider.auth_token.trim()) ||
        "sk-gateway-local",
      ANTHROPIC_MODEL: model,
      ANTHROPIC_SMALL_FAST_MODEL: model,
    };
  }

  return config?.profiles.find((profile) => profile.name === profileName)?.env ?? {};
}
