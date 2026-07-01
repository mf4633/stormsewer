// SPDX-License-Identifier: GPL-3.0-or-later

//! Quick-access toolbar below the menu bar.

use eframe::egui::{self, Ui};

use crate::panels::SideTab;
use crate::state::{AppState, ViewTab};

/// Draw primary workflow actions for fast access during design.
pub fn draw_toolbar(ui: &mut Ui, state: &mut AppState, canvas_rect: egui::Rect) {
    ui.horizontal(|ui| {
        if ui.button("Analyze (F5)").clicked() {
            state.run_analysis();
        }
        if ui.button("Auto-Size").clicked() {
            state.apply_sizing();
        }
        if ui.button("Tc Calculator").clicked() {
            state.open_tc_calculator();
        }

        ui.separator();

        if ui.button("Zoom Extents (F)").clicked() {
            state
                .viewport
                .zoom_to_fit(canvas_rect, &state.project);
        }
        if ui.button("Zoom Selection (G)").clicked() {
            state.viewport.zoom_to_selection(
                canvas_rect,
                &state.project,
                state.selected_node,
                state.selected_pipe,
            );
        }

        ui.separator();

        let (errors, warnings) = state.review_counts();
        let review_label = if errors + warnings > 0 {
            format!("Review ({errors}E/{warnings}W)")
        } else {
            "Review".into()
        };
        if ui.button(&review_label).clicked() {
            state.side_tab = SideTab::Review;
        }

        ui.separator();

        let mut snap_on = state.prefs.snap_grid_ft > 0.0;
        if ui.checkbox(&mut snap_on, "Snap grid").changed() {
            state.prefs.snap_grid_ft = if snap_on { 10.0 } else { 0.0 };
            state.prefs.save();
        }
        if snap_on {
            let mut grid = state.prefs.snap_grid_ft;
            if ui
                .add(egui::DragValue::new(&mut grid).speed(1.0).range(1.0..=100.0))
                .changed()
            {
                state.prefs.snap_grid_ft = grid;
                state.prefs.save();
            }
            ui.label("ft");
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.analysis_stale {
                ui.colored_label(egui::Color32::YELLOW, "Stale");
            }
            if state.project_dirty {
                ui.colored_label(egui::Color32::LIGHT_BLUE, "Unsaved");
            }
            ui.selectable_value(&mut state.view_tab, ViewTab::Plan, "Plan");
            ui.selectable_value(&mut state.view_tab, ViewTab::Profile, "Profile");
        });
    });
}