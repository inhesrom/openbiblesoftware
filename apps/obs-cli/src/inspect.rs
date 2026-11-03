//! `obs-cli inspect` and `obs-cli write-sample`.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use obs_model::{Resource, ResourceType, UnitId};
use obs_scripture::{ListedVersification, Passage, Verse, parse_reference};
use obs_store::{ObrReader, ResourceStore, StoreError, sample_resource, write_obr};

const INSPECT_USAGE: &str = "usage: obs-cli inspect <file.obr> [--unit ID] [--verse REF]";

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("obs-cli: {message}");
    ExitCode::FAILURE
}

pub fn write_sample(args: &[String]) -> ExitCode {
    let [path] = args else {
        eprintln!("usage: obs-cli write-sample <file.obr>");
        return ExitCode::from(2);
    };
    match write_obr(&sample_resource(), Path::new(path)) {
        Ok(()) => {
            println!("wrote {path}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

pub fn inspect(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("{INSPECT_USAGE}");
        return ExitCode::from(2);
    };
    let mut unit = None;
    let mut verse = None;
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let slot = match flag.as_str() {
            "--unit" => &mut unit,
            "--verse" => &mut verse,
            _ => {
                eprintln!("obs-cli inspect: unknown option `{flag}`\n{INSPECT_USAGE}");
                return ExitCode::from(2);
            }
        };
        let Some(value) = rest.next() else {
            eprintln!("obs-cli inspect: `{flag}` needs a value\n{INSPECT_USAGE}");
            return ExitCode::from(2);
        };
        *slot = Some(value.as_str());
    }
    let reader = match ObrReader::open(Path::new(path)) {
        Ok(r) => r,
        Err(e) => return fail(format!("{path}: {e}")),
    };
    let result = match (unit, verse) {
        (Some(id), _) => show_unit(&reader, id),
        (None, Some(reference)) => show_verse(&reader, reference),
        (None, None) => show_summary(&reader),
    };
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

fn show_summary(r: &ObrReader) -> Result<String, String> {
    let m = r.manifest();
    let resource: Resource = r.load().map_err(|e: StoreError| e.to_string())?;
    let mut out = String::new();
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
    let _ = writeln!(out, "resource_id     {}", m.resource_id);
    let _ = writeln!(out, "title           {}", m.title);
    let _ = writeln!(out, "abbreviation    {}", m.abbreviation);
    let _ = writeln!(out, "type            {:?}", m.resource_type);
    let _ = writeln!(out, "language        {} ({:?})", m.language, m.direction);
    let _ = writeln!(out, "key schemes     {:?}", m.key_schemes);
    let _ = writeln!(out, "revision        {}", m.revision);
    let _ = writeln!(out, "content hash    {}", m.content_hash);
    let _ = writeln!(out, "license         {}", m.license);
    let _ = writeln!(
        out,
        "attribution     {}",
        m.attribution.replace('\n', "\n                ")
    );
    let _ = writeln!(out, "publisher       {}", opt(&m.publisher));
    let _ = writeln!(out, "source url      {}", opt(&m.source_url));
    let _ = writeln!(out, "changes         {}", opt(&m.changes));
    let _ = writeln!(out, "origin          {:?}", m.origin);
    let _ = writeln!(
        out,
        "source          {} {}",
        m.source_format,
        opt(&m.source_file)
    );
    let _ = writeln!(out, "format version  {}", m.format_version);
    let s = r.summary().map_err(|e| e.to_string())?;
    let _ = writeln!(
        out,
        "\ncounts          {} units, {} spans, {} verses, {} keys, {} unit aliases",
        s.units, s.spans, s.verses, s.keys, s.unit_aliases
    );
    if s.cross_references + s.entities + s.mentions > 0 {
        let _ = writeln!(
            out,
            "datasets        {} cross-references, {} entities, {} mentions",
            s.cross_references, s.entities, s.mentions
        );
    }
    if let Some(map) = &resource.verse_map {
        let _ = writeln!(
            out,
            "verse map       {} unmapped, {} absent{}",
            map.unmapped().count(),
            map.absent().len(),
            if map.is_identity() { " (identity)" } else { "" }
        );
    }
    if !resource.dropped_markup.is_empty() {
        let dropped: Vec<String> = resource
            .dropped_markup
            .iter()
            .map(|(k, v)| format!("{k} ×{v}"))
            .collect();
        let _ = writeln!(out, "dropped markup  {}", dropped.join(", "));
    }
    let _ = writeln!(out, "\nunits:");
    for u in &resource.units {
        let _ = writeln!(
            out,
            "  {:<16} {:<10} {:>8} bytes {:>6} spans  {}",
            u.id.as_str(),
            format!("{:?}", u.kind),
            u.text.len(),
            u.spans.len(),
            u.title.as_deref().unwrap_or("")
        );
    }
    Ok(out)
}

fn excerpt(text: &str, start: usize, end: usize) -> String {
    let slice = text.get(start..end).unwrap_or("<bad range>");
    let mut s: String = slice.chars().take(60).collect();
    if slice.chars().count() > 60 {
        s.push('…');
    }
    s
}

fn show_unit(r: &ObrReader, id: &str) -> Result<String, String> {
    let uid = UnitId::new(id).map_err(|e| e.to_string())?;
    let unit = match r.unit(&uid).map_err(|e| e.to_string())? {
        Some(u) => u,
        None => match r.unit_alias(&uid).map_err(|e| e.to_string())? {
            Some(new) => return Err(format!("unit `{id}` was renamed to `{new}`")),
            None => return Err(format!("no unit `{id}`")),
        },
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} ({:?}) {}",
        unit.id,
        unit.kind,
        unit.title.unwrap_or_default()
    );
    let _ = writeln!(out, "\n{}\n", unit.text);
    let _ = writeln!(out, "spans:");
    for span in &unit.spans {
        let (_, attrs) = (
            span.kind.name(),
            serde_json::to_string(&span.kind).unwrap_or_default(),
        );
        let _ = writeln!(
            out,
            "  {:>6}..{:<6} {:<13} {:?}  {}",
            span.start,
            span.end,
            span.kind.name(),
            excerpt(&unit.text, span.start as usize, span.end as usize),
            attrs
        );
    }
    Ok(out)
}

fn show_verse(r: &ObrReader, reference: &str) -> Result<String, String> {
    if r.manifest().resource_type != ResourceType::Bible {
        return Err("--verse needs a Bible".into());
    }
    // A reference typed for a Bible is read in that Bible's own numbering (ADR 0002 rule 10).
    let verses = r.verses().map_err(|e| e.to_string())?;
    let own = ListedVersification::new(verses.iter().map(|v| (v.verse, v.last)));
    let map = r.verse_map().map_err(|e| e.to_string())?;
    let list = parse_reference(reference, &own, None).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for passage in list.iter() {
        for v in passage.verses(&own) {
            match r.verse(v).map_err(|e| e.to_string())? {
                None => {
                    let _ = writeln!(out, "{v}: not in this Bible");
                }
                Some(bv) => {
                    let unit = r
                        .unit(&bv.unit)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("missing unit {}", bv.unit))?;
                    let text = unit.text.get(bv.range()).unwrap_or("<bad range>");
                    // Like the verse-number tooltip: the Standard equivalent where it differs.
                    let standard = match map.as_ref().map(|m| m.to_standard(v)) {
                        None => String::new(),
                        Some(None) => " (Unmapped)".into(),
                        Some(Some(p)) if p == own_passage(v, bv.last) => String::new(),
                        Some(Some(p)) => format!(" (Standard {p})"),
                    };
                    let _ = writeln!(
                        out,
                        "{v}{standard} [{} {}..{}]: {text}",
                        bv.unit, bv.start, bv.end
                    );
                }
            }
        }
    }
    Ok(out)
}

fn own_passage(v: Verse, last: u16) -> Passage {
    Passage::new(v, Verse::new(v.book, v.chapter, last.max(v.verse))).unwrap_or(Passage::verse(v))
}
