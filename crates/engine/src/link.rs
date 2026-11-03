//! Link-set routing (#10, ADR 0002): a member's location is broadcast as a Standard Passage,
//! and every other member translates it into its own numbering through its Verse map.

use obs_scripture::{ListedVersification, Verse, VerseMap, Versification};

/// The Link sets offered in the chip menu.
pub const LINK_SETS: [char; 4] = ['A', 'B', 'C', 'D'];

/// Where a Bible Verse (own numbering) sits in the Standard versification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToStandard {
    /// The Standard Verse it starts at.
    Verse(Verse),
    /// An Unmapped verse: the Link set does not move.
    Unmapped,
}

/// A Bible Verse → the Standard Verse to broadcast. Bibles without a Verse map number like the
/// Standard.
pub fn to_standard(map: Option<&VerseMap>, native: Verse) -> ToStandard {
    let Some(map) = map else {
        return ToStandard::Verse(native);
    };
    match map.entry(native) {
        Some(e) => match e.standard {
            Some(p) => ToStandard::Verse(p.start()),
            None => ToStandard::Unmapped,
        },
        // Verses outside the map (e.g. a superscription slot) are taken as they are.
        None => ToStandard::Verse(native),
    }
}

/// A Standard Verse → where a follower goes in its own numbering: the first Bible Verse it
/// maps to, or for an Absent verse the nearest preceding Verse the Bible has.
pub fn to_native(
    map: Option<&VerseMap>,
    own: Option<&ListedVersification>,
    standard: Verse,
) -> Option<Verse> {
    if let Some(map) = map {
        if let Some(v) = map.first_native(standard) {
            return Some(v);
        }
        // Absent (or not covered): the mapped Verse with the greatest Standard start before it,
        // within the same book.
        return map
            .entries()
            .iter()
            .filter_map(|e| e.standard.map(|p| (p.start(), e.verse)))
            .filter(|(s, _)| s.book == standard.book && *s < standard)
            .max_by_key(|(s, _)| *s)
            .map(|(_, v)| v);
    }
    match own {
        None => Some(standard),
        Some(v11n) => nearest_preceding(v11n, standard),
    }
}

/// `verse` if the Versification has it, else the nearest Verse before it in the same book.
pub fn nearest_preceding(v11n: &ListedVersification, verse: Verse) -> Option<Verse> {
    if v11n.contains(verse) {
        return Some(verse);
    }
    let book = verse.book;
    let mut chapter = verse.chapter.min(v11n.chapter_count(book));
    let mut before = if chapter == verse.chapter {
        verse.verse
    } else {
        u16::MAX
    };
    while chapter >= 1 {
        if let Some(last) = v11n.last_verse(book, chapter) {
            let start = last.min(before.saturating_sub(1));
            for n in (0..=start).rev() {
                let v = Verse::new(book, chapter, n);
                if v11n.contains(v) {
                    return Some(v);
                }
            }
        }
        chapter -= 1;
        before = u16::MAX;
    }
    None
}
