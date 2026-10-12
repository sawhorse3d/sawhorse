//! The native Sawhorse document, `.shz` (ADR-0002; the open spec is `docs/format.md`).
//!
//! A zip holding, in this order:
//!
//! | Entry | What |
//! |---|---|
//! | `mimetype` | `application/vnd.sawhorse.shz`, stored uncompressed, so the type can be sniffed from the first bytes |
//! | `scene.json` | format, version, units, and the whole model as plain data (`sawhorse_kernel::snapshot`): sketches, solids as exact f64 B-reps with persistent face ids, imported meshes, groups, tags, materials, scenes, guides, dimensions |
//! | `geometry.glb` | every solid and mesh as glTF 2.0 (Y up, metres), so any glTF viewer can show a `.shz`; also the fallback geometry for objects a writer left without a B-rep |
//! | `thumbnail.png` | a picture of the model (optional) |
//! | `assets/` | texture images (none yet: textures are kept as file-name references) |
//!
//! Reading validates everything like an edit: a file that does not validate is refused
//! with a plain message, never patched up.

use std::io::{Cursor, Read, Write};

use sawhorse_kernel::document::Document;
use sawhorse_kernel::meshbuild::ImportItem;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::snapshot::{SNAPSHOT_VERSION, Snapshot};
use zip::write::SimpleFileOptions;

pub const MIMETYPE: &str = "application/vnd.sawhorse.shz";
pub const FORMAT: &str = "sawhorse-shz";

/// `scene.json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scene {
    /// Always `sawhorse-shz`.
    pub format: String,
    /// Schema version of the model data (`SNAPSHOT_VERSION`).
    pub version: u32,
    /// Who wrote the file, e.g. `Sawhorse 0.0.1`.
    #[serde(default)]
    pub generator: String,
    #[serde(default)]
    pub units: Units,
    /// Entry holding the glTF geometry.
    #[serde(default = "default_geometry")]
    pub geometry: String,
    /// Entry holding the thumbnail, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(flatten)]
    pub model: Snapshot,
}

fn default_geometry() -> String {
    "geometry.glb".into()
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Units {
    /// Unit of every coordinate in `scene.json`: always `metre`.
    pub length: String,
    /// Up axis of `scene.json`: always `Z` (`geometry.glb` is Y up, as glTF requires).
    pub up: String,
    /// The unit system the user was working in (`frac-in`, `dec-in`, `ft-in`, `mm`, `cm`,
    /// `m`), restored on open. Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl Default for Units {
    fn default() -> Units {
        Units {
            length: "metre".into(),
            up: "Z".into(),
            display: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ShzError {
    #[error("not a Sawhorse file: {0}")]
    NotShz(String),
    #[error("this file was written by a newer Sawhorse (format version {0}); update to open it")]
    TooNew(u32),
    #[error("the file is damaged: {0}")]
    Damaged(String),
    #[error("{0}")]
    Model(String),
    #[error("could not write the file: {0}")]
    Write(String),
}

/// What to put in a saved file beyond the model.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct SaveOptions {
    /// The user's unit system, restored on open.
    #[serde(default)]
    pub display_units: Option<String>,
    /// PNG bytes of a picture of the model.
    #[serde(skip)]
    pub thumbnail_png: Option<Vec<u8>>,
    /// e.g. `Sawhorse 0.0.1`.
    #[serde(default)]
    pub generator: Option<String>,
}

/// Write `model` as a `.shz`.
pub fn write(model: &Model, opts: &SaveOptions) -> Result<Vec<u8>, ShzError> {
    let w = |e: zip::result::ZipError| ShzError::Write(e.to_string());
    let io = |e: std::io::Error| ShzError::Write(e.to_string());
    let scene = Scene {
        format: FORMAT.into(),
        version: SNAPSHOT_VERSION,
        generator: opts
            .generator
            .clone()
            .unwrap_or_else(|| format!("Sawhorse {}", sawhorse_kernel::VERSION)),
        units: Units {
            display: opts.display_units.clone(),
            ..Units::default()
        },
        geometry: default_geometry(),
        thumbnail: opts.thumbnail_png.as_ref().map(|_| "thumbnail.png".into()),
        model: model.snapshot(),
    };
    let json = serde_json::to_vec_pretty(&scene).map_err(|e| ShzError::Write(e.to_string()))?;
    let all = sawhorse_export::Scope {
        objects: model.object_ids(),
        meshes: model.meshes().into_iter().map(|(id, _)| id).collect(),
    };
    let glb = match sawhorse_export::gltf::document(model, &all) {
        Ok(b) => b,
        Err(sawhorse_export::ExportError::Empty) => sawhorse_export::gltf::glb(empty_gltf(), &[]),
        Err(e) => return Err(ShzError::Write(e.to_string())),
    };
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("mimetype", stored).map_err(w)?;
    zw.write_all(MIMETYPE.as_bytes()).map_err(io)?;
    zw.start_file("scene.json", deflated).map_err(w)?;
    zw.write_all(&json).map_err(io)?;
    zw.start_file("geometry.glb", deflated).map_err(w)?;
    zw.write_all(&glb).map_err(io)?;
    if let Some(png) = &opts.thumbnail_png {
        zw.start_file("thumbnail.png", stored).map_err(w)?;
        zw.write_all(png).map_err(io)?;
    }
    zw.add_directory("assets/", stored).map_err(w)?;
    Ok(zw.finish().map_err(w)?.into_inner())
}

fn empty_gltf() -> serde_json::Value {
    serde_json::json!({
        "asset": { "version": "2.0", "generator": "Sawhorse" },
        "scene": 0,
        "scenes": [{ "name": "Sawhorse", "nodes": [] }]
    })
}

/// An opened file.
#[derive(Debug)]
pub struct Opened {
    pub document: Document,
    pub scene: Scene,
    pub thumbnail: Option<Vec<u8>>,
    /// What the reader had to do (rebuilding objects from `geometry.glb`, ...).
    pub notes: Vec<String>,
}

fn entry(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<Vec<u8>>, ShzError> {
    match zip.by_name(name) {
        Ok(mut f) => {
            let mut v = Vec::new();
            f.read_to_end(&mut v)
                .map_err(|e| ShzError::Damaged(format!("{name}: {e}")))?;
            Ok(Some(v))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(ShzError::Damaged(format!("{name}: {e}"))),
    }
}

/// Read a `.shz` into a fresh document (its log is the `restore` op, nothing to undo).
pub fn read(bytes: &[u8]) -> Result<Opened, ShzError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ShzError::NotShz(format!("not a zip ({e})")))?;
    let json = entry(&mut zip, "scene.json")?
        .ok_or_else(|| ShzError::NotShz("there is no scene.json inside".into()))?;
    let head: serde_json::Value =
        serde_json::from_slice(&json).map_err(|e| ShzError::Damaged(format!("scene.json: {e}")))?;
    if head["format"].as_str() != Some(FORMAT) {
        return Err(ShzError::NotShz(
            "scene.json does not say format \"sawhorse-shz\"".into(),
        ));
    }
    let version = head["version"].as_u64().unwrap_or(0) as u32;
    if version > SNAPSHOT_VERSION {
        return Err(ShzError::TooNew(version));
    }
    let scene: Scene =
        serde_json::from_value(head).map_err(|e| ShzError::Damaged(format!("scene.json: {e}")))?;
    if scene.units.length != "metre" || scene.units.up != "Z" {
        return Err(ShzError::Damaged(format!(
            "scene.json units must be metre, Z up (found {}, {} up)",
            scene.units.length, scene.units.up
        )));
    }
    let mut notes = Vec::new();
    let mut document =
        Document::open(scene.model.clone()).map_err(|e| ShzError::Model(e.to_string()))?;
    // Objects saved without an exact B-rep come back from geometry.glb.
    let missing: Vec<&sawhorse_kernel::snapshot::SnapObject> = scene
        .model
        .objects
        .iter()
        .filter(|o| o.brep.is_none())
        .collect();
    if !missing.is_empty() {
        let glb = entry(&mut zip, &scene.geometry)?
            .ok_or_else(|| ShzError::Damaged(format!("{} is missing", scene.geometry)))?;
        let by_id = sawhorse_import::gltf::items_by_object(&glb)
            .map_err(|e| ShzError::Damaged(e.to_string()))?;
        let mut items: Vec<ImportItem> = Vec::new();
        for o in &missing {
            match by_id.get(&o.id) {
                Some(it) => {
                    let mut it = it.clone();
                    it.name = o.name.clone().unwrap_or_else(|| format!("Object {}", o.id));
                    it.tag = o.tag.clone();
                    it.material = o.material.clone();
                    it.group = scene
                        .model
                        .groups
                        .iter()
                        .find(|g| g.objects.contains(&o.id))
                        .map(|g| g.name.clone());
                    items.push(it);
                }
                None => notes.push(format!(
                    "object {} has no geometry in scene.json or {}; left out",
                    o.id, scene.geometry
                )),
            }
        }
        if !items.is_empty() {
            let out = document
                .apply(&Op::Import {
                    items,
                    strokes: vec![],
                    materials: vec![],
                    scenes: vec![],
                    hidden_tags: vec![],
                })
                .map_err(|e| ShzError::Model(e.to_string()))?;
            if let OpOutcome::Imported {
                objects, notes: n, ..
            } = out
            {
                notes.push(format!(
                    "rebuilt {} object{} from {}",
                    objects.len(),
                    if objects.len() == 1 { "" } else { "s" },
                    scene.geometry
                ));
                notes.extend(n);
            }
            document.clear_history();
        }
    }
    let thumbnail = match &scene.thumbnail {
        Some(t) => entry(&mut zip, t)?,
        None => None,
    };
    Ok(Opened {
        document,
        scene,
        thumbnail,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::Op;

    fn sample() -> Document {
        let mut d = Document::with_ground_sketch();
        for op in [
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6,"h":0.4}"#,
            r#"{"op":"extrude","sketch":1,"region":1,"distance":0.3}"#,
            r#"{"op":"imprint","object":1,"face":2,"points":[[0.1,0.1,0.3],[0.3,0.1,0.3],[0.3,0.3,0.3],[0.1,0.3,0.3]]}"#,
            r#"{"op":"push_pull","object":1,"face":7,"distance":-0.1}"#,
            r#"{"op":"add_rectangle","sketch":1,"x":1,"y":0,"w":0.2,"h":0.2}"#,
            r#"{"op":"define_material","name":"Oak","color":[196,154,108,255],"texture":"oak.jpg"}"#,
            r#"{"op":"set_material","object":1,"material":"Oak"}"#,
            r#"{"op":"rename_object","object":1,"name":"Box with a pocket"}"#,
            r#"{"op":"set_tag","object":1,"tag":"Case"}"#,
            r#"{"op":"create_group","name":"Case parts","objects":[1]}"#,
            r#"{"op":"add_guide","point":[0,0,0],"dir":[1,0,0]}"#,
            r#"{"op":"add_dimension","a":[0,0,0],"b":[0.6,0,0],"offset":[0,-0.05,0]}"#,
            r#"{"op":"import","items":[{"name":"Sheet","positions":[[2,0,0],[3,0,0],[3,1,0]],"faces":[[[0,1,2]]]}],"scenes":[{"name":"Top","eye":[0,0,5],"target":[0,0,0],"up":[0,1,0],"parallel":true}]}"#,
        ] {
            d.apply(&serde_json::from_str::<Op>(op).unwrap()).unwrap();
        }
        d
    }

    #[test]
    fn save_then_open_is_the_same_model() {
        let d = sample();
        let bytes = write(
            d.model(),
            &SaveOptions {
                display_units: Some("mm".into()),
                thumbnail_png: Some(b"\x89PNG fake".to_vec()),
                generator: None,
            },
        )
        .unwrap();
        // The first entry is the uncompressed mimetype, readable at a fixed offset.
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..38 + MIMETYPE.len()], MIMETYPE.as_bytes());
        let o = read(&bytes).unwrap();
        assert!(o.notes.is_empty(), "{:?}", o.notes);
        assert_eq!(o.document.hash(), d.hash(), "same model, same hash");
        assert_eq!(o.document.log().len(), 1, "the restore op");
        assert!(!o.document.can_undo());
        assert_eq!(o.scene.units.display.as_deref(), Some("mm"));
        assert_eq!(o.thumbnail.as_deref(), Some(&b"\x89PNG fake"[..]));
        let m = o.document.model();
        assert_eq!(m.object(1).unwrap().name(), Some("Box with a pocket"));
        assert_eq!(m.meshes().len(), 1);
        assert_eq!(m.scenes()[0].name, "Top");
        assert_eq!(
            m.stats().regions,
            1,
            "the second rectangle is still a region"
        );
        // Saving the opened file again writes the same scene.json.
        let again = read(&write(m, &SaveOptions::default()).unwrap()).unwrap();
        assert_eq!(again.scene.model, o.scene.model);
        // The replayed log reproduces the opened document.
        let replayed = Document::replay(o.document.log()).unwrap();
        assert_eq!(replayed.hash(), o.document.hash());
    }

    #[test]
    fn geometry_glb_rebuilds_objects_without_a_brep() {
        let d = sample();
        let bytes = write(d.model(), &SaveOptions::default()).unwrap();
        // Rewrite scene.json without the exact B-rep, as a minimal third-party writer might.
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut scene: Scene =
            serde_json::from_slice(&entry(&mut zip, "scene.json").unwrap().unwrap()).unwrap();
        scene.model.objects[0].brep = None;
        let glb = entry(&mut zip, "geometry.glb").unwrap().unwrap();
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = SimpleFileOptions::default();
        zw.start_file("scene.json", o).unwrap();
        zw.write_all(&serde_json::to_vec(&scene).unwrap()).unwrap();
        zw.start_file("geometry.glb", o).unwrap();
        zw.write_all(&glb).unwrap();
        let minimal = zw.finish().unwrap().into_inner();
        let opened = read(&minimal).unwrap();
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains("rebuilt 1 object from geometry.glb")),
            "{:?}",
            opened.notes
        );
        let (a, b) = (d.model().stats(), opened.document.model().stats());
        assert_eq!((a.objects, a.faces, a.edges), (b.objects, b.faces, b.edges));
        // geometry.glb is f32: the volume agrees to f32 precision.
        assert!((a.volume_m3 - b.volume_m3).abs() < 1e-6 * a.volume_m3);
        let o = opened.document.model().object(1).unwrap();
        assert_eq!(o.name(), Some("Box with a pocket"));
        assert_eq!(o.material(), Some("Oak"));
    }

    #[test]
    fn refusals() {
        assert!(matches!(read(b"not a zip"), Err(ShzError::NotShz(_))));
        let d = sample();
        let bytes = write(d.model(), &SaveOptions::default()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut v: serde_json::Value =
            serde_json::from_slice(&entry(&mut zip, "scene.json").unwrap().unwrap()).unwrap();
        let rezip = |v: &serde_json::Value| {
            let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
            zw.start_file("scene.json", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(&serde_json::to_vec(v).unwrap()).unwrap();
            zw.finish().unwrap().into_inner()
        };
        v["version"] = serde_json::json!(99);
        assert_eq!(read(&rezip(&v)).unwrap_err(), ShzError::TooNew(99));
        v["version"] = serde_json::json!(1);
        v["objects"][0]["brep"]["points"][0][2] = serde_json::json!(0.25);
        assert!(matches!(read(&rezip(&v)), Err(ShzError::Model(_))));
        v["format"] = serde_json::json!("something-else");
        assert!(matches!(read(&rezip(&v)), Err(ShzError::NotShz(_))));
        // Unknown fields are ignored (forward compatibility).
        let mut fresh: serde_json::Value =
            serde_json::from_slice(&entry(&mut zip, "scene.json").unwrap().unwrap()).unwrap();
        fresh["cut_list"] = serde_json::json!({"future": true});
        fresh["objects"][0]["future_field"] = serde_json::json!(1);
        assert!(read(&rezip(&fresh)).is_ok());
    }

    #[test]
    fn an_empty_document_saves_and_opens() {
        let d = Document::with_ground_sketch();
        let o = read(&write(d.model(), &SaveOptions::default()).unwrap()).unwrap();
        assert_eq!(o.document.hash(), d.hash());
    }
}
