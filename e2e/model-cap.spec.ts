import { expect, test, type Page } from "@playwright/test";
import { strToU8, zipSync } from "fflate";
import { deflateSync } from "node:zlib";
import { box } from "./helpers";
import { calibrate, frameBound, note } from "./perf";

// Lane 16 fixes: the two failures the lane 16 verifier found, replayed against the real app.
//
// 1. The 'chain' plugin attack: two ordinary array ops land exactly at the documented model
//    limit of 100,000 solids. The document came through, but the window's thread froze for
//    46 to 54 seconds (the Outliner listed and drew 100,000 rows, quadratically; the cut list
//    and the print report drew 100,000 rows each; a selection's highlight passed over the
//    whole view mesh once per solid), the browser grew to about 7 GB, and the plugin's
//    watchdog stopped the plugin for not answering, while it was the app that was busy.
// 2. F10, texture decode memory: sixteen 4096 x 4096 images of one colour (a .shz of a few
//    hundred kilobytes) took the browser from 480 MB to over 1.5 GB, and the 512-image limit
//    allowed tens of gigabytes. The model now holds at most 128 million texture pixels, and
//    the viewport decodes all of them within 32 million (each at most 2048 a side).
// 3. Lane 16b: selecting all of them held the window's thread for 19 to 31 seconds on
//    GitHub's runners (no GPU there: WebGL is emulated on the CPU). The highlight built
//    1,200,000 fat lines and drew them every frame (8 s a frame emulated), and recoloured
//    every triangle; now it writes a byte per part and per solid into two state textures and
//    the frame costs what it did before. Every frame-gap bound here is stated in units of
//    the machine running it (e2e/perf.ts), so the bounds mean the same on any runner.

const requireKernel = !!process.env.REQUIRE_KERNEL;

async function ready(page: Page) {
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

/** Watch the window's thread from now: the longest gap between two frames. */
async function watchFrames(page: Page) {
  await page.evaluate(() => {
    const w = window as unknown as { __gap: number; __watching?: boolean };
    w.__gap = 0;
    if (w.__watching) return;
    w.__watching = true;
    let last = performance.now();
    const tick = (t: number) => {
      w.__gap = Math.max(w.__gap, t - last);
      last = t;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

const longestGap = (page: Page) => page.evaluate(() => (window as unknown as { __gap: number }).__gap);

const ID = "org.example.chain";
const manifest = {
  id: ID,
  name: "Chain",
  version: "1.0.0",
  api: 1,
  description: "Two array ops that land exactly at the model's limit.",
  author: "Sawhorse",
  license: "MIT",
  permissions: ["model.read", "model.write"],
};
const main = `import { model } from "sawhorse";
const shift = (x) => ({ linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation: [x, 0, 0] });
const report = (k, v) => console.log(k + "=" + JSON.stringify(v));
const attempt = async (k, f) => {
  const t0 = Date.now();
  try {
    const r = await f();
    report(k, { ok: true, r, ms: Date.now() - t0 });
  } catch (e) {
    report(k, { ok: false, code: e && e.code !== undefined ? e.code : null, ms: Date.now() - t0, message: String((e && e.message) || e).slice(0, 200) });
  }
};
const first = (await model.solids())[0].id;
await attempt("a", async () => {
  const b = model.batch("A thousand");
  b.op({ op: "array", objects: [first], xform: shift(0.2), count: 999 });
  await b.commit();
  return (await model.solids()).length;
});
const all = (await model.solids()).map((s) => s.id);
await attempt("over", async () => {
  const b = model.batch("A million");
  b.op({ op: "array", objects: all, xform: shift(0), count: 1000 });
  await b.commit();
});
await attempt("cap", async () => {
  const b = model.batch("Exactly the limit");
  b.op({ op: "array", objects: all, xform: shift(0), count: 99 });
  await b.commit();
});
console.log("done");
`;

type Hook = {
  plugins: { list(): { id: string; status: string; logs: string[] }[] };
  solids(): unknown[];
  parts(): unknown[];
  looks(): { decoded: { images: number; texels: number; largest: [number, number]; pending: number } } | null;
  highlightCounts(): { fatSelected: number; fatHover: number; stateSolids: number; stateParts: number };
};
const highlightCounts = (page: Page) => page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.highlightCounts());

async function logLine(page: Page, prefix: string): Promise<string> {
  let line = "";
  await expect
    .poll(
      async () => {
        const e = await page.evaluate((id) => (window as unknown as { __sawhorse: Hook }).__sawhorse.plugins.list().find((x) => x.id === id), ID);
        line = e?.logs.find((l) => l.includes(prefix)) ?? "";
        return line;
      },
      { timeout: 120_000 },
    )
    .not.toBe("");
  return line.slice(line.indexOf(prefix) + prefix.length);
}

test("a plugin's two array ops that land exactly at the 100,000-solid limit leave the app responsive, and the plugin running", async ({ page }) => {
  test.setTimeout(300_000);
  const vp = await ready(page);
  await box(page);
  await watchFrames(page);

  await page.getByRole("tab", { name: "Plugins" }).click();
  const zip = zipSync({ "chain/plugin.json": strToU8(JSON.stringify(manifest)), "chain/main.js": strToU8(main) });
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.locator('[data-do="load-session"]').click()]);
  await chooser.setFiles({ name: "chain.zip", mimeType: "application/zip", buffer: Buffer.from(zip) });
  const row = page.locator(`.plugin-row[data-plugin="${ID}"]`);
  await expect(row).toContainText("this session");
  await row.locator('[data-do="enable"]').click();
  await page.getByRole("dialog", { name: "Enable Chain" }).getByRole("button", { name: "Enable" }).click();

  await logLine(page, "done");
  const result = async (k: string) => JSON.parse(await logLine(page, `${k}=`)) as { ok: boolean; r?: number; code: string | null; ms: number; message?: string };
  expect(await result("a")).toMatchObject({ ok: true, r: 1000 });
  // A million is refused by the kernel before a copy is made.
  const over = await result("over");
  expect(over.ok).toBe(false);
  expect(over.message).toContain("at most 100000");
  expect(over.ms).toBeLessThan(5000);
  expect(await result("cap")).toMatchObject({ ok: true });
  await expect(vp).toHaveAttribute("data-objects", "100000", { timeout: 60_000 });

  // The window kept drawing frames through all of it, and the plugin was never taken for
  // hung (it was the app that used to be busy). The gap is read before the machine is
  // measured (measuring renders frames of its own). Bounds in this machine's units (e2e/
  // perf.ts): on the development machine this phase measured 13 workloads (0.55 s), 3 under
  // SwiftShader; 40 workloads and 5 renders still fail the 46-second freeze by far.
  const plugin = await longestGap(page);
  const machine = await calibrate(page);
  const pluginBound = frameBound(machine, 40, 5);
  note("two array ops to the limit", plugin, pluginBound, machine);
  expect(plugin).toBeLessThan(pluginBound);
  await expect(row).toHaveAttribute("data-status", "running");

  // Every pane, the selection and the view at that size. A pane changes the view's size, so
  // the view is drawn again: on CI's runners (no GPU) the Cut list measured 2.9 to 3.9 s,
  // two frames; 30 workloads and 5 renders.
  const paneBound = frameBound(machine, 30, 5);
  for (const tab of ["Outliner", "Cut list", "Materials", "Tags", "Outliner"]) {
    await watchFrames(page);
    await page.getByRole("tab", { name: tab }).click();
    await page.waitForTimeout(400);
    const gap = await longestGap(page);
    note(`the ${tab} pane`, gap, paneBound, machine);
    expect(gap, `the ${tab} pane`).toBeLessThan(paneBound);
  }
  // The Outliner lists 100,000 rows and draws the ones in view.
  const tree = page.getByRole("tree", { name: "Groups, components and solids" });
  await expect(tree).toHaveAttribute("data-rows", "100000");
  expect(await tree.getByRole("treeitem").count()).toBeLessThan(400);
  await page.locator("#pane-outliner").evaluate((e) => (e.scrollTop = e.scrollHeight / 2));
  await expect(tree.locator('[aria-posinset="50000"]')).toHaveCount(1);
  // Select everything, then nothing. Measured (longest gap, in this machine's units): the
  // development machine under 0.1 s with its GPU; 2.3 workloads under 6x CPU throttling;
  // 1.6 renders under SwiftShader at 1x and 4x. Before lane 16b: 19 workloads (6x) and 22
  // renders (SwiftShader), the 19 to 31 seconds of CI. 12 workloads and 5 renders sit
  // well above the first (the whole suite in parallel: 6 workloads, under the floor) and
  // well below the second.
  await page.locator("#cv").click({ position: { x: 5, y: 5 } });
  await page.waitForTimeout(400);
  await watchFrames(page);
  await page.keyboard.press("Control+a");
  await expect(page.locator("#hint")).toContainText("Selected 100000 items", { timeout: 30_000 });
  await page.waitForTimeout(400);
  // Drawn from the state textures: no fat line built, every solid's byte and every part's set.
  const parts = await page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.parts().length);
  expect(await highlightCounts(page)).toMatchObject({ fatSelected: 0, stateSolids: 100000, stateParts: parts });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  // Nothing selected (the Push tool's target face may still show as one).
  const none = await highlightCounts(page);
  expect(none).toMatchObject({ fatSelected: 0, stateSolids: 0 });
  expect(none.stateParts).toBeLessThanOrEqual(1);
  const all = await longestGap(page);
  const allBound = frameBound(machine, 12, 5);
  note("select all, then nothing", all, allBound, machine);
  expect(all, "select all").toBeLessThan(allBound);
  // The page's own heap stays well under what the 46-second freeze built up.
  const heap = await page.evaluate(() => (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize ?? 0);
  expect(heap).toBeLessThan(1.5e9);
  // And it still edits: undo the last array.
  await page.keyboard.press("Control+z");
  await expect(vp).toHaveAttribute("data-objects", "1000", { timeout: 60_000 });
  // A thousand solids (12,000 edge segments) are still drawn with the fat highlight lines.
  await page.locator("#cv").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("Control+a");
  await expect(page.locator("#hint")).toContainText("Selected 1000 items", { timeout: 30_000 });
  await expect.poll(() => highlightCounts(page)).toMatchObject({ fatSelected: 12000, stateSolids: 0 });
});

// ---- F10 ----------------------------------------------------------------------------------

function crc32(b: Uint8Array): number {
  let c = 0xffffffff;
  for (const x of b) {
    c ^= x;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  }
  return (c ^ 0xffffffff) >>> 0;
}

/** A real 8-bit grey PNG of `side` x `side` pixels of one value (a few kilobytes). */
function greyPng(side: number, fill: number): Buffer {
  const chunk = (type: string, data: Buffer) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(td));
    return Buffer.concat([len, td, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(side, 0);
  ihdr.writeUInt32BE(side, 4);
  ihdr[8] = 8;
  ihdr[9] = 0;
  const raw = Buffer.alloc(side * (side + 1), fill);
  for (let y = 0; y < side; y++) raw[y * (side + 1)] = 0;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw, { level: 9 })), chunk("IEND", Buffer.alloc(0))]);
}

/** A .shz of one box's worth of nothing but `n` distinct 4096 x 4096 images, each named by a material. */
function texturedShz(n: number): Buffer {
  const glbJson = Buffer.from(JSON.stringify({ asset: { version: "2.0" } }));
  const pad = Buffer.alloc((4 - (glbJson.length % 4)) % 4, 0x20);
  const head = Buffer.alloc(20);
  head.writeUInt32LE(0x46546c67, 0);
  head.writeUInt32LE(2, 4);
  head.writeUInt32LE(20 + glbJson.length + pad.length, 8);
  head.writeUInt32LE(glbJson.length + pad.length, 12);
  head.writeUInt32LE(0x4e4f534a, 16);
  const paths = Array.from({ length: n }, (_, i) => `assets/t${i}.png`);
  const scene = {
    format: "sawhorse-shz",
    version: 1,
    generator: "lane-16 test",
    units: { length: "metre", up: "Z" },
    geometry: "geometry.glb",
    sketches: [],
    objects: [],
    meshes: [],
    groups: [],
    tags: {},
    materials: paths.map((p, i) => ({ name: `M${i}`, color: [200, 100, 50, 255], texture: p })),
    textures: paths.map((p) => ({ path: p, mime: "image/png" })),
    scenes: [],
    guides: [],
    dimensions: [],
  };
  const files: Record<string, Uint8Array | [Uint8Array, { level: 0 }]> = {
    mimetype: [strToU8("application/vnd.sawhorse.shz"), { level: 0 }],
    "scene.json": strToU8(JSON.stringify(scene)),
    "geometry.glb": Buffer.concat([head, glbJson, pad]),
  };
  paths.forEach((p, i) => (files[p] = [greyPng(4096, 10 + i * 7), { level: 0 }]));
  return Buffer.from(zipSync(files));
}

test("F10: texture images are held to 128 million pixels in all, and the viewport decodes them within 32 million", async ({ page }) => {
  test.setTimeout(180_000);
  const vp = await ready(page);
  const hint = page.locator("#hint");

  // Sixteen 4096 x 4096 images (a few hundred kilobytes): refused before any is drawn.
  const sixteen = texturedShz(16);
  expect(sixteen.length).toBeLessThan(1_000_000);
  await page.getByTestId("file-input").setInputFiles({ name: "sixteen.shz", mimeType: "application/octet-stream", buffer: sixteen });
  await expect(hint).toContainText("million pixels", { timeout: 30_000 });
  await expect(hint).toContainText("at most 128 million");
  const none = await page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.looks()?.decoded);
  expect(none?.images ?? 0).toBe(0);
  await expect(vp).toHaveAttribute("data-kernel", "ready");

  // Eight (exactly the limit) open; the viewport decodes each at most 2048 a side, all of
  // them within its budget, one at a time.
  await page.getByTestId("file-input").setInputFiles({ name: "eight.shz", mimeType: "application/octet-stream", buffer: texturedShz(8) });
  await expect(hint).toContainText("Opened eight.shz", { timeout: 30_000 });
  await expect
    .poll(async () => page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.looks()?.decoded ?? null), { timeout: 60_000 })
    .toMatchObject({ images: 8, pending: 0 });
  const decoded = (await page.evaluate(() => (window as unknown as { __sawhorse: Hook }).__sawhorse.looks()!.decoded))!;
  expect(decoded.texels).toBeLessThanOrEqual(32 * 1024 * 1024);
  expect(decoded.largest[0]).toBeLessThanOrEqual(2048);
  expect(decoded.largest[1]).toBeLessThanOrEqual(2048);

  // The Materials pane shows small copies, not the images themselves.
  await page.getByRole("tab", { name: "Materials" }).click();
  const swatchUrls = () =>
    page.evaluate(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[aria-label="Materials in the model"] .swatch'))
        .map((e) => /url\(\"?(blob:[^")]+)\"?\)/.exec(e.getAttribute("style") ?? "")?.[1])
        .filter((u): u is string => !!u),
    );
  await expect.poll(async () => (await swatchUrls()).length, { timeout: 30_000 }).toBe(8);
  const sizes = await page.evaluate(async (urls) => {
    const out: number[] = [];
    for (const u of urls) {
      const img = new Image();
      img.src = u;
      await img.decode();
      out.push(Math.max(img.naturalWidth, img.naturalHeight));
    }
    return out;
  }, await swatchUrls());
  expect(sizes).toHaveLength(8);
  for (const s of sizes) expect(s).toBeLessThanOrEqual(96);
});
