# Changelog

All notable changes to Sawhorse are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/) once releases start.

## [Unreleased]

### Fixed (lane 20, tidy, 2026-10-13)

- **A whole piece of furniture drawn as one closed solid is not a board.** Corynth's coffee table 6 (one closed 16 x 24 x 48 in block) counted 128 board feet of 4/4, and the gondola's floor (0.39 x 98 x 150 in) 102. Three more kinds of part are now listed with their reason and **not counted** (no board feet, area or sheet; not nested, the nesting says why): a part **thicker than any board and over 12 in both ways**, solid or not (a coffee table, a carcass: 24 x 24 x 18 in); a **solid thicker than any board with more than 30% of its box empty** (a lamp, a stool); and **lumber wider than 4 ft** (no board is; mark it sheet goods if it is a sheet). A 2 x 12 x 96 board, a 1 x 48 x 96 sheet, a 6 x 6 post and a gusset still count. The Cut list's header says how many pieces are not counted and why. Kernel: `Uncounted::{WholePiece, Hollow, TooWide}` and the same `UnplacedReason`s; the cut list and nesting hashes of goldens holding such blocks changed (their document hashes did not).

### Fixed (lane 16b, from the lane 16 re-attack, 2026-10-13)

- **Selecting all of 100,000 solids no longer holds the window**, on any machine. `main`'s CI was red on it: the window paused 19 to 31 s on GitHub's runners (0.5 s on the development machine). Profiled under a 6x CPU slowdown and with the GPU emulated as on the runners, the time went to the highlight: it built 1,200,000 thick lines for the selected solids' edges and drew them every frame (8.1 s a frame emulated), recoloured every triangle and rebuilt the textured overlays on every selection or hover change, and the menus, the Outliner and the cut list made a key string for each selected entity. The faces' and edges' shaders now read the highlight from two state textures (a byte per face, a byte per solid) and a change writes only the bytes that change; past 20,000 segments the selected edges are drawn as the edges' own thin lines in the selection colour (thick lines as before below that). The view mesh's lookups (each solid's faces and edges) are built once per model change, in idle time. Select all then nothing at the limit: 5.2 s to 0.62 to 0.72 s at 6x slowdown, 8.6 s to 0.63 to 0.73 s emulated, and a frame with everything selected 8.1 s to 0.56 s emulated.
- **The end-to-end tests' responsiveness bounds hold on any machine.** Each spec measures the machine it runs on (a fixed JavaScript workload, and one render of the scene) and states its bound in those units, between a floor of 1.5 s and a ceiling of 20 s that fails a freeze anywhere (`e2e/perf.ts`, docs/DEVELOPMENT.md). An absolute 2,000 ms bound runs nightly on a dedicated runner when one is registered (`nightly-perf.yml`; it says why it skipped otherwise).
- **Guides, dimensions, tags, materials, scenes and section planes have model limits** (50,000; 50,000; 10,000; 4,096; 1,000; 1,000; sheets keep 200 and 2,000 items each). A file of a few megabytes could list millions of them: 2,000,000 guides in a 5 MB `.shz` crashed the tab, 1,000,000 materials took the browser to 6.2 GB. The `.shz` reader now counts what `scene.json` lists as it inflates, without building any of it, and refuses the file at the first element past a limit (the rest is never inflated): each of those files is refused in 15 to 45 ms, and the kernel's memory grows by the file and the few megabytes read before the refusal. A `.skp` is checked by what openskp read before it is mapped; an import, a scene and every edit are checked by the kernel (a typed refusal, nothing changed); a plugin batch is counted before it is sent and refused `too_large`, as is a batch the kernel refuses for a limit.
- **A plugin package's file over 8 MB is refused by the size its zip declares**, before it is inflated (it was inflated first, then refused).

### Fixed (lane 18, section caps and the slice, 2026-10-13)

- **The section cut keeps every cap again, however thin** (ADR-0010, lane-18 amendment). Lane 17 dropped a ring of the cut when its points, welded within 0.01 mm, left fewer than three corners, and with it real slivers 0.125 to 0.5 m long and a few micrometres wide (up to 2.4e-6 m2): the cut showed nothing there while the solid was cut, and the halves were open by that much. The cut, the fill, the halves and the exports now read every cap exactly as the plane cuts it.
- **Create Group from Slice no longer depends on which way the plane faces, and never refuses a cap it can draw.** What the slice can draw is one rule (`section::representable`): a cap's rings with points closer than 0.01 mm merged the way the sketch merges them (a point near another point or near an edge), keeping three corners and some area. A 226 cm2 cap whose tip had edges of 10 and 1.5 micrometres was refused as "that shape is too small (under 0.01 mm)" with the plane one way and sliced with it flipped; it now slices both ways, its tip merged. Caps of solids a micrometre apart are merged where they meet, as the sketch merges them, so they slice together (the fuzzer found two boxes 1 micrometre apart whose slice was refused). A plane whose every cap is thinner than that says "the section cut here is thinner than 0.01 mm; nothing to slice" instead of refusing as if it cut nothing.
- The property tests and the fuzzer check the rule: the slice covers every representable cap, refuses as too thin only when no cap is representable, and the caps' area is the solid's cross-section for every cap. The three finds of the lane-17 verification are pinned in the property tests' regressions and asserted exactly.

### Fixed (lane 16 fixes, from the lane 16 verification, 2026-10-13)

- **The window stays responsive at the model's 100,000-solid limit.** A plugin's two ordinary array ops landing exactly at the limit (a box, 999 copies, then 99 copies of all 1,000) left the document intact but froze the window for 46 to 54 seconds, took the browser to about 7 GB, and got the plugin stopped for not answering. The Outliner worked out its rows by filtering every list for every container (quadratic: 20 s) and drew a page node for each of 100,000 rows (20 s more); it now indexes the tree once and draws only the rows in view and a screenful either side (above 300 rows), with the whole list's scroll height. The Cut list draws its first 500 rows and more on request (the totals, exports, labels, Print and picking rows always cover every row), and the print report is built only when printing. Selecting all found each solid's faces and edges with a pass over the whole view mesh per solid (and a spread of a million edges overflowed the call stack); a selection's highlight is now one pass, worked out when the selection or the model changes, with the pointer's hover added on top. The scene graph's lookups (a solid's parent, a group's contents) are indexed once per graph. Measured in the production build: the longest pause is 0.55 s while the copies come in, and under half a second for each pane and for selecting all; the browser holds about 2.2 GB (`e2e/model-cap.spec.ts` replays the attack).
- **The plugin watchdog does not blame a plugin for the app's own pause**: when the app's thread was held for more than three pings, a plugin's silence is counted from when the app runs again; one really stuck is still stopped 15 s later.
- **Texture images are held to 128 million pixels in all** (eight of 4096 x 4096), counted from their headers before any is drawn, as well as 256 MB: sixteen legal 4096 x 4096 images of one colour (a .shz of a few hundred kilobytes) took the browser from 480 MB to over 1.5 GB, and the 512-image limit allowed tens of gigabytes. Such a file is now refused with the reason; a SketchUp import leaves the images past the limit out, with a note. The viewport decodes images one at a time, each at most 2048 a side and all of them within 32 million pixels (a model of many large images draws each a little coarser; the file keeps them whole), and lets an image's pixels go when the model no longer holds it. The Materials pane's swatches are small copies (96 pixels a side), no longer the images themselves decoded whole.

### Security (lane 17, plugin permissions, 2026-10-13)

A review of every path by which data or effects cross between a plugin and the app, against what the enable dialog says (ADR-0009, lane-17 amendment). Every finding fixed, each with a test that fails on the old code.

- **A plugin tool sees the model only with "Read the model".** Pointer events used to carry the snap and the pick (solid and face ids, corner positions) to a plugin with only `ui`. Without `model.read` a tool now gets the pointer and the eye ray, `picked: null`, and `snap: null` or the point of its own plane or locked line, worked out from the ray and its own values; the marker shows that point.
- **Saving in a browser without a save dialog asks first.** A plugin's `file.save` in Firefox, Safari or `?io=basic` used to download at once; the app now asks in its own dialog, naming the plugin, the file and its size. Names that would run when opened (`.exe`, `.bat`, `.js`, `.html` ...) are refused. `file.open` without the person's click is refused at once (`user_activation`) instead of waiting forever, and a save, open or dialog nobody answers is given up after 5 minutes (`timeout`).
- **`net` is checked by origin.** The allowlist compared the host alone, so `localhost` reached every local port. Entries are now origins (`api.example.com` is https on port 443; `https://api.example.com:8443`, `http://localhost:3000`; plain `localhost` is any port of it, and the dialog says so), and a wildcard on a public suffix (`*.vercel.app`, `*.github.io`, `*.co.uk`, `*.com` ...) is refused. **Breaking for plugins:** a bare host no longer reaches other ports of it; list `https://host:port`.
- **The dialog says what `model.read` allows**: "and change the selection". A plugin that may write but not read hears only which op of a refused batch failed, not the kernel's reason (which names the model).
- **A plugin cannot pass for the app**: its tool's status line and hint start with its name, and a plugin may not be named "Sawhorse" or "Sawhorse ...".
- **No consent bypass in the page**: `window.__sawhorse.plugins` (which could enable a plugin with no dialog) exists only in development and test builds; `npm run build` and `npm run site:check` fail a build that has it. Playwright runs against its own build with the hooks (`scripts/e2e-serve.mjs`).
- **A grant never grows by itself**: it records the hosts the person saw, and shrinks when a plugin's files ask for less, so a permission dropped and asked for again shows the dialog again.
- Method, event, colour and inference-kind lookups use own keys only (`"constructor"` was a colour).
- **A plugin's batch stops growing at its limit**: `b.op` throws `too_large` at the op past 2,000 or past 4 MB, instead of serializing a runaway loop's million ops for seconds (long enough, on a busy machine, for the watchdog to stop the plugin).

### Fixed (lane 17, section slices, 2026-10-13)

- **Create Group from Slice and the section cut agree on what a cap is** (ADR-0010, lane-17 amendment). One rule decides both: a ring is a cap's only when some edge is longer than 10 micrometres, it keeps three corners with closer points welded, and its area is over 1e-10 m2 per metre of the solid's size. A speck of a cap the cut used to show (4.8e-12 m2, a corner poking 4 micrometres through the plane) is no longer shown, so the slice's "cuts nothing here" matches what you see; a cap whose tip had three points within ten micrometres is now sliced into a closed face (it was left as an open loop while the app said its faces were ready to push). The slice draws the caps' own rings, so a cap bounded by another active plane closes too. Both finds from the lane-12d verification's long property run are named repros.

### Fixed (lane 12d, from the lane 12c verification, 2026-10-13)

- **Section caps are assembled from the plane's arrangement, not chained.** Every piece where a section plane crosses a face is now laid in a planar arrangement with the side its material is on (the Sketch tier's planar-graph machinery: the same robust predicates and face walk, at a tolerance that moves no corner); pieces are split where they cross or touch and merged where they coincide; each face of the arrangement is material when the pieces around it agree, and a ray cast against the solid decides when they do not; the cap is the union of the material faces, both halves are closed by it, and every ring comes out simple and closed by construction. Two finds of the lane 12c verification are fixed with it: a box sheared a micrometre and split near the plane lost its whole cap (a face lying within its own bend of the plane gave a piece that pointed the wrong way, and the head-to-tail chain could not close the ring; the kept half was 6.6% short), and a frame cut by a plane along the top edge of its hole lost the notch on the cut-away side (the hole's edge lay along the outline without meeting it at a node, so the cap was a plain rectangle; the halves were 24% short), with plane snapping on and off. A face lying within its own bend of the plane (a face is planar only to 1 micrometre) is now kept or cut whole, like one within a nanometre of it: a box with a top corner moved 0.3 micrometres up and in, cut through its other three top corners, otherwise left a dent no cap could close (found by the new property tests, which cut folded boxes, frames along a hole edge, solids touching at a corner, faces sheared or bent within a micrometre of the plane, planes through one to four vertices, nested holes and planes exactly on a face). Section fills, Create Group from Slice and the slice exports get the right polygons; every golden keeps its hash.

### Security (lane 16, resource hardening, 2026-10-13)

An audit of every way a file you open or a plugin you run could make the app run out of memory, hang, or stop working; every finding fixed, each limit checked where the work it bounds starts, and listed with its reason in [docs/LIMITS.md](docs/LIMITS.md#limits-that-protect-you).

- **Plugin messages are measured before they are read.** Every message between the app and a plugin is serialized once, by its sender, into one buffer handed over without a copy; the receiver checks its size against a budget (4 MB; 34 MB for `file.save` and `file.open`, 12 MB for `net.fetch`) before reading anything in it, then reads the values with every limit applied before anything is allocated (ADR-0009, amended). A plugin can no longer make the app hold 400 MB with one `storage.set`, copy a 1.5 GB buffer behind a 1-byte view, loop the app over a sparse list of length 2^32, or pass the sandbox's own checks and change its data afterwards. A request or an answer that does not fit is a typed refusal (`"too_large"`) that settles the plugin's call, never a dropped message and a plugin waiting forever.
- **A plugin cannot keep the app busy.** At most 300 messages a second (ten seconds' worth in one second stops it), 10 batches a second, 64 requests, 4 batches, 4 downloads, one file dialog and one prompt waiting at once, and a quarter of every second of the app's time; a plugin over these is slowed with `"busy"` or stopped and reported. Stopping a plugin now drops its queued batches, closes its dialog and cancels its downloads. **Stop** is in the Plugins pane whenever a plugin runs.
- **Plugin storage, panels, dialogs and the network have totals**, not just per-value caps: 1 MB of stored data for all plugins in a browser and 1 MB in a document (measured without building the text), a panel budget (5,000 table cells, 256 K characters, 4 images of 512 KB, images read from their headers and at most 2048 x 2048 pixels, no SVG), prompt and confirm text refused when over length instead of cut, `net.fetch` answers refused on their declared size or cut off at 10 MB, 50 MB a minute. Log lines and the error the Plugins pane shows are cut.
- **Plugin packages**: file names that are a drive, a device, an alternate stream or end in a dot or space are refused, so are names that differ only in case and any file whose bytes are not the size and checksum its zip declares; a desktop install is written to a staging folder and swapped in (a failed install keeps the old version); a folder is measured before it is read; a module over 4 MB (16 MB in all) is refused unparsed, and at most 20 problems are listed (a module of 100,000 forbidden names was 100,000 scans of itself); a package over 256 KB is parsed and digested in a worker, so installing one, and starting with it installed, no longer freezes the window for seconds.
- **`.shz` files are read within caps.** At most 2,048 entries, no name twice, no shared compressed bytes; each entry refused by its declared size and ratio before it is inflated, inflated at most one byte past its declaration, and required to be exactly that size (a 2 MB file declaring 2 GB is refused in milliseconds; one lying smaller stops one byte past the lie); `scene.json` parsed straight into the model's types; texture rows counted and checked for repeats before any image is read; names, coordinates and cameras checked; components counted along their definitions before anything is derived (thirty definitions each holding two copies of the next, a billion solids from a few kilobytes, are refused at once). `plugin-data.json` is written and read by the kernel with the rest of the file; the app never unzips a user's file.
- **Imports are read within budgets.** 1,500,000 faces, 3,000,000 points and 50,000 objects per file, 5,000 lines and curves; glTF accessors checked against their buffers before allocating (a count of 2^31 no longer traps the kernel) and a short face-label list no longer reads past its end; glTF node trees and 3MF component trees counted, not walked again per path; 3MF read through the same archive checks, each model part inflated once; DXF read without copying its text; SVG paths capped; a sketch takes all of an import's lines and curves in one pass, its regions found once (5,000 lines took 35 s in a test build, half a second now); a SketchUp zip's directory checked before openskp inflates anything, groups counted as they are expanded, and texture images too large for the kernel left out with a note instead of refusing the whole import; a file is measured before it is read.
- **The model has limits** (100,000 solids and meshes, 100,000 groups and instances, 20,000 definitions, 5,000,000 faces, 10,000,000 points, 20,000 edges per sketch, 256 MB of textures and 4096 x 4096 pixels per image), checked before every edit commits, and before an array or copy makes its copies; a plugin batch is held to 2,000 ops and a work budget; an op sent to the kernel is refused unread over 256 MB (a batch 32 MB).
- **Undo history costs what changed.** Solids, meshes and sketches are shared between the model and its undo steps until they change (each edit used to copy the whole model twice), the history is held to 512 MB, the log hash is kept as ops are logged (every answer used to serialize the whole log), and a solid's self-intersection check and a sketch's edge checks run once per change, through a grid instead of every pair (a 20,000-triangle shell used to be a billion comparisons per edit; classifying the shells of an import was quadratic in the shells).
- **A kernel failure no longer loses the session.** A kernel bug or running out of memory in the middle of an edit (a WebAssembly trap) used to leave every later request refused and Save broken until reload; the app keeps the document's history, starts a fresh kernel, rebuilds the document (undo included) and says so. A request that runs too long (an import or open over 3 minutes) is handled the same way, and **Cancel** stops a long import or open. A fresh kernel that fails while rebuilding is replaced in turn; a history that fails twice gives way to a new model, with the reason; past three restarts in half a minute the kernel is given up on and every request is refused at once, never left waiting.
- **Autosave never blocks the start.** It keeps at most 64 MB of history (and says when a model is too large for it); what it finds at start is set aside before this session saves, so the first edit no longer overwrites an unanswered offer; a record it cannot read, or one too large, is offered for discarding and never loaded. Save, New and Open clear it, and a model still as it was saved or opened keeps none.
- **Saving on the desktop never leaves a half-written file**: written beside it, synced, read back and compared, then put in place, the previous file kept as `<name>.bak` (`src-tauri/safe-save`); everywhere, the kernel reads every `.shz` it writes back through its own reader before handing it out, so a file that would not open is never written over a good one.

### Fixed (lane 12c, from the lane 12 re-verification, 2026-10-13)

- **Section cuts through a folded solid are exact.** Where a section plane passes a vertex at which the surface crosses it twice (a box with part of it moved and folded, a face lying on the plane touching a fold at a corner), the cut's pieces meet four at a time and the cap chain could run one backwards: the cap folded over itself or vanished and the kept halves did not make the whole (three finds of the long fuzz with plane snapping off, seeds 20261803, 20261904, 20261907). The cap is now traced head to tail, taking the leftmost way on at such a vertex and splitting rings where they touch, so every cap ring is simple; the section fill, Create Group from Slice and the exports get the right polygons. The fuzzer now also checks that no cap ring passes a point twice and that each half is watertight (its clipped faces and its cap close, `Model::kept_part`).
- **A pending fuzzer find no longer breaks everyone's tests**: the fuzzer writes it to the gitignored `crates/kernel/tests/repro/pending/`, which the replay test never reads (a `pending-*.json` left in `tests/repro/` is skipped with a warning); `tests/repro/README.md` says how to promote one. A find made with plane snapping off records `"plane_snap": false` and replays that way.
- **No render-service request when none is set**: a development build no longer falls back to the local service's default address, so with no service configured the app asks nothing and logs no refused connection; with one configured it asks once per start (React's development double mount shares the request).
- **Sheet items are picked by what they draw**: a dimension, a label, a line or a rectangle's outline takes a click only on its lines, text and arrowheads, a viewport, a table or an image anywhere in its frame, the topmost first, and a selected rectangle or label anywhere in its box. A dimension's bounding box no longer steals clicks inside a viewport.
- **A viewport keeps its scale through Isometric**: sending it to Isometric (or a perspective camera, or a scene made perspective) remembers the scale it had (`ortho_scale` in `.shz`, optional), and going back to Top or Front gives it back.
- **Dimensions in an isometric or perspective viewport read the model**: an aligned dimension the true length, an angle the true angle, a radius or diameter the true circle; horizontal and vertical ones read "NTS". Never a foreshortened length.
- **Two quick column ticks on a cut-list table keep each other**: the second built its column list from the sheet as shown and dropped the first; it now builds on the edit still on its way, as the pane's other fields do.

### Added (lane 15, alpha.2 readiness, 2026-10-12)

- **Cut list and nesting from parts that are not solids.** A group or component instance whose faces are not a watertight solid (most hand-made SketchUp furniture) is now a part: the cut list first tries a heal (gaps under 0.1 mm welded, inconsistent faces turned, isolated slivers dropped), and if the faces still do not close it sizes the part from the minimal box around them (T x W x L, grain along the length unless set) and flags it **not solid (sizes from bounds)** in the pane, the CSV, the report and its PDF, the labels and the nesting booklet. Board feet, the sheet reading and the nesting use that size. Faces outside any group are still never parts; the pane counts them ("312 faces outside parts") and **Make parts from loose faces** (kernel op `group_loose`, one undo step) puts each edge-connected piece into a group of its own.
- **Include per part** (`set_include`): untick a row to keep it listed but out of the totals, the labels and the nesting; saved with the model, hashed only when set.
- **Stock math: nominal versus actual.** Every stock in the library has a nominal thickness (what the yard calls it) and an actual one (what it measures). The built-in library now carries the thicknesses a yard sells: "3/4" plywood 23/32 in, "1/2" 15/32, "1/4" 7/32; Baltic birch 18 and 12 mm; 4/4 boards 3/4 in S4S (was 13/16), 5/4 1 1/16, 6/4 1 5/16, 8/4 1 3/4, 12/4 2 3/4. The Stock dialog has **Nominal** for sheets and boards and **Actual thickness**, filled in from the nominal until you type over it (your supplier's 3/4 ply at 0.709 in); editing a stock without touching a size keeps it exact (18 mm stays 18 mm). A model saved with its own edited library keeps it as saved.
- **Board feet on the rough thickness.** A lumber part's board feet are counted on the nominal thickness of the board it comes from (a 3/4 in part from 4/4: 1 in), times its finished width and length; the table still shows finished sizes. The pane (hover T), the CSV (new last column **Board feet on**), the report and the PDF say so. The user guide's bookshelf is now 25.33 board feet (was 18.99, the finished volume).
- **Fractions typed every way.** Every length field reads `1-1/2"`, `1 1/2`, `1.5"`, `1½`, `1 1/2''`, `1.5 inches` and `1'-1-1/2"` alike (parser edge-case tests).
- **Soft, smooth and hidden edges** (ADR-0011). The **Eraser** now does what its hint says: **Ctrl** softens and smooths the edges it is clicked or dragged over, **Shift** hides them, **Ctrl+Shift** unsoftens them (one undo step; nothing erased; hovering an edge says what it is). **Soften edges** in the Styles pane softens and / or smooths every edge of the selected solids whose faces turn by at most an angle (20° rounds a 24-sided cylinder, its rims stay sharp), one undo step; **Soft and hidden edges** in the Edges list draws them dashed. A soft edge is not drawn as a line except where it is a silhouette; across smooth edges the shading is continuous (unlit and with shadows). Flags follow their edges through moves, copies, pushes and every copy of a component; saved in `.shz` (optional `edge_flags`); hashed only when present, so every older document keeps its hash; carried into GLB (smoothed normals), OBJ (smoothing groups) and left out of SVG drawings. Kernel ops `set_edge_flags` and `soften_edges`; the fuzzer drives both and checks that every flag sits on an edge that exists.
- **`.skp` import**: soft, smooth and hidden edges come across per edge (the private round slab's 24 side edges arrive soft and smooth); a definition placed once comes in as a **component** when someone named it (a "Leg"), as a group when its name is SketchUp's own (`Group#3`, `Component#7`); an **empty file** (a blank template) opens as an empty model in the file's units and style instead of being refused.
- **Renders only when they can work.** The Credits chip and the Render pane are hidden unless a render service is set in the new **Edit > Settings…** (saved on this device; a development build also takes `VITE_RENDER_SERVICE_URL`) and answers its health check (`GET /healthz`) at startup or when the address changes; otherwise one Settings line says why ("Renders are off: no render service is set."). A first run shows nothing about credits. Render images are fetched into memory so a service set at run time works under the page's security policy.
- **A truthful public surface.** The README describes what is built today and lists what is not yet (`.skp` export, fillet and chamfer, copy and paste in place, Back and Bottom views, text and radius dimensions in the model, signed installers, render credits). New `docs/LIMITS.md` (measured import times on real SketchUp files, the hard limits, the `.skp` versions openskp 1.3.0 refuses) and `docs/upstream/openskp-2018-parse.md` (a drafted upstream issue). The site's landing page opens with a one-minute silent demo (a bookshelf, the cut list, nesting, the PDF; WebM under 4 MB, recorded by `scripts/record-demo.mjs`), a dated comparison with SketchUp Free and SketchUp with OpenCutList, and a paragraph on other free options (IngeTrazo, Hew, FreeCAD, OpenCutList); `check-site` holds the video, its poster and the comparison to their caps and words.
- **Public CI fixtures**: five real SketchUp 2023 furniture models by Corynth (CC BY 4.0, unmodified, `tests/fixtures/skp-ccby/` with `ATTRIBUTION.md`) run in CI through `e2e/skp-ccby.spec.ts`: each opens with no error and lists its parts in the Cut list (benches 3 after Make parts from loose faces, coffee tables 10, cupboard 4, desk 1, side table 1; the open ones flagged). They are test inputs only, never in the app, the installers or the site.

### Changed (lane 15, version 0.1.0-alpha.2 prepared, not tagged)

- **The site's comparison on a phone** is a card per row with each answer under its product's name, instead of a table that scrolled sideways and hid the third column.
- **Version 0.1.0-alpha.2** in `package.json` (and its lock), `src-tauri/tauri.conf.json` (MSI version 0.1.0.2), the `crates/` workspace and the desktop crates (with their lock files, checked `--locked`). The site names the v0.1.0-alpha.2 installers; `check-site --online --allow-missing-release` (used by the Pages workflow until the tag lands) lets links into an unpublished release through and still checks a published one. Release notes: `docs/releases/v0.1.0-alpha.2.md` (what is new since alpha.1, what is not yet, known limits, the click path past the unsigned-installer warnings on Windows and macOS). The maintainer tags the release.

### Fixed (lane 15 fixes, from the lane 15 verification)

- **A plugin's console no longer opens with a false warning.** In Edge and Chrome a worker's global object carries the constants `TEMPORARY` (0) and `PERSISTENT` (1), which no script can delete; the sandbox reported them as globals it "could not remove", so every plugin's Developer console began with `sandbox: could not remove TEMPORARY, PERSISTENT`. A plain number, string or boolean held as data reaches nothing and is no longer reported; anything callable or an object that survives still is. The sandbox spec now checks the real worker's console is clean.
- **The fuzzer's section check measures both sides the same way.** A section's kept volume integrates each face over its stored plane, while a solid's volume sums its tessellated triangles; a face may sit up to the 1 µm planarity tolerance off its plane, so the two differ by up to that times its area. A component copy squashed to a quarter and stretched to 12 m (fuzz seed 20261802, plane snapping on) had faces 0.5 µm off their planes, and the halves, which made the plane-measured whole exactly, missed the tessellated one by 3.4 ppm. The check now holds the halves to the whole measured their way (1 ppm, as before) and that whole to the tessellated volume within planarity times area. Repro kept.
- **The site's comparison is right about SketchUp Pro billing**: billed monthly or yearly (Trimble's plans page on 12 October 2026 offers both), not "paid yearly"; `site:check` holds the row to it.
- **The Stock dialog lists actual thicknesses as they are**: 23/32, 15/32 and 7/32 in plywood read "23/32 in actual" and so on (they read 3/4, 1/2 and 1/4 in, rounded to the cut list's 1/16 in, so the nominal-versus-actual difference showed only in the Edit form). A stock's size everywhere (the list, the nesting booklet) gives its thickness at the saw's precision: 1/64 in, 0.001 in or 0.01 mm.
- **Real furniture gets an honest cut list, not absurd totals.** On the verifier's real files a bench drawn as one piece of open faces counted 126 board feet (three benches 553, the desk 661, a gondola 8,142), and a flat face ("Rodeo#2", no thickness) was listed as sheet goods and added a 4 x 8 sheet. Two kinds of part are now listed and flagged but **not counted** (no board feet, area or sheet, greyed, the reason in the row, the CSV, the report and the labels) and reported by the nesting with that reason instead of "no board in the stock library is this thick": a **flat face** (thickness at most 0.01 mm), and **open faces thicker than any board** (16/4, 4 in, and every board in the library), almost always a whole piece drawn as one group. A closed solid always counts as drawn. Kernel: `CutRow::uncounted` (`flat`, `not_a_board`), `totals.uncounted`, `UnplacedReason::Flat` and `NotABoard`; hashed only when set. On the CC BY fixtures: 3 of 3 benches, the desk, the side table, 9 of 10 coffee tables and the cupboard's carcass are not counted, and no file adds a sheet.
- **Faces outside parts say where they are.** Make parts from loose faces acts on the open context, but the cupboard's 171 and the gondola's 40 loose faces sat inside assemblies, so the link refused. The cut list now splits the count (`totals.loose_here`, `totals.loose_places`): the link shows only for faces here; otherwise the line says where ("171 faces outside parts (in …)") and offers to **Open** that group, after which the link takes them. When every face in the file is loose (benches.skp, 500 faces), the empty list says so and what to do instead of only "No parts yet".
- **Tooling, from the verifier's friction**: `FUZZ_REPRO_DIR` sends a failing fuzz sequence somewhere other than `crates/kernel/tests/repro/` (forwarded into WSL by `scripts/cargo.sh`; `fuzz-shards.sh` passes it on), so a long local run leaves the checkout alone; `SAWHORSE_PRIVATE_CAPTURES=off` makes the private-fixtures spec take no screenshots; `scripts/serve-site.mjs` serves `.webm` as `video/webm` (and `.mp4`, `.pdf` by their types) as GitHub Pages does.

### Fixed (lane 15, from the lane 10 and 11 verifications)

- **The grid follows the style's background**: a dark Background in a light theme (or the other way round) no longer hides the grid; its lines are drawn a little towards black on a light background and towards white on a dark one.
- **A face lying on a section plane is never split** however slightly it is tilted: a face with every corner within a nanometre of the plane goes wholly to one side, like a parallel one. A Follow Me sweep along an edge a hair off square left a face tilted 5e-11 with three corners exactly on the plane, and the reversed plane kept the notch between them as a sliver with no cap (found by the CI fuzzer once the new edge ops shifted its sequences; repro kept).
- **A stretched copy of a component is checked where it is**: whether a component's open meshes heal closed was decided once, on the definition; a copy scaled up has wider gaps (0.05 mm in the definition, 0.11 mm in a copy stretched 2.25 times), so a copy that is not moved, turned or mirrored but scaled is now healed on its own and flagged "not solid" when it stays open (found by the fuzzer; two repros kept).
- **A SketchUp file with a section plane says what came in**: the planes go in after the import, in the same step, and the report described the last plane added ("Imported x.skp.", nothing counted); it describes the import again.
- **Edges follow the style's background too**: Shop drawing (white paper) on the dark theme drew the theme's light edges on white, so the model all but vanished; edges that would not read on the style's background are drawn in a near-black ink on a light one and a near-white one on a dark one.
- **Painting what already shows the material is not an edit** (no undo step; the status line says so). **Painting a group over parts with paint of their own** says how many faces kept theirs and points at the new **Edit > Clear Paint** (kernel op `clear_paint`: paint off what the selection holds, so what holds it shows; one undo step; component parts are cleared from inside the component).
- **Slider arrow keys are one undo step**: a run of steps on a Styles slider commits once the keys stop (0.7 s) or the slider is left; a click that does not move it is not an edit.
- **The material editor stays in step**: Enter commits once (it used to commit, then commit again on the blur), an unchanged field commits nothing, and every field follows the material when it changes elsewhere (undo, a rename). **Position texture**'s tooltip says why it is off, or what it does.
- **Texture handles** are at least 14 px across on screen however far out the view is, and a click anywhere on a drawn handle takes it.
- **`.shz` thumbnails** are WebP (JPEG where the browser cannot write WebP), at most 100 KB; the writer leaves out anything larger. Older files' PNG thumbnails still open.
- **No shadow-map warning**: three 0.186 removed PCFSoftShadowMap and warned each time shadows were first drawn; the renderer asks for PCF directly (the same picture).
- **Plugins**: the sandbox never reads a global it removed (no TEMPORARY / PERSISTENT deprecation warnings); storage keys such as `constructor`, `toString` and `__proto__` work like any other; every refused call rejects with a typed `PluginApiError` (`code`: "permission", "invalid", "send_failed" ...), including a request that could not be sent and invalid parameters, which used to throw at the caller; `events.on(...)` returns an unsubscribe function whose `ready` promise rejects for an event the plugin may not have or that does not exist. `plugin:check` prints paths relative to where it ran (absolute for a folder elsewhere, never `../../..`), rewrites tsc's file names the same way, and type-checks a plugin project with its own `tsconfig.json` when it has one.
- **Tool keys after a pane's select**: a select picked with the mouse gives the keyboard back to the model, so the next R or P picks the tool (a select worked from the keyboard keeps its arrows).

### Added (lane 12, section planes and drawing sheets, 2026-10-12)

- **Section planes** (ADR-0010): the **Section** tool (rail and Tools menu, no key) lays a plane on any face with a live preview, square to an axis with the arrow keys, reversed with Alt, placed with a click or Enter, active, in the open group or component. One active plane per context; a plane in a group cuts only what the group holds, and a solid is cut by every active plane around it. The view clips exactly and **fills the cut from the kernel's own polygons** (holes stay holes), outlined in the style's section colour. The Outliner lists planes (Activate / Deactivate Cut, Reverse, Create Group from Slice, Export Slice SVG / DXF, Hide, Rename, Delete); Move, Rotate, Scale and copies carry them. **Create Group from Slice** draws the cut's outline as faces in a new group. File > Export > Section slice (SVG filled, DXF closed polylines, 1:1 mm).
- **Styles**: the Sections switches are live (**Section cuts**, **Section planes**, **Section fill**), with the fill colour, the cut line colour and width; also in the View menu. **Scenes** remember which planes are active and hidden.
- **Drawing sheets**: a **Model / Sheets** switch, sheet tabs, a sheet canvas (zoom, pan, fit, grid snap) and the **Sheet** pane. Paper Letter, Legal, Tabloid, A4, A3, A2 or custom, portrait or landscape, margins, inches or millimetres, a grid, how lengths are written (fractions, decimals, feet and inches, millimetres) and a **title block** (title, project, sheet n of N, date, scale, drawn by, revision). Tools: **viewports** (a scene, a standard view or the 3D view's camera, at 1:1 to 1:48 or an architect scale from 3" to 1/4" = 1'-0"; isometric and perspective marked NTS; captions that follow the scale; Fit scale, Recentre, Alt+drag to slide the drawing), **dimensions** (horizontal, vertical, aligned, angle, radius, diameter; snapped to the drawing's corners, ends and middles; the model's true length at any scale; attached to model points, so they follow a new scale or scene), **labels** with leaders that follow the drawing, the **cut list as a table** (live, columns of your choice), images, lines and rectangles. Every edit is one undo step.
- **Exact drawings**: viewports draw the kernel's vector hidden-line drawing of the model (no raster: a box face-on is four lines), cuts honoured and their faces filled; profiles heavier; hidden lines dashed on request.
- **Sheet exports**: **PDF** (vector, the real paper size; all sheets as one PDF), **SVG** (paper units), **DXF** (paper size in the sheet's units; lines, outlines and text on a layer per kind; dimension values as text), **PNG** at 150 and 300 dpi, and **Print** through the system dialog at 100% (File > Print Sheets... for every sheet). Canvas and files draw the same laid-out picture, so what you see is what prints.
- **Files**: `.shz` keeps section planes, scenes' section state, the style's section settings and sheets (`docs/format.md`; optional fields, version 1). `.skp` import brings SketchUp's section planes in where they were drawn, switched off with a note (openskp does not say which was active; `docs/skp-compat.md`).
- **Kernel**: `crates/kernel/src/section.rs` (planes, the exact cut with caps and outline, slices), `paper.rs` (scales, what a dimension measures, lengths on paper), `sheets.rs` (sheets and items as ops); `crates/export/src/drawing.rs` (hidden-line removal), `sheet.rs` (one layout for every output), `table.rs`, `sheet_dxf.rs`, `section.rs` (slice exports); `crates/inference/src/flat.rs` (snapping on a drawing). Ops `add_section`, `place_section`, `flip_section`, `set_section_active`, `rename_section`, `set_section_hidden`, `delete_section`, `move_sections`, `copy_sections`, `slice_to_group`, `add_sheet`, `set_sheet`, `delete_sheet`, `move_sheet`, `add_sheet_item`, `set_sheet_item`, `move_sheet_items`, `delete_sheet_items`, `order_sheet_item`. Everything hashed only when used, so every older golden keeps its hash.
- **Tests**: `crates/kernel/tests/sections.rs` and `sheets.rs`, `crates/export/tests/drawings.rs` (hidden lines, scales: 30 in at 1:8 is 3.75 in, at 1/4" = 1'-0" 0.625 in; dimensions, tables, every export, 40 random sheets), `crates/inference/tests/sections.rs`; goldens `section.json` and `sheet.json` (WASM == native); Vitest for the section maths and tool, the sheet geometry, the pane, the PDF (dimension strings in the file), SVG and PNG; `e2e/sections.spec.ts` and `e2e/sheets.spec.ts`; the fuzzer drives every section and sheet op (20,000 sequences clean; three section repros kept). `scripts/capture-lane12.mjs` is the lane-end walk-through.

### Fixed (lane 12)

- The title block's **Today's date** wrote the UTC date, a day ahead on a Pacific evening; it now writes the local date.
- Two fields of the Sheet pane committed in quick succession (Enter in Title, then Project) no longer undo each other: an edit still on its way to the kernel is the base for the next one.
- A viewport's caption follows its view and scale when Sawhorse wrote it; a caption you typed stays.

### Fixed (lane 12, after its independent verification, 2026-10-12)

- **Dimensions went stale when the model changed**: widening a 30 in cabinet to 36 in left its sheet dimension saying 30", its line 3 3/4 in beside a 4 1/2 in outline. A dimension placed on a solid's corners, edge midpoints or edges now follows the solid through push/pull, Move, Rotate and Scale (ADR-0010, amended): each end keeps an anchor naming the solid and the faces that meet at the corner, found again on every layout. Leaders on a solid follow it too. An end on a sketch, a mesh or a section cut, or whose corner an edit took away, stays where it was placed, and the Sheet pane says which. Saved as optional `anchors` (`docs/format.md`); older sheets keep their hashes.
- **A stale kernel from a shared cargo target dir**: two checkouts sharing one could hand one the other's wasm kernel ("Finished" in two seconds, then `golden:wasm` failing with "unknown variant `add_section`", and a build that would have shipped the older kernel). Every checkout now gets its own target dir (`scripts/env.sh`, the `justfile`), and the kernel carries a stamp of the checkout that built it: `npm run wasm` rebuilds once by itself on a mismatch, `npm run golden:wasm` and `npm run build` refuse another checkout's kernel (`scripts/wasm-checkout.mjs`, `npm run check:wasm`).
- A click on a dimension with the Select tool (no drag) changed its stand-off when the click was off its line; it now only selects it.
- A dimension that measures nothing in its view said `?` in a standard view but `0"` in a scene with the same camera (float noise); lengths under the kernel's merge distance (10 µm) now read `?` everywhere.
- Exporting the slice of a plane that cuts nothing (laid on a face, keeping the side the whole solid is on) now says why and what to do, instead of only "Export refused: nothing to export".
- A section plane a hair off a solid's face (1e-13, after a copy with a squashing scale) cut it inconsistently: the cap skipped the face as parallel while the kept volume split it. A face the cut treats as parallel is now wholly on the plane or on one side (found by the long fuzz run; repro committed).
- The slice's DXF is documented and tested as what it is: one closed LWPOLYLINE per outline and per hole, never loose LINE entities, so a CAM program sees closed parts.

### Security (lane 11, after its independent verification, 2026-10-12)

- **A plugin without `net` could reach the network**: `FontFace(...).load()` inside its worker fetched any URL (the verifier sent the model's labels, volumes and bounds to a local listener), and `webkitRequestFileSystemSync` gave every plugin an origin-wide file system to write to. The sandbox now cuts the worker's global object to an **allowlist** (the language's built-ins and web APIs that reach nothing) instead of removing a list of known names, so these and any API a later browser adds are gone; `navigator` keeps only facts, and `URL.createObjectURL` is removed.
- **The browser build had no content security policy.** It now has one (`index.html`: scripts from the app and `blob:` only, fonts from the app and `data:` only, no objects), and each plugin's worker is started from a `blob:` URL so the page's policy governs it, in the browser and on the desktop: a plugin that got past the other locks still could not load an outside script or font.
- **The source check could be fooled**: it guessed regex versus division, so a regex holding a quote after `if (...)`, `throw`, `delete` or `export default` hid an `import("data:...")` or `import("http://...")` from `plugin:check`, the app and the sandbox. Plugin sources are now parsed with **acorn** as ES modules and checked on the syntax tree; a module the parser cannot read is refused.
- **A grant was keyed by id alone**: a zip with the id and permissions of an enabled plugin, even one replacing a bundled sample, started at once with the old grant. A grant now belongs to the files enabled (their SHA-256): other files under that id are off until enabled again, and the dialog names the sample a plugin replaces. Bundled samples keep their grant across app updates, unpacked folders across Reload.

### Fixed (lane 11, after its verification)

- After Enter in a plugin panel's field, or ticking a box in any pane, tool keys ("r", "p") went to the control. Enter that commits a panel field now hands the keyboard back to the model, and a checkbox, radio, range or colour picker keeps only the keys it uses.
- `npm run plugin:check -- <folder>` type-checks a plugin anywhere, not only under `plugins/`.
- `docs/PLUGINS.md`: the first plugin's `$schema` works outside this repository; what `plugin:check` does outside `plugins/`; where `plugin:pack` writes; **Remove** (session and unpacked plugins) beside **Uninstall**; the three locks and the list of what a plugin's global object has; grants and updates. `docs/DEVELOPMENT.md`: driving the file pickers in Playwright with `?io=basic`.
- `acorn` 8.19.0 (MIT) is a runtime dependency (plugin sources).

### Added (lane 11, plugins, 2026-10-12)

- **Plugins** (ADR-0009, the authoring guide `docs/PLUGINS.md`): a plugin is a folder (or a .zip of it) with `plugin.json` and an ES-module entry, and runs in its own Web Worker, locked down before its code loads (no DOM, no network unless granted and then only to its listed hosts through the app, no IndexedDB or other storage of its own, no `importScripts`, no `eval` or `new Function`), talking to the app only by messages. The app enforces the permissions it asked for (`model.read`, `model.write`, `ui`, `storage`, `file`, `net`) on every call; a plugin that stops answering is reported after 5 s (with Stop) and stopped after 15 s; a crash ends only its worker. The same in the browser and on the desktop.
- **The Plugins pane** (the puzzle-piece tab at the end of the side panel): each plugin with its state, permissions, last error, Enable (after a dialog listing what it asks for), Disable, Reload, Open folder (desktop), Uninstall and a developer console; plugin panels as collapsible sections under each plugin's name; Install from .zip, Load unpacked folder (desktop) and Load from .zip for this session (browser). A **Plugins** menu (each plugin's commands and tools, Manage Plugins, the install entries), plugin shortcuts (Ctrl, Alt or Meta plus a key), plugin tools in their own group at the end of the rail, prompts and confirms titled with the plugin's name.
- **The plugin API, version 1** (`packages/plugin-api`: `index.d.ts`, the `"sawhorse"` runtime shim, `manifest.schema.json`, the manifest validator, the RPC codec): model reads (solids, groups and components, faces, the tree, the selection, materials, the cut list), transactional named batches with placeholders (one undo step, "Undone: Generate cabinet", checked by the kernel; helpers for boxes and components of named parts), declarative panels (text, number with units, select, checkbox, button, table, note, image), tools receiving pointer events, inference and Value Control Box input like the built-in tools, units, geometry helpers, events (selection, document changed and opened, tool, units), app and document storage (the document's saved in the `.shz` as `plugin-data.json`), files through the save and open dialogs, `fetch` to listed hosts.
- **Three samples**, bundled and off until enabled, each MIT with a README: **Cabinet generator** (a frameless cabinet from its sizes as a component of named parts, Left side mirrored from Right side, read straight into the Cut list; Update replaces the selected one in place), **Shelf pin holes** (a tool: hover a face for a column preview, click to drill N holes of diameter D at pitch P, 8, 32 mm, 5 mm to start, one undo step per column), **Model report** (live counts, size, volume, board feet by material, warnings; Markdown and CSV export).
- **Kernel**: `sawhorse_kernel::refbatch` resolves a plugin's batch op by op on a scratch copy, checks the resolved batch does the same as one op, and applies it as one plain `batch` op; `Doc.apply_named_batch`; the worker's `namedBatch` and `faces` requests.
- **Scripts**: `npm run plugin:check` (manifests, sources and a type check of every plugin against the API) and `npm run plugin:pack <dir>` (a deterministic zip); `scripts/capture-lane11.mjs`, the lane-end walk-through.
- **Desktop**: installed plugins under `$APPDATA/plugins`, unpacked folders read from the dialog's pick, Open folder through a Rust command that opens only a plugin folder; `blob:` scripts allowed by the window's policy (the sandbox links a plugin's modules as blob URLs).
- **Tests**: Vitest for the codec, the manifest and its schema, the source rules, the runtime, geometry, zips, panels, the host's permissions, watchdog, crashes, grants, storage and files (fake worker), the samples and the scripts, and batches through the WASM kernel; `crates/kernel/tests/refbatch.rs` against the goldens (a batch drawing and pulling a box hashes as the built-in box; every plain golden as one batch lands on its hash); Playwright: the sandbox in a real browser (no DOM, no fetch, no importScripts, no eval, no change without model.write, outside code refused, a plugin spinning forever reported and stopped while the app keeps drawing), the model API (a named batch undone and redone in one step), the UI end to end, and the three samples (a 30 x 72 x 12 in cabinet with 3 shelves named in the Outliner and listed in the Cut list; a drilled column undone in one step; the report's counts against the model); the fuzzer sends plugin batches (10,000 sequences clean).

### Changed (lane 11)

- `fflate` is a runtime dependency (plugin zips; it was already used by tests and scripts).
- The right panel has a Plugins tab at the end, drawn as a glyph so the named tabs keep their widths; the top bar has a Plugins menu after Tools.
- Typing in a plugin panel's number field and pressing Esc also clears its error line.

### Added (lane 10, materials, styles and shadows, 2026-10-12)

- **Materials** (ADR-0008): a name, a colour, a texture (an image the model holds, with its real-world size, rotation and offset) and a stock link (lumber or sheet goods, and the species). Ops `define_material` (extended; old ops read the same), `rename_material`, `delete_material`, `add_texture` (PNG or JPEG under `assets/<name>`, checked against its bytes).
- **Paint** (B): a face in the open context; a closed group or component as a whole (its default: faces with their own material keep it; each copy of a component can differ); Ctrl+click a whole solid; a click on a selected item paints the selection; hover previews; Alt+click samples. Ops `paint` (faces or items) and `set_face_uv`. A face shows its own material, else its solid's, else the nearest group or component's. Paint follows splits and shapes drawn on a face, goes with faces that disappear, and reaches every copy of a component.
- **Texture positioning**: double-click a textured face (or **Position texture**): handles move, turn and scale the texture, live, one undo step on release. Textures come from each face's own frame, so they stay on the wood through moves, copies and component placements, with the grain along each face's longer side by default.
- **Materials pane**: the active material, the model's materials with face counts, the library (twelve woods and eight sheet goods with textures drawn by our own code, sixteen colours, glass and metal; search and categories), Import image..., and an editor (name, colour, texture size in your units, rotation, stock and species, delete).
- **Styles pane**: six presets (Sawhorse, Shop drawing, Shaded, X-ray, Clay, Outdoors); faces shaded with textures, shaded, monochrome, hidden line, wireframe or X-ray; edges on/off, profiles, extensions, endpoints; background, sky and ground colours; section cuts kept for later. **Shadows**: a sun by date, time and latitude, light and dark, on the ground and on faces, drawn with a soft GPU shadow map. Op `set_style`, `set_shadows`; scenes save and restore both.
- **Cut list reads the species**: a part's material is the one on most of its first solid's faces; a stock link names the species and decides lumber or sheet goods unless the part says otherwise.
- **Renders**: `material_hints` (each material in view, its species, colour and share of the picture, from an exact material-id pass) go with every render request, and a short "Materials: white oak (most of it), ..." sentence after the prompt.
- **Files**: `.shz` keeps materials, face paint, texture images (as `assets/` entries), styles and shadows (all optional fields, version 1). `.skp` import keeps each face's material, brings PNG and JPEG texture images in with their size, and fits each face's texture position (skew and projection dropped, noted). GLB export: one primitive per material, images embedded, `TEXCOORD_0`. OBJ: `usemtl` per face where materials differ, `vt` on textured faces, the images saved beside the MTL. SVG view drawings follow the edge style (profiles heavier, X-ray and wireframe show hidden edges, extensions, endpoints).
- **Under the hood**: `crates/kernel/src/looks.rs` (types, ops, sampling, texture coordinates, hashing only when used, so every older golden keeps its hash); `crates/export/src/library.rs` and `png.rs`; the worker sends each texture image to the UI once, by digest, transferred; the view mesh carries texture coordinates and edge normals. The renderer is unchanged for unpainted models (the corpus images and pixel tests hold); textured faces are overlay meshes per image (sRGB, mipmaps, anisotropy).
- **Tests**: `crates/kernel/tests/looks.rs` (paint and what faces show, splits, imprints, pushes, components and per-copy paint, the cut list's species, textures and positions, refusals, rename / delete, locked items, styles and shadows in scenes, hashes); the new golden `painted.json` (WASM == native); shz, wasm-api, GLB / OBJ / SVG and library tests; the fuzzer drives every lane-10 op and samples every face after each op (`testing::check_looks`); Vitest for the Paint tool and the handles' maths, the panes and the materials sentence; `e2e/materials.spec.ts` (paint, sample, position, styles, shadows at 3 pm, a scene, the cut list) and the golden image of a painted cabinet (`tests/corpus/golden/painted-cabinet.png`). `scripts/capture-lane10.mjs` is the lane-end walk-through.

### Changed (lane 10, with lane 9's nesting)

- Renaming a material carries its nesting stock (Stock dialog) to the new name when the cut list reads the material by its own name; a material linked to a species keeps the default on the species. Materials linked to stock name the species the Nesting section groups by.

### Added (lane 14, the public site, 2026-10-12)

- **https://sawhorse3d.com**: a landing page (`site/`, static HTML and CSS in the mock's tokens and fonts, light and dark, no scripts, no analytics, no cookies): the pitch, Try it in your browser, Download for Windows, macOS (Apple Silicon) and Linux from the v0.1.0-alpha.1 pre-release with a note that the builds are unsigned, three features, how it is built, two screenshots from the running app, links to GitHub, the guide and the changelog, "maintained by @boardfoot", the licence and the Trimble notice.
- **The browser demo at /app/**: `VITE_BASE` sets the build's base path (default `./`); with `VITE_WEB_DEMO=1` the app shows a dismissible "Alpha. Runs in your browser, nothing is uploaded." banner with a link to the latest release, and a Help menu (User Guide, What's New, Download the Desktop App, Source on GitHub). The kernel worker and its WASM load under the base; Open and Save use the browser's pickers as before.
- **/guide/ and /changelog/**: `docs/USER-GUIDE.md` and this file rendered to HTML, with anchored headings and a table of contents.
- **`scripts/build-site.mjs`, `check-site.mjs`, `serve-site.mjs`, `site-smoke.mjs`** (`npm run site`, `site:check`, `site:serve`, `site:smoke`) and **`.github/workflows/pages.yml`**: assemble, check (links, anchors, outside resources, image sizes, identity, the download links answer), smoke-test in a browser (the demo's kernel loads, a box extrudes, Ctrl+S saves and File > Open reads it back, the banner dismisses) and deploy with `actions/deploy-pages` on every push to `main`. `docs/DEVELOPMENT.md` describes the whole loop.
- `scripts/capture-lane09.mjs --theme dark`.

### Changed (lane 14, version 0.1.0-alpha.1, 2026-10-12)

- **Version 0.1.0-alpha.1** everywhere it is declared: `package.json`, `src-tauri/tauri.conf.json`, the `crates/` workspace and the desktop crates (with their lock files). Installers from the next tag carry it in their names; the files of the `v0.1.0-alpha.1` pre-release still say 0.0.1 because they were built before the bump. Windows Installer accepts only numeric versions, so the MSI is versioned `0.1.0.1` (`bundle.windows.wix.version`). Saved `.shz` files now record `"generator": "Sawhorse 0.1.0-alpha.1"`.

### Added (lane 9, nesting and cutting diagrams, 2026-10-12)

- **Nesting section** in the Cut list pane (the mock's "Nesting ›" button, now live): the cut list's pieces laid out on real sheets and boards, worked out again 200 ms after every edit. Saw (Table saw = guillotine, the default; Tightest (CNC) = maxrects), Kerf (1/8 in to start; typed lengths such as 3/32) and Trim per edge; tiles for sheets (by stock), boards (by nominal size, with board feet bought) and the overall yield; parts on no stock with the reason; a to-scale diagram per sheet or board with every part tagged by its cut-list row ("4c"), hover to light it up in the model (and the model's hovered part lights up in its diagram), click to select it; the cut sequence in shop words ("Rip to 11 1/4 in") and the parts under each diagram. ADR-0007.
- **Stock dialog** (Stock... in the section): each material's stock (or Automatic), the library of sheets and boards (add, edit, delete, back to the built-in set). The built-in library: 4 x 8 sheets at 3/4, 1/2, 1/4 in; 5 x 5 Baltic birch at 3/4, 1/2 in; 4 x 8 MDF without grain; 4/4 (13/16 in) boards 4, 6, 8, 12 in wide; 5/4, 6/4, 8/4, 12/4 boards 6 in wide; all 8 ft. Every edit is an op and an undo step, saved in `.shz` as an optional `stock` object (only when it differs from the built-in library).
- **Kernel** (`crates/kernel/src/stock.rs`, `nesting.rs`): stock choice (a part naming a stock, a material's default, then automatic: sheets by thickness within 1/16 in; lumber of one material and thickness on one board size, the narrowest as wide as its widest part); guillotine sheet layouts (four variants, fewest sheets wins), maxrects (parts and bin grown by the kerf), boards by first-fit-decreasing and a bounded lookahead; the kerf on every cut; grain never turned across (only with no grain on the part or the stock); trim per sheet edge and board end; cut sequences found from the layout (trim cuts first; a guillotine layout always comes apart; a maxrects one that does not is flagged); efficiency and waste from exact areas; parts that fit no stock reported (no stock of that kind and thickness, wrong thickness, too large, grain-bound), never dropped; deterministic, hashed. Ops `define_stock`, `delete_stock`, `set_material_stock`, `set_nest_settings`, `reset_stock_library`.
- **Files**, through the save pickers: SVG per sheet or board (1:1 in mm: parts with tags, names and sizes, overall dimensions, numbered cuts, the cut sequence); DXF for CNC and laser (mm; a closed polyline per part on a layer per sheet or board, outlines on STOCK, tags on LABELS; LWPOLYLINE, or strict R12 POLYLINE); a PDF booklet (what to buy, the settings, parts on no stock, an index; then a page per sheet or board with its diagram, parts and cuts).
- **WASM**: `Doc.nest(options)`, `replay_nesting`; the worker's `nest` request; `window.__sawhorse.nest()` for tests.
- **Tests**: `crates/kernel/tests/nesting.rs` (the bookshelf: oak on five 4/4 x 12 x 96 boards and the back on one 1/4 in sheet, 6,105 of 10,368 sq in, 58.9%; as 3/4 ply, three sheets; kerf, trim, maxrects, grain, named and default stock, unplaced reasons, the library's ops, undo, refusals, `.shz`, hashes) with property tests on random cut lists and settings; every golden records a nesting hash and the new `nesting.json` golden (WASM == native); Vitest for the words, the section, the Stock dialog, and every file read back (DOM XML parser, `dxf-parser`, our PDF reader) on a fixture and on real kernel output, the DXF also re-imported by our own DXF importer; `e2e/nesting.spec.ts` nests the UI-built bookshelf and checks sheet and board counts, overlaps and kerf gaps, and that the SVGs, DXF and PDF contain every part; the fuzzer nests after every op with both algorithms and checks every invariant. `scripts/capture-lane09.mjs` is the lane-end walk-through.

### Changed (lane 9)

- The cut list's note no longer says nesting and kerf are missing: its sheet count is a rough estimate and Nesting lays out the real ones.
- `e2e/cutlist.spec.ts`'s bookshelf builder moved to `e2e/bookshelf-build.ts`, shared with the nesting spec.

### Fixed (lane 7b, the lane-7 verifier's findings, 2026-10-12)

- **Rounding no longer changes totals.** Sheet goods were decided by comparing the *rounded* thickness with 1/4 in, so a 0.26 in panel was sheet goods at 1/16 in and lumber at 0.5 mm. The kernel now classifies on the exact thickness (1/4 in + 1 um), and board feet, areas, the sheet grouping (by exact thickness) and the sheet count all come from the exact sizes (snapped to a 24 nm grid only to drop float noise). Every golden keeps its cut-list hash; `every_rounding_classifies_and_totals_the_same` (the bookshelf: 18.994140625 board feet and one sheet at 1/16 in, 1/32 in and 0.5 mm) and `the_quarter_inch_line_uses_the_exact_thickness` pin it, and the cut-list e2e checks it through the WASM kernel.
- **CSV**: decimal T, W, L columns (inches or mm) beside the shown ones; shown fractions written as `="3/4"` so spreadsheets keep them as text (not a date); empty cells instead of a dash; the Total row's sheet count in its own Summary column (not under Stock). Round-trip tests read every cell back.
- **Report consistency**: the PDF and the print view are built from one `reportCells()`: Part, Qty, T, W, L, Material, Grain ("along L" / "across W" / "none"), Bd ft / area (board feet for lumber rows, area for sheet rows), Notes (grain set on the part, shaped, handed, hidden, scaled, in short words). The PDF wraps Part, Material and Notes rather than cutting words off.
- **The pane fits 300 px**: fixed column widths (sizes as wide as their longest value, capped, the grain mark reserved only where used), tabular figures, Part and Material ending in an ellipsis with the full text as a tooltip; a Playwright test walks every rounding, grain and display unit at the pane's width and asserts nothing scrolls sideways. The right-click menu is titled with the row's name and the row stays marked while it is open.
- **Ctrl+Z / Ctrl+Y** (and the file shortcuts) work while focus is on a pane's select, checkbox or button; only an editable text field keeps them (`src/lib/focus.ts`).
- **Render-service noise**: while the service is unreachable the balance poll backs off (10 s, 30 s, 60 s, then every 5 min, was every 10 s), a window focus right after a failure does not ask again, and each attempt is a `console.debug` line, never a console error.

### Added (lane 7b)

- **Round to: Follow units** (the default): 1/16 in for fractional, decimal and ft-in units, 0.5 mm for metric, saying which in the menu. Decimal-inch and ft-in units write the cut list in decimal inches (pane, CSV, report, PDF, labels). A picked rounding lasts for the session; **Pin** keeps it for next time (`src/cutlist/rounding-store.ts`).
- **Bulk stock and grain**: rows pick like a list (click, Shift+click a range, Ctrl+click one more); right-click a picked row to set the grain, the stock, or sheet / lumber for every picked row as one batch (one undo step).

### Added (lane 7, the cut list, 2026-10-12)

- **Cut list pane** (replaces the placeholder), as the approved mock: Part, Qty, T, W, L, Material; a board feet tile (with the species) and a 4 x 8 sheets tile (with the stock and thickness); CSV, PDF, Print, Labels; Nesting shown disabled; a note that kerf, edge banding and nesting are not included. Added around it: Round to (1/16 in, 1/32 in, 0.5 mm; metric units start at 0.5 mm), Visible only, a grain mark beside L (or W), notes under a part's name (sheet, shaped, handed, hidden, scaled copy).
- **What a part is** (ADR-0006): a component definition holding solids (quantity = its instances, multiplied through every component it is nested in), a solid-only group, or a plain solid; assemblies (components holding components or groups) list their pieces. Mirrored copies count in the same row as handed.
- **Sizes**: the minimal oriented bounding box as T <= W <= L, exact for parts made of rectangles, the blank it comes out of for shaped parts (tapered legs, mouldings); finished sizes in whole rounding steps, so every output writes the same fractions.
- **Grain and stock**: right-click a row for grain along the length, across (along the width) or none; Stock / material... (species or sheet stock, lumber / sheet / decide); Mark as sheet goods / solid lumber; Use the material's name. Ops `set_grain` and `set_stock`; attributes on definitions, groups and solids, saved in `.shz` (`docs/format.md`), reaching every copy of a component.
- **Grain arrows** in the viewport while the pane is open, on both broad faces of every drawn piece (walnut along L, copper across; new `--grain` tokens).
- **A row click selects its pieces** in the model (opening the group or component that holds them, as the Outliner does); Shift adds, Ctrl toggles.
- **Exports**, through lane 6's save pickers: CSV (RFC 4180, UTF-8 with BOM, a Total row); the printable report (Print shows only the report through a print stylesheet, in the browser and in Tauri); the report as a PDF from our own small PDF 1.4 writer; labels, one per part, on Avery 5160-size sheets (name, quantity, size, stock, grain in words and an arrow).
- **Kernel**: `crates/kernel/src/cutlist.rs` (`Model::cut_list`, `Model::part_counts`, `PartAttrs`, deterministic and hashed); `wasm-api` `Doc.cut_list` and `replay_cut_list`; the worker's `cutList` request.
- **Tests**: `crates/kernel/tests/cutlist.rs` (the bookshelf as components: Side x2 3/4 x 11 1/4 x 71 1/4, Shelf x4 3/4 x 11 x 34 1/2, Top 3/4 x 11 1/4 x 36, Toe kick 3/4 x 3 1/2 x 34 1/2, Back panel 1/4 x 34 1/2 x 71 1/4 sheet; 2,735.15625 in^3 = 18.994140625 board feet exactly; grain, stock, hidden, handed, rotated, scaled, nested, groups, a tapered leg, rounding, the snapshot); every golden records its cut list hash and the new `cutlist.json` golden replays the bookshelf (WASM == native for models and cut lists); the fuzzer checks the cut list against a brute-force count after every op (20,000 sequences clean); Vitest for the pane, the report, the arrows, CSV / PDF / labels round trips; `e2e/cutlist.spec.ts` builds the bookshelf through the UI and checks every row, the totals and the CSV. `scripts/capture-lane07.mjs` is the lane-end walk-through.

### Fixed (lane 7)

- Outliner rename starts with the old name selected.
- Hiding what is selected (Edit > Hide, the Outliner's eye, a tag or a scene) drops it from the selection, so Delete cannot erase what nobody can see.
- Status lines quote names after their noun, so default names never double it ("Saved scene 'Front'", "Tag 'Shelves' created", "component 'Side' (2 of 2)").
- Move with Ctrl says "Copying ..." for the whole gesture, along a locked axis too.
- Make Component on a single group turns the group itself into the instance (Model > Side > Solid, not Model > Side > Group > Solid), keeping its id, tag and flags; the definition takes the group's axes.
- The fuzzer found a board feet overflow on a solid kilometres across (now in f64; `tests/repro/2026-10-12-cut-list-board-feet-overflow.json`).

### Added (lane 5, groups, components, tags and scenes, 2026-10-11)

- **Groups**: Ctrl+G (Edit > Make Group) groups the selection. A click on anything in a closed group selects the whole group; Move, Rotate and Scale move it as a unit (face ids inside unchanged, the group's axes follow); double-click opens it for editing, with the rest of the model faded, a dashed box round it and the trail in the HUD ("Edit: Model › Carcass"); Esc closes it; a click outside closes it and selects what was clicked. Strokes drawn inside a group stick only to that group's strokes. Ctrl+Shift+G (Edit > Explode) takes it apart.
- **Components**: Edit > Make Component (asks for a name) turns the selection into a definition with one instance. Ctrl+Move copies and Move arrays of an instance are instances; an edit inside any instance (push/pull, draw, erase, offset, Follow Me, moves, grouping, adding or deleting parts) changes every instance. Make Unique gives one instance its own definition ("Shelf (2)"); a cut that severs a solid inside a shared instance makes that instance unique first. Copies of plain solids stay plain.
- **Outliner** (replaces the mock's example rows): the tree of groups, instances ("component · 2 of 4") and solids, folded except around the open context; eye (hide / show) and lock per row; click to select (Shift adds, Ctrl toggles), selection shown both ways; double-click to rename (a component renames its definition); right-click for Edit inside, Make Group, Make Component, Make Unique, Explode, Hide, Lock, Rename, Delete.
- **Tags pane** (replaces the example tags): Untagged first, then each tag with its colour (a colour picker), item count, show / hide, put the selection on it, rename, delete; New tag. Tags hide and show only; hiding a tag on a group hides everything in it.
- **Scenes**: tabs in their own row above the viewport. + saves the camera, the hidden tags and the open group; a click goes to it; right-click updates, renames or deletes; double-click renames. View > Add Scene and the scene list.
- **Kernel** (`crates/kernel/src/nodes.rs`, ADR-0005): the model is a tree of groups and instances over world-space solids. Definitions are templates by slot in their own coordinates; instance solids are derived from them bit for bit (validated). After every op the changed instance is captured into its definition and every instance re-derived (ids and display numbers kept). The editing context is model state: SetContext / LeaveContext are logged but never undo steps, and undo returns to where the edit was made. New ops: MakeGroup, MakeComponent, MakeUnique, Explode, DeleteNode, Rename, RenameDefinition, SetHidden, SetLocked, TagItems, CreateTag, RenameTag, SetTagColor, DeleteTag, AddScene, UpdateScene, DeleteScene, ActivateScene; Transform, Copy and Array take nodes. Typed refusals: `StructureError`.
- **Scene graph across the worker**: `DocState.graph` and `Doc.scene_graph()` (nodes with labels and kinds, where every solid is, definitions with instance counts and their own-axes bounds for the cut list, the context and its path, tag colours).
- **.shz** (`docs/format.md`): `nodes`, `definitions` (each component's geometry once), memberships, sketch parents, `context`, `tag_colors`, scene contexts; instance solids derived on open. Lane-6 files still open; files stay version 1 (optional fields).
- **.skp import** (`docs/skp-compat.md`): definitions placed more than once become components with one instance per placement, placed once become groups; nesting kept; hidden placements come in hidden; layers become tags of their groups and instances.
- **Tests**: `crates/kernel/tests/structure.rs` (a bookshelf of components, groups, scoped stickiness, severance, nesting, tags, scenes, hidden / locked, mirrored and rotated instances, the snapshot), `structure_props.rs` (instance edits propagate, Make Unique forks, Make Component / Make Group then Explode are the identity on hashes), golden `components.json` (WASM == native), shz and wasm-api round trips, `src/lib/scope.test.ts`, `src/components/structure.test.tsx`, `e2e/groups.spec.ts`; the fuzzer drives every lane-5 op (50,000 sequences clean, `docs/DEVELOPMENT.md`). `scripts/capture-lane05.mjs` builds the bookshelf from components in the running app.

### Fixed (lane 5)

- The first `.skp` import no longer reloads the dev page (openskp is pre-bundled by Vite).
- A dev server run from inside a lane worktree (`<repo>/.worktrees/<lane>`) now sees edits: Vite's watch exclusions are anchored at the checkout, where the bare `**/.worktrees/**` had excluded every file of such a server.
- The Outliner keeps names readable when a row's hint is long (the hint shortens first); the Tags pane counts what is inside a component once (in its definition), not once per copy.
- The measurements box clears after undo and redo (a tape reading of geometry that is gone no longer lingers); the active tool's gesture ends.
- No raw ids in the status line: every error is reworded (group, component, definition and mesh refusals too, and any bare public id); the tape's guide message no longer shows a guide id.
- Face edits (draw, imprint, split, erase) that bring a corner within 0.01 mm of another face of the solid (a loop drawn on a sliver a few micrometres thick) are refused, typed, instead of reaching the validator as a kernel bug. Found by the fuzzer; kept as `tests/repro/2026-10-11-imprint-on-a-five-micron-sliver.json` and `-imprint-near-a-folded-face.json`.

### Changed (lane 5)

- What is directly in the open context is what tools edit: solids inside a closed group are selected, moved and deleted as the group, and push/pull, drawing and erasing inside it need it open. Lane-2 groups behave like this too (the `import_mixed` golden now opens its group before its push/pull; its hash is unchanged).
- Hidden SketchUp placements come in hidden (the Outliner's eye) instead of on a hidden "Hidden in SketchUp" tag.
- The Cut list pane says plainly that it is not built yet (and which components will be its parts) instead of showing example rows.
- The corpus render harness hides the scene-tab row so the canvas keeps the size its references were rendered at.

### Integration (lanes 4, 6 and 8 merged, 2026-10-11)

- Lanes 4 (transforms), 6 (file import/export) and 8 (render service and credit store) are merged into `main`; every gate passes on the merged tree (cargo test, clippy, fmt, wasm32 check, Vitest, build, golden:wasm, boolean:diff, the 200-file `.skp` corpus, all Playwright specs with `REQUIRE_KERNEL=1 REQUIRE_CORPUS=1`, and the render service's tests).
- Display numbers ("Solid 3") now survive a save and reopen: `.shz` stores each solid's `number` and the model's `last_number` (both optional; a file without them is numbered in saved order, see `docs/format.md`). Imported and restored solids get numbers like drawn ones.
- The desktop shell registers lane 6's dialog and fs plugins (with the drag-and-drop scope) and lane 8's opener plugin and device-key commands together.

### Fixed (integration)

- An imprint on a face with a notch and collinear vertices was rejected as a kernel bug ("non-positive volume 0"). The triangulator mis-bridged the new hole; it now falls back to its own ear clipper when earcut's area check fails, so the face triangulates. A face that still cannot be triangulated is reported as such (`Untessellatable`), not as a zero volume. An imprint loop whose corner lies within the merge tolerance of one of its own sides is refused as self-intersecting. Found by CI's fuzzer; kept as `crates/kernel/tests/repro/imprint-near-tolerance-non-positive-volume.json`, with `crates/kernel/tests/imprint_tolerance.rs` covering loops near an edge and near-coincident points.

### Changed (integration)

- CI: the long fuzz step on `main` runs a fixed seed (`FUZZ_SEED=20261011`, 10,000 sequences), so a red `main` is always a reproducible regression. Random seeds run nightly in `.github/workflows/nightly-fuzz.yml` (eight shards, 50,000 sequences, continue-on-error, repro artifacts). The `.skp` corpus now runs inside the `web` job (one wasm and one Vite build, one Playwright run with `REQUIRE_CORPUS=1`); `tauri-check` runs fmt, clippy (which compiles the shell, replacing the separate `cargo check`) and the device-key tests.

### Added (lane 8, render service and credit store, 2026-10-11)

- Render credits per ADR-0004, with no accounts:
  - The app holds an Ed25519 device key. On the desktop it lives in the OS secure store (`src-tauri/device-key`, `keyring`); in the browser demo it is a non-extractable WebCrypto key in IndexedDB.
  - `install_id = base32(sha256(public key))[:20]`, and every request to the render service is signed.
- The Credits chip shows this device's balance (a dash when the service is not connected) and opens the Render pane.
- The Render pane is live:
  - Presets, the prompt, and **Render · 1 credit**.
  - The depth pass shows faintly while the render runs, then the result.
  - A history gallery.
  - The $10 / $25 / $50 packs open Stripe Checkout in the system browser and the chip follows the payment.
  - **Move credits**: an emailed claim code moves a balance to a new device, two devices at most.
  - "Render service not connected" when the service cannot be reached; nothing is charged or simulated.
- Render passes from the current three.js view (`src/render/passes.ts`), each with our own PNG writer:
  - a 16-bit linear depth map normalised over the model;
  - an edge line render with hidden lines removed;
  - a clay render.
- The render service, in its own repository (`../render-service`, not deployed):
  - Hono on Node 24, Postgres (PGlite locally, node-postgres in production).
  - Stripe Checkout and an idempotent webhook.
  - Atomic credit decrement; fal first, Replicate as the fallback, re-credit on failure.
  - Claim codes over Resend; limits of 2 concurrent, 6 a minute and 200 a day.
  - 56 tests, also run on real Postgres; a Dockerfile, a systemd unit for the production host and a deploy guide.
- Tauri: the `device_public_key`, `device_sign` and `open_external` commands (the last opens only Stripe Checkout and the local dev checkout), plus the opener plugin. The CSP allows the local service.
- Tests:
  - The shared signing vector, checked in TypeScript, Rust and the service.
  - Client, PNG and Render pane unit tests.
  - `e2e/render.spec.ts`, with a fake service inside Playwright that verifies every signature and decodes the real passes.
  - `scripts/capture-credits.mjs`, the full session against the real service, with screenshots.
- Docs: `docs/RENDER.md`, and `docs/LAUNCH-CREDITS.md` (what the maintainer provides before credits go live).

### Changed (lane 8)

- CI `tauri-check` installs `libdbus-1-dev`, which `keyring`'s Secret Service backend needs, and runs the device-key tests, clippy and fmt for `src-tauri`. The release workflow installs `libdbus-1-dev` too.
- `PW_PORT` sets the Playwright preview port (default 4173), so parallel worktrees can run e2e side by side.

### Added (lane 6, file import/export, 2026-10-11)

- File menu: New, Open, Save, Save As, Import, Import units, and an Export submenu; Ctrl+O, Ctrl+S, Ctrl+Shift+S and Ctrl+I.
  - Drag and drop a file onto the window: a `.shz` opens, anything else is imported.
  - Browsers with the File System Access API use native pickers, and Save writes back to the same file. Other browsers (or `?io=basic`) use a file input and downloads. The desktop app uses Tauri's dialog and fs plugins, limited to the paths the user picked or dropped.
  - An import report lists what became solids, what stayed a mesh and why, and what was healed or skipped.
- Exports, over everything visible or the selected solids:
  - STL (binary, mm, Z up) and 3MF (one named object per solid, mm, colours). Both refuse anything that is not a watertight solid, naming it.
  - glTF binary (Y up, metres, a node per object, face ids as `_FACE_ID`), OBJ (an object and group per part, with a material library), DXF (3D faces and edges, a layer per object).
  - SVG: a hidden-line drawing of the current view (to scale in mm for parallel views), and the outline of a selected face for laser cutting and CNC, also as DXF.
- Imports, each one undo step:
  - STL, OBJ, glTF/GLB, 3MF, DXF (3D faces, lines, polylines with arcs, circles) and SVG (paths and shapes as sketch strokes on the ground).
  - Closed shells become solids, including parts that touch each other. Open, non-manifold or self-intersecting geometry comes in as a visible, amber-tinted mesh with a note; it is never silently repaired.
  - Units for STL, OBJ and unitless DXF: millimetres by default, chosen under File > Import units.
- SketchUp `.skp` import through the npm `openskp` package (MIT), parsed in the kernel worker: groups, each component placement, nested and mirrored placements, materials as colours, layers as tags (hidden ones hidden), hidden objects on a hidden tag, scenes. What is and is not supported: `docs/skp-compat.md`.
- The native `.shz` document, an open format (`docs/format.md`): `scene.json` with exact geometry and persistent face ids, `geometry.glb` for any glTF viewer, and a thumbnail. Opening a saved file gives the same model, hash for hash.
- Materials (colour, texture file name), object names, saved scenes and imported meshes in the kernel; faces drawn in their material colour; the View menu lists saved scenes; Edit > Delete Imported Meshes.
- Tests: a writer and reader test for every format, including round trips through our own readers; every export read by an independent reader (`dxf-parser`, three.js GLTFLoader, fflate); `.shz` round trips of every golden document; a 200-file `.skp` corpus with a golden-image harness and a CI job; `e2e/files.spec.ts` for the menus, export, save, open and drag and drop.

### Changed (lane 6)

- The same per-worktree cargo target directory and `PW_PORT` change as lane 4 (merged once).
- `serde_json` parses floats exactly (`float_roundtrip`), so saved files read back bit for bit.

### Added (lane 4, transforms, 2026-10-11)

- Move (M):
  - Click a solid (or select first), move, click again; or press, drag and let go.
  - The cursor snaps from the start point; arrow keys lock red, green or blue, and Shift holds the line showing.
  - Type a distance mid-move (it goes the way the cursor went) or after it (the move changes).
  - Ctrl copies. After a copy, `x3` (or `3x`, `*3`) makes three copies at one, two and three times the move, and `/4` divides it into quarters. One undo takes the whole array back.
  - With faces, edges or points of one solid selected, Move moves that part. Faces that stay flat follow; one that would bend is refused unless Alt turns autofold on.
- Rotate (Q):
  - The protractor sits on the face under the cursor, or level. Arrow keys lock it upright facing red or green, or level, and Shift keeps its plane.
  - Click the centre, the start of the angle and the end, or type the angle (`90`, `-22.5`, `30deg`).
  - Cursor angles snap to the protractor's 15 degree ticks; typed angles are exact and can be retyped after the turn.
  - Ctrl copies. `x3` and `/4` make arrays the same way Move does.
- Scale (S):
  - Grips sit on the bounding box of the selection: corners, edge middles and face centres. A selected face gets its 8.
  - Each grip scales about the opposite grip; Shift makes the scale uniform and Ctrl scales about the centre.
  - Type a factor (`2`, `0.5`, `-1` mirrors, or `2,1,0.5` for red, green and blue), or an absolute size with a unit (`30in`, `1200mm`).
  - The kernel checks every non-uniform scale for volume sign, flat faces and a closed shell.
  - Scaling a selected face tapers a leg.
- Offset (F):
  - Click a face or a sketch region (or near selected edges), then move inward or outward, or type a distance.
  - Offsets come from `cavalier_contours` with sharp corners. A face with holes is offset as one shape.
  - The result imprints on the solid when it stays on the face, and becomes a new sketch on that plane when it reaches off it (the same way Draw decides). The kernel's own preview shows while the cursor moves.
- Follow Me (W):
  - Select the path (edges end to end, or one face to go round its edge), then click the profile.
  - The profile is swept along straight segments with mitered corners; open and closed paths both work.
  - A sketch region becomes a new solid. A face adds to its solid when the path leaves it, and cuts the solid when the path runs in, so a moulding drawn on a box's end routes round its top.
- Kernel:
  - `Transform`, `Copy`, `Array` and `TransformGeometry` ops: affine, object-level, face ids kept, validated, with typed refusals.
  - A `divided` helper gives the `/n` step of a move, turn, screw or scale.
  - `Offset` with `Doc.offset_preview`, and `FollowMe` with mitered sweeps.
  - Severance: a push or a sweep that cuts through a board leaves one solid per piece, each keeping its face ids.
  - Plane snapping before and after every boolean: nearly coplanar faces share one plane, drifted vertices are re-solved onto it, and coincident vertices are welded. The engine's output is welded at the merge distance.
- Lane-3 verifier conditions:
  - A dev autosave to IndexedDB, with "Restore unsaved work?" (Restore / Discard, also on the Edit menu) after a reload.
  - Status text names solids by display number ("solid 3"), never by raw id.
  - The Rect preview labels each side's length and direction, and its message states them.
  - Per-solid bounding boxes in the bookshelf test.
  - Vite ignores `.worktrees/`, and CI uses `actions/upload-artifact@v5`.
- Tests:
  - Kernel suites for transforms (with the proptest "transform then inverse restores the hash"), offsets, Follow Me (exact volumes), severance and snapping.
  - Goldens `transforms`, `offset` and `follow_me`, where the WASM hash equals the native one.
  - Vitest suites for each new tool, and seven new Playwright specs (`session`, `move`, `rotate`, `scale`, `offset`, `follow`; `vcb` updated).
  - The fuzzer drives every new op. 50,000 sequences are clean, and the boolean refusal rate is measured with plane snapping off and on.
  - A lane-end walk-through, `scripts/capture-lane04.mjs`.

### Changed (lane 4)

- Typing straight after a tool key goes to that tool and is not wiped when the switch lands. Arrow keys and Ctrl / Alt taps wait for clicks still on their way. A render that skipped pending edit updates no longer restores a stale Push target. Fast keyboard use found all three.
- `scripts/env.sh` gives each worktree its own cargo target directory, and `PW_PORT` lets parallel worktrees run Playwright side by side.
- A push that cuts a solid in two now reports `severed` (two solids) instead of leaving a two-shell solid.

### Added (lane 1, foundations, 2026-10-11)

- Browser app shell built from the approved mock: top bar with menus, Iso/Top/Front/Right/Persp views and Credits chip; tool rail with the SketchUp-2017 default keys and our own glyphs; three.js WebGL2 viewport (Z-up, grid, axes, coarse hover inference in our four colours); Outliner / Tags / Cut list / Render panel; Value Control Box with fractional in, decimal in, ft-in, mm, cm and m. Light and dark themes.
- Tauri 2.x desktop scaffold wrapping the same front end (not yet built locally; CI builds installers).
- Rust workspace: `kernel`, `tessellate`, `inference`, `export`, `wasm-api`. Every crate builds for `wasm32-unknown-unknown`.
- Kernel: half-edge B-rep with hole loops and stored planes; named tolerances; topology validator (integrity, loop closure, twins, planarity, winding, self-intersection, hole containment, Euler-Poincare per shell, watertight); sketches with automatic region detection; extrude to a watertight solid; push/pull for faces whose neighbours run parallel to the push; transactional ops with typed errors; deterministic hashing.
- Booleans behind a `BooleanEngine` trait with a manifold-rust adapter that keeps per-face provenance.
- Watertight-only binary STL and 3MF exporters.
- Kernel running as WebAssembly in a Web Worker; Push/Pull and Rect typed values drive it end to end.
- Tests: property invariants, an op-sequence fuzzer with repro files, golden-hash replays checked natively and in WASM, a cargo-fuzz harness, Vitest and Playwright suites.
- CI on GitHub Actions and a tag-triggered, unsigned-by-default release workflow for Windows, macOS and Linux.
- Docs: ADRs 0001-0004, development guide, clean-room log, dual MIT / Apache-2.0 licence.

### Added (lane 2, kernel core, 2026-10-11)

- Sketch tier: strokes insert with the sticky rules (endpoint merge, T-junction splits, crossings split both, collinear overlaps normalised); regions are minimal circuits with holes and keep stable ids across edits; erase heals what a stroke split. Extruding consumes the region.
- Object tier: `split_face` and `imprint` on any face (validated, transactional, user-drawn edges).
- General push/pull on any planar face of a watertight solid: side walls, sliding prism walls, coplanar merge that keeps user-drawn edges, flush welds, collapse, and a boolean path (manifold-rust, re-polygonised into n-gons with holes) for pushing through or into the solid. A global self-intersection check guards every commit.
- Persistent document in the kernel worker: op log, undo/redo (200 steps), groups, tags, delete; Ctrl+Z / Ctrl+Y in the app; Rect draws on the ground or imprints on a face; Push/Pull acts on the clicked face or region. OBJ export.
- Tests: sketch / face / push property tests, 26-case boolean differential (kernel vs npm manifold-3d vs hand-computed, `npm run boolean:diff`), 13 golden logs (WASM == native), an op-sequence fuzzer over every op with undo/redo interleaved (50,000 sequences clean in WSL), lane-end captures in `../_captures/2026-10-11-lane02-*.png`.
- Renamed identifiers that sat close to a copyleft project's names (see `docs/CLEANROOM.md`).

### Added (lane 3, feel, 2026-10-11)

- Persistent face ids. They survive push/pull, split, merge and booleans, and ops, view-mesh parts and the model hash (format v3) use them. Golden and repro logs were migrated.
- Navigation:
  - Middle-drag orbits, Shift+middle or right drag pans, and the wheel zooms to the cursor.
  - The Iso / Top / Front / Right / Persp views and a parallel / perspective toggle.
  - Shift+Z zooms to the extents. Auto-fit only ever zooms out.
  - The grid follows the units (1 in and 1 ft, or 10 mm and 100 mm). The negative axes are dashed.
- Selection and erasing:
  - Select: hover highlight; click, Shift adds, Ctrl toggles; window and crossing boxes; double-click selects the solid; Ctrl+A.
  - Delete and the Erase tool go through the kernel. Edges drawn on a face, imprinted faces, sketch regions, guides and dimensions erase. Structural edges are refused with the reason. A multi-erase is one undo step.
- The inference engine, in Rust (`crates/inference`):
  - Our own BVH for picking.
  - Pixel tolerance at depth, and occlusion.
  - Ranked inference kinds: endpoint, midpoint, intersection, centre, on edge, on face, the three axes, parallel and perpendicular, from point, on guide, guide point, and grid.
  - Locks with the arrow keys and Shift. Length snapping to grid steps.
  - Our colours (lime, teal, amber, violet, plus magenta, blue, indigo, grey, tan and sage).
- The Value Control Box has one grammar for every tool:
  - Fractions, ft-in and metric.
  - `w, d` for sizes, `24s` for segments and `12r` for radii.
  - Angles and the array forms are reserved for lane 4.
  - Focus rules, Esc, and Ctrl+Z mid-entry.
- Draw tools, all on sketch planes and on solids' faces:
  - Line, Rectangle, Rotated Rectangle, Circle, Polygon, Arc and 3-Point Arc.
  - A closed shape inside a face imprints.
  - A stroke crossing a face's edges becomes `split_face` cuts, so a dado strip across a board is one step.
  - A shape reaching off a solid starts a new sketch on that plane.
  - A Rectangle started on a corner, an edge or the grid takes its plane from the cursor.
- Push/Pull:
  - Press-drag-release with a live preview, or click-move-click.
  - Typed distances mid-gesture or after the gesture.
  - Double-click repeats the last distance.
  - Ctrl pulls out a new face and leaves a seam.
  - Snaps to other geometry's points along the face normal.
  - Refusals show typed in the status line.
- Tape and Dimension:
  - Tape measures between points, places guide lines parallel to an edge at a typed or clicked offset, and drops guide points.
  - Guides snap, including where they cross edges. Edit > Delete Guides clears them.
  - The Dimension tool adds linear dimensions.
  - Guides and dimensions are kernel annotations, so undo covers them.
- Tests:
  - Rust inference tests.
  - Kernel tests for face ids, erase, draw and annotations.
  - Vitest suites for each tool against a fake host.
  - Eight new Playwright specs.
  - The Phase-2 exit test, `e2e/bookshelf.spec.ts`: a 36 x 72 x 11 1/4 in bookshelf with dados, 8 watertight solids, 2,761.03125 in^3, 0 refusals, about 10 s. It runs alone with `npm run e2e:bookshelf`.
  - The op-sequence fuzzer covers every new op: 50,000 sequences clean, with boolean-path refusals now counted in the report.
- Docs: `docs/USER-GUIDE.md`, a first-ten-minutes tour with every key and the inference colours.

### Changed (lane 3)

- CI: `actions/checkout@v5` and `actions/setup-node@v5`. The `rust` and `web` jobs are pinned to `ubuntu-24.04`; `tauri-check` stays on 22.04.
- The coarse lane-1 hover inference was removed; the viewport asks the kernel.

### Fixed (lane 3)

- An edge of the face under the cursor was hidden by that same face at oblique view angles, so a corner could not be snapped there. Found during the bookshelf built by hand.
