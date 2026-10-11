# ADR-0001: Geometry kernel

- Status: Accepted, 2026-10-11 (seed decisions); lane 1 (foundations), lane 2 (kernel core) and lane 3 (feel) implemented, 2026-10-11
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

`cargo check --workspace --target wasm32-unknown-unknown` passes with glam 0.34 (f64), slotmap 1.1, robust 1.2, earcut 0.4.11 (georust), i_overlay 9.0, manifold-rust =0.16.0 (pure Rust; `parallel`/rayon left off), threemf 0.8 (zip 8 + zlib-rs, pure Rust) and wasm-bindgen 0.2.129. proptest 1.12 and arbitrary 1.5 are test-only. The day-1 WASM release build was 556 KB (183 KB gzipped) without wasm-opt; since lane 2 calls into manifold-rust for booleans it is about 2.55 MB (about 770 KB gzipped; 2,548,421 bytes / 759,537 bytes at gzip -9 on 2026-10-11).

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

### Built in lane 2 (kernel core, 2026-10-11)

| Area | Where | Notes |
|---|---|---|
| Sketch insertion | `kernel/src/sketch.rs` | Segments insert with the sticky rules: endpoint within POINT_MERGE of a vertex merges; on an edge interior splits it (T-junction); existing vertices on the new segment split it; proper crossings split both; collinear overlaps normalise (duplicate pieces skipped). Only zero-length / NaN / out-of-range input is refused. Ops: `insert_segment`, `add_polyline`, `add_rectangle` (four segments, one transaction), `erase_segment` (heals the T-junctions and crossings it leaves) |
| Regions | `sketch.rs` | Angular face walk; bridges and dangling strokes ignored; circuits split at repeated vertices (pinched regions render but refuse to extrude); clockwise rings become holes of the smallest containing region. **Stable ids**: regions live in a slotmap; re-detection keeps an id by same boundary, same outer ring, or the new region containing the old anchor point (a golden-section interior point). Public ids (slot + reuse count, first key is 1) address sketches, regions, objects and groups in ops |
| Extrude semantics (decided) | `sketch.rs`, `model.rs` | Extruding **consumes** the region: edges no live region needs leave the sketch; a region enclosed entirely by live neighbours stays as a hidden consumed region (so it keeps its id and does not reappear). The resulting face belongs to the Object; drawing on it goes through the Object tier. A solid that collapses flat onto its source sketch releases its footprint back as regions |
| Object-tier drawing | `kernel/src/faceops.rs` | `split_face` (cut path, ends on one boundary loop, interior strictly inside, coplanar; an end on an edge splits it in both faces) and `imprint` (closed loop strictly inside becomes a coplanar sub-face twinned with a hole loop; enclosed holes move in). Typed `FaceEditError` refusals; new edges are flagged **user-drawn** |
| Plain-data B-rep | `brep.rs` (`Polys`, `Brep::build`, `Brep::to_polys`) | Every topology edit is to_polys -> edit -> build -> validate; faces keep stored planes; `Edge.user` survives edits and booleans |
| Coplanar merge | `faceops::merge_coplanar`, `heal_vertices` | Dissolves edges between coplanar same-facing faces except user-drawn ones; heals pass-through vertices |
| General push/pull | `kernel/src/push.rs` | Collapse check; topological path (vertices whose other faces contain the push direction slide, others get rails and side walls; collapsed rails weld so a pocket pulled back flush closes exactly; accepted only if valid, one shell, positive volume and volume change = area x distance); boolean path for everything else (union or difference with the exactly-coincident swept prism; a push landing on a far face reaches 0.5 x POINT_MERGE past it) |
| Booleans | `boolean.rs` | `polys_tri_solid` -> `BooleanEngine` (manifold-rust) -> `repolygonise`: triangles relabelled if the engine leaves one off its source plane, grouped by face id plus coplanar neighbours not split by a user-drawn source edge, loops chained into n-gons with holes (pinches refused), planes kept (flipped where the engine flipped a face), pass-through vertices healed, user edges re-flagged. `combine` is the round trip; `Object::combine` validates |
| Global self-intersection | `kernel/src/intersect.rs` | Part of `Object::validate`: edges through non-incident faces (exact orient3d), chords across a face, vertices within POINT_MERGE of a non-incident face, edges within POINT_MERGE of each other, coplanar faces overlapping; plus positive enclosed volume |
| Document | `kernel/src/document.rs` | Model + op log (undo/redo included) + undo ring of 200 snapshots; replaying the log reproduces the state and the log hash; groups, tags (visibility), delete |
| WASM | `crates/wasm-api` | `document_new()` -> `Doc` with `apply_op(json)`, `undo`, `redo`, `get_view_mesh(object_id)`, `get_scene_mesh`, `document_hash`, `export_stl(object_id)`, `export_obj(object_id)`; the worker holds one persistent document |
| Tests | `kernel/tests/` | sketch_props (order independence, simple boundaries, split-then-erase identity), face_props (split/imprint then merge identity, transactional refusals), push.rs (every push path, volume/area/Euler), booleans.rs (26 hand-computed cases; `npm run boolean:diff` runs them against npm manifold-3d), document unit tests, 13 goldens (WASM == native), op-sequence fuzzer over every op with undo/redo interleaved |

### Built in lane 3 (feel, 2026-10-11)

| Area | Where | Notes |
|---|---|---|
| Persistent face ids | `brep.rs`, `object.rs`, `boolean.rs`, `push.rs` | see the closed gap below; hash format v3 |
| Erase | `faceops::erase_edge`, `erase_face`, `dissolve_between`, `Sketch::erase_region` | `EraseEdge` / `EraseFace` / `EraseRegion`: only user-drawn edges and imprinted faces dissolve; a structural edge or face is refused (`WouldOpenSolid`, `WouldPinch`, `NothingToErase`); `Batch` applies several ops as one transaction and one undo step (`EmptyBatch`, `NotInBatch`) |
| Draw on anything | `kernel/src/draw.rs`, `Op::Draw { target: Sketch / Plane / Face, points, closed }` | One op for every stroke tool. On a face: a closed loop strictly inside imprints; a path or loop crossing the face's edges becomes `split_face` cuts (one per crossing piece), so a dado strip across a board is one op; a stroke that stops inside the face is refused as `Dangling` (the UI keeps it as a preview). On a plane: a new sketch, or the existing sketch on that plane. Outcome `Drawn { sketch, regions, faces, inside }` names what to push next |
| Push/pull with a new face | `Op::PushPullNewFace`, `push::push_pull_with`, `topological_with(allow_slide)` | Ctrl: the old face stays as a seam, new walls rise from it instead of the old walls stretching |
| Annotations | `Guide`, `Dimension`; `AddGuide` / `EraseGuide` / `DeleteGuides` / `AddDimension` / `EraseDimension` | Guide lines (point + direction) and guide points; linear dimensions (a, b, offset). Part of the document, undoable, hashed only when present (older goldens keep their hashes); typed `BadAnnotation`, `NoSuchAnnotation`, `NoGuides` |
| Inference | `crates/inference` (`geom`, `bvh`, `scene`, `pick`, `infer`) | **Own BVH** (median split over segment and face boxes) instead of `rstar` / `parry3d`: the candidate set is small per frame and one structure serves picking and snapping, so no extra crate. Picking with pixel tolerance at depth and occlusion (an edge of the face under the cursor stays visible at oblique angles). Inference kinds with explicit ranks (points > lines > faces): endpoint, midpoint, intersection (edges, guides, edge x guide), centre, on-edge, on-face, axis X/Y/Z from the start point, parallel / perpendicular to a remembered edge, from-point (alignment with a hovered point), on-guide, guide point, grid; axis and direction locks; length snapping to grid steps along a line inference |
| WASM | `crates/wasm-api` | `pick`, `infer`, `outline`, `faces_json`, `edge_owners_json`; `DocState` carries sketches, groups, guides and dimensions. Release build 2,899,563 bytes (861,439 at gzip -9) |
| Tests | `kernel/tests/{face_ids,erase,draw,annotations}.rs`, `inference/tests/infer.rs` | plus the fuzzer over every lane-3 op (50,000 sequences clean, `docs/DEVELOPMENT.md`) |

### Deliberate gaps (owned by later lanes)

- ~~Inference and the draw tools' UI~~ (closed in lane 3, above). Strokes crossing a face's edges are `split_face` cuts (lane-2 open question 1: yes).
- Move / Copy / arrays, Rotate, Scale, Offset, Follow Me: lane 4. The VCB already parses angles and `x3` / `3x` / `*3` / `/4` and refuses them with a message until then.
- Groups and components in the UI (enter / exit, outliner, tags pane): lane 5. The kernel has groups and tags; double-click calls the `enterGroup` hook.
- Booleans on sliver geometry: Manifold's simplification can leave a triangle off its source plane; the kernel relabels it from a neighbour or refuses the op (typed). Differential tests run against npm `manifold-3d` (the C++ engine compiled to WASM); a native `manifold-csg` CI job is still open.
- Tilted planes: positions are f64, so faces that should be coincident after a push on an off-axis plane can miss by 1e-16 m; the kernel snaps where it can (exact edge splits, prism base = the face's own vertices, push-through reach) and otherwise refuses rather than commit a sliver.
- The `i_triangle` triangulation fallback, offsets (cavalier_contours), explicit boolean ops between objects in the UI, .shz save/load of the op log: their lanes.
- ~~Faces are addressed by ordinal~~ (closed in lane 3): faces carry **persistent face ids** (per object, from 1, never reused). Extrude numbers base 1, cap 2, walls 3..; the exact push path keeps every id and gives new walls fresh ones; split_face keeps the id on the first piece; coplanar merge keeps the smallest member's id; the boolean path inherits ids through Manifold's per-triangle face id (the swept prism's cap carries the pushed face's id) and then `boolean::resolve_face_ids` settles duplicates (best plane + centroid + area match keeps the id) and matches renumbered faces to unused operand faces within `ID_MATCH_DISTANCE` / `ID_MATCH_AREA`. Ops, outcomes, view-mesh parts and `Doc.faces_json` all use ids; the model hash (format v3) includes them. Golden and repro logs were migrated ordinal -> id by replay.
