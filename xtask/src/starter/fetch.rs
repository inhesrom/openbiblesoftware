//! The download cache: files are stored under their sha256 and re-verified on every use.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use super::lock::{PinnedFile, Source};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// The bytes of `source`'s main file, then of each extra file, from the cache or freshly
/// downloaded, after checking each sha256.
pub fn fetch(source: &Source, cache: &Path) -> Result<Vec<Vec<u8>>> {
    let main = PinnedFile {
        url: source.url.clone(),
        sha256: source.sha256.clone(),
    };
    std::iter::once(&main)
        .chain(&source.extra_files)
        .map(|file| fetch_file(&source.resource_id, file, cache))
        .collect()
}

fn fetch_file(resource_id: &str, file: &PinnedFile, cache: &Path) -> Result<Vec<u8>> {
    std::fs::create_dir_all(cache)
        .with_context(|| format!("cannot create the cache {}", cache.display()))?;
    let cached = cache.join(&file.sha256);
    if let Ok(bytes) = std::fs::read(&cached) {
        if sha256_hex(&bytes) == file.sha256 {
            return Ok(bytes);
        }
        // A corrupt or tampered cache entry: discard it and download again.
        std::fs::remove_file(&cached)
            .with_context(|| format!("cannot remove {}", cached.display()))?;
    }
    let partial = cache.join(format!("{}.partial", file.sha256));
    download(&file.url, &partial)
        .with_context(|| format!("{resource_id}: download of {} failed", file.url))?;
    let bytes =
        std::fs::read(&partial).with_context(|| format!("cannot read {}", partial.display()))?;
    let actual = sha256_hex(&bytes);
    if actual != file.sha256 {
        let _ = std::fs::remove_file(&partial);
        bail!(
            "{resource_id}: sha256 mismatch for {}\n  expected {}\n  got      {actual}\n\
             The file changed upstream or was tampered with. Check it, then update the lock.",
            file.url,
            file.sha256
        );
    }
    std::fs::rename(&partial, &cached)
        .with_context(|| format!("cannot move the download to {}", cached.display()))?;
    Ok(bytes)
}

fn download(url: &str, to: &Path) -> Result<()> {
    if let Some(path) = url.strip_prefix("file://") {
        let from = PathBuf::from(path);
        std::fs::copy(&from, to).with_context(|| format!("cannot copy {}", from.display()))?;
        return Ok(());
    }
    let status = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args([
            "--retry",
            "3",
            "--retry-delay",
            "5",
            "--connect-timeout",
            "30",
        ])
        .arg("--output")
        .arg(to)
        .arg(url)
        .status()
        .context("could not run `curl`; it is needed to download Starter sources")?;
    if !status.success() {
        let _ = std::fs::remove_file(to);
        bail!("curl exited with {status}");
    }
    Ok(())
}

/// The last path segment of a URL, percent-decoded where it is plain ASCII.
pub fn file_name(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or(url);
    let mut out = Vec::with_capacity(last.len());
    let b = last.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && let (Some(Some(h)), Some(Some(l))) =
                (b.get(i + 1).map(|&c| hex(c)), b.get(i + 2).map(|&c| hex(c)))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| last.to_string())
}
