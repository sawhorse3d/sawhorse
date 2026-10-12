//! Wavefront OBJ, hand-written: millimetres, Z-up (same convention as our STL). Each
//! object is an `o` with a matching `g` group (so both object-aware and group-aware readers
//! split it), with `usemtl` when it is painted; [`mtl`] writes the matching material
//! library. Faces stay n-gons where OBJ can say so (no holes); faces with holes are written
//! as their triangles, since OBJ has no hole loops. Imported non-solid meshes come after
//! the solids, their loose lines as `l` elements.
//!
//! Lane 10: faces that show another material than the rest of their solid (face paint,
//! paint on the groups around it) get their own `usemtl`; faces whose material has a
//! texture the model holds get `vt` texture coordinates laid out as the view draws them,
//! and the MTL names the image file (`map_Kd lib-white-oak.png`) the app saves beside it.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};
use sawhorse_kernel::Model;
use sawhorse_kernel::object::Object;
use sawhorse_tessellate::triangulate;

use crate::{ExportError, MM_PER_M, Scope, object_name};

/// A face as point indices: the outer ring and its holes.
type IndexFace = (Vec<usize>, Vec<Vec<usize>>);

/// An OBJ-safe name: letters, digits, `-` and `_` kept, anything else becomes `_`.
pub fn clean_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "_".into() } else { s }
}

/// Faces of one polygon set: n-gons, or triangles where a face has holes.
fn faces(
    out: &mut String,
    points: &[DVec3],
    faces: &[IndexFace],
    base: usize,
) -> Result<(), ExportError> {
    for (outer, holes) in faces {
        if holes.is_empty() {
            out.push('f');
            for &i in outer {
                let _ = write!(out, " {}", base + i + 1);
            }
            out.push('\n');
            continue;
        }
        let ring_pts: Vec<DVec3> = outer.iter().map(|&i| points[i]).collect();
        let plane = sawhorse_kernel::plane::Plane::fit(&ring_pts)
            .map_err(|e| ExportError::Tessellate(e.to_string()))?;
        let flat: Vec<usize> = std::iter::once(outer)
            .chain(holes)
            .flatten()
            .copied()
            .collect();
        let ring = |r: &Vec<usize>| -> Vec<DVec2> {
            r.iter().map(|&i| plane.to_local(points[i])).collect()
        };
        let hs: Vec<Vec<DVec2>> = holes.iter().map(ring).collect();
        let tris =
            triangulate(&ring(outer), &hs).map_err(|e| ExportError::Tessellate(e.to_string()))?;
        for t in tris {
            let [a, b, c] = t.map(|k| base + flat[k as usize] + 1);
            let _ = writeln!(out, "f {a} {b} {c}");
        }
    }
    Ok(())
}

fn vertices(out: &mut String, points: &[DVec3]) {
    for v in points {
        let m: DVec3 = *v * MM_PER_M;
        let _ = writeln!(out, "v {} {} {}", m.x, m.y, m.z);
    }
}

fn solid_faces(object: &Object) -> (Vec<DVec3>, Vec<IndexFace>) {
    let p = object.brep().to_polys();
    let f = p
        .faces
        .iter()
        .map(|f| (f.outer.clone(), f.holes.clone()))
        .collect();
    (p.points, f)
}

/// OBJ text for one object, named `name`.
pub fn write(object: &Object, name: &str) -> Result<String, ExportError> {
    if !object.is_watertight() {
        return Err(ExportError::NotWatertight);
    }
    let (points, f) = solid_faces(object);
    if f.is_empty() {
        return Err(ExportError::Empty);
    }
    let mut out = String::new();
    out.push_str("# Sawhorse OBJ; units mm; Z up\n");
    let _ = writeln!(out, "o {}", clean_name(name));
    vertices(&mut out, &points);
    faces(&mut out, &points, &f, 0)?;
    Ok(out)
}

/// Everything in `scope`: one `o` + `g` per solid, then per imported mesh. `mtllib` names
/// `mtl_file` when any of them is painted.
pub fn document(model: &Model, scope: &Scope, mtl_file: &str) -> Result<String, ExportError> {
    let solids = scope.solids(model)?;
    let loose = scope.loose(model)?;
    if solids.is_empty() && loose.is_empty() {
        return Err(ExportError::Empty);
    }
    let painted = solids.iter().any(|(id, o)| {
        o.brep()
            .persistent_face_ids()
            .into_iter()
            .any(|f| model.face_material(*id, f).is_some())
    }) || loose.iter().any(|(_, m)| m.material.is_some());
    let mut vt_base = 0;
    let mut out = String::new();
    out.push_str("# Sawhorse OBJ; units mm; Z up\n");
    if painted {
        let _ = writeln!(out, "mtllib {mtl_file}");
    }
    let mut base = 0;
    let mut used = std::collections::BTreeSet::new();
    let mut unique = |name: String| -> String {
        let mut n = clean_name(&name);
        let mut k = 2;
        while !used.insert(n.clone()) {
            n = format!("{}_{k}", clean_name(&name));
            k += 1;
        }
        n
    };
    for (id, o) in &solids {
        let name = unique(object_name(model, *id));
        let _ = writeln!(out, "o {name}\ng {name}");
        let ids = o.brep().persistent_face_ids();
        let mats: Vec<Option<&str>> = ids.iter().map(|&f| model.face_material(*id, f)).collect();
        let textured = ids.iter().any(|&f| model.face_texture(*id, f).is_some());
        // As before lane 10 when every face shows the solid's own material and none is
        // textured: one usemtl for the whole object.
        if !textured && mats.iter().all(|m| *m == o.material()) {
            if let Some(m) = o.material() {
                let _ = writeln!(out, "usemtl {}", clean_name(m));
            }
            let (points, f) = solid_faces(o);
            vertices(&mut out, &points);
            faces(&mut out, &points, &f, base)?;
            base += points.len();
            continue;
        }
        let (points, f) = solid_faces(o);
        vertices(&mut out, &points);
        let mut current: Option<Option<&str>> = None;
        for (k, face) in f.iter().enumerate() {
            if current != Some(mats[k]) {
                let _ = writeln!(
                    out,
                    "usemtl {}",
                    mats[k].map_or("default".to_string(), clean_name)
                );
                current = Some(mats[k]);
            }
            match model.face_texture(*id, ids[k]) {
                Some(t) => {
                    // One vt per point of this face (its own texture coordinates).
                    let pts: Vec<usize> = std::iter::once(&face.0)
                        .chain(&face.1)
                        .flatten()
                        .copied()
                        .collect();
                    let mut vt_of = std::collections::BTreeMap::new();
                    for &i in &pts {
                        if let std::collections::btree_map::Entry::Vacant(e) = vt_of.entry(i) {
                            let [u, v] = t.uv(points[i]);
                            let _ = writeln!(out, "vt {u} {v}");
                            vt_base += 1;
                            e.insert(vt_base);
                        }
                    }
                    let mut tmp = String::new();
                    faces(&mut tmp, &points, std::slice::from_ref(face), 0)?;
                    for line in tmp.lines() {
                        out.push('f');
                        for idx in line.split_whitespace().skip(1) {
                            let i: usize = idx.parse().unwrap_or(1) - 1;
                            let _ = write!(out, " {}/{}", base + i + 1, vt_of[&i]);
                        }
                        out.push('\n');
                    }
                }
                None => faces(&mut out, &points, std::slice::from_ref(face), base)?,
            }
        }
        base += points.len();
    }
    for (_, m) in &loose {
        let name = unique(m.name.clone());
        let _ = writeln!(out, "# {}: {}", m.name, m.note);
        let _ = writeln!(out, "o {name}\ng {name}");
        if let Some(mat) = &m.material {
            let _ = writeln!(out, "usemtl {}", clean_name(mat));
        }
        vertices(&mut out, &m.positions);
        let f: Vec<IndexFace> = m
            .faces
            .iter()
            .map(|f| {
                let r = |r: &Vec<u32>| r.iter().map(|&i| i as usize).collect::<Vec<usize>>();
                (r(&f[0]), f[1..].iter().map(r).collect())
            })
            .collect();
        // Faces of a non-solid mesh that do not triangulate are written as n-gons anyway.
        if faces(&mut out, &m.positions, &f, base).is_err() {
            for (outer, _) in &f {
                out.push('f');
                for &i in outer {
                    let _ = write!(out, " {}", base + i + 1);
                }
                out.push('\n');
            }
        }
        for l in &m.lines {
            let _ = writeln!(
                out,
                "l {} {}",
                base + l[0] as usize + 1,
                base + l[1] as usize + 1
            );
        }
        base += m.positions.len();
    }
    Ok(out)
}

/// The material library for [`document`]: `Kd` colour, `d` opacity, and `map_Kd` naming
/// the texture file a material had.
pub fn mtl(model: &Model) -> String {
    let mut out = String::from("# Sawhorse MTL\n");
    for (name, m) in model.materials() {
        let c = m.color.map(|x| f64::from(x) / 255.0);
        let _ = writeln!(
            out,
            "newmtl {}\nKd {:.4} {:.4} {:.4}\nd {:.4}",
            clean_name(name),
            c[0],
            c[1],
            c[2],
            c[3]
        );
        match &m.texture {
            // An image the model holds: its file name, saved beside the MTL.
            Some(t) if model.textures().contains_key(t) => {
                let _ = writeln!(out, "map_Kd {}", texture_file(t));
            }
            Some(t) => {
                let _ = writeln!(out, "map_Kd {t}");
            }
            None => {}
        }
    }
    out
}

/// The file name a texture is saved under next to an OBJ (`assets/lib-oak.png` ->
/// `lib-oak.png`).
pub fn texture_file(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
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
                face: 2, // the top (persistent id 2)
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

    #[test]
    fn document_groups_per_object_with_materials() {
        let m = fixtures::mixed_model();
        let text = document(&m, &Scope::visible(&m), "model.mtl").unwrap();
        let heads: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("o ") || l.starts_with("g ") || l.starts_with("usemtl"))
            .collect();
        assert_eq!(
            heads,
            [
                "o Side_panel",
                "g Side_panel",
                "usemtl Oak",
                "o Block",
                "g Block",
                "o Sheet",
                "g Sheet"
            ]
        );
        assert!(text.contains("mtllib model.mtl\n"));
        // Indices continue across objects: the second box's first face uses vertices 9..
        assert_eq!(
            text.lines().filter(|l| l.starts_with("v ")).count(),
            8 + 8 + 4
        );
        let last_f = text.lines().rfind(|l| l.starts_with("f ")).unwrap();
        assert_eq!(last_f, "f 17 18 19 20", "the sheet's quad");
        let lib = mtl(&m);
        assert!(lib.contains("newmtl Oak\nKd 0.7843 0.6275 0.3922\nd 1.0000\nmap_Kd oak.jpg"));
    }

    #[test]
    fn painted_faces_and_textures() {
        let mut m = fixtures::box_model();
        for op in crate::library::ops(crate::library::find("White oak").unwrap(), None) {
            m.apply(&op).unwrap();
        }
        m.apply(&Op::Paint {
            target: sawhorse_kernel::looks::PaintTarget::Faces {
                object: 1,
                faces: vec![2],
            },
            material: Some("White oak".into()),
        })
        .unwrap();
        let text = document(&m, &Scope::visible(&m), "m.mtl").unwrap();
        assert!(text.contains("mtllib m.mtl\n"));
        let usemtl: Vec<&str> = text.lines().filter(|l| l.starts_with("usemtl")).collect();
        assert_eq!(
            usemtl,
            ["usemtl default", "usemtl White_oak", "usemtl default"]
        );
        assert_eq!(text.lines().filter(|l| l.starts_with("vt ")).count(), 4);
        assert!(
            text.lines().any(|l| l.starts_with("f ") && l.contains('/')),
            "the top refers to its texture coordinates"
        );
        let lib = mtl(&m);
        assert!(lib.contains("map_Kd lib-white-oak.png"), "{lib}");
        assert_eq!(
            texture_file("assets/lib-white-oak.png"),
            "lib-white-oak.png"
        );
    }

    #[test]
    fn duplicate_names_stay_distinct() {
        let mut m = fixtures::mixed_model();
        m.apply(&Op::RenameObject {
            object: 2,
            name: Some("Side panel".into()),
        })
        .unwrap();
        let text = document(&m, &Scope::visible(&m), "x.mtl").unwrap();
        assert!(text.contains("o Side_panel\n") && text.contains("o Side_panel_2\n"));
    }
}
