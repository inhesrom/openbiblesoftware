//! The Passage Guide's People & places section (#54; decision #15): the people and places a
//! People & places index (TIPNR) lists for the Passage, each with structured facts and links
//! to the Dictionary articles about them.
//!
//! - **Rows** come from every People & places index in Preferred order (an entity listed by
//!   two indexes shows once). They are ordered by where the ★ Bible's text of the Passage
//!   first names them, then by first mention.
//! - **Name:** the form the ★ Bible uses in the Passage (`Abram` in Gen 12:1), with the usual
//!   name beside it when it differs (`Abraham`).
//! - **Facts:** TIPNR's structured fields only: kind, region (a person's tribe or nation, a
//!   place's area) and relations. TIPNR's descriptions, partly AI-written, are never read
//!   (#15): the converter does not even store them.
//! - **Articles:** one link per Dictionary in Preferred order that has an article on the
//!   entity, from the derived alias cache ([`crate::aliases`]).

use obs_model::{Entity, EntityKind, Relation};
use obs_scripture::{Passage, Verse};

/// A link to one Dictionary article.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleLink {
    pub resource_id: String,
    pub abbreviation: String,
    pub title: String,
    /// The article's headword, as the Dictionary spells it (its Key).
    pub headword: String,
}

/// One structured fact: `Father: Terah`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub label: &'static str,
    pub values: Vec<String>,
}

/// One person, people group or place mentioned in the Passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonOrPlace {
    /// The index's id for the entity (TIPNR's unique name).
    pub entity: String,
    /// The People & places index it comes from.
    pub index_id: String,
    /// As the Passage names them.
    pub name: String,
    /// The usual name, when the Passage uses another (`Abraham` for `Abram`).
    pub usual_name: Option<String>,
    pub kind: EntityKind,
    pub region: Option<String>,
    pub facts: Vec<Fact>,
    /// The Verses of the Passage that mention them (Standard versification).
    pub verses: Vec<Verse>,
    pub articles: Vec<ArticleLink>,
}

impl PersonOrPlace {
    /// `Man · Early Patriarch`.
    pub fn summary(&self) -> String {
        let mut s = kind_name(self.kind).to_string();
        if let Some(r) = &self.region {
            s.push_str(" · ");
            s.push_str(r);
        }
        s
    }
}

pub fn kind_name(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Male => "Man",
        EntityKind::Female => "Woman",
        EntityKind::Group => "People group",
        EntityKind::Place => "Place",
    }
}

pub fn relation_label(r: Relation) -> &'static str {
    match r {
        Relation::Father => "Father",
        Relation::Mother => "Mother",
        Relation::Ancestor => "Descended from",
        Relation::Sibling => "Siblings",
        Relation::Partner => "Married to",
        Relation::Offspring => "Children",
        Relation::Founded => "Founded",
        Relation::Founder => "Founded by",
        Relation::Inhabitant => "Lived here",
    }
}

/// The name an entity id stands for: `Terah@Gen.11.24-Luk` → `Terah`,
/// `Mary_Magdalene@…` → `Mary Magdalene`.
pub fn display_name(entity_id: &str) -> String {
    entity_id
        .split('@')
        .next()
        .unwrap_or(entity_id)
        .replace('_', " ")
}

/// The usual name to show for an entity: its name, or for TIPNR's composed names
/// (`Olives_Mount`) its first other name (`Mount of Olives`).
pub fn usual_name(e: &Entity) -> String {
    if e.name.contains('_')
        && let Some(first) = e.other_names.first()
    {
        return first.clone();
    }
    e.name.replace('_', " ")
}

/// Relations as facts, in relation order, each relative once.
pub fn facts(e: &Entity) -> Vec<Fact> {
    let mut by: Vec<(Relation, Vec<String>)> = Vec::new();
    for r in &e.relations {
        let mut name = display_name(&r.entity);
        if r.uncertain {
            name.push_str(" (?)");
        }
        match by.iter_mut().find(|(rel, _)| *rel == r.relation) {
            Some((_, names)) if !names.contains(&name) => names.push(name),
            Some(_) => {}
            None => by.push((r.relation, vec![name])),
        }
    }
    by.sort_by_key(|(r, _)| *r);
    by.into_iter()
        .map(|(r, values)| Fact {
            label: relation_label(r),
            values,
        })
        .collect()
}

/// Where in `text` one of `names` first appears as a whole word (case-sensitive: these are
/// proper names), and which name. Longer names win at the same place.
pub fn first_named(text: &str, names: &[String]) -> Option<(usize, String)> {
    let mut best: Option<(usize, String)> = None;
    for name in names.iter().filter(|n| !n.is_empty()) {
        let mut from = 0;
        while let Some(i) = text.get(from..).and_then(|t| t.find(name.as_str())) {
            let at = from + i;
            let end = at + name.len();
            let before = text[..at].chars().next_back();
            let after = text[end..].chars().next();
            let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            if boundary(before) && boundary(after) {
                let better = match &best {
                    None => true,
                    Some((b, n)) => at < *b || (at == *b && name.len() > n.len()),
                };
                if better {
                    best = Some((at, name.clone()));
                }
                break;
            }
            from = end;
        }
    }
    best
}

/// One row from an entity, its mentions and the ★ Bible's text of the Passage.
pub fn row(
    index_id: &str,
    e: &Entity,
    verses: Vec<Verse>,
    text: &str,
    articles: Vec<ArticleLink>,
) -> (Option<usize>, PersonOrPlace) {
    let usual = usual_name(e);
    let found = first_named(text, &crate::aliases::entity_names(e));
    let name = found.as_ref().map_or(usual.clone(), |(_, n)| n.clone());
    let row = PersonOrPlace {
        entity: e.id.clone(),
        index_id: index_id.to_string(),
        usual_name: (name != usual).then_some(usual),
        name,
        kind: e.kind,
        region: e.region.clone(),
        facts: facts(e),
        verses,
        articles,
    };
    (found.map(|(at, _)| at), row)
}

/// Sort rows by where the text names them (unnamed last), then first mention, then name.
pub fn order(rows: &mut [(Option<usize>, PersonOrPlace)]) {
    rows.sort_by(|(a, x), (b, y)| {
        a.unwrap_or(usize::MAX)
            .cmp(&b.unwrap_or(usize::MAX))
            .then(x.verses.first().cmp(&y.verses.first()))
            .then(x.name.cmp(&y.name))
    });
}

/// The ★ Bible's text of `passage`: the first Bible in Preferred order that has it.
#[cfg(not(target_arch = "wasm32"))]
fn passage_text(
    library: &crate::Library,
    passage: Passage,
    cache: &crate::key_words::BibleCache,
) -> String {
    use obs_store::ResourceStore;
    for entry in library.of_type(obs_model::ResourceType::Bible) {
        let Ok(reader) = obs_store::ObrReader::open(&entry.path) else {
            continue;
        };
        let Ok(map) = cache.verse_map(&reader) else {
            continue;
        };
        let Ok(verses) = crate::key_words::passage_verses(&reader, map.as_ref().as_ref(), passage)
        else {
            continue;
        };
        let mut texts: std::collections::HashMap<obs_model::UnitId, String> = Default::default();
        let mut out = String::new();
        for v in verses {
            if !texts.contains_key(&v.unit)
                && let Ok(Some((_, t))) = reader.unit_text(&v.unit)
            {
                texts.insert(v.unit.clone(), t);
            }
            if let Some(t) = texts.get(&v.unit).and_then(|t| t.get(v.range())) {
                out.push_str(t);
                out.push(' ');
            }
        }
        if !out.trim().is_empty() {
            return out;
        }
    }
    String::new()
}

/// The People & places of `passage` from the library.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_people_and_places(
    library: &crate::Library,
    passage: Passage,
    cache: &crate::key_words::BibleCache,
    aliases: &crate::aliases::AliasCache,
) -> Result<Vec<PersonOrPlace>, String> {
    use obs_model::ResourceType;
    use obs_store::ResourceStore;
    let mut text: Option<String> = None;
    let mut rows: Vec<(Option<usize>, PersonOrPlace)> = Vec::new();
    let mut errors = Vec::new();
    let dictionaries: Vec<&crate::LibraryEntry> =
        library.of_type(ResourceType::Dictionary).collect();
    for index in library.of_type(ResourceType::PeopleAndPlaces) {
        let read = obs_store::ObrReader::open(&index.path).and_then(|r| {
            let mentions = r.mentions_in(passage)?;
            Ok((r, mentions))
        });
        let (reader, mentions) = match read {
            Ok(x) => x,
            Err(e) => {
                errors.push(format!("{}: {e}", index.manifest.abbreviation));
                continue;
            }
        };
        // Entity → the Verses of the Passage that mention it, in order.
        let mut by_entity: Vec<(String, Vec<Verse>)> = Vec::new();
        for m in mentions {
            let verses: Vec<Verse> = m
                .passage
                .verses(&obs_scripture::Standard)
                .filter(|v| passage.contains_verse(*v))
                .collect();
            match by_entity.iter_mut().find(|(id, _)| *id == m.entity) {
                Some((_, vs)) => vs.extend(verses),
                None => by_entity.push((m.entity, verses)),
            }
        }
        if by_entity.is_empty() {
            continue;
        }
        let tables: Vec<(
            &crate::LibraryEntry,
            std::sync::Arc<crate::aliases::AliasTable>,
        )> = dictionaries
            .iter()
            .filter_map(|d| aliases.table(index, d).ok().map(|t| (*d, t)))
            .collect();
        let text = text.get_or_insert_with(|| passage_text(library, passage, cache));
        for (id, mut verses) in by_entity {
            if rows.iter().any(|(_, r)| r.entity == id) {
                continue;
            }
            let Ok(Some(entity)) = reader.entity(&id) else {
                continue;
            };
            verses.sort();
            verses.dedup();
            let articles = tables
                .iter()
                .filter_map(|(d, t)| {
                    Some(ArticleLink {
                        resource_id: d.manifest.resource_id.clone(),
                        abbreviation: d.manifest.abbreviation.clone(),
                        title: d.manifest.title.clone(),
                        headword: t.headword(&id)?.to_string(),
                    })
                })
                .collect();
            rows.push(row(
                &index.manifest.resource_id,
                &entity,
                verses,
                text,
                articles,
            ));
        }
    }
    if rows.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    order(&mut rows);
    Ok(rows.into_iter().map(|(_, r)| r).collect())
}

#[cfg(target_arch = "wasm32")]
pub fn library_people_and_places(
    _: &crate::Library,
    _: Passage,
    _: &crate::key_words::BibleCache,
    _: &crate::aliases::AliasCache,
) -> Result<Vec<PersonOrPlace>, String> {
    Err("Opening Resources is not supported here".into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use obs_model::EntityRelation;

    fn abraham() -> Entity {
        let rel = |relation, entity: &str, uncertain| EntityRelation {
            relation,
            entity: entity.into(),
            uncertain,
        };
        Entity {
            id: "Abraham@Gen.11.26-1Pe".into(),
            name: "Abraham".into(),
            kind: EntityKind::Male,
            strongs: None,
            other_strongs: Vec::new(),
            other_names: vec!["Abram".into()],
            region: Some("Early Patriarch".into()),
            relations: vec![
                rel(Relation::Offspring, "Isaac@Gen.17.19-Jas", false),
                rel(Relation::Father, "Terah@Gen.11.24-Luk", false),
                rel(Relation::Partner, "Sarah@Gen.11.29-1Pe", false),
                rel(Relation::Offspring, "Ishmael@Gen.16.11-Psa", true),
            ],
        }
    }

    #[test]
    fn the_row_uses_the_passages_name_and_lists_relations_as_facts() {
        let text = "Now the LORD had said unto Abram, Get thee out of thy country.";
        let (at, r) = row("tipnr", &abraham(), vec![], text, vec![]);
        assert_eq!(at, Some(27));
        assert_eq!(r.name, "Abram");
        assert_eq!(r.usual_name.as_deref(), Some("Abraham"));
        assert_eq!(r.summary(), "Man · Early Patriarch");
        let facts: Vec<(&str, Vec<&str>)> = r
            .facts
            .iter()
            .map(|f| (f.label, f.values.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(
            facts,
            [
                ("Father", vec!["Terah"]),
                ("Married to", vec!["Sarah"]),
                ("Children", vec!["Isaac", "Ishmael (?)"]),
            ]
        );
        // Not named in the text (another Bible's wording): the usual name, ranked last.
        let (at, r) = row("tipnr", &abraham(), vec![], "Abrams and abram", vec![]);
        assert_eq!((at, r.name.as_str(), r.usual_name), (None, "Abraham", None));
    }

    #[test]
    fn composed_names_read_naturally() {
        let mut e = abraham();
        e.name = "Olives_Mount".into();
        e.other_names = vec!["Mount of Olives".into(), "Olivet".into()];
        assert_eq!(usual_name(&e), "Mount of Olives");
        assert_eq!(
            display_name("Mary_Magdalene@Mat.27.56-Jhn"),
            "Mary Magdalene"
        );
        assert_eq!(
            first_named(
                "from the mount called Olivet",
                &["Olives Mount".into(), "Olivet".into()]
            ),
            Some((22, "Olivet".to_string()))
        );
    }
}
