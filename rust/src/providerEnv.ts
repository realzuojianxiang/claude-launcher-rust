import type { Config } from "./types";

export const NVIDIA_PROVIDER = "🟩 NVIDIA 代理 (本地 8082)";

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

  return config?.profiles.find((profile) => profile.name === profileName)?.env ?? {};
}
