//! The stock library (lane 9): what a model's parts are cut from, and how the cuts are made.
//!
//! **Stock** is a sheet (a 4 x 8 sheet of 3/4 in ply, a 5 x 5 of Baltic birch, a sheet of
//! MDF) or a board (4/4 lumber, 6 in wide, 8 ft long): its length (the direction its grain
//! runs, when it has one), width, actual thickness, a nominal thickness for boards ("4/4"),
//! and whether it has a grain at all. A **default per material** says which stock a
//! material's parts come from ("Oak" -> "4/4 x 12 x 96"); anything not said is chosen by
//! kind and thickness ([`crate::nesting`]). The **settings** are the saw's kerf, the trim
//! taken off each edge of a sheet (each end of a board) before anything else, and the
//! nesting algorithm.
//!
//! A new document uses the built-in library ([`StockLibrary::builtin`]) and stores nothing:
//! the model's library is `None` until someone changes it, so older documents keep their
//! hashes and a library edited back to the built-in one is `None` again. Edits are ops
//! ([`crate::model::Op::DefineStock`] and friends): undoable, logged, replayed, saved in
//! `.shz` (`docs/format.md`).

use std::collections::BTreeMap;

use crate::cutlist::M_PER_IN;
use crate::error::StockError;
use crate::hash::StableHasher;

/// The kerf a new document assumes: 1/8 in (a full-kerf table-saw blade).
pub const DEFAULT_KERF: f64 = M_PER_IN / 8.0;
/// Widest kerf accepted: 1/2 in.
pub const MAX_KERF: f64 = 0.5 * M_PER_IN;
/// Most trim accepted per edge: 4 in.
pub const MAX_TRIM: f64 = 4.0 * M_PER_IN;
/// Smallest stock dimension accepted: 1 mm.
pub const MIN_STOCK: f64 = 0.001;
/// Largest stock dimension accepted: 10 m.
pub const MAX_STOCK: f64 = 10.0;
/// Most stock entries in a library.
pub const MAX_STOCKS: usize = 200;
/// A sheet part is cut from a sheet within this much of its thickness (1/16 in): a part
/// drawn 3/4 in thick comes from 23/32 in "3/4" plywood.
pub const SHEET_THICKNESS_SLACK: f64 = M_PER_IN / 16.0;
/// Slack on every stock comparison (1 micrometre): drawing noise, not a tolerance anyone sees.
pub const STOCK_EPSILON: f64 = 1e-6;

/// Sheet goods or solid lumber.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum StockKind {
    Sheet,
    Board,
}

/// Which way a stock's grain runs.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum StockGrain {
    /// Along its length (plywood faces, boards).
    #[default]
    Length,
    /// No grain (MDF, melamine, hardboard): parts may turn.
    None,
}

/// One kind of stock. Lengths in metres; `length` runs along the grain.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct StockDef {
    /// Unique in the library (compared without case), 1 to 64 characters.
    pub name: String,
    pub kind: StockKind,
    pub length: f64,
    pub width: f64,
    /// Actual thickness.
    pub thickness: f64,
    /// A board's nominal thickness ("4/4", "8/4"), at most 16 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal: Option<String>,
    #[serde(default)]
    pub grain: StockGrain,
}

/// How parts are laid out on sheets.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum NestAlgorithm {
    /// Every sheet comes apart in straight cuts from edge to edge (a table saw, a panel saw).
    #[default]
    Guillotine,
    /// Parts packed as tightly as their rectangles allow (maximal rectangles); some layouts
    /// need a track saw or a CNC.
    Maxrects,
}

/// The saw and the layout.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct NestSettings {
    /// Width of every cut, metres.
    #[serde(default = "default_kerf")]
    pub kerf: f64,
    /// Taken off each edge of a sheet (each end of a board) first, metres; the trim cut's
    /// kerf is inside it.
    #[serde(default)]
    pub trim: f64,
    #[serde(default)]
    pub algorithm: NestAlgorithm,
}

fn default_kerf() -> f64 {
    DEFAULT_KERF
}

impl Default for NestSettings {
    fn default() -> Self {
        NestSettings {
            kerf: DEFAULT_KERF,
            trim: 0.0,
            algorithm: NestAlgorithm::Guillotine,
        }
    }
}

impl NestSettings {
    pub fn check(&self) -> Result<(), StockError> {
        if !(self.kerf.is_finite() && (0.0..=MAX_KERF).contains(&self.kerf)) {
            return Err(StockError::BadSettings("the kerf must be 0 to 1/2 in"));
        }
        if !(self.trim.is_finite() && (0.0..=MAX_TRIM).contains(&self.trim)) {
            return Err(StockError::BadSettings(
                "the trim must be 0 to 4 in per edge",
            ));
        }
        Ok(())
    }

    fn hash_into(&self, h: &mut StableHasher) {
        h.len(self.kerf);
        h.len(self.trim);
        h.u64(match self.algorithm {
            NestAlgorithm::Guillotine => 0,
            NestAlgorithm::Maxrects => 1,
        });
    }
}

/// Every stock a document knows, its per-material defaults and the nesting settings.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct StockLibrary {
    pub stocks: Vec<StockDef>,
    /// Material (or stock text) -> the name of the stock its parts come from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, String>,
    #[serde(default)]
    pub settings: NestSettings,
}

impl Default for StockLibrary {
    fn default() -> Self {
        StockLibrary::builtin()
    }
}

fn inch(x: f64) -> f64 {
    x * M_PER_IN
}

/// Metres as inches, snapped to 2^-20 in (about 24 nm): drops floating-point noise only, so
/// a 12 in board reads as exactly 12 in.
fn in_inches(m: f64) -> f64 {
    (m / M_PER_IN * 1_048_576.0).round() / 1_048_576.0
}

/// The actual thickness (inches) a nominal thickness comes to by default (lane 15), as a
/// lumber yard sells it: plywood is undersized ("3/4" ply is 23/32 in, "1/2" is 15/32, "1/4"
/// is 7/32); hardwood boards are sold rough by the quarter and surfaced four sides (S4S):
/// 4/4 (1 in rough) is 3/4 in, 5/4 is 1 1/16, 6/4 is 1 5/16, 8/4 is 1 3/4, 12/4 is 2 3/4.
/// `None` for a nominal not in the table (the stock's own actual thickness is used). The
/// Stock dialog fills the actual thickness from this and lets it be overridden per stock.
pub fn standard_actual(kind: StockKind, nominal: &str) -> Option<f64> {
    let n = nominal.trim();
    match kind {
        StockKind::Sheet => match n {
            "3/4" => Some(23.0 / 32.0),
            "1/2" => Some(15.0 / 32.0),
            "1/4" => Some(7.0 / 32.0),
            "5/8" => Some(19.0 / 32.0),
            "3/8" => Some(11.0 / 32.0),
            _ => None,
        },
        StockKind::Board => match n {
            "4/4" => Some(0.75),
            "5/4" => Some(1.0625),
            "6/4" => Some(1.3125),
            "8/4" => Some(1.75),
            "10/4" => Some(2.25),
            "12/4" => Some(2.75),
            "16/4" => Some(3.75),
            _ => None,
        },
    }
}

fn sheet(name: &str, w: f64, l: f64, nominal: &str, actual: f64, grain: StockGrain) -> StockDef {
    StockDef {
        name: name.into(),
        kind: StockKind::Sheet,
        length: inch(l),
        width: inch(w),
        thickness: inch(actual),
        nominal: Some(nominal.into()),
        grain,
    }
}

fn board(nominal: &str, w: f64, l: f64) -> StockDef {
    StockDef {
        name: format!("{nominal} x {w} x {l}"),
        kind: StockKind::Board,
        length: inch(l),
        width: inch(w),
        thickness: inch(standard_actual(StockKind::Board, nominal).expect("a standard nominal")),
        nominal: Some(nominal.into()),
        grain: StockGrain::Length,
    }
}

/// Names compare without case and surrounding spaces.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A name a library accepts: 1 to 64 characters after trimming.
fn check_name(n: &str) -> Result<(), StockError> {
    let t = n.trim();
    if t.is_empty() || t.chars().count() > 64 {
        Err(StockError::BadName)
    } else {
        Ok(())
    }
}

impl StockDef {
    /// Refuse a stock nobody can cut from.
    pub fn check(&self) -> Result<(), StockError> {
        check_name(&self.name)?;
        let ok = |x: f64| x.is_finite() && (MIN_STOCK..=MAX_STOCK).contains(&x);
        if !(ok(self.length) && ok(self.width) && ok(self.thickness)) {
            return Err(StockError::BadStock(
                "its length, width and thickness must be 1 mm to 10 m",
            ));
        }
        if self.thickness > self.width.min(self.length) {
            return Err(StockError::BadStock(
                "it cannot be thicker than it is wide or long",
            ));
        }
        if self
            .nominal
            .as_ref()
            .is_some_and(|n| n.chars().count() > 16)
        {
            return Err(StockError::BadStock(
                "a nominal thickness is at most 16 characters",
            ));
        }
        Ok(())
    }

    /// The nominal thickness in inches when it reads "n/4" (quarters), else the actual one.
    pub fn nominal_inches(&self) -> f64 {
        if let Some(n) = &self.nominal
            && let Some((a, b)) = n.trim().split_once('/')
            && let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>())
            && b > 0
        {
            return f64::from(a) / f64::from(b);
        }
        in_inches(self.thickness)
    }

    /// Board feet of one piece of this stock (nominal thickness for boards; 0 for sheets).
    pub fn board_feet(&self) -> f64 {
        match self.kind {
            StockKind::Sheet => 0.0,
            StockKind::Board => {
                self.nominal_inches() * in_inches(self.width) * in_inches(self.length) / 144.0
            }
        }
    }

    fn hash_into(&self, h: &mut StableHasher) {
        h.tag(&self.name);
        h.u64(match self.kind {
            StockKind::Sheet => 0,
            StockKind::Board => 1,
        });
        h.len(self.length);
        h.len(self.width);
        h.len(self.thickness);
        match &self.nominal {
            Some(n) => h.tag(n),
            None => h.u64(u64::MAX),
        }
        h.u64(match self.grain {
            StockGrain::Length => 0,
            StockGrain::None => 1,
        });
    }
}

impl StockLibrary {
    /// What a new document starts with: plywood and MDF sheets, and hardwood boards 8 ft
    /// long in the common nominal thicknesses, each at the actual thickness a yard sells
    /// ([`standard_actual`], lane 15: "3/4" ply is 23/32 in, 4/4 S4S is 3/4 in; Baltic birch
    /// is metric, 18 and 12 mm; MDF is its nominal thickness).
    pub fn builtin() -> StockLibrary {
        use StockGrain::{Length, None};
        let ply = |n: &str| standard_actual(StockKind::Sheet, n).expect("a standard nominal");
        StockLibrary {
            stocks: vec![
                sheet("4 x 8 sheet, 3/4", 48.0, 96.0, "3/4", ply("3/4"), Length),
                sheet("4 x 8 sheet, 1/2", 48.0, 96.0, "1/2", ply("1/2"), Length),
                sheet("4 x 8 sheet, 1/4", 48.0, 96.0, "1/4", ply("1/4"), Length),
                sheet(
                    "5 x 5 Baltic birch, 3/4",
                    60.0,
                    60.0,
                    "3/4",
                    18.0 / 25.4,
                    Length,
                ),
                sheet(
                    "5 x 5 Baltic birch, 1/2",
                    60.0,
                    60.0,
                    "1/2",
                    12.0 / 25.4,
                    Length,
                ),
                sheet("4 x 8 MDF, 3/4", 48.0, 96.0, "3/4", 0.75, None),
                board("4/4", 4.0, 96.0),
                board("4/4", 6.0, 96.0),
                board("4/4", 8.0, 96.0),
                board("4/4", 12.0, 96.0),
                board("5/4", 6.0, 96.0),
                board("6/4", 6.0, 96.0),
                board("8/4", 6.0, 96.0),
                board("12/4", 6.0, 96.0),
            ],
            defaults: BTreeMap::new(),
            settings: NestSettings::default(),
        }
    }

    /// The stock named `name` (without case), with its index.
    pub fn find(&self, name: &str) -> Option<(usize, &StockDef)> {
        self.stocks
            .iter()
            .enumerate()
            .find(|(_, s)| same_name(&s.name, name))
    }

    /// The default stock for a material, when one is set (without case).
    pub fn default_for(&self, material: &str) -> Option<&str> {
        self.defaults
            .iter()
            .find(|(m, _)| same_name(m, material))
            .map(|(_, s)| s.as_str())
    }

    /// Every rule a saved or edited library must keep.
    pub fn check(&self) -> Result<(), StockError> {
        if self.stocks.len() > MAX_STOCKS {
            return Err(StockError::TooMany);
        }
        for (i, s) in self.stocks.iter().enumerate() {
            s.check()?;
            if s.name != s.name.trim() {
                return Err(StockError::BadName);
            }
            if self.stocks[..i].iter().any(|o| same_name(&o.name, &s.name)) {
                return Err(StockError::Exists(s.name.clone()));
            }
        }
        for (i, (m, s)) in self.defaults.iter().enumerate() {
            check_name(m)?;
            if m != m.trim()
                || self
                    .defaults
                    .keys()
                    .take(i)
                    .any(|other| same_name(other, m))
            {
                return Err(StockError::BadName);
            }
            match self.find(s) {
                Some((_, d)) if d.name == *s => {}
                _ => return Err(StockError::NoSuchStock(s.clone())),
            }
        }
        self.settings.check()
    }

    /// Add `stock`, or replace the stock named `replace` with it (a rename carries the
    /// per-material defaults along).
    pub fn define(&mut self, stock: &StockDef, replace: Option<&str>) -> Result<(), StockError> {
        let mut s = stock.clone();
        s.name = s.name.trim().to_string();
        s.nominal = s
            .nominal
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        s.check()?;
        let at = match replace {
            Some(old) => Some(
                self.find(old)
                    .ok_or_else(|| StockError::NoSuchStock(old.trim().to_string()))?
                    .0,
            ),
            None => None,
        };
        if let Some((k, _)) = self.find(&s.name)
            && Some(k) != at
        {
            return Err(StockError::Exists(s.name));
        }
        match at {
            Some(k) => {
                let old = std::mem::replace(&mut self.stocks[k], s.clone()).name;
                for v in self.defaults.values_mut() {
                    if *v == old {
                        *v = s.name.clone();
                    }
                }
            }
            None => {
                if self.stocks.len() >= MAX_STOCKS {
                    return Err(StockError::TooMany);
                }
                self.stocks.push(s);
            }
        }
        Ok(())
    }

    /// Remove a stock; materials that defaulted to it go back to the automatic choice.
    pub fn delete(&mut self, name: &str) -> Result<(), StockError> {
        let (k, _) = self
            .find(name)
            .ok_or_else(|| StockError::NoSuchStock(name.trim().to_string()))?;
        let gone = self.stocks.remove(k).name;
        self.defaults.retain(|_, v| *v != gone);
        Ok(())
    }

    /// Say which stock a material's parts come from (`None`: choose automatically).
    pub fn set_default(&mut self, material: &str, stock: Option<&str>) -> Result<(), StockError> {
        check_name(material)?;
        let key = self
            .defaults
            .keys()
            .find(|m| same_name(m, material))
            .cloned()
            .unwrap_or_else(|| material.trim().to_string());
        match stock {
            Some(s) => {
                let name = self
                    .find(s)
                    .ok_or_else(|| StockError::NoSuchStock(s.trim().to_string()))?
                    .1
                    .name
                    .clone();
                self.defaults.insert(key, name);
            }
            None => {
                self.defaults.remove(&key);
            }
        }
        Ok(())
    }

    pub(crate) fn hash_into(&self, h: &mut StableHasher) {
        h.tag("stock-library");
        h.usize(self.stocks.len());
        for s in &self.stocks {
            s.hash_into(h);
        }
        h.usize(self.defaults.len());
        for (m, s) in &self.defaults {
            h.tag(m);
            h.tag(s);
        }
        self.settings.hash_into(h);
    }

    /// A hash of the library alone (the nesting's hash includes it).
    pub fn hash_value(&self) -> u64 {
        let mut h = StableHasher::default();
        self.hash_into(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_library_is_valid_and_named_plainly() {
        let l = StockLibrary::builtin();
        l.check().unwrap();
        assert!(l.find("4 x 8 SHEET, 3/4").is_some(), "names ignore case");
        let b = l.find("4/4 x 12 x 96").unwrap().1;
        assert_eq!(b.kind, StockKind::Board);
        assert_eq!(b.thickness, 0.75 * M_PER_IN, "4/4 S4S is 3/4 in");
        let ply = l.find("4 x 8 sheet, 3/4").unwrap().1;
        assert_eq!(ply.thickness, 23.0 / 32.0 * M_PER_IN, "3/4 ply is 23/32 in");
        assert_eq!(ply.nominal.as_deref(), Some("3/4"));
        assert_eq!(
            l.find("4 x 8 sheet, 1/2").unwrap().1.thickness,
            15.0 / 32.0 * M_PER_IN
        );
        assert_eq!(
            l.find("4 x 8 sheet, 1/4").unwrap().1.thickness,
            7.0 / 32.0 * M_PER_IN
        );
        assert_eq!(
            l.find("5/4 x 6 x 96").unwrap().1.thickness,
            1.0625 * M_PER_IN
        );
        assert_eq!(standard_actual(StockKind::Board, " 8/4 "), Some(1.75));
        assert_eq!(standard_actual(StockKind::Sheet, "thick"), None);
        assert_eq!(b.board_feet(), 8.0);
        assert_eq!(l.find("8/4 x 6 x 96").unwrap().1.board_feet(), 8.0);
        assert_eq!(l.find("4 x 8 sheet, 1/4").unwrap().1.board_feet(), 0.0);
        // Every name is plain ASCII: it becomes a DXF layer and a PDF heading.
        assert!(l.stocks.iter().all(|s| s.name.is_ascii()));
    }

    #[test]
    fn define_replace_delete_and_defaults() {
        let mut l = StockLibrary::builtin();
        let mut s = l.stocks[0].clone();
        s.name = "  Shop ply  ".into();
        l.define(&s, None).unwrap();
        assert_eq!(l.stocks.last().unwrap().name, "Shop ply");
        assert_eq!(
            l.define(&s, None),
            Err(StockError::Exists("Shop ply".into()))
        );
        l.set_default("Oak", Some("shop PLY")).unwrap();
        assert_eq!(l.default_for("oak"), Some("Shop ply"));
        // A rename carries the default.
        s.name = "Shop ply 3/4".into();
        l.define(&s, Some("Shop ply")).unwrap();
        assert_eq!(l.default_for("Oak"), Some("Shop ply 3/4"));
        l.check().unwrap();
        l.delete("shop ply 3/4").unwrap();
        assert_eq!(l.default_for("Oak"), None);
        assert_eq!(
            l.delete("nothing"),
            Err(StockError::NoSuchStock("nothing".into()))
        );
        assert_eq!(
            l.set_default("Oak", Some("nothing")),
            Err(StockError::NoSuchStock("nothing".into()))
        );
        let mut bad = l.stocks[0].clone();
        bad.thickness = 2.0;
        assert!(l.define(&bad, Some(&l.stocks[0].name.clone())).is_err());
        bad.thickness = f64::NAN;
        assert!(bad.check().is_err());
        let st = NestSettings {
            kerf: -1.0,
            ..NestSettings::default()
        };
        assert!(st.check().is_err());
        let st = NestSettings {
            kerf: MAX_KERF,
            trim: MAX_TRIM + 1e-3,
            ..NestSettings::default()
        };
        assert!(st.check().is_err());
    }

    #[test]
    fn nominal_thickness() {
        let l = StockLibrary::builtin();
        assert_eq!(l.find("5/4 x 6 x 96").unwrap().1.nominal_inches(), 1.25);
        let mut s = l.stocks[0].clone();
        assert_eq!(s.nominal_inches(), 0.75, "\"3/4\" ply is bought as 3/4");
        s.nominal = Some("thick".into());
        assert_eq!(
            s.nominal_inches(),
            23.0 / 32.0,
            "an unreadable nominal: the actual"
        );
    }
}
