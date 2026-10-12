//! The whole model as plain, versionable data: what `.shz` files store in `scene.json`
//! (see `docs/format.md`) and what [`Op::Restore`](crate::model::Op::Restore) brings back.
//!
//! Lossless for everything the kernel has: sketches (vertices, edges, regions with their
//! flags), solids as exact f64 polygon B-reps (faces with holes, stored planes, persistent
//! face ids and each object's id counter, user-drawn edges), imported non-solid meshes,
//! groups, tags, materials, scenes, guides and dimensions. Restoring validates everything
//! exactly like an edit does; a file that does not validate is refused, never patched.
//!
//! Public ids are renumbered compactly in saved order (1, 2, 3, ...): a document whose ids
//! were already compact (nothing deleted) restores to the same model hash.

use std::collections::BTreeMap;

use glam::DVec3;

use crate::brep::{Brep, PolyFace, Polys};
use crate::error::{DocumentError, KernelError};
use crate::meshbuild::LooseMesh;
use crate::model::{MaterialDef, Model, SceneDef};
use crate::plane::Plane;
use crate::sketch::RawSketch;

/// Current snapshot schema. Readers accept files with a higher minor-compatible schema and
/// ignore fields they do not know.
pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapPlane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
}

impl From<&Plane> for SnapPlane {
    fn from(p: &Plane) -> SnapPlane {
        SnapPlane {
            origin: p.origin.to_array(),
            normal: p.normal.to_array(),
            u: p.u.to_array(),
            v: p.v.to_array(),
        }
    }
}

impl SnapPlane {
    /// The stored frame, checked to be orthonormal and right-handed.
    pub fn plane(&self) -> Result<Plane, String> {
        let p = Plane {
            origin: DVec3::from_array(self.origin),
            normal: DVec3::from_array(self.normal),
            u: DVec3::from_array(self.u),
            v: DVec3::from_array(self.v),
        };
        let ok = [p.origin, p.normal, p.u, p.v].iter().all(|v| v.is_finite())
            && (p.normal.length() - 1.0).abs() < 1e-9
            && (p.u.length() - 1.0).abs() < 1e-9
            && p.u.cross(p.v).distance(p.normal) < 1e-9;
        if ok {
            Ok(p)
        } else {
            Err("a plane frame is not orthonormal".into())
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapSketch {
    pub plane: SnapPlane,
    #[serde(flatten)]
    pub graph: RawSketch,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapFace {
    /// Persistent face id.
    pub id: u64,
    pub outer: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<u32>>,
    pub plane: SnapPlane,
}

/// A solid's exact B-rep: f64 points, faces as index rings, user-drawn edges.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapBrep {
    pub points: Vec<[f64; 3]>,
    pub faces: Vec<SnapFace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_edges: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapObject {
    /// The public id it had when saved (groups refer to objects by it).
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The next persistent face id the object hands out.
    pub next_face_id: u64,
    /// Display number ("Solid 3"), stable for the object's life. Optional: a file without
    /// numbers (or with duplicates) gets fresh ones in saved order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// Index (in `sketches`) of the sketch it was extruded from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sketch: Option<usize>,
    /// Exact geometry. A writer that cannot produce it may leave it out; readers then
    /// rebuild the object from `geometry.glb` (see `docs/format.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brep: Option<SnapBrep>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapMesh {
    pub id: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    pub note: String,
    pub positions: Vec<[f64; 3]>,
    pub faces: Vec<Vec<Vec<u32>>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapGroup {
    pub name: String,
    /// Saved object ids ([`SnapObject::id`]).
    pub objects: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapGuide {
    pub point: [f64; 3],
    /// Direction of a guide line; absent for a guide point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapDimension {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub offset: [f64; 3],
}

/// The whole model as plain data.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Snapshot {
    #[serde(default)]
    pub sketches: Vec<SnapSketch>,
    #[serde(default)]
    pub objects: Vec<SnapObject>,
    #[serde(default)]
    pub meshes: Vec<SnapMesh>,
    #[serde(default)]
    pub groups: Vec<SnapGroup>,
    /// Tag name -> visible.
    #[serde(default)]
    pub tags: BTreeMap<String, bool>,
    #[serde(default)]
    pub materials: Vec<MaterialDef>,
    #[serde(default)]
    pub scenes: Vec<SceneDef>,
    #[serde(default)]
    pub guides: Vec<SnapGuide>,
    #[serde(default)]
    pub dimensions: Vec<SnapDimension>,
    /// The last display number handed out, so a deleted solid's number is not reused after
    /// a reopen. Optional (0 = derive it from the objects' numbers).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub last_number: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn bad(m: impl Into<String>) -> KernelError {
    DocumentError::BadSnapshot(m.into()).into()
}

impl Model {
    /// Everything, as plain data (deterministic order: creation order for sketches,
    /// objects and meshes; id order for groups, guides and dimensions).
    pub fn snapshot(&self) -> Snapshot {
        let sketch_index: BTreeMap<crate::ids::SketchId, usize> = self
            .sketch_order
            .iter()
            .enumerate()
            .map(|(i, &k)| (k, i))
            .collect();
        Snapshot {
            sketches: self
                .sketch_order
                .iter()
                .map(|&k| {
                    let s = &self.sketches[k];
                    SnapSketch {
                        plane: s.plane().into(),
                        graph: s.to_raw(),
                    }
                })
                .collect(),
            objects: self
                .objects()
                .into_iter()
                .map(|(id, o)| {
                    let p = o.brep().to_polys();
                    SnapObject {
                        id,
                        name: o.name.clone(),
                        tag: o.tag.clone(),
                        material: o.material.clone(),
                        next_face_id: o.next_face_id,
                        number: (o.number > 0).then_some(o.number),
                        source_sketch: o
                            .source
                            .as_ref()
                            .and_then(|s| sketch_index.get(&s.sketch).copied()),
                        brep: Some(SnapBrep {
                            points: p.points.iter().map(|v| v.to_array()).collect(),
                            faces: p
                                .faces
                                .iter()
                                .enumerate()
                                .map(|(i, f)| SnapFace {
                                    id: f.id.unwrap_or(0),
                                    outer: f.outer.iter().map(|&k| k as u32).collect(),
                                    holes: f
                                        .holes
                                        .iter()
                                        .map(|h| h.iter().map(|&k| k as u32).collect())
                                        .collect(),
                                    plane: (&p.plane(i).unwrap_or(Plane::GROUND)).into(),
                                })
                                .collect(),
                            user_edges: p
                                .user_edges
                                .iter()
                                .map(|&(a, b)| [a as u32, b as u32])
                                .collect(),
                        }),
                    }
                })
                .collect(),
            meshes: self
                .meshes()
                .into_iter()
                .map(|(id, m)| SnapMesh {
                    id,
                    name: m.name.clone(),
                    tag: m.tag.clone(),
                    material: m.material.clone(),
                    note: m.note.clone(),
                    positions: m.positions.iter().map(|p| p.to_array()).collect(),
                    faces: m.faces.clone(),
                    lines: m.lines.clone(),
                })
                .collect(),
            groups: self
                .groups()
                .into_iter()
                .map(|(_, g)| SnapGroup {
                    name: g.name.clone(),
                    objects: g.objects.clone(),
                })
                .collect(),
            tags: self.tags().clone(),
            materials: self
                .materials()
                .iter()
                .map(|(name, m)| MaterialDef {
                    name: name.clone(),
                    color: m.color,
                    texture: m.texture.clone(),
                })
                .collect(),
            scenes: self.scenes().to_vec(),
            guides: self
                .guides()
                .into_iter()
                .map(|(_, g)| SnapGuide {
                    point: g.point.to_array(),
                    dir: (g.dir != DVec3::ZERO).then(|| g.dir.to_array()),
                })
                .collect(),
            dimensions: self
                .dimensions()
                .into_iter()
                .map(|(_, d)| SnapDimension {
                    a: d.a.to_array(),
                    b: d.b.to_array(),
                    offset: d.offset.to_array(),
                })
                .collect(),
            last_number: self.last_number,
        }
    }

    /// Rebuild a model from a snapshot and validate it like any edit. Objects without a
    /// `brep` are skipped here (the `.shz` reader rebuilds those from `geometry.glb`).
    pub fn from_snapshot(s: &Snapshot) -> Result<Model, KernelError> {
        let mut m = Model::new();
        let mut sketch_ids = Vec::with_capacity(s.sketches.len());
        for (i, sk) in s.sketches.iter().enumerate() {
            let plane = sk
                .plane
                .plane()
                .map_err(|e| bad(format!("sketch {i}: {e}")))?;
            let sketch = crate::sketch::Sketch::from_raw(plane, &sk.graph)
                .map_err(|e| bad(format!("sketch {i}: {e}")))?;
            let id = m.sketches.insert(sketch);
            m.sketch_order.push(id);
            sketch_ids.push(id);
        }
        for (name, mat) in s.materials.iter().map(|d| (&d.name, d)) {
            m.materials.insert(
                name.clone(),
                crate::model::Material {
                    color: mat.color,
                    texture: mat.texture.clone(),
                },
            );
        }
        for (t, v) in &s.tags {
            m.tags.insert(t.clone(), *v);
        }
        let mut object_ids: BTreeMap<u64, u64> = BTreeMap::new();
        // Saved display numbers are kept when every restored solid has one and none repeats;
        // otherwise every solid gets a fresh number in saved order.
        let keep_numbers = {
            let mut seen = std::collections::BTreeSet::new();
            s.objects
                .iter()
                .filter(|o| o.brep.is_some())
                .all(|o| matches!(o.number, Some(n) if n > 0 && seen.insert(n)))
        };
        if keep_numbers {
            let top = s
                .objects
                .iter()
                .filter_map(|o| o.brep.as_ref().and(o.number))
                .max()
                .unwrap_or(0);
            m.last_number = top.max(s.last_number);
        }
        for o in &s.objects {
            let Some(b) = &o.brep else { continue };
            let n = b.points.len();
            let pts: Vec<DVec3> = b.points.iter().map(|p| DVec3::from_array(*p)).collect();
            if pts
                .iter()
                .any(|p| !p.is_finite() || p.abs().max_element() > crate::tolerance::MAX_COORD)
            {
                return Err(bad(format!("object {}: a point out of bounds", o.id)));
            }
            let idx = |r: &[u32]| -> Result<Vec<usize>, KernelError> {
                r.iter()
                    .map(|&k| {
                        ((k as usize) < n)
                            .then_some(k as usize)
                            .ok_or_else(|| bad(format!("object {}: index {k} out of range", o.id)))
                    })
                    .collect()
            };
            let mut faces = Vec::with_capacity(b.faces.len());
            for f in &b.faces {
                let mut pf = PolyFace::new(
                    idx(&f.outer)?,
                    f.holes.iter().map(|h| idx(h)).collect::<Result<_, _>>()?,
                );
                pf.plane = Some(
                    f.plane
                        .plane()
                        .map_err(|e| bad(format!("object {}: {e}", o.id)))?,
                );
                pf.id = Some(f.id);
                faces.push(pf);
            }
            let mut user = std::collections::BTreeSet::new();
            for e in &b.user_edges {
                let (a, c) = (e[0] as usize, e[1] as usize);
                if a >= n || c >= n {
                    return Err(bad(format!("object {}: user edge out of range", o.id)));
                }
                user.insert(Polys::key(a, c));
            }
            let polys = Polys {
                points: pts,
                faces,
                user_edges: user,
            };
            let brep = Brep::build(&polys).map_err(|e| bad(format!("object {}: {e}", o.id)))?;
            let mut obj = crate::object::Object::from_brep(brep);
            if o.next_face_id < obj.next_face_id {
                return Err(bad(format!(
                    "object {}: face ids at or above its counter",
                    o.id
                )));
            }
            obj.next_face_id = o.next_face_id;
            obj.name = o.name.clone();
            obj.tag = o.tag.clone();
            obj.material = o.material.clone();
            obj.source = o
                .source_sketch
                .and_then(|i| sketch_ids.get(i))
                .map(|&sketch| crate::object::Provenance { sketch });
            let id = match o.number {
                Some(n) if keep_numbers => {
                    obj.number = n;
                    let id = m.objects.insert(obj);
                    m.object_order.push(id);
                    id
                }
                _ => m.insert_object(obj),
            };
            object_ids.insert(o.id, crate::ids::to_public(id));
        }
        for mesh in &s.meshes {
            let id = m.meshes.insert(LooseMesh {
                name: mesh.name.clone(),
                positions: mesh
                    .positions
                    .iter()
                    .map(|p| DVec3::from_array(*p))
                    .collect(),
                faces: mesh.faces.clone(),
                lines: mesh.lines.clone(),
                tag: mesh.tag.clone(),
                material: mesh.material.clone(),
                note: mesh.note.clone(),
            });
            m.mesh_order.push(id);
        }
        for g in &s.groups {
            let objects: Vec<u64> = g
                .objects
                .iter()
                .filter_map(|o| object_ids.get(o).copied())
                .collect();
            if !objects.is_empty() {
                m.groups.insert(crate::model::Group {
                    name: g.name.clone(),
                    objects,
                });
            }
        }
        for sc in &s.scenes {
            m.scenes.push(sc.clone());
        }
        for g in &s.guides {
            let point = DVec3::from_array(g.point);
            let dir = g
                .dir
                .map(|d| DVec3::from_array(d).normalize_or_zero())
                .unwrap_or(DVec3::ZERO);
            if !point.is_finite() || !dir.is_finite() {
                return Err(bad("a guide is not finite"));
            }
            m.guides.insert(crate::model::Guide { point, dir });
        }
        for d in &s.dimensions {
            let (a, b, offset) = (
                DVec3::from_array(d.a),
                DVec3::from_array(d.b),
                DVec3::from_array(d.offset),
            );
            if !(a.is_finite() && b.is_finite() && offset.is_finite()) {
                return Err(bad("a dimension is not finite"));
            }
            m.dimensions
                .insert(crate::model::Dimension { a, b, offset });
        }
        // Everything an edit would check, plus objects' own rules.
        m.validate().map_err(|e| bad(e.to_string()))?;
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Op};

    #[test]
    fn every_golden_document_round_trips_to_the_same_hash() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        let mut n = 0;
        for f in std::fs::read_dir(dir).unwrap() {
            let f = f.unwrap().path();
            let g: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
            let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
            let d = Document::replay(&ops).unwrap();
            let snap = d.model().snapshot();
            let json = serde_json::to_string(&snap).unwrap();
            let back: Snapshot = serde_json::from_str(&json).unwrap();
            assert_eq!(back, snap, "{}", f.display());
            let m = Model::from_snapshot(&back).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            // Ids are compact in these documents unless something was deleted.
            let ids_compact = d
                .model()
                .object_ids()
                .iter()
                .enumerate()
                .all(|(i, &id)| id == i as u64 + 1);
            if ids_compact {
                assert_eq!(m.hash(), d.hash(), "{}", f.display());
            }
            // Saving what was opened gives the same snapshot (ids renumbered once).
            let again = Model::from_snapshot(&m.snapshot()).unwrap();
            assert_eq!(again.snapshot(), m.snapshot(), "{}", f.display());
            assert_eq!(again.hash(), m.hash());
            n += 1;
        }
        assert!(n >= 14);
    }

    #[test]
    fn tampered_snapshots_are_refused() {
        let d = Document::replay(&[
            Op::NewSketch {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            },
            Op::AddRectangle {
                sketch: 1,
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            Op::Extrude {
                sketch: 1,
                region: 1,
                distance: 0.5,
            },
        ])
        .unwrap();
        let good = d.model().snapshot();
        let mut s = good.clone();
        s.objects[0].brep.as_mut().unwrap().points[0][2] = 0.3; // off its faces' planes
        assert!(Model::from_snapshot(&s).is_err());
        let mut s = good.clone();
        s.objects[0].brep.as_mut().unwrap().faces.pop(); // open
        assert!(Model::from_snapshot(&s).is_err());
        let mut s = good.clone();
        s.objects[0].next_face_id = 2;
        assert!(Model::from_snapshot(&s).is_err());
        let mut s = good.clone();
        s.objects[0].material = Some("Nope".into());
        assert!(Model::from_snapshot(&s).is_err());
        let mut s = good;
        s.objects[0].brep.as_mut().unwrap().faces[0].outer[0] = 99;
        assert!(Model::from_snapshot(&s).is_err());
    }
}
