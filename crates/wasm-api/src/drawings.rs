//! Lane 12's part of the WASM surface: section planes (the cut the renderer fills and
//! outlines, which planes clip which solids) and, later, drawing sheets. Kept in its own
//! file so the rest of the surface stays as it was.

use sawhorse_kernel::document::Document;
use sawhorse_kernel::model::Op;
use wasm_bindgen::prelude::*;

use crate::{Doc, DocCore};

/// One active section's cut, ready for the renderer: the caps as world triangles (seen from
/// the side that is cut away) and the outline as world segments (6 numbers each).
#[derive(Debug, serde::Serialize)]
struct CutView {
    section: u64,
    positions: Vec<f64>,
    indices: Vec<u32>,
    outline: Vec<f64>,
    area: f64,
}

/// Solids, imported meshes and sketches clipped by the same active planes, each plane as
/// three.js wants it: `[nx, ny, nz, c]`, keeping `n . p + c >= 0`.
#[derive(Debug, serde::Serialize)]
struct ClipSet {
    planes: Vec<[f64; 4]>,
    objects: Vec<u64>,
    meshes: Vec<u64>,
    sketches: Vec<u64>,
}

#[derive(Debug, serde::Serialize)]
struct SectionView {
    cuts: Vec<CutView>,
    clips: Vec<ClipSet>,
}

impl DocCore {
    /// What the viewport needs to draw section cuts (lane 12): every active plane's caps and
    /// outline, and which planes clip what. Empty when the style turns section cuts off.
    pub fn section_view_json(&self) -> Result<String, String> {
        use std::collections::BTreeMap;
        let m = self.doc.model();
        let mut view = SectionView {
            cuts: Vec::new(),
            clips: Vec::new(),
        };
        if m.style().is_some_and(|s| !s.section_cuts) || m.sections().is_empty() {
            return serde_json::to_string(&view).map_err(|e| e.to_string());
        }
        for (id, s) in m.sections() {
            if !s.active {
                continue;
            }
            let cut = m.section_cut(id).map_err(|e| e.to_string())?;
            let (pos, tris) = cut.triangles();
            let outline = cut
                .outline
                .iter()
                .flat_map(|[a, b]| {
                    let (a, b) = (cut.to_world(*a), cut.to_world(*b));
                    [a.x, a.y, a.z, b.x, b.y, b.z]
                })
                .collect();
            view.cuts.push(CutView {
                section: id,
                positions: pos.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                indices: tris.into_iter().flatten().collect(),
                outline,
                area: cut.area,
            });
        }
        // (planes, kind, id): kind 0 a solid, 1 a mesh, 2 a sketch.
        let mut cut_by: Vec<(Vec<u64>, u8, u64)> = Vec::new();
        for (id, _) in m.objects() {
            cut_by.push((m.sections_cutting(id), 0, id));
        }
        for (id, x) in m.meshes() {
            cut_by.push((m.active_sections_over(x.member.parent), 1, id));
        }
        for id in m.sketch_ids() {
            let parent = m.sketch_parent(id).map(sawhorse_kernel::ids::from_public);
            cut_by.push((m.active_sections_over(parent), 2, id));
        }
        let mut sets: BTreeMap<Vec<u64>, ClipSet> = BTreeMap::new();
        for (planes, kind, id) in cut_by {
            if planes.is_empty() {
                continue;
            }
            let c = sets.entry(planes.clone()).or_insert_with(|| ClipSet {
                planes: planes
                    .iter()
                    .filter_map(|&k| m.section(k))
                    .map(|x| [x.normal.x, x.normal.y, x.normal.z, -x.normal.dot(x.origin)])
                    .collect(),
                objects: Vec::new(),
                meshes: Vec::new(),
                sketches: Vec::new(),
            });
            match kind {
                0 => c.objects.push(id),
                1 => c.meshes.push(id),
                _ => c.sketches.push(id),
            }
        }
        view.clips = sets.into_values().collect();
        serde_json::to_string(&view).map_err(|e| e.to_string())
    }
}

impl DocCore {
    /// The document's drawing sheets in order (lane 12): `[{id, sheet}]`, each sheet with
    /// its setup and items as the kernel stores them.
    pub fn sheets_json(&self) -> String {
        #[derive(serde::Serialize)]
        struct Row<'a> {
            id: u64,
            sheet: &'a sawhorse_kernel::sheets::Sheet,
        }
        let m = self.doc.model();
        let rows: Vec<Row> = m
            .sheets()
            .into_iter()
            .map(|(id, sheet)| Row { id, sheet })
            .collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }
}

#[wasm_bindgen]
impl Doc {
    /// See [`DocCore::section_view_json`].
    pub fn section_view(&self) -> Result<String, JsError> {
        self.core.section_view_json().map_err(|e| JsError::new(&e))
    }

    /// See [`DocCore::sheets_json`].
    pub fn sheets(&self) -> String {
        self.core.sheets_json()
    }
}

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
    fn the_section_view_has_caps_outlines_and_clip_sets() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../kernel/tests/golden/section.json");
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
        let core = DocCore::from_document(Document::replay(&ops).unwrap());
        let v: serde_json::Value =
            serde_json::from_str(&core.section_view_json().unwrap()).unwrap();
        let cuts = v["cuts"].as_array().unwrap();
        assert_eq!(cuts.len(), 1, "one active plane");
        assert!(!cuts[0]["indices"].as_array().unwrap().is_empty());
        assert_eq!(cuts[0]["outline"].as_array().unwrap().len() % 6, 0);
        let clips = v["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 1);
        // Every solid of the bookshelf is cut by the model's plane.
        assert_eq!(
            clips[0]["objects"].as_array().unwrap().len(),
            core.document().model().objects().len()
        );
        // The style switch turns it all off.
        let mut d = core.document().clone();
        d.apply(&Op::SetStyle {
            style: Some(sawhorse_kernel::looks::Style {
                section_cuts: false,
                ..Default::default()
            }),
        })
        .unwrap();
        let off: serde_json::Value =
            serde_json::from_str(&DocCore::from_document(d).section_view_json().unwrap()).unwrap();
        assert!(off["cuts"].as_array().unwrap().is_empty());
        assert!(off["clips"].as_array().unwrap().is_empty());
    }

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
