# Sawhorse — architecture decisions (seed, 2026-10-11)

Split these into `docs/adr/NNNN-*.md` as they are implemented. Source of truth for the plan: the vault note `the maintainer's notes`.

## ADR-0001 Kernel

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
- Robustness machinery FIRST: topology validator after every mutation (debug) and validate-on-clone before commit (release); transactional apply (clone -> mutate -> validate -> commit); typed errors (StickyError, PushPullError, BooleanError); proptest invariants (extrude => watertight; push/pull then inverse restores topology; split then merge is identity); op-sequence fuzzer writing `tests/repro/*.json` on failure; deterministic replay with golden hashes.
- Run the kernel in a Web Worker; Manifold WASM is single-threaded.
- Day-1 spike: compile the candidate crate set to wasm32-unknown-unknown before any tool work.

## ADR-0002 File formats

- Native format is `.shz`: a zip containing `scene.json` (objects, groups, components, tags, scenes, materials, cut-list metadata, units), `geometry.glb` (or per-object glTF), `assets/` (textures), `thumbnail.png`. Open spec in `docs/format.md`. Versioned; readers must tolerate unknown fields.
- `.skp` import via `openskp` (npm, MIT) in the webview: SketchUp 2014-2025. Reading for interoperability.
- `.skp` export via `openskp`'s writer, SketchUp 2017 format, with the writer's bundled SDK-authored blank template REPLACED by `assets/blank-2017.skp`, a blank document the maintainer saved himself from SketchUp Free. A build-time check asserts the template SHA-256 is ours. Pending the attorney read, export ships import-first if counsel objects.
- Never link or call the Trimble SketchUp SDK. Never redistribute OpenSKP's committed fixtures (no stated terms). Never put 3D Warehouse models in the corpus.

## ADR-0003 UI shell (approved mock, the maintainer 2026-10-11: "mock looks great")

- Build exactly the mock: https://(mock link removed). Local copy to be saved at `docs/mock/sawhorse-mock.html`.
- Layout: top bar (menus, view buttons Iso/Top/Front/Right/Persp, Credits chip); left tool rail with SketchUp-2017 default keys (Space select, E erase, L line, R rect, C circle, A arc, P push/pull, M move, Q rotate, S scale, F offset, W follow-me, T tape, D dimension, B paint); viewport; right panel tabs Outliner / Tags / Cut list / Render; bottom Value Control Box (typed length/angle/count mid-gesture; units in, ft-in, fractional in, mm, cm, m).
- Inference colours are OURS: endpoint lime, midpoint teal, on-edge amber, on-face violet. Not SketchUp's.
- Own glyphs, own layout. No copied icons, no pixel-for-pixel UI (Tetris v. Xio).
- Stack: Tauri 2.x (stay off 3.0 alpha), Vite, React, TypeScript, three.js (WebGL2; WebGPU behind a flag), Vitest, Playwright. Browser build is first-class (the zero-install demo).

## ADR-0004 Credits and render service

- No user accounts. The app generates an Ed25519 device keypair stored in the OS secure store; `install_id` = base32(sha256(pubkey))[:20].
- Stripe Checkout (mode=payment, `client_reference_id` = install_id, guest customer) for packs $10/100, $25/250, $50/500. Never a pack under $10.
- Webhook (`checkout.session.completed`, `checkout.session.async_payment_succeeded`) is the source of truth; idempotent fulfilment table keyed by session id; server-side ledger in Postgres (personal managed Postgres). Signed requests (install id, timestamp, nonce, signature); nonce cache; 5-minute skew.
- `/v1/render`: decrement atomically, call fal `flux-control-lora-depth` (depth map from the engine, preprocess off), fall back to Replicate `flux-depth-dev` (clay render), re-credit on failure. Device binding max 2 devices; rebind via emailed claim code; limits 2 concurrent / 6 per minute / ~200 per day.
- Provider keys live only on the server (`~/.sawhorse/.env` locally, the production host in production). The desktop app never holds them.

## Build rules (from CLAUDE.md, applied here)

- Opus builds, Sonnet grounds/refutes, Codex = one pre-launch review.
- Every lane after the scaffold runs in `.worktrees/<slug>` (gitignored + excluded). Never `git worktree remove --force`; follow the junction-safety steps if any junction exists.
- `CARGO_TARGET_DIR=D:\build-cache\sawhorse` (regenerable bulk bytes stay off C:).
- No MSVC Build Tools locally (cellular bandwidth). Native Windows installers are built by GitHub Actions (`tauri-action`). Local = kernel + tests (WSL or Windows), browser build, `tauri dev` only once a linker is available.
- Every lane ends with the feature exercised in the running build and a screenshot in `../_captures/`.
- Clean-room log in `docs/CLEANROOM.md`: Hew (AGPL) and hew3d/openskp (GPL) are design references only; record what was read and when; no code copied.
