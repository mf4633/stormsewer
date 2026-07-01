// SPDX-License-Identifier: GPL-3.0-or-later

//! Tabular editors for project structures and pipes.

use eframe::egui::{self, DragValue, Ui};

use crate::state::AppState;

/// Structures and pipes tables with row selection and inline numeric editing.
pub fn draw_tables_tab(ui: &mut Ui, state: &mut AppState) {
    let mut needs_analysis = false;
    let edit_snapshot = state.project.clone();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Structures");
        ui.separator();

        egui::Grid::new("structures_table")
            .num_columns(6)
            .spacing([8.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("ID").strong());
                ui.label(egui::RichText::new("Kind").strong());
                ui.label(egui::RichText::new("Invert").strong());
                ui.label(egui::RichText::new("Rim").strong());
                ui.label(egui::RichText::new("Area").strong());
                ui.label(egui::RichText::new("C").strong());
                ui.end_row();

                let count = state.project.nodes.len();
                for idx in 0..count {
                    let id = state.project.nodes[idx].id.clone();
                    let kind = state.project.nodes[idx].kind.clone();
                    let selected = state.selected_node == Some(idx);

                    if ui.selectable_label(selected, &id).clicked() {
                        state.set_selection(Some(idx), None, None);
                    }

                    let node = &mut state.project.nodes[idx];
                    ui.label(&kind);

                    if ui
                        .add(DragValue::new(&mut node.invert).speed(0.1).range(0.0..=500.0))
                        .changed()
                    {
                        needs_analysis = true;
                    }
                    if ui
                        .add(DragValue::new(&mut node.rim).speed(0.1).range(0.0..=600.0))
                        .changed()
                    {
                        needs_analysis = true;
                    }
                    if ui
                        .add(
                            DragValue::new(&mut node.area_ac)
                                .speed(0.05)
                                .range(0.0..=1000.0),
                        )
                        .changed()
                    {
                        needs_analysis = true;
                    }
                    if ui
                        .add(DragValue::new(&mut node.c).speed(0.01).range(0.0..=1.0))
                        .changed()
                    {
                        needs_analysis = true;
                    }

                    ui.end_row();
                }
            });

        ui.add_space(12.0);
        ui.heading("Pipes");
        ui.separator();

        egui::Grid::new("pipes_table")
            .num_columns(6)
            .spacing([8.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("ID").strong());
                ui.label(egui::RichText::new("From").strong());
                ui.label(egui::RichText::new("To").strong());
                ui.label(egui::RichText::new("Length").strong());
                ui.label(egui::RichText::new("Dia (in)").strong());
                ui.label(egui::RichText::new("n").strong());
                ui.end_row();

                let count = state.project.pipes.len();
                for idx in 0..count {
                    let id = state.project.pipes[idx].id.clone();
                    let from = state.project.pipes[idx].from.clone();
                    let to = state.project.pipes[idx].to.clone();
                    let selected = state.selected_pipe == Some(idx);

                    if ui.selectable_label(selected, &id).clicked() {
                        state.set_selection(None, Some(idx), None);
                    }

                    let pipe = &mut state.project.pipes[idx];
                    ui.label(&from);
                    ui.label(&to);

                    if ui
                        .add(DragValue::new(&mut pipe.length).speed(1.0).range(1.0..=10000.0))
                        .changed()
                    {
                        needs_analysis = true;
                    }

                    let mut dia_in = pipe.diameter * 12.0;
                    if ui
                        .add(DragValue::new(&mut dia_in).speed(1.0).range(6.0..=120.0))
                        .changed()
                    {
                        pipe.diameter = dia_in / 12.0;
                        needs_analysis = true;
                    }

                    if ui
                        .add(DragValue::new(&mut pipe.n).speed(0.001).range(0.009..=0.05))
                        .changed()
                    {
                        needs_analysis = true;
                    }

                    ui.end_row();
                }
            });

        ui.add_space(12.0);
        ui.heading("Catchments");
        ui.separator();

        let areas = state.project.catchment_areas();
        let inlet_ids: Vec<String> = state
            .project
            .nodes
            .iter()
            .filter(|n| n.kind == "inlet")
            .map(|n| n.id.clone())
            .collect();

        egui::Grid::new("catchments_table")
            .num_columns(6)
            .spacing([8.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("ID").strong());
                ui.label(egui::RichText::new("Area (ac)").strong());
                ui.label(egui::RichText::new("C").strong());
                ui.label(egui::RichText::new("Flow Len").strong());
                ui.label(egui::RichText::new("Slope").strong());
                ui.label(egui::RichText::new("Inlet").strong());
                ui.end_row();

                let count = state.project.catchments.len();
                for idx in 0..count {
                    let id = state.project.catchments[idx].id.clone();
                    let selected = state.edit.selected_catchment == Some(idx);

                    if ui.selectable_label(selected, &id).clicked() {
                        state.set_selection(None, None, Some(idx));
                    }

                    let catchment = &mut state.project.catchments[idx];
                    let area = areas.get(idx).copied().unwrap_or(0.0);
                    ui.label(format!("{area:.3}"));

                    if ui
                        .add(DragValue::new(&mut catchment.c).speed(0.01).range(0.0..=1.0))
                        .changed()
                    {
                        needs_analysis = true;
                    }
                    if ui
                        .add(
                            DragValue::new(&mut catchment.flow_length_ft)
                                .speed(1.0)
                                .range(1.0..=10000.0),
                        )
                        .changed()
                    {
                        needs_analysis = true;
                    }
                    if ui
                        .add(DragValue::new(&mut catchment.slope).speed(0.001).range(0.0..=1.0))
                        .changed()
                    {
                        needs_analysis = true;
                    }

                    let current_inlet = catchment.inlet_node_id.clone().unwrap_or_default();
                    egui::ComboBox::from_id_salt(format!("catchment_inlet_{idx}"))
                        .selected_text(if current_inlet.is_empty() {
                            "(none)"
                        } else {
                            &current_inlet
                        })
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut catchment.inlet_node_id, None, "(none)")
                                .clicked()
                            {
                                needs_analysis = true;
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
                                    needs_analysis = true;
                                }
                            }
                        });

                    ui.end_row();
                }
            });

        if ui.button("Delete Catchment").clicked() {
            if let Some(idx) = state.edit.selected_catchment {
                if idx < state.project.catchments.len() {
                    let id = state.project.catchments[idx].id.clone();
                    state.project.catchments.remove(idx);
                    state.edit.selected_catchment = None;
                    state.status = format!("Deleted catchment {id}");
                    needs_analysis = true;
                }
            }
        }
    });

    if needs_analysis {
        state.record_undo_snapshot(edit_snapshot);
        state.run_analysis();
    }
}