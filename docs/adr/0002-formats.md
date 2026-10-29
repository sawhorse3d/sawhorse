# ADR-0002: File formats

- Status: Accepted, 2026-10-11 (seed decisions). Implemented in lane 6 (all imports and exports, `.shz`, `.skp` import). `.skp` export pending the attorney read and our own blank template.
- Deciders: the maintainer
- Source: `docs/DECISIONS.md` seed

## Context

Woodworkers and makers already have SketchUp files and need printable, cuttable output. Our own format has to be open and future-proof; interoperability with `.skp` has to stay clean of the Trimble SDK and of material with unclear terms.

## Decision

- Native format is `.shz`: a zip containing `scene.json` (objects, groups, components, tags, scenes, materials, cut-list metadata, units), `geometry.glb` (or per-object glTF), `assets/` (textures), `thumbnail.png`. Open spec in `docs/format.md`. Versioned; readers must tolerate unknown fields.
- `.skp` import via `openskp` (npm, MIT) in the webview: SketchUp 2014-2025. Reading for interoperability.
- `.skp` export via `openskp`'s writer, SketchUp 2017 format, with the writer's bundled SDK-authored blank template REPLACED by `assets/blank-2017.skp`, a blank document the maintainer saved himself from SketchUp Free. A build-time check asserts the template SHA-256 is ours. Pending the attorney read, export ships import-first if counsel objects.
- Never link or call the Trimble SketchUp SDK. Never redistribute OpenSKP's committed fixtures (no stated terms). Never put 3D Warehouse models in the corpus.

## Consequences and implementation status

- Lane 1 ships the first two exporters in `crates/export`, both refusing non-watertight input with a typed error: hand-written binary STL (millimetres, Z-up) and 3MF through `threemf` (millimetres). `Scene.export_stl` exposes STL to the webview. Not yet wired to a menu.
- The kernel's op log (`Op` JSON, `crates/kernel/src/model.rs`) is the first stable on-disk shape: fuzzer repros and golden replays use it. `.shz` and `docs/format.md` arrive with the file lane.
- Lane 6 (2026-10-11) built the file lane:
  - Exports over the whole document or a selection (`crates/export`): STL and 3MF refuse anything that is not a watertight solid, naming it; 3MF is a hand-written OPC package (one named object per solid, base-material colours) checked against the core spec basics; GLB (Y up, metres, a node per object, `_FACE_ID`), OBJ (`o` + `g` per object, MTL), DXF (R12 `3DFACE` + `LINE`; `LWPOLYLINE` or R12 outline of a face), SVG (hidden-line view, to scale for parallel views; face outline in mm). Every writer is read back by an independent reader and by our own importer in tests.
  - Imports (`crates/import`): STL, OBJ, glTF/GLB, 3MF, DXF, SVG, all into one kernel `import` op; the kernel builds solids through `meshbuild.rs` (weld, clean, separate touching solids, orient, merge coplanar triangles, validate) and keeps anything else as a visible non-solid mesh with a note, never silently repaired.
  - `.skp` import through npm `openskp` (1.3.0; 1.4.0 since lane 25) in the kernel worker (`src/io/skp.ts`); what comes across is in `docs/skp-compat.md`. No `.skp` export in this lane.
  - `.shz` save and open (`crates/shz`, `crates/kernel/src/snapshot.rs`) per `docs/format.md`: `mimetype`, `scene.json` (exact f64 B-reps with persistent face ids), `geometry.glb`, `thumbnail.png`, `assets/`. Save then open reproduces the model hash.
  - The `.skp` test corpus (200 files) is generated with openskp's writer from our own procedural models and is not committed, because that writer starts every file from an embedded SketchUp-saved blank scaffold; only the generator, a SHA-256 manifest and 20 reference renders are committed (`tests/corpus/README.md`). The same scaffold is why `.skp` export still needs `assets/blank-2017.skp` (the maintainer's own blank) before it ships: openskp's writer patches fixed byte offsets of its scaffold, so swapping the template needs work in or around that writer.
- npm `openskp` (MIT) and hew3d/openskp (GPL) are different things: only the MIT package may ever be a dependency; the GPL repository is a design reference at most and every reading goes in `docs/CLEANROOM.md`.
