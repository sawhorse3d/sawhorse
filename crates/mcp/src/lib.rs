//! A Model Context Protocol server on the Sawhorse kernel (issue #14): an agent starts
//! `sawhorse-mcp <folder>` and talks to it over stdin and stdout, one JSON-RPC message a
//! line, to build a model from boards, change it with the kernel's own ops, read back its
//! parts, cut list and nesting, undo, and save it as a `.shz` the app opens as it is.
//!
//! It speaks MCP revision [`PROTOCOL_VERSION`] (and answers a client asking for one of
//! [`SUPPORTED_VERSIONS`] in that revision's terms, which for the tools here are the same).
//! The tools are in [`tools`]; `docs/MCP.md` says what each takes and gives, with a worked
//! example.
//!
//! What it holds to (the review on #14):
//! - **Files**: only `.shz` files under the folder it was started with, every path resolved
//!   first; one that leaves the folder through `..`, an absolute path or a link is refused
//!   ([`paths`]). Saving goes through the kernel's writer (`sawhorse_shz::write`) and the
//!   file is read back, and its model hash compared, before the save is reported done.
//! - **Untrusted input**: a message that is not JSON, nested deeper than the parser's limit,
//!   longer than [`MAX_MESSAGE`], or of the wrong shape gets an error back; the server goes
//!   on reading. A tool that panics is reported as a failed call, and the document is as it
//!   was before it (every change is applied to a copy first).
//! - **Results**: parts by name; where two parts share a name the answer says so and never
//!   picks one.
//!
//! Nothing here touches the network, opens a window or asks a question.

use std::io::{BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Value, json};

pub mod paths;
pub mod tools;

pub use tools::Server;

/// The MCP revision this server speaks.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// Revisions a client may ask for and get back as they asked. Their tool calls, results and
/// stdio framing are the same as far as this server goes.
pub const SUPPORTED_VERSIONS: [&str; 3] = [PROTOCOL_VERSION, "2025-06-18", "2025-03-26"];

/// Longest message read, in bytes (a batch of `apply_ops` at its 2,000-op limit fits many
/// times over). A longer one is skipped to its end without being held, and answered with
/// an error.
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn reply(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

impl Server {
    /// Answer one message (one line, without its newline). `None`: nothing to send back
    /// (a notification, or a client's response).
    pub fn handle_line(&mut self, line: &[u8]) -> Option<Value> {
        let msg: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, PARSE_ERROR, format!("not JSON: {e}"))),
        };
        let Some(obj) = msg.as_object() else {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "a message is one JSON object (batches are not part of the protocol)",
            ));
        };
        let id = obj.get("id").cloned();
        let id_ok = matches!(&id, Some(Value::String(_) | Value::Number(_)));
        let method = obj.get("method").and_then(Value::as_str);
        let Some(method) = method else {
            // A client's response to a request of ours (we send none): nothing to say.
            if obj.contains_key("result") || obj.contains_key("error") {
                return None;
            }
            let id = if id_ok { id.unwrap() } else { Value::Null };
            return Some(error(id, INVALID_REQUEST, "a request needs a method"));
        };
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return id.map(|id| {
                let id = if id_ok { id } else { Value::Null };
                error(id, INVALID_REQUEST, "jsonrpc must be \"2.0\"")
            });
        }
        // A notification: no answer, whatever it says.
        let id = id?;
        if !id_ok {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "id must be a string or a number",
            ));
        }
        let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
        Some(match method {
            "initialize" => reply(id, initialize(&params)),
            "ping" => reply(id, json!({})),
            "tools/list" => reply(id, json!({"tools": tools::list()})),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return Some(error(id, INVALID_PARAMS, "tools/call needs a tool name"));
                };
                if !tools::NAMES.contains(&name) {
                    return Some(error(
                        id,
                        INVALID_PARAMS,
                        format!("no tool named {name} (tools/list lists them)"),
                    ));
                }
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !args.is_object() {
                    return Some(error(id, INVALID_PARAMS, "arguments must be an object"));
                }
                let name = name.to_string();
                let result = catch_unwind(AssertUnwindSafe(|| self.call(&name, &args)))
                    .unwrap_or_else(|_| {
                        Err(format!(
                            "{name} failed inside the server (a bug: please report it); the model is as it was"
                        ))
                    });
                reply(id, tool_result(result))
            }
            other => error(id, METHOD_NOT_FOUND, format!("no method {other}")),
        })
    }
}

/// The answer to `initialize`: the client's revision when we speak it, else ours.
fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .filter(|v| SUPPORTED_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {
            "name": "sawhorse-mcp",
            "title": "Sawhorse",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": tools::INSTRUCTIONS,
    })
}

/// A tool's answer: the structured result, the same as text, and whether it failed.
fn tool_result(result: Result<Value, String>) -> Value {
    let (structured, is_error) = match result {
        Ok(v) => (v, false),
        Err(e) => (json!({"error": e}), true),
    };
    let text = serde_json::to_string(&structured).unwrap_or_default();
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": is_error,
    })
}

/// Read messages from `input`, one a line, and write each answer to `output` as one line,
/// until `input` ends. A line longer than [`MAX_MESSAGE`] is read past, not held, and
/// answered with an error. Only a failure to read or write ends it early.
pub fn serve<R: BufRead, W: Write>(
    server: &mut Server,
    mut input: R,
    mut output: W,
) -> std::io::Result<()> {
    let mut line = Vec::new();
    let mut over = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return finish(server, &line, over, &mut output);
        }
        let (take, end) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i, Some(i + 1)),
            None => (chunk.len(), None),
        };
        if !over {
            if line.len() + take > MAX_MESSAGE {
                over = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(&chunk[..take]);
            }
        }
        input.consume(end.unwrap_or(take));
        if end.is_some() {
            finish(server, &line, over, &mut output)?;
            line.clear();
            over = false;
        }
    }
}

/// Answer one complete line (nothing for a blank one).
fn finish<W: Write>(
    server: &mut Server,
    line: &[u8],
    over: bool,
    out: &mut W,
) -> std::io::Result<()> {
    let answer = if over {
        Some(error(
            Value::Null,
            INVALID_REQUEST,
            format!("a message may be at most {MAX_MESSAGE} bytes"),
        ))
    } else {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {
            None
        } else {
            server.handle_line(line)
        }
    };
    if let Some(a) = answer {
        // serde_json escapes every newline inside strings, so one answer is one line.
        let text = serde_json::to_string(&a).unwrap_or_default();
        out.write_all(text.as_bytes())?;
        out.write_all(b"\n")?;
        out.flush()?;
    }
    Ok(())
}
