// Fails if Tauri (Rust crate or npm CLI/API) resolved to anything but 2.x (ADR-0003: stay off 3.0 alpha).
import { readFileSync, existsSync } from "node:fs";

const problems = [];
const lock = JSON.parse(readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"));
for (const [path, meta] of Object.entries(lock.packages ?? {})) {
  if (/node_modules\/@tauri-apps\/(cli|api)$/.test(path) && !String(meta.version).startsWith("2.")) {
    problems.push(`${path} is ${meta.version}`);
  }
}
const cargoLock = new URL("../src-tauri/Cargo.lock", import.meta.url);
if (existsSync(cargoLock)) {
  const text = readFileSync(cargoLock, "utf8");
  for (const m of text.matchAll(/name = "(tauri(?:-build|-runtime|-utils|-codegen|-macros|-runtime-wry)?)"\nversion = "([^"]+)"/g)) {
    if (!m[2].startsWith("2.")) problems.push(`crate ${m[1]} is ${m[2]}`);
  }
}
if (problems.length) {
  console.error("Tauri must stay on 2.x stable:\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log("tauri: all 2.x");
