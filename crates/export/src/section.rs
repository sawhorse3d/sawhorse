//! A section plane's slice as a flat drawing (lane 12, File > Export > Section slice): the
//! caps the plane cuts through the solids it reaches, in the plane's own frame, millimetres,
//! 1:1, moved so the lower-left corner of the slice is at the origin.
//!
//! - [`svg`]: every cap filled (even-odd, so holes stay holes) in the style's section fill
//!   colour, and the cut's outline stroked.
//! - [`dxf`]: one closed polyline per ring (outer rings and holes), the same entities the
//!   face outline export writes, so any CAD or CAM program reads it.

use std::fmt::Write as _;

use glam::DVec2;
use sawhorse_kernel::Model;
use sawhorse_kernel::section::SectionCut;

use crate::ExportError;
use crate::dxf::{OutlineFlavour, normalised_rings};

/// The default section fill (a light warm grey) when the style does not set one.
pub const DEFAULT_FILL: [u8; 3] = [167, 158, 144];

/// Every ring of every cap, in the plane's frame (metres): outers and holes.
pub fn rings(cut: &SectionCut) -> Vec<Vec<DVec2>> {
    cut.caps
        .iter()
        .flat_map(|c| std::iter::once(&c.outer).chain(&c.holes))
        .map(|r| r.iter().map(|p| DVec2::from_array(*p)).collect())
        .collect()
}

fn cut_of(model: &Model, section: u64) -> Result<SectionCut, ExportError> {
    let cut = model
        .section_cut(section)
        .map_err(|e| ExportError::BadView(e.to_string()))?;
    if cut.caps.is_empty() {
        return Err(ExportError::Empty);
    }
    Ok(cut)
}

/// The slice as SVG (see the module docs).
pub fn svg(model: &Model, section: u64) -> Result<String, ExportError> {
    let cut = cut_of(model, section)?;
    let name = model
        .section(section)
        .map_or("Section", |s| s.name.as_str());
    let style = model.style().cloned().unwrap_or_default();
    let fill = style.section_fill_color.unwrap_or(DEFAULT_FILL);
    let (rings, size) = normalised_rings(&rings(&cut))?;
    let mut d = String::new();
    for r in &rings {
        for (k, p) in r.iter().enumerate() {
            let _ = write!(
                d,
                "{}{:.4} {:.4}",
                if k == 0 { 'M' } else { 'L' },
                p.x,
                size.y - p.y
            );
        }
        d.push('Z');
    }
    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.4}mm" height="{h:.4}mm" viewBox="0 0 {w:.4} {h:.4}">"#,
        w = size.x,
        h = size.y
    );
    let _ = writeln!(
        out,
        "  <title>Sawhorse section slice ({}), millimetres, 1:1</title>",
        escape(name)
    );
    let _ = writeln!(
        out,
        r##"  <path class="caps" d="{d}" fill="#{:02x}{:02x}{:02x}" fill-rule="evenodd" stroke="#000" stroke-width="0.35" stroke-linejoin="round"/>"##,
        fill[0], fill[1], fill[2]
    );
    out.push_str("</svg>\n");
    Ok(out)
}

/// The slice as DXF: closed polylines, millimetres (see the module docs).
pub fn dxf(model: &Model, section: u64, flavour: OutlineFlavour) -> Result<String, ExportError> {
    let cut = cut_of(model, section)?;
    crate::dxf::outline(&rings(&cut), flavour)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::model::{Model, Op, OpOutcome};

    /// A 2 x 1 x 1 m box with a 0.5 m square hole through it along x.
    fn tube() -> Model {
        let mut m = Model::with_ground_sketch();
        let draw = |m: &mut Model, pts: Vec<[f64; 3]>| -> OpOutcome {
            m.apply(&Op::Draw {
                target: sawhorse_kernel::model::DrawTarget::Plane {
                    origin: [0.0; 3],
                    normal: [1.0, 0.0, 0.0],
                },
                points: pts,
                closed: true,
            })
            .unwrap()
        };
        let OpOutcome::Drawn {
            sketch: Some(sk),
            inside: Some(outer),
            ..
        } = draw(
            &mut m,
            vec![
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
        )
        else {
            panic!()
        };
        draw(
            &mut m,
            vec![
                [0.0, 0.25, 0.25],
                [0.0, 0.75, 0.25],
                [0.0, 0.75, 0.75],
                [0.0, 0.25, 0.75],
            ],
        );
        m.apply(&Op::Extrude {
            sketch: sk,
            region: outer,
            distance: 2.0,
        })
        .unwrap();
        m
    }

    #[test]
    fn a_tube_slice_is_a_square_with_a_hole() {
        let mut m = tube();
        let OpOutcome::SectionAdded { section } = m
            .apply(&Op::AddSection {
                origin: [1.0, 0.0, 0.0],
                normal: [-1.0, 0.0, 0.0],
                name: Some("Middle <cut>".into()),
                active: true,
                hidden: false,
            })
            .unwrap()
        else {
            panic!()
        };
        let s = svg(&m, section).unwrap();
        assert!(
            s.contains(r#"width="1000.0000mm" height="1000.0000mm""#),
            "{s}"
        );
        assert!(s.contains("Middle &lt;cut&gt;"));
        assert!(s.contains(r#"fill-rule="evenodd""#));
        assert_eq!(s.matches('Z').count(), 2, "outer and hole");
        let d = dxf(&m, section, OutlineFlavour::Lwpolyline).unwrap();
        assert_eq!(d.matches("LWPOLYLINE").count(), 2);
        let r12 = dxf(&m, section, OutlineFlavour::R12).unwrap();
        assert_eq!(r12.matches("SEQEND").count(), 2);
        // A plane that cuts nothing exports nothing.
        let OpOutcome::SectionAdded { section: far } = m
            .apply(&Op::AddSection {
                origin: [9.0, 0.0, 0.0],
                normal: [-1.0, 0.0, 0.0],
                name: None,
                active: false,
                hidden: false,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(svg(&m, far), Err(ExportError::Empty));
        assert!(svg(&m, 999).is_err());
    }

    /// The slice's DXF is closed shapes, never loose LINE entities (the 2026-10-12 verifier
    /// looked for LINEs and found only polylines): one closed LWPOLYLINE per ring, outer
    /// outlines and holes alike, so a CAM program sees a part with a hole, 1:1 in mm. The
    /// rings' areas are the cap's: a 1 m square less its 0.5 m square hole.
    #[test]
    fn the_slice_dxf_is_one_closed_polyline_per_ring_and_no_lines() {
        let mut m = tube();
        let OpOutcome::SectionAdded { section } = m
            .apply(&Op::AddSection {
                origin: [1.0, 0.0, 0.0],
                normal: [-1.0, 0.0, 0.0],
                name: None,
                active: true,
                hidden: false,
            })
            .unwrap()
        else {
            panic!()
        };
        let d = dxf(&m, section, OutlineFlavour::Lwpolyline).unwrap();
        let p = crate::dxf::pairs(&d);
        let at = p
            .iter()
            .position(|(c, v)| *c == 2 && v == "ENTITIES")
            .unwrap();
        let kinds: Vec<&str> = p[at..]
            .iter()
            .filter(|(c, v)| *c == 0 && v != "ENDSEC" && v != "EOF")
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(kinds, ["LWPOLYLINE", "LWPOLYLINE"], "no LINE entities");
        // Each polyline: closed (70 = 1), four vertices, its area by the shoelace.
        let mut areas = Vec::new();
        for chunk in p[at..].split(|(c, v)| *c == 0 && v == "LWPOLYLINE").skip(1) {
            let flag = chunk.iter().find(|(c, _)| *c == 70).unwrap();
            assert_eq!(flag.1, "1", "closed");
            let n = chunk.iter().find(|(c, _)| *c == 90).unwrap();
            assert_eq!(n.1, "4");
            let xs: Vec<f64> = chunk
                .iter()
                .filter(|(c, _)| *c == 10)
                .map(|(_, v)| v.parse().unwrap())
                .collect();
            let ys: Vec<f64> = chunk
                .iter()
                .filter(|(c, _)| *c == 20)
                .map(|(_, v)| v.parse().unwrap())
                .collect();
            let a: f64 = (0..xs.len())
                .map(|i| {
                    let j = (i + 1) % xs.len();
                    xs[i] * ys[j] - xs[j] * ys[i]
                })
                .sum::<f64>()
                / 2.0;
            areas.push(a.abs());
        }
        assert_eq!(areas.len(), 2);
        assert!((areas[0] - 1.0e6).abs() < 1e-6, "outer, mm²: {areas:?}");
        assert!((areas[1] - 0.25e6).abs() < 1e-6, "hole, mm²: {areas:?}");
    }
}
