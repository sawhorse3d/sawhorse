#!/usr/bin/env node
// Replays every crates/kernel/tests/golden/*.json through the WASM kernel (src/wasm/pkg)
// and compares hashes (and cut list and nesting hashes) with the natively computed goldens. Passing proves the browser
// kernel and the native kernel agree bit for bit. Run after `npm run wasm`.
import { readdirSync, readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const pkg = path.join(root, "src", "wasm", "pkg");
if (!existsSync(path.join(pkg, "sawhorse_wasm.js"))) {
  console.error("golden-wasm: src/wasm/pkg missing; run `npm run wasm` first");
  process.exit(1);
}
const mod = await import(pathToFileURL(path.join(pkg, "sawhorse_wasm.js")).href);
mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });

const dir = path.join(root, "crates", "kernel", "tests", "golden");
let bad = 0;
for (const f of readdirSync(dir).filter((f) => f.endsWith(".json")).sort()) {
  const g = JSON.parse(readFileSync(path.join(dir, f), "utf8"));
  const ops = JSON.stringify(g.ops);
  const got = mod.replay_hash(ops);
  // The cut list too (lane 7): the same parts, sizes and counts, hash for hash.
  const cut = g.cut_list ? JSON.parse(mod.replay_cut_list(ops, "{}")).hash : null;
  // And the nesting (lane 9): the same sheets and boards, placements and cuts.
  const nest = g.nesting ? JSON.parse(mod.replay_nesting(ops, "{}")).hash : null;
  const ok = got === g.hash && cut === (g.cut_list ?? null) && nest === (g.nesting ?? null);
  if (!ok) bad++;
  console.log(
    `${ok ? "ok  " : "FAIL"} ${f.padEnd(24)} wasm ${got}  native ${g.hash}${g.cut_list ? `  cut list ${cut}${cut === g.cut_list ? "" : ` (native ${g.cut_list})`}` : ""}${g.nesting ? `  nesting ${nest}${nest === g.nesting ? "" : ` (native ${g.nesting})`}` : ""}`,
  );
}
console.log(`kernel ${mod.kernel_version()}: ${bad === 0 ? "WASM matches native goldens" : `${bad} mismatch(es)`}`);
// exitCode, not process.exit(): avoids a libuv teardown assertion seen with Node 24 on Windows.
process.exitCode = bad === 0 ? 0 : 1;
