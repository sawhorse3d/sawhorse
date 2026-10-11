//! The persistent document the UI edits (runs in the kernel Web Worker).
//!
//! A [`Document`] is a [`Model`] (sketches, objects, groups, tags) plus:
//!
//! - an **op log**: every command that changed the document, in order, including `Undo`
//!   and `Redo`. Replaying the log from an empty document reproduces the state exactly, so
//!   the log is the document's history, its golden-test format and (later) its save format;
//! - **undo / redo** as snapshots: before each op the current model is pushed onto a ring
//!   of [`UNDO_DEPTH`] snapshots (the oldest falls off); undo swaps the current model with
//!   the newest snapshot, redo swaps it back. Any new op clears the redo stack.
//!
//! Every op still goes through [`Model::apply`] (clone, mutate, validate, commit), so a
//! refused op changes nothing: not the model, not the log, not the undo ring.

use std::collections::VecDeque;

use crate::error::{DocumentError, KernelError};
use crate::hash::StableHasher;
use crate::model::{Model, Op, OpOutcome};

/// How many steps can be undone.
pub const UNDO_DEPTH: usize = 200;

#[derive(Debug, Clone, Default)]
pub struct Document {
    model: Model,
    log: Vec<Op>,
    undo: VecDeque<Model>,
    redo: Vec<Model>,
}

impl Document {
    /// An empty document (no sketches).
    pub fn new() -> Document {
        Document::default()
    }

    /// A document with the ground sketch (public id 1), as the UI starts. The sketch is
    /// created by a logged op, so it is part of the history but cannot be undone.
    pub fn with_ground_sketch() -> Document {
        let mut d = Document::new();
        d.apply(&Op::NewSketch {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        })
        .expect("ground sketch");
        d.undo.clear();
        d
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Every command applied so far, including undo and redo.
    pub fn log(&self) -> &[Op] {
        &self.log
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Steps available to undo / redo.
    pub fn depth(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    /// Apply a command (any [`Op`], including `Undo` / `Redo`). Transactional.
    pub fn apply(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        match op {
            Op::Undo => self.undo(),
            Op::Redo => self.redo(),
            _ => {
                let before = self.model.clone();
                let out = self.model.apply(op)?;
                self.undo.push_back(before);
                if self.undo.len() > UNDO_DEPTH {
                    self.undo.pop_front();
                }
                self.redo.clear();
                self.log.push(op.clone());
                Ok(out)
            }
        }
    }

    pub fn undo(&mut self) -> Result<OpOutcome, KernelError> {
        let prev = self.undo.pop_back().ok_or(DocumentError::NothingToUndo)?;
        let current = std::mem::replace(&mut self.model, prev);
        self.redo.push(current);
        self.log.push(Op::Undo);
        Ok(OpOutcome::Undone)
    }

    pub fn redo(&mut self) -> Result<OpOutcome, KernelError> {
        let next = self.redo.pop().ok_or(DocumentError::NothingToRedo)?;
        let current = std::mem::replace(&mut self.model, next);
        self.undo.push_back(current);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.pop_front();
        }
        self.log.push(Op::Redo);
        Ok(OpOutcome::Redone)
    }

    /// Replay a log from an empty document, stopping at the first refused command.
    pub fn replay(ops: &[Op]) -> Result<Document, (usize, KernelError)> {
        let mut d = Document::new();
        for (i, op) in ops.iter().enumerate() {
            d.apply(op).map_err(|e| (i, e))?;
        }
        Ok(d)
    }

    /// Hash of the current state (the model hash; see [`crate::hash`]).
    pub fn hash(&self) -> u64 {
        self.model.hash()
    }

    pub fn hash_hex(&self) -> String {
        self.model.hash_hex()
    }

    /// Deterministic hash of the op log: FNV-1a over each op's canonical JSON (serde_json
    /// prints f64 identically natively and in WASM).
    pub fn log_hash(&self) -> u64 {
        let mut h = StableHasher::default();
        h.tag("sawhorse-op-log-v1");
        h.usize(self.log.len());
        for op in &self.log {
            h.tag(&serde_json::to_string(op).unwrap_or_default());
        }
        h.finish()
    }

    pub fn log_hash_hex(&self) -> String {
        format!("{:016x}", self.log_hash())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64) -> Op {
        Op::AddRectangle {
            sketch: 1,
            x,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }

    #[test]
    fn undo_redo_restore_exact_states() {
        let mut d = Document::with_ground_sketch();
        assert!(!d.can_undo());
        let h0 = d.hash();
        d.apply(&rect(0.0)).unwrap();
        let h1 = d.hash();
        d.apply(&Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 0.5,
        })
        .unwrap();
        let h2 = d.hash();
        assert_eq!(d.apply(&Op::Undo).unwrap(), OpOutcome::Undone);
        assert_eq!(d.hash(), h1);
        d.undo().unwrap();
        assert_eq!(d.hash(), h0);
        assert_eq!(d.undo(), Err(DocumentError::NothingToUndo.into()));
        d.redo().unwrap();
        d.redo().unwrap();
        assert_eq!(d.hash(), h2);
        assert_eq!(d.redo(), Err(DocumentError::NothingToRedo.into()));
        // A new op after an undo drops the redo branch.
        d.undo().unwrap();
        d.apply(&rect(3.0)).unwrap();
        assert!(!d.can_redo());
    }

    #[test]
    fn refused_ops_touch_nothing() {
        let mut d = Document::with_ground_sketch();
        d.apply(&rect(0.0)).unwrap();
        let (h, log, depth) = (d.hash(), d.log().len(), d.depth());
        assert!(d.apply(&rect(f64::NAN)).is_err());
        assert!(d.apply(&Op::DeleteObject { object: 9 }).is_err());
        assert_eq!((d.hash(), d.log().len(), d.depth()), (h, log, depth));
    }

    #[test]
    fn replaying_the_log_reproduces_state_and_log() {
        let mut d = Document::with_ground_sketch();
        d.apply(&rect(0.0)).unwrap();
        d.apply(&Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 0.5,
        })
        .unwrap();
        d.undo().unwrap();
        d.redo().unwrap();
        d.apply(&Op::SetTag {
            object: 1,
            tag: Some("Legs".into()),
        })
        .unwrap();
        d.apply(&Op::CreateGroup {
            name: "Table".into(),
            objects: vec![1],
        })
        .unwrap();
        let r = Document::replay(d.log()).unwrap();
        assert_eq!(r.hash(), d.hash());
        assert_eq!(r.log_hash(), d.log_hash());
        assert_eq!(
            r.depth(),
            (d.depth().0 + 1, 0),
            "the replay can also undo the ground sketch"
        );
        let json = serde_json::to_string(d.log()).unwrap();
        assert!(json.contains(r#"{"op":"undo"}"#));
    }

    #[test]
    fn undo_ring_holds_200_steps() {
        let mut d = Document::with_ground_sketch();
        for i in 0..(UNDO_DEPTH + 25) {
            d.apply(&rect(2.0 * i as f64)).unwrap();
        }
        assert_eq!(d.depth().0, UNDO_DEPTH);
        for _ in 0..UNDO_DEPTH {
            d.undo().unwrap();
        }
        assert!(!d.can_undo());
        assert_eq!(
            d.model().stats().regions,
            25,
            "the oldest 25 steps fell off the ring"
        );
    }

    #[test]
    fn groups_tags_and_delete() {
        let mut d = Document::with_ground_sketch();
        d.apply(&rect(0.0)).unwrap();
        d.apply(&rect(2.0)).unwrap();
        for r in [1, 2] {
            d.apply(&Op::Extrude {
                sketch: 1,
                region: r,
                distance: 0.5,
            })
            .unwrap();
        }
        let g = d
            .apply(&Op::CreateGroup {
                name: "Pair".into(),
                objects: vec![1, 2, 2],
            })
            .unwrap();
        assert_eq!(g, OpOutcome::GroupCreated { group: 1 });
        assert_eq!(d.model().groups()[0].1.objects, vec![1, 2]);
        d.apply(&Op::SetTag {
            object: 2,
            tag: Some("Hidden".into()),
        })
        .unwrap();
        d.apply(&Op::SetTagVisible {
            tag: "Hidden".into(),
            visible: false,
        })
        .unwrap();
        assert!(!d.model().is_visible(2));
        let mesh = crate::mesh::view_mesh(d.model()).unwrap();
        assert_eq!(mesh.triangle_count(), 12, "only the visible box is drawn");
        d.apply(&Op::DeleteObject { object: 1 }).unwrap();
        assert_eq!(d.model().groups()[0].1.objects, vec![2]);
        d.apply(&Op::DeleteObject { object: 2 }).unwrap();
        assert!(d.model().groups().is_empty(), "an emptied group goes away");
        for bad in [
            Op::CreateGroup {
                name: " ".into(),
                objects: vec![1],
            },
            Op::CreateGroup {
                name: "x".into(),
                objects: vec![],
            },
            Op::DeleteGroup { group: 7 },
            Op::SetTag {
                object: 5,
                tag: None,
            },
        ] {
            assert!(d.apply(&bad).is_err(), "{bad:?}");
        }
        assert!(Model::new().apply(&Op::Undo).is_err());
    }
}
