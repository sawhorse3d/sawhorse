import { describe, expect, it } from "vitest";
import type { Entity, V3 } from "../kernel/types";
import { sketchParts } from "./xform";

/**
 * #11: the gather at the start of a Move, Rotate or Scale drag. `sketchParts` collects the
 * selected sketch points, edges and moving points, each once. It checked every new one
 * against all those gathered so far, so a drag of 4,000 edges paused for most of 0.2 s
 * before it moved (8 ms at 1,000 points, 389 ms at 8,000).
 */
describe("#11: gathering a big selection's sketch geometry", () => {
  const near = (p: V3, q: V3) => Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]) <= 1e-9;
  const isSketch = (e: Entity) => e.kind === "region" || ((e.kind === "edge" || e.kind === "vertex" || e.kind === "curve") && e.owner.kind === "sketch");
  const sketchOf = (e: Entity) => (e.kind === "region" ? e.sketch : (e as { owner: { id: number } }).owner.id);

  /** The gather as it was before #11, written out: every new point and edge checked
   * against every one gathered so far. */
  function scan(es: Entity[], outlines: Map<string, V3[][]>) {
    type Part = { sketch: number; points: V3[]; edges: [V3, V3][]; regions: number[]; rings: V3[][]; moving: V3[]; curves?: number[] };
    const by = new Map<number, Part>();
    for (const e of es) {
      if (!isSketch(e)) continue;
      const id = sketchOf(e);
      let p = by.get(id);
      if (!p) {
        p = { sketch: id, points: [], edges: [], regions: [], rings: [], moving: [] };
        by.set(id, p);
      }
      const part = p;
      const mv = (q: V3) => {
        if (!part.moving.some((r) => near(r, q))) part.moving.push(q);
      };
      if (e.kind === "vertex") {
        if (!p.points.some((q) => near(q, e.at))) p.points.push(e.at);
        mv(e.at);
      } else if (e.kind === "edge") {
        if (!p.edges.some(([a, b]) => (near(a, e.a) && near(b, e.b)) || (near(a, e.b) && near(b, e.a)))) p.edges.push([e.a, e.b]);
        mv(e.a);
        mv(e.b);
      } else if (e.kind === "region" && !p.regions.includes(e.region)) {
        p.regions.push(e.region);
        for (const ring of outlines.get(`r${e.sketch}:${e.region}`) ?? []) {
          p.rings.push(ring);
          ring.forEach(mv);
        }
      } else if (e.kind === "curve") {
        const curves = (p.curves ??= []);
        if (!curves.includes(e.curve)) {
          curves.push(e.curve);
          for (const ring of outlines.get(`c${e.owner.id}:${e.curve}`) ?? []) ring.forEach(mv);
        }
      }
    }
    return [...by.values()];
  }

  const strip = (parts: ReturnType<typeof sketchParts>) => parts.map(({ sketch, points, edges, regions, rings, moving, curves }) => ({ sketch, points, edges, regions, rings, moving, ...(curves ? { curves } : {}) }));

  it("over 500 seeded random selections, gathers the same points, edges and moving points, in the same order, as checking each against all", () => {
    let seed = 2026;
    const rnd = () => {
      seed = (seed + 0x6d2b79f5) >>> 0;
      let t = seed;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const pick = <T,>(a: T[]): T => a[Math.floor(rnd() * a.length)];
    // A copy of `p`, or one moved by a fraction or a multiple of the 1 nm tolerance (or a
    // coordinate that is not finite, now and then) in a random direction.
    const nudge = (p: V3): V3 => {
      const r = rnd();
      if (r < 0.3) return [p[0], p[1], p[2]];
      if (r < 0.33) return pick<V3>([[NaN, p[1], p[2]], [Infinity, p[1], p[2]], [p[0], -Infinity, p[2]]]);
      const d: V3 = [rnd() - 0.5, rnd() - 0.5, rnd() - 0.5];
      const f = (pick([0, 0.25, 0.5, 0.999, 1, 1.0001, 1.5, 2, 3]) * 1e-9) / (Math.hypot(...d) || 1);
      return [p[0] + d[0] * f, p[1] + d[1] * f, p[2] + d[2] * f];
    };
    const seen = { kept: 0, dropped: 0 };
    for (let c = 0; c < 500; c++) {
      // Points from 1 nm to 10 km apart, below zero, off the ground, on two sketches.
      const step = pick([1e-9, 3e-9, 1e-3, 1, 1e4]);
      const base: V3[] = Array.from({ length: 2 + Math.floor(rnd() * 10) }, () => [Math.floor(rnd() * 7 - 3) * step, Math.floor(rnd() * 7 - 3) * step, rnd() < 0.6 ? 0 : Math.floor(rnd() * 7 - 3) * step]);
      const owner = () => ({ kind: "sketch" as const, id: rnd() < 0.8 ? 1 : 2 });
      const outlines = new Map<string, V3[][]>();
      const es: Entity[] = Array.from({ length: 1 + Math.floor(rnd() * 40) }, (_, i): Entity => {
        const r = rnd();
        if (r < 0.25) return { kind: "vertex", owner: owner(), at: nudge(pick(base)) };
        if (r < 0.8) {
          const [a, b] = [nudge(pick(base)), nudge(pick(base))];
          return { kind: "edge", owner: owner(), a, b, drawn: true };
        }
        if (r < 0.9) {
          const sketch = rnd() < 0.8 ? 1 : 2;
          const region = 1 + Math.floor(rnd() * 3);
          outlines.set(`r${sketch}:${region}`, [Array.from({ length: 3 + Math.floor(rnd() * 3) }, () => nudge(pick(base)))]);
          return { kind: "region", sketch, region };
        }
        const o = owner();
        outlines.set(`c${o.id}:${i}`, [Array.from({ length: 3 + Math.floor(rnd() * 4) }, () => nudge(pick(base)))]);
        return { kind: "curve", owner: o, curve: rnd() < 0.3 ? 1 : i };
      });
      // Now and then a solid's edge or point with them: never gathered.
      if (rnd() < 0.2) es.push({ kind: "edge", owner: { kind: "object", id: 1 }, a: pick(base), b: pick(base), drawn: true });
      const want = scan(es, outlines);
      expect(strip(sketchParts(es, outlines)), `case ${c}`).toEqual(want);
      const given = es.filter((e) => e.kind === "edge" || e.kind === "vertex").length;
      const gathered = want.reduce((n, p) => n + p.edges.length + p.points.length, 0);
      seen.kept += gathered;
      seen.dropped += given - gathered;
    }
    // The cases reach both sides: repeats dropped as well as near misses kept.
    expect(seen.kept).toBeGreaterThan(1000);
    expect(seen.dropped).toBeGreaterThan(300);
  });

  /** `n` sketch edges as separate unit squares in a row, selected one by one. */
  function squares(n: number): Entity[] {
    const es: Entity[] = [];
    for (let i = 0; i < n / 4; i++) {
      const x = i * 2;
      const sq: V3[] = [[x, 0, 0], [x + 1, 0, 0], [x + 1, 1, 0], [x, 1, 0]];
      for (let k = 0; k < 4; k++) es.push({ kind: "edge", owner: { kind: "sketch", id: 1 }, a: sq[k], b: sq[(k + 1) % 4], drawn: true });
    }
    return es;
  }

  /** How much longer `big` takes than `small`: one warm-up of each, then the fastest of
   * nine runs of each, taken in turn so a burst of load from elsewhere hits both. */
  function slower(small: () => void, big: () => void): number {
    small();
    big();
    let [s, b] = [Infinity, Infinity];
    for (let k = 0; k < 9; k++) {
      let t = performance.now();
      small();
      s = Math.min(s, performance.now() - t);
      t = performance.now();
      big();
      b = Math.min(b, performance.now() - t);
    }
    return b / Math.max(0.05, s);
  }

  it("costs in step with the selection, not with its square", () => {
    const small = squares(1000);
    const big = squares(8000);
    const none = new Map<string, V3[][]>();
    expect(sketchParts(big, none)[0].edges).toHaveLength(8000);
    expect(sketchParts(big, none)[0].moving).toHaveLength(8000);
    // Eight times the edges: about eight times the work in step, sixty-four times when each
    // is checked against all gathered so far. The bound sits well between the two, as the
    // #8 test's does.
    expect(slower(() => sketchParts(small, none), () => sketchParts(big, none))).toBeLessThan(24);
  });
});
