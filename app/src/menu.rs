// SPDX-License-Identifier: GPL-3.0-or-later

//! Right-click context menu for plan-view structures and pipes.

use eframe::egui::Ui;

use crate::edit::{self, snap_placement, ContextTarget};
use crate::state::AppState;

/// Populate the plan-view context menu based on what was right-clicked.
pub fn draw_context_menu(ui: &mut Ui, state: &mut AppState) {
    match state.edit.context_target {
        Some(ContextTarget::Node(idx)) => node_menu(ui, state, idx),
        Some(ContextTarget::Pipe { idx, x, y }) => pipe_menu(ui, state, idx, x, y),
        Some(ContextTarget::Empty { x, y }) => empty_menu(ui, state, x, y),
        None => {
            ui.label("Right-click a structure or pipe");
        }
    }
}

fn empty_menu(ui: &mut Ui, state: &mut AppState, x: f64, y: f64) {
    ui.label("Place here");
    ui.separator();
    for (label, kind) in [("Inlet", "inlet"), ("Junction", "junction"), ("Outfall", "outfall")] {
        if ui.button(label).clicked() {
            state.checkpoint_undo();
            let (sx, sy) = snap_placement(x, y, state.prefs.snap_grid_ft);
            let id = edit::place_structure(&mut state.project, &mut state.edit, kind, sx, sy);
            let sel = state.project.nodes.len().checked_sub(1);
            state.set_selection(sel, None, None);
            state.status = format!("Placed {kind} {id}");
            state.run_analysis();
            state.update_inlet_check();
            ui.close_menu();
        }
    }
}

fn node_menu(ui: &mut Ui, state: &mut AppState, idx: usize) {
    let Some(name) = state.project.nodes.get(idx).map(|n| n.id.clone()) else {
        return;
    };
    ui.label(format!("Structure {name}"));
    ui.separator();

    ui.menu_button("Set type", |ui| {
        for (label, kind) in [("Inlet", "inlet"), ("Junction", "junction"), ("Outfall", "outfall")]
        {
            if ui.button(label).clicked() {
                state.checkpoint_undo();
                if let Some(n) = state.project.nodes.get_mut(idx) {
                    n.kind = kind.into();
                }
                state.status = format!("{name} set to {kind}");
                state.run_analysis();
                state.update_inlet_check();
                ui.close_menu();
            }
        }
    });

    if ui.button("Duplicate").clicked() {
        state.checkpoint_undo();
        if let Some(id) = edit::duplicate_node(&mut state.project, &mut state.edit, idx) {
            let sel = state.project.nodes.iter().position(|n| n.id == id);
            state.set_selection(sel, None, None);
            state.status = format!("Duplicated {name} as {id}");
        }
        state.run_analysis();
        state.update_inlet_check();
        ui.close_menu();
    }

    if ui.button("Delete structure").clicked() {
        state.checkpoint_undo();
        if let Some(msg) = edit::delete_selection(&mut state.project, Some(idx), None) {
            state.status = msg;
        }
        state.clear_selection();
        state.run_analysis();
        state.update_inlet_check();
        ui.close_menu();
    }
}

fn pipe_menu(ui: &mut Ui, state: &mut AppState, idx: usize, x: f64, y: f64) {
    let Some(name) = state.project.pipes.get(idx).map(|p| p.id.clone()) else {
        return;
    };
    ui.label(format!("Pipe {name}"));
    ui.separator();

    if ui.button("Reverse direction").clicked() {
        state.checkpoint_undo();
        if let Some(msg) = edit::reverse_pipe(&mut state.project, idx) {
            state.status = msg;
        }
        state.run_analysis();
        ui.close_menu();
    }

    if ui.button("Insert junction here").clicked() {
        state.checkpoint_undo();
        if let Some((id, from_id, to_id)) =
            edit::split_pipe(&mut state.project, &mut state.edit, idx, "junction", x, y)
        {
            state.status = format!("Inserted {id} on the run: {from_id} → {id} → {to_id}");
        }
        state.run_analysis();
        state.update_inlet_check();
        ui.close_menu();
    }

    if ui.button("Delete pipe").clicked() {
        state.checkpoint_undo();
        if let Some(msg) = edit::delete_selection(&mut state.project, None, Some(idx)) {
            state.status = msg;
        }
        state.clear_selection();
        state.run_analysis();
        ui.close_menu();
    }
}
