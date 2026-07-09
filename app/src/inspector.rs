// SPDX-License-Identifier: GPL-3.0-or-later

//! Bottom-panel property inspector for selected nodes and pipes.

use eframe::egui::{self, RichText, Ui};

use crate::edit::{delete_selection, sync_pipe_lengths};
use crate::state::AppState;
use crate::theme::palette;
use stormsewer::catchment::{shoelace_area_sqft, sqft_to_acres};
use stormsewer::network::{NodeResult, PipeResult};

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

    // Capture (cloned) analysis results for the selection up front, so the
    // read-only readout can be drawn after the mutable edit borrows are released.
    let pipe_res: Option<PipeResult> = state
        .selected_pipe
        .and_then(|i| state.project.pipes.get(i).map(|p| p.id.clone()))
        .and_then(|id| {
            state.analysis.as_ref()?.pipes.iter().find(|r| r.id == id).cloned()
        });
    let node_res: Option<NodeResult> = state
        .selected_node
        .and_then(|i| state.project.nodes.get(i).map(|n| n.id.clone()))
        .and_then(|id| {
            state.analysis.as_ref()?.nodes.iter().find(|r| r.id == id).cloned()
        });

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
                        for shape in ["circular", "box", "elliptical", "arch"] {
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

    if let Some(r) = &pipe_res {
        draw_pipe_results(ui, r);
    }
    if let Some(r) = &node_res {
        draw_node_results(ui, r);
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

/// One `label : value` row in a results grid.
fn kv(ui: &mut Ui, key: &str, value: String) {
    ui.label(RichText::new(key).color(palette::muted_text(ui.visuals().dark_mode)));
    ui.label(value);
    ui.end_row();
}

/// Live hydraulics for the selected pipe, color-coded by design status.
fn draw_pipe_results(ui: &mut Ui, r: &PipeResult) {
    let dark = ui.visuals().dark_mode;
    ui.separator();
    ui.label(RichText::new("Analysis results").strong());
    egui::Grid::new("inspector_pipe_results")
        .num_columns(2)
        .spacing([16.0, 2.0])
        .show(ui, |ui| {
            kv(ui, "Design Q", format!("{:.2} cfs", r.design_q));
            kv(ui, "Full capacity", format!("{:.2} cfs", r.capacity));

            let pct = r.pct_full * 100.0;
            let pct_color = if r.surcharged || pct > 100.0 {
                palette::error_text(dark)
            } else if pct > 85.0 {
                palette::warning_text(dark)
            } else {
                palette::ok_text(dark)
            };
            ui.label(RichText::new("% full").color(palette::muted_text(dark)));
            ui.label(RichText::new(format!("{pct:.0}%")).color(pct_color));
            ui.end_row();

            kv(ui, "Velocity", format!("{:.2} ft/s", r.velocity));
            match r.normal_depth {
                Some(y) => kv(ui, "Normal depth", format!("{y:.2} ft")),
                None => {
                    ui.label(RichText::new("Normal depth").color(palette::muted_text(dark)));
                    ui.label(RichText::new("surcharged").color(palette::error_text(dark)));
                    ui.end_row();
                }
            }
            kv(ui, "Critical depth", format!("{:.2} ft", r.critical_depth));
            kv(ui, "Flow regime", r.regime().label().to_string());
            kv(ui, "Slope", format!("{:.4} ft/ft", r.slope));
            if let (Some(up), Some(dn)) = (r.hgl_up, r.hgl_dn) {
                kv(ui, "HGL up / dn", format!("{up:.2} / {dn:.2} ft"));
            }
        });
}

/// Live hydraulics for the selected structure (Tc, HGL, freeboard).
fn draw_node_results(ui: &mut Ui, r: &NodeResult) {
    let dark = ui.visuals().dark_mode;
    ui.separator();
    ui.label(RichText::new("Analysis results").strong());
    egui::Grid::new("inspector_node_results")
        .num_columns(2)
        .spacing([16.0, 2.0])
        .show(ui, |ui| {
            kv(ui, "Tc", format!("{:.1} min", r.tc));
            if r.hgl.is_finite() {
                let hgl_color = if r.surcharge_to_surface {
                    palette::error_text(dark)
                } else {
                    ui.visuals().text_color()
                };
                ui.label(RichText::new("HGL").color(palette::muted_text(dark)));
                ui.label(RichText::new(format!("{:.2} ft", r.hgl)).color(hgl_color));
                ui.end_row();
                kv(ui, "Rim", format!("{:.2} ft", r.rim));
                let freeboard = r.rim - r.hgl;
                let fb_color = if freeboard < 0.0 {
                    palette::error_text(dark)
                } else if freeboard < 1.0 {
                    palette::warning_text(dark)
                } else {
                    palette::ok_text(dark)
                };
                ui.label(RichText::new("Freeboard").color(palette::muted_text(dark)));
                ui.label(RichText::new(format!("{freeboard:.2} ft")).color(fb_color));
                ui.end_row();
            } else {
                kv(ui, "Rim", format!("{:.2} ft", r.rim));
            }
        });
    if r.surcharge_to_surface {
        ui.label(
            RichText::new("! HGL above rim — surface flooding")
                .strong()
                .color(palette::error_text(dark)),
        );
    }
}