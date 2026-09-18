// SPDX-License-Identifier: GPL-3.0-or-later

//! The Export GIS Layers dialog: the model's nodes, links, subcatchments
//! and rain gages as a shapefile set (with a `.prj` from the model CRS)
//! or as GeoJSON, optionally reprojected to WGS 84, with the loaded run's
//! peaks as extra fields. The writing is `stormsewer_swmm::gis::export`'s.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText, Vec2};
use stormsewer_swmm::gis::export::{self, Peaks};

use crate::state::AppState;

/// The dialog's choices.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportDialog {
    /// GeoJSON rather than a shapefile set.
    pub geojson: bool,
    /// GeoJSON only: reproject to WGS 84 longitude/latitude (RFC 7946).
    pub wgs84: bool,
    /// GeoJSON only: one file with every feature, not one per layer.
    pub combined: bool,
    /// Add the loaded run's peaks as fields.
    pub include_peaks: bool,
    /// The last result line.
    pub result: Option<String>,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self {
            geojson: false,
            wgs84: true,
            combined: false,
            include_peaks: true,
            result: None,
        }
    }
}

pub fn open(state: &mut AppState) {
    if state.swmm_doc.gis.export.is_none() {
        state.swmm_doc.gis.export = Some(ExportDialog::default());
    }
}

fn settings(state: &AppState) -> ExportDialog {
    state.swmm_doc.gis.export.clone().unwrap_or_default()
}

fn peaks<'a>(state: &'a AppState, d: &ExportDialog) -> Peaks<'a> {
    if d.include_peaks {
        Peaks {
            nodes: &state.swmm.node_peaks,
            links: &state.swmm.link_peaks,
        }
    } else {
        Peaks::default()
    }
}

/// Write `<dir>/<stem>_nodes.shp` and siblings. Returns the `.shp` paths.
pub fn export_shapefiles(state: &AppState, dir: &Path, stem: &str) -> Result<Vec<PathBuf>, String> {
    let d = settings(state);
    let crs = state.swmm_doc.gis.model_crs.as_ref();
    export::write_shapefiles(dir, stem, &state.swmm_doc.doc, &peaks(state, &d), crs).map_err(|e| e.to_string())
}

/// Write GeoJSON at `path` (combined) or beside it, one file per layer.
pub fn export_geojson(state: &AppState, path: &Path) -> Result<Vec<PathBuf>, String> {
    let d = settings(state);
    let crs = state.swmm_doc.gis.model_crs.as_ref();
    if d.wgs84 && crs.is_none() {
        return Err("reprojecting to WGS 84 needs a model CRS (File → Model CRS…)".into());
    }
    export::write_geojson(path, &state.swmm_doc.doc, &peaks(state, &d), crs, d.wgs84, d.combined).map_err(|e| e.to_string())
}

fn stem(state: &AppState) -> String {
    state
        .swmm_doc
        .path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".into())
}

fn run_export(state: &mut AppState) -> Option<String> {
    let d = settings(state);
    let start = state.swmm_doc.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
    let result = if d.geojson {
        let mut dlg = rfd::FileDialog::new().add_filter("GeoJSON", &["geojson"]).set_file_name(format!("{}.geojson", stem(state)));
        if let Some(dir) = &start {
            dlg = dlg.set_directory(dir);
        }
        let path = dlg.save_file()?;
        export_geojson(state, &path)
    } else {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = &start {
            dlg = dlg.set_directory(dir);
        }
        let dir = dlg.pick_folder()?;
        export_shapefiles(state, &dir, &stem(state))
    };
    Some(match result {
        Ok(paths) if paths.is_empty() => "Export GIS Layers: the model has nothing to export".into(),
        Ok(paths) => format!(
            "Exported {} file(s): {}",
            paths.len(),
            paths
                .iter()
                .map(|p| p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Err(e) => format!("Export GIS Layers: {e}"),
    })
}

/// The dialog window.
pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.gis.export.take() else { return };
    let has_crs = state.swmm_doc.gis.model_crs.is_some();
    let has_run = !state.swmm.node_peaks.is_empty() || !state.swmm.link_peaks.is_empty();
    let mut open = true;
    let mut action: Option<&str> = None;
    super::window(ctx, "Export GIS Layers", Vec2::new(460.0, 300.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Nodes, links, subcatchments and rain gages, each with its defining columns as attributes.");
            ui.horizontal(|ui| {
                if ui.selectable_label(!d.geojson, "Shapefile set").clicked() {
                    d.geojson = false;
                }
                if ui.selectable_label(d.geojson, "GeoJSON").clicked() {
                    d.geojson = true;
                }
            });
            if d.geojson {
                ui.add_enabled_ui(has_crs, |ui| {
                    ui.checkbox(&mut d.wgs84, "Reproject to WGS 84 (RFC 7946)");
                });
                ui.checkbox(&mut d.combined, "One combined file");
            } else {
                ui.label(RichText::new("One .shp/.shx/.dbf/.cpg set per object type, with a .prj when the model has a CRS.").small());
            }
            ui.add_enabled_ui(has_run, |ui| {
                ui.checkbox(&mut d.include_peaks, "Include run peaks");
            });
            if !has_run {
                ui.label(RichText::new("No run is loaded: no peak fields.").small());
            }
            if !has_crs {
                ui.label(
                    RichText::new("The model has no CRS: files carry no coordinate system (set one with Model CRS…).")
                        .small()
                        .color(Color32::from_rgb(200, 130, 40)),
                );
            }
            if let Some(r) = &d.result {
                ui.label(RichText::new(r).small());
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Export…").clicked() {
                    action = Some("export");
                }
                if ui.button("Close").clicked() {
                    action = Some("close");
                }
            });
        });
    if !has_crs && d.geojson {
        d.wgs84 = false;
    }
    match action {
        Some("close") => {}
        Some(_) => {
            state.swmm_doc.gis.export = Some(d);
            if let Some(r) = run_export(state) {
                state.status = r.clone();
                state.swmm_doc.gis.last_report = Some(r.clone());
                if let Some(d) = state.swmm_doc.gis.export.as_mut() {
                    d.result = Some(r);
                }
            }
        }
        None => {
            if open {
                state.swmm_doc.gis.export = Some(d);
            }
        }
    }
}
