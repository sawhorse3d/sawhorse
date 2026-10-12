//! Drawing sheets (lane 12, ADR-0010): scaled drawings on paper, the LayOut-shaped part of
//! a woodworker's model.
//!
//! A document holds zero or more [`Sheet`]s, in order. A sheet is a piece of paper (Letter,
//! Legal, Tabloid, A4, A3, A2 or a custom size, portrait or landscape) with margins, the
//! units its rulers and grid use, how its dimensions write lengths, a title block, and a
//! list of [`SheetItem`]s placed in paper coordinates (metres from the paper's top-left
//! corner, y down): viewports (a scene, a standard view or a camera, drawn by the export
//! crate's hidden-line drawing at a [`Scale`]), dimensions (attached to model points seen in
//! a viewport, measuring the model), labels with leaders, cut-list tables, images (an image
//! the model holds), lines and rectangles. Items keep ids for good (per sheet), drawn in
//! list order.
//!
//! Everything is plain data, changed only by ops (undoable, logged, replayed), validated on
//! every commit, saved in `.shz` (`docs/format.md`) and hashed only when a sheet exists, so
//! every older document keeps its hash. Scenes renamed carry their viewports along; a scene
//! deleted leaves its viewports on its camera; a viewport deleted takes its dimensions.

use std::collections::BTreeMap;

use crate::error::{KernelError, SheetError};
use crate::hash::StableHasher;
use crate::ids::{SheetId, from_public, to_public};
use crate::model::{Model, OpOutcome};
use crate::paper::{CameraSpec, CutColumn, DimKind, LengthFormat, Scale, StdView};

/// Most sheets a document holds.
pub const MAX_SHEETS: usize = 200;
/// Most items one sheet holds.
pub const MAX_ITEMS: usize = 2000;
/// Longest text on a sheet (a label, a title block field).
pub const MAX_TEXT: usize = 500;
/// Paper and everything on it stays within this many metres of the paper (2 m).
const REACH: f64 = 2.0;

const IN: f64 = 0.0254;
const MM: f64 = 0.001;

/// A paper size.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaperSize {
    Letter,
    Legal,
    Tabloid,
    A4,
    A3,
    A2,
    /// Width and height in metres (portrait: as given).
    Custom {
        width: f64,
        height: f64,
    },
}

impl PaperSize {
    /// Portrait width and height, metres.
    pub fn portrait(&self) -> (f64, f64) {
        match *self {
            PaperSize::Letter => (8.5 * IN, 11.0 * IN),
            PaperSize::Legal => (8.5 * IN, 14.0 * IN),
            PaperSize::Tabloid => (11.0 * IN, 17.0 * IN),
            PaperSize::A4 => (210.0 * MM, 297.0 * MM),
            PaperSize::A3 => (297.0 * MM, 420.0 * MM),
            PaperSize::A2 => (420.0 * MM, 594.0 * MM),
            PaperSize::Custom { width, height } => (width, height),
        }
    }

    pub fn label(&self) -> String {
        match *self {
            PaperSize::Letter => "Letter".into(),
            PaperSize::Legal => "Legal".into(),
            PaperSize::Tabloid => "Tabloid".into(),
            PaperSize::A4 => "A4".into(),
            PaperSize::A3 => "A3".into(),
            PaperSize::A2 => "A2".into(),
            PaperSize::Custom { .. } => "Custom".into(),
        }
    }
}

/// The units a sheet's rulers, grid and fields use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum PaperUnits {
    #[default]
    Inches,
    Millimetres,
}

/// The title block in the paper's bottom-right corner. Empty fields print nothing; the sheet
/// number and the scale fill themselves in when left empty ("2 of 3"; the viewports' scale,
/// or "As noted").
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct TitleBlock {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub show: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project: String,
    /// Empty: "n of N".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sheet: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
    /// Empty: the viewports' scale.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scale: String,
    /// Left blank by default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub drawn_by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub revision: String,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl TitleBlock {
    fn fields(&self) -> [&str; 7] {
        [
            &self.title,
            &self.project,
            &self.sheet,
            &self.date,
            &self.scale,
            &self.drawn_by,
            &self.revision,
        ]
    }
}

/// A sheet's setup: everything but its items.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SheetSetup {
    pub name: String,
    pub paper: PaperSize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub landscape: bool,
    /// Top, right, bottom, left, metres.
    #[serde(default = "default_margins")]
    pub margins: [f64; 4],
    #[serde(default)]
    pub units: PaperUnits,
    /// The snap grid on paper, metres (0: no snapping).
    #[serde(default = "default_grid")]
    pub grid: f64,
    /// How dimensions write lengths.
    #[serde(default)]
    pub format: LengthFormat,
    #[serde(default)]
    pub title: TitleBlock,
}

fn default_margins() -> [f64; 4] {
    [0.5 * IN; 4]
}

fn default_grid() -> f64 {
    IN / 8.0
}

impl SheetSetup {
    /// A Letter landscape sheet in inches, half-inch margins, a 1/8 in grid, fractions to
    /// 1/16 in, the title block on.
    pub fn named(name: &str) -> SheetSetup {
        SheetSetup {
            name: name.to_string(),
            paper: PaperSize::Letter,
            landscape: true,
            margins: default_margins(),
            units: PaperUnits::Inches,
            grid: default_grid(),
            format: LengthFormat::default(),
            title: TitleBlock {
                show: true,
                ..Default::default()
            },
        }
    }

    /// Width and height as laid out, metres.
    pub fn size(&self) -> (f64, f64) {
        let (w, h) = self.paper.portrait();
        if self.landscape { (h, w) } else { (w, h) }
    }

    fn check(&self) -> Result<(), SheetError> {
        let bad = |m: &str| Err(SheetError::Bad(m.to_string()));
        let n = self.name.trim();
        if n.is_empty() || n.chars().count() > 64 {
            return bad("a sheet's name must be 1 to 64 characters");
        }
        let (w, h) = self.paper.portrait();
        if !(w.is_finite()
            && h.is_finite()
            && (0.05..=REACH).contains(&w)
            && (0.05..=REACH).contains(&h))
        {
            return bad("paper must be 50 mm to 2 m on each side");
        }
        let (pw, ph) = self.size();
        let [t, r, b, l] = self.margins;
        if self.margins.iter().any(|m| !m.is_finite() || *m < 0.0)
            || l + r >= pw * 0.8
            || t + b >= ph * 0.8
        {
            return bad("the margins leave no room on the paper");
        }
        if !(self.grid.is_finite() && (0.0..=0.05).contains(&self.grid)) {
            return bad("the grid must be 0 to 50 mm");
        }
        self.format.check().map_err(|m| SheetError::Bad(m.into()))?;
        if self
            .title
            .fields()
            .iter()
            .any(|f| f.chars().count() > MAX_TEXT)
        {
            return bad("a title block field is longer than 500 characters");
        }
        Ok(())
    }

    fn hash_into(&self, h: &mut StableHasher) {
        h.tag(&self.name);
        let (w, hh) = self.paper.portrait();
        h.tag(&self.paper.label());
        h.len(w);
        h.len(hh);
        h.u64(
            u64::from(self.landscape) | (matches!(self.units, PaperUnits::Millimetres) as u64) << 1,
        );
        self.margins.iter().for_each(|m| h.len(*m));
        h.len(self.grid);
        self.format.hash_into(h);
        h.u64(u64::from(self.title.show));
        self.title.fields().iter().for_each(|f| h.tag(f));
    }
}

/// What a viewport shows.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ViewportSource {
    /// A scene: its camera, tags, style and section planes.
    Scene { name: String },
    /// A standard orthographic view of the model as it is (or the isometric one, NTS).
    Standard { view: StdView },
    /// A camera kept with the viewport (the live camera when it was placed).
    Camera { camera: CameraSpec },
}

/// How a dimension ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum DimStyle {
    #[default]
    Arrows,
    /// Architectural slashes.
    Ticks,
    Dots,
}

/// Where a label's leader points.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum LeaderEnd {
    /// A point on the paper.
    Paper { at: [f64; 2] },
    /// A model point seen in a viewport: the leader follows it when the viewport changes.
    Model { viewport: u64, point: [f64; 3] },
}

/// What an item is (paper coordinates: metres from the paper's top-left, y down; a rect is
/// `[x, y, width, height]`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemKind {
    Viewport {
        rect: [f64; 4],
        source: ViewportSource,
        scale: Scale,
        /// The drawing point (view-plane metres) at the viewport's centre; absent: the
        /// drawing is centred.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        centre: Option<[f64; 2]>,
        /// Hidden edges dashed.
        #[serde(default, skip_serializing_if = "is_false")]
        hidden_lines: bool,
        /// A caption under it ("FRONT 1:8").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        /// Draw its frame.
        #[serde(default, skip_serializing_if = "is_false")]
        border: bool,
    },
    Dimension {
        /// The viewport item whose model points it measures.
        viewport: u64,
        dim: DimKind,
        /// Model points (two, or three for angles and arcs).
        points: Vec<[f64; 3]>,
        /// How far the dimension line stands off its points, paper metres (sign: side).
        offset: f64,
        #[serde(default)]
        style: DimStyle,
        /// Text in place of the measured value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Label {
        at: [f64; 2],
        text: String,
        /// Text height, paper metres.
        size: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leader: Option<LeaderEnd>,
    },
    Table {
        rect: [f64; 4],
        columns: Vec<CutColumn>,
        #[serde(default, skip_serializing_if = "is_false")]
        decimal: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        visible_only: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Image {
        rect: [f64; 4],
        /// An image the model holds (`assets/...`, added with `add_texture`).
        path: String,
    },
    Line {
        a: [f64; 2],
        b: [f64; 2],
        /// Pen width, paper metres.
        width: f64,
        #[serde(default, skip_serializing_if = "is_false")]
        dashed: bool,
    },
    Rect {
        rect: [f64; 4],
        width: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<[u8; 3]>,
    },
}

impl ItemKind {
    /// The item's rect, for those that have one.
    pub fn rect(&self) -> Option<[f64; 4]> {
        match self {
            ItemKind::Viewport { rect, .. }
            | ItemKind::Table { rect, .. }
            | ItemKind::Image { rect, .. }
            | ItemKind::Rect { rect, .. } => Some(*rect),
            _ => None,
        }
    }

    /// The same item moved by `d` on paper (model points stay where they are).
    pub fn moved(&self, d: [f64; 2]) -> ItemKind {
        let mv = |p: [f64; 2]| [p[0] + d[0], p[1] + d[1]];
        let mr = |r: [f64; 4]| [r[0] + d[0], r[1] + d[1], r[2], r[3]];
        let mut out = self.clone();
        match &mut out {
            ItemKind::Viewport { rect, .. }
            | ItemKind::Table { rect, .. }
            | ItemKind::Image { rect, .. }
            | ItemKind::Rect { rect, .. } => *rect = mr(*rect),
            ItemKind::Label { at, leader, .. } => {
                *at = mv(*at);
                if let Some(LeaderEnd::Paper { at }) = leader {
                    *at = mv(*at);
                }
            }
            ItemKind::Line { a, b, .. } => {
                *a = mv(*a);
                *b = mv(*b);
            }
            ItemKind::Dimension { .. } => {}
        }
        out
    }

    fn hash_into(&self, h: &mut StableHasher) {
        let rect = |h: &mut StableHasher, r: &[f64; 4]| r.iter().for_each(|v| h.len(*v));
        let p2 = |h: &mut StableHasher, p: &[f64; 2]| p.iter().for_each(|v| h.len(*v));
        match self {
            ItemKind::Viewport {
                rect: r,
                source,
                scale,
                centre,
                hidden_lines,
                caption,
                border,
            } => {
                h.tag("viewport");
                rect(h, r);
                match source {
                    ViewportSource::Scene { name } => h.tag(&format!("scene:{name}")),
                    ViewportSource::Standard { view } => h.tag(&format!("std:{view:?}")),
                    ViewportSource::Camera { camera } => {
                        h.tag("camera");
                        camera
                            .eye
                            .iter()
                            .chain(&camera.target)
                            .chain(&camera.up)
                            .for_each(|v| h.len(*v));
                        h.u64(u64::from(camera.parallel));
                        h.len(camera.fov * 1e-6);
                    }
                }
                scale.hash_into(h);
                match centre {
                    Some(c) => p2(h, c),
                    None => h.u64(u64::MAX),
                }
                h.u64(u64::from(*hidden_lines) | u64::from(*border) << 1);
                h.tag(caption.as_deref().unwrap_or("\u{0}"));
            }
            ItemKind::Dimension {
                viewport,
                dim,
                points,
                offset,
                style,
                text,
            } => {
                h.tag("dimension");
                h.u64(*viewport);
                h.tag(&format!("{dim:?}{style:?}"));
                h.usize(points.len());
                points.iter().flatten().for_each(|v| h.len(*v));
                h.len(*offset);
                h.tag(text.as_deref().unwrap_or("\u{0}"));
            }
            ItemKind::Label {
                at,
                text,
                size,
                leader,
            } => {
                h.tag("label");
                p2(h, at);
                h.tag(text);
                h.len(*size);
                match leader {
                    None => h.u64(0),
                    Some(LeaderEnd::Paper { at }) => {
                        h.u64(1);
                        p2(h, at);
                    }
                    Some(LeaderEnd::Model { viewport, point }) => {
                        h.u64(2);
                        h.u64(*viewport);
                        point.iter().for_each(|v| h.len(*v));
                    }
                }
            }
            ItemKind::Table {
                rect: r,
                columns,
                decimal,
                visible_only,
                title,
            } => {
                h.tag("table");
                rect(h, r);
                h.tag(&format!("{columns:?}"));
                h.u64(u64::from(*decimal) | u64::from(*visible_only) << 1);
                h.tag(title.as_deref().unwrap_or("\u{0}"));
            }
            ItemKind::Image { rect: r, path } => {
                h.tag("image");
                rect(h, r);
                h.tag(path);
            }
            ItemKind::Line {
                a,
                b,
                width,
                dashed,
            } => {
                h.tag("line");
                p2(h, a);
                p2(h, b);
                h.len(*width);
                h.u64(u64::from(*dashed));
            }
            ItemKind::Rect {
                rect: r,
                width,
                fill,
            } => {
                h.tag("rect");
                rect(h, r);
                h.len(*width);
                match fill {
                    Some(c) => h.bytes(c),
                    None => h.u64(u64::MAX),
                }
            }
        }
    }
}

/// An item on a sheet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SheetItem {
    /// Its id on the sheet (from 1, never reused).
    pub id: u64,
    #[serde(flatten)]
    pub kind: ItemKind,
}

/// A sheet: its setup and its items, drawn in order.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Sheet {
    #[serde(flatten)]
    pub setup: SheetSetup,
    #[serde(default)]
    pub items: Vec<SheetItem>,
    /// The id the next item gets.
    #[serde(default = "first_id")]
    pub next_item: u64,
}

fn first_id() -> u64 {
    1
}

impl Sheet {
    pub fn item(&self, id: u64) -> Option<&SheetItem> {
        self.items.iter().find(|i| i.id == id)
    }

    fn item_index(&self, sheet: u64, id: u64) -> Result<usize, SheetError> {
        self.items
            .iter()
            .position(|i| i.id == id)
            .ok_or(SheetError::NoSuchItem(sheet, id))
    }

    fn hash_into(&self, h: &mut StableHasher) {
        self.setup.hash_into(h);
        h.u64(self.next_item);
        h.usize(self.items.len());
        for it in &self.items {
            h.u64(it.id);
            it.kind.hash_into(h);
        }
    }
}

fn finite2(p: &[f64; 2]) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= REACH)
}

fn finite_rect(r: &[f64; 4]) -> bool {
    r.iter().all(|v| v.is_finite() && v.abs() <= REACH) && r[2] >= 0.002 && r[3] >= 0.002
}

impl Model {
    /// The sheets in order, with their public ids.
    pub fn sheets(&self) -> Vec<(u64, &Sheet)> {
        self.sheet_order
            .iter()
            .map(|&k| (to_public(k), &self.sheets[k]))
            .collect()
    }

    pub fn sheet(&self, id: u64) -> Option<&Sheet> {
        self.sheets.get(from_public::<SheetId>(id))
    }

    fn sheet_key(&self, id: u64) -> Result<SheetId, SheetError> {
        let k = from_public::<SheetId>(id);
        if self.sheets.contains_key(k) {
            Ok(k)
        } else {
            Err(SheetError::NoSuchSheet(id))
        }
    }

    pub(crate) fn hash_sheets(&self, h: &mut StableHasher) {
        h.tag("sheets");
        h.usize(self.sheet_order.len());
        for (id, s) in self.sheets() {
            h.u64(id);
            s.hash_into(h);
        }
    }

    /// Whether a viewport may show `source` at `scale`: views that are not square to the
    /// model (isometric, perspective) are not to scale.
    fn check_viewport(&self, source: &ViewportSource, scale: &Scale) -> Result<(), SheetError> {
        let bad = |m: &str| Err(SheetError::Bad(m.to_string()));
        scale.check().map_err(|m| SheetError::Bad(m.into()))?;
        let to_scale = match source {
            ViewportSource::Standard { view } => view.to_scale(),
            ViewportSource::Camera { camera } => {
                let ok = camera
                    .eye
                    .iter()
                    .chain(&camera.target)
                    .chain(&camera.up)
                    .all(|v| v.is_finite() && v.abs() <= crate::tolerance::MAX_COORD)
                    && camera.fov.is_finite();
                if !ok {
                    return bad("a viewport's camera must be finite");
                }
                camera.parallel
            }
            ViewportSource::Scene { name } => match self.scenes.iter().find(|s| s.name == *name) {
                Some(s) => s.parallel,
                None => return bad(&format!("no scene named {name:?}")),
            },
        };
        if !to_scale && *scale != Scale::Nts {
            return bad("an isometric or perspective view is not to scale: give it the scale NTS");
        }
        Ok(())
    }

    /// An item is valid on `sheet` (the one it is going on, without itself at `skip`).
    fn check_item(
        &self,
        sheet: &Sheet,
        kind: &ItemKind,
        skip: Option<u64>,
    ) -> Result<(), SheetError> {
        let bad = |m: &str| Err(SheetError::Bad(m.to_string()));
        let text_ok = |t: &str| t.chars().count() <= MAX_TEXT;
        let viewport = |id: u64| {
            sheet.items.iter().any(|i| {
                i.id == id && Some(id) != skip && matches!(i.kind, ItemKind::Viewport { .. })
            })
        };
        match kind {
            ItemKind::Viewport {
                rect,
                source,
                scale,
                centre,
                caption,
                ..
            } => {
                if !finite_rect(rect)
                    || centre.is_some_and(|c| {
                        !c.iter()
                            .all(|v| v.is_finite() && v.abs() <= crate::tolerance::MAX_COORD)
                    })
                {
                    return bad("a viewport needs a finite place on the paper");
                }
                if caption.as_deref().is_some_and(|c| !text_ok(c)) {
                    return bad("a caption is longer than 500 characters");
                }
                self.check_viewport(source, scale)
            }
            ItemKind::Dimension {
                viewport: v,
                dim,
                points,
                offset,
                text,
                ..
            } => {
                if !viewport(*v) {
                    return bad("a dimension must be attached to a viewport on the same sheet");
                }
                if points.len() != dim.points()
                    || !points
                        .iter()
                        .flatten()
                        .all(|x| x.is_finite() && x.abs() <= crate::tolerance::MAX_COORD)
                {
                    return bad(
                        "a dimension needs its model points (two, or three for angles and arcs)",
                    );
                }
                if !(offset.is_finite() && offset.abs() <= REACH)
                    || text.as_deref().is_some_and(|t| !text_ok(t))
                {
                    return bad("a dimension's offset or text is out of range");
                }
                Ok(())
            }
            ItemKind::Label {
                at,
                text,
                size,
                leader,
            } => {
                if !finite2(at)
                    || text.trim().is_empty()
                    || !text_ok(text)
                    || !(size.is_finite() && (0.0005..=0.1).contains(size))
                {
                    return bad(
                        "a label needs a place, 1 to 500 characters and a text size of 0.5 to 100 mm",
                    );
                }
                match leader {
                    Some(LeaderEnd::Paper { at }) if !finite2(at) => {
                        bad("a leader must end on the paper")
                    }
                    Some(LeaderEnd::Model { viewport: v, point })
                        if !viewport(*v) || !point.iter().all(|x| x.is_finite()) =>
                    {
                        bad("a leader must point at a model point in a viewport on the same sheet")
                    }
                    _ => Ok(()),
                }
            }
            ItemKind::Table {
                rect,
                columns,
                title,
                ..
            } => {
                if !finite_rect(rect) || columns.is_empty() || columns.len() > 12 {
                    return bad("a table needs a place and 1 to 12 columns");
                }
                if title.as_deref().is_some_and(|t| !text_ok(t)) {
                    return bad("a table's title is longer than 500 characters");
                }
                Ok(())
            }
            ItemKind::Image { rect, path } => {
                if !finite_rect(rect) {
                    return bad("an image needs a place on the paper");
                }
                if !self.textures.contains_key(path) {
                    return bad(&format!("no image {path:?} in the model (add it first)"));
                }
                Ok(())
            }
            ItemKind::Line { a, b, width, .. } => {
                if !finite2(a)
                    || !finite2(b)
                    || !(width.is_finite() && (0.0..=0.01).contains(width))
                {
                    return bad("a line needs two points on the paper and a width up to 10 mm");
                }
                Ok(())
            }
            ItemKind::Rect { rect, width, .. } => {
                if !finite_rect(rect) || !(width.is_finite() && (0.0..=0.01).contains(width)) {
                    return bad("a rectangle needs a place and a pen width up to 10 mm");
                }
                Ok(())
            }
        }
    }

    // ---- ops -------------------------------------------------------------------------

    /// [`crate::model::Op::AddSheet`]: a new sheet at the end.
    pub(crate) fn add_sheet(&mut self, setup: &SheetSetup) -> Result<OpOutcome, KernelError> {
        if self.sheet_order.len() >= MAX_SHEETS {
            return Err(SheetError::TooManySheets.into());
        }
        setup.check()?;
        let mut setup = setup.clone();
        setup.name = setup.name.trim().to_string();
        let k = self.sheets.insert(Sheet {
            setup,
            items: Vec::new(),
            next_item: 1,
        });
        self.sheet_order.push(k);
        Ok(OpOutcome::SheetAdded {
            sheet: to_public(k),
        })
    }

    pub(crate) fn set_sheet(
        &mut self,
        id: u64,
        setup: &SheetSetup,
    ) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        setup.check()?;
        let mut setup = setup.clone();
        setup.name = setup.name.trim().to_string();
        self.sheets[k].setup = setup;
        Ok(OpOutcome::SheetSet)
    }

    pub(crate) fn delete_sheet(&mut self, id: u64) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        self.sheets.remove(k);
        self.sheet_order.retain(|&x| x != k);
        Ok(OpOutcome::SheetSet)
    }

    /// Put sheet `id` at position `to` (clamped to the end).
    pub(crate) fn move_sheet(&mut self, id: u64, to: u32) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        self.sheet_order.retain(|&x| x != k);
        let at = (to as usize).min(self.sheet_order.len());
        self.sheet_order.insert(at, k);
        Ok(OpOutcome::SheetSet)
    }

    pub(crate) fn add_sheet_item(
        &mut self,
        id: u64,
        kind: &ItemKind,
    ) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        if self.sheets[k].items.len() >= MAX_ITEMS {
            return Err(SheetError::TooManyItems.into());
        }
        self.check_item(&self.sheets[k], kind, None)?;
        let sheet = &mut self.sheets[k];
        let item = sheet.next_item;
        sheet.next_item += 1;
        sheet.items.push(SheetItem {
            id: item,
            kind: kind.clone(),
        });
        Ok(OpOutcome::SheetItemAdded { sheet: id, item })
    }

    /// Replace an item (moved, resized, changed); it keeps its id and its place in the order.
    pub(crate) fn set_sheet_item(
        &mut self,
        id: u64,
        item: u64,
        kind: &ItemKind,
    ) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        let at = self.sheets[k].item_index(id, item)?;
        // A viewport stays a viewport (dimensions hang on it); other kinds may not turn into one
        // under an id a dimension uses.
        let was_viewport = matches!(self.sheets[k].items[at].kind, ItemKind::Viewport { .. });
        let is_viewport = matches!(kind, ItemKind::Viewport { .. });
        if was_viewport != is_viewport {
            return Err(SheetError::Bad(
                "an item keeps its kind: delete it and add another".into(),
            )
            .into());
        }
        self.check_item(&self.sheets[k], kind, None)?;
        self.sheets[k].items[at].kind = kind.clone();
        Ok(OpOutcome::SheetSet)
    }

    /// Move items by `by` on the paper (dimensions follow their viewports by themselves).
    pub(crate) fn move_sheet_items(
        &mut self,
        id: u64,
        items: &[u64],
        by: [f64; 2],
    ) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        if items.is_empty() || !by.iter().all(|v| v.is_finite() && v.abs() <= REACH) {
            return Err(SheetError::Bad("pick items to move, and a move within 2 m".into()).into());
        }
        let mut next = self.sheets[k].clone();
        for &item in items {
            let at = next.item_index(id, item)?;
            next.items[at].kind = next.items[at].kind.moved(by);
        }
        for it in &next.items {
            if items.contains(&it.id) {
                self.check_item(&next, &it.kind, None)?;
            }
        }
        self.sheets[k] = next;
        Ok(OpOutcome::SheetSet)
    }

    /// Delete items; dimensions and leaders on a deleted viewport go too.
    pub(crate) fn delete_sheet_items(
        &mut self,
        id: u64,
        items: &[u64],
    ) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        if items.is_empty() {
            return Err(SheetError::Bad("pick items to delete".into()).into());
        }
        for &item in items {
            self.sheets[k].item_index(id, item)?;
        }
        let sheet = &mut self.sheets[k];
        let gone: Vec<u64> = sheet
            .items
            .iter()
            .filter(|i| items.contains(&i.id) && matches!(i.kind, ItemKind::Viewport { .. }))
            .map(|i| i.id)
            .collect();
        sheet.items.retain(|i| !items.contains(&i.id));
        sheet.items.retain(
            |i| !matches!(&i.kind, ItemKind::Dimension { viewport, .. } if gone.contains(viewport)),
        );
        for i in &mut sheet.items {
            if let ItemKind::Label { leader, .. } = &mut i.kind
                && matches!(leader, Some(LeaderEnd::Model { viewport, .. }) if gone.contains(viewport))
            {
                *leader = None;
            }
        }
        Ok(OpOutcome::SheetSet)
    }

    /// Put item `item` at position `to` in the drawing order (clamped).
    pub(crate) fn order_sheet_item(
        &mut self,
        id: u64,
        item: u64,
        to: u32,
    ) -> Result<OpOutcome, KernelError> {
        let k = self.sheet_key(id)?;
        let at = self.sheets[k].item_index(id, item)?;
        let sheet = &mut self.sheets[k];
        let it = sheet.items.remove(at);
        let to = (to as usize).min(sheet.items.len());
        sheet.items.insert(to, it);
        Ok(OpOutcome::SheetSet)
    }

    /// A scene updated: viewports showing it follow a rename, and become not to scale when
    /// its camera is a perspective one now.
    pub(crate) fn sheets_scene_updated(&mut self, from: &str, scene: &crate::model::SceneDef) {
        for s in self.sheets.values_mut() {
            for i in &mut s.items {
                if let ItemKind::Viewport {
                    source: ViewportSource::Scene { name },
                    scale,
                    ..
                } = &mut i.kind
                    && name == from
                {
                    *name = scene.name.clone();
                    if !scene.parallel {
                        *scale = Scale::Nts;
                    }
                }
            }
        }
    }

    /// A scene deleted: viewports showing it keep its camera (and a perspective one is no
    /// longer to scale).
    pub(crate) fn sheets_scene_deleted(&mut self, scene: &crate::model::SceneDef) {
        let camera = CameraSpec {
            eye: scene.eye,
            target: scene.target,
            up: scene.up,
            parallel: scene.parallel,
            fov: scene.fov,
        };
        for s in self.sheets.values_mut() {
            for i in &mut s.items {
                if let ItemKind::Viewport { source, scale, .. } = &mut i.kind
                    && matches!(source, ViewportSource::Scene { name } if *name == scene.name)
                {
                    *source = ViewportSource::Camera { camera };
                    if !camera.parallel {
                        *scale = Scale::Nts;
                    }
                }
            }
        }
    }

    /// The sheets' rules (the validator): every item valid, ids unique and below the counter.
    pub(crate) fn check_sheets(&self) -> Result<(), String> {
        if self.sheet_order.len() != self.sheets.len()
            || self
                .sheet_order
                .iter()
                .any(|k| !self.sheets.contains_key(*k))
        {
            return Err("the sheet order does not list every sheet once".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for k in &self.sheet_order {
            if !seen.insert(*k) {
                return Err("a sheet is listed twice".into());
            }
        }
        for (id, s) in self.sheets() {
            s.setup.check().map_err(|e| format!("sheet {id}: {e}"))?;
            let mut ids: BTreeMap<u64, ()> = BTreeMap::new();
            for it in &s.items {
                if it.id == 0 || it.id >= s.next_item || ids.insert(it.id, ()).is_some() {
                    return Err(format!(
                        "sheet {id}: item ids are not unique or not below the counter"
                    ));
                }
                self.check_item(s, &it.kind, None)
                    .map_err(|e| format!("sheet {id} item {}: {e}", it.id))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_sizes() {
        let s = SheetSetup::named("A");
        let (w, h) = s.size();
        assert!(
            (w - 11.0 * IN).abs() < 1e-12 && (h - 8.5 * IN).abs() < 1e-12,
            "Letter landscape"
        );
        assert_eq!(PaperSize::A3.portrait(), (0.297, 0.42));
        assert!(s.check().is_ok());
        let mut bad = s.clone();
        bad.margins = [0.2; 4];
        assert!(bad.check().is_err());
        let mut bad = s.clone();
        bad.paper = PaperSize::Custom {
            width: 0.01,
            height: 0.3,
        };
        assert!(bad.check().is_err());
        let mut bad = s;
        bad.name = " ".into();
        assert!(bad.check().is_err());
    }

    #[test]
    fn items_move_on_paper_and_keep_their_model_points() {
        let d = ItemKind::Dimension {
            viewport: 1,
            dim: DimKind::Horizontal,
            points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            offset: 0.01,
            style: DimStyle::Ticks,
            text: None,
        };
        assert_eq!(d.moved([0.1, 0.1]), d);
        let l = ItemKind::Label {
            at: [0.1, 0.1],
            text: "Dado".into(),
            size: 0.003,
            leader: Some(LeaderEnd::Paper { at: [0.2, 0.2] }),
        };
        let ItemKind::Label { at, leader, .. } = l.moved([0.01, -0.01]) else {
            panic!()
        };
        let near =
            |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < 1e-15 && (p[1] - q[1]).abs() < 1e-15;
        assert!(near(at, [0.11, 0.09]), "{at:?}");
        let Some(LeaderEnd::Paper { at: end }) = leader else {
            panic!()
        };
        assert!(near(end, [0.21, 0.19]), "{end:?}");
    }
}
