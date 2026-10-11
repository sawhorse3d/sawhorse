# Changelog

All notable changes to Sawhorse are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/) once releases start.

## [Unreleased]

### Added (lane 8, render service and credit store, 2026-10-11)

- Render credits per ADR-0004, with no accounts:
  - The app holds an Ed25519 device key. On the desktop it lives in the OS secure store (`src-tauri/device-key`, `keyring`); in the browser demo it is a non-extractable WebCrypto key in IndexedDB.
  - `install_id = base32(sha256(public key))[:20]`, and every request to the render service is signed.
- The Credits chip shows this device's balance (a dash when the service is not connected) and opens the Render pane.
- The Render pane is live:
  - Presets, the prompt, and **Render · 1 credit**.
  - The depth pass shows faintly while the render runs, then the result.
  - A history gallery.
  - The $10 / $25 / $50 packs open Stripe Checkout in the system browser and the chip follows the payment.
  - **Move credits**: an emailed claim code moves a balance to a new device, two devices at most.
  - "Render service not connected" when the service cannot be reached; nothing is charged or simulated.
- Render passes from the current three.js view (`src/render/passes.ts`), each with our own PNG writer:
  - a 16-bit linear depth map normalised over the model;
  - an edge line render with hidden lines removed;
  - a clay render.
- The render service, in its own repository (`../render-service`, not deployed):
  - Hono on Node 24, Postgres (PGlite locally, node-postgres in production).
  - Stripe Checkout and an idempotent webhook.
  - Atomic credit decrement; fal first, Replicate as the fallback, re-credit on failure.
  - Claim codes over Resend; limits of 2 concurrent, 6 a minute and 200 a day.
  - 56 tests, also run on real Postgres; a Dockerfile, a systemd unit for the production host and a deploy guide.
- Tauri: the `device_public_key`, `device_sign` and `open_external` commands (the last opens only Stripe Checkout and the local dev checkout), plus the opener plugin. The CSP allows the local service.
- Tests:
  - The shared signing vector, checked in TypeScript, Rust and the service.
  - Client, PNG and Render pane unit tests.
  - `e2e/render.spec.ts`, with a fake service inside Playwright that verifies every signature and decodes the real passes.
  - `scripts/capture-credits.mjs`, the full session against the real service, with screenshots.
- Docs: `docs/RENDER.md`, and `docs/LAUNCH-CREDITS.md` (what the maintainer provides before credits go live).

### Changed (lane 8)

- CI `tauri-check` installs `libdbus-1-dev`, which `keyring`'s Secret Service backend needs, and runs the device-key tests, clippy and fmt for `src-tauri`. The release workflow installs `libdbus-1-dev` too.
- `PW_PORT` sets the Playwright preview port (default 4173), so parallel worktrees can run e2e side by side.

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

### Changed (lane 3)

- CI: `actions/checkout@v5` and `actions/setup-node@v5`. The `rust` and `web` jobs are pinned to `ubuntu-24.04`; `tauri-check` stays on 22.04.
- The coarse lane-1 hover inference was removed; the viewport asks the kernel.

### Fixed (lane 3)

- An edge of the face under the cursor was hidden by that same face at oblique view angles, so a corner could not be snapped there. Found during the bookshelf built by hand.
