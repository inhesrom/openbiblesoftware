//! Command-line tool for Open Bible Software.
//!
//! Subcommands (import, inspect, index, run, starter) arrive with their tickets.

use std::process::ExitCode;

const USAGE: &str = "usage: obs-cli <command>

commands:
  help       show this message
  version    print the version";

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        None | Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("version" | "-V" | "--version") => {
            println!("obs-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("obs-cli: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
