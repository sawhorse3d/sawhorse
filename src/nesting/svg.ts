// A cutting diagram as an SVG file (lane 9): one stock piece, drawn 1:1 in millimetres (so
// it prints or projects to scale), with every part outlined and labelled (its tag, name and
// size), the stock's overall dimensions, every cut numbered where it starts, and the cut
// sequence written underneath. Our own writer: plain SVG 1.1, no scripts, no fonts beyond
// the generic families.
import type { Nesting, StockPiece } from "../kernel/types";
import { diagram, fitSize } from "./diagram";
import { cutText, dims, lenU, pieceTitle, pieceYield, settingsText, sizeUnit, type Units } from "./format";

export const SVG_TYPE = { description: "SVG drawing", mime: "image/svg+xml", extensions: [".svg"] };

const MM = 1000;

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** A number for SVG: at most three decimals, no "-0". */
export function n3(x: number): string {
  const r = Math.round(x * 1000) / 1000;
  return Object.is(r, -0) || r === 0 ? "0" : String(r);
}

export interface PieceSvgOptions {
  /** The model's name, for the title. */
  title: string;
  rounding: Units;
}

/** One stock piece as a standalone SVG document. */
export function pieceSvg(n: Nesting, piece: StockPiece, o: PieceSvgOptions): string {
  const r = o.rounding;
  const d = diagram(piece);
  const W = d.width * MM;
  const H = d.height * MM;
  // Text scales with the stock so a 4 x 8 sheet and a 2 ft board both read when printed.
  const font = Math.min(60, Math.max(6, Math.min(W, H) / 22));
  const margin = font * 4;
  const ox = margin;
  const oy = margin + font * 2.4;
  const lines = [pieceTitle(piece), `${pieceYield(piece, r)} · ${piece.placements.length} part${piece.placements.length === 1 ? "" : "s"} · ${settingsText(n, r)}`];
  const seq = piece.cuts.map((c) => `${c.n}. ${cutText(c, r)}`);
  if (!piece.guillotine) seq.push("Not every part comes free in straight cuts from edge to edge: finish with a track saw, a jigsaw or a CNC.");
  const listTop = oy + H + font * 4;
  const TW = W + margin * 2;
  const TH = listTop + (seq.length + 1) * font * 1.35 + font;
  const out: string[] = [];
  out.push(`<?xml version="1.0" encoding="UTF-8"?>`);
  out.push(
    `<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="${n3(TW)}mm" height="${n3(TH)}mm" viewBox="0 0 ${n3(TW)} ${n3(TH)}" font-family="Helvetica, Arial, sans-serif" data-stock="${esc(piece.stock)}" data-piece="${piece.index}" data-units="mm">`,
  );
  out.push(`<title>${esc(`${o.title}: ${pieceTitle(piece)}`)}</title>`);
  out.push(`<desc>Cutting diagram by Sawhorse, 1:1 in millimetres. ${esc(settingsText(n, r))}.</desc>`);
  out.push(`<rect x="0" y="0" width="${n3(TW)}" height="${n3(TH)}" fill="#ffffff"/>`);
  out.push(`<text x="${n3(ox)}" y="${n3(font * 1.3)}" font-size="${n3(font * 1.1)}" font-weight="bold" fill="#111">${esc(`${o.title}: ${lines[0]}`)}</text>`);
  out.push(`<text x="${n3(ox)}" y="${n3(font * 2.5)}" font-size="${n3(font * 0.8)}" fill="#444">${esc(lines[1])}</text>`);
  // Overall dimensions: the length above the stock, the width to its left.
  const dimY = oy - font * 1.2;
  out.push(`<g id="dimensions" stroke="#444" stroke-width="${n3(font / 14)}" fill="#444" font-size="${n3(font * 0.8)}">`);
  out.push(`<line x1="${n3(ox)}" y1="${n3(dimY)}" x2="${n3(ox + W)}" y2="${n3(dimY)}"/>`);
  for (const x of [ox, ox + W]) out.push(`<line x1="${n3(x)}" y1="${n3(dimY - font * 0.4)}" x2="${n3(x)}" y2="${n3(dimY + font * 0.4)}"/>`);
  out.push(`<text x="${n3(ox + W / 2)}" y="${n3(dimY - font * 0.35)}" text-anchor="middle" stroke="none" data-dim="length">${esc(lenU(d.width, r))}</text>`);
  const dimX = ox - font * 1.2;
  out.push(`<line x1="${n3(dimX)}" y1="${n3(oy)}" x2="${n3(dimX)}" y2="${n3(oy + H)}"/>`);
  for (const y of [oy, oy + H]) out.push(`<line x1="${n3(dimX - font * 0.4)}" y1="${n3(y)}" x2="${n3(dimX + font * 0.4)}" y2="${n3(y)}"/>`);
  out.push(
    `<text x="${n3(dimX - font * 0.35)}" y="${n3(oy + H / 2)}" text-anchor="middle" stroke="none" transform="rotate(-90 ${n3(dimX - font * 0.35)} ${n3(oy + H / 2)})" data-dim="width">${esc(lenU(d.height, r))}</text>`,
  );
  out.push(`</g>`);
  // The stock, its usable area inside the trim, and its grain.
  out.push(`<g id="stock">`);
  out.push(`<rect x="${n3(ox)}" y="${n3(oy)}" width="${n3(W)}" height="${n3(H)}" fill="#f3ead8" stroke="#222" stroke-width="${n3(font / 10)}"/>`);
  if (d.usable) {
    const u = d.usable;
    out.push(
      `<rect x="${n3(ox + u.x * MM)}" y="${n3(oy + u.y * MM)}" width="${n3(u.w * MM)}" height="${n3(u.h * MM)}" fill="none" stroke="#999" stroke-dasharray="${n3(font / 2)} ${n3(font / 3)}" stroke-width="${n3(font / 16)}"/>`,
    );
  }
  out.push(`</g>`);
  // Parts.
  out.push(`<g id="parts">`);
  for (const q of d.parts) {
    const [x, y, w, h] = [ox + q.x * MM, oy + q.y * MM, q.w * MM, q.h * MM];
    const p = q.p;
    const size = dims(p.dy, p.dx, r);
    out.push(
      `<g class="part" data-tag="${esc(p.tag)}" data-name="${esc(p.name)}" data-x="${n3(q.x * MM)}" data-y="${n3(q.y * MM)}" data-w="${n3(q.w * MM)}" data-h="${n3(q.h * MM)}"${p.turned ? ` data-turned="1"` : ""}>`,
    );
    out.push(`<title>${esc(`${p.tag} ${p.name}: ${size} ${sizeUnit(r)}${p.turned ? ", turned" : ""}`)}</title>`);
    out.push(`<rect x="${n3(x)}" y="${n3(y)}" width="${n3(w)}" height="${n3(h)}" fill="#ffffff" stroke="#1d4f8f" stroke-width="${n3(font / 12)}"/>`);
    const head = `${p.tag} ${p.name}`;
    const big = fitSize(w, h / 2.2, head.length, font);
    if (big >= font * 0.35) {
      out.push(`<text x="${n3(x + w / 2)}" y="${n3(y + h / 2 - big * 0.15)}" font-size="${n3(big)}" text-anchor="middle" fill="#111">${esc(head)}</text>`);
      out.push(`<text x="${n3(x + w / 2)}" y="${n3(y + h / 2 + big * 1.05)}" font-size="${n3(big * 0.8)}" text-anchor="middle" fill="#444">${esc(size)}</text>`);
    } else {
      const small = fitSize(w, h, p.tag.length, font);
      out.push(`<text x="${n3(x + w / 2)}" y="${n3(y + h / 2 + small * 0.35)}" font-size="${n3(Math.max(small, font * 0.2))}" text-anchor="middle" fill="#111">${esc(p.tag)}</text>`);
    }
    out.push(`</g>`);
  }
  out.push(`</g>`);
  // Cuts: the kerf's centre line, numbered where the cut starts.
  out.push(`<g id="cuts" stroke="#c0392b" stroke-width="${n3(font / 12)}">`);
  for (const c of d.cuts) {
    const [x1, y1, x2, y2] = [ox + c.x1 * MM, oy + c.y1 * MM, ox + c.x2 * MM, oy + c.y2 * MM];
    out.push(`<line class="cut" data-n="${c.c.n}" x1="${n3(x1)}" y1="${n3(y1)}" x2="${n3(x2)}" y2="${n3(y2)}" stroke-dasharray="${n3(font / 2)} ${n3(font / 4)}"/>`);
    const rad = font * 0.55;
    out.push(`<circle cx="${n3(x1)}" cy="${n3(y1)}" r="${n3(rad)}" fill="#c0392b" stroke="none"/>`);
    out.push(`<text x="${n3(x1)}" y="${n3(y1 + rad * 0.4)}" font-size="${n3(rad * 1.1)}" text-anchor="middle" fill="#ffffff" stroke="none">${c.c.n}</text>`);
  }
  out.push(`</g>`);
  // The cut sequence.
  out.push(`<g id="sequence" font-size="${n3(font * 0.8)}" fill="#222">`);
  out.push(`<text x="${n3(ox)}" y="${n3(listTop)}" font-weight="bold">Cut sequence</text>`);
  seq.forEach((s, i) => out.push(`<text x="${n3(ox)}" y="${n3(listTop + (i + 1) * font * 1.35)}">${esc(s)}</text>`));
  out.push(`</g>`);
  out.push(`</svg>`);
  return out.join("\n") + "\n";
}

/** The file name of a piece's SVG: "Bookshelf 01 sheet 1 of 2, 4 x 8 sheet, 3-4.svg". */
export function pieceFileName(title: string, piece: StockPiece): string {
  const stock = piece.stock.replace(/[\\/:*?"<>|]/g, "-");
  return `${title} ${String(piece.index).padStart(2, "0")} ${piece.kind} ${piece.seq} of ${piece.count}, ${stock}.svg`;
}
