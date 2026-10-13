# Known limits

What Sawhorse 0.1 alpha handles, measured, and where it stops. Status: lane 16, 2026-10-13 (the limits that protect you, below, are new in lane 16).

## Model size

Measured on the development machine (a desktop PC: 24-thread x86-64 CPU, 32 GB of RAM, a mid-range discrete GPU; Windows 11, Microsoft Edge), importing real SketchUp files through File > Import in the browser build (`e2e/private-fixtures.spec.ts`, "ms" is from choosing the file to the model on screen, kernel included):

| File (SketchUp-authored) | Faces in the file | What came in | Import |
|---|---|---|---|
| A 1.9k-face model (gondola, SketchUp 2020) | 1,887 | 27 solids, 84 instances, 56 meshes | 1.0 s |
| A sofa set (SketchUp 2023) | 13,184 | 14 solids, 32 instances, 20 meshes | 0.9 s |
| An office fit-out (SketchUp 2023) | 13,562 | 275 solids, 409 instances, 374 meshes | 4.5 s |

An earlier run on the same machine (2026-10-12, before named single placements came in as components) measured 1.9k faces in 0.5 s and 13.5k faces with 352 instances in 3.8 s. Treat these as orders of magnitude: a model of a few thousand faces is quick; tens of thousands of faces take seconds to import and the view stays usable; a whole building of hundreds of thousands of faces is beyond what this alpha has been tried on. Every edit re-validates the solids it touches, so editing inside a large component is slower than editing a single board.

### At the model's limit

The model may hold 100,000 solids (below), and the window stays responsive at that size (lane 16 fixes, measured on the same machine, the production build in Edge, `e2e/model-cap.spec.ts`). A plugin made two ordinary array ops land exactly at the limit (a box, 999 copies of it, then 99 copies of all 1,000): the window's longest pause while the 99,000 copies came in was 0.55 s, switching to the Outliner, the Cut list, Materials or Tags and selecting all 100,000 solids each paused it under half a second, and the browser held about 2.2 GB in all (the kernel's share about 0.5 GB). Before the fix the same two ops froze the window for 46 to 54 seconds and took the browser to about 7 GB, and the plugin was stopped for not answering while it was the app that was busy. What made the difference:

- The **Outliner** lists every row but draws only the rows in view and a screenful either side (above 300 rows; the scroll bar is the whole list's). Its rows are worked out in one pass over the model.
- The **Cut list** draws its first 500 rows and 500 more each time you ask; the totals, CSV, PDF, labels, Print and picking rows always cover every row. The print report is built only when you print.
- **Selecting** many solids or groups finds their faces and edges in one pass over the view, not one pass per solid.
- The **plugin watchdog** does not count a pause of the app itself against a plugin: its budget starts again when the app's thread runs again.

A model that large is still slow to work in where every solid is involved: the cut list of 100,000 loose solids is 100,000 rows, and a plugin cannot read the list of all its solids in one message (`model.solids` is refused as `too_large` past the 4 MB message budget; `model.stats` counts them).

## Hard limits (refused with a message)

| What | Limit |
|---|---|
| Coordinates | within 10 km of the origin |
| Smallest feature | 0.01 mm (points closer than that are one point) |
| One imported item | 4,000,000 points or faces |
| One import (a whole file) | 1,500,000 faces, 3,000,000 points, 50,000 objects |
| Texture images | 512 per model, 16 MB and 4096 x 4096 pixels each, 256 MB and 128 million pixels in all |
| An array | 1,000 copies |
| A plugin batch | 2,000 ops (and a work budget, below) |
| Stock library | 200 stocks; kerf up to 1/2 in, trim up to 4 in; stock 1 mm to 10 m |
| Drawing sheets | 200 sheets, 2,000 items per sheet |
| `.shz` thumbnail | 100 KB (a larger one is left out) |

## Limits that protect you

Every limit below is checked where the work it bounds starts, before memory is spent on what a file or a plugin claims: a refused input is answered with a plain reason and leaves the model, the autosave and every plugin's stored data as they were. Lane 16 put them in after an audit of every way a file or a plugin could make the app run out of memory, hang, or stop working. The numbers are in the code next to their reasons (`crates/kernel/src/limits.rs`, `crates/shz/src/lib.rs`, `crates/import/src`, `packages/plugin-api/src/wire.ts`, `src/plugins`, `src/io`, `src/lib/autosave.ts`, `src/kernel/client.ts`).

### Files you open or import

| What | Limit | Why |
|---|---|---|
| A file read whole | `.shz` 512 MB; an import 256 MB; a plugin `.zip` 64 MB | Measured before it is read: the whole file is in memory at least twice (the page and the kernel). |
| `.shz` archive | 2,048 entries; no name twice; no two entries sharing compressed bytes | A zip can list one name twice (readers disagree which wins) or point many entries at one compressed stream (a bomb). |
| `.shz` entries | `scene.json` 256 MB, `geometry.glb` 512 MB, a texture 16 MB, the thumbnail 100 KB, `plugin-data.json` 4 MB; 1 GB in all | Each is refused by the size its header declares, before it is inflated, and is inflated at most one byte past that declaration: a 2 MB file declaring 2 GB is refused in milliseconds. |
| A declared size | must be the real size; at most 1,100 to 1 compressed | A smaller declaration is how a bomb gets past a size check; Deflate cannot compress better than about 1,032 to 1. |
| `.shz` texture rows | 512, no path twice | Counted before any image is read (5,000 rows naming one 1 MB image used to inflate it 5,000 times). |
| `.shz` names | 64 characters (objects, meshes, groups, components, tags, materials, scenes); notes 1,000; texture references 1,024 | Every edit copies the model and every answer to the window carries its names. |
| `.shz` coordinates | mesh points, guides and dimensions within 10 km; cameras within 1,000 km and finite | A finite but huge coordinate makes infinite bounds downstream. |
| `.shz` components | nested at most 64 deep; no definition inside itself; what they derive held to the model limits below | Counted along the definitions before anything is derived: 30 definitions each holding two copies of the next are a billion solids from a few kilobytes. |
| `.3mf` | 4,096 entries; a model part 256 MB; 512 MB in all; model parts named by the package's relationships | Same checks as `.shz`; each part inflated once. What the build expands to through components is counted per object, once, before anything is built. |
| `.skp` (SketchUp 2021 and later are zips) | 512 MB declared and 5,000 entries; 100,000 groups and copies, 3,000,000 points, 1,500,000 faces mapped | Checked from the archive's directory before openskp inflates anything; groups (expanded per placement, unlike components) are counted as they are expanded. |
| `.skp` texture images | 16 MB, 4096 x 4096 pixels, 512; 256 MB and 128 million pixels in all | One the kernel would refuse is left out with a note, so the rest of the model still comes in. |
| One import, any format | 1,500,000 faces; 3,000,000 points; 50,000 objects; 5,000 lines and curves with 250,000 points; component copies counted | Building a solid takes about 2 KB per triangle at its peak, and WebAssembly has 4 GB. Checked by the reader as it reads and by the kernel again before it builds. A sketch takes all of an import's lines in one pass (5,000 lines: half a second). |
| A glTF accessor | its count and stride must fit its buffer (checked in 64-bit arithmetic); 4,000,000 values without a buffer | Checked before anything is allocated: a count of 2^31 is refused at once. |
| A glTF node tree | each node reached once; 64 deep | glTF nodes form a tree; a node listed twice would be walked again for every path to it. |
| A DXF | 500,000 entities | Group codes are read as slices of the file, never copied. |
| An SVG path | 250,000 points | A curve becomes up to 64 points. |
| An STL facet | 1,024 vertices | |
| One solid | 250,000 faces (a larger closed shell comes in as a mesh, with a note) | A solid is checked whole (self-intersection) whenever it changes. |

### The model

| What | Limit | Why |
|---|---|---|
| Solids and meshes | 100,000 | Every edit works on a copy of the model and checks what changed. |
| Groups and component copies | 100,000 | |
| Component definitions | 20,000 | |
| Faces | 5,000,000 | About 0.5 KB each with their edges. |
| Points | 10,000,000 | |
| One sketch | 20,000 edges | A sketch's edges are checked against each other whenever it changes. |
| Texture images | 512; 16 MB, 4096 x 4096 pixels each; 256 MB and 128 million pixels in all | The browser decodes an image to 4 bytes a pixel and keeps mipmaps: a 300 KB PNG can say 16384 x 16384, which is 1 GB decoded; sixteen legal 4096 x 4096 images of one colour (a few hundred kilobytes of `.shz`) were a gigabyte, and 512 of them would have been tens. The pixels of all of them together are counted, from their headers, before any is drawn. |
| What the view decodes of them | each at most 2048 x 2048, all together within 32 million pixels (128 MB), one image at a time; the swatches in the Materials pane are small copies (96 pixels a side) | A texture tiles over a face, and a view of the whole model does not show more. A model with many large images draws each a little coarser; the file keeps them at full size. |
| An array or copy | refused before any copy is made when the copies (and everything inside copied groups and components) would pass the limits above | |
| A section cap | the cut shows every cap exactly, however thin (only a cap under 1e-16 m2, the arrangement's rounding, is not one); Create Group from Slice draws a cap when, with points closer than 10 micrometres (POINT_MERGE) to a point or an edge merged, its outer ring keeps three corners and an area over 1e-10 m2 per metre of the solid's size | Lane 18: one rule (`section::representable`) for what the slice can draw, the same whichever way the plane faces; the slice draws every such cap, and when a plane's caps are all thinner than that it says "the section cut here is thinner than 0.01 mm; nothing to slice" (ADR-0010). |
| An op sent to the kernel | 256 MB of JSON; a plugin batch 32 MB; a replayed history 1 GB | Refused before it is parsed. |
| Undo | 200 steps, and 512 MB of history | Unchanged solids and meshes are shared between the steps (a step is charged only for what it changed); past the budget the oldest steps fall off, but the last step can always be undone. |
| Autosave | 64 MB of history | Past it, autosave stops for that model and says so; save the model as a file. An autosave that cannot be read, or is too large, is set aside and offered for discarding, never loaded, so it cannot keep the app from starting. Save, New and Open clear it. |

### When the kernel fails

| What | Limit | Why |
|---|---|---|
| A kernel failure (a trap: a bug or running out of memory) | the kernel's worker is replaced and the document rebuilt from its history | The model is not lost: the app keeps the history (the op log), and the step that failed changed nothing. The status line says so; undo works as before. A fresh worker that fails while rebuilding is replaced in turn; a history that fails to rebuild twice (it should not: every step in it was applied before) gives way to a new model, and the app says why. |
| A request that runs too long | an import or an open 3 minutes, rebuilding a history 5 minutes, anything else 2 minutes; Cancel on an import or an open | The worker is replaced and the document rebuilt, as for a failure. |
| A kernel that keeps failing | more than 3 restarts in 30 seconds | It is given up on (reload the page), instead of restarting forever; every request is refused at once, none is left waiting. |
| Saving on the desktop | written beside the file, synced, read back and compared, then put in place; the previous file kept as `<name>.bak` | A crash, a power cut or a full disk while saving never leaves a half-written file in place of a good one. |
| Saving anywhere | the kernel reads every `.shz` it writes back through its own reader (directory, checksums, limits, model) before it is handed out | A file that would not open is never written over the last good copy. |

### Plugins

| What | Limit | Why |
|---|---|---|
| One message | 4 MB; `file.save` and the answer to `file.open` 34 MB (a 32 MB file); `net.fetch` 12 MB (a 10 MB body); a plugin's start (its modules) 24 MB; log lines and the watchdog's pings 64 KB | Every message is serialized once, by its sender, into one buffer that is handed over without a copy; the receiver checks its size before reading anything in it (ADR-0009). |
| Values in a message | strings 4 M characters; byte arrays 32 MB (only a view's own bytes are sent, never the rest of its buffer); 1,000,000 values; 64 deep; object keys 256 characters; no `__proto__` key, no key twice; lists without holes or extra properties | Read out of the bytes with every limit applied before anything is allocated: a list claiming 2^32 items in a 30-byte message is refused at once. |
| Requests | 300 a second (bursts of 100), refused with `busy` above that; ten seconds' worth in one second stops the plugin | A plugin cannot keep the app's thread busy with messages. |
| Time the app spends on a plugin | 250 ms of every second; 5 seconds over that in a row stops the plugin | Measured around everything the app does for its messages. |
| Work in flight per plugin | 64 requests; 4 batches; 4 network requests; one file dialog; one prompt or confirm | A plugin's waiting work is bounded, and all of it (queued batches, dialogs, downloads) is dropped when the plugin stops. |
| Waiting on the person | a `file.save`, `file.open`, prompt or confirm is given up after 5 minutes (`timeout`); in the browser `file.open` needs the person's click (`user_activation`) | Lane 17: a file input opened without a click never answers, and held the plugin's one file slot for good. A request given up has its dialog closed and its picker's answer dropped. |
| Saving files | names whose last extension runs when opened (`.exe`, `.bat`, `.js`, `.html` ... `BLOCKED_SAVE_EXTENSIONS`) or that end in a dot or space are refused; in a browser without a save dialog the app asks before each download, naming the plugin, the file and its size | Lane 17: the browser fallback downloaded at once with no dialog at all. |
| Batches | 10 a second; 2,000 ops; ops times things in the model at most 20,000,000 | Each op costs about the size of the model it is applied to. |
| Storage | 256 KB per plugin and 1 MB for all plugins (this browser); 64 KB per plugin and 1 MB for all plugins (each document); JSON values only | Measured without building the text (a byte array used to be turned into a 400 MB string to be measured). |
| Panels | 100 fields; 5,000 table cells, 256 K characters of text, 4 images of 512 KB in all per panel; images PNG, JPEG, GIF or WebP of at most 2048 x 2048 pixels (read from their headers) | Every cell is drawn; an image is decoded by the browser. SVG is refused (its size is not in a header). |
| Tool views | 200 polylines of 5,000 points, 50,000 points in all, 100 labels | Checked once, when it arrives for the tool in use. |
| Prompt, confirm, notify | title and label 80 characters, value 500, text 1,000, button 32, notify 500; text only | Refused when longer, never cut (a cut can keep a huge text alive). |
| `net.fetch` | answers of 10 MB (refused on Content-Length before reading, else read as a stream and cut off at 10 MB; an answer with no stream is refused over 10 MB too); 30 s; 50 MB a minute; only the manifest's origins (scheme, host and port), no wildcard on a public suffix | Lane 17: the allowlist checked the host alone (`localhost` reached every local port), and a wildcard like `*.vercel.app` reached every tenant. |
| The console | lines cut to 2,000 characters; 100 lines a second; 500 kept; the Plugins pane shows 600 characters of an error | |
| Packages | 2,000 files, 50 MB, 8 MB per file (measured before reading); modules 4 MB each and 16 MB in all; at most 20 problems listed | A module of 100,000 forbidden names used to cost 100,000 scans of the whole module. A package over 256 KB is parsed and digested in a worker (60 s at most), never on the app's thread. |
| Package file names | no drive (`C:`), device (`con.js`), alternate stream (`a.json:x`) or trailing dot or space; no two names that differ only in case; every file exactly the size and checksum its zip says | Installed as files on the desktop; installed into a staging folder and swapped in, so a failed install leaves the old version. |

## SketchUp files that do not open

`.skp` files are read by openskp 1.3.0. It reads SketchUp 2013 to 2026 saves, with these known refusals (the file is refused with openskp's message and the model is unchanged):

- **A real SketchUp 2018 save** (`{18.0.16975}`): "legacy .skp parse failed: base probe: anchor resolved to null". Other saves in the 2013 to 2020 range open (2014, 2017 and 2020 have been tried). Reported upstream: [draft issue](upstream/openskp-2018-parse.md).
- **SketchUp 2012 and older** (SketchUp 3, 6 and 7 test files): outside openskp's stated range.

What comes across from a file that opens, and what does not: [skp-compat.md](skp-compat.md).

## Hand-made furniture that is not watertight

Most furniture drawn in SketchUp is open faces, not closed solids. It opens (as amber meshes), its groups and components are listed in the cut list sized from their bounds and flagged "not solid (sizes from bounds)", and they nest like any part. A group that holds a whole piece of furniture as one set of faces is sized as a whole: when its box is thicker than any board it is listed but not counted or nested, and a flat face is never sheet goods. The cut list does not split a piece of furniture into its boards for you; group each board's faces first. They cannot be pushed or pulled, and STL and 3MF export refuse them by name; GLB, OBJ and DXF export them.
