import { expect, test, type Page } from "@playwright/test";
import { crc32, deflateRawSync } from "node:zlib";
import { calibrate } from "./perf";

// Lane 16b: the model's count limits on guides, dimensions, tags and materials, replayed
// the way the lane 16 attacker found them: valid .shz files of a few megabytes whose
// scene.json lists millions of each (2,000,000 guides in a 5 MB file crashed the tab;
// 1,000,000 materials took the browser to 6.2 GB). Each is now refused by the count it
// declares, as the kernel inflates scene.json, at the first element past the limit:
// nothing of it is parsed into the model, the rest is never inflated, and the kernel's
// memory and the page's heap do not grow. Every call into the page has a hard deadline, so
// a renderer that hangs fails the test at once instead of at its timeout.

const requireKernel = !!process.env.REQUIRE_KERNEL;

/** A page call that must answer within `ms`. */
async function within<T>(what: string, ms: number, p: Promise<T>): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what}: no answer within ${ms} ms (the page is frozen or gone)`)), ms);
  });
  try {
    return await Promise.race([p, late]);
  } finally {
    clearTimeout(timer);
  }
}

async function ready(page: Page) {
  await page.goto("/?autosave=0&io=basic&kerneltest=1");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
  await expect(vp).toHaveAttribute("data-kernel", "ready");
  return vp;
}

type Hooks = { __sawhorse: { kernelTest: { wasmBytes(): Promise<number> } } };

/** The kernel's WebAssembly memory (it only grows: the most it ever held) and the page's
 * JavaScript heap after a garbage collection, bytes. */
async function memory(page: Page) {
  const cdp = await page.context().newCDPSession(page);
  await within("collect garbage", 10_000, cdp.send("HeapProfiler.collectGarbage"));
  await cdp.detach();
  return within(
    "memory",
    10_000,
    page.evaluate(async () => ({
      wasm: await (window as unknown as Hooks).__sawhorse.kernelTest.wasmBytes(),
      heap: (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize ?? 0,
    })),
  );
}

/** A .shz (mimetype, scene.json, an empty geometry.glb) whose scene.json lists `body`
 * under `key`. The JSON is written as text and deflated by zlib: a few megabytes. */
function shz(key: string, body: string): Buffer {
  const glbJson = Buffer.from('{"asset":{"version":"2.0"}}   ');
  const head = Buffer.alloc(20);
  head.writeUInt32LE(0x46546c67, 0);
  head.writeUInt32LE(2, 4);
  head.writeUInt32LE(20 + glbJson.length, 8);
  head.writeUInt32LE(glbJson.length, 12);
  head.writeUInt32LE(0x4e4f534a, 16);
  const base: Record<string, unknown> = { format: "sawhorse-shz", version: 1, generator: "lane-16b test", units: { length: "metre", up: "Z" }, geometry: "geometry.glb", sketches: [], objects: [], meshes: [], groups: [], tags: {}, materials: [], scenes: [], guides: [], dimensions: [] };
  delete base[key];
  const scene = Buffer.from(`${JSON.stringify(base).slice(0, -1)},${JSON.stringify(key)}:${body}}`);
  return zip([
    { name: "mimetype", data: Buffer.from("application/vnd.sawhorse.shz"), deflate: false },
    { name: "scene.json", data: scene, deflate: true },
    { name: "geometry.glb", data: Buffer.concat([head, glbJson]), deflate: false },
  ]);
}

/** A plain zip (stored or deflated entries, honest sizes and CRCs). */
function zip(entries: { name: string; data: Buffer; deflate: boolean }[]): Buffer {
  const local: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  for (const e of entries) {
    const body = e.deflate ? deflateRawSync(e.data, { level: 6 }) : e.data;
    const name = Buffer.from(e.name);
    const h = Buffer.alloc(30);
    h.writeUInt32LE(0x04034b50, 0);
    h.writeUInt16LE(20, 4);
    h.writeUInt16LE(e.deflate ? 8 : 0, 8);
    h.writeUInt32LE(crc32(e.data), 14);
    h.writeUInt32LE(body.length, 18);
    h.writeUInt32LE(e.data.length, 22);
    h.writeUInt16LE(name.length, 26);
    local.push(h, name, body);
    const c = Buffer.alloc(46);
    c.writeUInt32LE(0x02014b50, 0);
    c.writeUInt16LE(20, 4);
    c.writeUInt16LE(20, 6);
    c.writeUInt16LE(e.deflate ? 8 : 0, 10);
    c.writeUInt32LE(crc32(e.data), 16);
    c.writeUInt32LE(body.length, 20);
    c.writeUInt32LE(e.data.length, 24);
    c.writeUInt16LE(name.length, 28);
    c.writeUInt32LE(offset, 42);
    central.push(c, name);
    offset += 30 + name.length + body.length;
  }
  const cd = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, cd, end]);
}

/** `n` items joined as a JSON list (or object, `map`). */
function many(n: number, item: (i: number) => string, map = false): string {
  const parts: string[] = new Array(n);
  for (let i = 0; i < n; i++) parts[i] = item(i);
  return (map ? "{" : "[") + parts.join(",") + (map ? "}" : "]");
}

const CASES = [
  { key: "guides", n: 2_000_000, words: "guides", max: 50_000, item: (i: number) => `{"point":[${(i % 1000) * 0.001},${Math.floor(i / 1000) * 0.001},0],"dir":[0,1,0]}` },
  { key: "dimensions", n: 2_000_000, words: "dimensions", max: 50_000, item: (i: number) => `{"a":[${i * 1e-4},0,0],"b":[${i * 1e-4},1,0],"offset":[0,0,0.1]}` },
  { key: "tags", n: 1_000_000, words: "tags", max: 10_000, map: true, item: (i: number) => `"t${i}":true` },
  { key: "materials", n: 1_000_000, words: "materials", max: 4_096, item: (i: number) => `{"name":"m${i}","color":[${i % 256},2,3,255]}` },
];

for (const c of CASES) {
  test(`a .shz listing ${c.n.toLocaleString("en-US")} ${c.words} is refused by that count before any is parsed, at once, and nothing grows`, async ({ page }) => {
    test.setTimeout(120_000);
    const vp = await ready(page);
    const file = shz(c.key, many(c.n, c.item, c.map));
    // What the attacker sent: a file of a few megabytes.
    expect(file.length).toBeLessThan(16 * 1024 * 1024);
    // This machine, so the time bound means the same on a slower runner (e2e/perf.ts):
    // 100 ms on a machine whose workload takes 30 ms or less (the development machine's
    // takes 30 to 55 ms; its refusals took 16 to 44 ms), more in proportion on a slower or
    // busier one (the full suite in parallel there: workload 110 to 125 ms, refusals 35 to
    // 233 ms).
    const machine = await calibrate(page);
    const bound = Math.round(100 * Math.max(1, machine.cpu / 30));
    const before = await memory(page);
    // Time it in the page: from the file input's change to the refusal in the status line.
    await within(
      "arm",
      5_000,
      page.evaluate(() => {
        const w = window as unknown as { __refusal: { start: number; end: number; text: string } };
        w.__refusal = { start: 0, end: 0, text: "" };
        document.querySelector('[data-testid="file-input"]')!.addEventListener("change", () => (w.__refusal.start = performance.now()), { capture: true, once: true });
        const hint = document.querySelector("#hint")!;
        new MutationObserver((_, obs) => {
          const text = hint.textContent ?? "";
          if (/the file holds more than/.test(text)) {
            w.__refusal.end = performance.now();
            w.__refusal.text = text;
            obs.disconnect();
          }
        }).observe(hint, { childList: true, subtree: true, characterData: true });
      }),
    );
    await page.getByTestId("file-input").setInputFiles({ name: `${c.key}.shz`, mimeType: "application/octet-stream", buffer: file });
    await expect(page.locator("#hint")).toContainText(`the file holds more than ${c.max} ${c.words}; a model holds at most ${c.max}`, { timeout: 30_000 });
    const r = await within("timing", 5_000, page.evaluate(() => (window as unknown as { __refusal: { start: number; end: number } }).__refusal));
    const took = r.end - r.start;
    console.log(`[limits] ${c.n} ${c.words} (${(file.length / 1048576).toFixed(1)} MB): refused in ${took.toFixed(1)} ms (bound ${bound} ms, cpu ${machine.cpu.toFixed(1)} ms)`);
    expect(r.start).toBeGreaterThan(0);
    expect(took).toBeLessThan(bound);
    // Nothing was built: the model is as it was. The kernel's memory grew by the file (the
    // kernel is handed a copy of it) and what was read of scene.json before the refusal,
    // not by the tens of megabytes it inflates to, nor the model it describes; the page's
    // heap is where it was.
    await expect(vp).toHaveAttribute("data-objects", "0");
    const after = await memory(page);
    const grew = { wasm: after.wasm - before.wasm, heap: after.heap - before.heap };
    console.log(`[limits] ${c.words}: kernel memory grew ${(grew.wasm / 1048576).toFixed(1)} MB, page heap ${(grew.heap / 1048576).toFixed(1)} MB`);
    expect(grew.wasm).toBeLessThan(file.length + 16 * 1024 * 1024);
    expect(grew.heap).toBeLessThan(16 * 1024 * 1024);
    await expect(vp).toHaveAttribute("data-kernel", "ready");
  });
}

test("a .shz at exactly a limit still opens (50,000 guides)", async ({ page }) => {
  test.setTimeout(120_000);
  await ready(page);
  const file = shz("guides", many(50_000, CASES[0].item));
  await page.getByTestId("file-input").setInputFiles({ name: "fifty-thousand.shz", mimeType: "application/octet-stream", buffer: file });
  await expect(page.locator("#hint")).toContainText("Opened fifty-thousand.shz", { timeout: 60_000 });
});
