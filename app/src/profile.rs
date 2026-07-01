// SPDX-License-Identifier: GPL-3.0-or-later

//! Profile (long-section) view rendering for the StormSewer desktop application.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use stormsewer::drawing::{draw_network, DrawConfig, Polyline, ProfileRole};
use stormsewer::io::Project;
use stormsewer::network::Analysis;

const PADDING: f32 = 24.0;

/// Draw the hydraulic profile view scaled to fit `rect`.
pub fn draw_profile(
    ui: &mut egui::Ui,
    rect: Rect,
    project: &Project,
    analysis: Option<&Analysis>,
) {
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, Color32::from_gray(28));

    let Some(analysis) = analysis else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Run analysis to view profile",
            egui::FontId::proportional(16.0),
            Color32::from_gray(160),
        );
        return;
    };

    let net = project.to_network();
    let drawing = draw_network(&net, analysis, &DrawConfig::default());

    if drawing.profile_lines.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No profile data",
            egui::FontId::proportional(16.0),
            Color32::from_gray(160),
        );
        return;
    }

    let Some((min_x, min_y, max_x, max_y)) = profile_bounds(&drawing.profile_lines) else {
        return;
    };

    let to_screen = |x: f64, y: f64| -> Pos2 {
        profile_to_screen(x, y, min_x, min_y, max_x, max_y, rect)
    };

    for pl in &drawing.profile_lines {
        let color = profile_role_color(pl.role);
        let stroke = Stroke::new(profile_stroke_width(pl.role), color);
        for window in pl.pts.windows(2) {
            let a = to_screen(window[0].0, window[0].1);
            let b = to_screen(window[1].0, window[1].1);
            painter.line_segment([a, b], stroke);
        }
    }

    for lbl in &drawing.profile_labels {
        let pos = to_screen(lbl.x, lbl.y);
        painter.text(
            pos,
            egui::Align2::CENTER_BOTTOM,
            &lbl.text,
            egui::FontId::monospace(11.0),
            Color32::WHITE,
        );
    }

    draw_station_axis(&painter, rect, min_x, max_x, min_y, &to_screen);
    draw_legend(&painter, rect, analysis);
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
    let inner = rect.shrink(PADDING);
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

fn profile_role_color(role: ProfileRole) -> Color32 {
    match role {
        ProfileRole::Ground => Color32::from_rgb(139, 90, 43),
        ProfileRole::Invert => Color32::from_gray(160),
        ProfileRole::Hgl => Color32::from_rgb(80, 160, 255),
    }
}

fn profile_stroke_width(role: ProfileRole) -> f32 {
    match role {
        ProfileRole::Ground => 2.5,
        ProfileRole::Invert => 2.0,
        ProfileRole::Hgl => 2.5,
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
    rect: Rect,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    to_screen: &dyn Fn(f64, f64) -> Pos2,
) {
    let cfg = DrawConfig::default();
    let step = station_tick_step(max_x - min_x);
    let axis_screen_y = to_screen(min_x, min_y).y + 6.0;

    painter.line_segment(
        [
            Pos2::new(rect.left() + PADDING, axis_screen_y),
            Pos2::new(rect.right() - PADDING, axis_screen_y),
        ],
        Stroke::new(1.0, Color32::from_gray(100)),
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
            Stroke::new(1.0, Color32::from_gray(140)),
        );
        painter.text(
            Pos2::new(tick_x, axis_screen_y + 8.0),
            egui::Align2::CENTER_TOP,
            format!("{station_ft:.0}"),
            egui::FontId::monospace(10.0),
            Color32::from_gray(180),
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

fn draw_legend(painter: &egui::Painter, rect: Rect, analysis: &Analysis) {
    let entries = [
        (ProfileRole::Ground, "Ground"),
        (ProfileRole::Invert, "Invert"),
        (ProfileRole::Hgl, "HGL"),
    ];

    let mut pos = rect.left_top() + Vec2::new(12.0, 12.0);
    painter.text(
        pos,
        egui::Align2::LEFT_TOP,
        "Profile View",
        egui::FontId::proportional(13.0),
        Color32::from_gray(180),
    );
    pos.y += 20.0;

    for (role, label) in entries {
        let color = profile_role_color(role);
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
            Color32::WHITE,
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
            format!("Surcharged pipes: {}", surcharged.join(", ")),
            egui::FontId::proportional(11.0),
            Color32::from_rgb(220, 60, 60),
        );
    }
}