import { expect, test, type Page } from "@playwright/test";
import { bridge, desktopBridge, setBridge } from "./tauri";

// Lane 67 (#97): no title in the menu bar does nothing on a click. Help had no entries in the
// desktop app, and Draw and Window had none in any build, so a click on them opened nothing.
// Now a title is in the bar only while its menu has an entry, and the desktop app's Help menu
// opens the user guide, the changelog and the source in the system's browser through the
// shell's open_external command. The desktop shell cannot run here: the page runs as it does
// inside it, with the bridge stood in for (e2e/tauri.ts), which records every address the page
// asked it to open. The shell's own list is tested in src-tauri/device-key
// (only_the_help_pages_open_beside_checkout).
const requireKernel = !!process.env.REQUIRE_KERNEL;

async function ready(page: Page, url = "/?autosave=0") {
  await page.goto(url);
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** window.open, recorded and never opening anything, before the page's scripts run. */
async function recordOpen(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { __opened: unknown[][] };
    w.__opened = [];
    window.open = (...args: unknown[]) => {
      w.__opened.push(args.map(String));
      return null;
    };
  });
}

const openedTabs = (page: Page) => page.evaluate(() => (window as unknown as { __opened: string[][] }).__opened);

/** The titles in the bar, and the entries of each title's menu (separators left out). */
async function everyMenu(page: Page) {
  const bar = page.getByRole("navigation", { name: "Menu" });
  const titles = await bar.getByRole("button").allTextContents();
  const menus: Record<string, string[]> = {};
  for (const t of titles) {
    const title = bar.getByRole("button", { name: t, exact: true });
    await title.click();
    await expect(title).toHaveAttribute("aria-expanded", "true");
    const menu = page.getByRole("menu", { name: t, exact: true });
    await expect(menu).toBeVisible();
    menus[t] = (await menu.locator(":scope > button, :scope > .subwrap > button").allTextContents()).map((s) => s.trim());
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(title).toHaveAttribute("aria-expanded", "false");
  }
  return { titles, menus };
}

async function helpEntry(page: Page, name: string) {
  await page.getByRole("navigation", { name: "Menu" }).getByRole("button", { name: "Help", exact: true }).click();
  await page.getByRole("menu", { name: "Help" }).getByRole("menuitem", { name, exact: true }).click();
  await expect(page.getByRole("menu", { name: "Help" })).toHaveCount(0);
}

test("the browser build: every title in the bar opens a menu with an entry, and Help holds its four", async ({ page }) => {
  await ready(page);
  const { titles, menus } = await everyMenu(page);
  expect(titles).toEqual(["File", "Edit", "View", "Tools", "Plugins", "Help"]);
  for (const t of titles) expect(menus[t].length, t).toBeGreaterThan(0);
  expect(menus.Help).toEqual(["User Guide", "What's New", "Download the Desktop App", "Source on GitHub"]);
});

test("the browser build: each Help entry opens its page in a new tab and the app stays where it is", async ({ page }) => {
  await recordOpen(page);
  await ready(page);
  const at = page.url();
  for (const name of ["User Guide", "What's New", "Download the Desktop App", "Source on GitHub"]) await helpEntry(page, name);
  // This build's base is relative ("./"), as `npm run dev` and `npm run preview` are, so the
  // site's pages are the public site's; the demo's base (/app/) is in src/lib/help.test.ts and
  // scripts/site-smoke.mjs.
  await expect.poll(() => openedTabs(page)).toEqual([
    ["https://sawhorse3d.com/guide/", "_blank", "noopener,noreferrer"],
    ["https://sawhorse3d.com/changelog/", "_blank", "noopener,noreferrer"],
    ["https://github.com/sawhorse3d/sawhorse/releases/latest", "_blank", "noopener,noreferrer"],
    ["https://github.com/sawhorse3d/sawhorse", "_blank", "noopener,noreferrer"],
  ]);
  expect(page.url()).toBe(at);
});

for (const platform of ["Win32", "MacIntel"]) {
  test(`the desktop app (${platform}): every title opens a menu with an entry, and Help holds the three desktop entries`, async ({ page }) => {
    await desktopBridge(page, platform);
    await ready(page);
    const { titles, menus } = await everyMenu(page);
    expect(titles).toEqual(["File", "Edit", "View", "Tools", "Plugins", "Help"]);
    for (const t of titles) expect(menus[t].length, t).toBeGreaterThan(0);
    expect(menus.Help).toEqual(["User Guide", "What's New", "Source on GitHub"]);
  });
}

test("the desktop app: a Help entry asks the shell to open the public page, and nothing else opens or moves", async ({ page }) => {
  await recordOpen(page);
  await desktopBridge(page);
  await ready(page);
  const at = page.url();
  expect((await bridge(page)).opened).toEqual([]);
  for (const name of ["User Guide", "What's New", "Source on GitHub"]) await helpEntry(page, name);
  await expect.poll(async () => (await bridge(page)).opened).toEqual([
    "https://sawhorse3d.com/guide/",
    "https://sawhorse3d.com/changelog/",
    "https://github.com/sawhorse3d/sawhorse",
  ]);
  expect(await openedTabs(page)).toEqual([]);
  expect(page.url()).toBe(at);
  await expect(page.locator("#hint")).not.toContainText("Could not open");
});

test("the desktop app: a shell that does not open the page says so with the address", async ({ page }) => {
  await desktopBridge(page);
  await ready(page);
  await setBridge(page, { openFails: true });
  await helpEntry(page, "User Guide");
  await expect(page.locator("#hint")).toContainText("Could not open https://sawhorse3d.com/guide/ in the browser: refusing to open https://sawhorse3d.com/guide/");
  expect((await bridge(page)).opened).toEqual(["https://sawhorse3d.com/guide/"]);
});
