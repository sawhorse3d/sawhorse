# Changelog

All notable changes to Sawhorse are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/) once releases start.

## [Unreleased]

### Added (lane 1, foundations, 2026-10-11)

- Browser app shell built from the approved mock: top bar with menus, Iso/Top/Front/Right/Persp views and Credits chip; tool rail with the SketchUp-2017 default keys and our own glyphs; three.js WebGL2 viewport (Z-up, grid, axes, coarse hover inference in our four colours); Outliner / Tags / Cut list / Render panel; Value Control Box with fractional in, decimal in, ft-in, mm, cm and m. Light and dark themes.
- Tauri 2.x desktop scaffold wrapping the same front end (not yet built locally; CI builds installers).
- Rust workspace: `kernel`, `tessellate`, `inference`, `export`, `wasm-api`. Every crate builds for `wasm32-unknown-unknown`.
- Kernel: half-edge B-rep with hole loops and stored planes; named tolerances; topology validator (integrity, loop closure, twins, planarity, winding, self-intersection, hole containment, Euler-Poincare per shell, watertight); sketches with automatic region detection; extrude to a watertight solid; push/pull for faces whose neighbours run parallel to the push; transactional ops with typed errors; deterministic hashing.
- Booleans behind a `BooleanEngine` trait with a manifold-rust adapter that keeps per-face provenance.
- Watertight-only binary STL and 3MF exporters.
- Kernel running as WebAssembly in a Web Worker; Push/Pull and Rect typed values drive it end to end.
- Tests: property invariants, an op-sequence fuzzer with repro files, golden-hash replays checked natively and in WASM, a cargo-fuzz harness, Vitest and Playwright suites.
- CI on GitHub Actions and a tag-triggered, unsigned-by-default release workflow for Windows, macOS and Linux.
- Docs: ADRs 0001-0004, development guide, clean-room log, dual MIT / Apache-2.0 licence.

### Added (lane 2, kernel core, 2026-10-11)

- Sketch tier: strokes insert with the sticky rules (endpoint merge, T-junction splits, crossings split both, collinear overlaps normalised); regions are minimal circuits with holes and keep stable ids across edits; erase heals what a stroke split. Extruding consumes the region.
- Object tier: `split_face` and `imprint` on any face (validated, transactional, user-drawn edges).
- General push/pull on any planar face of a watertight solid: side walls, sliding prism walls, coplanar merge that keeps user-drawn edges, flush welds, collapse, and a boolean path (manifold-rust, re-polygonised into n-gons with holes) for pushing through or into the solid. A global self-intersection check guards every commit.
- Persistent document in the kernel worker: op log, undo/redo (200 steps), groups, tags, delete; Ctrl+Z / Ctrl+Y in the app; Rect draws on the ground or imprints on a face; Push/Pull acts on the clicked face or region. OBJ export.
- Tests: sketch / face / push property tests, 26-case boolean differential (kernel vs npm manifold-3d vs hand-computed, `npm run boolean:diff`), 13 golden logs (WASM == native), an op-sequence fuzzer over every op with undo/redo interleaved (50,000 sequences clean in WSL), lane-end captures in `../_captures/2026-10-11-lane02-*.png`.
- Renamed identifiers that sat close to a copyleft project's names (see `docs/CLEANROOM.md`).
