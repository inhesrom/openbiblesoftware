//! `cargo xtask ios`: the Mobile app's Rust side for iOS (`docs/dev/mobile-core.md`).
//!
//! - `bindgen` builds `obs-ffi` as a host cdylib and runs `apps/uniffi-bindgen` over it in
//!   library mode, writing the Swift bindings to [`BINDINGS`].
//! - `check` is the host-only check: `cargo check -p obs-ffi`, then `bindgen`.
//! - `build` builds the `aarch64-apple-ios` and `aarch64-apple-ios-sim` staticlibs with
//!   `[profile.ios]` and wraps them, with the bindings' header and module map, in
//!   [`XCFRAMEWORK`]. It needs Xcode's iOS SDKs (`rusqlite` compiles SQLite with them) and
//!   says so, without building anything, when they are missing.
//!
//! None of these is a gate of `cargo xtask ci`, which never needs macOS or Xcode.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Where `bindgen` writes the Swift bindings.
pub const BINDINGS: &str = "target/ios/bindings";
/// Where `build` writes the XCFramework.
pub const XCFRAMEWORK: &str = "target/ios/ObsFFI.xcframework";
/// The bindings' header and module map, as the XCFramework's `Headers` wants them.
const HEADERS: &str = "target/ios/headers";
/// The UniFFI crate and its library's file stem.
const FFI_PACKAGE: &str = "obs-ffi";
const FFI_LIB: &str = "obs_ffi";
/// The Swift module `apps/obs-ffi/uniffi.toml` names, and UniFFI's name for its C module.
const SWIFT_MODULE: &str = "ObsFFI";
const C_MODULE: &str = "ObsFFIFFI";

/// One library in the XCFramework: a Rust target and the Xcode SDK its C code builds with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slice {
    target: &'static str,
    sdk: &'static str,
}

const DEVICE: Slice = Slice {
    target: "aarch64-apple-ios",
    sdk: "iphoneos",
};
const SIMULATOR: Slice = Slice {
    target: "aarch64-apple-ios-sim",
    sdk: "iphonesimulator",
};

const USAGE: &str = "usage: cargo xtask ios <command>

  bindgen            build obs-ffi for the host and write its Swift bindings
                     (ObsFFI.swift, ObsFFIFFI.h, ObsFFIFFI.modulemap) to target/ios/bindings
  check              host only: cargo check -p obs-ffi, then bindgen
  build [--sim-only] the aarch64-apple-ios and aarch64-apple-ios-sim staticlibs ([profile.ios])
                     and target/ios/ObsFFI.xcframework; --sim-only: the simulator's alone.
                     Needs Xcode with the iOS SDKs
  bundled-set [--from DIR] [--out DIR] [--update-lock | --verify]
                     copy the Bundled set out of dist/starter/ into apps/ios/Bundled/starter/
                     and check it against apps/ios/bundled-set.lock (`--help`)

docs/dev/mobile-core.md, docs/dev/starter-library.md";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("bindgen") => {
            no_more(&args[1..])?;
            bindgen(root).map(|_| ())
        }
        Some("check") => {
            no_more(&args[1..])?;
            crate::cargo(root, &["check", "--locked", "-p", FFI_PACKAGE])?;
            bindgen(root).map(|_| ())
        }
        Some("build") => {
            let sim_only = build_options(&args[1..])?;
            build(root, sim_only, &xcrun_has_sdk)
        }
        Some("bundled-set") => crate::starter::bundled_set::run(root, &args[1..]),
        None | Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("unknown ios command `{other}`\n\n{USAGE}"),
    }
}

fn no_more(args: &[String]) -> Result<()> {
    match args.first() {
        None => Ok(()),
        Some(a) => bail!("unexpected argument `{a}`\n\n{USAGE}"),
    }
}

/// `build`'s options: whether only the simulator slice is wanted.
fn build_options(args: &[String]) -> Result<bool> {
    let mut sim_only = false;
    for a in args {
        match a.as_str() {
            "--sim-only" => sim_only = true,
            other => bail!("unknown option `{other}`\n\n{USAGE}"),
        }
    }
    Ok(sim_only)
}

fn slices(sim_only: bool) -> &'static [Slice] {
    if sim_only {
        &[SIMULATOR]
    } else {
        &[DEVICE, SIMULATOR]
    }
}

/// Whether Xcode has this SDK: `xcrun --sdk <sdk> --show-sdk-path` succeeds. False where
/// there is no `xcrun` (Linux, Windows, a Mac with only the Command Line Tools).
fn xcrun_has_sdk(sdk: &str) -> bool {
    Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Fails, naming the missing SDKs, unless `has_sdk` finds every one the slices need.
fn require_sdks(slices: &[Slice], has_sdk: &dyn Fn(&str) -> bool) -> Result<()> {
    let missing: Vec<&str> = slices
        .iter()
        .map(|s| s.sdk)
        .filter(|sdk| !has_sdk(sdk))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let probes: Vec<String> = missing
        .iter()
        .map(|sdk| format!("`xcrun --sdk {sdk} --show-sdk-path`"))
        .collect();
    bail!(
        "`cargo xtask ios build` needs Xcode: {} failed, so the iOS SDK is missing. Install \
         Xcode with the iOS platform and select it (`sudo xcode-select -s \
         <path to Xcode.app>`). `cargo xtask ios check` works without it.",
        probes.join(" and ")
    )
}

/// Build `obs-ffi` for the host and write its Swift bindings. Returns the bindings folder.
fn bindgen(root: &Path) -> Result<PathBuf> {
    crate::cargo(root, &["build", "--locked", "-p", FFI_PACKAGE, "--lib"])?;
    let lib = root.join("target/debug").join(format!(
        "{}{FFI_LIB}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let out = root.join(BINDINGS);
    if out.exists() {
        std::fs::remove_dir_all(&out).with_context(|| format!("cannot clear {}", out.display()))?;
    }
    std::fs::create_dir_all(&out).with_context(|| format!("cannot create {}", out.display()))?;
    let lib_arg = lib.to_string_lossy();
    let out_arg = out.to_string_lossy();
    crate::cargo(
        root,
        &[
            "run",
            "--locked",
            "--quiet",
            "-p",
            "uniffi-bindgen",
            "--",
            "generate",
            &lib_arg,
            "--language",
            "swift",
            // Unformatted, so every host writes the same bytes (no swift-format dependence).
            "--no-format",
            "--out-dir",
            &out_arg,
        ],
    )?;
    for file in binding_files() {
        if !out.join(&file).is_file() {
            bail!(
                "uniffi-bindgen wrote no {file} in {BINDINGS} (is `module_name` in \
                 apps/obs-ffi/uniffi.toml still {SWIFT_MODULE}?)"
            );
        }
    }
    eprintln!("ios: wrote {} to {BINDINGS}", binding_files().join(", "));
    Ok(out)
}

fn binding_files() -> [String; 3] {
    [
        format!("{SWIFT_MODULE}.swift"),
        format!("{C_MODULE}.h"),
        format!("{C_MODULE}.modulemap"),
    ]
}

fn build(root: &Path, sim_only: bool, has_sdk: &dyn Fn(&str) -> bool) -> Result<()> {
    let slices = slices(sim_only);
    require_sdks(slices, has_sdk)?;
    let bindings = bindgen(root)?;

    let mut libs = Vec::new();
    for slice in slices {
        crate::cargo(
            root,
            &[
                "rustc",
                "--locked",
                "-p",
                FFI_PACKAGE,
                "--lib",
                "--profile",
                "ios",
                "--target",
                slice.target,
                "--crate-type",
                "staticlib",
            ],
        )?;
        let lib = root
            .join("target")
            .join(slice.target)
            .join("ios")
            .join(format!("lib{FFI_LIB}.a"));
        if !lib.is_file() {
            bail!("cargo built no {}", lib.display());
        }
        libs.push(lib);
    }

    // The XCFramework's Headers: the C header and the module map under the name clang
    // looks for.
    let headers = root.join(HEADERS);
    if headers.exists() {
        std::fs::remove_dir_all(&headers)
            .with_context(|| format!("cannot clear {}", headers.display()))?;
    }
    std::fs::create_dir_all(&headers)
        .with_context(|| format!("cannot create {}", headers.display()))?;
    for (from, to) in [
        (format!("{C_MODULE}.h"), format!("{C_MODULE}.h")),
        (
            format!("{C_MODULE}.modulemap"),
            "module.modulemap".to_string(),
        ),
    ] {
        std::fs::copy(bindings.join(&from), headers.join(&to))
            .with_context(|| format!("cannot copy {from} into {HEADERS}"))?;
    }

    // `-create-xcframework` refuses to write over an existing one.
    let xcframework = root.join(XCFRAMEWORK);
    if xcframework.exists() {
        std::fs::remove_dir_all(&xcframework)
            .with_context(|| format!("cannot clear {}", xcframework.display()))?;
    }
    let mut cmd = Command::new("xcodebuild");
    cmd.arg("-create-xcframework");
    for lib in &libs {
        cmd.arg("-library").arg(lib).arg("-headers").arg(&headers);
    }
    cmd.arg("-output").arg(&xcframework).current_dir(root);
    let status = cmd.status().context("could not run `xcodebuild`")?;
    if !status.success() {
        bail!("`xcodebuild -create-xcframework` exited with {status}");
    }
    for lib in &libs {
        let bytes = std::fs::metadata(lib).map(|m| m.len()).unwrap_or(0);
        eprintln!("ios: {} ({:.1} MB)", lib.display(), bytes as f64 / 1e6);
    }
    eprintln!("ios: wrote {XCFRAMEWORK}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn args(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn build_needs_xcode_when_an_sdk_is_missing() {
        let asked = RefCell::new(Vec::new());
        let no_sdk = |sdk: &str| {
            asked.borrow_mut().push(sdk.to_string());
            false
        };
        let err = require_sdks(slices(false), &no_sdk)
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs Xcode"), "{err}");
        assert!(
            err.contains("`xcrun --sdk iphoneos --show-sdk-path`")
                && err.contains("`xcrun --sdk iphonesimulator --show-sdk-path`"),
            "{err}"
        );
        assert_eq!(*asked.borrow(), ["iphoneos", "iphonesimulator"]);

        // Only the missing one is named.
        let device_only = |sdk: &str| sdk == "iphoneos";
        let err = require_sdks(slices(false), &device_only)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("--sdk iphonesimulator") && !err.contains("--sdk iphoneos "),
            "{err}"
        );
    }

    #[test]
    fn build_goes_on_when_the_sdks_are_there() {
        assert!(require_sdks(slices(false), &|_| true).is_ok());
        // The simulator slice alone does not need the device SDK.
        assert!(require_sdks(slices(true), &|sdk| sdk == "iphonesimulator").is_ok());
    }

    #[test]
    fn sim_only_builds_only_the_simulator_slice() {
        assert_eq!(slices(false), [DEVICE, SIMULATOR]);
        assert_eq!(slices(true), [SIMULATOR]);
        assert!(!build_options(&args(&[])).unwrap());
        assert!(build_options(&args(&["--sim-only"])).unwrap());
        assert!(build_options(&args(&["--device-only"])).is_err());
    }

    #[test]
    fn unknown_commands_and_arguments_fail() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert!(run(root, &args(&["frobnicate"])).is_err());
        assert!(run(root, &args(&["bindgen", "--sim-only"])).is_err());
        assert!(run(root, &args(&["check", "extra"])).is_err());
    }

    /// The bindings' file names come from `apps/obs-ffi/uniffi.toml`.
    #[test]
    fn the_swift_module_is_the_one_uniffi_toml_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let text = std::fs::read_to_string(root.join("apps/obs-ffi/uniffi.toml")).unwrap();
        let config: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            config["bindings"]["swift"]["module_name"].as_str(),
            Some(SWIFT_MODULE)
        );
        assert_eq!(C_MODULE, format!("{SWIFT_MODULE}FFI"), "UniFFI's default");
    }

    /// `[profile.ios]` keeps unwinding: the job queue catches a panicking job, which
    /// `panic = "abort"` would turn into an app crash.
    #[test]
    fn the_ios_profile_unwinds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let manifest: toml::Table = toml::from_str(&text).unwrap();
        let ios = manifest["profile"]["ios"].as_table().unwrap();
        assert_eq!(ios["inherits"].as_str(), Some("release"));
        assert_eq!(ios["opt-level"].as_str(), Some("s"));
        assert!(
            ios.get("panic")
                .is_none_or(|p| p.as_str() == Some("unwind"))
        );
    }
}
