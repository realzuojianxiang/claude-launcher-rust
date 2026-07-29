import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Vite 配置：Tauri 前端构建入口
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    clearMocks: true,
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: "es2021",
    minify: "oxc",
    sourcemap: false,
    // 不自动清空 dist：环境中 fs.rmSync 被 safe-delete shim 拦截（回收站操作会失败），
    // 故交由 Tauri/手动清理，避免 vite emptyOutDir 触发构建失败。
    emptyOutDir: false,
  },
});
