// SPDX-License-Identifier: GPL-3.0-or-later

//! Global editing dialog (Hydraflow Edit → Global Editing).

use eframe::egui;

use crate::state::AppState;

/// Draw global edit dialog for all-pipe property changes.
pub fn draw_global_edit_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_global_edit {
        return;
    }

    let mut close = false;
    let mut apply_n = false;
    let mut apply_dia = false;

    egui::Window::new("Global Pipe Editing")
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .open(&mut state.show_global_edit)
        .show(ctx, |ui| {
            ui.label("Apply a value to all pipes in the network.");
            ui.separator();

            ui.heading("Manning Roughness");
            ui.horizontal(|ui| {
                ui.label("n:");
                ui.add(
                    egui::DragValue::new(&mut state.global_edit_n)
                        .speed(0.001)
                        .range(0.009..=0.05),
                );
                if ui.button("Apply to All Pipes").clicked() {
                    apply_n = true;
                }
            });

            ui.add_space(8.0);
            ui.heading("Circular Pipe Diameter");
            ui.horizontal(|ui| {
                ui.label("Diameter (in):");
                ui.add(
                    egui::DragValue::new(&mut state.global_edit_dia_in)
                        .speed(1.0)
                        .range(6.0..=120.0),
                );
                if ui.button("Apply to Circular Pipes").clicked() {
                    apply_dia = true;
                }
            });

            ui.add_space(8.0);
            ui.separator();
            if ui.button("Close").clicked() {
                close = true;
            }
        });

    if apply_n {
        let n = state.global_edit_n;
        state.global_set_pipe_n(n);
    }
    if apply_dia {
        let dia = state.global_edit_dia_in;
        state.global_set_pipe_diameter_in(dia);
    }
    if close {
        state.show_global_edit = false;
    }
}