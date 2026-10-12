/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

// This checkout's own root (forward slashes), so watch exclusions below match only what is
// inside it: a dev server started inside a lane worktree (<repo>/.worktrees/<lane>) must
// still watch its own files.
const root = fileURLToPath(new URL(".", import.meta.url)).replace(/\\/g, "/").replace(/\/$/, "");

// One front end, two hosts: the plain browser build (zero-install demo) and the
// Tauri 2 webview. Tauri expects a fixed dev port and no screen clearing.
const host = process.env.TAURI_DEV_HOST;

/**
 * The browser build's content security policy, written into index.html as a meta tag.
 *
 * It is the second lock on plugins (ADR-0009): each plugin's worker is started from a blob:
 * URL (src/plugins/host.ts), so it runs under this policy, and a plugin that got past the
 * sandbox's lockdown still could not load a script or a font from anywhere but the app
 * itself (a font load is a request to any URL; so is an import()). `connect-src` stays wide
 * because a plugin's granted `net` requests are made by the page (to the hosts its manifest
 * lists, checked in src/plugins/manager.ts), and the render service may be on another host.
 *
 * The desktop build gets its policy from src-tauri/tauri.conf.json instead (Tauri sends it
 * with every page and may add nonces and hashes for its own scripts, which a second policy
 * written here would also have to allow), so the tag is left out when the Tauri CLI runs
 * the build (it sets TAURI_ENV_PLATFORM). Both policies refuse outside scripts and fonts.
 * Development (vite serve) has none: the dev client injects inline scripts.
 */
export function contentSecurityPolicy(renderService = process.env.VITE_RENDER_SERVICE_URL || "http://127.0.0.1:8787"): string {
  let render = "";
  try {
    render = new URL(renderService).origin;
  } catch {
    render = "";
  }
  const withRender = (list: string) => (render ? `${list} ${render}` : list);
  return [
    "default-src 'self'",
    "script-src 'self' 'wasm-unsafe-eval' blob:",
    "style-src 'self' 'unsafe-inline'",
    withRender("img-src 'self' data: blob:"),
    "font-src 'self' data:",
    "worker-src 'self' blob:",
    withRender("connect-src 'self'") + " https: http://localhost:* http://127.0.0.1:*",
    "media-src 'self' data: blob:",
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'none'",
  ].join("; ");
}

function cspMeta(): Plugin {
  return {
    name: "sawhorse-csp",
    apply: "build",
    transformIndexHtml() {
      if (process.env.TAURI_ENV_PLATFORM) return [];
      return [{ tag: "meta", attrs: { "http-equiv": "Content-Security-Policy", content: contentSecurityPolicy() }, injectTo: "head-prepend" }];
    },
  };
}

export default defineConfig({
  plugins: [react(), cspMeta()],
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
