// SPDX-License-Identifier: GPL-3.0-or-later

//! Geometry for a SWMM profile (long-section) plot: the nodes and links of a
//! model reduced to inverts, crowns, rims, offsets and lengths, a path finder
//! between two nodes, and the station table along that path.
//!
//! Elevations follow the engine's own rules rather than the drawing's
//! convenience:
//!
//! - A link end's invert is the node invert plus the link's offset. With
//!   `LINK_OFFSETS ELEVATION` the offset column is an absolute elevation and
//!   is converted to a depth above the node invert (never below it).
//! - A node whose `MaxDepth` is zero or absent takes its rim from the highest
//!   crown among the links that connect to it, which is what SWMM does when
//!   it validates a junction.
//! - Stations accumulate conduit `Length`. Pumps, orifices, weirs and outlets
//!   have no length; they are given a short nominal span so the plot can show
//!   them without stacking two nodes on one station.

use std::collections::{HashMap, VecDeque};

use crate::doc::InpDoc;

/// Nominal station span for a link without a length, in model length units.
pub const NON_CONDUIT_SPAN: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Junction,
    Outfall,
    Storage,
    Divider,
}

impl NodeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Junction => "junction",
            Self::Outfall => "outfall",
            Self::Storage => "storage",
            Self::Divider => "divider",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Conduit,
    Pump,
    Orifice,
    Weir,
    Outlet,
}

impl LinkKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Conduit => "conduit",
            Self::Pump => "pump",
            Self::Orifice => "orifice",
            Self::Weir => "weir",
            Self::Outlet => "outlet",
        }
    }
}

/// How `[CONDUITS]` offsets are written, from `[OPTIONS] LINK_OFFSETS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OffsetMode {
    #[default]
    Depth,
    Elevation,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileNode {
    pub id: String,
    pub kind: NodeKind,
    pub invert: f64,
    /// `MaxDepth` as written; zero or absent means "highest connecting crown".
    pub max_depth: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileLink {
    pub id: String,
    pub kind: LinkKind,
    pub from: String,
    pub to: String,
    /// Conduit length; zero for every other kind.
    pub length: f64,
    /// Depth of the upstream invert above the `from` node invert.
    pub in_offset: f64,
    /// Depth of the downstream invert above the `to` node invert.
    pub out_offset: f64,
    /// `[XSECTIONS] Geom1`: the full depth of the section. Zero when absent.
    pub geom1: f64,
    pub shape: String,
    /// An orifice of `Type` BOTTOM. The engine does not raise its end nodes'
    /// full depth to its crown, as it does for every other link.
    pub bottom_orifice: bool,
}

/// The parts of a model a profile needs, in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileNetwork {
    pub nodes: Vec<ProfileNode>,
    pub links: Vec<ProfileLink>,
    pub offsets: OffsetMode,
    /// `FLOW_UNITS` as written, so a caller can label axes.
    pub flow_units: String,
}

fn num(s: Option<&str>) -> Option<f64> {
    s.and_then(|s| s.trim().parse::<f64>().ok())
}

impl ProfileNetwork {
    /// Length and elevation axes are metric when the flow units are.
    pub fn is_metric(&self) -> bool {
        matches!(
            self.flow_units.to_ascii_uppercase().as_str(),
            "CMS" | "LPS" | "MLD"
        )
    }

    pub fn length_unit(&self) -> &'static str {
        if self.is_metric() {
            "m"
        } else {
            "ft"
        }
    }

    /// Read the node and link sections of a parsed document.
    pub fn from_doc(doc: &InpDoc) -> Self {
        let offsets = match doc.option("LINK_OFFSETS").map(|s| s.to_ascii_uppercase()) {
            Some(s) if s == "ELEVATION" => OffsetMode::Elevation,
            _ => OffsetMode::Depth,
        };
        let flow_units = doc.option("FLOW_UNITS").unwrap_or("CFS").to_string();

        let mut nodes = Vec::new();
        for (section, kind) in [
            ("JUNCTIONS", NodeKind::Junction),
            ("OUTFALLS", NodeKind::Outfall),
            ("STORAGE", NodeKind::Storage),
            ("DIVIDERS", NodeKind::Divider),
        ] {
            for (_, row) in doc.rows(section) {
                let cols = doc.columns(section, row);
                let Some(id) = row.value(0) else { continue };
                let Some(invert) = num(row.get(cols, "Elevation")) else {
                    continue;
                };
                let max_depth = num(row.get(cols, "MaxDepth")).filter(|d| *d > 0.0);
                nodes.push(ProfileNode {
                    id: id.to_string(),
                    kind,
                    invert,
                    max_depth,
                });
            }
        }

        let inverts: HashMap<&str, f64> = nodes.iter().map(|n| (n.id.as_str(), n.invert)).collect();
        // An offset written as an elevation becomes a depth above the node
        // invert; "*" and anything unparsable is the invert itself.
        let to_depth = |raw: Option<&str>, node: &str| -> f64 {
            let Some(v) = num(raw) else { return 0.0 };
            match offsets {
                OffsetMode::Depth => v.max(0.0),
                OffsetMode::Elevation => inverts.get(node).map_or(0.0, |inv| (v - inv).max(0.0)),
            }
        };

        let mut links = Vec::new();
        for (section, kind, in_col, out_col) in [
            ("CONDUITS", LinkKind::Conduit, "InOffset", "OutOffset"),
            ("PUMPS", LinkKind::Pump, "", ""),
            ("ORIFICES", LinkKind::Orifice, "Offset", ""),
            ("WEIRS", LinkKind::Weir, "CrestHt", ""),
            ("OUTLETS", LinkKind::Outlet, "Offset", ""),
        ] {
            for (_, row) in doc.rows(section) {
                let cols = doc.columns(section, row);
                let (Some(id), Some(from), Some(to)) = (row.value(0), row.value(1), row.value(2))
                else {
                    continue;
                };
                let length = if kind == LinkKind::Conduit {
                    num(row.get(cols, "Length")).unwrap_or(0.0).max(0.0)
                } else {
                    0.0
                };
                let in_offset = if in_col.is_empty() {
                    0.0
                } else {
                    to_depth(row.get(cols, in_col), from)
                };
                let out_offset = if out_col.is_empty() {
                    // A weir or orifice sits at one height; its downstream end
                    // is drawn level with its crest so the barrel is flat.
                    match kind {
                        LinkKind::Conduit | LinkKind::Pump => 0.0,
                        _ => {
                            let up = inverts.get(from).copied().unwrap_or(0.0) + in_offset;
                            inverts.get(to).map_or(0.0, |inv| (up - inv).max(0.0))
                        }
                    }
                } else {
                    to_depth(row.get(cols, out_col), to)
                };
                let bottom_orifice = kind == LinkKind::Orifice
                    && row
                        .get(cols, "Type")
                        .is_some_and(|t| t.eq_ignore_ascii_case("BOTTOM"));
                links.push(ProfileLink {
                    id: id.to_string(),
                    kind,
                    from: from.to_string(),
                    to: to.to_string(),
                    length,
                    in_offset,
                    out_offset,
                    geom1: 0.0,
                    shape: String::new(),
                    bottom_orifice,
                });
            }
        }

        for (_, row) in doc.rows("XSECTIONS") {
            let cols = doc.columns("XSECTIONS", row);
            let Some(link) = row.value(0) else { continue };
            if let Some(l) = links.iter_mut().find(|l| l.id == link) {
                l.shape = row.get(cols, "Shape").unwrap_or("").to_string();
                l.geom1 = num(row.get(cols, "Geom1")).unwrap_or(0.0).max(0.0);
            }
        }

        Self {
            nodes,
            links,
            offsets,
            flow_units,
        }
    }

    pub fn node(&self, id: &str) -> Option<&ProfileNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn link(&self, id: &str) -> Option<&ProfileLink> {
        self.links.iter().find(|l| l.id == id)
    }

    /// Ground/rim elevation as the engine sees it: invert plus
    /// [`full_depth`](Self::full_depth).
    pub fn rim_of(&self, node: &ProfileNode) -> f64 {
        node.invert + self.full_depth(node)
    }

    /// The node's full depth exactly as EPA SWMM 5.2 sets it (`link.c`,
    /// `link_validate`): `MaxDepth`, raised to the crown of every link that
    /// meets the node, except at storage units. A link's upstream end counts
    /// unless it is a pump or a bottom orifice; its downstream end counts
    /// only for conduits. This is always a maximum, so a written `MaxDepth`
    /// below a pipe's crown is raised too, not only a zero one.
    ///
    /// Storage units with a surcharge depth are also raised by the engine;
    /// that column is not read here, so a storage unit keeps its `MaxDepth`.
    pub fn full_depth(&self, node: &ProfileNode) -> f64 {
        let mut full = node.max_depth.unwrap_or(0.0);
        if node.kind == NodeKind::Storage {
            return full;
        }
        for l in &self.links {
            if l.kind == LinkKind::Pump || l.bottom_orifice {
                continue;
            }
            if l.from == node.id {
                full = full.max(l.in_offset + l.geom1);
            }
            if l.to == node.id && l.kind == LinkKind::Conduit {
                full = full.max(l.out_offset + l.geom1);
            }
        }
        full
    }

    /// Links from `start` to `end` as `(link index, traversed forward)`.
    ///
    /// Breadth-first, so the path is the fewest links. Direction is honoured
    /// first: a profile normally follows the flow. When no directed path
    /// exists the search is repeated ignoring direction, which reaches a path
    /// through a link drawn against the flow rather than reporting nothing.
    pub fn find_path(&self, start: &str, end: &str) -> Result<Vec<(usize, bool)>, String> {
        if self.node(start).is_none() {
            return Err(format!("node {start:?} is not in the model"));
        }
        if self.node(end).is_none() {
            return Err(format!("node {end:?} is not in the model"));
        }
        if start == end {
            return Err("choose two different nodes".to_string());
        }
        if let Some(p) = self.bfs(start, end, true) {
            return Ok(p);
        }
        if let Some(p) = self.bfs(start, end, false) {
            return Ok(p);
        }
        Err(format!("no path of links joins {start} to {end}"))
    }

    fn bfs(&self, start: &str, end: &str, directed: bool) -> Option<Vec<(usize, bool)>> {
        // Adjacency: node -> (link index, forward, other node).
        let mut adj: HashMap<&str, Vec<(usize, bool, &str)>> = HashMap::new();
        for (i, l) in self.links.iter().enumerate() {
            adj.entry(l.from.as_str())
                .or_default()
                .push((i, true, l.to.as_str()));
            if !directed {
                adj.entry(l.to.as_str())
                    .or_default()
                    .push((i, false, l.from.as_str()));
            }
        }
        let mut prev: HashMap<&str, (usize, bool, &str)> = HashMap::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        seen.insert(start);
        while let Some(n) = queue.pop_front() {
            if n == end {
                let mut path = Vec::new();
                let mut cur = end;
                while cur != start {
                    let (i, fwd, from) = prev[cur];
                    path.push((i, fwd));
                    cur = from;
                }
                path.reverse();
                return Some(path);
            }
            for &(i, fwd, other) in adj.get(n).map(|v| v.as_slice()).unwrap_or(&[]) {
                if seen.insert(other) {
                    prev.insert(other, (i, fwd, n));
                    queue.push_back(other);
                }
            }
        }
        None
    }

    /// The station table along the path from `start` to `end`.
    pub fn build(&self, start: &str, end: &str) -> Result<Profile, String> {
        let path = self.find_path(start, end)?;
        let mut nodes = Vec::new();
        let mut links = Vec::new();
        let mut station = 0.0;
        let mut current = start.to_string();

        let push_node = |nodes: &mut Vec<ProfileStation>, id: &str, station: f64| {
            let n = self.node(id).expect("path nodes exist");
            nodes.push(ProfileStation {
                node: n.id.clone(),
                kind: n.kind,
                station,
                invert: n.invert,
                rim: self.rim_of(n),
            });
        };
        push_node(&mut nodes, &current, station);

        for (i, forward) in path {
            let l = &self.links[i];
            let (near, far, near_off, far_off) = if forward {
                (&l.from, &l.to, l.in_offset, l.out_offset)
            } else {
                (&l.to, &l.from, l.out_offset, l.in_offset)
            };
            debug_assert_eq!(near, &current);
            let up_invert = self.node(near).map_or(0.0, |n| n.invert) + near_off;
            let dn_invert = self.node(far).map_or(0.0, |n| n.invert) + far_off;
            let span = if l.kind == LinkKind::Conduit && l.length > 0.0 {
                l.length
            } else {
                NON_CONDUIT_SPAN
            };
            let from_station = station;
            station += span;
            links.push(ProfileSegment {
                link: l.id.clone(),
                kind: l.kind,
                forward,
                from_station,
                to_station: station,
                length: l.length,
                up_invert,
                dn_invert,
                up_crown: up_invert + l.geom1,
                dn_crown: dn_invert + l.geom1,
                depth: l.geom1,
                shape: l.shape.clone(),
            });
            current = far.clone();
            push_node(&mut nodes, &current, station);
        }

        Ok(Profile {
            nodes,
            links,
            metric: self.is_metric(),
        })
    }
}

/// A node on the profile.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileStation {
    pub node: String,
    pub kind: NodeKind,
    pub station: f64,
    pub invert: f64,
    /// Ground or rim elevation (see [`ProfileNetwork::rim_of`]).
    pub rim: f64,
}

/// A link on the profile, oriented along the path (up = nearer the start).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileSegment {
    pub link: String,
    pub kind: LinkKind,
    /// False when the path runs against the link's from→to direction.
    pub forward: bool,
    pub from_station: f64,
    pub to_station: f64,
    pub length: f64,
    pub up_invert: f64,
    pub dn_invert: f64,
    pub up_crown: f64,
    pub dn_crown: f64,
    /// Full depth of the section (`Geom1`).
    pub depth: f64,
    pub shape: String,
}

/// The station table for one path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub nodes: Vec<ProfileStation>,
    pub links: Vec<ProfileSegment>,
    pub metric: bool,
}

impl Profile {
    pub fn length(&self) -> f64 {
        self.nodes.last().map_or(0.0, |n| n.station)
    }

    /// Lowest and highest elevation on the profile, including the given HGL
    /// values, so a plot can frame everything.
    pub fn elevation_range(&self, extra: &[f64]) -> Option<(f64, f64)> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for n in &self.nodes {
            lo = lo.min(n.invert);
            hi = hi.max(n.rim);
        }
        for l in &self.links {
            lo = lo.min(l.up_invert).min(l.dn_invert);
            hi = hi.max(l.up_crown).max(l.dn_crown);
        }
        for &v in extra {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
    }

    /// Interpolated pipe invert and crown at `station`, taken from the segment
    /// covering it; `None` beyond the ends.
    pub fn pipe_at(&self, station: f64) -> Option<(f64, f64, &ProfileSegment)> {
        let seg = self
            .links
            .iter()
            .find(|l| station >= l.from_station && station <= l.to_station)?;
        let span = (seg.to_station - seg.from_station).max(1e-9);
        let t = ((station - seg.from_station) / span).clamp(0.0, 1.0);
        Some((
            seg.up_invert + (seg.dn_invert - seg.up_invert) * t,
            seg.up_crown + (seg.dn_crown - seg.up_crown) * t,
            seg,
        ))
    }

    /// Segments whose crown is below the HGL at either end (surcharged), given
    /// the HGL at each profile node in order.
    pub fn surcharged(&self, hgl: &[Option<f64>]) -> Vec<usize> {
        self.links
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                let up = hgl.get(*i).copied().flatten();
                let dn = hgl.get(i + 1).copied().flatten();
                up.is_some_and(|h| h > l.up_crown + 1e-9)
                    || dn.is_some_and(|h| h > l.dn_crown + 1e-9)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Nodes whose HGL is above the rim (flooding), same indexing.
    pub fn flooded(&self, hgl: &[Option<f64>]) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(i, n)| {
                hgl.get(*i)
                    .copied()
                    .flatten()
                    .is_some_and(|h| h > n.rim + 1e-9)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The station table as CSV. `hgl` and `max_hgl` are per profile node,
    /// in order, and may be shorter than the node list.
    pub fn to_csv(&self, hgl: &[Option<f64>], max_hgl: &[Option<f64>]) -> String {
        let unit = if self.metric { "m" } else { "ft" };
        let fmt = |v: Option<f64>| v.map(|v| format!("{v:.3}")).unwrap_or_default();
        let mut out = crate::rpt::csv_line(&[
            "node".to_string(),
            "kind".to_string(),
            format!("station_{unit}"),
            format!("invert_{unit}"),
            format!("rim_{unit}"),
            format!("hgl_{unit}"),
            format!("max_hgl_{unit}"),
            "link_in".to_string(),
            format!("link_in_invert_{unit}"),
            format!("link_in_crown_{unit}"),
            "link_out".to_string(),
            format!("link_out_invert_{unit}"),
            format!("link_out_crown_{unit}"),
        ]);
        for (i, n) in self.nodes.iter().enumerate() {
            let link_in = i.checked_sub(1).and_then(|j| self.links.get(j));
            let link_out = self.links.get(i);
            out.push_str(&crate::rpt::csv_line(&[
                n.node.clone(),
                n.kind.label().to_string(),
                format!("{:.3}", n.station),
                format!("{:.3}", n.invert),
                format!("{:.3}", n.rim),
                fmt(hgl.get(i).copied().flatten()),
                fmt(max_hgl.get(i).copied().flatten()),
                link_in.map(|l| l.link.clone()).unwrap_or_default(),
                fmt(link_in.map(|l| l.dn_invert)),
                fmt(link_in.map(|l| l.dn_crown)),
                link_out.map(|l| l.link.clone()).unwrap_or_default(),
                fmt(link_out.map(|l| l.up_invert)),
                fmt(link_out.map(|l| l.up_crown)),
            ]));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = "\
[OPTIONS]
FLOW_UNITS CMS
LINK_OFFSETS ELEVATION

[JUNCTIONS]
A 100 0 0 0 0
B 98 3 0 0 0

[OUTFALLS]
O 95 FREE

[CONDUITS]
AB A B 100 0.013 100.5 * 0 0
BO B O 50 0.013 98 95 0 0

[XSECTIONS]
AB CIRCULAR 1 0 0 0 1
BO CIRCULAR 1.5 0 0 0 1
";

    #[test]
    fn elevation_offsets_become_depths_and_rims_follow_crowns() {
        let doc = InpDoc::parse(MODEL);
        let net = ProfileNetwork::from_doc(&doc);
        assert_eq!(net.offsets, OffsetMode::Elevation);
        assert!(net.is_metric());
        let ab = net.link("AB").unwrap();
        assert!((ab.in_offset - 0.5).abs() < 1e-9, "100.5 above invert 100");
        assert_eq!(ab.out_offset, 0.0, "'*' is the node invert");
        // A's MaxDepth is 0: its rim is AB's crown, 100.5 + 1.0.
        let a = net.node("A").unwrap();
        assert!((net.rim_of(a) - 101.5).abs() < 1e-9);
        // B's MaxDepth (3) is above both crowns (1.0 and 1.5), so it stands.
        let b = net.node("B").unwrap();
        assert!((net.rim_of(b) - 101.0).abs() < 1e-9);
    }

    #[test]
    fn full_depth_follows_the_engine_rule() {
        // EPA link.c link_validate: MaxDepth is raised to every connecting
        // crown (always a maximum), except at storage; a link's downstream
        // end counts only for conduits; pumps and bottom orifices never.
        let doc = InpDoc::parse(
            "[JUNCTIONS]
A 100 0.5 0 0 0
B 99 0 0 0 0
C 98 0 0 0 0
             [STORAGE]
S 97 2 0 FUNCTIONAL 1000 0 0 0 0
             [CONDUITS]
AB A B 100 0.013 0 0.2 0 0
BS B S 100 0.013 0 0 0 0
             [PUMPS]
P1 S C P 1
             [ORIFICES]
OR1 C A BOTTOM 0 0.65 NO 0
OR2 A C SIDE 0.5 0.65 NO 0
             [XSECTIONS]
AB CIRCULAR 1 0 0 0 1
BS CIRCULAR 3 0 0 0 1
             OR1 CIRCULAR 5 0 0 0
OR2 RECT_CLOSED 0.6 1 0 0
",
        );
        let net = ProfileNetwork::from_doc(&doc);
        let fd = |id: &str| net.full_depth(net.node(id).unwrap());
        // A: MaxDepth 0.5 raised to AB's crown 1.0 and OR2's 0.5+0.6=1.1;
        // the bottom orifice OR1 (5 ft) is ignored at both ends.
        assert!((fd("A") - 1.1).abs() < 1e-9, "{}", fd("A"));
        // B: AB downstream end 0.2+1 = 1.2, BS upstream 3.
        assert!((fd("B") - 3.0).abs() < 1e-9);
        // S: storage keeps its MaxDepth despite BS's 3 ft crown.
        assert!((fd("S") - 2.0).abs() < 1e-9);
        // C: the pump's end, the bottom orifice's upstream end, and a side
        // orifice's DOWNSTREAM end all count for nothing.
        assert_eq!(fd("C"), 0.0);
    }

    #[test]
    fn profile_stations_accumulate_length() {
        let doc = InpDoc::parse(MODEL);
        let net = ProfileNetwork::from_doc(&doc);
        let p = net.build("A", "O").unwrap();
        let st: Vec<f64> = p.nodes.iter().map(|n| n.station).collect();
        assert_eq!(st, vec![0.0, 100.0, 150.0]);
        assert_eq!(p.links[1].up_crown, 98.0 + 1.5);
        assert!(p.to_csv(&[], &[]).starts_with("node,kind,station_m,"));
    }

    #[test]
    fn undirected_fallback_and_missing_paths() {
        let doc = InpDoc::parse(MODEL);
        let net = ProfileNetwork::from_doc(&doc);
        let back = net.find_path("O", "A").unwrap();
        assert_eq!(back, vec![(1, false), (0, false)]);
        assert!(net.find_path("A", "A").is_err());
        assert!(net.find_path("A", "nope").unwrap_err().contains("nope"));
        let p = net.build("O", "A").unwrap();
        assert!(!p.links[0].forward);
        // Reversed segment: the near end is O's end of BO (invert 95).
        assert_eq!(p.links[0].up_invert, 95.0);
    }
}
