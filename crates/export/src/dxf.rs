//! DXF, ASCII, millimetres.
//!
//! - [`document`]: the model for CAD, R12-compatible (`$ACADVER` AC1009, `LTYPE` and
//!   `LAYER` tables, a layer per object): every face as `3DFACE` triangles (or one
//!   quadrilateral where the face is a plain convex quad) with all four edge-visibility bits
//!   set, and every real edge of the model as a `LINE`, so a reader shows exactly the
//!   model's edges and none of the triangulation.
//! - [`outline`]: the 2D outline of one face (or sketch region) for laser cutters and CNC,
//!   in the face's own plane, moved so its lower-left corner is at the origin: one closed
//!   polyline per ring (outer first, then holes). [`OutlineFlavour::Lwpolyline`] writes
//!   `LWPOLYLINE`, which every laser and CAM program reads; [`OutlineFlavour::R12`] writes
//!   `POLYLINE` / `VERTEX` / `SEQEND` for strict R12-only readers.
//!
//! `$INSUNITS` = 4 (millimetres) in both.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};
use sawhorse_kernel::Model;
use sawhorse_tessellate::triangulate;

use crate::{ExportError, MM_PER_M, Scope, object_name};

/// Which polyline entity the 2D outline uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutlineFlavour {
    #[default]
    Lwpolyline,
    R12,
}

pub(crate) struct Dxf(pub(crate) String);

impl Dxf {
    pub(crate) fn pair(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = write!(self.0, "{code:>3}\n{value}\n");
    }
    fn xyz(&mut self, base: i32, p: DVec3) {
        let p = p * MM_PER_M;
        self.pair(base, num(p.x));
        self.pair(base + 10, num(p.y));
        self.pair(base + 20, num(p.z));
    }
}

/// A number as DXF wants it: plain decimal, no exponent, `-0` written as `0`.
pub(crate) fn num(x: f64) -> String {
    let x = if x == 0.0 { 0.0 } else { x };
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

/// A layer name DXF accepts: no `<>/\":;?*|=,` and backquote; at most 255 characters.
pub fn layer_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if "<>/\\\":;?*|=,`".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(255)
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() { "0".into() } else { s }
}

fn header(d: &mut Dxf, r12: bool) {
    d.pair(0, "SECTION");
    d.pair(2, "HEADER");
    if r12 {
        d.pair(9, "$ACADVER");
        d.pair(1, "AC1009");
    }
    d.pair(9, "$INSUNITS");
    d.pair(70, 4);
    d.pair(0, "ENDSEC");
}

fn tables(d: &mut Dxf, layers: &[String]) {
    d.pair(0, "SECTION");
    d.pair(2, "TABLES");
    d.pair(0, "TABLE");
    d.pair(2, "LTYPE");
    d.pair(70, 1);
    d.pair(0, "LTYPE");
    d.pair(2, "CONTINUOUS");
    d.pair(70, 0);
    d.pair(3, "Solid line");
    d.pair(72, 65);
    d.pair(73, 0);
    d.pair(40, "0.0");
    d.pair(0, "ENDTAB");
    d.pair(0, "TABLE");
    d.pair(2, "LAYER");
    d.pair(70, layers.len() + 1);
    for name in std::iter::once("0").chain(layers.iter().map(String::as_str)) {
        d.pair(0, "LAYER");
        d.pair(2, name);
        d.pair(70, 0);
        d.pair(62, 7);
        d.pair(6, "CONTINUOUS");
    }
    d.pair(0, "ENDTAB");
    d.pair(0, "ENDSEC");
}

fn face3d(d: &mut Dxf, layer: &str, corners: [DVec3; 4]) {
    d.pair(0, "3DFACE");
    d.pair(8, layer);
    for (k, p) in corners.iter().enumerate() {
        d.xyz(10 + k as i32, *p);
    }
    // All four edges invisible: the model's edges are drawn as LINEs.
    d.pair(70, 15);
}

fn line(d: &mut Dxf, layer: &str, a: DVec3, b: DVec3) {
    d.pair(0, "LINE");
    d.pair(8, layer);
    d.xyz(10, a);
    d.xyz(11, b);
}

/// True for a convex planar quad with no holes (written as one 3DFACE).
fn convex_quad(r: &[DVec3], normal: DVec3) -> bool {
    r.len() == 4
        && (0..4).all(|k| {
            let (a, b, c) = (r[k], r[(k + 1) % 4], r[(k + 2) % 4]);
            (b - a).cross(c - b).dot(normal) > 0.0
        })
}

/// Faces of one polygon (outer + holes, world points) as 3DFACEs.
fn polygon(
    d: &mut Dxf,
    layer: &str,
    rings: &[Vec<DVec3>],
    plane: &sawhorse_kernel::plane::Plane,
) -> Result<(), ExportError> {
    if rings.len() == 1 && convex_quad(&rings[0], plane.normal) {
        let r = &rings[0];
        face3d(d, layer, [r[0], r[1], r[2], r[3]]);
        return Ok(());
    }
    let flat: Vec<DVec3> = rings.iter().flatten().copied().collect();
    let to2 = |r: &Vec<DVec3>| -> Vec<DVec2> { r.iter().map(|&p| plane.to_local(p)).collect() };
    let holes: Vec<Vec<DVec2>> = rings[1..].iter().map(to2).collect();
    let tris =
        triangulate(&to2(&rings[0]), &holes).map_err(|e| ExportError::Tessellate(e.to_string()))?;
    for t in tris {
        let [a, b, c] = t.map(|k| flat[k as usize]);
        face3d(d, layer, [a, b, c, c]);
    }
    Ok(())
}

/// The model (solids and imported meshes in `scope`) as R12 3DFACEs and LINEs.
pub fn document(model: &Model, scope: &Scope) -> Result<String, ExportError> {
    let solids = scope.solids(model)?;
    let loose = scope.loose(model)?;
    if solids.is_empty() && loose.is_empty() {
        return Err(ExportError::Empty);
    }
    let mut layers: Vec<String> = Vec::new();
    let mut layer_for = |name: String| -> String {
        let base = layer_name(&name);
        let mut n = base.clone();
        let mut k = 2;
        while layers.contains(&n) {
            n = format!("{base}_{k}");
            k += 1;
        }
        layers.push(n.clone());
        n
    };
    let mut body = Dxf(String::new());
    for (id, o) in &solids {
        let layer = layer_for(object_name(model, *id));
        let b = o.brep();
        for (_, f) in b.faces() {
            let rings: Vec<Vec<DVec3>> = f.loops().map(|l| b.loop_points(l)).collect();
            polygon(&mut body, &layer, &rings, &f.plane)?;
        }
        for (e, _) in b.edges() {
            let (p, q) = b.edge_points(e);
            line(&mut body, &layer, p, q);
        }
    }
    for (_, m) in &loose {
        let layer = layer_for(m.name.clone());
        let mut drawn = std::collections::BTreeSet::new();
        for (fi, f) in m.faces.iter().enumerate() {
            let rings: Vec<Vec<DVec3>> = f
                .iter()
                .map(|r| r.iter().map(|&i| m.positions[i as usize]).collect())
                .collect();
            if let Some(plane) = m.plane(fi) {
                // A face of a non-solid mesh that will not triangulate keeps only its edges.
                let _ = polygon(&mut body, &layer, &rings, &plane);
            }
            for r in f {
                for k in 0..r.len() {
                    let (a, b) = (r[k], r[(k + 1) % r.len()]);
                    if drawn.insert((a.min(b), a.max(b))) {
                        line(
                            &mut body,
                            &layer,
                            m.positions[a as usize],
                            m.positions[b as usize],
                        );
                    }
                }
            }
        }
        for l in &m.lines {
            if drawn.insert((l[0].min(l[1]), l[0].max(l[1]))) {
                line(
                    &mut body,
                    &layer,
                    m.positions[l[0] as usize],
                    m.positions[l[1] as usize],
                );
            }
        }
    }
    let mut d = Dxf(String::new());
    header(&mut d, true);
    tables(&mut d, &layers);
    d.pair(0, "SECTION");
    d.pair(2, "ENTITIES");
    d.0.push_str(&body.0);
    d.pair(0, "ENDSEC");
    d.pair(0, "EOF");
    Ok(d.0)
}

/// Rings (metres, in the face's plane frame; outer first) moved so the lower-left corner of
/// their bounding box is the origin, in millimetres. Shared with the SVG outline.
pub fn normalised_rings(rings: &[Vec<DVec2>]) -> Result<(Vec<Vec<DVec2>>, DVec2), ExportError> {
    if rings.is_empty()
        || rings.iter().any(|r| r.len() < 3)
        || rings.iter().flatten().any(|p| !p.is_finite())
    {
        return Err(ExportError::BadOutline);
    }
    let lo = rings
        .iter()
        .flatten()
        .fold(DVec2::splat(f64::INFINITY), |m, &p| m.min(p));
    let hi = rings
        .iter()
        .flatten()
        .fold(DVec2::splat(f64::NEG_INFINITY), |m, &p| m.max(p));
    let out = rings
        .iter()
        .map(|r| r.iter().map(|&p| (p - lo) * MM_PER_M).collect())
        .collect();
    Ok((out, (hi - lo) * MM_PER_M))
}

/// The 2D outline of a face for cutting (see the module docs).
pub fn outline(rings: &[Vec<DVec2>], flavour: OutlineFlavour) -> Result<String, ExportError> {
    let (rings, _) = normalised_rings(rings)?;
    let r12 = flavour == OutlineFlavour::R12;
    let mut d = Dxf(String::new());
    header(&mut d, r12);
    if r12 {
        tables(&mut d, &["OUTLINE".to_string()]);
    }
    d.pair(0, "SECTION");
    d.pair(2, "ENTITIES");
    for r in &rings {
        if r12 {
            d.pair(0, "POLYLINE");
            d.pair(8, "OUTLINE");
            d.pair(66, 1);
            d.pair(10, "0");
            d.pair(20, "0");
            d.pair(30, "0");
            d.pair(70, 1);
            for p in r {
                d.pair(0, "VERTEX");
                d.pair(8, "OUTLINE");
                d.pair(10, num(p.x));
                d.pair(20, num(p.y));
                d.pair(30, "0");
            }
            d.pair(0, "SEQEND");
            d.pair(8, "OUTLINE");
        } else {
            d.pair(0, "LWPOLYLINE");
            d.pair(100, "AcDbEntity");
            d.pair(8, "OUTLINE");
            d.pair(100, "AcDbPolyline");
            d.pair(90, r.len());
            d.pair(70, 1);
            for p in r {
                d.pair(10, num(p.x));
                d.pair(20, num(p.y));
            }
        }
    }
    d.pair(0, "ENDSEC");
    d.pair(0, "EOF");
    Ok(d.0)
}

/// Group-code pairs of a DXF text (tests and the importer's sanity checks).
pub fn pairs(text: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|c| Some((c[0].trim().parse().ok()?, c[1].trim().to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn entities(text: &str) -> Vec<String> {
        let p = pairs(text);
        let start = p
            .iter()
            .position(|(c, v)| *c == 2 && v == "ENTITIES")
            .unwrap();
        p[start..]
            .iter()
            .filter(|(c, _)| *c == 0)
            .map(|(_, v)| v.clone())
            .take_while(|v| v != "ENDSEC")
            .collect()
    }

    #[test]
    fn model_is_faces_and_edges_on_object_layers() {
        let m = fixtures::mixed_model();
        let text = document(&m, &Scope::visible(&m)).unwrap();
        assert!(text.starts_with("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1009\n"));
        assert!(text.ends_with("  0\nEOF\n"));
        let e = entities(&text);
        // Two boxes: 6 quads + 12 edges each; the sheet: 1 quad + 4 edges.
        assert_eq!(e.iter().filter(|v| *v == "3DFACE").count(), 13);
        assert_eq!(e.iter().filter(|v| *v == "LINE").count(), 28);
        assert!(text.contains("  2\nSide panel\n"), "a layer per object");
        assert!(text.contains("  8\nBlock\n"));
        assert!(text.contains(" 70\n15\n"), "triangulation edges hidden");
        // Millimetres: the far corner of the first box.
        assert!(text.contains(" 10\n100\n 20\n200\n 30\n50\n"));
    }

    #[test]
    fn faces_with_holes_become_triangles() {
        let mut m = fixtures::box_model();
        m.apply(&sawhorse_kernel::Op::Imprint {
            object: 1,
            face: 2,
            points: vec![
                [0.02, 0.02, 0.05],
                [0.08, 0.02, 0.05],
                [0.08, 0.1, 0.05],
                [0.02, 0.1, 0.05],
            ],
        })
        .unwrap();
        let e = entities(&document(&m, &Scope::visible(&m)).unwrap());
        assert_eq!(e.iter().filter(|v| *v == "3DFACE").count(), 6 + 8);
    }

    #[test]
    fn outline_lwpolyline_and_r12() {
        let sq = |o: f64, s: f64| {
            vec![
                DVec2::new(o, o),
                DVec2::new(o + s, o),
                DVec2::new(o + s, o + s),
                DVec2::new(o, o + s),
            ]
        };
        let rings = vec![sq(1.0, 0.3), sq(1.1, 0.1)];
        let lw = outline(&rings, OutlineFlavour::Lwpolyline).unwrap();
        let e = entities(&lw);
        assert_eq!(e, ["LWPOLYLINE", "LWPOLYLINE"]);
        assert!(
            lw.contains(" 90\n4\n 70\n1\n 10\n0\n 20\n0\n 10\n300\n"),
            "{lw}"
        );
        assert!(lw.contains(" 10\n100\n 20\n100\n"), "the hole, 100 mm in");
        let r12 = outline(&rings, OutlineFlavour::R12).unwrap();
        assert_eq!(entities(&r12).iter().filter(|v| *v == "VERTEX").count(), 8);
        assert!(r12.contains("$ACADVER\n  1\nAC1009"));
        assert_eq!(
            outline(&[], OutlineFlavour::R12),
            Err(ExportError::BadOutline)
        );
    }

    #[test]
    fn numbers_and_layer_names() {
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(1e-9), "0");
        assert_eq!(num(12.5), "12.5");
        assert_eq!(num(1e7), "10000000");
        assert_eq!(layer_name("a/b:c"), "a_b_c");
        assert_eq!(layer_name("  "), "0");
    }
}
