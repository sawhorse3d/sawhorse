# Sawhorse

**[sawhorse3d.com](https://sawhorse3d.com)** · **[Try it in your browser](https://sawhorse3d.com/app/)** (nothing to install, nothing uploaded) · [Download the desktop app](https://github.com/sawhorse3d/sawhorse/releases) (v0.1.0-alpha.1, unsigned for now) · [User guide](https://sawhorse3d.com/guide/)

A free, local-first, open-source push/pull 3D modeler for woodworkers and makers. Draw a rectangle, push it up, type `3/4` and press Enter. Sawhorse keeps the keys your hands already know, adds a built-in cut list, exports watertight files for printing and CNC, and runs in a browser tab with nothing to install, or as a desktop app.

**Status: pre-alpha.** Lanes 1 to 3 are done: the app shell from the approved design; a robust geometry kernel (sketches, push/pull on any face, booleans, undo) compiled to WebAssembly; and the feel: navigation, selection, inference snapping, typed values, Line / Rectangle / Circle / Polygon / Arc, Push/Pull, Tape with guides, and Dimension. You can model a bookshelf with dados by hand. Move / Rotate / Scale / Offset / Follow Me, groups and components, the cut list and file import/export land over the next lanes.

## Run the browser build

Needs Node 24+ and, for the kernel, Rust with the `wasm32-unknown-unknown` target and `wasm-pack` (see `docs/DEVELOPMENT.md`; on a Windows machine without Visual Studio Build Tools the Rust step runs in WSL automatically).

```bash
npm install
npm run wasm      # build the kernel to WebAssembly
npm run dev       # open http://localhost:5173
```

Try it: press `R` (Rectangle), type `36, 12` and press Enter; press `P` (Push/Pull), type `3/4` and press Enter: a board. Click the unit label in the bottom bar to switch between fractional inches, decimal inches, ft-in, mm, cm and m; a typed suffix (`19mm`, `2'6"`) always wins. Middle-drag orbits, right-drag pans, the wheel zooms. The ten-minute tour, every key and the inference colours: [`docs/USER-GUIDE.md`](docs/USER-GUIDE.md).

A production build: `npm run build:all` (kernel + site into `dist/`, servable from any static host).

## What is inside

| Path | What |
|---|---|
| `src/` | React 18 + TypeScript + three.js front end (the same build runs in the browser and in Tauri) |
| `crates/kernel` | Half-edge B-rep with hole loops, sketches, extrude, push/pull, topology validator, transactional ops |
| `crates/tessellate` | Triangulation with holes (earcut) and 2D polygon booleans (i_overlay) |
| `crates/inference` | Picking and inference snapping: own BVH, pixel tolerance at depth, occlusion, ranked inference kinds, locks |
| `crates/export` | Watertight-only binary STL and 3MF |
| `crates/wasm-api` | wasm-bindgen surface the front end's kernel worker loads |
| `src-tauri/` | Tauri 2 desktop shell |
| `site/` | The public site's landing page; `scripts/build-site.mjs` adds the demo, the guide and the changelog (`docs/DEVELOPMENT.md`) |
| `docs/` | `USER-GUIDE.md`, `DECISIONS.md` (index) and `adr/`, `DEVELOPMENT.md`, `CLEANROOM.md`, the approved UI mock |

## Tests

```bash
npx vitest run                                   # front end
bash scripts/cargo.sh test --workspace           # kernel: unit, property, fuzzer, repro, golden
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test
```

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option. Contributions are accepted under the same terms. Sawhorse does not contain or call the Trimble SketchUp SDK; see `docs/CLEANROOM.md` for the clean-room rule.

## Identity

Sawhorse is published pseudonymously by its maintainer under the project account. Copyright is held by the Sawhorse contributors under MIT OR Apache-2.0.
