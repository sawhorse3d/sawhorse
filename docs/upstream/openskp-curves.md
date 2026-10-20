# Draft upstream request: openskp's parse output does not say which edges make up a curve

Status: a draft for the maintainer to file on openskp's issue tracker (not filed by the build). Written 2026-10-19 (lane 23). It describes what the published npm package (1.3.0) gives a caller, read from its `dist/index.d.ts` and `dist/index.js`; nothing here comes from any repository's source.

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
