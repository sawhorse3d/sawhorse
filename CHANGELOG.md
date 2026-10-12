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

### Added (lane 3, feel, 2026-10-11)

- Persistent face ids. They survive push/pull, split, merge and booleans, and ops, view-mesh parts and the model hash (format v3) use them. Golden and repro logs were migrated.
- Navigation:
  - Middle-drag orbits, Shift+middle or right drag pans, and the wheel zooms to the cursor.
  - The Iso / Top / Front / Right / Persp views and a parallel / perspective toggle.
  - Shift+Z zooms to the extents. Auto-fit only ever zooms out.
  - The grid follows the units (1 in and 1 ft, or 10 mm and 100 mm). The negative axes are dashed.
- Selection and erasing:
  - Select: hover highlight; click, Shift adds, Ctrl toggles; window and crossing boxes; double-click selects the solid; Ctrl+A.
  - Delete and the Erase tool go through the kernel. Edges drawn on a face, imprinted faces, sketch regions, guides and dimensions erase. Structural edges are refused with the reason. A multi-erase is one undo step.
- The inference engine, in Rust (`crates/inference`):
  - Our own BVH for picking.
  - Pixel tolerance at depth, and occlusion.
  - Ranked inference kinds: endpoint, midpoint, intersection, centre, on edge, on face, the three axes, parallel and perpendicular, from point, on guide, guide point, and grid.
  - Locks with the arrow keys and Shift. Length snapping to grid steps.
  - Our colours (lime, teal, amber, violet, plus magenta, blue, indigo, grey, tan and sage).
- The Value Control Box has one grammar for every tool:
  - Fractions, ft-in and metric.
  - `w, d` for sizes, `24s` for segments and `12r` for radii.
  - Angles and the array forms are reserved for lane 4.
  - Focus rules, Esc, and Ctrl+Z mid-entry.
- Draw tools, all on sketch planes and on solids' faces:
  - Line, Rectangle, Rotated Rectangle, Circle, Polygon, Arc and 3-Point Arc.
  - A closed shape inside a face imprints.
  - A stroke crossing a face's edges becomes `split_face` cuts, so a dado strip across a board is one step.
  - A shape reaching off a solid starts a new sketch on that plane.
  - A Rectangle started on a corner, an edge or the grid takes its plane from the cursor.
- Push/Pull:
  - Press-drag-release with a live preview, or click-move-click.
  - Typed distances mid-gesture or after the gesture.
  - Double-click repeats the last distance.
  - Ctrl pulls out a new face and leaves a seam.
  - Snaps to other geometry's points along the face normal.
  - Refusals show typed in the status line.
- Tape and Dimension:
  - Tape measures between points, places guide lines parallel to an edge at a typed or clicked offset, and drops guide points.
  - Guides snap, including where they cross edges. Edit > Delete Guides clears them.
  - The Dimension tool adds linear dimensions.
  - Guides and dimensions are kernel annotations, so undo covers them.
- Tests:
  - Rust inference tests.
  - Kernel tests for face ids, erase, draw and annotations.
  - Vitest suites for each tool against a fake host.
  - Eight new Playwright specs.
  - The Phase-2 exit test, `e2e/bookshelf.spec.ts`: a 36 x 72 x 11 1/4 in bookshelf with dados, 8 watertight solids, 2,761.03125 in^3, 0 refusals, about 10 s. It runs alone with `npm run e2e:bookshelf`.
  - The op-sequence fuzzer covers every new op: 50,000 sequences clean, with boolean-path refusals now counted in the report.
- Docs: `docs/USER-GUIDE.md`, a first-ten-minutes tour with every key and the inference colours.

### Added (lane 6, file import/export, 2026-10-11)

- File menu: New, Open, Save, Save As, Import, Import units, and an Export submenu; Ctrl+O, Ctrl+S, Ctrl+Shift+S and Ctrl+I.
  - Drag and drop a file onto the window: a `.shz` opens, anything else is imported.
  - Browsers with the File System Access API use native pickers, and Save writes back to the same file. Other browsers (or `?io=basic`) use a file input and downloads. The desktop app uses Tauri's dialog and fs plugins, limited to the paths the user picked or dropped.
  - An import report lists what became solids, what stayed a mesh and why, and what was healed or skipped.
- Exports, over everything visible or the selected solids:
  - STL (binary, mm, Z up) and 3MF (one named object per solid, mm, colours). Both refuse anything that is not a watertight solid, naming it.
  - glTF binary (Y up, metres, a node per object, face ids as `_FACE_ID`), OBJ (an object and group per part, with a material library), DXF (3D faces and edges, a layer per object).
  - SVG: a hidden-line drawing of the current view (to scale in mm for parallel views), and the outline of a selected face for laser cutting and CNC, also as DXF.
- Imports, each one undo step:
  - STL, OBJ, glTF/GLB, 3MF, DXF (3D faces, lines, polylines with arcs, circles) and SVG (paths and shapes as sketch strokes on the ground).
  - Closed shells become solids, including parts that touch each other. Open, non-manifold or self-intersecting geometry comes in as a visible, amber-tinted mesh with a note; it is never silently repaired.
  - Units for STL, OBJ and unitless DXF: millimetres by default, chosen under File > Import units.
- SketchUp `.skp` import through the npm `openskp` package (MIT), parsed in the kernel worker: groups, each component placement, nested and mirrored placements, materials as colours, layers as tags (hidden ones hidden), hidden objects on a hidden tag, scenes. What is and is not supported: `docs/skp-compat.md`.
- The native `.shz` document, an open format (`docs/format.md`): `scene.json` with exact geometry and persistent face ids, `geometry.glb` for any glTF viewer, and a thumbnail. Opening a saved file gives the same model, hash for hash.
- Materials (colour, texture file name), object names, saved scenes and imported meshes in the kernel; faces drawn in their material colour; the View menu lists saved scenes; Edit > Delete Imported Meshes.
- Tests: a writer and reader test for every format, including round trips through our own readers; every export read by an independent reader (`dxf-parser`, three.js GLTFLoader, fflate); `.shz` round trips of every golden document; a 200-file `.skp` corpus with a golden-image harness and a CI job; `e2e/files.spec.ts` for the menus, export, save, open and drag and drop.

### Changed (lane 6)

- `scripts/env.sh`: a lane working in a git worktree gets its own cargo target directory.
- `playwright.config.ts`: `PW_PORT` picks the preview port, so parallel lanes never reuse another lane's server.
- `serde_json` parses floats exactly (`float_roundtrip`), so saved files read back bit for bit.

### Changed (lane 3)

- CI: `actions/checkout@v5` and `actions/setup-node@v5`. The `rust` and `web` jobs are pinned to `ubuntu-24.04`; `tauri-check` stays on 22.04.
- The coarse lane-1 hover inference was removed; the viewport asks the kernel.

### Fixed (lane 3)

- An edge of the face under the cursor was hidden by that same face at oblique view angles, so a corner could not be snapped there. Found during the bookshelf built by hand.
