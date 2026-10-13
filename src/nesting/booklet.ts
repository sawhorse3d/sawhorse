// The nesting as a PDF booklet (lane 9), through the cut list's own PDF writer: a summary
// page (what to buy, by stock and by nominal size, the yield, the settings, anything no
// stock takes, and an index of the pieces), then one landscape page per stock piece: its
// cutting diagram to scale, the parts on it and its cut sequence.
import type { Nesting, StockPiece } from "../kernel/types";
import { fit, LETTER, PdfDoc, textWidth, type PdfPage } from "../cutlist/pdf";
import { NOT_SOLID_NOTE } from "../cutlist/format";
import { diagram } from "./diagram";
import { area, cutText, dims, len, nestNote, nestSummary, pct, pieceNumber, pieceTitle, pieceYield, reasonText, settingsText, sizeUnit, stockSize, type Units } from "./format";

const LANDSCAPE: [number, number] = [LETTER[1], LETTER[0]];

/** Text lines that wrap at `width` points (Helvetica at `size`). */
function wrap(s: string, size: number, width: number): string[] {
  const out: string[] = [];
  let line = "";
  for (const word of s.split(" ")) {
    const next = line ? `${line} ${word}` : word;
    if (textWidth(next, size) > width && line) {
      out.push(line);
      line = word;
    } else line = next;
  }
  if (line) out.push(line);
  return out;
}

function summaryPages(doc: PdfDoc, n: Nesting, title: string, r: Units) {
  const [W, H] = LETTER;
  const left = 48;
  const right = W - 48;
  const bottom = 56;
  let p: PdfPage = doc.page();
  let y = H - 56;
  const need = (h: number) => {
    if (y - h < bottom) {
      p = doc.page();
      y = H - 56;
      p.text(left, y, `Nesting: ${title} (continued)`, 11, { bold: true });
      y -= 24;
    }
  };
  const s = nestSummary(n, r);
  p.text(left, y, `Nesting: ${title}`, 18, { bold: true });
  y -= 20;
  p.text(left, y, settingsText(n, r), 10, { gray: 0.35 });
  y -= 30;
  // Three tiles.
  const tiles: [string, string][] = [
    [s.sheets, s.sheetsLabel],
    [s.boards, s.boardsLabel],
    [s.yieldText, s.yieldLabel],
  ];
  const tw = (right - left - 16) / 3;
  tiles.forEach(([big, label], i) => {
    const x = left + i * (tw + 8);
    p.rect(x, y - 44, tw, 50, { stroke: 0.75, width: 0.75 });
    p.text(x + 8, y - 14, big, 20, { bold: true });
    wrap(label, 8, tw - 16)
      .slice(0, 2)
      .forEach((l, k) => p.text(x + 8, y - 28 - k * 9, l, 8, { gray: 0.35 }));
  });
  y -= 72;
  // What to buy.
  p.text(left, y, "What to buy", 12, { bold: true });
  y -= 16;
  const cols: [string, number, boolean][] = [
    ["Stock", left, false],
    ["Material", left + 190, false],
    ["Pieces", left + 340, true],
    ["Parts", left + 390, true],
    ["Yield", left + 450, true],
    ["Bd ft", right, true],
  ];
  for (const [label, x, alignRight] of cols) p.text(x, y, label.toUpperCase(), 8, { bold: true, gray: 0.35, align: alignRight ? "right" : "left" });
  y -= 5;
  p.line(left, y, right, y, 0.75);
  y -= 13;
  for (const g of n.totals.groups) {
    need(16);
    p.text(left, y, fit(g.stock, 10, 180), 10);
    p.text(left + 190, y, fit(g.material ?? "—", 10, 110), 10);
    p.text(left + 340, y, String(g.count), 10, { align: "right" });
    p.text(left + 390, y, String(g.parts), 10, { align: "right" });
    p.text(left + 450, y, pct(g.efficiency), 10, { align: "right" });
    p.text(right, y, g.kind === "board" ? g.board_feet.toFixed(1) : "", 10, { align: "right" });
    y -= 15;
  }
  if (!n.totals.groups.length) {
    p.text(left, y, "Nothing to nest: no part fits any stock.", 10, { gray: 0.35 });
    y -= 15;
  }
  if (n.totals.by_nominal.length) {
    y -= 6;
    need(16);
    p.text(left, y, `Boards by nominal size: ${n.totals.by_nominal.map((b) => `${b.count} × ${b.nominal} (${b.board_feet.toFixed(1)} bd ft)`).join(", ")}`, 10);
    y -= 15;
  }
  need(16);
  p.text(left, y, `Parts: ${n.totals.placed} of ${n.totals.parts_in} placed; ${area(n.totals.parts_area, r)} of ${area(n.totals.stock_area, r)} used.`, 10);
  y -= 26;
  if (n.unplaced.length) {
    need(40);
    p.text(left, y, "Not on any stock", 12, { bold: true });
    y -= 16;
    for (const u of n.unplaced) {
      const lines = wrap(`${u.tag} ${u.name} (${len(u.size[0], r)} × ${len(u.size[1], r)} × ${len(u.size[2], r)} ${sizeUnit(r)}): ${reasonText(u)}`, 10, right - left);
      need(lines.length * 13 + 4);
      lines.forEach((l) => {
        p.text(left, y, l, 10);
        y -= 13;
      });
      y -= 2;
    }
    y -= 10;
  }
  need(40);
  p.text(left, y, "Stock pieces", 12, { bold: true });
  y -= 16;
  for (const piece of n.pieces) {
    need(14);
    p.text(left, y, fit(`${piece.index}. ${pieceTitle(piece)}`, 10, 330), 10);
    p.text(left + 340, y, `${piece.placements.length} part${piece.placements.length === 1 ? "" : "s"}`, 10);
    p.text(right, y, pieceYield(piece, r), 10, { align: "right" });
    y -= 14;
  }
  y -= 12;
  need(30);
  wrap(nestNote(n, r), 8.5, right - left).forEach((l) => {
    p.text(left, y, l, 8.5, { gray: 0.35 });
    y -= 11;
  });
}

function piecePages(doc: PdfDoc, n: Nesting, piece: StockPiece, title: string, r: Units) {
  const [W, H] = LANDSCAPE;
  const left = 40;
  const right = W - 40;
  const p = doc.page(LANDSCAPE);
  let y = H - 44;
  p.text(left, y, `${title}: ${pieceTitle(piece)}`, 14, { bold: true });
  p.text(right, y, `${piece.index} / ${n.pieces.length}`, 9, { align: "right", gray: 0.35 });
  y -= 16;
  p.text(left, y, `${stockSize(piece, r)} · grain ${piece.grain === "none" ? "none" : "along the length"} · ${pieceYield(piece, r)}${piece.guillotine ? "" : " · not every part comes free in straight cuts"}`, 9, { gray: 0.35 });
  y -= 18;
  // The diagram, to scale, as wide as the page allows (at most 250 pt tall).
  const d = diagram(piece);
  const scale = Math.min((right - left) / d.width, 250 / d.height);
  const top = y;
  const X = (x: number) => left + x * scale;
  const Y = (yy: number) => top - yy * scale;
  p.rect(X(0), Y(d.height), d.width * scale, d.height * scale, { fill: 0.9, stroke: 0, width: 0.8 });
  if (d.usable) p.rect(X(d.usable.x), Y(d.usable.y + d.usable.h), d.usable.w * scale, d.usable.h * scale, { stroke: 0.55, width: 0.4 });
  for (const q of d.parts) {
    const [x, yy, w, h] = [X(q.x), Y(q.y + q.h), q.w * scale, q.h * scale];
    p.rect(x, yy, w, h, { fill: 1, stroke: 0.15, width: 0.6 });
    const head = `${q.p.tag} ${q.p.name}`;
    const size = dims(q.p.dy, q.p.dx, r);
    if (h >= 20 && textWidth(head, 8, true) <= w - 4) {
      p.text(x + w / 2, yy + h / 2 + 1, head, 8, { bold: true, align: "center" });
      if (textWidth(size, 7) <= w - 4) p.text(x + w / 2, yy + h / 2 - 8, size, 7, { align: "center", gray: 0.3 });
    } else {
      const fs = Math.max(4, Math.min(8, h - 2, (w - 2) / (q.p.tag.length * 0.6)));
      p.text(x + w / 2, yy + h / 2 - fs / 3, q.p.tag, fs, { bold: true, align: "center" });
    }
  }
  for (const c of d.cuts) {
    p.line(X(c.x1), Y(c.y1), X(c.x2), Y(c.y2), 0.5, 0.45);
    p.text(X(c.x1) + 1.5, Y(c.y1) - 6.5, String(c.c.n), 5.5, { bold: true, gray: 0.25 });
  }
  y = top - d.height * scale - 22;
  // Two columns: parts, and the cut sequence.
  const colW = (right - left - 24) / 2;
  const startY = y;
  const parts = piece.placements.map((q) => `${q.tag}  ${q.name}  ${dims(q.size[1], q.size[2], r)} ${sizeUnit(r)}${q.turned ? " (turned)" : ""}${q.not_solid ? ` · ${NOT_SOLID_NOTE}` : ""}`);
  const cuts = piece.cuts.map((c) => `${c.n}. ${cutText(c, r)}`);
  const lineH = 11.5;
  const bottom = 40;
  const first = p;
  // Each column starts under the diagram; one that runs off the page goes on over
  // continuation pages of its own.
  const write = (lines: string[], heading: string, x0: number) => {
    let page = first;
    let x = x0;
    let yy = startY;
    page.text(x, yy, heading, 10, { bold: true });
    yy -= 14;
    for (const l of lines) {
      if (yy < bottom) {
        page = doc.page(LANDSCAPE);
        page.text(left, H - 44, `${title}: ${pieceNumber(piece)} (continued)`, 11, { bold: true });
        x = left;
        yy = H - 66;
        page.text(x, yy, `${heading} (continued)`, 10, { bold: true });
        yy -= 14;
      }
      page.text(x, yy, fit(l, 9, x === left && x0 !== left ? right - left : colW), 9);
      yy -= lineH;
    }
  };
  write(parts, `Parts (${parts.length})`, left);
  write(cuts, `Cut sequence (${cuts.length})`, left + colW + 24);
}

/** The booklet. `title`: the model's name. */
export function nestingPdf(n: Nesting, title: string, r: Units): Uint8Array {
  const doc = new PdfDoc({ title: `Nesting: ${title}`, creator: "Sawhorse" });
  summaryPages(doc, n, title, r);
  for (const piece of n.pieces) piecePages(doc, n, piece, title, r);
  return doc.bytes();
}
