//! The server over its own protocol (issue #14): what an agent sends on stdin and gets back
//! on stdout, the tools, and the holds from the issue: files only under the start folder,
//! saved through the kernel's writer and read back, untrusted input answered with an
//! error and never a crash or a hang, and parts by name with shared names reported.
mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{
    Session, answers, board, folder, greeting, hash_of, names, near, run_binary, temporary_files,
    tool_call,
};
use sawhorse_mcp::{MAX_MESSAGE, PROTOCOL_VERSION, Server, serve};
use serde_json::{Value, json};

#[test]
fn initialize_names_the_protocol_and_lists_the_tools() {
    let root = folder("init");
    let mut server = Server::new(&root).unwrap();
    let r = server
        .handle_line(br#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#)
        .unwrap();
    assert_eq!(r["id"], "a");
    assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(r["result"]["serverInfo"]["name"], "sawhorse-mcp");
    assert!(r["result"]["capabilities"]["tools"].is_object(), "{r}");
    // A version it does not speak gets its own back, as the protocol says.
    let r = server
        .handle_line(br#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#)
        .unwrap();
    assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
    let mut s = Session::new(&root);
    let r = s.request("tools/list", json!({}));
    let names: Vec<&str> = r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "new_model",
            "open_model",
            "save_model",
            "summary",
            "add_board",
            "apply_ops",
            "cut_list",
            "nest",
            "check_clearance",
            "measure",
            "undo",
            "redo"
        ]
    );
    for t in r["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].as_str().unwrap().len() > 20, "{t}");
    }
    let r = s.request("ping", json!({}));
    assert_eq!(r["result"], json!({}));
}

/// The bookshelf from the user guide, built with add_board in millimetres: two sides, a
/// top, a bottom and two shelves in 19 mm pine, saved, and the command-line tool's cut list
/// of the saved file is the server's own.
#[test]
fn a_bookshelf_built_with_add_board_saves_the_cut_list_it_shows() {
    let root = folder("bookshelf");
    let mut s = Session::new(&root);
    s.ok("new_model", json!({"units": "mm"}));
    let board = |name: &str, size: [f64; 3], at: [f64; 3], length: &str, thickness: &str| {
        json!({"name": name, "thickness": size[0], "width": size[1], "length": size[2],
               "position": at, "length_axis": length, "thickness_axis": thickness, "material": "Pine"})
    };
    // Sides stand up: length along z, thickness along x.
    s.ok(
        "add_board",
        board("Side", [19.0, 300.0, 900.0], [0.0, 0.0, 0.0], "z", "x"),
    );
    s.ok(
        "add_board",
        board("Side", [19.0, 300.0, 900.0], [781.0, 0.0, 0.0], "z", "x"),
    );
    // Top, bottom and shelves lie flat between them: length along x, thickness along z.
    for (name, z) in [
        ("Bottom", 0.0),
        ("Shelf", 300.0),
        ("Shelf", 600.0),
        ("Top", 881.0),
    ] {
        s.ok(
            "add_board",
            board(name, [19.0, 300.0, 762.0], [19.0, 0.0, z], "x", "z"),
        );
    }
    let shown = s.ok("cut_list", json!({}));
    let saved = s.ok("save_model", json!({"path": "Bookshelf.shz"}));
    assert_eq!(saved["path"], "Bookshelf.shz");
    let bytes = std::fs::read(root.join("Bookshelf.shz")).unwrap();
    let cli = sawhorse_cli::cut_list_json(&bytes, &Default::default(), false).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&cli).unwrap(), shown);
    // Six pieces: each plain solid is its own row, so the two sides and two shelves share
    // their names, and the summary says so rather than picking one.
    let rows = shown["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let sum = s.ok("summary", json!({}));
    assert_eq!(sum["path"], "Bookshelf.shz");
    assert_eq!(sum["unsaved"], false);
    assert_eq!(sum["parts"].as_array().unwrap().len(), 6);
    let shared: Vec<&str> = sum["shared_names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    assert_eq!(shared, ["Shelf", "Side"]);
    for p in sum["parts"].as_array().unwrap() {
        assert_eq!(p["material"], "Pine", "{p}");
        let mm = p["size_mm"].as_array().unwrap();
        let mm: Vec<f64> = mm.iter().map(|x| x.as_f64().unwrap()).collect();
        let want = if p["name"] == "Side" {
            [19.0, 300.0, 900.0]
        } else {
            [19.0, 300.0, 762.0]
        };
        for k in 0..3 {
            assert!((mm[k] - want[k]).abs() < 1e-6, "{p}");
        }
    }
    // The file opens again as the same model.
    let hash = s.hash();
    s.ok("new_model", json!({}));
    assert_ne!(s.hash(), hash);
    let opened = s.ok("open_model", json!({"path": "Bookshelf.shz"}));
    assert_eq!(opened["model_hash"], hash);
}

/// A board sits where it is asked: its box runs from `position` along each axis, whichever
/// way it is turned, and inches work as millimetres do.
#[test]
fn a_board_lands_where_it_is_asked_on_every_axis() {
    let root = folder("axes");
    let mut s = Session::new(&root);
    let axes = ["x", "y", "z"];
    let mut n = 0;
    for length in axes {
        for thickness in axes {
            if length == thickness {
                continue;
            }
            n += 1;
            let at = [100.0 * n as f64, -50.0, 25.0];
            let out = s.ok(
                "add_board",
                json!({"name": format!("B{n}"), "thickness": 20.0, "width": 90.0, "length": 400.0,
                       "position": at, "length_axis": length, "thickness_axis": thickness}),
            );
            let lo = out["board"]["min_mm"].as_array().unwrap();
            let hi = out["board"]["max_mm"].as_array().unwrap();
            let width = axes
                .iter()
                .find(|a| **a != length && **a != thickness)
                .unwrap();
            for (k, a) in axes.iter().enumerate() {
                let size = if *a == length {
                    400.0
                } else if *a == thickness {
                    20.0
                } else {
                    assert_eq!(a, width);
                    90.0
                };
                let (l, h) = (lo[k].as_f64().unwrap(), hi[k].as_f64().unwrap());
                assert!(
                    near(l, at[k]) && near(h, at[k] + size),
                    "{length} {thickness}: {out}"
                );
            }
        }
    }
    // A sketch already on the plane facing down (an extrude goes the way its sketch faces):
    // the board still sits above the plane, where it was asked.
    s.ok(
        "apply_ops",
        json!({"name": "Sketch facing down", "ops": [{"op": "new_sketch", "origin": [0.0, 0.0, 0.5], "normal": [0.0, 0.0, -1.0]}]}),
    );
    let out = s.ok(
        "add_board",
        json!({"name": "Over", "thickness": 19.0, "width": 100.0, "length": 300.0, "position": [0.0, 2000.0, 500.0]}),
    );
    let (lo, hi) = (&out["board"]["min_mm"], &out["board"]["max_mm"]);
    assert!(
        near(lo[2].as_f64().unwrap(), 500.0) && near(hi[2].as_f64().unwrap(), 519.0),
        "{out}"
    );
    // Inches: a 3/4 x 12 x 36 in board.
    let out = s.ok(
        "add_board",
        json!({"name": "Inch", "thickness": 0.75, "width": 12.0, "length": 36.0, "units": "in"}),
    );
    let hi = out["board"]["max_mm"].as_array().unwrap();
    // Answers are to 0.1 mm: 36 in is 914.4 mm, and 3/4 in (19.05 mm) reads 19.0. The
    // cut list keeps the exact sizes.
    assert_eq!(hi[0].as_f64().unwrap(), 914.4, "{out}");
    assert_eq!(hi[2].as_f64().unwrap(), 19.0, "{out}");
    let rows = s.ok("cut_list", json!({}))["rows"].clone();
    let inch = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Inch")
        .unwrap();
    assert!(
        near(inch["size"][0].as_f64().unwrap(), 0.75 * 0.0254),
        "{inch}"
    );
}

/// Every change is one undo step and says what it changed in the cut list.
#[test]
fn each_change_is_one_undo_step_and_reports_the_cut_list_rows_it_moved() {
    let root = folder("undo");
    let mut s = Session::new(&root);
    let empty = s.hash();
    let out = s.ok(
        "add_board",
        json!({"name": "Top", "thickness": 19.0, "width": 400.0, "length": 800.0, "material": "Walnut"}),
    );
    let added = out["changes"]["added"].as_array().unwrap();
    assert_eq!(added.len(), 1, "{out}");
    assert_eq!(added[0]["name"], "Top");
    assert!(out["changes"]["removed"].as_array().unwrap().is_empty());
    let object = out["board"]["object"].as_u64().unwrap();
    let one = s.hash();
    // A raw op through apply_ops: a resize is a changed row, not a new one.
    let out = s.ok(
        "apply_ops",
        json!({"name": "Rename", "ops": [{"op": "rename_object", "object": object, "name": "Lid"}]}),
    );
    let changed = out["changes"]["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{out}");
    assert_eq!(changed[0]["before"]["name"], "Top");
    assert_eq!(changed[0]["after"]["name"], "Lid");
    let out = s.ok("undo", json!({}));
    assert_eq!(
        out["changes"]["changed"][0]["after"]["name"], "Top",
        "{out}"
    );
    assert_eq!(s.hash(), one);
    let out = s.ok("undo", json!({}));
    assert_eq!(out["changes"]["removed"][0]["name"], "Top", "{out}");
    assert_eq!(s.hash(), empty);
    let e = s.refused("undo", json!({}));
    assert!(e.contains("undo"), "{e}");
    s.ok("redo", json!({}));
    assert_eq!(s.hash(), one);
    // The library adds Walnut to the model with the board; one undo takes both away.
    let sum = s.ok("summary", json!({}));
    assert_eq!(sum["parts"][0]["material"], "Walnut");
}

/// #116: copy and array of a component instance by `nodes` alone, as an agent first wrote
/// them, and a known op with a field missing says which field.
#[test]
fn copy_and_array_take_nodes_alone() {
    let mut s = Session::new(&folder("nodes-alone"));
    let r = s.ok(
        "add_board",
        json!({"name": "Slat", "thickness": 19.0, "width": 70.0, "length": 900.0}),
    );
    let slat = r["board"]["object"].clone();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Component", "ops": [{"op": "make_component", "items": [{"object": slat}], "name": "Slat"}]}),
    );
    let node = r["outcome"]["outcomes"][0]["instance"].clone();
    let step = json!({"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [0, 0.1, 0]});
    s.ok(
        "apply_ops",
        json!({"name": "Copy", "ops": [{"op": "copy", "nodes": [node], "xform": step}]}),
    );
    s.ok(
        "apply_ops",
        json!({"name": "Array", "ops": [{"op": "array", "nodes": [node], "xform": step, "count": 3}]}),
    );
    let parts = s.ok("summary", json!({}))["parts"].clone();
    assert_eq!(parts.as_array().unwrap().len(), 1, "{parts}");
    assert_eq!(parts[0]["qty"], 5, "{parts}");
    let e = s.refused(
        "apply_ops",
        json!({"name": "Array", "ops": [{"op": "array", "nodes": [node], "xform": step}]}),
    );
    assert!(e.contains("op 0 (array): missing field `count`"), "{e}");
}

/// #116: `component: true` wraps the board in a component named after it, in the same one
/// undo step, and answers with the instance and the definition. The board is where and what
/// it would be without it, and summary names the definition on its row.
#[test]
fn add_board_with_component_wraps_the_board_in_a_component_named_after_it() {
    let args = json!({"name": "Slat", "thickness": 19.0, "width": 70.0, "length": 900.0,
        "position": [10.0, 20.0, 100.0], "material": "Pine"});
    let mut plain = Session::new(&folder("component-plain"));
    let loose = plain.ok("add_board", args.clone())["board"].clone();

    let mut s = Session::new(&folder("component-true"));
    let empty = s.hash();
    let mut with = args.clone();
    with["component"] = json!(true);
    let out = s.ok("add_board", with);
    let board = &out["board"];
    assert!(board["object"].is_u64(), "{out}");
    let instance = board["instance"].as_u64().expect("an instance id");
    let definition = board["definition"].as_u64().expect("a definition id");
    assert_eq!(board["name"], "Slat");
    assert_eq!(board["material"], "Pine");
    assert_eq!(board["min_mm"], loose["min_mm"], "{out}");
    assert_eq!(board["max_mm"], loose["max_mm"], "{out}");
    // One row added: the component, named after the board, selected by its instance.
    let added = out["changes"]["added"].as_array().unwrap();
    assert_eq!(added.len(), 1, "{out}");
    assert_eq!(added[0]["name"], "Slat");
    assert_eq!(added[0]["material"], "Pine");
    assert_eq!(added[0]["items"], json!([{"node": instance}]), "{out}");
    assert_eq!(added[0]["definition"], definition, "{out}");
    assert_eq!(out["shares_name_with"], json!([]), "{out}");
    let parts = s.ok("summary", json!({}))["parts"].clone();
    assert_eq!(parts.as_array().unwrap().len(), 1, "{parts}");
    assert_eq!(parts[0]["qty"], 1);
    assert_eq!(parts[0]["definition"], definition, "{parts}");
    assert_eq!(parts[0]["size_mm"], json!([19.0, 70.0, 900.0]), "{parts}");
    // The board, the component and the library's Pine are one undo step.
    s.ok("undo", json!({}));
    assert_eq!(s.hash(), empty);
    // A name at the limit names the component too; one past it is refused, nothing made.
    let long = "L".repeat(64);
    let out = s.ok(
        "add_board",
        json!({"name": long, "thickness": 19.0, "width": 70.0, "length": 900.0, "component": true}),
    );
    assert_eq!(out["changes"]["added"][0]["name"], long, "{out}");
    let before = s.hash();
    let e = s.refused(
        "add_board",
        json!({"name": "L".repeat(65), "thickness": 19.0, "width": 70.0, "length": 900.0, "component": true}),
    );
    assert!(e.contains("1 to 64 characters"), "{e}");
    assert_eq!(s.hash(), before);
}

/// #116: without `component`, or with it false, add_board makes the loose solid it always
/// made: the same model, and the same answer, with no instance or definition in it. A loose
/// board's summary row names no definition.
#[test]
fn add_board_without_component_makes_what_it_made() {
    let args = json!({"name": "Rail", "thickness": 19.0, "width": 50.0, "length": 600.0});
    let mut a = Session::new(&folder("component-absent"));
    let left_out = a.ok("add_board", args.clone());
    let mut b = Session::new(&folder("component-false"));
    let mut off = args.clone();
    off["component"] = json!(false);
    let set_false = b.ok("add_board", off);
    assert_eq!(left_out, set_false);
    assert_eq!(a.hash(), b.hash());
    let board = left_out["board"].as_object().unwrap();
    let mut keys: Vec<&str> = board.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["material", "max_mm", "min_mm", "name", "object"],
        "{left_out}"
    );
    let parts = a.ok("summary", json!({}))["parts"].clone();
    assert_eq!(parts[0]["items"], json!([{"object": board["object"]}]));
    assert!(parts[0].get("definition").is_none(), "{parts}");
    let e = a.refused(
        "add_board",
        json!({"name": "Rail", "thickness": 19.0, "width": 50.0, "length": 600.0, "component": "yes"}),
    );
    assert!(e.contains("arguments"), "{e}");
}

/// #116: copies and an array of a component board are instances of its definition, so the
/// cut list reads one row with a quantity; a second component board of the same name is a
/// definition of its own, reported under shares_name_with.
#[test]
fn copies_of_a_component_board_are_instances_of_its_definition() {
    let mut s = Session::new(&folder("component-copies"));
    let out = s.ok(
        "add_board",
        json!({"name": "Slat", "thickness": 19.0, "width": 70.0, "length": 900.0, "component": true}),
    );
    let instance = out["board"]["instance"].clone();
    let definition = out["board"]["definition"].clone();
    let step = json!({"linear": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [0, 0.1, 0]});
    let r = s.ok(
        "apply_ops",
        json!({"name": "Slats", "ops": [{"op": "array", "nodes": [instance], "xform": step, "count": 11}]}),
    );
    // The row changed: its quantity, not a new row per copy.
    assert!(r["changes"]["added"].as_array().unwrap().is_empty(), "{r}");
    assert_eq!(r["changes"]["changed"][0]["after"]["qty"], 12, "{r}");
    let parts = s.ok("summary", json!({}))["parts"].clone();
    assert_eq!(parts.as_array().unwrap().len(), 1, "{parts}");
    assert_eq!(parts[0]["qty"], 12);
    assert_eq!(parts[0]["definition"], definition);
    let items = parts[0]["items"].as_array().unwrap();
    assert_eq!(items.len(), 12);
    let nodes: std::collections::BTreeSet<u64> =
        items.iter().map(|i| i["node"].as_u64().unwrap()).collect();
    assert_eq!(nodes.len(), 12, "{parts}");
    // A second component board named Slat: its own definition, and the first named beside it.
    let out = s.ok(
        "add_board",
        json!({"name": "Slat", "thickness": 19.0, "width": 70.0, "length": 450.0, "position": [0, 0, 500], "component": true}),
    );
    assert_ne!(out["board"]["definition"], definition, "{out}");
    // Its ids differ now (13 nodes, 2 definitions): each is the one on its own row.
    let added = &out["changes"]["added"][0];
    assert_eq!(out["board"]["instance"], added["items"][0]["node"], "{out}");
    assert_eq!(out["board"]["definition"], added["definition"], "{out}");
    assert_ne!(
        out["board"]["instance"], out["board"]["definition"],
        "{out}"
    );
    let others = out["shares_name_with"].as_array().unwrap();
    assert_eq!(others.len(), 1, "{out}");
    assert_eq!(others[0]["qty"], 12, "{out}");
    assert_eq!(others[0]["definition"], definition, "{out}");
}

/// #116: only a component's own row names a definition. A group's row and the rows of the
/// parts inside an assembly component (a component holding a group) carry no `definition`,
/// as before.
#[test]
fn only_a_component_s_own_row_names_its_definition() {
    let mut s = Session::new(&folder("component-rows"));
    let a = s.ok("add_board", board("Side", 0.0))["board"]["object"].clone();
    let b = s.ok("add_board", board("Shelf", 100.0))["board"]["object"].clone();
    let c = s.ok("add_board", board("Back", 200.0))["board"]["object"].clone();
    let d = s.ok("add_board", board("Top", 300.0))["board"]["object"].clone();
    // A group of one board: a group's row.
    let r = s.ok(
        "apply_ops",
        json!({"name": "Group", "ops": [{"op": "make_group", "items": [{"object": a}], "name": "Carcass side"}]}),
    );
    assert!(r["changes"]["added"][0].get("definition").is_none(), "{r}");
    // A component holding a group and a board: an assembly, its parts member rows.
    let r = s.ok(
        "apply_ops",
        json!({"name": "Assembly", "ops": [
            {"op": "make_group", "items": [{"object": b}], "name": "Shelf group"},
            {"op": "make_component", "items": [{"node": {"$ref": 0, "path": "group"}}, {"object": c}], "name": "Unit"},
        ]}),
    );
    let added = r["changes"]["added"].as_array().unwrap();
    assert_eq!(added.len(), 2, "{r}");
    for row in added {
        assert!(row.get("definition").is_none(), "{r}");
    }
    let parts = s.ok("summary", json!({}))["parts"].clone();
    let named: Vec<&Value> = parts
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p.get("definition").is_some())
        .collect();
    assert!(named.is_empty(), "{parts}");
    // A component of one board does: the only row that names one.
    let r = s.ok(
        "apply_ops",
        json!({"name": "Component", "ops": [{"op": "make_component", "items": [{"object": d}], "name": "Top"}]}),
    );
    let def = r["outcome"]["outcomes"][0]["definition"].clone();
    assert_eq!(r["changes"]["added"][0]["definition"], def, "{r}");
}

/// #116: the tool list and the instructions point an agent at components: add_board takes
/// `component`, and apply_ops names `nodes` beside `objects` for copy and array.
#[test]
fn the_tools_say_how_to_make_and_copy_components() {
    let mut s = Session::new(&folder("component-docs"));
    let r = s.request("tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap().clone();
    let add = tool("add_board");
    assert_eq!(
        add["inputSchema"]["properties"]["component"]["type"], "boolean",
        "{add}"
    );
    assert!(
        add["description"].as_str().unwrap().contains("component"),
        "{add}"
    );
    let ops = tool("apply_ops")["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(ops.contains("nodes"), "{ops}");
    let r = s.request(
        "initialize",
        json!({"protocolVersion": sawhorse_mcp::PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    let text = r["result"]["instructions"].as_str().unwrap();
    assert!(text.contains("component"), "{text}");
}

/// #117: `move_into` through apply_ops moves a top-level board into a group; with no way to
/// open the group in a batch, taking it out again is refused.
#[test]
fn move_into_puts_a_board_in_a_group_and_cannot_take_it_out() {
    let mut s = Session::new(&folder("move-into"));
    let a = s.ok(
        "add_board",
        json!({"name": "Side", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    )["board"]["object"]
        .clone();
    let b = s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0, "position": [0, 0, 100]}),
    )["board"]["object"]
        .clone();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Group", "ops": [{"op": "make_group", "items": [{"object": a}], "name": "Carcass"}]}),
    );
    let g = r["outcome"]["outcomes"][0]["group"].clone();
    let r = s.ok(
        "apply_ops",
        json!({"name": "Move", "ops": [{"op": "move_into", "items": [{"object": b}], "into": g}]}),
    );
    assert_eq!(r["outcome"]["outcomes"][0]["kind"], "moved_into", "{r}");
    let e = s.refused(
        "apply_ops",
        json!({"name": "Out", "ops": [{"op": "move_into", "items": [{"object": b}], "into": null}]}),
    );
    assert!(e.contains("not open"), "{e}");
    let e = s.refused(
        "apply_ops",
        json!({"name": "Open", "ops": [{"op": "set_context", "node": g}]}),
    );
    assert!(e.contains("cannot be part of a batch"), "{e}");
}

/// #117: a batch works in the context the model has open, and a `.shz` keeps the group that
/// was open when it was saved. Opened from such a file, `move_into` with `into: null` takes a
/// board out of that group: "cannot take one out" holds only while the model is open.
#[test]
fn move_into_in_a_file_saved_with_a_group_open_takes_a_board_out() {
    use sawhorse_kernel::Document;
    use sawhorse_kernel::model::{DrawTarget, Op, OpOutcome};
    use sawhorse_kernel::nodes::Item;
    let root = folder("move-into-open");
    let mut d = Document::with_ground_sketch();
    let OpOutcome::Drawn {
        sketch: Some(sketch),
        inside: Some(region),
        ..
    } = d
        .apply(&Op::Draw {
            target: DrawTarget::Plane {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            },
            points: vec![
                [0.0, 0.0, 0.0],
                [0.3, 0.0, 0.0],
                [0.3, 0.6, 0.0],
                [0.0, 0.6, 0.0],
            ],
            closed: true,
            curve: None,
        })
        .unwrap()
    else {
        panic!("not drawn")
    };
    let OpOutcome::ObjectCreated { object } = d
        .apply(&Op::Extrude {
            sketch,
            region,
            distance: 0.019,
        })
        .unwrap()
    else {
        panic!("not extruded")
    };
    let OpOutcome::GroupCreated { group } = d
        .apply(&Op::MakeGroup {
            items: vec![Item::Object(object)],
            name: Some("Carcass".into()),
        })
        .unwrap()
    else {
        panic!("not grouped")
    };
    d.apply(&Op::SetContext { node: Some(group) }).unwrap();
    let bytes = sawhorse_shz::write(d.model(), &Default::default()).unwrap();
    std::fs::write(root.join("open.shz"), bytes).unwrap();
    let mut s = Session::new(&root);
    s.ok("open_model", json!({"path": "open.shz"}));
    let r = s.ok(
        "apply_ops",
        json!({"name": "Out", "ops": [{"op": "move_into", "items": [{"object": object}], "into": null}]}),
    );
    assert_eq!(r["outcome"]["outcomes"][0]["kind"], "moved_into", "{r}");
    assert_eq!(r["outcome"]["outcomes"][0]["into"], Value::Null, "{r}");
}

/// A failing apply_ops changes nothing: not the model, not the undo history.
#[test]
fn a_failing_apply_ops_changes_nothing() {
    let root = folder("atomic");
    let mut s = Session::new(&root);
    s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    let before = s.hash();
    let e = s.refused(
        "apply_ops",
        json!({"name": "Half done", "ops": [
            {"op": "add_rectangle", "sketch": 1, "x": 2.0, "y": 2.0, "w": 0.5, "h": 0.5},
            {"op": "extrude", "sketch": 1, "region": {"$ref": 0, "path": "regions.0"}, "distance": 0.1},
            {"op": "rename_object", "object": 999, "name": "Nobody"}
        ]}),
    );
    assert!(e.contains("op 2"), "{e}");
    assert_eq!(s.hash(), before);
    // The one undo step there was is still the board.
    s.ok("undo", json!({}));
    assert!(
        s.ok("summary", json!({}))["parts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Undo, batches and restores are not ops an agent sends.
    let e = s.refused(
        "apply_ops",
        json!({"name": "Sneaky", "ops": [{"op": "undo"}]}),
    );
    assert!(e.contains("cannot be part of a batch"), "{e}");
    // Neither is a bad board.
    for bad in [
        json!({"name": "", "thickness": 19.0, "width": 300.0, "length": 600.0}),
        json!({"name": "A", "thickness": 0.0, "width": 300.0, "length": 600.0}),
        json!({"name": "A", "thickness": -19.0, "width": 300.0, "length": 600.0}),
        json!({"name": "A", "thickness": 19.0, "width": 300.0, "length": 1e30}),
        json!({"name": "A", "thickness": 19.0, "width": 300.0, "length": 600.0, "length_axis": "z", "thickness_axis": "z"}),
        json!({"name": "A", "thickness": 19.0, "width": 300.0, "length": 600.0, "length_axis": "w"}),
        json!({"name": "A", "thickness": 19.0, "width": 300.0, "length": 600.0, "units": "furlong"}),
        json!({"name": "A", "thickness": 19.0, "width": 300.0, "length": 600.0, "material": "Unobtainium"}),
        json!({"name": "A", "thickness": 19.0, "width": 300.0, "length": 600.0, "position": [0, 0]}),
    ] {
        s.refused("add_board", bad);
    }
    assert!(
        s.ok("summary", json!({}))["parts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// A board given a name already in the model is added, and the answer says the name is
/// shared rather than treating the two as one.
#[test]
fn a_shared_name_is_reported_not_picked() {
    let root = folder("names");
    let mut s = Session::new(&root);
    let board = json!({"name": "Rail", "thickness": 19.0, "width": 70.0, "length": 500.0});
    let out = s.ok("add_board", board.clone());
    assert!(
        out["shares_name_with"].as_array().unwrap().is_empty(),
        "{out}"
    );
    let mut second = board.clone();
    second["position"] = json!([0, 200, 0]);
    second["length"] = json!(450.0);
    let out = s.ok("add_board", second);
    let shares = out["shares_name_with"].as_array().unwrap();
    assert_eq!(shares.len(), 1, "{out}");
    let sum = s.ok("summary", json!({}));
    assert_eq!(sum["shared_names"][0]["name"], "Rail");
    assert_eq!(sum["shared_names"][0]["parts"], 2);
}

#[test]
fn files_stay_inside_the_start_folder() {
    let base = folder("paths");
    let root = base.join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let outside = base.join("outside.shz");
    let mut s = Session::new(&root);
    s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    s.ok("save_model", json!({"path": "sub/a.shz"}));
    std::fs::copy(root.join("sub/a.shz"), &outside).unwrap();
    for (tool, path) in [
        ("save_model", "../outside.shz"),
        ("save_model", "sub/../../outside.shz"),
        ("open_model", "../outside.shz"),
        ("open_model", "sub/../a.shz"),
        ("save_model", outside.to_str().unwrap()),
        ("open_model", outside.to_str().unwrap()),
        ("save_model", "notes.txt"),
        ("save_model", "missing/a.shz"),
        ("open_model", "nothing.shz"),
        ("save_model", ""),
    ] {
        let e = s.refused(tool, json!({"path": path}));
        assert!(!e.is_empty(), "{tool} {path}");
    }
    assert!(!root.join("notes.txt").exists());
    // Nothing was written outside, and the copy there is untouched.
    assert_eq!(
        std::fs::read(&outside).unwrap(),
        std::fs::read(root.join("sub/a.shz")).unwrap()
    );
    // save_model with no path saves where the model came from.
    s.ok("open_model", json!({"path": "sub/a.shz"}));
    s.ok(
        "add_board",
        json!({"name": "Back", "thickness": 6.0, "width": 600.0, "length": 900.0}),
    );
    assert_eq!(s.ok("summary", json!({}))["unsaved"], true);
    let saved = s.ok("save_model", json!({}));
    assert_eq!(saved["path"], "sub/a.shz");
    assert_eq!(saved["model_hash"], s.hash());
    // A new model has nowhere to save until it is given a path.
    s.ok("new_model", json!({}));
    let e = s.refused("save_model", json!({}));
    assert!(e.contains("path"), "{e}");
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_folder_is_refused() {
    let base = folder("links");
    let root = base.join("root");
    let away = base.join("away");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&away).unwrap();
    let mut s = Session::new(&root);
    s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    s.ok("save_model", json!({"path": "in.shz"}));
    std::fs::copy(root.join("in.shz"), away.join("far.shz")).unwrap();
    std::os::unix::fs::symlink(&away, root.join("door")).unwrap();
    std::os::unix::fs::symlink(away.join("far.shz"), root.join("far.shz")).unwrap();
    for (tool, path) in [
        ("open_model", "door/far.shz"),
        ("save_model", "door/new.shz"),
        ("open_model", "far.shz"),
        ("save_model", "far.shz"),
    ] {
        s.refused(tool, json!({"path": path}));
    }
    assert!(!away.join("new.shz").exists());
}

/// Untrusted input gets a JSON-RPC error and the server goes on answering.
#[test]
fn bad_input_gets_an_error_never_a_crash() {
    let root = folder("input");
    let mut s = Session::new(&root);
    let code = |r: &Value| r["error"]["code"].as_i64().unwrap();
    let r = s.line("{not json").unwrap();
    assert_eq!(code(&r), -32700);
    assert_eq!(r["id"], Value::Null);
    let r = s.line("[1, 2, 3]").unwrap();
    assert_eq!(code(&r), -32600);
    let r = s.line(r#"{"jsonrpc":"2.0","id":7}"#).unwrap();
    assert_eq!(code(&r), -32600);
    let r = s
        .line(r#"{"jsonrpc":"1.0","id":7,"method":"ping"}"#)
        .unwrap();
    assert_eq!(code(&r), -32600);
    let r = s.line("\u{0}\u{ff}").unwrap();
    assert_eq!(code(&r), -32700);
    let r = s.request("no/such/method", json!({}));
    assert_eq!(code(&r), -32601);
    let r = s.request(
        "tools/call",
        json!({"name": "no_such_tool", "arguments": {}}),
    );
    assert_eq!(code(&r), -32602);
    let r = s.request("tools/call", json!({"arguments": {}}));
    assert_eq!(code(&r), -32602);
    // Deeply nested: a hundred thousand brackets.
    let deep = format!(
        r#"{{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{{"name":"apply_ops","arguments":{{"name":"x","ops":{}{}}}}}}}"#,
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    let r = s.line(&deep).unwrap();
    assert!(r.get("error").is_some(), "{r}");
    // Arguments of the wrong shape are a tool error, not a crash.
    let (error, _) = s.call("add_board", json!({"name": 5}));
    assert!(error);
    let (error, _) = s.call("apply_ops", json!({"name": "x", "ops": "nope"}));
    assert!(error);
    // A notification gets no answer, even a bad one.
    assert!(
        s.line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#)
            .is_none()
    );
    // Still answering.
    let r = s.request("ping", json!({}));
    assert_eq!(r["result"], json!({}));
}

/// A message longer than the limit is answered with an error without holding it all, and
/// the next message is read as usual.
#[test]
fn a_message_over_the_limit_is_refused_and_the_next_one_answered() {
    let root = folder("huge");
    let mut input = Vec::new();
    input.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"pad":""#);
    input.resize(input.len() + MAX_MESSAGE + 10, b'a');
    input.extend_from_slice(b"\"}}\n");
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n");
    // A last message with no newline before the end is read too.
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}");
    let mut out = Vec::new();
    let mut server = Server::new(&root).unwrap();
    serve(&mut server, &input[..], &mut out).unwrap();
    let lines: Vec<Value> = out
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0]["error"]["code"], -32600);
    assert_eq!(lines[0]["id"], Value::Null);
    assert_eq!(lines[1]["id"], 2);
    assert_eq!(lines[2]["id"], 3);
}

/// The binary itself, started and spoken to over stdin and stdout.
#[test]
fn the_binary_speaks_over_stdin_and_stdout() {
    let root = folder("binary");
    let mut child = Command::new(env!("CARGO_BIN_EXE_sawhorse-mcp"))
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_board","arguments":{"name":"Shelf","thickness":19,"width":300,"length":600}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"save_model","arguments":{"path":"shelf.shz"}}}),
    ];
    for m in &messages {
        writeln!(stdin, "{m}").unwrap();
    }
    writeln!(stdin, "garbage").unwrap();
    drop(stdin);
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
    assert!(child.wait().unwrap().success());
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(lines[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(lines[1]["result"]["isError"], false, "{}", lines[1]);
    assert_eq!(lines[2]["result"]["isError"], false, "{}", lines[2]);
    assert_eq!(lines[3]["error"]["code"], -32700);
    assert!(root.join("shelf.shz").exists());
    // No folder, or one that is not there: it says why and stops.
    let out = Command::new(env!("CARGO_BIN_EXE_sawhorse-mcp"))
        .arg(root.join("not-here"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

/// The worked example in docs/MCP.md, sent line by line as it is written there, answers as
/// the page says.
#[test]
fn the_worked_example_in_the_docs_runs() {
    let doc = include_str!("../../../docs/MCP.md");
    let start = doc.find("## A worked example").unwrap();
    let block = &doc[start..];
    let block = &block[block.find("```json\n").unwrap() + 8..];
    let block = &block[..block.find("```").unwrap()];
    let root = folder("example");
    let mut out = Vec::new();
    let mut server = Server::new(&root).unwrap();
    serve(&mut server, block.as_bytes(), &mut out).unwrap();
    let answers: Vec<Value> = out
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).unwrap())
        .collect();
    // Ten requests and one notification.
    assert_eq!(answers.len(), 10, "{answers:?}");
    // measure between the sides: the inside width.
    let facing = &answers[7]["result"]["structuredContent"]["facing"];
    assert_eq!(facing[0]["distance_mm"], 462.0, "{facing}");
    // check_clearance between the first side and the top shelf: seated.
    let c = &answers[8]["result"]["structuredContent"];
    assert_eq!(c["gap_mm"], 0.0, "{c}");
    assert_eq!(c["overlap_mm3"], 0.0, "{c}");
    assert_eq!(c["seated"], true, "{c}");
    for a in &answers[1..] {
        assert_eq!(a["result"]["isError"], false, "{a}");
    }
    let second = &answers[3]["result"]["structuredContent"];
    assert_eq!(second["board"]["object"], 2, "{second}");
    let lo: Vec<f64> = second["board"]["min_mm"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let hi: Vec<f64> = second["board"]["max_mm"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    for (got, want) in lo
        .iter()
        .chain(&hi)
        .zip([481.0, 0.0, 0.0, 500.0, 250.0, 600.0])
    {
        assert!((got - want).abs() < 1e-6, "{second}");
    }
    assert_eq!(
        second["shares_name_with"][0]["items"][0]["object"], 1,
        "{second}"
    );
    let summary = &answers[6]["result"]["structuredContent"];
    assert_eq!(summary["parts"].as_array().unwrap().len(), 4);
    assert_eq!(
        summary["shared_names"],
        json!([{"name": "Shelf", "parts": 2}, {"name": "Side", "parts": 2}])
    );
    assert!(root.join("Shelf.shz").exists());
}

/// The verifier's finds on the first version.
#[test]
fn verifier_finds_stay_fixed() {
    let root = folder("finds");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let mut s = Session::new(&root);
    s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    let before = s.hash();

    // A draw of more points than a sketch may hold edges is refused before any work: the
    // kernel would take seconds to minutes and then refuse it anyway.
    let n = sawhorse_kernel::limits::MAX_SKETCH_EDGES + 1;
    let points: Vec<[f64; 3]> = (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [a.cos(), a.sin(), 0.0]
        })
        .collect();
    let started = std::time::Instant::now();
    let e = s.refused(
        "apply_ops",
        json!({"name": "Big", "ops": [{"op": "draw", "target": {"on": "plane", "origin": [0.0, 0.0, 0.0], "normal": [0.0, 0.0, 1.0]}, "points": points, "closed": true}]}),
    );
    assert!(started.elapsed().as_secs_f64() < 1.0, "{e}");
    assert!(e.contains("points"), "{e}");
    assert_eq!(s.hash(), before);

    // A file of the temporary file's name, left by someone else, is neither in the way of a
    // save nor deleted by it.
    let stray = root.join(".t.shz.sawhorse-mcp-tmp");
    std::fs::write(&stray, b"mine").unwrap();
    s.ok("save_model", json!({"path": "t.shz"}));
    assert_eq!(std::fs::read(&stray).unwrap(), b"mine");
    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.contains("sawhorse-mcp") && n != ".t.shz.sawhorse-mcp-tmp")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    // ".." is refused as such, even where it would stay inside the folder.
    s.ok("save_model", json!({"path": "a.shz"}));
    let e = s.refused("open_model", json!({"path": "sub/../a.shz"}));
    assert!(e.contains(".."), "{e}");

    // A name the kernel would refuse is refused by add_board, saying so.
    let e = s.refused(
        "add_board",
        json!({"name": "x".repeat(65), "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    assert!(e.contains("1 to 64"), "{e}");
    assert!(!e.contains("batch"), "{e}");
    s.ok(
        "add_board",
        json!({"name": "y".repeat(64), "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );

    // cut_list and nest refuse arguments they do not take, as the other tools do.
    s.refused("cut_list", json!({"pad": 1}));
    s.refused("nest", json!({"pad": 1}));
    s.ok(
        "cut_list",
        json!({"rounding": "half_mm", "visible_only": true}),
    );
    s.ok("nest", json!({"algorithm": "maxrects"}));

    // 2025-03-26 takes JSON-RPC batches, which this server does not: it answers in its own
    // revision. 2025-06-18 is echoed.
    let mut server = Server::new(&root).unwrap();
    for (asked, got) in [
        ("2025-03-26", PROTOCOL_VERSION),
        ("2025-06-18", "2025-06-18"),
    ] {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": asked, "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}});
        let r = server.handle_line(msg.to_string().as_bytes()).unwrap();
        assert_eq!(r["result"]["protocolVersion"], got);
    }
}

/// Names that mean something else on Windows (a drive, a stream of another file, a device,
/// a name Windows trims) are refused on every system, so a path names the same file
/// everywhere, and nothing is written for them. Names close to them are still saved.
#[test]
fn a_name_that_means_something_else_on_windows_is_refused_everywhere() {
    let root = folder("windows-names");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let mut s = Session::new(&root);
    s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    s.ok("save_model", json!({"path": "keep.shz"}));
    let listing = || {
        let mut names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    let before = listing();
    // On Windows "\" separates folders, so these two are a ".." and a path into sub there.
    #[cfg(not(windows))]
    let backslashes = ["..\\a.shz", "sub\\a.shz"];
    #[cfg(windows)]
    let backslashes: [&str; 0] = [];
    for path in backslashes.into_iter().chain([
        "keep.shz:stream.shz",
        "keep:a.shz",
        "sub:x.shz",
        "CON.shz",
        "con.shz",
        "NUL.shz",
        "nul.tar.shz",
        "AUX.shz",
        "PRN.shz",
        "COM1.shz",
        "lpt9.shz",
        "COM\u{b9}.shz",
        "CONOUT$.shz",
        "CON/a.shz",
        "sub./a.shz",
        "sub /a.shz",
        "a.shz.",
        "a\u{0}.shz",
        "a\nb.shz",
        "a\u{7f}.shz",
        "a<b.shz",
        "a|b.shz",
        "a?.shz",
        "a*.shz",
        "a\"b.shz",
    ]) {
        for tool in ["save_model", "open_model"] {
            let e = s.refused(tool, json!({"path": path}));
            assert!(
                e.contains("same file on every system"),
                "{tool} {path:?}: {e}"
            );
        }
    }
    assert_eq!(before, listing());
    for path in [
        "console.shz",
        "COM10.shz",
        "lpt.shz",
        "Nullify.shz",
        "con-table.shz",
        "a.b.shz",
        "sub/Shelf.SHZ",
    ] {
        s.ok("save_model", json!({"path": path}));
        s.ok("open_model", json!({"path": path}));
    }
}

/// A file already where the temporary file would go is neither written over nor removed:
/// the name the first try takes (`.<name>.<pid>-0.sawhorse-mcp-tmp`, beside the target) is
/// taken, and the save goes on under the next.
#[test]
fn a_save_never_writes_over_or_removes_a_file_it_did_not_make() {
    let root = folder("stray");
    let mut s = Session::new(&root);
    s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    let stray = root.join(format!(".t.shz.{}-0.sawhorse-mcp-tmp", std::process::id()));
    std::fs::write(&stray, b"not the server's").unwrap();
    s.ok("save_model", json!({"path": "t.shz"}));
    assert_eq!(std::fs::read(&stray).unwrap(), b"not the server's");
    let mut names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            format!(".t.shz.{}-0.sawhorse-mcp-tmp", std::process::id()),
            "t.shz".to_string()
        ]
    );
    // What was saved is the model: it opens to the same hash.
    let hash = s.hash();
    s.ok("new_model", json!({}));
    assert_eq!(
        s.ok("open_model", json!({"path": "t.shz"}))["model_hash"],
        hash
    );
}

/// A save replaces the name it is given and never writes through it into another file: a
/// hard link to a file outside the folder, and a link to another file inside it, each
/// become a file of their own, and the other file keeps its bytes.
#[cfg(unix)]
#[test]
fn a_save_replaces_a_link_and_never_writes_through_it() {
    let base = folder("through");
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let mut s = Session::new(&root);
    s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19.0, "width": 300.0, "length": 600.0}),
    );
    s.ok("save_model", json!({"path": "first.shz"}));
    let first = std::fs::read(root.join("first.shz")).unwrap();
    std::fs::write(base.join("away.shz"), &first).unwrap();
    std::fs::hard_link(base.join("away.shz"), root.join("hard.shz")).unwrap();
    std::os::unix::fs::symlink(root.join("first.shz"), root.join("soft.shz")).unwrap();
    s.ok(
        "add_board",
        json!({"name": "Top", "thickness": 19.0, "width": 300.0, "length": 600.0, "position": [0, 0, 400]}),
    );
    s.ok("save_model", json!({"path": "hard.shz"}));
    s.ok("save_model", json!({"path": "soft.shz"}));
    // Each save put a new file in the link's place; the files behind them are untouched.
    assert_eq!(std::fs::read(base.join("away.shz")).unwrap(), first);
    assert_eq!(std::fs::read(root.join("first.shz")).unwrap(), first);
    assert!(
        !std::fs::symlink_metadata(root.join("soft.shz"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata(root.join("hard.shz")).unwrap().nlink(), 1);
    assert_ne!(std::fs::read(root.join("hard.shz")).unwrap(), first);
}

/// What the first real run asked for: where the file went, sizes without float noise, and
/// a pointer to copy and array.
#[test]
fn first_run_finds_stay_fixed() {
    let root = folder("first-run");
    let mut s = Session::new(&root);
    // The second side and the shelves came back as 19.000000000000018 and 212.66999999999996.
    s.ok(
        "add_board",
        json!({"name": "Side", "thickness": 19, "width": 300, "length": 600, "length_axis": "z", "thickness_axis": "x"}),
    );
    let side = s.ok(
        "add_board",
        json!({"name": "Side", "thickness": 19, "width": 300, "length": 600, "position": [881, 0, 0], "length_axis": "z", "thickness_axis": "x"}),
    );
    let shelf = s.ok(
        "add_board",
        json!({"name": "Shelf", "thickness": 19, "width": 300, "length": 862, "position": [19, 0, 193.67]}),
    );
    let tenths = |v: &Value| {
        for x in v.as_array().unwrap() {
            let x = x.as_f64().unwrap();
            assert_eq!(x, (x * 10.0).round() / 10.0, "{v}");
        }
    };
    for out in [&side, &shelf] {
        tenths(&out["board"]["min_mm"]);
        tenths(&out["board"]["max_mm"]);
        tenths(&out["changes"]["added"][0]["size_mm"]);
    }
    assert_eq!(
        shelf["board"]["max_mm"],
        json!([881.0, 300.0, 212.7]),
        "{shelf}"
    );
    for p in s.ok("summary", json!({}))["parts"].as_array().unwrap() {
        tenths(&p["size_mm"]);
    }
    // The answer says where the file is, not only its name in the folder.
    let saved = s.ok("save_model", json!({"path": "Bookcase.shz"}));
    let file = std::path::PathBuf::from(saved["file"].as_str().unwrap());
    assert!(file.is_absolute(), "{saved}");
    assert_eq!(
        std::fs::canonicalize(&file).unwrap(),
        std::fs::canonicalize(root.join("Bookcase.shz")).unwrap()
    );
    let opened = s.ok("open_model", json!({"path": "Bookcase.shz"}));
    assert_eq!(opened["file"], saved["file"]);
    // apply_ops points to the ops that copy and repeat parts.
    let r = s.request("tools/list", json!({}));
    let apply = r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "apply_ops")
        .unwrap();
    let d = apply["description"].as_str().unwrap();
    assert!(d.contains("copy") && d.contains("array"), "{d}");
}

/// Requests ending in CRLF, as a client on Windows may send them, are read as the same
/// requests, and every answer ends in a bare newline.
#[test]
fn the_binary_reads_requests_that_end_in_crlf() {
    let root = folder("crlf");
    let [hello, ready] = greeting();
    let requests = [
        hello,
        ready,
        tool_call(1, "new_model", json!({"units": "mm"})),
        tool_call(2, "add_board", board("Shelf", 0.0)),
        tool_call(3, "save_model", json!({"path": "Shelf.shz"})),
        tool_call(4, "cut_list", json!({})),
    ];
    // A blank line after each request gets no answer.
    let (status, out) = run_binary(&root, &root, &requests, "\r\n\r\n");
    assert!(status.success());
    assert!(!out.contains(&b'\r'), "{}", String::from_utf8_lossy(&out));
    let lines = answers(&out);
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert_eq!(lines[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
    for (a, id) in lines[1..].iter().zip(1..) {
        assert_eq!(a["id"], id, "{a}");
        assert_eq!(a["result"]["isError"], false, "{a}");
    }
    let rows = &lines[4]["result"]["structuredContent"]["rows"];
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");
    assert_eq!(rows[0]["name"], "Shelf");
    assert_eq!(names(&root), ["Shelf.shz"]);
}

/// The start folder may be given relative to where the binary starts, and with a separator
/// or a "." at its end; each names the same folder.
#[test]
fn the_binary_takes_a_relative_start_folder_and_one_ending_in_a_separator() {
    let base = folder("relative");
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let mut given = vec!["root", "root/", "./root", "root/."];
    // On Windows "\" separates folders too.
    if cfg!(windows) {
        given.extend(["root\\", ".\\root", "root\\.", ".\\root/"]);
    }
    for (n, arg) in given.iter().enumerate() {
        let name = format!("rel-{n}.shz");
        let [hello, ready] = greeting();
        let requests = [
            hello,
            ready,
            tool_call(1, "add_board", board("Shelf", 0.0)),
            tool_call(2, "save_model", json!({"path": name})),
        ];
        let (status, out) = run_binary(&base, arg, &requests, "\n");
        assert!(status.success(), "{arg}");
        let lines = answers(&out);
        assert_eq!(lines.len(), 3, "{arg}: {lines:?}");
        let saved = &lines[2]["result"]["structuredContent"];
        assert_eq!(saved["path"], name.as_str(), "{arg}: {saved}");
        let file = PathBuf::from(saved["file"].as_str().unwrap());
        eprintln!("start folder {arg:?}: saved as {}", file.display());
        assert!(file.is_absolute(), "{arg}: {saved}");
        assert_eq!(
            std::fs::canonicalize(&file).unwrap(),
            std::fs::canonicalize(root.join(&name)).unwrap(),
            "{arg}"
        );
    }
    // Nothing was saved beside the folder, only in it.
    assert_eq!(names(&base), ["root"]);
    assert_eq!(names(&root).len(), given.len());
}

/// A save over a read-only file either replaces it (Linux and macOS let a file be renamed
/// over one) or is refused (Windows may refuse); either way the file there is whole, the
/// old one or the new one, and no temporary file is left.
#[test]
fn a_save_over_a_read_only_file_leaves_the_old_file_or_the_new_one_whole() {
    /// Puts a file's permissions back as they were, also when the test fails.
    struct Restore<'a>(&'a Path, std::fs::Permissions);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(self.0, self.1.clone());
        }
    }
    let root = folder("read-only");
    let target = root.join("ro.shz");
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    s.ok("save_model", json!({"path": "ro.shz"}));
    let old = std::fs::read(&target).unwrap();
    let writable = std::fs::metadata(&target).unwrap().permissions();
    let mut read_only = writable.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&target, read_only).unwrap();
    let restore = Restore(&target, writable);
    s.ok("add_board", board("Top", 400.0));
    let new = s.hash();
    let (failed, answer) = s.call("save_model", json!({"path": "ro.shz"}));
    let now = std::fs::read(&target).unwrap();
    eprintln!(
        "save over a read-only file: {} ({answer})",
        if failed { "refused" } else { "done" }
    );
    if failed {
        assert_eq!(now, old, "a refused save left the old file as it was");
    } else {
        assert_eq!(hash_of(&now), new, "a save reported done left the new file");
        assert_eq!(answer["model_hash"], new.as_str());
    }
    assert!(temporary_files(&root).is_empty(), "{:?}", names(&root));
    assert_eq!(names(&root), ["ro.shz"]);
    // The server goes on: a save under another name is done.
    s.ok("save_model", json!({"path": "other.shz"}));
    drop(restore);
    let _ = std::fs::remove_dir_all(&root);
}

/// A name differing only in case from a file that is there: where the volume ignores case
/// (Windows, and macOS by default) it opens that file, and a save over it leaves one file
/// with the new model; where it does not (Linux), it is another file, and the first keeps
/// its bytes. Which name the one file keeps is printed.
#[test]
fn a_name_differing_only_in_case_names_one_file_where_the_volume_ignores_case() {
    let root = folder("case");
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    s.ok("save_model", json!({"path": "Keep.shz"}));
    let first = std::fs::read(root.join("Keep.shz")).unwrap();
    let first_hash = s.hash();
    let ignores_case = root.join("KEEP.SHZ").exists();
    eprintln!("this volume ignores case: {ignores_case}");
    let (failed, opened) = s.call("open_model", json!({"path": "keep.shz"}));
    if ignores_case {
        assert!(!failed, "{opened}");
        assert_eq!(opened["path"], "keep.shz");
        assert_eq!(opened["model_hash"], first_hash.as_str());
    } else {
        assert!(failed, "{opened}");
        let e = opened["error"].as_str().unwrap();
        assert!(e.contains("no such file"), "{e}");
    }
    s.ok("add_board", board("Top", 400.0));
    let new = s.hash();
    assert_ne!(new, first_hash);
    let saved = s.ok("save_model", json!({"path": "keep.shz"}));
    assert_eq!(saved["path"], "keep.shz");
    let like: Vec<String> = names(&root)
        .into_iter()
        .filter(|n| n.eq_ignore_ascii_case("keep.shz"))
        .collect();
    eprintln!("after the save: {like:?}");
    if ignores_case {
        assert_eq!(like.len(), 1, "{like:?}");
        assert_eq!(hash_of(&std::fs::read(root.join(&like[0])).unwrap()), new);
    } else {
        assert_eq!(like, ["Keep.shz", "keep.shz"]);
        assert_eq!(std::fs::read(root.join("Keep.shz")).unwrap(), first);
        assert_eq!(hash_of(&std::fs::read(root.join("keep.shz")).unwrap()), new);
    }
    // Nothing else is there: no temporary file.
    assert_eq!(names(&root), like);
}

/// A start folder given through a link works, and files go where the link points. On
/// macOS the system's temporary folder is itself behind one (/var is a link to
/// /private/var), so every test's start folder there is given by a path not yet resolved;
/// this prints whether it is.
#[cfg(unix)]
#[test]
fn a_start_folder_given_through_a_link_saves_where_it_points() {
    let temp = std::env::temp_dir();
    let resolved = std::fs::canonicalize(&temp).unwrap();
    eprintln!(
        "the temporary folder {} resolves to {}",
        temp.display(),
        resolved.display()
    );
    let base = folder("start-link");
    let real = base.join("real");
    std::fs::create_dir_all(real.join("sub")).unwrap();
    let alias = base.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let mut s = Session::new(&alias);
    s.ok("add_board", board("Shelf", 0.0));
    let saved = s.ok("save_model", json!({"path": "sub/a.shz"}));
    assert_eq!(saved["path"], "sub/a.shz");
    let file = PathBuf::from(saved["file"].as_str().unwrap());
    let real_resolved = std::fs::canonicalize(&real).unwrap();
    assert_eq!(file, real_resolved.join("sub").join("a.shz"), "{saved}");
    assert!(file.starts_with(&resolved), "{saved}");
    let hash = s.hash();
    assert_eq!(
        hash_of(&std::fs::read(real.join("sub/a.shz")).unwrap()),
        hash
    );
    // The folder the link is in is still outside.
    for path in ["../a.shz", "../alias/a.shz"] {
        let e = s.refused("save_model", json!({"path": path}));
        assert!(e.contains(".."), "{path}: {e}");
    }
    assert_eq!(names(&base), ["alias", "real"]);
    // Given with a separator at its end, the link names the same folder.
    let mut s = Session::new(&base.join("alias/"));
    assert_eq!(
        s.ok("open_model", json!({"path": "sub/a.shz"}))["model_hash"],
        hash.as_str()
    );
    std::fs::remove_file(&alias).unwrap();
    let _ = std::fs::remove_dir_all(&base);
}

/// Lane 51: an absolute path is refused as absolute on every system, for each tool that
/// takes a path, and nothing is made or changed outside the folder or in it. Removing the
/// absolute check in paths.rs (a root or a drive read as nothing) makes "/probe.shz" save
/// inside the folder and a real absolute path fail only because its folders are not there;
/// either way the message here is the one that says why.
#[test]
fn an_absolute_path_is_refused_as_absolute_everywhere() {
    let base = folder("absolute");
    let root = base.join("root");
    let away = base.join("away");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&away).unwrap();
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    s.ok("save_model", json!({"path": "keep.shz"}));
    let kept = std::fs::read(root.join("keep.shz")).unwrap();
    std::fs::write(away.join("far.shz"), &kept).unwrap();
    let far = std::fs::canonicalize(away.join("far.shz")).unwrap();
    assert!(far.is_absolute());
    let new_far = std::fs::canonicalize(&away).unwrap().join("new.shz");
    let probe = Path::new("/probe.shz");
    let probe_there = probe.exists();
    let hash = s.hash();
    for path in [
        "/probe.shz".to_string(),
        far.to_str().unwrap().to_string(),
        new_far.to_str().unwrap().to_string(),
    ] {
        for tool in ["save_model", "open_model"] {
            let e = s.refused(tool, json!({"path": path}));
            assert!(
                e.contains("paths are relative to the server's folder, not absolute"),
                "{tool} {path}: {e}"
            );
        }
        // new_model takes no path at all: one given is refused and the model kept.
        let e = s.refused("new_model", json!({"path": path}));
        assert!(e.contains("path"), "new_model {path}: {e}");
        assert_eq!(s.hash(), hash);
    }
    assert_eq!(names(&root), ["keep.shz"]);
    assert_eq!(names(&away), ["far.shz"]);
    assert_eq!(std::fs::read(&far).unwrap(), kept);
    assert_eq!(probe.exists(), probe_there);
    let _ = std::fs::remove_dir_all(&base);
}

/// Lane 51: spaces around a path are trimmed on every system: " a.shz " and "a.shz " save
/// to "a.shz", answer "a.shz", and open it, and nothing else is made. Without the trim in
/// paths.rs the space at the end is a name Windows trims, refused.
#[test]
fn spaces_around_a_path_are_trimmed() {
    let root = folder("trimmed");
    let mut s = Session::new(&root);
    s.ok("add_board", board("Shelf", 0.0));
    for path in [" a.shz ", "a.shz ", "  a.shz"] {
        let saved = s.ok("save_model", json!({"path": path}));
        assert_eq!(saved["path"], "a.shz", "{path:?}: {saved}");
        assert_eq!(names(&root), ["a.shz"], "{path:?}");
        let opened = s.ok("open_model", json!({"path": path}));
        assert_eq!(opened["path"], "a.shz", "{path:?}: {opened}");
        assert_eq!(s.ok("summary", json!({}))["path"], "a.shz");
    }
    let hash = s.hash();
    assert_eq!(hash_of(&std::fs::read(root.join("a.shz")).unwrap()), hash);
    let _ = std::fs::remove_dir_all(&root);
}
