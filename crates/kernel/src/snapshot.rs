//! The whole model as plain, versionable data: what `.shz` files store in `scene.json`
//! (see `docs/format.md`) and what [`Op::Restore`](crate::model::Op::Restore) brings back.
//!
//! Lossless for everything the kernel has: sketches (vertices, edges, regions with their
//! flags), solids as exact f64 polygon B-reps (faces with holes, stored planes, persistent
//! face ids and each object's id counter, user-drawn edges), imported non-solid meshes,
//! groups and component instances (`nodes`), component definitions with their template
//! geometry (`definitions`), tags and their colours, materials, scenes, guides, dimensions
//! and the open editing context. Restoring validates everything exactly like an edit does;
//! a file that does not validate is refused, never patched.
//!
//! A component's geometry is stored once, in its definition. The solids of its instances
//! are listed (id, display number, parent, slot) without a B-rep and are derived again on
//! open, bit for bit, from the definition and the instance's placement.
//!
//! Public ids are renumbered compactly in saved order (1, 2, 3, ...): a document whose ids
//! were already compact (nothing deleted) restores to the same model hash.

use crate::cutlist::PartAttrs;
use std::collections::BTreeMap;

use glam::DVec3;

use crate::brep::{Brep, PolyFace, Polys};
use crate::error::{DocumentError, KernelError};
use crate::ids::{DefId, NodeId, to_public};
use crate::meshbuild::LooseMesh;
use crate::model::{MaterialDef, Model, SceneDef};
use crate::nodes::{Definition, Member, MemberKind, Membership, Node, NodeKind};
use crate::object::Object;
use crate::plane::Plane;
use crate::sketch::RawSketch;
use crate::transform::Xform;

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
    /// The group or instance it was drawn in (a saved node id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
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
    /// rebuild the object from `geometry.glb` (see `docs/format.md`). Always absent for a
    /// solid of a component instance (it has a `slot`): those are derived from the
    /// definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brep: Option<SnapBrep>,
    /// The group or instance it is in (a saved node id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// Inside a component instance: its definition member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Cut-list attributes (lane 7): grain, stock, sheet. Absent: none said.
    #[serde(default, skip_serializing_if = "PartAttrs::is_default")]
    pub part: PartAttrs,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A group or a component instance.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapNode {
    /// The id it had when saved (parents, scenes and the context refer to it).
    pub id: u64,
    /// `"group"` or `"instance"`.
    pub kind: SnapNodeKind,
    /// For an instance: index into `definitions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
    /// Local axes to world (an instance's placement). Absent: the identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xform: Option<Xform>,
    /// Cut-list attributes (lane 7): grain, stock, sheet. Absent: none said.
    #[serde(default, skip_serializing_if = "PartAttrs::is_default")]
    pub part: PartAttrs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum SnapNodeKind {
    Group,
    Instance,
}

/// A component definition: its members by slot, in its own coordinates.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapDefinition {
    pub name: String,
    pub next_slot: u32,
    pub members: Vec<SnapMember>,
    /// Cut-list attributes (lane 7): grain, stock, sheet. Absent: none said.
    #[serde(default, skip_serializing_if = "PartAttrs::is_default")]
    pub part: PartAttrs,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SnapMember {
    pub slot: u32,
    /// The slot of the nested group it is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Cut-list attributes of a solid or nested group member (lane 7).
    #[serde(default, skip_serializing_if = "PartAttrs::is_default")]
    pub part: PartAttrs,
    #[serde(flatten)]
    pub kind: SnapMemberKind,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapMemberKind {
    Solid {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        material: Option<String>,
        next_face_id: u64,
        brep: SnapBrep,
    },
    Mesh {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        material: Option<String>,
        #[serde(default)]
        note: String,
        positions: Vec<[f64; 3]>,
        faces: Vec<Vec<Vec<u32>>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        lines: Vec<[u32; 2]>,
    },
    Group {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        xform: Option<Xform>,
    },
    Instance {
        definition: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        xform: Option<Xform>,
    },
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
    /// Empty for a mesh of a component instance (derived from its definition).
    #[serde(default)]
    pub positions: Vec<[f64; 3]>,
    #[serde(default)]
    pub faces: Vec<Vec<Vec<u32>>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
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
    /// Lane-2 flat groups (name and solids directly in each group), still written for
    /// readers that do not know `nodes`; ignored when `nodes` is present.
    #[serde(default)]
    pub groups: Vec<SnapGroup>,
    /// Groups and component instances, in id order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<SnapNode>,
    /// Component definitions (instances refer to them by index).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<SnapDefinition>,
    /// The open group or instance when saved (a saved node id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Tag name -> visible.
    #[serde(default)]
    pub tags: BTreeMap<String, bool>,
    /// Tag name -> colour (`#rrggbb`), for tags that have one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tag_colors: BTreeMap<String, String>,
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

/// The exact B-rep of a solid as plain data.
pub fn snap_brep(o: &Object) -> SnapBrep {
    let p = o.brep().to_polys();
    SnapBrep {
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
    }
}

/// A solid from its saved B-rep and face-id counter (`what` names it in refusals).
pub fn brep_object(what: &str, b: &SnapBrep, next_face_id: u64) -> Result<Object, KernelError> {
    let n = b.points.len();
    let pts: Vec<DVec3> = b.points.iter().map(|p| DVec3::from_array(*p)).collect();
    if pts
        .iter()
        .any(|p| !p.is_finite() || p.abs().max_element() > crate::tolerance::MAX_COORD)
    {
        return Err(bad(format!("{what}: a point out of bounds")));
    }
    let idx = |r: &[u32]| -> Result<Vec<usize>, KernelError> {
        r.iter()
            .map(|&k| {
                ((k as usize) < n)
                    .then_some(k as usize)
                    .ok_or_else(|| bad(format!("{what}: index {k} out of range")))
            })
            .collect()
    };
    let mut faces = Vec::with_capacity(b.faces.len());
    for f in &b.faces {
        let mut pf = PolyFace::new(
            idx(&f.outer)?,
            f.holes.iter().map(|h| idx(h)).collect::<Result<_, _>>()?,
        );
        pf.plane = Some(f.plane.plane().map_err(|e| bad(format!("{what}: {e}")))?);
        pf.id = Some(f.id);
        faces.push(pf);
    }
    let mut user = std::collections::BTreeSet::new();
    for e in &b.user_edges {
        let (a, c) = (e[0] as usize, e[1] as usize);
        if a >= n || c >= n {
            return Err(bad(format!("{what}: user edge out of range")));
        }
        user.insert(Polys::key(a, c));
    }
    let polys = Polys {
        points: pts,
        faces,
        user_edges: user,
    };
    let brep = Brep::build(&polys).map_err(|e| bad(format!("{what}: {e}")))?;
    let mut obj = Object::from_brep(brep);
    if next_face_id < obj.next_face_id {
        return Err(bad(format!("{what}: face ids at or above its counter")));
    }
    obj.next_face_id = next_face_id;
    Ok(obj)
}

fn opt_xform(x: &Xform) -> Option<Xform> {
    (*x != Xform::IDENTITY).then_some(*x)
}

fn parse_hex(c: &str) -> Option<[u8; 3]> {
    let h = c.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
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
        let def_index: BTreeMap<DefId, usize> =
            self.defs.keys().enumerate().map(|(i, k)| (k, i)).collect();
        Snapshot {
            sketches: self
                .sketch_order
                .iter()
                .map(|&k| {
                    let s = &self.sketches[k];
                    SnapSketch {
                        plane: s.plane().into(),
                        graph: s.to_raw(),
                        parent: s.parent.map(to_public),
                    }
                })
                .collect(),
            objects: self
                .objects()
                .into_iter()
                .map(|(id, o)| SnapObject {
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
                    brep: o.member.slot.is_none().then(|| snap_brep(o)),
                    parent: o.member.parent.map(to_public),
                    slot: o.member.slot,
                    hidden: o.member.hidden,
                    locked: o.member.locked,
                    part: o.part.clone(),
                })
                .collect(),
            meshes: self
                .meshes()
                .into_iter()
                .map(|(id, m)| {
                    let derived = m.member.slot.is_some();
                    SnapMesh {
                        id,
                        name: m.name.clone(),
                        tag: m.tag.clone(),
                        material: m.material.clone(),
                        note: m.note.clone(),
                        positions: if derived {
                            Vec::new()
                        } else {
                            m.positions.iter().map(|p| p.to_array()).collect()
                        },
                        faces: if derived { Vec::new() } else { m.faces.clone() },
                        lines: if derived { Vec::new() } else { m.lines.clone() },
                        parent: m.member.parent.map(to_public),
                        slot: m.member.slot,
                        hidden: m.member.hidden,
                        locked: m.member.locked,
                    }
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
            nodes: self.snap_nodes(&def_index),
            definitions: self.snap_definitions(&def_index),
            context: self.context.map(to_public),
            tags: self.tags().clone(),
            tag_colors: self
                .tag_colors
                .iter()
                .map(|(t, c)| (t.clone(), crate::nodes::hex_color(*c)))
                .collect(),
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

    /// Nodes in id order (a reader resolves parents after reading them all), so a document
    /// whose ids are compact reopens with the same ids.
    fn snap_nodes(&self, def_index: &BTreeMap<DefId, usize>) -> Vec<SnapNode> {
        self.nodes
            .keys()
            .map(|k| {
                let n = &self.nodes[k];
                SnapNode {
                    id: to_public(k),
                    kind: match n.kind {
                        NodeKind::Group => SnapNodeKind::Group,
                        NodeKind::Instance(_) => SnapNodeKind::Instance,
                    },
                    definition: match n.kind {
                        NodeKind::Instance(d) => def_index.get(&d).copied(),
                        NodeKind::Group => None,
                    },
                    parent: n.member.parent.map(to_public),
                    name: n.name.clone(),
                    tag: n.tag.clone(),
                    hidden: n.member.hidden,
                    locked: n.member.locked,
                    slot: n.member.slot,
                    xform: opt_xform(&n.xform),
                    part: n.part.clone(),
                }
            })
            .collect()
    }

    fn snap_definitions(&self, def_index: &BTreeMap<DefId, usize>) -> Vec<SnapDefinition> {
        self.defs
            .values()
            .map(|d| SnapDefinition {
                name: d.name.clone(),
                next_slot: d.next_slot,
                part: d.part.clone(),
                members: d
                    .members
                    .iter()
                    .map(|(&slot, m)| SnapMember {
                        slot,
                        parent: m.parent,
                        name: m.name.clone(),
                        tag: m.tag.clone(),
                        hidden: m.hidden,
                        locked: m.locked,
                        part: match &m.kind {
                            MemberKind::Solid(o) => o.part.clone(),
                            _ => m.part.clone(),
                        },
                        kind: match &m.kind {
                            MemberKind::Solid(o) => SnapMemberKind::Solid {
                                material: o.material.clone(),
                                next_face_id: o.next_face_id,
                                brep: snap_brep(o),
                            },
                            MemberKind::Mesh(x) => SnapMemberKind::Mesh {
                                material: x.material.clone(),
                                note: x.note.clone(),
                                positions: x.positions.iter().map(|p| p.to_array()).collect(),
                                faces: x.faces.clone(),
                                lines: x.lines.clone(),
                            },
                            MemberKind::Group(x) => SnapMemberKind::Group {
                                xform: opt_xform(x),
                            },
                            MemberKind::Instance(e, x) => SnapMemberKind::Instance {
                                definition: def_index[e],
                                xform: opt_xform(x),
                            },
                        },
                    })
                    .collect(),
            })
            .collect()
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
        for (t, c) in &s.tag_colors {
            let c = parse_hex(c).ok_or_else(|| bad(format!("tag {t:?}: colour {c:?}")))?;
            m.tag_colors.insert(t.clone(), c);
        }
        // Definitions: keys first (members refer to each other by index), then members.
        let def_ids: Vec<DefId> = s
            .definitions
            .iter()
            .map(|d| {
                m.defs.insert(Definition {
                    name: d.name.clone(),
                    members: BTreeMap::new(),
                    next_slot: d.next_slot,
                    part: d.part.clone(),
                })
            })
            .collect();
        for (di, d) in s.definitions.iter().enumerate() {
            let mut members = BTreeMap::new();
            for mem in &d.members {
                let what = format!("definition {:?} member {}", d.name, mem.slot);
                let kind = match &mem.kind {
                    SnapMemberKind::Solid {
                        material,
                        next_face_id,
                        brep,
                    } => {
                        let mut o = brep_object(&what, brep, *next_face_id)?;
                        o.material = material.clone();
                        o.part = mem.part.clone();
                        MemberKind::Solid(Box::new(o))
                    }
                    SnapMemberKind::Mesh {
                        material,
                        note,
                        positions,
                        faces,
                        lines,
                    } => MemberKind::Mesh(Box::new(LooseMesh {
                        name: mem.name.clone().unwrap_or_default(),
                        positions: positions.iter().map(|p| DVec3::from_array(*p)).collect(),
                        faces: faces.clone(),
                        lines: lines.clone(),
                        tag: None,
                        material: material.clone(),
                        note: note.clone(),
                        member: Membership::default(),
                    })),
                    SnapMemberKind::Group { xform } => {
                        MemberKind::Group(xform.unwrap_or(Xform::IDENTITY))
                    }
                    SnapMemberKind::Instance { definition, xform } => MemberKind::Instance(
                        *def_ids
                            .get(*definition)
                            .ok_or_else(|| bad(format!("{what}: no definition {definition}")))?,
                        xform.unwrap_or(Xform::IDENTITY),
                    ),
                };
                if members
                    .insert(
                        mem.slot,
                        Member {
                            parent: mem.parent,
                            name: mem.name.clone(),
                            tag: mem.tag.clone(),
                            hidden: mem.hidden,
                            locked: mem.locked,
                            part: if matches!(kind, MemberKind::Solid(_)) {
                                PartAttrs::default()
                            } else {
                                mem.part.clone()
                            },
                            kind,
                        },
                    )
                    .is_some()
                {
                    return Err(bad(format!("{what}: slot repeated")));
                }
            }
            m.defs[def_ids[di]].members = members;
        }
        // Nodes (parents first), then their parents by saved id.
        let mut node_ids: BTreeMap<u64, NodeId> = BTreeMap::new();
        for n in &s.nodes {
            let kind = match (n.kind, n.definition) {
                (SnapNodeKind::Group, _) => NodeKind::Group,
                (SnapNodeKind::Instance, Some(d)) => NodeKind::Instance(
                    *def_ids
                        .get(d)
                        .ok_or_else(|| bad(format!("node {}: no definition {d}", n.id)))?,
                ),
                (SnapNodeKind::Instance, None) => {
                    return Err(bad(format!(
                        "node {}: an instance needs a definition",
                        n.id
                    )));
                }
            };
            let k = m.nodes.insert(Node {
                name: n.name.clone(),
                kind,
                xform: n.xform.unwrap_or(Xform::IDENTITY),
                tag: n.tag.clone(),
                member: Membership {
                    parent: None,
                    slot: n.slot,
                    hidden: n.hidden,
                    locked: n.locked,
                },
                part: n.part.clone(),
            });
            if node_ids.insert(n.id, k).is_some() {
                return Err(bad(format!("node {} listed twice", n.id)));
            }
        }
        let node_of = |id: Option<u64>, what: &str| -> Result<Option<NodeId>, KernelError> {
            match id {
                None => Ok(None),
                Some(p) => node_ids
                    .get(&p)
                    .copied()
                    .map(Some)
                    .ok_or_else(|| bad(format!("{what}: no node {p}"))),
            }
        };
        for n in &s.nodes {
            let p = node_of(n.parent, &format!("node {}", n.id))?;
            m.nodes[node_ids[&n.id]].member.parent = p;
        }
        for (i, sk) in s.sketches.iter().enumerate() {
            m.sketches[sketch_ids[i]].parent = node_of(sk.parent, &format!("sketch {i}"))?;
        }
        m.context = node_of(s.context, "the editing context")?;
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
        // A solid exists when it has a B-rep, or is derived (a component instance's).
        let present = |o: &&SnapObject| o.brep.is_some() || o.slot.is_some();
        let keep_numbers = {
            let mut seen = std::collections::BTreeSet::new();
            s.objects
                .iter()
                .filter(present)
                .all(|o| matches!(o.number, Some(n) if n > 0 && seen.insert(n)))
        };
        if keep_numbers {
            let top = s
                .objects
                .iter()
                .filter(present)
                .filter_map(|o| o.number)
                .max()
                .unwrap_or(0);
            m.last_number = top.max(s.last_number);
        }
        for o in &s.objects {
            let what = format!("object {}", o.id);
            let mut obj = match (&o.brep, o.slot) {
                // Derived: a placeholder until the instance is derived below (same key, so
                // ids stay in saved order).
                (_, Some(_)) => {
                    let mut x = Object::extrude(
                        &Plane::GROUND,
                        &[
                            glam::DVec2::ZERO,
                            glam::DVec2::X,
                            glam::DVec2::ONE,
                            glam::DVec2::Y,
                        ],
                        &[],
                        1.0,
                    )
                    .map_err(|e| bad(format!("{what}: {e}")))?;
                    x.next_face_id = o.next_face_id.max(x.next_face_id);
                    x
                }
                (Some(b), None) => brep_object(&what, b, o.next_face_id)?,
                (None, None) => continue,
            };
            obj.member = Membership {
                parent: node_of(o.parent, &what)?,
                slot: o.slot,
                hidden: o.hidden,
                locked: o.locked,
            };
            obj.name = o.name.clone();
            obj.tag = o.tag.clone();
            obj.material = o.material.clone();
            obj.part = o.part.clone();
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
            let what = format!("mesh {}", mesh.id);
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
                member: Membership {
                    parent: node_of(mesh.parent, &what)?,
                    slot: mesh.slot,
                    hidden: mesh.hidden,
                    locked: mesh.locked,
                },
            });
            m.mesh_order.push(id);
        }
        // Lane-2 files: flat groups of solids (a solid in two groups stays in the first).
        if s.nodes.is_empty() {
            for g in &s.groups {
                let objects: Vec<crate::ids::ObjectId> = g
                    .objects
                    .iter()
                    .filter_map(|o| object_ids.get(o).copied())
                    .map(crate::ids::from_public)
                    .filter(|&k| m.objects[k].member.parent.is_none())
                    .collect();
                if !objects.is_empty() {
                    let k = m.nodes.insert(Node {
                        name: Some(g.name.clone()),
                        kind: NodeKind::Group,
                        xform: Xform::IDENTITY,
                        tag: None,
                        member: Membership::default(),
                        part: PartAttrs::default(),
                    });
                    for o in objects {
                        m.objects[o].member.parent = Some(k);
                    }
                }
            }
        }
        for sc in &s.scenes {
            let mut sc = sc.clone();
            sc.context = node_of(sc.context, &format!("scene {:?}", sc.name))?.map(to_public);
            m.scenes.push(sc);
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
        // Component instances: derive their solids (outermost first; nested ones follow).
        let outer: Vec<NodeId> = m
            .nodes
            .iter()
            .filter(|(_, n)| matches!(n.kind, NodeKind::Instance(_)))
            .filter(|(_, n)| n.member.parent.and_then(|p| m.instance_of(p)).is_none())
            .map(|(k, _)| k)
            .collect();
        let keys = |m: &Model| {
            (
                m.objects.keys().collect::<Vec<_>>(),
                m.meshes.keys().collect::<Vec<_>>(),
                m.nodes.keys().collect::<Vec<_>>(),
            )
        };
        let listed = keys(&m);
        for i in outer {
            m.rederive(i).map_err(|e| bad(e.to_string()))?;
        }
        // Deriving must only fill in geometry: a file whose instances list other members
        // than their definitions have is refused, not repaired.
        if keys(&m) != listed {
            return Err(bad(
                "a component instance's contents do not match its definition",
            ));
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
            let compact = |ids: Vec<u64>| ids.iter().enumerate().all(|(i, &id)| id == i as u64 + 1);
            let ids_compact = compact(d.model().object_ids())
                && compact(d.model().nodes().iter().map(|(id, _)| *id).collect())
                && compact(d.model().definitions().iter().map(|(id, _)| *id).collect());
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
