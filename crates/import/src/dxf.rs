//! DXF, ASCII (R12 through current). Units from `$INSUNITS` (millimetres when it is
//! missing or 0). What comes in:
//!
//! - `3DFACE` and polyface meshes (`POLYLINE` with flag 64) become faces, one item per
//!   layer, coplanar triangles merged (Sawhorse's own DXF writes faces this way).
//! - `LINE`, `LWPOLYLINE` (bulges become arcs), 2D and 3D `POLYLINE`, `CIRCLE` and `ARC`
//!   become sketch strokes. A `LINE` lying along an edge of a face in the file is that
//!   face's edge, not a separate stroke, and is not added twice.
//! - Everything else (`TEXT`, `MTEXT`, `DIMENSION`, `HATCH`, `SPLINE`, `INSERT`, ...) is
//!   counted and listed in the notes. So are entities in a tilted object coordinate system
//!   (extrusion direction other than +Z), which are skipped.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};
use sawhorse_kernel::model::ImportStroke;

use crate::util::{plural, soup};
use crate::{ImportError, ImportOptions, Parsed, Unit, malformed};

/// Segments per full circle when arcs become strokes.
pub const ARC_SEGMENTS: usize = 48;

/// Vertices and polygon faces of one layer.
type Soup = (Vec<[f64; 3]>, Vec<Vec<Vec<u32>>>);

/// One entity: its type and group codes in order.
#[derive(Debug, Clone, Default)]
struct Entity {
    kind: String,
    codes: Vec<(i32, String)>,
}

impl Entity {
    fn f(&self, code: i32) -> Option<f64> {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.trim().parse().ok())
    }
    fn i(&self, code: i32) -> i64 {
        self.f(code).map(|x| x as i64).unwrap_or(0)
    }
    fn s(&self, code: i32) -> Option<&str> {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
    }
    fn layer(&self) -> String {
        self.s(8)
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "0".into())
    }
    fn pt(&self, base: i32) -> DVec3 {
        DVec3::new(
            self.f(base).unwrap_or(0.0),
            self.f(base + 10).unwrap_or(0.0),
            self.f(base + 20).unwrap_or(0.0),
        )
    }
    /// Extrusion direction is +Z (or absent).
    fn flat(&self) -> bool {
        let z = DVec3::new(
            self.f(210).unwrap_or(0.0),
            self.f(220).unwrap_or(0.0),
            self.f(230).unwrap_or(1.0),
        );
        (z.normalize_or_zero() - DVec3::Z).length() < 1e-9
    }
}

fn pairs(text: &str) -> Result<Vec<(i32, String)>, ImportError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::with_capacity(lines.len() / 2);
    let mut k = 0;
    while k + 1 < lines.len() {
        let code = lines[k].trim();
        if code.is_empty() && k + 2 >= lines.len() {
            break;
        }
        let c: i32 = code.parse().map_err(|_| {
            malformed(
                "DXF",
                format!(
                    "line {}: expected a group code, found {code:?} (binary DXF is not supported)",
                    k + 1
                ),
            )
        })?;
        out.push((c, lines[k + 1].trim_end_matches('\r').to_string()));
        k += 2;
    }
    Ok(out)
}

/// Points of an arc from angle `a0` to `a1` (degrees, counter-clockwise), centre `c`.
fn arc_points(c: DVec3, r: f64, a0: f64, a1: f64) -> Vec<DVec3> {
    let mut sweep = (a1 - a0).rem_euclid(360.0);
    if sweep <= 1e-9 {
        sweep = 360.0;
    }
    let n = ((sweep / 360.0 * ARC_SEGMENTS as f64).ceil() as usize).max(2);
    (0..=n)
        .map(|k| {
            let a = (a0 + sweep * k as f64 / n as f64).to_radians();
            c + DVec3::new(r * a.cos(), r * a.sin(), 0.0)
        })
        .collect()
}

/// A polyline segment from `a` to `b` with DXF bulge `bulge` (tan of a quarter of the
/// included angle; positive is counter-clockwise), as points after `a` up to `b`.
fn bulge_points(a: DVec3, b: DVec3, bulge: f64) -> Vec<DVec3> {
    if bulge.abs() < 1e-9 || !bulge.is_finite() {
        return vec![b];
    }
    let theta = 4.0 * bulge.atan();
    let (a2, b2) = (a.truncate(), b.truncate());
    let chord = b2 - a2;
    let d = chord.length();
    if d < 1e-12 {
        return vec![b];
    }
    // The centre sits on the chord's left for a counter-clockwise (positive) bulge.
    let left = DVec2::new(-chord.y, chord.x) / d;
    let c = (a2 + b2) * 0.5 + left * (d / 2.0 / (theta / 2.0).tan());
    let r = (a2 - c).length();
    let a0 = (a2.y - c.y).atan2(a2.x - c.x);
    let n = ((theta.abs() / std::f64::consts::TAU * ARC_SEGMENTS as f64).ceil() as usize).max(2);
    (1..=n)
        .map(|k| {
            if k == n {
                b
            } else {
                let t = a0 + theta * k as f64 / n as f64;
                DVec3::new(c.x + r * t.cos(), c.y + r * t.sin(), a.z)
            }
        })
        .collect()
}

pub fn read(bytes: &[u8], opts: &ImportOptions) -> Result<Parsed, ImportError> {
    let text = String::from_utf8_lossy(bytes);
    let pairs = pairs(&text)?;
    let mut out = Parsed::default();

    // Sections: units from the header, entities from ENTITIES.
    let mut insunits: Option<i64> = None;
    let mut section = String::new();
    let mut entities: Vec<Entity> = Vec::new();
    let mut k = 0;
    while k < pairs.len() {
        let (c, v) = (&pairs[k].0, pairs[k].1.trim());
        if *c == 0 && v == "SECTION" {
            section = pairs
                .get(k + 1)
                .map(|p| p.1.trim().to_string())
                .unwrap_or_default();
            k += 2;
            continue;
        }
        if *c == 0 && v == "ENDSEC" {
            section.clear();
            k += 1;
            continue;
        }
        if section == "HEADER" && *c == 9 && v == "$INSUNITS" {
            insunits = pairs.get(k + 1).and_then(|p| p.1.trim().parse().ok());
        }
        if section == "ENTITIES" && *c == 0 {
            entities.push(Entity {
                kind: v.to_string(),
                codes: Vec::new(),
            });
        } else if section == "ENTITIES"
            && let Some(e) = entities.last_mut()
        {
            e.codes.push((*c, pairs[k].1.clone()));
        }
        k += 1;
    }
    if entities.is_empty() && !text.contains("ENTITIES") {
        return Err(malformed("DXF", "no ENTITIES section"));
    }
    let (unit, said) = match insunits {
        Some(1) => (Unit::In, true),
        Some(2) => (Unit::Ft, true),
        Some(4) => (Unit::Mm, true),
        Some(5) => (Unit::Cm, true),
        Some(6) => (Unit::M, true),
        // The file does not say: the user's choice (File > Import units), else millimetres.
        _ => (opts.units.unwrap_or_default(), false),
    };
    let s = unit.metres();
    if !said {
        out.notes.push(format!(
            "DXF does not say its units; read as {} (File > Import units)",
            unit.label()
        ));
    }
    if insunits.is_some_and(|u| u != 0 && ![1, 2, 4, 5, 6].contains(&u)) {
        out.notes.push(format!(
            "DXF units code {} is not supported; read as {}",
            insunits.unwrap_or(0),
            unit.label()
        ));
    }

    // Faces per layer.
    let mut layers: BTreeMap<String, Soup> = BTreeMap::new();
    let mut face_edges: BTreeSet<[i64; 6]> = BTreeSet::new();
    let key = |p: DVec3| -> [i64; 3] {
        [
            (p.x * 1e4).round() as i64,
            (p.y * 1e4).round() as i64,
            (p.z * 1e4).round() as i64,
        ]
    };
    let edge_key = |a: DVec3, b: DVec3| -> [i64; 6] {
        let (ka, kb) = (key(a), key(b));
        let (x, y) = if ka <= kb { (ka, kb) } else { (kb, ka) };
        [x[0], x[1], x[2], y[0], y[1], y[2]]
    };
    let mut add_face = |layer: String, pts: Vec<DVec3>, face_edges: &mut BTreeSet<[i64; 6]>| {
        let entry = layers.entry(layer).or_default();
        let base = entry.0.len() as u32;
        for k in 0..pts.len() {
            face_edges.insert(edge_key(pts[k], pts[(k + 1) % pts.len()]));
        }
        entry.0.extend(pts.iter().map(|p| (*p * s).to_array()));
        entry
            .1
            .push(vec![(base..base + pts.len() as u32).collect()]);
    };
    let mut strokes: Vec<(Vec<DVec3>, bool)> = Vec::new();
    let mut lines: Vec<(DVec3, DVec3)> = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut tilted = 0usize;
    let mut i = 0;
    while i < entities.len() {
        let e = &entities[i];
        match e.kind.as_str() {
            "3DFACE" => {
                let mut pts = vec![e.pt(10), e.pt(11), e.pt(12), e.pt(13)];
                if pts[3].distance(pts[2]) < 1e-12 {
                    pts.pop();
                }
                add_face(e.layer(), pts, &mut face_edges);
            }
            "LINE" => lines.push((e.pt(10), e.pt(11))),
            "LWPOLYLINE" => {
                if !e.flat() {
                    tilted += 1;
                } else {
                    let z = e.f(38).unwrap_or(0.0);
                    let mut verts: Vec<(DVec3, f64)> = Vec::new();
                    for (c, v) in &e.codes {
                        match c {
                            10 => verts
                                .push((DVec3::new(v.trim().parse().unwrap_or(0.0), 0.0, z), 0.0)),
                            20 => {
                                if let Some(last) = verts.last_mut() {
                                    last.0.y = v.trim().parse().unwrap_or(0.0);
                                }
                            }
                            42 => {
                                if let Some(last) = verts.last_mut() {
                                    last.1 = v.trim().parse().unwrap_or(0.0);
                                }
                            }
                            _ => {}
                        }
                    }
                    let closed = e.i(70) & 1 == 1;
                    strokes.push(polyline(&verts, closed));
                }
            }
            "POLYLINE" => {
                let flags = e.i(70);
                let mut verts: Vec<Entity> = Vec::new();
                let mut j = i + 1;
                while j < entities.len() && entities[j].kind == "VERTEX" {
                    verts.push(entities[j].clone());
                    j += 1;
                }
                if entities.get(j).is_some_and(|x| x.kind == "SEQEND") {
                    j += 1;
                }
                if flags & 64 != 0 {
                    // Polyface mesh: position vertices (flag 64 + 128... 192), then face records
                    // (flag 128) whose 71..74 are 1-based vertex numbers (negative: hidden edge).
                    let positions: Vec<DVec3> = verts
                        .iter()
                        .filter(|v| v.i(70) & 64 != 0)
                        .map(|v| v.pt(10))
                        .collect();
                    for f in verts
                        .iter()
                        .filter(|v| v.i(70) & 64 == 0 && v.i(70) & 128 != 0)
                    {
                        let idx: Vec<usize> = [71, 72, 73, 74]
                            .iter()
                            .map(|&c| f.i(c).unsigned_abs() as usize)
                            .filter(|&n| n >= 1 && n <= positions.len())
                            .map(|n| n - 1)
                            .collect();
                        let mut pts: Vec<DVec3> = idx.iter().map(|&n| positions[n]).collect();
                        pts.dedup_by(|a, b| a.distance(*b) < 1e-12);
                        if pts.len() >= 3 {
                            add_face(e.layer(), pts, &mut face_edges);
                        }
                    }
                } else if flags & 16 != 0 {
                    *skipped.entry("POLYLINE (polygon mesh)".into()).or_default() += 1;
                } else if flags & 8 == 0 && !e.flat() {
                    tilted += 1;
                } else {
                    let three_d = flags & 8 != 0;
                    let z = e.f(30).unwrap_or(0.0);
                    let pts: Vec<(DVec3, f64)> = verts
                        .iter()
                        .map(|v| {
                            let mut p = v.pt(10);
                            if !three_d {
                                p.z = z;
                            }
                            (p, if three_d { 0.0 } else { v.f(42).unwrap_or(0.0) })
                        })
                        .collect();
                    strokes.push(polyline(&pts, flags & 1 == 1));
                }
                i = j;
                continue;
            }
            "CIRCLE" | "ARC" => {
                if !e.flat() {
                    tilted += 1;
                } else {
                    let c = e.pt(10);
                    let r = e.f(40).unwrap_or(0.0);
                    if r > 0.0 {
                        if e.kind == "CIRCLE" {
                            let mut pts = arc_points(c, r, 0.0, 360.0);
                            pts.pop();
                            strokes.push((pts, true));
                        } else {
                            strokes.push((
                                arc_points(c, r, e.f(50).unwrap_or(0.0), e.f(51).unwrap_or(360.0)),
                                false,
                            ));
                        }
                    }
                }
            }
            "VERTEX" | "SEQEND" => {}
            other => *skipped.entry(other.to_string()).or_default() += 1,
        }
        i += 1;
    }
    // LINEs along face edges are the faces' own edges.
    let mut on_faces = 0usize;
    for (a, b) in lines {
        if face_edges.contains(&edge_key(a, b)) {
            on_faces += 1;
        } else {
            strokes.push((vec![a, b], false));
        }
    }
    for (layer, (positions, faces)) in layers {
        let mut item = soup(&layer, positions, faces);
        item.tag = (layer != "0").then(|| layer.clone());
        out.items.push(item);
    }
    for (pts, closed) in strokes {
        let pts: Vec<[f64; 3]> = pts.iter().map(|p| (*p * s).to_array()).collect();
        if pts.len() >= 2 {
            out.strokes.push(ImportStroke {
                points: pts,
                closed,
            });
        }
    }
    if on_faces > 0 {
        out.notes.push(format!(
            "DXF: {on_faces} line{} along face edges taken as those edges",
            plural(on_faces)
        ));
    }
    if tilted > 0 {
        out.notes.push(format!(
            "DXF: skipped {tilted} entit{} drawn in a tilted coordinate system",
            if tilted == 1 { "y" } else { "ies" }
        ));
    }
    if !skipped.is_empty() {
        let list: Vec<String> = skipped.iter().map(|(k, n)| format!("{n} {k}")).collect();
        out.notes
            .push(format!("DXF: not imported: {}", list.join(", ")));
    }
    Ok(out)
}

/// Points of a polyline with bulges (each vertex's bulge applies to the segment after it).
fn polyline(verts: &[(DVec3, f64)], closed: bool) -> (Vec<DVec3>, bool) {
    let mut pts = Vec::new();
    let Some(first) = verts.first() else {
        return (pts, closed);
    };
    pts.push(first.0);
    let n = verts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for k in 0..segs {
        let (a, bulge) = verts[k];
        let b = verts[(k + 1) % n].0;
        pts.extend(bulge_points(a, b, bulge));
    }
    if closed && pts.len() > 1 && pts.last().is_some_and(|p| p.distance(pts[0]) < 1e-12) {
        pts.pop();
    }
    (pts, closed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_export::{Scope, dxf as wdxf};
    use sawhorse_kernel::{Document, OpOutcome};

    #[test]
    fn round_trip_through_our_dxf() {
        let src = crate::test_support::two_boxes();
        let text = wdxf::document(src.model(), &Scope::visible(src.model())).unwrap();
        let p = read(text.as_bytes(), &ImportOptions::default()).unwrap();
        assert_eq!(p.items.len(), 2, "a layer per object");
        assert!(p.strokes.is_empty(), "every LINE is a face edge");
        assert!(
            p.notes
                .iter()
                .any(|n| n.contains("24 lines along face edges")),
            "{:?}",
            p.notes
        );
        let mut d = Document::with_ground_sketch();
        let OpOutcome::Imported {
            objects, meshes, ..
        } = d.apply(&p.op()).unwrap()
        else {
            panic!()
        };
        assert_eq!((objects.len(), meshes.len()), (2, 0));
        let (a, b) = (src.model().stats(), d.model().stats());
        assert_eq!(
            (a.faces, a.edges, a.vertices),
            (b.faces, b.edges, b.vertices)
        );
        assert!((a.volume_m3 - b.volume_m3).abs() < 1e-12);
    }

    #[test]
    fn outline_round_trips_as_strokes_with_holes() {
        use glam::DVec2;
        let sq = |o: f64, s: f64| {
            vec![
                DVec2::new(o, o),
                DVec2::new(o + s, o),
                DVec2::new(o + s, o + s),
                DVec2::new(o, o + s),
            ]
        };
        for flavour in [wdxf::OutlineFlavour::Lwpolyline, wdxf::OutlineFlavour::R12] {
            let text = wdxf::outline(&[sq(0.0, 0.3), sq(0.1, 0.1)], flavour).unwrap();
            let p = read(text.as_bytes(), &ImportOptions::default()).unwrap();
            assert_eq!(p.strokes.len(), 2, "{flavour:?}");
            assert!(p.strokes.iter().all(|s| s.closed && s.points.len() == 4));
            assert_eq!(p.strokes[0].points[2], [0.3, 0.3, 0.0]);
            let mut d = Document::with_ground_sketch();
            d.apply(&p.op()).unwrap();
            assert_eq!(d.model().stats().regions, 2, "a ring region and the hole");
        }
    }

    #[test]
    fn units_bulges_circles_and_skips() {
        let text = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
0\nLWPOLYLINE\n8\nCUT\n90\n2\n70\n1\n10\n0\n20\n0\n42\n1\n10\n2\n20\n0\n42\n1\n\
0\nCIRCLE\n8\n0\n10\n5\n20\n5\n30\n0\n40\n1\n\
0\nTEXT\n8\n0\n1\nhello\n\
0\nCIRCLE\n8\n0\n10\n5\n20\n5\n30\n0\n40\n1\n210\n1\n220\n0\n230\n0\n\
0\nENDSEC\n0\nEOF\n";
        let p = read(text.as_bytes(), &ImportOptions::default()).unwrap();
        assert_eq!(p.strokes.len(), 2);
        // Two half-circle bulges: a full circle of diameter 2 in, centred at (1, 0) in.
        let r = p.strokes[0]
            .points
            .iter()
            .map(|q| ((q[0] - 0.0254).powi(2) + q[1].powi(2)).sqrt());
        for d in r {
            assert!((d - 0.0254).abs() < 1e-9, "{d}");
        }
        assert_eq!(p.strokes[1].points.len(), ARC_SEGMENTS);
        assert!(p.notes.iter().any(|n| n.contains("not imported: 1 TEXT")));
        assert!(
            p.notes
                .iter()
                .any(|n| n.contains("1 entity drawn in a tilted"))
        );
        assert!(read(b"999\nnot a dxf", &ImportOptions::default()).is_err());
    }
}
