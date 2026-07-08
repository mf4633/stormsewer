// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive network editing tools (place structures, draw pipes).

use stormsewer::io::project::{Project, ProjectNode, ProjectPipe};

/// Active editing tool in the plan view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    PlaceInlet,
    PlaceJunction,
    PlaceOutfall,
    DrawPipe,
    DrawCatchment,
}

impl Tool {
    /// Human-readable name for status bar and plan-view overlay.
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::PlaceInlet => "Place Inlet",
            Tool::PlaceJunction => "Place Junction",
            Tool::PlaceOutfall => "Place Outfall",
            Tool::DrawPipe => "Draw Pipe",
            Tool::DrawCatchment => "Draw Catchment",
        }
    }

    /// Keyboard shortcut shown on toolbar buttons.
    pub fn shortcut(self) -> &'static str {
        match self {
            Tool::Select => "1",
            Tool::PlaceInlet => "2",
            Tool::PlaceJunction => "3",
            Tool::PlaceOutfall => "4",
            Tool::DrawPipe => "5",
            Tool::DrawCatchment => "6",
        }
    }

    /// Contextual hint for the status bar.
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click a structure, pipe, or catchment",
            Tool::PlaceInlet => "Click the plan to place an inlet",
            Tool::PlaceJunction => "Click the plan to place a junction",
            Tool::PlaceOutfall => "Click the plan to place an outfall",
            Tool::DrawPipe => "Click to drop manholes and link them into a run; click a node to tie in; Esc to finish",
            Tool::DrawCatchment => "Click vertices; click first point to close (Esc to cancel)",
        }
    }

    /// Short label for the compact toolbar tool palette.
    pub fn short(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::PlaceInlet => "Inlet",
            Tool::PlaceJunction => "Junction",
            Tool::PlaceOutfall => "Outfall",
            Tool::DrawPipe => "Pipe",
            Tool::DrawCatchment => "Catchment",
        }
    }

    /// All tools in palette order.
    pub fn all() -> [Tool; 6] {
        [
            Tool::Select,
            Tool::PlaceInlet,
            Tool::PlaceJunction,
            Tool::PlaceOutfall,
            Tool::DrawPipe,
            Tool::DrawCatchment,
        ]
    }
}

/// Mutable editing session state (tool selection, pipe-in-progress, ID counters).
#[derive(Clone, Debug, Default)]
pub struct EditState {
    pub tool: Tool,
    pub pipe_from: Option<String>,
    pub catchment_vertices: Vec<(f64, f64)>,
    pub next_node_id: u32,
    pub next_pipe_id: u32,
    pub next_catchment_id: u32,
    pub selected_node: Option<usize>,
    pub selected_pipe: Option<usize>,
    pub selected_catchment: Option<usize>,
}

/// Outcome of a plan-view click.
#[derive(Clone, Debug, Default)]
pub struct EditResult {
    pub status: Option<String>,
    pub needs_analysis: bool,
    pub selected_node: Option<usize>,
    pub selected_pipe: Option<usize>,
    pub selected_catchment: Option<usize>,
}

/// Default hydraulic / catchment values for newly placed structures.
const DEFAULT_INVERT: f64 = 100.0;
const DEFAULT_RIM: f64 = 106.0;
const DEFAULT_AREA_AC: f64 = 1.0;
const DEFAULT_C: f64 = 0.7;
const DEFAULT_TC_INLET: f64 = 10.0;

/// Default pipe hydraulic values.
const DEFAULT_DIAMETER: f64 = 1.5;
const DEFAULT_N: f64 = 0.013;

/// Snap radius in world (drawing) units.
const SNAP_RADIUS: f64 = 15.0;

/// Snap a coordinate to the nearest grid increment (0 = no snap).
pub fn snap_grid(value: f64, spacing: f64) -> f64 {
    if spacing <= 0.0 {
        return value;
    }
    (value / spacing).round() * spacing
}

/// Snap placement coordinates to a drawing grid.
pub fn snap_placement(x: f64, y: f64, grid_ft: f64) -> (f64, f64) {
    (snap_grid(x, grid_ft), snap_grid(y, grid_ft))
}

/// Find the closest node within `radius` of `(x, y)`, if any.
pub fn snap_node(project: &Project, x: f64, y: f64, radius: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, node) in project.nodes.iter().enumerate() {
        let dx = node.x - x;
        let dy = node.y - y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist <= radius {
            if best.map_or(true, |(_, d)| dist < d) {
                best = Some((i, dist));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Catchment whose polygon contains `(x, y)`, if any (topmost wins).
pub fn snap_catchment(project: &Project, x: f64, y: f64) -> Option<usize> {
    for (i, catchment) in project.catchments.iter().enumerate().rev() {
        if catchment.vertices.len() >= 3
            && stormsewer::catchment::point_in_polygon(x, y, &catchment.vertices)
        {
            return Some(i);
        }
    }
    None
}

/// Find the closest pipe segment within `radius` of `(x, y)`, if any.
pub fn snap_pipe(project: &Project, x: f64, y: f64, radius: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, pipe) in project.pipes.iter().enumerate() {
        let from = project.nodes.iter().find(|n| n.id == pipe.from)?;
        let to = project.nodes.iter().find(|n| n.id == pipe.to)?;
        let dist = point_to_segment_dist(x, y, from.x, from.y, to.x, to.y);
        if dist <= radius {
            if best.map_or(true, |(_, d)| dist < d) {
                best = Some((i, dist));
            }
        }
    }
    best.map(|(i, _)| i)
}

fn point_to_segment_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

/// Place a new structure at `(x, y)` and return its generated id.
pub fn place_structure(
    project: &mut Project,
    edit: &mut EditState,
    kind: &str,
    x: f64,
    y: f64,
) -> String {
    let id = format!("N{}", edit.next_node_id);
    edit.next_node_id += 1;
    project.nodes.push(ProjectNode {
        id: id.clone(),
        kind: kind.into(),
        x,
        y,
        invert: DEFAULT_INVERT,
        rim: DEFAULT_RIM,
        area_ac: DEFAULT_AREA_AC,
        c: DEFAULT_C,
        tc_inlet: DEFAULT_TC_INLET,
        inlet: Default::default(),
    });
    id
}

/// Connect two nodes with a new pipe. Length is computed from coordinates.
pub fn place_pipe(
    project: &mut Project,
    edit: &mut EditState,
    from_id: &str,
    to_id: &str,
) -> Result<String, String> {
    if from_id == to_id {
        return Err("Cannot connect a node to itself".into());
    }

    let from = project
        .nodes
        .iter()
        .find(|n| n.id == from_id)
        .ok_or_else(|| format!("Unknown node: {from_id}"))?;
    let to = project
        .nodes
        .iter()
        .find(|n| n.id == to_id)
        .ok_or_else(|| format!("Unknown node: {to_id}"))?;

    if project
        .pipes
        .iter()
        .any(|p| p.from == from_id && p.to == to_id)
    {
        return Err(format!("Pipe from {from_id} to {to_id} already exists"));
    }

    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length = (dx * dx + dy * dy).sqrt().max(1.0);

    let id = format!("P{}", edit.next_pipe_id);
    edit.next_pipe_id += 1;

    project.pipes.push(ProjectPipe::new(
        &id,
        from_id,
        to_id,
        length,
        DEFAULT_DIAMETER,
        DEFAULT_N,
    ));

    Ok(id)
}

/// Recompute each pipe length from its endpoint node coordinates.
pub fn sync_pipe_lengths(project: &mut Project) {
    for pipe in &mut project.pipes {
        let (from, to) = (
            project.nodes.iter().find(|n| n.id == pipe.from),
            project.nodes.iter().find(|n| n.id == pipe.to),
        );
        if let (Some(from), Some(to)) = (from, to) {
            let dx = to.x - from.x;
            let dy = to.y - from.y;
            pipe.length = (dx * dx + dy * dy).sqrt();
        }
    }
}

/// Move a node by `(dx, dy)` in world coordinates and update connected pipe lengths.
pub fn move_node(project: &mut Project, idx: usize, dx: f64, dy: f64) {
    if idx >= project.nodes.len() {
        return;
    }
    project.nodes[idx].x += dx;
    project.nodes[idx].y += dy;
    sync_pipe_lengths(project);
}

/// Delete the currently selected node or pipe from the project.
pub fn delete_selection(
    project: &mut Project,
    selected_node: Option<usize>,
    selected_pipe: Option<usize>,
) -> Option<String> {
    if let Some(idx) = selected_pipe {
        if idx < project.pipes.len() {
            let id = project.pipes[idx].id.clone();
            project.pipes.remove(idx);
            return Some(format!("Deleted pipe {id}"));
        }
    }
    if let Some(idx) = selected_node {
        if idx < project.nodes.len() {
            let id = project.nodes[idx].id.clone();
            project.pipes.retain(|p| p.from != id && p.to != id);
            project.nodes.remove(idx);
            return Some(format!("Deleted node {id}"));
        }
    }
    None
}

/// Handle a plan-view click at world coordinates.
pub fn handle_click(
    project: &mut Project,
    edit: &mut EditState,
    world_x: f64,
    world_y: f64,
    grid_ft: f64,
) -> EditResult {
    let (world_x, world_y) = snap_placement(world_x, world_y, grid_ft);
    match edit.tool {
        Tool::Select => {
            if let Some(idx) = snap_node(project, world_x, world_y, SNAP_RADIUS) {
                EditResult {
                    status: Some(format!("Selected {}", project.nodes[idx].id)),
                    selected_node: Some(idx),
                    selected_pipe: None,
                    selected_catchment: None,
                    ..Default::default()
                }
            } else if let Some(idx) = snap_pipe(project, world_x, world_y, SNAP_RADIUS) {
                EditResult {
                    status: Some(format!("Selected {}", project.pipes[idx].id)),
                    selected_node: None,
                    selected_pipe: Some(idx),
                    selected_catchment: None,
                    ..Default::default()
                }
            } else if let Some(idx) = snap_catchment(project, world_x, world_y) {
                let id = project.catchments[idx].id.clone();
                EditResult {
                    status: Some(format!("Selected catchment {id}")),
                    selected_node: None,
                    selected_pipe: None,
                    selected_catchment: Some(idx),
                    ..Default::default()
                }
            } else {
                EditResult {
                    status: Some("Selection cleared".into()),
                    selected_node: None,
                    selected_pipe: None,
                    selected_catchment: None,
                    ..Default::default()
                }
            }
        }
        Tool::PlaceInlet => {
            let id = place_structure(project, edit, "inlet", world_x, world_y);
            EditResult {
                status: Some(format!("Placed inlet {id}")),
                needs_analysis: true,
                ..Default::default()
            }
        }
        Tool::PlaceJunction => {
            let id = place_structure(project, edit, "junction", world_x, world_y);
            EditResult {
                status: Some(format!("Placed junction {id}")),
                needs_analysis: true,
                ..Default::default()
            }
        }
        Tool::PlaceOutfall => {
            let id = place_structure(project, edit, "outfall", world_x, world_y);
            EditResult {
                status: Some(format!("Placed outfall {id}")),
                needs_analysis: true,
                ..Default::default()
            }
        }
        Tool::DrawPipe => {
            // Get the node under the cursor, or drop a fresh junction there. This
            // lets the user sketch a run over empty ground without pre-placing
            // structures — each click extends the run to a new manhole.
            let (node_id, created) = match snap_node(project, world_x, world_y, SNAP_RADIUS) {
                Some(idx) => (project.nodes[idx].id.clone(), false),
                None => (
                    place_structure(project, edit, "junction", world_x, world_y),
                    true,
                ),
            };

            match edit.pipe_from.take() {
                None => {
                    // First point of a new run.
                    edit.pipe_from = Some(node_id.clone());
                    EditResult {
                        status: Some(if created {
                            format!("Run started at new junction {node_id} — click the next point")
                        } else {
                            format!("Run started at {node_id} — click the next point")
                        }),
                        needs_analysis: created,
                        ..Default::default()
                    }
                }
                Some(from_id) => {
                    if from_id == node_id {
                        // Clicked the run's own head again — nothing to connect.
                        edit.pipe_from = Some(node_id);
                        return EditResult {
                            status: Some("Click a different point to extend the run".into()),
                            ..Default::default()
                        };
                    }
                    match place_pipe(project, edit, &from_id, &node_id) {
                        Ok(pipe_id) => {
                            // Chain: the reached node becomes the next start, so
                            // repeated clicks lay a continuous run. Esc ends it.
                            edit.pipe_from = Some(node_id.clone());
                            EditResult {
                                status: Some(format!(
                                    "Pipe {pipe_id}: {from_id} → {node_id} — click to extend, Esc to finish"
                                )),
                                needs_analysis: true,
                                ..Default::default()
                            }
                        }
                        Err(e) => {
                            // Keep the run anchored at the reached node so the user
                            // can pick a different next point.
                            edit.pipe_from = Some(node_id);
                            EditResult {
                                status: Some(e),
                                needs_analysis: created,
                                ..Default::default()
                            }
                        }
                    }
                }
            }
        }
        Tool::DrawCatchment => EditResult::default(),
    }
}

#[cfg(test)]
mod headless_tests {
    use super::*;
    use stormsewer::io::Project;

    #[test]
    fn snap_placement_aligns_to_grid() {
        let (x, y) = snap_placement(23.0, 37.0, 10.0);
        assert!((x - 20.0).abs() < 1e-9);
        assert!((y - 40.0).abs() < 1e-9);
    }

    #[test]
    fn snap_catchment_finds_polygon_under_point() {
        let mut project = Project::empty();
        project.catchments.push(stormsewer::io::project::ProjectCatchment {
            id: "C1".into(),
            vertices: vec![(0.0, 0.0), (200.0, 0.0), (200.0, 200.0)],
            c: 0.7,
            flow_length_ft: 100.0,
            slope: 0.01,
            inlet_node_id: None,
        });
        assert_eq!(snap_catchment(&project, 50.0, 50.0), Some(0));
        assert_eq!(snap_catchment(&project, 500.0, 500.0), None);
    }

    #[test]
    fn handle_click_selects_pipe_and_node() {
        let mut project = Project::demo();
        let mut edit = EditState::default();
        edit.tool = Tool::Select;

        let (n0x, n0y) = (project.nodes[0].x, project.nodes[0].y);
        let n1x = project.nodes[1].x;

        let on_n1 = handle_click(&mut project, &mut edit, n0x, n0y, 0.0);
        assert_eq!(on_n1.selected_node, Some(0));

        let mid_x = (n0x + n1x) / 2.0;
        let on_p1 = handle_click(&mut project, &mut edit, mid_x, n0y, 0.0);
        assert_eq!(on_p1.selected_pipe, Some(0));
    }

    #[test]
    fn draw_pipe_places_connection() {
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        edit.tool = Tool::PlaceInlet;
        let inlet_id = place_structure(&mut project, &mut edit, "inlet", 100.0, 0.0);
        assert_eq!(inlet_id, "N1");
        edit.tool = Tool::DrawPipe;

        let start = handle_click(&mut project, &mut edit, 100.0, 0.0, 0.0);
        assert!(start.status.as_deref().unwrap().contains("Run started"));

        let finish = handle_click(&mut project, &mut edit, 0.0, 0.0, 0.0);
        assert!(finish.needs_analysis);
        assert_eq!(project.pipes.len(), 1);
        assert_eq!(project.pipes[0].from, "N1");
        assert_eq!(project.pipes[0].to, "OUT");
        // The run chains: the reached node stays armed as the next start.
        assert_eq!(edit.pipe_from.as_deref(), Some("OUT"));
    }

    #[test]
    fn draw_pipe_sketches_a_run_on_empty_canvas() {
        // Starting from a blank project, clicking bare ground should drop
        // manholes and connect them into a continuous run — no pre-placing.
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        edit.tool = Tool::DrawPipe;

        // Three clicks on empty ground away from the origin outfall.
        let p1 = handle_click(&mut project, &mut edit, 100.0, 100.0, 0.0);
        assert!(p1.needs_analysis); // a node was created
        assert_eq!(project.nodes.len(), 2); // OUT + first junction
        assert_eq!(project.pipes.len(), 0);
        assert!(edit.pipe_from.is_some());

        handle_click(&mut project, &mut edit, 200.0, 100.0, 0.0);
        assert_eq!(project.nodes.len(), 3);
        assert_eq!(project.pipes.len(), 1);

        handle_click(&mut project, &mut edit, 300.0, 100.0, 0.0);
        assert_eq!(project.nodes.len(), 4);
        assert_eq!(project.pipes.len(), 2);

        // Every auto-created node is a junction; the run is still armed.
        assert!(project
            .nodes
            .iter()
            .filter(|n| n.id != "OUT")
            .all(|n| n.kind == "junction"));
        assert!(edit.pipe_from.is_some());

        // Pipe length is computed from the coordinates (100 ft spans).
        assert!((project.pipes[0].length - 100.0).abs() < 1e-6);
    }

    #[test]
    fn draw_pipe_ties_into_existing_node() {
        // A run drawn on empty ground can be closed onto an existing structure.
        let mut project = Project::empty(); // OUT at (0,0)
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        edit.tool = Tool::DrawPipe;

        handle_click(&mut project, &mut edit, 0.0, 200.0, 0.0); // new junction N1
        handle_click(&mut project, &mut edit, 0.0, 0.0, 0.0); // snaps onto OUT
        assert_eq!(project.pipes.len(), 1);
        assert_eq!(project.pipes[0].to, "OUT");
        assert_eq!(project.nodes.len(), 2); // no duplicate node at the outfall
    }
}