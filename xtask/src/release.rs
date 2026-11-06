//! `cargo xtask release-archive` and `cargo xtask release-smoke` (#184): the Release archives
//! the `release` workflow publishes, and the check that one launches with its Starter library.
//!
//! A Release archive is a plain `.tar.gz` (Linux, macOS) or `.zip` (Windows) holding one
//! folder, `NAME/`, with the app binary, the licence files and `starter/`: the whole of
//! `dist/starter/`, beside the executable, where the app finds a bundled Starter library
//! (`obs_engine::starter::bundle_candidates`, #131). Archives are unsigned.
//! TODO(#1): signing, notarization and installers wait for the packaging decisions in #1.

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use obs_engine::starter::{MANIFEST_FILE, StarterManifest, sha256_hex};

/// The app binary's name, without `.exe`.
pub const APP_BIN: &str = "openbible";
/// The folder in a Release archive, beside the binary, that holds the Starter library.
pub const STARTER_DIR: &str = "starter";
/// Licence files copied from the workspace root into every archive.
const LICENSE_FILES: &[&str] = &["LICENSE-APACHE", "LICENSE-MIT"];
/// How long `release-smoke` lets the app run before it counts as hung.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(300);

const ARCHIVE_USAGE: &str = "usage: cargo xtask release-archive --bin FILE --name NAME [options]

Packs the app and the built Starter library into a Release archive, OUT/NAME.tar.gz or
OUT/NAME.zip, holding NAME/<binary>, NAME/starter/ and the licence files. Fails unless the
Starter library's manifest lists only .obr files that are present with the right sha256.

  --bin FILE      the built app (target/release/openbible or openbible.exe)
  --name NAME     archive and top-level folder name, e.g. openbible-v0.2.0-x86_64-unknown-linux-gnu
  --starter DIR   the Starter library build (default dist/starter)
  --format FMT    tar.gz or zip (default: zip when FILE ends in .exe, else tar.gz)
  --out DIR       where to write the archive (default dist/release)";

const SMOKE_USAGE: &str = "usage: cargo xtask release-smoke ARCHIVE [--shot FILE]

Unpacks a Release archive into target/release-smoke/, launches its app on an empty data
folder (its own Library, User database and caches; no Starter download source), lets it draw
and close, and fails unless every Starter Resource in the archive's manifest was added to the
Library byte for byte, with its prebuilt Search index caches. Needs a display (CI: xvfb-run).

  --shot FILE   keep the app's screenshot (a .ppm) here";

/// Archive formats: `.tar.gz` keeps the binary's executable bit; `.zip` is what Windows opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    TarGz,
    Zip,
}

impl Format {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "tar.gz" => Ok(Self::TarGz),
            "zip" => Ok(Self::Zip),
            other => bail!("unknown archive format `{other}` (tar.gz or zip)"),
        }
    }

    fn for_bin(bin: &Path) -> Self {
        if bin
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            Self::Zip
        } else {
            Self::TarGz
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
        }
    }
}

/// What goes into one Release archive.
#[derive(Debug, Clone)]
pub struct ArchiveSpec {
    pub bin: PathBuf,
    pub name: String,
    pub starter: PathBuf,
    pub format: Format,
    pub out: PathBuf,
    /// Where the licence files are read from (the workspace root).
    pub root: PathBuf,
}

/// One file in an archive: its path under `NAME/` (always `/`-separated), where it is read
/// from, and its Unix mode.
struct Entry {
    path: String,
    source: PathBuf,
    mode: u32,
}

pub fn run_archive(root: &Path, args: &[String]) -> Result<()> {
    let mut bin = None;
    let mut name = None;
    let mut format = None;
    let mut starter = root.join("dist/starter");
    let mut out = root.join("dist/release");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| anyhow!("`{a}` needs a value\n\n{ARCHIVE_USAGE}"))
        };
        match a.as_str() {
            "--bin" => bin = Some(PathBuf::from(value()?)),
            "--name" => name = Some(value()?),
            "--starter" => starter = PathBuf::from(value()?),
            "--format" => format = Some(Format::parse(&value()?)?),
            "--out" => out = PathBuf::from(value()?),
            "-h" | "--help" => {
                println!("{ARCHIVE_USAGE}");
                return Ok(());
            }
            other => bail!("unknown option `{other}`\n\n{ARCHIVE_USAGE}"),
        }
    }
    let bin = bin.ok_or_else(|| anyhow!("`--bin` is required\n\n{ARCHIVE_USAGE}"))?;
    let name = name.ok_or_else(|| anyhow!("`--name` is required\n\n{ARCHIVE_USAGE}"))?;
    let format = format.unwrap_or_else(|| Format::for_bin(&bin));
    let path = release_archive(&ArchiveSpec {
        bin,
        name,
        starter,
        format,
        out,
        root: root.to_path_buf(),
    })?;
    println!("{}", path.display());
    Ok(())
}

/// Write the Release archive `spec` describes and return its path.
pub fn release_archive(spec: &ArchiveSpec) -> Result<PathBuf> {
    ensure!(
        !spec.name.is_empty()
            && spec
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            && !spec.name.starts_with('.'),
        "archive name `{}` must be letters, digits, `-`, `_` and `.`",
        spec.name
    );
    ensure!(
        spec.bin.is_file(),
        "{}: no such app binary",
        spec.bin.display()
    );
    check_starter_build(&spec.starter)?;
    let bin_name = spec
        .bin
        .file_name()
        .context("the app binary has no file name")?
        .to_string_lossy()
        .into_owned();
    let mut entries = vec![Entry {
        path: bin_name,
        source: spec.bin.clone(),
        // Artifacts lose the executable bit between jobs; the archive sets it.
        mode: 0o755,
    }];
    for license in LICENSE_FILES {
        entries.push(Entry {
            path: (*license).to_string(),
            source: spec.root.join(license),
            mode: 0o644,
        });
    }
    for e in walkdir::WalkDir::new(&spec.starter).sort_by_file_name() {
        let e = e.with_context(|| format!("reading {}", spec.starter.display()))?;
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e
            .path()
            .strip_prefix(&spec.starter)
            .expect("walkdir yields paths under its root");
        let rel: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        entries.push(Entry {
            path: format!("{STARTER_DIR}/{}", rel.join("/")),
            source: e.path().to_path_buf(),
            mode: 0o644,
        });
    }
    fs::create_dir_all(&spec.out).with_context(|| format!("{}", spec.out.display()))?;
    let path = spec
        .out
        .join(format!("{}.{}", spec.name, spec.format.extension()));
    let part = path.with_extension("part");
    let written = match spec.format {
        Format::TarGz => write_tar_gz(&part, &spec.name, &entries),
        Format::Zip => write_zip(&part, &spec.name, &entries),
    };
    if let Err(e) = written {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    fs::rename(&part, &path).with_context(|| format!("{}", path.display()))?;
    Ok(path)
}

/// The Starter library build has a manifest, and every `.obr` it lists is there with the
/// sha256 it names: what the app checks before adding them, checked before shipping.
fn check_starter_build(starter: &Path) -> Result<StarterManifest> {
    let manifest_path = starter.join(MANIFEST_FILE);
    let json = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "{}: no Starter library build (run `cargo xtask starter`)",
            manifest_path.display()
        )
    })?;
    let manifest =
        StarterManifest::parse(&json).map_err(|e| anyhow!("{}: {e}", manifest_path.display()))?;
    ensure!(
        !manifest.resources.is_empty(),
        "{}: lists no Starter Resources",
        manifest_path.display()
    );
    for item in &manifest.resources {
        let obr = starter.join(format!("{}.obr", item.resource_id));
        let got = file_sha256(&obr)?;
        ensure!(
            got.eq_ignore_ascii_case(&item.file_sha256),
            "{}: sha256 {got}, but {MANIFEST_FILE} says {}",
            obr.display(),
            item.file_sha256
        );
    }
    Ok(manifest)
}

fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("{}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

fn write_tar_gz(path: &Path, name: &str, entries: &[Entry]) -> Result<()> {
    let file = File::create(path).with_context(|| format!("{}", path.display()))?;
    let gz =
        flate2::write::GzEncoder::new(io::BufWriter::new(file), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    let mtime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for entry in entries {
        let source =
            File::open(&entry.source).with_context(|| format!("{}", entry.source.display()))?;
        let len = source.metadata()?.len();
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(len);
        header.set_mode(entry.mode);
        header.set_mtime(mtime);
        header.set_cksum();
        tar.append_data(
            &mut header,
            format!("{name}/{}", entry.path),
            BufReader::new(source),
        )
        .with_context(|| format!("adding {}", entry.path))?;
    }
    let gz = tar.into_inner()?;
    let mut out = gz.finish()?;
    io::Write::flush(&mut out)?;
    Ok(())
}

fn write_zip(path: &Path, name: &str, entries: &[Entry]) -> Result<()> {
    use zip::write::SimpleFileOptions;
    let file = File::create(path).with_context(|| format!("{}", path.display()))?;
    let mut zip = zip::ZipWriter::new(io::BufWriter::new(file));
    for entry in entries {
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(entry.mode)
            .large_file(true);
        zip.start_file(format!("{name}/{}", entry.path), options)?;
        let mut source =
            File::open(&entry.source).with_context(|| format!("{}", entry.source.display()))?;
        io::copy(&mut source, &mut zip).with_context(|| format!("adding {}", entry.path))?;
    }
    let mut out = zip.finish()?;
    io::Write::flush(&mut out)?;
    Ok(())
}

/// Unpack a Release archive into `dir` (emptied first) and return its top-level folder.
pub fn unpack(archive: &Path, dir: &Path) -> Result<PathBuf> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("{}", dir.display()))?;
    }
    fs::create_dir_all(dir).with_context(|| format!("{}", dir.display()))?;
    let file = File::open(archive).with_context(|| format!("{}", archive.display()))?;
    let name = archive.to_string_lossy();
    if name.ends_with(".tar.gz") {
        tar::Archive::new(flate2::read::GzDecoder::new(BufReader::new(file)))
            .unpack(dir)
            .with_context(|| format!("unpacking {}", archive.display()))?;
    } else if name.ends_with(".zip") {
        zip::ZipArchive::new(BufReader::new(file))?
            .extract(dir)
            .with_context(|| format!("unpacking {}", archive.display()))?;
    } else {
        bail!("{}: not a .tar.gz or .zip", archive.display());
    }
    let mut tops: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    ensure!(
        tops.len() == 1 && tops[0].is_dir(),
        "{}: a Release archive holds exactly one folder",
        archive.display()
    );
    Ok(tops.remove(0))
}

/// The app binary in an unpacked Release archive.
pub fn app_in(folder: &Path) -> Result<PathBuf> {
    [APP_BIN.to_string(), format!("{APP_BIN}.exe")]
        .iter()
        .map(|n| folder.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow!("{}: no {APP_BIN} binary", folder.display()))
}

pub fn run_smoke(root: &Path, args: &[String]) -> Result<()> {
    let mut archive = None;
    let mut shot = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--shot" => {
                shot = Some(PathBuf::from(it.next().ok_or_else(|| {
                    anyhow!("`--shot` needs a value\n\n{SMOKE_USAGE}")
                })?));
            }
            "-h" | "--help" => {
                println!("{SMOKE_USAGE}");
                return Ok(());
            }
            other if !other.starts_with('-') && archive.is_none() => {
                archive = Some(PathBuf::from(other));
            }
            other => bail!("unknown option `{other}`\n\n{SMOKE_USAGE}"),
        }
    }
    let archive = archive.ok_or_else(|| anyhow!("no archive given\n\n{SMOKE_USAGE}"))?;
    let work = root.join("target/release-smoke");
    let folder = unpack(&archive, &work.join("unpacked"))?;
    let app = app_in(&folder)?;
    let data = work.join("data");
    let shot_file = data.join("shot.ppm");
    launch(&app, &data, &shot_file)?;
    let (w, h) = ppm_size(&shot_file)?;
    let report = check_library(&folder.join(STARTER_DIR), &data.join("library"))?;
    if let Some(keep) = shot {
        fs::copy(&shot_file, &keep).with_context(|| format!("{}", keep.display()))?;
    }
    eprintln!(
        "release-smoke: {} launched, drew a {w}×{h} frame and closed; {report}",
        app.display()
    );
    Ok(())
}

/// Run the unpacked app on a fresh data folder until it has drawn a frame, saved it to
/// `shot` and closed (`OBS_SHOT`). Everything it may write goes under `data`, and the
/// Starter download source is a folder that does not exist, so the Library can only come
/// from the bundle beside the binary.
fn launch(app: &Path, data: &Path, shot: &Path) -> Result<()> {
    if data.exists() {
        fs::remove_dir_all(data).with_context(|| format!("{}", data.display()))?;
    }
    let home = data.join("home");
    fs::create_dir_all(&home)?;
    let mut cmd = Command::new(app);
    cmd.current_dir(data)
        .env("OBS_LIBRARY", data.join("library"))
        .env("OBS_USER_DB", data.join("user.db"))
        .env("OBS_SEARCH_CACHE", data.join("search-cache"))
        .env("OBS_ALIAS_CACHE", data.join("alias-cache"))
        .env("OBS_SWORD_CATALOG", data.join("sword-catalog.json"))
        .env("OBS_SWORD_DIR", data.join("sword"))
        .env("OBS_STARTER_URL", "file:///nonexistent/obs-release-smoke/")
        .env("OBS_SHOT", shot)
        .env("OBS_SHOT_FRAME", "60")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env_remove("OBS_STARTER_DIR")
        .env_remove("OBS_SCRIPT")
        .env_remove("OBS_INPUT");
    let mut child = cmd
        .spawn()
        .with_context(|| format!("could not launch {}", app.display()))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > SMOKE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "{} did not close within {}s",
                app.display(),
                SMOKE_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    ensure!(status.success(), "{} exited with {status}", app.display());
    ensure!(
        shot.is_file(),
        "{} closed without drawing a frame (no {})",
        app.display(),
        shot.display()
    );
    Ok(())
}

/// Width and height of a binary PPM (`P6 W H 255\n…`), the format `OBS_SHOT` writes.
fn ppm_size(path: &Path) -> Result<(u32, u32)> {
    let mut head = [0u8; 64];
    let n = File::open(path)
        .with_context(|| format!("{}", path.display()))?
        .read(&mut head)?;
    let text = String::from_utf8_lossy(&head[..n]);
    let mut fields = text.split_ascii_whitespace();
    ensure!(fields.next() == Some("P6"), "{}: not a PPM", path.display());
    let mut dim = || -> Result<u32> {
        fields
            .next()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("{}: empty screenshot", path.display()))
    };
    Ok((dim()?, dim()?))
}

/// Every Starter Resource the bundle's manifest lists is in `library` byte for byte, with
/// the prebuilt Search index caches the bundle has for it. Returns a one-line summary.
pub fn check_library(bundle: &Path, library: &Path) -> Result<String> {
    let manifest = check_starter_build(bundle)?;
    let mut caches = 0;
    for item in &manifest.resources {
        let id = &item.resource_id;
        let obr = library.join(format!("{id}.obr"));
        ensure!(
            obr.is_file(),
            "the Starter Resource {id} is not in the Library ({} is missing)",
            obr.display()
        );
        let got = file_sha256(&obr)?;
        ensure!(
            got.eq_ignore_ascii_case(&item.file_sha256),
            "{}: sha256 {got}, but {MANIFEST_FILE} says {}",
            obr.display(),
            item.file_sha256
        );
        for cache in obs_store::search_index_files(&bundle.join("search"), id) {
            let name = cache.file_name().expect("a cache file has a name");
            ensure!(
                library.join("search").join(name).is_file(),
                "the prebuilt Search index cache {} of {id} was not put in the Library",
                name.to_string_lossy()
            );
            caches += 1;
        }
    }
    Ok(format!(
        "{} Starter Resources and {caches} prebuilt Search index caches in the Library",
        manifest.resources.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Starter library build with two (fake) Resources, a Search index cache and a change
    /// record, laid out as `cargo xtask starter` writes it.
    fn fake_starter(dir: &Path) -> Vec<(&'static str, Vec<u8>)> {
        let files = vec![
            ("kjv", b"kjv bytes".to_vec()),
            ("bdb", b"bdb bytes".to_vec()),
        ];
        fs::create_dir_all(dir.join("search")).unwrap();
        let mut resources = Vec::new();
        for (id, bytes) in &files {
            fs::write(dir.join(format!("{id}.obr")), bytes).unwrap();
            resources.push(serde_json::json!({
                "resource_id": id,
                "revision": 1,
                "content_hash": "c",
                "file_sha256": sha256_hex(bytes),
                "bytes": bytes.len(),
            }));
        }
        fs::write(dir.join("search").join(search_cache_name("kjv")), b"index").unwrap();
        fs::write(dir.join("CHANGES-kjv.md"), b"# Changes").unwrap();
        fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::json!({ "format": 1, "resources": resources }).to_string(),
        )
        .unwrap();
        files
    }

    /// A name `obs_store::search_index_files` recognises as `id`'s cache.
    fn search_cache_name(id: &str) -> String {
        format!(
            "{id}.0123456789abcdef.v1.{}",
            obs_store::SEARCH_INDEX_EXTENSION
        )
    }

    fn spec(tmp: &Path, format: Format, bin_name: &str) -> ArchiveSpec {
        let bin = tmp.join(bin_name);
        fs::write(&bin, b"\x7fELF app").unwrap();
        fake_starter(&tmp.join("dist/starter"));
        for license in LICENSE_FILES {
            fs::write(tmp.join(license), license.as_bytes()).unwrap();
        }
        ArchiveSpec {
            bin,
            name: "openbible-test-target".into(),
            starter: tmp.join("dist/starter"),
            format,
            out: tmp.join("dist/release"),
            root: tmp.to_path_buf(),
        }
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("obs-xtask-release-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Unpacked, the archive has the app with the Starter library beside it, where the app
    /// looks for a bundle (#131), and the licence files.
    fn assert_layout(folder: &Path, bin_name: &str) {
        assert_eq!(folder.file_name().unwrap(), "openbible-test-target");
        let app = app_in(folder).unwrap();
        assert_eq!(app, folder.join(bin_name));
        assert_eq!(fs::read(&app).unwrap(), b"\x7fELF app");
        for license in LICENSE_FILES {
            assert!(folder.join(license).is_file(), "{license}");
        }
        let starter = folder.join(STARTER_DIR);
        for f in ["kjv.obr", "bdb.obr", MANIFEST_FILE, "CHANGES-kjv.md"] {
            assert!(starter.join(f).is_file(), "{f}");
        }
        assert!(
            starter
                .join("search")
                .join(search_cache_name("kjv"))
                .is_file()
        );
        let found = obs_engine::starter::bundle_candidates(None, Some(&app), None)
            .into_iter()
            .find(|d| d.join(MANIFEST_FILE).is_file());
        assert_eq!(found, Some(starter));
    }

    #[test]
    fn tar_gz_archive_holds_the_app_beside_the_starter_library() {
        let tmp = tempdir("tar");
        let spec = spec(&tmp, Format::TarGz, "openbible");
        let path = release_archive(&spec).unwrap();
        assert_eq!(path, tmp.join("dist/release/openbible-test-target.tar.gz"));
        let folder = unpack(&path, &tmp.join("unpacked")).unwrap();
        assert_layout(&folder, "openbible");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(folder.join("openbible"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "the app is executable once unpacked");
        }
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn zip_archive_holds_the_app_beside_the_starter_library() {
        let tmp = tempdir("zip");
        let mut spec = spec(&tmp, Format::Zip, "openbible.exe");
        spec.format = Format::for_bin(&spec.bin);
        assert_eq!(spec.format, Format::Zip);
        let path = release_archive(&spec).unwrap();
        assert_eq!(path, tmp.join("dist/release/openbible-test-target.zip"));
        let folder = unpack(&path, &tmp.join("unpacked")).unwrap();
        assert_layout(&folder, "openbible.exe");
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn archive_refuses_a_starter_build_that_does_not_match_its_manifest() {
        let tmp = tempdir("mismatch");
        let spec = spec(&tmp, Format::TarGz, "openbible");
        fs::write(spec.starter.join("bdb.obr"), b"tampered").unwrap();
        let err = release_archive(&spec).unwrap_err().to_string();
        assert!(err.contains("bdb.obr") && err.contains("sha256"), "{err}");
        fs::remove_file(spec.starter.join("bdb.obr")).unwrap();
        let err = format!("{:#}", release_archive(&spec).unwrap_err());
        assert!(err.contains("bdb.obr"), "{err}");
        assert!(!spec.out.join("openbible-test-target.tar.gz").exists());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn archive_refuses_a_missing_starter_build() {
        let tmp = tempdir("missing");
        let mut spec = spec(&tmp, Format::TarGz, "openbible");
        spec.starter = tmp.join("nowhere");
        let err = release_archive(&spec).unwrap_err().to_string();
        assert!(err.contains("cargo xtask starter"), "{err}");
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn smoke_check_needs_every_starter_resource_and_cache_in_the_library() {
        let tmp = tempdir("library");
        let bundle = tmp.join("starter");
        let files = fake_starter(&bundle);
        let lib = tmp.join("library");
        fs::create_dir_all(lib.join("search")).unwrap();
        let err = check_library(&bundle, &lib).unwrap_err().to_string();
        assert!(err.contains("kjv is not in the Library"), "{err}");
        for (id, bytes) in &files {
            fs::write(lib.join(format!("{id}.obr")), bytes).unwrap();
        }
        let err = check_library(&bundle, &lib).unwrap_err().to_string();
        assert!(err.contains("Search index cache"), "{err}");
        fs::write(lib.join("search").join(search_cache_name("kjv")), b"index").unwrap();
        assert_eq!(
            check_library(&bundle, &lib).unwrap(),
            "2 Starter Resources and 1 prebuilt Search index caches in the Library"
        );
        fs::write(lib.join("bdb.obr"), b"other").unwrap();
        let err = check_library(&bundle, &lib).unwrap_err().to_string();
        assert!(err.contains("bdb.obr") && err.contains("sha256"), "{err}");
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn ppm_size_reads_the_screenshot_header() {
        let tmp = tempdir("ppm");
        let shot = tmp.join("shot.ppm");
        fs::write(&shot, b"P6 1280 800 255\n\x00\x00\x00").unwrap();
        assert_eq!(ppm_size(&shot).unwrap(), (1280, 800));
        fs::write(&shot, b"P6 0 0 255\n").unwrap();
        assert!(ppm_size(&shot).is_err());
        fs::remove_dir_all(&tmp).unwrap();
    }
}
