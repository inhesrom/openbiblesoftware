//! The Search Tool's results views (#146), above Bible mode's results: Verses (the Verse
//! list), Grid (a heat map of matching Verses by Bible book × Bible), Chart (bars by Bible
//! book, or by chapter of one book) and Analysis (a sortable table of the matched words,
//! exported as CSV). A Grid cell or Chart bar narrows the Verse list to it and shows it; the
//! filter reads as a chip above the list until it is cleared.
//!
//! Book names and counts are chrome (egui); the Analysis table's words, Lemmas and Context
//! are Resource text and go through the text pipeline, like the Verse list (ADR 0004).

use std::ops::Range;

use egui::{Align2, Color32, FontId, Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, vec2};
use obs_engine::{AnalysisColumn, ChartBy, HitGrid, ResultsView, SearchTab, TabId, chart_bars};
use obs_layout::ColorRole;
use serde_json::{Value, json};

use super::State;
use crate::theme;

/// A Grid cell.
const CELL: egui::Vec2 = vec2(64.0, 24.0);
/// The Chart's label column.
const BAR_LABEL: f32 = 130.0;
const BAR_HEIGHT: f32 = 20.0;
/// The longest bar.
const BAR_MAX: f32 = 640.0;
/// The Analysis table's fixed columns (Context takes the rest).
const COLUMN_WIDTHS: [f32; 5] = [150.0, 64.0, 130.0, 120.0, 100.0];
const CONTEXT_MIN: f32 = 260.0;

/// A Grid cell's fill: none for no hit, then from a pale to a full lapis by heat.
pub(super) fn heat_fill(heat: f32) -> Color32 {
    let c = theme::c();
    if heat <= 0.0 {
        c.field
    } else {
        c.field.lerp_to_gamma(c.lapis, 0.15 + 0.85 * heat)
    }
}

/// Whether a text is Hebrew (laid out right to left).
fn is_hebrew(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x0590..=0x05FF | 0xFB1D..=0xFB4F))
}

impl State {
    /// The Verses / Grid / Chart / Analysis switch, and in Verses the Results filter's chip.
    pub(super) fn results_view_switch(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for v in ResultsView::ALL {
                let r = ui.selectable_label(st.view == v, v.label());
                if r.clicked() && st.view != v {
                    run.push(("search.view", json!({ "tab": tab.0, "value": v.id() })));
                }
            }
            if st.view == ResultsView::Verses
                && let (Some(filter), Some(all)) = (st.filter, st.all_results())
            {
                ui.add_space(12.0);
                egui::Frame::new()
                    .fill(theme::c().field)
                    .stroke(Stroke::new(1.0, theme::c().rule))
                    .corner_radius(11.0)
                    .inner_margin(egui::Margin::symmetric(6, 0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            ui.label(
                                RichText::new(format!("Only {}", filter.label(all))).size(12.5),
                            );
                            let x = egui::Button::new(RichText::new("×").size(12.0))
                                .frame(false)
                                .min_size(vec2(16.0, 20.0));
                            if ui.add(x).on_hover_text("Show every result").clicked() {
                                run.push(("search.filter", json!({ "tab": tab.0 })));
                            }
                        });
                    });
            }
        });
        for (id, params) in run {
            self.run(id, &params);
        }
    }

    /// The Grid: a row per Bible book with a hit, a column per Bible, each cell the matching
    /// Verses shaded by how many. A cell shows that book's Verses of that Bible; a book name
    /// shows the book's Verses of every Bible.
    pub(super) fn results_grid(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        let Some(results) = st.all_results().cloned() else {
            return;
        };
        let grid = HitGrid::new(&results);
        let mut run: Vec<Value> = Vec::new();
        let muted = theme::c().muted;
        egui::ScrollArea::both()
            .id_salt(("search-grid", tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        egui::Grid::new(("search-grid-cells", tab))
                            .spacing(vec2(3.0, 3.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new("Bible book").small().color(muted));
                                for b in &grid.bibles {
                                    ui.add_sized(CELL, egui::Label::new(RichText::new(b).strong()));
                                }
                                ui.end_row();
                                for (i, book) in grid.books.iter().enumerate() {
                                    let name = egui::Button::new(book.name()).frame(false);
                                    if ui
                                        .add(name)
                                        .on_hover_text(format!(
                                            "Show the Verses in {}",
                                            book.name()
                                        ))
                                        .clicked()
                                    {
                                        run.push(json!({ "tab": tab.0, "book": book.osis() }));
                                    }
                                    for (j, count) in grid.counts[i].iter().enumerate() {
                                        let (rect, r) =
                                            ui.allocate_exact_size(CELL, Sense::click());
                                        let heat = grid.heat(*count);
                                        let painter = ui.painter();
                                        painter.rect_filled(rect, 3.0, heat_fill(heat));
                                        if r.hovered() && *count > 0 {
                                            painter.rect_stroke(
                                                rect,
                                                3.0,
                                                Stroke::new(1.5, theme::c().ink),
                                                egui::StrokeKind::Inside,
                                            );
                                        }
                                        let ink = if heat > 0.5 {
                                            Color32::WHITE
                                        } else {
                                            theme::c().ink
                                        };
                                        painter.text(
                                            rect.center(),
                                            Align2::CENTER_CENTER,
                                            count.to_string(),
                                            FontId::proportional(13.0),
                                            if *count == 0 { muted } else { ink },
                                        );
                                        let bible = &grid.bibles[j];
                                        let label = format!("{} in {bible}: {count}", book.name());
                                        r.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::Role::Button,
                                                *count > 0,
                                                &label,
                                            )
                                        });
                                        if *count > 0 {
                                            let r = r.on_hover_text(format!(
                                                "Show the {count} Verses of {} in {bible}",
                                                book.name()
                                            ));
                                            if r.clicked() {
                                                run.push(json!({
                                                    "tab": tab.0,
                                                    "book": book.osis(),
                                                    "bible": results.bibles[j].id,
                                                }));
                                            }
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
        for params in run {
            self.run("search.filter", &params);
        }
    }

    /// The Chart: a bar per Bible book with a hit, or per chapter of one book; a bar shows its
    /// Verses.
    pub(super) fn results_chart(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        let Some(results) = st.all_results().cloned() else {
            return;
        };
        let books = chart_bars(&results, ChartBy::Book);
        let bars = chart_bars(&results, st.chart);
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(st.chart == ChartBy::Book, "By book")
                        .clicked()
                    {
                        run.push(("search.chart", json!({ "tab": tab.0, "value": "books" })));
                    }
                    let current = match st.chart {
                        ChartBy::Book => "By chapter of…".to_string(),
                        ChartBy::Chapter(b) => format!("Chapters of {}", b.name()),
                    };
                    egui::ComboBox::from_id_salt(("search-chart-book", tab))
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for bar in &books {
                                let book = bar.filter.book;
                                let on = st.chart == ChartBy::Chapter(book);
                                if ui.selectable_label(on, book.name()).clicked() {
                                    run.push((
                                        "search.chart",
                                        json!({ "tab": tab.0, "value": book.osis() }),
                                    ));
                                }
                            }
                        });
                });
            });
        let max = bars.iter().map(|b| b.count).max().unwrap_or(0).max(1);
        let palette = self.palette;
        egui::ScrollArea::vertical()
            .id_salt(("search-chart", tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 4))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        for bar in &bars {
                            let (rect, r) = ui.allocate_exact_size(
                                vec2(ui.available_width(), BAR_HEIGHT),
                                if bar.count > 0 {
                                    Sense::click()
                                } else {
                                    Sense::hover()
                                },
                            );
                            let painter = ui.painter();
                            if r.hovered() && bar.count > 0 {
                                painter.rect_filled(rect, 2.0, palette.hover);
                            }
                            painter.text(
                                rect.left_center(),
                                Align2::LEFT_CENTER,
                                &bar.label,
                                FontId::proportional(13.0),
                                theme::c().ink,
                            );
                            let room = (rect.width() - BAR_LABEL - 40.0).clamp(20.0, BAR_MAX);
                            let w = room * bar.count as f32 / max as f32;
                            let bar_rect = Rect::from_min_size(
                                rect.left_top() + vec2(BAR_LABEL, 3.0),
                                vec2(w, BAR_HEIGHT - 6.0),
                            );
                            painter.rect_filled(bar_rect, 2.0, theme::c().lapis);
                            painter.text(
                                bar_rect.right_center() + vec2(6.0, 0.0),
                                Align2::LEFT_CENTER,
                                bar.count.to_string(),
                                FontId::proportional(12.0),
                                theme::c().muted,
                            );
                            let label = format!("{}: {}", bar.label, bar.count);
                            r.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, bar.count > 0, &label)
                            });
                            if bar.count > 0
                                && r.on_hover_text(format!(
                                    "Show the {} Verses of {}",
                                    bar.count, bar.label
                                ))
                                .clicked()
                            {
                                let f = bar.filter;
                                let mut p = json!({ "tab": tab.0, "book": f.book.osis() });
                                if let Some(c) = f.chapter {
                                    p["chapter"] = json!(c);
                                }
                                run.push(("search.filter", p));
                            }
                        }
                    });
            });
        for (id, params) in run {
            self.run(id, &params);
        }
    }

    /// The Analysis table: a header of sortable columns (click again to reverse), a row per
    /// matched word, and Export CSV. A row opens its Verse; Ctrl/Cmd opens a new Tab.
    pub(super) fn results_analysis(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        let muted = theme::c().muted;
        let Some((analysis, order)) = st.analysis_rows() else {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Analysing the matched words…").color(muted));
                });
            });
            if !st.analysis_running() && st.error.is_none() {
                self.run("search.view", &json!({ "tab": tab.0, "value": "analysis" }));
            }
            return;
        };
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        let mut export = false;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let n = analysis.rows.len();
                    ui.label(
                        RichText::new(format!(
                            "{n} matched {}",
                            if n == 1 { "word" } else { "words" }
                        ))
                        .color(muted),
                    );
                    export = ui
                        .button("Export CSV…")
                        .on_hover_text("Save the table, in this order, for a spreadsheet")
                        .clicked();
                });
            });
        let settings = self.session.text;
        let palette = self.palette;
        let line = settings.size * settings.line_spacing * 1.2;
        let row_height = line + 6.0;
        let full = ui.available_width();
        let fixed: f32 = COLUMN_WIDTHS.iter().sum();
        let context_width = (full - fixed - 24.0).max(CONTEXT_MIN);
        let widths: Vec<f32> = COLUMN_WIDTHS
            .iter()
            .copied()
            .chain(std::iter::once(context_width))
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + 24.0;
        let sort = st.analysis_sort;
        egui::ScrollArea::horizontal()
            .id_salt(("search-analysis-h", tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Header.
                let (head, _) = ui.allocate_exact_size(vec2(total, 26.0), Sense::hover());
                ui.painter().rect_filled(head, 0.0, theme::c().surface);
                let mut x = head.left() + 12.0;
                for (column, w) in AnalysisColumn::ALL.iter().zip(&widths) {
                    let cell = Rect::from_min_size(Pos2::new(x, head.top()), vec2(*w, 26.0));
                    let sorted = (sort.column == *column).then_some(sort.descending);
                    let r = ui
                        .interact(
                            cell,
                            ui.scope_id()
                                .with(("search-analysis-head", tab, column.id())),
                            Sense::click(),
                        )
                        .on_hover_text(format!("Sort by {}", column.label()));
                    let ink = if r.hovered() || sorted.is_some() {
                        theme::c().ink
                    } else {
                        theme::c().lapis
                    };
                    let text = ui.painter().text(
                        cell.left_center(),
                        Align2::LEFT_CENTER,
                        column.label(),
                        FontId::proportional(13.0),
                        ink,
                    );
                    // The sorted column's arrow, drawn (the interface font has no ▲ ▼).
                    if let Some(descending) = sorted {
                        let c = text.right_center() + vec2(9.0, 0.0);
                        let (tip, base) = if descending { (4.0, -3.0) } else { (-4.0, 3.0) };
                        ui.painter().add(Shape::convex_polygon(
                            vec![
                                c + vec2(-4.5, base),
                                c + vec2(4.5, base),
                                c + vec2(0.0, tip),
                            ],
                            ink,
                            Stroke::NONE,
                        ));
                    }
                    let label = match sorted {
                        Some(false) => format!("{}, sorted ascending", column.label()),
                        Some(true) => format!("{}, sorted descending", column.label()),
                        None => column.label().to_string(),
                    };
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if r.clicked() {
                        run.push((
                            "search.sort",
                            json!({ "tab": tab.0, "column": column.id() }),
                        ));
                    }
                    x += w;
                }
                egui::ScrollArea::vertical()
                    .id_salt(("search-analysis", tab))
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, order.len(), |ui, range| {
                        for k in range {
                            let row = &analysis.rows[order[k]];
                            let (rect, r) =
                                ui.allocate_exact_size(vec2(total, row_height), Sense::click());
                            let clip = rect.intersect(ui.clip_rect());
                            let painter = ui.painter().with_clip_rect(clip);
                            if r.hovered() {
                                painter.rect_filled(rect, 0.0, palette.hover);
                            }
                            let mut x = rect.left() + 12.0;
                            let cells: Vec<Rect> = widths
                                .iter()
                                .map(|w| {
                                    let c = Rect::from_min_size(
                                        Pos2::new(x, rect.top() + 3.0),
                                        vec2(w - 8.0, line),
                                    );
                                    x += w;
                                    c
                                })
                                .collect();
                            for (i, text) in [
                                (0, row.reference.as_str()),
                                (1, row.bible.as_str()),
                                (4, row.morphology.as_str()),
                            ] {
                                // On the pipeline text's baseline, near the bottom of its line.
                                painter.with_clip_rect(cells[i].intersect(clip)).text(
                                    cells[i].left_center() + vec2(0.0, 4.0),
                                    Align2::LEFT_CENTER,
                                    text,
                                    FontId::proportional(13.0),
                                    if i == 0 { theme::c().ink } else { muted },
                                );
                            }
                            self.table_cell_text(ui, cells[2], clip, &row.word, row.rtl, None);
                            let lemma_rtl = is_hebrew(&row.lemma);
                            self.table_cell_text(ui, cells[3], clip, &row.lemma, lemma_rtl, None);
                            let (excerpt, word) = row.excerpt(5, 8);
                            self.table_cell_text(ui, cells[5], clip, &excerpt, row.rtl, Some(word));
                            painter.hline(
                                rect.x_range(),
                                rect.bottom() - 0.5,
                                Stroke::new(1.0, palette.verse_number.gamma_multiply(0.15)),
                            );
                            let label = format!(
                                "{} {} {} {} {}",
                                row.reference, row.bible, row.word, row.lemma, row.morphology
                            );
                            r.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                            });
                            if r.clicked() {
                                let split = ui.input(|i| i.modifiers.command);
                                run.push((
                                    "search.open",
                                    json!({
                                        "tab": tab.0,
                                        "row": row.row,
                                        "hit": row.hit,
                                        "all": true,
                                        "split": split,
                                    }),
                                ));
                            }
                        }
                    });
            });
        if export {
            self.spawn_csv_picker(ui.ctx(), tab, &st.query);
        }
        for (id, params) in run {
            if id == "search.open" {
                self.session.workspace.user_focused(tab);
            }
            self.run(id, &params);
        }
    }

    /// Resource text in a table cell, through the text pipeline, clipped to the cell, with
    /// `highlight` marked as the Verse list marks hits. Draws a placeholder bar until laid.
    fn table_cell_text(
        &mut self,
        ui: &Ui,
        cell: Rect,
        clip: Rect,
        text: &str,
        rtl: bool,
        highlight: Option<Range<usize>>,
    ) {
        if text.is_empty() {
            return;
        }
        let settings = self.session.text;
        let palette = self.palette;
        let painter = ui.painter().with_clip_rect(cell.intersect(clip));
        // Laid on one line: wide enough for any cell's text; right-to-left text is drawn
        // against the cell's right edge.
        let width = 2000.0;
        let Some(laid) = self
            .snippets
            .get(text, ColorRole::Ink, rtl, width, &settings)
        else {
            painter.rect_filled(
                Rect::from_min_size(
                    cell.left_top() + vec2(0.0, cell.height() * 0.3),
                    vec2(cell.width().min(60.0), cell.height() * 0.35),
                ),
                2.0,
                palette.verse_number.gamma_multiply(0.12),
            );
            ui.ctx().request_repaint();
            return;
        };
        let origin = if rtl {
            Pos2::new(cell.right() - width, cell.top())
        } else {
            cell.left_top()
        };
        if let Some(h) = &highlight {
            for [x0, y0, x1, y1] in crate::compare_view::range_rects(&laid, h) {
                painter.rect_filled(
                    Rect::from_min_max(origin + vec2(x0, y0), origin + vec2(x1, y1)),
                    2.0,
                    palette.search_hit,
                );
            }
        }
        let quads =
            self.atlas
                .atlas
                .quads(&laid, [origin.x, origin.y], ui.ctx().pixels_per_point());
        let mut mesh = Mesh::with_texture(self.atlas.sync(ui.ctx()));
        for q in quads {
            mesh.add_rect_with_uv(
                Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
                Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
                palette.color(q.role),
            );
        }
        painter.add(Shape::mesh(mesh));
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_csv_picker(&mut self, _: &egui::Context, _: TabId, _: &str) {
        self.toast("Exporting a CSV file needs the desktop app");
    }

    /// Ask where to save the Analysis CSV on the picker's own thread; the answer is picked up
    /// by [`State::poll_csv_picker`].
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_csv_picker(&mut self, ctx: &egui::Context, tab: TabId, query: &str) {
        let name: String = query
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let name = format!(
            "search-{}.csv",
            if name.is_empty() { "analysis" } else { &name }
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-csv-picker".into())
            .spawn(move || {
                let chosen = rfd::FileDialog::new()
                    .set_title("Export the search Analysis as CSV")
                    .add_filter("CSV", &["csv"])
                    .set_file_name(name)
                    .save_file();
                let _ = tx.send(chosen);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.csv_picker = Some((tab, rx)),
            Err(e) => self.toast(format!("Could not open the save dialog: {e}")),
        }
    }

    /// Export the Analysis once the save dialog has answered; never wait for it.
    pub(super) fn poll_csv_picker(&mut self) {
        let Some((tab, rx)) = &self.csv_picker else {
            return;
        };
        let tab = *tab;
        match rx.try_recv() {
            Ok(path) => {
                self.csv_picker = None;
                if let Some(path) = path {
                    let out = self.run(
                        "search.export_csv",
                        &json!({ "tab": tab.0, "path": path.to_string_lossy() }),
                    );
                    if let Some(out) = out {
                        self.toast(format!(
                            "Exported {} rows to {}",
                            out["records"],
                            path.display()
                        ));
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.csv_picker = None;
                self.toast("The save dialog closed unexpectedly");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
}
