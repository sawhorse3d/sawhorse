# ADR-0002: File formats

- Status: Accepted, 2026-10-11 (seed decisions). `.skp` export pending the attorney read.
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
- npm `openskp` (MIT) and hew3d/openskp (GPL) are different things: only the MIT package may ever be a dependency; the GPL repository is a design reference at most and every reading goes in `docs/CLEANROOM.md`.
