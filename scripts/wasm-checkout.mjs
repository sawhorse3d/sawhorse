#!/usr/bin/env node
// Was src/wasm/pkg built from THIS checkout's kernel?
//
// The kernel exports build_checkout(): the FNV-1a hash of the directory cargo compiled
// crates/wasm-api from (crates/wasm-api/src/lib.rs). When two checkouts share a cargo target
// dir, cargo copies the unhashed sawhorse_wasm.wasm out of deps/ only when it compiles, so a
// checkout that cargo thinks is fresh silently gets the kernel the OTHER checkout copied out
// last (the 2026-10-12 trap: golden:wasm failed with "unknown variant `add_section`", and the
// build and Playwright would have exercised a pre-lane kernel). This catches it.
//
//   node scripts/wasm-checkout.mjs        exit 0: built here; 1: missing; 2: another checkout
//                                          or a kernel older than the stamp
//
// Imported by scripts/build-wasm.mjs (which repairs the case once by itself) and
// scripts/golden-wasm.mjs. SAWHORSE_SKIP_WASM_CHECKOUT=1 skips the check (an unusual
// machine whose paths the candidates below miss; say so in the commit that uses it).
import { existsSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** FNV-1a, 64-bit, of a string's UTF-8 bytes, as 16 hex digits (the kernel's `fnv1a`). */
export function fnv1a(s) {
  let h = 0xcbf29ce484222325n;
  for (const b of new TextEncoder().encode(s)) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, "0");
}

/** The WSL path of a Windows path (C:\x\y -> /mnt/c/x/y), or null. */
export function toWslPath(p) {
  const m = /^([A-Za-z]):[\\/](.*)$/.exec(p);
  return m ? `/mnt/${m[1].toLowerCase()}/${m[2].replace(/\\/g, "/")}` : null;
}

/** Every way cargo may have spelled this checkout's crates/wasm-api directory, hashed. */
export function checkoutStamps(root) {
  const roots = new Set([root]);
  try {
    roots.add(realpathSync.native(root));
  } catch {
    /* a root that does not resolve keeps its own spelling */
  }
  const dirs = new Set();
  for (const r of roots) {
    const native = path.join(r, "crates", "wasm-api");
    dirs.add(native);
    dirs.add(native.replace(/\\/g, "/"));
    const wsl = toWslPath(native);
    if (wsl) dirs.add(wsl);
  }
  return new Set([...dirs].map(fnv1a));
}

/**
 * Check `root`'s src/wasm/pkg: `{ ok, reason }`. Loads the kernel (once per process: ES
 * modules are cached, so run this file as a child process to re-check after a rebuild).
 */
export async function checkPkg(root) {
  if (process.env.SAWHORSE_SKIP_WASM_CHECKOUT === "1") return { ok: true, reason: "skipped (SAWHORSE_SKIP_WASM_CHECKOUT=1)" };
  const pkg = path.join(root, "src", "wasm", "pkg");
  const js = path.join(pkg, "sawhorse_wasm.js");
  const wasm = path.join(pkg, "sawhorse_wasm_bg.wasm");
  if (!existsSync(js) || !existsSync(wasm)) return { ok: false, missing: true, reason: "src/wasm/pkg is missing; run `npm run wasm`" };
  const mod = await import(pathToFileURL(js).href);
  mod.initSync({ module: readFileSync(wasm) });
  if (typeof mod.build_checkout !== "function") {
    return { ok: false, reason: "src/wasm/pkg holds a kernel older than this checkout's (it has no build_checkout): another checkout built it into a shared cargo target dir" };
  }
  const got = mod.build_checkout();
  if (!checkoutStamps(root).has(got)) {
    return { ok: false, reason: `src/wasm/pkg was built by another checkout (stamp ${got}): two checkouts share a cargo target dir (scripts/env.sh gives each its own)` };
  }
  return { ok: true, reason: `built here (stamp ${got})` };
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
  const r = await checkPkg(root);
  console.log(`[wasm-checkout] ${r.ok ? "ok" : "FAIL"}: ${r.reason}`);
  process.exitCode = r.ok ? 0 : r.missing ? 1 : 2;
}
