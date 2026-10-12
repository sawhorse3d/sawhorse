//! The cut list (lane 7): parts from components, quantities rolled up through nesting,
//! minimal boxes in lumber order, grain and stock, sheet goods, hidden and handed pieces,
//! determinism and the `.shz` round trip. The bookshelf here is the one the user guide and
//! the Playwright test build.

use glam::DVec3;
use sawhorse_kernel::Document;
use sawhorse_kernel::cutlist::{
    CutList, CutOptions, CutRow, Grain, M_PER_IN, PartKey, PartRef, Rounding,
};
use sawhorse_kernel::model::{DrawTarget, Model, Op, OpOutcome};
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::transform::Xform;

const IN: f64 = M_PER_IN;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A board from (x, y, z), `w` along red, `d` along green, `h` up (inches), in the open
/// context. Returns its object id.
fn board(doc: &mut Document, x: f64, y: f64, z: f64, w: f64, d: f64, h: f64) -> u64 {
    let (x, y, z, w, d, h) = (x * IN, y * IN, z * IN, w * IN, d * IN, h * IN);
    let out = ok(
        doc,
        Op::Draw {
            target: DrawTarget::Plane {
                origin: [0.0, 0.0, z],
                normal: [0.0, 0.0, 1.0],
            },
            points: vec![[x, y, z], [x + w, y, z], [x + w, y + d, z], [x, y + d, z]],
            closed: true,
        },
    );
    let OpOutcome::Drawn {
        sketch: Some(sketch),
        inside: Some(region),
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    match ok(
        doc,
        Op::Extrude {
            sketch,
            region,
            distance: h,
        },
    ) {
        OpOutcome::ObjectCreated { object } => object,
        o => panic!("{o:?}"),
    }
}

/// A board made a component named `name` (painted `material` first). Returns the instance.
fn part(doc: &mut Document, name: &str, material: Option<&str>, b: [f64; 6]) -> (u64, u64) {
    let o = board(doc, b[0], b[1], b[2], b[3], b[4], b[5]);
    if let Some(m) = material {
        ok(
            doc,
            Op::SetMaterial {
                object: o,
                material: Some(m.into()),
            },
        );
    }
    match ok(
        doc,
        Op::MakeComponent {
            items: vec![Item::Object(o)],
            name: name.into(),
        },
    ) {
        OpOutcome::ComponentCreated {
            instance,
            definition,
        } => (instance, definition),
        o => panic!("{o:?}"),
    }
}

fn by(x: f64, y: f64, z: f64) -> Xform {
    Xform::translation(DVec3::new(x * IN, y * IN, z * IN))
}

/// The bookshelf as components: two sides 3/4 x 11 1/4 x 71 1/4 under a 36 in top, four
/// 34 1/2 in shelves 11 in deep (the bottom one on a 3 1/2 in toe kick), and a 1/4 in back
/// panel between the sides behind the shelves. Returns (document, side instance, shelf
/// instances, top definition, back panel definition).
fn bookshelf() -> (Document, u64, Vec<u64>, u64, u64) {
    let mut d = Document::with_ground_sketch();
    ok(
        &mut d,
        Op::DefineMaterial {
            name: "Oak".into(),
            color: [196, 160, 110, 255],
            texture: None,
        },
    );
    let (side, _) = part(
        &mut d,
        "Side",
        Some("Oak"),
        [0.0, 0.0, 0.0, 0.75, 11.25, 71.25],
    );
    ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side],
            xform: by(35.25, 0.0, 0.0),
        },
    );
    let (shelf, _) = part(
        &mut d,
        "Shelf",
        Some("Oak"),
        [0.75, 0.0, 3.5, 34.5, 11.0, 0.75],
    );
    let OpOutcome::Copied { nodes: more, .. } = ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![shelf],
            xform: by(0.0, 0.0, 18.0),
            count: 3,
        },
    ) else {
        panic!()
    };
    let (_, top) = part(
        &mut d,
        "Top",
        Some("Oak"),
        [0.0, 0.0, 71.25, 36.0, 11.25, 0.75],
    );
    part(
        &mut d,
        "Toe kick",
        Some("Oak"),
        [0.75, 2.0, 0.0, 34.5, 0.75, 3.5],
    );
    let (_, back) = part(
        &mut d,
        "Back panel",
        None,
        [0.75, 11.0, 0.0, 34.5, 0.25, 71.25],
    );
    let shelves = std::iter::once(shelf).chain(more).collect();
    (d, side, shelves, top, back)
}

fn list(m: &Model) -> CutList {
    m.cut_list(&CutOptions::default())
}

fn row<'a>(l: &'a CutList, name: &str) -> &'a CutRow {
    l.rows.iter().find(|r| r.name == name).unwrap_or_else(|| {
        panic!(
            "no row {name}: {:?}",
            l.rows.iter().map(|r| &r.name).collect::<Vec<_>>()
        )
    })
}

#[test]
fn the_bookshelf_as_components() {
    let (d, ..) = bookshelf();
    let l = list(d.model());
    type Row<'a> = (&'a str, usize, [i64; 3], Option<&'a str>, bool);
    let got: Vec<Row> = l
        .rows
        .iter()
        .map(|r| {
            (
                r.name.as_str(),
                r.qty,
                r.steps,
                r.material.as_deref(),
                r.sheet,
            )
        })
        .collect();
    // Sizes in sixteenths: 3/4 = 12, 11 1/4 = 180, 71 1/4 = 1140, 11 = 176, 34 1/2 = 552,
    // 36 = 576, 3 1/2 = 56, 1/4 = 4.
    assert_eq!(
        got,
        vec![
            ("Side", 2, [12, 180, 1140], Some("Oak"), false),
            ("Shelf", 4, [12, 176, 552], Some("Oak"), false),
            ("Top", 1, [12, 180, 576], Some("Oak"), false),
            ("Toe kick", 1, [12, 56, 552], Some("Oak"), false),
            ("Back panel", 1, [4, 552, 1140], None, true),
        ]
    );
    for r in &l.rows {
        assert!(
            !r.shaped && !r.scaled && r.handed == 0 && r.hidden == 0,
            "{}",
            r.name
        );
        assert_eq!(r.grain, Grain::Length);
        assert!(matches!(r.key, PartKey::Definition(_)));
        assert_eq!(r.occurrences.len(), r.total);
    }
    // Board feet, exactly: (2 x 3/4 x 11 1/4 x 71 1/4 + 4 x 3/4 x 11 x 34 1/2
    //   + 3/4 x 11 1/4 x 36 + 3/4 x 3 1/2 x 34 1/2) / 144 = 2735.15625 / 144.
    assert_eq!(l.totals.board_feet, 2735.15625 / 144.0);
    assert_eq!(l.totals.board_feet, 18.994140625);
    assert_eq!(row(&l, "Side").board_feet, 2.0 * 601.171875 / 144.0);
    assert_eq!(l.totals.lumber.len(), 1);
    assert_eq!(l.totals.lumber[0].material.as_deref(), Some("Oak"));
    // The back panel: 1/4 in thick, so sheet goods: 34 1/2 x 71 1/4 in on one 4 x 8 sheet.
    assert_eq!(l.totals.sheets.len(), 1);
    let s = &l.totals.sheets[0];
    assert_eq!((s.steps, s.sheets, s.oversize), (4, 1, false));
    assert!((s.area_m2 - 34.5 * 71.25 * IN * IN).abs() < 1e-12);
    assert_eq!(
        (l.totals.parts, l.totals.pieces, l.totals.sheet_count),
        (5, 9, 1)
    );
    // The roll-up through definitions agrees with the pieces found in the world.
    let counts = d.model().part_counts();
    for r in &l.rows {
        if let PartKey::Definition(def) = r.key {
            assert_eq!(counts[&def], r.total, "{}", r.name);
        }
    }
}

#[test]
fn grain_and_stock_are_said_once_and_reach_every_copy() {
    let (mut d, side, _, top, back) = bookshelf();
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Definition(top),
            grain: Grain::Width,
        },
    );
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Definition(back),
            stock: Some("  1/4 birch ply ".into()),
            sheet: None,
        },
    );
    // An instance names its definition.
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Node(side),
            stock: Some("White oak".into()),
            sheet: None,
        },
    );
    let l = list(d.model());
    let t = row(&l, "Top");
    assert_eq!((t.grain, t.grain_set), (Grain::Width, true));
    let b = row(&l, "Back panel");
    assert_eq!(
        (b.material.as_deref(), b.stock_set, b.sheet),
        (Some("1/4 birch ply"), true, true)
    );
    let s = row(&l, "Side");
    assert_eq!((s.material.as_deref(), s.qty), (Some("White oak"), 2));
    assert_eq!(l.totals.lumber.len(), 2, "white oak and oak");
    assert_eq!(
        l.totals.sheets[0].material.as_deref(),
        Some("1/4 birch ply")
    );
    // The sides as sheet stock, explicitly; then back to deciding by name and thickness.
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Node(side),
            stock: Some("3/4 maple ply".into()),
            sheet: None,
        },
    );
    assert!(
        row(&list(d.model()), "Side").sheet,
        "ply reads as sheet stock"
    );
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Node(side),
            stock: Some("3/4 maple ply".into()),
            sheet: Some(false),
        },
    );
    let s = row(&list(d.model()), "Side").clone();
    assert!(!s.sheet && s.sheet_set);
    // Length clears the grain override.
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Definition(top),
            grain: Grain::Length,
        },
    );
    assert!(!row(&list(d.model()), "Top").grain_set);
    // Undo walks it back; a stock longer than a name allows is refused.
    let before = d.hash();
    assert!(
        d.apply(&Op::SetStock {
            part: PartRef::Definition(top),
            stock: Some("x".repeat(65)),
            sheet: None,
        })
        .is_err()
    );
    assert!(
        d.apply(&Op::SetGrain {
            part: PartRef::Definition(9999),
            grain: Grain::Width,
        })
        .is_err()
    );
    assert_eq!(d.hash(), before);
    d.apply(&Op::Undo).unwrap();
    assert!(row(&list(d.model()), "Top").grain_set);
}

#[test]
fn hidden_pieces_are_flagged_and_left_out_with_visible_only() {
    let (mut d, _, shelves, ..) = bookshelf();
    ok(
        &mut d,
        Op::SetHidden {
            items: vec![Item::Node(shelves[3])],
            hidden: true,
        },
    );
    let l = list(d.model());
    let s = row(&l, "Shelf");
    assert_eq!((s.qty, s.total, s.hidden), (4, 4, 1));
    let v = d.model().cut_list(&CutOptions {
        visible_only: true,
        ..Default::default()
    });
    let s = row(&v, "Shelf");
    assert_eq!((s.qty, s.total, s.hidden), (3, 4, 0));
    assert!(v.totals.board_feet < l.totals.board_feet);
    // A hidden tag hides what is on it.
    ok(
        &mut d,
        Op::TagItems {
            items: vec![Item::Node(shelves[0])],
            tag: Some("Shelves".into()),
        },
    );
    ok(
        &mut d,
        Op::SetTagVisible {
            tag: "Shelves".into(),
            visible: false,
        },
    );
    assert_eq!(row(&list(d.model()), "Shelf").hidden, 2);
}

#[test]
fn mirrored_copies_are_handed_and_rotated_ones_measure_the_same() {
    let (mut d, side, ..) = bookshelf();
    // A mirror image of a side (about the plane x = 50 in).
    let mirror = Xform::scaling(DVec3::new(50.0 * IN, 0.0, 0.0), DVec3::new(-1.0, 1.0, 1.0));
    ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side],
            xform: mirror,
        },
    );
    // And one turned 30 degrees about blue.
    let turn = Xform::rotation(
        DVec3::new(80.0 * IN, 0.0, 0.0),
        DVec3::Z,
        30f64.to_radians(),
    );
    ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side],
            xform: turn,
        },
    );
    let l = list(d.model());
    let s = row(&l, "Side");
    assert_eq!((s.qty, s.handed, s.scaled), (4, 1, false));
    assert_eq!(s.steps, [12, 180, 1140]);
    let turned = s.occurrences.last().unwrap();
    // Its length still runs up blue; its thickness is turned 30 degrees off red.
    assert!((turned.bbox.axes[2][2] - 1.0).abs() < 1e-12);
    assert!((turned.bbox.axes[0][0] - 30f64.to_radians().cos()).abs() < 1e-12);
}

#[test]
fn a_scaled_copy_gets_its_own_row() {
    let (mut d, ..) = bookshelf();
    let toe = d
        .model()
        .graph()
        .nodes
        .iter()
        .find(|n| n.label == "Toe kick")
        .unwrap()
        .id;
    let OpOutcome::Copied { nodes, .. } = ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![toe],
            xform: by(0.0, 20.0, 0.0),
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::Transform {
            objects: vec![],
            nodes,
            xform: Xform::scaling(DVec3::new(0.75 * IN, 0.0, 0.0), DVec3::new(0.5, 1.0, 1.0)),
        },
    );
    let l = list(d.model());
    let toes: Vec<&CutRow> = l.rows.iter().filter(|r| r.name == "Toe kick").collect();
    assert_eq!(toes.len(), 2);
    assert!(toes.iter().all(|r| r.scaled && r.qty == 1));
    assert_eq!(toes[1].steps, [12, 56, 276], "half as long");
}

#[test]
fn nested_components_multiply_through_their_parents() {
    // A drawer: two sides (a component) and a bottom (a plain solid); three drawers.
    let mut d = Document::with_ground_sketch();
    let (side, _) = part(&mut d, "Drawer side", None, [0.0, 0.0, 0.0, 0.5, 16.0, 4.0]);
    let OpOutcome::Copied { nodes: other, .. } = ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side],
            xform: by(12.0, 0.0, 0.0),
        },
    ) else {
        panic!()
    };
    let bottom = board(&mut d, 0.5, 0.0, 0.0, 11.5, 16.0, 0.25);
    let OpOutcome::ComponentCreated {
        instance: drawer, ..
    } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Node(side), Item::Node(other[0]), Item::Object(bottom)],
            name: "Drawer".into(),
        },
    )
    else {
        panic!()
    };
    ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![drawer],
            xform: by(0.0, 0.0, 6.0),
            count: 2,
        },
    );
    let l = list(d.model());
    let names: Vec<(&str, usize)> = l.rows.iter().map(|r| (r.name.as_str(), r.qty)).collect();
    assert_eq!(names, vec![("Drawer side", 6), ("Drawer · solid", 3)]);
    assert!(matches!(l.rows[1].key, PartKey::Member { .. }));
    assert_eq!(l.rows[1].steps, [4, 184, 256], "1/4 x 11 1/2 x 16");
    let counts = d.model().part_counts();
    let def = |name: &str| {
        d.model()
            .definitions()
            .into_iter()
            .find(|(_, x)| x.name == name)
            .unwrap()
            .0
    };
    assert_eq!(counts[&def("Drawer")], 3);
    assert_eq!(counts[&def("Drawer side")], 6);
    // Naming the bottom inside one drawer names it in all three; its grain too.
    let bottom_any = l.rows[1].occurrences[2].item;
    let Item::Object(b) = bottom_any else {
        panic!()
    };
    ok(
        &mut d,
        Op::Rename {
            item: Item::Object(b),
            name: Some("Drawer bottom".into()),
        },
    );
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Object(b),
            grain: Grain::None,
        },
    );
    let l = list(d.model());
    let r = row(&l, "Drawer bottom");
    assert_eq!((r.qty, r.grain), (3, Grain::None));
}

#[test]
fn plain_solids_and_solid_groups_are_parts_of_their_own() {
    let mut d = Document::with_ground_sketch();
    let a = board(&mut d, 0.0, 0.0, 0.0, 2.0, 2.0, 30.0);
    let b = board(&mut d, 2.0, 0.0, 0.0, 2.0, 2.0, 30.0);
    let c = board(&mut d, 10.0, 0.0, 0.0, 1.0, 5.0, 20.0);
    // Two legs glued up as a group: one part, 2 x 4 x 30.
    let OpOutcome::GroupCreated { group } = ok(
        &mut d,
        Op::MakeGroup {
            items: vec![Item::Object(a), Item::Object(b)],
            name: Some("Glue-up".into()),
        },
    ) else {
        panic!()
    };
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Node(group),
            grain: Grain::Width,
        },
    );
    let l = list(d.model());
    let got: Vec<(&str, usize, [i64; 3])> = l
        .rows
        .iter()
        .map(|r| (r.name.as_str(), r.qty, r.steps))
        .collect();
    assert_eq!(
        got,
        vec![("Glue-up", 1, [32, 64, 480]), ("Solid 3", 1, [16, 80, 320])]
    );
    assert_eq!(l.rows[0].key, PartKey::Node(group));
    assert_eq!(l.rows[1].key, PartKey::Object(c));
    assert_eq!(l.rows[0].grain, Grain::Width);
    // Make Component on the group: the component keeps what was said about the group.
    let OpOutcome::ComponentCreated { definition, .. } = ok(
        &mut d,
        Op::MakeComponent {
            items: vec![Item::Node(group)],
            name: "Glue-up".into(),
        },
    ) else {
        panic!()
    };
    let l = list(d.model());
    assert_eq!(l.rows[0].key, PartKey::Definition(definition));
    assert_eq!(l.rows[0].grain, Grain::Width);
}

#[test]
fn a_tapered_leg_is_shaped_and_measured_from_its_blank() {
    let mut d = Document::with_ground_sketch();
    let leg = board(&mut d, 0.0, 0.0, 0.0, 2.0, 2.0, 28.0);
    let top: Vec<[f64; 3]> = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]
        .iter()
        .map(|p| [p[0] * IN, p[1] * IN, 28.0 * IN])
        .collect();
    ok(
        &mut d,
        Op::TransformGeometry {
            object: leg,
            points: top,
            xform: Xform::scaling(DVec3::new(IN, IN, 28.0 * IN), DVec3::new(0.6, 0.6, 1.0)),
            autofold: false,
        },
    );
    let l = list(d.model());
    let r = &l.rows[0];
    assert!(r.shaped);
    assert_eq!(r.steps, [32, 32, 448], "the 2 x 2 x 28 blank");
}

#[test]
fn rounding_follows_the_choice_and_is_deterministic() {
    let (d, ..) = bookshelf();
    let a = list(d.model());
    let b = list(d.model());
    assert_eq!(a, b);
    assert_eq!(a.hash, b.hash);
    let m = d.model().cut_list(&CutOptions {
        rounding: Rounding::HalfMm,
        visible_only: false,
    });
    // 3/4 in = 19.05 mm -> 19; 11 1/4 in = 285.75 mm -> 286; 71 1/4 in = 1809.75 -> 1810.
    assert_eq!(row(&m, "Side").steps, [38, 572, 3620]);
    assert_ne!(m.hash, a.hash);
    let t = d.model().cut_list(&CutOptions {
        rounding: Rounding::ThirtySecond,
        visible_only: false,
    });
    assert_eq!(row(&t, "Side").steps, [24, 360, 2280]);
    // The same model replayed from its log lists the same parts, hash for hash.
    let again = Document::replay(d.log()).unwrap();
    assert_eq!(list(again.model()).hash, a.hash);
}

const ROUNDINGS: [Rounding; 3] = [
    Rounding::Sixteenth,
    Rounding::ThirtySecond,
    Rounding::HalfMm,
];

/// What must not depend on the rounding: per row (name, qty, sheet, board feet, area) and
/// the totals (board feet, sheet count, per-stock sheets).
type Classified = (
    Vec<(String, usize, bool, f64, f64)>,
    f64,
    u32,
    Vec<(Option<String>, f64, u32, bool)>,
);

fn classified(l: &CutList) -> Classified {
    (
        l.rows
            .iter()
            .map(|r| (r.name.clone(), r.qty, r.sheet, r.board_feet, r.area_m2))
            .collect(),
        l.totals.board_feet,
        l.totals.sheet_count,
        l.totals
            .sheets
            .iter()
            .map(|s| (s.material.clone(), s.area_m2, s.sheets, s.oversize))
            .collect(),
    )
}

/// Rounding is display only (the lane-7 verifier's first finding): sheet goods, board feet,
/// areas and sheet counts come from the exact sizes, so every rounding (and so every unit
/// system: fractional, decimal and ft-in inches use 1/16 in or 1/32 in, metric 0.5 mm)
/// gives the bookshelf's 18.994140625 board feet and one sheet.
#[test]
fn every_rounding_classifies_and_totals_the_same() {
    let (d, ..) = bookshelf();
    let lists: Vec<CutList> = ROUNDINGS
        .iter()
        .map(|&rounding| {
            d.model().cut_list(&CutOptions {
                rounding,
                visible_only: false,
            })
        })
        .collect();
    for l in &lists {
        assert_eq!(l.totals.board_feet, 18.994140625, "{:?}", l.rounding);
        assert_eq!(l.totals.sheet_count, 1, "{:?}", l.rounding);
        assert!(row(l, "Back panel").sheet, "{:?}", l.rounding);
        assert_eq!(
            l.rows.iter().filter(|r| r.sheet).count(),
            1,
            "{:?}",
            l.rounding
        );
        assert_eq!(classified(l), classified(&lists[0]), "{:?}", l.rounding);
    }
    // The displayed thickness of the sheet follows the rounding; the group does not.
    let steps: Vec<i64> = lists.iter().map(|l| l.totals.sheets[0].steps).collect();
    assert_eq!(steps, [4, 8, 13], "1/4 in = 4/16 = 8/32 = 6.5 mm");
}

/// Parts at the edge of 1/4 in: a panel 0.26 in thick rounds to 1/4 in at 1/16 in and to
/// 6.5 mm at 0.5 mm, but it is thicker than 1/4 in, so it is lumber at every rounding; a
/// 6.35 mm panel drawn in millimetres is 1/4 in, so it is sheet goods at every rounding;
/// and two sheet stocks 6.3 mm and 6.1 mm thick (both 6 mm or 6.5 mm, or 1/4 in, once
/// rounded) stay two groups, so the sheet count never changes.
#[test]
fn the_quarter_inch_line_uses_the_exact_thickness() {
    let mut d = Document::with_ground_sketch();
    ok(
        &mut d,
        Op::DefineMaterial {
            name: "Ply".into(),
            color: [220, 200, 160, 255],
            texture: None,
        },
    );
    part(&mut d, "Thick", None, [0.0, 0.0, 0.0, 30.0, 20.0, 0.26]);
    part(
        &mut d,
        "Quarter",
        None,
        [40.0, 0.0, 0.0, 30.0, 20.0, 6.35 / 25.4],
    );
    part(
        &mut d,
        "Ply A",
        Some("Ply"),
        [80.0, 0.0, 0.0, 47.0, 95.0, 6.3 / 25.4],
    );
    part(
        &mut d,
        "Ply B",
        Some("Ply"),
        [140.0, 0.0, 0.0, 47.0, 95.0, 6.1 / 25.4],
    );
    let lists: Vec<CutList> = ROUNDINGS
        .iter()
        .map(|&rounding| {
            d.model().cut_list(&CutOptions {
                rounding,
                visible_only: false,
            })
        })
        .collect();
    assert_eq!(row(&lists[0], "Thick").steps[0], 4, "0.26 in shows as 1/4");
    for l in &lists {
        assert!(!row(l, "Thick").sheet, "{:?}", l.rounding);
        assert!(row(l, "Quarter").sheet, "{:?}", l.rounding);
        assert_eq!(
            row(l, "Thick").board_feet,
            lists[0].rows[0].board_feet,
            "{:?}",
            l.rounding
        );
        assert_eq!(l.totals.sheets.len(), 3, "{:?}", l.rounding);
        assert_eq!(classified(l), classified(&lists[0]), "{:?}", l.rounding);
    }
    // 0.26 x 20 x 30 in = 156 in^3 = 13/12 board feet (0.26 is not a binary fraction, so
    // within the 24 nm noise grid).
    assert!((row(&lists[0], "Thick").board_feet - 156.0 / 144.0).abs() < 1e-5);
}

#[test]
fn part_attributes_survive_the_snapshot() {
    let (mut d, side, shelves, top, _) = bookshelf();
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Definition(top),
            grain: Grain::Width,
        },
    );
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Node(side),
            stock: Some("Quartersawn oak".into()),
            sheet: Some(false),
        },
    );
    // A group inside a component, and a solid inside one, with attributes of their own.
    ok(
        &mut d,
        Op::SetContext {
            node: Some(shelves[0]),
        },
    );
    let g = d.model().graph();
    let solid = g
        .objects
        .iter()
        .find(|o| o.parent == Some(shelves[0]))
        .unwrap()
        .id;
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Object(solid),
            grain: Grain::None,
        },
    );
    ok(&mut d, Op::LeaveContext);
    let snap = d.model().snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    assert!(json.contains("\"part\":{\"grain\":\"width\"}"), "{json}");
    let back = Model::from_snapshot(&serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(back.hash(), d.model().hash());
    assert_eq!(list(&back).hash, list(d.model()).hash);
}

/// The golden `cutlist.json` replays exactly this: the bookshelf, the top's grain across,
/// the back panel named as ply, a hidden shelf and a mirrored side. Its ops are written
/// from here under `UPDATE_GOLDEN=1` (then `--test golden` fills in the hashes), and must
/// match it otherwise, so the golden never drifts from the documented bookshelf.
#[test]
fn the_cut_list_golden_is_this_bookshelf() {
    let (mut d, side, shelves, top, back) = bookshelf();
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Definition(top),
            grain: Grain::Width,
        },
    );
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Definition(back),
            stock: Some("1/4 birch ply".into()),
            sheet: Some(true),
        },
    );
    ok(
        &mut d,
        Op::SetHidden {
            items: vec![Item::Node(shelves[1])],
            hidden: true,
        },
    );
    ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side],
            xform: Xform::scaling(DVec3::new(60.0 * IN, 0.0, 0.0), DVec3::new(-1.0, 1.0, 1.0)),
        },
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cutlist.json");
    let ops = serde_json::to_value(d.log()).unwrap();
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        let mut g: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "name": "cutlist",
                    "hash": "",
                    "stats": {"objects": 0, "faces": 0, "edges": 0, "vertices": 0, "genus": 0, "watertight": true}
                })
            });
        g["description"] = "Lane 7: the bookshelf as components (Side x2, Shelf x4, Top, Toe kick, Back panel), the top's grain set across, the back panel named 1/4 birch ply and marked sheet, one shelf hidden, a mirrored side copy. Its cut_list hash proves WASM lists the same parts as native.".into();
        g["ops"] = ops;
        std::fs::write(&path, serde_json::to_string_pretty(&g).unwrap() + "\n").unwrap();
    } else {
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            g["ops"], ops,
            "golden/cutlist.json is not this bookshelf (UPDATE_GOLDEN=1)"
        );
    }
    let l = list(d.model());
    assert_eq!(row(&l, "Side").qty, 3);
    assert_eq!(row(&l, "Side").handed, 1);
    assert_eq!(row(&l, "Shelf").hidden, 1);
}
