// SPDX-License-Identifier: GPL-3.0-or-later

//! The Import GIS Layer dialog: a shapefile or GeoJSON layer becomes
//! nodes, conduits, subcatchments or rain gages. The dialog picks the
//! target, the name field and a source for each SWMM column; the mapping,
//! snapping and unit conversion are `stormsewer_swmm::gis::import`'s.
//! Applying is one undo step.

use std::path::PathBuf;

use eframe::egui::{self, Color32, Id, RichText, Vec2};
use stormsewer_swmm::gis::crs::Crs;
use stormsewer_swmm::gis::import::{self, FieldMap, ImportOptions, ImportPlan, Source, Target, UnitFactors};
use stormsewer_swmm::gis::vector::Layer;

use crate::state::AppState;

use super::RefLayer;

/// Where one SWMM column's value comes from, as the dialog shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    /// Leave the default the editor gives a new object.
    Skip,
    /// A layer field, by index.
    Field(usize),
    /// The same text for every feature.
    Constant(String),
}

/// One row of the field-mapping table.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMap {
    pub section: String,
    pub column: String,
    pub choice: Choice,
}

/// The dialog's draft.
pub struct ImportDialog {
    pub path: PathBuf,
    pub layer: Layer,
    pub target: Target,
    /// Field naming each object; `None` numbers them with `name_prefix`.
    pub name_field: Option<usize>,
    pub name_prefix: String,
    pub maps: Vec<ColumnMap>,
    /// The target `maps` were built for; a new target rebuilds them.
    maps_for: Target,
    /// Conduit ends snap to a node within this distance (map units).
    pub snap: String,
    pub create_end_nodes: bool,
    pub outlet_nearest: bool,
    /// Reproject from the layer's CRS to the model's.
    pub reproject: bool,
    /// Adopt the layer's CRS as the model CRS (the model has none).
    pub set_model_crs: bool,
    /// Keep the layer on the map as a reference layer after importing.
    pub keep_reference: bool,
    pub preview: Option<ImportPlan>,
    pub error: Option<String>,
}

fn maps_for(target: Target, layer: &Layer) -> (Vec<ColumnMap>, Option<usize>) {
    let opts = ImportOptions::new(target).auto_map(layer);
    let maps = opts
        .fields
        .into_iter()
        .map(|m| ColumnMap {
            section: m.section,
            column: m.column,
            choice: match m.source {
                Source::Skip => Choice::Skip,
                Source::Field(i) => Choice::Field(i),
                Source::Constant(c) => Choice::Constant(c),
            },
        })
        .collect();
    (maps, opts.name_field)
}

impl ImportDialog {
    /// A draft for `layer` against a model whose CRS is `model_crs`.
    pub fn new(path: PathBuf, layer: Layer, model_crs: Option<&Crs>) -> Result<Self, String> {
        let kind = layer
            .geometry_kind()
            .ok_or_else(|| "the layer has no features with a geometry".to_string())?;
        let target = Target::for_kind(kind)[0];
        let (maps, name_field) = maps_for(target, &layer);
        let (set_model_crs, reproject) = match (&layer.crs, model_crs) {
            (Some(_), None) => (true, false),
            (Some(l), Some(m)) => (false, !l.same_as(m)),
            _ => (false, false),
        };
        Ok(Self {
            path,
            target,
            name_field,
            name_prefix: target.default_prefix().to_string(),
            maps,
            maps_for: target,
            snap: "1".into(),
            create_end_nodes: true,
            outlet_nearest: true,
            reproject,
            set_model_crs,
            keep_reference: false,
            preview: None,
            error: None,
            layer,
        })
    }

    /// Rebuild the mapping table when the target changed.
    pub fn sync_target(&mut self) {
        if self.maps_for != self.target {
            let (maps, name) = maps_for(self.target, &self.layer);
            self.maps = maps;
            if self.name_field.is_none() {
                self.name_field = name;
            }
            self.name_prefix = self.target.default_prefix().to_string();
            self.maps_for = self.target;
            self.preview = None;
        }
    }

    /// The library's options for this draft.
    pub fn options(&self, model_crs: Option<&Crs>) -> Result<ImportOptions, String> {
        let mut o = ImportOptions::new(self.target);
        o.name_field = self.name_field;
        o.name_prefix = if self.name_prefix.trim().is_empty() {
            self.target.default_prefix().to_string()
        } else {
            self.name_prefix.trim().to_string()
        };
        o.fields = self
            .maps
            .iter()
            .map(|m| FieldMap {
                section: m.section.clone(),
                column: m.column.clone(),
                source: match &m.choice {
                    Choice::Skip => Source::Skip,
                    Choice::Field(i) => Source::Field(*i),
                    Choice::Constant(c) if c.trim().is_empty() => Source::Skip,
                    Choice::Constant(c) => Source::Constant(c.trim().to_string()),
                },
            })
            .collect();
        o.snap_tolerance = match self.snap.trim().parse::<f64>() {
            Ok(v) if v >= 0.0 && v.is_finite() => v,
            _ => return Err(format!("snap tolerance {:?} is not a distance", self.snap)),
        };
        o.create_end_nodes = self.create_end_nodes;
        o.outlet_nearest_node = self.outlet_nearest;
        o.reproject = match (self.reproject, &self.layer.crs, model_crs) {
            (true, Some(from), Some(to)) if !from.same_as(to) => Some((from.clone(), to.clone())),
            _ => None,
        };
        Ok(o)
    }
}

/// Read `path` and open the dialog for it.
pub fn open(state: &mut AppState, path: PathBuf) -> Result<(), String> {
    let layer = stormsewer_swmm::gis::read_vector(&path).map_err(|e| e.to_string())?;
    let d = ImportDialog::new(path, layer, state.swmm_doc.gis.model_crs.as_ref())?;
    state.swmm_doc.gis.import = Some(d);
    Ok(())
}

/// The CRS the import will run against: the model's, or the layer's when
/// the dialog adopts it.
fn effective_crs(state: &AppState, d: &ImportDialog) -> Option<Crs> {
    match (&state.swmm_doc.gis.model_crs, d.set_model_crs, &d.layer.crs) {
        (Some(m), _, _) => Some(m.clone()),
        (None, true, Some(l)) => Some(l.clone()),
        _ => None,
    }
}

fn build_plan(state: &AppState, d: &ImportDialog) -> Result<ImportPlan, String> {
    let crs = effective_crs(state, d);
    let opts = d.options(crs.as_ref())?;
    let units = UnitFactors::from_doc(&state.swmm_doc.doc, crs.as_ref());
    import::plan(&state.swmm_doc.doc, &d.layer, &opts, units).map_err(|e| e.to_string())
}

/// Compute the plan into the dialog's `preview` (or `error`).
pub fn preview(state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.gis.import.take() else { return };
    d.sync_target();
    match build_plan(state, &d) {
        Ok(p) => {
            d.preview = Some(p);
            d.error = None;
        }
        Err(e) => {
            d.preview = None;
            d.error = Some(e);
        }
    }
    state.swmm_doc.gis.import = Some(d);
}

/// Import the layer as one undo step, select what was created and close
/// the dialog. Returns the report; on an error the dialog stays open.
pub fn apply(state: &mut AppState) -> Result<String, String> {
    let Some(mut d) = state.swmm_doc.gis.import.take() else {
        return Err("no import in progress".into());
    };
    d.sync_target();
    let plan = match build_plan(state, &d) {
        Ok(p) => p,
        Err(e) => {
            d.error = Some(e.clone());
            state.swmm_doc.gis.import = Some(d);
            return Err(e);
        }
    };
    if plan.items.is_empty() {
        let why = plan
            .skipped
            .first()
            .map(|(i, r)| format!(" (feature {}: {r})", i + 1))
            .unwrap_or_default();
        let e = format!("nothing to import{why}");
        d.preview = Some(plan);
        d.error = Some(e.clone());
        state.swmm_doc.gis.import = Some(d);
        return Err(e);
    }
    let mut crs_note = None;
    if state.swmm_doc.gis.model_crs.is_none() && d.set_model_crs {
        if let Some(c) = d.layer.crs.clone() {
            crs_note = Some(super::set_model_crs(&mut state.swmm_doc, Some(c)));
        }
    }
    let ed = &mut state.swmm_doc;
    let label = "import GIS layer";
    let mut done = ImportPlan {
        notes: plan.notes.clone(),
        skipped: plan.skipped.clone(),
        ..Default::default()
    };
    ed.begin_gesture(label);
    for item in &plan.items {
        if ed.apply(item.command.clone(), label) {
            done.items.push(item.clone());
        } else {
            done.skipped.push((item.feature, ed.last_error.clone().unwrap_or_default()));
        }
    }
    ed.end_gesture();
    ed.refresh();
    ed.select_many(import::created_refs(&done, d.target));
    let mut report = format!(
        "Imported {} from {}",
        done.summary(d.target),
        d.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    );
    for n in &done.notes {
        report.push_str(&format!("; {n}"));
    }
    if let Some(n) = crs_note {
        report.push_str(&format!("; {n}"));
    }
    if d.keep_reference {
        let model_crs = ed.gis.model_crs.clone();
        let layer = match (&d.layer.crs, &model_crs) {
            (Some(from), Some(to)) if d.reproject && !from.same_as(to) => import::reproject(&d.layer, from, to).unwrap_or(d.layer),
            _ => d.layer,
        };
        let color = ed.gis.next_color();
        ed.gis.layers.push(RefLayer {
            name: layer.name.clone(),
            layer,
            color,
            visible: true,
            label_field: None,
            source: d.path,
        });
    }
    ed.gis.last_report = Some(report.clone());
    Ok(report)
}

fn choice_text(layer: &Layer, c: &Choice) -> String {
    match c {
        Choice::Skip => "(default)".into(),
        Choice::Field(i) => layer.fields.get(*i).map(|f| f.name.clone()).unwrap_or_else(|| "?".into()),
        Choice::Constant(_) => "(constant)".into(),
    }
}

/// The dialog window.
pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.gis.import.take() else { return };
    let model_crs = state.swmm_doc.gis.model_crs.clone();
    let mut open = true;
    let mut action: Option<&str> = None;
    super::window(ctx, "Import GIS Layer", Vec2::new(560.0, 520.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let kind = d.layer.geometry_kind();
            ui.label(format!(
                "{} — {} feature(s), {}, {} field(s)",
                d.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                d.layer.features.len(),
                kind.map(|k| k.label()).unwrap_or("no geometry"),
                d.layer.fields.len()
            ));
            let layer_crs = match (&d.layer.crs, &d.layer.crs_text) {
                (Some(c), _) => c.label(),
                (None, Some(t)) => format!("not recognised: {}", t.chars().take(60).collect::<String>()),
                (None, None) => "not declared".into(),
            };
            ui.label(RichText::new(format!("Layer CRS: {layer_crs}")).small());
            ui.label(
                RichText::new(format!(
                    "Model CRS: {}",
                    model_crs.as_ref().map(Crs::label).unwrap_or_else(|| "not set".into())
                ))
                .small(),
            );
            match (&d.layer.crs, &model_crs) {
                (Some(l), Some(m)) if !l.same_as(m) => {
                    ui.checkbox(&mut d.reproject, "Reproject to the model CRS");
                }
                (Some(_), None) => {
                    ui.checkbox(&mut d.set_model_crs, "Use the layer's CRS as the model CRS");
                }
                _ => {}
            }
            ui.separator();
            egui::Grid::new("swmm-gis-import-opts").num_columns(2).show(ui, |ui| {
                ui.label("Import as:");
                egui::ComboBox::from_id_salt("swmm-gis-import-target")
                    .selected_text(d.target.label())
                    .show_ui(ui, |ui| {
                        for t in kind.map(Target::for_kind).unwrap_or_default() {
                            ui.selectable_value(&mut d.target, t, t.label());
                        }
                    });
                ui.end_row();
                ui.label("Name from:");
                let current = d
                    .name_field
                    .and_then(|i| d.layer.fields.get(i))
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| "(numbered)".into());
                egui::ComboBox::from_id_salt("swmm-gis-import-name")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.name_field, None, "(numbered)");
                        for (i, f) in d.layer.fields.iter().enumerate() {
                            ui.selectable_value(&mut d.name_field, Some(i), f.name.as_str());
                        }
                    });
                ui.end_row();
                ui.label("Name prefix:");
                ui.add(egui::TextEdit::singleline(&mut d.name_prefix).id(Id::new("swmm-gis-import-prefix")).desired_width(80.0));
                ui.end_row();
                if d.target == Target::Conduit {
                    ui.label("Snap tolerance:");
                    ui.add(egui::TextEdit::singleline(&mut d.snap).id(Id::new("swmm-gis-import-snap")).desired_width(80.0));
                    ui.end_row();
                }
            });
            d.sync_target();
            if d.target == Target::Conduit {
                ui.checkbox(&mut d.create_end_nodes, "Create junctions at unsnapped ends");
            }
            if d.target == Target::Subcatchment {
                ui.checkbox(&mut d.outlet_nearest, "Outlet to the nearest node");
            }
            ui.checkbox(&mut d.keep_reference, "Keep as a reference layer");
            ui.separator();
            ui.label(RichText::new("Field mapping").strong());
            egui::ScrollArea::vertical()
                .id_salt("swmm-gis-import-maps")
                .max_height(220.0)
                .show(ui, |ui| {
                    egui::Grid::new("swmm-gis-import-map-grid").num_columns(3).striped(true).show(ui, |ui| {
                        let fields: Vec<String> = d.layer.fields.iter().map(|f| f.name.clone()).collect();
                        for (k, m) in d.maps.iter_mut().enumerate() {
                            ui.label(format!("{} {}", m.section, m.column));
                            let text = choice_text(&d.layer, &m.choice);
                            egui::ComboBox::from_id_salt(("swmm-gis-import-map", k))
                                .selected_text(text)
                                .width(140.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut m.choice, Choice::Skip, "(default)");
                                    for (i, f) in fields.iter().enumerate() {
                                        ui.selectable_value(&mut m.choice, Choice::Field(i), f.as_str());
                                    }
                                    if !matches!(m.choice, Choice::Constant(_))
                                        && ui.selectable_label(false, "(constant)").clicked()
                                    {
                                        m.choice = Choice::Constant(String::new());
                                    }
                                });
                            if let Choice::Constant(c) = &mut m.choice {
                                ui.add(egui::TextEdit::singleline(c).id(Id::new(("swmm-gis-import-const", k))).desired_width(90.0));
                            } else {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    });
                });
            ui.separator();
            if let Some(p) = &d.preview {
                ui.label(format!("Preview: {}", p.summary(d.target)));
                for n in &p.notes {
                    ui.label(RichText::new(n).small());
                }
                for (i, why) in p.skipped.iter().take(8) {
                    ui.label(RichText::new(format!("feature {}: {why}", i + 1)).small());
                }
                if p.skipped.len() > 8 {
                    ui.label(RichText::new(format!("… and {} more skipped", p.skipped.len() - 8)).small());
                }
            }
            if let Some(e) = &d.error {
                ui.colored_label(Color32::from_rgb(200, 60, 60), e);
            }
            ui.horizontal(|ui| {
                if ui.button("Preview").clicked() {
                    action = Some("preview");
                }
                if ui.button("Import").clicked() {
                    action = Some("import");
                }
                if ui.button("Cancel").clicked() {
                    action = Some("cancel");
                }
            });
        });
    match action {
        Some("cancel") => {}
        Some(a) => {
            state.swmm_doc.gis.import = Some(d);
            if a == "preview" {
                preview(state);
            } else {
                match apply(state) {
                    Ok(r) => state.status = r,
                    Err(e) => state.status = format!("Import GIS Layer: {e}"),
                }
            }
        }
        None => {
            if open {
                state.swmm_doc.gis.import = Some(d);
            }
        }
    }
}
