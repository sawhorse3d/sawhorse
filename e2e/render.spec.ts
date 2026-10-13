import { expect, test, type Page } from "@playwright/test";
import { fakeRenderService, SERVICE } from "./fakeRenderService";

// Lane 8: the Credits chip and the Render pane against the render service's API, with the
// service faked inside Playwright (see fakeRenderService.ts; it verifies every signature).
// Lane 15: a release build shows neither until a render service is set in Settings and
// answers its health check; each test here sets the address first (as Settings saves it).
const requireKernel = !!process.env.REQUIRE_KERNEL;

test.beforeEach(async ({ page }) => {
  await page.addInitScript((url) => {
    try {
      if (!sessionStorage.getItem("render-url-set")) {
        localStorage.setItem("sawhorse.renderServiceUrl", url);
        sessionStorage.setItem("render-url-set", "1");
      }
    } catch {
      /* storage blocked */
    }
  }, SERVICE);
});

async function kernelReady(page: Page) {
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  const state = await vp.getAttribute("data-kernel");
  if (!requireKernel && state === "missing") test.skip(true, "kernel WASM not built (run npm run wasm)");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

test("no render service, or one not answering: no chip, no Render pane, Settings says why (lane 15)", async ({ page }) => {
  // Set but not answering.
  await page.route(`${SERVICE}/**`, (route) => route.abort("connectionrefused"));
  await page.goto("/");
  await expect(page.locator("main.vp")).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  await page.getByRole("button", { name: "Edit" }).click();
  await page.getByRole("menuitem", { name: /Settings/ }).click();
  const line = page.getByTestId("render-gate");
  await expect(line).toHaveAttribute("data-status", "unreachable");
  await expect(line).toHaveText(`Renders are off: the render service at ${SERVICE} did not answer its health check.`);
  await expect(page.locator("header.top .chip")).toHaveCount(0);
  await expect(page.getByRole("tab", { name: "Render" })).toHaveCount(0);
  // Cleared: off, nothing asked.
  await page.getByRole("button", { name: "Clear" }).click();
  await expect(line).toHaveAttribute("data-status", "off");
  await expect(line).toHaveText("Renders are off: no render service is set. Sawhorse needs none to model, list or nest.");
  await expect(page.getByRole("tab", { name: "Render" })).toHaveCount(0);
});

test("a service set: the app asks its health once at startup (lane 12c)", async ({ page }) => {
  let health = 0;
  await page.route(`${SERVICE}/**`, (route) => {
    if (new URL(route.request().url()).pathname === "/healthz") health++;
    return route.abort("connectionrefused");
  });
  await page.goto("/");
  await expect(page.locator("main.vp")).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  await page.waitForTimeout(500);
  expect(health).toBe(1);
});

test("set and answering, but the balance call fails: the chip shows a dash and Render explains, nothing is charged", async ({ page }) => {
  await page.route(`${SERVICE}/**`, (route) => (new URL(route.request().url()).pathname === "/healthz" ? route.fulfill({ status: 200, body: "{}", headers: { "Access-Control-Allow-Origin": "*" } }) : route.abort("connectionrefused")));
  await page.goto("/");
  const chip = page.locator("header.top .chip");
  await expect(chip).toHaveAttribute("data-state", "unreachable");
  await expect(page.locator("#credits")).toHaveText("—");
  await chip.click();
  await expect(page.locator("#pane-render")).toBeVisible();
  await expect(page.locator(".note.svc")).toHaveText("Render service not connected.");
  await page.locator("#renderBtn").click();
  await expect(page.locator("#result")).toContainText("Render service not connected (http://127.0.0.1:8787)");
});

test("renders the current view: signed requests, 16-bit depth + edge + clay passes, one credit", async ({ page }) => {
  const svc = await fakeRenderService(page, 3);
  await page.goto("/");
  const vp = await kernelReady(page);
  await expect(page.locator("#credits")).toHaveText("3");
  await expect(page.locator("header.top .chip")).toHaveAttribute("data-state", "connected");

  // Nothing drawn yet: no charge, a plain explanation.
  await page.getByRole("tab", { name: "Render" }).click();
  await page.locator("#renderBtn").click();
  await expect(page.locator("#result")).toContainText("Draw something first");
  expect(svc.renders).toHaveLength(0);

  // A 24 x 16 x 12 in box, then render it with the Kitchen preset.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press("r");
  await page.keyboard.type("24,16");
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await page.keyboard.type("12");
  await page.keyboard.press("Enter");
  await expect(vp).toHaveAttribute("data-watertight", "true");
  await page.getByRole("button", { name: /Kitchen/ }).click();
  await page.locator("#renderBtn").click();
  await expect(page.locator("#result")).toHaveAttribute("data-state", "done");
  await expect(page.locator("#renderImg")).toHaveJSProperty("complete", true);
  expect(await page.locator("#renderImg").evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(64);
  await expect(page.locator("#credits")).toHaveText("2");
  await expect(page.locator("#result .cap")).toHaveText("Mock render · no provider keys");

  const r = svc.renders[0];
  expect(r.prompt).toMatch(/warm kitchen/);
  expect(r.preset).toBe("Kitchen");
  // Depth: 16-bit grey at the view's aspect; background 0, model 1/8 grey up to white.
  expect([r.depth.bitDepth, r.depth.colorType]).toEqual([16, 0]);
  expect(Math.max(r.depth.width, r.depth.height)).toBe(1024);
  expect(r.depth.width % 16).toBe(0);
  const model = r.depth.samples.filter((v) => v > 0);
  expect(model.length).toBeGreaterThan(1000);
  expect(model.reduce((a, b) => Math.max(a, b), 0)).toBe(65535);
  expect(model.reduce((a, b) => Math.min(a, b), 65535)).toBeGreaterThanOrEqual(8192);
  expect(r.depth.samples[0]).toBe(0);
  // Edges: 8-bit, white paper with dark lines. Clay: 8-bit, shaded.
  expect([r.edges.bitDepth, r.edges.width, r.edges.height]).toEqual([8, r.depth.width, r.depth.height]);
  expect(r.edges.samples.filter((v) => v < 128).length).toBeGreaterThan(200);
  expect(r.edges.samples[0]).toBe(255);
  expect(new Set(r.clay.samples).size).toBeGreaterThan(3);
  expect(svc.rejected).toEqual([]);
  expect(svc.installIds.size).toBe(1);
});

test("a credit pack opens checkout in a new window and the chip follows the payment", async ({ page, context }) => {
  const svc = await fakeRenderService(page, 0);
  await context.route(`${SERVICE}/dev/checkout/**`, (route) => route.fulfill({ status: 200, contentType: "text/html", body: "<title>Simulated checkout</title><h1>Simulated checkout</h1>" }));
  await page.goto("/");
  await expect(page.locator("#credits")).toHaveText("0");
  await page.getByRole("tab", { name: "Render" }).click();
  await page.locator("#renderBtn").click();
  await expect(page.locator("#result")).toHaveText("No credits on this device. Buy a pack below.");

  const popup = page.waitForEvent("popup");
  await page.getByRole("button", { name: /\$10/ }).click();
  const checkout = await popup;
  await expect(checkout).toHaveURL(`${SERVICE}/dev/checkout/cs_test_e2e`);
  expect(svc.checkouts).toEqual(["p100"]);
  await expect(page.locator("#result")).toContainText("simulated checkout");
  await expect(page.locator(".note.svc")).toContainText("waiting for checkout");

  // The webhook lands on the service; the app's poll picks it up within a few seconds.
  svc.credits = 100;
  await expect(page.locator("#credits")).toHaveText("100", { timeout: 10_000 });
  await expect(page.locator(".note.svc")).not.toContainText("waiting for checkout");
});
