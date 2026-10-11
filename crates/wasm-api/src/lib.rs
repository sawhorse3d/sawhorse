//! wasm-bindgen surface of the Sawhorse kernel, loaded by the front end's kernel Web Worker
//! in both the browser build and the Tauri webview (ADR-0001).
//!
//! The worker holds one persistent [`Doc`] (a kernel [`Document`]: model, op log, undo
//! ring) for the whole session; every edit is an op applied transactionally. Everything
//! here is a thin wrapper over [`DocCore`], which is plain Rust and tested natively; the
//! `#[wasm_bindgen]` layer only converts errors into `JsError` and vectors into typed
//! arrays.
//!
//! JS usage (see src/kernel/kernel.worker.ts):
//! ```js
//! const doc = document_new();                         // ground sketch is id 1
//! doc.apply_op('{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6,"h":0.4}');
//! doc.apply_op('{"op":"extrude","sketch":1,"region":1,"distance":0.3}');
//! doc.undo(); doc.redo();
//! const mesh = doc.get_scene_mesh();                  // or doc.get_view_mesh(objectId)
//! const hash = doc.document_hash();
//! const stl = doc.export_stl(1), obj = doc.export_obj(1);
//! ```
//! Ids cross the boundary as JS numbers (public ids; exact below 2^53).

use sawhorse_kernel::document::Document;
use sawhorse_kernel::mesh::{ViewMesh, object_view_mesh, view_mesh};
use sawhorse_kernel::model::Op;
use wasm_bindgen::prelude::*;

/// Kernel version string, for the UI's about box and the worker handshake.
#[wasm_bindgen]
pub fn kernel_version() -> String {
    sawhorse_kernel::VERSION.to_string()
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Plain-Rust document logic behind the JS class. Errors are strings (a refusal message).
#[derive(Debug, Clone)]
pub struct DocCore {
    doc: Document,
}

impl Default for DocCore {
    fn default() -> Self {
        DocCore {
            doc: Document::with_ground_sketch(),
        }
    }
}

/// Summary the UI shows after every op.
#[derive(Debug, serde::Serialize)]
struct DocState<'a> {
    stats: sawhorse_kernel::ModelStats,
    can_undo: bool,
    can_redo: bool,
    undo_depth: usize,
    redo_depth: usize,
    log_len: usize,
    log_hash: String,
    objects: Vec<u64>,
    tags: &'a std::collections::BTreeMap<String, bool>,
}

impl DocCore {
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Apply one op given as JSON; returns the outcome as JSON.
    pub fn apply_json(&mut self, op_json: &str) -> Result<String, String> {
        let op: Op = serde_json::from_str(op_json).map_err(|e| format!("bad op: {e}"))?;
        let out = self.doc.apply(&op).map_err(|e| e.to_string())?;
        serde_json::to_string(&out).map_err(|e| e.to_string())
    }

    pub fn undo(&mut self) -> Result<String, String> {
        self.apply_json(r#"{"op":"undo"}"#)
    }

    pub fn redo(&mut self) -> Result<String, String> {
        self.apply_json(r#"{"op":"redo"}"#)
    }

    pub fn scene_mesh(&self) -> Result<ViewMesh, String> {
        view_mesh(self.doc.model()).map_err(|e| e.to_string())
    }

    pub fn object_mesh(&self, object: u64) -> Result<ViewMesh, String> {
        object_view_mesh(self.doc.model(), object)
            .ok_or_else(|| format!("no object #{object}"))?
            .map_err(|e| e.to_string())
    }

    pub fn state_json(&self) -> String {
        let m = self.doc.model();
        let (undo_depth, redo_depth) = self.doc.depth();
        let st = DocState {
            stats: m.stats(),
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            undo_depth,
            redo_depth,
            log_len: self.doc.log().len(),
            log_hash: self.doc.log_hash_hex(),
            objects: m.object_ids(),
            tags: m.tags(),
        };
        serde_json::to_string(&st).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn log_json(&self) -> String {
        serde_json::to_string(self.doc.log()).unwrap_or_else(|_| "[]".to_string())
    }

    fn object(&self, id: u64) -> Result<&sawhorse_kernel::object::Object, String> {
        self.doc
            .model()
            .object(id)
            .ok_or_else(|| format!("no object #{id}"))
    }

    pub fn stl(&self, object: u64) -> Result<Vec<u8>, String> {
        sawhorse_export::object_stl(self.object(object)?).map_err(|e| e.to_string())
    }

    pub fn obj(&self, object: u64) -> Result<String, String> {
        sawhorse_export::object_obj(self.object(object)?, &format!("sawhorse-object-{object}"))
            .map_err(|e| e.to_string())
    }
}

/// State hash of the document produced by replaying a JSON op log (golden format).
pub fn replay_hash_core(ops_json: &str) -> Result<String, String> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| e.to_string())?;
    Document::replay(&ops)
        .map(|d| d.hash_hex())
        .map_err(|(i, e)| format!("op {i}: {e}"))
}

/// Run one boolean differential case (tests/booleans/cases.json format) through the kernel
/// and return its result plus both operand meshes, as JSON, for scripts/boolean-diff.mjs.
pub fn boolean_case_core(case_json: &str) -> Result<String, String> {
    use sawhorse_kernel::testing::{BoolCase, build_solid, run_boolean_case};
    let c: BoolCase = serde_json::from_str(case_json).map_err(|e| e.to_string())?;
    let result = run_boolean_case(&c)?;
    let mesh = |spec| -> Result<serde_json::Value, String> {
        let o = build_solid(spec)?;
        let t = o.tri_solid().map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "positions": t.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect::<Vec<f64>>(),
            "triangles": t.triangles.iter().flatten().copied().collect::<Vec<u32>>(),
        }))
    };
    Ok(serde_json::json!({ "result": result, "a": mesh(&c.a)?, "b": mesh(&c.b)? }).to_string())
}

fn js(e: String) -> JsError {
    JsError::new(&e)
}

fn id(x: f64) -> Result<u64, JsError> {
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < 9_007_199_254_740_992.0 {
        Ok(x as u64)
    } else {
        Err(JsError::new(&format!("bad id {x}")))
    }
}

/// The persistent kernel document (one per worker).
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct Doc {
    core: DocCore,
}

/// A new document with the ground sketch (sketch id 1).
#[wasm_bindgen]
pub fn document_new() -> Doc {
    Doc::default()
}

/// A document rebuilt by replaying a JSON op log (undo and redo entries included).
#[wasm_bindgen]
pub fn document_replay(ops_json: &str) -> Result<Doc, JsError> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| JsError::new(&e.to_string()))?;
    let doc = Document::replay(&ops).map_err(|(i, e)| JsError::new(&format!("op {i}: {e}")))?;
    Ok(Doc {
        core: DocCore { doc },
    })
}

/// Replay a JSON op log from empty and return its state hash (golden cross-check).
#[wasm_bindgen]
pub fn replay_hash(ops_json: &str) -> Result<String, JsError> {
    replay_hash_core(ops_json).map_err(js)
}

/// See [`boolean_case_core`].
#[wasm_bindgen]
pub fn boolean_case(case_json: &str) -> Result<String, JsError> {
    boolean_case_core(case_json).map_err(js)
}

#[wasm_bindgen]
impl Doc {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Doc {
        Doc::default()
    }

    /// Apply one op (JSON, the op-log format). Returns the outcome as JSON; a refusal is a
    /// thrown error with the kernel's message, and changes nothing.
    pub fn apply_op(&mut self, op_json: &str) -> Result<String, JsError> {
        self.core.apply_json(op_json).map_err(js)
    }

    pub fn undo(&mut self) -> Result<String, JsError> {
        self.core.undo().map_err(js)
    }

    pub fn redo(&mut self) -> Result<String, JsError> {
        self.core.redo().map_err(js)
    }

    /// View mesh of one object.
    pub fn get_view_mesh(&self, object_id: f64) -> Result<Mesh, JsError> {
        self.core
            .object_mesh(id(object_id)?)
            .map(|inner| Mesh { inner })
            .map_err(js)
    }

    /// View mesh of everything: visible objects, then sketch regions and edges.
    pub fn get_scene_mesh(&self) -> Result<Mesh, JsError> {
        self.core
            .scene_mesh()
            .map(|inner| Mesh { inner })
            .map_err(js)
    }

    /// Deterministic state hash (golden-hash format), hex.
    pub fn document_hash(&self) -> String {
        self.core.document().hash_hex()
    }

    /// Deterministic hash of the op log, hex.
    pub fn log_hash(&self) -> String {
        self.core.document().log_hash_hex()
    }

    /// The op log as a JSON array.
    pub fn op_log_json(&self) -> String {
        self.core.log_json()
    }

    /// Stats, undo/redo availability, object ids and tags, as JSON.
    pub fn state_json(&self) -> String {
        self.core.state_json()
    }

    /// Binary STL (mm, Z-up) of one object; refuses non-watertight solids.
    pub fn export_stl(&self, object_id: f64) -> Result<Vec<u8>, JsError> {
        self.core.stl(id(object_id)?).map_err(js)
    }

    /// Wavefront OBJ (mm, Z-up) of one object.
    pub fn export_obj(&self, object_id: f64) -> Result<String, JsError> {
        self.core.obj(id(object_id)?).map_err(js)
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct Mesh {
    inner: ViewMesh,
}

#[wasm_bindgen]
impl Mesh {
    /// xyz per render vertex (metres). Copied into a fresh Float32Array.
    pub fn positions(&self) -> Vec<f32> {
        self.inner.positions.clone()
    }
    pub fn indices(&self) -> Vec<u32> {
        self.inner.indices.clone()
    }
    /// One part index per triangle (see `parts_json`).
    pub fn face_ids(&self) -> Vec<u32> {
        self.inner.face_ids.clone()
    }
    pub fn edges(&self) -> Vec<f32> {
        self.inner.edges.clone()
    }
    /// The pickable parts (object faces, sketch regions) with their plane frames, as JSON.
    pub fn parts_json(&self) -> String {
        serde_json::to_string(&self.inner.parts).unwrap_or_else(|_| "[]".to_string())
    }
    pub fn triangle_count(&self) -> usize {
        self.inner.triangle_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(d: &mut DocCore, json: &str) -> serde_json::Value {
        serde_json::from_str(&d.apply_json(json).unwrap()).unwrap()
    }

    #[test]
    fn document_end_to_end() {
        let mut d = DocCore::default();
        let out = op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6096,"h":0.4064}"#,
        );
        assert_eq!(out["kind"], "sketch_edited");
        assert_eq!(out["regions"][0], 1);
        assert_eq!(d.scene_mesh().unwrap().triangle_count(), 2, "the region");
        let out = op(
            &mut d,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.3048}"#,
        );
        assert_eq!(out["object"], 1);
        let m = d.scene_mesh().unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(
            m.edges.len(),
            12 * 6,
            "12 box edges; the region left the sketch"
        );
        assert_eq!(m.parts.len(), 6);
        assert_eq!(d.object_mesh(1).unwrap().triangle_count(), 12);
        op(
            &mut d,
            r#"{"op":"imprint","object":1,"face":1,"points":[[0.15,0.1,0.3048],[0.45,0.1,0.3048],[0.45,0.3,0.3048],[0.15,0.3,0.3048]]}"#,
        );
        op(
            &mut d,
            r#"{"op":"push_pull","object":1,"face":6,"distance":0.1}"#,
        );
        let boss = d.document().hash_hex();
        op(
            &mut d,
            r#"{"op":"push_pull","object":1,"face":6,"distance":-0.5}"#,
        );
        let state: serde_json::Value = serde_json::from_str(&d.state_json()).unwrap();
        assert_eq!(state["stats"]["genus"], 1, "pushed through");
        assert_eq!(state["can_undo"], true);
        d.undo().unwrap();
        assert_eq!(d.document().hash_hex(), boss);
        d.undo().unwrap();
        d.redo().unwrap();
        assert_eq!(d.document().hash_hex(), boss);
        assert_eq!(replay_hash_core(&d.log_json()).unwrap(), boss);
        assert!(d.stl(1).unwrap().len() > 84);
        assert!(d.obj(1).unwrap().starts_with("# Sawhorse OBJ"));
        assert!(d.object_mesh(9).is_err());
    }

    #[test]
    fn refusals_are_messages() {
        let mut d = DocCore::default();
        op(
            &mut d,
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":1,"h":1}"#,
        );
        let before = d.document().hash_hex();
        let e = d
            .apply_json(r#"{"op":"extrude","sketch":1,"region":1,"distance":0}"#)
            .unwrap_err();
        assert!(e.contains("0.01 mm"), "{e}");
        assert!(
            d.apply_json(r#"{"op":"nope"}"#)
                .unwrap_err()
                .starts_with("bad op")
        );
        assert_eq!(d.document().hash_hex(), before);
        let mut fresh = DocCore::default();
        assert_eq!(fresh.undo().unwrap_err(), "nothing to undo");
    }

    #[test]
    fn replay_hash_matches_golden() {
        let text = include_str!("../../kernel/tests/golden/document_undo_redo.json");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let h = replay_hash_core(&v["ops"].to_string()).unwrap();
        assert_eq!(h, v["hash"].as_str().unwrap());
    }
}
