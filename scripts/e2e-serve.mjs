#!/usr/bin/env node
// The end-to-end tests' server (playwright.config.ts webServer).
//
// Lane 17: a release build has no page-context plugin hooks (window.__sawhorse.plugins can
// enable a plugin without the permission dialog, so it is compiled only for tests:
// vite.config.ts testHooks). The specs drive plugins through those hooks, so they run
// against their own build: the same production build (same minifier, same content security
// policy, same chunks) with SAWHORSE_TEST_HOOKS=1, written to dist-e2e/ and served by
// `vite preview`. `npm run build` (dist/) stays the release and is checked to have none.
//
//   node scripts/e2e-serve.mjs --port 4173
import { spawn, spawnSync } from "node:child_process";
import { existsSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const at = args.indexOf("--port");
const port = at >= 0 ? args[at + 1] : process.env.PW_PORT || "4173";
const out = join(root, "dist-e2e");
const vite = join(root, "node_modules", "vite", "bin", "vite.js");
const env = { ...process.env, SAWHORSE_TEST_HOOKS: "1" };

const step = (what, cmd, argv) => {
  console.log(`[e2e-serve] ${what}`);
  const r = spawnSync(cmd, argv, { cwd: root, env, stdio: "inherit" });
  if (r.status !== 0) {
    console.error(`[e2e-serve] ${what} failed (${r.status ?? r.signal})`);
    process.exit(r.status ?? 1);
  }
};

step("this checkout's kernel", process.execPath, [join(root, "scripts", "wasm-checkout.mjs")]);
if (existsSync(out)) rmSync(out, { recursive: true, force: true });
step("vite build --outDir dist-e2e (SAWHORSE_TEST_HOOKS=1)", process.execPath, [vite, "build", "--outDir", out, "--emptyOutDir"]);
step("the build has the test hooks", process.execPath, [join(root, "scripts", "check-test-hooks.mjs"), "--expect", out]);

console.log(`[e2e-serve] vite preview --outDir dist-e2e --port ${port}`);
const server = spawn(process.execPath, [vite, "preview", "--outDir", out, "--port", String(port), "--strictPort"], { cwd: root, env, stdio: "inherit" });
const stop = () => server.kill();
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
process.on("exit", stop);
server.on("exit", (code) => process.exit(code ?? 0));
