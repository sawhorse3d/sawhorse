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
    DefId, DimensionId, GuideId, MeshId, NodeId, ObjectId, RegionId, SketchId, from_public,
    to_public,
};
use crate::meshbuild::{ImportItem, LooseMesh, build_item};
use crate::nodes::{Definition, Dirty, Item, Node, NodeKind, Thing};
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
    NewSketch {
        origin: [f64; 3],
        normal: [f64; 3],
    },
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
    EraseFace {
        object: u64,
        face: u64,
    },
    /// Erase a sketch region: its edges that bound no other region leave the sketch.
    EraseRegion {
        sketch: u64,
        region: u64,
    },
    /// Several ops as one transaction and one undo step (no undo, redo or batch inside).
    Batch {
        ops: Vec<Op>,
    },
    /// Add a guide: an infinite line through `point` along `dir`, or a guide point when
    /// `dir` is absent (the Tape measure). Guides are construction help: inferable, never
    /// part of a solid.
    AddGuide {
        point: [f64; 3],
        dir: Option<[f64; 3]>,
    },
    /// Remove one guide.
    EraseGuide {
        guide: u64,
    },
    /// Remove every guide.
    DeleteGuides,
    /// Add a linear dimension between `a` and `b`, its line drawn `offset` away from them.
    AddDimension {
        a: [f64; 3],
        b: [f64; 3],
        offset: [f64; 3],
    },
    /// Remove one dimension.
    EraseDimension {
        dimension: u64,
    },
    /// Move, rotate, scale or mirror whole solids, groups and component instances by an
    /// affine map (Move, Rotate and Scale tools). Face ids, drawn edges, tags and display
    /// numbers stay; each result must validate (see [`crate::transform`]). Solids never weld
    /// to what they touch. A group moves with everything in it; an instance's placement
    /// changes and its solids are re-derived from its definition.
    Transform {
        objects: Vec<u64>,
        xform: Xform,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        nodes: Vec<u64>,
    },
    /// Copy solids, groups and instances, each copy mapped by `xform` (Ctrl with Move /
    /// Rotate). Solid copies are new plain solids (new ids and display numbers) with the
    /// originals' face ids, tags and drawn edges, in the open context. A group's copy is a
    /// new group with copies of everything in it; an instance's copy is a new instance of
    /// the same definition.
    Copy {
        objects: Vec<u64>,
        xform: Xform,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        nodes: Vec<u64>,
    },
    /// An array: `count` copies of each solid (and node), copy k mapped by `xform` applied k
    /// times (x3 after a 36 in move: copies at 36, 72 and 108 in; /4 is the same with a
    /// quarter of the move). Copies are made in order k = 1..=count, solids then nodes.
    Array {
        objects: Vec<u64>,
        xform: Xform,
        count: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        nodes: Vec<u64>,
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
    Offset {
        source: OffsetSource,
        distance: f64,
    },
    /// Follow Me: sweep a profile along a path of straight segments (world points; `closed`
    /// joins the last to the first). A region profile becomes a new solid; a face profile
    /// adds to its solid (path leaving the face) or cuts it (path running into it). See
    /// [`crate::sweep`].
    FollowMe {
        profile: Profile,
        path: Vec<[f64; 3]>,
        closed: bool,
    },
    /// Delete an object (in the open context).
    DeleteObject {
        object: u64,
    },
    /// Group solids under a name (lane 2's form of [`Op::MakeGroup`]).
    CreateGroup {
        name: String,
        objects: Vec<u64>,
    },
    /// Dissolve a group, keeping what is in it (lane 2's form of [`Op::Explode`]).
    DeleteGroup {
        group: u64,
    },
    /// Open a group or component instance for editing (`None`: the whole model). Not an
    /// undo step: Ctrl+Z still undoes the last edit (and returns to where it was made).
    SetContext {
        node: Option<u64>,
    },
    /// Close the open group or instance (one level up). Not an undo step.
    LeaveContext,
    /// Make Group: solids, groups and instances directly in the open context go into a new
    /// group there.
    MakeGroup {
        items: Vec<Item>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Make Component: the items become a new definition named `name` with one instance
    /// where they were (nothing moves).
    MakeComponent {
        items: Vec<Item>,
        name: String,
    },
    /// Give one instance its own copy of its definition.
    MakeUnique {
        node: u64,
    },
    /// Explode a group or instance: what is inside moves up a level, unchanged.
    Explode {
        node: u64,
    },
    /// Delete a group or instance and everything in it.
    DeleteNode {
        node: u64,
    },
    /// Name a solid, group or instance (`None` clears it).
    Rename {
        item: Item,
        name: Option<String>,
    },
    /// Rename a component definition (every instance shows the new name).
    RenameDefinition {
        definition: u64,
        name: String,
    },
    /// Hide or show solids, groups and instances (the Outliner's eye).
    SetHidden {
        items: Vec<Item>,
        hidden: bool,
    },
    /// Lock or unlock solids, groups and instances.
    SetLocked {
        items: Vec<Item>,
        locked: bool,
    },
    /// Put solids, groups and instances on a tag (`None` or "Untagged": no tag). Tags
    /// control visibility only; nothing moves.
    TagItems {
        items: Vec<Item>,
        tag: Option<String>,
    },
    /// Create a tag (visible), with an sRGB colour for the Tags pane.
    CreateTag {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<[u8; 3]>,
    },
    /// Rename a tag everywhere it is used (items, definitions, scenes).
    RenameTag {
        tag: String,
        name: String,
    },
    SetTagColor {
        tag: String,
        color: Option<[u8; 3]>,
    },
    /// Delete a tag: whatever was on it becomes untagged and stays where it is.
    DeleteTag {
        tag: String,
    },
    /// Save a scene: camera, hidden tags and the open context.
    AddScene {
        scene: SceneDef,
    },
    /// Replace the scene named `name` (also renames it).
    UpdateScene {
        name: String,
        scene: SceneDef,
    },
    DeleteScene {
        name: String,
    },
    /// Switch to a scene's tag visibility and editing context (the UI sets its camera).
    ActivateScene {
        name: String,
    },
    /// Put an object on a tag (layer), or take it off (`None`). New tags start visible.
    SetTag {
        object: u64,
        tag: Option<String>,
    },
    /// Show or hide every object on a tag.
    SetTagVisible {
        tag: String,
        visible: bool,
    },
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
        /// Component definitions the import places (a `.skp`'s components), in dependency
        /// order: a definition only places definitions listed before it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        definitions: Vec<ImportDefinition>,
        /// Groups and component instances, top level (put in the open context).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        nodes: Vec<ImportNode>,
    },
    /// Add or replace a material: a colour, a texture (a path of an image the model holds,
    /// or the file name of one it does not, kept as a reference), the texture's real-world
    /// size, rotation and offset, and what it is as stock (lane 10).
    DefineMaterial {
        name: String,
        color: [u8; 4],
        #[serde(default)]
        texture: Option<String>,
        /// Texture width and height, metres (absent: one foot across, at the image's aspect).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<[f64; 2]>,
        /// Texture rotation, degrees.
        #[serde(default, skip_serializing_if = "is_zero_f")]
        rotation: f64,
        /// Texture offset along the face, metres.
        #[serde(default, skip_serializing_if = "is_zero2")]
        offset: [f64; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stock: Option<crate::looks::StockLink>,
    },
    /// Rename a material everywhere it is used.
    RenameMaterial {
        name: String,
        to: String,
    },
    /// Delete a material: what was painted with it shows the default (or what is around it).
    DeleteMaterial {
        name: String,
    },
    /// Put a texture image into the model (PNG or JPEG bytes, base64 in JSON) under
    /// `assets/<name>.png|.jpg`. The same path again replaces the image.
    AddTexture {
        path: String,
        mime: String,
        #[serde(with = "crate::looks::b64::serde_bytes")]
        data: Vec<u8>,
    },
    /// Paint faces of a solid, or solids, groups and instances (their default), with a
    /// material (`None`: take it off). Faces with a material of their own keep it when their
    /// container is painted.
    Paint {
        target: crate::looks::PaintTarget,
        material: Option<String>,
    },
    /// How the texture sits on one face (`None`: the material's own placement).
    SetFaceUv {
        object: u64,
        face: u64,
        uv: Option<crate::looks::UvXform>,
    },
    /// The document's drawing style (`None`: the default).
    SetStyle {
        style: Option<crate::looks::Style>,
    },
    /// The document's sun and shadows (`None`: the default, off).
    SetShadows {
        shadows: Option<crate::looks::Shadows>,
    },
    /// Paint an object with a material, or take its material off (`None`).
    SetMaterial {
        object: u64,
        material: Option<String>,
    },
    /// Name an object, or clear its name (`None`).
    RenameObject {
        object: u64,
        name: Option<String>,
    },
    /// Delete an imported non-solid mesh.
    DeleteMesh {
        mesh: u64,
    },
    /// Cut list (lane 7): which way a part's grain runs. `Length` is the default and clears
    /// the override. A component instance names its definition. Metadata: allowed anywhere
    /// in the tree, like a name, and it reaches every copy of a component.
    SetGrain {
        part: crate::cutlist::PartRef,
        grain: crate::cutlist::Grain,
    },
    /// Cut list: the stock a part is cut from, in words (`None`: its material's name), and
    /// whether it is sheet goods or solid lumber (`None`: decided from the name and the
    /// thickness).
    SetStock {
        part: crate::cutlist::PartRef,
        #[serde(default)]
        stock: Option<String>,
        #[serde(default)]
        sheet: Option<bool>,
    },
    /// Cut list (lane 15): count a part, or drop it from the totals, the outputs' counts and
    /// the nesting (it stays listed, marked). Metadata, like grain.
    SetInclude {
        part: crate::cutlist::PartRef,
        include: bool,
    },
    /// Lane 15: put each edge-connected piece of non-solid meshes into a group of its own, so
    /// the cut list counts it as a part (loose faces are never parts). `meshes` empty: every
    /// mesh in the open context. One undo step; the geometry does not move or change.
    GroupLoose {
        #[serde(default)]
        meshes: Vec<u64>,
    },
    /// Nesting (lane 9): add a stock to the document's library, or replace the stock named
    /// `replace` with it (a rename carries the per-material defaults along).
    DefineStock {
        stock: crate::stock::StockDef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replace: Option<String>,
    },
    /// Nesting: remove a stock from the library (materials that defaulted to it go back to
    /// the automatic choice).
    DeleteStock {
        name: String,
    },
    /// Nesting: which stock a material's parts come from (`None`: chosen automatically).
    SetMaterialStock {
        material: String,
        #[serde(default)]
        stock: Option<String>,
    },
    /// Nesting: the saw's kerf, the trim per edge and the algorithm.
    SetNestSettings {
        settings: crate::stock::NestSettings,
    },
    /// Nesting: back to the built-in stock library and settings.
    ResetStockLibrary,
    /// Section planes (lane 12): a new plane in the open context through `origin`, keeping
    /// the side `normal` points to; active by default (the other planes of its context are
    /// then deactivated).
    AddSection {
        origin: [f64; 3],
        normal: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        active: bool,
        /// Placed hidden (an import's hidden planes).
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    /// Put a section plane exactly here (it must be in the open context).
    PlaceSection {
        section: u64,
        origin: [f64; 3],
        normal: [f64; 3],
    },
    /// Keep the other side (the plane must be in the open context).
    FlipSection {
        section: u64,
    },
    /// Activate a section plane (the others of its context are deactivated), or deactivate
    /// it. Allowed anywhere, like the Outliner's flags.
    SetSectionActive {
        section: u64,
        active: bool,
    },
    RenameSection {
        section: u64,
        name: String,
    },
    /// Hide or show a section plane (an active hidden plane still cuts).
    SetSectionHidden {
        section: u64,
        hidden: bool,
    },
    /// Delete a section plane (in the open context).
    DeleteSection {
        section: u64,
    },
    /// Move, turn or mirror section planes (Move / Rotate / Scale; in the open context). A
    /// selection of solids and planes moves as one batch of [`Op::Transform`] and this.
    MoveSections {
        sections: Vec<u64>,
        xform: Xform,
    },
    /// `count` copies of each plane, copy k mapped by `xform` k times (Ctrl with Move or
    /// Rotate, arrays). The copies start inactive.
    CopySections {
        sections: Vec<u64>,
        xform: Xform,
        #[serde(default = "one")]
        count: u32,
    },
    /// Create Group from Slice: the plane's cut outline drawn into a sketch on the plane,
    /// in a new group in the open context (closed loops become faces).
    SliceToGroup {
        section: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Drawing sheets (lane 12, see [`crate::sheets`]): a new sheet at the end.
    AddSheet {
        setup: Box<crate::sheets::SheetSetup>,
    },
    /// A sheet's paper, margins, units, grid, length format, title block and name.
    SetSheet {
        sheet: u64,
        setup: Box<crate::sheets::SheetSetup>,
    },
    DeleteSheet {
        sheet: u64,
    },
    /// Put a sheet at position `to` in the document's order.
    MoveSheet {
        sheet: u64,
        to: u32,
    },
    /// Place an item on a sheet (viewport, dimension, label, table, image, line, rectangle).
    AddSheetItem {
        sheet: u64,
        item: Box<crate::sheets::ItemKind>,
    },
    /// Replace an item (moved, resized or changed); it keeps its id and kind.
    SetSheetItem {
        sheet: u64,
        item: u64,
        kind: Box<crate::sheets::ItemKind>,
    },
    /// Move items on the paper by `by` (metres).
    MoveSheetItems {
        sheet: u64,
        items: Vec<u64>,
        by: [f64; 2],
    },
    /// Delete items (a viewport's dimensions go with it).
    DeleteSheetItems {
        sheet: u64,
        items: Vec<u64>,
    },
    /// Put an item at position `to` in its sheet's drawing order.
    OrderSheetItem {
        sheet: u64,
        item: u64,
        to: u32,
    },
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

impl Op {
    /// Opening or closing a group or component for editing: logged, never an undo step.
    pub fn is_navigation(&self) -> bool {
        matches!(self, Op::SetContext { .. } | Op::LeaveContext)
    }
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

/// A material as an op or a file carries it: an sRGB colour with alpha, a texture (a path
/// of an image the model holds, or the file name of one it does not), its real-world size,
/// rotation and offset, and its stock link (see [`crate::looks`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct MaterialDef {
    pub name: String,
    pub color: [u8; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub rotation: f64,
    #[serde(default, skip_serializing_if = "is_zero2")]
    pub offset: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock: Option<crate::looks::StockLink>,
}

impl MaterialDef {
    /// The material it defines.
    pub fn material(&self) -> Material {
        Material {
            color: self.color,
            texture: self.texture.clone(),
            size: self.size,
            rotation: self.rotation,
            offset: self.offset,
            stock: self.stock.clone(),
        }
    }

    /// As an op or a file writes it.
    pub fn of(name: &str, m: &Material) -> MaterialDef {
        MaterialDef {
            name: name.to_string(),
            color: m.color,
            texture: m.texture.clone(),
            size: m.size,
            rotation: m.rotation,
            offset: m.offset,
            stock: m.stock.clone(),
        }
    }

    /// A plain colour, as before lane 10.
    pub fn plain(name: &str, color: [u8; 4], texture: Option<String>) -> MaterialDef {
        MaterialDef::of(name, &Material::plain(color, texture))
    }
}

pub(crate) fn is_zero_f(x: &f64) -> bool {
    *x == 0.0
}

pub(crate) fn is_zero2(x: &[f64; 2]) -> bool {
    *x == [0.0, 0.0]
}

fn yes() -> bool {
    true
}

fn one() -> u32 {
    1
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
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
    /// The group or instance open when the scene was saved (public id; `None`: the model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// The drawing style saved with the scene (lane 10); activating the scene brings it
    /// back. Absent: the scene leaves the style alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<crate::looks::Style>,
    /// The sun and shadows saved with the scene, likewise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadows: Option<crate::looks::Shadows>,
    /// The active and hidden section planes saved with the scene (lane 12); activating
    /// the scene brings them back. Absent: the scene leaves the sections alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<Box<crate::section::SceneSections>>,
}

fn default_fov() -> f64 {
    35.0
}

/// A component definition in an import: its own geometry (in its own coordinates) and the
/// groups and instances nested in it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct ImportDefinition {
    pub name: String,
    #[serde(default)]
    pub items: Vec<ImportItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ImportNode>,
    /// Section planes drawn in the definition (lane 12), in its own coordinates: each copy
    /// placed at the top of the import gets its own, inactive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<ImportSection>,
}

/// A section plane in an import (lane 12): a point, the direction of the side it keeps, a
/// name, hidden or not. Imported planes come in inactive.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct ImportSection {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

/// A group (geometry in the enclosing frame's coordinates, plus nested nodes) or an instance
/// of an import definition (index into `definitions`, placed by `xform` in the enclosing
/// frame).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct ImportNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    /// Section planes drawn in the group (lane 12), in the enclosing frame (world for a
    /// group at the top of the import); they come in inactive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<ImportSection>,
    #[serde(flatten)]
    pub kind: ImportNodeKind,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportNodeKind {
    Group {
        #[serde(default)]
        items: Vec<ImportItem>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<ImportNode>,
    },
    Instance {
        definition: usize,
        xform: Xform,
    },
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
    /// The editing context changed (`None`: the model).
    ContextChanged {
        context: Option<u64>,
    },
    ComponentCreated {
        instance: u64,
        definition: u64,
    },
    MadeUnique {
        definition: u64,
    },
    /// A node was exploded: the solids and nodes that were directly in it.
    Exploded {
        objects: Vec<u64>,
        nodes: Vec<u64>,
    },
    NodeDeleted,
    VisibilitySet,
    LockSet,
    SceneSet,
    SceneActivated {
        context: Option<u64>,
    },
    TagSet,
    Undone,
    Redone,
    /// Follow Me on a face: the solid grew along the path, or (`cut`) the sweep was cut out
    /// of it; `objects` is the solid, then any pieces it came apart into.
    Swept {
        objects: Vec<u64>,
        cut: bool,
    },
    /// Solids (and groups and instances) were moved, turned or scaled (the same ids).
    Transformed {
        objects: Vec<u64>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        nodes: Vec<u64>,
    },
    /// New solids (and groups and instances) from a copy or an array, in creation order.
    Copied {
        objects: Vec<u64>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        nodes: Vec<u64>,
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
    /// A texture image went into the model under `path`.
    TextureAdded {
        path: String,
    },
    /// The style or the shadows changed.
    StyleSet,
    Renamed,
    MeshDeleted,
    /// A part's grain or stock was set (cut list).
    PartSet,
    /// Loose meshes went into groups, one per edge-connected piece (lane 15).
    LooseGrouped {
        groups: Vec<u64>,
    },
    /// The stock library or the nesting settings changed.
    StockSet,
    /// A saved model was restored.
    Restored {
        objects: usize,
        meshes: usize,
        sketches: usize,
    },
    /// A sheet was added (lane 12).
    SheetAdded {
        sheet: u64,
    },
    /// An item was placed on a sheet.
    SheetItemAdded {
        sheet: u64,
        item: u64,
    },
    /// A sheet or its items changed.
    SheetSet,
    /// A section plane was added (lane 12).
    SectionAdded {
        section: u64,
    },
    /// A section plane was placed, flipped, (de)activated, renamed, hidden or shown.
    SectionSet,
    SectionDeleted,
    /// Section planes moved (the same ids) or copied (the new ids, in creation order).
    SectionsMoved {
        sections: Vec<u64>,
    },
    SectionsCopied {
        sections: Vec<u64>,
    },
    /// Create Group from Slice: the new group and the sketch in it.
    Sliced {
        group: u64,
        sketch: u64,
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
    /// Groups and component instances (see [`crate::nodes`]).
    pub(crate) nodes: SlotMap<NodeId, Node>,
    /// Component definitions.
    pub(crate) defs: SlotMap<DefId, Definition>,
    /// The open group or instance (`None`: the model).
    pub(crate) context: Option<NodeId>,
    /// Tag name -> visible. Every tag an object uses is listed here.
    pub(crate) tags: BTreeMap<String, bool>,
    /// Tag name -> sRGB colour (the Tags pane), for tags that have one.
    pub(crate) tag_colors: BTreeMap<String, [u8; 3]>,
    /// What the op in progress changed (propagation; transient).
    pub(crate) dirty: Dirty,
    pub(crate) guides: SlotMap<GuideId, Guide>,
    pub(crate) dimensions: SlotMap<DimensionId, Dimension>,
    /// Imported geometry that is not a valid solid (see [`crate::meshbuild`]).
    pub(crate) meshes: SlotMap<MeshId, LooseMesh>,
    pub(crate) mesh_order: Vec<MeshId>,
    /// Material name -> definition.
    pub(crate) materials: BTreeMap<String, Material>,
    /// Texture images by path (`assets/...`), lane 10.
    pub(crate) textures: BTreeMap<String, crate::looks::TextureImage>,
    /// The drawing style (`None`: the default) and the sun and shadows (`None`: off).
    pub(crate) style: Option<crate::looks::Style>,
    pub(crate) shadows: Option<crate::looks::Shadows>,
    /// Saved views, in order.
    pub(crate) scenes: Vec<SceneDef>,
    /// Last display number handed to an object (see [`Object::number`]).
    pub(crate) last_number: u64,
    /// The stock library and nesting settings (lane 9); `None`: the built-in library.
    pub(crate) stock: Option<crate::stock::StockLibrary>,
    /// Section planes (lane 12), see [`crate::section`].
    pub(crate) sections: SlotMap<crate::ids::SectionId, crate::section::SectionPlane>,
    /// Drawing sheets and their order (lane 12), see [`crate::sheets`].
    pub(crate) sheets: SlotMap<crate::ids::SheetId, crate::sheets::Sheet>,
    pub(crate) sheet_order: Vec<crate::ids::SheetId>,
}

/// A material: sRGB colour with alpha; a texture (a path in the model's texture table, or a
/// file name kept as a reference) with its real-world size (metres; `None`: one foot across
/// at the image's aspect), rotation (degrees) and offset (metres); and its stock link.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub color: [u8; 4],
    pub texture: Option<String>,
    pub size: Option<[f64; 2]>,
    pub rotation: f64,
    pub offset: [f64; 2],
    pub stock: Option<crate::looks::StockLink>,
}

impl Material {
    /// A plain colour (and maybe a texture reference), as before lane 10.
    pub fn plain(color: [u8; 4], texture: Option<String>) -> Material {
        Material {
            color,
            texture,
            size: None,
            rotation: 0.0,
            offset: [0.0, 0.0],
            stock: None,
        }
    }

    /// Nothing beyond the colour and the texture name (hashed as before lane 10).
    pub fn is_plain(&self) -> bool {
        self.size.is_none()
            && self.rotation == 0.0
            && self.offset == [0.0, 0.0]
            && self.stock.is_none()
    }
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

/// A group as lane 2 saw it: its name and the solids directly in it (public ids).
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

    /// Groups (not instances) with their public ids, in id order: name ("Group" when it
    /// has none) and the solids directly in each.
    pub fn groups(&self) -> Vec<(u64, Group)> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.kind == NodeKind::Group)
            .map(|(k, n)| {
                (
                    to_public(k),
                    Group {
                        name: n.name.clone().unwrap_or_else(|| "Group".into()),
                        objects: self
                            .object_order
                            .iter()
                            .filter(|&&o| self.objects[o].member.parent == Some(k))
                            .map(|&o| to_public(o))
                            .collect(),
                    },
                )
            })
            .collect()
    }

    /// Anything lane 5 added beyond lane 2's flat, named groups of solids: instances,
    /// definitions, nested or non-default nodes, hidden / locked items, sketches and meshes
    /// in groups, the editing context, tag colours, scene contexts. Hashed only when
    /// present, so older documents keep their hashes.
    pub(crate) fn has_structure(&self) -> bool {
        !self.defs.is_empty()
            || self.context.is_some()
            || !self.tag_colors.is_empty()
            || self.nodes.values().any(|n| {
                n.kind != NodeKind::Group
                    || n.name.is_none()
                    || n.tag.is_some()
                    || n.member != crate::nodes::Membership::default()
                    || n.xform != Xform::IDENTITY
                    || !n.part.is_default()
            })
            || self
                .objects
                .values()
                .any(|o| o.member.hidden || o.member.locked || o.member.slot.is_some())
            || self
                .meshes
                .values()
                .any(|m| m.member != crate::nodes::Membership::default())
            || self.sketches.values().any(|s| s.parent.is_some())
            || self.scenes.iter().any(|s| s.context.is_some())
    }

    /// Tags and their visibility, by name.
    pub fn tags(&self) -> &BTreeMap<String, bool> {
        &self.tags
    }

    /// True when the object is shown: not hidden, untagged or on a visible tag, and every
    /// group and instance around it shown.
    pub fn is_visible(&self, object: u64) -> bool {
        self.object(object)
            .is_some_and(|o| self.member_shown(&o.member, o.tag()))
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

    /// True when the mesh is shown (see [`Model::is_visible`]).
    pub fn is_mesh_visible(&self, mesh: u64) -> bool {
        self.mesh(mesh)
            .is_some_and(|m| self.member_shown(&m.member, m.tag.as_deref()))
    }

    /// Materials by name.
    pub fn materials(&self) -> &BTreeMap<String, Material> {
        &self.materials
    }

    /// Saved views, in order.
    pub fn scenes(&self) -> &[SceneDef] {
        &self.scenes
    }

    /// The stock library and nesting settings (the built-in ones until someone edits them).
    pub fn stock_library(&self) -> std::borrow::Cow<'_, crate::stock::StockLibrary> {
        match &self.stock {
            Some(l) => std::borrow::Cow::Borrowed(l),
            None => std::borrow::Cow::Owned(crate::stock::StockLibrary::builtin()),
        }
    }

    /// The library as stored: `None` when it is the built-in one.
    pub fn stored_stock_library(&self) -> Option<&crate::stock::StockLibrary> {
        self.stock.as_ref()
    }

    /// Edit the library; a library edited back to the built-in one is stored as `None`.
    pub(crate) fn edit_stock(
        &mut self,
        f: impl FnOnce(&mut crate::stock::StockLibrary) -> Result<(), crate::error::StockError>,
    ) -> Result<OpOutcome, KernelError> {
        let builtin = crate::stock::StockLibrary::builtin();
        let mut lib = self.stock.clone().unwrap_or_else(|| builtin.clone());
        f(&mut lib)?;
        lib.check()?;
        self.stock = (lib != builtin).then_some(lib);
        Ok(OpOutcome::StockSet)
    }

    /// Public ids of the sketches, in creation order.
    pub fn sketch_ids(&self) -> Vec<u64> {
        self.sketch_order.iter().map(|&k| to_public(k)).collect()
    }

    /// Apply an op transactionally.
    pub fn apply(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        let mut next = self.clone();
        next.dirty = Dirty::default();
        let outcome = next.apply_in_place(op)?;
        next.tidy()?;
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

    /// One op, then propagation of what it changed into component definitions.
    fn apply_in_place(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        let out = self.apply_op(op)?;
        self.propagate()?;
        Ok(out)
    }

    fn apply_op(&mut self, op: &Op) -> Result<OpOutcome, KernelError> {
        match op {
            Op::NewSketch { origin, normal } => {
                let plane = Plane::new(DVec3::from_array(*origin), DVec3::from_array(*normal))
                    .map_err(SketchError::from)?;
                let mut sk = Sketch::new(plane);
                sk.parent = self.context;
                let id = self.sketches.insert(sk);
                self.sketch_order.push(id);
                Ok(OpOutcome::SketchCreated {
                    sketch: to_public(id),
                })
            }
            Op::AddRectangle { sketch, x, y, w, h } => {
                let sid = self.sketch_edit_key(*sketch)?;
                let r = self.sketches[sid].add_rectangle(DVec2::new(*x, *y), DVec2::new(*w, *h))?;
                Ok(Self::edited(r))
            }
            Op::InsertSegment { sketch, a, b } => {
                let sid = self.sketch_edit_key(*sketch)?;
                let r = self.sketches[sid]
                    .insert_segment(DVec2::from_array(*a), DVec2::from_array(*b))?;
                Ok(Self::edited(r))
            }
            Op::EraseSegment { sketch, a, b } => {
                let sid = self.sketch_edit_key(*sketch)?;
                let r = self.sketches[sid]
                    .erase_segment(DVec2::from_array(*a), DVec2::from_array(*b))?;
                Ok(Self::edited(r))
            }
            Op::AddPolyline {
                sketch,
                points,
                closed,
            } => {
                let sid = self.sketch_edit_key(*sketch)?;
                let pts: Vec<DVec2> = points.iter().map(|p| DVec2::from_array(*p)).collect();
                let r = self.sketches[sid].insert_polyline(&pts, *closed)?;
                Ok(Self::edited(r))
            }
            Op::Extrude {
                sketch,
                region,
                distance,
            } => {
                let sid = self.sketch_edit_key(*sketch)?;
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
                obj.member.parent = self.context;
                self.sketches[sid].consume_region(rid)?;
                let id = self.insert_object(obj);
                self.mark(self.context);
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
                self.editable_object(oid)?;
                let o = &mut self.objects[oid];
                let pushed = match op {
                    Op::PushPullNewFace { .. } => o.push_pull_new_face(*face, *distance)?,
                    _ => o.push_pull_id(*face, *distance)?,
                };
                match pushed {
                    PushOutcome::Moved => {
                        self.touch(oid);
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
                        self.mark(removed.member.parent);
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
                    // A stroke on a face or sketch outside the open context goes into the
                    // context's own sketch on that plane (strokes never stick across
                    // contexts).
                    DrawTarget::Face { object, face }
                        if self
                            .objects
                            .get(from_public::<ObjectId>(*object))
                            .is_some_and(|_| {
                                !self.object_is_editable(from_public::<ObjectId>(*object))
                            }) =>
                    {
                        let o = &self.objects[from_public::<ObjectId>(*object)];
                        let fi = o
                            .face_ordinal(*face)
                            .ok_or(FaceEditError::NoSuchFace(*face))?;
                        let plane = o
                            .brep()
                            .to_polys()
                            .plane(fi)
                            .ok_or(FaceEditError::Degenerate)?;
                        self.draw_on_plane(&plane, &pts, *closed)
                    }
                    DrawTarget::Sketch { sketch }
                        if self
                            .sketches
                            .get(from_public::<SketchId>(*sketch))
                            .is_some_and(|_| {
                                !self.sketch_is_editable(from_public::<SketchId>(*sketch))
                            }) =>
                    {
                        let plane = *self.sketches[from_public::<SketchId>(*sketch)].plane();
                        self.draw_on_plane(&plane, &pts, *closed)
                    }
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
                        let sid = self.sketch_edit_key(*sketch)?;
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
                        self.draw_on_plane(&plane, &pts, *closed)
                    }
                }
            }
            Op::EraseEdge { object, a, b } => {
                let k = from_public::<ObjectId>(*object);
                if !self.objects.contains_key(k) {
                    return Err(FaceEditError::NoSuchObject(*object).into());
                }
                self.editable_object(k)?;
                let face =
                    self.objects[k].erase_edge(DVec3::from_array(*a), DVec3::from_array(*b))?;
                self.touch(k);
                Ok(OpOutcome::FacesMerged { face })
            }
            Op::EraseFace { object, face } => {
                let (o, fi) = self.face_edit_target(*object, *face)?;
                let face = o.erase_face(fi)?;
                Ok(OpOutcome::FacesMerged { face })
            }
            Op::EraseRegion { sketch, region } => {
                let sid = self.sketch_edit_key(*sketch)?;
                let r = self.sketches[sid].erase_region(from_public::<RegionId>(*region))?;
                Ok(Self::edited(r))
            }
            Op::Batch { ops } => {
                if ops.is_empty() {
                    return Err(DocumentError::EmptyBatch.into());
                }
                let mut outcomes = Vec::with_capacity(ops.len());
                for op in ops {
                    if matches!(
                        op,
                        Op::Undo
                            | Op::Redo
                            | Op::Batch { .. }
                            | Op::SetContext { .. }
                            | Op::LeaveContext
                    ) {
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
            Op::Transform {
                objects,
                xform,
                nodes,
            } => {
                let (keys, nkeys) = self.transform_targets(objects, nodes)?;
                xform.check()?;
                for &k in &keys {
                    let next = self.objects[k].transformed(xform)?;
                    self.objects[k] = next;
                    self.touch(k);
                }
                for &n in &nkeys {
                    self.transform_node(n, xform)?;
                }
                Ok(OpOutcome::Transformed {
                    objects: keys.into_iter().map(to_public).collect(),
                    nodes: nkeys.into_iter().map(to_public).collect(),
                })
            }
            Op::Copy {
                objects,
                xform,
                nodes,
            } => {
                let (keys, nkeys) = self.transform_targets(objects, nodes)?;
                let (objects, nodes) = self.copies(&keys, &nkeys, xform, 1)?;
                Ok(OpOutcome::Copied { objects, nodes })
            }
            Op::Array {
                objects,
                xform,
                count,
                nodes,
            } => {
                if *count == 0 || *count > MAX_ARRAY {
                    return Err(TransformError::BadCount.into());
                }
                let (keys, nkeys) = self.transform_targets(objects, nodes)?;
                let (objects, nodes) = self.copies(&keys, &nkeys, xform, *count)?;
                Ok(OpOutcome::Copied { objects, nodes })
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
                self.editable_object(k)?;
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from_array(*p)).collect();
                let (next, faces) = o.transform_geometry(&pts, xform, *autofold)?;
                self.objects[k] = next;
                self.touch(k);
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
                if !self.objects.contains_key(k) {
                    return Err(DocumentError::NoSuchObject(*object).into());
                }
                self.editable_object(k)?;
                let removed = self.objects.remove(k).expect("checked");
                self.object_order.retain(|&o| o != k);
                self.mark(removed.member.parent);
                Ok(OpOutcome::ObjectDeleted)
            }
            Op::CreateGroup { name, objects } => {
                check_name(name)?;
                if objects.is_empty() {
                    return Err(DocumentError::EmptyGroup.into());
                }
                let items: Vec<Item> = objects.iter().map(|&o| Item::Object(o)).collect();
                self.make_group(&items, Some(name))
            }
            Op::DeleteGroup { group } => {
                let k = from_public::<NodeId>(*group);
                if self.nodes.get(k).is_none_or(|n| n.kind != NodeKind::Group) {
                    return Err(DocumentError::NoSuchGroup(*group).into());
                }
                self.explode(*group)?;
                Ok(OpOutcome::GroupDeleted)
            }
            Op::SetContext { node } => self.set_context(*node),
            Op::LeaveContext => self.leave_context(),
            Op::MakeGroup { items, name } => self.make_group(items, name.as_deref()),
            Op::MakeComponent { items, name } => self.make_component(items, name),
            Op::MakeUnique { node } => self.make_unique(*node),
            Op::Explode { node } => self.explode(*node),
            Op::DeleteNode { node } => self.delete_node(*node),
            Op::Rename { item, name } => self.rename(*item, name.as_deref()),
            Op::RenameDefinition { definition, name } => self.rename_definition(*definition, name),
            Op::SetHidden { items, hidden } => self.set_flag(items, false, *hidden),
            Op::SetLocked { items, locked } => self.set_flag(items, true, *locked),
            Op::TagItems { items, tag } => self.tag_items(items, tag.as_deref()),
            Op::CreateTag { name, color } => self.create_tag(name, *color),
            Op::RenameTag { tag, name } => self.rename_tag(tag, name),
            Op::SetTagColor { tag, color } => self.set_tag_color(tag, *color),
            Op::DeleteTag { tag } => self.delete_tag(tag),
            Op::AddScene { scene } => self.add_scene(scene),
            Op::UpdateScene { name, scene } => self.update_scene(name, scene),
            Op::DeleteScene { name } => self.delete_scene(name),
            Op::ActivateScene { name } => self.activate_scene(name),
            Op::SetTag { object, tag } => {
                let items = [Item::Object(*object)];
                if let Some(t) = tag
                    && !crate::nodes::is_untagged_name(t)
                {
                    check_name(t)?;
                }
                if self.object(*object).is_none() {
                    return Err(DocumentError::NoSuchObject(*object).into());
                }
                self.tag_items(&items, tag.as_deref())
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
                definitions,
                nodes,
            } => self.import(
                items,
                strokes,
                materials,
                scenes,
                hidden_tags,
                definitions,
                nodes,
            ),
            Op::DefineMaterial {
                name,
                color,
                texture,
                size,
                rotation,
                offset,
                stock,
            } => {
                check_name(name)?;
                self.define_material(
                    name.trim(),
                    Material {
                        color: *color,
                        texture: texture.clone(),
                        size: *size,
                        rotation: *rotation,
                        offset: *offset,
                        stock: stock.clone(),
                    },
                )
            }
            Op::RenameMaterial { name, to } => self.rename_material(name, to),
            Op::DeleteMaterial { name } => self.delete_material(name),
            Op::AddTexture { path, mime, data } => self.add_texture(path, mime, data),
            Op::Paint { target, material } => self.paint(target, material.as_deref()),
            Op::SetFaceUv { object, face, uv } => self.set_face_uv(*object, *face, uv.as_ref()),
            Op::SetStyle { style } => self.set_style(style.as_ref()),
            Op::SetShadows { shadows } => self.set_shadows(shadows.as_ref()),
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
                let p = o.member.parent;
                self.mark(p);
                Ok(OpOutcome::MaterialSet)
            }
            Op::RenameObject { object, name } => {
                if self.object(*object).is_none() {
                    return Err(DocumentError::NoSuchObject(*object).into());
                }
                self.rename(Item::Object(*object), name.as_deref())
            }
            Op::DeleteMesh { mesh } => {
                let k = from_public::<MeshId>(*mesh);
                if !self.meshes.contains_key(k) {
                    return Err(DocumentError::NoSuchMesh(*mesh).into());
                }
                self.editable_mesh(k)?;
                let removed = self.meshes.remove(k).expect("checked");
                self.mesh_order.retain(|&m| m != k);
                self.mark(removed.member.parent);
                Ok(OpOutcome::MeshDeleted)
            }
            Op::SetGrain { part, grain } => {
                let g = (*grain != crate::cutlist::Grain::Length).then_some(*grain);
                self.set_part(*part, |a| a.grain = g)
            }
            Op::SetStock { part, stock, sheet } => {
                let stock = match stock {
                    Some(s) if !s.trim().is_empty() => {
                        check_name(s)?;
                        Some(s.trim().to_string())
                    }
                    _ => None,
                };
                let sheet = *sheet;
                self.set_part(*part, move |a| {
                    a.stock = stock;
                    a.sheet = sheet;
                })
            }
            Op::SetInclude { part, include } => {
                let v = (!*include).then_some(false);
                self.set_part(*part, |a| a.include = v)
            }
            Op::GroupLoose { meshes } => self.group_loose(meshes),
            Op::DefineStock { stock, replace } => {
                self.edit_stock(|l| l.define(stock, replace.as_deref()))
            }
            Op::DeleteStock { name } => self.edit_stock(|l| l.delete(name)),
            Op::SetMaterialStock { material, stock } => {
                self.edit_stock(|l| l.set_default(material, stock.as_deref()))
            }
            Op::SetNestSettings { settings } => self.edit_stock(|l| {
                settings.check()?;
                l.settings = *settings;
                Ok(())
            }),
            Op::ResetStockLibrary => {
                self.stock = None;
                Ok(OpOutcome::StockSet)
            }
            Op::AddSection {
                origin,
                normal,
                name,
                active,
                hidden,
            } => self.add_section(origin, normal, name.as_deref(), *active, *hidden),
            Op::PlaceSection {
                section,
                origin,
                normal,
            } => self.place_section(*section, origin, normal),
            Op::FlipSection { section } => self.flip_section(*section),
            Op::SetSectionActive { section, active } => self.set_section_active(*section, *active),
            Op::RenameSection { section, name } => self.rename_section(*section, name),
            Op::SetSectionHidden { section, hidden } => self.set_section_hidden(*section, *hidden),
            Op::DeleteSection { section } => self.delete_section(*section),
            Op::MoveSections { sections, xform } => {
                if sections.is_empty() {
                    return Err(TransformError::NothingSelected.into());
                }
                xform.check()?;
                let keys = self.section_targets(sections)?;
                self.transform_sections(&keys, xform)?;
                Ok(OpOutcome::SectionsMoved {
                    sections: keys.into_iter().map(to_public).collect(),
                })
            }
            Op::CopySections {
                sections,
                xform,
                count,
            } => {
                if sections.is_empty() {
                    return Err(TransformError::NothingSelected.into());
                }
                if *count == 0 || *count > MAX_ARRAY {
                    return Err(TransformError::BadCount.into());
                }
                xform.check()?;
                let keys = self.section_targets(sections)?;
                let sections = self.copy_sections(&keys, xform, *count)?;
                Ok(OpOutcome::SectionsCopied { sections })
            }
            Op::SliceToGroup { section, name } => self.slice_to_group(*section, name.as_deref()),
            Op::AddSheet { setup } => self.add_sheet(setup),
            Op::SetSheet { sheet, setup } => self.set_sheet(*sheet, setup),
            Op::DeleteSheet { sheet } => self.delete_sheet(*sheet),
            Op::MoveSheet { sheet, to } => self.move_sheet(*sheet, *to),
            Op::AddSheetItem { sheet, item } => self.add_sheet_item(*sheet, item),
            Op::SetSheetItem { sheet, item, kind } => self.set_sheet_item(*sheet, *item, kind),
            Op::MoveSheetItems { sheet, items, by } => self.move_sheet_items(*sheet, items, *by),
            Op::DeleteSheetItems { sheet, items } => self.delete_sheet_items(*sheet, items),
            Op::OrderSheetItem { sheet, item, to } => self.order_sheet_item(*sheet, *item, *to),
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
                let sid = self.sketch_edit_key(*sketch)?;
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
                let mut obj = swept_object(&p)?;
                obj.member.parent = self.context;
                self.sketches[sid].consume_region(rid)?;
                let id = self.insert_object(obj);
                self.mark(self.context);
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
                self.editable_object(k)?;
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
                self.touch(k);
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
            OffsetSource::Region { sketch, .. } => self.sketch_edit_key(*sketch)?,
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

    /// The live objects and nodes named (duplicates dropped, order kept), each directly in
    /// the open context and unlocked.
    fn transform_targets(
        &self,
        objects: &[u64],
        nodes: &[u64],
    ) -> Result<(Vec<ObjectId>, Vec<NodeId>), KernelError> {
        if objects.is_empty() && nodes.is_empty() {
            return Err(TransformError::NothingSelected.into());
        }
        let mut keys: Vec<ObjectId> = Vec::with_capacity(objects.len());
        for &o in objects {
            let k = from_public::<ObjectId>(o);
            if !self.objects.contains_key(k) {
                return Err(TransformError::NoSuchObject(o).into());
            }
            self.editable_object(k)?;
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        let mut nkeys: Vec<NodeId> = Vec::with_capacity(nodes.len());
        for &n in nodes {
            let k = self.node_key(n)?;
            self.editable_node(k)?;
            if !nkeys.contains(&k) {
                nkeys.push(k);
            }
        }
        Ok((keys, nkeys))
    }

    /// `count` copies of each object and node, copy k mapped by `xform` k times. Returns the
    /// new ids (solids, nodes). Solids stay plain solids; an instance's copy is an instance.
    fn copies(
        &mut self,
        keys: &[ObjectId],
        nkeys: &[NodeId],
        xform: &Xform,
        count: u32,
    ) -> Result<(Vec<u64>, Vec<u64>), KernelError> {
        xform.check()?;
        let mut made = Vec::with_capacity(keys.len() * count as usize);
        let mut made_nodes = Vec::with_capacity(nkeys.len() * count as usize);
        for k in 1..=count {
            let x = xform.power(k);
            for &key in keys {
                let mut copy = self.objects[key].transformed(&x)?;
                copy.source = None;
                copy.member.slot = None;
                copy.member.locked = false;
                made.push(to_public(self.insert_object(copy)));
            }
            for &n in nkeys {
                made_nodes.push(to_public(self.copy_node(n, &x, self.context)?));
            }
        }
        self.mark(self.context);
        Ok((made, made_nodes))
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
        for piece in it {
            keys.push(self.insert_object(piece));
        }
        // Inside a shared component, a cut that comes apart makes that copy unique.
        self.mark_severed(self.objects[oid].member.parent);
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

    /// [`Op::Import`]: everything or nothing (the caller validates and commits). Everything
    /// goes into the open context.
    #[allow(clippy::too_many_arguments)]
    fn import(
        &mut self,
        items: &[ImportItem],
        strokes: &[ImportStroke],
        materials: &[MaterialDef],
        scenes: &[SceneDef],
        hidden_tags: &[String],
        definitions: &[ImportDefinition],
        nodes: &[ImportNode],
    ) -> Result<OpOutcome, KernelError> {
        let mut notes = Vec::new();
        for m in materials {
            let mut mat = m.material();
            mat.rotation = mat.rotation.rem_euclid(360.0);
            if Self::check_material_fields(&mat).is_err() {
                notes.push(format!(
                    "{}: texture size, rotation or offset out of range, left at the defaults",
                    m.name
                ));
                mat = Material {
                    stock: None,
                    ..Material::plain(m.color, m.texture.clone())
                };
            }
            self.materials.insert(clip_name(&m.name, "Material"), mat);
        }
        let (mut objects, mut meshes) = (Vec::new(), Vec::new());
        let mut groups: Vec<(String, Vec<Thing>)> = Vec::new();
        let ctx = self.context;
        for item in items {
            let built = build_item(item)?;
            let sources = built.face_sources.clone();
            notes.extend(built.notes);
            let tag = self.import_tag(item.tag.as_deref());
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
                notes.extend(self.import_face_paint(&mut o, item, sources.get(k)));
                o.member.parent = ctx;
                let id = self.insert_object(o);
                made.push(Thing::Object(id));
                objects.push(to_public(id));
            }
            if let Some(mut m) = built.loose {
                m.name = base.clone();
                m.tag = tag.clone();
                m.material = material.clone();
                m.member.parent = ctx;
                let id = self.meshes.insert(m);
                self.mesh_order.push(id);
                meshes.push(to_public(id));
                made.push(Thing::Mesh(id));
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
        }
        let mut group_ids = Vec::new();
        for (name, members) in groups {
            let g = self.nodes.insert(Node {
                name: Some(name),
                kind: NodeKind::Group,
                xform: Xform::IDENTITY,
                tag: None,
                member: crate::nodes::Membership {
                    parent: ctx,
                    ..Default::default()
                },
                part: Default::default(),
                material: None,
            });
            for t in members {
                match t {
                    Thing::Object(k) => self.objects[k].member.parent = Some(g),
                    Thing::Mesh(k) => self.meshes[k].member.parent = Some(g),
                    Thing::Node(_) => {}
                }
            }
            group_ids.push(to_public(g));
        }
        // Groups and components (a `.skp`'s structure).
        let mut def_ids: Vec<DefId> = Vec::with_capacity(definitions.len());
        for d in definitions {
            let mut def = Definition {
                name: clip_name(&d.name, crate::nodes::DEFAULT_COMPONENT_NAME),
                members: BTreeMap::new(),
                next_slot: 1,
                part: Default::default(),
            };
            self.template_items(&mut def, None, &d.items, &mut notes)?;
            self.template_nodes(&mut def, None, &d.children, &def_ids, &mut notes)?;
            def_ids.push(self.defs.insert(def));
        }
        let mut made_nodes = Vec::new();
        let def_sections: Vec<&[ImportSection]> =
            definitions.iter().map(|d| d.sections.as_slice()).collect();
        for n in nodes {
            made_nodes.push(self.import_node(
                n,
                ctx,
                &def_ids,
                &def_sections,
                &mut notes,
                &mut meshes,
            )?);
        }
        if !made_nodes.is_empty() {
            let kids = self.kids();
            for &n in &made_nodes {
                for t in self.subtree_things(&kids, n) {
                    if let Thing::Object(k) = t {
                        objects.push(to_public(k));
                    }
                }
            }
            group_ids.extend(made_nodes.iter().map(|&n| to_public(n)));
        }
        self.mark(ctx);
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
            if !crate::nodes::is_untagged_name(t) {
                self.tags.insert(clip_name(t, "Tag"), false);
            }
        }
        if objects.is_empty()
            && group_ids.is_empty()
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

    /// A tag from a file: "", "Untagged" and "Layer0"-style defaults mean none; otherwise the
    /// tag is listed (visible unless it already is listed hidden).
    fn import_tag(&mut self, tag: Option<&str>) -> Option<String> {
        let t = tag?;
        if crate::nodes::is_untagged_name(t) {
            return None;
        }
        let t = clip_name(t, "Tag");
        self.tags.entry(t.clone()).or_insert(true);
        Some(t)
    }

    /// Import items as template members of `def` (definition coordinates).
    fn template_items(
        &mut self,
        def: &mut Definition,
        parent: Option<u32>,
        items: &[ImportItem],
        notes: &mut Vec<String>,
    ) -> Result<(), KernelError> {
        use crate::nodes::{Member, MemberKind};
        for item in items {
            let built = build_item(item)?;
            let sources = built.face_sources.clone();
            notes.extend(built.notes);
            let tag = self.import_tag(item.tag.as_deref());
            let material = item
                .material
                .as_deref()
                .map(|m| clip_name(m, "Material"))
                .filter(|m| self.materials.contains_key(m));
            let base = clip_name(&item.name, "Imported");
            let n = built.solids.len();
            for (k, mut o) in built.solids.into_iter().enumerate() {
                o.material = material.clone();
                notes.extend(self.import_face_paint(&mut o, item, sources.get(k)));
                let name = if n == 1 {
                    base.clone()
                } else {
                    clip_name(&format!("{base} {}", k + 1), "Imported")
                };
                let slot = def.next_slot;
                def.next_slot += 1;
                def.members.insert(
                    slot,
                    Member {
                        parent,
                        name: Some(name),
                        tag: tag.clone(),
                        hidden: false,
                        locked: false,
                        part: Default::default(),
                        kind: MemberKind::Solid(Box::new(o)),
                        material: None,
                    },
                );
            }
            if let Some(mut m) = built.loose {
                m.name = base.clone();
                m.material = material.clone();
                let slot = def.next_slot;
                def.next_slot += 1;
                def.members.insert(
                    slot,
                    Member {
                        parent,
                        name: Some(base.clone()),
                        tag: tag.clone(),
                        hidden: false,
                        locked: false,
                        part: Default::default(),
                        kind: MemberKind::Mesh(Box::new(m)),
                        material: None,
                    },
                );
            }
        }
        Ok(())
    }

    /// Import nested groups and instances as template members of `def`.
    fn template_nodes(
        &mut self,
        def: &mut Definition,
        parent: Option<u32>,
        nodes: &[ImportNode],
        def_ids: &[DefId],
        notes: &mut Vec<String>,
    ) -> Result<(), KernelError> {
        use crate::nodes::{Member, MemberKind};
        for n in nodes {
            let slot = def.next_slot;
            def.next_slot += 1;
            let tag = self.import_tag(n.tag.as_deref());
            let name = n.name.as_deref().map(|x| clip_name(x, "Group"));
            match &n.kind {
                ImportNodeKind::Group { items, children } => {
                    def.members.insert(
                        slot,
                        Member {
                            parent,
                            name,
                            tag,
                            hidden: n.hidden,
                            locked: false,
                            part: Default::default(),
                            kind: MemberKind::Group(Xform::IDENTITY),
                            material: None,
                        },
                    );
                    self.template_items(def, Some(slot), items, notes)?;
                    self.template_nodes(def, Some(slot), children, def_ids, notes)?;
                }
                ImportNodeKind::Instance { definition, xform } => {
                    let d = *def_ids.get(*definition).ok_or_else(|| {
                        crate::error::StructureError::BadStructure(
                            "a definition places one that is not listed before it".into(),
                        )
                    })?;
                    xform.check()?;
                    def.members.insert(
                        slot,
                        Member {
                            parent,
                            name,
                            tag,
                            hidden: n.hidden,
                            locked: false,
                            part: Default::default(),
                            kind: MemberKind::Instance(d, *xform),
                            material: None,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    /// Import a group or instance into the world under `parent`.
    fn import_node(
        &mut self,
        n: &ImportNode,
        parent: Option<NodeId>,
        def_ids: &[DefId],
        def_sections: &[&[ImportSection]],
        notes: &mut Vec<String>,
        meshes: &mut Vec<u64>,
    ) -> Result<NodeId, KernelError> {
        let node = self.import_node_inner(n, parent, def_ids, def_sections, notes, meshes)?;
        // Section planes (lane 12): the group's own, in the enclosing frame; an instance's
        // come from its definition, placed with it. All inactive.
        let mut planes: Vec<(ImportSection, Option<Xform>)> =
            n.sections.iter().map(|s| (s.clone(), None)).collect();
        if let ImportNodeKind::Instance { definition, xform } = &n.kind {
            planes.extend(
                def_sections
                    .get(*definition)
                    .into_iter()
                    .flat_map(|x| x.iter())
                    .map(|s| (s.clone(), Some(*xform))),
            );
        }
        for (s, x) in planes {
            let Ok((origin, normal)) = crate::section::plane_from(&s.origin, &s.normal) else {
                notes.push("a section plane without a usable position was left out".into());
                continue;
            };
            let mut plane = crate::section::SectionPlane {
                name: s
                    .name
                    .as_deref()
                    .map_or_else(|| "Section".to_string(), |x| clip_name(x, "Section")),
                origin,
                normal,
                parent: Some(node),
                active: false,
                hidden: s.hidden,
            };
            if let Some(x) = x {
                plane = plane.mapped(&x)?;
            }
            self.sections.insert(plane);
        }
        Ok(node)
    }

    fn import_node_inner(
        &mut self,
        n: &ImportNode,
        parent: Option<NodeId>,
        def_ids: &[DefId],
        def_sections: &[&[ImportSection]],
        notes: &mut Vec<String>,
        meshes: &mut Vec<u64>,
    ) -> Result<NodeId, KernelError> {
        let tag = self.import_tag(n.tag.as_deref());
        let member = crate::nodes::Membership {
            parent,
            slot: None,
            hidden: n.hidden,
            locked: false,
        };
        match &n.kind {
            ImportNodeKind::Group { items, children } => {
                let g = self.nodes.insert(Node {
                    name: n.name.as_deref().map(|x| clip_name(x, "Group")),
                    kind: NodeKind::Group,
                    xform: Xform::IDENTITY,
                    tag,
                    member,
                    part: Default::default(),
                    material: None,
                });
                for item in items {
                    let built = build_item(item)?;
                    let sources = built.face_sources.clone();
                    notes.extend(built.notes);
                    let tag = self.import_tag(item.tag.as_deref());
                    let material = item
                        .material
                        .as_deref()
                        .map(|m| clip_name(m, "Material"))
                        .filter(|m| self.materials.contains_key(m));
                    let base = clip_name(&item.name, "Imported");
                    let count = built.solids.len();
                    for (k, mut o) in built.solids.into_iter().enumerate() {
                        o.name = Some(if count == 1 {
                            base.clone()
                        } else {
                            clip_name(&format!("{base} {}", k + 1), "Imported")
                        });
                        o.tag = tag.clone();
                        o.material = material.clone();
                        notes.extend(self.import_face_paint(&mut o, item, sources.get(k)));
                        o.member.parent = Some(g);
                        self.insert_object(o);
                    }
                    if let Some(mut m) = built.loose {
                        m.name = base.clone();
                        m.tag = tag.clone();
                        m.material = material.clone();
                        m.member.parent = Some(g);
                        let id = self.meshes.insert(m);
                        self.mesh_order.push(id);
                        meshes.push(to_public(id));
                    }
                }
                for c in children {
                    self.import_node(c, Some(g), def_ids, def_sections, notes, meshes)?;
                }
                Ok(g)
            }
            ImportNodeKind::Instance { definition, xform } => {
                let d = *def_ids.get(*definition).ok_or_else(|| {
                    crate::error::StructureError::BadStructure(
                        "an instance places a definition that is not in the import".into(),
                    )
                })?;
                xform.check()?;
                let i = self.nodes.insert(Node {
                    name: n.name.as_deref().map(|x| clip_name(x, "Component")),
                    kind: NodeKind::Instance(d),
                    xform: *xform,
                    tag,
                    member,
                    part: Default::default(),
                    material: None,
                });
                self.rederive(i)?;
                let kids = self.kids();
                for t in self.subtree_things(&kids, i) {
                    if let Thing::Mesh(k) = t {
                        meshes.push(to_public(k));
                    }
                }
                Ok(i)
            }
        }
    }

    /// The open context's sketch lying on `plane` (either facing, within the planarity
    /// tolerance), or a new one on it there. Strokes only stick to strokes of the same
    /// editing context.
    fn sketch_on(&mut self, plane: &Plane) -> SketchId {
        let found = self.sketch_order.iter().copied().find(|&k| {
            let s = &self.sketches[k];
            let sp = s.plane();
            s.parent == self.context
                && sp.normal.dot(plane.normal).abs() >= 1.0 - 1e-12
                && sp.signed_distance(plane.origin).abs() <= PLANARITY
        });
        found.unwrap_or_else(|| {
            let mut s = Sketch::new(*plane);
            s.parent = self.context;
            let id = self.sketches.insert(s);
            self.sketch_order.push(id);
            id
        })
    }

    /// A stroke onto a plane: into the open context's sketch on it (or a new one).
    fn draw_on_plane(
        &mut self,
        plane: &Plane,
        pts: &[DVec3],
        closed: bool,
    ) -> Result<OpOutcome, KernelError> {
        let sid = self.sketch_on(plane);
        let regions = self.draw_in_sketch(sid, pts, closed)?;
        Ok(OpOutcome::Drawn {
            sketch: Some(to_public(sid)),
            regions: regions.into_iter().map(to_public).collect(),
            faces: Vec::new(),
            inside: self.region_inside(sid, pts, closed),
        })
    }

    /// A sketch that may be edited: it exists and belongs to the open context.
    fn sketch_edit_key(&self, id: u64) -> Result<SketchId, KernelError> {
        let k = self.sketch_key(id)?;
        self.editable_sketch(k)?;
        Ok(k)
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

    /// The object and the ordinal of its face with persistent id `face`.
    fn face_edit_target(
        &mut self,
        object: u64,
        face: u64,
    ) -> Result<(&mut Object, usize), KernelError> {
        let k = from_public::<ObjectId>(object);
        if !self.objects.contains_key(k) {
            return Err(FaceEditError::NoSuchObject(object).into());
        }
        self.editable_object(k)?;
        self.touch(k);
        let o = &mut self.objects[k];
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
        if let Err(e) = self
            .check_structure()
            .and_then(|_| self.check_looks())
            .and_then(|_| self.check_sections())
            .and_then(|_| self.check_sheets())
        {
            return Err(TopologyError {
                violations: vec![Violation::Structure(e)],
                report: Default::default(),
            });
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
