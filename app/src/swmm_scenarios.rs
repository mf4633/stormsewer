// SPDX-License-Identifier: GPL-3.0-or-later

//! Scenario manager: named sets of edits on a base model, run singly or as
//! a batch and compared. Filled by the scenarios-and-calibration stream.

use eframe::egui::{self, Ui};

use crate::state::AppState;

/// Per-editor scenario state.
#[derive(Default)]
pub struct ScenarioState {}

/// `Tools` menu entries.
pub fn tools_menu_items(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    let _ = &state.swmm_doc.scenarios;
    let _ = (ctx, state);
}
