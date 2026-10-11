//! 2D polygon booleans over `i_overlay`, used by coplanar merge and imprint tests.
//!
//! Shapes are `Vec<contour>` where the first contour is the outer boundary and the
//! rest are holes; a result is a list of shapes.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

pub type Contour = Vec<[f64; 2]>;
pub type Shape = Vec<Contour>;

fn run(subject: &[Contour], clip: &[Contour], rule: OverlayRule) -> Vec<Shape> {
    let subj: Vec<Contour> = subject.to_vec();
    let clip: Vec<Contour> = clip.to_vec();
    subj.overlay(&clip, rule, FillRule::EvenOdd)
}

/// Union of two sets of contours (even-odd fill).
pub fn union(subject: &[Contour], clip: &[Contour]) -> Vec<Shape> {
    run(subject, clip, OverlayRule::Union)
}

/// Subject minus clip (even-odd fill).
pub fn difference(subject: &[Contour], clip: &[Contour]) -> Vec<Shape> {
    run(subject, clip, OverlayRule::Difference)
}

/// Intersection (even-odd fill).
pub fn intersection(subject: &[Contour], clip: &[Contour]) -> Vec<Shape> {
    run(subject, clip, OverlayRule::Intersect)
}

/// Total area of a boolean result (outer areas minus hole areas).
pub fn area(shapes: &[Shape]) -> f64 {
    shapes
        .iter()
        .map(|s| {
            s.iter()
                .enumerate()
                .map(|(i, c)| {
                    let ring: Vec<glam::DVec2> =
                        c.iter().map(|p| glam::DVec2::new(p[0], p[1])).collect();
                    let a = crate::signed_area(&ring).abs();
                    if i == 0 { a } else { -a }
                })
                .sum::<f64>()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(x: f64, y: f64, s: f64) -> Contour {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    #[test]
    fn union_of_overlapping_squares() {
        let u = union(&[sq(0.0, 0.0, 2.0)], &[sq(1.0, 1.0, 2.0)]);
        assert_eq!(u.len(), 1);
        assert!((area(&u) - 7.0).abs() < 1e-9);
    }

    #[test]
    fn difference_makes_a_hole() {
        let d = difference(&[sq(0.0, 0.0, 4.0)], &[sq(1.0, 1.0, 2.0)]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].len(), 2, "outer + one hole");
        assert!((area(&d) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn disjoint_intersection_is_empty() {
        assert!(intersection(&[sq(0.0, 0.0, 1.0)], &[sq(5.0, 5.0, 1.0)]).is_empty());
    }
}
