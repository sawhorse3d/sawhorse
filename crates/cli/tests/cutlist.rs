//! The command prints exactly what the kernel computes for the file, and says why not.
use sawhorse_cli::{Command, cut_list_json, nest_json, open, parse, run};
use sawhorse_kernel::cutlist::{CutOptions, Rounding};
use sawhorse_kernel::document::Document;
use sawhorse_kernel::model::Op;
use sawhorse_kernel::nesting::NestOptions;
use sawhorse_kernel::stock::NestAlgorithm;
use sawhorse_shz::SaveOptions;

/// A 3/4 x 12 x 36 in board named "Shelf", saved as a .shz.
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

/// A model with nothing to cut, saved as a .shz.
fn empty() -> Vec<u8> {
    sawhorse_shz::write(
        Document::with_ground_sketch().model(),
        &SaveOptions {
            display_units: Some("mm".to_string()),
            thumbnail_png: None,
            generator: None,
            plugin_data: None,
        },
    )
    .unwrap()
}

/// Lane 30: a model with no lumber printed `"board_feet":-0.0` in its totals (a float sum
/// over nothing is -0.0 in Rust), and its nesting `-0.0` areas. Totals are formed from zero.
#[test]
fn nothing_to_cut_totals_zero_not_minus_zero() {
    let bytes = empty();
    let cut = cut_list_json(&bytes, &CutOptions::default(), false).unwrap();
    assert!(cut.contains(r#""board_feet":0.0"#), "{cut}");
    assert!(!cut.contains("-0.0"), "{cut}");
    let v: serde_json::Value = serde_json::from_str(&cut).unwrap();
    assert!(
        v["totals"]["board_feet"]
            .as_f64()
            .unwrap()
            .is_sign_positive()
    );
    let nest = nest_json(&bytes, &NestOptions::default(), false).unwrap();
    assert!(!nest.contains("-0.0"), "{nest}");
    let n: serde_json::Value = serde_json::from_str(&nest).unwrap();
    for k in ["parts_area", "stock_area"] {
        assert!(
            n["totals"][k].as_f64().unwrap().is_sign_positive(),
            "{k}: {nest}"
        );
    }
    // The pretty printer says the same.
    let pretty = cut_list_json(&bytes, &CutOptions::default(), true).unwrap();
    assert!(pretty.contains(r#""board_feet": 0.0"#), "{pretty}");
    // A model with lumber adds up as before: the total is its one row's board feet.
    let (_, shelf) = shelves();
    let v: serde_json::Value =
        serde_json::from_str(&cut_list_json(&shelf, &CutOptions::default(), false).unwrap())
            .unwrap();
    let row = v["rows"][0]["board_feet"].as_f64().unwrap();
    assert!(row > 0.0, "{v}");
    assert_eq!(v["totals"]["board_feet"].as_f64().unwrap(), row);
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
    assert!(printed.notes.is_empty());
    assert_eq!(
        printed.text,
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
    assert!(run(&c).unwrap().text.starts_with("{\n"));

    assert_eq!(parse(&args(&[])).unwrap(), Command::Help);
    assert_eq!(parse(&args(&["cutlist", "--help"])).unwrap(), Command::Help);
    assert!(run(&Command::Help).unwrap().text.starts_with("Usage:"));
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
    let c = parse(&args(&[
        "nest",
        &p,
        "--algorithm",
        "maxrects",
        "--visible-only",
    ]))
    .unwrap();
    assert_eq!(
        c,
        Command::Nest {
            path: p.clone(),
            options: NestOptions {
                visible_only: true,
                algorithm: Some(NestAlgorithm::Maxrects)
            },
            pretty: false
        }
    );
    let c = parse(&args(&["nest", &p, "--algorithm", "table-saw"])).unwrap();
    assert_eq!(
        c,
        Command::Nest {
            path: p.clone(),
            options: NestOptions {
                visible_only: false,
                algorithm: Some(NestAlgorithm::Guillotine)
            },
            pretty: false
        }
    );
    assert!(
        parse(&args(&["cutlist", &p, "--algorithm", "maxrects"]))
            .unwrap_err()
            .contains("nest only")
    );
    assert!(
        parse(&args(&["nest", &p, "--algorithm", "tetris"]))
            .unwrap_err()
            .contains("unknown algorithm")
    );
    assert!(
        parse(&args(&["nest", &p, "--algorithm"]))
            .unwrap_err()
            .contains("needs a value")
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_algorithm_reaches_the_nesting() {
    let (d, bytes) = shelves();
    for algorithm in [NestAlgorithm::Guillotine, NestAlgorithm::Maxrects] {
        let opts = NestOptions {
            visible_only: false,
            algorithm: Some(algorithm),
        };
        let printed = nest_json(&bytes, &opts, false).unwrap();
        assert_eq!(
            printed,
            serde_json::to_string(&d.model().nest(&opts)).unwrap()
        );
        let v: serde_json::Value = serde_json::from_str(&printed).unwrap();
        assert_eq!(v["algorithm"], serde_json::to_value(algorithm).unwrap());
    }
}

/// `bytes` with one more entry, `plugin-data.json`, that is not text: the reader has to
/// leave it out and says so.
fn with_binary_plugin_data(bytes: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut from = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut to = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..from.len() {
        to.raw_copy_file(from.by_index_raw(i).unwrap()).unwrap();
    }
    to.start_file("plugin-data.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    to.write_all(&[0xff, 0xfe, 0xfd]).unwrap();
    to.finish().unwrap().into_inner()
}

#[test]
fn what_the_reader_had_to_do_comes_back_as_notes() {
    let (d, clean) = shelves();
    assert!(
        open(&clean).unwrap().notes.is_empty(),
        "a clean file has no notes"
    );
    let bytes = with_binary_plugin_data(&clean);
    let opened = open(&bytes).unwrap();
    assert_eq!(
        opened.notes,
        ["plugin-data.json is not text; the plugins' data in it was left out"]
    );
    // The model itself is untouched, and the notes ride along with what the command prints.
    let path = std::env::temp_dir().join(format!("sawhorse-cli-notes-{}.shz", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let printed = run(&Command::CutList {
        path: path.to_string_lossy().to_string(),
        options: CutOptions::default(),
        pretty: false,
    })
    .unwrap();
    assert_eq!(printed.notes, opened.notes);
    assert_eq!(
        printed.text,
        serde_json::to_string(&d.model().cut_list(&CutOptions::default())).unwrap()
    );
    std::fs::remove_file(&path).ok();
}
