//! Wavefront OBJ, hand-written: millimetres, Z-up (same convention as our STL), one `o`
//! per object. Faces stay n-gons where OBJ can say so (no holes); faces with holes are
//! written as their triangles, since OBJ has no hole loops.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};
use sawhorse_kernel::object::Object;
use sawhorse_tessellate::triangulate;

use crate::{ExportError, MM_PER_M};

/// OBJ text for one object, named `name` (letters, digits, `-` and `_` are kept).
pub fn write(object: &Object, name: &str) -> Result<String, ExportError> {
    if !object.is_watertight() {
        return Err(ExportError::NotWatertight);
    }
    let p = object.brep().to_polys();
    if p.faces.is_empty() {
        return Err(ExportError::Empty);
    }
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut out = String::new();
    out.push_str("# Sawhorse OBJ; units mm; Z up\n");
    let _ = writeln!(out, "o {clean}");
    for v in &p.points {
        let m: DVec3 = *v * MM_PER_M;
        let _ = writeln!(out, "v {} {} {}", m.x, m.y, m.z);
    }
    for (fi, f) in p.faces.iter().enumerate() {
        if f.holes.is_empty() {
            out.push('f');
            for &i in &f.outer {
                let _ = write!(out, " {}", i + 1);
            }
            out.push('\n');
            continue;
        }
        let plane = p
            .plane(fi)
            .ok_or_else(|| ExportError::Tessellate(format!("face {fi} has no plane")))?;
        let flat: Vec<usize> = f.rings().flatten().copied().collect();
        let ring = |r: &Vec<usize>| -> Vec<DVec2> {
            r.iter().map(|&i| plane.to_local(p.points[i])).collect()
        };
        let holes: Vec<Vec<DVec2>> = f.holes.iter().map(ring).collect();
        let tris = triangulate(&ring(&f.outer), &holes)
            .map_err(|e| ExportError::Tessellate(e.to_string()))?;
        for t in tris {
            let [a, b, c] = t.map(|k| flat[k as usize] + 1);
            let _ = writeln!(out, "f {a} {b} {c}");
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::{Model, Op};

    #[test]
    fn box_with_an_imprint() {
        let mut m = Model::with_ground_sketch();
        for op in [
            Op::AddRectangle {
                sketch: 1,
                x: 0.0,
                y: 0.0,
                w: 0.1,
                h: 0.2,
            },
            Op::Extrude {
                sketch: 1,
                region: 1,
                distance: 0.05,
            },
            Op::Imprint {
                object: 1,
                face: 1,
                points: vec![
                    [0.02, 0.02, 0.05],
                    [0.08, 0.02, 0.05],
                    [0.08, 0.1, 0.05],
                    [0.02, 0.1, 0.05],
                ],
            },
        ] {
            m.apply(&op).unwrap();
        }
        let text = write(m.object(1).unwrap(), "box 1").unwrap();
        assert!(text.starts_with("# Sawhorse OBJ; units mm; Z up\no box_1\n"));
        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 12);
        assert!(text.contains("v 100 200 50\n"), "millimetres");
        // 5 quads + the sub-face quad + the holed top as 8 triangles.
        let faces: Vec<&str> = text.lines().filter(|l| l.starts_with("f ")).collect();
        assert_eq!(faces.len(), 6 + 8);
        assert_eq!(
            faces.iter().filter(|f| f.split(' ').count() == 5).count(),
            6
        );
    }
}
