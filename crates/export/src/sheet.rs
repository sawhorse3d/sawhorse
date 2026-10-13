//! A drawing sheet laid out on paper (lane 12, ADR-0010): every item of a sheet turned into
//! simple primitives in paper metres (lines, filled rings, rectangles, text, images), the
//! one picture the sheet canvas, the PDF, the SVG, the DXF and the PNG all draw, so what
//! you see is what prints.
//!
//! - **Viewports** draw the hidden-line [`crate::drawing`] of their scene or view at their
//!   scale (a model length times the scale's factor is its length on paper), centred on the
//!   drawing or on the point the viewport keeps, clipped to their rectangle. Not-to-scale
//!   views (isometric, perspective) fit their rectangle. Pens: profiles 0.5 mm, edges
//!   0.25 mm, cut lines 0.7 mm, hidden lines 0.18 mm dashed; section caps filled.
//! - **Dimensions** are computed from their model points every time (never stored as
//!   paper): their value is what [`measure`] reads in the viewport's view, written in the
//!   sheet's length format; their lines follow the points wherever the viewport puts them.
//!   A point anchored on a solid (`ModelAnchor`) is found again on the solid as it is now,
//!   so a dimension follows push/pull, Move, Rotate and Scale; a point that is not (or
//!   whose corner is gone) stays where it was placed.
//! - **Labels** (with leaders to the paper or to model points), **cut-list tables** (the
//!   kernel's cut list in the item's columns), **images**, **lines**, **rectangles**, each
//!   viewport's **caption**, the **margin frame** and the **title block**.
//!
//! Deterministic and hashed (the hash covers every primitive at 1 nm).

use glam::{DVec2, DVec3};
use sawhorse_kernel::Model;
use sawhorse_kernel::cutlist::{CutOptions, Rounding};
use sawhorse_kernel::hash::StableHasher;
use sawhorse_kernel::paper::{
    DimKind, LengthFormat, Scale, circumcentre, format_angle, format_length, measure,
};
use sawhorse_kernel::sheets::{DimStyle, ItemKind, LeaderEnd, PaperUnits, Sheet, ViewportSource};

use crate::drawing::{DrawOptions, Drawing, Frame, LineKind, ViewSource, draw_scene};

const MM: f64 = 0.001;
const IN: f64 = 0.0254;

/// Pens (paper metres).
pub const PEN_PROFILE: f64 = 0.5 * MM;
pub const PEN_EDGE: f64 = 0.25 * MM;
pub const PEN_CUT: f64 = 0.7 * MM;
pub const PEN_HIDDEN: f64 = 0.18 * MM;
pub const PEN_THIN: f64 = 0.18 * MM;
/// Dimension text height.
pub const DIM_TEXT: f64 = 2.4 * MM;

const INK: [u8; 3] = [24, 26, 25];
const GREY: [u8; 3] = [110, 116, 112];
const LIGHT: [u8; 3] = [170, 174, 170];

/// How text sits on its point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// A primitive of a sheet, paper metres from the top-left corner, y down.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "p", rename_all = "snake_case")]
pub enum Prim {
    Line {
        a: [f64; 2],
        b: [f64; 2],
        width: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        dash: Option<[f64; 2]>,
        color: [u8; 3],
    },
    /// Filled rings (even-odd).
    Fill {
        rings: Vec<Vec<[f64; 2]>>,
        color: [u8; 3],
    },
    Rect {
        rect: [f64; 4],
        width: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        stroke: Option<[u8; 3]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fill: Option<[u8; 3]>,
    },
    /// One line of text: `at` is on its baseline; `angle` in degrees, counter-clockwise on
    /// the page; `max` the width it must fit (shortened with "..." when longer).
    Text {
        at: [f64; 2],
        text: String,
        size: f64,
        anchor: Anchor,
        #[serde(skip_serializing_if = "is_zero")]
        angle: f64,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        bold: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        color: [u8; 3],
    },
    /// An image the model holds, drawn into `rect`.
    Image { rect: [f64; 4], path: String },
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// Where a viewport puts the model: drawing point `centre` at its rectangle's centre, at
/// `factor` paper metres per drawing unit.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ViewportInfo {
    pub id: u64,
    pub rect: [f64; 4],
    pub frame: Frame,
    pub factor: f64,
    pub centre: [f64; 2],
    /// To scale (the scale's factor) or fitted (NTS).
    pub to_scale: bool,
    /// Why it could not be drawn, if it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl ViewportInfo {
    /// Paper point of a drawing point.
    pub fn paper(&self, d: DVec2) -> DVec2 {
        let cx = self.rect[0] + self.rect[2] / 2.0;
        let cy = self.rect[1] + self.rect[3] / 2.0;
        DVec2::new(
            cx + (d.x - self.centre[0]) * self.factor,
            cy - (d.y - self.centre[1]) * self.factor,
        )
    }

    /// Paper point of a model point (`None`: behind a perspective eye).
    pub fn model_to_paper(&self, p: DVec3) -> Option<DVec2> {
        self.frame.project(p).map(|q| self.paper(q.truncate()))
    }

    /// Drawing point of a paper point.
    pub fn drawing(&self, q: DVec2) -> DVec2 {
        let cx = self.rect[0] + self.rect[2] / 2.0;
        let cy = self.rect[1] + self.rect[3] / 2.0;
        DVec2::new(
            self.centre[0] + (q.x - cx) / self.factor,
            self.centre[1] - (q.y - cy) / self.factor,
        )
    }
}

/// What the canvas needs to pick and edit an item.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ItemInfo {
    pub id: u64,
    pub kind: &'static str,
    /// `[x, y, w, h]` around everything it draws.
    pub bounds: [f64; 4],
    /// A dimension's text, as drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A dimension's model points as drawn: its anchors found on the model as it is now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<[f64; 3]>>,
    /// A dimension with a point whose anchor is no longer on the model (the solid or the
    /// corner is gone): that point stays where it was placed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
    /// The range of `prims` it drew (`start..end`): the canvas moves them while dragging.
    pub prims: [usize; 2],
}

/// A sheet laid out (see the module docs).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SheetRender {
    pub id: u64,
    pub name: String,
    /// Width and height, metres.
    pub size: [f64; 2],
    pub prims: Vec<Prim>,
    pub items: Vec<ItemInfo>,
    pub viewports: Vec<ViewportInfo>,
    /// Every viewport's drawing hash (what goldens compare).
    pub drawings: Vec<String>,
    pub hash: String,
}

// ---- text metrics -------------------------------------------------------------------

/// Helvetica advance widths (1/1000 em) for ASCII 32..126 (the font's public metrics, the
/// same table the PDF writer uses).
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Width of `s` at text height `size` (Helvetica; bold about 5% wider).
pub fn text_width(s: &str, size: f64, bold: bool) -> f64 {
    let w: u32 = s
        .chars()
        .map(|c| {
            let k = c as u32;
            if (32..=126).contains(&k) {
                return u32::from(HELVETICA[(k - 32) as usize]);
            }
            // The few others the PDF writer encodes (src/cutlist/pdf.ts, WIDE); anything
            // else is drawn as "?" there, 556 wide.
            match c {
                '×' => 584,
                '·' => 278,
                '—' => 1000,
                '²' => 333,
                '½' | '¼' | '¾' => 834,
                '°' => 400,
                '’' => 222,
                '“' | '”' => 333,
                'Ø' => 778,
                _ => 556,
            }
        })
        .sum();
    f64::from(w) * size * if bold { 1.05 } else { 1.0 } / 1000.0
}

/// `s` shortened with "..." to fit `width` (as the PDF writer's `fit` does).
pub fn fit_text(s: &str, size: f64, width: f64, bold: bool) -> String {
    if text_width(s, size, bold) <= width {
        return s.to_string();
    }
    let mut t: Vec<char> = s.chars().collect();
    while t.len() > 1
        && text_width(&format!("{}...", t.iter().collect::<String>()), size, bold) > width
    {
        t.pop();
    }
    format!("{}...", t.iter().collect::<String>().trim_end())
}

// ---- geometry helpers -------------------------------------------------------------------

/// Liang-Barsky: the part of segment a-b inside `rect`.
fn clip_line(a: DVec2, b: DVec2, r: [f64; 4]) -> Option<(DVec2, DVec2)> {
    let (x0, y0, x1, y1) = (r[0], r[1], r[0] + r[2], r[1] + r[3]);
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - x0),
        (d.x, x1 - a.x),
        (-d.y, a.y - y0),
        (d.y, y1 - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then(|| (a + d * t0, a + d * t1))
}

/// Sutherland-Hodgman: a ring clipped to `rect`.
fn clip_ring(ring: &[DVec2], r: [f64; 4]) -> Vec<DVec2> {
    let edges: [(DVec2, f64); 4] = [
        (DVec2::X, r[0]),
        (-DVec2::X, -(r[0] + r[2])),
        (DVec2::Y, r[1]),
        (-DVec2::Y, -(r[1] + r[3])),
    ];
    let mut out = ring.to_vec();
    for (n, c) in edges {
        let pts = std::mem::take(&mut out);
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            let (dp, dq) = (p.dot(n) - c, q.dot(n) - c);
            if dp >= 0.0 {
                out.push(p);
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                out.push(p + (q - p) * (dp / (dp - dq)));
            }
        }
    }
    out
}

/// Grows a bounds box.
#[derive(Default)]
struct Bounds(Option<[f64; 4]>);

impl Bounds {
    fn add(&mut self, p: [f64; 2]) {
        let b = self.0.get_or_insert([p[0], p[1], p[0], p[1]]);
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    fn rect(&self) -> [f64; 4] {
        self.0
            .map_or([0.0; 4], |b| [b[0], b[1], b[2] - b[0], b[3] - b[1]])
    }
}

fn line(a: DVec2, b: DVec2, width: f64, color: [u8; 3]) -> Prim {
    Prim::Line {
        a: a.to_array(),
        b: b.to_array(),
        width,
        dash: None,
        color,
    }
}

fn text(at: DVec2, s: &str, size: f64, anchor: Anchor, bold: bool) -> Prim {
    Prim::Text {
        at: at.to_array(),
        text: s.to_string(),
        size,
        anchor,
        angle: 0.0,
        bold,
        max: None,
        color: INK,
    }
}

/// An arrowhead at `tip` pointing along `dir` (unit), as a filled triangle.
fn arrow(tip: DVec2, dir: DVec2) -> Prim {
    let (len, half) = (2.4 * MM, 0.75 * MM);
    let back = tip - dir * len;
    let n = dir.perp();
    Prim::Fill {
        rings: vec![vec![
            tip.to_array(),
            (back + n * half).to_array(),
            (back - n * half).to_array(),
        ]],
        color: INK,
    }
}

/// The end of a dimension line at `p` along `dir` (unit, pointing outwards), in `style`.
fn dim_end(out: &mut Vec<Prim>, p: DVec2, dir: DVec2, style: DimStyle) {
    match style {
        DimStyle::Arrows => out.push(arrow(p, dir)),
        DimStyle::Ticks => {
            // A slash at 45 degrees across the line.
            let s = (dir + dir.perp()).normalize() * 1.6 * MM;
            out.push(line(p - s, p + s, 0.35 * MM, INK));
        }
        DimStyle::Dots => {
            let r = 0.6 * MM;
            let ring: Vec<[f64; 2]> = (0..12)
                .map(|k| {
                    let a = std::f64::consts::TAU * f64::from(k) / 12.0;
                    (p + DVec2::new(a.cos(), a.sin()) * r).to_array()
                })
                .collect();
            out.push(Prim::Fill {
                rings: vec![ring],
                color: INK,
            });
        }
    }
}

/// Text along a line from `a` to `b`, centred, `lift` above it, kept upright.
fn text_along(a: DVec2, b: DVec2, s: &str, lift: f64) -> Prim {
    let mut d = b - a;
    if d.x < -1e-12 || (d.x.abs() <= 1e-12 && d.y > 0.0) {
        d = -d;
    }
    let d = d.normalize_or(DVec2::X);
    // Paper y runs down: "above" the line is -perp in page terms.
    let up = DVec2::new(d.y, -d.x);
    let mid = (a + b) * 0.5 + up * lift;
    let angle = (-d.y).atan2(d.x).to_degrees();
    Prim::Text {
        at: mid.to_array(),
        text: s.to_string(),
        size: DIM_TEXT,
        anchor: Anchor::Middle,
        angle: if angle.abs() < 1e-9 { 0.0 } else { angle },
        bold: false,
        max: None,
        color: INK,
    }
}

// ---- the sheet ----------------------------------------------------------------------------

/// What the sheet render needs to know beyond the sheet.
#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
pub struct RenderOptions {
    /// The sheet's number and the document's count of sheets, for "n of N".
    #[serde(default)]
    pub number: usize,
    #[serde(default)]
    pub count: usize,
}

fn source_of(s: &ViewportSource, model: &Model) -> (Option<String>, ViewSource) {
    match s {
        ViewportSource::Standard { view } => (None, ViewSource::Standard { view: *view }),
        ViewportSource::Camera { camera } => (None, ViewSource::Camera { camera: *camera }),
        ViewportSource::Scene { name } => {
            let cam = model
                .scenes()
                .iter()
                .find(|sc| sc.name == *name)
                .map(|sc| sawhorse_kernel::paper::CameraSpec {
                    eye: sc.eye,
                    target: sc.target,
                    up: sc.up,
                    parallel: sc.parallel,
                    fov: sc.fov,
                })
                .unwrap_or(sawhorse_kernel::paper::CameraSpec {
                    eye: [0.0, -1.0, 0.0],
                    target: [0.0; 3],
                    up: [0.0, 0.0, 1.0],
                    parallel: true,
                    fov: 35.0,
                });
            (Some(name.clone()), ViewSource::Camera { camera: cam })
        }
    }
}

/// The drawing of one viewport item and where it puts the model.
pub fn viewport_drawing(
    model: &Model,
    item: &ItemKind,
    id: u64,
) -> Option<(Drawing, ViewportInfo)> {
    let ItemKind::Viewport {
        rect,
        source,
        scale,
        centre,
        hidden_lines,
        ..
    } = item
    else {
        return None;
    };
    let (scene, view) = source_of(source, model);
    let drawn = draw_scene(
        model,
        scene.as_deref(),
        &view,
        &DrawOptions {
            hidden: *hidden_lines,
        },
    );
    let (drawing, problem) = match drawn {
        Ok(d) => (d, None),
        Err(e) => (
            Drawing {
                frame: Frame::of(&ViewSource::Standard {
                    view: sawhorse_kernel::paper::StdView::Front,
                })
                .ok()?,
                lines: Vec::new(),
                fills: Vec::new(),
                bounds: None,
                hash: String::new(),
                edges: Vec::new(),
            },
            Some(e.to_string()),
        ),
    };
    let b = drawing.bounds.unwrap_or([-0.05, -0.05, 0.05, 0.05]);
    let (dw, dh) = ((b[2] - b[0]).max(1e-6), (b[3] - b[1]).max(1e-6));
    let (factor, to_scale) = match scale.factor() {
        Some(f) => (f, true),
        None => (0.92 * (rect[2] / dw).min(rect[3] / dh), false),
    };
    let c = centre.unwrap_or([(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]);
    let info = ViewportInfo {
        id,
        rect: *rect,
        frame: drawing.frame,
        factor,
        centre: c,
        to_scale,
        problem,
    };
    Some((drawing, info))
}

/// The scale label a title block shows when its field is empty.
fn scale_note(sheet: &Sheet) -> String {
    let mut labels: Vec<String> = Vec::new();
    for it in &sheet.items {
        if let ItemKind::Viewport { scale, .. } = &it.kind
            && *scale != Scale::Nts
            && !labels.contains(&scale.label())
        {
            labels.push(scale.label());
        }
    }
    match labels.len() {
        0 => String::new(),
        1 => labels.remove(0),
        _ => "As noted".into(),
    }
}

/// Lay out sheet `id` of `model` (see the module docs).
pub fn render(model: &Model, id: u64, opts: &RenderOptions) -> Option<SheetRender> {
    let sheet = model.sheet(id)?;
    let (w, h) = sheet.setup.size();
    let mut prims: Vec<Prim> = Vec::new();
    let mut items = Vec::new();
    let mut viewports: Vec<ViewportInfo> = Vec::new();
    let mut drawings = Vec::new();
    let fmt = sheet.setup.format;
    // The margin frame.
    let [mt, mr, mb, ml] = sheet.setup.margins;
    prims.push(Prim::Rect {
        rect: [ml, mt, w - ml - mr, h - mt - mb],
        width: 0.35 * MM,
        stroke: Some(INK),
        fill: None,
    });
    // Viewports first (dimensions and leaders need them).
    for it in &sheet.items {
        if let Some((d, info)) = viewport_drawing(model, &it.kind, it.id) {
            drawings.push((it.id, d));
            viewports.push(info);
        }
    }
    let vp = |id: u64| viewports.iter().find(|v| v.id == id);
    for it in &sheet.items {
        let start = prims.len();
        let mut text_out = None;
        let mut points_out = None;
        let mut detached = false;
        let kind = match &it.kind {
            ItemKind::Viewport {
                rect,
                caption,
                border,
                ..
            } => {
                let info = vp(it.id).expect("drawn above");
                let d = &drawings
                    .iter()
                    .find(|(k, _)| *k == it.id)
                    .expect("drawn above")
                    .1;
                for f in &d.fills {
                    let rings: Vec<Vec<[f64; 2]>> = f
                        .rings
                        .iter()
                        .map(|r| {
                            let ring: Vec<DVec2> = r
                                .iter()
                                .map(|p| info.paper(DVec2::from_array(*p)))
                                .collect();
                            clip_ring(&ring, *rect)
                                .into_iter()
                                .map(|p| p.to_array())
                                .collect::<Vec<_>>()
                        })
                        .filter(|r| r.len() >= 3)
                        .collect();
                    if !rings.is_empty() {
                        let fill = model
                            .style()
                            .and_then(|s| s.section_fill_color)
                            .unwrap_or(crate::section::DEFAULT_FILL);
                        prims.push(Prim::Fill { rings, color: fill });
                    }
                }
                // Hidden lines under, cut lines over.
                let order = [
                    LineKind::Hidden,
                    LineKind::Edge,
                    LineKind::Profile,
                    LineKind::Cut,
                ];
                for kind in order {
                    for l in d.lines.iter().filter(|l| l.kind == kind) {
                        let (a, b) = (
                            info.paper(DVec2::from_array(l.a)),
                            info.paper(DVec2::from_array(l.b)),
                        );
                        if let Some((a, b)) = clip_line(a, b, *rect) {
                            let (width, dash, color) = match kind {
                                LineKind::Edge => (PEN_EDGE, None, INK),
                                LineKind::Profile => (PEN_PROFILE, None, INK),
                                LineKind::Cut => (PEN_CUT, None, INK),
                                LineKind::Hidden => (PEN_HIDDEN, Some([1.5 * MM, 1.0 * MM]), GREY),
                            };
                            prims.push(Prim::Line {
                                a: a.to_array(),
                                b: b.to_array(),
                                width,
                                dash,
                                color,
                            });
                        }
                    }
                }
                if let Some(p) = &info.problem {
                    prims.push(text(
                        DVec2::new(rect[0] + 2.0 * MM, rect[1] + 5.0 * MM),
                        &format!("Cannot draw: {p}"),
                        2.5 * MM,
                        Anchor::Start,
                        false,
                    ));
                }
                if *border {
                    prims.push(Prim::Rect {
                        rect: *rect,
                        width: PEN_THIN,
                        stroke: Some(INK),
                        fill: None,
                    });
                }
                if let Some(c) = caption.as_deref().filter(|c| !c.trim().is_empty()) {
                    let at = DVec2::new(rect[0], rect[1] + rect[3] + 4.5 * MM);
                    prims.push(text(at, c, 3.0 * MM, Anchor::Start, true));
                    let tw = text_width(c, 3.0 * MM, true);
                    prims.push(line(
                        at + DVec2::new(0.0, 1.2 * MM),
                        at + DVec2::new(tw, 1.2 * MM),
                        0.3 * MM,
                        INK,
                    ));
                }
                "viewport"
            }
            ItemKind::Dimension {
                viewport,
                dim,
                points,
                anchors,
                offset,
                style,
                text: own,
            } => {
                if let Some(info) = vp(*viewport) {
                    let model_pts: Vec<DVec3> = points
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let Some(Some(a)) = anchors.get(i) else {
                                return DVec3::from_array(*p);
                            };
                            match model.anchor_point(a, *p) {
                                Some(q) => DVec3::from_array(q),
                                None => {
                                    detached = true;
                                    DVec3::from_array(*p)
                                }
                            }
                        })
                        .collect();
                    points_out = Some(model_pts.iter().map(|p| p.to_array()).collect());
                    let r = DVec3::from_array(info.frame.right);
                    let u = DVec3::from_array(info.frame.up);
                    let value = measure(r, u, *dim, &model_pts);
                    let words = match (own, value) {
                        (Some(t), _) => t.clone(),
                        (None, Some(v)) => match dim {
                            DimKind::Angular => format_angle(v),
                            DimKind::Radius => format!("R {}", format_length(v, fmt)),
                            DimKind::Diameter => format!("Ø {}", format_length(v, fmt)),
                            _ => format_length(v, fmt),
                        },
                        (None, None) => "?".into(),
                    };
                    let paper: Option<Vec<DVec2>> =
                        model_pts.iter().map(|p| info.model_to_paper(*p)).collect();
                    if let Some(p) = paper {
                        dimension(&mut prims, *dim, &p, *offset, *style, &words);
                    }
                    text_out = Some(words);
                }
                "dimension"
            }
            ItemKind::Label {
                at,
                text: t,
                size,
                leader,
            } => {
                let at = DVec2::from_array(*at);
                let lines: Vec<&str> = t.lines().collect();
                let widest = lines
                    .iter()
                    .map(|l| text_width(l, *size, false))
                    .fold(0.0, f64::max);
                for (k, l) in lines.iter().enumerate() {
                    prims.push(text(
                        at + DVec2::new(0.0, *size * 1.25 * k as f64),
                        l,
                        *size,
                        Anchor::Start,
                        false,
                    ));
                }
                let target = match leader {
                    Some(LeaderEnd::Paper { at }) => Some(DVec2::from_array(*at)),
                    Some(LeaderEnd::Model {
                        viewport,
                        point,
                        anchor,
                    }) => {
                        let p = anchor
                            .as_ref()
                            .and_then(|a| model.anchor_point(a, *point))
                            .unwrap_or(*point);
                        vp(*viewport).and_then(|v| v.model_to_paper(DVec3::from_array(p)))
                    }
                    None => None,
                };
                if let Some(tip) = target {
                    // From the side of the first line nearer the target.
                    let mid_y = at.y - *size * 0.35;
                    let from = if tip.x < at.x + widest / 2.0 {
                        DVec2::new(at.x - 1.0 * MM, mid_y)
                    } else {
                        DVec2::new(at.x + widest + 1.0 * MM, mid_y)
                    };
                    let dir = (tip - from).normalize_or(DVec2::X);
                    prims.push(line(from, tip - dir * 1.2 * MM, PEN_THIN, INK));
                    prims.push(arrow(tip, dir));
                }
                "label"
            }
            ItemKind::Table {
                rect,
                columns,
                decimal,
                visible_only,
                title,
            } => {
                let rounding = if sheet.setup.units == PaperUnits::Millimetres
                    || matches!(fmt, LengthFormat::Millimetres { .. })
                {
                    Rounding::HalfMm
                } else {
                    Rounding::Sixteenth
                };
                let list = model.cut_list(&CutOptions {
                    rounding,
                    visible_only: *visible_only,
                });
                let t = crate::table::cut_table(&list, columns, *decimal);
                table(&mut prims, *rect, title.as_deref(), &t);
                "table"
            }
            ItemKind::Image { rect, path } => {
                // Contain-fit the image in its rectangle, keeping its aspect.
                let r = match model.textures().get(path) {
                    Some(img) if img.width > 0 && img.height > 0 => {
                        let aspect = f64::from(img.width) / f64::from(img.height);
                        let (mut iw, mut ih) = (rect[2], rect[2] / aspect);
                        if ih > rect[3] {
                            ih = rect[3];
                            iw = ih * aspect;
                        }
                        [
                            rect[0] + (rect[2] - iw) / 2.0,
                            rect[1] + (rect[3] - ih) / 2.0,
                            iw,
                            ih,
                        ]
                    }
                    _ => *rect,
                };
                prims.push(Prim::Image {
                    rect: r,
                    path: path.clone(),
                });
                "image"
            }
            ItemKind::Line {
                a,
                b,
                width,
                dashed,
            } => {
                prims.push(Prim::Line {
                    a: *a,
                    b: *b,
                    width: *width,
                    dash: dashed.then_some([2.0 * MM, 1.2 * MM]),
                    color: INK,
                });
                "line"
            }
            ItemKind::Rect { rect, width, fill } => {
                prims.push(Prim::Rect {
                    rect: *rect,
                    width: *width,
                    stroke: (*width > 0.0).then_some(INK),
                    fill: *fill,
                });
                "rect"
            }
        };
        let mut b = Bounds::default();
        if let Some(r) = it.kind.rect() {
            b.add([r[0], r[1]]);
            b.add([r[0] + r[2], r[1] + r[3]]);
        }
        for p in &prims[start..] {
            prim_bounds(p, &mut b);
        }
        items.push(ItemInfo {
            id: it.id,
            kind,
            bounds: b.rect(),
            text: text_out,
            points: points_out,
            detached,
            prims: [start, prims.len()],
        });
    }
    if sheet.setup.title.show {
        title_block(&mut prims, sheet, opts);
    }
    let mut hsh = StableHasher::default();
    hsh.tag("sheet-render");
    hsh.len(w);
    hsh.len(h);
    let json = serde_json::to_string(&prims).unwrap_or_default();
    hsh.tag(&json);
    for (_, d) in &drawings {
        hsh.tag(&d.hash);
    }
    Some(SheetRender {
        id,
        name: sheet.setup.name.clone(),
        size: [w, h],
        prims,
        items,
        viewports,
        drawings: drawings.into_iter().map(|(_, d)| d.hash).collect(),
        hash: format!("{:016x}", hsh.finish()),
    })
}

fn prim_bounds(p: &Prim, b: &mut Bounds) {
    match p {
        Prim::Line { a, b: e, .. } => {
            b.add(*a);
            b.add(*e);
        }
        Prim::Fill { rings, .. } => rings.iter().flatten().for_each(|q| b.add(*q)),
        Prim::Rect { rect, .. } | Prim::Image { rect, .. } => {
            b.add([rect[0], rect[1]]);
            b.add([rect[0] + rect[2], rect[1] + rect[3]]);
        }
        Prim::Text {
            at,
            text: t,
            size,
            anchor,
            angle,
            bold,
            max,
            ..
        } => {
            let w = max
                .unwrap_or(f64::INFINITY)
                .min(text_width(t, *size, *bold));
            let dx = match anchor {
                Anchor::Start => 0.0,
                Anchor::Middle => -w / 2.0,
                Anchor::End => -w,
            };
            let a = angle.to_radians();
            let (c, s) = (a.cos(), -a.sin());
            for (x, y) in [(dx, 0.0), (dx + w, 0.0), (dx, -size), (dx + w, -size)] {
                b.add([at[0] + x * c - y * s, at[1] + x * s + y * c]);
            }
        }
    }
}

/// A dimension's lines, ends and text (paper points `p`).
fn dimension(
    out: &mut Vec<Prim>,
    kind: DimKind,
    p: &[DVec2],
    offset: f64,
    style: DimStyle,
    words: &str,
) {
    let gap = 1.2 * MM;
    let over = 1.6 * MM;
    // Extension line from a measured point to the dimension line.
    let ext = |out: &mut Vec<Prim>, from: DVec2, to: DVec2| {
        let d = to - from;
        let len = d.length();
        if len > gap {
            let u = d / len;
            out.push(line(from + u * gap, to + u * over, PEN_THIN, INK));
        }
    };
    let linear = |out: &mut Vec<Prim>, a: DVec2, b: DVec2| {
        out.push(line(a, b, PEN_THIN, INK));
        let u = (b - a).normalize_or(DVec2::X);
        dim_end(out, a, -u, style);
        dim_end(out, b, u, style);
        out.push(text_along(a, b, words, 1.0 * MM));
    };
    match kind {
        DimKind::Horizontal => {
            let y = p[0].y - offset;
            let (a, b) = (DVec2::new(p[0].x, y), DVec2::new(p[1].x, y));
            ext(out, p[0], a);
            ext(out, p[1], b);
            linear(out, a, b);
        }
        DimKind::Vertical => {
            let x = p[0].x + offset;
            let (a, b) = (DVec2::new(x, p[0].y), DVec2::new(x, p[1].y));
            ext(out, p[0], a);
            ext(out, p[1], b);
            linear(out, a, b);
        }
        DimKind::Aligned => {
            let d = (p[1] - p[0]).normalize_or(DVec2::X);
            let n = DVec2::new(d.y, -d.x) * offset;
            let (a, b) = (p[0] + n, p[1] + n);
            ext(out, p[0], a);
            ext(out, p[1], b);
            linear(out, a, b);
        }
        DimKind::Angular => {
            let r = offset.abs().max(4.0 * MM);
            let (a0, a1) = ((p[1] - p[0]).to_angle(), (p[2] - p[0]).to_angle());
            let mut sweep = a1 - a0;
            while sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            }
            while sweep < -std::f64::consts::PI {
                sweep += std::f64::consts::TAU;
            }
            let at = |t: f64| p[0] + DVec2::from_angle(a0 + sweep * t) * r;
            let n = 24;
            for k in 0..n {
                out.push(line(
                    at(k as f64 / n as f64),
                    at((k + 1) as f64 / n as f64),
                    PEN_THIN,
                    INK,
                ));
            }
            let tangent = |t: f64| DVec2::from_angle(a0 + sweep * t).perp() * sweep.signum();
            dim_end(out, at(0.0), -tangent(0.0), style);
            dim_end(out, at(1.0), tangent(1.0), style);
            let mid = p[0] + DVec2::from_angle(a0 + sweep * 0.5) * (r + 3.0 * MM);
            out.push(Prim::Text {
                at: mid.to_array(),
                text: words.to_string(),
                size: DIM_TEXT,
                anchor: Anchor::Middle,
                angle: 0.0,
                bold: false,
                max: None,
                color: INK,
            });
        }
        DimKind::Radius | DimKind::Diameter => {
            let Some(c) = circumcentre(p[0], p[1], p[2]) else {
                return;
            };
            let d = (p[0] - c).normalize_or(DVec2::X);
            let r = c.distance(p[0]);
            let tip = c + d * r;
            let from = if kind == DimKind::Radius {
                c
            } else {
                c - d * r
            };
            let tail = tip + d * offset.abs().max(6.0 * MM);
            out.push(line(from, tail, PEN_THIN, INK));
            dim_end(out, tip, d, style);
            if kind == DimKind::Diameter {
                dim_end(out, from, -d, style);
            }
            out.push(text_along(tip, tail, words, 1.0 * MM));
        }
    }
}

/// A table in `rect`: a title row, a header row and the rows, as many as fit.
fn table(out: &mut Vec<Prim>, rect: [f64; 4], title: Option<&str>, t: &crate::table::Table) {
    let [x, y, w, h] = rect;
    let rows_wanted = t.rows.len() + 1 + usize::from(title.is_some()) + 1;
    let size = (h / rows_wanted as f64 / 1.7).clamp(1.4 * MM, 2.8 * MM);
    let row_h = size * 1.7;
    let total: f64 = t.weights.iter().sum::<f64>().max(1e-9);
    let mut col_x = vec![x];
    for wgt in &t.weights {
        col_x.push(col_x.last().unwrap() + w * wgt / total);
    }
    out.push(Prim::Rect {
        rect,
        width: PEN_THIN * 1.5,
        stroke: Some(INK),
        fill: None,
    });
    let mut at_y = y;
    if let Some(ti) = title {
        out.push(Prim::Text {
            at: [x + 1.5 * MM, at_y + row_h * 0.72],
            text: ti.to_string(),
            size: size * 1.1,
            anchor: Anchor::Start,
            angle: 0.0,
            bold: true,
            max: Some(w - 3.0 * MM),
            color: INK,
        });
        at_y += row_h;
        out.push(line(
            DVec2::new(x, at_y),
            DVec2::new(x + w, at_y),
            PEN_THIN,
            INK,
        ));
    }
    let cell = |out: &mut Vec<Prim>, k: usize, yy: f64, s: &str, bold: bool, color: [u8; 3]| {
        out.push(Prim::Text {
            at: [col_x[k] + 1.0 * MM, yy + row_h * 0.7],
            text: s.to_string(),
            size,
            anchor: Anchor::Start,
            angle: 0.0,
            bold,
            max: Some((col_x[k + 1] - col_x[k] - 2.0 * MM).max(0.0)),
            color,
        });
    };
    let header_top = at_y;
    for (k, hd) in t.header.iter().enumerate() {
        cell(out, k, at_y, hd, true, INK);
    }
    at_y += row_h;
    out.push(line(
        DVec2::new(x, at_y),
        DVec2::new(x + w, at_y),
        PEN_THIN,
        INK,
    ));
    let room = ((y + h - at_y) / row_h).floor().max(0.0) as usize;
    let shown = if t.rows.len() > room {
        room.saturating_sub(1)
    } else {
        t.rows.len()
    };
    for row in t.rows.iter().take(shown) {
        for (k, c) in row.iter().enumerate() {
            cell(out, k, at_y, c, false, INK);
        }
        at_y += row_h;
        if at_y < y + h - 1e-9 {
            out.push(line(
                DVec2::new(x, at_y),
                DVec2::new(x + w, at_y),
                PEN_THIN * 0.6,
                LIGHT,
            ));
        }
    }
    if shown < t.rows.len() {
        cell(
            out,
            0,
            at_y,
            &format!("... {} more parts", t.rows.len() - shown),
            false,
            GREY,
        );
    }
    for cx in &col_x[1..col_x.len() - 1] {
        out.push(line(
            DVec2::new(*cx, header_top),
            DVec2::new(*cx, y + h),
            PEN_THIN * 0.6,
            LIGHT,
        ));
    }
}

/// The title block in the bottom-right corner, inside the margins.
fn title_block(out: &mut Vec<Prim>, sheet: &Sheet, opts: &RenderOptions) {
    let s = &sheet.setup;
    let (w, h) = s.size();
    let [_, mr, mb, _] = s.margins;
    let metric = s.units == PaperUnits::Millimetres;
    let (bw, bh) = if metric {
        (95.0 * MM, 32.0 * MM)
    } else {
        (3.75 * IN, 1.25 * IN)
    };
    let (x, y) = (w - mr - bw, h - mb - bh);
    out.push(Prim::Rect {
        rect: [x, y, bw, bh],
        width: 0.35 * MM,
        stroke: Some(INK),
        fill: Some([255, 255, 255]),
    });
    let t = &s.title;
    let number = if t.sheet.is_empty() {
        if opts.count > 0 {
            format!("{} of {}", opts.number, opts.count)
        } else {
            String::new()
        }
    } else {
        t.sheet.clone()
    };
    let scale = if t.scale.is_empty() {
        scale_note(sheet)
    } else {
        t.scale.clone()
    };
    let title_h = bh * 0.34;
    let label = |out: &mut Vec<Prim>, at: DVec2, k: &str, v: &str, max: f64| {
        out.push(Prim::Text {
            at: [at.x, at.y + 2.0 * MM],
            text: k.to_uppercase(),
            size: 1.5 * MM,
            anchor: Anchor::Start,
            angle: 0.0,
            bold: false,
            max: Some(max),
            color: GREY,
        });
        out.push(Prim::Text {
            at: [at.x, at.y + 5.6 * MM],
            text: v.to_string(),
            size: 2.8 * MM,
            anchor: Anchor::Start,
            angle: 0.0,
            bold: false,
            max: Some(max),
            color: INK,
        });
    };
    label(out, DVec2::new(x + 1.5 * MM, y), "Title", "", bw - 3.0 * MM);
    out.push(Prim::Text {
        at: [x + 1.5 * MM, y + title_h - 1.6 * MM],
        text: if t.title.is_empty() {
            s.name.clone()
        } else {
            t.title.clone()
        },
        size: 4.6 * MM,
        anchor: Anchor::Start,
        angle: 0.0,
        bold: true,
        max: Some(bw - 3.0 * MM),
        color: INK,
    });
    out.push(line(
        DVec2::new(x, y + title_h),
        DVec2::new(x + bw, y + title_h),
        PEN_THIN,
        INK,
    ));
    let rows = [
        ("Project", t.project.as_str(), "Sheet", number.as_str()),
        ("Scale", scale.as_str(), "Date", t.date.as_str()),
        ("Drawn by", t.drawn_by.as_str(), "Rev", t.revision.as_str()),
    ];
    let row_h = (bh - title_h) / 3.0;
    let split = x + bw * 0.62;
    for (k, (a, av, b, bv)) in rows.iter().enumerate() {
        let ry = y + title_h + row_h * k as f64;
        if k > 0 {
            out.push(line(
                DVec2::new(x, ry),
                DVec2::new(x + bw, ry),
                PEN_THIN * 0.7,
                LIGHT,
            ));
        }
        label(
            out,
            DVec2::new(x + 1.5 * MM, ry - 0.6 * MM),
            a,
            av,
            split - x - 3.0 * MM,
        );
        label(
            out,
            DVec2::new(split + 1.5 * MM, ry - 0.6 * MM),
            b,
            bv,
            x + bw - split - 3.0 * MM,
        );
    }
    out.push(line(
        DVec2::new(split, y + title_h),
        DVec2::new(split, y + bh),
        PEN_THIN,
        INK,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_to_a_viewport() {
        let r = [0.0, 0.0, 1.0, 1.0];
        let (a, b) = clip_line(DVec2::new(-1.0, 0.5), DVec2::new(2.0, 0.5), r).unwrap();
        assert_eq!((a, b), (DVec2::new(0.0, 0.5), DVec2::new(1.0, 0.5)));
        assert!(clip_line(DVec2::new(-1.0, 2.0), DVec2::new(2.0, 2.0), r).is_none());
        let sq = [
            DVec2::new(-0.5, -0.5),
            DVec2::new(0.5, -0.5),
            DVec2::new(0.5, 0.5),
            DVec2::new(-0.5, 0.5),
        ];
        let c = clip_ring(&sq, r);
        assert!((sawhorse_tessellate::signed_area(&c) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn text_widths() {
        assert!((text_width("W", 1.0, false) - 0.944).abs() < 1e-12);
        assert!(text_width("72\"", 2.4 * MM, false) > 0.0);
    }
}
