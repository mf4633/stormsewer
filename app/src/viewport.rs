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
            // Screen +Y is down but world +Y (and pan.y) is up, so the
            // vertical component flips: the drawing follows the hand in
            // both axes.
            let d = resp.drag_delta();
            self.pan.x += d.x;
            self.pan.y -= d.y;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                let anchor = resp.hover_pos().unwrap_or_else(|| resp.rect.center());
                self.zoom_at(resp.rect, anchor, 1.0 + scroll * 0.001);
            }
        }
    }

    /// Zoom by `factor`, keeping the world point under `anchor` fixed on
    /// screen — the wheel zooms into what the cursor is pointing at.
    pub fn zoom_at(&mut self, rect: Rect, anchor: Pos2, factor: f32) {
        let (wx, wy) = self.screen_to_world(rect, anchor);
        self.zoom = (self.zoom * factor).clamp(0.05, 8.0);
        self.pan.x = anchor.x - rect.left() - wx as f32 * self.zoom;
        self.pan.y = rect.bottom() - anchor.y - wy as f32 * self.zoom;
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_at_keeps_the_cursor_point_fixed() {
        let rect = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 600.0));
        let mut vp = Viewport::default();
        let anchor = Pos2::new(430.0, 275.0);
        let world_before = vp.screen_to_world(rect, anchor);
        vp.zoom_at(rect, anchor, 1.4);
        let screen_after = vp.world_to_screen(rect, world_before.0, world_before.1);
        assert!(
            (screen_after - anchor).length() < 0.01,
            "anchored point drifted: {screen_after:?} vs {anchor:?}"
        );
        assert!((vp.zoom - 0.6 * 1.4).abs() < 1e-6);
    }

    #[test]
    fn zoom_at_respects_clamps() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0));
        let mut vp = Viewport::default();
        vp.zoom_at(rect, Pos2::new(200.0, 200.0), 1000.0);
        assert!((vp.zoom - 8.0).abs() < 1e-6);
        vp.zoom_at(rect, Pos2::new(200.0, 200.0), 1e-6);
        assert!((vp.zoom - 0.05).abs() < 1e-6);
    }
}
