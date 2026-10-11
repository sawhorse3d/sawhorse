# Sawhorse

A free, local-first, open-source push/pull 3D modeler for woodworkers and makers. Draw a rectangle, push it up, type `3/4` and press Enter. Sawhorse keeps the keys your hands already know, adds a built-in cut list, exports watertight files for printing and CNC, and runs in a browser tab with nothing to install, or as a desktop app.

**Status: pre-alpha.** Lane 1 (foundations) is done: the app shell from the approved design, the geometry kernel's data model and test machinery, and an end-to-end path from a typed value through the Rust kernel (compiled to WebAssembly) to the 3D view. Drawing tools, real push/pull on any face, inference snapping, groups, the cut list and file import/export land over the next lanes.

## Run the browser build

Needs Node 24+ and, for the kernel, Rust with the `wasm32-unknown-unknown` target and `wasm-pack` (see `docs/DEVELOPMENT.md`; on a Windows machine without Visual Studio Build Tools the Rust step runs in WSL automatically).

```bash
npm install
npm run wasm      # build the kernel to WebAssembly
npm run dev       # open http://localhost:5173
```

Try it: press `P` (Push/Pull), type `3/4`, press Enter, and the kernel rebuilds the board 3/4 in thick. Press `R`, type `36,12`, Enter, and the rectangle changes. Click the unit label in the bottom bar to switch between fractional inches, decimal inches, ft-in, mm, cm and m; a typed suffix (`19mm`, `2'6"`) always wins. Middle-drag orbits, right-drag pans, the wheel zooms.

A production build: `npm run build:all` (kernel + site into `dist/`, servable from any static host).

## What is inside

| Path | What |
|---|---|
| `src/` | React 18 + TypeScript + three.js front end (the same build runs in the browser and in Tauri) |
| `crates/kernel` | Half-edge B-rep with hole loops, sketches, extrude, push/pull, topology validator, transactional ops |
| `crates/tessellate` | Triangulation with holes (earcut) and 2D polygon booleans (i_overlay) |
| `crates/inference` | Inference vocabulary and snapping (grows in the inference lane) |
| `crates/export` | Watertight-only binary STL and 3MF |
| `crates/wasm-api` | wasm-bindgen surface the front end's kernel worker loads |
| `src-tauri/` | Tauri 2 desktop shell |
| `docs/` | `DECISIONS.md` (index) and `adr/`, `DEVELOPMENT.md`, `CLEANROOM.md`, the approved UI mock |

## Tests

```bash
npx vitest run                                   # front end
bash scripts/cargo.sh test --workspace           # kernel: unit, property, fuzzer, repro, golden
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test
```

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option. Contributions are accepted under the same terms. Sawhorse does not contain or call the Trimble SketchUp SDK; see `docs/CLEANROOM.md` for the clean-room rule.
