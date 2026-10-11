//! General push/pull (ADR-0001): any planar face of a watertight solid, either direction.
//!
//! Three outcomes, tried in this order:
//!
//! 1. **Collapse.** Pushing inward far enough that the whole solid lies inside the swept
//!    prism (a box's cap pushed down onto its base, or past it): the solid is gone, and its
//!    flat footprint is handed back (the model returns it to the source sketch when it lies
//!    on that sketch's plane).
//! 2. **Topological path** (the common case, exact, no booleans). The face moves along its
//!    normal. At every vertex of the face whose other faces all contain the push direction
//!    (the walls of a prism, say), the vertex simply slides: those faces stretch or shrink.
//!    Every other vertex stays and gets a copy at the new position joined by a rail, and
//!    every face edge whose neighbour does not contain the push direction gets a new side
//!    wall. Coplanar walls merge. The candidate is accepted only if it validates (topology,
//!    watertight, no self-intersection: see [`crate::intersect`]), is still one shell, and
//!    its volume changed by exactly `area * distance`.
//! 3. **Boolean path** (everything else: pushing through the opposite face, into another
//!    part of the solid, flush with a neighbour, through a shell). Union (pull) or
//!    difference (push) with the prism swept by the face, through the
//!    [`crate::boolean::BooleanEngine`] (manifold-rust), re-polygonised into n-gons with
//!    holes, coplanar faces merged except across user-drawn edges (so an imprint pushed in
//!    and pulled back flush is still there), validated.
//!
//! Positive distance pulls outward (along the face normal), negative pushes inward.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use sawhorse_tessellate::signed_area;

use crate::boolean::{BoolOp, BooleanError, combine, default_engine};
use crate::brep::{PolyFace, Polys};
use crate::error::PushError;
use crate::faceops::merge_coplanar;
use crate::object::{Object, PushOutcome};
use crate::plane::Plane;
use crate::sketch::point_segment_distance;
use crate::tolerance::{MIN_EXTRUDE, POINT_MERGE, in_bounds};
use crate::validate::point_in_ring;

/// |cos| below which a face counts as containing the push direction.
const CONTAINS_DIR: f64 = 1e-9;

fn face_rings(p: &Polys, fi: usize) -> Vec<Vec<usize>> {
    p.faces[fi].rings().cloned().collect()
}

/// Area of face `fi` (outer minus holes).
fn face_area(p: &Polys, fi: usize, plane: &Plane) -> f64 {
    face_rings(p, fi)
        .iter()
        .map(|r| {
            signed_area(
                &r.iter()
                    .map(|&i| plane.to_local(p.points[i]))
                    .collect::<Vec<_>>(),
            )
        })
        .sum()
}

/// The prism swept by face `fi` over `d` along its normal, as a closed `Polys` whose base
/// vertices are exactly the face's vertices (so the engine sees exact coincidence). The
/// cap (the face's new position) carries the face's persistent id, so the boolean path
/// hands it to the moved face; the base and walls carry none.
pub fn prism(p: &Polys, fi: usize, plane: &Plane, d: f64) -> Polys {
    let rings = face_rings(p, fi);
    let mut points = Vec::new();
    let mut base: Vec<Vec<usize>> = Vec::new();
    for r in &rings {
        base.push((points.len()..points.len() + r.len()).collect());
        points.extend(r.iter().map(|&i| p.points[i]));
    }
    let n_base = points.len();
    let offset = plane.normal * d;
    for i in 0..n_base {
        points.push(points[i] + offset);
    }
    let cap: Vec<Vec<usize>> = base
        .iter()
        .map(|r| r.iter().map(|&i| i + n_base).collect())
        .collect();
    let rev = |r: &Vec<usize>| r.iter().rev().copied().collect::<Vec<_>>();
    let mut cap_face = PolyFace::new(cap[0].clone(), cap[1..].to_vec());
    cap_face.id = p.faces[fi].id;
    let mut faces = vec![
        PolyFace::new(rev(&base[0]), base[1..].iter().map(rev).collect()),
        cap_face,
    ];
    for (b, c) in base.iter().zip(&cap) {
        for k in 0..b.len() {
            let j = (k + 1) % b.len();
            faces.push(PolyFace::new(vec![b[k], b[j], c[j], c[k]], vec![]));
        }
    }
    if d < 0.0 {
        for f in &mut faces {
            f.outer.reverse();
            for h in &mut f.holes {
                h.reverse();
            }
        }
    }
    Polys {
        points,
        faces,
        user_edges: BTreeSet::new(),
    }
}

/// Every vertex of the solid lies in the slab swept by the face and projects onto the
/// face: pushing inward by `d` (< 0) removes the whole solid.
fn collapses(p: &Polys, fi: usize, plane: &Plane, d: f64) -> bool {
    if d >= 0.0 {
        return false;
    }
    let rings: Vec<Vec<DVec2>> = face_rings(p, fi)
        .iter()
        .map(|r| r.iter().map(|&i| plane.to_local(p.points[i])).collect())
        .collect();
    let covers = |x: DVec3| {
        let h = plane.signed_distance(x);
        if h > POINT_MERGE || h < d - POINT_MERGE {
            return false;
        }
        let q = plane.to_local(x);
        let inside = point_in_ring(q, &rings[0]) && !rings[1..].iter().any(|r| point_in_ring(q, r));
        inside
            || rings.iter().any(|r| {
                (0..r.len())
                    .any(|k| point_segment_distance(q, r[k], r[(k + 1) % r.len()]) <= POINT_MERGE)
            })
    };
    let mut probes: Vec<DVec3> = p.points.clone();
    for f in &p.faces {
        for r in f.rings() {
            for k in 0..r.len() {
                probes.push(0.5 * (p.points[r[k]] + p.points[r[(k + 1) % r.len()]]));
            }
        }
    }
    probes.into_iter().all(covers)
}

/// The topological push/pull candidate (see the module docs), or `None` when the face's
/// neighbourhood cannot take it (the caller then uses the boolean path).
pub fn topological(p: &Polys, fi: usize, plane: &Plane, d: f64) -> Option<Polys> {
    let n = plane.normal;
    let rings = face_rings(p, fi);
    let fverts: BTreeSet<usize> = rings.iter().flatten().copied().collect();
    let mut around: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (g, f) in p.faces.iter().enumerate() {
        for r in f.rings() {
            for &v in r {
                around.entry(v).or_default().insert(g);
            }
        }
    }
    let planes: Vec<Option<Plane>> = (0..p.faces.len()).map(|g| p.plane(g)).collect();
    let contains_n = |g: usize| planes[g].is_some_and(|pl| pl.normal.dot(n).abs() <= CONTAINS_DIR);
    let slide: BTreeMap<usize, bool> = fverts
        .iter()
        .map(|&v| (v, around[&v].iter().all(|&g| g == fi || contains_n(g))))
        .collect();
    let mut q = p.clone();
    let mut moved: BTreeMap<usize, usize> = BTreeMap::new();
    for &v in &fverts {
        if slide[&v] {
            q.points[v] += n * d;
            moved.insert(v, v);
        } else {
            q.points.push(p.points[v] + n * d);
            moved.insert(v, q.points.len() - 1);
        }
    }
    let directed = p.directed_edges();
    for r in &rings {
        for k in 0..r.len() {
            let (a, b) = (r[k], r[(k + 1) % r.len()]);
            let &(g, _) = directed.get(&(b, a))?;
            if g == fi {
                return None;
            }
            if contains_n(g) {
                // The neighbour stretches: b -> a becomes b -> [b'] -> [a'] -> a.
                let mut extra = Vec::new();
                if !slide[&b] {
                    extra.push(moved[&b]);
                }
                if !slide[&a] {
                    extra.push(moved[&a]);
                }
                if extra.is_empty() {
                    continue;
                }
                let face = &mut q.faces[g];
                let ring = std::iter::once(&mut face.outer)
                    .chain(face.holes.iter_mut())
                    .find(|ring| {
                        (0..ring.len()).any(|i| ring[i] == b && ring[(i + 1) % ring.len()] == a)
                    })?;
                let at =
                    (0..ring.len()).find(|&i| ring[i] == b && ring[(i + 1) % ring.len()] == a)?;
                for (off, x) in extra.into_iter().enumerate() {
                    ring.insert(at + 1 + off, x);
                }
            } else {
                if slide[&a] || slide[&b] {
                    return None;
                }
                q.faces
                    .push(PolyFace::new(vec![a, b, moved[&b], moved[&a]], vec![]));
            }
        }
    }
    let face = &mut q.faces[fi];
    for r in std::iter::once(&mut face.outer).chain(face.holes.iter_mut()) {
        for v in r.iter_mut() {
            *v = moved[v];
        }
    }
    face.plane = Some(plane.offset(d));
    weld_collapsed_rails(&mut q, &slide);
    merge_coplanar(&mut q, true);
    Some(q)
}

/// A slid vertex that lands on the far end of one of its edges (a pocket floor pulled back
/// up flush, a step pushed down onto the next) is welded to it: the zero-height faces
/// between them disappear and the moved face meets its new neighbour edge to edge, exactly.
fn weld_collapsed_rails(q: &mut Polys, slide: &BTreeMap<usize, bool>) {
    let mut weld: BTreeMap<usize, usize> = BTreeMap::new();
    for f in &q.faces {
        for r in f.rings() {
            for k in 0..r.len() {
                let (a, b) = (r[k], r[(k + 1) % r.len()]);
                for (v, w) in [(a, b), (b, a)] {
                    if slide.get(&v) == Some(&true)
                        && slide.get(&w) != Some(&true)
                        && q.points[v].distance(q.points[w]) <= POINT_MERGE
                    {
                        weld.entry(v).or_insert(w);
                    }
                }
            }
        }
    }
    if weld.is_empty() {
        return;
    }
    let map = |v: usize| *weld.get(&v).unwrap_or(&v);
    for f in &mut q.faces {
        for r in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
            let mapped: Vec<usize> = r.iter().map(|&v| map(v)).collect();
            let n = mapped.len();
            *r = (0..n)
                .filter(|&k| mapped[k] != mapped[(k + 1) % n])
                .map(|k| mapped[k])
                .collect();
        }
        f.holes.retain(|h| h.len() >= 3);
    }
    q.faces.retain(|f| f.outer.len() >= 3);
    q.user_edges = q
        .user_edges
        .iter()
        .map(|&(a, b)| Polys::key(map(a), map(b)))
        .filter(|(a, b)| a != b)
        .collect();
    q.compact();
}

/// A candidate solid is acceptable: valid (incl. no self-intersection), `shells` shells.
/// New faces get fresh persistent ids from `base`'s counter.
fn accept(base: &Object, q: &Polys, shells: Option<usize>) -> Result<Object, String> {
    let obj = base.rebuilt(q.clone()).map_err(|e| e.to_string())?;
    let r = obj.validate().map_err(|e| e.to_string())?;
    if let Some(s) = shells
        && r.shells.len() != s
    {
        return Err(format!("{} shells, expected {s}", r.shells.len()));
    }
    Ok(obj)
}

/// Push/pull face `fi` (ordinal) of `obj` by `d` metres. On success the new solid (which
/// keeps `obj`'s provenance) or the collapse footprint.
pub(crate) fn push_pull(
    obj: &Object,
    fi: usize,
    d: f64,
) -> Result<(PushOutcome, Option<Object>), PushError> {
    if !in_bounds(d) {
        return Err(PushError::OutOfBounds);
    }
    if d.abs() < MIN_EXTRUDE {
        return Err(PushError::ZeroDistance);
    }
    let p = obj.brep().to_polys();
    if fi >= p.faces.len() {
        return Err(PushError::NoSuchFace(fi as u64));
    }
    let plane = p
        .plane(fi)
        .ok_or(PushError::Boolean(BooleanError::Degenerate(
            "face has no plane".into(),
        )))?;
    let footprint = || -> Vec<Vec<DVec3>> {
        face_rings(&p, fi)
            .iter()
            .map(|r| r.iter().map(|&i| p.points[i] + plane.normal * d).collect())
            .collect()
    };
    if collapses(&p, fi, &plane, d) {
        return Ok((
            PushOutcome::Collapsed {
                footprint: footprint(),
            },
            None,
        ));
    }
    let before = obj.validate().map(|r| r.shells.len()).unwrap_or(0);
    if before == 1
        && let Some(q) = topological(&p, fi, &plane, d)
        && let Ok(cand) = accept(obj, &q, Some(1))
    {
        let expect = obj.volume() + face_area(&p, fi, &plane) * d;
        if expect > 0.0 && (cand.volume() - expect).abs() <= 1e-9 * expect.abs().max(1.0) {
            return Ok((PushOutcome::Moved, Some(cand)));
        }
    }
    let engine = default_engine()?;
    let op = if d > 0.0 {
        BoolOp::Union
    } else {
        BoolOp::Difference
    };
    // Pushing exactly onto a far face that faces the other way (drilling through a slab):
    // reach a hair past it, so the engine cuts cleanly instead of meeting a face that is
    // coplanar only up to rounding. The overshoot is below the merge distance.
    let mut reach = d;
    if d < 0.0 {
        let lands = (0..p.faces.len()).any(|g| {
            p.plane(g).is_some_and(|pg| {
                pg.normal.dot(plane.normal) <= -(1.0 - 1e-9)
                    && (plane.signed_distance(pg.origin) - d).abs() <= POINT_MERGE
            })
        });
        if lands {
            reach = d - 0.5 * POINT_MERGE;
        }
    }
    match combine(&p, &prism(&p, fi, &plane, reach), op, engine.as_ref())? {
        None => Ok((
            PushOutcome::Collapsed {
                footprint: footprint(),
            },
            None,
        )),
        Some(q) => {
            let cand = accept(obj, &q, None).map_err(BooleanError::InvalidResult)?;
            Ok((PushOutcome::Moved, Some(cand)))
        }
    }
}
