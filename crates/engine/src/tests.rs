use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Value, json};

use crate::command::MAX_STRING_PARAM;
use crate::*;

fn session() -> Session {
    Session::new(JobMode::Inline)
}

#[test]
fn command_ids_are_unique_and_well_formed() {
    let s = session();
    let mut seen = HashSet::new();
    for c in s.commands().iter() {
        assert!(seen.insert(c.id), "duplicate {}", c.id);
        assert!(
            c.id.contains('.')
                && c.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_'),
            "bad id {}",
            c.id
        );
        assert!(!c.label.is_empty());
    }
    assert!(s.commands().len() >= 5);
}

#[test]
fn sample_command_runs_through_the_session_and_the_registry() {
    let mut s = session();
    let out = s
        .run(
            "reference.parse",
            &json!({ "text": "Jn 3:16, 18", "long": true }),
        )
        .unwrap();
    assert_eq!(out["text"], "John 3:16; John 3:18");
    assert_eq!(out["passages"][0]["start"], "John.3.16");

    let spec = *s.commands().get("text.transliterate").unwrap();
    let params = spec.params_from_pairs(&[("text", "ἀγάπη")]).unwrap();
    assert_eq!(
        (spec.run)(&mut s, spec.validate(&params).unwrap()).unwrap(),
        json!({ "transliteration": "agape" })
    );

    let out = s
        .run(
            "reference.parse",
            &json!({ "text": "5", "context": "Rom 8:28" }),
        )
        .unwrap();
    assert_eq!(out["text"], "Rom 8:5");
}

#[test]
fn errors_are_typed() {
    let mut s = session();
    assert_eq!(
        s.run("nope.nothing", &Value::Null),
        Err(CommandError::UnknownCommand("nope.nothing".into()))
    );
    assert!(matches!(
        s.run("reference.parse", &json!({})),
        Err(CommandError::InvalidParams { message, .. }) if message == "missing parameter `text`"
    ));
    assert!(matches!(
        s.run("reference.parse", &json!({ "text": 3 })),
        Err(CommandError::InvalidParams { message, .. })
            if message == "parameter `text` must be string, not a number"
    ));
    assert!(matches!(
        s.run("app.version", &json!({ "x": 1 })),
        Err(CommandError::InvalidParams { message, .. }) if message == "unknown parameter `x`"
    ));
    assert!(matches!(
        s.run("reference.parse", &json!({ "text": "Jn 3:99" })),
        Err(CommandError::Failed { message, .. }) if message.contains("does not exist")
    ));
}

#[test]
fn params_from_pairs_converts_by_kind() {
    let s = session();
    let spec = s.commands().get("reference.parse").unwrap();
    assert_eq!(
        spec.params_from_pairs(&[("text", "42"), ("long", "yes")])
            .unwrap(),
        json!({ "text": "42", "long": true })
    );
    assert!(spec.params_from_pairs(&[("long", "maybe")]).is_err());
    assert!(spec.params_from_pairs(&[("bogus", "1")]).is_err());
}

/// Junk values for any parameter slot.
fn junk_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(0),
        json!(-1),
        json!(i64::MAX),
        json!(1.5),
        json!(""),
        json!(" "),
        json!("\u{0}"),
        json!("Jn 3:16"),
        json!("Ps 51:0-2; Mal 4:6-Matt 1:1"),
        json!("בְּרֵאשִׁ֖ית ἀγάπη"),
        json!("-:;,—"),
        json!("Gen 65535:65535"),
        json!("\u{202E}\u{FEFF}\u{10FFFF}"),
        json!("a".repeat(MAX_STRING_PARAM)),
        json!("a".repeat(MAX_STRING_PARAM + 1)),
        json!([]),
        json!([1, "x"]),
        json!({}),
        json!({ "nested": { "deep": [null] } }),
    ]
}

/// Every registered command, run with hostile parameters, returns `Ok` or `Err` and never
/// panics (#12 rule 8). New commands are covered automatically.
#[test]
fn hostile_params_never_panic() {
    let mut s = session();
    let ids: Vec<&'static str> = s.commands().iter().map(|c| c.id).collect();
    let mut runs = 0;
    for id in ids {
        let spec = *s.commands().get(id).unwrap();
        let mut cases: Vec<Value> = junk_values();
        cases.push(json!({ "unknown_param": 1 }));
        for p in spec.params {
            for v in junk_values() {
                cases.push(json!({ p.name: v }));
            }
            // Every parameter set at once, to reach past validation.
            for v in junk_values() {
                let all: serde_json::Map<String, Value> = spec
                    .params
                    .iter()
                    .map(|q| (q.name.to_string(), v.clone()))
                    .collect();
                cases.push(Value::Object(all));
            }
        }
        for params in cases {
            let result = catch_unwind(AssertUnwindSafe(|| s.run(id, &params)));
            let shown: String = params.to_string().chars().take(80).collect();
            assert!(result.is_ok(), "`{id}` panicked with params {shown}");
            runs += 1;
        }
    }
    assert!(runs > 100, "only {runs} runs");
}

#[test]
fn session_job_queue_is_usable() {
    let s = Session::new(JobMode::Threads(1));
    let id = s.jobs().submit_fn("double", |_| Ok(21 * 2));
    loop {
        let e = s
            .jobs()
            .wait_event(std::time::Duration::from_secs(10))
            .expect("event");
        if e.id == id && e.kind.is_terminal() {
            assert!(
                matches!(e.kind, JobEventKind::Finished(out) if out.downcast_ref::<i32>() == Some(&42))
            );
            break;
        }
    }
}
