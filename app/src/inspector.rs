// SPDX-License-Identifier: GPL-3.0-or-later

//! Bottom-panel property inspector for selected nodes and pipes.

use eframe::egui::{self, RichText, Ui};

use crate::edit::{delete_selection, sync_pipe_lengths};
use crate::state::AppState;
use stormsewer::catchment::{shoelace_area_sqft, sqft_to_acres};

const NODE_KINDS: [&str; 3] = ["inlet", "junction", "outfall"];

/// Draw the collapsible bottom inspector for the current selection.
pub fn draw_inspector(ui: &mut Ui, state: &mut AppState) {
    state.edit.selected_node = state.selected_node;
    state.edit.selected_pipe = state.selected_pipe;

    if !state.has_selection() {
        ui.label(
            RichText::new("No selection")
                .strong()
                .size(14.0),
        );
        ui.label("Click a structure, pipe, or catchment on the plan view, or select a row in the Tables tab.");
        ui.label("Keyboard: 1–6 switch tools · F zoom extents · G zoom to selection · Esc cancel drawing");
        return;
    }

    let edit_snapshot = state.project.clone();
    let mut changed = false;
    let mut do_delete = false;
    let mut delete_catchment: Option<(usize, String)> = None;
    let mut sync_lengths = false;
    let mut open_tc = false;

    if let Some(idx) = state.selected_node {
        if idx < state.project.nodes.len() {
            let node = &mut state.project.nodes[idx];
            ui.heading(format!("Structure: {}", node.id));
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Kind:");
                egui::ComboBox::from_id_salt("inspector_node_kind")
                    .selected_text(&node.kind)
                    .show_ui(ui, |ui| {
                        for kind in NODE_KINDS {
                            if ui
                                .selectable_value(&mut node.kind, kind.to_string(), kind)
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("X:");
                if ui.add(egui::DragValue::new(&mut node.x).speed(1.0)).changed() {
                    sync_lengths = true;
                    changed = true;
                }
                ui.label("Y:");
                if ui.add(egui::DragValue::new(&mut node.y).speed(1.0)).changed() {
                    sync_lengths = true;
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Invert (ft):");
                if ui.add(egui::DragValue::new(&mut node.invert).speed(0.1)).changed() {
                    changed = true;
                }
                ui.label("Rim (ft):");
                if ui.add(egui::DragValue::new(&mut node.rim).speed(0.1)).changed() {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Area (ac):");
                if ui
                    .add(
                        egui::DragValue::new(&mut node.area_ac)
                            .speed(0.1)
                            .range(0.0..=1000.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.label("C:");
                if ui
                    .add(egui::DragValue::new(&mut node.c).speed(0.01).range(0.0..=1.0))
                    .changed()
                {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Tc (min):");
                if ui
                    .add(
                        egui::DragValue::new(&mut node.tc_inlet)
                            .speed(0.5)
                            .range(1.0..=120.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui.button("Tc Calculator…").clicked() {
                    open_tc = true;
                }
            });
            ui.separator();
            if ui.button("Delete").clicked() {
                do_delete = true;
            }
        }
    } else if let Some(idx) = state.selected_pipe {
        if idx < state.project.pipes.len() {
            let pipe = &mut state.project.pipes[idx];
            ui.heading(format!("Pipe: {}", pipe.id));
            ui.separator();
            ui.label(format!("From: {}  ->  To: {}", pipe.from, pipe.to));
            ui.horizontal(|ui| {
                ui.label("Length (ft):");
                if ui
                    .add(
                        egui::DragValue::new(&mut pipe.length)
                            .speed(1.0)
                            .range(1.0..=10000.0),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Shape:");
                egui::ComboBox::from_id_salt("inspector_pipe_shape")
                    .selected_text(&pipe.shape)
                    .show_ui(ui, |ui| {
                        for shape in ["circular", "box", "elliptical"] {
                            if ui
                                .selectable_value(&mut pipe.shape, shape.to_string(), shape)
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
            });
            if pipe.shape == "circular" {
                ui.horizontal(|ui| {
                    ui.label("Diameter (in):");
                    let mut dia_in = pipe.diameter * 12.0;
                    if ui
                        .add(egui::DragValue::new(&mut dia_in).speed(1.0).range(6.0..=120.0))
                        .changed()
                    {
                        pipe.diameter = dia_in / 12.0;
                        changed = true;
                    }
                    ui.label("n:");
                    if ui
                        .add(egui::DragValue::new(&mut pipe.n).speed(0.001).range(0.009..=0.05))
                        .changed()
                    {
                        changed = true;
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("Rise (ft):");
                    if ui
                        .add(egui::DragValue::new(&mut pipe.rise_ft).speed(0.1).range(0.5..=20.0))
                        .changed()
                    {
                        changed = true;
                    }
                    ui.label("Span (ft):");
                    if ui
                        .add(egui::DragValue::new(&mut pipe.span_ft).speed(0.1).range(0.5..=30.0))
                        .changed()
                    {
                        changed = true;
                    }
                    ui.label("n:");
                    if ui
                        .add(egui::DragValue::new(&mut pipe.n).speed(0.001).range(0.009..=0.05))
                        .changed()
                    {
                        changed = true;
                    }
                });
                ui.label("Hydraulics use equivalent circular diameter.");
            }
            ui.separator();
            if ui.button("Delete").clicked() {
                do_delete = true;
            }
        }
    } else if let Some(idx) = state.edit.selected_catchment {
        if idx < state.project.catchments.len() {
            let catchment = &mut state.project.catchments[idx];
            let area_ac = sqft_to_acres(shoelace_area_sqft(&catchment.vertices));
            ui.heading(format!("Catchment: {}", catchment.id));
            ui.separator();
            ui.label(format!("Area: {area_ac:.3} ac (from polygon)"));
            ui.horizontal(|ui| {
                ui.label("C:");
                if ui
                    .add(egui::DragValue::new(&mut catchment.c).speed(0.01).range(0.0..=1.0))
                    .changed()
                {
                    changed = true;
                }
                ui.label("Flow length (ft):");
                if ui
                    .add(
                        egui::DragValue::new(&mut catchment.flow_length_ft)
                            .speed(1.0)
                            .range(1.0..=10000.0),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Slope:");
                if ui
                    .add(egui::DragValue::new(&mut catchment.slope).speed(0.001).range(0.0..=1.0))
                    .changed()
                {
                    changed = true;
                }
                if ui.button("Tc Calculator…").clicked() {
                    open_tc = true;
                }
            });
            let inlet_ids: Vec<String> = state
                .project
                .nodes
                .iter()
                .filter(|n| n.kind == "inlet")
                .map(|n| n.id.clone())
                .collect();
            let current_inlet = catchment.inlet_node_id.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Inlet:");
                egui::ComboBox::from_id_salt("inspector_catchment_inlet")
                    .selected_text(if current_inlet.is_empty() {
                        "(none)"
                    } else {
                        &current_inlet
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(&mut catchment.inlet_node_id, None, "(none)")
                            .clicked()
                        {
                            changed = true;
                        }
                        for inlet_id in &inlet_ids {
                            if ui
                                .selectable_value(
                                    &mut catchment.inlet_node_id,
                                    Some(inlet_id.clone()),
                                    inlet_id,
                                )
                                .clicked()
                            {
                                changed = true;
                            }
                        }
                    });
            });
            ui.separator();
            if ui.button("Delete").clicked() {
                delete_catchment = Some((idx, catchment.id.clone()));
            }
        }
    }

    if let Some((idx, id)) = delete_catchment {
        state.record_undo_snapshot(edit_snapshot);
        state.project.catchments.remove(idx);
        state.clear_selection();
        state.status = format!("Deleted catchment {id}");
        state.run_analysis();
        return;
    }

    if open_tc {
        state.open_tc_calculator();
    }

    if sync_lengths {
        sync_pipe_lengths(&mut state.project);
    }

    if do_delete {
        state.record_undo_snapshot(edit_snapshot);
        if let Some(msg) = delete_selection(
            &mut state.project,
            state.selected_node,
            state.selected_pipe,
        ) {
            state.status = msg;
            state.clear_selection();
            state.dragging_node = None;
            state.run_analysis();
        }
    } else if changed {
        state.record_undo_snapshot(edit_snapshot);
        if sync_lengths {
            sync_pipe_lengths(&mut state.project);
        }
        state.run_analysis();
        state.update_inlet_check();
    }
}