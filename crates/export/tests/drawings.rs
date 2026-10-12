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
