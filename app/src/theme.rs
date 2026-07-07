// SPDX-License-Identifier: GPL-3.0-or-later

//! Centralized visual theme and semantic color palette.
//!
//! One place for every color and spacing decision, so the UI reads as a single
//! system. [`apply`] installs the egui style at startup; [`palette`] names the
//! domain colors (flow state, node kinds, selection, status) shared by the plan
//! view, legend, toolbar, and side panels — so a swatch in the legend can never
//! drift out of sync with the line it describes.

use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};

/// Semantic colors. Names describe *meaning*, not hue.
pub mod palette {
    use eframe::egui::Color32;

    /// UI accent (selected tab, focus, primary highlight) — matches normal flow.
    pub const ACCENT: Color32 = Color32::from_rgb(66, 150, 245);

    // ── Pipe / flow state ────────────────────────────────────────────────
    /// Pipe carrying its design flow within capacity.
    pub const FLOW_OK: Color32 = Color32::from_rgb(80, 160, 255);
    /// Surcharged pipe or design error.
    pub const ERROR: Color32 = Color32::from_rgb(224, 64, 64);
    /// Design warning flagged by review.
    pub const WARNING: Color32 = Color32::from_rgb(226, 162, 60);
    /// Selection highlight.
    pub const SELECTION: Color32 = Color32::from_rgb(255, 224, 64);
    /// "All clear" / passing state.
    pub const OK_GREEN: Color32 = Color32::from_rgb(96, 200, 120);

    // ── Node kinds ───────────────────────────────────────────────────────
    pub const NODE_INLET: Color32 = Color32::from_rgb(60, 220, 120);
    pub const NODE_JUNCTION: Color32 = Color32::from_rgb(180, 120, 255);
    pub const NODE_OUTFALL: Color32 = Color32::from_rgb(255, 180, 60);

    // ── Status chips ─────────────────────────────────────────────────────
    /// Results out of date with the inputs.
    pub const STALE: Color32 = Color32::from_rgb(240, 200, 64);
    /// Unsaved project changes.
    pub const UNSAVED: Color32 = Color32::from_rgb(120, 190, 255);

    // ── Canvas — always dark, whatever the UI theme (CAD convention). ─────
    /// Plan/profile canvas background.
    pub const CANVAS_BG: Color32 = Color32::from_gray(26);
    /// Grid lines (premultiplied, subtle).
    pub const GRID: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 16);
    /// Muted overlay text (headers, hints, legend labels) on the dark canvas.
    pub const MUTED: Color32 = Color32::from_gray(170);
    /// Ground surface line in the profile.
    pub const PROFILE_GROUND: Color32 = Color32::from_rgb(150, 100, 52);
    /// Pipe invert line in the profile.
    pub const PROFILE_INVERT: Color32 = Color32::from_gray(165);

    // ── Status text — legible on both light and dark panel backgrounds. ───
    // Panels/toolbars sit on themed surfaces, so status text must adapt; the
    // vivid canvas colors above never change. Pass `ui.visuals().dark_mode`.
    pub fn error_text(dark: bool) -> Color32 {
        if dark { ERROR } else { Color32::from_rgb(190, 44, 44) }
    }
    pub fn warning_text(dark: bool) -> Color32 {
        if dark { WARNING } else { Color32::from_rgb(168, 110, 20) }
    }
    pub fn ok_text(dark: bool) -> Color32 {
        if dark { OK_GREEN } else { Color32::from_rgb(30, 138, 70) }
    }
    pub fn stale_text(dark: bool) -> Color32 {
        if dark { STALE } else { Color32::from_rgb(158, 120, 20) }
    }
    pub fn accent_text(dark: bool) -> Color32 {
        if dark { UNSAVED } else { ACCENT }
    }
    /// Muted label text for UI panels (distinct from canvas [`MUTED`]).
    pub fn muted_text(dark: bool) -> Color32 {
        if dark { Color32::from_gray(160) } else { Color32::from_gray(110) }
    }
}

/// UI color scheme. The drawing canvas stays dark in both variants (CAD
/// convention); only the surrounding chrome changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn is_dark(self) -> bool {
        matches!(self, Theme::Dark)
    }
}

/// Install the StormSewer theme (dark or light) into the egui context.
/// Idempotent — call at startup and whenever the user toggles the scheme.
pub fn apply(ctx: &egui::Context, theme: Theme) {
    let mut style = (*ctx.style()).clone();

    // ── Spacing: a touch more breathing room than egui's defaults (shared). ─
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(9.0, 4.0);
    s.menu_margin = egui::Margin::same(6.0);
    s.interact_size.y = 26.0;

    let rounding = Rounding::same(5.0);
    let mut v = if theme.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // ── Shared: rounding, accent selection, links. ────────────────────────
    v.window_rounding = rounding;
    v.menu_rounding = rounding;
    v.hyperlink_color = palette::ACCENT;
    v.selection.stroke = Stroke::new(1.0, palette::ACCENT);

    if theme.is_dark() {
        // Layered dark surfaces.
        v.panel_fill = Color32::from_gray(32);
        v.window_fill = Color32::from_gray(38);
        v.extreme_bg_color = Color32::from_gray(20);
        v.faint_bg_color = Color32::from_gray(46);
        v.window_stroke = Stroke::new(1.0, Color32::from_gray(62));
        v.selection.bg_fill = Color32::from_rgb(40, 82, 140);
    } else {
        // Soft light surfaces (a hair grayer than pure white for comfort).
        v.panel_fill = Color32::from_gray(240);
        v.window_fill = Color32::from_gray(250);
        v.extreme_bg_color = Color32::from_gray(255);
        v.faint_bg_color = Color32::from_gray(232);
        v.window_stroke = Stroke::new(1.0, Color32::from_gray(200));
        v.selection.bg_fill = Color32::from_rgb(197, 222, 252);
    }

    // ── Widget states: shared rounding + scheme-appropriate fills. ────────
    let w = &mut v.widgets;
    for wv in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        wv.rounding = rounding;
    }
    if theme.is_dark() {
        w.inactive.bg_fill = Color32::from_gray(54);
        w.inactive.weak_bg_fill = Color32::from_gray(50);
        w.hovered.bg_fill = Color32::from_gray(66);
        w.hovered.weak_bg_fill = Color32::from_gray(62);
        w.active.bg_fill = Color32::from_rgb(46, 96, 160);
        w.active.weak_bg_fill = Color32::from_rgb(42, 86, 144);
    } else {
        w.inactive.bg_fill = Color32::from_gray(225);
        w.inactive.weak_bg_fill = Color32::from_gray(230);
        w.hovered.bg_fill = Color32::from_gray(212);
        w.hovered.weak_bg_fill = Color32::from_gray(218);
        w.active.bg_fill = Color32::from_rgb(120, 172, 244);
        w.active.weak_bg_fill = Color32::from_rgb(150, 192, 248);
    }

    style.visuals = v;
    ctx.set_style(style);
}
