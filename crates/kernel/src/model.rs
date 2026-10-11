//! The geometric state: sketches and objects, changed only through transactional [`Op`]s.
//!
//! `apply` = clone -> mutate -> validate -> commit (ADR-0001). A refused or failed op
//! leaves the model exactly as it was. In debug builds every B-rep and sketch mutation
//! additionally runs the full validator on the spot, so a kernel bug panics at the mutation
//! that caused it.
//!
//! Ops address sketches, regions and objects by **public id** (see [`crate::ids`]) and
//! faces by **persistent face id** within their object (see [`crate::object::Object`]):
//! all stable across edits, so the UI can hold on to them. A face id survives the push
//! that moves the face (topological and boolean paths alike), splits (the first piece
//! keeps it) and coplanar merges (the merged face keeps the smallest member's id).
//! An op sequence is plain data: it serialises to JSON for the fuzzer's repro files, the
//! golden replays and the document's op log, and replays identically everywhere.

use glam::{DVec2, DVec3};
use slotmap::SlotMap;

use std::collections::BTreeMap;

use crate::error::{DocumentError, FaceEditError, KernelError, PushError, SketchError};
use crate::ids::{
    DimensionId, GroupId, GuideId, ObjectId, RegionId, SketchId, from_public, to_public,
};
use crate::object::{Object, Provenance, PushOutcome};
use crate::plane::Plane;
use crate::sketch::Sketch;
use crate::tolerance::PLANARITY;
use crate::validate::{TopologyError, Violation};

/// One undoable edit. Lengths in metres; ids are public ids.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// New empty sketch on the plane through `origin` with `normal`.
    NewSketch { origin: [f64; 3], normal: [f64; 3] },
    /// Rectangle in the sketch's 2D frame: corner (x, y), size (w, h).
    AddRectangle {
        sketch: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// One straight segment in the sketch's 2D frame, inserted with the sticky rules.
    InsertSegment {
        sketch: u64,
        a: [f64; 2],
        b: [f64; 2],
    },
    /// Erase every edge lying along a segment (the Eraser), healing the T-junctions and
    /// crossings it leaves behind.
    EraseSegment {
        sketch: u64,
        a: [f64; 2],
        b: [f64; 2],
    },
    /// A polyline (closed: the last point joins the first), as one transaction.
    AddPolyline {
        sketch: u64,
        points: Vec<[f64; 2]>,
        closed: bool,
    },
    /// Extrude region `region` of sketch `sketch` into a new solid. The region leaves the
    /// sketch; the solid's base face takes its place.
    Extrude {
        sketch: u64,
        region: u64,
        distance: f64,
    },
    /// Push/pull face `face` (persistent face id) of object `object` along its outward
    /// normal.
    PushPull {
        object: u64,
        face: u64,
        distance: f64,
    },
    /// Push/pull that leaves a seam where the face was (Ctrl in the UI): an outward pull
    /// gives every edge of the face a new side wall instead of stretching the old walls.
    /// Inward pushes behave like [`Op::PushPull`].
    PushPullNewFace {
        object: u64,
        face: u64,
        distance: f64,
    },
    /// Cut face `face` of `object` in two along `path` (world points on the face's plane;
    /// ends on the face's boundary, interior strictly inside).
    SplitFace {
        object: u64,
        face: u64,
        path: Vec<[f64; 3]>,
    },
    /// Imprint the closed loop `points` (strictly inside face `face`) as a coplanar
    /// sub-face twinned with a new hole in the face.
    Imprint {
        object: u64,
        face: u64,
        points: Vec<[f64; 3]>,
    },
    /// Draw a stroke (world points; `closed` joins the last to the first) as one undoable
    /// edit: into a sketch, onto a plane (the sketch already on that plane, or a new one),
    /// or onto a solid's face plane (cuts and imprints; see [`crate::draw`]).
    Draw {
        target: DrawTarget,
        points: Vec<[f64; 3]>,
        closed: bool,
    },
    /// Erase the edge of `object` from `a` to `b` (world points at its two ends). Only a
    /// line drawn on a face can go: its two coplanar faces merge (with every other edge they
    /// share). Anything else would open the solid and is refused.
    EraseEdge {
        object: u64,
        a: [f64; 3],
        b: [f64; 3],
    },
    /// Erase face `face` (persistent id) of `object` into a coplanar neighbour across a drawn
    /// line (an imprint, one side of a cut). Refused where it would open the solid.
    EraseFace { object: u64, face: u64 },
    /// Erase a sketch region: its edges that bound no other region leave the sketch.
    EraseRegion { sketch: u64, region: u64 },
    /// Several ops as one transaction and one undo step (no undo, redo or batch inside).
    Batch { ops: Vec<Op> },
    /// Add a guide: an infinite line through `point` along `dir`, or a guide point when
    /// `dir` is absent (the Tape measure). Guides are construction help: inferable, never
    /// part of a solid.
    AddGuide {
        point: [f64; 3],
        dir: Option<[f64; 3]>,
    },
    /// Remove one guide.
    EraseGuide { guide: u64 },
    /// Remove every guide.
    DeleteGuides,
    /// Add a linear dimension between `a` and `b`, its line drawn `offset` away from them.
    AddDimension {
        a: [f64; 3],
        b: [f64; 3],
        offset: [f64; 3],
    },
    /// Remove one dimension.
    EraseDimension { dimension: u64 },
    /// Delete an object (it also leaves any group).
    DeleteObject { object: u64 },
    /// Group objects under a name.
    CreateGroup { name: String, objects: Vec<u64> },
    /// Dissolve a group (its objects stay).
    DeleteGroup { group: u64 },
    /// Put an object on a tag (layer), or take it off (`None`). New tags start visible.
    SetTag { object: u64, tag: Option<String> },
    /// Show or hide every object on a tag.
    SetTagVisible { tag: String, visible: bool },
    /// Step back one op (document command; see [`crate::document::Document`]).
    Undo,
    /// Step forward again after an undo (document command).
    Redo,
}

/// Where a stroke goes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum DrawTarget {
    /// Into this sketch (points must lie on its plane).
    Sketch { sketch: u64 },
    /// Onto this plane: into the sketch already lying on it (either facing), or a new one.
    Plane { origin: [f64; 3], normal: [f64; 3] },
    /// Onto the plane of this face of a solid (persistent face id).
    Face { object: u64, face: u64 },
}

/// What an op did, with the ids the UI needs next.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpOutcome {
    SketchCreated {
        sketch: u64,
    },
    /// A sketch edit. `regions`: the regions it drew (inside a rectangle) or touched.
    SketchEdited {
        regions: Vec<u64>,
    },
    ObjectCreated {
        object: u64,
    },
    FaceMoved,
    /// A face was split; the two pieces' persistent ids (the first keeps the old id).
    FaceSplit {
        faces: Vec<u64>,
    },
    /// A loop was imprinted; the new sub-face's persistent id.
    Imprinted {
        face: u64,
    },
    ObjectDeleted,
    GuideAdded {
        guide: u64,
    },
    DimensionAdded {
        dimension: u64,
    },
    AnnotationErased,
    /// A stroke was drawn: into `sketch` (the regions it bounds), or on a solid (the
    /// persistent ids of the faces it cut or made).
    Drawn {
        sketch: Option<u64>,
        regions: Vec<u64>,
        faces: Vec<u64>,
        /// A closed stroke: the region or face it now encloses (what Push/Pull acts on next).
        inside: Option<u64>,
    },
    /// An edge or face was erased; the merged face's persistent id.
    FacesMerged {
        face: u64,
    },
    /// What each op of a batch did, in order.
    Batch {
        outcomes: Vec<OpOutcome>,
    },
    GroupCreated {
        group: u64,
    },
    GroupDeleted,
    TagSet,
    Undone,
    Redone,
    /// The pushed solid flattened to nothing and was removed. When the flat footprint lay
    /// on the solid's source sketch it went back there as a region.
    ObjectCollapsed {
        regions: Vec<u64>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    sketches: SlotMap<SketchId, Sketch>,
    sketch_order: Vec<SketchId>,
    objects: SlotMap<ObjectId, Object>,
    object_order: Vec<ObjectId>,
    groups: SlotMap<GroupId, Group>,
    /// Tag name -> visible. Every tag an object uses is listed here.
    tags: BTreeMap<String, bool>,
    guides: SlotMap<GuideId, Guide>,
    dimensions: SlotMap<DimensionId, Dimension>,
}

/// A guide line (unit `dir`) or a guide point (`dir` zero).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub point: DVec3,
    pub dir: DVec3,
}

/// A linear dimension: the measured points and where its line is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension {
    pub a: DVec3,
    pub b: DVec3,
    pub offset: DVec3,
}

impl Dimension {
    /// The measured length, metres.
    pub fn length(&self) -> f64 {
        self.a.distance(self.b)
    }
}

fn finite3(p: &[f64; 3]) -> Option<DVec3> {
    let v = DVec3::from_array(*p);
    (v.is_finite() && v.abs().max_element() <= crate::tolerance::MAX_COORD).then_some(v)
}

/// A named set of objects (by public id).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub objects: Vec<u64>,
}

fn check_name(n: &str) -> Result<(), DocumentError> {
    if n.trim().is_empty() || n.chars().count() > 64 {
        Err(DocumentError::BadName)
    } else {
        Ok(())
    }
}

/// Counts for the UI and tests.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct ModelStats {
    pub sketches: usize,
    pub regions: usize,
    pub objects: usize,
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub holes: usize,
    pub shells: usize,
    /// Every object watertight (vacuously true with no objects).
    pub watertight: bool,
    pub euler: i64,
    pub genus: i64,
    pub volume_m3: f64,
    pub hash: String,
}

impl Model {
    pub fn new() -> Model {
        Model::default()
    }

    /// A model with one sketch on the ground plane (public id 1), as the UI starts.
    pub fn with_ground_sketch() -> Model {
        let mut m = Model::new();
        m.apply(&Op::NewSketch {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        })
        .expect("ground sketch");
        m
    }

    /// Sketches in creation order, with their public ids.
    pub fn sketches(&self) -> Vec<(u64, &Sketch)> {
        self.sketch_order
            .iter()
            .map(|&k| (to_public(k), &self.sketches[k]))
            .collect()
    }

    /// Objects in creation order, with their public ids.
    pub fn objects(&self) -> Vec<(u64, &Object)> {
        self.object_order
            .iter()
            .map(|&k| (to_public(k), &self.objects[k]))
            .collect()
    }

    pub fn sketch(&self, id: u64) -> Option<&Sketch> {
        self.sketches.get(from_public::<SketchId>(id))
    }

    pub fn object(&self, id: u64) -> Option<&Object> {
        self.objects.get(from_public::<ObjectId>(id))
    }

    /// Public ids of the objects, in creation order.
    pub fn object_ids(&self) -> Vec<u64> {
        self.object_order.iter().map(|&k| to_public(k)).collect()
    }

    /// Guides with their public ids, in id order.
    pub fn guides(&self) -> Vec<(u64, &Guide)> {
        self.guides.iter().map(|(k, g)| (to_public(k), g)).collect()
    }

    /// Dimensions with their public ids, in id order.
    pub fn dimensions(&self) -> Vec<(u64, &Dimension)> {
        self.dimensions
            .iter()
            .map(|(k, d)| (to_public(k), d))
            .collect()
    }

    /// Groups with their public ids, in id order.
    pub fn groups(&self) -> Vec<(u64, &Group)> {
        self.groups.iter().map(|(k, g)| (to_public(k), g)).collect()
    }

    /// Tags and their visibility, by name.
    pub fn tags(&self) -> &BTreeMap<String, bool> {
        &self.tags
    }

    /// True when the object is shown (untagged, or on a visible tag).
    pub fn is_visible(&self, object: u64) -> bool {
        self.object(object).is_some_and(|o| {
            o.tag()
                .is_none_or(|t| self.tags.get(t).copied().unwrap_or(true))
        })
    }

    /// Public ids of the sketches, in creation order.
    pub fn sketch_ids(&self) -> Vec<u64> {
        self.sketch_order.iter().map(|&k| to_public(k)).collect()
    }

    /// Apply an op transactionally.
    pub fn apply(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        let mut next = self.clone();
        let outcome = next.apply_in_place(op)?;
        next.validate()?;
        *self = next;
        Ok(outcome)
    }

    /// Replay a sequence from an empty model, stopping at the first error.
    pub fn replay(ops: &[Op]) -> Result<Model, (usize, KernelError)> {
        let mut m = Model::new();
        for (i, op) in ops.iter().enumerate() {
            m.apply(op).map_err(|e| (i, e))?;
        }
        Ok(m)
    }

    fn sketch_key(&self, id: u64) -> Result<SketchId, SketchError> {
        let k = from_public::<SketchId>(id);
        if self.sketches.contains_key(k) {
            Ok(k)
        } else {
            Err(SketchError::NoSuchSketch(id))
        }
    }

    fn object_key(&self, id: u64) -> Result<ObjectId, PushError> {
        let k = from_public::<ObjectId>(id);
        if self.objects.contains_key(k) {
            Ok(k)
        } else {
            Err(PushError::NoSuchObject(id))
        }
    }

    fn edited(regions: Vec<RegionId>) -> OpOutcome {
        OpOutcome::SketchEdited {
            regions: regions.into_iter().map(to_public).collect(),
        }
    }

    fn apply_in_place(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        match op {
            Op::NewSketch { origin, normal } => {
                let plane = Plane::new(DVec3::from_array(*origin), DVec3::from_array(*normal))
                    .map_err(SketchError::from)?;
                let id = self.sketches.insert(Sketch::new(plane));
                self.sketch_order.push(id);
                Ok(OpOutcome::SketchCreated {
                    sketch: to_public(id),
                })
            }
            Op::AddRectangle { sketch, x, y, w, h } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid].add_rectangle(DVec2::new(*x, *y), DVec2::new(*w, *h))?;
                Ok(Self::edited(r))
            }
            Op::InsertSegment { sketch, a, b } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid]
                    .insert_segment(DVec2::from_array(*a), DVec2::from_array(*b))?;
                Ok(Self::edited(r))
            }
            Op::EraseSegment { sketch, a, b } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid]
                    .erase_segment(DVec2::from_array(*a), DVec2::from_array(*b))?;
                Ok(Self::edited(r))
            }
            Op::AddPolyline {
                sketch,
                points,
                closed,
            } => {
                let sid = self.sketch_key(*sketch)?;
                let pts: Vec<DVec2> = points.iter().map(|p| DVec2::from_array(*p)).collect();
                let r = self.sketches[sid].insert_polyline(&pts, *closed)?;
                Ok(Self::edited(r))
            }
            Op::Extrude {
                sketch,
                region,
                distance,
            } => {
                let sid = self.sketch_key(*sketch)?;
                let rid = from_public::<RegionId>(*region);
                let s = &self.sketches[sid];
                let r = s.region(rid).ok_or(SketchError::NoSuchRegion(*region))?;
                if r.consumed {
                    return Err(SketchError::RegionConsumed(*region).into());
                }
                if r.pinched {
                    return Err(SketchError::PinchedRegion(*region).into());
                }
                let (outer, holes) = s.region_rings(r);
                let mut obj = Object::extrude(s.plane(), &outer, &holes, *distance)?;
                obj.source = Some(Provenance { sketch: sid });
                self.sketches[sid].consume_region(rid)?;
                let id = self.objects.insert(obj);
                self.object_order.push(id);
                Ok(OpOutcome::ObjectCreated {
                    object: to_public(id),
                })
            }
            Op::PushPull {
                object,
                face,
                distance,
            }
            | Op::PushPullNewFace {
                object,
                face,
                distance,
            } => {
                let oid = self.object_key(*object)?;
                let o = &mut self.objects[oid];
                let pushed = match op {
                    Op::PushPullNewFace { .. } => o.push_pull_new_face(*face, *distance)?,
                    _ => o.push_pull_id(*face, *distance)?,
                };
                match pushed {
                    PushOutcome::Moved => Ok(OpOutcome::FaceMoved),
                    PushOutcome::Collapsed { footprint } => {
                        let removed = self.objects.remove(oid).expect("object exists");
                        self.object_order.retain(|&k| k != oid);
                        self.forget_object(*object);
                        let regions = match removed.source {
                            Some(src) => self.return_footprint(src.sketch, &footprint)?,
                            None => Vec::new(),
                        };
                        Ok(OpOutcome::ObjectCollapsed {
                            regions: regions.into_iter().map(to_public).collect(),
                        })
                    }
                }
            }
            Op::SplitFace { object, face, path } => {
                let pts: Vec<DVec3> = path.iter().map(|p| DVec3::from_array(*p)).collect();
                let (o, fi) = self.face_edit_target(*object, *face)?;
                let faces = o.split_face(fi, &pts)?;
                Ok(OpOutcome::FaceSplit {
                    faces: faces.to_vec(),
                })
            }
            Op::Imprint {
                object,
                face,
                points,
            } => {
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from_array(*p)).collect();
                let (o, fi) = self.face_edit_target(*object, *face)?;
                let sub = o.imprint(fi, &pts)?;
                Ok(OpOutcome::Imprinted { face: sub })
            }
            Op::Draw {
                target,
                points,
                closed,
            } => {
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from_array(*p)).collect();
                match target {
                    DrawTarget::Face { object, face } => {
                        let (o, fi) = self.face_edit_target(*object, *face)?;
                        let (faces, inside) = o.draw(fi, &pts, *closed)?;
                        Ok(OpOutcome::Drawn {
                            sketch: None,
                            regions: Vec::new(),
                            faces,
                            inside,
                        })
                    }
                    DrawTarget::Sketch { sketch } => {
                        let sid = self.sketch_key(*sketch)?;
                        let regions = self.draw_in_sketch(sid, &pts, *closed)?;
                        Ok(OpOutcome::Drawn {
                            sketch: Some(*sketch),
                            regions: regions.into_iter().map(to_public).collect(),
                            faces: Vec::new(),
                            inside: self.region_inside(sid, &pts, *closed),
                        })
                    }
                    DrawTarget::Plane { origin, normal } => {
                        let plane =
                            Plane::new(DVec3::from_array(*origin), DVec3::from_array(*normal))
                                .map_err(SketchError::from)?;
                        let sid = self.sketch_on(&plane);
                        let regions = self.draw_in_sketch(sid, &pts, *closed)?;
                        Ok(OpOutcome::Drawn {
                            sketch: Some(to_public(sid)),
                            regions: regions.into_iter().map(to_public).collect(),
                            faces: Vec::new(),
                            inside: self.region_inside(sid, &pts, *closed),
                        })
                    }
                }
            }
            Op::EraseEdge { object, a, b } => {
                let o = self
                    .objects
                    .get_mut(from_public::<ObjectId>(*object))
                    .ok_or(FaceEditError::NoSuchObject(*object))?;
                let face = o.erase_edge(DVec3::from_array(*a), DVec3::from_array(*b))?;
                Ok(OpOutcome::FacesMerged { face })
            }
            Op::EraseFace { object, face } => {
                let (o, fi) = self.face_edit_target(*object, *face)?;
                let face = o.erase_face(fi)?;
                Ok(OpOutcome::FacesMerged { face })
            }
            Op::EraseRegion { sketch, region } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid].erase_region(from_public::<RegionId>(*region))?;
                Ok(Self::edited(r))
            }
            Op::Batch { ops } => {
                if ops.is_empty() {
                    return Err(DocumentError::EmptyBatch.into());
                }
                let mut outcomes = Vec::with_capacity(ops.len());
                for op in ops {
                    if matches!(op, Op::Undo | Op::Redo | Op::Batch { .. }) {
                        return Err(DocumentError::NotInBatch.into());
                    }
                    outcomes.push(self.apply_in_place(op)?);
                }
                Ok(OpOutcome::Batch { outcomes })
            }
            Op::AddGuide { point, dir } => {
                let p = finite3(point).ok_or(DocumentError::BadAnnotation)?;
                let d = match dir {
                    Some(d) => {
                        let v = finite3(d).ok_or(DocumentError::BadAnnotation)?;
                        if v.length() < 1e-12 {
                            return Err(DocumentError::BadAnnotation.into());
                        }
                        v.normalize()
                    }
                    None => DVec3::ZERO,
                };
                let id = self.guides.insert(Guide { point: p, dir: d });
                Ok(OpOutcome::GuideAdded {
                    guide: to_public(id),
                })
            }
            Op::EraseGuide { guide } => {
                self.guides
                    .remove(from_public::<GuideId>(*guide))
                    .ok_or(DocumentError::NoSuchAnnotation(*guide))?;
                Ok(OpOutcome::AnnotationErased)
            }
            Op::DeleteGuides => {
                if self.guides.is_empty() {
                    return Err(DocumentError::NoGuides.into());
                }
                self.guides.clear();
                Ok(OpOutcome::AnnotationErased)
            }
            Op::AddDimension { a, b, offset } => {
                let (a, b, o) = (
                    finite3(a).ok_or(DocumentError::BadAnnotation)?,
                    finite3(b).ok_or(DocumentError::BadAnnotation)?,
                    finite3(offset).ok_or(DocumentError::BadAnnotation)?,
                );
                if a.distance(b) <= crate::tolerance::POINT_MERGE {
                    return Err(DocumentError::BadAnnotation.into());
                }
                let id = self.dimensions.insert(Dimension { a, b, offset: o });
                Ok(OpOutcome::DimensionAdded {
                    dimension: to_public(id),
                })
            }
            Op::EraseDimension { dimension } => {
                self.dimensions
                    .remove(from_public::<DimensionId>(*dimension))
                    .ok_or(DocumentError::NoSuchAnnotation(*dimension))?;
                Ok(OpOutcome::AnnotationErased)
            }
            Op::DeleteObject { object } => {
                let k = from_public::<ObjectId>(*object);
                if self.objects.remove(k).is_none() {
                    return Err(DocumentError::NoSuchObject(*object).into());
                }
                self.object_order.retain(|&o| o != k);
                self.forget_object(*object);
                Ok(OpOutcome::ObjectDeleted)
            }
            Op::CreateGroup { name, objects } => {
                check_name(name)?;
                if objects.is_empty() {
                    return Err(DocumentError::EmptyGroup.into());
                }
                let mut members: Vec<u64> = Vec::new();
                for &o in objects {
                    if self.object(o).is_none() {
                        return Err(DocumentError::NoSuchObject(o).into());
                    }
                    if !members.contains(&o) {
                        members.push(o);
                    }
                }
                let g = self.groups.insert(Group {
                    name: name.clone(),
                    objects: members,
                });
                Ok(OpOutcome::GroupCreated {
                    group: to_public(g),
                })
            }
            Op::DeleteGroup { group } => {
                if self.groups.remove(from_public::<GroupId>(*group)).is_none() {
                    return Err(DocumentError::NoSuchGroup(*group).into());
                }
                Ok(OpOutcome::GroupDeleted)
            }
            Op::SetTag { object, tag } => {
                if let Some(t) = tag {
                    check_name(t)?;
                }
                let o = self
                    .objects
                    .get_mut(from_public::<ObjectId>(*object))
                    .ok_or(DocumentError::NoSuchObject(*object))?;
                o.tag = tag.clone();
                if let Some(t) = tag {
                    self.tags.entry(t.clone()).or_insert(true);
                }
                Ok(OpOutcome::TagSet)
            }
            Op::SetTagVisible { tag, visible } => {
                check_name(tag)?;
                self.tags.insert(tag.clone(), *visible);
                Ok(OpOutcome::TagSet)
            }
            Op::Undo | Op::Redo => Err(DocumentError::NotAModelOp.into()),
        }
    }

    /// The sketch lying on `plane` (either facing, within the planarity tolerance), or a
    /// new one on it.
    fn sketch_on(&mut self, plane: &Plane) -> SketchId {
        let found = self.sketch_order.iter().copied().find(|&k| {
            let sp = self.sketches[k].plane();
            sp.normal.dot(plane.normal).abs() >= 1.0 - 1e-12
                && sp.signed_distance(plane.origin).abs() <= PLANARITY
        });
        found.unwrap_or_else(|| {
            let id = self.sketches.insert(Sketch::new(*plane));
            self.sketch_order.push(id);
            id
        })
    }

    /// The region a closed stroke encloses (holding an interior point of it), if any.
    fn region_inside(&self, sid: SketchId, pts: &[DVec3], closed: bool) -> Option<u64> {
        if !closed {
            return None;
        }
        let s = &self.sketches[sid];
        let ring: Vec<DVec2> = pts.iter().map(|&p| s.plane().to_local(p)).collect();
        let q = sawhorse_tessellate::interior_point(&ring, &[])?;
        s.region_at(q).map(to_public)
    }

    /// Insert a world-space stroke into a sketch (every point on its plane).
    fn draw_in_sketch(
        &mut self,
        sid: SketchId,
        pts: &[DVec3],
        closed: bool,
    ) -> Result<Vec<RegionId>, SketchError> {
        let s = &mut self.sketches[sid];
        let plane = *s.plane();
        let mut local = Vec::with_capacity(pts.len());
        for p in pts {
            if !p.is_finite() {
                return Err(SketchError::OutOfBounds);
            }
            if plane.signed_distance(*p).abs() > PLANARITY {
                return Err(SketchError::OffPlane);
            }
            local.push(plane.to_local(*p));
        }
        if local.len() < 2 {
            return Err(SketchError::Degenerate);
        }
        s.insert_polyline(&local, closed)
    }

    /// Drop a removed object from every group (and empty groups with it).
    fn forget_object(&mut self, object: u64) {
        for g in self.groups.values_mut() {
            g.objects.retain(|&o| o != object);
        }
        self.groups.retain(|_, g| !g.objects.is_empty());
    }

    /// The object and the ordinal of its face with persistent id `face`.
    fn face_edit_target(
        &mut self,
        object: u64,
        face: u64,
    ) -> Result<(&mut Object, usize), FaceEditError> {
        let k = from_public::<ObjectId>(object);
        let o = self
            .objects
            .get_mut(k)
            .ok_or(FaceEditError::NoSuchObject(object))?;
        let fi = o
            .face_ordinal(face)
            .ok_or(FaceEditError::NoSuchFace(face))?;
        Ok((o, fi))
    }

    /// A collapsed solid's flat footprint goes back to its source sketch when it lies on
    /// that sketch's plane (the usual case: a box pushed back down onto its base).
    fn return_footprint(
        &mut self,
        sketch: SketchId,
        rings: &[Vec<DVec3>],
    ) -> Result<Vec<RegionId>, SketchError> {
        let Some(s) = self.sketches.get_mut(sketch) else {
            return Ok(Vec::new());
        };
        let plane = *s.plane();
        if rings
            .iter()
            .flatten()
            .any(|p| plane.signed_distance(*p).abs() > PLANARITY)
        {
            return Ok(Vec::new());
        }
        let rings2: Vec<Vec<DVec2>> = rings
            .iter()
            .map(|ring| ring.iter().map(|&p| plane.to_local(p)).collect())
            .collect();
        s.release_footprint(&rings2)
    }

    /// Validate every object (topology + watertight), every sketch graph, and the
    /// document bookkeeping (groups name live objects, every object tag is listed).
    pub fn validate(&self) -> Result<(), TopologyError> {
        let dangling = |what: &'static str| TopologyError {
            violations: vec![Violation::DanglingKey(what)],
            report: Default::default(),
        };
        for g in self.groups.values() {
            if g.objects.is_empty() || g.objects.iter().any(|&o| self.object(o).is_none()) {
                return Err(dangling("group names a missing object"));
            }
        }
        for (_, o) in self.objects() {
            if o.tag().is_some_and(|t| !self.tags.contains_key(t)) {
                return Err(dangling("object tag is not listed"));
            }
        }
        for (_, o) in self.objects() {
            o.validate()?;
        }
        for (_, s) in self.sketches() {
            if let Err(e) = s.check() {
                return Err(TopologyError {
                    violations: vec![Violation::SketchInvalid(e)],
                    report: Default::default(),
                });
            }
        }
        Ok(())
    }

    pub fn hash(&self) -> u64 {
        crate::hash::model_hash(self)
    }

    pub fn hash_hex(&self) -> String {
        format!("{:016x}", self.hash())
    }

    pub fn stats(&self) -> ModelStats {
        let mut s = ModelStats {
            sketches: self.sketch_order.len(),
            regions: self.sketches().iter().map(|(_, s)| s.region_count()).sum(),
            objects: self.object_order.len(),
            watertight: true,
            hash: self.hash_hex(),
            ..Default::default()
        };
        for (_, o) in self.objects() {
            match o.validate() {
                Ok(r) => {
                    s.faces += r.faces;
                    s.edges += r.edges;
                    s.vertices += r.vertices;
                    s.holes += r.holes;
                    s.shells += r.shells.len();
                    s.watertight &= r.watertight;
                    for sh in &r.shells {
                        s.euler += sh.euler;
                        s.genus += sh.genus.unwrap_or(0);
                    }
                }
                Err(_) => s.watertight = false,
            }
            s.volume_m3 += o.volume();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND: u64 = 1;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Op {
        Op::AddRectangle {
            sketch: GROUND,
            x,
            y,
            w,
            h,
        }
    }

    fn first_region(m: &Model) -> u64 {
        to_public(m.sketch(GROUND).unwrap().regions().next().unwrap().0)
    }

    #[test]
    fn spike_box() {
        let mut m = Model::with_ground_sketch();
        let r = m.apply(&rect(0.0, 0.0, 0.6096, 0.4064)).unwrap();
        assert_eq!(r, OpOutcome::SketchEdited { regions: vec![1] });
        assert_eq!(
            m.apply(&Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.3048
            })
            .unwrap(),
            OpOutcome::ObjectCreated { object: 1 }
        );
        let st = m.stats();
        assert!(st.watertight);
        assert_eq!(st.regions, 0, "the extruded region left the sketch");
        assert_eq!(
            (st.faces, st.edges, st.vertices, st.euler, st.genus),
            (6, 12, 8, 2, 0)
        );
        assert!((st.volume_m3 - 0.6096 * 0.4064 * 0.3048).abs() < 1e-12);
    }

    #[test]
    fn refused_ops_roll_back() {
        let mut m = Model::with_ground_sketch();
        m.apply(&rect(0.0, 0.0, 1.0, 1.0)).unwrap();
        let before = m.hash();
        for op in [
            Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.0,
            },
            Op::Extrude {
                sketch: GROUND,
                region: 7,
                distance: 1.0,
            },
            Op::Extrude {
                sketch: 3,
                region: 1,
                distance: 1.0,
            },
            Op::PushPull {
                object: 1,
                face: 0,
                distance: 1.0,
            },
            rect(0.5, 0.5, 0.0, 1.0),
            Op::InsertSegment {
                sketch: GROUND,
                a: [0.0, 0.0],
                b: [f64::NAN, 1.0],
            },
            Op::NewSketch {
                origin: [0.0; 3],
                normal: [0.0; 3],
            },
        ] {
            assert!(m.apply(&op).is_err(), "{op:?} should be refused");
            assert_eq!(m.hash(), before, "{op:?} must not change the model");
        }
    }

    #[test]
    fn extrude_then_push_back_restores_the_sketch() {
        let mut m = Model::with_ground_sketch();
        m.apply(&rect(0.0, 0.0, 1.0, 2.0)).unwrap();
        let sketch_only = m.hash();
        m.apply(&Op::Extrude {
            sketch: GROUND,
            region: 1,
            distance: 0.5,
        })
        .unwrap();
        assert!(
            m.apply(&Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.5
            })
            .is_err(),
            "region already consumed"
        );
        let out = m
            .apply(&Op::PushPull {
                object: 1,
                face: 2,
                distance: -0.5,
            })
            .unwrap();
        assert!(matches!(out, OpOutcome::ObjectCollapsed { ref regions } if regions.len() == 1));
        assert_eq!(m.hash(), sketch_only);
        assert!(m.objects().is_empty());
    }

    #[test]
    fn crossing_rectangles_extrude_one_piece() {
        let mut m = Model::with_ground_sketch();
        m.apply(&rect(0.0, 0.0, 2.0, 2.0)).unwrap();
        let out = m.apply(&rect(1.0, 1.0, 2.0, 2.0)).unwrap();
        let OpOutcome::SketchEdited { regions } = out else {
            panic!("{out:?}")
        };
        assert_eq!(regions.len(), 2);
        assert_eq!(m.stats().regions, 3);
        // Extrude the overlap square (area 1).
        let s = m.sketch(GROUND).unwrap();
        let overlap = s
            .regions()
            .find(|(_, r)| (s.region_area(r) - 1.0).abs() < 1e-12)
            .map(|(id, _)| to_public(id))
            .unwrap();
        m.apply(&Op::Extrude {
            sketch: GROUND,
            region: overlap,
            distance: 1.0,
        })
        .unwrap();
        let st = m.stats();
        assert_eq!(st.regions, 2, "the L-shaped neighbours stay");
        assert!((st.volume_m3 - 1.0).abs() < 1e-12);
        assert!(
            m.sketch(GROUND)
                .unwrap()
                .region(from_public(first_region(&m)))
                .is_some()
        );
    }

    #[test]
    fn ops_round_trip_through_json() {
        let ops = vec![
            Op::NewSketch {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
            rect(0.0, 0.0, 1.0, 1.0),
            Op::InsertSegment {
                sketch: GROUND,
                a: [0.5, 0.0],
                b: [0.5, 1.0],
            },
            Op::AddPolyline {
                sketch: GROUND,
                points: vec![[2.0, 0.0], [3.0, 0.0], [2.5, 1.0]],
                closed: true,
            },
            Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.25,
            },
            Op::PushPull {
                object: 1,
                face: 2,
                distance: 0.1,
            },
        ];
        let json = serde_json::to_string(&ops).unwrap();
        assert!(json.contains(r#""op":"add_rectangle""#));
        assert!(json.contains(r#""op":"insert_segment""#));
        let back: Vec<Op> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ops);
        let m = Model::replay(&ops).unwrap();
        assert_eq!(m.hash(), Model::replay(&back).unwrap().hash());
        assert_eq!(
            m.stats().regions,
            2,
            "one half of the split square and the triangle"
        );
    }
}
