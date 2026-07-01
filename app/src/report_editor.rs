// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom report template editor (Hydraflow MyReport column picker).

use eframe::egui::{self, RichText};
use stormsewer::io::{ReportTemplate, ReportVariable};

use crate::state::AppState;

pub fn draw_report_editor_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_report_editor {
        return;
    }

    let mut open = state.show_report_editor;
    egui::Window::new("Custom Report Editor (MyReport)")
        .open(&mut open)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.label("Template name");
            ui.text_edit_singleline(&mut state.report_template.name);
            ui.add_space(6.0);
            ui.label(RichText::new("Columns (top to bottom = left to right in export)").strong());
            ui.separator();

            let mut remove_idx: Option<usize> = None;
            let mut move_up: Option<usize> = None;
            let mut move_down: Option<usize> = None;

            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for (i, col) in state.report_template.columns.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}.", i + 1));
                            ui.label(col.header());
                            if ui.small_button("▲").clicked() && i > 0 {
                                move_up = Some(i);
                            }
                            if ui.small_button("▼").clicked()
                                && i + 1 < state.report_template.columns.len()
                            {
                                move_down = Some(i);
                            }
                            if ui.small_button("✕").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    }
                });

            if let Some(i) = remove_idx {
                state.report_template.columns.remove(i);
            }
            if let Some(i) = move_up {
                state.report_template.columns.swap(i, i - 1);
            }
            if let Some(i) = move_down {
                state.report_template.columns.swap(i, i + 1);
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Add column:");
                egui::ComboBox::from_id_salt("report_add_col")
                    .selected_text("Choose…")
                    .show_ui(ui, |ui| {
                        for var in ReportVariable::ALL {
                            let already = state.report_template.columns.contains(&var);
                            if ui
                                .add_enabled(!already, egui::Button::new(var.header()))
                                .clicked()
                            {
                                state.report_template.columns.push(var);
                            }
                        }
                    });
            });

            ui.add_space(8.0);
            ui.label(RichText::new("Presets").strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Municipal").clicked() {
                    state.report_template = ReportTemplate::municipal_summary();
                }
                if ui.button("Hydraflow").clicked() {
                    state.report_template = ReportTemplate::hydraflow_style();
                }
                if ui.button("Cost").clicked() {
                    state.report_template = ReportTemplate::cost_report();
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Load .srpt…").clicked() {
                    state.pick_load_report_template();
                }
                if ui.button("Save .srpt…").clicked() {
                    state.pick_save_report_template();
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Export CSV…").clicked() {
                    state.pick_export_custom_csv();
                }
                if ui.button("Export HTML…").clicked() {
                    state.pick_export_custom_html();
                }
            });
        });
    state.show_report_editor = open;
}