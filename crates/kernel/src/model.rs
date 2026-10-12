//! The geometric state: sketches and objects, changed only through transactional [`Op`]s.
//!
//! `apply` = clone -> mutate -> validate -> commit (ADR-0001). A refused or failed op
//! leaves the model exactly as it was. In debug builds every B-rep and sketch mutation
//! additionally runs the full validator on the spot, so a kernel bug panics at the mutation
//! that caused it.
//!
//! Ops address sketches, regions and objects by **public id** (see [`crate::ids`]) and
//! faces by **persistent face id** within their object (see [`crate::object::Object`]):
//! all stable across edits, so the UI can hold on to them. A face id survives the push
//! that moves the face (topological and boolean paths alike), splits (the first piece
//! keeps it) and coplanar merges (the merged face keeps the smallest member's id).
//! An op sequence is plain data: it serialises to JSON for the fuzzer's repro files, the
//! golden replays and the document's op log, and replays identically everywhere.

use glam::{DVec2, DVec3};
use slotmap::SlotMap;

use std::collections::BTreeMap;

use crate::error::{
    DocumentError, FaceEditError, KernelError, OffsetError, PushError, SketchError, TransformError,
};
use crate::ids::{
    DimensionId, GroupId, GuideId, MeshId, ObjectId, RegionId, SketchId, from_public, to_public,
};
use crate::meshbuild::{ImportItem, LooseMesh, build_item};
use crate::object::{Object, Provenance, PushOutcome};
use crate::offset::OffsetSource;
use crate::plane::Plane;
use crate::sketch::Sketch;
use crate::sweep::Profile;
use crate::tolerance::PLANARITY;
use crate::transform::Xform;
use crate::validate::{TopologyError, Violation};

/// Most copies one array op makes.
pub const MAX_ARRAY: u32 = 1000;

/// One undoable edit. Lengths in metres; ids are public ids.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// New empty sketch on the plane through `origin` with `normal`.
    NewSketch { origin: [f64; 3], normal: [f64; 3] },
    /// Rectangle in the sketch's 2D frame: corner (x, y), size (w, h).
    AddRectangle {
        sketch: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// One straight segment in the sketch's 2D frame, inserted with the sticky rules.
    InsertSegment {
        sketch: u64,
        a: [f64; 2],
        b: [f64; 2],
    },
    /// Erase every edge lying along a segment (the Eraser), healing the T-junctions and
    /// crossings it leaves behind.
    EraseSegment {
        sketch: u64,
        a: [f64; 2],
        b: [f64; 2],
    },
    /// A polyline (closed: the last point joins the first), as one transaction.
    AddPolyline {
        sketch: u64,
        points: Vec<[f64; 2]>,
        closed: bool,
    },
    /// Extrude region `region` of sketch `sketch` into a new solid. The region leaves the
    /// sketch; the solid's base face takes its place.
    Extrude {
        sketch: u64,
        region: u64,
        distance: f64,
    },
    /// Push/pull face `face` (persistent face id) of object `object` along its outward
    /// normal.
    PushPull {
        object: u64,
        face: u64,
        distance: f64,
    },
    /// Push/pull that leaves a seam where the face was (Ctrl in the UI): an outward pull
    /// gives every edge of the face a new side wall instead of stretching the old walls.
    /// Inward pushes behave like [`Op::PushPull`].
    PushPullNewFace {
        object: u64,
        face: u64,
        distance: f64,
    },
    /// Cut face `face` of `object` in two along `path` (world points on the face's plane;
    /// ends on the face's boundary, interior strictly inside).
    SplitFace {
        object: u64,
        face: u64,
        path: Vec<[f64; 3]>,
    },
    /// Imprint the closed loop `points` (strictly inside face `face`) as a coplanar
    /// sub-face twinned with a new hole in the face.
    Imprint {
        object: u64,
        face: u64,
        points: Vec<[f64; 3]>,
    },
    /// Draw a stroke (world points; `closed` joins the last to the first) as one undoable
    /// edit: into a sketch, onto a plane (the sketch already on that plane, or a new one),
    /// or onto a solid's face plane (cuts and imprints; see [`crate::draw`]).
    Draw {
        target: DrawTarget,
        points: Vec<[f64; 3]>,
        closed: bool,
    },
    /// Erase the edge of `object` from `a` to `b` (world points at its two ends). Only a
    /// line drawn on a face can go: its two coplanar faces merge (with every other edge they
    /// share). Anything else would open the solid and is refused.
    EraseEdge {
        object: u64,
        a: [f64; 3],
        b: [f64; 3],
    },
    /// Erase face `face` (persistent id) of `object` into a coplanar neighbour across a drawn
    /// line (an imprint, one side of a cut). Refused where it would open the solid.
    EraseFace { object: u64, face: u64 },
    /// Erase a sketch region: its edges that bound no other region leave the sketch.
    EraseRegion { sketch: u64, region: u64 },
    /// Several ops as one transaction and one undo step (no undo, redo or batch inside).
    Batch { ops: Vec<Op> },
    /// Add a guide: an infinite line through `point` along `dir`, or a guide point when
    /// `dir` is absent (the Tape measure). Guides are construction help: inferable, never
    /// part of a solid.
    AddGuide {
        point: [f64; 3],
        dir: Option<[f64; 3]>,
    },
    /// Remove one guide.
    EraseGuide { guide: u64 },
    /// Remove every guide.
    DeleteGuides,
    /// Add a linear dimension between `a` and `b`, its line drawn `offset` away from them.
    AddDimension {
        a: [f64; 3],
        b: [f64; 3],
        offset: [f64; 3],
    },
    /// Remove one dimension.
    EraseDimension { dimension: u64 },
    /// Move, rotate, scale or mirror whole solids by an affine map (Move, Rotate and Scale
    /// tools). Face ids, drawn edges, tags and display numbers stay; each result must
    /// validate (see [`crate::transform`]). Solids never weld to what they touch.
    Transform { objects: Vec<u64>, xform: Xform },
    /// Copy solids, each copy mapped by `xform` (Ctrl with Move / Rotate). The copies are new
    /// objects (new ids and display numbers) with the originals' face ids, tags and drawn
    /// edges; they join no group.
    Copy { objects: Vec<u64>, xform: Xform },
    /// An array: `count` copies of each solid, copy k mapped by `xform` applied k times
    /// (x3 after a 36 in move: copies at 36, 72 and 108 in; /4 is the same with a quarter
    /// of the move). Copies are made in order k = 1..=count, object by object.
    Array {
        objects: Vec<u64>,
        xform: Xform,
        count: u32,
    },
    /// Move (or scale, or turn) part of a solid: the vertices at `points` (world positions)
    /// by `xform`. A face bent out of its plane is refused unless `autofold` (then it folds
    /// along a triangulation and coplanar pieces merge back).
    TransformGeometry {
        object: u64,
        points: Vec<[f64; 3]>,
        xform: Xform,
        autofold: bool,
    },
    /// Offset a face, a sketch region or a chain of edges by `distance` (positive grows;
    /// for an open chain, positive is to its left). The result is drawn like a stroke: on
    /// the solid when it lies on its faces (imprints, cuts), else into a sketch on the plane
    /// (see [`crate::offset`]).
    Offset { source: OffsetSource, distance: f64 },
    /// Follow Me: sweep a profile along a path of straight segments (world points; `closed`
    /// joins the last to the first). A region profile becomes a new solid; a face profile
    /// adds to its solid (path leaving the face) or cuts it (path running into it). See
    /// [`crate::sweep`].
    FollowMe {
        profile: Profile,
        path: Vec<[f64; 3]>,
        closed: bool,
    },
    /// Delete an object (it also leaves any group).
    DeleteObject { object: u64 },
    /// Group objects under a name.
    CreateGroup { name: String, objects: Vec<u64> },
    /// Dissolve a group (its objects stay).
    DeleteGroup { group: u64 },
    /// Put an object on a tag (layer), or take it off (`None`). New tags start visible.
    SetTag { object: u64, tag: Option<String> },
    /// Show or hide every object on a tag.
    SetTagVisible { tag: String, visible: bool },
    /// Bring in geometry from a file as one undoable step: polygon soups become solids
    /// where they are valid solids and non-solid meshes (with a note) where they are not,
    /// strokes go into sketches, plus materials, scenes and hidden tags. See
    /// [`crate::meshbuild`].
    Import {
        #[serde(default)]
        items: Vec<ImportItem>,
        #[serde(default)]
        strokes: Vec<ImportStroke>,
        #[serde(default)]
        materials: Vec<MaterialDef>,
        #[serde(default)]
        scenes: Vec<SceneDef>,
        #[serde(default)]
        hidden_tags: Vec<String>,
    },
    /// Add or replace a material (a colour, plus the file name of a texture if it had one).
    DefineMaterial {
        name: String,
        color: [u8; 4],
        #[serde(default)]
        texture: Option<String>,
    },
    /// Paint an object with a material, or take its material off (`None`).
    SetMaterial {
        object: u64,
        material: Option<String>,
    },
    /// Name an object, or clear its name (`None`).
    RenameObject { object: u64, name: Option<String> },
    /// Delete an imported non-solid mesh.
    DeleteMesh { mesh: u64 },
    /// Replace the whole model with a saved one (opening a `.shz`). Validated like any
    /// edit; refused (typed) when the snapshot does not validate.
    Restore {
        snapshot: Box<crate::snapshot::Snapshot>,
    },
    /// Step back one op (document command; see [`crate::document::Document`]).
    Undo,
    /// Step forward again after an undo (document command).
    Redo,
}

/// Where a stroke goes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum DrawTarget {
    /// Into this sketch (points must lie on its plane).
    Sketch { sketch: u64 },
    /// Onto this plane: into the sketch already lying on it (either facing), or a new one.
    Plane { origin: [f64; 3], normal: [f64; 3] },
    /// Onto the plane of this face of a solid (persistent face id).
    Face { object: u64, face: u64 },
}

/// A stroke from a file (DXF lines and polylines, SVG paths): world points on one plane.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct ImportStroke {
    pub points: Vec<[f64; 3]>,
    #[serde(default)]
    pub closed: bool,
}

/// A material as an op carries it: an sRGB colour with alpha, and the file name of the
/// texture it had (kept as a reference; textures are not drawn yet).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct MaterialDef {
    pub name: String,
    pub color: [u8; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

/// A saved view (SketchUp "scene"): camera in metres, Z up, and the tags it hides.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SceneDef {
    pub name: String,
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    /// Vertical field of view, degrees.
    #[serde(default = "default_fov")]
    pub fov: f64,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_tags: Vec<String>,
}

fn default_fov() -> f64 {
    35.0
}

/// What an op did, with the ids the UI needs next.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpOutcome {
    SketchCreated {
        sketch: u64,
    },
    /// A sketch edit. `regions`: the regions it drew (inside a rectangle) or touched.
    SketchEdited {
        regions: Vec<u64>,
    },
    ObjectCreated {
        object: u64,
    },
    FaceMoved,
    /// A face was split; the two pieces' persistent ids (the first keeps the old id).
    FaceSplit {
        faces: Vec<u64>,
    },
    /// A loop was imprinted; the new sub-face's persistent id.
    Imprinted {
        face: u64,
    },
    ObjectDeleted,
    GuideAdded {
        guide: u64,
    },
    DimensionAdded {
        dimension: u64,
    },
    AnnotationErased,
    /// A stroke was drawn: into `sketch` (the regions it bounds), or on a solid (the
    /// persistent ids of the faces it cut or made).
    Drawn {
        sketch: Option<u64>,
        regions: Vec<u64>,
        faces: Vec<u64>,
        /// A closed stroke: the region or face it now encloses (what Push/Pull acts on next).
        inside: Option<u64>,
    },
    /// An edge or face was erased; the merged face's persistent id.
    FacesMerged {
        face: u64,
    },
    /// What each op of a batch did, in order.
    Batch {
        outcomes: Vec<OpOutcome>,
    },
    GroupCreated {
        group: u64,
    },
    GroupDeleted,
    TagSet,
    Undone,
    Redone,
    /// Follow Me on a face: the solid grew along the path, or (`cut`) the sweep was cut out
    /// of it; `objects` is the solid, then any pieces it came apart into.
    Swept {
        objects: Vec<u64>,
        cut: bool,
    },
    /// Solids were moved, turned or scaled (the same ids).
    Transformed {
        objects: Vec<u64>,
    },
    /// New solids from a copy or an array, in creation order.
    Copied {
        objects: Vec<u64>,
    },
    /// Part of a solid moved; the persistent ids of the faces that changed (folds included).
    GeometryMoved {
        faces: Vec<u64>,
    },
    /// A push/pull (or a sweep) cut a solid into pieces: each piece is its own solid. The
    /// first id is the original solid (the piece holding its smallest face id).
    Severed {
        objects: Vec<u64>,
    },
    /// A file was imported: the solids, non-solid meshes, groups and sketches it made, and
    /// plain-language notes (what was healed, what stayed a mesh and why, what was skipped).
    Imported {
        objects: Vec<u64>,
        meshes: Vec<u64>,
        groups: Vec<u64>,
        sketches: Vec<u64>,
        notes: Vec<String>,
    },
    MaterialSet,
    Renamed,
    MeshDeleted,
    /// A saved model was restored.
    Restored {
        objects: usize,
        meshes: usize,
        sketches: usize,
    },
    /// The pushed solid flattened to nothing and was removed. When the flat footprint lay
    /// on the solid's source sketch it went back there as a region.
    ObjectCollapsed {
        regions: Vec<u64>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub(crate) sketches: SlotMap<SketchId, Sketch>,
    pub(crate) sketch_order: Vec<SketchId>,
    pub(crate) objects: SlotMap<ObjectId, Object>,
    pub(crate) object_order: Vec<ObjectId>,
    pub(crate) groups: SlotMap<GroupId, Group>,
    /// Tag name -> visible. Every tag an object uses is listed here.
    pub(crate) tags: BTreeMap<String, bool>,
    pub(crate) guides: SlotMap<GuideId, Guide>,
    pub(crate) dimensions: SlotMap<DimensionId, Dimension>,
    /// Imported geometry that is not a valid solid (see [`crate::meshbuild`]).
    pub(crate) meshes: SlotMap<MeshId, LooseMesh>,
    pub(crate) mesh_order: Vec<MeshId>,
    /// Material name -> definition.
    pub(crate) materials: BTreeMap<String, Material>,
    /// Saved views, in order.
    pub(crate) scenes: Vec<SceneDef>,
    /// Last display number handed to an object (see [`Object::number`]).
    pub(crate) last_number: u64,
}

/// A material: sRGB colour with alpha, and a texture file name kept as a reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub color: [u8; 4],
    pub texture: Option<String>,
}

/// A guide line (unit `dir`) or a guide point (`dir` zero).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub point: DVec3,
    pub dir: DVec3,
}

/// A linear dimension: the measured points and where its line is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension {
    pub a: DVec3,
    pub b: DVec3,
    pub offset: DVec3,
}

impl Dimension {
    /// The measured length, metres.
    pub fn length(&self) -> f64 {
        self.a.distance(self.b)
    }
}

fn finite3(p: &[f64; 3]) -> Option<DVec3> {
    let v = DVec3::from_array(*p);
    (v.is_finite() && v.abs().max_element() <= crate::tolerance::MAX_COORD).then_some(v)
}

/// A named set of objects (by public id).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub objects: Vec<u64>,
}

/// A usable name from a file: trimmed, at most 64 characters, `fallback` when empty.
fn clip_name(n: &str, fallback: &str) -> String {
    let t: String = n.trim().chars().take(64).collect();
    let t = t.trim().to_string();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t
    }
}

fn check_name(n: &str) -> Result<(), DocumentError> {
    if n.trim().is_empty() || n.chars().count() > 64 {
        Err(DocumentError::BadName)
    } else {
        Ok(())
    }
}

/// Counts for the UI and tests.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct ModelStats {
    pub sketches: usize,
    pub regions: usize,
    pub objects: usize,
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub holes: usize,
    pub shells: usize,
    /// Every object watertight (vacuously true with no objects).
    pub watertight: bool,
    pub euler: i64,
    pub genus: i64,
    pub volume_m3: f64,
    pub hash: String,
}

impl Model {
    pub fn new() -> Model {
        Model::default()
    }

    /// A model with one sketch on the ground plane (public id 1), as the UI starts.
    pub fn with_ground_sketch() -> Model {
        let mut m = Model::new();
        m.apply(&Op::NewSketch {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        })
        .expect("ground sketch");
        m
    }

    /// Sketches in creation order, with their public ids.
    pub fn sketches(&self) -> Vec<(u64, &Sketch)> {
        self.sketch_order
            .iter()
            .map(|&k| (to_public(k), &self.sketches[k]))
            .collect()
    }

    /// Objects in creation order, with their public ids.
    pub fn objects(&self) -> Vec<(u64, &Object)> {
        self.object_order
            .iter()
            .map(|&k| (to_public(k), &self.objects[k]))
            .collect()
    }

    pub fn sketch(&self, id: u64) -> Option<&Sketch> {
        self.sketches.get(from_public::<SketchId>(id))
    }

    pub fn object(&self, id: u64) -> Option<&Object> {
        self.objects.get(from_public::<ObjectId>(id))
    }

    /// Public ids of the objects, in creation order.
    pub fn object_ids(&self) -> Vec<u64> {
        self.object_order.iter().map(|&k| to_public(k)).collect()
    }

    /// Guides with their public ids, in id order.
    pub fn guides(&self) -> Vec<(u64, &Guide)> {
        self.guides.iter().map(|(k, g)| (to_public(k), g)).collect()
    }

    /// Dimensions with their public ids, in id order.
    pub fn dimensions(&self) -> Vec<(u64, &Dimension)> {
        self.dimensions
            .iter()
            .map(|(k, d)| (to_public(k), d))
            .collect()
    }

    /// Groups with their public ids, in id order.
    pub fn groups(&self) -> Vec<(u64, &Group)> {
        self.groups.iter().map(|(k, g)| (to_public(k), g)).collect()
    }

    /// Tags and their visibility, by name.
    pub fn tags(&self) -> &BTreeMap<String, bool> {
        &self.tags
    }

    /// True when the object is shown (untagged, or on a visible tag).
    pub fn is_visible(&self, object: u64) -> bool {
        self.object(object).is_some_and(|o| {
            o.tag()
                .is_none_or(|t| self.tags.get(t).copied().unwrap_or(true))
        })
    }

    /// Imported non-solid meshes in import order, with their public ids.
    pub fn meshes(&self) -> Vec<(u64, &LooseMesh)> {
        self.mesh_order
            .iter()
            .map(|&k| (to_public(k), &self.meshes[k]))
            .collect()
    }

    pub fn mesh(&self, id: u64) -> Option<&LooseMesh> {
        self.meshes.get(from_public::<MeshId>(id))
    }

    /// True when the mesh is shown (untagged, or on a visible tag).
    pub fn is_mesh_visible(&self, mesh: u64) -> bool {
        self.mesh(mesh).is_some_and(|m| {
            m.tag
                .as_deref()
                .is_none_or(|t| self.tags.get(t).copied().unwrap_or(true))
        })
    }

    /// Materials by name.
    pub fn materials(&self) -> &BTreeMap<String, Material> {
        &self.materials
    }

    /// Saved views, in order.
    pub fn scenes(&self) -> &[SceneDef] {
        &self.scenes
    }

    /// Public ids of the sketches, in creation order.
    pub fn sketch_ids(&self) -> Vec<u64> {
        self.sketch_order.iter().map(|&k| to_public(k)).collect()
    }

    /// Apply an op transactionally.
    pub fn apply(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        let mut next = self.clone();
        let outcome = next.apply_in_place(op)?;
        next.validate()?;
        *self = next;
        Ok(outcome)
    }

    /// Replay a sequence from an empty model, stopping at the first error.
    pub fn replay(ops: &[Op]) -> Result<Model, (usize, KernelError)> {
        let mut m = Model::new();
        for (i, op) in ops.iter().enumerate() {
            m.apply(op).map_err(|e| (i, e))?;
        }
        Ok(m)
    }

    fn sketch_key(&self, id: u64) -> Result<SketchId, SketchError> {
        let k = from_public::<SketchId>(id);
        if self.sketches.contains_key(k) {
            Ok(k)
        } else {
            Err(SketchError::NoSuchSketch(id))
        }
    }

    fn object_key(&self, id: u64) -> Result<ObjectId, PushError> {
        let k = from_public::<ObjectId>(id);
        if self.objects.contains_key(k) {
            Ok(k)
        } else {
            Err(PushError::NoSuchObject(id))
        }
    }

    fn edited(regions: Vec<RegionId>) -> OpOutcome {
        OpOutcome::SketchEdited {
            regions: regions.into_iter().map(to_public).collect(),
        }
    }

    fn apply_in_place(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        match op {
            Op::NewSketch { origin, normal } => {
                let plane = Plane::new(DVec3::from_array(*origin), DVec3::from_array(*normal))
                    .map_err(SketchError::from)?;
                let id = self.sketches.insert(Sketch::new(plane));
                self.sketch_order.push(id);
                Ok(OpOutcome::SketchCreated {
                    sketch: to_public(id),
                })
            }
            Op::AddRectangle { sketch, x, y, w, h } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid].add_rectangle(DVec2::new(*x, *y), DVec2::new(*w, *h))?;
                Ok(Self::edited(r))
            }
            Op::InsertSegment { sketch, a, b } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid]
                    .insert_segment(DVec2::from_array(*a), DVec2::from_array(*b))?;
                Ok(Self::edited(r))
            }
            Op::EraseSegment { sketch, a, b } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid]
                    .erase_segment(DVec2::from_array(*a), DVec2::from_array(*b))?;
                Ok(Self::edited(r))
            }
            Op::AddPolyline {
                sketch,
                points,
                closed,
            } => {
                let sid = self.sketch_key(*sketch)?;
                let pts: Vec<DVec2> = points.iter().map(|p| DVec2::from_array(*p)).collect();
                let r = self.sketches[sid].insert_polyline(&pts, *closed)?;
                Ok(Self::edited(r))
            }
            Op::Extrude {
                sketch,
                region,
                distance,
            } => {
                let sid = self.sketch_key(*sketch)?;
                let rid = from_public::<RegionId>(*region);
                let s = &self.sketches[sid];
                let r = s.region(rid).ok_or(SketchError::NoSuchRegion(*region))?;
                if r.consumed {
                    return Err(SketchError::RegionConsumed(*region).into());
                }
                if r.pinched {
                    return Err(SketchError::PinchedRegion(*region).into());
                }
                let (outer, holes) = s.region_rings(r);
                let mut obj = Object::extrude(s.plane(), &outer, &holes, *distance)?;
                obj.source = Some(Provenance { sketch: sid });
                self.sketches[sid].consume_region(rid)?;
                let id = self.insert_object(obj);
                Ok(OpOutcome::ObjectCreated {
                    object: to_public(id),
                })
            }
            Op::PushPull {
                object,
                face,
                distance,
            }
            | Op::PushPullNewFace {
                object,
                face,
                distance,
            } => {
                let oid = self.object_key(*object)?;
                let o = &mut self.objects[oid];
                let pushed = match op {
                    Op::PushPullNewFace { .. } => o.push_pull_new_face(*face, *distance)?,
                    _ => o.push_pull_id(*face, *distance)?,
                };
                match pushed {
                    PushOutcome::Moved => {
                        let pieces = self.sever(oid);
                        if pieces.len() > 1 {
                            Ok(OpOutcome::Severed {
                                objects: pieces.into_iter().map(to_public).collect(),
                            })
                        } else {
                            Ok(OpOutcome::FaceMoved)
                        }
                    }
                    PushOutcome::Collapsed { footprint } => {
                        let removed = self.objects.remove(oid).expect("object exists");
                        self.object_order.retain(|&k| k != oid);
                        self.forget_object(*object);
                        let regions = match removed.source {
                            Some(src) => self.return_footprint(src.sketch, &footprint)?,
                            None => Vec::new(),
                        };
                        Ok(OpOutcome::ObjectCollapsed {
                            regions: regions.into_iter().map(to_public).collect(),
                        })
                    }
                }
            }
            Op::SplitFace { object, face, path } => {
                let pts: Vec<DVec3> = path.iter().map(|p| DVec3::from_array(*p)).collect();
                let (o, fi) = self.face_edit_target(*object, *face)?;
                let faces = o.split_face(fi, &pts)?;
                Ok(OpOutcome::FaceSplit {
                    faces: faces.to_vec(),
                })
            }
            Op::Imprint {
                object,
                face,
                points,
            } => {
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from_array(*p)).collect();
                let (o, fi) = self.face_edit_target(*object, *face)?;
                let sub = o.imprint(fi, &pts)?;
                Ok(OpOutcome::Imprinted { face: sub })
            }
            Op::Draw {
                target,
                points,
                closed,
            } => {
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from_array(*p)).collect();
                match target {
                    DrawTarget::Face { object, face } => {
                        let (o, fi) = self.face_edit_target(*object, *face)?;
                        let (faces, inside) = o.draw(fi, &pts, *closed)?;
                        Ok(OpOutcome::Drawn {
                            sketch: None,
                            regions: Vec::new(),
                            faces,
                            inside,
                        })
                    }
                    DrawTarget::Sketch { sketch } => {
                        let sid = self.sketch_key(*sketch)?;
                        let regions = self.draw_in_sketch(sid, &pts, *closed)?;
                        Ok(OpOutcome::Drawn {
                            sketch: Some(*sketch),
                            regions: regions.into_iter().map(to_public).collect(),
                            faces: Vec::new(),
                            inside: self.region_inside(sid, &pts, *closed),
                        })
                    }
                    DrawTarget::Plane { origin, normal } => {
                        let plane =
                            Plane::new(DVec3::from_array(*origin), DVec3::from_array(*normal))
                                .map_err(SketchError::from)?;
                        let sid = self.sketch_on(&plane);
                        let regions = self.draw_in_sketch(sid, &pts, *closed)?;
                        Ok(OpOutcome::Drawn {
                            sketch: Some(to_public(sid)),
                            regions: regions.into_iter().map(to_public).collect(),
                            faces: Vec::new(),
                            inside: self.region_inside(sid, &pts, *closed),
                        })
                    }
                }
            }
            Op::EraseEdge { object, a, b } => {
                let o = self
                    .objects
                    .get_mut(from_public::<ObjectId>(*object))
                    .ok_or(FaceEditError::NoSuchObject(*object))?;
                let face = o.erase_edge(DVec3::from_array(*a), DVec3::from_array(*b))?;
                Ok(OpOutcome::FacesMerged { face })
            }
            Op::EraseFace { object, face } => {
                let (o, fi) = self.face_edit_target(*object, *face)?;
                let face = o.erase_face(fi)?;
                Ok(OpOutcome::FacesMerged { face })
            }
            Op::EraseRegion { sketch, region } => {
                let sid = self.sketch_key(*sketch)?;
                let r = self.sketches[sid].erase_region(from_public::<RegionId>(*region))?;
                Ok(Self::edited(r))
            }
            Op::Batch { ops } => {
                if ops.is_empty() {
                    return Err(DocumentError::EmptyBatch.into());
                }
                let mut outcomes = Vec::with_capacity(ops.len());
                for op in ops {
                    if matches!(op, Op::Undo | Op::Redo | Op::Batch { .. }) {
                        return Err(DocumentError::NotInBatch.into());
                    }
                    outcomes.push(self.apply_in_place(op)?);
                }
                Ok(OpOutcome::Batch { outcomes })
            }
            Op::AddGuide { point, dir } => {
                let p = finite3(point).ok_or(DocumentError::BadAnnotation)?;
                let d = match dir {
                    Some(d) => {
                        let v = finite3(d).ok_or(DocumentError::BadAnnotation)?;
                        if v.length() < 1e-12 {
                            return Err(DocumentError::BadAnnotation.into());
                        }
                        v.normalize()
                    }
                    None => DVec3::ZERO,
                };
                let id = self.guides.insert(Guide { point: p, dir: d });
                Ok(OpOutcome::GuideAdded {
                    guide: to_public(id),
                })
            }
            Op::EraseGuide { guide } => {
                self.guides
                    .remove(from_public::<GuideId>(*guide))
                    .ok_or(DocumentError::NoSuchAnnotation(*guide))?;
                Ok(OpOutcome::AnnotationErased)
            }
            Op::DeleteGuides => {
                if self.guides.is_empty() {
                    return Err(DocumentError::NoGuides.into());
                }
                self.guides.clear();
                Ok(OpOutcome::AnnotationErased)
            }
            Op::AddDimension { a, b, offset } => {
                let (a, b, o) = (
                    finite3(a).ok_or(DocumentError::BadAnnotation)?,
                    finite3(b).ok_or(DocumentError::BadAnnotation)?,
                    finite3(offset).ok_or(DocumentError::BadAnnotation)?,
                );
                if a.distance(b) <= crate::tolerance::POINT_MERGE {
                    return Err(DocumentError::BadAnnotation.into());
                }
                let id = self.dimensions.insert(Dimension { a, b, offset: o });
                Ok(OpOutcome::DimensionAdded {
                    dimension: to_public(id),
                })
            }
            Op::EraseDimension { dimension } => {
                self.dimensions
                    .remove(from_public::<DimensionId>(*dimension))
                    .ok_or(DocumentError::NoSuchAnnotation(*dimension))?;
                Ok(OpOutcome::AnnotationErased)
            }
            Op::Transform { objects, xform } => {
                let keys = self.transform_targets(objects)?;
                for &k in &keys {
                    let next = self.objects[k].transformed(xform)?;
                    self.objects[k] = next;
                }
                Ok(OpOutcome::Transformed {
                    objects: keys.into_iter().map(to_public).collect(),
                })
            }
            Op::Copy { objects, xform } => {
                let keys = self.transform_targets(objects)?;
                let made = self.copies(&keys, xform, 1)?;
                Ok(OpOutcome::Copied { objects: made })
            }
            Op::Array {
                objects,
                xform,
                count,
            } => {
                if *count == 0 || *count > MAX_ARRAY {
                    return Err(TransformError::BadCount.into());
                }
                let keys = self.transform_targets(objects)?;
                let made = self.copies(&keys, xform, *count)?;
                Ok(OpOutcome::Copied { objects: made })
            }
            Op::TransformGeometry {
                object,
                points,
                xform,
                autofold,
            } => {
                let k = from_public::<ObjectId>(*object);
                let o = self
                    .objects
                    .get(k)
                    .ok_or(TransformError::NoSuchObject(*object))?;
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from_array(*p)).collect();
                let (next, faces) = o.transform_geometry(&pts, xform, *autofold)?;
                self.objects[k] = next;
                Ok(OpOutcome::GeometryMoved { faces })
            }
            Op::Offset { source, distance } => self.offset(source, *distance),
            Op::FollowMe {
                profile,
                path,
                closed,
            } => self.follow_me(profile, path, *closed),
            Op::DeleteObject { object } => {
                let k = from_public::<ObjectId>(*object);
                if self.objects.remove(k).is_none() {
                    return Err(DocumentError::NoSuchObject(*object).into());
                }
                self.object_order.retain(|&o| o != k);
                self.forget_object(*object);
                Ok(OpOutcome::ObjectDeleted)
            }
            Op::CreateGroup { name, objects } => {
                check_name(name)?;
                if objects.is_empty() {
                    return Err(DocumentError::EmptyGroup.into());
                }
                let mut members: Vec<u64> = Vec::new();
                for &o in objects {
                    if self.object(o).is_none() {
                        return Err(DocumentError::NoSuchObject(o).into());
                    }
                    if !members.contains(&o) {
                        members.push(o);
                    }
                }
                let g = self.groups.insert(Group {
                    name: name.clone(),
                    objects: members,
                });
                Ok(OpOutcome::GroupCreated {
                    group: to_public(g),
                })
            }
            Op::DeleteGroup { group } => {
                if self.groups.remove(from_public::<GroupId>(*group)).is_none() {
                    return Err(DocumentError::NoSuchGroup(*group).into());
                }
                Ok(OpOutcome::GroupDeleted)
            }
            Op::SetTag { object, tag } => {
                if let Some(t) = tag {
                    check_name(t)?;
                }
                let o = self
                    .objects
                    .get_mut(from_public::<ObjectId>(*object))
                    .ok_or(DocumentError::NoSuchObject(*object))?;
                o.tag = tag.clone();
                if let Some(t) = tag {
                    self.tags.entry(t.clone()).or_insert(true);
                }
                Ok(OpOutcome::TagSet)
            }
            Op::SetTagVisible { tag, visible } => {
                check_name(tag)?;
                self.tags.insert(tag.clone(), *visible);
                Ok(OpOutcome::TagSet)
            }
            Op::Import {
                items,
                strokes,
                materials,
                scenes,
                hidden_tags,
            } => self.import(items, strokes, materials, scenes, hidden_tags),
            Op::DefineMaterial {
                name,
                color,
                texture,
            } => {
                check_name(name)?;
                self.materials.insert(
                    name.clone(),
                    Material {
                        color: *color,
                        texture: texture.clone(),
                    },
                );
                Ok(OpOutcome::MaterialSet)
            }
            Op::SetMaterial { object, material } => {
                if let Some(m) = material
                    && !self.materials.contains_key(m)
                {
                    return Err(DocumentError::NoSuchMaterial(m.clone()).into());
                }
                let o = self
                    .objects
                    .get_mut(from_public::<ObjectId>(*object))
                    .ok_or(DocumentError::NoSuchObject(*object))?;
                o.material = material.clone();
                Ok(OpOutcome::MaterialSet)
            }
            Op::RenameObject { object, name } => {
                if let Some(n) = name {
                    check_name(n)?;
                }
                let o = self
                    .objects
                    .get_mut(from_public::<ObjectId>(*object))
                    .ok_or(DocumentError::NoSuchObject(*object))?;
                o.name = name.clone();
                Ok(OpOutcome::Renamed)
            }
            Op::DeleteMesh { mesh } => {
                let k = from_public::<MeshId>(*mesh);
                if self.meshes.remove(k).is_none() {
                    return Err(DocumentError::NoSuchMesh(*mesh).into());
                }
                self.mesh_order.retain(|&m| m != k);
                Ok(OpOutcome::MeshDeleted)
            }
            Op::Restore { snapshot } => {
                *self = Model::from_snapshot(snapshot)?;
                Ok(OpOutcome::Restored {
                    objects: self.object_order.len(),
                    meshes: self.mesh_order.len(),
                    sketches: self.sketch_order.len(),
                })
            }
            Op::Undo | Op::Redo => Err(DocumentError::NotAModelOp.into()),
        }
    }

    /// The loops an offset makes (world points, closed or not) and the plane they are on,
    /// plus where they go: a solid's face on that plane (object key, face ordinal) or a
    /// sketch (when they reach off the solid, or the source is a sketch region).
    pub fn offset_result(
        &self,
        source: &OffsetSource,
        distance: f64,
    ) -> Result<(Vec<Vec<DVec3>>, bool, Plane), KernelError> {
        use crate::offset::{chain_on_plane, check_distance, offset_chain, offset_rings};
        check_distance(distance)?;
        let wind = |rings: Vec<Vec<DVec2>>| -> Vec<Vec<DVec2>> {
            rings
                .into_iter()
                .enumerate()
                .map(|(k, mut r)| {
                    let a = sawhorse_tessellate::signed_area(&r);
                    if (k == 0) == (a < 0.0) {
                        r.reverse();
                    }
                    r
                })
                .collect()
        };
        let (plane, loops, closed) = match source {
            OffsetSource::Face { object, face } => {
                let o = self
                    .object(*object)
                    .ok_or(FaceEditError::NoSuchObject(*object))?;
                let fi = o
                    .face_ordinal(*face)
                    .ok_or(FaceEditError::NoSuchFace(*face))?;
                let p = o.brep().to_polys();
                let plane = p.plane(fi).ok_or(OffsetError::NotPlanar)?;
                let rings: Vec<Vec<DVec2>> = p.faces[fi]
                    .rings()
                    .map(|r| r.iter().map(|&i| plane.to_local(p.points[i])).collect())
                    .collect();
                (plane, offset_rings(&wind(rings), distance), true)
            }
            OffsetSource::Region { sketch, region } => {
                let s = self
                    .sketch(*sketch)
                    .ok_or(SketchError::NoSuchSketch(*sketch))?;
                let r = s
                    .region(from_public::<RegionId>(*region))
                    .ok_or(SketchError::NoSuchRegion(*region))?;
                let (o, h) = s.region_rings(r);
                let rings: Vec<Vec<DVec2>> = std::iter::once(o).chain(h).collect();
                (*s.plane(), offset_rings(&wind(rings), distance), true)
            }
            OffsetSource::Edges {
                points,
                closed,
                normal,
                ..
            } => {
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from_array(*p)).collect();
                if pts.len() < 2 || (*closed && pts.len() < 3) {
                    return Err(OffsetError::TooFewPoints.into());
                }
                let (plane, q) = chain_on_plane(&pts, DVec3::from_array(*normal))?;
                if *closed {
                    (plane, offset_rings(&wind(vec![q]), distance), true)
                } else {
                    (plane, offset_chain(&q, distance), false)
                }
            }
        };
        if loops.is_empty() {
            return Err(OffsetError::Vanishes.into());
        }
        let world = loops
            .iter()
            .map(|r| r.iter().map(|&q| plane.to_world(q)).collect())
            .collect();
        Ok((world, closed, plane))
    }

    /// Apply Follow Me (see [`Op::FollowMe`]).
    fn follow_me(
        &mut self,
        profile: &Profile,
        path: &[[f64; 3]],
        closed: bool,
    ) -> Result<OpOutcome, KernelError> {
        use crate::sweep::{sweep_with_direction, swept_object, wound};
        let path: Vec<DVec3> = path.iter().map(|p| DVec3::from_array(*p)).collect();
        match profile {
            Profile::Region { sketch, region } => {
                let sid = self.sketch_key(*sketch)?;
                let rid = from_public::<RegionId>(*region);
                let s = &self.sketches[sid];
                let r = s.region(rid).ok_or(SketchError::NoSuchRegion(*region))?;
                if r.consumed {
                    return Err(SketchError::RegionConsumed(*region).into());
                }
                if r.pinched {
                    return Err(SketchError::PinchedRegion(*region).into());
                }
                let plane = *s.plane();
                let (o, h) = s.region_rings(r);
                let rings: Vec<Vec<DVec3>> = std::iter::once(o)
                    .chain(h)
                    .map(|ring| ring.iter().map(|&q| plane.to_world(q)).collect())
                    .collect();
                let (p, _) =
                    sweep_with_direction(&wound(&plane, &rings), plane.normal, &path, closed)?;
                let obj = swept_object(&p)?;
                self.sketches[sid].consume_region(rid)?;
                let id = self.insert_object(obj);
                Ok(OpOutcome::ObjectCreated {
                    object: to_public(id),
                })
            }
            Profile::Face { object, face } => {
                let k = from_public::<ObjectId>(*object);
                let o = self
                    .objects
                    .get(k)
                    .ok_or(FaceEditError::NoSuchObject(*object))?;
                let fi = o
                    .face_ordinal(*face)
                    .ok_or(FaceEditError::NoSuchFace(*face))?;
                let p = o.brep().to_polys();
                let plane = p.plane(fi).ok_or(crate::error::SweepError::Degenerate)?;
                let rings: Vec<Vec<DVec3>> = p.faces[fi]
                    .rings()
                    .map(|r| r.iter().map(|&i| p.points[i]).collect())
                    .collect();
                let (sw, leaves) = sweep_with_direction(&rings, plane.normal, &path, closed)?;
                swept_object(&sw)?;
                // Add or cut: where the sweep is just beside the profile. Material on the
                // face's outer side means it grows the solid; on its inner side, it cuts.
                // (A closed path passes through its profile, so its first segment's way
                // says nothing; an open one agrees with the first segment.)
                let outward = {
                    let q2: Vec<Vec<DVec2>> = rings
                        .iter()
                        .map(|r| r.iter().map(|&q| plane.to_local(q)).collect())
                        .collect();
                    let eps = 10.0 * crate::tolerance::POINT_MERGE;
                    match sawhorse_tessellate::interior_point(&q2[0], &q2[1..]) {
                        Some(c) => {
                            let q = plane.to_world(c);
                            let ahead = crate::sever::contains(&sw, q + plane.normal * eps);
                            let behind = crate::sever::contains(&sw, q - plane.normal * eps);
                            if ahead != behind { ahead } else { leaves }
                        }
                        None => leaves,
                    }
                };
                let engine = crate::boolean::default_engine()?;
                let op = if outward {
                    crate::boolean::BoolOp::Union
                } else {
                    crate::boolean::BoolOp::Difference
                };
                let q = crate::boolean::combine(&p, &sw, op, engine.as_ref())?
                    .ok_or(crate::error::SweepError::RemovesEverything)?;
                let next = o
                    .rebuilt(q)
                    .map_err(|e| crate::boolean::BooleanError::InvalidResult(e.to_string()))?;
                next.validate()
                    .map_err(|e| crate::boolean::BooleanError::InvalidResult(e.to_string()))?;
                self.objects[k] = next;
                let pieces = self.sever(k);
                Ok(OpOutcome::Swept {
                    objects: pieces.into_iter().map(to_public).collect(),
                    cut: !outward,
                })
            }
        }
    }

    /// Apply an offset (see [`Op::Offset`]).
    fn offset(&mut self, source: &OffsetSource, distance: f64) -> Result<OpOutcome, KernelError> {
        let (loops, closed, plane) = self.offset_result(source, distance)?;
        // On a solid: the face it came from, or the given solid's face on the chain's plane.
        let solid = match source {
            OffsetSource::Face { object, face } => Some((*object, *face)),
            OffsetSource::Edges {
                object: Some(object),
                ..
            } => self.object(*object).and_then(|o| {
                let p = o.brep().to_polys();
                crate::draw::face_on(&p, &plane)
                    .and_then(|fi| o.face_id_at(fi).map(|id| (*object, id)))
            }),
            _ => None,
        };
        if let Some((object, face)) = solid {
            let o = self
                .object(object)
                .ok_or(FaceEditError::NoSuchObject(object))?;
            let p = o.brep().to_polys();
            let on_solid = loops
                .iter()
                .flatten()
                .all(|&q| crate::draw::covers(&p, &plane, plane.to_local(q)));
            if on_solid {
                let mut faces: Vec<u64> = Vec::new();
                let mut inside = None;
                for (k, ring) in loops.iter().enumerate() {
                    let (o, fi) = self.face_edit_target(object, face)?;
                    let (made, enclosed) = o.draw(fi, ring, closed)?;
                    faces.extend(made);
                    if k == 0 {
                        inside = enclosed;
                    }
                }
                faces.sort_unstable();
                faces.dedup();
                return Ok(OpOutcome::Drawn {
                    sketch: None,
                    regions: Vec::new(),
                    faces,
                    inside,
                });
            }
        }
        // Into a sketch: the region's own, or the sketch on the plane.
        let sid = match source {
            OffsetSource::Region { sketch, .. } => self.sketch_key(*sketch)?,
            _ => self.sketch_on(&plane),
        };
        let mut regions: Vec<RegionId> = Vec::new();
        for ring in &loops {
            regions.extend(self.draw_in_sketch(sid, ring, closed)?);
        }
        regions.sort_unstable();
        regions.dedup();
        let inside = self.region_inside(sid, &loops[0], closed);
        Ok(OpOutcome::Drawn {
            sketch: Some(to_public(sid)),
            regions: regions.into_iter().map(to_public).collect(),
            faces: Vec::new(),
            inside,
        })
    }

    /// The live objects named by `objects` (duplicates dropped, order kept).
    fn transform_targets(&self, objects: &[u64]) -> Result<Vec<ObjectId>, TransformError> {
        if objects.is_empty() {
            return Err(TransformError::NothingSelected);
        }
        let mut keys: Vec<ObjectId> = Vec::with_capacity(objects.len());
        for &o in objects {
            let k = from_public::<ObjectId>(o);
            if !self.objects.contains_key(k) {
                return Err(TransformError::NoSuchObject(o));
            }
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        Ok(keys)
    }

    /// `count` copies of each object, copy k mapped by `xform` k times. Returns the new ids.
    fn copies(
        &mut self,
        keys: &[ObjectId],
        xform: &Xform,
        count: u32,
    ) -> Result<Vec<u64>, TransformError> {
        xform.check()?;
        let mut made = Vec::with_capacity(keys.len() * count as usize);
        for k in 1..=count {
            let x = xform.power(k);
            for &key in keys {
                let mut copy = self.objects[key].transformed(&x)?;
                copy.source = None;
                made.push(to_public(self.insert_object(copy)));
            }
        }
        Ok(made)
    }

    /// Split an object that has come apart into one object per piece (see
    /// [`crate::sever`]). Returns every piece's key, the original first; the new pieces join
    /// the original's groups.
    fn sever(&mut self, oid: ObjectId) -> Vec<ObjectId> {
        let Some(pieces) = self.objects[oid].split_shells() else {
            return vec![oid];
        };
        let mut it = pieces.into_iter();
        let first = it.next().expect("at least two pieces");
        self.objects[oid] = first;
        let mut keys = vec![oid];
        let original = to_public(oid);
        for piece in it {
            let k = self.insert_object(piece);
            let id = to_public(k);
            for g in self.groups.values_mut() {
                if g.objects.contains(&original) {
                    g.objects.push(id);
                }
            }
            keys.push(k);
        }
        keys
    }

    /// Add a new object at the end of the creation order with the next display number.
    pub(crate) fn insert_object(&mut self, mut obj: Object) -> ObjectId {
        self.last_number += 1;
        obj.number = self.last_number;
        let id = self.objects.insert(obj);
        self.object_order.push(id);
        id
    }

    /// [`Op::Import`]: everything or nothing (the caller validates and commits).
    fn import(
        &mut self,
        items: &[ImportItem],
        strokes: &[ImportStroke],
        materials: &[MaterialDef],
        scenes: &[SceneDef],
        hidden_tags: &[String],
    ) -> Result<OpOutcome, KernelError> {
        let mut notes = Vec::new();
        for m in materials {
            self.materials.insert(
                clip_name(&m.name, "Material"),
                Material {
                    color: m.color,
                    texture: m.texture.clone(),
                },
            );
        }
        let (mut objects, mut meshes) = (Vec::new(), Vec::new());
        let mut groups: Vec<(String, Vec<u64>)> = Vec::new();
        for item in items {
            let built = build_item(item)?;
            notes.extend(built.notes);
            let tag = item.tag.as_deref().map(|t| clip_name(t, "Untagged"));
            if let Some(t) = &tag {
                self.tags.entry(t.clone()).or_insert(true);
            }
            let material = match item.material.as_deref().map(|m| clip_name(m, "Material")) {
                Some(m) if self.materials.contains_key(&m) => Some(m),
                Some(m) => {
                    notes.push(format!(
                        "{}: material {m:?} is not defined, left unpainted",
                        item.name
                    ));
                    None
                }
                None => None,
            };
            let base = clip_name(&item.name, "Imported");
            let n = built.solids.len();
            let mut made = Vec::new();
            for (k, mut o) in built.solids.into_iter().enumerate() {
                o.name = Some(if n == 1 {
                    base.clone()
                } else {
                    clip_name(&format!("{base} {}", k + 1), "Imported")
                });
                o.tag = tag.clone();
                o.material = material.clone();
                let id = self.insert_object(o);
                made.push(to_public(id));
            }
            if let Some(mut m) = built.loose {
                m.name = base.clone();
                m.tag = tag.clone();
                m.material = material.clone();
                let id = self.meshes.insert(m);
                self.mesh_order.push(id);
                meshes.push(to_public(id));
            }
            if let Some(g) = &item.group
                && !made.is_empty()
            {
                let g = clip_name(g, "Group");
                match groups.iter_mut().find(|(name, _)| *name == g) {
                    Some((_, list)) => list.extend(&made),
                    None => groups.push((g, made.clone())),
                }
            }
            objects.extend(made);
        }
        let mut group_ids = Vec::new();
        for (name, members) in groups {
            let g = self.groups.insert(Group {
                name,
                objects: members,
            });
            group_ids.push(to_public(g));
        }
        let mut sketches = Vec::new();
        let mut skipped = 0usize;
        for st in strokes {
            let pts: Vec<DVec3> = st.points.iter().map(|p| DVec3::from_array(*p)).collect();
            if pts.len() < 2 || pts.iter().any(|p| !p.is_finite()) {
                skipped += 1;
                continue;
            }
            let plane = if pts.len() == 2 {
                // A single segment lies on many planes: prefer the horizontal one.
                Plane::new(pts[0], DVec3::Z).ok()
            } else {
                Plane::fit(&pts).ok().map(|p| {
                    // Strokes on the ground face up, like drawn ones.
                    if p.normal.z < 0.0 {
                        Plane::new(p.origin, -p.normal).unwrap_or(p)
                    } else {
                        p
                    }
                })
            };
            let Some(plane) = plane.filter(|pl| {
                pts.iter()
                    .all(|p| pl.signed_distance(*p).abs() <= PLANARITY)
            }) else {
                skipped += 1;
                continue;
            };
            let existed = self.sketch_order.len();
            let sid = self.sketch_on(&plane);
            match self.draw_in_sketch(sid, &pts, st.closed && pts.len() > 2) {
                Ok(_) => {
                    let p = to_public(sid);
                    if !sketches.contains(&p) {
                        sketches.push(p);
                    }
                }
                Err(_) => {
                    skipped += 1;
                    if self.sketch_order.len() > existed && self.sketches[sid].edge_count() == 0 {
                        self.sketches.remove(sid);
                        self.sketch_order.retain(|&k| k != sid);
                    }
                }
            }
        }
        if skipped > 0 {
            notes.push(format!(
                "skipped {skipped} stroke{} that were not flat or would not go into a sketch",
                if skipped == 1 { "" } else { "s" }
            ));
        }
        for sc in scenes {
            let mut sc = sc.clone();
            sc.name = clip_name(&sc.name, "Scene");
            let finite = sc
                .eye
                .iter()
                .chain(&sc.target)
                .chain(&sc.up)
                .all(|v| v.is_finite());
            if finite && sc.fov.is_finite() {
                self.scenes.push(sc);
            } else {
                notes.push(format!("scene {:?} has a broken camera, skipped", sc.name));
            }
        }
        for t in hidden_tags {
            self.tags.insert(clip_name(t, "Untagged"), false);
        }
        if objects.is_empty()
            && meshes.is_empty()
            && sketches.is_empty()
            && materials.is_empty()
            && scenes.is_empty()
        {
            return Err(DocumentError::NothingImported.into());
        }
        Ok(OpOutcome::Imported {
            objects,
            meshes,
            groups: group_ids,
            sketches,
            notes,
        })
    }

    /// The sketch lying on `plane` (either facing, within the planarity tolerance), or a
    /// new one on it.
    fn sketch_on(&mut self, plane: &Plane) -> SketchId {
        let found = self.sketch_order.iter().copied().find(|&k| {
            let sp = self.sketches[k].plane();
            sp.normal.dot(plane.normal).abs() >= 1.0 - 1e-12
                && sp.signed_distance(plane.origin).abs() <= PLANARITY
        });
        found.unwrap_or_else(|| {
            let id = self.sketches.insert(Sketch::new(*plane));
            self.sketch_order.push(id);
            id
        })
    }

    /// The region a closed stroke encloses (holding an interior point of it), if any.
    fn region_inside(&self, sid: SketchId, pts: &[DVec3], closed: bool) -> Option<u64> {
        if !closed {
            return None;
        }
        let s = &self.sketches[sid];
        let ring: Vec<DVec2> = pts.iter().map(|&p| s.plane().to_local(p)).collect();
        let q = sawhorse_tessellate::interior_point(&ring, &[])?;
        s.region_at(q).map(to_public)
    }

    /// Insert a world-space stroke into a sketch (every point on its plane).
    fn draw_in_sketch(
        &mut self,
        sid: SketchId,
        pts: &[DVec3],
        closed: bool,
    ) -> Result<Vec<RegionId>, SketchError> {
        let s = &mut self.sketches[sid];
        let plane = *s.plane();
        let mut local = Vec::with_capacity(pts.len());
        for p in pts {
            if !p.is_finite() {
                return Err(SketchError::OutOfBounds);
            }
            if plane.signed_distance(*p).abs() > PLANARITY {
                return Err(SketchError::OffPlane);
            }
            local.push(plane.to_local(*p));
        }
        if local.len() < 2 {
            return Err(SketchError::Degenerate);
        }
        s.insert_polyline(&local, closed)
    }

    /// Drop a removed object from every group (and empty groups with it).
    fn forget_object(&mut self, object: u64) {
        for g in self.groups.values_mut() {
            g.objects.retain(|&o| o != object);
        }
        self.groups.retain(|_, g| !g.objects.is_empty());
    }

    /// The object and the ordinal of its face with persistent id `face`.
    fn face_edit_target(
        &mut self,
        object: u64,
        face: u64,
    ) -> Result<(&mut Object, usize), FaceEditError> {
        let k = from_public::<ObjectId>(object);
        let o = self
            .objects
            .get_mut(k)
            .ok_or(FaceEditError::NoSuchObject(object))?;
        let fi = o
            .face_ordinal(face)
            .ok_or(FaceEditError::NoSuchFace(face))?;
        Ok((o, fi))
    }

    /// A collapsed solid's flat footprint goes back to its source sketch when it lies on
    /// that sketch's plane (the usual case: a box pushed back down onto its base).
    fn return_footprint(
        &mut self,
        sketch: SketchId,
        rings: &[Vec<DVec3>],
    ) -> Result<Vec<RegionId>, SketchError> {
        let Some(s) = self.sketches.get_mut(sketch) else {
            return Ok(Vec::new());
        };
        let plane = *s.plane();
        if rings
            .iter()
            .flatten()
            .any(|p| plane.signed_distance(*p).abs() > PLANARITY)
        {
            return Ok(Vec::new());
        }
        let rings2: Vec<Vec<DVec2>> = rings
            .iter()
            .map(|ring| ring.iter().map(|&p| plane.to_local(p)).collect())
            .collect();
        s.release_footprint(&rings2)
    }

    /// Validate every object (topology + watertight), every sketch graph, and the
    /// document bookkeeping (groups name live objects, every object tag is listed).
    pub fn validate(&self) -> Result<(), TopologyError> {
        let dangling = |what: &'static str| TopologyError {
            violations: vec![Violation::DanglingKey(what)],
            report: Default::default(),
        };
        for g in self.groups.values() {
            if g.objects.is_empty() || g.objects.iter().any(|&o| self.object(o).is_none()) {
                return Err(dangling("group names a missing object"));
            }
        }
        for (_, o) in self.objects() {
            if o.tag().is_some_and(|t| !self.tags.contains_key(t)) {
                return Err(dangling("object tag is not listed"));
            }
            if o.material()
                .is_some_and(|m| !self.materials.contains_key(m))
            {
                return Err(dangling("object material is not defined"));
            }
        }
        for (_, m) in self.meshes() {
            if m.tag.as_deref().is_some_and(|t| !self.tags.contains_key(t))
                || m.material
                    .as_deref()
                    .is_some_and(|x| !self.materials.contains_key(x))
            {
                return Err(dangling("mesh tag or material is not listed"));
            }
            if let Err(e) = m.check() {
                return Err(dangling(e));
            }
        }
        for (_, o) in self.objects() {
            o.validate()?;
        }
        for (_, s) in self.sketches() {
            if let Err(e) = s.check() {
                return Err(TopologyError {
                    violations: vec![Violation::SketchInvalid(e)],
                    report: Default::default(),
                });
            }
        }
        Ok(())
    }

    pub fn hash(&self) -> u64 {
        crate::hash::model_hash(self)
    }

    pub fn hash_hex(&self) -> String {
        format!("{:016x}", self.hash())
    }

    pub fn stats(&self) -> ModelStats {
        let mut s = ModelStats {
            sketches: self.sketch_order.len(),
            regions: self.sketches().iter().map(|(_, s)| s.region_count()).sum(),
            objects: self.object_order.len(),
            watertight: true,
            hash: self.hash_hex(),
            ..Default::default()
        };
        for (_, o) in self.objects() {
            match o.validate() {
                Ok(r) => {
                    s.faces += r.faces;
                    s.edges += r.edges;
                    s.vertices += r.vertices;
                    s.holes += r.holes;
                    s.shells += r.shells.len();
                    s.watertight &= r.watertight;
                    for sh in &r.shells {
                        s.euler += sh.euler;
                        s.genus += sh.genus.unwrap_or(0);
                    }
                }
                Err(_) => s.watertight = false,
            }
            s.volume_m3 += o.volume();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND: u64 = 1;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Op {
        Op::AddRectangle {
            sketch: GROUND,
            x,
            y,
            w,
            h,
        }
    }

    fn first_region(m: &Model) -> u64 {
        to_public(m.sketch(GROUND).unwrap().regions().next().unwrap().0)
    }

    #[test]
    fn spike_box() {
        let mut m = Model::with_ground_sketch();
        let r = m.apply(&rect(0.0, 0.0, 0.6096, 0.4064)).unwrap();
        assert_eq!(r, OpOutcome::SketchEdited { regions: vec![1] });
        assert_eq!(
            m.apply(&Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.3048
            })
            .unwrap(),
            OpOutcome::ObjectCreated { object: 1 }
        );
        let st = m.stats();
        assert!(st.watertight);
        assert_eq!(st.regions, 0, "the extruded region left the sketch");
        assert_eq!(
            (st.faces, st.edges, st.vertices, st.euler, st.genus),
            (6, 12, 8, 2, 0)
        );
        assert!((st.volume_m3 - 0.6096 * 0.4064 * 0.3048).abs() < 1e-12);
    }

    #[test]
    fn refused_ops_roll_back() {
        let mut m = Model::with_ground_sketch();
        m.apply(&rect(0.0, 0.0, 1.0, 1.0)).unwrap();
        let before = m.hash();
        for op in [
            Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.0,
            },
            Op::Extrude {
                sketch: GROUND,
                region: 7,
                distance: 1.0,
            },
            Op::Extrude {
                sketch: 3,
                region: 1,
                distance: 1.0,
            },
            Op::PushPull {
                object: 1,
                face: 0,
                distance: 1.0,
            },
            rect(0.5, 0.5, 0.0, 1.0),
            Op::InsertSegment {
                sketch: GROUND,
                a: [0.0, 0.0],
                b: [f64::NAN, 1.0],
            },
            Op::NewSketch {
                origin: [0.0; 3],
                normal: [0.0; 3],
            },
        ] {
            assert!(m.apply(&op).is_err(), "{op:?} should be refused");
            assert_eq!(m.hash(), before, "{op:?} must not change the model");
        }
    }

    #[test]
    fn extrude_then_push_back_restores_the_sketch() {
        let mut m = Model::with_ground_sketch();
        m.apply(&rect(0.0, 0.0, 1.0, 2.0)).unwrap();
        let sketch_only = m.hash();
        m.apply(&Op::Extrude {
            sketch: GROUND,
            region: 1,
            distance: 0.5,
        })
        .unwrap();
        assert!(
            m.apply(&Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.5
            })
            .is_err(),
            "region already consumed"
        );
        let out = m
            .apply(&Op::PushPull {
                object: 1,
                face: 2,
                distance: -0.5,
            })
            .unwrap();
        assert!(matches!(out, OpOutcome::ObjectCollapsed { ref regions } if regions.len() == 1));
        assert_eq!(m.hash(), sketch_only);
        assert!(m.objects().is_empty());
    }

    #[test]
    fn crossing_rectangles_extrude_one_piece() {
        let mut m = Model::with_ground_sketch();
        m.apply(&rect(0.0, 0.0, 2.0, 2.0)).unwrap();
        let out = m.apply(&rect(1.0, 1.0, 2.0, 2.0)).unwrap();
        let OpOutcome::SketchEdited { regions } = out else {
            panic!("{out:?}")
        };
        assert_eq!(regions.len(), 2);
        assert_eq!(m.stats().regions, 3);
        // Extrude the overlap square (area 1).
        let s = m.sketch(GROUND).unwrap();
        let overlap = s
            .regions()
            .find(|(_, r)| (s.region_area(r) - 1.0).abs() < 1e-12)
            .map(|(id, _)| to_public(id))
            .unwrap();
        m.apply(&Op::Extrude {
            sketch: GROUND,
            region: overlap,
            distance: 1.0,
        })
        .unwrap();
        let st = m.stats();
        assert_eq!(st.regions, 2, "the L-shaped neighbours stay");
        assert!((st.volume_m3 - 1.0).abs() < 1e-12);
        assert!(
            m.sketch(GROUND)
                .unwrap()
                .region(from_public(first_region(&m)))
                .is_some()
        );
    }

    #[test]
    fn display_numbers_are_never_reused() {
        let mut m = Model::with_ground_sketch();
        m.apply(&rect(0.0, 0.0, 1.0, 1.0)).unwrap();
        m.apply(&Op::Extrude {
            sketch: GROUND,
            region: first_region(&m),
            distance: 1.0,
        })
        .unwrap();
        m.apply(&Op::DeleteObject { object: 1 }).unwrap();
        m.apply(&rect(3.0, 0.0, 1.0, 1.0)).unwrap();
        let out = m
            .apply(&Op::Extrude {
                sketch: GROUND,
                region: first_region(&m),
                distance: 1.0,
            })
            .unwrap();
        let OpOutcome::ObjectCreated { object } = out else {
            panic!("{out:?}")
        };
        assert_eq!(
            object,
            (1 << 32) | 1,
            "the slot is reused, the public id says so"
        );
        assert_eq!(
            m.object(object).unwrap().number(),
            2,
            "the display number is not"
        );
        let (lo, hi) = m.object(object).unwrap().bounds();
        assert_eq!(
            (lo, hi),
            (DVec3::new(3.0, 0.0, 0.0), DVec3::new(4.0, 1.0, 1.0))
        );
    }

    #[test]
    fn display_numbers_survive_a_save_and_reopen() {
        let mut m = Model::with_ground_sketch();
        for x in [0.0, 3.0, 6.0] {
            m.apply(&rect(x, 0.0, 1.0, 1.0)).unwrap();
            m.apply(&Op::Extrude {
                sketch: GROUND,
                region: first_region(&m),
                distance: 1.0,
            })
            .unwrap();
        }
        // Delete the last one: its number (3) must not come back after a reopen.
        let last = *m.object_order.last().unwrap();
        m.apply(&Op::DeleteObject {
            object: to_public(last),
        })
        .unwrap();
        let snap = m.snapshot();
        assert_eq!(snap.last_number, 3);
        let mut back = Model::from_snapshot(&snap).unwrap();
        let numbers: Vec<u64> = back
            .object_order
            .iter()
            .map(|&k| back.objects[k].number())
            .collect();
        assert_eq!(numbers, vec![1, 2]);
        back.apply(&rect(9.0, 0.0, 1.0, 1.0)).unwrap();
        let out = back
            .apply(&Op::Extrude {
                sketch: GROUND,
                region: first_region(&back),
                distance: 1.0,
            })
            .unwrap();
        let OpOutcome::ObjectCreated { object } = out else {
            panic!("{out:?}")
        };
        assert_eq!(back.object(object).unwrap().number(), 4);

        // A file without numbers (an older writer) is numbered in saved order.
        let mut bare = m.snapshot();
        for o in &mut bare.objects {
            o.number = None;
        }
        bare.last_number = 0;
        let back = Model::from_snapshot(&bare).unwrap();
        let numbers: Vec<u64> = back
            .object_order
            .iter()
            .map(|&k| back.objects[k].number())
            .collect();
        assert_eq!(numbers, vec![1, 2]);
    }

    #[test]
    fn ops_round_trip_through_json() {
        let ops = vec![
            Op::NewSketch {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
            rect(0.0, 0.0, 1.0, 1.0),
            Op::InsertSegment {
                sketch: GROUND,
                a: [0.5, 0.0],
                b: [0.5, 1.0],
            },
            Op::AddPolyline {
                sketch: GROUND,
                points: vec![[2.0, 0.0], [3.0, 0.0], [2.5, 1.0]],
                closed: true,
            },
            Op::Extrude {
                sketch: GROUND,
                region: 1,
                distance: 0.25,
            },
            Op::PushPull {
                object: 1,
                face: 2,
                distance: 0.1,
            },
        ];
        let json = serde_json::to_string(&ops).unwrap();
        assert!(json.contains(r#""op":"add_rectangle""#));
        assert!(json.contains(r#""op":"insert_segment""#));
        let back: Vec<Op> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ops);
        let m = Model::replay(&ops).unwrap();
        assert_eq!(m.hash(), Model::replay(&back).unwrap().hash());
        assert_eq!(
            m.stats().regions,
            2,
            "one half of the split square and the triangle"
        );
    }
}
