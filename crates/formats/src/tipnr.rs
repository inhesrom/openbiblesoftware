//! STEPBible TIPNR (Translators Individualised Proper Names with all References, CC BY) → a
//! People & places index.
//!
//! The source is one text file of records. A record starts at a `$==========` line naming its
//! section (`PERSON(s)`, `PLACE`, `PLACE+PERSON`, `OTHER`, `EXCLUDED OTHER`); its next line is
//! the header (tab-separated), then sub-records starting with `–` (one per form of the name,
//! with its references, and a `– Total` line), then `@Briefest=`, `@Brief=`, `@Short=` and
//! `@Article=` lines. A bare `$` line ends the data.
//!
//! Kept, for PERSON, PLACE and PLACE+PERSON records only (structured fields):
//!
//! | source | kept as |
//! |---|---|
//! | header `UniqueName=uStrong` | entity `id` (the unique name), `name`, `strongs` |
//! | header `Type` | `kind` (Male, Female, Group, Place) |
//! | person: parents, siblings, partners, offspring | `relations` (Father/Mother or Ancestor, Sibling, Partner, Offspring or Founded) |
//! | place: founder, people living there | `relations` (Founder, Inhabitant) |
//! | person: tribe/nation; place: geographical area | `region` |
//! | `– Total` line, all names | `other_names` |
//! | each sub-record's references | mentions, one per Verse (Standard versification) |
//!
//! **Never kept:** the `Description` and `#Summary description` columns and the `@Briefest`,
//! `@Brief`, `@Short` and `@Article` lines (prose, partly written by an AI model; #15), map
//! links and coordinates, and OTHER records (deities, months, titles, languages). Relations
//! must name another kept entity; free text in a relation column is dropped. Every excluded
//! column is counted in the report, and the converter fails if any description prose ends up
//! in an entity.

use std::collections::{BTreeMap, BTreeSet};

use obs_model::{
    Entity, EntityKind, EntityRelation, Relation, Resource, ResourceBuilder, ResourceType,
};
use obs_scripture::{Book, Passage, Standard, StrongsNumber, Verse, Versification};
use unicode_normalization::UnicodeNormalization;

use crate::dataset::{DatasetError, DatasetOptions, start};
use crate::report::ImportReport;

/// `source_format` of a People & places index converted from TIPNR.
pub const TIPNR_FORMAT: &str = "stepbible-tipnr";

/// The section a record belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Person,
    Place,
    /// OTHER and EXCLUDED OTHER: not people or places.
    Other,
}

/// One record, as lines split into tab-separated fields.
struct Record<'a> {
    section: Section,
    line: usize,
    header: Vec<&'a str>,
    /// `–` lines, the significance in field 0.
    subs: Vec<Vec<&'a str>>,
    /// Text of the `@Briefest=` … `@Article=` lines.
    descriptions: Vec<(&'static str, &'a str)>,
}

/// Header columns.
const COL_ID: usize = 0;
const COL_DESCRIPTION: usize = 1;
/// Person: parents (`Father + Mother`); place: founder.
const COL_PARENTS: usize = 2;
/// Person: siblings; place: people living there.
const COL_SIBLINGS: usize = 3;
const COL_PARTNERS: usize = 4;
const COL_OFFSPRING: usize = 5;
/// Person: tribe/nation; place: geographical area.
const COL_REGION: usize = 6;
const COL_SUMMARY: usize = 7;
const COL_TYPE: usize = 8;
/// Sub-record columns.
const SUB_UNIQUE_NAME: usize = 1;
const SUB_REFS: usize = 4;
const TOTAL_NAMES: usize = 1;

const DESCRIPTION_LINES: [(&str, &str); 4] = [
    ("@Briefest=", "description: @Briefest"),
    ("@Brief=", "description: @Brief"),
    ("@Short=", "description: @Short"),
    ("@Article=", "description: @Article"),
];

/// Convert the TIPNR text file.
pub fn import_tipnr(
    text: &str,
    options: &DatasetOptions,
) -> Result<(Resource, ImportReport), DatasetError> {
    let (mut builder, mut report) = start(options, ResourceType::PeopleAndPlaces, TIPNR_FORMAT)?;
    let records = split_records(text)?;
    let mut entities: Vec<(Entity, &Record<'_>)> = Vec::new();
    for record in &records {
        if record.section == Section::Other {
            builder.count_dropped("record: OTHER (not a person or place)");
            continue;
        }
        let Some(entity) = entity(record, &mut builder, &mut report) else {
            continue;
        };
        entities.push((entity, record));
    }
    if entities.is_empty() {
        return Err(DatasetError::NoRecords);
    }
    let ids: BTreeSet<String> = entities.iter().map(|(e, _)| e.id.clone()).collect();
    for (entity, record) in &mut entities {
        entity.relations.retain(|r| {
            let known = ids.contains(&r.entity);
            if !known {
                builder.count_dropped("relation: not a person or place in the index");
            }
            known
        });
        check_excluded_text_not_kept(entity, record)?;
        for sub in &record.subs {
            add_mentions(&entity.id, sub, record.line, &mut builder, &mut report);
        }
    }
    for (entity, _) in entities {
        builder.add_entity(entity);
    }
    let resource = builder.build().map_err(DatasetError::Invalid)?;
    report.entities = resource.entities.len();
    report.mentions = resource.mentions.len();
    report.dropped_markup = resource.dropped_markup.clone();
    Ok((resource, report))
}

fn split_records(text: &str) -> Result<Vec<Record<'_>>, DatasetError> {
    let mut records: Vec<Record<'_>> = Vec::new();
    // Inside a record whose header has been read.
    let mut open = false;
    let mut pending: Option<(Section, usize)> = None;
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let row = raw
            .trim_start_matches('\u{feff}')
            .trim_end_matches(['\r', '\t']);
        if let Some(rest) = row.strip_prefix('$') {
            let name = rest.split('\t').next().unwrap_or("");
            let name = name.trim_matches(|c| c == '=' || c == ' ');
            let section = match name {
                // A bare `$` ends the data; working notes follow it.
                "" => break,
                "PERSON(s)" => Section::Person,
                "PLACE" | "PLACE+PERSON" => Section::Place,
                "OTHER" | "EXCLUDED OTHER" => Section::Other,
                other => {
                    return Err(DatasetError::Syntax {
                        line,
                        message: format!("unknown section `{other}`"),
                    });
                }
            };
            pending = Some((section, line));
            open = false;
            continue;
        }
        if let Some((section, start)) = pending.take() {
            let header: Vec<&str> = row.split('\t').collect();
            // The field-description block at the top uses the same markers; real records
            // have a unique name with `@` in their first field.
            if header[0].contains('@') {
                records.push(Record {
                    section,
                    line: start,
                    header,
                    subs: Vec::new(),
                    descriptions: Vec::new(),
                });
                open = true;
            }
            continue;
        }
        if !open {
            continue;
        }
        let Some(record) = records.last_mut() else {
            continue;
        };
        if row.starts_with('–') {
            record.subs.push(row.split('\t').collect());
        } else if let Some((marker, name)) = DESCRIPTION_LINES
            .iter()
            .find(|(marker, _)| row.starts_with(marker))
        {
            let value = row[marker.len()..].split('\t').next().unwrap_or("");
            record.descriptions.push((name, value));
        }
    }
    Ok(records)
}

fn field<'a>(fields: &[&'a str], i: usize) -> &'a str {
    fields.get(i).map_or("", |f| f.trim())
}

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// The entity of one PERSON or PLACE record, or `None` (counted) if it cannot be kept.
fn entity(
    record: &Record<'_>,
    builder: &mut ResourceBuilder,
    report: &mut ImportReport,
) -> Option<Entity> {
    let h = &record.header;
    let unique = field(h, COL_ID);
    let (id, strongs) = match unique.split_once('=') {
        Some((id, strongs)) => (id.trim(), Some(strongs.trim())),
        None => (unique, None),
    };
    let name = id.split('@').next().unwrap_or("").trim();
    let kind = match (record.section, field(h, COL_TYPE)) {
        (Section::Person, "Male") => EntityKind::Male,
        (Section::Person, "Female") => EntityKind::Female,
        (Section::Person, "Group") => EntityKind::Group,
        (Section::Place, "Place") => EntityKind::Place,
        (_, other) => {
            builder.count_dropped("record: unknown type");
            report.warnings.push(format!(
                "line {}: `{id}` dropped: unknown type `{other}`",
                record.line
            ));
            return None;
        }
    };
    // Unnamed individuals carry the placeholder `H0000` / `G0000`.
    let placeholder = |s: &str| s.len() > 1 && s[1..].bytes().all(|b| b == b'0');
    let strongs = strongs.and_then(|s| match s.parse::<StrongsNumber>() {
        _ if placeholder(s) => None,
        Ok(n) => Some(n),
        Err(_) => {
            report.warnings.push(format!(
                "line {}: `{id}` has an unreadable Strong's number `{s}`",
                record.line
            ));
            None
        }
    });
    for (col, what) in [
        (COL_DESCRIPTION, "description: Description column"),
        (COL_SUMMARY, "description: #Summary description"),
    ] {
        // A place's column 1 is its OpenBible.info name, not a description.
        if !(record.section == Section::Place && col == COL_DESCRIPTION)
            && !field(h, col).is_empty()
        {
            builder.count_dropped(what);
        }
    }
    for (what, text) in &record.descriptions {
        if !text.trim().is_empty() {
            builder.count_dropped(what);
        }
    }
    let mut relations = Vec::new();
    let mut add = |column: &str, plain: Relation, builder: &mut ResourceBuilder| {
        for token in column.split([',', '+']) {
            link(token, plain, &mut relations, builder);
        }
    };
    match record.section {
        Section::Person => {
            let parents = field(h, COL_PARENTS);
            let (father, mother) = parents.split_once('+').unwrap_or((parents, ""));
            add(father, Relation::Father, builder);
            add(mother, Relation::Mother, builder);
            add(field(h, COL_SIBLINGS), Relation::Sibling, builder);
            add(field(h, COL_PARTNERS), Relation::Partner, builder);
            add(field(h, COL_OFFSPRING), Relation::Offspring, builder);
        }
        Section::Place | Section::Other => {
            add(field(h, COL_PARENTS), Relation::Founder, builder);
            add(field(h, COL_SIBLINGS), Relation::Inhabitant, builder);
        }
    }
    let region = match field(h, COL_REGION) {
        "" | ">" => None,
        r => Some(nfc(r)),
    };
    let mut other_names: Vec<String> = Vec::new();
    if let Some(total) = record
        .subs
        .iter()
        .find(|s| field(s, 0).trim_start_matches('–').trim() == "Total")
    {
        for n in field(total, TOTAL_NAMES).split(" or ") {
            let n = nfc(n.trim());
            if !n.is_empty() && n != name && !other_names.contains(&n) {
                other_names.push(n);
            }
        }
    }
    Some(Entity {
        id: nfc(id),
        name: nfc(name),
        kind,
        strongs,
        other_names,
        region,
        relations,
    })
}

/// Add the relation named by one list item such as `Amram@Exo.6.18-1Ch`,
/// `Canaan@Gen.9.18-1Ch(d)` or `Shobal@1Ch.2.50-=H7732H`. Markers: `(a)` ancestor, `(d)`
/// descended from, `(f)` founder of, `(?)` uncertain.
fn link(
    token: &str,
    plain: Relation,
    relations: &mut Vec<EntityRelation>,
    builder: &mut ResourceBuilder,
) {
    let mut t = token.trim();
    if t.is_empty() {
        return;
    }
    let mut markers = String::new();
    while let Some(open) = t.strip_suffix(')').and_then(|s| s.rfind('(')) {
        markers.push_str(&t[open + 1..t.len() - 1]);
        t = t[..open].trim_end();
    }
    let id = t.split_once('=').map_or(t, |(id, _)| id).trim();
    if !id.contains('@') {
        builder.count_dropped("relation: free text, not a unique name");
        return;
    }
    let relation = match plain {
        Relation::Father | Relation::Mother if markers.contains(['a', 'd']) => Relation::Ancestor,
        Relation::Offspring if markers.contains('f') => Relation::Founded,
        other => other,
    };
    let relation = EntityRelation {
        relation,
        entity: nfc(id),
        uncertain: markers.contains('?'),
    };
    if !relations.contains(&relation) {
        relations.push(relation);
    }
}

/// Fail if any description prose of `record` is inside a kept field of `entity`.
fn check_excluded_text_not_kept(entity: &Entity, record: &Record<'_>) -> Result<(), DatasetError> {
    let excluded = prose(record);
    let kept = std::iter::once(&entity.name)
        .chain(&entity.other_names)
        .chain(&entity.region);
    for value in kept {
        for (_, text) in &excluded {
            if value.contains(text) {
                return Err(DatasetError::ExcludedTextKept {
                    entity: entity.id.clone(),
                    text: text.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Add a mention for each Verse in a sub-record's references (`Exo.4.14; Exo.7.10a`,
/// `Num.13.8,16`); the `– Total` line abbreviates and is skipped.
fn add_mentions(
    entity: &str,
    sub: &[&str],
    line: usize,
    builder: &mut ResourceBuilder,
    report: &mut ImportReport,
) {
    if field(sub, 0).trim_start_matches('–').trim() == "Total"
        || !field(sub, SUB_UNIQUE_NAME).contains('@')
    {
        return;
    }
    let mut last: Option<(Book, u16)> = None;
    for piece in field(sub, SUB_REFS).split([';', ',']) {
        let t = piece.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("LXX") {
            builder.count_dropped("mention: only in the Septuagint");
            continue;
        }
        let Some(verse) = parse_ref(t, last) else {
            builder.count_dropped("mention: unreadable reference");
            report
                .warnings
                .push(format!("record at line {line}: unreadable reference `{t}`"));
            continue;
        };
        last = Some((verse.book, verse.chapter));
        if !Standard.contains(verse) {
            builder.count_dropped("mention: outside the Standard versification");
            report.warnings.push(format!(
                "record at line {line}: {verse} is not in the Standard versification"
            ));
            continue;
        }
        builder.add_mention(entity, Passage::verse(verse));
    }
}

/// `Exo.7.10a` (a letter marks a second occurrence in the verse), or a bare verse number
/// continuing the previous reference's chapter (`16` after `Num.13.8`).
fn parse_ref(s: &str, last: Option<(Book, u16)>) -> Option<Verse> {
    let parts: Vec<&str> = s.split('.').collect();
    let (book, chapter, verse) = match parts.as_slice() {
        [b, c, v] => (Book::from_usfm(b)?, c.parse().ok()?, *v),
        [v] => {
            let (b, c) = last?;
            (b, c, *v)
        }
        _ => return None,
    };
    let verse = verse.trim_end_matches(|c: char| c.is_ascii_lowercase());
    Some(Verse::new(book, chapter, verse.parse().ok()?))
}

/// A record's description prose, by column: the `Description` and `#Summary description`
/// columns and the `@Brief`, `@Short` and `@Article` lines. `@Briefest` is excluded from the
/// Resource too but not listed: its labels of three words or fewer coincide with structured
/// values (a region "Early Patriarch", a name "Gennesaret").
fn prose<'a>(record: &Record<'a>) -> Vec<(&'static str, &'a str)> {
    let mut out = Vec::new();
    let mut put = |what: &'static str, t: &'a str| {
        let t = t.trim().trim_start_matches('#').trim();
        if !t.is_empty() {
            out.push((what, t));
        }
    };
    if record.section != Section::Place {
        put("Description", field(&record.header, COL_DESCRIPTION));
    }
    put("#Summary description", field(&record.header, COL_SUMMARY));
    for (what, t) in &record.descriptions {
        if *what != "description: @Briefest" {
            put(what.trim_start_matches("description: "), t);
        }
    }
    out
}

/// Every description prose text in a TIPNR source (see [`import_tipnr`]), by column, for
/// checks that none reaches a written file.
pub fn tipnr_description_texts(
    text: &str,
) -> Result<BTreeMap<&'static str, BTreeSet<String>>, DatasetError> {
    let mut out: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    for record in split_records(text)? {
        for (what, t) in prose(&record) {
            out.entry(what).or_default().insert(t.to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_parse_with_letters_and_continuations() {
        let num_13_8 = Verse::new(Book::Numbers, 13, 8);
        assert_eq!(parse_ref("Num.13.8", None), Some(num_13_8));
        assert_eq!(
            parse_ref("16", Some((Book::Numbers, 13))),
            Some(Verse::new(Book::Numbers, 13, 16))
        );
        assert_eq!(
            parse_ref("Exo.7.10a", None),
            Some(Verse::new(Book::Exodus, 7, 10))
        );
        assert_eq!(
            parse_ref("Jhn.3.16", None),
            Some(Verse::new(Book::John, 3, 16))
        );
        assert_eq!(parse_ref("etc", None), None);
        assert_eq!(parse_ref("16", None), None);
    }

    #[test]
    fn relation_markers_change_the_relation() {
        let mut b = ResourceBuilder::new(obs_model::Manifest::new(
            "t",
            "t",
            "t",
            ResourceType::PeopleAndPlaces,
            "en",
        ));
        let mut rels = Vec::new();
        link(
            "Canaan@Gen.9.18-1Ch(d)",
            Relation::Father,
            &mut rels,
            &mut b,
        );
        link(
            "Tekoa@2Sa.14.2-Amo(f)",
            Relation::Offspring,
            &mut rels,
            &mut b,
        );
        link("Jahdai@1Ch.2.47(?)", Relation::Sibling, &mut rels, &mut b);
        link(
            " Shobal@1Ch.2.50-=H7732H ",
            Relation::Founder,
            &mut rels,
            &mut b,
        );
        link(
            "Word sometimes translated as a place",
            Relation::Founder,
            &mut rels,
            &mut b,
        );
        let got: Vec<(Relation, &str, bool)> = rels
            .iter()
            .map(|r| (r.relation, r.entity.as_str(), r.uncertain))
            .collect();
        assert_eq!(
            got,
            [
                (Relation::Ancestor, "Canaan@Gen.9.18-1Ch", false),
                (Relation::Founded, "Tekoa@2Sa.14.2-Amo", false),
                (Relation::Sibling, "Jahdai@1Ch.2.47", true),
                (Relation::Founder, "Shobal@1Ch.2.50-", false),
            ]
        );
    }
}
