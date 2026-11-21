//! Search Christian Open Library (#343): a link out to COL's website, searching for a Verse, a
//! Passage or selected text, in the system browser. The app sends nothing: the search is after
//! the `#`, which the browser keeps to itself; COL's page reads it and its server never
//! receives it.

use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;

/// COL's website.
pub const COL_WEBSITE: &str = "https://christianopenlibrary.org";

/// The longest search COL's website takes, in characters.
pub const WEBSITE_QUERY_CHARS: usize = 500;

/// Settings → Search → Search Christian Open Library: `off` hides it.
pub(crate) const SEARCH_LINK_PREF: &str = "col/search-link";

/// COL's website searching for `query`: `https://christianopenlibrary.org/#q=…`, the query's
/// whitespace collapsed and cut to [`WEBSITE_QUERY_CHARS`] at a word, percent-encoded. `None`
/// for an empty query.
pub fn website_search_url(query: &str) -> Option<String> {
    let mut q = String::new();
    for w in query.split_whitespace() {
        let more = usize::from(!q.is_empty()) + w.chars().count();
        if q.chars().count() + more > WEBSITE_QUERY_CHARS {
            if q.is_empty() {
                q = w.chars().take(WEBSITE_QUERY_CHARS).collect();
            }
            break;
        }
        if !q.is_empty() {
            q.push(' ');
        }
        q.push_str(w);
    }
    if q.is_empty() {
        return None;
    }
    let mut encoded = String::with_capacity(q.len() * 3);
    for b in q.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    Some(format!("{COL_WEBSITE}/#q={encoded}"))
}

/// Whether Search Christian Open Library is offered, once read from the User database.
#[derive(Debug, Default, Clone, Copy)]
pub struct ColLink {
    shown: Option<bool>,
}

impl ColLink {
    /// Read the preference again when next asked (a restore replaced the User database).
    pub(crate) fn forget(&mut self) {
        self.shown = None;
    }
}

impl Session {
    /// Whether menus and the Passage Guide offer Search Christian Open Library; on unless the
    /// user turned it off.
    pub fn col_search_link(&mut self) -> bool {
        if let Some(on) = self.col_link.shown {
            return on;
        }
        let on = self
            .user
            .as_ref()
            .and_then(|u| u.preference(SEARCH_LINK_PREF).ok().flatten())
            .is_none_or(|v| v != "off");
        self.col_link.shown = Some(on);
        on
    }
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// `col.search_website {text}`: open COL's website searching for `text`.
fn search_website(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let url = website_search_url(p.str("text").unwrap_or_default())
        .ok_or_else(|| failed("col.search_website", "Nothing to search for"))?;
    s.workspace.push_effect(crate::Effect::OpenUrl(url.clone()));
    Ok(json!({ "url": url }))
}

/// `col.search_link {on}`: show or hide Search Christian Open Library.
fn search_link(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let on = p.bool("on").unwrap_or(!s.col_search_link());
    s.col_link.shown = Some(on);
    if let Some(user) = &s.user {
        user.set_preference(SEARCH_LINK_PREF, if on { "on" } else { "off" })
            .map_err(|e| failed("col.search_link", format!("saving: {e}")))?;
    }
    Ok(json!({ "on": on }))
}

const TEXT: ParamSpec = ParamSpec {
    name: "text",
    kind: ParamKind::String,
    required: true,
    help: "A Bible reference or words to search COL's website for",
};
const ON: ParamSpec = ParamSpec {
    name: "on",
    kind: ParamKind::Boolean,
    required: false,
    help: "Offer it (true) or not (false); without it, toggle",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "col.search_website",
        label: "Search Christian Open Library",
        shortcut: None,
        params: &[TEXT],
        enabled: always,
        run: search_website,
    },
    CommandSpec {
        id: "col.search_link",
        label: "Offer Search Christian Open Library",
        shortcut: None,
        params: &[ON],
        enabled: always,
        run: search_link,
    },
];

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::{Arc, Mutex};

    use obs_formats::sword_repo::{Fetch, FetchError, Progress};
    use serde_json::json;

    use super::*;
    use crate::{Effect, JobMode};

    #[test]
    fn the_website_search_url_keeps_the_query_after_the_hash_encoded_and_bounded() {
        assert_eq!(
            website_search_url("Romans 8:28").as_deref(),
            Some("https://christianopenlibrary.org/#q=Romans%208%3A28")
        );
        assert_eq!(
            website_search_url("  the   Spirit\n helps ").as_deref(),
            Some("https://christianopenlibrary.org/#q=the%20Spirit%20helps")
        );
        assert_eq!(
            website_search_url("ἀγάπη & “love”").as_deref(),
            Some(
                "https://christianopenlibrary.org/#q=%E1%BC%80%CE%B3%CE%AC%CF%80%CE%B7%20%26%20%E2%80%9Clove%E2%80%9D"
            )
        );
        assert_eq!(website_search_url("   "), None);
        // Cut at a word, never past 500 characters.
        let long = "covenant ".repeat(100);
        let url = website_search_url(&long).unwrap();
        let q = url.split_once("#q=").unwrap().1.replace("%20", " ");
        assert!(q.chars().count() <= WEBSITE_QUERY_CHARS);
        assert!(q.ends_with("covenant"), "{q}");
        let one = "x".repeat(600);
        let q = website_search_url(&one).unwrap();
        assert_eq!(q.split_once("#q=").unwrap().1.len(), WEBSITE_QUERY_CHARS);
    }

    /// A transport that records every URL asked for.
    #[derive(Debug, Default)]
    struct Recording(Mutex<Vec<String>>);

    impl Fetch for Recording {
        fn get(&self, url: &str, _: Progress<'_>) -> Result<Vec<u8>, FetchError> {
            self.0.lock().unwrap().push(url.to_string());
            Err(FetchError::NotFound)
        }
    }

    #[test]
    fn search_christian_open_library_opens_the_website_and_sends_nothing() {
        let dir = std::env::temp_dir().join(format!("obs-col-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::new(JobMode::Inline);
        let (user, _) = obs_store::UserStore::open(&dir.join("user.db")).unwrap();
        s.attach_user(Box::new(user));
        let fetch = Arc::new(Recording::default());
        s.set_fetch(fetch.clone());
        assert!(s.col_search_link(), "on by default");
        let out = s
            .run("col.search_website", &json!({ "text": "Romans 8:28" }))
            .unwrap();
        let url = "https://christianopenlibrary.org/#q=Romans%208%3A28";
        assert_eq!(out["url"], url);
        assert!(
            s.workspace
                .take_effects()
                .contains(&Effect::OpenUrl(url.into()))
        );
        s.jobs().pump(64);
        assert!(
            fetch.0.lock().unwrap().is_empty(),
            "the app fetched nothing"
        );
        assert!(
            s.run("col.search_website", &json!({ "text": "  " }))
                .is_err()
        );
        // The Setting hides it, and is kept.
        s.run("col.search_link", &json!({ "on": false })).unwrap();
        assert!(!s.col_search_link());
        let setting = crate::settings::SETTINGS
            .iter()
            .find(|st| st.id == "col_search_link")
            .unwrap();
        assert_eq!(setting.section, crate::settings::SettingsSection::Search);
        assert_eq!(s.setting_is_default("col_search_link"), Some(false));
        s.col_link = ColLink::default();
        assert!(!s.col_search_link(), "read back from the User database");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
