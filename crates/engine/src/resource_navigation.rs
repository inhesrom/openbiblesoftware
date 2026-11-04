//! Resource Keys used by reading Tabs. Commentary starts are ordered in Standard numbering;
//! entry lookups use the same normalization and Strong's fallback as the Resource store.

use std::collections::BTreeMap;

use obs_model::{Key, KeyEntry, UnitId, normalize, transliteration_key};
use obs_scripture::Verse;

#[derive(Debug, Clone, Default)]
pub struct ResourceNavigation {
    pub keys: Vec<KeyEntry>,
    sections: BTreeMap<Verse, usize>,
    entries: Vec<usize>,
    entry_units: BTreeMap<UnitId, usize>,
    unit_keys: BTreeMap<UnitId, Vec<usize>>,
    lookup: BTreeMap<Key, usize>,
    transliterations: BTreeMap<String, usize>,
}

impl ResourceNavigation {
    pub fn new(mut keys: Vec<KeyEntry>) -> Self {
        keys.sort();
        let mut sections = BTreeMap::new();
        for (i, entry) in keys.iter().enumerate() {
            if let Key::Passage(p) = entry.key {
                // For coincident book/chapter introductions and sections, the narrowest
                // Passage wins. Equal Passages keep their stable KeyEntry order.
                sections.entry(p.start()).or_insert(i);
            }
        }
        let mut entries = Vec::new();
        let mut entry_units = BTreeMap::new();
        let mut unit_keys: BTreeMap<UnitId, Vec<usize>> = BTreeMap::new();
        for (i, e) in keys.iter().enumerate() {
            unit_keys.entry(e.unit.clone()).or_default().push(i);
            if !e.alias && !matches!(e.key, Key::Passage(_)) && !entry_units.contains_key(&e.unit) {
                entry_units.insert(e.unit.clone(), entries.len());
                entries.push(i);
            }
        }
        let mut lookup: BTreeMap<Key, usize> = BTreeMap::new();
        let mut transliterations = BTreeMap::new();
        for (i, e) in keys.iter().enumerate() {
            let key = normalized(&e.key);
            if lookup
                .get(&key)
                .is_none_or(|old| keys[*old].alias && !e.alias)
            {
                lookup.insert(key, i);
            }
            if let Key::Lemma(lemma) = &e.key {
                transliterations
                    .entry(transliteration_key(lemma))
                    .or_insert(i);
            }
        }
        Self {
            lookup,
            transliterations,
            keys,
            sections,
            entries,
            entry_units,
            unit_keys,
        }
    }

    /// Last section starting at or before the requested Verse, even across chapters/books.
    /// Before the first section there is no earlier section, so no target is invented.
    pub fn section(&self, verse: Verse) -> Option<&KeyEntry> {
        let (_, i) = self.sections.range(..=verse).next_back()?;
        self.keys.get(*i)
    }

    pub fn lookup(&self, key: &Key) -> Option<&KeyEntry> {
        let found = self.lookup.get(&normalized(key)).map(|i| &self.keys[*i]);
        found.or_else(|| match key {
            Key::StrongsNumber(n) if n.is_extended() => self.lookup(&Key::StrongsNumber(n.base())),
            _ => None,
        })
    }

    pub fn parse_entry(&self, text: &str) -> Option<&KeyEntry> {
        let text = text.trim();
        if let Ok(n) = text.to_uppercase().parse()
            && let Some(entry) = self.lookup(&Key::StrongsNumber(n))
        {
            return Some(entry);
        }
        self.lookup(&Key::Headword(text.into()))
            .or_else(|| self.lookup(&Key::Lemma(text.into())))
            .or_else(|| {
                let text = obs_model::tokenize(text)
                    .into_iter()
                    .map(|t| t.text)
                    .collect::<Vec<_>>()
                    .join(" ");
                self.transliterations.get(&text).map(|i| &self.keys[*i])
            })
    }

    /// One primary Key per entry, in Key order; aliases never add navigation stops.
    pub fn entries(&self) -> impl Iterator<Item = &KeyEntry> {
        self.entries.iter().map(|i| &self.keys[*i])
    }

    pub fn entry_for_unit(&self, unit: &UnitId) -> Option<&KeyEntry> {
        Some(&self.keys[*self.entries.get(*self.entry_units.get(unit)?)?])
    }

    pub fn passage_at(&self, unit: &UnitId, byte: usize) -> Option<Verse> {
        self.unit_keys
            .get(unit)?
            .iter()
            .filter_map(|i| {
                let e = &self.keys[*i];
                match e.key {
                    Key::Passage(p) if e.range.is_none_or(|(start, _)| start as usize <= byte) => {
                        Some(p.start())
                    }
                    _ => None,
                }
            })
            .max()
    }

    pub fn adjacent(&self, key: &Key, forward: bool) -> Option<&KeyEntry> {
        let current = self.lookup(key)?;
        let at = *self.entry_units.get(&current.unit)?;
        let next = if forward {
            at.checked_add(1)?
        } else {
            at.checked_sub(1)?
        };
        self.entries.get(next).map(|i| &self.keys[*i])
    }
}

fn normalized(key: &Key) -> Key {
    match key {
        Key::Lemma(s) => Key::Lemma(normalize(s)),
        Key::Headword(s) => Key::Headword(normalize(s)),
        _ => key.clone(),
    }
}
