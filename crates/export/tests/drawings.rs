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

/// Lane 26 (issue #6): a scene's grid switch goes into the style its copy of the model
/// shows, and a drawing never draws the ground grid, so a scene drawn with the grid hidden
/// or shown is the same drawing as the scene without the switch (the sheet goldens keep
/// their hashes).
#[test]
fn a_scene_s_grid_switch_does_not_change_its_drawing() {
    let mut m = golden("section");
    let base = m
        .scenes()
        .iter()
        .find(|s| s.name == "Section A")
        .unwrap()
        .clone();
    assert_eq!(base.grid, None, "the golden's scene predates the switch");
    for (name, grid) in [("Grid off", false), ("Grid on", true)] {
        let mut sc = base.clone();
        sc.name = name.into();
        sc.grid = Some(grid);
        m.apply(&Op::AddScene { scene: sc }).unwrap();
    }
    let before = m.hash();
    let draw_of = |name: &str, view: StdView| {
        draw_scene(&m, Some(name), &std(view), &DrawOptions { hidden: true }).unwrap()
    };
    for view in [StdView::Left, StdView::Front, StdView::Top] {
        let want = draw_of("Section A", view);
        for name in ["Grid off", "Grid on"] {
            let got = draw_of(name, view);
            assert_eq!(got.hash, want.hash, "{name} {view:?}");
            assert_eq!(got.lines, want.lines, "{name} {view:?}");
        }
    }
    assert_eq!(m.hash(), before, "drawing a scene changes nothing");
    assert!(m.style().is_none_or(|s| s.grid));
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

/// Random sheets on the section golden (both planes, scenes with and without them): every
/// item the kernel accepts lays out with finite primitives, deterministically, and goes
/// out as DXF. A small seeded generator, no dependencies.
#[test]
fn random_sheets_lay_out() {
    use sawhorse_export::sheet::{Prim, RenderOptions, render};
    use sawhorse_export::sheet_dxf::dxf;
    use sawhorse_kernel::paper::{CameraSpec, DimKind, Scale, StdView};
    use sawhorse_kernel::sheets::{ItemKind, LeaderEnd, PaperUnits, SheetSetup, ViewportSource};
    let base = golden("section");
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut laid = 0;
    for round in 0..40 {
        let mut m = base.clone();
        m.apply(&Op::AddSheet {
            setup: Box::new(SheetSetup::named(&format!("S{round}"))),
        })
        .unwrap();
        let sheet = m.sheets()[0].0;
        let scenes: Vec<String> = m.scenes().iter().map(|s| s.name.clone()).collect();
        for _ in 0..12 {
            let r = next();
            let f = |k: u64| (k % 1000) as f64 / 1000.0;
            let at = [0.01 + 0.25 * f(r >> 8), 0.01 + 0.19 * f(r >> 20)];
            let rect = [
                at[0],
                at[1],
                0.02 + 0.1 * f(r >> 32),
                0.02 + 0.08 * f(r >> 44),
            ];
            let viewports: Vec<u64> = m
                .sheet(sheet)
                .unwrap()
                .items
                .iter()
                .filter(|i| matches!(i.kind, ItemKind::Viewport { .. }))
                .map(|i| i.id)
                .collect();
            let kind = match r % 6 {
                0 | 1 => {
                    let views = [
                        StdView::Front,
                        StdView::Top,
                        StdView::Left,
                        StdView::Right,
                        StdView::Back,
                        StdView::Iso,
                    ];
                    let source = match (r >> 4) % 3 {
                        0 if !scenes.is_empty() => ViewportSource::Scene {
                            name: scenes[(r >> 5) as usize % scenes.len()].clone(),
                        },
                        1 => ViewportSource::Camera {
                            camera: CameraSpec {
                                eye: [2.0, -3.0, 1.5],
                                target: [0.4, 0.1, 0.8],
                                up: [0.0, 0.0, 1.0],
                                parallel: r & 64 == 0,
                                fov: 35.0,
                            },
                        },
                        _ => ViewportSource::Standard {
                            view: views[(r >> 6) as usize % 6],
                        },
                    };
                    let scale = if r & 128 == 0 {
                        Scale::Nts
                    } else {
                        Scale::Ratio {
                            paper: 1.0,
                            model: [4.0, 8.0, 12.0, 24.0][(r >> 9) as usize % 4],
                        }
                    };
                    ItemKind::Viewport {
                        rect,
                        source,
                        scale,
                        centre: (r & 256 != 0).then_some([0.3, 0.5]),
                        hidden_lines: r & 512 != 0,
                        caption: Some("VIEW".into()),
                        border: r & 1024 != 0,
                        ortho_scale: None,
                    }
                }
                2 if !viewports.is_empty() => {
                    let dims = [
                        DimKind::Horizontal,
                        DimKind::Vertical,
                        DimKind::Aligned,
                        DimKind::Angular,
                        DimKind::Radius,
                        DimKind::Diameter,
                    ];
                    let dim = dims[(r >> 3) as usize % 6];
                    let p = |k: u64| {
                        [
                            0.9144 * f(r >> k),
                            0.28 * f(r >> (k + 7)),
                            1.83 * f(r >> (k + 14)),
                        ]
                    };
                    ItemKind::Dimension {
                        viewport: viewports[(r >> 2) as usize % viewports.len()],
                        dim,
                        points: [p(5), p(17), p(29)][..dim.points()].to_vec(),
                        anchors: Vec::new(),
                        offset: 0.02 * (f(r >> 40) - 0.5),
                        style: Default::default(),
                        text: None,
                    }
                }
                3 => ItemKind::Label {
                    at,
                    text: "Back \u{d8} 1/4\" at 45\u{b0}\nsecond line".into(),
                    size: 0.003,
                    leader: viewports.first().map(|v| LeaderEnd::Model {
                        viewport: *v,
                        point: [0.45, 0.1, 0.9],
                        anchor: None,
                    }),
                },
                4 => ItemKind::Table {
                    rect,
                    columns: sawhorse_kernel::paper::CutColumn::default_set(),
                    decimal: r & 8 != 0,
                    visible_only: r & 16 != 0,
                    title: Some("CUT LIST".into()),
                },
                _ => ItemKind::Line {
                    a: at,
                    b: [rect[0] + rect[2], rect[1] + rect[3]],
                    width: 0.0005,
                    dashed: r & 4 != 0,
                },
            };
            let _ = m.apply(&Op::AddSheetItem {
                sheet,
                item: Box::new(kind),
            });
        }
        let r = render(
            &m,
            sheet,
            &RenderOptions {
                number: 1,
                count: 1,
            },
        )
        .unwrap();
        let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
        for p in &r.prims {
            let ok = match p {
                Prim::Line { a, b, width, .. } => finite(a) && finite(b) && width.is_finite(),
                Prim::Fill { rings, .. } => rings.iter().flatten().all(|q| finite(q)),
                Prim::Rect { rect, .. } | Prim::Image { rect, .. } => finite(rect),
                Prim::Text { at, size, .. } => finite(at) && size.is_finite(),
            };
            assert!(ok, "round {round}: {p:?}");
        }
        for it in &r.items {
            assert!(
                finite(&it.bounds) && it.prims[0] <= it.prims[1] && it.prims[1] <= r.prims.len(),
                "{it:?}"
            );
        }
        assert_eq!(
            render(
                &m,
                sheet,
                &RenderOptions {
                    number: 1,
                    count: 1
                }
            )
            .unwrap(),
            r
        );
        assert!(dxf(&r, PaperUnits::Millimetres).ends_with("EOF\n"));
        laid += r.items.len();
    }
    assert!(laid > 200, "{laid}");
}

/// Lane 12c: a dimension in an isometric or perspective viewport reads the model's own
/// length (or "NTS" for a horizontal or vertical one), never the foreshortened length the
/// view shows; the same points in a Front viewport read what Front shows.
#[test]
fn dimensions_in_views_not_to_scale_read_the_model() {
    use sawhorse_export::sheet::{RenderOptions, render};
    use sawhorse_kernel::paper::{CameraSpec, StdView as View};
    use sawhorse_kernel::sheets::{DimStyle, ItemKind, SheetSetup, ViewportSource};
    let mut d = {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../kernel/tests/golden/cutlist.json");
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
        Document::replay(&ops).unwrap()
    };
    let sheet = match d
        .apply(&Op::AddSheet {
            setup: Box::new(SheetSetup::named("Dims")),
        })
        .unwrap()
    {
        sawhorse_kernel::model::OpOutcome::SheetAdded { sheet } => sheet,
        o => panic!("{o:?}"),
    };
    let add = |d: &mut Document, kind: ItemKind| match d
        .apply(&Op::AddSheetItem {
            sheet,
            item: Box::new(kind),
        })
        .unwrap()
    {
        sawhorse_kernel::model::OpOutcome::SheetItemAdded { item, .. } => item,
        o => panic!("{o:?}"),
    };
    let vp = |source: ViewportSource, scale: Scale, x: f64| ItemKind::Viewport {
        rect: [x * IN, 0.5 * IN, 3.0 * IN, 3.0 * IN],
        source,
        scale,
        centre: None,
        hidden_lines: false,
        caption: None,
        border: false,
        ortho_scale: None,
    };
    let front = add(
        &mut d,
        vp(
            ViewportSource::Standard { view: View::Front },
            Scale::Ratio {
                paper: 1.0,
                model: 24.0,
            },
            0.5,
        ),
    );
    let iso = add(
        &mut d,
        vp(
            ViewportSource::Standard { view: View::Iso },
            Scale::Nts,
            3.75,
        ),
    );
    let persp = add(
        &mut d,
        vp(
            ViewportSource::Camera {
                camera: CameraSpec {
                    eye: [3.0, -3.0, 2.0],
                    target: [0.4, 0.0, 0.8],
                    up: [0.0, 0.0, 1.0],
                    parallel: false,
                    fov: 35.0,
                },
            },
            Scale::Nts,
            7.0,
        ),
    );
    // A 30 in edge along red, and a point 12 in back: a 30 in by 12 in diagonal.
    let (a, b) = ([0.0, 0.0, 0.0], [30.0 * IN, 0.0, 0.0]);
    let c = [30.0 * IN, 12.0 * IN, 0.0];
    let dim = |viewport: u64, kind: DimKind, points: Vec<[f64; 3]>| ItemKind::Dimension {
        viewport,
        dim: kind,
        points,
        anchors: Vec::new(),
        offset: 0.2 * IN,
        style: DimStyle::Arrows,
        text: None,
    };
    let mut want = Vec::new();
    for (v, square) in [(front, true), (iso, false), (persp, false)] {
        let al = add(&mut d, dim(v, DimKind::Aligned, vec![a, b]));
        let diag = add(&mut d, dim(v, DimKind::Aligned, vec![a, c]));
        let h = add(&mut d, dim(v, DimKind::Horizontal, vec![a, b]));
        let ang = add(&mut d, dim(v, DimKind::Angular, vec![b, a, c]));
        want.push((al, "30\"".to_string()));
        want.push((
            h,
            if square {
                "30\"".to_string()
            } else {
                "NTS".to_string()
            },
        ));
        // Front sees the diagonal edge-on (its depth does not show): 30 in and 0 degrees.
        // The model has 32 5/16 in and 90 degrees; the other two views read exactly that.
        let len = (30.0f64.hypot(12.0)) * IN;
        if square {
            want.push((diag, "30\"".to_string()));
        } else {
            want.push((diag, format_length(len, LengthFormat::Fraction { den: 16 })));
            want.push((ang, "90°".to_string()));
        }
    }
    let (id, _) = d.model().sheets()[0];
    let r = render(
        d.model(),
        id,
        &RenderOptions {
            number: 1,
            count: 1,
        },
    )
    .unwrap();
    for (item, text) in want {
        let got = r
            .items
            .iter()
            .find(|i| i.id == item)
            .and_then(|i| i.text.clone());
        assert_eq!(got.as_deref(), Some(text.as_str()), "item {item}");
    }
}
