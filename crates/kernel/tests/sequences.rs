//! Op-sequence fuzzer (proptest-driven) over the whole document: sketch strokes with
//! merging / T-junctions / crossings / overlaps / erasing, rectangles, polylines, extrude,
//! general push/pull (both ways, through the far side, severing), split_face, imprint,
//! erasing (object edges and faces, sketch regions), batches, delete, groups, tags, and
//! (lane 4) transforms, copies, arrays, moving part of a solid (autofold on and off),
//! offsets of faces / regions / edge chains, and Follow Me (open and closed paths, adding
//! and cutting), and (lane 5) groups and components: make group / component / unique,
//! explode, delete, open and close (the editing context), node moves / copies / arrays,
//! hide, lock, tags (create, rename, colour, delete, assign) and scenes (add, update,
//! delete, activate), and (lane 7) grain and stock on cut-list parts, (lane 9) the stock
//! library (define, replace, delete, per-material defaults, kerf / trim / algorithm, reset),
//! and (lane 10) materials (defined with and without a texture image and a stock link,
//! renamed, deleted), paint on faces and on solids, groups and instances, texture
//! positions, styles and shadows (in the document and in scenes), and (lane 11) plugin
//! batches: named batches whose ops name what earlier ops of the same batch made through
//! placeholders (boxes drawn and pulled, imported, renamed, made components, pushed, mixed
//! with ordinary intents and with garbage on purpose: placeholders to later ops, bad paths,
//! forbidden ops), resolved by `refbatch` and applied as one op; a refused batch must leave
//! the document, its log and its undo depth untouched, with undo / redo interleaved
//! anywhere. After every accepted op every face is sampled and its texture coordinates
//! checked (`testing::check_looks`), the cut list is worked out and checked against a
//! brute-force count of the world (`testing::check_cut_list`), and the parts are nested
//! with both algorithms and every layout invariant checked
//! (`testing::check_nesting`). While a group or component is
//! open, most intents pick what is in it, so edits propagate through definitions.
//!
//! `FUZZ_PLANE_SNAP=0` turns the boolean plane-snapping pass off for the run, so the same
//! seeds measure the "not a valid solid" refusal rate with and without it.
//!
//! Generated ops are *intents* ([`Fuzz`]) resolved against the live document as the
//! sequence runs: "an edge of some face of some solid", "a region of some sketch", "a
//! rectangle at these fractions of a face". That keeps most ops meaningful (a random id
//! would almost always be refused) while still feeding garbage on purpose (NaN, huge,
//! sub-tolerance, unknown ids). The resolved, concrete op list is what gets checked by
//! [`check_sequence`] and, on failure, written to `tests/repro/pending-<name>.json`
//! (rename it, fix the kernel, commit both; `tests/repro.rs` replays it forever).
//!
//! `cargo test --test sequences` runs a quick pass. The long run, seeded so shards can run
//! in parallel: `FUZZ_CASES=6250 FUZZ_SEED=1 cargo test --release --test sequences --
//! --ignored long_fuzz --nocapture` (see docs/DEVELOPMENT.md for the 8-shard recipe).

use std::path::PathBuf;

use glam::{DVec2, DVec3};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use sawhorse_kernel::Document;
use sawhorse_kernel::cutlist::{Grain, PartRef};
use sawhorse_kernel::model::{Op, SceneDef};
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::testing::{check_sequence, write_repro};
use sawhorse_kernel::transform::Xform;

fn grid() -> impl Strategy<Value = f64> {
    prop_oneof![
        6 => (-8i32..8).prop_map(|k| f64::from(k) * 0.25),
        2 => -3.0..3.0f64,
        1 => Just(1e-6),
        1 => Just(0.25 + 4e-6), // inside POINT_MERGE of a grid line
    ]
}

fn distance() -> impl Strategy<Value = f64> {
    prop_oneof![
        6 => (-8i32..8).prop_map(|k| f64::from(k) * 0.25),
        3 => -3.0..3.0f64,
        2 => (-12i32..-4).prop_map(|k| f64::from(k) * 0.5), // push through
        1 => Just(0.0),
        1 => Just(5e-6),
        1 => Just(2e4),
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
    ]
}

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        3 => Just([0.0, 0.0, 1.0]),
        1 => Just([1.0, 0.0, 0.0]),
        1 => Just([0.0, 1.0, -1.0]),
        1 => Just([0.0, 0.0, 0.0]),
        2 => (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64).prop_map(|(x, y, z)| [x, y, z]),
    ]
}

/// Eighths in (0, 1): positions along edges and across faces.
fn frac() -> impl Strategy<Value = f64> {
    prop_oneof![4 => (1i32..8).prop_map(|k| f64::from(k) / 8.0), 1 => 0.0..1.0f64]
}

/// An op intent; `pick` fields choose among what exists (modulo the count).
#[derive(Debug, Clone)]
enum Fuzz {
    NewSketch([f64; 3], [f64; 3]),
    Rect(u8, f64, f64, f64, f64),
    Segment(u8, [f64; 2], [f64; 2]),
    Erase(u8, [f64; 2], [f64; 2]),
    Polyline(u8, Vec<[f64; 2]>, bool),
    Extrude(u8, u8, f64),
    Push(u8, u8, f64),
    /// Push/pull leaving a seam (Ctrl).
    PushNew(u8, u8, f64),
    /// Guides and dimensions (annotations): add, erase by pick, delete all.
    Guide([f64; 3], Option<[f64; 3]>),
    Dimension([f64; 3], [f64; 3]),
    EraseGuide(u8),
    EraseDimension(u8),
    Split {
        object: u8,
        face: u8,
        e1: u8,
        t1: f64,
        e2: u8,
        t2: f64,
        bend: f64,
    },
    Imprint {
        object: u8,
        face: u8,
        at: [f64; 2],
        size: [f64; 2],
        triangle: bool,
    },
    /// Erase edge `edge` of face `face` of an object (mostly refused unless it was drawn).
    EraseEdge(u8, u8, u8),
    EraseFace(u8, u8),
    EraseRegion(u8, u8),
    /// A stroke over a face's plane, possibly crossing its edges or hanging off it: corners
    /// at fractions of the face's bounding box (outside 0..1 means past the edge).
    DrawFace {
        object: u8,
        face: u8,
        at: [f64; 2],
        size: [f64; 2],
        closed: bool,
    },
    /// A stroke on a horizontal plane at height `z` (find-or-create its sketch).
    DrawPlane(f64, Vec<[f64; 2]>, bool),
    /// Two simple intents resolved against the same state, applied as one batch.
    Batch(Vec<Fuzz>),
    Delete(u8),
    Group(Vec<u8>),
    Tag(u8, Option<u8>),
    TagVisible(u8, bool),
    Undo,
    Redo,
    /// Raw op with whatever ids it has (mostly refused).
    Raw(Box<Op>),
    /// Lane 4: move / turn / scale / mirror a solid (or two).
    Transform(u8, u8, XformPick),
    Copy(u8, XformPick),
    Array(u8, XformPick, u8),
    /// Lane 9: add or replace a stock (kind, name pick, length / width / thickness picks,
    /// no grain, the stock it replaces).
    DefineStock(bool, u8, [u8; 3], bool, Option<u8>),
    DeleteStock(u8),
    /// A material's default stock (material pick, stock pick or automatic).
    MaterialStock(u8, Option<u8>),
    /// Kerf, trim (picks) and the algorithm.
    NestSettings(u8, u8, bool),
    ResetStock,
    /// Lane 15: count a part or drop it from the list.
    Include(u8, bool),
    /// Lane 15: import a box as a polygon soup (shape: 0 closed, 1 no top, 2 no top or
    /// bottom, 3 a closed box with a 50 micrometre gap the cut list's heal closes), in a
    /// group or loose, at a grid point, sized by the picks.
    ImportPart(u8, bool, [u8; 3], [u8; 3]),
    /// Lane 15: Group loose faces (all of them, or one mesh by pick).
    GroupLoose(Option<u8>),
    /// Move part of a solid: a face's vertices (which 0), an edge (1) or a vertex (2) of
    /// face `face`, by the pick, with or without autofold.
    MoveGeometry {
        object: u8,
        face: u8,
        which: u8,
        index: u8,
        xform: XformPick,
        autofold: bool,
    },
    /// Offset a face (kind 0), a sketch region (1) or an edge chain of a face (2).
    Offset {
        kind: u8,
        owner: u8,
        part: u8,
        edges: u8,
        distance: f64,
    },
    /// Follow Me: a face profile along a path (0 straight along the normal, 1 an L, 2 round
    /// another face's edge), or a region profile along its plane's normal then sideways.
    FollowMe {
        region: bool,
        owner: u8,
        part: u8,
        path: u8,
        other: u8,
        a: f64,
        b: f64,
    },
    /// Lane 5. Items are picked among what the open context holds (solids and nodes).
    MakeGroup(Vec<u8>),
    MakeComponent(Vec<u8>),
    MakeUnique(u8),
    Explode(u8),
    DeleteNode(u8),
    /// Open a node (any node in the model), or the model (`None`).
    Enter(Option<u8>),
    Leave,
    NodeTransform(u8, XformPick),
    NodeCopy(u8, XformPick),
    NodeArray(u8, XformPick, u8),
    Hide(u8, bool),
    Lock(u8, bool),
    TagItems(u8, Option<u8>),
    CreateTag(u8, Option<[u8; 3]>),
    RenameTag(u8, u8),
    DeleteTag(u8),
    TagColor(u8, Option<[u8; 3]>),
    /// Scenes: 0 add, 1 update, 2 delete, 3 activate; name index; hidden-tag mask; open
    /// node (or the model).
    Scene(u8, u8, u8, Option<u8>),
    Rename(u8, Option<u8>),
    /// Lane 7: grain (0 along L, 1 across, 2 none) on a part the cut list lists (or a solid,
    /// group or definition picked anywhere, or an unknown id).
    Grain(u8, u8),
    /// Lane 7: stock (a name index, or none) and sheet / lumber / decide.
    Stock(u8, Option<u8>, Option<bool>),
    /// Lane 10: define material `name` (an index into MATERIALS): colour, a texture image
    /// (kind 0 none, 1 an image the op adds, 2 a file-name reference, 3 a path never added),
    /// a size pick, a rotation and a stock link (0 none, 1 lumber, 2 sheet).
    DefineMat {
        name: u8,
        color: [u8; 4],
        texture: u8,
        size: u8,
        rotation: f64,
        stock: u8,
    },
    /// Paint faces (a bit mask over the k-th solid's faces) with a material (or take it off).
    PaintFaces(u8, u8, Option<u8>),
    /// Paint items of the open context (or anything, with a high pick) with a material.
    PaintItems(Vec<u8>, Option<u8>),
    /// Position a texture on a face: scale, rotation, offset (garbage on purpose), or clear.
    FaceUv(u8, u8, Option<(f64, f64, f64, f64)>),
    RenameMat(u8, u8),
    DeleteMat(u8),
    /// A style (index into a few) or none; shadows on / off at a time, or none.
    Style(Option<u8>),
    Shadows(Option<(bool, u16, f64)>),
    /// Lane 11: a plugin's named batch with placeholders (resolved by `refbatch`, not by
    /// `resolve`).
    PluginBatch(Vec<PluginStep>),
    /// Lane 12: a section plane (origin, normal, active).
    AddSection([f64; 3], [f64; 3], bool),
    /// Put plane k elsewhere.
    PlaceSection(u8, [f64; 3], [f64; 3]),
    /// Plane k: 0 flip, 1 (de)activate, 2 hide / show, 3 rename, 4 delete, 5 slice to a group.
    SectionAct(u8, u8, bool),
    MoveSections(Vec<u8>, XformPick),
    CopySections(Vec<u8>, XformPick, u8),
    /// Lane 12: a sheet with setup variant k.
    AddSheet(u8),
    SetSheet(u8, u8),
    DeleteSheet(u8),
    MoveSheet(u8, u8),
    AddSheetItem(u8, SheetPick),
    SetSheetItem(u8, u8, SheetPick),
    MoveSheetItems(u8, Vec<u8>, [f64; 2]),
    DeleteSheetItems(u8, Vec<u8>),
    OrderSheetItem(u8, u8, u8),
}

/// A sheet item, as an intent (lane 12): `pick` fields choose among what exists.
#[derive(Debug, Clone)]
enum SheetPick {
    /// Source (0 a scene, 1 a standard view, 2 a parallel camera, 3 a perspective one, 4 a
    /// scene that does not exist), scale (0 NTS, 1-4 standard, 5 out of range), rect.
    Viewport(u8, u8, u8, [f64; 4], bool),
    /// Viewport pick, kind, model points, offset.
    Dimension(u8, u8, [[f64; 3]; 3], f64),
    /// Where, text variant, size, leader (0 none, 1 paper, 2 a viewport's model point).
    Label([f64; 2], u8, f64, u8),
    /// Rect, columns (bits), decimal, visible only.
    Table([f64; 4], u16, bool, bool),
    /// Rect, an image the model holds or not.
    Image([f64; 4], u8),
    Line([f64; 2], [f64; 2], f64, bool),
    Rect([f64; 4], f64, bool),
}

/// One step of a plugin batch (lane 11). Steps that name "what step k made" use a
/// placeholder to the op that made it; with nothing made yet they fall back to an id in the
/// document.
#[derive(Debug, Clone)]
enum PluginStep {
    /// A rectangle on the ground sketch and the extrusion of the region it made.
    GroundBox(f64, f64, f64, f64, f64),
    /// A new level sketch at a height, a rectangle in it, its region extruded (three
    /// placeholders: the sketch twice, the region).
    SketchBox([f64; 3], [f64; 2], f64),
    /// A box as an import item (a polygon soup), as `batch.box` sends it.
    ImportBox([f64; 3], [f64; 3]),
    /// Rename what step k made.
    Rename(u8),
    /// Make a component of what step k made.
    Component(u8),
    /// Push face `face` (a raw persistent id) of what step k made.
    Push(u8, u8, f64),
    /// Move what step k made.
    Move(u8, [f64; 3]),
    /// An ordinary intent, resolved against the document as it was before the batch.
    Plain(Box<Fuzz>),
    /// Garbage on purpose: 0 a placeholder to a later op, 1 a bad path, 2 a forbidden op,
    /// 3 an unknown op, 4 a placeholder that is not an id, 5 extra fields in a placeholder.
    Garbage(u8),
}

/// A transform, as an intent.
#[derive(Debug, Clone)]
enum XformPick {
    Translate([f64; 3]),
    /// About axis 0/1/2 (or a skew axis for 3) through a point, by degrees.
    Rotate(u8, f64, [f64; 3]),
    Scale([f64; 3], [f64; 3]),
    /// Garbage on purpose (NaN, huge, singular).
    Bad(u8),
}

fn xform_of(x: &XformPick) -> Xform {
    match x {
        XformPick::Translate(d) => Xform::translation(DVec3::from_array(*d)),
        XformPick::Rotate(axis, deg, about) => {
            let a = match axis % 4 {
                0 => DVec3::X,
                1 => DVec3::Y,
                2 => DVec3::Z,
                _ => DVec3::new(0.3, -0.5, 0.8),
            };
            Xform::rotation(DVec3::from_array(*about), a, deg.to_radians())
        }
        XformPick::Scale(f, about) => {
            Xform::scaling(DVec3::from_array(*about), DVec3::from_array(*f))
        }
        XformPick::Bad(k) => {
            let mut x = Xform::IDENTITY;
            match k % 3 {
                0 => x.translation[1] = f64::NAN,
                1 => x.translation[0] = 2e4,
                _ => x.linear[2][2] = 0.0,
            }
            x
        }
    }
}

fn xform_pick() -> impl Strategy<Value = XformPick> {
    let v = || (grid(), grid(), grid()).prop_map(|(x, y, z)| [x, y, z]);
    let factor = || {
        prop_oneof![
            4 => (1i32..9).prop_map(|k| f64::from(k) * 0.25),
            2 => -2.0..2.0f64,
            1 => Just(-1.0),
            1 => Just(0.0),
            1 => Just(1e-6),
        ]
    };
    prop_oneof![
        4 => v().prop_map(XformPick::Translate),
        3 => (0u8..4, prop_oneof![3 => (-8i32..8).prop_map(|k| f64::from(k) * 45.0), 1 => -360.0..360.0f64], v())
            .prop_map(|(a, d, o)| XformPick::Rotate(a, d, o)),
        2 => ((factor(), factor(), factor()), v()).prop_map(|((x, y, z), o)| XformPick::Scale([x, y, z], o)),
        1 => any::<u8>().prop_map(XformPick::Bad),
    ]
}

/// Lane-4 intents.
fn lane4_op() -> impl Strategy<Value = Fuzz> {
    prop_oneof![
        3 => (any::<u8>(), any::<u8>(), xform_pick()).prop_map(|(o, p, x)| Fuzz::Transform(o, p, x)),
        2 => (any::<u8>(), xform_pick()).prop_map(|(o, x)| Fuzz::Copy(o, x)),
        1 => (any::<u8>(), xform_pick(), 0u8..5).prop_map(|(o, x, n)| Fuzz::Array(o, x, n)),
        3 => (any::<u8>(), any::<u8>(), 0u8..3, any::<u8>(), xform_pick(), any::<bool>())
            .prop_map(|(object, face, which, index, xform, autofold)| Fuzz::MoveGeometry { object, face, which, index, xform, autofold }),
        3 => (0u8..3, any::<u8>(), any::<u8>(), 1u8..4, distance())
            .prop_map(|(kind, owner, part, edges, distance)| Fuzz::Offset { kind, owner, part, edges, distance: distance * 0.25 }),
        3 => (any::<bool>(), any::<u8>(), any::<u8>(), 0u8..3, any::<u8>(), distance(), distance())
            .prop_map(|(region, owner, part, path, other, a, b)| Fuzz::FollowMe { region, owner, part, path, other, a, b }),
    ]
}

/// Lane-5 intents.
fn lane5_op() -> impl Strategy<Value = Fuzz> {
    let picks = || prop::collection::vec(any::<u8>(), 1..4);
    let color = || {
        prop::option::of((any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| [r, g, b]))
    };
    prop_oneof![
        3 => picks().prop_map(Fuzz::MakeGroup),
        4 => picks().prop_map(Fuzz::MakeComponent),
        2 => any::<u8>().prop_map(Fuzz::MakeUnique),
        2 => any::<u8>().prop_map(Fuzz::Explode),
        1 => any::<u8>().prop_map(Fuzz::DeleteNode),
        5 => prop::option::of(any::<u8>()).prop_map(Fuzz::Enter),
        3 => Just(Fuzz::Leave),
        2 => (any::<u8>(), xform_pick()).prop_map(|(n, x)| Fuzz::NodeTransform(n, x)),
        3 => (any::<u8>(), xform_pick()).prop_map(|(n, x)| Fuzz::NodeCopy(n, x)),
        2 => (any::<u8>(), xform_pick(), 0u8..4).prop_map(|(n, x, c)| Fuzz::NodeArray(n, x, c)),
        1 => (any::<u8>(), any::<bool>()).prop_map(|(i, b)| Fuzz::Hide(i, b)),
        1 => (any::<u8>(), any::<bool>()).prop_map(|(i, b)| Fuzz::Lock(i, b)),
        1 => (any::<u8>(), prop::option::of(0u8..4)).prop_map(|(i, t)| Fuzz::TagItems(i, t)),
        1 => (0u8..5, color()).prop_map(|(t, c)| Fuzz::CreateTag(t, c)),
        1 => (0u8..5, 0u8..5).prop_map(|(a, b)| Fuzz::RenameTag(a, b)),
        1 => (0u8..5).prop_map(Fuzz::DeleteTag),
        1 => (0u8..5, color()).prop_map(|(t, c)| Fuzz::TagColor(t, c)),
        2 => (0u8..4, 0u8..3, any::<u8>(), prop::option::of(any::<u8>())).prop_map(|(k, n, h, c)| Fuzz::Scene(k, n, h, c)),
        1 => (any::<u8>(), prop::option::of(0u8..3)).prop_map(|(i, n)| Fuzz::Rename(i, n)),
        2 => (any::<u8>(), 0u8..3).prop_map(|(k, g)| Fuzz::Grain(k, g)),
        2 => (any::<u8>(), prop::option::of(0u8..5), prop::option::of(any::<bool>())).prop_map(|(k, t, s)| Fuzz::Stock(k, t, s)),
        2 => (any::<bool>(), any::<u8>(), any::<[u8; 3]>(), any::<bool>(), prop::option::of(any::<u8>()))
            .prop_map(|(sheet, n, d, none, r)| Fuzz::DefineStock(sheet, n, d, none, r)),
        1 => any::<u8>().prop_map(Fuzz::DeleteStock),
        2 => (any::<u8>(), prop::option::of(any::<u8>())).prop_map(|(m, s)| Fuzz::MaterialStock(m, s)),
        2 => (any::<u8>(), any::<u8>(), any::<bool>()).prop_map(|(k, t, a)| Fuzz::NestSettings(k, t, a)),
        1 => Just(Fuzz::ResetStock),
        2 => (any::<u8>(), any::<bool>()).prop_map(|(k, b)| Fuzz::Include(k, b)),
        2 => (0u8..4, any::<bool>(), any::<[u8; 3]>(), any::<[u8; 3]>()).prop_map(|(s, g, a, z)| Fuzz::ImportPart(s, g, a, z)),
        1 => prop::option::of(any::<u8>()).prop_map(Fuzz::GroupLoose),
    ]
}

fn lane10_op() -> impl Strategy<Value = Fuzz> {
    let mat = || prop::option::of(0u8..6);
    let garbage =
        || prop_oneof![8 => -5.0..5.0f64, 1 => Just(f64::NAN), 1 => Just(1e9), 1 => Just(0.0)];
    prop_oneof![
        3 => (0u8..6, any::<[u8; 4]>(), 0u8..4, any::<u8>(), garbage(), 0u8..3)
            .prop_map(|(name, color, texture, size, rotation, stock)| Fuzz::DefineMat { name, color, texture, size, rotation, stock }),
        6 => (any::<u8>(), any::<u8>(), mat()).prop_map(|(o, f, m)| Fuzz::PaintFaces(o, f, m)),
        4 => (prop::collection::vec(any::<u8>(), 1..3), mat()).prop_map(|(i, m)| Fuzz::PaintItems(i, m)),
        3 => (any::<u8>(), any::<u8>(), prop::option::of((garbage(), garbage(), garbage(), garbage())))
            .prop_map(|(o, f, u)| Fuzz::FaceUv(o, f, u)),
        1 => (0u8..6, 0u8..6).prop_map(|(a, b)| Fuzz::RenameMat(a, b)),
        1 => (0u8..6).prop_map(Fuzz::DeleteMat),
        1 => prop::option::of(0u8..4).prop_map(Fuzz::Style),
        1 => prop::option::of((any::<bool>(), 0u16..1500, -100.0..100.0f64)).prop_map(Fuzz::Shadows),
    ]
}

/// Paper coordinates: mostly on a page, sometimes off it or garbage.
fn paper() -> impl Strategy<Value = f64> {
    prop_oneof![
        14 => (0i32..40).prop_map(|k| f64::from(k) * 0.00635),
        3 => -0.05..0.5f64,
        1 => Just(3.0),
        1 => Just(f64::NAN),
    ]
}

fn paper_rect() -> impl Strategy<Value = [f64; 4]> {
    let size = || prop_oneof![14 => (1i32..30).prop_map(|k| f64::from(k) * 0.00635), 1 => Just(0.0), 1 => Just(-0.01), 1 => Just(5.0)];
    (paper(), paper(), size(), size()).prop_map(|(x, y, w, h)| [x, y, w, h])
}

fn sheet_pick() -> impl Strategy<Value = SheetPick> {
    let p3 = || (grid(), grid(), grid()).prop_map(|(x, y, z)| [x, y, z]);
    prop_oneof![
        5 => (0u8..5, 0u8..6, any::<u8>(), paper_rect(), any::<bool>()).prop_map(|(s, k, v, r, h)| SheetPick::Viewport(s, k, v, r, h)),
        5 => (any::<u8>(), 0u8..7, (p3(), p3(), p3()), prop_oneof![4 => -0.03..0.03f64, 1 => Just(f64::NAN)])
            .prop_map(|(v, k, (a, b, c), o)| SheetPick::Dimension(v, k, [a, b, c], o)),
        3 => ((paper(), paper()), 0u8..4, prop_oneof![4 => Just(0.003), 1 => Just(0.0), 1 => Just(0.5)], 0u8..3)
            .prop_map(|((x, y), t, z, l)| SheetPick::Label([x, y], t, z, l)),
        2 => (paper_rect(), any::<u16>(), any::<bool>(), any::<bool>()).prop_map(|(r, c, d, v)| SheetPick::Table(r, c, d, v)),
        1 => (paper_rect(), 0u8..3).prop_map(|(r, k)| SheetPick::Image(r, k)),
        2 => ((paper(), paper()), (paper(), paper()), prop_oneof![3 => Just(0.00035), 1 => Just(0.0), 1 => Just(0.02)], any::<bool>())
            .prop_map(|((ax, ay), (bx, by), w, d)| SheetPick::Line([ax, ay], [bx, by], w, d)),
        2 => (paper_rect(), prop_oneof![3 => Just(0.00035), 1 => Just(-1.0)], any::<bool>()).prop_map(|(r, w, f)| SheetPick::Rect(r, w, f)),
    ]
}

/// Lane-12 intents: section planes and drawing sheets.
fn lane12_op() -> impl Strategy<Value = Fuzz> {
    let p3 = || (grid(), grid(), grid()).prop_map(|(x, y, z)| [x, y, z]);
    let picks = || prop::collection::vec(any::<u8>(), 0..3);
    prop_oneof![
        4 => (p3(), normal(), any::<bool>()).prop_map(|(o, n, a)| Fuzz::AddSection(o, n, a)),
        2 => (any::<u8>(), p3(), normal()).prop_map(|(k, o, n)| Fuzz::PlaceSection(k, o, n)),
        4 => (any::<u8>(), 0u8..6, any::<bool>()).prop_map(|(k, a, b)| Fuzz::SectionAct(k, a, b)),
        2 => (picks(), xform_pick()).prop_map(|(k, x)| Fuzz::MoveSections(k, x)),
        1 => (picks(), xform_pick(), 0u8..4).prop_map(|(k, x, n)| Fuzz::CopySections(k, x, n)),
        3 => any::<u8>().prop_map(Fuzz::AddSheet),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(k, v)| Fuzz::SetSheet(k, v)),
        1 => any::<u8>().prop_map(Fuzz::DeleteSheet),
        1 => (any::<u8>(), 0u8..5).prop_map(|(k, t)| Fuzz::MoveSheet(k, t)),
        8 => (any::<u8>(), sheet_pick()).prop_map(|(k, i)| Fuzz::AddSheetItem(k, i)),
        3 => (any::<u8>(), any::<u8>(), sheet_pick()).prop_map(|(k, i, p)| Fuzz::SetSheetItem(k, i, p)),
        2 => (any::<u8>(), picks(), (paper(), paper())).prop_map(|(k, i, (x, y))| Fuzz::MoveSheetItems(k, i, [x - 0.1, y - 0.1])),
        1 => (any::<u8>(), picks()).prop_map(|(k, i)| Fuzz::DeleteSheetItems(k, i)),
        1 => (any::<u8>(), any::<u8>(), 0u8..9).prop_map(|(k, i, t)| Fuzz::OrderSheetItem(k, i, t)),
    ]
}

/// A sheet setup from a variant: every paper, both orientations and units, every length
/// format; some refused (no name, margins that leave no room, a grid too coarse, a title
/// block field too long, a custom paper too small, a fraction that is not a power of two).
fn setup_of(k: u8) -> sawhorse_kernel::sheets::SheetSetup {
    use sawhorse_kernel::paper::LengthFormat;
    use sawhorse_kernel::sheets::{PaperSize, PaperUnits, SheetSetup};
    let mut s = SheetSetup::named(&format!("Sheet {}", k % 5));
    s.paper = match k % 7 {
        0 => PaperSize::Letter,
        1 => PaperSize::Legal,
        2 => PaperSize::Tabloid,
        3 => PaperSize::A4,
        4 => PaperSize::A3,
        5 => PaperSize::A2,
        _ => PaperSize::Custom {
            width: if k.is_multiple_of(3) { 0.01 } else { 0.3 },
            height: 0.2,
        },
    };
    s.landscape = k & 8 != 0;
    if k & 16 != 0 {
        s.units = PaperUnits::Millimetres;
    }
    s.format = [
        LengthFormat::Fraction { den: 16 },
        LengthFormat::Decimal { places: 2 },
        LengthFormat::FeetInches { den: 8 },
        LengthFormat::Millimetres { places: 1 },
    ][(k / 7) as usize % 4];
    if k == 97 {
        s.format = LengthFormat::Fraction { den: 3 };
    }
    match k % 23 {
        0 => s.name = String::new(),
        1 => s.margins = [0.5; 4],
        2 => s.grid = 0.2,
        3 => s.title.project = "x".repeat(600),
        4 => s.title.show = false,
        5 => s.grid = 0.0,
        _ => {}
    }
    s.title.title = format!("Title {}", k % 3);
    s
}

fn fuzz_op() -> impl Strategy<Value = Fuzz> {
    let pt = || (grid(), grid()).prop_map(|(x, y)| [x, y]);
    prop_oneof![
        1 => (prop_oneof![Just([0.0; 3]), (grid(), grid(), grid()).prop_map(|(x, y, z)| [x, y, z])], normal())
            .prop_map(|(o, n)| Fuzz::NewSketch(o, n)),
        5 => (any::<u8>(), grid(), grid(), grid(), grid()).prop_map(|(s, x, y, w, h)| Fuzz::Rect(s, x, y, w, h)),
        4 => (any::<u8>(), pt(), pt()).prop_map(|(s, a, b)| Fuzz::Segment(s, a, b)),
        1 => (any::<u8>(), pt(), pt()).prop_map(|(s, a, b)| Fuzz::Erase(s, a, b)),
        1 => (any::<u8>(), prop::collection::vec(pt(), 2..6), any::<bool>()).prop_map(|(s, p, c)| Fuzz::Polyline(s, p, c)),
        4 => (any::<u8>(), any::<u8>(), distance()).prop_map(|(s, r, d)| Fuzz::Extrude(s, r, d)),
        6 => (any::<u8>(), any::<u8>(), distance()).prop_map(|(o, f, d)| Fuzz::Push(o, f, d)),
        3 => (any::<u8>(), any::<u8>(), any::<u8>(), frac(), any::<u8>(), frac(), prop_oneof![Just(0.0), 0.0..0.6f64])
            .prop_map(|(object, face, e1, t1, e2, t2, bend)| Fuzz::Split { object, face, e1, t1, e2, t2, bend }),
        3 => (any::<u8>(), any::<u8>(), (frac(), frac()), (frac(), frac()), any::<bool>())
            .prop_map(|(object, face, at, size, triangle)| Fuzz::Imprint { object, face, at: [at.0, at.1], size: [size.0, size.1], triangle }),
        3 => (any::<u8>(), any::<u8>(), (-0.25..1.25f64, -0.25..1.25f64), (frac(), frac()), any::<bool>())
            .prop_map(|(object, face, at, size, closed)| Fuzz::DrawFace { object, face, at: [at.0, at.1], size: [size.0, size.1], closed }),
        1 => (grid(), prop::collection::vec(pt(), 2..5), any::<bool>()).prop_map(|(z, p, c)| Fuzz::DrawPlane(z, p, c)),
        2 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(o, f, e)| Fuzz::EraseEdge(o, f, e)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(o, f)| Fuzz::EraseFace(o, f)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(s, r)| Fuzz::EraseRegion(s, r)),
        1 => prop::collection::vec(
                prop_oneof![
                    (any::<u8>(), grid(), grid(), grid(), grid()).prop_map(|(s, x, y, w, h)| Fuzz::Rect(s, x, y, w, h)),
                    (any::<u8>(), any::<u8>(), distance()).prop_map(|(o, f, d)| Fuzz::Push(o, f, d)),
                    (any::<u8>(), any::<u8>()).prop_map(|(o, f)| Fuzz::EraseFace(o, f)),
                ],
                0..3,
            )
            .prop_map(Fuzz::Batch),
        1 => any::<u8>().prop_map(Fuzz::Delete),
        1 => prop::collection::vec(any::<u8>(), 0..3).prop_map(Fuzz::Group),
        1 => (any::<u8>(), prop::option::of(0u8..3)).prop_map(|(o, t)| Fuzz::Tag(o, t)),
        1 => (0u8..3, any::<bool>()).prop_map(|(t, v)| Fuzz::TagVisible(t, v)),
        3 => Just(Fuzz::Undo),
        2 => Just(Fuzz::Redo),
        1 => (0u64..4, 0u64..6, distance()).prop_map(|(sketch, region, distance)| Fuzz::Raw(Box::new(Op::Extrude { sketch, region, distance }))),
        1 => (0u64..4, 0u64..14, distance()).prop_map(|(object, face, distance)| Fuzz::Raw(Box::new(Op::PushPull { object, face, distance }))),
        2 => (any::<u8>(), any::<u8>(), distance()).prop_map(|(o, f, d)| Fuzz::PushNew(o, f, d)),
        1 => ((grid(), grid(), grid()), prop::option::of(normal())).prop_map(|((x, y, z), d)| Fuzz::Guide([x, y, z], d)),
        1 => ((grid(), grid(), grid()), (grid(), grid(), grid())).prop_map(|(a, b)| Fuzz::Dimension([a.0, a.1, a.2], [b.0, b.1, b.2])),
        1 => any::<u8>().prop_map(Fuzz::EraseGuide),
        1 => any::<u8>().prop_map(Fuzz::EraseDimension),
        9 => lane4_op(),
        8 => lane5_op(),
        6 => lane10_op(),
        3 => prop::collection::vec(plugin_step(), 1..6).prop_map(Fuzz::PluginBatch),
        16 => lane12_op(),
    ]
}

fn plugin_step() -> impl Strategy<Value = PluginStep> {
    let size = || prop_oneof![4 => (1i32..9).prop_map(|k| f64::from(k) * 0.25), 1 => 0.05..2.0f64, 1 => Just(1e-7)];
    prop_oneof![
        3 => (grid(), grid(), size(), size(), distance()).prop_map(|(x, y, w, h, d)| PluginStep::GroundBox(x, y, w, h, d)),
        2 => ((grid(), grid(), grid()), (size(), size()), distance()).prop_map(|((x, y, z), (w, h), d)| PluginStep::SketchBox([x, y, z], [w, h], d)),
        3 => ((grid(), grid(), grid()), (size(), size(), size())).prop_map(|((x, y, z), (a, b, c))| PluginStep::ImportBox([x, y, z], [a, b, c])),
        2 => any::<u8>().prop_map(PluginStep::Rename),
        2 => any::<u8>().prop_map(PluginStep::Component),
        2 => (any::<u8>(), 1u8..9, distance()).prop_map(|(k, f, d)| PluginStep::Push(k, f, d)),
        1 => (any::<u8>(), (grid(), grid(), grid())).prop_map(|(k, (x, y, z))| PluginStep::Move(k, [x, y, z])),
        2 => prop_oneof![
            (any::<u8>(), grid(), grid(), grid(), grid()).prop_map(|(s, x, y, w, h)| Fuzz::Rect(s, x, y, w, h)),
            (any::<u8>(), any::<u8>(), distance()).prop_map(|(o, f, d)| Fuzz::Push(o, f, d)),
            (any::<u8>(), any::<u8>()).prop_map(|(o, f)| Fuzz::EraseFace(o, f)),
            any::<u8>().prop_map(Fuzz::Delete),
            (any::<u8>(), prop::option::of(0u8..3)).prop_map(|(i, n)| Fuzz::Rename(i, n)),
            prop::collection::vec(any::<u8>(), 1..3).prop_map(Fuzz::MakeGroup),
            (any::<u8>(), any::<bool>()).prop_map(|(k, h)| Fuzz::Hide(k, h)),
        ]
        .prop_map(|f| PluginStep::Plain(Box::new(f))),
        1 => (0u8..6).prop_map(PluginStep::Garbage),
    ]
}

/// An item of sheet `sheet` by pick (an id that is not there now and then).
fn sheet_item_id(m: &sawhorse_kernel::Model, sheet: u64, k: u8) -> u64 {
    let ids: Vec<u64> = m
        .sheet(sheet)
        .map(|s| s.items.iter().map(|i| i.id).collect())
        .unwrap_or_default();
    match pick(&ids, k) {
        Some(id) if k % 16 != 15 => id,
        _ => 500 + u64::from(k % 3),
    }
}

/// A sheet item from an intent, against what sheet `sheet` and the model hold now.
fn sheet_item(
    m: &sawhorse_kernel::Model,
    sheet: u64,
    p: &SheetPick,
) -> sawhorse_kernel::sheets::ItemKind {
    use sawhorse_kernel::paper::{CameraSpec, CutColumn, DimKind, Scale, StdView};
    use sawhorse_kernel::sheets::{DimStyle, ItemKind, LeaderEnd, ModelAnchor, ViewportSource};
    let viewports: Vec<u64> = m
        .sheet(sheet)
        .map(|s| {
            s.items
                .iter()
                .filter(|i| matches!(i.kind, ItemKind::Viewport { .. }))
                .map(|i| i.id)
                .collect()
        })
        .unwrap_or_default();
    let viewport = |k: u8| match pick(&viewports, k) {
        Some(id) if k % 16 != 15 => id,
        _ => 700,
    };
    match p {
        SheetPick::Viewport(src, scale, v, rect, hidden) => {
            let scenes: Vec<String> = m.scenes().iter().map(|s| s.name.clone()).collect();
            let views = [
                StdView::Top,
                StdView::Bottom,
                StdView::Front,
                StdView::Back,
                StdView::Left,
                StdView::Right,
                StdView::Iso,
            ];
            let source = match src {
                0 => match pick_ref(&scenes, *v) {
                    Some(name) => ViewportSource::Scene { name: name.clone() },
                    None => ViewportSource::Standard {
                        view: views[*v as usize % 7],
                    },
                },
                1 => ViewportSource::Standard {
                    view: views[*v as usize % 7],
                },
                2 | 3 => ViewportSource::Camera {
                    camera: CameraSpec {
                        eye: [2.0, -3.0, 1.0 + f64::from(*v % 4)],
                        target: [0.0, 0.0, 0.0],
                        up: [0.0, 0.0, 1.0],
                        parallel: *src == 2,
                        fov: 35.0,
                    },
                },
                _ => ViewportSource::Scene {
                    name: "Nowhere".into(),
                },
            };
            let scale = match scale {
                0 => Scale::Nts,
                1 => Scale::Ratio {
                    paper: 1.0,
                    model: 8.0,
                },
                2 => Scale::Ratio {
                    paper: 1.0,
                    model: 1.0,
                },
                3 => Scale::Architect {
                    inches_per_foot: 0.25,
                },
                4 => Scale::Architect {
                    inches_per_foot: 3.0,
                },
                _ => Scale::Ratio {
                    paper: 1.0,
                    model: 0.0,
                },
            };
            ItemKind::Viewport {
                rect: *rect,
                source,
                scale,
                centre: (v % 5 == 1).then_some([0.1, -0.2]),
                hidden_lines: *hidden,
                caption: (v % 3 == 0).then(|| format!("VIEW {v}")),
                border: v % 2 == 0,
            }
        }
        SheetPick::Dimension(v, k, pts, offset) => {
            let dim = [
                DimKind::Horizontal,
                DimKind::Vertical,
                DimKind::Aligned,
                DimKind::Angular,
                DimKind::Radius,
                DimKind::Diameter,
            ][*k as usize % 6];
            // Kind 6: too few points (refused).
            let n = if *k == 6 { 1 } else { dim.points() };
            // Anchors: none (an older document), on the first solid's corners (found or
            // not), or malformed (refused).
            let first = m.object_ids().first().copied().unwrap_or(1);
            let anchors = match v % 4 {
                0 | 1 => Vec::new(),
                2 => (0..n)
                    .map(|i| {
                        (i % 2 == 0).then(|| {
                            ModelAnchor::on_edge(
                                first,
                                &[1, 2 + i as u64, 5],
                                &[2, 3, 6],
                                f64::from(*k % 3) / 2.0,
                            )
                        })
                    })
                    .collect(),
                _ => vec![
                    Some(ModelAnchor {
                        object: first,
                        at: vec![3, 1],
                        to: Vec::new(),
                        t: 0.5,
                    });
                    n
                ],
            };
            ItemKind::Dimension {
                viewport: viewport(*v),
                dim,
                points: pts[..n].to_vec(),
                anchors,
                offset: *offset,
                style: [DimStyle::Arrows, DimStyle::Ticks, DimStyle::Dots][*v as usize % 3],
                text: (v % 6 == 5).then(|| "36 (overall)".to_string()),
            }
        }
        SheetPick::Label(at, t, size, leader) => ItemKind::Label {
            at: *at,
            text: [
                "Walnut",
                "Two lines\nof text",
                "",
                "\u{d8} 1 1/2\" at 45\u{b0}",
            ][*t as usize % 4]
                .to_string(),
            size: *size,
            leader: match leader {
                0 => None,
                1 => Some(LeaderEnd::Paper {
                    at: [at[0] + 0.02, at[1] - 0.01],
                }),
                _ => Some(LeaderEnd::Model {
                    viewport: viewport(*t),
                    point: [0.25, 0.0, 0.5],
                    anchor: (t % 2 == 1).then(|| {
                        ModelAnchor::on_edge(
                            m.object_ids().first().copied().unwrap_or(1),
                            &[1, 3, 5],
                            &[],
                            0.0,
                        )
                    }),
                }),
            },
        },
        SheetPick::Table(rect, cols, decimal, visible_only) => {
            let all = [
                CutColumn::Part,
                CutColumn::Qty,
                CutColumn::Thickness,
                CutColumn::Width,
                CutColumn::Length,
                CutColumn::Size,
                CutColumn::Material,
                CutColumn::Grain,
                CutColumn::Amount,
                CutColumn::Notes,
            ];
            ItemKind::Table {
                rect: *rect,
                columns: all
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| cols & (1 << i) != 0)
                    .map(|(_, c)| *c)
                    .collect(),
                decimal: *decimal,
                visible_only: *visible_only,
                title: (cols % 2 == 0).then(|| "CUT LIST".to_string()),
            }
        }
        SheetPick::Image(rect, k) => {
            let held: Vec<String> = m.textures().keys().cloned().collect();
            ItemKind::Image {
                rect: *rect,
                path: match pick_ref(&held, *k) {
                    Some(p) if *k != 2 => p.clone(),
                    _ => "assets/missing.png".into(),
                },
            }
        }
        SheetPick::Line(a, b, width, dashed) => ItemKind::Line {
            a: *a,
            b: *b,
            width: *width,
            dashed: *dashed,
        },
        SheetPick::Rect(rect, width, fill) => ItemKind::Rect {
            rect: *rect,
            width: *width,
            fill: fill.then_some([235, 235, 230]),
        },
    }
}

/// A placeholder: the value at `path` in the outcome of op `at` of the batch.
fn placeholder(at: usize, path: &str) -> serde_json::Value {
    serde_json::json!({ "$ref": at, "path": path })
}

/// The plugin batch request for some steps, as a plugin would build it against `d`.
fn plugin_batch(steps: &[PluginStep], d: &Document) -> sawhorse_kernel::refbatch::NamedBatch {
    use serde_json::json;
    let mut ops: Vec<serde_json::Value> = Vec::new();
    // What earlier steps made: a placeholder to a solid, and whether it is a node.
    let mut made: Vec<(serde_json::Value, bool)> = Vec::new();
    let objects = d.model().object_ids();
    let thing = |made: &Vec<(serde_json::Value, bool)>, k: u8| -> (serde_json::Value, bool) {
        if made.is_empty() {
            (
                json!(
                    objects
                        .get(k as usize % objects.len().max(1))
                        .copied()
                        .unwrap_or(4242)
                ),
                false,
            )
        } else {
            made[k as usize % made.len()].clone()
        }
    };
    let item = |(v, node): (serde_json::Value, bool)| {
        if node {
            json!({ "node": v })
        } else {
            json!({ "object": v })
        }
    };
    for step in steps {
        match step {
            PluginStep::GroundBox(x, y, w, h, dist) => {
                ops.push(json!({ "op": "add_rectangle", "sketch": 1, "x": x, "y": y, "w": w, "h": h }));
                let rect = ops.len() - 1;
                ops.push(json!({ "op": "extrude", "sketch": 1, "region": placeholder(rect, "regions.0"), "distance": dist }));
                made.push((placeholder(ops.len() - 1, "object"), false));
            }
            PluginStep::SketchBox(at, [w, h], dist) => {
                ops.push(json!({ "op": "new_sketch", "origin": at, "normal": [0.0, 0.0, 1.0] }));
                let sk = ops.len() - 1;
                ops.push(json!({ "op": "add_rectangle", "sketch": placeholder(sk, "sketch"), "x": 0.0, "y": 0.0, "w": w, "h": h }));
                let rect = ops.len() - 1;
                ops.push(json!({ "op": "extrude", "sketch": placeholder(sk, "sketch"), "region": placeholder(rect, "regions.0"), "distance": dist }));
                made.push((placeholder(ops.len() - 1, "object"), false));
            }
            PluginStep::ImportBox(min, size) => {
                let p = |i: usize| -> [f64; 3] {
                    [min[0] + if i & 1 != 0 { size[0] } else { 0.0 }, min[1] + if i & 2 != 0 { size[1] } else { 0.0 }, min[2] + if i & 4 != 0 { size[2] } else { 0.0 }]
                };
                ops.push(json!({ "op": "import", "items": [{
                    "name": "Plugin box",
                    "positions": (0..8).map(p).collect::<Vec<_>>(),
                    "faces": [[[0, 2, 3, 1]], [[4, 5, 7, 6]], [[0, 1, 5, 4]], [[2, 6, 7, 3]], [[0, 4, 6, 2]], [[1, 3, 7, 5]]],
                }] }));
                made.push((placeholder(ops.len() - 1, "objects.0"), false));
            }
            PluginStep::Rename(k) => ops.push(json!({ "op": "rename", "item": item(thing(&made, *k)), "name": format!("Part {k}") })),
            PluginStep::Component(k) => {
                ops.push(json!({ "op": "make_component", "items": [item(thing(&made, *k))], "name": format!("Plugin part {k}") }));
                made.push((placeholder(ops.len() - 1, "instance"), true));
            }
            PluginStep::Push(k, face, dist) => {
                let (v, node) = thing(&made, *k);
                if !node {
                    ops.push(json!({ "op": "push_pull", "object": v, "face": face, "distance": dist }));
                }
            }
            PluginStep::Move(k, t) => {
                let (v, node) = thing(&made, *k);
                let xform = json!({ "linear": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "translation": t });
                ops.push(if node {
                    json!({ "op": "transform", "objects": [], "nodes": [v], "xform": xform })
                } else {
                    json!({ "op": "transform", "objects": [v], "xform": xform })
                });
            }
            PluginStep::Plain(f) => {
                let op = resolve(f, d);
                ops.push(serde_json::to_value(&op).unwrap_or(json!({ "op": "delete_guides" })));
            }
            PluginStep::Garbage(kind) => {
                let at = ops.len();
                let forbidden = ["undo", "redo", "leave_context"][at % 3];
                ops.push(match kind {
                    0 => json!({ "op": "rename", "item": { "object": placeholder(at + 1, "object") }, "name": "Later" }),
                    1 => json!({ "op": "rename", "item": { "object": placeholder(at.saturating_sub(1), "no.such.path") }, "name": "Nope" }),
                    2 => json!({ "op": forbidden }),
                    3 => json!({ "op": "frobnicate", "object": 1 }),
                    4 => json!({ "op": "rename", "item": { "object": placeholder(at.saturating_sub(1), "") }, "name": "Whole outcome" }),
                    _ => json!({ "op": "rename", "item": { "object": { "$ref": 0, "path": "object", "extra": 1 } }, "name": "Extra" }),
                });
            }
        }
    }
    if ops.is_empty() {
        ops.push(json!({ "op": "delete_guides" }));
    }
    sawhorse_kernel::refbatch::NamedBatch {
        name: "Fuzz plugin".into(),
        ops,
    }
}

fn pick<T: Copy>(items: &[T], k: u8) -> Option<T> {
    (!items.is_empty()).then(|| items[k as usize % items.len()])
}

fn pick_ref<T>(items: &[T], k: u8) -> Option<&T> {
    (!items.is_empty()).then(|| &items[k as usize % items.len()])
}

const TAGS: [&str; 3] = ["Legs", "Top", "Jig"];
/// Stock names the lane-7 intents set (one reads as sheet stock; one is blank; one too long).
const STOCKS: [&str; 5] = [
    "White oak",
    "3/4 Baltic birch ply",
    "  ",
    "a stock name far longer than any name may be, sixty-four characters",
    // Lane 9: a part naming a library stock is cut from it.
    "4 x 8 MDF, 3/4",
];
/// Stock names the lane-9 intents define, replace, delete and default to: built-in ones,
/// new ones, a blank and an over-long one.
const STOCK_NAMES: [&str; 8] = [
    "4 x 8 sheet, 3/4",
    "4/4 x 12 x 96",
    "5 x 5 Baltic birch, 1/2",
    "Shop sheet",
    "Short board",
    "SHOP SHEET",
    " ",
    "a stock name far longer than any name may be, sixty-four characters",
];
/// Stock dimensions (metres): sheet and board sizes, tiny, huge, broken.
const STOCK_SIZES: [f64; 12] = [
    0.0254 * 0.25,
    0.0254 * 0.75,
    0.0254 * 4.0,
    0.0254 * 12.0,
    0.0254 * 48.0,
    0.0254 * 96.0,
    0.3,
    1.5,
    0.0005,
    12.0,
    -1.0,
    f64::NAN,
];
/// Kerfs and trims (metres): none, 1/8 in, 3/32 in, 1/2 in, too wide, negative, NaN.
const NEST_LENGTHS: [f64; 8] = [
    0.0,
    0.0254 / 8.0,
    0.0254 * 3.0 / 32.0,
    0.0254 / 2.0,
    0.02,
    0.2,
    -0.001,
    f64::NAN,
];
/// Materials the lane-10 intents define, paint with, rename and delete (one name blank, one
/// too long: refused).
const MATERIALS: [&str; 6] = [
    "White oak",
    "Birch ply",
    "Red",
    "Walnut",
    " ",
    "a material name far longer than any name may be, sixty-four chars",
];
/// Tags the lane-5 intents create, rename, colour and delete (the first two shared with
/// the older intents, so renames and deletes hit tags in use).
const NODE_TAGS: [&str; 5] = ["Legs", "Top", "Carcass", "Shelves", "Untagged"];

/// Turn an intent into a concrete op against the current document.
fn resolve(f: &Fuzz, d: &Document) -> Op {
    let m = d.model();
    let sketches = m.sketch_ids();
    let objects = m.object_ids();
    // Mostly what the open context offers (so edits inside components happen), sometimes
    // anything (so refusals of what is outside it are exercised too).
    let (ctx_sketches, ctx_objects) = (m.context_sketches(), m.context_objects());
    let sketch = |k: u8| {
        if !k.is_multiple_of(8)
            && let Some(s) = pick(&ctx_sketches, k)
        {
            return s;
        }
        pick(&sketches, k).unwrap_or(1)
    };
    let object = |k: u8| {
        if !k.is_multiple_of(8)
            && let Some(o) = pick(&ctx_objects, k)
        {
            return o;
        }
        pick(&objects, k).unwrap_or(1)
    };
    // Items of the open context: its solids, then its groups and instances.
    let ctx_items: Vec<Item> = ctx_objects
        .iter()
        .map(|&o| Item::Object(o))
        .chain(m.context_nodes().into_iter().map(Item::Node))
        .collect();
    let item = |k: u8| pick(&ctx_items, k).unwrap_or(Item::Object(1));
    // Lane 12: section planes and sheets by pick (an id that does not exist now and then).
    let section_ids: Vec<u64> = m.sections().iter().map(|(id, _)| *id).collect();
    let section = |k: u8| match pick(&section_ids, k) {
        Some(id) if k % 16 != 15 => id,
        _ => 90 + u64::from(k % 3),
    };
    let sheet_ids: Vec<u64> = m.sheets().iter().map(|(id, _)| *id).collect();
    let sheet = |k: u8| match pick(&sheet_ids, k) {
        Some(id) if k % 16 != 15 => id,
        _ => 90 + u64::from(k % 3),
    };
    // An edit of a section plane, a sheet or a sheet's item when there is none makes one
    // first (so those edits run often), except now and then (to keep the refusals).
    let needs = match f {
        Fuzz::PlaceSection(k, ..) | Fuzz::SectionAct(k, ..) => Some((0, *k)),
        Fuzz::MoveSections(k, _) | Fuzz::CopySections(k, ..) => {
            Some((0, k.first().copied().unwrap_or(0)))
        }
        Fuzz::SetSheet(k, _)
        | Fuzz::DeleteSheet(k)
        | Fuzz::MoveSheet(k, _)
        | Fuzz::AddSheetItem(k, _) => Some((1, *k)),
        Fuzz::SetSheetItem(k, ..)
        | Fuzz::MoveSheetItems(k, ..)
        | Fuzz::DeleteSheetItems(k, _)
        | Fuzz::OrderSheetItem(k, ..) => Some((2, *k)),
        _ => None,
    };
    if let Some((what, k)) = needs
        && k % 16 != 15
    {
        if what == 0 && section_ids.is_empty() {
            return Op::AddSection {
                origin: [0.25 * f64::from(k % 5), 0.25, 0.5],
                normal: [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]][k as usize % 3],
                name: None,
                active: true,
                hidden: false,
            };
        }
        if what > 0 && sheet_ids.is_empty() {
            return Op::AddSheet {
                setup: Box::new(sawhorse_kernel::sheets::SheetSetup::named("Sheet")),
            };
        }
        // A dimension or a label on a model point wants a viewport to be on.
        let wants_viewport = matches!(
            f,
            Fuzz::AddSheetItem(_, SheetPick::Dimension(..))
                | Fuzz::AddSheetItem(_, SheetPick::Label(_, _, _, 2))
        );
        let no_viewport = |id: u64| {
            m.sheet(id).is_some_and(|s| {
                !s.items
                    .iter()
                    .any(|i| matches!(i.kind, sawhorse_kernel::sheets::ItemKind::Viewport { .. }))
            })
        };
        if (what == 2 && m.sheet(sheet(k)).is_some_and(|s| s.items.is_empty()))
            || (wants_viewport && no_viewport(sheet(k)))
        {
            return Op::AddSheetItem {
                sheet: sheet(k),
                item: Box::new(sheet_item(
                    m,
                    sheet(k),
                    &SheetPick::Viewport(1, 0, 2, [0.02, 0.02, 0.1, 0.08], false),
                )),
            };
        }
    }
    // A tag the model has (mostly), else one of the fuzzer's names.
    let tag_names: Vec<String> = m.tags().keys().cloned().collect();
    let existing_tag = |k: u8| match pick_ref(&tag_names, k) {
        Some(t) if k % 4 != 3 => t.clone(),
        _ => NODE_TAGS[k as usize % NODE_TAGS.len()].to_string(),
    };
    let all_nodes: Vec<u64> = m.nodes().iter().map(|(id, _)| *id).collect();
    let ctx_nodes = m.context_nodes();
    let node = |k: u8| {
        if !k.is_multiple_of(8)
            && let Some(n) = pick(&ctx_nodes, k)
        {
            return n;
        }
        pick(&all_nodes, k).unwrap_or(1)
    };
    // A part the cut list lists (mostly), else a solid, node or definition picked anywhere,
    // sometimes an id that does not exist.
    let parts: Vec<PartRef> = m
        .cut_list(&Default::default())
        .rows
        .iter()
        .map(|r| r.target)
        .collect();
    let defs: Vec<u64> = m.definitions().iter().map(|(id, _)| *id).collect();
    let part = |k: u8| match k % 8 {
        0 => PartRef::Object(object(k / 8)),
        1 => PartRef::Node(node(k / 8)),
        2 => PartRef::Definition(pick(&defs, k / 8).unwrap_or(99)),
        3 if k > 200 => PartRef::Definition(9999),
        _ => pick(&parts, k / 8).unwrap_or(PartRef::Object(object(k))),
    };
    // Outer ring (world) and plane of a face, if it exists.
    // Faces are addressed by persistent id: pick the k-th face, send its id.
    let face = |o: u64, k: u8| -> Option<(u64, Vec<DVec3>, sawhorse_kernel::plane::Plane)> {
        let ob = m.object(o)?;
        let p = ob.brep().to_polys();
        if p.faces.is_empty() {
            return None;
        }
        let fi = k as usize % p.faces.len();
        let ring = p.faces[fi].outer.iter().map(|&i| p.points[i]).collect();
        Some((ob.face_id_at(fi)?, ring, p.plane(fi)?))
    };
    match f {
        Fuzz::NewSketch(o, n) => Op::NewSketch {
            origin: *o,
            normal: *n,
        },
        Fuzz::Rect(s, x, y, w, h) => Op::AddRectangle {
            sketch: sketch(*s),
            x: *x,
            y: *y,
            w: *w,
            h: *h,
        },
        Fuzz::Segment(s, a, b) => Op::InsertSegment {
            sketch: sketch(*s),
            a: *a,
            b: *b,
        },
        Fuzz::Erase(s, a, b) => Op::EraseSegment {
            sketch: sketch(*s),
            a: *a,
            b: *b,
        },
        Fuzz::Polyline(s, p, closed) => Op::AddPolyline {
            sketch: sketch(*s),
            points: p.clone(),
            closed: *closed,
        },
        Fuzz::Extrude(s, r, distance) => {
            let sid = sketch(*s);
            let regions: Vec<u64> = m
                .sketch(sid)
                .map(|sk| {
                    sk.regions()
                        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                        .collect()
                })
                .unwrap_or_default();
            Op::Extrude {
                sketch: sid,
                region: pick(&regions, *r).unwrap_or(1),
                distance: *distance,
            }
        }
        Fuzz::Push(o, k, distance) => {
            let oid = object(*o);
            let face = m
                .object(oid)
                .and_then(|ob| {
                    let n = ob.brep().face_count().max(1);
                    ob.face_id_at(*k as usize % n)
                })
                .unwrap_or(1);
            Op::PushPull {
                object: oid,
                face,
                distance: *distance,
            }
        }
        Fuzz::Guide(p, d) => Op::AddGuide { point: *p, dir: *d },
        Fuzz::Dimension(a, b) => Op::AddDimension {
            a: *a,
            b: *b,
            offset: [0.0, 0.0, 0.1],
        },
        Fuzz::EraseGuide(k) => {
            let ids: Vec<u64> = m.guides().iter().map(|(id, _)| *id).collect();
            match pick(&ids, *k) {
                Some(guide) => Op::EraseGuide { guide },
                None => Op::DeleteGuides,
            }
        }
        Fuzz::EraseDimension(k) => {
            let ids: Vec<u64> = m.dimensions().iter().map(|(id, _)| *id).collect();
            Op::EraseDimension {
                dimension: pick(&ids, *k).unwrap_or(1),
            }
        }
        Fuzz::PushNew(o, k, distance) => {
            let oid = object(*o);
            Op::PushPullNewFace {
                object: oid,
                face: face(oid, *k).map_or(1, |(id, _, _)| id),
                distance: *distance,
            }
        }
        Fuzz::Split {
            object: o,
            face: k,
            e1,
            t1,
            e2,
            t2,
            bend,
        } => {
            let oid = object(*o);
            let Some((fi, ring, _)) = face(oid, *k) else {
                return Op::SplitFace {
                    object: oid,
                    face: 0,
                    path: vec![],
                };
            };
            let n = ring.len();
            let (i, j) = (
                *e1 as usize % n,
                (*e1 as usize + 1 + *e2 as usize % (n - 1)) % n,
            );
            let a = ring[i].lerp(ring[(i + 1) % n], *t1);
            let b = ring[j].lerp(ring[(j + 1) % n], *t2);
            let centre = ring.iter().copied().sum::<DVec3>() / n as f64;
            let mut path = vec![a.to_array()];
            if *bend > 0.0 {
                path.push(a.lerp(b, 0.5).lerp(centre, *bend).to_array());
            }
            path.push(b.to_array());
            Op::SplitFace {
                object: oid,
                face: fi,
                path,
            }
        }
        Fuzz::Imprint {
            object: o,
            face: k,
            at,
            size,
            triangle,
        } => {
            let oid = object(*o);
            let Some((fi, ring, plane)) = face(oid, *k) else {
                return Op::Imprint {
                    object: oid,
                    face: 0,
                    points: vec![],
                };
            };
            let q: Vec<DVec2> = ring.iter().map(|&p| plane.to_local(p)).collect();
            let lo = q
                .iter()
                .copied()
                .fold(DVec2::splat(f64::INFINITY), DVec2::min);
            let hi = q
                .iter()
                .copied()
                .fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max);
            let span = hi - lo;
            let p0 = lo + span * DVec2::new(at[0], at[1]) * 0.8 + span * 0.05;
            let p1 = p0 + span * DVec2::new(size[0], size[1]) * 0.4;
            let corners: Vec<DVec2> = if *triangle {
                vec![p0, DVec2::new(p1.x, p0.y), DVec2::new(p0.x, p1.y)]
            } else {
                vec![p0, DVec2::new(p1.x, p0.y), p1, DVec2::new(p0.x, p1.y)]
            };
            Op::Imprint {
                object: oid,
                face: fi,
                points: corners
                    .iter()
                    .map(|&c| plane.to_world(c).to_array())
                    .collect(),
            }
        }
        Fuzz::EraseEdge(o, k, e) => {
            let oid = object(*o);
            match face(oid, *k) {
                Some((_, ring, _)) => {
                    let i = *e as usize % ring.len();
                    Op::EraseEdge {
                        object: oid,
                        a: ring[i].to_array(),
                        b: ring[(i + 1) % ring.len()].to_array(),
                    }
                }
                None => Op::EraseEdge {
                    object: oid,
                    a: [0.0; 3],
                    b: [1.0, 0.0, 0.0],
                },
            }
        }
        Fuzz::DrawFace {
            object: o,
            face: k,
            at,
            size,
            closed,
        } => {
            let oid = object(*o);
            let target = sawhorse_kernel::model::DrawTarget::Face {
                object: oid,
                face: 0,
            };
            let Some((fi, ring, plane)) = face(oid, *k) else {
                return Op::Draw {
                    target,
                    points: vec![],
                    closed: *closed,
                };
            };
            let q: Vec<DVec2> = ring.iter().map(|&p| plane.to_local(p)).collect();
            let lo = q
                .iter()
                .copied()
                .fold(DVec2::splat(f64::INFINITY), DVec2::min);
            let hi = q
                .iter()
                .copied()
                .fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max);
            let span = hi - lo;
            let p0 = lo + span * DVec2::new(at[0], at[1]);
            let p1 = p0 + span * DVec2::new(size[0], size[1]) * 0.6;
            let corners = if *closed {
                vec![p0, DVec2::new(p1.x, p0.y), p1, DVec2::new(p0.x, p1.y)]
            } else {
                vec![p0, DVec2::new(p1.x, p0.y), p1]
            };
            Op::Draw {
                target: sawhorse_kernel::model::DrawTarget::Face {
                    object: oid,
                    face: fi,
                },
                points: corners
                    .iter()
                    .map(|&c| plane.to_world(c).to_array())
                    .collect(),
                closed: *closed,
            }
        }
        Fuzz::DrawPlane(z, pts, closed) => Op::Draw {
            target: sawhorse_kernel::model::DrawTarget::Plane {
                origin: [0.0, 0.0, *z],
                normal: [0.0, 0.0, 1.0],
            },
            points: pts.iter().map(|p| [p[0], p[1], *z]).collect(),
            closed: *closed,
        },
        Fuzz::EraseFace(o, k) => {
            let oid = object(*o);
            Op::EraseFace {
                object: oid,
                face: face(oid, *k).map_or(0, |(id, _, _)| id),
            }
        }
        Fuzz::EraseRegion(s, r) => {
            let sid = sketch(*s);
            let regions: Vec<u64> = m
                .sketch(sid)
                .map(|sk| {
                    sk.regions()
                        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                        .collect()
                })
                .unwrap_or_default();
            Op::EraseRegion {
                sketch: sid,
                region: pick(&regions, *r).unwrap_or(1),
            }
        }
        Fuzz::Batch(fs) => Op::Batch {
            ops: fs.iter().map(|f| resolve(f, d)).collect(),
        },
        Fuzz::Delete(o) => Op::DeleteObject { object: object(*o) },
        Fuzz::Group(ks) => Op::CreateGroup {
            name: "Fuzz".into(),
            objects: ks.iter().map(|&k| object(k)).collect(),
        },
        Fuzz::Tag(o, t) => Op::SetTag {
            object: object(*o),
            tag: t.map(|t| TAGS[t as usize].to_string()),
        },
        Fuzz::TagVisible(t, v) => Op::SetTagVisible {
            tag: TAGS[*t as usize].to_string(),
            visible: *v,
        },
        Fuzz::MakeGroup(ks) => Op::MakeGroup {
            items: ks.iter().map(|&k| item(k)).collect(),
            name: (ks[0] % 2 == 0).then(|| "Fuzz group".into()),
        },
        Fuzz::MakeComponent(ks) => Op::MakeComponent {
            items: ks.iter().map(|&k| item(k)).collect(),
            name: format!("Part {}", ks[0] % 3),
        },
        Fuzz::MakeUnique(k) => Op::MakeUnique { node: node(*k) },
        Fuzz::Explode(k) => Op::Explode { node: node(*k) },
        Fuzz::DeleteNode(k) => Op::DeleteNode { node: node(*k) },
        Fuzz::Enter(k) => Op::SetContext {
            node: k.map(|k| pick(&all_nodes, k).unwrap_or(1)),
        },
        Fuzz::Leave => Op::LeaveContext,
        Fuzz::NodeTransform(k, x) => Op::Transform {
            objects: vec![],
            nodes: vec![node(*k)],
            xform: xform_of(x),
        },
        Fuzz::NodeCopy(k, x) => Op::Copy {
            objects: vec![],
            nodes: vec![node(*k)],
            xform: xform_of(x),
        },
        Fuzz::NodeArray(k, x, c) => Op::Array {
            objects: vec![],
            nodes: vec![node(*k)],
            xform: xform_of(x),
            count: u32::from(*c),
        },
        Fuzz::Hide(k, b) => Op::SetHidden {
            items: vec![item(*k)],
            hidden: *b,
        },
        Fuzz::Lock(k, b) => Op::SetLocked {
            items: vec![item(*k)],
            locked: *b,
        },
        Fuzz::TagItems(k, t) => Op::TagItems {
            items: vec![item(*k)],
            tag: t.map(|t| NODE_TAGS[t as usize].to_string()),
        },
        Fuzz::CreateTag(t, c) => Op::CreateTag {
            name: NODE_TAGS[*t as usize].into(),
            color: *c,
        },
        Fuzz::RenameTag(a, b) => Op::RenameTag {
            tag: existing_tag(*a),
            name: NODE_TAGS[*b as usize].into(),
        },
        Fuzz::DeleteTag(t) => Op::DeleteTag {
            tag: existing_tag(*t),
        },
        Fuzz::TagColor(t, c) => Op::SetTagColor {
            tag: existing_tag(*t),
            color: *c,
        },
        Fuzz::Scene(kind, n, hidden, ctx) => {
            // Updates, deletes and activations mostly name a scene that exists.
            let names: Vec<String> = m.scenes().iter().map(|s| s.name.clone()).collect();
            let name = match pick_ref(&names, *n) {
                Some(s) if *kind > 0 && n % 4 != 3 => s.clone(),
                _ => format!("Scene {n}"),
            };
            let tags: Vec<String> = m.tags().keys().cloned().collect();
            let scene = SceneDef {
                name: name.clone(),
                eye: [3.0, -3.0, 2.0 + f64::from(*n)],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                fov: 35.0,
                parallel: n % 2 == 0,
                hidden_tags: tags
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| hidden & (1 << (i % 8)) != 0)
                    .map(|(_, t)| t.clone())
                    .collect(),
                context: ctx.and_then(|k| pick(&all_nodes, k)),
                // Some scenes carry the document's style and sun.
                style: if n % 3 == 0 { m.style().cloned() } else { None },
                shadows: if n % 3 == 1 {
                    m.shadows().copied()
                } else {
                    None
                },
                // Some carry the section planes' state (lane 12).
                sections: (n % 4 == 2).then(|| Box::new(m.scene_sections_now())),
            };
            match kind {
                0 => Op::AddScene { scene },
                1 => Op::UpdateScene { name, scene },
                2 => Op::DeleteScene { name },
                _ => Op::ActivateScene { name },
            }
        }
        Fuzz::Rename(k, n) => Op::Rename {
            item: item(*k),
            name: n.map(|n| format!("Name {n}")),
        },
        Fuzz::Grain(k, g) => Op::SetGrain {
            part: part(*k),
            grain: [Grain::Length, Grain::Width, Grain::None][*g as usize % 3],
        },
        // Resolved in `concrete` through refbatch; never reaches here in a run.
        Fuzz::PluginBatch(_) => Op::Batch { ops: Vec::new() },
        Fuzz::AddSection(origin, normal, active) => Op::AddSection {
            origin: *origin,
            normal: *normal,
            name: None,
            active: *active,
            hidden: false,
        },
        Fuzz::PlaceSection(k, origin, normal) => Op::PlaceSection {
            section: section(*k),
            origin: *origin,
            normal: *normal,
        },
        Fuzz::SectionAct(k, act, b) => {
            let section = section(*k);
            match act {
                0 => Op::FlipSection { section },
                1 => Op::SetSectionActive {
                    section,
                    active: *b,
                },
                2 => Op::SetSectionHidden {
                    section,
                    hidden: *b,
                },
                3 => Op::RenameSection {
                    section,
                    name: if *b {
                        format!("Section {k}")
                    } else {
                        String::new()
                    },
                },
                4 => Op::DeleteSection { section },
                _ => Op::SliceToGroup {
                    section,
                    name: b.then(|| "Slice".to_string()),
                },
            }
        }
        Fuzz::MoveSections(k, x) => Op::MoveSections {
            sections: k.iter().map(|k| section(*k)).collect(),
            xform: xform_of(x),
        },
        Fuzz::CopySections(k, x, n) => Op::CopySections {
            sections: k.iter().map(|k| section(*k)).collect(),
            xform: xform_of(x),
            count: u32::from(*n),
        },
        Fuzz::AddSheet(k) => Op::AddSheet {
            setup: Box::new(setup_of(*k)),
        },
        Fuzz::SetSheet(k, v) => Op::SetSheet {
            sheet: sheet(*k),
            setup: Box::new(setup_of(*v)),
        },
        Fuzz::DeleteSheet(k) => Op::DeleteSheet { sheet: sheet(*k) },
        Fuzz::MoveSheet(k, to) => Op::MoveSheet {
            sheet: sheet(*k),
            to: u32::from(*to),
        },
        Fuzz::AddSheetItem(k, pick) => {
            let sheet = sheet(*k);
            Op::AddSheetItem {
                sheet,
                item: Box::new(sheet_item(m, sheet, pick)),
            }
        }
        Fuzz::SetSheetItem(k, i, pick) => {
            use sawhorse_kernel::sheets::ItemKind;
            let sheet = sheet(*k);
            let item = sheet_item_id(m, sheet, *i);
            let mut kind = sheet_item(m, sheet, pick);
            // Mostly a change of the same kind of item (a kind never changes: that is
            // refused), else the intent as it is.
            if let Some(old) = m.sheet(sheet).and_then(|s| s.item(item))
                && std::mem::discriminant(&old.kind) != std::mem::discriminant(&kind)
                && i % 4 != 3
            {
                kind = match &old.kind {
                    ItemKind::Dimension { .. } => {
                        let mut d = old.kind.clone();
                        if let ItemKind::Dimension { offset, .. } = &mut d {
                            *offset += 0.005;
                        }
                        d
                    }
                    other => other.moved([0.0127, -0.00635]),
                };
            }
            Op::SetSheetItem {
                sheet,
                item,
                kind: Box::new(kind),
            }
        }
        Fuzz::MoveSheetItems(k, items, by) => {
            let sheet = sheet(*k);
            Op::MoveSheetItems {
                sheet,
                items: items.iter().map(|i| sheet_item_id(m, sheet, *i)).collect(),
                by: *by,
            }
        }
        Fuzz::DeleteSheetItems(k, items) => {
            let sheet = sheet(*k);
            Op::DeleteSheetItems {
                sheet,
                items: items.iter().map(|i| sheet_item_id(m, sheet, *i)).collect(),
            }
        }
        Fuzz::OrderSheetItem(k, i, to) => {
            let sheet = sheet(*k);
            Op::OrderSheetItem {
                sheet,
                item: sheet_item_id(m, sheet, *i),
                to: u32::from(*to),
            }
        }
        Fuzz::Stock(k, t, sheet) => Op::SetStock {
            part: part(*k),
            stock: t.map(|i| STOCKS[i as usize % STOCKS.len()].to_string()),
            sheet: *sheet,
        },
        Fuzz::DefineStock(sheet, n, d, none, replace) => Op::DefineStock {
            stock: sawhorse_kernel::stock::StockDef {
                name: STOCK_NAMES[*n as usize % STOCK_NAMES.len()].to_string(),
                kind: if *sheet {
                    sawhorse_kernel::stock::StockKind::Sheet
                } else {
                    sawhorse_kernel::stock::StockKind::Board
                },
                length: STOCK_SIZES[d[0] as usize % STOCK_SIZES.len()],
                width: STOCK_SIZES[d[1] as usize % STOCK_SIZES.len()],
                thickness: STOCK_SIZES[d[2] as usize % 4],
                nominal: (!*sheet).then(|| "4/4".to_string()),
                grain: if *none {
                    sawhorse_kernel::stock::StockGrain::None
                } else {
                    sawhorse_kernel::stock::StockGrain::Length
                },
            },
            replace: replace.map(|r| STOCK_NAMES[r as usize % STOCK_NAMES.len()].to_string()),
        },
        Fuzz::DeleteStock(n) => Op::DeleteStock {
            name: STOCK_NAMES[*n as usize % STOCK_NAMES.len()].to_string(),
        },
        Fuzz::MaterialStock(k, s) => {
            // The model's own materials and stock texts, and some it does not have.
            let mut names: Vec<String> = m
                .cut_list(&Default::default())
                .rows
                .iter()
                .filter_map(|r| r.material.clone())
                .collect();
            names.extend(["Oak", "3/4 Baltic birch ply", " "].map(String::from));
            Op::SetMaterialStock {
                material: names[*k as usize % names.len()].clone(),
                stock: s.map(|i| STOCK_NAMES[i as usize % STOCK_NAMES.len()].to_string()),
            }
        }
        Fuzz::NestSettings(k, t, maxrects) => Op::SetNestSettings {
            settings: sawhorse_kernel::stock::NestSettings {
                kerf: NEST_LENGTHS[*k as usize % NEST_LENGTHS.len()],
                trim: NEST_LENGTHS[(*t as usize / 3) % NEST_LENGTHS.len()],
                algorithm: if *maxrects {
                    sawhorse_kernel::stock::NestAlgorithm::Maxrects
                } else {
                    sawhorse_kernel::stock::NestAlgorithm::Guillotine
                },
            },
        },
        Fuzz::ResetStock => Op::ResetStockLibrary,
        Fuzz::Include(k, b) => Op::SetInclude {
            part: part(*k),
            include: *b,
        },
        Fuzz::ImportPart(shape, grouped, at, size) => {
            let at = at.map(|v| f64::from(v % 8) * 0.05);
            let size = size.map(|v| 0.01 + f64::from(v % 16) * 0.02);
            let c = |i: u32| {
                [
                    at[0] + if i & 1 != 0 { size[0] } else { 0.0 },
                    at[1] + if i & 2 != 0 { size[1] } else { 0.0 },
                    at[2] + if i & 4 != 0 { size[2] } else { 0.0 },
                ]
            };
            let quads: [[u32; 4]; 6] = [
                [0, 2, 3, 1],
                [4, 5, 7, 6],
                [0, 1, 5, 4],
                [2, 6, 7, 3],
                [0, 4, 6, 2],
                [1, 3, 7, 5],
            ];
            let skip: &[usize] = match shape {
                1 => &[1],
                2 => &[0, 1],
                _ => &[],
            };
            let mut positions: Vec<[f64; 3]> = (0..8).map(c).collect();
            let mut faces: Vec<Vec<Vec<u32>>> = quads
                .iter()
                .enumerate()
                .filter(|(i, _)| !skip.contains(i))
                .map(|(_, q)| vec![q.to_vec()])
                .collect();
            if *shape == 3 {
                let mut p = c(7);
                p[0] += 5e-5;
                positions.push(p);
                for v in faces[1][0].iter_mut() {
                    if *v == 7 {
                        *v = 8;
                    }
                }
            }
            let item = sawhorse_kernel::meshbuild::ImportItem {
                name: "Fuzz part".into(),
                group: None,
                tag: None,
                material: None,
                positions,
                faces,
                lines: vec![],
                merge_coplanar: false,
                face_labels: vec![],
                face_materials: vec![],
                face_uvs: vec![],
            };
            let (items, nodes) = if *grouped {
                (
                    vec![],
                    vec![sawhorse_kernel::model::ImportNode {
                        name: Some("Fuzz group".into()),
                        tag: None,
                        hidden: false,
                        kind: sawhorse_kernel::model::ImportNodeKind::Group {
                            items: vec![item],
                            children: vec![],
                        },
                    }],
                )
            } else {
                (vec![item], vec![])
            };
            Op::Import {
                items,
                strokes: vec![],
                materials: vec![],
                scenes: vec![],
                hidden_tags: vec![],
                definitions: vec![],
                nodes,
            }
        }
        Fuzz::GroupLoose(k) => Op::GroupLoose {
            meshes: match k {
                None => vec![],
                Some(k) => {
                    let ids: Vec<u64> = m.meshes().iter().map(|(id, _)| *id).collect();
                    vec![pick(&ids, *k).unwrap_or(9999)]
                }
            },
        },
        Fuzz::DefineMat {
            name,
            color,
            texture,
            size,
            rotation,
            stock,
        } => {
            let name = MATERIALS[*name as usize % MATERIALS.len()].to_string();
            let path = format!("assets/fuzz-{}.png", name.len());
            let def = Op::DefineMaterial {
                name: name.clone(),
                color: *color,
                texture: match texture {
                    0 => None,
                    1 | 3 => Some(path.clone()),
                    _ => Some("oak.jpg".into()),
                },
                size: match size % 4 {
                    0 => None,
                    1 => Some([0.3, 0.15]),
                    2 => Some([f64::from(*size) * 1e-3, 0.5]),
                    _ => Some([0.0, 1.0]),
                },
                rotation: *rotation,
                offset: [0.0, f64::from(*size) * 1e-3],
                stock: match stock {
                    0 => None,
                    1 => Some(sawhorse_kernel::looks::StockLink {
                        kind: sawhorse_kernel::looks::StockKind::Lumber,
                        species: name.clone(),
                    }),
                    _ => Some(sawhorse_kernel::looks::StockLink {
                        kind: sawhorse_kernel::looks::StockKind::Sheet,
                        species: String::new(),
                    }),
                },
            };
            if *texture == 1 {
                Op::Batch {
                    ops: vec![
                        Op::AddTexture {
                            path,
                            mime: "image/png".into(),
                            data: sawhorse_kernel::testing::tiny_png(
                                1 + u32::from(color[0] % 4),
                                1 + u32::from(color[1] % 3),
                                [color[0], color[1], color[2]],
                            ),
                        },
                        def,
                    ],
                }
            } else {
                def
            }
        }
        Fuzz::PaintFaces(o, mask, mat) => {
            let o = object(*o);
            let ids = m
                .object(o)
                .map(|x| x.brep().persistent_face_ids())
                .unwrap_or_default();
            let mut faces: Vec<u64> = ids
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << (i % 8)) != 0)
                .map(|(_, &f)| f)
                .collect();
            if faces.is_empty() {
                faces = ids.first().copied().into_iter().collect();
            }
            if mask % 16 == 15 {
                faces.push(9999);
            }
            Op::Paint {
                target: sawhorse_kernel::looks::PaintTarget::Faces { object: o, faces },
                material: mat.map(|k| MATERIALS[k as usize % MATERIALS.len()].to_string()),
            }
        }
        Fuzz::PaintItems(ks, mat) => Op::Paint {
            target: sawhorse_kernel::looks::PaintTarget::Items {
                items: ks
                    .iter()
                    .map(|&k| {
                        if k > 240 {
                            Item::Node(node(k))
                        } else {
                            item(k)
                        }
                    })
                    .collect(),
            },
            material: mat.map(|k| MATERIALS[k as usize % MATERIALS.len()].to_string()),
        },
        Fuzz::FaceUv(o, f, uv) => {
            let o = object(*o);
            let f = face(o, *f).map_or(1, |x| x.0);
            Op::SetFaceUv {
                object: o,
                face: f,
                uv: uv.map(|(s, r, x, y)| sawhorse_kernel::looks::UvXform {
                    scale: [s.abs(), (s * 0.5).abs()],
                    rotation: r * 90.0,
                    offset: [x * 0.01, y * 0.01],
                }),
            }
        }
        Fuzz::RenameMat(a, b) => {
            // Mostly a material the model has, to a name it may not have yet.
            let names: Vec<String> = m.materials().keys().cloned().collect();
            let name = match pick_ref(&names, *a) {
                Some(n) if a % 4 != 3 => n.clone(),
                _ => MATERIALS[*a as usize % MATERIALS.len()].to_string(),
            };
            let to = if b % 3 == 0 {
                format!("{name} {b}")
            } else {
                MATERIALS[*b as usize % MATERIALS.len()].to_string()
            };
            Op::RenameMaterial { name, to }
        }
        Fuzz::DeleteMat(a) => {
            let names: Vec<String> = m.materials().keys().cloned().collect();
            Op::DeleteMaterial {
                name: match pick_ref(&names, *a) {
                    Some(n) if a % 4 != 3 => n.clone(),
                    _ => MATERIALS[*a as usize % MATERIALS.len()].to_string(),
                },
            }
        }
        Fuzz::Style(s) => {
            use sawhorse_kernel::looks::{EdgeStyle, FaceStyle, Style};
            Op::SetStyle {
                style: s.map(|k| Style {
                    preset: (k == 1).then(|| "Shop drawing".to_string()),
                    face: [
                        FaceStyle::Textured,
                        FaceStyle::HiddenLine,
                        FaceStyle::XRay,
                        FaceStyle::Wireframe,
                    ][k as usize % 4],
                    edges: EdgeStyle {
                        edges: k != 3,
                        profiles: k % 2 == 1,
                        extensions: k == 1,
                        endpoints: k == 2,
                    },
                    background: (k == 2).then_some([250, 248, 240]),
                    sky: None,
                    ground: (k == 3).then_some([190, 180, 150]),
                    // Lane 12: section switches and colours too.
                    section_cuts: k != 2,
                    section_planes: k != 1,
                    section_fill: k % 2 == 0,
                    section_fill_color: (k == 1).then_some([180, 60, 40]),
                    section_line_color: (k == 3).then_some([20, 20, 20]),
                    section_line_width: (k == 2).then_some(5),
                }),
            }
        }
        Fuzz::Shadows(s) => Op::SetShadows {
            shadows: s.map(|(on, minutes, latitude)| sawhorse_kernel::looks::Shadows {
                on,
                minutes,
                latitude,
                ..Default::default()
            }),
        },
        Fuzz::Undo => Op::Undo,
        Fuzz::Redo => Op::Redo,
        Fuzz::Raw(op) => (**op).clone(),
        Fuzz::Transform(o, pair, x) => {
            let mut objects = vec![object(*o)];
            if pair % 4 == 0 {
                objects.push(object(o.wrapping_add(*pair)));
            }
            Op::Transform {
                objects,
                xform: xform_of(x),
                nodes: vec![],
            }
        }
        Fuzz::Copy(o, x) => Op::Copy {
            objects: vec![object(*o)],
            xform: xform_of(x),
            nodes: vec![],
        },
        Fuzz::Array(o, x, n) => Op::Array {
            objects: vec![object(*o)],
            xform: xform_of(x),
            count: u32::from(*n),
            nodes: vec![],
        },
        Fuzz::MoveGeometry {
            object: o,
            face: k,
            which,
            index,
            xform,
            autofold,
        } => {
            let oid = object(*o);
            let points = match face(oid, *k) {
                Some((_, ring, _)) => {
                    let i = *index as usize % ring.len();
                    match which {
                        0 => ring.iter().map(|p| p.to_array()).collect(),
                        1 => vec![ring[i].to_array(), ring[(i + 1) % ring.len()].to_array()],
                        _ => vec![ring[i].to_array()],
                    }
                }
                None => vec![[0.0; 3]],
            };
            Op::TransformGeometry {
                object: oid,
                points,
                xform: xform_of(xform),
                autofold: *autofold,
            }
        }
        Fuzz::Offset {
            kind,
            owner,
            part,
            edges,
            distance,
        } => {
            use sawhorse_kernel::offset::OffsetSource;
            let source = match kind {
                0 => {
                    let oid = object(*owner);
                    OffsetSource::Face {
                        object: oid,
                        face: face(oid, *part).map_or(1, |(id, _, _)| id),
                    }
                }
                1 => {
                    let sid = sketch(*owner);
                    let regions: Vec<u64> = m
                        .sketch(sid)
                        .map(|sk| {
                            sk.regions()
                                .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                                .collect()
                        })
                        .unwrap_or_default();
                    OffsetSource::Region {
                        sketch: sid,
                        region: pick(&regions, *part).unwrap_or(1),
                    }
                }
                _ => {
                    let oid = object(*owner);
                    match face(oid, *part) {
                        Some((_, ring, plane)) => {
                            let n = (*edges as usize).min(ring.len());
                            let closed = n == ring.len();
                            let pts: Vec<[f64; 3]> = ring
                                .iter()
                                .take(if closed { n } else { n + 1 })
                                .map(|p| p.to_array())
                                .collect();
                            OffsetSource::Edges {
                                points: pts,
                                closed,
                                normal: plane.normal.to_array(),
                                object: Some(oid),
                            }
                        }
                        None => OffsetSource::Edges {
                            points: vec![],
                            closed: false,
                            normal: [0.0, 0.0, 1.0],
                            object: None,
                        },
                    }
                }
            };
            Op::Offset {
                source,
                distance: *distance,
            }
        }
        Fuzz::FollowMe {
            region,
            owner,
            part,
            path,
            other,
            a,
            b,
        } => {
            use sawhorse_kernel::sweep::Profile;
            if *region {
                let sid = sketch(*owner);
                let regions: Vec<u64> = m
                    .sketch(sid)
                    .map(|sk| {
                        sk.regions()
                            .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
                            .collect()
                    })
                    .unwrap_or_default();
                let plane = m.sketch(sid).map(|s| *s.plane());
                let (o, n, u) = plane.map_or((DVec3::ZERO, DVec3::Z, DVec3::X), |p| {
                    (p.origin, p.normal, p.u)
                });
                let p1 = o + n * *a;
                let mut pts = vec![o.to_array(), p1.to_array()];
                if *path > 0 {
                    pts.push((p1 + u * *b).to_array());
                }
                return Op::FollowMe {
                    profile: Profile::Region {
                        sketch: sid,
                        region: pick(&regions, *part).unwrap_or(1),
                    },
                    path: pts,
                    closed: false,
                };
            }
            let oid = object(*owner);
            let Some((fid, ring, plane)) = face(oid, *part) else {
                return Op::FollowMe {
                    profile: Profile::Face {
                        object: oid,
                        face: 1,
                    },
                    path: vec![],
                    closed: false,
                };
            };
            let p0 = ring[0];
            match path {
                0 | 1 => {
                    let p1 = p0 + plane.normal * *a;
                    let mut pts = vec![p0.to_array(), p1.to_array()];
                    if *path == 1 {
                        pts.push((p1 + plane.u * *b).to_array());
                    }
                    Op::FollowMe {
                        profile: Profile::Face {
                            object: oid,
                            face: fid,
                        },
                        path: pts,
                        closed: false,
                    }
                }
                _ => {
                    // Round the edge of another face (the moulding case).
                    let pts = face(oid, *other)
                        .map(|(_, r, _)| r.iter().map(|p| p.to_array()).collect())
                        .unwrap_or_default();
                    Op::FollowMe {
                        profile: Profile::Face {
                            object: oid,
                            face: fid,
                        },
                        path: pts,
                        closed: true,
                    }
                }
            }
        }
    }
}

/// Resolve a whole sequence (each intent against the document the earlier ops built).
/// A panic while resolving (a kernel bug hit before `check_sequence` runs) stops the
/// sequence there and is returned with the ops so far, so it still becomes a repro.
fn concrete(intents: &[Fuzz], tally: &mut Tally) -> (Vec<Op>, Option<String>) {
    let mut d = Document::new();
    let mut ops = vec![Op::NewSketch {
        origin: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    }];
    let _ = d.apply(&ops[0]);
    for f in intents {
        if let Fuzz::PluginBatch(steps) = f {
            let req = plugin_batch(steps, &d);
            let before = (d.hash(), d.log().len(), d.depth());
            let mut probe = d.clone();
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sawhorse_kernel::refbatch::apply(&mut probe, &req)
            }));
            let req_json = serde_json::to_string(&req).unwrap_or_default();
            match attempt {
                Err(p) => {
                    let msg = p
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "non-string panic".into());
                    return (
                        ops,
                        Some(format!("plugin batch {req_json} panicked: {msg}")),
                    );
                }
                Ok(Ok((_, op))) => {
                    tally.plugin(true, None);
                    // The logged op must do exactly what refbatch did, applied as itself.
                    ops.push(op.clone());
                    let r = d.apply(&op);
                    tally.record(&op, &r);
                    if r.is_err() || d.hash() != probe.hash() || d.log() != probe.log() {
                        return (
                            ops,
                            Some(format!(
                                "plugin batch {req_json}: its logged op did not land where refbatch did ({r:?})"
                            )),
                        );
                    }
                }
                Ok(Err(e)) => {
                    tally.plugin(false, Some(&e.to_string()));
                    if e.is_bug() {
                        return (
                            ops,
                            Some(format!(
                                "plugin batch {req_json}: validator rejected the result: {e}"
                            )),
                        );
                    }
                    if (probe.hash(), probe.log().len(), probe.depth()) != before {
                        return (
                            ops,
                            Some(format!(
                                "plugin batch {req_json} was refused ({e}) but the document changed"
                            )),
                        );
                    }
                }
            }
            continue;
        }
        let op = resolve(f, &d);
        ops.push(op.clone());
        let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.apply(&op)));
        if let Ok(r) = &step {
            tally.record(&op, r);
            // FUZZ_DUMP_REFUSALS=1: keep the first few boolean refusals as replayable logs
            // (tests/repro/refusals/, gitignored) to study what the engine left behind.
            if let Err(e) = r
                && e.to_string().contains("not a valid solid")
                && std::env::var("FUZZ_DUMP_REFUSALS").is_ok_and(|v| v == "1")
            {
                tally.dumped += 1;
                if tally.dumped <= 30 {
                    let _ = write_repro(
                        &repro_dir().join("refusals"),
                        &format!("refusal-{:02}", tally.dumped),
                        &e.to_string(),
                        &ops,
                    );
                }
            }
        }
        if let Err(p) = step {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "non-string panic".into());
            let at = ops.len() - 1;
            return (ops, Some(format!("op {at} {op:?} panicked: {msg}")));
        }
    }
    (ops, None)
}

fn repro_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repro")
}

/// Per-op-kind accepted / refused counts, and the most common refusal reasons.
#[derive(Default)]
struct Tally {
    kinds: std::collections::BTreeMap<String, (usize, usize)>,
    reasons: std::collections::BTreeMap<String, usize>,
    dumped: usize,
    /// Lane 11: plugin batches accepted and refused, and the refusals by reason.
    plugin: (usize, usize),
    plugin_reasons: std::collections::BTreeMap<String, usize>,
}

impl Tally {
    fn plugin(&mut self, accepted: bool, reason: Option<&str>) {
        if accepted {
            self.plugin.0 += 1;
        } else {
            self.plugin.1 += 1;
        }
        if let Some(r) = reason {
            // "op 3 (push_pull): no object #12" -> "(push_pull): no object #N"
            let mut words: Vec<String> = Vec::new();
            for w in r.split_whitespace().take(12) {
                let w = if w.chars().any(|c| c.is_ascii_digit()) && !w.contains('(') {
                    w.chars()
                        .map(|c| if c.is_ascii_digit() { 'N' } else { c })
                        .collect::<String>()
                } else {
                    w.to_string()
                };
                words.push(w);
            }
            *self.plugin_reasons.entry(words.join(" ")).or_default() += 1;
        }
    }

    fn record(
        &mut self,
        op: &Op,
        r: &Result<sawhorse_kernel::OpOutcome, sawhorse_kernel::KernelError>,
    ) {
        let json = serde_json::to_value(op).unwrap_or_default();
        let kind = json["op"].as_str().unwrap_or("?").to_string();
        let e = self.kinds.entry(kind.clone()).or_default();
        match r {
            Ok(_) => e.0 += 1,
            Err(err) => {
                e.1 += 1;
                let reason: String = err
                    .to_string()
                    .chars()
                    .take(
                        std::env::var("FUZZ_REASON_LEN")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(70),
                    )
                    .collect();
                *self.reasons.entry(format!("{kind}: {reason}")).or_default() += 1;
            }
        }
    }

    fn report(&self) -> String {
        let mut out = String::from(
            "  op kind          accepted   refused
",
        );
        for (k, (a, r)) in &self.kinds {
            out += &format!(
                "  {k:<16} {a:>8} {r:>9}
"
            );
        }
        // The boolean path's typed refusal (near-coincident geometry, mostly on tilted
        // planes) is tracked by name: the lane reports quote it.
        let boolean: usize = self
            .reasons
            .iter()
            .filter(|(r, _)| r.contains("not a valid solid"))
            .map(|(_, n)| n)
            .sum();
        out += &format!(
            "  boolean-path refusals (not a valid solid): {boolean}
"
        );
        // The boolean path's other typed refusal: the engine's output could not be turned
        // back into faces (pinches, slivers, an operand the engine would not take).
        let degenerate: usize = self
            .reasons
            .iter()
            .filter(|(r, _)| {
                r.contains("degenerate geometry")
                    || r.contains("boolean engine failed")
                    || r.contains("is not a closed manifold")
            })
            .map(|(_, n)| n)
            .sum();
        out += &format!(
            "  boolean-path refusals (degenerate engine output): {degenerate}
"
        );
        let accepted = |k: &str| self.kinds.get(k).map_or(0, |e| e.0);
        let pushes = accepted("push_pull") + accepted("push_pull_new_face");
        let sweeps = accepted("follow_me");
        let push_refusals: usize = self
            .reasons
            .iter()
            .filter(|(r, _)| r.contains("not a valid solid") && r.starts_with("push_pull"))
            .map(|(_, n)| n)
            .sum();
        out += &format!(
            "  boolean refusal rate: {boolean} / ({pushes} accepted push/pulls + {sweeps} accepted sweeps) = {:.4}%; push/pull alone {push_refusals} / {pushes} = {:.4}%
",
            100.0 * boolean as f64 / (pushes + sweeps).max(1) as f64,
            100.0 * push_refusals as f64 / pushes.max(1) as f64,
        );
        out += &format!(
            "  plugin batches (lane 11): {} accepted, {} refused (document untouched each time)
",
            self.plugin.0, self.plugin.1
        );
        let mut pr: Vec<(&String, &usize)> = self.plugin_reasons.iter().collect();
        pr.sort_by(|a, b| b.1.cmp(a.1));
        for (reason, n) in pr.into_iter().take(12) {
            out += &format!(
                "  {n:>7}  plugin batch refused: {reason}
"
            );
        }
        out += &format!(
            "  plane snapping: {}
",
            if sawhorse_kernel::snap::plane_snapping() {
                "on"
            } else {
                "off (FUZZ_PLANE_SNAP=0)"
            }
        );
        // Lane 12: why section and sheet edits were refused.
        let lane12 = [
            "add_section",
            "place_section",
            "flip_section",
            "set_section",
            "rename_section",
            "delete_section",
            "move_sections",
            "copy_sections",
            "slice_to_group",
            "add_sheet",
            "set_sheet",
            "delete_sheet",
            "move_sheet",
            "order_sheet",
        ];
        let mut l12: Vec<(&String, &usize)> = self
            .reasons
            .iter()
            .filter(|(r, _)| lane12.iter().any(|k| r.starts_with(k)))
            .collect();
        l12.sort_by(|a, b| b.1.cmp(a.1));
        out += "  section and sheet refusals (lane 12):\n";
        for (reason, n) in l12.into_iter().take(24) {
            out += &format!("  {n:>7}  {reason}\n");
        }
        // What the boolean refusals were (FUZZ_REASON_LEN=200 shows the validator's words).
        let mut bools: Vec<(&String, &usize)> = self
            .reasons
            .iter()
            .filter(|(r, _)| r.contains("not a valid solid") || r.contains("degenerate geometry"))
            .collect();
        bools.sort_by(|a, b| b.1.cmp(a.1));
        out += "  boolean refusals by reason:
";
        for (reason, n) in bools.into_iter().take(16) {
            out += &format!(
                "  {n:>7}  {reason}
"
            );
        }
        let mut top: Vec<(&String, &usize)> = self.reasons.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        out += "  top refusals:
";
        for (reason, n) in top.into_iter().take(40) {
            out += &format!(
                "  {n:>7}  {reason}
"
            );
        }
        out
    }
}

#[derive(Default)]
struct Totals {
    sequences: usize,
    accepted: usize,
    refused: usize,
    objects: usize,
}

fn run(name: &str, cases: u32, max_len: usize, seed: Option<u64>) {
    // Measurement switch: the same seeds with and without boolean plane snapping.
    sawhorse_kernel::snap::set_plane_snapping(
        std::env::var("FUZZ_PLANE_SNAP").map_or(true, |v| v != "0"),
    );
    let config = Config {
        cases,
        failure_persistence: None,
        ..Config::default()
    };
    let mut runner = match seed {
        Some(s) => {
            let mut bytes = [0u8; 32];
            bytes[..8].copy_from_slice(&s.to_le_bytes());
            TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &bytes))
        }
        None => TestRunner::new(config),
    };
    let totals = std::cell::RefCell::new(Totals::default());
    let tally = std::cell::RefCell::new(Tally::default());
    let started = std::time::Instant::now();
    // Most sequences start from a solid (a rectangle pulled up), so face ops, push/pull and
    // booleans see real geometry; the rest start from an empty ground sketch.
    let size = || prop_oneof![(1i32..9).prop_map(|k| f64::from(k) * 0.25), 0.1..2.5f64];
    let start = prop_oneof![
        3 => (grid(), grid(), size(), size(), size(), any::<bool>()).prop_map(|(x, y, w, h, d, down)| vec![
            Fuzz::Rect(0, x, y, w, h),
            Fuzz::Extrude(0, 0, if down { -d } else { d }),
        ]),
        // A component with copies, one of them open: edits inside propagate.
        2 => (grid(), grid(), size(), size(), size(), xform_pick(), any::<u8>()).prop_map(|(x, y, w, h, d, step, which)| vec![
            Fuzz::Rect(0, x, y, w, h),
            Fuzz::Extrude(0, 0, d),
            Fuzz::MakeComponent(vec![0]),
            Fuzz::NodeArray(0, step, 2),
            Fuzz::Enter(Some(which)),
        ]),
        1 => Just(Vec::new()),
    ];
    let strategy =
        (start, prop::collection::vec(fuzz_op(), 1..max_len)).prop_map(|(mut s, rest)| {
            s.extend(rest);
            s
        });
    let result = runner.run(&strategy, |intents| {
        let (ops, panicked) = concrete(&intents, &mut tally.borrow_mut());
        let checked = match panicked {
            Some(e) => Err(e),
            None => check_sequence(&ops),
        };
        match checked {
            Ok(s) => {
                let mut t = totals.borrow_mut();
                t.sequences += 1;
                t.accepted += s.accepted;
                t.refused += s.refused;
                t.objects += s.objects;
                Ok(())
            }
            Err(e) => {
                let _ = write_repro(&repro_dir(), &format!("pending-{name}"), &e, &ops);
                Err(TestCaseError::fail(e))
            }
        }
    });
    let t = totals.borrow();
    println!(
        "{name}: {} sequences, {} ops accepted, {} refused, {} solids at the end, {:.1}s",
        t.sequences,
        t.accepted,
        t.refused,
        t.objects,
        started.elapsed().as_secs_f64()
    );
    if std::env::var("FUZZ_REPORT").is_ok_and(|v| v == "1") {
        println!("{}", tally.borrow().report());
    }
    if let Err(e) = result {
        panic!(
            "{e}\nminimal failing sequence written to {}",
            repro_dir().join(format!("pending-{name}.json")).display()
        );
    }
}

#[test]
fn quick_fuzz() {
    run("quick_fuzz", 192, 24, None);
}

/// Long run for CI nightlies and before releases. FUZZ_CASES sets the case count,
/// FUZZ_SEED a deterministic seed (one per parallel shard).
#[test]
#[ignore]
fn long_fuzz() {
    let cases = std::env::var("FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let seed: Option<u64> = std::env::var("FUZZ_SEED").ok().and_then(|v| v.parse().ok());
    let name = match seed {
        Some(s) => format!("long_fuzz_seed{s}"),
        None => "long_fuzz".to_string(),
    };
    run(&name, cases, 48, seed);
}
