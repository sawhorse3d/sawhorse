//! Named batches with placeholders: how a plugin changes the model (lane 11, ADR-0009).
//!
//! A plugin sends a name and a list of ops. An op may hold a **placeholder**,
//! `{"$ref": 2, "path": "regions.0"}`, wherever a value goes: "the value at `path` in the
//! outcome of op 2 of this batch" (an earlier op only). That is how a plugin draws a
//! rectangle, extrudes the region it made and names the solid that came out, without
//! knowing any id in advance.
//!
//! Nothing new reaches the model: the batch becomes one ordinary [`Op::Batch`] of plain
//! ops, applied through [`Document::apply`] like any edit, so it is one undo step, it is
//! validated by the kernel's validator, it is logged and replays identically, and a
//! refusal anywhere leaves the document exactly as it was.
//!
//! How the ids are found: the ops are first applied one by one to a scratch copy of the
//! model, each placeholder filled from the outcomes so far. The resolved list is then
//! applied as one batch to a second copy and its outcomes compared with the scratch ones
//! (a batch tidies once at the end, single ops after each; if that ever made an id come out
//! differently, the batch is refused rather than applied with a wrong id). Only then is it
//! applied to the document: the checked copy itself becomes the model (lane 16: it used to
//! be applied a third time).
//!
//! Lane 16: a batch has a work budget as well as an op count. Each op costs about the size
//! of the model it is applied to (the copy, the checks), so a batch of [`MAX_OPS`] ops is
//! fine on a small model and refused on a huge one before it starts: ops times things in
//! the model must stay under [`WORK_BUDGET`].

use serde::Deserialize as _;
use serde_json::Value;

use crate::document::Document;
use crate::error::KernelError;
use crate::model::{Model, Op, OpOutcome};

/// Most ops one batch may hold.
pub const MAX_OPS: usize = 2_000;
/// Ops times things in the model (solids, meshes, groups and instances, sketches) one
/// batch may cost (lane 16).
pub const WORK_BUDGET: usize = 20_000_000;
/// Longest batch name (it names the undo step).
pub const MAX_NAME: usize = 80;
/// Deepest placeholder path.
const MAX_PATH: usize = 16;

/// What a plugin sends.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NamedBatch {
    pub name: String,
    pub ops: Vec<Value>,
}

/// Why a batch was refused. Nothing changed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RefBatchError {
    #[error("a batch needs a name of 1 to {MAX_NAME} characters")]
    BadName,
    #[error("a batch needs at least one op")]
    Empty,
    #[error("a batch holds at most {MAX_OPS} ops")]
    TooMany,
    #[error(
        "a batch of {ops} ops on a model of {things} things is more work than one batch may do (split it into smaller batches)"
    )]
    TooMuchWork { ops: usize, things: usize },
    #[error("op {index}: {message}")]
    BadOp { index: usize, message: String },
    /// An op the kernel knows, with a field missing or of the wrong kind (#116).
    #[error("op {index} ({kind}): {message}")]
    BadField {
        index: usize,
        kind: String,
        message: String,
    },
    #[error("op {index}: placeholder {message}")]
    BadRef { index: usize, message: String },
    #[error("op {index} ({kind}): {error}")]
    Refused {
        index: usize,
        kind: String,
        error: Box<KernelError>,
    },
    #[error(
        "the batch came out differently applied as one step than op by op (op {index}); nothing changed"
    )]
    Diverged { index: usize },
    #[error("{0}")]
    Kernel(Box<KernelError>),
}

impl RefBatchError {
    /// A validator failure (a kernel bug), not a refusal of bad input.
    pub fn is_bug(&self) -> bool {
        match self {
            RefBatchError::Refused { error, .. } | RefBatchError::Kernel(error) => error.is_bug(),
            _ => false,
        }
    }
}

/// Ops a batch may not hold: history, nested batches, navigation, whole-document restores.
pub fn forbidden(op: &Op) -> bool {
    matches!(
        op,
        Op::Undo | Op::Redo | Op::Batch { .. } | Op::Restore { .. }
    ) || op.is_navigation()
}

/// Whether `v` holds a placeholder anywhere.
pub fn has_placeholders(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.contains_key("$ref") || m.values().any(has_placeholders),
        Value::Array(a) => a.iter().any(has_placeholders),
        _ => false,
    }
}

/// The value at `path` ("regions.0", "groups.0", "object") in an outcome.
fn lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(v);
    }
    let mut at = v;
    for part in path.split('.') {
        at = match at {
            Value::Object(m) => m.get(part)?,
            Value::Array(a) => a.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(at)
}

/// `raw` with every placeholder replaced by the value it names in `outcomes`.
fn fill(raw: &Value, index: usize, outcomes: &[Value]) -> Result<Value, RefBatchError> {
    let bad = |message: String| RefBatchError::BadRef { index, message };
    match raw {
        Value::Object(m) if m.contains_key("$ref") => {
            if m.keys().any(|k| k != "$ref" && k != "path") {
                return Err(bad("may hold only \"$ref\" and \"path\"".to_string()));
            }
            let at = m["$ref"]
                .as_u64()
                .ok_or_else(|| bad("\"$ref\" must be the index of an earlier op".to_string()))?
                as usize;
            if at >= index {
                return Err(bad(format!(
                    "refers to op {at}, which is not before it (a placeholder names an earlier op of the batch)"
                )));
            }
            let path = match m.get("path") {
                None => "",
                Some(Value::String(s)) => s.as_str(),
                Some(_) => return Err(bad("\"path\" must be text".to_string())),
            };
            if path.split('.').count() > MAX_PATH {
                return Err(bad(format!("path {path:?} is too deep")));
            }
            let kind = outcomes[at]
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("outcome");
            lookup(&outcomes[at], path).cloned().ok_or_else(|| {
                bad(format!(
                    "{{$ref: {at}, path: {path:?}}}: op {at}'s outcome ({kind}) has nothing at {path:?}"
                ))
            })
        }
        Value::Object(m) => {
            let mut out = serde_json::Map::with_capacity(m.len());
            for (k, v) in m {
                out.insert(k.clone(), fill(v, index, outcomes)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(a) => a
            .iter()
            .map(|v| fill(v, index, outcomes))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        v => Ok(v.clone()),
    }
}

/// Resolve every placeholder against `model` (which is not changed): the plain ops and
/// what each did.
pub fn resolve(
    model: &Model,
    batch: &NamedBatch,
) -> Result<(Vec<Op>, Vec<OpOutcome>), RefBatchError> {
    let name = batch.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(RefBatchError::BadName);
    }
    if batch.ops.is_empty() {
        return Err(RefBatchError::Empty);
    }
    if batch.ops.len() > MAX_OPS {
        return Err(RefBatchError::TooMany);
    }
    let things =
        1 + model.objects.len() + model.meshes.len() + model.nodes.len() + model.sketches.len();
    if batch.ops.len().saturating_mul(things) > WORK_BUDGET {
        return Err(RefBatchError::TooMuchWork {
            ops: batch.ops.len(),
            things,
        });
    }
    let mut scratch = model.clone();
    let mut outcome_json: Vec<Value> = Vec::with_capacity(batch.ops.len());
    let mut ops = Vec::with_capacity(batch.ops.len());
    let mut outcomes = Vec::with_capacity(batch.ops.len());
    for (index, raw) in batch.ops.iter().enumerate() {
        let filled = fill(raw, index, &outcome_json)?;
        let kind = filled
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let op = Op::deserialize(&filled).map_err(|e| unreadable(index, &kind, &filled, &e))?;
        if forbidden(&op) {
            return Err(RefBatchError::BadOp {
                index,
                message: format!(
                    "{kind} cannot be part of a batch (undo, redo, batches, opening and closing groups, restoring a document)"
                ),
            });
        }
        let out = scratch.apply(&op).map_err(|error| RefBatchError::Refused {
            index,
            kind: kind.clone(),
            error: Box::new(error),
        })?;
        outcome_json.push(serde_json::to_value(&out).unwrap_or(Value::Null));
        ops.push(op);
        outcomes.push(out);
    }
    Ok((ops, outcomes))
}

/// Apply a named batch to the document as ONE undo step. Returns the batch's outcome and the
/// plain op that was applied (and logged). On any refusal the document is unchanged.
pub fn apply(doc: &mut Document, batch: &NamedBatch) -> Result<(OpOutcome, Op), RefBatchError> {
    let (ops, outcomes) = resolve(doc.model(), batch)?;
    let op = Op::Batch { ops };
    // The resolved batch must do exactly what the ops did one by one.
    let (next, out) = doc
        .model()
        .applied(&op)
        .map_err(|e| RefBatchError::Kernel(Box::new(e)))?;
    match &out {
        OpOutcome::Batch { outcomes: got } => {
            if let Some(index) = (0..outcomes.len()).find(|&i| got.get(i) != Some(&outcomes[i])) {
                return Err(RefBatchError::Diverged { index });
            }
        }
        _ => return Err(RefBatchError::Diverged { index: 0 }),
    }
    // The checked copy is the new model: committed with the op logged, as Document::apply
    // would (it is the same validated result).
    doc.commit_applied(op.clone(), next);
    Ok((out, op))
}

/// Why `op` (filled in, and named `kind`) does not read as an op. An op the kernel does not
/// know says so. A known one with a field missing or of the wrong kind names the field
/// (#116). Serde names a missing field itself; for a wrong one it says only what it
/// expected, so the field is found by reading the op again without each field in turn:
/// the one whose absence makes it read, or be missing, is the one that was wrong.
fn unreadable(index: usize, kind: &str, op: &Value, e: &serde_json::Error) -> RefBatchError {
    let text = e.to_string();
    // Only the op's own name being unknown makes it an op the kernel does not know; an
    // unknown variant inside one of its fields (an item's kind, a draw target's) is a field.
    if !op.get("op").is_some_and(Value::is_string)
        || text.starts_with(&format!("unknown variant `{kind}`"))
    {
        return RefBatchError::BadOp {
            index,
            message: format!("not an op the kernel knows ({e})"),
        };
    }
    let field = |message: String| RefBatchError::BadField {
        index,
        kind: kind.to_string(),
        message,
    };
    if text.starts_with("missing field") {
        return field(text);
    }
    if let Some(fields) = op.as_object() {
        for name in fields.keys().filter(|k| *k != "op") {
            let mut without = fields.clone();
            without.remove(name);
            let culprit = match serde_json::from_value::<Op>(Value::Object(without)) {
                Ok(_) => true,
                Err(e) => e
                    .to_string()
                    .starts_with(&format!("missing field `{name}`")),
            };
            if culprit {
                return field(format!("field `{name}`: {text}"));
            }
        }
    }
    field(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ground() -> Document {
        Document::with_ground_sketch()
    }

    fn batch(ops: Value) -> NamedBatch {
        NamedBatch {
            name: "Test".into(),
            ops: ops.as_array().unwrap().clone(),
        }
    }

    #[test]
    fn placeholders_name_what_earlier_ops_made() {
        let mut d = ground();
        let (out, _) = apply(
            &mut d,
            &batch(json!([
                {"op": "add_rectangle", "sketch": 1, "x": 0.0, "y": 0.0, "w": 0.6, "h": 0.4},
                {"op": "extrude", "sketch": 1, "region": {"$ref": 0, "path": "regions.0"}, "distance": 0.3},
                {"op": "rename", "item": {"object": {"$ref": 1, "path": "object"}}, "name": "Top"}
            ])),
        )
        .unwrap();
        let OpOutcome::Batch { outcomes } = out else {
            panic!("not a batch")
        };
        assert_eq!(outcomes.len(), 3);
        assert_eq!(d.model().objects().len(), 1);
        // One undo step takes the whole batch back.
        let (undo, _) = d.depth();
        assert_eq!(undo, 1);
        d.undo().unwrap();
        assert_eq!(d.model().objects().len(), 0);
    }

    #[test]
    fn refusals_change_nothing_and_say_which_op() {
        let mut d = ground();
        let before = (d.hash(), d.log().len(), d.depth());
        let cases = [
            (
                json!([{"op": "extrude", "sketch": 1, "region": {"$ref": 0}, "distance": 1.0}]),
                "not before it",
            ),
            (
                json!([{"op": "add_rectangle", "sketch": 1, "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0}, {"op": "extrude", "sketch": 1, "region": {"$ref": 0, "path": "nope"}, "distance": 1.0}]),
                "has nothing at \"nope\"",
            ),
            (json!([{"op": "undo"}]), "cannot be part of a batch"),
            (
                json!([{"op": "set_context", "node": null}]),
                "cannot be part of a batch",
            ),
            (json!([{"op": "frobnicate"}]), "not an op the kernel knows"),
            (
                json!([{"op": "add_rectangle", "sketch": 1, "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0}, {"op": "push_pull", "object": 99, "face": 1, "distance": 1.0}]),
                "op 1 (push_pull): no object #99",
            ),
            (
                json!([{"op": "extrude", "sketch": 1, "region": {"$ref": 0, "extra": 1}, "distance": 1.0}]),
                "may hold only",
            ),
            (json!([]), "at least one op"),
        ];
        for (ops, want) in cases {
            let e = apply(&mut d, &batch(ops)).unwrap_err();
            assert!(e.to_string().contains(want), "{e} should say {want}");
            assert!(!e.is_bug());
            assert_eq!((d.hash(), d.log().len(), d.depth()), before);
        }
        let blank = NamedBatch {
            name: "  ".into(),
            ops: vec![json!({"op": "delete_guides"})],
        };
        assert_eq!(apply(&mut d, &blank).unwrap_err(), RefBatchError::BadName);
    }

    #[test]
    fn the_logged_op_is_a_plain_batch_that_replays() {
        let mut d = ground();
        apply(
            &mut d,
            &batch(json!([
                {"op": "new_sketch", "origin": [0.0, 0.0, 0.5], "normal": [0.0, 0.0, 1.0]},
                {"op": "add_rectangle", "sketch": {"$ref": 0, "path": "sketch"}, "x": 0.0, "y": 0.0, "w": 0.2, "h": 0.2},
                {"op": "extrude", "sketch": {"$ref": 0, "path": "sketch"}, "region": {"$ref": 1, "path": "regions.0"}, "distance": 0.1}
            ])),
        )
        .unwrap();
        let logged = d.log().last().unwrap();
        let text = serde_json::to_string(logged).unwrap();
        assert!(!text.contains("$ref"), "{text}");
        let again = Document::replay(d.log()).unwrap();
        assert_eq!(again.hash(), d.hash());
        assert_eq!(again.log_hash(), d.log_hash());
    }

    /// A board, made a component: the instance's node id.
    fn instance(d: &mut Document) -> u64 {
        let out = apply(
            d,
            &batch(json!([
                {"op": "add_rectangle", "sketch": 1, "x": 0.0, "y": 0.0, "w": 0.6, "h": 0.3},
                {"op": "extrude", "sketch": 1, "region": {"$ref": 0, "path": "regions.0"}, "distance": 0.019},
                {"op": "make_component", "items": [{"object": {"$ref": 1, "path": "object"}}], "name": "Slat"}
            ])),
        )
        .unwrap();
        let OpOutcome::Batch { outcomes } = out.0 else {
            panic!("not a batch")
        };
        let OpOutcome::ComponentCreated { instance, .. } = outcomes[2] else {
            panic!("no component: {:?}", outcomes[2])
        };
        instance
    }

    /// #116: copy, array and transform of nodes alone, with no `objects`.
    #[test]
    fn copy_array_and_transform_take_nodes_alone() {
        let mut d = ground();
        let node = instance(&mut d);
        let shift = json!({"linear": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "translation": [0.0, 0.4, 0.0]});
        apply(
            &mut d,
            &batch(json!([{"op": "copy", "nodes": [node], "xform": shift}])),
        )
        .unwrap();
        apply(
            &mut d,
            &batch(json!([{"op": "array", "nodes": [node], "xform": shift, "count": 2}])),
        )
        .unwrap();
        apply(
            &mut d,
            &batch(json!([{"op": "transform", "nodes": [node], "xform": shift}])),
        )
        .unwrap();
        // Four instances of one definition: one row of four in the cut list.
        let list = d.model().cut_list(&crate::cutlist::CutOptions::default());
        assert_eq!(list.rows.len(), 1);
        assert_eq!(list.rows[0].qty, 4);
    }

    /// #116: `objects` is still written as before, so no stored log or golden changes.
    #[test]
    fn objects_is_written_as_before() {
        for op in [
            json!({"op": "copy", "nodes": [3], "xform": {"linear": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "translation": [0.0, 0.0, 0.0]}}),
            json!({"op": "array", "nodes": [3], "count": 2, "xform": {"linear": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "translation": [0.0, 0.0, 0.0]}}),
            json!({"op": "transform", "nodes": [3], "xform": {"linear": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "translation": [0.0, 0.0, 0.0]}}),
        ] {
            let read: Op = serde_json::from_value(op).unwrap();
            let written = serde_json::to_value(&read).unwrap();
            assert_eq!(written["objects"], json!([]), "{written}");
            assert_eq!(serde_json::from_value::<Op>(written).unwrap(), read);
        }
    }

    /// #116: a known op with a missing or wrong field says which, not that the op is unknown.
    #[test]
    fn a_known_op_with_a_bad_field_names_the_field() {
        let mut d = ground();
        let before = (d.hash(), d.log().len(), d.depth());
        let shift = json!({"linear": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "translation": [0.0, 0.0, 0.0]});
        let cases = [
            // Missing.
            (
                json!({"op": "copy", "nodes": [1]}),
                "op 0 (copy): missing field `xform`",
            ),
            (
                json!({"op": "array", "nodes": [1], "xform": shift}),
                "op 0 (array): missing field `count`",
            ),
            // Wrong: required and optional fields.
            (
                json!({"op": "array", "nodes": [1], "xform": shift, "count": "three"}),
                "op 0 (array): field `count`",
            ),
            (
                json!({"op": "copy", "nodes": "all", "xform": shift}),
                "op 0 (copy): field `nodes`",
            ),
            (
                json!({"op": "add_rectangle", "sketch": 1, "x": "0", "y": 0.0, "w": 1.0, "h": 1.0}),
                "op 0 (add_rectangle): field `x`",
            ),
        ];
        for (op, want) in cases {
            let e = apply(&mut d, &batch(json!([op]))).unwrap_err().to_string();
            assert!(e.contains(want), "{e} should say {want}");
            assert!(!e.contains("not an op the kernel knows"), "{e}");
            assert_eq!((d.hash(), d.log().len(), d.depth()), before);
        }
        // A wrong value of an enum inside a known op is the op's field, not an unknown op.
        let e = apply(
            &mut d,
            &batch(json!([{"op": "rename", "item": {"objekt": 1}, "name": "x"}])),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("op 0 (rename): field `item`"), "{e}");
        assert_eq!((d.hash(), d.log().len(), d.depth()), before);
        // An op the kernel does not know, none, or an `op` that is not a name, still says so.
        for op in [
            json!({"op": "frobnicate"}),
            json!({"sketch": 1}),
            json!({"op": 5}),
        ] {
            let e = apply(&mut d, &batch(json!([op]))).unwrap_err().to_string();
            assert!(e.contains("not an op the kernel knows"), "{e}");
        }
    }
}
