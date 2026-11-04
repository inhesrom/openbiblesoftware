//! The locked Easton zLD/TEI and Smith RawLD/ThML editions. This is not the general SWORD
//! Import surface (#48): callers supply only the edition's four named data files.
use super::{StudyImport, html};
use anyhow::{Context, Result, bail};
use obs_model::Key;
use std::{collections::BTreeMap, io::Read};

pub enum DictionaryData<'a> {
    Easton {
        index: &'a [u8],
        data: &'a [u8],
        block_index: &'a [u8],
        blocks: &'a [u8],
    },
    Smith {
        index: &'a [u8],
        data: &'a [u8],
    },
}

fn slice(bytes: &[u8], start: usize, size: usize) -> Result<&[u8]> {
    bytes
        .get(
            start
                ..start
                    .checked_add(size)
                    .context("dictionary offset overflow")?,
        )
        .context("dictionary offset outside file")
}
fn u32_at(bytes: &[u8], at: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(slice(bytes, at, 4)?.try_into()?) as usize)
}

/// Decode entries, preserve reference spans, and add the documented Abram → Abraham alias.
/// Abram's short original entry stays readable; the alias additionally opens Abraham.
pub fn import_dictionary(import: &mut StudyImport, input: DictionaryData<'_>) -> Result<()> {
    let (index, data, width) = match input {
        DictionaryData::Easton { index, data, .. } => (index, data, 8),
        DictionaryData::Smith { index, data } => (index, data, 6),
    };
    if index.is_empty() || index.len() % width != 0 {
        bail!("invalid dictionary index length");
    }
    let mut cache = BTreeMap::new();
    let mut entries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for record in index.chunks_exact(width) {
        let offset = u32_at(record, 0)?;
        let size = if width == 8 {
            u32_at(record, 4)?
        } else {
            u16::from_le_bytes(record[4..6].try_into()?) as usize
        };
        let entry = slice(data, offset, size)?;
        let newline = entry
            .iter()
            .position(|b| *b == b'\n')
            .context("dictionary entry without headword")?;
        let head = std::str::from_utf8(&entry[..newline])?.trim().to_string();
        let value = &entry[newline + 1..];
        let text = match input {
            DictionaryData::Smith { .. } => encoding_rs::WINDOWS_1252.decode(value).0.into_owned(),
            DictionaryData::Easton {
                block_index,
                blocks,
                ..
            } => {
                if value.starts_with(b"@LINK ") {
                    let target = std::str::from_utf8(&value[6..])?.trim().to_string();
                    import.aliases.push((display(&head), display(&target)));
                    continue;
                }
                let block = u32_at(value, 0)?;
                let item = u32_at(value, 4)?;
                if let std::collections::btree_map::Entry::Vacant(e) = cache.entry(block) {
                    let row = block.checked_mul(8).context("block index overflow")?;
                    let off = u32_at(block_index, row)?;
                    let len = u32_at(block_index, row + 4)?;
                    let mut decoded = Vec::new();
                    flate2::read::ZlibDecoder::new(slice(blocks, off, len)?)
                        .take(32 * 1024 * 1024 + 1)
                        .read_to_end(&mut decoded)?;
                    if decoded.len() > 32 * 1024 * 1024 {
                        bail!("dictionary block exceeds 32 MiB");
                    }
                    e.insert(decoded);
                }
                let bytes = &cache[&block];
                if item >= u32_at(bytes, 0)? {
                    bail!("dictionary item outside block");
                }
                let row = 4 + item * 8;
                let off = u32_at(bytes, row)?;
                let len = u32_at(bytes, row + 4)?;
                std::str::from_utf8(slice(bytes, off, len)?)?
                    .trim_end_matches('\0')
                    .to_string()
            }
        };
        let title = text
            .find('>')
            .and_then(|end| html::attr(&text[..=end], "n"))
            .filter(|title| title.eq_ignore_ascii_case(&head))
            .unwrap_or_else(|| display(&head));
        let senses = entries.entry(title).or_default();
        if !senses.contains(&text) {
            senses.push(text);
        } else {
            *import
                .report
                .dropped_markup
                .entry("duplicate_dictionary_entry".into())
                .or_default() += 1;
        }
    }
    if entries.contains_key("Abraham") {
        import.aliases.push(("Abram".into(), "Abraham".into()));
    }
    for (title, senses) in entries {
        let source = senses.join("<br>");
        let text = html::render(&source, None, &mut import.report);
        if text.text.trim().is_empty() {
            bail!("empty dictionary entry {title}");
        }
        import
            .sections
            .push((Key::Headword(title.clone()), title, text));
    }
    Ok(())
}

fn display(s: &str) -> String {
    s.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
