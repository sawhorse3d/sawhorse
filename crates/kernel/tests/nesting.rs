//! Nesting (lane 9): the bookshelf of the user guide nested on the built-in stock (oak on
//! 4/4 boards, the back on a 1/4 in sheet), then as a plywood carcass; kerf, trim, both
//! algorithms, grain, stock named by a part and by a material default, parts no stock
//! takes, the stock library's ops (undo, refusals, the `.shz` round trip, hashes), and
//! property tests on random cut lists: no overlaps, every part inside its stock, kerf gaps,
//! grain, parts in = placed + unplaced, deterministic hashes.

use glam::DVec3;
use proptest::prelude::*;
use sawhorse_kernel::Document;
use sawhorse_kernel::cutlist::{
    CutList, CutOptions, CutRow, CutTotals, Grain, M_PER_IN, Occurrence, PartBox, PartKey, PartRef,
    Rounding,
};
use sawhorse_kernel::model::{DrawTarget, Op, OpOutcome};
use sawhorse_kernel::nesting::{CutDir, NestOptions, Nesting, UnplacedReason, nest_list};
use sawhorse_kernel::nodes::Item;
use sawhorse_kernel::stock::{
    NestAlgorithm, NestSettings, StockDef, StockGrain, StockKind, StockLibrary,
};
use sawhorse_kernel::testing::{check_nesting, check_nesting_of};
use sawhorse_kernel::transform::Xform;

const IN: f64 = M_PER_IN;

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A board from (x, y, z), `w` along red, `d` along green, `h` up (inches).
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
            curve: None,
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

/// Definitions of the bookshelf's parts.
struct Shelf {
    side: u64,
    shelf: u64,
    top: u64,
    toe: u64,
    back: u64,
}

/// The bookshelf of the user guide (the cut list's, lane 7): oak Side x2 (3/4 x 11 1/4 x
/// 71 1/4), Shelf x4 (3/4 x 11 x 34 1/2), Top (3/4 x 11 1/4 x 36), Toe kick (3/4 x 3 1/2 x
/// 34 1/2), and a 1/4 in Back panel (34 1/2 x 71 1/4) with no material.
fn bookshelf() -> (Document, Shelf) {
    let mut d = Document::with_ground_sketch();
    ok(
        &mut d,
        Op::DefineMaterial {
            name: "Oak".into(),
            color: [196, 160, 110, 255],
            texture: None,
            size: None,
            rotation: 0.0,
            offset: [0.0; 2],
            stock: None,
        },
    );
    let (side_i, side) = part(
        &mut d,
        "Side",
        Some("Oak"),
        [0.0, 0.0, 0.0, 0.75, 11.25, 71.25],
    );
    ok(
        &mut d,
        Op::Copy {
            objects: vec![],
            nodes: vec![side_i],
            xform: by(35.25, 0.0, 0.0),
        },
    );
    let (shelf_i, shelf) = part(
        &mut d,
        "Shelf",
        Some("Oak"),
        [0.75, 0.0, 3.5, 34.5, 11.0, 0.75],
    );
    ok(
        &mut d,
        Op::Array {
            objects: vec![],
            nodes: vec![shelf_i],
            xform: by(0.0, 0.0, 18.0),
            count: 3,
        },
    );
    let (_, top) = part(
        &mut d,
        "Top",
        Some("Oak"),
        [0.0, 0.0, 71.25, 36.0, 11.25, 0.75],
    );
    let (_, toe) = part(
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
    (
        d,
        Shelf {
            side,
            shelf,
            top,
            toe,
            back,
        },
    )
}

fn nest(d: &Document) -> Nesting {
    let n = d.model().nest(&NestOptions::default());
    let list = d.model().cut_list(&CutOptions::default());
    check_nesting_of(&list, &n).unwrap_or_else(|e| panic!("{e}"));
    n
}

fn sq_in(m2: f64) -> f64 {
    m2 / (IN * IN)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9 * b.abs().max(1.0)
}

#[test]
fn the_bookshelf_on_the_builtin_stock() {
    let (d, _) = bookshelf();
    let n = nest(&d);
    assert_eq!(n.algorithm, NestAlgorithm::Guillotine);
    assert_eq!(n.settings.kerf, IN / 8.0);
    // Oak, 3/4 in: the 4/4 class (13/16 in); the widest part is 11 1/4 in, so 4/4 x 12 x 96
    // boards for all eight oak pieces. Sides take a board each (71 1/4 + 1/8 + 34 1/2 > 96);
    // the top and the 34 1/2 in parts go two to a board.
    let t = &n.totals;
    assert_eq!((t.sheets, t.boards), (1, 5));
    assert_eq!((t.parts_in, t.placed, t.unplaced), (9, 9, 0));
    assert_eq!(t.groups.len(), 2);
    let names = |i: usize| {
        n.pieces[i]
            .placements
            .iter()
            .map(|q| q.name.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(0), ["Side"]);
    // (Equal lengths go in cut-list order: here the shelves come before the toe kick.)
    assert_eq!(names(2), ["Top", "Shelf"]);
    assert_eq!(names(3), ["Shelf", "Shelf"]);
    assert_eq!(names(4), ["Shelf", "Toe kick"]);
    let oak = &t.groups[0];
    assert_eq!(oak.stock, "4/4 x 12 x 96");
    assert_eq!(oak.material.as_deref(), Some("Oak"));
    assert_eq!((oak.kind, oak.count, oak.parts), (StockKind::Board, 5, 8));
    assert!(near(oak.board_feet, 40.0), "{}", oak.board_feet);
    let back = &t.groups[1];
    assert_eq!(back.stock, "4 x 8 sheet, 1/4");
    assert_eq!(
        (back.kind, back.count, back.parts),
        (StockKind::Sheet, 1, 1)
    );
    assert_eq!(t.by_nominal.len(), 1);
    assert_eq!(
        (t.by_nominal[0].nominal.as_str(), t.by_nominal[0].count),
        ("4/4", 5)
    );
    // Areas from the exact sizes: oak 2 x 11 1/4 x 71 1/4 + 4 x 11 x 34 1/2 + 11 1/4 x 36 +
    // 3 1/2 x 34 1/2 = 3,646.875 in²; the back 34 1/2 x 71 1/4 = 2,458.125 in²; stock 5
    // boards of 12 x 96 and a 48 x 96 sheet: 10,368 in².
    assert!(near(sq_in(t.parts_area), 6105.0), "{}", sq_in(t.parts_area));
    assert!(near(sq_in(t.stock_area), 10368.0));
    assert!(near(t.efficiency, 6105.0 / 10368.0));
    // The back panel lies along the sheet's grain, at the corner, and its two cuts free it.
    let sheet = n
        .pieces
        .iter()
        .find(|p| p.kind == StockKind::Sheet)
        .unwrap();
    let q = &sheet.placements[0];
    assert_eq!(
        (q.name.as_str(), q.tag.as_str(), q.turned),
        ("Back panel", "5", false)
    );
    assert!(near(q.dx / IN, 71.25) && near(q.dy / IN, 34.5));
    assert_eq!((q.x, q.y), (0.0, 0.0));
    assert!(near(sq_in(sheet.waste_area), 4608.0 - 2458.125));
    assert_eq!(sheet.cuts.len(), 2);
    assert!(sheet.guillotine);
    // A side's board: crosscut to length, then ripped to width.
    let first = &n.pieces[0];
    assert_eq!(first.placements.len(), 1);
    assert_eq!(first.placements[0].name, "Side");
    assert_eq!(first.placements[0].tag, "1a");
    let dirs: Vec<CutDir> = first.cuts.iter().map(|c| c.dir).collect();
    assert_eq!(dirs, [CutDir::Crosscut, CutDir::Rip]);
    assert!(near(first.cuts[0].at / IN, 71.25));
    assert!(near(first.cuts[1].at / IN, 11.25));
    // Every piece is somewhere, exactly once, and its item selects it in the model.
    let tags: Vec<&str> = n
        .pieces
        .iter()
        .flat_map(|p| p.placements.iter().map(|q| q.tag.as_str()))
        .collect();
    let mut sorted = tags.clone();
    sorted.sort();
    assert_eq!(sorted, ["1a", "1b", "2a", "2b", "2c", "2d", "3", "4", "5"]);
    // Deterministic, and the same through a replay of the log.
    assert_eq!(nest(&d).hash, n.hash);
    let again = Document::replay(d.log()).unwrap();
    assert_eq!(nest(&again).hash, n.hash);
}

/// The carcass as 3/4 plywood: every part but the back named "3/4 birch ply" (sheet goods by
/// its name), the back as 1/4 ply.
fn plywood(d: &mut Document, s: &Shelf) {
    for def in [s.side, s.shelf, s.top, s.toe] {
        ok(
            d,
            Op::SetStock {
                part: PartRef::Definition(def),
                stock: Some("3/4 birch ply".into()),
                sheet: None,
            },
        );
    }
    ok(
        d,
        Op::SetStock {
            part: PartRef::Definition(s.back),
            stock: Some("1/4 birch ply".into()),
            sheet: None,
        },
    );
}

#[test]
fn a_plywood_carcass_takes_two_sheets_and_the_back_one() {
    let (mut d, s) = bookshelf();
    plywood(&mut d, &s);
    let n = nest(&d);
    let t = &n.totals;
    // 3/4 ply: two 11 1/4 x 71 1/4 sides and six parts about 11 x 35 on 48 x 96 sheets with
    // the grain along: four 11 1/4 strips fit across a sheet, but the sides' strips have
    // 24 5/8 in left over that nothing 34 1/2 long fits, so two sheets; the back a third.
    assert_eq!((t.sheets, t.boards, t.unplaced), (3, 0, 0));
    assert_eq!(t.groups[0].stock, "4 x 8 sheet, 3/4");
    assert_eq!(t.groups[0].material.as_deref(), Some("3/4 birch ply"));
    assert_eq!(t.groups[0].count, 2);
    assert_eq!(t.groups[1].stock, "4 x 8 sheet, 1/4");
    for p in &n.pieces {
        assert!(p.guillotine);
        for q in &p.placements {
            assert!(!q.turned, "{} turned against the grain", q.tag);
        }
    }
    // The top's grain across: it lies across the sheet now (turned), still two sheets.
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Definition(s.top),
            grain: Grain::Width,
        },
    );
    let n2 = nest(&d);
    let top = n2
        .pieces
        .iter()
        .flat_map(|p| &p.placements)
        .find(|q| q.name == "Top")
        .unwrap();
    assert!(top.turned);
    assert!(near(top.dx / IN, 11.25) && near(top.dy / IN, 36.0));
    assert_ne!(n2.hash, n.hash);
}

#[test]
fn kerf_trim_and_maxrects_change_the_layout_not_the_rules() {
    let (mut d, s) = bookshelf();
    plywood(&mut d, &s);
    let base = nest(&d);
    ok(
        &mut d,
        Op::SetNestSettings {
            settings: NestSettings {
                kerf: 3.0 / 32.0 * IN,
                trim: 0.0,
                algorithm: NestAlgorithm::Guillotine,
                ..NestSettings::default()
            },
        },
    );
    let thin = nest(&d);
    assert_eq!(thin.settings.kerf, 3.0 / 32.0 * IN);
    assert_ne!(thin.hash, base.hash);
    // Parts sit a kerf apart: the second strip starts 11 1/4 + 3/32 in up.
    let ys: Vec<f64> = thin.pieces[0]
        .placements
        .iter()
        .map(|q| q.y / IN)
        .filter(|y| *y > 0.0)
        .collect();
    assert!(ys.iter().any(|y| near(*y, 11.25 + 3.0 / 32.0)), "{ys:?}");
    // Trim 1/2 in: four trim cuts first, every part inside the trimmed sheet.
    ok(
        &mut d,
        Op::SetNestSettings {
            settings: NestSettings {
                kerf: 3.0 / 32.0 * IN,
                trim: 0.5 * IN,
                algorithm: NestAlgorithm::Guillotine,
                ..NestSettings::default()
            },
        },
    );
    let trimmed = nest(&d);
    let p = &trimmed.pieces[0];
    for (got, want) in p.usable.iter().zip([0.5, 0.5, 95.5, 47.5]) {
        assert!(near(*got / IN, want), "{:?}", p.usable);
    }
    assert!(p.cuts[..4].iter().all(|c| c.trim));
    assert!(!p.cuts[4].trim);
    assert_eq!(
        p.cuts[..4].iter().map(|c| c.dir).collect::<Vec<_>>(),
        [CutDir::Rip, CutDir::Rip, CutDir::Crosscut, CutDir::Crosscut]
    );
    // Maxrects: the same parts, valid, and (here) a guillotine layout too.
    ok(
        &mut d,
        Op::SetNestSettings {
            settings: NestSettings {
                kerf: 3.0 / 32.0 * IN,
                trim: 0.5 * IN,
                algorithm: NestAlgorithm::Maxrects,
                ..NestSettings::default()
            },
        },
    );
    let mr = nest(&d);
    assert_eq!(mr.algorithm, NestAlgorithm::Maxrects);
    assert_eq!(mr.totals.placed, 9);
    assert_ne!(mr.hash, trimmed.hash);
    // The settings are an undo step.
    ok(&mut d, Op::Undo);
    assert_eq!(nest(&d).hash, trimmed.hash);
}

#[test]
fn stock_named_by_a_part_or_a_material_and_parts_no_stock_takes() {
    let (mut d, s) = bookshelf();
    // The shelves cut from MDF, named exactly as the library names it: no grain, so they
    // may turn.
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Definition(s.shelf),
            stock: Some("4 x 8 mdf, 3/4".into()),
            sheet: None,
        },
    );
    let n = nest(&d);
    let mdf = n
        .totals
        .groups
        .iter()
        .find(|g| g.stock == "4 x 8 MDF, 3/4")
        .unwrap();
    assert_eq!((mdf.kind, mdf.parts), (StockKind::Sheet, 4));
    // Oak defaults to 8 in boards: the 11 1/4 in sides and top are wider, so (#49) each is
    // glued up from two strips of that board, 8 in and a 3 3/8 in filler (11 1/4 + 1/8
    // joint loss - 8); the toe kick fits whole. Before #49 these three were reported too
    // large.
    ok(
        &mut d,
        Op::SetMaterialStock {
            material: "oak".into(),
            stock: Some("4/4 x 8 x 96".into()),
        },
    );
    let n = nest(&d);
    // (Rows: lumber first, so Side 1, Top 2, Toe kick 3, then the MDF shelves and the back.)
    let glued: Vec<&str> = n.glue_ups.iter().map(|g| g.tag.as_str()).collect();
    assert_eq!(glued, ["1a", "1b", "2"]);
    for g in &n.glue_ups {
        assert_eq!(g.stock, "4/4 x 8 x 96");
        assert_eq!(g.widths.len(), 2);
        assert!(
            near(g.widths[0], 8.0 * IN) && near(g.widths[1], 3.375 * IN),
            "{g:?}"
        );
    }
    assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
    assert_eq!(n.totals.parts_in, 9);
    assert_eq!(n.totals.placed, 9);
    // A board too short for the sides, the top and the toe kick: they are too large for the
    // stock their material names, and reported so with that stock (gluing only adds width).
    ok(
        &mut d,
        Op::DefineStock {
            stock: StockDef {
                name: "4/4 x 8 x 24".into(),
                kind: StockKind::Board,
                length: 24.0 * IN,
                width: 8.0 * IN,
                thickness: 0.75 * IN,
                nominal: Some("4/4".into()),
                grain: StockGrain::Length,
            },
            replace: None,
        },
    );
    ok(
        &mut d,
        Op::SetMaterialStock {
            material: "oak".into(),
            stock: Some("4/4 x 8 x 24".into()),
        },
    );
    let n = nest(&d);
    let large: Vec<&str> = n
        .unplaced
        .iter()
        .filter(|u| u.reason == UnplacedReason::TooLarge)
        .map(|u| u.tag.as_str())
        .collect();
    assert_eq!(large, ["1a", "1b", "2", "3"]);
    assert_eq!(n.unplaced[0].stock.as_deref(), Some("4/4 x 8 x 24"));
    assert_eq!(n.totals.parts_in, 9);
    assert_eq!(n.totals.placed + n.totals.unplaced, 9);
    // Back to automatic.
    ok(
        &mut d,
        Op::SetMaterialStock {
            material: "Oak".into(),
            stock: None,
        },
    );
    assert!(nest(&d).unplaced.is_empty());
    // The top's grain across on a 12 in board: 36 in across does not fit, along would.
    ok(
        &mut d,
        Op::SetGrain {
            part: PartRef::Definition(s.top),
            grain: Grain::Width,
        },
    );
    let n = nest(&d);
    assert_eq!(n.unplaced.len(), 1);
    assert_eq!(n.unplaced[0].reason, UnplacedReason::Grain);
    assert_eq!(n.unplaced[0].name, "Top");
    // A part named for a stock of the wrong thickness.
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Definition(s.toe),
            stock: Some("4 x 8 sheet, 1/2".into()),
            sheet: None,
        },
    );
    let n = nest(&d);
    let toe = n.unplaced.iter().find(|u| u.name == "Toe kick").unwrap();
    assert_eq!(toe.reason, UnplacedReason::Thickness);
}

#[test]
fn too_thick_and_too_long_are_reported_never_dropped() {
    let mut d = Document::with_ground_sketch();
    // A 4 in thick beam (no board is that thick), a 10 ft rail (longer than any board) and
    // a 1 in thick panel marked sheet goods (no sheet within 1/16 in).
    board(&mut d, 0.0, 0.0, 0.0, 4.0, 4.0, 30.0);
    board(&mut d, 10.0, 0.0, 0.0, 0.75, 3.0, 120.0);
    let panel = board(&mut d, 20.0, 0.0, 0.0, 20.0, 30.0, 1.0);
    ok(
        &mut d,
        Op::SetStock {
            part: PartRef::Object(panel),
            stock: None,
            sheet: Some(true),
        },
    );
    let n = nest(&d);
    let reasons: Vec<(String, UnplacedReason)> = n
        .unplaced
        .iter()
        .map(|u| (u.name.clone(), u.reason))
        .collect();
    assert_eq!(
        reasons,
        [
            ("Solid 1".to_string(), UnplacedReason::NoStock),
            ("Solid 2".to_string(), UnplacedReason::TooLarge),
            ("Solid 3".to_string(), UnplacedReason::NoStock),
        ]
    );
    assert_eq!(n.totals.placed, 0);
    assert_eq!(n.totals.parts_in, 3);
    assert!(n.pieces.is_empty());
    assert_eq!(n.totals.efficiency, 0.0);
}

#[test]
fn the_stock_library_is_undoable_saved_and_hashed_only_when_changed() {
    let (mut d, _) = bookshelf();
    let plain = d.hash();
    assert!(d.model().stored_stock_library().is_none());
    let mut wide = StockLibrary::builtin().stocks[9].clone();
    wide.name = "4/4 x 12 x 120".into();
    wide.length = 120.0 * IN;
    ok(
        &mut d,
        Op::DefineStock {
            stock: wide.clone(),
            replace: None,
        },
    );
    assert_ne!(d.hash(), plain);
    // Longer boards of the same width: the oak now comes from them (the longest of the
    // narrowest), four boards instead of five.
    let n = nest(&d);
    assert_eq!(n.totals.groups[0].stock, "4/4 x 12 x 120");
    assert_eq!(n.totals.boards, 4);
    // Refusals are typed and change nothing.
    let h = d.hash();
    for bad in [
        Op::DefineStock {
            stock: wide.clone(),
            replace: None,
        },
        Op::DefineStock {
            stock: StockDef {
                thickness: f64::NAN,
                ..wide.clone()
            },
            replace: Some(wide.name.clone()),
        },
        Op::DeleteStock {
            name: "nothing".into(),
        },
        Op::SetMaterialStock {
            material: "Oak".into(),
            stock: Some("nothing".into()),
        },
        Op::SetMaterialStock {
            material: " ".into(),
            stock: None,
        },
        Op::SetNestSettings {
            settings: NestSettings {
                kerf: -1.0,
                ..NestSettings::default()
            },
        },
        Op::SetNestSettings {
            settings: NestSettings {
                trim: f64::INFINITY,
                ..NestSettings::default()
            },
        },
    ] {
        let e = d.apply(&bad).unwrap_err();
        assert!(!e.is_bug(), "{e}");
        assert_eq!(d.hash(), h, "{bad:?}");
    }
    // Saved and opened: the same library, the same hash, the same nesting.
    let snap = d.model().snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    assert!(json.contains("\"stock\""));
    let back =
        sawhorse_kernel::Model::from_snapshot(&serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(back.hash_hex(), d.model().hash_hex());
    assert_eq!(back.nest(&NestOptions::default()).hash, n.hash);
    // A library edited back to the built-in one is stored as nothing: the old hash again.
    ok(
        &mut d,
        Op::DeleteStock {
            name: "4/4 X 12 X 120".into(),
        },
    );
    assert_eq!(d.hash(), plain);
    assert!(d.model().stored_stock_library().is_none());
    assert!(
        !serde_json::to_string(&d.model().snapshot())
            .unwrap()
            .contains("\"stock\"")
    );
    // Reset after an edit; undo brings the edit back.
    ok(
        &mut d,
        Op::SetMaterialStock {
            material: "Oak".into(),
            stock: Some("4/4 x 8 x 96".into()),
        },
    );
    let edited = d.hash();
    ok(&mut d, Op::ResetStockLibrary);
    assert_eq!(d.hash(), plain);
    ok(&mut d, Op::Undo);
    assert_eq!(d.hash(), edited);
    // A saved library that breaks a rule is refused on open.
    let mut s = d.model().snapshot();
    let mut lib = StockLibrary::builtin();
    lib.defaults.insert("Oak".into(), "nothing".into());
    s.stock = Some(lib);
    assert!(sawhorse_kernel::Model::from_snapshot(&s).is_err());
    check_nesting(d.model()).unwrap();
}

#[test]
fn visible_only_nests_the_drawn_pieces() {
    let (mut d, _) = bookshelf();
    let shelf_instance = d
        .model()
        .cut_list(&CutOptions::default())
        .rows
        .iter()
        .find(|r| r.name == "Shelf")
        .unwrap()
        .occurrences[1]
        .item;
    ok(
        &mut d,
        Op::SetHidden {
            items: vec![shelf_instance],
            hidden: true,
        },
    );
    let all = nest(&d);
    let drawn = d.model().nest(&NestOptions {
        visible_only: true,
        algorithm: None,
    });
    let list = d.model().cut_list(&CutOptions {
        visible_only: true,
        ..CutOptions::default()
    });
    check_nesting_of(&list, &drawn).unwrap();
    assert_eq!(all.totals.parts_in, 9);
    assert_eq!(drawn.totals.parts_in, 8);
    assert!(drawn.visible_only);
    assert!(
        !drawn
            .pieces
            .iter()
            .flat_map(|p| &p.placements)
            .any(|q| q.item == shelf_instance)
    );
}

// ---- properties on random cut lists ------------------------------------------------------

/// A cut list of plain parts: (thickness, width, length in inches, quantity, grain, sheet,
/// material index).
fn list_of(parts: &[(f64, f64, f64, usize, Grain, bool, u8)], visible_mask: u64) -> CutList {
    let mut rows = Vec::new();
    let mut next = 1u64;
    for (k, &(t, w, l, qty, grain, sheet, mat)) in parts.iter().enumerate() {
        let mut size = [t * IN, w * IN, l * IN];
        size.sort_by(f64::total_cmp);
        let occurrences: Vec<Occurrence> = (0..qty)
            .map(|_| {
                let id = next;
                next += 1;
                Occurrence {
                    item: Item::Object(id),
                    visible: visible_mask & (1 << (id % 64)) == 0,
                    handed: false,
                    bbox: PartBox {
                        axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                        size,
                        center: [0.0; 3],
                        shaped: false,
                    },
                }
            })
            .collect();
        rows.push(CutRow {
            key: PartKey::Object(k as u64 + 1),
            target: PartRef::Object(k as u64 + 1),
            name: format!("Part {}", k + 1),
            qty,
            total: qty,
            hidden: 0,
            handed: 0,
            size,
            rounded: size,
            steps: [0; 3],
            shaped: false,
            scaled: false,
            // Lane 15: flagged parts nest like any other; a dropped part is never nested.
            not_solid: k % 3 == 1,
            included: k % 7 != 3,
            // Lane 15 fixes: now and then a row counted nowhere (reported, never placed).
            uncounted: match k % 11 {
                5 => Some(sawhorse_kernel::cutlist::Uncounted::Flat),
                8 => Some(sawhorse_kernel::cutlist::Uncounted::NotABoard),
                _ => None,
            },
            nominal_thickness: size[0],
            nominal: None,
            grain,
            grain_set: grain != Grain::Length,
            material: Some(["Oak", "Maple", "Birch ply", "MDF"][mat as usize % 4].into()),
            stock_set: false,
            sheet,
            sheet_set: true,
            board_feet: 0.0,
            area_m2: 0.0,
            occurrences,
        });
    }
    CutList {
        rounding: Rounding::Sixteenth,
        visible_only: false,
        rows,
        totals: CutTotals {
            parts: 0,
            pieces: 0,
            excluded: 0,
            not_solid: 0,
            loose_faces: 0,
            loose_here: 0,
            loose_places: vec![],
            uncounted: 0,
            hidden: 0,
            board_feet: 0.0,
            lumber: vec![],
            sheets: vec![],
            sheet_count: 0,
        },
        sheet_size: [48.0 * IN, 96.0 * IN],
        hash: String::new(),
    }
}

fn a_part() -> impl Strategy<Value = (f64, f64, f64, usize, Grain, bool, u8)> {
    let thickness = prop_oneof![
        Just(0.25),
        Just(0.5),
        Just(0.75),
        Just(0.72),
        Just(1.0),
        Just(1.5),
        Just(3.0),
        0.1..3.0f64
    ];
    let side = prop_oneof![
        4 => 1.0..40.0f64,
        2 => (1i32..97).prop_map(f64::from),
        1 => 40.0..130.0f64,
        1 => Just(48.0),
        1 => Just(96.0),
    ];
    let grain = prop_oneof![Just(Grain::Length), Just(Grain::Width), Just(Grain::None)];
    (
        thickness,
        side.clone(),
        side,
        1usize..6,
        grain,
        any::<bool>(),
        any::<u8>(),
    )
}

fn settings() -> impl Strategy<Value = NestSettings> {
    let kerf = prop_oneof![
        Just(0.0),
        Just(IN / 8.0),
        Just(3.0 / 32.0 * IN),
        0.0..(0.5 * IN)
    ];
    let trim = prop_oneof![2 => Just(0.0), 1 => Just(0.5 * IN), 1 => 0.0..(4.0 * IN)];
    let alg = prop_oneof![
        Just(NestAlgorithm::Guillotine),
        Just(NestAlgorithm::Maxrects)
    ];
    let joint = prop_oneof![
        2 => Just(IN / 8.0),
        1 => Just(0.0),
        1 => 0.0..(0.5 * IN)
    ];
    (kerf, trim, alg, joint).prop_map(|(kerf, trim, algorithm, joint_loss)| NestSettings {
        kerf,
        trim,
        algorithm,
        joint_loss,
    })
}

fn library() -> impl Strategy<Value = StockLibrary> {
    (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(grainless, small, long)| {
        let mut l = StockLibrary::builtin();
        // #56: longer, narrower boards beside the built-in 96 in ones, so a part too long for
        // the board the rest shares can go to one long enough (the verifier's case).
        if long {
            let boards: Vec<StockDef> = l
                .stocks
                .iter()
                .filter(|s| s.kind == StockKind::Board)
                .cloned()
                .collect();
            for mut s in boards {
                s.name = format!("{} long", s.name);
                s.length = 120.0 * IN;
                s.width *= 0.5;
                l.stocks.push(s);
            }
        }
        if grainless {
            for s in &mut l.stocks {
                s.grain = StockGrain::None;
            }
        }
        if small {
            l.stocks.push(StockDef {
                name: "Offcut 24 x 30, 3/4".into(),
                kind: StockKind::Sheet,
                length: 30.0 * IN,
                width: 24.0 * IN,
                thickness: 0.75 * IN,
                nominal: None,
                grain: StockGrain::Length,
            });
            l.defaults
                .insert("Birch ply".into(), "Offcut 24 x 30, 3/4".into());
        }
        l
    })
}

/// Oak parts on the built-in boards, no material defaults, kerf 1/8 in, no trim, the joint
/// loss given (inches).
fn oak(parts: &[(f64, f64, f64, usize)], joint_loss: f64) -> Nesting {
    let rows: Vec<_> = parts
        .iter()
        .map(|&(t, w, l, q)| (t, w, l, q, Grain::Length, false, 0u8))
        .collect();
    let list = list_of(&rows, 0);
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    let st = NestSettings {
        kerf: IN / 8.0,
        trim: 0.0,
        algorithm: NestAlgorithm::Guillotine,
        joint_loss: joint_loss * IN,
    };
    let n = nest_list(&list, &lib, st);
    check_nesting_of(&list, &n).unwrap();
    n
}

/// The strips placed for a part's row, by strip number: (stock, width in inches).
fn strips_of(n: &Nesting, row: usize) -> Vec<(String, f64)> {
    let mut v: Vec<(usize, String, f64)> = n
        .pieces
        .iter()
        .flat_map(|p| {
            p.placements
                .iter()
                .filter(|q| q.row == row)
                .map(|q| (q.strip.expect("a strip").n, p.stock.clone(), q.dy / IN))
        })
        .collect();
    v.sort_by_key(|s| s.0);
    v.into_iter().map(|(_, s, w)| (s, w)).collect()
}

fn near_in(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// #49: a solid part wider than every board is glued up from full-width strips and one
/// filler instead of being reported too large. A 15 x 40 in panel is wider than the widest
/// built-in 4/4 board (12 in). #56: 4/4 x 8 and 4/4 x 12 both glue it from two strips, and
/// with nothing else of that oak library order decides, 4/4 x 8: one 8 in strip and a
/// 7 1/8 in filler (15 + 1/8 joint loss - 8), on one board, tagged 1.1 and 1.2 (#53 took
/// the widest board, 12 + 3 1/8 in).
#[test]
fn a_part_wider_than_every_board_is_glued_up() {
    let n = oak(&[(0.75, 15.0, 40.0, 1)], 0.125);
    assert_eq!(n.totals.unplaced, 0, "{:#?}", n.unplaced);
    assert_eq!(n.totals.placed, 1);
    assert_eq!(n.totals.boards, 1);
    let s = strips_of(&n, 0);
    assert_eq!(s.len(), 2);
    assert!(s.iter().all(|(stock, _)| stock == "4/4 x 8 x 96"));
    assert!(near_in(s[0].1, 8.0) && near_in(s[1].1, 7.125), "{s:?}");
    let tags: Vec<&str> = n.pieces[0]
        .placements
        .iter()
        .map(|q| q.tag.as_str())
        .collect();
    assert!(tags.contains(&"1.1") && tags.contains(&"1.2"), "{tags:?}");
    assert!(
        n.pieces[0]
            .placements
            .iter()
            .all(|q| q.strip.unwrap().of == 2 && near_in(q.dx / IN, 40.0) && !q.turned)
    );
    assert_eq!(n.glue_ups.len(), 1);
    assert_eq!(n.glue_ups[0].tag, "1");
    assert_eq!(n.glue_ups[0].stock, "4/4 x 8 x 96");
}

/// #49's example: a 17 in cabinet side beside two 5 in shelves, which put the rest of the
/// oak on 4/4 x 6. #56: it is glued from the board that takes the fewest strips, 4/4 x 12:
/// a 12 in strip and a 5 1/8 in filler (17 + 1/8 - 12), one joint, where the shelves' 6 in
/// board took three strips, two joints (#53 glued it 6 + 6 + 5 1/4 in). A part a listed
/// board makes in one piece is never glued: a 10 in part fits 4/4 x 12, so it is one piece.
#[test]
fn a_side_glued_from_the_board_that_takes_the_fewest_strips() {
    let n = oak(&[(0.75, 5.0, 30.0, 2), (0.75, 17.0, 30.0, 1)], 0.125);
    assert_eq!(n.totals.unplaced, 0, "{:#?}", n.unplaced);
    let s = strips_of(&n, 1);
    assert_eq!(s.len(), 2, "{s:?}");
    assert!(s.iter().all(|(stock, _)| stock == "4/4 x 12 x 96"), "{s:?}");
    assert!(near_in(s[0].1, 12.0) && near_in(s[1].1, 5.125), "{s:?}");
    // The shelves are whole pieces.
    assert!(
        n.pieces
            .iter()
            .flat_map(|p| &p.placements)
            .filter(|q| q.row == 0)
            .all(|q| q.strip.is_none() && near_in(q.dy / IN, 5.0))
    );
    // A part a listed width makes in one piece is one piece: 10 in fits 4/4 x 12.
    let one = oak(&[(0.75, 5.0, 30.0, 2), (0.75, 10.0, 30.0, 1)], 0.125);
    assert!(one.glue_ups.is_empty());
    assert!(
        one.pieces
            .iter()
            .flat_map(|p| &p.placements)
            .all(|q| q.strip.is_none())
    );
}

/// A part some listed board makes in one piece is never glued, even when that board is
/// thicker than its class and the rest of the oak shares a narrower one (from the #49
/// verification): a 14 in part beside two 3 in parts goes whole on a 5/4 x 16 board, not
/// glued from the 4 in board the 3 in parts use.
#[test]
fn a_wider_thicker_board_that_makes_it_whole_wins_over_gluing() {
    let list = list_of(
        &[
            (0.75, 14.0, 30.0, 1, Grain::Length, false, 0),
            (0.75, 3.0, 30.0, 2, Grain::Length, false, 0),
        ],
        0,
    );
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    lib.stocks.push(StockDef {
        name: "5/4 x 16 x 96".into(),
        kind: StockKind::Board,
        length: 96.0 * IN,
        width: 16.0 * IN,
        thickness: 1.0625 * IN,
        nominal: None,
        grain: StockGrain::Length,
    });
    let n = nest_list(&list, &lib, NestSettings::default());
    check_nesting_of(&list, &n).unwrap();
    assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
    let whole = n
        .pieces
        .iter()
        .find(|p| p.placements.iter().any(|q| q.row == 0))
        .unwrap();
    assert_eq!(whole.stock, "5/4 x 16 x 96");
}

/// No joint loss: a 24 in panel is exactly two 12 in boards edge to edge.
#[test]
fn no_joint_loss_glues_whole_boards() {
    let n = oak(&[(0.75, 24.0, 30.0, 1)], 0.0);
    let s = strips_of(&n, 0);
    assert_eq!(s.len(), 2, "{s:?}");
    assert!(near_in(s[0].1, 12.0) && near_in(s[1].1, 12.0), "{s:?}");
}

/// Lane 42: a glue-up never plans a strip narrower than the kerf. #56: nor a last strip
/// under the minimum, the smaller of 1 1/2 in and a quarter of the board: it and the strip
/// before it are made equal. With no joint loss and a 1/8 in kerf, a 6 1/16 in panel on
/// 6 in boards would be a 6 in strip and a 1/16 in sliver, reported too large (#53); now it
/// is 3 1/32 + 3 1/32 in. With a 1/16 in joint loss it was 6 + 1/8 in; now
/// 3 1/16 + 3 1/16 in. #53's 12 1/16 in oak panel, refused then, is now one joint on
/// 4/4 x 8 (8 + 4 1/16 in), the first of the boards that glue it from two strips.
#[test]
fn a_filler_under_the_minimum_is_made_equal_with_the_strip_before_it() {
    for (joint, each) in [(0.0, 3.03125), (0.0625, 3.0625)] {
        let (list, n) =
            on_six_inch_boards([0.75 * IN, 6.0625 * IN, 40.0 * IN], joint * IN, IN / 8.0);
        check_nesting_of(&list, &n).unwrap();
        assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
        let s = strips_of(&n, 0);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(near_in(s[0].1, each) && near_in(s[1].1, each), "{s:?}");
    }
    let n = oak(&[(0.75, 12.0625, 40.0, 1)], 0.0);
    let s = strips_of(&n, 0);
    assert!(s.iter().all(|(stock, _)| stock == "4/4 x 8 x 96"), "{s:?}");
    assert!(near_in(s[0].1, 8.0) && near_in(s[1].1, 4.0625), "{s:?}");
    // #56's example: a 6.003 in part on 6 in boards, with a 1/8 in joint loss, would be
    // 6 + 0.128 in; the minimum on a 6 in board is 1 1/2 in, so it is 3.064 + 3.064 in.
    let (list, n) = on_six_inch_boards([0.75 * IN, 6.003 * IN, 20.0 * IN], IN / 8.0, IN / 8.0);
    check_nesting_of(&list, &n).unwrap();
    let s = strips_of(&n, 0);
    assert_eq!(s.len(), 2, "{s:?}");
    assert!(near_in(s[0].1, 3.064) && near_in(s[1].1, 3.064), "{s:?}");
    // A filler at the minimum or wider stays: 7 1/2 in on 6 in boards with no joint loss is
    // 6 + 1 1/2 in.
    let (list, n) = on_six_inch_boards([0.75 * IN, 7.5 * IN, 20.0 * IN], 0.0, IN / 8.0);
    check_nesting_of(&list, &n).unwrap();
    let s = strips_of(&n, 0);
    assert!(near_in(s[0].1, 6.0) && near_in(s[1].1, 1.5), "{s:?}");
    // A quarter of the board when that is less: on 4 in boards the minimum is 1 in, so
    // 5.1 in is 4 + 1.1 in, and 4.9 in is 4 + 0.9 in made equal, 2.45 + 2.45 in.
    let on_four = |w: f64| {
        let list = list_of(&[(0.75, w, 30.0, 1, Grain::Length, false, 0)], 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks
            .retain(|s| s.kind == StockKind::Sheet || s.name == "4/4 x 4 x 96");
        let st = NestSettings {
            joint_loss: 0.0,
            ..NestSettings::default()
        };
        let n = nest_list(&list, &lib, st);
        check_nesting_of(&list, &n).unwrap();
        strips_of(&n, 0)
    };
    let s = on_four(5.1);
    assert!(near_in(s[0].1, 4.0) && near_in(s[1].1, 1.1), "{s:?}");
    let s = on_four(4.9);
    assert!(near_in(s[0].1, 2.45) && near_in(s[1].1, 2.45), "{s:?}");
}

/// Lane 42: a glue-up has a bounded number of strips. A 1/2 in board with a 0.49 in joint
/// loss gains 0.01 in a strip, so a 12 in panel would take 1,151 strips (and a wide enough
/// part more than memory holds): it is reported too large with its board, as is a part on a
/// board no wider than the joint loss. A hundred 1 in strips still make a 100 in panel, and
/// a part a hair wider than two whole boards never puts a strip past its board.
#[test]
fn a_glue_up_has_a_bounded_number_of_strips() {
    let nest_on = |board_w: f64, board_l: f64, part: (f64, f64), joint: f64| {
        let list = list_of(&[(0.25, part.0, part.1, 1, Grain::Length, false, 0)], 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks = vec![StockDef {
            name: "Strip stock".into(),
            kind: StockKind::Board,
            length: board_l * IN,
            width: board_w * IN,
            thickness: 0.25 * IN,
            nominal: None,
            grain: StockGrain::Length,
        }];
        let st = NestSettings {
            joint_loss: joint * IN,
            ..NestSettings::default()
        };
        let n = nest_list(&list, &lib, st);
        check_nesting_of(&list, &n).unwrap();
        n
    };
    let too_large = |n: &Nesting| {
        n.glue_ups.is_empty()
            && n.unplaced.len() == 1
            && n.unplaced[0].reason == UnplacedReason::TooLarge
            && n.unplaced[0].stock.as_deref() == Some("Strip stock")
    };
    let n = nest_on(0.5, 120.0, (12.0, 30.0), 0.49);
    assert!(
        too_large(&n),
        "{:#?}",
        n.glue_ups.first().map(|g| g.widths.len())
    );
    assert!(too_large(&nest_on(0.5, 120.0, (12.0, 30.0), 0.5)));
    assert!(too_large(&nest_on(0.375, 120.0, (12.0, 30.0), 0.5)));
    let n = nest_on(1.0, 120.0, (100.0, 110.0), 0.0);
    let s = strips_of(&n, 0);
    assert_eq!(
        s.len(),
        sawhorse_kernel::nesting::MAX_GLUE_STRIPS,
        "{}",
        s.len()
    );
    assert!(s.iter().all(|(_, w)| near_in(*w, 1.0)));
    assert!(too_large(&nest_on(1.0, 120.0, (100.5, 110.0), 0.0)));
    // 4 m boards (the library takes up to 10 m): 8 m and 2 nm is two whole boards and a
    // 2 nm filler; never one strip 2 nm wider than its board. #56: the filler and the board
    // before it are made two equal strips, 2 m and 1 nm each (#53 refused it).
    let n = nest_on(4.0 / IN, 10.0 / IN, ((8.0 + 2e-9) / IN, 9.0 / IN), 0.0);
    let s = strips_of(&n, 0);
    assert_eq!(s.len(), 3, "{s:?}");
    assert!(
        (s[0].1 * IN - 4.0).abs() < 1e-12
            && (s[1].1 * IN - (2.0 + 1e-9)).abs() < 1e-12
            && (s[2].1 * IN - (2.0 + 1e-9)).abs() < 1e-12,
        "{s:?}"
    );
}

/// One part of the size given (metres, as the cut list row holds it: thickness, width,
/// length) on a library of only 6 x 96 in boards of 3/4 in, no trim, the joint loss and
/// kerf given (metres).
fn on_six_inch_boards(size: [f64; 3], joint: f64, kerf: f64) -> (CutList, Nesting) {
    let mut list = list_of(&[(0.75, 10.0, 20.0, 1, Grain::Length, false, 0)], 0);
    list.rows[0].size = size;
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    lib.stocks = vec![StockDef {
        name: "Six".into(),
        kind: StockKind::Board,
        length: 96.0 * IN,
        width: 6.0 * IN,
        thickness: 0.75 * IN,
        nominal: None,
        grain: StockGrain::Length,
    }];
    let st = NestSettings {
        kerf,
        trim: 0.0,
        algorithm: NestAlgorithm::Guillotine,
        joint_loss: joint,
    };
    let n = nest_list(&list, &lib, st);
    (list, n)
}

/// Lane 42 fix pass: a part a hair wider than whole boards. The strip count came from a
/// quotient and the filler from a sum, and the two can round apart: a part 12 in and 1 nm
/// wide on 6 in boards with no joint loss was counted as 2 strips with a filler a hair wider
/// than 6 in and 1 nm, which the board layout refused, a panic ("only fitting parts are laid
/// out"). The count is settled on the filler, so that part is three strips; #56 makes its
/// 1 nm filler and the board before it two equal strips, 6 + 3 + 3 in and a hair (#53
/// reported it too large). Every width a hundred floating point steps either side of each
/// such boundary, for several strip counts, joint losses and kerfs, nests without a panic,
/// keeps every invariant, never puts a strip wider than its board, and never one narrower
/// than the minimum.
#[test]
fn a_part_a_hair_wider_than_whole_boards_never_panics() {
    let (list, n) = on_six_inch_boards([0.75 * IN, 0.304_800_001, 20.0 * IN], 0.0, IN / 8.0);
    check_nesting_of(&list, &n).unwrap();
    assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
    let s = strips_of(&n, 0);
    assert_eq!(s.len(), 3, "{s:?}");
    assert!(
        near_in(s[0].1, 6.0) && (s[1].1 - 3.0).abs() < 1e-6 && s[1].1 == s[2].1,
        "{s:?}"
    );
    let h = 6.0 * IN;
    let min = sawhorse_kernel::nesting::glue_minimum(h);
    let (mut glued, mut made_equal) = (0, 0);
    for joint in [0.0, IN / 16.0, IN / 8.0, IN / 2.0] {
        for kerf in [0.0, IN / 8.0] {
            for strips in [2usize, 3, 5, 40] {
                // The width at which `strips - 1` full strips leave a filler exactly as wide
                // as the board and 1 nm: one strip more past it, the same count below it.
                let edge = (strips - 1) as f64 * (h - joint) + h + 1e-9;
                for step in -100i64..=100 {
                    let w = f64::from_bits(edge.to_bits().wrapping_add_signed(step));
                    let (list, n) = on_six_inch_boards([0.75 * IN, w, 20.0 * IN], joint, kerf);
                    check_nesting_of(&list, &n)
                        .unwrap_or_else(|e| panic!("{w} m, joint {joint}, kerf {kerf}: {e}"));
                    for q in n.pieces.iter().flat_map(|p| &p.placements) {
                        assert!(q.dy <= h + 1e-9, "{w} m: a strip {} m wide", q.dy);
                        assert!(q.dy >= min - 1e-9, "{w} m: a strip {} m wide", q.dy);
                    }
                    assert!(n.unplaced.is_empty(), "{w} m: {:#?}", n.unplaced);
                    glued += n.glue_ups.len();
                    made_equal += n
                        .glue_ups
                        .iter()
                        .filter(|g| {
                            g.widths[g.widths.len() - 1] < h - 1e-9 && {
                                let k = g.widths.len();
                                g.widths[k - 1] == g.widths[k - 2]
                            }
                        })
                        .count();
                }
            }
        }
    }
    // Below each edge the filler is a whole board or nearly; just past it, it would be a
    // hair and is made equal with the board before it.
    assert!(
        glued > 0 && made_equal > 0,
        "{glued} glued, {made_equal} made equal"
    );
}

/// Lane 42 fix pass: a part of no length is never glued up, even when it is wider than
/// every board; it is reported as before #49 (a 30 in part of no length would fit only
/// turned, its 30 in along the board). A real model never makes one (the cut list sorts a
/// part's sizes, so a part of no length has no thickness and is not counted), but the
/// kernel took such a row and made strips of no length from it, and those beside a part of
/// no width, with no kerf, sent the cut sequence round without end.
#[test]
fn a_part_of_no_length_is_never_glued() {
    for l in [0.0, 1e-12, 1e-9] {
        let (_, n) = on_six_inch_boards([0.75 * IN, 30.0 * IN, l], 0.0, 0.0);
        assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
        assert_eq!(n.unplaced.len(), 1, "{:#?}", n.unplaced);
        assert_eq!(n.unplaced[0].reason, UnplacedReason::Grain);
    }
    let mut list = list_of(
        &[
            (0.75, 1.0, 10.0, 1, Grain::Length, false, 0),
            (0.75, 1.0, 10.0, 1, Grain::Length, false, 0),
        ],
        0,
    );
    list.rows[0].size = [0.75 * IN, 0.0, 10.0 * IN];
    list.rows[1].size = [0.75 * IN, 30.0 * IN, 0.0];
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    let st = NestSettings {
        kerf: 0.0,
        ..NestSettings::default()
    };
    let n = nest_list(&list, &lib, st);
    assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
    assert_eq!(n.totals.unplaced, 1, "{:#?}", n.unplaced);
}

/// Lane 42 fix pass: parts of no size end their cut sequence. With no kerf, a part of no
/// length laid out after another (or of no width beside one) sits on the far end of the
/// region the cut before it left, and the first cut found was "just before it", which left
/// the same region with the same part, made over and over until memory ran out (1.9 GB
/// asked for, then an abort). Older than #49 and not reached from a model, whose cut list
/// never counts a part of no thickness; the kernel takes any list, so it must end on any.
#[test]
fn parts_of_no_size_end_their_cut_sequence() {
    let cases: [&[([f64; 3], usize)]; 3] = [
        &[([0.75, 0.0, 10.0], 1), ([0.75, 5.0, 0.0], 1)],
        &[([0.75, 0.0, 10.0], 1), ([0.75, 8.0, 0.0], 1)],
        &[([0.75, 0.0, 10.0], 3), ([0.75, 4.0, 0.0], 3)],
    ];
    for parts in cases {
        let rows: Vec<_> = parts
            .iter()
            .map(|&(_, q)| (0.75, 1.0, 10.0, q, Grain::Length, false, 0u8))
            .collect();
        let mut list = list_of(&rows, 0);
        for (row, &(size, _)) in list.rows.iter_mut().zip(parts) {
            row.size = size.map(|s| s * IN);
        }
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        for kerf in [0.0, IN / 8.0] {
            let st = NestSettings {
                kerf,
                ..NestSettings::default()
            };
            let n = nest_list(&list, &lib, st);
            let placed: usize = n.pieces.iter().map(|p| p.placements.len()).sum();
            let want: usize = parts.iter().map(|p| p.1).sum();
            assert_eq!(placed + n.unplaced.len(), want, "{parts:?}, kerf {kerf}");
        }
    }
}
/// #56: ties. A 13 in part on 4/4 is two strips on 4/4 x 8 (8 + 5 1/8) and on 4/4 x 12
/// (12 + 1 1/8, made 6 9/16 + 6 9/16), so the board the rest of the oak uses decides: 9 in
/// parts put it on 4/4 x 12. With nothing else of that oak, or the rest on 4/4 x 4 (which
/// takes four strips), library order decides: 4/4 x 8 comes before 4/4 x 12.
#[test]
fn a_tie_goes_to_the_board_the_rest_uses_then_to_library_order() {
    let stock = |n: &Nesting, row: usize| {
        let s = strips_of(n, row);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s.iter().all(|(st, _)| *st == s[0].0), "{s:?}");
        s
    };
    let n = oak(&[(0.75, 9.0, 30.0, 1), (0.75, 13.0, 30.0, 1)], 0.125);
    let s = stock(&n, 1);
    assert_eq!(s[0].0, "4/4 x 12 x 96");
    assert!(near_in(s[0].1, 6.5625) && near_in(s[1].1, 6.5625), "{s:?}");
    let n = oak(&[(0.75, 13.0, 30.0, 1)], 0.125);
    let s = stock(&n, 0);
    assert_eq!(s[0].0, "4/4 x 8 x 96");
    assert!(near_in(s[0].1, 8.0) && near_in(s[1].1, 5.125), "{s:?}");
    let n = oak(&[(0.75, 3.0, 30.0, 2), (0.75, 13.0, 30.0, 1)], 0.125);
    assert_eq!(stock(&n, 1)[0].0, "4/4 x 8 x 96");
}

/// #56's example: with 4/4 x 12 x 96 and 4/4 x 6 x 120 in the library, a 15 x 100 in top
/// is too long for the wider board, and #53 reported it too large for it; the longer,
/// narrower board glues it, 6 + 6 + 3 1/4 in. Too large only when no board of its
/// thickness long enough can glue it: at 130 in it is.
#[test]
fn a_board_too_short_for_the_part_is_passed_over_for_one_long_enough() {
    let nest = |len: f64| {
        let list = list_of(&[(0.75, 15.0, len, 1, Grain::Length, false, 0)], 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks.retain(|s| s.kind == StockKind::Sheet);
        for (w, l) in [(12.0, 96.0), (6.0, 120.0)] {
            lib.stocks.push(StockDef {
                name: format!("4/4 x {w} x {l}"),
                kind: StockKind::Board,
                length: l * IN,
                width: w * IN,
                thickness: 0.75 * IN,
                nominal: None,
                grain: StockGrain::Length,
            });
        }
        let st = NestSettings {
            kerf: IN / 8.0,
            trim: 0.0,
            algorithm: NestAlgorithm::Guillotine,
            joint_loss: IN / 8.0,
        };
        let n = nest_list(&list, &lib, st);
        check_nesting_of(&list, &n).unwrap();
        n
    };
    let n = nest(100.0);
    assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
    let s = strips_of(&n, 0);
    assert_eq!(s.len(), 3, "{s:?}");
    assert!(s.iter().all(|(st, _)| st == "4/4 x 6 x 120"), "{s:?}");
    assert!(
        near_in(s[0].1, 6.0) && near_in(s[1].1, 6.0) && near_in(s[2].1, 3.25),
        "{s:?}"
    );
    let n = nest(130.0);
    assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
    assert_eq!(n.unplaced.len(), 1);
    assert_eq!(n.unplaced[0].reason, UnplacedReason::TooLarge);
}

/// #56: a part no wider than the widest board but longer than it goes to a board long
/// enough too: glued from it when no board makes it whole, or whole on it when one does.
/// The independent verifier found the 10 x 100 in part reported too large. A part no
/// board holds or glues keeps the board the rest uses, as before.
#[test]
fn a_part_too_long_for_the_shared_board_goes_to_one_long_enough() {
    let nest = |parts: &[(f64, f64)]| {
        let rows: Vec<_> = parts
            .iter()
            .map(|&(w, l)| (0.75, w, l, 1, Grain::Length, false, 0))
            .collect();
        let list = list_of(&rows, 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks.retain(|s| s.kind == StockKind::Sheet);
        for (w, l) in [(12.0, 96.0), (6.0, 120.0)] {
            lib.stocks.push(StockDef {
                name: format!("4/4 x {w} x {l}"),
                kind: StockKind::Board,
                length: l * IN,
                width: w * IN,
                thickness: 0.75 * IN,
                nominal: None,
                grain: StockGrain::Length,
            });
        }
        let st = NestSettings {
            kerf: IN / 8.0,
            trim: 0.0,
            algorithm: NestAlgorithm::Guillotine,
            joint_loss: IN / 8.0,
        };
        let n = nest_list(&list, &lib, st);
        check_nesting_of(&list, &n).unwrap();
        n
    };
    // Glued: 6 + 4 1/8 in from the longer board, alone and beside a 15 x 100 in part.
    for parts in [&[(10.0, 100.0)][..], &[(10.0, 100.0), (15.0, 100.0)][..]] {
        let n = nest(parts);
        assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
        let s = strips_of(&n, 0);
        assert!(s.iter().all(|(st, _)| st == "4/4 x 6 x 120"), "{s:?}");
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(near_in(s[0].1, 6.0) && near_in(s[1].1, 4.125), "{s:?}");
    }
    // Whole: a 5 x 100 in part beside a 10 x 50 in one fits the longer board in one piece.
    let n = nest(&[(5.0, 100.0), (10.0, 50.0)]);
    assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
    assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
    // Neither: 10 x 130 in is too large, reported on the board the rest would share.
    let n = nest(&[(10.0, 130.0)]);
    assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
    assert_eq!(n.unplaced.len(), 1);
    assert_eq!(n.unplaced[0].reason, UnplacedReason::TooLarge);
    assert_eq!(n.unplaced[0].stock.as_deref(), Some("4/4 x 12 x 96"));
}

/// A part longer than every board is still too large: gluing only adds width.
#[test]
fn a_part_too_long_to_glue_is_still_too_large() {
    let n = oak(&[(0.75, 15.0, 100.0, 1)], 0.125);
    assert_eq!(n.totals.unplaced, 1);
    assert_eq!(n.unplaced[0].reason, UnplacedReason::TooLarge);
    assert!(n.glue_ups.is_empty());
}

/// The joint loss is a document setting like the kerf: 0 to 1/2 in, saved and hashed only
/// when it is not the 1/8 in default, so every document saved before it keeps its file and
/// its hash.
#[test]
fn the_joint_loss_is_saved_and_hashed_only_when_set() {
    let st = NestSettings::default();
    assert_eq!(st.joint_loss, IN / 8.0);
    let json = serde_json::to_value(st).unwrap();
    assert!(json.get("jointLoss").is_none(), "{json}");
    let set = NestSettings {
        joint_loss: IN / 16.0,
        ..st
    };
    assert_eq!(serde_json::to_value(set).unwrap()["jointLoss"], IN / 16.0);
    let back: NestSettings =
        serde_json::from_value(serde_json::json!({ "kerf": IN / 8.0 })).unwrap();
    assert_eq!(back, st);
    for bad in [-1e-6, 0.5 * IN + 1e-6, f64::NAN] {
        assert!(
            NestSettings {
                joint_loss: bad,
                ..st
            }
            .check()
            .is_err(),
            "{bad}"
        );
    }
    assert!(
        NestSettings {
            joint_loss: 0.0,
            ..st
        }
        .check()
        .is_ok()
    );
    // A model with the default library hashes as before; setting the joint loss changes it.
    let (mut d, _) = bookshelf();
    let before = d.hash();
    ok(&mut d, Op::SetNestSettings { settings: st });
    assert_eq!(d.hash(), before);
    ok(&mut d, Op::SetNestSettings { settings: set });
    assert_ne!(d.hash(), before);
}

/// Oak rails on the built-in boards, no material defaults, kerf 1/8 in, no trim.
fn rails(width: f64, length: f64, qty: usize) -> Nesting {
    let list = list_of(&[(0.75, width, length, qty, Grain::Length, false, 0)], 0);
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    let st = NestSettings {
        kerf: IN / 8.0,
        trim: 0.0,
        algorithm: NestAlgorithm::Guillotine,
        ..NestSettings::default()
    };
    let n = nest_list(&list, &lib, st);
    check_nesting_of(&list, &n).unwrap();
    n
}

/// #50: narrow parts share a board's width. Four 1 1/2 x 40 in rails go on a 4/4 x 4 x 96
/// board; each one taking the whole 4 in, two to a board, needed two boards. Crosscut into
/// two 40 in sections, two rails ripped from each, they fit on one.
#[test]
fn narrow_rails_share_a_boards_width() {
    let n = rails(1.5, 40.0, 4);
    assert_eq!(n.totals.unplaced, 0);
    assert_eq!(n.totals.boards, 1, "{:#?}", n.pieces);
    let p = &n.pieces[0];
    assert_eq!(p.stock, "4/4 x 4 x 96");
    assert_eq!(p.placements.len(), 4);
    // Two sections along the board, two rails across each, a kerf apart both ways.
    let mut xs: Vec<f64> = p.placements.iter().map(|q| q.x).collect();
    let mut ys: Vec<f64> = p.placements.iter().map(|q| q.y).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert_eq!(xs, vec![0.0, 40.0 * IN + IN / 8.0]);
    assert_eq!(ys, vec![0.0, 1.5 * IN + IN / 8.0]);
    // The crosscut that frees a section comes before the rips inside it.
    let first_rip = p.cuts.iter().position(|c| c.dir == CutDir::Rip).unwrap();
    let first_cross = p
        .cuts
        .iter()
        .position(|c| c.dir == CutDir::Crosscut)
        .unwrap();
    assert!(first_cross < first_rip, "{:#?}", p.cuts);
}

/// Width nesting changes a layout only when it saves a board: two rails that already fit
/// end to end on one board stay end to end, each on the board's near edge.
#[test]
fn rails_that_fit_end_to_end_stay_end_to_end() {
    let n = rails(1.5, 40.0, 2);
    assert_eq!(n.totals.boards, 1);
    assert!(
        n.pieces[0].placements.iter().all(|q| q.y == 0.0),
        "{:#?}",
        n.pieces
    );
}

/// A part as wide as the board still takes the whole width, and a narrow one shares a
/// section only beside a part with room left across it.
#[test]
fn a_full_width_part_takes_its_own_section() {
    let list = list_of(
        &[
            (0.75, 4.0, 40.0, 1, Grain::Length, false, 0),
            (0.75, 1.5, 50.0, 2, Grain::Length, false, 0),
        ],
        0,
    );
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    let st = NestSettings {
        kerf: IN / 8.0,
        trim: 0.0,
        algorithm: NestAlgorithm::Guillotine,
        ..NestSettings::default()
    };
    let n = nest_list(&list, &lib, st);
    check_nesting_of(&list, &n).unwrap();
    // 40 + 50 + 50 end to end is two boards; the two rails side by side in one 50 in
    // section beside the 40 in part make one.
    assert_eq!(n.totals.boards, 1, "{:#?}", n.pieces);
    let wide = n.pieces[0]
        .placements
        .iter()
        .find(|q| q.dy == 4.0 * IN)
        .unwrap();
    assert_eq!(wide.y, 0.0);
}

/// A section whose parts differ in length and width: two each of 1 1/2 x 40, 1 1/4 x 30
/// and 1 x 20 in on a 4 in board. End to end they take two boards; in sections of 40, 30
/// and 20 in they take one. The cuts come in an order a person can make them, each across
/// the whole piece in hand: a section crosscut off the board, its rips, a crosscut to
/// length where a shorter part rides beside a longer one, then the next section. The parts
/// are listed in the same order, section by section.
#[test]
fn parts_of_different_lengths_share_sections_and_cut_in_order() {
    let list = list_of(
        &[
            (0.75, 1.5, 40.0, 2, Grain::Length, false, 0),
            (0.75, 1.25, 30.0, 2, Grain::Length, false, 0),
            (0.75, 1.0, 20.0, 2, Grain::Length, false, 0),
        ],
        0,
    );
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    let st = NestSettings {
        kerf: IN / 8.0,
        trim: 0.0,
        algorithm: NestAlgorithm::Guillotine,
        ..NestSettings::default()
    };
    let n = nest_list(&list, &lib, st);
    check_nesting_of(&list, &n).unwrap();
    assert_eq!(n.totals.boards, 1, "{:#?}", n.pieces);
    let p = &n.pieces[0];
    assert_eq!(p.stock, "4/4 x 4 x 96");
    assert!(p.guillotine);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    // Sections at 0, 40 1/8 and 70 1/4 in: the two 40s; the two 30s with a 20 beside them;
    // the other 20 alone.
    let want = [
        ("1a", 0.0, 0.0),
        ("1b", 0.0, 1.625),
        ("2a", 40.125, 0.0),
        ("2b", 40.125, 1.375),
        ("3a", 40.125, 2.75),
        ("3b", 70.25, 0.0),
    ];
    assert_eq!(p.placements.len(), want.len());
    for (q, &(tag, x, y)) in p.placements.iter().zip(&want) {
        assert!(
            q.tag == tag && near(q.x / IN, x) && near(q.y / IN, y) && !q.turned,
            "{tag} at ({x}, {y}): {q:#?}"
        );
    }
    use CutDir::{Crosscut, Rip};
    let cuts = [
        (Crosscut, 40.0, 40.0),
        (Rip, 1.5, 1.5),
        (Rip, 3.125, 1.5),
        (Crosscut, 70.125, 30.0),
        (Rip, 1.25, 1.25),
        (Rip, 2.625, 1.25),
        (Crosscut, 60.125, 20.0),
        (Rip, 3.75, 1.0),
        (Crosscut, 90.25, 20.0),
        (Rip, 1.0, 1.0),
    ];
    assert_eq!(p.cuts.len(), cuts.len(), "{:#?}", p.cuts);
    for (k, (c, &(dir, at, size))) in p.cuts.iter().zip(&cuts).enumerate() {
        assert!(
            c.n as usize == k + 1
                && c.dir == dir
                && near(c.at / IN, at)
                && near(c.size / IN, size)
                && !c.trim,
            "cut {}: {c:#?}",
            k + 1
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// No overlaps, every part inside its stock, kerf gaps, grain, parts in = placed +
    /// unplaced, efficiency from the exact areas, cuts that cross no part, guillotine
    /// layouts that come apart, deterministic hashes.
    #[test]
    fn every_nesting_keeps_its_invariants(
        parts in prop::collection::vec(a_part(), 0..14),
        st in settings(),
        lib in library(),
        hidden in any::<u64>(),
        visible_only in any::<bool>(),
    ) {
        let mut list = list_of(&parts, hidden);
        list.visible_only = visible_only;
        let mut lib = lib;
        lib.settings = st;
        let n = nest_list(&list, &lib, st);
        if let Err(e) = check_nesting_of(&list, &n) {
            return Err(TestCaseError::fail(e));
        }
        prop_assert_eq!(nest_list(&list, &lib, st).hash, n.hash.clone());
        // The other algorithm on the same parts places the same pieces.
        let other = NestSettings {
            algorithm: match st.algorithm {
                NestAlgorithm::Guillotine => NestAlgorithm::Maxrects,
                NestAlgorithm::Maxrects => NestAlgorithm::Guillotine,
            },
            ..st
        };
        let m = nest_list(&list, &lib, other);
        if let Err(e) = check_nesting_of(&list, &m) {
            return Err(TestCaseError::fail(e));
        }
        prop_assert_eq!(m.totals.placed, n.totals.placed);
        prop_assert_eq!(m.totals.unplaced, n.totals.unplaced);
    }

    /// One more part never takes fewer stock pieces of its group (monotone in the parts),
    /// and a wider kerf never fits more parts on the first sheet of a single size.
    #[test]
    fn identical_parts_fill_sheets_in_order(
        w in 4.0..30.0f64,
        l in 4.0..60.0f64,
        qty in 1usize..40,
    ) {
        let list = list_of(&[(0.75, w, l, qty, Grain::Length, true, 2)], 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        let st = NestSettings::default();
        let n = nest_list(&list, &lib, st);
        if let Err(e) = check_nesting_of(&list, &n) {
            return Err(TestCaseError::fail(e));
        }
        // Every sheet but the last is at least as full as the last.
        let counts: Vec<usize> = n.pieces.iter().map(|p| p.placements.len()).collect();
        if let Some((last, rest)) = counts.split_last() {
            prop_assert!(rest.iter().all(|c| c >= last), "{:?}", counts);
        }
        let more = list_of(&[(0.75, w, l, qty + 1, Grain::Length, true, 2)], 0);
        let m = nest_list(&more, &lib, st);
        prop_assert!(m.pieces.len() >= n.pieces.len());
    }
}

/// The golden `nesting.json` replays exactly this: the bookshelf with a plywood carcass and
/// a 1/4 ply back, a library edit (a 5 x 10 sheet of Baltic birch, the ply's default), kerf
/// 3/32 in, trim 1/4 in, maxrects. Its ops are written from here under `UPDATE_GOLDEN=1` (then `--test golden`
/// fills in the hashes, the nesting's included), and must match it otherwise.
#[test]
fn the_nesting_golden_is_this_bookshelf() {
    let (mut d, s) = bookshelf();
    plywood(&mut d, &s);
    ok(
        &mut d,
        Op::DefineStock {
            stock: StockDef {
                name: "5 x 10 Baltic birch, 3/4".into(),
                kind: StockKind::Sheet,
                length: 120.0 * IN,
                width: 60.0 * IN,
                thickness: 0.75 * IN,
                nominal: None,
                grain: StockGrain::Length,
            },
            replace: None,
        },
    );
    ok(
        &mut d,
        Op::SetMaterialStock {
            material: "3/4 birch ply".into(),
            stock: Some("5 x 10 Baltic birch, 3/4".into()),
        },
    );
    ok(
        &mut d,
        Op::SetNestSettings {
            settings: NestSettings {
                kerf: 3.0 / 32.0 * IN,
                trim: 0.25 * IN,
                algorithm: NestAlgorithm::Maxrects,
                ..NestSettings::default()
            },
        },
    );
    let n = nest(&d);
    assert_eq!(n.totals.unplaced, 0);
    assert!(
        n.totals
            .groups
            .iter()
            .any(|g| g.stock == "5 x 10 Baltic birch, 3/4")
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/nesting.json");
    let ops = serde_json::to_value(d.log()).unwrap();
    let description = "Lane 9: the bookshelf with a 3/4 birch ply carcass defaulted to a new 5 x 10 Baltic birch stock and a 1/4 ply back, kerf 3/32 in, trim 1/4 in, maxrects. Its nesting hash proves WASM lays out the same sheets as native.";
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        let mut g: serde_json::Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "name": "nesting",
                    "hash": "",
                    "stats": {"objects": 0, "faces": 0, "edges": 0, "vertices": 0, "genus": 0, "watertight": true}
                })
            });
        g["description"] = description.into();
        g["ops"] = ops;
        std::fs::write(&path, serde_json::to_string_pretty(&g).unwrap() + "\n").unwrap();
    } else {
        let g: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            g["ops"], ops,
            "golden/nesting.json is not this bookshelf (UPDATE_GOLDEN=1)"
        );
    }
}
