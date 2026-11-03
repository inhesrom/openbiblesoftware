//! `obs-cli import`: USFM files or a directory of them, or a module in a SWORD library → `.obr`,
//! with the Import report.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use obs_formats::{
    ImportError, ImportReport, SwordOptions, SwordSource, UsfmImport, UsfmOptions, import_sword,
};
use obs_model::{Direction, Resource};
use obs_store::write_obr;

const USAGE: &str = "usage: obs-cli import <file.usfm|dir>... -o <out.obr> [--id ID] [--title T] [--abbrev A] [--lang L] [--rtl]
       obs-cli import <sword-library-dir|mods.d/NAME.conf> -o <out.obr> [--module NAME] [--id ID] [--title T] [--abbrev A] [--lang L]";

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

/// A SWORD library folder (it holds `mods.d/`) or a module's `.conf` file.
fn is_sword(path: &Path) -> bool {
    path.join("mods.d").is_dir()
        || (path.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("conf")))
}

/// What the user asked for, before choosing an importer.
#[derive(Default)]
struct Request<'a> {
    inputs: Vec<&'a str>,
    out: Option<String>,
    module: Option<String>,
    resource_id: String,
    title: String,
    abbreviation: String,
    language: String,
    rtl: bool,
}

fn parse_args(args: &[String]) -> Result<Request<'_>, String> {
    let mut r = Request::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| match it.next() {
            Some(v) => Ok(v.clone()),
            None => Err(format!("`{name}` needs a value")),
        };
        match a.as_str() {
            "-o" | "--out" => r.out = Some(value(a)?),
            "--module" => r.module = Some(value(a)?),
            "--id" => r.resource_id = value(a)?,
            "--title" => r.title = value(a)?,
            "--abbrev" => r.abbreviation = value(a)?,
            "--lang" => r.language = value(a)?,
            "--rtl" => r.rtl = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option `{flag}`")),
            input => r.inputs.push(input),
        }
    }
    Ok(r)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn tool_version() -> String {
    format!("obs-cli {}", env!("CARGO_PKG_VERSION"))
}

pub fn import(args: &[String]) -> ExitCode {
    let r = match parse_args(args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("obs-cli import: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (Some(out), false) = (r.out.as_deref(), r.inputs.is_empty()) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let sword = match r.inputs.as_slice() {
        [one] => is_sword(Path::new(one)),
        _ => false,
    };
    let result = if sword {
        if r.rtl {
            eprintln!(
                "obs-cli import: `--rtl` is for USFM; a SWORD module's direction comes from its conf"
            );
            return ExitCode::from(2);
        }
        import_sword_module(&r)
    } else if r.module.is_some() {
        eprintln!(
            "obs-cli import: `--module` needs a SWORD library folder (with mods.d/) or a .conf file\n{USAGE}"
        );
        return ExitCode::from(2);
    } else {
        import_usfm(&r)
    };
    let (resource, report) = match result {
        Ok(done) => done,
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

fn import_sword_module(r: &Request<'_>) -> Result<(Resource, ImportReport), String> {
    let path = Path::new(r.inputs[0]);
    let source = SwordSource::open(path).map_err(|e| e.to_string())?;
    let module = match r.module.clone().or_else(|| source.default_module()) {
        Some(m) => m,
        None => {
            let names: Vec<String> = source
                .modules()
                .into_iter()
                .map(|m| match m.resource_type {
                    Some(t) => format!("{} ({t:?})", m.name),
                    None => format!("{} (not importable)", m.name),
                })
                .collect();
            return Err(format!(
                "{} holds several SWORD modules; choose one with `--module NAME`: {}",
                path.display(),
                names.join(", ")
            ));
        }
    };
    let options = SwordOptions {
        resource_id: r.resource_id.clone(),
        title: r.title.clone(),
        abbreviation: r.abbreviation.clone(),
        language: r.language.clone(),
        source_file: String::new(),
        tool_version: tool_version(),
    };
    import_sword(&source, &module, &options).map_err(|e| e.to_string())
}

fn import_usfm(r: &Request<'_>) -> Result<(Resource, ImportReport), String> {
    let mut options = UsfmOptions {
        resource_id: r.resource_id.clone(),
        title: r.title.clone(),
        abbreviation: r.abbreviation.clone(),
        language: r.language.clone(),
        tool_version: tool_version(),
        ..UsfmOptions::default()
    };
    if r.rtl {
        options.direction = Direction::Rtl;
    }
    if let Some(first) = r.inputs.first() {
        options.source_file = file_name(Path::new(first));
    }
    let mut import = UsfmImport::new(options);
    for input in &r.inputs {
        let files = usfm_files(Path::new(input)).map_err(|e| format!("{input}: {e}"))?;
        for file in files {
            let source =
                std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            import
                .add_book(&file_name(&file), source)
                .map_err(|e: ImportError| e.to_string())?;
        }
    }
    import.finish().map_err(|e| e.to_string())
}
