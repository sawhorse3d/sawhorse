//! Inference (snapping) and picking for Sawhorse: a global, read-only query over the model
//! (ADR-0001).
//!
//! - [`scene::Scene`]: triangles of every visible face and sketch region, every edge, the
//!   arcs and circles among them and the guides, with a BVH over triangles and one over
//!   edges ([`bvh`], our own; no `bvh` crate).
//! - [`pick::pick`]: the entity under the cursor (vertex, edge, face, region, guide) with
//!   pixel radii converted to world units at each candidate's depth, occlusion-tested.
//! - [`infer::infer`]: where the cursor snaps for a drawing tool: endpoints, midpoints,
//!   intersections, arc centres, guide points, on-edge / on-guide, axis lines, parallel and
//!   perpendicular to a reference edge, "from point" alignments, on-face, and the ground
//!   grid when nothing else is near; with axis locks and a drawing plane.
//!
//! Colours are OURS (ADR-0003), never SketchUp's: endpoint lime, midpoint teal, on-edge
//! amber, on-face violet (the approved mock), plus intersection raspberry, centre sky,
//! parallel / perpendicular indigo, from-point and guides in muted ink, grid sage; axis
//! lines use the mock's axis colours. The UI reads them from the CSS tokens named here.

pub mod bvh;
pub mod flat;
pub mod geom;
pub mod infer;
pub mod pick;
pub mod scene;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceKind {
    Endpoint,
    Midpoint,
    OnEdge,
    OnFace,
    Intersection,
    Centre,
    AxisX,
    AxisY,
    AxisZ,
    Parallel,
    Perpendicular,
    FromPoint,
    OnGuide,
    GuidePoint,
    Grid,
    Free,
}

impl InferenceKind {
    /// Rank among point snaps within reach: lower wins outright (an endpoint beats a
    /// midpoint at the same distance). Line and surface snaps rank below every point.
    pub fn priority(self) -> u8 {
        match self {
            Self::Endpoint => 0,
            Self::Intersection | Self::Centre | Self::GuidePoint => 1,
            Self::Midpoint => 1,
            Self::FromPoint => 2,
            Self::OnEdge | Self::OnGuide => 3,
            Self::AxisX | Self::AxisY | Self::AxisZ | Self::Parallel | Self::Perpendicular => 4,
            Self::OnFace => 5,
            Self::Grid => 6,
            Self::Free => 7,
        }
    }

    /// CSS custom property holding this inference's colour in the UI.
    pub fn css_token(self) -> &'static str {
        match self {
            Self::Endpoint => "--inf-end",
            Self::Midpoint => "--inf-mid",
            Self::OnEdge => "--inf-edge",
            Self::OnFace => "--inf-face",
            Self::Intersection => "--inf-int",
            Self::Centre => "--inf-ctr",
            Self::AxisX => "--axis-x",
            Self::AxisY => "--axis-y",
            Self::AxisZ => "--axis-z",
            Self::Parallel | Self::Perpendicular => "--inf-par",
            Self::FromPoint => "--inf-from",
            Self::OnGuide | Self::GuidePoint => "--inf-guide",
            Self::Grid => "--inf-grid",
            Self::Free => "--muted",
        }
    }

    /// Light-theme colour value, for exporters and tests.
    pub fn colour(self) -> &'static str {
        match self {
            Self::Endpoint => "#7bc043",
            Self::Midpoint => "#17a2a0",
            Self::OnEdge => "#e08a1e",
            Self::OnFace => "#8a5cd6",
            Self::Intersection => "#c2417f",
            Self::Centre => "#3b8fd9",
            Self::AxisX => "#c0392b",
            Self::AxisY => "#27ae60",
            Self::AxisZ => "#2a63a8",
            Self::Parallel | Self::Perpendicular => "#4d5bd1",
            Self::FromPoint => "#6b726c",
            Self::OnGuide | Self::GuidePoint => "#7a6a4f",
            Self::Grid => "#8f9a8a",
            Self::Free => "#5f6660",
        }
    }

    /// Tooltip text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Endpoint => "Endpoint",
            Self::Midpoint => "Midpoint",
            Self::OnEdge => "On Edge",
            Self::OnFace => "On Face",
            Self::Intersection => "Intersection",
            Self::Centre => "Centre",
            Self::AxisX => "On Red Axis",
            Self::AxisY => "On Green Axis",
            Self::AxisZ => "On Blue Axis",
            Self::Parallel => "Parallel to Edge",
            Self::Perpendicular => "Perpendicular to Edge",
            Self::FromPoint => "From Point",
            Self::OnGuide => "On Guide",
            Self::GuidePoint => "Guide Point",
            Self::Grid => "Grid",
            Self::Free => "",
        }
    }

    /// The axis inference for a unit axis direction (X, Y, Z in either sense).
    pub fn axis(dir: glam::DVec3) -> Option<InferenceKind> {
        let a = dir.abs();
        if a.x > 1.0 - 1e-12 {
            Some(Self::AxisX)
        } else if a.y > 1.0 - 1e-12 {
            Some(Self::AxisY)
        } else if a.z > 1.0 - 1e-12 {
            Some(Self::AxisZ)
        } else {
            None
        }
    }

    /// Snaps to a point (as opposed to along a line or on a surface).
    pub fn is_point(self) -> bool {
        self.priority() <= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_colours() {
        let mock = [
            InferenceKind::Endpoint,
            InferenceKind::Midpoint,
            InferenceKind::OnEdge,
            InferenceKind::OnFace,
        ];
        assert_eq!(
            mock.map(InferenceKind::colour),
            ["#7bc043", "#17a2a0", "#e08a1e", "#8a5cd6"]
        );
        assert_eq!(
            mock.map(InferenceKind::css_token),
            ["--inf-end", "--inf-mid", "--inf-edge", "--inf-face"]
        );
        // SketchUp's own inference colours are not ours: no pure magenta, no pure cyan.
        for k in [
            InferenceKind::Intersection,
            InferenceKind::Centre,
            InferenceKind::Parallel,
            InferenceKind::FromPoint,
        ] {
            assert!(!["#ff00ff", "#00ffff", "#ff0000", "#00ff00"].contains(&k.colour()));
        }
    }

    #[test]
    fn kinds_serialise_like_the_ui_names_them() {
        let s = serde_json::to_string(&[
            InferenceKind::OnEdge,
            InferenceKind::AxisX,
            InferenceKind::FromPoint,
        ])
        .unwrap();
        assert_eq!(s, r#"["on-edge","axis-x","from-point"]"#);
        assert_eq!(
            InferenceKind::axis(glam::DVec3::NEG_Y),
            Some(InferenceKind::AxisY)
        );
        assert_eq!(InferenceKind::axis(glam::DVec3::ONE.normalize()), None);
    }

    #[test]
    fn points_outrank_lines_outrank_faces() {
        use InferenceKind::*;
        assert!(Endpoint.priority() < Midpoint.priority());
        assert!(Midpoint.priority() < OnEdge.priority());
        assert!(OnEdge.priority() < AxisX.priority());
        assert!(AxisX.priority() < OnFace.priority());
        assert!(OnFace.priority() < Grid.priority());
        assert!(Endpoint.is_point() && FromPoint.is_point() && !OnEdge.is_point());
    }
}
