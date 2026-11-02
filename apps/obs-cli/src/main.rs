//! Command-line tool for Open Bible Software.

mod import;
mod inspect;
mod shape;

use std::process::ExitCode;

use obs_engine::{JobMode, Session};
use serde_json::Value;

const USAGE: &str = "usage: obs-cli <command> [args]

commands:
  run <command-id> [name=value ...|'{json}']   run an engine command, print JSON
  import <file.usfm|dir>... -o <out.obr> [--id ID] [--title T] [--abbrev A] [--lang L] [--rtl]
                                                import USFM to a resource file, print the Import report
  inspect <file.obr> [--unit ID] [--verse REF]  show a resource file's contents
  shape [--size PX] [--svg OUT] [--smcp] <text>... shape text with the bundled fonts
  write-sample <file.obr>                       write a tiny sample Bible (for trying inspect)
  commands                                     list engine commands
  version                                      print the version
  help                                         show this message

examples:
  obs-cli run reference.parse text='jn 3:16-18; rom 5:8'
  obs-cli run text.transliterate text=ἀγάπη";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut session = Session::new(JobMode::default_for_host());
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("version" | "-V" | "--version") => {
            println!("obs-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("commands") => {
            for c in session.commands().iter() {
                let params: Vec<String> = c
                    .params
                    .iter()
                    .map(|p| {
                        let opt = if p.required { "" } else { "?" };
                        format!("{}{opt}:{}", p.name, p.kind.name())
                    })
                    .collect();
                println!("{:<22} {:<32} {}", c.id, c.label, params.join(" "));
            }
            ExitCode::SUCCESS
        }
        Some("run") => run(&mut session, &args[1..]),
        Some("import") => import::import(&args[1..]),
        Some("inspect") => inspect::inspect(&args[1..]),
        Some("shape") => shape::shape(&args[1..]),
        Some("write-sample") => inspect::write_sample(&args[1..]),
        Some(other) => {
            eprintln!("obs-cli: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(session: &mut Session, args: &[String]) -> ExitCode {
    let Some(id) = args.first() else {
        eprintln!("obs-cli run: missing command id (see `obs-cli commands`)");
        return ExitCode::from(2);
    };
    let Some(spec) = session.commands().get(id).copied() else {
        eprintln!("obs-cli: unknown command `{id}` (see `obs-cli commands`)");
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    let params = if let [json] = rest
        && json.trim_start().starts_with('{')
    {
        match serde_json::from_str::<Value>(json) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("obs-cli: parameters are not valid JSON: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        let mut pairs = Vec::new();
        for a in rest {
            let Some((name, value)) = a.split_once('=') else {
                eprintln!("obs-cli: expected name=value, got `{a}`");
                return ExitCode::from(2);
            };
            pairs.push((name, value));
        }
        match spec.params_from_pairs(&pairs) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("obs-cli: {e}");
                return ExitCode::from(2);
            }
        }
    };
    match session.run(id, &params) {
        Ok(out) => {
            match serde_json::to_string_pretty(&out) {
                Ok(text) => println!("{text}"),
                Err(e) => println!("{out} ({e})"),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("obs-cli: {e}");
            ExitCode::FAILURE
        }
    }
}
