# SketchUp (.skp) import: what comes across

Sawhorse reads `.skp` files with the npm package [`openskp`](https://www.npmjs.com/package/openskp) (MIT, pinned at 1.3.0) inside the app: in the kernel Web Worker of the browser build and of the Tauri webview. No Trimble SDK is involved and nothing leaves the machine. `src/io/skp.ts` maps the parsed file onto **one kernel import op**, so a whole SketchUp model is one undo step, and the kernel builds every piece through the same validated path as an STL or a 3MF (`crates/kernel/src/meshbuild.rs`).

Import only. There is no `.skp` export in this lane (ADR-0002: export waits for the attorney read and for our own blank template).

Status of this page: lane 6, 2026-10-11; groups and components brought across as Sawhorse groups, components and instances in lane 5 (same day); section planes in lane 12 (2026-10-12); named definitions placed once as components, soft / smooth / hidden edges and empty files opening as empty models in lane 15 (same day); curves (what the reader does not give, and so what comes in as plain edges) in lane 23 (2026-10-19), the per-file curves note dropped in lane 23b (2026-10-20). The tests that hold it true are `src/io/skp.test.ts` (mapping and kernel results for every corpus family) and the golden-image corpus (`e2e/corpus.spec.ts`).

## File versions

| SketchUp | Container | Read? | Notes |
|---|---|---|---|
| 2013 to 2020 | classic MFC `CArchive` | yes | Our corpus is written in this format (SketchUp 2017, `{17.0.1}`) and every family round-trips. Saved scenes are not stored in a way openskp reads from these files, so they come in with none. **Not every save in this range reads:** SketchUp 2017 saves (`{17.0.1}`, `{17.0.18899}`) and a 2014 file read, but a real SketchUp 2018 save (`{18.0.16975}`, openskp's own test file `zero_material_custom_layer_v18.skp`) is refused by openskp 1.3.0 with "legacy .skp parse failed: base probe: anchor resolved to null". The app shows that message and changes nothing. |
| 2021 to 2025 | VFF (zip-based) | yes, via openskp | Scenes (pages) are read. Per-instance layers are not resolved by openskp for VFF, so instances inherit no tag from their own layer override; layer visibility is not stored in VFF either (layers come in visible). Exercised in CI by five SketchUp 2023 saves by Corynth (CC BY 4.0, `tests/fixtures/skp-ccby/`, `e2e/skp-ccby.spec.ts`), the only SketchUp-authored files in the repository. SketchUp 25 and 26.2 saves import in the maintainer's private fixtures run (`e2e/private-fixtures.spec.ts`), whose files are never committed. |
| 2012 and older | older formats | no | Outside openskp's stated range (2013 onwards). openskp's synthetic SketchUp 3, 6 and 7 test files (`{3.0.1}`, `{6.0.1}`, `{7.0.1}`) are refused by openskp 1.3.0 with "legacy .skp parse failed: ..." ("expected a string record", "back-ref to unwalked slot", "definition list misaligned"); a file openskp cannot parse is refused with "could not read the SketchUp file: ..." and changes nothing. |

## What is supported

| SketchUp | Sawhorse | How |
|---|---|---|
| Faces (with holes) and edges | Faces of solids or of a non-solid mesh | Every face keeps its own outline: SketchUp's faces are not re-merged, so a line drawn across a face stays a drawn line (a "user edge" between two coplanar faces). Faces are wound around SketchUp's stored front normal. |
| Groups | Groups, named after them, at any depth | A group's geometry becomes a solid where it is a closed manifold shell (several shells: several solids), otherwise a visible non-solid mesh with a note. Its contents keep their world positions. |
| Components | A component definition with one instance per placement | A component placed four times comes in as one definition (`Leg`) and four instances of it, each with its placement (rotation, scale, mirroring, translation; inches to metres). Editing inside one instance changes all four; Make Unique separates one. The definition's geometry is in its own axes, as SketchUp stores it. |
| Group or component? | By its name and how often it is placed (lane 15) | openskp does not say whether a definition is a group's or a component's. A definition placed more than once anywhere in the file comes in as a component (unless its name is SketchUp's own group naming, `Group#3`). A definition placed once comes in as a **component** when someone gave it a name (a "Leg" drawn once is still the Leg component, as SketchUp made it), and as a **group** when its name is SketchUp's own (`Group#3`, `Component#7`) or empty. A group in SketchUp always has a `Group#N` definition, so groups still arrive as groups. |
| Nested groups and components | Nested the same way | A group inside a component is part of the component's definition; a component inside a component is a nested instance in the outer definition. Mirrored placements (negative determinant) keep their faces outward. |
| One component painted differently | One definition per paint | When a component's faces take their colour from the placement ("paint the component") and its placements are painted differently, each paint gets its own definition (`Leg`, `Leg (Walnut)`), noted. Sawhorse keeps materials in the definition. |
| Loose geometry at the top level | Solids where closed, otherwise one mesh named after the file | |
| Loose edges (edges on no face) | Lines on that piece's non-solid mesh | Hidden loose edges are left out. |
| Soft, smooth and hidden edges | The same flags on the same edges (lane 15) | Each edge's soft, smooth and hidden flags come across, on solids and on non-solid meshes (ADR-0011): a circle pulled into a cylinder arrives round, its side edges not drawn and its shading smooth. The import notes count them ("24 soft, smooth or hidden edges came in as they were"). |
| Materials | Colours (sRGB + opacity), by name; each face keeps its own (lane 10) | A piece's own material is the one on most of its faces; faces with another one keep theirs as face paint. A face with no material inherits its placement's material ("paint the component"), as SketchUp does. |
| Textures | Texture images with their size (lane 10) | PNG and JPEG images come into the model (an `add_texture` step batched before the import, one undo step) with the tile size SketchUp stores, in inches. Other image types keep the file name as a reference, and the colour shows (noted). |
| Texture positions | Per face, as a scale, a rotation and an offset (lane 10) | A face's mapping (SketchUp's pins, or its default projection from the face's own axes) is turned into the affine map it gives in world metres and fitted to Sawhorse's per-face scale, rotation and offset, so the texture lies where SketchUp drew it. Skewed (four-pin) and projected textures lose the skew or the projection (noted). |
| Layers / tags | Tags, by name | `Layer0` / `Untagged` means untagged. A placement's own layer becomes the tag of its group or instance (hiding the tag hides everything inside). Layers hidden in the file come in hidden. |
| Hidden groups and components | Hidden (the Outliner's eye is closed) | Nothing is dropped; open the eye to see them. (Before lane 5 they went on a hidden tag, `Hidden in SketchUp`.) |
| Section planes | Section planes, inactive (lane 12) | openskp gives each plane as `a, b, c, d` (a x + b y + c z + d = 0, inches) with its name and whether it is hidden. It comes in where SketchUp drew it: in the group it was drawn in, in every copy of a component it was drawn in (a plane in Sawhorse belongs to a copy), or at the top. The side kept is taken to be the one opposite `(a, b, c)`; if a file's cut comes in the wrong way round, right-click the plane, Reverse. **Which plane was active is not read** (openskp does not expose it), so every plane comes in switched off, with a note: activate one from the Outliner. Scenes do not bring section state across either. A plane without a usable position (zero or non-finite) is counted in the notes and left out. |
| Scenes (pages) | Saved scenes: camera (eye, target, up, field of view, parallel or perspective) and hidden tags | Scenes without a stored camera are left out (noted). |
| Reversed faces | Turned to match their neighbours | Only on closed shells, where there is exactly one consistent way round; the note says how many faces were turned. |
| Units | Inches to metres | SketchUp stores inches whatever the model's display unit; the display unit is noted when the file states it. |

## What is not supported (yet)

Each of these is counted in the import notes when present, never silently dropped:

- **Curves** (circles, arcs, polygons, welded curves) come in as plain edges, in every file version: see [Curves](#curves) below. The import report says nothing about them (lane 23b).
- Text and leader text, linear and radial dimensions, guide lines and guide points (the kernel has guides and dimensions; mapping SketchUp's onto them is a later lane).
- Which section plane was active, and the section state saved in scenes (planes come in inactive; see above). Section fill and section line styles.
- Image entities (`Image` objects placed in the model).
- Back-face materials (Sawhorse draws back faces in the default back-face colour), skewed and projected texture positions (see above), textures other than PNG and JPEG.
- Per-face layers inside a definition (faces and edges on their own layers): only placements carry tags.
- An instance's own name when it equals its definition's (shown as the definition's name).
- Component attributes and dynamic-component properties, glue-to and cut-opening behaviour, the component's insertion point and axes beyond its placement.
- Which definitions were groups in SketchUp, beyond their names (see "Group or component?" above): a component placed once that nobody named arrives as a group.
- Styles beyond their face mode, shadows, fog, geo-location, and the file's thumbnail (Sawhorse has its own styles and sun). An empty file's first style is read by name: a hidden-line, wireframe, X-ray, monochrome or shaded style sets Sawhorse's face mode; any other keeps Sawhorse's own look (noted).
- `.skp` export.

## Curves

Sawhorse has curves since lane 23 ([ADR-0013](adr/0013-curves.md)): a circle, an arc or a polygon drawn by its tool is one curve, selected, moved and erased whole. A SketchUp file stores the same thing: each edge of a curve points at a shared curve object, a `CArcCurve` for a circle, an arc or a polygon (with its centre, axes, radius, start and end angles and segment count) or a `CCurve` for a welded freeform curve.

**What openskp 1.3.0 gives Sawhorse: no curve membership, in either family.**

| File family | What its reader does with curves | What Sawhorse imports |
|---|---|---|
| Classic MFC `CArchive`, SketchUp 2013 to 2020 | Reads each `CEdge`'s curve pointer, and reads each `CArcCurve`'s fourteen parameters and each `CCurve`'s edge count, then builds the parse output's edges (`Edge { id, v1Id, v2Id, soft, smooth, hidden }`) without them: the pointer and the parameters are dropped. | Plain edges |
| VFF (zip-based), SketchUp 2021 to 2025 | Its edges carry their two vertices and their soft, smooth and hidden flags only. | Plain edges |

(Read from the published package's `dist/index.d.ts` and `dist/index.js`; no SketchUp, OpenSKP repository or Hew source.) So every SketchUp curve comes in as plain edges. Until lane 23b the import report said so in one line on every file with an edge; openskp gives no curve record at all, so it cannot tell a file with curves from one without, and a note on every file (curves or not) was noise. The report now says nothing about curves, and this page and the guide say it once. Sawhorse does **not** guess curves from the edges' shape (a run of equal segments turning by equal angles is very often a curve, but not always, and a guess that turns a woodworker's faceted rail into a "circle" would be worse than none). The inference engine still finds the centres of such runs, as before, so a SketchUp circle's centre snaps. A circle pulled into a cylinder in SketchUp still arrives round: its soft and smooth flags come across (above).

What would change it: openskp exposing each edge's curve id (and a curve's kind and parameters) in its parse output. The request is drafted in [upstream/openskp-curves.md](upstream/openskp-curves.md); the kernel's import op already takes a stroke's curve (DXF circles and arcs use it), so the mapping would be a few lines in `src/io/skp.ts`.

The corpus has one file of each (`201-curve-circle.skp`: a disc whose 24 edges share one `CArcCurve`; `202-curve-arc.skp`: 12 edges of a quarter arc; `203-curve-polygon.skp`: a closed `CCurve` of six edges), written as curves by openskp's writer (`addCircle`, `addArc`, `addPolyline`), so the corpus check imports curves written as SketchUp writes them; `e2e/curves.spec.ts` imports the circle and checks it comes in as plain edges with no note (`src/io/skp.test.ts`: no import of the corpus or of the three curve files says anything about curves).

## Healing and refusals

The kernel never repairs geometry silently. For each piece it may weld vertices closer than 0.01 mm, drop faces with no area or exact repeats, turn a closed shell that is inside out the right way round, turn reversed faces on a closed shell, and separate closed solids that touch along an edge or a face; every such change is listed in the import notes. A piece that is still not a valid solid (open edges, an edge shared by three or more faces, a self-intersection, no volume) comes in as a visible non-solid mesh, tinted amber in the viewport, with its reason in the notes and in the document state (`meshes[].note`). STL and 3MF export refuse such a mesh by name; GLB, OBJ and DXF export it, marked as not solid.

**Known refusals (openskp 1.3.0)**: a real SketchUp 2018 save ("base probe: anchor resolved to null"; draft issue written, not yet filed: [upstream/openskp-2018-parse.md](upstream/openskp-2018-parse.md)) and files from SketchUp 2012 and older. Model-size measurements: [LIMITS.md](LIMITS.md).

A file openskp cannot parse is refused with its message and changes nothing ("could not read the SketchUp file: legacy .skp parse failed: ..." for openskp 1.3.0's refusals of a SketchUp 2018 save and of the pre-2013 files above).

A model with nothing in it (no faces, no loose edges, no placements, no saved scenes: a blank template) is **opened as an empty model** (lane 15), in place of the current one (after asking, when it has unsaved changes), in the file's own units ("Millimeter" opens in mm, "Inches" in fractional inches; a file from before 2021 states none and the units stay as they were) and with its style's face mode as above. The report says so: "Opened blank.skp: an empty model". Nothing is imported and there is nothing to undo.

## Corpus

`tests/corpus/skp/` holds 203 files written by openskp's writer from our own procedural models (`src/io/corpus/models.ts`: 200 cycling through boxes, tables, bookshelves, houses with windows, stairs, curved parts, nested and mirrored components, loose geometry, an open tray and a reversed face; then, lane 23, a circle, an arc and a polygon written as SketchUp curves). See `tests/corpus/README.md` for how they are generated and checked.
