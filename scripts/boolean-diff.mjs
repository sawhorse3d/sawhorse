#!/usr/bin/env node
// Boolean differential test: every case in crates/kernel/tests/booleans/cases.json runs
//   1. through the Sawhorse kernel compiled to WASM (src/wasm/pkg: triangulate ->
//      manifold-rust -> re-polygonise -> coplanar merge -> validate), and
//   2. through npm `manifold-3d` (the C++ Manifold, compiled to WASM) on the very same
//      operand meshes the kernel built,
// and compares volume, surface area, genus and component count with each other and with
// the hand-computed expectations in the case file. Run after `npm run wasm`:
//
//   npm run boolean:diff
//
// manifold-3d meshes are Float32; every case uses coordinates exact in f32, so the
// comparison tolerance is tight (1e-6 relative).
import { readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import Module from "manifold-3d";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const pkg = path.join(root, "src", "wasm", "pkg");
if (!existsSync(path.join(pkg, "sawhorse_wasm.js"))) {
  console.error("boolean-diff: src/wasm/pkg missing; run `npm run wasm` first");
  process.exit(1);
}
const kernel = await import(pathToFileURL(path.join(pkg, "sawhorse_wasm.js")).href);
kernel.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
const wasm = await Module();
wasm.setup();
const { Manifold, Mesh } = wasm;

const cases = JSON.parse(readFileSync(path.join(root, "crates", "kernel", "tests", "booleans", "cases.json"), "utf8"));
const near = (a, b, rel = 1e-6) => Math.abs(a - b) <= rel * Math.max(1, Math.abs(b));

function manifold(m) {
  const mesh = new Mesh({ numProp: 3, vertProperties: Float32Array.from(m.positions), triVerts: Uint32Array.from(m.triangles) });
  mesh.merge();
  return new Manifold(mesh);
}

let bad = 0;
const rows = [];
for (const c of cases) {
  const out = JSON.parse(kernel.boolean_case(JSON.stringify(c)));
  const ours = out.result;
  const a = manifold(out.a);
  const b = manifold(out.b);
  const r = c.op === "union" ? a.add(b) : c.op === "difference" ? a.subtract(b) : a.intersect(b);
  const ref = {
    empty: r.isEmpty(),
    volume: r.volume(),
    area: r.surfaceArea(),
    genus: r.genus(),
    shells: r.isEmpty() ? 0 : r.decompose().length,
  };
  const problems = [];
  const x = c.expect;
  // Reference engine vs hand-computed truth.
  if (x.empty !== undefined && x.empty !== ref.empty && !x.refused) problems.push(`manifold-3d empty=${ref.empty}`);
  if (x.volume !== undefined && !near(ref.volume, x.volume)) problems.push(`manifold-3d volume ${ref.volume} vs hand ${x.volume}`);
  if (x.area !== undefined && !near(ref.area, x.area)) problems.push(`manifold-3d area ${ref.area} vs hand ${x.area}`);
  // Kernel vs reference engine.
  if (x.refused) {
    if (!ours.refused) problems.push("kernel accepted a result it should refuse");
  } else if (ours.refused) {
    problems.push(`kernel refused: ${ours.refused}`);
  } else {
    if (ours.empty !== ref.empty) problems.push(`empty: kernel ${ours.empty}, manifold-3d ${ref.empty}`);
    if (!ref.empty) {
      if (!near(ours.volume, ref.volume)) problems.push(`volume: kernel ${ours.volume}, manifold-3d ${ref.volume}`);
      if (!near(ours.area, ref.area)) problems.push(`area: kernel ${ours.area}, manifold-3d ${ref.area}`);
      if (ours.shells !== ref.shells) problems.push(`components: kernel ${ours.shells}, manifold-3d ${ref.shells}`);
      // manifold-3d's genus is 1 - chi/2 over the whole mesh (components summed with -1
      // each), the kernel's is per shell, summed.
      const refGenus = ref.genus + (ref.shells - 1);
      if (ours.genus !== refGenus) problems.push(`genus: kernel ${ours.genus}, manifold-3d ${refGenus}`);
    }
  }
  for (const m of [a, b, r]) m.delete();
  if (problems.length) bad++;
  rows.push(
    `${problems.length ? "FAIL" : "ok  "} ${c.name.padEnd(38)} kernel ${ours.refused ? "refused".padEnd(26) : `V ${ours.volume.toFixed(4).padStart(8)} A ${ours.area.toFixed(4).padStart(8)}`}  manifold-3d V ${ref.volume.toFixed(4).padStart(8)} A ${ref.area.toFixed(4).padStart(8)} parts ${ref.shells}` +
      (problems.length ? `\n     ${problems.join("\n     ")}` : ""),
  );
}
console.log(rows.join("\n"));
console.log(`${cases.length} cases, ${bad === 0 ? "kernel == manifold-3d == hand-computed" : `${bad} mismatch(es)`}`);
process.exitCode = bad === 0 ? 0 : 1;
