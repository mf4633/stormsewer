// SPDX-License-Identifier: GPL-3.0-or-later

//! Calibration: observed series, objective functions, sensitivity and the
//! optimiser, with progress and an apply-as-one-undo-step. Filled by the
//! scenarios-and-calibration stream.

use eframe::egui::{self, Ui};

use crate::state::AppState;

/// Per-editor calibration state.
#[derive(Default)]
pub struct CalibState {}

/// `Tools` menu entries.
pub fn tools_menu_items(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    let _ = &state.swmm_doc.calib;
    let _ = (ctx, state);
}
