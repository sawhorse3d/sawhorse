//! Inference on a flat drawing (lane 12): what a cursor snaps to on a sheet viewport's
//! hidden-line drawing, the way the 3D engine snaps in the model, with the same ranks:
//! endpoints beat midpoints beat intersections beat a point on an edge, each within a
//! radius (paper metres), the nearest of a rank winning. Every drawing line keeps the model
//! points it came from, so a snap knows its model point (an intersection takes the point
//! on the first line), which is what a dimension attaches to.

use glam::{DVec2, DVec3};

/// A drawing line: paper points and the model points they are, and the caller's own tag
/// for it (which line of its drawing it is), handed back with a snap on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatSeg {
    pub a: DVec2,
    pub b: DVec2,
    pub a3: DVec3,
    pub b3: DVec3,
    pub tag: usize,
}

/// What a flat snap found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlatKind {
    Endpoint,
    Midpoint,
    Intersection,
    OnEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct FlatSnap {
    pub kind: FlatKind,
    /// On the paper.
    pub at: [f64; 2],
    /// In the model.
    pub point: [f64; 3],
    /// The tag of the line the point is on (an intersection: the first line).
    #[serde(skip)]
    pub tag: usize,
}

fn closest_on(s: &FlatSeg, p: DVec2) -> (f64, DVec2) {
    let d = s.b - s.a;
    let l2 = d.length_squared();
    let t = if l2 > 0.0 {
        ((p - s.a).dot(d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (t, s.a + d * t)
}

/// Where segments `s` and `u` cross (parameters along each), if they do.
fn cross(s: &FlatSeg, u: &FlatSeg) -> Option<(f64, f64)> {
    let (r, q) = (s.b - s.a, u.b - u.a);
    let den = r.perp_dot(q);
    if den.abs() <= 1e-18 * r.length() * q.length() {
        return None;
    }
    let w = u.a - s.a;
    let t = w.perp_dot(q) / den;
    let v = w.perp_dot(r) / den;
    ((-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&v)).then_some((t, v))
}

/// The snap for a cursor at `p` within `radius` (see the module docs).
pub fn snap(segs: &[FlatSeg], p: DVec2, radius: f64) -> Option<FlatSnap> {
    let near: Vec<&FlatSeg> = segs
        .iter()
        .filter(|s| closest_on(s, p).1.distance(p) <= radius)
        .collect();
    let mut best: Option<(FlatKind, f64, DVec2, DVec3, usize)> = None;
    let mut offer = |kind: FlatKind, at: DVec2, point: DVec3, tag: usize| {
        let d = at.distance(p);
        if d > radius {
            return;
        }
        if best.is_none_or(|(k, bd, ..)| kind < k || (kind == k && d < bd)) {
            best = Some((kind, d, at, point, tag));
        }
    };
    for s in &near {
        offer(FlatKind::Endpoint, s.a, s.a3, s.tag);
        offer(FlatKind::Endpoint, s.b, s.b3, s.tag);
        offer(
            FlatKind::Midpoint,
            (s.a + s.b) * 0.5,
            s.a3.lerp(s.b3, 0.5),
            s.tag,
        );
    }
    for (i, s) in near.iter().enumerate() {
        for u in &near[i + 1..] {
            if let Some((t, _)) = cross(s, u) {
                offer(
                    FlatKind::Intersection,
                    s.a.lerp(s.b, t),
                    s.a3.lerp(s.b3, t),
                    s.tag,
                );
            }
        }
    }
    for s in &near {
        let (t, at) = closest_on(s, p);
        offer(FlatKind::OnEdge, at, s.a3.lerp(s.b3, t), s.tag);
    }
    best.map(|(kind, _, at, point, tag)| FlatSnap {
        kind,
        at: at.to_array(),
        point: point.to_array(),
        tag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(a: [f64; 2], b: [f64; 2]) -> FlatSeg {
        FlatSeg {
            a: DVec2::from_array(a),
            b: DVec2::from_array(b),
            a3: DVec3::new(a[0], 0.0, a[1]),
            b3: DVec3::new(b[0], 0.0, b[1]),
            tag: 0,
        }
    }

    #[test]
    fn ranks_and_radius() {
        let segs = [seg([0.0, 0.0], [1.0, 0.0]), seg([0.5, -0.5], [0.5, 0.5])];
        // Near a corner: the endpoint wins.
        let s = snap(&segs, DVec2::new(0.01, 0.01), 0.05).unwrap();
        assert_eq!((s.kind, s.at), (FlatKind::Endpoint, [0.0, 0.0]));
        // Near the crossing (which is also the first line's midpoint): the midpoint ranks higher.
        let s = snap(&segs, DVec2::new(0.51, 0.01), 0.05).unwrap();
        assert_eq!(s.kind, FlatKind::Midpoint);
        // A crossing that is no midpoint.
        let segs = [seg([0.0, 0.0], [1.0, 0.0]), seg([0.3, -0.5], [0.3, 0.7])];
        let s = snap(&segs, DVec2::new(0.31, 0.01), 0.05).unwrap();
        assert_eq!(s.kind, FlatKind::Intersection);
        assert!((s.point[0] - 0.3).abs() < 1e-12);
        // Along a line.
        let s = snap(&segs, DVec2::new(0.8, 0.02), 0.05).unwrap();
        assert_eq!(s.kind, FlatKind::OnEdge);
        assert_eq!(s.at, [0.8, 0.0]);
        assert!(snap(&segs, DVec2::new(5.0, 5.0), 0.05).is_none());
    }
}
