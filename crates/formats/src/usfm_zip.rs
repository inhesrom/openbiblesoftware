//! A publisher's zip of USFM books (eBible.org's `<id>_usfm.zip`), as the Open catalogue
//! downloads it (#134): the books, and any other text file in it (its licence notice).

use std::io::{Cursor, Read};

use crate::ImportError;

/// Largest file read out of a zip. The largest real book is about 1 MiB (Jeremiah in
/// eBible.org's `engwebp_usfm.zip`), so this is far beyond any book and still a bounded
/// allocation for a hostile one (#253).
pub const ZIP_ENTRY_LIMIT: u64 = 32 * 1024 * 1024;

/// Most bytes decompressed out of one zip in total. Whole real Bibles are 5 to 19 MiB.
pub const ZIP_TOTAL_LIMIT: u64 = 256 * 1024 * 1024;

/// The USFM books in a zip, as (file name without folders, text), sorted by their path in
/// the zip. Files named `*.usfm` or `*.sfm` count; everything else is left out. A file that
/// expands past [`ZIP_ENTRY_LIMIT`], or books past [`ZIP_TOTAL_LIMIT`] together, is an
/// archive error.
pub fn usfm_zip_books(bytes: &[u8]) -> Result<Vec<(String, String)>, ImportError> {
    books_within(bytes, ZipLimits::DEFAULT)
}

/// The text of the file `name` in a zip (matched without folders, ignoring case), or `None`
/// when the zip has no such file. A file that expands past [`ZIP_ENTRY_LIMIT`] is an
/// archive error.
pub fn zip_text_file(bytes: &[u8], name: &str) -> Result<Option<String>, ImportError> {
    text_file_within(bytes, name, ZipLimits::DEFAULT)
}

/// Decompressed-size bounds for one zip: per file, and for every file read together.
#[derive(Clone, Copy, Debug)]
struct ZipLimits {
    entry: u64,
    total: u64,
}

impl ZipLimits {
    const DEFAULT: ZipLimits = ZipLimits {
        entry: ZIP_ENTRY_LIMIT,
        total: ZIP_TOTAL_LIMIT,
    };
}

fn books_within(bytes: &[u8], limits: ZipLimits) -> Result<Vec<(String, String)>, ImportError> {
    let mut zip = open(bytes)?;
    let mut names: Vec<(String, usize)> = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(archive)?;
        let name = entry.name().to_string();
        let lower = name.to_ascii_lowercase();
        if entry.is_file() && (lower.ends_with(".usfm") || lower.ends_with(".sfm")) {
            names.push((name, i));
        }
    }
    names.sort();
    let mut books = Vec::with_capacity(names.len());
    let mut used = 0;
    for (name, i) in names {
        let text = read_text(&mut zip, i, &name, limits, &mut used)?;
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        books.push((base, text));
    }
    Ok(books)
}

fn text_file_within(
    bytes: &[u8],
    name: &str,
    limits: ZipLimits,
) -> Result<Option<String>, ImportError> {
    let mut zip = open(bytes)?;
    for i in 0..zip.len() {
        let entry_name = zip.by_index(i).map_err(archive)?.name().to_string();
        let base = entry_name.rsplit('/').next().unwrap_or(&entry_name);
        if base.eq_ignore_ascii_case(name) {
            return read_text(&mut zip, i, &entry_name, limits, &mut 0).map(Some);
        }
    }
    Ok(None)
}

fn open(bytes: &[u8]) -> Result<zip::ZipArchive<Cursor<&[u8]>>, ImportError> {
    zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ImportError::Archive(format!("not a zip archive: {e}")))
}

/// Entry `i` as text. It decompresses at most `limits.entry` bytes and at most what `used`
/// leaves of `limits.total`, then adds what it read to `used`. Sizes the zip declares are not
/// trusted: the reader stops one byte past the bound.
fn read_text(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    i: usize,
    name: &str,
    limits: ZipLimits,
    used: &mut u64,
) -> Result<String, ImportError> {
    let left = limits.total.saturating_sub(*used);
    let limit = limits.entry.min(left);
    let mut raw = Vec::new();
    zip.by_index(i)
        .map_err(archive)?
        .take(limit + 1)
        .read_to_end(&mut raw)
        .map_err(|e| ImportError::Archive(format!("cannot read {name}: {e}")))?;
    let read = raw.len() as u64;
    if read > limit {
        return Err(ImportError::Archive(if read > limits.entry {
            format!("{name} is larger than {} decompressed", mib(limits.entry))
        } else {
            format!(
                "{name} takes the zip past {} decompressed",
                mib(limits.total)
            )
        }));
    }
    *used += read;
    if raw.starts_with(b"\xEF\xBB\xBF") {
        raw.drain(..3);
    }
    String::from_utf8(raw).map_err(|_| ImportError::Archive(format!("{name} is not UTF-8 text")))
}

fn mib(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MiB", bytes / (1024 * 1024))
    } else {
        format!("{bytes} bytes")
    }
}

fn archive(e: zip::result::ZipError) -> ImportError {
    ImportError::Archive(e.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::io::Write;

    use super::*;

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn books_are_the_usfm_files_in_path_order_without_folders() {
        let bytes = zip_of(&[
            ("x/03-MRKtest.usfm", "\\id MRK\n"),
            ("copr.htm", "<p>Public Domain</p>"),
            ("x/02-MATtest.SFM", "\u{feff}\\id MAT\n"),
        ]);
        let books = usfm_zip_books(&bytes).unwrap();
        let names: Vec<&str> = books.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["02-MATtest.SFM", "03-MRKtest.usfm"]);
        // A byte-order mark is dropped.
        assert_eq!(books[0].1, "\\id MAT\n");
    }

    #[test]
    fn a_named_text_file_is_found_in_any_folder() {
        let bytes = zip_of(&[("a/COPR.htm", "<p>Public Domain</p>")]);
        assert_eq!(
            zip_text_file(&bytes, "copr.htm").unwrap().as_deref(),
            Some("<p>Public Domain</p>")
        );
        assert_eq!(zip_text_file(&bytes, "license.txt").unwrap(), None);
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_an_archive_error() {
        let err = usfm_zip_books(b"<html>Not found</html>").unwrap_err();
        assert!(matches!(err, ImportError::Archive(_)), "{err}");
        assert!(err.to_string().starts_with("archive: not a zip archive"));
    }

    /// AUD-FORMATS-8 (#253): a small zip that expands one book past the per-file bound is
    /// an archive error naming the file, not a 64 MiB string.
    #[test]
    fn a_book_that_expands_past_the_entry_limit_is_an_archive_error() {
        let bomb = " ".repeat(64 * 1024 * 1024);
        let bytes = zip_of(&[("GEN.usfm", &bomb)]);
        drop(bomb);
        assert!(bytes.len() < 1024 * 1024, "{}", bytes.len());
        let err = usfm_zip_books(&bytes).unwrap_err();
        assert!(matches!(err, ImportError::Archive(_)), "{err}");
        assert_eq!(
            err.to_string(),
            "archive: GEN.usfm is larger than 32 MiB decompressed"
        );
        let err = zip_text_file(&bytes, "gen.usfm").unwrap_err();
        assert!(err.to_string().contains("larger than 32 MiB"), "{err}");
    }

    #[test]
    fn books_that_together_pass_the_total_limit_are_an_archive_error() {
        let limits = ZipLimits {
            entry: 100,
            total: 250,
        };
        let book = "x".repeat(100);
        let bytes = zip_of(&[("A.usfm", &book), ("B.usfm", &book)]);
        assert_eq!(books_within(&bytes, limits).unwrap().len(), 2);
        let bytes = zip_of(&[("A.usfm", &book), ("B.usfm", &book), ("C.usfm", &book)]);
        let err = books_within(&bytes, limits).unwrap_err();
        assert_eq!(
            err.to_string(),
            "archive: C.usfm takes the zip past 250 bytes decompressed"
        );
        let err = books_within(&zip_of(&[("A.usfm", &"x".repeat(101))]), limits).unwrap_err();
        assert_eq!(
            err.to_string(),
            "archive: A.usfm is larger than 100 bytes decompressed"
        );
        assert!(
            text_file_within(&bytes, "c.usfm", limits)
                .unwrap()
                .is_some()
        );
    }
}
