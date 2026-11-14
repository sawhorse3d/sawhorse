//! The server over its own protocol (issue #14): what an agent sends on stdin and gets back
//! on stdout, the tools, and the holds from the issue: files only under the start folder,
//! saved through the kernel's writer and read back, untrusted input answered with an
//! error and never a crash or a hang, and parts by name with shared names reported.
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sawhorse_mcp::{MAX_MESSAGE, PROTOCOL_VERSION, Server, serve};
use serde_json::{Value, json};

/// A fresh folder for one test, under the target dir's temp space.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("sawhorse-mcp-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Session {
    server: Server,
    next: u64,
}

impl Session {
    fn new(root: &Path) -> Session {
        let mut s = Session {
            server: Server::new(root).unwrap(),
            next: 1,
        };
        let r = s.request("initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}));
        assert!(r.get("result").is_some(), "{r}");
        assert!(
            s.line(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string())
                .is_none()
        );
        s
    }

    fn line(&mut self, text: &str) -> Option<Value> {
        self.server.handle_line(text.as_bytes())
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let r = self
            .line(&msg.to_string())
            .expect("a request gets an answer");
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], id);
        r
    }

    /// A tool's structured result, and whether the tool said it failed.
    fn call(&mut self, tool: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let result = r.get("result").unwrap_or_else(|| panic!("{tool}: {r}"));
        let error = result["isError"].as_bool().unwrap_or(false);
        // The structured result and the text block say the same thing.
        let text = result["content"][0]["text"].as_str().unwrap();
        let structured = result["structuredContent"].clone();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), structured);
        (error, structured)
    }

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let (error, v) = self.call(tool, args.clone());
        assert!(!error, "{tool} {args}: {v}");
        v
    }

    fn refused(&mut self, tool: &str, args: Value) -> String {
        let (error, v) = self.call(tool, args.clone());
        assert!(error, "{tool} {args} should be refused: {v}");
        v["error"].as_str().unwrap().to_string()
    }

    fn hash(&mut self) -> String {
        self.ok("summary", json!({}))["model_hash"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

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
    assert!(near(hi[0].as_f64().unwrap(), 36.0 * 25.4), "{out}");
    assert!(near(hi[2].as_f64().unwrap(), 0.75 * 25.4), "{out}");
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
    // Eight requests and one notification.
    assert_eq!(answers.len(), 8, "{answers:?}");
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
