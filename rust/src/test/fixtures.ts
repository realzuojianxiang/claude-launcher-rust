import type { Config } from "../types";

export const configFixture: Config = {
  work_dir: "D:\\work",
  anthropic_url: "http://localhost:8317",
  anthropic_key: "legacy-key",
  cliproxyapi_key: "",
  yolo_mode: false,
  compact_window: 1_000_000,
  compact_pct: 70,
  cliproxyapi_dir: "",
  profiles: [
    {
      name: "CLIProxyAPI",
      env: {
        ANTHROPIC_BASE_URL: "http://localhost:8317",
        ANTHROPIC_API_KEY: "profile-key",
      },
    },
  ],
  nvidia: {
    api_keys: ["nv-test-key"],
    models: ["nvidia/test-model"],
    base_url: "https://integrate.api.nvidia.com/v1",
    host: "127.0.0.1",
    port: 8082,
    key_cooldown_seconds: 65,
    max_retries: 3,
    request_timeout_seconds: 600,
    auth_token: "local-auth-token",
  },
};
