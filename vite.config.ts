/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// This checkout's own root (forward slashes), so watch exclusions below match only what is
// inside it: a dev server started inside a lane worktree (<repo>/.worktrees/<lane>) must
// still watch its own files.
const root = fileURLToPath(new URL(".", import.meta.url)).replace(/\\/g, "/").replace(/\/$/, "");

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
    // Other lanes' git worktrees (this checkout's .worktrees/) and lane-end captures
    // (_captures/) never trigger a reload. Anchored at this checkout: a bare
    // "**/.worktrees/**" also matched every file of a server run from inside a worktree, so
    // edits there were never picked up.
    watch: { ignored: [`${root}/src-tauri/**`, `${root}/crates/**`, `${root}/.worktrees/**`, "**/_captures/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  // openskp is only imported on the first .skp import (by the kernel worker). Without this,
  // Vite discovers it then, re-optimises and reloads the dev page mid-import.
  optimizeDeps: { include: ["openskp"] },
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
