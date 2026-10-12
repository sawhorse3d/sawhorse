import { describe, expect, it } from "vitest";
import { IN, MM, canBeToScale, captionFor, fittingScale, newSetup, paperSize, paperText, parsePaper, scaleFactor, scaleLabel, STANDARD_SCALES } from "./paper";
import { circumcentre, dimOffset, dimPreview, hitItem, modelToPaper, sheetLayers, sheetSvg, snapPoint, toDrawing, toPaper, type SheetRender, type ViewportInfo } from "./render";
import { nextSheetName } from "./SheetsWorkspace";
import { dimPaper } from "./SheetCanvas";

const front: ViewportInfo = {
  id: 1,
  rect: [0.02, 0.02, 0.1, 0.08],
  frame: { ortho: true, right: [1, 0, 0], up: [0, 0, 1], forward: [0, 1, 0], eye: [0, 0, 0], scale: 1 },
  factor: 1 / 8,
  centre: [0.381, 0.6],
  to_scale: true,
};

const render: SheetRender = {
  id: 1,
  name: "Sheet 1",
  size: [11 * IN, 8.5 * IN],
  prims: [
    { p: "rect", rect: [0.0127, 0.0127, 0.254, 0.1905], width: 0.00035, stroke: [24, 26, 25] },
    { p: "line", a: [0.03, 0.05], b: [0.09, 0.05], width: 0.00025, color: [24, 26, 25] },
    { p: "text", at: [0.05, 0.04], text: 'A & B <30">', size: 0.0024, anchor: "middle", color: [24, 26, 25] },
    { p: "fill", rings: [[[0, 0], [0.01, 0], [0.01, 0.01]]], color: [167, 158, 144] },
    { p: "text", at: [0.2, 0.19], text: "Bookshelf", size: 0.0046, anchor: "start", bold: true, max: 0.09, color: [24, 26, 25] },
  ],
  items: [
    { id: 1, kind: "viewport", bounds: [0.02, 0.02, 0.1, 0.08], prims: [1, 2] },
    { id: 2, kind: "dimension", bounds: [0.03, 0.035, 0.06, 0.016], text: '30"', prims: [2, 4] },
  ],
  viewports: [front],
  drawings: ["abc"],
  hash: "0",
};

describe("paper", () => {
  it("lays paper out portrait or landscape", () => {
    const s = newSetup("Sheet 1", false);
    expect(paperSize(s)).toEqual([11 * IN, 8.5 * IN]);
    expect(paperSize({ ...s, landscape: false })).toEqual([8.5 * IN, 11 * IN]);
    const a4 = newSetup("A", true);
    expect(paperSize(a4)[0]).toBeCloseTo(297 * MM, 12);
    expect(a4.units).toBe("millimetres");
  });

  it("writes scales as the kernel does and picks the one that fits", () => {
    expect(scaleLabel({ kind: "ratio", paper: 1, model: 8 })).toBe("1:8");
    expect(scaleLabel({ kind: "architect", inches_per_foot: 0.25 })).toBe('1/4" = 1\'-0"');
    expect(scaleLabel({ kind: "architect", inches_per_foot: 1.5 })).toBe('1-1/2" = 1\'-0"');
    expect(scaleLabel({ kind: "architect", inches_per_foot: 3 })).toBe('3" = 1\'-0"');
    expect(scaleLabel({ kind: "nts" })).toBe("NTS");
    // 30 in at 1:8 is 3.75 in; at 1/4" = 1' it is 0.625 in.
    expect((30 * IN * scaleFactor({ kind: "ratio", paper: 1, model: 8 })!) / IN).toBeCloseTo(3.75, 12);
    expect((30 * IN * scaleFactor({ kind: "architect", inches_per_foot: 0.25 })!) / IN).toBeCloseTo(0.625, 12);
    // A 72 in bookshelf in a 6 in tall frame (92%): 1:13.04 fits, so 1:16 is the largest standard.
    const f = (0.92 * 6) / 72;
    expect(scaleLabel(fittingScale(f))).toBe("1:16");
    expect(scaleLabel(fittingScale(1 / 12))).toBe("1:12");
    expect(scaleLabel(fittingScale(5))).toBe("1:1");
    // Nothing standard is small enough: a ratio that is.
    expect(scaleFactor(fittingScale(1 / 1000))!).toBeLessThanOrEqual(1 / 1000);
    expect(STANDARD_SCALES.length).toBe(14);
  });

  it("knows which views can be to scale", () => {
    const scenes = [{ name: "Front", parallel: true }, { name: "Persp" }];
    expect(canBeToScale({ from: "standard", view: "front" }, scenes)).toBe(true);
    expect(canBeToScale({ from: "standard", view: "iso" }, scenes)).toBe(false);
    expect(canBeToScale({ from: "scene", name: "Front" }, scenes)).toBe(true);
    expect(canBeToScale({ from: "scene", name: "Persp" }, scenes)).toBe(false);
    expect(canBeToScale({ from: "camera", camera: { eye: [0, 0, 1], target: [0, 0, 0], up: [0, 1, 0], parallel: true } }, scenes)).toBe(true);
    expect(captionFor({ from: "standard", view: "top" }, { kind: "ratio", paper: 1, model: 12 })).toBe("PLAN 1:12");
    expect(captionFor({ from: "scene", name: "Section A" }, { kind: "ratio", paper: 1, model: 4 })).toBe("SECTION A 1:4");
  });

  it("reads and writes paper lengths", () => {
    expect(parsePaper("1/2", "inches")).toBeCloseTo(0.5 * IN, 12);
    expect(parsePaper('1 1/4"', "inches")).toBeCloseTo(1.25 * IN, 12);
    expect(parsePaper("12 mm", "inches")).toBeCloseTo(0.012, 12);
    expect(parsePaper("10", "millimetres")).toBeCloseTo(0.01, 12);
    expect(parsePaper("0.5in", "millimetres")).toBeCloseTo(0.0127, 12);
    expect(parsePaper("wide", "inches")).toBeNull();
    expect(paperText(0.5 * IN, "inches")).toBe('0.5"');
    expect(paperText(0.0125, "millimetres")).toBe("12.5 mm");
  });

  it("names new sheets", () => {
    expect(nextSheetName([])).toBe("Sheet 1");
    expect(nextSheetName(["Sheet 1"])).toBe("Sheet 2");
    expect(nextSheetName(["Sheet 2", "Plans"])).toBe("Sheet 3");
  });
});

describe("render geometry", () => {
  it("maps model, drawing and paper points both ways", () => {
    // The drawing centre sits at the rect centre.
    expect(toPaper(front, front.centre)).toEqual([0.07, 0.06]);
    const q = toPaper(front, [0.381 + 0.08, 0.6 + 0.04]);
    expect(q[0]).toBeCloseTo(0.07 + 0.01, 12);
    expect(q[1]).toBeCloseTo(0.06 - 0.005, 12);
    const back = toDrawing(front, q);
    expect(back[0]).toBeCloseTo(0.461, 12);
    expect(back[1]).toBeCloseTo(0.64, 12);
    // Model (x, y, z) seen from the front: right = x, up = z.
    const p = modelToPaper(front, [0.381, 5, 0.6]);
    expect(p![0]).toBeCloseTo(0.07, 12);
    expect(p![1]).toBeCloseTo(0.06, 12);
    // A perspective point behind the eye is not drawn.
    const persp: ViewportInfo = { ...front, frame: { ...front.frame, ortho: false, eye: [0, -2, 0], scale: 2 } };
    expect(modelToPaper(persp, [0, -3, 0])).toBeNull();
    expect(modelToPaper(persp, [0, 0, 0])).not.toBeNull();
  });

  it("snaps to the grid and hits the smallest item under the cursor", () => {
    const s = snapPoint([0.0031, 0.0099], IN / 8);
    expect(s[0]).toBeCloseTo(IN / 8, 12);
    expect(s[1]).toBeCloseTo((3 * IN) / 8, 12);
    expect(snapPoint([0.0031, 0.0099], 0)).toEqual([0.0031, 0.0099]);
    expect(hitItem(render.items, [0.05, 0.04])?.id).toBe(2);
    expect(hitItem(render.items, [0.025, 0.09])?.id).toBe(1);
    expect(hitItem(render.items, [0.2, 0.2])).toBeNull();
    expect(hitItem(render.items, [0.125, 0.05], 0.01)?.id).toBe(1);
  });

  it("stands a dimension off where the cursor is", () => {
    const p: [number, number][] = [
      [0.03, 0.05],
      [0.09, 0.05],
    ];
    const off = dimOffset("horizontal", p, [0.06, 0.04]);
    expect(off).toBeCloseTo(0.01, 12);
    expect(dimPreview("horizontal", p, off)).toEqual([
      [0.03, 0.04],
      [0.09, 0.04],
    ]);
    expect(dimOffset("vertical", p, [0.05, 0])).toBeCloseTo(0.02, 12);
    // Aligned: the left normal of a->b (y down) is (d.y, -d.x): up the page for a rightward line.
    expect(dimOffset("aligned", p, [0.06, 0.04])).toBeCloseTo(0.01, 12);
    const line = dimPreview("aligned", p, 0.01)!;
    expect(line[0][1]).toBeCloseTo(0.04, 12);
    expect(dimOffset("angular", [[0, 0], [1, 0], [0, 1]], [0.03, 0.04])).toBeCloseTo(0.05, 12);
    const c = circumcentre([1, 0], [0, 1], [-1, 0])!;
    expect(c[0]).toBeCloseTo(0, 12);
    expect(c[1]).toBeCloseTo(0, 12);
    expect(circumcentre([0, 0], [1, 1], [2, 2])).toBeNull();
    expect(dimOffset("radius", [[1, 0], [0, 1], [-1, 0]], [2, 0])).toBeCloseTo(1, 12);
  });

  it("finds a dimension's paper points through its viewport", () => {
    const pts = dimPaper(render, { kind: "dimension", viewport: 1, dim: "horizontal", points: [[0.381, 0, 0.6], [0.381 + 0.08, 0, 0.6]], offset: 0.01 });
    expect(pts![0][0]).toBeCloseTo(0.07, 12);
    expect(pts![1][0]).toBeCloseTo(0.08, 12);
    expect(dimPaper(render, { kind: "dimension", viewport: 9, dim: "horizontal", points: [[0, 0, 0], [1, 0, 0]], offset: 0 })).toBeNull();
  });
});

describe("SVG", () => {
  it("draws the sheet at its real size, in points, with text escaped and fitted", () => {
    const svg = sheetSvg(render, () => null, "inches");
    expect(svg).toContain('width="11in" height="8.5in"');
    expect(svg).toContain('viewBox="0 0 792 612"');
    expect(svg).toContain("A &amp; B &lt;30&quot;&gt;");
    expect(svg).toContain('font-weight="bold"');
    expect(svg).toContain('fill-rule="evenodd"');
    const mm = sheetSvg(render, () => null, "millimetres");
    expect(mm).toContain('width="279.4mm" height="215.9mm"');
    expect(mm).toContain('viewBox="0 0 279.4 215.9"');
    // Deterministic.
    expect(sheetSvg(render, () => null)).toBe(svg);
  });

  it("splits the sheet into layers by item", () => {
    const l = sheetLayers(render, () => null, 1000);
    expect(l.under).toContain("<rect");
    expect(l.items.map((i) => i.id)).toEqual([1, 2]);
    expect(l.items[0].svg).toContain("<line");
    expect(l.items[1].svg).toContain("<text");
    expect(l.items[1].svg).toContain("<path");
    expect(l.over).toContain("Bookshelf");
  });
});
