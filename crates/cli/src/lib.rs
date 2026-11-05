//! A headless Sawhorse (issue #14): read a saved `.shz` and print its cut list or its
//! nesting as JSON, exactly what the app's Cut list pane and Nesting section read, so a
//! script or an agent can get them without opening the app.
//!
//! ```text
//! sawhorse-cli cutlist Bookshelf.shz [--visible-only] [--rounding sixteenth|thirty-second|half-mm] [--pretty]
//! sawhorse-cli nest Bookshelf.shz [--visible-only] [--pretty]
//! ```
//!
//! The JSON is the kernel's own ([`sawhorse_kernel::cutlist::CutList`] and
//! [`sawhorse_kernel::nesting::Nesting`] through serde), hash included, so what this prints
//! for a file is what the app shows for it. Nothing here writes anything.

use sawhorse_kernel::cutlist::{CutOptions, Rounding};
use sawhorse_kernel::nesting::NestOptions;

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The cut list of the file.
    CutList {
        path: String,
        options: CutOptions,
        pretty: bool,
    },
    /// The nesting of the file.
    Nest {
        path: String,
        options: NestOptions,
        pretty: bool,
    },
    /// `--help`, or no arguments.
    Help,
}

pub const USAGE: &str = "\
Usage: sawhorse-cli <command> <file.shz> [options]

Commands
  cutlist   The cut list as JSON: one row per part with its finished size, quantity,
            grain, stock and board feet or area, the totals, and a hash.
  nest      The nesting as JSON: every sheet and board with its placements and cuts,
            the parts no stock takes, the totals and the stock library used.

Options
  --visible-only          Count only what is drawn (hidden parts, tags and groups left out).
  --rounding <r>          cutlist only: sixteenth (the default), thirty-second or half-mm.
  --pretty                Indented JSON.
  -h, --help              This text.

The JSON is the app's own: what the Cut list pane and the Nesting section read.";

/// Parse the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    let Some(command) = it.next() else {
        return Ok(Command::Help);
    };
    if command == "-h" || command == "--help" || command == "help" {
        return Ok(Command::Help);
    }
    if command != "cutlist" && command != "nest" {
        return Err(format!(
            "unknown command {command}: cutlist or nest (--help for the usage)"
        ));
    }
    let mut path: Option<String> = None;
    let mut visible_only = false;
    let mut rounding = Rounding::default();
    let mut pretty = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--visible-only" => visible_only = true,
            "--pretty" => pretty = true,
            "--rounding" => {
                if command != "cutlist" {
                    return Err("--rounding is for cutlist only".to_string());
                }
                let Some(r) = it.next() else {
                    return Err(
                        "--rounding needs a value: sixteenth, thirty-second or half-mm".to_string(),
                    );
                };
                rounding = match r.as_str() {
                    "sixteenth" | "1/16" => Rounding::Sixteenth,
                    "thirty-second" | "thirty_second" | "1/32" => Rounding::ThirtySecond,
                    "half-mm" | "half_mm" | "0.5mm" => Rounding::HalfMm,
                    other => {
                        return Err(format!(
                            "unknown rounding {other}: sixteenth, thirty-second or half-mm"
                        ));
                    }
                };
            }
            s if s.starts_with('-') => {
                return Err(format!("unknown option {s} (--help for the usage)"));
            }
            s => {
                if path.replace(s.to_string()).is_some() {
                    return Err("one file at a time".to_string());
                }
            }
        }
    }
    let Some(path) = path else {
        return Err(format!(
            "{command} needs a file: sawhorse-cli {command} <file.shz>"
        ));
    };
    Ok(if command == "cutlist" {
        Command::CutList {
            path,
            options: CutOptions {
                rounding,
                visible_only,
            },
            pretty,
        }
    } else {
        Command::Nest {
            path,
            options: NestOptions {
                visible_only,
                algorithm: None,
            },
            pretty,
        }
    })
}

/// The cut list of a `.shz` file's bytes, as JSON.
pub fn cut_list_json(shz: &[u8], options: &CutOptions, pretty: bool) -> Result<String, String> {
    let opened = sawhorse_shz::read(shz).map_err(|e| format!("could not open the file: {e}"))?;
    let list = opened.document.model().cut_list(options);
    to_json(&list, pretty)
}

/// The nesting of a `.shz` file's bytes, as JSON.
pub fn nest_json(shz: &[u8], options: &NestOptions, pretty: bool) -> Result<String, String> {
    let opened = sawhorse_shz::read(shz).map_err(|e| format!("could not open the file: {e}"))?;
    let nesting = opened.document.model().nest(options);
    to_json(&nesting, pretty)
}

fn to_json<T: serde::Serialize>(value: &T, pretty: bool) -> Result<String, String> {
    if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|e| e.to_string())
}

/// Run the parsed command: the text to print, or why not.
pub fn run(command: &Command) -> Result<String, String> {
    match command {
        Command::Help => Ok(USAGE.to_string()),
        Command::CutList {
            path,
            options,
            pretty,
        } => cut_list_json(&read(path)?, options, *pretty),
        Command::Nest {
            path,
            options,
            pretty,
        } => nest_json(&read(path)?, options, *pretty),
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))
}
