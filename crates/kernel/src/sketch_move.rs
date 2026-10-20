//! Moving, turning, scaling and copying sketch geometry (lane 22, ADR-0012): the
//! [`Op::TransformSketch`](crate::model::Op::TransformSketch) the Move, Rotate and Scale
//! tools send for sketch points, edges and regions (geometry not yet pushed into a solid).
//!
//! # What moves
//!
//! The selection names sketch vertices (world points), edges (world point pairs), whole
//! regions and (lane 23) whole curves of one sketch. The **moving vertices** are the points,
//! the edges' ends, the regions' ring vertices and the curves' points. The **carried edges**
//! are the selected edges, the regions' ring edges and the curves' edges: what a copy
//! copies and what lifts off the plane with a whole shape.
//!
//! # In the plane (the usual case)
//!
//! When the map keeps the sketch's plane (a move along it, a turn about its normal, a
//! scale of a flat selection), or when the selection is part of a shape (a point, an edge
//! of a rectangle: its neighbours' edges have to follow), every moving vertex goes to its
//! mapped position **projected into the plane**, and every edge touching one is drawn
//! again with the sketch's insertion rules ([`crate::sketch::Sketch`]'s
//! `with_moved_vertices`): coincident edges weld, crossings split, an edge shrunk to
//! nothing goes and its ends merge, regions are re-detected. Regions stay planar (they
//! never leave the plane) and simple (the arrangement splits a bow-tie into two regions; it
//! never keeps a ring that crosses itself). Vertex ids survive, so a region that only
//! changed shape keeps its id.
//!
//! # Off the plane (a whole shape)
//!
//! When the map takes the plane somewhere else and the selection is **detachable** (no
//! lone point, and every edge left behind that touches a moving vertex meets it on the
//! ring of a region that moves whole), the carried edges are drawn into the open context's
//! sketch on the mapped plane (an existing one there, or a new one), and leave the source
//! sketch except where they bound a region that stays (a neighbour sharing an edge keeps
//! it). A region moves whole when every edge of its rings is carried, so an island in a
//! carried hole goes along.
//!
//! # Copies
//!
//! With `copy`, the source is left alone and the carried edges are drawn mapped (`count`
//! times for an array, copy k by the map applied k times), each copy into the plane it
//! lands on by the same two rules. A copy is independent geometry: it welds onto what it
//! touches the way a stroke drawn there would.
//!
//! # Curves (lane 23, ADR-0013)
//!
//! A curve whose every edge is carried moves as one: its points move like any others, and a
//! circle, an arc or a polygon keeps its kind with its centre mapped and its radius scaled
//! when the map is a similarity within the plane it lands on (a move, a turn, a mirror, a
//! uniform scale); under any other map (a non-uniform scale, a tilt projected into the
//! plane) it stays one curve, a polyline. A copy of a curve is a new curve with a new id.
//! A curve only some of whose points move is bent: it becomes a polyline. A curve's point
//! selected on its own is refused ([`TransformError::CurvePoint`]): moving it would bend the
//! curve, and the message names Explode Curve.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DVec2, DVec3};

use crate::curve::{CurveKind, Round, similarity_scale};
use crate::error::{KernelError, SketchError, TransformError};
use crate::ids::{RegionId, SVertexId, SketchId, from_public, to_public};
use crate::model::{MAX_ARRAY, Model, OpOutcome};
use crate::plane::Plane;
use crate::tolerance::{PLANARITY, POINT_MERGE, in_bounds};
use crate::transform::{Xform, map_plane};

/// A point that moves less than this has not moved (rounding): a move where nothing moves
/// more is refused as nothing to move.
const STILL: f64 = 1e-9;

/// What [`Model::transform_sketch`] works from: the selection resolved against the sketch.
struct Resolved {
    sid: SketchId,
    plane: Plane,
    moving: BTreeSet<SVertexId>,
    carried: BTreeSet<(SVertexId, SVertexId)>,
    /// Live regions whose rings are all carried (the selected ones, and any island in them).
    whole: BTreeSet<RegionId>,
    /// Curves whose every edge is carried (lane 23): the named ones first, in the op's
    /// order, then the rest in id order.
    curves: Vec<u64>,
    detachable: bool,
}

/// A curve as a copy or a lift draws it again (lane 23): world points in order, whether it
/// closes, its kind, its segment count and its world centre (round kinds).
#[derive(Debug, Clone)]
struct CurveCopy {
    points: Vec<DVec3>,
    closed: bool,
    kind: CurveKind,
    segments: u32,
    centre: Option<DVec3>,
}

/// The plane `p` lands on under `x`, or `None` when the map is singular or takes the plane
/// beyond the coordinate limit.
fn mapped_plane(x: &Xform, p: &Plane) -> Option<Plane> {
    let q = map_plane(&x.to_affine(), p)?;
    let o = q.origin;
    (o.is_finite() && in_bounds(o.x) && in_bounds(o.y) && in_bounds(o.z)).then_some(q)
}

/// The same plane (either facing), by the test [`Model`] finds the sketch on a plane with.
fn same_plane(a: &Plane, b: &Plane) -> bool {
    a.normal.dot(b.normal).abs() >= 1.0 - 1e-12 && a.signed_distance(b.origin).abs() <= PLANARITY
}

/// The scale `a` applies to lengths in `plane`, seen in `onto` (its points projected into
/// it when `project`), when that is a similarity; `None` otherwise.
fn plane_scale(a: &DAffine3, plane: &Plane, onto: &Plane, project: bool) -> Option<f64> {
    let at = |q: DVec2| {
        let w = a.transform_point3(plane.to_world(q));
        if project {
            onto.to_world(onto.to_local(w))
        } else {
            w
        }
    };
    let (o, u, v) = (at(DVec2::ZERO), at(DVec2::X), at(DVec2::Y));
    let (du, dv) = (u - o, v - o);
    if project {
        similarity_scale(
            onto.to_local(u) - onto.to_local(o),
            onto.to_local(v) - onto.to_local(o),
        )
    } else {
        let s = du.length().max(dv.length());
        ((du.length() - dv.length()).abs() <= 1e-9 * s
            && du.dot(dv).abs() <= 1e-9 * s * s
            && s > 0.0)
            .then(|| 0.5 * (du.length() + dv.length()))
    }
}

impl Model {
    /// Apply [`Op::TransformSketch`](crate::model::Op::TransformSketch).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transform_sketch(
        &mut self,
        sketch: u64,
        points: &[[f64; 3]],
        edges: &[[[f64; 3]; 2]],
        regions: &[u64],
        curves: &[u64],
        xform: &Xform,
        copy: bool,
        count: u32,
    ) -> Result<OpOutcome, KernelError> {
        if count == 0 || count > MAX_ARRAY || (count > 1 && !copy) {
            return Err(TransformError::BadCount.into());
        }
        xform.check()?;
        let r = self.resolve_sketch_selection(sketch, points, edges, regions, curves)?;
        if copy {
            return self.copy_sketch(&r, xform, count);
        }
        let a = xform.to_affine();
        let plane = r.plane;
        let lands = mapped_plane(xform, &plane).ok_or(TransformError::OutOfBounds)?;
        let s = &self.sketches[r.sid];
        if same_plane(&plane, &lands) || !r.detachable {
            // In the plane: every moving vertex to its projected image.
            let mut to: BTreeMap<SVertexId, DVec2> = BTreeMap::new();
            let (mut moved, mut wanted) = (0.0f64, 0.0f64);
            for &v in &r.moving {
                let p = s.vertex(v).ok_or(TransformError::NoSuchSketchPoint)?;
                let w = a.transform_point3(plane.to_world(p));
                let q = plane.to_local(w);
                moved = moved.max(q.distance(p));
                wanted = wanted.max(w.distance(plane.to_world(p)));
                to.insert(v, q);
            }
            if moved <= STILL {
                return Err(if wanted > POINT_MERGE {
                    TransformError::LeavesSketchPlane
                } else {
                    TransformError::NothingToMove
                }
                .into());
            }
            let anchor = move |q: DVec2| plane.to_local(a.transform_point3(plane.to_world(q)));
            // Lane 23: a whole round curve keeps its circle when the projected map is a
            // similarity in the plane.
            let scale = plane_scale(&a, &plane, &plane, true);
            let round = move |c: &Round| scale.map(|k| (anchor(c.centre), c.radius * k));
            let (next, touched) = s.with_moved_vertices(&to, &anchor, &round)?;
            let kept: Vec<[u64; 2]> = r
                .curves
                .iter()
                .filter(|id| next.curve(**id).is_some())
                .map(|&id| [to_public(r.sid), id])
                .collect();
            self.sketches[r.sid] = next;
            return Ok(OpOutcome::SketchMoved {
                sketch: to_public(r.sid),
                regions: pairs(r.sid, &touched),
                curves: kept,
            });
        }
        // Off the plane, whole: the carried edges go to the sketch on the plane they land on.
        let mut segs: Vec<(DVec3, DVec3)> = Vec::with_capacity(r.carried.len());
        for &(u, w) in &r.carried {
            let (pu, pw) = (
                s.vertex(u).ok_or(TransformError::NoSuchSketchPoint)?,
                s.vertex(w).ok_or(TransformError::NoSuchSketchPoint)?,
            );
            segs.push((
                a.transform_point3(plane.to_world(pu)),
                a.transform_point3(plane.to_world(pw)),
            ));
        }
        let lifted = self.curve_copies(&r, &a, None)?;
        let s = &self.sketches[r.sid];
        let left = s.without_edges(&r.carried, &r.whole);
        self.sketches[r.sid] = left;
        let dest = self.sketch_on(&lands);
        let touched = self.draw_segments(dest, &segs)?;
        let mut made = Vec::new();
        for c in &lifted {
            if let Some(id) = self.redraw_curve(dest, c) {
                made.push([to_public(dest), id]);
            }
        }
        Ok(OpOutcome::SketchMoved {
            sketch: to_public(dest),
            regions: pairs(dest, &touched),
            curves: made,
        })
    }

    /// Lane 23: the whole curves of a selection mapped by `a` (projected into `onto` when it
    /// is given, the selection's own plane): their world points and, for a round kind, the
    /// mapped centre when the map is a similarity there (otherwise a polyline).
    fn curve_copies(
        &self,
        r: &Resolved,
        a: &DAffine3,
        onto: Option<&Plane>,
    ) -> Result<Vec<CurveCopy>, KernelError> {
        let s = &self.sketches[r.sid];
        let plane = r.plane;
        let dest = map_plane(a, &plane).ok_or(TransformError::Singular)?;
        let scale = match onto {
            Some(p) => plane_scale(a, &plane, p, true),
            None => plane_scale(a, &plane, &dest, false),
        };
        let place = |w: DVec3| match onto {
            Some(p) => p.to_world(p.to_local(w)),
            None => w,
        };
        let mut out = Vec::with_capacity(r.curves.len());
        for &id in &r.curves {
            let Some(c) = s.curve(id) else { continue };
            let pts = s
                .curve_points(id)
                .ok_or(TransformError::NoSuchSketchPoint)?;
            let round = c.round.filter(|_| scale.is_some());
            out.push(CurveCopy {
                points: pts
                    .iter()
                    .map(|&q| place(a.transform_point3(plane.to_world(q))))
                    .collect(),
                closed: c.closed,
                kind: if round.is_some() || !c.kind.is_round() {
                    c.kind
                } else {
                    CurveKind::Polyline
                },
                segments: if round.is_some() {
                    c.segments
                } else {
                    c.edge_count() as u32
                },
                centre: round.map(|rd| place(a.transform_point3(plane.to_world(rd.centre)))),
            });
        }
        Ok(out)
    }

    /// Lane 23: a mapped curve made a curve again in sketch `sid`, where its edges were just
    /// drawn. Returns its id (its first piece's), `None` when it did not land as one.
    fn redraw_curve(&mut self, sid: SketchId, c: &CurveCopy) -> Option<u64> {
        let s = &mut self.sketches[sid];
        let plane = *s.plane();
        let local: Vec<DVec2> = c.points.iter().map(|&p| plane.to_local(p)).collect();
        let centre = c.centre.map(|w| plane.to_local(w));
        s.add_curve(&local, c.closed, c.kind, centre, c.segments)
    }

    /// Copies (and arrays) of the carried edges: copy k mapped by the map applied k times,
    /// into the sketch on the plane it lands on (the source sketch when that is its own
    /// plane, or when the selection is part of a shape and so stays in its plane). Whole
    /// curves are copied as new curves (lane 23).
    fn copy_sketch(
        &mut self,
        r: &Resolved,
        xform: &Xform,
        count: u32,
    ) -> Result<OpOutcome, KernelError> {
        if r.carried.is_empty() {
            return Err(TransformError::NothingToMove.into());
        }
        let s = &self.sketches[r.sid];
        let plane = r.plane;
        let mut base: Vec<(DVec3, DVec3)> = Vec::with_capacity(r.carried.len());
        for &(u, w) in &r.carried {
            base.push((
                plane.to_world(s.vertex(u).ok_or(TransformError::NoSuchSketchPoint)?),
                plane.to_world(s.vertex(w).ok_or(TransformError::NoSuchSketchPoint)?),
            ));
        }
        // Each destination sketch once, in the order the copies first reach it, with its
        // segments and the curves to make there.
        type Dest = (SketchId, Vec<(DVec3, DVec3)>, Vec<CurveCopy>);
        let mut dests: Vec<Dest> = Vec::new();
        let mut far = 0.0f64;
        for k in 1..=count {
            let xk = xform.power(k);
            xk.check()?;
            let ak = xk.to_affine();
            let pk = mapped_plane(&xk, &plane).ok_or(TransformError::OutOfBounds)?;
            let into = if same_plane(&plane, &pk) || !r.detachable {
                r.sid
            } else {
                self.sketch_on(&pk)
            };
            let curves = self.curve_copies(r, &ak, (into == r.sid).then_some(&plane))?;
            let i = match dests.iter().position(|(d, _, _)| *d == into) {
                Some(i) => i,
                None => {
                    dests.push((into, Vec::new(), Vec::new()));
                    dests.len() - 1
                }
            };
            for &(p, q) in &base {
                let (mut mp, mut mq) = (ak.transform_point3(p), ak.transform_point3(q));
                if into == r.sid {
                    // Part of a shape, or a copy within the plane: projected into it.
                    mp = plane.to_world(plane.to_local(mp));
                    mq = plane.to_world(plane.to_local(mq));
                    far = far.max(mp.distance(p)).max(mq.distance(q));
                } else {
                    far = f64::INFINITY;
                }
                dests[i].1.push((mp, mq));
            }
            dests[i].2.extend(curves);
        }
        if far <= POINT_MERGE {
            return Err(TransformError::NothingToMove.into());
        }
        let mut out: Vec<[u64; 2]> = Vec::new();
        let mut made: Vec<[u64; 2]> = Vec::new();
        for (into, segs, curves) in dests {
            let touched = self.draw_segments(into, &segs)?;
            out.extend(pairs(into, &touched));
            for c in &curves {
                if let Some(id) = self.redraw_curve(into, c) {
                    made.push([to_public(into), id]);
                }
            }
        }
        Ok(OpOutcome::SketchCopied {
            regions: out,
            curves: made,
        })
    }

    /// The selection an op names, resolved against its sketch (which must be in the open
    /// context): vertices by position, edges by their ends, live regions and curves by id.
    fn resolve_sketch_selection(
        &self,
        sketch: u64,
        points: &[[f64; 3]],
        edges: &[[[f64; 3]; 2]],
        regions: &[u64],
        curves: &[u64],
    ) -> Result<Resolved, KernelError> {
        let sid = self.sketch_edit_key(sketch)?;
        let s = &self.sketches[sid];
        let plane = *s.plane();
        if points.is_empty() && edges.is_empty() && regions.is_empty() && curves.is_empty() {
            return Err(TransformError::NothingSelected.into());
        }
        let vertex = |p: &[f64; 3]| -> Result<SVertexId, KernelError> {
            let w = DVec3::from_array(*p);
            if !w.is_finite() || plane.signed_distance(w).abs() > POINT_MERGE {
                return Err(TransformError::NoSuchSketchPoint.into());
            }
            s.vertex_at(plane.to_local(w))
                .ok_or_else(|| TransformError::NoSuchSketchPoint.into())
        };
        let mut moving: BTreeSet<SVertexId> = BTreeSet::new();
        let mut lone: BTreeSet<SVertexId> = BTreeSet::new();
        let mut carried: BTreeSet<(SVertexId, SVertexId)> = BTreeSet::new();
        for p in points {
            let v = vertex(p)?;
            moving.insert(v);
            lone.insert(v);
        }
        for [p, q] in edges {
            let (u, w) = (vertex(p)?, vertex(q)?);
            if u == w || !s.has_edge(u, w) {
                return Err(TransformError::NoSuchSketchEdge.into());
            }
            moving.extend([u, w]);
            carried.insert((u.min(w), u.max(w)));
        }
        for &id in regions {
            let rid = from_public::<RegionId>(id);
            let r = s.region(rid).ok_or(SketchError::NoSuchRegion(id))?;
            if r.consumed {
                return Err(SketchError::RegionConsumed(id).into());
            }
            moving.extend(s.region_vertices(rid));
            carried.extend(r.boundary_edges());
        }
        let mut named: Vec<u64> = Vec::new();
        for &id in curves {
            let c = s.curve(id).ok_or(SketchError::NoSuchCurve(id))?;
            moving.extend(c.chain.iter().copied());
            carried.extend(c.pairs().map(|(a, b)| (a.min(b), a.max(b))));
            if !named.contains(&id) {
                named.push(id);
            }
        }
        // A point also at the end of a carried edge is not lone.
        let on_carried: BTreeSet<SVertexId> = carried.iter().flat_map(|&(a, b)| [a, b]).collect();
        lone.retain(|v| !on_carried.contains(v));
        // Lane 23: a curve's point on its own would bend the curve.
        if lone.iter().any(|&v| !s.curves_at(v).is_empty()) {
            return Err(TransformError::CurvePoint.into());
        }
        // Every live region whose rings are all carried moves whole.
        let whole: BTreeSet<RegionId> = s
            .regions()
            .filter(|(_, r)| r.boundary_edges().is_subset(&carried))
            .map(|(rid, _)| rid)
            .collect();
        let on_whole: BTreeSet<SVertexId> = whole
            .iter()
            .flat_map(|&rid| s.region_vertices(rid))
            .collect();
        // An edge left behind that touches a moving vertex must meet it on a whole region's
        // ring for the shape to lift off its plane (otherwise it would have to bend with it).
        let detachable = lone.is_empty()
            && !carried.is_empty()
            && s.edges().all(|(_, [a, b])| {
                carried.contains(&(a.min(b), a.max(b)))
                    || [a, b]
                        .iter()
                        .all(|v| !moving.contains(v) || on_whole.contains(v))
            });
        let mut curves = named;
        for id in s.whole_curves(&carried) {
            if !curves.contains(&id) {
                curves.push(id);
            }
        }
        Ok(Resolved {
            sid,
            plane,
            moving,
            carried,
            whole,
            curves,
            detachable,
        })
    }

    /// Draw world segments into a sketch (every point on its plane, within the merge
    /// distance) as one transaction. Returns the live regions bounded by what was drawn.
    fn draw_segments(
        &mut self,
        sid: SketchId,
        segs: &[(DVec3, DVec3)],
    ) -> Result<Vec<RegionId>, KernelError> {
        let s = &self.sketches[sid];
        let plane = *s.plane();
        let mut local = Vec::with_capacity(segs.len());
        for &(p, q) in segs {
            for w in [p, q] {
                if !(w.is_finite() && in_bounds(w.x) && in_bounds(w.y) && in_bounds(w.z)) {
                    return Err(SketchError::OutOfBounds.into());
                }
                // Mapped points land on the plane up to rounding, and an existing sketch on
                // that plane may sit up to the planarity tolerance away: project those.
                if plane.signed_distance(w).abs() > POINT_MERGE {
                    return Err(SketchError::OffPlane.into());
                }
            }
            local.push((plane.to_local(p), plane.to_local(q)));
        }
        match s.with_segments(&local)? {
            Some((next, touched)) => {
                self.sketches[sid] = next;
                Ok(touched)
            }
            None => Ok(Vec::new()),
        }
    }
}

/// (sketch, region) public id pairs.
fn pairs(sid: SketchId, regions: &[RegionId]) -> Vec<[u64; 2]> {
    regions
        .iter()
        .map(|&r| [to_public(sid), to_public(r)])
        .collect()
}
