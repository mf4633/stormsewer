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

    // ── Canvas ───────────────────────────────────────────────────────────
    /// Plan/profile canvas background.
    pub const CANVAS_BG: Color32 = Color32::from_gray(26);
    /// Grid lines (premultiplied, subtle).
    pub const GRID: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 16);
    /// Muted overlay text (headers, hints, legend labels).
    pub const MUTED: Color32 = Color32::from_gray(170);
}

/// Install the StormSewer dark theme into the egui context. Idempotent — safe
/// to call once at startup.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // ── Spacing: a touch more breathing room than egui's defaults. ────────
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(9.0, 4.0);
    s.menu_margin = egui::Margin::same(6.0);
    s.interact_size.y = 26.0;

    // ── Visuals: layered dark surfaces with a single accent. ──────────────
    let v = &mut style.visuals;
    v.dark_mode = true;
    let rounding = Rounding::same(5.0);
    v.window_rounding = rounding;
    v.menu_rounding = rounding;

    v.panel_fill = Color32::from_gray(32);
    v.window_fill = Color32::from_gray(38);
    v.extreme_bg_color = Color32::from_gray(20);
    v.faint_bg_color = Color32::from_gray(46);
    v.window_stroke = Stroke::new(1.0, Color32::from_gray(62));

    // Accent-driven selection & links.
    v.selection.bg_fill = Color32::from_rgb(40, 82, 140);
    v.selection.stroke = Stroke::new(1.0, palette::ACCENT);
    v.hyperlink_color = palette::ACCENT;

    // Widget states: shared rounding, subtle hover, accent when active.
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
    w.inactive.bg_fill = Color32::from_gray(54);
    w.inactive.weak_bg_fill = Color32::from_gray(50);
    w.hovered.bg_fill = Color32::from_gray(66);
    w.hovered.weak_bg_fill = Color32::from_gray(62);
    w.active.bg_fill = Color32::from_rgb(46, 96, 160);
    w.active.weak_bg_fill = Color32::from_rgb(42, 86, 144);

    ctx.set_style(style);
}
