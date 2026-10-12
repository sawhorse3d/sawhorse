# SketchUp (.skp) import: what comes across

Sawhorse reads `.skp` files with the npm package [`openskp`](https://www.npmjs.com/package/openskp) (MIT, pinned at 1.3.0) inside the app: in the kernel Web Worker of the browser build and of the Tauri webview. No Trimble SDK is involved and nothing leaves the machine. `src/io/skp.ts` maps the parsed file onto **one kernel import op**, so a whole SketchUp model is one undo step, and the kernel builds every piece through the same validated path as an STL or a 3MF (`crates/kernel/src/meshbuild.rs`).

Import only. There is no `.skp` export in this lane (ADR-0002: export waits for the attorney read and for our own blank template).

Status of this page: lane 6, 2026-10-11; groups and components brought across as Sawhorse groups, components and instances in lane 5 (same day). The tests that hold it true are `src/io/skp.test.ts` (mapping and kernel results for every corpus family) and the golden-image corpus (`e2e/corpus.spec.ts`).

## File versions

| SketchUp | Container | Read? | Notes |
|---|---|---|---|
| 2013 to 2020 | classic MFC `CArchive` | yes | Our corpus is written in this format (SketchUp 2017, `{17.0.1}`) and every family round-trips. Saved scenes are not stored in a way openskp reads from these files, so they come in with none. |
| 2021 to 2025 | VFF (zip-based) | yes, via openskp | Scenes (pages) are read. Per-instance layers are not resolved by openskp for VFF, so instances inherit no tag from their own layer override; layer visibility is not stored in VFF either (layers come in visible). Not exercised by a committed fixture (we ship no SketchUp-authored files). |
| 2012 and older | older formats | no | Outside openskp's stated range (2013 onwards). Not tested here (we hold no such file); a file openskp cannot parse is refused with "could not read the SketchUp file" and changes nothing. |

## What is supported

| SketchUp | Sawhorse | How |
|---|---|---|
| Faces (with holes) and edges | Faces of solids or of a non-solid mesh | Every face keeps its own outline: SketchUp's faces are not re-merged, so a line drawn across a face stays a drawn line (a "user edge" between two coplanar faces). Faces are wound around SketchUp's stored front normal. |
| Groups | Groups, named after them, at any depth | A group's geometry becomes a solid where it is a closed manifold shell (several shells: several solids), otherwise a visible non-solid mesh with a note. Its contents keep their world positions. |
| Components | A component definition with one instance per placement | A component placed four times comes in as one definition (`Leg`) and four instances of it, each with its placement (rotation, scale, mirroring, translation; inches to metres). Editing inside one instance changes all four; Make Unique separates one. The definition's geometry is in its own axes, as SketchUp stores it. |
| Group or component? | By how often it is placed | openskp does not say whether a definition is a group's or a component's. A definition placed more than once anywhere in the file comes in as a component; one placed once comes in as a group, unless its name is SketchUp's own group naming (`Group#3`), which is always a group. So a component placed only once arrives as a group (Make Component turns it back into one). |
| Nested groups and components | Nested the same way | A group inside a component is part of the component's definition; a component inside a component is a nested instance in the outer definition. Mirrored placements (negative determinant) keep their faces outward. |
| One component painted differently | One definition per paint | When a component's faces take their colour from the placement ("paint the component") and its placements are painted differently, each paint gets its own definition (`Leg`, `Leg (Walnut)`), noted. Sawhorse keeps materials in the definition. |
| Loose geometry at the top level | Solids where closed, otherwise one mesh named after the file | |
| Loose edges (edges on no face) | Lines on that piece's non-solid mesh | Hidden edges are left out. Soft and smooth edges are kept and drawn like any edge. |
| Materials | Colours (sRGB + opacity), by name | A piece takes the material used by most of its faces (noted when there were several). A face with no material inherits its placement's material ("paint the component"), as SketchUp does. Textures: the image file name is kept as a reference on the material; images are not drawn yet. |
| Layers / tags | Tags, by name | `Layer0` / `Untagged` means untagged. A placement's own layer becomes the tag of its group or instance (hiding the tag hides everything inside). Layers hidden in the file come in hidden. |
| Hidden groups and components | Hidden (the Outliner's eye is closed) | Nothing is dropped; open the eye to see them. (Before lane 5 they went on a hidden tag, `Hidden in SketchUp`.) |
| Scenes (pages) | Saved scenes: camera (eye, target, up, field of view, parallel or perspective) and hidden tags | Scenes without a stored camera are left out (noted). |
| Reversed faces | Turned to match their neighbours | Only on closed shells, where there is exactly one consistent way round; the note says how many faces were turned. |
| Units | Inches to metres | SketchUp stores inches whatever the model's display unit; the display unit is noted when the file states it. |

## What is not supported (yet)

Each of these is counted in the import notes when present, never silently dropped:

- Text and leader text, linear and radial dimensions, section planes, guide lines and guide points (the kernel has guides and dimensions; mapping SketchUp's onto them is a later lane).
- Image entities (`Image` objects placed in the model).
- Textures as pictures (only the file name is kept), texture positioning (UV pins), back-face materials.
- Per-face materials inside one piece: the piece takes its most used material.
- Per-face layers inside a definition (faces and edges on their own layers): only placements carry tags.
- An instance's own name when it equals its definition's (shown as the definition's name).
- Component attributes and dynamic-component properties, glue-to and cut-opening behaviour, the component's insertion point and axes beyond its placement.
- Which definitions were groups in SketchUp (see "Group or component?" above).
- Styles, shadows, fog, geo-location, and the file's thumbnail.
- `.skp` export.

## Healing and refusals

The kernel never repairs geometry silently. For each piece it may weld vertices closer than 0.01 mm, drop faces with no area or exact repeats, turn a closed shell that is inside out the right way round, turn reversed faces on a closed shell, and separate closed solids that touch along an edge or a face; every such change is listed in the import notes. A piece that is still not a valid solid (open edges, an edge shared by three or more faces, a self-intersection, no volume) comes in as a visible non-solid mesh, tinted amber in the viewport, with its reason in the notes and in the document state (`meshes[].note`). STL and 3MF export refuse such a mesh by name; GLB, OBJ and DXF export it, marked as not solid.

A file openskp cannot parse is refused with its message and changes nothing.

## Corpus

`tests/corpus/skp/` holds 200 files written by openskp's writer from our own procedural models (`src/io/corpus/models.ts`: boxes, tables, bookshelves, houses with windows, stairs, curved parts, nested and mirrored components, loose geometry, an open tray and a reversed face). See `tests/corpus/README.md` for how they are generated and checked.
