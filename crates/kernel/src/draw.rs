//! Drawing a stroke on a solid's face (lane 3): the Object-tier half of the draw tools.
//!
//! A stroke is a polyline (open or closed) on the plane of one of the object's faces. Every
//! face of the object on that plane (same plane, same facing) takes part, so a stroke may
//! run across several faces that earlier strokes made:
//!
//! - a closed stroke strictly inside one face becomes an **imprint** (a sub-face twinned
//!   with a hole);
//! - otherwise the stroke is cut where it crosses the faces' edges, and every piece that
//!   runs through the inside of a face from one edge to another becomes a **split_face**
//!   cut (lane-2 open question 1, decided: yes). Pieces outside the faces (in the air) and
//!   pieces lying along an existing edge are dropped;
//! - a piece that ends inside a face (a dangling line) is refused with
//!   [`FaceEditError::Dangling`]: a solid's face cannot carry a loose line. The draw tools
//!   keep such a stroke as a preview until it reaches an edge or closes.
//!
//! All or nothing: the caller rebuilds and validates; any refusal leaves the solid as it was.

use glam::{DVec2, DVec3};

use crate::brep::Polys;
use crate::error::FaceEditError;
use crate::faceops::{imprint, split_face};
use crate::plane::Plane;
use crate::sketch::{orient, point_segment_distance};
use crate::tolerance::{PLANARITY, POINT_MERGE, in_bounds};
use crate::validate::point_in_ring;

/// Faces of `p` on `plane` (same plane, same facing).
fn faces_on(p: &Polys, plane: &Plane) -> Vec<usize> {
    (0..p.faces.len())
        .filter(|&g| {
            p.plane(g).is_some_and(|pg| {
                pg.normal.dot(plane.normal) >= 1.0 - 1e-9
                    && p.faces[g]
                        .rings()
                        .flatten()
                        .all(|&i| plane.signed_distance(p.points[i]).abs() <= PLANARITY)
            })
        })
        .collect()
}

struct View {
    /// (face index, rings in the plane's 2D frame) for every face on the plane.
    faces: Vec<(usize, Vec<Vec<DVec2>>)>,
}

impl View {
    fn new(p: &Polys, plane: &Plane) -> View {
        View {
            faces: faces_on(p, plane)
                .into_iter()
                .map(|g| {
                    let rings = p.faces[g]
                        .rings()
                        .map(|r| r.iter().map(|&i| plane.to_local(p.points[i])).collect())
                        .collect();
                    (g, rings)
                })
                .collect(),
        }
    }

    fn segments(&self) -> impl Iterator<Item = (DVec2, DVec2)> + '_ {
        self.faces.iter().flat_map(|(_, rings)| {
            rings
                .iter()
                .flat_map(|r| (0..r.len()).map(move |k| (r[k], r[(k + 1) % r.len()])))
        })
    }

    fn on_boundary(&self, q: DVec2) -> bool {
        self.segments()
            .any(|(a, b)| point_segment_distance(q, a, b) <= POINT_MERGE)
    }

    /// The face whose interior holds `q` (not on any boundary).
    fn face_at(&self, q: DVec2) -> Option<usize> {
        if self.on_boundary(q) {
            return None;
        }
        self.faces.iter().find_map(|(g, rings)| {
            (point_in_ring(q, &rings[0]) && !rings[1..].iter().any(|h| point_in_ring(q, h)))
                .then_some(*g)
        })
    }
}

/// The face of `p` on `plane` whose interior holds `q` (plane frame), if any.
pub fn face_containing(p: &Polys, plane: &Plane, q: DVec2) -> Option<usize> {
    View::new(p, plane).face_at(q)
}

/// Parameters in (0, 1) where segment `a`-`b` meets segment `c`-`d` (a proper crossing, a
/// touch at an end, or the ends of a collinear overlap).
fn hits(a: DVec2, b: DVec2, c: DVec2, d: DVec2, out: &mut Vec<f64>) {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= 0.0 {
        return;
    }
    let param = |q: DVec2| (q - a).dot(ab) / len2;
    // Boundary vertices lying on the stroke (touching, or collinear overlap ends).
    for q in [c, d] {
        if point_segment_distance(q, a, b) <= POINT_MERGE {
            out.push(param(q));
        }
    }
    let (o1, o2) = (orient(a, b, c), orient(a, b, d));
    let (o3, o4) = (orient(c, d, a), orient(c, d, b));
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        let cd = d - c;
        let den = ab.perp_dot(cd);
        if den != 0.0 {
            out.push((c - a).perp_dot(cd) / den);
        }
    }
}

/// Stroke `pts` (world, on the plane of face `fi`), drawn on every face of `p` on that
/// plane. Returns the indices of the faces the stroke cut or imprinted.
pub fn draw_on_faces(
    p: &mut Polys,
    fi: usize,
    pts: &[DVec3],
    closed: bool,
) -> Result<Vec<usize>, FaceEditError> {
    let plane = p.plane(fi).ok_or(FaceEditError::NoSuchFace(fi as u64))?;
    if pts.len() < 2 || (closed && pts.len() < 3) {
        return Err(FaceEditError::TooFewPoints(if closed { 3 } else { 2 }));
    }
    let mut q2: Vec<DVec2> = Vec::with_capacity(pts.len());
    for p3 in pts {
        if !(in_bounds(p3.x) && in_bounds(p3.y) && in_bounds(p3.z)) {
            return Err(FaceEditError::OutOfBounds);
        }
        if plane.signed_distance(*p3).abs() > PLANARITY {
            return Err(FaceEditError::NotOnPlane);
        }
        q2.push(plane.to_local(*p3));
    }
    if closed && q2.len() > 3 && q2[0].distance(q2[q2.len() - 1]) <= POINT_MERGE {
        q2.pop();
    }
    let n = q2.len();
    let nseg = if closed { n } else { n - 1 };
    if (0..nseg).any(|k| q2[k].distance(q2[(k + 1) % n]) <= POINT_MERGE) {
        return Err(FaceEditError::Degenerate);
    }
    let view = View::new(p, &plane);
    // A closed stroke inside one face: an imprint (the face does the detailed checks).
    if closed
        && let Some(g) = view.face_at(q2[0])
        && q2.iter().all(|&q| view.face_at(q) == Some(g))
        && !(0..n).any(|k| {
            let (a, b) = (q2[k], q2[(k + 1) % n]);
            let mut t = Vec::new();
            for (c, d) in view.segments() {
                hits(a, b, c, d, &mut t);
            }
            !t.is_empty()
        })
    {
        let world: Vec<DVec3> = q2.iter().map(|&q| plane.to_world(q)).collect();
        let sub = imprint(p, g, &world)?;
        return Ok(vec![g, sub]);
    }
    // Cut the stroke where it meets any edge of the faces on the plane.
    let mut pieces: Vec<(DVec2, DVec2)> = Vec::new();
    for k in 0..nseg {
        let (a, b) = (q2[k], q2[(k + 1) % n]);
        let mut ts = vec![0.0, 1.0];
        for (c, d) in view.segments() {
            hits(a, b, c, d, &mut ts);
        }
        ts.retain(|t| (0.0..=1.0).contains(t));
        ts.sort_by(f64::total_cmp);
        let len = a.distance(b);
        let mut kept: Vec<f64> = Vec::new();
        for t in ts {
            if kept.last().is_none_or(|&l| (t - l) * len > POINT_MERGE) {
                kept.push(t);
            }
        }
        if let Some(l) = kept.last_mut() {
            *l = 1.0;
        }
        for w in kept.windows(2) {
            pieces.push((a + (b - a) * w[0], a + (b - a) * w[1]));
        }
    }
    // Which face each piece runs through (None: outside, or along an edge).
    let class: Vec<Option<usize>> = pieces
        .iter()
        .map(|&(a, b)| view.face_at(0.5 * (a + b)))
        .collect();
    // Runs of consecutive pieces through the same face. A closed stroke is rotated to
    // start just after a break, so a run does not wrap around its own start.
    let m = pieces.len();
    let start = if closed {
        match (0..m).find(|&k| class[k] != class[(k + m - 1) % m] || class[k].is_none()) {
            Some(s) => s,
            None => return Err(FaceEditError::CrossesBoundary),
        }
    } else {
        0
    };
    let mut runs: Vec<(usize, Vec<DVec2>)> = Vec::new();
    let mut cur: Option<(usize, Vec<DVec2>)> = None;
    for step in 0..m {
        let k = (start + step) % m;
        match (class[k], cur.as_mut()) {
            (Some(g), Some((h, pts))) if *h == g => pts.push(pieces[k].1),
            (c, _) => {
                if let Some(r) = cur.take() {
                    runs.push(r);
                }
                if let Some(g) = c {
                    cur = Some((g, vec![pieces[k].0, pieces[k].1]));
                }
            }
        }
    }
    if let Some(r) = cur.take() {
        runs.push(r);
    }
    if runs.is_empty() {
        return Err(FaceEditError::NotInside);
    }
    for (_, run) in &runs {
        let (a, b) = (run[0], run[run.len() - 1]);
        if !view.on_boundary(a) || !view.on_boundary(b) {
            return Err(FaceEditError::Dangling);
        }
    }
    // Cut, run by run; each run's face is looked up again after the previous cuts.
    let mut q = p.clone();
    let mut touched: Vec<usize> = Vec::new();
    for (_, run) in &runs {
        let probe = 0.5 * (run[0] + run[1]);
        let now = View::new(&q, &plane);
        let Some(g) = now.face_at(probe) else {
            return Err(FaceEditError::NotInside);
        };
        // Drop interior points that lie on the run's own straight continuation within the
        // merge distance (the cut needs distinct points), keep the shape otherwise.
        let world: Vec<DVec3> = run.iter().map(|&v| plane.to_world(v)).collect();
        let (a, b) = split_face(&mut q, g, &world)?;
        touched.retain(|&t| t != a && t != b);
        touched.push(a);
        touched.push(b);
    }
    *p = q;
    Ok(touched)
}
