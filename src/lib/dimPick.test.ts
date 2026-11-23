// #129: a dimension in the model is picked on its dimension line, on each extension line and
// on its number, with the kernel's pick radius for the lines; not just outside any of them,
// not seen from behind the eye; and what wins over what when the kernel picked something too.
import { describe, expect, it } from "vitest";
import type { Picked, V3 } from "../kernel/types";
import { DIM_EDGE_PX, dimensionAt, dimensionLines, estimatedLabelSize, overshoot, raySegment, withDimension, type DimPickInput, type PixelScale } from "./dimPick";

// A Top view, orthographic: 1 mm a pixel, the origin at canvas (500, 400), y up the screen.
const MM = 0.001;
const TOP: PixelScale = { ortho: true, scale: MM, eye: [0, 0, 10], forward: [0, 0, -1] };
const topScreen = (p: V3): [number, number] => [500 + p[0] / MM, 400 - p[1] / MM];
const topRay = (x: number, y: number) => ({ origin: [(x - 500) * MM, (400 - y) * MM, 10] as V3, dir: [0, 0, -1] as V3 });

// A 300 mm dimension from (0,0) to (300,0), its line 100 mm in front (y = -100 mm).
const D = { id: 7, a: [0, 0, 0] as V3, b: [0.3, 0, 0] as V3, offset: [0, -0.1, 0] as V3, length: 0.3 };
const SIZE: [number, number] = [30, 18];

function pickAt(x: number, y: number, over: Partial<DimPickInput> = {}) {
  return dimensionAt({ dims: [D], ray: topRay(x, y), px: TOP, cursor: [x, y], toScreen: topScreen, text: () => "300", size: () => SIZE, ...over });
}

describe("the lines of a dimension", () => {
  it("are drawn as annotationView draws them: the line, and two extension lines with their overshoot", () => {
    const g = dimensionLines(D);
    expect(g.line).toEqual([
      [0, -0.1, 0],
      [0.3, -0.1, 0],
    ]);
    expect(overshoot(D.offset)).toEqual([0, -0.0127, 0]);
    expect(g.extensions[0][0]).toEqual([0, 0, 0]);
    expect(g.extensions[0][1][1]).toBeCloseTo(-0.1127, 12);
    expect(g.extensions[1][0]).toEqual([0.3, 0, 0]);
    expect(g.middle).toEqual([0.15, -0.1, 0]);
  });

  it("raySegment finds the nearest points of a ray and a segment", () => {
    const [t, u] = raySegment([0.1, 1, 1], [0, 0, -1], [0, 0, 0], [1, 0, 0]);
    expect(t).toBeCloseTo(1, 12);
    expect(u).toBeCloseTo(0.1, 12);
  });
});

describe("dimensionAt", () => {
  it("picks the dimension line within the kernel's 5 px, and not 6 px off it", () => {
    expect(DIM_EDGE_PX).toBe(5);
    const [x, y] = topScreen([0.25, -0.1, 0]);
    expect(pickAt(x, y + 4)).toMatchObject({ dimension: 7, part: "line" });
    expect(pickAt(x, y + 4)!.px).toBeCloseTo(4, 9);
    expect(pickAt(x, y + 6)).toBeNull();
  });

  it("picks each extension line within 5 px, and not 6 px off it", () => {
    for (const ex of [0, 0.3]) {
      const [x, y] = topScreen([ex, -0.05, 0]);
      expect(pickAt(x + 4.9, y)).toMatchObject({ dimension: 7, part: "extension" });
      expect(pickAt(x - 4, y)).toMatchObject({ dimension: 7, part: "extension" });
      expect(pickAt(x + 6, y)).toBeNull();
      expect(pickAt(x - 6, y)).toBeNull();
    }
    // The overshoot past the dimension line is part of the extension line, as it is drawn.
    const [ox, oy] = topScreen([0, -0.112, 0]);
    expect(pickAt(ox + 2, oy)).toMatchObject({ part: "extension" });
    const [px, py] = topScreen([0, -0.12, 0]);
    expect(pickAt(px + 2, py)).toBeNull();
  });

  it("picks the number anywhere inside its box, and not just outside it", () => {
    const [cx, cy] = topScreen(D.offset.map((v, k) => v + (D.a[k] + D.b[k]) / 2) as V3);
    expect([cx, cy]).toEqual([650, 500]);
    // Inside the box, off the line by more than 5 px: only the number takes it.
    expect(pickAt(cx + 14, cy + 8.5)).toMatchObject({ dimension: 7, part: "number", px: 0 });
    expect(pickAt(cx - 14, cy - 8.5)).toMatchObject({ part: "number" });
    expect(pickAt(cx, cy - 8)).toMatchObject({ part: "number" });
    // Just outside the box (half sizes 15 and 9), and more than 5 px off every line.
    expect(pickAt(cx, cy + 9.5)).toBeNull();
    expect(pickAt(cx, cy - 9.5)).toBeNull();
    // On the line beyond the box: the line, as before.
    expect(pickAt(cx + 40, cy + 2)).toMatchObject({ part: "line" });
    // The box comes from the text's size: a wider number takes a wider box.
    expect(pickAt(cx + 30, cy + 8, { size: () => [70, 18] })).toMatchObject({ part: "number" });
    expect(pickAt(cx + 30, cy + 8)).toBeNull();
  });

  it("a dimension seen end on: its line is a point, and the number and extension lines still pick, nothing just outside", () => {
    // Its line runs straight at the eye (along z); the extension lines lie across the view.
    const E = { id: 3, a: [0, 0, 0] as V3, b: [0, 0, 0.3] as V3, offset: [0.1, 0, 0] as V3, length: 0.3 };
    const at = (x: number, y: number) => dimensionAt({ dims: [E], ray: topRay(x, y), px: TOP, cursor: [x, y], toScreen: topScreen, text: () => "300", size: () => SIZE });
    const [x, y] = topScreen([0.1, 0, 0]);
    const hit = at(x, y);
    expect(hit).toMatchObject({ dimension: 3 });
    expect(Number.isFinite(hit!.t) && Number.isFinite(hit!.px)).toBe(true);
    // Along the extension lines, 4 px off them.
    expect(at(...topScreen([0.05, 0, 0]).map((v, k) => (k ? v + 4 : v)) as [number, number])).toMatchObject({ part: "extension" });
    // 10 px off: outside the box and every line.
    expect(at(x, y + 10)).toBeNull();
    expect(at(x + 20, y)).toBeNull();
  });

  it("a dimension behind the eye is not picked, by its lines or its number", () => {
    // Perspective, the eye at z = 1 looking down: 1 mm a pixel at a depth of 1 m.
    const P: PixelScale = { ortho: false, scale: MM, eye: [0, 0, 1], forward: [0, 0, -1] };
    const ray = (x: number, y: number) => {
      const d: V3 = [(x - 500) * MM, (400 - y) * MM, -1];
      const l = Math.hypot(...d);
      return { origin: P.eye, dir: d.map((v) => v / l) as V3 };
    };
    // A naive projection, as a pinhole maps a point behind it: mirrored through the eye.
    const project = (p: V3): [number, number] => {
      const depth = P.eye[2] - p[2];
      return [500 + p[0] / depth / MM, 400 - p[1] / depth / MM];
    };
    const behind = { ...D, id: 9, a: [0, 0, 2] as V3, b: [0.3, 0, 2] as V3 };
    const front = { ...D, id: 8 };
    const q = (dims: (typeof D)[], p: V3) => {
      const [x, y] = project(p);
      return dimensionAt({ dims, ray: ray(x, y), px: P, cursor: [x, y], toScreen: project, text: () => "300", size: () => SIZE });
    };
    expect(q([front], [0.15, -0.1, 0])).toMatchObject({ dimension: 8, part: "number" });
    expect(q([front], [0.25, -0.1, 0])).toMatchObject({ dimension: 8 });
    expect(q([behind], [0.15, -0.1, 2])).toBeNull();
    expect(q([behind], [0.25, -0.1, 2])).toBeNull();
    expect(q([behind], [0, -0.05, 2])).toBeNull();
  });

  it("of two dimensions, the nearer the cursor wins", () => {
    const other = { ...D, id: 8, offset: [0, -0.108, 0] as V3 };
    const [x, y] = topScreen([0.25, -0.1, 0]);
    expect(dimensionAt({ dims: [D, other], ray: topRay(x, y + 1), px: TOP, cursor: [x, y + 1], toScreen: topScreen, text: () => "300", size: () => SIZE })).toMatchObject({ dimension: 7 });
    expect(dimensionAt({ dims: [D, other], ray: topRay(x, y + 7), px: TOP, cursor: [x, y + 7], toScreen: topScreen, text: () => "300", size: () => SIZE })).toMatchObject({ dimension: 8 });
  });

  it("estimates the box from the label style when nothing measured it", () => {
    expect(estimatedLabelSize("24")).toEqual([2 * 6.6 + 12, 17]);
  });
});

describe("withDimension: what wins", () => {
  const face: Picked = { entity: { kind: "face", object: 1, face: 2 }, point: [0, 0, 0], surface: { at: { kind: "face", object: 1, face: 2 } } } as unknown as Picked;
  const edge: Picked = { entity: { kind: "edge", owner: { kind: "object", id: 1 }, a: [0, 0, 0], b: [1, 0, 0], drawn: false }, point: [0, 0, 0], surface: null } as unknown as Picked;
  const vertex: Picked = { entity: { kind: "vertex", owner: { kind: "object", id: 1 }, at: [0, 0, 0] }, point: [0, 0, 0], surface: null } as unknown as Picked;
  const guide: Picked = { entity: { kind: "guide", guide: 4 }, point: [0, 0, 0], surface: null } as unknown as Picked;
  const hit = (part: "line" | "extension" | "number") => ({ dimension: 7, point: [0.1, 0, 0] as V3, px: 1, t: 1, part });

  it("nothing else under the cursor: the dimension", () => {
    expect(withDimension(null, hit("extension"))).toEqual({ entity: { kind: "dimension", dimension: 7 }, point: [0.1, 0, 0], surface: null });
    expect(withDimension(null, null)).toBeNull();
    expect(withDimension(face, null)).toBe(face);
  });

  it("over a face: the dimension, the face kept as the surface under it", () => {
    for (const part of ["line", "extension", "number"] as const) {
      const r = withDimension(face, hit(part))!;
      expect(r.entity).toEqual({ kind: "dimension", dimension: 7 });
      expect(r.surface).toBe(face.surface);
    }
  });

  it("a point, an edge or a guide the kernel found beats a dimension's lines, as it beat its line before", () => {
    for (const p of [vertex, edge, guide]) {
      expect(withDimension(p, hit("line"))).toBe(p);
      expect(withDimension(p, hit("extension"))).toBe(p);
    }
  });

  it("the number, drawn over everything, wins inside its box", () => {
    for (const p of [vertex, edge, guide, face]) expect(withDimension(p, hit("number"))?.entity).toEqual({ kind: "dimension", dimension: 7 });
  });
});
