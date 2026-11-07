//! Every toolbar action over a corpus of tricky Notes (#186 review): either the safety net
//! lets the change through and it shows as intended, or the Note stays byte for byte.

use std::ops::Range;

use obs_model::{NoteLineKind, NoteMarkKind, NoteShown, NoteShownMark};

use super::*;

/// Every repro from the review rounds of #186, then samples written for this test after
/// the sections of the CommonMark spec (tabs, rules, headings, code, HTML, link reference
/// definitions, paragraphs, quotes, list items, lists, then the inlines).
const CORPUS: &[&str] = &[
    // Review repros.
    "-     **faith**",
    "    **faith**",
    "a\n\n    **x**",
    "- a\n\n      **x**",
    "1. a\n\n       **x**",
    "- a\n  ```\n  **x**\n  ```",
    "```\n**x**\n```",
    "> a\n>\n>     **x**",
    "` a\nb `",
    "`a\nb`",
    "`` `a\nb` ``",
    "` \n hope `",
    "> `a\n> b`",
    "a\n`b\nc`",
    "`a b` [x](u)\nc",
    "[x\ny](u)",
    "[x\ny](<Rom 5:8> \"t\")",
    "[x\ny]\n\n[x y]: <https://example.org/?q=\\&amp;>",
    "![a\nb](u)",
    "*[x\ny](u)*",
    "**a `b\nc` d**",
    "**a *b\nc* d**",
    "**faith\nhope**",
    "*a\nb*",
    "__a\nb__",
    "> **a *b***\n> ***c* d**",
    "*faith\nhope*\n---",
    "*faith\nhope*\n===",
    "by *grace*\n  alone\n---",
    "by grace\nalone\n===",
    "Grace\n===\nafter",
    "Grace\n\n---",
    "intro\n### Grace\n---",
    "## Grace\n- faith\n  1. hope\n\n> love",
    "intro\n\n## by grace\n## alone",
    "- a\n\n  more\n\n  - b\n\n    deeper\n\nafter",
    "- a\n\n- b\n\n  c",
    "- a\n  - b\n    - c\n  - d\n- e",
    "1. x\n   - y\n   - z",
    "1. x\n   - y\nz",
    "  9) nine\n  10) ten",
    "> - faith\n> - hope",
    "caf&eacute; &#233;t&eacute;",
    "see `a b` now",
    "[faith](<Rom 5:8>) now",
    "<https://a.org> x",
    // Tabs.
    "\tfoo\tbaz",
    "-\tfoo\n\n\tbar",
    "- a\n\t- b",
    // Thematic breaks.
    "***\n---\n___",
    "a\n***\nb",
    "- - -",
    // ATX headings.
    "# foo *bar* \\*baz\\*",
    "#5 bolt\n\n#hashtag",
    "\\## foo",
    "### foo ###",
    "## \n#",
    "foo\n# bar\nbaz",
    // Setext headings.
    "Foo *bar*\n=========",
    "Foo\n    ---",
    "- Foo\n---",
    "> foo\n-----",
    "\\> foo\n------",
    // Indented code.
    "    a simple\n      indented code block",
    "  - foo\n\n    bar",
    "1.  foo\n\n    - bar",
    "    chunk1\n\n    chunk2",
    "Foo\n    bar",
    "    foo\nbar",
    // Fenced code.
    "```\n<\n >\n```",
    "~~~\naaa\n~~~",
    "```\naaa",
    "> ```\n> aaa\n\nbbb",
    "```ruby\ndef foo(x)\n```",
    "``` ```\naaa",
    // HTML blocks.
    "<div>\n*hello*\n         <foo><a>",
    "<!-- foo -->*bar*\n*baz*",
    "<del>*foo*</del>",
    "<div>\n\n*Markdown*\n\n</div>",
    // Link reference definitions.
    "[foo]: /url \"title\"\n\n[foo]",
    "[foo]:\n/url\n\n[foo]",
    "[foo]: a\n[foo]: b\n\n[foo]",
    "[Foo bar]:\n<my url>\n'title'\n\n[Foo bar]",
    // Paragraphs and blank lines.
    "aaa\n\nbbb",
    "  aaa\n bbb",
    "aaa     \nbbb     ",
    "\n\naaa\n\n",
    // Block quotes.
    "> # Foo\n> bar\n> baz",
    "> bar\nbaz\n> foo",
    "> - foo\n- bar",
    ">     foo\n    bar",
    "> foo\n\n> bar",
    ">",
    "> a\n>\n> b",
    "> > a",
    // List items.
    "1.  A paragraph\n    with two lines.\n\n        indented code\n\n    > A block quote.",
    "- one\n\n two",
    "- one\n\n  two",
    "-one\n\n2.two",
    "1. a\n\n  2. b\n\n    3. c",
    "- a\n - b\n  - c\n   - d\n    - e",
    "10) foo\n    - bar",
    "- # Foo\n- Bar\n  ---\n  baz",
    "* a\n*\n\n* c",
    "1. foo\n2.\n3. bar",
    // Lists.
    "- foo\n- bar\n+ baz",
    "The number of windows in my house is\n14.  The number of doors is 6.",
    "- a\n- b\n\n- c",
    "- a\n  - b\n\n    c\n- d",
    "- a\n  > b\n  ```\n  c\n  ```\n- d",
    // Inlines.
    "\\*not emphasized*",
    "``foo ` bar``",
    "a * foo bar*",
    "_foo_bar",
    "*foo**bar**baz*",
    "***strong emph***",
    "*(**foo**)*",
    "**foo*",
    "foo***bar***baz",
    "[link](/uri \"title\")",
    "[link](</my uri>)",
    "[link [foo [bar]]](/uri)",
    "*[foo*](/uri)",
    "![foo](/url \"title\")",
    "<a><bab><c2c>",
    "foo  \nbaz",
    "foo\\\nbaz",
    "&nbsp; &amp; &copy;",
    "**a\n\nb**",
    "a *b\nc* d",
    "a_b_c",
    "*a `*` b*",
    "<span>*a*</span>",
    "café **grâce**",
    "→ *x* 😀",
];

/// The selections tried on `text` (char indices): the cursor at every place, a few
/// characters from every other place, each line whole, and the whole Note.
fn selections(text: &str) -> Vec<Range<usize>> {
    let n = text.chars().count();
    let mut out: Vec<Range<usize>> = (0..=n).map(|i| i..i).collect();
    out.extend((0..n).step_by(2).map(|i| i..(i + 3).min(n)));
    let mut start = 0;
    for line in text.split('\n') {
        let len = line.chars().count();
        out.push(start..start + len);
        out.push(start..n);
        start += len + 1;
    }
    out.push(0..n);
    out.sort_by_key(|r| (r.start, r.end));
    out.dedup();
    out
}

/// Each non-white-space character `range` (bytes) of `text` shows, and whether it is
/// marked `kind`.
fn marked(text: &str, range: Range<usize>, kind: &NoteMarkKind) -> Vec<bool> {
    let mut out = Vec::new();
    for line in obs_model::parse_note_text(text) {
        let on = range.start.max(line.source.start)..range.end.min(line.source.end);
        if on.start >= on.end {
            continue;
        }
        let shown = line.text_range(on);
        for (k, c) in line.text[shown.clone()].char_indices() {
            if c.is_whitespace() {
                continue;
            }
            let at = shown.start + k;
            out.push(
                line.marks
                    .iter()
                    .any(|m| m.kind == *kind && m.range.contains(&at)),
            );
        }
    }
    out
}

fn is_kind(f: Format, k: NoteLineKind) -> bool {
    match f {
        Format::Bullet => matches!(k, NoteLineKind::Bullet { .. }),
        Format::Numbered => matches!(k, NoteLineKind::Numbered { .. }),
        Format::Quote => k == NoteLineKind::Quote,
        _ => false,
    }
}

/// The kinds of the lines `range` (bytes) of `text` touches, gaps left out.
fn kinds(text: &str, range: Range<usize>) -> Vec<NoteLineKind> {
    obs_model::parse_note_text(text)
        .into_iter()
        .filter(|l| {
            l.kind != NoteLineKind::Blank
                && l.source.start <= range.end.max(range.start)
                && range.start <= l.source.end
        })
        .map(|l| l.kind)
        .collect()
}

/// Panics, saying how, unless `f` on `sel` of `text` either left it byte for byte or
/// changed it as intended.
fn check(text: &str, sel: Range<usize>, f: Format) {
    let why = format_blocked(text, sel.clone(), f);
    let (new, r) = apply_format(text, sel.clone(), f);
    let at = format!("{text:?} {sel:?} {f:?} -> {new:?}");
    if why.is_some() || new == text {
        assert_eq!((new.as_str(), r), (text, sel), "off, yet changed: {at}");
        return;
    }
    let (a, b) = (byte_at(text, sel.start), byte_at(text, sel.end));
    let (na, nb) = (byte_at(&new, r.start), byte_at(&new, r.end));
    match f {
        Format::Bold | Format::Italic if a == b => {
            let mark = if f == Format::Bold { "**" } else { "*" };
            assert_eq!(
                new,
                format!("{}{mark}{mark}{}", &text[..a], &text[a..]),
                "{at}"
            );
        }
        Format::Bold | Format::Italic => {
            let kind = if f == Format::Bold {
                NoteMarkKind::Bold
            } else {
                NoteMarkKind::Italic
            };
            let before = marked(text, a..b, &kind);
            let after = marked(&new, na..nb, &kind);
            let all = |v: &[bool]| !v.is_empty() && v.iter().all(|m| *m);
            let none = |v: &[bool]| v.iter().all(|m| !*m);
            // Only white space selected: nothing shows to be marked.
            assert!(
                (before.is_empty() && after.is_empty())
                    || all(&after)
                    || (all(&before) && none(&after)),
                "not {kind:?} as asked: {at} {after:?}"
            );
        }
        Format::Link if a == b => {
            assert_eq!(new, format!("{}[](<>){}", &text[..a], &text[a..]), "{at}");
        }
        Format::Link => {
            let want = format!("{}[{}](<>){}", &text[..a], &text[a..b], &text[b..]);
            assert_eq!(new, want, "{at}");
            // Its text, where it shows any, is the link's.
            if !obs_model::note_plain_text(&text[a..b]).trim().is_empty() {
                let linked = obs_model::note_shown(&new).iter().any(|s| {
                    matches!(s, NoteShown::Text { marks, .. }
                        if marks.contains(&NoteShownMark::Link(String::new())))
                });
                assert!(linked, "no link: {at}");
            }
        }
        Format::Heading => {
            // A setext heading's underline is a gap: the heading is its first line.
            let line = |t: &str, at: usize| {
                let lines = obs_model::parse_note_text(t);
                let l = lines
                    .iter()
                    .find(|l| l.source.start <= at && at <= l.source.end)
                    .unwrap();
                let from = l.heading.as_ref().map_or(at, |h| h.source.start);
                lines
                    .iter()
                    .find(|l| l.source.start <= from && from <= l.source.end)
                    .unwrap()
                    .kind
            };
            let want = match line(text, a) {
                NoteLineKind::Heading(n) if n < 3 => NoteLineKind::Heading(n + 1),
                NoteLineKind::Heading(_) => NoteLineKind::Text,
                _ => NoteLineKind::Heading(1),
            };
            let got = line(&new, na);
            assert!(
                got == want || (want == NoteLineKind::Text && got == NoteLineKind::Blank),
                "{at}: {got:?}, not {want:?}"
            );
        }
        _ => {
            let lines = obs_model::parse_note_text(text);
            let (first, last, _) = rewritten_lines(text, &lines, sel.clone(), f);
            let off = lines[first..=last].iter().all(|l| is_kind(f, l.kind));
            let now = kinds(&new, na..nb);
            assert!(
                if off {
                    now.iter().all(|k| !is_kind(f, *k))
                } else {
                    now.iter().all(|k| is_kind(f, *k))
                },
                "{at}: {now:?}"
            );
        }
    }
    // What it shows, read again: the same, but for the mark asked for.
    if a != b || !matches!(f, Format::Bold | Format::Italic | Format::Link) {
        assert!(shows_as_intended(text, &new, f), "shows otherwise: {at}");
    }
}

#[test]
fn every_toolbar_action_on_every_tricky_note_is_as_intended_or_leaves_it_as_it_is() {
    let mut made = 0;
    let mut off = 0;
    for text in CORPUS {
        for sel in selections(text) {
            for f in Format::TOOLBAR {
                let (new, _) = apply_format(text, sel.clone(), f);
                if new == *text {
                    off += 1;
                } else {
                    made += 1;
                }
                check(text, sel.clone(), f);
            }
        }
    }
    eprintln!(
        "{} Notes: {made} changes made, {off} left as they were",
        CORPUS.len()
    );
    // The net is not simply off everywhere.
    assert!(made > off / 4, "made {made}, off {off}");
}
