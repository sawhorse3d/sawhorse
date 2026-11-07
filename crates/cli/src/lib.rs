//! A headless Sawhorse (issue #14): read a saved `.shz` and print its cut list or its
//! nesting as JSON, exactly what the app's Cut list pane and Nesting section read, so a
//! script or an agent can get them without opening the app.
//!
//! ```text
//! sawhorse-cli cutlist Bookshelf.shz [--visible-only] [--rounding sixteenth|thirty-second|half-mm] [--pretty]
//! sawhorse-cli nest Bookshelf.shz [--visible-only] [--algorithm guillotine|maxrects] [--pretty]
//! ```
//!
//! The JSON is the kernel's own ([`sawhorse_kernel::cutlist::CutList`] and
//! [`sawhorse_kernel::nesting::Nesting`] through serde), hash included, so what this prints
//! for a file is what the app shows for it. What the reader had to do to open the file (a
//! texture missing from it, an object rebuilt from `geometry.glb`) goes to stderr, one
//! `note:` line each, so a script can tell a clean read from a repaired one while the JSON
//! on stdout stays parseable. Nothing here writes anything.

use sawhorse_kernel::cutlist::{CutOptions, Rounding};
use sawhorse_kernel::nesting::NestOptions;
use sawhorse_kernel::stock::NestAlgorithm;
use sawhorse_shz::Opened;

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
  --algorithm <a>         nest only: guillotine (straight cuts edge to edge, a table saw) or
                          maxrects (tightest, a CNC); unset, the document's own.
  --pretty                Indented JSON.
  -h, --help              This text.

The JSON is the app's own: what the Cut list pane and the Nesting section read. What the
reader had to do to open the file goes to stderr as \"note:\" lines; stdout is JSON only.";

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
    let mut algorithm: Option<NestAlgorithm> = None;
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
            "--algorithm" => {
                if command != "nest" {
                    return Err("--algorithm is for nest only".to_string());
                }
                let Some(a) = it.next() else {
                    return Err("--algorithm needs a value: guillotine or maxrects".to_string());
                };
                algorithm = Some(match a.as_str() {
                    "guillotine" | "table-saw" => NestAlgorithm::Guillotine,
                    "maxrects" | "cnc" => NestAlgorithm::Maxrects,
                    other => {
                        return Err(format!("unknown algorithm {other}: guillotine or maxrects"));
                    }
                });
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
                algorithm,
            },
            pretty,
        }
    })
}

/// Open a `.shz` file's bytes. `notes` on the result say what the reader had to do.
pub fn open(shz: &[u8]) -> Result<Opened, String> {
    sawhorse_shz::read(shz).map_err(|e| format!("could not open the file: {e}"))
}

/// The cut list of an opened file, as JSON.
pub fn cut_list_text(
    opened: &Opened,
    options: &CutOptions,
    pretty: bool,
) -> Result<String, String> {
    to_json(&opened.document.model().cut_list(options), pretty)
}

/// The nesting of an opened file, as JSON.
pub fn nest_text(opened: &Opened, options: &NestOptions, pretty: bool) -> Result<String, String> {
    to_json(&opened.document.model().nest(options), pretty)
}

/// The cut list of a `.shz` file's bytes, as JSON.
pub fn cut_list_json(shz: &[u8], options: &CutOptions, pretty: bool) -> Result<String, String> {
    cut_list_text(&open(shz)?, options, pretty)
}

/// The nesting of a `.shz` file's bytes, as JSON.
pub fn nest_json(shz: &[u8], options: &NestOptions, pretty: bool) -> Result<String, String> {
    nest_text(&open(shz)?, options, pretty)
}

fn to_json<T: serde::Serialize>(value: &T, pretty: bool) -> Result<String, String> {
    if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|e| e.to_string())
}

/// What a command prints: the text for stdout, and the reader's notes for stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printed {
    pub text: String,
    pub notes: Vec<String>,
}

/// Run the parsed command: what to print, or why not.
pub fn run(command: &Command) -> Result<Printed, String> {
    match command {
        Command::Help => Ok(Printed {
            text: USAGE.to_string(),
            notes: Vec::new(),
        }),
        Command::CutList {
            path,
            options,
            pretty,
        } => {
            let opened = open(&read(path)?)?;
            Ok(Printed {
                text: cut_list_text(&opened, options, *pretty)?,
                notes: opened.notes,
            })
        }
        Command::Nest {
            path,
            options,
            pretty,
        } => {
            let opened = open(&read(path)?)?;
            Ok(Printed {
                text: nest_text(&opened, options, *pretty)?,
                notes: opened.notes,
            })
        }
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))
}
