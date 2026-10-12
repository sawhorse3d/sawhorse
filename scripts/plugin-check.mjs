#!/usr/bin/env node
// npm run plugin:check [dir ...]
//
// Checks plugins the way the app does before it lists or starts one, plus a type check:
//   1. plugin.json against the manifest rules (packages/plugin-api/src/manifest.ts, the
//      same validator the app uses);
//   2. every module reachable from the entry, parsed as an ES module (acorn): imports only
//      "sawhorse" and its own files, no dynamic import(), no importScripts, no circles
//      (packages/plugin-api/src/source.ts);
//   3. README.md and a licence file present (the samples' rule; a warning elsewhere);
//   4. the JavaScript type-checked against packages/plugin-api/index.d.ts: under plugins/
//      by plugins/tsconfig.json; a folder anywhere else by a throwaway tsconfig listing its
//      `// @ts-check` modules (typeCheckOutside).
// With no arguments it checks every folder under plugins/. Exit code 1 on any problem.
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const { parseManifest } = await import(pathToFileURL(path.join(root, "packages/plugin-api/src/manifest.ts")).href);
const { checkPackage } = await import(pathToFileURL(path.join(root, "packages/plugin-api/src/source.ts")).href);

/** Every file under `dir` as text (or bytes), keyed by its path inside the plugin (forward slashes). */
export function readTree(dir, { bytes = false } = {}) {
  const out = {};
  const walk = (d, prefix) => {
    for (const name of readdirSync(d).sort()) {
      if (name.startsWith(".") || name === "node_modules") continue;
      const full = path.join(d, name);
      if (statSync(full).isDirectory()) walk(full, `${prefix}${name}/`);
      else out[`${prefix}${name}`] = bytes ? new Uint8Array(readFileSync(full)) : readFileSync(full, "utf8");
    }
  };
  walk(dir, "");
  return out;
}

/** Problems with one plugin folder (empty: fine). */
export function checkFolder(dir, { strict = true } = {}) {
  const problems = [];
  const warnings = [];
  if (!existsSync(path.join(dir, "plugin.json"))) return { problems: [`${dir}: no plugin.json`], warnings, manifest: null };
  const files = readTree(dir);
  const r = parseManifest(files["plugin.json"]);
  if (!r.ok) return { problems: r.errors.map((e) => `plugin.json: ${e}`), warnings, manifest: null };
  const pkg = checkPackage(files, r.manifest.entry);
  problems.push(...pkg.problems);
  const need = (cond, text) => (cond ? null : (strict ? problems : warnings).push(text));
  need("README.md" in files, "README.md is missing");
  need(Object.keys(files).some((f) => /^LICEN[CS]E(\.md|\.txt)?$/i.test(f)), "a LICENSE file is missing");
  for (const m of pkg.order) if (!/^\s*\/\/\s*@ts-check/.test(files[m])) warnings.push(`${m} does not start with // @ts-check (it is not type-checked)`);
  return { problems, warnings, manifest: r.manifest, modules: pkg.order };
}

/**
 * Type-check a plugin folder outside plugins/: a throwaway tsconfig (in the system's temp
 * folder) lists its `// @ts-check` modules and the API's declarations, with the options of
 * plugins/tsconfig.json except that only the listed files are checked.
 */
export function typeCheckOutside(tsc, dir, modules) {
  const tmp = mkdtempSync(path.join(tmpdir(), "sawhorse-plugin-check-"));
  try {
    const config = {
      compilerOptions: { allowJs: true, checkJs: false, noEmit: true, strict: true, noUnusedLocals: true, target: "ES2022", module: "ESNext", moduleResolution: "bundler", lib: ["ES2022", "WebWorker"], types: [], skipLibCheck: true },
      files: [...modules.map((m) => path.join(dir, m)), path.join(root, "packages", "plugin-api", "index.d.ts")].map((f) => f.replace(/\\/g, "/")),
    };
    const file = path.join(tmp, "tsconfig.json");
    writeFileSync(file, JSON.stringify(config, null, 2));
    const r = spawnSync(process.execPath, [tsc, "-p", file], { cwd: root, encoding: "utf8" });
    return { ok: r.status === 0, out: `${r.stdout ?? ""}${r.stderr ?? ""}`.trim() };
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

async function main() {
  const args = process.argv.slice(2);
  const pluginsDir = path.join(root, "plugins");
  const dirs = args.length ? args.map((a) => path.resolve(a)) : existsSync(pluginsDir) ? readdirSync(pluginsDir).map((d) => path.join(pluginsDir, d)).filter((d) => statSync(d).isDirectory()) : [];
  let bad = 0;
  const tsc = path.join(root, "node_modules", "typescript", "bin", "tsc");
  for (const dir of dirs) {
    const rel = path.relative(root, dir).replace(/\\/g, "/");
    const inside = rel.startsWith("plugins/");
    const { problems, warnings, manifest, modules } = checkFolder(dir, { strict: inside });
    if (problems.length) bad++;
    console.log(`${problems.length ? "FAIL" : "ok  "} ${rel}${manifest ? `  ${manifest.id} ${manifest.version} (${manifest.permissions.join(", ") || "no permissions"}; ${modules.length} module${modules.length === 1 ? "" : "s"})` : ""}`);
    for (const p of problems) console.log(`       ${p}`);
    for (const w of warnings) console.log(`       warning: ${w}`);
    // A folder outside plugins/ is type-checked on its own: its `// @ts-check` modules
    // against packages/plugin-api/index.d.ts, with the samples' compiler options.
    if (!inside && manifest && !problems.length) {
      const checked = modules.filter((m) => /^\s*\/\/\s*@ts-check/.test(readFileSync(path.join(dir, m), "utf8")));
      if (!checked.length) {
        console.log(`       no // @ts-check module: not type-checked`);
        continue;
      }
      const r = typeCheckOutside(tsc, dir, checked);
      if (r.ok) console.log(`ok   type check ${rel} (${checked.length} module${checked.length === 1 ? "" : "s"} against packages/plugin-api/index.d.ts)`);
      else {
        bad++;
        console.log(`FAIL type check ${rel} (against packages/plugin-api/index.d.ts)\n${r.out}`);
      }
    }
  }
  if (!dirs.length) console.log("plugin:check: no plugins to check");
  // Type check against the published declarations.
  const tsconfig = path.join(pluginsDir, "tsconfig.json");
  if (existsSync(tsconfig) && dirs.some((d) => d.startsWith(pluginsDir))) {
    const r = spawnSync(process.execPath, [tsc, "-p", tsconfig], { cwd: root, encoding: "utf8" });
    const out = `${r.stdout ?? ""}${r.stderr ?? ""}`.trim();
    if (r.status !== 0) {
      bad++;
      console.log(`FAIL type check (plugins/tsconfig.json against packages/plugin-api/index.d.ts)\n${out}`);
    } else console.log("ok   type check (plugins/tsconfig.json against packages/plugin-api/index.d.ts)");
  }
  console.log(bad ? `plugin:check: ${bad} problem${bad === 1 ? "" : "s"}` : `plugin:check: ${dirs.length} plugin${dirs.length === 1 ? "" : "s"} clean`);
  process.exitCode = bad ? 1 : 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
