//! The download cache: files are stored under their sha256 and re-verified on every use.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use super::lock::Source;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// One downloaded, verified file of a source.
pub struct Fetched {
    /// The file name, from the publisher's URL.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Every pinned file of `source`, from the cache or freshly downloaded, after checking each
/// sha256.
pub fn fetch(source: &Source, cache: &Path) -> Result<Vec<Fetched>> {
    let mut out = Vec::new();
    for (i, (url, sha256)) in source.files().enumerate() {
        let bytes = fetch_file(&source.resource_id, url, sha256, cache)?;
        // The first file may be an archived copy; name it after the publisher's URL.
        let named = match (i, &source.origin_url) {
            (0, Some(origin)) => origin.as_str(),
            _ => url,
        };
        out.push(Fetched {
            name: file_name(named),
            bytes,
        });
    }
    Ok(out)
}

/// The last path segment of a URL, percent-decoded where it is plain ASCII.
fn file_name(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or(url);
    let mut out = Vec::with_capacity(last.len());
    let b = last.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && let (Some(Some(h)), Some(Some(l))) = (b.get(i + 1).map(|&c| hex(c)), b.get(i + 2).map(|&c| hex(c)))
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

/// The bytes of one pinned file, from the cache or freshly downloaded, after checking the
/// sha256.
fn fetch_file(resource_id: &str, url: &str, sha256: &str, cache: &Path) -> Result<Vec<u8>> {
    std::fs::create_dir_all(cache)
        .with_context(|| format!("cannot create the cache {}", cache.display()))?;
    let cached = cache.join(sha256);
    if let Ok(bytes) = std::fs::read(&cached) {
        if sha256_hex(&bytes) == sha256 {
            return Ok(bytes);
        }
        // A corrupt or tampered cache entry: discard it and download again.
        std::fs::remove_file(&cached)
            .with_context(|| format!("cannot remove {}", cached.display()))?;
    }
    let partial = cache.join(format!("{sha256}.partial"));
    download(url, &partial)
        .with_context(|| format!("{resource_id}: download of {url} failed"))?;
    let bytes =
        std::fs::read(&partial).with_context(|| format!("cannot read {}", partial.display()))?;
    let actual = sha256_hex(&bytes);
    if actual != sha256 {
        let _ = std::fs::remove_file(&partial);
        bail!(
            "{resource_id}: sha256 mismatch for {url}\n  expected {sha256}\n  got      {actual}\n\
             The file changed upstream or was tampered with. Check it, then update the lock."
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
