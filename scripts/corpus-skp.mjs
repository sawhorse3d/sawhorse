#!/usr/bin/env node
// The .skp test corpus: 203 SketchUp files written by openskp's MIT writer from our own
// procedural models (src/io/corpus/models.ts), plus (lane 23) three files of SketchUp curves
// (a circle, an arc and a polygon written as curves), into tests/corpus/skp/ (gitignored).
//
//   npm run corpus:skp                  write the 203 files and check them against the manifest
//   npm run corpus:skp -- --update      write them and rewrite tests/corpus/manifest.json
//   npm run corpus:skp -- --count 20    only the first N (the golden sample)
//
// Every file is reproducible byte for byte: the manifest records each file's size and
// SHA-256, and CI regenerates the corpus and fails on any difference. The .skp files are not
// committed because openskp's writer starts every file from a SketchUp-saved blank scaffold
// that we do not redistribute (docs/CLEANROOM.md, ADR-0002).
//
// Node 24 runs the TypeScript model library directly (type stripping).
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const outDir = path.join(root, "tests", "corpus", "skp");
const manifestPath = path.join(root, "tests", "corpus", "manifest.json");
const args = process.argv.slice(2);
const update = args.includes("--update");
const countArg = args.indexOf("--count");
const { corpus, curveModels } = await import(new URL("../src/io/corpus/models.ts", import.meta.url).href);
const all = [...corpus(200), ...curveModels()];
const count = countArg >= 0 ? Number(args[countArg + 1]) : all.length;
const { parseSkp } = await import("openskp");

mkdirSync(outDir, { recursive: true });
for (const f of readdirSync(outDir)) if (f.endsWith(".skp")) rmSync(path.join(outDir, f));

const files = [];
let total = 0;
const t0 = performance.now();
for (const spec of all.slice(0, count)) {
  const bytes = spec.build();
  // Every file must parse back with the same library the app uses.
  const model = parseSkp(bytes.slice().buffer);
  const faces = [model.root, ...model.definitions.values()].reduce((n, d) => n + d.faces.length, 0);
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  writeFileSync(path.join(outDir, `${spec.name}.skp`), bytes);
  files.push({ name: `${spec.name}.skp`, family: spec.family, bytes: bytes.length, sha256, definitions: model.definitions.size, faces });
  total += bytes.length;
}
const ms = performance.now() - t0;
const families = {};
for (const f of files) families[f.family] = (families[f.family] ?? 0) + 1;
console.log(`[corpus] ${files.length} files, ${(total / 1024 / 1024).toFixed(2)} MB, ${Math.round(ms)} ms -> ${path.relative(root, outDir)}`);
console.log(`[corpus] families: ${Object.entries(families).map(([k, n]) => `${k} ${n}`).join(", ")}`);
console.log(`[corpus] faces: ${files.reduce((n, f) => n + f.faces, 0)}, largest file ${(Math.max(...files.map((f) => f.bytes)) / 1024).toFixed(1)} KB`);
if (total > 20 * 1024 * 1024) {
  console.error("[corpus] over the 20 MB budget");
  process.exitCode = 1;
}

if (update) {
  writeFileSync(manifestPath, JSON.stringify({ generator: "scripts/corpus-skp.mjs", openskp: JSON.parse(readFileSync(path.join(root, "node_modules", "openskp", "package.json"), "utf8")).version, count: files.length, bytes: total, files }, null, 2) + "\n");
  console.log(`[corpus] wrote ${path.relative(root, manifestPath)}`);
} else if (existsSync(manifestPath)) {
  const want = JSON.parse(readFileSync(manifestPath, "utf8"));
  const byName = new Map(want.files.map((f) => [f.name, f]));
  let bad = 0;
  for (const f of files) {
    const w = byName.get(f.name);
    if (!w || w.sha256 !== f.sha256 || w.bytes !== f.bytes) {
      bad++;
      console.error(`[corpus] ${f.name}: differs from the manifest (${w ? `${w.bytes} B ${w.sha256.slice(0, 12)}` : "missing"} -> ${f.bytes} B ${f.sha256.slice(0, 12)})`);
    }
  }
  if (bad) {
    console.error(`[corpus] ${bad} file(s) differ: the generator or openskp changed. If intended: npm run corpus:skp -- --update, and refresh the golden images.`);
    process.exitCode = 1;
  } else {
    console.log(`[corpus] all ${files.length} match the manifest byte for byte`);
  }
} else {
  console.error(`[corpus] no manifest at ${path.relative(root, manifestPath)}; run with --update`);
  process.exitCode = 1;
}
