import { describe, expect, it } from "vitest";
import { IN, MM, canBeToScale, captionFor, fittingScale, localDate, newSetup, paperSize, paperText, parsePaper, scaleFactor, scaleLabel, STANDARD_SCALES } from "./paper";
import { circumcentre, dimOffset, dimPreview, hitItem, modelToPaper, sheetLayers, sheetSvg, snapPoint, toDrawing, toPaper, type SheetRender, type ViewportInfo } from "./render";
import { nextSheetName } from "./SheetsWorkspace";
import { PT, crc32, pngDpi, sheetsPdf, withDpi } from "./exports";
import { readPdf, textWidth, winAnsi } from "../cutlist/pdf";
import { dimPaper } from "./SheetCanvas";
import type { V3 } from "../kernel/types";

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

  it("dates the title block by the local calendar, not UTC", () => {
    // 17:30 on 12 October in the person's time zone, whatever UTC says.
    expect(localDate(new Date(2026, 9, 12, 17, 30))).toBe("2026-10-12");
    expect(localDate(new Date(2026, 0, 3, 23, 59))).toBe("2026-01-03");
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

  it("snaps to the grid and hits the item drawn under the cursor", () => {
    const s = snapPoint([0.0031, 0.0099], IN / 8);
    expect(s[0]).toBeCloseTo(IN / 8, 12);
    expect(s[1]).toBeCloseTo((3 * IN) / 8, 12);
    expect(snapPoint([0.0031, 0.0099], 0)).toEqual([0.0031, 0.0099]);
    expect(hitItem(render, [0.05, 0.04])?.id).toBe(2);
    expect(hitItem(render, [0.025, 0.09])?.id).toBe(1);
    expect(hitItem(render, [0.2, 0.2])).toBeNull();
    expect(hitItem(render, [0.125, 0.05], 0.01)?.id).toBe(1);
    // Lane 12c: inside the dimension's bounding box but off what it draws (its text), the
    // click goes through to the viewport underneath.
    expect(hitItem(render, [0.035, 0.048])?.id).toBe(1);
    expect(hitItem(render, [0.08, 0.05])?.id).toBe(1);
  });

  it("picks drawn items by what they draw, viewports by their frame, the topmost first (lane 12c)", () => {
    const ink: [number, number, number] = [24, 26, 25];
    const r: Pick<SheetRender, "items" | "prims"> = {
      prims: [
        { p: "rect", rect: [0.02, 0.02, 0.2, 0.15], width: 0.00035, stroke: ink },
        // A dimension across the viewport: its line, two arrowheads and its text.
        { p: "line", a: [0.04, 0.1], b: [0.18, 0.1], width: 0.00025, color: ink },
        { p: "fill", rings: [[[0.04, 0.1], [0.044, 0.099], [0.044, 0.101]]], color: ink },
        { p: "fill", rings: [[[0.18, 0.1], [0.176, 0.099], [0.176, 0.101]]], color: ink },
        { p: "text", at: [0.11, 0.098], text: '30"', size: 0.0024, anchor: "middle", color: ink },
        // A vertical label, turned 90 degrees.
        { p: "text", at: [0.2, 0.15], text: "SHELF", size: 0.003, anchor: "start", angle: 90, color: ink },
        // An outline rectangle item on top of everything.
        { p: "rect", rect: [0.05, 0.03, 0.04, 0.03], width: 0.0003, stroke: ink },
      ],
      items: [
        { id: 1, kind: "viewport", bounds: [0.02, 0.02, 0.2, 0.15], prims: [0, 1] },
        { id: 2, kind: "dimension", bounds: [0.04, 0.09, 0.14, 0.02], text: '30"', prims: [1, 5] },
        { id: 3, kind: "label", bounds: [0.195, 0.12, 0.01, 0.04], text: "SHELF", prims: [5, 6] },
        { id: 4, kind: "rect", bounds: [0.05, 0.03, 0.04, 0.03], prims: [6, 7] },
      ],
    };
    const slack = 0.0005;
    // On the dimension line, on an arrowhead, on its text: the dimension.
    expect(hitItem(r, [0.07, 0.1002], slack)?.id).toBe(2);
    expect(hitItem(r, [0.0425, 0.1], slack)?.id).toBe(2);
    expect(hitItem(r, [0.11, 0.097], slack)?.id).toBe(2);
    // In its bounding box, 6 mm off its line: the viewport it sits on.
    expect(hitItem(r, [0.07, 0.106], slack)?.id).toBe(1);
    // The turned label: along its run upwards from the anchor, not beside it.
    expect(hitItem(r, [0.199, 0.14], slack)?.id).toBe(3);
    expect(hitItem(r, [0.21, 0.155], slack)?.id).toBe(1);
    // An outline rectangle takes clicks on its outline, not inside it ...
    expect(hitItem(r, [0.05, 0.045], slack)?.id).toBe(4);
    expect(hitItem(r, [0.07, 0.045], slack)?.id).toBe(1);
    // ... unless it is selected: then anywhere in its box (it drags from its middle).
    expect(hitItem(r, [0.07, 0.045], slack, [4])?.id).toBe(4);
    // A selected dimension still takes clicks only on what it draws.
    expect(hitItem(r, [0.07, 0.106], slack, [2])?.id).toBe(1);
    // Outside everything.
    expect(hitItem(r, [0.3, 0.3], slack)).toBeNull();
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

  it("takes a dimension's ends from where the kernel drew them (its anchors followed the model)", () => {
    // Stored at 30 in wide; the kernel found the anchored corner 6 in further out (36 in).
    const k = { kind: "dimension" as const, viewport: 1, dim: "horizontal" as const, points: [[0.381, 0, 0.6], [0.381 + 0.08, 0, 0.6]] as V3[], offset: 0.01 };
    const followed: SheetRender = { ...render, items: render.items.map((i) => (i.id === 2 ? { ...i, points: [[0.381, 0, 0.6], [0.381 + 0.16, 0, 0.6]] as V3[] } : i)) };
    const stored = dimPaper(followed, k)!;
    const drawn = dimPaper(followed, k, 2)!;
    expect(stored[1][0] - stored[0][0]).toBeCloseTo(0.01, 12);
    expect(drawn[1][0] - drawn[0][0]).toBeCloseTo(0.02, 12);
    // No points from the kernel (an older render, or another id): the stored ones.
    expect(dimPaper(render, k, 2)).toEqual(stored);
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

describe("PDF and PNG", () => {
  it("writes the sheet as a vector PDF at its paper size, the strings as text", () => {
    const bytes = sheetsPdf([render, { ...render, id: 2, name: "Two" }], new Map(), "Bookshelf");
    const pdf = readPdf(bytes);
    expect(pdf.pages).toBe(2);
    expect(pdf.text[0]).toEqual(['A & B <30">', "Bookshelf"]);
    const s = new TextDecoder("latin1").decode(bytes);
    expect(s).toContain("/MediaBox [0 0 792 612]");
    // Colours, the even-odd fill, the margin frame, a dashless line; no images, so no XObject.
    expect(s).toContain("f* Q");
    expect(s).toContain("0.094 0.102 0.098 RG");
    expect(s).not.toContain("/XObject");
    // Middle-anchored text starts half its width to the left of its point.
    const tm = /\/F1 6\.8 Tf 1 0 0 1 ([\d.]+) ([\d.]+) Tm \(A & B <30">\) Tj/.exec(s);
    expect(tm).not.toBeNull();
    expect(Number(tm![1])).toBeCloseTo(0.05 * PT - textWidth('A & B <30">', 0.0024 * PT) / 2, 1);
    expect(Number(tm![2])).toBeCloseTo(612 - 0.04 * PT, 1);
    // Deterministic.
    expect(sheetsPdf([render, { ...render, id: 2, name: "Two" }], new Map(), "Bookshelf")).toEqual(bytes);
  });

  it("puts images in as JPEG XObjects and turns rotated text", () => {
    const withImage: SheetRender = { ...render, prims: [...render.prims, { p: "image", rect: [0.1, 0.1, 0.02, 0.01], path: "assets/logo.png" }, { p: "text", at: [0.1, 0.1], text: "72\"", size: 0.0024, anchor: "middle", angle: 90, color: [24, 26, 25] }] };
    const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 1, 2, 0xff, 0xd9]);
    const bytes = sheetsPdf([withImage], new Map([["assets/logo.png", { jpeg, width: 40, height: 20, dataUrl: "" }]]), "x");
    const pdf = readPdf(bytes);
    expect(pdf.text[0]).toContain('72"');
    const s = new TextDecoder("latin1").decode(bytes);
    expect(s).toContain("/Subtype /Image /Width 40 /Height 20 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length 9");
    expect(s).toMatch(/\/XObject << \/Im1 \d+ 0 R >>/);
    expect(s).toContain("/Im1 Do Q");
    expect(s).toMatch(/0 1 -1 0 [\d.]+ [\d.]+ Tm \(72"\) Tj/);
  });

  it("knows the diameter sign", () => {
    expect(winAnsi("Ø")).toEqual([0xd8]);
    expect(textWidth("Ø", 1000)).toBe(778);
  });

  it("writes the resolution into a PNG", () => {
    expect(crc32(new TextEncoder().encode("123456789"))).toBe(0xcbf43926);
    // The smallest PNG: signature, IHDR (1 x 1 RGB), IEND (CRCs are not checked here).
    const chunk = (type: string, body: number[]) => {
      const out = [0, 0, 0, body.length, ...[...type].map((c) => c.charCodeAt(0)), ...body];
      const crc = crc32(new Uint8Array(out.slice(4)));
      return [...out, (crc >>> 24) & 255, (crc >>> 16) & 255, (crc >>> 8) & 255, crc & 255];
    };
    const png = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10, ...chunk("IHDR", [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]), ...chunk("IEND", [])]);
    expect(pngDpi(png)).toBeNull();
    const at300 = withDpi(png, 300);
    expect(pngDpi(at300)).toBe(300);
    expect(at300.length).toBe(png.length + 21);
    // Again: replaced, not added twice.
    expect(pngDpi(withDpi(at300, 150))).toBe(150);
    expect(withDpi(at300, 150).length).toBe(at300.length);
    expect(() => withDpi(new Uint8Array(40), 300)).toThrow("not a PNG");
  });
});
