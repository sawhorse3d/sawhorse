import { expect, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

// Lane 20b: the Cut list table reads in full at any panel width. Shared by the CC-BY
// cupboard spec and the private gondola spec: the panel set by its drag handle, nothing in
// the table wider than its box, and named tokens on one line each.

/** The side panel's width (px, rounded). */
export async function panelWidth(page: Page): Promise<number> {
  return Math.round((await page.locator("aside.side").first().boundingBox())!.width);
}

/** Drag the panel's left edge until the panel is `width` px wide (right narrows it). */
export async function dragPanelTo(page: Page, width: number) {
  const grip = page.getByTestId("side-grip");
  const g = (await grip.boundingBox())!;
  const from = await panelWidth(page);
  const x = g.x + g.width / 2;
  const y = g.y + 220;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + (from - width), y, { steps: 6 });
  await page.mouse.up();
  await expect.poll(() => panelWidth(page)).toBe(width);
}

/** Every element of the Cut list table whose content is wider than its box, ends in an
 * ellipsis, or lies outside the panel (empty when the table reads in full). */
export function tableClipped(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const table = document.querySelector<HTMLElement>(".cutlist table");
    if (!table) return ["no Cut list table"];
    const pane = document.querySelector("aside.side")!.getBoundingClientRect();
    const out: string[] = [];
    const label = (el: Element) => `${el.tagName.toLowerCase()}${el.className && typeof el.className === "string" ? `.${el.className.trim().replace(/\s+/g, ".")}` : ""} "${(el.textContent ?? "").slice(0, 60)}"`;
    for (const el of [table, ...Array.from(table.querySelectorAll("*"))]) {
      if (el.scrollWidth > el.clientWidth) out.push(`${label(el)}: scrollWidth ${el.scrollWidth} > clientWidth ${el.clientWidth}`);
      const cs = getComputedStyle(el);
      if (cs.textOverflow === "ellipsis") out.push(`${label(el)}: text-overflow ellipsis`);
      const r = el.getBoundingClientRect();
      if (r.width > 0 && (r.left < pane.left - 0.5 || r.right > pane.right + 0.5)) out.push(`${label(el)}: outside the panel (${r.left.toFixed(1)}..${r.right.toFixed(1)} vs ${pane.left.toFixed(1)}..${pane.right.toFixed(1)})`);
    }
    return out;
  });
}

/** For each place `token` is written in the Cut list table, the number of lines it spans
 * (1: a whole token). */
export function tokenLines(page: Page, token: string): Promise<number[]> {
  return page.evaluate((token) => {
    const table = document.querySelector(".cutlist table");
    if (!table) return [];
    const holders = Array.from(table.querySelectorAll("*")).filter((el) => (el.textContent ?? "").includes(token) && !Array.from(el.children).some((c) => (c.textContent ?? "").includes(token)));
    const out: number[] = [];
    for (const el of holders) {
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      const nodes: Text[] = [];
      for (let n = walker.nextNode(); n; n = walker.nextNode()) nodes.push(n as Text);
      const text = nodes.map((x) => x.data).join("");
      for (let at = text.indexOf(token); at >= 0; at = text.indexOf(token, at + 1)) {
        const range = document.createRange();
        let pos = 0;
        for (const t of nodes) {
          const end = pos + t.data.length;
          if (at >= pos && at < end) range.setStart(t, at - pos);
          if (at + token.length > pos && at + token.length <= end) range.setEnd(t, at + token.length - pos);
          pos = end;
        }
        const rects = Array.from(range.getClientRects())
          .filter((r) => r.width > 0 && r.height > 0)
          .sort((a, b) => a.top - b.top);
        let lines = 0;
        let bottom = -Infinity;
        for (const r of rects) {
          if (r.top >= bottom - 1) {
            lines++;
            bottom = r.bottom;
          } else bottom = Math.max(bottom, r.bottom);
        }
        out.push(lines);
      }
    }
    return out;
  }, token);
}

/** The Cut list's layout: "one-line" (a row a part) or "two-line" (name, then sizes). */
export function layoutOf(page: Page): Promise<string | null> {
  return page.locator(".cutlist table").getAttribute("data-layout");
}

/** The Part column's width (px) as laid out. */
export async function partColumn(page: Page): Promise<number> {
  return (await page.locator(".cutlist thead th.part").boundingBox())!.width;
}

/**
 * Check the table at the panel's current width: nothing clipped, each token on one line
 * (and written at least once), the layout as expected; then a screenshot of the window.
 * SAWHORSE_CAPTURES (a directory) also receives it as <SAWHORSE_CAPTURE_PREFIX><name>.png.
 */
export async function checkCutListFits(page: Page, opts: { tokens: string[]; layout: "one-line" | "two-line"; shot: string; outputPath: (name: string) => string; capturesDir?: string; capturePrefix?: string }) {
  await page.evaluate(() => document.fonts.ready.then(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))));
  const width = await panelWidth(page);
  await expect.poll(() => layoutOf(page), `layout at ${width} px`).toBe(opts.layout);
  expect(await tableClipped(page), `nothing in the table is cut off at ${width} px`).toEqual([]);
  if (opts.layout === "one-line") expect(await partColumn(page), `the Part column at ${width} px`).toBeGreaterThanOrEqual(120);
  for (const token of opts.tokens) {
    const lines = await tokenLines(page, token);
    expect(lines.length, `"${token}" is in the table at ${width} px`).toBeGreaterThan(0);
    expect(lines, `"${token}" is whole (on one line) at ${width} px`).toEqual(lines.map(() => 1));
  }
  const name = `${opts.shot}.png`;
  await page.screenshot({ path: opts.outputPath(name), animations: "disabled" });
  if (opts.capturesDir) {
    mkdirSync(opts.capturesDir, { recursive: true });
    await page.screenshot({ path: path.join(opts.capturesDir, `${opts.capturePrefix ?? ""}${name}`), animations: "disabled" });
  }
}
