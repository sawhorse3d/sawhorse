#!/usr/bin/env node
// Replays every crates/kernel/tests/golden/*.json through the WASM kernel (src/wasm/pkg)
// and compares hashes (and cut list, nesting, section cut and sheet layout hashes) with the natively computed goldens. Passing proves the browser
// kernel and the native kernel agree bit for bit. Run after `npm run wasm`.
import { readdirSync, readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { checkPkg } from "./wasm-checkout.mjs";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const pkg = path.join(root, "src", "wasm", "pkg");
if (!existsSync(path.join(pkg, "sawhorse_wasm.js"))) {
  console.error("golden-wasm: src/wasm/pkg missing; run `npm run wasm` first");
  process.exit(1);
}
// Another checkout's kernel proves nothing against these goldens (an older one fails with
// "unknown variant"): say which it is, first.
const built = await checkPkg(root);
if (!built.ok) {
  console.error(`golden-wasm: ${built.reason}; run \`npm run wasm\``);
  process.exit(1);
}
console.log(`golden-wasm: kernel ${built.reason}`);
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
  // And every section plane's cut (lane 12): the same caps and outlines, plane by plane.
  const cuts = g.sections ? JSON.parse(mod.replay_sections(ops)) : null;
  const sameCuts = JSON.stringify(cuts) === JSON.stringify(g.sections ?? null);
  // And every drawing sheet as laid out (lane 12): the same primitives, sheet by sheet.
  const renders = g.renders ? JSON.parse(mod.replay_sheet_renders(ops)) : null;
  const sameRenders = JSON.stringify(renders) === JSON.stringify(g.renders ?? null);
  const ok = got === g.hash && cut === (g.cut_list ?? null) && nest === (g.nesting ?? null) && sameCuts && sameRenders;
  if (!ok) bad++;
  console.log(
    `${ok ? "ok  " : "FAIL"} ${f.padEnd(24)} wasm ${got}  native ${g.hash}${g.cut_list ? `  cut list ${cut}${cut === g.cut_list ? "" : ` (native ${g.cut_list})`}` : ""}${g.nesting ? `  nesting ${nest}${nest === g.nesting ? "" : ` (native ${g.nesting})`}` : ""}${g.sections ? `  sections ${cuts.join(",")}${sameCuts ? "" : ` (native ${g.sections.join(",")})`}` : ""}${g.renders ? `  sheets ${renders.join(",")}${sameRenders ? "" : ` (native ${g.renders.join(",")})`}` : ""}`,
  );
}
console.log(`kernel ${mod.kernel_version()}: ${bad === 0 ? "WASM matches native goldens" : `${bad} mismatch(es)`}`);
// exitCode, not process.exit(): avoids a libuv teardown assertion seen with Node 24 on Windows.
process.exitCode = bad === 0 ? 0 : 1;
