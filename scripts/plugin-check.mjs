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
//      `// @ts-check` modules (typeCheckOutside), which extends the plugin project's own
//      tsconfig.json when there is one (in the folder or above it, up to its package.json),
//      so the project's compiler options hold (lane 15).
// Paths print relative to where you ran it (absolute when outside), never as ../../.. from
// this repository (lane 15).
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

/** A path as a person reads it: relative to where the command ran, or absolute when it is
 * not below it (lane 15: no "../../../.." from this repository). Forward slashes. */
export function displayPath(p, cwd = process.cwd()) {
  const rel = path.relative(cwd, p);
  const out = rel && !rel.startsWith("..") && !path.isAbsolute(rel) ? rel : rel === "" ? "." : p;
  return out.replace(/\\/g, "/");
}

/** The plugin project's own tsconfig.json: in `dir` or a folder above it, stopping at the
 * first folder with a package.json (the project's root). `null` when there is none. */
export function projectTsconfig(dir) {
  let at = path.resolve(dir);
  for (;;) {
    const t = path.join(at, "tsconfig.json");
    if (existsSync(t)) return t;
    if (existsSync(path.join(at, "package.json"))) return null;
    const up = path.dirname(at);
    if (up === at) return null;
    at = up;
  }
}

/**
 * Type-check a plugin folder outside plugins/: a throwaway tsconfig (in the system's temp
 * folder) lists its `// @ts-check` modules and the API's declarations. With the project's
 * own tsconfig.json it extends that one (its compiler options hold; only the files listed
 * are checked, and nothing is emitted); without one it uses plugins/tsconfig.json's options.
 */
export function typeCheckOutside(tsc, dir, modules, cwd = process.cwd()) {
  const tmp = mkdtempSync(path.join(tmpdir(), "sawhorse-plugin-check-"));
  try {
    const own = projectTsconfig(dir);
    const files = [...modules.map((m) => path.join(dir, m)), path.join(root, "packages", "plugin-api", "index.d.ts")].map((f) => f.replace(/\\/g, "/"));
    const config = own
      ? { extends: own.replace(/\\/g, "/"), compilerOptions: { noEmit: true, allowJs: true }, files, include: [] }
      : {
          compilerOptions: { allowJs: true, checkJs: false, noEmit: true, strict: true, noUnusedLocals: true, target: "ES2022", module: "ESNext", moduleResolution: "bundler", lib: ["ES2022", "WebWorker"], types: [], skipLibCheck: true },
          files,
        };
    const file = path.join(tmp, "tsconfig.json");
    writeFileSync(file, JSON.stringify(config, null, 2));
    // tsc prints file names relative to its working folder: run it where the person did.
    const r = spawnSync(process.execPath, [tsc, "-p", file, "--pretty", "false"], { cwd, encoding: "utf8" });
    // tsc names files relative to its working folder (a climb of ../ for a folder elsewhere
    // on the disk): name them the way the rest of the report does.
    const out = `${r.stdout ?? ""}${r.stderr ?? ""}`.trim().replace(/^(.+?)(\(\d+,\d+\): )/gm, (_m, f, rest) => `${displayPath(path.resolve(cwd, f), cwd)}${rest}`);
    return { ok: r.status === 0, out, tsconfig: own };
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
    const inside = path.relative(root, dir).replace(/\\/g, "/").startsWith("plugins/");
    const rel = displayPath(dir);
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
      const how = `against the Sawhorse plugin API${r.tsconfig ? `, with ${displayPath(r.tsconfig)}` : ""}`;
      if (r.ok) console.log(`ok   type check ${rel} (${checked.length} module${checked.length === 1 ? "" : "s"} ${how})`);
      else {
        bad++;
        console.log(`FAIL type check ${rel} (${how})\n${r.out}`);
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
