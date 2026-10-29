# SketchUp (.skp) test corpus

203 `.skp` files made by openskp's MIT writer (version 1.4.0 since lane 25; the manifest records which) from Sawhorse's own procedural models (200 cycling through ten families, then, since lane 23, a circle, an arc and a polygon written as SketchUp curves), used to test `.skp` import end to end in the browser build.

| Path | Committed | What |
|---|---|---|
| `../../src/io/corpus/models.ts` | yes | The generator: ten families (boxes, tables, bookshelves, houses with windows, stairs, curved parts, nested and mirrored components, loose geometry, an open tray, a reversed face), each seeded by the file's index. GUIDs and timestamps the writer would randomise are seeded too, so every file is reproducible byte for byte. |
| `../../scripts/corpus-skp.mjs` | yes | Writes the files here and checks them against the manifest (`npm run corpus:skp`; `-- --update` rewrites the manifest). Each run prints which openskp writer wrote the files and which one the manifest records, and says so when they differ. |
| `manifest.json` | yes | Every file's name, family, size and SHA-256, the openskp version whose writer made them, and face and definition counts. |
| `skp/` | **no** (gitignored) | The 203 generated files, about 2.1 MB. |
| `golden/` | yes | Reference renders of the first 20 files (the sample: two of each family). |

Why the `.skp` files are not committed: openskp's writer starts every file from a blank document SketchUp itself saved (its embedded scaffold). ADR-0002 keeps SketchUp-authored bytes out of this repository until our own blank template replaces that scaffold, so the files are generated wherever they are needed (locally and in CI) instead. Nothing here comes from 3D Warehouse, from OpenSKP's own test fixtures, or from any SketchUp model.

## Running it

```bash
npm run wasm && npm run build          # the browser build the harness drives
npm run corpus:skp                     # write tests/corpus/skp/ and check the manifest
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npm run e2e:corpus
UPDATE_GOLDEN=1 PW_CHANNEL=msedge REQUIRE_KERNEL=1 npm run e2e:corpus   # after an intended change
```

`e2e/corpus.spec.ts` imports every file through File > Import's input on a fresh model, frames it in the Iso view (Zoom Extents), and renders the canvas to `test-results/corpus/<name>.png`, with the HUD and legend hidden. Every file must import (no refusal), leave every solid watertight, and (except the open trays) become at least one solid. The sample is compared with `golden/` by a perceptual diff: both images are box-filtered to 96 x 72 luminance cells; a file fails when the mean cell difference exceeds 0.03 or more than 1% of cells differ by over 0.25. The harness also checks the metric is not vacuous (two different sample models must fail it). `test-results/corpus/report.json` has per-file and per-family results.

Measured on 2026-10-11 (Windows, Edge): 200 files, 706 solids, 40 non-solid meshes (the 20 open trays and the 20 loose drawings), about 130 ms per import including the render, 1.1 minutes for all 200. The same renders in SwiftShader (software WebGL, as on CI runners) differ from the references by a mean of at most 0.011, a third of the threshold.

CI: the `corpus` job in `.github/workflows/ci.yml` regenerates the corpus (failing on any byte difference from the manifest), runs the harness on Chromium, and uploads the renders.

## The writer: openskp 1.4.0 (lane 25, 2026-10-28)

The corpus was written by openskp 1.3.0's writer until lane 25 bumped the reader to 1.4.0, whose writer marks a group's definition as a group (SketchUp's `SUComponentType_Group`; 1.3.0 wrote every group's definition as a component's). **Decision: regenerate all 203 with the 1.4.0 writer and a new manifest** (option (a) of the lane's brief), rather than freeze the 1.3.0-written set as fixtures beside a 1.4.0 set. Why: the generator is deterministic (seeded GUIDs and timestamps), so regenerating costs one command and minutes, not a day; one set keeps CI, the goldens and the manifest simple; and the 1.3.0-written bytes stay reproducible from git history (check out the commit before lane 25 and run `npm run corpus:skp`) for anyone who needs to compare.

What changed, measured before the manifest was rewritten:

- **166 files changed bytes**, exactly the ones with a group (the 37 without one, the nested-components family, ten stairs, four curved parts and the three curve files, are byte for byte the same). Sizes did not change (the flag is a field the 1.3.0 writer already wrote as 0); the total is still 2,270,277 bytes.
- **Every file imports to the same op.** Each of the 203 files written by 1.3.0 and by 1.4.0 was mapped by the lane-25 importer (`skpToOp`) and the two kernel ops compared as JSON: identical for all 203, and so the same groups (0), components (387), placements (663), faces (3,659) and curves handed to the kernel (43: the three curve files and two in each of the 20 loose-geometry files). openskp 1.4.0 reads 273 group definitions from the new files (`isGroup`) and none from the old; Sawhorse does not use the flag yet ([#16](https://github.com/sawhorse3d/sawhorse/issues/16)), which is why nothing else moved. (The writer names a group's definition after the group, so the importer's name rule calls them components; SketchUp-saved files name them `Group#N`.)
- The golden sample renders were compared again after the change (the loose-geometry samples now draw their curves as sketch lines rather than mesh lines; see `e2e/corpus.spec.ts`).
