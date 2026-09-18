// SPDX-License-Identifier: GPL-3.0-or-later

//! The 2D overland-flow views: setup, interfaces, running, and depth /
//! velocity / hazard overlays on the map with the time slider. Filled by the
//! 2D app stream against the contract in `stormsewer_swmm::twod`.

use eframe::egui::{self, Ui};

use crate::state::AppState;

/// Per-editor 2D state (config draft, run progress, loaded results, textures).
#[derive(Default)]
pub struct TwoDState {}

/// The top-level `2D` menu.
pub fn menu(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Keep result textures in step with the selected frame.
pub fn sync(ctx: &egui::Context, ed: &mut crate::swmm_doc::SwmmEditor) {
    let _ = &ed.twod;
    let _ = (ctx, ed);
}

/// Draw the 2D overlay (called after the backdrop and GIS layers, before
/// the network).
pub fn draw_overlay(
    painter: &egui::Painter,
    ed: &crate::swmm_doc::SwmmEditor,
    w2s: &dyn Fn((f64, f64)) -> egui::Pos2,
) {
    let _ = (painter, ed, w2s);
}

/// The layers pane section for the 2D results and interfaces.
pub fn layers_section(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Windows and dialogs (setup, interfaces, run progress, legend).
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    let _ = (ctx, state);
}
