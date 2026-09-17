// SPDX-License-Identifier: GPL-3.0-or-later

//! The map backdrop: a PNG or JPEG placed in model coordinates, kept in
//! the `.inp`'s `[BACKDROP]` section (so it travels with the model and the
//! EPA GUI reads it too), georeferenced from a sibling world file
//! (`.pgw`, `.jgw`, `.wld`) when there is one and by hand when there is
//! not; plus the `[MAP] DIMENSIONS` dialog with its set-from buttons.
//!
//! Loading and georeferencing are one undo step each; the texture is a
//! runtime cache rebuilt whenever the `FILE` line changes.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Id, Pos2, Rect, RichText, TextureHandle, Ui, Vec2};
use stormsewer_swmm::backdrop::{self, Backdrop, WorldFile};
use stormsewer_swmm::doc::{format_number, Command};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;

/// The loaded picture and the settings that are not the model's.
#[derive(Default)]
pub struct BackdropView {
    pub texture: Option<TextureHandle>,
    /// Pixel size of the loaded image.
    pub size: (u32, u32),
    /// The `FILE` the texture was loaded for (resolved), or the one that
    /// failed, so a bad path is not retried every frame.
    pub loaded_for: Option<PathBuf>,
    pub error: Option<String>,
    /// 0 transparent … 1 opaque. Not the model's: a viewing choice.
    pub opacity: f32,
    pub visible: bool,
}

impl BackdropView {
    pub fn new() -> Self {
        Self {
            opacity: 1.0,
            visible: true,
            ..Default::default()
        }
    }
}

/// The georeference dialog's draft.
#[derive(Clone, Debug, PartialEq)]
pub struct GeorefDraft {
    /// The image, as it will be written to `FILE`.
    pub file: String,
    pub size: (u32, u32),
    /// Extent mode when true, else scale-and-offset.
    pub by_extent: bool,
    pub x0: String,
    pub y0: String,
    pub x1: String,
    pub y1: String,
    /// Scale-and-offset mode: lower-left corner and map units per pixel.
    pub ox: String,
    pub oy: String,
    pub sx: String,
    pub sy: String,
    pub keep_aspect: bool,
    pub error: String,
}

/// The `[MAP]` dimensions dialog's draft.
#[derive(Clone, Debug, PartialEq)]
pub struct MapExtentDraft {
    pub x0: String,
    pub y0: String,
    pub x1: String,
    pub y1: String,
    pub units: String,
    pub error: String,
}

pub const MAP_UNITS: [&str; 4] = ["Feet", "Meters", "Degrees", "None"];

// --- loading ------------------------------------------------------------------------

/// Keep the texture in step with the document's `FILE` line.
pub fn sync(ctx: &egui::Context, ed: &mut SwmmEditor) {
    let Some(bd) = Backdrop::read(&ed.doc) else {
        ed.backdrop.texture = None;
        ed.backdrop.loaded_for = None;
        ed.backdrop.error = None;
        return;
    };
    let path = bd.resolve(ed.path.as_deref());
    if ed.backdrop.loaded_for.as_deref() == Some(path.as_path()) {
        return;
    }
    ed.backdrop.loaded_for = Some(path.clone());
    match load_texture(ctx, &path) {
        Ok((tex, size)) => {
            ed.backdrop.texture = Some(tex);
            ed.backdrop.size = size;
            ed.backdrop.error = None;
        }
        Err(e) => {
            ed.backdrop.texture = None;
            ed.backdrop.error = Some(format!("backdrop {}: {e}", path.display()));
        }
    }
}

fn load_texture(ctx: &egui::Context, path: &Path) -> Result<(TextureHandle, (u32, u32)), String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let size = (rgba.width(), rgba.height());
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [size.0 as usize, size.1 as usize],
        rgba.as_flat_samples().as_slice(),
    );
    let tex = ctx.load_texture(
        format!("swmm-backdrop-{}", path.display()),
        color,
        egui::TextureOptions::LINEAR,
    );
    Ok((tex, size))
}

/// The `FILE` value for an image: relative to the model's folder when it
/// is under it, else as given.
fn file_value(image: &Path, model: Option<&Path>) -> String {
    let rel = model
        .and_then(Path::parent)
        .and_then(|dir| image.strip_prefix(dir).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    rel.unwrap_or_else(|| image.to_string_lossy().replace('\\', "/"))
}

/// Load `image` as the backdrop: with its world file when one sits beside
/// it (one undo step, done), else the georeference dialog opens with the
/// model's extent filled in. Returns the status line.
pub fn load_image(ed: &mut SwmmEditor, image: &Path) -> String {
    let size = match image::image_dimensions(image) {
        Ok(s) => s,
        Err(e) => return format!("Cannot read {}: {e}", image.display()),
    };
    let file = file_value(image, ed.path.as_deref());
    if let Some((wpath, wf)) = backdrop::find_world_file(image) {
        if wf.is_rotated() {
            return format!(
                "{} is rotated; only axis-aligned world files are supported",
                wpath.display()
            );
        }
        let extent = wf.extent(size.0, size.1);
        let bd = Backdrop {
            file,
            dimensions: Some(extent),
            units: backdrop::map_units(&ed.doc),
            offset: None,
            scaling: None,
        };
        return if ed.apply(bd.command(&ed.doc), "load backdrop") {
            format!(
                "Backdrop {} placed from {}",
                image.display(),
                wpath.file_name().unwrap_or_default().to_string_lossy()
            )
        } else {
            ed.last_error.clone().unwrap_or_default()
        };
    }
    let extent = ed
        .bounds
        .map(|b| backdrop::padded(b, 0.06))
        .or_else(|| backdrop::map_dimensions(&ed.doc))
        .unwrap_or((0.0, 0.0, size.0 as f64, size.1 as f64));
    ed.dialogs.georef = Some(georef_draft(file, size, extent));
    format!(
        "No world file beside {}: place the backdrop in the Georeference dialog",
        image.display()
    )
}

/// The dialog for the backdrop already in the model.
pub fn open_georef(ed: &mut SwmmEditor) {
    let Some(bd) = Backdrop::read(&ed.doc) else {
        ed.last_error = Some("no backdrop: View → Backdrop → Load Image…".into());
        return;
    };
    let size = if ed.backdrop.size.0 > 0 {
        ed.backdrop.size
    } else {
        image::image_dimensions(bd.resolve(ed.path.as_deref())).unwrap_or((1, 1))
    };
    let extent = bd
        .dimensions
        .or_else(|| ed.bounds.map(|b| backdrop::padded(b, 0.06)))
        .unwrap_or((0.0, 0.0, size.0 as f64, size.1 as f64));
    ed.dialogs.georef = Some(georef_draft(bd.file, size, extent));
}

fn georef_draft(file: String, size: (u32, u32), extent: (f64, f64, f64, f64)) -> GeorefDraft {
    let (x0, y0, x1, y1) = extent;
    let sx = (x1 - x0) / size.0.max(1) as f64;
    let sy = (y1 - y0) / size.1.max(1) as f64;
    GeorefDraft {
        file,
        size,
        by_extent: true,
        x0: format_number(x0),
        y0: format_number(y0),
        x1: format_number(x1),
        y1: format_number(y1),
        ox: format_number(x0),
        oy: format_number(y0),
        sx: format_number(sx),
        sy: format_number(sy),
        keep_aspect: true,
        error: String::new(),
    }
}

fn num(s: &str, what: &str) -> Result<f64, String> {
    s.trim()
        .parse::<f64>()
        .map_err(|_| format!("{what}: not a number"))
}

/// The backdrop the draft describes.
pub fn georef_backdrop(d: &GeorefDraft, units: Option<String>) -> Result<Backdrop, String> {
    let (w, h) = (d.size.0.max(1) as f64, d.size.1.max(1) as f64);
    if d.by_extent {
        let (x0, y0, x1, y1) = (
            num(&d.x0, "X1")?,
            num(&d.y0, "Y1")?,
            num(&d.x1, "X2")?,
            num(&d.y1, "Y2")?,
        );
        if x1 <= x0 || y1 <= y0 {
            return Err("the upper-right corner must be above and right of the lower-left".into());
        }
        Ok(Backdrop {
            file: d.file.clone(),
            dimensions: Some((x0, y0, x1, y1)),
            units,
            offset: None,
            scaling: None,
        })
    } else {
        let (ox, oy) = (num(&d.ox, "offset X")?, num(&d.oy, "offset Y")?);
        let sx = num(&d.sx, "X scale")?;
        let sy = if d.keep_aspect {
            sx
        } else {
            num(&d.sy, "Y scale")?
        };
        if sx <= 0.0 || sy <= 0.0 {
            return Err("scale must be positive map units per pixel".into());
        }
        Ok(Backdrop {
            file: d.file.clone(),
            dimensions: Some((ox, oy, ox + w * sx, oy + h * sy)),
            units,
            offset: Some((ox, oy)),
            scaling: Some((sx, sy)),
        })
    }
}

/// Write the draft: one undo step. Returns the status line.
pub fn apply_georef(ed: &mut SwmmEditor) -> Option<String> {
    let mut d = ed.dialogs.georef.take()?;
    match georef_backdrop(&d, backdrop::map_units(&ed.doc)) {
        Ok(bd) => {
            if ed.apply(bd.command(&ed.doc), "georeference backdrop") {
                Some(format!("Backdrop {} placed", d.file))
            } else {
                d.error = ed.last_error.clone().unwrap_or_default();
                ed.dialogs.georef = Some(d);
                None
            }
        }
        Err(e) => {
            d.error = e;
            ed.dialogs.georef = Some(d);
            None
        }
    }
}

/// Write a world file beside the image for the draft's placement.
fn write_world_file(ed: &SwmmEditor, d: &GeorefDraft) -> Result<PathBuf, String> {
    let bd = georef_backdrop(d, None)?;
    let extent = bd.dimensions.ok_or("no extent")?;
    let image = bd.resolve(ed.path.as_deref());
    let path = backdrop::world_file_candidates(&image)
        .into_iter()
        .next()
        .ok_or("no world-file name for this image")?;
    let wf = WorldFile::from_extent(extent, d.size.0, d.size.1);
    std::fs::write(&path, wf.to_text()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn remove(ed: &mut SwmmEditor) -> bool {
    ed.apply(Backdrop::remove_command(&ed.doc), "remove backdrop")
}

/// View → Backdrop → Load Image…: the file picker, then [`load_image`].
pub fn pick_and_load(state: &mut AppState) {
    let mut dialog =
        rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"]);
    if let Some(dir) = state.swmm_doc.path.as_deref().and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(path) = dialog.pick_file() {
        state.status = load_image(&mut state.swmm_doc, &path);
    }
}

// --- map dimensions -------------------------------------------------------------------

pub fn open_map_extent(ed: &mut SwmmEditor) {
    let (x0, y0, x1, y1) = backdrop::map_dimensions(&ed.doc)
        .or_else(|| ed.bounds.map(|b| backdrop::padded(b, 0.06)))
        .unwrap_or((0.0, 0.0, 10000.0, 10000.0));
    ed.dialogs.map_extent = Some(MapExtentDraft {
        x0: format_number(x0),
        y0: format_number(y0),
        x1: format_number(x1),
        y1: format_number(y1),
        units: backdrop::map_units(&ed.doc).unwrap_or_else(|| "None".into()),
        error: String::new(),
    });
}

fn fill_extent(d: &mut MapExtentDraft, b: (f64, f64, f64, f64)) {
    d.x0 = format_number(b.0);
    d.y0 = format_number(b.1);
    d.x1 = format_number(b.2);
    d.y1 = format_number(b.3);
}

/// Write `[MAP] DIMENSIONS` and `Units` from the draft as one step.
pub fn apply_map_extent(ed: &mut SwmmEditor) -> Option<String> {
    let mut d = ed.dialogs.map_extent.take()?;
    let parsed = (|| -> Result<(f64, f64, f64, f64), String> {
        let b = (
            num(&d.x0, "X1")?,
            num(&d.y0, "Y1")?,
            num(&d.x1, "X2")?,
            num(&d.y1, "Y2")?,
        );
        if b.2 <= b.0 || b.3 <= b.1 {
            return Err("the upper-right corner must be above and right of the lower-left".into());
        }
        Ok(b)
    })();
    match parsed {
        Ok((x0, y0, x1, y1)) => {
            let cmd = Command::Batch(vec![
                backdrop::set_map_dimensions(x0, y0, x1, y1),
                backdrop::set_map_units(&d.units),
            ]);
            if ed.apply(cmd, "set map dimensions") {
                Some("Map dimensions set".into())
            } else {
                d.error = ed.last_error.clone().unwrap_or_default();
                ed.dialogs.map_extent = Some(d);
                None
            }
        }
        Err(e) => {
            d.error = e;
            ed.dialogs.map_extent = Some(d);
            None
        }
    }
}

// --- menus and panes -------------------------------------------------------------------

/// The View → Backdrop submenu.
pub fn view_menu_items(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.swmm_doc.loaded;
    let has = Backdrop::read(&state.swmm_doc.doc).is_some();
    ui.add_enabled_ui(loaded, |ui| {
        ui.menu_button("Backdrop", |ui| {
            if ui
                .button("Load Image…")
                .on_hover_text("PNG or JPEG; a .pgw/.jgw/.wld beside it places it")
                .clicked()
            {
                pick_and_load(state);
                ui.close_menu();
            }
            if ui
                .add_enabled(has, egui::Button::new("Georeference…"))
                .clicked()
            {
                open_georef(&mut state.swmm_doc);
                ui.close_menu();
            }
            if has {
                ui.checkbox(&mut state.swmm_doc.backdrop.visible, "Show");
            }
            if ui.add_enabled(has, egui::Button::new("Remove")).clicked() {
                if remove(&mut state.swmm_doc) {
                    state.status = "Backdrop removed".into();
                }
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Map Dimensions…").clicked() {
                open_map_extent(&mut state.swmm_doc);
                ui.close_menu();
            }
        });
    });
}

/// The layers pane's backdrop section.
pub fn layers_section(ui: &mut Ui, state: &mut AppState) {
    ui.label(RichText::new("Backdrop").strong());
    match Backdrop::read(&state.swmm_doc.doc) {
        Some(bd) => {
            ui.label(RichText::new(format!("Image: {}", bd.file)).small());
            if let Some(e) = &state.swmm_doc.backdrop.error {
                ui.label(
                    RichText::new(e)
                        .small()
                        .color(crate::theme::palette::error_text(ui.visuals().dark_mode)),
                );
            }
            ui.checkbox(&mut state.swmm_doc.backdrop.visible, "Show");
            ui.add(
                egui::Slider::new(&mut state.swmm_doc.backdrop.opacity, 0.0..=1.0).text("opacity"),
            );
            ui.horizontal(|ui| {
                if ui.small_button("Georeference…").clicked() {
                    open_georef(&mut state.swmm_doc);
                }
                if ui.small_button("Remove").clicked() && remove(&mut state.swmm_doc) {
                    state.status = "Backdrop removed".into();
                }
            });
        }
        None => {
            if ui.small_button("Load Image…").clicked() {
                pick_and_load(state);
            }
        }
    }
}

/// Paint the backdrop under the map. `w2s` is the map's transform.
pub fn draw(painter: &egui::Painter, ed: &SwmmEditor, w2s: &dyn Fn((f64, f64)) -> Pos2) {
    if !ed.backdrop.visible {
        return;
    }
    let (Some(tex), Some(bd)) = (ed.backdrop.texture.as_ref(), Backdrop::read(&ed.doc)) else {
        return;
    };
    let Some((x0, y0, x1, y1)) = bd.dimensions else {
        return;
    };
    let tl = w2s((x0, y1));
    let br = w2s((x1, y0));
    painter.image(
        tex.id(),
        Rect::from_two_pos(tl, br),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::from_white_alpha((ed.backdrop.opacity.clamp(0.0, 1.0) * 255.0) as u8),
    );
}

fn window<'a>(ctx: &egui::Context, title: &'a str, size: Vec2) -> egui::Window<'a> {
    egui::Window::new(title)
        .id(Id::new(("swmm-dialog", title)))
        .collapsible(false)
        .resizable(false)
        .default_size(size)
        .default_pos(ctx.screen_rect().center() - size / 2.0)
}

fn field(ui: &mut Ui, label: &str, id: &str, s: &mut String) -> bool {
    ui.label(label);
    let r = ui.add(
        egui::TextEdit::singleline(s)
            .id(Id::new(("swmm-backdrop", id)))
            .desired_width(110.0),
    );
    ui.end_row();
    r.changed()
}

pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    draw_georef(ctx, state);
    draw_map_extent(ctx, state);
}

fn draw_georef(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.georef.clone() else {
        return;
    };
    let mut open = true;
    let mut action: Option<&str> = None;
    window(ctx, "Georeference Backdrop", Vec2::new(420.0, 340.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(RichText::new(format!("{} — {} × {} px", d.file, d.size.0, d.size.1)).small());
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.by_extent, true, "Extent (map corners)");
                ui.radio_value(&mut d.by_extent, false, "Scale and offset");
            });
            egui::Grid::new("swmm-georef-grid")
                .num_columns(2)
                .show(ui, |ui| {
                    if d.by_extent {
                        field(ui, "Lower-left X", "x0", &mut d.x0);
                        field(ui, "Lower-left Y", "y0", &mut d.y0);
                        field(ui, "Upper-right X", "x1", &mut d.x1);
                        field(ui, "Upper-right Y", "y1", &mut d.y1);
                    } else {
                        field(ui, "Lower-left X", "ox", &mut d.ox);
                        field(ui, "Lower-left Y", "oy", &mut d.oy);
                        field(ui, "Map units per pixel (X)", "sx", &mut d.sx);
                        ui.label("Map units per pixel (Y)");
                        ui.horizontal(|ui| {
                            ui.add_enabled(
                                !d.keep_aspect,
                                egui::TextEdit::singleline(&mut d.sy)
                                    .id(Id::new(("swmm-backdrop", "sy")))
                                    .desired_width(70.0),
                            );
                            ui.checkbox(&mut d.keep_aspect, "same as X");
                        });
                        ui.end_row();
                    }
                });
            if d.by_extent {
                if let Some(b) = state.swmm_doc.bounds {
                    if ui
                        .small_button("Fit to model")
                        .on_hover_text(
                            "The model's extent with 6% room, keeping the image's aspect",
                        )
                        .clicked()
                    {
                        let (x0, y0, x1, y1) =
                            fit_aspect(stormsewer_swmm::backdrop::padded(b, 0.06), d.size);
                        d.x0 = format_number(x0);
                        d.y0 = format_number(y0);
                        d.x1 = format_number(x1);
                        d.y1 = format_number(y1);
                    }
                }
            }
            if let Ok(bd) = georef_backdrop(&d, None) {
                if let Some((x0, y0, x1, y1)) = bd.dimensions {
                    ui.label(
                        RichText::new(format!(
                            "{} × {} map units; {} units/px",
                            format_number(x1 - x0),
                            format_number(y1 - y0),
                            format_number((x1 - x0) / d.size.0.max(1) as f64)
                        ))
                        .small(),
                    );
                }
            }
            if !d.error.is_empty() {
                ui.label(
                    RichText::new(&d.error)
                        .color(crate::theme::palette::error_text(ui.visuals().dark_mode)),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    action = Some("ok");
                }
                if ui.button("Cancel").clicked() {
                    action = Some("cancel");
                }
                if ui
                    .button("Write world file")
                    .on_hover_text(
                        "Save a .pgw/.jgw beside the image so the next load is automatic",
                    )
                    .clicked()
                {
                    action = Some("world");
                }
            });
            ui.label(RichText::new("Written to [BACKDROP]; one undo step.").small());
        });
    let ed = &mut state.swmm_doc;
    match action {
        Some("ok") => {
            ed.dialogs.georef = Some(d);
            if let Some(s) = apply_georef(ed) {
                state.status = s;
            }
        }
        Some("cancel") => ed.dialogs.georef = None,
        Some("world") => {
            match write_world_file(ed, &d) {
                Ok(p) => state.status = format!("Wrote {}", p.display()),
                Err(e) => d.error = e,
            }
            ed.dialogs.georef = Some(d);
        }
        _ => ed.dialogs.georef = if open { Some(d) } else { None },
    }
}

/// The largest box of the image's aspect centred in `b`... or rather the
/// smallest box of that aspect that contains `b`, so the whole model is
/// on the picture.
fn fit_aspect(b: (f64, f64, f64, f64), size: (u32, u32)) -> (f64, f64, f64, f64) {
    let (x0, y0, x1, y1) = b;
    let (w, h) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));
    let aspect = size.0.max(1) as f64 / size.1.max(1) as f64;
    let (bw, bh) = if w / h >= aspect {
        (w, w / aspect)
    } else {
        (h * aspect, h)
    };
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    (cx - bw / 2.0, cy - bh / 2.0, cx + bw / 2.0, cy + bh / 2.0)
}

fn draw_map_extent(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.map_extent.clone() else {
        return;
    };
    let mut open = true;
    let mut action: Option<&str> = None;
    window(ctx, "Map Dimensions", Vec2::new(360.0, 260.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new("[MAP] DIMENSIONS — the extent the EPA GUI opens the map at.")
                    .small(),
            );
            egui::Grid::new("swmm-map-extent-grid")
                .num_columns(2)
                .show(ui, |ui| {
                    field(ui, "Lower-left X", "mx0", &mut d.x0);
                    field(ui, "Lower-left Y", "my0", &mut d.y0);
                    field(ui, "Upper-right X", "mx1", &mut d.x1);
                    field(ui, "Upper-right Y", "my1", &mut d.y1);
                    ui.label("Units");
                    egui::ComboBox::from_id_salt("swmm-map-units")
                        .selected_text(d.units.clone())
                        .show_ui(ui, |ui| {
                            for u in MAP_UNITS {
                                ui.selectable_value(&mut d.units, u.to_string(), u);
                            }
                        });
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        state.swmm_doc.bounds.is_some(),
                        egui::Button::new("Set from model"),
                    )
                    .on_hover_text("The drawn objects' extent with 6% room")
                    .clicked()
                {
                    if let Some(b) = state.swmm_doc.bounds {
                        fill_extent(&mut d, stormsewer_swmm::backdrop::padded(b, 0.06));
                    }
                }
                let bd = Backdrop::read(&state.swmm_doc.doc).and_then(|b| b.dimensions);
                if ui
                    .add_enabled(bd.is_some(), egui::Button::new("Set from backdrop"))
                    .clicked()
                {
                    if let Some(b) = bd {
                        fill_extent(&mut d, b);
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
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    action = Some("ok");
                }
                if ui.button("Cancel").clicked() {
                    action = Some("cancel");
                }
            });
        });
    let ed = &mut state.swmm_doc;
    match action {
        Some("ok") => {
            ed.dialogs.map_extent = Some(d);
            if let Some(s) = apply_map_extent(ed) {
                state.status = s;
            }
        }
        Some("cancel") => ed.dialogs.map_extent = None,
        _ => ed.dialogs.map_extent = if open { Some(d) } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join("stormsewer-app-tests").join(tag);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let p = dir.join(name);
        image::RgbImage::from_pixel(w, h, image::Rgb([120, 160, 200]))
            .save(&p)
            .unwrap();
        p
    }

    #[test]
    fn load_with_world_file_writes_backdrop_in_one_step_and_the_texture_follows() {
        let dir = temp_dir("backdrop-world");
        let img = png(&dir, "site.png", 40, 20);
        // 2 map units per pixel, upper-left pixel centre at (101, 199).
        std::fs::write(dir.join("site.pgw"), "2\n0\n0\n-2\n101\n199\n").unwrap();
        let mut ed = SwmmEditor::default();
        ed.new_model();
        ed.path = Some(dir.join("model.inp"));
        let before = ed.undo_depth();
        let status = load_image(&mut ed, &img);
        assert!(status.contains("placed"), "{status}");
        assert_eq!(ed.undo_depth(), before + 1);
        let bd = Backdrop::read(&ed.doc).unwrap();
        assert_eq!(bd.file, "site.png", "relative to the model");
        assert_eq!(bd.dimensions, Some((100.0, 160.0, 180.0, 200.0)));
        assert!(ed.doc.to_string().contains("[BACKDROP]"));
        let ctx = egui::Context::default();
        sync(&ctx, &mut ed);
        assert!(ed.backdrop.texture.is_some(), "{:?}", ed.backdrop.error);
        assert_eq!(ed.backdrop.size, (40, 20));
        // Undo takes the section away and the texture with it.
        assert!(ed.undo().is_some());
        sync(&ctx, &mut ed);
        assert!(Backdrop::read(&ed.doc).is_none());
        assert!(ed.backdrop.texture.is_none());
    }

    #[test]
    fn load_without_world_file_opens_georeference_and_scale_mode_writes_offset_and_scaling() {
        let dir = temp_dir("backdrop-georef");
        let img = png(&dir, "plan.jpg", 30, 10);
        let mut ed = SwmmEditor::default();
        ed.new_model();
        let status = load_image(&mut ed, &img);
        assert!(status.contains("Georeference"), "{status}");
        let mut d = ed.dialogs.georef.clone().expect("dialog open");
        assert_eq!(d.size, (30, 10));
        d.by_extent = false;
        d.ox = "1000".into();
        d.oy = "2000".into();
        d.sx = "5".into();
        d.keep_aspect = true;
        ed.dialogs.georef = Some(d);
        assert!(apply_georef(&mut ed).is_some());
        assert!(ed.dialogs.georef.is_none());
        let bd = Backdrop::read(&ed.doc).unwrap();
        assert_eq!(bd.dimensions, Some((1000.0, 2000.0, 1150.0, 2050.0)));
        assert_eq!(bd.offset, Some((1000.0, 2000.0)));
        assert_eq!(bd.scaling, Some((5.0, 5.0)));
        assert_eq!(ed.undo_depth(), 1);
        // A bad extent stays in the dialog with the reason.
        open_georef(&mut ed);
        let mut d = ed.dialogs.georef.clone().unwrap();
        assert!(!d.by_extent || d.x0 == "1000", "{d:?}");
        d.by_extent = true;
        d.x1 = "abc".into();
        ed.dialogs.georef = Some(d);
        assert!(apply_georef(&mut ed).is_none());
        assert!(ed.dialogs.georef.as_ref().unwrap().error.contains("X2"));
        // The world file writer round-trips the placement.
        let d = ed.dialogs.georef.clone().unwrap();
        let mut d = d;
        d.x1 = "1150".into();
        let p = write_world_file(&ed, &d).unwrap();
        assert!(p.ends_with("plan.jgw"), "{}", p.display());
        let (_, wf) = backdrop::find_world_file(&img).unwrap();
        assert_eq!(wf.extent(30, 10), (1000.0, 2000.0, 1150.0, 2050.0));
    }

    #[test]
    fn map_dimensions_set_from_model_and_backdrop() {
        let mut ed = SwmmEditor::default();
        ed.new_model();
        for (x, y) in [(100.0, 100.0), (300.0, 200.0)] {
            let o = stormsewer_swmm::doc::build::new_node(
                &ed.doc,
                stormsewer_swmm::doc::build::NodeType::Junction,
                x,
                y,
            );
            assert!(ed.apply(o.command, "node"));
        }
        ed.refresh();
        open_map_extent(&mut ed);
        let mut d = ed.dialogs.map_extent.clone().unwrap();
        fill_extent(&mut d, backdrop::padded(ed.bounds.unwrap(), 0.06));
        assert_eq!(d.x0, "88");
        assert_eq!(d.x1, "312");
        d.units = "Feet".into();
        ed.dialogs.map_extent = Some(d);
        let depth = ed.undo_depth();
        assert!(apply_map_extent(&mut ed).is_some());
        assert_eq!(ed.undo_depth(), depth + 1);
        assert_eq!(
            backdrop::map_dimensions(&ed.doc),
            Some((88.0, 88.0, 312.0, 212.0))
        );
        assert_eq!(backdrop::map_units(&ed.doc).as_deref(), Some("Feet"));
        let (x0, y0, x1, y1) = fit_aspect((0.0, 0.0, 100.0, 100.0), (200, 100));
        assert_eq!((x0, y0, x1, y1), (-50.0, 0.0, 150.0, 100.0));
    }
}
