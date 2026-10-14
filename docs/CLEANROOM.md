# Clean-room log

Sawhorse is MIT OR Apache-2.0. Two related projects carry copyleft licences that are incompatible with that:

| Project | Licence | Allowed use |
|---|---|---|
| hew3d/hew | AGPL | Design reference only |
| hew3d/openskp (the GitHub repository) | GPL | Design reference only |

(npm `openskp`, MIT, is a separate package and may be a dependency per ADR-0002. Its committed test fixtures have no stated terms and are never redistributed.)

## The rule

1. **Never clone, copy, vendor, transliterate or paste** code, tests, fixtures, shaders, icons or data from the projects above. Not into this repo, not into a scratch file that later becomes this repo, not into a prompt for an agent that writes this repo.
2. Reading their **public documentation, issues or design write-ups** for ideas is allowed, and **every reading is logged below before any related code is written**: date, who (person or agent and model), exactly what was read (URL + commit or date), what idea was taken, and which of our files it informed.
3. Whoever read the material does not write the matching implementation in the same session when it can be avoided; when it cannot, say so in the entry.
4. If code from either project is ever found in this repository, stop, remove it, log the incident here, and tell the maintainer.
5. Agents: treat this file as part of your instructions for this repo. If you were asked to look at Hew or hew3d/openskp, log it here in the same commit as anything it influenced.

## Log

| Date | Who | What was read (URL, commit/date) | Idea taken | Our files informed |
|---|---|---|---|---|
| (none yet) | | | | |

Lane 1 (foundations, 2026-10-11): nothing from either project was opened or read. The half-edge B-rep, validator, sketch region walk, push/pull "extend" branch and tests were written from general computational-geometry knowledge (Euler-Poincare, Newell normals, Shewchuk predicates via `robust`, minimal-circuit face walks) and the decisions in `docs/adr/`.

## Log

### 2026-10-10 17:53 — survey agent read Hew (disclosed 2026-10-11)
- **Who:** a research agent in the maintainer's planning session (kernel-crate survey), NOT the lane-1 builder. The builder read neither Hew nor hew3d/openskp.
- **What:** cloned github.com/hew3d/hew (AGPL-3.0-only + plugin exception) into the session scratchpad and read docs/agents/ARCHITECTURE.md, docs/INTEROP.md and the public source of ops.rs, topo.rs, validate.rs, sketch.rs, boolean.rs, tessellate and mesh-heal, to answer "which Rust crates and what architecture" for the plan.
- **Influence:** the design positions in ADR-0001 (pure kernel to WASM in the webview; two-tier Sketch/Object sticky geometry; editing-context-scoped stickiness; validator-first, transactional apply, typed errors; faceID round-trip through a manifold boolean) follow Hew's PUBLISHED architecture notes. Ideas and architecture are not copyrightable expression; no Hew code, comments or docs text were copied into this repository.
- **Evidence:** lane-1 verifier (2026-10-11) ran an overlap check without reading the Hew text: 8 of 1,221 long lines identical, all generic idioms (SlotMap field declarations, a clamp-projection formula, union-find setup); best sequence-similarity cluster 0.12; max 10.5% of any file's lines also present in some Hew file. `git grep -i 'hew3d\|hew\|openskp'` hits only these docs.
- **Naming proximity:** `PushPullError`, `StickyError`, `RenderMesh`, `loop_half_edges` exist in both codebases. Not protectable, but lane 2 renames them to Sawhorse-specific names to remove any doubt.
- **Remediation:** the scratchpad clone and the extracted architecture note were deleted on 2026-10-11 before lane 2. Rule from here: no agent clones or reads Hew source; Hew's published docs may be cited by URL only, and every citation is logged here in the same commit as the code it influenced.

### 2026-10-11 — lane 2: naming-proximity renames done
- **Who:** the lane-2 builder (Claude, Opus), kernel-core lane.
- **What changed:** `PushPullError` -> `PushError`, `StickyError` -> `SketchError`, `RenderMesh` -> `ViewMesh`, `loop_half_edges` -> `half_edges_of_loop`; for consistency also `PushPullOutcome` -> `PushOutcome`, `render_mesh` -> `view_mesh`, and the `KernelError` variants `Sticky`/`PushPull` -> `Sketch`/`Push`. `git grep -E 'PushPullError|StickyError|RenderMesh|loop_half_edges'` now matches only this log.
- **Read:** nothing from hew3d/hew or hew3d/openskp (source, docs or issues) was opened for the rename or for any lane-2 work.

### 2026-10-11 — lane 3 (feel): nothing read
- **Who:** the lane-3 builder (Claude), interaction lane: face ids, camera, selection and erase, inference, the Value Control Box, the draw tools, push/pull feel, tape, guides and dimensions.
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No design question needed them.
- **SketchUp:** its public behaviour (keys, the push/pull gesture, typing mid-gesture, inference kinds, window vs crossing selection, guide lines from the tape) was described from memory as a user knows it. No SketchUp icon, UI string or layout was copied; the glyphs, wording and layout are the approved Sawhorse mock's, and the inference colours are ours (endpoint lime, midpoint teal, on-edge amber, on-face violet; the added kinds in magenta, blue, indigo, grey, tan and sage; see `docs/USER-GUIDE.md`).
- **Check:** `git grep -i -E 'hew3d|\bhew\b|openskp'` matches only this log, `docs/DECISIONS.md` and ADR-0001/0002 (the MIT npm package and the rule itself).

### 2026-10-11 — lane 4 (transforms): nothing read
- **Who:** the lane-4 builder (Claude), transforms lane: Move, Rotate, Scale, Offset, Follow Me, the kernel's transform, copy, array, offset, sweep, severance and plane-snapping code, and the lane-3 verifier conditions.
- **Read:** nothing from hew3d/hew or hew3d/openskp. No source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published URL of any kind was read for this lane.
- **Third-party code used, not copied:** `cavalier_contours` 0.9.0 (MIT OR Apache-2.0, crates.io) is a dependency for Offset, as ADR-0001 names it. To call it, its public API (`Polyline`, `Shape::parallel_offset`, `ShapeOffsetOptions`) and its `Cargo.toml` (licence and features) were read in the local cargo registry. Nothing was copied out of it.
- **SketchUp:** its public behaviour was described from memory, as a user knows it: Move with Ctrl for copies and `x3` / `/4` arrays, the Rotate protractor and arrow-key plane locks, Scale grips with Shift / Ctrl, Offset, Follow Me with a selected path, and autofold. No SketchUp icon, UI string or layout was copied. The wording, grips, protractor drawing and colours are Sawhorse's own.
- **Algorithms** were written from general geometry: affine maps and the inverse-transpose for normals, Rodrigues rotations, mitered sweeps on bisector planes (with Pappus's theorem for the test volumes), union-find shells for severance, ray-parity point-in-solid, least-squares plane intersections for snapping.
- **Check:** `git grep -i -E 'hew3d|\bhew\b|openskp'` matches only this log, `docs/DECISIONS.md` and ADR-0001/0002.

### 2026-10-11 — lane 6 (file import/export): what was read
- **Who:** the lane-6 builder (Claude, Opus), file import/export lane.
- **Read:** nothing from hew3d/hew or hew3d/openskp (no source, docs, issues, design write-ups or URLs). No published web page was opened for this lane.
- **npm `openskp@1.3.0` (MIT, a dependency per ADR-0002):** read its packaged `README.md` and `dist/index.d.ts` to learn the API, and two helpers in `dist/index.mjs` (`transformPoint`, `reconstructLoopVertices`) to confirm two conventions our mapping depends on: an instance's 12-number placement is a row-major 3x3 followed by the translation, and a co-edge with `orientation === 1` starts at the edge's `v1`. `src/io/skp.ts` is our own code written against those conventions; nothing was copied. The package has no LICENSE file in the tarball; its `package.json` and README state MIT. Its README links to the author's GitHub repository (iamahsanmehmood/openskp); it was not opened.
- **Finding that shaped the corpus:** openskp's writer starts every file from an embedded blank SketchUp document (`SCAFFOLD_BASE64` in `dist/index.mjs`, a `{17.0.1}` model with a "Boulder (CO)" location), i.e. a SketchUp-authored template, the thing ADR-0002 says we must replace before shipping `.skp` export. So the corpus `.skp` files are generated on demand (locally and in CI) and are not committed; only the generator, a SHA-256 manifest and the reference renders are. See `tests/corpus/README.md`.
- **SketchUp behaviour** (components vs groups, reversed faces, Layer0, hidden instances) was described from general user knowledge.
- **Other dependencies read for their APIs** (permissive licences, not copyleft): the `threemf` 0.8 crate (to learn why it does not see prefixed `m:basematerials`), and the `tauri` 2.12 and `tauri-plugin-fs` 2.7 crate sources in the cargo registry (to confirm `FsExt::fs_scope`, `Scope::allow_file` and `DragDropEvent::Drop`). Nothing was copied.
- **Check:** `git grep -i -E 'hew3d|\bhew\b'` matches only this log, `docs/DECISIONS.md` and ADR-0001/0002.

### 2026-10-11 — lane 8 (render service and credit store): nothing read
- **Who:** the lane-8 builder (Claude Opus 5.5).
- **Read:** nothing from hew3d/hew or hew3d/openskp, and no published URL of any kind. The Stripe, fal, Replicate and Resend integrations were written from general knowledge of those public APIs. The Stripe webhook signature check uses the official `stripe` npm SDK, and its own type definitions in `node_modules` were consulted. The fal and Replicate request field names are marked for verification against the live schemas before launch (`docs/LAUNCH-CREDITS.md`). Package versions were looked up in the crates.io and npm registries; no documentation was read.
- **Check:** `git grep -i -E 'hew3d|\bhew\b|openskp'` matches only this log, `docs/DECISIONS.md` and ADR-0001/0002.

### 2026-10-11 — integration (lanes 4, 6 and 8 merged; fuzz fix; CI): nothing read
- **Who:** the integrator (Claude), merging lanes 4, 6 and 8 into `main`, fixing the imprint bug CI's fuzzer found, and reshaping CI.
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published URL of any kind was read, and no third-party source (including the npm `openskp` package and the cargo registry) was opened.
- **Written from general knowledge:** the triangulator's fallback (`ear_clip` in `crates/tessellate`) is textbook ear clipping with holes bridged at their rightmost vertex to a visible outer vertex; the merge resolutions only combined the lanes' own code.
- **Check:** `git grep -i -E 'hew3d|\bhew\b'` matches only this log, `docs/DECISIONS.md` and ADR-0002.

### 2026-10-11 — lane 5 (groups, components, tags, scenes): what was read
- **Who:** the lane-5 builder (Claude), groups and components lane.
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published web page was opened for this lane.
- **npm `openskp@1.3.0` (MIT, a dependency per ADR-0002):** read the `Definition`, `Instance`, `SkpModel`, `Layer` and `Page` declarations in its packaged `dist/index.d.ts` to map definitions and placements onto Sawhorse components, and parsed our own generated corpus files with it to see how its writer names group definitions. Nothing was copied; `src/io/skp.ts` is our own code.
- **SketchUp behaviour** (groups vs components, editing in place with the rest of the model faded, Make Unique, Explode, the Outliner, layers as visibility only, scenes as tabs) was described from memory, as a user knows it, and from the lane brief. The Outliner and Tags layout follows the approved Sawhorse mock; the glyphs, wording, the faded context and the dashed context box are our own.
- **Algorithms** (templates captured and re-derived by slot, affine maps of B-reps, a tidy pass, the definition-reachability purge) were written from general knowledge.
- **Check:** `git grep -i -E 'hew3d|\bhew\b'` matches only this log, `docs/DECISIONS.md` and ADR-0002.

### 2026-10-12 — lane 12 (section planes and drawing sheets): what was read
- **Who:** the lane-12 builders (Claude, Opus), sections and sheets lane (one session built steps 1 to 7, a second resumed it for the verification, the docs and the merge).
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published web page of any kind was opened for this lane.
- **npm `openskp@1.3.0` (MIT, a dependency per ADR-0002):** read the `SectionPlane` declaration (`plane: [a, b, c, d]`, `name`, `label`, `hidden`) and the `sectionPlanes` fields of its definitions in the packaged `dist/index.d.ts`. `src/io/skp.ts` maps them with our own code; nothing was copied.
- **SketchUp and LayOut** (the Section Plane tool, active cuts, section fill, Create Group from Slice; LayOut's viewports, scales, dimensions, labels, title blocks and its PDF export) were described from memory, as a user knows them. No SketchUp or LayOut source, documentation, template, title block, icon or UI string was read or used; the tool, the pane, the sheet layout, the title block and every word are our own.
- **Algorithms** from general knowledge: plane and polygon clipping with a consistent tie rule, clipping triangles by the signs of their corners (the classic marching-triangles table), winding numbers on a planar arrangement, Douglas and Peucker line simplification, ring assembly and nesting by containment, hidden-line removal by interval subtraction against triangles in view space (a 2D BVH), the projective map for perspective views, drawing scales and architect scales, shop-fraction formatting, PDF 1.4 (the lane-7 writer), SVG 1.1, DXF R12-style entities (the subset `crates/export/src/dxf.rs` writes), the PNG `pHYs` chunk.
- **Check:** `git grep -i -E 'hew3d|\bhew\b'` matches only this log, `docs/DECISIONS.md` and ADR-0002.

### 2026-10-12 — lane 11 (the plugin API): what was read
- **Who:** the lane-11 builder (Claude), plugins lane.
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published web page of any kind was opened for this lane. The SketchUp Ruby API's source and documentation were not opened, and no code from any SketchUp extension was read.
- **SketchUp extensions** (the Extension Manager, extensions adding menus, toolbars, tools and dialogs, cabinet and joinery extensions, line-boring for shelf pins) were described from memory, as a user knows them. The API, its names, the manifest, the sandbox and the three samples are our own design and words.
- **Third-party code used, not copied:** `fflate` (MIT, already a dependency) for zips. The permission sets of `tauri-plugin-fs` 2.7.0 (`permissions/read-all.toml`, `write-all.toml`, the generated `appdata` sets) and the `recursive` option of `tauri-plugin-dialog` 2.8.2's `open` (`src/commands.rs`) were read in the local cargo registry to choose the capabilities; nothing was copied.
- **Written from general knowledge:** Web Worker isolation and its globals, removing properties along prototype chains, blob-URL module linking, a small JavaScript scanner (comments, strings, templates, regular expressions), JSON Schema, request/response RPC with ids and a ping watchdog.
- **Check:** `git grep -i -E 'hew3d|\bhew\b'` matches only this log, `docs/DECISIONS.md` and ADR-0002.

### 2026-10-12 — lane 9 (nesting and cutting diagrams): what was read
- **Who:** the lane-9 builder (Claude), nesting lane.
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published web page of any kind was opened for this lane.
- **OpenCutList, CutList Plus, MaxCut and Cutlist Optimizer** (cut list and sheet-optimising tools) were described from memory, as a user knows them: a stock library of sheet sizes and board sizes per material, kerf and trim settings, diagrams with numbered parts, a cut order, efficiency figures, DXF and SVG for CNC. Nothing from any of them was opened, copied or consulted; the layout of the Nesting section, the Stock dialog, the diagrams, the booklet and every word in them are our own.
- **Algorithms** were written from general knowledge: guillotine bin packing with free-rectangle splits (best area fit, shorter-leftover and rip-first split rules), maximal-rectangles packing with best short side fit and containment pruning, first-fit-decreasing for one-dimensional cutting, a bounded depth-first subset search for the lookahead, recursive guillotine decomposition of a layout by gap sweeps, DXF R12 group codes (`LWPOLYLINE`, `POLYLINE`/`VERTEX`/`SEQEND`, `TEXT`, the `LAYER` table; the same subset `crates/export/src/dxf.rs` writes), SVG 1.1, and the lane-7 PDF writer.
- **Third-party code used, not copied:** `dxf-parser` (MIT, already a dev dependency) reads our DXF back in tests; its API was already in use in `src/io/formats.test.ts`.
- **Check:** `git grep -i -E 'hew3d|\bhew\b'` matches only this log, `docs/DECISIONS.md` and ADR-0002.

### 2026-10-12 — lane 7 (cut list): what was read
- **Who:** the lane-7 builder (Claude), cut list lane.
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published web page of any kind was opened for this lane.
- **OpenCutList and CutList Plus** (cut list tools for SketchUp and standalone) were described from memory, as a user knows them: parts as components, quantities from instances, grain, sheet goods, labels. Nothing from either was opened, copied or consulted; the pane follows the approved Sawhorse mock, and the wording, layout, rules and outputs are our own.
- **Algorithms** were written from general knowledge: minimal oriented boxes by face-normal candidates and rotating calipers over a 2D convex hull (Andrew's monotone chain), principal axes by cyclic Jacobi rotations, RFC 4180 CSV, and a PDF 1.4 file written from the format's public structure (objects, content streams, the standard Type 1 fonts with WinAnsi encoding, a cross-reference table). The Helvetica advance widths are the font's public metrics, used as numbers for text measurement. Avery 5160 geometry (3 x 10 labels, 2 5/8 x 1 in, 1/2 in top margin, 3/16 in side margins) is the sheet's published size, known as a user knows it.
- **Check:** `git grep -i -E 'hew3d|\bhew\b'` matches only this log, `docs/DECISIONS.md` and ADR-0002.

### 2026-10-12 — lane 10 (materials, styles and shadows): what was read
- **Who:** the lane-10 builder (Claude, Opus), materials and styles lane.
- **Read:** nothing from hew3d/hew or hew3d/openskp: no source, docs, issues or design write-ups were cloned, fetched or opened, and no URL from either is cited. No published web page of any kind was opened for this lane, and no texture or image was downloaded.
- **npm `openskp@1.3.0` (MIT, a dependency per ADR-0002):** read the `Face` (`materialId`, `uvTransform`, `uvProjected`), `Material` and `Texture` declarations and the `addTextureMaterial` / `AddFaceOptions` declarations in its packaged `dist/index.d.ts`, and three lines of `dist/index.mjs` (the `tileW` / `tileH` lines of its glTF builder) to confirm that a texture's `width` and `height` are its tile size in inches. `src/io/skp.ts` maps them with our own code (the face basis and the affine fit are written from the d.ts description and general linear algebra); nothing was copied.
- **SketchUp** (the Paint Bucket and Alt to sample, painting a closed group or component, texture positioning pins, the Styles and Shadows windows, scenes saving a style) was described from memory, as a user knows it. No SketchUp material, texture, style, icon or UI string was used; the library's textures are drawn by our own code (`docs/THIRD-PARTY.md`), and the pane layouts, preset names and wording are ours.
- **Algorithms** from general knowledge: value noise on a periodic lattice, PNG (filters and zlib through `flate2`), the solar declination and hour-angle formulas for the sun, silhouette edges by the signs of the two face normals against the view, shadow mapping (three.js), affine fitting by a 3 x 3 solve.
- **Check:** `git grep -i -E 'hew3d|hew'` matches only this log, `docs/DECISIONS.md` and ADR-0002.

### 2026-10-12 — provenance audit of the shipped bytes (openskp's blank template)
- **Who:** the maintainer's orchestrating session, after a competitive read noted that npm `openskp`'s writer ships a 5,796-byte blank-document template its maintainer says was generated with the official SketchUp SDK without Trimble's permission (iamahsanmehmood/openskp issue #415, 2026-10-11/12).
- **Method:** scanned every file of `node_modules/openskp` and of our built `dist/` for the SketchUp document header (`SketchUp Model` in UTF-16LE) raw, inside base64 literals and inside gzip/deflate streams, and for the VFF container marker.
- **Found:** the template lives as a base64 literal in `openskp/dist/index.js` and `index.mjs` (5,796 bytes decoded, header present). Our shipped JavaScript does NOT contain it: tree-shaking drops the writer (`dist/assets/skp-*.js` is clean). It DID appear in `dist/assets/skp-*.js.map`, the source map, because Vite emitted maps for every build except the public demo, and Tauri bundles `dist/` as is. So the v0.1.0-alpha.1 installers carried the template inside a source map; the public site never did (maps off for the demo; confirmed 404 on the live site).
- **Fix (this commit):** source maps are opt-in (`SAWHORSE_SOURCEMAP=1`), `npm run build` ends with `scripts/check-provenance.mjs dist`, which fails on any `.map`, inline map, or SketchUp document bytes (raw, base64 or compressed), and `pages.yml` runs it on `dist-site`. v0.1.0-alpha.2 is the first release built under the tripwire.
- **Position:** we only READ `.skp` through the MIT reader; no template, no SketchUp-authored bytes and no SDK are shipped. The reader itself is openskp's clean-room-claimed code (see ADR-0002); the attorney read on `.skp` interop stays open, and `.skp` export stays unbuilt until it closes and the maintainer's own blank template exists.
- **Corpus:** the 200 generated `.skp` files are never committed (only the generator and the SHA manifest are); they exist on disk only in test runs.

### 2026-10-12 — public CI fixtures: five CC BY 4.0 SketchUp files (lane 15)
- **What:** `tests/fixtures/skp-ccby/` holds five furniture models (`benches.skp`, `coffee tables.skp`, `cupboard.skp`, `desk.skp`, `side table.skp`) by **Corynth**, from https://corynth.itch.io/free-assets-sketchup-3d-assets, under Creative Commons Attribution 4.0 as stated on that page; unmodified, SHA-256 listed in `ATTRIBUTION.md` and checked by `e2e/skp-ccby.spec.ts` before use. They are the only SketchUp-authored files in the repository.
- **Why:** real hand-made furniture (open faces, not watertight) is what the cut list from non-solid parts (lane 15) is for; CI needs it without the maintainer's private fixtures, which stay outside the repository.
- **Where they never go:** they are test inputs only. They are not imported by the app's code, so the build never bundles them; `npm run build` ends with `scripts/check-provenance.mjs dist` and the Pages workflow runs it on the assembled site, both refusing any SketchUp document bytes, so neither the installers nor the site can carry them.
- **Credit:** README's Credits and the attribution file name the author, the source and the licence.

