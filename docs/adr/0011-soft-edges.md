# ADR-0011: Soft, smooth and hidden edges

- Status: Accepted and implemented, 2026-10-12 (lane 15)
- Deciders: the maintainer (the lane-15 brief: "makes the Eraser hint true"); the lane-15 builder (how the kernel keys the flags)
- Source: the lane-15 brief; ADR-0001 (persistent face ids), ADR-0005 (components derived from their definitions), ADR-0008 (styles)

## Context

Every curved part a woodworker draws is a run of flat faces: a 24-sided dowel, a rounded slab, a turned leg, a cove. SketchUp files mark the edges between those faces **soft** (not drawn) and **smooth** (shaded across), so the part reads as one surface; Sawhorse imported them and drew every edge, so a dowel arrived as 24 lines and 24 shaded strips. The Eraser's hint promised "Shift hides, Ctrl softens" and neither did anything.

## Decision

- **Three flags per edge**, as bits: soft (1, not drawn as a line), smooth (2, faces meeting there share their shading normals at its ends), hidden (4, not drawn at all, not picked, not inferred on). They are view data: geometry, validation, volumes, the cut list and every watertight export are unchanged.
- **Where they live.** On a solid, by the **pair of persistent face ids** the edge lies between (`Object::edge_flags`). Face ids already survive moves, copies, component placements, push/pull of other faces and splits (the first piece keeps its id), so a flag follows its edge through all of them with no bookkeeping of its own; after every edit the flags of face pairs that no longer meet are dropped (`settle_edges`), and the fuzzer checks that no flag survives on faces that do not meet. On an imported non-solid mesh, by **vertex-index pair** (its topology never changes).
- **Ops.** `set_edge_flags` (edges named by owner and end points, as the UI already names an edge for `erase_edge`; set bits and clear bits; one undo step for all) and `soften_edges` (every edge of the given solids and meshes whose faces turn by at most an angle gets soft and / or smooth, every other edge of them loses both; hidden ones stay hidden). Both refuse edges outside the open context, unknown edges, flags that are not 1, 2 or 4, and angles outside 0 to 180 degrees, and change nothing then.
- **Saved and hashed only when present.** `.shz`: `edge_flags` on a B-rep (`[face id, face id, flags]`) and on a mesh (`[vertex, vertex, flags]`), optional (docs/format.md); the model hash includes them only when an object or mesh has any, so every older document and golden keeps its hash. A component definition carries its solids' flags; an instance's solids are derived with them; editing the flags inside one copy reaches them all.
- **Drawing.** The view mesh carries one flag byte per edge segment and, only when some edge in view is smooth, a shading normal per render vertex (the face's own normal, averaged with the faces it meets across smooth edges at that vertex). The renderer leaves soft and hidden edges out of the edge lines, draws a soft edge where it turns into a silhouette (so a softened cylinder keeps its outline), blends the theme's three face tones by those normals (unlit) or lights them (with shadows on), and with the style's **Soft and hidden edges** switch draws both dashed. GLB export writes the smoothed normals; OBJ writes smoothing groups (`s 1`, `s off`); SVG drawings leave soft and hidden edges out. STL, 3MF and DXF carry no normals or edge data, so nothing changes there.
- **Picking.** Soft and hidden edges are not picked unless the query asks (the Eraser with Ctrl+Shift); hidden edges are not inferred on; soft edges still are (their ends and midpoints are real points).
- **The Eraser.** Ctrl softens and smooths, Shift hides, Ctrl+Shift unsoftens (clears all three), on a click or across a drag; one undo step. Hovering an edge says its flags in the status line.
- **Import.** `.skp` soft, smooth and hidden flags come across per edge (an import item's `edge_flags`, matched by end points to the solids and meshes the item becomes).

## Consequences

- A drawn line across a face that is later softened, then split, keeps its flag only on the piece that kept the face id; the new piece's edge is hard. Woodworkers soften after shaping, so this is rarely visible; Soften selected re-applies the rule in one click.
- Two faces that meet along more than one edge share one flag (rare: a face wrapped round a notch). Documented, not modelled.
- Not yet: unhiding hidden edges from a menu (Ctrl+Shift on them with the switch on does it), soft edges in the `.skp` export (there is none yet).
