// SPDX-License-Identifier: GPL-3.0-or-later

//! Live results while the engine runs, and a Stop that works. Filled by the
//! engine-control stream.

use eframe::egui::{self, Ui};

use crate::state::AppState;

/// Per-editor live-run state.
#[derive(Default)]
pub struct LiveState {}

/// `Run` menu entries (live results toggle and the like).
pub fn run_menu_items(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    let _ = &state.swmm_doc.live;
    let _ = (ctx, state);
}
