//! What the server's test files share: a fresh folder, a session over the protocol, the
//! binary run over stdin and stdout, and what is in a folder.
// Each test file uses its own share of these.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use sawhorse_mcp::{PROTOCOL_VERSION, Server};
use serde_json::{Value, json};

/// A fresh folder for one test, under the system's temporary folder.
pub fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("sawhorse-mcp-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub struct Session {
    pub server: Server,
    next: u64,
}

impl Session {
    pub fn new(root: &Path) -> Session {
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

    pub fn line(&mut self, text: &str) -> Option<Value> {
        self.server.handle_line(text.as_bytes())
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
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
    pub fn call(&mut self, tool: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let result = r.get("result").unwrap_or_else(|| panic!("{tool}: {r}"));
        let error = result["isError"].as_bool().unwrap_or(false);
        // The structured result and the text block say the same thing.
        let text = result["content"][0]["text"].as_str().unwrap();
        let structured = result["structuredContent"].clone();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), structured);
        (error, structured)
    }

    pub fn ok(&mut self, tool: &str, args: Value) -> Value {
        let (error, v) = self.call(tool, args.clone());
        assert!(!error, "{tool} {args}: {v}");
        v
    }

    pub fn refused(&mut self, tool: &str, args: Value) -> String {
        let (error, v) = self.call(tool, args.clone());
        assert!(error, "{tool} {args} should be refused: {v}");
        v["error"].as_str().unwrap().to_string()
    }

    pub fn hash(&mut self) -> String {
        self.ok("summary", json!({}))["model_hash"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Still answering.
    pub fn answers(&mut self) {
        let r = self.request("ping", json!({}));
        assert_eq!(r["result"], json!({}), "{r}");
    }
}

pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A 19 x 300 x 600 mm board lying flat at height `z` mm.
pub fn board(name: &str, z: f64) -> Value {
    json!({"name": name, "thickness": 19.0, "width": 300.0, "length": 600.0, "position": [0.0, 0.0, z]})
}

/// The model hash of a `.shz` file's bytes, as the server writes it.
pub fn hash_of(bytes: &[u8]) -> String {
    let opened = sawhorse_shz::read(bytes).unwrap();
    format!("{:016x}", opened.document.model().hash())
}

/// The names in `dir`, sorted.
pub fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The names in `dir` a save's temporary file would have.
pub fn temporary_files(dir: &Path) -> Vec<String> {
    names(dir)
        .into_iter()
        .filter(|n| n.contains("sawhorse-mcp-tmp"))
        .collect()
}

/// Start the binary in `cwd` on `folder`, send `requests` (each followed by `end`), close its
/// stdin, and give back how it ended and everything it wrote to stdout. The requests are
/// written from another thread, so a full pipe either way never holds it up.
pub fn run_binary(
    cwd: &Path,
    folder: impl AsRef<std::ffi::OsStr>,
    requests: &[Value],
    end: &str,
) -> (ExitStatus, Vec<u8>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sawhorse-mcp"))
        .current_dir(cwd)
        .arg(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let text: String = requests.iter().map(|r| format!("{r}{end}")).collect();
    // A write that fails because the binary already stopped shows in its answers.
    let writer = std::thread::spawn(move || stdin.write_all(text.as_bytes()));
    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    let _ = writer.join();
    (child.wait().unwrap(), out)
}

/// The answers in what the binary wrote, one JSON value a line.
pub fn answers(out: &[u8]) -> Vec<Value> {
    out.split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).unwrap())
        .collect()
}

/// A tools/call request.
pub fn tool_call(id: u64, tool: &str, args: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": args}})
}

/// The initialize request and the notification after it.
pub fn greeting() -> [Value; 2] {
    [
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ]
}
