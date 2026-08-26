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

    /// UI accent (selected tab, focus, active tool, links): survey-flag
    /// pink. In the APWA uniform marking code, pink is *temporary survey
    /// markings* — the color of work in progress on the ground — which is
    /// exactly what selection and an armed tool mean here. It also leaves
    /// blue free to always mean water, green to mean passing, and amber to
    /// mean warning; nothing else in the palette can be mistaken for it.
    pub const ACCENT: Color32 = Color32::from_rgb(224, 86, 127);

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
    fonts::install(ctx);
    let mut style = (*ctx.style()).clone();

    // ── Type scale: Plex Sans for UI, Plex Mono for data, semibold headings.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(16.0, fonts::semibold())),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
    ]
    .into();

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
        // Asphalt: cool blue-gray layers, not neutral gray — the chrome
        // recedes like wet pavement and lets the drawing carry the color.
        v.panel_fill = Color32::from_rgb(21, 25, 31);
        v.window_fill = Color32::from_rgb(27, 32, 40);
        v.extreme_bg_color = Color32::from_rgb(15, 18, 23);
        v.faint_bg_color = Color32::from_rgb(33, 39, 48);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(53, 61, 72));
        v.selection.bg_fill = Color32::from_rgb(88, 40, 58);
    } else {
        // Bond paper: warm near-white with ink-gray strokes, like a sheet
        // on a drafting table rather than a software-gray dialog.
        v.panel_fill = Color32::from_rgb(246, 244, 239);
        v.window_fill = Color32::from_rgb(252, 251, 248);
        v.extreme_bg_color = Color32::from_rgb(255, 255, 254);
        v.faint_bg_color = Color32::from_rgb(238, 235, 228);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(207, 202, 192));
        v.selection.bg_fill = Color32::from_rgb(246, 209, 222);
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
        w.inactive.bg_fill = Color32::from_rgb(42, 49, 59);
        w.inactive.weak_bg_fill = Color32::from_rgb(38, 44, 54);
        w.hovered.bg_fill = Color32::from_rgb(54, 62, 74);
        w.hovered.weak_bg_fill = Color32::from_rgb(49, 57, 68);
        w.active.bg_fill = Color32::from_rgb(140, 52, 82);
        w.active.weak_bg_fill = Color32::from_rgb(120, 46, 72);
    } else {
        w.inactive.bg_fill = Color32::from_rgb(232, 229, 222);
        w.inactive.weak_bg_fill = Color32::from_rgb(237, 234, 228);
        w.hovered.bg_fill = Color32::from_rgb(222, 218, 209);
        w.hovered.weak_bg_fill = Color32::from_rgb(228, 224, 216);
        w.active.bg_fill = Color32::from_rgb(228, 140, 168);
        w.active.weak_bg_fill = Color32::from_rgb(236, 165, 188);
    }

    style.visuals = v;
    ctx.set_style(style);
}

/// Embedded IBM Plex faces. Plex was drawn for engineering-adjacent work at
/// IBM: neutral, legible at small sizes, with tabular numerals in the mono —
/// exactly what schedules of flows and elevations need. OFL-licensed
/// (app/assets/fonts/OFL.txt); embedding keeps the app a single binary.
pub mod fonts {
    use eframe::egui;

    /// Install Plex as the primary proportional + monospace faces, keeping
    /// egui's built-ins in each family as glyph fallback (symbols, emoji).
    /// Named families expose the heavier weights for text styles.
    pub fn install(ctx: &egui::Context) {
        use egui::{FontData, FontDefinitions, FontFamily};
        let mut f = FontDefinitions::default();
        for (name, bytes) in [
            ("plex-sans", &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..]),
            ("plex-sans-medium", &include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf")[..]),
            ("plex-sans-semibold", &include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")[..]),
            ("plex-mono", &include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")[..]),
            ("plex-mono-medium", &include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf")[..]),
        ] {
            f.font_data
                .insert(name.to_owned(), FontData::from_static(bytes));
        }
        let prop_fallback: Vec<String> = f.families[&FontFamily::Proportional].clone();
        let mono_fallback: Vec<String> = f.families[&FontFamily::Monospace].clone();
        f.families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "plex-sans".into());
        f.families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .insert(0, "plex-mono".into());
        for (family, face) in [
            ("plex-medium", "plex-sans-medium"),
            ("plex-semibold", "plex-sans-semibold"),
        ] {
            let mut list = vec![face.to_owned()];
            list.extend(prop_fallback.iter().cloned());
            f.families.insert(FontFamily::Name(family.into()), list);
        }
        let mut list = vec!["plex-mono-medium".to_owned()];
        list.extend(mono_fallback.iter().cloned());
        f.families
            .insert(FontFamily::Name("mono-medium".into()), list);
        ctx.set_fonts(f);
    }

    /// Heading family (installed by [`install`]; falls back if absent).
    pub fn semibold() -> egui::FontFamily {
        egui::FontFamily::Name("plex-semibold".into())
    }
}

/// Draw the sheet frame and corner title block on a plan/profile canvas.
///
/// The signature element of the app: a thin drawing-sheet border with
/// registration ticks and a title block carrying the project name, design
/// storm, and unit system — so the canvas reads as a drawing, and every
/// screenshot leaves the app looking like a plan sheet. Skipped on small
/// canvases where it would crowd the work.
pub fn draw_sheet_frame(
    painter: &egui::Painter,
    rect: egui::Rect,
    project: &stormsewer::io::Project,
) {
    use eframe::egui::{Align2, FontId, Pos2, Rect, Vec2};

    if rect.width() < 430.0 || rect.height() < 280.0 {
        return;
    }
    let line = Color32::from_rgba_premultiplied(255, 255, 255, 46);
    let frame = rect.shrink(6.0);
    painter.rect_stroke(frame, 0.0, Stroke::new(1.2, line));
    // Registration ticks at the frame midpoints.
    for (p, d) in [
        (Pos2::new(frame.center().x, frame.top()), Vec2::new(0.0, 5.0)),
        (Pos2::new(frame.center().x, frame.bottom()), Vec2::new(0.0, -5.0)),
        (Pos2::new(frame.left(), frame.center().y), Vec2::new(5.0, 0.0)),
        (Pos2::new(frame.right(), frame.center().y), Vec2::new(-5.0, 0.0)),
    ] {
        painter.line_segment([p, p + d], Stroke::new(1.2, line));
    }

    let (w, h) = (216.0, 60.0);
    let tb = Rect::from_min_size(
        Pos2::new(frame.right() - w, frame.bottom() - h),
        Vec2::new(w, h),
    );
    painter.rect_filled(tb, 0.0, Color32::from_rgba_premultiplied(16, 18, 22, 235));
    painter.rect_stroke(tb, 0.0, Stroke::new(1.2, line));

    let pad = 8.0;
    let mut name = project.name.trim().to_owned();
    if name.is_empty() {
        name = "UNTITLED".into();
    }
    name = name.to_uppercase();
    if name.chars().count() > 26 {
        name = name.chars().take(25).collect::<String>() + "…";
    }
    painter.text(
        tb.left_top() + Vec2::new(pad, 6.0),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(11.5),
        Color32::from_gray(225),
    );
    painter.line_segment(
        [
            Pos2::new(tb.left() + pad, tb.top() + 23.0),
            Pos2::new(tb.right() - pad, tb.top() + 23.0),
        ],
        Stroke::new(1.0, line),
    );
    let mono = FontId::monospace(9.0);
    let units = match project.units {
        stormsewer::units::UnitSystem::UsCustomary => "U.S. CUSTOMARY",
        stormsewer::units::UnitSystem::Si => "SI (METRIC)",
    };
    painter.text(
        tb.left_top() + Vec2::new(pad, 28.0),
        Align2::LEFT_TOP,
        format!(
            "DESIGN STORM  {:.0}-YR",
            project.design_return_period_years
        ),
        mono.clone(),
        palette::MUTED,
    );
    painter.text(
        tb.left_top() + Vec2::new(pad, 40.0),
        Align2::LEFT_TOP,
        format!("UNITS         {units}"),
        mono,
        palette::MUTED,
    );
    painter.text(
        Pos2::new(tb.right() - pad, tb.bottom() - 4.0),
        Align2::RIGHT_BOTTOM,
        "STORMSEWER",
        FontId::monospace(8.0),
        Color32::from_gray(115),
    );
}
