//! `check_clearance` and `measure` (#14): how two parts sit against each other. Both only
//! read: the intersection is worked out on copies of the solids, so the model, its hash and
//! its undo steps stay as they were.
//!
//! A part is an item from `summary` (`{"object": id}` or `{"node": id}`). A node stands for
//! every solid inside it, at any depth, where it sits in the world (an instance's solids are
//! its world copies already).

use sawhorse_kernel::DVec3;
use sawhorse_kernel::boolean::BoolOp;
use sawhorse_kernel::cutlist::CutOptions;
use sawhorse_kernel::model::Model;
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::object::Object;
use sawhorse_kernel::tolerance::{LENGTH_EPS, PARALLEL_EPS, POINT_MERGE};
use serde_json::{Value, json};

/// Most triangle pairs one call compares, the two parts' triangles multiplied. The gap and
/// the facing faces compare every triangle of one part with every triangle of the other
/// (for the gap, with a box check that skips most of them); past this a call could run for
/// minutes, so it is refused before any work. Measured 2026-11-20 on a Windows 11 machine, release
/// build in WSL Ubuntu 24.04 (`pair_cost` below): 4,000,000 pairs compared one by one, none
/// skipped, took 0.45 s, about 113 ns a pair, or about 2.3 s at the limit at that rate (the
/// intersections have a limit of their own, below); two plates of 2,000 slivers 1 mm apart,
/// where the box checks skip most pairs, took 7 ms.
pub const MAX_TRIANGLE_PAIRS: usize = 20_000_000;

/// Most triangles `check_clearance`'s intersections may take in, added up over every pair of
/// solids (one from each part) whose boxes meet, both solids' triangles counted for each
/// pair. The pair limit above does not bound them: two groups of 372 overlapping boards
/// (19,927,296 pairs, 138,384 intersections, 3,321,216 triangles) took 11.6 s, and a disc of
/// 19,996 triangles against a group of 83 cubes in it (19,916,016 pairs, 1,660,664
/// triangles) 5.1 s (2026-11-20, release build in WSL Ubuntu 24.04, over stdin). So past this
/// a call is refused before any work too. The slowest accepted calls found, the same way with
/// nothing else running: 3.5 s (two groups of 204 overlapping boards, 998,784 triangles) and
/// 6.9 s (two groups of 117 ten-sided prisms crossing, 985,608) when the intersections built;
/// 9.4 s when none could be built either way round, so each was worked twice (the 204 boards
/// 0.01 mm into each other and turned 0.3 rad about x).
pub const MAX_INTERSECTION_TRIANGLES: usize = 1_000_000;

/// Most facing face pairs `measure` lists (nearest first); the answer says how many more.
pub const MAX_FACING: usize = 50;

/// Two parts are seated when the gap and the overlap's thinnest side are both within the
/// kernel's merge distance, with the noise floor for rounding (both from `tolerance.rs`).
const SEATED: f64 = POINT_MERGE + LENGTH_EPS;

/// One part as named, with its solids.
struct Part<'m> {
    item: Item,
    name: Option<String>,
    solids: Vec<(u64, &'m Object)>,
    hidden: bool,
}

fn item_value(item: &Item) -> Value {
    serde_json::to_value(item).unwrap_or(Value::Null)
}

/// The part an item names, or why not.
fn part<'m>(model: &'m Model, item: &Item, which: &str) -> Result<Part<'m>, String> {
    let shown = serde_json::to_string(item).unwrap_or_default();
    let solids: Vec<(u64, &Object)> = match *item {
        Item::Object(id) => vec![(
            id,
            model.object(id).ok_or_else(|| {
                format!("{which}: no solid {shown} in the model (summary lists the items)")
            })?,
        )],
        Item::Node(id) => {
            if model.node(id).is_none() {
                return Err(format!(
                    "{which}: no group or component {shown} in the model (summary lists the items)"
                ));
            }
            model
                .objects()
                .into_iter()
                .filter(|(_, o)| inside(model, o.membership().parent(), id))
                .collect()
        }
    };
    if solids.is_empty() {
        return Err(format!("{which}: {shown} holds no solid to measure"));
    }
    let name = model
        .cut_list(&CutOptions::default())
        .rows
        .iter()
        .find(|r| r.occurrences.iter().any(|o| o.item == *item))
        .map(|r| r.name.clone())
        .or_else(|| match *item {
            Item::Node(id) => model.node(id).and_then(|n| n.name.clone()),
            // A solid inside a group is no row of its own: its own name.
            Item::Object(id) => model.object(id).and_then(|o| o.name()).map(str::to_string),
        });
    let hidden = match *item {
        Item::Node(id) => !model.node_visible(id),
        Item::Object(_) => false,
    } || solids.iter().any(|(id, _)| !model.is_visible(*id));
    Ok(Part {
        item: *item,
        name,
        solids,
        hidden,
    })
}

/// `parent` is `node` or inside it.
fn inside(model: &Model, mut parent: Option<u64>, node: u64) -> bool {
    while let Some(p) = parent {
        if p == node {
            return true;
        }
        parent = model.node(p).and_then(|n| n.member.parent());
    }
    false
}

/// Both parts, refused when they are the same or share a solid.
fn pair<'m>(model: &'m Model, a: &Item, b: &Item) -> Result<(Part<'m>, Part<'m>), String> {
    if a == b {
        return Err("a and b name the same part: name two different items".into());
    }
    let pa = part(model, a, "a")?;
    let pb = part(model, b, "b")?;
    if pa
        .solids
        .iter()
        .any(|(i, _)| pb.solids.iter().any(|(j, _)| i == j))
    {
        return Err(
            "one of a and b is inside the other (they hold the same solids): name two parts apart"
                .into(),
        );
    }
    Ok((pa, pb))
}

fn describe(p: &Part) -> Value {
    json!({
        "item": item_value(&p.item),
        "name": p.name,
        "solids": p.solids.len(),
        "hidden": p.hidden,
    })
}

/// A part's triangles in world coordinates, each with the solid and persistent face it is on.
struct Tris {
    tris: Vec<[DVec3; 3]>,
    /// (object, face id) per triangle.
    on: Vec<(u64, u64)>,
    /// Outward normal of each triangle's face.
    normal: Vec<DVec3>,
    /// The face's plane origin, for distances between parallel faces.
    origin: Vec<DVec3>,
}

fn triangles(p: &Part) -> Result<Tris, String> {
    let mut t = Tris {
        tris: Vec::new(),
        on: Vec::new(),
        normal: Vec::new(),
        origin: Vec::new(),
    };
    for (id, o) in &p.solids {
        let s = o
            .tri_solid()
            .map_err(|e| format!("solid {id} could not be read as triangles: {e}"))?;
        let planes: Vec<_> = o.brep().faces().map(|(_, f)| f.plane).collect();
        for (tri, &fi) in s.triangles.iter().zip(&s.face_ids) {
            let fi = fi as usize;
            t.tris.push(tri.map(|v| s.positions[v as usize]));
            t.on.push((*id, o.face_id_at(fi).unwrap_or(0)));
            t.normal.push(planes[fi].normal);
            t.origin.push(planes[fi].origin);
        }
    }
    Ok(t)
}

/// Refuse before work when the two parts' triangles are too many to compare.
fn check_cost(a: &Tris, b: &Tris) -> Result<(), String> {
    let pairs = a.tris.len().saturating_mul(b.tris.len());
    if pairs > MAX_TRIANGLE_PAIRS {
        return Err(format!(
            "these parts have {} and {} triangles, {pairs} pairs to compare, more than the {MAX_TRIANGLE_PAIRS} a call may (it could take minutes); measure smaller parts, or the solids of a group one at a time",
            a.tris.len(),
            b.tris.len()
        ));
    }
    Ok(())
}

/// Millimetres from metres, to the nearest nanometre (a gap is read at micrometres).
fn mm(m: f64) -> f64 {
    (m * 1e9).round() / 1e6 + 0.0
}

fn mm3(m3: f64) -> f64 {
    (m3 * 1e12).round() / 1e3 + 0.0
}

fn point_mm(p: DVec3) -> Value {
    json!([mm(p.x), mm(p.y), mm(p.z)])
}

/// `check_clearance(a, b)`.
pub fn clearance(model: &Model, a: &Item, b: &Item) -> Result<Value, String> {
    let (pa, pb) = pair(model, a, b)?;
    let (ta, tb) = (triangles(&pa)?, triangles(&pb)?);
    check_cost(&ta, &tb)?;
    // The solid pairs whose boxes meet are the intersections to work out: refuse before
    // any work when they would take in too many triangles.
    let counts = |t: &Tris| {
        let mut n = std::collections::HashMap::new();
        for (o, _) in &t.on {
            *n.entry(*o).or_insert(0usize) += 1;
        }
        n
    };
    let (na, nb) = (counts(&ta), counts(&tb));
    let bounds_b: Vec<_> = pb.solids.iter().map(|(_, o)| o.bounds()).collect();
    let mut meeting = Vec::new();
    let mut taken = 0usize;
    for (i, oa) in &pa.solids {
        let bounds_a = oa.bounds();
        for ((j, ob), &bb) in pb.solids.iter().zip(&bounds_b) {
            if boxes_meet(bounds_a, bb) {
                taken += na.get(i).copied().unwrap_or(0) + nb.get(j).copied().unwrap_or(0);
                meeting.push(((*i, *oa), (*j, *ob)));
            }
        }
    }
    if taken > MAX_INTERSECTION_TRIANGLES {
        return Err(format!(
            "the overlap would be worked out for {} pairs of solids whose boxes meet, taking in {taken} triangles, more than the {MAX_INTERSECTION_TRIANGLES} a call may (it could take minutes); measure smaller parts, or the solids of a group one at a time",
            meeting.len()
        ));
    }
    let (gap, pa_at, pb_at) = gap(&ta.tris, &tb.tris);
    // The overlap: every solid of a against every solid of b, on copies.
    let mut volume = 0.0;
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    let mut unknown: Option<String> = None;
    for ((i, oa), (j, ob)) in meeting {
        // The kernel's intersection can fail one way round and work the other (a shelf
        // whose end sits on a dado's floor: a.combine(b) refused, b.combine(a) right); the
        // volume and the box are the same either way, so the other order is tried before
        // giving up, and the first order's reason is the one kept.
        match oa
            .combine(ob, BoolOp::Intersection)
            .or_else(|e| ob.combine(oa, BoolOp::Intersection).map_err(|_| e))
        {
            Ok(Some(o)) => {
                volume += o.volume();
                let (l, h) = o.bounds();
                lo = lo.min(l);
                hi = hi.max(h);
            }
            // Empty, or thinner than the kernel keeps (it drops an intersection of
            // about the merge distance's depth): no overlap to report.
            Ok(None) => {}
            // The kernel could not build the intersection either way round (cases seen are
            // listed in docs/MCP.md). The gap still stands; the overlap and seated are not
            // known, and the answer gives the first pair's reason, from the first order.
            Err(e) => {
                unknown.get_or_insert_with(|| {
                    format!("the overlap of solids {i} and {j} could not be worked out: {e}")
                });
            }
        }
    }
    let size = (volume > 0.0).then(|| hi - lo);
    // A part wholly inside another meets none of its surfaces: the overlap says they are
    // not apart, so the gap is 0, at the middle of the overlap.
    let (gap, pa_at, pb_at) = if size.is_some() && gap > 0.0 {
        let mid = (lo + hi) / 2.0;
        (0.0, mid, mid)
    } else {
        (gap, pa_at, pb_at)
    };
    // The overlap's box is along the world axes, so for parts turned to them its thinnest
    // side can only overstate how deep the overlap is.
    let seated = gap <= SEATED && size.is_none_or(|s| s.min_element() <= SEATED);
    let mut answer = json!({
        "a": describe(&pa),
        "b": describe(&pb),
        "gap_mm": mm(gap),
        "closest_mm": [point_mm(pa_at), point_mm(pb_at)],
        "overlap_mm3": mm3(volume),
        "overlap_size_mm": size.map(point_mm),
        "seated": seated,
        "merge_mm": mm(POINT_MERGE),
    });
    if let Some(why) = unknown {
        answer["overlap_mm3"] = Value::Null;
        answer["overlap_size_mm"] = Value::Null;
        answer["seated"] = Value::Null;
        answer["overlap_unknown"] = Value::String(why);
    }
    Ok(answer)
}

fn boxes_meet((la, ha): (DVec3, DVec3), (lb, hb): (DVec3, DVec3)) -> bool {
    la.cmple(hb + DVec3::splat(SEATED)).all() && lb.cmple(ha + DVec3::splat(SEATED)).all()
}

/// A face of a and a face of b looking at each other: the distance, a's (object, face id),
/// b's, and a's outward normal.
type Facing = (f64, (u64, u64), (u64, u64), DVec3);

/// `measure(a, b)`: the distance between each pair of parallel flat faces, one from each
/// part, that face each other with some area in common.
pub fn measure(model: &Model, a: &Item, b: &Item) -> Result<Value, String> {
    let (pa, pb) = pair(model, a, b)?;
    let (ta, tb) = (triangles(&pa)?, triangles(&pb)?);
    check_cost(&ta, &tb)?;
    let mut found: Vec<Facing> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for i in 0..ta.tris.len() {
        let na = ta.normal[i];
        for j in 0..tb.tris.len() {
            let nb = tb.normal[j];
            // Parallel (the kernel's PARALLEL_EPS, on the sine) and opposite: the faces look
            // at each other or away.
            if na.dot(nb) >= 0.0 || na.cross(nb).length() >= PARALLEL_EPS {
                continue;
            }
            let d = na.dot(tb.origin[j] - ta.origin[i]);
            if d < -SEATED {
                continue;
            }
            let key = (ta.on[i], tb.on[j]);
            if seen.contains(&key) {
                continue;
            }
            if overlap_across(&ta.tris[i], &tb.tris[j], na) {
                seen.insert(key);
                found.push((d.max(0.0), ta.on[i], tb.on[j], na));
            }
        }
    }
    found.sort_by(|x, y| x.0.total_cmp(&y.0));
    let more = found.len().saturating_sub(MAX_FACING);
    let facing: Vec<Value> = found
        .iter()
        .take(MAX_FACING)
        .map(|(d, fa, fb, n)| {
            json!({
                "distance_mm": mm(*d),
                "a_face": {"object": fa.0, "face": fa.1},
                "b_face": {"object": fb.0, "face": fb.1},
                "normal": [round9(n.x), round9(n.y), round9(n.z)],
            })
        })
        .collect();
    Ok(json!({
        "a": describe(&pa),
        "b": describe(&pb),
        "facing": facing,
        "more": more,
    }))
}

fn round9(x: f64) -> f64 {
    (x * 1e9).round() / 1e9 + 0.0
}

/// The two triangles, seen along `n`, cover some area in common (more than touching).
fn overlap_across(a: &[DVec3; 3], b: &[DVec3; 3], n: DVec3) -> bool {
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let flat = |t: &[DVec3; 3]| t.map(|p| (p.dot(u), p.dot(v)));
    let (a, b) = (flat(a), flat(b));
    // Separating axis test on the six edge normals; a separation of at least the merge
    // distance (or touching) is no common area.
    for t in [&a, &b] {
        for k in 0..3 {
            let (p, q) = (t[k], t[(k + 1) % 3]);
            let axis = (q.1 - p.1, p.0 - q.0);
            let len = (axis.0 * axis.0 + axis.1 * axis.1).sqrt();
            if len == 0.0 {
                continue;
            }
            let proj = |s: &[(f64, f64); 3]| {
                let d: Vec<f64> = s
                    .iter()
                    .map(|w| (w.0 * axis.0 + w.1 * axis.1) / len)
                    .collect();
                (
                    d.iter().copied().fold(f64::INFINITY, f64::min),
                    d.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                )
            };
            let ((amin, amax), (bmin, bmax)) = (proj(&a), proj(&b));
            if amax <= bmin + SEATED || bmax <= amin + SEATED {
                return false;
            }
        }
    }
    true
}

/// The shortest distance between two sets of triangles, and a point on each where it is.
/// Exact over the triangles: 0 when they touch or cross.
pub fn gap(a: &[[DVec3; 3]], b: &[[DVec3; 3]]) -> (f64, DVec3, DVec3) {
    let boxes = |t: &[[DVec3; 3]]| -> Vec<(DVec3, DVec3)> {
        t.iter()
            .map(|t| (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2])))
            .collect()
    };
    let (ba, bb) = (boxes(a), boxes(b));
    let mut best = (f64::INFINITY, DVec3::ZERO, DVec3::ZERO);
    for (i, ta) in a.iter().enumerate() {
        for (j, tb) in b.iter().enumerate() {
            let sep = (ba[i].0 - bb[j].1).max(bb[j].0 - ba[i].1).max(DVec3::ZERO);
            if sep.length() >= best.0 {
                continue;
            }
            let d = tri_tri(ta, tb);
            if d.0 < best.0 {
                best = d;
                if best.0 == 0.0 {
                    return best;
                }
            }
        }
    }
    best
}

/// Distance between two triangles and the closest points.
fn tri_tri(a: &[DVec3; 3], b: &[DVec3; 3]) -> (f64, DVec3, DVec3) {
    // An edge of one through the other: they cross.
    for (s, t) in [(a, b), (b, a)] {
        for k in 0..3 {
            if let Some(p) = segment_through(s[k], s[(k + 1) % 3], t) {
                return (0.0, p, p);
            }
        }
    }
    let mut best = (f64::INFINITY, DVec3::ZERO, DVec3::ZERO);
    let mut keep = |d: f64, p: DVec3, q: DVec3| {
        if d < best.0 {
            best = (d, p, q);
        }
    };
    for i in 0..3 {
        for j in 0..3 {
            let (p, q) = seg_seg(a[i], a[(i + 1) % 3], b[j], b[(j + 1) % 3]);
            keep(p.distance(q), p, q);
        }
    }
    for p in a {
        let q = point_tri(*p, b);
        keep(p.distance(q), *p, q);
    }
    for q in b {
        let p = point_tri(*q, a);
        keep(p.distance(*q), p, *q);
    }
    best
}

/// Where segment p-q passes through triangle t, if it does (not when they are coplanar).
fn segment_through(p: DVec3, q: DVec3, t: &[DVec3; 3]) -> Option<DVec3> {
    let d = q - p;
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let h = d.cross(e2);
    let det = e1.dot(h);
    let scale = e1.length() * e2.length() * d.length();
    if det.abs() <= 1e-12 * scale {
        return None;
    }
    let f = 1.0 / det;
    let s = p - t[0];
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = s.cross(e1);
    let v = f * d.dot(qv);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let w = f * e2.dot(qv);
    (0.0..=1.0).contains(&w).then(|| p + d * w)
}

/// Closest points of segments p1-q1 and p2-q2 (Ericson, Real-Time Collision Detection 5.1.9).
fn seg_seg(p1: DVec3, q1: DVec3, p2: DVec3, q2: DVec3) -> (DVec3, DVec3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let tiny = 1e-30;
    let (s, t);
    if a <= tiny && e <= tiny {
        return (p1, p2);
    }
    if a <= tiny {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= tiny {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > tiny {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    (p1 + d1 * s, p2 + d2 * t)
}

/// The point of triangle t closest to p (Ericson 5.1.5).
fn point_tri(p: DVec3, t: &[DVec3; 3]) -> DVec3 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [DVec3; 3] {
        [a, b, c].map(DVec3::from_array)
    }

    #[test]
    fn triangles_apart_crossing_and_touching() {
        let a = tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        // Parallel, 0.5 above.
        let b = tri([0.0, 0.0, 0.5], [1.0, 0.0, 0.5], [0.0, 1.0, 0.5]);
        assert!((tri_tri(&a, &b).0 - 0.5).abs() < 1e-15);
        // Standing up through a's middle: they cross.
        let c = tri([0.2, 0.2, -1.0], [0.2, 0.2, 1.0], [0.3, 0.1, 0.0]);
        assert_eq!(tri_tri(&a, &c).0, 0.0);
        // Coplanar, overlapping: 0.
        let d = tri([0.1, 0.1, 0.0], [2.0, 0.1, 0.0], [0.1, 2.0, 0.0]);
        assert_eq!(tri_tri(&a, &d).0, 0.0);
        // Edge to edge, skew: 1 apart.
        let e = tri([0.5, -1.0, 1.0], [0.5, -1.0, -1.0], [0.5, -2.0, 0.0]);
        assert!((tri_tri(&a, &e).0 - 1.0).abs() < 1e-15);
    }

    /// The gap the box checks skip to is the one checking every pair gives.
    #[test]
    fn the_box_checks_skip_nothing_that_matters() {
        let mut seed: u64 = 14;
        let mut r = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..200 {
            let mut soup = |off: f64| -> Vec<[DVec3; 3]> {
                (0..20)
                    .map(|_| [0, 1, 2].map(|_| DVec3::new(r() + off, r(), r()) * 0.1))
                    .collect()
            };
            // Far apart, and so close that the nearest pair is a hair nearer than many others.
            for off in [1.5, 1.001] {
                let (a, b) = (soup(0.0), soup(off));
                let fast = gap(&a, &b).0;
                let every = a
                    .iter()
                    .flat_map(|x| b.iter().map(move |y| tri_tri(x, y).0))
                    .fold(f64::INFINITY, f64::min);
                assert_eq!(fast, every);
            }
        }
    }

    /// What one pair costs, to set MAX_TRIANGLE_PAIRS. Run with
    /// `cargo test --release -p sawhorse-mcp --lib -- --ignored --nocapture pair_cost`.
    #[test]
    #[ignore]
    fn pair_cost() {
        let n = 2000;
        let mk = |z: f64| -> Vec<[DVec3; 3]> {
            (0..n)
                .map(|i| {
                    let x = i as f64 * 1e-3;
                    tri([x, 0.0, z], [x + 1e-3, 0.0, z], [x, 1.0, z])
                })
                .collect()
        };
        // Two plates of slivers 1 mm apart: every pair's box is within the best so far
        // until the very end, so few are skipped.
        let (a, b) = (mk(0.0), mk(0.001));
        let t = std::time::Instant::now();
        let g = gap(&a, &b);
        eprintln!("{} pairs in {:?} (gap {})", n * n, t.elapsed(), g.0);
        let t = std::time::Instant::now();
        let mut best = f64::INFINITY;
        for x in &a {
            for y in &b {
                best = best.min(tri_tri(x, y).0);
            }
        }
        eprintln!(
            "{} pairs, none skipped, in {:?} ({best})",
            n * n,
            t.elapsed()
        );
    }
}
