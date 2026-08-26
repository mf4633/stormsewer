// SPDX-License-Identifier: GPL-3.0-or-later

//! Convert an analyzed network into drawable primitives (CAD-agnostic).
//!
//! Produces a **plan** view (pipes as lines, structures as markers, flow/HGL
//! labels) and a **profile** (HGL long-section of the main stem: ground,
//! invert, and hydraulic-grade-line polylines). Coordinates are plain `f64`
//! drawing units; the host CAD turns these into its own entities.

use crate::network::{Analysis, Network, NodeKind};
use std::collections::HashMap;

/// A pipe drawn in plan.
#[derive(Clone, Debug)]
pub struct PlanPipe {
    pub id: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub surcharged: bool,
}

/// A structure marker drawn in plan.
#[derive(Clone, Debug)]
pub struct PlanNode {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub kind: NodeKind,
}

/// A text label placed at a point.
#[derive(Clone, Debug)]
pub struct Label {
    pub x: f64,
    pub y: f64,
    pub text: String,
    pub height: f64,
}

/// Which line of the profile a polyline represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileRole {
    Ground,
    Invert,
    Hgl,
    /// Energy grade line: HGL + V^2/2g of the outgoing pipe.
    Egl,
}

/// A polyline (sequence of points) with a role.
#[derive(Clone, Debug)]
pub struct Polyline {
    pub pts: Vec<(f64, f64)>,
    pub role: ProfileRole,
}

/// The full set of primitives for a network drawing.
#[derive(Clone, Debug, Default)]
pub struct NetworkDrawing {
    pub plan_pipes: Vec<PlanPipe>,
    pub plan_nodes: Vec<PlanNode>,
    pub plan_labels: Vec<Label>,
    pub profile_lines: Vec<Polyline>,
    pub profile_labels: Vec<Label>,
    /// Datum elevation (ft) of the profile — the lowest invert on the main stem,
    /// i.e. the elevation plotted at profile drawing-Y = [`DrawConfig::profile_origin_y`].
    /// Lets a renderer recover absolute elevations for a vertical axis. `0` when
    /// there is no profile.
    pub profile_datum: f64,
}

/// Layout / styling knobs for [`draw_network`].
#[derive(Clone, Debug)]
pub struct DrawConfig {
    pub text_height: f64,
    pub node_radius: f64,
    /// Plan X mapped to profile X=0 (station origin in drawing units).
    pub profile_origin_x: f64,
    /// Drawing Y at which the profile datum elevation is plotted.
    pub profile_origin_y: f64,
    /// Horizontal scale: station feet → drawing units.
    pub h_scale: f64,
    /// Vertical exaggeration applied to elevations in the profile.
    pub v_exag: f64,
}

impl Default for DrawConfig {
    fn default() -> Self {
        Self {
            text_height: 5.0,
            node_radius: 3.0,
            profile_origin_x: 0.0,
            profile_origin_y: -200.0,
            h_scale: 1.0,
            v_exag: 10.0,
        }
    }
}

/// Build plan + profile primitives for an analyzed network.
pub fn draw_network(net: &Network, a: &Analysis, cfg: &DrawConfig) -> NetworkDrawing {
    let mut d = NetworkDrawing::default();
    let pos: HashMap<&str, (f64, f64)> =
        net.nodes.iter().map(|n| (n.id.as_str(), (n.x, n.y))).collect();
    let hgl: HashMap<&str, f64> = a.nodes.iter().map(|n| (n.id.as_str(), n.hgl)).collect();

    // ── Plan: pipes + labels ────────────────────────────────────────────────
    for pr in &a.pipes {
        let (x1, y1) = pos[pr.from.as_str()];
        let (x2, y2) = pos[pr.to.as_str()];
        d.plan_pipes.push(PlanPipe { id: pr.id.clone(), x1, y1, x2, y2, surcharged: pr.surcharged });
        d.plan_labels.push(Label {
            x: (x1 + x2) / 2.0,
            y: (y1 + y2) / 2.0 + cfg.text_height,
            text: format!("{}: {:.1} cfs {:.0}%", pr.id, pr.design_q, pr.pct_full * 100.0),
            height: cfg.text_height,
        });
    }

    // ── Plan: structure markers + labels ────────────────────────────────────
    for n in &net.nodes {
        d.plan_nodes.push(PlanNode { x: n.x, y: n.y, radius: cfg.node_radius, kind: n.kind });
        let h = hgl.get(n.id.as_str()).copied().unwrap_or(f64::NAN);
        let label = if h.is_finite() {
            format!("{} HGL {:.1}", n.id, h)
        } else {
            n.id.clone()
        };
        d.plan_labels.push(Label {
            x: n.x + cfg.node_radius,
            y: n.y + cfg.node_radius,
            text: label,
            height: cfg.text_height,
        });
    }

    // ── Profile of the main stem ────────────────────────────────────────────
    let stem = main_stem(net);
    if stem.len() >= 2 {
        let datum = stem.iter().map(|&i| net.nodes[i].invert).fold(f64::INFINITY, f64::min);
        d.profile_datum = datum;
        let vh = velocity_heads(net, a);
        push_stem_profile(&mut d, net, &hgl, &vh, cfg, &stem, 0.0, datum);
    }

    d
}

/// Append one stem's ground/invert/HGL polylines and node labels to the
/// drawing, starting at `station_offset` (ft). Returns the end station.
fn push_stem_profile(
    d: &mut NetworkDrawing,
    net: &Network,
    hgl: &HashMap<&str, f64>,
    vel_head: &HashMap<&str, f64>,
    cfg: &DrawConfig,
    stem: &[usize],
    station_offset: f64,
    datum: f64,
) -> f64 {
    let mut stations = vec![station_offset; stem.len()];
    for k in 1..stem.len() {
        let len = pipe_between(net, stem[k - 1], stem[k]).map(|p| p.length).unwrap_or(0.0);
        stations[k] = stations[k - 1] + len;
    }
    let px = |st: f64| cfg.profile_origin_x + st * cfg.h_scale;
    let py = |elev: f64| cfg.profile_origin_y + (elev - datum) * cfg.v_exag;

    let mut ground = Vec::new();
    let mut invert = Vec::new();
    let mut hgl_line = Vec::new();
    let mut egl_line = Vec::new();
    for (k, &i) in stem.iter().enumerate() {
        let n = &net.nodes[i];
        let st = stations[k];
        ground.push((px(st), py(n.rim)));
        invert.push((px(st), py(n.invert)));
        if let Some(&h) = hgl.get(n.id.as_str()) {
            if h.is_finite() {
                hgl_line.push((px(st), py(h)));
                if let Some(&vh) = vel_head.get(n.id.as_str()) {
                    if vh.is_finite() {
                        egl_line.push((px(st), py(h + vh)));
                    }
                }
            }
        }
        d.profile_labels.push(Label {
            x: px(st),
            y: py(n.rim) + cfg.text_height,
            text: n.id.clone(),
            height: cfg.text_height,
        });
    }
    d.profile_lines.push(Polyline { pts: ground, role: ProfileRole::Ground });
    d.profile_lines.push(Polyline { pts: invert, role: ProfileRole::Invert });
    if hgl_line.len() >= 2 {
        d.profile_lines.push(Polyline { pts: hgl_line, role: ProfileRole::Hgl });
    }
    if egl_line.len() >= 2 {
        d.profile_lines.push(Polyline { pts: egl_line, role: ProfileRole::Egl });
    }
    *stations.last().unwrap_or(&station_offset)
}

/// Velocity head V^2/2g (ft) at each node, from its outgoing pipe's design
/// velocity (the outfall uses its incoming pipe), for EGL plotting.
fn velocity_heads<'a>(_net: &'a Network, a: &'a Analysis) -> HashMap<&'a str, f64> {
    // The one gravity constant the hydraulics use. A local 32.174 here put the
    // drawn EGL on a different g from the HGL underneath it.
    const G: f64 = crate::hydraulics::G_US;
    let mut vh: HashMap<&str, f64> = HashMap::new();
    for pr in &a.pipes {
        let head = pr.velocity * pr.velocity / (2.0 * G);
        vh.entry(pr.from.as_str()).or_insert(head);
        // Outfalls (no outgoing pipe) take the incoming pipe's head.
        vh.entry(pr.to.as_str()).or_insert(head);
    }
    // Prefer the OUTGOING pipe's head wherever one exists.
    for pr in &a.pipes {
        vh.insert(pr.from.as_str(), pr.velocity * pr.velocity / (2.0 * G));
    }
    vh
}

/// Chain a set of pipe ids into upstream-first stems (runs of node indices).
///
/// Selected pipes that connect end-to-end form one stem; disconnected
/// selections yield multiple stems in network order. Unknown ids are
/// ignored, and each pipe is used at most once, so the result is stable
/// whatever order the user clicked in.
pub fn stems_from_pipes(net: &Network, pipe_ids: &[String]) -> Vec<Vec<usize>> {
    use std::collections::HashSet;
    let nidx: HashMap<&str, usize> =
        net.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let wanted: HashSet<&str> = pipe_ids.iter().map(|s| s.as_str()).collect();
    let selected: Vec<&crate::network::Pipe> =
        net.pipes.iter().filter(|p| wanted.contains(p.id.as_str())).collect();
    if selected.is_empty() {
        return Vec::new();
    }
    let downstream_of: HashSet<&str> = selected.iter().map(|p| p.to.as_str()).collect();
    let mut used: HashSet<&str> = HashSet::new();
    let mut stems = Vec::new();
    // Chain starts: selected pipes whose upstream node is not fed by
    // another selected pipe, in network declaration order for stability.
    for start in &selected {
        if downstream_of.contains(start.from.as_str()) || used.contains(start.id.as_str()) {
            continue;
        }
        let (Some(&u), Some(&v)) = (nidx.get(start.from.as_str()), nidx.get(start.to.as_str()))
        else {
            continue;
        };
        used.insert(start.id.as_str());
        let mut stem = vec![u, v];
        let mut cur = start.to.as_str();
        loop {
            let next = selected.iter().find(|p| {
                p.from == cur && !used.contains(p.id.as_str())
            });
            match next {
                Some(p) => {
                    let Some(&w) = nidx.get(p.to.as_str()) else { break };
                    used.insert(p.id.as_str());
                    stem.push(w);
                    cur = p.to.as_str();
                }
                None => break,
            }
        }
        stems.push(stem);
    }
    stems
}

/// Station gap drawn between disconnected stems in a selected-run profile.
const RUN_GAP_FT: f64 = 60.0;

/// Profile drawing for a user-selected run of pipes (upstream-first),
/// instead of the automatic main stem. Contiguous selections read as one
/// continuous profile; disconnected chains follow with a station gap.
pub fn draw_profile_run(
    net: &Network,
    a: &Analysis,
    cfg: &DrawConfig,
    pipe_ids: &[String],
) -> NetworkDrawing {
    let mut d = NetworkDrawing::default();
    let hgl: HashMap<&str, f64> = a.nodes.iter().map(|n| (n.id.as_str(), n.hgl)).collect();
    let stems = stems_from_pipes(net, pipe_ids);
    if stems.is_empty() {
        return d;
    }
    let datum = stems
        .iter()
        .flatten()
        .map(|&i| net.nodes[i].invert)
        .fold(f64::INFINITY, f64::min);
    d.profile_datum = datum;
    let vh = velocity_heads(net, a);
    let mut station = 0.0;
    for stem in &stems {
        if stem.len() < 2 {
            continue;
        }
        station =
            push_stem_profile(&mut d, net, &hgl, &vh, cfg, stem, station, datum) + RUN_GAP_FT;
    }
    d
}

/// The main trunk, upstream-first: walk from the outfall up the incoming pipe
/// whose upstream node carries the most accumulated drainage area.
fn main_stem(net: &Network) -> Vec<usize> {
    let n = net.nodes.len();
    if n == 0 {
        return Vec::new();
    }
    let nidx: HashMap<&str, usize> =
        net.nodes.iter().enumerate().map(|(i, nd)| (nd.id.as_str(), i)).collect();
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n]; // upstream node indices
    let mut has_out = vec![false; n];
    for p in &net.pipes {
        if let (Some(&u), Some(&v)) = (nidx.get(p.from.as_str()), nidx.get(p.to.as_str())) {
            incoming[v].push(u);
            has_out[u] = true;
        }
    }
    let ca = net.accumulate_ca().unwrap_or_default();
    let size = |i: usize| ca.get(net.nodes[i].id.as_str()).copied().unwrap_or(0.0);

    // Start at the outfall (kind, else the first node with no outgoing pipe).
    let start = net
        .nodes
        .iter()
        .position(|nd| nd.kind == NodeKind::Outfall)
        .or_else(|| (0..n).find(|&i| !has_out[i]))
        .unwrap_or(0);

    let mut stem = vec![start];
    let mut cur = start;
    let mut guard = 0;
    while guard < n {
        guard += 1;
        match incoming[cur].iter().copied().max_by(|&a, &b| {
            size(a).partial_cmp(&size(b)).unwrap_or(std::cmp::Ordering::Equal)
        }) {
            Some(up) => {
                stem.push(up);
                cur = up;
            }
            None => break,
        }
    }
    stem.reverse(); // upstream-first
    stem
}

fn pipe_between<'a>(net: &'a Network, up: usize, dn: usize) -> Option<&'a crate::network::Pipe> {
    let up_id = net.nodes[up].id.as_str();
    let dn_id = net.nodes[dn].id.as_str();
    net.pipes.iter().find(|p| p.from == up_id && p.to == dn_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idf::IdfCurve;
    use crate::network::{AnalysisOptions, Node, Pipe};

    fn sample() -> Network {
        Network {
            nodes: vec![
                Node::inlet("N1", 104.0, 110.0, 1.0, 0.70).at(0.0, 0.0),
                Node::inlet("N2", 102.5, 108.5, 1.0, 0.70).at(300.0, 0.0),
                Node::junction("N3", 101.2, 107.0, 0.5, 0.80).at(550.0, 0.0),
                Node::outfall("OUT", 100.0, 106.0).at(730.0, 0.0),
            ],
            pipes: vec![
                Pipe::new("P1", "N1", "N2", 300.0, 1.25, 0.013),
                Pipe::new("P2", "N2", "N3", 250.0, 1.50, 0.013),
                Pipe::new("P3", "N3", "OUT", 180.0, 1.75, 0.013),
            ],
        }
    }

    fn analyzed() -> (Network, Analysis) {
        let net = sample();
        let a = net.analyze(&IdfCurve::new(60.0, 10.0, 0.8), &AnalysisOptions { tailwater: Some(100.5), ..Default::default() }).unwrap();
        (net, a)
    }

    /// Trunk N1->N2->N3->OUT with branch B1->N2 — a real dendritic tree.
    fn branched() -> Network {
        let mut net = sample();
        net.nodes
            .push(Node::inlet("B1", 105.0, 111.0, 2.0, 0.60).at(300.0, 250.0));
        net.pipes
            .push(Pipe::new("PB1", "B1", "N2", 250.0, 1.25, 0.013));
        net
    }

    fn branched_analyzed() -> (Network, Analysis) {
        let net = branched();
        let a = net
            .analyze(
                &IdfCurve::new(60.0, 10.0, 0.8),
                &AnalysisOptions { tailwater: Some(100.5), ..Default::default() },
            )
            .unwrap();
        (net, a)
    }

    #[test]
    fn main_stem_follows_larger_ca_arm_leaving_other_arm_unprofiled() {
        // Documents WHY selected-run profiles exist: at a junction the
        // automatic profile follows the larger-C*A arm (here B1, 1.2 ac
        // vs N1's 0.7), so the other arm never appears in it.
        let (net, a) = branched_analyzed();
        let d = draw_network(&net, &a, &DrawConfig::default());
        let labels: Vec<&str> =
            d.profile_labels.iter().map(|l| l.text.as_str()).collect();
        assert!(labels.contains(&"B1"), "larger-CA arm should be the stem");
        assert!(
            !labels.contains(&"N1"),
            "smaller arm unexpectedly profiled: {labels:?}"
        );
    }

    #[test]
    fn stems_chain_contiguous_selection_upstream_first() {
        let net = branched();
        // Branch run: PB1 (B1->N2) then P2 (N2->N3) then P3 (N3->OUT),
        // given in scrambled click order.
        let ids = ["P3", "PB1", "P2"].map(String::from).to_vec();
        let stems = stems_from_pipes(&net, &ids);
        assert_eq!(stems.len(), 1, "contiguous selection must form one stem");
        let names: Vec<&str> =
            stems[0].iter().map(|&i| net.nodes[i].id.as_str()).collect();
        assert_eq!(names, ["B1", "N2", "N3", "OUT"]);
    }

    #[test]
    fn stems_split_disconnected_selection() {
        let net = branched();
        // P1 (N1->N2) and P3 (N3->OUT) don't touch: two stems.
        let ids = ["P1", "P3"].map(String::from).to_vec();
        let stems = stems_from_pipes(&net, &ids);
        assert_eq!(stems.len(), 2);
    }

    #[test]
    fn stems_ignore_unknown_ids() {
        let net = branched();
        let ids = ["NOPE", "P1"].map(String::from).to_vec();
        let stems = stems_from_pipes(&net, &ids);
        assert_eq!(stems.len(), 1);
        assert_eq!(stems[0].len(), 2);
    }

    #[test]
    fn profile_run_draws_selected_branch() {
        let (net, a) = branched_analyzed();
        let ids = ["PB1", "P2", "P3"].map(String::from).to_vec();
        let d = draw_profile_run(&net, &a, &DrawConfig::default(), &ids);
        let labels: Vec<&str> =
            d.profile_labels.iter().map(|l| l.text.as_str()).collect();
        assert!(labels.contains(&"B1"), "branch head missing: {labels:?}");
        assert!(labels.contains(&"OUT"));
        assert!(
            !labels.contains(&"N1"),
            "unselected trunk head leaked into the run: {labels:?}"
        );
        let roles: Vec<ProfileRole> =
            d.profile_lines.iter().map(|p| p.role).collect();
        assert!(roles.contains(&ProfileRole::Ground));
        assert!(roles.contains(&ProfileRole::Invert));
        assert!(roles.contains(&ProfileRole::Hgl));
        // Stations strictly increase along the chained run.
        let xs: Vec<f64> = d
            .profile_labels
            .iter()
            .map(|l| l.x)
            .collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "stations not monotone");
    }

    #[test]
    fn branched_accumulation_is_exact_at_the_junction() {
        // The trunk pipe below the junction must carry the sum of all
        // upstream C*A, at the intensity for its accumulated Tc.
        let (net, a) = branched_analyzed();
        let p2 = a.pipes.iter().find(|p| p.id == "P2").unwrap();
        // CA upstream of P2: N1 (1.0*0.70) + B1 (2.0*0.60) + N2 (1.0*0.70).
        let expected_ca = 1.0 * 0.70 + 2.0 * 0.60 + 1.0 * 0.70;
        assert!(
            (p2.total_ca - expected_ca).abs() < 1e-9,
            "CA at P2: {} vs {}",
            p2.total_ca,
            expected_ca
        );
        let i = 60.0 / (p2.tc + 10.0_f64).powf(0.8);
        assert!((p2.intensity - i).abs() < 1e-9, "intensity mismatch");
        assert!(
            (p2.design_q - expected_ca * i).abs() < 1e-9,
            "Q != CA*i at the junction"
        );
    }

    #[test]
    fn plan_has_one_line_per_pipe_and_marker_per_node() {
        let (net, a) = analyzed();
        let d = draw_network(&net, &a, &DrawConfig::default());
        assert_eq!(d.plan_pipes.len(), net.pipes.len());
        assert_eq!(d.plan_nodes.len(), net.nodes.len());
        assert!(d.plan_labels.len() >= net.pipes.len() + net.nodes.len());
    }

    #[test]
    fn main_stem_is_full_trunk_upstream_first() {
        let net = sample();
        let stem = main_stem(&net);
        let ids: Vec<&str> = stem.iter().map(|&i| net.nodes[i].id.as_str()).collect();
        assert_eq!(ids, vec!["N1", "N2", "N3", "OUT"]);
    }

    #[test]
    fn profile_has_ground_invert_and_hgl() {
        let (net, a) = analyzed();
        let d = draw_network(&net, &a, &DrawConfig::default());
        let roles: Vec<ProfileRole> = d.profile_lines.iter().map(|p| p.role).collect();
        assert!(roles.contains(&ProfileRole::Ground));
        assert!(roles.contains(&ProfileRole::Invert));
        assert!(roles.contains(&ProfileRole::Hgl));
        // Datum is the lowest invert on the stem (OUT = 100.0 in the sample).
        assert!((d.profile_datum - 100.0).abs() < 1e-9, "datum {}", d.profile_datum);
        for pl in &d.profile_lines {
            assert!(pl.pts.len() >= 2, "{:?} too short", pl.role);
        }
    }
}
