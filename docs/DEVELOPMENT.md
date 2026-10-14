# Developing Sawhorse

Three places run the code: the maintainer's Windows machine (no MSVC), WSL Ubuntu on that machine (all Rust work), and GitHub Actions (everything, plus the installers). Commands below are exact; run them from the repo root (`%USERPROFILE%\Desktop\sawhorse\app`, or wherever you cloned it) in Git Bash unless a section says otherwise.

## The two rules that shape everything

1. **No MSVC on the Windows machine.** Visual Studio Build Tools are a multi-gigabyte download on a metered link, so they are not installed. Without `link.exe`, Rust cannot even `cargo check` on Windows: build scripts and proc-macros (serde_derive, thiserror, wasm-bindgen-macro) are host programs that must be linked. Git Bash's `/usr/bin/link` (coreutils) shadows the name and fails with `link: extra operand`. Therefore:
   - every Rust command runs in **WSL Ubuntu**; `scripts/cargo.sh` and `npm run wasm` forward there automatically when they see Windows without MSVC;
   - native Windows, macOS and Linux installers are built by **GitHub Actions** (`.github/workflows/release.yml`, `tauri-action`);
   - `tauri dev` / `tauri build` do not run locally until a linker exists.
2. **Build bytes live on D:, one target dir per checkout.** `CARGO_TARGET_DIR` is `D:/build-cache/sawhorse-<folder>-<sum>` on Windows and `/mnt/d/build-cache/sawhorse/wsl-<folder>-<sum>` in WSL (separate subdirs: Linux host artifacts must not mix with Windows ones), where `<folder>` is the checkout's folder (a worktree's slug) and `<sum>` the `cksum` of its full path. `scripts/env.sh` sets it (the `justfile` asks it). Two checkouts must never share one: cargo copies the wasm kernel (`sawhorse_wasm.wasm`, an unhashed name) out of `deps/` only when it compiles, so a checkout cargo thinks is fresh gets whatever kernel the other one copied out last. Worse, cargo names workspace crates' artifacts without their checkout's path, so two checkouts overwrite each other's compiled crates too (reproduced while fixing this: a checkout's wasm-api compiled against the other checkout's older `sawhorse-inference`). On 2026-10-12 that handed a clone a kernel without the section ops: `npm run wasm` said "Finished" in two seconds and `golden:wasm` failed with "unknown variant `add_section`". `npm run wasm` now checks the kernel's `build_checkout()` stamp (a hash of the directory it was compiled from) against the checkout (`scripts/wasm-checkout.mjs`; `npm run check:wasm` runs it alone), rebuilds once by itself when it is another checkout's, and `npm run golden:wasm` and `npm run build` refuse such a kernel. Old shared dirs (`D:/build-cache/sawhorse`, `.../wsl`, `.../wsl-<slug>`) are regenerable and can be deleted. Never commit a `target-dir` into `.cargo/config.toml`. CI uses cargo's default (`crates/target`, gitignored).

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

Parallel lanes: each checkout (each `.worktrees/<slug>`, each clone) gets its own cargo target directory automatically (`scripts/env.sh`: `D:/build-cache/sawhorse-<slug>-<sum>`, `/mnt/d/build-cache/sawhorse/wsl-<slug>-<sum>`), so lanes never wait on each other's cargo lock or pick up each other's kernel. The first build in a new checkout is a clean one (about two minutes for the wasm kernel). Give each lane its own Vite and preview ports (`npx vite --port 5184`, `PW_PORT=4184`); Playwright reuses an existing server on its port, which would otherwise be another lane's build.

### Render service (lane 8)

The Render pane talks to the render service (`../render-service`, its own repository) at the address saved in Edit > Settings, or in a development build at `VITE_RENDER_SERVICE_URL` (lane 15: the Credits chip and the Render pane show only when a service is set there and answers `GET /healthz` with its own JSON, `{"service": "sawhorse-render", "version": ...}`, since lane 20; `src/credits/gate.ts`). Since lane 12c a development build has no default address: with neither set, nothing is asked (no refused connection in the console), and one start asks once. For the local service run `VITE_RENDER_SERVICE_URL=http://127.0.0.1:8787 npx vite` (or set that address in Settings) and start it with `cd ../render-service && npm ci && npm run dev`. With no keys it uses a mock provider, a simulated checkout and an embedded Postgres. Without it, the pane says "Render service not connected" and the chip shows a dash. The full local loop, the device key and what is sent are in `docs/RENDER.md`. Device-key tests run in WSL: `cd src-tauri && cargo test -p sawhorse-device-key` (WSL needs `libdbus-1-dev` and `pkg-config`).

### The e2e suites (lanes 3, 4, 5, 6, 7 and 8)

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
| `e2e/model-cap.spec.ts` | lanes 16 and 16b: a plugin's two array ops to the 100,000-solid limit, every pane and select-all at that size within frame-gap bounds calibrated to the machine (`e2e/perf.ts`, below), the highlight drawn from its state textures; texture decode limits (F10) |
| `e2e/model-counts.spec.ts` | lane 16b: `.shz` files listing 2,000,000 guides or dimensions, 1,000,000 tags or materials, each refused by that count as `scene.json` inflates, timed in the page, with the kernel's memory and the page's heap measured before and after; a file at exactly a limit opens |
| `e2e/private-fixtures.spec.ts` | real SketchUp-authored `.skp` files that are never in this repo, through File > Import: three specific files (a deliberately defective box with a slot, a 24-sided slab with a "Leg" component, an empty model) asserting what the importer does today against the file's own numbers, and every `.skp` under the directory (recursive) imported with a generic check and one row of counts each. Skips unless `SAWHORSE_PRIVATE_FIXTURES` names a directory (below) |
| `e2e/skp-ccby.spec.ts` | lane 15: the five CC BY SketchUp fixtures (`tests/fixtures/skp-ccby/`, hashes checked): each imports with no dialog or error and its parts are in the Cut list, flagged where open; benches.skp through Make parts from loose faces |
| `e2e/edges.spec.ts` | lane 15: the Eraser's Ctrl (soften and smooth), Shift (hide) and Ctrl+Shift (unsoften) on a box edge, one undo step each, nothing erased; Soften selected on a 24-sided cylinder (24 soft edges, smooth normals, one undo step); the soft-edges style switch; the golden image of the softened cylinder (`tests/corpus/golden/softened-cylinder.png`) |
| `e2e/render.spec.ts` | lane 8: Credits chip and Render pane against a fake render service that verifies every signature; not connected, a render with the real 16-bit depth, edge and clay passes, a pack opening checkout and the chip following the payment |
| `e2e/session.spec.ts` | lane 4: the dev autosave (restore and discard after a reload, `?autosave=0`), solids named by display number after slot reuse, the Rect preview's direction labels and message |
| `e2e/move.spec.ts` | Move: typed and retyped moves, drag corner to corner, Ctrl copy then `x3` and `/2` with bounds, a face and a corner moved with autofold off and on |
| `e2e/rotate.spec.ts` | Rotate: a locked protractor, a quarter turn and a retyped angle, a ring of four copies about an off-solid centre, a top face tilted into a wedge (exact volume), a twist refused |
| `e2e/scale.spec.ts` | Scale: grips, a face grip with a factor, an absolute size, a mirror; a tapered leg (frustum volume) |
| `e2e/offset.spec.ts` | Offset: a face inward (imprint, tenon pulled up), outward (sketch), a sketch region, a selected edge offset across a face |
| `e2e/follow.spec.ts` | Follow Me: a chamfer routed round a box top through the UI, checked against the exact removed volume |
| `e2e/cutlist.spec.ts` | lane 7: the user guide's bookshelf built from components through the UI (Back panel, Side x2, Top, Toe kick, Shelf x4; every part's bounds checked), then the Cut list pane: every row and both tiles (18.99 board feet, 1 sheet), stock and grain from the right-click menu, a row selecting its pieces, CSV / PDF / labels saved and read back, Print with the print stylesheet, a hidden shelf with Visible only, 0.5 mm rounding; and the empty state |
| `e2e/plugins-sandbox.spec.ts` | lane 11: the sandbox in a real worker: no DOM, `window`, `fetch`, XHR, WebSocket, IndexedDB, `importScripts`, `postMessage`, `FontFace`, `fonts`, the webkit file systems, storage buckets, locks, `URL.createObjectURL`, `MessageChannel`, and it runs from `blob:`; `eval`, `Function` and string timers throw; no model change without `model.write`, no network without `net`; dynamic `import()` and outside imports refused before the plugin runs, including the verifier's regex bypasses; the verifier's `FontFace` exfiltration fails with nothing reaching a live listener; a raw `blob:` worker under the page's policy cannot load an outside font or module or a `data:` module; a command spinning forever reported (with Stop) and stopped while a box is drawn; a plugin throwing while it loads |
| `e2e/csp.spec.ts` | lane 11: the browser build's content security policy is there and the app (every pane, the three samples running, library swatches) raises no violation under it |
| `e2e/plugins-model.spec.ts` | lane 11: model reads in the API's shapes, the selection both ways, a batch with placeholders as one step named "Plugin box" in the Edit menu, undone and redone |
| `e2e/plugins-ui.spec.ts` | lane 11: Install from .zip through the picker, the permission dialog, every panel field, Enter and a ticked box giving tool keys back to the model, prompt and confirm, the Plugins menu and a shortcut, a plugin tool and the VCB, the console, Reload, installs surviving a reload, Disable, Uninstall, a broken zip, a session load |
| `e2e/plugins-samples.spec.ts` | lane 11: the samples off with their permissions; the cabinet generator (30 x 72 x 12 in, 3 shelves: the Outliner's names, the Cut list's sizes, one undo; Update in place); shelf pin holes (a column of exact volume, one undo); the model report (counts against the model, Markdown and CSV); a plugin claiming the enabled Model report's id stays off, with the dialog naming the sample it replaces |
| `e2e/sections.spec.ts` | lane 12: the bookshelf opened as a `.shz`, cut down the middle with the Section Plane tool (locked square to red): the left half gone, the cut filled and outlined; reversed from the Outliner; the Styles pane's Section cuts / planes / fill switches; a scene bringing the active plane back; Create Group from Slice; the slice exported as SVG; the plane deleted |
| `e2e/sheets.spec.ts` | lane 12: the sheet golden laid out on the canvas exactly as the kernel draws it; a sheet of the bookshelf built with every tool (viewport, dimension, label, table, line, rectangle), moved, resized, deleted and undone, the paper and title block changed, saved as SVG; the golden out as PDF (its dimension strings are text in it), DXF, PNG and print |
| `e2e/groups.spec.ts` | lane 5: Ctrl+G, a click selecting the group, Push/Pull refusing a closed group's face, double-click to open, Esc to close, undo returning inside, Ctrl+Shift+G; Make Component (dialog), a Ctrl+Move copy as an instance, an edit inside one reaching both, Make Unique from the Outliner; tags (New tag, assign, hide), scene tabs (add, switch, rename), the Outliner's eye and lock |

**Private fixtures.** `e2e/private-fixtures.spec.ts` reads real `.skp` files from the directory named by the environment variable `SAWHORSE_PRIVATE_FIXTURES` (for example `SAWHORSE_PRIVATE_FIXTURES=<your private dir> PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test e2e/private-fixtures.spec.ts`). The files are authored in SketchUp itself, are not redistributable and must never be copied into this repository or committed; the spec reads them in place. With the variable unset, or naming a directory that does not exist, every test in it skips, so CI never depends on them. Besides three files it expects at the top of the directory (`box-slot-defective-current.skp`, `round-slab-leg-current.skp`, `blank-26-purged.skp`; each test skips if its file is absent), it imports every `.skp` found anywhere below the directory and prints one `[private-fixtures:sweep] {...}` row per file (what the file holds, what the model holds after import, whether an amber mesh appeared, the import time). `SAWHORSE_PRIVATE_CAPTURES` (a directory; default `test-results/private-fixtures/`) receives a screenshot per model, named `<SAWHORSE_PRIVATE_CAPTURE_PREFIX>private-fixture-<name>.png`, and the sweep's table as `private-fixtures-sweep.md`; `SAWHORSE_PRIVATE_CAPTURES=off` takes no screenshots (the table still goes to the default directory).

Shared helpers for the lane-4 specs live in `e2e/helpers.ts`. `hover` re-reads the screen position and nudges the cursor until the expected inference shows, because the view refits after an edit. `solids()` reads every solid's display number, bounds and volume from `window.__sawhorse.solids()`.

The specs drive the page through read-only hooks on `window.__sawhorse`: `worldToScreen(p)` (metres to canvas pixels), `visibleEndpoints()`, `parts()`, `partScreenCenter(part)`, `camera()`, `grid()`, `pick(x, y)`, `pixel(x, y)`, `solids()`, `graph()` (the scene graph: groups, instances, definitions, the open context), `cutList(options?)` (the kernel's cut list, lane 7), and `refusals` (kernel refusal messages and how often each came back this page load); since lane 10 also `looks()` (the face style, profiles, shadows, decoded images, textured parts, the sun), `paint` (the active material, the face being positioned, its handles on screen and in the world, `position(object, face)`), `frameTime(n)` (milliseconds per frame over `n` renders with the camera turning) and `engineTriangles()`. The viewport mirrors its state in `data-*` attributes on `main.vp`: `data-kernel`, `data-view`, `data-projection`, `data-inference` (the kind under the cursor), `data-objects`, `data-volume` (m^3), `data-watertight`, `data-selection`, `data-hover`. A spec waits on those attributes rather than on timers.

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

Lane 5's walk-through, `scripts/capture-lane05.mjs`, builds a bookshelf from components in the running app: a Side component and its Ctrl+Move copy, a Shelf component and a Move array of three more, one shelf opened and made 1 in thick (all four follow), Make Unique on another and back to 3/4 in (only it changes), tags Carcass and Shelves, and two scenes (Front hides the shelves, Iso shows everything). It checks the solids after each stage and exits non-zero when one is wrong.

```bash
npm run dev -- --port 5195            # in another terminal
node scripts/capture-lane05.mjs --url http://localhost:5195 --out ../_captures
```

Lane 10's walk-through, `scripts/capture-lane10.mjs`, opens the component bookshelf (a `.shz` the WASM kernel writes from the `cutlist.json` golden), paints every part White oak with one click on the selection, turns the view round to paint the back Baltic birch ply, opens the Top and turns and slides its texture with the handles, switches to hidden line, X-ray and the Shop drawing preset, turns the shadows on at 3 pm (and logs the frame time of the lit, textured bookshelf), saves a scene and shows it bringing the style and the sun back, and ends on the cut list and the Materials pane.

Lane 7's walk-through, `scripts/capture-lane07.mjs`, builds the same bookshelf from components, opens the Cut list (grain arrows in the view), names the stock, turns the top's grain across, marks the back panel as 1/4 birch ply, saves the CSV (and checks it), shows the print preview, and saves the labels and the report as PDFs (rendered with MuPDF when Python has `fitz`). It checks the rows and exits non-zero when anything is off.

```bash
npm run dev -- --port 5197 --strictPort   # in another terminal
node scripts/capture-lane07.mjs --url http://localhost:5197 --out ../_captures
```

### Responsiveness bounds in the end-to-end tests (lane 16b)

A spec that asserts the window stayed responsive (the longest gap between two animation frames) never states its bound in plain milliseconds: the same work is 0.1 s on the development machine and ten or twenty times that on a GitHub runner, which has two to four shared CPUs and no GPU (WebGL there is SwiftShader, the GPU emulated on the CPU). Lane 16's 6-second bound on "select all at 100,000 solids" held locally and failed every CI run (19 to 31 s). The rule, implemented in `e2e/perf.ts`:

1. **Measure the machine in the page, inside the test.** `calibrate(page)` returns `cpu`, the median time of a fixed pure-JavaScript workload (fill and sort 200,000 doubles, then 100,000 string-keyed map entries; one warm-up, median of five), and `frame`, the median time of one full render of the scene as it stands, waited for by reading back a pixel at the canvas centre (an emulated GPU waits only for the work drawn where the read is: a corner returns before the frame is done). Call it when the page is otherwise idle, and read any gap you are about to assert first (calibrating draws frames of its own).
2. **State the bound in those units:** `frameBound(c, kCpu, kFrame) = min(20,000 ms, max(1,500 ms, kCpu x cpu + kFrame x frame))`. Choose `kCpu` and `kFrame` about three times what the work measures on the development machine, both as it is and under a 6x CPU slowdown (CDP `Emulation.setCPUThrottlingRate`), and under SwiftShader (`--use-angle=swiftshader --disable-gpu`) at 1x and 4x, and check that the bug the bound guards against still fails it on all of them. Write the measurements next to the bound.
3. **The floor and the ceiling are absolute.** 1,500 ms keeps a fast machine's garbage collection from failing a tight bound; 20,000 ms is a freeze on any machine, so no calibration lets one pass.
4. **Log what was measured**: `note(...)` adds the gap, the bound and the calibration to the test's annotations and the log (`[perf] ...` lines in CI's output), so a failure on a runner says how far off it was.
5. **Time bounds on short operations scale the same way**: `e2e/model-counts.spec.ts` allows 100 ms for a refusal on a machine whose workload takes 30 ms or less, and that times `cpu / 30` on a slower or busier one.

The timed specs (`e2e/model-cap.spec.ts`, `e2e/model-counts.spec.ts`) are their own Playwright project, `chromium-timed`, which depends on the `chromium` project (every other spec) and runs one test at a time: nothing else shares the machine while they measure it. A calibration cannot know about a load that starts after it: on a four-CPU runner model-cap's Materials pane once measured 15.7 s while the spec beside it built and deflated a 90 MB `scene.json`. (A failure in the other specs skips the timed ones; the run is red either way.) A file filter runs the whole `chromium` project first; to run a timed spec by itself add `--no-deps` (`npx playwright test e2e/model-cap.spec.ts --no-deps`).

`PERF_ABSOLUTE_MS=2000` replaces every frame-gap bound with that absolute number. The nightly workflow `nightly-perf.yml` runs `e2e/model-cap.spec.ts` that way on a self-hosted runner labelled `sawhorse-perf` (a machine with a real GPU and nothing else scheduled), only when the repository variable `SAWHORSE_PERF_RUNNER` is `true`; GitHub provides no such runner, so otherwise its `no-runner` job says why in the run summary and the absolute check is skipped.

The model-cap numbers (`e2e/model-cap.spec.ts`, longest gap for select all then nothing at 100,000 solids, the production build in Edge): development machine under 0.1 s (bound 1.5 s, the floor); 6x CPU slowdown 0.62 to 0.72 s (2.3 workloads; bound 12 workloads and 5 renders, about 3.3 s); SwiftShader 1x 0.63 to 0.73 s (1.6 renders; bound about 2.9 s) and 4x 0.97 to 1.0 s (bound about 4.5 s); the whole suite in parallel on the development machine 0.72 s (6 workloads; bound the 1.5 s floor). Before lane 16b the same select-all measured 5.2 s at 6x and 8.6 s under SwiftShader, both well past their bounds.

To profile a select-all yourself: an unminified e2e build (`SAWHORSE_TEST_HOOKS=1 node node_modules/vite/bin/vite.js build --outDir dist-prof --minify false`, served by `vite preview --outDir dist-prof`), a spec that reaches the cap (the chain plugin in `e2e/model-cap.spec.ts`), then CDP `Emulation.setCPUThrottlingRate`, `Profiler.start` / `Profiler.stop` around Ctrl+A and Escape, and self and total time summed per function from the profile's samples. Keep such a spec out of the repository (`*.local.spec.ts`).

### Section planes and drawing sheets (lane 12)

| What | Where |
|---|---|
| Planes, the exact cut (caps with holes, outline), slices, scenes' section state, the hash | `crates/kernel/src/section.rs` (design: `docs/adr/0010-sections-and-sheets.md`); the cap's planar arrangement in `crates/kernel/src/arrangement.rs` (since lane 12d); ops in `crates/kernel/src/model.rs`; `testing::check_sections`; property tests on hostile solids in `crates/kernel/tests/section_props.rs` |
| Scales, what a dimension measures, lengths on paper | `crates/kernel/src/paper.rs` |
| Sheets and items as ops, validation, `.shz`, the hash | `crates/kernel/src/sheets.rs`, `snapshot.rs` (`docs/format.md`) |
| Hidden-line drawings, the sheet layout, tables, DXF, slice exports | `crates/export/src/drawing.rs`, `sheet.rs`, `table.rs`, `sheet_dxf.rs`, `section.rs` |
| Snapping on a drawing | `crates/inference/src/flat.rs` |
| The worker queries | `crates/wasm-api/src/drawings.rs` (`Doc.section_view`, `sheet_render`, `sheet_dxf`, the slice exports; `replay_sections`, `replay_sheet_renders` for the goldens), `KernelClient` |
| The tool, clipping and the filled cut in the view | `src/tools/section.ts`, `src/lib/sections.ts`, `src/viewport/engine.ts` |
| Sheets mode, canvas, pane, tabs, exports | `src/sheets/` (`SheetsWorkspace`, `SheetCanvas`, `SheetsPane`, `SheetTabs`, `SheetRail`, `paper.ts`, `render.ts`, `exports.ts`, `runExport.ts`) |
| `.skp` section planes | `src/io/skp.ts` (`sectionOf`) |

```bash
bash scripts/cargo.sh test -p sawhorse-kernel --test sections --test sheets   # cuts, slices, scenes, sheets, items, refusals, hashes
PROPTEST_CASES=50000 bash scripts/cargo.sh test -p sawhorse-kernel --test section_props   # hostile solids: folds, frames, slivers, nested holes, touching solids
bash scripts/cargo.sh test -p sawhorse-export --test drawings                  # hidden lines, scales, dimensions, tables, every export
npx vitest run src/sheets src/lib/sections.test.ts src/tools/section.test.ts
PW_PORT=4221 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test e2e/sections.spec.ts e2e/sheets.spec.ts
```

The goldens `section.json` and `sheet.json` pin the cut and the laid-out sheet (`npm run golden:wasm` checks WASM against native). For tests, `window.__sawhorse.sections()` returns the planes and the cut being drawn, and `window.__sawhorse.sheet` (in Sheets mode) gives `render()` (the kernel's layout of the shown sheet) and `toClient(p)` (paper metres to page pixels).

Lane 12's walk-through, `scripts/capture-lane12.mjs`, opens the component bookshelf (written by the WASM kernel from the `cutlist.json` golden), cuts it with the Section Plane tool, reverses the plane, saves the Right view in the Shop drawing style with the cut as the scene "Section A", then builds a Letter landscape sheet through the canvas: the front elevation at 1:12, the plan at 1:8, the section at 1:4 slid down to the toe kick, three dimensions (36", 72", 17 1/4"), a label, the cut list and the title block. It exports PDF (checking the dimension strings, the captions and the paper size in it), SVG, DXF and a 300 dpi PNG beside the screenshots, and reopens the saved document in the dark theme. It exits non-zero when a stage does not produce what it should.

```bash
npx vite --port 5221 --strictPort        # in another terminal
node scripts/capture-lane12.mjs --url http://localhost:5221 --out ../_captures
```

### Plugins (lane 11)

| What | Where |
|---|---|
| The API's types, shim, manifest schema and validator, protocol, plugin-side runtime, geometry | `packages/plugin-api` (design: `docs/adr/0009-plugin-api.md`; guide: `docs/PLUGINS.md`) |
| The sandbox (one module worker per plugin) and its lockdown (an allowlist of globals) | `src/plugins/sandbox.worker.ts`, `lockdown.ts` |
| The source rules (sources parsed with acorn) | `packages/plugin-api/src/source.ts` |
| Permissions, requests, the watchdog; each worker started from a `blob:` module so the page's policy governs it | `src/plugins/host.ts` |
| The browser build's content security policy | `vite.config.ts` (`contentSecurityPolicy`, `cspMeta`); the desktop's: `src-tauri/tauri.conf.json` |
| Registry, loading, enabling, contributions, request handlers; grants bound to a package's SHA-256 | `src/plugins/manager.ts`, `loader.ts`, `zip.ts`, `store.ts` (`packageDigest`, `grantMatches`; `src/lib/sha256.ts`), `platform.ts` (desktop folders, browser IndexedDB), `bundled.ts` |
| The model as plugins read it, panel checks | `src/plugins/model.ts`, `panel.ts` |
| The window: pane, panels, dialogs, menu, shortcuts, rail group, plugin tools | `src/components/PluginsPane.tsx`, `PluginPanel.tsx`, `PluginDialog.tsx`, `src/plugins/usePlugins.tsx`, `src/tools/plugin.ts` |
| Named batches with placeholders | `crates/kernel/src/refbatch.rs`, `Doc.apply_named_batch` (`crates/wasm-api`), the worker's `namedBatch` |
| The samples | `plugins/` (type-checked by `plugins/tsconfig.json`) |

```bash
npm run plugin:check                                    # every plugin under plugins/: manifest, sources, type check
npm run plugin:check -- path/to/plugin                  # any folder: the same, its // @ts-check files type-checked
npm run plugin:pack -- plugins/cabinet-generator        # -> dist-plugins/<id>-<version>.zip (gitignored), deterministic; --out <file> elsewhere
npx vitest run packages src/plugins                     # codec, manifest, sources, runtime, host, panels, zips, samples, WASM batches
bash scripts/cargo.sh test -p sawhorse-kernel --lib --test refbatch     # batches against the goldens
PW_PORT=4211 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test e2e/plugins-sandbox.spec.ts e2e/plugins-model.spec.ts e2e/plugins-ui.spec.ts e2e/plugins-samples.spec.ts e2e/csp.spec.ts
```

`window.__sawhorse.plugins` (tests and developers): `list()`, `loadSession(files)`, `enable(id)`, `disable(id)`, `commands()`, `run(key)`. Since lane 17 it exists only where `__SAWHORSE_TEST_HOOKS__` is true (`vite.config.ts`, `testHooks`): the dev server, vitest, and a build with `SAWHORSE_TEST_HOOKS=1`. `enable` grants a whole manifest without the dialog, so a release must not have it: `npm run build` ends with `node scripts/check-test-hooks.mjs dist`, which fails a build that does. Playwright's server (`scripts/e2e-serve.mjs`, `npm run e2e:serve -- --port N`) builds `dist-e2e/` with the hooks (the same production build otherwise) and serves it with `vite preview`; that build is part of every Playwright run (a minute or two), and `reuseExistingServer` still reuses a server already on `PW_PORT`.

Testing plugins in Playwright: open the app with `?autosave=0&io=basic`. `io=basic` swaps the browser's File System Access pickers for a file input and downloads, which `filechooser.setFiles` and `waitForEvent("download")` can drive (the native picker ignores `setFiles`), so Install from .zip and the samples' exports work in a spec. The policy (`e2e/csp.spec.ts`) applies only to a built app (Playwright's server builds one, `dist-e2e/`, the release build plus the test hooks); `npm run dev` has none, because Vite's dev client injects inline scripts. A listener on a free loopback port (`listener()` in `e2e/plugins-sandbox.spec.ts`) is how a spec proves nothing left the sandbox: `connect-src` allows loopback, so a `fetch` from the page reaching it shows the test is not vacuous.

Lane 11's walk-through, `scripts/capture-lane11.mjs`, enables the three samples through the permission dialog, generates a 30 x 72 x 12 in cabinet with three shelves, shows the Outliner and the Cut list, opens the Right side from the Outliner and drills two columns of shelf pin holes (checked by volume: 32 holes, both sides), shows the model report and reads its exports, in light and dark:

```bash
npx vite --port 5211 --strictPort        # in another terminal
node scripts/capture-lane11.mjs --url http://localhost:5211 --out ../_captures
```

### The cut list (lane 7)

| What | Where |
|---|---|
| Parts, boxes, rounding, board feet and sheets, the hash | `crates/kernel/src/cutlist.rs` (design: `docs/adr/0006-cut-list.md`) |
| Grain and stock ops | `Op::SetGrain`, `Op::SetStock` in `crates/kernel/src/model.rs`; `PartAttrs` on `Definition`, `Node`, `Member` and `Object` |
| The worker query | `Doc.cut_list(options)` (`crates/wasm-api`), `KernelClient.cutList` |
| The pane, the report, the arrows | `src/components/CutListPane.tsx`, `CutListReport.tsx`, `src/cutlist/arrows.ts`, wiring in `src/App.tsx` |
| CSV, PDF writer, report and label PDFs | `src/cutlist/csv.ts`, `pdf.ts`, `documents.ts`, `format.ts` |

```bash
bash scripts/cargo.sh test -p sawhorse-kernel --test cutlist        # the bookshelf and every rule
npx vitest run src/cutlist src/components/CutListPane.test.tsx
PW_PORT=4197 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test e2e/cutlist.spec.ts
```

`testing::check_cut_list` runs after every accepted op in the fuzzer and in every repro replay, so a cut-list bug shows up as a fuzz find with its sequence. `readPdf` (in `src/cutlist/pdf.ts`) checks a PDF we wrote (header, every xref offset, the page tree, stream lengths) and reads its text back; the tests use it, and the walk-through also opens the files in MuPDF.

### Nesting (lane 9)

| What | Where |
|---|---|
| The stock library, its rules and ops | `crates/kernel/src/stock.rs` (design: `docs/adr/0007-nesting.md`); `Op::DefineStock`, `DeleteStock`, `SetMaterialStock`, `SetNestSettings`, `ResetStockLibrary` in `crates/kernel/src/model.rs`; `stock` in `.shz` (`docs/format.md`) |
| Stock choice, guillotine / maxrects / board layouts, cut sequences, totals, the hash | `crates/kernel/src/nesting.rs` (`Model::nest`, `nest_list`) |
| The invariants (fuzzer, repros, property tests) | `testing::check_nesting`, `testing::check_nesting_of` |
| The worker query | `Doc.nest(options)` and `replay_nesting` (`crates/wasm-api`), `KernelClient.nest` |
| The section, the Stock dialog, the wiring | `src/components/NestingSection.tsx`, `StockDialog.tsx`, `src/App.tsx` |
| Words, diagram geometry, SVG, DXF, PDF booklet | `src/nesting/format.ts`, `diagram.ts`, `svg.ts`, `dxf.ts`, `booklet.ts`; read back by `src/nesting/checks.ts` |

```bash
bash scripts/cargo.sh test -p sawhorse-kernel --test nesting --lib      # the bookshelf, every rule, property tests on random cut lists
npx vitest run src/nesting src/components/NestingSection.test.tsx        # words, files read back (fixture and WASM round trips)
PW_PORT=4201 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test e2e/nesting.spec.ts
```

`e2e/nesting.spec.ts` builds the bookshelf through the UI (`e2e/bookshelf-build.ts`, shared with the cut list spec), nests it (five 4/4 x 12 boards and one 1/4 in sheet), clicks a part, types a kerf, switches to maxrects, makes the carcass ply (three sheets), sets a material's stock in the Stock dialog (two parts reported too large), and reads every SVG, the DXF and the PDF back. `window.__sawhorse.nest(options?)` returns the kernel's nesting for tests.

Lane 9's walk-through, `scripts/capture-lane09.mjs`, does the same in a running dev server and saves screenshots:

```bash
npx vite --port 5201 --strictPort        # in another terminal
node scripts/capture-lane09.mjs --url http://localhost:5201 --out ../_captures
```

### Groups, components, tags and scenes (lane 5)

| What | Where |
|---|---|
| The tree, definitions, propagation, tidying, the structure validator | `crates/kernel/src/nodes.rs` (design: `docs/adr/0005-groups-components.md`) |
| The ops and their context rules | `crates/kernel/src/model.rs` (`Op::SetContext` ... `Op::ActivateScene`; `editable_object`, `sketch_edit_key`, `draw_on_plane`) |
| Undo and navigation | `crates/kernel/src/document.rs` (SetContext / LeaveContext are logged, not undo steps) |
| Persistence | `crates/kernel/src/snapshot.rs` (`nodes`, `definitions`), `docs/format.md` |
| What a click means in the open context | `src/lib/scope.ts` |
| Outliner, Tags pane, scene tabs, naming dialog | `src/components/Outliner.tsx`, `TagsPane.tsx`, `SceneTabs.tsx`, `NameDialog.tsx`; wiring in `src/App.tsx` |

```bash
bash scripts/cargo.sh test -p sawhorse-kernel --test structure --test structure_props   # behaviour and invariants
npx vitest run src/lib/scope.test.ts src/components/structure.test.tsx
PW_PORT=4195 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test e2e/groups.spec.ts
```

Every instance's solids must equal its definition's under its placement: the model validator checks it on every commit, so a propagation bug shows up as a kernel bug (a `Structure(...)` topology violation) in any test or fuzz run, never as a silently different copy.

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
. scripts/env.sh            # cargo + ~/.local/bin on PATH, CARGO_TARGET_DIR=/mnt/d/build-cache/sawhorse/wsl-<folder>-<sum>
cd crates
cargo test --workspace
cargo check --workspace --target wasm32-unknown-unknown
cd .. && wasm-pack build crates/wasm-api --target web --out-dir ../../src/wasm/pkg --out-name sawhorse_wasm --no-pack --release
node scripts/wasm-checkout.mjs   # is the kernel in src/wasm/pkg this checkout's? (npm run wasm checks it itself)
```

## Test machinery (ADR-0001 "robustness first")

| What | Where | Runs |
|---|---|---|
| Validator after every B-rep and sketch mutation | `Brep::debug_validate`, `Sketch::check` | every debug build (tests) |
| Validate before commit | `Model::apply` (clone, mutate, validate, commit) | always |
| Global self-intersection (edges through faces, chords, vertex/edge contact, coplanar overlap) and positive volume | `kernel/src/intersect.rs`, `Object::validate` | every commit |
| Property invariants | `crates/kernel/tests/invariants.rs` (extrude, push/pull inverse), `sketch_props.rs` (insertion order independence, simple region boundaries, split-then-erase identity), `face_props.rs` (split/imprint then merge identity, transactional refusals), `push.rs` (every push/pull path, volume/area/Euler) | `cargo test` |
| Boolean differential | `crates/kernel/tests/booleans/cases.json` (26 cases) via `tests/booleans.rs` (hand-computed volume, area, shells, genus, faces) and `scripts/boolean-diff.mjs` (same cases, WASM kernel vs npm `manifold-3d` on identical operand meshes) | `cargo test`, `npm run boolean:diff` (CI) |
| Op-sequence fuzzer | `crates/kernel/tests/sequences.rs`: op intents resolved against the live document (strokes, rectangles, polylines, erase, extrude, push/pull incl. push-through, split_face, imprint, delete, groups, tags, since lane 4 transforms, copies, arrays, partial moves, offsets and Follow Me, and since lane 5 make group / component / unique, explode, delete nodes, open and close (the context), node moves, copies and arrays, hide, lock, tag create / rename / colour / delete / assign, scenes add / update / delete / activate, renames, and since lane 12 section planes (add, place, flip, activate, hide, rename, delete, move, copy, slice to a group) and drawing sheets (add, set, delete, move; items added, changed, moved, deleted and reordered); undo/redo interleaved; a third of the sequences start with a component, two copies and one copy open, and while a context is open most intents pick what it holds); every solid must be one piece; every op checked for validity (the tree and every instance included), typed refusals, unchanged state on refusal, undo/redo round trip (navigation excepted: it is not an undo step), log replay and determinism (`quick_fuzz` 192 sequences; `long_fuzz` ignored, seeded) | `cargo test`; CI runs 10,000 |
| Nesting (lane 9) | `crates/kernel/tests/nesting.rs` (the bookshelf on the built-in stock and as a plywood carcass with exact counts and yield, kerf, trim, both algorithms, grain, stock named by a part and by a material, parts no stock takes, the library's ops, undo, refusals and `.shz`; proptest on random cut lists, settings and libraries: no overlaps, kerf gaps, inside the usable area, grain, parts in = placed + unplaced, cuts crossing no part, guillotine layouts coming apart, identical parts filling sheets in order; its last test writes the `nesting.json` golden's ops under `UPDATE_GOLDEN=1`); `nesting.rs` and `stock.rs` unit tests (FFD and the lookahead, cut finding, the pinwheel that is not guillotine, the built-in library) | `cargo test` |
| Materials, styles, shadows (lane 10) | `crates/kernel/tests/looks.rs` (what faces show through face, solid and container paint; paint through splits, imprints, pushes; components and per-copy paint; the cut list's species; textures and positions, the live preview equal to the op; refusals; rename and delete; locked items; styles and shadows in scenes; hashes; it writes the `painted.json` golden's ops under `UPDATE_GOLDEN=1`), unit tests in `looks.rs` (base64, image headers, texture paths and frames, the affine fit, the sun), `crates/export` (the library, PNG, GLB textures, OBJ `vt` / `map_Kd`, SVG edge styles), `crates/shz` (round trip with images, a missing image), `crates/wasm-api` (a library batch through JSON), `src/tools/paint.test.ts`, `src/io/skp.test.ts` (per-face materials, an image and its size), `e2e/materials.spec.ts` and the painted cabinet's golden image | `cargo test`, `npx vitest run`, Playwright |
| Cut list (lane 7) | `crates/kernel/tests/cutlist.rs` (the bookshelf as components with exact board feet, grain, stock, sheets, hidden and Visible only, handed and rotated copies, a scaled copy, nested roll-up, groups and plain solids, a tapered leg, rounding, every rounding giving the same classification and totals, the 1/4 in line on the exact thickness, determinism, the snapshot; its last test writes the `cutlist.json` golden's ops under `UPDATE_GOLDEN=1`), `cutlist.rs` unit tests (boxes of boards, rotated boards, a tapered leg, a hexagonal prism; rounding; principal axes), and `testing::check_cut_list` in every fuzz sequence | `cargo test` |
| Groups and components (lane 5) | `crates/kernel/tests/structure.rs` (a bookshelf of components, groups, scoped stickiness, severance making a copy unique, nesting, tags, scenes, hidden / locked, rotated and mirrored instances, the snapshot) and `structure_props.rs` (proptest: an instance edit reaches every instance exactly; Make Unique forks; Make Component then Explode and Make Group then Explode are the identity on hashes) | `cargo test` |
| Repro replay | `crates/kernel/tests/repro/*.json` via `tests/repro.rs` | `cargo test` |
| Transforms (lane 4) | `crates/kernel/tests/transforms.rs`: object-level moves, turns, scales and mirrors keep face ids and drawn edges; copies and arrays; moving part of a solid with autofold off and on; severance; proptest "transform then inverse restores the hash"; `transform.rs` unit tests (`Xform` algebra, `divided` steps compose back) | `cargo test` |
| Offset and Follow Me (lane 4) | `crates/kernel/tests/offset.rs` and `follow_me.rs` (exact areas and volumes: Pappus for mitered sweeps, the moulding round a box top at either end); `offset.rs`, `sweep.rs`, `snap.rs` and `sever.rs` unit tests | `cargo test` |
| Inference | `crates/inference/tests/infer.rs` (every inference kind, ranks, pixel tolerance at depth, occlusion incl. oblique views of the face under the cursor, locks, length snapping, from-point memory, guides) and the TS tool tests in `src/tools/*.test.ts` (each tool against a fake host) | `cargo test`, `npx vitest run` |
| Golden hashes | `crates/kernel/tests/golden/*.json` (document op logs, undo/redo included; since lane 7 each also records its cut list hash, since lane 9 its nesting hash) via `tests/golden.rs` and `scripts/golden-wasm.mjs` (both hashes, WASM against native) | `cargo test`, `npm run golden:wasm` |
| cargo-fuzz | `crates/kernel/fuzz` (own workspace, nightly + libFuzzer) | `cd crates/kernel && cargo +nightly fuzz run op_sequence` (Linux/WSL) |

When the fuzzer fails it writes the shrunk sequence to `crates/kernel/tests/repro/pending/pending-<test>.json` (a panic while resolving intents is captured too). That directory is gitignored and the replay test never reads it, so a fresh find breaks nobody else's `cargo test`. Promote it: move it up into `crates/kernel/tests/repro/` under a name that says what it found, fix the kernel, commit both; see `crates/kernel/tests/repro/README.md`. A find made with `FUZZ_PLANE_SNAP=0` records `"plane_snap": false` and replays with snapping off.

Fuzzer knobs (all forwarded into WSL by `scripts/cargo.sh`): `FUZZ_CASES` (count), `FUZZ_SEED` (deterministic shard seed; the repro is then `pending/pending-long_fuzz_seed<N>.json`), `FUZZ_REPORT=1` (per-op accepted/refused table, the top refusal reasons, both boolean-path refusal counts and the refusal rate per accepted push/pull and sweep), `FUZZ_PLANE_SNAP=0` (turn the boolean plane-snapping pass off for the run: the same seeds then measure what it buys), `FUZZ_REASON_LEN=200` (longer refusal reasons in the report, the validator's own words), `FUZZ_DUMP_REFUSALS=1` (write the first 30 boolean refusals as replayable logs to `crates/kernel/tests/repro/refusals/`, gitignored, to study what the engine left behind). `FUZZ_REPRO_DIR=<dir>` writes a failing sequence (and the refusal dumps) there instead of `crates/kernel/tests/repro/pending/` (and `repro/refusals/`), for a long local run that should leave the checkout alone; move the file into `tests/repro/` when you fix it.

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
| 2026-10-13 (lane 16b) | The model's count limits (guides, dimensions, tags, materials, scenes, section planes) in `check_model`, `check_snapshot`, imports and scenes. `testing::check_sequence` now records the peak of every count over a run, and the sequence fuzzer fails a generated sequence that holds more than a tenth of any limit (so a limit refusal never stands in for the op it refused). `scripts/fuzz-shards.sh 2500 20264000 1` (seeds 20264001..20264008, snapping on) and `fuzz-shards.sh 6250 20264100 0` (seeds 20264101..20264108, **snapping off**), debug build, WSL Ubuntu 24.04, parallel, `FUZZ_REPRO_DIR` outside the checkout; and CI's fixed seed (`FUZZ_CASES=10000 FUZZ_SEED=20261011`) | **70,000 sequences clean, no generated sequence near a limit.** Snapping on: 20,000 sequences, 293,527 ops accepted, 248,800 refused (51 s wall; boolean-path refusals: not a valid solid 250, degenerate 9). Snapping off: 50,000 sequences, 733,453 accepted, 623,867 refused (108 s wall; 689 and 21). CI's seed: 10,000 sequences, 146,570 accepted, 123,491 refused (64 s), the same counts as before the limits: no op the fuzzer makes reaches one. No find, no pending repro. |
| 2026-10-13 (lane 18) | The cut keeps every cap; one rule for what Create Group from Slice can draw (ADR-0010, lane-18 amendment). First the property tests, three runs of `PROPTEST_CASES=100000 bash scripts/cargo.sh test -p sawhorse-kernel --test section_props`, each with its own random seed (`PROPTEST_RNG_SEED` unset), on the final code: the seven hostile generators at 100,000 cases each, the named repros, and the three lane-17 verification seeds in `section_props.proptest-regressions`; then `scripts/fuzz-shards.sh 3125 20263800 0` (seeds 20263801..20263808, **plane snapping off**) and `fuzz-shards.sh 1250 20263900 1` (seeds 20263901..20263908, snapping on), debug build, WSL Ubuntu 24.04, parallel, `FUZZ_REPRO_DIR` outside the checkout; CI's fixed seed (`FUZZ_CASES=10000 FUZZ_SEED=20261011`); and the base of the lane's own find again, `fuzz-shards.sh 3125 20263600 0`. `testing::check_sections` now also slices a copy of the model after every accepted op and requires the slice to succeed exactly when a cap is representable | **Property tests: 12 passed in each of the three runs, 2,100,000 generated cases clean** (307 s, 302 s and 312 s). **Fuzz: 35,000 sequences clean**: snapping off 25,000 sequences, 365,101 ops accepted (60 s wall; boolean-path refusals: not a valid solid 377, degenerate 11); snapping on 10,000 sequences, 146,063 ops accepted (28 s wall; 118 and 6); CI's seed 10,000 sequences, 146,570 accepted (70 s); base 20263600 again, 25,000 clean. One find on the way, on the lane's first commit (seed 20263608, snapping off, after 2,450 clean sequences of 3,125): two boxes a micrometre apart whose caps were each representable but which the sketch, drawing them one after the other, merged into each other and refused; the slice now welds all its loops together (`tests/repro/2026-10-13-section-slice-two-caps-a-micrometre-apart.json`). The first round of property runs, before that fix, was clean too (517 s, 333 s, 324 s). |
| 2026-10-13 (lane 17) | One rule for what a section cap is, for the cut and Create Group from Slice (ADR-0010, lane-17 amendment). First the property tests, `PROPTEST_CASES=100000 bash scripts/cargo.sh test -p sawhorse-kernel --test section_props`: the seven hostile generators at 100,000 cases each, plus the named repros (the two lane-12d finds among them); then `scripts/fuzz-shards.sh 12500 20263400 0` (seeds 20263401..20263408, **plane snapping off**), `fuzz-shards.sh 6250 20263500 1` (seeds 20263501..20263508, snapping on), debug build, WSL Ubuntu 24.04, parallel, `FUZZ_REPRO_DIR` outside the checkout; and CI's fixed seed (`FUZZ_CASES=10000 FUZZ_SEED=20261011`) | **Property tests: 11 passed, 700,000 generated cases clean** in 261 s. **Fuzz: 160,000 sequences clean**: snapping off 100,000 sequences, 1,463,113 ops accepted (210 s wall; boolean-path refusals: not a valid solid 1,450, degenerate 69); snapping on 50,000 sequences, 733,357 ops accepted (122 s wall; 607 and 27); CI's seed 10,000 sequences, 146,570 accepted (65 s). No find, no pending repro. |
| 2026-10-13 (lane 12d) | The section cap rebuilt on a planar arrangement (ADR-0010, lane-12d amendment). `scripts/fuzz-shards.sh 12500 20263100 0`: eight shards of 12,500 (seeds 20263101..20263108) with **plane snapping off**, then `fuzz-shards.sh 6250 20263200 1` (seeds 20263201..20263208, snapping on), debug build, WSL Ubuntu 24.04, parallel, `FUZZ_REPRO_DIR` outside the checkout; and CI's fixed seed (`FUZZ_CASES=10000 FUZZ_SEED=20261011`). Before them, the property tests `tests/section_props.rs` at `PROPTEST_CASES=50000`: seven hostile generators (folded boxes, frame prisms cut along a hole edge at any angle, two boxes touching at a corner, faces sheared, tilted or bent within a micrometre of the plane and sometimes split, planes through one to four vertices, a pocket round a post with its own hole, planes exactly on a face), each checking both sides of the plane for halves that make the whole, closed halves, simple rings, the cap's area against the faces clipped to the plane independently, and a slice that covers every cap | **100,000 sequences clean with snapping off** in 361 s wall: 1,468,020 ops accepted, 1,246,886 refused (typed), 158,888 solids at the end; boolean-path refusals: not a valid solid 1,344, degenerate 59. **50,000 clean with snapping on** in 302 s wall: 732,253 ops accepted, 617,472 refused, 79,788 solids; not a valid solid 632, degenerate 20. CI's seed: 10,000 clean. After the rebase onto lane 16, `fuzz-shards.sh 2500 20263300 0`: 20,000 more clean with snapping off (292,825 ops accepted, 248,791 refused). **Property tests: 350,000 cases clean** (50,000 per generator) in 181 s, plus the fixed cases (every section repro played to its end and checked the same way, 19 files; the two verification finds; the property tests' own find). Put back, the lane-12c chain (and its tie rule) fails the frame generator (cap rings crossing each other) and the sliver generator within 2,000 cases, and the fixed cases. Finds on the way, all kept: the two lane-12c verification counterexamples (`2026-10-13-section-cap-lost-after-split`, seed 20262101: a weak face's piece pointed the wrong way and the chain dropped the cap; `2026-10-13-section-frame-prism-along-a-hole-edge`, seed 20262303: the hole's edge lay along the outline, met at no node, and the cut-away cap lost its notch), both fixed by the arrangement; and three of the property tests' own, older than this lane (the chain fails them the same way): a box face bent 0.3 micrometres off a plane through its other three corners, classified corner by corner, clipped to a polygon no cap closes (the tie rule now lays a face within twice its own bend of the plane on it; `a_face_within_its_bend_of_the_plane_lies_on_it`), a face within a nanometre of the plane but tilted 1.5e-8 rad, 1.2e-9 m2 off the cap it lies on (the watertight check now allows such a face its tilt), and a face sheared until a solid a micrometre thick turned inside out (accepted by the validator as thinner than the merge distance; no cap can close it, so the generator keeps partial shears within half a side). |
| 2026-10-13 (lane 12c) | `scripts/fuzz-shards.sh 12500 20261400 0`: eight shards of 12,500 (seeds 20261401..20261408) with **plane snapping off**, debug build, WSL Ubuntu 24.04, parallel; then the lane-12 re-verifier's own seed bases with snapping off, `fuzz-shards.sh 6250 20261800 0` and `6250 20261900 0` (the bases that found seeds 20261803, 20261904 and 20261907); the standard mixed run `fuzz-shards.sh 2500 20261410 1` (snapping on); and CI's fixed seed (`FUZZ_CASES=10000 FUZZ_SEED=20261011`). `testing::check_sections` now also checks that no cap ring passes a point twice and that each half a plane keeps is watertight (`Model::kept_part`'s vector area, within the faces' bend times the cap's length) | **100,000 sequences clean with snapping off** in 309 s wall: 1,461,129 ops accepted, 1,241,193 refused (typed), 157,637 solids at the end; boolean-path refusals: not a valid solid 1,380, degenerate 53. **The verifier's bases with snapping off: 100,000 more clean** (732,745 and 731,893 ops accepted; seeds 20261803, 20261904 and 20261907 each run their full 6,250). **Mixed, snapping on: 20,000 clean** in 260 s wall: 293,561 ops accepted, 248,806 refused, 31,744 solids; not a valid solid 267, degenerate 12. CI's seed: 10,000 clean. Finds on the way: the three verifier counterexamples, one cause (the cap chain ran a piece backwards at a vertex where the surface crosses the plane twice; `2026-10-12-section-pinch-*`, fixed by tracing head to tail); and eight shrunk sequences where the new checks were too strict on faces planar only to within PLANARITY (a micrometre off after Move with autofold with snapping off), the halves always right: six where a half's faces and cap joined along chords up to the faces' bend apart (`2026-10-13-section-closure-on-a-bent-face-1..6`), one where the allowance measured a cap another plane had clipped away (`-section-closure-under-another-plane`), and one, older than this lane, where the whole read on its triangles exceeded the halves read in the faces' planes by 1.4e-6 m3 (`-section-halves-on-bent-faces`; main fixed the check the same night, holding the halves to the whole measured their own way, and the case passes under it). The 20261400 run stopped on those twice and the 20261800 run once; each was run again to the end after the check was corrected (the numbers above are the complete runs). The property test `tests/section_props.rs` (folded boxes cut with snapping off through their own vertices, along their faces and at random: halves, closure, simple rings, slices that close) ran 30,000 cases clean; the old chain fails it within the default 512. |
| 2026-10-12 (lane 12) | `scripts/fuzz-shards.sh 2500 20261200 1`, eight shards of 2,500 (seeds 20261201..20261208), debug build, WSL Ubuntu 24.04, parallel; the fuzzer now also drives section planes (`add_section` with finite, zero and garbage normals; `place_section`, `flip_section`, `set_section_active`, `set_section_hidden`, `rename_section` (blank names too), `delete_section`, `move_sections` and `copy_sections` with every transform pick, `slice_to_group`), scenes carrying the planes' state, and drawing sheets (`add_sheet` / `set_sheet` over every paper, orientation, units and length format, some refused: no name, margins with no room, a coarse grid, a field over 500 characters, a 10 mm paper, a 1/3 fraction; `delete_sheet`, `move_sheet`; `add_sheet_item` / `set_sheet_item` with viewports of scenes, standard views, parallel and perspective cameras and a missing scene at NTS, standard and out-of-range scales, dimensions of every kind on the sheet's viewports or a missing one, labels with paper and model leaders, tables with any columns, images held and not, lines and rectangles, garbage coordinates; `move_sheet_items`, `delete_sheet_items`, `order_sheet_item`). An edit of a plane, a sheet or an item when there is none makes one first (except one time in sixteen, to keep those refusals). After every accepted op the validator checks every sheet and plane, and `testing::check_sections` checks every cut and that the two halves of each plane add up to the whole. Also CI's fixed seed (`FUZZ_CASES=10000 FUZZ_SEED=20261011`) | **20,000 sequences clean** in 86 s wall: 301,458 ops accepted, 236,976 refused (typed), 31,726 solids at the end. Accepted / refused: add_section 17,877 / 1,173, place_section 1,691 / 561, flip_section 685 / 111, rename_section 347 / 398, delete_section 650 / 140, move_sections 1,026 / 1,236, copy_sections 341 / 816, slice_to_group 142 / 609, add_sheet 20,705 / 1,611, set_sheet 2,021 / 880, delete_sheet 1,305 / 139, move_sheet 1,287 / 147, add_sheet_item 11,070 / 6,282, set_sheet_item 915 / 939, move_sheet_items 465 / 740, delete_sheet_items 302 / 315, order_sheet_item 443 / 162. Three finds fixed on the way, kept in `tests/repro/`: a box turned by 1e-16 rad put its top face a hair off the plane, and the faces were clipped exactly while the cap used the tie rule, so a half came out negative (`2026-10-12-section-cap-on-a-tilted-face`; the faces now use the cap's own side of each vertex and its crossing points); a plane through a reflex vertex of a notched prism paired its coincident crossings differently on the top and bottom faces and the cap folded over itself to nothing (`2026-10-12-section-through-a-reflex-vertex`; a crossing at a vertex on the plane is now that vertex, one node for every face, and rings drop zero-width spikes); and a face tilted 0.75 micrometres off the plane, kept whole by the tie rule, made a half -3.5 cubic millimetres (`2026-10-12-section-along-a-face-tilted-a-micrometre`; geometry within POINT_MERGE, so the check now allows the solids' area times POINT_MERGE). Boolean-path refusals: not a valid solid 239, degenerate 12. CI's seed: 10,000 sequences clean, 149,625 ops accepted, 118,499 refused. The export crate adds `random_sheets_lay_out` (40 seeded random sheets on the section golden: finite primitives, deterministic, DXF out). |
| 2026-10-12 (lane 11, on lane 10) | `scripts/fuzz-shards.sh 1250 20261320 1`, eight shards of 1,250 (seeds 20261321..20261328), debug build, WSL Ubuntu 24.04, parallel, after rebasing onto lanes 9 and 10 (one fuzzer driving every lane's ops); the fuzzer now also sends plugin batches (`sawhorse_kernel::refbatch`): named batches whose ops name what earlier ops of the same batch made through placeholders (boxes drawn on the ground or on a new sketch and pulled, boxes imported as polygon soups, renamed, made components, pushed, moved, mixed with ordinary intents resolved before the batch, and garbage: placeholders to later ops, bad paths, forbidden ops, unknown ops, placeholders with extra fields). An accepted batch is logged as one plain `batch` op that must land exactly where refbatch did, and is then checked like every op (validator, looks, cut list, nesting, undo/redo round trip, replay, determinism); a refused batch must leave the document, its log and its undo depth untouched. Also CI's fixed seed (`FUZZ_CASES=10000 FUZZ_SEED=20261011`) | **10,000 sequences clean** in 32 s wall: 136,201 ops accepted, 133,313 refused (typed), 16,955 solids at the end. Plugin batches: 1,991 accepted, 6,994 refused, the document untouched every time (top reasons: rectangles under 0.01 mm, edits outside the open group or component, placeholders that resolve to nothing, unknown ids). Boolean-path refusals: not a valid solid 160, degenerate 11. CI's seed: 10,000 sequences clean, 138,131 ops accepted, 2,072 plugin batches accepted and 6,960 refused. Before the rebase, seeds 20261311..20261318: 10,000 clean, 2,166 batches accepted, 7,523 refused. No finds, so no new repro. |
| 2026-10-12 (lane 10 on lane 9) | `scripts/fuzz-shards.sh 2500 20261220 1`, eight shards of 2,500 (seeds 20261221..20261228), after rebasing lane 10 onto lane 9: one fuzzer driving both lanes' ops, and after every accepted op `check_looks`, `check_cut_list` and `check_nesting` | **20,000 sequences clean** in 59 s wall: 275,665 ops accepted, 278,585 refused (typed), 30,599 solids at the end; accepted / refused: paint 5,976 / 13,016, set_face_uv 2,509 / 3,041, define_material 1,774 / 2,433, set_style 1,936 / 0, set_shadows 1,756 / 129, rename_material 66 / 1,826, delete_material 99 / 1,769, set_material_stock 937 / 1,062, define_stock 163 / 1,889, delete_stock 341 / 620, set_nest_settings 652 / 1,374, reset_stock_library 1,025 / 0. No finds, no new repro files. Boolean-path refusals: not a valid solid 360, degenerate 13. |
| 2026-10-12 (lane 10) | `scripts/fuzz-shards.sh 2500 20261210 1`, eight shards of 2,500 (seeds 20261211..20261218), debug build, WSL Ubuntu, parallel; the fuzzer now also drives `define_material` (with and without a texture image added in the same batch, a file-name texture, a path never added, sizes including zero, NaN rotations, stock links), `paint` (face masks with an unknown face sometimes, items in and out of the open context), `set_face_uv` (NaN, zero and huge scales), `rename_material`, `delete_material`, `set_style`, `set_shadows` (latitudes past the poles), scenes carrying a style or shadows; after every accepted op every face is sampled and its texture coordinates checked (`testing::check_looks`) | **20,000 sequences clean** in 241 s wall: 275,296 ops accepted (paint 5,858, set_face_uv 2,586, define_material 1,801, set_style 1,951, set_shadows 1,768, rename_material 72, delete_material 87), 281,265 refused (typed), 30,636 solids at the end; a first 20,000 with seeds 20261201..20261208 (before the rename / delete intents named existing materials) was clean too. No finds, so no new repro files. Boolean-path refusals: not a valid solid 350, degenerate 14. |
| 2026-10-12 (lane 9) | `scripts/fuzz-shards.sh 2500 20261290 1`, eight shards of 2,500 (seeds 20261291..20261298), debug build, WSL Ubuntu 24.04, parallel; the fuzzer now also drives the stock library (`define_stock` with built-in and new names, replacements, blank and over-long names and broken sizes; `delete_stock`; `set_material_stock` on the model's materials and others; `set_nest_settings` with kerfs and trims in and out of range and both algorithms; `reset_stock_library`), a part's stock can name a library stock, and after every accepted op `testing::check_nesting` nests three ways (guillotine, maxrects, Visible only) and checks every layout invariant | **20,000 sequences clean** in 177 s wall: 278,665 ops accepted, 278,846 refused (typed). Lane-9 ops accepted / refused: define_stock 198 / 2,058, delete_stock 409 / 664, set_material_stock 1,044 / 1,249, set_nest_settings 774 / 1,446, reset_stock_library 1,164 / 0. Boolean-path refusals: not a valid solid 344, degenerate 15. No finds, so no new repro; the property test's own shrunk case (a part on a board whose trim was narrower than the kerf, which made "a trim narrower than the kerf takes a kerf") is kept in `crates/kernel/tests/nesting.proptest-regressions`. |
| 2026-10-12 (lane 7) | `scripts/fuzz-shards.sh 2500 20261200 1`, eight shards of 2,500 (seeds 20261201..20261208), debug build, WSL Ubuntu 24.04, parallel; the fuzzer now also drives `set_grain` and `set_stock` (on listed parts, on solids, nodes and definitions picked anywhere, on unknown ids, with blank and over-long stock), and after every accepted op works out the cut list three ways (1/16 in, Visible only, 0.5 mm) and checks it against a brute-force count of the world (`testing::check_cut_list`) | **20,000 sequences clean** in 67 s wall: 277,493 ops accepted (set_grain 1,508, set_stock 1,310), 279,474 refused (typed), 31,288 solids at the end. One find fixed on the way: a solid offset 5,000 m across overflowed an i64 in the board feet (`tests/repro/2026-10-12-cut-list-board-feet-overflow.json`; the product is now in f64). Boolean-path refusals: not a valid solid 448, degenerate 24. |
| 2026-10-11 (lane 5) | `scripts/fuzz-shards.sh 6250 20261100 1`, eight shards of 6,250 (seeds 20261101..20261108), debug build, WSL Ubuntu 24.04, parallel; the fuzzer now also drives every lane-5 op (groups, components, the editing context, node moves / copies / arrays, hide, lock, tags, scenes, renames), a third of the sequences start inside one copy of an arrayed component, and the validator checks every instance against its definition after every op | **50,000 sequences clean** (about 47 s per shard): 688,017 ops accepted, 694,357 refused (typed), 79,952 solids at the end. Accepted lane-5 ops: make_component 26,211; set_context 29,912; make_group 7,045; leave_context 2,791; explode 1,643; make_unique 1,307; delete_node 798; set_hidden 2,453; set_locked 2,502; tag_items 2,473; create_tag 2,775; rename_tag 572; set_tag_color 674; delete_tag 707; add_scene 1,801; update_scene 36; activate_scene 41; delete_scene 34; rename 2,424 (with array 17,207, copy 9,111 and transform 12,254 now including node moves). Five finds fixed on the way, kept in `tests/repro/`: emptying the open group inside one copy of a component (by deleting or collapsing its only solid) let tidying remove its empty counterparts in the other copies, which removed it from the definition and so from under the open context (`2026-10-11-empty-open-group-in-a-component`, `-collapse-empties-open-group-in-a-component`, `-collapse-in-open-group-of-a-copy`; tidying now keeps the counterparts of anything on the context path); and two pre-existing face-edit bugs the new op mix reached: an imprint on a sweep 5 micrometres thick, and one near a face folded by a partial move, put a corner within the merge distance of another face and reached the validator as a kernel bug (`-imprint-on-a-five-micron-sliver`, `-imprint-near-a-folded-face`; face edits now refuse that, typed, `FaceEditError::TooClose`). Boolean-path refusals: not a valid solid 951, degenerate 44. |
| 2026-10-11 (integration) | `scripts/fuzz-shards.sh 6250 20261100 1` after merging lanes 4, 6 and 8 and the imprint fix, and CI's fixed seed `FUZZ_CASES=10000 FUZZ_SEED=20261011` | **50,000 sequences clean** in 89 s wall (seeds 20261101..20261108; about 86,000 to 88,000 ops accepted and 78,000 to 80,000 refused per shard). Boolean-path refusals: not a valid solid 1,563, degenerate 83. The CI seed: 10,000 sequences clean, 138,694 ops accepted. The find that prompted it, from a random-seed CI run, is `tests/repro/imprint-near-tolerance-non-positive-volume.json` (earcut mis-bridged a hole on a notched face; fixed with the ear-clipping fallback). |
| 2026-10-11 (lane 4) | `scripts/fuzz-shards.sh 6250 20261100`, eight shards of 6,250 (seeds 20261101..20261108), debug build, WSL Ubuntu 24.04, parallel; the fuzzer now also drives transforms, copies, arrays, moving part of a solid (autofold on and off), offsets (faces, regions, edge chains) and Follow Me (open, L and closed paths), and checks that every solid is one piece (severance) | **50,000 sequences clean** in 91 s wall: 695,256 ops accepted, 630,713 refused (typed), 52,297 solids at the end. Accepted lane-4 ops: transform 15,506; copy 10,291; array 4,163; transform_geometry 4,878; offset 8,952; follow_me 5,726 (push_pull 43,172, push_pull_new_face 13,975). Six finds fixed on the way, kept in `tests/repro/`: an inward offset whose grown hole cut the shrunk outline left a spike where the closing arc was mitered (`2026-10-11-offset-closing-arc-miter-spike-1..5`), and an offset passing exactly through a vertex tripped a `cavalier_contours` debug assertion (`2026-10-11-offset-through-a-vertex-exactly`; tests now run that crate without debug assertions, as release and the browser do). **Boolean refusal rate, same seeds, plane snapping off then on**: "not a valid solid" 1,677 then 1,563 (-6.8%); with the "degenerate engine output" refusals, 1,759 then 1,646 (-6.4%); per accepted push/pull or sweep 2.80% then 2.62% (push/pull alone 2.55% then 2.38%). Higher than lane 3's 0.5% because transformed (turned, scaled) solids put many more faces on tilted planes; the rest is mostly zero-thickness fins where a pushed face's swept side wall lies, up to rounding, in the plane of a tilted outer wall (see `docs/adr/0001-kernel.md`). |
| 2026-10-11 (lane 3) | eight shards of `FUZZ_CASES=6250`, `FUZZ_SEED=20261012..20261019`, debug build, WSL Ubuntu 24.04, parallel | **50,000 sequences clean** in 64 s wall: 720,677 ops accepted, 604,336 refused (typed), 32,268 solids at the end. The fuzzer now also drives lane 3's ops: draw (29,858 accepted), push_pull_new_face (14,938), add_guide / erase_guide / delete_guides, add_dimension / erase_dimension, erase_edge (2,022), erase_face (2,439), erase_region (9,247), batch (4,247). Boolean-path refusals (`not a valid solid`, near-coincident geometry on tilted planes): 291 of 61,102 accepted push/pulls, about 0.5%, unchanged from lane 2. |
| 2026-10-11 (lane 2) | `FUZZ_CASES=50000 FUZZ_SEED=20261011`, debug build (validator after every mutation), WSL Ubuntu 24.04, one process | **50,000 sequences clean** in 266.7 s: 803,893 ops accepted, 517,228 refused (typed), 40,677 solids at the end. Accepted per kind: add_rectangle 147,426; insert_segment 118,657; undo 90,323; extrude 79,612; push_pull 66,949; split_face 48,214; imprint 37,363; set_tag_visible 30,960; erase_segment 29,362; new_sketch 26,676; add_polyline 25,785; set_tag 16,537; delete_object 16,408; create_group 10,951; redo 8,670. |
| 2026-10-11 (lane 2) | `FUZZ_CASES=20000 FUZZ_SEED=7`, second shard after the fixes | **20,000 sequences clean** in 111.0 s: 322,825 ops accepted, 207,562 refused. 128 push/pulls were typed boolean-path refusals (`the result is not a valid solid`: near-coincident geometry on tilted planes), about 0.5% of accepted push/pulls. |
| 2026-10-11 (lane 2) | seed 20261011, earlier attempts | Five kernel bugs found and fixed before the clean run, each kept as a repro in `crates/kernel/tests/repro/`: an inside-out solid accepted from the topological push path (`2026-10-11-push-inverts-thin-wedge`); duplicate sketch edges when two crossings merged next to a shared vertex (`-sketch-near-coincident-crossings`); a zero-thickness fin from a boolean on a split vertex 1e-17 m off its plane (`-split-vertex-off-plane-fin`); a cut grazing a hole corner by 1e-16 m (`-split-grazes-hole-corner`); a crossing landing within the merge distance of an unrelated short edge (`-sketch-crossing-cluster`). |


## Source maps and the provenance tripwire

Production builds emit no source maps: a map embeds every dependency's original source, and openskp's carries a SketchUp-authored blank template that tree-shaking keeps out of our JavaScript (see `docs/CLEANROOM.md`, 2026-10-12). `SAWHORSE_SOURCEMAP=1 npm run build` turns maps on for a local debugging build; never ship one. `npm run build` ends with `node scripts/check-provenance.mjs dist`, which fails on any `.map`, any inline map, or SketchUp document bytes (raw, base64 or compressed) anywhere in the output; `npm run check:provenance <dir>` runs it on another directory, and `pages.yml` runs it on `dist-site` before deploying.

## Desktop (Tauri 2)

`src-tauri/` is configured (`tauri.conf.json`, capabilities, icons from `src-tauri/app-icon.svg`) but not built locally. CI's `tauri-check` job compiles it on Ubuntu; tagging `v*` runs `release.yml`, which builds Windows, macOS and Linux installers into a draft prerelease, unsigned unless signing secrets exist. Once a machine has a linker: `npm run tauri dev`.

Stay on Tauri 2.x stable. 3.0 is alpha; `npm run check:tauri` fails on anything else.

## CI (`.github/workflows/ci.yml`)

- `rust` (ubuntu-24.04): `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, the 10,000-sequence fuzz on the fixed seed `FUZZ_SEED=20261011` (deterministic: a red `main` reproduces locally with the same two variables), wasm32 check; a failing fuzz uploads its shrunk sequence as the `fuzz-repro` artifact.
- `web` (ubuntu-24.04): `npm ci`, Tauri guard, `wasm-pack` build, golden cross-check (`npm run golden:wasm`), boolean differential (`npm run boolean:diff`), `npm run build`, Vitest, `npm run corpus:skp` (regenerates the 200 `.skp` files and fails on any byte difference from `tests/corpus/manifest.json`), then one Playwright Chromium run of every spec with `REQUIRE_KERNEL=1 REQUIRE_CORPUS=1` (the corpus import and its golden images included); the corpus renders are uploaded as `corpus-renders`.
- `tauri-check` (ubuntu-22.04, kept on the older glibc the Linux release build targets), in `src-tauri` with webkit2gtk and libdbus: `cargo fmt --check`, `cargo clippy --locked --workspace --all-targets -D warnings` (this also compiles the shell, so there is no separate `cargo check`), and the device-key tests (`cargo test --locked -p sawhorse-device-key`; the OS keychain test is ignored on runners). The shell uses `tauri-plugin-dialog` and `tauri-plugin-fs` (lane 6) and `tauri-plugin-opener` (lane 8); the lockfile is resolved in WSL, the shell itself is not compiled locally (that needs webkit2gtk).

The e2e step's frame-gap bounds are calibrated to the runner (see "Responsiveness bounds in the end-to-end tests"); the absolute 2,000 ms bound runs nightly on a dedicated runner (`nightly-perf.yml`) when one is registered.

Runners are pinned (never `ubuntu-latest`, which moves to a new Ubuntu on 2026-10-19 without notice in the logs); actions are `actions/checkout@v5` and `actions/setup-node@v5` (Node 24 runtime).

No secrets are needed; no auto-merge is configured.

## Nightly fuzz (`.github/workflows/nightly-fuzz.yml`)

`main`'s CI fuzzes one fixed seed so it stays deterministic; new bugs are hunted nightly instead. Every day at 09:17 UTC (and on demand from the Actions tab, with optional `cases` per shard and `seed_base`) eight shards run `long_fuzz` in a debug build with `FUZZ_REPORT=1`: 6,250 sequences each, 50,000 in all, seeds `base + 1 .. base + 8` where the base comes from the run id. The job is `continue-on-error`, so a find does not turn the workflow red: the run summary lists each shard's seed and verdict, and a failing shard uploads its shrunk sequence as the `fuzz-repro-seed<seed>` artifact.

To keep a find: download the artifact, copy its JSON into `crates/kernel/tests/repro/` under a descriptive name, fix the kernel, and commit both (`tests/repro.rs` replays every file there forever). The same sequence reproduces locally with the shard's seed:

```bash
FUZZ_CASES=6250 FUZZ_SEED=<seed> FUZZ_REPORT=1 bash scripts/cargo.sh test -p sawhorse-kernel --test sequences -- --ignored long_fuzz --nocapture
```

## The public site and the browser demo (`.github/workflows/pages.yml`, lane 14)

https://sawhorse3d.com is GitHub Pages with the "workflow" build type. Every push to `main` runs `pages.yml`: it builds the kernel (`npm run wasm`), assembles the site, checks it, smoke-tests it in Chromium and deploys it. Nothing is committed to a `gh-pages` branch.

| Path on the site | Comes from |
|---|---|
| `/` | `site/` (static HTML and CSS, the mock's tokens and fonts, light and dark, no scripts; screenshots in `site/img/`, each under 400 KB) |
| `/app/` | the Vite app built with `VITE_BASE=<base>app/` and `VITE_WEB_DEMO=1` (the dismissible alpha banner, the Help menu's links, no source maps) |
| `/guide/` | `docs/USER-GUIDE.md`, rendered by `scripts/build-site.mjs` (marked; relative links point at the file on GitHub) |
| `/changelog/` | `CHANGELOG.md`, rendered the same way |
| `/CNAME` | `sawhorse3d.com` |

Locally (port 5214 is this lane's; any free port works):

```bash
npm run wasm                                   # once; the demo refuses to build without the kernel
npm run site                                   # -> dist-site/ (gitignored), base "/"
npm run site:check -- --online                 # links, anchors, outside resources, sizes, identity, download links
PW_CHANNEL=msedge npm run site:smoke           # serves dist-site on 5214, opens it, extrudes a box, saves and reopens
npm run site:serve                             # serve dist-site at http://127.0.0.1:5214/ to look at it
```

The base: `actions/configure-pages` reports `""` once the custom domain is set and `/sawhorse` on the github.io project address; the workflow passes it as `--base` (and `--prefix` to the smoke test), so the demo's script, worker and WASM URLs match wherever it is served. To try the github.io shape locally: `MSYS_NO_PATHCONV=1 node scripts/build-site.mjs --base /sawhorse/ --out dist-site-gh`, then `node scripts/site-smoke.mjs --dir dist-site-gh --prefix /sawhorse/` (Git Bash rewrites a bare `/sawhorse/` argument into a Windows path without `MSYS_NO_PATHCONV=1`).

Rules the checker enforces: no `<script>` on the site pages; the only outside resource is the Google Fonts stylesheet; no analytics, no cookies; the pitch, the licence line, the Trimble notice, the unsigned-builds note and "maintained by @boardfoot" are present; no machine paths anywhere in the output.

**Download links** in `site/index.html` name the exact release assets (`releases/download/<tag>/<file>`). On a new release, update the tag and the file names there. `check-site --online` fails on a link that does not answer; with `--allow-missing-release` (lane 15) a link into a release that is not published yet (its tag page answers 404) is noted and let through, so the site can name the next release's files before the tag lands, and a link into a published release must still answer. The Pages workflow passes plain `--online` since v0.1.0-alpha.2 was published (2026-10-13); add the flag there only while the site names a release that is not out yet, and take it off after publishing. Release files carry the version from `tauri.conf.json`; the v0.1.0-alpha.2 files, as published: `Sawhorse_0.1.0-alpha.2_x64-setup.exe`, `Sawhorse_0.1.0-alpha.2_x64_en-US.msi`, `Sawhorse_0.1.0-alpha.2_aarch64.dmg`, `Sawhorse_0.1.0-alpha.2_amd64.AppImage`, `Sawhorse_0.1.0-alpha.2_amd64.deb` and `Sawhorse-0.1.0-alpha.2-1.x86_64.rpm` (the rpm keeps the `-`; read the names off the release page, never guess them). (The `v0.1.0-alpha.1` files said 0.0.1: they were built before the version bump.) The MSI's own version is numeric: `bundle.windows.wix.version` 0.1.0.2.

**Screenshots** (`site/img/`) are lane 9's walk-through: with `npx vite --port 5214 --strictPort` running, `node scripts/capture-lane09.mjs --url http://localhost:5214 --out <dir> --prefix light` and again with `--theme dark --prefix dark`; stage 0 (the bookshelf) is `app-*.png`, stage 1 (Nesting) is `cutlist-*.png`, each kept at 1440 x 900 and quantised to a 256-colour palette (Pillow's fast octree, no dithering) to stay near 100 to 140 KB.

**Custom domain.** The domain is set on the repository (`PUT repos/sawhorse3d/sawhorse/pages` with `cname`); DNS at the registrar points the apex at GitHub's four A (and four AAAA) addresses and `www` at `sawhorse3d.github.io` (see GitHub's "Managing a custom domain" page). HTTPS enforcement turns on after the certificate is issued, which needs the DNS in place. Until then github.io redirects to a domain that does not reach GitHub yet.

## Troubleshooting

- `link: extra operand ... rcgu.o` on Windows: that is Git's coreutils `link`, not MSVC. Use `scripts/cargo.sh` (WSL).
- `Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)` from a Node script on Windows: a Node 24 teardown bug when calling `process.exit()`; our scripts set `process.exitCode` instead.
- Viewport says WebGL2 is unavailable: hardware acceleration is off in that browser.
- `cargo` missing in WSL: `wsl.exe bash -lc 'rustup default stable'`.
