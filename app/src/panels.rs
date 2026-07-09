// SPDX-License-Identifier: GPL-3.0-or-later

//! Side-panel UI: project parameters, tables, design review, tools, and hydraulic report.

use eframe::egui::{self, RichText, Ui};

use crate::edit::Tool;
use crate::state::AppState;
use crate::tables;
use crate::theme::palette;
use stormsewer::design::inlets::InletKind;
use stormsewer::design::review::{design_review, Severity};
use stormsewer::units::UnitSystem;

/// Left sidebar tab selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SideTab {
    #[default]
    Parameters,
    Tables,
    Review,
}

/// Left sidebar: tabbed project settings, tables, and design review.
pub fn draw_left_panel(ui: &mut Ui, state: &mut AppState) {
    let (errors, warnings) = state.review_counts();
    let review_tab = if errors + warnings > 0 {
        format!("Review ({errors}E/{warnings}W)")
    } else {
        "Review".into()
    };

    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.side_tab, SideTab::Parameters, "Parameters");
        ui.selectable_value(&mut state.side_tab, SideTab::Tables, "Tables");
        ui.selectable_value(&mut state.side_tab, SideTab::Review, review_tab);
    });

    ui.separator();
    ui.add_space(4.0);

    match state.side_tab {
        SideTab::Parameters => draw_parameters_tab(ui, state),
        SideTab::Tables => tables::draw_tables_tab(ui, state),
        SideTab::Review => draw_review_tab(ui, state),
    }
}

fn draw_parameters_tab(ui: &mut Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    if state.analysis_stale {
        ui.horizontal(|ui| {
            ui.colored_label(palette::stale_text(dark), "Results may be outdated");
            if ui.button("Re-analyze now").clicked() {
                state.run_analysis();
            }
        });
        ui.add_space(4.0);
    }

    ui.heading("Project");
    ui.separator();

    ui.label("Name");
    ui.text_edit_singleline(&mut state.project.name);

    ui.horizontal(|ui| {
        ui.label("Units:");
        let current = state.project.units;
        if ui
            .selectable_label(current == UnitSystem::UsCustomary, "U.S.")
            .clicked()
            && current != UnitSystem::UsCustomary
        {
            state.convert_units(UnitSystem::UsCustomary);
        }
        if ui
            .selectable_label(current == UnitSystem::Si, "SI")
            .clicked()
            && current != UnitSystem::Si
        {
            state.convert_units(UnitSystem::Si);
        }
    });

    ui.add_space(8.0);
    ui.heading("IDF Curve");
    let idf_units = if state.project.units == UnitSystem::Si {
        "mm/hr"
    } else {
        "in/hr"
    };
    ui.label(format!("i = a / (t + b)^c  ({idf_units})"));
    ui.horizontal(|ui| {
        ui.label("a:");
        if ui
            .add(egui::DragValue::new(&mut state.project.idf_a).speed(0.5).range(1.0..=300.0))
            .changed()
        {
            state.mark_analysis_stale();
        }
    });
    ui.horizontal(|ui| {
        ui.label("b:");
        if ui
            .add(egui::DragValue::new(&mut state.project.idf_b).speed(0.1).range(0.1..=60.0))
            .changed()
        {
            state.mark_analysis_stale();
        }
    });
    ui.horizontal(|ui| {
        ui.label("c:");
        if ui
            .add(egui::DragValue::new(&mut state.project.idf_c).speed(0.01).range(0.1..=2.0))
            .changed()
        {
            state.mark_analysis_stale();
        }
    });

    ui.add_space(8.0);
    ui.heading("Hydraulics");
    ui.horizontal(|ui| {
        ui.label("Design RP (yr):");
        if ui
            .add(
                egui::DragValue::new(&mut state.project.design_return_period_years)
                    .speed(1.0)
                    .range(1.0..=500.0),
            )
            .changed()
        {
            state.mark_analysis_stale();
        }
    });
    ui.horizontal(|ui| {
        ui.label("P2 rainfall (in):");
        if ui
            .add(
                egui::DragValue::new(&mut state.project.p2_rainfall_in)
                    .speed(0.1)
                    .range(1.0..=12.0),
            )
            .changed()
        {
            state.mark_analysis_stale();
        }
    });
    ui.label(
        RichText::new("2-yr 24-hr depth for TR-55 / FAA Tc (sheet flow)")
            .size(10.0)
            .color(egui::Color32::GRAY),
    );
    ui.horizontal(|ui| {
        ui.label("Tailwater (ft):");
        let mut use_tw = state.project.tailwater.is_some();
        if ui.checkbox(&mut use_tw, "").changed() {
            state.project.tailwater = if use_tw {
                Some(100.0)
            } else {
                None
            };
            state.mark_analysis_stale();
        }
        if let Some(ref mut tw) = state.project.tailwater {
            if ui.add(egui::DragValue::new(tw).speed(0.1).range(0.0..=500.0)).changed() {
                state.mark_analysis_stale();
            }
        } else {
            ui.label("(none)");
        }
    });
    ui.horizontal(|ui| {
        ui.label("Min Tc (min):");
        if ui
            .add(
                egui::DragValue::new(&mut state.project.min_tc)
                    .speed(0.5)
                    .range(1.0..=120.0),
            )
            .changed()
        {
            state.mark_analysis_stale();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Junction K:");
        if ui
            .add(
                egui::DragValue::new(&mut state.project.junction_k)
                    .speed(0.05)
                    .range(0.0..=2.0),
            )
            .changed()
        {
            state.mark_analysis_stale();
        }
        ui.label("Bend K:");
        if ui
            .add(
                egui::DragValue::new(&mut state.project.bend_loss_coeff)
                    .speed(0.05)
                    .range(0.0..=2.0),
            )
            .on_hover_text("Extra structure loss for flow deflection (0 = off)")
            .changed()
        {
            state.mark_analysis_stale();
        }
    });
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut state.project.hec22_structure_loss, "HEC-22 access-hole loss")
            .on_hover_text(
                "Use the HEC-22 access-hole coefficient Ko (relative size + deflection angle) \
                 at each structure instead of Junction K",
            )
            .changed()
        {
            state.mark_analysis_stale();
        }
        if state.project.hec22_structure_loss {
            ui.label("AH dia (ft):");
            if ui
                .add(
                    egui::DragValue::new(&mut state.project.access_hole_diam_ft)
                        .speed(0.25)
                        .range(1.0..=20.0),
                )
                .changed()
            {
                state.mark_analysis_stale();
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Min slope:");
        if ui
            .add(
                egui::DragValue::new(&mut state.project.min_slope)
                    .speed(0.0001)
                    .range(0.0..=0.05),
            )
            .changed()
        {
            state.run_analysis();
        }
    });
    ui.checkbox(&mut state.show_multi_rp, "Show multi-RP comparison");

    ui.add_space(8.0);
    ui.heading("Design Codes");
    ui.label("(Hydraflow: Design Codes dialog)");
    ui.horizontal(|ui| {
        ui.label("Min V (ft/s):");
        ui.add(
            egui::DragValue::new(&mut state.review_criteria.min_velocity)
                .speed(0.1)
                .range(0.5..=5.0),
        );
        ui.label("Max V:");
        ui.add(
            egui::DragValue::new(&mut state.review_criteria.max_velocity)
                .speed(0.5)
                .range(5.0..=20.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Max % full:");
        ui.add(
            egui::DragValue::new(&mut state.review_criteria.max_pct_full)
                .speed(0.01)
                .range(0.5..=1.0),
        );
        ui.label("Min cover (ft):");
        ui.add(
            egui::DragValue::new(&mut state.review_criteria.min_cover_ft)
                .speed(0.1)
                .range(0.0..=10.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Review min slope:");
        ui.add(
            egui::DragValue::new(&mut state.review_criteria.min_slope)
                .speed(0.0001)
                .range(0.0..=0.01),
        );
    });
    ui.checkbox(
        &mut state.review_criteria.check_size_progression,
        "Check pipe size progression",
    );

    ui.add_space(8.0);
    ui.heading("Inlet Analysis (HEC-22)");
    ui.horizontal(|ui| {
        ui.label("Type:");
        egui::ComboBox::from_id_salt("inlet_kind")
            .selected_text(state.inlet_geom.kind.label())
            .show_ui(ui, |ui| {
                for kind in [
                    InletKind::GrateOnGrade,
                    InletKind::CurbOpening,
                    InletKind::Combination,
                    InletKind::SagGrate,
                ] {
                    ui.selectable_value(&mut state.inlet_geom.kind, kind, kind.label());
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("Grate L×W (ft):");
        ui.add(
            egui::DragValue::new(&mut state.inlet_geom.grate_length_ft)
                .speed(0.1)
                .range(0.5..=20.0),
        );
        ui.add(
            egui::DragValue::new(&mut state.inlet_geom.grate_width_ft)
                .speed(0.1)
                .range(0.5..=10.0),
        );
        ui.label("Curb L (ft):");
        ui.add(
            egui::DragValue::new(&mut state.inlet_geom.curb_opening_length_ft)
                .speed(0.1)
                .range(0.5..=20.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Sx / SL:");
        ui.add(
            egui::DragValue::new(&mut state.inlet_geom.cross_slope)
                .speed(0.002)
                .range(0.005..=0.1),
        )
        .on_hover_text("Gutter cross slope Sx");
        ui.add(
            egui::DragValue::new(&mut state.inlet_geom.gutter_slope)
                .speed(0.001)
                .range(0.001..=0.1),
        )
        .on_hover_text("Longitudinal gutter slope SL");
        ui.label("n:");
        ui.add(
            egui::DragValue::new(&mut state.inlet_geom.gutter_n)
                .speed(0.001)
                .range(0.010..=0.05),
        );
        ui.label("Allow. spread (ft):");
        ui.add(
            egui::DragValue::new(&mut state.inlet_geom.allowable_spread_ft)
                .speed(0.5)
                .range(2.0..=40.0),
        );
    });
    if ui.button("Check Selected Inlet").clicked() {
        state.update_inlet_check();
    }
    if !state.inlet_check_text.is_empty() {
        ui.label(
            RichText::new(&state.inlet_check_text)
                .monospace()
                .size(11.0),
        );
    }

    ui.add_space(12.0);
    ui.separator();

    if ui.button("Analyze").clicked() {
        state.run_analysis();
    }
    if ui.button("Auto-Size Pipes").clicked() {
        state.apply_sizing();
    }
    if ui.button("Tc Calculator…").clicked() {
        state.open_tc_calculator();
    }

    ui.add_space(8.0);
    ui.heading("Construction Cost");
    ui.label(
        RichText::new(&state.cost_text)
            .monospace()
            .size(10.0),
    );

    ui.add_space(12.0);
    ui.heading("Tools");
    ui.separator();

    let tool_button = |ui: &mut Ui, tool: Tool, active: bool| -> bool {
        let label = format!("{} ({})", tool.label(), tool.shortcut());
        let text = if active {
            RichText::new(label).strong()
        } else {
            RichText::new(label)
        };
        ui.selectable_label(active, text).clicked()
    };

    ui.horizontal_wrapped(|ui| {
        if tool_button(ui, Tool::Select, state.tool == Tool::Select) {
            state.tool = Tool::Select;
            state.edit.tool = Tool::Select;
            state.edit.pipe_from = None;
        }
        if tool_button(ui, Tool::PlaceInlet, state.tool == Tool::PlaceInlet) {
            state.tool = Tool::PlaceInlet;
            state.edit.tool = Tool::PlaceInlet;
            state.edit.pipe_from = None;
        }
        if tool_button(ui, Tool::PlaceJunction, state.tool == Tool::PlaceJunction) {
            state.tool = Tool::PlaceJunction;
            state.edit.tool = Tool::PlaceJunction;
            state.edit.pipe_from = None;
        }
        if tool_button(ui, Tool::PlaceOutfall, state.tool == Tool::PlaceOutfall) {
            state.tool = Tool::PlaceOutfall;
            state.edit.tool = Tool::PlaceOutfall;
            state.edit.pipe_from = None;
        }
        if tool_button(ui, Tool::DrawPipe, state.tool == Tool::DrawPipe) {
            state.tool = Tool::DrawPipe;
            state.edit.tool = Tool::DrawPipe;
        }
        if tool_button(ui, Tool::DrawCatchment, state.tool == Tool::DrawCatchment) {
            state.tool = Tool::DrawCatchment;
            state.edit.tool = Tool::DrawCatchment;
            state.edit.pipe_from = None;
        }
    });

    if state.tool == Tool::DrawPipe {
        if let Some(ref from) = state.edit.pipe_from {
            ui.label(format!(
                "Run from: {from} — click to extend; Esc, right-click, or double-click to finish"
            ));
        } else {
            ui.label("Click to drop manholes and link them; click a node to tie in");
        }
    } else if state.tool == Tool::DrawCatchment {
        ui.label("Click vertices; click near first point to close polygon");
    }

    if let Some(ref mut bg) = state.project.background {
        ui.add_space(8.0);
        ui.heading("Background");
        ui.horizontal(|ui| {
            ui.label("Opacity:");
            ui.add(egui::Slider::new(&mut bg.opacity, 0.1..=1.0).show_value(true));
        });
        ui.horizontal(|ui| {
            ui.label("Width (ft):");
            ui.add(egui::DragValue::new(&mut bg.width).speed(10.0).range(10.0..=5000.0));
        });
    }
}

fn draw_review_tab(ui: &mut Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    ui.heading("Network Diagnostics");
    ui.separator();
    egui::ScrollArea::vertical()
        .max_height(120.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new(&state.diagnostics_text)
                    .monospace()
                    .size(10.0),
            );
        });

    ui.add_space(8.0);
    ui.heading("Design Review");
    ui.separator();

    let Some(ref analysis) = state.analysis else {
        ui.label("Run analysis to generate design review findings.");
        return;
    };

    let net = state.project.to_network();
    let findings = design_review(&net, analysis, &state.review_criteria);

    if findings.is_empty() {
        ui.colored_label(palette::ok_text(dark), "No design issues found.");
        return;
    }

    ui.label("Click a finding to select it on the plan.");
    let findings_snapshot: Vec<_> = findings.to_vec();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for finding in &findings_snapshot {
            let (color, tag) = match finding.severity {
                Severity::Error => (palette::error_text(dark), "Error"),
                Severity::Warning => (palette::warning_text(dark), "Warning"),
            };
            let label = format!("[{tag}] {} — {}", finding.id, finding.message);
            if ui
                .add(egui::Button::new(RichText::new(label).color(color)))
                .clicked()
            {
                state.select_by_id(&finding.id);
                state.view_tab = crate::state::ViewTab::Plan;
                state.status = format!("Selected {} from design review", finding.id);
            }
        }
    });
}

/// Right sidebar: monospace hydraulic report and pipe-sizing summary.
pub fn draw_report_panel(ui: &mut Ui, state: &AppState) {
    ui.heading("Hydraulic Report");
    if state.analysis_stale {
        ui.colored_label(
            palette::stale_text(ui.visuals().dark_mode),
            "Parameters changed — re-analyze to refresh this report",
        );
    }
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(
            RichText::new(&state.report_text)
                .monospace()
                .size(11.0),
        );

        if !state.sizing_text.is_empty() {
            ui.add_space(12.0);
            ui.separator();
            ui.heading("Pipe Sizing");
            ui.label(
                RichText::new(&state.sizing_text)
                    .monospace()
                    .size(11.0),
            );
        }

        if !state.review_text.is_empty() {
            ui.add_space(12.0);
            ui.separator();
            ui.heading("Design Review");
            ui.label(
                RichText::new(&state.review_text)
                    .monospace()
                    .size(11.0),
            );
        }

        if state.show_multi_rp && !state.multi_rp_text.is_empty() {
            ui.add_space(12.0);
            ui.separator();
            ui.heading("Multi-RP Comparison");
            ui.label(
                RichText::new(&state.multi_rp_text)
                    .monospace()
                    .size(11.0),
            );
        }
    });
}