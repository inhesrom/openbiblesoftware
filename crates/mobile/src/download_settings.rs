//! The downloads' own settings and the first-launch Study set offer (#333 "Downloads", S24):
//! "Use cellular data" (off by default) and whether the offer has been shown on this device,
//! kept in the device-only preference [`DOWNLOADS_PREF`] (never synced, ADR 0022 rule 4), and
//! every word of the offer. The shell asks [`MobileApp::study_set_offer`] at launch and shows
//! the sheet while it answers one; [`MobileApp::study_set_offer_shown`] is called as the sheet
//! appears, so the offer is never made again on this device, even if the app is closed while
//! it shows (as the desktop's First-run tour).

use serde::{Deserialize, Serialize};

use crate::library::Network;
use crate::{Changed, MobileApp, MobileError};

/// The User database preference holding [`DownloadSettings`] (device-only).
pub const DOWNLOADS_PREF: &str = "mobile.downloads";

/// What [`DOWNLOADS_PREF`] holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DownloadSettings {
    /// "Use cellular data": off by default (#333).
    #[serde(default)]
    pub(crate) use_cellular: bool,
    /// The first-launch Study set offer has been shown on this device.
    #[serde(default)]
    pub(crate) study_set_offered: bool,
}

impl DownloadSettings {
    /// The settings kept in the User database `user`, or the defaults (none kept, no User
    /// database, or a value that does not parse).
    pub(crate) fn read(user: Option<&dyn obs_engine::UserData>) -> Self {
        (user.and_then(|u| u.preference(DOWNLOADS_PREF).ok().flatten()))
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }
}

/// The first-launch Study set offer, every word of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudySetOffer {
    pub title: String,
    /// What the Study set brings.
    pub message: String,
    /// "15 Resources, 136.6 MB", once the released Starter library list is in.
    pub size: Option<String>,
    /// While the list is fetched, or why it could not be.
    pub status: Option<String>,
    /// The network now, and what Download does on it.
    pub network: String,
    /// Where to find it later.
    pub later: String,
    pub download: String,
    pub not_now: String,
    /// The label of the button that fetches the list again, after a failure.
    pub retry: Option<String>,
    /// The released Starter library list is being looked up (`status` says so).
    pub looking_up: bool,
}

/// "Use cellular data", as the setting is named everywhere.
pub const USE_CELLULAR_LABEL: &str = "Use cellular data";

impl MobileApp {
    /// "Use cellular data" (off by default): the `allow_cellular` of every [`Network`] the
    /// shell passes.
    pub fn use_cellular(&self) -> bool {
        self.download_settings.use_cellular
    }

    /// Set "Use cellular data", kept on this device.
    pub fn set_use_cellular(&mut self, on: bool) -> Result<(), MobileError> {
        self.download_settings.use_cellular = on;
        self.save_download_settings()?;
        self.events.push(Changed::Downloads);
        Ok(())
    }

    /// The first-launch Study set offer, while there is one to make: never once it has been
    /// shown on this device at an earlier launch, and never when nothing of the Study set is
    /// missing (every Resource of it in the library or removed, once the released list says
    /// which; or the debug Study set override). Before the released list is in, it has no size
    /// and says it is being looked up (the shell starts [`MobileApp::check_starter`] as the
    /// sheet appears).
    pub fn study_set_offer(&self, network: Network) -> Option<StudySetOffer> {
        if self.offer_shown_before || self.paths.study_set.is_some() {
            return None;
        }
        let starter = &self.session.starter;
        let view = self.starter_library();
        let known = starter.released().is_some();
        if known && view.study_set.resources == 0 {
            return None;
        }
        // Without the released list: looking it up, or why it could not be fetched. With
        // one (this run's, or the one kept on the device), its size; a later check that
        // fails changes nothing here.
        let error = (!known && !starter.is_checking())
            .then(|| starter.error())
            .flatten();
        let looking_up = !known && error.is_none();
        let status = match (known, error) {
            (true, _) => None,
            (false, Some(e)) => Some(format!(
                "The Starter library list could not be fetched: {e}"
            )),
            (false, None) => Some("Looking up the Study set…".to_string()),
        };
        // Not known yet counts as not Wi-Fi ([`Network::is_wifi`]); the shell asks again
        // when its path changes.
        let now = match network.is_wifi {
            Some(true) => "You are on Wi-Fi.",
            Some(false) => "You are not on Wi-Fi.",
            None => "Checking the network.",
        };
        let network = if network.is_wifi == Some(true) {
            now.to_string()
        } else if network.allow_cellular {
            format!("{now} {USE_CELLULAR_LABEL} is on, so it downloads now.")
        } else {
            format!("{now} Download asks before it uses cellular data.")
        };
        Some(StudySetOffer {
            title: "Get the Study set".into(),
            message: "Word Info, Word Study and the Passage Guide's sections work in full with \
                      the Study set: the tagged Bibles (KJV with Strong's, Hebrew and Greek), \
                      more Lexicons, the Dictionaries, a Commentary and the People & places \
                      data."
                .into(),
            size: known.then(|| view.study_set.label.clone()),
            status,
            network,
            later: "You can get it later, or one Resource at a time, in Library → Starter \
                    library."
                .into(),
            download: "Download".into(),
            not_now: "Not now".into(),
            retry: error.is_some().then(|| "Try again".to_string()),
            looking_up,
        })
    }

    /// The offer's sheet appeared: it is not made again at a later launch (this launch
    /// still answers [`Self::study_set_offer`], so the open sheet can follow the list).
    pub fn study_set_offer_shown(&mut self) -> Result<(), MobileError> {
        if self.download_settings.study_set_offered {
            return Ok(());
        }
        self.download_settings.study_set_offered = true;
        self.save_download_settings()
    }

    fn save_download_settings(&mut self) -> Result<(), MobileError> {
        let Some(user) = &self.session.user else {
            return Ok(());
        };
        let json = serde_json::to_string(&self.download_settings)
            .map_err(|e| MobileError::Failed(e.to_string()))?;
        (user.set_preference(DOWNLOADS_PREF, &json))
            .map_err(|e| MobileError::Failed(format!("Could not save the download settings: {e}")))
    }
}
