import { expect, test, type Page } from "@playwright/test";
import { IN, box, ready, type V3 } from "./helpers";

// Lane 21 (issue #5): middle-drag orbits about the point under the cursor at the press,
// pinned to its screen position for the whole drag (SketchUp's pivot), with a small marker
// while the button is down; no lock near the poles; right-drag pans with the point under
// the cursor; the standard views and zoom extents still land where they did. The drags are
// synthetic pointer events (no real pointer behind them), so pointer capture is stubbed
// first, as three.js OrbitControls needed; the controller itself never asks for it.

interface Cam {
  eye: V3;
  target: V3;
  projection: string;
  view: string | null;
  zoom: number;
}
type Hooks = {
  worldToScreen(p: V3): [number, number];
  camera(): Cam;
  navPick(x: number, y: number, snap?: boolean): V3 | null;
  nav(): { mode: string | null; pivot: V3 | null };
};

const cam = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: Hooks }).__sawhorse.camera());
const screen = (page: Page, p: V3) => page.evaluate((p) => (window as unknown as { __sawhorse: Hooks }).__sawhorse.worldToScreen(p), p);

/** Stub pointer capture (synthetic pointers have no live pointer to capture). */
async function stubCapture(page: Page) {
  await page.evaluate(() => {
    HTMLElement.prototype.setPointerCapture = () => undefined;
    HTMLElement.prototype.releasePointerCapture = () => undefined;
  });
}

/**
 * Dispatch a synthetic pointer gesture on the canvas (canvas CSS pixels): `down` at the
 * first point, a move to each of the others, `up` at the last unless `hold`. Returns the
 * largest distance, after any move, between `probe`'s projection and where it was at the
 * press (the drift a pinned pivot must not have), and the probe's last screen position.
 */
async function gesture(page: Page, button: 1 | 2, pts: [number, number][], opts: { probe?: V3; shift?: boolean; hold?: boolean; skipDown?: boolean } = {}) {
  return page.evaluate(
    ({ button, pts, probe, shift, hold, skipDown }) => {
      const h = (window as unknown as { __sawhorse: Hooks }).__sawhorse;
      const cv = document.getElementById("cv")!;
      const r = cv.getBoundingClientRect();
      const buttons = button === 1 ? 4 : 2;
      const fire = (type: string, [x, y]: [number, number], b: number, bs: number) =>
        cv.dispatchEvent(
          new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: r.left + x, clientY: r.top + y, button: b, buttons: bs, shiftKey: !!shift, pointerId: 1, pointerType: "mouse", isPrimary: true }),
        );
      if (!skipDown) fire("pointerdown", pts[0], button, buttons);
      const start = probe ? h.worldToScreen(probe) : null;
      let drift = 0;
      let last = start;
      for (const p of pts.slice(1)) {
        fire("pointermove", p, -1, buttons);
        if (probe && start) {
          last = h.worldToScreen(probe);
          drift = Math.max(drift, Math.hypot(last[0] - start[0], last[1] - start[1]));
        }
      }
      if (!hold) fire("pointerup", pts[pts.length - 1], button, 0);
      return { drift, start, last };
    },
    { button, pts, probe: opts.probe ?? null, shift: !!opts.shift, hold: !!opts.hold, skipDown: !!opts.skipDown },
  );
}

/** `n` steps from a to b, b included. */
function steps(a: [number, number], b: [number, number], n: number): [number, number][] {
  return Array.from({ length: n }, (_, i) => [a[0] + ((b[0] - a[0]) * (i + 1)) / n, a[1] + ((b[1] - a[1]) * (i + 1)) / n] as [number, number]);
}

const dirOf = (c: Cam): V3 => {
  const d: V3 = [c.eye[0] - c.target[0], c.eye[1] - c.target[1], c.eye[2] - c.target[2]];
  const l = Math.hypot(...d);
  return [d[0] / l, d[1] / l, d[2] / l];
};
const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

test("middle-drag orbits about the corner under the cursor, pinned to the pixel, with a marker; no lock at the poles; right-drag pans with it", async ({ page }) => {
  const vp = await ready(page);
  await box(page); // 12 x 12 x 6 in at the origin
  await page.keyboard.press("Escape");
  await stubCapture(page);

  // Zoom in on the box's top corner at (12, 0, 6) in: the wheel keeps it under the cursor.
  const corner: V3 = [12 * IN, 0, 6 * IN];
  const cv = (await page.locator("#cv").boundingBox())!;
  let c = await screen(page, corner);
  await page.mouse.move(cv.x + c[0], cv.y + c[1]);
  const z0 = (await cam(page)).zoom;
  for (let i = 0; i < 4; i++) await page.mouse.wheel(0, -300);
  await expect.poll(async () => (await cam(page)).zoom).toBeGreaterThan(z0 * 1.5);
  c = await screen(page, corner);
  // Zoomed in, the target (where the last zoom left it) is not the corner: OrbitControls
  // would have swung the corner off the cursor.
  const t = (await cam(page)).target;
  expect(Math.hypot(t[0] - corner[0], t[1] - corner[1], t[2] - corner[2])).toBeGreaterThan(0.01);

  // The pivot under the cursor is the corner itself (a vertex within 6 px wins).
  const at: [number, number] = [c[0] + 2, c[1] - 1];
  const picked = await page.evaluate(([x, y]) => (window as unknown as { __sawhorse: Hooks }).__sawhorse.navPick(x, y), at);
  expect(picked).not.toBeNull();
  for (let k = 0; k < 3; k++) expect(Math.abs(picked![k] - corner[k])).toBeLessThan(1e-6);

  // Orbit 200 px horizontally, holding the button: the marker shows at the corner.
  const across = await gesture(page, 1, [at, ...steps(at, [at[0] + 200, at[1]], 20)], { probe: corner, hold: true });
  await expect(vp).toHaveAttribute("data-nav", "orbit");
  const marker = page.getByTestId("orbit-pivot");
  await expect(marker).toBeVisible();
  const mb = (await marker.boundingBox())!;
  expect(Math.hypot(mb.x + mb.width / 2 - cv.x - c[0], mb.y + mb.height / 2 - cv.y - c[1])).toBeLessThan(1.5);
  // ... then 200 px vertically, same drag; let go.
  const down = await gesture(page, 1, [[at[0] + 200, at[1]], ...steps([at[0] + 200, at[1]], [at[0] + 200, at[1] + 200], 20)], { probe: corner, skipDown: true });
  await expect(marker).toHaveCount(0);
  await expect(vp).toHaveAttribute("data-nav", "");
  await expect(vp).toHaveAttribute("data-view", "custom");
  const end = await screen(page, corner);
  const moved = Math.hypot(end[0] - c[0], end[1] - c[1]);
  console.info(`[orbit] corner drift: across ${across.drift.toFixed(6)} px, down ${down.drift.toFixed(6)} px, net ${moved.toFixed(6)} px`);
  expect(across.drift).toBeLessThan(1);
  expect(down.drift).toBeLessThan(1);
  expect(moved).toBeLessThan(1);
  // The look-at point moved with the orbit: on the view direction at the corner's depth.
  const after = await cam(page);
  expect(after.target).not.toEqual(t);

  // Drag 2000 px down (far past the pole), then across: the view still turns.
  const p0: [number, number] = [c[0], c[1]];
  await gesture(page, 1, [p0, ...steps(p0, [p0[0], p0[1] + 2000], 40)], { hold: true });
  const atPole = await cam(page);
  const d0 = dirOf(atPole);
  // 89.5 degrees from the horizon at most: the eye is above, not over the top.
  expect(d0[2]).toBeGreaterThan(Math.sin((89 * Math.PI) / 180));
  expect(d0[2]).toBeLessThan(Math.sin((89.75 * Math.PI) / 180));
  await gesture(page, 1, [[p0[0], p0[1] + 2000], ...steps([p0[0], p0[1] + 2000], [p0[0] + 300, p0[1] + 2000], 15)], { skipDown: true });
  const turned = await cam(page);
  const d1 = dirOf(turned);
  // The horizontal part of the view direction turned (about 2.6 rad for 300 px).
  const h0: V3 = [d0[0], d0[1], 0];
  const h1: V3 = [d1[0], d1[1], 0];
  const cosTurn = dot(h0, h1) / (Math.hypot(...h0) * Math.hypot(...h1));
  console.info(`[orbit] after 2000 px down: view ${((Math.asin(d0[2]) * 180) / Math.PI).toFixed(3)} deg above the target; 300 px across then turned it ${((Math.acos(Math.max(-1, Math.min(1, cosTurn))) * 180) / Math.PI).toFixed(1)} deg`);
  expect(cosTurn).toBeLessThan(Math.cos(1));

  // Back to a usable view, zoomed toward the corner again, then right-drag pan.
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await expect(vp).toHaveAttribute("data-view", "iso");
  c = await screen(page, corner);
  await page.mouse.move(cv.x + c[0], cv.y + c[1]);
  for (let i = 0; i < 3; i++) await page.mouse.wheel(0, -300);
  await expect.poll(async () => (await cam(page)).zoom).toBeGreaterThan(1.3);
  c = await screen(page, corner);
  // The point a pan drags is the one exactly under the cursor (no snapping: nothing jumps).
  const grab: [number, number] = [c[0] - 3, c[1] + 4];
  const held = await page.evaluate(([x, y]) => (window as unknown as { __sawhorse: Hooks }).__sawhorse.navPick(x, y, false), grab);
  expect(held).not.toBeNull();
  const g0 = await screen(page, held!);
  expect(Math.hypot(g0[0] - grab[0], g0[1] - grab[1])).toBeLessThan(0.01);
  const to: [number, number] = [grab[0] - 150, grab[1] + 80];
  const pan = await gesture(page, 2, [grab, ...steps(grab, to, 10)], { probe: held! });
  const off = Math.hypot(pan.last![0] - to[0], pan.last![1] - to[1]);
  // The corner went the same way (a parallel view slides as one).
  const cc = await screen(page, corner);
  console.info(`[orbit] right-drag pan 150 x 80 px: the grabbed point is ${off.toFixed(6)} px from the cursor; the corner moved (${(cc[0] - c[0]).toFixed(3)}, ${(cc[1] - c[1]).toFixed(3)}) px`);
  expect(off).toBeLessThan(1);
  expect(Math.hypot(cc[0] - c[0] + 150, cc[1] - c[1] - 80)).toBeLessThan(1);
  await expect(vp).toHaveAttribute("data-view", "iso"); // a pan keeps the direction
  await expect(vp).toHaveAttribute("data-nav", "");

  // Standard views and zoom extents land where they always did.
  await page.keyboard.press("Shift+Z");
  await expect.poll(async () => (await cam(page)).zoom).toBe(1);
  const refit = await cam(page);
  const centre: V3 = [6 * IN, 6 * IN, 3 * IN];
  for (let k = 0; k < 3; k++) expect(Math.abs(refit.target[k] - centre[k])).toBeLessThan(1e-3);
  const sc = await screen(page, refit.target);
  expect(Math.abs(sc[0] - cv.width / 2)).toBeLessThan(2);
  expect(Math.abs(sc[1] - cv.height / 2)).toBeLessThan(2);
  for (const [name, id] of [["Front", "front"], ["Right", "right"], ["Top", "top"], ["Persp", "persp"], ["Iso", "iso"]] as const) {
    await page.getByRole("button", { name, exact: true }).click();
    await expect(vp).toHaveAttribute("data-view", id);
  }
});

test("orbiting from the Top view and in perspective keeps the pivot pinned too", async ({ page }) => {
  const vp = await ready(page);
  await box(page);
  await page.keyboard.press("Escape");
  await stubCapture(page);
  const corner: V3 = [12 * IN, 12 * IN, 6 * IN];
  for (const view of ["Top", "Persp"] as const) {
    await page.getByRole("button", { name: view, exact: true }).click();
    await expect(vp).toHaveAttribute("data-view", view.toLowerCase());
    const c = await screen(page, corner);
    const at: [number, number] = [c[0] - 1, c[1] + 1];
    const g = await gesture(page, 1, [at, ...steps(at, [at[0] - 120, at[1] - 90], 12)], { probe: corner });
    expect(g.drift, view).toBeLessThan(1);
    await expect(vp).toHaveAttribute("data-view", "custom");
  }
  // Orbiting off the model pivots on the ground under the cursor.
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  const cv = (await page.locator("#cv").boundingBox())!;
  const spot: [number, number] = [cv.width * 0.2, cv.height * 0.75];
  const ground = await page.evaluate(([x, y]) => (window as unknown as { __sawhorse: Hooks }).__sawhorse.navPick(x, y), spot);
  expect(ground).not.toBeNull();
  expect(Math.abs(ground![2])).toBeLessThan(1e-9);
  const g = await gesture(page, 1, [spot, ...steps(spot, [spot[0] + 90, spot[1] - 60], 9)], { probe: ground! });
  expect(g.drift).toBeLessThan(1);
});
