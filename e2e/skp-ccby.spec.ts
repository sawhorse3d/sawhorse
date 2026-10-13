import { expect, test, type Page } from "@playwright/test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";

// Public SketchUp-authored fixtures (lane 15): five furniture models by Corynth (CC BY 4.0,
// unmodified; tests/fixtures/skp-ccby/ATTRIBUTION.md), the kind of hand-made furniture most
// woodworkers have: groups and components of open faces, not watertight solids. Each is
// imported through File > Import's input with no dialog and no page error, and the Cut list
// lists its parts, flagged "not solid (sizes from bounds)" where they are open; benches.skp
// (all loose faces) gets its parts from "Make parts from loose faces". The numbers per file
// are what this build makes of it, so a change in the importer or the cut list shows here.
const dir = path.join("tests", "fixtures", "skp-ccby");

// parts / notSolid: the Cut list's parts and how many are flagged (benches: after Make parts
// from loose faces). meshes: amber meshes the import makes.
// uncounted: "name:why" of parts listed but counted nowhere (lane 15 fixes); sheets: 4x8s.
const FILES: { file: string; sha256: string; parts: number; notSolid: number; meshes: number; loose?: boolean; uncounted: string[]; sheets: number }[] = [
  { file: "benches.skp", sha256: "c9f1cc7486aacf897e3d9d44107954bad0d6b4ba20012192e47a4b8cddacc994", parts: 3, notSolid: 3, meshes: 1, loose: true, uncounted: ["benches 1:not_a_board", "benches 2:not_a_board", "benches 3:not_a_board"], sheets: 0 },
  { file: "coffee tables.skp", sha256: "91d7321550b78147cda25b436275e3ae1ac60ae39bf52b7c6f1b8def7ef0359d", parts: 10, notSolid: 9, meshes: 10, uncounted: ["coffee table 1:not_a_board", "coffee table 2:not_a_board", "coffee table 3:not_a_board", "coffee table 4:not_a_board", "coffee table 5:not_a_board", "coffee table 7:not_a_board", "coffee table 8:not_a_board", "coffee table 9:not_a_board", "coffee table 10:not_a_board"], sheets: 0 },
  { file: "cupboard.skp", sha256: "63431b71863688994d987e9dc90f7d369df336ecafc9a7cca2db4018432c372e", parts: 4, notSolid: 4, meshes: 8, uncounted: ["Group:not_a_board", "Rodeo#2:flat"], sheets: 0 },
  { file: "desk.skp", sha256: "6d370f9ce838b5a0c0abde9271cec7e7850f895f88a8438e8d5cbd50ce909301", parts: 1, notSolid: 1, meshes: 1, uncounted: ["desk 1:not_a_board"], sheets: 0 },
  { file: "side table.skp", sha256: "eddaf9709e8027e419207d5507a7bba3085b9c5f1dd4f322b46489aa4c28bf99", parts: 1, notSolid: 1, meshes: 1, uncounted: ["side table 2:not_a_board"], sheets: 0 },
];

type CutRow = { name: string; qty: number; not_solid: boolean; size: number[]; uncounted?: "flat" | "not_a_board"; board_feet: number; sheet: boolean };
type Cut = {
  rows: CutRow[];
  totals: { parts: number; not_solid: number; loose_faces: number; loose_here: number; loose_places: { node: number | null; name: string; faces: number }[]; uncounted: number; board_feet: number; sheet_count: number };
};
const cutList = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { cutList(): Promise<Cut> } }).__sawhorse.cutList());

for (const f of FILES) {
  test(`${f.file}: opens, and the Cut list lists its parts`, async ({ page }) => {
    test.setTimeout(90_000);
    const bytes = readFileSync(path.join(dir, f.file));
    expect(createHash("sha256").update(bytes).digest("hex"), "the fixture is the file as published").toBe(f.sha256);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
    const dialogs: string[] = [];
    page.on("dialog", (d) => {
      dialogs.push(d.message());
      void d.dismiss();
    });
    await page.goto("/?io=basic&theme=light&autosave=0");
    const vp = page.locator("main.vp");
    await expect(vp).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
    await page.getByTestId("file-input").setInputFiles(path.join(dir, f.file));
    const report = page.getByTestId("import-report");
    await expect(report).toContainText(f.file, { timeout: 30_000 });
    const title = ((await report.locator("header b").textContent()) ?? "").trim();
    expect(title, "imported, not refused").toMatch(new RegExp(`^Imported ${f.file.replace(/[.]/g, "\\.")}: `));
    await expect(vp).toHaveAttribute("data-kernel", "ready");
    await page.getByRole("tab", { name: "Cut list" }).click();
    expect((await report.textContent()) ?? "", "the amber meshes the import made").toContain(`${f.meshes} mesh${f.meshes === 1 ? "" : "es"} that ${f.meshes === 1 ? "is" : "are"} not solid`);
    let cut = await cutList(page);
    if (f.loose) {
      // All loose faces: counted, never parts, until they are grouped; the empty list says
      // why (lane 15 verifier: it showed only the counts).
      expect(cut.totals.parts).toBe(0);
      expect(cut.totals.loose_faces).toBeGreaterThan(400);
      await expect(page.getByTestId("cutlist-empty")).toContainText(`the model's ${cut.totals.loose_faces} faces are loose`);
      await page.getByRole("button", { name: "Make parts from loose faces" }).click();
      await expect.poll(async () => (await cutList(page)).totals.parts).toBeGreaterThan(0);
      cut = await cutList(page);
    }
    const row = { file: f.file, title, parts: cut.totals.parts, notSolid: cut.totals.not_solid, loose: cut.totals.loose_faces, boardFeet: Math.round(cut.totals.board_feet * 100) / 100, names: cut.rows.slice(0, 8).map((r) => `${r.name} x${r.qty}${r.not_solid ? "*" : ""}`) };
    console.log(`[skp-ccby] ${JSON.stringify(row)}`);
    expect(cut.totals.parts, "parts in the cut list").toBeGreaterThan(0);
    expect(cut.totals.not_solid, "at least one part sized from its bounds and flagged").toBeGreaterThan(0);
    expect(cut.totals.parts, "parts, as this build reads the file").toBe(f.parts);
    expect(cut.totals.not_solid, "of them sized from their bounds").toBe(f.notSolid);
    expect(await page.locator("main.vp").getAttribute("data-objects")).not.toBeNull();
    // The flag reaches the pane.
    await expect(page.locator('.cutlist tbody tr[data-not-solid="true"]').first()).toBeVisible();
    // Lane 15 verifier: a whole bench, desk or carcass drawn as one piece of open faces is
    // not a board (it counted 126 to 661 board feet), and a flat face is not sheet goods (it
    // added a 4 x 8 sheet). Listed and flagged, counted nowhere.
    const uncounted = cut.rows.filter((r) => r.uncounted);
    expect(uncounted.map((r) => `${r.name}:${r.uncounted}`).sort(), "parts counted nowhere, and why").toEqual([...f.uncounted].sort());
    for (const r of uncounted) expect([r.board_feet, r.sheet], r.name).toEqual([0, false]);
    // (A closed solid is what was drawn and keeps its board feet however thick: coffee table 6.)
    for (const r of cut.rows.filter((x) => x.not_solid && !x.uncounted && !x.sheet)) expect(r.size[0], `${r.name}, open faces, is no thicker than a board`).toBeLessThanOrEqual(4 * 0.0254 + 1e-9);
    expect(cut.totals.sheet_count, "sheets 4x8").toBe(f.sheets);
    if (uncounted.length) await expect(page.locator(".cutlist tbody tr[data-uncounted] small").first()).toContainText("(not counted)");
    // Faces outside parts that sit inside an assembly: the pane says where and offers to open
    // it, never a Make parts link that would refuse.
    if (cut.totals.loose_faces > 0 && cut.totals.loose_here === 0) {
      const note = page.getByTestId("cutlist-geometry");
      await expect(note.getByRole("button", { name: "Make parts from loose faces" })).toHaveCount(0);
      const place = cut.totals.loose_places[0];
      await expect(note).toContainText(`faces outside parts (${place.node === null ? "at the top level" : `in ${place.name}`}`);
      await note.getByRole("button", { name: place.node === null ? "Go to the top level" : `Open ${place.name}` }).click();
      await expect.poll(async () => (await cutList(page)).totals.loose_here).toBeGreaterThan(0);
      await page.getByTestId("cutlist-geometry").getByRole("button", { name: "Make parts from loose faces" }).click();
      await expect.poll(async () => (await cutList(page)).totals.loose_here).toBe(0);
    }
    expect(dialogs, "no dialogs").toEqual([]);
    expect(errors, "no page or console errors").toEqual([]);
  });
}
