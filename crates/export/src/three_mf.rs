//! 3MF (3D Manufacturing Format, core spec 1.x): a zip (OPC package) holding
//! `[Content_Types].xml`, `_rels/.rels` and `3D/3dmodel.model`. Millimetres (the unit is
//! written explicitly), Z up, one `<object type="model">` per solid with its name, one build
//! item each, materials as a `<m:basematerials>` group with display colours. Slicers
//! (PrusaSlicer, Bambu Studio, Cura, OrcaSlicer) open it as separate parts, ready to plate.
//!
//! Written by hand rather than through the `threemf` crate so the package uses the spec's
//! recommended part name and carries names, types and colours; `threemf` reads it back in
//! the tests.

use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use sawhorse_kernel::Model;
use zip::write::SimpleFileOptions;

use crate::{ExportError, ExportMesh, MM_PER_M, Scope, object_name, solid_triangles};

pub const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
pub const MATERIAL_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/material/2015/02";
/// The OPC relationship type that marks the 3D model part.
pub const MODEL_REL: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
pub const MODEL_PART: &str = "3D/3dmodel.model";

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

/// XML-escape text for an attribute value.
pub(crate) fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// One mesh object for the model part.
struct Part {
    name: String,
    material: Option<usize>,
    positions: Vec<glam::DVec3>,
    triangles: Vec<[u32; 3]>,
}

fn package(parts: &[Part], materials: &[(String, [u8; 4])]) -> Result<Vec<u8>, ExportError> {
    let mut xml = String::new();
    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        xml,
        r#"<model unit="millimeter" xml:lang="en-US" xmlns="{CORE_NS}" xmlns:m="{MATERIAL_NS}">"#
    );
    let _ = writeln!(xml, r#"  <metadata name="Application">Sawhorse</metadata>"#);
    let _ = writeln!(xml, "  <resources>");
    let mat_group = parts.len() + 1;
    if !materials.is_empty() {
        let _ = writeln!(xml, r#"    <m:basematerials id="{mat_group}">"#);
        for (name, c) in materials {
            let _ = writeln!(
                xml,
                "      <m:base name=\"{}\" displaycolor=\"#{:02X}{:02X}{:02X}{:02X}\"/>",
                esc(name),
                c[0],
                c[1],
                c[2],
                c[3]
            );
        }
        let _ = writeln!(xml, "    </m:basematerials>");
    }
    for (k, p) in parts.iter().enumerate() {
        let pid = match p.material {
            Some(i) => format!(r#" pid="{mat_group}" pindex="{i}""#),
            None => String::new(),
        };
        let _ = writeln!(
            xml,
            r#"    <object id="{}" type="model" name="{}"{pid}>"#,
            k + 1,
            esc(&p.name)
        );
        let _ = writeln!(xml, "      <mesh>\n        <vertices>");
        for v in &p.positions {
            let v = *v * MM_PER_M;
            let _ = writeln!(
                xml,
                r#"          <vertex x="{}" y="{}" z="{}"/>"#,
                v.x, v.y, v.z
            );
        }
        let _ = writeln!(xml, "        </vertices>\n        <triangles>");
        for t in &p.triangles {
            let _ = writeln!(
                xml,
                r#"          <triangle v1="{}" v2="{}" v3="{}"/>"#,
                t[0], t[1], t[2]
            );
        }
        let _ = writeln!(xml, "        </triangles>\n      </mesh>\n    </object>");
    }
    let _ = writeln!(xml, "  </resources>\n  <build>");
    for k in 0..parts.len() {
        let _ = writeln!(xml, r#"    <item objectid="{}"/>"#, k + 1);
    }
    let _ = writeln!(xml, "  </build>\n</model>");

    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/{MODEL_PART}" Id="rel0" Type="{MODEL_REL}"/>
</Relationships>
"#
    );
    let z = |e: zip::result::ZipError| ExportError::Zip(e.to_string());
    let io = |e: std::io::Error| ExportError::Zip(e.to_string());
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        (MODEL_PART, xml.as_bytes()),
    ] {
        zw.start_file(name, opts).map_err(z)?;
        zw.write_all(body).map_err(io)?;
    }
    Ok(zw.finish().map_err(z)?.into_inner())
}

/// One watertight triangle mesh as a single-object 3MF.
pub fn write(mesh: &ExportMesh<'_>) -> Result<Vec<u8>, ExportError> {
    mesh.check()?;
    package(
        &[Part {
            name: "Object 1".into(),
            material: None,
            positions: mesh.positions.to_vec(),
            triangles: mesh.triangles.to_vec(),
        }],
        &[],
    )
}

/// Every solid in `scope` as its own named 3MF object (refuses non-solid meshes, see
/// [`Scope::printable`]); materials become base materials with display colours.
pub fn document(model: &Model, scope: &Scope) -> Result<Vec<u8>, ExportError> {
    let solids = scope.printable(model)?;
    let mut materials: Vec<(String, [u8; 4])> = Vec::new();
    let mut parts = Vec::new();
    for (id, o) in solids {
        let material = o.material().and_then(|name| {
            let def = model.materials().get(name)?;
            Some(match materials.iter().position(|(n, _)| n == name) {
                Some(i) => i,
                None => {
                    materials.push((name.to_string(), def.color));
                    materials.len() - 1
                }
            })
        });
        let (positions, triangles, _) = solid_triangles(o)?;
        parts.push(Part {
            name: object_name(model, id),
            material,
            positions,
            triangles,
        });
    }
    package(&parts, &materials)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use glam::DVec3;
    use std::collections::BTreeMap;
    use std::io::Read;

    #[test]
    fn writes_a_zip_with_the_model() {
        let p = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z];
        let t = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let bytes = write(&ExportMesh {
            positions: &p,
            triangles: &t,
            watertight: true,
        })
        .unwrap();
        assert_eq!(&bytes[..2], b"PK", "3MF is a zip container");
        let models = threemf::read(Cursor::new(&bytes)).unwrap();
        assert_eq!(models.len(), 1);
        let mesh = models[0].resources.object[0].mesh.as_ref().unwrap();
        assert_eq!(mesh.vertices.vertex[1].x, 1000.0, "millimetres");
    }

    /// The 3MF core spec basics a slicer relies on, checked on the package we write.
    #[test]
    fn document_meets_the_core_spec_basics() {
        let m = fixtures::mixed_model();
        let bytes = document(
            &m,
            &Scope {
                objects: vec![1, 2],
                meshes: vec![],
            },
        )
        .unwrap();
        // OPC package: the three parts, content types for both extensions, a 3D model
        // relationship pointing at the model part.
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["3D/3dmodel.model", "[Content_Types].xml", "_rels/.rels"]
        );
        let mut read = |n: &str| {
            let mut s = String::new();
            zip.by_name(n).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        let ct = read("[Content_Types].xml");
        assert!(ct.contains(r#"Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml""#));
        assert!(ct.contains(r#"Extension="rels""#));
        let rels = read("_rels/.rels");
        assert!(rels.contains(&format!(r#"Target="/{MODEL_PART}""#)));
        assert!(rels.contains(MODEL_REL));
        let xml = read(MODEL_PART);
        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(xml.contains(r#"unit="millimeter""#));
        assert!(xml.contains(r#"name="Side panel""#));
        assert!(xml.contains(r##"displaycolor="#C8A064FF""##));

        // Parsed back: two model objects, a build item for each, closed manifold meshes
        // (every edge used exactly twice, once in each direction), in-range indices, no
        // degenerate triangles, and positive volume (outward winding).
        let models = threemf::read(Cursor::new(&bytes)).unwrap();
        let model = &models[0];
        assert_eq!(model.resources.object.len(), 2);
        let ids: Vec<usize> = model.resources.object.iter().map(|o| o.id).collect();
        let items: Vec<usize> = model.build.item.iter().map(|i| i.objectid).collect();
        assert_eq!(ids, items);
        let vols: Vec<f64> = model
            .resources
            .object
            .iter()
            .map(|o| {
                let mesh = o.mesh.as_ref().unwrap();
                let v = &mesh.vertices.vertex;
                let mut edges: BTreeMap<(usize, usize), i32> = BTreeMap::new();
                let mut vol = 0.0;
                for t in &mesh.triangles.triangle {
                    let k = [t.v1, t.v2, t.v3];
                    assert!(k.iter().all(|&i| i < v.len()));
                    assert!(k[0] != k[1] && k[1] != k[2] && k[0] != k[2]);
                    for e in 0..3 {
                        *edges.entry((k[e], k[(e + 1) % 3])).or_default() += 1;
                    }
                    let p = |i: usize| DVec3::new(v[i].x, v[i].y, v[i].z);
                    vol += p(k[0]).dot(p(k[1]).cross(p(k[2]))) / 6.0;
                }
                for (&(a, b), &n) in &edges {
                    assert_eq!(n, 1, "directed edge {a}->{b} used once");
                    assert_eq!(edges.get(&(b, a)), Some(&1), "and its twin once");
                }
                vol
            })
            .collect();
        assert!((vols[0] - 100.0 * 200.0 * 50.0).abs() < 1e-6, "{vols:?}");
        assert!((vols[1] - 50.0 * 50.0 * 20.0).abs() < 1e-6);
    }

    #[test]
    fn document_refuses_open_meshes() {
        let m = fixtures::mixed_model();
        assert!(matches!(
            document(&m, &Scope::visible(&m)),
            Err(ExportError::NotSolid(..))
        ));
    }

    #[test]
    fn names_are_escaped() {
        assert_eq!(esc(r#"a<b>&"c'"#), "a&lt;b&gt;&amp;&quot;c&apos;");
    }
}
