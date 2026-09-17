// SPDX-License-Identifier: GPL-3.0-or-later

//! The SWMM map: drawn from the document caches, edited with the tools.
//!
//! Symbols follow the EPA SWMM GUI: a junction is a circle, an outfall a
//! triangle, a storage unit a rectangle, a divider a diamond, a rain gage
//! a drop; a pump, orifice, weir, or outlet is marked at the middle of its
//! line. Subcatchments are filled outlines with a dashed line from the
//! centroid to the outlet. Every editing gesture is one undo step.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Rect, Response, RichText, Shape, Stroke, Ui, Vec2};
use stormsewer_swmm::doc::build::{self, LinkType, NodeType, ObjRef};
use stormsewer_swmm::doc::{Command, Severity};

use crate::state::AppState;
use crate::swmm_doc::{describe, SwmmEditor};
use crate::swmm_tools::{
    band_world, dist_to_segment, inside, point_in_polygon, polyline_length, snap_to_grid, Drag,
    SwmmTool, HIT_RADIUS, VERTEX_RADIUS,
};
use crate::theme::palette;
use crate::viewport::Viewport;

/// Subcatchment fill.
const SUB_FILL: Color32 = Color32::from_rgba_premultiplied(60, 140, 60, 40);
const SUB_FILL_SELECTED: Color32 = Color32::from_rgba_premultiplied(224, 86, 127, 60);

// --- hit testing ---------------------------------------------------------------

fn w2s(vp: &Viewport, rect: Rect, p: (f64, f64)) -> Pos2 {
    vp.world_to_screen(rect, p.0, p.1)
}

/// The vertex handle under `pos` on a selected link or subcatchment.
pub fn vertex_hit(
    ed: &SwmmEditor,
    vp: &Viewport,
    rect: Rect,
    pos: Pos2,
) -> Option<(ObjRef, usize)> {
    let mut best: Option<(f32, ObjRef, usize)> = None;
    for r in &ed.selection {
        let pts: Vec<(f64, f64)> = match r {
            ObjRef::Link(n) => ed.link(n)?.vertices.clone(),
            ObjRef::Subcatchment(n) => ed.sub(n)?.polygon.clone(),
            _ => continue,
        };
        for (i, p) in pts.iter().enumerate() {
            let d = (w2s(vp, rect, *p) - pos).length();
            if d <= VERTEX_RADIUS && best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, r.clone(), i));
            }
        }
    }
    best.map(|(_, r, i)| (r, i))
}

/// The object under `pos`: point objects first (nodes, gages, labels),
/// then links by distance to a segment, then the subcatchment containing
/// the point.
pub fn hit_test(ed: &SwmmEditor, vp: &Viewport, rect: Rect, pos: Pos2) -> Option<ObjRef> {
    let mut best: Option<(f32, ObjRef)> = None;
    let mut consider = |d: f32, r: ObjRef| {
        if d <= HIT_RADIUS && best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, r));
        }
    };
    for n in &ed.nodes {
        consider(
            (w2s(vp, rect, (n.x, n.y)) - pos).length(),
            ObjRef::Node(n.name.clone()),
        );
    }
    for g in &ed.gages {
        consider(
            (w2s(vp, rect, (g.x, g.y)) - pos).length(),
            ObjRef::Gage(g.name.clone()),
        );
    }
    for l in &ed.labels {
        consider(
            (w2s(vp, rect, (l.x, l.y)) - pos).length(),
            ObjRef::Label(l.line),
        );
    }
    if let Some((_, r)) = best {
        return Some(r);
    }
    let mut best: Option<(f32, ObjRef)> = None;
    for l in &ed.links {
        for seg in l.path.windows(2) {
            let d = dist_to_segment(pos, w2s(vp, rect, seg[0]), w2s(vp, rect, seg[1]));
            if d <= HIT_RADIUS && best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, ObjRef::Link(l.name.clone())));
            }
        }
    }
    if let Some((_, r)) = best {
        return Some(r);
    }
    let (wx, wy) = vp.screen_to_world(rect, pos);
    ed.subs
        .iter()
        .rev()
        .find(|s| point_in_polygon(wx, wy, &s.polygon))
        .map(|s| ObjRef::Subcatchment(s.name.clone()))
}

/// The segment index (into a link's path or a polygon's edge list) under
/// `pos`, for inserting a vertex.
fn segment_hit(
    pts: &[(f64, f64)],
    closed: bool,
    vp: &Viewport,
    rect: Rect,
    pos: Pos2,
) -> Option<usize> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let count = if closed { n } else { n - 1 };
    (0..count)
        .map(|i| {
            let a = w2s(vp, rect, pts[i]);
            let b = w2s(vp, rect, pts[(i + 1) % n]);
            (i, dist_to_segment(pos, a, b))
        })
        .filter(|(_, d)| *d <= HIT_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Where a click lands: on an object's point when object snap is on and
/// one is within reach, else on the grid when grid snap is on.
fn snap_point(
    ed: &SwmmEditor,
    vp: &Viewport,
    rect: Rect,
    world: (f64, f64),
    objects: bool,
) -> (f64, f64) {
    if objects && ed.snap_objects {
        let pos = w2s(vp, rect, world);
        let mut best: Option<(f32, (f64, f64))> = None;
        let mut consider = |p: (f64, f64)| {
            let d = (w2s(vp, rect, p) - pos).length();
            if d <= HIT_RADIUS && best.is_none_or(|b| d < b.0) {
                best = Some((d, p));
            }
        };
        for n in &ed.nodes {
            consider((n.x, n.y));
        }
        for g in &ed.gages {
            consider((g.x, g.y));
        }
        for l in &ed.links {
            for v in &l.vertices {
                consider(*v);
            }
        }
        for s in &ed.subs {
            for v in &s.polygon {
                consider(*v);
            }
        }
        if let Some((_, p)) = best {
            return p;
        }
    }
    if ed.snap_grid {
        snap_to_grid(world.0, world.1, ed.grid_spacing)
    } else {
        world
    }
}

// --- editing --------------------------------------------------------------------

fn shifted(pts: &[(f64, f64)], dx: f64, dy: f64) -> Vec<(f64, f64)> {
    pts.iter().map(|(x, y)| (x + dx, y + dy)).collect()
}

/// Set every dragged object to its origin plus `(dx, dy)`. One batch per
/// pointer event, all inside the open gesture.
fn apply_move(ed: &mut SwmmEditor, drag: &Drag, dx: f64, dy: f64) {
    let Drag::Move {
        nodes,
        gages,
        links,
        subs,
        labels,
        ..
    } = drag
    else {
        return;
    };
    let mut cmds = Vec::new();
    for (name, x, y) in nodes {
        cmds.push(Command::MoveNode {
            name: name.clone(),
            x: x + dx,
            y: y + dy,
        });
    }
    for (name, x, y) in gages {
        cmds.push(Command::MoveGage {
            name: name.clone(),
            x: x + dx,
            y: y + dy,
        });
    }
    for (name, pts) in links {
        cmds.push(Command::SetVertices {
            link: name.clone(),
            points: shifted(pts, dx, dy),
        });
    }
    for (name, pts) in subs {
        cmds.push(Command::SetPolygon {
            subcatchment: name.clone(),
            points: shifted(pts, dx, dy),
        });
    }
    for (line, fields) in labels {
        let mut fields = fields.clone();
        let shift = |f: &mut String, d: f64| {
            if let Ok(v) = f.parse::<f64>() {
                *f = stormsewer_swmm::doc::format_number(v + d);
            }
        };
        if let Some(f) = fields.get_mut(0) {
            shift(f, dx);
        }
        if let Some(f) = fields.get_mut(1) {
            shift(f, dy);
        }
        cmds.push(Command::SetLine {
            section: "LABELS".into(),
            line: *line,
            fields,
            comment: None,
        });
    }
    ed.apply(Command::Batch(cmds), "move");
}

/// The origins of everything selected, for a move drag.
fn move_origins(ed: &SwmmEditor, start: (f64, f64)) -> Drag {
    let mut nodes = Vec::new();
    let mut gages = Vec::new();
    let mut links = Vec::new();
    let mut subs = Vec::new();
    let mut labels = Vec::new();
    for r in &ed.selection {
        match r {
            ObjRef::Node(n) => {
                if let Some(n) = ed.node(n) {
                    nodes.push((n.name.clone(), n.x, n.y));
                }
            }
            ObjRef::Gage(n) => {
                if let Some(g) = ed.gage(n) {
                    gages.push((g.name.clone(), g.x, g.y));
                }
            }
            ObjRef::Link(n) => {
                if let Some(l) = ed.link(n) {
                    if !l.vertices.is_empty() {
                        links.push((l.name.clone(), l.vertices.clone()));
                    }
                }
            }
            ObjRef::Subcatchment(n) => {
                if let Some(s) = ed.sub(n) {
                    if !s.polygon.is_empty() {
                        subs.push((s.name.clone(), s.polygon.clone()));
                    }
                }
            }
            ObjRef::Label(li) => {
                if let Some(row) = ed
                    .doc
                    .section("LABELS")
                    .and_then(|s| s.lines.get(*li))
                    .and_then(|l| l.row.as_ref())
                {
                    labels.push((*li, row.fields.clone()));
                }
            }
        }
    }
    Drag::Move {
        start,
        nodes,
        gages,
        links,
        subs,
        labels,
    }
}

fn set_points(ed: &mut SwmmEditor, target: &ObjRef, pts: Vec<(f64, f64)>, label: &str) -> bool {
    match target {
        ObjRef::Link(n) => ed.apply(
            Command::SetVertices {
                link: n.clone(),
                points: pts,
            },
            label,
        ),
        ObjRef::Subcatchment(n) => ed.apply(
            Command::SetPolygon {
                subcatchment: n.clone(),
                points: pts,
            },
            label,
        ),
        _ => false,
    }
}

fn points_of(ed: &SwmmEditor, target: &ObjRef) -> Option<Vec<(f64, f64)>> {
    match target {
        ObjRef::Link(n) => ed.link(n).map(|l| l.vertices.clone()),
        ObjRef::Subcatchment(n) => ed.sub(n).map(|s| s.polygon.clone()),
        _ => None,
    }
}

/// Delete the active vertex (Delete key with a vertex chosen). Returns
/// whether a vertex, rather than the selection, was the target.
pub fn delete_active_vertex(ed: &mut SwmmEditor) -> bool {
    let Some((target, idx)) = ed.edit.active_vertex.take() else {
        return false;
    };
    let Some(mut pts) = points_of(ed, &target) else {
        return false;
    };
    if idx >= pts.len() {
        return false;
    }
    if matches!(target, ObjRef::Subcatchment(_)) && pts.len() <= 3 {
        ed.last_error = Some("a subcatchment keeps at least three corners".into());
        return true;
    }
    pts.remove(idx);
    set_points(
        ed,
        &target,
        pts,
        &format!("delete vertex of {}", describe(&target)),
    );
    true
}

/// Insert a vertex where a selected link or polygon was double-clicked.
fn insert_vertex(ed: &mut SwmmEditor, vp: &Viewport, rect: Rect, pos: Pos2) -> bool {
    let world = vp.screen_to_world(rect, pos);
    for r in ed.selection.clone() {
        match &r {
            ObjRef::Link(n) => {
                let Some(l) = ed.link(n) else { continue };
                if let Some(i) = segment_hit(&l.path, false, vp, rect, pos) {
                    // Path index i is the segment from path[i] to path[i+1];
                    // vertices are path[1..len-1], so the new vertex goes at i.
                    let mut pts = l.vertices.clone();
                    pts.insert(i.min(pts.len()), world);
                    let label = format!("add vertex to {}", describe(&r));
                    set_points(ed, &r, pts, &label);
                    ed.edit.active_vertex = Some((r, i));
                    return true;
                }
            }
            ObjRef::Subcatchment(n) => {
                let Some(s) = ed.sub(n) else { continue };
                if let Some(i) = segment_hit(&s.polygon, true, vp, rect, pos) {
                    let mut pts = s.polygon.clone();
                    pts.insert(i + 1, world);
                    let label = format!("add vertex to {}", describe(&r));
                    set_points(ed, &r, pts, &label);
                    ed.edit.active_vertex = Some((r, i + 1));
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Close the polygon in progress into a subcatchment draining to the node
/// nearest its centroid.
pub fn finish_polygon(ed: &mut SwmmEditor) -> Option<String> {
    if ed.edit.polygon.len() < 3 {
        return None;
    }
    let polygon = std::mem::take(&mut ed.edit.polygon);
    let outlet = crate::swmm_tools::centroid(&polygon)
        .and_then(|(cx, cy)| ed.nearest_node(cx, cy, f64::INFINITY))
        .map(|n| n.name.clone())
        .unwrap_or_else(|| "*".into());
    let obj = build::new_subcatchment(&ed.doc, &polygon, &outlet);
    let name = obj.name.clone();
    if ed.apply(obj.command, &format!("add subcatchment {name}")) {
        ed.select_only(ObjRef::Subcatchment(name.clone()));
        Some(name)
    } else {
        None
    }
}

/// Place a label with the text typed into the prompt.
pub fn finish_label(ed: &mut SwmmEditor) {
    if let Some(((x, y), text)) = ed.edit.label_prompt.take() {
        let text = text.trim().to_string();
        if !text.is_empty() {
            ed.apply(
                build::new_label(x, y, &text),
                &format!("add label \"{text}\""),
            );
        }
    }
}

/// A link-tool click: start at a node, add vertices on empty ground, end
/// at a node.
fn link_click(ed: &mut SwmmEditor, vp: &Viewport, rect: Rect, pos: Pos2, kind: LinkType) -> String {
    let node = hit_test(ed, vp, rect, pos).and_then(|r| match r {
        ObjRef::Node(n) => Some(n),
        _ => None,
    });
    match (ed.edit.link_from.clone(), node) {
        (None, Some(n)) => {
            ed.edit.link_from = Some(n.clone());
            format!(
                "{} from {n}: click the next vertex or the end node",
                kind.label()
            )
        }
        (None, None) => "Click a node to start the link".into(),
        (Some(from), Some(to)) => {
            if from.eq_ignore_ascii_case(&to) && ed.edit.link_vertices.is_empty() {
                return "Click a different node to end the link".into();
            }
            let mut pts = Vec::new();
            if let Some(a) = ed.node(&from) {
                pts.push((a.x, a.y));
            }
            pts.extend(ed.edit.link_vertices.iter().copied());
            if let Some(b) = ed.node(&to) {
                pts.push((b.x, b.y));
            }
            let length = polyline_length(&pts);
            let obj = build::new_link(&ed.doc, kind, &from, &to, &ed.edit.link_vertices, length);
            let name = obj.name.clone();
            let ok = ed.apply(
                obj.command,
                &format!("add {} {name}", kind.label().to_lowercase()),
            );
            ed.edit.link_from = None;
            ed.edit.link_vertices.clear();
            if ok {
                ed.select_only(ObjRef::Link(name.clone()));
                format!(
                    "Added {} {name}: {from} → {to}",
                    kind.label().to_lowercase()
                )
            } else {
                ed.last_error.clone().unwrap_or_default()
            }
        }
        (Some(_), None) => {
            let world = vp.screen_to_world(rect, pos);
            let p = snap_point(ed, vp, rect, world, true);
            ed.edit.link_vertices.push(p);
            format!(
                "{} vertex {}: click the next vertex or the end node",
                ed.edit.link_vertices.len(),
                kind.label()
            )
        }
    }
}

fn band_select(ed: &mut SwmmEditor, bounds: (f64, f64, f64, f64), shift: bool, ctrl: bool) {
    let mut hits: Vec<ObjRef> = Vec::new();
    hits.extend(
        ed.gages
            .iter()
            .filter(|g| inside(bounds, (g.x, g.y)))
            .map(|g| ObjRef::Gage(g.name.clone())),
    );
    hits.extend(
        ed.subs
            .iter()
            .filter(|s| !s.polygon.is_empty() && s.polygon.iter().all(|p| inside(bounds, *p)))
            .map(|s| ObjRef::Subcatchment(s.name.clone())),
    );
    hits.extend(
        ed.nodes
            .iter()
            .filter(|n| inside(bounds, (n.x, n.y)))
            .map(|n| ObjRef::Node(n.name.clone())),
    );
    hits.extend(
        ed.links
            .iter()
            .filter(|l| !l.path.is_empty() && l.path.iter().all(|p| inside(bounds, *p)))
            .map(|l| ObjRef::Link(l.name.clone())),
    );
    hits.extend(
        ed.labels
            .iter()
            .filter(|l| inside(bounds, (l.x, l.y)))
            .map(|l| ObjRef::Label(l.line)),
    );
    if !shift && !ctrl {
        ed.clear_selection();
    }
    for h in hits {
        if ctrl {
            ed.toggle_select(h);
        } else {
            ed.add_select(h);
        }
    }
}

/// Handle pointer input on the map for one frame.
pub fn interact(ui: &mut Ui, rect: Rect, resp: &Response, state: &mut AppState) {
    let AppState {
        swmm, swmm_doc: ed, ..
    } = state;
    let vp = &mut swmm.map_viewport;
    let mods = ui.input(|i| i.modifiers);
    let hover = resp.hover_pos();
    ed.edit.cursor_world = hover.map(|p| vp.screen_to_world(rect, p));
    ed.edit.hover = hover.and_then(|p| hit_test(ed, vp, rect, p));
    let tool = ed.edit.tool;
    let mut status: Option<String> = None;

    if resp.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            let anchor = hover.unwrap_or_else(|| rect.center());
            vp.zoom_at(rect, anchor, 1.0 + scroll * 0.001);
        }
    }
    let primary_drag = resp.dragged_by(egui::PointerButton::Primary);
    if resp.dragged_by(egui::PointerButton::Middle) || (tool == SwmmTool::Pan && primary_drag) {
        let d = resp.drag_delta();
        vp.pan.x += d.x;
        vp.pan.y -= d.y;
    }

    let pointer = resp.interact_pointer_pos().or(hover).unwrap_or(Pos2::ZERO);
    let world = vp.screen_to_world(rect, pointer);

    // -- drags --------------------------------------------------------------------
    if resp.drag_started_by(egui::PointerButton::Primary) {
        // A drag is recognised a few pixels after the press, so what was
        // grabbed is whatever was under the press, not under the pointer now.
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
        let world = vp.screen_to_world(rect, origin);
        match tool {
            SwmmTool::Select => {
                if let Some((target, index)) = vertex_hit(ed, vp, rect, origin) {
                    if let Some(points) = points_of(ed, &target) {
                        ed.edit.active_vertex = Some((target.clone(), index));
                        ed.begin_gesture(&format!("move vertex of {}", describe(&target)));
                        ed.edit.drag = Some(Drag::Vertex {
                            target,
                            index,
                            points,
                        });
                    }
                } else if let Some(hit) = hit_test(ed, vp, rect, origin) {
                    if !ed.is_selected(&hit) {
                        if mods.shift || mods.ctrl {
                            ed.add_select(hit);
                        } else {
                            ed.select_only(hit);
                        }
                    }
                    let label = match ed.selection.as_slice() {
                        [one] => format!("move {}", describe(one)),
                        many => format!("move {} objects", many.len()),
                    };
                    ed.begin_gesture(&label);
                    ed.edit.drag = Some(move_origins(ed, world));
                } else {
                    ed.edit.drag = Some(Drag::Band {
                        start: origin,
                        end: pointer,
                    });
                }
            }
            SwmmTool::ZoomWindow => {
                ed.edit.drag = Some(Drag::Band {
                    start: origin,
                    end: pointer,
                });
            }
            _ => {}
        }
    }
    if primary_drag {
        if let Some(drag) = ed.edit.drag.clone() {
            match &drag {
                Drag::Move { start, nodes, .. } => {
                    let mut dx = world.0 - start.0;
                    let mut dy = world.1 - start.1;
                    if ed.snap_grid {
                        // The first node lands on the grid; the rest keep
                        // their spacing.
                        if let Some((_, x, y)) = nodes.first() {
                            let (sx, sy) = snap_to_grid(x + dx, y + dy, ed.grid_spacing);
                            dx = sx - x;
                            dy = sy - y;
                        }
                    }
                    apply_move(ed, &drag, dx, dy);
                }
                Drag::Vertex {
                    target,
                    index,
                    points,
                } => {
                    let mut pts = points.clone();
                    if *index < pts.len() {
                        pts[*index] = snap_point(ed, vp, rect, world, false);
                        set_points(ed, target, pts, "move vertex");
                    }
                }
                Drag::Band { start, .. } => {
                    ed.edit.drag = Some(Drag::Band {
                        start: *start,
                        end: pointer,
                    });
                }
            }
        }
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary) {
        if let Some(drag) = ed.edit.drag.take() {
            match drag {
                Drag::Move { .. } | Drag::Vertex { .. } => {
                    ed.end_gesture();
                    status = Some(format!("Moved {}", ed.selection_summary()));
                }
                Drag::Band { start, end } => {
                    let bounds = band_world(vp, rect, start, end);
                    if tool == SwmmTool::ZoomWindow {
                        if (end - start).length() > 4.0 {
                            vp.fit_bounds(rect, bounds.0, bounds.1, bounds.2, bounds.3);
                        }
                    } else {
                        band_select(ed, bounds, mods.shift, mods.ctrl);
                        status = Some(match ed.selection.len() {
                            0 => "Nothing in the window".into(),
                            n => format!("{n} selected"),
                        });
                    }
                }
            }
        }
    }

    // -- clicks -------------------------------------------------------------------
    if resp.clicked() {
        match tool {
            SwmmTool::Select => {
                if let Some((target, index)) = vertex_hit(ed, vp, rect, pointer) {
                    ed.edit.active_vertex = Some((target, index));
                    status = Some("Vertex chosen — drag it, or press Delete".into());
                } else if let Some(hit) = hit_test(ed, vp, rect, pointer) {
                    if mods.ctrl {
                        ed.toggle_select(hit);
                    } else if mods.shift {
                        ed.add_select(hit);
                    } else {
                        ed.select_only(hit);
                    }
                    status = Some(format!("Selected {}", ed.selection_summary()));
                } else if !mods.shift && !mods.ctrl {
                    ed.clear_selection();
                }
            }
            SwmmTool::Pan | SwmmTool::ZoomWindow => {}
            SwmmTool::ZoomIn => vp.zoom_at(rect, pointer, 1.5),
            SwmmTool::ZoomOut => vp.zoom_at(rect, pointer, 1.0 / 1.5),
            SwmmTool::AddGage => {
                let p = snap_point(ed, vp, rect, world, false);
                let obj = build::new_gage(&ed.doc, p.0, p.1);
                let name = obj.name.clone();
                if ed.apply(obj.command, &format!("add rain gage {name}")) {
                    ed.select_only(ObjRef::Gage(name.clone()));
                    status = Some(format!("Added rain gage {name}"));
                }
            }
            SwmmTool::AddNode(kind) => {
                let p = snap_point(ed, vp, rect, world, false);
                let obj = build::new_node(&ed.doc, kind, p.0, p.1);
                let name = obj.name.clone();
                if ed.apply(
                    obj.command,
                    &format!("add {} {name}", kind.label().to_lowercase()),
                ) {
                    ed.select_only(ObjRef::Node(name.clone()));
                    status = Some(format!("Added {} {name}", kind.label().to_lowercase()));
                }
            }
            SwmmTool::AddLink(kind) => {
                status = Some(link_click(ed, vp, rect, pointer, kind));
            }
            SwmmTool::AddSubcatchment => {
                let p = snap_point(ed, vp, rect, world, true);
                let closes = ed.edit.polygon.len() >= 3
                    && (w2s(vp, rect, ed.edit.polygon[0]) - pointer).length() <= HIT_RADIUS;
                if closes {
                    if let Some(name) = finish_polygon(ed) {
                        status = Some(format!("Added subcatchment {name}"));
                    }
                } else {
                    ed.edit.polygon.push(p);
                    status = Some(format!(
                        "{} corner(s) — double-click, Enter, or the first corner closes",
                        ed.edit.polygon.len()
                    ));
                }
            }
            SwmmTool::AddLabel => {
                ed.edit.label_prompt = Some((world, String::new()));
            }
        }
    }
    if resp.double_clicked() {
        match tool {
            SwmmTool::AddSubcatchment => {
                // The second click of the pair added a corner on top of the
                // previous one; it is not a corner.
                if ed.edit.polygon.len() >= 2 {
                    let n = ed.edit.polygon.len();
                    let a = w2s(vp, rect, ed.edit.polygon[n - 1]);
                    let b = w2s(vp, rect, ed.edit.polygon[n - 2]);
                    if (a - b).length() <= HIT_RADIUS {
                        ed.edit.polygon.pop();
                    }
                }
                if let Some(name) = finish_polygon(ed) {
                    status = Some(format!("Added subcatchment {name}"));
                }
            }
            SwmmTool::Select => {
                if insert_vertex(ed, vp, rect, pointer) {
                    status = Some("Added a vertex".into());
                } else if let Some(hit) = hit_test(ed, vp, rect, pointer) {
                    ed.select_only(hit.clone());
                    ed.show_properties = true;
                    status = Some(format!("{}: properties", describe(&hit)));
                }
            }
            _ => {}
        }
    }
    if resp.secondary_clicked() {
        let hit = hit_test(ed, vp, rect, pointer);
        if let Some(h) = &hit {
            if !ed.is_selected(h) {
                ed.select_only(h.clone());
            }
        }
        ed.edit.context = hit;
        ed.edit.context_world = Some(world);
    }
    if let Some(s) = status {
        state.status = s;
    }
    if let Some(e) = state.swmm_doc.last_error.take() {
        state.status = e;
    }
}

/// The right-click menu on the map.
pub fn context_menu(ui: &mut Ui, state: &mut AppState, rect: Rect) {
    let target = state.swmm_doc.edit.context.clone();
    let Some(target) = target else {
        ui.label("Right-click an object");
        if ui.button("Zoom Extents").clicked() {
            state.swmm.pending_map_fit = true;
            ui.close_menu();
        }
        return;
    };
    ui.label(describe(&target));
    ui.separator();
    if ui.button("Edit Properties…").clicked() {
        state.swmm_doc.select_only(target.clone());
        state.swmm_doc.show_properties = true;
        ui.close_menu();
    }
    if ui.button("Delete").clicked() {
        if !state.swmm_doc.is_selected(&target) {
            state.swmm_doc.select_only(target.clone());
        }
        if let Some(n) = state.swmm_doc.delete_selection() {
            state.status = format!("Deleted {n} object(s)");
        }
        ui.close_menu();
    }
    if let ObjRef::Link(name) = &target {
        if ui.button("Reverse Link").clicked() {
            if state.swmm_doc.reverse_link(name) {
                state.status = format!("Reversed {name}");
            }
            ui.close_menu();
        }
    }
    if let ObjRef::Node(name) = &target {
        let current = build::node_type_of(&state.swmm_doc.doc, name);
        ui.menu_button("Convert Node Type", |ui| {
            for kind in NodeType::ALL {
                if ui
                    .add_enabled(current != Some(kind), egui::Button::new(kind.label()))
                    .clicked()
                {
                    if state.swmm_doc.convert_node(name, kind) {
                        state.status = format!("{name} is now a {}", kind.label().to_lowercase());
                    }
                    ui.close_menu();
                }
            }
        });
    }
    if ui.button("Zoom To").clicked() {
        zoom_to(state, rect, &target);
        ui.close_menu();
    }
}

/// Fit the viewport to one object with room around it.
pub fn zoom_to(state: &mut AppState, rect: Rect, target: &ObjRef) {
    let Some((x0, y0, x1, y1)) = state.swmm_doc.bounds_of(target) else {
        return;
    };
    let span = state
        .swmm_doc
        .bounds
        .map(|(a, b, c, d)| (c - a).max(d - b))
        .unwrap_or(100.0);
    let pad = ((x1 - x0).max(y1 - y0) * 0.5).max(span * 0.05).max(1.0);
    state
        .swmm
        .map_viewport
        .fit_bounds(rect, x0 - pad, y0 - pad, x1 + pad, y1 + pad);
}

// --- drawing ------------------------------------------------------------------

/// A grid spacing (1, 2, 5 × 10^k world units) at least `min_px` apart.
fn grid_spacing(zoom: f32, min_px: f32) -> f64 {
    let min_world = (min_px / zoom.max(1e-6)) as f64;
    let mut s = 10f64.powf(min_world.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if s * m >= min_world {
            return s * m;
        }
    }
    s *= 10.0;
    s
}

fn draw_grid(painter: &egui::Painter, rect: Rect, vp: &Viewport, dark: bool) {
    let spacing = grid_spacing(vp.zoom, 40.0);
    let (wx0, wy0) = vp.screen_to_world(rect, rect.left_top());
    let (wx1, wy1) = vp.screen_to_world(rect, rect.right_bottom());
    let stroke = Stroke::new(1.0_f32, palette::canvas::grid(dark));
    let mut x = (wx0.min(wx1) / spacing).floor() * spacing;
    let mut guard = 0;
    while x <= wx1.max(wx0) && guard < 400 {
        painter.line_segment(
            [
                w2s(vp, rect, (x, wy0.min(wy1))),
                w2s(vp, rect, (x, wy0.max(wy1))),
            ],
            stroke,
        );
        x += spacing;
        guard += 1;
    }
    let mut y = (wy0.min(wy1) / spacing).floor() * spacing;
    guard = 0;
    while y <= wy1.max(wy0) && guard < 400 {
        painter.line_segment(
            [
                w2s(vp, rect, (wx0.min(wx1), y)),
                w2s(vp, rect, (wx0.max(wx1), y)),
            ],
            stroke,
        );
        y += spacing;
        guard += 1;
    }
}

fn node_symbol(
    painter: &egui::Painter,
    c: Pos2,
    kind: NodeType,
    r: f32,
    fill: Color32,
    stroke: Stroke,
) {
    match kind {
        NodeType::Junction => {
            painter.circle_filled(c, r, fill);
            painter.circle_stroke(c, r, stroke);
        }
        NodeType::Outfall => {
            let pts = vec![
                c + Vec2::new(-r * 1.1, -r * 0.9),
                c + Vec2::new(r * 1.1, -r * 0.9),
                c + Vec2::new(0.0, r * 1.1),
            ];
            painter.add(Shape::convex_polygon(pts, fill, stroke));
        }
        NodeType::Storage => {
            let rr = Rect::from_center_size(c, Vec2::new(r * 2.4, r * 1.7));
            painter.rect_filled(rr, 1.0, fill);
            painter.rect_stroke(rr, 1.0, stroke);
        }
        NodeType::Divider => {
            let pts = vec![
                c + Vec2::new(0.0, -r * 1.3),
                c + Vec2::new(r * 1.3, 0.0),
                c + Vec2::new(0.0, r * 1.3),
                c + Vec2::new(-r * 1.3, 0.0),
            ];
            painter.add(Shape::convex_polygon(pts, fill, stroke));
        }
    }
}

fn gage_symbol(painter: &egui::Painter, c: Pos2, r: f32, fill: Color32, stroke: Stroke) {
    // A drop: a triangle over a circle.
    let pts = vec![
        c + Vec2::new(0.0, -r * 1.8),
        c + Vec2::new(r * 0.95, r * 0.2),
        c + Vec2::new(-r * 0.95, r * 0.2),
    ];
    painter.add(Shape::convex_polygon(pts, fill, Stroke::NONE));
    painter.circle_filled(c + Vec2::new(0.0, r * 0.3), r, fill);
    painter.circle_stroke(c + Vec2::new(0.0, r * 0.3), r, stroke);
}

/// The midpoint of the longest segment and its direction.
fn mid_and_dir(pts: &[Pos2]) -> Option<(Pos2, Vec2)> {
    pts.windows(2)
        .map(|w| (w[0], w[1], (w[1] - w[0]).length()))
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(a, b, len)| {
            let dir = if len > 0.0 { (b - a) / len } else { Vec2::X };
            (Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), dir)
        })
}

fn arrow(painter: &egui::Painter, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
    let n = Vec2::new(-dir.y, dir.x);
    let base = tip - dir * size;
    painter.add(Shape::convex_polygon(
        vec![tip, base + n * size * 0.5, base - n * size * 0.5],
        color,
        Stroke::NONE,
    ));
}

fn link_mark(
    painter: &egui::Painter,
    kind: LinkType,
    mid: Pos2,
    dir: Vec2,
    color: Color32,
    ink: Color32,
) {
    let n = Vec2::new(-dir.y, dir.x);
    match kind {
        LinkType::Conduit => {}
        LinkType::Pump => {
            painter.circle_filled(mid, 6.0, color);
            painter.circle_stroke(mid, 6.0, Stroke::new(1.0_f32, ink));
            arrow(painter, mid + dir * 5.0, dir, 6.0, ink);
        }
        LinkType::Orifice => {
            painter.circle_stroke(mid, 6.0, Stroke::new(2.5_f32, color));
            painter.circle_stroke(mid, 6.0, Stroke::new(1.0_f32, ink));
        }
        LinkType::Weir => {
            // A notch across the line.
            painter.add(Shape::line(
                vec![mid + n * 7.0 - dir * 5.0, mid, mid + n * 7.0 + dir * 5.0],
                Stroke::new(2.0_f32, ink),
            ));
            painter.line_segment([mid - n * 7.0, mid + n * 7.0], Stroke::new(2.0_f32, ink));
        }
        LinkType::Outlet => {
            let rr = Rect::from_center_size(mid, Vec2::splat(10.0));
            painter.rect_filled(rr, 0.0, color);
            painter.rect_stroke(rr, 0.0, Stroke::new(1.0_f32, ink));
        }
    }
}

fn node_color(kind: NodeType) -> Color32 {
    match kind {
        NodeType::Junction => palette::NODE_INLET,
        NodeType::Outfall => palette::NODE_OUTFALL,
        NodeType::Storage | NodeType::Divider => palette::NODE_JUNCTION,
    }
}

/// Results from the last run, keyed by name: whether a node floods and how
/// full a link is — at the animated instant when there is one, else at the
/// peaks. The colour vocabulary is the plan view's.
fn result_colours(state: &AppState) -> (HashMap<String, bool>, HashMap<String, f64>) {
    let mut nodes = HashMap::new();
    let mut links = HashMap::new();
    let frame = state.swmm.frame();
    match (frame, state.swmm.results.as_ref()) {
        (Some(fr), Some(f)) => {
            for (i, id) in f.meta.node_ids.iter().enumerate() {
                if let Some(s) = fr.nodes.get(i) {
                    nodes.insert(id.to_ascii_uppercase(), s.flooding_now());
                }
            }
            for (i, id) in f.meta.link_ids.iter().enumerate() {
                if let Some(s) = fr.links.get(i) {
                    links.insert(id.to_ascii_uppercase(), s.capacity);
                }
            }
        }
        _ => {
            for p in &state.swmm.node_peaks {
                nodes.insert(p.id.to_ascii_uppercase(), p.flooded());
            }
            for p in &state.swmm.link_peaks {
                links.insert(p.id.to_ascii_uppercase(), p.max_capacity);
            }
        }
    }
    (nodes, links)
}

/// Draw the map from the document. Consumes a pending fit or zoom-to.
pub fn draw_map(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    state.swmm_doc.refresh();
    if state.swmm.pending_map_fit {
        if let Some((x0, y0, x1, y1)) = state.swmm_doc.bounds {
            state.swmm.map_viewport.fit_bounds(rect, x0, y0, x1, y1);
        }
        state.swmm.pending_map_fit = false;
    }
    if let Some(target) = state.swmm_doc.pending_zoom_to.take() {
        zoom_to(state, rect, &target);
    }
    let (flooded, capacity) = result_colours(state);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));
    let vp = &state.swmm.map_viewport;
    let ed = &state.swmm_doc;

    if !ed.loaded {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "File → New SWMM Model, or Open .inp…",
            egui::FontId::proportional(15.0),
            palette::canvas::muted(dark),
        );
        return;
    }
    if ed.show_grid {
        draw_grid(&painter, rect, vp, dark);
    }

    // Underlays, shared with the plan view.
    if let Some(bg_dxf) = &state.project.background_dxf {
        let alpha = (bg_dxf.opacity * 255.0).round() as u8;
        let color = Color32::from_rgba_unmultiplied(100, 100, 100, alpha);
        for seg in &state.dxf_underlay {
            painter.line_segment(
                [
                    w2s(vp, rect, (seg.x1, seg.y1)),
                    w2s(vp, rect, (seg.x2, seg.y2)),
                ],
                Stroke::new(1.0_f32, color),
            );
        }
    }
    if let (Some(tex), Some(bg)) = (state.bg_texture.as_ref(), &state.project.background) {
        let tex_w = tex.size()[0].max(1) as f32;
        let tex_h = tex.size()[1].max(1) as f32;
        let aspect = tex_h as f64 / tex_w as f64;
        let w = bg.width as f32 * vp.zoom;
        let h = w * (tex_h / tex_w);
        let tl = w2s(vp, rect, (bg.origin_x, bg.origin_y + bg.width * aspect));
        painter.image(
            tex.id(),
            Rect::from_min_size(tl, Vec2::new(w, h)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_white_alpha((bg.opacity * 255.0) as u8),
        );
    }

    let ink = palette::canvas::ink(dark);
    let sel_color = palette::canvas::selection(dark);
    let font = egui::FontId::proportional(12.0);

    // Subcatchments.
    for s in &ed.subs {
        if s.polygon.len() < 2 {
            continue;
        }
        let selected = ed.is_selected(&ObjRef::Subcatchment(s.name.clone()));
        let pts: Vec<Pos2> = s.polygon.iter().map(|p| w2s(vp, rect, *p)).collect();
        if pts.len() >= 3 {
            // Filled as a fan of triangles from the centroid; concave
            // outlines get a slightly wrong fill but a correct outline.
            let fill = if selected {
                SUB_FILL_SELECTED
            } else {
                SUB_FILL
            };
            let c = s.centroid.map(|c| w2s(vp, rect, c)).unwrap_or(pts[0]);
            for i in 0..pts.len() {
                painter.add(Shape::convex_polygon(
                    vec![c, pts[i], pts[(i + 1) % pts.len()]],
                    fill,
                    Stroke::NONE,
                ));
            }
        }
        let mut outline = pts.clone();
        outline.push(pts[0]);
        let stroke = if selected {
            Stroke::new(2.5_f32, sel_color)
        } else {
            Stroke::new(1.2_f32, palette::OK_GREEN)
        };
        painter.add(Shape::line(outline, stroke));
        if let Some(c) = s.centroid {
            let cs = w2s(vp, rect, c);
            let outlet = ed
                .node(&s.outlet)
                .map(|n| (n.x, n.y))
                .or_else(|| ed.sub(&s.outlet).and_then(|t| t.centroid));
            if let Some(o) = outlet {
                painter.add(Shape::dashed_line(
                    &[cs, w2s(vp, rect, o)],
                    Stroke::new(1.0_f32, palette::canvas::muted(dark)),
                    6.0,
                    4.0,
                ));
            }
            painter.circle_filled(cs, 3.0, palette::OK_GREEN);
            if ed.show_labels {
                painter.text(
                    cs + Vec2::new(6.0, -6.0),
                    egui::Align2::LEFT_BOTTOM,
                    &s.name,
                    font.clone(),
                    ink,
                );
            }
        }
    }

    // Links.
    for l in &ed.links {
        if l.path.len() < 2 {
            continue;
        }
        let selected = ed.is_selected(&ObjRef::Link(l.name.clone()));
        let color = match capacity.get(&l.name.to_ascii_uppercase()) {
            Some(c) if *c >= 1.0 => palette::ERROR,
            Some(c) if *c >= 0.85 => palette::WARNING,
            Some(_) => palette::FLOW_OK,
            None => palette::canvas::line(dark),
        };
        let (color, width) = if selected {
            (sel_color, 4.5_f32)
        } else {
            (color, 2.5_f32)
        };
        let pts: Vec<Pos2> = l.path.iter().map(|p| w2s(vp, rect, *p)).collect();
        painter.add(Shape::line(pts.clone(), Stroke::new(width, color)));
        if let Some((mid, dir)) = mid_and_dir(&pts) {
            if ed.show_arrows {
                arrow(
                    &painter,
                    mid + dir * 5.0,
                    dir,
                    9.0,
                    if selected { sel_color } else { ink },
                );
            }
            link_mark(&painter, l.kind, mid, dir, color, ink);
            if ed.show_labels {
                let n = Vec2::new(-dir.y, dir.x);
                painter.text(
                    mid + n * 9.0,
                    egui::Align2::CENTER_CENTER,
                    &l.name,
                    font.clone(),
                    palette::canvas::muted(dark),
                );
            }
        }
        if selected {
            let active = ed.edit.active_vertex.as_ref();
            for (i, v) in l.vertices.iter().enumerate() {
                let p = w2s(vp, rect, *v);
                let rr = Rect::from_center_size(p, Vec2::splat(7.0));
                let is_active = active.is_some_and(|(t, k)| {
                    *k == i && t.is(stormsewer_swmm::doc::ObjectKind::Link, &l.name)
                });
                if is_active {
                    painter.rect_filled(rr, 0.0, sel_color);
                } else {
                    painter.rect_filled(rr, 0.0, palette::canvas::bg(dark));
                }
                painter.rect_stroke(rr, 0.0, Stroke::new(1.5_f32, sel_color));
            }
        }
    }
    for s in &ed.subs {
        if !ed.is_selected(&ObjRef::Subcatchment(s.name.clone())) {
            continue;
        }
        let active = ed.edit.active_vertex.as_ref();
        for (i, v) in s.polygon.iter().enumerate() {
            let p = w2s(vp, rect, *v);
            let rr = Rect::from_center_size(p, Vec2::splat(7.0));
            let is_active = active.is_some_and(|(t, k)| {
                *k == i && t.is(stormsewer_swmm::doc::ObjectKind::Subcatchment, &s.name)
            });
            if is_active {
                painter.rect_filled(rr, 0.0, sel_color);
            } else {
                painter.rect_filled(rr, 0.0, palette::canvas::bg(dark));
            }
            painter.rect_stroke(rr, 0.0, Stroke::new(1.5_f32, sel_color));
        }
    }

    // In-progress link and polygon.
    let cursor = ed.edit.cursor_world;
    if let Some(from) = &ed.edit.link_from {
        if let Some(n) = ed.node(from) {
            let mut pts = vec![w2s(vp, rect, (n.x, n.y))];
            pts.extend(ed.edit.link_vertices.iter().map(|p| w2s(vp, rect, *p)));
            if let Some(c) = cursor {
                pts.push(w2s(vp, rect, c));
            }
            painter.add(Shape::line(
                pts.clone(),
                Stroke::new(2.0_f32, palette::ACCENT),
            ));
            for p in &pts[..pts.len().saturating_sub(1)] {
                painter.circle_filled(*p, 3.5, palette::ACCENT);
            }
        }
    }
    if !ed.edit.polygon.is_empty() {
        let mut pts: Vec<Pos2> = ed.edit.polygon.iter().map(|p| w2s(vp, rect, *p)).collect();
        for p in &pts {
            painter.circle_filled(*p, 3.5, palette::ACCENT);
        }
        if let Some(c) = cursor {
            pts.push(w2s(vp, rect, c));
        }
        painter.add(Shape::line(
            pts.clone(),
            Stroke::new(2.0_f32, palette::ACCENT),
        ));
        if pts.len() >= 3 {
            painter.add(Shape::dashed_line(
                &[pts[pts.len() - 1], pts[0]],
                Stroke::new(1.0_f32, palette::ACCENT),
                5.0,
                4.0,
            ));
        }
    }

    // Nodes.
    for n in &ed.nodes {
        let c = w2s(vp, rect, (n.x, n.y));
        let selected = ed.is_selected(&ObjRef::Node(n.name.clone()));
        let hovered = ed
            .edit
            .hover
            .as_ref()
            .is_some_and(|h| h.is(stormsewer_swmm::doc::ObjectKind::Node, &n.name));
        let mut fill = node_color(n.kind);
        if flooded
            .get(&n.name.to_ascii_uppercase())
            .copied()
            .unwrap_or(false)
        {
            fill = palette::ERROR;
        }
        let r = if selected { 8.0_f32 } else { 6.0_f32 };
        let stroke = Stroke::new(1.5_f32, if selected { sel_color } else { ink });
        node_symbol(&painter, c, n.kind, r, fill, stroke);
        if hovered && !selected {
            painter.circle_stroke(c, r + 5.0, Stroke::new(1.0_f32, palette::ACCENT));
        }
        if ed.show_labels {
            painter.text(
                c + Vec2::new(9.0, -9.0),
                egui::Align2::LEFT_BOTTOM,
                &n.name,
                font.clone(),
                ink,
            );
        }
    }

    // Gages.
    for g in &ed.gages {
        let c = w2s(vp, rect, (g.x, g.y));
        let selected = ed.is_selected(&ObjRef::Gage(g.name.clone()));
        let stroke = Stroke::new(1.5_f32, if selected { sel_color } else { ink });
        gage_symbol(
            &painter,
            c,
            if selected { 6.0_f32 } else { 5.0_f32 },
            palette::FLOW_OK,
            stroke,
        );
        if ed.show_labels {
            painter.text(
                c + Vec2::new(9.0, -9.0),
                egui::Align2::LEFT_BOTTOM,
                &g.name,
                font.clone(),
                palette::canvas::muted(dark),
            );
        }
    }

    // Labels.
    for l in &ed.labels {
        let c = w2s(vp, rect, (l.x, l.y));
        let selected = ed.is_selected(&ObjRef::Label(l.line));
        let galley = painter.layout_no_wrap(l.text.clone(), egui::FontId::proportional(13.0), ink);
        let r = Rect::from_min_size(c, galley.size()).expand(2.0);
        if selected {
            painter.rect_stroke(r, 2.0, Stroke::new(1.5_f32, sel_color));
        }
        painter.galley(c, galley, ink);
    }

    // Rubber band / zoom window.
    if let Some(Drag::Band { start, end }) = &ed.edit.drag {
        let r = Rect::from_two_pos(*start, *end);
        painter.rect_filled(r, 0.0, Color32::from_rgba_premultiplied(224, 86, 127, 25));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, palette::ACCENT));
    }

    // Header.
    let header = format!(
        "{}  ·  {} nodes, {} links, {} subcatchments  ·  {}",
        ed.file_name(),
        ed.nodes.len(),
        ed.links.len(),
        ed.subs.len(),
        ed.edit.tool.label()
    );
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        header,
        egui::FontId::proportional(13.0),
        palette::canvas::muted(dark),
    );
}

/// The editable map: input first, then the picture of the result.
pub fn canvas(ui: &mut Ui, rect: Rect, resp: &Response, state: &mut AppState) {
    if state.swmm_doc.loaded {
        interact(ui, rect, resp, state);
        resp.context_menu(|ui| context_menu(ui, state, rect));
    }
    draw_map(ui, rect, state);
}

/// Cursor position, tool, object under the cursor, undo depth, dirty flag.
pub fn draw_status_bar(ui: &mut Ui, state: &AppState) {
    let ed = &state.swmm_doc;
    ui.horizontal(|ui| {
        ui.label(format!(
            "Tool: {} ({})",
            ed.edit.tool.label(),
            ed.edit.tool.shortcut()
        ));
        ui.separator();
        match ed.edit.cursor_world {
            Some((x, y)) => ui.label(RichText::new(format!("X {x:.2}  Y {y:.2}")).monospace()),
            None => ui.label(RichText::new("X —  Y —").monospace()),
        };
        ui.separator();
        if let Some(h) = &ed.edit.hover {
            ui.label(describe(h));
            ui.separator();
        }
        let sel = ed.selection_summary();
        if !sel.is_empty() {
            ui.label(format!("selected: {sel}"));
            ui.separator();
        }
        ui.label(format!("undo {}", ed.undo_depth()));
        ui.separator();
        if ed.dirty() {
            ui.label(
                RichText::new("● Unsaved").color(palette::accent_text(ui.visuals().dark_mode)),
            );
            ui.separator();
        }
        ui.label(ed.edit.tool.hint());
        ui.separator();
        ui.label(&state.status);
    });
}

/// Error and warning counts; expands to the list, where a click selects
/// the object and zooms to it.
pub fn draw_findings_strip(ui: &mut Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let errors = state.swmm_doc.error_count();
    let warnings = state.swmm_doc.warning_count();
    ui.horizontal(|ui| {
        let text = if errors + warnings == 0 {
            RichText::new("Validation: no findings").color(palette::ok_text(dark))
        } else {
            RichText::new(format!(
                "Validation: {errors} error(s), {warnings} warning(s)"
            ))
            .color(if errors > 0 {
                palette::error_text(dark)
            } else {
                palette::warning_text(dark)
            })
        };
        if ui
            .selectable_label(state.swmm_doc.show_findings, text)
            .on_hover_text("Click to list; click a finding to select the object")
            .clicked()
        {
            state.swmm_doc.show_findings = !state.swmm_doc.show_findings;
        }
    });
    if !state.swmm_doc.show_findings || errors + warnings == 0 {
        return;
    }
    let findings = state.swmm_doc.findings.clone();
    egui::ScrollArea::vertical()
        .max_height(140.0)
        .show(ui, |ui| {
            for f in &findings {
                let tag = match f.severity {
                    Severity::Error => "E",
                    Severity::Warning => "W",
                };
                let color = match f.severity {
                    Severity::Error => palette::error_text(dark),
                    Severity::Warning => palette::warning_text(dark),
                };
                let target = state.swmm_doc.finding_target(f);
                let selected = target
                    .as_ref()
                    .is_some_and(|t| state.swmm_doc.is_selected(t));
                let line = format!("[{tag}] [{}] {}: {}", f.section, f.name, f.message);
                if ui
                    .selectable_label(selected, RichText::new(line).color(color).small())
                    .clicked()
                {
                    if let Some(t) = target {
                        state.swmm_doc.select_only(t.clone());
                        state.swmm_doc.pending_zoom_to = Some(t);
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_spacing_is_a_nice_number_at_least_the_minimum() {
        for zoom in [0.05_f32, 0.3, 1.0, 4.0, 8.0] {
            let s = grid_spacing(zoom, 40.0);
            assert!(s * zoom as f64 >= 40.0 - 1e-6, "zoom {zoom}: {s}");
            let m = s / 10f64.powf(s.log10().floor());
            assert!([1.0, 2.0, 5.0].iter().any(|k| (m - k).abs() < 1e-6), "{s}");
        }
    }
}
