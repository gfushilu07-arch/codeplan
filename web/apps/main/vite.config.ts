import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // 后端地址:默认 8081(避开本机 8080 常驻代理),可用 CODEPLAN_BACKEND_URL 覆盖
    proxy: {
      "/api": {
        target: (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.CODEPLAN_BACKEND_URL ?? "http://localhost:8081",
        changeOrigin: true,
      },
    },
  },
});
