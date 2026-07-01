// SPDX-License-Identifier: GPL-3.0-or-later

//! Tc calculator and TR-55 worksheet (Hydraflow FAA / TR-55 / Kirpich).

use crate::state::AppState;
use eframe::egui::{self, RichText};
use stormsewer::hydrology::{
    faa_sheet_flow_minutes, format_tr55_worksheet, kirpich_minutes, tr55_sheet_flow_minutes,
    Tr55Segment, Tr55SegmentKind, tr55_worksheet_tc_minutes,
};

/// Single-method or multi-segment TR-55 worksheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TcCalcMode {
    #[default]
    SingleMethod,
    Tr55Worksheet,
}

/// Tc estimation method (single-method mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TcMethod {
    #[default]
    Faa,
    Tr55,
    Kirpich,
}

impl TcMethod {
    fn label(self) -> &'static str {
        match self {
            Self::Faa => "FAA (paved, n=0.02)",
            Self::Tr55 => "TR-55 sheet flow",
            Self::Kirpich => "Kirpich (channel/overland)",
        }
    }

    fn compute(self, length: f64, slope: f64, n: f64, p2_in: f64) -> f64 {
        match self {
            Self::Faa => faa_sheet_flow_minutes(length, slope, p2_in),
            Self::Tr55 => tr55_sheet_flow_minutes(length, slope, n, p2_in),
            Self::Kirpich => kirpich_minutes(length, slope),
        }
    }
}

/// Tc calculator window state.
#[derive(Clone, Debug)]
pub struct TcCalcState {
    pub open: bool,
    pub mode: TcCalcMode,
    pub method: TcMethod,
    pub length: f64,
    pub slope: f64,
    pub roughness: f64,
    /// 2-yr 24-hr rainfall (in) for FAA / TR-55 sheet flow (TR-55 Eq. 3-3).
    pub p2_in: f64,
    pub result_min: f64,
    pub segments: Vec<Tr55Segment>,
    pub worksheet_text: String,
}

impl Default for TcCalcState {
    fn default() -> Self {
        Self {
            open: false,
            mode: TcCalcMode::default(),
            method: TcMethod::default(),
            length: 300.0,
            slope: 0.01,
            roughness: 0.02,
            p2_in: 3.0,
            result_min: 0.0,
            segments: vec![
                Tr55Segment {
                    kind: Tr55SegmentKind::Sheet,
                    length_ft: 100.0,
                    slope: 0.02,
                    n: 0.02,
                    paved: false,
                    p2_in: 3.0,
                },
                Tr55Segment {
                    kind: Tr55SegmentKind::ShallowConcentrated,
                    length_ft: 200.0,
                    slope: 0.01,
                    n: 0.0,
                    paved: true,
                    p2_in: 3.0,
                },
                Tr55Segment {
                    kind: Tr55SegmentKind::Channel,
                    length_ft: 400.0,
                    slope: 0.005,
                    n: 0.035,
                    paved: false,
                    p2_in: 3.0,
                },
            ],
            worksheet_text: String::new(),
        }
    }
}

/// Draw the Tc calculator modal.
pub fn draw_tc_calc_window(ctx: &egui::Context, app: &mut AppState) {
    if !app.tc_calc.open {
        return;
    }

    let mut close = false;
    let mut calc = false;
    let selection_hint = app
        .selection_label()
        .unwrap_or_else(|| "None — select a structure or catchment first".into());

    let mut open = app.tc_calc.open;
    egui::Window::new("Time of Concentration Calculator")
        .collapsible(false)
        .resizable(true)
        .default_width(480.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Apply to:");
                ui.label(RichText::new(&selection_hint).strong());
            });
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut app.tc_calc.mode,
                    TcCalcMode::SingleMethod,
                    "Single Method",
                );
                ui.selectable_value(
                    &mut app.tc_calc.mode,
                    TcCalcMode::Tr55Worksheet,
                    "TR-55 Worksheet",
                );
            });
            ui.separator();

            match app.tc_calc.mode {
                TcCalcMode::SingleMethod => draw_single_method(ui, &mut app.tc_calc, &mut calc),
                TcCalcMode::Tr55Worksheet => draw_worksheet(ui, &mut app.tc_calc, &mut calc),
            }

            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                let can_apply = app.tc_calc.result_min > 0.0 && app.has_selection();
                if ui
                    .add_enabled(
                        can_apply,
                        egui::Button::new("Apply to Selection"),
                    )
                    .clicked()
                {
                    if app.apply_tc_minutes(app.tc_calc.result_min) {
                        close = true;
                    }
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
            if app.tc_calc.result_min > 0.0 && !app.has_selection() {
                ui.label(
                    RichText::new("Select a structure or catchment to apply this Tc")
                        .color(egui::Color32::YELLOW),
                );
            }
        });
    app.tc_calc.open = open;

    if calc {
        match app.tc_calc.mode {
            TcCalcMode::SingleMethod => {
                app.tc_calc.result_min = app.tc_calc.method.compute(
                    app.tc_calc.length,
                    app.tc_calc.slope,
                    app.tc_calc.roughness,
                    app.tc_calc.p2_in,
                );
            }
            TcCalcMode::Tr55Worksheet => {
                app.tc_calc.result_min = tr55_worksheet_tc_minutes(&app.tc_calc.segments);
                app.tc_calc.worksheet_text = format_tr55_worksheet(&app.tc_calc.segments);
            }
        }
    }

    if close {
        app.tc_calc.open = false;
    }
}

fn draw_single_method(ui: &mut egui::Ui, state: &mut TcCalcState, calc: &mut bool) {
    ui.horizontal(|ui| {
        ui.label("Method:");
        egui::ComboBox::from_id_salt("tc_method")
            .selected_text(state.method.label())
            .show_ui(ui, |ui| {
                for m in [TcMethod::Faa, TcMethod::Tr55, TcMethod::Kirpich] {
                    ui.selectable_value(&mut state.method, m, m.label());
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("Flow path length (ft):");
        ui.add(egui::DragValue::new(&mut state.length).speed(5.0).range(10.0..=10000.0));
    });
    ui.horizontal(|ui| {
        ui.label("Slope (ft/ft):");
        ui.add(
            egui::DragValue::new(&mut state.slope)
                .speed(0.001)
                .range(0.0001..=0.5),
        );
    });
    if matches!(state.method, TcMethod::Faa | TcMethod::Tr55) {
        ui.horizontal(|ui| {
            ui.label("2-yr 24-hr rainfall P2 (in):");
            ui.add(
                egui::DragValue::new(&mut state.p2_in)
                    .speed(0.1)
                    .range(1.0..=12.0),
            );
        });
    }
    if state.method == TcMethod::Tr55 {
        ui.horizontal(|ui| {
            ui.label("Manning n:");
            ui.add(
                egui::DragValue::new(&mut state.roughness)
                    .speed(0.005)
                    .range(0.005..=0.2),
            );
        });
    }
    if ui.button("Calculate").clicked() {
        *calc = true;
    }
    if state.result_min > 0.0 {
        ui.label(
            RichText::new(format!("Tc = {:.2} minutes", state.result_min))
                .strong()
                .size(16.0),
        );
    }
}

fn draw_worksheet(ui: &mut egui::Ui, state: &mut TcCalcState, calc: &mut bool) {
    ui.label("Hydraflow: Calculate Tc Using the TR-55 Worksheet — add flow path segments.");
    ui.horizontal(|ui| {
        ui.label("Default P2 for sheet segments (in):");
        ui.add(
            egui::DragValue::new(&mut state.p2_in)
                .speed(0.1)
                .range(1.0..=12.0),
        );
    });
    if ui.button("Calculate Total Tc").clicked() {
        *calc = true;
    }

    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        let mut remove: Option<usize> = None;
        for (i, seg) in state.segments.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Segment {}", i + 1));
                    if ui.button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Type:");
                    egui::ComboBox::from_id_salt(format!("seg_kind_{i}"))
                        .selected_text(seg.kind.label())
                        .show_ui(ui, |ui| {
                            for k in [
                                Tr55SegmentKind::Sheet,
                                Tr55SegmentKind::ShallowConcentrated,
                                Tr55SegmentKind::Channel,
                            ] {
                                ui.selectable_value(&mut seg.kind, k, k.label());
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Length (ft):");
                    ui.add(egui::DragValue::new(&mut seg.length_ft).speed(5.0).range(1.0..=10000.0));
                    ui.label("Slope:");
                    ui.add(
                        egui::DragValue::new(&mut seg.slope)
                            .speed(0.001)
                            .range(0.0001..=0.5),
                    );
                });
                if seg.kind == Tr55SegmentKind::Sheet {
                    ui.horizontal(|ui| {
                        ui.label("Manning n:");
                        ui.add(
                            egui::DragValue::new(&mut seg.n)
                                .speed(0.005)
                                .range(0.005..=0.2),
                        );
                        ui.label("P2 (in):");
                        ui.add(
                            egui::DragValue::new(&mut seg.p2_in)
                                .speed(0.1)
                                .range(1.0..=12.0),
                        );
                    });
                } else if seg.kind == Tr55SegmentKind::ShallowConcentrated {
                    ui.checkbox(&mut seg.paved, "Paved surface (higher velocity)");
                }
            });
        }
        if let Some(i) = remove {
            state.segments.remove(i);
        }
    });

    if ui.button("Add Segment").clicked() {
        state.segments.push(Tr55Segment {
            kind: Tr55SegmentKind::Sheet,
            length_ft: 100.0,
            slope: 0.01,
            n: 0.02,
            paved: false,
            p2_in: state.p2_in,
        });
    }

    if state.result_min > 0.0 {
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("Total Tc = {:.2} minutes", state.result_min))
                .strong()
                .size(16.0),
        );
    }
    if !state.worksheet_text.is_empty() {
        ui.label(
            RichText::new(&state.worksheet_text)
                .monospace()
                .size(10.0),
        );
    }
}