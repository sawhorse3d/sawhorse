# SketchUp (.skp) test corpus

203 `.skp` files made by openskp's MIT writer from Sawhorse's own procedural models (200 cycling through ten families, then, since lane 23, a circle, an arc and a polygon written as SketchUp curves), used to test `.skp` import end to end in the browser build.

| Path | Committed | What |
|---|---|---|
| `../../src/io/corpus/models.ts` | yes | The generator: ten families (boxes, tables, bookshelves, houses with windows, stairs, curved parts, nested and mirrored components, loose geometry, an open tray, a reversed face), each seeded by the file's index. GUIDs and timestamps the writer would randomise are seeded too, so every file is reproducible byte for byte. |
| `../../scripts/corpus-skp.mjs` | yes | Writes the files here and checks them against the manifest (`npm run corpus:skp`; `-- --update` rewrites the manifest). |
| `manifest.json` | yes | Every file's name, family, size and SHA-256, the openskp version, and face and definition counts. |
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
