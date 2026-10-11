//! wasm-bindgen surface of the Sawhorse kernel, loaded by the front end's kernel Web Worker
//! in both the browser build and the Tauri webview (ADR-0001).
//!
//! Everything here is a thin wrapper over [`SceneCore`], which is plain Rust and tested
//! natively; the `#[wasm_bindgen]` layer only converts errors into `JsError` and vectors
//! into typed arrays.
//!
//! JS usage (see src/kernel/kernel.worker.ts):
//! ```js
//! const scene = new_scene();                       // model with a ground sketch
//! const region = scene.add_rectangle(0, 0, w, h);  // metres
//! scene.extrude(region, distance);                 // metres, negative = downward
//! const mesh = scene.get_mesh();                   // Float32Array / Uint32Array views
//! ```

use sawhorse_kernel::mesh::{ViewMesh, view_mesh};
use sawhorse_kernel::model::{Model, Op, OpOutcome};
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

/// Plain-Rust scene logic behind the JS class. Errors are strings (a refusal message).
#[derive(Debug, Clone)]
pub struct SceneCore {
    model: Model,
}

impl Default for SceneCore {
    fn default() -> Self {
        SceneCore {
            model: Model::with_ground_sketch(),
        }
    }
}

impl SceneCore {
    pub fn model(&self) -> &Model {
        &self.model
    }

    fn apply(&mut self, op: Op) -> Result<OpOutcome, String> {
        self.model.apply(&op).map_err(|e| e.to_string())
    }

    pub fn add_rectangle(
        &mut self,
        sketch: u32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<u32, String> {
        match self.apply(Op::AddRectangle { sketch, x, y, w, h })? {
            OpOutcome::RegionAdded(r) => Ok(r),
            other => Err(format!("unexpected outcome {other:?}")),
        }
    }

    pub fn extrude(&mut self, sketch: u32, region: u32, distance: f64) -> Result<u32, String> {
        match self.apply(Op::Extrude {
            sketch,
            region,
            distance,
        })? {
            OpOutcome::ObjectCreated(o) => Ok(o),
            other => Err(format!("unexpected outcome {other:?}")),
        }
    }

    pub fn push_pull(&mut self, object: u32, face: u32, distance: f64) -> Result<String, String> {
        Ok(
            match self.apply(Op::PushPull {
                object,
                face,
                distance,
            })? {
                OpOutcome::ObjectCollapsed => "collapsed".to_string(),
                _ => "moved".to_string(),
            },
        )
    }

    /// Apply one op given as JSON (the repro / golden format).
    pub fn apply_json(&mut self, op_json: &str) -> Result<String, String> {
        let op: Op = serde_json::from_str(op_json).map_err(|e| e.to_string())?;
        let out = self.apply(op)?;
        serde_json::to_string(&out).map_err(|e| e.to_string())
    }

    pub fn mesh(&self) -> Result<ViewMesh, String> {
        view_mesh(&self.model).map_err(|e| e.to_string())
    }

    pub fn stats_json(&self) -> String {
        serde_json::to_string(&self.model.stats()).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn stl(&self, object: u32) -> Result<Vec<u8>, String> {
        let o = self
            .model
            .object(object)
            .ok_or_else(|| format!("no object #{object}"))?;
        sawhorse_export::object_stl(o).map_err(|e| e.to_string())
    }
}

/// Hash of the model produced by replaying a JSON op list from empty (golden format).
pub fn replay_hash_core(ops_json: &str) -> Result<String, String> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| e.to_string())?;
    Model::replay(&ops)
        .map(|m| m.hash_hex())
        .map_err(|(i, e)| format!("op {i}: {e}"))
}

fn js(e: String) -> JsError {
    JsError::new(&e)
}

/// A kernel document: one ground sketch (sketch 0) to start.
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct Scene {
    core: SceneCore,
}

/// Create a new scene with a ground sketch.
#[wasm_bindgen]
pub fn new_scene() -> Scene {
    Scene::default()
}

/// Replay a JSON op list from an empty model and return its hash (golden cross-check).
#[wasm_bindgen]
pub fn replay_hash(ops_json: &str) -> Result<String, JsError> {
    replay_hash_core(ops_json).map_err(js)
}

#[wasm_bindgen]
impl Scene {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Scene {
        Scene::default()
    }

    /// Rectangle on the ground sketch, corner (x, y), size (w, h), metres. Returns the region index.
    pub fn add_rectangle(&mut self, x: f64, y: f64, w: f64, h: f64) -> Result<u32, JsError> {
        self.core.add_rectangle(0, x, y, w, h).map_err(js)
    }

    /// Rectangle on any sketch.
    pub fn add_rectangle_on(
        &mut self,
        sketch: u32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<u32, JsError> {
        self.core.add_rectangle(sketch, x, y, w, h).map_err(js)
    }

    /// Extrude a ground-sketch region by `distance` metres. Returns the object index.
    pub fn extrude(&mut self, region: u32, distance: f64) -> Result<u32, JsError> {
        self.core.extrude(0, region, distance).map_err(js)
    }

    /// Push/pull face `face` of object `object`. Returns "moved" or "collapsed".
    pub fn push_pull(&mut self, object: u32, face: u32, distance: f64) -> Result<String, JsError> {
        self.core.push_pull(object, face, distance).map_err(js)
    }

    /// Apply one op in the JSON repro format.
    pub fn apply_json(&mut self, op_json: &str) -> Result<String, JsError> {
        self.core.apply_json(op_json).map_err(js)
    }

    /// Render mesh: flat positions (f32 metres), u32 indices, one face id per triangle, edges.
    pub fn get_mesh(&self) -> Result<Mesh, JsError> {
        self.core.mesh().map(|inner| Mesh { inner }).map_err(js)
    }

    /// Counts, watertightness, Euler characteristic, genus, volume and hash, as JSON.
    pub fn stats_json(&self) -> String {
        self.core.stats_json()
    }

    pub fn hash(&self) -> String {
        self.core.model().hash_hex()
    }

    /// Binary STL (mm, Z-up) of one object; refuses non-watertight solids.
    pub fn export_stl(&self, object: u32) -> Result<Vec<u8>, JsError> {
        self.core.stl(object).map_err(js)
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
    pub fn face_ids(&self) -> Vec<u32> {
        self.inner.face_ids.clone()
    }
    pub fn edges(&self) -> Vec<f32> {
        self.inner.edges.clone()
    }
    pub fn triangle_count(&self) -> usize {
        self.inner.triangle_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spike_scene_end_to_end() {
        let mut s = SceneCore::default();
        let r = s.add_rectangle(0, 0.0, 0.0, 0.6096, 0.4064).unwrap();
        let o = s.extrude(0, r, 0.3048).unwrap();
        assert_eq!(o, 0);
        let m = s.mesh().unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.face_ids.len(), 12);
        assert_eq!(
            m.edges.len(),
            12 * 6,
            "12 box edges, sketch edges hidden under the solid"
        );
        let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
        assert_eq!(stats["watertight"], true);
        assert_eq!(stats["faces"], 6);
        assert_eq!(s.stl(0).unwrap().len(), 84 + 12 * 50);
    }

    #[test]
    fn refusals_are_messages() {
        let mut s = SceneCore::default();
        let r = s.add_rectangle(0, 0.0, 0.0, 1.0, 1.0).unwrap();
        let e = s.extrude(0, r, 0.0).unwrap_err();
        assert!(e.contains("0.01 mm"), "{e}");
        assert!(s.apply_json("{\"op\":\"nope\"}").is_err());
    }

    #[test]
    fn replay_hash_matches_golden() {
        let text = include_str!("../../kernel/tests/golden/spike_box.json");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let h = replay_hash_core(&v["ops"].to_string()).unwrap();
        assert_eq!(h, v["hash"].as_str().unwrap());
    }
}
