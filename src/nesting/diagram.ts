// The cutting diagram of one stock piece as plain geometry (lane 9), drawn the same way in
// the pane (React SVG), the SVG file and the PDF booklet. Stock coordinates, metres: x
// along the stock's length (its grain), y across it from the top edge, so the drawing reads
// like the sheet lying on the bench, length left to right.
import type { NestCut, Placement, StockPiece } from "../kernel/types";

export interface DiagramPart {
  p: Placement;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface DiagramCut {
  c: NestCut;
  /** The centre line of the kerf. */
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

export interface Diagram {
  width: number;
  height: number;
  /** The usable area inside the trim (when there is a trim). */
  usable: { x: number; y: number; w: number; h: number } | null;
  parts: DiagramPart[];
  cuts: DiagramCut[];
}

export function diagram(piece: StockPiece): Diagram {
  const [x0, y0, x1, y1] = piece.usable;
  const k = piece.kerf / 2;
  return {
    width: piece.length,
    height: piece.width,
    usable: piece.trim > 0 ? { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } : null,
    parts: piece.placements.map((p) => ({ p, x: p.x, y: p.y, w: p.dx, h: p.dy })),
    cuts: piece.cuts.map((c) => (c.dir === "rip" ? { c, x1: c.from, y1: c.at + k, x2: c.to, y2: c.at + k } : { c, x1: c.at + k, y1: c.from, x2: c.at + k, y2: c.to })),
  };
}

/** Whether a label of `chars` characters at `size` (same units as the box) fits in a box. */
export function labelFits(w: number, h: number, chars: number, size: number): boolean {
  return chars * size * 0.6 <= w * 0.92 && size * 1.2 <= h;
}

/** The largest font size (in box units) up to `max` at which `chars` characters fit in a box. */
export function fitSize(w: number, h: number, chars: number, max: number): number {
  return Math.max(0, Math.min(max, (w * 0.92) / (Math.max(chars, 1) * 0.6), h / 1.2));
}
