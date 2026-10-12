import { expect, test, type Page } from "@playwright/test";
import { IN, ready, solids } from "./helpers";

// The model API in the running app (lane 11): reads in the documented shapes, the
// selection both ways, and a batch with placeholders applied as ONE named undo step.

type Hook = {
  plugins: {
    list(): { id: string; status: string; logs: string[] }[];
    loadSession(files: Record<string, string>): Promise<string>;
    enable(id: string): Promise<void>;
    commands(): string[];
    run(key: string): Promise<void>;
  };
};
const hook = (page: Page) => page.evaluate.bind(page);

const BUILDER = `
import { model, ui } from "sawhorse";
const report = (k, v) => console.log(k + "=" + JSON.stringify(v));
await ui.registerCommand({ id: "box", async run() {
  const b = model.batch("Plugin box");
  const rect = b.op({ op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 24 * 0.0254, h: 16 * 0.0254 });
  const solid = b.op({ op: "extrude", sketch: 1, region: rect.region(0), distance: 12 * 0.0254 });
  b.rename({ object: solid.object }, "Plinth");
  const r = await b.commit();
  report("made", r.value(solid.object));
  await model.select([{ kind: "solid", id: r.value(solid.object) }]);
} });
await ui.registerCommand({ id: "read", async run() {
  report("solids", await model.solids());
  report("selection", await model.selection());
  report("stats", await model.stats());
  report("tree", await model.tree());
  report("faces", (await model.faces((await model.solids())[0].id)).length);
  report("document", await model.document());
} });
console.log("ready");
`;

const manifest = JSON.stringify({
  id: "org.example.builder",
  name: "Builder",
  version: "1.0.0",
  api: 1,
  description: "test",
  author: "Sawhorse",
  license: "MIT",
  permissions: ["model.read", "model.write", "ui"],
  contributes: { commands: [{ id: "box", title: "Box" }, { id: "read", title: "Read" }] },
});

async function last(page: Page, key: string): Promise<unknown> {
  let v: unknown = undefined;
  await expect
    .poll(async () => {
      const logs = (await page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((e) => e.id === "org.example.builder")!.logs)) as string[];
      const line = [...logs].reverse().find((l) => l.startsWith(`log: ${key}=`));
      v = line ? JSON.parse(line.slice(`log: ${key}=`.length)) : undefined;
      return v !== undefined;
    })
    .toBe(true);
  return v;
}

test("a plugin's batch is one named undo step, and its reads match the model", async ({ page }) => {
  await ready(page);
  const run = hook(page);
  const id = await run((files) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.loadSession(files), { "plugin.json": manifest, "main.js": BUILDER });
  await run((i) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.enable(i), id);
  await expect.poll(() => run(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.commands())).toContain("org.example.builder/box");

  // Run it from the Plugins menu.
  await page.getByRole("button", { name: "Plugins", exact: true }).click();
  await page.getByRole("menuitem", { name: "Builder" }).click();
  await page.getByRole("menuitem", { name: "Box", exact: true }).click();
  await expect(page.locator("#hint")).toContainText("Builder: Plugin box");
  const made = (await last(page, "made")) as number;
  const s = await solids(page);
  expect(s).toHaveLength(1);
  expect(s[0].volume).toBeCloseTo(24 * 16 * 12, 6);
  // The plugin selected what it made.
  await expect(page.locator("main.vp")).toHaveAttribute("data-selection", "1");

  // Reads.
  await run((k) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.run(k), "org.example.builder/read");
  const read = (await last(page, "solids")) as { id: number; label: string; min: number[]; max: number[]; editable: boolean }[];
  expect(read).toHaveLength(1);
  expect(read[0]).toMatchObject({ id: made, label: "Plinth", editable: true });
  expect(read[0].max[0] / IN).toBeCloseTo(24, 9);
  expect(await last(page, "selection")).toEqual([{ kind: "solid", id: made }]);
  expect(await last(page, "stats")).toMatchObject({ solids: 1, faces: 6, edges: 12, groups: 0, watertight: true });
  expect(await last(page, "faces")).toBe(6);
  expect(await last(page, "tree")).toMatchObject({ kind: "model", children: [{ kind: "solid", id: made, label: "Plinth" }] });
  expect(await last(page, "document")).toMatchObject({ units: "frac-in", canUndo: true, context: null });

  // The Edit menu names the step; one Ctrl+Z takes the whole batch back, Ctrl+Y brings it again.
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(page.getByRole("menuitem", { name: /Undo Plugin box/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await solids(page)).length).toBe(0);
  await expect(page.locator("#hint")).toContainText("Undone: Plugin box");
  await page.keyboard.press("Control+y");
  await expect.poll(async () => (await solids(page)).length).toBe(1);
  await expect(page.locator("#hint")).toContainText("Redone: Plugin box");
});
