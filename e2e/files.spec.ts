import { expect, test, type Page } from "@playwright/test";
import { FAMILIES, rng } from "../src/io/corpus/models";

// Lane 6: the File menu, import, export, save and open, drag-and-drop, on the production
// browser build. `?io=basic` uses the file input and downloads (the File System Access
// pickers are native dialogs Playwright cannot drive); the same handlers run behind both.
const requireKernel = !!process.env.REQUIRE_KERNEL;

async function ready(page: Page) {
  await page.goto("/?io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  const state = await vp.getAttribute("data-kernel");
  if (!requireKernel && state === "missing") test.skip(true, "kernel WASM not built (run npm run wasm)");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** A binary STL of axis boxes (millimetres), optionally missing its last two triangles (open). */
function stlBoxes(boxes: { at: [number, number, number]; size: [number, number, number] }[], open = false): Buffer {
  const tris: number[][][] = [];
  for (const { at, size } of boxes) {
    const c = (i: number) => [at[0] + (i & 1 ? size[0] : 0), at[1] + (i & 2 ? size[1] : 0), at[2] + (i & 4 ? size[2] : 0)];
    for (const q of [
      [0, 2, 3, 1],
      [4, 5, 7, 6],
      [0, 1, 5, 4],
      [2, 6, 7, 3],
      [1, 3, 7, 5],
      [0, 4, 6, 2],
    ])
      tris.push([c(q[0]), c(q[1]), c(q[2])], [c(q[0]), c(q[2]), c(q[3])]);
  }
  if (open) tris.splice(-2, 2);
  const b = Buffer.alloc(84 + tris.length * 50);
  b.write("test stl", 0);
  b.writeUInt32LE(tris.length, 80);
  tris.forEach((t, k) => t.forEach((v, j) => v.forEach((x, a) => b.writeFloatLE(x, 84 + k * 50 + 12 + j * 12 + a * 4))));
  return b;
}

async function pick(page: Page, name: string, buffer: Buffer, mimeType = "application/octet-stream") {
  await page.getByTestId("file-input").setInputFiles({ name, mimeType, buffer });
}

async function menu(page: Page, top: string, ...path: string[]) {
  await page.getByRole("button", { name: top, exact: true }).click();
  for (const [i, label] of path.entries()) {
    const item = page.getByRole("menuitem", { name: label, exact: true });
    if (i < path.length - 1) await item.hover();
    else await item.click();
  }
}

async function download(page: Page, action: () => Promise<void>) {
  const [d] = await Promise.all([page.waitForEvent("download"), action()]);
  const path = await d.path();
  const fs = await import("node:fs");
  return { name: d.suggestedFilename(), bytes: fs.readFileSync(path) };
}

test("the File menu: open, save, import, units and every export", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "File", exact: true }).click();
  const items = page.getByRole("menu", { name: "File" }).locator(":scope > button, :scope > .subwrap > button");
  await expect(items).toHaveText([/New/, /Open…\s*Ctrl\+O/, /Save\s*Ctrl\+S/, /Save As…/, /Import…\s*Ctrl\+I/, /Import units/, /Export/]);
  await page.getByRole("menuitem", { name: "Export", exact: true }).hover();
  await expect(page.getByRole("menu", { name: "Export" }).getByRole("menuitem")).toHaveText([
    "STL…",
    "3MF…",
    "glTF (GLB)…",
    "OBJ…",
    "DXF (3D)…",
    "DXF outline of selected face…",
    "SVG drawing of this view…",
    "SVG outline of selected face…",
    // Lane 12: a section plane's slice (greyed out until the model has a section plane).
    "Section slice (SVG)…",
    "Section slice (DXF)…",
  ]);
  await expect(page.getByRole("menuitem", { name: "Section slice (SVG)…" })).toBeDisabled();
});

test("import an STL, export STL and 3MF, re-import the STL", async ({ page }) => {
  const vp = await ready(page);
  await pick(page, "blocks.stl", stlBoxes([{ at: [0, 0, 0], size: [100, 200, 50] }]));
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(page.getByTestId("import-report")).toContainText("Imported blocks.stl: 1 solid");
  await expect(page.getByTestId("import-report")).toContainText("STL has no units; read as millimetres");
  const volume = Number(await vp.getAttribute("data-volume"));
  expect(volume).toBeCloseTo(0.001, 9);

  const stl = await download(page, () => menu(page, "File", "Export", "STL…"));
  expect(stl.name).toBe("Untitled.stl");
  expect(stl.bytes.readUInt32LE(80)).toBe(12);
  expect(stl.bytes.length).toBe(84 + 12 * 50);
  const tmf = await download(page, () => menu(page, "File", "Export", "3MF…"));
  expect(tmf.name).toBe("Untitled.3mf");
  expect(tmf.bytes.subarray(0, 2).toString()).toBe("PK");

  // Round trip: our STL comes back as the same box, beside the first.
  await pick(page, "again.stl", stl.bytes);
  await expect(vp).toHaveAttribute("data-objects", "2");
  expect(Number(await vp.getAttribute("data-volume"))).toBeCloseTo(0.002, 9);
  // One undo step per import.
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-objects", "1");
});

test("an open mesh comes in visible, is named in the HUD, and STL export refuses it by name", async ({ page }) => {
  const vp = await ready(page);
  await pick(page, "tray.stl", stlBoxes([{ at: [0, 0, 0], size: [50, 50, 20] }], true));
  await expect(page.getByTestId("import-report")).toContainText("kept as a mesh, not a solid: 4 open edges");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page.locator(".hud")).toContainText("1 mesh not solid (imported)");
  expect(Number(await vp.getAttribute("data-triangles"))).toBe(10);
  await menu(page, "File", "Export", "STL…");
  await expect(page.locator(".vcb")).toContainText('Export refused: the imported mesh "tray" is not a solid (4 open edges)');
  // GLB still takes it.
  const glb = await download(page, () => menu(page, "File", "Export", "glTF (GLB)…"));
  expect(glb.bytes.subarray(0, 4).toString()).toBe("glTF");
  await menu(page, "Edit", "Delete Imported Meshes");
  await expect(page.locator(".hud")).not.toContainText("not solid");
});

test("save as .shz, start a new model, open the file again", async ({ page }) => {
  const vp = await ready(page);
  await pick(page, "parts.stl", stlBoxes([{ at: [0, 0, 0], size: [40, 40, 40] }, { at: [100, 0, 0], size: [20, 30, 10] }]));
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  const hash = await vp.getAttribute("data-hash");
  const saved = await download(page, () => page.keyboard.press("Control+s"));
  expect(saved.name).toBe("Untitled.shz");
  expect(saved.bytes.subarray(30, 38).toString()).toBe("mimetype");
  await expect(page).toHaveTitle("Untitled - Sawhorse");
  await menu(page, "File", "New");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await pick(page, "parts.shz", saved.bytes);
  await expect(vp).toHaveAttribute("data-objects", "2");
  await expect(vp).toHaveAttribute("data-hash", hash!);
  await expect(page).toHaveTitle("parts - Sawhorse");
  // Nothing to undo in a freshly opened file.
  await expect(page.locator(".hud")).toContainText("undo 0");
});

test("drag and drop: an SVG becomes sketch regions on the ground", async ({ page }) => {
  const vp = await ready(page);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><rect x="10" y="10" width="50" height="30"/><circle cx="80" cy="70" r="10"/></svg>`;
  const dt = await page.evaluateHandle((text) => {
    const d = new DataTransfer();
    d.items.add(new File([text], "plate.svg", { type: "image/svg+xml" }));
    return d;
  }, svg);
  await page.dispatchEvent("#app", "dragover", { dataTransfer: dt });
  await expect(page.locator(".drop-hint")).toBeVisible();
  await page.dispatchEvent("#app", "drop", { dataTransfer: dt });
  await expect(page.getByTestId("import-report")).toContainText("Imported plate.svg: 0 solids, strokes in 1 sketch");
  await expect(page.locator(".hud")).toContainText("Sketch: 2 regions");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
});

test("a SketchUp file imports through openskp: a group and a component's instances", async ({ page }) => {
  const vp = await ready(page);
  const bytes = Buffer.from(FAMILIES.table(rng(7)).toBytes());
  await pick(page, "table.skp", bytes);
  await expect(page.getByTestId("import-report")).toContainText("Imported table.skp: 5 solids", { timeout: 15_000 });
  await expect(page.getByTestId("import-report")).toContainText("1 group, 4 component instances (1 component)");
  // The Outliner shows the legs as one component placed four times.
  await expect(page.locator(".outline li", { hasText: "component · 4 of 4" })).toBeVisible();
  await expect(vp).toHaveAttribute("data-objects", "5");
  await expect(vp).toHaveAttribute("data-watertight", "true");
});
