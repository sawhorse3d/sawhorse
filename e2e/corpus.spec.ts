import { expect, test, type Page } from "@playwright/test";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";

// The .skp corpus through the real browser build: every file in tests/corpus/skp/ (made by
// `npm run corpus:skp`) is imported through File > Import's input, framed in the Iso view
// and rendered to PNG (test-results/corpus/). The first 20 (the golden sample) are compared
// with tests/corpus/golden/*.png by a perceptual diff that ignores anti-aliasing and GPU
// differences: both images are box-filtered down to 96 x 72 luminance cells and compared
// cell by cell. UPDATE_GOLDEN=1 rewrites the references. REQUIRE_CORPUS=1 (CI's corpus job)
// fails when the corpus has not been generated; otherwise the spec skips.
const corpusDir = path.join("tests", "corpus", "skp");
const goldenDir = path.join("tests", "corpus", "golden");
const outDir = path.join("test-results", "corpus");
const files = existsSync(corpusDir)
  ? readdirSync(corpusDir)
      .filter((f) => f.endsWith(".skp"))
      .sort()
  : [];
const SAMPLE = 20;
const CHUNK = 20;
const update = process.env.UPDATE_GOLDEN === "1";

/** Largest mean cell difference (0..1) and share of cells off by more than 0.25 allowed. */
export const MAX_MEAN = 0.03;
export const MAX_BAD = 0.01;

if (!files.length && process.env.REQUIRE_CORPUS === "1") throw new Error("REQUIRE_CORPUS=1 but tests/corpus/skp is empty: run npm run corpus:skp");

interface Row {
  file: string;
  family: string;
  ms: number;
  objects: number;
  meshes: number;
  groups: number;
  notes: number;
  watertight: string | null;
  diff?: { mean: number; bad: number; ink: number };
}
const rows: Row[] = [];

async function ready(page: Page) {
  page.on("dialog", (d) => void d.accept());
  await page.goto("/?io=basic&theme=light");
  const vp = page.locator("main.vp");
  await expect(vp).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  // Only the model: no HUD, tips, axes label or report over the canvas.
  await page.addStyleTag({ content: ".vp .hud,.vp .tip,.vp .axes,.vp .legend,.import-report,.drop-hint{visibility:hidden!important}" });
  return vp;
}

/** Perceptual difference of two PNGs, computed in the page (canvas box filter). */
async function perceptual(page: Page, a: Buffer, b: Buffer) {
  return page.evaluate(
    async ([a64, b64]) => {
      const W = 96;
      const H = 72;
      const load = async (s: string) => {
        const img = new Image();
        img.src = `data:image/png;base64,${s}`;
        await img.decode();
        return img;
      };
      // Halve repeatedly (each step a 2x2 average), then the last step to W x H.
      const cells = (img: HTMLImageElement) => {
        let src: CanvasImageSource = img;
        let w = img.naturalWidth;
        let h = img.naturalHeight;
        while (w / 2 >= W * 2 && h / 2 >= H * 2) {
          const c = document.createElement("canvas");
          c.width = Math.round(w / 2);
          c.height = Math.round(h / 2);
          const x = c.getContext("2d")!;
          x.imageSmoothingQuality = "high";
          x.drawImage(src, 0, 0, c.width, c.height);
          src = c;
          w = c.width;
          h = c.height;
        }
        const c = document.createElement("canvas");
        c.width = W;
        c.height = H;
        const x = c.getContext("2d")!;
        x.imageSmoothingQuality = "high";
        x.drawImage(src, 0, 0, W, H);
        const d = x.getImageData(0, 0, W, H).data;
        const out = new Float32Array(W * H);
        for (let i = 0; i < W * H; i++) out[i] = (0.2126 * d[i * 4] + 0.7152 * d[i * 4 + 1] + 0.0722 * d[i * 4 + 2]) / 255;
        return out;
      };
      const [ia, ib] = await Promise.all([load(a64), load(b64)]);
      const [la, lb] = [cells(ia), cells(ib)];
      let sum = 0;
      let bad = 0;
      let ink = 0;
      const bg = la[0];
      for (let i = 0; i < la.length; i++) {
        const dd = Math.abs(la[i] - lb[i]);
        sum += dd;
        if (dd > 0.25) bad++;
        if (Math.abs(la[i] - bg) > 0.08) ink++;
      }
      return { mean: sum / la.length, bad: bad / la.length, ink: ink / la.length };
    },
    [a.toString("base64"), b.toString("base64")] as const,
  );
}

async function renderOne(page: Page, file: string): Promise<{ png: Buffer; row: Row }> {
  const t0 = Date.now();
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "New", exact: true }).click();
  await expect(page.locator("main.vp")).toHaveAttribute("data-objects", "0");
  await page.getByTestId("file-input").setInputFiles(path.join(corpusDir, file));
  const report = page.getByTestId("import-report");
  await expect(report).toContainText(`${file}`, { timeout: 20_000 });
  const title = ((await report.locator("header b").textContent()) ?? "").trim();
  expect(title, `${file} was refused: ${await report.textContent()}`).toMatch(/^Imported /);
  const ms = Date.now() - t0;
  const imp = await page.evaluate(() => (window as unknown as { __sawhorse: { lastImport: { outcome: { objects: number[]; meshes: number[]; groups: number[]; notes: string[] }; notes: string[] } } }).__sawhorse.lastImport);
  // Frame it: the Iso direction, zoomed to the model.
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const png = await page.locator("#cv").screenshot({ animations: "disabled" });
  const vp = page.locator("main.vp");
  return {
    png,
    row: {
      file,
      family: file.replace(/^\d+-|\.skp$/g, ""),
      ms,
      objects: imp.outcome.objects.length,
      meshes: imp.outcome.meshes.length,
      groups: imp.outcome.groups.length,
      notes: imp.notes.length,
      watertight: await vp.getAttribute("data-watertight"),
    },
  };
}

test.describe("skp corpus", () => {
  test.skip(!files.length, "no corpus: run npm run corpus:skp");
  // A smaller window keeps the committed reference images small (about 60 KB each).
  test.use({ viewport: { width: 1000, height: 720 } });
  mkdirSync(outDir, { recursive: true });

  for (let start = 0; start < Math.max(files.length, 1); start += CHUNK) {
    const chunk = files.slice(start, start + CHUNK);
    test(`files ${start + 1}-${start + chunk.length}`, async ({ page }) => {
      test.setTimeout(240_000);
      await ready(page);
      for (const file of chunk) {
        const { png, row } = await renderOne(page, file);
        writeFileSync(path.join(outDir, file.replace(/\.skp$/, ".png")), png);
        expect(row.watertight, `${file}: every solid is watertight`).toBe("true");
        if (row.family !== "open") expect(row.objects, `${file}: became at least one solid`).toBeGreaterThan(0);
        const index = files.indexOf(file);
        if (index < SAMPLE) {
          const golden = path.join(goldenDir, file.replace(/\.skp$/, ".png"));
          if (update || !existsSync(golden)) {
            if (!update) throw new Error(`missing golden image ${golden}: run UPDATE_GOLDEN=1 npx playwright test e2e/corpus.spec.ts`);
            mkdirSync(goldenDir, { recursive: true });
            writeFileSync(golden, png);
          } else {
            const d = await perceptual(page, png, readFileSync(golden));
            row.diff = d;
            expect(d.ink, `${file}: the render shows the model`).toBeGreaterThan(0.002);
            expect(d.mean, `${file}: mean cell difference ${d.mean.toFixed(4)} (see ${outDir})`).toBeLessThanOrEqual(MAX_MEAN);
            expect(d.bad, `${file}: share of very different cells ${d.bad.toFixed(4)}`).toBeLessThanOrEqual(MAX_BAD);
          }
        }
        rows.push(row);
      }
      // The metric is not vacuous: two different sample models must fail it.
      if (start === 0 && !update && chunk.length >= 2) {
        const [a, b] = chunk.slice(0, 2).map((f) => readFileSync(path.join(goldenDir, f.replace(/\.skp$/, ".png"))));
        const cross = await perceptual(page, a, b);
        expect(cross.mean > MAX_MEAN || cross.bad > MAX_BAD, `different models must differ (${cross.mean.toFixed(4)}, ${cross.bad.toFixed(4)})`).toBe(true);
      }
    });
  }

  test.afterAll(() => {
    if (!rows.length) return;
    rows.sort((a, b) => a.file.localeCompare(b.file));
    const byFamily: Record<string, { files: number; solids: number; meshes: number; ms: number }> = {};
    for (const r of rows) {
      const f = (byFamily[r.family] ??= { files: 0, solids: 0, meshes: 0, ms: 0 });
      f.files++;
      f.solids += r.objects;
      f.meshes += r.meshes;
      f.ms += r.ms;
    }
    const summary = {
      files: rows.length,
      solids: rows.reduce((n, r) => n + r.objects, 0),
      meshes: rows.reduce((n, r) => n + r.meshes, 0),
      sample_compared: rows.filter((r) => r.diff).length,
      worst_sample_mean: Math.max(0, ...rows.filter((r) => r.diff).map((r) => r.diff!.mean)),
      families: Object.fromEntries(Object.entries(byFamily).map(([k, v]) => [k, { ...v, mean_ms: Math.round(v.ms / v.files) }])),
    };
    writeFileSync(path.join(outDir, "report.json"), JSON.stringify({ summary, rows }, null, 2));
    console.log(`[corpus] ${summary.files} files: ${summary.solids} solids, ${summary.meshes} non-solid meshes; ${summary.sample_compared} compared with goldens, worst mean ${summary.worst_sample_mean.toFixed(4)}`);
  });
});
