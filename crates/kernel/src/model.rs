//! The geometric state: sketches and objects, changed only through transactional [`Op`]s.
//!
//! `apply` = clone -> mutate -> validate -> commit (ADR-0001). A refused or failed op
//! leaves the model exactly as it was. In debug builds every B-rep and sketch mutation
//! additionally runs the full validator on the spot, so a kernel bug panics at the mutation
//! that caused it.
//!
//! Ops address sketches, regions and objects by **public id** (see [`crate::ids`]): stable
//! across edits, so the UI can hold on to them. Faces are addressed by ordinal within their
//! object (the deterministic face order; the UI reads it from the view mesh it just drew).
//! An op sequence is plain data: it serialises to JSON for the fuzzer's repro files, the
//! golden replays and the document's op log, and replays identically everywhere.

use glam::{DVec2, DVec3};
use slotmap::SlotMap;

use crate::error::{FaceEditError, KernelError, PushError, SketchError};
use crate::ids::{ObjectId, RegionId, SketchId, from_public, to_public};
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
    /// Push/pull face `face` (ordinal) of object `object` along its outward normal.
    PushPull {
        object: u64,
        face: u32,
        distance: f64,
    },
    /// Cut face `face` of `object` in two along `path` (world points on the face's plane;
    /// ends on the face's boundary, interior strictly inside).
    SplitFace {
        object: u64,
        face: u32,
        path: Vec<[f64; 3]>,
    },
    /// Imprint the closed loop `points` (strictly inside face `face`) as a coplanar
    /// sub-face twinned with a new hole in the face.
    Imprint {
        object: u64,
        face: u32,
        points: Vec<[f64; 3]>,
    },
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
    /// A face was split; the two pieces' ordinals (the first keeps the old ordinal).
    FaceSplit {
        faces: Vec<u32>,
    },
    /// A loop was imprinted; the new sub-face's ordinal.
    Imprinted {
        face: u32,
    },
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
            } => {
                let oid = self.object_key(*object)?;
                let o = &mut self.objects[oid];
                match o.push_pull(*face as usize, *distance)? {
                    PushOutcome::Moved => Ok(OpOutcome::FaceMoved),
                    PushOutcome::Collapsed { footprint } => {
                        let removed = self.objects.remove(oid).expect("object exists");
                        self.object_order.retain(|&k| k != oid);
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
                let o = self.face_edit_target(*object, *face)?;
                let faces = o.split_face(*face as usize, &pts)?;
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
                let o = self.face_edit_target(*object, *face)?;
                let sub = o.imprint(*face as usize, &pts)?;
                Ok(OpOutcome::Imprinted { face: sub })
            }
        }
    }

    fn face_edit_target(&mut self, object: u64, face: u32) -> Result<&mut Object, FaceEditError> {
        let k = from_public::<ObjectId>(object);
        let o = self
            .objects
            .get_mut(k)
            .ok_or(FaceEditError::NoSuchObject(object))?;
        if face as usize >= o.brep().face_count() {
            return Err(FaceEditError::NoSuchFace(face));
        }
        Ok(o)
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

    /// Validate every object (topology + watertight) and every sketch graph.
    pub fn validate(&self) -> Result<(), TopologyError> {
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
                face: 1,
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
                face: 1,
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
