//! glTF 2.0 binary (GLB). Metres, Y up (glTF's convention): a kernel point (x, y, z) Z-up
//! is written as (x, z, -y). One node and one mesh per solid or imported mesh, named after
//! the object; flat-shaded (each face has its own vertices, so normals are exact); the
//! persistent face id of every vertex in a custom attribute `_FACE_ID` (FLOAT, exact for ids
//! below 2^24; glTF only allows integer types for indices); materials as metallic-roughness
//! base colours. Node `extras.sawhorse` carries the object id, tag, group names, whether
//! it is a solid, and for imported meshes the note saying why not.
//!
//! Lane 10: a solid whose faces show different materials (face paint, or paint on the groups
//! and components around it) gets one primitive per material; a material whose texture the
//! model holds is written with its image embedded (`images`, `textures`, a repeating
//! sampler) and `TEXCOORD_0` on its primitives, laid out exactly as the view draws it.
//! The `geometry.glb` inside a `.shz` leaves the images out (they are in `assets/`).

use glam::{DVec2, DVec3};
use sawhorse_kernel::Model;
use sawhorse_tessellate::triangulate;
use serde_json::{Value, json};

use crate::{ExportError, Scope, object_name, srgb_to_linear};

/// The glTF accessor component types and buffer view targets we use.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Kernel Z-up metres to glTF Y-up metres.
pub fn to_y_up(p: DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, (-p.y) as f32]
}

/// Flat-shaded triangles of one thing: per-vertex positions (Y up), normals, face ids and
/// (textured) texture coordinates.
#[derive(Default)]
struct Prim {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    face_ids: Vec<f32>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// What to write beyond geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlbOptions {
    /// Embed the texture images materials use (and texture coordinates).
    pub textures: bool,
}

impl Prim {
    /// Add one planar face (outer ring + holes, world points) with its normal and id.
    fn face(
        &mut self,
        rings: &[Vec<DVec3>],
        normal: DVec3,
        plane_local: impl Fn(DVec3) -> DVec2,
        id: u64,
        uv: Option<&dyn Fn(DVec3) -> [f64; 2]>,
    ) -> Result<(), ExportError> {
        let outer: Vec<DVec2> = rings[0].iter().map(|&p| plane_local(p)).collect();
        let holes: Vec<Vec<DVec2>> = rings[1..]
            .iter()
            .map(|r| r.iter().map(|&p| plane_local(p)).collect())
            .collect();
        let tris =
            triangulate(&outer, &holes).map_err(|e| ExportError::Tessellate(e.to_string()))?;
        let base = self.positions.len() as u32;
        let n = to_y_up(normal);
        for p in rings.iter().flatten() {
            self.positions.push(to_y_up(*p));
            self.normals.push(n);
            self.face_ids.push(id as f32);
            if let Some(f) = uv {
                let [u, v] = f(*p);
                self.uvs.push([u as f32, v as f32]);
            }
        }
        for t in tris {
            self.indices.extend(t.map(|k| base + k));
        }
        Ok(())
    }
}

#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn view(&mut self, bytes: &[u8], target: u32) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.views.push(json!({
            "buffer": 0, "byteOffset": offset, "byteLength": bytes.len(), "target": target
        }));
        self.views.len() - 1
    }

    fn vec3(&mut self, data: &[[f32; 3]], minmax: bool) -> usize {
        let bytes: Vec<u8> = data
            .iter()
            .flatten()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        let view = self.view(&bytes, ARRAY_BUFFER);
        let mut a = json!({
            "bufferView": view, "componentType": FLOAT, "count": data.len(), "type": "VEC3"
        });
        if minmax {
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for p in data {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn vec2(&mut self, data: &[[f32; 2]]) -> usize {
        let bytes: Vec<u8> = data
            .iter()
            .flatten()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        let view = self.view(&bytes, ARRAY_BUFFER);
        self.accessors.push(json!({
            "bufferView": view, "componentType": FLOAT, "count": data.len(), "type": "VEC2"
        }));
        self.accessors.len() - 1
    }

    /// An image's bytes as a buffer view (no target).
    fn image(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.views
            .push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() }));
        self.views.len() - 1
    }

    fn scalar_f32(&mut self, data: &[f32]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|x| x.to_le_bytes()).collect();
        let view = self.view(&bytes, ARRAY_BUFFER);
        self.accessors.push(json!({
            "bufferView": view, "componentType": FLOAT, "count": data.len(), "type": "SCALAR"
        }));
        self.accessors.len() - 1
    }

    fn indices(&mut self, data: &[u32]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|x| x.to_le_bytes()).collect();
        let view = self.view(&bytes, ELEMENT_ARRAY_BUFFER);
        self.accessors.push(json!({
            "bufferView": view, "componentType": UNSIGNED_INT, "count": data.len(), "type": "SCALAR"
        }));
        self.accessors.len() - 1
    }
}

/// Assemble a GLB container from the JSON document and the binary buffer.
pub fn glb(mut doc: Value, bin: &[u8]) -> Vec<u8> {
    if !bin.is_empty() {
        doc["buffers"] = json!([{ "byteLength": bin.len() }]);
    }
    let mut json_bytes = serde_json::to_vec(&doc).unwrap_or_default();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

/// Everything in `scope` as one GLB, texture images embedded. Imported meshes are included
/// (glTF is a viewing format): their node extras say they are not solids, and why.
pub fn document(model: &Model, scope: &Scope) -> Result<Vec<u8>, ExportError> {
    document_with(model, scope, GlbOptions { textures: true })
}

/// [`document`] with options (a `.shz` leaves the images out of its `geometry.glb`).
pub fn document_with(
    model: &Model,
    scope: &Scope,
    opts: GlbOptions,
) -> Result<Vec<u8>, ExportError> {
    let solids = scope.solids(model)?;
    let loose = scope.loose(model)?;
    if solids.is_empty() && loose.is_empty() {
        return Err(ExportError::Empty);
    }
    let mut b = Builder::default();
    let mut materials: Vec<Value> = Vec::new();
    let mut material_index: Vec<String> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    let mut textures: Vec<Value> = Vec::new();
    let mut image_index: Vec<String> = Vec::new();
    let mut material_for = |b: &mut Builder, name: Option<&str>| -> Option<usize> {
        let name = name?;
        let def = model.materials().get(name)?;
        if let Some(i) = material_index.iter().position(|n| n == name) {
            return Some(i);
        }
        let c = def.color;
        let image = def
            .texture
            .as_deref()
            .filter(|_| opts.textures)
            .and_then(|t| model.textures().get(t).map(|img| (t, img)));
        let factor = if image.is_some() {
            json!([1.0, 1.0, 1.0, f64::from(c[3]) / 255.0])
        } else {
            json!([
                srgb_to_linear(c[0]),
                srgb_to_linear(c[1]),
                srgb_to_linear(c[2]),
                f64::from(c[3]) / 255.0
            ])
        };
        let mut m = json!({
            "name": name,
            "pbrMetallicRoughness": {
                "baseColorFactor": factor,
                "metallicFactor": 0.0,
                "roughnessFactor": 0.8
            },
            "doubleSided": false
        });
        if c[3] < 255 {
            m["alphaMode"] = json!("BLEND");
        }
        if let Some((path, img)) = image {
            let source = match image_index.iter().position(|p| p == path) {
                Some(i) => i,
                None => {
                    let view = b.image(&img.data);
                    images.push(json!({ "bufferView": view, "mimeType": img.mime, "name": path }));
                    textures.push(json!({ "sampler": 0, "source": images.len() - 1 }));
                    image_index.push(path.to_string());
                    image_index.len() - 1
                }
            };
            m["pbrMetallicRoughness"]["baseColorTexture"] = json!({ "index": source });
        }
        if let Some(t) = &def.texture {
            m["extras"] = json!({ "sawhorse": { "texture": t } });
        }
        materials.push(m);
        material_index.push(name.to_string());
        Some(materials.len() - 1)
    };
    // The groups and components around a solid, outermost first.
    let graph = model.graph();
    let groups_of = |id: u64| -> Vec<String> {
        let mut at = graph
            .objects
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.parent);
        let mut names = Vec::new();
        while let Some(p) = at {
            let Some(n) = graph.nodes.iter().find(|n| n.id == p) else {
                break;
            };
            names.push(n.label.clone());
            at = n.parent;
        }
        names.reverse();
        names
    };
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut push =
        |b: &mut Builder, prims: Vec<(Prim, Option<usize>)>, name: String, extras: Value| {
            let mut out = Vec::new();
            for (prim, material) in prims {
                if prim.indices.is_empty() {
                    continue;
                }
                let pos = b.vec3(&prim.positions, true);
                let nor = b.vec3(&prim.normals, false);
                let fid = b.scalar_f32(&prim.face_ids);
                let mut attrs = json!({ "POSITION": pos, "NORMAL": nor, "_FACE_ID": fid });
                if !prim.uvs.is_empty() && prim.uvs.len() == prim.positions.len() {
                    attrs["TEXCOORD_0"] = json!(b.vec2(&prim.uvs));
                }
                let idx = b.indices(&prim.indices);
                let mut p = json!({ "attributes": attrs, "indices": idx, "mode": 4 });
                if let Some(m) = material {
                    p["material"] = json!(m);
                }
                out.push(p);
            }
            if out.is_empty() {
                return;
            }
            meshes.push(json!({ "name": name, "primitives": out }));
            nodes.push(
                json!({ "name": name, "mesh": meshes.len() - 1, "extras": { "sawhorse": extras } }),
            );
        };
    for (id, o) in &solids {
        // One primitive per material the faces show, in order of first face.
        let mut by_material: Vec<(Option<String>, Prim)> = Vec::new();
        let br = o.brep();
        // Lane 15: across smooth edges the faces share normals (a softened cylinder reads
        // round in a viewer, as in the app).
        let smooth = o.smooth_normals();
        for (fid, f) in br.faces() {
            let rings: Vec<Vec<DVec3>> = f.loops().map(|l| br.loop_points(l)).collect();
            let plane = f.plane;
            let mat = model.face_material(*id, f.id).map(str::to_string);
            let k = match by_material.iter().position(|(m, _)| *m == mat) {
                Some(k) => k,
                None => {
                    by_material.push((mat, Prim::default()));
                    by_material.len() - 1
                }
            };
            let tex = if opts.textures {
                model.face_texture(*id, f.id)
            } else {
                None
            };
            let uv = tex.as_ref().map(|t| move |p: DVec3| t.uv(p));
            let start = by_material[k].1.normals.len();
            by_material[k].1.face(
                &rings,
                plane.normal,
                |p| plane.to_local(p),
                f.id,
                uv.as_ref().map(|f| f as &dyn Fn(DVec3) -> [f64; 2]),
            )?;
            if let Some(map) = &smooth {
                let verts = f.loops().flat_map(|l| br.loop_vertices(l));
                for (slot, v) in by_material[k].1.normals[start..].iter_mut().zip(verts) {
                    if let Some(n) = map.get(&(fid, v)) {
                        *slot = to_y_up(*n);
                    }
                }
            }
        }
        let extras = json!({
            "object": id, "solid": true, "tag": o.tag(), "groups": groups_of(*id)
        });
        let prims = by_material
            .into_iter()
            .map(|(m, p)| {
                let mi = material_for(&mut b, m.as_deref());
                (p, mi)
            })
            .collect();
        push(&mut b, prims, object_name(model, *id), extras);
    }
    for (id, m) in &loose {
        let mut prim = Prim::default();
        let smooth = m.smooth_normals();
        for (fi, f) in m.faces.iter().enumerate() {
            let Some(plane) = m.plane(fi) else {
                continue;
            };
            let rings: Vec<Vec<DVec3>> = f
                .iter()
                .map(|r| r.iter().map(|&i| m.positions[i as usize]).collect())
                .collect();
            // A face that will not triangulate is left out of the picture, as in the view.
            let start = prim.normals.len();
            if prim
                .face(&rings, plane.normal, |p| plane.to_local(p), fi as u64, None)
                .is_ok()
                && let Some(map) = &smooth
            {
                for (slot, &v) in prim.normals[start..].iter_mut().zip(f.iter().flatten()) {
                    if let Some(n) = map.get(&(fi, v)) {
                        *slot = to_y_up(*n);
                    }
                }
            }
        }
        let extras = json!({ "mesh": id, "solid": false, "note": m.note, "tag": m.tag });
        let mi = material_for(&mut b, m.material.as_deref());
        push(&mut b, vec![(prim, mi)], m.name.clone(), extras);
    }
    if nodes.is_empty() {
        return Err(ExportError::Empty);
    }
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "Sawhorse" },
        "extras": { "sawhorse": { "units": "metre", "up": "Y" } },
        "scene": 0,
        "scenes": [{ "name": "Sawhorse", "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": b.accessors,
        "bufferViews": b.views,
    });
    if !materials.is_empty() {
        doc["materials"] = json!(materials);
    }
    if !images.is_empty() {
        doc["images"] = json!(images);
        doc["textures"] = json!(textures);
        // Repeat both ways, linear with mipmaps (as the view draws them).
        doc["samplers"] =
            json!([{ "wrapS": 10497, "wrapT": 10497, "magFilter": 9729, "minFilter": 9987 }]);
    }
    Ok(glb(doc, &b.bin))
}

/// Split a GLB into its JSON document and binary chunk (tests and readers).
pub fn parse_glb(bytes: &[u8]) -> Option<(Value, &[u8])> {
    if bytes.len() < 20 || &bytes[..4] != b"glTF" {
        return None;
    }
    let u = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
    if u(4) != 2 || u(8) != bytes.len() {
        return None;
    }
    let jl = u(12);
    if &bytes[16..20] != b"JSON" || 20 + jl > bytes.len() {
        return None;
    }
    let doc: Value = serde_json::from_slice(&bytes[20..20 + jl]).ok()?;
    let rest = 20 + jl;
    let bin: &[u8] = if rest + 8 <= bytes.len() && &bytes[rest + 4..rest + 8] == b"BIN\0" {
        let bl = u(rest);
        bytes.get(rest + 8..rest + 8 + bl)?
    } else {
        &[]
    };
    Some((doc, bin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn floats(doc: &Value, bin: &[u8], acc: usize) -> Vec<f32> {
        let a = &doc["accessors"][acc];
        let v = &doc["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let off = v["byteOffset"].as_u64().unwrap() as usize;
        let len = v["byteLength"].as_u64().unwrap() as usize;
        bin[off..off + len]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    }

    #[test]
    fn glb_layout_nodes_y_up_and_face_ids() {
        let m = fixtures::mixed_model();
        let bytes = document(&m, &Scope::visible(&m)).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(bytes.len() % 4, 0);
        let (doc, bin) = parse_glb(&bytes).unwrap();
        assert_eq!(doc["asset"]["version"], "2.0");
        let names: Vec<&str> = doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Side panel", "Block", "Sheet"]);
        assert_eq!(doc["nodes"][2]["extras"]["sawhorse"]["solid"], false);
        assert_eq!(doc["materials"][0]["name"], "Oak");
        // Box 0.1 x 0.2 x 0.05 m, Z up: in glTF Y is up (0..0.05) and Z is -y (-0.2..0).
        let prim = &doc["meshes"][0]["primitives"][0];
        let pos = prim["attributes"]["POSITION"].as_u64().unwrap() as usize;
        assert_eq!(doc["accessors"][pos]["max"], json!([0.1f32, 0.05f32, 0.0]));
        assert_eq!(doc["accessors"][pos]["min"], json!([0.0, 0.0, -0.2f32]));
        let fid = prim["attributes"]["_FACE_ID"].as_u64().unwrap() as usize;
        let mut ids: Vec<u32> = floats(&doc, bin, fid).iter().map(|&f| f as u32).collect();
        ids.dedup();
        assert_eq!(
            ids,
            [1, 2, 3, 4, 5, 6],
            "four vertices per face, faces in order"
        );
        // Normals are unit and point along an axis.
        let nor = prim["attributes"]["NORMAL"].as_u64().unwrap() as usize;
        for n in floats(&doc, bin, nor).chunks(3) {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-6);
        }
        assert_eq!(
            doc["accessors"][prim["indices"].as_u64().unwrap() as usize]["count"],
            36
        );
    }

    #[test]
    fn painted_faces_get_primitives_and_textures_are_embedded() {
        let mut m = fixtures::box_model();
        for op in crate::library::ops(crate::library::find("Walnut").unwrap(), None) {
            m.apply(&op).unwrap();
        }
        m.apply(&sawhorse_kernel::Op::Paint {
            target: sawhorse_kernel::looks::PaintTarget::Faces {
                object: 1,
                faces: vec![2],
            },
            material: Some("Walnut".into()),
        })
        .unwrap();
        let (doc, bin) = {
            let bytes = document(&m, &Scope::visible(&m)).unwrap();
            let (d, b) = parse_glb(&bytes).unwrap();
            (d, b.to_vec())
        };
        let prims = doc["meshes"][0]["primitives"].as_array().unwrap();
        assert_eq!(prims.len(), 2, "the unpainted faces, then the walnut top");
        assert!(prims[0].get("material").is_none());
        assert_eq!(doc["materials"][0]["name"], "Walnut");
        let tex = prims[1]["attributes"]["TEXCOORD_0"].as_u64().unwrap() as usize;
        assert_eq!(doc["accessors"][tex]["type"], "VEC2");
        assert_eq!(doc["accessors"][tex]["count"], 4);
        let uv = floats(&doc, &bin, tex);
        assert!(uv.iter().all(|x| x.is_finite()));
        assert_eq!(
            doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"],
            0
        );
        let img = &doc["images"][0];
        assert_eq!(img["mimeType"], "image/png");
        let view = &doc["bufferViews"][img["bufferView"].as_u64().unwrap() as usize];
        let off = view["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(&bin[off + 1..off + 4], b"PNG");
        assert_eq!(doc["samplers"][0]["wrapS"], 10497);
        // The .shz copy keeps the image out.
        let bytes = document_with(&m, &Scope::visible(&m), GlbOptions { textures: false }).unwrap();
        let (d2, _) = parse_glb(&bytes).unwrap();
        assert!(d2.get("images").is_none());
        assert!(
            d2["meshes"][0]["primitives"][1]["attributes"]
                .get("TEXCOORD_0")
                .is_none()
        );
    }

    #[test]
    fn empty_scope_is_refused() {
        let m = fixtures::box_model();
        assert_eq!(document(&m, &Scope::default()), Err(ExportError::Empty));
    }
}
