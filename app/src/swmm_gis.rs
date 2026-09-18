// SPDX-License-Identifier: GPL-3.0-or-later

//! GIS in the editor: import of shapefiles and GeoJSON with field mapping,
//! coordinate systems from `.prj`, DEM loading, ground elevations from the
//! DEM, and vector layers drawn under the network. The hooks below are
//! called from the menus, layers pane and map; the readers, projections
//! and the import/export mapping live in `stormsewer_swmm::gis`.
//!
//! The model's coordinate system is kept in a `<model>.crs` sidecar
//! (`gis::sidecar`), never in the `.inp`, and is re-read whenever the
//! editor's path changes.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Button, Color32, FontId, Id, Pos2, Rect, RichText, Shape, Stroke, Ui, Vec2};
use stormsewer_swmm::gis::crs::Crs;
use stormsewer_swmm::gis::vector::{Geometry, Layer};
use stormsewer_swmm::gis::{sidecar, stateplane};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;
use crate::viewport::Viewport;

#[path = "swmm_gis_dem.rs"]
pub mod dem;
#[path = "swmm_gis_export.rs"]
pub mod export_dialog;
#[path = "swmm_gis_import.rs"]
pub mod import_dialog;

/// A vector layer kept on the map for reference, in model coordinates.
pub struct RefLayer {
    pub name: String,
    pub layer: Layer,
    pub color: Color32,
    pub visible: bool,
    /// Field drawn as a label beside each feature.
    pub label_field: Option<usize>,
    pub source: PathBuf,
}

/// The Model CRS dialog's draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CrsDialog {
    /// `EPSG:nnnn`, a bare code, or WKT.
    pub text: String,
    /// State Plane zone search.
    pub search: String,
    pub parsed: Option<Crs>,
    pub error: Option<String>,
}

/// Per-editor GIS state (loaded layers, dialogs, the DEM).
pub struct GisState {
    pub model_crs: Option<Crs>,
    /// The editor path the sidecar was last read for (`None` before the
    /// first sync).
    crs_loaded_for: Option<Option<PathBuf>>,
    /// A CRS was set while the model had no path: write the sidecar when
    /// it gets one.
    crs_pending: bool,
    pub layers: Vec<RefLayer>,
    pub dem: Option<dem::DemView>,
    pub import: Option<import_dialog::ImportDialog>,
    pub ground: Option<dem::GroundDialog>,
    pub export: Option<export_dialog::ExportDialog>,
    pub crs_dialog: Option<CrsDialog>,
    /// Show the cursor's longitude/latitude in the layers pane.
    pub show_cursor: bool,
    /// The last import/export report, for the layers pane.
    pub last_report: Option<String>,
}

impl Default for GisState {
    fn default() -> Self {
        Self {
            model_crs: None,
            crs_loaded_for: None,
            crs_pending: false,
            layers: Vec::new(),
            dem: None,
            import: None,
            ground: None,
            export: None,
            crs_dialog: None,
            show_cursor: true,
            last_report: None,
        }
    }
}

const LAYER_COLORS: [Color32; 6] = [
    Color32::from_rgb(180, 60, 200),
    Color32::from_rgb(30, 150, 160),
    Color32::from_rgb(200, 120, 30),
    Color32::from_rgb(60, 130, 60),
    Color32::from_rgb(160, 40, 60),
    Color32::from_rgb(90, 90, 200),
];

impl GisState {
    /// The next colour for a new reference layer.
    pub fn next_color(&self) -> Color32 {
        LAYER_COLORS[self.layers.len() % LAYER_COLORS.len()]
    }
}

// ---------------------------------------------------------------------------
// Model CRS and the sidecar
// ---------------------------------------------------------------------------

/// Re-read the sidecar when the editor's path changes; write a pending
/// CRS once the model has a path.
pub fn sync_model_crs(ed: &mut SwmmEditor) {
    let current = ed.path.clone();
    if ed.gis.crs_loaded_for.as_ref() == Some(&current) {
        return;
    }
    let came_from_unsaved = matches!(ed.gis.crs_loaded_for, Some(None));
    ed.gis.crs_loaded_for = Some(current.clone());
    match current {
        Some(path) => {
            if ed.gis.crs_pending && came_from_unsaved {
                if let Some(crs) = &ed.gis.model_crs {
                    let _ = sidecar::write(&path, crs);
                }
                ed.gis.crs_pending = false;
            } else {
                ed.gis.model_crs = sidecar::read(&path).ok().flatten();
                ed.gis.crs_pending = false;
            }
        }
        None => {
            if !ed.gis.crs_pending {
                ed.gis.model_crs = None;
            }
        }
    }
}

/// Set (or clear) the model CRS, writing or removing the sidecar when the
/// model has a path. Returns the status line.
pub fn set_model_crs(ed: &mut SwmmEditor, crs: Option<Crs>) -> String {
    sync_model_crs(ed);
    let label = crs.as_ref().map(Crs::label);
    ed.gis.model_crs = crs;
    match (&ed.path, &ed.gis.model_crs) {
        (Some(path), Some(c)) => match sidecar::write(path, c) {
            Ok(p) => format!("Model CRS set to {} (saved in {})", c.label(), p.display()),
            Err(e) => format!("Model CRS set to {} but the sidecar could not be written: {e}", c.label()),
        },
        (Some(path), None) => {
            let _ = sidecar::remove(path);
            "Model CRS cleared".into()
        }
        (None, Some(_)) => {
            ed.gis.crs_pending = true;
            format!(
                "Model CRS set to {} (the .crs sidecar is written when the model is saved)",
                label.unwrap_or_default()
            )
        }
        (None, None) => {
            ed.gis.crs_pending = false;
            "Model CRS cleared".into()
        }
    }
}

/// Keep textures and caches in step with the loaded layers.
pub fn sync(ctx: &egui::Context, ed: &mut SwmmEditor) {
    sync_model_crs(ed);
    dem::sync_texture(ctx, ed);
}

// ---------------------------------------------------------------------------
// Map drawing
// ---------------------------------------------------------------------------

fn w2s(vp: &Viewport, rect: Rect, p: (f64, f64)) -> Pos2 {
    vp.world_to_screen(rect, p.0, p.1)
}

/// Draw the GIS layers under the network (called after the backdrop).
pub fn draw(painter: &egui::Painter, rect: Rect, vp: &Viewport, ed: &SwmmEditor) {
    dem::draw(painter, rect, vp, ed);
    let font = FontId::proportional(11.0);
    for layer in ed.gis.layers.iter().filter(|l| l.visible) {
        let stroke = Stroke::new(1.5, layer.color);
        let fill = Color32::from_rgba_unmultiplied(layer.color.r(), layer.color.g(), layer.color.b(), 40);
        let text_color = layer.color;
        for f in &layer.layer.features {
            let Some(g) = &f.geometry else { continue };
            let Some((x0, y0, x1, y1)) = g.bounds() else { continue };
            let a = w2s(vp, rect, (x0, y1));
            let b = w2s(vp, rect, (x1, y0));
            if !rect.intersects(Rect::from_two_pos(a, b).expand(4.0)) {
                continue;
            }
            match g {
                Geometry::Point(x, y) => painter.circle(w2s(vp, rect, (*x, *y)), 4.0, fill, stroke),
                Geometry::MultiPoint(pts) => {
                    for p in pts {
                        painter.circle(w2s(vp, rect, *p), 4.0, fill, stroke);
                    }
                }
                Geometry::LineString(p) => {
                    painter.add(Shape::line(p.iter().map(|q| w2s(vp, rect, *q)).collect(), stroke));
                }
                Geometry::MultiLineString(parts) => {
                    for p in parts {
                        painter.add(Shape::line(p.iter().map(|q| w2s(vp, rect, *q)).collect(), stroke));
                    }
                }
                Geometry::Polygon(rings) => {
                    for r in rings {
                        painter.add(Shape::closed_line(r.iter().map(|q| w2s(vp, rect, *q)).collect(), stroke));
                    }
                }
                Geometry::MultiPolygon(polys) => {
                    for rings in polys {
                        for r in rings {
                            painter.add(Shape::closed_line(r.iter().map(|q| w2s(vp, rect, *q)).collect(), stroke));
                        }
                    }
                }
            }
            if let Some(i) = layer.label_field {
                if let (Some(v), Some(p)) = (f.values.get(i), g.anchor()) {
                    if !v.is_null() {
                        painter.text(
                            w2s(vp, rect, p) + Vec2::new(5.0, -2.0),
                            Align2::LEFT_BOTTOM,
                            v.to_field(),
                            font.clone(),
                            text_color,
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Layers pane
// ---------------------------------------------------------------------------

/// `35.22710° N, 80.84310° W`.
pub fn format_latlon(lon: f64, lat: f64) -> String {
    format!(
        "{:.5}° {}, {:.5}° {}",
        lat.abs(),
        if lat >= 0.0 { "N" } else { "S" },
        lon.abs(),
        if lon >= 0.0 { "E" } else { "W" }
    )
}

/// The layers pane section for GIS layers and the DEM.
pub fn layers_section(ui: &mut Ui, state: &mut AppState) {
    ui.separator();
    ui.label(RichText::new("GIS").strong());
    let crs_label = state
        .swmm_doc
        .gis
        .model_crs
        .as_ref()
        .map(Crs::label)
        .unwrap_or_else(|| "not set".into());
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("Map CRS: {crs_label}")).small());
        if ui.small_button("Model CRS…").clicked() {
            open_crs_dialog(state);
        }
    });
    if state.swmm_doc.gis.model_crs.is_some() {
        ui.checkbox(&mut state.swmm_doc.gis.show_cursor, "Cursor lat/lon");
        if state.swmm_doc.gis.show_cursor {
            let text = match (state.swmm_doc.gis.model_crs.as_ref(), state.swmm_doc.edit.cursor_world) {
                (Some(crs), Some((x, y))) => match crs.inverse(x, y) {
                    Ok((lon, lat)) => format!("Cursor: {}", format_latlon(lon, lat)),
                    Err(_) => "Cursor: outside the projection".into(),
                },
                _ => "Cursor: —".into(),
            };
            ui.label(RichText::new(text).small());
        }
    }
    dem::layers_rows(ui, state);
    let mut remove = None;
    let gis = &mut state.swmm_doc.gis;
    for (i, l) in gis.layers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut l.visible, l.name.as_str());
            ui.color_edit_button_srgba(&mut l.color);
            let current = l
                .label_field
                .and_then(|k| l.layer.fields.get(k))
                .map(|f| f.name.clone())
                .unwrap_or_else(|| "no labels".into());
            egui::ComboBox::from_id_salt(("gis-layer-label", i))
                .selected_text(current)
                .width(90.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut l.label_field, None, "no labels");
                    for (k, f) in l.layer.fields.iter().enumerate() {
                        ui.selectable_value(&mut l.label_field, Some(k), f.name.as_str());
                    }
                });
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
        ui.label(
            RichText::new(format!(
                "{} feature(s) from {}",
                l.layer.features.len(),
                l.source.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
            ))
            .small(),
        );
    }
    if let Some(i) = remove {
        gis.layers.remove(i);
    }
    if let Some(r) = &gis.last_report {
        ui.label(RichText::new(r.clone()).small());
    }
}

// ---------------------------------------------------------------------------
// File menu and dialogs
// ---------------------------------------------------------------------------

/// Open the right dialog for a dropped or opened file: `.shp`/`.geojson`/
/// `.json` start the import dialog, `.tif`/`.tiff`/`.asc` load a DEM.
/// Returns the status line.
pub fn open_path(state: &mut AppState, path: &Path) -> String {
    if stormsewer_swmm::gis::vector_format(path).is_some() {
        match import_dialog::open(state, path.to_path_buf()) {
            Ok(()) => format!("Import GIS Layer: {}", path.display()),
            Err(e) => format!("Could not read {}: {e}", path.display()),
        }
    } else if stormsewer_swmm::gis::raster_format(path).is_some() {
        match dem::load(state, path.to_path_buf()) {
            Ok(s) => s,
            Err(e) => format!("Could not read {}: {e}", path.display()),
        }
    } else {
        format!("{}: not a GIS file this editor reads", path.display())
    }
}

/// `File` menu: import and export entries.
pub fn file_menu_items(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.swmm_doc.loaded;
    if ui
        .add_enabled(loaded, Button::new("Import GIS Layer…"))
        .on_hover_text("Shapefile or GeoJSON features become nodes, conduits, subcatchments or rain gages, with a field-mapping table")
        .clicked()
    {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("GIS vector layer", &["shp", "geojson", "json"])
            .pick_file()
        {
            state.status = open_path(state, &path);
        }
        ui.close_menu();
    }
    if ui
        .add_enabled(loaded, Button::new("Import DEM…"))
        .on_hover_text("A GeoTIFF or ESRI ASCII grid drawn as hillshade under the map, and sampled for ground elevations")
        .clicked()
    {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Digital elevation model", &["tif", "tiff", "asc"])
            .pick_file()
        {
            state.status = open_path(state, &path);
        }
        ui.close_menu();
    }
    if ui
        .add_enabled(loaded && state.swmm_doc.gis.dem.is_some(), Button::new("Set Ground From DEM…"))
        .on_hover_text("Write each node's MaxDepth so invert + MaxDepth equals the DEM ground elevation")
        .clicked()
    {
        dem::open_ground(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(loaded, Button::new("Export GIS Layers…"))
        .on_hover_text("Nodes, links, subcatchments and gages as a shapefile set or GeoJSON, with run peaks when a run is loaded")
        .clicked()
    {
        export_dialog::open(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(loaded, Button::new("Model CRS…"))
        .on_hover_text("The coordinate system of the map, kept in a .crs sidecar beside the model")
        .clicked()
    {
        open_crs_dialog(state);
        ui.close_menu();
    }
    ui.separator();
}

pub fn open_crs_dialog(state: &mut AppState) {
    let text = state
        .swmm_doc
        .gis
        .model_crs
        .as_ref()
        .map(|c| c.epsg.map_or_else(|| c.to_wkt(), |e| format!("EPSG:{e}")))
        .unwrap_or_default();
    state.swmm_doc.gis.crs_dialog = Some(CrsDialog {
        text,
        ..Default::default()
    });
}

/// Parse the dialog's text into `parsed`/`error`.
pub fn parse_crs_dialog(d: &mut CrsDialog) {
    match Crs::parse(&d.text) {
        Ok(c) => {
            d.parsed = Some(c);
            d.error = None;
        }
        Err(e) => {
            d.parsed = None;
            d.error = Some(e.to_string());
        }
    }
}

/// Apply the dialog: set the model CRS to the parsed system. Returns the
/// status line, or None when nothing parsed.
pub fn apply_crs_dialog(state: &mut AppState) -> Option<String> {
    let mut d = state.swmm_doc.gis.crs_dialog.take()?;
    if d.parsed.is_none() {
        parse_crs_dialog(&mut d);
    }
    match d.parsed.take() {
        Some(c) => Some(set_model_crs(&mut state.swmm_doc, Some(c))),
        None => {
            state.swmm_doc.gis.crs_dialog = Some(d);
            None
        }
    }
}

pub(crate) fn window<'a>(ctx: &egui::Context, title: &'a str, size: Vec2) -> egui::Window<'a> {
    egui::Window::new(title)
        .id(Id::new(("swmm-gis-dialog", title)))
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .default_pos(ctx.screen_rect().center() - size / 2.0)
}

fn draw_crs_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.gis.crs_dialog.take() else { return };
    let mut open = true;
    let mut action: Option<&str> = None;
    window(ctx, "Model CRS", Vec2::new(520.0, 380.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("EPSG code (e.g. EPSG:2264), a State Plane zone below, or the WKT from a .prj file:");
            let r = ui.add(
                egui::TextEdit::multiline(&mut d.text)
                    .id(Id::new("swmm-gis-crs-text"))
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            if r.changed() {
                d.parsed = None;
                d.error = None;
            }
            ui.horizontal(|ui| {
                if ui.button("Parse").clicked() {
                    parse_crs_dialog(&mut d);
                }
                if let Some(c) = &d.parsed {
                    ui.label(RichText::new(c.describe()).small());
                }
                if let Some(e) = &d.error {
                    ui.colored_label(Color32::from_rgb(200, 60, 60), e);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("State Plane 1983 zone:");
                ui.add(egui::TextEdit::singleline(&mut d.search).id(Id::new("swmm-gis-crs-search")).hint_text("e.g. North Carolina"));
            });
            let search = d.search.trim().to_string();
            if !search.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("swmm-gis-crs-zones")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for z in stateplane::find_by_name(&search) {
                            ui.horizontal(|ui| {
                                if ui.selectable_label(false, format!("{} (metres, EPSG:{})", z.name, z.epsg)).clicked() {
                                    d.text = format!("EPSG:{}", z.epsg);
                                    parse_crs_dialog(&mut d);
                                }
                                if let Some((code, us)) = z.epsg_ft {
                                    let unit = if us { "US ft" } else { "intl ft" };
                                    if ui.selectable_label(false, format!("{unit}, EPSG:{code}")).clicked() {
                                        d.text = format!("EPSG:{code}");
                                        parse_crs_dialog(&mut d);
                                    }
                                }
                            });
                        }
                    });
            }
            ui.label(
                RichText::new(
                    "NAD83 and WGS 84 are treated as one datum (they differ by about 1–2 m). NAD27 systems are recognised but cannot be transformed.",
                )
                .small(),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Set").clicked() {
                    action = Some("set");
                }
                if ui.button("Clear CRS").clicked() {
                    action = Some("clear");
                }
                if ui.button("Cancel").clicked() {
                    action = Some("cancel");
                }
            });
        });
    match action {
        Some("set") => {
            state.swmm_doc.gis.crs_dialog = Some(d);
            if let Some(s) = apply_crs_dialog(state) {
                state.status = s;
            }
        }
        Some("clear") => state.status = set_model_crs(&mut state.swmm_doc, None),
        Some("cancel") => {}
        _ => {
            if open {
                state.swmm_doc.gis.crs_dialog = Some(d);
            }
        }
    }
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    import_dialog::draw(ctx, state);
    dem::draw_ground(ctx, state);
    export_dialog::draw(ctx, state);
    draw_crs_dialog(ctx, state);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::swmm_doc::LeftTab;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;
    use stormsewer_swmm::doc::build::ObjRef;
    use stormsewer_swmm::gis::import::Target;

    pub(crate) fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../swmm/tests/fixtures/gis")
            .join(name)
    }

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join("stormsewer-app-tests").join("gis").join(tag);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub(crate) fn app() -> StormSewerApp {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        run_frame(&mut app);
        app
    }

    #[test]
    fn model_crs_lives_in_a_sidecar_that_follows_the_model_path() {
        let mut app = app();
        let dir = temp_dir("sidecar");
        let model = dir.join("site.inp");
        app.state.swmm_doc.path = Some(model.clone());
        let nc = Crs::from_epsg(2264).unwrap();
        let s = set_model_crs(&mut app.state.swmm_doc, Some(nc));
        assert!(s.contains("EPSG:2264") && s.contains("site.crs"), "{s}");
        assert!(dir.join("site.crs").exists());
        // A new model (no path) has no CRS; reopening the path reads it back.
        app.state.swmm_doc.new_model();
        sync_model_crs(&mut app.state.swmm_doc);
        assert!(app.state.swmm_doc.gis.model_crs.is_none());
        app.state.swmm_doc.path = Some(model.clone());
        sync_model_crs(&mut app.state.swmm_doc);
        assert_eq!(app.state.swmm_doc.gis.model_crs.as_ref().and_then(|c| c.epsg), Some(2264));
        // Clearing removes the sidecar.
        set_model_crs(&mut app.state.swmm_doc, None);
        assert!(!dir.join("site.crs").exists());
        // Set with no path: pending until the model gets one.
        app.state.swmm_doc.new_model();
        sync_model_crs(&mut app.state.swmm_doc);
        let s = set_model_crs(&mut app.state.swmm_doc, Some(Crs::from_epsg(32617).unwrap()));
        assert!(s.contains("when the model is saved"), "{s}");
        let later = dir.join("later.inp");
        app.state.swmm_doc.path = Some(later.clone());
        sync_model_crs(&mut app.state.swmm_doc);
        assert!(dir.join("later.crs").exists());
        assert_eq!(app.state.swmm_doc.gis.model_crs.as_ref().and_then(|c| c.epsg), Some(32617));
        // The CRS dialog parses text and a zone search finds NC.
        open_crs_dialog(&mut app.state);
        let mut d = app.state.swmm_doc.gis.crs_dialog.take().unwrap();
        assert_eq!(d.text, "EPSG:32617");
        d.text = "EPSG:2264".into();
        parse_crs_dialog(&mut d);
        assert_eq!(d.parsed.as_ref().and_then(|c| c.epsg), Some(2264));
        d.text = "nonsense".into();
        parse_crs_dialog(&mut d);
        assert!(d.error.is_some());
        d.text = "EPSG:2264".into();
        d.search = "carolina".into();
        app.state.swmm_doc.gis.crs_dialog = Some(d);
        run_frame(&mut app);
        assert!(app.state.swmm_doc.gis.crs_dialog.is_some(), "dialog stays open");
        assert!(apply_crs_dialog(&mut app.state).is_some());
        assert_eq!(app.state.swmm_doc.gis.model_crs.as_ref().and_then(|c| c.epsg), Some(2264));
        assert_eq!(format_latlon(-80.8431, 35.2271), "35.22710° N, 80.84310° W");
    }

    #[test]
    fn importing_points_then_lines_from_the_fixtures_is_one_undo_step_each() {
        let mut app = app();
        assert!(app.state.swmm_doc.nodes.is_empty());
        import_dialog::open(&mut app.state, fixture("manholes.shp")).unwrap();
        {
            let d = app.state.swmm_doc.gis.import.as_ref().unwrap();
            assert_eq!(d.target, Target::Junction);
            assert_eq!(d.layer.features.len(), 3);
            assert_eq!(d.name_field, Some(0), "NAME column picked");
            assert!(d.maps.iter().any(|m| m.column == "Elevation" && m.choice == import_dialog::Choice::Field(1)), "INVERT → Elevation");
            assert!(d.set_model_crs, "layer has a CRS, the model none: offered");
        }
        run_frame(&mut app);
        assert!(app.state.swmm_doc.gis.import.is_some(), "dialog drew and stayed open");
        let before = app.state.swmm_doc.undo_depth();
        let report = import_dialog::apply(&mut app.state).unwrap();
        assert!(report.contains("3 junction(s)"), "{report}");
        assert_eq!(app.state.swmm_doc.undo_depth(), before + 1);
        assert_eq!(app.state.swmm_doc.undo_label(), Some("import GIS layer"));
        assert_eq!(app.state.swmm_doc.nodes.len(), 3);
        let doc = &app.state.swmm_doc.doc;
        assert_eq!(doc.field("JUNCTIONS", "MH-1", "Elevation"), Some("700.25"));
        assert_eq!(doc.coordinates("MH-2"), Some((1449720.4, 542689.1)));
        assert!(doc.contains("JUNCTIONS", "Café"));
        assert_eq!(app.state.swmm_doc.gis.model_crs.as_ref().and_then(|c| c.epsg), Some(2264), "model CRS taken from the .prj");
        assert_eq!(app.state.swmm_doc.selection.len(), 3, "created objects selected");
        assert!(app.state.swmm_doc.gis.import.is_none());

        // Conduits snap to those nodes; the far ends get junctions.
        import_dialog::open(&mut app.state, fixture("pipes.shp")).unwrap();
        {
            let d = app.state.swmm_doc.gis.import.as_mut().unwrap();
            assert_eq!(d.target, Target::Conduit);
            assert!(!d.set_model_crs && !d.reproject, "same CRS as the model");
            d.name_field = Some(0);
            d.snap = "0.5".into();
            for m in &mut d.maps {
                if m.column == "Geom1" {
                    m.choice = import_dialog::Choice::Field(1);
                }
                if m.column == "Roughness" {
                    m.choice = import_dialog::Choice::Field(2);
                }
            }
            d.keep_reference = true;
        }
        import_dialog::preview(&mut app.state);
        let plan = app.state.swmm_doc.gis.import.as_ref().unwrap().preview.clone().unwrap();
        assert_eq!(plan.items.len(), 2, "{:?}", plan.skipped);
        let before = app.state.swmm_doc.undo_depth();
        let report = import_dialog::apply(&mut app.state).unwrap();
        assert!(report.contains("2 conduit(s)"), "{report}");
        assert_eq!(app.state.swmm_doc.undo_depth(), before + 1);
        let doc = &app.state.swmm_doc.doc;
        assert_eq!(stormsewer_swmm::doc::build::link_ends(doc, "P1"), Some(("MH-1".into(), "MH-2".into())));
        assert_eq!(doc.field("XSECTIONS", "P1", "Geom1"), Some("24"));
        assert_eq!(doc.field("CONDUITS", "P2", "Roughness"), Some("0.012"));
        // P2 is multipart: its longer part is the route, and both of its
        // ends sit on manholes (the short second part is dropped).
        let (from, to) = stormsewer_swmm::doc::build::link_ends(doc, "P2").unwrap();
        assert_eq!(from, "MH-2");
        assert_eq!(to, "Café");
        assert!(!report.contains("at conduit ends"), "no end junctions needed: {report}");
        let len: f64 = doc.field("CONDUITS", "P1", "Length").unwrap().parse().unwrap();
        // US-foot CRS on a CFS model: lengths are the drawn feet (× 1.000002).
        let drawn = ((50.0f64).powi(2) + (10.9f64).powi(2)).sqrt() * 2.0;
        assert!((len - drawn).abs() < 0.01, "{len} vs {drawn}");
        assert_eq!(app.state.swmm_doc.gis.layers.len(), 1, "kept as a reference layer");
        assert_eq!(app.state.swmm_doc.gis.layers[0].name, "pipes");
        // The layers pane draws the reference layer and the CRS line.
        app.state.swmm_doc.left_tab = LeftTab::Layers;
        app.state.swmm_doc.gis.layers[0].label_field = Some(0);
        app.state.swmm_doc.edit.cursor_world = Some((1449620.4, 542689.1));
        run_frame(&mut app);
        // Undo removes both conduits and the created junction together.
        assert!(app.state.swmm_doc.undo().is_some());
        assert!(!app.state.swmm_doc.doc.contains("CONDUITS", "P1"));
        assert!(!app.state.swmm_doc.doc.contains("JUNCTIONS", &to));
        assert_eq!(app.state.swmm_doc.doc.names("JUNCTIONS").len(), 3);
    }

    #[test]
    fn polygons_from_geojson_become_subcatchments_with_areas_in_acres() {
        let mut app = app();
        app.state.swmm_doc.path = Some(temp_dir("geojson-import").join("m.inp"));
        set_model_crs(&mut app.state.swmm_doc, Some(Crs::from_epsg(2264).unwrap()));
        import_dialog::open(&mut app.state, fixture("manholes.shp")).unwrap();
        import_dialog::apply(&mut app.state).unwrap();
        import_dialog::open(&mut app.state, fixture("basins.geojson")).unwrap();
        {
            let d = app.state.swmm_doc.gis.import.as_ref().unwrap();
            assert_eq!(d.target, Target::Subcatchment);
            assert!(d.outlet_nearest);
        }
        let report = import_dialog::apply(&mut app.state).unwrap();
        assert!(report.contains("2 subcatchment(s)"), "{report}");
        let doc = &app.state.swmm_doc.doc;
        let area: f64 = doc.field("SUBCATCHMENTS", "B1", "Area").unwrap().parse().unwrap();
        // 660 ft square less a 100 ft square hole = 425,600 sq ft = 9.77 acres.
        assert!((area - 425600.0 / 43560.0).abs() < 0.01, "{area}");
        assert_eq!(doc.field("SUBCATCHMENTS", "B1", "Outlet"), Some("MH-1"));
        assert_eq!(doc.polygon("B1").len(), 4);
        // A WGS 84 layer into a State Plane model is reprojected.
        import_dialog::open(&mut app.state, fixture("gages_wgs84.geojson")).unwrap();
        {
            let d = app.state.swmm_doc.gis.import.as_mut().unwrap();
            assert!(d.reproject, "different CRS: reprojection on by default");
            d.target = Target::RainGage;
            d.name_field = Some(d.layer.field_index("id").unwrap());
        }
        import_dialog::apply(&mut app.state).unwrap();
        let (x, y) = app.state.swmm_doc.doc.symbol("RG1").unwrap();
        assert!((x - 1449620.4).abs() < 0.01 && (y - 542689.07).abs() < 0.01, "{x} {y}");
    }

    #[test]
    fn dem_loads_draws_and_sets_ground_depths_as_one_step() {
        let mut app = app();
        app.state.swmm_doc.path = Some(temp_dir("dem").join("m.inp"));
        set_model_crs(&mut app.state.swmm_doc, Some(Crs::from_epsg(2264).unwrap()));
        import_dialog::open(&mut app.state, fixture("manholes.shp")).unwrap();
        import_dialog::apply(&mut app.state).unwrap();
        let s = dem::load(&mut app.state, fixture("dem_deflate_pred3_f32.tif")).unwrap();
        assert!(s.contains("Deflate"), "{s}");
        let ctx = egui::Context::default();
        sync(&ctx, &mut app.state.swmm_doc);
        assert!(app.state.swmm_doc.gis.dem.as_ref().unwrap().texture.is_some());
        // The DEM covers x 1449000..1449030, y 541975..542000 — none of
        // the manholes (y 542689) fall on it, so move one onto the grid.
        app.state
            .swmm_doc
            .apply(stormsewer_swmm::doc::Command::MoveNode { name: "MH-1".into(), x: 1449012.5, y: 541987.5 }, "move");
        app.state.swmm_doc.refresh();
        // DEM value at column 2, row 2 = 100.25 + 3 + 0.5 = 103.75; invert 700.25
        // is above ground so the depth clamps to 0 with a note; set a lower
        // invert first.
        app.state.swmm_doc.apply(
            stormsewer_swmm::doc::Command::SetField { section: "JUNCTIONS".into(), name: "MH-1".into(), field: "Elevation".into(), value: "95.5".into() },
            "invert",
        );
        dem::open_ground(&mut app.state);
        {
            let g = app.state.swmm_doc.gis.ground.as_ref().unwrap();
            assert!(g.all_nodes);
            assert_eq!(g.rows.len(), 3);
            let mh1 = g.rows.iter().find(|r| r.name == "MH-1").unwrap();
            assert_eq!(mh1.ground, Some(103.75));
            assert_eq!(mh1.new_max_depth, Some(8.25));
            assert!(g.rows.iter().filter(|r| r.name != "MH-1").all(|r| r.new_max_depth.is_none()));
        }
        run_frame(&mut app);
        let before = app.state.swmm_doc.undo_depth();
        let s = dem::apply_ground(&mut app.state).unwrap();
        assert!(s.contains("1 node"), "{s}");
        assert_eq!(app.state.swmm_doc.undo_depth(), before + 1);
        assert_eq!(app.state.swmm_doc.doc.field("JUNCTIONS", "MH-1", "MaxDepth"), Some("8.25"));
        assert!(app.state.swmm_doc.gis.ground.is_none());
        // Selection scope with a factor: DEM in metres, model in feet.
        app.state.swmm_doc.select_many(vec![ObjRef::Node("MH-1".into())]);
        dem::open_ground(&mut app.state);
        {
            let g = app.state.swmm_doc.gis.ground.as_mut().unwrap();
            g.all_nodes = false;
            g.factor = "3.2808".into();
        }
        dem::recompute(&mut app.state);
        let g = app.state.swmm_doc.gis.ground.as_ref().unwrap();
        assert_eq!(g.rows.len(), 1);
        assert!((g.rows[0].ground.unwrap() - 103.75 * 3.2808).abs() < 1e-9);
        // Elevation tint mode rebuilds the texture; the layers pane draws it.
        app.state.swmm_doc.gis.dem.as_mut().unwrap().mode = dem::DemMode::Elevation;
        app.state.swmm_doc.left_tab = LeftTab::Layers;
        run_frame(&mut app);
        assert!(app.state.swmm_doc.gis.dem.as_ref().unwrap().texture.is_some());
        // An ASCII grid loads too, and a bad file reports.
        let asc = temp_dir("dem-asc").join("g.asc");
        std::fs::write(&asc, "ncols 2\nnrows 2\nxllcorner 0\nyllcorner 0\ncellsize 10\n1 2\n3 4\n").unwrap();
        assert!(dem::load(&mut app.state, asc).is_ok());
        assert!(dem::load(&mut app.state, fixture("manholes.shp")).is_err());
    }

    #[test]
    fn export_writes_shapefiles_and_geojson_with_the_model_crs() {
        let mut app = app();
        let dir = temp_dir("export");
        app.state.swmm_doc.path = Some(dir.join("m.inp"));
        set_model_crs(&mut app.state.swmm_doc, Some(Crs::from_epsg(2264).unwrap()));
        import_dialog::open(&mut app.state, fixture("manholes.shp")).unwrap();
        import_dialog::apply(&mut app.state).unwrap();
        import_dialog::open(&mut app.state, fixture("pipes.shp")).unwrap();
        import_dialog::apply(&mut app.state).unwrap();
        app.state.swmm.node_peaks = vec![stormsewer_swmm::out::NodePeak {
            id: "MH-1".into(),
            max_depth: 2.5,
            depth_at_s: 0.0,
            max_total_inflow: 1.0,
            max_flooding: 0.0,
        }];
        export_dialog::open(&mut app.state);
        run_frame(&mut app);
        assert!(app.state.swmm_doc.gis.export.is_some());
        let written = export_dialog::export_shapefiles(&app.state, &dir, "m").unwrap();
        assert_eq!(written.len(), 2, "nodes and links only: {written:?}");
        let nodes = stormsewer_swmm::gis::shapefile::read(&dir.join("m_nodes.shp")).unwrap();
        assert_eq!(nodes.features.len(), 4, "3 manholes + 1 conduit-end junction");
        assert_eq!(nodes.crs.as_ref().and_then(|c| c.epsg), Some(2264));
        let mh1 = nodes.features.iter().position(|f| f.values[0] == stormsewer_swmm::gis::vector::FieldValue::Text("MH-1".into())).unwrap();
        assert_eq!(nodes.value(mh1, "PEAKDEPTH"), Some(&stormsewer_swmm::gis::vector::FieldValue::Number(2.5)));
        app.state.swmm_doc.gis.export.as_mut().unwrap().wgs84 = true;
        app.state.swmm_doc.gis.export.as_mut().unwrap().combined = true;
        let g = export_dialog::export_geojson(&app.state, &dir.join("m.geojson")).unwrap();
        assert_eq!(g.len(), 1);
        let text = std::fs::read_to_string(&g[0]).unwrap();
        assert!(!text.contains("\"crs\""));
        let back = stormsewer_swmm::gis::geojson::parse(&text, "m").unwrap();
        assert_eq!(back.features.len(), 6);
        let (x0, _, x1, _) = back.bounds().unwrap();
        assert!(x0 > -81.0 && x1 < -80.0, "reprojected to longitude: {x0}..{x1}");
        // Without a run and without WGS 84, the crs member is declared.
        app.state.swmm.node_peaks.clear();
        app.state.swmm_doc.gis.export.as_mut().unwrap().wgs84 = false;
        let g = export_dialog::export_geojson(&app.state, &dir.join("m2.geojson")).unwrap();
        let text = std::fs::read_to_string(&g[0]).unwrap();
        assert!(text.contains("EPSG::2264"));
    }

    #[test]
    fn dropped_gis_files_open_the_right_dialog() {
        let mut app = app();
        let ctx = egui::Context::default();
        app.state.open_any_path(&ctx, fixture("manholes.shp"));
        assert!(app.state.swmm_doc.gis.import.is_some(), "{}", app.state.status);
        app.state.swmm_doc.gis.import = None;
        app.state.open_any_path(&ctx, fixture("basins.geojson"));
        assert!(app.state.swmm_doc.gis.import.is_some(), "{}", app.state.status);
        app.state.open_any_path(&ctx, fixture("dem_lzw_pred2_u16.tif"));
        assert!(app.state.swmm_doc.gis.dem.is_some(), "{}", app.state.status);
        assert!(app.state.status.contains("LZW"), "{}", app.state.status);
        // The menu draws with a model open.
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                file_menu_items(ui, &mut state);
            });
        });
    }
}
