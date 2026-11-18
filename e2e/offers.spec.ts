import { expect, test, type Page } from "@playwright/test";
import { box, near, solids, type V3 } from "./helpers";

// Lane 64 (#94): unsaved work kept in this browser is never lost to an offer nobody answered.
// Before, an unanswered "Restore unsaved work?" stayed the offer at every later start, the
// newer work of the session in between was never offered, and Restore of the old offer wrote
// the old work over it about 300 ms later. Every piece of work is set aside now (src/lib/
// autosave.ts), offered newest first, kept until it is answered, at most five; Restore sets
// aside the model it replaces (moved, not copied); the status line says so while work is set aside and its offer
// is not on the line. The page is reloaded for each start (a closed window keeps its storage
// the same way). Three pieces of work, told apart by their size and their steps (Undo and
// Redo are steps of the log):
//   A: 12 x 12 x 6 in, 3 steps; B: 24 x 16 x 12 in, 5 steps; C: 8 x 8 x 4 in, 7 steps.
const requireKernel = !!process.env.REQUIRE_KERNEL;
const A: V3 = [12, 12, 6];
const B: V3 = [24, 16, 12];
const C: V3 = [8, 8, 4];

async function loaded(page: Page) {
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

async function ready(page: Page, url = "/?io=basic") {
  await page.goto(url);
  return loaded(page);
}

/** A new start of the app with this browser's storage as it is. */
async function restart(page: Page) {
  await page.reload();
  return loaded(page);
}

/** What the autosave store holds: this session's steps, and each offer's steps by id. */
async function kept(page: Page): Promise<{ current: number | null; offers: Record<string, number | string> }> {
  return page.evaluate(
    () =>
      new Promise<{ current: number | null; offers: Record<string, number | string> }>((resolve) => {
        const req = indexedDB.open("sawhorse", 1);
        req.onupgradeneeded = () => req.result.createObjectStore("autosave");
        req.onsuccess = () => {
          const db = req.result;
          const s = db.transaction("autosave", "readonly").objectStore("autosave");
          const out = { current: null as number | null, offers: {} as Record<string, number | string> };
          const cur = s.openCursor();
          cur.onsuccess = () => {
            const c = cur.result;
            if (!c) {
              db.close();
              return resolve(out);
            }
            const k = String(c.key);
            const steps = (c.value as { steps?: unknown } | null)?.steps;
            if (k === "current-meta") out.current = typeof steps === "number" ? steps : null;
            const m = /^(offered(?:-\d+)?)-meta$/.exec(k);
            if (m) out.offers[m[1]] = typeof steps === "number" ? steps : "unreadable";
            c.continue();
          };
        };
      }),
  );
}
const offerSteps = async (page: Page) => Object.values((await kept(page)).offers).sort();

/** Raw records, as an older build (or an earlier start) left them. */
async function plant(page: Page, records: Record<string, unknown>) {
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

/** The raw log and record this session autosaved. */
async function currentRecords(page: Page): Promise<{ log: string; meta: { savedAt: number; solids: number; steps: number; bytes: number; worth: boolean } }> {
  return page.evaluate(
    () =>
      new Promise((resolve) => {
        const req = indexedDB.open("sawhorse", 1);
        req.onsuccess = () => {
          const db = req.result;
          const s = db.transaction("autosave", "readonly").objectStore("autosave");
          const log = s.get("current");
          const meta = s.get("current-meta");
          meta.onsuccess = () => {
            db.close();
            resolve({ log: log.result, meta: meta.result });
          };
        };
      }),
  );
}

/** A, B or C, drawn in this session and autosaved. */
async function draw(page: Page, which: "A" | "B" | "C") {
  const [size, h, history] = which === "A" ? ["12,12", "6", 0] : which === "B" ? ["24,16", "12", 1] : ["8,8", "4", 2];
  await box(page, size, h);
  const vp = page.locator("main.vp");
  for (let i = 0; i < history; i++) {
    await page.keyboard.press("Control+z");
    await expect(vp).toHaveAttribute("data-objects", "0");
    await page.keyboard.press("Control+y");
    await expect(vp).toHaveAttribute("data-objects", "1");
  }
  await expect.poll(async () => (await kept(page)).current).toBe(3 + 2 * history);
}

/** The model is exactly one solid, `size` from the origin. */
async function shows(page: Page, size: V3) {
  await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "1");
  await expect.poll(async () => (await solids(page))[0]?.max.map((x) => Math.round(x * 1000) / 1000)).toEqual(size);
  near((await solids(page))[0].min, [0, 0, 0]);
}

async function editRestore(page: Page) {
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Restore Unsaved Work" }).click();
}

const reminder = (page: Page) => page.getByTestId("hint-offers");

test("the issue's three starts: the newer work is offered first, Restore of one leaves the other, and both come back", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  await draw(page, "A");
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  // The offer is left unanswered and B is drawn: the offer line is gone, the reminder is there.
  await draw(page, "B");
  await expect(hint).not.toContainText("Restore unsaved work?");
  await expect(reminder(page)).toHaveText("Unsaved work set aside. Click to restore or discard.");
  await restart(page);
  // Before lane 64 this offered A again, and B was never offered.
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 5 steps, saved just now. 1 older set of unsaved work is set aside too.");
  expect(await offerSteps(page)).toEqual([3, 5]);
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, B);
  await expect(hint).toContainText("Restored your unsaved work: 1 solid, 5 steps (undo works as before).");
  await expect(hint).not.toContainText("set aside as unsaved work");
  // Well past the autosave after Restore (300 ms): A is still set aside.
  await page.waitForTimeout(800);
  expect(await kept(page)).toEqual({ current: 5, offers: { offered: 3 } });
  await expect(reminder(page)).toHaveText("Unsaved work set aside. Click to restore or discard.");
  // A comes back from the Edit menu, and B (the model it replaces) is set aside in its turn.
  await editRestore(page);
  await shows(page, A);
  await expect(hint).toContainText("Restored your unsaved work: 1 solid, 3 steps (undo works as before). The model you had open is set aside as unsaved work: Edit > Restore Unsaved Work brings it back.");
  await page.waitForTimeout(800);
  const k = await kept(page);
  expect(k.current).toBe(3);
  expect(Object.values(k.offers)).toEqual([5]);
  // The next start offers both, and each comes back.
  await restart(page);
  expect(await offerSteps(page)).toEqual([3, 5]);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, A);
  await editRestore(page);
  await shows(page, B);
});

test("Discard of the newer work offers the older at once", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  await draw(page, "A");
  await restart(page);
  await draw(page, "B");
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 5 steps");
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps, saved just now.");
  await expect(hint).not.toContainText("older");
  expect(await offerSteps(page)).toEqual([3]);
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, A);
  await page.waitForTimeout(800);
  expect(await kept(page)).toEqual({ current: 3, offers: {} });
  await expect(reminder(page)).toHaveCount(0);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(page.getByRole("menuitem", { name: "Restore Unsaved Work" })).toBeDisabled();
});

test("edits while an offer waits, then a close: all three pieces of work are still there", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  await draw(page, "A");
  await restart(page);
  await draw(page, "B");
  await restart(page);
  await draw(page, "C");
  await expect(reminder(page)).toHaveText("Unsaved work set aside (2). Click to restore or discard.");
  // The reminder brings the offer back to the line.
  await reminder(page).click();
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 5 steps");
  await expect(reminder(page)).toHaveCount(0);
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 7 steps, saved just now. 2 older sets of unsaved work are set aside too.");
  expect(await offerSteps(page)).toEqual([3, 5, 7]);
  // Each comes back: C from the offer, then B and A from the Edit menu, each in turn setting
  // aside the one it replaces.
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, C);
  await editRestore(page);
  await shows(page, B);
  await editRestore(page);
  await shows(page, A);
  await page.waitForTimeout(800);
  const k = await kept(page);
  expect(k.current).toBe(3);
  expect(Object.values(k.offers).sort()).toEqual([5, 7]);
  await restart(page);
  expect(await offerSteps(page)).toEqual([3, 5, 7]);
});

test("Save, New and Open while an offer waits never delete it, and the reminder stays; Restore over an open file sets its edits aside", async ({ page }) => {
  const hint = page.locator("#hint");
  const vp = await ready(page);
  await draw(page, "A");
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  await draw(page, "B");
  // Save.
  const [d] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  const fs = await import("node:fs");
  const saved = fs.readFileSync((await d.path())!);
  await expect(hint).toContainText("Saved");
  await expect(reminder(page)).toBeVisible();
  await page.waitForTimeout(600);
  expect(await kept(page)).toEqual({ current: null, offers: { offered: 3 } });
  // New.
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "New", exact: true }).click();
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(reminder(page)).toBeVisible();
  await page.waitForTimeout(600);
  expect(await kept(page)).toEqual({ current: null, offers: { offered: 3 } });
  // Open.
  await page.getByTestId("file-input").setInputFiles({ name: "shelf.shz", mimeType: "application/octet-stream", buffer: saved });
  await expect(hint).toContainText("Opened shelf.shz");
  await expect(page).toHaveTitle("shelf - Sawhorse");
  await shows(page, B);
  await expect(reminder(page)).toBeVisible();
  await page.waitForTimeout(600);
  expect(await kept(page)).toEqual({ current: null, offers: { offered: 3 } });
  // An edit to the file, then Restore from the Edit menu: the work comes back untitled (Save
  // never writes it over shelf.shz), and the edited file's model is set aside.
  await page.keyboard.press("Space");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  await expect(vp).toHaveAttribute("data-objects", "0");
  await expect(page).toHaveTitle("• shelf - Sawhorse");
  await editRestore(page);
  await shows(page, A);
  await expect(page).toHaveTitle("• Untitled - Sawhorse");
  await expect(hint).toContainText("The model you had open is set aside as unsaved work");
  await page.waitForTimeout(800);
  const k = await kept(page);
  expect(k.current).toBe(3);
  expect(Object.keys(k.offers)).toHaveLength(1);
});

test("Restore from the Edit menu after drawing in this session loses nothing", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  await draw(page, "A");
  await restart(page);
  await draw(page, "B");
  await editRestore(page);
  await shows(page, A);
  await expect(hint).toContainText("The model you had open is set aside as unsaved work: Edit > Restore Unsaved Work brings it back.");
  await expect(reminder(page)).toHaveText("Unsaved work set aside. Click to restore or discard.");
  await page.waitForTimeout(800);
  let k = await kept(page);
  expect(k.current).toBe(3);
  expect(Object.values(k.offers)).toEqual([5]);
  // And back: B is the model again, A set aside.
  await editRestore(page);
  await shows(page, B);
  await page.waitForTimeout(800);
  k = await kept(page);
  expect(k.current).toBe(5);
  expect(Object.values(k.offers)).toEqual([3]);
  // Undo works on the restored work as before.
  await page.keyboard.press("Control+z");
  await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "0");
});

test("what older builds left is read with nothing lost: current only, offered only, both, and a pre-lane-16 record", async ({ page }) => {
  const hint = page.locator("#hint");
  // Real logs: A (3 steps), then the same with Undo and Redo (5 steps).
  await ready(page, "/?io=basic");
  await box(page, "12,12", "6");
  await expect.poll(async () => (await kept(page)).current).toBe(3);
  const three = await currentRecords(page);
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+y");
  await expect.poll(async () => (await kept(page)).current).toBe(5);
  const five = await currentRecords(page);
  const ago = (mins: number) => Date.now() - mins * 60000;

  // current only.
  await plant(page, { current: three.log, "current-meta": { ...three.meta, savedAt: ago(5) } });
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps, saved 5 minutes ago.");
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, A);
  await page.waitForTimeout(600);

  // offered only: an offer an older build left unanswered.
  await plant(page, { offered: three.log, "offered-meta": { ...three.meta, savedAt: ago(30) } });
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps, saved 30 minutes ago.");
  await expect(hint).not.toContainText("older");
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, A);
  await page.waitForTimeout(600);

  // Both pairs (the issue's state): the newer, in current, first; the older after it.
  await plant(page, { offered: three.log, "offered-meta": { ...three.meta, savedAt: ago(30) }, current: five.log, "current-meta": { ...five.meta, savedAt: ago(5) } });
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 5 steps, saved 5 minutes ago. 1 older set of unsaved work is set aside too.");
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps, saved 30 minutes ago.");
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, A);
  await page.waitForTimeout(600);

  // Before lane 16: a log array in current and no record beside it.
  await plant(page, { current: { log: JSON.parse(three.log), savedAt: ago(90), solids: 1 } });
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps, saved 90 minutes ago.");
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, A);
});

test("at most five sets of unsaved work are kept: the oldest goes, and the start says so", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  await draw(page, "A");
  const three = await currentRecords(page);
  const ago = (mins: number) => Date.now() - mins * 60000;
  // Six different logs (an exact copy would be kept once): the same steps, n spaces after.
  const v = (n: number) => three.log + " ".repeat(n);
  const at = (n: number, savedAt: number) => ({ ...three.meta, bytes: v(n).length, savedAt });
  const records: Record<string, unknown> = { offered: v(1), "offered-meta": at(1, ago(180)) };
  for (let n = 2; n <= 5; n++) {
    records[`offered-${n}`] = v(n);
    records[`offered-${n}-meta`] = at(n, ago(60 - n));
  }
  await plant(page, { ...records, current: v(6), "current-meta": at(6, ago(1)) });
  await restart(page);
  await expect(hint).toContainText(
    "Restore unsaved work? 1 solid, 3 steps, saved 1 minute ago. 4 older sets of unsaved work are set aside too. At most 5 sets of unsaved work are kept: the oldest (1 solid, 3 steps, saved 3 hours ago) was deleted.",
  );
  const k = await kept(page);
  expect(Object.keys(k.offers).sort()).toEqual(["offered-2", "offered-3", "offered-4", "offered-5", "offered-6"]);
  // Said once: after a Discard the next offer does not repeat it.
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps, saved 55 minutes ago. 3 older sets");
  await expect(hint).not.toContainText("was deleted");
});

test("?autosave=0 offers nothing, reminds of nothing and leaves what is kept as it is", async ({ page }) => {
  await ready(page);
  await draw(page, "A");
  const three = await currentRecords(page);
  await plant(page, { offered: three.log, "offered-meta": three.meta, current: three.log, "current-meta": three.meta });
  await ready(page, "/?autosave=0&io=basic");
  await page.waitForTimeout(500);
  await expect(page.locator("#hint")).not.toContainText("Restore unsaved work?");
  await box(page);
  await expect(reminder(page)).toHaveCount(0);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(page.getByRole("menuitem", { name: "Restore Unsaved Work" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(600);
  expect(await kept(page)).toEqual({ current: 3, offers: { offered: 3 } });
});

test("Restore with other sets waiting says the model set aside comes back after them, and it does", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  await draw(page, "A");
  await restart(page);
  await draw(page, "B");
  await restart(page);
  // Offered: B, then A. C is drawn while the offer waits.
  await draw(page, "C");
  await reminder(page).click();
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, B);
  await expect(hint).toContainText("The model you had open is set aside as unsaved work: Edit > Restore Unsaved Work brings it back after 1 other set.");
  await editRestore(page);
  await shows(page, A);
  await expect(hint).toContainText("The model you had open is set aside as unsaved work: Edit > Restore Unsaved Work brings it back after 1 other set.");
  await editRestore(page);
  await shows(page, C);
  await expect(hint).toContainText("The model you had open is set aside as unsaved work: Edit > Restore Unsaved Work brings it back after 1 other set.");
  await page.waitForTimeout(800);
  const k = await kept(page);
  expect(k.current).toBe(7);
  expect(Object.values(k.offers).sort()).toEqual([3, 5]);
  // The next start orders by the time each was kept: C (autosaved last), then A (set aside by
  // the last Restore), then B, not the session's order.
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 7 steps");
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 3 steps");
  await hint.getByRole("button", { name: "Discard" }).click();
  await expect(hint).toContainText("Unsaved work discarded. Restore unsaved work? 1 solid, 5 steps");
});

test("the count after Restore leaves out sets the Edit menu cannot restore", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  await draw(page, "B");
  const five = await currentRecords(page);
  const ago = (mins: number) => Date.now() - mins * 60000;
  // B, one set over 64 MB and one that cannot be read; C is drawn while they wait.
  await plant(page, {
    offered: five.log,
    "offered-meta": { ...five.meta, savedAt: ago(5) },
    "offered-2": "[]",
    "offered-2-meta": { savedAt: ago(10), solids: 3, steps: 9, bytes: 100 * 1024 * 1024, worth: true },
    "offered-3": 12345,
    "offered-3-meta": "garbage",
  });
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 5 steps, saved 5 minutes ago. 2 older sets of unsaved work are set aside too.");
  await draw(page, "C");
  await reminder(page).click();
  await hint.getByRole("button", { name: "Restore" }).click();
  await shows(page, B);
  // Before, "after 2 other sets": the menu skips both, so C is next.
  await expect(hint).toContainText("The model you had open is set aside as unsaved work: Edit > Restore Unsaved Work brings it back.");
  await expect(hint).not.toContainText("other set");
  await editRestore(page);
  await shows(page, C);
  await expect(hint).toContainText("The model you had open is set aside as unsaved work: Edit > Restore Unsaved Work brings it back.");
  await editRestore(page);
  await shows(page, B);
  await page.waitForTimeout(800);
  const k = await kept(page);
  expect(k.current).toBe(5);
  expect(Object.values(k.offers).sort()).toEqual([7, 9, "unreadable"]);
});

test("a Restore the kernel refuses leaves the open model kept once, as this session's autosave", async ({ page }) => {
  const hint = page.locator("#hint");
  await ready(page);
  // A record that reads well over a log the kernel refuses.
  const bad = '[{"op":"no_such_op"}]';
  await plant(page, { offered: bad, "offered-meta": { savedAt: Date.now() - 60000, solids: 1, steps: 1, bytes: bad.length, worth: true } });
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 1 steps");
  await draw(page, "B");
  await editRestore(page);
  await expect(hint).toContainText("Could not restore the unsaved work");
  await shows(page, B);
  // Before, B was kept twice: in current and in a pair of its own.
  await page.waitForTimeout(800);
  expect(await kept(page)).toEqual({ current: 5, offers: { offered: 1 } });
  await restart(page);
  expect(await offerSteps(page)).toEqual([1, 5]);
});

test("the restored work is this session's autosave the moment Restore says so, before any autosave runs", async ({ page }) => {
  const hint = page.locator("#hint");
  await page.clock.install();
  await ready(page);
  await draw(page, "A");
  const three = await currentRecords(page);
  const savedAt = Date.now() - 30 * 60000;
  await plant(page, { offered: three.log, "offered-meta": { ...three.meta, savedAt } });
  await restart(page);
  await expect(hint).toContainText("Restore unsaved work? 1 solid, 3 steps");
  // Timers stop: the autosave that follows an edit (300 ms) cannot run until they go on.
  await page.clock.pauseAt(Date.now() + 1000);
  await hint.getByRole("button", { name: "Restore" }).click();
  await expect(hint).toContainText("Restored your unsaved work: 1 solid, 3 steps");
  // The offer's own record (its time), moved into current: not a fresh autosave.
  const now = await currentRecords(page);
  expect(now.log).toBe(three.log);
  expect(now.meta.savedAt).toBe(savedAt);
  expect((await kept(page)).offers).toEqual({});
  await page.clock.resume();
});
