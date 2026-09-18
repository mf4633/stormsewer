// SPDX-License-Identifier: GPL-3.0-or-later

//! The DEM: a GeoTIFF or ESRI ASCII grid drawn under the map as a
//! hillshade or elevation tint, and sampled by Set Ground From DEM, which
//! writes each node's `MaxDepth` so that invert + MaxDepth is the ground.
//!
//! The texture is a runtime cache rebuilt when the display mode changes;
//! the grid is decimated to at most [`MAX_TEXTURE`] pixels a side for
//! drawing only (sampling always reads the full grid).

use std::path::PathBuf;

use eframe::egui::{self, Color32, Id, Pos2, Rect, RichText, TextureHandle, Ui, Vec2};
use stormsewer_swmm::doc::build::NodeType;
use stormsewer_swmm::doc::format_number;
use stormsewer_swmm::gis::crs::{transform, Crs};
use stormsewer_swmm::gis::import::{self, GroundRow};
use stormsewer_swmm::gis::raster::Raster;
use stormsewer_swmm::gis::{self as gis, RasterFormat};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;
use crate::viewport::Viewport;

/// The largest texture side drawn; bigger grids are decimated for display.
pub const MAX_TEXTURE: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemMode {
    /// Grey hillshade (Horn's method, sun from the north-west at 45°).
    Hillshade,
    /// Colour by elevation, shaded by the hillshade.
    Elevation,
}

/// A loaded DEM and how it is shown.
pub struct DemView {
    pub name: String,
    pub path: PathBuf,
    pub raster: Raster,
    /// The DEM's own CRS, when the file declares one.
    pub crs: Option<Crs>,
    /// How the file was stored (size, type, compression).
    pub info: String,
    pub mode: DemMode,
    pub visible: bool,
    pub opacity: f32,
    pub texture: Option<TextureHandle>,
    /// The mode `texture` was built for.
    built: Option<DemMode>,
}

/// Read a DEM and make it the loaded one. Returns the status line.
pub fn load(state: &mut AppState, path: PathBuf) -> Result<String, String> {
    let (raster, crs, info) = match gis::raster_format(&path) {
        Some(RasterFormat::GeoTiff) => {
            let g = gis::geotiff::read(&path).map_err(|e| e.to_string())?;
            (g.raster, g.crs, format!("GeoTIFF {}", g.info))
        }
        Some(RasterFormat::EsriAscii) => {
            let (r, crs) = gis::read_raster(&path).map_err(|e| e.to_string())?;
            let info = format!("{}x{} ESRI ASCII grid, cell {}", r.ncols, r.nrows, format_number(r.cell));
            (r, crs, info)
        }
        None => return Err("not a GeoTIFF (.tif) or ESRI ASCII grid (.asc)".into()),
    };
    if raster.ncols == 0 || raster.nrows == 0 {
        return Err("the grid is empty".into());
    }
    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "DEM".into());
    let mut status = format!("DEM {name}: {info}");
    match (&crs, &state.swmm_doc.gis.model_crs) {
        (Some(d), Some(m)) if !d.datum.compatible(&m.datum) => {
            status.push_str(&format!("; its datum ({}) cannot be transformed to the model's — drawn unprojected", d.datum.label()));
        }
        (Some(d), Some(m)) if !d.same_as(m) => status.push_str(&format!("; reprojected from {}", d.label())),
        (Some(d), None) => status.push_str(&format!("; DEM CRS {} (the model has none: set Model CRS… to match)", d.label())),
        (None, _) => status.push_str("; no CRS declared, taken to be the model's"),
        _ => {}
    }
    if let Some((lo, hi)) = raster.range() {
        status.push_str(&format!("; elevations {} to {}", format_number(lo), format_number(hi)));
    }
    state.swmm_doc.gis.dem = Some(DemView {
        name,
        path,
        raster,
        crs,
        info,
        mode: DemMode::Hillshade,
        visible: true,
        opacity: 0.8,
        texture: None,
        built: None,
    });
    state.swmm_doc.gis.ground = None;
    Ok(status)
}

/// The grid decimated so neither side exceeds [`MAX_TEXTURE`].
fn decimated(r: &Raster) -> Raster {
    let step = r.ncols.max(r.nrows).div_ceil(MAX_TEXTURE).max(1);
    if step == 1 {
        return r.clone();
    }
    let nc = r.ncols.div_ceil(step);
    let nr = r.nrows.div_ceil(step);
    let mut data = Vec::with_capacity(nc * nr);
    for row in 0..nr {
        for col in 0..nc {
            data.push(r.data[(row * step) * r.ncols + col * step]);
        }
    }
    Raster {
        ncols: nc,
        nrows: nr,
        x0: r.x0,
        y0: r.y1() - nr as f64 * r.cell * step as f64,
        cell: r.cell * step as f64,
        nodata: r.nodata,
        data,
    }
}

/// Elevation tint: green lowlands through tan to white peaks.
fn ramp(t: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 4] = [
        (0.0, [70.0, 140.0, 80.0]),
        (0.4, [190.0, 190.0, 110.0]),
        (0.75, [160.0, 110.0, 70.0]),
        (1.0, [245.0, 245.0, 245.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if t <= b {
            let k = (t - a) / (b - a);
            return [ca[0] + k * (cb[0] - ca[0]), ca[1] + k * (cb[1] - ca[1]), ca[2] + k * (cb[2] - ca[2])];
        }
    }
    STOPS[3].1
}

/// The image for a mode: one pixel per (decimated) cell, no-data clear.
pub fn image(r: &Raster, mode: DemMode) -> egui::ColorImage {
    let g = decimated(r);
    let shade = g.hillshade(315.0, 45.0, 1.0);
    let (lo, hi) = g.range().unwrap_or((0.0, 1.0));
    let span = if hi > lo { hi - lo } else { 1.0 };
    let mut px = Vec::with_capacity(g.data.len());
    for (v, s) in g.data.iter().zip(&shade) {
        if v.is_nan() {
            px.push(Color32::TRANSPARENT);
            continue;
        }
        // Edge cells whose neighbours are missing get a flat mid-grey.
        let s = if s.is_nan() { 0.7 } else { *s };
        let c = match mode {
            DemMode::Hillshade => {
                let k = (s * 255.0).round() as u8;
                [k, k, k]
            }
            DemMode::Elevation => {
                let base = ramp(((v - lo) / span) as f32);
                let m = 0.55 + 0.45 * s;
                [(base[0] * m) as u8, (base[1] * m) as u8, (base[2] * m) as u8]
            }
        };
        px.push(Color32::from_rgb(c[0], c[1], c[2]));
    }
    egui::ColorImage {
        size: [g.ncols, g.nrows],
        pixels: px,
    }
}

/// Build the texture when the DEM or its mode changed.
pub fn sync_texture(ctx: &egui::Context, ed: &mut SwmmEditor) {
    let Some(d) = ed.gis.dem.as_mut() else { return };
    if d.texture.is_some() && d.built == Some(d.mode) {
        return;
    }
    let img = image(&d.raster, d.mode);
    d.texture = Some(ctx.load_texture(format!("swmm-gis-dem-{}", d.path.display()), img, egui::TextureOptions::LINEAR));
    d.built = Some(d.mode);
}

/// A DEM coordinate in model coordinates (identity when either CRS is
/// unknown or they agree).
fn dem_to_model(d: &DemView, model: Option<&Crs>, p: (f64, f64)) -> Option<(f64, f64)> {
    match (&d.crs, model) {
        (Some(from), Some(to)) if !from.same_as(to) && from.datum.compatible(&to.datum) => transform(from, to, p.0, p.1).ok(),
        _ => Some(p),
    }
}

/// A model coordinate in the DEM's coordinates.
fn model_to_dem(d: &DemView, model: Option<&Crs>, p: (f64, f64)) -> Option<(f64, f64)> {
    match (&d.crs, model) {
        (Some(to), Some(from)) if !from.same_as(to) && from.datum.compatible(&to.datum) => transform(from, to, p.0, p.1).ok(),
        _ => Some(p),
    }
}

/// Paint the DEM under the network. A DEM in another CRS is drawn as the
/// quadrilateral its corners project to.
pub fn draw(painter: &egui::Painter, rect: Rect, vp: &Viewport, ed: &SwmmEditor) {
    let Some(d) = ed.gis.dem.as_ref() else { return };
    if !d.visible {
        return;
    }
    let Some(tex) = &d.texture else { return };
    let model = ed.gis.model_crs.as_ref();
    let (x0, y0, x1, y1) = d.raster.bounds();
    let corners = [(x0, y1), (x1, y1), (x1, y0), (x0, y0)];
    let mut pts = [Pos2::ZERO; 4];
    for (k, c) in corners.iter().enumerate() {
        let Some((x, y)) = dem_to_model(d, model, *c) else { return };
        pts[k] = vp.world_to_screen(rect, x, y);
    }
    let bb = Rect::from_points(&pts);
    if !rect.intersects(bb) {
        return;
    }
    let tint = Color32::from_white_alpha((d.opacity.clamp(0.0, 1.0) * 255.0) as u8);
    let uvs = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (p, uv) in pts.iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// The DEM's rows in the layers pane.
pub fn layers_rows(ui: &mut Ui, state: &mut AppState) {
    let mut remove = false;
    let Some(d) = state.swmm_doc.gis.dem.as_mut() else { return };
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.visible, format!("DEM: {}", d.name));
        if ui.small_button("Remove").clicked() {
            remove = true;
        }
    });
    ui.horizontal(|ui| {
        if ui.selectable_label(d.mode == DemMode::Hillshade, "Hillshade").clicked() {
            d.mode = DemMode::Hillshade;
        }
        if ui.selectable_label(d.mode == DemMode::Elevation, "Elevation tint").clicked() {
            d.mode = DemMode::Elevation;
        }
        ui.add(egui::Slider::new(&mut d.opacity, 0.0..=1.0).show_value(false)).on_hover_text("DEM opacity");
    });
    ui.label(RichText::new(&d.info).small());
    if remove {
        state.swmm_doc.gis.dem = None;
        state.swmm_doc.gis.ground = None;
    }
}

// ---------------------------------------------------------------------------
// Set Ground From DEM
// ---------------------------------------------------------------------------

/// The Set Ground From DEM dialog's draft.
pub struct GroundDialog {
    /// Every node, or only the selected ones.
    pub all_nodes: bool,
    /// Multiplies DEM values into the model's elevation unit.
    pub factor: String,
    pub rows: Vec<GroundRow>,
    pub error: Option<String>,
}

/// DEM elevations to model elevations: the DEM's linear unit over the
/// model's (feet for US flow units, metres for SI). Exactly 1 when the
/// two agree within survey-foot noise or the DEM's unit is unknown.
fn default_factor(state: &AppState) -> f64 {
    let Some(d) = &state.swmm_doc.gis.dem else { return 1.0 };
    let Some(c) = d.crs.as_ref().filter(|c| !c.is_geographic()) else { return 1.0 };
    let units = import::UnitFactors::from_doc(&state.swmm_doc.doc, None);
    let model_m = if units.us { 0.3048 } else { 1.0 };
    let f = c.unit.to_metre / model_m;
    if (f - 1.0).abs() < 1e-5 {
        1.0
    } else {
        f
    }
}

/// Open the dialog with rows for every node.
pub fn open_ground(state: &mut AppState) {
    if state.swmm_doc.gis.dem.is_none() {
        state.status = "Set Ground From DEM: load a DEM first (File → Import DEM…)".into();
        return;
    }
    state.swmm_doc.gis.ground = Some(GroundDialog {
        all_nodes: true,
        factor: format_number(default_factor(state)),
        rows: Vec::new(),
        error: None,
    });
    recompute(state);
}

/// Recompute the before/after rows from the draft's scope and factor.
pub fn recompute(state: &mut AppState) {
    let Some(mut g) = state.swmm_doc.gis.ground.take() else { return };
    let ed = &state.swmm_doc;
    let names: Vec<String> = if g.all_nodes {
        NodeType::ALL.iter().flat_map(|k| ed.doc.names(k.section())).collect()
    } else {
        ed.selected_nodes()
    };
    match (g.factor.trim().parse::<f64>(), ed.gis.dem.as_ref()) {
        (Ok(f), Some(d)) if f.is_finite() && f > 0.0 => {
            let model = ed.gis.model_crs.as_ref();
            let sample = |x: f64, y: f64| model_to_dem(d, model, (x, y)).and_then(|(u, v)| d.raster.sample_bilinear(u, v));
            g.rows = import::ground_rows(&ed.doc, &names, &sample, f);
            g.error = None;
        }
        (_, None) => {
            g.rows.clear();
            g.error = Some("no DEM is loaded".into());
        }
        _ => {
            g.rows.clear();
            g.error = Some(format!("factor {:?} is not a positive number", g.factor));
        }
    }
    state.swmm_doc.gis.ground = Some(g);
}

/// Write the new depths as one undo step and close the dialog.
pub fn apply_ground(state: &mut AppState) -> Result<String, String> {
    recompute(state);
    let Some(g) = state.swmm_doc.gis.ground.as_ref() else {
        return Err("Set Ground From DEM is not open".into());
    };
    if let Some(e) = &g.error {
        return Err(e.clone());
    }
    let cmds = import::ground_commands(&g.rows);
    if cmds.is_empty() {
        return Err("no node's MaxDepth changes".into());
    }
    let n = cmds.len();
    let label = "set ground from DEM";
    let ed = &mut state.swmm_doc;
    ed.begin_gesture(label);
    let mut failed = 0;
    for c in cmds {
        if !ed.apply(c, label) {
            failed += 1;
        }
    }
    ed.end_gesture();
    ed.refresh();
    ed.gis.ground = None;
    let mut s = format!("MaxDepth set from the DEM on {} node(s)", n - failed);
    if failed > 0 {
        s.push_str(&format!(", {failed} failed"));
    }
    ed.gis.last_report = Some(s.clone());
    Ok(s)
}

fn num(v: Option<f64>) -> String {
    v.map(format_number).unwrap_or_else(|| "—".into())
}

/// The dialog window.
pub fn draw_ground(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut g) = state.swmm_doc.gis.ground.take() else { return };
    let mut open = true;
    let mut action: Option<&str> = None;
    let mut dirty = false;
    super::window(ctx, "Set Ground From DEM", Vec2::new(600.0, 440.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("MaxDepth = DEM ground − invert, so each node's rim sits on the ground surface.");
            ui.horizontal(|ui| {
                if ui.selectable_label(g.all_nodes, "All nodes").clicked() && !g.all_nodes {
                    g.all_nodes = true;
                    dirty = true;
                }
                if ui.selectable_label(!g.all_nodes, "Selected nodes").clicked() && g.all_nodes {
                    g.all_nodes = false;
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("DEM-to-model elevation factor:");
                if ui
                    .add(egui::TextEdit::singleline(&mut g.factor).id(Id::new("swmm-gis-ground-factor")).desired_width(80.0))
                    .changed()
                {
                    dirty = true;
                }
                ui.label(RichText::new("3.2808 for a DEM in metres on a model in feet").small());
            });
            if let Some(e) = &g.error {
                ui.colored_label(Color32::from_rgb(200, 60, 60), e);
            }
            let changes = import::ground_commands(&g.rows).len();
            ui.label(format!("{} node(s), {changes} MaxDepth change(s)", g.rows.len()));
            egui::ScrollArea::vertical()
                .id_salt("swmm-gis-ground-rows")
                .max_height(260.0)
                .show(ui, |ui| {
                    egui::Grid::new("swmm-gis-ground-grid").num_columns(6).striped(true).show(ui, |ui| {
                        for h in ["Node", "Invert", "Ground", "MaxDepth now", "New MaxDepth", "Note"] {
                            ui.label(RichText::new(h).strong());
                        }
                        ui.end_row();
                        for r in &g.rows {
                            ui.label(&r.name);
                            ui.label(num(r.invert));
                            ui.label(num(r.ground));
                            ui.label(num(r.old_max_depth));
                            ui.label(num(r.new_max_depth));
                            ui.label(RichText::new(&r.note).small());
                            ui.end_row();
                        }
                    });
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(changes > 0, egui::Button::new("Write MaxDepth")).clicked() {
                    action = Some("apply");
                }
                if ui.button("Cancel").clicked() {
                    action = Some("cancel");
                }
            });
        });
    match action {
        Some("cancel") => {}
        Some(_) => {
            state.swmm_doc.gis.ground = Some(g);
            match apply_ground(state) {
                Ok(s) => state.status = s,
                Err(e) => state.status = format!("Set Ground From DEM: {e}"),
            }
        }
        None => {
            if open {
                state.swmm_doc.gis.ground = Some(g);
                if dirty {
                    recompute(state);
                }
            }
        }
    }
}
