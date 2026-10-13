#!/usr/bin/env node
// Lane 17: a release has no page-context plugin hooks.
//
// window.__sawhorse.plugins (src/plugins/usePlugins.tsx) can load a plugin's files and enable
// it with no permission dialog, so any script running in the page could grant itself a whole
// manifest. It is compiled only when __SAWHORSE_TEST_HOOKS__ is true (vite.config.ts:
// the dev server, the unit tests, SAWHORSE_TEST_HOOKS=1 for the end-to-end build). This
// checks a built directory for the marker string the hooks carry.
//
// Usage:
//   node scripts/check-test-hooks.mjs [dir ...]            refuse a build that has the hooks (default: dist)
//   node scripts/check-test-hooks.mjs --expect [dir ...]   refuse a build that lacks them (the e2e build)
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const MARKER = "sawhorse-test-hooks";
const args = process.argv.slice(2);
const expect = args.includes("--expect");
const dirs = args.filter((a) => a !== "--expect");
if (!dirs.length) dirs.push("dist");

function* files(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) yield* files(p);
    else if (/\.(js|mjs|cjs|html)$/i.test(name)) yield p;
  }
}

let bad = 0;
for (const dir of dirs) {
  if (!existsSync(dir)) {
    console.error(`check-test-hooks: ${dir} does not exist (build it first)`);
    process.exit(1);
  }
  const hits = [...files(dir)].filter((p) => readFileSync(p, "latin1").includes(MARKER)).map((p) => relative(dir, p));
  if (expect && hits.length === 0) {
    console.error(`check-test-hooks: ${dir} has no plugin test hooks, but the end-to-end tests need them (build with SAWHORSE_TEST_HOOKS=1)`);
    bad++;
  } else if (!expect && hits.length) {
    console.error(`check-test-hooks: ${dir} has the plugin test hooks (window.__sawhorse.plugins) in ${hits.join(", ")}: a release must not (is SAWHORSE_TEST_HOOKS set?)`);
    bad++;
  } else console.log(`check-test-hooks: ${dir} ${expect ? `has the test hooks (${hits.join(", ")})` : "has no test hooks"}`);
}
process.exit(bad ? 1 : 0);
