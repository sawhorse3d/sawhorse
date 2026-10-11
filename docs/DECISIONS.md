# Sawhorse: architecture decisions (index)

The seed record of 2026-10-11 has been split into one file per decision under `docs/adr/`. This file stays as the index and keeps the build rules. Source of truth for the plan: the vault note `the maintainer's notes`.

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/0001-kernel.md) | Geometry kernel: pure-Rust half-edge B-rep with hole loops, sketch + object tiers, compiled to WASM, robustness machinery first | Accepted; lane 1 built the data model, validator, transactions, extrude, push/pull "extend", tests; day-1 spike passed with nothing dropped; lane 2 the kernel core; lane 3 face ids, erase, draw-on-anything, annotations and the inference crate (own BVH) |
| [0002](adr/0002-formats.md) | File formats: open `.shz`, `.skp` via npm `openskp` (MIT), watertight STL/3MF/GLB/DXF/SVG | Accepted; STL and 3MF exporters exist; `.skp` export pending the attorney read |
| [0003](adr/0003-ui-shell.md) | UI shell: exactly the approved mock, SketchUp-2017 keys, our glyphs and inference colours, Tauri 2 + Vite + React + three.js | Accepted; shell built in lane 1; navigation, selection, inference, VCB grammar and the draw / push / tape tools in lane 3 |
| [0004](adr/0004-credits.md) | Credits and render service: no accounts, device keys, Stripe packs, server-side keys only | Accepted; lane 8 built the app side (device key, signed client, Render pane, passes) and `../render-service` (tested, not deployed); go-live checklist in `docs/LAUNCH-CREDITS.md` |

New decisions get the next number (`docs/adr/0005-*.md`) with Status, Context, Decision, Consequences.

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
