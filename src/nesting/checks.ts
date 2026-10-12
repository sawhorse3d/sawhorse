// Read every nesting export back and check every part is in it, where the kernel put it
// (lane 9): the SVG through the DOM's XML parser, the DXF through the independent
// `dxf-parser` library (both flavours), the PDF through our PDF reader. Shared by the
// fixture tests (`exports.test.ts`) and the WASM round trip (`nesting.test.ts`).
import { expect } from "vitest";
import DxfParser from "dxf-parser";
import type { Nesting } from "../kernel/types";
import { readPdf } from "../cutlist/pdf";
import { nestingPdf } from "./booklet";
import { nestingDxf, pieceLayer, pieceOrigins } from "./dxf";
import { cutText, lenU, pieceTitle } from "./format";
import { pieceSvg } from "./svg";

/** Every exporter's view of every part, read back; shared with the WASM round trip. */
export function checkSvg(n: Nesting, title: string) {
  for (const piece of n.pieces) {
    const text = pieceSvg(n, piece, { title, rounding: "sixteenth" });
    const doc = new DOMParser().parseFromString(text, "image/svg+xml");
    expect(doc.getElementsByTagName("parsererror").length, text.slice(0, 200)).toBe(0);
    const svg = doc.documentElement;
    expect(svg.getAttribute("width")).toMatch(/mm$/);
    expect(svg.getAttribute("data-piece")).toBe(String(piece.index));
    const parts = [...doc.querySelectorAll("g.part")];
    expect(parts.map((g) => g.getAttribute("data-tag")).sort()).toEqual(piece.placements.map((q) => q.tag).sort());
    for (const q of piece.placements) {
      const g = parts.find((x) => x.getAttribute("data-tag") === q.tag)!;
      expect(Number(g.getAttribute("data-x"))).toBeCloseTo(q.x * 1000, 2);
      expect(Number(g.getAttribute("data-y"))).toBeCloseTo(q.y * 1000, 2);
      expect(Number(g.getAttribute("data-w"))).toBeCloseTo(q.dx * 1000, 2);
      expect(Number(g.getAttribute("data-h"))).toBeCloseTo(q.dy * 1000, 2);
      expect(g.getAttribute("data-name")).toBe(q.name);
      const rect = g.querySelector("rect")!;
      expect(Number(rect.getAttribute("width"))).toBeCloseTo(q.dx * 1000, 2);
    }
    expect(doc.querySelectorAll("line.cut").length).toBe(piece.cuts.length);
    expect(doc.querySelector('[data-dim="length"]')!.textContent).toBe(lenU(piece.length, "sixteenth"));
    const seq = [...doc.querySelectorAll("#sequence text")].map((t) => t.textContent);
    expect(seq.slice(1, 1 + piece.cuts.length)).toEqual(piece.cuts.map((c) => `${c.n}. ${cutText(c, "sixteenth")}`));
  }
}

export function checkDxf(n: Nesting) {
  for (const flavour of ["lwpolyline", "r12"] as const) {
    const text = nestingDxf(n, flavour);
    expect(text.endsWith("  0\nEOF\n")).toBe(true);
    if (flavour === "r12") expect(text).toContain("$ACADVER\n  1\nAC1009");
    const dxf = new DxfParser().parseSync(text) as unknown as {
      entities: { type: string; layer: string; vertices?: { x: number; y: number }[]; text?: string }[];
      tables: { layer: { layers: Record<string, unknown> } };
    };
    const layers = Object.keys(dxf.tables.layer.layers);
    expect(layers).toEqual(expect.arrayContaining(["STOCK", "LABELS", ...n.pieces.map(pieceLayer)]));
    const kind = flavour === "r12" ? "POLYLINE" : "LWPOLYLINE";
    const origins = pieceOrigins(n);
    n.pieces.forEach((piece, i) => {
      const polys = dxf.entities.filter((e) => e.type === kind && e.layer === pieceLayer(piece));
      expect(polys.length, pieceLayer(piece)).toBe(piece.placements.length);
      const [ox, oy] = origins[i];
      const W = piece.width * 1000;
      for (const q of piece.placements) {
        const x0 = ox + q.x * 1000;
        const y0 = oy + W - (q.y + q.dy) * 1000;
        const hit = polys.find((p) => {
          const v = p.vertices ?? [];
          return v.length === 4 && Math.abs(v[0].x - x0) < 1e-3 && Math.abs(v[0].y - y0) < 1e-3 && Math.abs(v[2].x - (x0 + q.dx * 1000)) < 1e-3 && Math.abs(v[2].y - (y0 + q.dy * 1000)) < 1e-3;
        });
        expect(hit, `${q.tag} on ${pieceLayer(piece)}`).toBeTruthy();
      }
      // The stock outline on STOCK.
      const outline = dxf.entities.filter((e) => e.type === kind && e.layer === "STOCK");
      expect(outline.some((p) => Math.abs((p.vertices ?? [])[2]?.x - (ox + piece.length * 1000)) < 1e-3 && Math.abs((p.vertices ?? [])[2]?.y - (oy + W)) < 1e-3)).toBe(true);
    });
    const labels = dxf.entities.filter((e) => e.type === "TEXT" && e.layer === "LABELS").map((e) => e.text);
    for (const piece of n.pieces) for (const q of piece.placements) expect(labels).toContain(q.tag);
    // Pieces do not overlap in the file.
    for (let i = 1; i < origins.length; i++) expect(origins[i - 1][1]).toBeGreaterThan(origins[i][1] + n.pieces[i].width * 1000);
  }
}

export function checkPdf(n: Nesting, title: string) {
  const bytes = nestingPdf(n, title, "sixteenth");
  const pdf = readPdf(bytes);
  expect(pdf.pages).toBeGreaterThanOrEqual(n.pieces.length + 1);
  const first = pdf.text[0].join("\n");
  expect(first).toContain(`Nesting: ${title}`);
  expect(first).toContain("What to buy");
  for (const g of n.totals.groups) expect(first).toContain(g.stock);
  for (const u of n.unplaced) expect(first).toContain(u.tag);
  const all = pdf.text.slice(1).map((t) => t.join("\n"));
  for (const piece of n.pieces) {
    const page = all.find((t) => t.includes(`${title}: ${pieceTitle(piece)}`));
    expect(page, pieceTitle(piece)).toBeTruthy();
    const pieceText = all.filter((t) => t.includes(`${title}: ${pieceTitle(piece)}`) || t.includes("(continued)")).join("\n");
    for (const q of piece.placements) expect(pieceText).toContain(q.tag);
    for (const c of piece.cuts) expect(pieceText).toContain(`${c.n}. ${cutText(c, "sixteenth")}`.slice(0, 20));
  }
  // Deterministic: the same nesting writes the same bytes.
  expect(nestingPdf(n, title, "sixteenth")).toEqual(bytes);
}
