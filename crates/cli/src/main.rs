//! `sawhorse-cli`: the cut list or the nesting of a saved `.shz`, as JSON (see the crate docs).
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match sawhorse_cli::parse(&args).and_then(|c| sawhorse_cli::run(&c)) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sawhorse-cli: {e}");
            ExitCode::from(2)
        }
    }
}
