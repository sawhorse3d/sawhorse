// Procedural SketchUp models for the .skp test corpus (tests/corpus/skp/) and the skp tests.
// Every model is built with openskp's MIT writer from a seed, so the corpus is reproducible
// byte for byte (scripts/corpus-skp.mjs writes it; CI regenerates it). Nothing here comes
// from 3D Warehouse, OpenSKP's own fixtures, or any SketchUp-authored file (ADR-0002).
//
// Units are SketchUp's: inches, Z up. Faces are wound counter-clockwise seen from outside.
// This file must stay plain enough for Node's type stripping (no enums, no namespaces).
import { create, type ComponentDefinitionBuilder, type SkpBuilder } from "openskp";

export type P2 = [number, number];
export type P3 = [number, number, number];
type Target = SkpBuilder | ComponentDefinitionBuilder;

/** Mulberry32: a small, fast, seeded PRNG (the corpus must be reproducible). */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const round = (x: number) => Math.round(x * 1000) / 1000;

function signedArea(p: P2[]): number {
  let a = 0;
  for (let i = 0; i < p.length; i++) {
    const [x0, y0] = p[i];
    const [x1, y1] = p[(i + 1) % p.length];
    a += x0 * y1 - x1 * y0;
  }
  return a / 2;
}

const ccw = (p: P2[]) => (signedArea(p) < 0 ? [...p].reverse() : p);
const cw = (p: P2[]) => (signedArea(p) > 0 ? [...p].reverse() : p);

export interface FaceOpts {
  material?: number;
  layer?: number;
}

/** A closed prism: polygon `outer` (with optional holes) from z0 up to z1, outward faces. */
export function prism(t: Target, outer: P2[], z0: number, z1: number, o: FaceOpts = {}, holes: P2[][] = []): void {
  const out = ccw(outer);
  const hs = holes.map(cw);
  const at = (p: P2, z: number): P3 => [round(p[0]), round(p[1]), round(z)];
  const opts = (extra: object = {}) => ({ ...(o.material !== undefined ? { material: o.material } : {}), ...(o.layer !== undefined ? { layer: o.layer } : {}), ...extra });
  // Top faces up (CCW from above), bottom faces down (reversed).
  t.addFace(
    out.map((p) => at(p, z1)),
    opts(hs.length ? { holes: hs.map((h) => h.map((p) => at(p, z1))) } : {}),
  );
  t.addFace(
    [...out].reverse().map((p) => at(p, z0)),
    opts(hs.length ? { holes: hs.map((h) => [...h].reverse().map((p) => at(p, z0))) } : {}),
  );
  for (const ring of [out, ...hs]) {
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i];
      const b = ring[(i + 1) % ring.length];
      t.addFace([at(a, z0), at(b, z0), at(b, z1), at(a, z1)], opts());
    }
  }
}

export function rect(x: number, y: number, w: number, d: number): P2[] {
  return [
    [x, y],
    [x + w, y],
    [x + w, y + d],
    [x, y + d],
  ];
}

export function circle(cx: number, cy: number, r: number, n: number, a0 = 0): P2[] {
  return Array.from({ length: n }, (_, k) => {
    const a = a0 + (2 * Math.PI * k) / n;
    return [cx + r * Math.cos(a), cy + r * Math.sin(a)] as P2;
  });
}

/** A box from (x, y, z) with size (w, d, h). */
export function box(t: Target, x: number, y: number, z: number, w: number, d: number, h: number, o: FaceOpts = {}): void {
  prism(t, rect(x, y, w, d), z, z + h, o);
}

/** A solid standing on the XZ plane: polygon in (x, z), extruded along +Y by `depth`. */
export function extrudeXZ(t: Target, profile: P2[], y0: number, depth: number, o: FaceOpts = {}): void {
  // Seen from the front (-Y), x runs right and z up, so a counter-clockwise profile faces
  // -Y: it is the front face as given, the back face reversed, and each side quad
  // (a, a', b', b) faces outward.
  const prof = ccw(profile);
  const at = (p: P2, y: number): P3 => [round(p[0]), round(y), round(p[1])];
  const opts = { ...(o.material !== undefined ? { material: o.material } : {}), ...(o.layer !== undefined ? { layer: o.layer } : {}) };
  t.addFace(prof.map((p) => at(p, y0)), opts);
  t.addFace([...prof].reverse().map((p) => at(p, y0 + depth)), opts);
  for (let i = 0; i < prof.length; i++) {
    const a = prof[i];
    const b = prof[(i + 1) % prof.length];
    t.addFace([at(a, y0), at(a, y0 + depth), at(b, y0 + depth), at(b, y0)], opts);
  }
}

export interface ModelSpec {
  /** File name without extension, e.g. "007-table". */
  name: string;
  family: string;
  build: () => Uint8Array;
}

const WOODS: [string, number[]][] = [
  ["Oak", [196, 154, 108, 255]],
  ["Walnut", [93, 64, 45, 255]],
  ["Maple", [232, 208, 167, 255]],
  ["Cherry", [150, 75, 50, 255]],
  ["Pine", [222, 189, 135, 255]],
  ["Painted white", [240, 240, 236, 255]],
  ["Slate", [90, 100, 110, 255]],
  ["Glass", [170, 210, 230, 110]],
];

function wood(b: SkpBuilder, r: () => number): number {
  const [n, c] = WOODS[Math.floor(r() * WOODS.length)];
  return b.addMaterial(n, c);
}

// ---- families --------------------------------------------------------------------------

function boxes(r: () => number): SkpBuilder {
  const b = create();
  const m = r() < 0.6 ? wood(b, r) : undefined;
  const tag = r() < 0.5 ? b.addLayer("Stock") : undefined;
  const n = 1 + Math.floor(r() * 4);
  const parts = Array.from({ length: n }, (_, k) => ({ w: 4 + r() * 30, d: 2 + r() * 20, h: 0.75 + r() * 10, x: k * 40 }));
  for (const [k, p] of parts.entries())
    b.addGroup((g) => box(g, 0, 0, 0, p.w, p.d, p.h), { name: `Board ${k + 1}`, translation: [p.x, 0, 0], ...(m !== undefined ? { material: m } : {}), ...(tag !== undefined ? { layer: tag } : {}) });
  return b;
}

function table(r: () => number): SkpBuilder {
  const b = create();
  const top = wood(b, r);
  const legM = wood(b, r);
  const legs = b.addLayer("Legs");
  const w = 30 + r() * 40;
  const d = 20 + r() * 20;
  const h = 26 + r() * 6;
  const s = 1.5 + r() * 1.5;
  const leg = b.addComponentDefinition("Leg", (def) => box(def, 0, 0, 0, s, s, h - 1), {});
  b.addGroup((g) => box(g, 0, 0, h - 1, w, d, 1), { name: "Top", material: top });
  for (const [x, y] of [
    [0, 0],
    [w - s, 0],
    [w - s, d - s],
    [0, d - s],
  ])
    b.addInstance(leg, { translation: [x, y, 0], layer: legs, material: legM });
  return b;
}

function bookshelf(r: () => number): SkpBuilder {
  const b = create();
  const m = wood(b, r);
  const shelves = 3 + Math.floor(r() * 4);
  const w = 24 + r() * 24;
  const d = 9 + r() * 4;
  const h = 36 + r() * 48;
  const t = 0.75;
  const side = b.addComponentDefinition("Side", (def) => box(def, 0, 0, 0, t, d, h));
  const shelf = b.addComponentDefinition("Shelf", (def) => box(def, 0, 0, 0, w - 2 * t, d, t));
  b.addGroup((g) => box(g, 0, d, 0, w, 0.25, h), { name: "Back panel" });
  b.addInstance(side, { translation: [0, 0, 0], material: m });
  b.addInstance(side, { translation: [w - t, 0, 0], material: m });
  for (let k = 0; k < shelves; k++) b.addInstance(shelf, { translation: [t, 0, round(((h - t) * k) / (shelves - 1))], material: m });
  return b;
}

function house(r: () => number): SkpBuilder {
  const b = create();
  const wallM = b.addMaterial("Siding", [214, 200, 180, 255]);
  const roofM = b.addMaterial("Roofing", [120, 60, 50, 255]);
  const walls = b.addLayer("Walls");
  const roof = b.addLayer("Roof");
  const W = 240 + r() * 240;
  const D = 180 + r() * 180;
  const H = 96 + r() * 24;
  const t = 6;
  const win = r() < 0.7;
  // Front wall with a window hole (a face with a hole on both sides, and the reveal faces).
  b.addGroup(
    (g) => {
      // The wall is a prism built in XY and turned up into XZ: (x, y, z) -> (x, z, y).
      const outer = rect(0, 0, W, H);
      const holes = win ? [rect(W * 0.35, H * 0.4, 36, 30)] : [];
      // Extrude in Y by building in XY then mapping (x, y, z) -> (x, z, y) is a mirror, so
      // reverse the polygons to keep faces outward.
      const tmp: { pts: P3[]; holes?: P3[][] }[] = [];
      const collector = {
        addFace: (pts: P3[], o?: { holes?: P3[][] }) => tmp.push({ pts, holes: o?.holes }),
      } as unknown as Target;
      prism(collector, outer, 0, t, {}, holes);
      for (const f of tmp) {
        const sw = (p: P3): P3 => [p[0], p[2], p[1]];
        g.addFace([...f.pts].reverse().map(sw), { material: wallM, layer: walls, ...(f.holes ? { holes: f.holes.map((h) => [...h].reverse().map(sw)) } : {}) });
      }
    },
    { name: "Front wall", layer: walls },
  );
  b.addGroup((g) => box(g, 0, D - t, 0, W, t, H, { material: wallM }), { name: "Back wall", layer: walls });
  b.addGroup((g) => box(g, 0, t, 0, t, D - 2 * t, H, { material: wallM }), { name: "Left wall", layer: walls });
  b.addGroup((g) => box(g, W - t, t, 0, t, D - 2 * t, H, { material: wallM }), { name: "Right wall", layer: walls });
  const rise = 40 + r() * 50;
  b.addGroup(
    (g) =>
      extrudeXZ(
        g,
        [
          [-12, 0],
          [W + 12, 0],
          [W / 2, rise],
        ].map(([x, z]) => [x, H + z] as P2),
        -12,
        D + 24,
        { material: roofM },
      ),
    { name: "Roof", layer: roof },
  );
  return b;
}

function stairs(r: () => number): SkpBuilder {
  const b = create();
  const m = wood(b, r);
  const n = 3 + Math.floor(r() * 10);
  const run = 9 + r() * 3;
  const rise = 6.5 + r() * 1.5;
  const width = 30 + r() * 18;
  if (r() < 0.5) {
    // One solid: a stepped profile extruded across the width.
    const prof: P2[] = [[0, 0]];
    for (let k = 0; k < n; k++) {
      prof.push([k * run, (k + 1) * rise]);
      prof.push([(k + 1) * run, (k + 1) * rise]);
    }
    prof.push([n * run, 0]);
    b.addGroup((g) => extrudeXZ(g, prof, 0, width, { material: m }), { name: "Stair" });
  } else {
    const tread = b.addComponentDefinition("Step", (def) => box(def, 0, 0, 0, run, width, rise));
    for (let k = 0; k < n; k++) b.addInstance(tread, { translation: [round(k * run), 0, round(k * rise)], material: m });
  }
  return b;
}

function curved(r: () => number): SkpBuilder {
  const b = create();
  const m = wood(b, r);
  const kind = Math.floor(r() * 4);
  const seg = 12 + Math.floor(r() * 36);
  const R = 2 + r() * 10;
  if (kind === 0) b.addGroup((g) => prism(g, circle(0, 0, R, seg), 0, 1 + r() * 30, { material: m }), { name: "Dowel" });
  if (kind === 1) b.addGroup((g) => prism(g, circle(0, 0, R, seg), 0, 1 + r() * 4, { material: m }, [circle(0, 0, R * 0.5, seg)]), { name: "Ring" });
  if (kind === 2) {
    // A half-ring arch standing up.
    const outer = circle(0, 0, R, seg * 2).filter(([, y]) => y >= -1e-9);
    const inner = circle(0, 0, R * 0.6, seg * 2).filter(([, y]) => y >= -1e-9).reverse();
    b.addGroup((g) => extrudeXZ(g, [...outer, ...inner], 0, 1 + r() * 3, { material: m }), { name: "Arch" });
  }
  if (kind === 3) {
    // A round table top with a pedestal (two solids in one component).
    const def = b.addComponentDefinition("Round table", (d) => {
      prism(d, circle(0, 0, R * 3, seg), 28, 29);
      prism(d, circle(0, 0, R * 0.5, seg), 0, 28);
    });
    b.addInstance(def, { translation: [0, 0, 0], material: m });
  }
  return b;
}

function nested(r: () => number): SkpBuilder {
  const b = create();
  const m = wood(b, r);
  const brass = b.addMaterial("Brass", [181, 166, 66, 255]);
  const w = 14 + r() * 10;
  const knob = b.addComponentDefinition("Knob", (def) => prism(def, circle(0, 0, 0.5, 12), 0, 1));
  const drawer = b.addComponentDefinition("Drawer", (def) => {
    box(def, 0, 0, 0, w, 16, 5);
    def.addInstance(knob, { translation: [w / 2, 0, 2.5], matrix3x3: [1, 0, 0, 0, 0, -1, 0, 1, 0], material: brass });
  });
  const n = 2 + Math.floor(r() * 3);
  for (let k = 0; k < n; k++) b.addInstance(drawer, { translation: [0, 0, round(k * 6)], material: m });
  // A mirrored copy (negative scale) of the stack beside it.
  b.addInstance(drawer, { translation: [2 * w + 4, 0, 0], matrix3x3: [-1, 0, 0, 0, 1, 0, 0, 0, 1], material: m });
  return b;
}

function loose(r: () => number): SkpBuilder {
  const b = create();
  const m = wood(b, r);
  // Loose faces (not a solid), loose edges, and a closed box drawn straight into the model.
  const s = 10 + r() * 20;
  b.addGroup((g) => box(g, 0, 0, 0, s, s, s, { material: m }), { name: "Crate" });
  b.addFace(
    rect(s + 10, 0, s, s).map(([x, y]) => [round(x), round(y), 0] as P3),
    { material: m },
  );
  b.addPolyline([
    [0, -10, 0],
    [s, -10, 0],
    [s, -20, 0],
  ]);
  b.addArc([0, -30, 0], [0, 0, 1], 5, 0, Math.PI);
  return b;
}

function open(r: () => number): SkpBuilder {
  const b = create();
  const m = wood(b, r);
  // A box with no lid: open edges, so it must come in as a visible non-solid mesh.
  const s = 6 + r() * 20;
  b.addGroup(
    (g) => {
      const at = (x: number, y: number, z: number): P3 => [round(x * s), round(y * s), round(z * s)];
      g.addFace([at(0, 0, 0), at(0, 1, 0), at(1, 1, 0), at(1, 0, 0)], { material: m });
      g.addFace([at(0, 0, 0), at(1, 0, 0), at(1, 0, 1), at(0, 0, 1)], { material: m });
      g.addFace([at(1, 0, 0), at(1, 1, 0), at(1, 1, 1), at(1, 0, 1)], { material: m });
      g.addFace([at(1, 1, 0), at(0, 1, 0), at(0, 1, 1), at(1, 1, 1)], { material: m });
      g.addFace([at(0, 1, 0), at(0, 0, 0), at(0, 0, 1), at(0, 1, 1)], { material: m });
    },
    { name: "Tray" },
  );
  return b;
}

function reversed(r: () => number): SkpBuilder {
  const b = create();
  const s = 5 + r() * 20;
  // A closed box whose top was drawn the wrong way round (a classic reversed face).
  b.addGroup(
    (g) => {
      const at = (x: number, y: number, z: number): P3 => [round(x * s), round(y * s), round(z * s)];
      g.addFace([at(0, 0, 0), at(0, 1, 0), at(1, 1, 0), at(1, 0, 0)]);
      g.addFace([at(0, 0, 1), at(0, 1, 1), at(1, 1, 1), at(1, 0, 1)]); // reversed: faces down
      g.addFace([at(0, 0, 0), at(1, 0, 0), at(1, 0, 1), at(0, 0, 1)]);
      g.addFace([at(1, 0, 0), at(1, 1, 0), at(1, 1, 1), at(1, 0, 1)]);
      g.addFace([at(1, 1, 0), at(0, 1, 0), at(0, 1, 1), at(1, 1, 1)]);
      g.addFace([at(0, 1, 0), at(0, 0, 0), at(0, 0, 1), at(0, 1, 1)]);
    },
    { name: "Block" },
  );
  return b;
}

export const FAMILIES: Record<string, (r: () => number) => SkpBuilder> = {
  boxes,
  table,
  bookshelf,
  house,
  stairs,
  curved,
  nested,
  loose,
  open,
  reversed,
};

/** The corpus: `count` models cycling through the families, seeded by their index. */
export function corpus(count = 200): ModelSpec[] {
  const names = Object.keys(FAMILIES);
  return Array.from({ length: count }, (_, i) => {
    const family = names[i % names.length];
    return {
      name: `${String(i + 1).padStart(3, "0")}-${family}`,
      family,
      build: () => FAMILIES[family](rng(1000 + i)).toBytes(),
    };
  });
}

/** The committed golden sample: the first two models of each family (20 files). */
export function sample(): ModelSpec[] {
  return corpus().slice(0, 20);
}
