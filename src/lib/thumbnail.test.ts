// Lane 16 fixes: a material's swatch is a small copy of its texture image, never the image
// itself (an <img> of a 4096 x 4096 texture is decoded whole: 64 MB for a 40 px swatch).
import { afterEach, describe, expect, it, vi } from "vitest";
import { THUMB_SIDE, thumbSize, thumbnailUrl } from "./thumbnail";

function pngHeader(w: number, h: number): Uint8Array {
  const b = new Uint8Array(33);
  b.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
  const dv = new DataView(b.buffer);
  dv.setUint32(16, w);
  dv.setUint32(20, h);
  return b;
}

afterEach(() => vi.unstubAllGlobals());

describe("swatch thumbnails", () => {
  it("are at most THUMB_SIDE a side, in proportion, and never larger than the image", () => {
    expect(thumbSize(4096, 4096)).toEqual({ width: THUMB_SIDE, height: THUMB_SIDE });
    expect(thumbSize(4096, 1024)).toEqual({ width: THUMB_SIDE, height: THUMB_SIDE / 4 });
    expect(thumbSize(32, 16)).toEqual({ width: 32, height: 16 });
  });

  it("are decoded at their small size, one image at a time; null where the browser cannot", async () => {
    // jsdom has no createImageBitmap: the caller shows the image itself.
    expect(await thumbnailUrl(pngHeader(4096, 4096), "image/png")).toBeNull();
    let inFlight = 0;
    let most = 0;
    const asked: unknown[] = [];
    vi.stubGlobal("createImageBitmap", async (_b: Blob, o: unknown) => {
      inFlight++;
      most = Math.max(most, inFlight);
      asked.push(o);
      await new Promise((r) => setTimeout(r, 2));
      inFlight--;
      return { width: 96, height: 96, close: () => undefined };
    });
    vi.stubGlobal(
      "OffscreenCanvas",
      class {
        getContext() {
          return { drawImage: () => undefined };
        }
        async convertToBlob() {
          return new Blob(["png"], { type: "image/png" });
        }
      },
    );
    const urls = await Promise.all(Array.from({ length: 6 }, () => thumbnailUrl(pngHeader(4096, 4096), "image/png")));
    expect(urls.every((u) => typeof u === "string")).toBe(true);
    expect(most).toBe(1);
    expect(asked[0]).toMatchObject({ resizeWidth: THUMB_SIDE, resizeHeight: THUMB_SIDE });
  });
});
