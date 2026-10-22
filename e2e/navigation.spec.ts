import { setUnits } from "./helpers";
import { expect, test, type Page } from "@playwright/test";

// Camera and navigation (lane 3; the orbit pivot is lane 21's, see orbit.spec.ts):
// middle-drag orbit, Shift+middle pan, wheel zoom toward the cursor, Shift+Z zoom extents,
// standard views, the projection toggle, a unit-aware grid, and sketch regions drawn over
// the grid.
const requireKernel = !!process.env.REQUIRE_KERNEL;

type V3 = [number, number, number];
interface Cam {
  eye: V3;
  target: V3;
  projection: string;
  view: string | null;
  zoom: number;
}

async function ready(page: Page) {
  await page.goto("/");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

const cam = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: { camera(): Cam } }).__sawhorse.camera());

async function canvasBox(page: Page) {
  const b = await page.locator("#cv").boundingBox();
  if (!b) throw new Error("no canvas");
  return b;
}

const dir = (c: Cam): V3 => {
  const d: V3 = [c.eye[0] - c.target[0], c.eye[1] - c.target[1], c.eye[2] - c.target[2]];
  const l = Math.hypot(...d);
  return [d[0] / l, d[1] / l, d[2] / l];
};

test("middle-drag orbits, Shift+middle-drag pans, the wheel zooms toward the cursor", async ({ page }) => {
  const vp = await ready(page);
  await expect(vp).toHaveAttribute("data-view", "iso");
  const b = await canvasBox(page);
  const cx = b.x + b.width / 2;
  const cy = b.y + b.height / 2;

  // Orbit: the eye direction changes and the point under the cursor at the press stays
  // under it (lane 21: the pivot is that point, not the look-at point); the view is no
  // longer "iso".
  const before = await cam(page);
  const pivot = await page.evaluate(([x, y]) => (window as unknown as { __sawhorse: { navPick(x: number, y: number): V3 | null } }).__sawhorse.navPick(x, y), [b.width / 2, b.height / 2] as const);
  expect(pivot).not.toBeNull();
  await page.mouse.move(cx, cy);
  await page.mouse.down({ button: "middle" });
  await page.mouse.move(cx + 120, cy + 30, { steps: 6 });
  await page.mouse.up({ button: "middle" });
  const orbited = await cam(page);
  const still = await page.evaluate((p) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(p), pivot!);
  expect(Math.hypot(still[0] - b.width / 2, still[1] - b.height / 2)).toBeLessThan(1);
  const d0 = dir(before);
  const d1 = dir(orbited);
  expect(d0[0] * d1[0] + d0[1] * d1[1] + d0[2] * d1[2]).toBeLessThan(0.98);
  await expect(vp).toHaveAttribute("data-view", "custom");

  // Pan: the target moves, the direction does not.
  await page.keyboard.down("Shift");
  await page.mouse.move(cx, cy);
  await page.mouse.down({ button: "middle" });
  await page.mouse.move(cx - 80, cy + 50, { steps: 6 });
  await page.mouse.up({ button: "middle" });
  await page.keyboard.up("Shift");
  const panned = await cam(page);
  expect(panned.target).not.toEqual(orbited.target);
  const d2 = dir(panned);
  expect(d1[0] * d2[0] + d1[1] * d2[1] + d1[2] * d2[2]).toBeGreaterThan(0.9999);

  // Zoom toward the cursor: the ground point under the cursor stays under it.
  await page.getByRole("button", { name: "Top" }).click();
  await expect(vp).toHaveAttribute("data-view", "top");
  const px = b.width * 0.3;
  const py = b.height * 0.35;
  const under = await page.evaluate(([x, y]) => (window as unknown as { __sawhorse: { pick(x: number, y: number): { point: V3 } | null } }).__sawhorse.pick(x, y), [px, py] as const);
  expect(under).not.toBeNull();
  const z0 = (await cam(page)).zoom;
  await page.mouse.move(b.x + px, b.y + py);
  await page.mouse.wheel(0, -400);
  await expect.poll(async () => (await cam(page)).zoom).toBeGreaterThan(z0 * 1.2);
  const back = await page.evaluate((p) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(p), under!.point);
  expect(Math.hypot(back[0] - px, back[1] - py)).toBeLessThan(3);
});

test("Shift+Z zooms to the model's extents; views and the projection toggle", async ({ page }) => {
  const vp = await ready(page);
  // A 24 x 16 x 12 in box so there is something to frame.
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  await page.keyboard.press("Escape");
  const fitted = await cam(page);

  // Zoom far out, then Shift+Z frames the box again.
  const b = await canvasBox(page);
  await page.mouse.move(b.x + 40, b.y + 40);
  for (let i = 0; i < 5; i++) await page.mouse.wheel(0, 600);
  await expect.poll(async () => (await cam(page)).zoom).toBeLessThan(fitted.zoom * 0.6);
  await page.keyboard.press("Shift+Z");
  await expect.poll(async () => (await cam(page)).zoom).toBe(1);
  const refit = await cam(page);
  // Framed on the box: its centre (12, 8, 6 in) is the orbit target and the screen centre.
  const boxCentre = [0.3048, 0.2032, 0.1524];
  for (let k = 0; k < 3; k++) expect(Math.abs(refit.target[k] - boxCentre[k])).toBeLessThan(1e-3);
  const centre = await page.evaluate(
    (t) => (window as unknown as { __sawhorse: { worldToScreen(p: V3): [number, number] } }).__sawhorse.worldToScreen(t),
    refit.target,
  );
  expect(Math.abs(centre[0] - b.width / 2)).toBeLessThan(2);
  expect(Math.abs(centre[1] - b.height / 2)).toBeLessThan(2);

  // Standard views and projection.
  for (const [name, id] of [["Front", "front"], ["Right", "right"], ["Top", "top"], ["Persp", "persp"], ["Iso", "iso"]] as const) {
    await page.getByRole("button", { name, exact: true }).click();
    await expect(vp).toHaveAttribute("data-view", id);
    await expect(vp).toHaveAttribute("data-projection", id === "persp" ? "perspective" : "parallel");
    await expect(page.getByRole("button", { name, exact: true })).toHaveAttribute("aria-pressed", "true");
  }
  // View > Perspective toggles the projection and keeps the view direction.
  const iso = await cam(page);
  await page.getByRole("button", { name: "View", exact: true }).click();
  await page.getByRole("menuitemcheckbox", { name: /Perspective/ }).click();
  await expect(vp).toHaveAttribute("data-projection", "perspective");
  await expect(vp).toHaveAttribute("data-view", "iso");
  const persp = await cam(page);
  const a = dir(iso);
  const c = dir(persp);
  expect(a[0] * c[0] + a[1] * c[1] + a[2] * c[2]).toBeGreaterThan(0.9999);
  await page.getByRole("button", { name: "View", exact: true }).click();
  await page.getByRole("menuitemcheckbox", { name: /Perspective/ }).click();
  await expect(vp).toHaveAttribute("data-projection", "parallel");
});

test("the ground grid follows the units, and sketch regions draw over it", async ({ page }) => {
  const vp = await ready(page);
  const grid = () => page.evaluate(() => (window as unknown as { __sawhorse: { grid(): { minor: boolean; major: boolean; spec: { label: string } } } }).__sawhorse.grid());
  expect((await grid()).spec.label).toBe("1 in / 1 ft");
  expect((await grid()).major).toBe(true);
  // Pick mm from the value bar's unit chip (lane 24: a picker, not a cycle).
  await setUnits(page, "mm");
  await expect(page.locator(".vcb .units")).toHaveText("mm");
  expect((await grid()).spec.label).toBe("10 mm / 100 mm");
  await setUnits(page, "frac-in");
  await expect(page.locator(".vcb .units")).toHaveText("in");

  // A rectangle on the ground: its interior shows the region colour, not grid lines.
  await page.getByRole("button", { name: "Top", exact: true }).click();
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-triangles", "2");
  await page.keyboard.press("Escape");
  const topColour = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--face-top").trim());
  const hex = (c: string): [number, number, number] => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)) as [number, number, number];
  const want = hex(topColour);
  const worst = () =>
    page.evaluate((want) => {
      const h = (window as unknown as {
        __sawhorse: { worldToScreen(p: V3): [number, number]; pixel(x: number, y: number): [number, number, number] };
      }).__sawhorse;
      let d = 0;
      // Points exactly on 1 in grid lines inside the 24 x 16 in region (metres).
      for (const [x, y] of [
        [0.0254 * 5, 0.0254 * 3],
        [0.0254 * 12, 0.0254 * 7],
        [0.0254 * 20, 0.0254 * 12],
      ]) {
        const [sx, sy] = h.worldToScreen([x, y, 0]);
        const px = h.pixel(sx, sy);
        for (let k = 0; k < 3; k++) d = Math.max(d, Math.abs(px[k] - want[k]));
      }
      return d;
    }, want);
  await expect.poll(worst).toBeLessThanOrEqual(3);
});
