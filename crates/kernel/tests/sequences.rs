//! Op-sequence fuzzer (proptest-driven) over the whole document: sketch strokes with
//! merging / T-junctions / crossings / overlaps / erasing, rectangles, polylines, extrude,
//! general push/pull (both ways, through the far side, severing), split_face, imprint,
//! erasing (object edges and faces, sketch regions), batches, delete, groups, tags, and
//! (lane 4) transforms, copies, arrays, moving part of a solid (autofold on and off),
//! offsets of faces / regions / edge chains, and Follow Me (open and closed paths, adding
//! and cutting), and (lane 5) groups and components: make group / component / unique,
//! explode, delete, open and close (the editing context), node moves / copies / arrays,
//! hide, lock, tags (create, rename, colour, delete, assign) and scenes (add, update,
//! delete, activate), and (lane 7) grain and stock on cut-list parts, and (lane 9) the stock
//! library (define, replace, delete, per-material defaults, kerf / trim / algorithm, reset),
//! with undo / redo interleaved anywhere. After every accepted op the cut list is worked out
//! and checked against a brute-force count of the world (`testing::check_cut_list`), and the
//! parts are nested with both algorithms and every layout invariant checked
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
    Raw(Op),
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
    ]
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
        1 => (0u64..4, 0u64..6, distance()).prop_map(|(sketch, region, distance)| Fuzz::Raw(Op::Extrude { sketch, region, distance })),
        1 => (0u64..4, 0u64..14, distance()).prop_map(|(object, face, distance)| Fuzz::Raw(Op::PushPull { object, face, distance })),
        2 => (any::<u8>(), any::<u8>(), distance()).prop_map(|(o, f, d)| Fuzz::PushNew(o, f, d)),
        1 => ((grid(), grid(), grid()), prop::option::of(normal())).prop_map(|((x, y, z), d)| Fuzz::Guide([x, y, z], d)),
        1 => ((grid(), grid(), grid()), (grid(), grid(), grid())).prop_map(|(a, b)| Fuzz::Dimension([a.0, a.1, a.2], [b.0, b.1, b.2])),
        1 => any::<u8>().prop_map(Fuzz::EraseGuide),
        1 => any::<u8>().prop_map(Fuzz::EraseDimension),
        9 => lane4_op(),
        8 => lane5_op(),
    ]
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
                style: None,
                shadows: None,
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
        Fuzz::Undo => Op::Undo,
        Fuzz::Redo => Op::Redo,
        Fuzz::Raw(op) => op.clone(),
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
}

impl Tally {
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
            "  plane snapping: {}
",
            if sawhorse_kernel::snap::plane_snapping() {
                "on"
            } else {
                "off (FUZZ_PLANE_SNAP=0)"
            }
        );
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
