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
const FILES: { file: string; sha256: string; parts: number; notSolid: number; meshes: number; loose?: boolean }[] = [
  { file: "benches.skp", sha256: "c9f1cc7486aacf897e3d9d44107954bad0d6b4ba20012192e47a4b8cddacc994", parts: 3, notSolid: 3, meshes: 1, loose: true },
  { file: "coffee tables.skp", sha256: "91d7321550b78147cda25b436275e3ae1ac60ae39bf52b7c6f1b8def7ef0359d", parts: 10, notSolid: 9, meshes: 10 },
  { file: "cupboard.skp", sha256: "63431b71863688994d987e9dc90f7d369df336ecafc9a7cca2db4018432c372e", parts: 4, notSolid: 4, meshes: 8 },
  { file: "desk.skp", sha256: "6d370f9ce838b5a0c0abde9271cec7e7850f895f88a8438e8d5cbd50ce909301", parts: 1, notSolid: 1, meshes: 1 },
  { file: "side table.skp", sha256: "eddaf9709e8027e419207d5507a7bba3085b9c5f1dd4f322b46489aa4c28bf99", parts: 1, notSolid: 1, meshes: 1 },
];

type CutRow = { name: string; qty: number; not_solid: boolean; size: number[] };
type Cut = { rows: CutRow[]; totals: { parts: number; not_solid: number; loose_faces: number; board_feet: number } };
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
      // All loose faces: counted, never parts, until they are grouped.
      expect(cut.totals.parts).toBe(0);
      expect(cut.totals.loose_faces).toBeGreaterThan(400);
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
    expect(dialogs, "no dialogs").toEqual([]);
    expect(errors, "no page or console errors").toEqual([]);
  });
}
