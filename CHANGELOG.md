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

### Added (lane 4, transforms, 2026-10-11)

- Move (M):
  - Click a solid (or select first), move, click again; or press, drag and let go.
  - The cursor snaps from the start point; arrow keys lock red, green or blue, and Shift holds the line showing.
  - Type a distance mid-move (it goes the way the cursor went) or after it (the move changes).
  - Ctrl copies. After a copy, `x3` (or `3x`, `*3`) makes three copies at one, two and three times the move, and `/4` divides it into quarters. One undo takes the whole array back.
  - With faces, edges or points of one solid selected, Move moves that part. Faces that stay flat follow; one that would bend is refused unless Alt turns autofold on.
- Rotate (Q):
  - The protractor sits on the face under the cursor, or level. Arrow keys lock it upright facing red or green, or level, and Shift keeps its plane.
  - Click the centre, the start of the angle and the end, or type the angle (`90`, `-22.5`, `30deg`).
  - Cursor angles snap to the protractor's 15 degree ticks; typed angles are exact and can be retyped after the turn.
  - Ctrl copies. `x3` and `/4` make arrays the same way Move does.
- Scale (S):
  - Grips sit on the bounding box of the selection: corners, edge middles and face centres. A selected face gets its 8.
  - Each grip scales about the opposite grip; Shift makes the scale uniform and Ctrl scales about the centre.
  - Type a factor (`2`, `0.5`, `-1` mirrors, or `2,1,0.5` for red, green and blue), or an absolute size with a unit (`30in`, `1200mm`).
  - The kernel checks every non-uniform scale for volume sign, flat faces and a closed shell.
  - Scaling a selected face tapers a leg.
- Offset (F):
  - Click a face or a sketch region (or near selected edges), then move inward or outward, or type a distance.
  - Offsets come from `cavalier_contours` with sharp corners. A face with holes is offset as one shape.
  - The result imprints on the solid when it stays on the face, and becomes a new sketch on that plane when it reaches off it (the same way Draw decides). The kernel's own preview shows while the cursor moves.
- Follow Me (W):
  - Select the path (edges end to end, or one face to go round its edge), then click the profile.
  - The profile is swept along straight segments with mitered corners; open and closed paths both work.
  - A sketch region becomes a new solid. A face adds to its solid when the path leaves it, and cuts the solid when the path runs in, so a moulding drawn on a box's end routes round its top.
- Kernel:
  - `Transform`, `Copy`, `Array` and `TransformGeometry` ops: affine, object-level, face ids kept, validated, with typed refusals.
  - A `divided` helper gives the `/n` step of a move, turn, screw or scale.
  - `Offset` with `Doc.offset_preview`, and `FollowMe` with mitered sweeps.
  - Severance: a push or a sweep that cuts through a board leaves one solid per piece, each keeping its face ids.
  - Plane snapping before and after every boolean: nearly coplanar faces share one plane, drifted vertices are re-solved onto it, and coincident vertices are welded. The engine's output is welded at the merge distance.
- Lane-3 verifier conditions:
  - A dev autosave to IndexedDB, with "Restore unsaved work?" (Restore / Discard, also on the Edit menu) after a reload.
  - Status text names solids by display number ("solid 3"), never by raw id.
  - The Rect preview labels each side's length and direction, and its message states them.
  - Per-solid bounding boxes in the bookshelf test.
  - Vite ignores `.worktrees/`, and CI uses `actions/upload-artifact@v5`.
- Tests:
  - Kernel suites for transforms (with the proptest "transform then inverse restores the hash"), offsets, Follow Me (exact volumes), severance and snapping.
  - Goldens `transforms`, `offset` and `follow_me`, where the WASM hash equals the native one.
  - Vitest suites for each new tool, and seven new Playwright specs (`session`, `move`, `rotate`, `scale`, `offset`, `follow`; `vcb` updated).
  - The fuzzer drives every new op. 50,000 sequences are clean, and the boolean refusal rate is measured with plane snapping off and on.
  - A lane-end walk-through, `scripts/capture-lane04.mjs`.

### Changed (lane 4)

- Typing straight after a tool key goes to that tool and is not wiped when the switch lands. Arrow keys and Ctrl / Alt taps wait for clicks still on their way. A render that skipped pending edit updates no longer restores a stale Push target. Fast keyboard use found all three.
- `scripts/env.sh` gives each worktree its own cargo target directory, and `PW_PORT` lets parallel worktrees run Playwright side by side.
- A push that cuts a solid in two now reports `severed` (two solids) instead of leaving a two-shell solid.

### Changed (lane 3)

- CI: `actions/checkout@v5` and `actions/setup-node@v5`. The `rust` and `web` jobs are pinned to `ubuntu-24.04`; `tauri-check` stays on 22.04.
- The coarse lane-1 hover inference was removed; the viewport asks the kernel.

### Fixed (lane 3)

- An edge of the face under the cursor was hidden by that same face at oblique view angles, so a corner could not be snapped there. Found during the bookshelf built by hand.
