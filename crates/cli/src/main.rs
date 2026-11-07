//! `sawhorse-cli`: the cut list or the nesting of a saved `.shz`, as JSON (see the crate docs).
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match sawhorse_cli::parse(&args).and_then(|c| sawhorse_cli::run(&c)) {
        Ok(printed) => {
            // The reader's notes first, on stderr: stdout stays JSON only.
            for note in &printed.notes {
                eprintln!("sawhorse-cli: note: {note}");
            }
            println!("{}", printed.text);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sawhorse-cli: {e}");
            ExitCode::from(2)
        }
    }
}
