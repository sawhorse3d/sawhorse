import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { box, hover, at, IN, solids, setUnits, type V3 } from "./helpers";

// The three sample plugins (plugins/), driven through the UI as a person would (lane 11):
// enabled from the Plugins pane after the permission dialog, the cabinet generator's panel,
// the shelf-pin tool on the rail, and the model report's live counts and export.

const CABINET = "com.sawhorse3d.cabinet-generator";
const PINS = "com.sawhorse3d.shelf-pin-holes";
const REPORT = "com.sawhorse3d.model-report";
const requireKernel = !!process.env.REQUIRE_KERNEL;

async function start(page: Page) {
  // io=basic: saves become downloads Playwright can read.
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
}

const row = (page: Page, id: string) => page.locator(`.plugin-row[data-plugin="${id}"]`);

/** Enable a sample from the Plugins pane, through the permission dialog. */
async function enable(page: Page, id: string, name: string, permissions: string[]) {
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(row(page, id)).toHaveAttribute("data-status", "disabled");
  await row(page, id).locator('[data-do="enable"]').click();
  const dialog = page.getByRole("dialog", { name: `Enable ${name}` });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator("li[data-permission]")).toHaveCount(permissions.length);
  for (const p of permissions) await expect(dialog.locator(`li[data-permission="${p}"]`)).toBeVisible();
  await dialog.getByRole("button", { name: "Enable" }).click();
  await expect(row(page, id)).toHaveAttribute("data-status", "running", { timeout: 10_000 });
}

/** Type into a plugin panel's field and commit it (Enter). */
async function field(page: Page, panel: string, id: string, value: string) {
  const input = page.locator(`[data-panel="${panel}"] [data-field="${id}"]`);
  await input.fill(value);
  await input.press("Enter");
}

test("the samples are listed, disabled, with their permissions", async ({ page }) => {
  await start(page);
  await page.getByRole("tab", { name: "Plugins" }).click();
  for (const [id, perms] of [
    [CABINET, ["model.read", "model.write", "ui", "storage"]],
    [PINS, ["model.read", "model.write", "ui", "storage"]],
    [REPORT, ["model.read", "ui", "file", "storage"]],
  ] as const) {
    await expect(row(page, id)).toHaveAttribute("data-status", "disabled");
    await expect(row(page, id).locator(".plugin-state")).toHaveText("Off");
    await expect(row(page, id).locator(".plugin-chips li")).toHaveText([...perms]);
    await expect(row(page, id)).toContainText("sample · Sawhorse · MIT");
  }
  // Cancel leaves it off.
  await row(page, CABINET).locator('[data-do="enable"]').click();
  await page.getByRole("dialog", { name: "Enable Cabinet generator" }).getByRole("button", { name: "Cancel" }).click();
  await expect(row(page, CABINET)).toHaveAttribute("data-status", "disabled");
});

test("cabinet generator: a 30 x 72 x 12 in cabinet with 3 shelves; the Outliner names the parts and the Cut list lists them", async ({ page }) => {
  await start(page);
  await enable(page, CABINET, "Cabinet generator", ["model.read", "model.write", "ui", "storage"]);
  const panel = `${CABINET}/cabinet`;
  await expect(page.locator(`[data-panel="${panel}"] summary`)).toContainText("Cabinet generator");
  await field(page, panel, "width", "30");
  await field(page, panel, "height", "72");
  await field(page, panel, "depth", "12");
  await field(page, panel, "shelves", "3");
  await expect(page.locator(`[data-panel="${panel}"] [data-note="status"]`)).toContainText("Adds a cabinet: 30 x 72 x 12, 3 shelves, 1/4 back, 4 toe kick");
  await page.locator(`[data-panel="${panel}"] [data-action="generate"]`).click();
  await expect(page.locator("#hint")).toContainText("Made a cabinet: Side x 2, Top, Bottom, Shelf x 3, Back, Toe kick");

  // Nine parts, the whole cabinet 30 x 12 x 72 in.
  const s = await solids(page);
  expect(s).toHaveLength(9);
  const lo = [0, 1, 2].map((k) => Math.min(...s.map((x) => x.min[k])));
  const hi = [0, 1, 2].map((k) => Math.max(...s.map((x) => x.max[k])));
  expect(hi.map((h, k) => h - lo[k]).map((d) => Math.round(d * 1e6) / 1e6)).toEqual([30, 12, 72]);
  // It is selected, so the button now updates it.
  await expect(page.locator(`[data-panel="${panel}"] [data-action="generate"]`)).toHaveText("Update selected cabinet");

  // The Outliner: the cabinet, unfolded, shows its named parts.
  await page.getByRole("tab", { name: "Outliner" }).click();
  const cab = page.locator('.outline li[data-kind="instance"]', { hasText: "Cabinet" }).first();
  await expect(cab).toBeVisible();
  await cab.getByRole("button", { name: "Unfold Cabinet" }).click();
  for (const name of ["Left side", "Right side", "Top", "Bottom", "Shelf 1", "Shelf 2", "Shelf 3", "Back", "Toe kick"]) await expect(page.locator(".outline li .label", { hasText: new RegExp(`^${name}$`) })).toHaveCount(1);

  // The Cut list reads the parts straight away.
  await page.getByRole("tab", { name: "Cut list" }).click();
  await expect(page.locator(".cutlist table")).toBeVisible();
  const rows = await page.locator(".cutlist table tbody tr").evaluateAll((trs) => trs.map((tr) => Array.from(tr.querySelectorAll("td")).map((td) => ((td.querySelector(".name") ?? td).textContent ?? "").trim())));
  const byName = Object.fromEntries(rows.map((r) => [r[0], r.slice(1, 5)]));
  expect(byName["Side"]).toEqual(["2", "3/4", "12", "72"]);
  expect(byName["Top"]).toEqual(["1", "3/4", "11 3/4", "28 1/2"]);
  expect(byName["Bottom"]).toEqual(["1", "3/4", "11 3/4", "28 1/2"]);
  expect(byName["Shelf"]).toEqual(["3", "3/4", "11 3/4", "28 1/2"]);
  expect(byName["Back"]).toEqual(["1", "1/4", "28 1/2", "68"]);
  expect(byName["Toe kick"]).toEqual(["1", "3/4", "4", "28 1/2"]);
  // One undo step takes the whole cabinet back.
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await solids(page)).length).toBe(0);
  await expect(page.locator("#hint")).toContainText("Undone: Generate cabinet");
});

test("cabinet generator: Update replaces the selected cabinet in place", async ({ page }) => {
  await start(page);
  await enable(page, CABINET, "Cabinet generator", ["model.read", "model.write", "ui", "storage"]);
  const panel = `${CABINET}/cabinet`;
  await page.locator(`[data-panel="${panel}"] [data-action="generate"]`).click();
  await expect(page.locator("#hint")).toContainText("Made a cabinet");
  const first = await solids(page);
  await field(page, panel, "shelves", "2");
  await page.locator(`[data-panel="${panel}"] [data-action="generate"]`).click();
  await expect(page.locator("#hint")).toContainText("Updated the cabinet");
  const second = await solids(page);
  expect(second).toHaveLength(first.length + 1);
  // Same place: the same overall box.
  const box = (ss: typeof first) => [0, 1, 2].map((k) => [Math.min(...ss.map((x) => x.min[k])), Math.max(...ss.map((x) => x.max[k]))]);
  expect(box(second)).toEqual(box(first));
});

/** The Cut list tab's rows by part name: Qty, T, W, L as shown. */
async function cutRows(page: Page): Promise<Record<string, string[]>> {
  await page.getByRole("tab", { name: "Cut list" }).click();
  await expect(page.locator(".cutlist table")).toBeVisible();
  const rows = await page.locator(".cutlist table tbody tr").evaluateAll((trs) => trs.map((tr) => Array.from(tr.querySelectorAll("td")).map((td) => ((td.querySelector(".name") ?? td).textContent ?? "").trim())));
  return Object.fromEntries(rows.map((r) => [r[0], r.slice(1, 5)]));
}

type NestPiece = { kind: string; stock: string; placements: { name: string }[] };
/** Which stock each part name was nested on (the Nesting section's layout). */
async function nestedOn(page: Page): Promise<Record<string, string[]>> {
  const n = (await page.evaluate(async () => (window as unknown as { __sawhorse: { nest(): Promise<unknown> } }).__sawhorse.nest())) as { pieces: NestPiece[]; unplaced: unknown[] };
  expect(n.unplaced).toEqual([]);
  const out: Record<string, string[]> = {};
  for (const p of n.pieces) for (const q of p.placements) (out[q.name] ??= []).includes(`${p.kind} ${p.stock}`) || out[q.name].push(`${p.kind} ${p.stock}`);
  return out;
}

// Issue #60: the back's thickness is a field beside Stock, 6 mm in a metric document and
// 1/4 in otherwise to start, so a 16 or 18 mm back is one number to type.
test("cabinet generator: an 18 mm back typed beside Stock is the Back on the cut list, nested like the sides, and kept (#60)", async ({ page }) => {
  await start(page);
  await setUnits(page, "mm");
  await enable(page, CABINET, "Cabinet generator", ["model.read", "model.write", "ui", "storage"]);
  const panel = page.locator(`[data-panel="${CABINET}/cabinet"]`);
  const back = panel.locator('[data-field="backThickness"]');
  // Right after Stock, labelled, in millimetres, starting at 6.
  const labels = await panel.locator(".pf-field > span, .pf-check > span").allTextContents();
  expect(labels.slice(3, 7)).toEqual(["Stock", "Back thickness", "Shelves", "Back panel"]);
  await expect(back).toHaveValue("6");
  // Greyed out while there is no back.
  await panel.locator('[data-field="back"]').click();
  await expect(panel.locator('[data-field="back"]')).not.toBeChecked();
  await expect(back).toBeDisabled();
  await expect(panel.locator('[data-note="status"]')).toHaveText("Adds a cabinet: 600 x 720 x 560, 1 shelf, 100 toe kick.");
  await panel.locator('[data-field="back"]').click();
  await expect(panel.locator('[data-field="back"]')).toBeChecked();
  await expect(back).toBeEnabled();
  // No thickness is refused in the panel's words; the button waits.
  await field(page, `${CABINET}/cabinet`, "backThickness", "0");
  await expect(panel.locator('[data-note="status"]')).toHaveText("The back needs a thickness.");
  await expect(panel.locator('[data-action="generate"]')).toBeDisabled();
  await field(page, `${CABINET}/cabinet`, "backThickness", "18");
  await expect(panel.locator('[data-note="status"]')).toHaveText("Adds a cabinet: 600 x 720 x 560, 1 shelf, 18 back, 100 toe kick.");
  await panel.locator('[data-action="generate"]').click();
  await expect(page.locator("#hint")).toContainText("Made a cabinet: Side x 2, Top, Bottom, Shelf, Back, Toe kick");

  // The model: the back 18 mm deep at the rear, between the sides, from the toe kick up.
  const s = await solids(page);
  expect(s).toHaveLength(7);
  const mm = (v: number) => Math.round(v * 25.4 * 1000) / 1000;
  const rear = s.filter((x) => mm(x.min[1]) === 542);
  expect(rear).toHaveLength(1);
  expect([...rear[0].min, ...rear[0].max].map(mm)).toEqual([18, 542, 100, 582, 560, 720]);

  // The Cut list: Back 18 x 564 x 620, the sides 18 x 560 x 720, the top 542 deep.
  const rows = await cutRows(page);
  expect(rows["Back"]).toEqual(["1", "18", "564", "620"]);
  expect(rows["Side"]).toEqual(["2", "18", "560", "720"]);
  for (const name of ["Top", "Bottom", "Shelf"]) expect(rows[name]).toEqual(["1", "18", "542", "564"]);
  // Nesting, with the built-in stock library: a back over 1/4 in with no material is solid
  // stock, so it goes on the boards the sides go on (both glued up, being wider than 12 in).
  const nest = await nestedOn(page);
  expect(nest["Side"]).toEqual(["board 4/4 x 12 x 96"]);
  expect(nest["Back"]).toEqual(nest["Side"]);

  // Saved, opened again: the field still says 18.
  await saveAndReopen(page);
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(back).toHaveValue("18");
});

test("cabinet generator: a 1/2 in back in an inch document, on the cut list, and still there after a reload (#60)", async ({ page }) => {
  await start(page);
  await enable(page, CABINET, "Cabinet generator", ["model.read", "model.write", "ui", "storage"]);
  const panel = `${CABINET}/cabinet`;
  const back = page.locator(`[data-panel="${panel}"] [data-field="backThickness"]`);
  await expect(back).toHaveValue("1/4");
  await field(page, panel, "width", "30");
  await field(page, panel, "height", "72");
  await field(page, panel, "depth", "12");
  await field(page, panel, "shelves", "3");
  await field(page, panel, "backThickness", "1/2");
  await expect(page.locator(`[data-panel="${panel}"] [data-note="status"]`)).toHaveText("Adds a cabinet: 30 x 72 x 12, 3 shelves, 1/2 back, 4 toe kick.");
  await page.locator(`[data-panel="${panel}"] [data-action="generate"]`).click();
  await expect(page.locator("#hint")).toContainText("Made a cabinet: Side x 2, Top, Bottom, Shelf x 3, Back, Toe kick");
  const rows = await cutRows(page);
  expect(rows["Back"]).toEqual(["1", "1/2", "28 1/2", "68"]);
  expect(rows["Side"]).toEqual(["2", "3/4", "12", "72"]);
  for (const [name, qty] of [["Top", "1"], ["Bottom", "1"], ["Shelf", "3"]]) expect(rows[name]).toEqual([qty, "3/4", "11 1/2", "28 1/2"]);
  expect(rows["Toe kick"]).toEqual(["1", "3/4", "4", "28 1/2"]);
  // Thicker than 1/4 in, so solid stock: on the 4/4 boards with the sides.
  const nest = await nestedOn(page);
  expect(nest["Back"]).toEqual(["board 4/4 x 12 x 96"]);
  expect(nest["Side"]).toEqual(nest["Back"]);
  // The plugin keeps it for next time: after a reload the field still says 1/2.
  await page.reload();
  await expect(page.locator("main.vp")).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(row(page, CABINET)).toHaveAttribute("data-status", "running", { timeout: 10_000 });
  await expect(back).toHaveValue("1/2");
});

// Lane 28 (#34, #21): Generate pressed again with the cabinet selected ("Update selected
// cabinet") deletes the old cabinet and makes a new one in its slots, and Explode removes
// the cabinet's instance; either left ids the file reader numbered afresh, so Ctrl+S said
// "Could not save: the file did not read back as written: the model differs", and kept
// saying it for every edit after. Each is saved here, opened again, and must be the same
// model; the cabinet the plugin keeps in the document is still the one it updates.
async function saveAndReopen(page: Page) {
  const vp = page.locator("main.vp");
  const hash = await vp.getAttribute("data-hash");
  const [d] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  await expect(page.locator("#hint")).toContainText(/Saved (Untitled|cabinet)\.shz/);
  const bytes = readFileSync((await d.path())!);
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "New", exact: true }).click();
  await expect.poll(async () => (await solids(page)).length).toBe(0);
  await page.getByTestId("file-input").setInputFiles({ name: "cabinet.shz", mimeType: "application/octet-stream", buffer: bytes });
  await expect(page).toHaveTitle("cabinet - Sawhorse");
  await expect(vp).toHaveAttribute("data-hash", hash!);
}

test("cabinet generator: a cabinet updated in place saves and opens as the same model, and Update still finds it (#34)", async ({ page }) => {
  await start(page);
  await enable(page, CABINET, "Cabinet generator", ["model.read", "model.write", "ui", "storage"]);
  const panel = `${CABINET}/cabinet`;
  const generate = page.locator(`[data-panel="${panel}"] [data-action="generate"]`);
  await generate.click();
  await expect(page.locator("#hint")).toContainText("Made a cabinet");
  await field(page, panel, "shelves", "2");
  await generate.click();
  await expect(page.locator("#hint")).toContainText("Updated the cabinet");
  await saveAndReopen(page);
  expect(await solids(page)).toHaveLength(8);
  // The plugin's stored id names the reopened cabinet: selected, the button updates it.
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.locator('.outline li[data-kind="instance"]', { hasText: "Cabinet" }).first().locator(".label").first().click();
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(generate).toHaveText("Update selected cabinet");
  await field(page, panel, "shelves", "3");
  await generate.click();
  await expect(page.locator("#hint")).toContainText("Updated the cabinet");
  expect(await solids(page)).toHaveLength(9);
  // And the model saves again.
  await saveAndReopen(page);
});

test("cabinet generator: a cabinet exploded and its parts grouped saves and opens as the same model (#21)", async ({ page }) => {
  await start(page);
  await enable(page, CABINET, "Cabinet generator", ["model.read", "model.write", "ui", "storage"]);
  await page.locator(`[data-panel="${CABINET}/cabinet"] [data-action="generate"]`).click();
  await expect(page.locator("#hint")).toContainText("Made a cabinet");
  // The new cabinet is selected: Ctrl+Shift+G explodes it into its parts.
  await page.keyboard.press("Control+Shift+g");
  await expect(page.locator("#hint")).toContainText("Exploded");
  // Its seven parts are top-level components now; all of them grouped.
  const top = async () => (await page.evaluate(() => (window as unknown as { __sawhorse: { graph(): { nodes: { kind: string; parent: number | null }[] } } }).__sawhorse.graph())).nodes.filter((n) => n.parent === null).map((n) => n.kind);
  await expect.poll(top).toEqual(Array(7).fill("instance"));
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Control+g");
  await expect.poll(top).toEqual(["group"]);
  await saveAndReopen(page);
  expect(await solids(page)).toHaveLength(7);
});

test("shelf pin holes: hover a face, see the column, click to drill it; one undo removes it", async ({ page }) => {
  await start(page);
  await enable(page, PINS, "Shelf pin holes", ["model.read", "model.write", "ui", "storage"]);
  // A 24 x 12 x 30 in side to drill into.
  await box(page, "24,12", "30");
  const before = (await solids(page))[0];
  const tool = page.locator(`.rail [data-group="plugins"] button[data-tool="plugin:${PINS}/drill"]`);
  await expect(tool).toBeVisible();
  await tool.click();
  await expect(tool).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#vcbName")).toHaveText("Count, pitch, dia");
  // The front face (y = 0), 4 in up: a column of 8 previews.
  const p: V3 = [12, 0, 4];
  await hover(page, p, "on-face");
  await expect(page.locator("#hint")).toContainText("Click to drill 8 holes");
  const [x, y] = await at(page, p, 1, -1);
  await page.mouse.click(x, y);
  await expect(page.locator("#hint")).toContainText("Drilled 8 holes");
  const after = (await solids(page))[0];
  // Eight 16-sided holes 5 mm across and 3/8 in deep.
  const hole = 0.5 * 16 * (0.0025 ** 2) * Math.sin((2 * Math.PI) / 16) * 0.375 * IN;
  expect((before.volume - after.volume) * IN ** 3).toBeCloseTo(8 * hole, 9);
  // One Ctrl+Z puts the side back exactly.
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await solids(page))[0].volume).toBeCloseTo(before.volume, 9);
  await expect(page.locator("#hint")).toContainText("Undone: Drill 8 shelf pin holes");
});

test("model report: live counts that match the model, and an export as Markdown and CSV", async ({ page }) => {
  await start(page);
  await enable(page, REPORT, "Model report", ["model.read", "ui", "file", "storage"]);
  await enable(page, CABINET, "Cabinet generator", ["model.read", "model.write", "ui", "storage"]);
  const report = page.locator(`[data-panel="${REPORT}/report"]`);
  const counts = async () => Object.fromEntries(await report.locator('table[data-table="counts"] tbody tr').evaluateAll((trs) => trs.map((tr) => [tr.children[0].textContent, Number(tr.children[1].textContent)])));
  await expect.poll(counts).toMatchObject({ Solids: 0, Faces: 0 });
  await page.locator(`[data-panel="${CABINET}/cabinet"] [data-action="generate"]`).click();
  await expect(page.locator("#hint")).toContainText("Made a cabinet");
  // The report follows the edit; its counts are the model's.
  const truth = await page.evaluate(() => {
    const w = window as unknown as { __sawhorse: { graph(): { nodes: { kind: string }[]; definitions: unknown[] }; solids(): unknown[] } };
    const g = w.__sawhorse.graph();
    return { Solids: w.__sawhorse.solids().length, Groups: g.nodes.filter((n) => n.kind === "group").length, Components: g.definitions.length, Instances: g.nodes.filter((n) => n.kind === "instance").length };
  });
  // The default cabinet: two sides, top, bottom, one shelf, back, toe kick.
  expect(truth).toEqual({ Solids: 7, Groups: 0, Components: 7, Instances: 8 });
  await expect.poll(counts).toMatchObject({ ...truth, Faces: 7 * 6, Edges: 7 * 12 });
  await expect(report.locator('[data-note="size"]')).toContainText("Overall 24 wide x 24 deep x 34 1/2 high");
  await expect(report.locator('[data-note="warnings"]')).toHaveText("No warnings.");
  // Units: millimetres, remembered by the plugin.
  await report.locator('[data-field="units"]').selectOption("mm");
  await expect(report.locator('[data-note="size"]')).toContainText("Overall 609.6 mm wide");
  // Export: Markdown, then CSV, through the save dialog (downloads here).
  const downloads: string[] = [];
  page.on("download", async (d) => downloads.push(readFileSync((await d.path())!, "utf8")));
  await report.locator('[data-action="export"]').click();
  // Lane 17: no save dialog in this browser, so the app asks before each download, naming
  // the plugin, the file and its size; nothing is downloaded before the person says yes.
  for (const name of ["Untitled report.md", "Untitled report.csv"]) {
    const ask = page.getByRole("dialog", { name: `Save ${name}?` });
    await expect(ask).toContainText(`Model report wants to save ${name} (`);
    await expect(ask).toContainText(/\d[\d,]* bytes\)/);
    expect(downloads.length).toBe(name.endsWith(".md") ? 0 : 1);
    await ask.getByRole("button", { name: "Save" }).click();
    await expect(ask).toHaveCount(0);
    if (name.endsWith(".md")) await expect.poll(() => downloads.length).toBe(1);
  }
  await expect(page.locator("#hint")).toContainText("Saved Untitled report.md and Untitled report.csv");
  await expect.poll(() => downloads.length).toBe(2);
  const md = downloads.find((d) => d.startsWith("# Model report"))!;
  expect(md).toContain("| Solids | 7 |");
  expect(md).toMatch(/\| Shelf \| 1 \| 19(\.1)? mm \|/);
  const csv = downloads.find((d) => d.includes("Section,Item,Value"))!;
  expect(csv).toContain("Counts,Instances,8");
});

// The verifier: a zip with the same id and permissions as an enabled sample replaced it and
// ran at once with the sample's grant, without a dialog. A grant now belongs to the files.
test("a plugin claiming an enabled sample's id does not run on the sample's grant", async ({ page }) => {
  await start(page);
  const perms = ["model.read", "ui", "file", "storage"];
  await enable(page, REPORT, "Model report", perms);
  const evil = {
    "plugin.json": JSON.stringify({ id: REPORT, name: "Model report (evil)", version: "1.0.0", api: 1, description: "Not the sample.", author: "Someone", license: "MIT", permissions: perms }),
    "main.js": 'import { model } from "sawhorse";\nawait model.solids();\nconsole.log("evil ran");',
  };
  await page.evaluate((f) => (window as unknown as { __sawhorse: { plugins: { loadSession(f: Record<string, string>): Promise<string> } } }).__sawhorse.plugins.loadSession(f), evil);
  await expect(row(page, REPORT)).toHaveAttribute("data-status", "disabled");
  await expect(row(page, REPORT)).toContainText("Model report (evil)");
  await expect(row(page, REPORT)).toContainText("These are not the files you enabled (it takes the place of the bundled Model report)");
  const logs = await page.evaluate((id) => (window as unknown as { __sawhorse: { plugins: { list(): { id: string; logs: string[] }[] } } }).__sawhorse.plugins.list().find((e) => e.id === id)!.logs, REPORT);
  expect(logs.join("\n")).not.toContain("evil ran");
  // Enabling it is the person's choice, and the dialog says what it replaces.
  await row(page, REPORT).locator('[data-do="enable"]').click();
  const dialog = page.getByRole("dialog", { name: "Enable Model report (evil)" });
  await expect(dialog.locator("[data-replaces]")).toContainText("same id as the bundled sample Model report");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(row(page, REPORT)).toHaveAttribute("data-status", "disabled");
});
