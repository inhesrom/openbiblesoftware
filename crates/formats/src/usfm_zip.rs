//! A publisher's zip of USFM books (eBible.org's `<id>_usfm.zip`), as the Open catalogue
//! downloads it (#134): the books, and any other text file in it (its licence notice).

use std::io::{Cursor, Read};

use crate::ImportError;

/// The USFM books in a zip, as (file name without folders, text), sorted by their path in
/// the zip. Files named `*.usfm` or `*.sfm` count; everything else is left out.
pub fn usfm_zip_books(bytes: &[u8]) -> Result<Vec<(String, String)>, ImportError> {
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
    for (name, i) in names {
        let text = read_text(&mut zip, i, &name)?;
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        books.push((base, text));
    }
    Ok(books)
}

/// The text of the file `name` in a zip (matched without folders, ignoring case), or `None`
/// when the zip has no such file.
pub fn zip_text_file(bytes: &[u8], name: &str) -> Result<Option<String>, ImportError> {
    let mut zip = open(bytes)?;
    for i in 0..zip.len() {
        let entry_name = zip.by_index(i).map_err(archive)?.name().to_string();
        let base = entry_name.rsplit('/').next().unwrap_or(&entry_name);
        if base.eq_ignore_ascii_case(name) {
            return read_text(&mut zip, i, &entry_name).map(Some);
        }
    }
    Ok(None)
}

fn open(bytes: &[u8]) -> Result<zip::ZipArchive<Cursor<&[u8]>>, ImportError> {
    zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ImportError::Archive(format!("not a zip archive: {e}")))
}

fn read_text(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    i: usize,
    name: &str,
) -> Result<String, ImportError> {
    let mut raw = Vec::new();
    zip.by_index(i)
        .map_err(archive)?
        .read_to_end(&mut raw)
        .map_err(|e| ImportError::Archive(format!("cannot read {name}: {e}")))?;
    let raw = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&raw);
    String::from_utf8(raw.to_vec())
        .map_err(|_| ImportError::Archive(format!("{name} is not UTF-8 text")))
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
}
