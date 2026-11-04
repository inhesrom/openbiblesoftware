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
    assert_eq!(targets(&doc, unit, 0), [standard]);
    assert_eq!(targets(&doc, unit, 1), [Passage::verse(native)]);
    doc.verse_map = Some(Arc::new(VerseMap::new(
        vec![VerseMapEntry {
            verse: native,
            last: 16,
            standard: None,
        }],
        vec![],
        &Standard,
    )));
    assert!(targets(&doc, unit, 0).is_empty());
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
    assert_eq!(targets(&doc, unit, 0), passages);
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
