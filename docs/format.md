# The `.shz` file format, version 1

Status: open specification, 2026-10-11 (lane 6). License: this document is CC0; the format is free to implement by anyone, for anything.

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
  "tags": {},
  "materials": [],
  "scenes": [],
  "guides": [],
  "dimensions": []
}
```

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

### `objects[]`

An object is a watertight solid.

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | The id it had when saved; `groups` refer to objects by it. Unique in the file. |
| `name`, `tag`, `material` | string, optional | Display name; tag (layer) name, which must be a key of `tags`; material name, which must be in `materials`. |
| `next_face_id` | integer | The next persistent face id the object will hand out. Greater than every face id. |
| `source_sketch` | integer, optional | Index into `sketches` of the sketch it was extruded from. |
| `brep` | B-rep, optional | Exact geometry (below). A writer that cannot produce it may leave it out; the reader then rebuilds the object from the `geometry.glb` node whose `extras.sawhorse.object` equals `id`. |

A B-rep: `points` (`[[x, y, z], ...]`), `faces` and `user_edges`. A face: `id` (persistent face id, unique in the object, at least 1), `outer` (indices into `points`, counter-clockwise seen from outside the solid), `holes` (lists of indices, clockwise seen from outside), `plane` (as for sketches; the face's frame, with `normal` pointing out of the solid). `user_edges` lists `[a, b]` index pairs of edges a person drew: the only edges allowed between two coplanar faces.

The B-rep must be a valid solid: every edge shared by exactly two faces, once in each direction; every face planar (vertices within 1 micrometre of its plane) and simple; no face touches another except along shared edges and vertices; the solid encloses positive volume. A reader refuses a file whose solids do not validate; it does not repair them.

### `meshes[]`

Imported geometry that is not a valid solid (kept so nothing is lost on import). `id`, `name`, `tag`, `material` as for objects; `note` (why it is not a solid, in plain words); `positions` (`[[x, y, z], ...]`); `faces` (each a list of rings, outer first, of indices into `positions`); `lines` (`[a, b]` pairs: edges with no face).

### Other tables

| Field | Type | Meaning |
|---|---|---|
| `groups[]` | `{ "name", "objects": [id, ...] }` | Named sets of objects (by `objects[].id`). |
| `tags` | `{ name: visible }` | Every tag used, and whether it is shown. |
| `materials[]` | `{ "name", "color": [r, g, b, a], "texture"? }` | sRGB colour with alpha, 0 to 255; `texture` names an image (a path inside `assets/`, or the original file name when the image was not kept). |
| `scenes[]` | `{ "name", "eye", "target", "up", "fov", "parallel", "hidden_tags"? }` | Saved views: camera in metres, vertical field of view in degrees, parallel or perspective projection, the tags the scene hides. |
| `guides[]` | `{ "point", "dir"? }` | Construction lines (a point and a unit direction) and guide points (no `dir`). |
| `dimensions[]` | `{ "a", "b", "offset" }` | Linear dimensions between `a` and `b`, the line drawn at `offset` from them. |

## `geometry.glb`

glTF 2.0 binary. One node per object and per imported mesh, named after it, flat-shaded, with:

- `extras.sawhorse`: `object` (the object id) or `mesh` (the mesh id); `solid` (true for objects); `tag`; `groups` (names); for meshes, `note`.
- A custom vertex attribute `_FACE_ID` (FLOAT SCALAR): the persistent face id of the face each vertex belongs to, so faces (and the drawn edges between coplanar faces) can be rebuilt exactly.
- Materials as `pbrMetallicRoughness.baseColorFactor` (linear), named after the model's materials.

## Compatibility rules

1. Readers ignore unknown entries and unknown JSON fields.
2. A new optional field does not change `version`. A change that an old reader would misread does, and old readers refuse the newer file with a message naming the version.
3. `scene.json` is the source of truth; `geometry.glb` is a view of it and a fallback.
4. Opening a file never repairs it. A file that fails validation is refused with a message; nothing in the open model changes.

## Reference implementation

`crates/shz` (write and read), `crates/kernel/src/snapshot.rs` (the model data and its validation), `crates/export/src/gltf.rs` (`geometry.glb`). Tests: `crates/shz/src/lib.rs` (save then open gives the same model hash; a file with no B-rep is rebuilt from `geometry.glb`; refusals), `crates/kernel/src/snapshot.rs` (every golden document round-trips).
