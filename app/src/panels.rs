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
    // Parameter edits (IDF, hydraulics, design codes, inlet geometry) are
    // undo-able: snapshot the document, and if any widget in this panel
    // changed it this frame, record one undo step per edit gesture.
    let edit_snapshot = state.project.clone();
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

    if state.project != edit_snapshot {
        let gesture_active = ui.ctx().input(|inp| inp.pointer.any_down())
            || ui.ctx().memory(|m| m.focused().is_some());
        if !state.undo_gesture_active {
            state.undo.record_previous(edit_snapshot);
        }
        state.undo_gesture_active = gesture_active;
        state.mark_project_dirty();
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

    ui.collapsing("Report info (for submittals)", |ui| {
        egui::Grid::new("report_info_grid").num_columns(2).show(ui, |ui| {
            ui.label("Project No.");
            ui.text_edit_singleline(&mut state.project.report.project_number);
            ui.end_row();
            ui.label("Engineer");
            ui.text_edit_singleline(&mut state.project.report.engineer);
            ui.end_row();
            ui.label("Firm");
            ui.text_edit_singleline(&mut state.project.report.firm);
            ui.end_row();
            ui.label("Jurisdiction");
            ui.text_edit_singleline(&mut state.project.report.jurisdiction);
            ui.end_row();
        });
    });

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

    // Fitted multi-return-period curves (from NOAA Atlas 14 import).
    if !state.project.idf_curves.is_empty() {
        let n = state.project.idf_curves.len();
        egui::CollapsingHeader::new(format!("Fitted curves ({n} return periods)"))
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new("idf_fitted_grid")
                    .num_columns(4)
                    .spacing([12.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("RP (yr)").strong());
                        ui.label(RichText::new("a").strong());
                        ui.label(RichText::new("b").strong());
                        ui.label(RichText::new("c").strong());
                        ui.end_row();
                        for c in &state.project.idf_curves {
                            ui.label(format!("{}", c.rp_years));
                            ui.label(format!("{:.2}", c.a));
                            ui.label(format!("{:.2}", c.b));
                            ui.label(format!("{:.3}", c.c));
                            ui.end_row();
                        }
                    });
                if ui.button("Clear fitted curves").clicked() {
                    state.project.idf_curves.clear();
                    state.mark_analysis_stale();
                }
            });
    }
    if ui
        .button("Paste NOAA data…")
        .on_hover_text("Fit a/b/c curves from a NOAA Atlas 14 precipitation CSV")
        .clicked()
    {
        state.noaa_paste_open = true;
    }

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
    if ui
        .checkbox(&mut state.prefs.auto_analyze, "Live recompute")
        .on_hover_text("Re-run the analysis automatically after every edit")
        .changed()
    {
        state.prefs.save();
    }

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
        if let Some(a) = &state.analysis {
            draw_schedules(ui, state, a);
            ui.add_space(6.0);
            egui::CollapsingHeader::new("Report text (for copy/paste)")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(&state.report_text)
                            .monospace()
                            .size(11.0),
                    );
                });
        } else {
            ui.label(
                RichText::new(&state.report_text)
                    .monospace()
                    .size(11.0),
            );
        }

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
/// Small-caps section label, drafting-schedule style.
fn eyebrow(ui: &mut Ui, text: &str) {
    let dark = ui.visuals().dark_mode;
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.5)
            .strong()
            .color(palette::muted_text(dark)),
    );
}

/// Right-aligned monospace cell — schedules are read down columns.
fn num_cell(ui: &mut Ui, text: String) {
    ui.with_layout(
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(RichText::new(text).monospace().size(11.5));
        },
    );
}

/// Colored status dot + short word; wording matches the review vocabulary.
fn status_cell(ui: &mut Ui, color: egui::Color32, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.5, color);
        ui.label(RichText::new(label).size(11.0).color(color));
    });
}

/// The analysis rendered as drawing-set schedules: a pipe schedule and a
/// structure schedule, in place of a raw text dump. The full text stays one
/// click away for pasting into submittals.
fn draw_schedules(ui: &mut Ui, state: &AppState, a: &stormsewer::network::Analysis) {
    use stormsewer::units::UnitSystem;
    let dark = ui.visuals().dark_mode;
    let si = state.project.units == UnitSystem::Si;
    let (q_u, v_u, el_u, sz_u) = if si {
        ("m³/s", "m/s", "m", "mm")
    } else {
        ("cfs", "ft/s", "ft", "in")
    };

    eyebrow(ui, "Pipe schedule");
    egui::Grid::new("pipe_schedule")
        .striped(true)
        .min_col_width(30.0)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            for h in [
                "PIPE".to_owned(),
                format!("SIZE {sz_u}"),
                "SLOPE".into(),
                format!("Q {q_u}"),
                format!("CAP {q_u}"),
                "FULL".into(),
                format!("VEL {v_u}"),
                format!("HGL DN {el_u}"),
                "".into(),
            ] {
                ui.label(
                    RichText::new(h)
                        .size(9.5)
                        .color(palette::muted_text(dark)),
                );
            }
            ui.end_row();

            for pr in &a.pipes {
                ui.label(RichText::new(&pr.id).monospace().size(11.5).strong());
                let size = state
                    .project
                    .pipes
                    .iter()
                    .find(|p| p.id == pr.id)
                    .map(|p| {
                        if p.shape == "circular" {
                            if si {
                                format!("{:.0}", p.diameter * 1000.0)
                            } else {
                                format!("{:.0}", p.diameter * 12.0)
                            }
                        } else {
                            p.shape.clone()
                        }
                    })
                    .unwrap_or_default();
                num_cell(ui, size);
                num_cell(ui, format!("{:.4}", pr.slope));
                num_cell(ui, format!("{:.2}", pr.design_q));
                num_cell(ui, format!("{:.2}", pr.capacity));
                num_cell(ui, format!("{:.0}%", pr.pct_full * 100.0));
                num_cell(ui, format!("{:.2}", pr.velocity));
                num_cell(
                    ui,
                    pr.hgl_dn.map(|h| format!("{h:.2}")).unwrap_or("—".into()),
                );
                if pr.surcharged {
                    status_cell(ui, palette::error_text(dark), "Surcharged");
                } else if pr.pct_full > 0.85 {
                    status_cell(ui, palette::warning_text(dark), "Near full");
                } else {
                    status_cell(ui, palette::ok_text(dark), "OK");
                }
                ui.end_row();
            }
        });

    ui.add_space(10.0);
    eyebrow(ui, "Structure schedule");
    egui::Grid::new("structure_schedule")
        .striped(true)
        .min_col_width(30.0)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            for h in [
                "NODE".to_owned(),
                "TC min".into(),
                format!("RIM {el_u}"),
                format!("HGL {el_u}"),
                format!("FREEBD {el_u}"),
                "".into(),
            ] {
                ui.label(
                    RichText::new(h)
                        .size(9.5)
                        .color(palette::muted_text(dark)),
                );
            }
            ui.end_row();

            for nr in &a.nodes {
                ui.label(RichText::new(&nr.id).monospace().size(11.5).strong());
                num_cell(ui, format!("{:.1}", nr.tc));
                num_cell(ui, format!("{:.2}", nr.rim));
                num_cell(ui, format!("{:.2}", nr.hgl));
                num_cell(ui, format!("{:.2}", nr.rim - nr.hgl));
                if nr.surcharge_to_surface {
                    status_cell(ui, palette::error_text(dark), "Floods");
                } else if nr.rim - nr.hgl < 1.0 {
                    status_cell(ui, palette::warning_text(dark), "Low freebd");
                } else {
                    status_cell(ui, palette::ok_text(dark), "OK");
                }
                ui.end_row();
            }
        });

    if !state.inlet_rows.is_empty() {
        ui.add_space(10.0);
        eyebrow(ui, "Inlet schedule (HEC-22, with bypass carryover)");
        egui::Grid::new("inlet_schedule")
            .striped(true)
            .min_col_width(30.0)
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                for h in [
                    "INLET",
                    "LOCAL cfs",
                    "C/O IN",
                    "INTERC",
                    "BYPASS",
                    "TO",
                    "SPREAD ft",
                    "",
                ] {
                    ui.label(
                        RichText::new(h)
                            .size(9.5)
                            .color(palette::muted_text(dark)),
                    );
                }
                ui.end_row();
                for r in &state.inlet_rows {
                    ui.label(
                        RichText::new(&r.node_id).monospace().size(11.5).strong(),
                    );
                    num_cell(ui, format!("{:.2}", r.local_cfs));
                    num_cell(ui, format!("{:.2}", r.carryover_in_cfs));
                    num_cell(ui, format!("{:.2}", r.intercepted_cfs));
                    num_cell(ui, format!("{:.2}", r.bypass_cfs));
                    ui.label(
                        RichText::new(
                            r.bypass_to.as_deref().unwrap_or("(off)"),
                        )
                        .monospace()
                        .size(11.0),
                    );
                    num_cell(
                        ui,
                        if r.spread_ft > 0.0 {
                            format!("{:.1}", r.spread_ft)
                        } else {
                            "—".into()
                        },
                    );
                    if r.cycle_broken {
                        status_cell(ui, palette::warning_text(dark), "Cycle");
                    } else if !r.ok {
                        status_cell(ui, palette::error_text(dark), "Exceeds");
                    } else if r.bypass_cfs > 0.005 {
                        status_cell(ui, palette::warning_text(dark), "Bypassing");
                    } else {
                        status_cell(ui, palette::ok_text(dark), "OK");
                    }
                    ui.end_row();
                }
            });
        ui.label(
            RichText::new(
                "Pipe design flows remain full Rational C·A (conservative); \
                 this schedule checks surface capture and spread.",
            )
            .size(9.5)
            .color(palette::muted_text(dark)),
        );
    }
}
