# SketchUp (.skp) import: what comes across

Sawhorse reads `.skp` files with the npm package [`openskp`](https://www.npmjs.com/package/openskp) (MIT, pinned at 1.3.0) inside the app: in the kernel Web Worker of the browser build and of the Tauri webview. No Trimble SDK is involved and nothing leaves the machine. `src/io/skp.ts` maps the parsed file onto **one kernel import op**, so a whole SketchUp model is one undo step, and the kernel builds every piece through the same validated path as an STL or a 3MF (`crates/kernel/src/meshbuild.rs`).

Import only. There is no `.skp` export in this lane (ADR-0002: export waits for the attorney read and for our own blank template).

Status of this page: lane 6, 2026-10-11; groups and components brought across as Sawhorse groups, components and instances in lane 5 (same day); section planes in lane 12 (2026-10-12); named definitions placed once as components, soft / smooth / hidden edges and empty files opening as empty models in lane 15 (same day). The tests that hold it true are `src/io/skp.test.ts` (mapping and kernel results for every corpus family) and the golden-image corpus (`e2e/corpus.spec.ts`).

## File versions

| SketchUp | Container | Read? | Notes |
|---|---|---|---|
| 2013 to 2020 | classic MFC `CArchive` | yes | Our corpus is written in this format (SketchUp 2017, `{17.0.1}`) and every family round-trips. Saved scenes are not stored in a way openskp reads from these files, so they come in with none. **Not every save in this range reads:** SketchUp 2017 saves (`{17.0.1}`, `{17.0.18899}`) and a 2014 file read, but a real SketchUp 2018 save (`{18.0.16975}`, openskp's own test file `zero_material_custom_layer_v18.skp`) is refused by openskp 1.3.0 with "legacy .skp parse failed: base probe: anchor resolved to null". The app shows that message and changes nothing. |
| 2021 to 2025 | VFF (zip-based) | yes, via openskp | Scenes (pages) are read. Per-instance layers are not resolved by openskp for VFF, so instances inherit no tag from their own layer override; layer visibility is not stored in VFF either (layers come in visible). Not exercised by a committed fixture (we ship no SketchUp-authored files). SketchUp 25 and 26.2 saves import in the maintainer's private fixtures run (`e2e/private-fixtures.spec.ts`), whose files are never committed. |
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

## Healing and refusals

The kernel never repairs geometry silently. For each piece it may weld vertices closer than 0.01 mm, drop faces with no area or exact repeats, turn a closed shell that is inside out the right way round, turn reversed faces on a closed shell, and separate closed solids that touch along an edge or a face; every such change is listed in the import notes. A piece that is still not a valid solid (open edges, an edge shared by three or more faces, a self-intersection, no volume) comes in as a visible non-solid mesh, tinted amber in the viewport, with its reason in the notes and in the document state (`meshes[].note`). STL and 3MF export refuse such a mesh by name; GLB, OBJ and DXF export it, marked as not solid.

A file openskp cannot parse is refused with its message and changes nothing ("could not read the SketchUp file: legacy .skp parse failed: ..." for openskp 1.3.0's refusals of a SketchUp 2018 save and of the pre-2013 files above).

A model with nothing in it (no faces, no loose edges, no placements, no saved scenes: a blank template) is **opened as an empty model** (lane 15), in place of the current one (after asking, when it has unsaved changes), in the file's own units ("Millimeter" opens in mm, "Inches" in fractional inches; a file from before 2021 states none and the units stay as they were) and with its style's face mode as above. The report says so: "Opened blank.skp: an empty model". Nothing is imported and there is nothing to undo.

## Corpus

`tests/corpus/skp/` holds 200 files written by openskp's writer from our own procedural models (`src/io/corpus/models.ts`: boxes, tables, bookshelves, houses with windows, stairs, curved parts, nested and mirrored components, loose geometry, an open tray and a reversed face). See `tests/corpus/README.md` for how they are generated and checked.
