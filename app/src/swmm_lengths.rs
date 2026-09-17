// SPDX-License-Identifier: GPL-3.0-or-later

//! Project → Compute Conduit Lengths…: the geometric length of every
//! conduit (start node, vertices, end node, in map units, times a
//! map-to-model factor) against the stored `Length`, with the difference,
//! and one batch that writes the ticked ones. Nothing here runs on its
//! own: drawing a conduit writes its drawn length once (that is the
//! draw), and lengths are never touched again until the user asks.

use eframe::egui::{self, Id, RichText, Vec2};
use stormsewer_swmm::doc::build::{LinkType, ObjRef};
use stormsewer_swmm::doc::{format_number, Command, InpDoc};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;
use crate::swmm_tools::polyline_length;

#[derive(Clone, Debug, PartialEq)]
pub struct LengthRow {
    pub name: String,
    pub stored: Option<f64>,
    /// Map-unit length of the drawn path.
    pub drawn: f64,
    /// `drawn × factor`, in the model's length unit.
    pub geometric: f64,
    pub diff_pct: Option<f64>,
    pub apply: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LengthsDraft {
    pub rows: Vec<LengthRow>,
    pub factor: String,
    pub factor_note: String,
    pub only_selected: bool,
    /// Only rows differing by more than this many percent are ticked.
    pub threshold: String,
    pub error: String,
}

/// The map-unit → model-length factor implied by `[MAP] Units` and
/// `FLOW_UNITS`, with the reason.
pub fn map_factor(doc: &InpDoc) -> (f64, String) {
    let map = doc
        .key_value("MAP", "UNITS")
        .map(|u| u.to_ascii_uppercase())
        .unwrap_or_default();
    let metric = matches!(
        doc.option("FLOW_UNITS")
            .map(|u| u.to_ascii_uppercase())
            .as_deref(),
        Some("CMS" | "LPS" | "MLD")
    );
    let model = if metric { "metres" } else { "feet" };
    match map.as_str() {
        "FEET" if !metric => (1.0, "map feet → model feet".into()),
        "METERS" if metric => (1.0, "map metres → model metres".into()),
        "FEET" => (0.3048, format!("map feet → model {model}")),
        "METERS" => (3.28084, format!("map metres → model {model}")),
        "DEGREES" => (
            1.0,
            "map units are degrees — not lengths; set the factor yourself".into(),
        ),
        _ => (
            1.0,
            format!("[MAP] Units not set; assuming map units are model {model}"),
        ),
    }
}

/// Every conduit's drawn length.
pub fn rows(ed: &SwmmEditor, factor: f64) -> Vec<LengthRow> {
    ed.links
        .iter()
        .filter(|l| l.kind == LinkType::Conduit && l.path.len() >= 2)
        .map(|l| {
            let stored = ed
                .doc
                .field("CONDUITS", &l.name, "Length")
                .and_then(|s| s.parse::<f64>().ok());
            let drawn = polyline_length(&l.path);
            let geometric = drawn * factor;
            let diff_pct = stored
                .filter(|s| s.abs() > 1e-9)
                .map(|s| (geometric - s) / s * 100.0);
            LengthRow {
                selected: ed.is_selected(&ObjRef::Link(l.name.clone())),
                name: l.name.clone(),
                stored,
                drawn,
                geometric,
                diff_pct,
                apply: false,
            }
        })
        .collect()
}

fn tick(d: &mut LengthsDraft) {
    let thr = d.threshold.trim().parse::<f64>().unwrap_or(0.0).abs();
    for r in &mut d.rows {
        let differs = match r.diff_pct {
            Some(p) => p.abs() > thr,
            None => true,
        };
        r.apply = differs && (!d.only_selected || r.selected);
    }
}

pub fn open(ed: &mut SwmmEditor) {
    ed.refresh();
    let (factor, note) = map_factor(&ed.doc);
    let has_selection = ed.selection.iter().any(|r| matches!(r, ObjRef::Link(_)));
    let mut d = LengthsDraft {
        rows: rows(ed, factor),
        factor: format_number(factor),
        factor_note: note,
        only_selected: has_selection,
        threshold: "0.5".into(),
        error: String::new(),
    };
    tick(&mut d);
    ed.dialogs.lengths = Some(d);
}

/// Recompute after the factor changed.
fn recompute(ed: &SwmmEditor, d: &mut LengthsDraft) {
    let Ok(f) = d.factor.trim().parse::<f64>() else {
        d.error = "factor: not a number".into();
        return;
    };
    d.error.clear();
    let ticked: Vec<(String, bool)> = d.rows.iter().map(|r| (r.name.clone(), r.apply)).collect();
    d.rows = rows(ed, f);
    for r in &mut d.rows {
        if let Some((_, a)) = ticked.iter().find(|(n, _)| n == &r.name) {
            r.apply = *a;
        }
    }
}

/// The one batch that writes every ticked row's geometric length.
pub fn command(rows: &[LengthRow]) -> Command {
    Command::Batch(
        rows.iter()
            .filter(|r| r.apply)
            .map(|r| Command::SetField {
                section: "CONDUITS".into(),
                name: r.name.clone(),
                field: "Length".into(),
                value: format_number((r.geometric * 100.0).round() / 100.0),
            })
            .collect(),
    )
}

/// Write the ticked rows; returns the status line.
pub fn apply(ed: &mut SwmmEditor) -> Option<String> {
    let d = ed.dialogs.lengths.take()?;
    let n = d.rows.iter().filter(|r| r.apply).count();
    if n == 0 {
        ed.dialogs.lengths = Some(d);
        return Some("No conduits ticked".into());
    }
    if ed.apply(command(&d.rows), &format!("set {n} conduit length(s)")) {
        Some(format!("Set {n} conduit length(s) from the map"))
    } else {
        let e = ed.last_error.clone().unwrap_or_default();
        ed.dialogs.lengths = Some(d);
        Some(e)
    }
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.lengths.clone() else {
        return;
    };
    let mut open = true;
    let mut action: Option<&str> = None;
    let size = Vec2::new(620.0, 460.0);
    egui::Window::new("Compute Conduit Lengths")
        .id(Id::new(("swmm-dialog", "lengths")))
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .default_pos(ctx.screen_rect().center() - size / 2.0)
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &state.swmm_doc;
            ui.horizontal(|ui| {
                ui.label("Map units × ");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut d.factor)
                            .id(Id::new("swmm-lengths-factor"))
                            .desired_width(70.0),
                    )
                    .changed()
                {
                    recompute(ed, &mut d);
                }
                ui.label(RichText::new(&d.factor_note).small());
            });
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut d.only_selected, "Selected conduits only")
                    .changed()
                {
                    tick(&mut d);
                }
                ui.label("Tick where |diff| >");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut d.threshold)
                            .id(Id::new("swmm-lengths-threshold"))
                            .desired_width(40.0),
                    )
                    .changed()
                {
                    tick(&mut d);
                }
                ui.label("%");
                if ui.small_button("All").clicked() {
                    for r in &mut d.rows {
                        r.apply = !d.only_selected || r.selected;
                    }
                }
                if ui.small_button("None").clicked() {
                    for r in &mut d.rows {
                        r.apply = false;
                    }
                }
            });
            if !d.error.is_empty() {
                ui.label(
                    RichText::new(&d.error)
                        .color(crate::theme::palette::error_text(ui.visuals().dark_mode)),
                );
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("swmm-lengths-table")
                .max_height(300.0)
                .show(ui, |ui| {
                    egui::Grid::new("swmm-lengths-grid")
                        .num_columns(6)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Apply").strong());
                            ui.label(RichText::new("Conduit").strong());
                            ui.label(RichText::new("Stored").strong());
                            ui.label(RichText::new("Drawn (map)").strong());
                            ui.label(RichText::new("Geometric").strong());
                            ui.label(RichText::new("Diff %").strong());
                            ui.end_row();
                            for (i, r) in d.rows.iter_mut().enumerate() {
                                let enabled = !d.only_selected || r.selected;
                                ui.add_enabled(enabled, egui::Checkbox::without_text(&mut r.apply))
                                    .on_hover_text(if enabled { "" } else { "not selected" });
                                let _ = i;
                                ui.label(if r.selected {
                                    RichText::new(&r.name).strong()
                                } else {
                                    RichText::new(&r.name)
                                });
                                ui.label(r.stored.map(format_number).unwrap_or_else(|| "—".into()));
                                ui.label(format!("{:.2}", r.drawn));
                                ui.label(format!("{:.2}", r.geometric));
                                ui.label(match r.diff_pct {
                                    Some(p) => format!("{p:+.1}"),
                                    None => "—".into(),
                                });
                                ui.end_row();
                            }
                        });
                    if d.rows.is_empty() {
                        ui.label(RichText::new("No conduit has both ends on the map.").weak());
                    }
                });
            ui.separator();
            let n = d.rows.iter().filter(|r| r.apply).count();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(n > 0, egui::Button::new(format!("Apply to {n} ticked")))
                    .clicked()
                {
                    action = Some("apply");
                }
                if ui.button("Close").clicked() {
                    action = Some("close");
                }
                ui.label(RichText::new("One undo step for all of them.").small());
            });
        });
    let ed = &mut state.swmm_doc;
    match action {
        Some("apply") => {
            ed.dialogs.lengths = Some(d);
            if let Some(s) = apply(ed) {
                state.status = s;
            }
        }
        Some("close") => ed.dialogs.lengths = None,
        _ => ed.dialogs.lengths = if open { Some(d) } else { None },
    }
}

/// The status-bar note while a link is being drawn: its length so far.
pub fn drawing_note(ed: &SwmmEditor) -> Option<String> {
    let from = ed.edit.link_from.as_ref()?;
    let n = ed.node(from)?;
    let mut pts = vec![(n.x, n.y)];
    pts.extend(ed.edit.link_vertices.iter().copied());
    if let Some(c) = ed.edit.cursor_world {
        pts.push(c);
    }
    let (factor, _) = map_factor(&ed.doc);
    let len = polyline_length(&pts) * factor;
    Some(format!("drawn length {len:.1}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pond() -> SwmmEditor {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../swmm/tests/fixtures/epa-samples/Detention_Pond_Model.inp");
        let mut ed = SwmmEditor::default();
        ed.open_path(&path).unwrap();
        ed.refresh();
        ed
    }

    #[test]
    fn pond_fixture_lengths_preview_and_apply_all_in_one_step() {
        let mut ed = pond();
        let (f, note) = map_factor(&ed.doc);
        assert_eq!(f, 1.0, "{note}");
        open(&mut ed);
        let d = ed.dialogs.lengths.clone().unwrap();
        assert!(!d.only_selected);
        assert_eq!(d.rows.len(), 12, "every conduit");
        let c1 = d.rows.iter().find(|r| r.name == "C1").unwrap();
        assert_eq!(c1.stored, Some(185.0));
        assert!((c1.geometric - 185.4).abs() < 0.5, "{}", c1.geometric);
        assert!(c1.diff_pct.unwrap().abs() < 1.0);
        let out = d.rows.iter().find(|r| r.name == "C_out").unwrap();
        assert!((out.geometric - 168.07).abs() < 0.05, "{}", out.geometric);
        assert!(out.apply, "differs by more than the threshold");
        // Tick everything and apply.
        let mut d = d;
        for r in &mut d.rows {
            r.apply = true;
        }
        ed.dialogs.lengths = Some(d);
        let depth = ed.undo_depth();
        let s = apply(&mut ed).unwrap();
        assert!(s.contains("12"), "{s}");
        assert_eq!(ed.undo_depth(), depth + 1, "one batch");
        assert_eq!(ed.doc.field("CONDUITS", "C_out", "Length"), Some("168.07"));
        assert_eq!(ed.doc.field("CONDUITS", "C1", "Length"), Some("185.39"));
        assert!(ed.undo().is_some());
        assert_eq!(ed.doc.field("CONDUITS", "C_out", "Length"), Some("100"));
        assert!(ed.dialogs.lengths.is_none());
    }

    #[test]
    fn selection_limits_the_ticks_and_the_factor_follows_the_units() {
        let mut ed = pond();
        ed.select_only(ObjRef::Link("C_out".into()));
        open(&mut ed);
        let d = ed.dialogs.lengths.clone().unwrap();
        assert!(d.only_selected);
        assert_eq!(
            d.rows
                .iter()
                .filter(|r| r.apply)
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            vec!["C_out"]
        );
        // Map in metres, model in feet.
        ed.apply(stormsewer_swmm::backdrop::set_map_units("Meters"), "units");
        let (f, note) = map_factor(&ed.doc);
        assert!((f - 3.28084).abs() < 1e-6, "{note}");
        ed.apply(
            Command::SetOption {
                section: "OPTIONS".into(),
                key: "FLOW_UNITS".into(),
                value: "CMS".into(),
            },
            "si",
        );
        assert_eq!(map_factor(&ed.doc).0, 1.0);
        ed.apply(stormsewer_swmm::backdrop::set_map_units("Degrees"), "units");
        assert!(map_factor(&ed.doc).1.contains("degrees"));
        // The status note while drawing.
        ed.edit.link_from = Some("J1".into());
        ed.edit.cursor_world = Some((648.532, 1043.713 + 50.0));
        assert_eq!(drawing_note(&ed).as_deref(), Some("drawn length 50.0"));
    }
}
