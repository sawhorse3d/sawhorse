# Sawhorse

**[sawhorse3d.com](https://sawhorse3d.com)** · **[Try it in your browser](https://sawhorse3d.com/app/)** (nothing to install, nothing uploaded) · [Download the desktop app](https://github.com/sawhorse3d/sawhorse/releases/tag/v0.1.0-alpha.4) (0.1.0-alpha.4, unsigned for now) · [User guide](https://sawhorse3d.com/guide/)

A free, local-first, open-source push/pull 3D modeler for woodworkers and makers. Draw a rectangle, push it up, type `3/4` and press Enter. Sawhorse keeps the keys your hands already know, works out a cut list and lays your parts out on real sheets and boards as you draw, opens SketchUp files, and exports watertight files for printing and CNC. It runs in a browser tab with nothing to install, or as a desktop app on Windows, macOS and Linux. No account, no subscription, no tracking.

**Status: alpha (0.1.0-alpha).** Everything below is built and tested; the "Not yet" list is honest. Expect rough edges and report them: [issues](https://github.com/sawhorse3d/sawhorse/issues).

## What it does today

**Modeling**
- Line, Rectangle (and rotated), Circle, Polygon, Arc and 3-point Arc (a circle, arc or polygon is one curve: selected, moved and erased whole); Push/Pull on any face (through, into and flush with other parts); Move, Rotate, Scale with copies and arrays, for sketch shapes too before they are pulled; Offset; Follow Me; Erase.
- Inference snapping (endpoints, midpoints, edges, faces, axes, parallel and perpendicular, guides) with arrow-key locks; exact typed values in fractions, decimals, feet and inches or metric (`1-1/2"`, `1 1/2`, `1.5"`, `38mm`, `2'6"`).
- Tape measure with guides, linear dimensions.
- Groups and components (edit one copy, all follow; Make Unique), tags, scenes, an Outliner.
- Materials with textures you can position, styles (hidden line, X-ray, shop drawing ...), shadows from a sun by date and time.
- Soft and smooth edges (the Eraser's Ctrl, Shift and Ctrl+Shift; Soften by angle), so round parts read round.
- Section planes with exact filled cuts; drawing sheets with scaled viewports, dimensions, labels, a cut-list table and a title block, out as PDF, SVG, DXF and PNG.
- A geometry kernel in Rust (compiled to WebAssembly) that validates the model after every operation: solids stay closed.

**For the shop**
- **Cut list**: parts from components, solids and groups, with quantities through nesting, finished T x W x L, grain, stock, board feet on the rough (nominal) lumber you buy, sheet goods by area; parts that are not watertight (most hand-made SketchUp furniture) are sized from their bounds and flagged. CSV, a printable report, PDF, and labels.
- **Nesting**: parts laid out on real sheets and boards from a stock library that knows nominal and actual thickness (3/4 ply is 23/32, 4/4 S4S is 3/4), with kerf, trim and grain; cutting diagrams and the cut order as SVG, DXF (for CNC and laser) and a PDF booklet.

**Files**
- Opens `.skp` (SketchUp 2013 and later, read on your machine by the MIT-licensed openskp library; [what comes across](docs/skp-compat.md)), STL, 3MF, glTF/GLB, OBJ, DXF and SVG.
- Saves `.shz` (an open, documented format: [docs/format.md](docs/format.md)); exports STL and 3MF (watertight only: an open mesh is refused by name), GLB, OBJ, DXF and SVG.

**Plugins**: tools, panels and commands in sandboxed JavaScript with declared permissions; three samples come with the app. Writing one: [`docs/PLUGINS.md`](docs/PLUGINS.md).

## Not yet

So nobody finds out the hard way:

- **`.skp` export** (you can open SketchUp files, not save them).
- **Fillet and chamfer** tools (Follow Me can route a chamfer).
- **Copy and paste in place**.
- **Back and Bottom views** (Top, Front, Right, Iso and perspective are there).
- **Text, and radius or diameter dimensions in the model** (linear dimensions only; drawing sheets have radius and diameter).
- **Weld Edges**: erasing the line that split a circle leaves two arcs.
- **Signed installers**: Windows and macOS warn before the first launch ([how to get past it](docs/releases/v0.1.0-alpha.4.md#unsigned-installers)).
- **Render credits**: an AI render service exists in the code but is not launched; the app shows nothing about it unless you point it at a service of your own (Edit > Settings).

Known limits (model size, `.skp` versions that do not open): [docs/LIMITS.md](docs/LIMITS.md).

**Security and signing**: the installers are unsigned today; how they will be signed and who approves a release, what the app sends (nothing, unless you set up a render service or grant a plugin the network) and how to uninstall: [docs/SIGNING.md](docs/SIGNING.md).

## Run the browser build

Needs Node 24+ and, for the kernel, Rust with the `wasm32-unknown-unknown` target and `wasm-pack` (see `docs/DEVELOPMENT.md`; on a Windows machine without Visual Studio Build Tools the Rust step runs in WSL automatically).

```bash
npm ci
npm run wasm      # build the kernel to WebAssembly
npm run dev       # open http://localhost:5173
```

Try it: press `R` (Rectangle), type `36, 12` and press Enter; press `P` (Push/Pull), type `3/4` and press Enter: a board. Click the unit label in the bottom bar to switch between fractional inches, decimal inches, ft-in, mm, cm and m; a typed suffix (`19mm`, `2'6"`) always wins. Middle-drag orbits, right-drag pans, the wheel zooms. The ten-minute tour, every key and the inference colours: [`docs/USER-GUIDE.md`](docs/USER-GUIDE.md).

A production build: `npm run build:all` (kernel + app into `dist/`, servable from any static host).

## What is inside

| Path | What |
|---|---|
| `src/` | React 18 + TypeScript + three.js front end (the same build runs in the browser and in Tauri) |
| `crates/kernel` | Half-edge B-rep with hole loops, sketches and their curves, sketch moves, push/pull, booleans, groups and components, cut list, nesting, sections and sheets, soft edges; a validator, transactions and a fuzzer |
| `crates/tessellate` | Triangulation with holes (earcut) and 2D polygon booleans (i_overlay) |
| `crates/inference` | Picking and inference snapping: own BVH, pixel tolerance at depth, occlusion, ranked inference kinds, locks |
| `crates/import`, `crates/export`, `crates/shz` | File readers and writers, the `.shz` format |
| `crates/wasm-api` | The wasm-bindgen surface the front end's kernel worker loads |
| `packages/plugin-api` | The plugin API's types, runtime and checks |
| `src-tauri/` | Tauri 2 desktop shell |
| `site/` | The public site's landing page; `scripts/build-site.mjs` adds the demo, the guide and the changelog |
| `docs/` | The user guide, decisions (`DECISIONS.md` and `adr/`), development, file format, `.skp` compatibility, limits, plugins, clean-room log |

## Tests

```bash
npx vitest run                                   # front end
bash scripts/cargo.sh test --workspace           # kernel: unit, property, fuzzer, repro, golden
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test
```

## Credits

- SketchUp files are read by [openskp](https://www.npmjs.com/package/openskp) (MIT). Third-party code and licences: [`docs/THIRD-PARTY.md`](docs/THIRD-PARTY.md).
- The public CI fixtures in `tests/fixtures/skp-ccby/` are furniture models by **Corynth**, from [Corynth's free assets](https://corynth.itch.io/free-assets-sketchup-3d-assets), under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), unmodified ([attribution](tests/fixtures/skp-ccby/ATTRIBUTION.md)).

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option. Contributions are accepted under the same terms. Sawhorse does not contain or call the Trimble SketchUp SDK; see `docs/CLEANROOM.md` for the clean-room rule. SketchUp is a trademark of Trimble Inc.; Sawhorse is not affiliated with Trimble.

## Identity

Sawhorse is published pseudonymously by its maintainer under the project account. Copyright is held by the Sawhorse contributors under MIT OR Apache-2.0.
