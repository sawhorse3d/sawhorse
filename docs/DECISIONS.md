# Sawhorse: architecture decisions (index)

The seed record of 2026-10-11 has been split into one file per decision under `docs/adr/`. This file stays as the index and keeps the build rules.

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/0001-kernel.md) | Geometry kernel: pure-Rust half-edge B-rep with hole loops, sketch + object tiers, compiled to WASM, robustness machinery first | Accepted; lane 1 built the data model, validator, transactions, extrude, push/pull "extend", tests; day-1 spike passed with nothing dropped; lane 2 the kernel core; lane 3 face ids, erase, draw-on-anything, annotations and the inference crate (own BVH); lane 4 transforms, copies, arrays, partial moves, severance, plane snapping, offsets (cavalier_contours) and Follow Me |
| [0002](adr/0002-formats.md) | File formats: open `.shz`, `.skp` via npm `openskp` (MIT), watertight STL/3MF/GLB/DXF/SVG | Accepted; lane 6 built every import and export (STL, 3MF, GLB, OBJ, DXF, SVG), `.skp` import through openskp, the `.shz` format (`docs/format.md`) and the .skp corpus; `.skp` export pending the attorney read and our own blank template |
| [0003](adr/0003-ui-shell.md) | UI shell: exactly the approved mock, SketchUp-2017 keys, our glyphs and inference colours, Tauri 2 + Vite + React + three.js | Accepted; shell built in lane 1; navigation, selection, inference, VCB grammar and the draw / push / tape tools in lane 3; Move, Rotate, Scale, Offset, Follow Me, the dev autosave and friendly names in lane 4; the File menu, drag-and-drop and the import report in lane 6 |
| [0004](adr/0004-credits.md) | Credits and render service: no accounts, device keys, Stripe packs, server-side keys only | Accepted; lane 8 built the app side (device key, signed client, Render pane, passes) and `../render-service` (tested, not deployed); go-live checklist in `docs/LAUNCH-CREDITS.md` |
| [0005](adr/0005-groups-components.md) | Groups, components, tags and scenes: a tree of nodes over world-space solids; definitions as templates, instance solids derived bit for bit; edits captured and re-derived; the editing context as model state, opening and closing it not an undo step | Accepted; lane 5 built the kernel, the scene graph across the worker, context-aware selection and tools, the Outliner, Tags pane and scene tabs, `.shz` and `.skp` mapping |
| [0006](adr/0006-cut-list.md) | The cut list: parts from components (quantities multiplied through nesting), plain solids and solid-only groups; minimal oriented boxes as T x W x L (exact when rectilinear, "shaped" otherwise); grain and stock as part attributes with ops; lumber in board feet, sheet goods in rough 4 x 8 sheets; computed in the kernel, deterministic, hashed | Accepted; lane 7 built the kernel module, the ops, the Cut list pane with grain arrows, CSV, the printable report and PDF, and the label sheet |
| [0007](adr/0007-nesting.md) | Nesting and cutting diagrams: a stock library (sheets and boards, per-material defaults, kerf / trim / algorithm) stored only when edited; guillotine sheets first, maxrects selectable, boards one-dimensional (first-fit-decreasing + lookahead); grain never turned across; cut sequences found from the layout; every part placed or reported; computed in the kernel, deterministic, hashed | Accepted; lane 9 built the kernel modules and ops, the Nesting section and Stock dialog, SVG / DXF / PDF booklet, goldens, property tests and the fuzzer's invariants |
| [0008](adr/0008-materials-styles.md) | Materials, styles and shadows: materials with textures (real size, rotation, offset) and stock links the cut list reads; paint per face or per solid / group / instance (the container's default), following splits and components; per-face texture positions from the face's own frame; a built-in library drawn by our own code; styles (faces, edges, background, sky, ground) and a sun with a shadow map, saved per document and in scenes; hashed only when used | Accepted; lane 10 built the kernel module, the ops, the worker's texture hand-off, the Materials and Styles panes, the Paint tool with sampling and texture handles, the renderer's textures, styles and shadows, material hints for renders, `.skp` materials and textures, GLB / OBJ / SVG exports |
| [0009](adr/0009-plugin-api.md) | Plugins: a folder with plugin.json and an ES-module entry, each in its own Web Worker locked down (no DOM, no network, no importScripts, no eval), talking to the app only through a typed RPC with a watchdog; permissions declared, shown and enforced by the app; model writes as named batches with placeholders applied as one validated op and one undo step; declarative panels; plugin tools in the tool system; API 1 typed by `packages/plugin-api` | Accepted; lane 11 built the package, the sandbox, the host and registry, the Plugins pane and menu, desktop folders, `refbatch` in the kernel, three samples (cabinet generator, shelf pin holes, model report), `plugin:check` and `plugin:pack`, and the fuzzer's plugin batches; amended after the lane's verification: the worker's globals cut to an allowlist, each sandbox under the page's content security policy (the browser build now has one), sources parsed with acorn, grants bound to the files enabled |

New decisions get the next number (`docs/adr/0010-*.md`) with Status, Context, Decision, Consequences.

## Day-1 spike result (2026-10-11, lane 1)

`cargo check --workspace --target wasm32-unknown-unknown` passes with the full candidate set, so nothing was feature-gated or dropped. Details and versions: [ADR-0001](adr/0001-kernel.md#day-1-spike-2026-10-11-passed-nothing-dropped).

| Crate | Version | Used by | wasm32 |
|---|---|---|---|
| glam (f64) | 0.34.1 | all | builds |
| slotmap | 1.1.1 | kernel | builds |
| robust | 1.2.0 | kernel (sketch predicates, point-in-ring) | builds |
| earcut (georust) | 0.4.11 | tessellate | builds |
| i_overlay | 9.0.1 | tessellate::overlay | builds |
| manifold-rust | =0.16.0 (pure Rust, Apache-2.0) | kernel::boolean (feature `manifold`, default on) | builds, `parallel`/rayon left off |
| threemf | 0.8.0 (0BSD; zip 8 + zlib-rs, pure Rust) | export | builds |
| wasm-bindgen | 0.2.129 | wasm-api | builds |
| proptest 1.12 / arbitrary 1.5 | dev / feature `arbitrary` | kernel tests, fuzz harness | dev-only |
| cavalier_contours | 0.9.0 (MIT OR Apache-2.0, added in lane 4) | kernel::offset | builds |

Findings that change how we work (details in `docs/DEVELOPMENT.md`):

- **On Windows without MSVC, `cargo check` itself fails**, not just linking final binaries: build scripts and proc-macros (serde_derive, thiserror, wasm-bindgen-macro) are host executables that need `link.exe`. Git Bash's `/usr/bin/link` shadows the name and fails with "extra operand". So every Rust command (check, test, clippy, fmt, wasm-pack) runs in WSL Ubuntu; `scripts/cargo.sh` and `scripts/build-wasm.mjs` forward automatically.
- The `crates/` workspace excludes `src-tauri/` (it has its own `[workspace]`), so the wasm32 check never sees Tauri.
- `manifold-rust` imports a MeshGL64 with per-triangle `face_id` and returns it through booleans, which is the hook ADR-0001's re-polygonise step needs. Verified by `kernel::boolean` tests (difference / union / intersection volumes and face-id provenance).
- Native and WASM kernels hash every golden op sequence identically (`npm run golden:wasm`; six in lane 1, 13 after lane 2, hash format v3 since lane 3's face ids).

## Build rules (from CLAUDE.md, applied here)

- Opus builds, Sonnet grounds/refutes, Codex = one pre-launch review.
- Every lane after the scaffold runs in `.worktrees/<slug>` (gitignored + excluded). Never `git worktree remove --force`; follow the junction-safety steps if any junction exists.
- `CARGO_TARGET_DIR=D:\build-cache\sawhorse` (regenerable bulk bytes stay off C:). Set by `scripts/env.sh` and the `justfile`; never committed to `.cargo/config.toml`. WSL uses `/mnt/d/build-cache/sawhorse/wsl`.
- No MSVC Build Tools locally (cellular bandwidth). Native Windows installers are built by GitHub Actions (`tauri-action`). Local = kernel + tests (WSL), browser build, `tauri dev` only once a linker is available.
- Every lane ends with the feature exercised in the running build and a screenshot in `../_captures/`.
- Clean-room log in `docs/CLEANROOM.md`: Hew (AGPL) and hew3d/openskp (GPL) are design references only; record what was read and when; no code copied.
