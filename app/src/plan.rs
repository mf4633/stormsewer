// SPDX-License-Identifier: GPL-3.0-or-later

//! Plan-view rendering for the StormSewer desktop application.

use std::collections::HashSet;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, TextureHandle, Vec2};
use stormsewer::design::review::{DesignFinding, Severity};
use stormsewer::drawing::{draw_network, DrawConfig};
use stormsewer::io::{DxfUnderlaySegment, Project};
use stormsewer::network::Analysis;

use crate::catchment_draw::draw_catchments;
use crate::edit::EditState;
use crate::theme::palette;
use crate::viewport::Viewport;

/// Draw the plan view: background image, pipes, structures, and flow labels.
pub fn draw_plan(
    ui: &mut egui::Ui,
    rect: Rect,
    project: &Project,
    analysis: Option<&Analysis>,
    viewport: &Viewport,
    bg_texture: Option<&TextureHandle>,
    dxf_underlay: &[DxfUnderlaySegment],
    edit: &EditState,
    selected_node: Option<usize>,
    selected_pipe: Option<usize>,
    findings: &[DesignFinding],
    tool_label: Option<&str>,
    pipe_preview_to: Option<(f64, f64)>,
) {
    let painter = ui.painter_at(rect);
    let flagged_ids: HashSet<&str> = findings.iter().map(|f| f.id.as_str()).collect();
    let error_ids: HashSet<&str> = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| f.id.as_str())
        .collect();

    painter.rect_filled(rect, 4.0, palette::CANVAS_BG);

    if viewport.zoom > 0.3 {
        draw_grid(&painter, rect, viewport);
    }

    if let Some(bg_dxf) = &project.background_dxf {
        let alpha = (bg_dxf.opacity * 255.0).round() as u8;
        let color = Color32::from_rgba_unmultiplied(100, 100, 100, alpha);
        for seg in dxf_underlay {
            painter.line_segment(
                [
                    viewport.world_to_screen(rect, seg.x1, seg.y1),
                    viewport.world_to_screen(rect, seg.x2, seg.y2),
                ],
                Stroke::new(1.0, color),
            );
        }
    }

    if let (Some(tex), Some(bg)) = (bg_texture, &project.background) {
        let tex_w = tex.size()[0].max(1) as f32;
        let tex_h = tex.size()[1].max(1) as f32;
        let aspect = tex_h as f64 / tex_w as f64;
        let w = bg.width as f32 * viewport.zoom;
        let h = w * (tex_h / tex_w);
        let tl = viewport.world_to_screen(
            rect,
            bg.origin_x,
            bg.origin_y + bg.width * aspect,
        );
        let image_rect = Rect::from_min_size(tl, Vec2::new(w, h));
        painter.image(
            tex.id(),
            image_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_white_alpha((bg.opacity * 255.0) as u8),
        );
    }

    draw_catchments(&painter, project, edit, viewport, rect);

    if let Some(ref from_id) = edit.pipe_from {
        if let (Some(from), Some((wx, wy))) = (
            project.nodes.iter().find(|n| n.id == *from_id),
            pipe_preview_to,
        ) {
            let a = viewport.world_to_screen(rect, from.x, from.y);
            let b = viewport.world_to_screen(rect, wx, wy);
            painter.line_segment([a, b], Stroke::new(2.0, palette::SELECTION));
            painter.circle_filled(b, 5.0, palette::SELECTION);
        }
    }

    let net = project.to_network();
    let drawing = analysis.map(|a| draw_network(&net, a, &DrawConfig::default()));

    if let Some(d) = &drawing {
        let selected_pipe_id = selected_pipe.and_then(|i| project.pipes.get(i)).map(|p| p.id.as_str());
        for pp in d.plan_pipes.iter() {
            let pipe_id = pp.id.as_str();
            let is_selected = selected_pipe_id == Some(pipe_id);
            let color = if is_selected {
                palette::SELECTION
            } else if error_ids.contains(pipe_id) || pp.surcharged {
                palette::ERROR
            } else if flagged_ids.contains(pipe_id) {
                palette::WARNING
            } else {
                palette::FLOW_OK
            };
            let width = if is_selected { 5.0 } else { 3.0 };
            painter.line_segment(
                [
                    viewport.world_to_screen(rect, pp.x1, pp.y1),
                    viewport.world_to_screen(rect, pp.x2, pp.y2),
                ],
                Stroke::new(width, color),
            );
        }
    } else {
        for (i, p) in project.pipes.iter().enumerate() {
            let from = project.nodes.iter().find(|n| n.id == p.from);
            let to = project.nodes.iter().find(|n| n.id == p.to);
            if let (Some(a), Some(b)) = (from, to) {
                let color = if selected_pipe == Some(i) {
                    palette::SELECTION
                } else if error_ids.contains(p.id.as_str()) {
                    palette::ERROR
                } else if flagged_ids.contains(p.id.as_str()) {
                    palette::WARNING
                } else {
                    palette::FLOW_OK
                };
                let width = if selected_pipe == Some(i) { 5.0 } else { 3.0 };
                painter.line_segment(
                    [
                        viewport.world_to_screen(rect, a.x, a.y),
                        viewport.world_to_screen(rect, b.x, b.y),
                    ],
                    Stroke::new(width, color),
                );
            }
        }
    }

    for (i, n) in project.nodes.iter().enumerate() {
        let center = viewport.world_to_screen(rect, n.x, n.y);
        let r = if selected_node == Some(i) { 11.0 } else { 8.0 };
        let mut color = node_color(&n.kind);
        if error_ids.contains(n.id.as_str()) {
            color = palette::ERROR;
        } else if flagged_ids.contains(n.id.as_str()) {
            color = palette::WARNING;
        }
        painter.circle_filled(center, r, color);
        let stroke_color = if selected_node == Some(i) {
            palette::SELECTION
        } else {
            Color32::WHITE
        };
        painter.circle_stroke(center, r, Stroke::new(1.5, stroke_color));
        painter.text(
            center + Vec2::new(12.0, -12.0),
            egui::Align2::LEFT_BOTTOM,
            &n.id,
            egui::FontId::proportional(14.0),
            Color32::WHITE,
        );
    }

    if let Some(d) = &drawing {
        for lbl in &d.plan_labels {
            let pos = viewport.world_to_screen(rect, lbl.x, lbl.y);
            painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                &lbl.text,
                egui::FontId::monospace(12.0),
                Color32::from_rgb(255, 255, 100),
            );
        }
    }

    let mut header = String::from("Plan view");
    if let Some(tool) = tool_label {
        header.push_str("  ·  ");
        header.push_str(tool);
    }
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        header,
        egui::FontId::proportional(13.0),
        palette::MUTED,
    );

    if !project.pipes.is_empty() || !project.nodes.is_empty() {
        draw_legend(&painter, rect);
    }
}

/// Marker style for a legend row.
enum Marker {
    Line,
    Dot,
}

/// Draw a compact color-key in the top-right corner so the plan's color coding
/// is self-explanatory.
fn draw_legend(painter: &egui::Painter, rect: Rect) {
    let rows: [(Marker, Color32, &str); 6] = [
        (Marker::Line, palette::FLOW_OK, "Pipe within capacity"),
        (Marker::Line, palette::ERROR, "Surcharged / error"),
        (Marker::Line, palette::WARNING, "Design warning"),
        (Marker::Dot, palette::NODE_INLET, "Inlet"),
        (Marker::Dot, palette::NODE_JUNCTION, "Junction"),
        (Marker::Dot, palette::NODE_OUTFALL, "Outfall"),
    ];

    let pad = 8.0;
    let row_h = 17.0;
    let marker_w = 16.0;
    let box_w = 172.0;
    let box_h = pad * 2.0 + row_h * rows.len() as f32;
    let origin = rect.right_top() + Vec2::new(-box_w - 12.0, 12.0);
    let bg = Rect::from_min_size(origin, Vec2::new(box_w, box_h));

    painter.rect_filled(bg, 5.0, Color32::from_rgba_unmultiplied(18, 18, 22, 220));
    painter.rect_stroke(bg, 5.0, Stroke::new(1.0, Color32::from_gray(70)));

    for (i, (marker, color, label)) in rows.iter().enumerate() {
        let cy = bg.top() + pad + row_h * i as f32 + row_h / 2.0;
        let mx = bg.left() + pad;
        match marker {
            Marker::Line => {
                painter.line_segment(
                    [Pos2::new(mx, cy), Pos2::new(mx + marker_w, cy)],
                    Stroke::new(3.0, *color),
                );
            }
            Marker::Dot => {
                painter.circle_filled(Pos2::new(mx + marker_w / 2.0, cy), 5.0, *color);
                painter.circle_stroke(
                    Pos2::new(mx + marker_w / 2.0, cy),
                    5.0,
                    Stroke::new(1.0, Color32::WHITE),
                );
            }
        }
        painter.text(
            Pos2::new(mx + marker_w + 7.0, cy),
            egui::Align2::LEFT_CENTER,
            *label,
            egui::FontId::proportional(12.0),
            palette::MUTED,
        );
    }
}

fn draw_grid(painter: &egui::Painter, rect: Rect, viewport: &Viewport) {
    let spacing = 50.0;
    let (wx0, wy0) = viewport.screen_to_world(rect, rect.left_top());
    let (wx1, wy1) = viewport.screen_to_world(rect, rect.right_bottom());
    let stroke = Stroke::new(1.0, palette::GRID);
    let mut x = (wx0 / spacing).floor() * spacing;
    while x <= wx1.max(wx0) {
        let a = viewport.world_to_screen(rect, x, wy0.min(wy1));
        let b = viewport.world_to_screen(rect, x, wy0.max(wy1));
        painter.line_segment([a, b], stroke);
        x += spacing;
    }
    let mut y = (wy0 / spacing).floor() * spacing;
    while y <= wy1.max(wy0) {
        let a = viewport.world_to_screen(rect, wx0.min(wx1), y);
        let b = viewport.world_to_screen(rect, wx0.max(wx1), y);
        painter.line_segment([a, b], stroke);
        y += spacing;
    }
}

fn node_color(kind: &str) -> Color32 {
    match kind {
        "outfall" => palette::NODE_OUTFALL,
        "junction" => palette::NODE_JUNCTION,
        _ => palette::NODE_INLET,
    }
}