// SPDX-License-Identifier: GPL-3.0-or-later

//! GIS in the editor: import of shapefiles and GeoJSON with field mapping,
//! coordinate systems from `.prj`, DEM loading, ground elevations from the
//! DEM, and vector layers drawn under the network. Filled by the GIS stream;
//! the hooks below are already called from the menus, layers pane and map.

use eframe::egui::{self, Ui};

use crate::state::AppState;

/// Per-editor GIS state (loaded layers, dialogs, the DEM).
#[derive(Default)]
pub struct GisState {}

/// `File` menu: import and export entries.
pub fn file_menu_items(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Keep textures and caches in step with the loaded layers.
pub fn sync(ctx: &egui::Context, ed: &mut crate::swmm_doc::SwmmEditor) {
    let _ = &ed.gis;
    let _ = (ctx, ed);
}

/// Draw the GIS layers under the network (called after the backdrop).
pub fn draw(
    painter: &egui::Painter,
    rect: egui::Rect,
    vp: &crate::viewport::Viewport,
    ed: &crate::swmm_doc::SwmmEditor,
) {
    let _ = (painter, rect, vp, ed);
}

/// The layers pane section for GIS layers and the DEM.
pub fn layers_section(ui: &mut Ui, state: &mut AppState) {
    let _ = (ui, state);
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    let _ = (ctx, state);
}
