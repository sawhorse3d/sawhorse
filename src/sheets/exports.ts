// Sheets out of the app (lane 12): PDF (vector, the real paper size, one page per sheet,
// the lane 7 writer), SVG (paper units), PNG at 150 or 300 dpi, and printing through the
// system print dialog at 100%. DXF is the kernel's (crates/export/src/sheet_dxf.rs). Every
// one draws the kernel's own layout of the sheet, so they all match the canvas.
import { PdfDoc, fit } from "../cutlist/pdf";
import type { PaperUnits } from "../kernel/sheetTypes";
import { IN, sheetSvg, type Prim, type SheetRender } from "./render";

/** Points (1/72 in) per metre. */
export const PT = 72 / IN;

/** An image a sheet shows, ready for a PDF (JPEG on white) and an SVG or PNG (data URL). */
export interface SheetImage {
  jpeg: Uint8Array;
  width: number;
  height: number;
  dataUrl: string;
}

/** The sheets as one PDF, a page per sheet at its paper size, everything vector but the
 * images. Deterministic: the same sheets give the same bytes. */
export function sheetsPdf(renders: SheetRender[], images: Map<string, SheetImage>, title: string): Uint8Array {
  const doc = new PdfDoc({ title, creator: "Sawhorse" });
  const names = new Map<string, string>();
  for (const [path, im] of [...images].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) names.set(path, doc.addJpeg(im.jpeg, im.width, im.height));
  for (const r of renders) {
    const [W, H] = [r.size[0] * PT, r.size[1] * PT];
    const page = doc.page([W, H]);
    const X = (x: number) => x * PT;
    const Y = (y: number) => H - y * PT;
    for (const p of r.prims) drawPrim(page, p, X, Y, names);
  }
  return doc.bytes();
}

function drawPrim(page: ReturnType<PdfDoc["page"]>, p: Prim, X: (x: number) => number, Y: (y: number) => number, names: Map<string, string>) {
  switch (p.p) {
    case "line":
      page.lineRGB(X(p.a[0]), Y(p.a[1]), X(p.b[0]), Y(p.b[1]), p.width * PT, p.color, p.dash ? [p.dash[0] * PT, p.dash[1] * PT] : undefined);
      return;
    case "fill":
      page.fillRings(
        p.rings.map((r) => r.map((q): [number, number] => [X(q[0]), Y(q[1])])),
        p.color,
      );
      return;
    case "rect":
      page.rectRGB(X(p.rect[0]), Y(p.rect[1] + p.rect[3]), p.rect[2] * PT, p.rect[3] * PT, { stroke: p.stroke, fill: p.fill, width: p.width * PT });
      return;
    case "text": {
      const s = p.max !== undefined ? fit(p.text, p.size, p.max, !!p.bold) : p.text;
      page.textRGB(X(p.at[0]), Y(p.at[1]), s, p.size * PT, { rgb: p.color, bold: p.bold, angle: p.angle, align: p.anchor === "middle" ? "center" : p.anchor === "end" ? "right" : "left" });
      return;
    }
    case "image": {
      const name = names.get(p.path);
      if (name) page.image(name, X(p.rect[0]), Y(p.rect[1] + p.rect[3]), p.rect[2] * PT, p.rect[3] * PT);
      else page.rectRGB(X(p.rect[0]), Y(p.rect[1] + p.rect[3]), p.rect[2] * PT, p.rect[3] * PT, { stroke: [170, 174, 170], width: 0.5 });
      return;
    }
  }
}

/** The sheet as an SVG file (paper units), images inside it. */
export function sheetSvgFile(r: SheetRender, images: Map<string, SheetImage>, units: PaperUnits): string {
  return sheetSvg(r, (p) => images.get(p)?.dataUrl ?? null, units);
}

/** Every image the sheets show, read from the model and decoded once (browser only):
 * JPEG on white for the PDF, a data URL for SVG and PNG. */
export async function sheetImages(renders: SheetRender[], read: (path: string) => Promise<Uint8Array>): Promise<Map<string, SheetImage>> {
  const out = new Map<string, SheetImage>();
  const paths = new Set(renders.flatMap((r) => r.prims.flatMap((p) => (p.p === "image" ? [p.path] : []))));
  for (const path of paths) {
    const bytes = await read(path);
    const mime = bytes[0] === 0xff && bytes[1] === 0xd8 ? "image/jpeg" : "image/png";
    const blob = new Blob([bytes as BlobPart], { type: mime });
    const bmp = await createImageBitmap(blob);
    // At most 2400 px on the long side: plenty for print at the sizes sheets use.
    const k = Math.min(1, 2400 / Math.max(bmp.width, bmp.height));
    const [w, h] = [Math.max(1, Math.round(bmp.width * k)), Math.max(1, Math.round(bmp.height * k))];
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) continue;
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, w, h);
    ctx.drawImage(bmp, 0, 0, w, h);
    bmp.close();
    const jpeg = await new Promise<Blob | null>((ok) => canvas.toBlob(ok, "image/jpeg", 0.92));
    if (!jpeg) continue;
    out.set(path, { jpeg: new Uint8Array(await jpeg.arrayBuffer()), width: w, height: h, dataUrl: `data:${mime};base64,${base64(bytes)}` });
  }
  return out;
}

function base64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

// ---- PNG --------------------------------------------------------------------------------

/** The sheet as a PNG at `dpi` (white paper, its real size at that resolution, the dpi
 * written in the file's pHYs chunk). Browser only. */
export async function sheetPng(r: SheetRender, images: Map<string, SheetImage>, units: PaperUnits, dpi: number): Promise<Uint8Array> {
  const svg = sheetSvgFile(r, images, units);
  const [w, h] = [Math.round((r.size[0] / IN) * dpi), Math.round((r.size[1] / IN) * dpi)];
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    const img = new Image();
    img.decoding = "async";
    img.src = url;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no 2D canvas in this browser");
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, w, h);
    ctx.drawImage(img, 0, 0, w, h);
    const blob = await new Promise<Blob | null>((ok) => canvas.toBlob(ok, "image/png"));
    if (!blob) throw new Error("the browser could not make the PNG (too large?)");
    return withDpi(new Uint8Array(await blob.arrayBuffer()), dpi);
  } finally {
    URL.revokeObjectURL(url);
  }
}

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

export function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/** A PNG with its resolution set (a pHYs chunk right after IHDR, replacing any). */
export function withDpi(png: Uint8Array, dpi: number): Uint8Array {
  const sig = [137, 80, 78, 71, 13, 10, 26, 10];
  if (png.length < 33 || sig.some((b, i) => png[i] !== b)) throw new Error("not a PNG");
  const view = new DataView(png.buffer, png.byteOffset, png.byteLength);
  const chunks: Uint8Array[] = [png.subarray(0, 8)];
  let at = 8;
  let ihdr = true;
  while (at + 12 <= png.length) {
    const len = view.getUint32(at);
    const type = String.fromCharCode(...png.subarray(at + 4, at + 8));
    const end = at + 12 + len;
    if (type !== "pHYs") chunks.push(png.subarray(at, end));
    if (ihdr && type === "IHDR") {
      const ppm = Math.round(dpi / IN);
      const body = new Uint8Array(17);
      const dv = new DataView(body.buffer);
      body.set([0x70, 0x48, 0x59, 0x73], 0); // "pHYs"
      dv.setUint32(4, ppm);
      dv.setUint32(8, ppm);
      body[12] = 1; // the unit is the metre
      const chunk = new Uint8Array(4 + 13 + 4);
      const cv = new DataView(chunk.buffer);
      cv.setUint32(0, 9);
      chunk.set(body.subarray(0, 13), 4);
      cv.setUint32(17, crc32(body.subarray(0, 13)));
      chunks.push(chunk);
      ihdr = false;
    }
    at = end;
  }
  const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let o = 0;
  for (const c of chunks) {
    out.set(c, o);
    o += c.length;
  }
  return out;
}

/** The dots per inch a PNG declares (null: none). */
export function pngDpi(png: Uint8Array): number | null {
  const view = new DataView(png.buffer, png.byteOffset, png.byteLength);
  let at = 8;
  while (at + 12 <= png.length) {
    const len = view.getUint32(at);
    const type = String.fromCharCode(...png.subarray(at + 4, at + 8));
    if (type === "pHYs" && png[at + 16] === 1) return Math.round(view.getUint32(at + 8) * IN);
    at += 12 + len;
  }
  return null;
}

// ---- printing ---------------------------------------------------------------------------

/** Print the sheets through the system print dialog (the desktop app's and the browser's):
 * a page per sheet at its paper size, 100%, nothing else of the window. */
export function printSheets(renders: SheetRender[], images: Map<string, SheetImage>, units: PaperUnits): void {
  if (!renders.length) return;
  const [w, h] = renders[0].size;
  const size = (m: number) => (units === "millimetres" ? `${Math.round((m / 0.001) * 100) / 100}mm` : `${Math.round((m / IN) * 1000) / 1000}in`);
  const holder = document.createElement("div");
  holder.className = "print-sheets";
  holder.innerHTML = renders.map((r) => `<div class="print-sheet-page">${sheetSvgFile(r, images, units).replace(/^<\?xml[^>]*>\s*/, "")}</div>`).join("");
  const style = document.createElement("style");
  style.dataset.printSheets = "";
  style.textContent = `@page { size: ${size(w)} ${size(h)}; margin: 0 }`;
  document.head.appendChild(style);
  document.body.appendChild(holder);
  document.body.classList.add("print-sheet");
  const done = () => {
    document.body.classList.remove("print-sheet");
    holder.remove();
    style.remove();
    window.removeEventListener("afterprint", done);
  };
  window.addEventListener("afterprint", done);
  window.print();
  // Some hosts print synchronously and never send afterprint.
  setTimeout(() => {
    if (document.body.contains(holder)) done();
  }, 60_000);
}
