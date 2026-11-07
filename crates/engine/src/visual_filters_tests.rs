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
    AlignedWords, FilterMatcher, MarkedText, VerseSlice, VisualFilter, block_hits, built_in_filters,
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
fn slices(r: &Resource) -> Vec<VerseSlice> {
    r.verses
        .iter()
        .map(|v| VerseSlice {
            range: v.start as usize..v.end as usize,
            standard: Passage::new(
                v.verse,
                Verse::new(v.verse.book, v.verse.chapter, v.last.max(v.verse.verse)),
            )
            .ok(),
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
    assert_eq!(matcher.context_verses(), 1);
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
