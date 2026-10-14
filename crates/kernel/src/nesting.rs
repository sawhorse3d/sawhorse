//! Nesting (lane 9): the cut list's parts laid out on real stock, with the cuts that free
//! them, so a woodworker knows how many sheets and boards to buy and how to cut them.
//!
//! **Parts** come from the cut list ([`Model::cut_list`]): every counted piece of every row,
//! at its exact (unrounded) width and length, with its grain and whether it is sheet goods
//! or solid lumber. Each piece gets a short tag: its row number in the cut list, and a
//! letter when the row has more than one piece ("2", "3a", "3b").
//!
//! **Which stock.** A row whose stock text names a stock in the library uses it. Otherwise
//! the material's default stock (the library's per-material defaults) when it is of the
//! right kind and thickness. Otherwise the choice is automatic: a sheet part comes from the
//! sheet within 1/16 in of its thickness that it fits, the closest thickness first, then
//! library order; solid lumber of one material and thickness class (the thinnest board
//! thickness that covers it) comes from one board size, the narrowest as wide as its widest
//! part (then the longest), so a narrow part rides on the offcuts of the wide ones. A part
//! no stock covers is reported, never dropped: no stock of its kind and thickness, too thick
//! or thin for the stock it names, larger than the stock, or fitting only with its grain
//! across the stock's.
//!
//! **Layout.** Stock coordinates: x along the stock's length (its grain), y across its
//! width, from a corner; the usable area is inside the trim (every edge of a sheet, the two
//! ends of a board). A part's grain must run along the stock's grain, so it may turn 90
//! degrees only when the part or the stock has no grain. Every cut is `kerf` wide.
//!
//! - **Sheets, guillotine** (the default): free rectangles split by straight cuts from edge
//!   to edge, best area fit, each part on the first sheet it fits. Four variants (sorted by
//!   area or by longest side; splits that keep the largest offcut, or rips first) are run
//!   and the one with the fewest sheets wins (then the emptiest last sheet). Every layout is
//!   cuttable on a table saw.
//! - **Sheets, maxrects**: maximal free rectangles, best short side fit, every part grown
//!   by the kerf inside a bin grown by the kerf; tighter, but not always guillotine.
//! - **Boards**: one-dimensional cutting on the lengths (the part's width must fit the
//!   board's): first-fit-decreasing, and a lookahead that fills each board in turn with the
//!   subset of the remaining parts that leaves the least waste (a bounded search); the one
//!   with fewer boards wins.
//!
//! **Cuts.** Each piece's cut sequence is found from its layout: trim cuts first, then
//! recursively the first straight cut across the current offcut that crosses no part (rips
//! along a sheet's longer side first; crosscuts first on a board), lower side first. A
//! guillotine layout always comes apart this way; a maxrects layout that does not is
//! flagged (`guillotine: false`) with the cuts that were found.
//!
//! **Numbers.** Areas and efficiency come from the exact sizes. Only `+ - * /`, `min`, `max`
//! and comparisons are used, nothing is random, every tie is broken by input order: the
//! same model gives the same layout, natively and in WASM, and [`Nesting::hash`] says so.

use std::collections::BTreeMap;

use crate::cutlist::{CutList, CutOptions, CutRow, Grain};
use crate::hash::StableHasher;
use crate::model::Model;
use crate::nodes::Item;
use crate::stock::{
    NestAlgorithm, NestSettings, SHEET_THICKNESS_SLACK, STOCK_EPSILON, StockDef, StockGrain,
    StockKind, StockLibrary,
};

/// Geometric slack in fits and gaps (1 nm): floating-point noise only.
pub const EPS: f64 = 1e-9;
/// Nodes the board lookahead may visit for one board, and in all.
const LOOKAHEAD_PER_BOARD: usize = 20_000;
const LOOKAHEAD_TOTAL: usize = 400_000;

/// What the nesting is asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NestOptions {
    /// Nest only the drawn pieces (as the cut list's "Visible only").
    #[serde(default)]
    pub visible_only: bool,
    /// Use this algorithm instead of the document's (comparisons and tests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<NestAlgorithm>,
}

/// Which way a cut runs on the stock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CutDir {
    /// Along the length (x): the kerf spans `at..at + kerf` across the width (y).
    Rip,
    /// Across (y): the kerf spans `at..at + kerf` along the length (x).
    Crosscut,
}

/// One cut, in the order it is made.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Cut {
    /// 1, 2, 3...
    pub n: u32,
    pub dir: CutDir,
    /// Where the kerf starts (y for a rip, x for a crosscut), metres from the stock's corner.
    pub at: f64,
    /// The cut's extent along its own direction (x for a rip, y for a crosscut).
    pub from: f64,
    pub to: f64,
    /// What a fence is set to: the width (rip) or length (crosscut) of the piece the cut
    /// leaves on its near side; for a trim cut, the trim.
    pub size: f64,
    /// Taking off the trim allowance (before any part is cut).
    pub trim: bool,
}

/// A piece placed on stock.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Placement {
    /// Its row in the cut list (0-based) and which piece of the row (1-based).
    pub row: usize,
    pub copy: usize,
    /// "3" or "3b": the row number and, for rows of more than one piece, a letter.
    pub tag: String,
    pub name: String,
    /// What selecting it selects in the model.
    pub item: Item,
    /// Its rectangle on the stock (metres; x along the stock's length).
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    /// Its length runs across the stock (turned 90 degrees).
    pub turned: bool,
    pub grain: Grain,
    /// Thickness, width, length (exact).
    pub size: [f64; 3],
    /// Lane 15: its part is not a solid; the rectangle is the box around its faces.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub not_solid: bool,
}

/// One sheet or board and what is cut from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StockPiece {
    /// 1-based, over the whole nesting.
    pub index: usize,
    /// Which group (stock and material) it belongs to, and its number in that group.
    pub group: usize,
    pub seq: usize,
    pub count: usize,
    pub stock: String,
    pub kind: StockKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nominal: Option<String>,
    pub material: Option<String>,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    pub grain: StockGrain,
    /// The trim taken off each edge (each end of a board): the setting, or a kerf when the
    /// setting is narrower than one.
    pub trim: f64,
    pub kerf: f64,
    /// The usable area inside the trim: x0, y0, x1, y1.
    pub usable: [f64; 4],
    pub placements: Vec<Placement>,
    pub cuts: Vec<Cut>,
    /// The cuts free every part (always, for the guillotine algorithm and boards).
    pub guillotine: bool,
    /// Parts, stock and waste, m² (exact sizes); efficiency = parts / stock.
    pub parts_area: f64,
    pub stock_area: f64,
    pub waste_area: f64,
    pub efficiency: f64,
}

/// Why a piece is not on any stock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnplacedReason {
    /// No stock of its kind (sheet or board) covers its thickness.
    NoStock,
    /// The stock it names is too thin (or, for a sheet, not its thickness).
    Thickness,
    /// Larger than the stock, inside the trim.
    TooLarge,
    /// It would fit turned 90 degrees, but its grain must run along the stock's.
    Grain,
    /// Lane 15 fixes: a flat face with no thickness ([`crate::cutlist::Uncounted::Flat`]).
    Flat,
    /// Lane 15 fixes: open faces thicker than any board, sized as a whole
    /// ([`crate::cutlist::Uncounted::NotABoard`]): split it into its boards first.
    NotABoard,
    /// Lane 20: a whole piece drawn as one block, thicker than any board and over 12 in
    /// both ways ([`crate::cutlist::Uncounted::WholePiece`]).
    WholePiece,
    /// Lane 20: a solid thicker than any board, more than 30% of its box empty
    /// ([`crate::cutlist::Uncounted::Hollow`]).
    Hollow,
    /// Lane 20: lumber wider than 4 ft and than every board
    /// ([`crate::cutlist::Uncounted::TooWide`]).
    TooWide,
}

impl From<crate::cutlist::Uncounted> for UnplacedReason {
    /// The nesting's word for a row the cut list counts nowhere: the same reason.
    fn from(u: crate::cutlist::Uncounted) -> UnplacedReason {
        use crate::cutlist::Uncounted as U;
        match u {
            U::Flat => UnplacedReason::Flat,
            U::NotABoard => UnplacedReason::NotABoard,
            U::WholePiece => UnplacedReason::WholePiece,
            U::Hollow => UnplacedReason::Hollow,
            U::TooWide => UnplacedReason::TooWide,
        }
    }
}

impl UnplacedReason {
    /// A row counted nowhere by the cut list, reported with its reason (never stocked).
    pub fn is_uncounted(self) -> bool {
        matches!(
            self,
            UnplacedReason::Flat
                | UnplacedReason::NotABoard
                | UnplacedReason::WholePiece
                | UnplacedReason::Hollow
                | UnplacedReason::TooWide
        )
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Unplaced {
    pub row: usize,
    pub copy: usize,
    pub tag: String,
    pub name: String,
    pub item: Item,
    pub size: [f64; 3],
    pub material: Option<String>,
    /// The stock it was meant for, when there was one.
    pub stock: Option<String>,
    pub sheet: bool,
    pub reason: UnplacedReason,
}

/// One stock and material: how many pieces of it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NestGroup {
    pub stock: String,
    pub kind: StockKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nominal: Option<String>,
    pub material: Option<String>,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    pub count: usize,
    pub parts: usize,
    pub parts_area: f64,
    pub stock_area: f64,
    pub efficiency: f64,
    /// Boards: board feet bought (nominal thickness x width x length); 0 for sheets.
    pub board_feet: f64,
}

/// Boards of one nominal thickness.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NominalTotal {
    pub nominal: String,
    pub count: usize,
    pub board_feet: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NestTotals {
    pub sheets: usize,
    pub boards: usize,
    /// Pieces asked for, placed, and not placed (`parts_in = placed + unplaced`).
    pub parts_in: usize,
    pub placed: usize,
    pub unplaced: usize,
    pub parts_area: f64,
    pub stock_area: f64,
    /// Overall yield: parts area / stock area (0 with no stock).
    pub efficiency: f64,
    pub groups: Vec<NestGroup>,
    pub by_nominal: Vec<NominalTotal>,
}

/// The nesting ([`Model::nest`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Nesting {
    pub algorithm: NestAlgorithm,
    pub settings: NestSettings,
    pub visible_only: bool,
    /// The library it was worked out with (the Stock dialog edits it).
    pub library: StockLibrary,
    pub pieces: Vec<StockPiece>,
    pub unplaced: Vec<Unplaced>,
    pub totals: NestTotals,
    /// Deterministic hash of the layout (hex).
    pub hash: String,
}

// ---- parts ----------------------------------------------------------------------------------

/// One piece to place.
#[derive(Debug, Clone)]
struct Need {
    row: usize,
    copy: usize,
    tag: String,
    item: Item,
}

/// "a".."z", "aa".. for 1, 2, 3...
pub fn letters(mut n: usize) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    s.reverse();
    String::from_utf8(s).expect("ascii")
}

/// The ways a part may lie on stock: (dx, dy, turned). Along the stock's grain unless the
/// part or the stock has none.
fn orientations(row: &CutRow, stock_grain: StockGrain) -> Vec<(f64, f64, bool)> {
    let (w, l) = (row.size[1], row.size[2]);
    let free = stock_grain == StockGrain::None || row.grain == Grain::None;
    let mut out = match (free, row.grain) {
        (true, _) => vec![(l, w, false), (w, l, true)],
        (false, Grain::Width) => vec![(w, l, true)],
        (false, _) => vec![(l, w, false)],
    };
    if out.len() == 2 && w == l {
        out.pop();
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn x2(&self) -> f64 {
        self.x + self.w
    }
    fn y2(&self) -> f64 {
        self.y + self.h
    }
    fn holds(&self, dx: f64, dy: f64) -> bool {
        dx <= self.w + EPS && dy <= self.h + EPS
    }
}

/// The trim actually taken off an edge: none, or at least a kerf (a cut cannot take off
/// less than its own width).
pub fn effective_trim(trim: f64, kerf: f64) -> f64 {
    if trim > 0.0 { trim.max(kerf) } else { 0.0 }
}

/// The usable area of a stock (inside the trim): sheets lose it on every edge, boards on
/// their two ends.
fn usable(s: &StockDef, trim: f64) -> Rect {
    match s.kind {
        StockKind::Sheet => Rect {
            x: trim,
            y: trim,
            w: s.length - 2.0 * trim,
            h: s.width - 2.0 * trim,
        },
        StockKind::Board => Rect {
            x: trim,
            y: 0.0,
            w: s.length - 2.0 * trim,
            h: s.width,
        },
    }
}

/// Whether a part fits a stock at all: `Ok` with its allowed orientations, or why not.
fn fit(row: &CutRow, s: &StockDef, trim: f64) -> Result<Vec<(f64, f64, bool)>, UnplacedReason> {
    let u = usable(s, trim);
    let allowed = orientations(row, s.grain);
    let fitting: Vec<_> = allowed
        .iter()
        .copied()
        .filter(|&(dx, dy, _)| u.holds(dx, dy))
        .collect();
    if !fitting.is_empty() {
        return Ok(allowed);
    }
    let (w, l) = (row.size[1], row.size[2]);
    if u.holds(l, w) || u.holds(w, l) {
        Err(UnplacedReason::Grain)
    } else {
        Err(UnplacedReason::TooLarge)
    }
}

fn thickness_fits(s: &StockDef, t: f64) -> bool {
    match s.kind {
        StockKind::Sheet => (s.thickness - t).abs() <= SHEET_THICKNESS_SLACK + STOCK_EPSILON,
        StockKind::Board => s.thickness >= t - STOCK_EPSILON,
    }
}

fn kind_of(row: &CutRow) -> StockKind {
    if row.sheet {
        StockKind::Sheet
    } else {
        StockKind::Board
    }
}

/// Across the board: what must fit its width (the part's width, or its length when its
/// grain runs across).
fn across(row: &CutRow) -> f64 {
    if row.grain == Grain::Width {
        row.size[2]
    } else {
        row.size[1]
    }
}

/// A row's stock: its index in the library, or why there is none (and the stock meant).
type Choice = Result<usize, (UnplacedReason, Option<usize>)>;

/// The stock for each row: `Ok(index)` or `Err((reason, the stock meant))`.
fn choose_stock(list: &CutList, lib: &StockLibrary, trim: f64) -> Vec<Choice> {
    let n = list.rows.len();
    let mut out: Vec<Option<Choice>> = vec![None; n];
    // Said: a stock named by the part, or the material's default.
    for (i, row) in list.rows.iter().enumerate() {
        // A part dropped from the list takes no stock (lane 15); `nest_list` skips it.
        if !row.included {
            out[i] = Some(Err((UnplacedReason::NoStock, None)));
            continue;
        }
        // Listed but counted nowhere (lane 15 fixes): reported with the reason, never stocked.
        if let Some(u) = row.uncounted {
            out[i] = Some(Err((u.into(), None)));
            continue;
        }
        let t = row.size[0];
        if row.stock_set
            && let Some(m) = &row.material
            && let Some((k, s)) = lib.find(m)
        {
            out[i] = Some(if thickness_fits(s, t) {
                Ok(k)
            } else {
                Err((UnplacedReason::Thickness, Some(k)))
            });
            continue;
        }
        if let Some(m) = &row.material
            && let Some(name) = lib.default_for(m)
            && let Some((k, s)) = lib.find(name)
            && s.kind == kind_of(row)
            && thickness_fits(s, t)
        {
            out[i] = Some(Ok(k));
        }
    }
    // How well a part sits on a stock: fits, fits only across its grain, does not fit.
    let rank = |row: &CutRow, s: &StockDef| match fit(row, s, trim) {
        Ok(_) => 0,
        Err(UnplacedReason::Grain) => 1,
        Err(_) => 2,
    };
    // Sheets, automatically: within 1/16 in of the thickness, fitting first, then a stock
    // whose name shares a sheet word with the part's material ("MDF", "Baltic"), then the
    // closest thickness, actual or nominal (lane 15: a part drawn 3/4 in is as close to
    // "3/4" ply, 23/32 actual, as to 3/4 MDF), then library order.
    let words = |s: &str| -> Vec<&'static str> {
        let l = s.to_lowercase();
        crate::cutlist::SHEET_WORDS
            .iter()
            .copied()
            .filter(|w| *w != "sheet" && l.contains(w))
            .collect()
    };
    let near = |s: &StockDef, t: f64| {
        let nominal = s
            .nominal
            .as_ref()
            .map(|_| s.nominal_inches() * crate::cutlist::M_PER_IN);
        let d = (s.thickness - t).abs();
        nominal.map_or(d, |n| d.min((n - t).abs()))
    };
    for (i, row) in list.rows.iter().enumerate() {
        if out[i].is_some() || !row.sheet {
            continue;
        }
        let t = row.size[0];
        let mine = row.material.as_deref().map(words).unwrap_or_default();
        let affinity = |s: &StockDef| u8::from(!words(&s.name).iter().any(|w| mine.contains(w)));
        let best = lib
            .stocks
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == StockKind::Sheet && thickness_fits(s, t))
            .min_by(|(a, sa), (b, sb)| {
                rank(row, sa)
                    .cmp(&rank(row, sb))
                    .then(affinity(sa).cmp(&affinity(sb)))
                    .then(near(sa, t).total_cmp(&near(sb, t)))
                    .then(a.cmp(b))
            })
            .map(|(k, _)| k);
        out[i] = Some(best.ok_or((UnplacedReason::NoStock, None)));
    }
    // Lumber, automatically: grouped by material and thickness class.
    let boards: Vec<(usize, &StockDef)> = lib
        .stocks
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == StockKind::Board)
        .collect();
    let mut classes: BTreeMap<(Option<String>, u64), Vec<usize>> = BTreeMap::new();
    for (i, row) in list.rows.iter().enumerate() {
        if out[i].is_some() || row.sheet {
            continue;
        }
        let t = row.size[0];
        let class = boards
            .iter()
            .filter(|(_, s)| thickness_fits(s, t))
            .map(|(_, s)| s.thickness)
            .min_by(f64::total_cmp);
        match class {
            None => out[i] = Some(Err((UnplacedReason::NoStock, None))),
            Some(c) => classes
                .entry((row.material.clone(), c.to_bits()))
                .or_default()
                .push(i),
        }
    }
    for ((_, bits), rows) in classes {
        let class = f64::from_bits(bits);
        let widest = boards
            .iter()
            .filter(|(_, s)| s.thickness == class)
            .map(|(_, s)| s.width)
            .fold(0.0, f64::max);
        // Parts no board of the class is wide enough for are placed on their own (below);
        // the rest share the narrowest board as wide as the widest of them.
        let (shared, alone): (Vec<usize>, Vec<usize>) = rows
            .iter()
            .partition(|&&i| across(&list.rows[i]) <= widest + EPS);
        let need_w = shared
            .iter()
            .map(|&i| across(&list.rows[i]))
            .fold(0.0, f64::max);
        let mut wide: Vec<(usize, &StockDef)> = boards
            .iter()
            .copied()
            .filter(|(_, s)| s.thickness == class && s.width >= need_w - EPS)
            .collect();
        // Narrowest, then longest, then library order.
        wide.sort_by(|(a, sa), (b, sb)| {
            sa.width
                .total_cmp(&sb.width)
                .then(sb.length.total_cmp(&sa.length))
                .then(a.cmp(b))
        });
        let all_fit = |s: &StockDef| shared.iter().all(|&i| fit(&list.rows[i], s, trim).is_ok());
        if let Some(&(k, _)) = wide.iter().find(|(_, s)| all_fit(s)).or(wide.first()) {
            for &i in &shared {
                out[i] = Some(Ok(k));
            }
        }
        for &i in &alone {
            // Each on its own best board: fitting first (then fitting across its grain),
            // thinnest, widest, longest, library order.
            let row = &list.rows[i];
            let t = row.size[0];
            out[i] = Some(
                boards
                    .iter()
                    .filter(|(_, s)| thickness_fits(s, t))
                    .min_by(|(a, sa), (b, sb)| {
                        rank(row, sa)
                            .cmp(&rank(row, sb))
                            .then(sa.thickness.total_cmp(&sb.thickness))
                            .then(sb.width.total_cmp(&sa.width))
                            .then(sb.length.total_cmp(&sa.length))
                            .then(a.cmp(b))
                    })
                    .map(|(k, _)| *k)
                    .ok_or((UnplacedReason::NoStock, None)),
            );
        }
    }
    out.into_iter()
        .map(|o| o.unwrap_or(Err((UnplacedReason::NoStock, None))))
        .collect()
}

// ---- sheets ---------------------------------------------------------------------------------

/// A piece put somewhere: which need, where, turned or not.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Put {
    need: usize,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    turned: bool,
}

/// Something to lay out: a need and its allowed orientations.
struct Item2 {
    need: usize,
    orients: Vec<(f64, f64, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Area,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Split {
    /// Keep the largest offcut.
    MaxArea,
    /// Rip first: strips along the stock's length.
    Rips,
}

fn sorted(items: &[Item2], by: SortBy) -> Vec<usize> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    let key = |i: usize| {
        let (dx, dy, _) = items[i].orients[0];
        (dx * dy, dx.max(dy))
    };
    order.sort_by(|&a, &b| {
        let (aa, al) = key(a);
        let (ba, bl) = key(b);
        let c = match by {
            SortBy::Area => ba.total_cmp(&aa).then(bl.total_cmp(&al)),
            SortBy::Long => bl.total_cmp(&al).then(ba.total_cmp(&aa)),
        };
        c.then(items[a].need.cmp(&items[b].need))
    });
    order
}

/// Guillotine layout: free rectangles cut edge to edge, best area fit, first sheet that fits.
fn guillotine(items: &[Item2], u: Rect, kerf: f64, by: SortBy, split: Split) -> Vec<Vec<Put>> {
    let mut sheets: Vec<(Vec<Rect>, Vec<Put>)> = Vec::new();
    for i in sorted(items, by) {
        let it = &items[i];
        let mut chosen: Option<(usize, usize, usize)> = None;
        for (si, (free, _)) in sheets.iter().enumerate() {
            let mut best: Option<((f64, f64), usize, usize)> = None;
            for (ri, r) in free.iter().enumerate() {
                for (oi, &(dx, dy, _)) in it.orients.iter().enumerate() {
                    if !r.holds(dx, dy) {
                        continue;
                    }
                    let score = (r.w * r.h - dx * dy, (r.w - dx).min(r.h - dy));
                    let better = best.is_none_or(|(b, _, _)| {
                        score.0 < b.0 - EPS * EPS
                            || (score.0 <= b.0 + EPS * EPS && score.1 < b.1 - EPS)
                    });
                    if better {
                        best = Some((score, ri, oi));
                    }
                }
            }
            if let Some((_, ri, oi)) = best {
                chosen = Some((si, ri, oi));
                break;
            }
        }
        let (si, ri, oi) = match chosen {
            Some(c) => c,
            None => {
                let oi = it
                    .orients
                    .iter()
                    .position(|&(dx, dy, _)| u.holds(dx, dy))
                    .expect("only fitting parts are laid out");
                sheets.push((vec![u], Vec::new()));
                (sheets.len() - 1, 0, oi)
            }
        };
        let (dx, dy, turned) = it.orients[oi];
        let (free, puts) = &mut sheets[si];
        let r = free.remove(ri);
        puts.push(Put {
            need: it.need,
            x: r.x,
            y: r.y,
            dx,
            dy,
            turned,
        });
        let rw = r.w - dx - kerf;
        let th = r.h - dy - kerf;
        // A: crosscut first (right offcut full height); B: rip first (top offcut full length).
        let a = [
            Rect {
                x: r.x + dx + kerf,
                y: r.y,
                w: rw,
                h: r.h,
            },
            Rect {
                x: r.x,
                y: r.y + dy + kerf,
                w: dx,
                h: th,
            },
        ];
        let b = [
            Rect {
                x: r.x,
                y: r.y + dy + kerf,
                w: r.w,
                h: th,
            },
            Rect {
                x: r.x + dx + kerf,
                y: r.y,
                w: rw,
                h: dy,
            },
        ];
        let area = |q: &Rect| {
            if q.w > EPS && q.h > EPS {
                q.w * q.h
            } else {
                0.0
            }
        };
        let pick_a = match split {
            Split::Rips => false,
            Split::MaxArea => area(&a[0]).max(area(&a[1])) > area(&b[0]).max(area(&b[1])),
        };
        for q in if pick_a { a } else { b } {
            if q.w > EPS && q.h > EPS {
                free.push(q);
            }
        }
    }
    sheets.into_iter().map(|(_, p)| p).collect()
}

/// Maximal-rectangles layout, best short side fit, parts and bin grown by the kerf.
fn maxrects(items: &[Item2], u: Rect, kerf: f64, by: SortBy) -> Vec<Vec<Put>> {
    let bin = Rect {
        x: u.x,
        y: u.y,
        w: u.w + kerf,
        h: u.h + kerf,
    };
    let mut sheets: Vec<(Vec<Rect>, Vec<Put>)> = Vec::new();
    for i in sorted(items, by) {
        let it = &items[i];
        let mut chosen: Option<(usize, usize, usize)> = None;
        for (si, (free, _)) in sheets.iter().enumerate() {
            let mut best: Option<((f64, f64), usize, usize)> = None;
            for (ri, r) in free.iter().enumerate() {
                for (oi, &(dx, dy, _)) in it.orients.iter().enumerate() {
                    let (pw, ph) = (dx + kerf, dy + kerf);
                    if !r.holds(pw, ph) {
                        continue;
                    }
                    let (lw, lh) = (r.w - pw, r.h - ph);
                    let score = (lw.min(lh), lw.max(lh));
                    let better = best.is_none_or(|(b, _, _)| {
                        score.0 < b.0 - EPS || (score.0 <= b.0 + EPS && score.1 < b.1 - EPS)
                    });
                    if better {
                        best = Some((score, ri, oi));
                    }
                }
            }
            if let Some((_, ri, oi)) = best {
                chosen = Some((si, ri, oi));
                break;
            }
        }
        let (si, ri, oi) = match chosen {
            Some(c) => c,
            None => {
                let oi = it
                    .orients
                    .iter()
                    .position(|&(dx, dy, _)| bin.holds(dx + kerf, dy + kerf))
                    .expect("only fitting parts are laid out");
                sheets.push((vec![bin], Vec::new()));
                (sheets.len() - 1, 0, oi)
            }
        };
        let (dx, dy, turned) = it.orients[oi];
        let (free, puts) = &mut sheets[si];
        let r = free[ri];
        puts.push(Put {
            need: it.need,
            x: r.x,
            y: r.y,
            dx,
            dy,
            turned,
        });
        let o = Rect {
            x: r.x,
            y: r.y,
            w: dx + kerf,
            h: dy + kerf,
        };
        let mut next: Vec<Rect> = Vec::with_capacity(free.len() + 4);
        for f in free.iter() {
            let overlaps = f.x < o.x2() - EPS
                && o.x < f.x2() - EPS
                && f.y < o.y2() - EPS
                && o.y < f.y2() - EPS;
            if !overlaps {
                next.push(*f);
                continue;
            }
            if o.x > f.x + EPS {
                next.push(Rect {
                    x: f.x,
                    y: f.y,
                    w: o.x - f.x,
                    h: f.h,
                });
            }
            if f.x2() > o.x2() + EPS {
                next.push(Rect {
                    x: o.x2(),
                    y: f.y,
                    w: f.x2() - o.x2(),
                    h: f.h,
                });
            }
            if o.y > f.y + EPS {
                next.push(Rect {
                    x: f.x,
                    y: f.y,
                    w: f.w,
                    h: o.y - f.y,
                });
            }
            if f.y2() > o.y2() + EPS {
                next.push(Rect {
                    x: f.x,
                    y: o.y2(),
                    w: f.w,
                    h: f.y2() - o.y2(),
                });
            }
        }
        // Drop rectangles inside another (the first of two equal ones stays).
        let contains = |a: &Rect, b: &Rect| {
            b.x >= a.x - EPS && b.y >= a.y - EPS && b.x2() <= a.x2() + EPS && b.y2() <= a.y2() + EPS
        };
        let keep: Vec<bool> = (0..next.len())
            .map(|i| {
                !(0..next.len()).any(|j| {
                    j != i
                        && contains(&next[j], &next[i])
                        && (!contains(&next[i], &next[j]) || j < i)
                })
            })
            .collect();
        *free = next
            .into_iter()
            .zip(keep)
            .filter_map(|(r, k)| k.then_some(r))
            .collect();
    }
    sheets.into_iter().map(|(_, p)| p).collect()
}

/// The better of two layouts: fewer stock pieces, then the emptier last piece.
fn better(a: &[Vec<Put>], b: &[Vec<Put>]) -> bool {
    let last = |l: &[Vec<Put>]| {
        l.last()
            .map_or(0.0, |p| p.iter().map(|q| q.dx * q.dy).sum::<f64>())
    };
    a.len() < b.len() || (a.len() == b.len() && last(a) < last(b) - EPS * EPS)
}

fn sheet_layout(items: &[Item2], u: Rect, s: &NestSettings) -> Vec<Vec<Put>> {
    let mut best: Option<Vec<Vec<Put>>> = None;
    for by in [SortBy::Area, SortBy::Long] {
        let tries: Vec<Vec<Vec<Put>>> = match s.algorithm {
            NestAlgorithm::Guillotine => [Split::MaxArea, Split::Rips]
                .into_iter()
                .map(|sp| guillotine(items, u, s.kerf, by, sp))
                .collect(),
            NestAlgorithm::Maxrects => vec![maxrects(items, u, s.kerf, by)],
        };
        for t in tries {
            if best.as_ref().is_none_or(|b| better(&t, b)) {
                best = Some(t);
            }
        }
    }
    best.unwrap_or_default()
}

// ---- boards ---------------------------------------------------------------------------------

/// First-fit decreasing on lengths: bins of `cap`, each part `a` long, a kerf between parts.
fn ffd(lens: &[(usize, f64)], cap: f64, kerf: f64) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..lens.len()).collect();
    order.sort_by(|&a, &b| lens[b].1.total_cmp(&lens[a].1).then(a.cmp(&b)));
    let mut bins: Vec<(f64, Vec<usize>)> = Vec::new();
    for i in order {
        let a = lens[i].1;
        match bins.iter_mut().find(|(end, _)| end + a <= cap + EPS) {
            Some((end, v)) => {
                *end += a + kerf;
                v.push(i);
            }
            None => bins.push((a + kerf, vec![i])),
        }
    }
    bins.into_iter().map(|(_, v)| v).collect()
}

/// Fill one board at a time: the longest part left, then the subset of the rest that leaves
/// the least waste (depth-first, equal lengths tried once, a node budget).
fn lookahead(lens: &[(usize, f64)], cap: f64, kerf: f64) -> Vec<Vec<usize>> {
    let mut left: Vec<usize> = (0..lens.len()).collect();
    left.sort_by(|&a, &b| lens[b].1.total_cmp(&lens[a].1).then(a.cmp(&b)));
    let mut budget = LOOKAHEAD_TOTAL;
    let mut bins = Vec::new();
    while !left.is_empty() {
        let first = left[0];
        let room = cap - lens[first].1;
        let cand: Vec<usize> = left[1..]
            .iter()
            .copied()
            .filter(|&i| lens[i].1 + kerf <= room + EPS)
            .collect();
        let cost: Vec<f64> = cand.iter().map(|&i| lens[i].1 + kerf).collect();
        let mut suffix = vec![0.0; cost.len() + 1];
        for k in (0..cost.len()).rev() {
            suffix[k] = suffix[k + 1] + cost[k];
        }
        struct Search<'a> {
            cost: &'a [f64],
            suffix: &'a [f64],
            room: f64,
            best: f64,
            best_set: Vec<usize>,
            cur: Vec<usize>,
            nodes: usize,
            limit: usize,
        }
        fn dfs(s: &mut Search, start: usize, used: f64) {
            if used > s.best + EPS {
                s.best = used;
                s.best_set = s.cur.clone();
            }
            for k in start..s.cost.len() {
                if s.nodes >= s.limit || s.best >= s.room - EPS {
                    return;
                }
                if used + s.suffix[k] <= s.best + EPS {
                    return;
                }
                if k > start && s.cost[k] == s.cost[k - 1] {
                    continue;
                }
                if used + s.cost[k] <= s.room + EPS {
                    s.nodes += 1;
                    s.cur.push(k);
                    dfs(s, k + 1, used + s.cost[k]);
                    s.cur.pop();
                }
            }
        }
        let mut s = Search {
            cost: &cost,
            suffix: &suffix,
            room,
            best: 0.0,
            best_set: Vec::new(),
            cur: Vec::new(),
            nodes: 0,
            limit: LOOKAHEAD_PER_BOARD.min(budget),
        };
        dfs(&mut s, 0, 0.0);
        budget = budget.saturating_sub(s.nodes);
        let mut bin = vec![first];
        bin.extend(s.best_set.iter().map(|&k| cand[k]));
        left.retain(|i| !bin.contains(i));
        bins.push(bin);
    }
    bins
}

fn board_layout(items: &[Item2], u: Rect, kerf: f64) -> Vec<Vec<Put>> {
    // Each part's one orientation that fits the board's width (along the board first).
    let chosen: Vec<(f64, f64, bool)> = items
        .iter()
        .map(|it| {
            *it.orients
                .iter()
                .find(|&&(dx, dy, _)| u.holds(dx, dy))
                .expect("only fitting parts are laid out")
        })
        .collect();
    let lens: Vec<(usize, f64)> = chosen.iter().enumerate().map(|(i, o)| (i, o.0)).collect();
    let a = ffd(&lens, u.w, kerf);
    let b = lookahead(&lens, u.w, kerf);
    let bins = if a.len() < b.len() { a } else { b };
    bins.into_iter()
        .map(|bin| {
            let mut x = u.x;
            bin.into_iter()
                .map(|i| {
                    let (dx, dy, turned) = chosen[i];
                    let p = Put {
                        need: items[i].need,
                        x,
                        y: u.y,
                        dx,
                        dy,
                        turned,
                    };
                    x += dx + kerf;
                    p
                })
                .collect()
        })
        .collect()
}

// ---- cuts -----------------------------------------------------------------------------------

/// The first straight cut across `reg` (direction `dir`) that crosses no part: before the
/// first part, between two, or after the last; `None` when there is none.
fn find_cut(reg: Rect, parts: &[Put], dir: CutDir, kerf: f64) -> Option<f64> {
    let span = |p: &Put| match dir {
        CutDir::Rip => (p.y, p.y + p.dy),
        CutDir::Crosscut => (p.x, p.x + p.dx),
    };
    let (r0, r1) = match dir {
        CutDir::Rip => (reg.y, reg.y2()),
        CutDir::Crosscut => (reg.x, reg.x2()),
    };
    let mut iv: Vec<(f64, f64)> = parts.iter().map(span).collect();
    iv.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let lead = iv[0].0 - kerf;
    if lead > r0 + EPS {
        return Some(lead);
    }
    let mut hi = iv[0].1;
    for &(lo, h) in &iv[1..] {
        if lo >= hi + kerf - EPS && hi < r1 - EPS {
            return Some(hi);
        }
        hi = hi.max(h);
    }
    (hi < r1 - EPS).then_some(hi)
}

/// The cut sequence that frees every part, and whether it does (see the module docs).
fn decompose(u: Rect, parts: &[Put], kerf: f64, kind: StockKind) -> (Vec<Cut>, bool) {
    let mut cuts = Vec::new();
    let mut ok = true;
    let mut stack: Vec<(Rect, Vec<Put>)> = vec![(u, parts.to_vec())];
    while let Some((reg, ps)) = stack.pop() {
        if ps.is_empty() {
            continue;
        }
        let dirs = match kind {
            StockKind::Board => [CutDir::Crosscut, CutDir::Rip],
            StockKind::Sheet if reg.h > reg.w + EPS => [CutDir::Crosscut, CutDir::Rip],
            StockKind::Sheet => [CutDir::Rip, CutDir::Crosscut],
        };
        let found = dirs
            .iter()
            .find_map(|&d| find_cut(reg, &ps, d, kerf).map(|c| (d, c)));
        let Some((dir, c)) = found else {
            if ps.len() > 1 {
                ok = false;
            }
            continue;
        };
        let (lo_reg, hi_reg, from, to) = match dir {
            CutDir::Rip => (
                Rect {
                    h: c - reg.y,
                    ..reg
                },
                Rect {
                    y: c + kerf,
                    h: reg.y2() - c - kerf,
                    ..reg
                },
                reg.x,
                reg.x2(),
            ),
            CutDir::Crosscut => (
                Rect {
                    w: c - reg.x,
                    ..reg
                },
                Rect {
                    x: c + kerf,
                    w: reg.x2() - c - kerf,
                    ..reg
                },
                reg.y,
                reg.y2(),
            ),
        };
        let near = match dir {
            CutDir::Rip => reg.y,
            CutDir::Crosscut => reg.x,
        };
        cuts.push(Cut {
            n: 0,
            dir,
            at: c,
            from,
            to,
            size: c - near,
            trim: false,
        });
        let (lo, hi): (Vec<Put>, Vec<Put>) = ps.into_iter().partition(|p| match dir {
            CutDir::Rip => p.y + p.dy <= c + EPS,
            CutDir::Crosscut => p.x + p.dx <= c + EPS,
        });
        // Lower side first: push it last.
        stack.push((hi_reg, hi));
        stack.push((lo_reg, lo));
    }
    (cuts, ok)
}

/// Trim cuts: each edge of a sheet, each end of a board, when there is a trim.
fn trim_cuts(s: &StockDef, trim: f64, kerf: f64) -> Vec<Cut> {
    if trim <= 0.0 {
        return Vec::new();
    }
    let low = trim - kerf;
    let mut out = Vec::new();
    let mut cut = |dir, at, from, to| {
        out.push(Cut {
            n: 0,
            dir,
            at,
            from,
            to,
            size: trim,
            trim: true,
        })
    };
    if s.kind == StockKind::Sheet {
        cut(CutDir::Rip, low, 0.0, s.length);
        cut(CutDir::Rip, s.width - trim, 0.0, s.length);
    }
    cut(CutDir::Crosscut, low, 0.0, s.width);
    cut(CutDir::Crosscut, s.length - trim, 0.0, s.width);
    out
}

// ---- the whole nesting ----------------------------------------------------------------------

/// Nest a cut list on a library's stock (see the module docs). Pure: the model's
/// [`Model::nest`] calls this with its own cut list and library.
pub fn nest_list(list: &CutList, lib: &StockLibrary, settings: NestSettings) -> Nesting {
    let kerf = settings.kerf;
    let trim = effective_trim(settings.trim, kerf);
    let choice = choose_stock(list, lib, trim);
    let mut needs: Vec<Need> = Vec::new();
    let mut unplaced: Vec<Unplaced> = Vec::new();
    // Groups by stock and material, in order of their first piece.
    let mut group_order: Vec<(usize, Option<String>)> = Vec::new();
    let mut group_items: BTreeMap<(usize, Option<String>), Vec<Item2>> = BTreeMap::new();
    for (ri, row) in list.rows.iter().enumerate() {
        // Dropped from the list: not nested, not reported (row numbers stay the list's).
        if !row.included {
            continue;
        }
        let counted: Vec<&crate::cutlist::Occurrence> = row
            .occurrences
            .iter()
            .filter(|o| !list.visible_only || o.visible)
            .collect();
        let many = counted.len() > 1;
        let stocked = choice[ri].and_then(|k| {
            fit(row, &lib.stocks[k], trim)
                .map(|o| (k, o))
                .map_err(|r| (r, Some(k)))
        });
        for (ci, o) in counted.iter().enumerate() {
            let copy = ci + 1;
            let tag = if many {
                format!("{}{}", ri + 1, letters(copy))
            } else {
                (ri + 1).to_string()
            };
            match &stocked {
                Ok((k, orients)) => {
                    let need = needs.len();
                    needs.push(Need {
                        row: ri,
                        copy,
                        tag,
                        item: o.item,
                    });
                    let key = (*k, row.material.clone());
                    group_items
                        .entry(key.clone())
                        .or_insert_with(|| {
                            group_order.push(key);
                            Vec::new()
                        })
                        .push(Item2 {
                            need,
                            orients: orients.clone(),
                        });
                }
                Err((reason, k)) => unplaced.push(Unplaced {
                    row: ri,
                    copy,
                    tag,
                    name: row.name.clone(),
                    item: o.item,
                    size: row.size,
                    material: row.material.clone(),
                    stock: k.map(|k| lib.stocks[k].name.clone()),
                    sheet: row.sheet,
                    reason: *reason,
                }),
            }
        }
    }
    let mut pieces: Vec<StockPiece> = Vec::new();
    let mut groups: Vec<NestGroup> = Vec::new();
    for (gi, key) in group_order.iter().enumerate() {
        let s = &lib.stocks[key.0];
        let items = &group_items[key];
        let u = usable(s, trim);
        let layout = match s.kind {
            StockKind::Sheet => sheet_layout(items, u, &settings),
            StockKind::Board => board_layout(items, u, kerf),
        };
        let count = layout.len();
        let stock_area = s.length * s.width;
        let mut g = NestGroup {
            stock: s.name.clone(),
            kind: s.kind,
            nominal: s.nominal.clone(),
            material: key.1.clone(),
            length: s.length,
            width: s.width,
            thickness: s.thickness,
            count,
            parts: 0,
            parts_area: 0.0,
            stock_area: stock_area * count as f64,
            efficiency: 0.0,
            board_feet: s.board_feet() * count as f64,
        };
        for (seq, puts) in layout.into_iter().enumerate() {
            // Placements in reading order: along the length, then across.
            let mut puts = puts;
            puts.sort_by(|a, b| {
                a.y.total_cmp(&b.y)
                    .then(a.x.total_cmp(&b.x))
                    .then(a.need.cmp(&b.need))
            });
            let (mut cuts, guillotine) = decompose(u, &puts, kerf, s.kind);
            let mut all = trim_cuts(s, trim, kerf);
            all.append(&mut cuts);
            for (k, c) in all.iter_mut().enumerate() {
                c.n = k as u32 + 1;
            }
            let placements: Vec<Placement> = puts
                .iter()
                .map(|p| {
                    let n = &needs[p.need];
                    let row = &list.rows[n.row];
                    Placement {
                        row: n.row,
                        copy: n.copy,
                        tag: n.tag.clone(),
                        name: row.name.clone(),
                        item: n.item,
                        x: p.x,
                        y: p.y,
                        dx: p.dx,
                        dy: p.dy,
                        turned: p.turned,
                        grain: row.grain,
                        size: row.size,
                        not_solid: row.not_solid,
                    }
                })
                .collect();
            let parts_area: f64 = placements.iter().map(|p| p.dx * p.dy).sum();
            g.parts += placements.len();
            g.parts_area += parts_area;
            pieces.push(StockPiece {
                index: pieces.len() + 1,
                group: gi,
                seq: seq + 1,
                count,
                stock: s.name.clone(),
                kind: s.kind,
                nominal: s.nominal.clone(),
                material: key.1.clone(),
                length: s.length,
                width: s.width,
                thickness: s.thickness,
                grain: s.grain,
                trim,
                kerf,
                usable: [u.x, u.y, u.x2(), u.y2()],
                placements,
                cuts: all,
                guillotine,
                parts_area,
                stock_area,
                waste_area: stock_area - parts_area,
                efficiency: parts_area / stock_area,
            });
        }
        g.efficiency = if g.stock_area > 0.0 {
            g.parts_area / g.stock_area
        } else {
            0.0
        };
        groups.push(g);
    }
    let mut by_nominal: Vec<NominalTotal> = Vec::new();
    for g in groups.iter().filter(|g| g.kind == StockKind::Board) {
        let nominal = g.nominal.clone().unwrap_or_else(|| g.stock.clone());
        match by_nominal.iter_mut().find(|n| n.nominal == nominal) {
            Some(n) => {
                n.count += g.count;
                n.board_feet += g.board_feet;
            }
            None => by_nominal.push(NominalTotal {
                nominal,
                count: g.count,
                board_feet: g.board_feet,
            }),
        }
    }
    let placed: usize = pieces.iter().map(|p| p.placements.len()).sum();
    let parts_area: f64 = pieces.iter().map(|p| p.parts_area).sum();
    let stock_area: f64 = pieces.iter().map(|p| p.stock_area).sum();
    let totals = NestTotals {
        sheets: groups
            .iter()
            .filter(|g| g.kind == StockKind::Sheet)
            .map(|g| g.count)
            .sum(),
        boards: groups
            .iter()
            .filter(|g| g.kind == StockKind::Board)
            .map(|g| g.count)
            .sum(),
        parts_in: placed + unplaced.len(),
        placed,
        unplaced: unplaced.len(),
        parts_area,
        stock_area,
        efficiency: if stock_area > 0.0 {
            parts_area / stock_area
        } else {
            0.0
        },
        groups,
        by_nominal,
    };
    let mut n = Nesting {
        algorithm: settings.algorithm,
        settings,
        visible_only: list.visible_only,
        library: lib.clone(),
        pieces,
        unplaced,
        totals,
        hash: String::new(),
    };
    n.hash = format!("{:016x}", n.hash_value());
    n
}

impl Nesting {
    fn hash_value(&self) -> u64 {
        let mut h = StableHasher::default();
        h.tag("sawhorse-nesting-v1");
        self.library.hash_into(&mut h);
        h.u64(match self.algorithm {
            NestAlgorithm::Guillotine => 0,
            NestAlgorithm::Maxrects => 1,
        });
        h.u64(u64::from(self.visible_only));
        let item = |h: &mut StableHasher, i: &Item| match *i {
            Item::Object(id) => h.u64(id << 1),
            Item::Node(id) => h.u64(id << 1 | 1),
        };
        h.usize(self.pieces.len());
        for p in &self.pieces {
            h.tag(&p.stock);
            h.tag(p.material.as_deref().unwrap_or(""));
            h.u64(u64::from(p.guillotine));
            h.usize(p.placements.len());
            for q in &p.placements {
                h.usize(q.row);
                h.usize(q.copy);
                item(&mut h, &q.item);
                for v in [q.x, q.y, q.dx, q.dy] {
                    h.len(v);
                }
                h.u64(u64::from(q.turned));
            }
            h.usize(p.cuts.len());
            for c in &p.cuts {
                h.u64(u64::from(c.dir == CutDir::Rip) | u64::from(c.trim) << 1);
                for v in [c.at, c.from, c.to] {
                    h.len(v);
                }
            }
        }
        h.usize(self.unplaced.len());
        for u in &self.unplaced {
            h.usize(u.row);
            h.usize(u.copy);
            item(&mut h, &u.item);
            h.u64(u.reason as u64);
        }
        h.usize(self.totals.sheets);
        h.usize(self.totals.boards);
        h.finish()
    }
}

impl Model {
    /// The model's parts nested on its stock library (see the module docs). Read-only.
    pub fn nest(&self, opts: &NestOptions) -> Nesting {
        let lib = self.stock_library();
        let mut settings = lib.settings;
        if let Some(a) = opts.algorithm {
            settings.algorithm = a;
        }
        let list = self.cut_list(&CutOptions {
            visible_only: opts.visible_only,
            ..CutOptions::default()
        });
        nest_list(&list, &lib, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(x: f64, y: f64, dx: f64, dy: f64) -> Put {
        Put {
            need: 0,
            x,
            y,
            dx,
            dy,
            turned: false,
        }
    }

    #[test]
    fn letters_count_like_columns() {
        assert_eq!(letters(1), "a");
        assert_eq!(letters(26), "z");
        assert_eq!(letters(27), "aa");
        assert_eq!(letters(52), "az");
        assert_eq!(letters(53), "ba");
    }

    #[test]
    fn first_fit_decreasing_and_lookahead() {
        // Cap 10, kerf 0: 6 4 5 5 -> FFD [6 4] [5 5]; lookahead the same.
        let lens: Vec<(usize, f64)> = [6.0, 4.0, 5.0, 5.0].into_iter().enumerate().collect();
        assert_eq!(ffd(&lens, 10.0, 0.0).len(), 2);
        assert_eq!(lookahead(&lens, 10.0, 0.0).len(), 2);
        // Where FFD needs more boards: 5 4 3 3 3 2 in 10 -> FFD [5 4] [3 3 3] [2]... lookahead
        // [5 3 2] [4 3 3].
        let lens: Vec<(usize, f64)> = [5.0, 4.0, 3.0, 3.0, 3.0, 2.0]
            .into_iter()
            .enumerate()
            .collect();
        let f = ffd(&lens, 10.0, 0.0);
        let l = lookahead(&lens, 10.0, 0.0);
        assert_eq!(l.len(), 2, "{l:?}");
        assert!(f.len() >= l.len());
        // The kerf counts between parts, not after the last.
        let lens = vec![(0, 5.0), (1, 4.875)];
        assert_eq!(lookahead(&lens, 10.0, 0.125).len(), 1);
        assert_eq!(ffd(&lens, 10.0, 0.125).len(), 1);
        assert_eq!(lookahead(&lens, 10.0, 0.1251).len(), 2);
    }

    #[test]
    fn cuts_free_a_strip_layout_rips_first() {
        let u = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 4.0,
        };
        // Two strips 1.9 high, the bottom one holding two parts.
        let parts = [
            put(0.0, 0.0, 4.0, 1.9),
            put(4.1, 0.0, 4.0, 1.9),
            put(0.0, 2.0, 9.0, 1.9),
        ];
        let (cuts, ok) = decompose(u, &parts, 0.1, StockKind::Sheet);
        assert!(ok);
        assert_eq!(cuts[0].dir, CutDir::Rip);
        assert!((cuts[0].at - 1.9).abs() < 1e-12);
        // Then the bottom strip: a crosscut between the parts, then the offcut after the
        // second; the top strip: its offcut along the top, then its end.
        assert_eq!(cuts.len(), 5, "{cuts:?}");
        // A pinwheel is not guillotine.
        let pin = [
            put(0.0, 0.0, 6.0, 1.0),
            put(6.1, 0.0, 1.0, 3.0),
            put(1.1, 3.1, 6.0, 0.9),
            put(0.0, 1.1, 1.0, 2.9),
            put(1.1, 1.1, 4.9, 1.9),
        ];
        let u = Rect {
            x: 0.0,
            y: 0.0,
            w: 7.1,
            h: 4.0,
        };
        assert!(!decompose(u, &pin, 0.1, StockKind::Sheet).1);
    }
}
