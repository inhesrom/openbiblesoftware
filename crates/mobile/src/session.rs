//! The Mobile Last session (CONTEXT.md: Last session): which Resource each reader shows and
//! where, kept per device in the User database through `UserData::save_last_session` as its
//! own JSON (ADR 0022 rule 4: never synced). It is not the desktop's Workspace snapshot,
//! whose Panels, Link sets and Floating windows the Mobile app does not have; a snapshot in
//! another format is ignored (the default reader opens) and replaced at the next save.
//!
//! ```json
//! {"mobile":1,"readers":[{"resource":"bsb","verse":{"book":"John","chapter":3,"verse":16}}]}
//! ```

use obs_model::Key;
use obs_scripture::Verse;
use serde::{Deserialize, Serialize};

use crate::events::Changed;
use crate::readers::{MAX_READERS, Reader};
use crate::{MobileApp, MobileError, ReaderId};

/// The format version of the Mobile Last session.
const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MobileSession {
    /// [`FORMAT`]; a desktop snapshot has no such field and is not read.
    mobile: u32,
    readers: Vec<ReaderPlace>,
}

/// Where one reader was: its Resource and Current location, a Verse in the Resource's own
/// numbering or an entry Key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ReaderPlace {
    resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verse: Option<Verse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<Key>,
}

impl MobileApp {
    /// The Mobile Last session of the readers now, as JSON.
    fn session_json(&self) -> String {
        let readers = (self.readers.iter())
            .filter_map(|r| {
                let t = self.session.workspace.tab(r.tab)?;
                let keyed = t.keeps_entries();
                Some(ReaderPlace {
                    resource: r.view.doc.manifest.resource_id.clone(),
                    verse: if keyed { None } else { t.history.current() },
                    key: if keyed {
                        t.entry_history.current()
                    } else {
                        None
                    },
                })
            })
            .collect();
        serde_json::to_string(&MobileSession {
            mobile: FORMAT,
            readers,
        })
        .unwrap_or_default()
    }

    /// Save the Mobile Last session if it changed since it was last saved, and record
    /// History's new Visits. The shell calls it when the app goes to the background
    /// ([`Self::on_background`]) and may call it any time.
    pub fn save_session(&mut self) -> Result<(), MobileError> {
        self.session.record_history();
        self.session.refresh_history_day();
        let json = self.session_json();
        let Some(user) = &self.session.user else {
            return Ok(());
        };
        if self.saved_session.as_deref() == Some(json.as_str()) {
            return Ok(());
        }
        (user.save_last_session(&json))
            .map_err(|e| MobileError::Failed(format!("Could not save the Last session: {e}")))?;
        self.saved_session = Some(json);
        Ok(())
    }

    /// The app goes to the background (it may not come back): save what would be lost.
    pub fn on_background(&mut self) -> Result<(), MobileError> {
        self.save_session()
    }

    /// The app comes back to the foreground: History's day headings may be a day old.
    pub fn on_resume(&mut self) {
        self.session.refresh_history_day();
        self.events.push(Changed::History);
    }

    /// Open the readers of the Mobile Last session at their places, or, with none (a first
    /// launch, a snapshot of another format, its Resources gone), the first Bible in
    /// Preferred order at its start. A restore is no Visit; the first launch's Bible is.
    /// (Boot calls it; boot is native only.)
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn restore_readers(&mut self) {
        let saved = (self.session.user.as_ref()).and_then(|u| u.last_session().ok().flatten());
        let places: Vec<ReaderPlace> = (saved.as_deref())
            .and_then(|json| serde_json::from_str::<MobileSession>(json).ok())
            .filter(|s| s.mobile == FORMAT)
            .map(|s| s.readers)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| self.session.library.get(&p.resource).is_some())
            .take(MAX_READERS)
            .collect();
        for place in &places {
            if let Err(e) = self.restore_reader(place) {
                self.problems.push(e.to_string());
            }
        }
        if !self.readers.is_empty() {
            let _ = self.session.workspace.take_visits();
            self.saved_session = saved;
            return;
        }
        let first_bible = (self.session.library)
            .preferred(obs_model::ResourceType::Bible)
            .map(|e| e.manifest.resource_id.clone());
        if let Some(id) = first_bible
            && let Err(e) = self.open(ReaderId(0), &id, None)
        {
            self.problems.push(e.to_string());
        }
    }

    /// Open the next reader at `place`.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn restore_reader(&mut self, place: &ReaderPlace) -> Result<(), MobileError> {
        self.doc(&place.resource)?;
        let tab = self.open_tab(&place.resource)?;
        let ws = &mut self.session.workspace;
        if let Some(v) = place.verse {
            ws.scrolled(tab, v);
        }
        if let (Some(key), Some(t)) = (&place.key, ws.tab_mut(tab)) {
            t.entry_history.scrolled_to(key.clone());
        }
        match self.load_view(tab) {
            Ok(view) => self.readers.push(Reader::new(tab, view)),
            Err(e) => {
                let _ = (self.session).run("tab.close", &serde_json::json!({ "tab": tab.0 }));
                return Err(e);
            }
        }
        self.apply_effects();
        Ok(())
    }
}
