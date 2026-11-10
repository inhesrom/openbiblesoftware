//! Visual filters (#211): the built-ins and the user's own filters in the User database, the
//! Tab header's switches (kept in Layouts), and what a filter colours: imperatives in Romans
//! 12 in TAGNT and, through Strong's alignment, in the KJV.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ops::Range;
use std::path::{Path, PathBuf};

use obs_model::{Resource, SpanKind, TextUnit};
use obs_scripture::{Passage, Verse};
use serde_json::json;

use crate::testing::bibles::write_library;
use crate::testing::{kjv_romans_12, tagnt_romans_12};
use crate::visual_filters::{
    AlignedWords, DocumentSlice, FilterMatcher, MarkedText, VisualFilter, block_hits,
    built_in_filters, paragraph_slices,
};
use crate::*;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// TAGNT and KJV at Romans 12 as real `.obr` files, with a User database.
fn launch(name: &str) -> (Session, Temp) {
    let root = std::env::temp_dir().join(format!("obs-filters-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_library(&root.join("library"), &[kjv_romans_12(), tagnt_romans_12()]);
    let s = reopen(&root);
    (s, Temp(root))
}

fn reopen(root: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn open(s: &mut Session, id: &str, split: bool) -> TabId {
    let out = s
        .run("resource.open", &json!({ "id": id, "split": split }))
        .unwrap();
    TabId(out["tab"].as_u64().unwrap())
}

fn shown(s: &Session, tab: TabId) -> Vec<String> {
    s.tab_visual_filters(tab)
        .into_iter()
        .map(|f| f.id)
        .collect()
}

#[test]
fn the_built_in_filters_are_there_until_the_user_changes_them_and_changes_last() {
    let (mut s, t) = launch("persist");
    let ids: Vec<String> = s.visual_filters().iter().map(|f| f.id.clone()).collect();
    assert_eq!(ids, ["words-of-christ", "divine-names", "imperatives"]);
    let list = s.run("filter.list", &json!({})).unwrap();
    assert_eq!(list[1]["name"], "Divine names");
    assert_eq!(list[1]["marked"], "divine_names");
    assert_eq!(list[2]["query"], "@imperative");
    assert!(
        s.visual_filters().iter().all(|f| !f.all_tabs),
        "off until switched on"
    );

    let before = s.visual_filters_revision();
    let made = s
        .run(
            "filter.save",
            &json!({
                "name": "Imperatives in Romans 12",
                "query": "@imperative",
                "scope": "Rom 12",
                "look": "background",
                "color": "#ffd6aa",
                "all_tabs": true,
            }),
        )
        .unwrap();
    assert_eq!(made["id"], "filter-1");
    assert!(s.visual_filters_revision() > before);
    s.run(
        "filter.save",
        &json!({ "id": "imperatives", "color": "#284696" }),
    )
    .unwrap();
    s.run("filter.delete", &json!({ "id": "words-of-christ" }))
        .unwrap();

    // A relaunch reads them back from the User database.
    let s = reopen(&t.0);
    let filters = s.visual_filters();
    let ids: Vec<&str> = filters.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["divine-names", "imperatives", "filter-1"]);
    assert_eq!(filters[1].color.to_string(), "#284696");
    assert_eq!(filters[2].scope, "Rom 12");
    assert_eq!(filters[2].look, HighlightLook::Background);
    assert!(filters[2].all_tabs);
}

#[test]
fn a_filter_needs_a_name_and_something_to_colour_and_its_query_and_passages_must_read() {
    let (mut s, _t) = launch("checks");
    let err = |s: &mut Session, params: serde_json::Value| {
        s.run("filter.save", &params).unwrap_err().to_string()
    };
    assert!(err(&mut s, json!({ "query": "@imperative" })).contains("needs a name"));
    assert!(err(&mut s, json!({ "name": "Nothing" })).contains("query or marked text"));
    assert!(
        err(&mut s, json!({ "name": "Typo", "query": "@imperatve" })).contains("Query:"),
        "a misspelt Morphology criterion is refused as the Search Tool refuses it"
    );
    assert!(
        err(
            &mut s,
            json!({ "name": "Where", "query": "love", "scope": "Hezekiah 3" })
        )
        .contains("Passages:")
    );
    assert!(
        err(
            &mut s,
            json!({ "name": "Look", "query": "love", "look": "glow" })
        )
        .contains("not a look")
    );
    assert!(err(&mut s, json!({ "id": "nope", "name": "x" })).contains("no Visual filter"));
    assert_eq!(s.visual_filters().len(), 3, "nothing was saved");
    let ok = s
        .run(
            "filter.save",
            &json!({ "name": "Red letter", "marked": "words_of_christ" }),
        )
        .unwrap();
    assert_eq!(ok["marked"], "words_of_christ");
}

#[test]
fn the_tab_header_switches_a_filter_for_one_tab_and_layouts_keep_the_switch() {
    let (mut s, _t) = launch("tabs");
    let kjv = open(&mut s, "kjv", false);
    let tagnt = open(&mut s, "tagnt", true);
    assert!(shown(&s, kjv).is_empty());
    let out = s
        .run(
            "tab.visual_filter",
            &json!({ "tab": kjv.0, "id": "imperatives" }),
        )
        .unwrap();
    assert_eq!(out, json!({ "on": true, "shown": ["imperatives"] }));
    assert!(shown(&s, tagnt).is_empty(), "only that Tab");

    // A filter on in all Tabs shows everywhere, until a Tab switches it off.
    s.run(
        "filter.save",
        &json!({ "id": "divine-names", "all_tabs": true }),
    )
    .unwrap();
    assert_eq!(shown(&s, kjv), ["divine-names", "imperatives"]);
    assert_eq!(shown(&s, tagnt), ["divine-names"]);
    s.run(
        "tab.visual_filter",
        &json!({ "tab": tagnt.0, "id": "divine-names", "on": false }),
    )
    .unwrap();
    assert!(shown(&s, tagnt).is_empty());

    s.run("layout.save", &json!({ "name": "Filters" })).unwrap();
    s.run(
        "tab.visual_filter",
        &json!({ "tab": kjv.0, "id": "imperatives", "on": false }),
    )
    .unwrap();
    s.run(
        "tab.visual_filter",
        &json!({ "tab": tagnt.0, "id": "divine-names", "on": true }),
    )
    .unwrap();
    s.run("layout.restore", &json!({ "name": "Filters" }))
        .unwrap();
    let by_resource = |s: &Session, id: &str| {
        let tab = s
            .workspace
            .tabs()
            .find(|t| t.kind == TabKind::Resource(id.into()))
            .unwrap()
            .id;
        shown(s, tab)
    };
    assert_eq!(by_resource(&s, "kjv"), ["divine-names", "imperatives"]);
    assert!(by_resource(&s, "tagnt").is_empty());

    // The Last session keeps them; a Tab that switched nothing saves nothing extra.
    let snap = s.workspace.snapshot();
    let back = WorkspaceSnapshot::from_json(&snap.to_json()).unwrap();
    assert_eq!(back, snap);
    let plain = TabSnapshot {
        visual_filters: TabFilters::default(),
        ..snap.tabs[0].clone()
    };
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("visual_filters")
    );

    // Deleting a filter forgets every Tab's switch of it.
    s.run("filter.delete", &json!({ "id": "imperatives" }))
        .unwrap();
    assert_eq!(by_resource(&s, "kjv"), ["divine-names"]);
    assert!(
        s.workspace
            .tabs()
            .all(|t| !t.visual_filters.switched.contains("imperatives"))
    );
    // Only Resource Tabs have text to colour.
    let search = TabId(
        s.run("tool.search", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert!(
        s.run(
            "tab.visual_filter",
            &json!({ "tab": search.0, "id": "divine-names" })
        )
        .is_err()
    );
}

// ------------------------------------------------------------------------------- matching

/// The words of `unit` in `hits` of one filter, as written.
fn words<'a>(unit: &'a TextUnit, hits: &[Range<usize>]) -> Vec<&'a str> {
    hits.iter().map(|r| &unit.text[r.clone()]).collect()
}

/// Every Verse of a Resource numbered like the Standard versification.
fn slices(r: &Resource) -> Vec<DocumentSlice> {
    r.verses
        .iter()
        .map(|v| DocumentSlice {
            range: v.start as usize..v.end as usize,
            standard: Passage::new(
                v.verse,
                Verse::new(v.verse.book, v.verse.chapter, v.last.max(v.verse.verse)),
            )
            .ok(),
            book: Some(v.verse.book),
        })
        .collect()
}

/// The aligned words at a Standard Passage of `original` (numbered like the Standard).
fn aligned(original: &Resource) -> impl FnMut(Passage) -> AlignedWords + '_ {
    move |p: Passage| {
        let mut out = Vec::new();
        for v in &original.verses {
            if v.verse < p.start() || v.verse > p.end() {
                continue;
            }
            let unit = original.units.iter().find(|u| u.id == v.unit).unwrap();
            for s in &unit.spans {
                if let SpanKind::TaggedWord { parts } = &s.kind
                    && v.start <= s.start
                    && s.end <= v.end
                {
                    out.push(parts.clone());
                }
            }
        }
        out
    }
}

fn filter(name: &str, query: &str, scope: &str) -> VisualFilter {
    VisualFilter {
        id: name.to_lowercase(),
        name: name.into(),
        query: query.into(),
        marked: None,
        scope: scope.into(),
        look: HighlightLook::Underline,
        color: Rgb::new(20, 130, 120),
        all_tabs: false,
    }
}

/// What `filters` colour in the whole of `r`'s first unit, one list of words per filter.
fn coloured<'a>(
    r: &'a Resource,
    filters: &[VisualFilter],
    align: bool,
    original: &mut dyn FnMut(Passage) -> AlignedWords,
) -> Vec<Vec<&'a str>> {
    let matchers: Vec<FilterMatcher> = filters
        .iter()
        .map(|f| FilterMatcher::new(f, &obs_search::NoLexicon).unwrap())
        .collect();
    let unit = &r.units[0];
    let hits = block_hits(
        &matchers,
        unit,
        0..unit.text.len(),
        &slices(r),
        align,
        original,
    );
    (0..filters.len())
        .map(|k| {
            let ranges: Vec<Range<usize>> = hits
                .iter()
                .filter(|h| h.filter == k)
                .map(|h| h.range.clone())
                .collect();
            words(unit, &ranges)
        })
        .collect()
}

/// Acceptance (#211): a filter for "imperatives in Romans 12" colours the right words in
/// TAGNT and, through Strong's alignment, in the KJV, whose words carry only Strong's numbers.
#[test]
fn imperatives_in_romans_12_colour_tagnt_and_through_strongs_alignment_the_kjv() {
    let tagnt = tagnt_romans_12();
    let kjv = kjv_romans_12();
    let rom_12 = filter("Imperatives in Romans 12", "@imperative", "Rom 12");
    let mut none = |_: Passage| -> AlignedWords { panic!("TAGNT is not aligned") };
    let greek = coloured(&tagnt, std::slice::from_ref(&rom_12), false, &mut none);
    assert_eq!(
        greek[0],
        [
            "συσχηματίζεσθε",
            "μεταμορφοῦσθε",
            "εὐλογεῖτε",
            "εὐλογεῖτε",
            "καταρᾶσθε"
        ],
        "every imperative of Romans 12, and neither of 13:14's"
    );
    let mut lookups = 0;
    let mut original = aligned(&tagnt);
    let mut counted = |p: Passage| {
        lookups += 1;
        original(p)
    };
    let english = coloured(&kjv, std::slice::from_ref(&rom_12), true, &mut counted);
    assert_eq!(
        english[0],
        ["be", "conformed", "transformed", "Bless", "bless", "curse"],
        "the KJV words rendering those Greek imperatives: G4964 (be conformed), G3339, G2127 \
         twice, G2672; not 12:14's participle \"persecute\" (G1377), nor anything in 13:14"
    );
    assert_eq!(
        lookups, 3,
        "one lookup per Verse in scope (12:2, 12:13, 12:14)"
    );

    // Without the scope the same filter reaches 13:14 too: "put ye on", "make".
    let everywhere = filter("Imperatives", "@imperative", "");
    let english = coloured(&kjv, &[everywhere], true, &mut aligned(&tagnt));
    assert_eq!(
        english[0],
        [
            "be",
            "conformed",
            "transformed",
            "Bless",
            "bless",
            "curse",
            "put",
            "on",
            "make"
        ]
    );
    // A Bible that is not aligned colours only what its own words carry.
    let mut never = |_: Passage| -> AlignedWords { panic!("not aligned") };
    let english = coloured(&kjv, &[rom_12], false, &mut never);
    assert!(english[0].is_empty());
}

#[test]
fn several_filters_colour_their_own_words_and_strongs_numbers_need_no_alignment() {
    let kjv = kjv_romans_12();
    let tagnt = tagnt_romans_12();
    let filters = [
        filter("Bless", "bless", ""),
        filter("Not", "strong:G3361", "Rom 12:14"),
        filter("Aorist imperatives", "@aorist imperative", ""),
    ];
    let mut original = aligned(&tagnt);
    let hits = coloured(&kjv, &filters, true, &mut original);
    assert_eq!(hits[0], ["Bless", "bless"]);
    assert_eq!(hits[1], ["not"], "12:14's only, by its own Strong's number");
    assert_eq!(
        hits[2],
        ["put", "on"],
        "ἐνδύσασθε is aorist; ποιεῖσθε (make) is present"
    );
}

#[test]
fn marked_text_colours_the_words_of_christ_and_divine_names_the_resource_marks() {
    let mut r = kjv_romans_12();
    let unit = &mut r.units[0];
    let say = unit.text.find("Bless them").unwrap();
    let lord = unit.text.find("Lord").unwrap();
    unit.spans.push(obs_model::Span {
        start: say as u32,
        end: (say + "Bless them".len()) as u32,
        kind: SpanKind::Woj,
    });
    unit.spans.push(obs_model::Span {
        start: lord as u32,
        end: (lord + 4) as u32,
        kind: SpanKind::DivineName,
    });
    let built_in = built_in_filters();
    assert_eq!(built_in[0].marked, Some(MarkedText::WordsOfChrist));
    let tagnt = tagnt_romans_12();
    let mut original = aligned(&tagnt);
    let hits = coloured(&r, &built_in[..2], true, &mut original);
    assert_eq!(hits[0], ["Bless them"]);
    assert_eq!(
        hits[1],
        ["God", "Lord"],
        "θεός by its Strong's number (G2316), \"Lord\" as the Resource marks it"
    );
}

#[test]
fn a_block_colours_only_its_own_bytes() {
    let kjv = kjv_romans_12();
    let tagnt = tagnt_romans_12();
    let unit = &kjv.units[0];
    let v14 = kjv
        .verses
        .iter()
        .find(|v| v.verse.verse == 14)
        .map(|v| v.start as usize..v.end as usize)
        .unwrap();
    let matcher =
        FilterMatcher::new(&filter("Imp", "@imperative", ""), &obs_search::NoLexicon).unwrap();
    let hits = block_hits(
        &[matcher],
        unit,
        v14.clone(),
        &slices(&kjv),
        true,
        &mut aligned(&tagnt),
    );
    let ranges: Vec<Range<usize>> = hits.into_iter().map(|h| h.range).collect();
    assert!(
        ranges
            .iter()
            .all(|r| v14.start <= r.start && r.end <= v14.end)
    );
    assert_eq!(words(unit, &ranges), ["Bless", "bless", "curse"]);
}

/// Review of #211: a filter reads its query as the Search Tool does, Verse by Verse, so an
/// excluded word, a word the Verse lacks or a proximity it misses colours nothing.
#[test]
fn a_filter_colours_only_verses_that_satisfy_the_whole_query() {
    let kjv = kjv_romans_12();
    let tagnt = tagnt_romans_12();
    let colour = |query: &str, scope: &str| -> Vec<String> {
        let mut original = aligned(&tagnt);
        coloured(&kjv, &[filter("F", query, scope)], true, &mut original)[0]
            .iter()
            .map(|w| w.to_string())
            .collect()
    };
    // NOT: 12:14 says "persecute", so nothing in it is coloured.
    assert!(colour("bless NOT persecute", "Rom 12:14").is_empty());
    assert!(colour("bless -persecute", "").is_empty());
    assert_eq!(colour("bless NOT hospitality", ""), ["Bless", "bless"]);
    // Implicit AND: both words in one Verse.
    assert!(
        colour("bless world", "").is_empty(),
        "\"world\" is in 12:2, \"bless\" in 12:14"
    );
    assert_eq!(colour("bless curse", ""), ["Bless", "bless", "curse"]);
    assert_eq!(colour("world OR curse", ""), ["world", "curse"]);
    // Proximity: only the occurrences close enough, counted as the Search index counts.
    assert_eq!(colour("bless NEAR/1 them", ""), ["Bless", "them"]);
    assert_eq!(colour("curse NEAR/1 bless", ""), ["bless", "curse"]);
    assert!(colour("curse BEFORE/1 bless", "").is_empty());
    assert_eq!(colour("bless BEFORE/1 curse", ""), ["bless", "curse"]);
    assert!(colour("persecute NEAR/0 curse", "").is_empty());
    // WITHIN n verses: neighbouring Verses (12:13 and 12:14; 12:2 is two Verses away).
    assert_eq!(
        colour("hospitality WITHIN 2 verses curse", ""),
        ["hospitality", "curse"]
    );
    assert!(colour("world WITHIN 2 verses curse", "").is_empty());
    assert_eq!(
        colour("world WITHIN 3 verses curse", ""),
        ["world", "curse"]
    );
    // Aligned terms obey the same operators.
    assert_eq!(
        colour("@imperative NOT persecute", "Rom 12"),
        ["be", "conformed", "transformed"]
    );
    assert_eq!(
        colour("@imperative bless", ""),
        ["Bless", "bless", "curse"],
        "12:14 only: the Verse with both"
    );
}

/// `WITHIN n verses` reads the Verses around a block, but colours only the block's bytes.
#[test]
fn within_verses_reads_neighbouring_verses_outside_the_block() {
    let kjv = kjv_romans_12();
    let unit = &kjv.units[0];
    let v14 = kjv
        .verses
        .iter()
        .find(|v| v.verse.verse == 14)
        .map(|v| v.start as usize..v.end as usize)
        .unwrap();
    let matcher = FilterMatcher::new(
        &filter("W", "hospitality WITHIN 2 verses curse", ""),
        &obs_search::NoLexicon,
    )
    .unwrap();
    assert_eq!(matcher.context_documents(), 1);
    let mut never = |_: Passage| -> AlignedWords { panic!("not aligned") };
    let hits = block_hits(&[matcher], unit, v14, &slices(&kjv), false, &mut never);
    let ranges: Vec<Range<usize>> = hits.into_iter().map(|h| h.range).collect();
    assert_eq!(words(unit, &ranges), ["curse"]);
}

/// Review of #211: a deleted filter's id is never given to a new one, so a Layout that
/// switched the deleted filter on does not switch on an unrelated new filter, even after a
/// relaunch.
#[test]
fn a_deleted_filters_id_is_never_reused_so_restored_layouts_do_not_switch_new_filters() {
    let (mut s, t) = launch("ids");
    let kjv = open(&mut s, "kjv", false);
    let made = s
        .run("filter.save", &json!({ "name": "Love", "query": "love" }))
        .unwrap();
    assert_eq!(made["id"], "filter-1");
    s.run(
        "tab.visual_filter",
        &json!({ "tab": kjv.0, "id": "filter-1", "on": true }),
    )
    .unwrap();
    s.run("layout.save", &json!({ "name": "Love" })).unwrap();
    s.run("filter.delete", &json!({ "id": "filter-1" }))
        .unwrap();
    let other = s
        .run("filter.save", &json!({ "name": "Grace", "query": "grace" }))
        .unwrap();
    assert_eq!(other["id"], "filter-2", "filter-1 is not given again");
    s.run("layout.restore", &json!({ "name": "Love" })).unwrap();
    let kjv = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap()
        .id;
    assert!(
        shown(&s, kjv).is_empty(),
        "the Layout's switch of the deleted filter does not reach the new one"
    );

    // The counter is kept in the User database: deleting the newest filter and relaunching
    // does not give its id again either.
    s.run("filter.delete", &json!({ "id": "filter-2" }))
        .unwrap();
    let mut s = reopen(&t.0);
    let third = s
        .run("filter.save", &json!({ "name": "Peace", "query": "peace" }))
        .unwrap();
    assert_eq!(third["id"], "filter-3");
}

/// #290: a hand-edited User database whose filter has the id `filter-<u64::MAX>` (which also
/// puts the counter there at launch) neither panics nor collides on the next new filter: the
/// counter wraps to the first number no filter's id has.
#[test]
fn a_filter_id_at_the_maximum_wraps_the_counter_to_a_free_number() {
    let (mut s, t) = launch("max-id");
    for (name, query) in [("Love", "love"), ("Grace", "grace")] {
        s.run("filter.save", &json!({ "name": name, "query": query }))
            .unwrap();
    }
    let user = s.user.as_ref().unwrap();
    let list = user
        .preference(crate::visual_filters::VISUAL_FILTERS_PREF)
        .unwrap()
        .unwrap();
    let max = format!("filter-{}", u64::MAX);
    user.set_preference(
        crate::visual_filters::VISUAL_FILTERS_PREF,
        &list.replace("\"filter-2\"", &format!("\"{max}\"")),
    )
    .unwrap();
    let mut s = reopen(&t.0);
    assert_eq!(s.visual_filters.next_id, u64::MAX);
    let mut made = Vec::new();
    for (name, query) in [("Peace", "peace"), ("Joy", "joy")] {
        let out = s
            .run("filter.save", &json!({ "name": name, "query": query }))
            .unwrap();
        made.push(out["id"].as_str().unwrap().to_string());
    }
    assert_eq!(made, ["filter-2", "filter-3"]);
    let ids: Vec<&str> = s
        .visual_filters
        .list
        .iter()
        .map(|f| f.id.as_str())
        .filter(|id| id.starts_with("filter-"))
        .collect();
    assert_eq!(ids, ["filter-1", &max, "filter-2", "filter-3"]);
}

// ------------------------------------------------------- review of #211: scope and reach

/// A unit of one word per Verse (Romans 1:1, 1:2, …), and its Verses as documents.
fn one_word_verses(words: &[&str]) -> (TextUnit, Vec<DocumentSlice>) {
    let mut text = String::new();
    let mut slices = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if !text.is_empty() {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(w);
        let v = Verse::new(
            obs_scripture::Book::Romans,
            1,
            u16::try_from(i + 1).unwrap(),
        );
        slices.push(DocumentSlice {
            range: start..text.len(),
            standard: Some(Passage::new(v, v).unwrap()),
            book: Some(v.book),
        });
    }
    let unit = TextUnit {
        id: obs_model::UnitId::new("ROM").unwrap(),
        kind: obs_model::UnitKind::BibleBook,
        title: None,
        text,
        spans: Vec::new(),
    };
    (unit, slices)
}

/// The words one filter colours in `block`, given `documents`.
fn colour_block(
    unit: &TextUnit,
    f: &VisualFilter,
    block: Range<usize>,
    documents: &[DocumentSlice],
) -> Vec<String> {
    let matcher = FilterMatcher::new(f, &obs_search::NoLexicon).unwrap();
    let mut never = |_: Passage| -> AlignedWords { panic!("not aligned") };
    let hits = block_hits(&[matcher], unit, block, documents, false, &mut never);
    let ranges: Vec<Range<usize>> = hits.into_iter().map(|h| h.range).collect();
    words(unit, &ranges).into_iter().map(String::from).collect()
}

/// A dictionary entry as the Search index test reads it: a heading and two paragraphs.
fn grace_entry() -> TextUnit {
    let text = "GRACE\nFavour freely shown.\nThe grace of God that bringeth salvation.";
    let para = |r: Range<usize>| obs_model::Span {
        start: r.start as u32,
        end: r.end as u32,
        kind: SpanKind::Para {
            style: obs_model::ParaStyle::Prose,
        },
    };
    let p1 = text.find("Favour").unwrap()..text.find('.').unwrap() + 1;
    let p2 = text.find("The").unwrap()..text.len();
    TextUnit {
        id: obs_model::UnitId::new("Grace").unwrap(),
        kind: obs_model::UnitKind::Entry,
        title: Some("Grace".into()),
        text: text.into(),
        spans: vec![
            obs_model::Span {
                start: 0,
                end: 5,
                kind: SpanKind::Heading { level: 1 },
            },
            para(p1),
            para(p2),
        ],
    }
}

/// A filter limited to Passages colours nothing outside the Verses in them: not a
/// paragraph outside Bibles, not a block with no Verse, its marked text neither.
#[test]
fn a_scoped_filter_colours_nothing_outside_the_verses_in_its_passages() {
    let mut unit = grace_entry();
    let all = 0..unit.text.len();
    unit.spans.push(obs_model::Span {
        start: 6,
        end: 12,
        kind: SpanKind::Woj,
    });
    let paragraphs = paragraph_slices(&unit, all.clone(), 0);
    assert_eq!(paragraphs.len(), 3, "the heading and both paragraphs");
    let grace = filter("Grace", "grace", "Rom 12");
    assert!(colour_block(&unit, &grace, all.clone(), &paragraphs).is_empty());
    assert!(colour_block(&unit, &grace, all.clone(), &[]).is_empty());
    let red = VisualFilter {
        marked: Some(MarkedText::WordsOfChrist),
        ..filter("Red", "", "Rom 12")
    };
    assert!(colour_block(&unit, &red, all.clone(), &paragraphs).is_empty());
    // An Unmapped verse is in no Passage either.
    let unmapped = [DocumentSlice {
        range: all.clone(),
        standard: None,
        book: Some(obs_scripture::Book::Romans),
    }];
    assert!(colour_block(&unit, &grace, all.clone(), &unmapped).is_empty());
    // Without a scope both colour there.
    assert_eq!(
        colour_block(
            &unit,
            &filter("Grace", "grace", ""),
            all.clone(),
            &paragraphs
        ),
        ["GRACE", "grace"]
    );
    assert_eq!(
        colour_block(
            &unit,
            &VisualFilter {
                scope: String::new(),
                ..red
            },
            all,
            &paragraphs
        ),
        ["Favour"]
    );
}

/// `WITHIN n verses` counts the Verses left out of a filter's scope, as the Search index
/// numbers its documents: Romans 1:1 and 1:3 are two Verses apart with 1:2 out of scope.
#[test]
fn within_verses_counts_the_verses_a_scope_leaves_out() {
    let (unit, verses) = one_word_verses(&["alpha", "beta", "gamma"]);
    let all = 0..unit.text.len();
    let scoped = |q: &str| filter("F", q, "Rom 1:1; Rom 1:3");
    assert!(
        colour_block(
            &unit,
            &scoped("alpha WITHIN 2 verses gamma"),
            all.clone(),
            &verses
        )
        .is_empty()
    );
    assert_eq!(
        colour_block(
            &unit,
            &scoped("alpha WITHIN 3 verses gamma"),
            all.clone(),
            &verses
        ),
        ["alpha", "gamma"]
    );
    assert!(
        colour_block(&unit, &scoped("alpha WITHIN 2 verses beta"), all, &verses).is_empty(),
        "1:2 is out of scope"
    );
}

/// A nested `WITHIN` reads as far as its reaches added: a block holding only 1:3 reads 1:1
/// for `(alpha WITHIN 2 verses beta) WITHIN 2 verses gamma`, and colours what reading the
/// whole unit colours there.
#[test]
fn nested_within_reads_the_verses_its_reaches_add_up_to() {
    let (unit, verses) = one_word_verses(&["alpha", "beta", "gamma", "delta"]);
    let nested = filter(
        "N",
        "(alpha WITHIN 2 verses beta) WITHIN 2 verses gamma",
        "",
    );
    let matcher = FilterMatcher::new(&nested, &obs_search::NoLexicon).unwrap();
    assert_eq!(matcher.context_documents(), 2);
    let whole = colour_block(&unit, &nested, 0..unit.text.len(), &verses);
    assert_eq!(
        whole,
        ["beta", "gamma"],
        "alpha is two Verses from gamma: not within the outer WITHIN"
    );
    for (i, v) in verses.iter().enumerate() {
        // As a pane passes them: the block's Verse and `context_documents` on either side.
        let window = &verses[i.saturating_sub(2)..(i + 3).min(verses.len())];
        let block = colour_block(&unit, &nested, v.range.clone(), window);
        let expected: Vec<String> = whole
            .iter()
            .filter(|w| unit.text[v.range.clone()] == **w)
            .cloned()
            .collect();
        assert_eq!(block, expected, "Verse {}", i + 1);
    }
}

/// Outside Bibles `WITHIN n paragraphs` joins neighbouring paragraphs of the unit, as the
/// Search index does, each block colouring only its own words.
#[test]
fn within_paragraphs_reads_the_paragraphs_around_a_block() {
    let unit = grace_entry();
    let f = filter("F", "favour WITHIN 2 paragraphs salvation", "");
    let context = FilterMatcher::new(&f, &obs_search::NoLexicon)
        .unwrap()
        .context_documents();
    let favour = unit.text.find("Favour").unwrap()..unit.text.find('.').unwrap() + 1;
    let the = unit.text.find("The").unwrap()..unit.text.len();
    // The heading, "Favour …" and "The grace …" are three paragraphs.
    for (block, word, read) in [(favour, "Favour", 3), (the, "salvation", 2)] {
        let documents = paragraph_slices(&unit, block.clone(), context);
        assert_eq!(documents.len(), read, "{word}");
        assert_eq!(colour_block(&unit, &f, block, &documents), [word]);
    }
    let both = filter("F", "favour salvation", "");
    let all = 0..unit.text.len();
    assert!(
        colour_block(&unit, &both, all.clone(), &paragraph_slices(&unit, all, 0)).is_empty(),
        "each paragraph is its own document"
    );
}

// ------------------------------------- review of #211: proximity, headings and local tags

/// The Verses (the Bible's own numbering) the Search index finds `query` in.
fn search_verses(r: &Resource, query: &str) -> Vec<Verse> {
    use obs_search::SearchIndex;
    let dir = std::env::temp_dir().join(format!(
        "obs-filters-search-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let path = obs_store::build_search_index(r, 1, &dir, &mut |_, _| true).unwrap();
    let index = obs_store::SqliteSearchIndex::open(&path, None).unwrap();
    let compiled = obs_search::to_fts5(query, &obs_search::NoLexicon).unwrap();
    let hits = index
        .search(&compiled, &obs_search::SearchOptions::default())
        .unwrap();
    drop(index);
    let _ = std::fs::remove_dir_all(&dir);
    hits.into_iter()
        .map(|h| match h.key {
            Some(obs_model::Key::Passage(p)) => p.start(),
            other => panic!("a Verse, not {other:?}"),
        })
        .collect()
}

/// The Verses of `r`'s first unit in which a filter of `query` colours a word, each read as
/// its own block (with every Verse as context), and the words it colours in all of them.
fn filter_verses<'a>(r: &'a Resource, query: &str) -> (Vec<Verse>, Vec<&'a str>) {
    filter_verses_reading(r, query, &slices(r))
}

/// [`filter_verses`] with `documents` as the Verses of `r`'s first unit.
fn filter_verses_reading<'a>(
    r: &'a Resource,
    query: &str,
    documents: &[DocumentSlice],
) -> (Vec<Verse>, Vec<&'a str>) {
    let matcher = FilterMatcher::new(&filter("F", query, ""), &obs_search::NoLexicon).unwrap();
    let unit = &r.units[0];
    let mut verses = Vec::new();
    let mut ranges = Vec::new();
    for v in &r.verses {
        let mut never = |_: Passage| -> AlignedWords { panic!("not aligned") };
        let block = v.start as usize..v.end as usize;
        let hits = block_hits(
            std::slice::from_ref(&matcher),
            unit,
            block,
            documents,
            false,
            &mut never,
        );
        if !hits.is_empty() {
            verses.push(v.verse);
        }
        ranges.extend(hits.into_iter().map(|h| h.range));
    }
    (verses, words(unit, &ranges))
}

/// Review of #211: proximity pairs each word or phrase on its own, as the Search index does,
/// even where alternatives overlap: in 12:14 `"bless them" OR bless` has "Bless" just before
/// "them", though "Bless them" holds it.
#[test]
fn proximity_pairs_overlapping_alternatives_one_by_one_as_search_does() {
    let kjv = kjv_romans_12();
    let rom = |v| Verse::new(obs_scripture::Book::Romans, 12, v);
    for (query, verses, coloured) in [
        (
            "(\"bless them\" OR bless) BEFORE/0 them",
            vec![rom(14)],
            vec!["Bless", "them"],
        ),
        (
            "(\"bless them\" OR bless) NEAR/0 them",
            vec![rom(14)],
            // NEAR pairs "Bless them" with the "them" inside it too, as FTS5 does.
            vec!["Bless them"],
        ),
        (
            "(\"bless them\" OR bless) BEFORE/1 which",
            vec![rom(14)],
            // Both "Bless them" and the "Bless" in it, drawn as one range.
            vec!["Bless them", "which"],
        ),
    ] {
        assert_eq!(search_verses(&kjv, query), verses, "Search: {query}");
        let (found, words) = filter_verses(&kjv, query);
        assert_eq!(found, verses, "filter: {query}");
        assert_eq!(words, coloured, "{query}");
    }
}

/// A Bible Verse holding a heading, as some Bibles place a section heading inside the Verse
/// before it: Romans 1:1 "Bless Persecution bless", "Persecution" a heading, and 1:2
/// "persecution of saints".
fn verse_with_heading() -> Resource {
    let rom = |v| Verse::new(obs_scripture::Book::Romans, 1, v);
    let mut r = crate::testing::tagged_bible(
        "headed",
        "Headed Bible",
        "HB",
        "en",
        &[
            (rom(1), "Bless Persecution bless"),
            (rom(2), "persecution of saints"),
        ],
    );
    let unit = &mut r.units[0];
    let at = unit.text.find("Persecution").unwrap();
    unit.spans.push(obs_model::Span {
        start: at as u32,
        end: (at + "Persecution".len()) as u32,
        kind: SpanKind::Heading { level: 2 },
    });
    r
}

/// Review of #211: a Verse's headings are not read, as the Search index leaves them out:
/// they neither satisfy nor exclude a term, nor stand between the words of a phrase or of
/// a proximity.
#[test]
fn a_verses_headings_are_read_out_as_search_reads_them() {
    let r = verse_with_heading();
    let rom = |v| Verse::new(obs_scripture::Book::Romans, 1, v);
    for (query, verses, coloured) in [
        (
            "bless NOT persecution",
            vec![rom(1)],
            vec!["Bless", "bless"],
        ),
        ("persecution", vec![rom(2)], vec!["persecution"]),
        ("\"bless bless\"", vec![rom(1)], vec!["Bless", "bless"]),
        ("bless NEAR/0 bless", vec![rom(1)], vec!["Bless", "bless"]),
        ("bless BEFORE/0 bless", vec![rom(1)], vec!["Bless", "bless"]),
        ("bless persecution", vec![], vec![]),
    ] {
        assert_eq!(search_verses(&r, query), verses, "Search: {query}");
        let (found, words) = filter_verses(&r, query);
        assert_eq!(found, verses, "filter: {query}");
        assert_eq!(words, coloured, "{query}");
    }
}

/// Review of #211: Strong's alignment fills in only what a word lacks. A Tagged word with
/// Morphology of its own is read by it: here the KJV's first "Bless" of 12:14 is tagged as a
/// participle and "persecute" as an imperative, against what TAGNT says of G2127 and G1377.
#[test]
fn alignment_reaches_only_words_without_tags_of_their_own() {
    let mut kjv = kjv_romans_12();
    let tagnt = tagnt_romans_12();
    let unit = &mut kjv.units[0];
    let v14 = unit.text.find("Bless them").unwrap();
    let persecute = unit.text.find("persecute").unwrap();
    for s in &mut unit.spans {
        let morph = match s.start as usize {
            at if at == v14 => "V-PAP-NPM",
            at if at == persecute => "V-PAM-2P",
            _ => continue,
        };
        let SpanKind::TaggedWord { parts } = &mut s.kind else {
            continue;
        };
        parts[0].morph = Some(morph.into());
    }
    let rom_12 = filter("Imperatives in Romans 12", "@imperative", "Rom 12");
    let english = coloured(&kjv, &[rom_12], true, &mut aligned(&tagnt));
    assert_eq!(
        english[0],
        [
            "be",
            "conformed",
            "transformed",
            "persecute",
            "bless",
            "curse"
        ],
        "the participle \"Bless\" is left plain, the imperative \"persecute\" coloured by its \
         own tag; the untagged words still through TAGNT"
    );

    // A Bible whose words all carry Morphology never asks for the original-language Bible.
    let mut never = |_: Passage| -> AlignedWords { panic!("TAGNT reads its own tags") };
    let greek = coloured(
        &tagnt,
        &[filter("Imp", "@imperative", "Rom 12")],
        true,
        &mut never,
    );
    assert_eq!(greek[0].len(), 5);
}

/// Review of #211: a query asking for a Lemma and Morphology at once takes from alignment
/// only the one a word lacks, and the one it has must still meet the query. In the KJV's
/// 12:14 the first "Bless" has Morphology (a participle) and no Lemma, the second "bless" a
/// Lemma and no Morphology, "persecute" Morphology (an imperative) and "curse" a Lemma
/// (another) of their own.
#[test]
fn a_lemma_and_morphology_query_aligns_only_the_field_a_word_lacks() {
    let tagnt = tagnt_romans_12();
    let mut kjv = kjv_romans_12();
    let unit = &mut kjv.units[0];
    let first_bless = unit.text.find("Bless them").unwrap();
    let second_bless = unit.text.find("bless,").unwrap();
    let persecute = unit.text.find("persecute").unwrap();
    let curse = unit.text.find("curse").unwrap();
    for s in &mut unit.spans {
        let at = s.start as usize;
        let SpanKind::TaggedWord { parts } = &mut s.kind else {
            continue;
        };
        let part = &mut parts[0];
        match at {
            at if at == first_bless => part.morph = Some("V-PAP-NPM".into()),
            at if at == persecute => part.morph = Some("V-PAM-2P".into()),
            at if at == second_bless => part.lemma = Some("εὐλογέω".into()),
            at if at == curse => part.lemma = Some("ἀρά".into()),
            _ => {}
        }
    }
    for (query, expected, why) in [
        (
            "lemma:εὐλογέω @imperative",
            vec!["bless"],
            "the participle \"Bless\" keeps its own Morphology though TAGNT's G2127 is an \
             imperative; the second takes only the Morphology it lacks",
        ),
        (
            "lemma:διώκω @imperative",
            vec!["persecute"],
            "its own imperative Morphology with TAGNT's Lemma, though TAGNT's διώκοντας is a \
             participle",
        ),
        (
            "lemma:καταράομαι @imperative",
            vec![],
            "\"curse\" keeps its own Lemma though TAGNT's G2672 is καταράομαι",
        ),
    ] {
        let found = coloured(&kjv, &[filter("F", query, "")], true, &mut aligned(&tagnt));
        assert_eq!(found[0], expected, "{query}: {why}");
    }
    // Without the local tags the same queries read TAGNT's words whole.
    let plain = kjv_romans_12();
    for (query, expected) in [
        ("lemma:εὐλογέω @imperative", vec!["Bless", "bless"]),
        ("lemma:διώκω @imperative", vec![]),
        ("lemma:καταράομαι @imperative", vec!["curse"]),
    ] {
        let found = coloured(
            &plain,
            &[filter("F", query, "")],
            true,
            &mut aligned(&tagnt),
        );
        assert_eq!(found[0], expected, "{query}");
    }
}

/// Review of #211: `NEAR/n` pairs operands as FTS5's NEAR does, overlapping ones too:
/// "them" lies inside "Bless them", so `"bless them" NEAR/0 them` finds 12:14 in Search and
/// the filter colours it; `BEFORE/n` still wants the second after the first.
#[test]
fn near_pairs_overlapping_operands_as_search_does() {
    let kjv = kjv_romans_12();
    let rom = |v| Verse::new(obs_scripture::Book::Romans, 12, v);
    for (query, verses, coloured) in [
        (
            "\"bless them\" NEAR/0 them",
            vec![rom(14)],
            vec!["Bless them"],
        ),
        (
            "them NEAR/0 \"bless them\"",
            vec![rom(14)],
            vec!["Bless them"],
        ),
        (
            "\"them which\" NEAR/0 \"bless them\"",
            vec![rom(14)],
            vec!["Bless them which"],
        ),
        ("\"bless them\" BEFORE/0 them", vec![], vec![]),
        ("them BEFORE/0 \"bless them\"", vec![], vec![]),
    ] {
        assert_eq!(search_verses(&kjv, query), verses, "Search: {query}");
        let (found, words) = filter_verses(&kjv, query);
        assert_eq!(found, verses, "filter: {query}");
        assert_eq!(words, coloured, "{query}");
    }
}

/// Review of #211: Strong's alignment does not reach a word in one of the Verse's headings,
/// as no term does: with the KJV's first "Bless" of 12:14 marked as a heading, TAGNT's
/// imperative G2127 colours only the second.
#[test]
fn alignment_leaves_a_verses_headings_out() {
    let tagnt = tagnt_romans_12();
    let mut kjv = kjv_romans_12();
    let unit = &mut kjv.units[0];
    let first_bless = unit.text.find("Bless them").unwrap();
    unit.spans.push(obs_model::Span {
        start: first_bless as u32,
        end: (first_bless + "Bless".len()) as u32,
        kind: SpanKind::Heading { level: 2 },
    });
    let rom_12 = filter("Imperatives in Romans 12", "@imperative", "Rom 12");
    let english = coloured(&kjv, &[rom_12], true, &mut aligned(&tagnt));
    assert_eq!(
        english[0],
        ["be", "conformed", "transformed", "bless", "curse"],
        "the heading \"Bless\" is left plain"
    );
}

/// Review of #211: `WITHIN n verses` joins Verses of one book of the Bible whether or not
/// they are mapped to a Standard Passage, as the Search index joins them by the Bible's own
/// book: Romans 1:1 is mapped and 1:2 Unmapped, and both are found.
#[test]
fn within_verses_joins_a_mapped_verse_and_an_unmapped_one_as_search_does() {
    let rom = |v| Verse::new(obs_scripture::Book::Romans, 1, v);
    let r = crate::testing::tagged_bible(
        "unmapped",
        "Unmapped Bible",
        "UB",
        "en",
        &[(rom(1), "alpha"), (rom(2), "beta")],
    );
    let query = "alpha WITHIN 2 verses beta";
    assert_eq!(
        search_verses(&r, query),
        [rom(1), rom(2)],
        "Search: {query}"
    );
    let mut documents = slices(&r);
    assert_eq!(documents.len(), 2);
    documents[1].standard = None;
    for reading in [slices(&r), documents] {
        let (found, words) = filter_verses_reading(&r, query, &reading);
        assert_eq!(
            found,
            [rom(1), rom(2)],
            "filter: {query} reading {reading:?}"
        );
        assert_eq!(words, ["alpha", "beta"]);
    }
    // Verses of two books are not joined, mapped or not.
    let two_books = crate::testing::tagged_bible(
        "books",
        "Two Books",
        "TB",
        "en",
        &[
            (rom(1), "alpha"),
            (
                Verse::new(obs_scripture::Book::FirstCorinthians, 1, 1),
                "beta",
            ),
        ],
    );
    assert!(search_verses(&two_books, query).is_empty());
    let (unit, mut verses) = one_word_verses(&["alpha", "beta"]);
    for standard in [None, verses[0].standard] {
        verses[1].book = Some(obs_scripture::Book::FirstCorinthians);
        verses[1].standard = standard;
        let all = 0..unit.text.len();
        let within = filter("F", query, "");
        assert!(colour_block(&unit, &within, all, &verses).is_empty());
    }
}

/// Review of #211: a Tagged word in one of a Verse's headings is left out as Search leaves
/// it out (its Strong's number, Lemma and Morphology with the heading's text), so it neither
/// satisfies nor excludes a term. With the KJV's "persecute" (G1377) of 12:14 marked as a
/// heading, `curse NOT strong:G1377` finds 12:14 in Search and the filter colours "curse".
#[test]
fn a_tagged_word_in_a_verses_heading_is_read_out_as_search_reads_it() {
    let mut kjv = kjv_romans_12();
    let unit = &mut kjv.units[0];
    let persecute = unit.text.find("persecute").unwrap();
    unit.spans.push(obs_model::Span {
        start: persecute as u32,
        end: (persecute + "persecute".len()) as u32,
        kind: SpanKind::Heading { level: 2 },
    });
    let rom = |v| Verse::new(obs_scripture::Book::Romans, 12, v);
    for (query, verses, coloured) in [
        ("curse NOT strong:G1377", vec![rom(14)], vec!["curse"]),
        ("curse strong:G1377", vec![], vec![]),
        ("strong:G1377", vec![rom(13)], vec!["to"]),
        (
            "strong:G2672 NOT strong:G1377",
            vec![rom(14)],
            vec!["curse"],
        ),
    ] {
        assert_eq!(search_verses(&kjv, query), verses, "Search: {query}");
        let (found, words) = filter_verses(&kjv, query);
        assert_eq!(found, verses, "filter: {query}");
        assert_eq!(words, coloured, "{query}");
    }
}
