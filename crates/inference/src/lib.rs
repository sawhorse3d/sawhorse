//! Inference (snapping) for Sawhorse: a global, read-only query over the model (ADR-0001).
//!
//! This lane ships the vocabulary and a brute-force world-space snapper; the inference
//! lane replaces the brute force with a BVH over faces-with-holes, edges and vertices plus
//! `rstar` for snap candidates, and converts a pixel tolerance into a world tolerance.
//!
//! Colours are OURS (ADR-0003), never SketchUp's: endpoint lime, midpoint teal,
//! on-edge amber, on-face violet. The UI reads them from CSS tokens named here.

use glam::DVec3;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceKind {
    Endpoint,
    Midpoint,
    OnEdge,
    OnFace,
}

impl InferenceKind {
    /// Priority when several candidates are in range: lower wins.
    pub fn priority(self) -> u8 {
        match self {
            Self::Endpoint => 0,
            Self::Midpoint => 1,
            Self::OnEdge => 2,
            Self::OnFace => 3,
        }
    }

    /// CSS custom property holding this inference's colour in the UI.
    pub fn css_token(self) -> &'static str {
        match self {
            Self::Endpoint => "--inf-end",
            Self::Midpoint => "--inf-mid",
            Self::OnEdge => "--inf-edge",
            Self::OnFace => "--inf-face",
        }
    }

    /// Light-theme colour value, for exporters and tests.
    pub fn colour(self) -> &'static str {
        match self {
            Self::Endpoint => "#7bc043",
            Self::Midpoint => "#17a2a0",
            Self::OnEdge => "#e08a1e",
            Self::OnFace => "#8a5cd6",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Endpoint => "Endpoint",
            Self::Midpoint => "Midpoint",
            Self::OnEdge => "On Edge",
            Self::OnFace => "On Face",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub kind: InferenceKind,
    pub point: DVec3,
    /// Distance from the query point to `point`.
    pub distance: f64,
}

/// Snap `query` to the nearest endpoint, midpoint or edge point within `tolerance`
/// (world units). Points beat midpoints beat edges, regardless of distance.
pub fn snap_to_edges(query: DVec3, edges: &[(DVec3, DVec3)], tolerance: f64) -> Option<Snap> {
    let mut best: Option<Snap> = None;
    let mut consider = |kind: InferenceKind, point: DVec3| {
        let distance = query.distance(point);
        if distance > tolerance {
            return;
        }
        let better = match best {
            None => true,
            Some(b) => (kind.priority(), distance) < (b.kind.priority(), b.distance),
        };
        if better {
            best = Some(Snap {
                kind,
                point,
                distance,
            });
        }
    };
    for &(a, b) in edges {
        consider(InferenceKind::Endpoint, a);
        consider(InferenceKind::Endpoint, b);
        consider(InferenceKind::Midpoint, (a + b) * 0.5);
        let ab = b - a;
        let len2 = ab.length_squared();
        if len2 > 0.0 {
            let t = ((query - a).dot(ab) / len2).clamp(0.0, 1.0);
            consider(InferenceKind::OnEdge, a + ab * t);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: (DVec3, DVec3) = (DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0));

    #[test]
    fn priorities() {
        assert_eq!(
            snap_to_edges(DVec3::new(0.01, 0.01, 0.0), &[E], 0.05)
                .unwrap()
                .kind,
            InferenceKind::Endpoint
        );
        assert_eq!(
            snap_to_edges(DVec3::new(0.51, 0.01, 0.0), &[E], 0.05)
                .unwrap()
                .kind,
            InferenceKind::Midpoint
        );
        let s = snap_to_edges(DVec3::new(0.25, 0.01, 0.0), &[E], 0.05).unwrap();
        assert_eq!(s.kind, InferenceKind::OnEdge);
        assert!((s.point - DVec3::new(0.25, 0.0, 0.0)).length() < 1e-12);
        assert!(snap_to_edges(DVec3::new(0.25, 0.5, 0.0), &[E], 0.05).is_none());
    }

    #[test]
    fn our_colours() {
        let all = [
            InferenceKind::Endpoint,
            InferenceKind::Midpoint,
            InferenceKind::OnEdge,
            InferenceKind::OnFace,
        ];
        assert_eq!(
            all.map(InferenceKind::colour),
            ["#7bc043", "#17a2a0", "#e08a1e", "#8a5cd6"]
        );
    }
}
