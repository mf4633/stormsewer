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
            Tool::PlaceInlet => "Click to place an inlet — or click a pipe to insert one on the run",
            Tool::PlaceJunction => "Click to place a junction — or click a pipe to insert one on the run",
            Tool::PlaceOutfall => "Click to place an outfall — or click a pipe to insert one on the run",
            Tool::DrawPipe => "Click to drop manholes and link them into a run; click a node to tie in; Esc or right-click to finish",
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

/// What the plan-view context menu is acting on (set on right-click).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContextTarget {
    Node(usize),
    /// A pipe plus the world point that was right-clicked (for "insert here").
    Pipe { idx: usize, x: f64, y: f64 },
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
    /// When set, manholes dropped while drawing a run start with zero drainage
    /// area — lay out the skeleton first, assign loads later. Synced from prefs.
    pub zero_area_nodes: bool,
    /// Node or pipe the plan-view context menu currently targets.
    pub context_target: Option<ContextTarget>,
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
    let (proj_x, proj_y) = project_point_on_segment(px, py, x1, y1, x2, y2);
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

/// Closest point on segment `(x1,y1)-(x2,y2)` to `(px,py)` (clamped to the ends).
fn project_point_on_segment(
    px: f64,
    py: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
) -> (f64, f64) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        return (x1, y1);
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    (x1 + t * dx, y1 + t * dy)
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

/// Insert a new structure of `kind` onto the pipe at `pipe_idx`, splitting it in
/// two. The new node is placed at the point on the pipe closest to `(x, y)`; the
/// original pipe becomes `from → new` and a clone (same diameter, roughness, and
/// section) carries `new → to`. Returns `(new_id, from_id, to_id)`.
pub fn split_pipe(
    project: &mut Project,
    edit: &mut EditState,
    pipe_idx: usize,
    kind: &str,
    x: f64,
    y: f64,
) -> Option<(String, String, String)> {
    let (from_id, to_id) = {
        let p = project.pipes.get(pipe_idx)?;
        (p.from.clone(), p.to.clone())
    };
    let a = project.nodes.iter().find(|n| n.id == from_id)?;
    let b = project.nodes.iter().find(|n| n.id == to_id)?;
    let (px, py) = project_point_on_segment(x, y, a.x, a.y, b.x, b.y);

    let id = place_structure(project, edit, kind, px, py);

    // Second half inherits every hydraulic property of the original pipe.
    let mut second = project.pipes[pipe_idx].clone();
    second.id = format!("P{}", edit.next_pipe_id);
    edit.next_pipe_id += 1;
    second.from = id.clone();
    second.to = to_id.clone();
    project.pipes[pipe_idx].to = id.clone();
    project.pipes.push(second);

    sync_pipe_lengths(project);
    Some((id, from_id, to_id))
}

/// Swap a pipe's endpoints so it runs the other way — fixes a run drawn in the
/// wrong direction (flow always goes `from → to`). Length is unchanged.
pub fn reverse_pipe(project: &mut Project, pipe_idx: usize) -> Option<String> {
    let p = project.pipes.get_mut(pipe_idx)?;
    std::mem::swap(&mut p.from, &mut p.to);
    Some(format!("Reversed {} — now {} → {}", p.id, p.from, p.to))
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

/// Find the closest node to `(x, y)` within `radius`, skipping `exclude`.
/// Used while dragging to spot the node a dragged structure would merge into.
pub fn nearest_other_node(
    project: &Project,
    x: f64,
    y: f64,
    radius: f64,
    exclude: usize,
) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, node) in project.nodes.iter().enumerate() {
        if i == exclude {
            continue;
        }
        let dist = ((node.x - x).powi(2) + (node.y - y).powi(2)).sqrt();
        if dist <= radius && best.map_or(true, |(_, d)| dist < d) {
            best = Some((i, dist));
        }
    }
    best.map(|(i, _)| i)
}

/// Merge the node at `from_idx` into the node with id `to_id`: repoint every
/// pipe endpoint and catchment inlet from the dragged node to the target, drop
/// the self-loops and duplicate pipes that creates, then remove the dragged
/// node. Returns a status message, or `None` if the merge is a no-op.
pub fn merge_node(project: &mut Project, from_idx: usize, to_id: &str) -> Option<String> {
    if from_idx >= project.nodes.len() {
        return None;
    }
    let from_id = project.nodes[from_idx].id.clone();
    if from_id == to_id {
        return None;
    }

    for pipe in &mut project.pipes {
        if pipe.from == from_id {
            pipe.from = to_id.to_string();
        }
        if pipe.to == from_id {
            pipe.to = to_id.to_string();
        }
    }
    // Drop self-loops, then collapse duplicate parallel pipes (keep the first).
    project.pipes.retain(|p| p.from != p.to);
    let mut seen = std::collections::HashSet::new();
    project
        .pipes
        .retain(|p| seen.insert((p.from.clone(), p.to.clone())));

    for c in &mut project.catchments {
        if c.inlet_node_id.as_deref() == Some(from_id.as_str()) {
            c.inlet_node_id = Some(to_id.to_string());
        }
    }

    project.nodes.remove(from_idx);
    sync_pipe_lengths(project);
    Some(format!("Merged {from_id} into {to_id}"))
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

/// Handle a plan click for the Place tools: dropping a structure on a pipe
/// splits that pipe; otherwise the structure is placed free-standing.
fn place_node_click(
    project: &mut Project,
    edit: &mut EditState,
    kind: &str,
    x: f64,
    y: f64,
) -> EditResult {
    if snap_node(project, x, y, SNAP_RADIUS).is_none() {
        if let Some(pi) = snap_pipe(project, x, y, SNAP_RADIUS) {
            if let Some((id, from_id, to_id)) = split_pipe(project, edit, pi, kind, x, y) {
                let sel = project.nodes.iter().position(|n| n.id == id);
                return EditResult {
                    status: Some(format!(
                        "Inserted {kind} {id} on the run: {from_id} → {id} → {to_id}"
                    )),
                    needs_analysis: true,
                    selected_node: sel,
                    ..Default::default()
                };
            }
        }
    }
    let id = place_structure(project, edit, kind, x, y);
    EditResult {
        status: Some(format!("Placed {kind} {id}")),
        needs_analysis: true,
        selected_node: Some(project.nodes.len() - 1),
        ..Default::default()
    }
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
        Tool::PlaceInlet => place_node_click(project, edit, "inlet", world_x, world_y),
        Tool::PlaceJunction => place_node_click(project, edit, "junction", world_x, world_y),
        Tool::PlaceOutfall => place_node_click(project, edit, "outfall", world_x, world_y),
        Tool::DrawPipe => {
            // Get the node under the cursor, or drop a fresh junction there. This
            // lets the user sketch a run over empty ground without pre-placing
            // structures — each click extends the run to a new manhole.
            let (node_idx, node_id, created) =
                match snap_node(project, world_x, world_y, SNAP_RADIUS) {
                    Some(idx) => (idx, project.nodes[idx].id.clone(), false),
                    None => {
                        let id = place_structure(project, edit, "junction", world_x, world_y);
                        if edit.zero_area_nodes {
                            if let Some(n) = project.nodes.last_mut() {
                                n.area_ac = 0.0;
                            }
                        }
                        (project.nodes.len() - 1, id, true)
                    }
                };
            // Selecting the run's current head opens the inspector on it, so its
            // area / type / invert are one glance away right after drawing.
            let select = Some(node_idx);

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
                        selected_node: select,
                        ..Default::default()
                    }
                }
                Some(from_id) => {
                    if from_id == node_id {
                        // Clicked the run's own head again — nothing to connect.
                        edit.pipe_from = Some(node_id);
                        return EditResult {
                            status: Some("Click a different point to extend the run".into()),
                            selected_node: select,
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
                                selected_node: select,
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
                                selected_node: select,
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

    #[test]
    fn draw_pipe_selects_the_head_node() {
        // A freshly drawn manhole is selected so the inspector opens on it.
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        edit.tool = Tool::DrawPipe;

        let r = handle_click(&mut project, &mut edit, 100.0, 100.0, 0.0);
        assert_eq!(r.selected_node, Some(project.nodes.len() - 1));
    }

    #[test]
    fn draw_pipe_skeleton_gives_zero_area_nodes() {
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        edit.tool = Tool::DrawPipe;
        edit.zero_area_nodes = true;

        handle_click(&mut project, &mut edit, 100.0, 100.0, 0.0);
        let n = project.nodes.last().unwrap();
        assert_eq!(n.kind, "junction");
        assert!(n.area_ac.abs() < 1e-9, "skeleton node should carry no area");
    }

    #[test]
    fn nearest_other_node_excludes_self() {
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        let a = place_structure(&mut project, &mut edit, "junction", 100.0, 100.0);
        let b = place_structure(&mut project, &mut edit, "junction", 105.0, 100.0);
        let ia = project.nodes.iter().position(|n| n.id == a).unwrap();
        let ib = project.nodes.iter().position(|n| n.id == b).unwrap();

        assert_eq!(nearest_other_node(&project, 100.0, 100.0, 15.0, ia), Some(ib));
        assert_eq!(nearest_other_node(&project, 100.0, 100.0, 15.0, ib), Some(ia));
        assert_eq!(nearest_other_node(&project, 500.0, 500.0, 15.0, ia), None);
    }

    #[test]
    fn merge_node_repoints_pipes_and_removes_node() {
        // A → B → C; merge B into C. Expect A → C, B gone, self-loop dropped.
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        let a = place_structure(&mut project, &mut edit, "junction", 0.0, 0.0);
        let b = place_structure(&mut project, &mut edit, "junction", 100.0, 0.0);
        let c = place_structure(&mut project, &mut edit, "junction", 200.0, 0.0);
        place_pipe(&mut project, &mut edit, &a, &b).unwrap();
        place_pipe(&mut project, &mut edit, &b, &c).unwrap();

        let b_idx = project.nodes.iter().position(|n| n.id == b).unwrap();
        let msg = merge_node(&mut project, b_idx, &c).unwrap();
        assert!(msg.contains(&b) && msg.contains(&c));
        assert!(!project.nodes.iter().any(|n| n.id == b));
        assert_eq!(project.pipes.len(), 1);
        assert_eq!(project.pipes[0].from, a);
        assert_eq!(project.pipes[0].to, c);
    }

    #[test]
    fn merge_node_collapses_parallel_pipes() {
        // A → C and B → C; merge B into A. B → C becomes a duplicate A → C.
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        let a = place_structure(&mut project, &mut edit, "junction", 0.0, 0.0);
        let b = place_structure(&mut project, &mut edit, "junction", 50.0, 0.0);
        let c = place_structure(&mut project, &mut edit, "junction", 100.0, 0.0);
        place_pipe(&mut project, &mut edit, &a, &c).unwrap();
        place_pipe(&mut project, &mut edit, &b, &c).unwrap();

        let b_idx = project.nodes.iter().position(|n| n.id == b).unwrap();
        merge_node(&mut project, b_idx, &a).unwrap();
        assert_eq!(project.pipes.len(), 1);
        assert_eq!(project.pipes[0].from, a);
        assert_eq!(project.pipes[0].to, c);
    }

    #[test]
    fn split_pipe_inserts_node_and_inherits_properties() {
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        let a = place_structure(&mut project, &mut edit, "junction", 0.0, 100.0);
        let b = place_structure(&mut project, &mut edit, "junction", 100.0, 100.0);
        let pid = place_pipe(&mut project, &mut edit, &a, &b).unwrap();
        let pi = project.pipes.iter().position(|p| p.id == pid).unwrap();
        project.pipes[pi].diameter = 2.5; // distinctive value to check inheritance

        let (id, from_id, to_id) =
            split_pipe(&mut project, &mut edit, pi, "junction", 50.0, 105.0).unwrap();
        assert_eq!(from_id, a);
        assert_eq!(to_id, b);
        assert_eq!(project.pipes.len(), 2);

        let first = project.pipes.iter().find(|p| p.from == a && p.to == id).unwrap();
        let second = project.pipes.iter().find(|p| p.from == id && p.to == b).unwrap();
        assert!((first.diameter - 2.5).abs() < 1e-9);
        assert!((second.diameter - 2.5).abs() < 1e-9);

        // New node sits on the segment (projected onto y = 100 at x ≈ 50).
        let n = project.nodes.iter().find(|nn| nn.id == id).unwrap();
        assert!((n.y - 100.0).abs() < 1e-9);
        assert!((n.x - 50.0).abs() < 1e-6);
        // The two halves span the original 100 ft length.
        assert!((first.length + second.length - 100.0).abs() < 1e-6);
    }

    #[test]
    fn place_tool_on_a_pipe_splits_it() {
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        let a = place_structure(&mut project, &mut edit, "junction", 0.0, 100.0);
        let b = place_structure(&mut project, &mut edit, "junction", 100.0, 100.0);
        place_pipe(&mut project, &mut edit, &a, &b).unwrap();

        edit.tool = Tool::PlaceJunction;
        let before = project.nodes.len();
        let r = handle_click(&mut project, &mut edit, 50.0, 102.0, 0.0); // on the pipe
        assert!(r.status.as_deref().unwrap().contains("Inserted"));
        assert!(r.selected_node.is_some());
        assert_eq!(project.nodes.len(), before + 1);
        assert_eq!(project.pipes.len(), 2);
    }

    #[test]
    fn place_tool_off_a_pipe_places_free_standing() {
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.tool = Tool::PlaceInlet;
        let r = handle_click(&mut project, &mut edit, 500.0, 500.0, 0.0); // empty ground
        assert!(r.status.as_deref().unwrap().contains("Placed inlet"));
        assert_eq!(project.pipes.len(), 0);
    }

    #[test]
    fn reverse_pipe_swaps_endpoints() {
        let mut project = Project::empty();
        let mut edit = EditState::default();
        edit.next_node_id = 1;
        edit.next_pipe_id = 1;
        let a = place_structure(&mut project, &mut edit, "junction", 0.0, 0.0);
        let b = place_structure(&mut project, &mut edit, "junction", 100.0, 0.0);
        place_pipe(&mut project, &mut edit, &a, &b).unwrap();
        let len_before = project.pipes[0].length;

        reverse_pipe(&mut project, 0).unwrap();
        assert_eq!(project.pipes[0].from, b);
        assert_eq!(project.pipes[0].to, a);
        assert!((project.pipes[0].length - len_before).abs() < 1e-9);
    }
}