// Lane 16 fixes (the verifier's F10): what the viewport decodes of a model's texture images.
// The browser decodes an image to 4 bytes a pixel, and sixteen 4096 x 4096 images of one
// colour (a few hundred kilobytes of .shz) used to be a gigabyte. The viewport now decodes
// them one at a time, each at most 2048 a side and all of them within 32 million pixels.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TextureCache, VIEW_TEXEL_BUDGET, VIEW_TEXTURE_SIDE, viewFactor, viewSize } from "./looks";

/** A PNG that is only its header: `w` x `h`, made distinct by `n`. */
function pngHeader(w: number, h: number, n = 0): Uint8Array {
  const b = new Uint8Array(34);
  b.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
  const dv = new DataView(b.buffer);
  dv.setUint32(16, w);
  dv.setUint32(20, h);
  b[24] = 8;
  b[25] = 6;
  b[33] = n;
  return b;
}

describe("the viewport's share of each texture image", () => {
  it("an image fits the side limit; a few images keep their size", () => {
    expect(viewSize(1024, 512, 1)).toEqual({ width: 1024, height: 512 });
    expect(viewSize(4096, 4096, 1)).toEqual({ width: VIEW_TEXTURE_SIDE, height: VIEW_TEXTURE_SIDE });
    expect(viewSize(8000, 1000, 1)).toEqual({ width: 2048, height: 256 });
    expect(viewFactor([{ width: 1024, height: 1024 }, { width: 4096, height: 4096 }])).toBe(1);
  });

  it("many large images share the budget: every one a little coarser, never more than the budget together", () => {
    const sixteen = Array.from({ length: 16 }, () => ({ width: 4096, height: 4096 }));
    const f = viewFactor(sixteen);
    expect(f).toBeLessThan(1);
    const total = sixteen.reduce((t, x) => {
      const s = viewSize(x.width, x.height, f);
      return t + s.width * s.height;
    }, 0);
    expect(total).toBeLessThanOrEqual(VIEW_TEXEL_BUDGET);
    expect(total).toBeGreaterThan(VIEW_TEXEL_BUDGET * 0.95);
    // The model's 512-image limit, at the kernel's 128 million pixels in all.
    const many = Array.from({ length: 512 }, () => ({ width: 512, height: 512 }));
    const g = viewFactor(many);
    const sum = many.reduce((t, x) => t + viewSize(x.width, x.height, g).width * viewSize(x.width, x.height, g).height, 0);
    expect(sum).toBeLessThanOrEqual(VIEW_TEXEL_BUDGET);
  });
});

describe("TextureCache", () => {
  const calls: { size: number; to: { width: number; height: number } | null }[] = [];
  let inFlight = 0;
  let most = 0;
  let closed = 0;
  beforeEach(() => {
    calls.length = 0;
    inFlight = 0;
    most = 0;
    closed = 0;
    vi.stubGlobal("createImageBitmap", async (blob: Blob, opts?: { resizeWidth?: number; resizeHeight?: number }) => {
      inFlight++;
      most = Math.max(most, inFlight);
      calls.push({ size: blob.size, to: opts?.resizeWidth ? { width: opts.resizeWidth, height: opts.resizeHeight ?? 0 } : null });
      await new Promise((r) => setTimeout(r, 1));
      inFlight--;
      const head = new DataView(await blob.arrayBuffer());
      return { width: opts?.resizeWidth ?? head.getUint32(16), height: opts?.resizeHeight ?? head.getUint32(20), close: () => closed++ };
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  const settle = async (c: TextureCache) => {
    for (let i = 0; i < 400 && c.stats().pending; i++) await new Promise((r) => setTimeout(r, 2));
  };

  it("sixteen 4096 x 4096 images: decoded one at a time, smaller, within the budget together", async () => {
    let ready = 0;
    const c = new TextureCache(1, () => ready++);
    for (let n = 0; n < 16; n++) c.add(`d${n}`, "image/png", pngHeader(4096, 4096, n));
    await settle(c);
    expect(most).toBe(1);
    const s = c.stats();
    expect(s.images).toBe(16);
    expect(s.texels).toBeLessThanOrEqual(VIEW_TEXEL_BUDGET);
    expect(s.largest[0]).toBeLessThanOrEqual(VIEW_TEXTURE_SIDE);
    // Every decode asked the browser for the smaller size.
    expect(calls.every((x) => x.to && x.to.width < 4096)).toBe(true);
    expect(ready).toBeGreaterThanOrEqual(16);
    c.dispose();
  });

  it("images that come later shrink the share: one drawn at more than twice its share is decoded again, smaller", async () => {
    const c = new TextureCache(1, () => undefined);
    c.add("a", "image/png", pngHeader(2048, 2048, 1));
    await settle(c);
    expect(c.stats().largest).toEqual([2048, 2048]);
    for (let n = 0; n < 31; n++) c.add(`b${n}`, "image/png", pngHeader(2048, 2048, n + 2));
    await settle(c);
    const s = c.stats();
    expect(s.images).toBe(32);
    expect(s.texels).toBeLessThanOrEqual(VIEW_TEXEL_BUDGET);
    expect(s.largest[0]).toBeLessThan(2048);
    // The first decode's pixels were let go when it was replaced.
    expect(closed).toBeGreaterThanOrEqual(1);
    c.dispose();
  });

  it("an image the model no longer holds lets its pixels go, and comes back (an undo) without its bytes being sent again", async () => {
    const c = new TextureCache(1, () => undefined);
    c.add("a", "image/png", pngHeader(64, 64, 1));
    c.add("b", "image/png", pngHeader(64, 64, 2));
    await settle(c);
    expect(c.stats().images).toBe(2);
    c.keep(["b"]);
    expect(c.get("a")).toBeNull();
    expect(c.stats().images).toBe(1);
    c.keep(["a", "b"]);
    await settle(c);
    expect(c.get("a")).not.toBeNull();
    expect(c.stats().images).toBe(2);
    c.dispose();
  });
});
