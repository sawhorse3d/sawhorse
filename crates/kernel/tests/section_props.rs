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
//!   of the solids worked out independently: every one of the solid's own triangles clipped
//!   to the kept side by its corners' distances, its vector area along the normal, summed
//!   (lane 19c: the triangles the cut itself is made of, so the oracle and the cut cannot
//!   drift apart);
//! - Create Group from Slice draws closed loops: the sketch's faces cover every representable
//!   cap's outer ring (`Cap::representable`, the kernel's one rule for what the slice can
//!   draw), and a section whose caps are all thinner than that is refused in its own words.
//!
//! Lane 19c rebuilt the cut so that it closes the half it caps by construction (one distance
//! per vertex, zero within a nanometre; the solid's own triangles clipped by them; the cap
//! laid from exactly the chords the kept triangles leave open; ADR-0010). Lanes 12d to 19b
//! had decided a corner's side by the faces and surfaces around it, and each of their finds
//! below now passes under that one rule; where a find's assertions spelled out what the old
//! rules made of it (a face "on the plane by its bend"), they now say what the one rule makes
//! of it, the geometry as it is.

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

/// The plane's cross-section of what `section` reaches, worked out without the cap (lane
/// 19c): every visible solid's own triangles (`Object::tri_solid`, the ones its volume is
/// measured by), each clipped to the kept side by its three corners' distances, the vector
/// area of what is kept (Newell, over its 3D corners and crossings) along the normal, summed
/// (`testing::cross_section`, which shares no code with the cut; `check_sections` holds every
/// cut to it too). The kept part is a closed surface whose only face along the plane is the
/// cap, facing `-normal`, so that sum is the cap's area. The tie rule is the documented one
/// (ADR-0010, lane 19c): a corner within 1e-9 of the plane is on it (distance zero), every
/// other corner is where it is, and zero counts as kept. Returns the area and how far it may
/// honestly differ from the cap's: a part per billion of the solids' area, and the cap's
/// corners moving by its arrangement's tolerance (far below a nanometre) along its rings.
fn independent_cross_section(m: &Model, section: u64) -> (f64, f64) {
    let s = m.section(section).unwrap();
    let mut area = 0.0;
    let mut surface = 0.0;
    for (id, x) in m.objects() {
        if !m.is_visible(id) {
            continue;
        }
        let (a, sa) = sawhorse_kernel::testing::cross_section(x, s.origin, s.normal)
            .expect("a committed solid triangulates");
        area += a;
        surface += sa;
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
        1e-9 * (1.0 + surface) + 2.0 * sawhorse_kernel::tolerance::LENGTH_EPS * perimeter,
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
    assert!(seen >= 31, "{seen} section repros");
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
/// dented and bent by its own planarity; classified corner by corner and clipped loop by loop
/// it clipped to a triangle over the dent that no cap could close (the chain and the
/// arrangement both failed it). Lanes 12d to 19b put a face within twice its bend of the plane
/// on it. Since lane 19c every corner is where it is (the moved one 0.3 micrometres above the
/// plane) and the face is clipped along its own triangles, whose chords the cap is laid from:
/// both halves close, and the caps are the cross-section of the triangles.
#[test]
fn a_dented_face_near_the_plane_is_cut_along_its_triangles() {
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

/// Lane 19: a box with a section plane lying on one of its faces, that face's corners (and
/// others) nudged by 0.1 to 5 micrometres, and rectangles and circles drawn on it, some across
/// its edges: the lane-18 verifier's find (seed 27182803) and its family. Every part of the
/// face a stroke splits off keeps the face's stored plane, so it must be classified with it.
/// Lane 19b widened it ([`Extra`]): a fourth nudged corner, a fourth stroke, and planes
/// through a poked corner instead of on the face (the lane-19 verifier's find: a corner poked
/// 2.5 micrometres out of the face, with autofold, and the facets around it).
#[derive(Debug, Clone)]
struct Nudged {
    size: [f64; 3],
    /// The face the plane lies on: -x, +x, -y, +y, -z, +z.
    face: usize,
    /// Corners moved (0..8, `(k & 1, k & 2, k & 4)` times the size), each by its own shift.
    nudges: Vec<(usize, [f64; 3])>,
    autofold: bool,
    /// Strokes on the face, before or after the nudges.
    strokes: Vec<Stroke>,
    strokes_first: bool,
    /// The plane facing out of the face (`true`) or into the box.
    out: bool,
    /// Lane 19b's additions, drawn after everything above so that a seed found before them
    /// still draws the same box, nudges, strokes and plane for those fields.
    extra: Extra,
}

/// What lane 19b added to [`Nudged`]: up to four nudged corners and four strokes, and where
/// the plane is.
#[derive(Debug, Clone)]
struct Extra {
    /// One more corner moved (after the others; autofold as they are).
    nudge: Option<(usize, [f64; 3])>,
    /// One more stroke (drawn with the others).
    stroke: Option<Stroke>,
    plane: PlaneAt,
}

/// Where the plane of a [`Nudged`] box is.
#[derive(Debug, Clone)]
enum PlaneAt {
    /// On the face as it was drawn (lane 19).
    Face,
    /// Through the corner the given nudge moved (modulo the nudges), where it is now: facing
    /// along the face's normal (`None`, either way as `out` says), or along a direction of its
    /// own.
    Corner {
        nudge: usize,
        normal: Option<[f64; 3]>,
    },
}

/// A closed stroke on the face, in fractions of its two sides (it may run past an edge).
#[derive(Debug, Clone)]
enum Stroke {
    Rect {
        centre: [f64; 2],
        half: [f64; 2],
    },
    Circle {
        centre: [f64; 2],
        radius: f64,
        sides: usize,
    },
}

fn nudge() -> impl Strategy<Value = [f64; 3]> {
    (
        prop_oneof![1 => 1e-7..1e-6f64, 2 => 1e-6..5e-6f64, 1 => Just(1e-6)],
        prop::array::uniform3(-1.0..1.0f64),
    )
        .prop_map(|(len, dir)| {
            let d = DVec3::from_array(dir);
            let d = if d.length() < 1e-3 {
                DVec3::Z
            } else {
                d.normalize()
            };
            (d * len).to_array()
        })
}

fn stroke() -> impl Strategy<Value = Stroke> {
    let at = || prop::array::uniform2(prop_oneof![3 => 0.1..0.9f64, 1 => -0.1..1.1f64]);
    prop_oneof![
        2 => (at(), prop::array::uniform2(0.02..0.4f64))
            .prop_map(|(centre, half)| Stroke::Rect { centre, half }),
        1 => (at(), 0.02..0.4f64, 8usize..25)
            .prop_map(|(centre, radius, sides)| Stroke::Circle { centre, radius, sides }),
    ]
}

fn extra() -> impl Strategy<Value = Extra> {
    (
        prop::option::of((0usize..8, nudge())),
        prop::option::of(stroke()),
        prop_oneof![
            1 => Just(PlaneAt::Face),
            1 => (0usize..4, prop::option::of(normal()))
                .prop_map(|(nudge, normal)| PlaneAt::Corner { nudge, normal }),
        ],
    )
        .prop_map(|(nudge, stroke, plane)| Extra {
            nudge,
            stroke,
            plane,
        })
}

fn nudged() -> impl Strategy<Value = Nudged> {
    (
        prop::array::uniform3(quarters()),
        0usize..6,
        prop::collection::vec((0usize..8, nudge()), 1..4),
        any::<bool>(),
        prop::collection::vec(stroke(), 1..4),
        any::<bool>(),
        any::<bool>(),
        extra(),
    )
        .prop_map(
            |(size, face, nudges, autofold, strokes, strokes_first, out, extra)| Nudged {
                size,
                face,
                nudges,
                autofold,
                strokes,
                strokes_first,
                out,
                extra,
            },
        )
}

/// The persistent id and plane of the box's face nearest `want` (the face the strokes go on;
/// every part of it shares that plane, so any of them takes the stroke).
fn face_towards(d: &Document, object: u64, want: DVec3) -> (u64, sawhorse_kernel::plane::Plane) {
    let b = d.model().object(object).unwrap().brep();
    let ids = b.persistent_face_ids();
    let (k, f) = b
        .faces()
        .enumerate()
        .map(|(k, (_, f))| (k, f))
        .max_by(|x, y| {
            x.1.plane
                .normal
                .dot(want)
                .total_cmp(&y.1.plane.normal.dot(want))
                .then(y.0.cmp(&x.0))
        })
        .unwrap();
    (ids[k], f.plane)
}

/// The box, the plane on its face (origin and normal), as [`Nudged`] says.
fn nudged_box(n: &Nudged) -> (Document, u64, [f64; 3], [f64; 3]) {
    let mut d = Document::new();
    let object = add_box(&mut d, [0.0; 3], n.size);
    let (axis, high) = face_axis(n.face);
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let level = if high { n.size[axis] } else { 0.0 };
    let mut want = DVec3::ZERO;
    want[axis] = if high { 1.0 } else { -1.0 };
    let strokes: Vec<&Stroke> = n.strokes.iter().chain(&n.extra.stroke).collect();
    let nudges: Vec<(usize, [f64; 3])> = n.nudges.iter().copied().chain(n.extra.nudge).collect();
    let draw = |d: &mut Document| {
        for s in &strokes {
            let (id, plane) = face_towards(d, object, want);
            // The face's corner at (0, 0) and its two sides, in the world.
            let mut o = DVec3::ZERO;
            o[axis] = level;
            let (mut eu, mut ev) = (DVec3::ZERO, DVec3::ZERO);
            eu[u] = n.size[u];
            ev[v] = n.size[v];
            let at = |a: f64, b: f64| {
                // Onto the face's own (stored) plane, as a person drawing on it would.
                let p = o + eu * a + ev * b;
                plane.to_world(plane.to_local(p)).to_array()
            };
            let points: Vec<[f64; 3]> = match *s {
                Stroke::Rect { centre, half } => vec![
                    at(centre[0] - half[0], centre[1] - half[1]),
                    at(centre[0] + half[0], centre[1] - half[1]),
                    at(centre[0] + half[0], centre[1] + half[1]),
                    at(centre[0] - half[0], centre[1] + half[1]),
                ],
                Stroke::Circle {
                    centre,
                    radius,
                    sides,
                } => {
                    let r = radius * n.size[u].min(n.size[v]);
                    (0..*sides)
                        .map(|k| {
                            let t = std::f64::consts::TAU * k as f64 / *sides as f64;
                            at(
                                centre[0] + r * t.cos() / n.size[u],
                                centre[1] + r * t.sin() / n.size[v],
                            )
                        })
                        .collect()
                }
            };
            // A stroke the kernel refuses (too small, off the face) leaves the box as it was.
            let _ = d.apply(&Op::Draw {
                target: sawhorse_kernel::model::DrawTarget::Face { object, face: id },
                points,
                closed: true,
            });
        }
    };
    let corner = |k: usize| {
        [
            if k & 1 == 0 { 0.0 } else { n.size[0] },
            if k & 2 == 0 { 0.0 } else { n.size[1] },
            if k & 4 == 0 { 0.0 } else { n.size[2] },
        ]
    };
    if n.strokes_first {
        draw(&mut d);
    }
    let at_corner = |k: usize| {
        // A corner of the plane's face, mostly (a corner off it by its index otherwise).
        let mut p = corner(k);
        if k < 4 {
            p[axis] = level;
        }
        p
    };
    for &(k, shift) in &nudges {
        let p = at_corner(k);
        let _ = d.apply(&Op::TransformGeometry {
            object,
            points: vec![p],
            xform: Xform::translation(DVec3::from_array(shift)),
            autofold: n.autofold,
        });
    }
    if !n.strokes_first {
        draw(&mut d);
    }
    let mut origin = [0.0; 3];
    origin[axis] = level;
    let mut normal = if n.out { want } else { -want };
    if let PlaneAt::Corner { nudge, normal: own } = &n.extra.plane {
        // The vertex nearest where the corner was: it moved by micrometres, everything else
        // is centimetres away (a stroke's corners, rounded to the face, stay off the box's).
        let was = DVec3::from_array(at_corner(nudges[nudge % nudges.len()].0));
        origin = vertices(d.model(), object)
            .into_iter()
            .min_by(|a, b| a.distance(was).total_cmp(&b.distance(was)))
            .unwrap()
            .to_array();
        if let Some(m) = own {
            normal = DVec3::from_array(unit(DVec3::from_array(*m), normal));
        }
    }
    (d, object, origin, normal.to_array())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        ..ProptestConfig::default()
    })]

    /// Lane 19: a plane on a face nudged by micrometres and drawn on, cut with plane
    /// snapping on and off: the halves make the whole, each is closed, every cap ring is
    /// simple, the caps are the faces clipped to the plane, and the slice closes. Lane 19b:
    /// up to four corners nudged and four strokes, and half the planes through a poked corner
    /// (along the face, or turned) instead of on the face.
    #[test]
    fn planes_on_a_nudged_face_drawn_on(n in nudged(), snap_on in any::<bool>()) {
        snapping(snap_on, || -> Result<(), TestCaseError> {
            let (mut d, _, origin, normal) = nudged_box(&n);
            let section = add_section(&mut d, origin, normal);
            cut_cleanly(&mut d, section)
        })?;
    }
}

/// The lane-18 verifier's find (snapping on, seed 27182803, about 1 in 130,000 sequences;
/// `tests/repro/2026-10-13-section-tie-rule-on-an-imprinted-tilted-face.json`): a plane on a
/// box's bottom face, the face tilted by moving one corner a micrometre, then a rectangle
/// drawn across its edge. The drawn part (bent 1.1e-7 off the face's plane) was on the plane
/// by itself while the rest (bent 2.5e-7, one corner 1e-6 off) was not; its corners moved to
/// its own mean, the side face no longer met the flipped plane along its bottom edge, and the
/// cut-away half kept a 0.0313 m2 triangle of the bottom face with no cap (vector area
/// 0.0312505 m2, a cap 0 m around). Lane 19 made both parts one surface; since lane 19c every
/// corner is simply where it is (three of the bottom face's corners lie on the plane, the
/// moved one does not) and the face is clipped along its own triangles by the cut and the
/// halves alike: what the half below keeps of it is the triangle on the plane, a sheet of no
/// volume closed by a cap of the same area.
#[test]
fn the_lane_18_verification_find() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repro");
    let r: Repro = serde_json::from_str(
        &std::fs::read_to_string(
            dir.join("2026-10-13-section-tie-rule-on-an-imprinted-tilted-face.json"),
        )
        .unwrap(),
    )
    .unwrap();
    snapping(r.plane_snap.unwrap_or(true), || {
        let mut d = Document::new();
        for op in &r.ops {
            let _ = d.apply(op);
        }
        let s = d.model().sections()[0].0;
        // As found: the plane at z = -1.75 facing up, lying on the bottom face. It keeps
        // everything and cuts nothing.
        let cut = d.model().section_cut(s).unwrap();
        assert!(cut.caps.is_empty(), "{cut:?}");
        let kept = d.model().kept_part(s).unwrap();
        assert!(kept.vector_area.length() < 1e-12, "{kept:?}");
        let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
        assert!((kept.volume - whole).abs() < 1e-8, "{kept:?} of {whole}");
        cut_cleanly(&mut d.clone(), s).unwrap();
        // Flipped: the bottom face touches the plane at three corners and along two edges,
        // and the half below keeps the part of it on the plane, a sheet of no volume, closed
        // by a cap of the same area (it had none).
        d.apply(&Op::FlipSection { section: s }).unwrap();
        let cut = d.model().section_cut(s).unwrap();
        assert_eq!(cut.caps.len(), 1, "{cut:?}");
        assert!(cut.area > 0.01 && cut.area <= 0.0312505, "{}", cut.area);
        let below = d.model().kept_part(s).unwrap();
        assert!(below.vector_area.length() < 1e-12, "{below:?}");
        assert!(below.volume.abs() < 1e-8, "{below:?}");
        assert!((below.area - 2.0 * cut.area).abs() < 1e-9, "{below:?}");
        cut_cleanly(&mut d, s).unwrap();
    });
}

/// Play a repro of `tests/repro` with its plane snapping.
fn replay(name: &str) -> Document {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repro");
    let r: Repro = serde_json::from_str(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap();
    snapping(r.plane_snap.unwrap_or(true), || {
        let mut d = Document::new();
        for op in &r.ops {
            let _ = d.apply(op);
        }
        d
    })
}

/// Both halves of `section` (as it is and flipped): the cap areas, each half's vector area
/// and volume, and the whole they make, checked with every property along the way.
fn halves(d: &Document, section: u64) -> [(f64, DVec3, f64); 2] {
    let mut d = d.clone();
    let mut out = [(0.0, DVec3::ZERO, 0.0); 2];
    for half in &mut out {
        let cut = d.model().section_cut(section).unwrap();
        let kept = d.model().kept_part(section).unwrap();
        *half = (cut.area, kept.vector_area, kept.volume);
        cut_cleanly(&mut d.clone(), section).unwrap();
        d.apply(&Op::FlipSection { section }).unwrap();
    }
    out
}

/// The lane-19 verifier's find (`planes_on_a_nudged_face_drawn_on`, `PROPTEST_CASES=100000`,
/// run 1, about one case in 700,000, plane snapping off; failing on the lane-18 and lane-19
/// kernels; `tests/repro/2026-10-14-section-poked-corner-facets.json`): a 0.125 x 0.125 x
/// 0.5 m box with a 13-gon and a rectangle drawn on its +x face, then corner 1 poked 2.5
/// micrometres out of the face and corner 5 dented 1.4 micrometres in, with autofold, and a
/// plane on the face facing into the box. Autofold left the face as nine facets with
/// different stored planes. The big one alone was on the plane (bend 9.8e-7, so its dented
/// corner, within twice that, was put on it) and five exact triangles round the poked corner
/// were not; one had the dented corner, and it was cut along the line its stored plane gives,
/// nearly at right angles to the one between its two corners on the plane: its piece ran
/// backwards and the kept half lost the five facets (0.0235 m2) with no cap. Lane 19b cut
/// every face by its clipped rings' chords and made the nine facets one surface again. Since
/// lane 19c no facet decides anything: every corner is where it is (the poked one 2.5
/// micrometres off the plane, the dented one 1.4), every facet is clipped along its own
/// triangles, and the cap, laid from exactly the chords they leave open, is the part of the
/// face beyond the plane; `section::tests::the_kept_triangles_leave_open_exactly_the_chords`
/// and `the_cap_closes_the_half_under_any_distances` replay this solid under the tie rule's
/// distances, with every corner near the plane forced onto it, and under random distances.
#[test]
fn the_lane_19_verification_find() {
    let d = replay("2026-10-14-section-poked-corner-facets.json");
    let s = d.model().sections()[0].0;
    assert_eq!(d.model().section(s).unwrap().normal, DVec3::NEG_X);
    let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
    let [(cap, open, kept), (cap_beyond, open_beyond, beyond)] = halves(&d, s);
    // As found, facing into the box: the cap is where the bent face lies beyond the plane
    // (0.0170 m2 of the 0.0625 m2 face). Flipped, the half beyond is the sliver the poked
    // corner holds plus the parts of the face that lie exactly on the plane, sheets closed by
    // the cap (zero counts as kept on both sides).
    assert!((cap - 0.016_991_98).abs() < 1e-8, "{cap}");
    assert!((cap_beyond - 0.048_889_26).abs() < 1e-8, "{cap_beyond}");
    assert!(
        open.length() < 1e-15 && open_beyond.length() < 1e-15,
        "{open} {open_beyond}"
    );
    assert!(beyond > 0.0 && beyond < 2e-8, "{beyond}");
    // The halves integrate each face over its stored plane, the whole is the volume of the
    // tessellation through the corners: they differ by up to the faces' bend (1e-6) times
    // their area (0.0625 m2 on this face). check_sections holds them exactly to the
    // plane-integrated whole.
    assert!(
        (kept + beyond - whole).abs() < 1e-7,
        "{kept} + {beyond} of {whole}"
    );
}

/// The lane-19b property tests' two finds (`planes_on_a_nudged_face_drawn_on` as widened,
/// `PROPTEST_RNG_SEED` 2026101411 and 2026101412, plane snapping off; both seeds in
/// `section_props.proptest-regressions`): a box's -x face drawn on with two rectangles, then
/// one corner of it moved 0.06 or 0.22 micrometres without autofold, and a plane through that
/// corner along the face, facing into the box. The move re-planed the part of the face holding
/// the corner (now bent by it, 6e-8 and 1.2e-7 m) and left the parts the strokes split off on
/// the face's old plane, so they were separate surfaces: the bent part was on the plane by its
/// bend and the parts beside it, 0.06 or 0.22 micrometres off it, were not, and the corners
/// they share were on the plane for one and off it for the other. The first left the kept
/// half open by the bent part's whole area (0.0053 m2, the cap double-counting it); the second
/// folded a side face's triangles round a shared corner, a seam the cap cannot close, 7.8e-8
/// m2 open. Both were found on the lane-19b interim build; lane 19b made the parts one surface,
/// on the plane by the bent part's bend, so the half the box is in cut nothing.
///
/// Since lane 19c every corner is where it is: the plane runs through the moved corner, 0.06
/// or 0.22 micrometres inside the face, and every other corner of the face is that far outside
/// it, so the plane cuts a sliver that thin off the whole face. Both halves have the same cap,
/// the whole face less what the moved corner takes off it (0.078125 m2 to 1e-7), and both close
/// to 1e-15; the half outside is the sliver (5e-9 and 1.5e-8 m3).
#[test]
fn the_lane_19b_property_finds() {
    for name in [
        "2026-10-14-section-plane-through-a-poked-corner-of-a-drawn-on-face.json",
        "2026-10-14-section-plane-through-a-poked-corner-beside-a-drawn-part.json",
    ] {
        let d = replay(name);
        let s = d.model().sections()[0].0;
        assert_eq!(d.model().section(s).unwrap().normal, DVec3::X, "{name}");
        let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
        let [(cap, open, kept), (cap_beyond, open_beyond, beyond)] = halves(&d, s);
        assert!((cap - 0.078_125).abs() < 1e-7, "{name}: {cap}");
        assert!(
            (cap - cap_beyond).abs() < 1e-15,
            "{name}: {cap} {cap_beyond}"
        );
        assert!(open.length() < 1e-15, "{name}: {open}");
        assert!(open_beyond.length() < 1e-15, "{name}: {open_beyond}");
        assert!(beyond > 1e-9 && beyond < 2e-8, "{name}: {beyond}");
        assert!(
            (kept + beyond - whole).abs() < 1e-7,
            "{name}: {kept} + {beyond} of {whole}"
        );
    }
}

/// The lane-19b verifier's find (`planes_on_a_nudged_face_drawn_on` as widened, shrunk to
/// eight ops; failing with plane snapping on and off, on the lane-19 and lane-19b kernels;
/// `tests/repro/2026-10-14-section-top-face-poked-beyond-its-bend-snap-off.json` and
/// `-snap-on.json`): a 0.25 x 0.125 m box, its top corners at (0.25, 0.125) and (0, 0.125)
/// moved 0.78 micrometres down and 1.8 micrometres up without autofold, two n-gons larger
/// than the top face drawn on it, and a plane at the top as it was, facing down into the box.
/// The poked corner, 1.8 micrometres off the plane, is beyond twice the face's 0.65
/// micrometre bend, so the top was not on the plane as a surface, and was cut by rules that
/// read its parts' stored planes as well: the cut-away half kept 0.0251 m2 of the top with no
/// cap (a cap 0 m around).
/// Lane 19c: every corner is where it is, the top is clipped along its own triangles, and the
/// part of it above the plane is the cap of both halves, 0.0251 m2: the box below is closed
/// by it, and so is the sliver above (1.2e-8 m3), each to 1e-15.
#[test]
fn the_lane_19b_verification_find() {
    for name in [
        "2026-10-14-section-top-face-poked-beyond-its-bend-snap-off.json",
        "2026-10-14-section-top-face-poked-beyond-its-bend-snap-on.json",
    ] {
        let d = replay(name);
        let s = d.model().sections()[0].0;
        assert_eq!(d.model().section(s).unwrap().normal, DVec3::NEG_Z, "{name}");
        let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
        let [(cap, open, kept), (cap_beyond, open_beyond, beyond)] = halves(&d, s);
        for (side, c) in [("below", cap), ("above", cap_beyond)] {
            assert!(
                (c - 0.025_144_943_683_595).abs() < 1e-12,
                "{name}, the half {side}: {c}"
            );
        }
        assert!(open.length() < 1e-15, "{name}: {open}");
        assert!(open_beyond.length() < 1e-15, "{name}: {open_beyond}");
        assert!(beyond > 1e-8 && beyond < 1.5e-8, "{name}: {beyond}");
        assert!(
            (kept + beyond - whole).abs() < 1e-7,
            "{name}: {kept} + {beyond} of {whole}"
        );
        // What the cut-away half had no cap for: its cap now runs round the whole region.
        let mut flipped = d.clone();
        flipped.apply(&Op::FlipSection { section: s }).unwrap();
        let p = flipped.model().kept_part(s).unwrap();
        assert!(p.cap_length > 0.7, "{name}: {p:?}");
    }
}

/// Lane 19c, resumed: the resumed run's find (`PROPTEST_RNG_SEED` 2026102001, after 48,791
/// cases; `2026-10-14-section-side-face-sliver-folded-over-the-plane`): a plane on a box's
/// end face (x = 0), a rectangle drawn across its edge first, one corner pushed 1 micrometre
/// into the box and another moved 74 nm and 0.88 micrometres along the face (0.34 nm off
/// it), no autofold. The side face's tessellation had a triangle 0.34 nm wide in its frame,
/// standing across the face and lying folded flat over the end face; the flipped plane's
/// chords wound -1 round it and the cut-away half was open by 3.1e-9 m2. With the face's
/// triangles laid down, the plane facing into the box cuts nothing, and the flipped plane
/// keeps the parts of the end face that lie on the plane (the drawn rectangle and the part
/// above it, 0.0197522461 m2), sheets of no volume closed by a cap of the same area.
#[test]
fn the_lane_19c_resumed_find() {
    let d = replay("2026-10-14-section-side-face-sliver-folded-over-the-plane.json");
    let section = d.model().sections()[0].0;
    let [(cap_in, open_in, kept_in), (cap_out, open_out, kept_out)] = halves(&d, section);
    let whole: f64 = d.model().objects().iter().map(|(_, o)| o.volume()).sum();
    assert!(cap_in == 0.0, "{cap_in}");
    assert!(open_in.length() < 1e-15, "{open_in}");
    // The kept half is the whole box: its volume measured over the faces' stored planes,
    // the whole's over its triangles, 2e-9 m3 apart (the faces bend by up to 1 micrometre).
    assert!((kept_in - whole).abs() < 1e-8, "{kept_in} {whole}");
    assert!((cap_out - 0.019_752_246).abs() < 1e-9, "{cap_out}");
    assert!(open_out.length() < 1e-15, "{open_out}");
    // No volume, measured over the bent end face's stored plane: 1e-9 m3.
    assert!(kept_out.abs() < 1e-8, "{kept_out}");
}
