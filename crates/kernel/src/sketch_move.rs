//! Moving, turning, scaling and copying sketch geometry (lane 22, ADR-0012): the
//! [`Op::TransformSketch`](crate::model::Op::TransformSketch) the Move, Rotate and Scale
//! tools send for sketch points, edges and regions (geometry not yet pushed into a solid).
//!
//! # What moves
//!
//! The selection names sketch vertices (world points), edges (world point pairs) and whole
//! regions of one sketch. The **moving vertices** are the points, the edges' ends and the
//! regions' ring vertices. The **carried edges** are the selected edges and the regions'
//! ring edges: what a copy copies and what lifts off the plane with a whole shape.
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

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

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
    detachable: bool,
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

impl Model {
    /// Apply [`Op::TransformSketch`](crate::model::Op::TransformSketch).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transform_sketch(
        &mut self,
        sketch: u64,
        points: &[[f64; 3]],
        edges: &[[[f64; 3]; 2]],
        regions: &[u64],
        xform: &Xform,
        copy: bool,
        count: u32,
    ) -> Result<OpOutcome, KernelError> {
        if count == 0 || count > MAX_ARRAY || (count > 1 && !copy) {
            return Err(TransformError::BadCount.into());
        }
        xform.check()?;
        let r = self.resolve_sketch_selection(sketch, points, edges, regions)?;
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
            let (next, touched) = s.with_moved_vertices(&to, &anchor)?;
            self.sketches[r.sid] = next;
            return Ok(OpOutcome::SketchMoved {
                sketch: to_public(r.sid),
                regions: pairs(r.sid, &touched),
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
        let left = s.without_edges(&r.carried, &r.whole);
        self.sketches[r.sid] = left;
        let dest = self.sketch_on(&lands);
        let touched = self.draw_segments(dest, &segs)?;
        Ok(OpOutcome::SketchMoved {
            sketch: to_public(dest),
            regions: pairs(dest, &touched),
        })
    }

    /// Copies (and arrays) of the carried edges: copy k mapped by the map applied k times,
    /// into the sketch on the plane it lands on (the source sketch when that is its own
    /// plane, or when the selection is part of a shape and so stays in its plane).
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
        // Each destination sketch once, in the order the copies first reach it.
        let mut dests: Vec<(SketchId, Vec<(DVec3, DVec3)>)> = Vec::new();
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
            let i = match dests.iter().position(|(d, _)| *d == into) {
                Some(i) => i,
                None => {
                    dests.push((into, Vec::new()));
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
        }
        if far <= POINT_MERGE {
            return Err(TransformError::NothingToMove.into());
        }
        let mut out: Vec<[u64; 2]> = Vec::new();
        for (into, segs) in dests {
            let touched = self.draw_segments(into, &segs)?;
            out.extend(pairs(into, &touched));
        }
        Ok(OpOutcome::SketchCopied { regions: out })
    }

    /// The selection an op names, resolved against its sketch (which must be in the open
    /// context): vertices by position, edges by their ends, live regions by id.
    fn resolve_sketch_selection(
        &self,
        sketch: u64,
        points: &[[f64; 3]],
        edges: &[[[f64; 3]; 2]],
        regions: &[u64],
    ) -> Result<Resolved, KernelError> {
        let sid = self.sketch_edit_key(sketch)?;
        let s = &self.sketches[sid];
        let plane = *s.plane();
        if points.is_empty() && edges.is_empty() && regions.is_empty() {
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
        // A point also at the end of a carried edge is not lone.
        let on_carried: BTreeSet<SVertexId> = carried.iter().flat_map(|&(a, b)| [a, b]).collect();
        lone.retain(|v| !on_carried.contains(v));
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
        Ok(Resolved {
            sid,
            plane,
            moving,
            carried,
            whole,
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
