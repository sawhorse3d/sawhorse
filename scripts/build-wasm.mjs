#!/usr/bin/env node
// Build the kernel to WebAssembly with wasm-pack, into src/wasm/pkg (gitignored), where
// the kernel worker picks it up (src/kernel/kernel.worker.ts).
//
//   npm run wasm            release build
//   npm run wasm -- --dev   faster debug build (validator runs after every mutation)
//
// On Windows without the MSVC linker this runs inside WSL Ubuntu: build scripts and
// proc-macros are host binaries, so even a wasm32 build needs a native linker
// (docs/DEVELOPMENT.md). SAWHORSE_NO_WSL=1 forces a native run.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const profile = process.argv.includes("--dev") ? "--dev" : "--release";
const wasmPackArgs = [
  "build",
  "crates/wasm-api",
  "--target",
  "web",
  "--out-dir",
  "../../src/wasm/pkg",
  "--out-name",
  "sawhorse_wasm",
  "--no-pack",
  profile,
];

function hasMsvc() {
  const r = spawnSync("where", ["cl.exe"], { stdio: "ignore", shell: false });
  return r.status === 0;
}

function toWslPath(p) {
  const m = /^([A-Za-z]):[\\/](.*)$/.exec(p);
  if (!m) throw new Error(`cannot map ${p} into WSL`);
  return `/mnt/${m[1].toLowerCase()}/${m[2].replace(/\\/g, "/")}`;
}

let result;
if (process.platform === "win32" && !hasMsvc() && process.env.SAWHORSE_NO_WSL !== "1") {
  const cmd = `cd '${toWslPath(root)}' && . scripts/env.sh && wasm-pack ${wasmPackArgs.join(" ")}`;
  console.log(`[wasm] no MSVC linker on Windows: building in WSL\n[wasm] ${cmd}`);
  result = spawnSync("wsl.exe", ["bash", "-lc", cmd], { stdio: "inherit" });
} else {
  console.log(`[wasm] wasm-pack ${wasmPackArgs.join(" ")}`);
  result = spawnSync("wasm-pack", wasmPackArgs, { cwd: root, stdio: "inherit", shell: process.platform === "win32" });
}

if (result.error) {
  console.error(`[wasm] could not start the build: ${result.error.message}`);
  process.exit(1);
}
if (result.status !== 0) process.exit(result.status ?? 1);
const out = path.join(root, "src", "wasm", "pkg", "sawhorse_wasm_bg.wasm");
if (!existsSync(out)) {
  console.error(`[wasm] build reported success but ${out} is missing`);
  process.exit(1);
}
console.log(`[wasm] ok: ${path.relative(root, out)}`);
