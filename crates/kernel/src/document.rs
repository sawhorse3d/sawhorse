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
//!
//! Lane 16: the undo ring is also bounded in bytes ([`UNDO_BYTES`]): each snapshot is
//! charged for what it holds that the state after it does not share (B-reps and meshes are
//! shared between copies until they change), and the oldest steps fall off first. The log
//! hash is kept as each op is logged (a digest per op), never by serializing the whole log
//! again, and an op can be logged without a copy ([`Document::apply_owned`]).
//!
//! Opening and closing a group or component for editing ([`Op::SetContext`],
//! [`Op::LeaveContext`]) is navigation, not an edit: it is logged (so the log replays) but
//! is not an undo step, and it keeps the redo stack. Undo restores the model as it was
//! before the last edit, editing context included, so it returns to where that edit was
//! made.

use std::collections::VecDeque;

use crate::error::{DocumentError, KernelError};
use crate::hash::StableHasher;
use crate::model::{Model, Op, OpOutcome};

/// How many steps can be undone.
pub const UNDO_DEPTH: usize = 200;

/// Lane 16: most bytes the undo and redo history may hold (estimated, see
/// [`Model::history_cost`]); past it the oldest steps fall off, but the last step can
/// always be undone.
pub const UNDO_BYTES: usize = 512 * 1024 * 1024;

/// Lane 16: most bytes of JSON the op log may hold (every op is kept, an import's whole
/// soup included). Past it the log is compacted to one `restore` of the current model: the
/// model and its undo steps in memory are unchanged, but a rebuild from the log (a restarted
/// kernel, an autosave) starts from that point, without the steps before it.
pub const LOG_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct Document {
    model: Model,
    log: Vec<Op>,
    /// A digest of each logged op's JSON, worked out once when it is logged (lane 16).
    digests: Vec<u64>,
    /// Bytes of the logged ops' JSON (lane 16, [`LOG_BYTES`]).
    log_bytes: usize,
    undo: VecDeque<Model>,
    /// What each undo snapshot costs (bytes, estimated), in step with `undo`.
    undo_cost: VecDeque<usize>,
    redo: Vec<Model>,
    redo_cost: Vec<usize>,
}

/// The digest of one op's canonical JSON (what the log hash is made of), and its length.
fn op_digest(op: &Op) -> (u64, usize) {
    let json = serde_json::to_string(op).unwrap_or_default();
    let mut h = StableHasher::default();
    h.tag(&json);
    (h.finish(), json.len())
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
        d.clear_undo();
        d
    }

    fn clear_undo(&mut self) {
        self.undo.clear();
        self.undo_cost.clear();
    }

    fn push_log(&mut self, op: Op) {
        let (digest, len) = op_digest(&op);
        self.digests.push(digest);
        self.log_bytes += len;
        self.log.push(op);
        if self.log_bytes > LOG_BYTES && self.log.len() > 1 {
            self.compact_log();
        }
    }

    /// Replace the log with one `restore` of the current model (see [`LOG_BYTES`]). The model
    /// and the undo and redo steps are unchanged; replaying the log gives the current model.
    pub fn compact_log(&mut self) {
        let op = Op::Restore {
            snapshot: Box::new(self.model.snapshot()),
        };
        let (digest, len) = op_digest(&op);
        self.log = vec![op];
        self.digests = vec![digest];
        self.log_bytes = len;
    }

    /// Bytes of JSON the log holds.
    pub fn log_bytes(&self) -> usize {
        self.log_bytes
    }

    /// Keep the history within [`UNDO_DEPTH`] steps and [`UNDO_BYTES`].
    fn trim_history(&mut self) {
        let total =
            |d: &Document| d.undo_cost.iter().sum::<usize>() + d.redo_cost.iter().sum::<usize>();
        while self.undo.len() > UNDO_DEPTH || (self.undo.len() > 1 && total(self) > UNDO_BYTES) {
            self.undo.pop_front();
            self.undo_cost.pop_front();
        }
    }

    /// A document holding a saved model: its log is the one `Restore` op (so it replays),
    /// with nothing to undo, as a freshly opened file should be.
    pub fn open(snapshot: crate::snapshot::Snapshot) -> Result<Document, KernelError> {
        let mut d = Document::new();
        d.apply_owned(Op::Restore {
            snapshot: Box::new(snapshot),
        })?;
        d.clear_undo();
        Ok(d)
    }

    /// Forget undo and redo (the log stays): a file just opened has nothing to undo.
    pub fn clear_history(&mut self) {
        self.clear_undo();
        self.redo.clear();
        self.redo_cost.clear();
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
        let out = self.run(op)?;
        if !matches!(op, Op::Undo | Op::Redo) {
            self.push_log(op.clone());
        }
        Ok(out)
    }

    /// [`Document::apply`], logging the op itself instead of a copy (an import's soup is
    /// the largest thing in the log; lane 16).
    pub fn apply_owned(&mut self, op: Op) -> Result<OpOutcome, KernelError> {
        let out = self.run(&op)?;
        if !matches!(op, Op::Undo | Op::Redo) {
            self.push_log(op);
        }
        Ok(out)
    }

    /// Everything but logging the op (undo and redo log themselves).
    fn run(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        match op {
            Op::Undo => self.undo(),
            Op::Redo => self.redo(),
            _ if op.is_navigation() => self.model.apply(op),
            _ => {
                // One copy of the model per edit: the edit works on it and, when it
                // validates, it becomes the model and the old one the undo snapshot.
                let (next, out) = self.model.applied(op)?;
                let before = std::mem::replace(&mut self.model, next);
                let cost = before.history_cost(&self.model);
                self.undo.push_back(before);
                self.undo_cost.push_back(cost);
                self.redo.clear();
                self.redo_cost.clear();
                self.trim_history();
                Ok(out)
            }
        }
    }

    pub fn undo(&mut self) -> Result<OpOutcome, KernelError> {
        let prev = self.undo.pop_back().ok_or(DocumentError::NothingToUndo)?;
        self.undo_cost.pop_back();
        let current = std::mem::replace(&mut self.model, prev);
        let cost = current.history_cost(&self.model);
        self.redo.push(current);
        self.redo_cost.push(cost);
        self.push_log(Op::Undo);
        Ok(OpOutcome::Undone)
    }

    pub fn redo(&mut self) -> Result<OpOutcome, KernelError> {
        let next = self.redo.pop().ok_or(DocumentError::NothingToRedo)?;
        self.redo_cost.pop();
        let current = std::mem::replace(&mut self.model, next);
        let cost = current.history_cost(&self.model);
        self.undo.push_back(current);
        self.undo_cost.push_back(cost);
        self.trim_history();
        self.push_log(Op::Redo);
        Ok(OpOutcome::Redone)
    }

    /// Commit a model already made from the current one by `op` with [`Model::applied`] (so
    /// validated): the undo snapshot, the log and the history limits as [`Document::apply`]
    /// would (lane 16: a plugin batch is checked on a copy, which then becomes the model).
    pub(crate) fn commit_applied(&mut self, op: Op, next: Model) {
        let before = std::mem::replace(&mut self.model, next);
        let cost = before.history_cost(&self.model);
        self.undo.push_back(before);
        self.undo_cost.push_back(cost);
        self.redo.clear();
        self.redo_cost.clear();
        self.trim_history();
        self.push_log(op);
    }

    /// Bytes the history holds (estimated), undo and redo together.
    pub fn history_bytes(&self) -> usize {
        self.undo_cost.iter().sum::<usize>() + self.redo_cost.iter().sum::<usize>()
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

    /// Deterministic hash of the op log: FNV-1a over the digest of each op's canonical JSON
    /// (serde_json prints f64 identically natively and in WASM). The digests are worked out
    /// once, as each op is logged (lane 16: v1 serialized the whole log on every call).
    pub fn log_hash(&self) -> u64 {
        let mut h = StableHasher::default();
        h.tag("sawhorse-op-log-v2");
        h.usize(self.log.len());
        for &d in &self.digests {
            h.u64(d);
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
    fn lane_16_a_compacted_log_replays_to_the_same_model() {
        let mut d = Document::with_ground_sketch();
        d.apply(&rect(0.0)).unwrap();
        d.apply(&Op::Extrude {
            sketch: 1,
            region: 1,
            distance: 0.5,
        })
        .unwrap();
        let (h, depth) = (d.hash(), d.depth());
        assert!(d.log_bytes() > 0);
        d.compact_log();
        assert_eq!(d.log().len(), 1);
        assert_eq!(
            (d.hash(), d.depth()),
            (h, depth),
            "the model and its undo are kept"
        );
        let r = Document::replay(d.log()).unwrap();
        assert_eq!(r.hash(), h);
        assert_eq!(r.log_hash(), d.log_hash());
        // Undo still works on the live document.
        d.undo().unwrap();
        assert_ne!(d.hash(), h);
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
        // What is inside a group is edited inside it (lane 5).
        assert!(d.apply(&Op::DeleteObject { object: 1 }).is_err());
        let depth = d.depth();
        d.apply(&Op::SetContext { node: Some(1) }).unwrap();
        assert_eq!(d.depth(), depth, "opening a group is not an undo step");
        d.apply(&Op::DeleteObject { object: 1 }).unwrap();
        assert_eq!(d.model().groups()[0].1.objects, vec![2]);
        d.apply(&Op::DeleteObject { object: 2 }).unwrap();
        assert_eq!(
            d.model().groups().len(),
            1,
            "the open group stays while it is open"
        );
        d.apply(&Op::LeaveContext).unwrap();
        assert!(d.model().groups().is_empty(), "an emptied group goes away");
        // Undo returns to the group (where the last edit was made), with its solid back.
        d.undo().unwrap();
        assert_eq!(d.model().context(), Some(1));
        assert_eq!(d.model().groups()[0].1.objects, vec![2]);
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
