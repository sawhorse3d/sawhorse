//! The cut list (lane 7): the parts a model is built from, in lumber terms.
//!
//! **What a part is.** The model's tree is walked from the top. A group or component
//! instance that holds solids and no nested group or instance is a *part container*: its
//! solids together are one piece of wood (a glue-up counts once). Anything else is an
//! *assembly* and is walked into; a solid directly in an assembly (or at the top level) is
//! a part on its own. So:
//!
//! - a component definition with only solids is a part; its quantity is the number of its
//!   instances, multiplied through every component it is nested in ([`Model::part_counts`]
//!   rolls that up through the definitions; the world tree, where nested instances are
//!   materialised, gives the same count, which the fuzzer checks);
//! - a group with only solids is a part with quantity 1, named from the Outliner;
//! - a plain solid is a part with quantity 1, named from the Outliner ("Solid 3");
//! - a solid or solid-only group inside an assembly *component* is a member part of that
//!   definition: one piece per copy of the assembly.
//!
//! Mirrored instances count in the same row and are reported as "handed". An instance
//! scaled so its size differs from its siblings gets its own row (same name, `scaled`).
//!
//! **Dimensions** are the minimal oriented bounding box of the part's solids, in the part's
//! own axes, as thickness <= width <= length. A part whose faces all lie along three
//! perpendicular directions (anything made of rectangles: boards, dados, rabbets, tenons) is
//! measured exactly along those directions. Anything else (a tapered leg, a moulding, a
//! round) is *shaped*: its box is the smallest found over candidate directions (the part's
//! own axes, its principal axes and its face normals, each with a minimum-area rectangle of
//! the 2D convex hull across it): the blank it comes out of. Only `+ - * /` and `sqrt` are
//! used, so native and WASM agree bit for bit.
//!
//! **Lumber terms.** Sizes are finished sizes, rounded to the nearest 1/16 in (or 1/32 in,
//! or 0.5 mm). Grain runs along the length unless a part says otherwise
//! ([`crate::model::Op::SetGrain`]). Material is the part's own (its first solid's) unless
//! the part names its stock ([`crate::model::Op::SetStock`]). A part is sheet goods when it
//! says so, when its material reads like sheet stock (ply, MDF, ...), or when it is 1/4 in
//! thick or less; everything else is solid lumber in board feet (T x W x L / 144, inches).
//! Sheet goods report their area and a rough count of 4 x 8 ft sheets (true nesting, kerf
//! and edge banding are later work).
//!
//! The whole list is deterministic and carries a hash ([`CutList::hash`]).

use std::collections::BTreeMap;

use glam::{DMat3, DVec2, DVec3};

use crate::hash::StableHasher;
use crate::ids::{DefId, NodeId, ObjectId, from_public, to_public};
use crate::model::Model;
use crate::nodes::{Item, Kids, MemberKind, NodeKind, Thing};
use crate::object::Object;
use crate::transform::Xform;

/// Metres per inch.
pub const M_PER_IN: f64 = 0.0254;
/// A 4 x 8 ft sheet (metres): what the rough sheet count divides by.
pub const SHEET_SIZE: [f64; 2] = [48.0 * M_PER_IN, 96.0 * M_PER_IN];
/// Parts this thin or thinner are sheet goods (1/4 in), whatever their material.
pub const SHEET_MAX_THICKNESS: f64 = 0.25 * M_PER_IN;
/// Material names that read as sheet stock (lower case, matched anywhere in the name).
pub const SHEET_WORDS: [&str; 10] = [
    "ply",
    "mdf",
    "hdf",
    "osb",
    "particle",
    "melamine",
    "hardboard",
    "masonite",
    "baltic",
    "sheet",
];
/// Directions closer than this (1 - |cos|) are the same direction.
const PARALLEL: f64 = 1e-9;
/// Most face directions tried for a shaped part's box (largest faces first).
const MAX_CANDIDATES: usize = 48;

/// Which way the grain runs on a part.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum Grain {
    /// Along the length (the default).
    #[default]
    Length,
    /// Across: along the width.
    Width,
    /// No grain to speak of (MDF, a painted part).
    None,
}

impl Grain {
    fn code(self) -> u64 {
        match self {
            Grain::Length => 0,
            Grain::Width => 1,
            Grain::None => 2,
        }
    }
}

/// What a person said about a part, beyond its geometry: grain and stock. Stored on a
/// component definition, a group, or a solid (and carried into a definition's members).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct PartAttrs {
    /// Grain direction; absent: along the length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<Grain>,
    /// The stock it is cut from, in words ("White oak", "3/4 Baltic birch ply"); replaces
    /// the material name in the cut list. Absent: the material's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock: Option<String>,
    /// Sheet goods (true) or solid lumber (false); absent: decided from the stock or
    /// material name and the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<bool>,
}

impl PartAttrs {
    pub fn is_default(&self) -> bool {
        *self == PartAttrs::default()
    }

    /// Hash (callers write it only when not the default, so older documents keep their
    /// hashes).
    pub(crate) fn hash_into(&self, h: &mut StableHasher) {
        h.tag("part");
        h.u64(self.grain.map_or(u64::MAX, Grain::code));
        match &self.stock {
            Some(s) => h.tag(s),
            None => h.u64(u64::MAX),
        }
        h.u64(self.sheet.map_or(u64::MAX, u64::from));
    }
}

/// What a grain or stock op names: a component (its definition), a group or a solid.
/// JSON: `{"definition": 2}`, `{"node": 5}` (an instance means its definition), `{"object": 3}`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum PartRef {
    Definition(u64),
    Node(u64),
    Object(u64),
}

/// How finished sizes are rounded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rounding {
    /// The nearest 1/16 in (the default).
    #[default]
    Sixteenth,
    /// The nearest 1/32 in.
    ThirtySecond,
    /// The nearest 0.5 mm (metric units).
    HalfMm,
}

impl Rounding {
    /// Rounding steps per inch.
    pub fn per_inch(self) -> f64 {
        match self {
            Rounding::Sixteenth => 16.0,
            Rounding::ThirtySecond => 32.0,
            Rounding::HalfMm => 50.8,
        }
    }

    /// One step, metres.
    pub fn step(self) -> f64 {
        match self {
            Rounding::Sixteenth => M_PER_IN / 16.0,
            Rounding::ThirtySecond => M_PER_IN / 32.0,
            Rounding::HalfMm => 0.0005,
        }
    }

    /// `x` metres in whole steps (nearest; halves away from zero).
    pub fn steps(self, x: f64) -> i64 {
        (x / self.step()).round() as i64
    }

    fn code(self) -> u64 {
        match self {
            Rounding::Sixteenth => 16,
            Rounding::ThirtySecond => 32,
            Rounding::HalfMm => 5,
        }
    }
}

/// What the cut list is asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CutOptions {
    #[serde(default)]
    pub rounding: Rounding,
    /// Count only what is drawn (hidden items, hidden tags and hidden groups left out).
    #[serde(default)]
    pub visible_only: bool,
}

/// Which part a row is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PartKey {
    /// A solid-only component definition.
    Definition(u64),
    /// A solid-only group in the world (not inside a component).
    Node(u64),
    /// A plain solid in the world (not inside a component).
    Object(u64),
    /// A solid, or solid-only group, that is a member of an assembly component.
    Member { definition: u64, slot: u32 },
}

/// The minimal box around a part, in lumber order.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PartBox {
    /// Thickness, width and length directions (unit, world).
    pub axes: [[f64; 3]; 3],
    /// Thickness <= width <= length, metres (exact, not rounded).
    pub size: [f64; 3],
    /// The box's centre (world).
    pub center: [f64; 3],
    /// Not every face lies along the box's directions: cut from this box, then shaped.
    pub shaped: bool,
}

/// One piece of a part in the model.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Occurrence {
    /// What selecting it selects: the instance or group, or the solid.
    pub item: Item,
    /// Drawn (not hidden itself, by a tag or by a hidden group around it).
    pub visible: bool,
    /// A mirrored copy (a left and a right).
    pub handed: bool,
    /// Its box in the world (grain arrows are drawn along it).
    #[serde(flatten)]
    pub bbox: PartBox,
}

/// One line of the cut list.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CutRow {
    pub key: PartKey,
    /// What the grain and stock ops name for this part.
    pub target: PartRef,
    pub name: String,
    /// Pieces counted (all of them, or the drawn ones with `visible_only`).
    pub qty: usize,
    /// Pieces in the model, drawn or not.
    pub total: usize,
    /// Pieces hidden (counted in `qty` unless `visible_only`).
    pub hidden: usize,
    /// Mirrored pieces among `total`.
    pub handed: usize,
    /// Thickness, width, length, metres, exact.
    pub size: [f64; 3],
    /// The same, rounded (finished sizes).
    pub rounded: [f64; 3],
    /// The rounded sizes in rounding steps (sixteenths, thirty-seconds or half millimetres).
    pub steps: [i64; 3],
    pub shaped: bool,
    /// A copy scaled to a size its siblings do not have (it gets a row of its own).
    pub scaled: bool,
    pub grain: Grain,
    /// The grain was set on this part (not the default).
    pub grain_set: bool,
    /// The stock it is cut from (or its material's name); `None`: not said.
    pub material: Option<String>,
    /// `material` is the part's own stock text, not its material's name.
    pub stock_set: bool,
    pub sheet: bool,
    /// Sheet or lumber was said explicitly.
    pub sheet_set: bool,
    /// Board feet for the counted pieces (0 for sheet goods), from the rounded sizes.
    pub board_feet: f64,
    /// Area of the counted pieces (sheet goods; 0 for lumber), m², from the rounded sizes.
    pub area_m2: f64,
    /// Every piece, counted or not.
    pub occurrences: Vec<Occurrence>,
}

/// Board feet of one material.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LumberTotal {
    pub material: Option<String>,
    pub board_feet: f64,
}

/// Sheet goods of one material and thickness.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SheetTotal {
    pub material: Option<String>,
    /// Thickness, metres (rounded), and in rounding steps.
    pub thickness: f64,
    pub steps: i64,
    pub area_m2: f64,
    /// Rough count of 4 x 8 ft sheets: the area divided by a sheet's, rounded up.
    pub sheets: u32,
    /// A piece is larger than a sheet in one direction.
    pub oversize: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CutTotals {
    /// Rows.
    pub parts: usize,
    /// Pieces counted.
    pub pieces: usize,
    /// Pieces hidden among those in the model.
    pub hidden: usize,
    pub board_feet: f64,
    pub lumber: Vec<LumberTotal>,
    pub sheets: Vec<SheetTotal>,
    /// Sheets of every kind together.
    pub sheet_count: u32,
}

/// The cut list ([`Model::cut_list`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CutList {
    pub rounding: Rounding,
    pub visible_only: bool,
    pub rows: Vec<CutRow>,
    pub totals: CutTotals,
    /// Width and length of the sheet the rough count assumes, metres.
    pub sheet_size: [f64; 2],
    /// Deterministic hash of everything above (hex).
    pub hash: String,
}

// ---- boxes -------------------------------------------------------------------------------

/// Points and face directions (unit normal, area) of some solids under a map.
struct Shape {
    points: Vec<DVec3>,
    normals: Vec<(DVec3, f64)>,
}

impl Shape {
    fn of(solids: &[&Object], x: &Xform) -> Shape {
        let a = x.to_affine();
        let identity = *x == Xform::IDENTITY;
        // Normals map by the inverse transpose.
        let nm = a.matrix3.inverse().transpose();
        let mut points = Vec::new();
        let mut normals = Vec::new();
        for o in solids {
            let b = o.brep();
            for (_, v) in b.vertices() {
                points.push(if identity {
                    v.pos
                } else {
                    a.transform_point3(v.pos)
                });
            }
            for (_, f) in b.faces() {
                let ring = b.loop_points(f.outer);
                // Newell area of the outer ring (holes only shrink it; weights need not be
                // exact, only ordered).
                let mut n = DVec3::ZERO;
                for k in 0..ring.len() {
                    let (p, q) = (ring[k], ring[(k + 1) % ring.len()]);
                    n += p.cross(q);
                }
                let area = 0.5 * n.length();
                let dir = if identity {
                    f.plane.normal
                } else {
                    nm * f.plane.normal
                };
                let len = dir.length();
                if len > 0.0 && len.is_finite() {
                    normals.push((dir / len, area));
                }
            }
        }
        Shape { points, normals }
    }
}

/// `v` or `-v`, whichever has its largest component positive (first of x, y, z on a tie).
fn canonical(v: DVec3) -> DVec3 {
    let a = v.abs();
    let k = if a.x >= a.y && a.x >= a.z {
        v.x
    } else if a.y >= a.z {
        v.y
    } else {
        v.z
    };
    if k < 0.0 { -v } else { v }
}

fn parallel(a: DVec3, b: DVec3) -> bool {
    1.0 - a.dot(b).abs() <= PARALLEL
}

/// Face directions, merged (either sense), with their total area, largest first.
fn directions(normals: &[(DVec3, f64)]) -> Vec<(DVec3, f64)> {
    let mut out: Vec<(DVec3, f64)> = Vec::new();
    for &(n, area) in normals {
        match out.iter_mut().find(|(d, _)| parallel(*d, n)) {
            Some((_, a)) => *a += area,
            None => out.push((canonical(n), area)),
        }
    }
    // Stable: equal areas keep their face order.
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

fn extent(points: &[DVec3], axis: DVec3) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in points {
        let d = p.dot(axis);
        lo = lo.min(d);
        hi = hi.max(d);
    }
    (lo, hi)
}

/// Two unit vectors completing `n` to a right-handed frame.
fn basis(n: DVec3) -> (DVec3, DVec3) {
    let a = n.abs();
    let helper = if a.x <= a.y && a.x <= a.z {
        DVec3::X
    } else if a.y <= a.z {
        DVec3::Y
    } else {
        DVec3::Z
    };
    let u = n.cross(helper).normalize();
    (u, n.cross(u))
}

/// Convex hull of 2D points (Andrew's monotone chain), counter-clockwise, no repeats.
fn hull(points: &mut [DVec2]) -> Vec<DVec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<DVec2> = Vec::new();
    for &p in points.iter() {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<DVec2> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The box across direction `n`: the minimum-area rectangle around the points projected on
/// the plane across `n` (rotating calipers over the hull's edges), and the extent along
/// `n`. Returns (axes, lows, highs, volume).
fn box_across(points: &[DVec3], n: DVec3) -> ([DVec3; 3], [f64; 3], [f64; 3], f64) {
    let (u, v) = basis(n);
    let mut flat: Vec<DVec2> = points
        .iter()
        .map(|p| DVec2::new(p.dot(u), p.dot(v)))
        .collect();
    let h = hull(&mut flat);
    let (nlo, nhi) = extent(points, n);
    let depth = nhi - nlo;
    // Start with the plane's own axes, then each hull edge's direction.
    let mut best: Option<(DVec2, f64, f64, f64, f64, f64)> = None;
    let mut consider = |e: DVec2| {
        let p = e.perp();
        let (mut alo, mut ahi, mut blo, mut bhi) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for q in &h {
            let (a, b) = (q.dot(e), q.dot(p));
            alo = alo.min(a);
            ahi = ahi.max(a);
            blo = blo.min(b);
            bhi = bhi.max(b);
        }
        let area = (ahi - alo) * (bhi - blo);
        if best.is_none_or(|b| area < b.5 * (1.0 - 1e-12)) {
            best = Some((e, alo, ahi, blo, bhi, area));
        }
    };
    consider(DVec2::X);
    for k in 0..h.len() {
        let e = h[(k + 1) % h.len()] - h[k];
        let len = e.length();
        if len > 0.0 {
            consider(e / len);
        }
    }
    let (e, alo, ahi, blo, bhi, area) = best.expect("the first candidate");
    let p = e.perp();
    let a3 = u * e.x + v * e.y;
    let b3 = u * p.x + v * p.y;
    ([a3, b3, n], [alo, blo, nlo], [ahi, bhi, nhi], area * depth)
}

/// Eigenvectors of a symmetric 3x3 matrix (cyclic Jacobi; sqrt and arithmetic only).
fn eigenvectors(m: DMat3) -> [DVec3; 3] {
    let mut a = [
        [m.x_axis.x, m.y_axis.x, m.z_axis.x],
        [m.x_axis.y, m.y_axis.y, m.z_axis.y],
        [m.x_axis.z, m.y_axis.z, m.z_axis.z],
    ];
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let off = a[0][1] * a[0][1] + a[0][2] * a[0][2] + a[1][2] * a[1][2];
        if off <= 1e-300 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q] == 0.0 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in &mut a {
                let (x, y) = (row[p], row[q]);
                row[p] = c * x - s * y;
                row[q] = s * x + c * y;
            }
            let (rp, rq) = (a[p], a[q]);
            for k in 0..3 {
                a[p][k] = c * rp[k] - s * rq[k];
                a[q][k] = s * rp[k] + c * rq[k];
            }
            for row in &mut v {
                let (x, y) = (row[p], row[q]);
                row[p] = c * x - s * y;
                row[q] = s * x + c * y;
            }
        }
    }
    [0, 1, 2].map(|k| DVec3::new(v[0][k], v[1][k], v[2][k]))
}

/// Principal axes of the points (unit; any that degenerate are dropped).
fn principal_axes(points: &[DVec3]) -> Vec<DVec3> {
    if points.is_empty() {
        return Vec::new();
    }
    let mean = points.iter().fold(DVec3::ZERO, |s, p| s + *p) / points.len() as f64;
    let mut c = DMat3::ZERO;
    for p in points {
        let d = *p - mean;
        c += DMat3::from_cols(d * d.x, d * d.y, d * d.z);
    }
    eigenvectors(c)
        .into_iter()
        .filter_map(|e| {
            let l = e.length();
            (l > 0.0 && l.is_finite()).then(|| e / l)
        })
        .collect()
}

/// The minimal oriented box of a shape: exact along three perpendicular face directions
/// when the shape is rectilinear, else the smallest over the candidates. `own`: the part's
/// own axes, tried first (so ties keep them).
fn min_box(shape: &Shape, own: [DVec3; 3]) -> Option<PartBox> {
    let pts = &shape.points;
    if pts.is_empty() {
        return None;
    }
    let dirs = directions(&shape.normals);
    let rectilinear = dirs.len() <= 3
        && dirs
            .iter()
            .enumerate()
            .all(|(i, a)| dirs[i + 1..].iter().all(|b| a.0.dot(b.0).abs() <= PARALLEL));
    let axes: [DVec3; 3] = if rectilinear && dirs.len() >= 2 {
        // Prefer the part's own axis where a direction is one of them (exact: no noise).
        let snap = |d: DVec3| {
            own.iter()
                .copied()
                .find(|a| parallel(*a, d))
                .map_or(d, canonical)
        };
        let a = snap(dirs[0].0);
        let b = snap(dirs[1].0);
        let c = if dirs.len() == 3 {
            snap(dirs[2].0)
        } else {
            canonical(a.cross(b).normalize())
        };
        [a, b, c]
    } else {
        let mut cands: Vec<DVec3> = own.to_vec();
        for a in principal_axes(pts) {
            if !cands.iter().any(|c| parallel(*c, a)) {
                cands.push(a);
            }
        }
        for (d, _) in dirs.iter().take(MAX_CANDIDATES) {
            if !cands.iter().any(|c| parallel(*c, *d)) {
                cands.push(*d);
            }
        }
        let mut best: Option<([DVec3; 3], f64)> = None;
        for n in cands {
            let (axes, _, _, vol) = box_across(pts, n);
            if best.is_none_or(|b| vol < b.1 * (1.0 - 1e-9)) {
                best = Some((axes, vol));
            }
        }
        let (axes, _) = best?;
        axes.map(canonical)
    };
    let mut parts: Vec<(f64, f64, DVec3)> = axes
        .iter()
        .map(|&a| {
            let (lo, hi) = extent(pts, a);
            (hi - lo, 0.5 * (lo + hi), a)
        })
        .collect();
    let center = parts
        .iter()
        .fold(DVec3::ZERO, |s, (_, mid, a)| s + *a * *mid);
    // Thickness <= width <= length (stable: equal sizes keep their order).
    parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let shaped = !dirs
        .iter()
        .all(|(d, _)| parts.iter().any(|(_, _, a)| parallel(*a, *d)));
    Some(PartBox {
        axes: [parts[0].2, parts[1].2, parts[2].2].map(|a| a.to_array()),
        size: [parts[0].0, parts[1].0, parts[2].0],
        center: center.to_array(),
        shaped,
    })
}

/// Unit columns of a placement's linear part (the part's own axes in the world).
fn own_axes(x: &Xform) -> [DVec3; 3] {
    let m = x.to_affine().matrix3;
    [m.x_axis, m.y_axis, m.z_axis].map(|c| {
        let l = c.length();
        if l > 0.0 { canonical(c / l) } else { DVec3::X }
    })
}

/// The scale of a similarity (rotation, mirror, uniform scale), or `None`.
fn similarity(x: &Xform) -> Option<f64> {
    let m = x.to_affine().matrix3;
    let g = m.transpose() * m;
    let s2 = g.x_axis.x;
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * s2.abs().max(1.0);
    let ok = close(g.y_axis.y, s2)
        && close(g.z_axis.z, s2)
        && close(g.x_axis.y, 0.0)
        && close(g.x_axis.z, 0.0)
        && close(g.y_axis.z, 0.0);
    (ok && s2 > 0.0).then(|| s2.sqrt())
}

/// A box in part coordinates, placed in the world by `x`. Similarities map the box; any
/// other map (a non-uniform scale or a shear) is measured again in the world.
fn place(local: &PartBox, x: &Xform, solids: &[&Object]) -> Option<PartBox> {
    if *x == Xform::IDENTITY {
        return Some(*local);
    }
    match similarity(x) {
        Some(s) => {
            let a = x.to_affine();
            let axes = local.axes.map(|v| {
                let w = a.matrix3 * DVec3::from_array(v);
                canonical(w / w.length()).to_array()
            });
            Some(PartBox {
                axes,
                size: local.size.map(|v| v * s),
                center: a
                    .transform_point3(DVec3::from_array(local.center))
                    .to_array(),
                shaped: local.shaped,
            })
        }
        None => min_box(&Shape::of(solids, x), own_axes(x)),
    }
}

// ---- the model's parts ---------------------------------------------------------------------

/// One piece found in the world walk.
struct Piece {
    key: PartKey,
    item: Item,
    visible: bool,
    handed: bool,
    bbox: PartBox,
}

fn is_sheet_word(s: &str) -> bool {
    let l = s.to_lowercase();
    SHEET_WORDS.iter().any(|w| l.contains(w))
}

fn qbits(x: f64) -> i64 {
    (x / 1e-7).round() as i64
}

impl Model {
    /// Set what is said about a part ([`crate::model::Op::SetGrain`],
    /// [`crate::model::Op::SetStock`]). A component instance stands for its definition;
    /// a group or solid inside a component reaches every copy through propagation.
    pub(crate) fn set_part(
        &mut self,
        part: PartRef,
        f: impl FnOnce(&mut PartAttrs),
    ) -> Result<crate::model::OpOutcome, crate::error::KernelError> {
        use crate::error::{DocumentError, StructureError};
        match part {
            PartRef::Definition(id) => {
                let d = self
                    .defs
                    .get_mut(from_public::<DefId>(id))
                    .ok_or(StructureError::NoSuchDefinition(id))?;
                f(&mut d.part);
            }
            PartRef::Node(id) => {
                let n = self.node_key(id)?;
                match self.nodes[n].kind {
                    NodeKind::Instance(d) => f(&mut self.defs[d].part),
                    NodeKind::Group => {
                        f(&mut self.nodes[n].part);
                        self.mark(self.nodes[n].member.parent);
                    }
                }
            }
            PartRef::Object(id) => {
                let o = self
                    .objects
                    .get_mut(from_public::<ObjectId>(id))
                    .ok_or(DocumentError::NoSuchObject(id))?;
                f(&mut o.part);
                let p = o.member.parent;
                self.mark(p);
            }
        }
        Ok(crate::model::OpOutcome::PartSet)
    }

    /// How many pieces of each component definition the model holds: every top-level
    /// instance counts its definition once and everything nested in it, multiplied through
    /// (a Drawer used three times with two Drawer sides each: six sides). Rolled up through
    /// the definitions, not read from the world, so it is an independent count.
    pub fn part_counts(&self) -> BTreeMap<u64, usize> {
        fn expand(m: &Model, d: DefId, mult: usize, out: &mut BTreeMap<u64, usize>, depth: u32) {
            if depth > 64 {
                return;
            }
            *out.entry(to_public(d)).or_default() += mult;
            if let Some(def) = m.defs.get(d) {
                for mem in def.members.values() {
                    if let MemberKind::Instance(e, _) = mem.kind {
                        expand(m, e, mult, out, depth + 1);
                    }
                }
            }
        }
        let mut out = BTreeMap::new();
        for n in self.nodes.values() {
            if let NodeKind::Instance(d) = n.kind
                && n.member.slot.is_none()
            {
                expand(self, d, 1, &mut out, 0);
            }
        }
        out
    }

    /// A group or instance holding solids and no group or instance: one piece of wood.
    fn is_part_container(&self, kids: &Kids, n: NodeId) -> bool {
        let things = kids.things.get(&Some(n)).map_or(&[][..], |v| v.as_slice());
        !things.iter().any(|t| matches!(t, Thing::Node(_)))
            && things.iter().any(|t| matches!(t, Thing::Object(_)))
    }

    fn solids_in(&self, kids: &Kids, n: NodeId) -> Vec<&Object> {
        kids.things
            .get(&Some(n))
            .into_iter()
            .flatten()
            .filter_map(|t| match t {
                Thing::Object(k) => Some(&self.objects[*k]),
                _ => None,
            })
            .collect()
    }

    /// The definition whose member a thing inside an instance is.
    fn member_def(&self, parent: Option<NodeId>) -> Option<(DefId, NodeId)> {
        let i = self.instance_of(parent?)?;
        match self.nodes[i].kind {
            NodeKind::Instance(d) => Some((d, i)),
            NodeKind::Group => None,
        }
    }

    fn template_solids(&self, d: DefId) -> Vec<&Object> {
        self.defs[d]
            .members
            .values()
            .filter_map(|m| match &m.kind {
                MemberKind::Solid(o) => Some(&**o),
                _ => None,
            })
            .collect()
    }

    /// Every piece, in Outliner order (each container's groups and instances by id, then
    /// its solids in creation order).
    fn pieces(&self) -> Vec<Piece> {
        let kids = self.kids();
        let mut cache: BTreeMap<PartKey, Option<PartBox>> = BTreeMap::new();
        let mut out = Vec::new();
        // Depth first, in Outliner order.
        fn walk(
            m: &Model,
            kids: &Kids,
            c: Option<NodeId>,
            cache: &mut BTreeMap<PartKey, Option<PartBox>>,
            out: &mut Vec<Piece>,
        ) {
            let things = kids.things.get(&c).cloned().unwrap_or_default();
            for t in things {
                match t {
                    Thing::Mesh(_) => {}
                    Thing::Object(k) => {
                        if let Some(p) = m.solid_piece(k, cache) {
                            out.push(p);
                        }
                    }
                    Thing::Node(n) => {
                        if m.is_part_container(kids, n) {
                            if let Some(p) = m.node_piece(kids, n, cache) {
                                out.push(p);
                            }
                        } else {
                            walk(m, kids, Some(n), cache, out);
                        }
                    }
                }
            }
        }
        walk(self, &kids, None, &mut cache, &mut out);
        out
    }

    fn solid_piece(
        &self,
        k: ObjectId,
        cache: &mut BTreeMap<PartKey, Option<PartBox>>,
    ) -> Option<Piece> {
        let o = &self.objects[k];
        let item = Item::Object(to_public(k));
        let visible = self.member_shown(&o.member, o.tag());
        match (o.member.slot, self.member_def(o.member.parent)) {
            (Some(slot), Some((d, i))) => {
                let key = PartKey::Member {
                    definition: to_public(d),
                    slot,
                };
                let x = self.nodes[i].xform;
                let template = match self.defs[d].members.get(&slot).map(|m| &m.kind) {
                    Some(MemberKind::Solid(g)) => &**g,
                    _ => o,
                };
                let local = *cache.entry(key).or_insert_with(|| {
                    min_box(
                        &Shape::of(&[template], &Xform::IDENTITY),
                        own_axes(&Xform::IDENTITY),
                    )
                });
                let bbox = place(&local?, &x, &[template])?;
                Some(Piece {
                    key,
                    item,
                    visible,
                    handed: x.determinant() < 0.0,
                    bbox,
                })
            }
            _ => {
                let bbox = min_box(
                    &Shape::of(&[o], &Xform::IDENTITY),
                    own_axes(&Xform::IDENTITY),
                )?;
                Some(Piece {
                    key: PartKey::Object(to_public(k)),
                    item,
                    visible,
                    handed: false,
                    bbox,
                })
            }
        }
    }

    fn node_piece(
        &self,
        kids: &Kids,
        n: NodeId,
        cache: &mut BTreeMap<PartKey, Option<PartBox>>,
    ) -> Option<Piece> {
        let node = &self.nodes[n];
        let item = Item::Node(to_public(n));
        let visible = self.node_shown(n);
        match node.kind {
            NodeKind::Instance(d) => {
                let key = PartKey::Definition(to_public(d));
                let solids = self.template_solids(d);
                let local = *cache.entry(key).or_insert_with(|| {
                    min_box(
                        &Shape::of(&solids, &Xform::IDENTITY),
                        own_axes(&Xform::IDENTITY),
                    )
                });
                let bbox = place(&local?, &node.xform, &solids)?;
                Some(Piece {
                    key,
                    item,
                    visible,
                    handed: node.xform.determinant() < 0.0,
                    bbox,
                })
            }
            NodeKind::Group => {
                let solids = self.solids_in(kids, n);
                let bbox = min_box(&Shape::of(&solids, &Xform::IDENTITY), own_axes(&node.xform))?;
                let (key, handed) = match (node.member.slot, self.member_def(node.member.parent)) {
                    (Some(slot), Some((d, i))) => (
                        PartKey::Member {
                            definition: to_public(d),
                            slot,
                        },
                        self.nodes[i].xform.determinant() < 0.0,
                    ),
                    _ => (PartKey::Node(to_public(n)), false),
                };
                Some(Piece {
                    key,
                    item,
                    visible,
                    handed,
                    bbox,
                })
            }
        }
    }

    /// Name, attributes and material of a part, read from its first piece.
    fn part_info(&self, key: PartKey, first: Item) -> (String, PartAttrs, Option<String>, PartRef) {
        let kids_first_solid = |n: NodeId| {
            self.object_order
                .iter()
                .find(|&&o| self.objects[o].member.parent == Some(n))
                .map(|&o| &self.objects[o])
        };
        match key {
            PartKey::Definition(id) => {
                let d = from_public::<DefId>(id);
                let def = &self.defs[d];
                let material = self
                    .template_solids(d)
                    .first()
                    .and_then(|o| o.material().map(str::to_string));
                (
                    def.name.clone(),
                    def.part.clone(),
                    material,
                    PartRef::Definition(id),
                )
            }
            PartKey::Node(id) => {
                let n = from_public::<NodeId>(id);
                let node = &self.nodes[n];
                let material = kids_first_solid(n).and_then(|o| o.material().map(str::to_string));
                (
                    node.name.clone().unwrap_or_else(|| "Group".into()),
                    node.part.clone(),
                    material,
                    PartRef::Node(id),
                )
            }
            PartKey::Object(_) | PartKey::Member { .. } => match first {
                Item::Object(oid) => {
                    let o = &self.objects[from_public::<ObjectId>(oid)];
                    let name = match (o.name(), key) {
                        (Some(n), _) => n.to_string(),
                        (None, PartKey::Member { definition, .. }) => format!(
                            "{} · solid",
                            self.defs[from_public::<DefId>(definition)].name
                        ),
                        _ => format!("Solid {}", o.number()),
                    };
                    (
                        name,
                        o.part.clone(),
                        o.material().map(str::to_string),
                        PartRef::Object(oid),
                    )
                }
                Item::Node(nid) => {
                    let n = from_public::<NodeId>(nid);
                    let node = &self.nodes[n];
                    let material =
                        kids_first_solid(n).and_then(|o| o.material().map(str::to_string));
                    (
                        node.name.clone().unwrap_or_else(|| "Group".into()),
                        node.part.clone(),
                        material,
                        PartRef::Node(nid),
                    )
                }
            },
        }
    }

    /// The cut list (see the module docs).
    pub fn cut_list(&self, opts: &CutOptions) -> CutList {
        let pieces = self.pieces();
        // Rows: one per part and size (a scaled copy gets its own), in order of first piece.
        let mut order: Vec<(PartKey, [i64; 3])> = Vec::new();
        let mut groups: BTreeMap<(PartKey, [i64; 3]), Vec<usize>> = BTreeMap::new();
        for (i, p) in pieces.iter().enumerate() {
            let k = (p.key, p.bbox.size.map(qbits));
            groups
                .entry(k)
                .or_insert_with(|| {
                    order.push(k);
                    Vec::new()
                })
                .push(i);
        }
        let mut sizes_per_key: BTreeMap<PartKey, usize> = BTreeMap::new();
        for (k, _) in &order {
            *sizes_per_key.entry(*k).or_default() += 1;
        }
        let r = opts.rounding;
        let per_in = r.per_inch();
        let mut rows = Vec::new();
        for k in &order {
            let idx = &groups[k];
            let first = &pieces[idx[0]];
            let (name, attrs, material, target) = self.part_info(first.key, first.item);
            let size = first.bbox.size;
            let steps = size.map(|s| r.steps(s));
            let rounded = steps.map(|s| s as f64 * r.step());
            let total = idx.len();
            let visible = idx.iter().filter(|&&i| pieces[i].visible).count();
            let qty = if opts.visible_only { visible } else { total };
            let handed = idx.iter().filter(|&&i| pieces[i].handed).count();
            let stock = attrs.stock.clone().filter(|s| !s.trim().is_empty());
            let stock_set = stock.is_some();
            let material = stock.or(material);
            let sheet = attrs.sheet.unwrap_or_else(|| {
                material.as_deref().is_some_and(is_sheet_word)
                    || rounded[0] <= SHEET_MAX_THICKNESS + 1e-12
            });
            let board_feet = if sheet {
                0.0
            } else {
                (steps[0] * steps[1] * steps[2]) as f64 / (per_in * per_in * per_in * 144.0)
                    * qty as f64
            };
            let area_m2 = if sheet {
                rounded[1] * rounded[2] * qty as f64
            } else {
                0.0
            };
            rows.push(CutRow {
                key: first.key,
                target,
                name,
                qty,
                total,
                hidden: if opts.visible_only {
                    0
                } else {
                    total - visible
                },
                handed,
                size,
                rounded,
                steps,
                shaped: idx.iter().any(|&i| pieces[i].bbox.shaped),
                scaled: sizes_per_key[&first.key] > 1,
                grain: attrs.grain.unwrap_or_default(),
                grain_set: attrs.grain.is_some(),
                material,
                stock_set,
                sheet,
                sheet_set: attrs.sheet.is_some(),
                board_feet,
                area_m2,
                occurrences: idx
                    .iter()
                    .map(|&i| Occurrence {
                        item: pieces[i].item,
                        visible: pieces[i].visible,
                        handed: pieces[i].handed,
                        bbox: pieces[i].bbox,
                    })
                    .collect(),
            });
        }
        if opts.visible_only {
            rows.retain(|r| r.qty > 0);
        }
        let totals = totals(&rows, r);
        let mut list = CutList {
            rounding: r,
            visible_only: opts.visible_only,
            rows,
            totals,
            sheet_size: SHEET_SIZE,
            hash: String::new(),
        };
        list.hash = format!("{:016x}", list.hash_value());
        list
    }
}

fn totals(rows: &[CutRow], r: Rounding) -> CutTotals {
    let mut lumber: BTreeMap<Option<String>, f64> = BTreeMap::new();
    let mut sheets: BTreeMap<(Option<String>, i64), (f64, bool)> = BTreeMap::new();
    for row in rows {
        if row.sheet {
            let e = sheets
                .entry((row.material.clone(), row.steps[0]))
                .or_insert((0.0, false));
            e.0 += row.area_m2;
            let over =
                row.rounded[1] > SHEET_SIZE[0] + 1e-9 || row.rounded[2] > SHEET_SIZE[1] + 1e-9;
            e.1 |= over && row.qty > 0;
        } else {
            *lumber.entry(row.material.clone()).or_default() += row.board_feet;
        }
    }
    let sheet_area = SHEET_SIZE[0] * SHEET_SIZE[1];
    let sheets: Vec<SheetTotal> = sheets
        .into_iter()
        .map(|((material, steps), (area, oversize))| SheetTotal {
            material,
            thickness: steps as f64 * r.step(),
            steps,
            area_m2: area,
            sheets: if area <= 0.0 {
                0
            } else {
                (area / sheet_area - 1e-9).ceil().max(1.0) as u32
            },
            oversize,
        })
        .collect();
    CutTotals {
        parts: rows.len(),
        pieces: rows.iter().map(|r| r.qty).sum(),
        hidden: rows
            .iter()
            .map(|r| r.total - r.occurrences.iter().filter(|o| o.visible).count())
            .sum(),
        board_feet: rows.iter().map(|r| r.board_feet).sum(),
        lumber: lumber
            .into_iter()
            .map(|(material, board_feet)| LumberTotal {
                material,
                board_feet,
            })
            .collect(),
        sheet_count: sheets.iter().map(|s| s.sheets).sum(),
        sheets,
    }
}

impl CutList {
    fn hash_value(&self) -> u64 {
        let mut h = StableHasher::default();
        h.tag("sawhorse-cut-list-v1");
        h.u64(self.rounding.code());
        h.u64(u64::from(self.visible_only));
        h.usize(self.rows.len());
        let key = |h: &mut StableHasher, k: &PartKey| match *k {
            PartKey::Definition(d) => {
                h.u64(1);
                h.u64(d);
            }
            PartKey::Node(n) => {
                h.u64(2);
                h.u64(n);
            }
            PartKey::Object(o) => {
                h.u64(3);
                h.u64(o);
            }
            PartKey::Member { definition, slot } => {
                h.u64(4);
                h.u64(definition);
                h.u64(u64::from(slot));
            }
        };
        for r in &self.rows {
            key(&mut h, &r.key);
            h.tag(&r.name);
            for v in [r.qty, r.total, r.hidden, r.handed] {
                h.usize(v);
            }
            for s in r.steps {
                h.u64(s as u64);
            }
            // Exact sizes at 0.1 micrometre: platform noise below that never shows.
            for s in r.size {
                h.u64(qbits(s) as u64);
            }
            h.u64(u64::from(r.shaped) | u64::from(r.scaled) << 1 | u64::from(r.sheet) << 2);
            h.u64(
                r.grain.code()
                    | u64::from(r.grain_set) << 2
                    | u64::from(r.stock_set) << 3
                    | u64::from(r.sheet_set) << 4,
            );
            match &r.material {
                Some(m) => h.tag(m),
                None => h.u64(u64::MAX),
            }
            h.usize(r.occurrences.len());
            for o in &r.occurrences {
                match o.item {
                    Item::Object(id) => h.u64(id << 1),
                    Item::Node(id) => h.u64(id << 1 | 1),
                }
                h.u64(u64::from(o.visible) | u64::from(o.handed) << 1);
            }
        }
        // Totals follow from the rows; the counts are hashed as a check.
        h.u64(u64::from(self.totals.sheet_count));
        h.usize(self.totals.pieces);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cuboid(lo: DVec3, hi: DVec3) -> Shape {
        let mut points = Vec::new();
        for x in [lo.x, hi.x] {
            for y in [lo.y, hi.y] {
                for z in [lo.z, hi.z] {
                    points.push(DVec3::new(x, y, z));
                }
            }
        }
        let s = hi - lo;
        let normals = vec![
            (DVec3::X, s.y * s.z),
            (-DVec3::X, s.y * s.z),
            (DVec3::Y, s.x * s.z),
            (-DVec3::Y, s.x * s.z),
            (DVec3::Z, s.x * s.y),
            (-DVec3::Z, s.x * s.y),
        ];
        Shape { points, normals }
    }

    const OWN: [DVec3; 3] = [DVec3::X, DVec3::Y, DVec3::Z];

    #[test]
    fn a_board_is_measured_exactly_in_lumber_order() {
        let b = min_box(
            &cuboid(
                DVec3::ZERO,
                DVec3::new(0.75 * M_PER_IN, 11.25 * M_PER_IN, 71.25 * M_PER_IN),
            ),
            OWN,
        )
        .unwrap();
        assert_eq!(
            b.size,
            [0.75 * M_PER_IN, 11.25 * M_PER_IN, 71.25 * M_PER_IN]
        );
        assert_eq!(b.axes, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!(!b.shaped);
    }

    #[test]
    fn a_rotated_board_finds_its_own_directions() {
        // A 1 x 2 x 4 board turned 30 degrees about blue.
        let (c, s) = (3f64.sqrt() / 2.0, 0.5);
        let rot = |p: DVec3| DVec3::new(c * p.x - s * p.y, s * p.x + c * p.y, p.z);
        let base = cuboid(DVec3::ZERO, DVec3::new(4.0, 2.0, 1.0));
        let shape = Shape {
            points: base.points.iter().map(|p| rot(*p)).collect(),
            normals: base.normals.iter().map(|(n, a)| (rot(*n), *a)).collect(),
        };
        let b = min_box(&shape, OWN).unwrap();
        for (got, want) in b.size.iter().zip([1.0, 2.0, 4.0]) {
            assert!((got - want).abs() < 1e-12, "{:?}", b.size);
        }
        assert!(!b.shaped);
    }

    #[test]
    fn a_tapered_leg_is_shaped_and_comes_out_of_its_base_square() {
        // 2 x 2 at the foot, 1.2 x 1.2 at the top, 28 tall.
        let foot = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
        let top = [(0.4, 0.4), (1.6, 0.4), (1.6, 1.6), (0.4, 1.6)];
        let mut points: Vec<DVec3> = foot.iter().map(|&(x, y)| DVec3::new(x, y, 0.0)).collect();
        points.extend(top.iter().map(|&(x, y)| DVec3::new(x, y, 28.0)));
        let slant = |n: DVec3| n.normalize();
        let normals = vec![
            (-DVec3::Z, 4.0),
            (DVec3::Z, 1.44),
            (slant(DVec3::new(0.0, -28.0, 0.4)), 45.0),
            (slant(DVec3::new(28.0, 0.0, 0.4)), 45.0),
            (slant(DVec3::new(0.0, 28.0, 0.4)), 45.0),
            (slant(DVec3::new(-28.0, 0.0, 0.4)), 45.0),
        ];
        let b = min_box(&Shape { points, normals }, OWN).unwrap();
        assert!(b.shaped);
        for (got, want) in b.size.iter().zip([2.0, 2.0, 28.0]) {
            assert!((got - want).abs() < 1e-12, "{:?}", b.size);
        }
    }

    #[test]
    fn a_hexagon_prism_gets_its_smallest_box() {
        // A regular hexagon of circumradius 1, 5 long: the box is sqrt(3) x 2 x 5.
        let mut points = Vec::new();
        let mut normals = Vec::new();
        let r3 = 3f64.sqrt() / 2.0;
        let ring = [
            (1.0, 0.0),
            (0.5, r3),
            (-0.5, r3),
            (-1.0, 0.0),
            (-0.5, -r3),
            (0.5, -r3),
        ];
        for z in [0.0, 5.0] {
            points.extend(ring.iter().map(|&(x, y)| DVec3::new(x, y, z)));
        }
        for k in 0..6 {
            let (a, b) = (ring[k], ring[(k + 1) % 6]);
            let mid = DVec3::new(a.0 + b.0, a.1 + b.1, 0.0).normalize();
            normals.push((mid, 5.0));
        }
        normals.push((DVec3::Z, 2.6));
        normals.push((-DVec3::Z, 2.6));
        let b = min_box(&Shape { points, normals }, OWN).unwrap();
        assert!(b.shaped);
        for (got, want) in b.size.iter().zip([2.0 * r3, 2.0, 5.0]) {
            assert!((got - want).abs() < 1e-9, "{:?}", b.size);
        }
    }

    #[test]
    fn rounding_and_board_feet() {
        let r = Rounding::Sixteenth;
        assert_eq!(r.steps(11.25 * M_PER_IN), 180);
        assert_eq!(r.steps(0.7501 * M_PER_IN), 12);
        assert_eq!(
            r.steps(0.72 * M_PER_IN),
            12,
            "11.52 sixteenths rounds to 12"
        );
        assert_eq!(
            Rounding::ThirtySecond.steps(0.7344 * M_PER_IN),
            24,
            "23.5 thirty-seconds rounds up"
        );
        assert_eq!(
            Rounding::HalfMm.steps(0.01905),
            38,
            "19.05 mm is 38 half-millimetres"
        );
        assert!(
            is_sheet_word("3/4 Baltic birch ply")
                && is_sheet_word("MDF")
                && !is_sheet_word("White oak")
        );
    }

    #[test]
    fn principal_axes_are_unit_and_perpendicular() {
        let pts: Vec<DVec3> = (0..20)
            .map(|k| {
                let t = k as f64;
                DVec3::new(t, 0.5 * t + (t * 0.7).sin(), 0.1 * (t * 1.3).cos())
            })
            .collect();
        let ax = principal_axes(&pts);
        assert_eq!(ax.len(), 3);
        for i in 0..3 {
            assert!((ax[i].length() - 1.0).abs() < 1e-12);
            for j in i + 1..3 {
                assert!(ax[i].dot(ax[j]).abs() < 1e-9);
            }
        }
    }
}
