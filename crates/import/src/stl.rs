//! STL, binary and ASCII. STL carries no units: millimetres unless the options say
//! otherwise. Sawhorse's own STL says "units mm" in its header (that is what slicers
//! assume), and it is read as millimetres whatever the options say (#47). Each ASCII
//! `solid` block is its own item; a binary file is one item.

use crate::util::{plural, soup};
use crate::{Budget, ImportError, ImportOptions, Parsed, Unit, malformed};

/// An ASCII `solid` block being read: name, vertices, faces.
type Block = (String, Vec<[f64; 3]>, Vec<Vec<Vec<u32>>>);

fn binary_len_ok(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 84 {
        return None;
    }
    let n = u32::from_le_bytes(bytes[80..84].try_into().ok()?) as usize;
    (84 + n.checked_mul(50)? == bytes.len()).then_some(n)
}

/// The header Sawhorse's own STL starts with (`sawhorse_export::stl`), padded with NULs.
const OUR_HEADER: &[u8] = b"Sawhorse binary STL; units mm; Z up";

/// A binary STL Sawhorse wrote: millimetres, whatever File > Import units says (#47).
fn ours(bytes: &[u8]) -> bool {
    let header = &bytes[..80];
    let text = header.split(|&b| b == 0).next().unwrap_or(header);
    text == OUR_HEADER && header[text.len()..].iter().all(|&b| b == 0)
}

pub fn read(bytes: &[u8], opts: &ImportOptions) -> Result<Parsed, ImportError> {
    let ascii = bytes.trim_ascii_start().starts_with(b"solid") && binary_len_ok(bytes).is_none();
    let ours = !ascii && binary_len_ok(bytes).is_some() && ours(bytes);
    let unit = if ours {
        Unit::Mm
    } else {
        opts.units.unwrap_or_default()
    };
    let s = unit.metres();
    let mut out = Parsed::default();
    let name = opts.stem();
    let mut budget = Budget::default();
    if let Some(n) = binary_len_ok(bytes).filter(|_| !ascii) {
        // The count is checked against the file's length (binary_len_ok) and the import
        // limits before anything is allocated for it.
        budget.faces("STL", n)?;
        budget.points("STL", n * 3)?;
        let mut positions = Vec::with_capacity(n * 3);
        let mut faces = Vec::with_capacity(n);
        let mut nonfinite = 0;
        for t in 0..n {
            let b = 84 + t * 50 + 12;
            let f = |o: usize| f32::from_le_bytes(bytes[b + o..b + o + 4].try_into().unwrap());
            let tri: Vec<[f64; 3]> = (0..3)
                .map(|v| {
                    let o = v * 12;
                    [f(o).into(), f(o + 4).into(), f(o + 8).into()]
                })
                .collect();
            if tri.iter().flatten().any(|x: &f64| !x.is_finite()) {
                nonfinite += 1;
                continue;
            }
            let base = positions.len() as u32;
            positions.extend(tri.iter().map(|p| [p[0] * s, p[1] * s, p[2] * s]));
            faces.push(vec![vec![base, base + 1, base + 2]]);
        }
        if nonfinite > 0 {
            out.notes.push(format!(
                "STL: skipped {nonfinite} triangle{} with broken coordinates",
                plural(nonfinite)
            ));
        }
        if !faces.is_empty() {
            out.items.push(soup(&name, positions, faces));
        }
    } else {
        let text = std::str::from_utf8(bytes).map_err(|_| {
            malformed(
                "STL",
                "neither a binary STL (size does not match the triangle count) nor ASCII text",
            )
        })?;
        let mut cur: Option<Block> = None;
        let mut loop_pts: Vec<[f64; 3]> = Vec::new();
        for (ln, line) in text.lines().enumerate() {
            let mut w = line.split_whitespace();
            match w.next() {
                Some("solid") => {
                    let n: Vec<&str> = w.collect();
                    let n = if n.is_empty() {
                        name.clone()
                    } else {
                        n.join(" ")
                    };
                    cur = Some((n, Vec::new(), Vec::new()));
                }
                Some("outer") => loop_pts.clear(),
                Some("vertex") if loop_pts.len() >= 1024 => {
                    return Err(malformed(
                        "STL",
                        format!("line {}: a facet with more than 1,024 vertices", ln + 1),
                    ));
                }
                Some("vertex") => {
                    let v: Vec<f64> = w.filter_map(|x| x.parse().ok()).collect();
                    if v.len() != 3 || v.iter().any(|x| !x.is_finite()) {
                        return Err(malformed(
                            "STL",
                            format!("line {}: a vertex needs three numbers", ln + 1),
                        ));
                    }
                    loop_pts.push([v[0] * s, v[1] * s, v[2] * s]);
                }
                Some("endloop") => {
                    let c = cur.get_or_insert_with(|| (name.clone(), Vec::new(), Vec::new()));
                    if loop_pts.len() >= 3 {
                        budget.faces("STL", 1)?;
                        budget.points("STL", loop_pts.len())?;
                        let base = c.1.len() as u32;
                        let k = loop_pts.len() as u32;
                        c.1.append(&mut loop_pts);
                        c.2.push(vec![(base..base + k).collect()]);
                    }
                }
                Some("endsolid") => {
                    if let Some((n, p, f)) = cur.take()
                        && !f.is_empty()
                    {
                        out.items.push(soup(&n, p, f));
                    }
                }
                _ => {}
            }
        }
        if let Some((n, p, f)) = cur.take()
            && !f.is_empty()
        {
            out.items.push(soup(&n, p, f));
        }
    }
    out.notes.push(if ours {
        "Sawhorse STL: read as millimetres, as its header says (File > Import units is for other STL files)".to_string()
    } else {
        format!(
            "STL has no units; read as {} (File > Import units)",
            unit.label()
        )
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Unit;
    use sawhorse_export::{Scope, stl};
    use sawhorse_kernel::{Document, OpOutcome};

    #[test]
    fn round_trip_through_our_binary_stl() {
        let src = crate::test_support::two_boxes();
        let bytes = stl::document(src.model(), &Scope::visible(src.model())).unwrap();
        let p = read(
            &bytes,
            &ImportOptions {
                units: None,
                name: "parts.stl".into(),
            },
        )
        .unwrap();
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].name, "parts");
        assert_eq!(p.items[0].faces.len(), 24);
        assert!(p.notes[0].contains("read as millimetres"));
        let mut d = Document::with_ground_sketch();
        let out = d.apply(&p.op()).unwrap();
        let OpOutcome::Imported {
            objects, meshes, ..
        } = out
        else {
            panic!()
        };
        assert_eq!(
            (objects.len(), meshes.len()),
            (2, 0),
            "two shells, two solids"
        );
        let (a, b) = (src.model().stats(), d.model().stats());
        assert_eq!(
            (a.faces, a.edges, a.vertices),
            (b.faces, b.edges, b.vertices)
        );
        assert!((a.volume_m3 - b.volume_m3).abs() < 1e-9, "f32 in the file");
        assert_eq!(d.model().object(1).unwrap().name(), Some("parts 1"));
    }

    #[test]
    fn ascii_with_units_and_blocks() {
        let text = "solid a\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 1 0 0\n   vertex 0 1 0\n  endloop\n endfacet\nendsolid a\nsolid b\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 1\n   vertex 1 0 1\n   vertex 0 1 1\n  endloop\n endfacet\nendsolid b\n";
        let p = read(
            text.as_bytes(),
            &ImportOptions {
                units: Some(Unit::In),
                name: "x.stl".into(),
            },
        )
        .unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[1].name, "b");
        assert_eq!(p.items[0].positions[1], [0.0254, 0.0, 0.0]);
        assert!(p.notes[0].contains("read as inches"), "{:?}", p.notes);
        let bad = "solid a\n outer loop\n vertex 0 0\n";
        assert!(read(bad.as_bytes(), &ImportOptions::default()).is_err());
        assert!(read(&[0xff, 0xfe, 0x00], &ImportOptions::default()).is_err());
    }

    /// #47: a 48 in rail exported as STL and imported back, twice, with File > Import units
    /// on Inches read 30967 11/16 in (25.4 x 25.4 too long). Sawhorse's own STL says "units
    /// mm" in its header, and the reader now believes it whatever the menu says.
    #[test]
    fn our_stl_reads_as_millimetres_whatever_the_import_units_say() {
        use crate::test_support::{rail_48in, x_extent};
        for unit in [Unit::Mm, Unit::Cm, Unit::M, Unit::In, Unit::Ft] {
            let opts = ImportOptions {
                units: Some(unit),
                name: "frame.stl".into(),
            };
            let mut d = rail_48in();
            let volume = d.model().stats().volume_m3;
            for trip in 1..=2 {
                let bytes = stl::document(d.model(), &Scope::visible(d.model())).unwrap();
                let p = read(&bytes, &opts).unwrap();
                let long = x_extent(&p) / 0.0254;
                assert!(
                    (long - 48.0).abs() < 1e-4,
                    "{unit:?}, trip {trip}: {long} in"
                );
                assert!(
                    p.notes
                        .iter()
                        .any(|n| n.contains("Sawhorse STL") && n.contains("millimetres")),
                    "{:?}",
                    p.notes
                );
                d = Document::with_ground_sketch();
                d.apply(&p.op()).unwrap();
                let v = d.model().stats().volume_m3;
                assert!((v - volume).abs() < 1e-9, "{unit:?}, trip {trip}: {v} m³");
            }
        }
    }

    /// Any other binary STL still follows File > Import units (a part from a slicer, or
    /// one written in inches by another program).
    #[test]
    fn another_programs_stl_still_follows_the_import_units() {
        let mut bytes = Vec::new();
        let mut header = [0u8; 80];
        header[..20].copy_from_slice(b"Exported by OtherCAD");
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for v in [
            [0.0f32, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ] {
            for x in v {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let p = read(
            &bytes,
            &ImportOptions {
                units: Some(Unit::In),
                name: "part.stl".into(),
            },
        )
        .unwrap();
        assert_eq!(p.items[0].positions[1], [0.0254, 0.0, 0.0]);
        assert!(p.notes[0].contains("read as inches"), "{:?}", p.notes);
        // A header that only starts like ours is not ours.
        header[..36].copy_from_slice(b"Sawhorse binary STL; units mm; Z up!");
        bytes[..80].copy_from_slice(&header);
        let p = read(
            &bytes,
            &ImportOptions {
                units: Some(Unit::In),
                name: "part.stl".into(),
            },
        )
        .unwrap();
        assert_eq!(p.items[0].positions[1], [0.0254, 0.0, 0.0]);
    }

    /// Short files, our header with no triangles after it or with a count the length does
    /// not match, and our header followed by something other than NULs: none is read as
    /// ours, and none panics. An ASCII STL follows the menu as it always did.
    #[test]
    fn odd_files_with_our_header_and_ascii_stl() {
        let inches = ImportOptions {
            units: Some(Unit::In),
            name: "part.stl".into(),
        };
        let mut header = [0u8; 80];
        header[..OUR_HEADER.len()].copy_from_slice(OUR_HEADER);
        for len in [0, 1, 35, 79, 80, 83] {
            let short = &header[..len.min(80)];
            if let Ok(p) = read(short, &inches) {
                assert!(!p.notes.iter().any(|n| n.contains("Sawhorse STL")), "{len}");
            }
        }
        let mut wrong_count = header.to_vec();
        wrong_count.extend_from_slice(&2u32.to_le_bytes());
        wrong_count.extend_from_slice(&[0u8; 50]);
        if let Ok(p) = read(&wrong_count, &inches) {
            assert!(!p.notes.iter().any(|n| n.contains("Sawhorse STL")));
        }
        let mut empty = header.to_vec();
        empty.extend_from_slice(&0u32.to_le_bytes());
        let p = read(&empty, &inches).unwrap();
        assert!(p.items.is_empty());
        assert!(p.notes.iter().any(|n| n.contains("Sawhorse STL")));
        let mut tail = header;
        tail[79] = b' ';
        let mut bytes = tail.to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let p = read(&bytes, &inches).unwrap();
        assert!(p.notes.iter().any(|n| n.contains("read as inches")));
        let ascii = "solid rail\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid rail\n";
        let p = read(ascii.as_bytes(), &inches).unwrap();
        assert_eq!(p.items[0].positions[1], [0.0254, 0.0, 0.0]);
        assert!(p.notes.iter().any(|n| n.contains("read as inches")));
    }
}
