// SPDX-License-Identifier: GPL-3.0-or-later

//! The SWMM map editor's tools and in-progress gesture state, plus the
//! geometry the canvas needs: hit testing, snapping, polygon centroids.
//!
//! Conventions are the EPA SWMM GUI's own — a Select arrow, a Pan hand,
//! zoom in/out/extents/window, and one Add tool per object type — with
//! the keyboard shortcuts CAD users expect (Esc cancels, Enter closes a
//! polygon, Delete removes).

use eframe::egui::{Pos2, Rect};
use stormsewer_swmm::doc::build::{LinkType, NodeType, ObjRef};

use crate::viewport::Viewport;

/// Active tool on the SWMM map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwmmTool {
    #[default]
    Select,
    Pan,
    ZoomIn,
    ZoomOut,
    ZoomWindow,
    AddGage,
    AddNode(NodeType),
    AddLink(LinkType),
    AddSubcatchment,
    AddLabel,
}

impl SwmmTool {
    /// Toolbar order: navigation, then objects in the order the EPA GUI's
    /// object toolbar lists them.
    pub fn all() -> Vec<SwmmTool> {
        let mut out = vec![
            SwmmTool::Select,
            SwmmTool::Pan,
            SwmmTool::ZoomIn,
            SwmmTool::ZoomOut,
            SwmmTool::ZoomWindow,
            SwmmTool::AddGage,
            SwmmTool::AddSubcatchment,
        ];
        out.extend(NodeType::ALL.iter().map(|k| SwmmTool::AddNode(*k)));
        out.extend(LinkType::ALL.iter().map(|k| SwmmTool::AddLink(*k)));
        out.push(SwmmTool::AddLabel);
        out
    }

    /// Short label for the toolbar.
    pub fn short(self) -> &'static str {
        match self {
            SwmmTool::Select => "Select",
            SwmmTool::Pan => "Pan",
            SwmmTool::ZoomIn => "Zoom +",
            SwmmTool::ZoomOut => "Zoom −",
            SwmmTool::ZoomWindow => "Zoom Win",
            SwmmTool::AddGage => "Gage",
            SwmmTool::AddNode(NodeType::Junction) => "Junction",
            SwmmTool::AddNode(NodeType::Outfall) => "Outfall",
            SwmmTool::AddNode(NodeType::Divider) => "Divider",
            SwmmTool::AddNode(NodeType::Storage) => "Storage",
            SwmmTool::AddLink(LinkType::Conduit) => "Conduit",
            SwmmTool::AddLink(LinkType::Pump) => "Pump",
            SwmmTool::AddLink(LinkType::Orifice) => "Orifice",
            SwmmTool::AddLink(LinkType::Weir) => "Weir",
            SwmmTool::AddLink(LinkType::Outlet) => "Outlet",
            SwmmTool::AddSubcatchment => "Subcatch",
            SwmmTool::AddLabel => "Label",
        }
    }

    /// Full name for the status bar.
    pub fn label(self) -> String {
        match self {
            SwmmTool::Select => "Select".into(),
            SwmmTool::Pan => "Pan".into(),
            SwmmTool::ZoomIn => "Zoom In".into(),
            SwmmTool::ZoomOut => "Zoom Out".into(),
            SwmmTool::ZoomWindow => "Zoom Window".into(),
            SwmmTool::AddGage => "Add Rain Gage".into(),
            SwmmTool::AddNode(k) => format!("Add {}", k.label()),
            SwmmTool::AddLink(k) => format!("Add {}", k.label()),
            SwmmTool::AddSubcatchment => "Add Subcatchment".into(),
            SwmmTool::AddLabel => "Add Label".into(),
        }
    }

    /// Keyboard shortcut, shown on the toolbar.
    pub fn shortcut(self) -> &'static str {
        match self {
            SwmmTool::Select => "S",
            SwmmTool::Pan => "H",
            SwmmTool::ZoomIn => "+",
            SwmmTool::ZoomOut => "-",
            SwmmTool::ZoomWindow => "Z",
            SwmmTool::AddGage => "R",
            SwmmTool::AddNode(NodeType::Junction) => "J",
            SwmmTool::AddNode(NodeType::Outfall) => "O",
            SwmmTool::AddNode(NodeType::Divider) => "D",
            SwmmTool::AddNode(NodeType::Storage) => "T",
            SwmmTool::AddLink(LinkType::Conduit) => "C",
            SwmmTool::AddLink(LinkType::Pump) => "P",
            SwmmTool::AddLink(LinkType::Orifice) => "I",
            SwmmTool::AddLink(LinkType::Weir) => "W",
            SwmmTool::AddLink(LinkType::Outlet) => "U",
            SwmmTool::AddSubcatchment => "A",
            SwmmTool::AddLabel => "L",
        }
    }

    /// What the status bar tells the user to do next.
    pub fn hint(self) -> &'static str {
        match self {
            SwmmTool::Select => {
                "Click selects; Shift-click adds; Ctrl-click toggles; drag moves or rubber-bands"
            }
            SwmmTool::Pan => "Drag to pan (the wheel zooms in every tool)",
            SwmmTool::ZoomIn => "Click to zoom in on a point",
            SwmmTool::ZoomOut => "Click to zoom out from a point",
            SwmmTool::ZoomWindow => "Drag a rectangle to zoom to it",
            SwmmTool::AddGage | SwmmTool::AddNode(_) => "Click the map to place it",
            SwmmTool::AddLink(_) => {
                "Click the start node, click to add vertices, click the end node; Esc cancels"
            }
            SwmmTool::AddSubcatchment => {
                "Click each corner; double-click, Enter, or click the first corner to close; Esc cancels"
            }
            SwmmTool::AddLabel => "Click where the label goes",
        }
    }
}

/// A drag in progress on the map.
#[derive(Clone, Debug, PartialEq)]
pub enum Drag {
    /// Moving the selection; `origins` are the world positions each item
    /// (and each vertex of each selected link and polygon) started at, so
    /// every pointer event sets absolute positions rather than
    /// accumulating deltas.
    Move {
        start: (f64, f64),
        nodes: Vec<(String, f64, f64)>,
        gages: Vec<(String, f64, f64)>,
        links: Vec<(String, Vec<(f64, f64)>)>,
        subs: Vec<(String, Vec<(f64, f64)>)>,
        labels: Vec<(usize, Vec<String>)>,
    },
    /// Dragging one vertex of a link or polygon.
    Vertex {
        target: ObjRef,
        index: usize,
        points: Vec<(f64, f64)>,
    },
    /// Rubber-band selection or zoom window, in screen space.
    Band { start: Pos2, end: Pos2 },
}

/// Everything about the gesture in progress. Cleared by Esc and by
/// switching tools.
#[derive(Clone, Debug, Default)]
pub struct SwmmEditState {
    pub tool: SwmmTool,
    /// Link tool: the start node once chosen, and the vertices clicked
    /// since.
    pub link_from: Option<String>,
    pub link_vertices: Vec<(f64, f64)>,
    /// Subcatchment tool: the corners clicked so far.
    pub polygon: Vec<(f64, f64)>,
    pub drag: Option<Drag>,
    /// Vertex handle chosen on a selected link or polygon (Delete removes
    /// it).
    pub active_vertex: Option<(ObjRef, usize)>,
    /// What the context menu was opened on.
    pub context: Option<ObjRef>,
    pub context_world: Option<(f64, f64)>,
    /// Object under the cursor, for the status bar and hover ring.
    pub hover: Option<ObjRef>,
    pub cursor_world: Option<(f64, f64)>,
    /// A label waiting for its text: position and the text so far.
    pub label_prompt: Option<((f64, f64), String)>,
}

impl SwmmEditState {
    /// Abandon whatever is in progress without changing the document.
    pub fn cancel(&mut self) {
        self.link_from = None;
        self.link_vertices.clear();
        self.polygon.clear();
        self.drag = None;
        self.label_prompt = None;
    }

    pub fn in_progress(&self) -> bool {
        self.link_from.is_some() || !self.polygon.is_empty() || self.label_prompt.is_some()
    }
}

/// How close a click must land, in screen pixels.
pub const HIT_RADIUS: f32 = 10.0;
/// Vertex handles are smaller targets, tested before the object itself.
pub const VERTEX_RADIUS: f32 = 7.0;

pub fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Ray-cast point-in-polygon, in world units.
pub fn point_in_polygon(x: f64, y: f64, poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Area-weighted centroid; the vertex mean for a degenerate polygon.
pub fn centroid(poly: &[(f64, f64)]) -> Option<(f64, f64)> {
    if poly.is_empty() {
        return None;
    }
    let n = poly.len();
    let mut area = 0.0;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..n {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        area += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    if area.abs() < 1e-9 {
        let sx: f64 = poly.iter().map(|p| p.0).sum();
        let sy: f64 = poly.iter().map(|p| p.1).sum();
        return Some((sx / n as f64, sy / n as f64));
    }
    let area = area * 0.5;
    Some((cx / (6.0 * area), cy / (6.0 * area)))
}

/// Length of a polyline in world units.
pub fn polyline_length(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// Snap a world point to the grid (`spacing <= 0` leaves it alone).
pub fn snap_to_grid(x: f64, y: f64, spacing: f64) -> (f64, f64) {
    if spacing <= 0.0 {
        return (x, y);
    }
    (
        (x / spacing).round() * spacing,
        (y / spacing).round() * spacing,
    )
}

/// The world-space rectangle of a screen-space band.
pub fn band_world(vp: &Viewport, rect: Rect, a: Pos2, b: Pos2) -> (f64, f64, f64, f64) {
    let (x0, y0) = vp.screen_to_world(rect, a);
    let (x1, y1) = vp.screen_to_world(rect, b);
    (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

pub fn inside(bounds: (f64, f64, f64, f64), p: (f64, f64)) -> bool {
    p.0 >= bounds.0 && p.0 <= bounds.2 && p.1 >= bounds.1 && p.1 <= bounds.3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_contains_and_centroid() {
        let sq = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        assert!(point_in_polygon(5.0, 5.0, &sq));
        assert!(!point_in_polygon(15.0, 5.0, &sq));
        let (cx, cy) = centroid(&sq).unwrap();
        assert!((cx - 5.0).abs() < 1e-9 && (cy - 5.0).abs() < 1e-9);
        assert_eq!(centroid(&[]), None);
        assert_eq!(centroid(&[(2.0, 4.0)]), Some((2.0, 4.0)));
    }

    #[test]
    fn grid_snap_and_length() {
        assert_eq!(snap_to_grid(23.0, 37.0, 10.0), (20.0, 40.0));
        assert_eq!(snap_to_grid(23.0, 37.0, 0.0), (23.0, 37.0));
        assert!((polyline_length(&[(0.0, 0.0), (3.0, 4.0), (3.0, 0.0)]) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn every_tool_has_a_distinct_shortcut() {
        let tools = SwmmTool::all();
        for (i, a) in tools.iter().enumerate() {
            for b in &tools[i + 1..] {
                assert_ne!(a.shortcut(), b.shortcut(), "{a:?} and {b:?}");
            }
        }
    }
}
