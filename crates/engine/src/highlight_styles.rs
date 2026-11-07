//! Highlight styles and Highlight palettes (#181, ADR 0012): the user's own looks for
//! Highlights, grouped into palettes, one of which is active.
//!
//! - Styles and palettes are rows of the annotation tables, so every edit here is one undo
//!   step with Ctrl+Z like any Annotation edit. Switching the active palette is a preference
//!   (`highlights.palette`), not an edit, and is not undone.
//! - **Deleting a style** that Highlights use moves them to a replacement style the user
//!   chooses, in the same undo step; undo brings back the style and puts them back on it.
//!   Annotations are never dropped (ADR 0006), and the store refuses to delete a style still
//!   in use. Deleting a palette does the same for every style in it.
//! - A style's colour per Page tone is derived when drawn unless the user adjusted it
//!   (`paper`, `sepia`, `dark`); the contrast check of an adjustment (#21) belongs to the UI,
//!   which knows the tones' colours. Changing the colour drops the adjustments.
//! - The active palette's first nine styles are the Selection keys Ctrl+1…9.
//! - **Export** writes a palette as a small JSON file ([`PaletteFile`]): names, colours,
//!   looks and adjustments, no ids. **Import** always makes a new palette from one.

use std::collections::HashMap;

use obs_store::annotation::{new_id_at, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use obs_store::{
    HighlightLook, HighlightPalette, HighlightStyle, MAX_STYLE_NAME, Rgb, STANDARD_PALETTE_ID,
    StyleAppearance,
};

use crate::annotations::{Annotation, Change, Content, Row, failed, to_json};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::session::Session;

/// User database preference holding the active Highlight palette's id.
pub const ACTIVE_PALETTE_PREF: &str = "highlights.palette";

/// How many styles of the active palette have a Selection key (Ctrl+1 … Ctrl+9).
pub const STYLE_SLOTS: usize = 9;

/// `format` of an exported palette file.
pub const PALETTE_FILE_FORMAT: &str = "openbible-highlight-palette";

/// A Page tone a style can have its own colour for (Auto resolves to one of these).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleTone {
    Paper,
    Sepia,
    Dark,
}

impl StyleTone {
    pub const ALL: [StyleTone; 3] = [StyleTone::Paper, StyleTone::Sepia, StyleTone::Dark];

    pub fn id(self) -> &'static str {
        match self {
            StyleTone::Paper => "paper",
            StyleTone::Sepia => "sepia",
            StyleTone::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StyleTone::Paper => "Paper",
            StyleTone::Sepia => "Sepia",
            StyleTone::Dark => "Dark",
        }
    }

    /// The colour the user chose for this tone, if any.
    pub fn adjusted(self, a: &StyleAppearance) -> Option<Rgb> {
        match self {
            StyleTone::Paper => a.paper,
            StyleTone::Sepia => a.sepia,
            StyleTone::Dark => a.dark,
        }
    }

    fn slot(self, a: &mut StyleAppearance) -> &mut Option<Rgb> {
        match self {
            StyleTone::Paper => &mut a.paper,
            StyleTone::Sepia => &mut a.sepia,
            StyleTone::Dark => &mut a.dark,
        }
    }
}

/// A change to a style; `None` fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleEdit {
    pub name: Option<String>,
    pub look: Option<HighlightLook>,
    /// A new colour drops the tone adjustments not given in `tones`.
    pub color: Option<Rgb>,
    /// Per tone: `Some(Some(c))` adjusts it to `c`, `Some(None)` goes back to derived.
    pub tones: Vec<(StyleTone, Option<Rgb>)>,
}

/// One style in an exported palette file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteFileStyle {
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub appearance: StyleAppearance,
}

/// A palette as exported: a small JSON file another user (or another computer) imports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteFile {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub styles: Vec<PaletteFileStyle>,
}

/// A style's or palette's name, trimmed and checked; a style's may be empty.
fn clean_name(name: &str, what: &str, may_be_empty: bool) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() && !may_be_empty {
        return Err(format!("A {what} needs a name"));
    }
    if name.chars().count() > MAX_STYLE_NAME {
        return Err(format!(
            "{what} names are at most {MAX_STYLE_NAME} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(format!("{what} names cannot contain control characters"));
    }
    Ok(name.to_string())
}

/// `base` numbered `n` ("Themes (2)"), shortened so the whole stays within
/// [`MAX_STYLE_NAME`]: an imported palette's name must import again once exported.
fn numbered_name(base: &str, n: u32) -> String {
    let suffix = format!(" ({n})");
    let room = MAX_STYLE_NAME.saturating_sub(suffix.chars().count());
    let stem: String = base.chars().take(room).collect();
    format!("{}{suffix}", stem.trim_end())
}

/// What the UI calls a style: its name, or its place in its palette ("Style 3").
pub fn style_label(style: &HighlightStyle, styles: &[HighlightStyle]) -> String {
    if !style.name.is_empty() {
        return style.name.clone();
    }
    let n = styles
        .iter()
        .filter(|s| s.palette_id == style.palette_id)
        .position(|s| s.id == style.id)
        .map_or(1, |i| i + 1);
    format!("Style {n}")
}

impl Session {
    /// Every Highlight palette, in order.
    pub fn highlight_palettes(&self) -> Result<Vec<HighlightPalette>, String> {
        self.user_data()?.highlight_palettes()
    }

    /// Every Highlight style, by palette and then in order within it.
    pub fn highlight_styles(&self) -> Result<Vec<HighlightStyle>, String> {
        self.user_data()?.highlight_styles()
    }

    pub fn highlight_style(&self, id: &str) -> Result<HighlightStyle, String> {
        self.highlight_styles()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("There is no Highlight style “{id}”"))
    }

    pub fn highlight_palette(&self, id: &str) -> Result<HighlightPalette, String> {
        self.highlight_palettes()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("There is no Highlight palette “{id}”"))
    }

    /// The styles of one palette, in order.
    pub fn palette_styles(&self, palette_id: &str) -> Result<Vec<HighlightStyle>, String> {
        Ok(self
            .highlight_styles()?
            .into_iter()
            .filter(|s| s.palette_id == palette_id)
            .collect())
    }

    /// The active palette: the one chosen last, else the first.
    pub fn active_highlight_palette(&self) -> Result<HighlightPalette, String> {
        let all = self.highlight_palettes()?;
        let chosen = self.user_data()?.preference(ACTIVE_PALETTE_PREF)?;
        all.iter()
            .find(|p| Some(&p.id) == chosen.as_ref())
            .or(all.first())
            .cloned()
            .ok_or_else(|| "There is no Highlight palette".to_string())
    }

    /// The active palette's styles, in order: the selection toolbar's swatches and, the
    /// first nine, Ctrl+1…9.
    pub fn active_highlight_styles(&self) -> Result<Vec<HighlightStyle>, String> {
        self.palette_styles(&self.active_highlight_palette()?.id)
    }

    /// How many Highlights use each style.
    pub fn highlight_style_uses(&self) -> Result<HashMap<String, u64>, String> {
        Ok(self
            .user_data()?
            .highlight_style_uses()?
            .into_iter()
            .collect())
    }

    /// Make `id` the active palette.
    pub fn set_active_highlight_palette(&mut self, id: &str) -> Result<HighlightPalette, String> {
        let palette = self.highlight_palette(id)?;
        self.user_data()?
            .set_preference(ACTIVE_PALETTE_PREF, &palette.id)?;
        // Views reload what they show of the active palette with the Annotations.
        self.annotations_revision += 1;
        Ok(palette)
    }

    /// Check a style name is usable and, when not empty, not taken by another style of the
    /// palette (ignoring case).
    fn style_name(
        &self,
        palette: &str,
        name: &str,
        except: Option<&str>,
    ) -> Result<String, String> {
        let name = clean_name(name, "Highlight style", true)?;
        if name.is_empty() {
            return Ok(name);
        }
        let taken = self
            .palette_styles(palette)?
            .into_iter()
            .any(|s| Some(s.id.as_str()) != except && s.name.to_lowercase() == name.to_lowercase());
        if taken {
            return Err(format!("This palette already has a style named “{name}”"));
        }
        Ok(name)
    }

    fn palette_name(&self, name: &str, except: Option<&str>) -> Result<String, String> {
        let name = clean_name(name, "Highlight palette", false)?;
        let taken = self
            .highlight_palettes()?
            .into_iter()
            .any(|p| Some(p.id.as_str()) != except && p.name.to_lowercase() == name.to_lowercase());
        if taken {
            return Err(format!(
                "There is already a Highlight palette named “{name}”"
            ));
        }
        Ok(name)
    }

    /// Add a style at the end of `palette` (default: the active one).
    pub fn create_highlight_style(
        &mut self,
        palette: Option<&str>,
        name: &str,
        appearance: StyleAppearance,
    ) -> Result<HighlightStyle, String> {
        let palette = match palette {
            Some(id) => self.highlight_palette(id)?,
            None => self.active_highlight_palette()?,
        };
        let name = self.style_name(&palette.id, name, None)?;
        let position = self
            .palette_styles(&palette.id)?
            .last()
            .map_or(0, |s| s.position + 1);
        let now = now_ms();
        let style = HighlightStyle {
            id: new_id_at(now),
            palette_id: palette.id,
            position,
            name,
            appearance,
            created_at: now,
            modified_at: now,
        };
        self.commit(
            "New Highlight style",
            vec![Change::insert(Row::HighlightStyle(style.clone()))],
        )?;
        Ok(style)
    }

    /// Rename a style, change its look or colour, or adjust or reset its colour for a tone.
    pub fn edit_highlight_style(
        &mut self,
        id: &str,
        edit: &StyleEdit,
    ) -> Result<HighlightStyle, String> {
        let before = self.highlight_style(id)?;
        let mut after = before.clone();
        if let Some(name) = &edit.name {
            after.name = self.style_name(&before.palette_id, name, Some(id))?;
        }
        if let Some(look) = edit.look {
            after.appearance.look = look;
        }
        if let Some(color) = edit.color
            && color != before.appearance.color
        {
            after.appearance = StyleAppearance::new(after.appearance.look, color);
        }
        for (tone, color) in &edit.tones {
            *tone.slot(&mut after.appearance) = *color;
        }
        if after == before {
            return Ok(before);
        }
        after.modified_at = now_ms().max(before.modified_at);
        self.commit(
            "Edit Highlight style",
            vec![Change::update(
                Row::HighlightStyle(before),
                Row::HighlightStyle(after.clone()),
            )],
        )?;
        Ok(after)
    }

    /// Move a style to place `to` (0 first) in its palette; the others keep their order.
    pub fn move_highlight_style(
        &mut self,
        id: &str,
        to: usize,
    ) -> Result<Vec<HighlightStyle>, String> {
        let style = self.highlight_style(id)?;
        let mut order = self.palette_styles(&style.palette_id)?;
        let from = order
            .iter()
            .position(|s| s.id == id)
            .ok_or("The style is not in its palette")?;
        let moved = order.remove(from);
        order.insert(to.min(order.len()), moved);
        let mut changes = Vec::new();
        let mut placed = Vec::new();
        for (i, s) in order.into_iter().enumerate() {
            let position = i64::try_from(i).unwrap_or(i64::MAX);
            if s.position == position {
                placed.push(s);
            } else {
                let after = HighlightStyle {
                    position,
                    ..s.clone()
                };
                changes.push(Change::update(
                    Row::HighlightStyle(s),
                    Row::HighlightStyle(after.clone()),
                ));
                placed.push(after);
            }
        }
        self.commit("Reorder Highlight styles", changes)?;
        Ok(placed)
    }

    /// Changes moving every Highlight on one of `styles` to `replacement`, which must exist
    /// and not be one of them. Without a replacement, only when no Highlight uses them.
    fn move_highlights_off(
        &self,
        styles: &[String],
        replacement: Option<&str>,
    ) -> Result<Vec<Change>, String> {
        let users: Vec<Annotation> = self
            .user_data()?
            .annotations(None)?
            .into_iter()
            .filter(
                |a| matches!(&a.content, Content::Highlight { style } if styles.contains(style)),
            )
            .collect();
        if users.is_empty() {
            return Ok(Vec::new());
        }
        let Some(replacement) = replacement else {
            return Err(format!(
                "{} Highlight(s) use it: choose a style to move them to",
                users.len()
            ));
        };
        let replacement = self.highlight_style(replacement)?;
        if styles.contains(&replacement.id) {
            return Err("Choose a style that is not being deleted".into());
        }
        Ok(users
            .into_iter()
            .map(|a| {
                let moved = Annotation {
                    content: Content::Highlight {
                        style: replacement.id.clone(),
                    },
                    ..a.clone()
                };
                Change::update(Row::Annotation(a), Row::Annotation(moved))
            })
            .collect())
    }

    /// Delete a style, moving the Highlights that use it to `replacement`, as one undo step.
    /// Returns how many Highlights moved.
    pub fn delete_highlight_style(
        &mut self,
        id: &str,
        replacement: Option<&str>,
    ) -> Result<usize, String> {
        let style = self.highlight_style(id)?;
        let mut changes = self.move_highlights_off(std::slice::from_ref(&style.id), replacement)?;
        let moved = changes.len();
        changes.push(Change::delete(Row::HighlightStyle(style)));
        self.commit("Delete Highlight style", changes)?;
        Ok(moved)
    }

    /// Add an empty palette after the others.
    pub fn create_highlight_palette(&mut self, name: &str) -> Result<HighlightPalette, String> {
        let name = self.palette_name(name, None)?;
        let palette = self.new_palette(name)?;
        self.commit(
            "New Highlight palette",
            vec![Change::insert(Row::HighlightPalette(palette.clone()))],
        )?;
        Ok(palette)
    }

    fn new_palette(&self, name: String) -> Result<HighlightPalette, String> {
        let position = self
            .highlight_palettes()?
            .last()
            .map_or(0, |p| p.position + 1);
        let now = now_ms();
        Ok(HighlightPalette {
            id: new_id_at(now),
            name,
            position,
            created_at: now,
            modified_at: now,
        })
    }

    pub fn rename_highlight_palette(
        &mut self,
        id: &str,
        name: &str,
    ) -> Result<HighlightPalette, String> {
        let before = self.highlight_palette(id)?;
        let name = self.palette_name(name, Some(id))?;
        if name == before.name {
            return Ok(before);
        }
        let after = HighlightPalette {
            name,
            modified_at: now_ms().max(before.modified_at),
            ..before.clone()
        };
        self.commit(
            "Rename Highlight palette",
            vec![Change::update(
                Row::HighlightPalette(before),
                Row::HighlightPalette(after.clone()),
            )],
        )?;
        Ok(after)
    }

    /// Delete a palette (never the last) with its styles, moving the Highlights on them to
    /// `replacement` (a style of another palette), as one undo step. Returns how many
    /// Highlights moved.
    pub fn delete_highlight_palette(
        &mut self,
        id: &str,
        replacement: Option<&str>,
    ) -> Result<usize, String> {
        let palette = self.highlight_palette(id)?;
        if self.highlight_palettes()?.len() <= 1 {
            return Err("The last Highlight palette cannot be deleted".into());
        }
        let styles = self.palette_styles(id)?;
        let ids: Vec<String> = styles.iter().map(|s| s.id.clone()).collect();
        let mut changes = self.move_highlights_off(&ids, replacement)?;
        let moved = changes.len();
        changes.extend(
            styles
                .into_iter()
                .map(|s| Change::delete(Row::HighlightStyle(s))),
        );
        changes.push(Change::delete(Row::HighlightPalette(palette)));
        self.commit("Delete Highlight palette", changes)?;
        Ok(moved)
    }

    /// A palette as an export file's contents.
    pub fn export_highlight_palette(&self, id: &str) -> Result<PaletteFile, String> {
        let palette = self.highlight_palette(id)?;
        Ok(PaletteFile {
            format: PALETTE_FILE_FORMAT.into(),
            version: 1,
            name: palette.name,
            styles: self
                .palette_styles(id)?
                .into_iter()
                .map(|s| PaletteFileStyle {
                    name: s.name,
                    appearance: s.appearance,
                })
                .collect(),
        })
    }

    /// Make a new palette from an export file's contents, as one undo step. A name already
    /// taken gets a number ("Themes (2)").
    pub fn import_highlight_palette(
        &mut self,
        file: &PaletteFile,
    ) -> Result<HighlightPalette, String> {
        if file.format != PALETTE_FILE_FORMAT || file.version != 1 {
            return Err("This is not a Highlight palette file this version can read".into());
        }
        let base = clean_name(&file.name, "Highlight palette", false)?;
        let taken: Vec<String> = self
            .highlight_palettes()?
            .into_iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        let name = std::iter::once(base.clone())
            .chain((2..).map(|n| numbered_name(&base, n)))
            .find(|n| !taken.contains(&n.to_lowercase()))
            .unwrap_or(base);
        let palette = self.new_palette(name)?;
        let mut names: Vec<String> = Vec::new();
        let mut changes = vec![Change::insert(Row::HighlightPalette(palette.clone()))];
        for (i, s) in file.styles.iter().enumerate() {
            let name = clean_name(&s.name, "Highlight style", true)?;
            if !name.is_empty() {
                if names.contains(&name.to_lowercase()) {
                    return Err(format!("The file has two styles named “{name}”"));
                }
                names.push(name.to_lowercase());
            }
            changes.push(Change::insert(Row::HighlightStyle(HighlightStyle {
                id: new_id_at(palette.created_at),
                palette_id: palette.id.clone(),
                position: i64::try_from(i).unwrap_or(i64::MAX),
                name,
                appearance: s.appearance,
                created_at: palette.created_at,
                modified_at: palette.created_at,
            })));
        }
        self.commit("Import Highlight palette", changes)?;
        Ok(palette)
    }
}

// -------------------------------------------------------------------------------- commands

const STYLE_ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Highlight style id",
};
const PALETTE_ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Highlight palette id",
};
const PALETTE: ParamSpec = ParamSpec {
    name: "palette",
    kind: ParamKind::String,
    required: false,
    help: "Highlight palette id (default: the active palette)",
};
const NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::String,
    required: false,
    help: "Name, such as “Promises” (empty: none)",
};
const LOOK: ParamSpec = ParamSpec {
    name: "look",
    kind: ParamKind::String,
    required: false,
    help: "background (default), underline, box or text_color",
};
const COLOR: ParamSpec = ParamSpec {
    name: "color",
    kind: ParamKind::String,
    required: false,
    help: "Colour as #rrggbb",
};
const REPLACEMENT: ParamSpec = ParamSpec {
    name: "replacement",
    kind: ParamKind::String,
    required: false,
    help: "Style the Highlights that use it move to (needed when any do)",
};
const fn tone_param(name: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::String,
        required: false,
        help: "This Page tone's colour as #rrggbb, or empty to derive it",
    }
}

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "highlight_style.list",
        label: "List Highlight styles",
        shortcut: None,
        params: &[PALETTE],
        enabled: has_user,
        run: run_list,
    },
    CommandSpec {
        id: "highlight_style.create",
        label: "New Highlight style",
        shortcut: None,
        params: &[
            PALETTE,
            NAME,
            LOOK,
            ParamSpec {
                required: true,
                ..COLOR
            },
        ],
        enabled: has_user,
        run: run_create,
    },
    CommandSpec {
        id: "highlight_style.edit",
        label: "Edit Highlight style",
        shortcut: None,
        params: &[
            STYLE_ID,
            NAME,
            LOOK,
            COLOR,
            tone_param("paper"),
            tone_param("sepia"),
            tone_param("dark"),
        ],
        enabled: has_user,
        run: run_edit,
    },
    CommandSpec {
        id: "highlight_style.move",
        label: "Reorder Highlight styles",
        shortcut: None,
        params: &[
            STYLE_ID,
            ParamSpec {
                name: "position",
                kind: ParamKind::Integer,
                required: true,
                help: "New place in its palette, 0 first",
            },
        ],
        enabled: has_user,
        run: run_move,
    },
    CommandSpec {
        id: "highlight_style.delete",
        label: "Delete Highlight style",
        shortcut: None,
        params: &[STYLE_ID, REPLACEMENT],
        enabled: has_user,
        run: run_delete,
    },
    CommandSpec {
        id: "highlight_palette.create",
        label: "New Highlight palette",
        shortcut: None,
        params: &[ParamSpec {
            required: true,
            ..NAME
        }],
        enabled: has_user,
        run: run_palette_create,
    },
    CommandSpec {
        id: "highlight_palette.rename",
        label: "Rename Highlight palette",
        shortcut: None,
        params: &[
            PALETTE_ID,
            ParamSpec {
                required: true,
                ..NAME
            },
        ],
        enabled: has_user,
        run: run_palette_rename,
    },
    CommandSpec {
        id: "highlight_palette.delete",
        label: "Delete Highlight palette",
        shortcut: None,
        params: &[PALETTE_ID, REPLACEMENT],
        enabled: has_user,
        run: run_palette_delete,
    },
    CommandSpec {
        id: "highlight_palette.activate",
        label: "Use Highlight palette",
        shortcut: None,
        params: &[PALETTE_ID],
        enabled: has_user,
        run: run_palette_activate,
    },
    CommandSpec {
        id: "highlight_palette.export",
        label: "Export Highlight palette",
        shortcut: None,
        params: &[
            PALETTE_ID,
            ParamSpec {
                name: "path",
                kind: ParamKind::String,
                required: false,
                help: "Write the JSON file here (otherwise only returned)",
            },
        ],
        enabled: has_user,
        run: run_palette_export,
    },
    CommandSpec {
        id: "highlight_palette.import",
        label: "Import Highlight palette",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "path",
                kind: ParamKind::String,
                required: false,
                help: "A palette's JSON file",
            },
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: false,
                help: "The JSON itself, instead of a file",
            },
        ],
        enabled: has_user,
        run: run_palette_import,
    },
];

fn look(cmd: &'static str, p: &Params<'_>) -> Result<Option<HighlightLook>, CommandError> {
    p.str("look")
        .map(|id| {
            HighlightLook::from_id(id).ok_or_else(|| {
                failed(
                    cmd,
                    format!("“{id}” is not a look (background, underline, box, text_color)"),
                )
            })
        })
        .transpose()
}

fn color(cmd: &'static str, p: &Params<'_>, name: &str) -> Result<Option<Rgb>, CommandError> {
    p.str(name)
        .map(|c| c.parse::<Rgb>().map_err(|e| failed(cmd, e)))
        .transpose()
}

fn run_list(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_style.list";
    let palette = match p.str("palette") {
        Some(id) => s.highlight_palette(id),
        None => s.active_highlight_palette(),
    }
    .map_err(|e| failed(CMD, e))?;
    let styles = s.palette_styles(&palette.id).map_err(|e| failed(CMD, e))?;
    let uses = s.highlight_style_uses().map_err(|e| failed(CMD, e))?;
    let active = s.active_highlight_palette().map_err(|e| failed(CMD, e))?;
    Ok(json!({
        "palette": to_json(&palette),
        "active": palette.id == active.id,
        "styles": styles.iter().map(|st| {
            let mut v = to_json(st);
            v["label"] = json!(style_label(st, &styles));
            v["uses"] = json!(uses.get(&st.id).copied().unwrap_or(0));
            v
        }).collect::<Vec<_>>(),
        "palettes": to_json(&s.highlight_palettes().map_err(|e| failed(CMD, e))?),
    }))
}

fn run_create(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_style.create";
    let look = look(CMD, &p)?.unwrap_or(HighlightLook::Background);
    let color = color(CMD, &p, "color")?.ok_or_else(|| failed(CMD, "a style needs a colour"))?;
    let style = s
        .create_highlight_style(
            p.str("palette"),
            p.str("name").unwrap_or_default(),
            StyleAppearance::new(look, color),
        )
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "id": style.id, "style": to_json(&style) }))
}

fn run_edit(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_style.edit";
    let mut tones = Vec::new();
    for tone in StyleTone::ALL {
        match p.str(tone.id()) {
            None => {}
            Some(c) if c.trim().is_empty() => tones.push((tone, None)),
            Some(c) => tones.push((tone, Some(c.parse::<Rgb>().map_err(|e| failed(CMD, e))?))),
        }
    }
    let edit = StyleEdit {
        name: p.str("name").map(str::to_string),
        look: look(CMD, &p)?,
        color: color(CMD, &p, "color")?,
        tones,
    };
    let style = s
        .edit_highlight_style(p.str("id").unwrap_or_default(), &edit)
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "style": to_json(&style) }))
}

fn run_move(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_style.move";
    let to = usize::try_from(p.i64("position").unwrap_or(0).max(0)).unwrap_or(0);
    let styles = s
        .move_highlight_style(p.str("id").unwrap_or_default(), to)
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "styles": to_json(&styles) }))
}

fn run_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_style.delete";
    let moved = s
        .delete_highlight_style(p.str("id").unwrap_or_default(), p.str("replacement"))
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "moved": moved }))
}

fn run_palette_create(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_palette.create";
    let palette = s
        .create_highlight_palette(p.str("name").unwrap_or_default())
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "id": palette.id, "palette": to_json(&palette) }))
}

fn run_palette_rename(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_palette.rename";
    let palette = s
        .rename_highlight_palette(
            p.str("id").unwrap_or_default(),
            p.str("name").unwrap_or_default(),
        )
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "palette": to_json(&palette) }))
}

fn run_palette_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_palette.delete";
    let moved = s
        .delete_highlight_palette(p.str("id").unwrap_or_default(), p.str("replacement"))
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "moved": moved }))
}

fn run_palette_activate(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_palette.activate";
    let palette = s
        .set_active_highlight_palette(p.str("id").unwrap_or_default())
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "palette": to_json(&palette) }))
}

fn run_palette_export(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_palette.export";
    let file = s
        .export_highlight_palette(p.str("id").unwrap_or_default())
        .map_err(|e| failed(CMD, e))?;
    let text = serde_json::to_string_pretty(&file).map_err(|e| failed(CMD, e))?;
    if let Some(path) = p.str("path") {
        std::fs::write(path, &text).map_err(|e| failed(CMD, format!("{path}: {e}")))?;
    }
    Ok(json!({ "name": file.name, "styles": file.styles.len(), "text": text }))
}

fn run_palette_import(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "highlight_palette.import";
    let text = match (p.str("text"), p.str("path")) {
        (Some(t), _) => t.to_string(),
        (None, Some(path)) => {
            std::fs::read_to_string(path).map_err(|e| failed(CMD, format!("{path}: {e}")))?
        }
        (None, None) => return Err(failed(CMD, "give a `path` or the `text` of a palette")),
    };
    let file: PaletteFile = serde_json::from_str(&text)
        .map_err(|e| failed(CMD, format!("This is not a Highlight palette file: {e}")))?;
    let palette = s
        .import_highlight_palette(&file)
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "id": palette.id, "palette": to_json(&palette), "styles": file.styles.len() }))
}
