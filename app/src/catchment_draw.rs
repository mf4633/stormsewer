// SPDX-License-Identifier: GPL-3.0-or-later

//! Plan-view catchment polygon drawing and interactive placement.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use stormsewer::catchment::{default_flow_length_ft, polygon_centroid, shoelace_area_sqft, sqft_to_acres};
use stormsewer::io::project::{Project, ProjectCatchment};

use crate::edit::{snap_node, EditState};
use crate::viewport::Viewport;

const CLOSE_RADIUS: f64 = 20.0;
const DEFAULT_C: f64 = 0.7;
const DEFAULT_SLOPE: f64 = 0.01;

/// Draw saved catchments and the in-progress polygon (semi-transparent green).
pub fn draw_catchments(
    painter: &Painter,
    project: &Project,
    edit: &EditState,
    viewport: &Viewport,
    rect: Rect,
) {
    let fill = Color32::from_rgba_premultiplied(40, 180, 80, 70);
    let edge = Color32::from_rgb(60, 200, 100);
    let stroke = Stroke::new(2.0, edge);

    for (i, catchment) in project.catchments.iter().enumerate() {
        if catchment.vertices.len() < 3 {
            continue;
        }
        let selected = edit.selected_catchment == Some(i);
        let points: Vec<Pos2> = catchment
            .vertices
            .iter()
            .map(|(x, y)| viewport.world_to_screen(rect, *x, *y))
            .collect();
        let poly_fill = if selected {
            Color32::from_rgba_premultiplied(80, 220, 120, 110)
        } else {
            fill
        };
        let poly_stroke = if selected {
            Stroke::new(3.0, Color32::from_rgb(255, 255, 80))
        } else {
            stroke
        };
        painter.add(Shape::convex_polygon(points.clone(), poly_fill, poly_stroke));

        if let Some((cx, cy)) = catchment_label_pos(&catchment.vertices) {
            let area_ac = sqft_to_acres(shoelace_area_sqft(&catchment.vertices));
            let label = format!("{} ({:.2} ac)", catchment.id, area_ac);
            painter.text(
                viewport.world_to_screen(rect, cx, cy),
                eframe::egui::Align2::CENTER_CENTER,
                label,
                eframe::egui::FontId::proportional(11.0),
                Color32::from_rgb(200, 255, 200),
            );
        }
    }

    if !edit.catchment_vertices.is_empty() {
        let screen_pts: Vec<Pos2> = edit
            .catchment_vertices
            .iter()
            .map(|(x, y)| viewport.world_to_screen(rect, *x, *y))
            .collect();

        for w in screen_pts.windows(2) {
            painter.line_segment([w[0], w[1]], stroke);
        }

        let preview_fill = Color32::from_rgba_premultiplied(80, 220, 120, 50);
        if edit.catchment_vertices.len() >= 3 {
            painter.add(Shape::convex_polygon(screen_pts.clone(), preview_fill, stroke));
        }

        for pt in &screen_pts {
            painter.circle_filled(*pt, 4.0, edge);
        }
    }
}

fn catchment_label_pos(vertices: &[(f64, f64)]) -> Option<(f64, f64)> {
    if vertices.len() < 3 {
        return None;
    }
    Some(polygon_centroid(vertices))
}

/// Handle a plan click while the DrawCatchment tool is active.
///
/// Left-click adds vertices; clicking near the first vertex (with >= 3 points) closes the polygon.
pub fn handle_catchment_click(
    project: &mut Project,
    edit: &mut EditState,
    x: f64,
    y: f64,
) -> Option<String> {
    if edit.catchment_vertices.len() >= 3 {
        let (fx, fy) = edit.catchment_vertices[0];
        let dx = x - fx;
        let dy = y - fy;
        if (dx * dx + dy * dy).sqrt() <= CLOSE_RADIUS {
            return finish_catchment(project, edit);
        }
    }

    edit.catchment_vertices.push((x, y));
    let n = edit.catchment_vertices.len();
    Some(format!("Catchment vertex {n} — click near first point to close (need >= 3)"))
}

/// Commit the in-progress polygon as a [`ProjectCatchment`].
pub fn finish_catchment(project: &mut Project, edit: &mut EditState) -> Option<String> {
    if edit.catchment_vertices.len() < 3 {
        return Some("Need at least 3 vertices to close catchment".into());
    }

    let vertices = edit.catchment_vertices.clone();
    let centroid = polygon_centroid(&vertices);
    let inlet_node_id = nearest_inlet(project, centroid);

    let flow_length_ft = inlet_node_id
        .as_ref()
        .and_then(|id| project.nodes.iter().find(|n| &n.id == id))
        .map(|n| default_flow_length_ft(centroid, (n.x, n.y)))
        .unwrap_or(100.0);

    let id = format!("C{}", edit.next_catchment_id);
    edit.next_catchment_id += 1;

    let area_ac = sqft_to_acres(shoelace_area_sqft(&vertices));
    project.catchments.push(ProjectCatchment {
        id: id.clone(),
        vertices,
        c: DEFAULT_C,
        flow_length_ft,
        slope: DEFAULT_SLOPE,
        inlet_node_id: inlet_node_id.clone(),
    });
    edit.catchment_vertices.clear();

    let inlet_msg = inlet_node_id
        .map(|i| format!(", inlet {i}"))
        .unwrap_or_default();
    Some(format!(
        "Added catchment {id} ({area_ac:.2} ac{inlet_msg})"
    ))
}

fn nearest_inlet(project: &Project, from: (f64, f64)) -> Option<String> {
    snap_node(project, from.0, from.1, 200.0).and_then(|idx| {
        let node = &project.nodes[idx];
        if node.kind == "inlet" {
            Some(node.id.clone())
        } else {
            project
                .nodes
                .iter()
                .filter(|n| n.kind == "inlet")
                .min_by(|a, b| {
                    let da = default_flow_length_ft(from, (a.x, a.y));
                    let db = default_flow_length_ft(from, (b.x, b.y));
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|n| n.id.clone())
        }
    })
}