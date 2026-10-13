// Small copies of the model's texture images for the Materials pane's swatches (lane 16).
// A swatch is drawn about 40 px across, but an <img> of the image itself is decoded whole
// by the browser, at 4 bytes a pixel: a 4096 x 4096 texture is 64 MB for one swatch. Each
// image is decoded once, here, one at a time, to a PNG of at most `THUMB_SIDE` a side.
import { imageSize } from "./imageSize";

/** Longest side of a swatch image. */
export const THUMB_SIDE = 96;

let chain: Promise<unknown> = Promise.resolve();

/** The size a `width` x `height` image is brought to (never larger than it is). */
export function thumbSize(width: number, height: number, side = THUMB_SIDE): { width: number; height: number } {
  const s = Math.min(1, side / Math.max(width, height, 1));
  return { width: Math.max(1, Math.round(width * s)), height: Math.max(1, Math.round(height * s)) };
}

/**
 * An object URL of a small PNG of the image in `bytes`, or null where this browser cannot
 * make one (no createImageBitmap or canvas) or the image does not decode. Calls are made one
 * after another, so at most one full-size image is decoded at a time.
 */
export function thumbnailUrl(bytes: Uint8Array, mime: string, side = THUMB_SIDE): Promise<string | null> {
  const run = chain.then(() => make(bytes, mime, side));
  chain = run.catch(() => null);
  return run.catch(() => null);
}

async function make(bytes: Uint8Array, mime: string, side: number): Promise<string | null> {
  if (typeof createImageBitmap !== "function" || typeof URL === "undefined" || !URL.createObjectURL) return null;
  const px = imageSize(bytes);
  if (!px || !px.width || !px.height) return null;
  const to = thumbSize(px.width, px.height, side);
  const bmp = await createImageBitmap(new Blob([bytes as BlobPart], { type: mime }), { resizeWidth: to.width, resizeHeight: to.height, resizeQuality: "medium" });
  try {
    let blob: Blob | null = null;
    if (typeof OffscreenCanvas === "function") {
      const c = new OffscreenCanvas(to.width, to.height);
      const g = c.getContext("2d");
      if (!g) return null;
      g.drawImage(bmp, 0, 0);
      blob = await c.convertToBlob({ type: "image/png" });
    } else if (typeof document !== "undefined") {
      const c = document.createElement("canvas");
      c.width = to.width;
      c.height = to.height;
      const g = c.getContext("2d");
      if (!g) return null;
      g.drawImage(bmp, 0, 0);
      blob = await new Promise<Blob | null>((r) => c.toBlob(r, "image/png"));
    }
    return blob ? URL.createObjectURL(blob) : null;
  } finally {
    bmp.close();
  }
}
