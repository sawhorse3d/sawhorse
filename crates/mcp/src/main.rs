//! `sawhorse-mcp <folder>`: the Model Context Protocol server (see the crate docs). It reads
//! and writes `.shz` files under `<folder>` only, and speaks over stdin and stdout.
use std::process::ExitCode;

const USAGE: &str = "Usage: sawhorse-mcp <folder>

A Model Context Protocol server: an agent starts it and talks to it over stdin and stdout.
It reads and writes .shz files under <folder> only. See docs/MCP.md.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let folder = match args.as_slice() {
        [a] if a == "-h" || a == "--help" => {
            eprintln!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        [folder] => folder,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut server = match sawhorse_mcp::Server::new(std::path::Path::new(folder)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sawhorse-mcp: {e}");
            return ExitCode::from(2);
        }
    };
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    match sawhorse_mcp::serve(&mut server, stdin, stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sawhorse-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
