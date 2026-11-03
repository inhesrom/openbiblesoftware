//! A small hand-made Bible used by tests, examples and `obs-cli` demos. Public-domain KJV text.

use obs_model::{
    Direction, Key, KeyScheme, Manifest, MorphemePart, NoteKind, Origin, ParaStyle, Resource,
    ResourceBuilder, ResourceType, SpanKind, TextAccumulator, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, PassageList, Verse};

/// John 3:16-17 and Psalm 3:title-1, with every span shape the format must carry.
#[doc(hidden)]
pub fn sample_resource() -> Resource {
    let mut m = Manifest::new(
        "kjv-sample",
        "King James Version (sample)",
        "KJV",
        ResourceType::Bible,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.direction = Direction::Ltr;
    m.license = "LicenseRef-PD".into();
    m.attribution = "King James Version, public domain.".into();
    m.origin = Origin::Starter;
    m.source_format = "hand-made".into();
    m.import_tool_version = "test".into();
    let mut b = ResourceBuilder::new(m);

    let psa = UnitId::book(Book::Psalms);
    let mut t = TextAccumulator::new();
    let title = t.push("A Psalm of David, when he fled from Absalom his son.");
    t.push(" ");
    let v1 = t.push("LORD, how are they increased that trouble me!");
    let selah = t.push(" Selah.");
    let len = t.pos();
    b.add_unit(
        psa.clone(),
        UnitKind::BibleBook,
        Some("Psalms".into()),
        t.into_string(),
    );
    b.add_span(&psa, 0..len, SpanKind::Chapter { chapter: 3 });
    b.add_span(&psa, title.clone(), SpanKind::PsalmTitle);
    b.add_span(&psa, v1.start..v1.start + 4, SpanKind::DivineName);
    b.add_span(&psa, selah.start + 1..selah.end, SpanKind::Selah);
    b.add_span(
        &psa,
        v1.start..selah.end,
        SpanKind::Para {
            style: ParaStyle::Poetry1,
        },
    );
    b.add_verse(&psa, title, Verse::new(Book::Psalms, 3, 0), 0);
    b.add_verse(&psa, v1.start..selah.end, Verse::new(Book::Psalms, 3, 1), 1);

    let jhn = UnitId::book(Book::John);
    let mut t = TextAccumulator::new();
    let v16 = t.push(
        "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
    );
    t.push(" ");
    let v17 = t.push(
        "For God sent not his Son into the world to condemn the world; but that the world through him might be saved.",
    );
    let len = t.pos();
    let text = t.into_string();
    b.add_unit(jhn.clone(), UnitKind::BibleBook, Some("John".into()), text);
    b.add_unit(
        UnitId::note(1),
        UnitKind::Note,
        None,
        "Gr. only-begotten.".into(),
    );
    b.add_span(&jhn, 0..len, SpanKind::Chapter { chapter: 3 });
    b.add_span(
        &jhn,
        0..len,
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    );
    b.add_span(
        &jhn,
        4..7,
        SpanKind::TaggedWord {
            parts: vec![MorphemePart {
                strongs: "G2316".parse().unwrap_or_else(|_| unreachable!()),
                lemma: Some("θεός".into()),
                morph: Some("N-NSM".into()),
                gloss: Some("God".into()),
            }],
        },
    );
    b.add_span(
        &jhn,
        11..16,
        SpanKind::TaggedWord {
            parts: vec![MorphemePart::new(
                "G25".parse().unwrap_or_else(|_| unreachable!()),
            )],
        },
    );
    b.add_span(
        &jhn,
        63..63,
        SpanKind::NoteRef {
            note: UnitId::note(1),
            note_kind: NoteKind::Footnote,
        },
    );
    b.add_span(
        &jhn,
        v16.end..v16.end,
        SpanKind::Xref {
            targets: PassageList::from(vec![Passage::verse(Verse::new(Book::Romans, 5, 8))]),
        },
    );
    b.add_verse(&jhn, v16, Verse::new(Book::John, 3, 16), 16);
    b.add_verse(&jhn, v17, Verse::new(Book::John, 3, 17), 17);
    b.add_key(
        Key::Passage(Passage::verse(Verse::new(Book::John, 3, 16))),
        &jhn,
        Some(0..4),
        false,
    );
    b.add_unit_alias(UnitId::new("JOH").unwrap_or_else(|_| unreachable!()), jhn);
    b.count_dropped("\\fig");
    b.count_dropped("\\fig");
    b.count_dropped("\\rq");
    match b.build() {
        Ok(r) => r,
        Err(e) => unreachable!("sample resource is invalid: {e}"),
    }
}
