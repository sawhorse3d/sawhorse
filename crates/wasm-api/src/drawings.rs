//! Lane 12's part of the WASM surface: section planes (the cut the renderer fills and
//! outlines, which planes clip which solids) and, later, drawing sheets. Kept in its own
//! file so the rest of the surface stays as it was.

use sawhorse_kernel::document::Document;
use sawhorse_kernel::model::Op;
use wasm_bindgen::prelude::*;

/// Each section plane's cut hash, in id order, of the document produced by replaying a JSON
/// op log (the golden cross-check: they must match the native ones).
pub fn replay_sections_core(ops_json: &str) -> Result<String, String> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| e.to_string())?;
    let d = Document::replay(&ops).map_err(|(i, e)| format!("op {i}: {e}"))?;
    let m = d.model();
    let hashes = m
        .sections()
        .into_iter()
        .map(|(id, _)| m.section_cut(id).map(|c| c.hash))
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&hashes).map_err(|e| e.to_string())
}

/// See [`replay_sections_core`].
#[wasm_bindgen]
pub fn replay_sections(ops_json: &str) -> Result<String, JsError> {
    replay_sections_core(ops_json).map_err(|e| JsError::new(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_golden_replays_with_its_cut_hashes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../kernel/tests/golden/section.json");
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let got: Vec<String> =
            serde_json::from_str(&replay_sections_core(&g["ops"].to_string()).unwrap()).unwrap();
        let want: Vec<String> = serde_json::from_value(g["sections"].clone()).unwrap();
        assert_eq!(got, want);
    }
}
