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
  // Relative by default (the Tauri webview and any static folder). The public demo is built
  // with VITE_BASE=/app/ (.github/workflows/pages.yml, scripts/build-site.mjs) so it is
  // served at https://sawhorse3d.com/app/; the kernel worker and its WASM resolve under it.
  base: process.env.VITE_BASE || "./",
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
    // Source maps are opt-in (SAWHORSE_SOURCEMAP=1 for a local debugging build). A map embeds
    // every dependency's original source, and openskp's carries a 5,796-byte SketchUp-authored
    // blank template that tree-shaking keeps out of our shipped JS; the v0.1.0-alpha.1
    // installers shipped it inside skp-*.js.map. scripts/check-provenance.mjs refuses any
    // dist that contains a map or those bytes.
    sourcemap: process.env.SAWHORSE_SOURCEMAP === "1",
    // three.js alone is ~600 kB minified; one chunk is fine for a desktop-class app.
    chunkSizeWarningLimit: 1024,
  },
  worker: { format: "es" },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}", "packages/**/*.test.ts"],
    setupFiles: ["src/test-setup.ts"],
  },
});
