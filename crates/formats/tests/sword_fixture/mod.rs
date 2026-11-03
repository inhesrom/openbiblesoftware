//! Builds the SWORD test modules under `tests/assets/sword/modules/` from the readable sources
//! next to them (`mods.d/*.conf` and `src/*.imp`), and checks the committed files against
//! those sources.
//!
//! An `.imp` file is `$$$key` lines, each followed by one line of entry text. Verse keys are
//! OSIS-like (`Gen.1.1`, `John.0.0` for a book introduction, `John.1.0` for a chapter
//! introduction, `John.1.4-5` for linked verses); dictionary keys are as stored, and an
//! `@LINK KEY` body makes an alias.
//!
//! Only what the fixtures use is written: RawText/RawCom (`.vss`), zText/zCom with ZIP blocks
//! of one book (`.bzs`/`.bzv`/`.bzz`) and RawLD (`.idx`/`.dat`). Index files stop after the
//! last slot used, which the reader treats as empty slots.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use sword_modules::{ModuleConf, Testament, VerseKey, Versification};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/sword")
}

/// The entries of an `.imp` source, in file order.
pub fn read_imp(name: &str) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(root().join("src").join(format!("{name}.imp"))).unwrap();
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let key = line
            .strip_prefix("$$$")
            .unwrap_or_else(|| panic!("{name}.imp: expected `$$$key`, got `{line}`"));
        out.push((key.to_string(), lines.next().unwrap_or("").to_string()));
    }
    out
}

pub fn confs() -> Vec<ModuleConf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root().join("mods.d"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| ModuleConf::from_file(p).unwrap())
        .collect()
}

/// `(first, last)` verse keys of an `.imp` verse key.
pub fn verse_keys(key: &str) -> (VerseKey, VerseKey) {
    let (a, b) = key.split_once('-').unwrap_or((key, ""));
    let parts: Vec<&str> = a.split('.').collect();
    let (book, chapter, verse): (&str, u8, u8) = (
        parts[0],
        parts[1].parse().unwrap(),
        parts[2].parse().unwrap(),
    );
    let first = VerseKey::from_osis(book, chapter, verse).unwrap();
    let last = match b.split('.').collect::<Vec<_>>().as_slice() {
        [""] => first,
        [v] => VerseKey::from_osis(book, chapter, v.parse().unwrap()).unwrap(),
        [c, v] => VerseKey::from_osis(book, c.parse().unwrap(), v.parse().unwrap()).unwrap(),
        other => panic!("bad key end {other:?}"),
    };
    (first, last)
}

fn data_dir(conf: &ModuleConf) -> String {
    conf.data_path()
        .unwrap()
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

/// The module's files (path relative to the library root → bytes).
pub fn build(conf: &ModuleConf) -> BTreeMap<String, Vec<u8>> {
    let name = conf.name().to_ascii_lowercase();
    let entries = read_imp(&name);
    match conf.get("ModDrv").unwrap() {
        "RawText" | "RawCom" => raw_verse(conf, &entries),
        "zText" | "zCom" => compressed_verse(conf, &entries),
        "RawLD" => raw_ld(conf, &entries),
        other => panic!("the fixture writer does not write {other}"),
    }
}

/// Every slot an entry fills, with the entry, grouped by testament prefix (`ot`/`nt`).
fn slots<'a>(
    conf: &ModuleConf,
    entries: &'a [(String, String)],
) -> BTreeMap<&'static str, Vec<(u32, usize, &'a str)>> {
    let v11n = Versification::get(conf.versification()).unwrap();
    let mut out: BTreeMap<&'static str, Vec<(u32, usize, &str)>> = BTreeMap::new();
    for (i, (key, text)) in entries.iter().enumerate() {
        let (first, last) = verse_keys(key);
        let start = v11n.index_of(&first).unwrap();
        let end = v11n.index_of(&last).unwrap();
        assert_eq!(start.testament, end.testament);
        for slot in start.slot..=end.slot {
            let prefix = match start.testament {
                Testament::Old => "ot",
                Testament::New => "nt",
            };
            out.entry(prefix)
                .or_default()
                .push((slot, i, text.as_str()));
        }
    }
    out
}

fn raw_verse(conf: &ModuleConf, entries: &[(String, String)]) -> BTreeMap<String, Vec<u8>> {
    let dir = data_dir(conf);
    let mut files = BTreeMap::new();
    for (prefix, slots) in slots(conf, entries) {
        let mut data = Vec::new();
        let mut at: BTreeMap<usize, (u32, u16)> = BTreeMap::new();
        let max = slots.iter().map(|s| s.0).max().unwrap();
        let mut index = vec![0u8; (max as usize + 1) * 6];
        for (slot, i, text) in slots {
            let (offset, len) = *at.entry(i).or_insert_with(|| {
                let offset = data.len() as u32;
                data.extend_from_slice(text.as_bytes());
                (offset, text.len() as u16)
            });
            let p = slot as usize * 6;
            index[p..p + 4].copy_from_slice(&offset.to_le_bytes());
            index[p + 4..p + 6].copy_from_slice(&len.to_le_bytes());
        }
        files.insert(format!("{dir}/{prefix}.vss"), index);
        files.insert(format!("{dir}/{prefix}"), data);
    }
    files
}

fn compressed_verse(conf: &ModuleConf, entries: &[(String, String)]) -> BTreeMap<String, Vec<u8>> {
    assert_eq!(conf.get("CompressType"), Some("ZIP"));
    assert_eq!(conf.get("BlockType"), Some("BOOK"));
    let dir = data_dir(conf);
    let mut files = BTreeMap::new();
    for (prefix, slots) in slots(conf, entries) {
        // One block per book, in slot order.
        let mut blocks: Vec<(String, Vec<u8>)> = Vec::new();
        let mut placed: BTreeMap<usize, (u32, u32, u16)> = BTreeMap::new();
        let max = slots.iter().map(|s| s.0).max().unwrap();
        let mut index = vec![0u8; (max as usize + 1) * 10];
        for (slot, i, text) in slots {
            let book = entries[i].0.split('.').next().unwrap().to_string();
            let (block, offset, len) = *placed.entry(i).or_insert_with(|| {
                if blocks.last().is_none_or(|(b, _)| *b != book) {
                    blocks.push((book.clone(), Vec::new()));
                }
                let block = (blocks.len() - 1) as u32;
                let data = &mut blocks.last_mut().unwrap().1;
                let offset = data.len() as u32;
                data.extend_from_slice(text.as_bytes());
                (block, offset, text.len() as u16)
            });
            let p = slot as usize * 10;
            index[p..p + 4].copy_from_slice(&block.to_le_bytes());
            index[p + 4..p + 8].copy_from_slice(&offset.to_le_bytes());
            index[p + 8..p + 10].copy_from_slice(&len.to_le_bytes());
        }
        let mut bzs = Vec::new();
        let mut bzz = Vec::new();
        for (_, data) in &blocks {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
            z.write_all(data).unwrap();
            let packed = z.finish().unwrap();
            for n in [bzz.len() as u32, packed.len() as u32, data.len() as u32] {
                bzs.extend_from_slice(&n.to_le_bytes());
            }
            bzz.extend_from_slice(&packed);
        }
        files.insert(format!("{dir}/{prefix}.bzv"), index);
        files.insert(format!("{dir}/{prefix}.bzs"), bzs);
        files.insert(format!("{dir}/{prefix}.bzz"), bzz);
    }
    files
}

fn raw_ld(conf: &ModuleConf, entries: &[(String, String)]) -> BTreeMap<String, Vec<u8>> {
    let prefix = data_dir(conf);
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by_key(|a| a.0.to_uppercase());
    let mut idx = Vec::new();
    let mut dat = Vec::new();
    for (key, body) in sorted {
        let record = format!("{key}\r\n{body}");
        idx.extend_from_slice(&(dat.len() as u32).to_le_bytes());
        idx.extend_from_slice(&(record.len() as u16).to_le_bytes());
        dat.extend_from_slice(record.as_bytes());
    }
    BTreeMap::from([
        (format!("{prefix}.idx"), idx),
        (format!("{prefix}.dat"), dat),
    ])
}
