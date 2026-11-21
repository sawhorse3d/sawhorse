# ADR-0005: Groups, components, tags and scenes

- Status: Accepted and implemented, 2026-10-11 (lane 5)
- Deciders: the maintainer (the behaviour below was decided before the lane started); the lane-5 builder (how the kernel holds it)
- Source: the lane-5 brief; ADR-0001 ("stickiness is scoped to the editing context")

## Context

Furniture is built from parts that repeat: two sides, four shelves, four legs. People coming from SketchUp expect groups (a named container with its own editing context) and components (a definition shared by instances, edited once for all), tags that only change visibility, and scenes that save a view. The kernel until lane 4 held a flat list of solids, each its own B-rep in world coordinates, addressed by id from ops, picking, inference and every exporter; "groups" were named sets of solids and copies were independent.

## Decision

**Behaviour (decided before the lane):**

- A group is a named container of solids, sketches and nested groups with its own editing context. Stickiness is scoped to the open context: strokes only join strokes of the same context. Clicking any member of a closed group selects the whole group; double-click opens it, Esc closes it; only an open group allows member edits.
- A component is a definition (shared B-rep and nested structure) with instances (a placement and an id). Editing one instance edits the definition, so all instances update. Make Unique forks the definition; Ctrl+Move and arrays of an instance make instances; copies of plain solids stay plain. Make Group, Make Component and Explode work from a selection. A through-cut that severs a solid inside an instance makes that instance unique first.
- Tags: create, rename, colour, visibility; assigned to solids, groups and instances; visibility only, geometry stays where it is; the default is "Untagged".
- Scenes: named saves of the camera, tag visibility and the open context, as tabs above the viewport; update and delete.

**How the kernel holds it (`crates/kernel/src/nodes.rs`):**

- **Solids stay world-space objects.** Groups and instances are nodes of a tree; every solid, imported mesh and sketch records its parent, and inside an instance its slot (its member of the definition). Picking, inference, push/pull, booleans and the exporters keep working on plain solids with ids that do not change.
- **A definition is a template**: solids and meshes in its own coordinates, nested groups and nested instances, by slot. An instance's solids are *derived*: each is exactly its member's solid under the instance's placement (`Object::mapped`), bit for bit, which the validator checks on every commit. A `.shz` stores each definition's geometry once and lists instance solids without geometry.
- **Propagation is capture and re-derive.** Every op records which containers it changed. Afterwards the innermost instance around each changed container is captured into its definition (only the solids the op touched are re-read from the world, so untouched members keep their exact templates and nothing drifts), then every instance of that definition is re-derived, innermost definitions first. Derived solids keep their ids and display numbers (matched by slot), so the UI's handles stay valid in every copy.
- **The editing context is model state** (it changes what ops mean and must replay), but opening and closing a group is navigation, not an edit: logged, never an undo step, redo kept. Undo restores the model as it was before the last edit, context included, so it returns to where that edit was made.
- **Rules in the kernel, not only the UI**: geometry edits, moves, grouping and deletes refuse what is not directly in the open context (`StructureError::OutsideContext`) or is locked; a stroke on a face or sketch outside the context goes into the context's own sketch on that plane. Metadata (names, tags, hidden, locked) may be set anywhere, as in an Outliner, and propagates like any edit.
- **Tidying after every op**: empty groups and instances go (except the open context, its ancestors and their counterparts in other copies of a component), unused definitions are purged, scenes forget contexts that are gone.
- **Hashing**: everything new is hashed only when present, so every older golden keeps its hash; lane 2's flat groups hash as before.

## Consequences

- Every existing op works inside components with no per-op component code: the generic capture/re-derive does it. The cost is a re-derivation of every instance of an edited definition per op (and the validator's own check), linear in the definition's size times its instances: negligible for furniture, measurable for a file with thousands of instances of one part.
- A copy's solids are re-derived from the definition after an edit made in another copy, so each is the template under its own placement (rounding never accumulates: templates are only re-read for solids an op touched).
- `.skp` import maps SketchUp's definitions onto this directly (a definition placed more than once becomes a component; see `docs/skp-compat.md`), so components survive the trip.
- Lane 7's cut list reads parts from definitions (`Graph.definitions`: name, instance count, bounds in the definition's own axes).
- Not yet: a component's own axes and insertion point beyond its placement (definitions take the world axes of the selection they were made from), glue-to and cut-opening, per-instance materials (a material is part of the definition), "select all instances", and the Outliner's drag-to-reparent.

## Amendment: Move Into (2026-11-21, [#117](https://github.com/sawhorse3d/sawhorse/issues/117))

The Outliner's drag-to-reparent from the "Not yet" line above now exists, as a kernel op.

- **`move_into { items, into }`.** Solids, groups and instances directly in the open context (the rule Make Group has) go into the group `into`, or out to the model when `into` is absent or `null`. One undo step. It answers `moved_into { into, objects, nodes }`. Only each item's parent changes: solids are in world coordinates, a group's `xform` is metadata and an instance's maps its definition to the world, so nothing moves in the world, and what an item holds (solids, sketches, section planes) goes with it. Name, tag, hidden and locked flags, grain and stock, material and an instance's definition are the item's own and stay. A repeated item is dropped without a word, as `editable_things` does.
- **Refused, with nothing changed**: no items; an item or target that is not there; an item not directly in the open context; a locked item, a locked target or a target inside a locked group; a group into itself or into something inside it (`IntoItself`); a target that is a component instance or inside one (`IntoComponent`: a part put there would be in every copy); an item already directly in the target (`AlreadyThere`, the whole op, since no multi-item op skips an unfit item and `Model::apply` is all or nothing).
- **It differs from Make Group and Explode inside an open instance**, where those two work and change the definition for every copy. `move_into` is refused there (`InComponent`): an item moved out of a definition would keep a slot with no instance round it, which `check_structure` rejects. A move within one definition is left out of this first version.
- **What follows from the tree, unchanged rules:** a group of only solids is one part in the cut list (`is_part_container`), so a solid moved into one joins that part and its own grain and stock are not read while it is there, and a group moved into one makes it a group of parts; the section planes of the contexts around a solid cut it, so they follow its new parent; tidying after the op removes a group left empty (except on the open path), and a section plane in it goes too, since tidying does not count planes as content.
- **The model hash covers the open context**, so moving items in and out again gives back the hash once the context is the one it was.
- **Plugins and the MCP server** reach the op through their batches, but cannot open a group (`set_context` is not allowed in a batch), so through them it moves top-level items into a group and cannot take one out.
- **Unsaved work** that holds a `move_into` cannot be restored by 0.1.0-alpha.7 or older, as for any new op.
- **The Outliner** drags rows onto a group's row (into it), any other row (beside it), or a strip below the rows (out to the model). It opens what holds the rows first, as Make Group does, refuses rows from two places before opening anything (that switch is no undo step), puts back the context the person had open, and names on the status line any group the move left empty. Locked rows, imported meshes and section planes do not drag; Esc cancels.
