//! The cut list as a table on a sheet (lane 12): the same rows and the same words as the
//! Cut list pane, its report and its PDF (lane 7, `src/cutlist/format.ts`), in the columns
//! the table item chooses. Sizes come from the kernel's rounding steps, so every output
//! writes exactly the same fractions.

use sawhorse_kernel::cutlist::{CutList, CutRow, Grain, Rounding};
use sawhorse_kernel::paper::CutColumn;

/// A table: its headings, its rows of cells, the unit the sizes are in, and its footnote.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Each column's share of the width (from [`CutColumn::weight`]).
    pub weights: Vec<f64>,
    /// "in" or "mm".
    pub unit: String,
    pub note: String,
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// A size in rounding steps as a woodworker writes it ("11 1/4", "3/4", "72"; "19.5" mm;
/// "11.25" in the decimal style), exactly as `formatSteps` in `src/cutlist/format.ts`.
pub fn format_steps(steps: i64, r: Rounding, decimal: bool) -> String {
    if r == Rounding::HalfMm {
        let mm = steps as f64 / 2.0;
        return if steps % 2 == 0 {
            format!("{}", steps / 2)
        } else {
            format!("{mm:.1}")
        };
    }
    let den: i64 = if r == Rounding::Sixteenth { 16 } else { 32 };
    if decimal {
        let v = steps as f64 / den as f64;
        let s = format!("{v:.5}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        return if s.is_empty() || s == "-0" {
            "0".into()
        } else {
            s.to_string()
        };
    }
    let neg = steps < 0;
    let n = steps.abs();
    let (whole, num) = (n / den, n % den);
    let text = if num == 0 {
        format!("{whole}")
    } else {
        let g = gcd(num, den);
        let frac = format!("{}/{}", num / g, den / g);
        if whole == 0 {
            frac
        } else {
            format!("{whole} {frac}")
        }
    };
    if neg { format!("-{text}") } else { text }
}

const SQ_FT: f64 = 0.092_903_04;

fn amount(row: &CutRow, r: Rounding) -> String {
    if row.sheet {
        if r == Rounding::HalfMm {
            format!("{:.2} m²", row.area_m2)
        } else {
            format!("{:.2} sq ft", row.area_m2 / SQ_FT)
        }
    } else {
        format!("{:.2} bd ft", row.board_feet)
    }
}

fn grain_words(g: Grain) -> &'static str {
    match g {
        Grain::Length => "along L",
        Grain::Width => "across W",
        Grain::None => "none",
    }
}

/// The row's notes, as `rowNotes` writes them.
pub fn notes(row: &CutRow) -> String {
    let mut n: Vec<String> = Vec::new();
    if row.grain_set {
        n.push(if row.grain == Grain::None {
            "no grain".into()
        } else {
            format!("grain {}", grain_words(row.grain))
        });
    }
    if row.shaped {
        n.push("shaped".into());
    }
    if row.handed > 0 {
        n.push(if row.handed == row.total {
            "handed".into()
        } else {
            format!("{} of {} handed", row.handed, row.total)
        });
    }
    if row.hidden > 0 {
        n.push(if row.hidden == row.total {
            "hidden".into()
        } else {
            format!("{} hidden", row.hidden)
        });
    }
    if row.scaled {
        n.push("scaled".into());
    }
    n.join("; ")
}

/// The cut list as a table in `columns` (see the module docs). `decimal`: decimal inches
/// instead of fractions (inch roundings only).
pub fn cut_table(list: &CutList, columns: &[CutColumn], decimal: bool) -> Table {
    let r = list.rounding;
    let size = |s: i64| format_steps(s, r, decimal);
    let rows = list
        .rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .map(|c| match c {
                    CutColumn::Part => row.name.clone(),
                    CutColumn::Qty => row.qty.to_string(),
                    CutColumn::Thickness => size(row.steps[0]),
                    CutColumn::Width => size(row.steps[1]),
                    CutColumn::Length => size(row.steps[2]),
                    CutColumn::Size => row.steps.map(size).join(" x "),
                    CutColumn::Material => format!(
                        "{}{}",
                        row.material.as_deref().unwrap_or("—"),
                        if row.sheet { " (sheet)" } else { "" }
                    ),
                    CutColumn::Grain => grain_words(row.grain).to_string(),
                    CutColumn::Amount => amount(row, r),
                    CutColumn::Notes => notes(row),
                })
                .collect()
        })
        .collect();
    let step = match r {
        Rounding::Sixteenth => "1/16 in",
        Rounding::ThirtySecond => "1/32 in",
        Rounding::HalfMm => "0.5 mm",
    };
    Table {
        header: columns.iter().map(|c| c.header().to_string()).collect(),
        rows,
        weights: columns.iter().map(|c| c.weight()).collect(),
        unit: if r == Rounding::HalfMm { "mm" } else { "in" }.into(),
        note: format!(
            "Finished sizes in {}, to the nearest {step}. {} parts, {:.2} board feet{}.",
            if r == Rounding::HalfMm {
                "mm"
            } else {
                "inches"
            },
            list.totals.pieces,
            list.totals.board_feet,
            if list.totals.sheet_count > 0 {
                format!(
                    ", {} sheet{} (rough)",
                    list.totals.sheet_count,
                    if list.totals.sheet_count == 1 {
                        ""
                    } else {
                        "s"
                    }
                )
            } else {
                String::new()
            }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_like_the_cut_list_pane() {
        assert_eq!(format_steps(180, Rounding::Sixteenth, false), "11 1/4");
        assert_eq!(format_steps(12, Rounding::Sixteenth, false), "3/4");
        assert_eq!(format_steps(1152, Rounding::Sixteenth, false), "72");
        assert_eq!(format_steps(180, Rounding::Sixteenth, true), "11.25");
        assert_eq!(format_steps(1, Rounding::Sixteenth, true), "0.0625");
        assert_eq!(format_steps(39, Rounding::HalfMm, false), "19.5");
        assert_eq!(format_steps(38, Rounding::HalfMm, false), "19");
        assert_eq!(format_steps(-3, Rounding::ThirtySecond, false), "-3/32");
    }
}
