# ADR-0001: Geometry kernel

- Status: Accepted, 2026-10-11 (seed decisions); implementation started in lane 1 (foundations), 2026-10-11
- Deciders: the maintainer
- Source: `docs/DECISIONS.md` seed, vault note `the maintainer's notes`

## Context

Sawhorse is a push/pull modeler. Its kernel has to make "draw a closed loop, get a face; push the face, get a solid" feel sticky and predictable, export watertight files for printing and CNC, and run identically on the desktop (Tauri 2) and in a plain browser with no install. None of the surveyed Rust geometry crates supports faces with hole loops on a half-edge B-rep, so the topology layer is ours.

## Decision

- One pure-Rust `kernel` crate (no UI, no IO, no network), compiled to `wasm32-unknown-unknown` and run in the webview on both Tauri 2.x desktop and the browser build. One code path, no IPC serialization of geometry. A native sidecar stays possible because the kernel is pure.
- Math: `glam` f64 types (DVec3, DAffine3). All lengths in f64 metres inside the kernel; units convert at the edges (UI, exporters). One `tolerance` module with named constants; a separate, wider planarity tolerance for imported geometry.
- Ids: `slotmap` generational keys for Vertex/HalfEdge/Edge/Face/Loop/Object. Deterministic collections only (BTreeMap / slotmap order); never let HashMap iteration order affect output.
- Predicates: `robust` (Shewchuk) for orient2d/orient3d decisions on the face's projected 2D frame.
- Topology: our own half-edge B-rep. Face = one outer Loop + N inner Loops (holes) + a stored plane. Edge has one or two half-edges. Object tracks a watertight flag. No surveyed crate supports hole loops; do not adopt one.
- Two tiers (sticky geometry):
  - **Sketch**: a persistent planar graph on any plane, including planes attached to an Object's face. Partial strokes are legal. Rules on insertion: endpoint within POINT_MERGE of a vertex merges; endpoint within tolerance of an edge interior splits it (T-junction); proper crossings split both; collinear overlaps normalise to non-overlapping edges; after every insertion recompute regions as minimal closed circuits (angular sort of half-edges at each vertex, walk faces); a circuit strictly inside another becomes a hole. This is "a closed coplanar loop auto-forms a face".
  - **Object**: a manifold watertight half-edge solid born by extruding a Sketch region (push/pull). Drawing on an Object face = `split_face` (cut path with endpoints on the boundary) or `imprint` (closed loop inside a face becomes a coplanar sub-face twinned with a hole loop in the parent). Coplanar merge dissolves non-preserved edges between coplanar faces after push/pull-through and after booleans.
  - Stickiness is scoped to the editing context (the Object/Group being edited), never global. Touching Objects never weld; combining is an explicit boolean. Inference/snapping is a global read-only query.
- Push/pull: extrude the face region and build/merge side walls so the shell stays closed; pushing through the opposite face takes the boolean path (difference with the swept prism).
- Booleans: `manifold-rust` (Apache-2.0, pin exactly) behind our own `BooleanEngine` trait. Triangulate operand faces with `earcut` (georust), carry `face_id` per triangle through Manifold (MeshGL64), group output triangles by inherited face id, re-polygonise into n-gons with holes, re-fit planes, weld, coplanar-merge (preserving user-drawn imprint edges), validate, commit. Differential tests against `manifold-csg` (C++) in native CI; npm `manifold-3d` as the webview fallback.
- Triangulation for render/export: `earcut` with hole indices + area-deviation check; fallback `i_triangle`; typed failure (no silent repair).
- 2D polygon ops (coplanar merge, imprint tests): `i_overlay`. Offset tool: `cavalier_contours`.
- Picking/inference: own BVH over faces-with-holes, edges and vertices with pixel-to-world tolerance (or `parry3d-f64` partitioning::Bvh); `rstar` for snap candidates. three.js raycasting is only for coarse hover.
- Export: STL hand-written (binary, Z-up, mm, gated on watertight); 3MF via `threemf`; GLB hand-written (Y-up, metres); DXF hand-written ASCII (3DFACE + LINE); SVG line drawings. Import: skp (OpenSKP TS in the webview), dxf, stl, obj, gltf/glb, 3mf, svg.
- Do NOT use: truck/shapeops, csgrs, opencascade-rs, baby_shark, earcutr, lyon, the `bvh` crate (nalgebra + rayon defaults).
- Robustness machinery FIRST: topology validator after every mutation (debug) and validate-on-clone before commit (release); transactional apply (clone -> mutate -> validate -> commit); typed errors (SketchError, PushError, BooleanError); proptest invariants (extrude => watertight; push/pull then inverse restores topology; split then merge is identity); op-sequence fuzzer writing `tests/repro/*.json` on failure; deterministic replay with golden hashes.
- Run the kernel in a Web Worker; Manifold WASM is single-threaded.
- Day-1 spike: compile the candidate crate set to wasm32-unknown-unknown before any tool work.

## Consequences and implementation status

### Day-1 spike (2026-10-11): passed, nothing dropped

`cargo check --workspace --target wasm32-unknown-unknown` passes with glam 0.34 (f64), slotmap 1.1, robust 1.2, earcut 0.4.11 (georust), i_overlay 9.0, manifold-rust =0.16.0 (pure Rust; `parallel`/rayon left off), threemf 0.8 (zip 8 + zlib-rs, pure Rust) and wasm-bindgen 0.2.129. proptest 1.12 and arbitrary 1.5 are test-only. The WASM release build is 556 KB (183 KB gzipped) without wasm-opt.

### Built in lane 1

| Area | Where | Notes |
|---|---|---|
| Crates | `crates/{kernel,tessellate,inference,export,wasm-api}` | `tessellate` has no kernel dependency, so booleans, render and export share one triangulator |
| Tolerances | `kernel/src/tolerance.rs` | LENGTH_EPS 1 nm, POINT_MERGE 0.01 mm, PLANARITY 1 um, IMPORT_PLANARITY 0.1 mm, PARALLEL_EPS, MIN_EXTRUDE, MAX_COORD 10 km, HASH_QUANTUM 1 nm |
| B-rep | `kernel/src/brep.rs` | slotmap ids; builder from indexed polygons pairs twins by directed edge |
| Validator | `kernel/src/validate.rs` | integrity, next/prev, loop closure, twins, planarity, winding, ring self-intersection, hole containment, Euler-Poincare per shell, watertight |
| Sketch | `kernel/src/sketch.rs` | `add_rectangle` only; region recomputation is the general algorithm (angular sort + face walk + hole nesting) |
| Object | `kernel/src/object.rs` | `extrude(region, d)`; push/pull "extend" branch (faces whose neighbours are parallel to the push) |
| Transactions | `kernel/src/model.rs` | `Op` (serde JSON), `apply` = clone, mutate, validate, commit; refusals roll back |
| Booleans | `kernel/src/boolean.rs` | trait + Manifold adapter with face-id provenance; re-polygonise is the push/pull lane |
| Tests | `kernel/tests/` | proptest invariants, sequence fuzzer + repro replay, golden hashes (also replayed in WASM: identical) |

### Deliberate gaps (owned by later lanes)

- Sketch insertion refuses (typed `SketchError::IntersectionUnsupported`) instead of splitting at T-junctions and crossings and normalising collinear overlaps: draw-tools lane.
- Push/pull refuses (typed) when a neighbouring face is not parallel to the push, when edges would collapse, or when the face would pass through the opposite face or another part of the solid: push/pull lane (side-wall creation and the boolean path).
- `split_face`, `imprint`, coplanar merge, re-polygonising boolean output, differential tests against manifold-csg, the `i_triangle` fallback, BVH/rstar inference, offsets: their lanes.
- The kernel runs in a Web Worker today but rebuilds the spike scene per request; a persistent worker-side document with undo arrives with the tools.
- Extruding marks the sketch region consumed (the solid's base face replaces it); collapsing the solid unmarks it. An edit that would change a consumed region is refused. Revisit when drawing on faces lands.
