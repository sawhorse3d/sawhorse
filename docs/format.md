# The `.shz` file format, version 1

Status: open specification, 2026-10-11 (lane 6; groups, components, tags and scenes added in lane 5, cut-list part attributes in lane 7, the stock library and nesting settings in lane 9, materials, face paint, textures, styles and shadows in lane 10, section planes and drawing sheets in lane 12, all as optional fields). License: this document is CC0; the format is free to implement by anyone, for anything.

A `.shz` file is a Sawhorse model. It is a ZIP archive (APPNOTE 6.3, Deflate or Stored entries) with these entries:

| Entry | Required | What |
|---|---|---|
| `mimetype` | yes (Sawhorse writes it first) | The ASCII text `application/vnd.sawhorse.shz`, **stored** (uncompressed) as the first entry, so a reader can identify the file from its first bytes without unzipping (the text starts at byte 38: a 30-byte local header, then the 8-byte name). Readers must not depend on it. |
| `scene.json` | yes | Everything about the model: the header below and the model data. UTF-8 JSON. |
| `geometry.glb` | yes (Sawhorse writes it) | Every solid and imported mesh as glTF 2.0 binary, so any glTF viewer can show a `.shz`. Also the fallback geometry for objects that `scene.json` lists without a `brep`. |
| `thumbnail.webp` / `.jpg` / `.png` | no | A picture of the model (Sawhorse since lane 15: the view at save time as WebP, or JPEG where the browser cannot write WebP, at most 512 px on the long side and at most 100 KB; earlier files: PNG). Named by `scene.json`'s `thumbnail`; a reader takes any of the three. A writer leaves out a picture over 100 KB. |
| `plugin-data.json` | no | Plugins' document-scoped stores (lane 11, ADR-0009): `{"version": 1, "plugins": {"<plugin id>": {"<key>": <JSON value>, ...}}}`, at most 64 KB per plugin and 1 MB of text in all, at most 4 MB as stored. Written only when a plugin stored something in the document; readers that do not run plugins ignore it. Not part of the model hash. Since lane 16 Sawhorse writes and reads it with the rest of the file (`crates/shz`: deflated, read through the same caps as every entry) and hands its text to the app, which never unzips a user's file itself; the other entries keep their methods (`mimetype` and images stored). A reader drops any store whose key is `__proto__` and any store over its size. |
| `assets/` | no | Texture images (PNG or JPEG), one entry per image, named by `scene.json`'s `textures[].path` and referenced from materials by that path. Stored, not compressed again. |

Readers must ignore entries and JSON fields they do not know. Writers must not put required meaning into new fields without raising `version`.

## `scene.json`

```json
{
  "format": "sawhorse-shz",
  "version": 1,
  "generator": "Sawhorse 0.1.0-alpha.2",
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

(`nodes`, `definitions`, `context`, `tag_colors` and `stock` are optional and left out when empty, or for `stock` when the model uses the built-in library; so are lane 10's `textures`, `style` and `shadows`, and lane 12's `sections` and `sheets`.)

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

A B-rep: `points` (`[[x, y, z], ...]`), `faces` and `user_edges`. A face: `id` (persistent face id, unique in the object, at least 1), `outer` (indices into `points`, counter-clockwise seen from outside the solid), `holes` (lists of indices, clockwise seen from outside), `plane` (as for sketches; the face's frame, with `normal` pointing out of the solid), and optionally `material` (the face's own material, a name in `materials`; absent: the solid's) and `uv` (how a texture sits on the face, below). `user_edges` lists `[a, b]` index pairs of edges a person drew: the only edges allowed between two coplanar faces. Optional `edge_flags` (lane 15) lists `[face id, face id, flags]`: the edge between those two faces is soft (1, not drawn), smooth (2, shaded across) and / or hidden (4, not drawn or picked); flags are 1 to 7 and the two faces must meet along an edge, else the file is refused (ADR-0011). A mesh (and a definition's mesh member) has `edge_flags` too, as `[vertex, vertex, flags]`. Absent: every edge hard.

**What a face shows**: its own `material`, else its solid's, else the nearest enclosing node's (`nodes[].material`, or the definition member's for a nested node). Absent everywhere: the default look.

**Face `uv`** `{ "scale"?: [sx, sy], "rotation"?: degrees, "offset"?: [x, y] }` (defaults `[1, 1]`, `0`, `[0, 0]`). Texture coordinates of a point `p` on a face, in repeats of the image (x along its width, y along its height from its first row):

1. The face's texture frame is its `plane` (`origin`, `u`, `v`), turned a quarter (`u' = v`, `v' = -u`) when the outer ring's extent along `v` is longer than along `u` (more than 1e-9 relatively): the image's width runs along the face's longer side.
2. `q = ((p - origin) . u', (p - origin) . v') - material.offset - uv.offset`.
3. Turn by `-(material.rotation + uv.rotation)` degrees: `r = (cos t q.x + sin t q.y, -sin t q.x + cos t q.y)` with `t` that angle.
4. `(r.x / (W * sx), r.y / (H * sy))`, where `(W, H)` is the material's `size`, or 0.3048 m across at the image's aspect when it has none.

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
| `material` | string, optional | The node's material (lane 10): what faces inside it without a material of their own show. A top-level instance's is its own (copies may differ); a nested node repeats its definition member's. |

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
- `"group"`: `xform` (optional) of a nested group, in definition coordinates, and `material` (optional).
- `"instance"`: `definition` (index into `definitions`), `xform` (optional), a nested instance placed in definition coordinates, and `material` (optional). A definition must not contain itself, directly or through others.

A solid member's faces may carry `material` and `uv` like any B-rep's; an instance's solids show them too.

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
| `materials[]` | `{ "name", "color": [r, g, b, a], "texture"?, "size"?, "rotation"?, "offset"?, "stock"? }` | sRGB colour with alpha, 0 to 255; `texture` names an image (a path in `textures`, or the original file name when the image was not kept, in which case the colour shows); `size` the image's real-world width and height in metres (absent: 0.3048 m across at the image's aspect); `rotation` degrees; `offset` metres; `stock` `{ "kind": "lumber" or "sheet", "species"? }`: what the cut list calls a part showing this material (the species, else the material's name) and whether it is sheet goods, unless the part says otherwise. |
| `textures[]` | `{ "path", "mime" }`, optional | The images the model holds: `path` is `assets/<name>.png` or `.jpg` (letters, digits, `.`, `_`, `-`) and the archive entry holding the bytes; `mime` is `image/png` or `image/jpeg` and must match the bytes. At most 16 MB each and 512 in all. A reader that finds an entry missing opens the file with a note; materials naming it show their colour. (An op log or an in-memory snapshot carries the bytes inline as base64 `data`; a `.shz` never does.) |
| `style` | style, optional | The drawing style (absent: the default). `{ "preset"?, "face"?: "textured" / "shaded" / "monochrome" / "hidden_line" / "wireframe" / "x_ray", "edges"?: { "edges"?, "profiles"?, "extensions"?, "endpoints"? }, "background"?: [r, g, b], "sky"?: [r, g, b], "ground"?: [r, g, b], "section_cuts"?, "section_planes"?, "section_fill"?, "section_fill_color"?: [r, g, b], "section_line_color"?: [r, g, b], "section_line_width"?, "grid"? }`. Absent fields take the defaults: textured, edges on, the rest off, the theme's background, no sky or ground, section cuts, planes and fill on (lane 12: `section_cuts` makes active planes cut, `section_planes` draws the planes, `section_fill` fills the cut faces), the theme's section fill, the edge colour for cut lines, cut lines 3 px (1 to 12), and the ground grid shown (lane 21: `"grid": false` hides it; it is written only when false, so a file from before lane 21 shows the grid). |
| `shadows` | shadows, optional | The sun: `{ "on", "month" (1-12), "day", "minutes" (after midnight, local solar time), "latitude" (degrees, -90..90), "light", "dark" (0..100), "ground", "faces" }`. North is +Y. |
| `scenes[]` | `{ "name", "eye", "target", "up", "fov", "parallel", "hidden_tags"?, "context"?, "style"?, "shadows"?, "sections"? }` | Saved views: camera in metres, vertical field of view in degrees (for a parallel view, the angle a perspective camera at the eye's distance would need to show the same height), parallel or perspective projection, the tags the scene hides (every other tag is shown when it is activated), the node open for editing in it, and the style and shadows it brings back when activated (absent: it leaves them as they are). `sections` (lane 12) is `{ "active"?: [section id, ...], "hidden"?: [section id, ...] }`: activating the scene makes exactly those planes active and hidden and every other plane inactive and shown (`{}`: no plane cuts); absent, the scene leaves the planes as they are. Names are unique. |
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

### Section planes (lane 12)

```json
"sections": [
  { "id": 1, "name": "Section 1", "origin": [0.3048, 0.127, 0.762], "normal": [1, 0, 0], "active": true },
  { "id": 2, "name": "Plan at 30 in", "origin": [0, 0, 0.762], "normal": [0, 0, -1], "parent": 7, "hidden": true }
]
```

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | The plane's id in this file; scenes refer to it. Unique. A reader may renumber planes (Sawhorse does), mapping the scenes' ids along. |
| `name` | string | 1 to 64 characters. |
| `origin` | [x, y, z] | A point on the plane, metres, model coordinates. |
| `normal` | [x, y, z] | The direction of the side the plane **keeps**. A reader normalises it and refuses a zero or non-finite one. Everything on the other side is cut away while the plane is active. |
| `parent` | integer, optional | The node (`nodes[].id`) the plane is in: a group or a component instance (the plane belongs to that copy, not to the definition). Absent: the model's top level. |
| `active` | boolean, optional | The plane cuts. At most one active plane per `parent` (the top level counts as one). Absent: false. |
| `hidden` | boolean, optional | The plane is not drawn (an active hidden plane still cuts). Absent: false. |

The cut itself is not stored: it is computed from the planes and the solids, the same everywhere. A plane cuts every solid inside its context, at any depth; a solid is cut by every active plane of the contexts around it.

### Drawing sheets (lane 12)

`sheets[]` lists the document's sheets in order. Paper coordinates are **metres from the paper's top-left corner, y down**, whatever units the sheet shows; a rectangle is `[x, y, width, height]`. Model points are metres, Z up, as everywhere else.

```json
"sheets": [
  {
    "name": "Bookshelf",
    "paper": { "kind": "letter" },
    "landscape": true,
    "margins": [0.0127, 0.0127, 0.0127, 0.0127],
    "units": "inches",
    "grid": 0.003175,
    "format": { "kind": "fraction", "den": 16 },
    "title": { "title": "Bookshelf", "project": "Shop drawings", "date": "2026-10-12", "revision": "A" },
    "items": [
      { "id": 1, "kind": "viewport", "rect": [0.01524, 0.01524, 0.09144, 0.17272], "source": { "from": "scene", "name": "Front" }, "scale": { "kind": "ratio", "paper": 1, "model": 12 }, "caption": "FRONT ELEVATION 1:12" },
      { "id": 4, "kind": "dimension", "viewport": 1, "dim": "horizontal", "points": [[0, 0, 0], [0.9144, 0, 0]], "offset": -0.00762, "style": "arrows" },
      { "id": 7, "kind": "label", "at": [0.127, 0.187], "text": "Fixed shelf", "size": 0.003175, "leader": { "on": "model", "viewport": 1, "point": [0.6096, 0, 0.5461] } },
      { "id": 8, "kind": "table", "rect": [0.127, 0.1238, 0.1397, 0.0413], "columns": ["part", "qty", "thickness", "width", "length", "material", "amount", "notes"], "title": "CUT LIST" }
    ],
    "next_item": 9
  }
]
```

| Field | Type | Meaning |
|---|---|---|
| `name` | string | 1 to 64 characters. |
| `paper` | `{ "kind": "letter" / "legal" / "tabloid" / "a4" / "a3" / "a2" }` or `{ "kind": "custom", "width", "height" }` | The paper, given portrait (Letter 8.5 x 11 in, Legal 8.5 x 14, Tabloid 11 x 17, A4 210 x 297 mm, A3 297 x 420, A2 420 x 594; custom in metres, 50 mm to 2 m each side). |
| `landscape` | boolean, optional | Turned on its side. Absent: portrait. |
| `margins` | [top, right, bottom, left] | Metres; left and right together, and top and bottom together, stay under 80% of the paper. Default 1/2 in each. |
| `units` | `"inches"` or `"millimetres"` | What the sheet's rulers, grid and fields show. Default inches. |
| `grid` | number | The snap grid, metres, 0 (none) to 50 mm. Default 1/8 in. |
| `format` | `{ "kind": "fraction", "den" }`, `{ "kind": "decimal", "places" }`, `{ "kind": "feet_inches", "den" }` or `{ "kind": "millimetres", "places" }` | How dimensions write lengths. Default fractions to 1/16 in. |
| `title` | `{ "show"?, "title"?, "project"?, "sheet"?, "date"?, "scale"?, "drawn_by"?, "revision"? }` | The title block in the bottom-right corner. `show` defaults to true; empty or absent fields print nothing, except `sheet` ("n of N" from the order) and `scale` (the viewports' scale, or "As noted"). At most 500 characters each. |
| `items[]` | item | Drawn in list order (later on top). At most 2,000 per sheet; at most 200 sheets. |
| `items[].id` | integer | The item's id on its sheet, from 1, never reused. Dimensions and leaders refer to viewports by it. |
| `next_item` | integer | The id the next item gets. |

Items, by `kind`:

| `kind` | Fields | Meaning |
|---|---|---|
| `viewport` | `rect`, `source`, `scale`, `centre`?, `hidden_lines`?, `caption`?, `border`?, `ortho_scale`? | A hidden-line drawing of the model. `source`: `{ "from": "scene", "name" }` (the scene's camera, hidden tags, style and section planes), `{ "from": "standard", "view": "front" / "back" / "left" / "right" / "top" / "bottom" / "iso" }` (the model as it is; front looks along +Y, top looks down) or `{ "from": "camera", "camera": { "eye", "target", "up", "parallel", "fov" } }`. `scale`: `{ "kind": "ratio", "paper", "model" }` (1:8 is paper 1, model 8), `{ "kind": "architect", "inches_per_foot" }` (1/4" = 1'-0" is 0.25) or `{ "kind": "nts" }` (not to scale: the drawing fits the rectangle; isometric and perspective views are always NTS). `centre`: the drawing point (metres along the view's right and up) at the rectangle's centre; absent, the drawing is centred. `hidden_lines`: hidden edges drawn dashed. `caption`: the text under it. `border`: its frame drawn. `ortho_scale` (lane 12c, optional, never `nts`): the scale it had when it last showed a view to scale, kept while it shows an isometric or perspective one, so going back to a square view restores it; absent in older files. |
| `dimension` | `viewport`, `dim`, `points`, `anchors`?, `offset`, `style`?, `text`? | Measures the **model**: `points` are model points seen in the viewport (two; three for `angular`, the vertex first, and for `radius` and `diameter`, three points on the arc), where the ends were placed. `anchors` (one per point, `null` for a point on no solid) puts a point on a solid so it follows the solid's edits: `{ "object", "at": [face ids], "to"?: [face ids], "t"? }` is the corner of solid `object` where the faces with those persistent ids meet, or with `to`, the point `t` (0 to 1) of the way from that corner to the one where `to` meet. Face ids ascending, at most 64 per corner. A point whose corner no longer exists (or with no anchor) is drawn at its stored point. Absent in files from before anchors. `dim`: `horizontal` or `vertical` (along the view's right or up), `aligned`, `angular`, `radius`, `diameter`. `offset`: how far the dimension line stands off its points, paper metres, signed. `style`: `arrows` (default), `ticks`, `dots`. `text`: written instead of the measured value. The value is never stored: it is read from the points each time, in the sheet's `format`: in the view's plane for a viewport that can be to scale, and (lane 12c) the model's own length, angle or radius for an isometric or perspective one, where `horizontal` and `vertical` read "NTS". The viewport must be on the same sheet. |
| `label` | `at`, `text`, `size`, `leader`? | Text at `at` (paper), `size` its height in paper metres. `leader`: `{ "on": "paper", "at" }` or `{ "on": "model", "viewport", "point", "anchor"? }` (a model point seen in a viewport of the same sheet: the leader follows it; with an `anchor`, as for a dimension, it follows the solid too). |
| `table` | `rect`, `columns`, `decimal`?, `visible_only`?, `title`? | The model's cut list as a table, worked out when the sheet is drawn. `columns` from `part`, `qty`, `thickness`, `width`, `length`, `size` (T x W x L in one cell), `material`, `grain`, `amount` (board feet or area), `notes`. `decimal`: sizes as decimals instead of fractions. `visible_only`: hidden parts left out. |
| `image` | `rect`, `path` | An image the model holds (`textures[].path`). |
| `line` | `a`, `b`, `width`, `dashed`? | A line on the paper; `width` is the pen, paper metres. |
| `rect` | `rect`, `width`, `fill`? | A rectangle; `fill` an sRGB colour `[r, g, b]`. |

Renaming a scene renames it in the viewports that show it; deleting one leaves those viewports on its camera. A file whose sheets break a rule above is refused, as any invalid model is. Drawings, dimension values and tables are not stored: they are laid out from the model, the same everywhere.

## `geometry.glb`

glTF 2.0 binary. One node per object and per imported mesh, named after it, flat-shaded, with:

- `extras.sawhorse`: `object` (the object id) or `mesh` (the mesh id); `solid` (true for objects); `tag`; `groups` (the names of the groups and components around it, outermost first); for meshes, `note`.
- A custom vertex attribute `_FACE_ID` (FLOAT SCALAR): the persistent face id of the face each vertex belongs to, so faces (and the drawn edges between coplanar faces) can be rebuilt exactly.
- Materials as `pbrMetallicRoughness.baseColorFactor` (linear), named after the model's materials; a solid whose faces show different materials has one primitive per material. The `.shz` copy leaves texture images out (they are in `assets/`); a GLB exported on its own embeds them with `TEXCOORD_0`.

## Reading limits (lane 16)

A reader may refuse a file that would take unreasonable memory or time to read; Sawhorse refuses, with a plain message and nothing changed:

- an archive of more than 2,048 entries, a name listed twice, or two entries sharing compressed bytes;
- an entry whose declared uncompressed size is over its cap (`scene.json` 256 MB, `geometry.glb` 512 MB, a texture 16 MB, the thumbnail 100 KB, `plugin-data.json` 4 MB), whose declared size implies better than 1,100 to 1 compression (over 1 MB), or whose inflated bytes are not exactly its declared size; more than 1 GB in all;
- more than 512 `textures` rows, or one path listed twice (checked before any image is read); images over 4096 x 4096 pixels, or over 256 MB or 128 million pixels together (read from their headers, before any is drawn);
- a name over 64 characters, a mesh note over 1,000, a material texture reference over 1,024; a mesh point, guide or dimension more than 10 km from the origin or not a number; a scene camera more than 1,000 km away;
- components nested more than 64 deep, a definition that contains itself, or a model whose components would derive more than the model limits (100,000 solids and meshes, 100,000 groups and instances, 20,000 definitions, 5,000,000 faces, 10,000,000 points), counted along the definitions before anything is derived.
- (lane 16b) more than a model may hold of what `scene.json` lists: 50,000 `guides`, 50,000 `dimensions`, 10,000 `tags`, 10,000 `tag_colors`, 4,096 `materials`, 1,000 `scenes` (each hiding at most 10,000 tags), 1,000 `sections`, 200 `sheets` (each of at most 2,000 items), 512 `textures`, 100,000 `objects` or `meshes`, 100,000 `nodes`, 20,000 `definitions`. A reader counts these as it reads `scene.json`, without building them, and may stop at the first element past a limit; a file that lists more is refused as a whole.

Files Sawhorse writes are always within these. `docs/LIMITS.md` lists every limit with its reason.

## Compatibility rules

1. Readers ignore unknown entries and unknown JSON fields.
2. A new optional field does not change `version`. A change that an old reader would misread does, and old readers refuse the newer file with a message naming the version.
3. `scene.json` is the source of truth; `geometry.glb` is a view of it and a fallback.
4. Opening a file never repairs it. A file that fails validation is refused with a message; nothing in the open model changes.

## Reference implementation

`crates/shz` (write and read), `crates/kernel/src/snapshot.rs` (the model data and its validation), `crates/kernel/src/nodes.rs` (groups, components and how instances are derived), `crates/kernel/src/stock.rs` (the stock library and its rules), `crates/kernel/src/section.rs` and `sheets.rs` (section planes and sheets, lane 12), `crates/export/src/gltf.rs` (`geometry.glb`). Tests: `crates/shz/src/lib.rs` (save then open gives the same model hash, with and without groups, components, tags and scenes; a file with no B-rep is rebuilt from `geometry.glb` in place; lane-6 flat groups still open; refusals), `crates/kernel/src/snapshot.rs` (every golden document round-trips), `crates/kernel/tests/structure.rs` (a definition's geometry is stored once; tampered instances are refused).
