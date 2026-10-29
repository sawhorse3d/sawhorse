//! A scene remembers the ground grid (lane 26, issue #6). The grid is the style's `grid`
//! switch (lane 21); a scene saved with the default style stores no style, so before lane
//! 26 going to it left the grid as it was. A scene now carries its own `grid` switch, which
//! activating it writes into the style: two scenes with opposite grids flip it both ways,
//! from no style too, in one undo step; a scene without one (older files, imported scenes)
//! leaves the grid alone; the switch survives the snapshot; and drawing a scene on a sheet
//! does not change with it.

use sawhorse_kernel::looks::{FaceStyle, Style};
use sawhorse_kernel::model::{Model, Op, OpOutcome, SceneDef};
use sawhorse_kernel::{Document, KernelError};

fn ok(d: &mut Document, op: Op) -> OpOutcome {
    d.apply(&op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

/// A scene as the app saves it (src/App.tsx `sceneHere`): the style as the document holds
/// it (`None` for the default) and the grid that shows, always.
fn scene(name: &str, style: Option<Style>, grid: Option<bool>) -> SceneDef {
    SceneDef {
        name: name.into(),
        eye: [3.0, -3.0, 2.0],
        target: [0.0; 3],
        up: [0.0, 0.0, 1.0],
        fov: 35.0,
        parallel: false,
        hidden_tags: vec![],
        context: None,
        style,
        shadows: None,
        sections: None,
        grid,
    }
}

fn grid_off() -> Style {
    Style {
        grid: false,
        ..Style::default()
    }
}

fn go(d: &mut Document, name: &str) {
    ok(d, Op::ActivateScene { name: name.into() });
}

/// The grid that shows: no style, or a style without the switch off, shows it.
fn grid(d: &Document) -> bool {
    d.model().style().is_none_or(|s| s.grid)
}

/// The issue's steps: grid off (View > Grid), save Scene 1; grid on, save Scene 2 (the
/// default style, stored as none); go to Scene 1, then Scene 2.
fn issue_6() -> Document {
    let mut d = Document::with_ground_sketch();
    ok(
        &mut d,
        Op::SetStyle {
            style: Some(grid_off()),
        },
    );
    let style = d.model().style().cloned();
    assert_eq!(style, Some(grid_off()), "grid off is a style of its own");
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Scene 1", style, Some(false)),
        },
    );
    ok(&mut d, Op::SetStyle { style: None });
    assert_eq!(d.model().style(), None, "grid on again: the default style");
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Scene 2", None, Some(true)),
        },
    );
    d
}

#[test]
fn two_scenes_with_opposite_grids_flip_it_both_ways() {
    let mut d = issue_6();
    let default_hash = d.hash();
    go(&mut d, "Scene 1");
    assert!(!grid(&d), "Scene 1 hides the grid");
    assert_eq!(d.model().style(), Some(&grid_off()));
    assert_ne!(d.hash(), default_hash, "a hidden grid is hashed");
    go(&mut d, "Scene 2");
    assert!(grid(&d), "Scene 2 shows the grid again (issue #6)");
    // Back to the default style, stored as none: the model hashes as a default document.
    assert_eq!(d.model().style(), None);
    assert_eq!(d.hash(), default_hash);
    // And from no style at all (a new document's), Scene 1 hides it again.
    go(&mut d, "Scene 1");
    assert!(!grid(&d));
    go(&mut d, "Scene 1");
    assert!(!grid(&d), "going to it twice changes nothing");
    go(&mut d, "Scene 2");
    go(&mut d, "Scene 2");
    assert!(grid(&d));
    assert_eq!(d.model().style(), None);
}

#[test]
fn going_to_a_scene_is_one_undo_step_for_the_grid() {
    let mut d = issue_6();
    // From no style: undo brings no style back.
    let before = (d.hash(), d.model().style().cloned());
    go(&mut d, "Scene 1");
    assert!(!grid(&d));
    d.undo().unwrap();
    assert_eq!((d.hash(), d.model().style().cloned()), before);
    assert!(grid(&d));
    d.redo().unwrap();
    assert!(!grid(&d));
    // From grid off: undo brings the grid-off style back.
    let before = (d.hash(), d.model().style().cloned());
    go(&mut d, "Scene 2");
    assert!(grid(&d));
    d.undo().unwrap();
    assert_eq!((d.hash(), d.model().style().cloned()), before);
    assert_eq!(d.model().style(), Some(&grid_off()));
}

#[test]
fn the_grid_goes_with_the_rest_of_the_scene_s_style() {
    let mut d = Document::with_ground_sketch();
    let xray_off = Style {
        face: FaceStyle::XRay,
        grid: false,
        ..Style::default()
    };
    // X-ray faces and the grid hidden: both come back.
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("X-ray", Some(xray_off.clone()), Some(false)),
        },
    );
    go(&mut d, "X-ray");
    assert_eq!(d.model().style(), Some(&xray_off));
    // A scene's grid is applied over its style: when they disagree (a plugin may write
    // them so; the app never does), the grid switch wins.
    let xray_on = Style {
        grid: true,
        ..xray_off.clone()
    };
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Disagree", Some(xray_on.clone()), Some(false)),
        },
    );
    ok(&mut d, Op::SetStyle { style: None });
    go(&mut d, "Disagree");
    assert_eq!(d.model().style(), Some(&xray_off));
    // A grid switch with no style keeps the current look and sets only the grid.
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Grid on", None, Some(true)),
        },
    );
    go(&mut d, "Grid on");
    assert_eq!(d.model().style(), Some(&xray_on));
    // A style without a switch (a lane-10 scene) brings its own grid back, as before.
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Lane 10", Some(xray_off.clone()), None),
        },
    );
    go(&mut d, "Lane 10");
    assert_eq!(d.model().style(), Some(&xray_off));
}

#[test]
fn a_scene_without_a_grid_switch_leaves_the_grid_alone() {
    // Every scene written before lane 26, and every scene from a SketchUp file (SketchUp
    // has no ground grid).
    let mut d = Document::with_ground_sketch();
    ok(
        &mut d,
        Op::AddScene {
            scene: scene("Old", None, None),
        },
    );
    ok(
        &mut d,
        Op::SetStyle {
            style: Some(grid_off()),
        },
    );
    go(&mut d, "Old");
    assert_eq!(d.model().style(), Some(&grid_off()), "left hidden");
    ok(&mut d, Op::SetStyle { style: None });
    go(&mut d, "Old");
    assert_eq!(d.model().style(), None, "left shown");
}

#[test]
fn old_documents_with_scenes_leave_the_grid_alone() {
    // The sheet golden's ops are a lane-12 document: three scenes saved with no style and
    // one with a style, none with a grid switch. Replayed and reopened from its snapshot,
    // going to each leaves a hidden grid hidden.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sheet.json");
    let g: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let ops: Vec<Op> = serde_json::from_value(g["ops"].clone()).unwrap();
    let d = Document::replay(&ops).unwrap();
    let names: Vec<String> = d.model().scenes().iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, ["Section A", "Section", "Front", "Plan"]);
    assert!(d.model().scenes().iter().all(|s| s.grid.is_none()));
    let json = serde_json::to_string(&d.model().snapshot()).unwrap();
    let snap: serde_json::Value = serde_json::from_str(&json).unwrap();
    for sc in snap["scenes"].as_array().unwrap() {
        assert!(
            sc.get("grid").is_none(),
            "an old scene writes no grid: {sc}"
        );
    }
    let mut m = Model::from_snapshot(&serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(m.hash(), d.model().hash());
    m.apply(&Op::SetStyle {
        style: Some(grid_off()),
    })
    .unwrap();
    for name in ["Front", "Plan", "Section A"] {
        m.apply(&Op::ActivateScene { name: name.into() }).unwrap();
        assert_eq!(m.style(), Some(&grid_off()), "{name}");
    }
    // "Section" brings its Shop drawing style back, and with it the grid that style shows.
    m.apply(&Op::ActivateScene {
        name: "Section".into(),
    })
    .unwrap();
    let s = m.style().unwrap();
    assert_eq!(s.preset.as_deref(), Some("Shop drawing"));
    assert!(s.grid);
}

#[test]
fn the_grid_switch_survives_the_snapshot() {
    let d = issue_6();
    let snap = serde_json::to_value(d.model().snapshot()).unwrap();
    let scenes = snap["scenes"].as_array().unwrap();
    assert_eq!(scenes[0]["grid"], serde_json::json!(false));
    assert_eq!(scenes[1]["grid"], serde_json::json!(true));
    assert!(scenes[1].get("style").is_none(), "the default style: none");
    let mut back = Model::from_snapshot(&serde_json::from_value(snap.clone()).unwrap()).unwrap();
    assert_eq!(back.hash(), d.model().hash());
    assert_eq!(back.scenes(), d.model().scenes());
    // Reopened, the scenes still flip the grid both ways.
    back.apply(&Op::ActivateScene {
        name: "Scene 1".into(),
    })
    .unwrap();
    assert_eq!(back.style(), Some(&grid_off()));
    back.apply(&Op::ActivateScene {
        name: "Scene 2".into(),
    })
    .unwrap();
    assert_eq!(back.style(), None);
    // Absent is accepted (older files) ...
    let mut old = snap.clone();
    for sc in old["scenes"].as_array_mut().unwrap() {
        sc.as_object_mut().unwrap().remove("grid");
    }
    let old = Model::from_snapshot(&serde_json::from_value(old).unwrap()).unwrap();
    assert!(old.scenes().iter().all(|s| s.grid.is_none()));
    assert_eq!(
        old.hash(),
        d.model().hash(),
        "the scene's switch is not hashed"
    );
    // ... and anything but true or false is not a snapshot.
    let mut bad = snap;
    bad["scenes"][0]["grid"] = serde_json::json!("off");
    assert!(serde_json::from_value::<sawhorse_kernel::snapshot::Snapshot>(bad).is_err());
}

#[test]
fn updating_a_scene_rewrites_its_grid() {
    let mut d = issue_6();
    // Scene 2 updated while the grid is hidden: it now hides it.
    ok(
        &mut d,
        Op::SetStyle {
            style: Some(grid_off()),
        },
    );
    let style = d.model().style().cloned();
    ok(
        &mut d,
        Op::UpdateScene {
            name: "Scene 2".into(),
            scene: scene("Scene 2", style, Some(false)),
        },
    );
    ok(&mut d, Op::SetStyle { style: None });
    go(&mut d, "Scene 2");
    assert!(!grid(&d));
    // An unknown scene is still refused, and changes nothing.
    let before = d.hash();
    let e: KernelError = d
        .apply(&Op::ActivateScene {
            name: "Nope".into(),
        })
        .unwrap_err();
    assert!(!e.is_bug(), "{e}");
    assert_eq!(d.hash(), before);
}

#[test]
fn a_scene_seen_on_a_sheet_does_not_change_the_document() {
    // `as_scene` (what a sheet's viewport draws) applies the grid to its own copy only.
    let d = issue_6();
    let m = d.model();
    let one = m.as_scene("Scene 1").unwrap();
    assert_eq!(one.style(), Some(&grid_off()));
    assert_eq!(m.style(), None, "the document is untouched");
    let two = one.as_scene("Scene 2").unwrap();
    assert_eq!(two.style(), None);
}
