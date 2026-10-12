//! 3MF: every build item becomes one item (components resolved recursively with their
//! transforms), in the model's own `unit`, Z up as the spec says. An object's
//! `pid`/`pindex` pointing at a base-material group gives it a colour. Read through the
//! `threemf` crate (core spec mesh, components and build).

use std::collections::BTreeMap;
use std::io::Cursor;

use glam::{DMat4, DVec3};
use sawhorse_kernel::meshbuild::ImportItem;
use sawhorse_kernel::model::MaterialDef;
use threemf::model::{Model, Object, Unit as TUnit};

use crate::{ImportError, ImportOptions, Parsed, malformed};

fn err(m: impl Into<String>) -> ImportError {
    malformed("3MF", m)
}

fn unit_metres(u: &TUnit) -> f64 {
    match u {
        TUnit::Micron => 1e-6,
        TUnit::Millimeter => 1e-3,
        TUnit::Centimeter => 1e-2,
        TUnit::Inch => 0.0254,
        TUnit::Foot => 0.3048,
        TUnit::Meter => 1.0,
    }
}

/// A 3MF transform (`m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32`, row vectors).
fn transform(t: &Option<[f64; 12]>) -> DMat4 {
    match t {
        Some(m) => DMat4::from_cols_array(&[
            m[0], m[1], m[2], 0.0, m[3], m[4], m[5], 0.0, m[6], m[7], m[8], 0.0, m[9], m[10],
            m[11], 1.0,
        ]),
        None => DMat4::IDENTITY,
    }
}

/// Parse a `#RRGGBB` or `#RRGGBBAA` display colour.
fn hex_color(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix('#')?;
    let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some([b(0)?, b(2)?, b(4)?, 255]),
        8 => Some([b(0)?, b(2)?, b(4)?, b(6)?]),
        _ => None,
    }
}

/// (group id, index) -> (name, colour).
type BaseMaterials = BTreeMap<(usize, usize), (String, [u8; 4])>;

/// Base-material groups of every model part: (group id, index) -> (name, colour). Read
/// with a plain XML scan because `threemf`'s serde model does not see the prefixed
/// `m:basematerials` element.
fn base_materials(bytes: &[u8]) -> Result<BaseMaterials, ImportError> {
    use quick_xml::events::Event;
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| err(e.to_string()))?;
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| err(e.to_string()))?;
        if !f.name().ends_with(".model") {
            continue;
        }
        let mut xml = Vec::new();
        f.read_to_end(&mut xml).map_err(|e| err(e.to_string()))?;
        let mut r = quick_xml::Reader::from_reader(xml.as_slice());
        let mut buf = Vec::new();
        let mut group: Option<(usize, usize)> = None;
        loop {
            match r
                .read_event_into(&mut buf)
                .map_err(|e| err(e.to_string()))?
            {
                Event::Start(e) | Event::Empty(e) => {
                    let attr = |k: &str| {
                        e.attributes()
                            .flatten()
                            .find(|a| a.key.local_name().as_ref() == k.as_bytes())
                            .and_then(|a| a.unescape_value().ok().map(|v| v.to_string()))
                    };
                    match e.local_name().as_ref() {
                        b"basematerials" => {
                            group = attr("id").and_then(|v| v.parse().ok()).map(|g| (g, 0))
                        }
                        b"base" => {
                            if let Some((g, k)) = group.as_mut() {
                                let name = attr("name").unwrap_or_default();
                                let color = attr("displaycolor")
                                    .and_then(|c| hex_color(&c))
                                    .unwrap_or([200, 200, 200, 255]);
                                out.insert((*g, *k), (name, color));
                                *k += 1;
                            }
                        }
                        _ => {}
                    }
                }
                Event::End(e) if e.local_name().as_ref() == b"basematerials" => group = None,
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
    }
    Ok(out)
}

fn collect(
    objects: &BTreeMap<usize, &Object>,
    id: usize,
    m: DMat4,
    scale: f64,
    item: &mut ImportItem,
    depth: usize,
) -> Result<(), ImportError> {
    if depth > 32 {
        return Err(err("components nest deeper than 32 levels (a cycle?)"));
    }
    let o = objects
        .get(&id)
        .ok_or_else(|| err(format!("no object {id}")))?;
    if let Some(mesh) = &o.mesh {
        let base = item.positions.len() as u32;
        let mirrored = m.determinant() < 0.0;
        for v in &mesh.vertices.vertex {
            let p = m.transform_point3(DVec3::new(v.x, v.y, v.z)) * scale;
            item.positions.push(p.to_array());
        }
        let n = mesh.vertices.vertex.len();
        for t in &mesh.triangles.triangle {
            if t.v1 >= n || t.v2 >= n || t.v3 >= n {
                return Err(err(format!(
                    "object {id}: a triangle names a missing vertex"
                )));
            }
            let (a, b, c) = (base + t.v1 as u32, base + t.v2 as u32, base + t.v3 as u32);
            item.faces.push(vec![if mirrored {
                vec![a, c, b]
            } else {
                vec![a, b, c]
            }]);
        }
    }
    if let Some(cs) = &o.components {
        for c in &cs.component {
            collect(
                objects,
                c.objectid,
                m * transform(&c.transform),
                scale,
                item,
                depth + 1,
            )?;
        }
    }
    Ok(())
}

pub fn read(bytes: &[u8], opts: &ImportOptions) -> Result<Parsed, ImportError> {
    let models: Vec<Model> = threemf::read(Cursor::new(bytes)).map_err(|e| err(e.to_string()))?;
    let groups = base_materials(bytes)?;
    let mut out = Parsed::default();
    for model in &models {
        let scale = unit_metres(&model.unit);
        let objects: BTreeMap<usize, &Object> =
            model.resources.object.iter().map(|o| (o.id, o)).collect();
        // Base materials: (group id, index) -> material name.
        let mut mats: BTreeMap<(usize, usize), String> = BTreeMap::new();
        for ((gid, k), (name, color)) in &groups {
            let name = if name.trim().is_empty() {
                format!("Material {}", out.materials.len() + 1)
            } else {
                name.clone()
            };
            if !out.materials.iter().any(|m| m.name == name) {
                out.materials.push(MaterialDef {
                    name: name.clone(),
                    color: *color,
                    texture: None,
                });
            }
            mats.insert((*gid, *k), name);
        }
        for (k, it) in model.build.item.iter().enumerate() {
            let o = objects
                .get(&it.objectid)
                .ok_or_else(|| err(format!("build item names missing object {}", it.objectid)))?;
            let mut item = ImportItem {
                name: o
                    .name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| format!("{} {}", opts.stem(), k + 1)),
                group: None,
                tag: None,
                material: o.pid.zip(o.pindex).and_then(|key| mats.get(&key).cloned()),
                positions: Vec::new(),
                faces: Vec::new(),
                lines: Vec::new(),
                merge_coplanar: true,
                face_labels: Vec::new(),
            };
            collect(
                &objects,
                it.objectid,
                transform(&it.transform),
                scale,
                &mut item,
                0,
            )?;
            if !item.faces.is_empty() {
                out.items.push(item);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_export::{Scope, three_mf};
    use sawhorse_kernel::{Document, OpOutcome};

    #[test]
    fn round_trip_through_our_3mf() {
        let mut src = crate::test_support::two_boxes();
        for op in [
            r#"{"op":"define_material","name":"Ash","color":[220,210,190,255]}"#,
            r#"{"op":"set_material","object":1,"material":"Ash"}"#,
            r#"{"op":"rename_object","object":1,"name":"Plank"}"#,
        ] {
            src.apply(&serde_json::from_str(op).unwrap()).unwrap();
        }
        let bytes = three_mf::document(src.model(), &Scope::visible(src.model())).unwrap();
        let p = read(&bytes, &ImportOptions::default()).unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0].name, "Plank");
        assert_eq!(p.items[0].material.as_deref(), Some("Ash"));
        assert_eq!(p.materials[0].color, [220, 210, 190, 255]);
        let mut d = Document::with_ground_sketch();
        let OpOutcome::Imported {
            objects, meshes, ..
        } = d.apply(&p.op()).unwrap()
        else {
            panic!()
        };
        assert_eq!((objects.len(), meshes.len()), (2, 0));
        let (a, b) = (src.model().stats(), d.model().stats());
        assert_eq!(
            (a.faces, a.edges, a.vertices),
            (b.faces, b.edges, b.vertices)
        );
        assert!((a.volume_m3 - b.volume_m3).abs() < 1e-12);
    }

    #[test]
    fn transforms_and_colours() {
        let t = transform(&Some([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 10.0, 20.0, 30.0,
        ]));
        assert_eq!(
            t.transform_point3(DVec3::ZERO),
            DVec3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(hex_color("#FF8000"), Some([255, 128, 0, 255]));
        assert_eq!(hex_color("#FF800080"), Some([255, 128, 0, 128]));
        assert_eq!(hex_color("red"), None);
        assert!(read(b"PK\x03\x04garbage", &ImportOptions::default()).is_err());
    }
}
