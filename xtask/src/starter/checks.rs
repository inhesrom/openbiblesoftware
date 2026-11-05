//! Structural checks of the whole Starter library (#45, #20 decision 6.2).
//!
//! They run over every built Resource at once, after each one passed `Resource::validate`,
//! and every failure names the item that breaks the rule:
//!
//! 1. every Bible Verse maps to a Standard Passage or is explicitly Unmapped;
//! 2. every Standard Verse of a Bible book the Bible has is present or explicitly Absent;
//! 3. every Strong's number of a Tagged word resolves in at least one Lexicon of the library;
//! 4. every Cross-reference and Scripture-reference target is a Standard Passage;
//! 5. no span, Verse or Key range crosses the end of its unit.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use obs_model::{Key, Resource, ResourceType, SpanKind, TextUnit, UnitId};
use obs_scripture::{Passage, Standard, StrongsNumber, Verse, Versification};

/// What the rules looked at, for the build report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    pub bibles: usize,
    pub bible_verses: usize,
    /// Standard Verses of the Bible books each Bible has, summed over the Bibles.
    pub standard_verses: usize,
    pub lexicons: usize,
    pub tagged_words: usize,
    pub strongs_numbers: usize,
    pub cross_references: usize,
    pub scripture_references: usize,
    pub spans: usize,
}

/// Rule failures, each naming the Resource and the item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub coverage: Coverage,
    pub failures: Vec<String>,
}

/// Run every rule over `resources`. Rule 3 needs a Lexicon to resolve against: a library
/// without one has its Tagged words counted but not checked (`coverage.lexicons == 0`).
pub fn check(resources: &[&Resource]) -> Outcome {
    let mut out = Outcome::default();
    let mut strongs_keys: HashSet<StrongsNumber> = HashSet::new();
    for r in resources {
        if r.manifest.resource_type == ResourceType::Lexicon {
            out.coverage.lexicons += 1;
            strongs_keys.extend(r.keys.iter().filter_map(|k| match k.key {
                Key::StrongsNumber(n) => Some(n),
                _ => None,
            }));
        }
    }
    // Unresolved Strong's number → where it was first seen, and how many Tagged words use it.
    let mut unresolved: BTreeMap<StrongsNumber, (String, usize)> = BTreeMap::new();
    let mut seen_numbers: HashSet<StrongsNumber> = HashSet::new();
    for r in resources {
        check_ranges(r, &mut out);
        check_references(r, &mut out);
        if r.manifest.resource_type == ResourceType::Bible {
            out.coverage.bibles += 1;
            check_verse_map(r, &mut out);
            collect_tagged_words(
                r,
                &strongs_keys,
                &mut seen_numbers,
                &mut unresolved,
                &mut out,
            );
        }
    }
    out.coverage.strongs_numbers = seen_numbers.len();
    if out.coverage.lexicons > 0 {
        for (number, (first, count)) in unresolved {
            let more = if count > 1 {
                format!(" and {} more Tagged word(s)", count - 1)
            } else {
                String::new()
            };
            out.failures.push(format!(
                "Strong's number {number} resolves in no Lexicon ({first}{more})"
            ));
        }
    }
    out
}

/// Whether both ends of a Passage are Standard Verses.
fn is_standard(p: Passage) -> bool {
    Standard.contains(p.start()) && Standard.contains(p.end())
}

/// Rule 5: spans, Verses and Key ranges stay inside their unit's text, on char boundaries.
fn check_ranges(r: &Resource, out: &mut Outcome) {
    let id = &r.manifest.resource_id;
    let units: BTreeMap<&UnitId, &TextUnit> = r.units.iter().map(|u| (&u.id, u)).collect();
    let crosses = |unit: &TextUnit, start: usize, end: usize| -> Option<String> {
        let len = unit.text.len();
        if start > end {
            Some(format!("{start}..{end} ends before it starts"))
        } else if end > len {
            Some(format!(
                "{start}..{end} crosses the end of unit `{}` ({len} bytes)",
                unit.id
            ))
        } else if !unit.text.is_char_boundary(start) || !unit.text.is_char_boundary(end) {
            Some(format!(
                "{start}..{end} in unit `{}` splits a UTF-8 character",
                unit.id
            ))
        } else {
            None
        }
    };
    for unit in &r.units {
        for span in &unit.spans {
            out.coverage.spans += 1;
            if let Some(why) = crosses(unit, span.start as usize, span.end as usize) {
                out.failures
                    .push(format!("`{id}`: `{}` span {why}", span.kind.name()));
            }
        }
    }
    for v in &r.verses {
        if let Some(unit) = units.get(&v.unit)
            && let Some(why) = crosses(unit, v.start as usize, v.end as usize)
        {
            out.failures
                .push(format!("`{id}`: Verse {} range {why}", v.verse));
        }
    }
    for k in &r.keys {
        if let Some((s, e)) = k.range
            && let Some(unit) = units.get(&k.unit)
            && let Some(why) = crosses(unit, s as usize, e as usize)
        {
            out.failures
                .push(format!("`{id}`: Key `{}` range {why}", k.key));
        }
    }
}

/// Rule 4: Cross-references, and the `xref` and `scripture_ref` spans of any Resource, point
/// at Standard Passages.
fn check_references(r: &Resource, out: &mut Outcome) {
    let id = &r.manifest.resource_id;
    for x in &r.cross_references {
        out.coverage.cross_references += 1;
        for (what, p) in [("source", x.from), ("target", x.to)] {
            if !is_standard(p) {
                out.failures.push(format!(
                    "`{id}`: Cross-reference {} → {} has a {what} outside the Standard versification",
                    x.from, x.to
                ));
            }
        }
    }
    for unit in &r.units {
        for span in &unit.spans {
            let targets = match &span.kind {
                SpanKind::Xref { targets } | SpanKind::ScriptureRef { targets } => targets,
                _ => continue,
            };
            out.coverage.scripture_references += 1;
            for p in targets.iter().filter(|p| !is_standard(**p)) {
                out.failures.push(format!(
                    "`{id}`: `{}` span {}..{} in unit `{}` targets {p}, which is outside the \
                     Standard versification",
                    span.kind.name(),
                    span.start,
                    span.end,
                    unit.id
                ));
            }
        }
    }
}

/// Rules 1 and 2, over a Bible's Verse map.
fn check_verse_map(r: &Resource, out: &mut Outcome) {
    let id = &r.manifest.resource_id;
    out.coverage.bible_verses += r.verses.len();
    let Some(map) = &r.verse_map else {
        out.failures.push(format!("`{id}`: Bible has no Verse map"));
        return;
    };
    let entries = map.entries();
    for (i, v) in r.verses.iter().enumerate() {
        match entries.get(i) {
            Some(e) if e.verse == v.verse && e.last == v.last => {
                if let Some(p) = e.standard
                    && !is_standard(p)
                {
                    out.failures.push(format!(
                        "`{id}`: Bible Verse {} maps to {p}, which is outside the Standard \
                         versification (it must map to Standard Verses or be Unmapped)",
                        v.verse
                    ));
                }
            }
            _ => out.failures.push(format!(
                "`{id}`: Bible Verse {} has no Verse map entry (neither mapped nor Unmapped)",
                v.verse
            )),
        }
    }
    let books: BTreeSet<_> = entries.iter().map(|e| e.verse.book).collect();
    for standard in Standard.verses().filter(|v| books.contains(&v.book)) {
        out.coverage.standard_verses += 1;
        let native = map.to_native(standard);
        match (native.is_empty(), map.is_absent(standard)) {
            (true, true) | (false, false) => {}
            (true, false) => out.failures.push(format!(
                "`{id}`: Standard Verse {standard} is neither present nor Absent"
            )),
            (false, true) => out.failures.push(format!(
                "`{id}`: Standard Verse {standard} is Absent yet present as {}",
                native
                    .iter()
                    .map(Verse::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// Rule 3, collection half: every Strong's number of a Tagged word, with the Bible Verse of
/// its first use when no Lexicon Key answers it.
fn collect_tagged_words(
    r: &Resource,
    strongs_keys: &HashSet<StrongsNumber>,
    seen: &mut HashSet<StrongsNumber>,
    unresolved: &mut BTreeMap<StrongsNumber, (String, usize)>,
    out: &mut Outcome,
) {
    let id = &r.manifest.resource_id;
    let resolves = |n: StrongsNumber| {
        strongs_keys.contains(&n) || (n.is_extended() && strongs_keys.contains(&n.base()))
    };
    for unit in &r.units {
        let mut verses: Vec<_> = r.verses.iter().filter(|v| v.unit == unit.id).collect();
        verses.sort_by_key(|v| v.start);
        for span in &unit.spans {
            let SpanKind::TaggedWord { parts } = &span.kind else {
                continue;
            };
            out.coverage.tagged_words += 1;
            for part in parts {
                seen.insert(part.strongs);
                if resolves(part.strongs) {
                    continue;
                }
                let at = verses
                    .iter()
                    .take_while(|v| v.start <= span.start)
                    .last()
                    .map(|v| v.verse.to_string())
                    .unwrap_or_else(|| format!("unit `{}`", unit.id));
                unresolved
                    .entry(part.strongs)
                    .or_insert_with(|| (format!("`{id}` {at}"), 0))
                    .1 += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obs_model::{BibleVerse, KeyEntry, Manifest, MorphemePart, Span, UnitKind};
    use obs_scripture::{Book, VerseMap, VerseMapEntry};

    fn empty(id: &str, kind: ResourceType) -> Resource {
        Resource {
            manifest: Manifest::new(id, id, id, kind, "en"),
            units: vec![],
            verses: vec![],
            keys: vec![],
            dropped_markup: Default::default(),
            unit_aliases: Default::default(),
            verse_map: None,
            cross_references: vec![],
            entities: vec![],
            mentions: vec![],
            transliterations: vec![],
            renderings: Default::default(),
        }
    }

    fn unit(id: &str, text: &str) -> TextUnit {
        TextUnit {
            id: UnitId::new(id).unwrap(),
            kind: UnitKind::BibleBook,
            title: None,
            text: text.to_string(),
            spans: vec![],
        }
    }

    /// A Bible holding all of Obadiah (one word per verse), each Verse mapped to itself.
    fn obadiah() -> Resource {
        let mut r = empty("test-bible", ResourceType::Bible);
        let mut text = String::new();
        let mut entries = Vec::new();
        for v in 1..=21u16 {
            let start = text.len() as u32;
            text.push_str(&format!("v{v:02}"));
            let verse = Verse::new(Book::Obadiah, 1, v);
            r.verses.push(BibleVerse {
                verse,
                last: v,
                unit: UnitId::new("OBA").unwrap(),
                start,
                end: text.len() as u32,
            });
            entries.push(VerseMapEntry {
                verse,
                last: v,
                standard: Some(Passage::verse(verse)),
            });
        }
        r.units.push(unit("OBA", &text));
        r.verse_map = Some(VerseMap::new(entries, vec![], &Standard));
        r
    }

    fn failures(resources: &[&Resource]) -> Vec<String> {
        check(resources).failures
    }

    #[test]
    fn a_clean_bible_passes_with_its_coverage_counted() {
        let r = obadiah();
        let out = check(&[&r]);
        assert_eq!(out.failures, Vec::<String>::new());
        assert_eq!(out.coverage.bibles, 1);
        assert_eq!(out.coverage.bible_verses, 21);
        assert_eq!(out.coverage.standard_verses, 21);
    }

    #[test]
    fn a_bible_without_a_verse_map_fails() {
        let mut r = obadiah();
        r.verse_map = None;
        let f = failures(&[&r]);
        assert_eq!(f, ["`test-bible`: Bible has no Verse map"]);
    }

    #[test]
    fn a_bible_verse_without_a_map_entry_fails_naming_the_verse() {
        let mut r = obadiah();
        let map = r.verse_map.as_ref().unwrap();
        let entries: Vec<VerseMapEntry> = map.entries()[..20].to_vec();
        r.verse_map = Some(VerseMap::new(entries, vec![], &Standard));
        let f = failures(&[&r]);
        assert!(
            f.iter()
                .any(|m| m.contains("Bible Verse Obad 1:21 has no Verse map entry")),
            "{f:?}"
        );
    }

    #[test]
    fn a_verse_mapped_outside_the_standard_versification_fails() {
        let mut r = obadiah();
        let mut entries = r.verse_map.as_ref().unwrap().entries().to_vec();
        entries[0].standard = Some(Passage::verse(Verse::new(Book::Obadiah, 1, 99)));
        r.verse_map = Some(VerseMap::new(entries, vec![], &Standard));
        let f = failures(&[&r]);
        assert!(
            f.iter().any(|m| m.contains("Obad 1:1 maps to Obad 1:99")
                && m.contains("outside the Standard versification")),
            "{f:?}"
        );
    }

    #[test]
    fn an_unmapped_verse_passes_but_its_standard_slot_must_be_absent() {
        let mut r = obadiah();
        let mut entries = r.verse_map.as_ref().unwrap().entries().to_vec();
        entries[20].standard = None; // Obad 1:21 is now explicitly Unmapped…
        r.verse_map = Some(VerseMap::new(entries.clone(), vec![], &Standard));
        let f = failures(&[&r]);
        assert_eq!(
            f,
            ["`test-bible`: Standard Verse Obad 1:21 is neither present nor Absent"]
        );
        // …and marking the Standard Verse Absent satisfies rule 2.
        r.verse_map = Some(VerseMap::new(
            entries,
            vec![Verse::new(Book::Obadiah, 1, 21)],
            &Standard,
        ));
        assert_eq!(failures(&[&r]), Vec::<String>::new());
    }

    #[test]
    fn a_standard_verse_both_present_and_absent_fails() {
        let mut r = obadiah();
        let entries = r.verse_map.as_ref().unwrap().entries().to_vec();
        r.verse_map = Some(VerseMap::new(
            entries,
            vec![Verse::new(Book::Obadiah, 1, 5)],
            &Standard,
        ));
        let f = failures(&[&r]);
        assert_eq!(
            f,
            ["`test-bible`: Standard Verse Obad 1:5 is Absent yet present as Obad 1:5"]
        );
    }

    /// A one-entry Lexicon whose only Key is `number`.
    fn lexicon(number: &str) -> Resource {
        let mut r = empty("test-lexicon", ResourceType::Lexicon);
        r.units.push(unit("entry", "definition"));
        r.keys.push(KeyEntry {
            key: Key::StrongsNumber(number.parse().unwrap()),
            unit: UnitId::new("entry").unwrap(),
            range: None,
            alias: false,
        });
        r
    }

    fn tag(r: &mut Resource, start: u32, end: u32, number: &str) {
        r.units[0].spans.push(Span {
            start,
            end,
            kind: SpanKind::TaggedWord {
                parts: vec![MorphemePart::new(number.parse().unwrap())],
            },
        });
    }

    #[test]
    fn an_unresolved_strongs_number_fails_naming_its_first_use() {
        let mut bible = obadiah();
        tag(&mut bible, 0, 3, "H7225"); // in Obad 1:1
        tag(&mut bible, 3, 6, "H7225"); // in Obad 1:2
        let lexicon = lexicon("H1254");
        let f = failures(&[&bible, &lexicon]);
        assert_eq!(
            f,
            [
                "Strong's number H7225 resolves in no Lexicon (`test-bible` Obad 1:1 and 1 more Tagged word(s))"
            ]
        );
    }

    #[test]
    fn an_extended_number_resolves_through_its_base_entry() {
        let mut bible = obadiah();
        tag(&mut bible, 0, 3, "H1254A");
        let lexicon = lexicon("H1254");
        let out = check(&[&bible, &lexicon]);
        assert_eq!(out.failures, Vec::<String>::new());
        assert_eq!(out.coverage.tagged_words, 1);
        assert_eq!(out.coverage.strongs_numbers, 1);
    }

    #[test]
    fn without_a_lexicon_strongs_numbers_are_counted_but_not_checked() {
        let mut bible = obadiah();
        tag(&mut bible, 0, 3, "H7225");
        let out = check(&[&bible]);
        assert_eq!(out.failures, Vec::<String>::new());
        assert_eq!(out.coverage.lexicons, 0);
        assert_eq!(out.coverage.strongs_numbers, 1);
    }

    #[test]
    fn a_cross_reference_outside_the_standard_versification_fails() {
        let mut r = empty("test-xrefs", ResourceType::CrossReferenceSet);
        r.cross_references.push(obs_model::CrossReference {
            from: Passage::verse(Verse::new(Book::Genesis, 1, 1)),
            to: Passage::verse(Verse::new(Book::Genesis, 1, 99)),
            votes: 0,
        });
        let f = failures(&[&r]);
        assert_eq!(
            f,
            [
                "`test-xrefs`: Cross-reference Gen 1:1 → Gen 1:99 has a target outside the Standard versification"
            ]
        );
    }

    #[test]
    fn a_scripture_reference_span_with_an_invalid_target_fails() {
        let mut r = empty("test-commentary", ResourceType::Commentary);
        let mut u = unit("section", "see elsewhere");
        u.spans.push(Span {
            start: 4,
            end: 13,
            kind: SpanKind::ScriptureRef {
                targets: vec![Passage::verse(Verse::new(Book::Genesis, 99, 1))].into(),
            },
        });
        r.units.push(u);
        let f = failures(&[&r]);
        assert!(
            f.iter().any(|m| m.contains("`scripture_ref` span 4..13")
                && m.contains("targets Gen 99:1")),
            "{f:?}"
        );
    }

    #[test]
    fn a_span_crossing_the_end_of_its_unit_fails() {
        let mut r = empty("test-commentary", ResourceType::Commentary);
        let mut u = unit("section", "short");
        u.spans.push(Span {
            start: 0,
            end: 99,
            kind: SpanKind::Bold,
        });
        r.units.push(u);
        let f = failures(&[&r]);
        assert_eq!(
            f,
            ["`test-commentary`: `bold` span 0..99 crosses the end of unit `section` (5 bytes)"]
        );
    }

    #[test]
    fn a_verse_range_or_key_range_crossing_its_unit_fails() {
        let mut bible = obadiah();
        bible.verses[0].end = 9999;
        let f = failures(&[&bible]);
        assert!(
            f.iter()
                .any(|m| m.contains("Verse Obad 1:1 range") && m.contains("crosses the end")),
            "{f:?}"
        );
        let mut lexicon = lexicon("H1254");
        lexicon.keys[0].range = Some((0, 9999));
        let f = failures(&[&lexicon]);
        assert!(
            f.iter()
                .any(|m| m.contains("Key `H1254` range") && m.contains("crosses the end")),
            "{f:?}"
        );
    }

    #[test]
    fn a_span_splitting_a_utf8_character_fails() {
        let mut r = empty("test-commentary", ResourceType::Commentary);
        let mut u = unit("section", "π is pi");
        u.spans.push(Span {
            start: 1,
            end: 3,
            kind: SpanKind::Italic,
        });
        r.units.push(u);
        let f = failures(&[&r]);
        assert_eq!(
            f,
            ["`test-commentary`: `italic` span 1..3 in unit `section` splits a UTF-8 character"]
        );
    }
}
