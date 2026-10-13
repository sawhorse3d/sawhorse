# Known limits

What Sawhorse 0.1 alpha handles, measured, and where it stops. Status: lane 15, 2026-10-12.

## Model size

Measured on the development machine (a desktop PC: 24-thread x86-64 CPU, 32 GB of RAM, a mid-range discrete GPU; Windows 11, Microsoft Edge), importing real SketchUp files through File > Import in the browser build (`e2e/private-fixtures.spec.ts`, "ms" is from choosing the file to the model on screen, kernel included):

| File (SketchUp-authored) | Faces in the file | What came in | Import |
|---|---|---|---|
| A 1.9k-face model (gondola, SketchUp 2020) | 1,887 | 27 solids, 84 instances, 56 meshes | 1.0 s |
| A sofa set (SketchUp 2023) | 13,184 | 14 solids, 32 instances, 20 meshes | 0.9 s |
| An office fit-out (SketchUp 2023) | 13,562 | 275 solids, 409 instances, 374 meshes | 4.5 s |

An earlier run on the same machine (2026-10-12, before named single placements came in as components) measured 1.9k faces in 0.5 s and 13.5k faces with 352 instances in 3.8 s. Treat these as orders of magnitude: a model of a few thousand faces is quick; tens of thousands of faces take seconds to import and the view stays usable; a whole building of hundreds of thousands of faces is beyond what this alpha has been tried on. Every edit re-validates the solids it touches, so editing inside a large component is slower than editing a single board.

## Hard limits (refused with a message)

| What | Limit |
|---|---|
| Coordinates | within 10 km of the origin |
| Smallest feature | 0.01 mm (points closer than that are one point) |
| One imported item | 4,000,000 points or faces |
| Texture images | 512 per model, 16 MB each |
| An array | 1,000 copies |
| A plugin batch | 10,000 ops |
| Stock library | 200 stocks; kerf up to 1/2 in, trim up to 4 in; stock 1 mm to 10 m |
| Drawing sheets | 200 sheets, 2,000 items per sheet |
| `.shz` thumbnail | 100 KB (a larger one is left out) |

## SketchUp files that do not open

`.skp` files are read by openskp 1.3.0. It reads SketchUp 2013 to 2026 saves, with these known refusals (the file is refused with openskp's message and the model is unchanged):

- **A real SketchUp 2018 save** (`{18.0.16975}`): "legacy .skp parse failed: base probe: anchor resolved to null". Other saves in the 2013 to 2020 range open (2014, 2017 and 2020 have been tried). Reported upstream: [draft issue](upstream/openskp-2018-parse.md).
- **SketchUp 2012 and older** (SketchUp 3, 6 and 7 test files): outside openskp's stated range.

What comes across from a file that opens, and what does not: [skp-compat.md](skp-compat.md).

## Hand-made furniture that is not watertight

Most furniture drawn in SketchUp is open faces, not closed solids. It opens (as amber meshes), its groups and components are listed in the cut list sized from their bounds and flagged "not solid (sizes from bounds)", and they nest like any part. They cannot be pushed or pulled, and STL and 3MF export refuse them by name; GLB, OBJ and DXF export them.
