//! Drawing sheets (lane 12): sheets and their items as ops (add, set, move, reorder, delete,
//! undo), what is refused, scenes renamed and deleted under a viewport, a viewport deleted
//! under its dimensions, the `.shz` round trip, hashes only when a sheet exists, and the
//! `sheet.json` golden (written from the last test under `UPDATE_GOLDEN=1`): the cut
//! bookshelf on a Letter landscape sheet with elevations, a section, dimensions, a label,
//! the cut list and a title block.

use sawhorse_kernel::Document;
use sawhorse_kernel::model::{Model, Op, OpOutcome, SceneDef};
use sawhorse_kernel::paper::{CameraSpec, CutColumn, DimKind, Scale, StdView};
use sawhorse_kernel::sheets::{
    DimStyle, ItemKind, LeaderEnd, PaperSize, SheetSetup, TitleBlock, ViewportSource,
};

const IN: f64 = 0.0254;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn refused(d: &mut Document, op: Op) -> String {
    let before = d.hash();
    let e = d.apply(&op).expect_err("refused");
    assert!(!e.is_bug(), "{e}");
    assert_eq!(d.hash(), before);
    e.to_string()
}

fn golden_doc(name: &str) -> Document {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.json"));
    let g: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
    Document::replay(&ops).unwrap()
}

fn add_sheet(d: &mut Document, name: &str) -> u64 {
    match ok(
        d,
        Op::AddSheet {
            setup: Box::new(SheetSetup::named(name)),
        },
    ) {
        OpOutcome::SheetAdded { sheet } => sheet,
        o => panic!("{o:?}"),
    }
}

fn add_item(d: &mut Document, sheet: u64, item: ItemKind) -> u64 {
    match ok(
        d,
        Op::AddSheetItem {
            sheet,
            item: Box::new(item),
        },
    ) {
        OpOutcome::SheetItemAdded { item, .. } => item,
        o => panic!("{o:?}"),
    }
}

fn r(x: f64, y: f64, w: f64, h: f64) -> [f64; 4] {
    [x * IN, y * IN, w * IN, h * IN]
}

fn viewport(view: StdView, rect: [f64; 4], scale: Scale) -> ItemKind {
    ItemKind::Viewport {
        rect,
        source: ViewportSource::Standard { view },
        scale,
        centre: None,
        hidden_lines: false,
        caption: None,
        border: false,
    }
}

fn eighth() -> Scale {
    Scale::Ratio {
        paper: 1.0,
        model: 8.0,
    }
}

#[test]
fn sheets_and_items_are_undoable_ops() {
    let mut d = golden_doc("cutlist");
    let before = d.hash();
    let a = add_sheet(&mut d, "Elevations");
    let b = add_sheet(&mut d, "Details");
    assert_eq!(
        d.model()
            .sheets()
            .iter()
            .map(|(_, s)| s.setup.name.as_str())
            .collect::<Vec<_>>(),
        ["Elevations", "Details"]
    );
    ok(&mut d, Op::MoveSheet { sheet: b, to: 0 });
    assert_eq!(d.model().sheets()[0].0, b);
    let v = add_item(
        &mut d,
        a,
        viewport(
            StdView::Front,
            r(0.5, 0.5, 5.0, 7.0),
            Scale::Ratio {
                paper: 1.0,
                model: 12.0,
            },
        ),
    );
    let dim = add_item(
        &mut d,
        a,
        ItemKind::Dimension {
            viewport: v,
            dim: DimKind::Horizontal,
            points: vec![[0.0; 3], [36.0 * IN, 0.0, 0.0]],
            offset: -0.25 * IN,
            style: DimStyle::Ticks,
            text: None,
        },
    );
    let label = add_item(
        &mut d,
        a,
        ItemKind::Label {
            at: [6.0 * IN, 1.0 * IN],
            text: "White oak, oil finish".into(),
            size: 0.125 * IN,
            leader: Some(LeaderEnd::Model {
                viewport: v,
                point: [18.0 * IN, 0.0, 40.0 * IN],
            }),
        },
    );
    let line = add_item(
        &mut d,
        a,
        ItemKind::Line {
            a: [0.5 * IN, 8.0 * IN],
            b: [10.5 * IN, 8.0 * IN],
            width: 0.0005,
            dashed: false,
        },
    );
    assert_eq!((v, dim, label, line), (1, 2, 3, 4));
    // Move two items by a quarter inch; the dimension stays on its model points.
    ok(
        &mut d,
        Op::MoveSheetItems {
            sheet: a,
            items: vec![v, label],
            by: [0.25 * IN, 0.0],
        },
    );
    let s = d.model().sheet(a).unwrap();
    assert_eq!(s.item(v).unwrap().kind.rect().unwrap()[0], 0.75 * IN);
    // Resize the viewport; reorder the line to the back.
    let mut vk = s.item(v).unwrap().kind.clone();
    if let ItemKind::Viewport { rect, .. } = &mut vk {
        rect[2] = 6.0 * IN;
    }
    ok(
        &mut d,
        Op::SetSheetItem {
            sheet: a,
            item: v,
            kind: Box::new(vk),
        },
    );
    ok(
        &mut d,
        Op::OrderSheetItem {
            sheet: a,
            item: line,
            to: 0,
        },
    );
    assert_eq!(d.model().sheet(a).unwrap().items[0].id, line);
    // Deleting the viewport takes its dimension and drops the label's leader.
    ok(
        &mut d,
        Op::DeleteSheetItems {
            sheet: a,
            items: vec![v],
        },
    );
    let s = d.model().sheet(a).unwrap();
    assert!(s.item(dim).is_none());
    assert!(matches!(
        &s.item(label).unwrap().kind,
        ItemKind::Label { leader: None, .. }
    ));
    // New ids are never reused.
    let again = add_item(
        &mut d,
        a,
        viewport(StdView::Top, r(0.5, 0.5, 4.0, 2.0), eighth()),
    );
    assert_eq!(again, 5);
    // Undo everything back to the bookshelf alone: the hash it had.
    while d.model().sheet(a).is_some() || d.model().sheet(b).is_some() {
        d.undo().unwrap();
    }
    assert_eq!(d.hash(), before);
}

#[test]
fn what_is_refused() {
    let mut d = golden_doc("cutlist");
    let a = add_sheet(&mut d, "A");
    let mut bad = SheetSetup::named("B");
    bad.margins = [4.0 * IN; 4];
    refused(
        &mut d,
        Op::AddSheet {
            setup: Box::new(bad),
        },
    );
    refused(
        &mut d,
        Op::AddSheet {
            setup: Box::new(SheetSetup::named("  ")),
        },
    );
    // An isometric view at 1:8: not to scale.
    let msg = refused(
        &mut d,
        Op::AddSheetItem {
            sheet: a,
            item: Box::new(viewport(StdView::Iso, r(1.0, 1.0, 3.0, 3.0), eighth())),
        },
    );
    assert!(msg.contains("NTS"), "{msg}");
    add_item(
        &mut d,
        a,
        viewport(StdView::Iso, r(1.0, 1.0, 3.0, 3.0), Scale::Nts),
    );
    // A dimension on no viewport, or with one point.
    refused(
        &mut d,
        Op::AddSheetItem {
            sheet: a,
            item: Box::new(ItemKind::Dimension {
                viewport: 9,
                dim: DimKind::Aligned,
                points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
                offset: 0.0,
                style: DimStyle::Arrows,
                text: None,
            }),
        },
    );
    refused(
        &mut d,
        Op::AddSheetItem {
            sheet: a,
            item: Box::new(ItemKind::Dimension {
                viewport: 1,
                dim: DimKind::Angular,
                points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
                offset: 0.0,
                style: DimStyle::Arrows,
                text: None,
            }),
        },
    );
    // A scene that does not exist; an image the model does not hold; an empty label.
    refused(
        &mut d,
        Op::AddSheetItem {
            sheet: a,
            item: Box::new(ItemKind::Viewport {
                rect: r(1.0, 1.0, 2.0, 2.0),
                source: ViewportSource::Scene {
                    name: "Nope".into(),
                },
                scale: Scale::Nts,
                centre: None,
                hidden_lines: false,
                caption: None,
                border: false,
            }),
        },
    );
    refused(
        &mut d,
        Op::AddSheetItem {
            sheet: a,
            item: Box::new(ItemKind::Image {
                rect: r(1.0, 1.0, 2.0, 2.0),
                path: "assets/photo.png".into(),
            }),
        },
    );
    refused(
        &mut d,
        Op::AddSheetItem {
            sheet: a,
            item: Box::new(ItemKind::Label {
                at: [0.01, 0.01],
                text: " ".into(),
                size: 0.003,
                leader: None,
            }),
        },
    );
    // A viewport cannot turn into something else.
    refused(
        &mut d,
        Op::SetSheetItem {
            sheet: a,
            item: 1,
            kind: Box::new(ItemKind::Rect {
                rect: r(1.0, 1.0, 1.0, 1.0),
                width: 0.0003,
                fill: None,
            }),
        },
    );
    refused(&mut d, Op::DeleteSheet { sheet: 99 });
    refused(
        &mut d,
        Op::DeleteSheetItems {
            sheet: a,
            items: vec![42],
        },
    );
    refused(
        &mut d,
        Op::MoveSheetItems {
            sheet: a,
            items: vec![1],
            by: [f64::NAN, 0.0],
        },
    );
}

fn scene(name: &str, parallel: bool) -> SceneDef {
    SceneDef {
        name: name.into(),
        eye: [-3.0, 0.5, 1.0],
        target: [0.5, 0.5, 1.0],
        up: [0.0, 0.0, 1.0],
        fov: 35.0,
        parallel,
        hidden_tags: vec![],
        context: None,
        style: None,
        shadows: None,
        sections: None,
    }
}

#[test]
fn viewports_follow_their_scenes() {
    let mut d = golden_doc("cutlist");
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Side", true),
        },
    );
    let a = add_sheet(&mut d, "A");
    let v = add_item(
        &mut d,
        a,
        ItemKind::Viewport {
            rect: r(1.0, 1.0, 3.0, 6.0),
            source: ViewportSource::Scene {
                name: "Side".into(),
            },
            scale: eighth(),
            centre: None,
            hidden_lines: false,
            caption: Some("SIDE 1:8".into()),
            border: true,
        },
    );
    // Renamed: the viewport follows.
    ok(
        &mut d,
        Op::UpdateScene {
            name: "Side".into(),
            scene: scene("Left side", true),
        },
    );
    let src = |d: &Document| match &d.model().sheet(a).unwrap().item(v).unwrap().kind {
        ItemKind::Viewport { source, scale, .. } => (source.clone(), *scale),
        _ => unreachable!(),
    };
    assert_eq!(
        src(&d).0,
        ViewportSource::Scene {
            name: "Left side".into()
        }
    );
    // Made a perspective scene: the viewport is not to scale any more.
    ok(
        &mut d,
        Op::UpdateScene {
            name: "Left side".into(),
            scene: scene("Left side", false),
        },
    );
    assert_eq!(src(&d).1, Scale::Nts);
    // Deleted: the viewport keeps its camera.
    ok(
        &mut d,
        Op::DeleteScene {
            name: "Left side".into(),
        },
    );
    let (source, scale) = src(&d);
    assert_eq!(
        source,
        ViewportSource::Camera {
            camera: CameraSpec {
                eye: [-3.0, 0.5, 1.0],
                target: [0.5, 0.5, 1.0],
                up: [0.0, 0.0, 1.0],
                parallel: false,
                fov: 35.0
            }
        }
    );
    assert_eq!(scale, Scale::Nts);
}

#[test]
fn sheets_survive_the_snapshot_and_hash_only_when_present() {
    let mut d = golden_doc("cutlist");
    let plain = d.model().hash();
    let a = add_sheet(&mut d, "A");
    assert_ne!(d.model().hash(), plain);
    add_item(
        &mut d,
        a,
        viewport(
            StdView::Front,
            r(0.5, 0.5, 5.0, 7.0),
            Scale::Ratio {
                paper: 1.0,
                model: 12.0,
            },
        ),
    );
    add_item(
        &mut d,
        a,
        ItemKind::Table {
            rect: r(6.0, 0.5, 4.5, 3.0),
            columns: CutColumn::default_set(),
            decimal: false,
            visible_only: false,
            title: Some("Cut list".into()),
        },
    );
    let b = add_sheet(&mut d, "B");
    let mut setup = SheetSetup::named("B, A3");
    setup.paper = PaperSize::A3;
    setup.title = TitleBlock {
        show: true,
        title: "Bookshelf".into(),
        revision: "A".into(),
        ..Default::default()
    };
    ok(
        &mut d,
        Op::SetSheet {
            sheet: b,
            setup: Box::new(setup),
        },
    );
    let snap = d.model().snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: sawhorse_kernel::snapshot::Snapshot = serde_json::from_str(&json).unwrap();
    let m = Model::from_snapshot(&back).unwrap();
    assert_eq!(m.hash(), d.model().hash());
    assert_eq!(m.sheets().len(), 2);
    // A tampered sheet (an item id at or above the counter) is refused, not repaired.
    let mut bad = snap.clone();
    bad.sheets[0].next_item = 1;
    assert!(Model::from_snapshot(&bad).is_err());
    // Deleting the sheets brings the hash back.
    ok(&mut d, Op::DeleteSheet { sheet: a });
    ok(&mut d, Op::DeleteSheet { sheet: b });
    assert_eq!(d.model().hash(), plain);
}

/// The golden `sheet.json`: the section golden's bookshelf (cut by "Section A") with a
/// "Section" scene looking at the cut from the left, "Front" and "Plan" scenes with the cut
/// off, and a Letter landscape sheet: the front elevation and the plan at 1:12, the section at 1:4 cropped to the bottom shelf, the
/// overall width, height and the shelf spacing dimensioned, a label with a leader, the cut
/// list and the title block. (At 1:8 a 72 in bookshelf is 9 in tall: more than Letter's
/// 8 1/2 in, so the elevations are 1:12.)
#[test]
fn the_sheet_golden_is_this_drawing() {
    let mut d = golden_doc("section");
    let now = d.model().scene_sections_now();
    // Looking from -X at the cut (the plane keeps +X), parallel.
    ok(
        &mut d,
        Op::AddScene {
            scene: SceneDef {
                name: "Section".into(),
                eye: [-2.0, 0.142875, 0.9144],
                target: [0.4572, 0.142875, 0.9144],
                up: [0.0, 0.0, 1.0],
                fov: 35.0,
                parallel: true,
                hidden_tags: vec![],
                context: None,
                style: Some(sawhorse_kernel::looks::Style {
                    preset: Some("Shop drawing".into()),
                    face: sawhorse_kernel::looks::FaceStyle::HiddenLine,
                    ..Default::default()
                }),
                shadows: None,
                sections: Some(Box::new(now)),
            },
        },
    );
    // The elevations from scenes that switch the cut off.
    let uncut = sawhorse_kernel::section::SceneSections::default();
    for (name, eye, target, up) in [
        (
            "Front",
            [0.4572, -3.0, 0.9144],
            [0.4572, 0.0, 0.9144],
            [0.0, 0.0, 1.0],
        ),
        (
            "Plan",
            [0.4572, 0.142875, 3.0],
            [0.4572, 0.142875, 0.0],
            [0.0, 1.0, 0.0],
        ),
    ] {
        ok(
            &mut d,
            Op::AddScene {
                scene: SceneDef {
                    name: name.into(),
                    eye,
                    target,
                    up,
                    fov: 35.0,
                    parallel: true,
                    hidden_tags: vec![],
                    context: None,
                    style: None,
                    shadows: None,
                    sections: Some(Box::new(uncut.clone())),
                },
            },
        );
    }
    let mut setup = SheetSetup::named("Bookshelf");
    setup.title = TitleBlock {
        show: true,
        title: "Bookshelf".into(),
        project: "Shop drawings".into(),
        date: "2026-10-12".into(),
        revision: "A".into(),
        ..Default::default()
    };
    let s = match ok(
        &mut d,
        Op::AddSheet {
            setup: Box::new(setup),
        },
    ) {
        OpOutcome::SheetAdded { sheet } => sheet,
        o => panic!("{o:?}"),
    };
    let twelfth = Scale::Ratio {
        paper: 1.0,
        model: 12.0,
    };
    let front = add_item(
        &mut d,
        s,
        ItemKind::Viewport {
            rect: r(0.6, 0.6, 3.6, 6.8),
            source: ViewportSource::Scene {
                name: "Front".into(),
            },
            scale: twelfth,
            centre: None,
            hidden_lines: false,
            caption: Some("FRONT ELEVATION 1:12".into()),
            border: false,
        },
    );
    add_item(
        &mut d,
        s,
        ItemKind::Viewport {
            rect: r(4.5, 0.6, 3.4, 1.4),
            source: ViewportSource::Scene {
                name: "Plan".into(),
            },
            scale: twelfth,
            centre: None,
            hidden_lines: false,
            caption: Some("PLAN 1:12".into()),
            border: false,
        },
    );
    add_item(
        &mut d,
        s,
        ItemKind::Viewport {
            rect: r(8.2, 0.6, 2.2, 3.3),
            source: ViewportSource::Scene {
                name: "Section".into(),
            },
            scale: Scale::Ratio {
                paper: 1.0,
                model: 4.0,
            },
            // The bottom shelf and the toe kick, seen from the side (view right = -Y).
            centre: Some([-5.5 * IN, 4.0 * IN]),
            hidden_lines: false,
            caption: Some("SECTION A 1:4".into()),
            border: true,
        },
    );
    let dim = |kind: DimKind, a: [f64; 3], b: [f64; 3], offset: f64| ItemKind::Dimension {
        viewport: front,
        dim: kind,
        points: vec![a.map(|v| v * IN), b.map(|v| v * IN)],
        offset: offset * IN,
        style: DimStyle::Arrows,
        text: None,
    };
    add_item(
        &mut d,
        s,
        // Below the elevation (a horizontal dimension's offset is up the page).
        dim(DimKind::Horizontal, [0.0, 0.0, 0.0], [36.0, 0.0, 0.0], -0.3),
    );
    add_item(
        &mut d,
        s,
        dim(DimKind::Vertical, [36.0, 0.0, 0.0], [36.0, 0.0, 72.0], 0.35),
    );
    add_item(
        &mut d,
        s,
        dim(
            DimKind::Vertical,
            [0.75, 0.0, 4.25],
            [0.75, 0.0, 21.5],
            -0.3,
        ),
    );
    add_item(
        &mut d,
        s,
        ItemKind::Label {
            at: [4.5 * IN, 7.3 * IN],
            text: "Back panel 1/4 ply in a rabbet".into(),
            size: 0.11 * IN,
            leader: Some(LeaderEnd::Model {
                viewport: front,
                point: [18.0 * IN, 11.0 * IN, 30.0 * IN],
            }),
        },
    );
    add_item(
        &mut d,
        s,
        ItemKind::Table {
            rect: r(4.5, 4.3, 5.9, 2.1),
            columns: CutColumn::default_set(),
            decimal: false,
            visible_only: false,
            title: Some("CUT LIST".into()),
        },
    );
    d.undo().unwrap();
    d.redo().unwrap();
    let m = d.model();
    assert_eq!(m.sheets().len(), 1);
    assert_eq!(m.sheets()[0].1.items.len(), 8);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sheet.json");
    let ops = serde_json::to_value(d.log()).unwrap();
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        let mut g: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "name": "sheet",
                    "hash": "",
                    "stats": {"objects": 0, "faces": 0, "edges": 0, "vertices": 0, "genus": 0, "watertight": true}
                })
            });
        g["description"] = "Lane 12: the cut bookshelf (section.json) on a Letter landscape sheet: front elevation and plan at 1:12, the section at 1:4 cropped to the bottom shelf, three dimensions, a label with a leader on a model point, the cut list and a title block; undo and redo.".into();
        g["ops"] = ops;
        std::fs::write(&path, serde_json::to_string_pretty(&g).unwrap() + "\n").unwrap();
    } else {
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            g["ops"], ops,
            "golden/sheet.json is not this drawing (UPDATE_GOLDEN=1)"
        );
    }
}
