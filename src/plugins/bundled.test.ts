// The sample plugins as the app bundles them, and the plugin:check / plugin:pack scripts.
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { bundledPlugins } from "./bundled";
import { loadPackage } from "./loader";
import { readPluginZip } from "./zip";

const root = path.resolve(__dirname, "../..");
const node = (args: string[]) => spawnSync(process.execPath, args, { cwd: root, encoding: "utf8" });

describe("bundled samples", () => {
  const all = bundledPlugins();

  it("are the three samples, each valid, MIT, by Sawhorse, with a README and a licence", () => {
    expect(Object.keys(all).sort()).toEqual(["cabinet-generator", "model-report", "shelf-pin-holes"]);
    for (const [dir, files] of Object.entries(all)) {
      const r = loadPackage(files, "bundled");
      expect(r.ok, `${dir}: ${r.ok ? "" : r.errors.join("; ")}`).toBe(true);
      if (!r.ok) continue;
      expect(r.pkg.manifest).toMatchObject({ author: "Sawhorse", license: "MIT", api: 1, id: `com.sawhorse3d.${dir}` });
      expect(files["README.md"]).toContain(`# ${r.pkg.manifest.name}`);
      expect(files["LICENSE"]).toContain("MIT License");
      expect(files["main.js"].startsWith("// @ts-check")).toBe(true);
    }
  });

  it("ask only for what they use", () => {
    const perms = Object.fromEntries(Object.entries(all).map(([d, f]) => [d, JSON.parse(f["plugin.json"]).permissions]));
    expect(perms["model-report"]).not.toContain("model.write");
    expect(perms["cabinet-generator"]).toEqual(["model.read", "model.write", "ui", "storage"]);
    for (const p of Object.values(perms)) expect(p).not.toContain("net");
  });
});

describe("plugin:check and plugin:pack", () => {
  it("check every sample (manifests, sources, types)", () => {
    const r = node(["scripts/plugin-check.mjs"]);
    expect(r.status, r.stdout + r.stderr).toBe(0);
    expect(r.stdout).toContain("3 plugins clean");
    expect(r.stdout).toContain("ok   type check");
  });

  it("refuse a broken plugin and pack a good one the same way twice", () => {
    const dir = mkdtempSync(path.join(tmpdir(), "sawhorse-plugin-"));
    try {
      const bad = path.join(dir, "bad");
      mkdirSync(bad);
      writeFileSync(path.join(bad, "plugin.json"), JSON.stringify({ id: "org.example.bad", name: "Bad", version: "1", api: 1, description: "", author: "x", license: "MIT", permissions: [] }));
      writeFileSync(path.join(bad, "main.js"), 'import("https://example.com/x.js");');
      const r = node(["scripts/plugin-check.mjs", bad]);
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("semantic version");
      const pack = node(["scripts/plugin-pack.mjs", bad]);
      expect(pack.status).toBe(1);
      expect(pack.stderr).toContain("not packed");

      const out = (n: string) => path.join(dir, n);
      expect(node(["scripts/plugin-pack.mjs", "plugins/shelf-pin-holes", "--out", out("a.zip")]).status).toBe(0);
      expect(node(["scripts/plugin-pack.mjs", "plugins/shelf-pin-holes", "--out", out("b.zip")]).status).toBe(0);
      const a = readFileSync(out("a.zip"));
      expect(Buffer.compare(a, readFileSync(out("b.zip")))).toBe(0);
      // What Install from .zip reads back.
      const files = readPluginZip(new Uint8Array(a)).files;
      expect(Object.keys(files).sort()).toEqual(["LICENSE", "README.md", "main.js", "plugin.json"]);
      expect(loadPackage(files, "installed").ok).toBe(true);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 60_000);

  // The verifier wrote a plugin outside plugins/ from the docs; plugin:check skipped its
  // type check there. Now a folder anywhere has its // @ts-check modules checked.
  it("type-check a plugin folder outside plugins/, and pass the guide's first plugin", () => {
    const dir = mkdtempSync(path.join(tmpdir(), "sawhorse-plugin-"));
    try {
      const manifest = { id: "org.example.hello-box", name: "Hello box", version: "1.0.0", api: 1, description: "Puts a 12 in cube at the origin.", author: "You", license: "MIT", permissions: ["model.write", "ui"], contributes: { commands: [{ id: "cube", title: "Add a Cube" }], menus: [{ command: "cube" }] }, entry: "main.js" };
      const good = `// @ts-check
import { model, ui } from "sawhorse";

await ui.registerCommand({
  id: "cube",
  async run() {
    const b = model.batch("Add a cube");
    b.box({ min: [0, 0, 0], size: [0.3048, 0.3048, 0.3048], name: "Cube" });
    await b.commit();
    await ui.notify("A 12 in cube.");
  },
});
`;
      for (const [name, main] of [
        ["hello-box", good],
        ["typo-box", good.replace("[0.3048, 0.3048, 0.3048]", '"big"')],
      ] as const) {
        mkdirSync(path.join(dir, name));
        writeFileSync(path.join(dir, name, "plugin.json"), JSON.stringify(manifest));
        writeFileSync(path.join(dir, name, "main.js"), main);
      }
      const ok = node(["scripts/plugin-check.mjs", path.join(dir, "hello-box")]);
      expect(ok.status, ok.stdout + ok.stderr).toBe(0);
      expect(ok.stdout).toContain("ok   type check");
      // Outside plugins/ a missing README or LICENSE is a warning, not a failure.
      expect(ok.stdout).toContain("warning: README.md is missing");
      const typo = node(["scripts/plugin-check.mjs", path.join(dir, "typo-box")]);
      expect(typo.status).toBe(1);
      expect(typo.stdout).toContain("FAIL type check");
      expect(typo.stdout).toContain("Type 'string' is not assignable to type 'Vec3'");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 60_000);
});
