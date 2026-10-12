#!/usr/bin/env node
// Lane-4 lane-end walk-through, driven like a person at the keyboard and mouse (every
// click waits for the inference it should land on), with a screenshot after each stage:
//
//   1. a table leg: a 2 x 2 x 28 in post, its top face scaled to 0.6 about its centre
//      (Scale, Ctrl), turned end for end (Rotate 180 about red) so the taper is at the
//      foot, then a 1 in tenon: the top offset 1/2 in inward (Offset) and pulled up 1 in;
//   2. four copies with a Move array (Ctrl, 20 in along red, x4);
//   3. one leg turned 90 degrees about blue;
//   4. a chamfer moulding routed round the top of a 24 x 12 x 8 box (Line, Follow Me).
//
//   npm run dev -- --port 5184   (in another terminal)
//   node scripts/capture-lane04.mjs --url http://localhost:5184 --out ../_captures
//
// Uses the installed Edge (PW_CHANNEL=msedge) so nothing is downloaded.
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5184";
const outDir = path.resolve(args.out ?? "../_captures");
const prefix = args.prefix ?? "2026-10-11-lane04";
const IN = 0.0254;
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const log = [];
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  page.setDefaultTimeout(15_000);
  await page.goto(`${url}/?theme=light&autosave=0`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });

  const vp = page.locator("main.vp");
  const hint = page.locator("#hint");
  const at = async (p, dx = 0, dy = 0) => {
    const box = await page.locator("#cv").boundingBox();
    const xy = await page.evaluate((m) => window.__sawhorse.worldToScreen(m), p.map((v) => v * IN));
    return [box.x + xy[0] + dx, box.y + xy[1] + dy];
  };
  const waitAttr = async (name, want, timeout = 800) => {
    const re = want instanceof RegExp ? want : new RegExp(`^${want}$`);
    const t0 = Date.now();
    while (Date.now() - t0 < timeout) {
      if (re.test((await vp.getAttribute(name)) ?? "")) return true;
      await page.waitForTimeout(40);
    }
    return false;
  };
  /** Move to a point (inches) until the attribute shows `want` (the view may refit). */
  const hover = async (p, want, dx = 1, dy = -1, attr = "data-inference") => {
    for (let k = 0; k < 15; k++) {
      const [x, y] = await at(p, dx, dy);
      await page.mouse.move(x + 3, y + 2);
      await page.mouse.move(x, y, { steps: 2 });
      if (await waitAttr(attr, want)) return [x, y];
    }
    throw new Error(`no ${want} at ${p} (${attr} = ${await vp.getAttribute(attr)})`);
  };
  const clickAt = async (p, want, dx = 1, dy = -1) => {
    const [x, y] = await hover(p, want, dx, dy);
    await page.mouse.click(x, y);
  };
  const selectAt = async (p, want, dx = 1, dy = -1) => {
    const [x, y] = await hover(p, want, dx, dy, "data-hover");
    await page.mouse.click(x, y);
  };
  const point = async (p) => {
    const [x, y] = await at(p);
    await page.mouse.move(x, y, { steps: 4 });
  };
  const type = async (text) => {
    await page.keyboard.type(text);
    await page.keyboard.press("Enter");
  };
  const view = (name) => page.getByRole("button", { name, exact: true }).click();
  /** Roll the wheel over a point (inches) to zoom toward it. */
  const zoomAt = async (p, clicks = 5) => {
    for (let k = 0; k < clicks; k++) {
      const [x, y] = await at(p);
      await page.mouse.move(x, y);
      await page.mouse.wheel(0, -240);
      await page.waitForTimeout(60);
    }
  };
  const expectHint = async (text) => {
    await page.waitForFunction((t) => document.getElementById("hint")?.textContent?.includes(t), text, { timeout: 10_000 });
    log.push((await hint.textContent()).trim());
  };
  const shot = async (name) => {
    await page.mouse.move(5, 5);
    await page.waitForTimeout(250);
    const file = path.join(outDir, `${prefix}-${name}.png`);
    await page.screenshot({ path: file });
    const solids = await page.evaluate(() => window.__sawhorse.solids());
    log.push(`[${name}] ${file} :: ${solids.length} solids, ${solids.map((s) => `#${s.number} ${(s.volume / 0.0254 ** 3).toFixed(3)} in3`).join(", ")}`);
  };
  /** Double-click a solid's face to select the whole solid (the first of `ps` that shows a face). */
  const solidAt = async (...ps) => {
    await page.keyboard.press("Space");
    await page.keyboard.press("Escape");
    for (const p of ps) {
      const [x, y] = await at(p, 1, -1);
      await page.mouse.move(x + 3, y + 2);
      await page.mouse.move(x, y, { steps: 2 });
      if (await waitAttr("data-hover", "face", 1500)) {
        await page.mouse.dblclick(x, y);
        await expectHint("Selected solid");
        return;
      }
    }
    throw new Error(`no face to select at ${JSON.stringify(ps)}`);
  };

  // 1. The leg.
  await page.keyboard.press("r");
  await type("2,2");
  await page.keyboard.press("p");
  await type("28");
  await expectHint("Extruded 28 in");
  await page.keyboard.press("Shift+Z");
  // Taper: the top face to 0.6 of its size about its centre.
  await view("Top");
  await page.keyboard.press("Shift+Z");
  await zoomAt([1, 1, 28], 6);
  await page.keyboard.press("Space");
  await selectAt([1, 1, 28], "face");
  await page.keyboard.press("s");
  await page.waitForFunction(() => document.querySelectorAll(".vlabel.grip").length === 8);
  await page.keyboard.press("Control");
  await expectHint("Scaling about the centre");
  for (let k = 0; k < 15; k++) {
    const [x, y] = await at([2, 2, 28]);
    await page.mouse.move(x + 3, y + 2);
    await page.mouse.move(x, y, { steps: 2 });
    if ((await page.locator(".vlabel.grip.hot").count()) === 1) break;
  }
  const [gx, gy] = await at([2, 2, 28]);
  await page.mouse.click(gx, gy);
  await page.mouse.move(gx - 10, gy + 10, { steps: 3 });
  await type("0.6");
  await expectHint("Scaled face 2 of solid 1 ×0.6 along red and green about its centre");
  await view("Iso");
  await page.keyboard.press("Shift+Z");
  await shot("1-scale-taper");
  // End for end: 180 degrees about red through mid-height of its front right edge, so the
  // taper is at the foot (the leg lands 2 in toward -green: y from -2 to 0).
  await solidAt([1, 0, 14]);
  await page.keyboard.press("q");
  await page.keyboard.press("ArrowRight");
  await clickAt([2, 0, 14], "midpoint", -1, 0);
  // A half turn needs no start direction: type it straight after the centre.
  await type("180");
  await expectHint("Turned solid 1 180° about the red axis");
  // The tenon: the (new) top offset 1/2 in inward, pulled up 1 in.
  await page.keyboard.press("Escape");
  await page.keyboard.press("f");
  // From above: press inside the top face near its front edge, then move toward its middle.
  await view("Top");
  await page.keyboard.press("Shift+Z");
  await zoomAt([1, -1, 28], 6);
  await clickAt([0.6, -1.7, 28], /on-face|on-edge/, 0, 0);
  await point([1, -1, 28]);
  await type("1/2");
  await expectHint("1/2 in inward, on the solid");
  await page.keyboard.press("p");
  await type("1");
  await expectHint("Pulled 1 in");
  await view("Iso");
  await page.keyboard.press("Shift+Z");
  await shot("2-leg-offset-tenon");

  // 2. Four copies, 20 in apart along red.
  await solidAt([1, -1.8, 14]);
  await page.keyboard.press("m");
  await page.keyboard.press("Control");
  await clickAt([0, -2, 28], "endpoint", 1, -1);
  await page.keyboard.press("ArrowRight");
  await point([10, -2, 28]);
  await type("20");
  await expectHint("Copied solid 1 20 in along +red");
  await type("x4");
  await expectHint("Array: 4 copies of solid 1, 20 in along +red apart");
  await page.keyboard.press("Shift+Z");
  await shot("3-move-array-x4");

  // 3. Turn the last leg 90 degrees about blue, about its own corner.
  // Nothing selected: Rotate turns the solid its centre is clicked on.
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.keyboard.press("q");
  await page.keyboard.press("ArrowUp");
  await clickAt([80, -2, 28], "endpoint", 1, -1);
  await clickAt([82, -2, 28], "endpoint", -1, -1);
  await point([80, 6, 28]);
  await type("90");
  await expectHint("90° about the blue axis");
  await page.keyboard.press("Shift+Z");
  await shot("4-rotate-90");

  // 4. A chamfer moulding round the top of a 24 x 12 x 8 box, 30 in behind the legs.
  // The box at the origin (typed), then moved back along green until it starts 30 in
  // behind the legs, picked up by its top corner. Positions come from the kernel's bounds.
  const last = async () => {
    const all = await page.evaluate(() => window.__sawhorse.solids());
    const b = all[all.length - 1];
    return { lo: b.min.map((m) => m / IN), hi: b.max.map((m) => m / IN) };
  };
  await page.keyboard.press("Escape");
  await page.keyboard.press("r");
  await type("24,12");
  await page.keyboard.press("p");
  await type("8");
  await expectHint("Extruded 8 in");
  await page.keyboard.press("Shift+Z");
  let b = await last();
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await page.keyboard.press("m");
  const want = 30 - b.lo[1];
  for (const ahead of [6, 15, 30]) {
    await clickAt(b.hi, "endpoint", -1, -1);
    await page.keyboard.press("ArrowLeft");
    await point([b.hi[0], b.hi[1] + ahead, b.hi[2]]);
    await type(`${want}`);
    await page.waitForFunction(() => /Moved solid/.test(document.getElementById("hint")?.textContent ?? ""));
    const moved = await last();
    if (Math.abs(moved.lo[1] - 30) < 1e-6) break;
    // The cursor showed the other way along green: take it back and point further out.
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(300);
  }
  log.push((await hint.textContent()).trim());
  b = await last();
  const [x1, y0, z1] = [b.hi[0], b.lo[1], b.hi[2]];
  // The profile: a 2 in chamfer at the top front corner of the right end, drawn with Line.
  await view("Right");
  await page.keyboard.press("Shift+Z");
  await zoomAt([x1, y0 + 2, z1 - 2], 3);
  await page.keyboard.press("l");
  await clickAt([x1, y0 + 2, z1 - 0.03], "on-edge", 0, 0);
  await clickAt([x1, y0 + 0.03, z1 - 2], "on-edge", 0, 0);
  await expectHint("Cut on face");
  // The path: the top face's edge, all the way round; then click the profile.
  await view("Iso");
  await page.keyboard.press("Shift+Z");
  await page.keyboard.press("Space");
  await page.keyboard.press("Escape");
  await selectAt([(b.lo[0] + b.hi[0]) / 2, (b.lo[1] + b.hi[1]) / 2, z1], "face");
  await page.keyboard.press("w");
  await expectHint("closed, 4 segments");
  await view("Right");
  await page.keyboard.press("Shift+Z");
  await zoomAt([x1, y0 + 2, z1 - 2], 3);
  await clickAt([x1, y0 + 2 / 3, z1 - 2 / 3], /on-face|midpoint|on-edge/, 0, 0);
  await expectHint("cut from solid");
  // A close look at the routed edge, then everything.
  await view("Iso");
  await page.keyboard.press("Shift+Z");
  await zoomAt([x1, y0, z1], 9);
  await shot("5-follow-me-moulding");
  await view("Persp");
  await page.keyboard.press("Shift+Z");
  await view("Iso");
  await page.keyboard.press("Shift+Z");
  await shot("6-overview");
  log.push(`state: ${await vp.getAttribute("data-objects")} solids, watertight ${await vp.getAttribute("data-watertight")}`);
} catch (err) {
  // Show where it stopped: the status line and a picture (not a lane-end capture).
  const page = browser.contexts()[0]?.pages()[0];
  if (page) {
    log.push(`STOPPED: ${(await page.locator("#hint").textContent().catch(() => "")) ?? ""}`);
    await page.screenshot({ path: path.join(process.env.TEMP ?? ".", `${prefix}-stopped.png`) }).catch(() => undefined);
  }
  process.exitCode = 1;
  log.push(String(err));
} finally {
  console.log(log.join("\n"));
  await browser.close();
}
