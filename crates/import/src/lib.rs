//! Importers (ADR-0002): file bytes in, one kernel [`Op::Import`] out.
//!
//! Every reader here only *parses*: it turns a file into polygon soups ([`ImportItem`]s),
//! strokes for sketches, materials and scenes, in kernel units (metres, Z up). The kernel
//! then builds solids from the soups through its own validated path
//! (`sawhorse_kernel::meshbuild`), keeps what is not a valid solid as a visible mesh with a
//! note, and applies the whole import as one undoable op. Nothing is repaired here.
//!
//! | Format | Reader | Units | Becomes |
//! |---|---|---|---|
//! | STL (binary and ASCII) | [`stl`] | none in the file: millimetres unless told otherwise | one item per `solid` block, coplanar triangles merged |
//! | Wavefront OBJ | [`obj`] | none: millimetres unless told otherwise | one item per `o` (or `g`); `l` lines kept |
//! | glTF 2.0 / GLB | [`gltf`] | metres, Y up | one item per node with a mesh, transforms applied; `_FACE_ID` honoured |
//! | 3MF | [`three_mf`] | the file's `unit` | one item per build item (components resolved), base-material colours |
//! | DXF (ASCII) | [`dxf`] | `$INSUNITS` (millimetres when unset) | `3DFACE` and polyface meshes as items per layer; `LINE`, `LWPOLYLINE`, `POLYLINE`, `CIRCLE`, `ARC` as sketch strokes |
//! | SVG | [`svg`] | the root's `width`/`height` + `viewBox` (CSS px when unitless) | every path and shape as strokes on the ground plane |
//!
//! `.skp` is read in the webview by the npm `openskp` package (`src/io/skp.ts`), which
//! produces the same op.

pub mod dxf;
pub mod gltf;
pub mod obj;
pub mod stl;
pub mod svg;
pub mod three_mf;
mod util;

use sawhorse_kernel::meshbuild::ImportItem;
use sawhorse_kernel::model::{ImportStroke, MaterialDef, Op, SceneDef};

/// A file format we read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Stl,
    Obj,
    Gltf,
    Glb,
    #[serde(rename = "3mf")]
    ThreeMf,
    Dxf,
    Svg,
}

impl Format {
    /// From a file name's extension, then from the bytes (magic numbers and keywords).
    pub fn detect(name: &str, bytes: &[u8]) -> Option<Format> {
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let by_ext = match ext.as_str() {
            "stl" => Some(Format::Stl),
            "obj" => Some(Format::Obj),
            "gltf" => Some(Format::Gltf),
            "glb" => Some(Format::Glb),
            "3mf" => Some(Format::ThreeMf),
            "dxf" => Some(Format::Dxf),
            "svg" => Some(Format::Svg),
            _ => None,
        };
        by_ext.or_else(|| {
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
            if bytes.starts_with(b"glTF") {
                Some(Format::Glb)
            } else if bytes.starts_with(b"PK") {
                Some(Format::ThreeMf)
            } else if head.contains("<svg") {
                Some(Format::Svg)
            } else if head.trim_start().starts_with('{') && head.contains("\"asset\"") {
                Some(Format::Gltf)
            } else if head.contains("section") && head.contains("entities") {
                Some(Format::Dxf)
            } else if head.trim_start().starts_with("solid") || bytes.len() >= 84 {
                Some(Format::Stl)
            } else {
                None
            }
        })
    }
}

/// Length units a unitless file (STL, OBJ, a DXF without `$INSUNITS`) can be read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl Unit {
    pub fn metres(self) -> f64 {
        match self {
            Unit::Mm => 0.001,
            Unit::Cm => 0.01,
            Unit::M => 1.0,
            Unit::In => 0.0254,
            Unit::Ft => 0.3048,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Unit::Mm => "millimetres",
            Unit::Cm => "centimetres",
            Unit::M => "metres",
            Unit::In => "inches",
            Unit::Ft => "feet",
        }
    }
}

/// How to read a file.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct ImportOptions {
    /// Units for formats whose files do not say (STL, OBJ, a DXF without `$INSUNITS`).
    #[serde(default)]
    pub units: Option<Unit>,
    /// Name of the file (its stem names the imported objects).
    #[serde(default)]
    pub name: String,
}

impl ImportOptions {
    /// The file name without directory and extension.
    pub fn stem(&self) -> String {
        let base = self.name.rsplit(['/', '\\']).next().unwrap_or("");
        let stem = match base.rfind('.') {
            Some(i) if i > 0 => &base[..i],
            _ => base,
        };
        if stem.trim().is_empty() {
            "Imported".to_string()
        } else {
            stem.to_string()
        }
    }
}

/// What a reader found, before the kernel builds it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub items: Vec<ImportItem>,
    pub strokes: Vec<ImportStroke>,
    pub materials: Vec<MaterialDef>,
    pub scenes: Vec<SceneDef>,
    pub hidden_tags: Vec<String>,
    /// What the reader skipped or assumed (units, unsupported entities), in plain words.
    pub notes: Vec<String>,
}

impl Parsed {
    /// The kernel op that brings all of it in as one undoable step.
    pub fn op(&self) -> Op {
        Op::Import {
            items: self.items.clone(),
            strokes: self.strokes.clone(),
            materials: self.materials.clone(),
            scenes: self.scenes.clone(),
            hidden_tags: self.hidden_tags.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.strokes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ImportError {
    #[error("not a file Sawhorse can import (use STL, OBJ, glTF/GLB, 3MF, DXF, SVG or SKP)")]
    UnknownFormat,
    #[error("{format}: {message}")]
    Malformed {
        format: &'static str,
        message: String,
    },
    #[error("{0}: the file holds nothing Sawhorse can import")]
    Empty(&'static str),
}

pub(crate) fn malformed(format: &'static str, message: impl Into<String>) -> ImportError {
    ImportError::Malformed {
        format,
        message: message.into(),
    }
}

/// Read `bytes` as `format` (or the detected format).
pub fn read(
    format: Option<Format>,
    bytes: &[u8],
    opts: &ImportOptions,
) -> Result<Parsed, ImportError> {
    let format = format
        .or_else(|| Format::detect(&opts.name, bytes))
        .ok_or(ImportError::UnknownFormat)?;
    let parsed = match format {
        Format::Stl => stl::read(bytes, opts)?,
        Format::Obj => obj::read(bytes, opts)?,
        Format::Gltf | Format::Glb => gltf::read(bytes, opts)?,
        Format::ThreeMf => three_mf::read(bytes, opts)?,
        Format::Dxf => dxf::read(bytes, opts)?,
        Format::Svg => svg::read(bytes, opts)?,
    };
    if parsed.is_empty() && parsed.materials.is_empty() {
        let name = match format {
            Format::Stl => "STL",
            Format::Obj => "OBJ",
            Format::Gltf | Format::Glb => "glTF",
            Format::ThreeMf => "3MF",
            Format::Dxf => "DXF",
            Format::Svg => "SVG",
        };
        return Err(ImportError::Empty(name));
    }
    Ok(parsed)
}

#[cfg(test)]
pub(crate) mod test_support {
    use sawhorse_kernel::{Document, Op};

    pub fn two_boxes() -> Document {
        let mut d = Document::with_ground_sketch();
        for op in [
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.1,"h":0.2}"#,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.05}"#,
            r#"{"op":"add_rectangle","sketch":1,"x":0.3,"y":0,"w":0.04,"h":0.04}"#,
        ] {
            d.apply(&serde_json::from_str(op).unwrap()).unwrap();
        }
        let r = d.model().sketch(1).unwrap().regions().next().unwrap().0;
        d.apply(&Op::Extrude {
            sketch: 1,
            region: sawhorse_kernel::ids::to_public(r),
            distance: 0.03,
        })
        .unwrap();
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_then_content() {
        assert_eq!(Format::detect("a/b/Part.STL", b""), Some(Format::Stl));
        assert_eq!(Format::detect("x.3mf", b""), Some(Format::ThreeMf));
        assert_eq!(Format::detect("blob", b"glTF\x02\0\0\0"), Some(Format::Glb));
        assert_eq!(Format::detect("blob", b"PK\x03\x04"), Some(Format::ThreeMf));
        assert_eq!(Format::detect("blob", b"<?xml?><svg/>"), Some(Format::Svg));
        assert_eq!(
            Format::detect("blob", b"  0\nSECTION\n  2\nENTITIES\n"),
            Some(Format::Dxf)
        );
        assert_eq!(Format::detect("blob", b"solid x\n"), Some(Format::Stl));
        assert_eq!(Format::detect("blob", b"hello"), None);
        let o = ImportOptions {
            units: None,
            name: "C:\\models\\Shelf v2.final.stl".into(),
        };
        assert_eq!(o.stem(), "Shelf v2.final");
        assert_eq!(
            read(None, b"hello", &ImportOptions::default()),
            Err(ImportError::UnknownFormat)
        );
    }
}
