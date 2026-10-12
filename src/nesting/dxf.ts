// The nesting as a DXF for CNC routers and lasers (lane 9), ASCII, millimetres: one closed
// polyline per part on a layer per stock piece (SHEET-01, BOARD-02, ...), each stock's
// outline (and its trim line) on layer STOCK, and each part's tag as TEXT on layer LABELS,
// so a CAM program can cut one layer, or turn the outlines and labels off. Pieces are laid
// out one above the other, 100 mm apart, in the order of the booklet; y runs up, so each
// piece reads as in the diagrams (length left to right, the stock's first edge on top).
//
// Two flavours, like the face outline export (crates/export dxf.rs): LWPOLYLINE, which
// every laser and CAM program reads; or R12 (`$ACADVER` AC1009, POLYLINE / VERTEX / SEQEND)
// for strict R12-only readers.
import type { Nesting, StockPiece } from "../kernel/types";

export const DXF_TYPE = { description: "DXF", mime: "image/vnd.dxf", extensions: [".dxf"] };

export type DxfFlavour = "lwpolyline" | "r12";

const MM = 1000;
/** Space between pieces, mm. */
export const PIECE_GAP = 100;

/** A number as DXF wants it: plain decimal, at most six places, no "-0". */
export function dxfNum(x: number): string {
  const s = (Math.round(x * 1e6) / 1e6).toFixed(6).replace(/0+$/, "").replace(/\.$/, "");
  return s === "-0" ? "0" : s;
}

/** The layer of a stock piece: "SHEET-01", "BOARD-12". */
export function pieceLayer(p: StockPiece): string {
  return `${p.kind.toUpperCase()}-${String(p.index).padStart(2, "0")}`;
}

/** Each piece's lower-left corner in the file (mm): stacked upward, the first on top. */
export function pieceOrigins(n: Nesting): [number, number][] {
  const out: [number, number][] = [];
  let y = 0;
  for (let i = n.pieces.length - 1; i >= 0; i--) {
    out[i] = [0, y];
    y += n.pieces[i].width * MM + PIECE_GAP;
  }
  return out;
}

class Dxf {
  readonly lines: string[] = [];
  pair(code: number, value: string | number) {
    this.lines.push(String(code).padStart(3, " "), String(value));
  }
}

/** The whole nesting as one DXF. */
export function nestingDxf(n: Nesting, flavour: DxfFlavour = "lwpolyline"): string {
  const r12 = flavour === "r12";
  const d = new Dxf();
  const layers = ["STOCK", "LABELS", ...n.pieces.map(pieceLayer)];
  d.pair(0, "SECTION");
  d.pair(2, "HEADER");
  if (r12) {
    d.pair(9, "$ACADVER");
    d.pair(1, "AC1009");
  }
  d.pair(9, "$INSUNITS");
  d.pair(70, 4);
  d.pair(0, "ENDSEC");
  d.pair(0, "SECTION");
  d.pair(2, "TABLES");
  d.pair(0, "TABLE");
  d.pair(2, "LTYPE");
  d.pair(70, 1);
  d.pair(0, "LTYPE");
  d.pair(2, "CONTINUOUS");
  d.pair(70, 0);
  d.pair(3, "Solid line");
  d.pair(72, 65);
  d.pair(73, 0);
  d.pair(40, "0.0");
  d.pair(0, "ENDTAB");
  d.pair(0, "TABLE");
  d.pair(2, "LAYER");
  d.pair(70, layers.length + 1);
  // Colours: stock grey (8), labels yellow (2), parts by piece through the ACI wheel.
  ["0", ...layers].forEach((name, i) => {
    d.pair(0, "LAYER");
    d.pair(2, name);
    d.pair(70, 0);
    d.pair(62, name === "0" ? 7 : name === "STOCK" ? 8 : name === "LABELS" ? 2 : 1 + ((i - 3) % 6));
    d.pair(6, "CONTINUOUS");
  });
  d.pair(0, "ENDTAB");
  d.pair(0, "ENDSEC");
  d.pair(0, "SECTION");
  d.pair(2, "ENTITIES");
  const poly = (layer: string, pts: [number, number][]) => {
    if (r12) {
      d.pair(0, "POLYLINE");
      d.pair(8, layer);
      d.pair(66, 1);
      d.pair(10, "0");
      d.pair(20, "0");
      d.pair(30, "0");
      d.pair(70, 1);
      for (const [x, y] of pts) {
        d.pair(0, "VERTEX");
        d.pair(8, layer);
        d.pair(10, dxfNum(x));
        d.pair(20, dxfNum(y));
        d.pair(30, "0");
      }
      d.pair(0, "SEQEND");
      d.pair(8, layer);
    } else {
      d.pair(0, "LWPOLYLINE");
      d.pair(100, "AcDbEntity");
      d.pair(8, layer);
      d.pair(100, "AcDbPolyline");
      d.pair(90, pts.length);
      d.pair(70, 1);
      for (const [x, y] of pts) {
        d.pair(10, dxfNum(x));
        d.pair(20, dxfNum(y));
      }
    }
  };
  const text = (layer: string, x: number, y: number, h: number, s: string) => {
    d.pair(0, "TEXT");
    d.pair(8, layer);
    d.pair(10, dxfNum(x));
    d.pair(20, dxfNum(y));
    d.pair(30, "0");
    d.pair(40, dxfNum(h));
    // Plain ASCII: R12 readers vary in what else they take.
    d.pair(1, s.replace(/[^\x20-\x7e]/g, "?"));
    d.pair(72, 1);
    d.pair(11, dxfNum(x));
    d.pair(21, dxfNum(y));
    d.pair(31, "0");
  };
  const rect = (x: number, y: number, w: number, h: number): [number, number][] => [
    [x, y],
    [x + w, y],
    [x + w, y + h],
    [x, y + h],
  ];
  const origins = pieceOrigins(n);
  n.pieces.forEach((p, i) => {
    const [ox, oy] = origins[i];
    const W = p.width * MM;
    // Stock y (from the top edge) to file y (up): y' = oy + W - y.
    const fy = (y: number) => oy + W - y * MM;
    poly("STOCK", rect(ox, oy, p.length * MM, W));
    if (p.trim > 0) {
      const [x0, y0, x1, y1] = p.usable;
      poly("STOCK", rect(ox + x0 * MM, fy(y1), (x1 - x0) * MM, (y1 - y0) * MM));
    }
    text("LABELS", ox, oy + W + Math.min(40, W / 12), Math.min(30, W / 16), `${p.index}. ${p.stock}${p.material ? ` - ${p.material}` : ""} (${p.seq} of ${p.count})`);
    const layer = pieceLayer(p);
    for (const q of p.placements) {
      poly(layer, rect(ox + q.x * MM, fy(q.y + q.dy), q.dx * MM, q.dy * MM));
      const h = Math.max(1, Math.min(25, (q.dy * MM) / 4, (q.dx * MM) / (q.tag.length * 1.2 + 1)));
      text("LABELS", ox + (q.x + q.dx / 2) * MM, fy(q.y + q.dy / 2) - h / 2, h, q.tag);
    }
  });
  d.pair(0, "ENDSEC");
  d.pair(0, "EOF");
  return d.lines.join("\n") + "\n";
}
