// SPDX-License-Identifier: GPL-3.0-or-later

//! Water quality and RDII: buildup and washoff per land use, land use
//! coverages and initial loadings per subcatchment, treatment per node,
//! unit hydrograph sets and RDII inflow per node. Part of `swmm_lid`.

use eframe::egui::{self, Id, RichText, Ui, Vec2};
use stormsewer_swmm::doc::build;
use stormsewer_swmm::doc::validate;
use stormsewer_swmm::doc::{schema, Command, InpDoc};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;
use crate::swmm_props::text_field;

use super::gw::{apply_named, fields_form, list_column, NamedDraft};
use super::{
    apply_rows, cell_value, name_combo, name_list, node_names, pick_node, pick_subcatchment,
    rows_grid, window, RowsDraft,
};

fn ok_cancel(ui: &mut Ui, dirty: bool) -> (bool, bool, bool) {
    let mut out = (false, false, false);
    ui.horizontal(|ui| {
        if ui.button("OK").clicked() {
            out.0 = true;
        }
        if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
            out.1 = true;
        }
        if ui.button("Cancel").clicked() {
            out.2 = true;
        }
    });
    out
}

// --- buildup / washoff per land use --------------------------------------------------

/// One land use's `[BUILDUP]` and `[WASHOFF]` rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QualityDraft {
    pub landuse: Option<String>,
    pub buildup: Vec<Vec<String>>,
    pub washoff: Vec<Vec<String>>,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

impl QualityDraft {
    pub fn load(doc: &InpDoc, landuse: Option<String>) -> Self {
        let (buildup, washoff) = match landuse.as_deref() {
            Some(l) => (build::rows_of(doc, "BUILDUP", l), build::rows_of(doc, "WASHOFF", l)),
            None => (Vec::new(), Vec::new()),
        };
        Self {
            landuse,
            buildup,
            washoff,
            loaded_gen: Some(doc.generation()),
            dirty: false,
        }
    }

    pub fn command(&self, doc: &InpDoc) -> Command {
        match &self.landuse {
            Some(l) => Command::Batch(vec![
                build::replace_rows(doc, "BUILDUP", l, &self.buildup),
                build::replace_rows(doc, "WASHOFF", l, &self.washoff),
            ]),
            None => Command::Batch(Vec::new()),
        }
    }
}

pub fn open_quality(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("LANDUSES").first().cloned());
    ed.lid.quality = Some(QualityDraft::load(&ed.doc, name));
}

pub fn apply_quality(ed: &mut SwmmEditor, d: &mut QualityDraft) -> bool {
    let cmd = d.command(&ed.doc);
    let label = format!("edit buildup/washoff of {}", d.landuse.clone().unwrap_or_default());
    let ok = ed.apply(cmd, &label);
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

const BUILDUP_HELP: &str = "Function: NONE; POW (power: buildup = min(C1, C2·t^C3)); EXP (exponential: C1·(1 − e^(−C2·t)), C2 in 1/days); SAT (saturation: C1·t/(C2 + t), C2 = half-saturation time in days); EXT (external: C1 = maximum buildup, C2 = scaling factor, C3 = a time series name). C1 is the maximum buildup in mass per unit; C2 the rate constant (mass/unit/day for POW). PerUnit: AREA = per acre (per hectare); CURB = per unit of curb length as entered in CurbLen. Time t in days.";
const WASHOFF_HELP: &str = "Function: NONE; EXP (exponential: washoff = C1·q^C2·B, with q the runoff rate per unit area in in/hr or mm/hr and B the buildup); RC (rating curve: C1·Q^C2, with Q the runoff rate in the flow units, mass/s); EMC (event mean concentration: C1 in mass/L, C2 unused). SweepRmvl: % of the buildup removed by street sweeping. BmpRmvl: % removed by BMPs.";

fn draw_quality(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.quality.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    window(ctx, "Buildup / Washoff", Vec2::new(860.0, 560.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            if !d.dirty && d.loaded_gen != Some(ed.doc.generation()) {
                d = QualityDraft::load(&ed.doc, d.landuse.clone());
            }
            let landuses = ed.doc.names("LANDUSES");
            let pollutants = ed.doc.names("POLLUTANTS");
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.set_max_width(180.0);
                ui.label(RichText::new("Land uses").strong());
                if let Some(pick) = name_list(ui, "swmm-bw-list", &landuses, d.landuse.as_deref(), 360.0) {
                    if d.dirty {
                        apply_quality(ed, &mut d);
                    }
                    d = QualityDraft::load(&ed.doc, Some(pick));
                }
                ui.label(RichText::new("Add land uses with Project → Land Uses…, pollutants with Project → Pollutants…").small().weak());
                let ui = &mut cols[1];
                let Some(landuse) = d.landuse.clone() else {
                    ui.label("Choose a land use.");
                    return;
                };
                egui::ScrollArea::both()
                    .id_salt("swmm-bw-scroll")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("[BUILDUP]").strong());
                        if rows_grid(ui, &ed.doc, &mut ed.lid.draft, "swmm-buildup", "BUILDUP", &mut d.buildup, 1, true, 70.0) {
                            d.dirty = true;
                        }
                        if ui
                            .add_enabled(!pollutants.is_empty(), egui::Button::new("Add buildup row"))
                            .clicked()
                        {
                            let mut r = vec![landuse.clone(), pollutants[0].clone()];
                            r.extend(build::buildup_defaults());
                            d.buildup.push(r);
                            d.dirty = true;
                        }
                        ui.label(RichText::new(BUILDUP_HELP).small().weak());
                        ui.separator();
                        ui.label(RichText::new("[WASHOFF]").strong());
                        if rows_grid(ui, &ed.doc, &mut ed.lid.draft, "swmm-washoff", "WASHOFF", &mut d.washoff, 1, true, 70.0) {
                            d.dirty = true;
                        }
                        if ui
                            .add_enabled(!pollutants.is_empty(), egui::Button::new("Add washoff row"))
                            .clicked()
                        {
                            let mut r = vec![landuse.clone(), pollutants[0].clone()];
                            r.extend(build::washoff_defaults());
                            d.washoff.push(r);
                            d.dirty = true;
                        }
                        ui.label(RichText::new(WASHOFF_HELP).small().weak());
                    });
                let (ok, apply, cancel) = ok_cancel(ui, d.dirty);
                if (ok || apply) && apply_quality(ed, &mut d) {
                    state.status = format!("Buildup/washoff of {landuse} updated");
                }
                if ok || cancel {
                    close = true;
                }
            });
        });
    if close || !open {
        state.swmm_doc.lid.quality = None;
    } else {
        state.swmm_doc.lid.quality = Some(d);
    }
}

// --- coverages and loadings (pairs per subcatchment) ---------------------------------------

/// A subcatchment's `[COVERAGES]` (land use, percent) or `[LOADINGS]`
/// (pollutant, buildup) pairs, unrolled from rows that may carry several.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PairsDraft {
    pub section: String,
    pub name: String,
    pub pairs: Vec<(String, String)>,
    pub orig: Vec<(String, String)>,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

impl PairsDraft {
    pub fn load(doc: &InpDoc, section: &str, name: &str) -> Self {
        let pairs = build::pairs_of(doc, section, name);
        Self {
            section: section.to_ascii_uppercase(),
            name: name.to_string(),
            orig: pairs.clone(),
            pairs,
            loaded_gen: Some(doc.generation()),
            dirty: false,
        }
    }

    pub fn refresh(&mut self, doc: &InpDoc) {
        if !self.dirty && self.loaded_gen != Some(doc.generation()) {
            *self = Self::load(doc, &self.section.clone(), &self.name.clone());
        }
    }

    /// One row per pair when the pairs changed; nothing otherwise, so
    /// multi-pair rows stay as written.
    pub fn command(&self, doc: &InpDoc) -> Command {
        if self.pairs == self.orig {
            return Command::Batch(Vec::new());
        }
        let rows: Vec<Vec<String>> = self
            .pairs
            .iter()
            .map(|(a, b)| vec![self.name.clone(), super::raw(a), super::raw(b.trim())])
            .collect();
        build::replace_rows(doc, &self.section, &self.name, &rows)
    }

    pub fn total(&self) -> f64 {
        self.pairs
            .iter()
            .filter_map(|(_, v)| v.trim().parse::<f64>().ok())
            .sum()
    }
}

pub fn apply_pairs(ed: &mut SwmmEditor, d: &mut PairsDraft, label: &str) -> bool {
    let cmd = d.command(&ed.doc);
    let ok = ed.apply(cmd, label);
    if ok {
        *d = PairsDraft::load(&ed.doc, &d.section.clone(), &d.name.clone());
    }
    ok
}

pub fn open_coverages(ed: &mut SwmmEditor, item: Option<&str>) {
    let Some(sub) = pick_subcatchment(ed, item) else {
        ed.lid.message = "The model has no subcatchment.".into();
        return;
    };
    ed.lid.coverages = Some(PairsDraft::load(&ed.doc, "COVERAGES", &sub));
}

pub fn open_loadings(ed: &mut SwmmEditor, item: Option<&str>) {
    let Some(sub) = pick_subcatchment(ed, item) else {
        ed.lid.message = "The model has no subcatchment.".into();
        return;
    };
    ed.lid.loadings = Some(PairsDraft::load(&ed.doc, "LOADINGS", &sub));
}

/// The pairs grid: a name combo over `names` and a number.
fn pairs_grid(
    ui: &mut Ui,
    draft: &mut Option<(Id, String)>,
    id: &str,
    d: &mut PairsDraft,
    names: &[String],
    head: (&str, &str),
) {
    let mut remove: Option<usize> = None;
    egui::Grid::new((id, "pairs"))
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new(head.0).strong());
            ui.label(RichText::new(head.1).strong());
            ui.label("");
            ui.end_row();
            for i in 0..d.pairs.len() {
                if let Some(v) = name_combo(ui, &format!("{id}-n{i}"), names, &d.pairs[i].0, 150.0) {
                    d.pairs[i].0 = v;
                    d.dirty = true;
                }
                let val = d.pairs[i].1.clone();
                if let Some(v) = text_field(ui, Id::new((id, "v", i)), &val, draft, 80.0) {
                    d.pairs[i].1 = v.trim().to_string();
                    d.dirty = true;
                }
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        d.pairs.remove(i);
        d.dirty = true;
    }
}

fn draw_pairs_dialog(
    ctx: &egui::Context,
    state: &mut AppState,
    which: &str,
) {
    let (title, section, def_section, head, add_label, help) = if which == "COVERAGES" {
        (
            "Land Use Coverages",
            "COVERAGES",
            "LANDUSES",
            ("Land use", "Percent (%)"),
            "Add land use",
            "The percent of the subcatchment's area under each land use. They may total less than 100 % (the rest generates no buildup) but not more.",
        )
    } else {
        (
            "Initial Loadings",
            "LOADINGS",
            "POLLUTANTS",
            ("Pollutant", "Buildup"),
            "Add pollutant",
            "The buildup of each pollutant on the subcatchment at the start of the run, in mass per unit area (lb/acre or kg/ha), overriding what the dry days before the start would build up.",
        )
    };
    let Some(mut d) = (if which == "COVERAGES" {
        state.swmm_doc.lid.coverages.clone()
    } else {
        state.swmm_doc.lid.loadings.clone()
    }) else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let dark = ctx.style().visuals.dark_mode;
    window(ctx, title, Vec2::new(520.0, 400.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            d.refresh(&ed.doc);
            let subs = ed.doc.names("SUBCATCHMENTS");
            let label = format!("edit {} of {}", section.to_lowercase(), d.name);
            ui.horizontal(|ui| {
                ui.label("Subcatchment");
                if let Some(pick) = name_combo(ui, &format!("swmm-{section}-sub"), &subs, &d.name, 160.0) {
                    if d.dirty {
                        apply_pairs(ed, &mut d, &label);
                    }
                    d = PairsDraft::load(&ed.doc, section, &pick);
                }
            });
            let names = ed.doc.names(def_section);
            egui::ScrollArea::vertical()
                .id_salt((section, "scroll"))
                .max_height(220.0)
                .show(ui, |ui| {
                    pairs_grid(ui, &mut ed.lid.draft, &format!("swmm-{section}"), &mut d, &names, head);
                });
            if ui
                .add_enabled(!names.is_empty(), egui::Button::new(add_label))
                .clicked()
            {
                d.pairs.push((names[0].clone(), "0".into()));
                d.dirty = true;
            }
            if which == "COVERAGES" {
                let total = d.total();
                let text = format!("total {} %", stormsewer_swmm::doc::format_number(total));
                if total > 100.0 + 1e-6 {
                    ui.label(RichText::new(format!("{text}: more than 100 %")).color(crate::theme::palette::error_text(dark)));
                } else {
                    ui.label(RichText::new(text).small());
                }
            }
            ui.label(RichText::new(help).small().weak());
            let (ok, apply, cancel) = ok_cancel(ui, d.dirty);
            if (ok || apply) && apply_pairs(ed, &mut d, &label) {
                state.status = format!("{title} of {} updated", d.name);
            }
            if ok || cancel {
                close = true;
            }
        });
    let slot = if which == "COVERAGES" {
        &mut state.swmm_doc.lid.coverages
    } else {
        &mut state.swmm_doc.lid.loadings
    };
    *slot = if close || !open { None } else { Some(d) };
}

// --- treatment per node ------------------------------------------------------------------

/// A node's `[TREATMENT]` rows as (pollutant, expression) pairs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreatmentDraft {
    pub name: String,
    pub pairs: Vec<(String, String)>,
    pub orig: Vec<Vec<String>>,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

impl TreatmentDraft {
    pub fn load(doc: &InpDoc, name: &str) -> Self {
        let orig = build::rows_of(doc, "TREATMENT", name);
        let pairs = orig
            .iter()
            .map(|r| (cell_value(r, 1), build::expression_of(r)))
            .collect();
        Self {
            name: name.to_string(),
            pairs,
            orig,
            loaded_gen: Some(doc.generation()),
            dirty: false,
        }
    }

    pub fn refresh(&mut self, doc: &InpDoc) {
        if !self.dirty && self.loaded_gen != Some(doc.generation()) {
            *self = Self::load(doc, &self.name.clone());
        }
    }

    /// The rows: an unchanged pair keeps its original fields.
    pub fn rows(&self) -> Vec<Vec<String>> {
        self.pairs
            .iter()
            .enumerate()
            .map(|(i, (p, e))| {
                if let Some(o) = self.orig.get(i) {
                    if cell_value(o, 1) == *p && build::expression_of(o) == e.trim() {
                        return o.clone();
                    }
                }
                let mut r = vec![self.name.clone(), super::raw(p)];
                r.extend(build::expression_fields(e));
                r
            })
            .collect()
    }

    pub fn command(&self, doc: &InpDoc) -> Command {
        build::replace_rows(doc, "TREATMENT", &self.name, &self.rows())
    }
}

pub fn open_treatment(ed: &mut SwmmEditor, item: Option<&str>) {
    let Some(node) = pick_node(ed, item) else {
        ed.lid.message = "The model has no node.".into();
        return;
    };
    ed.lid.treatment = Some(TreatmentDraft::load(&ed.doc, &node));
}

pub fn apply_treatment(ed: &mut SwmmEditor, d: &mut TreatmentDraft) -> bool {
    let cmd = d.command(&ed.doc);
    let ok = ed.apply(cmd, &format!("edit treatment at {}", d.name));
    if ok {
        *d = TreatmentDraft::load(&ed.doc, &d.name.clone());
    }
    ok
}

fn draw_treatment(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.treatment.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let dark = ctx.style().visuals.dark_mode;
    window(ctx, "Treatment", Vec2::new(640.0, 420.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            d.refresh(&ed.doc);
            let nodes = node_names(&ed.doc);
            ui.horizontal(|ui| {
                ui.label("Node");
                if let Some(pick) = name_combo(ui, "swmm-treat-node", &nodes, &d.name, 160.0) {
                    if d.dirty {
                        apply_treatment(ed, &mut d);
                    }
                    d = TreatmentDraft::load(&ed.doc, &pick);
                }
            });
            let pollutants = ed.doc.names("POLLUTANTS");
            let mut remove: Option<usize> = None;
            egui::ScrollArea::vertical()
                .id_salt("swmm-treat-scroll")
                .max_height(220.0)
                .show(ui, |ui| {
                    egui::Grid::new("swmm-treat-grid")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Pollutant").strong());
                            ui.label(RichText::new("Expression (R = … or C = …)").strong());
                            ui.label("");
                            ui.end_row();
                            for i in 0..d.pairs.len() {
                                if let Some(v) = name_combo(ui, &format!("swmm-treat-p{i}"), &pollutants, &d.pairs[i].0, 120.0) {
                                    d.pairs[i].0 = v;
                                    d.dirty = true;
                                }
                                let e = d.pairs[i].1.clone();
                                if let Some(v) = text_field(ui, Id::new(("swmm-treat-e", i)), &e, &mut ed.lid.draft, 320.0) {
                                    d.pairs[i].1 = v;
                                    d.dirty = true;
                                }
                                if ui.small_button("×").clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                                if let Err(msg) = validate::check_treatment_expression(&d.pairs[i].1, &pollutants) {
                                    ui.label("");
                                    ui.label(RichText::new(msg).color(crate::theme::palette::error_text(dark)).small());
                                    ui.label("");
                                    ui.end_row();
                                }
                            }
                        });
                });
            if let Some(i) = remove {
                d.pairs.remove(i);
                d.dirty = true;
            }
            if ui
                .add_enabled(!pollutants.is_empty(), egui::Button::new("Add treatment"))
                .clicked()
            {
                d.pairs.push((pollutants[0].clone(), "R = 0".into()));
                d.dirty = true;
            }
            ui.label(
                RichText::new(
                    "R = fraction removed (0–1); C = outlet concentration. Variables: HRT (residence time, hours), DT (time step, s), FLOW (in the flow units), DEPTH, AREA, any pollutant's inflow concentration by name, R_<pollutant> for another pollutant's removal; functions of the engine's parser (exp, log, sqrt, step, …). Example: R = 1 − exp(−0.5 · HRT), written with * for multiplication.",
                )
                .small()
                .weak(),
            );
            let (ok, apply, cancel) = ok_cancel(ui, d.dirty);
            if (ok || apply) && apply_treatment(ed, &mut d) {
                state.status = format!("Treatment at {} updated", d.name);
            }
            if ok || cancel {
                close = true;
            }
        });
    if close || !open {
        state.swmm_doc.lid.treatment = None;
    } else {
        state.swmm_doc.lid.treatment = Some(d);
    }
}

// --- unit hydrographs ---------------------------------------------------------------------

pub type HydrographsDraft = NamedDraft;

pub fn open_hydrographs(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("HYDROGRAPHS").first().cloned());
    ed.lid.hydrographs = Some(NamedDraft::load(&ed.doc, "HYDROGRAPHS", name));
}

const UH_REFS: &[(&str, usize)] = &[("RDII", 1)];

/// The response ratios summed per month over the draft's rows.
pub fn ratio_totals(rows: &[Vec<String>]) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = Vec::new();
    for r in rows.iter().filter(|r| r.len() > 2) {
        let month = cell_value(r, 1).to_ascii_uppercase();
        let v: f64 = cell_value(r, 3).parse().unwrap_or(0.0);
        match out.iter_mut().find(|(m, _)| *m == month) {
            Some((_, t)) => *t += v,
            None => out.push((month, v)),
        }
    }
    out
}

fn draw_hydrographs(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.hydrographs.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let dark = ctx.style().visuals.dark_mode;
    window(ctx, "Unit Hydrographs", Vec2::new(860.0, 540.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            d.refresh(&ed.doc);
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.set_max_width(200.0);
                list_column(
                    ui,
                    ed,
                    &mut d,
                    "swmm-uh-list",
                    "unit hydrograph set",
                    &build::new_hydrograph,
                    UH_REFS,
                    "edit unit hydrographs",
                );
                let ui = &mut cols[1];
                let Some(name) = d.name.clone() else {
                    ui.label("Choose a unit hydrograph set, or + to add one.");
                    return;
                };
                let gages = ed.doc.names("RAINGAGES");
                let gage_at = d.rows.iter().position(|r| r.len() == 2);
                let gage = gage_at.map(|i| cell_value(&d.rows[i], 1)).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("Rain gage");
                    if let Some(pick) = name_combo(ui, "swmm-uh-gage", &gages, &gage, 160.0) {
                        match gage_at {
                            Some(i) => d.rows[i][1] = pick,
                            None => d.rows.insert(0, vec![name.clone(), pick]),
                        }
                        d.dirty = true;
                    }
                });
                egui::ScrollArea::both()
                    .id_salt("swmm-uh-scroll")
                    .max_height(330.0)
                    .show(ui, |ui| {
                        let mut params: Vec<Vec<String>> = d.rows.iter().filter(|r| r.len() != 2).cloned().collect();
                        if rows_grid(ui, &ed.doc, &mut ed.lid.draft, "swmm-uh", "HYDROGRAPHS", &mut params, 1, true, 64.0) {
                            let mut rows: Vec<Vec<String>> = d.rows.iter().filter(|r| r.len() == 2).cloned().collect();
                            rows.extend(params);
                            d.rows = rows;
                            d.dirty = true;
                        }
                        if ui.button("Add hydrograph row").clicked() {
                            let s = |v: &str| v.to_string();
                            d.rows.push(vec![name.clone(), s("ALL"), s("SHORT"), s("0"), s("1"), s("2")]);
                            d.dirty = true;
                        }
                        for (month, total) in ratio_totals(&d.rows) {
                            if total > 1.01 {
                                ui.label(
                                    RichText::new(format!("{month}: the response ratios sum to {}; at most 1 (ERROR 153)", stormsewer_swmm::doc::format_number(total)))
                                        .color(crate::theme::palette::error_text(dark))
                                        .small(),
                                );
                            }
                        }
                        ui.label(
                            RichText::new(
                                "Month: ALL or a month (JAN … DEC); a month's row overrides ALL. Response: SHORT, MEDIUM or LONG term. R: fraction of the rainfall that becomes RDII through this hydrograph (the three sum to at most 1). T: time to peak, hours. K: recession-limb to rising-limb ratio. Dmax, Drec, D0 (optional): initial abstraction maximum depth, recovery rate per day, and initial depth.",
                            )
                            .small()
                            .weak(),
                        );
                    });
                let (ok, apply, cancel) = ok_cancel(ui, d.dirty);
                if (ok || apply) && apply_named(ed, &mut d, "edit unit hydrographs") {
                    state.status = format!("Unit hydrographs {name} updated");
                }
                if ok || cancel {
                    close = true;
                }
                if !ed.lid.message.is_empty() {
                    ui.label(RichText::new(&ed.lid.message).small());
                }
            });
        });
    if close || !open {
        state.swmm_doc.lid.hydrographs = None;
        state.swmm_doc.lid.message.clear();
    } else {
        state.swmm_doc.lid.hydrographs = Some(d);
    }
}

// --- RDII per node --------------------------------------------------------------------------

pub fn open_rdii(ed: &mut SwmmEditor, item: Option<&str>) {
    let Some(node) = pick_node(ed, item) else {
        ed.lid.message = "The model has no node.".into();
        return;
    };
    ed.lid.rdii = Some(RowsDraft::load(&ed.doc, "RDII", &node));
}

fn draw_rdii(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.rdii.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    window(ctx, "RDII Inflow", Vec2::new(460.0, 300.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            d.refresh(&ed.doc);
            let nodes = node_names(&ed.doc);
            let label = format!("edit RDII at {}", d.name);
            ui.horizontal(|ui| {
                ui.label("Node");
                if let Some(pick) = name_combo(ui, "swmm-rdii-node", &nodes, &d.name, 160.0) {
                    if d.dirty {
                        apply_rows(ed, &mut d, &label);
                    }
                    d = RowsDraft::load(&ed.doc, "RDII", &pick);
                }
            });
            let sets = ed.doc.names("HYDROGRAPHS");
            let mut on = !d.rows.is_empty();
            if ui.checkbox(&mut on, "RDII inflow at this node").changed() {
                if on {
                    let uh = sets.first().cloned().unwrap_or_else(|| "*".into());
                    d.rows = vec![vec![d.name.clone(), uh, "0".into()]];
                } else {
                    d.rows.clear();
                }
                d.dirty = true;
            }
            if sets.is_empty() {
                ui.label("The model has no unit hydrograph sets yet: Project → Unit Hydrographs…");
            }
            if let Some(row) = d.rows.first_mut() {
                if fields_form(ui, &ed.doc, &mut ed.lid.draft, "swmm-rdii", "RDII", row, 1) {
                    d.dirty = true;
                }
            }
            ui.label(
                RichText::new("UnitHydrograph: the [HYDROGRAPHS] set whose gage's rainfall drives the inflow. SewerArea: the sewershed area (acres or hectares) that contributes rainfall-derived infiltration and inflow at this node.")
                    .small()
                    .weak(),
            );
            let (ok, apply, cancel) = ok_cancel(ui, d.dirty);
            if (ok || apply) && apply_rows(ed, &mut d, &label) {
                state.status = format!("RDII at {} updated", d.name);
            }
            if ok || cancel {
                close = true;
            }
        });
    if close || !open {
        state.swmm_doc.lid.rdii = None;
    } else {
        state.swmm_doc.lid.rdii = Some(d);
    }
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    draw_quality(ctx, state);
    draw_pairs_dialog(ctx, state, "COVERAGES");
    draw_pairs_dialog(ctx, state, "LOADINGS");
    draw_treatment(ctx, state);
    draw_hydrographs(ctx, state);
    draw_rdii(ctx, state);
    let _ = schema::UH_MONTHS;
}
