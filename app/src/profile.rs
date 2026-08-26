// SPDX-License-Identifier: GPL-3.0-or-later

//! Profile (long-section) view rendering for the StormSewer desktop application.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use stormsewer::drawing::{draw_network, DrawConfig, Polyline, ProfileRole};
use stormsewer::io::Project;
use stormsewer::network::Analysis;

use crate::theme::palette;

// Asymmetric plot margins: the left holds elevation labels, the bottom
// holds station labels + title, the top holds the header line and any node
// labels that ride above high ground; the right only clears the legend.
const PAD_LEFT: f32 = 64.0;
const PAD_RIGHT: f32 = 24.0;
const PAD_TOP: f32 = 48.0;
const PAD_BOTTOM: f32 = 52.0;

fn inner_plot(rect: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(rect.left() + PAD_LEFT, rect.top() + PAD_TOP),
        Pos2::new(rect.right() - PAD_RIGHT, rect.bottom() - PAD_BOTTOM),
    )
}

/// Draw the hydraulic profile view scaled to fit `rect`.
pub fn draw_profile(
    ui: &mut egui::Ui,
    rect: Rect,
    project: &Project,
    analysis: Option<&Analysis>,
    profile_run: &[String],
) {
    let dark = ui.visuals().dark_mode;
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));

    let Some(analysis) = analysis else {
        painter.text(
            rect.center() - egui::Vec2::new(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            "No profile yet",
            egui::FontId::proportional(16.0),
            palette::canvas::muted(dark),
        );
        painter.text(
            rect.center() + egui::Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            "Run the analysis (F5) to draw inverts, ground, and HGL",
            egui::FontId::proportional(12.0),
            palette::canvas::muted(dark),
        );
        crate::theme::draw_sheet_frame(&painter, rect, project, dark);
        return;
    };

    let net = project.to_network();
    crate::theme::draw_sheet_frame(&painter, rect, project, dark);
    let selected_run = !profile_run.is_empty();
    let drawing = if selected_run {
        stormsewer::drawing::draw_profile_run(
            &net,
            analysis,
            &DrawConfig::default(),
            profile_run,
        )
    } else {
        draw_network(&net, analysis, &DrawConfig::default())
    };
    let header = if selected_run {
        let list = if profile_run.len() <= 4 {
            profile_run.join(" → ")
        } else {
            format!("{} pipes", profile_run.len())
        };
        format!("Profile · selected run ({list}) — Esc clears")
    } else {
        "Profile · main trunk — Shift-click pipes in Plan to profile a branch"
            .to_owned()
    };
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        header,
        egui::FontId::proportional(13.0),
        palette::canvas::muted(dark),
    );

    if drawing.profile_lines.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No profile data — the selected pipes were not found",
            egui::FontId::proportional(16.0),
            palette::canvas::muted(dark),
        );
        return;
    }

    let Some((min_x, min_y, max_x, max_y)) = profile_bounds(&drawing.profile_lines) else {
        return;
    };

    let to_screen = |x: f64, y: f64| -> Pos2 {
        profile_to_screen(x, y, min_x, min_y, max_x, max_y, rect)
    };

    // Structure shafts: each profiled node drawn at its real barrel width
    // from invert to rim, under the ground/invert/HGL lines.
    for (cx, half_w, y_a, y_b) in
        structure_shafts(project, &drawing.profile_labels, drawing.profile_datum)
    {
        let r = Rect::from_two_pos(
            to_screen(cx - half_w, y_a),
            to_screen(cx + half_w, y_b),
        );
        painter.rect_filled(r, 1.0, palette::canvas::faint_fill(dark));
        painter.rect_stroke(r, 1.0, Stroke::new(1.0, palette::canvas::line(dark)));
    }

    for pl in &drawing.profile_lines {
        let color = profile_role_color(pl.role, dark);
        let stroke = Stroke::new(profile_stroke_width(pl.role), color);
        for window in pl.pts.windows(2) {
            let a = to_screen(window[0].0, window[0].1);
            let b = to_screen(window[1].0, window[1].1);
            painter.line_segment([a, b], stroke);
        }
    }

    for lbl in &drawing.profile_labels {
        let mut pos = to_screen(lbl.x, lbl.y);
        // Never let a structure label climb into the header band.
        pos.y = pos.y.max(rect.top() + PAD_TOP + 12.0);
        painter.text(
            pos,
            egui::Align2::CENTER_BOTTOM,
            &lbl.text,
            egui::FontId::monospace(11.0),
            palette::canvas::ink(dark),
        );
    }

    draw_station_axis(&painter, dark, rect, min_x, max_x, min_y, &to_screen);
    draw_elevation_axis(&painter, dark, rect, min_x, min_y, max_y, drawing.profile_datum, &to_screen);
    draw_legend(&painter, dark, rect, analysis);
}

/// Vertical elevation axis with gridlines and absolute-elevation tick labels,
/// recovered from the profile datum and the default vertical exaggeration.
fn draw_elevation_axis(
    painter: &egui::Painter,
    dark: bool,
    rect: Rect,
    min_x: f64,
    min_y: f64,
    max_y: f64,
    datum: f64,
    to_screen: &dyn Fn(f64, f64) -> Pos2,
) {
    let cfg = DrawConfig::default();
    // draw-Y (post-exaggeration) → absolute elevation (ft).
    let elev = |dy: f64| datum + (dy - cfg.profile_origin_y) / cfg.v_exag;
    let (e_lo, e_hi) = (elev(min_y), elev(max_y));
    if !(e_hi > e_lo) {
        return;
    }
    let step = station_tick_step(e_hi - e_lo);
    let axis_x = rect.left() + PAD_LEFT;
    let right = rect.right() - PAD_RIGHT;

    let mut e = (e_lo / step).ceil() * step;
    while e <= e_hi + step * 0.01 {
        let dy = cfg.profile_origin_y + (e - datum) * cfg.v_exag;
        let y = to_screen(min_x, dy).y;
        // faint gridline across the plot
        painter.line_segment(
            [Pos2::new(axis_x, y), Pos2::new(right, y)],
            Stroke::new(1.0, palette::canvas::grid(dark)),
        );
        painter.text(
            Pos2::new(axis_x - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{e:.0}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        e += step;
    }

    // Title lives in the top-left margin, under the header line, clear of
    // both the tick labels and the plot.
    painter.text(
        Pos2::new(rect.left() + 12.0, rect.top() + 30.0),
        egui::Align2::LEFT_TOP,
        "Elev (ft)",
        egui::FontId::proportional(11.0),
        palette::canvas::muted(dark),
    );
}

fn profile_bounds(lines: &[Polyline]) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;

    for pl in lines {
        for &(x, y) in &pl.pts {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            any = true;
        }
    }

    if !any || min_x >= max_x || min_y >= max_y {
        return None;
    }
    Some((min_x, min_y, max_x, max_y))
}

fn profile_to_screen(
    x: f64,
    y: f64,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    rect: Rect,
) -> Pos2 {
    let inner = inner_plot(rect);
    let draw_w = (max_x - min_x).max(1e-6);
    let draw_h = (max_y - min_y).max(1e-6);
    let scale = (inner.width() as f64 / draw_w).min(inner.height() as f64 / draw_h);
    let content_w = draw_w * scale;
    let content_h = draw_h * scale;
    let offset_x = inner.left() as f64 + (inner.width() as f64 - content_w) * 0.5;
    let offset_y = inner.top() as f64 + (inner.height() as f64 - content_h) * 0.5;

    Pos2::new(
        (offset_x + (x - min_x) * scale) as f32,
        (offset_y + (max_y - y) * scale) as f32,
    )
}

fn profile_role_color(role: ProfileRole, dark: bool) -> Color32 {
    match role {
        ProfileRole::Ground => palette::PROFILE_GROUND,
        ProfileRole::Invert => palette::canvas::invert_line(dark),
        ProfileRole::Hgl => palette::canvas::hgl(dark),
        // EGL: a lighter water-blue, thinner than the HGL below it.
        ProfileRole::Egl => palette::canvas::egl(dark),
    }
}

fn profile_stroke_width(role: ProfileRole) -> f32 {
    match role {
        ProfileRole::Ground => 2.5,
        ProfileRole::Invert => 2.0,
        ProfileRole::Hgl => 2.5,
        ProfileRole::Egl => 1.5,
    }
}

fn station_tick_step(range: f64) -> f64 {
    if range <= 0.0 {
        return 50.0;
    }
    let raw = range / 6.0;
    let magnitude = 10_f64.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let nice = if normalized < 1.5 {
        1.0
    } else if normalized < 3.5 {
        2.0
    } else if normalized < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

fn draw_station_axis(
    painter: &egui::Painter,
    dark: bool,
    rect: Rect,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    to_screen: &dyn Fn(f64, f64) -> Pos2,
) {
    let cfg = DrawConfig::default();
    let step = station_tick_step(max_x - min_x);
    let _ = min_y;
    // Fixed band inside the bottom margin: the axis line, then numbers,
    // then the title — none of them can collide with the plot or each
    // other whatever the content extents are.
    let axis_screen_y = rect.bottom() - PAD_BOTTOM + 12.0;

    painter.line_segment(
        [
            Pos2::new(rect.left() + PAD_LEFT, axis_screen_y),
            Pos2::new(rect.right() - PAD_RIGHT, axis_screen_y),
        ],
        Stroke::new(1.0, palette::canvas::line(dark)),
    );

    let mut st = (min_x / step).floor() * step;
    while st <= max_x + step * 0.01 {
        let station_ft = (st - cfg.profile_origin_x) / cfg.h_scale;
        let tick_x = to_screen(st, min_y).x;
        painter.line_segment(
            [
                Pos2::new(tick_x, axis_screen_y),
                Pos2::new(tick_x, axis_screen_y + 5.0),
            ],
            Stroke::new(1.0, palette::canvas::line(dark)),
        );
        painter.text(
            Pos2::new(tick_x, axis_screen_y + 8.0),
            egui::Align2::CENTER_TOP,
            format!("{station_ft:.0}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        st += step;
    }

    painter.text(
        rect.center_bottom() - Vec2::new(0.0, 4.0),
        egui::Align2::CENTER_BOTTOM,
        "Station (ft)",
        egui::FontId::proportional(11.0),
        Color32::from_gray(160),
    );
}

fn draw_legend(painter: &egui::Painter, dark: bool, rect: Rect, analysis: &Analysis) {
    let entries = [
        (ProfileRole::Ground, "Ground"),
        (ProfileRole::Invert, "Invert"),
        (ProfileRole::Hgl, "HGL"),
        (ProfileRole::Egl, "EGL"),
    ];

    // Anchored top-right so it clears the left-side elevation axis.
    let box_w = 132.0;
    let mut pos = Pos2::new(rect.right() - PAD_RIGHT - box_w, rect.top() + 12.0);
    painter.text(
        pos,
        egui::Align2::LEFT_TOP,
        "Profile view",
        egui::FontId::proportional(13.0),
        palette::canvas::muted(dark),
    );
    pos.y += 20.0;

    for (role, label) in entries {
        let color = profile_role_color(role, dark);
        let line_y = pos.y + 6.0;
        painter.line_segment(
            [Pos2::new(pos.x, line_y), Pos2::new(pos.x + 28.0, line_y)],
            Stroke::new(profile_stroke_width(role), color),
        );
        painter.text(
            Pos2::new(pos.x + 36.0, line_y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            Color32::from_gray(220),
        );
        pos.y += 18.0;
    }

    let surcharged: Vec<&str> = analysis
        .pipes
        .iter()
        .filter(|p| p.surcharged)
        .map(|p| p.id.as_str())
        .collect();
    if !surcharged.is_empty() {
        pos.y += 6.0;
        painter.text(
            pos,
            egui::Align2::LEFT_TOP,
            format!("Surcharged: {}", surcharged.join(", ")),
            egui::FontId::proportional(11.0),
            palette::ERROR,
        );
    }
}
/// Draw-space shafts `(x_center, half_width, y_invert, y_rim)` for every
/// node labeled on the profiled stem(s), sized by each structure's barrel
/// diameter (default 4 ft).
fn structure_shafts(
    project: &Project,
    labels: &[stormsewer::drawing::Label],
    datum: f64,
) -> Vec<(f64, f64, f64, f64)> {
    let cfg = DrawConfig::default();
    let py = |elev: f64| cfg.profile_origin_y + (elev - datum) * cfg.v_exag;
    labels
        .iter()
        .filter_map(|l| {
            let n = project.nodes.iter().find(|n| n.id == l.text)?;
            let half_w = (n.diameter_ft.max(0.5) * cfg.h_scale) / 2.0;
            Some((l.x, half_w, py(n.invert), py(n.rim)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stormsewer::drawing::Label;

    #[test]
    fn shafts_size_from_structure_diameter() {
        let mut p = Project::empty(); // seeds OUT: invert 100, rim 106
        p.nodes[0].diameter_ft = 6.0;
        let cfg = DrawConfig::default();
        let labels = vec![Label {
            x: 42.0,
            y: 0.0,
            text: "OUT".into(),
            height: 2.0,
        }];
        let shafts = structure_shafts(&p, &labels, 100.0);
        assert_eq!(shafts.len(), 1);
        let (cx, half_w, y_inv, y_rim) = shafts[0];
        assert_eq!(cx, 42.0);
        assert!((half_w - 6.0 * cfg.h_scale / 2.0).abs() < 1e-9);
        // 6 ft of rise under the vertical exaggeration.
        assert!(((y_rim - y_inv) - 6.0 * cfg.v_exag).abs() < 1e-9);
        // Unknown labels are skipped, not fabricated.
        let none = structure_shafts(
            &p,
            &[Label { x: 0.0, y: 0.0, text: "NOPE".into(), height: 2.0 }],
            100.0,
        );
        assert!(none.is_empty());
    }
}
