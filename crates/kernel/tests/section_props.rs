//! Section cuts of hostile solids (lanes 12c and 12d).
//!
//! The lane-12 re-verifier's long fuzz with `FUZZ_PLANE_SNAP=0` found three solids whose
//! kept halves did not add up to the whole (seeds 20261803, 20261904, 20261907; the shrunk
//! sequences are `tests/repro/2026-10-12-section-*-pinch*.json`): boxes with part of them
//! moved and folded (Move with autofold), cut by a plane through a vertex where the surface
//! crosses the plane twice. The lane-12c verification found two more
//! (`2026-10-13-section-cap-lost-after-split`, a face split a micrometre off the plane whose
//! piece pointed the wrong way, and `-section-frame-prism-along-a-hole-edge`, a hole whose
//! edge lies along the cap's outline). Since lane 12d the cap is assembled on a planar
//! arrangement (ADR-0010), and these properties cut every kind of solid that broke a chain:
//!
//! - folded boxes (Move with autofold), at planes through their own vertices, along their
//!   own faces and at random;
//! - frame prisms (a rectangle with a rectangular hole, pulled up, sometimes turned), cut
//!   by a plane through an edge of the hole at any angle, the CX2 case among them;
//! - two boxes touching at a corner, cut through that corner;
//! - boxes with a face sheared or bent by up to a micrometre (and sometimes split), cut by a
//!   plane along where the face was: a sliver face within a micrometre of the plane;
//! - planes through one, two, three or four vertices of a folded box;
//! - nested holes: a box with a pocket around a post that has a hole of its own;
//! - planes exactly on a face, facing either way.
//!
//! And they check, for the plane and for its flip:
//!
//! - the two halves make the whole and each half is a closed surface (the fuzzer's
//!   invariant, `testing::check_sections`);
//! - every cap ring is simple (no point twice, no two edges crossing), outer rings
//!   counter-clockwise, holes clockwise;
//! - the caps' area (every cap: the cut keeps them all, lane 18) is the plane's cross-section
//!   of the solids worked out independently: every face clipped to the kept side, its vector
//!   area along the normal, summed;
//! - Create Group from Slice draws closed loops: the sketch's faces cover every representable
//!   cap's outer ring (`Cap::representable`, the kernel's one rule for what the slice can
//!   draw), and a section whose caps are all thinner than that is refused in its own words.

use glam::{DVec2, DVec3};
use proptest::prelude::*;
use sawhorse_kernel::model::{Model, Op, OpOutcome};
use sawhorse_kernel::testing::{Repro, check_sections};
use sawhorse_kernel::transform::Xform;
use sawhorse_kernel::{Document, snap};

/// Plane snapping on or off for the closure, put back afterwards (it is per thread).
fn snapping<T>(on: bool, f: impl FnOnce() -> T) -> T {
    let was = snap::plane_snapping();
    snap::set_plane_snapping(on);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    snap::set_plane_snapping(was);
    r.unwrap_or_else(|p| std::panic::resume_unwind(p))
}

fn snapping_off<T>(f: impl FnOnce() -> T) -> T {
    snapping(false, f)
}

#[derive(Debug, Clone)]
struct Fold {
    corners: Vec<bool>,
    axis: [f64; 3],
    angle: f64,
    shift: [f64; 3],
}

#[derive(Debug, Clone)]
enum At {
    /// Through one of the solid's own vertices (picked modulo the count).
    Vertex(usize),
    /// At fractions of the solid's bounding box.
    Inside([f64; 3]),
}

fn quarters() -> impl Strategy<Value = f64> {
    prop_oneof![3 => (1i32..8).prop_map(|k| f64::from(k) * 0.125), 1 => 0.05..1.0f64]
}

/// Which corners move: any set, or (as in the three finds) a face's four, an edge's two or
/// a single corner. Corner `k` is at `(k & 1, k & 2, k & 4)` times the size.
fn corners() -> impl Strategy<Value = Vec<bool>> {
    let set = |ks: Vec<usize>| (0..8).map(|k| ks.contains(&k)).collect::<Vec<bool>>();
    let faces = vec![
        vec![0, 1, 2, 3],
        vec![4, 5, 6, 7],
        vec![0, 1, 4, 5],
        vec![2, 3, 6, 7],
        vec![0, 2, 4, 6],
        vec![1, 3, 5, 7],
    ];
    let edges: Vec<Vec<usize>> = (0..8)
        .flat_map(|a| [1, 2, 4].into_iter().map(move |b| (a, a ^ b)))
        .filter(|(a, b)| a < b)
        .map(|(a, b)| vec![a, b])
        .collect();
    prop_oneof![
        2 => prop::sample::select(faces).prop_map(set),
        2 => prop::sample::select(edges).prop_map(set),
        1 => (0usize..8).prop_map(move |k| set(vec![k])),
        1 => prop::collection::vec(any::<bool>(), 8),
    ]
}

fn fold() -> impl Strategy<Value = Fold> {
    (
        corners(),
        prop::array::uniform3(-1.0..1.0f64),
        prop_oneof![1 => Just(std::f64::consts::FRAC_PI_2), 3 => -1.2..1.2f64, 1 => -2.5..2.5f64],
        prop::array::uniform3(
            prop_oneof![2 => (-4i32..4).prop_map(|k| f64::from(k) * 0.125), 1 => -0.6..0.6f64],
        ),
    )
        .prop_map(|(corners, axis, angle, shift)| Fold {
            corners,
            axis,
            angle,
            shift,
        })
}

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        1 => Just([1.0, 0.0, 0.0]),
        1 => Just([-1.0, 0.0, 0.0]),
        1 => Just([0.0, 1.0, 0.0]),
        1 => Just([0.0, -1.0, 0.0]),
        1 => Just([0.0, 0.0, 1.0]),
        1 => Just([0.0, 0.0, -1.0]),
        3 => prop::array::uniform3(-1.0..1.0f64),
    ]
}

fn at() -> impl Strategy<Value = At> {
    prop_oneof![
        2 => (0usize..64).prop_map(At::Vertex),
        1 => prop::array::uniform3(0.0..1.0f64).prop_map(At::Inside),
    ]
}

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A box (`w`, `d`, `h`) from `at`, as a new solid.
fn add_box(d: &mut Document, at: [f64; 3], size: [f64; 3]) -> u64 {
    let sketch = match ok(
        d,
        Op::NewSketch {
            origin: [0.0, 0.0, at[2]],
            normal: [0.0, 0.0, 1.0],
        },
    ) {
        OpOutcome::SketchCreated { sketch } => sketch,
        o => panic!("{o:?}"),
    };
    let region = match ok(
        d,
        Op::AddRectangle {
            sketch,
            x: at[0],
            y: at[1],
            w: size[0],
            h: size[1],
        },
    ) {
        OpOutcome::SketchEdited { regions } => regions[0],
        o => panic!("{o:?}"),
    };
    match ok(
        d,
        Op::Extrude {
            sketch,
            region,
            distance: size[2],
        },
    ) {
        OpOutcome::ObjectCreated { object } => object,
        o => panic!("{o:?}"),
    }
}

/// A box (`w`, `d`, `h`) with a set of its corners moved and folded (Move with autofold);
/// a fold the kernel refuses leaves the plain box, which is tested all the same.
fn folded_box(size: [f64; 3], fold: &Fold) -> (Document, u64) {
    let mut d = Document::new();
    let object = add_box(&mut d, [0.0; 3], size);
    let corners: Vec<[f64; 3]> = (0..8)
        .filter(|&k| fold.corners[k])
        .map(|k| {
            [
                if k & 1 == 0 { 0.0 } else { size[0] },
                if k & 2 == 0 { 0.0 } else { size[1] },
                if k & 4 == 0 { 0.0 } else { size[2] },
            ]
        })
        .collect();
    if !corners.is_empty() && corners.len() < 8 {
        let axis = DVec3::from_array(fold.axis);
        let axis = if axis.length() < 1e-3 { DVec3::Z } else { axis };
        let centre = DVec3::from_array(size) * 0.5;
        let x = Xform::translation(DVec3::from_array(fold.shift))
            .after(&Xform::rotation(centre, axis, fold.angle));
        // Refused folds (a self-intersecting result) leave the box as it was.
        let _ = d.apply(&Op::TransformGeometry {
            object,
            points: corners,
            xform: x,
            autofold: true,
        });
    }
    (d, object)
}

fn vertices(m: &Model, object: u64) -> Vec<DVec3> {
    m.object(object)
        .unwrap()
        .brep()
        .vertices()
        .map(|(_, v)| v.pos)
        .collect()
}

fn plane_origin(m: &Model, object: u64, at: &At) -> [f64; 3] {
    let vs = vertices(m, object);
    match at {
        At::Vertex(k) => vs[k % vs.len()].to_array(),
        At::Inside(f) => {
            let lo = vs
                .iter()
                .fold(DVec3::splat(f64::INFINITY), |a, v| a.min(*v));
            let hi = vs
                .iter()
                .fold(DVec3::splat(f64::NEG_INFINITY), |a, v| a.max(*v));
            (lo + (hi - lo) * DVec3::from_array(*f)).to_array()
        }
    }
}

fn add_section(d: &mut Document, origin: [f64; 3], normal: [f64; 3]) -> u64 {
    match d.apply(&Op::AddSection {
        origin,
        normal,
        name: None,
        active: true,
        hidden: false,
    }) {
        Ok(OpOutcome::SectionAdded { section }) => section,
        o => panic!("{o:?}"),
    }
}

/// Segments `ab` and `cd` cross at a point inside both (exactly).
fn proper_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let o = |p: DVec2, q: DVec2, r: DVec2| {
        robust::orient2d(
            robust::Coord { x: p.x, y: p.y },
            robust::Coord { x: q.x, y: q.y },
            robust::Coord { x: r.x, y: r.y },
        )
    };
    o(a, b, c) * o(a, b, d) < 0.0 && o(c, d, a) * o(c, d, b) < 0.0
}

/// Every cap ring is simple (no point twice, no two of its edges crossing, no crossing with
/// another ring of its cap), outer rings counter-clockwise, holes clockwise.
fn caps_are_simple(m: &Model, section: u64) -> Result<(), String> {
    let cut = m.section_cut(section).map_err(|e| e.to_string())?;
    for c in &cut.caps {
        let rings: Vec<Vec<DVec2>> = std::iter::once(&c.outer)
            .chain(&c.holes)
            .map(|r| r.iter().map(|p| DVec2::from_array(*p)).collect())
            .collect();
        for (k, r) in rings.iter().enumerate() {
            for i in 0..r.len() {
                for j in i + 1..r.len() {
                    if r[i] == r[j] {
                        return Err(format!("a cap ring passes {:?} twice: {r:?}", r[i]));
                    }
                }
            }
            let a = sawhorse_tessellate::signed_area(r);
            if (k == 0) != (a > 0.0) {
                return Err(format!("cap ring {k} has area {a}"));
            }
        }
        let edges: Vec<(usize, usize, DVec2, DVec2)> = rings
            .iter()
            .enumerate()
            .flat_map(|(k, r)| (0..r.len()).map(move |i| (k, i, r[i], r[(i + 1) % r.len()])))
            .collect();
        for (x, &(k1, i1, a, b)) in edges.iter().enumerate() {
            for &(k2, i2, c, d) in &edges[x + 1..] {
                let n = rings[k1].len();
                let adjacent = k1 == k2 && ((i1 + 1) % n == i2 || (i2 + 1) % n == i1);
                if !adjacent && proper_cross(a, b, c, d) {
                    return Err(format!(
                        "cap edges cross: ring {k1} edge {i1} and ring {k2} edge {i2}"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The plane's cross-section of what `section` reaches, worked out without the cap: every
/// face clipped to the kept side, its vector area (Newell, over its 3D corners and
/// crossings) along the normal, summed. The kept part is a closed surface whose only face
/// along the plane is the cap, facing `-normal`, so that sum is the cap's area. The tie rule
/// is the documented one, restated: a distance within 1e-12 of zero is zero, and every corner
/// of a face parallel to the plane (normals within 1e-12) or lying within 1e-9 of it (or
/// within twice the face's bend off its own plane, when that is more) takes the face's mean
/// distance. Returns the area and how far it may honestly differ from the cap's:
/// the bend allowance of `check_sections` (a face planar to within PLANARITY meets the plane
/// along chords up to its bend apart).
fn independent_cross_section(m: &Model, section: u64) -> (f64, f64) {
    let s = m.section(section).unwrap();
    let (o, n) = (s.origin, s.normal);
    let mut area = 0.0;
    let mut surface = 0.0;
    let mut bend: f64 = 0.0;
    for (id, x) in m.objects() {
        if !m.is_visible(id) {
            continue;
        }
        let b = x.brep();
        let mut dist: std::collections::BTreeMap<_, f64> =
            b.vertices().map(|(k, v)| (k, (v.pos - o).dot(n))).collect();
        let raw = dist.clone();
        for (_, f) in b.faces() {
            let vs: Vec<_> = f.loops().flat_map(|l| b.loop_vertices(l)).collect();
            let flat = n.cross(f.plane.normal).length() < 1e-12;
            let own = vs
                .iter()
                .map(|v| f.plane.signed_distance(b.vertex(*v).unwrap().pos).abs())
                .fold(0.0, f64::max);
            let near = (2.0 * own).max(1e-9);
            if !(flat || vs.iter().all(|v| raw[v].abs() <= near)) {
                continue;
            }
            if vs.iter().all(|v| dist[v] == dist[&vs[0]]) {
                continue;
            }
            let mean = vs.iter().map(|v| dist[v]).sum::<f64>() / vs.len() as f64;
            for v in vs {
                dist.insert(v, mean);
            }
        }
        for d in dist.values_mut() {
            if d.abs() < 1e-12 {
                *d = 0.0;
            }
        }
        for (_, f) in b.faces() {
            for l in f.loops() {
                let vs = b.loop_vertices(l);
                for v in &vs {
                    bend = bend.max(f.plane.signed_distance(b.vertex(*v).unwrap().pos).abs());
                }
                let mut ring: Vec<DVec3> = Vec::new();
                for i in 0..vs.len() {
                    let (p, q) = (vs[i], vs[(i + 1) % vs.len()]);
                    let (dp, dq) = (dist[&p], dist[&q]);
                    let (pp, pq) = (b.vertex(p).unwrap().pos, b.vertex(q).unwrap().pos);
                    if dp >= 0.0 {
                        ring.push(pp);
                    }
                    if (dp >= 0.0) != (dq >= 0.0) {
                        ring.push(pp + (pq - pp) * (dp / (dp - dq)));
                    }
                }
                let mut va = DVec3::ZERO;
                for i in 0..ring.len() {
                    va += ring[i].cross(ring[(i + 1) % ring.len()]) * 0.5;
                }
                area += va.dot(n);
                let all: Vec<DVec3> = b.loop_points(l);
                let mut whole = DVec3::ZERO;
                for i in 0..all.len() {
                    whole += all[i].cross(all[(i + 1) % all.len()]) * 0.5;
                }
                surface += whole.length();
            }
        }
    }
    let cut = m.section_cut(section).unwrap();
    let perimeter: f64 = cut
        .caps
        .iter()
        .flat_map(|c| std::iter::once(&c.outer).chain(&c.holes))
        .map(|r| {
            (0..r.len())
                .map(|i| DVec2::from_array(r[i]).distance(DVec2::from_array(r[(i + 1) % r.len()])))
                .sum::<f64>()
        })
        .sum();
    (
        area,
        1e-9 * (1.0 + surface) + 2.0 * bend * perimeter.max(1.0),
    )
}

/// The caps' area is the independent cross-section.
fn cap_is_the_cross_section(m: &Model, section: u64) -> Result<(), String> {
    let cut = m.section_cut(section).map_err(|e| e.to_string())?;
    let (indep, slack) = independent_cross_section(m, section);
    // Every cap counts, representable or not: the cut keeps each exactly as the plane cuts
    // it (lane 18; it drops only a cap of no area at all, under 1e-16 m2, the arrangement's
    // rounding). Lane 17 dropped rings that welded to fewer than three corners, so slivers of
    // up to 2.4e-6 m2 went missing here, 2,000 times this allowance.
    if (cut.area - indep).abs() > slack + 1e-15 {
        return Err(format!(
            "the caps cover {} m2, the faces clipped to the plane {indep} m2 (allowed {slack})",
            cut.area
        ));
    }
    Ok(())
}

/// Create Group from Slice on `section` (when it cuts anything) draws closed loops: its
/// sketch's faces cover every representable cap's outer ring (the union of them: an island in
/// a hole is inside its outer ring, and two solids that overlap overlap here too). The sketch
/// may hold more: a region every edge of which is a cap's edge but which no cap holds (a face
/// lying on the plane, facing away from what it keeps, between two caps that touch at its
/// corners) closes into a face of the sketch like any other loop. When the plane cuts caps
/// but none is representable, the slice says so ("thinner than 0.01 mm"), and only then.
fn slice_is_closed(d: &Document, section: u64) -> Result<(), String> {
    let cut = d.model().section_cut(section).map_err(|e| e.to_string())?;
    if cut.caps.is_empty() {
        return Ok(());
    }
    let shown: Vec<&sawhorse_kernel::section::Cap> =
        cut.caps.iter().filter(|c| c.representable).collect();
    let mut d = d.clone();
    if shown.is_empty() {
        return match d.apply(&Op::SliceToGroup {
            section,
            name: None,
        }) {
            Err(e) if e.to_string() == THIN => Ok(()),
            Err(e) if e.to_string().contains("inside a component") => Ok(()),
            other => Err(format!(
                "no cap is representable, yet the slice gave {other:?} ({:?})",
                cut.caps
            )),
        };
    }
    // Slicing is for the model's own context; a plane inside a component is refused there.
    let sketch = match d.apply(&Op::SliceToGroup {
        section,
        name: None,
    }) {
        Ok(OpOutcome::Sliced { sketch, .. }) => sketch,
        Ok(o) => return Err(format!("slice: {o:?}")),
        Err(e) if e.to_string().contains("inside a component") => return Ok(()),
        Err(e) => return Err(format!("slice refused: {e}")),
    };
    let sk = d.model().sketch(sketch).ok_or("no sketch")?;
    let union = |rings: Vec<Vec<[f64; 2]>>| {
        // One outer ring at a time (the overlay fills even-odd, which would make an island
        // a hole of the ring around it).
        let mut shapes: Vec<Vec<Vec<[f64; 2]>>> = Vec::new();
        for r in rings {
            let flat: Vec<Vec<[f64; 2]>> = shapes.iter().flatten().cloned().collect();
            shapes = sawhorse_tessellate::overlay::union(&flat, &[r]);
        }
        shapes
    };
    let faces = union(
        sk.regions()
            .map(|(_, r)| sk.region_rings(r).0.iter().map(|p| p.to_array()).collect())
            .collect(),
    );
    let caps = union(shown.iter().map(|c| c.outer.clone()).collect());
    let outers = sawhorse_tessellate::overlay::area(&caps);
    let flat = |s: &Vec<Vec<Vec<[f64; 2]>>>| s.iter().flatten().cloned().collect::<Vec<_>>();
    let covered = sawhorse_tessellate::overlay::area(&sawhorse_tessellate::overlay::intersection(
        &flat(&faces),
        &flat(&caps),
    ));
    // The slice merges points within POINT_MERGE (`section::representable`), so a spike of a
    // cap thinner than that folds flat: at most POINT_MERGE along its edges.
    let perimeter: f64 = cut
        .outline
        .iter()
        .map(|[a, b]| DVec2::from_array(*a).distance(DVec2::from_array(*b)))
        .sum();
    let slack = 1e-9 * (1.0 + outers) + 2.0 * sawhorse_kernel::tolerance::POINT_MERGE * perimeter;
    if (covered - outers).abs() > slack {
        return Err(format!(
            "the slice's faces cover {covered} m2 of the caps' outer rings' {outers} m2"
        ));
    }
    Ok(())
}

/// What Create Group from Slice says when every cap is thinner than the model can draw.
const THIN: &str = "the section cut here is thinner than 0.01 mm; nothing to slice";

/// Every property, for the plane and for its flip.
fn cut_cleanly(d: &mut Document, section: u64) -> Result<(), TestCaseError> {
    for _ in 0..2 {
        check_sections(d.model()).map_err(TestCaseError::fail)?;
        caps_are_simple(d.model(), section).map_err(TestCaseError::fail)?;
        cap_is_the_cross_section(d.model(), section).map_err(TestCaseError::fail)?;
        slice_is_closed(d, section).map_err(TestCaseError::fail)?;
        d.apply(&Op::FlipSection { section }).unwrap();
    }
    Ok(())
}

/// A unit vector from any non-zero triple (or `fallback`).
fn unit(v: DVec3, fallback: DVec3) -> [f64; 3] {
    let l = v.length();
    if l > 1e-6 {
        (v / l).to_array()
    } else {
        fallback.to_array()
    }
}

/// A frame: a `w` x `d` rectangle with a rectangular hole (`hole` = x0, y0, x1, y1 as
/// fractions), pulled up `h`, then turned by `turn` (axis and angle) about its centre.
#[derive(Debug, Clone)]
struct Frame {
    size: [f64; 3],
    hole: [f64; 4],
    turn: Option<([f64; 3], f64)>,
}

fn frame() -> impl Strategy<Value = Frame> {
    (
        prop::array::uniform3(quarters()),
        (1i32..4, 1i32..4, 5i32..8, 5i32..8),
        prop_oneof![
            2 => Just(None),
            1 => (prop::array::uniform3(-1.0..1.0f64), -3.0..3.0f64).prop_map(Some),
        ],
    )
        .prop_map(|(size, (a, b, c, e), turn)| Frame {
            size,
            hole: [
                f64::from(a) * 0.125,
                f64::from(b) * 0.125,
                f64::from(c) * 0.125,
                f64::from(e) * 0.125,
            ],
            turn,
        })
}

/// The frame prism, its turn, and the corners of its hole on top and bottom (in the turned
/// position).
fn frame_prism(f: &Frame) -> (Document, u64, Xform, Vec<[DVec3; 2]>) {
    let mut d = Document::new();
    let [w, dd, h] = f.size;
    let sketch = match ok(
        &mut d,
        Op::NewSketch {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        },
    ) {
        OpOutcome::SketchCreated { sketch } => sketch,
        o => panic!("{o:?}"),
    };
    ok(
        &mut d,
        Op::AddRectangle {
            sketch,
            x: 0.0,
            y: 0.0,
            w,
            h: dd,
        },
    );
    let (x0, y0, x1, y1) = (f.hole[0] * w, f.hole[1] * dd, f.hole[2] * w, f.hole[3] * dd);
    ok(
        &mut d,
        Op::AddRectangle {
            sketch,
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        },
    );
    let sk = d.model().sketch(sketch).unwrap();
    let ring = sk
        .regions()
        .find(|(_, r)| !r.holes.is_empty())
        .map(|(id, _)| sawhorse_kernel::ids::to_public(id))
        .expect("a region with a hole");
    let object = match ok(
        &mut d,
        Op::Extrude {
            sketch,
            region: ring,
            distance: h,
        },
    ) {
        OpOutcome::ObjectCreated { object } => object,
        o => panic!("{o:?}"),
    };
    let x = match f.turn {
        Some((axis, angle)) => {
            let axis = DVec3::from_array(axis);
            let axis = if axis.length() < 1e-3 { DVec3::X } else { axis };
            Xform::rotation(DVec3::new(w, dd, h) * 0.5, axis, angle)
        }
        None => Xform::IDENTITY,
    };
    if f.turn.is_some() {
        ok(
            &mut d,
            Op::Transform {
                objects: vec![object],
                xform: x,
                nodes: Vec::new(),
            },
        );
    }
    let mut edges = Vec::new();
    for z in [0.0, h] {
        let c = [
            DVec3::new(x0, y0, z),
            DVec3::new(x1, y0, z),
            DVec3::new(x1, y1, z),
            DVec3::new(x0, y1, z),
        ];
        for i in 0..4 {
            edges.push([x.apply(c[i]), x.apply(c[(i + 1) % 4])]);
        }
    }
    (d, object, x, edges)
}

/// A box with one face's corners (`which` of them) moved by `shift`, at most a micrometre
/// along the face's normal, autofold on or off; sometimes that face split across. When only
/// some corners move, they move across by less than half the face's shorter side: further,
/// the face turns inside out, and a solid a micrometre thick and inside out (accepted:
/// thinner than the merge distance is coincident) has no cross-section a cap could close.
#[derive(Debug, Clone)]
struct Sliver {
    size: [f64; 3],
    face: usize,
    which: Vec<bool>,
    along: f64,
    across: [f64; 2],
    autofold: bool,
    split: Option<(f64, f64)>,
    flip: bool,
    tilt: f64,
}

fn sliver() -> impl Strategy<Value = Sliver> {
    (
        prop::array::uniform3(quarters()),
        0usize..6,
        prop_oneof![
            2 => Just(vec![true; 4]),
            1 => prop::collection::vec(any::<bool>(), 4),
        ],
        prop_oneof![
            1 => Just(1e-6),
            1 => Just(-1e-6),
            2 => -1e-6..1e-6f64,
            1 => -1e-9..1e-9f64,
        ],
        prop::array::uniform2(prop_oneof![
            1 => Just(0.0),
            2 => (-4i32..4).prop_map(|k| f64::from(k) * 0.125),
        ]),
        any::<bool>(),
        prop_oneof![1 => Just(None), 1 => (0.05..0.95f64, 0.05..0.95f64).prop_map(Some)],
        any::<bool>(),
        prop_oneof![3 => Just(0.0), 1 => -1e-7..1e-7f64],
    )
        .prop_map(
            |(size, face, which, along, across, autofold, split, flip, tilt)| Sliver {
                size,
                face,
                which,
                along,
                across,
                autofold,
                split,
                flip,
                tilt,
            },
        )
}

/// Axis `k` of a box face: faces 0..6 are -x, +x, -y, +y, -z, +z.
fn face_axis(face: usize) -> (usize, bool) {
    (face / 2, face % 2 == 1)
}

fn sliver_box(s: &Sliver) -> (Document, u64, [f64; 3], [f64; 3]) {
    let mut d = Document::new();
    let object = add_box(&mut d, [0.0; 3], s.size);
    let (axis, high) = face_axis(s.face);
    let level = if high { s.size[axis] } else { 0.0 };
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut corners = Vec::new();
    for (k, (a, b)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        if !s.which[k] {
            continue;
        }
        let mut p = [0.0; 3];
        p[axis] = level;
        p[u] = a * s.size[u];
        p[v] = b * s.size[v];
        corners.push(p);
    }
    let lim = if s.which.iter().all(|&w| w) {
        f64::INFINITY
    } else {
        0.45 * s.size[u].min(s.size[v])
    };
    let mut shift = [0.0; 3];
    shift[axis] = s.along;
    shift[u] = s.across[0].clamp(-lim, lim);
    shift[v] = s.across[1].clamp(-lim, lim);
    if !corners.is_empty() {
        let _ = d.apply(&Op::TransformGeometry {
            object,
            points: corners,
            xform: Xform::translation(DVec3::from_array(shift)),
            autofold: s.autofold,
        });
    }
    if let Some((fa, fb)) = s.split {
        // Split the face nearest the plane from one of its edges to another.
        let m = d.model();
        let o = m.object(object).unwrap();
        let b = o.brep();
        let ids = b.persistent_face_ids();
        let mut want = DVec3::ZERO;
        want[axis] = if high { 1.0 } else { -1.0 };
        if let Some((k, f)) = b
            .faces()
            .enumerate()
            .map(|(k, (_, f))| (k, f))
            .max_by(|x, y| {
                x.1.plane
                    .normal
                    .dot(want)
                    .total_cmp(&y.1.plane.normal.dot(want))
            })
        {
            let pts: Vec<DVec3> = f
                .loops()
                .next()
                .map(|l| b.loop_points(l))
                .unwrap_or_default();
            if pts.len() >= 4 {
                let n = pts.len();
                let a = pts[0].lerp(pts[1], fa);
                let c = pts[n / 2].lerp(pts[(n / 2 + 1) % n], fb);
                let _ = d.apply(&Op::SplitFace {
                    object,
                    face: ids[k],
                    path: vec![a.to_array(), c.to_array()],
                });
            }
        }
    }
    let mut origin = [0.0; 3];
    origin[axis] = level;
    origin[u] = s.size[u] * 0.5;
    origin[v] = s.size[v] * 0.5;
    let mut normal = [0.0; 3];
    normal[axis] = if s.flip { 1.0 } else { -1.0 };
    normal[u] = s.tilt;
    (d, object, origin, normal)
}

/// A box with a pocket: three nested rectangles imprinted on its top, the ring between the
/// outer two pushed down (the pocket) and the inner one pushed down (a hole in the post the
/// pocket leaves), so a cut through the pocket has a hole with an island with a hole.
#[derive(Debug, Clone)]
struct Nested {
    size: [f64; 3],
    pocket: f64,
    hole: f64,
    level: f64,
    normal: [f64; 3],
}

fn nested() -> impl Strategy<Value = Nested> {
    (
        (0.5..1.0f64, 0.5..1.0f64, 0.25..1.0f64),
        0.2..0.9f64,
        0.1..0.95f64,
        0.0..1.0f64,
        prop_oneof![
            3 => Just([0.0, 0.0, 1.0]),
            2 => Just([0.0, 0.0, -1.0]),
            2 => prop::array::uniform3(-1.0..1.0f64),
        ],
    )
        .prop_map(|((w, d, h), pocket, hole, level, normal)| Nested {
            size: [w, d, h],
            pocket,
            hole,
            level,
            normal,
        })
}

fn top_face(d: &Document, object: u64, z: f64, inside: DVec3) -> Option<u64> {
    let o = d.model().object(object)?;
    let b = o.brep();
    let ids = b.persistent_face_ids();
    b.faces()
        .enumerate()
        .find(|(_, (_, f))| {
            f.plane.normal.z > 0.999 && (f.plane.origin.z - z).abs() < 1e-9 && {
                let rings: Vec<Vec<DVec2>> = f
                    .loops()
                    .map(|l| b.loop_points(l).iter().map(|p| p.truncate()).collect())
                    .collect();
                let p = inside.truncate();
                rings
                    .iter()
                    .filter(|r| {
                        let mut c = false;
                        for i in 0..r.len() {
                            let (a, e) = (r[i], r[(i + 1) % r.len()]);
                            if (a.y > p.y) != (e.y > p.y)
                                && p.x < a.x + (p.y - a.y) / (e.y - a.y) * (e.x - a.x)
                            {
                                c = !c;
                            }
                        }
                        c
                    })
                    .count()
                    % 2
                    == 1
            }
        })
        .map(|(k, _)| ids[k])
}

fn nested_box(n: &Nested) -> (Document, u64, [f64; 3]) {
    let mut d = Document::new();
    let [w, dd, h] = n.size;
    let object = add_box(&mut d, [0.0; 3], n.size);
    let rect = |k: f64| {
        let (mx, my) = (w * k, dd * k);
        vec![
            [mx, my, h],
            [w - mx, my, h],
            [w - mx, dd - my, h],
            [mx, dd - my, h],
        ]
    };
    let centre = DVec3::new(w * 0.5, dd * 0.5, h);
    let depth = n.pocket * h;
    let mut faces = Vec::new();
    let mut face = top_face(&d, object, h, centre);
    for k in [0.1, 0.25, 0.4] {
        let Some(f) = face else { break };
        match d.apply(&Op::Imprint {
            object,
            face: f,
            points: rect(k),
        }) {
            Ok(OpOutcome::Imprinted { face: sub }) => {
                faces.push(sub);
                face = Some(sub);
            }
            _ => break,
        }
    }
    // faces[0]: inside the outer rectangle (the ring down to the second); faces[2]: the
    // innermost.
    if faces.len() == 3 {
        let _ = d.apply(&Op::PushPull {
            object,
            face: faces[0],
            distance: -depth,
        });
        let _ = d.apply(&Op::PushPull {
            object,
            face: faces[2],
            distance: -(n.hole * h * 0.95),
        });
    }
    let origin = [w * 0.37, dd * 0.41, h - depth * n.level];
    (d, object, origin)
}

proptest! {
    // PROPTEST_CASES raises it for a long run.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        ..ProptestConfig::default()
    })]

    #[test]
    fn folded_solids_cut_into_halves_that_make_the_whole(
        size in prop::array::uniform3(quarters()),
        fold in fold(),
        at in at(),
        normal in normal(),
    ) {
        let n = DVec3::from_array(normal);
        prop_assume!(n.length() > 0.05);
        snapping_off(|| -> Result<(), TestCaseError> {
            let (mut d, object) = folded_box(size, &fold);
            let origin = plane_origin(d.model(), object, &at);
            let section = add_section(&mut d, origin, normal);
            cut_cleanly(&mut d, section)
        })?;
    }

    /// The CX2 shape: a frame prism cut by a plane through an edge of its hole, turned
    /// about that edge by any angle (along the top, at 45 degrees, straight down the wall).
    #[test]
    fn frames_cut_along_a_hole_edge(
        f in frame(),
        edge in 0usize..8,
        angle in prop_oneof![
            1 => Just(std::f64::consts::FRAC_PI_4),
            1 => Just(-std::f64::consts::FRAC_PI_4),
            1 => Just(0.0),
            1 => Just(std::f64::consts::FRAC_PI_2),
            1 => Just(std::f64::consts::PI),
            3 => -3.2..3.2f64,
        ],
        snap_on in any::<bool>(),
    ) {
        snapping(snap_on, || -> Result<(), TestCaseError> {
            let (mut d, _, x, edges) = frame_prism(&f);
            let [a, b] = edges[edge];
            let along = (b - a).normalize();
            // A normal at right angles to the edge, turned by `angle` about it from the
            // frame's own up.
            let up = (x.apply(DVec3::Z) - x.apply(DVec3::ZERO)).normalize();
            let side = along.cross(up);
            let normal = up * angle.cos() + side * angle.sin();
            let section = add_section(&mut d, a.to_array(), unit(normal, up));
            cut_cleanly(&mut d, section)
        })?;
    }

    /// Two boxes meeting at one corner, cut through it.
    #[test]
    fn solids_touching_at_a_vertex(
        a in prop::array::uniform3(quarters()),
        b in prop::array::uniform3(quarters()),
        normal in normal(),
        fold in prop_oneof![2 => Just(None), 1 => fold().prop_map(Some)],
    ) {
        let n = DVec3::from_array(normal);
        prop_assume!(n.length() > 0.05);
        snapping_off(|| -> Result<(), TestCaseError> {
            let (mut d, _) = match &fold {
                Some(f) => folded_box(a, f),
                None => {
                    let mut d = Document::new();
                    let o = add_box(&mut d, [0.0; 3], a);
                    (d, o)
                }
            };
            add_box(&mut d, a, b);
            let section = add_section(&mut d, a, normal);
            cut_cleanly(&mut d, section)
        })?;
    }

    /// CX1's family: a face within a micrometre of the plane (sheared, tilted or bent by
    /// moving some of its corners, sometimes split across), cut along where it was.
    #[test]
    fn sliver_faces_within_a_micrometre_of_the_plane(
        s in sliver(),
        snap_on in any::<bool>(),
    ) {
        snapping(snap_on, || -> Result<(), TestCaseError> {
            let (mut d, _, origin, normal) = sliver_box(&s);
            let section = add_section(&mut d, origin, unit(DVec3::from_array(normal), DVec3::Z));
            cut_cleanly(&mut d, section)
        })?;
    }

    /// Planes through one, two, three or four vertices of a folded box.
    #[test]
    fn planes_through_one_to_four_vertices(
        size in prop::array::uniform3(quarters()),
        fold in fold(),
        picks in prop::array::uniform4(0usize..64),
        k in 1usize..=4,
        free in prop::array::uniform3(-1.0..1.0f64),
    ) {
        snapping_off(|| -> Result<(), TestCaseError> {
            let (mut d, object) = folded_box(size, &fold);
            let vs = vertices(d.model(), object);
            let p: Vec<DVec3> = picks.iter().map(|i| vs[i % vs.len()]).collect();
            let free = DVec3::from_array(free);
            let normal = match k {
                1 => free,
                2 => (p[1] - p[0]).cross(free),
                3 => (p[1] - p[0]).cross(p[2] - p[0]),
                // Four: the box's diagonal plane through two opposite edges (its corners
                // 0, 1, 6, 7 before the fold; those that did not move are on it).
                _ => DVec3::new(size[0], 0.0, 0.0).cross(DVec3::new(0.0, size[1], size[2])),
            };
            let origin = if k == 4 { DVec3::ZERO } else { p[0] };
            let section = add_section(&mut d, origin.to_array(), unit(normal, DVec3::Z));
            cut_cleanly(&mut d, section)
        })?;
    }

    /// A hole with an island with a hole.
    #[test]
    fn nested_holes(n in nested(), snap_on in any::<bool>()) {
        let normal = DVec3::from_array(n.normal);
        prop_assume!(normal.length() > 0.05);
        snapping(snap_on, || -> Result<(), TestCaseError> {
            let (mut d, _, origin) = nested_box(&n);
            let section = add_section(&mut d, origin, unit(normal, DVec3::Z));
            cut_cleanly(&mut d, section)
        })?;
    }

    /// A plane exactly on a face of a folded box, facing into the solid or out of it.
    #[test]
    fn planes_exactly_on_a_face(
        size in prop::array::uniform3(quarters()),
        fold in fold(),
        face in 0usize..64,
        out in any::<bool>(),
    ) {
        snapping_off(|| -> Result<(), TestCaseError> {
            let (mut d, object) = folded_box(size, &fold);
            let (origin, normal) = {
                let b = d.model().object(object).unwrap().brep();
                let fs: Vec<_> = b.faces().map(|(_, f)| f).collect();
                let f = fs[face % fs.len()];
                let p = f.loops().next().map(|l| b.loop_points(l)[0]).unwrap();
                (p, if out { f.plane.normal } else { -f.plane.normal })
            };
            let section = add_section(&mut d, origin.to_array(), normal.to_array());
            cut_cleanly(&mut d, section)
        })?;
    }
}

/// Every section counterexample in `tests/repro`, played to its end: every plane's caps are
/// simple and are the cross-section, its halves make the whole, and its slice closes.
#[test]
fn the_section_finds_cut_cleanly() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repro");
    let mut seen = 0;
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("-section-")
        })
        .collect();
    names.sort();
    for path in names {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let r: Repro = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        snapping(r.plane_snap.unwrap_or(true), || {
            let mut d = Document::new();
            for op in &r.ops {
                let _ = d.apply(op);
            }
            let m = d.model();
            check_sections(m).unwrap_or_else(|e| panic!("{name}: {e}"));
            let ids: Vec<u64> = m.sections().into_iter().map(|(id, _)| id).collect();
            for id in ids {
                caps_are_simple(m, id).unwrap_or_else(|e| panic!("{name}: {e}"));
                // Other active planes clip a cap, and a plane in a group or component reaches
                // only what that holds; the independent cross-section is of one plane in the
                // model, reaching every solid.
                let alone = m.sections().iter().all(|(k, s)| !s.active || *k == id);
                if alone && m.section(id).unwrap().parent.is_none() {
                    cap_is_the_cross_section(m, id).unwrap_or_else(|e| panic!("{name}: {e}"));
                }
                slice_is_closed(&d, id).unwrap_or_else(|e| panic!("{name}: {e}"));
                // Within what check_sections allows a half: a face a micrometre off the plane
                // is kept whole by the tie rule.
                let kept = m.kept_part(id).unwrap();
                let whole: f64 = m.objects().iter().map(|(_, o)| o.volume()).sum();
                let slack =
                    1e-6 * (1.0 + whole) + sawhorse_kernel::tolerance::POINT_MERGE * kept.area;
                assert!(
                    kept.volume >= -slack && kept.volume <= whole + slack,
                    "{name}: {kept:?} of {whole}"
                );
            }
        });
        seen += 1;
    }
    assert!(seen >= 21, "{seen} section repros");
}

/// The two lane-12c verification finds, exactly: the frame's cut-away half has its notch
/// (the cap is the rectangle less the hole, 24% less than a plain rectangle), and the split
/// box keeps its cap (the kept half is closed, not 6.6% short).
#[test]
fn the_lane_12c_verification_finds() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repro");
    let load = |n: &str| -> Document {
        let r: Repro =
            serde_json::from_str(&std::fs::read_to_string(dir.join(n)).unwrap()).unwrap();
        snapping(r.plane_snap.unwrap_or(true), || {
            let mut d = Document::new();
            for op in &r.ops {
                let _ = d.apply(op);
            }
            d
        })
    };
    // CX2: the plane runs along the top edge of the frame's hole. Its two sides cut the
    // same notched cap, in opposite directions.
    let mut d = load("2026-10-13-section-frame-prism-along-a-hole-edge.json");
    let s = d.model().sections()[0].0;
    let a = d.model().section_cut(s).unwrap();
    d.apply(&Op::FlipSection { section: s }).unwrap();
    let b = d.model().section_cut(s).unwrap();
    assert_eq!(a.caps.len(), 1);
    assert_eq!(b.caps.len(), 1);
    assert!((a.area - b.area).abs() < 1e-12, "{} {}", a.area, b.area);
    // The frame's section: (0.375 x 0.3125 less the 0.25 x 0.25 hole) on a 45-degree plane.
    let expect = (0.375 * 0.3125 - 0.25 * 0.25) * std::f64::consts::SQRT_2;
    assert!((a.area - expect).abs() < 1e-9, "{} {expect}", a.area);
    assert!(
        b.caps[0].holes.is_empty() && b.caps[0].outer.len() == 8,
        "{:?}",
        b.caps[0]
    );
    // CX1: the kept half's cap is there and closes it.
    let d = load("2026-10-13-section-cap-lost-after-split.json");
    let m = d.model();
    let s = m.sections()[0].0;
    let p = m.kept_part(s).unwrap();
    assert!(p.cap_length > 4.0, "{p:?}");
    assert!(p.vector_area.length() < 1e-5, "{p:?}");
    check_sections(m).unwrap();
}

/// The property tests' own find (lane 12d): a box whose top corner is moved 0.3 micrometres
/// up and in, folded, cut by a plane through the other three top corners. The top face is
/// dented and bent by its own planarity; classified corner by corner it clipped to a triangle
/// over the dent that no cap could close (the chain and the arrangement both failed it).
/// Within twice its bend of the plane, it lies on it.
#[test]
fn a_face_within_its_bend_of_the_plane_lies_on_it() {
    snapping_off(|| {
        let mut d = Document::new();
        let object = add_box(&mut d, [0.0; 3], [0.625, 0.5, 0.125]);
        d.apply(&Op::TransformGeometry {
            object,
            points: vec![[0.625, 0.0, 0.125]],
            xform: Xform::translation(DVec3::new(-0.25, 0.375, 3.1878639597293424e-7)),
            autofold: true,
        })
        .unwrap();
        let section = add_section(&mut d, [0.3125, 0.25, 0.125], [0.0, 0.0, -1.0]);
        cut_cleanly(&mut d, section).unwrap();
    });
}

/// The two lane-12d verification finds (`PROPTEST_CASES=50000`, run 1 of 2; lane 17):
/// Create Group from Slice and the cut disagreed on what a cap is.
///
/// - A folded box cut where one vertex pokes 4 micrometres through the plane: the cut showed
///   a 4.8e-12 m2 ring, every edge shorter than POINT_MERGE, and the slice, skipping those
///   edges, refused with "too small". Since lane 18 the cut keeps that speck (it is material)
///   and flags it not representable, and the slice says the cut is thinner than 0.01 mm,
///   on both sides.
/// - A folded box cut through a vertex: one hexagonal cap of 0.0221 m2 whose tip has three
///   points within ten micrometres. The slice skipped the two short edges and left the loop
///   open (no face) while saying its faces were ready to push. It now closes, covering the cap.
#[test]
fn the_lane_12d_verification_finds() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repro");
    let load = |n: &str| -> Document {
        let r: Repro =
            serde_json::from_str(&std::fs::read_to_string(dir.join(n)).unwrap()).unwrap();
        snapping(r.plane_snap.unwrap_or(true), || {
            let mut d = Document::new();
            for op in &r.ops {
                d.apply(op).unwrap();
            }
            d
        })
    };
    snapping_off(|| {
        // The speck: kept by the cut where the plane cuts it, never representable; the slice
        // says why it draws nothing, either way round.
        let mut d = load("2026-10-13-section-slice-refused-a-speck-of-cap.json");
        let s = d.model().sections()[0].0;
        let mut specks = 0;
        for _ in 0..2 {
            let cut = d.model().section_cut(s).unwrap();
            assert!(cut.caps.is_empty() == cut.outline.is_empty(), "{cut:?}");
            assert!(cut.caps.iter().all(|c| !c.representable), "{cut:?}");
            assert!(cut.area < 1e-11, "{cut:?}");
            specks += cut.caps.len();
            let e = d
                .clone()
                .apply(&Op::SliceToGroup {
                    section: s,
                    name: None,
                })
                .unwrap_err()
                .to_string();
            if cut.caps.is_empty() {
                assert!(e.contains("cuts nothing here"), "{e}");
            } else {
                assert_eq!(e, THIN);
            }
            cut_cleanly(&mut d.clone(), s).unwrap();
            d.apply(&Op::FlipSection { section: s }).unwrap();
        }
        assert!(
            specks > 0,
            "the cut keeps the speck on the side it pokes through"
        );

        // The tip: one cap, and the slice's face covers it.
        let mut d = load("2026-10-13-section-slice-left-open-at-a-tip.json");
        let s = d.model().sections()[0].0;
        let cut = d.model().section_cut(s).unwrap();
        assert_eq!(cut.caps.len(), 1, "{cut:?}");
        assert!(cut.caps[0].representable);
        assert!(
            (cut.area - 0.022092796851717753).abs() < 1e-9,
            "{}",
            cut.area
        );
        let mut sliced = d.clone();
        let sketch = match sliced
            .apply(&Op::SliceToGroup {
                section: s,
                name: None,
            })
            .unwrap()
        {
            OpOutcome::Sliced { sketch, .. } => sketch,
            o => panic!("{o:?}"),
        };
        let sk = sliced.model().sketch(sketch).unwrap();
        assert_eq!(sk.regions().count(), 1, "the slice's loop is closed");
        slice_is_closed(&d, s).unwrap();
        cut_cleanly(&mut d, s).unwrap();
    });
}

/// A folded box as the property tests build it, cut by the plane `at` picks from its vertices
/// (a point and a direction).
fn folded_cut(
    size: [f64; 3],
    fold: &Fold,
    at: impl FnOnce(&[DVec3]) -> (DVec3, DVec3),
) -> (Document, u64) {
    let (mut d, object) = folded_box(size, fold);
    let (origin, normal) = at(&vertices(d.model(), object));
    let section = add_section(&mut d, origin.to_array(), unit(normal, DVec3::Z));
    (d, section)
}

/// The lane-17 verification's finds (lane 18), each failing on the lane-17 kernel; their seeds
/// are in `section_props.proptest-regressions` too.
///
/// - Seeds c7f65cce... (`folded_solids_cut_into_halves_that_make_the_whole`) and 8248c33f...
///   (`planes_through_one_to_four_vertices`, through three vertices), shrunk: a folded box
///   whose cap is a sliver 0.125 m long and a few micrometres wide (1.6e-7 and 2.8e-7 m2).
///   Lane 17 welded it to two corners and dropped it from the cut, so the caps covered 0 m2 of
///   what the faces clipped to the plane close. The cut keeps it on both sides (the halves and
///   the cross-section agree); it is not representable, so the slice says the cut is thinner
///   than 0.01 mm.
/// - Seed 5daea986... (the lane-12d verification's unshrunk `planes_through_one_to_four_vertices`
///   find): a 0.0226 m2 cap whose tip has edges of 1.0e-5 and 1.5e-6 m. Welded by walking round,
///   one orientation kept a tip corner 9.9 micrometres from the next edge, which the sketch
///   folded into the corner before it, and the slice refused the cap as "too small"; flipped,
///   it inserted. Merged closest first, both orientations draw one face over the cap.
#[test]
fn the_lane_17_verification_finds() {
    snapping_off(|| {
        let slivers = [
            folded_cut(
                [0.125, 0.25, 0.125],
                &Fold {
                    corners: vec![false, false, true, true, false, false, true, true],
                    axis: [
                        0.5497345837255317,
                        -0.035317182389666704,
                        -0.38523898792518585,
                    ],
                    angle: -0.22254184501039867,
                    shift: [-0.1297484878255267, -0.125, 0.375],
                },
                |vs| (vs[47 % vs.len()], DVec3::X),
            ),
            folded_cut(
                [0.5, 0.5, 0.375],
                &Fold {
                    corners: vec![true, true, false, false, true, true, false, false],
                    axis: [
                        -0.19647241414043448,
                        0.33371854514174387,
                        0.8702247505221652,
                    ],
                    angle: -3.6236766389715407e-6,
                    shift: [0.25, -0.15449051954020962, 0.375],
                },
                |vs| {
                    let p: Vec<DVec3> = [44, 10, 58].iter().map(|i| vs[i % vs.len()]).collect();
                    (p[0], (p[1] - p[0]).cross(p[2] - p[0]))
                },
            ),
        ];
        for (k, (mut d, s)) in slivers.into_iter().enumerate() {
            for side in 0..2 {
                let cut = d.model().section_cut(s).unwrap();
                assert!(
                    !cut.caps.is_empty(),
                    "sliver {k}, side {side}: the cut keeps it"
                );
                assert!(
                    cut.area > 1e-7 && cut.area < 1e-6,
                    "sliver {k}, side {side}: {}",
                    cut.area
                );
                assert!(
                    cut.caps.iter().all(|c| !c.representable),
                    "sliver {k}, side {side}: {cut:?}"
                );
                let e = d
                    .clone()
                    .apply(&Op::SliceToGroup {
                        section: s,
                        name: None,
                    })
                    .unwrap_err();
                assert_eq!(e.to_string(), THIN, "sliver {k}, side {side}");
                cut_cleanly(&mut d.clone(), s).unwrap();
                d.apply(&Op::FlipSection { section: s }).unwrap();
            }
        }

        let (mut d, s) = folded_cut(
            [0.125, 0.625, 0.75],
            &Fold {
                corners: vec![false, false, false, false, true, true, true, true],
                axis: [
                    -0.16232463536875708,
                    0.16868704181458602,
                    -0.7534308543682535,
                ],
                angle: -0.129506946378861,
                shift: [
                    0.5264247490747762,
                    -0.10203994915735608,
                    -0.3875842074233285,
                ],
            },
            |vs| {
                (
                    vs[31 % vs.len()],
                    DVec3::new(0.6938111313663581, 0.21658302961471027, 0.732374827150452),
                )
            },
        );
        for side in 0..2 {
            let cut = d.model().section_cut(s).unwrap();
            assert_eq!(cut.caps.len(), 1, "side {side}: {cut:?}");
            assert!(cut.caps[0].representable, "side {side}");
            assert!(
                (cut.area - 0.022570250455469235).abs() < 1e-9,
                "side {side}: {}",
                cut.area
            );
            let mut sliced = d.clone();
            let sketch = match sliced
                .apply(&Op::SliceToGroup {
                    section: s,
                    name: None,
                })
                .unwrap_or_else(|e| panic!("side {side}: {e}"))
            {
                OpOutcome::Sliced { sketch, .. } => sketch,
                o => panic!("{o:?}"),
            };
            let sk = sliced.model().sketch(sketch).unwrap();
            assert_eq!(sk.regions().count(), 1, "side {side}: one closed face");
            cut_cleanly(&mut d.clone(), s).unwrap();
            d.apply(&Op::FlipSection { section: s }).unwrap();
        }
    });
}
