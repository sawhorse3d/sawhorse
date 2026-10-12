import { expect, test, type Page } from "@playwright/test";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { parseSkp, type Definition, type SkpModel } from "openskp";
import { IN, solids, type Solid } from "./helpers";

// Real SketchUp-authored files through Sawhorse's own .skp import (File > Import's input,
// src/io/skp.ts, the kernel worker), checked against what is in each file. The files are
// authored in SketchUp itself (or collected from openskp's own test files), are private, and
// are never in this repo: SAWHORSE_PRIVATE_FIXTURES names the directory that holds them (see
// docs/DEVELOPMENT.md), and the whole spec skips cleanly when it is unset or missing, so CI
// never depends on it.
//
// Part 1, three specific files at the top of the directory, each asserting what the importer
// does TODAY (the current, documented behaviour), told apart in every message as "FIXTURE"
// (the file is what it is described as: openskp's reading of it) or "IMPORT" (what Sawhorse
// made of it). Both are soft, so one run reports every discrepancy rather than the first.
//   box-slot-defective-current.skp  a defective model kept on purpose: a 4 x 3.109 in slot open
//                                   at the front, a capped mouth and a floating sliver (see
//                                   the test); one amber mesh, not a solid
//   round-slab-leg-current.skp      a 24-segment circle pulled 1 in (soft edges) and one "Leg"
//                                   component placed once, 2 x 2 x 28 in: a group today
//   blank-26-purged.skp             an empty model: refused cleanly today
//
// Part 2, every .skp anywhere under the directory (recursive): imported through the app with
// a generic check (no dialog, no page error, the report says what came in, the kernel still
// answers, undo and redo of the import round-trip to the same model hash) and one row of
// numbers each: what is in the file, what is in the model after import, whether an amber
// (not solid) mesh appeared, the import time. Rows print as "[private-fixtures:sweep] {...}",
// and one table of them at the end (also written to <captures>/private-fixtures-sweep.md).
//
// SAWHORSE_PRIVATE_CAPTURES (a directory; default test-results/private-fixtures) receives a
// screenshot of each imported model, named <SAWHORSE_PRIVATE_CAPTURE_PREFIX>private-fixture-
// <name>.png.
const FIXTURES = process.env.SAWHORSE_PRIVATE_FIXTURES;
const dir = FIXTURES && existsSync(FIXTURES) ? FIXTURES : null;
const capturesDir = process.env.SAWHORSE_PRIVATE_CAPTURES || path.join("test-results", "private-fixtures");
const capturePrefix = process.env.SAWHORSE_PRIVATE_CAPTURE_PREFIX ?? "";

const BOX = "box-slot-defective-current.skp";
const SLAB = "round-slab-leg-current.skp";
const BLANK = "blank-26-purged.skp";

const have = (file: string) => !!dir && existsSync(path.join(dir, file));
function model(file: string): SkpModel {
  const b = readFileSync(path.join(dir!, file));
  return parseSkp(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) as ArrayBuffer);
}

/** Every .skp under `root`, as paths relative to it with forward slashes, sorted. */
function skpFiles(root: string): string[] {
  const out: string[] = [];
  const walk = (rel: string) => {
    for (const e of readdirSync(path.join(root, rel), { withFileTypes: true })) {
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) walk(r);
      else if (/\.skp$/i.test(e.name)) out.push(r);
    }
  };
  walk("");
  return out.sort();
}

type Outcome = { kind: string; objects: number[]; meshes: number[]; groups: number[]; sketches: number[] };
type LastImport = { name: string; outcome: Outcome; notes: string[] };
type V = [number, number, number];

/**
 * Console errors and uncaught exceptions of the page: an import must not raise any. The
 * Credits chip polls the render service on 127.0.0.1:8787, which is not running here and
 * logs a refused connection; that is not the import's.
 */
function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error" && !/:8787(?![0-9])/.test(m.location().url)) errors.push(`console.error: ${m.text()} (${m.location().url})`);
  });
  return errors;
}

/** Native dialogs (alert, confirm) the page raises: recorded, then dismissed. None is expected. */
function watchDialogs(page: Page): string[] {
  const seen: string[] = [];
  page.on("dialog", (d) => {
    seen.push(`${d.type()}: ${d.message()}`);
    void d.dismiss();
  });
  return seen;
}

/** The app with the file input (?io=basic), no autosave, the kernel ready and empty. */
async function open(page: Page) {
  await page.goto("/?io=basic&autosave=0&theme=light");
  const vp = page.locator("main.vp");
  await expect(vp).toHaveAttribute("data-kernel", "ready", { timeout: 20_000 });
  await expect(vp).toHaveAttribute("data-objects", "0");
  return vp;
}

/**
 * Import through File > Import's input, exactly as the corpus spec does. Returns the
 * report's title and lines, and what the app recorded for the import (null when refused).
 */
async function importFile(page: Page, file: string) {
  const name = path.basename(file);
  await page.getByTestId("file-input").setInputFiles(path.join(dir!, file));
  const report = page.getByTestId("import-report");
  await expect(report).toContainText(name, { timeout: 60_000 });
  const title = ((await report.locator("header b").textContent()) ?? "").trim();
  const lines = await report.locator("li").allTextContents();
  const refused = /^Could not import /.test(title);
  await expect(page.locator("main.vp")).toHaveAttribute("data-kernel", "ready");
  const last = await page.evaluate(() => (window as unknown as { __sawhorse: { lastImport?: LastImport } }).__sawhorse.lastImport ?? null);
  return { title, lines, refused, last };
}

/** The HUD's model line: "2 solids · 32 faces · genus 0 · 123.4 in³ · watertight". */
async function hudLine(page: Page) {
  const text = await page.locator(".vp .hud").innerText();
  const m = /(\d+) solids? · (\d+) faces · genus (\d+) · ([\d.]+) in³ · (watertight|open)/.exec(text);
  return { text, solids: m ? +m[1] : -1, faces: m ? +m[2] : -1, genus: m ? +m[3] : -1, volume: m ? +m[4] : NaN, state: m ? m[5] : "" };
}

/** The Outliner's rows: key, kind, label and the muted hint. */
async function outline(page: Page) {
  await page.getByRole("tab", { name: "Outliner" }).click();
  return page.locator(".outline li").evaluateAll((lis) =>
    lis.map((li) => ({
      key: li.getAttribute("data-row") ?? "",
      kind: li.getAttribute("data-kind") ?? "",
      label: li.querySelector(".label")?.textContent ?? "",
      hint: li.querySelector(".k:not(.editing)")?.textContent ?? "",
    })),
  );
}

type MeshPart = { kind: string; owner: number; normal: V };
const parts = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { parts(): MeshPart[] } }).__sawhorse.parts());

/** Frame the model in the Iso view and save a screenshot of the whole window. */
async function capture(page: Page, name: string) {
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  mkdirSync(capturesDir, { recursive: true });
  const out = path.join(capturesDir, `${capturePrefix}private-fixture-${name}.png`);
  await page.screenshot({ path: out, animations: "disabled" });
  return out;
}
const stemOf = (file: string) => file.replace(/\.skp$/i, "");

/** |actual - expected| <= tolerance, as a soft assertion with both numbers in the message. */
const near = (what: string, actual: number, expected: number, tolerance: number) =>
  expect.soft(Math.abs(actual - expected), `${what}: ${actual} vs ${expected} (tolerance ${tolerance})`).toBeLessThanOrEqual(tolerance);

const extent = (vs: { x: number; y: number; z: number }[]) =>
  (["x", "y", "z"] as const).map((k) => {
    const c = vs.map((v) => v[k]);
    return { min: Math.min(...c), max: Math.max(...c), size: Math.max(...c) - Math.min(...c) };
  });

/**
 * What the file's own faces make, from the file alone. Edges are counted by the faces that
 * use them: a closed manifold shell has every edge used by exactly two (`open` counts the
 * edges used once, `over` those used by three or more). The volume (in^3) is the divergence
 * theorem over the faces' stored front normals, V = 1/3 sum area x (n . p), meaningful
 * when the shell is clean.
 */
function shellCheck(def: Definition) {
  const verts = new Map(def.vertices.map((v) => [v.id, v]));
  const edges = new Map(def.edges.map((e) => [e.id, e]));
  const start = (ce: Definition["faces"][number]["loops"][number][number]) => verts.get(ce.orientation === 1 ? edges.get(ce.edgeId)!.v1Id : edges.get(ce.edgeId)!.v2Id)!;
  const use = new Map<number, number>();
  for (const f of def.faces) for (const l of f.loops) for (const ce of l) use.set(ce.edgeId, (use.get(ce.edgeId) ?? 0) + 1);
  const area = (ring: { x: number; y: number; z: number }[]) => {
    let nx = 0;
    let ny = 0;
    let nz = 0;
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i];
      const b = ring[(i + 1) % ring.length];
      nx += (a.y - b.y) * (a.z + b.z);
      ny += (a.z - b.z) * (a.x + b.x);
      nz += (a.x - b.x) * (a.y + b.y);
    }
    return Math.hypot(nx, ny, nz) / 2;
  };
  let six = 0;
  for (const f of def.faces) {
    const rings = f.loops.map((l) => l.map(start));
    const a = area(rings[0]) - rings.slice(1).reduce((n, r) => n + area(r), 0);
    const p = rings[0][0];
    six += a * (f.normal[0] * p.x + f.normal[1] * p.y + f.normal[2] * p.z);
  }
  const counts = [...use.values()];
  return { volume: Math.abs(six) / 3, open: counts.filter((n) => n === 1).length, over: counts.filter((n) => n > 2).length };
}

/** The corners of the walls that are not on the outside of the box (a hole's or a notch's). */
function voidCorners(def: Definition, ex: { min: number; max: number }, ey: { min: number; max: number }) {
  const verts = new Map(def.vertices.map((v) => [v.id, v]));
  const edges = new Map(def.edges.map((e) => [e.id, e]));
  const ids = new Set<number>();
  for (const f of def.faces) {
    if (Math.abs(f.normal[2]) > 1e-6) continue;
    const e0 = edges.get(f.loops[0][0].edgeId)!;
    const p = verts.get(e0.v1Id)!;
    const [along, b] = f.normal[0] !== 0 ? [p.x, ex] : [p.y, ey];
    if (Math.abs(along - b.min) < 1e-6 || Math.abs(along - b.max) < 1e-6) continue;
    for (const ce of f.loops[0]) {
      ids.add(edges.get(ce.edgeId)!.v1Id);
      ids.add(edges.get(ce.edgeId)!.v2Id);
    }
  }
  return [...ids].map((id) => verts.get(id)!);
}

/** What a file holds, counted from openskp's reading of it (every definition once). */
function sourceCounts(m: SkpModel) {
  const defs = [m.root, ...m.definitions.values()];
  return {
    version: String(m.version),
    faces: defs.reduce((n, d) => n + d.faces.length, 0),
    edges: defs.reduce((n, d) => n + d.edges.length, 0),
    definitions: m.definitions.size,
    placements: defs.reduce((n, d) => n + d.instances.length, 0),
    materials: m.materials.length,
    layers: m.layers.length,
    scenes: m.pages.length,
  };
}

// ---- the model's own counts, read through the plugin API (model.read) ----------------------
// A session plugin (the same hook the plugin specs use) reports model.stats() and
// model.materials(): the kernel's counts after the import, in the documented shapes.
const PROBE_MANIFEST = JSON.stringify({
  id: "org.example.probe",
  name: "Probe",
  version: "1.0.0",
  api: 1,
  description: "reads counts",
  author: "Sawhorse",
  license: "MIT",
  permissions: ["model.read", "ui"],
  contributes: { commands: [{ id: "read", title: "Read" }] },
});
const PROBE_MAIN = `
import { model, ui } from "sawhorse";
await ui.registerCommand({ id: "read", async run() {
  console.log("counts=" + JSON.stringify({ stats: await model.stats(), materials: (await model.materials()).length, definitions: (await model.definitions()).length }));
} });
console.log("ready");
`;
type Counts = { stats: { solids: number; groups: number; components: number; instances: number; faces: number; edges: number; vertices: number; meshes: number; volume: number; watertight: boolean }; materials: number; definitions: number };
type PluginHook = { plugins: { list(): { id: string; logs: string[] }[]; loadSession(files: Record<string, string>): Promise<string>; enable(id: string): Promise<void>; commands(): string[]; run(key: string): Promise<void> } };

async function loadProbe(page: Page) {
  const id = await page.evaluate((files) => (window as unknown as { __sawhorse: PluginHook }).__sawhorse.plugins.loadSession(files), { "plugin.json": PROBE_MANIFEST, "main.js": PROBE_MAIN });
  await page.evaluate((i) => (window as unknown as { __sawhorse: PluginHook }).__sawhorse.plugins.enable(i), id);
  await expect.poll(() => page.evaluate(() => (window as unknown as { __sawhorse: PluginHook }).__sawhorse.plugins.commands())).toContain("org.example.probe/read");
}

async function readCounts(page: Page): Promise<Counts> {
  const logsOf = () => page.evaluate(() => (window as unknown as { __sawhorse: PluginHook }).__sawhorse.plugins.list().find((e) => e.id === "org.example.probe")!.logs.filter((l) => l.startsWith("log: counts=")));
  const before = (await logsOf()).length;
  await page.evaluate(() => (window as unknown as { __sawhorse: PluginHook }).__sawhorse.plugins.run("org.example.probe/read"));
  let line = "";
  await expect
    .poll(async () => {
      const all = await logsOf();
      line = all.length > before ? all[all.length - 1] : "";
      return line !== "";
    })
    .toBe(true);
  return JSON.parse(line.slice("log: counts=".length)) as Counts;
}

test.describe("private SketchUp fixtures (SAWHORSE_PRIVATE_FIXTURES)", () => {
  test.skip(!dir, "SAWHORSE_PRIVATE_FIXTURES is unset or not a directory: the private SketchUp files are not here");
  test.use({ viewport: { width: 1440, height: 900 } });

  // A defective model, kept on purpose. The 4 x 4 in rectangle of the intended "box with a
  // through-hole" was drawn straddling the front face, so the file holds a 4 x 3.109 in slot
  // through the full height, open at the front; a face capping the slot's mouth; and a
  // floating 4 x 0.891 in sliver at the top beyond the front face. Two edges (the slot
  // mouth's vertical ones) have three faces, four are open. The kernel never repairs
  // silently, so the whole piece stays ONE amber mesh of its 12 faces, with the defects named.
  //
  // A clean through-hole box is to be re-authored (12 x 12 x 6 in, a 4 x 4 in hole fully
  // inside the top face): when it is, add it as box-hole-current.skp and assert 1 solid, 768
  // in^3, 10 faces (the top and the bottom with a hole each, 4 outer and 4 inner walls),
  // genus 1, watertight. Those numbers were checked against a clean stand-in written with
  // openskp's writer (1 solid, 768.0 in^3, 10 faces, genus 1, and a Top-view ray down the
  // hole reaches the ground).
  test(`${BOX}: a defective model stays one amber mesh of 12 faces, its defects named`, async ({ page }, info) => {
    test.skip(!have(BOX), `${BOX} is not in the fixtures directory`);
    test.setTimeout(60_000);
    const errors = watchErrors(page);
    const dialogs = watchDialogs(page);

    // FIXTURE: what the file really is.
    const src = model(BOX);
    const [ex, ey, ez] = extent(src.root.vertices);
    expect.soft(src.root.faces, "FIXTURE: faces in the file").toHaveLength(12);
    expect.soft(src.root.instances, "FIXTURE: placements in the file").toHaveLength(0);
    near("FIXTURE: file extent x (in)", ex.size, 12, 1e-6);
    near("FIXTURE: file extent y (in): 12 for the box plus the 0.891 the sliver hangs out", ey.size, 12.891489, 1e-5);
    near("FIXTURE: file extent z (in)", ez.size, 6, 1e-6);
    expect.soft(
      src.root.faces.filter((f) => f.loops.length === 2),
      "FIXTURE: no face has a hole in it: the slot is open to the front",
    ).toHaveLength(0);
    const shell = shellCheck(src.root);
    expect.soft({ open: shell.open, over: shell.over }, "FIXTURE: edges used by one face (open), by three or more (over)").toEqual({ open: 4, over: 2 });
    const verts = new Map(src.root.vertices.map((v) => [v.id, v]));
    const edges = new Map(src.root.edges.map((e) => [e.id, e]));
    const front = ey.max - 12;
    const beyond = src.root.faces.filter((f) => f.loops[0].some((ce) => verts.get(edges.get(ce.edgeId)!.v1Id)!.y < front - 1e-6));
    expect.soft(beyond, "FIXTURE: one face lies beyond the box's front face (the sliver)").toHaveLength(1);
    const corners = voidCorners(src.root, ex, ey);
    const slot: V = corners.length ? [corners.reduce((n, v) => n + v.x, 0) / corners.length, corners.reduce((n, v) => n + v.y, 0) / corners.length, ez.max] : [(ex.min + ex.max) / 2, (ey.min + ey.max) / 2, ez.max];
    const margin: V = [ex.min + 1, ey.max - 1, ez.max];

    const vp = await open(page);
    const { title, lines, refused, last } = await importFile(page, BOX);

    // IMPORT: not refused, no solid, one amber mesh of 12 faces, the defects named.
    expect.soft(refused, `IMPORT: refused (${title})`).toBe(false);
    expect.soft(title, "IMPORT: the report's title").toBe("Imported box-slot-defective-current.skp: 0 solids, 1 mesh that is not solid.");
    expect.soft(last?.outcome.objects, "IMPORT: solids").toHaveLength(0);
    expect.soft(last?.outcome.meshes, "IMPORT: meshes that are not solid").toHaveLength(1);
    expect.soft(last?.outcome.groups, "IMPORT: groups or instances").toHaveLength(0);
    const report = lines.join("\n");
    expect.soft(report, "IMPORT: the report says why it is not a solid").toContain("kept as a mesh, not a solid");
    expect.soft(report, "IMPORT: the report counts the open edges").toContain("4 open edges");
    expect.soft(report, "IMPORT: the report counts the edges shared by three faces").toContain("2 edges shared by more than two faces");
    await expect.soft(vp).toHaveAttribute("data-objects", "0");
    expect.soft(await solids(page), "IMPORT: solids in the model").toHaveLength(0);
    const hud = await hudLine(page);
    expect.soft(hud.text, "IMPORT: the HUD names the amber mesh").toContain("1 mesh not solid (imported)");
    await expect.soft(page.locator(".vp .hud .err", { hasText: "1 mesh not solid (imported)" }), "IMPORT: and shows it in the error colour").toHaveCount(1);
    const meshFaces = (await parts(page)).filter((p) => p.kind === "mesh");
    expect.soft(meshFaces, "IMPORT: faces of the mesh: all 12, none lost or merged").toHaveLength(12);
    const rows = await outline(page);
    expect.soft(rows, "IMPORT: the Outliner lists one mesh").toEqual([{ key: expect.stringMatching(/^m:/), kind: "mesh", label: "box-slot-defective-current", hint: "not a solid" }]);

    // IMPORT: the slot is visible. Looking down the Top view, the ray through the slot's
    // centre passes through the model to the ground; the ray through the solid margin of the
    // top hits a face facing up.
    await page.getByRole("button", { name: "Top", exact: true }).click();
    await expect(vp).toHaveAttribute("data-view", "top");
    await page.keyboard.press("Shift+Z");
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const pickAt = (p: V) =>
      page.evaluate(
        ([m]) => {
          const api = (window as unknown as { __sawhorse: { worldToScreen(p: number[]): [number, number]; pick(x: number, y: number): { part: number } | null; parts(): { normal: number[] }[] } }).__sawhorse;
          const [x, y] = api.worldToScreen(m);
          const hit = api.pick(x, y);
          return { part: hit?.part ?? null, normal: hit && hit.part >= 0 ? api.parts()[hit.part].normal : null };
        },
        [p.map((c) => c * IN)] as const,
      );
    const through = await pickAt(slot);
    expect.soft(through.part, `IMPORT: the ray down the slot (${slot.map((c) => c.toFixed(3))}) reaches the ground, not the model`).toBe(-1);
    const top = await pickAt(margin);
    expect.soft(top.part, "IMPORT: the ray through the solid margin hits the model").toBeGreaterThanOrEqual(0);
    expect.soft(top.normal?.[2] ?? 0, "IMPORT: and it is a face facing up").toBeGreaterThan(0.999);

    const shot = await capture(page, stemOf(BOX));
    const observed = {
      file: { extent: [ex.size, ey.size, ez.size], faces: src.root.faces.length, edgesUsedOnce: shell.open, edgesUsedByMoreThanTwo: shell.over, facesBeyondFront: beyond.length },
      imported: { title, lines, meshes: last?.outcome.meshes.length, meshFaces: meshFaces.length, hud: hud.text.replace(/\n+/g, " | ") },
      pick: { through, top },
    };
    console.log(`[private-fixtures] ${BOX}: ${JSON.stringify(observed)}`);
    await info.attach("observed", { contentType: "application/json", body: JSON.stringify(observed, null, 2) });
    info.annotations.push({ type: "capture", description: shot });
    expect.soft(dialogs, "no dialogs").toEqual([]);
    expect.soft(errors, "no page errors or console errors").toEqual([]);
  });

  test(`${SLAB}: a 24-sided slab and the Leg (a group today), 2 x 2 x 28 in the Cut list`, async ({ page }, info) => {
    test.skip(!have(SLAB), `${SLAB} is not in the fixtures directory`);
    test.setTimeout(60_000);
    const errors = watchErrors(page);
    const dialogs = watchDialogs(page);

    // FIXTURE: a 24-gon prism on the root, one "Leg" definition placed once.
    const src = model(SLAB);
    expect.soft(src.root.faces, "FIXTURE: root faces").toHaveLength(26);
    expect.soft(src.root.instances, "FIXTURE: placements").toHaveLength(1);
    const leg = src.definitions.get(src.root.instances[0].refIdx)!;
    expect.soft(leg.name, "FIXTURE: the placed definition").toBe("Leg");
    expect.soft(leg.faces, "FIXTURE: the Leg's faces").toHaveLength(6);
    expect.soft(
      src.root.edges.filter((e) => e.soft || e.smooth),
      "FIXTURE: the circle's soft edges",
    ).toHaveLength(24);
    const zmin = Math.min(...src.root.vertices.map((v) => v.z));
    const zmax = Math.max(...src.root.vertices.map((v) => v.z));
    const ring = src.root.vertices.filter((v) => Math.abs(v.z - zmin) < 1e-6);
    expect.soft(ring, "FIXTURE: corners of the circle").toHaveLength(24);
    near("FIXTURE: slab thickness (in)", zmax - zmin, 1, 1e-6);
    // The circle's radius: its corners all lie on it. The area its 24 straight sides
    // enclose (shoelace, corners in order round the centre) times the thickness is the
    // exact volume; pi r^2 x 1 is the true-circle volume a 24-gon falls 1.14% short of.
    const cx = ring.reduce((a, v) => a + v.x, 0) / ring.length;
    const cy = ring.reduce((a, v) => a + v.y, 0) / ring.length;
    const radii = ring.map((v) => Math.hypot(v.x - cx, v.y - cy));
    const r = radii.reduce((a, b) => a + b, 0) / radii.length;
    near("FIXTURE: all 24 corners lie on one circle (in)", Math.max(...radii), Math.min(...radii), 1e-6);
    const ordered = [...ring].sort((a, b) => Math.atan2(a.y - cy, a.x - cx) - Math.atan2(b.y - cy, b.x - cx));
    let area = 0;
    for (let i = 0; i < ordered.length; i++) {
      const a = ordered[i];
      const b = ordered[(i + 1) % ordered.length];
      area += a.x * b.y - b.x * a.y;
    }
    const polygon = (Math.abs(area) / 2) * (zmax - zmin);
    const circle = Math.PI * r * r * 1;
    const [lx, ly, lz] = extent(leg.vertices);
    for (const [k, e] of [lx, ly, lz].entries()) near(`FIXTURE: Leg size ${"xyz"[k]} in the file (in)`, e.size, [2, 2, 28][k], 1e-6);

    const vp = await open(page);
    const { title, refused, last } = await importFile(page, SLAB);

    // IMPORT: the slab and the Leg: two solids, both watertight, nothing left as a mesh.
    expect.soft(refused, `IMPORT: refused (${title})`).toBe(false);
    expect.soft(title, "IMPORT: the report's title").toMatch(/^Imported round-slab-leg-current\.skp: 2 solids, /);
    expect.soft(last?.outcome.objects, "IMPORT: solids").toHaveLength(2);
    expect.soft(last?.outcome.meshes, "IMPORT: meshes that are not solid").toHaveLength(0);
    expect.soft(last?.outcome.groups, "IMPORT: top-level group or instance (the Leg)").toHaveLength(1);
    await expect.soft(vp).toHaveAttribute("data-objects", "2");
    await expect.soft(vp).toHaveAttribute("data-watertight", "true");

    // IMPORT: the slab's volume is the 24-gon prism exactly, and within 1.2% of pi r^2 x 1.
    const all = await solids(page);
    expect.soft(all, "IMPORT: solids in the model").toHaveLength(2);
    const slab = all.reduce((a, b) => (Math.abs(a.volume - polygon) <= Math.abs(b.volume - polygon) ? a : b));
    const legSolid = all.find((x) => x !== slab) as Solid | undefined;
    near("IMPORT: slab volume against the 24-gon prism in the file (ratio)", slab.volume / polygon, 1, 1e-6);
    near("IMPORT: slab volume against pi r^2 x 1 (ratio; a 24-gon is 1.14% short of its circle)", slab.volume / circle, 1, 0.012);
    for (let k = 0; k < 3; k++) near(`IMPORT: slab bounds ${"xyz"[k]} (in)`, slab.max[k] - slab.min[k], [2 * r, 2 * r, 1][k], k < 2 ? 2 * r * 0.0115 : 1e-6);
    if (legSolid) {
      near("IMPORT: Leg volume (in^3)", legSolid.volume, 2 * 2 * 28, 112 * 1e-6);
      for (let k = 0; k < 3; k++) near(`IMPORT: Leg bounds ${"xyz"[k]} (in)`, legSolid.max[k] - legSolid.min[k], [2, 2, 28][k], 1e-6);
    }

    // IMPORT: the circle's 24 facets all came in: 24 walls and 2 caps on the slab, the Leg's 6.
    const hud = await hudLine(page);
    expect.soft(hud.solids, "IMPORT: HUD solids").toBe(2);
    expect.soft(hud.faces, "IMPORT: 26 faces on the slab + 6 on the Leg").toBe(32);
    expect.soft(hud.genus, "IMPORT: genus").toBe(0);
    expect.soft(hud.state).toBe("watertight");
    const slabFaces = (await parts(page)).filter((p) => p.kind === "face" && p.owner === slab.id);
    expect.soft(slabFaces.filter((p) => Math.abs(p.normal[2]) < 1e-6), "IMPORT: the circle's 24 side faces").toHaveLength(24);
    expect.soft(slabFaces.filter((p) => Math.abs(p.normal[2]) > 0.999), "IMPORT: top and bottom").toHaveLength(2);

    // IMPORT: the Leg in the Outliner is a GROUP named "Leg" today: a definition placed once
    // comes in as a group (docs/skp-compat.md, "Group or component?"), though SketchUp made
    // it a component. Lane 15b will change the importer so a placed-once definition with a
    // user-given name (not SketchUp's own `Group#N`) becomes a component instance; when it
    // lands, flip these expectations to kind "instance", a hint matching /^component/, and a
    // report title ending "1 component instance (1 component)." instead of "1 group.".
    // The Cut list row below does not change: a group of solids is a part, quantity 1.
    const rows = await outline(page);
    const legRow = rows.find((x) => x.label === "Leg" && (x.kind === "instance" || x.kind === "group"));
    expect.soft(legRow, `IMPORT: a "Leg" node in the Outliner: ${JSON.stringify(rows)}`).toBeTruthy();
    expect.soft(legRow?.kind, `IMPORT: the Leg is a group in the Outliner today (row: ${JSON.stringify(legRow)})`).toBe("group");
    expect.soft(legRow?.hint ?? "", "IMPORT: the Outliner's hint for the Leg").toBe("group");
    expect.soft(title, "IMPORT: the report's title counts one group").toBe("Imported round-slab-leg-current.skp: 2 solids, 1 group.");

    // IMPORT: the Cut list lists a part 2 x 2 x 28 in, quantity 1.
    await page.getByRole("tab", { name: "Cut list" }).click();
    await expect(page.locator(".cutlist table")).toBeVisible();
    const cut = await page.evaluate(() => (window as unknown as { __sawhorse: { cutList(): Promise<{ rows: { name: string; qty: number; size: number[]; steps: number[]; shaped: boolean }[] }> } }).__sawhorse.cutList());
    const legPart = cut.rows.find((x) => x.name === "Leg");
    expect.soft(legPart, `IMPORT: a "Leg" part in the cut list: ${JSON.stringify(cut.rows.map((x) => x.name))}`).toBeTruthy();
    if (legPart) {
      expect.soft(legPart.qty, "IMPORT: Leg quantity").toBe(1);
      for (let k = 0; k < 3; k++) near(`IMPORT: Leg part size ${["T", "W", "L"][k]} (in)`, legPart.size[k] / IN, [2, 2, 28][k], 1e-6);
      expect.soft(legPart.steps, "IMPORT: sixteenths: 2, 2, 28 in").toEqual([32, 32, 448]);
      const cells = await page
        .locator(".cutlist tbody tr", { hasText: "Leg" })
        .first()
        .evaluate((tr) => Array.from(tr.querySelectorAll("td")).map((td) => (td.querySelector(".name") ?? td).textContent ?? ""));
      expect.soft(cells.slice(0, 5), "IMPORT: the Leg's row in the pane: Part, Qty, T, W, L").toEqual(["Leg", "1", "2", "2", "28"]);
    }
    const shotCut = await capture(page, `${stemOf(SLAB)}-cutlist`);
    await page.getByRole("tab", { name: "Outliner" }).click();
    const shot = await capture(page, stemOf(SLAB));
    const observed = {
      file: { r, polygon, circle, polygonOverCircle: polygon / circle },
      imported: { title, lines: last?.notes ?? [], solids: all.map((x) => ({ number: x.number, volume: x.volume, min: x.min, max: x.max })), hud: hud.text.replace(/\n+/g, " | ") },
      slabOverCircle: slab.volume / circle,
      outliner: rows,
      cutlist: cut.rows.map((x) => ({ name: x.name, qty: x.qty, steps: x.steps, shaped: x.shaped })),
    };
    console.log(`[private-fixtures] ${SLAB}: ${JSON.stringify(observed)}`);
    await info.attach("observed", { contentType: "application/json", body: JSON.stringify(observed, null, 2) });
    info.annotations.push({ type: "capture", description: `${shot}; ${shotCut}` });
    expect.soft(dialogs, "no dialogs").toEqual([]);
    expect.soft(errors, "no page errors or console errors").toEqual([]);
  });

  // An empty model is REFUSED today, cleanly, in plain words, and the document stays as it
  // was (the kernel's NothingImported, crates/kernel/tests/import.rs). Lane 15b will accept
  // empty models and open an empty document with the file's units; when it lands, flip the
  // expectations below to "Imported blank-26-purged.skp: 0 solids.", a report that is not an
  // error, and the document's units taken from the file (inches).
  test(`${BLANK}: an empty model is refused cleanly, the document untouched`, async ({ page }, info) => {
    test.skip(!have(BLANK), `${BLANK} is not in the fixtures directory`);
    test.setTimeout(60_000);
    const errors = watchErrors(page);
    const dialogs = watchDialogs(page);
    const src = model(BLANK);
    expect.soft(src.root.faces, "FIXTURE: faces").toHaveLength(0);
    expect.soft(src.root.instances, "FIXTURE: placements").toHaveLength(0);

    const vp = await open(page);
    const { title, lines, refused, last } = await importFile(page, BLANK);

    // IMPORT: refused with the plain message, as an error report, recording no import.
    expect.soft(refused, "IMPORT: refused").toBe(true);
    expect.soft(title, "IMPORT: the report's title").toBe("Could not import blank-26-purged.skp");
    expect.soft(lines, "IMPORT: the report's reason").toEqual(["the file has nothing Sawhorse can import"]);
    await expect.soft(page.getByTestId("import-report"), "IMPORT: the report is an error report").toHaveClass(/err/);
    expect.soft(last, "IMPORT: no import recorded").toBeNull();
    // The document is untouched and the app is still healthy.
    await expect(vp).toHaveAttribute("data-objects", "0");
    expect(await solids(page)).toHaveLength(0);
    expect(await outline(page), "an empty Outliner").toEqual([]);
    await expect(page.getByTestId("outliner")).toContainText("Nothing yet");
    await expect(page.locator(".vp .hud")).not.toContainText("not solid");
    await expect(page.locator(".vp .hud")).toContainText("undo 0");
    await expect(vp).toHaveAttribute("data-kernel", "ready");

    const shot = await capture(page, stemOf(BLANK));
    const observed = { title, lines, refused, recordedAsImport: last !== null, hud: (await page.locator(".vp .hud").innerText()).replace(/\n+/g, " | ") };
    console.log(`[private-fixtures] ${BLANK}: ${JSON.stringify(observed)}`);
    await info.attach("observed", { contentType: "application/json", body: JSON.stringify(observed, null, 2) });
    info.annotations.push({ type: "capture", description: shot });
    expect.soft(dialogs, "no dialogs").toEqual([]);
    expect.soft(errors, "no page errors or console errors").toEqual([]);
  });
});

// ---- part 2: every .skp under the directory ------------------------------------------------

const SWEEP = dir ? skpFiles(dir) : [];
type SweepRow = {
  file: string;
  version: string;
  outcome: string;
  ms: number;
  file_counts: { faces?: number; edges?: number };
  after_import: { solids: number; groups: number; instances: number; materials: number; solid_faces: number; solid_edges: number; mesh_count: number; mesh_faces: number };
  amber: boolean;
  undo_redo: string;
};
const sweepRows: SweepRow[] = [];

/** The sweep as a Markdown table: what each file holds, what the model holds after import. */
function sweepTable(rows: SweepRow[]): string {
  const head = ["file", "SketchUp", "import", "ms", "file faces/edges", "solids", "groups", "instances", "materials", "solid faces/edges", "meshes (faces)", "amber", "undo/redo"];
  const body = rows.map((r) => {
    const a = r.after_import;
    const f = r.file_counts;
    return [
      r.file,
      r.version,
      r.outcome.startsWith("refused") ? r.outcome.replace(/\|/g, "/").replace(/\s+/g, " ").slice(0, 70) : r.outcome,
      String(r.ms),
      f.faces === undefined ? "-" : `${f.faces}/${f.edges}`,
      String(a.solids),
      String(a.groups),
      String(a.instances),
      String(a.materials),
      `${a.solid_faces}/${a.solid_edges}`,
      `${a.mesh_count} (${a.mesh_faces})`,
      r.amber ? "yes" : "no",
      r.undo_redo,
    ];
  });
  return [head, head.map(() => "---"), ...body].map((r) => `| ${r.join(" | ")} |`).join("\n");
}

test.describe("private SketchUp fixtures: every .skp under the directory", () => {
  test.skip(!dir, "SAWHORSE_PRIVATE_FIXTURES is unset or not a directory: the private SketchUp files are not here");
  test.use({ viewport: { width: 1440, height: 900 } });

  test.afterAll(() => {
    if (!sweepRows.length) return;
    const table = sweepTable([...sweepRows].sort((a, b) => a.file.localeCompare(b.file)));
    console.log(`[private-fixtures:sweep] ${sweepRows.length} files\n${table}`);
    mkdirSync(capturesDir, { recursive: true });
    writeFileSync(path.join(capturesDir, `${capturePrefix}private-fixtures-sweep.md`), `${table}\n`);
  });

  for (const rel of SWEEP) {
    test(`sweep: ${rel}`, async ({ page }, info) => {
      test.setTimeout(180_000);
      const errors = watchErrors(page);
      const dialogs = watchDialogs(page);

      // What the file holds, by openskp. A file openskp cannot read is the app's to refuse
      // cleanly, so the sweep records that rather than failing on it here.
      let src: ReturnType<typeof sourceCounts> | null = null;
      let parseError = "";
      try {
        src = sourceCounts(model(rel));
      } catch (e) {
        parseError = e instanceof Error ? e.message : String(e);
      }
      const hasGeometry = !!src && (src.faces > 0 || src.edges > 0 || src.placements > 0);

      const vp = await open(page);
      await loadProbe(page);
      const t0 = Date.now();
      const { title, lines, refused, last } = await importFile(page, rel);
      const ms = Date.now() - t0;

      // The report says what came in (or why nothing did).
      expect.soft(title, "the import report names the file and what came in").toMatch(/^(Imported .+: \d+ solids?|Could not import .+)/);
      if (refused) {
        expect.soft(lines.length, "a refusal says why").toBeGreaterThan(0);
        if (hasGeometry) expect.soft(refused, `IMPORT: refused a file that has geometry (${title}: ${lines.join("; ")})`).toBe(false);
        if (!src) expect.soft(lines.join(" "), "a file openskp cannot read is refused in plain words").toMatch(/could not read the SketchUp file/);
      } else {
        expect.soft(parseError, "openskp read the file the app imported").toBe("");
      }

      // The kernel is still answering: a cut list round trip, and undo then redo of the import
      // comes back to the same model hash.
      await expect(vp).toHaveAttribute("data-kernel", "ready");
      const answered = await page.evaluate(() =>
        Promise.race([
          (window as unknown as { __sawhorse: { cutList(): Promise<unknown> } }).__sawhorse.cutList().then(() => "ok"),
          new Promise<string>((res) => setTimeout(() => res("no answer in 15 s"), 15_000)),
        ]),
      );
      expect.soft(answered, "the kernel answers a cut list request after the import").toBe("ok");
      const hash = await vp.getAttribute("data-hash");
      let undoRedo = "n/a (refused)";
      if (!refused) {
        await page.keyboard.press("Control+z");
        await expect.soft(vp, "Ctrl+Z takes the import back").toHaveAttribute("data-can-redo", "true", { timeout: 15_000 });
        await expect.soft(vp, "and leaves no solids").toHaveAttribute("data-objects", "0");
        await page.keyboard.press("Control+y");
        await expect.soft(vp, "Ctrl+Y puts it back").toHaveAttribute("data-can-redo", "false", { timeout: 15_000 });
        await expect.soft(vp, "to the same model hash").toHaveAttribute("data-hash", hash ?? "");
        undoRedo = (await vp.getAttribute("data-hash")) === hash ? "ok" : "hash changed";
      }

      // What came in, counted after the import (the kernel's own numbers through model.read).
      const counts = await readCounts(page);
      const hud = await hudLine(page);
      const hudText = (await page.locator(".vp .hud").innerText()).replace(/\n+/g, " | ");
      const meshParts = (await parts(page)).filter((p) => p.kind === "mesh");
      const meshes = last?.outcome.meshes.length ?? 0;
      expect.soft(counts.stats.meshes, "model.stats meshes match the import outcome").toBe(meshes);
      const shot = await capture(page, `sweep-${stemOf(rel).replace(/[\\/]/g, "-")}`);

      const row = {
        file: rel,
        bytes: statSync(path.join(dir!, rel)).size,
        version: src?.version ?? "unreadable",
        outcome: refused ? `refused: ${lines.join("; ")}` : "imported",
        ms,
        file_counts: src ?? { error: parseError },
        after_import: {
          solids: counts.stats.solids,
          groups: counts.stats.groups,
          instances: counts.stats.instances,
          components: counts.stats.components,
          materials: counts.materials,
          solid_faces: counts.stats.faces,
          solid_edges: counts.stats.edges,
          solid_vertices: counts.stats.vertices,
          mesh_count: counts.stats.meshes,
          mesh_faces: meshParts.length,
          faces_total: counts.stats.faces + meshParts.length,
          watertight: counts.stats.watertight,
          volume_in3: counts.stats.volume / IN ** 3,
        },
        amber: meshes > 0 || /not solid/.test(hudText),
        title,
        notes: [...lines].slice(0, 12),
        hud: hudText,
        undo_redo: undoRedo,
        dialogs,
        errors,
        capture: shot,
      };
      console.log(`[private-fixtures:sweep] ${JSON.stringify(row)}`);
      sweepRows.push(row as SweepRow);
      await info.attach("row", { contentType: "application/json", body: JSON.stringify(row, null, 2) });
      info.annotations.push({ type: "capture", description: shot });
      expect.soft(hud.state === "" || hud.state === "watertight" || meshes > 0 || refused, "every solid in the model is watertight").toBe(true);
      expect.soft(dialogs, "no dialogs").toEqual([]);
      expect.soft(errors, "no page errors or console errors").toEqual([]);
    });
  }
});
