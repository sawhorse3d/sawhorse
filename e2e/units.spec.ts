import { expect, test } from "@playwright/test";
import { ready, setUnits } from "./helpers";

// Lane 24, issue #13 (a YouTuber's first look at the web app: "it only gives me inches ...
// Nevermind, I found it"): a cold open names the units in the status line, the unit chip
// opens a picker, and one pick makes a typed 300 come out as 300 mm. Edit > Units is the
// same choice from the menu bar.

test("a cold open says the units, one click changes them, and 300 is then 300 mm", async ({ page }) => {
  const vp = await ready(page);
  // Where the eye lands: the status line, on an empty document.
  const line = page.getByTestId("hint-units");
  await expect(line).toHaveText("Units: fractional inches. Click to change.");
  await expect(page.locator(".vcb button.units")).toHaveText("in");
  await expect(page.locator(".vp .hud")).toContainText("Units: fractional in");
  // The chip is a click target for the picker, and says the same sentence to a screen reader.
  const chip = page.locator(".vcb button.units");
  await expect(chip).toHaveAccessibleName("Units: fractional inches. Click to change.");
  await chip.click();
  const picker = page.getByRole("menu", { name: "Units" });
  await expect(picker).toBeVisible();
  await expect(picker.getByRole("menuitemradio")).toHaveText([/Millimetres/, /Centimetres/, /Metres/, /Decimal inches/, /Feet and inches/, /Fractional inches/]);
  await expect(picker.getByRole("menuitemradio", { name: /Fractional inches/ })).toHaveAttribute("aria-checked", "true");
  await picker.getByRole("menuitemradio", { name: /Millimetres/ }).click();
  await expect(picker).toHaveCount(0);
  await expect(page.locator(".vcb button.units")).toHaveText("mm");
  await expect(page.locator(".vp .hud")).toContainText("Units: mm");
  await expect(line).toHaveText("Units: millimetres. Click to change.");
  // A typed 300 is 300 mm now: a 300 mm square pulled 300 mm is 0.027 m3.
  await page.keyboard.press("r");
  await page.keyboard.type("300,300");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Rectangle 300 × 300 mm");
  await page.keyboard.press("p");
  // Push's default distance is 12 in, which the box now writes in mm.
  await expect(page.locator("#vcb")).toHaveValue("304.8");
  await page.keyboard.type("300");
  await page.keyboard.press("Enter");
  await expect(page.locator("#hint")).toContainText("Extruded 300 mm");
  expect(Number(await vp.getAttribute("data-volume"))).toBeCloseTo(0.3 ** 3, 9);
  // Something is drawn: the status line no longer carries the units sentence (the chip does).
  await expect(page.getByTestId("hint-units")).toHaveCount(0);
  // The choice is kept on this browser.
  await page.reload();
  await ready(page);
  await expect(page.locator(".vcb button.units")).toHaveText("mm");
  // The status line's sentence opens the same picker; Esc closes it and cancels nothing.
  await page.getByTestId("hint-units").click();
  await expect(page.getByRole("menu", { name: "Units" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu", { name: "Units" })).toHaveCount(0);
  await expect(page.locator(".vcb button.units")).toHaveText("mm");
});

test("Edit > Units lists the same six, checks the current one, and Choose… opens the picker", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Units" }).click();
  const sub = page.getByRole("menu", { name: "Units" });
  await expect(sub.getByRole("menuitemcheckbox")).toHaveText(["   Millimetres", "   Centimetres", "   Metres", "   Decimal inches", "   Feet and inches", "✓ Fractional inches"]);
  await sub.getByRole("menuitemcheckbox", { name: "Centimetres" }).click();
  await expect(page.locator(".vcb button.units")).toHaveText("cm");
  await expect(page.getByTestId("hint-units")).toHaveText("Units: centimetres. Click to change.");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("menuitem", { name: "Units" }).click();
  await expect(page.getByRole("menu", { name: "Units" }).getByRole("menuitemcheckbox", { name: "Centimetres" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("menuitem", { name: "Choose…" }).click();
  const picker = page.getByTestId("unit-picker");
  await expect(picker).toBeVisible();
  await expect(picker.getByRole("menuitemradio", { name: /Centimetres/ })).toHaveAttribute("aria-checked", "true");
  await picker.getByRole("menuitemradio", { name: /Feet and inches/ }).click();
  await expect(page.locator(".vcb button.units")).toHaveText("ft-in");
  // The shared helper every other spec uses goes through the picker too.
  await setUnits(page, "mm");
  await expect(page.locator(".vcb button.units")).toHaveText("mm");
  await setUnits(page, "frac-in");
  await expect(page.locator(".vcb button.units")).toHaveText("in");
});

test("no first-run hint outside the browser demo", async ({ page }) => {
  // The e2e build is the plain app (VITE_WEB_DEMO unset): no demo banner, no units hint.
  await ready(page);
  await expect(page.getByRole("note", { name: "About this demo" })).toHaveCount(0);
  await expect(page.getByTestId("units-hint")).toHaveCount(0);
});
