//! The synced preferences that hold a list, read and written item by item (ADR 0022, ADR 0006):
//! Collections, Copy presets and Saved searches (JSON lists of objects known by `name`),
//! Resource tags (a JSON object of Resource id → tags) and each Guide arrangement
//! (`{"sections":[…]}`, each section known by its `id`).
//!
//! A value is read as items only when writing those items back gives the same text, byte for
//! byte: the engine writes these values compactly with `serde_json`, which is what
//! [`ListPreference::compose`] writes. Anything else (a damaged value, one edited by hand, two
//! items of one name, a field a later version added beside the list) is not in shape, and is
//! kept and written whole as a [`SyncedPreference`](crate::synced::SyncedPreference) row.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::annotation::{Change, Row, RowKey};
use crate::synced::{SyncedGuideSection, SyncedListItem, SyncedResourceTags};

/// The key of the Collections preference.
pub(crate) const COLLECTIONS_KEY: &str = "library.collections";
/// The key of the Resource tags preference.
pub(crate) const RESOURCE_TAGS_KEY: &str = "library.tags";
/// The key of the Copy presets preference.
pub(crate) const COPY_PRESETS_KEY: &str = "copy.presets";
/// The key of the Saved searches preference.
pub(crate) const SAVED_SEARCHES_KEY: &str = "search.saved";
/// The keys of the Guide arrangements: this, then the kind of Guide.
pub(crate) const GUIDE_ARRANGEMENT_PREFIX: &str = "guide-arrangement/";

/// A synced preference whose value is a list of items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListPreference {
    Collections,
    CopyPresets,
    SavedSearches,
    ResourceTags,
    /// The arrangement of one kind of Guide.
    GuideArrangement(String),
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Identified {
    id: String,
}

#[derive(Deserialize)]
struct Sections<'a> {
    #[serde(borrow)]
    sections: Vec<&'a RawValue>,
}

impl ListPreference {
    /// The list preference `key` is, if it is one.
    pub(crate) fn of_key(key: &str) -> Option<Self> {
        Some(match key {
            COLLECTIONS_KEY => Self::Collections,
            COPY_PRESETS_KEY => Self::CopyPresets,
            SAVED_SEARCHES_KEY => Self::SavedSearches,
            RESOURCE_TAGS_KEY => Self::ResourceTags,
            _ => {
                let guide = key.strip_prefix(GUIDE_ARRANGEMENT_PREFIX)?;
                if guide.is_empty() {
                    return None;
                }
                Self::GuideArrangement(guide.to_string())
            }
        })
    }

    /// The list preference an item row belongs to.
    pub(crate) fn of_row(key: &RowKey) -> Option<Self> {
        Some(match key {
            RowKey::Collection(_) => Self::Collections,
            RowKey::CopyPreset(_) => Self::CopyPresets,
            RowKey::SavedSearch(_) => Self::SavedSearches,
            RowKey::ResourceTags(_) => Self::ResourceTags,
            RowKey::GuideSection(guide, _) => Self::GuideArrangement(guide.clone()),
            _ => return None,
        })
    }

    /// Its preference key.
    pub(crate) fn key(&self) -> String {
        match self {
            Self::Collections => COLLECTIONS_KEY.into(),
            Self::CopyPresets => COPY_PRESETS_KEY.into(),
            Self::SavedSearches => SAVED_SEARCHES_KEY.into(),
            Self::ResourceTags => RESOURCE_TAGS_KEY.into(),
            Self::GuideArrangement(guide) => format!("{GUIDE_ARRANGEMENT_PREFIX}{guide}"),
        }
    }

    /// How its items' tombstones and item clocks are named: their `row_table`, and the
    /// start every `row_id` of its items has (a Guide arrangement's `<guide>/`).
    pub(crate) fn item_rows(&self) -> (&'static str, String) {
        match self {
            Self::Collections => ("collection", String::new()),
            Self::CopyPresets => ("copy_preset", String::new()),
            Self::SavedSearches => ("saved_search", String::new()),
            Self::ResourceTags => ("resource_tags", String::new()),
            Self::GuideArrangement(guide) => ("guide_section", format!("{guide}/")),
        }
    }

    /// The key of its item that `row_id` (an item's tombstone or item-clock id) names.
    pub(crate) fn item_key(&self, row_id: &str) -> Option<RowKey> {
        let (_, prefix) = self.item_rows();
        let id = row_id.strip_prefix(&prefix)?.to_string();
        Some(match self {
            Self::Collections => RowKey::Collection(id),
            Self::CopyPresets => RowKey::CopyPreset(id),
            Self::SavedSearches => RowKey::SavedSearch(id),
            Self::ResourceTags => RowKey::ResourceTags(id),
            Self::GuideArrangement(guide) => RowKey::GuideSection(guide.clone(), id),
        })
    }

    fn named(&self, item: SyncedListItem) -> Row {
        match self {
            Self::CopyPresets => Row::CopyPreset(item),
            Self::SavedSearches => Row::SavedSearch(item),
            _ => Row::Collection(item),
        }
    }

    /// The items of `value` (no value: none), in order; `None` if it is not in shape.
    pub(crate) fn items(&self, value: Option<&str>) -> Option<Vec<Row>> {
        let Some(value) = value else {
            return Some(Vec::new());
        };
        let rows = match self {
            Self::Collections | Self::CopyPresets | Self::SavedSearches => {
                let raw: Vec<&RawValue> = serde_json::from_str(value).ok()?;
                let mut rows = Vec::with_capacity(raw.len());
                for (i, item) in raw.into_iter().enumerate() {
                    let Named { name } = serde_json::from_str(item.get()).ok()?;
                    rows.push(self.named(SyncedListItem {
                        name,
                        position: u32::try_from(i).ok()?,
                        json: item.get().to_string(),
                    }));
                }
                rows
            }
            Self::ResourceTags => {
                let raw: BTreeMap<String, &RawValue> = serde_json::from_str(value).ok()?;
                raw.into_iter()
                    .map(|(resource_id, tags)| {
                        Row::ResourceTags(SyncedResourceTags {
                            resource_id,
                            json: tags.get().to_string(),
                        })
                    })
                    .collect()
            }
            Self::GuideArrangement(guide) => {
                let Sections { sections } = serde_json::from_str(value).ok()?;
                let mut rows = Vec::with_capacity(sections.len());
                for (i, item) in sections.into_iter().enumerate() {
                    let Identified { id } = serde_json::from_str(item.get()).ok()?;
                    rows.push(Row::GuideSection(SyncedGuideSection {
                        guide: guide.clone(),
                        section: id,
                        position: u32::try_from(i).ok()?,
                        json: item.get().to_string(),
                    }));
                }
                rows
            }
        };
        let mut keys: Vec<RowKey> = rows.iter().map(Row::key).collect();
        keys.sort_by_key(|k| k.tombstone().1);
        keys.dedup();
        (keys.len() == rows.len() && self.compose(&rows) == value).then_some(rows)
    }

    /// The value holding `items`: lists in order of position (then name), Resource tags by
    /// Resource id, written as `serde_json` writes the engine's values.
    pub(crate) fn compose(&self, items: &[Row]) -> String {
        let mut placed: Vec<(u32, String, &str)> = items
            .iter()
            .filter_map(|row| match row {
                Row::Collection(i) | Row::CopyPreset(i) | Row::SavedSearch(i) => {
                    Some((i.position, i.name.clone(), i.json.as_str()))
                }
                Row::GuideSection(s) => Some((s.position, s.section.clone(), s.json.as_str())),
                Row::ResourceTags(t) => Some((0, t.resource_id.clone(), t.json.as_str())),
                _ => None,
            })
            .collect();
        placed.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        match self {
            Self::Collections | Self::CopyPresets | Self::SavedSearches => {
                let items: Vec<&str> = placed.iter().map(|p| p.2).collect();
                format!("[{}]", items.join(","))
            }
            Self::GuideArrangement(_) => {
                let items: Vec<&str> = placed.iter().map(|p| p.2).collect();
                format!("{{\"sections\":[{}]}}", items.join(","))
            }
            Self::ResourceTags => {
                let entries: Vec<String> = placed
                    .iter()
                    .map(|(_, id, json)| {
                        let id = serde_json::to_string(id).unwrap_or_default();
                        format!("{id}:{json}")
                    })
                    .collect();
                format!("{{{}}}", entries.join(","))
            }
        }
    }
}

/// The named list item `row` (a Collection, Copy preset or Saved search) under the name
/// `name`: its JSON with its `name` field replaced, every other field as it was, in its
/// order. `None` for another kind of row or JSON that is not an object with a name.
pub(crate) fn renamed_item(row: &Row, name: &str) -> Option<Row> {
    let (Row::Collection(item) | Row::CopyPreset(item) | Row::SavedSearch(item)) = row else {
        return None;
    };
    let fields = ordered_fields(&item.json)?;
    let mut found = false;
    let mut parts = Vec::with_capacity(fields.len());
    for (field, value) in fields {
        let value = if field == "name" {
            found = true;
            serde_json::to_string(name).ok()?
        } else {
            value.get().to_string()
        };
        parts.push(format!("{}:{value}", serde_json::to_string(&field).ok()?));
    }
    if !found {
        return None;
    }
    let renamed = SyncedListItem {
        name: name.to_string(),
        position: item.position,
        json: format!("{{{}}}", parts.join(",")),
    };
    Some(match row {
        Row::CopyPreset(_) => Row::CopyPreset(renamed),
        Row::SavedSearch(_) => Row::SavedSearch(renamed),
        _ => Row::Collection(renamed),
    })
}

/// The fields of the JSON object `json`, in their order, each value as written.
fn ordered_fields(json: &str) -> Option<Vec<(String, Box<RawValue>)>> {
    struct Fields;
    impl<'de> serde::de::Visitor<'de> for Fields {
        type Value = Vec<(String, Box<RawValue>)>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(entry) = map.next_entry::<String, Box<RawValue>>()? {
                out.push(entry);
            }
            Ok(out)
        }
    }
    let mut de = serde_json::Deserializer::from_str(json);
    let fields = serde::Deserializer::deserialize_map(&mut de, Fields).ok()?;
    de.end().ok()?;
    Some(fields)
}

/// The Changes that turn the items `before` into `after`: deletes first, then updates and
/// inserts in the order of `after`. Items that did not change have none.
pub(crate) fn diff(before: &[Row], after: &[Row]) -> Vec<Change> {
    let old: HashMap<RowKey, &Row> = before.iter().map(|r| (r.key(), r)).collect();
    let new: HashMap<RowKey, &Row> = after.iter().map(|r| (r.key(), r)).collect();
    let mut changes: Vec<Change> = before
        .iter()
        .filter(|r| !new.contains_key(&r.key()))
        .map(|r| Change::delete(r.clone()))
        .collect();
    for row in after {
        match old.get(&row.key()) {
            Some(was) if *was == row => {}
            Some(was) => changes.push(Change::update((*was).clone(), row.clone())),
            None => changes.push(Change::insert(row.clone())),
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn values_the_engine_writes_read_as_items_and_compose_back_byte_for_byte() {
        let cases = [
            (
                COLLECTIONS_KEY,
                r#"[{"name":"Greek","rules":[{"language":"grc"}],"resources":[]},{"name":"Old","rules":[],"resources":["kjv"]}]"#,
                2,
            ),
            (
                COPY_PRESETS_KEY,
                r#"[{"name":"Plain","options":{"verse_numbers":false}}]"#,
                1,
            ),
            (SAVED_SEARCHES_KEY, "[]", 0),
            (
                RESOURCE_TAGS_KEY,
                r#"{"kjv":["older","\"quoted\""],"web":["modern"]}"#,
                2,
            ),
            (
                "guide-arrangement/passage_guide",
                r#"{"sections":[{"id":"commentaries","collapsed":false,"hidden":false,"collection":"Greek"},{"id":"cross_references","collapsed":true,"hidden":false}]}"#,
                2,
            ),
        ];
        for (key, value, n) in cases {
            let list = ListPreference::of_key(key).unwrap();
            let items = list.items(Some(value)).unwrap_or_else(|| panic!("{key}"));
            assert_eq!(items.len(), n, "{key}");
            assert_eq!(list.compose(&items), value, "{key}");
        }
    }

    #[test]
    fn a_value_not_in_shape_is_not_read_as_items() {
        for (key, value) in [
            (COLLECTIONS_KEY, "not json"),
            (COLLECTIONS_KEY, r#"[{"name":"A"},{"name":"A"}]"#),
            (COLLECTIONS_KEY, r#"[{"rules":[]}]"#),
            (COLLECTIONS_KEY, r#"[ {"name":"A"} ]"#),
            (COPY_PRESETS_KEY, r#"{"name":"A"}"#),
            (RESOURCE_TAGS_KEY, r#"{"kjv":["a"],"kjv":["b"]}"#),
            (RESOURCE_TAGS_KEY, r#"{"web":["a"],"kjv":["b"]}"#),
            ("guide-arrangement/word_study", "{}"),
            (
                "guide-arrangement/word_study",
                r#"{"sections":[],"later":1}"#,
            ),
            (
                "guide-arrangement/word_study",
                r#"{"sections":[{"id":"forms"},{"id":"forms"}]}"#,
            ),
        ] {
            let list = ListPreference::of_key(key).unwrap();
            assert!(list.items(Some(value)).is_none(), "{key} = {value}");
        }
        assert!(ListPreference::of_key("guide-arrangement/").is_none());
        assert!(ListPreference::of_key("highlights.palette").is_none());
    }

    #[test]
    fn a_renamed_item_keeps_every_other_field_in_its_order() {
        let list = ListPreference::Collections;
        let items = list
            .items(Some(r#"[{"rules":[{"a":1}],"name":"Greek","ids":["x"]}]"#))
            .unwrap();
        let renamed = renamed_item(&items[0], "Greek (2)").unwrap();
        let Row::Collection(item) = &renamed else {
            panic!("{renamed:?}")
        };
        assert_eq!(item.name, "Greek (2)");
        assert_eq!(
            item.json,
            r#"{"rules":[{"a":1}],"name":"Greek (2)","ids":["x"]}"#
        );
        assert_eq!(
            list.items(Some(&list.compose(&[renamed.clone()]))),
            Some(vec![renamed])
        );
        assert!(
            renamed_item(
                &Row::ResourceTags(SyncedResourceTags {
                    resource_id: "kjv".into(),
                    json: "[]".into()
                }),
                "x"
            )
            .is_none()
        );
    }

    #[test]
    fn a_diff_deletes_updates_and_inserts_only_what_changed() {
        let list = ListPreference::Collections;
        let before = list
            .items(Some(r#"[{"name":"A"},{"name":"B"},{"name":"C"}]"#))
            .unwrap();
        let after = list
            .items(Some(r#"[{"name":"B"},{"name":"C","x":1},{"name":"D"}]"#))
            .unwrap();
        let changes = diff(&before, &after);
        let kinds: Vec<(bool, bool, RowKey)> = changes
            .iter()
            .map(|c| (c.before.is_some(), c.after.is_some(), c.key().unwrap()))
            .collect();
        let key = |n: &str| RowKey::Collection(n.into());
        assert_eq!(
            kinds,
            vec![
                (true, false, key("A")),
                (true, true, key("B")),
                (true, true, key("C")),
                (false, true, key("D")),
            ]
        );
        assert!(diff(&after, &after).is_empty());
    }
}
