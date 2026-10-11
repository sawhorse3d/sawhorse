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
- **Who:** a Sonnet research agent in the planning session (kernel-crate survey for the Ultra-Plot), NOT the lane-1 builder. The builder read neither Hew nor hew3d/openskp.
- **What:** cloned github.com/hew3d/hew (AGPL-3.0-only + plugin exception) into the session scratchpad and read docs/agents/ARCHITECTURE.md, docs/INTEROP.md and the public source of ops.rs, topo.rs, validate.rs, sketch.rs, boolean.rs, tessellate and mesh-heal, to answer "which Rust crates and what architecture" for the plan.
- **Influence:** the design positions in ADR-0001 (pure kernel to WASM in the webview; two-tier Sketch/Object sticky geometry; editing-context-scoped stickiness; validator-first, transactional apply, typed errors; faceID round-trip through a manifold boolean) follow Hew's PUBLISHED architecture notes. Ideas and architecture are not copyrightable expression; no Hew code, comments or docs text were copied into this repository.
- **Evidence:** lane-1 verifier (2026-10-11) ran an overlap check without reading the Hew text: 8 of 1,221 long lines identical, all generic idioms (SlotMap field declarations, a clamp-projection formula, union-find setup); best sequence-similarity cluster 0.12; max 10.5% of any file's lines also present in some Hew file. `git grep -i 'hew3d\|hew\|openskp'` hits only these docs.
- **Naming proximity:** `PushPullError`, `StickyError`, `RenderMesh`, `loop_half_edges` exist in both codebases. Not protectable, but lane 2 renames them to Sawhorse-specific names to remove any doubt.
- **Remediation:** the scratchpad clone and the extracted architecture note were deleted on 2026-10-11 before lane 2. Rule from here: no agent clones or reads Hew source; Hew's published docs may be cited by URL only, and every citation is logged here in the same commit as the code it influenced.

### 2026-10-11 — lane 2: naming-proximity renames done
- **Who:** the lane-2 builder (Claude, Opus), kernel-core lane.
- **What changed:** `PushPullError` -> `PushError`, `StickyError` -> `SketchError`, `RenderMesh` -> `ViewMesh`, `loop_half_edges` -> `half_edges_of_loop`; for consistency also `PushPullOutcome` -> `PushOutcome`, `render_mesh` -> `view_mesh`, and the `KernelError` variants `Sticky`/`PushPull` -> `Sketch`/`Push`. `git grep -E 'PushPullError|StickyError|RenderMesh|loop_half_edges'` now matches only this log.
- **Read:** nothing from hew3d/hew or hew3d/openskp (source, docs or issues) was opened for the rename or for any lane-2 work.
