//! The document: sketches and objects, changed only through transactional [`Op`]s.
//!
//! `apply` = clone -> mutate -> validate -> commit (ADR-0001). A refused or failed op
//! leaves the model exactly as it was. In debug builds every B-rep mutation additionally
//! runs the full validator on the spot (see `Brep::debug_validate`), so a kernel bug
//! panics at the mutation that caused it.
//!
//! Ops address sketches, regions, objects and faces by ordinal (position in the current
//! deterministic order), so an op sequence is plain data: it serialises to JSON for the
//! fuzzer's repro files and golden replays, and replays identically everywhere.

use glam::{DVec2, DVec3};
use slotmap::SlotMap;

use crate::error::{KernelError, PushError, SketchError};
use crate::ids::{ObjectId, SketchId};
use crate::object::{Object, Provenance, PushOutcome};
use crate::plane::Plane;
use crate::sketch::Sketch;
use crate::validate::{TopologyError, Violation};

/// One undoable edit. Lengths in metres.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// New empty sketch on the plane through `origin` with `normal`.
    NewSketch { origin: [f64; 3], normal: [f64; 3] },
    /// Rectangle in sketch `sketch`'s 2D frame: corner (x, y), size (w, h).
    AddRectangle {
        sketch: u32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// Extrude region `region` of sketch `sketch` into a new solid.
    Extrude {
        sketch: u32,
        region: u32,
        distance: f64,
    },
    /// Push/pull face `face` of object `object` along its outward normal.
    PushPull {
        object: u32,
        face: u32,
        distance: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum OpOutcome {
    SketchCreated(u32),
    RegionAdded(u32),
    ObjectCreated(u32),
    FaceMoved,
    /// The pushed solid flattened to nothing and was removed; its region went back to the sketch.
    ObjectCollapsed,
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

    /// A model with one sketch on the ground plane (sketch 0), as the UI starts.
    pub fn with_ground_sketch() -> Model {
        let mut m = Model::new();
        m.apply(&Op::NewSketch {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        })
        .expect("ground sketch");
        m
    }

    pub fn sketches(&self) -> Vec<&Sketch> {
        self.sketch_order
            .iter()
            .map(|&k| &self.sketches[k])
            .collect()
    }

    pub fn objects(&self) -> Vec<&Object> {
        self.object_order
            .iter()
            .map(|&k| &self.objects[k])
            .collect()
    }

    pub fn sketch(&self, i: u32) -> Option<&Sketch> {
        self.sketch_order
            .get(i as usize)
            .map(|&k| &self.sketches[k])
    }

    pub fn object(&self, i: u32) -> Option<&Object> {
        self.object_order.get(i as usize).map(|&k| &self.objects[k])
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

    fn apply_in_place(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        match *op {
            Op::NewSketch { origin, normal } => {
                let plane = Plane::new(DVec3::from_array(origin), DVec3::from_array(normal))
                    .map_err(SketchError::from)?;
                let id = self.sketches.insert(Sketch::new(plane));
                self.sketch_order.push(id);
                Ok(OpOutcome::SketchCreated(
                    (self.sketch_order.len() - 1) as u32,
                ))
            }
            Op::AddRectangle { sketch, x, y, w, h } => {
                let sid = *self
                    .sketch_order
                    .get(sketch as usize)
                    .ok_or(SketchError::NoSuchSketch(sketch))?;
                let r = self.sketches[sid].add_rectangle(DVec2::new(x, y), DVec2::new(w, h))?;
                Ok(OpOutcome::RegionAdded(r as u32))
            }
            Op::Extrude {
                sketch,
                region,
                distance,
            } => {
                let sid = *self
                    .sketch_order
                    .get(sketch as usize)
                    .ok_or(SketchError::NoSuchSketch(sketch))?;
                let s = &self.sketches[sid];
                let r = s
                    .regions()
                    .get(region as usize)
                    .ok_or(SketchError::NoSuchRegion(region))?
                    .clone();
                if s.is_consumed(&r) {
                    return Err(SketchError::RegionConsumed(region).into());
                }
                let (outer, holes) = s.region_rings(&r);
                let mut obj = Object::extrude(s.plane(), &outer, &holes, distance)?;
                obj.source = Some(Provenance {
                    sketch: sid,
                    region: r.key(),
                });
                self.sketches[sid].mark_consumed(&r);
                let id = self.objects.insert(obj);
                self.object_order.push(id);
                Ok(OpOutcome::ObjectCreated(
                    (self.object_order.len() - 1) as u32,
                ))
            }
            Op::PushPull {
                object,
                face,
                distance,
            } => {
                let oid = *self
                    .object_order
                    .get(object as usize)
                    .ok_or(PushError::NoSuchObject(object))?;
                let o = &mut self.objects[oid];
                let fid = *o
                    .face_ids()
                    .get(face as usize)
                    .ok_or(PushError::NoSuchFace(face))?;
                match o.push_pull(fid, distance)? {
                    PushOutcome::Moved => Ok(OpOutcome::FaceMoved),
                    PushOutcome::Collapsed => {
                        let removed = self.objects.remove(oid).expect("object exists");
                        self.object_order.retain(|&k| k != oid);
                        if let Some(src) = removed.source
                            && let Some(s) = self.sketches.get_mut(src.sketch)
                        {
                            s.unmark_consumed(&src.region);
                        }
                        Ok(OpOutcome::ObjectCollapsed)
                    }
                }
            }
        }
    }

    /// Validate every object (topology + watertight). Sketch invariants are enforced by
    /// construction in `Sketch`.
    pub fn validate(&self) -> Result<(), TopologyError> {
        for o in self.objects() {
            o.validate()?;
        }
        for s in self.sketches() {
            if let Err(e) = s.check() {
                return Err(TopologyError {
                    violations: vec![Violation::SketchInvalid(e)],
                    report: Default::default(),
                });
            }
            for key in s.consumed().keys() {
                if !s.regions().iter().any(|r| &r.key() == key) {
                    return Err(TopologyError {
                        violations: vec![Violation::DanglingKey(
                            "consumed region missing from sketch",
                        )],
                        report: Default::default(),
                    });
                }
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
            regions: self.sketches().iter().map(|s| s.regions().len()).sum(),
            objects: self.object_order.len(),
            watertight: true,
            hash: self.hash_hex(),
            ..Default::default()
        };
        for o in self.objects() {
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

    #[test]
    fn spike_box() {
        let mut m = Model::with_ground_sketch();
        let r = m
            .apply(&Op::AddRectangle {
                sketch: 0,
                x: 0.0,
                y: 0.0,
                w: 0.6096,
                h: 0.4064,
            })
            .unwrap();
        assert_eq!(r, OpOutcome::RegionAdded(0));
        assert_eq!(
            m.apply(&Op::Extrude {
                sketch: 0,
                region: 0,
                distance: 0.3048
            })
            .unwrap(),
            OpOutcome::ObjectCreated(0)
        );
        let st = m.stats();
        assert!(st.watertight);
        assert_eq!(
            (st.faces, st.edges, st.vertices, st.euler, st.genus),
            (6, 12, 8, 2, 0)
        );
        assert!((st.volume_m3 - 0.6096 * 0.4064 * 0.3048).abs() < 1e-12);
    }

    #[test]
    fn refused_ops_roll_back() {
        let mut m = Model::with_ground_sketch();
        m.apply(&Op::AddRectangle {
            sketch: 0,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        })
        .unwrap();
        let before = m.hash();
        for op in [
            Op::Extrude {
                sketch: 0,
                region: 0,
                distance: 0.0,
            },
            Op::Extrude {
                sketch: 0,
                region: 7,
                distance: 1.0,
            },
            Op::Extrude {
                sketch: 3,
                region: 0,
                distance: 1.0,
            },
            Op::PushPull {
                object: 0,
                face: 0,
                distance: 1.0,
            },
            Op::AddRectangle {
                sketch: 0,
                x: 0.5,
                y: 0.5,
                w: 1.0,
                h: 1.0,
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
        m.apply(&Op::AddRectangle {
            sketch: 0,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 2.0,
        })
        .unwrap();
        let sketch_only = m.hash();
        m.apply(&Op::Extrude {
            sketch: 0,
            region: 0,
            distance: 0.5,
        })
        .unwrap();
        assert!(
            m.apply(&Op::Extrude {
                sketch: 0,
                region: 0,
                distance: 0.5
            })
            .is_err(),
            "region already consumed"
        );
        assert_eq!(
            m.apply(&Op::PushPull {
                object: 0,
                face: 1,
                distance: -0.5
            })
            .unwrap(),
            OpOutcome::ObjectCollapsed
        );
        assert_eq!(m.hash(), sketch_only);
        assert_eq!(m.objects().len(), 0);
    }

    #[test]
    fn ops_round_trip_through_json() {
        let ops = vec![
            Op::NewSketch {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
            Op::AddRectangle {
                sketch: 0,
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            Op::Extrude {
                sketch: 0,
                region: 0,
                distance: 0.25,
            },
            Op::PushPull {
                object: 0,
                face: 1,
                distance: 0.1,
            },
        ];
        let json = serde_json::to_string(&ops).unwrap();
        assert!(json.contains(r#""op":"add_rectangle""#));
        let back: Vec<Op> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ops);
        assert_eq!(
            Model::replay(&ops).unwrap().hash(),
            Model::replay(&back).unwrap().hash()
        );
    }
}
