import type { Page } from "@playwright/test";

// The perceptual image comparison the golden-image tests share (the .skp corpus and the
// painted reference): both images box-filtered down to 96 x 72 luminance cells in the page
// and compared cell by cell, so anti-aliasing and GPU differences do not count.

/** Largest mean cell difference (0..1) and share of cells off by more than 0.25 allowed. */
export const MAX_MEAN = 0.03;
export const MAX_BAD = 0.01;

/** Perceptual difference of two PNGs, computed in the page (canvas box filter). */
export async function perceptual(page: Page, a: Buffer, b: Buffer) {
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

/**
 * Lane 20b: the golden images were taken with the side panel at 300 px (its width before the
 * usual became 360), so the model's canvas keeps that size: a wider panel is a narrower
 * canvas, and the same model drawn in a different shape. Call before the page loads.
 */
export async function goldenPanelWidth(page: Page) {
  await page.addInitScript(() => {
    try {
      localStorage.setItem("sawhorse.sideWidth", "300");
    } catch {
      /* storage blocked: the panel keeps its usual width */
    }
  });
}
