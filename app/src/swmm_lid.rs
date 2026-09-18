// SPDX-License-Identifier: GPL-3.0-or-later

//! Dialogs for the model sections that had only attribute tables: LID
//! controls and usage (here), aquifers, groundwater and snow packs
//! (`swmm_lid_gw.rs`), pollutant buildup / washoff / coverages / loadings,
//! treatment and RDII unit hydrographs (`swmm_lid_quality.rs`).
//!
//! Every dialog edits a *draft* of the rows that belong to one name in one
//! section — the raw fields, quotes and all — and writes it back with
//! [`build::replace_rows`] on **OK** (or **Apply**), which is one undo
//! step. A row the draft did not touch compares equal to the file's and is
//! not rewritten, so OK on an unchanged draft is no step and no change to
//! the text: the lossless promise holds through these dialogs. **Cancel**
//! or closing the window drops the draft.
//!
//! Column meanings are the SWMM 5.2 User's Manual, Appendix D; the row
//! shapes the engine's readers (`lid.c`, `gwater.c`, `snow.c`,
//! `landuse.c`, `treatmnt.c`, `rdii.c`). Nothing here was taken from any
//! other program's documentation.

use eframe::egui::{self, Button, Id, RichText, Ui, Vec2};
use stormsewer_swmm::doc::build::{self, ObjRef};
use stormsewer_swmm::doc::{schema, Command, InpDoc, ObjectKind};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;
use crate::swmm_props::{self, field_widget, spec, text_field};

#[path = "swmm_lid_gw.rs"]
pub mod gw;
#[path = "swmm_lid_quality.rs"]
pub mod quality;
#[cfg(test)]
#[path = "swmm_lid_tests.rs"]
mod tests;

// --- drafts -------------------------------------------------------------------------

/// A draft of every row that belongs to one name in one section: the raw
/// fields of each row (quotes kept), in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowsDraft {
    pub section: String,
    pub name: String,
    pub rows: Vec<Vec<String>>,
    /// The document generation the draft was read at; a change elsewhere
    /// reloads an unedited draft.
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

impl RowsDraft {
    pub fn load(doc: &InpDoc, section: &str, name: &str) -> Self {
        Self {
            section: section.to_ascii_uppercase(),
            name: name.to_string(),
            rows: build::rows_of(doc, section, name),
            loaded_gen: Some(doc.generation()),
            dirty: false,
        }
    }

    /// Reload from the document unless the draft has edits.
    pub fn refresh(&mut self, doc: &InpDoc) {
        if !self.dirty && self.loaded_gen != Some(doc.generation()) {
            *self = Self::load(doc, &self.section, &self.name);
        }
    }

    /// The batch that writes the draft back (empty when nothing changed).
    pub fn command(&self, doc: &InpDoc) -> Command {
        build::replace_rows(doc, &self.section, &self.name, &self.rows)
    }
}

/// Apply a draft as one step. An unchanged draft applies nothing and adds
/// no step.
pub fn apply_rows(ed: &mut SwmmEditor, d: &mut RowsDraft, label: &str) -> bool {
    let cmd = d.command(&ed.doc);
    let ok = ed.apply(cmd, label);
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

/// A value as a row field: quoted when empty or containing blanks.
pub fn raw(v: &str) -> String {
    if v.is_empty() || v.chars().any(char::is_whitespace) {
        format!("\"{v}\"")
    } else {
        v.to_string()
    }
}

/// Set column `i` of `row` to `v`, filling the columns before it with their
/// defaults. Clearing the last column drops it (a trailing optional field
/// left out), so the engine reads the row as before the column was added.
pub fn set_cell(row: &mut Vec<String>, cols: &[&str], section: &str, i: usize, v: &str) {
    let v = v.trim();
    if v.is_empty() && i + 1 == row.len() {
        row.pop();
        while row.len() > 2 && row.last().is_some_and(|f| f == "\"\"") {
            row.pop();
        }
        return;
    }
    if v.is_empty() && i >= row.len() {
        return;
    }
    while row.len() < i {
        let col = cols.get(row.len()).copied().unwrap_or("");
        row.push(swmm_props::default_for(section, col));
    }
    if i < row.len() {
        row[i] = raw(v);
    } else {
        row.push(raw(v));
    }
}

/// The unquoted value of column `i`, or empty.
pub fn cell_value(row: &[String], i: usize) -> String {
    row.get(i)
        .map(|s| stormsewer_swmm::doc::unquote(s).to_string())
        .unwrap_or_default()
}

/// The layout of a draft row, from the schema.
pub fn row_columns(doc: &InpDoc, section: &str, row: &[String]) -> &'static [&'static str] {
    doc.columns(
        section,
        &stormsewer_swmm::doc::Row {
            fields: row.to_vec(),
            comment: None,
        },
    )
}

/// One editable cell: the widget for the column's field kind. Returns the
/// new value when the user commits a change.
#[allow(clippy::too_many_arguments)]
pub fn cell(
    ui: &mut Ui,
    doc: &InpDoc,
    draft: &mut Option<(Id, String)>,
    id: Id,
    section: &str,
    col: &str,
    value: &str,
    width: f32,
) -> Option<String> {
    let fs = spec(doc, section, col);
    field_widget(ui, id, &fs, value, draft, width)
}

/// The column header text: the name and its unit.
pub fn header(doc: &InpDoc, section: &str, col: &str) -> String {
    match spec(doc, section, col).unit {
        Some(u) => format!("{col} ({u})"),
        None => col.to_string(),
    }
}

/// A grid over `rows`: a header from the first row's layout, then one line
/// per row with a cell per column from `skip` on and a `×` to remove the
/// row when `removable`. Returns whether a cell changed.
#[allow(clippy::too_many_arguments)]
pub fn rows_grid(
    ui: &mut Ui,
    doc: &InpDoc,
    draft: &mut Option<(Id, String)>,
    id: &str,
    section: &str,
    rows: &mut Vec<Vec<String>>,
    skip: usize,
    removable: bool,
    width: f32,
) -> bool {
    let mut changed = false;
    let mut remove: Option<usize> = None;
    let header_cols: Vec<&'static str> = rows
        .iter()
        .map(|r| row_columns(doc, section, r))
        .max_by_key(|c| c.len())
        .map(|c| c.to_vec())
        .unwrap_or_default();
    egui::Grid::new((id, "grid"))
        .striped(true)
        .show(ui, |ui| {
            for col in header_cols.iter().skip(skip) {
                ui.label(RichText::new(header(doc, section, col)).strong());
            }
            if removable {
                ui.label("");
            }
            ui.end_row();
            for (ri, row) in rows.iter_mut().enumerate() {
                let cols = row_columns(doc, section, row);
                for (ci, col) in cols.iter().enumerate().skip(skip) {
                    let value = cell_value(row, ci);
                    let wid = Id::new((id, ri, *col));
                    if let Some(v) = cell(ui, doc, draft, wid, section, col, &value, width) {
                        set_cell(row, cols, section, ci, &v);
                        changed = true;
                    }
                }
                // Fields past the layout stay visible, read-only.
                for f in row.iter().skip(cols.len()) {
                    ui.label(RichText::new(f).monospace().weak());
                }
                if removable && ui.small_button("×").clicked() {
                    remove = Some(ri);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    changed
}

/// The rows as the engine reads them, one per line.
pub fn preview_text(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|r| r.join("  "))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn window<'a>(ctx: &egui::Context, title: &'a str, size: Vec2) -> egui::Window<'a> {
    egui::Window::new(title)
        .id(Id::new(("swmm-lid-dialog", title)))
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .default_pos(ctx.screen_rect().center() - size / 2.0)
}

/// A list of names, returning the pick.
pub fn name_list(
    ui: &mut Ui,
    id: &str,
    names: &[String],
    current: Option<&str>,
    height: f32,
) -> Option<String> {
    let mut pick = None;
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .show(ui, |ui| {
            for n in names {
                if ui
                    .selectable_label(current.is_some_and(|c| c.eq_ignore_ascii_case(n)), n)
                    .clicked()
                {
                    pick = Some(n.clone());
                }
            }
            if names.is_empty() {
                ui.label(RichText::new("(none)").weak());
            }
        });
    pick
}

/// A combo over `names` with the current pick; returns a new pick.
pub fn name_combo(ui: &mut Ui, id: &str, names: &[String], current: &str, width: f32) -> Option<String> {
    let mut pick = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(if current.is_empty() { "(none)" } else { current })
        .width(width)
        .show_ui(ui, |ui| {
            for n in names {
                if ui
                    .selectable_label(n.eq_ignore_ascii_case(current), n)
                    .clicked()
                    && !n.eq_ignore_ascii_case(current)
                {
                    pick = Some(n.clone());
                }
            }
        });
    pick
}

/// The subcatchment a per-subcatchment dialog opens on: the one asked
/// for, else the selected one, else the first.
pub fn pick_subcatchment(ed: &SwmmEditor, item: Option<&str>) -> Option<String> {
    item.map(str::to_string)
        .or_else(|| {
            ed.selection.iter().find_map(|r| match r {
                ObjRef::Subcatchment(n) => Some(n.clone()),
                _ => None,
            })
        })
        .or_else(|| ed.doc.names("SUBCATCHMENTS").first().cloned())
}

/// The node a per-node dialog opens on.
pub fn pick_node(ed: &SwmmEditor, item: Option<&str>) -> Option<String> {
    item.map(str::to_string)
        .or_else(|| ed.selected_nodes().first().cloned())
        .or_else(|| {
            schema::NODE_SECTIONS
                .iter()
                .find_map(|s| ed.doc.names(s).first().cloned())
        })
}

/// Every node name, every section.
pub fn node_names(doc: &InpDoc) -> Vec<String> {
    schema::NODE_SECTIONS
        .iter()
        .flat_map(|s| doc.names(s))
        .collect()
}

// --- state ----------------------------------------------------------------------------

/// The draft of the LID Controls dialog: the chosen process and its rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LidControlsDraft {
    pub name: Option<String>,
    pub rows: Vec<Vec<String>>,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

impl LidControlsDraft {
    pub fn load(doc: &InpDoc, name: Option<String>) -> Self {
        let rows = name
            .as_deref()
            .map(|n| build::rows_of(doc, "LID_CONTROLS", n))
            .unwrap_or_default();
        Self {
            name,
            rows,
            loaded_gen: Some(doc.generation()),
            dirty: false,
        }
    }

    /// The type keyword from the two-field row.
    pub fn kind(&self) -> String {
        self.rows
            .iter()
            .find(|r| r.len() == 2)
            .and_then(|r| r.get(1))
            .map(|k| k.to_ascii_uppercase())
            .unwrap_or_default()
    }

    /// The row for `layer`, if the draft has one.
    pub fn layer_row(&self, layer: &str) -> Option<usize> {
        self.rows
            .iter()
            .position(|r| r.len() != 2 && r.get(1).is_some_and(|l| l.eq_ignore_ascii_case(layer)))
    }

    /// Set the type: rewrites (or adds) the two-field row and adds the
    /// layers the new type insists on, with defaults. Other rows stay.
    pub fn set_kind(&mut self, kind: &str) {
        let name = self.name.clone().unwrap_or_default();
        match self.rows.iter().position(|r| r.len() == 2) {
            Some(i) => self.rows[i][1] = kind.to_string(),
            None => self.rows.insert(0, vec![name.clone(), kind.to_string()]),
        }
        for (layer, required) in build::lid_layers_for(kind) {
            if *required && self.layer_row(layer).is_none() {
                self.add_layer(layer);
            }
        }
        self.dirty = true;
    }

    /// Add a layer row with defaults, in the manual's layer order.
    pub fn add_layer(&mut self, layer: &str) {
        if self.layer_row(layer).is_some() {
            return;
        }
        let name = self.name.clone().unwrap_or_default();
        let mut r = vec![name, layer.to_string()];
        r.extend(build::lid_layer_defaults(layer));
        let order = |l: &str| schema::LID_LAYERS.iter().position(|x| x.eq_ignore_ascii_case(l)).unwrap_or(99);
        let pos = self
            .rows
            .iter()
            .position(|x| x.len() != 2 && order(&x[1]) > order(layer))
            .unwrap_or(self.rows.len());
        self.rows.insert(pos, r);
        self.dirty = true;
    }

    pub fn remove_layer(&mut self, layer: &str) {
        if let Some(i) = self.layer_row(layer) {
            self.rows.remove(i);
            self.dirty = true;
        }
    }

    pub fn command(&self, doc: &InpDoc) -> Command {
        match &self.name {
            Some(n) => build::replace_rows(doc, "LID_CONTROLS", n, &self.rows),
            None => Command::Batch(Vec::new()),
        }
    }
}

/// Per-editor state for these dialogs: one open draft per dialog, and
/// the text-field draft they share.
#[derive(Default)]
pub struct LidState {
    pub lid_controls: Option<LidControlsDraft>,
    /// `[LID_USAGE]` rows of one subcatchment.
    pub lid_usage: Option<RowsDraft>,
    pub aquifers: Option<gw::AquifersDraft>,
    pub groundwater: Option<gw::GroundwaterDraft>,
    pub snowpacks: Option<gw::SnowDraft>,
    pub quality: Option<quality::QualityDraft>,
    pub coverages: Option<quality::PairsDraft>,
    pub loadings: Option<quality::PairsDraft>,
    pub treatment: Option<quality::TreatmentDraft>,
    pub hydrographs: Option<quality::HydrographsDraft>,
    pub rdii: Option<RowsDraft>,
    /// The text field being typed into.
    pub draft: Option<(Id, String)>,
    pub message: String,
}

// --- open / apply -----------------------------------------------------------------------

pub fn open_lid_controls(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("LID_CONTROLS").first().cloned());
    ed.lid.lid_controls = Some(LidControlsDraft::load(&ed.doc, name));
}

pub fn open_lid_usage(ed: &mut SwmmEditor, item: Option<&str>) {
    let Some(sub) = pick_subcatchment(ed, item) else {
        ed.lid.message = "The model has no subcatchment.".into();
        return;
    };
    ed.lid.lid_usage = Some(RowsDraft::load(&ed.doc, "LID_USAGE", &sub));
}

/// Write the LID Controls draft back as one step.
pub fn apply_lid_controls(ed: &mut SwmmEditor, d: &mut LidControlsDraft) -> bool {
    let Some(name) = d.name.clone() else {
        return false;
    };
    let cmd = d.command(&ed.doc);
    let ok = ed.apply(cmd, &format!("edit LID control {name}"));
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

/// Rename an LID process everywhere it is named: its rows and the
/// `[LID_USAGE]` rows that use it. Refused when the name is taken.
pub fn rename_lid_control(ed: &mut SwmmEditor, old: &str, new: &str) -> bool {
    let new = new.trim();
    if new.is_empty() || new.contains(char::is_whitespace) {
        ed.last_error = Some("a name has no blanks".into());
        return false;
    }
    if !new.eq_ignore_ascii_case(old) && ed.doc.contains("LID_CONTROLS", new) {
        ed.last_error = Some(format!("an LID control named {new} exists"));
        return false;
    }
    let cmd = build::rename_in_columns(&ed.doc, &[("LID_CONTROLS", 0), ("LID_USAGE", 1)], old, new);
    ed.apply(cmd, &format!("rename LID control {old} to {new}"))
}

/// Delete an LID process with the `[LID_USAGE]` rows that place it.
pub fn delete_lid_control(doc: &InpDoc, name: &str) -> Command {
    let mut cmds = vec![Command::DeleteObject {
        section: "LID_CONTROLS".into(),
        name: name.into(),
    }];
    let lines: Vec<usize> = doc
        .rows("LID_USAGE")
        .into_iter()
        .filter(|(_, r)| r.value(1).is_some_and(|l| l.eq_ignore_ascii_case(name)))
        .map(|(li, _)| li)
        .collect();
    for li in lines.into_iter().rev() {
        cmds.push(Command::DeleteLine {
            section: "LID_USAGE".into(),
            line: li,
        });
    }
    Command::Batch(cmds)
}

/// How many subcatchments place `name`.
pub fn lid_uses(doc: &InpDoc, name: &str) -> usize {
    doc.rows("LID_USAGE")
        .into_iter()
        .filter(|(_, r)| r.value(1).is_some_and(|l| l.eq_ignore_ascii_case(name)))
        .count()
}

// --- menu ----------------------------------------------------------------------------------

/// `Project` menu entries for these dialogs.
pub fn project_menu_items(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.swmm_doc.loaded;
    let ed = &mut state.swmm_doc;
    let mut close = false;
    if ui
        .add_enabled(loaded, Button::new("LID Controls…"))
        .on_hover_text("[LID_CONTROLS]: the LID processes and their layers")
        .clicked()
    {
        open_lid_controls(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("LID Usage…"))
        .on_hover_text("[LID_USAGE]: the LID units placed in a subcatchment")
        .clicked()
    {
        open_lid_usage(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("Aquifers…"))
        .on_hover_text("[AQUIFERS]: the groundwater aquifers")
        .clicked()
    {
        gw::open_aquifers(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("Groundwater…"))
        .on_hover_text("[GROUNDWATER] and [GWF]: a subcatchment's groundwater flow")
        .clicked()
    {
        gw::open_groundwater(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("Snow Packs…"))
        .on_hover_text("[SNOWPACKS], with [TEMPERATURE] and [ADJUSTMENTS]")
        .clicked()
    {
        gw::open_snowpacks(ed, None);
        close = true;
    }
    ui.separator();
    if ui
        .add_enabled(loaded, Button::new("Buildup / Washoff…"))
        .on_hover_text("[BUILDUP] and [WASHOFF] per land use and pollutant")
        .clicked()
    {
        quality::open_quality(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("Land Use Coverages…"))
        .on_hover_text("[COVERAGES]: a subcatchment's land uses")
        .clicked()
    {
        quality::open_coverages(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("Initial Loadings…"))
        .on_hover_text("[LOADINGS]: a subcatchment's initial pollutant buildup")
        .clicked()
    {
        quality::open_loadings(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("Treatment…"))
        .on_hover_text("[TREATMENT]: a node's removal or concentration expressions")
        .clicked()
    {
        quality::open_treatment(ed, None);
        close = true;
    }
    ui.separator();
    if ui
        .add_enabled(loaded, Button::new("Unit Hydrographs…"))
        .on_hover_text("[HYDROGRAPHS]: RDII unit hydrograph sets")
        .clicked()
    {
        quality::open_hydrographs(ed, None);
        close = true;
    }
    if ui
        .add_enabled(loaded, Button::new("RDII Inflow…"))
        .on_hover_text("[RDII]: a node's unit hydrograph set and sewer area")
        .clicked()
    {
        quality::open_rdii(ed, None);
        close = true;
    }
    if close {
        ui.close_menu();
    }
}

// --- LID Controls dialog ----------------------------------------------------------------

/// The text under a layer's grid: what each column is, per Appendix D.
fn layer_help(layer: &str) -> &'static str {
    match layer {
        "SURFACE" => "StorHt: berm height (depth of surface storage). VegFrac: fraction of that volume filled by vegetation. Rough: Manning's n for overland flow (swales, pavement, roofs). Slope: surface slope (%). Xslope: swale side slope, run per unit rise.",
        "SOIL" => "Thick: soil layer thickness. Por: porosity (void volume / total). FC: field capacity. WP: wilting point. Ksat: saturated conductivity. Kslope: conductivity slope (how fast K falls as moisture falls). Suct: suction head at the wetting front.",
        "PAVEMENT" => "Thick: pavement thickness. Vratio: void ratio (voids / solids). FracImp: impervious fraction (0 for continuous porous pavement, more for modular blocks). Perm: permeability of the pavement. Vclog: void volume of runoff treated before clogging (0 = never). Treg: days between regeneration (0 = none). Freg: degree of regeneration, 0–1.",
        "STORAGE" => "Height: storage (gravel) layer thickness; rain barrel height. Vratio: void ratio. Seepage: infiltration rate into the native soil beneath. Vclog: void volumes treated before clogging (0 = never). Covrd: YES when the storage is covered and does not take rain (rain barrels).",
        "DRAIN" => "Coeff and Expon: underdrain flow = Coeff × (head)^Expon, in flow-rate-per-area units. Offset: drain height above the storage bottom. Delay: hours a rain barrel waits after rain stops before draining. Hopen / Hclose: heads at which a controlled drain opens / closes (optional). Qcurve: a control curve name (optional).",
        "DRAINMAT" => "Thick: drainage mat thickness. Vratio: void fraction. Rough: Manning's n for flow through the mat.",
        "REMOVALS" => "Pollutant and Removal (%) pairs: the fraction of each pollutant removed from the LID's outflow.",
        _ => "",
    }
}

fn draw_lid_controls(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.lid_controls.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let dark = ctx.style().visuals.dark_mode;
    window(ctx, "LID Controls", Vec2::new(760.0, 560.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            if !d.dirty && d.loaded_gen != Some(ed.doc.generation()) {
                let n = d.name.clone();
                d = LidControlsDraft::load(&ed.doc, n);
            }
            let names = ed.doc.names("LID_CONTROLS");
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.set_max_width(200.0);
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("Add an LID control").clicked() {
                        let o = build::new_lid_control(&ed.doc, "BC");
                        let n = o.name.clone();
                        if ed.apply(o.command, &format!("add LID control {n}")) {
                            d = LidControlsDraft::load(&ed.doc, Some(n));
                        }
                    }
                    if ui
                        .add_enabled(d.name.is_some(), egui::Button::new("−"))
                        .on_hover_text("Delete the chosen LID control and the usage rows that place it")
                        .clicked()
                    {
                        if let Some(n) = d.name.clone() {
                            if ed.apply(delete_lid_control(&ed.doc, &n), &format!("delete LID control {n}")) {
                                d = LidControlsDraft::load(&ed.doc, None);
                            }
                        }
                    }
                    if ui
                        .add_enabled(d.name.is_some(), egui::Button::new("Duplicate"))
                        .on_hover_text("Copy the chosen LID control under a new name")
                        .clicked()
                    {
                        if let Some(n) = d.name.clone() {
                            let new = build::unique_row_name(&ed.doc, "LID_CONTROLS", "LID");
                            let rows: Vec<Vec<String>> = build::rows_of(&ed.doc, "LID_CONTROLS", &n)
                                .into_iter()
                                .map(|mut r| {
                                    r[0] = new.clone();
                                    r
                                })
                                .collect();
                            let cmd = build::replace_rows(&ed.doc, "LID_CONTROLS", &new, &rows);
                            if ed.apply(cmd, &format!("duplicate LID control {n}")) {
                                d = LidControlsDraft::load(&ed.doc, Some(new));
                            }
                        }
                    }
                });
                if let Some(pick) = name_list(ui, "swmm-lid-list", &names, d.name.as_deref(), 380.0) {
                    if d.dirty {
                        apply_lid_controls(ed, &mut d);
                    }
                    d = LidControlsDraft::load(&ed.doc, Some(pick));
                }
                for n in &names {
                    if d.name.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(n)) {
                        let uses = lid_uses(&ed.doc, n);
                        ui.label(RichText::new(format!("placed {uses} time(s) in [LID_USAGE]")).small());
                    }
                }

                let ui = &mut cols[1];
                let Some(name) = d.name.clone() else {
                    ui.label("Choose an LID control, or + to add one.");
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let id = Id::new("swmm-lid-name");
                    if let Some(v) = text_field(ui, id, &name, &mut ed.lid.draft, 140.0) {
                        if rename_lid_control(ed, &name, &v) {
                            d = LidControlsDraft::load(&ed.doc, Some(v.trim().to_string()));
                        } else {
                            ed.lid.message = ed.last_error.take().unwrap_or_default();
                        }
                    }
                    ui.label("Type");
                    let kind = d.kind();
                    egui::ComboBox::from_id_salt("swmm-lid-type")
                        .selected_text(if kind.is_empty() {
                            "(none)".to_string()
                        } else {
                            format!("{kind} — {}", build::lid_type_label(&kind))
                        })
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for t in schema::LID_TYPES {
                                if ui
                                    .selectable_label(
                                        kind.eq_ignore_ascii_case(t),
                                        format!("{t} — {}", build::lid_type_label(t)),
                                    )
                                    .clicked()
                                {
                                    d.set_kind(t);
                                }
                            }
                        });
                });
                let kind = d.kind();
                let layers = build::lid_layers_for(&kind);
                egui::ScrollArea::vertical()
                    .id_salt("swmm-lid-layers")
                    .max_height(360.0)
                    .show(ui, |ui| {
                        // Layers the type uses, then any others the rows carry.
                        let mut shown: Vec<(String, bool)> = layers
                            .iter()
                            .map(|(l, req)| (l.to_string(), *req))
                            .collect();
                        for r in &d.rows {
                            if r.len() != 2 {
                                let l = r[1].to_ascii_uppercase();
                                if !shown.iter().any(|(s, _)| *s == l) {
                                    shown.push((l, false));
                                }
                            }
                        }
                        for (layer, required) in shown {
                            let present = d.layer_row(&layer).is_some();
                            let used = layers.iter().any(|(l, _)| *l == layer);
                            let mut on = present;
                            let title = if required {
                                format!("{layer} (required)")
                            } else if !used {
                                format!("{layer} (ignored for a {})", build::lid_type_label(&kind))
                            } else {
                                layer.clone()
                            };
                            let resp = ui.checkbox(&mut on, "");
                            ui.label(RichText::new(title).strong());
                            if resp.changed() {
                                if on {
                                    d.add_layer(&layer);
                                } else {
                                    d.remove_layer(&layer);
                                }
                            }
                            if let Some(ri) = d.layer_row(&layer) {
                                let mut one = vec![d.rows[ri].clone()];
                                let changed = if layer == "REMOVALS" {
                                    removals_grid(ui, &ed.doc, &mut ed.lid.draft, &mut one[0])
                                } else {
                                    rows_grid(
                                        ui,
                                        &ed.doc,
                                        &mut ed.lid.draft,
                                        &format!("swmm-lid-{layer}"),
                                        "LID_CONTROLS",
                                        &mut one,
                                        2,
                                        false,
                                        64.0,
                                    )
                                };
                                if changed {
                                    d.rows[ri] = one.remove(0);
                                    d.dirty = true;
                                }
                                ui.label(RichText::new(layer_help(&layer)).small().weak());
                            }
                            ui.add_space(4.0);
                        }
                        ui.separator();
                        ui.label(RichText::new("As the engine reads it").strong());
                        for (layer, required) in layers {
                            if *required && d.layer_row(layer).is_none() {
                                ui.label(
                                    RichText::new(format!("missing {layer} layer: the engine stops with ERROR 184"))
                                        .color(crate::theme::palette::error_text(dark))
                                        .small(),
                                );
                            }
                        }
                        ui.add(
                            egui::TextEdit::multiline(&mut preview_text(&d.rows))
                                .id(Id::new("swmm-lid-preview"))
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(4)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        if apply_lid_controls(ed, &mut d) {
                            state.status = format!("LID control {name} updated");
                        }
                        close = true;
                    }
                    if ui
                        .add_enabled(d.dirty, egui::Button::new("Apply"))
                        .clicked()
                        && apply_lid_controls(ed, &mut d)
                    {
                        state.status = format!("LID control {name} updated");
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
                if !ed.lid.message.is_empty() {
                    ui.label(RichText::new(&ed.lid.message).small());
                }
            });
        });
    if close || !open {
        state.swmm_doc.lid.lid_controls = None;
        state.swmm_doc.lid.message.clear();
    } else {
        state.swmm_doc.lid.lid_controls = Some(d);
    }
}

/// The `REMOVALS` row: pollutant / percent pairs.
fn removals_grid(ui: &mut Ui, doc: &InpDoc, draft: &mut Option<(Id, String)>, row: &mut Vec<String>) -> bool {
    let mut changed = false;
    let pollutants = doc.names("POLLUTANTS");
    let mut remove: Option<usize> = None;
    let pairs = row.len().saturating_sub(2).div_ceil(2);
    egui::Grid::new("swmm-lid-removals")
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("Pollutant").strong());
            ui.label(RichText::new("Removal (%)").strong());
            ui.label("");
            ui.end_row();
            for p in 0..pairs {
                let i = 2 + 2 * p;
                let name = cell_value(row, i);
                if let Some(v) = name_combo(ui, &format!("swmm-lid-rmv-p{p}"), &pollutants, &name, 120.0) {
                    row[i] = raw(&v);
                    changed = true;
                }
                let pct = cell_value(row, i + 1);
                if let Some(v) = text_field(ui, Id::new(("swmm-lid-rmv-v", p)), &pct, draft, 60.0) {
                    if row.len() <= i + 1 {
                        row.resize(i + 2, "0".into());
                    }
                    row[i + 1] = raw(v.trim());
                    changed = true;
                }
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        row.drain(i..(i + 2).min(row.len()));
        changed = true;
    }
    if ui.small_button("Add pollutant").clicked() {
        let p = pollutants.first().cloned().unwrap_or_else(|| "*".into());
        row.push(p);
        row.push("0".into());
        changed = true;
    }
    changed
}

// --- LID Usage dialog -----------------------------------------------------------------

fn draw_lid_usage(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.lid.lid_usage.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let dark = ctx.style().visuals.dark_mode;
    window(ctx, "LID Usage", Vec2::new(860.0, 380.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            d.refresh(&ed.doc);
            let subs = ed.doc.names("SUBCATCHMENTS");
            ui.horizontal(|ui| {
                ui.label("Subcatchment");
                if let Some(pick) = name_combo(ui, "swmm-lidu-sub", &subs, &d.name, 160.0) {
                    if d.dirty {
                        let label = format!("edit LID usage of {}", d.name);
                        apply_rows(ed, &mut d, &label);
                    }
                    d = RowsDraft::load(&ed.doc, "LID_USAGE", &pick);
                }
                let units = swmm_props::units(&ed.doc);
                let (big, small) = if units.metric { ("ha", "m²") } else { ("acres", "ft²") };
                if let Some(a) = ed.doc.field("SUBCATCHMENTS", &d.name, "Area") {
                    ui.label(RichText::new(format!("area {a} {big}")).small());
                }
                ui.label(RichText::new(format!("LID areas in {small}")).small().weak());
            });
            let lids = ed.doc.names("LID_CONTROLS");
            if lids.is_empty() {
                ui.label("The model has no LID controls yet: Project → LID Controls…");
            }
            egui::ScrollArea::both()
                .id_salt("swmm-lidu-scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    if rows_grid(ui, &ed.doc, &mut ed.lid.draft, "swmm-lidu", "LID_USAGE", &mut d.rows, 1, true, 70.0) {
                        d.dirty = true;
                    }
                });
            if ui
                .add_enabled(!lids.is_empty(), egui::Button::new("Add LID unit"))
                .clicked()
            {
                let mut r = vec![d.name.clone()];
                r.extend(build::lid_usage_defaults(&lids[0]));
                d.rows.push(r);
                d.dirty = true;
            }
            // The area check on the draft.
            let sub_area: Option<f64> = ed
                .doc
                .field("SUBCATCHMENTS", &d.name, "Area")
                .and_then(|a| a.trim().parse().ok());
            let total: f64 = d
                .rows
                .iter()
                .map(|r| {
                    cell_value(r, 2).parse::<f64>().unwrap_or(0.0) * cell_value(r, 3).parse::<f64>().unwrap_or(0.0)
                })
                .sum();
            if let Some(a) = sub_area {
                let metric = build::is_metric(&ed.doc);
                let cap = a * build::subcatchment_area_factor(metric);
                let msg = format!(
                    "{} LID unit(s) cover {} of {} (Number × Area)",
                    d.rows.len(),
                    stormsewer_swmm::doc::format_number(total),
                    stormsewer_swmm::doc::format_number(cap)
                );
                if total > cap * (1.0 + 1e-6) {
                    ui.label(
                        RichText::new(format!("{msg}: more than the subcatchment; the engine refuses it (ERROR 187)"))
                            .color(crate::theme::palette::error_text(dark)),
                    );
                } else {
                    ui.label(RichText::new(msg).small());
                }
            }
            ui.label(
                RichText::new(
                    "FromImp is the percent of the subcatchment's impervious area whose runoff is sent to this LID (0 when the LID takes only the rain on itself); FromPerv the same for the pervious area. ToPerv 1 returns the LID's outflow to the pervious area instead of the outlet. DrainTo names a node or subcatchment for the underdrain flow; * means the outlet. RptFile: a file for the LID's water balance, or *.",
                )
                .small()
                .weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    let label = format!("edit LID usage of {}", d.name);
                    if apply_rows(ed, &mut d, &label) {
                        state.status = format!("LID usage of {} updated", d.name);
                    }
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    if close || !open {
        state.swmm_doc.lid.lid_usage = None;
    } else {
        state.swmm_doc.lid.lid_usage = Some(d);
    }
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.loaded {
        return;
    }
    draw_lid_controls(ctx, state);
    draw_lid_usage(ctx, state);
    gw::draw(ctx, state);
    quality::draw(ctx, state);
    let _ = ObjectKind::Node;
}
