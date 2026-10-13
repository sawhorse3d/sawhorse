//! wasm-bindgen surface of the Sawhorse kernel, loaded by the front end's kernel Web Worker
//! in both the browser build and the Tauri webview (ADR-0001).
//!
//! The worker holds one persistent [`Doc`] (a kernel [`Document`]: model, op log, undo
//! ring) for the whole session; every edit is an op applied transactionally. Everything
//! here is a thin wrapper over [`DocCore`], which is plain Rust and tested natively; the
//! `#[wasm_bindgen]` layer only converts errors into `JsError` and vectors into typed
//! arrays.
//!
//! JS usage (see src/kernel/kernel.worker.ts):
//! ```js
//! const doc = document_new();                         // ground sketch is id 1
//! doc.apply_op('{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6,"h":0.4}');
//! doc.apply_op('{"op":"extrude","sketch":1,"region":1,"distance":0.3}');
//! doc.undo(); doc.redo();
//! const mesh = doc.get_scene_mesh();                  // or doc.get_view_mesh(objectId)
//! const hash = doc.document_hash();
//! const stl = doc.export_stl(1), obj = doc.export_obj(1);
//! ```
//! Ids cross the boundary as JS numbers (public ids; exact below 2^53).

mod drawings;

use sawhorse_kernel::document::Document;
use sawhorse_kernel::mesh::{ViewMesh, object_view_mesh, view_mesh};
use sawhorse_kernel::model::Op;
use wasm_bindgen::prelude::*;

/// Kernel version string, for the UI's about box and the worker handshake.
#[wasm_bindgen]
pub fn kernel_version() -> String {
    sawhorse_kernel::VERSION.to_string()
}

/// Which checkout built this kernel: the FNV-1a hash (64-bit, hex) of this crate's directory
/// as the compiler saw it. `scripts/build-wasm.mjs` and `scripts/golden-wasm.mjs` compare it
/// with their own checkout, so a kernel another checkout built into a shared cargo target
/// dir (cargo copies the unhashed `sawhorse_wasm.wasm` out only when it compiles, so a
/// checkout it thinks is fresh gets whatever was copied last) is caught, never loaded. A
/// hash, so no machine path is ever written into the build.
#[wasm_bindgen]
pub fn build_checkout() -> String {
    format!("{BUILD_CHECKOUT:016x}")
}

const BUILD_CHECKOUT: u64 = fnv1a(env!("CARGO_MANIFEST_DIR").as_bytes());

const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Plain-Rust document logic behind the JS class. Errors are strings (a refusal message).
#[derive(Debug, Clone)]
pub struct DocCore {
    doc: Document,
    /// Picking / inference picture of the current state, rebuilt lazily after edits.
    scene: std::cell::RefCell<Option<std::rc::Rc<sawhorse_inference::scene::Scene>>>,
    /// The last sheet viewport's drawing a snap used (lane 12, `drawings.rs`): a cursor
    /// moving over a viewport asks again and again between edits. Dropped with `scene`.
    snaps: std::cell::RefCell<Option<drawings::SnapCache>>,
}

impl Default for DocCore {
    fn default() -> Self {
        DocCore::from_document(Document::with_ground_sketch())
    }
}

/// A sketch's plane frame, for the UI to convert world points to sketch coordinates.
#[derive(Debug, serde::Serialize)]
struct SketchInfo {
    id: u64,
    origin: [f64; 3],
    normal: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
}

/// Summary the UI shows after every op.
#[derive(Debug, serde::Serialize)]
struct DocState<'a> {
    stats: sawhorse_kernel::ModelStats,
    can_undo: bool,
    can_redo: bool,
    undo_depth: usize,
    redo_depth: usize,
    log_len: usize,
    log_hash: String,
    objects: Vec<u64>,
    /// Per solid, in creation order: display number, bounds and volume.
    solids: Vec<SolidInfo>,
    sketches: Vec<SketchInfo>,
    /// Groups (not instances): name and the solids directly in each (lane-2 shape).
    groups: Vec<GroupInfo>,
    /// The model's tree: groups, instances, definitions, where every solid is, the open
    /// editing context and the path to it, tag colours (see `sawhorse_kernel::nodes::Graph`).
    graph: sawhorse_kernel::nodes::Graph,
    guides: Vec<GuideInfo>,
    dimensions: Vec<DimensionInfo>,
    tags: &'a std::collections::BTreeMap<String, bool>,
    /// Every object's name, tag and material (same order as `objects`).
    object_info: Vec<ObjectInfo<'a>>,
    /// Imported non-solid meshes, with the note saying why each is not a solid.
    meshes: Vec<MeshInfo<'a>>,
    materials: Vec<MaterialInfo<'a>>,
    scenes: &'a [sawhorse_kernel::model::SceneDef],
    /// Texture images the model holds (lane 10): path, type, pixel size and a digest the
    /// worker sends each image's bytes by, once.
    textures: Vec<TextureInfo<'a>>,
    /// The drawing style and the sun (lane 10); `null`: the defaults.
    style: Option<&'a sawhorse_kernel::looks::Style>,
    shadows: Option<&'a sawhorse_kernel::looks::Shadows>,
}

#[derive(Debug, serde::Serialize)]
struct TextureInfo<'a> {
    path: &'a str,
    mime: &'a str,
    width: u32,
    height: u32,
    digest: String,
}

#[derive(Debug, serde::Serialize)]
struct ObjectInfo<'a> {
    id: u64,
    name: Option<&'a str>,
    tag: Option<&'a str>,
    material: Option<&'a str>,
}

#[derive(Debug, serde::Serialize)]
struct MeshInfo<'a> {
    id: u64,
    name: &'a str,
    note: &'a str,
    faces: usize,
    lines: usize,
    tag: Option<&'a str>,
    visible: bool,
}

#[derive(Debug, serde::Serialize)]
struct MaterialInfo<'a> {
    name: &'a str,
    color: [u8; 4],
    texture: Option<&'a str>,
    /// The texture is an image the model holds (not just a file name).
    held: bool,
    /// Texture size, metres (as drawn: its own, else one foot across at the image's aspect).
    size: [f64; 2],
    /// The size was set (not the default).
    size_set: bool,
    rotation: f64,
    offset: [f64; 2],
    stock: Option<&'a sawhorse_kernel::looks::StockLink>,
    /// Faces showing it (their own or inherited), for the Materials pane's "in model" list.
    used: usize,
}

/// Options for [`DocCore::export`] (JSON from the UI).
#[derive(Debug, Default, serde::Deserialize)]
struct ExportOptions {
    /// What to export; every visible solid and mesh when absent.
    #[serde(default)]
    scope: Option<sawhorse_export::Scope>,
    /// The camera, for `svg-view`.
    #[serde(default)]
    view: Option<sawhorse_export::svg::ViewSpec>,
    /// The face (`{"kind":"face","owner":object,"index":face}`) or sketch region
    /// (`{"kind":"region","owner":sketch,"index":region}`) for the outline formats.
    #[serde(default)]
    outline: Option<OutlineTarget>,
    #[serde(default)]
    flavour: sawhorse_export::dxf::OutlineFlavour,
    /// The MTL file an OBJ names (`mtllib`).
    #[serde(default)]
    mtl_file: Option<String>,
    /// The section plane a slice export draws (lane 12).
    #[serde(default)]
    section: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct OutlineTarget {
    kind: String,
    owner: u64,
    index: u64,
}

/// One solid as the UI names and measures it.
#[derive(Debug, serde::Serialize)]
struct SolidInfo {
    id: u64,
    /// Display number: the UI says "solid 3", never the public id (which carries slot-reuse
    /// bits once objects are deleted).
    number: u64,
    min: [f64; 3],
    max: [f64; 3],
    volume: f64,
}

/// A guide: a line through `point` along `dir`, or a point (`dir` null).
#[derive(Debug, serde::Serialize)]
struct GuideInfo {
    id: u64,
    point: [f64; 3],
    dir: Option<[f64; 3]>,
}

#[derive(Debug, serde::Serialize)]
struct DimensionInfo {
    id: u64,
    a: [f64; 3],
    b: [f64; 3],
    offset: [f64; 3],
    length: f64,
}

#[derive(Debug, serde::Serialize)]
struct GroupInfo {
    id: u64,
    name: String,
    objects: Vec<u64>,
}

impl DocCore {
    pub fn from_document(doc: Document) -> DocCore {
        DocCore {
            doc,
            scene: std::cell::RefCell::new(None),
            snaps: std::cell::RefCell::new(None),
        }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Apply one op given as JSON; returns the outcome as JSON.
    pub fn apply_json(&mut self, op_json: &str) -> Result<String, String> {
        let op: Op = serde_json::from_str(op_json).map_err(|e| format!("bad op: {e}"))?;
        let out = self.doc.apply(&op).map_err(|e| e.to_string())?;
        self.scene.replace(None);
        self.snaps.replace(None);
        serde_json::to_string(&out).map_err(|e| e.to_string())
    }

    /// The picking / inference scene for the current state (cached until the next edit).
    pub fn scene(&self) -> std::rc::Rc<sawhorse_inference::scene::Scene> {
        if let Some(s) = self.scene.borrow().as_ref() {
            return s.clone();
        }
        let s = std::rc::Rc::new(sawhorse_inference::scene::Scene::build(self.doc.model()));
        self.scene.replace(Some(s.clone()));
        s
    }

    /// What is under the cursor (see `sawhorse_inference::pick`): JSON in, JSON (or null) out.
    pub fn pick_json(&self, query_json: &str) -> Result<String, String> {
        let q: sawhorse_inference::pick::PickQuery =
            serde_json::from_str(query_json).map_err(|e| format!("bad pick query: {e}"))?;
        let r = sawhorse_inference::pick::pick(&self.scene(), &q);
        serde_json::to_string(&r).map_err(|e| e.to_string())
    }

    /// Where the cursor snaps (see `sawhorse_inference::infer`): JSON in, JSON (or null) out.
    pub fn infer_json(&self, query_json: &str) -> Result<String, String> {
        let q: sawhorse_inference::infer::InferQuery =
            serde_json::from_str(query_json).map_err(|e| format!("bad inference query: {e}"))?;
        let r = sawhorse_inference::infer::infer(&self.scene(), &q);
        serde_json::to_string(&r).map_err(|e| e.to_string())
    }

    pub fn undo(&mut self) -> Result<String, String> {
        self.apply_json(r#"{"op":"undo"}"#)
    }

    /// A plugin's named batch (lane 11): `{name, ops}` where ops may hold placeholders
    /// (`{"$ref": i, "path": "regions.0"}`). Applied as one op, one undo step (see
    /// `sawhorse_kernel::refbatch`). Returns the batch's outcome as JSON; a refusal names
    /// the op and changes nothing.
    pub fn apply_named_batch_json(&mut self, json: &str) -> Result<String, String> {
        let batch: sawhorse_kernel::refbatch::NamedBatch =
            serde_json::from_str(json).map_err(|e| format!("bad batch: {e}"))?;
        let (out, _) =
            sawhorse_kernel::refbatch::apply(&mut self.doc, &batch).map_err(|e| e.to_string())?;
        self.scene.replace(None);
        self.snaps.replace(None);
        serde_json::to_string(&out).map_err(|e| e.to_string())
    }

    /// What an offset would draw, without drawing it: `{source, distance}` in (the
    /// `Op::Offset` fields), `{loops: [[[x,y,z]...]...], closed}` out (world, metres). A
    /// refusal (too far inward, off-plane edges) is an error with the kernel's message.
    pub fn offset_preview_json(&self, query_json: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct Query {
            source: sawhorse_kernel::offset::OffsetSource,
            distance: f64,
        }
        let q: Query =
            serde_json::from_str(query_json).map_err(|e| format!("bad offset query: {e}"))?;
        let (loops, closed, _) = self
            .doc
            .model()
            .offset_result(&q.source, q.distance)
            .map_err(|e| e.to_string())?;
        let loops: Vec<Vec<[f64; 3]>> = loops
            .iter()
            .map(|r| r.iter().map(|p| p.to_array()).collect())
            .collect();
        Ok(serde_json::json!({ "loops": loops, "closed": closed }).to_string())
    }

    pub fn redo(&mut self) -> Result<String, String> {
        self.apply_json(r#"{"op":"redo"}"#)
    }

    pub fn scene_mesh(&self) -> Result<ViewMesh, String> {
        view_mesh(self.doc.model()).map_err(|e| e.to_string())
    }

    pub fn object_mesh(&self, object: u64) -> Result<ViewMesh, String> {
        object_view_mesh(self.doc.model(), object)
            .ok_or_else(|| format!("no object #{object}"))?
            .map_err(|e| e.to_string())
    }

    pub fn state_json(&self) -> String {
        let m = self.doc.model();
        let (undo_depth, redo_depth) = self.doc.depth();
        let st = DocState {
            stats: m.stats(),
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            undo_depth,
            redo_depth,
            log_len: self.doc.log().len(),
            log_hash: self.doc.log_hash_hex(),
            objects: m.object_ids(),
            solids: m
                .objects()
                .into_iter()
                .map(|(id, o)| {
                    let (lo, hi) = o.bounds();
                    SolidInfo {
                        id,
                        number: o.number(),
                        min: lo.to_array(),
                        max: hi.to_array(),
                        volume: o.volume(),
                    }
                })
                .collect(),
            sketches: m
                .sketches()
                .into_iter()
                .map(|(id, s)| {
                    let p = s.plane();
                    SketchInfo {
                        id,
                        origin: p.origin.to_array(),
                        normal: p.normal.to_array(),
                        u: p.u.to_array(),
                        v: p.v.to_array(),
                    }
                })
                .collect(),
            groups: m
                .groups()
                .into_iter()
                .map(|(id, g)| GroupInfo {
                    id,
                    name: g.name,
                    objects: g.objects,
                })
                .collect(),
            graph: m.graph(),
            guides: m
                .guides()
                .into_iter()
                .map(|(id, g)| GuideInfo {
                    id,
                    point: g.point.to_array(),
                    dir: (g.dir != glam::DVec3::ZERO).then(|| g.dir.to_array()),
                })
                .collect(),
            dimensions: m
                .dimensions()
                .into_iter()
                .map(|(id, d)| DimensionInfo {
                    id,
                    a: d.a.to_array(),
                    b: d.b.to_array(),
                    offset: d.offset.to_array(),
                    length: d.length(),
                })
                .collect(),
            tags: m.tags(),
            object_info: m
                .objects()
                .into_iter()
                .map(|(id, o)| ObjectInfo {
                    id,
                    name: o.name(),
                    tag: o.tag(),
                    material: o.material(),
                })
                .collect(),
            meshes: m
                .meshes()
                .into_iter()
                .map(|(id, x)| MeshInfo {
                    id,
                    name: &x.name,
                    note: &x.note,
                    faces: x.faces.len(),
                    lines: x.lines.len(),
                    tag: x.tag.as_deref(),
                    visible: m.is_mesh_visible(id),
                })
                .collect(),
            materials: {
                let mut used: std::collections::BTreeMap<&str, usize> = Default::default();
                for (id, o) in m.objects() {
                    for f in o.brep().persistent_face_ids() {
                        if let Some(name) = m.face_material(id, f) {
                            *used.entry(name).or_default() += 1;
                        }
                    }
                }
                m.materials()
                    .iter()
                    .map(|(name, x)| MaterialInfo {
                        name,
                        color: x.color,
                        texture: x.texture.as_deref(),
                        held: x
                            .texture
                            .as_deref()
                            .is_some_and(|t| m.textures().contains_key(t)),
                        size: sawhorse_kernel::looks::material_size(x, m.texture_px(x)),
                        size_set: x.size.is_some(),
                        rotation: x.rotation,
                        offset: x.offset,
                        stock: x.stock.as_ref(),
                        used: used.get(name.as_str()).copied().unwrap_or(0),
                    })
                    .collect()
            },
            scenes: m.scenes(),
            textures: m
                .textures()
                .iter()
                .map(|(path, t)| TextureInfo {
                    path,
                    mime: &t.mime,
                    width: t.width,
                    height: t.height,
                    digest: t.digest_hex(),
                })
                .collect(),
            style: m.style(),
            shadows: m.shadows(),
        };
        serde_json::to_string(&st).unwrap_or_else(|_| "{}".to_string())
    }

    /// The cut list (see `sawhorse_kernel::cutlist`) for JSON options `{rounding?,
    /// visible_only?}` (empty: the defaults, 1/16 in and everything), as JSON.
    pub fn cut_list_json(&self, options_json: &str) -> Result<String, String> {
        let opts = cut_options(options_json)?;
        serde_json::to_string(&self.doc.model().cut_list(&opts)).map_err(|e| e.to_string())
    }

    /// The nesting (see `sawhorse_kernel::nesting`) for JSON options `{visible_only?,
    /// algorithm?}` (empty: everything, the document's own algorithm), as JSON: every
    /// stock piece with its placements and cuts, the parts no stock takes, the totals, the
    /// stock library it used and a hash. Read-only.
    pub fn nest_json(&self, options_json: &str) -> Result<String, String> {
        let opts = nest_options(options_json)?;
        serde_json::to_string(&self.doc.model().nest(&opts)).map_err(|e| e.to_string())
    }

    /// The Paint tool's sample (Alt+click): the material face `face` of `object` shows, as
    /// JSON (`"White oak"` or `null`).
    pub fn sample_json(&self, object: u64, face: u64) -> Result<String, String> {
        let s = self
            .doc
            .model()
            .sample(object, face)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&s).map_err(|e| e.to_string())
    }

    /// The bytes of a texture image the model holds.
    pub fn texture(&self, path: &str) -> Result<Vec<u8>, String> {
        self.doc
            .model()
            .textures()
            .get(path)
            .map(|t| t.data.as_ref().clone())
            .ok_or_else(|| format!("no texture {path}"))
    }

    /// Texture coordinates face `face` of `object` would get with positioning `uv` (JSON
    /// `{scale?, rotation?, offset?}`), in the view mesh's vertex order: the texture handles'
    /// live preview. Empty when the face shows no texture.
    pub fn face_uvs(&self, object: u64, face: u64, uv_json: &str) -> Result<Vec<f32>, String> {
        let uv: sawhorse_kernel::looks::UvXform =
            serde_json::from_str(uv_json).map_err(|e| format!("bad positioning: {e}"))?;
        Ok(
            sawhorse_kernel::mesh::face_uvs(self.doc.model(), object, face, &uv)
                .unwrap_or_default(),
        )
    }

    /// The built-in material library (see `sawhorse_export::library`), as JSON.
    pub fn library_json() -> String {
        serde_json::to_string(&sawhorse_export::library::entries())
            .unwrap_or_else(|_| "[]".to_string())
    }

    /// The ops that put library material `name` into the model (its texture image and its
    /// definition), named `as_name` when given, as a JSON array: the UI batches them with
    /// the paint so applying a library material is one undo step.
    pub fn library_ops_json(name: &str, as_name: &str) -> Result<String, String> {
        let m = sawhorse_export::library::find(name)
            .ok_or_else(|| format!("no library material {name:?}"))?;
        let ops = sawhorse_export::library::ops(m, (!as_name.is_empty()).then_some(as_name));
        serde_json::to_string(&ops).map_err(|e| e.to_string())
    }

    /// The PNG of a library material's texture (for the Materials pane's swatches).
    pub fn library_texture(name: &str) -> Result<Vec<u8>, String> {
        let m = sawhorse_export::library::find(name)
            .ok_or_else(|| format!("no library material {name:?}"))?;
        sawhorse_export::library::texture_png(m)
            .map(|p| p.as_ref().clone())
            .ok_or_else(|| format!("{name} has no texture"))
    }

    /// The model's tree on its own (what the Outliner shows), as JSON.
    pub fn scene_graph_json(&self) -> String {
        serde_json::to_string(&self.doc.model().graph()).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn log_json(&self) -> String {
        serde_json::to_string(self.doc.log()).unwrap_or_else(|_| "[]".to_string())
    }

    /// Every face of an object by persistent id, in face order: `[{id, normal, origin,
    /// centroid, area}]` (metres). Faces are addressed by these ids in ops, never by
    /// position in this list.
    pub fn faces_json(&self, object: u64) -> Result<String, String> {
        let o = self.object(object)?;
        let p = o.brep().to_polys();
        let faces: Vec<serde_json::Value> = (0..p.faces.len())
            .filter_map(|i| {
                let sig = sawhorse_kernel::boolean::face_signature(&p, i)?;
                Some(serde_json::json!({
                    "id": o.face_id_at(i)?,
                    "normal": sig.plane.normal.to_array(),
                    "origin": sig.plane.origin.to_array(),
                    "centroid": sig.centroid.to_array(),
                    "area": sig.area,
                }))
            })
            .collect();
        serde_json::to_string(&faces).map_err(|e| e.to_string())
    }

    /// The outline of a face (`kind` "face": object id, persistent face id) or a sketch
    /// region (`kind` "region": sketch id, region id) as world rings (outer first), JSON.
    pub fn outline_json(&self, kind: &str, owner: u64, index: u64) -> Result<String, String> {
        let m = self.doc.model();
        let rings: Vec<Vec<[f64; 3]>> = match kind {
            "face" => {
                let o = self.object(owner)?;
                let fi = o
                    .face_ordinal(index)
                    .ok_or_else(|| format!("no face #{index}"))?;
                let p = o.brep().to_polys();
                p.faces[fi]
                    .rings()
                    .map(|r| r.iter().map(|&i| p.points[i].to_array()).collect())
                    .collect()
            }
            "region" => {
                let s = m
                    .sketch(owner)
                    .ok_or_else(|| format!("no sketch #{owner}"))?;
                let r = s
                    .region(sawhorse_kernel::ids::from_public(index))
                    .ok_or_else(|| format!("no region #{index}"))?;
                let (o, h) = s.region_rings(r);
                std::iter::once(o)
                    .chain(h)
                    .map(|ring| {
                        ring.iter()
                            .map(|&q| s.plane().to_world(q).to_array())
                            .collect()
                    })
                    .collect()
            }
            _ => return Err(format!("unknown part kind {kind}")),
        };
        serde_json::to_string(&rings).map_err(|e| e.to_string())
    }

    fn object(&self, id: u64) -> Result<&sawhorse_kernel::object::Object, String> {
        self.doc
            .model()
            .object(id)
            .ok_or_else(|| format!("no object #{id}"))
    }

    pub fn stl(&self, object: u64) -> Result<Vec<u8>, String> {
        sawhorse_export::object_stl(self.object(object)?).map_err(|e| e.to_string())
    }

    pub fn obj(&self, object: u64) -> Result<String, String> {
        sawhorse_export::object_obj(self.object(object)?, &format!("sawhorse-object-{object}"))
            .map_err(|e| e.to_string())
    }

    /// The rings of a face or sketch region in its own plane frame (metres, outer first).
    fn outline_2d(&self, t: &OutlineTarget) -> Result<Vec<Vec<glam::DVec2>>, String> {
        let m = self.doc.model();
        match t.kind.as_str() {
            "face" => {
                let o = self.object(t.owner)?;
                let fi = o
                    .face_ordinal(t.index)
                    .ok_or_else(|| format!("no face #{}", t.index))?;
                let p = o.brep().to_polys();
                let plane = p.plane(fi).ok_or("the face has no plane")?;
                Ok(p.faces[fi]
                    .rings()
                    .map(|r| r.iter().map(|&i| plane.to_local(p.points[i])).collect())
                    .collect())
            }
            "region" => {
                let s = m
                    .sketch(t.owner)
                    .ok_or_else(|| format!("no sketch #{}", t.owner))?;
                let r = s
                    .region(sawhorse_kernel::ids::from_public(t.index))
                    .ok_or_else(|| format!("no region #{}", t.index))?;
                let (o, h) = s.region_rings(r);
                Ok(std::iter::once(o).chain(h).collect())
            }
            k => Err(format!("unknown outline kind {k}")),
        }
    }

    /// Read a file (`name` picks the format by extension, else the bytes do) and apply it
    /// as one import op. Returns `{"outcome": ..., "notes": [...]}` as JSON: the kernel's
    /// outcome (with its own notes on healing and non-solid meshes) and the reader's notes
    /// (units assumed, entities skipped).
    pub fn import(
        &mut self,
        name: &str,
        bytes: &[u8],
        options_json: &str,
    ) -> Result<String, String> {
        let mut opts: sawhorse_import::ImportOptions = if options_json.trim().is_empty() {
            sawhorse_import::ImportOptions::default()
        } else {
            serde_json::from_str(options_json).map_err(|e| format!("bad import options: {e}"))?
        };
        if opts.name.is_empty() {
            opts.name = name.to_string();
        }
        let parsed = sawhorse_import::read(None, bytes, &opts).map_err(|e| e.to_string())?;
        let out = self.doc.apply(&parsed.op()).map_err(|e| e.to_string())?;
        self.scene.replace(None);
        self.snaps.replace(None);
        serde_json::to_string(&serde_json::json!({ "outcome": out, "notes": parsed.notes }))
            .map_err(|e| e.to_string())
    }

    /// The document as a `.shz` (see `docs/format.md`), with the user's unit system and an
    /// optional PNG thumbnail.
    pub fn save_shz(&self, display_units: &str, thumbnail_png: &[u8]) -> Result<Vec<u8>, String> {
        let opts = sawhorse_shz::SaveOptions {
            display_units: (!display_units.is_empty()).then(|| display_units.to_string()),
            thumbnail_png: (!thumbnail_png.is_empty()).then(|| thumbnail_png.to_vec()),
            generator: None,
        };
        sawhorse_shz::write(self.doc.model(), &opts).map_err(|e| e.to_string())
    }

    /// Replace the document with an opened `.shz`. Returns `{"notes": [...],
    /// "display_units": ...}` as JSON. A file that does not open changes nothing.
    pub fn open_shz(&mut self, bytes: &[u8]) -> Result<String, String> {
        let opened = sawhorse_shz::read(bytes).map_err(|e| e.to_string())?;
        let info = serde_json::json!({
            "notes": opened.notes,
            "display_units": opened.scene.units.display,
            "generator": opened.scene.generator,
        });
        *self = DocCore::from_document(opened.document);
        Ok(info.to_string())
    }

    /// Write the document (or a scope of it) in `format`: `stl`, `3mf`, `glb`, `obj`,
    /// `mtl`, `dxf`, `dxf-outline`, `svg-view`, `svg-outline`, `svg-section`, `dxf-section`. Options as JSON (see
    /// [`ExportOptions`]); an empty string means the defaults.
    pub fn export(&self, format: &str, options_json: &str) -> Result<Vec<u8>, String> {
        let opts: ExportOptions = if options_json.trim().is_empty() {
            ExportOptions::default()
        } else {
            serde_json::from_str(options_json).map_err(|e| format!("bad export options: {e}"))?
        };
        let m = self.doc.model();
        let scope = opts
            .scope
            .clone()
            .unwrap_or_else(|| sawhorse_export::Scope::visible(m));
        let e = |e: sawhorse_export::ExportError| e.to_string();
        let outline = || -> Result<Vec<Vec<glam::DVec2>>, String> {
            let t = opts
                .outline
                .as_ref()
                .ok_or("select one face (or sketch region) to export its outline")?;
            self.outline_2d(t)
        };
        Ok(match format {
            "stl" => sawhorse_export::stl::document(m, &scope).map_err(e)?,
            "3mf" => sawhorse_export::three_mf::document(m, &scope).map_err(e)?,
            "glb" => sawhorse_export::gltf::document(m, &scope).map_err(e)?,
            "obj" => {
                let mtl = opts.mtl_file.as_deref().unwrap_or("model.mtl");
                sawhorse_export::obj::document(m, &scope, mtl)
                    .map_err(e)?
                    .into_bytes()
            }
            "mtl" => sawhorse_export::obj::mtl(m).into_bytes(),
            "dxf" => sawhorse_export::dxf::document(m, &scope)
                .map_err(e)?
                .into_bytes(),
            "dxf-outline" => sawhorse_export::dxf::outline(&outline()?, opts.flavour)
                .map_err(e)?
                .into_bytes(),
            "svg-view" => {
                let view = opts.view.as_ref().ok_or("svg-view needs the camera")?;
                sawhorse_export::svg::view(m, &scope, view)
                    .map_err(e)?
                    .into_bytes()
            }
            "svg-outline" => sawhorse_export::svg::outline(&outline()?)
                .map_err(e)?
                .into_bytes(),
            // Lane 12: a section plane's slice, flat, 1:1 in millimetres.
            "svg-section" | "dxf-section" => {
                let s = opts
                    .section
                    .ok_or("choose a section plane to export its slice")?;
                if format == "svg-section" {
                    sawhorse_export::section::svg(m, s).map_err(e)?.into_bytes()
                } else {
                    sawhorse_export::section::dxf(m, s, opts.flavour)
                        .map_err(e)?
                        .into_bytes()
                }
            }
            f => return Err(format!("unknown export format {f}")),
        })
    }
}

/// State hash of the document produced by replaying a JSON op log (golden format).
pub fn replay_hash_core(ops_json: &str) -> Result<String, String> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| e.to_string())?;
    Document::replay(&ops)
        .map(|d| d.hash_hex())
        .map_err(|(i, e)| format!("op {i}: {e}"))
}

fn cut_options(json: &str) -> Result<sawhorse_kernel::cutlist::CutOptions, String> {
    if json.trim().is_empty() {
        return Ok(Default::default());
    }
    serde_json::from_str(json).map_err(|e| format!("bad cut list options: {e}"))
}

/// The cut list of the document produced by replaying a JSON op log, as JSON (the golden
/// cross-check: its `hash` must match the native one).
pub fn replay_cut_list_core(ops_json: &str, options_json: &str) -> Result<String, String> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| e.to_string())?;
    let opts = cut_options(options_json)?;
    let d = Document::replay(&ops).map_err(|(i, e)| format!("op {i}: {e}"))?;
    serde_json::to_string(&d.model().cut_list(&opts)).map_err(|e| e.to_string())
}

fn nest_options(json: &str) -> Result<sawhorse_kernel::nesting::NestOptions, String> {
    if json.trim().is_empty() {
        return Ok(Default::default());
    }
    serde_json::from_str(json).map_err(|e| format!("bad nesting options: {e}"))
}

/// The nesting of the document produced by replaying a JSON op log, as JSON (the golden
/// cross-check: its `hash` must match the native one).
pub fn replay_nesting_core(ops_json: &str, options_json: &str) -> Result<String, String> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| e.to_string())?;
    let opts = nest_options(options_json)?;
    let d = Document::replay(&ops).map_err(|(i, e)| format!("op {i}: {e}"))?;
    serde_json::to_string(&d.model().nest(&opts)).map_err(|e| e.to_string())
}

/// Run one boolean differential case (tests/booleans/cases.json format) through the kernel
/// and return its result plus both operand meshes, as JSON, for scripts/boolean-diff.mjs.
pub fn boolean_case_core(case_json: &str) -> Result<String, String> {
    use sawhorse_kernel::testing::{BoolCase, build_solid, run_boolean_case};
    let c: BoolCase = serde_json::from_str(case_json).map_err(|e| e.to_string())?;
    let result = run_boolean_case(&c)?;
    let mesh = |spec| -> Result<serde_json::Value, String> {
        let o = build_solid(spec)?;
        let t = o.tri_solid().map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "positions": t.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect::<Vec<f64>>(),
            "triangles": t.triangles.iter().flatten().copied().collect::<Vec<u32>>(),
        }))
    };
    Ok(serde_json::json!({ "result": result, "a": mesh(&c.a)?, "b": mesh(&c.b)? }).to_string())
}

fn js(e: String) -> JsError {
    JsError::new(&e)
}

fn id(x: f64) -> Result<u64, JsError> {
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < 9_007_199_254_740_992.0 {
        Ok(x as u64)
    } else {
        Err(JsError::new(&format!("bad id {x}")))
    }
}

/// The persistent kernel document (one per worker).
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct Doc {
    core: DocCore,
}

/// A new document with the ground sketch (sketch id 1).
#[wasm_bindgen]
pub fn document_new() -> Doc {
    Doc::default()
}

/// A document rebuilt by replaying a JSON op log (undo and redo entries included).
#[wasm_bindgen]
pub fn document_replay(ops_json: &str) -> Result<Doc, JsError> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| JsError::new(&e.to_string()))?;
    let doc = Document::replay(&ops).map_err(|(i, e)| JsError::new(&format!("op {i}: {e}")))?;
    Ok(Doc {
        core: DocCore::from_document(doc),
    })
}

/// Replay a JSON op log from empty and return its state hash (golden cross-check).
#[wasm_bindgen]
pub fn replay_hash(ops_json: &str) -> Result<String, JsError> {
    replay_hash_core(ops_json).map_err(js)
}

/// See [`replay_cut_list_core`].
#[wasm_bindgen]
pub fn replay_cut_list(ops_json: &str, options_json: &str) -> Result<String, JsError> {
    replay_cut_list_core(ops_json, options_json).map_err(js)
}

/// See [`replay_nesting_core`].
#[wasm_bindgen]
pub fn replay_nesting(ops_json: &str, options_json: &str) -> Result<String, JsError> {
    replay_nesting_core(ops_json, options_json).map_err(js)
}

/// See [`boolean_case_core`].
#[wasm_bindgen]
pub fn boolean_case(case_json: &str) -> Result<String, JsError> {
    boolean_case_core(case_json).map_err(js)
}

/// The built-in material library, as JSON (see [`DocCore::library_json`]).
#[wasm_bindgen]
pub fn material_library() -> String {
    DocCore::library_json()
}

/// The ops that put a library material into a model, as JSON (see
/// [`DocCore::library_ops_json`]).
#[wasm_bindgen]
pub fn material_library_ops(name: &str, as_name: &str) -> Result<String, JsError> {
    DocCore::library_ops_json(name, as_name).map_err(js)
}

/// A library material's texture PNG.
#[wasm_bindgen]
pub fn material_library_texture(name: &str) -> Result<Vec<u8>, JsError> {
    DocCore::library_texture(name).map_err(js)
}

/// A library material's swatch PNG (96 x 48), for the Materials pane.
#[wasm_bindgen]
pub fn material_library_swatch(name: &str) -> Result<Vec<u8>, JsError> {
    let m = sawhorse_export::library::find(name)
        .ok_or_else(|| JsError::new(&format!("no library material {name:?}")))?;
    sawhorse_export::library::swatch_png(m)
        .ok_or_else(|| JsError::new(&format!("{name} has no texture")))
}

#[wasm_bindgen]
impl Doc {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Doc {
        Doc::default()
    }

    /// Apply one op (JSON, the op-log format). Returns the outcome as JSON; a refusal is a
    /// thrown error with the kernel's message, and changes nothing.
    pub fn apply_op(&mut self, op_json: &str) -> Result<String, JsError> {
        self.core.apply_json(op_json).map_err(js)
    }

    pub fn undo(&mut self) -> Result<String, JsError> {
        self.core.undo().map_err(js)
    }

    /// A plugin's named batch with placeholders, applied as one op (see
    /// [`DocCore::apply_named_batch_json`]).
    pub fn apply_named_batch(&mut self, json: &str) -> Result<String, JsError> {
        self.core.apply_named_batch_json(json).map_err(js)
    }

    /// What is under the cursor: query `{origin, dir, px, point_px?, edge_px?}` as JSON;
    /// returns `{entity, point, surface}` as JSON, or `null`.
    pub fn pick(&self, query_json: &str) -> Result<String, JsError> {
        self.core.pick_json(query_json).map_err(js)
    }

    /// Where the cursor snaps for a drawing tool: query `{origin, dir, px, start?, plane?,
    /// lock?, refs?, anchors?, grid?, grid_step?}` as JSON; returns the inference (kind,
    /// point, tip, entity, surface, lines, edge, picked) as JSON, or `null`.
    pub fn infer(&self, query_json: &str) -> Result<String, JsError> {
        self.core.infer_json(query_json).map_err(js)
    }

    pub fn redo(&mut self) -> Result<String, JsError> {
        self.core.redo().map_err(js)
    }

    /// The loops an offset would draw (see [`DocCore::offset_preview_json`]).
    pub fn offset_preview(&self, query_json: &str) -> Result<String, JsError> {
        self.core.offset_preview_json(query_json).map_err(js)
    }

    /// View mesh of one object.
    pub fn get_view_mesh(&self, object_id: f64) -> Result<Mesh, JsError> {
        self.core
            .object_mesh(id(object_id)?)
            .map(|inner| Mesh { inner })
            .map_err(js)
    }

    /// View mesh of everything: visible objects, then sketch regions and edges.
    pub fn get_scene_mesh(&self) -> Result<Mesh, JsError> {
        self.core
            .scene_mesh()
            .map(|inner| Mesh { inner })
            .map_err(js)
    }

    /// Deterministic state hash (golden-hash format), hex.
    pub fn document_hash(&self) -> String {
        self.core.document().hash_hex()
    }

    /// Deterministic hash of the op log, hex.
    pub fn log_hash(&self) -> String {
        self.core.document().log_hash_hex()
    }

    /// The op log as a JSON array.
    pub fn op_log_json(&self) -> String {
        self.core.log_json()
    }

    /// Stats, undo/redo availability, object ids and tags, as JSON.
    pub fn state_json(&self) -> String {
        self.core.state_json()
    }

    /// The model's tree (groups, component instances and definitions, where every solid
    /// is, the open editing context), as JSON: what the Outliner draws.
    pub fn scene_graph(&self) -> String {
        self.core.scene_graph_json()
    }

    /// World rings (outer first) of a face (`"face"`, object id, face id) or a region
    /// (`"region"`, sketch id, region id), as JSON: what Push/Pull previews.
    pub fn outline(&self, kind: &str, owner: f64, index: f64) -> Result<String, JsError> {
        self.core
            .outline_json(kind, id(owner)?, id(index)?)
            .map_err(js)
    }

    /// The faces of one object with their persistent ids, planes, centroids and areas, as
    /// JSON (see [`DocCore::faces_json`]).
    pub fn faces_json(&self, object_id: f64) -> Result<String, JsError> {
        self.core.faces_json(id(object_id)?).map_err(js)
    }

    /// Binary STL (mm, Z-up) of one object; refuses non-watertight solids.
    pub fn export_stl(&self, object_id: f64) -> Result<Vec<u8>, JsError> {
        self.core.stl(id(object_id)?).map_err(js)
    }

    /// Wavefront OBJ (mm, Z-up) of one object.
    pub fn export_obj(&self, object_id: f64) -> Result<String, JsError> {
        self.core.obj(id(object_id)?).map_err(js)
    }

    /// The document as a `.shz` file (scene.json + geometry.glb + thumbnail.png).
    pub fn save_shz(&self, display_units: &str, thumbnail_png: &[u8]) -> Result<Vec<u8>, JsError> {
        self.core.save_shz(display_units, thumbnail_png).map_err(js)
    }

    /// Replace this document with an opened `.shz` (nothing to undo afterwards). Returns
    /// `{notes, display_units, generator}` as JSON; a refused file changes nothing.
    pub fn open_shz(&mut self, bytes: &[u8]) -> Result<String, JsError> {
        self.core.open_shz(bytes).map_err(js)
    }

    /// The cut list for JSON options `{rounding?, visible_only?}`, as JSON (read-only).
    pub fn cut_list(&self, options_json: &str) -> Result<String, JsError> {
        self.core.cut_list_json(options_json).map_err(js)
    }

    /// The nesting for JSON options `{visible_only?, algorithm?}`, as JSON (read-only).
    pub fn nest(&self, options_json: &str) -> Result<String, JsError> {
        self.core.nest_json(options_json).map_err(js)
    }

    /// The material a face shows (the Paint tool's sample), as JSON: a name or `null`.
    pub fn sample(&self, object_id: f64, face: f64) -> Result<String, JsError> {
        self.core.sample_json(id(object_id)?, id(face)?).map_err(js)
    }

    /// The bytes of a texture image the model holds (copied once into a fresh Uint8Array;
    /// the worker sends each image to the UI once, by digest).
    pub fn texture(&self, path: &str) -> Result<Vec<u8>, JsError> {
        self.core.texture(path).map_err(js)
    }

    /// The texture handles' live preview (see [`DocCore::face_uvs`]).
    pub fn face_uvs(&self, object_id: f64, face: f64, uv_json: &str) -> Result<Vec<f32>, JsError> {
        self.core
            .face_uvs(id(object_id)?, id(face)?, uv_json)
            .map_err(js)
    }

    /// Import a file into the document as one undoable step (see [`DocCore::import`]).
    pub fn import_file(
        &mut self,
        name: &str,
        bytes: &[u8],
        options_json: &str,
    ) -> Result<String, JsError> {
        self.core.import(name, bytes, options_json).map_err(js)
    }

    /// The document in `format` (`stl`, `3mf`, `glb`, `obj`, `mtl`, `dxf`, `dxf-outline`,
    /// `svg-view`, `svg-outline`) with JSON options `{scope?, view?, outline?, flavour?,
    /// mtl_file?}`. A refusal (an open mesh in an STL, no face for an outline) is a thrown
    /// error with a plain message.
    pub fn export_file(&self, format: &str, options_json: &str) -> Result<Vec<u8>, JsError> {
        self.core.export(format, options_json).map_err(js)
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct Mesh {
    inner: ViewMesh,
}

#[wasm_bindgen]
impl Mesh {
    /// xyz per render vertex (metres). Copied into a fresh Float32Array.
    pub fn positions(&self) -> Vec<f32> {
        self.inner.positions.clone()
    }
    pub fn indices(&self) -> Vec<u32> {
        self.inner.indices.clone()
    }
    /// One part index per triangle (see `parts_json`).
    pub fn face_ids(&self) -> Vec<u32> {
        self.inner.face_ids.clone()
    }
    pub fn edges(&self) -> Vec<f32> {
        self.inner.edges.clone()
    }
    /// Texture coordinates, two per render vertex (lane 10).
    pub fn uvs(&self) -> Vec<f32> {
        self.inner.uvs.clone()
    }
    /// The two face normals at each edge, six per edge segment (lane 10: profile edges).
    pub fn edge_normals(&self) -> Vec<f32> {
        self.inner.edge_normals.clone()
    }
    /// Soft (1), smooth (2) and hidden (4) flags, one per edge segment (lane 15).
    pub fn edge_flags(&self) -> Vec<u8> {
        self.inner.edge_flags.clone()
    }
    /// Shading normals, three per render vertex, when some edge in view is smooth; empty
    /// otherwise (lane 15).
    pub fn normals(&self) -> Vec<f32> {
        self.inner.normals.clone()
    }
    /// One per edge segment: index into `edge_owners_json`.
    pub fn edge_owner(&self) -> Vec<u32> {
        self.inner.edge_owner.clone()
    }
    /// `[{kind: "object" | "sketch", id}]`.
    pub fn edge_owners_json(&self) -> String {
        serde_json::to_string(&self.inner.edge_owners).unwrap_or_else(|_| "[]".to_string())
    }
    /// The pickable parts (object faces, sketch regions) with their plane frames, as JSON.
    pub fn parts_json(&self) -> String {
        serde_json::to_string(&self.inner.parts).unwrap_or_else(|_| "[]".to_string())
    }
    pub fn triangle_count(&self) -> usize {
        self.inner.triangle_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stamp scripts/wasm-checkout.mjs recomputes: FNV-1a 64 (the published test
    /// vectors), of this crate's own directory, as 16 hex digits.
    #[test]
    fn the_build_checkout_stamp_is_fnv1a_of_the_crate_dir() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
        let s = build_checkout();
        assert_eq!(s.len(), 16);
        assert_eq!(
            s,
            format!("{:016x}", fnv1a(env!("CARGO_MANIFEST_DIR").as_bytes()))
        );
    }

    fn op(d: &mut DocCore, json: &str) -> serde_json::Value {
        serde_json::from_str(&d.apply_json(json).unwrap()).unwrap()
    }

    #[test]
    fn a_named_batch_is_one_undo_step_and_refusals_change_nothing() {
        let mut d = DocCore::default();
        let out: serde_json::Value = serde_json::from_str(
            &d.apply_named_batch_json(
                r#"{"name":"Box","ops":[
                    {"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6096,"h":0.4064},
                    {"op":"extrude","sketch":1,"region":{"$ref":0,"path":"regions.0"},"distance":0.3048}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(out["kind"], "batch");
        assert_eq!(out["outcomes"][1]["kind"], "object_created");
        assert_eq!(d.document().depth().0, 1);
        let before = (d.document().hash(), d.log_json());
        let err = d
            .apply_named_batch_json(
                r#"{"name":"Bad","ops":[{"op":"erase_face","object":1,"face":999}]}"#,
            )
            .unwrap_err();
        assert!(err.starts_with("op 0 (erase_face)"), "{err}");
        assert_eq!((d.document().hash(), d.log_json()), before);
        assert!(
            d.apply_named_batch_json("not json")
                .unwrap_err()
                .starts_with("bad batch")
        );
        d.undo().unwrap();
        assert_eq!(d.document().model().objects().len(), 0);
    }

    #[test]
    fn document_end_to_end() {
        let mut d = DocCore::default();
        let out = op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6096,"h":0.4064}"#,
        );
        assert_eq!(out["kind"], "sketch_edited");
        assert_eq!(out["regions"][0], 1);
        assert_eq!(d.scene_mesh().unwrap().triangle_count(), 2, "the region");
        let out = op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.3048}"#,
        );
        assert_eq!(out["object"], 1);
        let m = d.scene_mesh().unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(
            m.edges.len(),
            12 * 6,
            "12 box edges; the region left the sketch"
        );
        assert_eq!(m.parts.len(), 6);
        assert_eq!(d.object_mesh(1).unwrap().triangle_count(), 12);
        op(
            &mut d,
            r#"{"op":"imprint","object":1,"face":2,"points":[[0.15,0.1,0.3048],[0.45,0.1,0.3048],[0.45,0.3,0.3048],[0.15,0.3,0.3048]]}"#,
        );
        op(
            &mut d,
            r#"{"op":"push_pull","object":1,"face":7,"distance":0.1}"#,
        );
        let boss = d.document().hash_hex();
        op(
            &mut d,
            r#"{"op":"push_pull","object":1,"face":7,"distance":-0.5}"#,
        );
        let state: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        assert_eq!(state["stats"]["genus"], 1, "pushed through");
        assert_eq!(state["solids"][0]["number"], 1, "display number");
        assert_eq!(
            state["solids"][0]["min"],
            serde_json::json!([0.0, 0.0, 0.0])
        );
        assert_eq!(state["can_undo"], true);
        d.undo().unwrap();
        assert_eq!(d.document().hash_hex(), boss);
        d.undo().unwrap();
        d.redo().unwrap();
        assert_eq!(d.document().hash_hex(), boss);
        assert_eq!(replay_hash_core(&d.log_json()).unwrap(), boss);
        // Faces by persistent id (state: the boss again): the box's six, the boss top (the
        // imprint, still id 7 after its push) and the four boss walls 8..=11.
        let faces: serde_json::Value = serde_json::from_str(&d.faces_json(1).unwrap()).unwrap();
        let ids: Vec<u64> = faces
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_u64().unwrap())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(sorted, (1..=11).collect::<Vec<u64>>());
        let mesh_ids: Vec<u64> = d
            .object_mesh(1)
            .unwrap()
            .parts
            .iter()
            .map(|p| p.index)
            .collect();
        assert_eq!(ids, mesh_ids, "mesh parts name faces by the same ids");
        assert!(d.stl(1).unwrap().len() > 84);
        assert!(d.obj(1).unwrap().starts_with("# Sawhorse OBJ"));
        assert!(d.object_mesh(9).is_err());
        // Picking: straight down onto the middle of the cap, 1 mm per pixel.
        let hit: serde_json::Value = serde_json::from_str(
            &d.pick_json(r#"{"origin":[0.3,0.2,5],"dir":[0,0,-1],"px":{"ortho":true,"scale":0.001,"eye":[0.3,0.2,5],"forward":[0,0,-1]}}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(hit["entity"]["kind"], "face");
        let inf: serde_json::Value = serde_json::from_str(
            &d.infer_json(r#"{"origin":[0.6096,0.4064,5],"dir":[0,0,-1],"px":{"ortho":true,"scale":0.001,"eye":[0.6096,0.4064,5],"forward":[0,0,-1]}}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(inf["kind"], "endpoint");
        assert_eq!(inf["tip"], "Endpoint");
        assert_eq!(hit["entity"]["object"], 1);
        let st: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        assert_eq!(
            st["sketches"][0]["normal"],
            serde_json::json!([0.0, 0.0, 1.0])
        );
        let rings: Vec<Vec<[f64; 3]>> =
            serde_json::from_str(&d.outline_json("face", 1, 7).unwrap()).unwrap();
        assert_eq!(rings.len(), 1, "the boss top");
        assert_eq!(rings[0].len(), 4);
        assert!(d.outline_json("face", 1, 99).is_err());
        let m = d.scene_mesh().unwrap();
        assert_eq!(m.edge_owner.len() * 6, m.edges.len());
        assert_eq!(m.edge_owners[0].kind, "object");
    }

    #[test]
    fn offset_preview_draws_nothing() {
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":1,"h":1}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.5}"#,
        );
        let h = d.document().hash_hex();
        let v: serde_json::Value = serde_json::from_str(
            &d.offset_preview_json(
                r#"{"source":{"of":"face","object":1,"face":2},"distance":-0.25}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(v["closed"], true);
        assert_eq!(v["loops"][0].as_array().unwrap().len(), 4);
        assert_eq!(d.document().hash_hex(), h, "a preview changes nothing");
        let e = d
            .offset_preview_json(r#"{"source":{"of":"face","object":1,"face":2},"distance":-0.6}"#)
            .unwrap_err();
        assert!(e.contains("nothing"), "{e}");
    }

    #[test]
    fn refusals_are_messages() {
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":1,"h":1}"#,
        );
        let before = d.document().hash_hex();
        let e = d
            .apply_json(r#"{"op":"extrude","sketch":1,"region":1,"distance":0}"#)
            .unwrap_err();
        assert!(e.contains("0.01 mm"), "{e}");
        assert!(
            d.apply_json(r#"{"op":"nope"}"#)
                .unwrap_err()
                .starts_with("bad op")
        );
        assert_eq!(d.document().hash_hex(), before);
        let mut fresh = DocCore::default();
        assert_eq!(fresh.undo().unwrap_err(), "nothing to undo");
    }

    #[test]
    fn exports_every_format_through_the_doc() {
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.3,"h":0.2}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.1}"#,
        );
        op(
            &mut d,
            r#"{"op":"define_material","name":"Pine","color":[230,200,150,255]}"#,
        );
        op(
            &mut d,
            r#"{"op":"set_material","object":1,"material":"Pine"}"#,
        );
        op(
            &mut d,
            r#"{"op":"rename_object","object":1,"name":"Board"}"#,
        );
        let stl = d.export("stl", "").unwrap();
        assert_eq!(stl.len(), 84 + 12 * 50);
        assert_eq!(&d.export("3mf", "").unwrap()[..2], b"PK");
        assert_eq!(&d.export("glb", "").unwrap()[..4], b"glTF");
        let obj =
            String::from_utf8(d.export("obj", r#"{"mtl_file":"board.mtl"}"#).unwrap()).unwrap();
        assert!(obj.contains("mtllib board.mtl\no Board\ng Board\nusemtl Pine\n"));
        assert!(
            String::from_utf8(d.export("mtl", "").unwrap())
                .unwrap()
                .contains("newmtl Pine")
        );
        assert!(
            String::from_utf8(d.export("dxf", "").unwrap())
                .unwrap()
                .contains("3DFACE")
        );
        let outline = r#"{"outline":{"kind":"face","owner":1,"index":2}}"#;
        let svg = String::from_utf8(d.export("svg-outline", outline).unwrap()).unwrap();
        assert!(
            svg.contains(r#"width="300.0000mm" height="200.0000mm""#),
            "{svg}"
        );
        let dxf = String::from_utf8(d.export("dxf-outline", outline).unwrap()).unwrap();
        assert!(dxf.contains("LWPOLYLINE"));
        let view = r#"{"view":{"eye":[1,-1,1],"target":[0.15,0.1,0.05],"up":[0,0,1],"ortho":true,"half_height":0.3,"width":400,"height":300}}"#;
        assert!(
            String::from_utf8(d.export("svg-view", view).unwrap())
                .unwrap()
                .starts_with("<?xml")
        );
        // Refusals are plain messages.
        assert!(
            d.export("svg-outline", "")
                .unwrap_err()
                .contains("select one face")
        );
        assert!(d.export("svg-view", "").unwrap_err().contains("camera"));
        assert!(
            d.export("pdf", "")
                .unwrap_err()
                .contains("unknown export format")
        );
        let st: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        assert_eq!(st["object_info"][0]["name"], "Board");
        assert_eq!(
            st["materials"][0]["color"],
            serde_json::json!([230, 200, 150, 255])
        );
    }

    #[test]
    fn imports_files_through_the_doc() {
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.3,"h":0.2}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.1}"#,
        );
        let stl = d.export("stl", "").unwrap();
        let mut fresh = DocCore::default();
        let r: serde_json::Value =
            serde_json::from_str(&fresh.import("part.stl", &stl, "").unwrap()).unwrap();
        assert_eq!(r["outcome"]["kind"], "imported");
        assert_eq!(r["outcome"]["objects"], serde_json::json!([1]));
        assert!(r["notes"][0].as_str().unwrap().contains("millimetres"));
        let st: serde_json::Value = serde_json::from_str(&fresh.state_json()).unwrap();
        assert_eq!(st["object_info"][0]["name"], "part");
        assert!((st["stats"]["volume_m3"].as_f64().unwrap() - 0.006).abs() < 1e-9);
        // In inches, the same bytes come in 25.4 times bigger.
        let mut inch = DocCore::default();
        inch.import("part.stl", &stl, r#"{"units":"in"}"#).unwrap();
        let st: serde_json::Value = serde_json::from_str(&inch.state_json()).unwrap();
        assert!(
            (st["stats"]["volume_m3"].as_f64().unwrap() - 0.006 * 25.4f64.powi(3)).abs() < 1e-6
        );
        assert!(
            fresh
                .import("x.bin", b"nope", "")
                .unwrap_err()
                .contains("not a file")
        );
        fresh.undo().unwrap();
        assert!(
            fresh.document().model().objects().is_empty(),
            "one undo step"
        );
    }

    #[test]
    fn save_and_open_shz() {
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.3,"h":0.2}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.1}"#,
        );
        let h = d.document().hash_hex();
        let bytes = d.save_shz("mm", b"").unwrap();
        let mut fresh = DocCore::default();
        let info: serde_json::Value =
            serde_json::from_str(&fresh.open_shz(&bytes).unwrap()).unwrap();
        assert_eq!(info["display_units"], "mm");
        assert_eq!(fresh.document().hash_hex(), h);
        assert!(fresh.undo().is_err(), "nothing to undo after open");
        let before = fresh.document().hash_hex();
        assert!(
            fresh
                .open_shz(b"junk")
                .unwrap_err()
                .contains("not a Sawhorse file")
        );
        assert_eq!(
            fresh.document().hash_hex(),
            before,
            "a refused open changes nothing"
        );
    }

    #[test]
    fn components_groups_and_the_scene_graph_through_json() {
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"draw","target":{"on":"plane","origin":[0,0,0],"normal":[0,0,1]},"points":[[0,0,0],[0.8509,0,0],[0.8509,0.28575,0],[0,0.28575,0]],"closed":true}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.01905}"#,
        );
        let out = op(
            &mut d,
            r#"{"op":"make_component","items":[{"object":1}],"name":"Shelf"}"#,
        );
        assert_eq!(out["kind"], "component_created");
        assert_eq!(out["instance"], 1);
        let out = op(
            &mut d,
            r#"{"op":"array","objects":[],"nodes":[1],"xform":{"linear":[[1,0,0],[0,1,0],[0,0,1]],"translation":[0,0,0.6]},"count":3}"#,
        );
        assert_eq!(out["kind"], "copied");
        assert_eq!(out["nodes"], serde_json::json!([2, 3, 4]));
        assert!(out.get("objects").is_some());
        // Solids inside a closed component refuse edits, with a message, not an id.
        let e = d
            .apply_json(r#"{"op":"push_pull","object":2,"face":2,"distance":0.006}"#)
            .unwrap_err();
        assert!(e.contains("double-click"), "{e}");
        let out = op(&mut d, r#"{"op":"set_context","node":2}"#);
        assert_eq!(out["kind"], "context_changed");
        assert_eq!(out["context"], 2);
        op(
            &mut d,
            r#"{"op":"push_pull","object":2,"face":2,"distance":0.00635}"#,
        );
        let st: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        let g = &st["graph"];
        assert_eq!(g["context"], 2);
        assert_eq!(g["path"], serde_json::json!([2]));
        assert_eq!(g["nodes"].as_array().unwrap().len(), 4);
        assert_eq!(g["nodes"][0]["kind"], "instance");
        assert_eq!(g["nodes"][0]["label"], "Shelf");
        assert_eq!(g["definitions"][0]["instances"], 4);
        let thick = g["definitions"][0]["max"][2].as_f64().unwrap()
            - g["definitions"][0]["min"][2].as_f64().unwrap();
        assert!((thick - 0.0254).abs() < 1e-12, "every shelf is 1 in now");
        for s in st["solids"].as_array().unwrap() {
            let h = s["max"][2].as_f64().unwrap() - s["min"][2].as_f64().unwrap();
            assert!((h - 0.0254).abs() < 1e-12);
        }
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&d.scene_graph_json()).unwrap(),
            st["graph"]
        );
        // Undo the push: back in the shelf, 3/4 in again; leaving is navigation only.
        let depth = st["undo_depth"].as_u64().unwrap();
        op(&mut d, r#"{"op":"leave_context"}"#);
        let st: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        assert_eq!(st["undo_depth"].as_u64().unwrap(), depth);
        assert_eq!(st["graph"]["context"], serde_json::Value::Null);
        d.undo().unwrap();
        let st: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        assert_eq!(st["graph"]["context"], 2);
        // Tags with colours, scenes with a context.
        op(
            &mut d,
            r##"{"op":"create_tag","name":"Shelves","color":[106,155,90]}"##,
        );
        op(
            &mut d,
            r#"{"op":"tag_items","items":[{"node":1},{"node":2}],"tag":"Shelves"}"#,
        );
        op(
            &mut d,
            r#"{"op":"add_scene","scene":{"name":"Iso","eye":[3,-3,3],"target":[0,0,0],"up":[0,0,1],"fov":35,"parallel":false,"hidden_tags":["Shelves"],"context":null}}"#,
        );
        let out = op(&mut d, r#"{"op":"activate_scene","name":"Iso"}"#);
        assert_eq!(out["kind"], "scene_activated");
        let st: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        assert_eq!(st["graph"]["tag_colors"]["Shelves"], "#6a9b5a");
        assert_eq!(st["graph"]["nodes"][0]["visible"], false);
        assert_eq!(st["tags"]["Shelves"], false);
        assert_eq!(
            d.scene_mesh().unwrap().triangle_count(),
            24,
            "two shelves hidden"
        );
        // Save and open: the same document.
        let h = d.document().hash_hex();
        let bytes = d.save_shz("frac-in", b"").unwrap();
        let mut fresh = DocCore::default();
        fresh.open_shz(&bytes).unwrap();
        assert_eq!(fresh.document().hash_hex(), h);
    }

    #[test]
    fn the_cut_list_and_its_ops_through_json() {
        let mut d = DocCore::default();
        // A shelf (3/4 x 11 x 34 1/2 in), a component, three more by an array.
        op(
            &mut d,
            r#"{"op":"draw","target":{"on":"plane","origin":[0,0,0],"normal":[0,0,1]},"points":[[0,0,0],[0.8763,0,0],[0.8763,0.2794,0],[0,0.2794,0]],"closed":true}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.01905}"#,
        );
        op(
            &mut d,
            r#"{"op":"make_component","items":[{"object":1}],"name":"Shelf"}"#,
        );
        op(
            &mut d,
            r#"{"op":"array","objects":[],"nodes":[1],"xform":{"linear":[[1,0,0],[0,1,0],[0,0,1]],"translation":[0,0,0.4572]},"count":3}"#,
        );
        let l: serde_json::Value = serde_json::from_str(&d.cut_list_json("").unwrap()).unwrap();
        let r = &l["rows"][0];
        assert_eq!(r["name"], "Shelf");
        assert_eq!(r["qty"], 4);
        assert_eq!(r["steps"], serde_json::json!([12, 176, 552]));
        assert_eq!(r["key"], serde_json::json!({ "definition": 1 }));
        assert_eq!(r["target"], serde_json::json!({ "definition": 1 }));
        assert_eq!(r["grain"], "length");
        assert_eq!(r["occurrences"].as_array().unwrap().len(), 4);
        assert_eq!(
            r["occurrences"][0]["item"],
            serde_json::json!({ "node": 1 })
        );
        assert_eq!(
            r["occurrences"][0]["axes"][2],
            serde_json::json!([1.0, 0.0, 0.0])
        );
        // On the nominal 4/4 (1 in rough) a 3/4 in shelf comes from (lane 15).
        assert_eq!(l["totals"]["board_feet"], 4.0 * (379.5 / 144.0));
        // Grain across, stock named, through the op log's JSON.
        let out = op(
            &mut d,
            r#"{"op":"set_grain","part":{"node":2},"grain":"width"}"#,
        );
        assert_eq!(out["kind"], "part_set");
        op(
            &mut d,
            r#"{"op":"set_stock","part":{"definition":1},"stock":"White oak","sheet":null}"#,
        );
        let l: serde_json::Value = serde_json::from_str(
            &d.cut_list_json(r#"{"rounding":"half_mm","visible_only":true}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(l["rounding"], "half_mm");
        assert_eq!(l["rows"][0]["grain"], "width");
        assert_eq!(l["rows"][0]["material"], "White oak");
        assert_eq!(l["rows"][0]["steps"], serde_json::json!([38, 559, 1753]));
        assert!(d.cut_list_json(r#"{"rounding":"inch"}"#).is_err());
        // The replayed log lists the same thing, hash for hash.
        let log = d.log_json();
        let again: serde_json::Value =
            serde_json::from_str(&replay_cut_list_core(&log, "{}").unwrap()).unwrap();
        let here: serde_json::Value =
            serde_json::from_str(&d.cut_list_json("{}").unwrap()).unwrap();
        assert_eq!(again["hash"], here["hash"]);
    }

    #[test]
    fn nesting_and_the_stock_ops_through_json() {
        let mut d = DocCore::default();
        // Four 3/4 x 11 x 34 1/2 in shelves of 3/4 ply (sheet goods by the name).
        op(
            &mut d,
            r#"{"op":"draw","target":{"on":"plane","origin":[0,0,0],"normal":[0,0,1]},"points":[[0,0,0],[0.8763,0,0],[0.8763,0.2794,0],[0,0.2794,0]],"closed":true}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.01905}"#,
        );
        op(
            &mut d,
            r#"{"op":"make_component","items":[{"object":1}],"name":"Shelf"}"#,
        );
        op(
            &mut d,
            r#"{"op":"array","objects":[],"nodes":[1],"xform":{"linear":[[1,0,0],[0,1,0],[0,0,1]],"translation":[0,0,0.4572]},"count":3}"#,
        );
        op(
            &mut d,
            r#"{"op":"set_stock","part":{"definition":1},"stock":"3/4 birch ply","sheet":null}"#,
        );
        let n: serde_json::Value = serde_json::from_str(&d.nest_json("").unwrap()).unwrap();
        assert_eq!(n["algorithm"], "guillotine");
        assert_eq!(n["totals"]["sheets"], 1);
        assert_eq!(n["totals"]["placed"], 4);
        let p = &n["pieces"][0];
        assert_eq!(p["stock"], "4 x 8 sheet, 3/4");
        assert_eq!(p["kind"], "sheet");
        assert_eq!(p["placements"].as_array().unwrap().len(), 4);
        assert_eq!(p["placements"][0]["tag"], "1a");
        assert_eq!(p["placements"][0]["item"], serde_json::json!({ "node": 1 }));
        assert!(p["cuts"][0]["dir"] == "rip" || p["cuts"][0]["dir"] == "crosscut");
        assert_eq!(n["library"]["stocks"].as_array().unwrap().len(), 14);
        // The stock ops through the op log's JSON.
        let out = op(
            &mut d,
            r#"{"op":"set_nest_settings","settings":{"kerf":0.0023812,"trim":0,"algorithm":"maxrects"}}"#,
        );
        assert_eq!(out["kind"], "stock_set");
        op(
            &mut d,
            r#"{"op":"define_stock","stock":{"name":"Shop 3/4","kind":"sheet","length":1.2,"width":0.6,"thickness":0.01905,"grain":"none"}}"#,
        );
        op(
            &mut d,
            r#"{"op":"set_material_stock","material":"3/4 birch ply","stock":"shop 3/4"}"#,
        );
        let n: serde_json::Value =
            serde_json::from_str(&d.nest_json(r#"{"visible_only":true}"#).unwrap()).unwrap();
        assert_eq!(n["algorithm"], "maxrects");
        assert_eq!(n["pieces"][0]["stock"], "Shop 3/4");
        assert!(d.nest_json(r#"{"algorithm":"zigzag"}"#).is_err());
        assert!(
            d.apply_json(r#"{"op":"delete_stock","name":"nothing"}"#)
                .is_err()
        );
        // Replayed and reopened: the same layout.
        let log = d.log_json();
        let again: serde_json::Value =
            serde_json::from_str(&replay_nesting_core(&log, "{}").unwrap()).unwrap();
        let here: serde_json::Value = serde_json::from_str(&d.nest_json("{}").unwrap()).unwrap();
        assert_eq!(again["hash"], here["hash"]);
        let bytes = d.save_shz("frac-in", b"").unwrap();
        let mut fresh = DocCore::default();
        fresh.open_shz(&bytes).unwrap();
        let opened: serde_json::Value =
            serde_json::from_str(&fresh.nest_json("{}").unwrap()).unwrap();
        assert_eq!(opened["hash"], here["hash"]);
        op(&mut d, r#"{"op":"reset_stock_library"}"#);
        let n: serde_json::Value = serde_json::from_str(&d.nest_json("").unwrap()).unwrap();
        assert_eq!(n["algorithm"], "guillotine");
    }

    #[test]
    fn materials_through_json() {
        let lib: serde_json::Value = serde_json::from_str(&DocCore::library_json()).unwrap();
        let woods = lib
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["category"] == "Wood")
            .count();
        assert_eq!(woods, 12);
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6,"h":0.15}"#,
        );
        op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.02}"#,
        );
        // The library material and the paint as one batch: one undo step.
        let mut ops: Vec<serde_json::Value> =
            serde_json::from_str(&DocCore::library_ops_json("White oak", "").unwrap()).unwrap();
        assert_eq!(ops[0]["op"], "add_texture");
        ops.push(serde_json::json!({"op":"paint","target":{"on":"items","items":[{"object":1}]},"material":"White oak"}));
        let batch = serde_json::json!({"op":"batch","ops":ops}).to_string();
        op(&mut d, &batch);
        assert_eq!(d.sample_json(1, 2).unwrap(), "\"White oak\"");
        let st: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        let m = &st["materials"][0];
        assert_eq!(m["name"], "White oak");
        assert_eq!(m["held"], true);
        assert_eq!(m["stock"]["kind"], "lumber");
        assert_eq!(m["used"], 6);
        let t = &st["textures"][0];
        assert_eq!(t["path"], "assets/lib-white-oak.png");
        assert_eq!(t["width"], 512);
        let png = d.texture("assets/lib-white-oak.png").unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // The view mesh carries coordinates and edge normals; the preview matches the op.
        let mesh = d.scene_mesh().unwrap();
        assert_eq!(mesh.uvs.len(), mesh.positions.len() / 3 * 2);
        assert_eq!(mesh.edge_normals.len(), mesh.edges.len());
        let uv = r#"{"rotation":30,"scale":[2,2]}"#;
        let preview = d.face_uvs(1, 2, uv).unwrap();
        assert!(!preview.is_empty());
        op(
            &mut d,
            &format!(r#"{{"op":"set_face_uv","object":1,"face":2,"uv":{uv}}}"#),
        );
        let mesh = d.scene_mesh().unwrap();
        let part = mesh
            .parts
            .iter()
            .position(|p| p.owner == 1 && p.index == 2)
            .unwrap();
        let first = mesh
            .indices
            .chunks(3)
            .zip(&mesh.face_ids)
            .filter(|(_, f)| **f as usize == part)
            .flat_map(|(t, _)| t.iter().copied())
            .min()
            .unwrap() as usize;
        assert_eq!(
            &mesh.uvs[2 * first..2 * first + preview.len()],
            &preview[..]
        );
        // One undo takes the positioning off; another the whole library batch.
        d.undo().unwrap();
        d.undo().unwrap();
        assert_eq!(d.sample_json(1, 2).unwrap(), "null");
        assert!(d.sample_json(1, 99).is_err());
    }

    #[test]
    fn replay_hash_matches_golden() {
        let text = include_str!("../../kernel/tests/golden/document_undo_redo.json");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let h = replay_hash_core(&v["ops"].to_string()).unwrap();
        assert_eq!(h, v["hash"].as_str().unwrap());
    }
}
