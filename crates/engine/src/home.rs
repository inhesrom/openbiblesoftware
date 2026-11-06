//! Home (#178): the Tab that gathers today's reading, where the user left off, History and
//! Favorites, a Verse of the day, Quick start and a summary of the library, opened from the
//! toolbar (`home.open`, Alt+H) and shown at launch on the first run.
//!
//! Its cards are reordered and hidden like a Guide's sections; the Home arrangement and the
//! "Open Home at launch" setting are one preference in the User database ([`HOME_PREF`]).
//! Everything it shows comes from the library and the User database, so it works offline.

use obs_model::ResourceType;
use obs_scripture::{Passage, RefStyle, Standard, format_passage, parse_reference};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::favorites::{Favorite, Visit};
use crate::get_bibles::ResourceFilter;
use crate::reading_plans::PlanDate;
use crate::session::Session;
use crate::workspace::{Effect, OpenRequest, PanelId, TabId, TabKind};

/// Preference key of the Home arrangement and "Open Home at launch".
pub const HOME_PREF: &str = "home";

/// One card of Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HomeCard {
    /// The Readings due today in each Reading plan, each one click from its Bible.
    TodaysReading,
    /// The newest Visit in History.
    ContinueReading,
    /// The latest Visits and the Favorites.
    HistoryAndFavorites,
    /// One Passage a day from [`VERSES_OF_THE_DAY`], in the ★ Bible.
    VerseOfTheDay,
    /// Passage Guide, Word Study, Get Resources and Import.
    QuickStart,
    /// How many Resources of each Resource type the library has, and the Starter library.
    LibrarySummary,
}

impl HomeCard {
    /// In their default order.
    pub const ALL: [HomeCard; 6] = [
        HomeCard::TodaysReading,
        HomeCard::ContinueReading,
        HomeCard::HistoryAndFavorites,
        HomeCard::VerseOfTheDay,
        HomeCard::QuickStart,
        HomeCard::LibrarySummary,
    ];

    /// Its id in commands and the stored arrangement.
    pub fn id(self) -> &'static str {
        match self {
            HomeCard::TodaysReading => "todays_reading",
            HomeCard::ContinueReading => "continue",
            HomeCard::HistoryAndFavorites => "history_favorites",
            HomeCard::VerseOfTheDay => "verse_of_the_day",
            HomeCard::QuickStart => "quick_start",
            HomeCard::LibrarySummary => "library",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Its heading on Home.
    pub fn title(self) -> &'static str {
        match self {
            HomeCard::TodaysReading => "Today's reading",
            HomeCard::ContinueReading => "Continue where you left off",
            HomeCard::HistoryAndFavorites => "History and Favorites",
            HomeCard::VerseOfTheDay => "Verse of the day",
            HomeCard::QuickStart => "Quick start",
            HomeCard::LibrarySummary => "Your library",
        }
    }
}

/// One card's place in the Home arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardPlace {
    pub card: HomeCard,
    pub hidden: bool,
}

/// The Home arrangement (the order of the cards and which are hidden) and whether Home opens
/// at launch on top of the Last session. Always holds each card exactly once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSettings {
    pub cards: Vec<CardPlace>,
    pub open_at_launch: bool,
}

/// The stored form. Card ids stay strings so a save from a later version with more cards
/// still loads (unknown ids are dropped).
#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    cards: Vec<StoredCard>,
    #[serde(default)]
    open_at_launch: bool,
}

#[derive(Serialize, Deserialize)]
struct StoredCard {
    id: String,
    #[serde(default)]
    hidden: bool,
}

impl Default for HomeSettings {
    fn default() -> Self {
        Self {
            cards: HomeCard::ALL
                .into_iter()
                .map(|card| CardPlace {
                    card,
                    hidden: false,
                })
                .collect(),
            open_at_launch: false,
        }
    }
}

impl HomeSettings {
    /// Read saved settings. Unknown or repeated cards are dropped, and cards the save does
    /// not mention go to their default place.
    pub fn from_json(json: &str) -> Self {
        let Ok(stored) = serde_json::from_str::<Stored>(json) else {
            return Self::default();
        };
        let mut cards: Vec<CardPlace> = Vec::new();
        for c in stored.cards {
            if let Some(card) = HomeCard::from_id(&c.id)
                && !cards.iter().any(|p| p.card == card)
            {
                cards.push(CardPlace {
                    card,
                    hidden: c.hidden,
                });
            }
        }
        for (i, card) in HomeCard::ALL.into_iter().enumerate() {
            if cards.iter().any(|p| p.card == card) {
                continue;
            }
            let at = HomeCard::ALL[..i]
                .iter()
                .rev()
                .find_map(|before| cards.iter().position(|p| p.card == *before))
                .map_or(0, |p| p + 1);
            cards.insert(
                at,
                CardPlace {
                    card,
                    hidden: false,
                },
            );
        }
        Self {
            cards,
            open_at_launch: stored.open_at_launch,
        }
    }

    pub fn to_json(&self) -> String {
        let stored = Stored {
            cards: self
                .cards
                .iter()
                .map(|p| StoredCard {
                    id: p.card.id().into(),
                    hidden: p.hidden,
                })
                .collect(),
            open_at_launch: self.open_at_launch,
        };
        serde_json::to_string(&stored).unwrap_or_default()
    }

    /// The cards shown, in order.
    pub fn visible(&self) -> impl Iterator<Item = HomeCard> + '_ {
        self.cards.iter().filter(|p| !p.hidden).map(|p| p.card)
    }

    /// The hidden cards, in order.
    pub fn hidden(&self) -> impl Iterator<Item = HomeCard> + '_ {
        self.cards.iter().filter(|p| p.hidden).map(|p| p.card)
    }

    /// Move a card to index `to` of the list without it (clamped to the end). Returns whether
    /// anything changed.
    pub fn move_to(&mut self, card: HomeCard, to: usize) -> bool {
        let Some(from) = self.cards.iter().position(|p| p.card == card) else {
            return false;
        };
        let place = self.cards.remove(from);
        let to = to.min(self.cards.len());
        self.cards.insert(to, place);
        from != to
    }

    /// Hide or show a card. Returns whether anything changed.
    pub fn set_hidden(&mut self, card: HomeCard, hidden: bool) -> bool {
        match self.cards.iter_mut().find(|p| p.card == card) {
            Some(p) if p.hidden != hidden => {
                p.hidden = hidden;
                true
            }
            _ => false,
        }
    }

    /// Where a card is in the order (hidden ones count).
    pub fn position(&self, card: HomeCard) -> Option<usize> {
        self.cards.iter().position(|p| p.card == card)
    }
}

// ------------------------------------------------------------------------- Verse of the day

/// The Verse of the day list: well-known Passages of one to four Verses, in the Standard
/// versification, picked by us (a list of references, no Bible text). One is shown a day, in
/// turn ([`verse_of_the_day_on`]).
pub const VERSES_OF_THE_DAY: &[&str] = &[
    "Gen 1:1",
    "Num 6:24-26",
    "Deut 6:4-5",
    "Josh 1:9",
    "Ps 23:1",
    "Ps 46:1",
    "Ps 46:10",
    "Ps 119:105",
    "Ps 121:1-2",
    "Ps 139:14",
    "Prov 3:5-6",
    "Isa 26:3",
    "Isa 40:31",
    "Isa 41:10",
    "Isa 53:5",
    "Jer 29:11",
    "Lam 3:22-23",
    "Mic 6:8",
    "Zeph 3:17",
    "Matt 5:14",
    "Matt 6:33",
    "Matt 11:28",
    "Matt 28:19-20",
    "Mark 10:45",
    "Luke 1:37",
    "John 1:1",
    "John 3:16",
    "John 8:12",
    "John 14:6",
    "John 14:27",
    "Acts 1:8",
    "Rom 5:8",
    "Rom 8:28",
    "Rom 8:38-39",
    "Rom 12:2",
    "1 Cor 13:4-7",
    "2 Cor 5:17",
    "Gal 2:20",
    "Gal 5:22-23",
    "Eph 2:8-9",
    "Phil 4:6-7",
    "Phil 4:13",
    "Col 3:23",
    "2 Tim 3:16-17",
    "Heb 11:1",
    "Heb 12:1-2",
    "Jas 1:5",
    "1 Pet 5:7",
    "1 John 1:9",
    "1 John 4:19",
    "Rev 21:4",
];

/// The Verse of the day on `day`: the list in turn, one a day, the same all day.
pub fn verse_of_the_day_on(day: PlanDate) -> Passage {
    let len = i64::try_from(VERSES_OF_THE_DAY.len()).unwrap_or(1);
    let i = usize::try_from(day.0.rem_euclid(len)).unwrap_or(0);
    parse_reference(VERSES_OF_THE_DAY[i], &Standard, None)
        .ok()
        .and_then(|l| l.passages().first().copied())
        .unwrap_or_else(|| {
            Passage::verse(obs_scripture::Verse::new(obs_scripture::Book::John, 3, 16))
        })
}

/// Today's Verse of the day, and the Bible it is read in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerseOfTheDay {
    pub passage: Passage,
    /// "John 3:16".
    pub label: String,
    /// The ★ Bible, if the library has a Bible.
    pub bible: Option<String>,
}

// ------------------------------------------------------------------------------ the cards

/// What the Today's reading card shows of one unfinished Reading plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomePlan {
    pub id: i64,
    pub name: String,
    /// The Readings due today; with none due and the plan behind, the first missed one.
    pub readings: Vec<HomeReading>,
    pub behind: usize,
    /// The next day with a Reading, when none is due today.
    pub next: Option<PlanDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeReading {
    pub position: u32,
    /// "Gen 1-3".
    pub label: String,
    pub read: bool,
}

/// One Quick start: a label, the command it runs, and what it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickStart {
    pub label: &'static str,
    pub command: &'static str,
    pub help: &'static str,
}

/// The Quick start card, in order.
pub const QUICK_STARTS: [QuickStart; 4] = [
    QuickStart {
        label: "Passage Guide",
        command: "guide.passage_guide",
        help: "Commentaries, Cross-references, Key words and People & places for a Passage",
    },
    QuickStart {
        label: "Word Study",
        command: "guide.word_study",
        help: "A Hebrew or Greek word: its Lexicon entries, translations and occurrences",
    },
    QuickStart {
        label: "Get Resources",
        command: "library.get_resources",
        help: "Download Bibles, Commentaries, Lexicons and Dictionaries",
    },
    QuickStart {
        label: "Import",
        command: "library.import",
        help: "Add a USFM or SWORD file from this computer",
    },
];

/// How many of one Resource type the library has, and where to get more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryCount {
    pub resource_type: ResourceType,
    pub count: usize,
    /// The Get Resources filter that lists this type; `None` when the online repositories
    /// do not offer it (Cross-reference sets, People & places indexes).
    pub get: Option<ResourceFilter>,
}

/// How much of the Starter library is in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterSummary {
    /// Starter library Resources in the library.
    pub in_library: usize,
    /// How many the Starter library has, when its manifest is known (bundled or fetched).
    pub offered: Option<usize>,
}

/// History rows the History and Favorites card lists after "Continue where you left off".
pub const HOME_HISTORY_ROWS: usize = 5;
/// Favorites the History and Favorites card lists.
pub const HOME_FAVORITE_ROWS: usize = 6;
/// Reading plans the Today's reading card lists.
pub const HOME_PLAN_ROWS: usize = 3;

impl Session {
    /// Load the Home arrangement from the User database.
    pub(crate) fn load_home(&mut self) {
        self.home = self
            .user
            .as_ref()
            .and_then(|u| u.preference(HOME_PREF).ok().flatten())
            .map(|json| HomeSettings::from_json(&json))
            .unwrap_or_default();
    }

    /// The Home arrangement and "Open Home at launch".
    pub fn home_settings(&self) -> &HomeSettings {
        &self.home
    }

    /// Whether Home opens at launch on top of the Last session.
    pub fn home_opens_at_launch(&self) -> bool {
        self.home.open_at_launch
    }

    /// Change the Home settings and save them.
    fn change_home(&mut self, change: impl FnOnce(&mut HomeSettings)) -> Value {
        change(&mut self.home);
        if let Some(user) = &self.user
            && let Err(e) = user.set_preference(HOME_PREF, &self.home.to_json())
        {
            self.workspace.push_effect(Effect::Message(format!(
                "Could not save the Home cards: {e}"
            )));
        }
        home_json(&self.home)
    }

    /// The Today's reading card: each unfinished Reading plan (the first few), with the
    /// Readings due today.
    pub fn home_plans(&self) -> Vec<HomePlan> {
        let today = self.plan_today();
        self.reading_plans()
            .iter()
            .filter(|p| !p.is_finished())
            .take(HOME_PLAN_ROWS)
            .map(|p| {
                let due = p.due_on(today);
                let behind = p.behind_days(today);
                let next = if due.is_empty() {
                    p.next_day(today)
                } else {
                    None
                };
                let readings: Vec<_> = if due.is_empty() && behind > 0 {
                    p.todays_reading(today).into_iter().collect()
                } else {
                    due
                };
                HomePlan {
                    id: p.id,
                    name: p.name.clone(),
                    readings: readings
                        .into_iter()
                        .map(|r| HomeReading {
                            position: r.position,
                            label: r.label.clone(),
                            read: r.read,
                        })
                        .collect(),
                    behind,
                    next,
                }
            })
            .collect()
    }

    /// "Continue where you left off": the newest Visit whose Resource is still in the
    /// library.
    pub fn home_continue(&self) -> Option<&Visit> {
        self.history().iter().find(|v| self.can_open(v))
    }

    /// The History rows after "Continue where you left off": the newest Visits, each place
    /// once.
    pub fn home_history(&self) -> Vec<&Visit> {
        let first = self.home_continue();
        let mut out: Vec<&Visit> = Vec::new();
        for v in self.history() {
            if out.len() == HOME_HISTORY_ROWS {
                break;
            }
            let shown = first.is_some_and(|c| c.location == v.location)
                || out.iter().any(|o| o.location == v.location);
            if !shown && self.can_open(v) {
                out.push(v);
            }
        }
        out
    }

    /// The Favorites the card lists: the first few in Favorites order.
    pub fn home_favorites(&self) -> &[Favorite] {
        let all = self.favorites();
        &all[..all.len().min(HOME_FAVORITE_ROWS)]
    }

    fn can_open(&self, v: &Visit) -> bool {
        match &v.location.kind {
            TabKind::Resource(id) => self.library.get(id).is_some(),
            _ => true,
        }
    }

    /// Today's Verse of the day, read in the ★ Bible.
    pub fn verse_of_the_day(&self) -> VerseOfTheDay {
        let passage = verse_of_the_day_on(self.plan_today());
        VerseOfTheDay {
            passage,
            label: format_passage(passage, &Standard, RefStyle::Long),
            bible: self
                .library
                .preferred(ResourceType::Bible)
                .map(|e| e.manifest.resource_id.clone()),
        }
    }

    /// What a Quick start runs with: the Passage Guide opens at the Passage the most recent
    /// Bible Tab shows, else at the Verse of the day.
    pub fn quick_start_params(&self, q: &QuickStart) -> Value {
        if q.command != "guide.passage_guide" {
            return json!({});
        }
        let library = &self.library;
        let from_bible = self
            .workspace
            .most_recent(|t| {
                matches!(&t.kind, TabKind::Resource(id)
                    if library.get(id).is_some_and(|e| e.manifest.resource_type == ResourceType::Bible))
            })
            .and_then(|t| self.workspace.tab(t))
            .and_then(|t| {
                match crate::link::to_standard(t.verse_map.as_deref(), t.history.current()?) {
                    crate::link::ToStandard::Verse(v) => Some(Passage::verse(v)),
                    crate::link::ToStandard::Unmapped => None,
                }
            });
        let passage = from_bible.unwrap_or_else(|| verse_of_the_day_on(self.plan_today()));
        json!({ "passage": format_passage(passage, &Standard, RefStyle::Short) })
    }

    /// The Your library card: each Resource type with how many the library has.
    pub fn library_summary(&self) -> Vec<LibraryCount> {
        crate::toolbar::TYPE_ORDER
            .into_iter()
            .map(|ty| LibraryCount {
                resource_type: ty,
                count: self.library.of_type(ty).count(),
                get: Some(ResourceFilter::Type(ty)).filter(|f| f.id().is_some()),
            })
            .collect()
    }

    /// How much of the Starter library is in the library.
    pub fn starter_summary(&self) -> StarterSummary {
        StarterSummary {
            in_library: self
                .library
                .entries()
                .iter()
                .filter(|e| e.manifest.origin == obs_model::Origin::Starter)
                .count(),
            offered: self.starter.manifest().map(|m| m.resources.len()),
        }
    }

    /// Open Home (or focus it). At launch it goes in the reading Panel, over the ★ Bible.
    fn open_home(&mut self, req: OpenRequest) -> TabId {
        crate::workspace_commands::open_tab(self, TabKind::Home, req)
    }
}

fn home_json(h: &HomeSettings) -> Value {
    json!({
        "cards": h.cards.iter().map(|p| json!({ "card": p.card.id(), "hidden": p.hidden })).collect::<Vec<_>>(),
        "open_at_launch": h.open_at_launch,
    })
}

// -------------------------------------------------------------------------------- commands

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

const CARD: ParamSpec = ParamSpec {
    name: "card",
    kind: ParamKind::String,
    required: true,
    help: "todays_reading, continue, history_favorites, verse_of_the_day, quick_start or library",
};

fn card_param(command: &'static str, p: &Params<'_>) -> Result<HomeCard, CommandError> {
    let id = p.str("card").unwrap_or_default();
    HomeCard::from_id(id).ok_or_else(|| failed(command, format!("Home has no card `{id}`")))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "home.open",
        label: "Home",
        shortcut: Some("Alt+H"),
        params: &[
            ParamSpec {
                name: "panel",
                kind: ParamKind::Integer,
                required: false,
                help: "Panel to open in",
            },
            ParamSpec {
                name: "split",
                kind: ParamKind::Boolean,
                required: false,
                help: "Open in a new Panel split off to the right",
            },
        ],
        enabled: always,
        run: |s, p| {
            let req = OpenRequest {
                panel: p
                    .i64("panel")
                    .and_then(|n| u64::try_from(n).ok())
                    .map(PanelId),
                split: p.bool("split").unwrap_or(false),
            };
            let tab = s.open_home(req);
            Ok(json!({ "tab": tab.0 }))
        },
    },
    CommandSpec {
        id: "home.move_card",
        label: "Move Home card",
        shortcut: None,
        params: &[
            CARD,
            ParamSpec {
                name: "to",
                kind: ParamKind::Integer,
                required: true,
                help: "New position on Home; 0 is the first",
            },
        ],
        enabled: always,
        run: |s, p| {
            const ID: &str = "home.move_card";
            let card = card_param(ID, &p)?;
            let to = p
                .i64("to")
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| failed(ID, "`to` must be 0 or more"))?;
            Ok(s.change_home(|h| {
                h.move_to(card, to);
            }))
        },
    },
    CommandSpec {
        id: "home.hide_card",
        label: "Hide or show Home card",
        shortcut: None,
        params: &[
            CARD,
            ParamSpec {
                name: "hidden",
                kind: ParamKind::Boolean,
                required: false,
                help: "Hide (true, the default) or show again (false)",
            },
        ],
        enabled: always,
        run: |s, p| {
            let card = card_param("home.hide_card", &p)?;
            let hidden = p.bool("hidden").unwrap_or(true);
            Ok(s.change_home(|h| {
                h.set_hidden(card, hidden);
            }))
        },
    },
    CommandSpec {
        id: "home.reset_cards",
        label: "Reset Home cards",
        shortcut: None,
        params: &[],
        enabled: always,
        run: |s, _| {
            Ok(s.change_home(|h| {
                h.cards = HomeSettings::default().cards;
            }))
        },
    },
    CommandSpec {
        id: "home.open_at_launch",
        label: "Open Home at launch",
        shortcut: None,
        params: &[ParamSpec {
            name: "on",
            kind: ParamKind::Boolean,
            required: false,
            help: "Open Home at every launch, on top of the Last session (toggles when missing)",
        }],
        enabled: always,
        run: |s, p| {
            let on = p.bool("on").unwrap_or(!s.home.open_at_launch);
            Ok(s.change_home(|h| h.open_at_launch = on))
        },
    },
];
