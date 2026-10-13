//! glTF 2.0, as `.glb` or as `.gltf` JSON with embedded (`data:`) buffers. Metres, Y up:
//! a glTF point (x, y, z) becomes the kernel's (x, -z, y). The default scene's node tree is
//! walked with every node's transform (matrix or translation / rotation / scale); each node
//! with a mesh becomes one item named after the node (or its mesh). Triangle lists, strips
//! and fans are read; line primitives become loose lines; points are skipped (noted).
//! Materials become colours (base colour factor, linear to sRGB), with the base-colour
//! texture's file name kept as a reference. A `_FACE_ID` attribute (Sawhorse writes one)
//! keeps faces apart that are coplanar but were separate faces, so drawn edges survive the
//! round trip. External `.bin` / image files cannot be reached from a single-file import
//! and are reported.

use glam::{DMat4, DQuat, DVec3};
use serde_json::Value;

use crate::util::{base64_decode, linear_to_srgb, plural};
use crate::{Budget, ImportError, ImportOptions, Parsed, malformed};

/// Values one accessor without a buffer view may stand for (all zeros, glTF's default).
const MAX_EMPTY_ACCESSOR: usize = 4_000_000;
use sawhorse_kernel::meshbuild::ImportItem;
use sawhorse_kernel::model::MaterialDef;

fn err(m: impl Into<String>) -> ImportError {
    malformed("glTF", m)
}

/// Split a GLB into its JSON document and binary chunk.
fn split_glb(bytes: &[u8]) -> Result<(Value, Option<&[u8]>), ImportError> {
    let u = |o: usize| -> Result<usize, ImportError> {
        Ok(u32::from_le_bytes(
            bytes
                .get(o..o + 4)
                .ok_or_else(|| err("truncated GLB"))?
                .try_into()
                .map_err(|_| err("truncated GLB"))?,
        ) as usize)
    };
    if u(4)? != 2 {
        return Err(err("only glTF 2.0 is supported"));
    }
    let jl = u(12)?;
    if bytes.get(16..20) != Some(b"JSON") {
        return Err(err("the first GLB chunk is not JSON"));
    }
    let json = bytes.get(20..20 + jl).ok_or_else(|| err("truncated GLB"))?;
    let doc: Value = serde_json::from_slice(json).map_err(|e| err(e.to_string()))?;
    let rest = 20 + jl;
    let bin = if bytes.get(rest + 4..rest + 8) == Some(b"BIN\0") {
        let bl = u(rest)?;
        Some(
            bytes
                .get(rest + 8..rest + 8 + bl)
                .ok_or_else(|| err("truncated GLB"))?,
        )
    } else {
        None
    };
    Ok((doc, bin))
}

struct Gltf<'a> {
    doc: Value,
    buffers: Vec<Option<std::borrow::Cow<'a, [u8]>>>,
}

impl Gltf<'_> {
    fn arr(&self, key: &str) -> &[Value] {
        self.doc[key].as_array().map(Vec::as_slice).unwrap_or(&[])
    }

    /// An accessor as f64 values (normalised integers scaled), and its component count.
    fn accessor(&self, i: usize) -> Result<(Vec<f64>, usize), ImportError> {
        let a = self
            .arr("accessors")
            .get(i)
            .ok_or_else(|| err(format!("no accessor {i}")))?;
        if a.get("sparse").is_some() {
            return Err(err("sparse accessors are not supported"));
        }
        let comps = match a["type"].as_str().unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            t => return Err(err(format!("accessor type {t:?} is not supported"))),
        };
        let ct = a["componentType"].as_u64().unwrap_or(0);
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(err(format!("component type {ct} is not supported"))),
        };
        // Lane 16: the count is checked before anything is allocated for it, in u64 (a
        // count over 2^32 must not wrap on wasm32).
        let count64 = a["count"].as_u64().unwrap_or(0);
        let normalized = a["normalized"].as_bool().unwrap_or(false);
        let Some(vi) = a["bufferView"].as_u64() else {
            let n = count64.saturating_mul(comps as u64);
            if n > MAX_EMPTY_ACCESSOR as u64 {
                return Err(err(format!(
                    "accessor {i} stands for {n} values without data (at most {MAX_EMPTY_ACCESSOR})"
                )));
            }
            return Ok((vec![0.0; n as usize], comps));
        };
        let v = self
            .arr("bufferViews")
            .get(vi as usize)
            .ok_or_else(|| err("no buffer view"))?;
        let b = v["buffer"].as_u64().unwrap_or(0) as usize;
        let data = self
            .buffers
            .get(b)
            .and_then(|x| x.as_deref())
            .ok_or_else(|| err(format!("buffer {b} is an external file a single-file import cannot read; use .glb or embedded buffers")))?;
        let elem = (size * comps) as u64;
        let start = v["byteOffset"]
            .as_u64()
            .unwrap_or(0)
            .checked_add(a["byteOffset"].as_u64().unwrap_or(0))
            .ok_or_else(|| err("accessor offset overflows"))?;
        let stride = v["byteStride"].as_u64().unwrap_or(elem);
        if stride < elem || stride > 252 {
            return Err(err(format!(
                "accessor {i}: stride {stride} does not fit its elements"
            )));
        }
        // The last element must end inside the buffer: checked before allocating.
        if count64 > 0 {
            let end = (count64 - 1)
                .checked_mul(stride)
                .and_then(|x| x.checked_add(start))
                .and_then(|x| x.checked_add(elem))
                .ok_or_else(|| err("accessor runs past its buffer"))?;
            if end > data.len() as u64 {
                return Err(err("accessor runs past its buffer"));
            }
        }
        let (count, start, stride) = (count64 as usize, start as usize, stride as usize);
        let mut out = Vec::with_capacity(count * comps);
        for k in 0..count {
            for c in 0..comps {
                let o = start + k * stride + c * size;
                let raw = data
                    .get(o..o + size)
                    .ok_or_else(|| err("accessor runs past its buffer"))?;
                let x = match ct {
                    5120 => {
                        let v = f64::from(raw[0] as i8);
                        if normalized { (v / 127.0).max(-1.0) } else { v }
                    }
                    5121 => {
                        let v = f64::from(raw[0]);
                        if normalized { v / 255.0 } else { v }
                    }
                    5122 => {
                        let v = f64::from(i16::from_le_bytes([raw[0], raw[1]]));
                        if normalized {
                            (v / 32767.0).max(-1.0)
                        } else {
                            v
                        }
                    }
                    5123 => {
                        let v = f64::from(u16::from_le_bytes([raw[0], raw[1]]));
                        if normalized { v / 65535.0 } else { v }
                    }
                    5125 => f64::from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    _ => f64::from(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                };
                out.push(x);
            }
        }
        Ok((out, comps))
    }

    fn local(node: &Value) -> DMat4 {
        if let Some(m) = node["matrix"].as_array()
            && m.len() == 16
        {
            let v: Vec<f64> = m.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
            return DMat4::from_cols_slice(&v);
        }
        let v3 = |k: &str, d: f64| -> DVec3 {
            node[k]
                .as_array()
                .filter(|a| a.len() == 3)
                .map(|a| {
                    DVec3::new(
                        a[0].as_f64().unwrap_or(d),
                        a[1].as_f64().unwrap_or(d),
                        a[2].as_f64().unwrap_or(d),
                    )
                })
                .unwrap_or(DVec3::splat(d))
        };
        let r = node["rotation"]
            .as_array()
            .filter(|a| a.len() == 4)
            .map(|a| {
                DQuat::from_xyzw(
                    a[0].as_f64().unwrap_or(0.0),
                    a[1].as_f64().unwrap_or(0.0),
                    a[2].as_f64().unwrap_or(0.0),
                    a[3].as_f64().unwrap_or(1.0),
                )
                .normalize()
            })
            .unwrap_or(DQuat::IDENTITY);
        DMat4::from_scale_rotation_translation(v3("scale", 1.0), r, v3("translation", 0.0))
    }
}

pub fn read(bytes: &[u8], opts: &ImportOptions) -> Result<Parsed, ImportError> {
    read_with_ids(bytes, opts).map(|(p, _)| p)
}

/// The items of a Sawhorse-written GLB keyed by the object id in each node's
/// `extras.sawhorse.object` (the `.shz` reader's fallback when `scene.json` has no exact
/// B-rep for an object).
pub fn items_by_object(
    bytes: &[u8],
) -> Result<std::collections::BTreeMap<u64, ImportItem>, ImportError> {
    let (p, ids) = read_with_ids(bytes, &ImportOptions::default())?;
    Ok(ids
        .into_iter()
        .zip(p.items)
        .filter_map(|(id, item)| Some((id?, item)))
        .collect())
}

/// [`read`], plus each item's `extras.sawhorse.object` id when its node has one.
fn read_with_ids(
    bytes: &[u8],
    opts: &ImportOptions,
) -> Result<(Parsed, Vec<Option<u64>>), ImportError> {
    let (doc, bin) = if bytes.starts_with(b"glTF") {
        split_glb(bytes)?
    } else {
        let doc: Value = serde_json::from_slice(bytes).map_err(|e| err(e.to_string()))?;
        (doc, None)
    };
    if doc["asset"]["version"]
        .as_str()
        .is_some_and(|v| !v.starts_with('2'))
    {
        return Err(err("only glTF 2.0 is supported"));
    }
    let mut out = Parsed::default();
    let mut ids: Vec<Option<u64>> = Vec::new();
    let buffers = doc["buffers"]
        .as_array()
        .map(|bs| {
            bs.iter()
                .map(|b| match b["uri"].as_str() {
                    None => bin.map(std::borrow::Cow::Borrowed),
                    Some(u) if u.starts_with("data:") => u
                        .split_once(',')
                        .and_then(|(_, d)| base64_decode(d))
                        .map(std::borrow::Cow::Owned),
                    Some(_) => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let g = Gltf { doc, buffers };

    // Materials.
    let mut mat_names: Vec<String> = Vec::new();
    for (i, m) in g.arr("materials").iter().enumerate() {
        let name = m["name"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("Material {}", i + 1));
        let f = &m["pbrMetallicRoughness"]["baseColorFactor"];
        let c = |k: usize, d: f64| f[k].as_f64().unwrap_or(d);
        let color = [
            linear_to_srgb(c(0, 1.0)),
            linear_to_srgb(c(1, 1.0)),
            linear_to_srgb(c(2, 1.0)),
            (c(3, 1.0).clamp(0.0, 1.0) * 255.0).round() as u8,
        ];
        let texture = m["pbrMetallicRoughness"]["baseColorTexture"]["index"]
            .as_u64()
            .and_then(|t| g.arr("textures").get(t as usize))
            .and_then(|t| t["source"].as_u64())
            .and_then(|s| g.arr("images").get(s as usize))
            .and_then(|img| {
                img["uri"]
                    .as_str()
                    .filter(|u| !u.starts_with("data:"))
                    .or(img["name"].as_str())
            })
            .map(str::to_string)
            .or_else(|| {
                m["extras"]["sawhorse"]["texture"]
                    .as_str()
                    .map(str::to_string)
            });
        out.materials
            .push(MaterialDef::plain(&name, color, texture));
        mat_names.push(name);
    }

    // Walk the scene.
    let roots: Vec<usize> = match g.doc["scene"]
        .as_u64()
        .or_else(|| g.arr("scenes").first().map(|_| 0))
    {
        Some(s) => g
            .arr("scenes")
            .get(s as usize)
            .and_then(|sc| sc["nodes"].as_array())
            .map(|n| {
                n.iter()
                    .filter_map(|x| x.as_u64().map(|v| v as usize))
                    .collect()
            })
            .unwrap_or_default(),
        None => (0..g.arr("nodes").len()).collect(),
    };
    let (mut skipped_points, mut failed) = (0usize, Vec::new());
    let mut budget = Budget::default();
    // Lane 16: glTF nodes form a tree; a node reached twice (a node listed as the child of
    // two parents, or twice by one) would be walked again for every path to it.
    let mut visited = vec![false; g.arr("nodes").len()];
    let mut stack: Vec<(usize, DMat4, usize)> = roots
        .into_iter()
        .rev()
        .map(|n| (n, DMat4::IDENTITY, 0))
        .collect();
    while let Some((ni, parent, depth)) = stack.pop() {
        let Some(node) = g.arr("nodes").get(ni) else {
            continue;
        };
        if std::mem::replace(&mut visited[ni], true) {
            return Err(err(format!(
                "node {ni} is reached more than once (glTF nodes form a tree)"
            )));
        }
        if depth > 64 {
            return Err(err("the node tree is deeper than 64 levels (a cycle?)"));
        }
        let world = parent * Gltf::local(node);
        if let Some(children) = node["children"].as_array() {
            for c in children.iter().rev().filter_map(Value::as_u64) {
                stack.push((c as usize, world, depth + 1));
            }
        }
        let Some(mi) = node["mesh"].as_u64() else {
            continue;
        };
        let Some(mesh) = g.arr("meshes").get(mi as usize) else {
            continue;
        };
        let name = node["name"]
            .as_str()
            .or(mesh["name"].as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{} {}", opts.stem(), mi + 1));
        let mirrored = world.determinant() < 0.0;
        let mut item = ImportItem {
            name: name.clone(),
            group: node["extras"]["sawhorse"]["groups"][0]
                .as_str()
                .map(str::to_string),
            tag: node["extras"]["sawhorse"]["tag"]
                .as_str()
                .map(str::to_string),
            material: None,
            positions: Vec::new(),
            faces: Vec::new(),
            lines: Vec::new(),
            merge_coplanar: true,
            face_labels: Vec::new(),
            face_materials: vec![],
            face_uvs: vec![],
            edge_flags: vec![],
        };
        let mut all_labelled = true;
        let mut material_use: std::collections::BTreeMap<usize, usize> = Default::default();
        let prims = mesh["primitives"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut read_prim = |p: &Value, item: &mut ImportItem| -> Result<(), ImportError> {
            let pos_acc = p["attributes"]["POSITION"]
                .as_u64()
                .ok_or_else(|| err("a primitive without POSITION"))?;
            let (pos, comps) = g.accessor(pos_acc as usize)?;
            if comps != 3 {
                return Err(err("POSITION must be VEC3"));
            }
            budget.points("glTF", pos.len() / 3)?;
            let base = item.positions.len() as u32;
            for c in pos.chunks(3) {
                let w = world.transform_point3(DVec3::new(c[0], c[1], c[2]));
                item.positions.push([w.x, -w.z, w.y]);
            }
            let n = (pos.len() / 3) as u32;
            let idx: Vec<u32> = match p["indices"].as_u64() {
                Some(a) => g
                    .accessor(a as usize)?
                    .0
                    .iter()
                    .map(|&x| x as u32)
                    .collect(),
                None => (0..n).collect(),
            };
            if idx.iter().any(|&i| i >= n) {
                return Err(err("an index points past the vertices"));
            }
            // One label per vertex, or none (lane 16: a short label list used to index past
            // its end).
            let labels: Option<Vec<f64>> = match p["attributes"]["_FACE_ID"].as_u64() {
                Some(a) => match g.accessor(a as usize)? {
                    (l, 1) if l.len() == n as usize => Some(l),
                    _ => None,
                },
                None => None,
            };
            let mode = p["mode"].as_u64().unwrap_or(4);
            let mut tris: Vec<[u32; 3]> = Vec::new();
            match mode {
                4 => tris.extend(idx.as_chunks::<3>().0.iter().map(|t| [t[0], t[1], t[2]])),
                5 => {
                    for k in 2..idx.len() {
                        let t = if k % 2 == 0 {
                            [idx[k - 2], idx[k - 1], idx[k]]
                        } else {
                            [idx[k - 1], idx[k - 2], idx[k]]
                        };
                        tris.push(t);
                    }
                }
                6 => {
                    for k in 2..idx.len() {
                        tris.push([idx[0], idx[k - 1], idx[k]]);
                    }
                }
                1 => item.lines.extend(
                    idx.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|l| [base + l[0], base + l[1]]),
                ),
                2 | 3 => {
                    for k in 1..idx.len() {
                        item.lines.push([base + idx[k - 1], base + idx[k]]);
                    }
                    if mode == 2 && idx.len() > 2 {
                        item.lines.push([base + idx[idx.len() - 1], base + idx[0]]);
                    }
                }
                _ => skipped_points += 1,
            }
            budget.faces("glTF", tris.len() + item.lines.len())?;
            for t in tris {
                let t = if mirrored { [t[0], t[2], t[1]] } else { t };
                item.faces.push(vec![t.iter().map(|&v| base + v).collect()]);
                match &labels {
                    Some(l) => item.face_labels.push(l[t[0] as usize] as u32),
                    None => all_labelled = false,
                }
            }
            if let Some(m) = p["material"].as_u64() {
                *material_use.entry(m as usize).or_default() += 1;
            }
            Ok(())
        };
        for p in prims {
            if let Err(e) = read_prim(p, &mut item) {
                failed.push(format!("{name}: {e}"));
            }
        }
        // A primitive failing is a note; the file as a whole over the limits is a refusal.
        budget.check("glTF")?;
        if !all_labelled {
            item.face_labels.clear();
        }
        item.material = material_use
            .iter()
            .max_by_key(|(m, n)| (**n, std::cmp::Reverse(**m)))
            .and_then(|(m, _)| mat_names.get(*m).cloned());
        if material_use.len() > 1 {
            out.notes.push(format!(
                "{name}: {} materials on one mesh; painted with the most used",
                material_use.len()
            ));
        }
        if !item.faces.is_empty() || !item.lines.is_empty() {
            budget.items("glTF", 1)?;
            ids.push(node["extras"]["sawhorse"]["object"].as_u64());
            out.items.push(item);
        }
    }
    if skipped_points > 0 {
        out.notes.push(format!(
            "glTF: skipped {skipped_points} point primitive{}",
            plural(skipped_points)
        ));
    }
    for f in failed {
        out.notes.push(format!("glTF: {f}"));
    }
    if out.items.is_empty()
        && let Some(n) = out.notes.iter().find(|n| n.contains("external file"))
    {
        return Err(err(n.clone()));
    }
    Ok((out, ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_export::{Scope, gltf};
    use sawhorse_kernel::{Document, Op, OpOutcome};

    #[test]
    fn round_trip_through_our_glb_keeps_faces_ids_names_and_paint() {
        let mut src = crate::test_support::two_boxes();
        src.apply(&Op::SplitFace {
            object: 1,
            face: 2,
            path: vec![[0.05, 0.0, 0.05], [0.05, 0.2, 0.05]],
        })
        .unwrap();
        for op in [
            r#"{"op":"define_material","name":"Cherry","color":[150,60,40,255]}"#,
            r#"{"op":"set_material","object":2,"material":"Cherry"}"#,
            r#"{"op":"rename_object","object":2,"name":"Block"}"#,
        ] {
            src.apply(&serde_json::from_str(op).unwrap()).unwrap();
        }
        let bytes = gltf::document(src.model(), &Scope::visible(src.model())).unwrap();
        let p = read(&bytes, &ImportOptions::default()).unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[1].name, "Block");
        assert_eq!(p.items[1].material.as_deref(), Some("Cherry"));
        assert_eq!(
            p.materials[0].color,
            [150, 60, 40, 255],
            "linear and back to sRGB"
        );
        assert_eq!(p.items[0].face_labels.len(), p.items[0].faces.len());
        let mut d = Document::with_ground_sketch();
        let OpOutcome::Imported { objects, .. } = d.apply(&p.op()).unwrap() else {
            panic!()
        };
        assert_eq!(objects.len(), 2);
        let a = src.model().object(1).unwrap().brep().to_polys();
        let b = d.model().object(1).unwrap().brep().to_polys();
        assert_eq!(
            a.faces.len(),
            b.faces.len(),
            "seven faces: the split top stays split"
        );
        assert_eq!(b.user_edges.len(), 1);
        assert!((src.model().stats().volume_m3 - d.model().stats().volume_m3).abs() < 1e-9);
        // Y up -> Z up: the boxes stand on the ground again.
        let min_z = p
            .items
            .iter()
            .flat_map(|i| &i.positions)
            .map(|p| p[2])
            .fold(f64::INFINITY, f64::min);
        assert!(min_z.abs() < 1e-7);
        assert_eq!(d.model().object(2).unwrap().material(), Some("Cherry"));
    }

    #[test]
    fn embedded_gltf_with_node_transforms() {
        // One triangle (in glTF Y-up) on a node translated 1 m along +X and scaled 2x.
        let tri: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let b64 = {
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in tri.chunks(3) {
                let n = (u32::from(c[0]) << 16)
                    | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                    | u32::from(*c.get(2).unwrap_or(&0));
                for k in 0..4 {
                    if k <= c.len() {
                        s.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
                    } else {
                        s.push('=');
                    }
                }
            }
            s
        };
        let doc = serde_json::json!({
            "asset": {"version": "2.0"},
            "buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{b64}")}],
            "bufferViews": [{"buffer": 0, "byteLength": 36}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
            "meshes": [{"name": "tri", "primitives": [{"attributes": {"POSITION": 0}}]}],
            "nodes": [{"name": "Parent", "translation": [1, 0, 0], "children": [1]}, {"mesh": 0, "scale": [2, 2, 2]}],
            "scenes": [{"nodes": [0]}]
        });
        let p = read(doc.to_string().as_bytes(), &ImportOptions::default()).unwrap();
        assert_eq!(p.items[0].name, "tri");
        // glTF (0,0,-1) scaled 2 -> (0,0,-2), moved +1 x -> (1,0,-2) -> Z up (1, 2, 0).
        assert_eq!(p.items[0].positions[2], [1.0, 2.0, 0.0]);
        let ext = serde_json::json!({"asset": {"version": "2.0"}, "buffers": [{"byteLength": 36, "uri": "tri.bin"}],
            "bufferViews": [{"buffer": 0, "byteLength": 36}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}], "nodes": [{"mesh": 0}]});
        let e = read(ext.to_string().as_bytes(), &ImportOptions::default()).unwrap_err();
        assert!(e.to_string().contains("external file"), "{e}");
    }
}
