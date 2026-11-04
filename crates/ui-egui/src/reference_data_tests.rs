use super::*;
use obs_scripture::{Book, VerseMap, VerseMapEntry};

#[test]
fn scripture_ref_native_targets_translate_but_xrefs_are_already_standard() {
    let native = Verse::new(Book::John, 3, 16);
    let standard = Passage::verse(Verse::new(Book::John, 3, 18));
    let mut resource = obs_store::sample_resource();
    resource.verse_map = Some(VerseMap::new(
        vec![VerseMapEntry {
            verse: native,
            last: 16,
            standard: Some(standard),
        }],
        vec![],
        &Standard,
    ));
    let mut doc = ReadingDoc::from_resource(resource);
    let unit = doc
        .units
        .iter()
        .position(|u| u.id == obs_model::UnitId::book(Book::John))
        .unwrap();
    let list = vec![Passage::verse(native)].into();
    doc.units[unit].spans = vec![
        obs_model::Span {
            start: 0,
            end: 3,
            kind: SpanKind::ScriptureRef { targets: list },
        },
        obs_model::Span {
            start: 8,
            end: 8,
            kind: SpanKind::Xref {
                targets: vec![Passage::verse(native)].into(),
            },
        },
    ];
    assert_eq!(targets(&doc, unit, 0), [PassageList::from(vec![standard])]);
    assert_eq!(
        targets(&doc, unit, 1),
        [PassageList::from(vec![Passage::verse(native)])]
    );
    doc.verse_map = Some(Arc::new(VerseMap::new(
        vec![VerseMapEntry {
            verse: native,
            last: 16,
            standard: None,
        }],
        vec![],
        &Standard,
    )));
    assert!(targets(&doc, unit, 0)[0].is_empty());
}

#[test]
fn scripture_ref_identity_map_preserves_descending_target_order() {
    let mut doc = ReadingDoc::from_resource(obs_store::sample_resource());
    let unit = doc
        .units
        .iter()
        .position(|u| u.id == obs_model::UnitId::book(Book::John))
        .unwrap();
    let passages: Vec<_> = [17, 16, 17]
        .map(|n| Passage::verse(Verse::new(Book::John, 3, n)))
        .into();
    doc.verse_map = Some(Arc::new(VerseMap::new(
        [16, 17]
            .map(|n| VerseMapEntry {
                verse: Verse::new(Book::John, 3, n),
                last: n,
                standard: Some(Passage::verse(Verse::new(Book::John, 3, n))),
            })
            .into(),
        vec![],
        &Standard,
    )));
    doc.units[unit].spans = vec![obs_model::Span {
        start: 0,
        end: 3,
        kind: SpanKind::ScriptureRef {
            targets: passages.clone().into(),
        },
    }];
    assert_eq!(
        targets(&doc, unit, 0),
        passages
            .into_iter()
            .map(|p| vec![p].into())
            .collect::<Targets>()
    );
}

#[test]
fn query_submission_never_waits_for_io_and_stale_results_are_discarded() {
    let mut queries = Queries::<usize>::default();
    let library = Library::new();
    let passage = Passage::verse(Verse::new(Book::John, 3, 16));
    let (release, wait) = std::sync::mpsc::channel();
    let ui_thread = std::thread::current().id();
    assert!(
        queries
            .get(&library, passage, move |_, _| {
                assert_ne!(std::thread::current().id(), ui_thread);
                wait.recv().unwrap();
                Ok(1)
            })
            .is_none()
    );
    queries.clear();
    let _ = release.send(());
    let mut value = None;
    for _ in 0..100 {
        value = queries.get(&library, passage, |_, _| Ok(2));
        if value.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(*value.unwrap().unwrap(), 2);
}

#[test]
fn whole_bible_reference_translation_runs_on_worker_and_hover_reuses_cached_result() {
    let doc = crate::bench::large_reference_doc();
    assert!(doc.verse_map.as_ref().unwrap().entries().len() > 31_000);
    let key = TargetKey::new(&doc, 0, 0);
    let mut cache = TargetCache::default();
    let (release, wait) = std::sync::mpsc::channel();
    let ui_thread = std::thread::current().id();
    // Hold the worker so repeated hover polls deterministically cover the pending path.
    cache
        .queries
        .jobs
        .submit_fn("hold translation worker", move |_| {
            assert_ne!(std::thread::current().id(), ui_thread);
            wait.recv().unwrap();
            Ok(())
        });
    for _ in 0..100 {
        assert!(cache.get(key.clone()).is_none());
        assert_eq!(cache.queries.pending.len(), 1);
    }
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let result = loop {
        if let Some(result) = cache.get(key.clone()) {
            break result.unwrap();
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    let expected = Passage::new(
        Verse::new(Book::Genesis, 1, 1),
        Verse::new(Book::Revelation, 22, 21),
    )
    .unwrap();
    assert_eq!(*result, vec![PassageList::from(vec![expected])]);
    for _ in 0..100 {
        assert!(Arc::ptr_eq(
            &result,
            &cache.get(key.clone()).unwrap().unwrap()
        ));
        assert!(cache.queries.pending.is_empty());
    }
    // A replacement document with the same manifest must miss, not reuse old numbering.
    let replacement = crate::bench::large_reference_doc();
    assert!(cache.get(TargetKey::new(&replacement, 0, 0)).is_none());
}

#[test]
fn mapped_target_groups_merge_splits_and_contiguous_verses_but_keep_gaps_and_written_order() {
    let mut doc = ReadingDoc::from_resource(obs_store::sample_resource());
    let verse = |n| Verse::new(Book::John, 3, n);
    let passage = |a, b| Passage::new(verse(a), verse(b)).unwrap();
    doc.verse_map = Some(Arc::new(VerseMap::new(
        [
            (16, Some(passage(16, 17))),
            (17, Some(passage(16, 17))),
            (18, Some(passage(18, 19))),
            (19, None),
            (20, Some(passage(21, 21))),
        ]
        .map(|(n, standard)| VerseMapEntry {
            verse: verse(n),
            last: n,
            standard,
        })
        .into(),
        vec![],
        &Standard,
    )));
    doc.units[0].spans = vec![obs_model::Span {
        start: 0,
        end: 3,
        kind: SpanKind::ScriptureRef {
            targets: vec![passage(20, 20), passage(16, 20), passage(16, 17)].into(),
        },
    }];
    assert_eq!(
        targets(&doc, 0, 0),
        vec![
            vec![passage(21, 21)].into(),
            vec![passage(16, 19), passage(21, 21)].into(),
            vec![passage(16, 17)].into(),
        ]
    );
}
