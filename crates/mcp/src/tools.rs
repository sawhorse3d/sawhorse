//! The tools: what each takes, does and answers. `docs/MCP.md` is the guide.
//!
//! The server holds one document at a time, as the app does. Every tool that changes it is
//! one undo step, applied to a copy and kept only when the whole change validates, and it
//! answers with what the change did to the cut list (rows added, removed and changed), so
//! an agent sees the consequence without asking again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sawhorse_kernel::cutlist::{CutList, CutOptions, CutRow};
use sawhorse_kernel::document::Document;
use sawhorse_kernel::model::{Model, Op};
use sawhorse_kernel::nesting::NestOptions;
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::refbatch::{self, NamedBatch};
use sawhorse_shz::SaveOptions;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::paths;

/// The tools, in the order `tools/list` gives them.
pub const NAMES: [&str; 10] = [
    "new_model",
    "open_model",
    "save_model",
    "summary",
    "add_board",
    "apply_ops",
    "cut_list",
    "nest",
    "undo",
    "redo",
];

/// What `initialize` tells the client about using the server.
pub const INSTRUCTIONS: &str = "Sawhorse models furniture as solid parts and reports a cut list and a nesting \
of sheets and boards. Build with add_board (sizes in mm unless units says otherwise), check with summary and \
cut_list, change anything else with apply_ops (the kernel's ops, lengths in metres), and save with save_model. \
Every change is one undo step and returns the cut list rows it added, removed or changed. Parts are named; \
when two share a name, summary lists the name under shared_names.";

/// Largest `.shz` opened, bytes (the reader's own limit on what a file may hold).
const MAX_FILE: u64 = sawhorse_shz::MAX_TOTAL;

/// Longest part name: the kernel's own limit on a name.
const MAX_NAME: usize = 64;

/// Most points one `apply_ops` batch may hold, all its ops together. A stroke longer than a
/// sketch may hold edges is refused by the kernel only after the work of inserting it,
/// which grows faster than its length (32,000 points: about 10 s, then refused), so it is
/// refused here before any work.
pub const MAX_BATCH_POINTS: usize = sawhorse_kernel::limits::MAX_SKETCH_EDGES;

/// The server: the folder it works in and the document it holds.
pub struct Server {
    root: PathBuf,
    doc: Document,
    /// Where the document was opened from or last saved to (relative to the folder).
    path: Option<String>,
    /// The model hash when it was last opened, saved or made new.
    saved: u64,
    /// The unit system saved with the file (the app's own setting: "mm", "frac-in", ...).
    display_units: Option<String>,
}

impl Server {
    /// A server working in `folder` (which must exist), holding a new empty model.
    pub fn new(folder: &Path) -> Result<Server, String> {
        let doc = Document::with_ground_sketch();
        let saved = doc.model().hash();
        Ok(Server {
            root: paths::root(folder)?,
            doc,
            path: None,
            saved,
            display_units: None,
        })
    }

    /// Run one tool. `Err`: the tool failed, and the model is as it was.
    pub fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "new_model" => self.new_model(parse(args)?),
            "open_model" => self.open_model(parse(args)?),
            "save_model" => self.save_model(parse(args)?),
            "summary" => {
                parse::<Empty>(args)?;
                Ok(self.summary())
            }
            "add_board" => self.add_board(parse(args)?),
            "apply_ops" => {
                let batch: NamedBatch = parse(args)?;
                let points: usize = batch.ops.iter().map(points_in).sum();
                if points > MAX_BATCH_POINTS {
                    return Err(format!(
                        "a batch may hold at most {MAX_BATCH_POINTS} points in all (this one holds {points}); split it into smaller batches"
                    ));
                }
                self.change(|doc| {
                    refbatch::apply(doc, &batch)
                        .map(|(out, _)| json!({"outcome": out}))
                        .map_err(|e| e.to_string())
                })
            }
            "cut_list" => {
                let a: CutArgs = parse(args)?;
                let opts = CutOptions {
                    rounding: a.rounding.unwrap_or_default(),
                    visible_only: a.visible_only,
                };
                to_value(&self.doc.model().cut_list(&opts))
            }
            "nest" => {
                let a: NestArgs = parse(args)?;
                let opts = NestOptions {
                    visible_only: a.visible_only,
                    algorithm: a.algorithm,
                };
                to_value(&self.doc.model().nest(&opts))
            }
            "undo" => {
                parse::<Empty>(args)?;
                self.change(|doc| doc.undo().map(|_| json!({})).map_err(|e| e.to_string()))
            }
            "redo" => {
                parse::<Empty>(args)?;
                self.change(|doc| doc.redo().map(|_| json!({})).map_err(|e| e.to_string()))
            }
            other => Err(format!("no tool named {other}")),
        }
    }

    fn hash(&self) -> String {
        hex(self.doc.model().hash())
    }

    /// Make a change, and answer with what it did to the cut list and the new model hash.
    fn change(
        &mut self,
        f: impl FnOnce(&mut Document) -> Result<Value, String>,
    ) -> Result<Value, String> {
        let before = rows(&self.doc.model().cut_list(&CutOptions::default()));
        let mut answer = f(&mut self.doc)?;
        let after = rows(&self.doc.model().cut_list(&CutOptions::default()));
        answer["changes"] = changes(&before, &after);
        answer["model_hash"] = Value::String(self.hash());
        Ok(answer)
    }

    fn new_model(&mut self, args: NewArgs) -> Result<Value, String> {
        let display_units = match args.units.as_deref() {
            None => None,
            Some("mm") => Some("mm".to_string()),
            Some("in") => Some("frac-in".to_string()),
            Some(other) => return Err(format!("units {other}: mm or in")),
        };
        self.doc = Document::with_ground_sketch();
        self.path = None;
        self.saved = self.doc.model().hash();
        self.display_units = display_units;
        Ok(json!({"model_hash": self.hash()}))
    }

    fn open_model(&mut self, args: PathArgs) -> Result<Value, String> {
        let given = args.path.ok_or("open_model needs a path")?;
        let place = paths::place(&self.root, &given)?;
        if !place.exists {
            return Err(format!("{}: there is no such file", place.shown));
        }
        let size = std::fs::metadata(&place.full)
            .map_err(|e| format!("{}: {e}", place.shown))?
            .len();
        if size > MAX_FILE {
            return Err(format!("{}: larger than {MAX_FILE} bytes", place.shown));
        }
        let bytes = std::fs::read(&place.full).map_err(|e| format!("{}: {e}", place.shown))?;
        let opened = sawhorse_shz::read(&bytes).map_err(|e| format!("{}: {e}", place.shown))?;
        self.doc = opened.document;
        self.path = Some(place.shown.clone());
        self.saved = self.doc.model().hash();
        self.display_units = opened.scene.units.display;
        Ok(json!({
            "path": place.shown,
            "file": file_shown(&place.full),
            "model_hash": self.hash(),
            "parts": self.doc.model().cut_list(&CutOptions::default()).rows.len(),
            "notes": opened.notes,
        }))
    }

    fn save_model(&mut self, args: PathArgs) -> Result<Value, String> {
        let given = match args.path {
            Some(p) => p,
            None => self.path.clone().ok_or(
                "this model has not been saved yet: give save_model a path, like \"Bookshelf.shz\"",
            )?,
        };
        let place = paths::place(&self.root, &given)?;
        let model = self.doc.model();
        let bytes = sawhorse_shz::write(
            model,
            &SaveOptions {
                display_units: self.display_units.clone(),
                ..SaveOptions::default()
            },
        )
        .map_err(|e| format!("could not write the model: {e}"))?;
        // Written beside the file, read back and checked, then moved over it: a save that
        // fails leaves the old file as it was.
        // The temporary file is one this save made (never one that was there before), and
        // only that one is removed when the save fails.
        let tmp =
            write_new_beside(&place.full, &bytes).map_err(|e| format!("{}: {e}", place.shown))?;
        if let Err(e) = check_and_move(&tmp, &place.full, model.hash()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("{}: {e}", place.shown));
        }
        self.path = Some(place.shown.clone());
        self.saved = model.hash();
        Ok(
            json!({"path": place.shown, "file": file_shown(&place.full), "model_hash": self.hash(), "bytes": bytes.len()}),
        )
    }

    fn summary(&self) -> Value {
        let model = self.doc.model();
        let list = model.cut_list(&CutOptions::default());
        let parts: Vec<Value> = list.rows.iter().map(row).collect();
        let mut names: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &list.rows {
            *names.entry(r.name.as_str()).or_default() += 1;
        }
        let shared: Vec<Value> = names
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(name, n)| json!({"name": name, "parts": n}))
            .collect();
        json!({
            "path": self.path,
            "unsaved": model.hash() != self.saved,
            "model_hash": self.hash(),
            "display_units": self.display_units,
            "parts": parts,
            "shared_names": shared,
            "materials": model.materials().keys().collect::<Vec<_>>(),
        })
    }

    fn add_board(&mut self, b: BoardArgs) -> Result<Value, String> {
        let name = b.name.trim().to_string();
        if name.is_empty() || name.chars().count() > MAX_NAME {
            return Err(format!(
                "a board needs a name of 1 to {MAX_NAME} characters"
            ));
        }
        let unit = match b.units.as_deref().unwrap_or("mm") {
            "mm" => 0.001,
            "cm" => 0.01,
            "m" => 1.0,
            "in" => 0.0254,
            other => return Err(format!("units {other}: mm, cm, m or in")),
        };
        let size = [b.thickness, b.width, b.length];
        if size.iter().any(|s| !s.is_finite() || *s <= 0.0) {
            return Err("thickness, width and length must be numbers above zero".into());
        }
        let at = b.position.unwrap_or([0.0; 3]);
        if at.iter().any(|p| !p.is_finite()) {
            return Err("position must be three numbers".into());
        }
        let length_axis = b.length_axis.unwrap_or(Axis::X);
        let thickness_axis = b.thickness_axis.unwrap_or(Axis::Z);
        if length_axis == thickness_axis {
            return Err("length_axis and thickness_axis must differ".into());
        }
        let width_axis = Axis::other(length_axis, thickness_axis);
        let [t, w, l] = size.map(|s| s * unit);
        let p = at.map(|c| c * unit);
        let mut ops: Vec<Value> = Vec::new();
        let material = match &b.material {
            None => None,
            Some(m) if self.doc.model().materials().contains_key(m.as_str()) => Some(m.clone()),
            Some(m) => {
                let lib = sawhorse_export::library::find(m).ok_or_else(|| {
                    format!(
                        "no material {m} in the model or the library (for example Pine, Red oak, Walnut, Baltic birch ply, MDF)"
                    )
                })?;
                if self.doc.model().materials().contains_key(lib.name) {
                    Some(lib.name.to_string())
                } else {
                    for op in sawhorse_export::library::ops(lib, None) {
                        ops.push(to_value(&op)?);
                    }
                    Some(lib.name.to_string())
                }
            }
        };
        let corner = |dl: f64, dw: f64| {
            let mut q = p;
            q[length_axis.index()] += dl;
            q[width_axis.index()] += dw;
            q
        };
        let mut normal = [0.0; 3];
        normal[thickness_axis.index()] = 1.0;
        let draw = ops.len();
        ops.push(json!({
            "op": "draw",
            "target": {"on": "plane", "origin": p, "normal": normal},
            "points": [corner(0.0, 0.0), corner(l, 0.0), corner(l, w), corner(0.0, w)],
            "closed": true,
        }));
        let extrude = ops.len();
        ops.push(json!({
            "op": "extrude",
            "sketch": {"$ref": draw, "path": "sketch"},
            "region": {"$ref": draw, "path": "inside"},
            "distance": t,
        }));
        let object = json!({"$ref": extrude, "path": "object"});
        ops.push(json!({"op": "rename_object", "object": object, "name": name}));
        if let Some(m) = &material {
            ops.push(json!({"op": "set_material", "object": object, "material": m}));
        }
        let mut batch = NamedBatch {
            name: format!("Add board {name}"),
            ops,
        };
        // The sketch already on that plane may face the other way, and an extrude goes the
        // way its sketch faces: tried on a copy first, the distance is turned round when
        // the board would come out on the wrong side.
        let k = thickness_axis.index();
        let (lo, _) = trial_bounds(self.doc.model(), &batch, extrude)?;
        if lo[k] < p[k] - t / 2.0 {
            batch.ops[extrude]["distance"] = json!(-t);
        }
        let mut made = 0;
        let mut answer = self.change(|doc| {
            let (out, _) = refbatch::apply(doc, &batch).map_err(|e| e.to_string())?;
            made = outcome_object(&serde_json::to_value(&out).unwrap_or_default(), extrude)?;
            Ok(json!({}))
        })?;
        let model = self.doc.model();
        let (lo, hi) = model
            .object(made)
            .map(|o| o.bounds())
            .ok_or("the board was not made")?;
        answer["board"] = json!({
            "object": made,
            "name": name,
            "material": material,
            "min_mm": lo.to_array().map(mm),
            "max_mm": hi.to_array().map(mm),
        });
        let list = model.cut_list(&CutOptions::default());
        let others: Vec<Value> = list
            .rows
            .iter()
            .filter(|r| r.name == name)
            .filter(|r| !r.occurrences.iter().any(|o| o.item == Item::Object(made)))
            .map(row)
            .collect();
        answer["shares_name_with"] = Value::Array(others);
        Ok(answer)
    }
}

/// The box of the solid a board's batch would make, worked out on a copy of the model.
fn trial_bounds(
    model: &Model,
    batch: &NamedBatch,
    extrude: usize,
) -> Result<([f64; 3], [f64; 3]), String> {
    let (ops, outcomes) = refbatch::resolve(model, batch).map_err(|e| e.to_string())?;
    let (next, _) = model
        .applied(&Op::Batch { ops })
        .map_err(|e| e.to_string())?;
    let out = to_value(&outcomes[extrude])?;
    let object = out["object"].as_u64().ok_or("the board was not made")?;
    let (lo, hi) = next
        .object(object)
        .ok_or("the board was not made")?
        .bounds();
    Ok((lo.to_array(), hi.to_array()))
}

/// The object an op of a batch made, from the batch's outcome.
fn outcome_object(out: &Value, index: usize) -> Result<u64, String> {
    out["outcomes"][index]["object"]
        .as_u64()
        .ok_or_else(|| "the board was not made".to_string())
}

/// What an agent sees of a cut list row.
fn row(r: &CutRow) -> Value {
    json!({
        "name": r.name,
        "qty": r.qty,
        "size_mm": r.size.map(mm),
        "material": r.material,
        "grain": r.grain,
        "target": r.target,
        "items": r.occurrences.iter().map(|o| &o.item).collect::<Vec<_>>(),
    })
}

/// The rows of a cut list by part, each as [`row`] gives it.
fn rows(list: &CutList) -> BTreeMap<String, Vec<Value>> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in &list.rows {
        let key = serde_json::to_string(&r.key).unwrap_or_default();
        out.entry(key).or_default().push(row(r));
    }
    out
}

/// Rows added, removed and changed between two cut lists. A part that keeps one row is
/// changed; one whose rows split or merge is its old rows removed and its new ones added.
fn changes(before: &BTreeMap<String, Vec<Value>>, after: &BTreeMap<String, Vec<Value>>) -> Value {
    let (mut added, mut removed, mut changed) = (Vec::new(), Vec::new(), Vec::new());
    let none = Vec::new();
    let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for k in keys {
        let b = before.get(k).unwrap_or(&none);
        let a = after.get(k).unwrap_or(&none);
        if a == b {
            continue;
        }
        if let ([b], [a]) = (b.as_slice(), a.as_slice()) {
            changed.push(json!({"before": b, "after": a}));
        } else {
            removed.extend(b.iter().cloned());
            added.extend(a.iter().cloned());
        }
    }
    json!({"added": added, "removed": removed, "changed": changed})
}

fn hex(h: u64) -> String {
    format!("{h:016x}")
}

/// Metres as millimetres to the nearest 0.1 mm, for answers (finer is not a size in wood,
/// and the conversion leaves noise like 19.000000000000018). `-0.0` comes out as `0.0`.
fn mm(m: f64) -> f64 {
    (m * 10_000.0).round() / 10.0 + 0.0
}

/// Where a file is on this machine, as the server sees it.
fn file_shown(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("arguments: {e}"))
}

/// Points (arrays of two or three numbers) anywhere in an op.
fn points_in(v: &Value) -> usize {
    match v {
        Value::Array(a) if (2..=3).contains(&a.len()) && a.iter().all(Value::is_number) => 1,
        Value::Array(a) => a.iter().map(points_in).sum(),
        Value::Object(o) => o.values().map(points_in).sum(),
        _ => 0,
    }
}

/// Read the file written at `tmp` back, check it is the model (`hash`), and move it over
/// `target`.
fn check_and_move(tmp: &Path, target: &Path, hash: u64) -> Result<(), String> {
    let back = std::fs::read(tmp).map_err(|e| format!("reading it back: {e}"))?;
    let opened = sawhorse_shz::read(&back).map_err(|e| format!("reading it back: {e}"))?;
    if opened.document.model().hash() != hash {
        return Err("the file read back is not the model (nothing was saved)".to_string());
    }
    std::fs::rename(tmp, target).map_err(|e| e.to_string())
}

/// Write `bytes` to a new file beside `target`, under a name no file has, and say which.
/// A file already there is never written over or removed.
fn write_new_beside(target: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    use std::io::Write;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    for n in 0..100 {
        let tmp = target.with_file_name(format!(
            ".{name}.{}-{n}.sawhorse-mcp-tmp",
            std::process::id()
        ));
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        };
        let done = f.write_all(bytes).and_then(|()| f.sync_all());
        if let Err(e) = done {
            drop(f);
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        return Ok(tmp);
    }
    Err("no free name for the temporary file beside it".into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CutArgs {
    #[serde(default)]
    rounding: Option<sawhorse_kernel::cutlist::Rounding>,
    #[serde(default)]
    visible_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NestArgs {
    #[serde(default)]
    algorithm: Option<sawhorse_kernel::stock::NestAlgorithm>,
    #[serde(default)]
    visible_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewArgs {
    #[serde(default)]
    units: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    /// The axis that is neither.
    fn other(a: Axis, b: Axis) -> Axis {
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .find(|c| *c != a && *c != b)
            .unwrap_or(Axis::Y)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardArgs {
    name: String,
    thickness: f64,
    width: f64,
    length: f64,
    #[serde(default)]
    position: Option<[f64; 3]>,
    #[serde(default)]
    length_axis: Option<Axis>,
    #[serde(default)]
    thickness_axis: Option<Axis>,
    #[serde(default)]
    units: Option<String>,
    #[serde(default)]
    material: Option<String>,
}

/// `tools/list`: each tool with its description and the JSON Schema of its arguments.
pub fn list() -> Value {
    let axis = json!({"type": "string", "enum": ["x", "y", "z"]});
    let none = json!({"type": "object", "properties": {}, "additionalProperties": false});
    let path = |what: &str| {
        json!({"type": "object", "additionalProperties": false, "properties": {
            "path": {"type": "string", "description": what}}})
    };
    json!([
        {"name": "new_model", "title": "New model",
         "description": "Start a new empty model, dropping the one held (save it first). units sets the unit system saved with the file: mm or in.",
         "inputSchema": {"type": "object", "additionalProperties": false, "properties": {
             "units": {"type": "string", "enum": ["mm", "in"]}}}},
        {"name": "open_model", "title": "Open model",
         "description": "Open a .shz file under the server's folder (a relative path, no \"..\"), replacing the model held. Answers its model hash, its part count and what the reader had to repair.",
         "inputSchema": {"type": "object", "additionalProperties": false, "required": ["path"], "properties": {
             "path": {"type": "string", "description": "Relative to the server's folder, ending .shz"}}}},
        {"name": "save_model", "title": "Save model",
         "description": "Save the model as a .shz under the server's folder, the file the app writes. The file is read back and checked before the save is reported. Without a path it saves where the model was opened from or last saved.",
         "inputSchema": path("Relative to the server's folder, ending .shz; its folder must exist")},
        {"name": "summary", "title": "Summary",
         "description": "Every part: name, quantity, size in mm (thickness, width, length), material, grain, and the items (ids) apply_ops can name. Also the file path, whether there are unsaved changes, the model hash, the materials, and shared_names: names more than one part has.",
         "inputSchema": none},
        {"name": "add_board", "title": "Add board",
         "description": "Add one solid board: a name, thickness x width x length, the corner nearest the origin (position), which axis its length runs along (default x) and which its thickness (default z, lying flat). Sizes and position in mm unless units says otherwise. material is one in the model or the library (Pine, Red oak, Walnut, Baltic birch ply, MDF, ...). One undo step. Answers the board's object id and box, the cut list changes, and any other parts with the same name.",
         "inputSchema": {"type": "object", "additionalProperties": false,
             "required": ["name", "thickness", "width", "length"], "properties": {
             "name": {"type": "string"},
             "thickness": {"type": "number", "exclusiveMinimum": 0},
             "width": {"type": "number", "exclusiveMinimum": 0},
             "length": {"type": "number", "exclusiveMinimum": 0},
             "position": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
             "length_axis": axis.clone(),
             "thickness_axis": axis,
             "units": {"type": "string", "enum": ["mm", "cm", "m", "in"]},
             "material": {"type": "string"}}}},
        {"name": "apply_ops", "title": "Apply ops",
         "description": "Apply kernel ops as one undo step, all or nothing: the same named batch a plugin sends (docs/PLUGINS.md), lengths in metres, with {\"$ref\": i, \"path\": ...} placeholders for what an earlier op of the batch made. To repeat parts, copy them (op copy: objects, xform) or array them (op array: objects, xform, count) by the ids summary gives, rather than adding each again. Undo, redo, batches and restores are not allowed. Answers each op's outcome and the cut list changes; a refusal says which op and why, and changes nothing.",
         "inputSchema": {"type": "object", "additionalProperties": false, "required": ["name", "ops"], "properties": {
             "name": {"type": "string", "description": "Names the undo step"},
             "ops": {"type": "array", "items": {"type": "object"}}}}},
        {"name": "cut_list", "title": "Cut list",
         "description": "The cut list as the app's Cut list pane reads it (the same JSON as sawhorse-cli cutlist): rows with finished sizes in metres, quantities, grain, stock, board feet or area, totals and a hash.",
         "inputSchema": {"type": "object", "additionalProperties": false, "properties": {
             "rounding": {"type": "string", "enum": ["sixteenth", "thirty_second", "half_mm"]},
             "visible_only": {"type": "boolean"}}}},
        {"name": "nest", "title": "Nest",
         "description": "The nesting as the app's Nesting section reads it (the same JSON as sawhorse-cli nest): every sheet and board with its placements and cuts, glue-ups, parts no stock takes, and totals.",
         "inputSchema": {"type": "object", "additionalProperties": false, "properties": {
             "algorithm": {"type": "string", "enum": ["guillotine", "maxrects"]},
             "visible_only": {"type": "boolean"}}}},
        {"name": "undo", "title": "Undo",
         "description": "Undo the last change. Answers the cut list changes.",
         "inputSchema": none.clone()},
        {"name": "redo", "title": "Redo",
         "description": "Redo the last change undone. Answers the cut list changes.",
         "inputSchema": none},
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The read-back check: a file that does not read back as the model is never moved
    /// over the target, and one that does is.
    #[test]
    fn a_file_that_does_not_read_back_as_the_model_is_never_moved_over_the_target() {
        let dir = std::env::temp_dir()
            .join("sawhorse-mcp-tests")
            .join(format!("readback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let doc = Document::with_ground_sketch();
        let bytes = sawhorse_shz::write(doc.model(), &SaveOptions::default()).unwrap();
        let (tmp, target) = (dir.join("t.tmp"), dir.join("t.shz"));
        std::fs::write(&target, b"the old file").unwrap();
        // The right bytes under the wrong hash: refused, the target and the file as they were.
        std::fs::write(&tmp, &bytes).unwrap();
        let wrong = doc.model().hash() ^ 1;
        let e = check_and_move(&tmp, &target, wrong).unwrap_err();
        assert!(e.contains("not the model"), "{e}");
        assert_eq!(std::fs::read(&target).unwrap(), b"the old file");
        assert!(tmp.exists());
        // Bytes that are not a model at all: refused the same way.
        std::fs::write(&tmp, b"not a shz").unwrap();
        assert!(check_and_move(&tmp, &target, doc.model().hash()).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"the old file");
        // The right bytes under the right hash: moved.
        std::fs::write(&tmp, &bytes).unwrap();
        check_and_move(&tmp, &target, doc.model().hash()).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert!(!tmp.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
