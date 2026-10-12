//! glTF 2.0 binary (GLB). Metres, Y up (glTF's convention): a kernel point (x, y, z) Z-up
//! is written as (x, z, -y). One node and one mesh per solid or imported mesh, named after
//! the object; flat-shaded (each face has its own vertices, so normals are exact); the
//! persistent face id of every vertex in a custom attribute `_FACE_ID` (FLOAT, exact for ids
//! below 2^24; glTF only allows integer types for indices); materials as metallic-roughness
//! base colours. Node `extras.sawhorse` carries the object id, tag, group names, whether
//! it is a solid, and for imported meshes the note saying why not.

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

/// Flat-shaded triangles of one thing: per-vertex positions (Y up), normals and face ids.
#[derive(Default)]
struct Prim {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    face_ids: Vec<f32>,
    indices: Vec<u32>,
}

impl Prim {
    /// Add one planar face (outer ring + holes, world points) with its normal and id.
    fn face(
        &mut self,
        rings: &[Vec<DVec3>],
        normal: DVec3,
        plane_local: impl Fn(DVec3) -> DVec2,
        id: u64,
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

/// Everything in `scope` as one GLB. Imported meshes are included (glTF is a viewing
/// format): their node extras say they are not solids, and why.
pub fn document(model: &Model, scope: &Scope) -> Result<Vec<u8>, ExportError> {
    let solids = scope.solids(model)?;
    let loose = scope.loose(model)?;
    if solids.is_empty() && loose.is_empty() {
        return Err(ExportError::Empty);
    }
    let mut b = Builder::default();
    let mut materials: Vec<Value> = Vec::new();
    let mut material_index: Vec<String> = Vec::new();
    let mut material_for = |name: Option<&str>| -> Option<usize> {
        let name = name?;
        let def = model.materials().get(name)?;
        if let Some(i) = material_index.iter().position(|n| n == name) {
            return Some(i);
        }
        let c = def.color;
        let mut m = json!({
            "name": name,
            "pbrMetallicRoughness": {
                "baseColorFactor": [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), f64::from(c[3]) / 255.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.8
            },
            "doubleSided": false
        });
        if c[3] < 255 {
            m["alphaMode"] = json!("BLEND");
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
        |b: &mut Builder, prim: Prim, name: String, material: Option<usize>, extras: Value| {
            if prim.indices.is_empty() {
                return;
            }
            let pos = b.vec3(&prim.positions, true);
            let nor = b.vec3(&prim.normals, false);
            let fid = b.scalar_f32(&prim.face_ids);
            let idx = b.indices(&prim.indices);
            let mut p = json!({
                "attributes": { "POSITION": pos, "NORMAL": nor, "_FACE_ID": fid },
                "indices": idx,
                "mode": 4
            });
            if let Some(m) = material {
                p["material"] = json!(m);
            }
            meshes.push(json!({ "name": name, "primitives": [p] }));
            nodes.push(
                json!({ "name": name, "mesh": meshes.len() - 1, "extras": { "sawhorse": extras } }),
            );
        };
    for (id, o) in &solids {
        let mut prim = Prim::default();
        let br = o.brep();
        for (_, f) in br.faces() {
            let rings: Vec<Vec<DVec3>> = f.loops().map(|l| br.loop_points(l)).collect();
            let plane = f.plane;
            prim.face(&rings, plane.normal, |p| plane.to_local(p), f.id)?;
        }
        let extras = json!({
            "object": id, "solid": true, "tag": o.tag(), "groups": groups_of(*id)
        });
        push(
            &mut b,
            prim,
            object_name(model, *id),
            material_for(o.material()),
            extras,
        );
    }
    for (id, m) in &loose {
        let mut prim = Prim::default();
        for (fi, f) in m.faces.iter().enumerate() {
            let Some(plane) = m.plane(fi) else {
                continue;
            };
            let rings: Vec<Vec<DVec3>> = f
                .iter()
                .map(|r| r.iter().map(|&i| m.positions[i as usize]).collect())
                .collect();
            // A face that will not triangulate is left out of the picture, as in the view.
            let _ = prim.face(&rings, plane.normal, |p| plane.to_local(p), fi as u64);
        }
        let extras = json!({ "mesh": id, "solid": false, "note": m.note, "tag": m.tag });
        push(
            &mut b,
            prim,
            m.name.clone(),
            material_for(m.material.as_deref()),
            extras,
        );
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
    fn empty_scope_is_refused() {
        let m = fixtures::box_model();
        assert_eq!(document(&m, &Scope::default()), Err(ExportError::Empty));
    }
}
