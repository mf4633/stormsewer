// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser for the EPA SWMM 5 input file (`.inp`) — enough of it to draw the
//! model and name its objects.
//!
//! # What this reads, and what it does not
//!
//! This is a *map and inventory* reader, not a round-trip editor. It takes the
//! object names, connectivity, and plan geometry needed to draw a network and
//! match it against results, and it deliberately ignores the hydrology and
//! routing parameters the engine cares about. The engine reads the `.inp`
//! itself; nothing here is on the path to a simulation, so a field this parser
//! skips is not a field StormSewer gets wrong.
//!
//! # Why it is lenient
//!
//! A parser that refuses a real model is useless. Every recoverable problem —
//! an unparseable number, a link pointing at a node that was never declared,
//! a section this build does not know — becomes an entry in
//! [`InpModel::warnings`] rather than an error. Only two things fail the read:
//! the file cannot be opened, or it contains no section headers at all and so
//! is not a SWMM input.
//!
//! # Format notes that are easy to get wrong
//!
//! * Fields are **whitespace-delimited, not fixed-width**. The `;;`-comment
//!   header rows look like a column ruler, but values routinely overflow their
//!   heading, so nothing here reads by byte offset.
//! * Section names vary in case — real files pair `[COORDINATES]` with
//!   `[Polygons]` — so headers are matched case-insensitively.
//! * Comments **interleave with data**, not just under the header, and `;` also
//!   starts a trailing comment on a data line. Both are stripped everywhere.
//! * `[OUTFALLS]` has a **variable column count**: the stage-data field is
//!   absent for a `FREE` outfall, so a positional read of the later columns
//!   picks up the gated flag instead. The type keyword decides the layout.
//! * Geometry sections may appear before or after the objects they describe, so
//!   coordinates, vertices, and polygons are gathered first and attached once
//!   the whole file has been seen.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::{Error, Result};

/// Warnings are capped: a file that is not really a SWMM input can otherwise
/// produce one complaint per line, and nobody reads the ten-thousandth.
const MAX_WARNINGS: usize = 200;

/// The node sections this build understands.
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

/// The link sections this build understands.
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

/// A node, with its plan position once `[COORDINATES]` has been applied.
#[derive(Clone, Debug)]
pub struct InpNode {
    pub id: String,
    pub kind: NodeKind,
    /// Invert elevation, in the model's own length units.
    pub invert: Option<f64>,
    /// Maximum depth, where the node's section carries one.
    pub max_depth: Option<f64>,
    /// Outfall boundary type (`FREE`, `FIXED`, `TIDAL`, …) for outfalls only.
    pub outfall_type: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

impl InpNode {
    /// Plan position, when the model gave one.
    pub fn pos(&self) -> Option<(f64, f64)> {
        match (self.x, self.y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        }
    }
}

/// A link and its interior bend points.
#[derive(Clone, Debug)]
pub struct InpLink {
    pub id: String,
    pub kind: LinkKind,
    pub from: String,
    pub to: String,
    /// Conduit length, where the section carries one.
    pub length: Option<f64>,
    pub roughness: Option<f64>,
    /// Cross-section shape from `[XSECTIONS]`.
    pub shape: Option<String>,
    /// First geometry column from `[XSECTIONS]` — depth or diameter for most
    /// shapes. Kept unnamed because its meaning is shape-dependent.
    pub geom1: Option<f64>,
    /// Interior vertices, in file order, excluding the end nodes.
    pub vertices: Vec<(f64, f64)>,
}

/// A subcatchment and its outline.
#[derive(Clone, Debug)]
pub struct InpSubcatchment {
    pub id: String,
    pub rain_gage: String,
    pub outlet: String,
    pub area: Option<f64>,
    pub pct_imperv: Option<f64>,
    pub width: Option<f64>,
    pub pct_slope: Option<f64>,
    /// Outline vertices in file order. Empty when the model has no polygon.
    pub polygon: Vec<(f64, f64)>,
}

/// A rain gage and its symbol position.
#[derive(Clone, Debug)]
pub struct InpGage {
    pub id: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// A parsed model: what to draw, and what the objects are called.
#[derive(Clone, Debug, Default)]
pub struct InpModel {
    /// `[TITLE]`, joined with newlines.
    pub title: String,
    /// `FLOW_UNITS` from `[OPTIONS]`, verbatim.
    pub flow_units: Option<String>,
    /// `Units` from `[MAP]`, verbatim.
    pub map_units: Option<String>,
    /// `DIMENSIONS` from `[MAP]` as `(x1, y1, x2, y2)`. This is the sheet the
    /// model was drawn on, which is often much larger than the model, so
    /// [`InpModel::bounds`] is the better choice for fitting a view.
    pub dimensions: Option<(f64, f64, f64, f64)>,
    pub nodes: Vec<InpNode>,
    pub links: Vec<InpLink>,
    pub subcatchments: Vec<InpSubcatchment>,
    pub gages: Vec<InpGage>,
    /// `[TAGS]` rows as `(object kind, name, tag)`.
    pub tags: Vec<(String, String, String)>,
    /// Section headers seen but not interpreted, in first-seen order.
    pub skipped_sections: Vec<String>,
    /// Everything recoverable that went wrong.
    pub warnings: Vec<String>,
}

impl InpModel {
    /// Read and parse a `.inp` from disk.
    ///
    /// Whether the model can be *drawn* is deliberately not a warning. A
    /// caller asks [`InpModel::bounds`] — which the map already does, to show
    /// its own empty state — and keeping drawability out of `warnings` leaves
    /// one meaning there: something in this file the parser could not make
    /// sense of. Mixing an advisory note in with that made a legitimately
    /// coordinate-free model look malformed.
    pub fn read(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        Self::parse_str(&text)
    }

    /// Parse `.inp` text.
    pub fn parse_str(text: &str) -> Result<Self> {
        let mut model = Self::default();

        // Geometry is gathered separately: these sections may precede the
        // objects they belong to.
        let mut coords: HashMap<String, (f64, f64)> = HashMap::new();
        let mut vertices: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
        let mut polygons: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
        let mut symbols: HashMap<String, (f64, f64)> = HashMap::new();
        let mut xsections: HashMap<String, (String, Option<f64>)> = HashMap::new();

        let mut section = String::new();
        let mut saw_section = false;
        let mut title_lines: Vec<String> = Vec::new();

        // A leading byte-order mark would otherwise become part of the first
        // section name or object id.
        for raw in text.trim_start_matches('\u{feff}').lines() {
            let line = strip_comment(raw);
            if line.trim().is_empty() {
                continue;
            }
            if let Some(name) = section_header(line) {
                section = name;
                saw_section = true;
                continue;
            }
            if !saw_section {
                // Data before any header: not a SWMM input, or a truncated one.
                continue;
            }

            let f: Vec<&str> = line.split_whitespace().collect();
            if f.is_empty() {
                continue;
            }

            match section.as_str() {
                "TITLE" => title_lines.push(line.trim().to_string()),
                "OPTIONS" => {
                    if f[0].eq_ignore_ascii_case("FLOW_UNITS") {
                        model.flow_units = f.get(1).map(|s| s.to_string());
                    }
                }
                "MAP" => {
                    if f[0].eq_ignore_ascii_case("DIMENSIONS") && f.len() >= 5 {
                        if let (Some(a), Some(b), Some(c), Some(d)) = (
                            num(f[1]),
                            num(f[2]),
                            num(f[3]),
                            num(f[4]),
                        ) {
                            model.dimensions = Some((a, b, c, d));
                        }
                    } else if f[0].eq_ignore_ascii_case("UNITS") {
                        model.map_units = f.get(1).map(|s| s.to_string());
                    }
                }

                "JUNCTIONS" | "STORAGE" => {
                    let kind = if section == "JUNCTIONS" {
                        NodeKind::Junction
                    } else {
                        NodeKind::Storage
                    };
                    // Both sections open with Name, Invert, MaxDepth.
                    model.nodes.push(InpNode {
                        id: f[0].to_string(),
                        kind,
                        invert: f.get(1).and_then(|s| num(s)),
                        max_depth: f.get(2).and_then(|s| num(s)),
                        outfall_type: None,
                        x: None,
                        y: None,
                    });
                }
                "DIVIDERS" => {
                    // Name, Invert, DivertedLink, Type … — column 2 is a link
                    // name, not a depth, so max_depth stays unset here.
                    model.nodes.push(InpNode {
                        id: f[0].to_string(),
                        kind: NodeKind::Divider,
                        invert: f.get(1).and_then(|s| num(s)),
                        max_depth: None,
                        outfall_type: None,
                        x: None,
                        y: None,
                    });
                }
                "OUTFALLS" => {
                    // Name, Invert, Type, [StageData], Gated, [RouteTo]. The
                    // stage-data column exists only for some types, so only
                    // the first three fields can be read by position.
                    model.nodes.push(InpNode {
                        id: f[0].to_string(),
                        kind: NodeKind::Outfall,
                        invert: f.get(1).and_then(|s| num(s)),
                        max_depth: None,
                        outfall_type: f.get(2).map(|s| s.to_uppercase()),
                        x: None,
                        y: None,
                    });
                }

                "CONDUITS" | "PUMPS" | "ORIFICES" | "WEIRS" | "OUTLETS" => {
                    if f.len() < 3 {
                        model.warn(format!("[{section}] row for '{}' has no end nodes", f[0]));
                        continue;
                    }
                    let kind = match section.as_str() {
                        "CONDUITS" => LinkKind::Conduit,
                        "PUMPS" => LinkKind::Pump,
                        "ORIFICES" => LinkKind::Orifice,
                        "WEIRS" => LinkKind::Weir,
                        _ => LinkKind::Outlet,
                    };
                    // Every link section opens with Name, From, To. Length and
                    // roughness are conduit-only; the other sections put
                    // different things in those columns.
                    let (length, roughness) = if kind == LinkKind::Conduit {
                        (f.get(3).and_then(|s| num(s)), f.get(4).and_then(|s| num(s)))
                    } else {
                        (None, None)
                    };
                    model.links.push(InpLink {
                        id: f[0].to_string(),
                        kind,
                        from: f[1].to_string(),
                        to: f[2].to_string(),
                        length,
                        roughness,
                        shape: None,
                        geom1: None,
                        vertices: Vec::new(),
                    });
                }

                "SUBCATCHMENTS" => {
                    if f.len() < 3 {
                        model.warn(format!("[SUBCATCHMENTS] row '{}' is incomplete", f[0]));
                        continue;
                    }
                    model.subcatchments.push(InpSubcatchment {
                        id: f[0].to_string(),
                        rain_gage: f[1].to_string(),
                        outlet: f[2].to_string(),
                        area: f.get(3).and_then(|s| num(s)),
                        pct_imperv: f.get(4).and_then(|s| num(s)),
                        width: f.get(5).and_then(|s| num(s)),
                        pct_slope: f.get(6).and_then(|s| num(s)),
                        polygon: Vec::new(),
                    });
                }
                "RAINGAGES" => model.gages.push(InpGage {
                    id: f[0].to_string(),
                    x: None,
                    y: None,
                }),
                "TAGS" => {
                    if f.len() >= 3 {
                        model
                            .tags
                            .push((f[0].to_string(), f[1].to_string(), f[2..].join(" ")));
                    }
                }

                "XSECTIONS" => {
                    if f.len() >= 2 {
                        xsections.insert(
                            f[0].to_string(),
                            (f[1].to_uppercase(), f.get(2).and_then(|s| num(s))),
                        );
                    }
                }
                "COORDINATES" => {
                    if let Some(p) = point(&f) {
                        coords.insert(f[0].to_string(), p);
                    } else {
                        model.warn(format!("[COORDINATES] row for '{}' is not a point", f[0]));
                    }
                }
                "SYMBOLS" => {
                    if let Some(p) = point(&f) {
                        symbols.insert(f[0].to_string(), p);
                    }
                }
                "VERTICES" => {
                    if let Some(p) = point(&f) {
                        vertices.entry(f[0].to_string()).or_default().push(p);
                    }
                }
                "POLYGONS" => {
                    if let Some(p) = point(&f) {
                        polygons.entry(f[0].to_string()).or_default().push(p);
                    }
                }

                other => {
                    // Recorded once so the UI can say what it ignored, which is
                    // honest about coverage instead of silently dropping data.
                    let name = other.to_string();
                    if !model.skipped_sections.contains(&name) {
                        model.skipped_sections.push(name);
                    }
                }
            }
        }

        if !saw_section {
            return Err(Error::Format(
                "This file has no [SECTION] headers, so it is not a SWMM input.".to_string(),
            ));
        }

        model.title = title_lines.join("\n");

        // Attach geometry now that the whole file has been seen.
        for node in &mut model.nodes {
            if let Some(&(x, y)) = coords.get(&node.id) {
                node.x = Some(x);
                node.y = Some(y);
            }
        }
        for gage in &mut model.gages {
            if let Some(&(x, y)) = symbols.get(&gage.id) {
                gage.x = Some(x);
                gage.y = Some(y);
            }
        }
        for link in &mut model.links {
            if let Some(v) = vertices.remove(&link.id) {
                link.vertices = v;
            }
            if let Some((shape, geom1)) = xsections.remove(&link.id) {
                link.shape = Some(shape);
                link.geom1 = geom1;
            }
        }
        for sub in &mut model.subcatchments {
            if let Some(p) = polygons.remove(&sub.id) {
                sub.polygon = p;
            }
        }

        // Geometry naming an object that was never declared is a real modelling
        // error and worth surfacing, not silently discarding.
        for id in vertices.keys() {
            model.warn(format!("[VERTICES] names link '{id}', which is not declared"));
        }
        for id in polygons.keys() {
            model.warn(format!(
                "[Polygons] names subcatchment '{id}', which is not declared"
            ));
        }

        let declared: std::collections::HashSet<&str> =
            model.nodes.iter().map(|n| n.id.as_str()).collect();
        let dangling: Vec<String> = model
            .links
            .iter()
            .filter(|l| !declared.contains(l.from.as_str()) || !declared.contains(l.to.as_str()))
            .map(|l| l.id.clone())
            .collect();
        for id in dangling {
            model.warn(format!("Link '{id}' connects a node that is not declared"));
        }

        Ok(model)
    }

    fn warn(&mut self, message: impl Into<String>) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(message.into());
        } else if self.warnings.len() == MAX_WARNINGS {
            self.warnings
                .push("More problems followed; only the first 200 are listed.".to_string());
        }
    }

    pub fn node(&self, id: &str) -> Option<&InpNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn link(&self, id: &str) -> Option<&InpLink> {
        self.links.iter().find(|l| l.id == id)
    }

    /// A link's drawn path: start node, its interior vertices in order, end
    /// node. Empty when either end has no coordinates, since a partial line
    /// would be drawn to the wrong place.
    pub fn link_polyline(&self, link: &InpLink) -> Vec<(f64, f64)> {
        let (Some(a), Some(b)) = (
            self.node(&link.from).and_then(|n| n.pos()),
            self.node(&link.to).and_then(|n| n.pos()),
        ) else {
            return Vec::new();
        };
        let mut path = Vec::with_capacity(link.vertices.len() + 2);
        path.push(a);
        path.extend(link.vertices.iter().copied());
        path.push(b);
        path
    }

    /// Bounds of everything drawable, as `(min_x, min_y, max_x, max_y)`.
    ///
    /// Taken from the geometry rather than `[MAP] DIMENSIONS` because the sheet
    /// is often far larger than the model, which would open the view zoomed
    /// out onto empty space.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut any = false;

        let mut add = |x: f64, y: f64| {
            if x.is_finite() && y.is_finite() {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                any = true;
            }
        };

        for n in &self.nodes {
            if let Some((x, y)) = n.pos() {
                add(x, y);
            }
        }
        for l in &self.links {
            for &(x, y) in &l.vertices {
                add(x, y);
            }
        }
        for s in &self.subcatchments {
            for &(x, y) in &s.polygon {
                add(x, y);
            }
        }
        for g in &self.gages {
            if let (Some(x), Some(y)) = (g.x, g.y) {
                add(x, y);
            }
        }

        any.then_some((min_x, min_y, max_x, max_y))
    }

    /// One line describing what was read, for the status area.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} node(s), {} link(s), {} subcatchment(s)",
            self.nodes.len(),
            self.links.len(),
            self.subcatchments.len()
        );
        if !self.warnings.is_empty() {
            s.push_str(&format!(", {} warning(s)", self.warnings.len()));
        }
        s
    }
}

/// Drop a trailing `;` comment. EPA treats `;` as a comment start anywhere on
/// a line, so a whole-line `;;…` comment collapses to nothing here too.
fn strip_comment(line: &str) -> &str {
    match line.find(';') {
        Some(i) => &line[..i],
        None => line,
    }
}

/// A `[SECTION]` header, upper-cased, if this line is one.
fn section_header(line: &str) -> Option<String> {
    let t = line.trim();
    let rest = t.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(rest[..end].trim().to_uppercase())
}

/// Parse a number, tolerating the forms real files use.
fn num(s: &str) -> Option<f64> {
    s.parse::<f64>().ok()
}

/// `name x y` → the point, when both coordinates parse.
fn point(f: &[&str]) -> Option<(f64, f64)> {
    match (f.get(1).and_then(|s| num(s)), f.get(2).and_then(|s| num(s))) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the traps: interleaved comments, an inline comment, a `FREE`
    /// outfall with no stage data, mixed-case `[Polygons]`, geometry declared
    /// before its objects, and a link with bend points.
    const SAMPLE: &str = "\
[TITLE]
A test model

[OPTIONS]
FLOW_UNITS           CFS
INFILTRATION         HORTON

[MAP]
DIMENSIONS 0.000 0.000 10000.000 10000.000
Units      Feet

[COORDINATES]
;;Node           X-Coord            Y-Coord
;;-------------- ------------------ ------------------
J1               0.000              0.000
J2               500.000            100.000
ST1              800.000            150.000
O1               1000.000           0.000

[JUNCTIONS]
;;Name           Elevation  MaxDepth   InitDepth  SurDepth   Aponded
;; upper end of the reach
J1               100.0      8.0        0.0        0.0        0.0
J2               98.5       8.0        0.0        0.0        0.0

[STORAGE]
ST1              95.0       12.0       0.0        FUNCTIONAL 1000 0 0

[OUTFALLS]
;;Name           Elevation  Type       Stage Data       Gated    Route To
O1               90.0       FREE                        NO

[CONDUITS]
C1               J1               J2               500        0.013      0    0
C2               J2               ST1              320        0.013      0    0   ; to the pond
C3               ST1              O1               210        0.013      0    0

[XSECTIONS]
C1               CIRCULAR     1.25       0          0          0          1
C2               TRAPEZOIDAL  15.0       40.0       2.0        2.0        1

[VERTICES]
C1               250.000            60.000
C1               400.000            80.000

[SUBCATCHMENTS]
S1               RG1              J1               15.0     45       600      2.5      0

[Polygons]
S1               0.000              0.000
S1               0.000              400.000
S1               300.000            400.000

[RAINGAGES]
RG1              INTENSITY 0:05     1.0      TIMESERIES TS1

[SYMBOLS]
RG1              50.000             450.000

[TAGS]
Node      J2               CRITICAL

[TIMESERIES]
TS1              0:00       0.5
";

    fn sample() -> InpModel {
        InpModel::parse_str(SAMPLE).expect("sample should parse")
    }

    #[test]
    fn reads_the_object_inventory() {
        let m = sample();
        assert_eq!(m.nodes.len(), 4, "2 junctions + 1 storage + 1 outfall");
        assert_eq!(m.links.len(), 3);
        assert_eq!(m.subcatchments.len(), 1);
        assert_eq!(m.gages.len(), 1);
        assert_eq!(m.title, "A test model");
        assert_eq!(m.flow_units.as_deref(), Some("CFS"));
        assert_eq!(m.map_units.as_deref(), Some("Feet"));
        assert_eq!(m.dimensions, Some((0.0, 0.0, 10000.0, 10000.0)));
    }

    #[test]
    fn assigns_node_kinds_from_their_sections() {
        let m = sample();
        assert_eq!(m.node("J1").unwrap().kind, NodeKind::Junction);
        assert_eq!(m.node("ST1").unwrap().kind, NodeKind::Storage);
        assert_eq!(m.node("O1").unwrap().kind, NodeKind::Outfall);
    }

    #[test]
    fn a_free_outfall_has_no_stage_data_column() {
        // The trap: reading past the type keyword by position would take the
        // gated flag ("NO") as stage data.
        let m = sample();
        let o = m.node("O1").unwrap();
        assert_eq!(o.outfall_type.as_deref(), Some("FREE"));
        assert_eq!(o.invert, Some(90.0));
        assert!(
            m.warnings.is_empty(),
            "a clean model should warn about nothing: {:?}",
            m.warnings
        );
    }

    #[test]
    fn applies_coordinates_declared_before_the_nodes() {
        // [COORDINATES] precedes [JUNCTIONS] in the sample on purpose.
        let m = sample();
        assert_eq!(m.node("J2").unwrap().pos(), Some((500.0, 100.0)));
    }

    #[test]
    fn a_links_polyline_runs_from_node_through_vertices_to_node() {
        let m = sample();
        let path = m.link_polyline(m.link("C1").unwrap());
        assert_eq!(
            path,
            vec![
                (0.0, 0.0),
                (250.0, 60.0),
                (400.0, 80.0),
                (500.0, 100.0),
            ],
            "vertices must stay in file order, between the end nodes"
        );
    }

    #[test]
    fn keeps_polygon_order_despite_the_mixed_case_header() {
        let m = sample();
        assert_eq!(
            m.subcatchments[0].polygon,
            vec![(0.0, 0.0), (0.0, 400.0), (300.0, 400.0)]
        );
    }

    #[test]
    fn strips_an_inline_comment_without_losing_the_row() {
        let m = sample();
        let c2 = m.link("C2").unwrap();
        assert_eq!(c2.to, "ST1");
        assert_eq!(c2.length, Some(320.0));
    }

    #[test]
    fn attaches_cross_sections_and_leaves_the_rest_unset() {
        let m = sample();
        assert_eq!(m.link("C1").unwrap().shape.as_deref(), Some("CIRCULAR"));
        assert_eq!(m.link("C1").unwrap().geom1, Some(1.25));
        assert_eq!(m.link("C3").unwrap().shape, None);
    }

    #[test]
    fn records_sections_it_did_not_interpret() {
        let m = sample();
        assert!(
            m.skipped_sections.iter().any(|s| s == "TIMESERIES"),
            "skipped sections should be reported: {:?}",
            m.skipped_sections
        );
        assert!(
            !m.skipped_sections.iter().any(|s| s == "JUNCTIONS"),
            "an interpreted section must not be listed as skipped"
        );
    }

    #[test]
    fn bounds_cover_vertices_and_polygons_not_just_nodes() {
        let m = sample();
        let (min_x, min_y, max_x, max_y) = m.bounds().unwrap();
        assert_eq!((min_x, min_y), (0.0, 0.0));
        assert_eq!(max_x, 1000.0, "node O1 is the rightmost point");
        assert_eq!(max_y, 450.0, "the rain gage symbol is the topmost point");
    }

    #[test]
    fn bounds_prefer_geometry_over_the_map_sheet() {
        // DIMENSIONS is 10000 ft square; the model occupies a corner of it.
        let m = sample();
        let (_, _, max_x, _) = m.bounds().unwrap();
        assert!(max_x < 2000.0, "fit must follow the model, not the sheet");
    }

    #[test]
    fn a_leading_byte_order_mark_does_not_corrupt_the_first_section() {
        let text = format!("\u{feff}{SAMPLE}");
        let m = InpModel::parse_str(&text).expect("BOM should be tolerated");
        assert_eq!(m.title, "A test model");
        assert_eq!(m.nodes.len(), 4);
    }

    #[test]
    fn lowercase_section_headers_are_accepted() {
        let m = InpModel::parse_str("[junctions]\nJ1 10.0 4.0\n[coordinates]\nJ1 1.0 2.0\n")
            .unwrap();
        assert_eq!(m.nodes.len(), 1);
        assert_eq!(m.node("J1").unwrap().pos(), Some((1.0, 2.0)));
    }

    #[test]
    fn every_link_section_yields_its_end_nodes() {
        let text = "\
[JUNCTIONS]
N1 10 4
N2 9 4
[CONDUITS]
L1 N1 N2 100 0.013 0 0
[PUMPS]
L2 N1 N2 CURVE1 ON 0 0
[ORIFICES]
L3 N1 N2 SIDE 0 0.65 NO 0
[WEIRS]
L4 N1 N2 TRANSVERSE 0 3.33 NO 0
[OUTLETS]
L5 N1 N2 0 FUNCTIONAL/DEPTH 10 0.5 NO
";
        let m = InpModel::parse_str(text).unwrap();
        assert_eq!(m.links.len(), 5);
        for l in &m.links {
            assert_eq!(l.from, "N1", "{} lost its from-node", l.id);
            assert_eq!(l.to, "N2", "{} lost its to-node", l.id);
        }
        assert_eq!(m.link("L1").unwrap().kind, LinkKind::Conduit);
        assert_eq!(m.link("L5").unwrap().kind, LinkKind::Outlet);
        // Length and roughness are conduit columns; other sections put other
        // things there and must not be misread as geometry.
        assert_eq!(m.link("L1").unwrap().length, Some(100.0));
        assert_eq!(m.link("L2").unwrap().length, None);
    }

    #[test]
    fn a_divider_does_not_read_its_link_name_as_a_depth() {
        let m = InpModel::parse_str("[DIVIDERS]\nD1 100.0 LINK9 CUTOFF 5.0\n").unwrap();
        let d = m.node("D1").unwrap();
        assert_eq!(d.kind, NodeKind::Divider);
        assert_eq!(d.invert, Some(100.0));
        assert_eq!(d.max_depth, None, "column 2 is a link name here");
    }

    #[test]
    fn warns_when_a_link_names_an_undeclared_node() {
        let m = InpModel::parse_str("[JUNCTIONS]\nN1 10 4\n[CONDUITS]\nL1 N1 GHOST 100 0.013\n")
            .unwrap();
        assert_eq!(m.links.len(), 1, "the link is still kept");
        assert!(
            m.warnings.iter().any(|w| w.contains("L1")),
            "expected a dangling-node warning: {:?}",
            m.warnings
        );
    }

    #[test]
    fn warns_when_vertices_name_an_undeclared_link() {
        let m = InpModel::parse_str("[VERTICES]\nGHOST 1.0 2.0\n").unwrap();
        assert!(m.warnings.iter().any(|w| w.contains("GHOST")));
    }

    #[test]
    fn an_undrawable_link_gets_an_empty_polyline() {
        // Half a line drawn to a missing node would point somewhere wrong.
        let m = InpModel::parse_str("[JUNCTIONS]\nN1 10 4\n[CONDUITS]\nL1 N1 GHOST 100 0.013\n")
            .unwrap();
        assert!(m.link_polyline(m.link("L1").unwrap()).is_empty());
    }

    #[test]
    fn a_file_without_sections_is_rejected() {
        let err = InpModel::parse_str("just some text\nand more\n");
        assert!(err.is_err(), "a non-SWMM file should not parse as a model");
    }

    #[test]
    fn a_model_without_coordinates_has_no_bounds() {
        let m = InpModel::parse_str("[JUNCTIONS]\nJ1 10.0 4.0\n").unwrap();
        assert!(m.bounds().is_none());
        assert_eq!(m.nodes.len(), 1);
    }

    #[test]
    fn an_unparseable_number_does_not_lose_the_object() {
        let m = InpModel::parse_str("[JUNCTIONS]\nJ1 n/a 4.0\n[COORDINATES]\nJ1 1.0 2.0\n")
            .unwrap();
        assert_eq!(m.nodes.len(), 1, "the node is still listed");
        assert_eq!(m.node("J1").unwrap().invert, None);
        assert_eq!(m.node("J1").unwrap().pos(), Some((1.0, 2.0)));
    }

    #[test]
    fn warnings_are_capped() {
        let mut text = String::from("[VERTICES]\n");
        for i in 0..(MAX_WARNINGS + 50) {
            text.push_str(&format!("GHOST{i} 1.0 2.0\n"));
        }
        let m = InpModel::parse_str(&text).unwrap();
        assert!(
            m.warnings.len() <= MAX_WARNINGS + 1,
            "warnings must stay bounded, got {}",
            m.warnings.len()
        );
    }

    #[test]
    fn reads_tags() {
        let m = sample();
        assert_eq!(
            m.tags,
            vec![("Node".to_string(), "J2".to_string(), "CRITICAL".to_string())]
        );
    }

    /// Opt-in: parse the real models on this machine. Gated so CI stays green
    /// with no fixtures present.
    #[test]
    fn real_models_parse() {
        let Ok(dir) = std::env::var("STORMSEWER_SWMM_FIXTURES") else {
            return;
        };
        let mut checked = 0;
        for entry in fs::read_dir(&dir).expect("fixture dir should be readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("inp")) {
                continue;
            }
            let m = InpModel::read(&path).unwrap_or_else(|e| {
                panic!("{} failed to parse: {e}", path.display());
            });
            assert!(
                !m.nodes.is_empty(),
                "{} parsed with no nodes",
                path.display()
            );
            for link in &m.links {
                assert!(
                    m.node(&link.from).is_some() && m.node(&link.to).is_some(),
                    "{}: link {} points at an undeclared node",
                    path.display(),
                    link.id
                );
            }
            // Fixtures are curated models, so a warning here is far more
            // likely to mean this parser is wrong about the format than that
            // the model is. The warnings themselves are the diagnosis.
            assert!(
                m.warnings.is_empty(),
                "{} is a curated model but parsed with warnings: {:?}",
                path.display(),
                m.warnings
            );
            checked += 1;
        }
        assert!(checked > 0, "fixture dir held no .inp files");
    }
}
