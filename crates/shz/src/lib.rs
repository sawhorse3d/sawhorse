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
//! | `assets/` | texture images the materials use (lane 10), PNG or JPEG, one entry per image, named by `scene.json`'s `textures[].path` |
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
    /// Who wrote the file, e.g. `Sawhorse 0.1.0-alpha.2`.
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
    /// A picture of the model: PNG, WebP or JPEG bytes (the name is older than WebP and
    /// JPEG). Stored as `thumbnail.png` / `.webp` / `.jpg` by its bytes; one over
    /// [`MAX_THUMBNAIL_BYTES`] or of another type is left out (lane 15).
    #[serde(skip)]
    pub thumbnail_png: Option<Vec<u8>>,
    /// e.g. `Sawhorse 0.1.0-alpha.2`.
    #[serde(default)]
    pub generator: Option<String>,
}

/// Largest thumbnail written into a `.shz` (lane 15): 100 KB.
pub const MAX_THUMBNAIL_BYTES: usize = 100 * 1024;

/// The thumbnail's entry name by its bytes: PNG, WebP or JPEG; `None` for anything else.
pub fn thumbnail_entry(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("thumbnail.png")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("thumbnail.webp")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("thumbnail.jpg")
    } else {
        None
    }
}

/// Write `model` as a `.shz`.
pub fn write(model: &Model, opts: &SaveOptions) -> Result<Vec<u8>, ShzError> {
    // Lane 15: a thumbnail of a known type and at most 100 KB, else none.
    let thumb = opts
        .thumbnail_png
        .as_deref()
        .filter(|b| b.len() <= MAX_THUMBNAIL_BYTES)
        .and_then(|b| thumbnail_entry(b).map(|name| (name, b)));
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
        thumbnail: thumb.map(|(name, _)| name.into()),
        model: model.snapshot(),
    };
    // Texture images go into their own entries (`assets/...`), not into scene.json.
    let mut scene = scene;
    let images: Vec<(String, Vec<u8>)> = scene
        .model
        .textures
        .iter_mut()
        .map(|t| (t.path.clone(), std::mem::take(&mut t.data)))
        .collect();
    let json = serde_json::to_vec_pretty(&scene).map_err(|e| ShzError::Write(e.to_string()))?;
    let all = sawhorse_export::Scope {
        objects: model.object_ids(),
        meshes: model.meshes().into_iter().map(|(id, _)| id).collect(),
    };
    // The images are their own entries: geometry.glb keeps the texture names only.
    let glb_opts = sawhorse_export::gltf::GlbOptions { textures: false };
    let glb = match sawhorse_export::gltf::document_with(model, &all, glb_opts) {
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
    if let Some((name, bytes)) = thumb {
        zw.start_file(name, stored).map_err(w)?;
        zw.write_all(bytes).map_err(io)?;
    }
    zw.add_directory("assets/", stored).map_err(w)?;
    // Already compressed images are stored as they are.
    for (path, data) in &images {
        zw.start_file(path.as_str(), stored).map_err(w)?;
        zw.write_all(data).map_err(io)?;
    }
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
    let mut scene = scene;
    // Texture images come from their entries; a missing one leaves its materials showing
    // their colour (the path stays as a file-name reference).
    let mut kept = Vec::new();
    for mut t in std::mem::take(&mut scene.model.textures) {
        if t.data.is_empty() {
            match entry(&mut zip, &t.path)? {
                Some(d) if !d.is_empty() => t.data = d,
                _ => {
                    notes.push(format!(
                        "texture {} is missing from the file; its materials show their colour",
                        t.path
                    ));
                    continue;
                }
            }
        }
        kept.push(t);
    }
    scene.model.textures = kept;
    // Objects saved without an exact B-rep (and not derived from a component definition)
    // come back from geometry.glb: in place, when the mesh rebuilds into exactly one solid
    // (so they keep their id, group and order); otherwise imported afterwards.
    let mut model = scene.model.clone();
    let missing: Vec<usize> = model
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.brep.is_none() && o.slot.is_none())
        .map(|(i, _)| i)
        .collect();
    let mut leftovers: Vec<ImportItem> = Vec::new();
    let mut rebuilt = 0usize;
    if !missing.is_empty() {
        let glb = entry(&mut zip, &scene.geometry)?
            .ok_or_else(|| ShzError::Damaged(format!("{} is missing", scene.geometry)))?;
        let by_id = sawhorse_import::gltf::items_by_object(&glb)
            .map_err(|e| ShzError::Damaged(e.to_string()))?;
        let mut drop = Vec::new();
        for i in missing {
            let o = &model.objects[i];
            let Some(it) = by_id.get(&o.id) else {
                notes.push(format!(
                    "object {} has no geometry in scene.json or {}; left out",
                    o.id, scene.geometry
                ));
                drop.push(i);
                continue;
            };
            let mut it = it.clone();
            it.name = o.name.clone().unwrap_or_else(|| format!("Object {}", o.id));
            it.tag = o.tag.clone();
            it.material = o.material.clone();
            match sawhorse_kernel::meshbuild::build_item(&it) {
                Ok(b) if b.solids.len() == 1 && b.loose.is_none() => {
                    let solid = &b.solids[0];
                    let so = &mut model.objects[i];
                    so.brep = Some(sawhorse_kernel::snapshot::snap_brep(solid));
                    so.next_face_id = so.next_face_id.max(solid.next_face_id());
                    rebuilt += 1;
                    notes.extend(b.notes);
                }
                _ => {
                    it.group = None;
                    leftovers.push(it);
                    drop.push(i);
                }
            }
        }
        // What did not rebuild in place is imported below (or was left out).
        for i in drop.into_iter().rev() {
            model.objects.remove(i);
        }
    }
    let mut document = Document::open(model).map_err(|e| ShzError::Model(e.to_string()))?;
    if !leftovers.is_empty() {
        let out = document
            .apply(&Op::Import {
                items: leftovers,
                strokes: vec![],
                materials: vec![],
                scenes: vec![],
                hidden_tags: vec![],
                definitions: vec![],
                nodes: vec![],
            })
            .map_err(|e| ShzError::Model(e.to_string()))?;
        if let OpOutcome::Imported {
            objects, notes: n, ..
        } = out
        {
            rebuilt += objects.len();
            notes.extend(n);
        }
        document.clear_history();
    }
    if rebuilt > 0 {
        notes.insert(
            0,
            format!(
                "rebuilt {} object{} from {}",
                rebuilt,
                if rebuilt == 1 { "" } else { "s" },
                scene.geometry
            ),
        );
    }
    let thumbnail = match &scene.thumbnail {
        Some(t) => entry(&mut zip, t)?,
        None => None,
    };
    // The opened scene describes the file: texture bytes stay in the model, not here.
    for t in &mut scene.model.textures {
        t.data.clear();
    }
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
    fn thumbnails_are_png_webp_or_jpeg_at_most_100_kb() {
        let d = sample();
        let save = |t: Vec<u8>| {
            write(
                d.model(),
                &SaveOptions {
                    display_units: None,
                    thumbnail_png: Some(t),
                    generator: None,
                },
            )
            .unwrap()
        };
        let webp = [b"RIFF".as_slice(), &[0, 0, 0, 0], b"WEBPVP8 ", &[1, 2, 3]].concat();
        let o = read(&save(webp.clone())).unwrap();
        assert_eq!(o.scene.thumbnail.as_deref(), Some("thumbnail.webp"));
        assert_eq!(o.thumbnail.as_deref(), Some(webp.as_slice()));
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2];
        assert_eq!(
            read(&save(jpeg)).unwrap().scene.thumbnail.as_deref(),
            Some("thumbnail.jpg")
        );
        // Over 100 KB, or not a picture: left out, the model saved all the same.
        let mut big = b"\x89PNG".to_vec();
        big.resize(MAX_THUMBNAIL_BYTES + 1, 0);
        let o = read(&save(big)).unwrap();
        assert_eq!(o.scene.thumbnail, None);
        assert_eq!(o.thumbnail, None);
        assert_eq!(o.document.hash(), d.hash());
        assert_eq!(
            read(&save(b"GIF89a".to_vec())).unwrap().scene.thumbnail,
            None
        );
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
        // Rebuilt in place: still in its group, with its id.
        assert_eq!(o.membership().parent(), Some(1));
        assert_eq!(opened.document.model().groups()[0].1.name, "Case parts");
    }

    /// A bookshelf of components (two sides, four shelves from an array), one shelf made
    /// unique, a group, tags with colours, hidden and locked items, scenes (one opening a
    /// group) and an open editing context.
    fn structured() -> Document {
        let mut d = Document::with_ground_sketch();
        let x = |t: [f64; 3]| {
            format!(
                r#"{{"linear":[[1,0,0],[0,1,0],[0,0,1]],"translation":[{},{},{}]}}"#,
                t[0], t[1], t[2]
            )
        };
        for op in [
            r#"{"op":"draw","target":{"on":"plane","origin":[0,0,0],"normal":[0,0,1]},"points":[[0,0,0],[0.01905,0,0],[0.01905,0.28575,0],[0,0.28575,0]],"closed":true}"#.to_string(),
            r#"{"op":"extrude","sketch":1,"region":1,"distance":1.8288}"#.into(),
            r#"{"op":"make_component","items":[{"object":1}],"name":"Side"}"#.into(),
            format!(r#"{{"op":"copy","objects":[],"nodes":[1],"xform":{}}}"#, x([0.86995, 0.0, 0.0])),
            r#"{"op":"draw","target":{"on":"plane","origin":[0,0,0],"normal":[0,0,1]},"points":[[0.01905,0,0],[0.86995,0,0],[0.86995,0.28575,0],[0.01905,0.28575,0]],"closed":true}"#.into(),
        ] {
            d.apply(&serde_json::from_str::<Op>(&op).unwrap()).unwrap();
        }
        // The shelf region is the newest region of the ground sketch.
        let region = d
            .model()
            .sketch(1)
            .unwrap()
            .regions()
            .map(|(k, _)| sawhorse_kernel::ids::to_public(k))
            .max()
            .unwrap();
        for op in [
            format!(r#"{{"op":"extrude","sketch":1,"region":{region},"distance":0.01905}}"#),
            r#"{"op":"make_component","items":[{"object":3}],"name":"Shelf"}"#.into(),
            format!(r#"{{"op":"array","objects":[],"nodes":[3],"xform":{},"count":3}}"#, x([0.0, 0.0, 0.6])),
            r#"{"op":"make_unique","node":6}"#.into(),
            r#"{"op":"make_group","items":[{"node":1},{"node":2}],"name":"Carcass"}"#.into(),
            r#"{"op":"create_tag","name":"Carcass","color":[196,138,42]}"#.into(),
            r#"{"op":"create_tag","name":"Shelves","color":[106,155,90]}"#.into(),
            r#"{"op":"tag_items","items":[{"node":7}],"tag":"Carcass"}"#.into(),
            r#"{"op":"tag_items","items":[{"node":3},{"node":4},{"node":5},{"node":6}],"tag":"Shelves"}"#.into(),
            r#"{"op":"set_hidden","items":[{"node":6}],"hidden":true}"#.into(),
            r#"{"op":"set_locked","items":[{"node":5}],"locked":true}"#.into(),
            r#"{"op":"add_scene","scene":{"name":"Front","eye":[0.4,-3,0.9],"target":[0.4,0,0.9],"up":[0,0,1],"fov":20,"parallel":true,"hidden_tags":["Shelves"]}}"#.into(),
            r#"{"op":"add_scene","scene":{"name":"Iso","eye":[3,-3,3],"target":[0.4,0.14,0.9],"up":[0,0,1],"fov":35,"parallel":false,"context":7}}"#.into(),
            r#"{"op":"set_context","node":7}"#.into(),
        ] {
            d.apply(&serde_json::from_str::<Op>(&op).unwrap())
                .unwrap_or_else(|e| panic!("{op}: {e}"));
        }
        d
    }

    #[test]
    fn groups_components_tags_and_scenes_round_trip() {
        let d = structured();
        let m = d.model();
        assert_eq!(m.definitions().len(), 3, "Side, Shelf, Shelf (2)");
        let bytes = write(m, &SaveOptions::default()).unwrap();
        let o = read(&bytes).unwrap();
        assert!(o.notes.is_empty(), "{:?}", o.notes);
        let s = &o.scene.model;
        // Geometry is stored once per definition: only the definitions carry B-reps.
        assert_eq!(s.definitions.len(), 3);
        assert!(
            s.objects
                .iter()
                .all(|x| x.brep.is_none() && x.slot.is_some())
        );
        assert_eq!(s.nodes.len(), 7);
        assert_eq!(s.context, Some(7));
        assert_eq!(s.tag_colors["Carcass"], "#c48a2a");
        assert_eq!(s.scenes[1].context, Some(7));
        // The same model: hash, tree, context, flags.
        let back = o.document.model();
        assert_eq!(o.document.hash(), d.hash());
        assert_eq!(back.graph(), m.graph());
        assert_eq!(back.context(), Some(7));
        assert!(!back.node_visible(6));
        assert!(back.node(5).unwrap().member.locked);
        // And the instances still follow their definitions after opening: edit inside one
        // shelf, its siblings change, the unique one does not.
        let mut doc = o.document;
        doc.apply(&Op::SetContext { node: Some(4) }).unwrap();
        let solid = back_solid(&doc, 4);
        doc.apply(&Op::PushPull {
            object: solid,
            face: 2,
            distance: 0.00635,
        })
        .unwrap();
        let h = |doc: &Document, n: u64| {
            let o = doc.model().object(back_solid(doc, n)).unwrap();
            let (lo, hi) = o.bounds();
            hi.z - lo.z
        };
        for n in [3, 4, 5] {
            assert!((h(&doc, n) - 0.0254).abs() < 1e-9, "shelf {n}");
        }
        assert!((h(&doc, 6) - 0.01905).abs() < 1e-9, "the unique shelf");
        // Saving what was opened writes the same scene.json.
        let again = read(&write(back_ref(&d), &SaveOptions::default()).unwrap()).unwrap();
        assert_eq!(again.scene.model, *s);
    }

    fn back_solid(d: &Document, node: u64) -> u64 {
        d.model()
            .graph()
            .objects
            .iter()
            .find(|o| o.parent == Some(node))
            .unwrap()
            .id
    }

    fn back_ref(d: &Document) -> &sawhorse_kernel::Model {
        d.model()
    }

    #[test]
    fn lane_6_files_with_flat_groups_still_open() {
        // A lane-6 writer knew only `groups` (names and solid ids) and `tags` (visibility).
        let d = sample();
        let mut snap = d.model().snapshot();
        snap.nodes.clear();
        snap.tag_colors.clear();
        for o in &mut snap.objects {
            o.parent = None;
        }
        let m = sawhorse_kernel::Model::from_snapshot(&snap).unwrap();
        assert_eq!(m.groups()[0].1.name, "Case parts");
        assert_eq!(m.groups()[0].1.objects, vec![1]);
        assert_eq!(m.hash(), d.model().hash(), "the same model");
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

    /// The painted golden plus a library texture: paint, face positions, styles, shadows and
    /// texture images survive a save and an open; images are their own `assets/` entries.
    #[test]
    fn materials_textures_styles_and_shadows_round_trip() {
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../../kernel/tests/golden/painted.json")).unwrap();
        let ops: Vec<Op> = serde_json::from_value(golden["ops"].clone()).unwrap();
        let mut d = Document::replay(&ops).unwrap();
        let walnut = sawhorse_export::library::find("Walnut").unwrap();
        for op in sawhorse_export::library::ops(walnut, None) {
            d.apply(&op).unwrap();
        }
        let bytes = write(d.model(), &SaveOptions::default()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        let png = entry(&mut zip, "assets/lib-walnut.png").unwrap().unwrap();
        assert_eq!(
            sawhorse_kernel::looks::image_size(&png).map(|x| x.0),
            Some("image/png")
        );
        assert!(entry(&mut zip, "assets/oak.png").unwrap().is_some());
        let json = String::from_utf8(entry(&mut zip, "scene.json").unwrap().unwrap()).unwrap();
        assert!(
            !json.contains("\"data\""),
            "image bytes stay out of scene.json"
        );
        assert!(
            json.contains("\"stock\"") && json.contains("\"style\"") && json.contains("\"uv\"")
        );
        let o = read(&bytes).unwrap();
        assert_eq!(o.document.hash(), d.hash());
        assert!(o.notes.is_empty(), "{:?}", o.notes);
        // A file whose image entry went missing still opens: the materials show their colour.
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["mimetype", "scene.json", "geometry.glb"] {
            zw.start_file(name, SimpleFileOptions::default()).unwrap();
            zw.write_all(&entry(&mut zip, name).unwrap().unwrap())
                .unwrap();
        }
        let partial = zw.finish().unwrap().into_inner();
        let o = read(&partial).unwrap();
        assert!(o.document.model().textures().is_empty());
        assert_eq!(o.notes.len(), 2, "{:?}", o.notes);
        assert!(o.notes[0].contains("missing"));
    }

    #[test]
    fn an_empty_document_saves_and_opens() {
        let d = Document::with_ground_sketch();
        let o = read(&write(d.model(), &SaveOptions::default()).unwrap()).unwrap();
        assert_eq!(o.document.hash(), d.hash());
    }
}
