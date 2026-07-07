// SPDX-License-Identifier: GPL-3.0-or-later

//! Quick-access toolbar below the menu bar: tool palette + primary actions.

use eframe::egui::{self, Button, RichText, Ui};

use crate::edit::Tool;
use crate::panels::SideTab;
use crate::state::{AppState, ViewTab};
use crate::theme::palette;

/// Draw the tool palette and primary workflow actions for fast access.
pub fn draw_toolbar(ui: &mut Ui, state: &mut AppState, canvas_rect: egui::Rect) {
    ui.horizontal_centered(|ui| {
        // ── Tool palette — always visible, active tool highlighted. ───────
        for tool in Tool::all() {
            let active = state.tool == tool;
            let resp = ui
                .selectable_label(active, tool.short())
                .on_hover_text(format!("{} — press {}", tool.label(), tool.shortcut()));
            if resp.clicked() {
                state.set_tool(tool);
            }
        }

        ui.separator();

        // ── Primary action: Analyze stands out as accent-filled. ──────────
        let analyze = Button::new(RichText::new("Analyze").color(egui::Color32::WHITE))
            .fill(palette::ACCENT);
        if ui.add(analyze).on_hover_text("Run hydraulic analysis (F5)").clicked() {
            state.run_analysis();
        }
        if ui.button("Auto-Size").on_hover_text("Size pipes to design criteria").clicked() {
            state.apply_sizing();
        }
        if ui.button("Tc Calc").on_hover_text("Time-of-concentration calculator").clicked() {
            state.open_tc_calculator();
        }

        ui.separator();

        if ui.button("Extents").on_hover_text("Zoom to fit (F)").clicked() {
            state.viewport.zoom_to_fit(canvas_rect, &state.project);
        }
        if ui.button("Selection").on_hover_text("Zoom to selection (G)").clicked() {
            state.viewport.zoom_to_selection(
                canvas_rect,
                &state.project,
                state.selected_node,
                state.selected_pipe,
            );
        }

        ui.separator();

        let (errors, warnings) = state.review_counts();
        let review = if errors + warnings > 0 {
            RichText::new(format!("Review  {errors}E / {warnings}W"))
                .color(if errors > 0 { palette::ERROR } else { palette::WARNING })
        } else {
            RichText::new("Review")
        };
        if ui.add(Button::new(review)).clicked() {
            state.side_tab = SideTab::Review;
        }

        ui.separator();

        let mut snap_on = state.prefs.snap_grid_ft > 0.0;
        if ui.checkbox(&mut snap_on, "Snap").on_hover_text("Snap placement to grid").changed() {
            state.prefs.snap_grid_ft = if snap_on { 10.0 } else { 0.0 };
            state.prefs.save();
        }
        if snap_on {
            let mut grid = state.prefs.snap_grid_ft;
            if ui
                .add(egui::DragValue::new(&mut grid).speed(1.0).range(1.0..=100.0).suffix(" ft"))
                .changed()
            {
                state.prefs.snap_grid_ft = grid;
                state.prefs.save();
            }
        }

        // ── Right-aligned: status chips + view switch. ────────────────────
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.selectable_value(&mut state.view_tab, ViewTab::Profile, "Profile");
            ui.selectable_value(&mut state.view_tab, ViewTab::Plan, "Plan");
            ui.separator();
            if state.project_dirty {
                chip(ui, "● Unsaved", palette::UNSAVED);
            }
            if state.analysis_stale {
                chip(ui, "● Stale", palette::STALE);
            }
        });
    });
}

/// A small colored status pill.
fn chip(ui: &mut Ui, text: &str, color: egui::Color32) {
    ui.label(RichText::new(text).color(color).small());
}
