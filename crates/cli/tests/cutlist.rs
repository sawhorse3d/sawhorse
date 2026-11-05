//! The command prints exactly what the kernel computes for the file, and says why not.
use sawhorse_cli::{Command, cut_list_json, nest_json, parse, run};
use sawhorse_kernel::cutlist::{CutOptions, Rounding};
use sawhorse_kernel::document::Document;
use sawhorse_kernel::model::Op;
use sawhorse_kernel::nesting::NestOptions;
use sawhorse_shz::SaveOptions;

/// A 3/4 x 12 x 36 in board, as a component "Shelf" placed twice, saved as a .shz.
fn shelves() -> (Document, Vec<u8>) {
    let inch = 0.0254;
    let mut d = Document::with_ground_sketch();
    let ops = [
        format!(
            r#"{{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":{},"h":{}}}"#,
            36.0 * inch,
            12.0 * inch
        ),
        format!(
            r#"{{"op":"extrude","sketch":1,"region":1,"distance":{}}}"#,
            0.75 * inch
        ),
        r#"{"op":"rename_object","object":1,"name":"Shelf"}"#.to_string(),
    ];
    for op in ops {
        d.apply(&serde_json::from_str::<Op>(&op).unwrap()).unwrap();
    }
    let bytes = sawhorse_shz::write(
        d.model(),
        &SaveOptions {
            display_units: Some("frac-in".to_string()),
            thumbnail_png: None,
            generator: None,
            plugin_data: None,
        },
    )
    .unwrap();
    (d, bytes)
}

#[test]
fn the_cut_list_is_the_kernels_own_for_the_file() {
    let (d, bytes) = shelves();
    let opts = CutOptions::default();
    let printed = cut_list_json(&bytes, &opts, false).unwrap();
    let expected = serde_json::to_string(&d.model().cut_list(&opts)).unwrap();
    assert_eq!(printed, expected);
    let v: serde_json::Value = serde_json::from_str(&printed).unwrap();
    assert_eq!(v["rows"].as_array().unwrap().len(), 1);
    assert_eq!(v["rows"][0]["name"], "Shelf");
    assert_eq!(v["rows"][0]["qty"], 1);
    // Finished size in metres, thickness first: 3/4 x 12 x 36 in.
    let size = v["rows"][0]["size"].as_array().unwrap();
    for (got, want) in size.iter().zip([0.75, 12.0, 36.0]) {
        assert!(
            (got.as_f64().unwrap() / 0.0254 - want).abs() < 1e-9,
            "{size:?}"
        );
    }
    assert!(v["hash"].as_str().unwrap().len() >= 8);
}

#[test]
fn rounding_and_visible_only_reach_the_kernel() {
    let (d, bytes) = shelves();
    let opts = CutOptions {
        rounding: Rounding::HalfMm,
        visible_only: true,
    };
    let printed = cut_list_json(&bytes, &opts, true).unwrap();
    assert_eq!(
        printed,
        serde_json::to_string_pretty(&d.model().cut_list(&opts)).unwrap()
    );
    let v: serde_json::Value = serde_json::from_str(&printed).unwrap();
    assert_eq!(v["rounding"], "half_mm");
    assert_eq!(v["visible_only"], true);
}

#[test]
fn the_nesting_is_the_kernels_own_for_the_file() {
    let (d, bytes) = shelves();
    let opts = NestOptions::default();
    assert_eq!(
        nest_json(&bytes, &opts, false).unwrap(),
        serde_json::to_string(&d.model().nest(&opts)).unwrap()
    );
}

#[test]
fn a_file_that_is_not_a_document_says_so() {
    let e = cut_list_json(b"not a zip", &CutOptions::default(), false).unwrap_err();
    assert!(e.starts_with("could not open the file: "), "{e}");
}

#[test]
fn the_command_line_is_parsed_and_run_end_to_end() {
    let (d, bytes) = shelves();
    let path = std::env::temp_dir().join(format!("sawhorse-cli-{}.shz", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let p = path.to_string_lossy().to_string();
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    let c = parse(&args(&[
        "cutlist",
        &p,
        "--rounding",
        "thirty-second",
        "--visible-only",
    ]))
    .unwrap();
    assert_eq!(
        c,
        Command::CutList {
            path: p.clone(),
            options: CutOptions {
                rounding: Rounding::ThirtySecond,
                visible_only: true
            },
            pretty: false
        }
    );
    let printed = run(&c).unwrap();
    assert_eq!(
        printed,
        serde_json::to_string(&d.model().cut_list(&CutOptions {
            rounding: Rounding::ThirtySecond,
            visible_only: true
        }))
        .unwrap()
    );

    let c = parse(&args(&["nest", &p, "--pretty"])).unwrap();
    assert_eq!(
        c,
        Command::Nest {
            path: p.clone(),
            options: NestOptions::default(),
            pretty: true
        }
    );
    assert!(run(&c).unwrap().starts_with("{\n"));

    assert_eq!(parse(&args(&[])).unwrap(), Command::Help);
    assert!(run(&Command::Help).unwrap().starts_with("Usage:"));
    assert!(
        parse(&args(&["cutlist"]))
            .unwrap_err()
            .contains("needs a file")
    );
    assert!(
        parse(&args(&["nest", &p, "--rounding", "half-mm"]))
            .unwrap_err()
            .contains("cutlist only")
    );
    assert!(
        parse(&args(&["cutlist", &p, "--rounding", "coarse"]))
            .unwrap_err()
            .contains("unknown rounding")
    );
    assert!(
        parse(&args(&["saw", &p]))
            .unwrap_err()
            .contains("unknown command")
    );
    assert!(
        parse(&args(&["cutlist", &p, "--loud"]))
            .unwrap_err()
            .contains("unknown option")
    );
    let missing = run(&Command::CutList {
        path: "/nowhere/none.shz".to_string(),
        options: CutOptions::default(),
        pretty: false,
    })
    .unwrap_err();
    assert!(
        missing.starts_with("could not read /nowhere/none.shz"),
        "{missing}"
    );
    std::fs::remove_file(&path).ok();
}
