//! Triangulation and 2D polygon operations for Sawhorse.
//!
//! This crate knows nothing about the B-rep: it works on plain 2D rings (a face's loops
//! projected into the face's plane frame). The kernel depends on it, not the other way
//! round, so booleans, rendering and export all share one triangulator.
//!
//! - [`triangulate`]: `earcut` (georust) with hole rings, followed by an area-deviation
//!   check. When earcut's triangles do not cover the polygon (it can mis-bridge a hole
//!   into an outer ring that has collinear vertices), our own ear clipper
//!   ([`ear_clip`]) runs instead and must pass the same check. A triangulation that
//!   still fails is a typed [`TessellateError`], never a silent repair (ADR-0001).
//! - [`overlay`]: thin wrappers over `i_overlay` for coplanar merge / imprint tests.

pub mod overlay;

use glam::DVec2;

/// Largest accepted relative difference between the polygon area and the summed
/// triangle area, measured by `area_deviation` (areas accurate to their own size; lanes 22
/// and 23).
pub const MAX_AREA_DEVIATION: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TessellateError {
    #[error("a ring needs at least 3 points, got {0}")]
    TooFewPoints(usize),
    #[error("ring contains a non-finite coordinate")]
    NonFinite,
    #[error("polygon has (near) zero area")]
    ZeroArea,
    #[error("triangulation area deviates by {deviation:e} (limit {MAX_AREA_DEVIATION:e})")]
    AreaDeviation { deviation: f64 },
    #[error("too many vertices for u32 indices")]
    TooLarge,
}

/// Signed area of a ring (positive = counter-clockwise).
///
/// Summed about the ring's first point (lane 22): the same value, but its rounding scales
/// with the ring's own size, not with how far it lies from the frame's origin. Summed about
/// the origin, a 0.0044 m2 triangle 7 km out lost 1.5e-9 m2 (3e-7 of itself) to products
/// of 5,000 m coordinates.
pub fn signed_area(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    let Some(&o) = ring.first() else {
        return 0.0;
    };
    let mut a = 0.0;
    for i in 1..n.saturating_sub(1) {
        let (p, q) = (ring[i] - o, ring[i + 1] - o);
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

/// Triangulate a polygon with holes.
///
/// Indices refer to the concatenation `outer ++ holes[0] ++ holes[1] ++ ...`.
/// Output triangles are counter-clockwise in the 2D frame regardless of input winding.
pub fn triangulate(
    outer: &[DVec2],
    holes: &[Vec<DVec2>],
) -> Result<Vec<[u32; 3]>, TessellateError> {
    for ring in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)) {
        if ring.len() < 3 {
            return Err(TessellateError::TooFewPoints(ring.len()));
        }
        if ring.iter().any(|p| !p.is_finite()) {
            return Err(TessellateError::NonFinite);
        }
    }
    let total: usize = outer.len() + holes.iter().map(Vec::len).sum::<usize>();
    if total > u32::MAX as usize {
        return Err(TessellateError::TooLarge);
    }
    let mut data: Vec<[f64; 2]> = Vec::with_capacity(total);
    let mut hole_indices: Vec<u32> = Vec::with_capacity(holes.len());
    data.extend(outer.iter().map(|p| [p.x, p.y]));
    for h in holes {
        hole_indices.push(data.len() as u32);
        data.extend(h.iter().map(|p| [p.x, p.y]));
    }
    // "Near zero" against the polygon's own size (lane 22): scaled by the largest coordinate
    // instead, a 17 micrometre quadrilateral 5 km out (3e-10 m2, a fair region) read as zero
    // area and the view could not be built. The area itself is the polygon's net area to its
    // own size ([`polygon_area2`], fix pass of lane 23), not the outer ring's area less the
    // holes', each rounded to the size of the ring: a band 70 micrometres wide round a 9 km
    // circle is 4 m2 between two rings of 2.5e8 m2.
    let (lo, hi) = outer.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let scale = (hi - lo).max_element().max(1e-300);
    if polygon_area2(&data, &hole_indices) * 0.5 <= scale * scale * 1e-15 {
        return Err(TessellateError::ZeroArea);
    }

    let mut out: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(data.iter().copied(), &hole_indices, &mut out);
    let bad = |out: &[u32]| -> Option<f64> {
        let deviation = area_deviation(&data, &hole_indices, out);
        (deviation.is_nan() || deviation > MAX_AREA_DEVIATION || out.is_empty())
            .then_some(deviation)
    };
    if let Some(deviation) = bad(&out) {
        // Fallback: bridge the holes and clip ears ourselves. Only polygons earcut gets
        // wrong reach it, so every face earcut handles keeps its exact triangles (and the
        // golden hashes built on them).
        let pts: Vec<DVec2> = data.iter().map(|&p| DVec2::from(p)).collect();
        out = ear_clip(&pts, outer.len(), &hole_indices)
            .map(|t| t.into_iter().flatten().collect())
            .unwrap_or_default();
        if bad(&out).is_some() {
            return Err(TessellateError::AreaDeviation { deviation });
        }
    }
    let mut tris = Vec::with_capacity(out.len() / 3);
    for t in out.as_chunks::<3>().0 {
        let (a, b, c) = (
            data[t[0] as usize],
            data[t[1] as usize],
            data[t[2] as usize],
        );
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        tris.push(if cross >= 0.0 {
            [t[0], t[1], t[2]]
        } else {
            [t[0], t[2], t[1]]
        });
    }
    cover_all_vertices(&data, &mut tris);
    Ok(tris)
}

/// Twice the signed area of triangle abc, accurate to its own size wherever the points are,
/// however thin the triangle ([`ring_area2`] of the three).
fn area2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    ring_area2(&[a, b, c])
}

/// Twice a ring's signed area, accurate to its own size however long and thin the ring is
/// (lane 23). The shoelace sum's products are split exactly into a value and its rounding
/// error (`two_product`, by a fused multiply-add) and all of them summed with Neumaier's
/// compensated sum, whose error is a rounding of the result plus a term in the square of
/// the machine epsilon times the terms. Lane 22 measured both sides of the check with the
/// robust orientation predicate, on the belief that its value is accurate to the triangle's
/// own size; only its sign is (its value may be off by a rounding of the products it is made
/// of). A sliver 1.5e-5 m wide bent round a corner of a 9 km nonagon (the region between it
/// and a copy turned 1e-7 degrees) is 0.14 m2: as a fan from its first point, triangles of
/// 5e6 m2 either way whose roundings were 1e-8 of it, and earcut's correct triangles, 0.06
/// m2 each, came back 1.2e-8 off. Correct triangles failed the check and the view could not
/// be built.
fn ring_area2(ring: &[[f64; 2]]) -> f64 {
    let mut sum = Neumaier::default();
    add_shoelace(&mut sum, ring, 1.0);
    sum.value()
}

/// Add a ring's shoelace terms, times `sign` (1 or -1, so exactly), to `sum`: each product
/// split exactly into its rounded value and that rounding's error.
fn add_shoelace(sum: &mut Neumaier, ring: &[[f64; 2]], sign: f64) {
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let (p, e) = two_product(a[0], b[1]);
        let (q, f) = two_product(a[1], b[0]);
        sum.add(sign * p);
        sum.add(sign * e);
        sum.add(-sign * q);
        sum.add(-sign * f);
    }
}

/// Twice the net area of a polygon with holes (`data` is `outer ++ holes...`, each hole from
/// its index in `hole_indices`): the outer ring counted positive and every hole negative,
/// whatever their winding, all in ONE compensated sum, so the result is accurate to the
/// polygon's own area (fix pass of lane 23). Summed ring by ring, each ring's area is
/// accurate only to the ring's own size: a band 6.8e-5 m wide between a 9 km 15-gon and its
/// offset is 3.9 m2 between two rings of 2.4e8 m2 each, and their difference came out 2.3e-8
/// m2 off, 5.9e-9 of the band. earcut's 30 triangles were right (their areas sum to the
/// band's exactly, in rational arithmetic) and the area check refused them.
fn polygon_area2(data: &[[f64; 2]], hole_indices: &[u32]) -> f64 {
    let bounds: Vec<usize> = std::iter::once(0)
        .chain(hole_indices.iter().map(|&h| h as usize))
        .chain(std::iter::once(data.len()))
        .collect();
    let mut sum = Neumaier::default();
    for (k, w) in bounds.windows(2).enumerate() {
        let ring = &data[w[0]..w[1]];
        if ring.len() < 3 {
            continue;
        }
        // The ring's own orientation (its sign is right: the value is accurate to its size).
        let own = ring_area2(ring);
        let positive = if k == 0 { own >= 0.0 } else { own < 0.0 };
        add_shoelace(&mut sum, ring, if positive { 1.0 } else { -1.0 });
    }
    sum.value()
}

/// `a * b` as `p + e` exactly (`p` the rounded product, `e` its error), for finite products.
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

/// Neumaier's compensated sum (Kahan-Babuska): the running sum and the low-order parts it
/// lost, added back at the end.
#[derive(Debug, Default, Clone, Copy)]
struct Neumaier {
    sum: f64,
    lost: f64,
}

impl Neumaier {
    fn add(&mut self, x: f64) {
        let t = self.sum + x;
        self.lost += if self.sum.abs() >= x.abs() {
            (self.sum - t) + x
        } else {
            (x - t) + self.sum
        };
        self.sum = t;
    }

    fn value(&self) -> f64 {
        self.sum + self.lost
    }
}

/// `earcut::deviation`'s measure, |polygon area - triangles' area| / polygon area, with
/// every area worked out to its own size (lane 22; lane 23: exactly split products and a
/// compensated sum, [`ring_area2`]). earcut's own sums products of
/// absolute coordinates: a sliver 10 km long, or a region 5 km from its sketch's origin,
/// lost more to rounding than the 1e-9 the check allows, and correct triangles failed it.
/// Here each term is accurate to its own size, so the check stays as strict and measures
/// the triangulation, not the arithmetic.
fn area_deviation(data: &[[f64; 2]], hole_indices: &[u32], tris: &[u32]) -> f64 {
    let polygon = polygon_area2(data, hole_indices);
    let mut triangles = Neumaier::default();
    for t in tris.as_chunks::<3>().0 {
        triangles.add(
            area2(
                data[t[0] as usize],
                data[t[1] as usize],
                data[t[2] as usize],
            )
            .abs(),
        );
    }
    let triangles = triangles.value();
    if polygon == 0.0 && triangles == 0.0 {
        return 0.0;
    }
    (polygon - triangles).abs() / polygon.abs()
}

/// earcut drops points that are exactly collinear with their neighbours. A B-rep face keeps
/// such points (an edge split where a neighbouring face attaches), and the neighbour's
/// triangles use them, so dropping them would leave T-junctions: a mesh that is not
/// manifold (booleans refuse it) and an STL that is not watertight. Re-insert every unused
/// point by splitting the triangle whose edge it lies on.
///
/// A point that is a second copy of a point the triangles do use (two holes, or a hole and
/// the outer ring, touching there; earcut drops the copy) is already covered: the mesh has
/// a vertex at that place. Splitting a triangle "on" it took the nearest edge that projects
/// inside, which is not an edge it lies on, and made a clockwise triangle over the others
/// (fix pass of lane 23).
fn cover_all_vertices(data: &[[f64; 2]], tris: &mut Vec<[u32; 3]>) {
    let mut used = vec![false; data.len()];
    for t in tris.iter() {
        for &i in t {
            used[i as usize] = true;
        }
    }
    // + 0.0 makes -0.0 the same key as 0.0, as `==` has it.
    let key = |q: [f64; 2]| ((q[0] + 0.0).to_bits(), (q[1] + 0.0).to_bits());
    let at_used: std::collections::HashSet<(u64, u64)> = used
        .iter()
        .enumerate()
        .filter(|(_, u)| **u)
        .map(|(i, _)| key(data[i]))
        .collect();
    for (v, _) in used.iter().enumerate().filter(|(_, u)| !**u) {
        if at_used.contains(&key(data[v])) {
            continue;
        }
        let p = DVec2::from(data[v]);
        // The triangle edge (a, b) that `p` sits on: smallest distance, projecting inside.
        let mut best: Option<(f64, usize, usize)> = None;
        for (ti, t) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (
                    DVec2::from(data[t[k] as usize]),
                    DVec2::from(data[t[(k + 1) % 3] as usize]),
                );
                let ab = b - a;
                let len2 = ab.length_squared();
                if len2 == 0.0 {
                    continue;
                }
                let s = (p - a).dot(ab) / len2;
                if s <= 0.0 || s >= 1.0 {
                    continue;
                }
                let d = (a + ab * s).distance(p);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, ti, k));
                }
            }
        }
        if let Some((_, ti, k)) = best {
            let t = tris[ti];
            let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            tris[ti] = [a, v as u32, c];
            tris.push([v as u32, b, c]);
        }
    }
}

/// Orientation of `a`, `b`, `c`, exactly: positive when they turn counter-clockwise,
/// negative clockwise, zero when collinear (Shewchuk's adaptive predicate). The fallback
/// decides every turn with it (fix pass of lane 23), so its answers do not depend on how
/// far the polygon lies from the frame's origin.
fn orient(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    robust::orient2d(
        robust::Coord { x: a.x, y: a.y },
        robust::Coord { x: b.x, y: b.y },
        robust::Coord { x: c.x, y: c.y },
    )
}

/// Whether `p` and `q` lie on the same side of `o` along one line through it (`o`, `p`, `q`
/// already known collinear, `p` and `q` both different from `o`): compared coordinate by
/// coordinate, exactly.
fn same_way(o: DVec2, p: DVec2, q: DVec2) -> bool {
    let side = |a: f64, b: f64| i8::from(a > b) - i8::from(a < b);
    side(p.x, o.x) == side(q.x, o.x) && side(p.y, o.y) == side(q.y, o.y)
}

/// Whether the directions from `o` to `x` and to `y` are the same direction.
fn same_dir(o: DVec2, x: DVec2, y: DVec2) -> bool {
    orient(o, x, y) == 0.0 && same_way(o, x, y)
}

/// Whether the direction `o -> x` comes strictly before `o -> y` turning counter-clockwise
/// from the direction `o -> s` (which itself counts as the start, angle 0). Exact.
fn before_from(o: DVec2, s: DVec2, x: DVec2, y: DVec2) -> bool {
    // 0: angles in [0, pi) from s; 1: [pi, 2 pi).
    let half = |p: DVec2| {
        let t = orient(o, s, p);
        u8::from(!(t > 0.0 || (t == 0.0 && same_way(o, s, p))))
    };
    let (hx, hy) = (half(x), half(y));
    if hx != hy {
        return hx < hy;
    }
    orient(o, x, y) > 0.0
}

/// Whether the direction `o -> d` lies strictly inside the open wedge that turns
/// counter-clockwise from `o -> s` to `o -> e` (all the way round, less the ray itself, when
/// `e` lies along `s`).
fn in_wedge(o: DVec2, s: DVec2, e: DVec2, d: DVec2) -> bool {
    if same_dir(o, s, d) {
        return false;
    }
    if same_dir(o, s, e) {
        return true;
    }
    before_from(o, s, d, e)
}

/// Whether two open wedges at `o` (each turning counter-clockwise from its first direction
/// to its second, neither empty) share a direction.
fn wedges_overlap(o: DVec2, a: (DVec2, DVec2), b: (DVec2, DVec2)) -> bool {
    same_dir(o, a.0, b.0) || in_wedge(o, a.0, a.1, b.0) || in_wedge(o, b.0, b.1, a.0)
}

/// The polygon's interior wedge at position `j` of the counter-clockwise ring `poly`: from
/// the direction to the next point that is not on top of it, counter-clockwise round to the
/// direction to the previous one. `None` when every other point of the ring is on top of it.
fn ring_wedge(pts: &[DVec2], poly: &[u32], j: usize) -> Option<(DVec2, DVec2)> {
    let n = poly.len();
    let q = pts[poly[j] as usize];
    let next = (1..n)
        .map(|s| pts[poly[(j + s) % n] as usize])
        .find(|&p| p != q)?;
    let prev = (1..n)
        .map(|s| pts[poly[(j + n - s) % n] as usize])
        .find(|&p| p != q)?;
    Some((next, prev))
}

/// Whether the open segment `a b` and the segment `u v` meet anywhere but at `a` or `b`
/// themselves: a proper crossing, or `u` or `v` strictly inside `a b`. Exact.
fn segment_blocked(a: DVec2, b: DVec2, u: DVec2, v: DVec2) -> bool {
    let (o1, o2) = (orient(a, b, u), orient(a, b, v));
    let (o3, o4) = (orient(u, v, a), orient(u, v, b));
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    let strictly_inside =
        |p: DVec2, o: f64| o == 0.0 && p != a && p != b && same_way(a, b, p) && same_way(b, a, p);
    strictly_inside(u, o1) || strictly_inside(v, o2)
}

/// Ear clipping for a polygon with holes, the fallback behind [`triangulate`].
///
/// `pts` is `outer ++ holes...`; `hole_starts` gives where each hole begins. The outer ring
/// is taken counter-clockwise and the holes clockwise (whatever their input winding). Each
/// hole, rightmost first, is joined to the ring by a bridge from its rightmost vertex to a
/// ring vertex it can see ([`bridge_target`]). The bridged ring is a single weakly simple
/// polygon; ears are clipped from it, and a vertex collinear with its neighbours is dropped
/// without a triangle (the caller's `cover_all_vertices` puts it back on the edge it lies
/// on). Returns `None` when no ear can be found.
///
/// Every decision is made with exact predicates (fix pass of lane 23), and the ear test
/// knows that a point can occur twice: at a bridge's two ends, and where two holes (or a
/// hole and the outer ring) touch at a vertex. A candidate ear `a b c` is refused when any
/// other vertex lies in the closed triangle, when an edge crosses the diagonal `a c`, or
/// when another occurrence of `a`, `b` or `c` opens its interior wedge into the triangle's
/// corner there. The fuzzer found the last case (a 700 m circle drawn round two slivers
/// that touch at a point): the old test skipped every point on top of a corner, and clipped
/// an ear across the other sliver.
pub fn ear_clip(pts: &[DVec2], outer_len: usize, hole_starts: &[u32]) -> Option<Vec<[u32; 3]>> {
    let ring_area = |r: &[u32]| {
        let q: Vec<[f64; 2]> = r.iter().map(|&i| pts[i as usize].to_array()).collect();
        ring_area2(&q)
    };
    let mut poly: Vec<u32> = (0..outer_len as u32).collect();
    if ring_area(&poly) < 0.0 {
        poly.reverse();
    }
    let mut holes: Vec<Vec<u32>> = Vec::with_capacity(hole_starts.len());
    for (k, &start) in hole_starts.iter().enumerate() {
        let end = hole_starts.get(k + 1).map_or(pts.len() as u32, |&e| e);
        let mut h: Vec<u32> = (start..end).collect();
        if ring_area(&h) > 0.0 {
            h.reverse();
        }
        holes.push(h);
    }
    merge_touching_rings(pts, &mut poly, &mut holes);
    let rightmost = |h: &[u32]| -> usize {
        let mut best = 0;
        for (k, &i) in h.iter().enumerate() {
            let (p, b) = (pts[i as usize], pts[h[best] as usize]);
            if p.x > b.x || (p.x == b.x && p.y < b.y) {
                best = k;
            }
        }
        best
    };
    holes.sort_by(|a, b| {
        let (pa, pb) = (pts[a[rightmost(a)] as usize], pts[b[rightmost(b)] as usize]);
        pb.x.total_cmp(&pa.x).then(pa.y.total_cmp(&pb.y))
    });
    for (hi, h) in holes.iter().enumerate() {
        // A merged hole can pass its rightmost point twice: bridge from an occurrence that
        // sees out.
        let m = pts[h[rightmost(h)] as usize];
        let (r, k) = (0..h.len())
            .filter(|&r| pts[h[r] as usize] == m)
            .find_map(|r| bridge_target(pts, &poly, h, r, &holes[hi + 1..]).map(|k| (r, k)))?;
        let mut next = Vec::with_capacity(poly.len() + h.len() + 2);
        next.extend_from_slice(&poly[..=k]);
        next.extend(h[r..].iter().chain(&h[..r]).copied());
        next.push(h[r]);
        next.push(poly[k]);
        next.extend_from_slice(&poly[k + 1..]);
        poly = next;
    }

    let mut tris = Vec::with_capacity(poly.len());
    while poly.len() > 3 {
        let n = poly.len();
        let mut clipped = false;
        for i in 0..n {
            let (pa, pc) = ((i + n - 1) % n, (i + 1) % n);
            let (ia, ib, ic) = (poly[pa], poly[i], poly[pc]);
            let (a, b, c) = (pts[ia as usize], pts[ib as usize], pts[ic as usize]);
            let turn = orient(a, b, c);
            if turn == 0.0 {
                poly.remove(i);
                clipped = true;
                break;
            }
            if turn < 0.0 {
                continue;
            }
            if !ear_blocked(pts, &poly, [pa, i, pc]) {
                tris.push([ia, ib, ic]);
                poly.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            return None;
        }
    }
    if poly.len() == 3 {
        let (a, b, c) = (
            pts[poly[0] as usize],
            pts[poly[1] as usize],
            pts[poly[2] as usize],
        );
        if orient(a, b, c) > 0.0 {
            tris.push([poly[0], poly[1], poly[2]]);
        }
    }
    Some(tris)
}

/// Rings that touch at a point (two holes, or a hole and the outer ring, with a vertex at
/// exactly the same place) become one ring, for [`ear_clip`] (fix pass of lane 23).
///
/// Bridged as they are, such rings pass through the shared point twice with interior
/// wedges that overlap (each hole's outside holds the other hole), which is not a weakly
/// simple polygon, and ear clipping has no right answer on it. Swapping the two rings'
/// successors at the shared point (`a[..=i] ++ b[i'+1..] ++ b[..=i'] ++ a[i+1..]`) gives
/// one ring round both whose two passes through the point bound the two gaps between them:
/// wedges that do not overlap. Rings already merged are not split again where they meet a
/// second time.
fn merge_touching_rings(pts: &[DVec2], outer: &mut Vec<u32>, holes: &mut Vec<Vec<u32>>) {
    let key = |i: u32| {
        let p = pts[i as usize];
        // + 0.0 makes -0.0 the same key as 0.0, as `==` has it.
        ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits())
    };
    let mut rings: Vec<Vec<u32>> = std::iter::once(std::mem::take(outer))
        .chain(std::mem::take(holes))
        .collect();
    'again: loop {
        let mut seen: std::collections::HashMap<(u64, u64), (usize, usize)> =
            std::collections::HashMap::new();
        for (ri, ring) in rings.iter().enumerate() {
            for (pi, &i) in ring.iter().enumerate() {
                match seen.get(&key(i)) {
                    Some(&(rj, pj)) if rj != ri => {
                        let (a, b) = (&rings[rj], &rings[ri]);
                        let mut merged = Vec::with_capacity(a.len() + b.len());
                        merged.extend_from_slice(&a[..=pj]);
                        merged.extend_from_slice(&b[pi + 1..]);
                        merged.extend_from_slice(&b[..=pi]);
                        merged.extend_from_slice(&a[pj + 1..]);
                        rings[rj] = merged;
                        rings.remove(ri);
                        continue 'again;
                    }
                    Some(_) => {}
                    None => {
                        seen.insert(key(i), (ri, pi));
                    }
                }
            }
        }
        break;
    }
    let mut it = rings.into_iter();
    *outer = it.next().unwrap_or_default();
    *holes = it.collect();
}

/// Whether the counter-clockwise ear at positions `at = [a, b, c]` of `poly` may not be
/// clipped: see [`ear_clip`].
fn ear_blocked(pts: &[DVec2], poly: &[u32], at: [usize; 3]) -> bool {
    let n = poly.len();
    let corner = at.map(|k| pts[poly[k] as usize]);
    let [a, b, c] = corner;
    // The triangle's own wedge at each corner (it is a counter-clockwise ring too).
    let corner_wedge = [(b, c), (c, a), (a, b)];
    for (j, &iq) in poly.iter().enumerate() {
        if at.contains(&j) {
            continue;
        }
        let q = pts[iq as usize];
        if let Some(k) = corner.iter().position(|&x| x == q) {
            match ring_wedge(pts, poly, j) {
                Some(w) if !wedges_overlap(q, corner_wedge[k], w) => {}
                _ => return true,
            }
            continue;
        }
        if orient(a, b, q) >= 0.0 && orient(b, c, q) >= 0.0 && orient(c, a, q) >= 0.0 {
            return true;
        }
    }
    // The diagonal: no edge may cross it (an edge through a corner, into the triangle and out
    // across `a c`, has no vertex inside).
    (0..n).any(|e| {
        let (u, v) = (pts[poly[e] as usize], pts[poly[(e + 1) % n] as usize]);
        segment_blocked(a, c, u, v)
    })
}

/// Position in the counter-clockwise ring `poly` of a vertex that hole `hole`'s vertex at
/// `r` (its rightmost) can be bridged to, for [`ear_clip`]; `others` are the holes not yet
/// bridged.
///
/// First the classic choice: cast a ray in +x, take the nearest edge it hits and that
/// edge's right end, and if a reflex vertex lies inside the triangle (hole vertex, hit
/// point, end) take the reflex vertex nearest the ray's direction instead. That choice is
/// then checked exactly (fix pass of lane 23): the bridge must leave the hole outwards,
/// arrive inside the ring's wedge at the vertex it lands on, and cross or touch nothing on
/// the way (the ring, this hole, the holes still to come). When it fails, every ring vertex
/// is tried, nearest first, and the first that passes is taken.
fn bridge_target(
    pts: &[DVec2],
    poly: &[u32],
    hole: &[u32],
    r: usize,
    others: &[Vec<u32>],
) -> Option<usize> {
    let n = poly.len();
    let m = pts[hole[r] as usize];
    let hole_wedge = ring_wedge(pts, hole, r)?;
    let edges = || {
        std::iter::once(poly)
            .chain(std::iter::once(hole))
            .chain(others.iter().map(Vec::as_slice))
            .flat_map(|ring| {
                let k = ring.len();
                (0..k).map(move |e| (pts[ring[e] as usize], pts[ring[(e + 1) % k] as usize]))
            })
    };
    // Whether the bridge from `m` may land at position `j`.
    let lands = |j: usize| -> bool {
        let t = pts[poly[j] as usize];
        if t == m {
            // The hole touches the ring here: the hole must sit inside the ring's wedge.
            return ring_wedge(pts, poly, j).is_some_and(|w| {
                in_wedge(m, w.0, w.1, hole_wedge.0) && in_wedge(m, w.0, w.1, hole_wedge.1)
            });
        }
        in_wedge(m, hole_wedge.0, hole_wedge.1, t)
            && ring_wedge(pts, poly, j).is_some_and(|w| in_wedge(t, w.0, w.1, m))
            && !edges().any(|(u, v)| segment_blocked(m, t, u, v))
    };
    if let Some(k) = classic_bridge(pts, poly, m) {
        // A vertex can appear twice (an earlier bridge, or rings that touch): any occurrence
        // the bridge lands in will do.
        let id = poly[k];
        if let Some(j) = std::iter::once(k)
            .chain((0..n).filter(|&j| j != k && poly[j] == id))
            .find(|&j| lands(j))
        {
            return Some(j);
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| {
        let (p, q) = (pts[poly[i] as usize], pts[poly[j] as usize]);
        p.distance_squared(m)
            .total_cmp(&q.distance_squared(m))
            .then(i.cmp(&j))
    });
    order.into_iter().find(|&j| lands(j))
}

/// The classic bridge choice of [`bridge_target`], unchecked.
fn classic_bridge(pts: &[DVec2], poly: &[u32], m: DVec2) -> Option<usize> {
    let n = poly.len();
    let at = |k: usize| pts[poly[k % n] as usize];
    // Nearest crossing of the ray y = m.y, x >= m.x with a ring edge.
    let mut hit: Option<(f64, usize)> = None;
    for k in 0..n {
        let (a, b) = (at(k), at(k + 1));
        if (a.y > m.y) == (b.y > m.y) && a.y != m.y && b.y != m.y {
            continue;
        }
        let x = if a.y == b.y {
            if a.y != m.y {
                continue;
            }
            a.x.min(b.x)
        } else {
            a.x + (m.y - a.y) / (b.y - a.y) * (b.x - a.x)
        };
        if x < m.x {
            continue;
        }
        if hit.is_none_or(|(hx, _)| x < hx) {
            hit = Some((x, k));
        }
    }
    let (hx, k) = hit?;
    let hp = DVec2::new(hx, m.y);
    // The hit edge's end with the larger x (or the vertex the ray hit exactly).
    let (a, b) = (at(k), at(k + 1));
    let mut target = if a == hp {
        k
    } else if b == hp {
        (k + 1) % n
    } else if a.x >= b.x {
        k
    } else {
        (k + 1) % n
    };
    let p = at(target);
    if p != hp {
        // A reflex vertex inside the triangle (m, hit, p) would block the bridge: take the
        // one closest in angle to the ray (then the nearest) instead.
        let (t0, t1, t2) = if orient(m, hp, p) >= 0.0 {
            (m, hp, p)
        } else {
            (m, p, hp)
        };
        let mut best: Option<(f64, f64, usize)> = None;
        for j in 0..n {
            let q = at(j);
            if j == target || q == p || q == m {
                continue;
            }
            let reflex = orient(at(j + n - 1), q, at(j + 1)) < 0.0;
            if !reflex {
                continue;
            }
            let inside =
                orient(t0, t1, q) >= 0.0 && orient(t1, t2, q) >= 0.0 && orient(t2, t0, q) >= 0.0;
            if !inside {
                continue;
            }
            let d = q - m;
            let len = d.length();
            if len == 0.0 {
                continue;
            }
            let cos = d.x / len;
            if best.is_none_or(|(bc, bl, _)| cos > bc || (cos == bc && len < bl)) {
                best = Some((cos, len, j));
            }
        }
        if let Some((_, _, j)) = best {
            target = j;
        }
    }
    Some(target)
}

/// A point strictly inside a polygon with holes (even-odd), as far from the boundary as a
/// cheap scanline search finds, or `None` for an empty or degenerate polygon.
///
/// Scanlines run halfway between consecutive distinct vertex heights (so no vertex lies on
/// one); each yields interior intervals between pairs of crossings. The midpoint of the
/// interval with the best clearance wins. The point sits at the golden-section fraction of
/// its interval and gap rather than the exact middle, so it rarely lands where a user later
/// draws through a midpoint (which would make it a boundary point). Deterministic.
pub fn interior_point(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Option<DVec2> {
    let rings: Vec<&[DVec2]> = std::iter::once(outer)
        .chain(holes.iter().map(Vec::as_slice))
        .collect();
    if outer.len() < 3 {
        return None;
    }
    let mut ys: Vec<f64> = rings.iter().flat_map(|r| r.iter().map(|p| p.y)).collect();
    if ys.iter().any(|y| !y.is_finite()) {
        return None;
    }
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut gaps: Vec<(f64, f64)> = ys.windows(2).map(|w| (w[1] - w[0], w[0])).collect();
    // Widest gaps first; a handful of scanlines is plenty.
    gaps.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
    gaps.truncate(16);
    let mut best: Option<(f64, DVec2)> = None;
    const GOLDEN: f64 = 0.381_966_011_250_105_1;
    for (gap, lo) in gaps {
        if gap <= 0.0 {
            continue;
        }
        let y = lo + gap * GOLDEN;
        let mut xs: Vec<f64> = Vec::new();
        for r in &rings {
            let n = r.len();
            for i in 0..n {
                let (a, b) = (r[i], r[(i + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let w = pair[1] - pair[0];
            if w <= 0.0 {
                continue;
            }
            let score = (GOLDEN * w).min(GOLDEN * gap);
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, DVec2::new(pair[0] + w * GOLDEN, y)));
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Lane 22: shapes far from the origin are judged by their own size. A 17 micrometre
    /// square 5 km out is not "zero area", a 10 km sliver triangle 5 km out passes the area
    /// check, and a ring's area does not depend on where it lies.
    #[test]
    fn far_small_and_long_thin_polygons_triangulate() {
        let small = rect(5001.25, -5000.0, 1.7e-5, 1.7e-5);
        let t = triangulate(&small, &[]).unwrap();
        assert_eq!(t.len(), 2);
        let sliver = vec![
            DVec2::new(-5000.0, -5000.0),
            DVec2::new(5001.25, -4999.999982544526),
            DVec2::new(-5000.0, -4999.99996508905),
        ];
        assert_eq!(triangulate(&sliver, &[]).unwrap().len(), 1);
        // Corners exact in binary, so only the sum's rounding could tell the two apart.
        let here = signed_area(&rect(0.0, 0.0, 0.09375, 0.09375));
        let there = signed_area(&rect(5001.96875, -5000.375, 0.09375, 0.09375));
        assert_eq!(here, 0.0087890625);
        assert_eq!(there, here);
        // A frame with a hole, far out: robust areas, an exact check.
        let outer = rect(4000.0, 4000.0, 2.0, 2.0);
        let mut hole = rect(4000.5, 4000.5, 1.0, 1.0);
        hole.reverse();
        let t = triangulate(&outer, &[hole]).unwrap();
        assert!(t.len() >= 8);
    }

    #[test]
    fn a_sliver_bent_round_a_far_corner_triangulates() {
        // Lane 23's property-test find: a 9 km nonagon and its copy turned 1e-7 degrees about
        // a corner bound a 0.14 m2 sliver 1.5e-5 m wide bent round the next corner. earcut's
        // three triangles are right (a fan from the last point, 0.117 + 0.062 + 0.106 m2,
        // twice over, checked in exact arithmetic), but the robust predicate's values came
        // back 1.2e-8 off and the check refused them.
        let p = |x: f64, y: f64| DVec2::new(x, y);
        let sliver = [
            p(6894.3999880708025, 5785.088487178853),
            p(1562.8335990023738, 8863.269777109872),
            p(-4499.999999999998, 7794.2286340599485),
            p(-4500.000017222627, 7794.228613534819),
            p(1562.83358353305, 8863.269764129569),
        ];
        let t = triangulate(&sliver, &[]).unwrap();
        assert_eq!(t.len(), 3);
        // The same on the other side of the nonagon.
        let other = [
            p(-4500.000000000004, -7794.228634059946),
            p(-4499.999986396509, -7794.22865762189),
            p(1562.8336144716936, -8863.269790090177),
            p(6894.3999880708, -5785.088487178857),
            p(1562.8335990023697, -8863.269777109874),
        ];
        assert_eq!(triangulate(&other, &[]).unwrap().len(), 3);
        // The ring's area to its own size: twice it is 0.2850115512220417 exactly rounded.
        let data: Vec<[f64; 2]> = sliver.iter().map(|q| [q.x, q.y]).collect();
        assert!((ring_area2(&data) - 0.2850115512220417).abs() <= 1e-16);
        // A wrong triangulation of it still fails: a fan from its first point covers
        // 2.4e7 m2 twice over.
        assert!(area_deviation(&data, &[], &[0, 1, 2, 0, 2, 3, 0, 3, 4]) > 1.0);
    }

    /// The 9 km 15-gon and its offset 6.8e-5 m in (the fix pass of lane 23: `curve_props`
    /// at 100,000 cases, seed 7303): a band of 3.84 m2 between two rings of 2.47e8 m2 each.
    fn far_band() -> (Vec<DVec2>, Vec<DVec2>) {
        let p = |x: f64, y: f64| DVec2::new(x, y);
        let outer = vec![
            p(9000.0, 0.0),
            p(8221.909118783407, 3660.6297876822014),
            p(6022.175457229724, 6688.303429296548),
            p(2781.152949374527, 8559.508646656383),
            p(-940.7561694088799, 8950.697058314461),
            p(-4499.999999999998, 7794.2286340599485),
            p(-7281.152949374526, 5290.067270632259),
            p(-8803.32840660425, 1871.2052173598338),
            p(-8803.32840660425, -1871.2052173598315),
            p(-7281.152949374528, -5290.0672706322575),
            p(-4500.000000000004, -7794.228634059946),
            p(-940.7561694088881, -8950.69705831446),
            p(2781.152949374525, -8559.508646656383),
            p(6022.175457229726, -6688.303429296546),
            p(8221.90911878341, -3660.6297876822014),
        ];
        let hole = vec![
            p(8999.999930074986, 0.0),
            p(8221.90905490373, -3660.629759241136),
            p(6022.175410440759, -6688.303377332134),
            p(2781.1529277665077, -8559.508580153744),
            p(-940.756162099733, -8950.696988772503),
            p(-4499.999965037499, -7794.228573503106),
            p(-7281.152892804004, -5290.067229531365),
            p(-8803.328338207262, -1871.2052028216071),
            p(-8803.328338207262, 1871.2052028216096),
            p(-7281.152892804001, 5290.067229531368),
            p(-4499.999965037492, 7794.228573503111),
            p(-940.7561620997258, 8950.696988772504),
            p(2781.1529277665095, 8559.508580153744),
            p(6022.175410440757, 6688.303377332136),
            p(8221.909054903728, 3660.629759241136),
        ];
        (outer, hole)
    }

    #[test]
    fn a_thin_band_round_a_far_circle_triangulates() {
        // Each ring's area to its own size (2.47e8 m2) and the band's as their difference
        // lost 2.3e-8 m2: 5.9e-9 of the band, over the 1e-9 the check allows. Measured in one
        // compensated sum the net area is right to its own size and earcut's triangles pass.
        let (outer, hole) = far_band();
        let holes = vec![hole];
        let t = triangulate(&outer, &holes).unwrap();
        assert_eq!(t.len(), 30);
        let data: Vec<[f64; 2]> = outer
            .iter()
            .chain(holes[0].iter())
            .map(|q| [q.x, q.y])
            .collect();
        // Twice the band's area (2 x 3.84 m2): 7.679087624588129 is the exact value rounded
        // to the nearest double (worked out in rational arithmetic).
        let net = polygon_area2(&data, &[15]);
        assert_eq!(net, 7.679087624588129);
        // The ring-by-ring difference the check used to make is off by far more.
        let apart = ring_area2(&data[..15]).abs() - ring_area2(&data[15..]).abs();
        assert!((apart - net).abs() > 1e-8, "{apart} vs {net}");
        let flat: Vec<u32> = t.iter().flatten().copied().collect();
        assert!(area_deviation(&data, &[15], &flat) <= 1e-14);
        // Either winding of either ring, the same net area.
        let mut back = data.clone();
        back[..15].reverse();
        back[15..].reverse();
        assert!((polygon_area2(&back, &[15]) - net).abs() <= 2e-15);
    }

    /// The fuzzer's find in the fix pass of lane 23 (seed 20267102, plane snapping on;
    /// `tests/repro/2026-10-19-circle-drawn-over-slivers-of-a-moved-quad.json`): a 700 m
    /// nonagon round two triangular holes that touch at one point.
    fn holes_touching_at_a_point() -> (Vec<DVec2>, Vec<Vec<DVec2>>) {
        let p = |x: f64, y: f64| DVec2::new(x, y);
        let outer = vec![
            p(-147.5939192308382, -684.2631328707406),
            p(326.77236190967136, -619.0475131134723),
            p(648.2382232423729, -264.17268202365574),
            p(666.3862155546074, 214.31148293735538),
            p(372.7246915510117, 592.5169232251542),
            p(-95.3388580028314, 693.477110043811),
            p(-518.7922963237692, 469.9516499333846),
            p(-699.4970534606831, 26.530589888318648),
            p(-552.8993652395416, -429.3044280201545),
        ];
        let touch = p(0.1472180087078695, -1.2299763471267182);
        let holes = vec![
            vec![
                p(0.0625, -0.9246396336907776),
                p(0.812504, -0.42464063369077754),
                touch,
            ],
            vec![
                p(0.8125, -3.627754810377919),
                p(-0.1875, -1.6351560290542821),
                touch,
            ],
        ];
        (outer, holes)
    }

    #[test]
    fn holes_that_touch_at_a_point_triangulate() {
        // earcut drops the second occurrence of the shared point, so the second hole is
        // left with two points and its 0.54 m2 is covered by triangles; the old fallback
        // bridged the holes one by one, so the ring passed the shared point twice with
        // overlapping wedges, and it clipped a triangle of -80 m2. Merged into one ring
        // first, the holes triangulate exactly.
        let (outer, holes) = holes_touching_at_a_point();
        let all: Vec<DVec2> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        let starts = [9, 12];
        let t = ear_clip(&all, outer.len(), &starts).expect("an ear at every step");
        let data: Vec<[f64; 2]> = all.iter().map(|q| q.to_array()).collect();
        let flat: Vec<u32> = t.iter().flatten().copied().collect();
        assert!(area_deviation(&data, &starts, &flat) <= 1e-14);
        assert!(t.iter().all(|t| tri_area(&all, t) > 0.0));
        let t = triangulate(&outer, &holes).unwrap();
        assert_covers(&outer, &holes, &t);
        // The holes merge into one ring through the shared point, bridged to the nonagon: a
        // ring of 9 + 6 + 2 points, cut into 15 triangles.
        assert_eq!(t.len(), 15);
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + w, y),
            DVec2::new(x + w, y + h),
            DVec2::new(x, y + h),
        ]
    }

    fn tri_area(pts: &[DVec2], t: &[u32; 3]) -> f64 {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        0.5 * (b - a).perp_dot(c - a)
    }

    #[test]
    fn square_is_two_ccw_triangles() {
        let r = rect(0.0, 0.0, 2.0, 1.0);
        let t = triangulate(&r, &[]).unwrap();
        assert_eq!(t.len(), 2);
        let area: f64 = t.iter().map(|t| tri_area(&r, t)).sum();
        assert!((area - 2.0).abs() < 1e-12);
        assert!(t.iter().all(|t| tri_area(&r, t) > 0.0));
    }

    #[test]
    fn square_with_hole() {
        let outer = rect(0.0, 0.0, 4.0, 4.0);
        let mut hole = rect(1.0, 1.0, 2.0, 2.0);
        hole.reverse();
        let t = triangulate(&outer, std::slice::from_ref(&hole)).unwrap();
        let all: Vec<DVec2> = outer.iter().chain(hole.iter()).copied().collect();
        let area: f64 = t.iter().map(|t| tri_area(&all, t)).sum();
        assert!((area - 12.0).abs() < 1e-12, "area {area}");
        assert_eq!(t.len(), 8);
    }

    #[test]
    fn typed_failures() {
        assert_eq!(
            triangulate(&[DVec2::ZERO, DVec2::X], &[]),
            Err(TessellateError::TooFewPoints(2))
        );
        assert_eq!(
            triangulate(&[DVec2::ZERO, DVec2::X, DVec2::new(2.0, 0.0)], &[]),
            Err(TessellateError::ZeroArea)
        );
        assert_eq!(
            triangulate(&[DVec2::ZERO, DVec2::X, DVec2::new(f64::NAN, 1.0)], &[]),
            Err(TessellateError::NonFinite)
        );
    }

    #[test]
    fn collinear_points_are_kept() {
        // A face edge split where a neighbour attaches: (1, 0) and (2, 0) sit on the bottom edge.
        let r = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(3.0, 0.0),
            DVec2::new(3.0, 1.0),
            DVec2::new(0.0, 1.0),
        ];
        let t = triangulate(&r, &[]).unwrap();
        for i in 0..r.len() as u32 {
            assert!(t.iter().any(|t| t.contains(&i)), "vertex {i} unused: {t:?}");
        }
        let area: f64 = t.iter().map(|t| tri_area(&r, t)).sum();
        assert!((area - 3.0).abs() < 1e-12);
        assert!(t.iter().all(|t| tri_area(&r, t) > 0.0));
    }

    /// Every vertex used, every triangle counter-clockwise, area equal to outer minus holes.
    fn assert_covers(outer: &[DVec2], holes: &[Vec<DVec2>], t: &[[u32; 3]]) {
        let all: Vec<DVec2> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        for i in 0..all.len() as u32 {
            assert!(t.iter().any(|t| t.contains(&i)), "vertex {i} unused: {t:?}");
        }
        assert!(
            t.iter().all(|t| tri_area(&all, t) > 0.0),
            "a clockwise or flat triangle"
        );
        let want =
            signed_area(outer).abs() - holes.iter().map(|h| signed_area(h).abs()).sum::<f64>();
        let got: f64 = t.iter().map(|t| tri_area(&all, t)).sum();
        assert!(
            (got - want).abs() <= 1e-12 * want.abs().max(1.0),
            "area {got} != {want}"
        );
    }

    #[test]
    fn earcut_misbridge_falls_back_to_ear_clipping() {
        // Fuzz find (CI, 2026-10-11, tests/repro/imprint-near-tolerance-non-positive-volume):
        // a side face with a notch and three collinear vertices on its right edge, plus an
        // imprinted hole well inside it. earcut bridged the hole wrongly and its triangles
        // missed 1.9% of the area, so the solid measured a volume of 0 and the imprint was
        // rejected as a kernel bug.
        let outer = vec![
            DVec2::new(0.75, -0.022136401340476754),
            DVec2::new(0.75, 0.05),
            DVec2::new(-0.7499999999999999, 0.05),
            DVec2::new(-0.7499999999999999, -0.05),
            DVec2::new(0.75, -0.05),
            DVec2::new(0.75, -0.03218185117291716),
            DVec2::new(0.75, -0.02963640134047675),
            DVec2::new(0.48731172429706215, -0.02963640134047675),
            DVec2::new(0.48731172429706215, -0.02213640134047675),
        ];
        let hole = vec![
            DVec2::new(0.3750000000000002, 0.02500000000000001),
            DVec2::new(0.4500000000000002, 0.02500000000000001),
            DVec2::new(0.4500000000000002, 0.0050000000000000044),
            DVec2::new(0.3750000000000002, 0.0050000000000000044),
        ];
        let holes = vec![hole];
        let t = triangulate(&outer, &holes).unwrap();
        assert_covers(&outer, &holes, &t);
    }

    #[test]
    fn ear_clip_alone_handles_holes_notches_and_collinear_points() {
        // An L-shaped ring with collinear points on two edges and two holes.
        let outer = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 1.0),
            DVec2::new(4.0, 2.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(2.0, 4.0),
            DVec2::new(0.0, 4.0),
        ];
        let mut h1 = rect(0.5, 0.5, 1.0, 1.0);
        h1.reverse();
        let h2 = rect(2.5, 0.5, 1.0, 1.0); // counter-clockwise on purpose: ear_clip rewinds it
        let holes = vec![h1, h2];
        let all: Vec<DVec2> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        let starts = [outer.len() as u32, (outer.len() + 4) as u32];
        let t = ear_clip(&all, outer.len(), &starts).unwrap();
        let area: f64 = t.iter().map(|t| tri_area(&all, t).abs()).sum();
        assert!((area - 10.0).abs() < 1e-12, "area {area}");
        // Through the public entry point every vertex comes back.
        let t = triangulate(&outer, &holes).unwrap();
        assert_covers(&outer, &holes, &t);
    }

    proptest! {
        /// The fallback agrees with the exact area on a rectangle with a rectangular hole and
        /// extra collinear points on the outer ring's right edge (the shape earcut missed).
        #[test]
        fn ear_clip_area_with_hole_and_collinear_edge(
            w in 0.5..10.0f64, h in 0.5..10.0f64,
            fx in 0.1..0.6f64, fy in 0.1..0.6f64, fw in 0.05..0.3f64, fh in 0.05..0.3f64,
            splits in proptest::collection::vec(0.01..0.99f64, 0..5),
        ) {
            let mut ys: Vec<f64> = splits.iter().map(|s| s * h).collect();
            ys.sort_by(f64::total_cmp);
            ys.dedup();
            let mut outer = vec![DVec2::new(0.0, 0.0), DVec2::new(w, 0.0)];
            outer.extend(ys.iter().map(|&y| DVec2::new(w, y)));
            outer.push(DVec2::new(w, h));
            outer.push(DVec2::new(0.0, h));
            let mut hole = rect(fx * w, fy * h, fw * w, fh * h);
            hole.reverse();
            let all: Vec<DVec2> = outer.iter().chain(hole.iter()).copied().collect();
            let t = ear_clip(&all, outer.len(), &[outer.len() as u32]).unwrap();
            let area: f64 = t.iter().map(|t| tri_area(&all, t).abs()).sum();
            let want = w * h - fw * w * fh * h;
            prop_assert!((area - want).abs() <= 1e-9 * want, "area {} != {}", area, want);
        }
    }

    /// Exact coverage: every triangle counter-clockwise (exactly) and their areas summing to
    /// the polygon's net area to its own size.
    fn assert_exact_cover(outer: &[DVec2], holes: &[Vec<DVec2>], t: &[[u32; 3]]) {
        let all: Vec<DVec2> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        let data: Vec<[f64; 2]> = all.iter().map(|q| q.to_array()).collect();
        let mut starts = Vec::new();
        let mut at = outer.len() as u32;
        for h in holes {
            starts.push(at);
            at += h.len() as u32;
        }
        assert!(t.iter().all(|t| orient(
            all[t[0] as usize],
            all[t[1] as usize],
            all[t[2] as usize]
        ) > 0.0));
        let flat: Vec<u32> = t.iter().flatten().copied().collect();
        let dev = area_deviation(&data, &starts, &flat);
        assert!(dev <= 1e-12, "deviation {dev:e}");
    }

    proptest! {
        /// Fix pass of lane 23: a band between a regular polygon up to 10 km across, up to
        /// 5 km from the origin, and the same polygon a hair smaller (a curve and its offset)
        /// triangulates, and its triangles pass the area check (they are right: the check
        /// must say so whatever the rings' size).
        #[test]
        fn thin_bands_round_far_polygons_triangulate(
            n in 3usize..=96,
            r in prop_oneof![Just(9000.0), Just(600.0), 1.0..10_000.0f64],
            w in prop_oneof![Just(6.839698443395447e-5), 1e-6..1e-2f64],
            cx in -5000.0..5000.0f64,
            cy in -5000.0..5000.0f64,
            a0 in 0.0..std::f64::consts::TAU,
        ) {
            let ring = |rr: f64| -> Vec<DVec2> {
                (0..n)
                    .map(|k| {
                        let a = a0 + std::f64::consts::TAU * k as f64 / n as f64;
                        DVec2::new(cx, cy) + rr * DVec2::from_angle(a)
                    })
                    .collect()
            };
            let outer = ring(r);
            let mut hole = ring(r - w);
            hole.reverse();
            let holes = vec![hole];
            let t = triangulate(&outer, &holes).map_err(|e| TestCaseError::fail(e.to_string()))?;
            assert_exact_cover(&outer, &holes, &t);
        }

        /// Fix pass of lane 23: two to four triangular holes round one shared point (and,
        /// sometimes, one more hole touching a corner of the outer ring) triangulate exactly,
        /// through the fallback alone and through `triangulate`.
        #[test]
        fn holes_touching_at_points_triangulate(
            k in 2usize..=4,
            fr in proptest::collection::vec(0.0..1.0f64, 8),
            turn in 0.0..std::f64::consts::TAU,
            radii in proptest::collection::vec(0.01..0.45f64, 8),
            px in -0.5..0.5f64,
            py in -0.5..0.5f64,
            far in prop_oneof![Just(0.0), Just(700.0), Just(5000.0)],
            corner in any::<bool>(),
            size in prop_oneof![Just(2.0), Just(700.0)],
        ) {
            // Hole i is a triangle with its corner at the shared point, inside the i-th of k
            // equal shares of the turn (so no two overlap), at most 0.55 of its share wide.
            let tau = std::f64::consts::TAU;
            let o = DVec2::new(far, -far);
            let shared = o + DVec2::new(px, py);
            let mut holes: Vec<Vec<DVec2>> = (0..k)
                .map(|i| {
                    let share = tau / k as f64;
                    let a = turn + share * (i as f64 + 0.4 * fr[2 * i]);
                    let b = a + share * (0.05 + 0.5 * fr[2 * i + 1]);
                    let pa = shared + radii[2 * i] * DVec2::from_angle(a);
                    let pb = shared + radii[2 * i + 1] * DVec2::from_angle(b);
                    vec![shared, pb, pa]
                })
                .collect();
            let h = size / 2.0;
            let outer = vec![
                o + DVec2::new(-h, -h),
                o + DVec2::new(h, -h),
                o + DVec2::new(h, h),
                o + DVec2::new(-h, h),
            ];
            if corner {
                // A sliver hole hanging off the outer ring's top right corner (the others stay
                // within 0.95 of the middle).
                let c = outer[2];
                holes.push(vec![c, c + DVec2::new(-0.04, -0.01), c + DVec2::new(-0.01, -0.04)]);
            }
            let all: Vec<DVec2> = outer.iter().chain(holes.iter().flatten()).copied().collect();
            let mut starts = Vec::new();
            let mut at = outer.len() as u32;
            for h in &holes {
                starts.push(at);
                at += h.len() as u32;
            }
            let t = ear_clip(&all, outer.len(), &starts)
                .ok_or_else(|| TestCaseError::fail("no ear"))?;
            assert_exact_cover(&outer, &holes, &t);
            let t = triangulate(&outer, &holes).map_err(|e| TestCaseError::fail(e.to_string()))?;
            assert_exact_cover(&outer, &holes, &t);
        }
    }

    #[test]
    fn interior_point_avoids_holes_and_notches() {
        let outer = rect(0.0, 0.0, 4.0, 4.0);
        let mut hole = rect(1.0, 1.0, 2.0, 2.0);
        hole.reverse();
        let p = interior_point(&outer, std::slice::from_ref(&hole)).unwrap();
        let inside_outer = p.x > 0.0 && p.x < 4.0 && p.y > 0.0 && p.y < 4.0;
        let inside_hole = p.x > 1.0 && p.x < 3.0 && p.y > 1.0 && p.y < 3.0;
        assert!(inside_outer && !inside_hole, "{p}");
        // U shape: the notch is not inside.
        let u = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(3.0, 0.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(2.0, 3.0),
            DVec2::new(2.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 3.0),
            DVec2::new(0.0, 3.0),
        ];
        let p = interior_point(&u, &[]).unwrap();
        assert!(
            !(p.x > 1.0 && p.x < 2.0 && p.y > 1.0),
            "{p} is in the notch"
        );
        assert!(interior_point(&[DVec2::ZERO, DVec2::X], &[]).is_none());
    }

    proptest! {
        #[test]
        fn rect_area_preserved(x in -100.0..100.0f64, y in -100.0..100.0f64, w in 0.001..50.0f64, h in 0.001..50.0f64) {
            let r = rect(x, y, w, h);
            let t = triangulate(&r, &[]).unwrap();
            let area: f64 = t.iter().map(|t| tri_area(&r, t)).sum();
            prop_assert!((area - w * h).abs() <= 1e-9 * (w * h).max(1.0));
        }
    }
}
