// SPDX-License-Identifier: GPL-3.0-or-later

//! Quick-access toolbar below the menu bar: tool palette + primary actions.

use eframe::egui::{self, Button, RichText, Ui};

use crate::edit::Tool;
use crate::panels::SideTab;
use crate::state::{AppState, ViewTab};
use crate::theme::palette;

/// Width the *fixed* part of the right-hand group needs: the view switch
/// (SWMM / Plan / Profile), its separator, and the theme button. Measured off
/// a 1400px window, where that group spans about 282px.
///
/// They are drawn right-to-left in the same row, so this is the space the
/// left-hand controls must leave alone if the two are not to overlap.
const RIGHT_GROUP_W: f32 = 290.0;

/// Extra width to reserve per status chip, which appear only when the project
/// is dirty or the analysis is stale. Reserving for them unconditionally hides
/// the snap controls on a clean project for no reason; not reserving at all
/// puts the overlap back the moment a project is edited.
const CHIP_W: f32 = 78.0;

/// Draw the tool palette and primary workflow actions for fast access.
pub fn draw_toolbar(ui: &mut Ui, state: &mut AppState, canvas_rect: egui::Rect) {
    let dark = ui.visuals().dark_mode;
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
        let analyze =
            Button::new(RichText::new("Analyze").color(egui::Color32::WHITE)).fill(palette::ACCENT);
        if ui
            .add(analyze)
            .on_hover_text("Run hydraulic analysis (F5)")
            .clicked()
        {
            state.run_analysis();
        }
        if ui
            .button("Auto-Size")
            .on_hover_text("Size pipes to design criteria")
            .clicked()
        {
            state.apply_sizing();
        }
        if ui
            .button("Tc Calc")
            .on_hover_text("Time-of-concentration calculator")
            .clicked()
        {
            state.open_tc_calculator();
        }

        ui.separator();

        if ui
            .button("Extents")
            .on_hover_text("Zoom to fit (F)")
            .clicked()
        {
            state.viewport.zoom_to_fit(canvas_rect, &state.project);
        }
        if ui
            .button("Selection")
            .on_hover_text("Zoom to selection (G)")
            .clicked()
        {
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
            RichText::new(format!("Review  {errors}E / {warnings}W")).color(if errors > 0 {
                palette::error_text(dark)
            } else {
                palette::warning_text(dark)
            })
        } else {
            RichText::new("Review")
        };
        if ui.add(Button::new(review)).clicked() {
            state.side_tab = SideTab::Review;
        }

        ui.separator();

        // The view switch and status chips are right-aligned into this same
        // row, and egui does not wrap a `horizontal_centered`: once the
        // left-hand controls run past the width the right-hand group needs,
        // the two groups simply draw on top of one another. At the default
        // 1400x860 window that overprinted "Skeleton" with "Plan", rendering
        // both unreadable. So the optional controls yield when space is tight,
        // worst-priority first, rather than colliding with the view switch.
        //
        // The reserve tracks the chips, which come and go: a fixed worst-case
        // reserve hid the snap controls at the default window size even on a
        // clean project, which is a poor trade for avoiding a collision that
        // was not going to happen.
        let reserve = RIGHT_GROUP_W
            + if state.project_dirty { CHIP_W } else { 0.0 }
            + if state.analysis_stale { CHIP_W } else { 0.0 };

        if ui.available_width() > reserve + 70.0 {
            let mut snap_on = state.prefs.snap_grid_ft > 0.0;
            if ui
                .checkbox(&mut snap_on, "Snap")
                .on_hover_text("Snap placement to grid")
                .changed()
            {
                state.prefs.snap_grid_ft = if snap_on { 10.0 } else { 0.0 };
                state.prefs.save();
            }
            if snap_on && ui.available_width() > reserve + 60.0 {
                let mut grid = state.prefs.snap_grid_ft;
                if ui
                    .add(
                        egui::DragValue::new(&mut grid)
                            .speed(1.0)
                            .range(1.0..=100.0)
                            .suffix(" ft"),
                    )
                    .changed()
                {
                    state.prefs.snap_grid_ft = grid;
                    state.prefs.save();
                }
            }
        }

        if ui.available_width() > reserve + 80.0 {
            let mut skeleton = state.prefs.draw_zero_area;
            if ui
                .checkbox(&mut skeleton, "Skeleton")
                .on_hover_text(
                    "Drawn manholes start with 0 drainage area — sketch the layout, assign loads later",
                )
                .changed()
            {
                state.prefs.draw_zero_area = skeleton;
                state.prefs.save();
            }
        }

        // ── Right-aligned: status chips + view switch. ────────────────────
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let dark = ui.visuals().dark_mode;
            let label = if dark { "Light mode" } else { "Dark mode" };
            if ui
                .button(label)
                .on_hover_text("Switch the theme (View menu has Follow system)")
                .clicked()
            {
                state.prefs.theme = if dark {
                    crate::theme::Theme::Light
                } else {
                    crate::theme::Theme::Dark
                };
                state.prefs.save();
            }
            ui.separator();

            ui.selectable_value(&mut state.view_tab, ViewTab::Profile, "Profile");
            ui.selectable_value(&mut state.view_tab, ViewTab::Plan, "Plan");
            ui.selectable_value(&mut state.view_tab, ViewTab::Swmm, "SWMM");
            ui.separator();
            if state.project_dirty {
                chip(ui, "● Unsaved", palette::accent_text(dark));
            }
            if state.analysis_stale {
                chip(ui, "● Stale", palette::stale_text(dark));
            }
        });
    });
}

/// A small colored status pill.
fn chip(ui: &mut Ui, text: &str, color: egui::Color32) {
    ui.label(RichText::new(text).color(color).small());
}
