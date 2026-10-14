#!/usr/bin/env node
// A SketchUp file as the kernel op the app sends for it (File > Import's own code,
// src/io/skp.ts, through Vite's module runner), written as JSON: the input the section cut's
// timing harness reads (crates/kernel/tests/section_perf.rs, SAWHORSE_PERF_OP).
//
//   node scripts/skp-op.mjs <file.skp> <out.json>
//
// The model's files are private and never in this repository (docs/DEVELOPMENT.md); neither
// is the op written from one.
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runnerImport } from "vite";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const [input, output] = process.argv.slice(2);
if (!input || !output) {
  console.error("usage: node scripts/skp-op.mjs <file.skp> <out.json>");
  process.exit(2);
}
const { module } = await runnerImport(path.join(root, "src", "io", "skp.ts"), {
  root,
  configFile: false,
  logLevel: "error",
});
const bytes = readFileSync(input);
const r = module.skpToOp(new Uint8Array(bytes), path.basename(input));
if (!r.op) {
  console.error(`[skp-op] ${path.basename(input)} is empty: nothing to import`);
  process.exit(1);
}
const text = JSON.stringify(r.op);
writeFileSync(output, text);
const s = r.summary;
console.log(
  `[skp-op] ${path.basename(input)}: ${s.faces} faces, ${s.definitions} definitions, ${s.placements} placements -> ${path.basename(output)} (${(text.length / 1e6).toFixed(1)} MB)`,
);
