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
use sawhorse_kernel::nesting::{
    CutDir, NestOptions, Nesting, StockPiece, UnplacedReason, nest_list, up_to_stock,
};
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
    // #57: mostly the #56 rule; else none, half a board's width or more, or any in range.
    let min_strip = prop_oneof![
        3 => Just(None),
        1 => Just(Some(0.0)),
        1 => Just(Some(12.0 * IN)),
        1 => (0.0..(12.0 * IN)).prop_map(Some)
    ];
    (kerf, trim, alg, joint, min_strip).prop_map(
        |(kerf, trim, algorithm, joint_loss, min_strip)| NestSettings {
            kerf,
            trim,
            algorithm,
            joint_loss,
            min_strip,
        },
    )
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
        min_strip: None,
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
        min_strip: None,
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
    let min = sawhorse_kernel::nesting::glue_minimum(h, None);
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
            min_strip: None,
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
            min_strip: None,
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

/// Lane 45 (the review of #62): a part narrow enough for the board the rest of its wood
/// shares but longer than every board of its thickness goes in one piece on a thicker board
/// long enough, as a part wider than every board does since #49; #53 reported it too large.
/// A 3/4 x 3 x 100 in rail beside a 5 in shelf, with 4/4 x 6 x 96 and 5/4 x 6 x 120: the
/// rail is whole on 5/4 x 6 x 120, the shelf stays on 4/4 x 6 x 96. Without the 5/4 board
/// the rail is too large on 4/4 x 6 x 96, as before.
#[test]
fn a_part_too_long_for_its_thickness_goes_whole_on_a_thicker_board_long_enough() {
    let nest = |thicker: bool| {
        let rows = [
            (0.75, 3.0, 100.0, 1, Grain::Length, false, 0),
            (0.75, 5.0, 30.0, 1, Grain::Length, false, 0),
        ];
        let list = list_of(&rows, 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks
            .retain(|s| s.kind == StockKind::Sheet || s.name == "4/4 x 6 x 96");
        if thicker {
            lib.stocks.push(StockDef {
                name: "5/4 x 6 x 120".into(),
                kind: StockKind::Board,
                length: 120.0 * IN,
                width: 6.0 * IN,
                thickness: 1.0625 * IN,
                nominal: Some("5/4".into()),
                grain: StockGrain::Length,
            });
        }
        let n = nest_list(&list, &lib, NestSettings::default());
        check_nesting_of(&list, &n).unwrap();
        n
    };
    let stock_of = |n: &Nesting, row: usize| {
        n.pieces
            .iter()
            .find(|p| p.placements.iter().any(|q| q.row == row))
            .map(|p| p.stock.clone())
    };
    let n = nest(true);
    assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
    assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
    assert_eq!(stock_of(&n, 0).as_deref(), Some("5/4 x 6 x 120"));
    assert_eq!(stock_of(&n, 1).as_deref(), Some("4/4 x 6 x 96"));
    let n = nest(false);
    assert_eq!(n.unplaced.len(), 1, "{:#?}", n.unplaced);
    assert_eq!(n.unplaced[0].row, 0);
    assert_eq!(n.unplaced[0].reason, UnplacedReason::TooLarge);
    assert_eq!(n.unplaced[0].stock.as_deref(), Some("4/4 x 6 x 96"));
}

/// Lane 45 (the review of #62): a part whose stock is said keeps that board even when
/// another board of its thickness glues it from fewer strips. A 17 x 30 in oak side with
/// oak's default stock 4/4 x 6 x 96, or with 4/4 x 6 x 96 named by the part, is three
/// strips of it, 6 + 6 + 5 1/4 in, though 4/4 x 12 would take two.
#[test]
fn a_part_whose_stock_is_said_keeps_it_over_fewer_strips() {
    for named in [false, true] {
        let mut list = list_of(&[(0.75, 17.0, 30.0, 1, Grain::Length, false, 0)], 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        if named {
            list.rows[0].stock_set = true;
            list.rows[0].material = Some("4/4 x 6 x 96".into());
        } else {
            lib.defaults.insert("Oak".into(), "4/4 x 6 x 96".into());
        }
        let n = nest_list(&list, &lib, NestSettings::default());
        check_nesting_of(&list, &n).unwrap();
        let s = strips_of(&n, 0);
        assert_eq!(s.len(), 3, "named {named}: {s:?}");
        assert!(
            s.iter().all(|(st, _)| st == "4/4 x 6 x 96"),
            "named {named}: {s:?}"
        );
        assert!(
            near_in(s[0].1, 6.0) && near_in(s[1].1, 6.0) && near_in(s[2].1, 5.25),
            "named {named}: {s:?}"
        );
    }
}

/// Lane 45 (the review of #62): lane 42's refusal of a strip narrower than the kerf still
/// holds after the last two strips are made equal, on the narrowest of them. A 0.8 x 20 in
/// part on 3/4 in wide strip stock with no joint loss would leave a 0.05 in filler, so its
/// two strips are 0.4 + 0.4 in: glued with a 3/8 in kerf, too large with a 1/2 in kerf.
#[test]
fn equal_strips_narrower_than_the_kerf_are_refused() {
    let nest = |kerf: f64| {
        let list = list_of(&[(0.75, 0.8, 20.0, 1, Grain::Length, false, 0)], 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks.retain(|s| s.kind == StockKind::Sheet);
        lib.stocks.push(StockDef {
            name: "Strip stock".into(),
            kind: StockKind::Board,
            length: 96.0 * IN,
            width: 0.75 * IN,
            thickness: 0.75 * IN,
            nominal: None,
            grain: StockGrain::Length,
        });
        let st = NestSettings {
            kerf: kerf * IN,
            trim: 0.0,
            algorithm: NestAlgorithm::Guillotine,
            joint_loss: 0.0,
            min_strip: None,
        };
        let n = nest_list(&list, &lib, st);
        check_nesting_of(&list, &n).unwrap();
        n
    };
    let n = nest(0.375);
    assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
    let s = strips_of(&n, 0);
    assert_eq!(s.len(), 2, "{s:?}");
    assert!(near_in(s[0].1, 0.4) && near_in(s[1].1, 0.4), "{s:?}");
    let n = nest(0.5);
    assert!(n.glue_ups.is_empty(), "{:#?}", n.glue_ups);
    assert_eq!(n.unplaced.len(), 1, "{:#?}", n.unplaced);
    assert_eq!(n.unplaced[0].reason, UnplacedReason::TooLarge);
    assert_eq!(n.unplaced[0].stock.as_deref(), Some("Strip stock"));
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

/// #57: the minimum strip is a document setting: absent (the #56 rule) unless set, 0 to
/// 12 in, saved and hashed only when set, so every document saved before it keeps its file
/// and its hash.
#[test]
fn the_minimum_strip_is_saved_and_hashed_only_when_set() {
    let st = NestSettings::default();
    assert_eq!(st.min_strip, None);
    let json = serde_json::to_value(st).unwrap();
    assert!(json.get("minStrip").is_none(), "{json}");
    let set = NestSettings {
        min_strip: Some(2.0 * IN),
        ..st
    };
    assert_eq!(serde_json::to_value(set).unwrap()["minStrip"], 2.0 * IN);
    let back: NestSettings = serde_json::from_value(serde_json::to_value(set).unwrap()).unwrap();
    assert_eq!(back, set);
    let old: NestSettings =
        serde_json::from_value(serde_json::json!({ "kerf": IN / 8.0 })).unwrap();
    assert_eq!(old, st);
    for bad in [-1e-6, 12.0 * IN + 1e-6, f64::NAN, f64::INFINITY] {
        assert!(
            NestSettings {
                min_strip: Some(bad),
                ..st
            }
            .check()
            .is_err(),
            "{bad}"
        );
    }
    for good in [0.0, 12.0 * IN] {
        assert!(
            NestSettings {
                min_strip: Some(good),
                ..st
            }
            .check()
            .is_ok(),
            "{good}"
        );
    }
    // A model hashes as before until the setting is set, and as before again once cleared.
    let (mut d, _) = bookshelf();
    let before = d.hash();
    ok(&mut d, Op::SetNestSettings { settings: st });
    assert_eq!(d.hash(), before);
    // The value is in the hash, not only that the library was edited: 3 in hashes apart
    // from 2 in.
    ok(
        &mut d,
        Op::SetNestSettings {
            settings: NestSettings {
                min_strip: Some(3.0 * IN),
                ..st
            },
        },
    );
    let three = d.hash();
    ok(&mut d, Op::SetNestSettings { settings: set });
    let with = d.hash();
    assert_ne!(with, before);
    assert_ne!(with, three);
    // Saved and opened: the setting, the hash and the nesting come back.
    let json = serde_json::to_string(&d.model().snapshot()).unwrap();
    assert!(json.contains("\"minStrip\""), "{json}");
    let back =
        sawhorse_kernel::Model::from_snapshot(&serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(back.hash_hex(), d.model().hash_hex());
    assert_eq!(back.stock_library().settings.min_strip, Some(2.0 * IN));
    assert_eq!(
        back.nest(&NestOptions::default()).hash,
        d.model().nest(&NestOptions::default()).hash
    );
    // Out of range: a typed refusal that changes nothing.
    let e = d
        .apply(&Op::SetNestSettings {
            settings: NestSettings {
                min_strip: Some(-1.0),
                ..st
            },
        })
        .unwrap_err();
    assert!(!e.is_bug(), "{e}");
    assert_eq!(d.hash(), with);
    // Undo takes it back to 3 in and then to the rule, Redo forward again; cleared, the old
    // hash and no "minStrip" in the file.
    d.undo().unwrap();
    assert_eq!(d.hash(), three);
    d.undo().unwrap();
    assert_eq!(d.hash(), before);
    d.redo().unwrap();
    d.redo().unwrap();
    assert_eq!(d.hash(), with);
    ok(&mut d, Op::SetNestSettings { settings: st });
    assert_eq!(d.hash(), before);
    assert!(
        !serde_json::to_string(&d.model().snapshot())
            .unwrap()
            .contains("minStrip")
    );
}

/// The strip widths (in) of one part `part_w` in wide and 20 in long glued from a library of
/// only `board_w` x 96 in boards of 3/4 in: no joint loss, kerf 1/8 in, the minimum strip
/// given. `None` when it is not glued.
fn glued_from(board_w: f64, part_w: f64, min_strip: Option<f64>) -> Option<Vec<f64>> {
    let list = list_of(&[(0.75, part_w, 20.0, 1, Grain::Length, false, 0)], 0);
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    lib.stocks = vec![StockDef {
        name: "Board".into(),
        kind: StockKind::Board,
        length: 96.0 * IN,
        width: board_w * IN,
        thickness: 0.75 * IN,
        nominal: None,
        grain: StockGrain::Length,
    }];
    let st = NestSettings {
        kerf: IN / 8.0,
        joint_loss: 0.0,
        min_strip,
        ..NestSettings::default()
    };
    let n = nest_list(&list, &lib, st);
    check_nesting_of(&list, &n).unwrap_or_else(|e| panic!("{part_w} in, {min_strip:?}: {e}"));
    n.glue_ups
        .first()
        .map(|g| g.widths.iter().map(|w| w / IN).collect())
}

fn widths_are(got: Option<Vec<f64>>, want: &[f64]) {
    let got = got.expect("glued");
    assert!(
        got.len() == want.len() && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9),
        "{got:?}, want {want:?}"
    );
}

/// #57: a set minimum strip evens out the last strip with the one before it when it is
/// narrower than the setting, or half the board when that is smaller; the #56 rule's
/// quarter-board cap is the default's only. 0 never evens out: the last strip has only to
/// clear the kerf and 1 mm, as before #56.
#[test]
fn a_set_minimum_strip_evens_the_last_strip_up_to_half_the_board() {
    // Absent: the #56 rule, 1 1/2 in on a 6 in board. A 1 1/2 in last strip stays.
    widths_are(glued_from(6.0, 13.5, None), &[6.0, 6.0, 1.5]);
    widths_are(glued_from(6.0, 12.25, None), &[6.0, 3.125, 3.125]);
    // Set to 2 in, the 1 1/2 in last strip is evened out with the full one before it.
    widths_are(glued_from(6.0, 13.5, Some(2.0 * IN)), &[6.0, 3.75, 3.75]);
    widths_are(glued_from(6.0, 14.0, Some(2.0 * IN)), &[6.0, 6.0, 2.0]);
    // Set past half the board, it holds at half: 3 in on a 6 in board.
    widths_are(glued_from(6.0, 14.99, Some(4.0 * IN)), &[6.0, 4.495, 4.495]);
    widths_are(glued_from(6.0, 15.0, Some(4.0 * IN)), &[6.0, 6.0, 3.0]);
    widths_are(glued_from(6.0, 15.01, Some(4.0 * IN)), &[6.0, 6.0, 3.01]);
    // On a 2 in board the rule's quarter cap (1/2 in) leaves a 0.6 in last strip; a set
    // 3/4 in is what it says (under half the board, 1 in).
    widths_are(glued_from(2.0, 4.6, None), &[2.0, 2.0, 0.6]);
    widths_are(glued_from(2.0, 4.6, Some(0.75 * IN)), &[2.0, 1.3, 1.3]);
    // 0: never evened out. A 1/4 in last strip stays; one narrower than the kerf is refused
    // as before #56, where the rule would have evened it out.
    widths_are(glued_from(6.0, 12.25, Some(0.0)), &[6.0, 6.0, 0.25]);
    widths_are(glued_from(6.0, 12.0625, None), &[6.0, 3.03125, 3.03125]);
    assert_eq!(glued_from(6.0, 12.0625, Some(0.0)), None);
}

/// #57 (lane 56): the board a part is glued from is chosen with the setting it is planned
/// with. A 12 1/16 x 20 in part, kerf 1/8 in, no joint loss, on a library of 4/4 x 6 x 96
/// then 4/4 x 4 1/2 x 96. Both take three strips, so library order decides:
/// - 6 in: 6 + 6 + 1/16 in. The rule evens the 1/16 in out (under 1 1/2 in), 6 + 3 1/32 +
///   3 1/32 in; set to 0 it stays, narrower than the kerf, and the board is refused.
/// - 4 1/2 in: 4 1/2 + 4 1/2 + 3 1/16 in, over 1 1/8 in (a quarter of the board), so the
///   same with the setting absent or 0.
///
/// Absent, the 6 in board glues it. Set to 0, the 4 1/2 in board does; a choice made as if
/// the setting were absent takes the 6 in board, which then cannot make it, and the part is
/// left on no stock.
#[test]
fn the_board_a_part_is_glued_from_is_chosen_with_the_minimum_strip() {
    let nest = |min_strip: Option<f64>| {
        let list = list_of(&[(0.75, 12.0625, 20.0, 1, Grain::Length, false, 0)], 0);
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks = [(6.0, "4/4 x 6 x 96"), (4.5, "4/4 x 4 1/2 x 96")]
            .into_iter()
            .map(|(w, name)| StockDef {
                name: name.into(),
                kind: StockKind::Board,
                length: 96.0 * IN,
                width: w * IN,
                thickness: 0.75 * IN,
                nominal: None,
                grain: StockGrain::Length,
            })
            .collect();
        let st = NestSettings {
            kerf: IN / 8.0,
            joint_loss: 0.0,
            min_strip,
            ..NestSettings::default()
        };
        let n = nest_list(&list, &lib, st);
        check_nesting_of(&list, &n).unwrap_or_else(|e| panic!("{min_strip:?}: {e}"));
        n
    };
    let n = nest(None);
    assert!(n.unplaced.is_empty(), "{:#?}", n.unplaced);
    let s = strips_of(&n, 0);
    assert!(s.iter().all(|(st, _)| st == "4/4 x 6 x 96"), "{s:?}");
    widths_are(
        Some(s.iter().map(|(_, w)| *w).collect()),
        &[6.0, 3.03125, 3.03125],
    );
    let n = nest(Some(0.0));
    assert!(n.unplaced.is_empty(), "set to 0: {:#?}", n.unplaced);
    let s = strips_of(&n, 0);
    assert!(s.iter().all(|(st, _)| st == "4/4 x 4 1/2 x 96"), "{s:?}");
    widths_are(
        Some(s.iter().map(|(_, w)| *w).collect()),
        &[4.5, 4.5, 3.0625],
    );
    assert_eq!(n.glue_ups.len(), 1);
    assert_eq!(n.glue_ups[0].stock, "4/4 x 4 1/2 x 96");
}

/// #57, to the last bit: the setting's range ends at 12 in, written 0.3048 m (12 in typed,
/// `12 * 0.0254`, rounds a hair under it and is taken too), and the cap is exactly half the
/// board. A last strip of exactly half the board stays; one a hair under it is evened out,
/// never under half.
#[test]
fn the_minimum_strip_s_bounds_hold_to_the_last_bit() {
    use sawhorse_kernel::nesting::glue_minimum;
    let st = NestSettings::default();
    let check = |m: f64| {
        NestSettings {
            min_strip: Some(m),
            ..st
        }
        .check()
    };
    for good in [0.0, 12.0 * IN, 0.3048] {
        assert!(check(good).is_ok(), "{good:e}");
    }
    for bad in [0.3048f64.next_up(), 12.01 * IN, 0.0f64.next_down()] {
        assert!(check(bad).is_err(), "{bad:e}");
    }
    for board in [2.0 * IN, 3.0 * IN, 6.0 * IN, 0.125, 0.1] {
        let half: f64 = 0.5 * board;
        for (set, want) in [
            (half.next_down(), half.next_down()),
            (half, half),
            (half.next_up(), half),
            (0.3048, half),
        ] {
            assert_eq!(
                glue_minimum(board, Some(set)).to_bits(),
                want.to_bits(),
                "{board:e} {set:e}"
            );
        }
    }
    // A 0.1875 m part on 0.125 m boards, no joint loss: its filler is exactly 0.0625 m, half
    // the board. Set past half, the filler stays; a part one step narrower is evened out.
    let strips = |w: f64| {
        let mut list = list_of(&[(0.75, 1.0, 20.0, 1, Grain::Length, false, 0)], 0);
        list.rows[0].size[1] = w;
        let mut lib = StockLibrary::builtin();
        lib.defaults.clear();
        lib.stocks = vec![StockDef {
            name: "Board".into(),
            kind: StockKind::Board,
            length: 96.0 * IN,
            width: 0.125,
            thickness: 0.75 * IN,
            nominal: None,
            grain: StockGrain::Length,
        }];
        let st = NestSettings {
            kerf: IN / 8.0,
            joint_loss: 0.0,
            min_strip: Some(0.1),
            ..NestSettings::default()
        };
        let n = nest_list(&list, &lib, st);
        check_nesting_of(&list, &n).unwrap();
        n.glue_ups[0].widths.clone()
    };
    assert_eq!(strips(0.1875), vec![0.125, 0.0625]);
    let evened = strips(0.1875f64.next_down());
    assert_eq!(evened.len(), 2);
    assert!(
        evened.iter().all(|w| (0.0625..0.125).contains(w)),
        "{evened:?}"
    );
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

// ---- lane 51: a filled sheet or board shows no waste ------------------------------------

/// Plain parts nested with no kerf and no trim (inches), the library with no defaults.
fn filled(parts: &[(f64, f64, f64, usize, bool)]) -> Nesting {
    let rows: Vec<_> = parts
        .iter()
        .map(|&(t, w, l, q, sheet)| (t, w, l, q, Grain::Length, sheet, 0u8))
        .collect();
    let list = list_of(&rows, 0);
    let mut lib = StockLibrary::builtin();
    lib.defaults.clear();
    let st = NestSettings {
        kerf: 0.0,
        trim: 0.0,
        algorithm: NestAlgorithm::Guillotine,
        ..NestSettings::default()
    };
    let n = nest_list(&list, &lib, st);
    check_nesting_of(&list, &n).unwrap();
    n
}

/// The parts' areas on a piece added up as the nesting adds them, before lane 51.
fn raw_parts(p: &StockPiece) -> f64 {
    p.placements.iter().map(|q| q.dx * q.dy).sum()
}

/// Every piece, group and the totals of a nesting whose stock is all filled: exactly no
/// waste, with a positive sign, and an efficiency of exactly 1.
fn all_full(n: &Nesting) {
    for p in &n.pieces {
        assert_eq!(p.parts_area, p.stock_area, "piece {}", p.index);
        assert_eq!(
            p.waste_area.to_bits(),
            0.0f64.to_bits(),
            "piece {}",
            p.index
        );
        assert_eq!(p.efficiency, 1.0, "piece {}", p.index);
    }
    for g in &n.totals.groups {
        assert_eq!(g.parts_area, g.stock_area, "group {}", g.stock);
        assert_eq!(g.efficiency, 1.0, "group {}", g.stock);
    }
    let t = &n.totals;
    assert_eq!(t.parts_area, t.stock_area);
    assert_eq!((t.stock_area - t.parts_area).to_bits(), 0.0f64.to_bits());
    assert_eq!(t.efficiency, 1.0);
}

/// Nine 16 x 32 in parts fill a 4 x 8 sheet, and three 4 x 32 in parts a 4/4 x 4 x 96
/// board, with no kerf and no trim; added up, their areas came to a hair more than the
/// stock's (4.4e-16 m² and 2.8e-17 m²), which the Nesting panel showed as "-0.00 sq ft
/// waste". Now each reads exactly no waste and 100%.
#[test]
fn parts_that_fill_a_sheet_or_a_board_leave_exactly_no_waste() {
    let sheet = filled(&[(23.0 / 32.0, 16.0, 32.0, 9, true)]);
    let board = filled(&[(0.75, 4.0, 32.0, 3, false)]);
    for (n, stock, parts) in [(&sheet, "4 x 8 sheet, 3/4", 9), (&board, "4/4 x 4 x 96", 3)] {
        assert_eq!((n.totals.unplaced, n.pieces.len()), (0, 1), "{stock}");
        let p = &n.pieces[0];
        assert_eq!((p.stock.as_str(), p.placements.len()), (stock, parts));
        // The case is the one rounding put over: the plain sum is more than the stock.
        assert!(raw_parts(p) > p.stock_area, "{stock}: {:e}", raw_parts(p));
        all_full(n);
    }
}

/// A part exactly a 5 x 5 Baltic birch sheet, ten of them: each sheet's area is its own,
/// but ten sheets' areas added one by one came to more than the sheet's area times ten,
/// so the group read a hair over 100%. Now the group and the totals read exactly 1.
#[test]
fn ten_parts_each_a_whole_sheet_fill_their_group_exactly() {
    let n = filled(&[(18.0 / 25.4, 60.0, 60.0, 10, true)]);
    assert_eq!(n.totals.unplaced, 0);
    assert_eq!(n.totals.groups.len(), 1);
    let g = &n.totals.groups[0];
    assert_eq!((g.stock.as_str(), g.count), ("5 x 5 Baltic birch, 3/4", 10));
    let one = n.pieces[0].stock_area;
    let added = n.pieces.iter().fold(0.0, |s, p| s + raw_parts(p));
    assert!(added > one * 10.0, "{added:e} {:e}", one * 10.0);
    all_full(&n);
}

/// What is more than rounding can make still shows: a part half a nanometre larger than the
/// 5 x 5 sheet each way (inside the fits' 1 nm slack) comes to about 1.5e-9 m² more than
/// the sheet, so its waste is below zero and its efficiency over 1.
#[test]
fn parts_truly_larger_than_their_stock_still_show() {
    let over = 0.5e-9 / IN;
    let n = filled(&[(18.0 / 25.4, 60.0 + over, 60.0 + over, 1, true)]);
    assert_eq!((n.totals.unplaced, n.pieces.len()), (0, 1));
    let p = &n.pieces[0];
    assert_eq!(p.parts_area, raw_parts(p));
    assert!(p.waste_area < -1e-9, "{:e}", p.waste_area);
    assert!(p.efficiency > 1.0);
    assert!(n.totals.groups[0].efficiency > 1.0);
    assert!(n.totals.efficiency > 1.0);
    // The rule itself, at and past its slack.
    let slack = |terms: usize| (terms as f64 + 2.0) * f64::EPSILON;
    assert_eq!(up_to_stock(1.0 + slack(9), 1.0, 9), 1.0);
    assert_eq!(
        up_to_stock(1.0 + 2.0 * slack(9), 1.0, 9),
        1.0 + 2.0 * slack(9)
    );
    assert_eq!(up_to_stock(0.5, 1.0, 9), 0.5);
    assert_eq!(up_to_stock(-0.0f64, 1.0, 0).to_bits(), (-0.0f64).to_bits());
}

/// Stock the parts do not fill reads bit for bit as before lane 51: every piece's, group's
/// and total's numbers equal the sums and quotients the nesting worked them out with.
#[test]
fn stock_not_filled_reads_bit_for_bit_as_before() {
    let (mut d, s) = bookshelf();
    let mut nestings = vec![nest(&d)];
    plywood(&mut d, &s);
    nestings.push(nest(&d));
    nestings.push(filled(&[(23.0 / 32.0, 16.0, 31.0, 9, true)]));
    nestings.push(rails(1.5, 40.0, 4));
    for n in &nestings {
        let mut groups: Vec<(f64, f64)> = vec![(0.0, 0.0); n.totals.groups.len()];
        for p in &n.pieces {
            let parts = raw_parts(p);
            let stock = p.length * p.width;
            assert!(parts < stock);
            assert_eq!(p.parts_area.to_bits(), parts.to_bits());
            assert_eq!(p.stock_area.to_bits(), stock.to_bits());
            assert_eq!(p.waste_area.to_bits(), (stock - parts).to_bits());
            assert_eq!(p.efficiency.to_bits(), (parts / stock).to_bits());
            groups[p.group].0 += parts;
            groups[p.group].1 = stock * p.count as f64;
        }
        for (g, (parts, stock)) in n.totals.groups.iter().zip(&groups) {
            assert_eq!(g.parts_area.to_bits(), parts.to_bits());
            assert_eq!(g.stock_area.to_bits(), stock.to_bits());
            assert_eq!(g.efficiency.to_bits(), (parts / stock).to_bits());
        }
        let parts = n.pieces.iter().fold(0.0, |s, p| s + raw_parts(p));
        let stock = n.pieces.iter().fold(0.0, |s, p| s + p.length * p.width);
        let t = &n.totals;
        assert_eq!(t.parts_area.to_bits(), parts.to_bits());
        assert_eq!(t.stock_area.to_bits(), stock.to_bits());
        assert_eq!(t.efficiency.to_bits(), (parts / stock).to_bits());
    }
    // And the bookshelf's totals as they were (6,105 and 10,368 in²), to the bit.
    let t = &nestings[0].totals;
    assert_eq!(
        (
            t.parts_area.to_bits(),
            t.stock_area.to_bits(),
            t.efficiency.to_bits()
        ),
        (BOOKSHELF_BITS[0], BOOKSHELF_BITS[1], BOOKSHELF_BITS[2])
    );
}

/// The bookshelf's total parts area, stock area and efficiency before lane 51 (f64 bits).
const BOOKSHELF_BITS: [u64; 3] = [
    4616051586779419158,
    4619217184161248456,
    4603478937856444795,
];
