# openskp's curve record: asked for in lane 23, there since 1.4.0

Status: **answered upstream.** openskp 1.4.0 (npm, 2026-10-27; its #462) added what the draft below asked for, and Sawhorse uses it since lane 25 (2026-10-28). The draft was never filed by the build; it is kept below as history.

## What openskp 1.4.0 gives (read from its packaged `dist/index.d.ts` and its CHANGELOG)

- `Edge.curveId?: number | null`: SketchUp's own `Edge#curve` grouping, the id of the curve an edge belongs to, for classic (2013 to 2020) and 2021+ files.
- `Definition.arcs: Map<number, ArcCurve>`, keyed by curve id: for every curve that is an arc or a circle (including ones that bound a face), `center`, `normal`, `xAxis` (centre to the start, length the radius), `yAxis` (`normal x xAxis`), `startAngle`, `endAngle` (a full circle: equal in a classic file, `+ 2 pi` in a 2021+ one), `radius` and `isFullCircle`. A curve that is not an arc (a welded curve; a polygon) has no entry. An arc scaled unevenly keeps its record as an affine image of a circle (the axes no longer equal or square).
- In 2021+ files the arc is a record beside each curve's record that 1.3.0 skipped; in classic files it is the `CArcCurve` the reader already stepped over.

What it does not give, compared with the draft: an explicit kind for welded curves and polygons (no `isPolygon`, no segment count) and the curve's edge order (the edges come in their definition's order; Sawhorse orders them into a chain itself).

## What Sawhorse uses (lane 25)

- `curveId` to group a definition's edges into curves, ordered into one chain each (`src/io/skpCurves.ts` `chainOf`; a branch or two pieces stays plain edges, noted).
- `arcs` to know a curve is round and where: its centre and normal, its radius, its start and end angles (the arc's extent and direction) and whether it is a full circle. Each record is checked against the chain's points within the kernel's curve fit (`CURVE_FIT`, 0.04 mm and rounding) before it is trusted (`fitArc`); a record that disagrees leaves the edges plain, with a note naming the piece. An ellipse record, or an arc cut short on a chord, makes a polyline.
- No record: a polyline, closed or open.

They come in through the curve entity Sawhorse already had (ADR-0013 and its lane-25 addendum): a loose curve, or a flat unpainted face's curves, at the top of the model as sketch curves (the import's `strokes`, the path DXF circles use); every other curve as a rim of the solid it runs along (`ImportItem.curves`). `docs/skp-compat.md#curves` has the details and the measured results on real files.

## The draft, as written in lane 23 (2026-10-19)

It described what the published npm package (1.3.0) gave a caller, read from its `dist/index.d.ts` and `dist/index.js`; nothing came from any repository's source.

---

**Title:** `parseSkp`: expose curve membership (CCurve / CArcCurve) on edges

**Version:** openskp 1.3.0 (npm).

**What happens.** In SketchUp a circle, an arc, a polygon or a welded curve is one entity: each of its edges refers to a shared curve object (`CArcCurve`, with its centre, axes, radius, start and end angles and segment count; or `CCurve` for a freeform curve). `parseSkp`'s output has no trace of it: `Definition.edges` are `{ id, v1Id, v2Id, soft, smooth, hidden }`. For classic (2013 to 2020) files the reader does read each `CEdge`'s curve pointer and each `CArcCurve`'s parameters (the writer's `writeArcCurve` / `writeCurve` write them, and a written file parses), but the edges it outputs leave them out; for VFF (2021+) files the edges carry their vertices and flags only.

**To reproduce.**

```js
import { create, parseSkp } from "openskp";
const b = create();
b.addCircle([0, 0, 0], [0, 0, 1], 6, { numSegments: 24 }); // 24 edges sharing one CArcCurve
const m = parseSkp(b.toBytes().buffer);
console.log(Object.keys(m.root.edges[0])); // [ 'id', 'v1Id', 'v2Id', 'soft', 'smooth', 'hidden' ]
```

**Asked for.** On each `Edge`, the id of the curve it belongs to (`curveId?: number`), and on each `Definition`, the curves by id: `{ id, kind: "arc" | "curve", edgeIds (in order), and for an arc: center, normal, xaxis, radius, startAngle, endAngle, numSegments, isPolygon }`, for both file families where the format stores them.

**Why it matters to us.** Sawhorse (a free, MIT OR Apache-2.0 modeler) now has curves (since 0.1.0-alpha.4): a circle is selected, moved and erased whole, and its centre is an inference point. Today every SketchUp curve imports as loose edges, and we do not guess curves from geometry. Our import op already takes a stroke's curve (DXF circles and arcs use it), so the change would light up for SketchUp files at once. Happy to test against our 203-file corpus (written with your writer) and a set of real-world files.
