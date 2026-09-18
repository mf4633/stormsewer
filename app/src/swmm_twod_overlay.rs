// SPDX-License-Identifier: GPL-3.0-or-later

//! The 2D results on the map: a depth / velocity / hazard texture over the
//! DEM extent for the frame the time slider points at (or the run maxima
//! and arrival time as static grids), a legend, a hover readout, and the
//! interface markers (node circles coloured by the exchange direction,
//! bank lines). Submodule of `swmm_twod`; the state it draws from is
//! `TwoDState::overlay` and `TwoDState::results`.
//!
//! Same pattern as `swmm_backdrop`: `sync` keeps a `TextureHandle` in step
//! with (results file, mode, frame), `draw` paints it with the viewport
//! transform. A grid wider than [`MAX_TEXTURE`] cells is sampled down with a
//! stride, so a county-sized DEM cannot exceed the GPU's texture limit.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, TextureHandle, Vec2};
use stormsewer_swmm::gis::raster::Raster;
use stormsewer_swmm::twod::{Frame, InterfaceKind, Results};

use super::{interface_of, Pick};
use crate::swmm_doc::SwmmEditor;
use crate::viewport::Viewport;

/// Largest texture edge, in cells, before the grid is sampled down.
pub const MAX_TEXTURE: usize = 4096;

/// What the overlay paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum OverlayMode {
    /// Depth in the current frame.
    #[default]
    Depth,
    /// Velocity magnitude in the current frame.
    Velocity,
    /// Depth × velocity in the current frame.
    Hazard,
    MaxDepth,
    MaxVelocity,
    MaxHazard,
    /// Time until each cell first wets, hours.
    Arrival,
}

impl OverlayMode {
    pub const ALL: [OverlayMode; 7] = [
        Self::Depth,
        Self::Velocity,
        Self::Hazard,
        Self::MaxDepth,
        Self::MaxVelocity,
        Self::MaxHazard,
        Self::Arrival,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Depth => "Depth",
            Self::Velocity => "Velocity",
            Self::Hazard => "Hazard (depth × velocity)",
            Self::MaxDepth => "Max depth",
            Self::MaxVelocity => "Max velocity",
            Self::MaxHazard => "Max hazard",
            Self::Arrival => "Arrival time",
        }
    }

    /// Whole-run grids do not change with the time slider.
    pub fn is_static(self) -> bool {
        !matches!(self, Self::Depth | Self::Velocity | Self::Hazard)
    }

    /// The unit the values carry.
    pub fn unit(self, metric: bool) -> &'static str {
        match self {
            Self::Depth | Self::MaxDepth => {
                if metric {
                    "m"
                } else {
                    "ft"
                }
            }
            Self::Velocity | Self::MaxVelocity => {
                if metric {
                    "m/s"
                } else {
                    "ft/s"
                }
            }
            Self::Hazard | Self::MaxHazard => {
                if metric {
                    "m²/s"
                } else {
                    "ft²/s"
                }
            }
            Self::Arrival => "h",
        }
    }

    /// The static grid that sets the colour scale of a frame mode, so the
    /// same depth is the same blue in every frame of a run.
    fn scale_source(self) -> OverlayMode {
        match self {
            Self::Depth => Self::MaxDepth,
            Self::Velocity => Self::MaxVelocity,
            Self::Hazard => Self::MaxHazard,
            other => other,
        }
    }
}

/// Overlay settings and caches.
pub struct OverlayState {
    pub on: bool,
    pub mode: OverlayMode,
    /// 0 transparent … 1 opaque.
    pub opacity: f32,
    /// The frame the layers pane's own slider chose.
    pub frame: usize,
    /// The frame the SWMM time slider maps to, when SWMM results are shown
    /// as an instant (set once per frame by `draw_dialogs`).
    pub want_frame: Option<usize>,
    pub texture: Option<TextureHandle>,
    /// (results file, mode, frame) the texture was built for.
    key: Option<(PathBuf, OverlayMode, usize)>,
    /// The frame the texture was built from, for the hover readout.
    pub frame_data: Option<Frame>,
    /// Whole-run grids, read once per results file.
    statics: HashMap<OverlayMode, Result<Raster, String>>,
    /// Value range of the current texture (0 … max), for the legend.
    pub range: (f64, f64),
    pub error: Option<String>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            on: true,
            mode: OverlayMode::Depth,
            opacity: 0.75,
            frame: 0,
            want_frame: None,
            texture: None,
            key: None,
            frame_data: None,
            statics: HashMap::new(),
            range: (0.0, 0.0),
            error: None,
        }
    }
}

impl OverlayState {
    /// Forget everything read from a results file.
    pub fn reset(&mut self) {
        self.texture = None;
        self.key = None;
        self.frame_data = None;
        self.statics.clear();
        self.range = (0.0, 0.0);
        self.error = None;
        self.frame = 0;
        self.want_frame = None;
    }

    /// The frame index the overlay shows: the SWMM slider's when it maps,
    /// else the pane's own, clamped to the file.
    pub fn frame_index(&self, results: &Results) -> usize {
        let n = results.n_frames.max(1);
        self.want_frame.unwrap_or(self.frame).min(n - 1)
    }

    fn static_grid(&mut self, results: &Results, mode: OverlayMode) -> &Result<Raster, String> {
        self.statics.entry(mode).or_insert_with(|| {
            let r = match mode {
                OverlayMode::MaxDepth => results.max_depth(),
                OverlayMode::MaxVelocity => results.max_velocity(),
                OverlayMode::MaxHazard => results.max_hazard(),
                OverlayMode::Arrival => results.arrival(),
                _ => unreachable!("frame modes are not static grids"),
            };
            r.map_err(|e| e.to_string())
        })
    }

    /// The top of the colour scale for a mode.
    fn scale_max(&mut self, results: &Results, mode: OverlayMode, fallback: f64) -> f64 {
        let src = mode.scale_source();
        let hi = match self.static_grid(results, src) {
            Ok(r) => r.range().map(|(_, hi)| hi),
            Err(_) => None,
        };
        let hi = if src == OverlayMode::Arrival {
            hi.map(|s| s / 3600.0)
        } else {
            hi
        };
        hi.filter(|v| *v > 0.0).unwrap_or(fallback).max(1e-9)
    }
}

/// The nearest 2D frame to a simulation time.
pub fn nearest_frame(results: &Results, time_s: f64) -> usize {
    if results.n_frames == 0 || results.frame_step_s <= 0.0 {
        return 0;
    }
    ((time_s / results.frame_step_s).round().max(0.0) as usize).min(results.n_frames - 1)
}

// --- colour ---------------------------------------------------------------------------

fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

fn piecewise(stops: &[(u8, u8, u8)], t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let n = stops.len() - 1;
    let x = t * n as f32;
    let i = (x.floor() as usize).min(n - 1);
    lerp(stops[i], stops[i + 1], x - i as f32)
}

/// The colour for a value at `t` (0 … 1 of the scale) in a mode.
pub fn ramp(mode: OverlayMode, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = match mode {
        OverlayMode::Depth | OverlayMode::MaxDepth => {
            piecewise(&[(198, 226, 245), (66, 146, 214), (16, 62, 150), (6, 20, 80)], t)
        }
        OverlayMode::Velocity | OverlayMode::MaxVelocity => {
            piecewise(&[(60, 170, 90), (240, 220, 60), (230, 120, 30), (180, 20, 20)], t)
        }
        OverlayMode::Hazard | OverlayMode::MaxHazard => {
            piecewise(&[(250, 230, 120), (240, 140, 40), (200, 40, 40), (110, 10, 90)], t)
        }
        OverlayMode::Arrival => {
            piecewise(&[(250, 230, 60), (40, 170, 140), (60, 60, 160), (70, 10, 90)], t)
        }
    };
    let a = 150.0 + 105.0 * t;
    Color32::from_rgba_unmultiplied(r, g, b, a as u8)
}

// --- texture --------------------------------------------------------------------------

/// Build the image for one grid of values (row-major, top row first).
/// `hide` says which values are transparent (dry cells, no-data).
fn image_of(
    ncols: usize,
    nrows: usize,
    mode: OverlayMode,
    scale_max: f64,
    value: impl Fn(usize) -> Option<f64>,
) -> egui::ColorImage {
    let stride = ncols.max(nrows).div_ceil(MAX_TEXTURE).max(1);
    let w = ncols.div_ceil(stride).max(1);
    let h = nrows.div_ceil(stride).max(1);
    let mut pixels = vec![Color32::TRANSPARENT; w * h];
    for py in 0..h {
        for px in 0..w {
            let (c, r) = (px * stride, py * stride);
            if c >= ncols || r >= nrows {
                continue;
            }
            if let Some(v) = value(r * ncols + c) {
                let t = (v / scale_max) as f32;
                pixels[py * w + px] = ramp(mode, t);
            }
        }
    }
    egui::ColorImage {
        size: [w, h],
        pixels,
    }
}

/// Keep the texture in step with the results, the mode and the frame.
pub fn sync(ctx: &egui::Context, ed: &mut SwmmEditor) {
    let td = &mut ed.twod;
    let Some(results) = td.results.as_ref() else {
        if td.overlay.texture.is_some() || td.overlay.key.is_some() {
            td.overlay.reset();
        }
        return;
    };
    if !td.overlay.on {
        return;
    }
    let ov = &mut td.overlay;
    let mode = ov.mode;
    let idx = if mode.is_static() { 0 } else { ov.frame_index(results) };
    let key = (results.path.clone(), mode, idx);
    if ov.key.as_ref() == Some(&key) {
        return;
    }
    ov.key = Some(key);
    ov.texture = None;
    ov.error = None;
    let dry = td.draft.dry_depth.max(0.0);
    let (ncols, nrows) = (results.ncols, results.nrows);
    let image = if mode.is_static() {
        ov.frame_data = None;
        let grid = match ov.static_grid(results, mode) {
            Ok(r) => Ok(r.clone()),
            Err(e) => Err(e.clone()),
        };
        let grid = match grid {
            Ok(r) => r,
            Err(e) => {
                ov.error = Some(e);
                return;
            }
        };
        let to_h = if mode == OverlayMode::Arrival { 1.0 / 3600.0 } else { 1.0 };
        let hi = grid.range().map(|(_, hi)| hi * to_h).unwrap_or(0.0);
        let hi = hi.max(1e-9);
        ov.range = (0.0, hi);
        let (gc, gr) = (grid.ncols, grid.nrows);
        image_of(gc, gr, mode, hi, |i| {
            let v = grid.data[i];
            if v.is_nan() {
                return None;
            }
            let v = v * to_h;
            if mode != OverlayMode::Arrival && v < dry {
                return None;
            }
            Some(v)
        })
    } else {
        let frame = match results.frame(idx) {
            Ok(f) => f,
            Err(e) => {
                ov.error = Some(e.to_string());
                ov.frame_data = None;
                return;
            }
        };
        let cell_value = |f: &Frame, i: usize| -> Option<f64> {
            let d = *f.depth.get(i)? as f64;
            if d.is_nan() || d < dry {
                return None;
            }
            let speed = || {
                let vx = f.vx.get(i).copied().unwrap_or(0.0) as f64;
                let vy = f.vy.get(i).copied().unwrap_or(0.0) as f64;
                vx.hypot(vy)
            };
            Some(match mode {
                OverlayMode::Depth => d,
                OverlayMode::Velocity => speed(),
                _ => d * speed(),
            })
        };
        let frame_max = (0..ncols * nrows)
            .filter_map(|i| cell_value(&frame, i))
            .fold(0.0_f64, f64::max);
        let hi = ov.scale_max(results, mode, frame_max);
        ov.range = (0.0, hi);
        let img = image_of(ncols, nrows, mode, hi, |i| cell_value(&frame, i));
        ov.frame_data = Some(frame);
        img
    };
    let tex = ctx.load_texture(
        format!("swmm-2d-{}-{:?}-{idx}", results.path.display(), mode),
        image,
        egui::TextureOptions::NEAREST,
    );
    ov.texture = Some(tex);
}

// --- drawing --------------------------------------------------------------------------

fn w2s(vp: &Viewport, rect: Rect, p: (f64, f64)) -> Pos2 {
    vp.world_to_screen(rect, p.0, p.1)
}

/// Value at a world point: the frame's cell, or the static grid's.
pub fn readout(ed: &SwmmEditor, x: f64, y: f64) -> Option<String> {
    let td = &ed.twod;
    let results = td.results.as_ref()?;
    let ov = &td.overlay;
    let metric = results.metric;
    let (x1, y1) = (
        results.x0 + results.cell * results.ncols as f64,
        results.y0 + results.cell * results.nrows as f64,
    );
    if !(results.x0..x1).contains(&x) || !(results.y0..y1).contains(&y) || results.cell <= 0.0 {
        return None;
    }
    let col = (((x - results.x0) / results.cell).floor() as usize).min(results.ncols.saturating_sub(1));
    let row = (((y1 - y) / results.cell).floor() as usize).min(results.nrows.saturating_sub(1));
    let i = row * results.ncols + col;
    let len_u = OverlayMode::Depth.unit(metric);
    let vel_u = OverlayMode::Velocity.unit(metric);
    if ov.mode.is_static() {
        let grid = ov.statics.get(&ov.mode)?.as_ref().ok()?;
        let v = grid.sample(x, y)?;
        return Some(match ov.mode {
            OverlayMode::Arrival => format!("arrival {:.2} h", v / 3600.0),
            m => format!("{} {:.3} {}", m.label().to_lowercase(), v, m.unit(metric)),
        });
    }
    let f = ov.frame_data.as_ref()?;
    let d = *f.depth.get(i)? as f64;
    if d.is_nan() {
        return Some("no data".into());
    }
    let vx = f.vx.get(i).copied().unwrap_or(0.0) as f64;
    let vy = f.vy.get(i).copied().unwrap_or(0.0) as f64;
    let v = vx.hypot(vy);
    Some(format!(
        "depth {d:.3} {len_u} · velocity {v:.2} {vel_u} · hazard {:.3} {}",
        d * v,
        OverlayMode::Hazard.unit(metric)
    ))
}

/// Paint the results texture, the interface markers, the hover readout and
/// the legend.
pub fn draw(painter: &egui::Painter, rect: Rect, vp: &Viewport, ed: &SwmmEditor) {
    let td = &ed.twod;
    let dark = painter.ctx().style().visuals.dark_mode;
    if let (true, Some(tex), Some(results)) = (td.overlay.on, td.overlay.texture.as_ref(), td.results.as_ref()) {
        let (x0, y0) = (results.x0, results.y0);
        let x1 = x0 + results.cell * results.ncols as f64;
        let y1 = y0 + results.cell * results.nrows as f64;
        let tl = w2s(vp, rect, (x0, y1));
        let br = w2s(vp, rect, (x1, y0));
        painter.image(
            tex.id(),
            Rect::from_two_pos(tl, br),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_white_alpha((td.overlay.opacity.clamp(0.0, 1.0) * 255.0) as u8),
        );
    }
    if td.show_interfaces {
        draw_interfaces(painter, rect, vp, ed, dark);
    }
    draw_pick(painter, rect, vp, ed);
    if td.overlay.on && td.results.is_some() {
        if let Some(p) = painter.ctx().pointer_hover_pos() {
            if rect.contains(p) {
                let (x, y) = vp.screen_to_world(rect, p);
                if let Some(text) = readout(ed, x, y) {
                    let font = egui::FontId::proportional(12.0);
                    let galley = painter.layout_no_wrap(text, font, Color32::WHITE);
                    let pos = p + Vec2::new(14.0, 10.0);
                    let bg = Rect::from_min_size(pos, galley.size() + Vec2::splat(6.0));
                    painter.rect_filled(bg, 3.0, Color32::from_black_alpha(170));
                    painter.galley(pos + Vec2::splat(3.0), galley, Color32::WHITE);
                }
            }
        }
        if td.legend {
            draw_legend(painter, rect, ed, dark);
        }
    }
}

fn draw_legend(painter: &egui::Painter, rect: Rect, ed: &SwmmEditor, dark: bool) {
    let td = &ed.twod;
    let Some(results) = td.results.as_ref() else { return };
    let ov = &td.overlay;
    let pad = 8.0;
    let box_w = 200.0;
    let box_h = 78.0;
    let origin = Pos2::new(rect.right() - 12.0 - box_w, rect.bottom() - 12.0 - box_h);
    let bg = Rect::from_min_size(origin, Vec2::new(box_w, box_h));
    let (fill, ink) = if dark {
        (Color32::from_black_alpha(180), Color32::from_gray(230))
    } else {
        (Color32::from_white_alpha(220), Color32::from_gray(30))
    };
    painter.rect_filled(bg, 4.0, fill);
    let font = egui::FontId::proportional(12.0);
    let small = egui::FontId::proportional(11.0);
    let title = format!("2D · {} ({})", ov.mode.label(), ov.mode.unit(results.metric));
    painter.text(origin + Vec2::new(pad, pad), egui::Align2::LEFT_TOP, title, font.clone(), ink);
    let bar = Rect::from_min_size(origin + Vec2::new(pad, pad + 20.0), Vec2::new(box_w - 2.0 * pad, 12.0));
    let n = 32;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let x0 = bar.left() + bar.width() * t0;
        let x1 = bar.left() + bar.width() * (i + 1) as f32 / n as f32;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x0, bar.top()), Pos2::new(x1, bar.bottom())),
            0.0,
            ramp(ov.mode, t0),
        );
    }
    let (lo, hi) = ov.range;
    let below = bar.left_bottom() + Vec2::new(0.0, 3.0);
    painter.text(below, egui::Align2::LEFT_TOP, format!("{lo:.2}"), small.clone(), ink);
    painter.text(
        bar.right_bottom() + Vec2::new(0.0, 3.0),
        egui::Align2::RIGHT_TOP,
        format!("{hi:.2}"),
        small.clone(),
        ink,
    );
    let when = if ov.mode.is_static() {
        "whole run".to_string()
    } else {
        let i = ov.frame_index(results);
        let t = ov
            .frame_data
            .as_ref()
            .map(|f| f.time_s)
            .unwrap_or(i as f64 * results.frame_step_s);
        format!("frame {}/{} · {:.2} h", i + 1, results.n_frames.max(1), t / 3600.0)
    };
    painter.text(
        origin + Vec2::new(pad, box_h - pad - 12.0),
        egui::Align2::LEFT_TOP,
        when,
        small,
        ink,
    );
}

/// Exchange flow at a node for the frame the overlay shows.
fn exchange_now(ed: &SwmmEditor, node: &str) -> Option<f64> {
    let td = &ed.twod;
    let results = td.results.as_ref()?;
    let series = td.exchange.get(&node.to_ascii_uppercase())?;
    let i = td.overlay.frame_index(results);
    if td.overlay.mode.is_static() {
        // Whole-run view: the net direction over the run.
        let net: f64 = series.iter().map(|(_, q)| q).sum();
        return Some(net);
    }
    series.get(i).map(|(_, q)| *q).or_else(|| series.last().map(|(_, q)| *q))
}

fn draw_interfaces(painter: &egui::Painter, rect: Rect, vp: &Viewport, ed: &SwmmEditor, dark: bool) {
    let cfg = &ed.twod.draft;
    let grey = if dark { Color32::from_gray(170) } else { Color32::from_gray(90) };
    let out = Color32::from_rgb(220, 60, 50);
    let back = Color32::from_rgb(40, 110, 220);
    for n in &ed.nodes {
        let iface = interface_of(cfg, &n.name);
        let p = w2s(vp, rect, (n.x, n.y));
        if !rect.contains(p) {
            continue;
        }
        let colour = match exchange_now(ed, &n.name) {
            Some(q) if q > 1e-9 => out,
            Some(q) if q < -1e-9 => back,
            _ => grey,
        };
        match iface.kind {
            InterfaceKind::Sealed => {
                let d = 4.0;
                painter.line_segment([p + Vec2::new(-d, -d), p + Vec2::new(d, d)], Stroke::new(1.5_f32, grey));
                painter.line_segment([p + Vec2::new(-d, d), p + Vec2::new(d, -d)], Stroke::new(1.5_f32, grey));
            }
            InterfaceKind::Inlet { .. } => {
                painter.rect_stroke(Rect::from_center_size(p, Vec2::splat(11.0)), 1.0, Stroke::new(2.0_f32, colour));
            }
            InterfaceKind::Manhole => {
                painter.circle_stroke(p, 7.0, Stroke::new(2.0_f32, colour));
                if iface.lid_open {
                    painter.circle_filled(p, 2.0, colour);
                }
            }
        }
    }
    let bank = Color32::from_rgb(235, 140, 30);
    for b in &cfg.banks {
        let pts: Vec<Pos2> = b.polyline.iter().map(|p| w2s(vp, rect, *p)).collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(2.5_f32, bank)));
            let mid = pts[pts.len() / 2];
            painter.text(
                mid + Vec2::new(6.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{} {}", b.link, if b.right { "R" } else { "L" }),
                egui::FontId::proportional(11.0),
                bank,
            );
        } else if let Some(p) = pts.first() {
            painter.circle_filled(*p, 3.0, bank);
        }
    }
    for s in &cfg.sources {
        let p = w2s(vp, rect, (s.x, s.y));
        if rect.contains(p) {
            let c = Color32::from_rgb(120, 60, 200);
            painter.add(egui::Shape::convex_polygon(
                vec![p + Vec2::new(0.0, -7.0), p + Vec2::new(6.0, 5.0), p + Vec2::new(-6.0, 5.0)],
                c,
                Stroke::NONE,
            ));
            painter.text(
                p + Vec2::new(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &s.name,
                egui::FontId::proportional(11.0),
                c,
            );
        }
    }
}

/// The bank line being drawn: its vertices, and a rubber band to the
/// pointer.
fn draw_pick(painter: &egui::Painter, rect: Rect, vp: &Viewport, ed: &SwmmEditor) {
    let Some(pick) = ed.twod.pick.as_ref() else { return };
    let c = Color32::from_rgb(255, 170, 40);
    match pick {
        Pick::Bank { link, pts } => {
            if let Some(name) = link {
                if let Some(l) = ed.link(name) {
                    let sp: Vec<Pos2> = l.path.iter().map(|p| w2s(vp, rect, *p)).collect();
                    if sp.len() >= 2 {
                        painter.add(egui::Shape::line(sp, Stroke::new(4.0_f32, Color32::from_rgba_unmultiplied(255, 170, 40, 90))));
                    }
                }
            }
            let mut sp: Vec<Pos2> = pts.iter().map(|p| w2s(vp, rect, *p)).collect();
            for p in &sp {
                painter.circle_filled(*p, 3.5, c);
            }
            if let Some(h) = painter.ctx().pointer_hover_pos() {
                if rect.contains(h) {
                    sp.push(h);
                }
            }
            if sp.len() >= 2 {
                painter.add(egui::Shape::line(sp, Stroke::new(2.0_f32, c)));
            }
        }
        Pick::Source => {
            if let Some(h) = painter.ctx().pointer_hover_pos() {
                if rect.contains(h) {
                    painter.circle_stroke(h, 8.0, Stroke::new(1.5_f32, c));
                }
            }
        }
    }
}
