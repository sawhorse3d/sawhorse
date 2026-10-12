//! Paper arithmetic for drawing sheets (lane 12, ADR-0010): drawing scales, what a
//! dimension measures, and how lengths are written on paper.
//!
//! - A [`Scale`] turns model lengths into paper lengths: `1:8` makes a 30 in cabinet
//!   3 3/4 in wide on paper; the architect's `1/4" = 1'-0"` makes it 5/8 in. Isometric and
//!   perspective views are not to scale ([`Scale::Nts`]).
//! - A dimension measures the **model**, never the paper: its points are model points, and
//!   [`measure`] reads them in the view's plane (along its right and up directions), so the
//!   number is the same at 1:1 and 1:48 and stays right when the scale changes.
//! - [`format_length`] writes a length the way the sheet asks: shop fractions to 1/16 in,
//!   decimal inches, feet and inches, millimetres.
//!
//! Only `+ - * /`, `sqrt` and `atan2` are used (the angle of an angular dimension), and the
//! results are compared at 1 nm in the hash, so native and WASM agree.

use glam::{DVec2, DVec3};

/// Metres per inch.
pub const M_PER_IN: f64 = 0.0254;

/// A drawing scale.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scale {
    /// `paper : model`, such as 1:8 (or 2:1, larger than life).
    Ratio { paper: f64, model: f64 },
    /// So many inches on paper to a foot of model: `3" = 1'-0"` is 3.
    Architect { inches_per_foot: f64 },
    /// Not to scale (isometric and perspective views).
    Nts,
}

impl Scale {
    /// Paper length per model length (`None`: not to scale).
    pub fn factor(&self) -> Option<f64> {
        match *self {
            Scale::Ratio { paper, model } => (paper > 0.0 && model > 0.0).then(|| paper / model),
            Scale::Architect { inches_per_foot } => {
                (inches_per_foot > 0.0).then(|| inches_per_foot / 12.0)
            }
            Scale::Nts => None,
        }
    }

    /// The scales a viewport offers, in the order of its menu.
    pub fn standard() -> Vec<Scale> {
        let r = |n: f64| Scale::Ratio {
            paper: 1.0,
            model: n,
        };
        let a = |i: f64| Scale::Architect { inches_per_foot: i };
        vec![
            r(1.0),
            r(2.0),
            r(4.0),
            r(8.0),
            r(12.0),
            r(16.0),
            r(24.0),
            r(48.0),
            a(3.0),
            a(1.5),
            a(1.0),
            a(0.75),
            a(0.5),
            a(0.25),
        ]
    }

    /// "1:8", "3\" = 1'-0\"", "1-1/2\" = 1'-0\"", "NTS".
    pub fn label(&self) -> String {
        match *self {
            Scale::Ratio { paper, model } => format!("{}:{}", plain(paper), plain(model)),
            Scale::Architect { inches_per_foot } => {
                let whole = inches_per_foot.trunc();
                let frac = format_fraction(inches_per_foot - whole, 64);
                let text = match (whole as i64, frac) {
                    (0, Some(f)) => f,
                    (w, Some(f)) => format!("{w}-{f}"),
                    (w, None) => format!("{w}"),
                };
                format!("{text}\" = 1'-0\"")
            }
            Scale::Nts => "NTS".to_string(),
        }
    }

    /// A scale must be finite and positive, between 1:10,000 and 100:1.
    pub fn check(&self) -> Result<(), &'static str> {
        match self.factor() {
            None if *self == Scale::Nts => Ok(()),
            Some(f) if f.is_finite() && (1e-4..=100.0).contains(&f) => Ok(()),
            _ => Err("a scale must be between 1:10000 and 100:1"),
        }
    }

    pub fn hash_into(&self, h: &mut crate::hash::StableHasher) {
        match *self {
            Scale::Ratio { paper, model } => {
                h.u64(1);
                h.len(paper * 1e-6);
                h.len(model * 1e-6);
            }
            Scale::Architect { inches_per_foot } => {
                h.u64(2);
                h.len(inches_per_foot * 1e-6);
            }
            Scale::Nts => h.u64(3),
        }
    }
}

/// A number without trailing zeros ("8", "1.5").
fn plain(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

/// `x` (0 <= x < 1) as a fraction "1/2", "3/4" when it is one of 1/64ths, `None` for 0.
fn format_fraction(x: f64, den: u32) -> Option<String> {
    let n = (x * f64::from(den)).round() as u32;
    if n == 0 {
        return None;
    }
    let g = gcd(n, den);
    Some(format!("{}/{}", n / g, den / g))
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// The standard views of a sheet viewport (Z up; the model's front is the -Y side).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum StdView {
    /// Plan: looking down.
    Top,
    Bottom,
    /// Looking at the -Y side (towards +Y), like the app's Front view.
    Front,
    Back,
    /// Looking at the -X side (towards +X).
    Left,
    /// Looking at the +X side (towards -X), like the app's Right view.
    Right,
    /// The app's three-quarter view, parallel (not to scale on paper).
    Iso,
}

impl StdView {
    /// (view direction, up) for the view.
    pub fn axes(self) -> (DVec3, DVec3) {
        match self {
            StdView::Top => (-DVec3::Z, DVec3::Y),
            StdView::Bottom => (DVec3::Z, -DVec3::Y),
            StdView::Front => (DVec3::Y, DVec3::Z),
            StdView::Back => (-DVec3::Y, DVec3::Z),
            StdView::Left => (DVec3::X, DVec3::Z),
            StdView::Right => (-DVec3::X, DVec3::Z),
            StdView::Iso => (DVec3::new(1.0, 1.0, -1.0).normalize(), DVec3::Z),
        }
    }

    /// Orthographic and square to the model's axes: a drawing to scale.
    pub fn to_scale(self) -> bool {
        self != StdView::Iso
    }
}

/// A camera, as scenes store it (metres, Z up).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct CameraSpec {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    /// Parallel projection.
    #[serde(default)]
    pub parallel: bool,
    /// Vertical field of view, degrees (perspective).
    #[serde(default = "default_fov")]
    pub fov: f64,
}

fn default_fov() -> f64 {
    35.0
}

/// What a dimension measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum DimKind {
    /// Along the view's right direction (two points).
    Horizontal,
    /// Along the view's up direction (two points).
    Vertical,
    /// Point to point in the view's plane (two points).
    Aligned,
    /// The angle at the first point between the other two (three points), degrees.
    Angular,
    /// The radius of the arc through three points.
    Radius,
    /// Its diameter.
    Diameter,
}

impl DimKind {
    /// How many model points the dimension is attached to.
    pub fn points(self) -> usize {
        match self {
            DimKind::Horizontal | DimKind::Vertical | DimKind::Aligned => 2,
            DimKind::Angular | DimKind::Radius | DimKind::Diameter => 3,
        }
    }
}

/// What a dimension of `kind` reads for model points `pts`, in a view whose right and up
/// directions are `right` and `up`: metres (degrees for an angle). `None` when the points
/// do not make one (coincident, collinear for an arc).
pub fn measure(right: DVec3, up: DVec3, kind: DimKind, pts: &[DVec3]) -> Option<f64> {
    if pts.len() < kind.points() || pts.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let q: Vec<DVec2> = pts
        .iter()
        .map(|p| DVec2::new(p.dot(right), p.dot(up)))
        .collect();
    let v = match kind {
        DimKind::Horizontal => (q[1].x - q[0].x).abs(),
        DimKind::Vertical => (q[1].y - q[0].y).abs(),
        DimKind::Aligned => q[0].distance(q[1]),
        DimKind::Angular => {
            let (a, b) = (q[1] - q[0], q[2] - q[0]);
            if a.length() < 1e-12 || b.length() < 1e-12 {
                return None;
            }
            a.perp_dot(b).abs().atan2(a.dot(b)).to_degrees()
        }
        DimKind::Radius | DimKind::Diameter => {
            let r = circumradius(q[0], q[1], q[2])?;
            if kind == DimKind::Radius { r } else { 2.0 * r }
        }
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

/// The centre of the circle through three points (`None`: collinear).
pub fn circumcentre(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    let scale = (b - a).length().max((c - a).length()).max(1e-30);
    if d.abs() <= 1e-12 * scale * scale {
        return None;
    }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    Some(DVec2::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    ))
}

fn circumradius(a: DVec2, b: DVec2, c: DVec2) -> Option<f64> {
    circumcentre(a, b, c).map(|o| o.distance(a))
}

/// How lengths are written on a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LengthFormat {
    /// Inches with shop fractions to 1/`den` ("11 1/4\"").
    Fraction { den: u32 },
    /// Decimal inches to `places` ("11.25\"").
    Decimal { places: u8 },
    /// Feet and inches with fractions to 1/`den` ("5' 11 1/4\"").
    FeetInches { den: u32 },
    /// Millimetres to `places` ("285.8 mm").
    Millimetres { places: u8 },
}

impl Default for LengthFormat {
    fn default() -> Self {
        LengthFormat::Fraction { den: 16 }
    }
}

impl LengthFormat {
    pub fn check(&self) -> Result<(), &'static str> {
        match *self {
            LengthFormat::Fraction { den } | LengthFormat::FeetInches { den }
                if ![2, 4, 8, 16, 32, 64].contains(&den) =>
            {
                Err("fractions are to 1/2, 1/4, 1/8, 1/16, 1/32 or 1/64 in")
            }
            LengthFormat::Decimal { places } | LengthFormat::Millimetres { places }
                if places > 4 =>
            {
                Err("at most 4 decimal places")
            }
            _ => Ok(()),
        }
    }

    pub fn hash_into(&self, h: &mut crate::hash::StableHasher) {
        match *self {
            LengthFormat::Fraction { den } => h.u64(10 + u64::from(den)),
            LengthFormat::Decimal { places } => h.u64(200 + u64::from(places)),
            LengthFormat::FeetInches { den } => h.u64(300 + u64::from(den)),
            LengthFormat::Millimetres { places } => h.u64(400 + u64::from(places)),
        }
    }
}

/// Inches as "11 1/4", "3/4", "72" to 1/`den` (rounded to the nearest).
fn fraction_inches(inches: f64, den: u32) -> String {
    let neg = inches < 0.0;
    let steps = (inches.abs() * f64::from(den)).round() as u64;
    let whole = steps / u64::from(den);
    let num = (steps % u64::from(den)) as u32;
    let text = match (whole, num) {
        (w, 0) => format!("{w}"),
        (0, n) => {
            let g = gcd(n, den);
            format!("{}/{}", n / g, den / g)
        }
        (w, n) => {
            let g = gcd(n, den);
            format!("{w} {}/{}", n / g, den / g)
        }
    };
    if neg && steps > 0 {
        format!("-{text}")
    } else {
        text
    }
}

fn decimal(x: f64, places: u8) -> String {
    let s = format!("{:.*}", usize::from(places), x);
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s[1..].to_string()
    } else {
        s
    }
}

/// A length (metres) as the sheet writes it, with its unit mark.
pub fn format_length(m: f64, f: LengthFormat) -> String {
    let inches = m / M_PER_IN;
    match f {
        LengthFormat::Fraction { den } => format!("{}\"", fraction_inches(inches, den)),
        LengthFormat::Decimal { places } => format!("{}\"", decimal(inches, places)),
        LengthFormat::FeetInches { den } => {
            let steps = (inches.abs() * f64::from(den)).round() / f64::from(den);
            let feet = (steps / 12.0).floor();
            let rest = steps - feet * 12.0;
            let sign = if inches < 0.0 && steps > 0.0 { "-" } else { "" };
            if feet > 0.0 {
                format!("{sign}{}' {}\"", feet as u64, fraction_inches(rest, den))
            } else {
                format!("{sign}{}\"", fraction_inches(rest, den))
            }
        }
        LengthFormat::Millimetres { places } => format!("{} mm", decimal(m * 1000.0, places)),
    }
}

/// An angle (degrees) as written on a dimension: "90°", "22.5°".
pub fn format_angle(deg: f64) -> String {
    let s = format!("{deg:.1}");
    let s = s.strip_suffix(".0").unwrap_or(&s);
    format!("{s}°")
}

/// A column of a cut-list table on a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum CutColumn {
    Part,
    Qty,
    Thickness,
    Width,
    Length,
    /// T x W x L in one cell.
    Size,
    Material,
    Grain,
    /// Board feet for lumber, area for sheet goods.
    Amount,
    Notes,
}

impl CutColumn {
    /// The heading the table prints.
    pub fn header(self) -> &'static str {
        match self {
            CutColumn::Part => "Part",
            CutColumn::Qty => "Qty",
            CutColumn::Thickness => "T",
            CutColumn::Width => "W",
            CutColumn::Length => "L",
            CutColumn::Size => "T x W x L",
            CutColumn::Material => "Material",
            CutColumn::Grain => "Grain",
            CutColumn::Amount => "Bd ft / area",
            CutColumn::Notes => "Notes",
        }
    }

    /// What a new table shows: the report's columns.
    pub fn default_set() -> Vec<CutColumn> {
        vec![
            CutColumn::Part,
            CutColumn::Qty,
            CutColumn::Thickness,
            CutColumn::Width,
            CutColumn::Length,
            CutColumn::Material,
            CutColumn::Amount,
            CutColumn::Notes,
        ]
    }

    /// Its share of the table's width.
    pub fn weight(self) -> f64 {
        match self {
            CutColumn::Part | CutColumn::Material | CutColumn::Notes => 3.0,
            CutColumn::Size => 4.0,
            CutColumn::Amount => 2.0,
            CutColumn::Qty => 1.0,
            _ => 1.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN: f64 = M_PER_IN;

    #[test]
    fn scale_math() {
        // A 30 in cabinet: 3 3/4 in at 1:8, 5/8 in at 1/4" = 1'.
        let w = 30.0 * IN;
        let r8 = Scale::Ratio {
            paper: 1.0,
            model: 8.0,
        };
        assert!((w * r8.factor().unwrap() / IN - 3.75).abs() < 1e-12);
        let quarter = Scale::Architect {
            inches_per_foot: 0.25,
        };
        assert!((w * quarter.factor().unwrap() / IN - 0.625).abs() < 1e-12);
        assert_eq!(Scale::Nts.factor(), None);
        let labels: Vec<String> = Scale::standard().iter().map(Scale::label).collect();
        assert_eq!(
            labels,
            [
                "1:1",
                "1:2",
                "1:4",
                "1:8",
                "1:12",
                "1:16",
                "1:24",
                "1:48",
                "3\" = 1'-0\"",
                "1-1/2\" = 1'-0\"",
                "1\" = 1'-0\"",
                "3/4\" = 1'-0\"",
                "1/2\" = 1'-0\"",
                "1/4\" = 1'-0\""
            ]
        );
        assert!(Scale::standard().iter().all(|s| s.check().is_ok()));
        assert!(
            Scale::Ratio {
                paper: 1.0,
                model: 0.0
            }
            .check()
            .is_err()
        );
        assert!(
            Scale::Ratio {
                paper: 1.0,
                model: 1e6
            }
            .check()
            .is_err()
        );
        assert!(Scale::Nts.check().is_ok());
        // 3" = 1' is 1:4; 1/4" = 1' is 1:48.
        assert_eq!(
            Scale::Architect {
                inches_per_foot: 3.0
            }
            .factor(),
            Some(0.25)
        );
        assert!(
            (Scale::Architect {
                inches_per_foot: 0.25
            }
            .factor()
            .unwrap()
                - 1.0 / 48.0)
                .abs()
                < 1e-15
        );
    }

    #[test]
    fn dimensions_measure_the_model_in_the_view_plane() {
        let (r, u) = (DVec3::X, DVec3::Z);
        let a = DVec3::new(0.0, 0.3, 0.0);
        let b = DVec3::new(30.0 * IN, 0.0, 72.0 * IN);
        assert!((measure(r, u, DimKind::Horizontal, &[a, b]).unwrap() - 30.0 * IN).abs() < 1e-12);
        assert!((measure(r, u, DimKind::Vertical, &[a, b]).unwrap() - 72.0 * IN).abs() < 1e-12);
        // Aligned: in the view's plane (the depth difference does not count).
        let al = measure(r, u, DimKind::Aligned, &[a, b]).unwrap();
        assert!((al - (30.0f64.hypot(72.0)) * IN).abs() < 1e-12);
        assert_eq!(measure(r, u, DimKind::Aligned, &[a, a]), None);
        // A right angle, and the radius and diameter of a circle through three points.
        let o = DVec3::ZERO;
        let ang = measure(r, u, DimKind::Angular, &[o, DVec3::X, DVec3::Z]).unwrap();
        assert!((ang - 90.0).abs() < 1e-12);
        let c = |t: f64| DVec3::new(2.0 + 0.5 * t.cos(), 0.0, 1.0 + 0.5 * t.sin());
        let pts = [c(0.1), c(1.3), c(2.9)];
        assert!((measure(r, u, DimKind::Radius, &pts).unwrap() - 0.5).abs() < 1e-12);
        assert!((measure(r, u, DimKind::Diameter, &pts).unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(
            measure(r, u, DimKind::Radius, &[o, DVec3::X, DVec3::X * 2.0]),
            None
        );
    }

    #[test]
    fn lengths_on_paper() {
        let f16 = LengthFormat::Fraction { den: 16 };
        assert_eq!(format_length(11.25 * IN, f16), "11 1/4\"");
        assert_eq!(format_length(0.75 * IN, f16), "3/4\"");
        assert_eq!(format_length(72.0 * IN, f16), "72\"");
        assert_eq!(format_length(-0.5 * IN, f16), "-1/2\"");
        assert_eq!(format_length(11.0 * IN + 0.03 * IN, f16), "11\"");
        assert_eq!(
            format_length(11.25 * IN, LengthFormat::Decimal { places: 2 }),
            "11.25\""
        );
        assert_eq!(
            format_length(71.25 * IN, LengthFormat::FeetInches { den: 16 }),
            "5' 11 1/4\""
        );
        assert_eq!(
            format_length(11.25 * IN, LengthFormat::FeetInches { den: 16 }),
            "11 1/4\""
        );
        assert_eq!(
            format_length(0.28575, LengthFormat::Millimetres { places: 1 }),
            "285.8 mm"
        );
        assert_eq!(
            format_length(-0.0, LengthFormat::Millimetres { places: 0 }),
            "0 mm"
        );
        assert_eq!(format_angle(90.0), "90°");
        assert_eq!(format_angle(22.5), "22.5°");
        assert!(LengthFormat::Fraction { den: 10 }.check().is_err());
        assert!(LengthFormat::Decimal { places: 3 }.check().is_ok());
    }
}
