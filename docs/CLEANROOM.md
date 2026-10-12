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
