// SPDX-License-Identifier: GPL-3.0-or-later

//! Plan-view coordinate transforms and pan/zoom input handling.

use egui::{Pos2, Rect, Response, Ui, Vec2};
use stormsewer::io::Project;

/// Pan/zoom state for the plan-view canvas.
#[derive(Clone, Debug)]
pub struct Viewport {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: Vec2::new(80.0, 400.0),
            zoom: 0.6,
        }
    }
}

impl Viewport {
    /// Convert world (drawing) coordinates to screen position inside `rect`.
    ///
    /// World +Y points up; screen +Y points down.
    pub fn world_to_screen(&self, rect: Rect, x: f64, y: f64) -> Pos2 {
        Pos2::new(
            rect.left() + self.pan.x + x as f32 * self.zoom,
            rect.bottom() - self.pan.y - y as f32 * self.zoom,
        )
    }

    /// Convert a screen position inside `rect` to world (drawing) coordinates.
    pub fn screen_to_world(&self, rect: Rect, pos: Pos2) -> (f64, f64) {
        let x = (pos.x - rect.left() - self.pan.x) as f64 / self.zoom as f64;
        let y = (rect.bottom() - pos.y - self.pan.y) as f64 / self.zoom as f64;
        (x, y)
    }

    /// Apply drag-to-pan and scroll-to-zoom from an egui widget response.
    pub fn handle_pan_zoom(&mut self, resp: &Response, ui: &Ui) {
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.05, 8.0);
            }
        }
    }

    /// Fit all project nodes in `rect` with a 10% margin.
    pub fn zoom_to_fit(&mut self, rect: Rect, project: &Project) {
        if project.nodes.is_empty() {
            return;
        }
        let (min_x, min_y, max_x, max_y) = node_bounds(project);
        self.fit_bounds(rect, min_x, min_y, max_x, max_y);
    }

    /// Zoom to the selected node or pipe; fits the whole network when nothing is selected.
    pub fn zoom_to_selection(
        &mut self,
        rect: Rect,
        project: &Project,
        node_idx: Option<usize>,
        pipe_idx: Option<usize>,
    ) {
        if let Some(idx) = pipe_idx {
            if idx < project.pipes.len() {
                let pipe = &project.pipes[idx];
                if let (Some(from), Some(to)) = (
                    project.nodes.iter().find(|n| n.id == pipe.from),
                    project.nodes.iter().find(|n| n.id == pipe.to),
                ) {
                    let pad = 50.0;
                    self.fit_bounds(
                        rect,
                        from.x.min(to.x) - pad,
                        from.y.min(to.y) - pad,
                        from.x.max(to.x) + pad,
                        from.y.max(to.y) + pad,
                    );
                    return;
                }
            }
        }
        if let Some(idx) = node_idx {
            if idx < project.nodes.len() {
                let n = &project.nodes[idx];
                let pad = 75.0;
                self.fit_bounds(rect, n.x - pad, n.y - pad, n.x + pad, n.y + pad);
                return;
            }
        }
        self.zoom_to_fit(rect, project);
    }

    fn fit_bounds(&mut self, rect: Rect, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        let margin = rect.size().min_elem() * 0.05;
        let inner = rect.shrink(margin);
        let world_w = (max_x - min_x).max(1.0);
        let world_h = (max_y - min_y).max(1.0);

        let zoom_x = inner.width() / world_w as f32;
        let zoom_y = inner.height() / world_h as f32;
        self.zoom = zoom_x.min(zoom_y).clamp(0.05, 8.0);

        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        self.pan.x = rect.center().x - rect.left() - cx as f32 * self.zoom;
        self.pan.y = rect.bottom() - rect.center().y - cy as f32 * self.zoom;
    }
}

fn node_bounds(project: &Project) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for n in &project.nodes {
        min_x = min_x.min(n.x);
        min_y = min_y.min(n.y);
        max_x = max_x.max(n.x);
        max_y = max_y.max(n.y);
    }

    (min_x, min_y, max_x, max_y)
}