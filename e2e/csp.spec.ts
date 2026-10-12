import { expect, test } from "@playwright/test";
import { box, ready } from "./helpers";

// The browser build carries a content security policy (vite.config.ts, cspMeta): the plugin
// sandbox's second lock (ADR-0009). Every other spec runs under it too; this one walks the
// app's panes and the bundled plugins and checks the policy never refused anything the app
// itself does.

type Hook = { plugins: { enable(id: string): Promise<void>; list(): { id: string; status: string }[] } };

test("the app works under its content security policy without a single violation", async ({ page }) => {
  const consoleCsp: string[] = [];
  page.on("console", (m) => {
    if (/Content Security Policy|Content-Security-Policy/i.test(m.text())) consoleCsp.push(m.text());
  });
  await page.addInitScript(() => {
    const w = window as unknown as { __cspViolations: string[] };
    w.__cspViolations = [];
    document.addEventListener("securitypolicyviolation", (e) => w.__cspViolations.push(`${e.violatedDirective} ${e.blockedURI}`));
  });
  await ready(page);
  const csp = (await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content")) ?? "";
  expect(csp).toContain("default-src 'self'");
  expect(csp).toContain("object-src 'none'");

  await box(page);
  for (const tab of ["Outliner", "Tags", "Cut list", "Materials", "Styles", "Render", "Plugins"]) {
    await page.getByRole("tab", { name: tab, exact: true }).click();
    await expect(page.getByRole("tab", { name: tab, exact: true })).toHaveAttribute("aria-selected", "true");
  }
  // The bundled samples start in their sandboxes (blob: workers and blob: modules).
  for (const id of ["com.sawhorse3d.cabinet-generator", "com.sawhorse3d.model-report", "com.sawhorse3d.shelf-pin-holes"]) {
    await page.evaluate((i) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.enable(i), id);
    await expect.poll(async () => page.evaluate((i) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((e) => e.id === i)?.status, id), { timeout: 10_000 }).toBe("running");
  }
  // Library textures are drawn by the kernel and shown as blob: images.
  await page.getByRole("tab", { name: "Materials", exact: true }).click();
  await expect(page.locator('.swatch[style*="blob:"]').first()).toBeVisible({ timeout: 15_000 });

  expect(await page.evaluate(() => (window as unknown as { __cspViolations: string[] }).__cspViolations)).toEqual([]);
  expect(consoleCsp).toEqual([]);
});
