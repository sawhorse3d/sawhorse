//! Drawings for sheets on real models (lane 12): the component bookshelf drawn front, plan
//! and side, to scale; a dimension on the drawing's own model points; the cut list as a
//! table; and a section seen from the cut side.

use glam::DVec3;
use sawhorse_export::drawing::{DrawOptions, LineKind, StdView, ViewSource, draw, draw_scene};
use sawhorse_export::table::cut_table;
use sawhorse_kernel::Document;
use sawhorse_kernel::cutlist::CutOptions;
use sawhorse_kernel::model::{Model, Op};
use sawhorse_kernel::paper::{CutColumn, DimKind, LengthFormat, Scale, format_length, measure};

const IN: f64 = 0.0254;

fn golden(name: &str) -> Model {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../kernel/tests/golden/{name}.json"));
    let g: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
    Document::replay(&ops).unwrap().model().clone()
}

fn std(view: StdView) -> ViewSource {
    ViewSource::Standard { view }
}

#[test]
fn a_thirty_inch_cabinet_at_one_to_eight() {
    let mut m = Model::with_ground_sketch();
    m.apply(&Op::AddRectangle {
        sketch: 1,
        x: 0.0,
        y: 0.0,
        w: 30.0 * IN,
        h: 16.0 * IN,
    })
    .unwrap();
    m.apply(&Op::Extrude {
        sketch: 1,
        region: 1,
        distance: 34.5 * IN,
    })
    .unwrap();
    let d = draw(&m, &std(StdView::Front), &DrawOptions::default()).unwrap();
    let b = d.bounds.unwrap();
    let width = b[2] - b[0];
    assert!((width - 30.0 * IN).abs() < 1e-12);
    let paper = |s: Scale| width * s.factor().unwrap() / IN;
    assert!(
        (paper(Scale::Ratio {
            paper: 1.0,
            model: 8.0
        }) - 3.75)
            .abs()
            < 1e-12
    );
    assert!(
        (paper(Scale::Architect {
            inches_per_foot: 0.25
        }) - 0.625)
            .abs()
            < 1e-12
    );
    // A dimension across the bottom edge, on the drawing's own model points.
    let bottom = d
        .lines
        .iter()
        .find(|l| l.a[1].abs() < 1e-12 && l.b[1].abs() < 1e-12)
        .unwrap();
    let (r, u) = (
        DVec3::from_array(d.frame.right),
        DVec3::from_array(d.frame.up),
    );
    let w = measure(
        r,
        u,
        DimKind::Horizontal,
        &[DVec3::from_array(bottom.a3), DVec3::from_array(bottom.b3)],
    )
    .unwrap();
    assert_eq!(format_length(w, LengthFormat::Fraction { den: 16 }), "30\"");
    // The same dimension at 1:1 and 1:48: the model's length, not the paper's.
    assert!((w - 30.0 * IN).abs() < 1e-12);
}

#[test]
fn the_bookshelf_front_plan_and_side() {
    let m = golden("cutlist");
    let front = draw(&m, &std(StdView::Front), &DrawOptions::default()).unwrap();
    let b = front.bounds.unwrap();
    // 36 in wide, 72 in tall (plus the mirrored side copy at 60 in from the golden).
    assert!((b[3] - b[1] - 72.0 * IN).abs() < 1e-9, "{b:?}");
    let plan = draw(&m, &std(StdView::Top), &DrawOptions::default()).unwrap();
    let p = plan.bounds.unwrap();
    assert!((p[3] - p[1] - 11.25 * IN).abs() < 1e-9, "{p:?}");
    // Deterministic, and the visible lines never cross a face in front of them: drawing
    // with hidden lines adds lines but never changes the visible ones.
    assert_eq!(
        draw(&m, &std(StdView::Front), &DrawOptions::default())
            .unwrap()
            .hash,
        front.hash
    );
    let with_hidden = draw(&m, &std(StdView::Front), &DrawOptions { hidden: true }).unwrap();
    let visible: Vec<_> = with_hidden
        .lines
        .iter()
        .filter(|l| l.kind != LineKind::Hidden)
        .collect();
    assert_eq!(visible.len(), front.lines.len());
    assert!(with_hidden.lines.iter().any(|l| l.kind == LineKind::Hidden));
}

#[test]
fn the_section_golden_seen_from_the_cut_side() {
    let m = golden("section");
    // The scene "Section A": the plane at 12 in keeping +X; looking from -X at the cut.
    let d = draw_scene(
        &m,
        Some("Section A"),
        &std(StdView::Left),
        &DrawOptions::default(),
    )
    .unwrap();
    assert!(!d.fills.is_empty(), "the cut faces the eye: filled");
    let cut = d.lines.iter().filter(|l| l.kind == LineKind::Cut).count();
    assert!(
        cut >= 24,
        "the cut outlines of shelves, top, toe kick and back: {cut}"
    );
    // Seen from the other side, the caps face away: no fill.
    let r = draw_scene(
        &m,
        Some("Section A"),
        &std(StdView::Right),
        &DrawOptions::default(),
    )
    .unwrap();
    assert!(r.fills.is_empty());
    // An unknown scene is refused.
    assert!(
        draw_scene(
            &m,
            Some("Nope"),
            &std(StdView::Left),
            &DrawOptions::default()
        )
        .is_err()
    );
}

#[test]
fn the_cut_list_as_a_table() {
    let m = golden("cutlist");
    let list = m.cut_list(&CutOptions::default());
    let t = cut_table(&list, &CutColumn::default_set(), false);
    assert_eq!(
        t.header,
        [
            "Part",
            "Qty",
            "T",
            "W",
            "L",
            "Material",
            "Bd ft / area",
            "Notes"
        ]
    );
    let side = t.rows.iter().find(|r| r[0] == "Side").unwrap();
    assert_eq!(&side[1..5], ["3", "3/4", "11 1/4", "71 1/4"]);
    assert!(side[7].contains("handed"), "{side:?}");
    let back = t.rows.iter().find(|r| r[0] == "Back panel").unwrap();
    assert!(
        back[5].contains("(sheet)") && back[6].ends_with("sq ft"),
        "{back:?}"
    );
    let shelf = t.rows.iter().find(|r| r[0] == "Shelf").unwrap();
    assert!(shelf[7].contains("1 hidden"), "{shelf:?}");
    assert_eq!(t.unit, "in");
    let d = cut_table(&list, &[CutColumn::Part, CutColumn::Size], true);
    assert_eq!(
        d.rows.iter().find(|r| r[0] == "Side").unwrap()[1],
        "0.75 x 11.25 x 71.25"
    );
}

/// Every golden with sheets: each sheet's render hash, written under `UPDATE_GOLDEN=1`
/// (after `--test golden` in the kernel) and checked otherwise; `scripts/golden-wasm.mjs`
/// checks the WASM build draws the same.
#[test]
fn sheet_goldens() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kernel/tests/golden");
    let update = std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut seen = 0;
    for f in files {
        let mut g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
        let m = Document::replay(&ops).unwrap().model().clone();
        let sheets = m.sheets();
        if sheets.is_empty() {
            assert!(g.get("renders").is_none(), "{}", f.display());
            continue;
        }
        seen += 1;
        let n = sheets.len();
        let hashes: Vec<String> = sheets
            .iter()
            .enumerate()
            .map(|(k, (id, _))| {
                sawhorse_export::sheet::render(
                    &m,
                    *id,
                    &sawhorse_export::sheet::RenderOptions {
                        number: k + 1,
                        count: n,
                    },
                )
                .unwrap()
                .hash
            })
            .collect();
        if update {
            // Only the "renders" entry changes: the rest of the file keeps the order the
            // kernel's golden test writes it in.
            let text = std::fs::read_to_string(&f).unwrap();
            let entry = format!(
                "\"renders\": {}",
                serde_json::to_string_pretty(&hashes)
                    .unwrap()
                    .replace('\n', "\n  ")
            );
            let text = match text.find("\"renders\":") {
                Some(at) => {
                    let end = at + text[at..].find(']').unwrap() + 1;
                    format!("{}{}{}", &text[..at], entry, &text[end..])
                }
                None => {
                    let close = text.rfind('}').unwrap();
                    format!("{},\n  {}\n}}\n", text[..close].trim_end(), entry)
                }
            };
            std::fs::write(&f, text).unwrap();
            g = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
            assert_eq!(g["renders"], serde_json::json!(hashes));
        } else {
            assert_eq!(
                g["renders"],
                serde_json::json!(hashes),
                "{}: sheet renders drifted (UPDATE_GOLDEN=1)",
                f.display()
            );
        }
    }
    assert!(seen >= 1);
}

#[test]
fn the_sheet_golden_lays_out() {
    use sawhorse_export::sheet::{Prim, RenderOptions, render};
    let m = golden("sheet");
    let (id, _) = m.sheets()[0];
    let r = render(
        &m,
        id,
        &RenderOptions {
            number: 1,
            count: 1,
        },
    )
    .unwrap();
    assert!((r.size[0] - 11.0 * IN).abs() < 1e-12 && (r.size[1] - 8.5 * IN).abs() < 1e-12);
    let texts: Vec<&str> = r
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for want in [
        "36\"",
        "72\"",
        "17 1/4\"",
        "FRONT ELEVATION 1:12",
        "PLAN 1:12",
        "SECTION A 1:4",
        "Back panel 1/4 ply in a rabbet",
        "CUT LIST",
        "Side",
        "Bookshelf",
        "1 of 1",
        "As noted",
        "2026-10-12",
        "Shop drawings",
    ] {
        assert!(texts.contains(&want), "missing {want:?} in {texts:?}");
    }
    // Every dimension reports its text; the viewports are at their scales.
    let dims: Vec<&str> = r
        .items
        .iter()
        .filter(|i| i.kind == "dimension")
        .filter_map(|i| i.text.as_deref())
        .collect();
    assert_eq!(dims, ["36\"", "72\"", "17 1/4\""]);
    let f: Vec<f64> = r.viewports.iter().map(|v| v.factor).collect();
    assert!(
        (f[0] - 1.0 / 12.0).abs() < 1e-15 && (f[2] - 0.25).abs() < 1e-15,
        "{f:?}"
    );
    // The front elevation: a 6.8 in frame (72 in tall is 6 in on paper) and its caption.
    let front = &r.items[0];
    assert!(
        front.bounds[3] > 6.8 * IN && front.bounds[3] < 7.2 * IN,
        "{:?}",
        front.bounds
    );
    // Nothing overlaps: the items' boxes are apart (the label's leader reaches into the
    // elevation, which is what it is for).
    let boxes: Vec<_> = r
        .items
        .iter()
        .filter(|i| i.kind == "viewport" || i.kind == "table")
        .map(|i| i.bounds)
        .collect();
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            let apart = a[0] + a[2] <= b[0]
                || b[0] + b[2] <= a[0]
                || a[1] + a[3] <= b[1]
                || b[1] + b[3] <= a[1];
            assert!(apart, "{a:?} overlaps {b:?}");
        }
    }
    // The section is filled where it is cut.
    assert!(r.prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == sawhorse_export::section::DEFAULT_FILL)));
    // Deterministic.
    assert_eq!(
        render(
            &m,
            id,
            &RenderOptions {
                number: 1,
                count: 1
            }
        )
        .unwrap(),
        r
    );
}

#[test]
fn the_sheet_golden_as_dxf() {
    use sawhorse_export::dxf::pairs;
    use sawhorse_export::sheet::{RenderOptions, fit_text, render, text_width};
    use sawhorse_export::sheet_dxf::dxf;
    use sawhorse_kernel::sheets::PaperUnits;
    let m = golden("sheet");
    let (id, _) = m.sheets()[0];
    let r = render(
        &m,
        id,
        &RenderOptions {
            number: 1,
            count: 1,
        },
    )
    .unwrap();
    let text = dxf(&r, PaperUnits::Inches);
    assert!(text.ends_with("  0\nEOF\n"));
    let p = pairs(&text);
    let header: Vec<_> = p
        .windows(2)
        .filter(|w| w[0].0 == 9)
        .map(|w| (w[0].1.clone(), w[1].1.clone()))
        .collect();
    assert!(
        header.contains(&("$INSUNITS".into(), "1".into())),
        "{header:?}"
    );
    // Every entity is a LINE, an LWPOLYLINE or a TEXT: no DIMENSION entities.
    let kinds: std::collections::BTreeSet<&str> = p
        .iter()
        .skip_while(|(c, v)| !(*c == 2 && v == "ENTITIES"))
        .filter(|(c, _)| *c == 0)
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(
        kinds,
        ["ENDSEC", "EOF", "LINE", "LWPOLYLINE", "TEXT"]
            .into_iter()
            .collect()
    );
    // The dimension strings are text on the DIMENSIONS layer.
    let texts: Vec<&str> = p
        .iter()
        .filter(|(c, _)| *c == 1)
        .map(|(_, v)| v.as_str())
        .collect();
    for want in [
        "36\"",
        "72\"",
        "17 1/4\"",
        "CUT LIST",
        "FRONT ELEVATION 1:12",
        "Bookshelf",
    ] {
        assert!(texts.contains(&want), "missing {want:?}");
    }
    // Everything on the paper: x in 0..11, y in 0..8.5 (inches).
    for (c, v) in &p {
        let v: f64 = match v.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if *c == 10 || *c == 11 {
            assert!((-1e-6..=11.0 + 1e-6).contains(&v), "x {v}");
        }
        if *c == 20 || *c == 21 {
            assert!((-1e-6..=8.5 + 1e-6).contains(&v), "y {v}");
        }
    }
    // The overall width dimension's text sits under the elevation (y up: low on the page).
    let at = p.iter().position(|(c, v)| *c == 1 && v == "36\"").unwrap();
    let y = p[..at]
        .iter()
        .rev()
        .find(|(c, _)| *c == 20)
        .unwrap()
        .1
        .parse::<f64>()
        .unwrap();
    assert!(y < 1.5, "{y}");
    // The section's cut is outlined on its own layer; layers are declared.
    assert!(p.windows(2).any(|w| w[0] == (8, "SECTION".into())));
    // In millimetres: the same picture, 25.4 times larger.
    let mm = pairs(&dxf(&r, PaperUnits::Millimetres));
    let max_x = mm
        .iter()
        .filter(|(c, _)| *c == 10)
        .filter_map(|(_, v)| v.parse::<f64>().ok())
        .fold(0.0, f64::max);
    assert!(max_x > 250.0 && max_x <= 279.4 + 1e-6, "{max_x}");
    // Deterministic.
    assert_eq!(dxf(&r, PaperUnits::Inches), text);
    // Fitting text as the PDF does.
    assert_eq!(fit_text("Back panel", 1.0, 100.0, false), "Back panel");
    let short = fit_text("Back panel 1/4 ply in a rabbet", 1.0, 5.0, false);
    assert!(
        short.ends_with("...") && text_width(&short, 1.0, false) <= 5.0,
        "{short}"
    );
}
