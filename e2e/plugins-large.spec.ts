import { expect, test } from "@playwright/test";
import { strToU8, zipSync } from "fflate";

// Lane 16: a plugin package near its size limits is checked (its modules parsed, its files
// digested) in a worker, not on the app's thread: loading one keeps the window drawing,
// and the plugin then runs as any other.

const requireKernel = !!process.env.REQUIRE_KERNEL;
const ID = "org.example.large";

const manifest = {
  id: ID,
  name: "Large",
  version: "1.0.0",
  api: 1,
  description: "A plugin with a lot of code.",
  author: "Sawhorse",
  license: "MIT",
  permissions: [],
};

// About 1.2 MB of module, well past the 256 KB above which a package is checked in a worker:
// many small functions (what makes a parser slow is many nodes).
function bigModule(): string {
  const parts: string[] = ['import { ui } from "sawhorse";', 'void ui;'];
  for (let i = 0; i < 20_000; i++) parts.push(`export function f${i}(a, b) { return (a + ${i}) * (b - ${i % 97}) / 3; }`);
  parts.push('console.log("large ready " + f19999(1, 2));');
  return parts.join("\n");
}

type Hook = { plugins: { list(): { id: string; status: string; logs: string[] }[] } };

test("a plugin package near its size limits is checked off the app's thread and then runs", async ({ page }) => {
  test.setTimeout(180_000);
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");

  await page.getByRole("tab", { name: "Plugins" }).click();
  const code = bigModule();
  expect(code.length).toBeGreaterThan(1_000_000);
  const zip = zipSync({ "large/plugin.json": strToU8(JSON.stringify(manifest)), "large/main.js": strToU8(code) });

  // Watch the app's own thread from just before the package arrives.
  await page.evaluate(() => {
    const w = window as unknown as { __gap: number; __run: boolean };
    w.__gap = 0;
    w.__run = true;
    let last = performance.now();
    const tick = (t: number) => {
      w.__gap = Math.max(w.__gap, t - last);
      last = t;
      if (w.__run) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.locator('[data-do="load-session"]').click()]);
  await chooser.setFiles({ name: "large.zip", mimeType: "application/zip", buffer: Buffer.from(zip) });
  const row = page.locator(`.plugin-row[data-plugin="${ID}"]`);
  await expect(row).toContainText("this session", { timeout: 90_000 });
  const gap = await page.evaluate(() => {
    const w = window as unknown as { __gap: number; __run: boolean };
    w.__run = false;
    return w.__gap;
  });
  // Unpacking and reading the zip is on the app's thread and is linear but quick; the
  // parse (about a second at this size, seconds at the package limits) is not.
  expect(gap).toBeLessThan(1500);

  await row.locator('[data-do="enable"]').click();
  await page.getByRole("dialog", { name: "Enable Large" }).getByRole("button", { name: "Enable" }).click();
  await expect
    .poll(
      async () =>
        page.evaluate((id) => {
          const e = (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((x) => x.id === id);
          return !!e?.logs.some((l) => l.includes("large ready"));
        }, ID),
      { timeout: 90_000 },
    )
    .toBe(true);
});
