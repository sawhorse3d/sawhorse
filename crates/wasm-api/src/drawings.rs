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

/// A viewport's drawing as snap segments, by (sheet, item) (see `DocCore::snaps`).
pub(crate) type SnapCache = ((u64, u64), Vec<sawhorse_inference::flat::FlatSeg>);

impl DocCore {
    /// Sheet `sheet` laid out (lane 12): every primitive in paper metres, what the canvas
    /// and every export draw (see `sawhorse_export::sheet`).
    pub fn sheet_render_json(&self, sheet: u64) -> Result<String, String> {
        let m = self.doc.model();
        let order = m.sheets();
        let number = order
            .iter()
            .position(|(id, _)| *id == sheet)
            .ok_or(format!("no sheet #{sheet}"))?
            + 1;
        let r = sawhorse_export::sheet::render(
            m,
            sheet,
            &sawhorse_export::sheet::RenderOptions {
                number,
                count: order.len(),
            },
        )
        .ok_or(format!("no sheet #{sheet}"))?;
        serde_json::to_string(&r).map_err(|e| e.to_string())
    }

    /// Sheet `sheet` as DXF in its own units (see `sawhorse_export::sheet_dxf`).
    pub fn sheet_dxf_text(&self, sheet: u64) -> Result<String, String> {
        let m = self.doc.model();
        let order = m.sheets();
        let number = order
            .iter()
            .position(|(id, _)| *id == sheet)
            .ok_or(format!("no sheet #{sheet}"))?
            + 1;
        let units = m
            .sheet(sheet)
            .ok_or(format!("no sheet #{sheet}"))?
            .setup
            .units;
        let r = sawhorse_export::sheet::render(
            m,
            sheet,
            &sawhorse_export::sheet::RenderOptions {
                number,
                count: order.len(),
            },
        )
        .ok_or(format!("no sheet #{sheet}"))?;
        Ok(sawhorse_export::sheet_dxf::dxf(&r, units))
    }

    /// Where a cursor at paper point (`x`, `y`) snaps on viewport `item` of sheet `sheet`,
    /// within `radius` paper metres: `{kind, at, point}` (the model point) or null.
    pub fn sheet_snap_json(
        &self,
        sheet: u64,
        item: u64,
        x: f64,
        y: f64,
        radius: f64,
    ) -> Result<String, String> {
        use sawhorse_inference::flat::{FlatSeg, snap};
        let m = self.doc.model();
        let key = (sheet, item);
        if self.snaps.borrow().as_ref().is_none_or(|(k, _)| *k != key) {
            let sh = m.sheet(sheet).ok_or(format!("no sheet #{sheet}"))?;
            let it = sh.item(item).ok_or(format!("no item #{item}"))?;
            let (d, info) = sawhorse_export::sheet::viewport_drawing(m, &it.kind, item)
                .ok_or("that item is not a viewport")?;
            let segs: Vec<FlatSeg> = d
                .lines
                .iter()
                .filter(|l| l.kind != sawhorse_export::drawing::LineKind::Hidden)
                .map(|l| FlatSeg {
                    a: info.paper(glam::DVec2::from_array(l.a)),
                    b: info.paper(glam::DVec2::from_array(l.b)),
                    a3: glam::DVec3::from_array(l.a3),
                    b3: glam::DVec3::from_array(l.b3),
                })
                .collect();
            self.snaps.replace(Some((key, segs)));
        }
        let cache = self.snaps.borrow();
        let segs = &cache.as_ref().expect("filled above").1;
        let s = snap(segs, glam::DVec2::new(x, y), radius);
        serde_json::to_string(&s).map_err(|e| e.to_string())
    }
}

impl DocCore {
    /// How a viewport item (`item_json`, an `ItemKind`, not yet on any sheet) would draw:
    /// `{factor, size, to_scale, problem?}`, the factor that fits its drawing in its rect
    /// (what NTS uses; the UI picks the standard scale under it) and the drawing's size in
    /// model metres.
    pub fn viewport_fit_json(&self, item_json: &str) -> Result<String, String> {
        let kind: sawhorse_kernel::sheets::ItemKind =
            serde_json::from_str(item_json).map_err(|e| e.to_string())?;
        let sawhorse_kernel::sheets::ItemKind::Viewport { rect, source, .. } = &kind else {
            return Err("that item is not a viewport".into());
        };
        let fitted = sawhorse_kernel::sheets::ItemKind::Viewport {
            rect: *rect,
            source: source.clone(),
            scale: sawhorse_kernel::paper::Scale::Nts,
            centre: None,
            hidden_lines: false,
            caption: None,
            border: false,
        };
        let (d, info) = sawhorse_export::sheet::viewport_drawing(self.doc.model(), &fitted, 0)
            .ok_or("that item is not a viewport")?;
        let b = d.bounds.unwrap_or([0.0; 4]);
        let out = serde_json::json!({
            "factor": info.factor,
            "size": [b[2] - b[0], b[3] - b[1]],
            "empty": d.bounds.is_none(),
            "problem": info.problem,
        });
        Ok(out.to_string())
    }
}

/// Each sheet's render hash, in order, of the document produced by replaying a JSON op log
/// (the golden cross-check: they must match the native ones).
pub fn replay_sheet_renders_core(ops_json: &str) -> Result<String, String> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).map_err(|e| e.to_string())?;
    let d = Document::replay(&ops).map_err(|(i, e)| format!("op {i}: {e}"))?;
    let m = d.model();
    let n = m.sheets().len();
    let hashes: Vec<String> = m
        .sheets()
        .iter()
        .enumerate()
        .filter_map(|(k, (id, _))| {
            sawhorse_export::sheet::render(
                m,
                *id,
                &sawhorse_export::sheet::RenderOptions {
                    number: k + 1,
                    count: n,
                },
            )
            .map(|r| r.hash)
        })
        .collect();
    serde_json::to_string(&hashes).map_err(|e| e.to_string())
}

/// See [`replay_sheet_renders_core`].
#[wasm_bindgen]
pub fn replay_sheet_renders(ops_json: &str) -> Result<String, JsError> {
    replay_sheet_renders_core(ops_json).map_err(|e| JsError::new(&e))
}

fn js_id(x: f64) -> Result<u64, JsError> {
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < 9_007_199_254_740_992.0 {
        Ok(x as u64)
    } else {
        Err(JsError::new(&format!("bad id {x}")))
    }
}

#[wasm_bindgen]
impl Doc {
    /// See [`DocCore::sheet_render_json`].
    pub fn sheet_render(&self, sheet: f64) -> Result<String, JsError> {
        self.core
            .sheet_render_json(js_id(sheet)?)
            .map_err(|e| JsError::new(&e))
    }

    /// See [`DocCore::sheet_dxf_text`].
    pub fn sheet_dxf(&self, sheet: f64) -> Result<String, JsError> {
        self.core
            .sheet_dxf_text(js_id(sheet)?)
            .map_err(|e| JsError::new(&e))
    }

    /// See [`DocCore::viewport_fit_json`].
    pub fn viewport_fit(&self, item_json: &str) -> Result<String, JsError> {
        self.core
            .viewport_fit_json(item_json)
            .map_err(|e| JsError::new(&e))
    }

    /// See [`DocCore::sheet_snap_json`].
    pub fn sheet_snap(
        &self,
        sheet: f64,
        item: f64,
        x: f64,
        y: f64,
        radius: f64,
    ) -> Result<String, JsError> {
        self.core
            .sheet_snap_json(js_id(sheet)?, js_id(item)?, x, y, radius)
            .map_err(|e| JsError::new(&e))
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
    fn the_sheet_golden_renders_and_snaps() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../kernel/tests/golden/sheet.json");
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let hashes: Vec<String> =
            serde_json::from_str(&replay_sheet_renders_core(&g["ops"].to_string()).unwrap())
                .unwrap();
        let want: Vec<String> = serde_json::from_value(g["renders"].clone()).unwrap();
        assert_eq!(hashes, want);
        let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
        let core = DocCore::from_document(Document::replay(&ops).unwrap());
        let r: serde_json::Value =
            serde_json::from_str(&core.sheet_render_json(1).unwrap()).unwrap();
        assert_eq!(r["hash"].as_str().unwrap(), want[0]);
        // The front elevation's bottom-right corner (36 in along red; the left is cut away by
        // the active section plane).
        let vp = &r["viewports"][0];
        let (cx, cy) = (
            vp["rect"][0].as_f64().unwrap() + vp["rect"][2].as_f64().unwrap() / 2.0,
            vp["rect"][1].as_f64().unwrap() + vp["rect"][3].as_f64().unwrap() / 2.0,
        );
        let f = vp["factor"].as_f64().unwrap();
        let (ox, oy) = (
            cx + (36.0 * 0.0254 - vp["centre"][0].as_f64().unwrap()) * f,
            cy + vp["centre"][1].as_f64().unwrap() * f,
        );
        let s: serde_json::Value = serde_json::from_str(
            &core
                .sheet_snap_json(1, 1, ox - 0.0003, oy - 0.0003, 0.002)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(s["kind"], "endpoint");
        let p = &s["point"];
        assert!(
            (p[0].as_f64().unwrap() - 36.0 * 0.0254).abs() < 1e-9
                && p[2].as_f64().unwrap().abs() < 1e-9,
            "{s}"
        );
        assert!(core.sheet_snap_json(1, 99, 0.0, 0.0, 0.002).is_err());
        // A front view fitted into 4 x 3 in: 92% of the rect, the tighter way.
        let fit: serde_json::Value = serde_json::from_str(
            &core
                .viewport_fit_json(r#"{"kind":"viewport","rect":[0.0,0.0,0.1016,0.0762],"source":{"from":"standard","view":"front"},"scale":{"kind":"nts"}}"#)
                .unwrap(),
        )
        .unwrap();
        let (sw, sh) = (
            fit["size"][0].as_f64().unwrap(),
            fit["size"][1].as_f64().unwrap(),
        );
        assert!((sh - 72.0 * 0.0254).abs() < 1e-9, "{fit}");
        assert!(
            (fit["factor"].as_f64().unwrap() - 0.92 * (0.1016 / sw).min(0.0762 / sh)).abs() < 1e-12,
            "{fit}"
        );
        assert!(
            core.viewport_fit_json(r#"{"kind":"line","a":[0,0],"b":[1,1],"width":0.001}"#)
                .is_err()
        );
        // The sheet as DXF, in inches, with its dimension strings.
        let dxf = core.sheet_dxf_text(1).unwrap();
        assert!(
            dxf.contains(
                "$INSUNITS
 70
1
"
            ),
            "{}",
            &dxf[..200]
        );
        assert!(
            dxf.contains(
                "
36\"
"
            ) && dxf.ends_with(
                "EOF
"
            )
        );
        assert!(core.sheet_dxf_text(7).is_err());
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
