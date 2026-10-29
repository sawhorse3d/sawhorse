# SketchUp (.skp) import: what comes across

Sawhorse reads `.skp` files with the npm package [`openskp`](https://www.npmjs.com/package/openskp) (MIT, pinned at 1.4.0 since lane 25) inside the app: in the kernel Web Worker of the browser build and of the Tauri webview. No Trimble SDK is involved and nothing leaves the machine. `src/io/skp.ts` maps the parsed file onto **one kernel import op**, so a whole SketchUp model is one undo step, and the kernel builds every piece through the same validated path as an STL or a 3MF (`crates/kernel/src/meshbuild.rs`).

Import only. There is no `.skp` export in this lane (ADR-0002: export waits for the attorney read and for our own blank template).

Status of this page: lane 6, 2026-10-11; groups and components brought across as Sawhorse groups, components and instances in lane 5 (same day); section planes in lane 12 (2026-10-12); named definitions placed once as components, soft / smooth / hidden edges and empty files opening as empty models in lane 15 (same day); curves (what the reader did not give, and so what came in as plain edges) in lane 23 (2026-10-19), the per-file curves note dropped in lane 23b (2026-10-20); **openskp 1.4.0, SketchUp curves coming in as curves, classic files' scenes and the instance scale term in lane 25 (2026-10-28)**. The tests that hold it true are `src/io/skp.test.ts` and `src/io/skpCurves.test.ts` (mapping and kernel results for every corpus family and for curves), the golden-image corpus (`e2e/corpus.spec.ts`), `e2e/curves.spec.ts` (the corpus's three curve files) and, on the maintainer's machine, the private sweeps (`src/io/skp-sweep.test.ts` in Node, `e2e/private-fixtures.spec.ts` in the app).

## File versions

| SketchUp | Container | Read? | Notes |
|---|---|---|---|
| 2013 to 2020 | classic MFC `CArchive` | yes | Our corpus is written in this format (`{17.0.1}`) and every family round-trips. **Scenes come in since lane 25**: openskp 1.4.0 reads a classic file's scene list (SketchUp 2015 to 2020 desktop saves, and 2017 to 2020 saves made through SketchUp's own library) with each scene's camera and hidden layers, mapped exactly as a 2021+ file's (below); openskp's `gondola_v20.skp` (`{20.1.235}`) brings its one parallel scene. A SketchUp 2018 save that 1.3.0 refused ("base probe: anchor resolved to null", openskp's own `zero_material_custom_layer_v18.skp`, `{18.0.16975}`) opens with 1.4.0: it holds two guide lines and a `Guide` layer and nothing else, so it opens as an empty model ("not imported: 2 guide lines"). Saves from 2014, 2017 and 2020 read under both versions. |
| 2021 to 2025 | VFF (zip-based) | yes, via openskp | Scenes (pages) are read. **Arcs and circles come in as curves since lane 25** (openskp 1.4.0 reads the arc record these files store beside each curve, which 1.3.0 skipped; [Curves](#curves)). openskp 1.4.0 also keeps a scene that saves no camera (SketchUp's "Camera Location" unticked); a Sawhorse scene is a camera, so such a scene is left out with a note, as before. 1.4.0 reads guide lines and guide points from these files, and Sawhorse counts them in the notes ("not imported: 350 guide points"). Per-instance layers are not resolved by openskp for VFF, so instances inherit no tag from their own layer override; layer visibility is not stored in VFF either (layers come in visible). Exercised in CI by five SketchUp 2023 saves by Corynth (CC BY 4.0, `tests/fixtures/skp-ccby/`, `e2e/skp-ccby.spec.ts`; `cupboard.skp`'s 6 circles and 24 arcs are a unit test), the only SketchUp-authored files in the repository. SketchUp 25 and 26.2 saves import in the maintainer's private fixtures run, whose files are never committed. |
| 2012 and older | older formats | some | Outside openskp's stated range (2013 onwards), but 1.4.0's TypeScript reader carries the legacy fixes of openskp's Python reader ([iamahsanmehmood/openskp#284](https://github.com/iamahsanmehmood/openskp/issues/284)), and openskp's synthetic SketchUp 3, 6 and 7 files (`{3.0.1}`, `{6.0.1}`, `{7.0.1}`), which 1.3.0 refused, open. **A real SketchUp 4 or 5 save may still be refused**: a SketchUp 5 save (`{5.0.262}`, an armchair from the OSArch wiki) is refused by 1.3.0 with "legacy .skp parse failed: texture object is not a dib @0x1b41: ..." (the SketchUp 4 and 5 signature: before SketchUp 6 a texture is a bare inline image, not an object) and by 1.4.0, which reads past the texture and almost to the end of the file, with "legacy .skp parse failed: back-ref to unwalked slot 20617 @0x3618e: ..." (there the file holds another bare image, a PNG, whose signature `89 50` is read as an object back-reference: 0x5089 = 20617). openskp's Python reader reads SketchUp 4 and 5 saves ([#440](https://github.com/iamahsanmehmood/openskp/pull/440)); its TypeScript reader, the one Sawhorse uses, carries part of that port in 1.4.0 and does not yet read this file ([#15](https://github.com/sawhorse3d/sawhorse/issues/15), with the evidence on #284). **Workaround:** open the file in SketchUp 2017 or later (SketchUp Make 2017 or the free web version) and save it again; the new save opens. The app shows openskp's message ("could not read the SketchUp file: ...") and changes nothing. |

### openskp 1.3.0 and 1.4.0 on real files

Every `.skp` the maintainer holds (35 files: the five Corynth files in the repository and twelve more of theirs, openskp's fourteen fixtures, three of the maintainer's SketchUp 26.2 saves and the SketchUp 5 armchair; never committed), run through the import path in Node (`src/io/skp-sweep.test.ts`: openskp's parse, `src/io/skp.ts`, the kernel) on 2026-10-28, before the bump and after it. Numbers are solids / non-solid meshes / curves in the model (each copy of a component counts its own) / scenes. Nothing that opened before opens worse: every solid and mesh count is the same, and every note but the curve notes and a guide-point count is the same.

| File | SketchUp | openskp 1.3.0 | openskp 1.4.0 (lane 25) |
|---|---|---|---|
| OSArch wiki armchair | 5.0.262 | refused: "texture object is not a dib @0x1b41" | refused: "back-ref to unwalked slot 20617 @0x3618e" |
| openskp: legacy_v3_synthetic.skp | 3.0.1 | refused: "expected a string record @0x7d6" | 0 / 13 / 0 / 0 |
| openskp: legacy_v3_textured.skp | 3.0.1 | refused: "expected a string record @0x840" | 0 / 1 / 0 / 0 |
| openskp: legacy_v6_synthetic.skp | 6.0.1 | refused: "back-ref to unwalked slot 256 @0x16f2" | 0 / 13 / 0 / 0 |
| openskp: legacy_v7_synthetic.skp | 7.0.1 | refused: "definition list misaligned @0x1bcc" | 0 / 13 / 0 / 0 |
| openskp: legacy_v2014_synthetic.skp | 14.0.1 | 0 / 13 / 0 / 0 | 0 / 13 / 0 / 0 |
| openskp: capilla_quiroz_v17.skp | 17.0.18899 | 0 / 4 / 0 / 0 | 0 / 4 / 0 / 0 (its 6 arcs, cut short on chords, are polylines on meshes: plain edges) |
| openskp: chair_and_table.skp | 17.0.1 | 20 / 8 / 0 / 0 | 20 / 8 / 0 / 0 |
| openskp: legacy_annotations.skp | 17.0.1 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| openskp: single_material_v17.skp | 17.0.1 | empty model | empty model |
| openskp: zero_material_custom_layer_v18.skp | 18.0.16975 | refused: "base probe: anchor resolved to null" | empty model (two guide lines) |
| openskp: gondola_v20.skp | 20.1.235 | 27 / 56 / 0 / 0 | 27 / 56 / 24 / **1** (24 polylines: its arcs sit in components placed with an uneven scale) |
| Corynth: Basin.skp | 23.2.0 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| Corynth: Bathroom.skp | 23.2.0 | 5 / 5 / 0 / 0 | 5 / 5 / 3 / 0 (3 polylines) |
| Corynth: File cabinets.skp | 23.2.0 | 8 / 15 / 0 / 0 | 8 / 15 / 72 / 0 (24 circles, 48 arcs) |
| Corynth: Raithe office.skp | 23.2.0 | 275 / 374 / 0 / 0 | 275 / 374 / 1,602 / 0 (39 circles, 189 arcs, 1,374 polylines) |
| Corynth: benches.skp | 23.2.0 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| Corynth: coffee tables.skp | 23.2.0 | 5 / 10 / 0 / 0 | 5 / 10 / 0 / 0 |
| Corynth: cupboard.skp | 23.2.0 | 14 / 8 / 0 / 0 | 14 / 8 / 60 / 0 (12 circles, 48 arcs) |
| Corynth: desk.skp | 23.2.0 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| Corynth: lamps.skp | 23.2.0 | 12 / 10 / 0 / 0 | 12 / 10 / 32 / 0 (28 circles, 4 polylines) |
| Corynth: office stuff.skp | 23.2.0 | 8 / 10 / 0 / 0 | 8 / 10 / 50 / 0 (1 circle, 7 arcs, 42 polylines) |
| Corynth: pillows.skp | 23.2.0 | 0 / 4 / 0 / 0 | 0 / 4 / 0 / 0 |
| Corynth: pot plants.skp | 23.2.0 | 0 / 3 / 0 / 0 | 0 / 3 / 0 / 0 |
| Corynth: reception table.skp | 23.2.0 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| Corynth: side table.skp | 23.2.0 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| Corynth: side table 2.skp | 23.2.0 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| Corynth: sofa sets.skp | 23.2.0 | 14 / 20 / 0 / 0 | 14 / 20 / 35 / 0 (32 circles, 3 polylines) |
| Corynth: stationary.skp | 23.2.0 | 4 / 2 / 0 / 0 | 4 / 2 / 25 / 0 (1 circle, 14 arcs, 10 polylines) |
| openskp: SU_File.skp | 25.0.575 | 2 / 0 / 0 / 0 | 2 / 0 / 2 / 0 (2 circles) |
| openskp: Untitled.skp | 25.0.575 | 11 / 37 / 0 / 0 | 11 / 37 / 2 / 0 (2 circles; 350 guide points now counted) |
| openskp: style_watermark.skp | 25.0.575 | 2 / 0 / 0 / 0 | 2 / 0 / 2 / 0 (2 circles) |
| maintainer: blank-26-purged.skp | 26.2.0 | empty model | empty model |
| maintainer: box-slot-defective-current.skp | 26.2.0 | 0 / 1 / 0 / 0 | 0 / 1 / 0 / 0 |
| maintainer: round-slab-leg-current.skp | 26.2.0 | 2 / 0 / 0 / 0 | 2 / 0 / 2 / 0 (the pulled circle's two rims) |

Most curves in hand-made furniture lie on open faces (amber meshes), which keep no curves: the import notes count them ("481 curves on faces that are not a solid (or on no face) came in as plain edges: a mesh keeps no curves" for Bathroom.skp). Every arc record in these files matched its edges; none came in as "did not match".

## What is supported

| SketchUp | Sawhorse | How |
|---|---|---|
| Faces (with holes) and edges | Faces of solids or of a non-solid mesh | Every face keeps its own outline: SketchUp's faces are not re-merged, so a line drawn across a face stays a drawn line (a "user edge" between two coplanar faces). Faces are wound around SketchUp's stored front normal. |
| Groups | Groups, named after them, at any depth | A group's geometry becomes a solid where it is a closed manifold shell (several shells: several solids), otherwise a visible non-solid mesh with a note. Its contents keep their world positions. |
| Components | A component definition with one instance per placement | A component placed four times comes in as one definition (`Leg`) and four instances of it, each with its placement (rotation, scale, mirroring, translation; inches to metres). Editing inside one instance changes all four; Make Unique separates one. The definition's geometry is in its own axes, as SketchUp stores it. |
| A placement scaled through its scale term | Placed and sized as SketchUp shows it (lane 25) | SketchUp can keep a uniform scale in the homogeneous term of a placement instead of its 3 x 3 (openskp's 13-value matrix holds 1/w at index 12; its #444: such an instance came out 100 times too big and too far out). Sawhorse folds it into the placement. None of the files above carries one. |
| Group or component? | By its name and how often it is placed (lane 15) | A definition placed more than once anywhere in the file comes in as a component (unless its name is SketchUp's own group naming, `Group#3`). A definition placed once comes in as a **component** when someone gave it a name (a "Leg" drawn once is still the Leg component, as SketchUp made it), and as a **group** when its name is SketchUp's own (`Group#3`, `Component#7`) or empty. openskp 1.4.0 reads SketchUp's own answer (`Definition.isGroup`), but Sawhorse does not use it yet: files written by openskp before 1.4.0 (and so other tools' files) carry no flag and would read every group as a component. Measured on the files above, the flag and the name rule disagree on 59 definitions: Spanish SketchUp's `Agrupar#4` and `Group200#2`-style group names come in as components (56), and three `Group#N` definitions SketchUp calls components come in as groups. See [What is not supported](#what-is-not-supported-yet). |
| Nested groups and components | Nested the same way | A group inside a component is part of the component's definition; a component inside a component is a nested instance in the outer definition. Mirrored placements (negative determinant) keep their faces outward. |
| One component painted differently | One definition per paint | When a component's faces take their colour from the placement ("paint the component") and its placements are painted differently, each paint gets its own definition (`Leg`, `Leg (Walnut)`), noted. Sawhorse keeps materials in the definition. |
| Loose geometry at the top level | Solids where closed, otherwise one mesh named after the file; curves and the flat faces they bound in a sketch (lane 25) | A flat, unpainted set of faces at the top of the model holding a curve whole (a disc, a ring, a rounded rectangle) goes into a sketch with its curves: Sawhorse's own loose 2D geometry, ready to pull. See [Curves](#curves). |
| Loose edges (edges on no face) | Lines on that piece's non-solid mesh | Hidden loose edges are left out. A loose curve at the top of the model, flat and of two segments or more, goes into a sketch as one curve (lane 25). |
| Curves (circles, arcs, welded curves) | Curves (lane 25): sketch curves or solids' rims | See [Curves](#curves). |
| Soft, smooth and hidden edges | The same flags on the same edges (lane 15) | Each edge's soft, smooth and hidden flags come across, on solids and on non-solid meshes (ADR-0011): a circle pulled into a cylinder arrives round, its side edges not drawn and its shading smooth. The import notes count them ("24 soft, smooth or hidden edges came in as they were"). |
| Materials | Colours (sRGB + opacity), by name; each face keeps its own (lane 10) | A piece's own material is the one on most of its faces; faces with another one keep theirs as face paint. A face with no material inherits its placement's material ("paint the component"), as SketchUp does. |
| Textures | Texture images with their size (lane 10) | PNG and JPEG images come into the model (an `add_texture` step batched before the import, one undo step) with the tile size SketchUp stores, in inches. Other image types keep the file name as a reference, and the colour shows (noted). |
| Texture positions | Per face, as a scale, a rotation and an offset (lane 10) | A face's mapping (SketchUp's pins, or its default projection from the face's own axes) is turned into the affine map it gives in world metres and fitted to Sawhorse's per-face scale, rotation and offset, so the texture lies where SketchUp drew it. Skewed (four-pin) and projected textures lose the skew or the projection (noted). |
| Layers / tags | Tags, by name | `Layer0` / `Untagged` means untagged. A placement's own layer becomes the tag of its group or instance (hiding the tag hides everything inside). Layers hidden in the file come in hidden. |
| Hidden groups and components | Hidden (the Outliner's eye is closed) | Nothing is dropped; open the eye to see them. (Before lane 5 they went on a hidden tag, `Hidden in SketchUp`.) |
| Section planes | Section planes, inactive (lane 12) | openskp gives each plane as `a, b, c, d` (a x + b y + c z + d = 0, inches) with its name and whether it is hidden. It comes in where SketchUp drew it: in the group it was drawn in, in every copy of a component it was drawn in (a plane in Sawhorse belongs to a copy), or at the top. The side kept is taken to be the one opposite `(a, b, c)`; if a file's cut comes in the wrong way round, right-click the plane, Reverse. **Which plane was active is not read** (openskp does not expose it), so every plane comes in switched off, with a note: activate one from the Outliner. Scenes do not bring section state across either. A plane without a usable position (zero or non-finite) is counted in the notes and left out. |
| Scenes (pages), 2021+ and (lane 25) classic files | Saved scenes: camera (eye, target, up, field of view, parallel or perspective) and hidden tags | The same mapping for both families. A **parallel** scene keeps the height SketchUp shows (`orthoHeight`): Sawhorse frames a parallel view as a perspective camera at the eye's distance would, so the eye is placed along its line of sight at the distance that shows that height (lane 25; before, a parallel scene came in at the zoom its eye's distance gave). What is dropped: a scene without a stored camera (left out, noted: a Sawhorse scene is a camera), and everything else a scene captures (its style, shadows, section state, axes and which tags it shows beyond the hidden ones). SketchUp has no ground grid, so a scene from a `.skp` carries no grid switch and leaves the grid as it is when you go to it (lane 26). The field of view of a parallel scene is kept as stored and only used to place the eye. |
| Reversed faces | Turned to match their neighbours | Only on closed shells, where there is exactly one consistent way round; the note says how many faces were turned. |
| Units | Inches to metres | SketchUp stores inches whatever the model's display unit; the display unit is noted when the file states it. |

## What is not supported (yet)

Each of these is counted in the import notes when present, never silently dropped:

- **Curves on faces that are not a solid** (most hand-made furniture's: amber meshes), **curves drawn on a solid's face** (a circle imprinted on a top), and curves lying loose or on flat faces **inside a group or a component** come in as plain edges, counted: see [Curves](#curves).
- Text and leader text, linear and radial dimensions, guide lines and guide points (the kernel has guides and dimensions; mapping SketchUp's onto them is a later lane).
- Which section plane was active, and the section state saved in scenes (planes come in inactive; see above). Section fill and section line styles.
- Image entities (`Image` objects placed in the model).
- Back-face materials (Sawhorse draws back faces in the default back-face colour), skewed and projected texture positions (see above), textures other than PNG and JPEG.
- Per-face layers inside a definition (faces and edges on their own layers): only placements carry tags.
- An instance's own name when it equals its definition's (shown as the definition's name).
- Component attributes and dynamic-component properties, glue-to and cut-opening behaviour, the component's insertion point and axes beyond its placement.
- Which definitions were groups in SketchUp, beyond their names (see "Group or component?" above): a component placed once that nobody named arrives as a group, and a group whose definition SketchUp named in another language (`Agrupar#4`) or renumbered (`Group200#2`) arrives as a component. openskp 1.4.0's `isGroup` would settle it for files SketchUp wrote ([#16](https://github.com/sawhorse3d/sawhorse/issues/16)).
- Styles beyond their face mode, shadows, fog, geo-location, and the file's thumbnail (Sawhorse has its own styles and sun). An empty file's first style is read by name: a hidden-line, wireframe, X-ray, monochrome or shaded style sets Sawhorse's face mode; any other keeps Sawhorse's own look (noted).
- `.skp` export.

## Curves

Sawhorse has curves since lane 23 ([ADR-0013](adr/0013-curves.md)): a circle, an arc or a polygon drawn by its tool is one curve, selected, moved and erased whole. A SketchUp file stores the same thing: each edge of a curve points at a shared curve object, a `CArcCurve` for a circle or an arc (with its centre, axes, radius and start and end angles) or a `CCurve` for a welded curve.

**Since lane 25 (openskp 1.4.0) they come in as curves.** openskp 1.4.0 gives each edge the id of the curve it belongs to (`Edge.curveId`) and each definition the exact circle behind every arc and circle (`Definition.arcs`: centre, normal, x and y axes, start and end angles, radius, full circle or not), from classic and 2021+ files alike, including circles and arcs that bound a face. openskp 1.3.0 gave neither, so until lane 25 every SketchUp curve came in as plain edges. Sawhorse still never guesses a curve from the edges' shape: only edges the file itself groups into one curve become one.

What each curve becomes (`src/io/skpCurves.ts`, `src/io/skp.ts`):

| In the file | In Sawhorse |
|---|---|
| A closed chain of edges sharing a curve id, with a full-circle record that matches it | A **circle**: centre from the record, radius and segment count from its edges |
| An open chain with an arc record that matches it | An **arc** on the record's circle, from its start angle (counter-clockwise about its normal) to its end; an open run of a full circle's edges (the rest not in the curve) is an arc too |
| A chain sharing a curve id with no arc record (a welded curve; a SketchUp polygon has no arc record in openskp either) | A **polyline**, closed or open (lane 23's kind). The corpus's `203-curve-polygon.skp` is such a curve: it comes in as one closed polyline of six segments, not a polygon, because nothing in the file says it is round |
| An arc record of an arc scaled unevenly in SketchUp (an ellipse) | A polyline (Sawhorse has no ellipse), noted: "1 elliptical curve (an arc scaled unevenly in SketchUp) came in as a polyline: Sawhorse has no ellipse" |
| An arc cut short in SketchUp, an end on a chord inside the circle (what SketchUp leaves when another edge splits an arc's segment) | A polyline: the kernel's own curve rule allows such a point on a curve, but its import paths read the circle from all of a curve's points, so it cannot stay round. Noted: "51 arcs cut short in SketchUp (an end on a chord) came in as polylines" |
| A record that disagrees with its edges beyond the kernel's curve fit (0.04 mm and rounding, `CURVE_FIT`): a centre or a radius off, a point off the circle or its plane, a point outside the record's angles, a closed chain with a partial record | **Plain edges**, never a bent circle, and a note naming the piece: "Dowel: 1 curve's record did not match its edges; it came in as edges" (several: "Dowel: 3 curves' records did not match their edges; they came in as edges") |
| Edges sharing a curve id that do not make one chain (a branch, two pieces) | Plain edges, noted: "2 curves whose edges do not make one chain came in as plain edges" |
| A one-edge curve with no record (or one cut short to one segment) | A plain edge (a polyline needs two), not noted: nothing is lost |

Where each curve goes:

- **At the top of the model, lying loose** (on no face), flat, with three points or more, none of its edges soft, smooth or hidden (a sketch edge has no flags): into a sketch as a stroke that is a curve, as a DXF circle is (lane 23's import path). Any closed outline in a sketch bounds a face, so a SketchUp circle drawn without a face arrives with one.
- **At the top of the model, bounding a flat face that nothing else touches** (a set of faces joined by their edges, flat within 0.1 micrometre, unpainted, no edge soft, smooth or hidden, holding the curve whole): the faces go into the sketch with it, each loop a stroke, so a SketchUp disc arrives as a sketch circle and its face, **not an amber mesh**, and pulling it makes a cylinder whose two circles are rims (ADR-0013), as pulling a drawn circle does. A painted face stays on the mesh (a sketch face has no paint), and its curves come in as plain edges.
- **Everywhere else** (in groups and components, on solids and meshes): on its piece (`ImportItem.curves`), and the kernel makes it a **rim** of the solid it runs along when every segment is an edge of that one solid lying between two different faces, no pair of faces twice, and a circle's or an arc's points there lie on one circle (the ADR-0013 addendum). A cylinder's two circles come in as its rims, in every copy of a component, and ride up with a cap pulled later. A curve on a non-solid mesh stays plain edges ("481 curves on faces that are not a solid (or on no face) came in as plain edges: a mesh keeps no curves"), and so does a circle drawn on a solid's face, whose edges all lie between the same two faces ("7 curves drawn on a face of a solid came in as plain edges"). A circle or an arc in a group placed with an uneven scale is no longer round where it lands: it comes in as a polyline, noted; in a component placed that way, the rim reads as a polyline in that copy, as any rim scaled out of round does.

The import report counts what came in as curves, by kind when there is more than one: "3 curves came in as curves" for the three corpus files imported together, "30 curves came in as curves (6 circles, 24 arcs)" for Corynth's cupboard (a component's curves are counted once, for its definition).

The corpus has one file of each (`201-curve-circle.skp`: a disc whose 24 edges share one `CArcCurve`; `202-curve-arc.skp`: 12 edges of a quarter arc; `203-curve-polygon.skp`: a closed `CCurve` of six edges), written as curves by openskp's writer (`addCircle`, `addArc`, `addPolyline`); they import as a circle, an arc and a polyline in sketches, each noted "1 curve came in as curve" (`e2e/curves.spec.ts`, `src/io/skp.test.ts`), and the disc pulled is a cylinder with two circle rims. The corpus's loose-geometry family's loose polyline and arc come in as sketch curves too.

The request this answers is kept in [upstream/openskp-curves.md](upstream/openskp-curves.md).

## Healing and refusals

The kernel never repairs geometry silently. For each piece it may weld vertices closer than 0.01 mm, drop faces with no area or exact repeats, turn a closed shell that is inside out the right way round, turn reversed faces on a closed shell, and separate closed solids that touch along an edge or a face; every such change is listed in the import notes. A piece that is still not a valid solid (open edges, an edge shared by three or more faces, a self-intersection, no volume) comes in as a visible non-solid mesh, tinted amber in the viewport, with its reason in the notes and in the document state (`meshes[].note`). STL and 3MF export refuse such a mesh by name; GLB, OBJ and DXF export it, marked as not solid.

**Known refusals (openskp 1.4.0)**: a real SketchUp 5 save ("legacy .skp parse failed: back-ref to unwalked slot 20617 @0x3618e"; re-save it from SketchUp 2017 or later, above). The SketchUp 2018 save 1.3.0 refused opens ([upstream/openskp-2018-parse.md](upstream/openskp-2018-parse.md), kept as history). Model-size measurements: [LIMITS.md](LIMITS.md).

A file openskp cannot parse is refused with its message and changes nothing ("could not read the SketchUp file: legacy .skp parse failed: ...").

A model with nothing in it (no faces, no loose edges, no placements, no saved scenes: a blank template) is **opened as an empty model** (lane 15), in place of the current one (after asking, when it has unsaved changes), in the file's own units ("Millimeter" opens in mm, "Inches" in fractional inches; a file from before 2021 states none and the units stay as they were) and with its style's face mode as above. The report says so: "Opened blank.skp: an empty model". Nothing is imported and there is nothing to undo.

## Corpus

`tests/corpus/skp/` holds 203 files written by openskp 1.4.0's writer from our own procedural models (`src/io/corpus/models.ts`: 200 cycling through boxes, tables, bookshelves, houses with windows, stairs, curved parts, nested and mirrored components, loose geometry, an open tray and a reversed face; then, lane 23, a circle, an arc and a polygon written as SketchUp curves). Lane 25 regenerated all 203 with the 1.4.0 writer, which flags a group's definition as a group; every file imports to the same op as its 1.3.0-written self. See `tests/corpus/README.md` for how they are generated and checked.
