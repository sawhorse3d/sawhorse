/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// One front end, two hosts: the plain browser build (zero-install demo) and the
// Tauri 2 webview. Tauri expects a fixed dev port and no screen clearing.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 5174 } : undefined,
    // Other lanes' git worktrees (.worktrees/) and lane-end captures (_captures/) never
    // trigger a reload.
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/.worktrees/**", "**/_captures/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: "es2022",
    sourcemap: true,
    // three.js alone is ~600 kB minified; one chunk is fine for a desktop-class app.
    chunkSizeWarningLimit: 1024,
  },
  worker: { format: "es" },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test-setup.ts"],
  },
});
