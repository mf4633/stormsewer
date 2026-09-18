// SPDX-License-Identifier: GPL-3.0-or-later

//! Aquifers, groundwater (with the `[GWF]` flow expressions) and snow
//! packs (with the `[TEMPERATURE]` and `[ADJUSTMENTS]` text). Part of
//! `swmm_lid`; see that module for the draft model.

use eframe::egui::{self, Id, RichText, Ui, Vec2};
use stormsewer_swmm::doc::build;
use stormsewer_swmm::doc::validate;
use stormsewer_swmm::doc::{schema, Command, InpDoc};

use crate::state::AppState;
use crate::swmm_dialogs::{section_text, set_section_text};
use crate::swmm_doc::SwmmEditor;
use crate::swmm_props::text_field;

use super::{
    apply_rows, cell, cell_value, header, name_combo, name_list, node_names, pick_subcatchment,
    row_columns, rows_grid, set_cell, window, RowsDraft,
};

// --- a draft of one named object among many ------------------------------------------

/// The draft of a list dialog: the chosen name and every row of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedDraft {
    pub section: String,
    pub name: Option<String>,
    pub rows: Vec<Vec<String>>,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

impl NamedDraft {
    pub fn load(doc: &InpDoc, section: &str, name: Option<String>) -> Self {
        let rows = name
            .as_deref()
            .map(|n| build::rows_of(doc, section, n))
            .unwrap_or_default();
        Self {
            section: section.to_ascii_uppercase(),
            name,
            rows,
            loaded_gen: Some(doc.generation()),
            dirty: false,
        }
    }

    pub fn refresh(&mut self, doc: &InpDoc) {
        if !self.dirty && self.loaded_gen != Some(doc.generation()) {
            *self = Self::load(doc, &self.section, self.name.clone());
        }
    }

    pub fn command(&self, doc: &InpDoc) -> Command {
        match &self.name {
            Some(n) => build::replace_rows(doc, &self.section, n, &self.rows),
            None => Command::Batch(Vec::new()),
        }
    }
}

pub type AquifersDraft = NamedDraft;

/// Apply a named draft as one step (none when nothing changed).
pub fn apply_named(ed: &mut SwmmEditor, d: &mut NamedDraft, label: &str) -> bool {
    let cmd = d.command(&ed.doc);
    let ok = ed.apply(cmd, label);
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

/// Rename a named object in its section and in the columns that refer to
/// it. Refused when the new name is blank or taken.
pub fn rename_named(
    ed: &mut SwmmEditor,
    section: &str,
    targets: &[(&str, usize)],
    old: &str,
    new: &str,
) -> bool {
    let new = new.trim();
    if new.is_empty() || new.contains(char::is_whitespace) {
        ed.last_error = Some("a name has no blanks".into());
        return false;
    }
    if !new.eq_ignore_ascii_case(old) && ed.doc.contains(section, new) {
        ed.last_error = Some(format!("[{section}] already has {new}"));
        return false;
    }
    let cmd = build::rename_in_columns(&ed.doc, targets, old, new);
    ed.apply(cmd, &format!("rename {old} to {new}"))
}

/// The batch that deletes a named object and the rows in `cascade`
/// (section, column) that name it.
pub fn delete_named(doc: &InpDoc, section: &str, name: &str, cascade: &[(&str, usize)]) -> Command {
    let mut cmds = vec![Command::DeleteObject {
        section: section.into(),
        name: name.into(),
    }];
    for (sec, idx) in cascade {
        let lines: Vec<usize> = doc
            .rows(sec)
            .into_iter()
            .filter(|(_, r)| r.value(*idx).is_some_and(|v| v.eq_ignore_ascii_case(name)))
            .map(|(li, _)| li)
            .collect();
        for li in lines.into_iter().rev() {
            cmds.push(Command::DeleteLine {
                section: sec.to_string(),
                line: li,
            });
        }
    }
    Command::Batch(cmds)
}

/// The left column of a list dialog: `+`, `−`, the list and the name
/// field. `refs` are the `(section, column)` places the name is used, for
/// renames and the delete cascade.
#[allow(clippy::too_many_arguments)]
pub fn list_column(
    ui: &mut Ui,
    ed: &mut SwmmEditor,
    d: &mut NamedDraft,
    id: &str,
    what: &str,
    new_object: &dyn Fn(&InpDoc) -> build::NewObject,
    refs: &[(&str, usize)],
    label: &str,
) {
    let section = d.section.clone();
    let names = ed.doc.names(&section);
    ui.horizontal(|ui| {
        if ui.button("+").on_hover_text(format!("Add {what}")).clicked() {
            let o = new_object(&ed.doc);
            let n = o.name.clone();
            if ed.apply(o.command, &format!("add {what} {n}")) {
                *d = NamedDraft::load(&ed.doc, &section, Some(n));
            }
        }
        if ui
            .add_enabled(d.name.is_some(), egui::Button::new("−"))
            .on_hover_text(format!("Delete the chosen {what} and the rows that use it"))
            .clicked()
        {
            if let Some(n) = d.name.clone() {
                if ed.apply(delete_named(&ed.doc, &section, &n, refs), &format!("delete {what} {n}")) {
                    *d = NamedDraft::load(&ed.doc, &section, None);
                }
            }
        }
    });
    if let Some(pick) = name_list(ui, id, &names, d.name.as_deref(), 300.0) {
        if d.dirty {
            apply_named(ed, d, label);
        }
        *d = NamedDraft::load(&ed.doc, &section, Some(pick));
    }
    if let Some(name) = d.name.clone() {
        ui.horizontal(|ui| {
            ui.label("Name");
            let fid = Id::new((id, "name"));
            if let Some(v) = text_field(ui, fid, &name, &mut ed.lid.draft, 120.0) {
                let mut targets: Vec<(&str, usize)> = vec![(section.as_str(), 0)];
                targets.extend_from_slice(refs);
                if rename_named(ed, &section, &targets, &name, &v) {
                    *d = NamedDraft::load(&ed.doc, &section, Some(v.trim().to_string()));
                } else {
                    ed.lid.message = ed.last_error.take().unwrap_or_default();
                }
            }
        });
    }
}

/// A vertical form over one row: a label with unit and a widget per
/// column from `skip` on. Returns whether a cell changed.
pub fn fields_form(
    ui: &mut Ui,
    doc: &InpDoc,
    draft: &mut Option<(Id, String)>,
    id: &str,
    section: &str,
    row: &mut Vec<String>,
    skip: usize,
) -> bool {
    let mut changed = false;
    let cols = row_columns(doc, section, row);
    egui::Grid::new((id, "form"))
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (i, col) in cols.iter().enumerate().skip(skip) {
                ui.label(header(doc, section, col));
                let value = cell_value(row, i);
                let wid = Id::new((id, *col));
                if let Some(v) = cell(ui, doc, draft, wid, section, col, &value, 150.0) {
                    set_cell(row, cols, section, i, &v);
                    changed = true;
                }
                ui.end_row();
            }
        });
    changed
}

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

// --- aquifers ---------------------------------------------------------------------------

pub fn open_aquifers(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("AQUIFERS").first().cloned());
    ed.lid.aquifers = Some(NamedDraft::load(&ed.doc, "AQUIFERS", name));
}

const AQUIFER_REFS: &[(&str, usize)] = &[("GROUNDWATER", 1)];

fn draw_aquifers(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.aquifers.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    window(ctx, "Aquifers", Vec2::new(640.0, 520.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            d.refresh(&ed.doc);
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                list_column(
                    ui,
                    ed,
                    &mut d,
                    "swmm-aq-list",
                    "aquifer",
                    &build::new_aquifer,
                    AQUIFER_REFS,
                    "edit aquifer",
                );
                let ui = &mut cols[1];
                let Some(name) = d.name.clone() else {
                    ui.label("Choose an aquifer, or + to add one.");
                    return;
                };
                if d.rows.is_empty() {
                    let mut r = vec![name.clone()];
                    r.extend(build::aquifer_defaults());
                    d.rows.push(r);
                }
                egui::ScrollArea::vertical()
                    .id_salt("swmm-aq-scroll")
                    .max_height(380.0)
                    .show(ui, |ui| {
                        if fields_form(ui, &ed.doc, &mut ed.lid.draft, "swmm-aq", "AQUIFERS", &mut d.rows[0], 1) {
                            d.dirty = true;
                        }
                        ui.label(
                            RichText::new(
                                "Por: porosity. WP: wilting point. FC: field capacity (WP < FC < Por). Ksat: saturated conductivity. Kslope: slope of log(conductivity) against moisture deficit. Tslope: slope of soil tension against moisture content. ETu: fraction of evaporation taken from the upper zone. ETs: depth below the surface where lower-zone evaporation stops. Seep: seepage rate to deep groundwater at full saturation. Ebot: aquifer bottom elevation. Egw: initial water table elevation. Umc: initial upper-zone moisture content. ETupat: a monthly pattern scaling the upper-zone evaporation (optional).",
                            )
                            .small()
                            .weak(),
                        );
                    });
                let (ok, apply, cancel) = ok_cancel(ui, d.dirty);
                if ok || apply {
                    if apply_named(ed, &mut d, "edit aquifer") {
                        state.status = format!("Aquifer {name} updated");
                    }
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
        state.swmm_doc.lid.aquifers = None;
        state.swmm_doc.lid.message.clear();
    } else {
        state.swmm_doc.lid.aquifers = Some(d);
    }
}

// --- groundwater ------------------------------------------------------------------------

/// A subcatchment's `[GROUNDWATER]` row (at most one) and its `[GWF]`
/// expressions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroundwaterDraft {
    pub name: String,
    pub gw: Vec<Vec<String>>,
    /// The `[GWF]` rows as read, for the ones left unchanged.
    pub gwf_orig: Vec<Vec<String>>,
    pub lateral: String,
    pub deep: String,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

fn gwf_kind(row: &[String]) -> Option<&'static str> {
    let k = row.get(1)?.to_ascii_uppercase();
    if k.starts_with("LAT") {
        Some("LATERAL")
    } else if k == "DEEP" {
        Some("DEEP")
    } else {
        None
    }
}

impl GroundwaterDraft {
    pub fn load(doc: &InpDoc, name: &str) -> Self {
        let gwf_orig = build::rows_of(doc, "GWF", name);
        let expr = |kind: &str| {
            gwf_orig
                .iter()
                .find(|r| gwf_kind(r) == Some(kind))
                .map(|r| build::expression_of(r))
                .unwrap_or_default()
        };
        Self {
            name: name.to_string(),
            gw: build::rows_of(doc, "GROUNDWATER", name),
            lateral: expr("LATERAL"),
            deep: expr("DEEP"),
            gwf_orig,
            loaded_gen: Some(doc.generation()),
            dirty: false,
        }
    }

    pub fn refresh(&mut self, doc: &InpDoc) {
        if !self.dirty && self.loaded_gen != Some(doc.generation()) {
            *self = Self::load(doc, &self.name.clone());
        }
    }

    /// The `[GWF]` rows the draft stands for: unchanged expressions keep
    /// their original row, changed ones are re-tokenised, cleared ones go,
    /// new ones come after.
    pub fn gwf_rows(&self) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for r in &self.gwf_orig {
            match gwf_kind(r) {
                Some(k) if !seen.contains(&k) => {
                    seen.push(k);
                    let text = if k == "LATERAL" { &self.lateral } else { &self.deep };
                    if text.trim().is_empty() {
                        continue;
                    }
                    if build::expression_of(r) == text.trim() {
                        out.push(r.clone());
                    } else {
                        let mut row = vec![self.name.clone(), r[1].clone()];
                        row.extend(build::expression_fields(text));
                        out.push(row);
                    }
                }
                _ => out.push(r.clone()),
            }
        }
        for (k, text) in [("LATERAL", &self.lateral), ("DEEP", &self.deep)] {
            if !seen.contains(&k) && !text.trim().is_empty() {
                let mut row = vec![self.name.clone(), k.to_string()];
                row.extend(build::expression_fields(text));
                out.push(row);
            }
        }
        out
    }

    pub fn command(&self, doc: &InpDoc) -> Command {
        Command::Batch(vec![
            build::replace_rows(doc, "GROUNDWATER", &self.name, &self.gw),
            build::replace_rows(doc, "GWF", &self.name, &self.gwf_rows()),
        ])
    }
}

pub fn open_groundwater(ed: &mut SwmmEditor, item: Option<&str>) {
    let Some(sub) = pick_subcatchment(ed, item) else {
        ed.lid.message = "The model has no subcatchment.".into();
        return;
    };
    ed.lid.groundwater = Some(GroundwaterDraft::load(&ed.doc, &sub));
}

pub fn apply_groundwater(ed: &mut SwmmEditor, d: &mut GroundwaterDraft) -> bool {
    let cmd = d.command(&ed.doc);
    let ok = ed.apply(cmd, &format!("edit groundwater of {}", d.name));
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

fn expression_check(ui: &mut Ui, dark: bool, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let unknown = validate::unknown_gwf_identifiers(text);
    if unknown.is_empty() {
        ui.label(RichText::new("variables OK").small().weak());
    } else {
        ui.label(
            RichText::new(format!("unknown: {}", unknown.join(", ")))
                .color(crate::theme::palette::error_text(dark))
                .small(),
        );
    }
}

fn draw_groundwater(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.groundwater.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let dark = ctx.style().visuals.dark_mode;
    window(ctx, "Groundwater", Vec2::new(620.0, 600.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            d.refresh(&ed.doc);
            let subs = ed.doc.names("SUBCATCHMENTS");
            ui.horizontal(|ui| {
                ui.label("Subcatchment");
                if let Some(pick) = name_combo(ui, "swmm-gw-sub", &subs, &d.name, 160.0) {
                    if d.dirty {
                        apply_groundwater(ed, &mut d);
                    }
                    d = GroundwaterDraft::load(&ed.doc, &pick);
                }
            });
            let aquifers = ed.doc.names("AQUIFERS");
            let mut on = !d.gw.is_empty();
            if ui
                .checkbox(&mut on, "Groundwater flow from this subcatchment")
                .changed()
            {
                if on {
                    let aq = aquifers.first().cloned().unwrap_or_else(|| "*".into());
                    let node = ed
                        .doc
                        .field("SUBCATCHMENTS", &d.name, "Outlet")
                        .map(str::to_string)
                        .unwrap_or_else(|| "*".into());
                    let mut r = vec![d.name.clone()];
                    r.extend(build::groundwater_defaults(&aq, &node));
                    d.gw = vec![r];
                } else {
                    d.gw.clear();
                }
                d.dirty = true;
            }
            if aquifers.is_empty() {
                ui.label("The model has no aquifers yet: Project → Aquifers…");
            }
            egui::ScrollArea::vertical()
                .id_salt("swmm-gw-scroll")
                .max_height(440.0)
                .show(ui, |ui| {
                    if let Some(row) = d.gw.first_mut() {
                        if fields_form(ui, &ed.doc, &mut ed.lid.draft, "swmm-gw", "GROUNDWATER", row, 1) {
                            d.dirty = true;
                        }
                        ui.label(
                            RichText::new(
                                "Aquifer: the [AQUIFERS] row. Node: the node receiving the groundwater flow. Esurf: ground surface elevation. Flow = A1·(Hgw − Hcb)^B1 − A2·(Hsw − Hcb)^B2 + A3·Hgw·Hsw, with Hgw the water table height, Hsw the surface water depth and Hcb the channel bottom, all above the aquifer bottom. Dsw: a fixed surface water depth (0 = use the node's depth). Egwt, Ebot, Wgr, Umc (optional): the node's water elevation threshold, aquifer bottom, initial water table and initial upper moisture for this subcatchment, overriding the aquifer's; leave blank or * to inherit.",
                            )
                            .small()
                            .weak(),
                        );
                    }
                    ui.separator();
                    ui.label(RichText::new("[GWF] custom flow expressions (optional)").strong());
                    ui.label(RichText::new("LATERAL replaces the lateral flow equation, DEEP the deep percolation. Variables: Hgw, Hsw, Hcb, Hgs, Ks, K, Theta, Phi, Fi, Fu, A; functions: sin cos tan abs sgn sqrt log exp log10 step and the rest of the engine's parser. Blank means the built-in equation.").small().weak());
                    ui.label("LATERAL");
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut d.lateral)
                                .id(Id::new("swmm-gwf-lateral"))
                                .desired_rows(2)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        )
                        .changed()
                    {
                        d.dirty = true;
                    }
                    expression_check(ui, dark, &d.lateral);
                    ui.label("DEEP");
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut d.deep)
                                .id(Id::new("swmm-gwf-deep"))
                                .desired_rows(2)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        )
                        .changed()
                    {
                        d.dirty = true;
                    }
                    expression_check(ui, dark, &d.deep);
                });
            let (ok, apply, cancel) = ok_cancel(ui, d.dirty);
            if ok || apply {
                if apply_groundwater(ed, &mut d) {
                    state.status = format!("Groundwater of {} updated", d.name);
                }
            }
            if ok || cancel {
                close = true;
            }
        });
    if close || !open {
        state.swmm_doc.lid.groundwater = None;
    } else {
        state.swmm_doc.lid.groundwater = Some(d);
    }
}

// --- snow packs ---------------------------------------------------------------------------

/// A snow pack's rows plus the climatology text sections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnowDraft {
    pub packs: NamedDraft,
    pub temperature: String,
    pub adjustments: String,
    pub temperature_orig: String,
    pub adjustments_orig: String,
}

impl SnowDraft {
    pub fn load(doc: &InpDoc, name: Option<String>) -> Self {
        let temperature = section_text(doc, "TEMPERATURE");
        let adjustments = section_text(doc, "ADJUSTMENTS");
        Self {
            packs: NamedDraft::load(doc, "SNOWPACKS", name),
            temperature_orig: temperature.clone(),
            adjustments_orig: adjustments.clone(),
            temperature,
            adjustments,
        }
    }

    pub fn dirty(&self) -> bool {
        self.packs.dirty
            || self.temperature != self.temperature_orig
            || self.adjustments != self.adjustments_orig
    }

    pub fn command(&self, doc: &InpDoc) -> Command {
        let mut cmds = vec![self.packs.command(doc)];
        if self.temperature != self.temperature_orig {
            cmds.push(set_section_text(doc, "TEMPERATURE", &self.temperature));
        }
        if self.adjustments != self.adjustments_orig {
            cmds.push(set_section_text(doc, "ADJUSTMENTS", &self.adjustments));
        }
        Command::Batch(cmds)
    }
}

pub fn open_snowpacks(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("SNOWPACKS").first().cloned());
    ed.lid.snowpacks = Some(SnowDraft::load(&ed.doc, name));
}

pub fn apply_snowpacks(ed: &mut SwmmEditor, d: &mut SnowDraft) -> bool {
    let cmd = d.command(&ed.doc);
    let ok = ed.apply(cmd, "edit snow packs");
    if ok {
        let name = d.packs.name.clone();
        *d = SnowDraft::load(&ed.doc, name);
    }
    ok
}

const SNOW_REFS: &[(&str, usize)] = &[("SUBCATCHMENTS", 8)];

fn snow_help(layer: &str) -> &'static str {
    match layer {
        "PLOWABLE" => "The plowable fraction of the impervious area. Cmin / Cmax: melt coefficients on Dec 21 / Jun 21 (in/hr-°F or mm/hr-°C). Tbase: temperature below which no melt occurs. FWF: free water holding capacity as a fraction of snow depth. SD0: initial snow depth (water equivalent). FW0: initial free water. SNN0: the fraction of the impervious area that is plowable.",
        "IMPERVIOUS" | "PERVIOUS" => "Cmin / Cmax: melt coefficients on Dec 21 / Jun 21. Tbase: no melt below this temperature. FWF: free water fraction. SD0: initial snow depth. FW0: initial free water. SD100: the depth above which the whole area is snow-covered (the areal depletion curve applies below it).",
        "REMOVAL" => "Dplow: depth at which plowing starts. Fout: fraction of removed snow sent out of the watershed. Fimp: fraction moved to the impervious area. Fperv: to the pervious area. Fimelt: converted to immediate melt. Fsub: moved to another subcatchment, named in Scatch. Fractions sum to at most 1.",
        _ => "",
    }
}

fn draw_snowpacks(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.snowpacks.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    window(ctx, "Snow Packs", Vec2::new(860.0, 600.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            if !d.dirty() && d.packs.loaded_gen != Some(ed.doc.generation()) {
                d = SnowDraft::load(&ed.doc, d.packs.name.clone());
            }
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.set_max_width(200.0);
                list_column(
                    ui,
                    ed,
                    &mut d.packs,
                    "swmm-snow-list",
                    "snow pack",
                    &build::new_snowpack,
                    SNOW_REFS,
                    "edit snow pack",
                );
                let used = d
                    .packs
                    .name
                    .as_deref()
                    .map(|n| {
                        ed.doc
                            .rows("SUBCATCHMENTS")
                            .into_iter()
                            .filter(|(_, r)| r.value(8).is_some_and(|p| p.eq_ignore_ascii_case(n)))
                            .count()
                    })
                    .unwrap_or(0);
                if d.packs.name.is_some() {
                    ui.label(RichText::new(format!("assigned to {used} subcatchment(s); assign one on its property sheet (SnowPack)")).small());
                }
                let ui = &mut cols[1];
                egui::ScrollArea::vertical()
                    .id_salt("swmm-snow-scroll")
                    .max_height(440.0)
                    .show(ui, |ui| {
                        if let Some(name) = d.packs.name.clone() {
                            for layer in schema::SNOWPACK_LAYERS {
                                let at = d
                                    .packs
                                    .rows
                                    .iter()
                                    .position(|r| r.get(1).is_some_and(|l| l.eq_ignore_ascii_case(layer)));
                                let mut on = at.is_some();
                                ui.horizontal(|ui| {
                                    if ui.checkbox(&mut on, "").changed() {
                                        match at {
                                            Some(i) if !on => {
                                                d.packs.rows.remove(i);
                                            }
                                            None if on => {
                                                let mut r = vec![name.clone(), layer.to_string()];
                                                r.extend(build::snowpack_defaults(layer));
                                                d.packs.rows.push(r);
                                            }
                                            _ => {}
                                        }
                                        d.packs.dirty = true;
                                    }
                                    ui.label(RichText::new(*layer).strong());
                                });
                                if let Some(i) = d
                                    .packs
                                    .rows
                                    .iter()
                                    .position(|r| r.get(1).is_some_and(|l| l.eq_ignore_ascii_case(layer)))
                                {
                                    let mut one = vec![d.packs.rows[i].clone()];
                                    if rows_grid(
                                        ui,
                                        &ed.doc,
                                        &mut ed.lid.draft,
                                        &format!("swmm-snow-{layer}"),
                                        "SNOWPACKS",
                                        &mut one,
                                        2,
                                        false,
                                        56.0,
                                    ) {
                                        d.packs.rows[i] = one.remove(0);
                                        d.packs.dirty = true;
                                    }
                                    ui.label(RichText::new(snow_help(layer)).small().weak());
                                }
                                ui.add_space(4.0);
                            }
                        } else {
                            ui.label("Choose a snow pack, or + to add one.");
                        }
                        ui.separator();
                        egui::CollapsingHeader::new("Temperature and adjustments")
                            .id_salt("swmm-snow-climate")
                            .show(ui, |ui| {
                                ui.label(RichText::new("[TEMPERATURE] — TIMESERIES name | FILE \"name\" [start] [C/F] | WINDSPEED MONTHLY v1…v12 | WINDSPEED FILE | SNOWMELT Stemp ATIwt RNM Elev Lat DTLong | ADC IMPERVIOUS f0…f9 | ADC PERVIOUS f0…f9").small());
                                ui.add(
                                    egui::TextEdit::multiline(&mut d.temperature)
                                        .id(Id::new("swmm-temperature-text"))
                                        .desired_rows(4)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                );
                                ui.label(RichText::new("[ADJUSTMENTS] — TEMPERATURE | EVAPORATION | RAINFALL | CONDUCTIVITY, each followed by 12 monthly values").small());
                                ui.add(
                                    egui::TextEdit::multiline(&mut d.adjustments)
                                        .id(Id::new("swmm-adjustments-text"))
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                );
                            });
                    });
                let (ok, apply, cancel) = ok_cancel(ui, d.dirty());
                if ok || apply {
                    if apply_snowpacks(ed, &mut d) {
                        state.status = "Snow packs updated".into();
                    }
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
        state.swmm_doc.lid.snowpacks = None;
        state.swmm_doc.lid.message.clear();
    } else {
        state.swmm_doc.lid.snowpacks = Some(d);
    }
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    draw_aquifers(ctx, state);
    draw_groundwater(ctx, state);
    draw_snowpacks(ctx, state);
    let _ = (node_names, apply_rows, RowsDraft::default);
}
