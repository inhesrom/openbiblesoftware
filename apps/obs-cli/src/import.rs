//! `obs-cli import`: USFM files or a directory of them → `.obr`, with the Import report.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use obs_formats::{UsfmImport, UsfmOptions};
use obs_model::Direction;
use obs_store::write_obr;

const USAGE: &str = "usage: obs-cli import <file.usfm|dir>... -o <out.obr> [--id ID] [--title T] [--abbrev A] [--lang L] [--rtl]";

fn usfm_files(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "usfm" | "sfm"))
        })
        .collect();
    files.sort();
    Ok(files)
}

pub fn import(args: &[String]) -> ExitCode {
    let mut inputs: Vec<&str> = Vec::new();
    let mut out: Option<String> = None;
    let mut options = UsfmOptions {
        tool_version: format!("obs-cli {}", env!("CARGO_PKG_VERSION")),
        ..UsfmOptions::default()
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| match it.next() {
            Some(v) => Ok(v.clone()),
            None => Err(format!("`{name}` needs a value")),
        };
        let result = match a.as_str() {
            "-o" | "--out" => value(a).map(|v| out = Some(v)),
            "--id" => value(a).map(|v| options.resource_id = v),
            "--title" => value(a).map(|v| options.title = v),
            "--abbrev" => value(a).map(|v| options.abbreviation = v),
            "--lang" => value(a).map(|v| options.language = v),
            "--rtl" => {
                options.direction = Direction::Rtl;
                Ok(())
            }
            flag if flag.starts_with("--") => Err(format!("unknown option `{flag}`")),
            input => {
                inputs.push(input);
                Ok(())
            }
        };
        if let Err(e) = result {
            eprintln!("obs-cli import: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    let (Some(out), false) = (out.as_deref(), inputs.is_empty()) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if let Some(first) = inputs.first() {
        let p = Path::new(first);
        options.source_file = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    let mut import = UsfmImport::new(options);
    for input in &inputs {
        let files = match usfm_files(Path::new(input)) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("obs-cli import: {input}: {e}");
                return ExitCode::FAILURE;
            }
        };
        for file in files {
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("obs-cli import: {}: {e}", file.display());
                    return ExitCode::FAILURE;
                }
            };
            if let Err(e) = import.add_book(&name, source) {
                eprintln!("obs-cli import: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let (resource, report) = match import.finish() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("obs-cli import: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = write_obr(&resource, Path::new(out)) {
        eprintln!("obs-cli import: {out}: {e}");
        return ExitCode::FAILURE;
    }
    print!("{report}");
    println!("  wrote {out}");
    ExitCode::SUCCESS
}
