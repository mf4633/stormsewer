// SPDX-License-Identifier: GPL-3.0-or-later

//! Dialogs for the model sections that had only attribute tables: LID
//! controls and usage, aquifers and groundwater, snow packs, pollutant
//! buildup / washoff / coverages / loadings, treatment, RDII unit
//! hydrographs. Filled by the dialogs stream.

use eframe::egui::{self, Ui};

use crate::state::AppState;

/// Per-editor state for these dialogs.
#[derive(Default)]
pub struct LidState {}

/// `Project` menu entries for these dialogs.
pub fn project_menu_items(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    let _ = &state.swmm_doc.lid;
    let _ = (ctx, state);
}
