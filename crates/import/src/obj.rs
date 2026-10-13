//! Wavefront OBJ: `v`, `f` (with `v/vt/vn` forms and negative indices), `l`, `o`, `g`,
//! `usemtl`. OBJ carries no units: millimetres unless the options say otherwise (Sawhorse's
//! own OBJ is millimetres). Each `o` (or, without any `o`, each `g`) is its own item;
//! faces stay polygons, merged into coplanar n-gons only when every face is a triangle (a
//! triangulated export), so drawn edges between coplanar n-gons survive. Materials named by
//! `usemtl` live in a separate `.mtl` file that a single-file import cannot see, so the
//! name is noted and the item comes in unpainted.

use std::collections::BTreeSet;

use crate::util::{plural, soup};
use crate::{Budget, ImportError, ImportOptions, Parsed, malformed};

struct Piece {
    name: String,
    faces: Vec<Vec<u32>>,
    lines: Vec<[u32; 2]>,
}

pub fn read(bytes: &[u8], opts: &ImportOptions) -> Result<Parsed, ImportError> {
    let unit = opts.units.unwrap_or_default();
    let s = unit.metres();
    let text = String::from_utf8_lossy(bytes);
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut have_o = text.lines().any(|l| l.trim_start().starts_with("o "));
    let stem = opts.stem();
    let mut current = Piece {
        name: stem.clone(),
        faces: Vec::new(),
        lines: Vec::new(),
    };
    let mut mtls: BTreeSet<String> = BTreeSet::new();
    let mut skipped = 0usize;
    let mut budget = Budget::default();
    let resolve = |tok: &str, n: usize| -> Option<u32> {
        let i: i64 = tok.split('/').next()?.parse().ok()?;
        let k = if i < 0 { n as i64 + i } else { i - 1 };
        (0..n as i64).contains(&k).then_some(k as u32)
    };
    // Continuation lines end in a backslash.
    let joined = text.replace("\\\r\n", " ").replace("\\\n", " ");
    for (ln, raw) in joined.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let mut w = line.split_whitespace();
        let Some(kw) = w.next() else { continue };
        match kw {
            "v" => {
                let c: Vec<f64> = w.take(3).filter_map(|x| x.parse().ok()).collect();
                if c.len() != 3 || c.iter().any(|x| !x.is_finite()) {
                    return Err(malformed(
                        "OBJ",
                        format!("line {}: a vertex needs three numbers", ln + 1),
                    ));
                }
                budget.points("OBJ", 1)?;
                verts.push([c[0] * s, c[1] * s, c[2] * s]);
            }
            "f" | "fo" => {
                let idx: Option<Vec<u32>> = w.map(|t| resolve(t, verts.len())).collect();
                match idx {
                    Some(f) if f.len() >= 3 => {
                        budget.faces("OBJ", 1)?;
                        current.faces.push(f)
                    }
                    _ => skipped += 1,
                }
            }
            "l" => {
                let idx: Option<Vec<u32>> = w.map(|t| resolve(t, verts.len())).collect();
                match idx {
                    Some(l) if l.len() >= 2 => {
                        budget.faces("OBJ", l.len() - 1)?;
                        current.lines.extend(l.windows(2).map(|p| [p[0], p[1]]));
                    }
                    _ => skipped += 1,
                }
            }
            "o" | "g" if kw == "o" || !have_o => {
                let n: Vec<&str> = w.collect();
                let name = if n.is_empty() {
                    stem.clone()
                } else {
                    n.join(" ")
                };
                let done = std::mem::replace(
                    &mut current,
                    Piece {
                        name,
                        faces: Vec::new(),
                        lines: Vec::new(),
                    },
                );
                if !done.faces.is_empty() || !done.lines.is_empty() {
                    budget.items("OBJ", 1)?;
                    pieces.push(done);
                }
            }
            "usemtl" => {
                if let Some(m) = w.next() {
                    mtls.insert(m.to_string());
                }
            }
            _ => {}
        }
        have_o |= kw == "o";
    }
    if !current.faces.is_empty() || !current.lines.is_empty() {
        pieces.push(current);
    }
    let mut out = Parsed::default();
    for p in pieces {
        // Each piece gets its own compact vertex list.
        let mut map = std::collections::BTreeMap::new();
        let mut pos = Vec::new();
        let mut local = |v: u32| -> u32 {
            *map.entry(v).or_insert_with(|| {
                pos.push(verts[v as usize]);
                (pos.len() - 1) as u32
            })
        };
        let faces: Vec<Vec<Vec<u32>>> = p
            .faces
            .iter()
            .map(|f| vec![f.iter().map(|&v| local(v)).collect()])
            .collect();
        let lines: Vec<[u32; 2]> = p.lines.iter().map(|l| [local(l[0]), local(l[1])]).collect();
        let all_triangles = faces.iter().all(|f| f[0].len() == 3);
        let mut item = soup(&p.name, pos, faces);
        item.lines = lines;
        item.merge_coplanar = all_triangles;
        out.items.push(item);
    }
    if skipped > 0 {
        out.notes.push(format!(
            "OBJ: skipped {skipped} face{} or line{} naming missing vertices",
            plural(skipped),
            plural(skipped)
        ));
    }
    if !mtls.is_empty() {
        out.notes.push(format!(
            "OBJ: materials {} live in a separate .mtl file; imported unpainted",
            mtls.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    out.notes.push(format!(
        "OBJ has no units; read as {} (File > Import units)",
        unit.label()
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_export::{Scope, obj};
    use sawhorse_kernel::{Document, Op, OpOutcome};

    #[test]
    fn round_trip_through_our_obj_keeps_names_and_drawn_edges() {
        let mut src = crate::test_support::two_boxes();
        // Draw a line across the first box's top: two coplanar faces and a drawn edge.
        src.apply(&Op::SplitFace {
            object: 1,
            face: 2,
            path: vec![[0.05, 0.0, 0.05], [0.05, 0.2, 0.05]],
        })
        .unwrap();
        src.apply(&Op::RenameObject {
            object: 1,
            name: Some("Side".into()),
        })
        .unwrap();
        let text = obj::document(src.model(), &Scope::visible(src.model()), "m.mtl").unwrap();
        let p = read(
            text.as_bytes(),
            &ImportOptions {
                units: None,
                name: "m.obj".into(),
            },
        )
        .unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0].name, "Side");
        assert!(!p.items[0].merge_coplanar, "n-gons stay as drawn");
        let mut d = Document::with_ground_sketch();
        let OpOutcome::Imported { objects, .. } = d.apply(&p.op()).unwrap() else {
            panic!()
        };
        assert_eq!(objects.len(), 2);
        let a = src.model().object(1).unwrap().brep().to_polys();
        let b = d.model().object(1).unwrap().brep().to_polys();
        assert_eq!(a.faces.len(), b.faces.len());
        assert_eq!(
            b.user_edges.len(),
            1,
            "the drawn line is a drawn line again"
        );
        assert!((src.model().stats().volume_m3 - d.model().stats().volume_m3).abs() < 1e-12);
    }

    #[test]
    fn negative_indices_groups_lines_and_materials() {
        let text = "g first\nv 0 0 0\nv 10 0 0\nv 0 10 0\nf -3/1/1 -2/2/2 -1/3/3\nusemtl Red\ng second\nv 0 0 5\nl 1 4\nf 1 2 99\n";
        let p = read(text.as_bytes(), &ImportOptions::default()).unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0].name, "first");
        assert_eq!(p.items[0].positions[1], [0.01, 0.0, 0.0]);
        assert!(p.items[0].merge_coplanar);
        assert_eq!(p.items[1].lines.len(), 1);
        assert!(p.notes.iter().any(|n| n.contains("skipped 1 face")));
        assert!(p.notes.iter().any(|n| n.contains("materials Red")));
        assert!(read(b"v 1 2\n", &ImportOptions::default()).is_err());
    }
}
