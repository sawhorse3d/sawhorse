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
//!
//! Lane 16: reading is bounded before anything is inflated. The archive may hold at most
//! [`MAX_ENTRIES`] entries, none named twice, none sharing compressed bytes with another;
//! every entry read is checked against its own cap by its declared size (and its declared
//! ratio), read through that cap, and must be exactly the size it declares; all entries read
//! together stay under [`MAX_TOTAL`]. `scene.json` is parsed straight into the model's
//! types (never into a generic JSON tree). Texture rows are counted and checked for repeats
//! before a single image is read. `plugin-data.json` is read here too (capped), so the app
//! never unzips a user's file itself.

use std::io::{Cursor, Write};

mod declared;

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
    /// Who wrote the file, e.g. `Sawhorse 0.1.0-alpha.3`.
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
    /// Lane 16b: the file lists more of something than a model may hold.
    #[error("{0}")]
    Limit(sawhorse_kernel::limits::LimitError),
}

/// Most entries a `.shz` may hold (lane 16): 512 textures and a handful of others fit with
/// room to spare.
pub const MAX_ENTRIES: usize = 2048;
/// Largest `scene.json` read (declared and real size).
pub const MAX_SCENE_JSON: u64 = 256 * 1024 * 1024;
/// Largest `geometry.glb` read.
pub const MAX_GEOMETRY: u64 = 512 * 1024 * 1024;
/// Largest `plugin-data.json` read or written: the plugins' document stores (1 MiB of
/// characters in the app; up to 4 MiB as UTF-8).
pub const MAX_PLUGIN_DATA: u64 = 4 * 1024 * 1024;
/// Everything read from one file together.
pub const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
/// The most an entry over 1 MiB may claim to inflate per compressed byte (see
/// `sawhorse_import::archive`).
pub const MAX_RATIO: u64 = sawhorse_import::archive::MAX_RATIO;
/// The entry the app's plugins keep their document stores in (ADR-0009).
pub const PLUGIN_DATA: &str = "plugin-data.json";

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
    /// e.g. `Sawhorse 0.1.0-alpha.3`.
    #[serde(default)]
    pub generator: Option<String>,
    /// The plugins' document stores, written as `plugin-data.json` (lane 16: the app hands
    /// it to the kernel instead of unzipping and rezipping the saved file). At most
    /// [`MAX_PLUGIN_DATA`] bytes.
    #[serde(skip)]
    pub plugin_data: Option<String>,
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
    if opts
        .plugin_data
        .as_ref()
        .is_some_and(|d| d.len() as u64 > MAX_PLUGIN_DATA)
    {
        return Err(ShzError::Write(format!(
            "the plugins' document data is larger than {} MB",
            MAX_PLUGIN_DATA / 1024 / 1024
        )));
    }
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
    if let Some(data) = &opts.plugin_data {
        zw.start_file(PLUGIN_DATA, deflated).map_err(w)?;
        zw.write_all(data.as_bytes()).map_err(io)?;
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
    /// `plugin-data.json` as it is in the file (lane 16), when present and readable text.
    pub plugin_data: Option<String>,
}

/// The archive, read through `sawhorse_import::archive` (lane 16).
struct Archive<'a>(sawhorse_import::archive::Archive<'a>);

fn shz_error(e: sawhorse_import::archive::ArchiveError) -> ShzError {
    match e {
        sawhorse_import::archive::ArchiveError::NotZip(m) => {
            ShzError::NotShz(format!("not a zip ({m})"))
        }
        sawhorse_import::archive::ArchiveError::Damaged(m) => ShzError::Damaged(m),
    }
}

impl<'a> Archive<'a> {
    fn open(bytes: &'a [u8]) -> Result<Archive<'a>, ShzError> {
        sawhorse_import::archive::Archive::open(bytes, MAX_ENTRIES, MAX_TOTAL)
            .map(Archive)
            .map_err(shz_error)
    }

    fn entry(&mut self, name: &str, cap: u64) -> Result<Option<Vec<u8>>, ShzError> {
        self.0.entry(name, cap).map_err(shz_error)
    }
}

/// Read a `.shz` into a fresh document (its log is the `restore` op, nothing to undo).
pub fn read(bytes: &[u8]) -> Result<Opened, ShzError> {
    let mut zip = Archive::open(bytes)?;
    // The format, the version and what scene.json lists, counted as it inflates (a pass
    // that skips everything without building it, and stops at the first list past a model
    // limit: lane 16b), then the scene straight into its types: no generic JSON tree of a
    // large file.
    let (json, head) = match zip
        .0
        .entry_inspected("scene.json", MAX_SCENE_JSON, |r| declared::check(r))
        .map_err(shz_error)?
    {
        None => return Err(ShzError::NotShz("there is no scene.json inside".into())),
        Some(Ok(both)) => both,
        Some(Err(declared::Stop::Limit(e))) => return Err(ShzError::Limit(e)),
        Some(Err(declared::Stop::Json(e))) => {
            // Not JSON as far as it was read: if the entry is not what its header says
            // (a size lie, a damaged stream), that is the reason; read it whole to know.
            zip.entry("scene.json", MAX_SCENE_JSON)?;
            return Err(ShzError::Damaged(format!("scene.json: {e}")));
        }
    };
    if head.format.as_deref() != Some(FORMAT) {
        return Err(ShzError::NotShz(
            "scene.json does not say format \"sawhorse-shz\"".into(),
        ));
    }
    let version = head
        .version
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(u64::from(u32::MAX)) as u32;
    if version > SNAPSHOT_VERSION {
        return Err(ShzError::TooNew(version));
    }
    let scene: Scene =
        serde_json::from_slice(&json).map_err(|e| ShzError::Damaged(format!("scene.json: {e}")))?;
    drop(json);
    if scene.units.length != "metre" || scene.units.up != "Z" {
        return Err(ShzError::Damaged(format!(
            "scene.json units must be metre, Z up (found {}, {} up)",
            scene.units.length, scene.units.up
        )));
    }
    let mut notes = Vec::new();
    let mut scene = scene;
    // Texture rows are counted and checked for repeats before any image is read (lane 16:
    // 5,000 rows naming one image used to inflate it 5,000 times).
    if scene.model.textures.len() > sawhorse_kernel::looks::MAX_TEXTURES {
        return Err(ShzError::Damaged(format!(
            "scene.json lists {} textures (at most {})",
            scene.model.textures.len(),
            sawhorse_kernel::looks::MAX_TEXTURES
        )));
    }
    {
        let mut seen = std::collections::BTreeSet::new();
        for t in &scene.model.textures {
            if !seen.insert(t.path.as_str()) {
                return Err(ShzError::Damaged(format!(
                    "texture {} is listed twice",
                    t.path
                )));
            }
        }
    }
    // Texture images come from their entries; a missing one leaves its materials showing
    // their colour (the path stays as a file-name reference).
    let mut kept = Vec::new();
    for mut t in std::mem::take(&mut scene.model.textures) {
        if t.data.is_empty() {
            match zip.entry(&t.path, sawhorse_kernel::looks::MAX_TEXTURE_BYTES as u64)? {
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
        let glb = zip
            .entry(&scene.geometry, MAX_GEOMETRY)?
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
        // What did not rebuild in place is imported below (or was left out). One pass
        // (lane 16: removing one at a time was quadratic in the objects left out).
        let drop: std::collections::BTreeSet<usize> = drop.into_iter().collect();
        let mut k = 0;
        model.objects.retain(|_| {
            k += 1;
            !drop.contains(&(k - 1))
        });
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
        Some(t) => zip.entry(t, MAX_THUMBNAIL_BYTES as u64)?,
        None => None,
    };
    let plugin_data = match zip.entry(PLUGIN_DATA, MAX_PLUGIN_DATA)? {
        Some(b) => match String::from_utf8(b) {
            Ok(t) => Some(t),
            Err(_) => {
                notes.push(format!(
                    "{PLUGIN_DATA} is not text; the plugins' data in it was left out"
                ));
                None
            }
        },
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
        plugin_data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::Op;
    use std::io::Read;

    /// An entry, read whole (tests only).
    fn entry(
        zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
        name: &str,
    ) -> Result<Option<Vec<u8>>, ShzError> {
        match zip.by_name(name) {
            Ok(mut f) => {
                let mut v = Vec::new();
                f.read_to_end(&mut v)
                    .map_err(|e| ShzError::Damaged(e.to_string()))?;
                Ok(Some(v))
            }
            Err(_) => Ok(None),
        }
    }

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
                    plugin_data: None,
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
                plugin_data: None,
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

    // ---- lane 16: what a reader refuses before inflating ------------------------------------

    /// A raw Deflate stream inflating to `n` zero bytes (`n` a multiple of 64 MiB, or small),
    /// built in milliseconds: one fully-flushed block run repeated, then a final empty block.
    fn deflate_zeros(n: u64) -> Vec<u8> {
        use flate2::{Compress, Compression, FlushCompress};
        let chunk = n.min(64 << 20) as usize;
        let mut c = Compress::new(Compression::best(), false);
        let zeros = vec![0u8; chunk];
        let mut block = Vec::with_capacity(chunk / 500 + 1024);
        let mut fed = 0;
        while fed < chunk {
            let before = c.total_in() as usize;
            c.compress_vec(&zeros[fed..], &mut block, FlushCompress::Full)
                .unwrap();
            fed += c.total_in() as usize - before;
            if block.len() == block.capacity() {
                block.reserve(block.capacity());
            }
        }
        // Make sure the full flush finished (output fits).
        loop {
            let cap = block.capacity();
            c.compress_vec(&[], &mut block, FlushCompress::Full)
                .unwrap();
            if block.len() < cap {
                break;
            }
            block.reserve(cap);
        }
        let mut out = Vec::new();
        for _ in 0..(n / chunk as u64) {
            out.extend_from_slice(&block);
        }
        out.extend_from_slice(&[0x03, 0x00]);
        out
    }

    /// A zip of raw entries: (name, method, stored data, declared size, crc), with a correct
    /// directory but whatever sizes it is told.
    fn raw_zip(entries: &[(&str, u16, Vec<u8>, u64, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, method, data, size, crc) in entries {
            let off = out.len() as u32;
            let mut h = Vec::new();
            h.extend_from_slice(&0x04034b50u32.to_le_bytes());
            h.extend_from_slice(&20u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&method.to_le_bytes());
            h.extend_from_slice(&[0, 0, 0, 0]);
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(*size as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&h);
            out.extend_from_slice(data);
            cd.extend_from_slice(&0x02014b50u32.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&method.to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0]);
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(*size as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x06054b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn scene_json_of(d: &Document) -> serde_json::Value {
        let bytes = write(d.model(), &SaveOptions::default()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        serde_json::from_slice(&entry(&mut zip, "scene.json").unwrap().unwrap()).unwrap()
    }

    fn rezip(v: &serde_json::Value) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zw.start_file("scene.json", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(&serde_json::to_vec(v).unwrap()).unwrap();
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn a_zip_bomb_is_refused_by_its_declared_size_before_anything_is_inflated() {
        // About 2 MB of Deflate that inflates to 2 GiB of zeros, honestly declared.
        let bomb = deflate_zeros(2 << 30);
        assert!(bomb.len() < 4 << 20, "{} bytes", bomb.len());
        let file = raw_zip(&[("scene.json", 8, bomb.clone(), 2 << 30, 0)]);
        let t = std::time::Instant::now();
        let e = read(&file).unwrap_err();
        assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
        assert!(e.to_string().contains("more than the"), "{e}");
        // A texture entry that is the bomb: refused by the texture cap the same way.
        let d = sample();
        let mut v = scene_json_of(&d);
        v["textures"] = serde_json::json!([{ "path": "assets/a.png", "mime": "image/png" }]);
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zw.start_file("scene.json", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(&serde_json::to_vec(&v).unwrap()).unwrap();
        let head = zw.finish().unwrap().into_inner();
        let json = {
            let mut z = zip::ZipArchive::new(Cursor::new(head.as_slice())).unwrap();
            entry(&mut z, "scene.json").unwrap().unwrap()
        };
        let file = raw_zip(&[
            (
                "scene.json",
                0,
                json.clone(),
                json.len() as u64,
                crc32fast::hash(&json),
            ),
            ("assets/a.png", 8, bomb, 2 << 30, 0),
        ]);
        let t = std::time::Instant::now();
        let e = read(&file).unwrap_err();
        assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
        assert!(e.to_string().contains("assets/a.png"), "{e}");
    }

    #[test]
    fn an_entry_that_lies_about_its_size_is_refused_one_byte_past_the_lie() {
        // 64 MiB of zeros declared as 1,000 bytes: inflating stops at 1,001.
        let file = raw_zip(&[("scene.json", 8, deflate_zeros(64 << 20), 1000, 0)]);
        let t = std::time::Instant::now();
        let e = read(&file).unwrap_err();
        assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
        assert!(
            e.to_string().contains("not the 1000 its header says"),
            "{e}"
        );
        // And one that claims more than Deflate can do (a 200 MB claim from a few bytes).
        let small = deflate_zeros(1000);
        let file = raw_zip(&[("scene.json", 8, small, 200 << 20, 0)]);
        assert!(
            read(&file)
                .unwrap_err()
                .to_string()
                .contains("more than Deflate can")
        );
    }

    #[test]
    fn duplicate_names_and_too_many_entries_are_refused() {
        let json = b"{}".to_vec();
        let c = crc32fast::hash(&json);
        let file = raw_zip(&[
            ("scene.json", 0, json.clone(), 2, c),
            ("scene.json", 0, json.clone(), 2, c),
        ]);
        assert!(
            read(&file)
                .unwrap_err()
                .to_string()
                .contains("listed twice")
        );
        let names: Vec<String> = (0..MAX_ENTRIES + 1).map(|i| format!("x{i}")).collect();
        let many: Vec<(&str, u16, Vec<u8>, u64, u32)> = names
            .iter()
            .map(|n| (n.as_str(), 0, Vec::new(), 0, 0))
            .collect();
        assert!(
            read(&raw_zip(&many))
                .unwrap_err()
                .to_string()
                .contains("entries")
        );
    }

    #[test]
    fn ten_thousand_texture_rows_are_refused_before_any_image_is_read() {
        let d = sample();
        let mut v = scene_json_of(&d);
        v["textures"] = serde_json::Value::Array(
            (0..10_000)
                .map(|_| serde_json::json!({ "path": "assets/a.png", "mime": "image/png" }))
                .collect(),
        );
        let t = std::time::Instant::now();
        let e = read(&rezip(&v)).unwrap_err();
        assert!(t.elapsed().as_millis() < 2000, "{:?}", t.elapsed());
        // Lane 16b: counted as scene.json inflates, before its rows are parsed.
        assert!(
            e.to_string().contains("more than 512 texture images"),
            "{e}"
        );
        v["textures"] = serde_json::json!([
            { "path": "assets/a.png", "mime": "image/png" },
            { "path": "assets/a.png", "mime": "image/png" }
        ]);
        assert!(
            read(&rezip(&v))
                .unwrap_err()
                .to_string()
                .contains("listed twice")
        );
    }

    /// The lane 16 verifier's F10, from a file: sixteen 4096 x 4096 images of one colour
    /// are a few hundred kilobytes of .shz and a gigabyte decoded. Eight (128 million
    /// pixels) open; one more image is refused before anything is drawn.
    #[test]
    fn texture_images_are_held_to_their_pixels_in_all_when_a_file_opens() {
        let header = |n: u8| {
            let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
            png.extend_from_slice(&4096u32.to_be_bytes());
            png.extend_from_slice(&4096u32.to_be_bytes());
            png.extend_from_slice(&[8, 6, 0, 0, 0, n]);
            png
        };
        let mut d = sample();
        for n in 0..8u8 {
            d.apply(&Op::AddTexture {
                path: format!("assets/t{n}.png"),
                mime: "image/png".into(),
                data: header(n),
            })
            .unwrap();
        }
        let bytes = write(d.model(), &SaveOptions::default()).unwrap();
        assert_eq!(read(&bytes).unwrap().document.hash(), d.hash());
        // The same file with a ninth image listed and present.
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        let mut v: serde_json::Value =
            serde_json::from_slice(&entry(&mut zip, "scene.json").unwrap().unwrap()).unwrap();
        v["textures"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({ "path": "assets/t8.png", "mime": "image/png" }));
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..zip.len() {
            let name = zip.by_index(i).unwrap().name().to_string();
            zw.start_file(name.as_str(), SimpleFileOptions::default())
                .unwrap();
            if name == "scene.json" {
                zw.write_all(&serde_json::to_vec(&v).unwrap()).unwrap();
            } else {
                zw.write_all(&entry(&mut zip, &name).unwrap().unwrap())
                    .unwrap();
            }
        }
        zw.start_file("assets/t8.png", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(&header(8)).unwrap();
        let over = zw.finish().unwrap().into_inner();
        let t = std::time::Instant::now();
        let e = read(&over).unwrap_err();
        assert!(t.elapsed().as_millis() < 2000, "{:?}", t.elapsed());
        assert!(e.to_string().contains("million pixels"), "{e}");
    }

    #[test]
    fn plugin_data_is_written_and_read_by_the_kernel_capped() {
        let d = sample();
        let data = r#"{"version":1,"plugins":{"org.example.a":{"k":1}}}"#.to_string();
        let opts = SaveOptions {
            plugin_data: Some(data.clone()),
            ..SaveOptions::default()
        };
        let o = read(&write(d.model(), &opts).unwrap()).unwrap();
        assert_eq!(o.plugin_data.as_deref(), Some(data.as_str()));
        assert_eq!(o.document.hash(), d.hash(), "not part of the model");
        let big = SaveOptions {
            plugin_data: Some("x".repeat(MAX_PLUGIN_DATA as usize + 1)),
            ..SaveOptions::default()
        };
        assert!(matches!(write(d.model(), &big), Err(ShzError::Write(_))));
        assert!(
            read(&write(d.model(), &SaveOptions::default()).unwrap())
                .unwrap()
                .plugin_data
                .is_none()
        );
    }

    /// A scene.json with `defs` definitions, each holding `copies` instances of the next (the
    /// last a box), and one instance of the first at the top.
    fn chain(defs: usize, copies: usize) -> serde_json::Value {
        let d = structured();
        let mut v = scene_json_of(&d);
        let side = v["definitions"][0]["members"][0].clone();
        let mut out = Vec::new();
        for i in 0..defs {
            let members: Vec<serde_json::Value> = if i + 1 == defs {
                let mut m = side.clone();
                m["slot"] = serde_json::json!(1);
                vec![m]
            } else {
                (0..copies)
                    .map(|k| serde_json::json!({ "slot": k + 1, "kind": "instance", "definition": i + 1 }))
                    .collect()
            };
            out.push(serde_json::json!({ "name": format!("D{i}"), "next_slot": copies + 2, "members": members }));
        }
        v["definitions"] = serde_json::Value::Array(out);
        v["nodes"] = serde_json::json!([{ "id": 1, "kind": "instance", "definition": 0 }]);
        v["objects"] = serde_json::json!([]);
        v["groups"] = serde_json::json!([]);
        v["scenes"] = serde_json::json!([]);
        v["context"] = serde_json::Value::Null;
        v
    }

    #[test]
    fn components_that_would_derive_too_much_are_refused_before_deriving() {
        // 30 definitions each holding two copies of the next: a billion solids from a few KB.
        let t = std::time::Instant::now();
        let e = read(&rezip(&chain(30, 2))).unwrap_err();
        assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
        assert!(e.to_string().contains("solids and meshes"), "{e}");
        // Nested deeper than 64.
        let e = read(&rezip(&chain(70, 1))).unwrap_err();
        assert!(e.to_string().contains("deeper than 64"), "{e}");
        // A definition that contains itself.
        let mut v = chain(2, 1);
        v["definitions"][1]["members"] =
            serde_json::json!([{ "slot": 1, "kind": "instance", "definition": 1 }]);
        let e = read(&rezip(&v)).unwrap_err();
        assert!(e.to_string().contains("contains itself"), "{e}");
    }

    #[test]
    fn coordinates_names_and_transforms_a_reader_refuses() {
        let d = sample();
        let mut v = scene_json_of(&d);
        // A mesh point that is a number but 1e300 metres away (infinite bounds downstream).
        v["meshes"][0]["positions"][0] = serde_json::json!([1e300, 0.0, 0.0]);
        assert!(
            read(&rezip(&v))
                .unwrap_err()
                .to_string()
                .contains("more than 10 km")
        );
        let mut v = scene_json_of(&d);
        v["objects"][0]["name"] = serde_json::json!("n".repeat(65));
        assert!(
            read(&rezip(&v))
                .unwrap_err()
                .to_string()
                .contains("longer than 64")
        );
        let mut v = scene_json_of(&d);
        v["scenes"][0]["eye"] = serde_json::json!([1e12, 0.0, 0.0]);
        assert!(read(&rezip(&v)).unwrap_err().to_string().contains("camera"));
        let mut v = scene_json_of(&d);
        v["guides"][0]["point"] = serde_json::json!([0.0, 1e9, 0.0]);
        assert!(read(&rezip(&v)).is_err());
        // A degenerate placement (a flat matrix) is refused, never derived.
        let d = structured();
        let mut v = scene_json_of(&d);
        v["nodes"][0]["xform"] = serde_json::json!({ "linear": [[0, 0, 0], [0, 0, 0], [0, 0, 0]], "translation": [0, 0, 0] });
        assert!(read(&rezip(&v)).is_err());
    }

    // ---- lane 16b: counts a file declares --------------------------------------------------

    /// `sample()`'s scene.json with `key` holding `n` generated values (`item(i)`, JSON
    /// text), as a list or (`map`) an object, deflated. Written as text: building millions
    /// of serde_json values would take the test seconds.
    fn declaring(key: &str, n: usize, map: bool, item: &dyn Fn(usize) -> String) -> Vec<u8> {
        let mut v = scene_json_of(&sample());
        v[key] = serde_json::json!("__BULK__");
        let text = serde_json::to_string(&v).unwrap();
        let mut bulk = String::with_capacity(n * 48 + 2);
        bulk.push(if map { '{' } else { '[' });
        for i in 0..n {
            if i > 0 {
                bulk.push(',');
            }
            bulk.push_str(&item(i));
        }
        bulk.push(if map { '}' } else { ']' });
        let json = text.replacen("\"__BULK__\"", &bulk, 1);
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zw.start_file("scene.json", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(json.as_bytes()).unwrap();
        zw.finish().unwrap().into_inner()
    }

    fn refused_as_declared(file: &[u8], what: &str, max: usize) {
        let t = std::time::Instant::now();
        let e = read(file).map(|_| ()).unwrap_err();
        let took = t.elapsed();
        match &e {
            ShzError::Limit(sawhorse_kernel::limits::LimitError::Declared { what: w, max: m }) => {
                assert_eq!((*w, *m), (what, max), "{e}");
            }
            _ => panic!("not refused by its declared count: {e}"),
        }
        assert!(
            e.to_string()
                .contains(&format!("the file holds more than {max} {what}")),
            "{e}"
        );
        // Refused at the element past the limit: the rest is never inflated.
        assert!(took.as_millis() < 100, "{what}: {took:?}");
    }

    #[test]
    fn two_million_guides_are_refused_by_their_count_before_any_is_parsed() {
        // The attack: 2,000,000 guide lines, a 5 MB .shz, crashed the tab.
        let guides = |i: usize| {
            format!(
                r#"{{"point":[{},{},0],"dir":[0,1,0]}}"#,
                (i % 1000) as f64 * 1e-3,
                (i / 1000) as f64 * 1e-3
            )
        };
        let file = declaring("guides", 2_000_000, false, &guides);
        assert!(file.len() < 16 << 20, "{} bytes", file.len());
        refused_as_declared(&file, "guides", 50_000);
        // Exactly the limit opens, and holds them all.
        let at = read(&declaring("guides", 50_000, false, &guides)).unwrap();
        assert_eq!(at.document.model().snapshot().guides.len(), 50_000);
        refused_as_declared(
            &declaring("guides", 50_001, false, &guides),
            "guides",
            50_000,
        );
    }

    #[test]
    fn dimensions_tags_materials_and_scenes_are_refused_by_their_counts() {
        let dim = |i: usize| {
            format!(
                r#"{{"a":[{},0,0],"b":[{},1,0],"offset":[0,0,0.1]}}"#,
                i as f64 * 1e-4,
                i as f64 * 1e-4
            )
        };
        refused_as_declared(
            &declaring("dimensions", 2_000_000, false, &dim),
            "dimensions",
            50_000,
        );
        let tag = |i: usize| format!(r#""t{i}":true"#);
        refused_as_declared(&declaring("tags", 1_000_000, true, &tag), "tags", 10_000);
        let colour = |i: usize| format!(r##""t{i}":"#aabbcc""##);
        refused_as_declared(
            &declaring("tag_colors", 1_000_000, true, &colour),
            "tag colours",
            10_000,
        );
        let mat = |i: usize| format!(r#"{{"name":"m{i}","color":[{},2,3,255]}}"#, i % 256);
        refused_as_declared(
            &declaring("materials", 1_000_000, false, &mat),
            "materials",
            4_096,
        );
        let scene = |i: usize| {
            format!(
                r#"{{"name":"s{i}","eye":[1,1,1],"target":[0,0,0],"up":[0,0,1],"fov":45,"parallel":false}}"#
            )
        };
        refused_as_declared(
            &declaring("scenes", 300_000, false, &scene),
            "scenes",
            1_000,
        );
        // One scene hiding 3,000,000 tag names.
        let hiding = |_: usize| {
            let mut s = String::from(
                r#"{"name":"s","eye":[1,1,1],"target":[0,0,0],"up":[0,0,1],"hidden_tags":["#,
            );
            for i in 0..3_000_000 {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&format!("\"t{i}\""));
            }
            s.push_str("]}");
            s
        };
        refused_as_declared(
            &declaring("scenes", 1, false, &hiding),
            "tags hidden by one scene",
            10_000,
        );
        let plane = |i: usize| format!(r#"{{"id":{},"origin":[0,0,0],"normal":[0,0,1]}}"#, i + 1);
        refused_as_declared(
            &declaring("sections", 1_001, false, &plane),
            "section planes",
            1_000,
        );
        let tex = |i: usize| format!(r#"{{"path":"assets/t{i}.png","mime":"image/png"}}"#);
        refused_as_declared(
            &declaring("textures", 513, false, &tex),
            "texture images",
            512,
        );
    }

    #[test]
    fn counted_lists_of_another_type_are_left_to_the_typed_parse() {
        // Not a list: the count pass lets it through and the parse says what is wrong.
        let mut v = scene_json_of(&sample());
        v["guides"] = serde_json::json!(7);
        let e = read(&rezip(&v)).map(|_| ()).unwrap_err();
        assert!(matches!(e, ShzError::Damaged(_)), "{e}");
        // Not JSON at all.
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zw.start_file("scene.json", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(b"[1, 2").unwrap();
        let e = read(&zw.finish().unwrap().into_inner())
            .map(|_| ())
            .unwrap_err();
        assert!(matches!(e, ShzError::Damaged(_)), "{e}");
        // An ordinary file reads as it always did.
        let back = read(&write(sample().model(), &SaveOptions::default()).unwrap()).unwrap();
        assert_eq!(
            back.document.model().snapshot(),
            sample().model().snapshot()
        );
    }

    /// Lane 22: sketch geometry moved, turned, lifted onto a new plane and copied saves and
    /// opens as the same document (hash, the same sketches, the same regions), and the moved
    /// region still pushes into the same solid after opening.
    #[test]
    fn moved_sketches_round_trip() {
        let mut d = Document::with_ground_sketch();
        let x = |t: [f64; 3]| {
            format!(
                r#"{{"linear":[[1,0,0],[0,1,0],[0,0,1]],"translation":[{},{},{}]}}"#,
                t[0], t[1], t[2]
            )
        };
        let turn = r#"{"linear":[[0,-1,0],[1,0,0],[0,0,1]],"translation":[0,0,0]}"#;
        for op in [
            r#"{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":0.6,"h":0.4}"#.to_string(),
            r#"{"op":"add_rectangle","sketch":1,"x":1,"y":0,"w":0.2,"h":0.2}"#.into(),
            format!(
                r#"{{"op":"transform_sketch","sketch":1,"points":[[0.6,0.4,0]],"xform":{}}}"#,
                x([0.1, 0.05, 0.0])
            ),
            format!(r#"{{"op":"transform_sketch","sketch":1,"regions":[1],"xform":{turn}}}"#),
            format!(
                r#"{{"op":"transform_sketch","sketch":1,"regions":[2],"xform":{},"copy":true,"count":2}}"#,
                x([0.3, 0.0, 0.0])
            ),
            format!(
                r#"{{"op":"transform_sketch","sketch":1,"regions":[2],"xform":{}}}"#,
                x([0.0, 0.0, 0.5])
            ),
        ] {
            d.apply(&serde_json::from_str::<Op>(&op).unwrap())
                .unwrap_or_else(|e| panic!("{op}: {e}"));
        }
        let m = d.model();
        assert_eq!(
            m.sketch_ids().len(),
            2,
            "the ground and the lifted square's plane"
        );
        let o = read(&write(m, &SaveOptions::default()).unwrap()).unwrap();
        assert!(o.notes.is_empty(), "{:?}", o.notes);
        assert_eq!(o.document.hash(), d.hash());
        let back = o.document.model();
        for id in m.sketch_ids() {
            let (a, b) = (m.sketch(id).unwrap(), back.sketch(id).unwrap());
            assert_eq!(a.to_raw(), b.to_raw(), "sketch {id}");
        }
        // The turned, stretched region pushes the same after opening.
        let push = |doc: &mut Document| {
            doc.apply(&Op::Extrude {
                sketch: 1,
                region: 1,
                distance: 0.3,
            })
            .unwrap();
            doc.model().objects()[0].1.volume()
        };
        let mut opened = o.document;
        let (v1, v2) = (push(&mut d), push(&mut opened));
        assert_eq!(v1, v2);
        assert!(v1 > 0.6 * 0.4 * 0.3, "the corner moved out: {v1}");
    }

    /// Lane 23: sketch curves (a circle split into arcs by a line, a polygon, an erased and an
    /// exploded curve, so the counter runs ahead of the ids) and a solid's rims come back
    /// from a `.shz` with their ids, kinds, circles and the same hash.
    #[test]
    fn curves_round_trip() {
        let mut d = Document::with_ground_sketch();
        let ring = |c: [f64; 2], r: f64, n: usize| -> String {
            let p: Vec<String> = (0..n)
                .map(|k| {
                    let a = std::f64::consts::TAU * k as f64 / n as f64;
                    format!("[{},{},0]", c[0] + r * a.cos(), c[1] + r * a.sin())
                })
                .collect();
            p.join(",")
        };
        let draw = |pts: String, kind: &str, c: [f64; 2]| {
            format!(
                r#"{{"op":"draw","target":{{"on":"sketch","sketch":1}},"points":[{pts}],"closed":true,"curve":{{"kind":"{kind}","centre":[{},{},0]}}}}"#,
                c[0], c[1]
            )
        };
        for op in [
            draw(ring([0.0, 0.0], 0.3, 24), "circle", [0.0, 0.0]),
            r#"{"op":"draw","target":{"on":"sketch","sketch":1},"points":[[-1,0.1,0],[1,0.1,0]],"closed":false}"#.into(),
            draw(ring([2.0, 0.0], 0.2, 6), "polygon", [2.0, 0.0]),
            draw(ring([4.0, 0.0], 0.1, 12), "circle", [4.0, 0.0]),
            draw(ring([6.0, 0.0], 0.1, 12), "circle", [6.0, 0.0]),
            r#"{"op":"erase_curve","sketch":1,"curve":5}"#.into(),
            r#"{"op":"explode_curve","sketch":1,"curve":4}"#.into(),
        ] {
            d.apply(&serde_json::from_str::<Op>(&op).unwrap())
                .unwrap_or_else(|e| panic!("{op}: {e}"));
        }
        // Push the polygon into a solid with rims.
        let s = d.model().sketch(1).unwrap();
        let region = s
            .regions()
            .find(|(_, r)| {
                let (o, _) = s.region_rings(r);
                o.len() == 6
            })
            .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
            .unwrap();
        d.apply(&Op::Extrude {
            sketch: 1,
            region,
            distance: 0.25,
        })
        .unwrap();
        let m = d.model();
        let kinds: Vec<_> = m
            .sketch(1)
            .unwrap()
            .curves()
            .map(|(id, c)| (id, c.kind))
            .collect();
        assert_eq!(kinds.len(), 2, "two arcs: {kinds:?}");
        assert_eq!(m.objects()[0].1.curves().len(), 2);
        let o = read(&write(m, &SaveOptions::default()).unwrap()).unwrap();
        assert!(o.notes.is_empty(), "{:?}", o.notes);
        assert_eq!(o.document.hash(), d.hash());
        let back = o.document.model();
        assert_eq!(
            m.sketch(1).unwrap().to_raw(),
            back.sketch(1).unwrap().to_raw()
        );
        assert_eq!(
            back.sketch(1).unwrap().next_curve_id(),
            m.sketch(1).unwrap().next_curve_id()
        );
        assert_eq!(back.objects()[0].1.curves(), m.objects()[0].1.curves());
    }
}
