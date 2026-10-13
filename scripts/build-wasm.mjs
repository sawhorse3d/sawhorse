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
//
// Afterwards it checks that the kernel in src/wasm/pkg was built from THIS checkout
// (scripts/wasm-checkout.mjs): a cargo target dir shared with another checkout can hand
// back the other checkout's kernel while cargo reports "Finished" in two seconds. When
// that happens it touches crates/wasm-api/src/lib.rs (so cargo compiles and copies this
// checkout's kernel out) and builds once more; a second miss is an error.
import { spawnSync } from "node:child_process";
import { existsSync, utimesSync } from "node:fs";
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

// CI Windows runners have MSVC (found by rustc via the registry, not PATH): never use WSL there.
const forceNative = process.env.SAWHORSE_NO_WSL === "1" || !!process.env.CI;

function build() {
  let result;
  if (process.platform === "win32" && !forceNative && !hasMsvc()) {
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
  return out;
}

/** 0: built from this checkout; 2: by another; 1: missing. A child process each time, since ES modules are cached. */
function checkout() {
  const r = spawnSync(process.execPath, [path.join(root, "scripts", "wasm-checkout.mjs")], { stdio: "inherit" });
  return r.status ?? 1;
}

let out = build();
if (checkout() === 2) {
  console.log("[wasm] cargo handed back a kernel another checkout built: rebuilding this checkout's");
  const lib = path.join(root, "crates", "wasm-api", "src", "lib.rs");
  const now = new Date();
  utimesSync(lib, now, now);
  out = build();
  if (checkout() !== 0) {
    console.error("[wasm] still another checkout's kernel: give this checkout its own CARGO_TARGET_DIR (scripts/env.sh does, per checkout)");
    process.exit(1);
  }
}
console.log(`[wasm] ok: ${path.relative(root, out)}`);
