import type { Config } from "../types";

export const configFixture: Config = {
  work_dir: "D:\\work",
  yolo_mode: false,
  compact_window: 1_000_000,
  compact_pct: 70,
  profiles: [
    {
      name: "讯飞",
      env: {
        ANTHROPIC_BASE_URL: "https://maas-coding-api.cn-huabei-1.xf-yun.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "profile-key",
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
  grok: {
    auth_mode: "oauth",
    oauth_base_url: "https://cli-chat-proxy.grok.com/v1",
    api_base_url: "https://api.x.ai/v1",
    api_keys: [],
    models: ["grok-4.3", "grok-3-mini-fast"],
    model_map: [
      { anthropic_model: "claude-sonnet-4", grok_model: "grok-4.3" },
      { anthropic_model: "claude-haiku-4-5", grok_model: "grok-3-mini-fast" },
    ],
    host: "127.0.0.1",
    port: 8083,
    cooldown_seconds: 600,
    max_retries: 3,
    request_timeout_seconds: 600,
    auth_token: "grok-local-auth-token",
    oauth_account: "tester@example.com",
  },
};
