//! The Search preview scenario (#295): the Command box typed into, a key at a time, over a
//! library whose ★ Bible is a whole Bible (verses of 15–35 words, its Search index built). Each
//! query is timed from the frame its last key is typed in to the end of the first frame that
//! draws the preview's hits for it: the debounce, the background search on a worker thread as
//! the app runs it (`JobMode::default_for_host`), and the frame that shows them, in wall-clock
//! time. The queries include one that matches nearly every Verse (`LORD`), the costliest to
//! count.
//!
//! The frames are timed as every feature scenario's: the whole Shell (the dropdown included)
//! against the missed-frame budget, and the Bible Tab under the dropdown as the synchronous
//! work, as every frame must draw a feature Tab.

use egui::Event;
use obs_engine::{SearchPreview, TabKind};
use serde_json::json;

use super::tools::{ActionWork, ShellBench};
use super::{BenchConfig, Samples};

/// What is typed, a query a step: a word in nearly every Verse, then rarer words and phrases.
pub(crate) const QUERIES: [&str; 4] = ["LORD", "every man", "said unto", "shall be"];

/// Frames between two keys typed (83 ms at 60 Hz: a quick typist, inside the debounce).
const KEY_FRAMES: usize = 5;

/// The two whole Bibles, the first the ★ Bible, with their Search indexes built; each query
/// typed into the Command box and timed until its hits show.
pub(crate) fn search_preview(cfg: &BenchConfig) -> Result<Samples, String> {
    let resources = super::text_size::whole_bibles()?;
    let ids = ["bench-a", "bench-b"];
    let mut b = ShellBench::new(cfg, "search-preview", &resources, |s, root| {
        s.set_search_cache_dir(root.join("cache"));
        s.refresh_search_indexes();
    })?;
    b.wait("the Search indexes", |b| {
        let s = b.shell.session();
        !s.search.is_building() && ids.iter().all(|id| s.search.is_ready(id))
    })?;
    let bible = b
        .tab_of(&TabKind::Resource("bench-a".into()))
        .ok_or("no Bible Tab")?;
    b.time_tab(bible);
    let mut s = Samples::default();
    for step in 0..cfg.tool_steps {
        let query = QUERIES[step % QUERIES.len()];
        b.shell.st.command_box.text.clear();
        b.shell.run("command_box.focus", &json!({}));
        for _ in 0..3 {
            b.frame(Some(&mut s), |_| {});
        }
        let chars: Vec<char> = query.chars().collect();
        let (last, typed) = chars.split_last().ok_or("an empty query")?;
        for c in typed {
            b.events.push(Event::Text(c.to_string()));
            for _ in 0..KEY_FRAMES {
                b.frame(Some(&mut s), |_| {});
            }
        }
        b.events.push(Event::Text(last.to_string()));
        b.settle_timed(
            &mut s,
            ActionWork::Budgeted,
            |_| {},
            |b| shows_hits(b, query),
        );
        if !shows_hits(&b, query) {
            return Err(format!(
                "the Search preview never showed hits for {query:?}"
            ));
        }
        b.events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        for _ in 0..3 {
            b.frame(Some(&mut s), |_| {});
        }
    }
    b.measured(s)
}

/// Whether the last frame drew the Search preview's hits for `query`.
fn shows_hits(b: &ShellBench, query: &str) -> bool {
    match b.shell.session().search_preview() {
        SearchPreview::Hits(h) => {
            h.query == query && h.hits.first().is_some_and(|hit| b.has_text(&hit.reference))
        }
        _ => false,
    }
}
