import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import path from "node:path";
import { afterAll, describe, expect, it } from "vitest";

// scripts/wasm-checkout.mjs (the 2026-10-12 stale-kernel trap): src/wasm/pkg must hold the
// kernel THIS checkout built. Run in a node child process, as the build scripts run it.
const script = path.resolve("scripts", "wasm-checkout.mjs");
const run = (code: string): unknown => JSON.parse(execFileSync(process.execPath, ["--input-type=module", "-e", `import * as w from ${JSON.stringify(pathToFileURL(script).href)};\n${code}`], { encoding: "utf8" }));

const roots: string[] = [];
afterAll(() => roots.forEach((r) => rmSync(r, { recursive: true, force: true })));

/** A checkout whose src/wasm/pkg holds a stand-in kernel stamped `stamp` (null: no stamp). */
function fakeCheckout(stamp: string | null): string {
  const root = mkdtempSync(path.join(tmpdir(), "sawhorse-checkout-"));
  roots.push(root);
  const pkg = path.join(root, "src", "wasm", "pkg");
  mkdirSync(pkg, { recursive: true });
  writeFileSync(path.join(pkg, "sawhorse_wasm.js"), `export function initSync() {}\n${stamp === null ? "" : `export function build_checkout() { return ${JSON.stringify(stamp)}; }\n`}`);
  writeFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm"), "");
  return root;
}

describe("wasm-checkout", () => {
  it("hashes like the kernel: FNV-1a 64, the published vectors", () => {
    // The same three vectors crates/wasm-api/src/lib.rs checks.
    expect(run(`console.log(JSON.stringify(["", "a", "foobar"].map(w.fnv1a)));`)).toEqual(["cbf29ce484222325", "af63dc4c8601ec8c", "85944171f73967e8"]);
  });

  it("knows every way cargo may spell this checkout's crate dir", () => {
    const root = path.resolve(".");
    const stamps = run(`console.log(JSON.stringify([...w.checkoutStamps(${JSON.stringify(root)})]));`) as string[];
    const native = path.join(root, "crates", "wasm-api");
    const want = run(`console.log(JSON.stringify([w.fnv1a(${JSON.stringify(native)}), w.toWslPath(${JSON.stringify(native)}) && w.fnv1a(w.toWslPath(${JSON.stringify(native)}))]));`) as (string | null)[];
    expect(stamps).toContain(want[0]);
    if (want[1]) expect(stamps).toContain(want[1]);
    expect(run(`console.log(JSON.stringify(w.toWslPath("C:\\\\Users\\\\x\\\\app")));`)).toBe("/mnt/c/Users/x/app");
  });

  it("passes a kernel built here and catches one built elsewhere, an old one, or none", () => {
    const check = (root: string) => run(`console.log(JSON.stringify(await w.checkPkg(${JSON.stringify(root)})));`) as { ok: boolean; reason: string; missing?: boolean };
    const here = fakeCheckout("");
    const stamp = run(`console.log(JSON.stringify(w.fnv1a(${JSON.stringify(path.join(here, "crates", "wasm-api"))})));`) as string;
    writeFileSync(path.join(here, "src", "wasm", "pkg", "sawhorse_wasm.js"), `export function initSync() {}\nexport function build_checkout() { return ${JSON.stringify(stamp)}; }\n`);
    expect(check(here)).toMatchObject({ ok: true });
    const elsewhere = check(fakeCheckout("0123456789abcdef"));
    expect(elsewhere.ok).toBe(false);
    expect(elsewhere.reason).toContain("built by another checkout");
    const old = check(fakeCheckout(null));
    expect(old.ok).toBe(false);
    expect(old.reason).toContain("older than this checkout's");
    const none = mkdtempSync(path.join(tmpdir(), "sawhorse-checkout-"));
    roots.push(none);
    expect(check(none)).toMatchObject({ ok: false, missing: true });
  });
});
