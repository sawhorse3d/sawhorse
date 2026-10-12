# Developing Sawhorse

Three places run the code: the maintainer's Windows machine (no MSVC), WSL Ubuntu on that machine (all Rust work), and GitHub Actions (everything, plus the installers). Commands below are exact; run them from the repo root (`%USERPROFILE%\Desktop\sawhorse\app`, or wherever you cloned it) in Git Bash unless a section says otherwise.

## The two rules that shape everything

1. **No MSVC on the Windows machine.** Visual Studio Build Tools are a multi-gigabyte download on a metered link, so they are not installed. Without `link.exe`, Rust cannot even `cargo check` on Windows: build scripts and proc-macros (serde_derive, thiserror, wasm-bindgen-macro) are host programs that must be linked. Git Bash's `/usr/bin/link` (coreutils) shadows the name and fails with `link: extra operand`. Therefore:
   - every Rust command runs in **WSL Ubuntu**; `scripts/cargo.sh` and `npm run wasm` forward there automatically when they see Windows without MSVC;
   - native Windows, macOS and Linux installers are built by **GitHub Actions** (`.github/workflows/release.yml`, `tauri-action`);
   - `tauri dev` / `tauri build` do not run locally until a linker exists.
2. **Build bytes live on D:.** `CARGO_TARGET_DIR` is `D:/build-cache/sawhorse` on Windows and `/mnt/d/build-cache/sawhorse/wsl` in WSL (separate subdir: Linux host artifacts must not mix with Windows ones). `scripts/env.sh` and the `justfile` set it. Never commit a `target-dir` into `.cargo/config.toml`. CI uses cargo's default (`crates/target`, gitignored).

## Toolchains

| Where | What | Notes |
|---|---|---|
| Windows | Node 24 + npm 11 | front end, Vite, Vitest, Playwright |
| Windows | Rust stable 1.99 at `%USERPROFILE%\.cargo\bin`, `wasm32-unknown-unknown` target | only useful once MSVC exists; `export PATH="$HOME/.cargo/bin:$PATH"` |
| Windows | `just` | optional task runner (`just --list`) |
| WSL Ubuntu 24.04 | rustup stable 1.99 + `wasm32-unknown-unknown` + `rustfmt` + `clippy`, gcc, Node 22 | does all Rust work |
| WSL Ubuntu | `wasm-pack` 0.15.0 in `~/.local/bin` | installed with `npm i -g --prefix ~/.local wasm-pack@0.15.0` |

One-time WSL setup (already done on the maintainer's machine, 2026-10-11):

```bash
wsl.exe bash -lc 'rustup default stable && rustup target add wasm32-unknown-unknown && rustup component add rustfmt clippy'
wsl.exe bash -lc 'npm i -g --prefix ~/.local wasm-pack@0.15.0'
```

## Daily loop on Windows

```bash
npm install                 # once, or after package.json changes
npm run wasm                # builds the kernel to src/wasm/pkg (runs wasm-pack inside WSL)
npm run dev                 # http://localhost:5173 - the browser build, kernel included
```

Without `npm run wasm` the app still starts; the viewport HUD says "Kernel WASM is not built. Run: npm run wasm".

Front-end checks:

```bash
npx tsc -b                  # types
npx vitest run              # unit + component tests (jsdom)
npm run build               # tsc + vite production build into dist/
npm run build:all           # wasm + build (what Tauri's beforeBuildCommand runs)
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test   # e2e on the installed Edge, no browser download
PW_PORT=4184 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test   # the same from a worktree beside another lane (own preview port)
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npm run e2e:bookshelf # the Phase-2 exit test alone (`just bookshelf`)
npm run check:tauri         # fails if Tauri resolved to anything but 2.x
```

`?theme=light` or `?theme=dark` on the URL forces a theme.

Parallel lanes: each `.worktrees/<slug>` checkout gets its own cargo target directory automatically (`scripts/env.sh` appends the slug: `D:/build-cache/sawhorse-<slug>`, `/mnt/d/build-cache/sawhorse/wsl-<slug>`), so lanes never wait on each other's cargo lock. Give each lane its own Vite and preview ports (`npx vite --port 5184`, `PW_PORT=4184`); Playwright reuses an existing server on its port, which would otherwise be another lane's build.

### The e2e suites (lanes 3 and 4)

| Spec | What it drives |
|---|---|
| `e2e/shell.spec.ts` | lane-1/2 shell: Rect + Push/Pull through the kernel, undo/redo |
| `e2e/navigation.spec.ts` | orbit/pan/zoom, standard views, projection toggle, Shift+Z, unit-aware grid |
| `e2e/selection.spec.ts` | click, window vs crossing boxes, double-click, Delete and Erase through the kernel, Ctrl+A |
| `e2e/inference.spec.ts` | our colours and tips (endpoint, midpoint, on-edge, on-face, grid), axis inference, arrow-key locks, exact values from inferred clicks (intersection, guide-point and the rest are hit in `tape` and `bookshelf`; every kind has a Rust test in `crates/inference/tests/infer.rs`) |
| `e2e/vcb.spec.ts` | the typed-value grammar per tool, Esc, Ctrl+Z mid-entry, arrays refused until lane 4 |
| `e2e/draw.spec.ts` | Line, Rect, Rotated Rect, Circle, Polygon, Arc, 3-Point Arc; a stroke across a face becomes `split_face` |
| `e2e/push.spec.ts` | press-drag-release, click-move-click, typed mid-gesture, double-click repeat, Ctrl new face, push-through, typed refusals |
| `e2e/tape.spec.ts` | Tape measure, guides from an edge, guide crossings as snaps, Dimension, erasing a guide |
| `e2e/bookshelf.spec.ts` | the Phase-2 exit test: a 36 x 72 x 11 1/4 in bookshelf with dados, every click gated on the inference it should land on; asserts 8 watertight solids and 2,761.03125 in^3, each solid's bounding box (so a misplaced part fails), and prints its time and the refusal tally |
| `e2e/files.spec.ts` | lane 6: the File menu and Export submenu, STL import, STL and 3MF export and re-import, an open mesh refused by STL export by name, `.shz` save, New and open with the same hash, SVG drag and drop, a `.skp` through openskp |
| `e2e/corpus.spec.ts` | lane 6: all 200 corpus `.skp` files imported and rendered, the 20-file sample diffed against `tests/corpus/golden/` (skips without the corpus unless `REQUIRE_CORPUS=1`) |
| `e2e/session.spec.ts` | lane 4: the dev autosave (restore and discard after a reload, `?autosave=0`), solids named by display number after slot reuse, the Rect preview's direction labels and message |
| `e2e/move.spec.ts` | Move: typed and retyped moves, drag corner to corner, Ctrl copy then `x3` and `/2` with bounds, a face and a corner moved with autofold off and on |
| `e2e/rotate.spec.ts` | Rotate: a locked protractor, a quarter turn and a retyped angle, a ring of four copies about an off-solid centre, a top face tilted into a wedge (exact volume), a twist refused |
| `e2e/scale.spec.ts` | Scale: grips, a face grip with a factor, an absolute size, a mirror; a tapered leg (frustum volume) |
| `e2e/offset.spec.ts` | Offset: a face inward (imprint, tenon pulled up), outward (sketch), a sketch region, a selected edge offset across a face |
| `e2e/follow.spec.ts` | Follow Me: a chamfer routed round a box top through the UI, checked against the exact removed volume |

Shared helpers for the lane-4 specs live in `e2e/helpers.ts`. `hover` re-reads the screen position and nudges the cursor until the expected inference shows, because the view refits after an edit. `solids()` reads every solid's display number, bounds and volume from `window.__sawhorse.solids()`.

The specs drive the page through read-only hooks on `window.__sawhorse`: `worldToScreen(p)` (metres to canvas pixels), `visibleEndpoints()`, `parts()`, `partScreenCenter(part)`, `camera()`, `grid()`, `pick(x, y)`, `pixel(x, y)`, and `refusals` (kernel refusal messages and how often each came back this page load). The viewport mirrors its state in `data-*` attributes on `main.vp`: `data-kernel`, `data-view`, `data-projection`, `data-inference` (the kind under the cursor), `data-objects`, `data-volume` (m^3), `data-watertight`, `data-selection`, `data-hover`. A spec waits on those attributes rather than on timers.

Lane-end capture (every lane ends with one, saved beside the repo in `../_captures/`), with `npm run dev` running:

```bash
node scripts/capture.mjs --out ../_captures/2026-10-11-lane01-shell.png --theme light --tool p --type "3/4" --hover endpoint
```

It drives the installed Edge through Playwright, waits for the kernel, applies the typed value, hovers a visible corner (via the read-only `window.__sawhorse.visibleEndpoints()` hook) and prints the kernel state it captured.

Lane 4's walk-through, `scripts/capture-lane04.mjs`, drives a running dev server like a person at the keyboard and mouse. Each click waits for its inference. It saves a screenshot after each stage and prints every status line and each solid's volume. The stages are a tapered leg with a tenon (Scale, Rotate, Offset, Push/Pull), four copies with a Move array, one leg turned 90 degrees, and a moulding routed round a box with Follow Me.

```bash
npx vite --port 5184 --strictPort        # in another terminal
node scripts/capture-lane04.mjs --url http://localhost:5184 --out ../_captures
```

### Files, formats and the .skp corpus (lane 6)

| What | Where |
|---|---|
| Exporters (STL, 3MF, GLB, OBJ, DXF, SVG) | `crates/export` |
| Importers (STL, OBJ, glTF/GLB, 3MF, DXF, SVG) | `crates/import` |
| How imported geometry becomes solids (or noted meshes) | `crates/kernel/src/meshbuild.rs` |
| The `.shz` format | `crates/shz`, `crates/kernel/src/snapshot.rs`, spec in `docs/format.md` |
| `.skp` import (openskp, in the kernel worker) | `src/io/skp.ts`, support table in `docs/skp-compat.md` |
| Opening and saving files in each host | `src/io/files.ts` (Tauri plugins, File System Access API, input and download) |

```bash
npx vitest run src/io                     # every export read by an independent reader; .skp mapping (needs npm run wasm)
npm run corpus:skp                        # write the 200-file .skp corpus to tests/corpus/skp/ and check its manifest
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npm run e2e:corpus   # import and render all 200; diff the 20-file sample
UPDATE_GOLDEN=1 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npm run e2e:corpus   # after an intended rendering change
node scripts/capture-lane06.mjs --url http://localhost:5176 --out ../_captures --files ../_scratch/2026-10-11-lane06
```

`?io=basic` on the URL forces the file input and downloads instead of the native pickers (the e2e specs use it: Playwright cannot drive native dialogs). `e2e/files.spec.ts` covers the File menu, STL/3MF export and re-import, an open mesh refused by STL export, `.shz` save and open, and drag and drop. See `tests/corpus/README.md` for the corpus and its golden images.

## Rust (from Git Bash on Windows, forwarded to WSL)

`scripts/cargo.sh <args>` runs `cargo <args>` in `crates/`, inside WSL when needed (it also forwards `UPDATE_GOLDEN`, `FUZZ_CASES`, `PROPTEST_CASES`, `RUST_BACKTRACE`, `RUSTFLAGS`).

```bash
bash scripts/cargo.sh check --workspace --target wasm32-unknown-unknown   # day-1 spike: everything builds for the browser
bash scripts/cargo.sh test --workspace                                     # all tests (debug: validator after every mutation)
bash scripts/cargo.sh clippy --workspace --all-targets -- -D warnings
bash scripts/cargo.sh fmt --all                                            # rustfmt (not installed on Windows)
FUZZ_CASES=50000 FUZZ_SEED=20261011 FUZZ_REPORT=1 bash scripts/cargo.sh test -p sawhorse-kernel --test sequences -- --ignored long_fuzz --nocapture
UPDATE_GOLDEN=1 bash scripts/cargo.sh test -p sawhorse-kernel --test golden # accept an intended hash change; review the diff
npm run golden:wasm                                                        # after npm run wasm: WASM hashes == native goldens
npm run boolean:diff                                                       # after npm run wasm: 26 boolean cases, kernel == npm manifold-3d == hand-computed
```

The same through `just`: `just check-wasm`, `just test`, `just clippy`, `just fmt`, `just fuzz-long`, `just golden-update`, `just wasm`, `just e2e`, `just ci`.

## Rust directly inside WSL

```bash
wsl.exe
cd /mnt/c/Users/<you>/Desktop/sawhorse/app
. scripts/env.sh            # cargo + ~/.local/bin on PATH, CARGO_TARGET_DIR=/mnt/d/build-cache/sawhorse/wsl
cd crates
cargo test --workspace
cargo check --workspace --target wasm32-unknown-unknown
cd .. && wasm-pack build crates/wasm-api --target web --out-dir ../../src/wasm/pkg --out-name sawhorse_wasm --no-pack --release
```

## Test machinery (ADR-0001 "robustness first")

| What | Where | Runs |
|---|---|---|
| Validator after every B-rep and sketch mutation | `Brep::debug_validate`, `Sketch::check` | every debug build (tests) |
| Validate before commit | `Model::apply` (clone, mutate, validate, commit) | always |
| Global self-intersection (edges through faces, chords, vertex/edge contact, coplanar overlap) and positive volume | `kernel/src/intersect.rs`, `Object::validate` | every commit |
| Property invariants | `crates/kernel/tests/invariants.rs` (extrude, push/pull inverse), `sketch_props.rs` (insertion order independence, simple region boundaries, split-then-erase identity), `face_props.rs` (split/imprint then merge identity, transactional refusals), `push.rs` (every push/pull path, volume/area/Euler) | `cargo test` |
| Boolean differential | `crates/kernel/tests/booleans/cases.json` (26 cases) via `tests/booleans.rs` (hand-computed volume, area, shells, genus, faces) and `scripts/boolean-diff.mjs` (same cases, WASM kernel vs npm `manifold-3d` on identical operand meshes) | `cargo test`, `npm run boolean:diff` (CI) |
| Op-sequence fuzzer | `crates/kernel/tests/sequences.rs`: op intents resolved against the live document (strokes, rectangles, polylines, erase, extrude, push/pull incl. push-through, split_face, imprint, delete, groups, tags, and since lane 4 transforms, copies, arrays, partial moves, offsets and Follow Me; undo/redo interleaved); every solid must be one piece; every op checked for validity, typed refusals, unchanged state on refusal, undo/redo round trip, log replay and determinism (`quick_fuzz` 192 sequences; `long_fuzz` ignored, seeded) | `cargo test`; CI runs 10,000 |
| Repro replay | `crates/kernel/tests/repro/*.json` via `tests/repro.rs` | `cargo test` |
| Transforms (lane 4) | `crates/kernel/tests/transforms.rs`: object-level moves, turns, scales and mirrors keep face ids and drawn edges; copies and arrays; moving part of a solid with autofold off and on; severance; proptest "transform then inverse restores the hash"; `transform.rs` unit tests (`Xform` algebra, `divided` steps compose back) | `cargo test` |
| Offset and Follow Me (lane 4) | `crates/kernel/tests/offset.rs` and `follow_me.rs` (exact areas and volumes: Pappus for mitered sweeps, the moulding round a box top at either end); `offset.rs`, `sweep.rs`, `snap.rs` and `sever.rs` unit tests | `cargo test` |
| Inference | `crates/inference/tests/infer.rs` (every inference kind, ranks, pixel tolerance at depth, occlusion incl. oblique views of the face under the cursor, locks, length snapping, from-point memory, guides) and the TS tool tests in `src/tools/*.test.ts` (each tool against a fake host) | `cargo test`, `npx vitest run` |
| Golden hashes | `crates/kernel/tests/golden/*.json` (document op logs, undo/redo included) via `tests/golden.rs` and `scripts/golden-wasm.mjs` | `cargo test`, `npm run golden:wasm` |
| cargo-fuzz | `crates/kernel/fuzz` (own workspace, nightly + libFuzzer) | `cd crates/kernel && cargo +nightly fuzz run op_sequence` (Linux/WSL) |

When the fuzzer fails it writes the shrunk sequence to `crates/kernel/tests/repro/pending-<test>.json` (a panic while resolving intents is captured too). Rename it, fix the kernel, commit both; see `crates/kernel/tests/repro/README.md`.

Fuzzer knobs (all forwarded into WSL by `scripts/cargo.sh`): `FUZZ_CASES` (count), `FUZZ_SEED` (deterministic shard seed; the repro is then `pending-long_fuzz_seed<N>.json`), `FUZZ_REPORT=1` (per-op accepted/refused table, the top refusal reasons, both boolean-path refusal counts and the refusal rate per accepted push/pull and sweep), `FUZZ_PLANE_SNAP=0` (turn the boolean plane-snapping pass off for the run: the same seeds then measure what it buys), `FUZZ_REASON_LEN=200` (longer refusal reasons in the report, the validator's own words), `FUZZ_DUMP_REFUSALS=1` (write the first 30 boolean refusals as replayable logs to `crates/kernel/tests/repro/refusals/`, gitignored, to study what the engine left behind).

The eight-shard run is a script since lane 4 (Git Bash; it runs everything inside WSL):

```bash
MSYS_NO_PATHCONV=1 wsl.exe bash /mnt/c/Users/<you>/Desktop/sawhorse/app/scripts/fuzz-shards.sh 6250 20261100 1   # cases per shard, seed base, plane snapping 1|0
```

The same by hand, e.g. eight of 6,250:

```bash
wsl.exe bash -lc 'cd /mnt/c/Users/<you>/Desktop/sawhorse/app/crates && . ../scripts/env.sh && for s in 1 2 3 4 5 6 7 8; do FUZZ_CASES=6250 FUZZ_SEED=$s cargo test -p sawhorse-kernel --test sequences -- --ignored long_fuzz --nocapture > /tmp/fuzz$s.log 2>&1 & done; wait; grep -h long_fuzz_seed /tmp/fuzz*.log'
```

Run that from PowerShell or cmd. From Git Bash, put the loop in a script file and run `MSYS_NO_PATHCONV=1 wsl.exe bash /mnt/c/.../fuzz8.sh`: Git Bash rewrites `/mnt/...` arguments into Windows paths and can mangle `$s` in an inline `bash -lc` string, which silently runs all eight shards on one seed into one log (seen in lane 3). With `FUZZ_REPORT=1` the report also prints `boolean-path refusals (not a valid solid): N`, the typed refusal the lane reports quote.

### Fuzz record

| Date | Run | Result |
|---|---|---|
| 2026-10-11 (lane 4) | `scripts/fuzz-shards.sh 6250 20261100`, eight shards of 6,250 (seeds 20261101..20261108), debug build, WSL Ubuntu 24.04, parallel; the fuzzer now also drives transforms, copies, arrays, moving part of a solid (autofold on and off), offsets (faces, regions, edge chains) and Follow Me (open, L and closed paths), and checks that every solid is one piece (severance) | **50,000 sequences clean** in 91 s wall: 695,256 ops accepted, 630,713 refused (typed), 52,297 solids at the end. Accepted lane-4 ops: transform 15,506; copy 10,291; array 4,163; transform_geometry 4,878; offset 8,952; follow_me 5,726 (push_pull 43,172, push_pull_new_face 13,975). Six finds fixed on the way, kept in `tests/repro/`: an inward offset whose grown hole cut the shrunk outline left a spike where the closing arc was mitered (`2026-10-11-offset-closing-arc-miter-spike-1..5`), and an offset passing exactly through a vertex tripped a `cavalier_contours` debug assertion (`2026-10-11-offset-through-a-vertex-exactly`; tests now run that crate without debug assertions, as release and the browser do). **Boolean refusal rate, same seeds, plane snapping off then on**: "not a valid solid" 1,677 then 1,563 (-6.8%); with the "degenerate engine output" refusals, 1,759 then 1,646 (-6.4%); per accepted push/pull or sweep 2.80% then 2.62% (push/pull alone 2.55% then 2.38%). Higher than lane 3's 0.5% because transformed (turned, scaled) solids put many more faces on tilted planes; the rest is mostly zero-thickness fins where a pushed face's swept side wall lies, up to rounding, in the plane of a tilted outer wall (see `docs/adr/0001-kernel.md`). |
| 2026-10-11 (lane 3) | eight shards of `FUZZ_CASES=6250`, `FUZZ_SEED=20261012..20261019`, debug build, WSL Ubuntu 24.04, parallel | **50,000 sequences clean** in 64 s wall: 720,677 ops accepted, 604,336 refused (typed), 32,268 solids at the end. The fuzzer now also drives lane 3's ops: draw (29,858 accepted), push_pull_new_face (14,938), add_guide / erase_guide / delete_guides, add_dimension / erase_dimension, erase_edge (2,022), erase_face (2,439), erase_region (9,247), batch (4,247). Boolean-path refusals (`not a valid solid`, near-coincident geometry on tilted planes): 291 of 61,102 accepted push/pulls, about 0.5%, unchanged from lane 2. |
| 2026-10-11 (lane 2) | `FUZZ_CASES=50000 FUZZ_SEED=20261011`, debug build (validator after every mutation), WSL Ubuntu 24.04, one process | **50,000 sequences clean** in 266.7 s: 803,893 ops accepted, 517,228 refused (typed), 40,677 solids at the end. Accepted per kind: add_rectangle 147,426; insert_segment 118,657; undo 90,323; extrude 79,612; push_pull 66,949; split_face 48,214; imprint 37,363; set_tag_visible 30,960; erase_segment 29,362; new_sketch 26,676; add_polyline 25,785; set_tag 16,537; delete_object 16,408; create_group 10,951; redo 8,670. |
| 2026-10-11 (lane 2) | `FUZZ_CASES=20000 FUZZ_SEED=7`, second shard after the fixes | **20,000 sequences clean** in 111.0 s: 322,825 ops accepted, 207,562 refused. 128 push/pulls were typed boolean-path refusals (`the result is not a valid solid`: near-coincident geometry on tilted planes), about 0.5% of accepted push/pulls. |
| 2026-10-11 (lane 2) | seed 20261011, earlier attempts | Five kernel bugs found and fixed before the clean run, each kept as a repro in `crates/kernel/tests/repro/`: an inside-out solid accepted from the topological push path (`2026-10-11-push-inverts-thin-wedge`); duplicate sketch edges when two crossings merged next to a shared vertex (`-sketch-near-coincident-crossings`); a zero-thickness fin from a boolean on a split vertex 1e-17 m off its plane (`-split-vertex-off-plane-fin`); a cut grazing a hole corner by 1e-16 m (`-split-grazes-hole-corner`); a crossing landing within the merge distance of an unrelated short edge (`-sketch-crossing-cluster`). |


## Desktop (Tauri 2)

`src-tauri/` is configured (`tauri.conf.json`, capabilities, icons from `src-tauri/app-icon.svg`) but not built locally. CI's `tauri-check` job compiles it on Ubuntu; tagging `v*` runs `release.yml`, which builds Windows, macOS and Linux installers into a draft prerelease, unsigned unless signing secrets exist. Once a machine has a linker: `npm run tauri dev`.

Stay on Tauri 2.x stable. 3.0 is alpha; `npm run check:tauri` fails on anything else.

## CI (`.github/workflows/ci.yml`)

- `rust` (ubuntu-24.04): `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, 10,000-sequence fuzz, wasm32 check.
- `web` (ubuntu-24.04): `npm ci`, Tauri guard, `wasm-pack` build, golden cross-check, `npm run build`, Vitest, Playwright Chromium e2e with `REQUIRE_KERNEL=1`.
- `corpus` (ubuntu-24.04): wasm-pack build, `npm run build`, `npm run corpus:skp` (regenerates the 200 `.skp` files and fails on any byte difference from `tests/corpus/manifest.json`), then `npm run e2e:corpus` on Chromium with `REQUIRE_CORPUS=1`; the renders are uploaded as the `corpus-renders` artifact.
- `tauri-check` (ubuntu-22.04, kept on the older glibc the Linux release build targets): `cargo check --locked` in `src-tauri` with webkit2gtk. Lane 6 added `tauri-plugin-dialog` and `tauri-plugin-fs` (lockfile resolved in WSL; not compiled locally, which needs webkit2gtk).

Runners are pinned (never `ubuntu-latest`, which moves to a new Ubuntu on 2026-10-19 without notice in the logs); actions are `actions/checkout@v5` and `actions/setup-node@v5` (Node 24 runtime).

No secrets are needed; no auto-merge is configured.

## Troubleshooting

- `link: extra operand ... rcgu.o` on Windows: that is Git's coreutils `link`, not MSVC. Use `scripts/cargo.sh` (WSL).
- `Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)` from a Node script on Windows: a Node 24 teardown bug when calling `process.exit()`; our scripts set `process.exitCode` instead.
- Viewport says WebGL2 is unavailable: hardware acceleration is off in that browser.
- `cargo` missing in WSL: `wsl.exe bash -lc 'rustup default stable'`.
