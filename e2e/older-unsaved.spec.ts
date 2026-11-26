import { expect, test, type Page } from "@playwright/test";
import { box } from "./helpers";

// #149: unsaved work set aside by a build before the change, whose steps went through the
// case it changed (a line drawn in a group of one copy of a component, the group then
// deleted in another copy). That build removed the line, so the later steps name sketches
// by ids that differ here; replayed as it is, the push would land on another rectangle.
// Such a record has no mark (AutosaveMeta.rules), so Restore replays it guarded: refused,
// kept set aside, offered until it is discarded. The logs below are the ones that build
// wrote (its WASM kernel, d1dbe4e), steps and ids as it gave them.
const requireKernel = !!process.env.REQUIRE_KERNEL;

const I = [
  [1.0, 0.0, 0.0],
  [0.0, 1.0, 0.0],
  [0.0, 0.0, 1.0],
];
const plane = (z: number) => ({ on: "plane", origin: [0.0, 0.0, z], normal: [0.0, 0.0, 1.0] });
const rect = (z: number, x: number) => ({ op: "draw", target: plane(z), points: [[x, 0, z], [x + 1, 0, z], [x + 1, 1, z], [x, 1, z]], closed: true });
/** The #149 list (with a line in the copy's group when `line`), two rectangles at z 2 and
 * z 4 and the second pushed, as the older build logged it. */
const olderLog = (line: boolean) =>
  JSON.stringify([
    { op: "new_sketch", origin: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] },
    { op: "draw", target: plane(0), points: [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], closed: true },
    { op: "extrude", sketch: 1, region: 1, distance: 0.5 },
    { op: "copy", objects: [1], xform: { linear: I, translation: [2.0, 0.0, 0.0] } },
    { op: "make_group", items: [{ object: 1 }] },
    { op: "make_component", items: [{ node: 1 }, { object: 2 }], name: "C" },
    { op: "copy", objects: [], xform: { linear: I, translation: [0.0, 3.0, 0.0] }, nodes: [2] },
    { op: "set_context", node: 4 },
    ...(line ? [{ op: "draw", target: plane(0), points: [[5, 5, 0], [6, 5, 0]], closed: false }] : []),
    { op: "set_context", node: 2 },
    { op: "delete_node", node: 1 },
    { op: "leave_context" },
    rect(2, 10),
    rect(4, 12),
    { op: "extrude", sketch: 3, region: 1, distance: 0.25 },
  ]);
const REFUSED = "Could not restore the unsaved work: an older version set it aside, and one of its steps gives a different result in this version";

async function loaded(page: Page) {
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

type Records = Record<string, unknown>;
/** Every record of the autosave store, by key. */
async function stored(page: Page): Promise<Records> {
  return page.evaluate(
    () =>
      new Promise<Records>((resolve) => {
        const req = indexedDB.open("sawhorse", 1);
        req.onupgradeneeded = () => req.result.createObjectStore("autosave");
        req.onsuccess = () => {
          const db = req.result;
          const out: Records = {};
          const cur = db.transaction("autosave", "readonly").objectStore("autosave").openCursor();
          cur.onsuccess = () => {
            const c = cur.result;
            if (!c) {
              db.close();
              return resolve(out);
            }
            out[String(c.key)] = c.value;
            c.continue();
          };
        };
      }),
  );
}
async function plant(page: Page, records: Records) {
  await page.evaluate(
    (recs) =>
      new Promise<void>((resolve) => {
        const req = indexedDB.open("sawhorse", 1);
        req.onupgradeneeded = () => req.result.createObjectStore("autosave");
        req.onsuccess = () => {
          const db = req.result;
          const tx = db.transaction("autosave", "readwrite");
          tx.objectStore("autosave").clear();
          for (const [k, v] of Object.entries(recs)) tx.objectStore("autosave").put(v, k);
          tx.oncomplete = () => {
            db.close();
            resolve();
          };
        };
      }),
    records,
  );
}
const record = (log: string, savedAt: number, mark: boolean) => ({ savedAt, solids: 4, steps: JSON.parse(log).length, bytes: log.length, worth: true, ...(mark ? { rules: 1 } : {}) });
type G = { sketches: [number, number | null][]; nodes: { id: number; kind: string }[] };
const graph = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { graph(): G } }).__sawhorse.graph());
const pushed = async (page: Page) => {
  const s = await page.evaluate(() => (window as unknown as { __sawhorse: { solids(): { min: number[]; max: number[] }[] } }).__sawhorse.solids());
  return s.map((x) => x.min[2]).sort();
};

async function start(page: Page, records: Records) {
  await page.goto("/?io=basic");
  await loaded(page);
  await plant(page, records);
  await page.reload();
  return loaded(page);
}

test("unmarked work from an older build that the change would restore differently is refused, kept and offered until discarded", async ({ page }) => {
  const hint = page.locator("#hint");
  const log = olderLog(true);
  // An hour ahead, so it is offered before the model this test sets aside.
  const vp = await start(page, { offered: log, "offered-meta": record(log, Date.now() + 3_600_000, false) });
  await expect(hint).toContainText("Restore unsaved work? 4 solids, 15 steps");
  // A model of this session's own, open when Restore is asked for.
  await box(page);
  await expect(vp).toHaveAttribute("data-objects", "1");
  const before = await vp.getAttribute("data-hash");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Restore Unsaved Work" }).click();
  await expect(hint).toContainText(REFUSED);
  await expect(hint.getByRole("button", { name: "Discard" })).toBeVisible();
  await expect(vp).toHaveAttribute("data-objects", "1");
  await expect(vp).toHaveAttribute("data-hash", before!);
  // Still set aside after a reload, and offered first.
  await page.reload();
  await loaded(page);
  await expect(hint).toContainText("Restore unsaved work? 4 solids, 15 steps");
  await expect(hint).toContainText("1 older set of unsaved work is set aside too.");
  expect((await stored(page)).offered).toBe(log);
  // Discard removes that offer alone: the box drawn here is still set aside.
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid");
  const left = await stored(page);
  expect(Object.values(left)).not.toContain(log);
  expect(Object.keys(left).filter((k) => k.endsWith("-meta"))).toHaveLength(1);
});

test("the same log with the mark restores, the line kept in the copy", async ({ page }) => {
  const hint = page.locator("#hint");
  const log = olderLog(true);
  const vp = await start(page, { offered: log, "offered-meta": record(log, Date.now() - 60_000, true) });
  await hint.getByRole("button", { name: "Restore" }).click();
  await expect(hint).toContainText("Restored your unsaved work: 3 solids, 15 steps");
  await expect(vp).toHaveAttribute("data-objects", "3");
  expect((await graph(page)).sketches).toContainEqual([2, 3]);
});

test("unmarked work from an older build that went through nothing changed restores as before", async ({ page }) => {
  const hint = page.locator("#hint");
  const log = olderLog(false);
  const vp = await start(page, { offered: log, "offered-meta": record(log, Date.now() - 60_000, false) });
  await hint.getByRole("button", { name: "Restore" }).click();
  await expect(hint).toContainText("Restored your unsaved work: 3 solids, 14 steps");
  await expect(vp).toHaveAttribute("data-objects", "3");
  // The push is on the rectangle at z 4, where the older build put it.
  expect(await pushed(page)).toContain(4);
});

test("work made in this build is set aside with the mark and restores", async ({ page }) => {
  const hint = page.locator("#hint");
  await page.goto("/?io=basic");
  const vp = await loaded(page);
  await plant(page, {});
  await box(page);
  await expect.poll(async () => (await stored(page))["current-meta"]).toMatchObject({ steps: 3, rules: 1 });
  await page.reload();
  await loaded(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  expect((await stored(page))["offered-meta"]).toMatchObject({ rules: 1 });
  await hint.getByRole("button", { name: "Restore" }).click();
  await expect(hint).toContainText("Restored your unsaved work: 1 solid, 3 steps");
  await expect(vp).toHaveAttribute("data-objects", "1");
});
