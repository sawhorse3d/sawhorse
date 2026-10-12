#!/usr/bin/env node
// npm run plugin:pack -- <plugin folder> [--out <file.zip>]
//
// Checks one plugin (as plugin:check does) and zips its folder into
// dist-plugins/<id>-<version>.zip (or --out): the file "Install from .zip" takes. The zip
// holds the folder's files at its root (plugin.json first), in a stable order with a fixed
// date, so packing the same folder twice gives the same bytes.
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { zipSync } from "fflate";
import { checkFolder, readTree } from "./plugin-check.mjs";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const args = process.argv.slice(2);
const outAt = args.indexOf("--out");
const out = outAt >= 0 ? args[outAt + 1] : null;
const dir = args.find((a, i) => !a.startsWith("--") && i !== outAt + 1);
if (!dir) {
  console.error("usage: npm run plugin:pack -- <plugin folder> [--out file.zip]");
  process.exitCode = 2;
} else {
  const abs = path.resolve(dir);
  const { problems, warnings, manifest } = checkFolder(abs, { strict: false });
  for (const w of warnings) console.log(`warning: ${w}`);
  if (problems.length || !manifest) {
    for (const p of problems) console.error(`problem: ${p}`);
    console.error("plugin:pack: not packed");
    process.exitCode = 1;
  } else {
    const files = readTree(abs, { bytes: true });
    const names = Object.keys(files).sort((a, b) => (a === "plugin.json" ? -1 : b === "plugin.json" ? 1 : a.localeCompare(b)));
    const mtime = new Date(Date.UTC(2026, 0, 1));
    const entries = Object.fromEntries(names.map((n) => [n, [files[n], { mtime, level: 9 }]]));
    const zip = zipSync(entries);
    const target = path.resolve(out ?? path.join(root, "dist-plugins", `${manifest.id}-${manifest.version}.zip`));
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, zip);
    console.log(`plugin:pack: ${manifest.name} ${manifest.version} -> ${path.relative(process.cwd(), target).replace(/\\/g, "/")} (${names.length} files, ${(zip.length / 1024).toFixed(1)} KB)`);
  }
}
