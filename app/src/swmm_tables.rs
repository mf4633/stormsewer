// SPDX-License-Identifier: GPL-3.0-or-later

//! Summary tables from the `.rpt` as sortable, filterable grids.
//!
//! Reached from the SWMM tab's "Tables" view button once a run has finished.
//! Every table the report parser found is offered; clicking a row selects
//! that object (the map/chart selection, `plot_target`/`plot_id`), and the
//! table can be written out as CSV.

use eframe::egui::{self, Pos2, Rect, Ui, Vec2};

use stormsewer_swmm::rpt::SummaryTable;

use crate::state::AppState;
use crate::swmm_export::save_csv;
use crate::swmm_panel::PlotTarget;
use crate::theme::palette;

const STRIP_H: f32 = 34.0;

#[derive(Default)]
pub struct SwmmTablesState {
    pub table: usize,
    pub filter: String,
    /// `(column, ascending)`.
    pub sort: Option<(usize, bool)>,
    pub message: String,
}

/// Row indices that pass `filter` (case-insensitive substring of any cell),
/// in `sort` order. Numeric cells sort as numbers, others as text, and
/// blanks last.
pub fn visible_rows(table: &SummaryTable, filter: &str, sort: Option<(usize, bool)>) -> Vec<usize> {
    let needle = filter.trim().to_ascii_lowercase();
    let mut rows: Vec<usize> = table
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            needle.is_empty() || r.iter().any(|c| c.to_ascii_lowercase().contains(&needle))
        })
        .map(|(i, _)| i)
        .collect();
    if let Some((col, ascending)) = sort {
        let key = |i: usize| -> (u8, f64, String) {
            let cell = table.rows[i].get(col).map(String::as_str).unwrap_or("");
            if cell.is_empty() {
                (2, 0.0, String::new())
            } else if let Ok(v) = cell.parse::<f64>() {
                (0, v, String::new())
            } else {
                (1, 0.0, cell.to_ascii_lowercase())
            }
        };
        rows.sort_by(|&a, &b| {
            let (ka, kb) = (key(a), key(b));
            let ord =
                ka.0.cmp(&kb.0)
                    .then(ka.1.total_cmp(&kb.1))
                    .then(ka.2.cmp(&kb.2));
            // Blanks stay last whichever way the column is sorted.
            if ka.0 == 2 || kb.0 == 2 || ascending {
                ord
            } else {
                ord.reverse()
            }
        });
    }
    rows
}

/// What clicking a row of table `title` selects, if the table is about
/// nodes or links.
pub fn selection_for(title: &str, name: &str) -> Option<(PlotTarget, String)> {
    let t = title.to_ascii_lowercase();
    if name.is_empty() || name.eq_ignore_ascii_case("system") {
        return None;
    }
    if t.starts_with("node") || t.starts_with("outfall") || t.starts_with("storage") {
        Some((PlotTarget::Node, name.to_string()))
    } else if t.starts_with("link")
        || t.starts_with("conduit")
        || t.starts_with("pump")
        || t.starts_with("flow classification")
    {
        Some((PlotTarget::Link, name.to_string()))
    } else {
        None
    }
}

/// Draw the tables view.
pub fn draw_swmm_tables(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));

    let empty_state = |line: &str| {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            line,
            egui::FontId::proportional(15.0),
            palette::canvas::muted(dark),
        );
    };

    let Some(run) = state.swmm.last_run.as_ref() else {
        empty_state("Run a SWMM model to read its summary tables");
        return;
    };
    let tables = &run.report.tables;
    if tables.is_empty() {
        empty_state("The report has no summary tables");
        return;
    }
    let ts = &mut state.swmm.tables;
    ts.table = ts.table.min(tables.len() - 1);

    let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), STRIP_H));
    let body = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + STRIP_H), rect.max);

    let mut select: Option<(PlotTarget, String)> = None;
    ui.allocate_new_ui(
        egui::UiBuilder::new().max_rect(strip.shrink2(Vec2::new(8.0, 4.0))),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut pick = ts.table;
                egui::ComboBox::from_id_salt("swmm-tables-pick")
                    .selected_text(tables[pick].title.clone())
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for (i, t) in tables.iter().enumerate() {
                            ui.selectable_value(&mut pick, i, &t.title);
                        }
                    });
                if pick != ts.table {
                    ts.table = pick;
                    ts.sort = None;
                }
                ui.label("Filter");
                ui.add(egui::TextEdit::singleline(&mut ts.filter).desired_width(140.0));
                if !ts.filter.is_empty() && ui.small_button("✕").clicked() {
                    ts.filter.clear();
                }
                let table = &tables[ts.table];
                let shown = visible_rows(table, &ts.filter, ts.sort).len();
                ui.label(format!("{shown} of {} rows", table.rows.len()));
                if ui.small_button("Export CSV").clicked() {
                    let name =
                        format!("{}.csv", table.title.to_ascii_lowercase().replace(' ', "-"));
                    if let Some(msg) = save_csv(&name, &table.to_csv()) {
                        ts.message = msg;
                    }
                }
                if !ts.message.is_empty() {
                    ui.label(ts.message.clone());
                }
            });
        },
    );

    let table = &tables[ts.table];
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body.shrink(8.0)), |ui| {
        if let Some(note) = &table.note {
            ui.label(egui::RichText::new(note).italics());
            return;
        }
        if table.columns.is_empty() {
            ui.label("This table has no columns.");
            return;
        }
        let rows = visible_rows(table, &ts.filter, ts.sort);
        egui::ScrollArea::both()
            .id_salt("swmm-tables-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("swmm-tables-grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        for (ci, col) in table.columns.iter().enumerate() {
                            let marker = match ts.sort {
                                Some((c, true)) if c == ci => " ▲",
                                Some((c, false)) if c == ci => " ▼",
                                _ => "",
                            };
                            let text = egui::RichText::new(format!("{col}{marker}")).strong();
                            if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                ts.sort = match ts.sort {
                                    Some((c, true)) if c == ci => Some((ci, false)),
                                    Some((c, false)) if c == ci => None,
                                    _ => Some((ci, true)),
                                };
                            }
                        }
                        ui.end_row();
                        for ri in rows {
                            let row = &table.rows[ri];
                            let name = row.first().map(String::as_str).unwrap_or("");
                            let selected = state.swmm.plot_id.as_deref() == Some(name)
                                && selection_for(&table.title, name)
                                    .is_some_and(|(t, _)| t == state.swmm.plot_target);
                            for (ci, cell) in row.iter().enumerate() {
                                if ci == 0 {
                                    if ui.selectable_label(selected, cell).clicked() {
                                        select = selection_for(&table.title, name);
                                    }
                                } else {
                                    ui.monospace(cell);
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });

    if let Some((target, id)) = select {
        if state.swmm.plot_target != target {
            state.swmm.plot_var = 0;
        }
        state.swmm.plot_target = target;
        state.swmm.plot_id = Some(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_panel::SwmmSubView;
    use crate::swmm_profile::tests::{pond_state, run_frame};
    use crate::StormSewerApp;

    #[test]
    fn rows_filter_and_sort() {
        let t = SummaryTable {
            title: "Node Depth Summary".into(),
            header_lines: vec![],
            columns: vec!["Node".into(), "Type".into(), "Max".into()],
            rows: vec![
                vec!["J1".into(), "JUNCTION".into(), "0.49".into()],
                vec!["J10".into(), "JUNCTION".into(), "1.21".into()],
                vec!["O2".into(), "OUTFALL".into(), "".into()],
                vec!["SU1".into(), "STORAGE".into(), "2.15".into()],
            ],
            note: None,
        };
        assert_eq!(visible_rows(&t, "", None), vec![0, 1, 2, 3]);
        assert_eq!(visible_rows(&t, "j1", None), vec![0, 1]);
        assert_eq!(visible_rows(&t, "outfall", None), vec![2]);
        // Numeric sort, blanks last both ways.
        assert_eq!(visible_rows(&t, "", Some((2, true))), vec![0, 1, 3, 2]);
        assert_eq!(visible_rows(&t, "", Some((2, false))), vec![3, 1, 0, 2]);
        // Text sort.
        assert_eq!(visible_rows(&t, "", Some((1, false))), vec![3, 2, 0, 1]);

        assert_eq!(
            selection_for("Node Depth Summary", "J1"),
            Some((PlotTarget::Node, "J1".into()))
        );
        assert_eq!(
            selection_for("Link Flow Summary", "C1"),
            Some((PlotTarget::Link, "C1".into()))
        );
        assert_eq!(selection_for("Outfall Loading Summary", "System"), None);
        assert_eq!(selection_for("Subcatchment Runoff Summary", "S1"), None);
    }

    #[test]
    fn tables_view_renders_and_selection_round_trips() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        app.state.view_tab = crate::state::ViewTab::Swmm;
        app.state.swmm.sub_view = SwmmSubView::Tables;
        run_frame(&mut app);

        let mut app = StormSewerApp::new_for_test(pond_state());
        app.state.swmm.sub_view = SwmmSubView::Tables;
        let n = app
            .state
            .swmm
            .last_run
            .as_ref()
            .unwrap()
            .report
            .tables
            .len();
        assert!(n >= 9, "{n} tables");
        for i in 0..n {
            app.state.swmm.tables.table = i;
            run_frame(&mut app);
        }
        // Sorting and filtering render too.
        app.state.swmm.tables.table = app
            .state
            .swmm
            .last_run
            .as_ref()
            .unwrap()
            .report
            .tables
            .iter()
            .position(|t| t.title == "Link Flow Summary")
            .unwrap();
        app.state.swmm.tables.sort = Some((2, false));
        app.state.swmm.tables.filter = "c1".into();
        run_frame(&mut app);

        // Selecting from a table is the map/chart selection.
        let (t, id) = selection_for("Link Flow Summary", "C11").unwrap();
        app.state.swmm.plot_target = t;
        app.state.swmm.plot_id = Some(id);
        app.state.swmm.plot_var = 3;
        run_frame(&mut app);
        assert_eq!(app.state.swmm.plot_id.as_deref(), Some("C11"));
        assert_eq!(app.state.swmm.plot_target, PlotTarget::Link);
        // The chart view plots that selection.
        app.state.swmm.sub_view = SwmmSubView::Chart;
        run_frame(&mut app);
        let s = app.state.swmm.series().expect("series for the selection");
        assert_eq!(s.values.len(), 144);
    }
}
