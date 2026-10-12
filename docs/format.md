# The `.shz` file format, version 1

Status: open specification, 2026-10-11 (lane 6; groups, components, tags and scenes added in lane 5, cut-list part attributes in lane 7, the stock library and nesting settings in lane 9, as optional fields). License: this document is CC0; the format is free to implement by anyone, for anything.

A `.shz` file is a Sawhorse model. It is a ZIP archive (APPNOTE 6.3, Deflate or Stored entries) with these entries:

| Entry | Required | What |
|---|---|---|
| `mimetype` | yes (Sawhorse writes it first) | The ASCII text `application/vnd.sawhorse.shz`, **stored** (uncompressed) as the first entry, so a reader can identify the file from its first bytes without unzipping (the text starts at byte 38: a 30-byte local header, then the 8-byte name). Readers must not depend on it. |
| `scene.json` | yes | Everything about the model: the header below and the model data. UTF-8 JSON. |
| `geometry.glb` | yes (Sawhorse writes it) | Every solid and imported mesh as glTF 2.0 binary, so any glTF viewer can show a `.shz`. Also the fallback geometry for objects that `scene.json` lists without a `brep`. |
| `thumbnail.png` | no | A picture of the model (Sawhorse: the view at save time, at most 512 px on the long side). Named by `scene.json`'s `thumbnail`. |
| `assets/` | no | Texture images, referenced from materials by path inside the archive. Version 1 writers keep textures as file-name references only, so this folder is empty. |

Readers must ignore entries and JSON fields they do not know. Writers must not put required meaning into new fields without raising `version`.

## `scene.json`

```json
{
  "format": "sawhorse-shz",
  "version": 1,
  "generator": "Sawhorse 0.0.1",
  "units": { "length": "metre", "up": "Z", "display": "frac-in" },
  "geometry": "geometry.glb",
  "thumbnail": "thumbnail.png",
  "sketches": [],
  "objects": [],
  "meshes": [],
  "groups": [],
  "nodes": [],
  "definitions": [],
  "context": 3,
  "tags": {},
  "tag_colors": {},
  "materials": [],
  "scenes": [],
  "guides": [],
  "dimensions": []
}
```

(`nodes`, `definitions`, `context`, `tag_colors` and `stock` are optional and left out when empty, or for `stock` when the model uses the built-in library.)

| Field | Type | Meaning |
|---|---|---|
| `format` | string | Always `sawhorse-shz`. A reader refuses anything else. |
| `version` | integer | Schema version of the model data (this document: 1). A reader refuses a version it does not know (newer); version 1 readers accept version 1. |
| `generator` | string | Who wrote the file. Informational. |
| `units.length` | string | Unit of every coordinate in `scene.json`. Always `metre`. |
| `units.up` | string | Up axis of `scene.json`. Always `Z`. (`geometry.glb` is Y up, as glTF requires: a point (x, y, z) here is (x, z, -y) there.) |
| `units.display` | string, optional | The unit system the user worked in: `frac-in`, `dec-in`, `ft-in`, `mm`, `cm` or `m`. Display only; restored on open. |
| `geometry` | string | Entry name of the glTF geometry. Default `geometry.glb`. |
| `thumbnail` | string, optional | Entry name of the thumbnail. |

Numbers are JSON numbers written so that they read back to the same IEEE 754 double (Sawhorse writes shortest round-trip decimals). All coordinates are metres, Z up, right-handed.

### `sketches[]`

A sketch is a planar graph of strokes whose closed circuits are regions.

| Field | Type | Meaning |
|---|---|---|
| `plane` | plane | The sketch plane: `origin`, `normal`, `u`, `v`, each `[x, y, z]`. `normal`, `u`, `v` are unit length, `u` x `v` = `normal`. |
| `vertices` | `[[u, v], ...]` | Vertex positions in the plane's frame (metres along `u` and `v`). |
| `edges` | `[[a, b], ...]` | Edges as indices into `vertices`. No two edges cross, overlap or repeat; no vertex lies on an edge it does not end; every vertex has an edge. |
| `regions` | region[] | The faces of the graph (every one; a reader checks they match what the edges form). |

A region: `outer` (vertex indices, counter-clockwise seen from `normal`), `holes` (lists of vertex indices, clockwise), `anchor` (`[u, v]`, a point strictly inside), `pinched` (a hole touches the outer ring or another hole at a vertex; such a region cannot be extruded), `consumed` (it was extruded into a solid and is not drawn).

`parent` (integer, optional): the node (`nodes[].id`) the sketch was drawn in. Strokes only join strokes of the same parent, so a sketch drawn inside a group never merges with one outside it.

### `objects[]`

An object is a watertight solid.

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | The id it had when saved; `groups` refer to objects by it. Unique in the file. |
| `name`, `tag`, `material` | string, optional | Display name; tag (layer) name, which must be a key of `tags`; material name, which must be in `materials`. |
| `next_face_id` | integer | The next persistent face id the object will hand out. Greater than every face id. |
| `number` | integer, optional | Display number ("Solid 3"), at least 1. Kept on open when every solid has one and none repeats; otherwise every solid is renumbered in saved order. |
| `source_sketch` | integer, optional | Index into `sketches` of the sketch it was extruded from. |
| `brep` | B-rep, optional | Exact geometry (below). A writer that cannot produce it may leave it out; the reader then rebuilds the object from the `geometry.glb` node whose `extras.sawhorse.object` equals `id`. Never present for a solid with a `slot` (see components below). |
| `parent` | integer, optional | The node (`nodes[].id`) the solid is in: a group or a component instance. Absent: the model's top level. |
| `slot` | integer, optional | For a solid of a component instance: the definition member it is derived from (see below). |
| `hidden`, `locked` | boolean, optional | Hidden (not drawn, not exported by default) and locked (not editable). Absent: false. |
| `part` | part attributes, optional | What was said about the solid as a cut-list part (below). For a solid with a `slot` it repeats its definition member's. |

A B-rep: `points` (`[[x, y, z], ...]`), `faces` and `user_edges`. A face: `id` (persistent face id, unique in the object, at least 1), `outer` (indices into `points`, counter-clockwise seen from outside the solid), `holes` (lists of indices, clockwise seen from outside), `plane` (as for sketches; the face's frame, with `normal` pointing out of the solid). `user_edges` lists `[a, b]` index pairs of edges a person drew: the only edges allowed between two coplanar faces.

The B-rep must be a valid solid: every edge shared by exactly two faces, once in each direction; every face planar (vertices within 1 micrometre of its plane) and simple; no face touches another except along shared edges and vertices; the solid encloses positive volume. A reader refuses a file whose solids do not validate; it does not repair them.

### `meshes[]`

Imported geometry that is not a valid solid (kept so nothing is lost on import). `id`, `name`, `tag`, `material` as for objects; `note` (why it is not a solid, in plain words); `positions` (`[[x, y, z], ...]`); `faces` (each a list of rings, outer first, of indices into `positions`); `lines` (`[a, b]` pairs: edges with no face). `parent`, `slot`, `hidden`, `locked` as for objects; a mesh with a `slot` (inside a component instance) has empty `positions`, `faces` and `lines`: it is derived from its definition.

### `nodes[]`: groups and component instances

The model is a tree. Its inner nodes are groups and component instances; solids, meshes and sketches are its leaves and say which node they are in with `parent`. Nodes are listed in id order; a reader reads them all before resolving `parent`.

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | The id it had when saved. Unique among nodes. |
| `kind` | `"group"` or `"instance"` | |
| `definition` | integer | Instances only: index into `definitions`. |
| `parent` | integer, optional | The node it is in. Absent: the top level. Parents form no cycle. |
| `name` | string, optional | A group's name; an instance's own name (absent: it shows its definition's name). |
| `tag` | string, optional | Tag (layer) name; must be a key of `tags`. Hiding the tag hides everything in the node. |
| `hidden`, `locked` | boolean, optional | As for objects; hiding a node hides everything in it. |
| `slot` | integer, optional | A node nested inside a component instance: its definition member. |
| `xform` | transform, optional | Local axes to world: for an instance, its placement (definition coordinates to world); for a group, where its axes are (its contents are stored in world coordinates already). Absent: the identity. |
| `part` | part attributes, optional | A group's cut-list attributes (below). Instances carry none: theirs are their definition's. |

A transform is `{ "linear": [[a, b, c], [d, e, f], [g, h, i]], "translation": [x, y, z] }`, mapping a point `p` to `linear * p + translation` (rows of the matrix; metres). It must be finite and not flatten anything (`|det(linear)| >= 1e-9`).

### `definitions[]`: component definitions

A component definition is geometry and structure its instances share, stored once, in its own coordinates.

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Shown for every instance without a name of its own. |
| `next_slot` | integer | Greater than every member's `slot`; slots are never reused. |
| `members` | member[] | What the definition holds. |
| `part` | part attributes, optional | The component's cut-list attributes (below): what every instance is cut from and which way its grain runs. |

A member has `slot` (unique in the definition), `parent` (optional: the `slot` of the nested group member it is in), `name`, `tag`, `hidden`, `locked` (optional, as for nodes), `part` (optional: the cut-list attributes of a solid or nested group member) and a `kind`:

- `"solid"`: `material` (optional), `next_face_id`, `brep` (exactly as for objects, in definition coordinates).
- `"mesh"`: `material`, `note`, `positions`, `faces`, `lines` (as for meshes, in definition coordinates).
- `"group"`: `xform` (optional) of a nested group, in definition coordinates.
- `"instance"`: `definition` (index into `definitions`) and `xform` (optional), a nested instance placed in definition coordinates. A definition must not contain itself, directly or through others.

**Instances are derived.** Every instance contains exactly one item per member of its definition (a solid, mesh or node with that `slot`, in the right nested group), and every solid of an instance is its member's solid under the instance's `xform`: each point mapped, each face plane mapped (normals by the inverse transpose), loops reversed when the transform mirrors. Writers list those solids in `objects` (for their `id`, display `number`, `name`, `parent`, `slot`, flags) without a `brep`; readers derive their geometry. A reader refuses a file whose instances do not hold exactly their definitions' members.

Nested node placements compose: a member node's `xform` in the world is its instance's `xform` after the member's own.

### Part attributes (cut list)

An object, all optional (absent, or `{}`: nothing said). Written on definitions, groups and solids (`objects[]`, `nodes[]` of kind group, `definitions[]`, `definitions[].members[]` of kind solid or group). Readers that do not know them lose nothing but the cut list's overrides.

| Field | Type | Meaning |
|---|---|---|
| `grain` | `"length"`, `"width"` or `"none"` | Which way the grain runs: along the part's length (the default; writers leave it out), across it (along its width), or no grain (MDF, painted). Length, width and thickness are the part's minimal box, sorted. |
| `stock` | string | What the part is cut from, in words ("White oak", "3/4 Baltic birch ply"), at most 64 characters. The cut list shows it instead of the material's name. |
| `sheet` | boolean | Sheet goods (true) or solid lumber (false). Absent: sheet when the stock or material name reads as sheet stock (ply, MDF, HDF, OSB, particle, melamine, hardboard) or the part is 1/4 in thick or less. |

Example: `"part": { "grain": "width", "stock": "1/4 birch ply", "sheet": true }`.

### Other tables

| Field | Type | Meaning |
|---|---|---|
| `groups[]` | `{ "name", "objects": [id, ...] }` | Lane-6 groups: each group's name and the solids directly in it (by `objects[].id`). Still written, for readers that do not know `nodes`; a reader that finds `nodes` ignores `groups`. A file without `nodes` gets one top-level group per entry (a solid listed twice stays in the first). |
| `context` | integer, optional | The node open for editing when the file was saved. |
| `tags` | `{ name: visible }` | Every tag used, and whether it is shown. "Untagged" is not a tag: it is the absence of one. |
| `tag_colors` | `{ name: "#rrggbb" }`, optional | Each tag's colour in the Tags pane. |
| `materials[]` | `{ "name", "color": [r, g, b, a], "texture"? }` | sRGB colour with alpha, 0 to 255; `texture` names an image (a path inside `assets/`, or the original file name when the image was not kept). |
| `scenes[]` | `{ "name", "eye", "target", "up", "fov", "parallel", "hidden_tags"?, "context"? }` | Saved views: camera in metres, vertical field of view in degrees (for a parallel view, the angle a perspective camera at the eye's distance would need to show the same height), parallel or perspective projection, the tags the scene hides (every other tag is shown when it is activated), and the node open for editing in it. Names are unique. |
| `guides[]` | `{ "point", "dir"? }` | Construction lines (a point and a unit direction) and guide points (no `dir`). |
| `dimensions[]` | `{ "a", "b", "offset" }` | Linear dimensions between `a` and `b`, the line drawn at `offset` from them. |
| `last_number` | integer, optional | The last display number handed out, so a deleted solid's number is not reused after a reopen. |
| `stock` | stock library, optional | The sheets and boards the model's parts are nested on, each material's default stock, and the nesting settings (below). Absent: the built-in library. |

### Stock library (nesting)

What the nesting (cutting diagrams) lays parts out on. Absent: Sawhorse's built-in library (4 x 8 sheets at 3/4, 1/2 and 1/4 in; 5 x 5 Baltic birch at 3/4 and 1/2 in; 4 x 8 MDF; 4/4 boards 4, 6, 8 and 12 in wide; 5/4, 6/4, 8/4 and 12/4 boards 6 in wide; all 96 in long; kerf 1/8 in, no trim, guillotine). Writers leave it out when it equals the built-in library. Readers that do not know it lose nothing but the nesting's settings.

```json
"stock": {
  "stocks": [
    { "name": "4 x 8 sheet, 3/4", "kind": "sheet", "length": 2.4384, "width": 1.2192, "thickness": 0.01905, "grain": "length" },
    { "name": "4/4 x 12 x 96", "kind": "board", "length": 2.4384, "width": 0.3048, "thickness": 0.0206375, "nominal": "4/4", "grain": "length" }
  ],
  "defaults": { "Oak": "4/4 x 12 x 96" },
  "settings": { "kerf": 0.003175, "trim": 0, "algorithm": "guillotine" }
}
```

| Field | Type | Meaning |
|---|---|---|
| `stocks[]` | stock[] | At most 200. |
| `stocks[].name` | string | 1 to 64 characters, no surrounding spaces, unique in the library compared without case. Parts and defaults refer to stock by it. |
| `stocks[].kind` | `"sheet"` or `"board"` | Sheet goods (any part lies anywhere on it) or a board (parts are cut along its length, one across its width). |
| `stocks[].length`, `width`, `thickness` | number | Metres, each 1 mm to 10 m; the thickness (actual, not nominal) at most the width and the length. `length` runs along the grain. |
| `stocks[].nominal` | string, optional | A board's nominal thickness ("4/4", "8/4"), at most 16 characters; "n/4" also gives the board feet bought (n/4 in thick). |
| `stocks[].grain` | `"length"` or `"none"` | The grain runs along the length (default), or there is none (MDF): only then may a grained part turn across it. |
| `defaults` | `{ material: stock name }`, optional | Which stock the parts of a material (or of a part's stock text) come from; material names unique without case, each value the exact `name` of a stock in `stocks`. |
| `settings.kerf` | number | Width of every cut, metres, 0 to 1/2 in (0.0127). Default 1/8 in. |
| `settings.trim` | number | Taken off every edge of a sheet and both ends of a board first, metres, 0 to 4 in (0.1016). A trim narrower than the kerf takes a kerf. |
| `settings.algorithm` | `"guillotine"` or `"maxrects"` | Straight cuts edge to edge (default), or the tightest rectangle packing. |

A reader refuses a library that breaks any rule above (as it refuses any invalid model). The nesting itself is not stored: it is worked out from the model and this library, and is the same everywhere.

## `geometry.glb`

glTF 2.0 binary. One node per object and per imported mesh, named after it, flat-shaded, with:

- `extras.sawhorse`: `object` (the object id) or `mesh` (the mesh id); `solid` (true for objects); `tag`; `groups` (the names of the groups and components around it, outermost first); for meshes, `note`.
- A custom vertex attribute `_FACE_ID` (FLOAT SCALAR): the persistent face id of the face each vertex belongs to, so faces (and the drawn edges between coplanar faces) can be rebuilt exactly.
- Materials as `pbrMetallicRoughness.baseColorFactor` (linear), named after the model's materials.

## Compatibility rules

1. Readers ignore unknown entries and unknown JSON fields.
2. A new optional field does not change `version`. A change that an old reader would misread does, and old readers refuse the newer file with a message naming the version.
3. `scene.json` is the source of truth; `geometry.glb` is a view of it and a fallback.
4. Opening a file never repairs it. A file that fails validation is refused with a message; nothing in the open model changes.

## Reference implementation

`crates/shz` (write and read), `crates/kernel/src/snapshot.rs` (the model data and its validation), `crates/kernel/src/nodes.rs` (groups, components and how instances are derived), `crates/kernel/src/stock.rs` (the stock library and its rules), `crates/export/src/gltf.rs` (`geometry.glb`). Tests: `crates/shz/src/lib.rs` (save then open gives the same model hash, with and without groups, components, tags and scenes; a file with no B-rep is rebuilt from `geometry.glb` in place; lane-6 flat groups still open; refusals), `crates/kernel/src/snapshot.rs` (every golden document round-trips), `crates/kernel/tests/structure.rs` (a definition's geometry is stored once; tampered instances are refused).
