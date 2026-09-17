// SPDX-License-Identifier: GPL-3.0-or-later

//! Longitudinal profile of a SWMM model: a run of conduits drawn in section,
//! with inverts, rims, and the water surface from the last run.
//!
//! The path is derived from the map selection rather than asked for separately.
//! Clicking a node already tells the app what the user is looking at, and the
//! question that follows is almost always "where does this go from here", so
//! the profile follows the network downstream to an outfall.
//!
//! # What this draws, and what it does not
//!
//! Conduit inverts are drawn per link, not node to node, because a conduit may
//! leave a structure above that structure's invert through the `InOffset` and
//! `OutOffset` columns. The drop between a node's invert and the pipe hanging
//! off it is a large part of what a profile is read for. Four of the seven EPA
//! sample models use offsets — one by eight feet — so treating them as zero
//! draws those conduits well below where they sit.
//!
//! The rim line is `invert + MaxDepth`, which is what SWMM knows about a
//! junction's ground; it is not surveyed ground, and outfalls carry no depth
//! so the line simply ends there.

use eframe::egui::{self, Pos2, Rect, Stroke, Ui, Vec2};

use stormsewer_swmm::inp::InpModel;

use crate::profile::station_tick_step;
use crate::state::AppState;
use crate::swmm_panel::PlotTarget;
use crate::theme::palette;

// Margins: the left holds elevation labels, the bottom the station axis.
const PAD_LEFT: f32 = 68.0;
const PAD_RIGHT: f32 = 24.0;
const PAD_TOP: f32 = 44.0;
const PAD_BOTTOM: f32 = 48.0;

/// A chain of conduits and the nodes they connect, with cumulative stationing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfilePath {
    /// Nodes along the run, upstream first. One longer than `links`.
    pub nodes: Vec<String>,
    /// Links between consecutive nodes.
    pub links: Vec<String>,
    /// Station of each node, in model length units. Same length as `nodes`.
    pub stations: Vec<f64>,
}

impl ProfilePath {
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    pub fn length(&self) -> f64 {
        self.stations.last().copied().unwrap_or(0.0)
    }
}

/// How many links to follow before giving up. A model with a cycle the visited
/// check somehow misses must still not hang the UI.
const MAX_PATH_LINKS: usize = 5_000;

/// Follow the network downstream from `start`, collecting the run of links.
///
/// Stops at an outfall, at a node with nothing leaving it, or on revisiting a
/// node — a SWMM network may legitimately contain loops, and a profile of a
/// loop is not a profile.
pub fn downstream_path(model: &InpModel, start: &str) -> ProfilePath {
    let mut path = ProfilePath::default();
    if model.node(start).is_none() {
        return path;
    }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut current = start.to_string();
    let mut station = 0.0;

    seen.insert(current.clone());
    path.nodes.push(current.clone());
    path.stations.push(station);

    while path.links.len() < MAX_PATH_LINKS {
        let Some(link) = model.links.iter().find(|l| l.from == current) else {
            break;
        };
        if !seen.insert(link.to.clone()) {
            break;
        }
        // A conduit's own length is the stationing SWMM routes on. Where a
        // section has none — pumps, weirs, orifices — fall back to the plan
        // distance so the run still advances rather than stacking vertically.
        let run = link
            .length
            .filter(|l| *l > 0.0)
            .or_else(|| plan_distance(model, &link.from, &link.to))
            .unwrap_or(0.0);
        station += run;

        path.links.push(link.id.clone());
        path.nodes.push(link.to.clone());
        path.stations.push(station);
        current = link.to.clone();
    }

    path
}

/// Straight-line distance between two nodes, when both carry coordinates.
fn plan_distance(model: &InpModel, from: &str, to: &str) -> Option<f64> {
    let a = model.node(from)?.pos()?;
    let b = model.node(to)?.pos()?;
    Some(((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt())
}

/// The node the profile should start from, given what is selected.
///
/// A selected link profiles from its upstream node, so clicking a pipe shows
/// the pipe and everything below it.
fn start_node(state: &AppState, model: &InpModel) -> Option<String> {
    let id = state.swmm.plot_id.as_deref();
    match (state.swmm.plot_target, id) {
        (PlotTarget::Node, Some(id)) if model.node(id).is_some() => Some(id.to_string()),
        (PlotTarget::Link, Some(id)) => model.link(id).map(|l| l.from.clone()),
        _ => model.nodes.first().map(|n| n.id.clone()),
    }
}

/// Water-surface elevation at each node: the frame's depth when the map is
/// animating, otherwise the run's peak depth. `None` where there are no
/// results, which draws no water line rather than a flat one at the invert.
fn water_surface(state: &AppState, path: &ProfilePath, model: &InpModel) -> Vec<Option<f64>> {
    let frame = state.swmm.frame();
    let index: std::collections::HashMap<&str, usize> = state
        .swmm
        .results
        .as_ref()
        .map(|f| {
            f.meta
                .node_ids
                .iter()
                .enumerate()
                .map(|(i, s)| (s.as_str(), i))
                .collect()
        })
        .unwrap_or_default();

    path.nodes
        .iter()
        .map(|id| {
            let invert = model.node(id)?.invert?;
            let depth = match (frame, index.get(id.as_str())) {
                (Some(fr), Some(&i)) => fr.nodes.get(i).map(|s| s.depth),
                _ => state
                    .swmm
                    .node_peaks
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| p.max_depth),
            }?;
            Some(invert + depth)
        })
        .collect()
}

/// Invert elevations at the two ends of the `i`th conduit on the path,
/// including its `InOffset` and `OutOffset`.
///
/// `None` when either end node has no invert, so a gap is drawn as a gap
/// rather than as a pipe running to an invented elevation.
pub fn conduit_invert_ends(model: &InpModel, path: &ProfilePath, i: usize) -> Option<(f64, f64)> {
    let link = model.link(path.links.get(i)?)?;
    let up = model.node(path.nodes.get(i)?)?.invert? + link.in_offset.unwrap_or(0.0);
    let down = model.node(path.nodes.get(i + 1)?)?.invert? + link.out_offset.unwrap_or(0.0);
    Some((up, down))
}

/// Draw the profile of the selected run.
pub fn draw_swmm_profile(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));

    let empty_state = |line: &str| {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            line,
            egui::FontId::proportional(15.0),
            palette::canvas::muted(dark),
        );
    };

    let Some(model) = state.swmm.model_inp.as_ref() else {
        empty_state("Choose a model (.inp) in the SWMM tab to see a profile");
        return;
    };
    let Some(start) = start_node(state, model) else {
        empty_state("This model has no nodes to profile");
        return;
    };
    let path = downstream_path(model, &start);
    if path.is_empty() {
        empty_state(&format!("Nothing runs downstream of {start}"));
        return;
    }

    // Elevations along the run. A node without an invert breaks the line
    // rather than being drawn at zero.
    let node_inverts: Vec<Option<f64>> = path
        .nodes
        .iter()
        .map(|id| model.node(id).and_then(|n| n.invert))
        .collect();
    let rims: Vec<Option<f64>> = path
        .nodes
        .iter()
        .map(|id| {
            let n = model.node(id)?;
            Some(n.invert? + n.max_depth?)
        })
        .collect();
    let water = water_surface(state, &path, model);
    // A conduit hangs off its end nodes rather than sitting on them, so its
    // two ends are carried apart from the node inverts.
    let conduit_ends: Vec<Option<(f64, f64)>> = (0..path.links.len())
        .map(|i| conduit_invert_ends(model, &path, i))
        .collect();

    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for value in node_inverts.iter().chain(&rims).chain(&water).flatten() {
        lo = lo.min(*value);
        hi = hi.max(*value);
    }
    for (up, down) in conduit_ends.iter().flatten() {
        lo = lo.min(*up).min(*down);
        hi = hi.max(*up).max(*down);
    }
    if !lo.is_finite() || !hi.is_finite() {
        empty_state("This run has no invert elevations to draw");
        return;
    }
    if hi - lo < 1e-9 {
        lo -= 0.5;
        hi += 0.5;
    } else {
        let pad = (hi - lo) * 0.08;
        lo -= pad;
        hi += pad;
    }

    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD_LEFT, rect.top() + PAD_TOP),
        Pos2::new(rect.right() - PAD_RIGHT, rect.bottom() - PAD_BOTTOM),
    );
    if inner.width() < 40.0 || inner.height() < 40.0 {
        return;
    }
    let total = path.length().max(1e-6);

    // Independent axes, as a hydraulic profile is normally drawn: a run is
    // hundreds of feet long and a few feet deep, so preserving geometry would
    // flatten the whole thing into a line.
    let x_at = |s: f64| inner.left() + (s / total) as f32 * inner.width();
    let y_at = |e: f64| inner.bottom() - ((e - lo) / (hi - lo)) as f32 * inner.height();

    // Elevation gridlines.
    let e_step = station_tick_step(hi - lo);
    let mut e = (lo / e_step).ceil() * e_step;
    while e <= hi + e_step * 0.01 {
        let y = y_at(e);
        painter.line_segment(
            [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
            Stroke::new(1.0_f32, palette::canvas::grid(dark)),
        );
        painter.text(
            Pos2::new(inner.left() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{e:.1}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        e += e_step;
    }

    // Station axis.
    let axis_y = inner.bottom();
    painter.line_segment(
        [
            Pos2::new(inner.left(), axis_y),
            Pos2::new(inner.right(), axis_y),
        ],
        Stroke::new(1.0_f32, palette::canvas::line(dark)),
    );
    let s_step = station_tick_step(total);
    let mut s = 0.0;
    while s <= total + s_step * 0.01 {
        let x = x_at(s);
        painter.line_segment(
            [Pos2::new(x, axis_y), Pos2::new(x, axis_y + 5.0)],
            Stroke::new(1.0_f32, palette::canvas::line(dark)),
        );
        painter.text(
            Pos2::new(x, axis_y + 8.0),
            egui::Align2::CENTER_TOP,
            format!("{s:.0}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        s += s_step;
    }

    // Each line is drawn segment by segment so a gap in the data is a gap on
    // screen, rather than a straight line across the missing part.
    let polyline = |values: &[Option<f64>], stroke: Stroke| {
        for i in 0..values.len().saturating_sub(1) {
            if let (Some(a), Some(b)) = (values[i], values[i + 1]) {
                painter.line_segment(
                    [
                        Pos2::new(x_at(path.stations[i]), y_at(a)),
                        Pos2::new(x_at(path.stations[i + 1]), y_at(b)),
                    ],
                    stroke,
                );
            }
        }
    };

    polyline(&rims, Stroke::new(2.0_f32, palette::PROFILE_GROUND));
    polyline(&water, Stroke::new(2.5_f32, palette::canvas::hgl(dark)));

    // Conduit inverts run link by link. Drawing them node to node would hide
    // every offset drop, which is the thing a reviewer looks for.
    let invert_stroke = Stroke::new(2.0_f32, palette::canvas::invert_line(dark));
    for (i, ends) in conduit_ends.iter().enumerate() {
        let Some((up, down)) = ends else {
            continue;
        };
        painter.line_segment(
            [
                Pos2::new(x_at(path.stations[i]), y_at(*up)),
                Pos2::new(x_at(path.stations[i + 1]), y_at(*down)),
            ],
            invert_stroke,
        );
    }

    // Structure shafts, invert to rim, so the run reads as a sewer rather than
    // as three loose lines.
    for (i, id) in path.nodes.iter().enumerate() {
        let (Some(invert), Some(rim)) = (node_inverts[i], rims[i]) else {
            continue;
        };
        let x = x_at(path.stations[i]);
        painter.line_segment(
            [Pos2::new(x, y_at(invert)), Pos2::new(x, y_at(rim))],
            Stroke::new(1.5_f32, palette::canvas::line(dark)),
        );
        // Label only what will not collide: every node on a short run, and
        // the ends of a long one.
        if path.nodes.len() <= 12 || i == 0 || i + 1 == path.nodes.len() {
            painter.text(
                Pos2::new(x, y_at(rim) - 6.0),
                egui::Align2::CENTER_BOTTOM,
                id,
                egui::FontId::monospace(10.0),
                palette::canvas::ink(dark),
            );
        }
    }

    let units = state
        .swmm
        .results
        .as_ref()
        .map(|f| if f.meta.flow_units.is_metric() { "m" } else { "ft" })
        .unwrap_or("model units");
    let water_note = if state.swmm.frame().is_some() {
        "water surface at this instant"
    } else if state.swmm.node_peaks.is_empty() {
        "no results yet"
    } else {
        "peak water surface"
    };
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        format!(
            "SWMM profile · {} → {} · {} links · {:.0} {units} · {water_note}",
            path.nodes.first().cloned().unwrap_or_default(),
            path.nodes.last().cloned().unwrap_or_default(),
            path.links.len(),
            path.length(),
        ),
        egui::FontId::proportional(13.0),
        palette::canvas::muted(dark),
    );
    painter.text(
        rect.center_bottom() - Vec2::new(0.0, 4.0),
        egui::Align2::CENTER_BOTTOM,
        "Station",
        egui::FontId::proportional(11.0),
        palette::canvas::muted(dark),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight reach: J1 → J2 → J3 → OUT, 100 units apart.
    fn reach() -> InpModel {
        InpModel::parse_str(
            "[JUNCTIONS]\nJ1 100 8\nJ2 99 8\nJ3 98 8\n\
             [OUTFALLS]\nOUT 97 FREE  NO\n\
             [CONDUITS]\nC1 J1 J2 100 0.013 0 0\nC2 J2 J3 100 0.013 0 0\n\
             C3 J3 OUT 100 0.013 0 0\n\
             [COORDINATES]\nJ1 0 0\nJ2 100 0\nJ3 200 0\nOUT 300 0\n",
        )
        .expect("reach should parse")
    }

    #[test]
    fn follows_the_network_downstream_to_the_outfall() {
        let m = reach();
        let p = downstream_path(&m, "J1");
        assert_eq!(p.links, vec!["C1", "C2", "C3"]);
        assert_eq!(p.nodes, vec!["J1", "J2", "J3", "OUT"]);
        assert_eq!(p.stations, vec![0.0, 100.0, 200.0, 300.0]);
        assert_eq!(p.length(), 300.0);
    }

    #[test]
    fn starting_partway_down_profiles_only_what_is_below() {
        let m = reach();
        let p = downstream_path(&m, "J3");
        assert_eq!(p.links, vec!["C3"]);
        assert_eq!(p.nodes, vec!["J3", "OUT"]);
        assert_eq!(p.stations, vec![0.0, 100.0], "stationing restarts at zero");
    }

    #[test]
    fn an_outfall_has_nothing_downstream() {
        let m = reach();
        let p = downstream_path(&m, "OUT");
        assert!(p.is_empty());
        assert_eq!(p.nodes, vec!["OUT"], "the start node is still known");
        assert_eq!(p.length(), 0.0);
    }

    #[test]
    fn an_unknown_start_yields_nothing() {
        let m = reach();
        let p = downstream_path(&m, "NOPE");
        assert!(p.is_empty());
        assert!(p.nodes.is_empty());
    }

    /// A loop must terminate. SWMM networks may legitimately contain one, and
    /// a profile that walked it forever would hang the window.
    #[test]
    fn a_loop_terminates_instead_of_running_forever() {
        let m = InpModel::parse_str(
            "[JUNCTIONS]\nA 10 4\nB 9 4\nC 8 4\n\
             [CONDUITS]\nL1 A B 50 0.013 0 0\nL2 B C 50 0.013 0 0\nL3 C A 50 0.013 0 0\n\
             [COORDINATES]\nA 0 0\nB 50 0\nC 100 0\n",
        )
        .unwrap();
        let p = downstream_path(&m, "A");
        assert_eq!(p.links, vec!["L1", "L2"], "stops on revisiting A");
        assert_eq!(p.nodes, vec!["A", "B", "C"]);
    }

    /// A link with no length of its own — a pump or weir — still advances the
    /// station, using the plan distance, so it does not stack vertically.
    #[test]
    fn a_lengthless_link_advances_by_plan_distance() {
        let m = InpModel::parse_str(
            "[JUNCTIONS]\nA 10 4\n\
             [OUTFALLS]\nO 9 FREE  NO\n\
             [PUMPS]\nP1 A O CURVE1 ON 0 0\n\
             [COORDINATES]\nA 0 0\nO 30 40\n",
        )
        .unwrap();
        let p = downstream_path(&m, "A");
        assert_eq!(p.links, vec!["P1"]);
        // 3-4-5 triangle: the plan distance is 50.
        assert_eq!(p.stations, vec![0.0, 50.0]);
    }

    /// The defect this fixes: a conduit leaving a structure four feet up was
    /// drawn sitting on the structure's invert.
    #[test]
    fn conduit_ends_include_their_offsets() {
        let m = InpModel::parse_str(
            "[JUNCTIONS]\nA 100 8\nB 90 8\n[CONDUITS]\nC1 A B 50 0.013 2 4\n\
             [COORDINATES]\nA 0 0\nB 50 0\n",
        )
        .unwrap();
        let p = downstream_path(&m, "A");
        let (up, down) = conduit_invert_ends(&m, &p, 0).unwrap();
        assert_eq!(up, 102.0, "upstream end sits InOffset above its node invert");
        assert_eq!(down, 94.0, "downstream end sits OutOffset above its node");
    }

    #[test]
    fn a_conduit_without_offsets_sits_on_its_node_inverts() {
        let m = InpModel::parse_str(
            "[JUNCTIONS]\nA 100 8\nB 90 8\n[CONDUITS]\nC1 A B 50 0.013 0 0\n",
        )
        .unwrap();
        let p = downstream_path(&m, "A");
        assert_eq!(conduit_invert_ends(&m, &p, 0), Some((100.0, 90.0)));
    }

    #[test]
    fn an_out_of_range_conduit_index_is_none() {
        let m = reach();
        let p = downstream_path(&m, "J1");
        assert!(conduit_invert_ends(&m, &p, 99).is_none());
    }

    #[test]
    fn a_link_with_no_length_and_no_coordinates_does_not_panic() {
        let m = InpModel::parse_str(
            "[JUNCTIONS]\nA 10 4\n[OUTFALLS]\nO 9 FREE  NO\n[PUMPS]\nP1 A O CURVE1 ON 0 0\n",
        )
        .unwrap();
        let p = downstream_path(&m, "A");
        assert_eq!(p.links, vec!["P1"]);
        assert_eq!(p.stations, vec![0.0, 0.0], "no distance is knowable");
    }
}
