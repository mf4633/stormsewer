// SPDX-License-Identifier: GPL-3.0-or-later

//! Storm-sewer design on a SWMM model: the mapping between an [`InpDoc`] and
//! the engine's [`Project`] / `Network`, in both directions, and the
//! `[XSECTIONS]` batch that writes auto-sized conduits back.
//!
//! Everything here is pure: nothing touches the file system or applies a
//! command. The app maps, analyses with the engine, and applies the batch.
//!
//! # SWMM → storm-sewer (`to_project`)
//!
//! * **Units.** `FLOW_UNITS` `CFS`, `GPM` or `MGD` mean feet, acres and
//!   in/hr, which is what the engine computes in. `CMS`, `LPS` and `MLD`
//!   are refused: the engine's results are U.S. customary only, and a silent
//!   conversion would still report feet and cfs for a metric model.
//! * **Nodes.** `[JUNCTIONS]` become junctions (inlets once a subcatchment
//!   or an `[INLET_USAGE]` row drains to them), `[OUTFALLS]` outfalls,
//!   `[STORAGE]` and `[DIVIDERS]` junctions with a note — the engine routes
//!   peaks, it does not store or divert. Invert is `Elevation`; rim is
//!   `Elevation + MaxDepth`, or the highest connecting crown when `MaxDepth`
//!   is zero, exactly as the engine's own profile does.
//! * **Conduits.** `CIRCULAR` (and `FORCE_MAIN`, `FILLED_CIRCULAR`, by
//!   their diameter) map to circular pipes, `RECT_CLOSED` to boxes
//!   (`Geom1` rise × `Geom2` span), `HORIZ_ELLIPSE` / `VERT_ELLIPSE` to
//!   elliptical (`Geom1` rise × `Geom2` span), `ARCH` to arch. Every other
//!   shape, and every pump, orifice, weir and outlet, is listed in
//!   [`Mapping::skipped`] with the reason — never dropped silently. Length
//!   and Manning's n come from `[CONDUITS]`; pipe end inverts are the node
//!   invert plus the offset, honouring `LINK_OFFSETS`.
//! * **Subcatchments.** Fold into the node they drain to (through other
//!   subcatchments when `Outlet` names one): area in acres, a runoff
//!   coefficient from `%Imperv` by the documented straight line
//!   `C = 0.20 + 0.75·(%Imperv/100)` (0.20 pervious, 0.95 impervious —
//!   the usual Rational-table band for lawns and pavement), area-weighted
//!   when several drain to one node, and an inlet time by Kirpich over the
//!   overland flow length `Area / Width` at `%Slope`, the largest of them
//!   kept. The engine floors the inlet time at the design minimum Tc. The
//!   SWMM `Area` is authoritative; polygons are not re-measured.
//! * **Inlets.** `[INLET_USAGE]` rows mark their node an inlet and carry the
//!   `[INLETS]` grate length and width (or curb length) and `ON_SAG`
//!   placement into the per-inlet HEC-22 overrides; a `STREET` section on
//!   the conduit supplies the cross slope. Inlet count and clogging are
//!   noted, not carried.
//! * **Tailwater.** A `FIXED` outfall's stage becomes the tailwater; other
//!   outfall types keep the design basis's value.
//! * **Everything hydrological** (IDF curves, return period, minimum Tc,
//!   junction K, minimum slope) comes from the `template` project, since a
//!   SWMM model has no IDF curve of its own.
//!
//! # storm-sewer → SWMM (`import_commands`)
//!
//! One [`Command::Batch`] over a blank model, so the import is one undo
//! step. Values are written in U.S. customary units whatever the project's
//! unit system, with `FLOW_UNITS CFS`. Options are dynamic-wave defaults.
//! The rain gage reads a **design storm built by the alternating-block
//! method from the project's design-return-period IDF curve** (5-minute
//! blocks over 2 hours by default): the IDF curve is what the project has,
//! it is valid over that duration range, and the alternating-block storm
//! reproduces the curve's intensity at every duration up to the storm
//! length. A user-supplied hyetograph can be passed instead. Catchment
//! polygons become subcatchments (area by the polygon, `%Imperv` by the
//! inverse of the C line above, width = area / flow length); a node's own
//! local area becomes a polygon-less subcatchment `<node>_DA` with width
//! √area and 2 % slope, since the project holds no slope for it.

use std::collections::{HashMap, HashSet};

use stormsewer::catchment::{catchment_tc_minutes, polygon_centroid, shoelace_area_sqft};
use stormsewer::design::{PipeSizeRecommendation, SizeOutcome};
use stormsewer::idf::IdfCurve;
use stormsewer::io::project::{InletOverrides, Project, ProjectNode, ProjectPipe};
use stormsewer::units::UnitSystem;

use crate::doc::{format_number, Command, InpDoc};
use crate::profile::{NodeKind, ProfileNetwork};

/// Runoff coefficient of a fully pervious surface in the `%Imperv` line.
pub const C_PERVIOUS: f64 = 0.20;
/// Runoff coefficient of a fully impervious surface in the `%Imperv` line.
pub const C_IMPERVIOUS: f64 = 0.95;

/// Rational C from a SWMM `%Imperv`: a straight line from [`C_PERVIOUS`] to
/// [`C_IMPERVIOUS`].
pub fn runoff_c_from_pct_imperv(pct_imperv: f64) -> f64 {
    let f = (pct_imperv / 100.0).clamp(0.0, 1.0);
    C_PERVIOUS + (C_IMPERVIOUS - C_PERVIOUS) * f
}

/// The inverse of [`runoff_c_from_pct_imperv`], clamped to 0–100.
pub fn pct_imperv_from_runoff_c(c: f64) -> f64 {
    ((c - C_PERVIOUS) / (C_IMPERVIOUS - C_PERVIOUS)).clamp(0.0, 1.0) * 100.0
}

/// An object the mapping could not carry into the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// `conduit`, `pump`, `orifice`, `weir`, `outlet`, `subcatchment`.
    pub kind: String,
    pub name: String,
    pub reason: String,
}

/// A SWMM model as the engine sees it, with what was left out.
#[derive(Clone, Debug)]
pub struct Mapping {
    pub project: Project,
    pub skipped: Vec<Skipped>,
    /// Assumptions made, one line each, for the user and the report.
    pub notes: Vec<String>,
    /// `FLOW_UNITS` as written.
    pub flow_units: String,
    /// Conduits mapped, by name, in file order.
    pub mapped_conduits: Vec<String>,
}

impl Mapping {
    pub fn skipped_of<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Skipped> + 'a {
        self.skipped.iter().filter(move |s| s.kind == kind)
    }
}

fn num(s: Option<&str>) -> Option<f64> {
    s.and_then(|s| s.trim().parse::<f64>().ok())
}

fn is_metric(flow_units: &str) -> bool {
    matches!(
        flow_units.to_ascii_uppercase().as_str(),
        "CMS" | "LPS" | "MLD"
    )
}

/// Equal-area circular diameter of a non-circular section (ft).
fn equivalent_diameter(shape: &str, rise: f64, span: f64) -> f64 {
    let area = match shape {
        "box" => rise * span,
        "elliptical" => std::f64::consts::PI / 4.0 * rise * span,
        "arch" => {
            let r = span / 2.0;
            span * (rise - r).max(0.0) + std::f64::consts::PI * r * r / 2.0
        }
        _ => std::f64::consts::PI / 4.0 * rise * rise,
    };
    (4.0 * area / std::f64::consts::PI).sqrt()
}

/// Map a SWMM model onto the engine's project, taking the hydrology and
/// hydraulic basis (IDF, return period, minimum Tc, junction K, minimum
/// slope, tailwater fallback) from `template`.
pub fn to_project(doc: &InpDoc, template: &Project) -> Result<Mapping, String> {
    let flow_units = doc
        .option("FLOW_UNITS")
        .unwrap_or("CFS")
        .to_ascii_uppercase();
    if is_metric(&flow_units) {
        return Err(format!(
            "FLOW_UNITS is {flow_units}: the storm-sewer engine analyses and reports in U.S. customary units only (feet, acres, cfs). Set FLOW_UNITS to CFS, GPM or MGD, with elevations and lengths in feet and areas in acres, before designing."
        ));
    }

    let mut notes: Vec<String> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();
    let net = ProfileNetwork::from_doc(doc);

    // -- design basis from the template, in engine units -----------------------
    let mut project = template.clone();
    let tu = template.units;
    project.units = UnitSystem::UsCustomary;
    project.idf_a = tu.idf_a_to_engine(template.idf_a);
    for c in &mut project.idf_curves {
        c.a = tu.idf_a_to_engine(c.a);
    }
    project.tailwater = template.tailwater.map(|t| tu.length_to_engine_ft(t));
    if template.access_hole_diam_ft > 0.0 {
        project.access_hole_diam_ft = tu.length_to_engine_ft(template.access_hole_diam_ft);
    }
    project.nodes.clear();
    project.pipes.clear();
    project.catchments.clear();
    project.background = None;
    project.background_dxf = None;
    let title = doc.title();
    project.name = title
        .lines()
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("SWMM model")
        .to_string();

    // -- nodes -------------------------------------------------------------------
    let mut nodes: Vec<ProjectNode> = Vec::new();
    let mut without_xy = 0usize;
    for pn in &net.nodes {
        let kind = match pn.kind {
            NodeKind::Outfall => "outfall",
            NodeKind::Junction => "junction",
            NodeKind::Storage => {
                notes.push(format!(
                    "Storage unit {} is analysed as a junction: the engine routes peak flows and does not attenuate.",
                    pn.id
                ));
                "junction"
            }
            NodeKind::Divider => {
                notes.push(format!(
                    "Divider {} is analysed as a junction: its diversion is not modelled.",
                    pn.id
                ));
                "junction"
            }
        };
        let (x, y) = match doc.coordinates(&pn.id) {
            Some(xy) => xy,
            None => {
                without_xy += 1;
                (0.0, 0.0)
            }
        };
        nodes.push(ProjectNode {
            id: pn.id.clone(),
            kind: kind.into(),
            x,
            y,
            invert: pn.invert,
            rim: net.rim_of(pn),
            area_ac: 0.0,
            c: 0.0,
            tc_inlet: 0.0,
            inlet: InletOverrides::default(),
            bypass_to: None,
            diameter_ft: 4.0,
        });
    }
    if without_xy > 0 {
        notes.push(format!(
            "{without_xy} node(s) have no [COORDINATES]; placed at the origin (bend losses, if enabled, see a straight line)."
        ));
    }

    // Tailwater from a FIXED outfall.
    for (_, row) in doc.rows("OUTFALLS") {
        let cols = doc.columns("OUTFALLS", row);
        let kind = row.get(cols, "Type").unwrap_or("").to_ascii_uppercase();
        let name = row.value(0).unwrap_or("");
        match kind.as_str() {
            "FIXED" => {
                if let Some(stage) = num(row.get(cols, "Stage")) {
                    project.tailwater = Some(stage);
                    notes.push(format!(
                        "Tailwater {stage:.2} ft taken from FIXED outfall {name}."
                    ));
                }
            }
            "TIDAL" | "TIMESERIES" => notes.push(format!(
                "Outfall {name} is {kind}: its stage varies; the design basis tailwater is used instead."
            )),
            _ => {}
        }
    }

    let node_index: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.to_ascii_uppercase(), i))
        .collect();
    let find_node = |name: &str| node_index.get(&name.to_ascii_uppercase()).copied();

    // -- conduits ----------------------------------------------------------------
    let mut xsections: HashMap<String, (String, f64, f64, Option<String>)> = HashMap::new();
    for (_, row) in doc.rows("XSECTIONS") {
        let cols = doc.columns("XSECTIONS", row);
        let Some(link) = row.value(0) else { continue };
        let shape = row.get(cols, "Shape").unwrap_or("").to_ascii_uppercase();
        let g1 = num(row.get(cols, "Geom1")).unwrap_or(0.0);
        let g2 = num(row.get(cols, "Geom2")).unwrap_or(0.0);
        let street = row.get(cols, "Street").map(|s| s.to_string());
        xsections.insert(link.to_ascii_uppercase(), (shape, g1, g2, street));
    }

    let mut pipes: Vec<ProjectPipe> = Vec::new();
    let mut mapped_conduits = Vec::new();
    for (_, row) in doc.rows("CONDUITS") {
        let cols = doc.columns("CONDUITS", row);
        let (Some(name), Some(from), Some(to)) = (row.value(0), row.value(1), row.value(2))
        else {
            continue;
        };
        let skip = |reason: String| Skipped {
            kind: "conduit".into(),
            name: name.to_string(),
            reason,
        };
        let (Some(fi), Some(ti)) = (find_node(from), find_node(to)) else {
            skipped.push(skip(format!(
                "an end node ({from} or {to}) is not a junction, outfall, storage unit or divider"
            )));
            continue;
        };
        let length = num(row.get(cols, "Length")).unwrap_or(0.0);
        let n = num(row.get(cols, "Roughness")).unwrap_or(0.013);
        let Some((shape, g1, g2, _)) = xsections.get(&name.to_ascii_uppercase()) else {
            skipped.push(skip("no [XSECTIONS] row".into()));
            continue;
        };
        if *g1 <= 0.0 {
            skipped.push(skip(format!("{shape} with Geom1 = {g1}")));
            continue;
        }
        let mut pipe = ProjectPipe::new(name, from, to, length, *g1, n);
        match shape.as_str() {
            "CIRCULAR" => {}
            "FORCE_MAIN" => notes.push(format!(
                "Conduit {name} is a FORCE_MAIN: analysed as a circular gravity pipe with Manning's n {n}."
            )),
            "FILLED_CIRCULAR" => notes.push(format!(
                "Conduit {name} is FILLED_CIRCULAR: the sediment depth is ignored."
            )),
            "RECT_CLOSED" if *g2 > 0.0 => {
                pipe.shape = "box".into();
                pipe.rise_ft = *g1;
                pipe.span_ft = *g2;
                pipe.diameter = equivalent_diameter("box", *g1, *g2);
            }
            "HORIZ_ELLIPSE" | "VERT_ELLIPSE" if *g2 > 0.0 => {
                pipe.shape = "elliptical".into();
                pipe.rise_ft = *g1;
                pipe.span_ft = *g2;
                pipe.diameter = equivalent_diameter("elliptical", *g1, *g2);
                if shape == "VERT_ELLIPSE" {
                    notes.push(format!(
                        "Conduit {name} is a VERT_ELLIPSE: analysed as an ellipse {g1} high by {g2} wide."
                    ));
                }
            }
            "ARCH" if *g2 > 0.0 && *g1 >= *g2 / 2.0 => {
                pipe.shape = "arch".into();
                pipe.rise_ft = *g1;
                pipe.span_ft = *g2;
                pipe.diameter = equivalent_diameter("arch", *g1, *g2);
            }
            "ARCH" => {
                skipped.push(skip(format!(
                    "ARCH {g1} × {g2}: the engine's arch needs rise ≥ span/2 and a span"
                )));
                continue;
            }
            "RECT_CLOSED" | "HORIZ_ELLIPSE" | "VERT_ELLIPSE" => {
                skipped.push(skip(format!("{shape} with Geom2 = {g2}")));
                continue;
            }
            other => {
                skipped.push(skip(format!(
                    "{other} cross-section has no equivalent in the storm-sewer engine (circular, box, elliptical, arch)"
                )));
                continue;
            }
        }
        if let Some(pl) = net.link(name) {
            if pl.in_offset > 1e-9 {
                pipe.invert_up = Some(nodes[fi].invert + pl.in_offset);
            }
            if pl.out_offset > 1e-9 {
                pipe.invert_dn = Some(nodes[ti].invert + pl.out_offset);
            }
        }
        mapped_conduits.push(name.to_string());
        pipes.push(pipe);
    }
    for (section, kind, why) in [
        ("PUMPS", "pump", "a pump lifts against gravity; not a gravity conduit"),
        ("ORIFICES", "orifice", "an orifice is a control, not a conduit"),
        ("WEIRS", "weir", "a weir is a control, not a conduit"),
        ("OUTLETS", "outlet", "an outlet is a rating relation, not a conduit"),
    ] {
        for name in doc.names(section) {
            skipped.push(Skipped {
                kind: kind.into(),
                name,
                reason: why.into(),
            });
        }
    }

    // -- subcatchments -------------------------------------------------------------
    struct Sub {
        name: String,
        outlet: String,
        area: f64,
        pct_imperv: f64,
        width: f64,
        pct_slope: f64,
    }
    let subs: Vec<Sub> = doc
        .rows("SUBCATCHMENTS")
        .into_iter()
        .filter_map(|(_, row)| {
            let cols = doc.columns("SUBCATCHMENTS", row);
            Some(Sub {
                name: row.value(0)?.to_string(),
                outlet: row.get(cols, "Outlet").unwrap_or("").to_string(),
                area: num(row.get(cols, "Area")).unwrap_or(0.0),
                pct_imperv: num(row.get(cols, "PctImperv")).unwrap_or(0.0),
                width: num(row.get(cols, "Width")).unwrap_or(0.0),
                pct_slope: num(row.get(cols, "PctSlope")).unwrap_or(0.0),
            })
        })
        .collect();
    let sub_index: HashMap<String, usize> = subs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.to_ascii_uppercase(), i))
        .collect();
    let mut any_sub = false;
    for s in &subs {
        // Follow subcatchment-to-subcatchment routing to a node.
        let mut outlet = s.outlet.clone();
        let mut seen: HashSet<String> = HashSet::new();
        let node = loop {
            if let Some(i) = find_node(&outlet) {
                break Some(i);
            }
            let key = outlet.to_ascii_uppercase();
            if !seen.insert(key.clone()) {
                break None;
            }
            match sub_index.get(&key) {
                Some(&j) => outlet = subs[j].outlet.clone(),
                None => break None,
            }
        };
        let Some(ni) = node else {
            skipped.push(Skipped {
                kind: "subcatchment".into(),
                name: s.name.clone(),
                reason: format!("outlet {:?} does not reach a node", s.outlet),
            });
            continue;
        };
        if s.area <= 0.0 {
            continue;
        }
        any_sub = true;
        let c = runoff_c_from_pct_imperv(s.pct_imperv);
        let tc = if s.width > 0.0 && s.pct_slope > 0.0 {
            catchment_tc_minutes(s.area * 43_560.0 / s.width, s.pct_slope / 100.0)
        } else {
            0.0
        };
        let node = &mut nodes[ni];
        let new_area = node.area_ac + s.area;
        node.c = (node.c * node.area_ac + c * s.area) / new_area;
        node.area_ac = new_area;
        node.tc_inlet = node.tc_inlet.max(tc);
        if node.kind == "junction" {
            node.kind = "inlet".into();
        }
    }
    if any_sub {
        notes.push(format!(
            "Runoff coefficient from %Imperv: C = {C_PERVIOUS:.2} + {:.2}·(%Imperv/100); inlet time by Kirpich over Area/Width at %Slope, floored at the design minimum Tc of {:.0} min.",
            C_IMPERVIOUS - C_PERVIOUS,
            project.min_tc
        ));
    }

    // -- inlets ----------------------------------------------------------------------
    // [INLETS]: Name GRATE Length Width Type | Name CURB Length Height Throat
    let mut inlet_defs: HashMap<String, (f64, f64, f64)> = HashMap::new();
    for (_, row) in doc.rows("INLETS") {
        let (Some(name), Some(kind)) = (row.value(0), row.value(1)) else {
            continue;
        };
        let e = inlet_defs
            .entry(name.to_ascii_uppercase())
            .or_insert((0.0, 0.0, 0.0));
        let l = num(row.value(2)).unwrap_or(0.0);
        let w = num(row.value(3)).unwrap_or(0.0);
        match kind.to_ascii_uppercase().as_str() {
            "GRATE" | "DROP_GRATE" => {
                e.0 = l;
                e.1 = w;
            }
            "CURB" | "DROP_CURB" => e.2 = l,
            _ => {}
        }
    }
    // [STREETS]: Name Tcrown Hcurb Sroad(%) nRoad ...
    let mut streets: HashMap<String, f64> = HashMap::new();
    for (_, row) in doc.rows("STREETS") {
        if let (Some(name), Some(sx)) = (row.value(0), num(row.value(3))) {
            streets.insert(name.to_ascii_uppercase(), sx / 100.0);
        }
    }
    let mut inlet_rows = 0usize;
    for (_, row) in doc.rows("INLET_USAGE") {
        let cols = doc.columns("INLET_USAGE", row);
        let (Some(link), Some(inlet), Some(node)) = (
            row.value(0),
            row.get(cols, "Inlet"),
            row.get(cols, "Node"),
        ) else {
            continue;
        };
        let Some(ni) = find_node(node) else { continue };
        inlet_rows += 1;
        let n = &mut nodes[ni];
        if n.kind == "junction" {
            n.kind = "inlet".into();
        }
        if let Some((gl, gw, cl)) = inlet_defs.get(&inlet.to_ascii_uppercase()) {
            n.inlet.length_ft = if *gl > 0.0 { *gl } else { *cl };
            n.inlet.grate_width_ft = *gw;
        }
        n.inlet.sag = row
            .get(cols, "Placement")
            .is_some_and(|p| p.eq_ignore_ascii_case("ON_SAG"));
        if let Some((_, _, _, Some(street))) = xsections.get(&link.to_ascii_uppercase()) {
            if let Some(sx) = streets.get(&street.to_ascii_uppercase()) {
                n.inlet.cross_slope = *sx;
            }
        }
    }
    if inlet_rows > 0 {
        notes.push(format!(
            "{inlet_rows} [INLET_USAGE] row(s) mapped to HEC-22 inlet overrides (grate/curb length, grate width, sag placement, street cross slope); the number of inlets per node and %clogged are not carried."
        ));
    }

    project.nodes = nodes;
    project.pipes = pipes;
    Ok(Mapping {
        project,
        skipped,
        notes,
        flow_units,
        mapped_conduits,
    })
}

// ---------------------------------------------------------------------------
// Auto-size
// ---------------------------------------------------------------------------

/// One conduit the auto-size batch changes.
#[derive(Clone, Debug, PartialEq)]
pub struct SizeChange {
    pub link: String,
    pub shape: String,
    pub before_ft: f64,
    pub after_ft: f64,
    /// Design flow the size was chosen for (cfs).
    pub design_q: f64,
    pub note: String,
}

/// The one-step batch that writes recommended sizes into `[XSECTIONS]`, and
/// the before/after list for a preview. Only recommendations with a
/// solution and a size different from the current one are written:
/// `CIRCULAR` (and `FORCE_MAIN`) get `Geom1`; `RECT_CLOSED` gets `Geom1` and
/// `Geom2` set to the recommended circular diameter (a square box of that
/// side, which carries at least what the circle does); ellipses get `Geom1`
/// = d and `Geom2` = 1.5 d, as the storm-sewer workspace does. Arches are
/// left as they are and noted.
pub fn autosize_command(doc: &InpDoc, recs: &[PipeSizeRecommendation]) -> (Command, Vec<SizeChange>) {
    let mut cmds = Vec::new();
    let mut changes = Vec::new();
    for r in recs {
        if r.outcome != SizeOutcome::Sized {
            continue;
        }
        let d = r.recommended_diameter_ft;
        if (d - r.current_diameter_ft).abs() < 1e-6 || d <= 0.0 {
            continue;
        }
        let Some((_, row)) = doc.find("XSECTIONS", &r.pipe_id) else {
            continue;
        };
        let cols = doc.columns("XSECTIONS", row);
        let shape = row.get(cols, "Shape").unwrap_or("").to_ascii_uppercase();
        let set = |field: &str, v: f64| Command::SetField {
            section: "XSECTIONS".into(),
            name: r.pipe_id.clone(),
            field: field.into(),
            value: format_number(v),
        };
        let before = num(row.get(cols, "Geom1")).unwrap_or(0.0);
        let note = match shape.as_str() {
            "CIRCULAR" | "FORCE_MAIN" | "FILLED_CIRCULAR" => {
                cmds.push(set("Geom1", d));
                String::new()
            }
            "RECT_CLOSED" => {
                cmds.push(set("Geom1", d));
                cmds.push(set("Geom2", d));
                "box set square at the recommended diameter".into()
            }
            "HORIZ_ELLIPSE" | "VERT_ELLIPSE" => {
                cmds.push(set("Geom1", d));
                cmds.push(set("Geom2", d * 1.5));
                "ellipse set to d × 1.5 d".into()
            }
            _ => continue,
        };
        changes.push(SizeChange {
            link: r.pipe_id.clone(),
            shape,
            before_ft: before,
            after_ft: d,
            design_q: r.design_q,
            note,
        });
    }
    (Command::Batch(cmds), changes)
}

// ---------------------------------------------------------------------------
// Import: storm-sewer project → SWMM model
// ---------------------------------------------------------------------------

/// The rainfall a new model's gage reads.
#[derive(Clone, Debug, PartialEq)]
pub enum DesignStorm {
    /// Alternating-block hyetograph from the project's design IDF curve.
    AlternatingBlock { duration_min: u32, step_min: u32 },
    /// A user-supplied hyetograph: `(minutes from start, intensity in/hr)`.
    Series(Vec<(u32, f64)>),
}

impl Default for DesignStorm {
    fn default() -> Self {
        Self::AlternatingBlock {
            duration_min: 120,
            step_min: 5,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportOptions {
    pub storm: DesignStorm,
}

/// Alternating-block hyetograph: `(minutes, in/hr)` per block, the peak
/// block at the centre and the others alternating right and left of it.
pub fn alternating_block(idf: &IdfCurve, duration_min: u32, step_min: u32) -> Vec<(u32, f64)> {
    let step = step_min.max(1);
    let n = (duration_min / step).max(1) as usize;
    let depth_at = |t_min: f64| idf.intensity(t_min) * t_min / 60.0;
    let mut blocks: Vec<f64> = (1..=n)
        .map(|k| {
            let t1 = k as f64 * step as f64;
            let t0 = (k - 1) as f64 * step as f64;
            (depth_at(t1) - depth_at(t0)).max(0.0)
        })
        .collect();
    blocks.sort_by(|a, b| b.total_cmp(a));
    // Positions in fill order: the centre, then alternately one to the
    // right and one to the left, spilling to the other side at an edge so
    // every block is placed whatever the parity of `n`.
    let centre = n / 2;
    let mut order = vec![centre];
    let (mut l, mut r) = (centre, centre);
    while order.len() < n {
        if r + 1 < n {
            r += 1;
            order.push(r);
        }
        if order.len() < n && l > 0 {
            l -= 1;
            order.push(l);
        }
    }
    let mut placed = vec![0.0; n];
    for (pos, d) in order.into_iter().zip(blocks) {
        placed[pos] = d;
    }
    let hours = step as f64 / 60.0;
    placed
        .into_iter()
        .enumerate()
        .map(|(k, d)| (k as u32 * step, d / hours))
        .collect()
}

fn hhmm(minutes: u32) -> String {
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

fn row(section: &str, fields: Vec<String>) -> Command {
    Command::AddRow {
        section: section.into(),
        fields,
        comment: None,
    }
}

fn opt(section: &str, key: &str, value: impl Into<String>) -> Command {
    Command::SetOption {
        section: section.into(),
        key: key.into(),
        value: value.into(),
    }
}

/// The batch that turns a blank model into the project: nodes, conduits and
/// cross-sections, coordinates, subcatchments (with polygons where the
/// project has them), a rain gage with the design storm, and dynamic-wave
/// options. Returns the command and notes on what was assumed.
pub fn import_commands(project: &Project, opts: &ImportOptions) -> (Command, Vec<String>) {
    let u = project.units;
    let ft = |v: f64| u.length_to_engine_ft(v);
    let s = |v: &str| v.to_string();
    let f = format_number;
    let mut cmds = Vec::new();
    let mut notes = Vec::new();

    cmds.push(Command::SetTitle {
        text: format!("{}\nImported from a StormSewer project", project.name),
    });
    let storm = match &opts.storm {
        DesignStorm::AlternatingBlock {
            duration_min,
            step_min,
        } => {
            notes.push(format!(
                "Design storm: alternating-block hyetograph from the {:.0}-yr IDF curve i = {:.2}/(t + {:.1})^{:.2}, {step_min}-min blocks over {duration_min} min.",
                project.design_return_period_years,
                project.idf().a,
                project.idf().b,
                project.idf().c
            ));
            alternating_block(&project.idf(), *duration_min, *step_min)
        }
        DesignStorm::Series(pts) => {
            notes.push("Design storm: user-supplied hyetograph.".into());
            pts.clone()
        }
    };
    let storm_end = storm.last().map_or(0, |(t, _)| *t);
    let step = storm
        .windows(2)
        .map(|w| w[1].0.saturating_sub(w[0].0))
        .find(|d| *d > 0)
        .unwrap_or(5);
    let end_min = storm_end + step + 6 * 60;
    let end_days = end_min / 1440;
    let end_hhmm = format!("{}:00", hhmm(end_min % 1440));
    for (k, v) in [
        ("FLOW_UNITS", s("CFS")),
        ("FLOW_ROUTING", s("DYNWAVE")),
        ("LINK_OFFSETS", s("DEPTH")),
        ("START_DATE", s("01/01/2026")),
        ("START_TIME", s("00:00:00")),
        ("REPORT_START_DATE", s("01/01/2026")),
        ("REPORT_START_TIME", s("00:00:00")),
        ("END_DATE", format!("01/{:02}/2026", 1 + end_days)),
        ("END_TIME", end_hhmm),
        ("REPORT_STEP", s("00:05:00")),
        ("WET_STEP", s("00:01:00")),
        ("DRY_STEP", s("01:00:00")),
        ("ROUTING_STEP", s("0:00:05")),
        ("INERTIAL_DAMPING", s("PARTIAL")),
        ("NORMAL_FLOW_LIMITED", s("BOTH")),
        ("VARIABLE_STEP", s("0.75")),
        ("MIN_SURFAREA", s("12.557")),
        ("MAX_TRIALS", s("8")),
        ("HEAD_TOLERANCE", s("0.005")),
    ] {
        cmds.push(opt("OPTIONS", k, v));
    }
    notes.push("Options: dynamic-wave routing, 5-s routing step, 5-min reporting, storm plus 6 h of drain-down.".into());

    // Rain gage and storm.
    cmds.push(row(
        "RAINGAGES",
        vec![
            s("RG1"),
            s("INTENSITY"),
            hhmm(step),
            s("1.0"),
            s("TIMESERIES"),
            s("DesignStorm"),
        ],
    ));
    for (t, i) in &storm {
        cmds.push(row(
            "TIMESERIES",
            vec![s("DesignStorm"), hhmm(*t), format!("{i:.4}")],
        ));
    }

    // Nodes.
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    let mut grow = |x: f64, y: f64| {
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    };
    let tailwater = project.tailwater.map(ft);
    for n in &project.nodes {
        let invert = ft(n.invert);
        let rim = ft(n.rim);
        if n.kind == "outfall" {
            let mut fields = vec![n.id.clone(), f(invert)];
            match tailwater {
                Some(tw) => fields.extend([s("FIXED"), f(tw), s("NO")]),
                None => fields.extend([s("FREE"), s("NO")]),
            }
            cmds.push(row("OUTFALLS", fields));
        } else {
            cmds.push(row(
                "JUNCTIONS",
                vec![
                    n.id.clone(),
                    f(invert),
                    f((rim - invert).max(0.0)),
                    s("0"),
                    s("0"),
                    s("0"),
                ],
            ));
        }
        let (x, y) = (ft(n.x), ft(n.y));
        grow(x, y);
        cmds.push(Command::MoveNode {
            name: n.id.clone(),
            x,
            y,
        });
    }
    if tailwater.is_some() {
        notes.push("Outfalls are FIXED at the project tailwater.".into());
    }

    // Conduits.
    let invert_of = |id: &str| -> f64 {
        project
            .nodes
            .iter()
            .find(|n| n.id.eq_ignore_ascii_case(id))
            .map_or(0.0, |n| ft(n.invert))
    };
    for p in &project.pipes {
        let in_off = p
            .invert_up
            .map_or(0.0, |v| (ft(v) - invert_of(&p.from)).max(0.0));
        let out_off = p
            .invert_dn
            .map_or(0.0, |v| (ft(v) - invert_of(&p.to)).max(0.0));
        cmds.push(row(
            "CONDUITS",
            vec![
                p.id.clone(),
                p.from.clone(),
                p.to.clone(),
                f(ft(p.length)),
                format_number(p.n),
                f(in_off),
                f(out_off),
                s("0"),
                s("0"),
            ],
        ));
        let (rise, span) = (ft(p.rise_ft), ft(p.span_ft));
        let xs = match p.shape.as_str() {
            "box" if rise > 0.0 && span > 0.0 => vec![s("RECT_CLOSED"), f(rise), f(span), s("0"), s("0"), s("1")],
            "elliptical" if rise > 0.0 && span > 0.0 => {
                vec![s("HORIZ_ELLIPSE"), f(rise), f(span), s("0"), s("0"), s("1")]
            }
            "arch" if rise > 0.0 && span > 0.0 => vec![s("ARCH"), f(rise), f(span), s("0"), s("0"), s("1")],
            _ => vec![s("CIRCULAR"), f(ft(p.diameter)), s("0"), s("0"), s("0"), s("1")],
        };
        let mut fields = vec![p.id.clone()];
        fields.extend(xs);
        cmds.push(row("XSECTIONS", fields));
    }

    // Subcatchments: catchment polygons, then node-local areas.
    let mut sub_names: Vec<String> = Vec::new();
    let mut n_polys = 0usize;
    for c in &project.catchments {
        let verts: Vec<(f64, f64)> = c.vertices.iter().map(|(x, y)| (ft(*x), ft(*y))).collect();
        let area_sqft = shoelace_area_sqft(&verts);
        if verts.len() < 3 || area_sqft <= 0.0 {
            notes.push(format!(
                "Catchment {} has no polygon area and was not imported.",
                c.id
            ));
            continue;
        }
        let outlet = c
            .inlet_node_id
            .clone()
            .filter(|id| project.nodes.iter().any(|n| n.id.eq_ignore_ascii_case(id)))
            .or_else(|| {
                let (cx, cy) = polygon_centroid(&verts);
                project
                    .nodes
                    .iter()
                    .filter(|n| n.kind != "outfall")
                    .min_by(|a, b| {
                        let da = (ft(a.x) - cx).hypot(ft(a.y) - cy);
                        let db = (ft(b.x) - cx).hypot(ft(b.y) - cy);
                        da.total_cmp(&db)
                    })
                    .map(|n| n.id.clone())
            });
        let Some(outlet) = outlet else {
            notes.push(format!(
                "Catchment {} has no inlet to drain to and was not imported.",
                c.id
            ));
            continue;
        };
        let flow_len = ft(c.flow_length_ft);
        let width = if flow_len > 0.0 {
            area_sqft / flow_len
        } else {
            area_sqft.sqrt()
        };
        let name = c.id.clone();
        cmds.push(row(
            "SUBCATCHMENTS",
            vec![
                name.clone(),
                s("RG1"),
                outlet,
                format!("{:.4}", area_sqft / 43_560.0),
                format!("{:.1}", pct_imperv_from_runoff_c(c.c)),
                f(width),
                format!("{:.2}", (c.slope * 100.0).max(0.0)),
                s("0"),
            ],
        ));
        cmds.push(Command::SetPolygon {
            subcatchment: name.clone(),
            points: verts.clone(),
        });
        for (x, y) in &verts {
            grow(*x, *y);
        }
        n_polys += 1;
        sub_names.push(name);
    }
    let mut n_local = 0usize;
    for n in &project.nodes {
        if n.kind == "outfall" || n.area_ac <= 0.0 {
            continue;
        }
        let area_ac = u.area_to_engine_ac(n.area_ac);
        let width = (area_ac * 43_560.0).sqrt();
        let name = format!("{}_DA", n.id);
        cmds.push(row(
            "SUBCATCHMENTS",
            vec![
                name.clone(),
                s("RG1"),
                n.id.clone(),
                format!("{area_ac:.4}"),
                format!("{:.1}", pct_imperv_from_runoff_c(n.c)),
                f(width),
                s("2"),
                s("0"),
            ],
        ));
        n_local += 1;
        sub_names.push(name);
    }
    for name in &sub_names {
        cmds.push(row(
            "SUBAREAS",
            vec![
                name.clone(),
                s("0.015"),
                s("0.24"),
                s("0.06"),
                s("0.3"),
                s("25"),
                s("OUTLET"),
            ],
        ));
        cmds.push(row(
            "INFILTRATION",
            vec![name.clone(), s("3.0"), s("0.5"), s("4"), s("7"), s("0")],
        ));
    }
    if n_polys + n_local > 0 {
        notes.push(format!(
            "{n_polys} catchment polygon(s) and {n_local} node-local area(s) became subcatchments: %Imperv = ({} − {C_PERVIOUS:.2}) / {:.2} × 100 from C; polygon width = area / flow length; node-local width = √area at 2 % slope. Horton infiltration and SWMM GUI subarea defaults.",
            "C",
            C_IMPERVIOUS - C_PERVIOUS
        ));
    }

    // Map extent and the gage symbol.
    if let Some((x0, y0, x1, y1)) = bounds {
        let pad = ((x1 - x0).max(y1 - y0) * 0.1).max(10.0);
        cmds.push(opt(
            "MAP",
            "DIMENSIONS",
            format!(
                "{} {} {} {}",
                f(x0 - pad),
                f(y0 - pad),
                f(x1 + pad),
                f(y1 + pad)
            ),
        ));
        cmds.push(opt("MAP", "Units", "Feet"));
        cmds.push(Command::MoveGage {
            name: "RG1".into(),
            x: x0 - pad / 2.0,
            y: y1 + pad / 2.0,
        });
    }
    if u == UnitSystem::Si {
        notes.push("The project is in SI; the model is written in U.S. customary units (feet, acres, CFS).".into());
    }

    (Command::Batch(cmds), notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::build::new_model_text;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InpDoc {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/epa-samples")
            .join(name);
        InpDoc::read(&path).unwrap()
    }

    fn pipe<'a>(p: &'a Project, id: &str) -> &'a ProjectPipe {
        p.pipes.iter().find(|x| x.id == id).unwrap()
    }

    #[test]
    fn detention_pond_maps_counts_and_lists_the_unmappable() {
        let doc = fixture("Detention_Pond_Model.inp");
        let m = to_project(&doc, &Project::empty()).unwrap();
        // 12 junctions + 1 outfall + 1 storage unit.
        assert_eq!(m.project.nodes.len(), 14);
        // 4 circular conduits map; 8 trapezoidal channels do not.
        assert_eq!(m.mapped_conduits, vec!["C3", "C7", "C11", "C_out"]);
        assert_eq!(m.project.pipes.len(), 4);
        let trapezoids: Vec<&str> = m
            .skipped_of("conduit")
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            trapezoids,
            vec!["C1", "C2", "C4", "C5", "C6", "C8", "C9", "C10"]
        );
        assert!(m.skipped_of("conduit").all(|s| s.reason.contains("TRAPEZOIDAL")));
        let orifice = m.skipped_of("orifice").next().unwrap();
        assert_eq!(orifice.name, "O1");
        let weir = m.skipped_of("weir").next().unwrap();
        assert_eq!(weir.name, "W1");
        assert!(m.notes.iter().any(|n| n.contains("Storage unit SU1")));

        // C11: J11 → SU1, 150 ft, n 0.016, 4.75 ft circular, 1 ft outlet offset.
        let c11 = pipe(&m.project, "C11");
        assert_eq!((c11.from.as_str(), c11.to.as_str()), ("J11", "SU1"));
        assert!((c11.length - 150.0).abs() < 1e-9);
        assert!((c11.n - 0.016).abs() < 1e-9);
        assert!((c11.diameter - 4.75).abs() < 1e-9);
        assert_eq!(c11.shape, "circular");
        assert_eq!(c11.invert_up, None);
        assert!((c11.invert_dn.unwrap() - (4956.0 + 1.0)).abs() < 1e-9);
        // C3 has no offsets.
        let c3 = pipe(&m.project, "C3");
        assert_eq!((c3.invert_up, c3.invert_dn), (None, None));

        // Nodes: invert and rim; J11 has MaxDepth 0 so rim = highest crown.
        let j11 = m.project.nodes.iter().find(|n| n.id == "J11").unwrap();
        assert!((j11.invert - 4963.0).abs() < 1e-9);
        assert!(j11.rim > j11.invert, "rim from a connecting crown");
        assert_eq!(j11.kind, "inlet", "S6 drains to J11");
        let su1 = m.project.nodes.iter().find(|n| n.id == "SU1").unwrap();
        assert_eq!(su1.kind, "junction");
        assert!((su1.rim - 4966.0).abs() < 1e-9, "Elevation + MaxDepth");
        assert!(m.project.nodes.iter().any(|n| n.id == "O2" && n.kind == "outfall"));

        // S5 (4.79 ac, 87.7 %) and S7 (2.33 ac, 0 %) both drain to J10.
        let j10 = m.project.nodes.iter().find(|n| n.id == "J10").unwrap();
        assert!((j10.area_ac - 7.12).abs() < 1e-9);
        let c5 = runoff_c_from_pct_imperv(87.7);
        let c7 = runoff_c_from_pct_imperv(0.0);
        let expect = (c5 * 4.79 + c7 * 2.33) / 7.12;
        assert!((j10.c - expect).abs() < 1e-9, "{} vs {expect}", j10.c);
        assert!(j10.tc_inlet > 0.0);
        // Coordinates came across.
        assert!(j10.x != 0.0 || j10.y != 0.0);
        assert_eq!(m.flow_units, "CFS");
        // The engine can analyse what was mapped.
        let net = m.project.to_analysis_network();
        assert!(net.analyze(&m.project.idf(), &m.project.options()).is_ok());
    }

    #[test]
    fn site_drainage_maps_and_keeps_the_template_basis() {
        let doc = fixture("Site_Drainage_Model.inp");
        let mut template = Project::empty();
        template.idf_a = 77.0;
        template.design_return_period_years = 25.0;
        template.tailwater = Some(4962.5);
        let m = to_project(&doc, &template).unwrap();
        assert_eq!(m.project.nodes.len(), 12);
        assert_eq!(m.project.pipes.len(), 3);
        assert_eq!(m.skipped_of("conduit").count(), 8);
        assert!(m.skipped_of("pump").next().is_none());
        assert!((m.project.idf_a - 77.0).abs() < 1e-9);
        assert!((m.project.design_return_period_years - 25.0).abs() < 1e-9);
        assert_eq!(m.project.tailwater, Some(4962.5), "FREE outfall keeps the basis tailwater");
        assert_eq!(m.project.name, "A site surface drainage model.");
        let c7 = pipe(&m.project, "C7");
        assert!((c7.diameter - 3.5).abs() < 1e-9);
        assert!((c7.length - 95.0).abs() < 1e-9);
        assert!(m.project.validate().is_empty());
    }

    #[test]
    fn metric_models_are_refused_with_a_reason() {
        let doc = InpDoc::parse("[OPTIONS]\nFLOW_UNITS CMS\n[JUNCTIONS]\nJ1 10\n");
        let err = to_project(&doc, &Project::empty()).unwrap_err();
        assert!(err.contains("CMS") && err.contains("U.S. customary"), "{err}");
        let doc = InpDoc::parse("[OPTIONS]\nFLOW_UNITS GPM\n[JUNCTIONS]\nJ1 10\n");
        assert!(to_project(&doc, &Project::empty()).is_ok());
    }

    #[test]
    fn shapes_offsets_and_inlets_map() {
        let doc = InpDoc::parse(
            "[OPTIONS]\nFLOW_UNITS CFS\nLINK_OFFSETS ELEVATION\n\
             [JUNCTIONS]\nJ1 100 8\nJ2 98 0\n[OUTFALLS]\nO1 95 FIXED 96.5 NO\n\
             [CONDUITS]\nC1 J1 J2 200 0.013 101 98.5 0 0\nC2 J2 O1 100 0.012 0 0 0 0\nC3 J1 O1 50 0.02 0 0 0 0\n\
             [PUMPS]\nP1 J2 O1 * ON 0 0\n\
             [XSECTIONS]\nC1 RECT_CLOSED 3 4 0 0 1\nC2 ARCH 2 3 0 0 1\nC3 TRIANGULAR 2 3 0 0\n\
             [INLETS]\nCombo GRATE 2 1.5 P_BAR-50\nCombo CURB 3 0.5 HORIZONTAL\n\
             [INLET_USAGE]\nC1 Combo J1 1 0 0 0 0 ON_SAG\n\
             [COORDINATES]\nJ1 0 0\nJ2 200 0\nO1 300 0\n",
        );
        let m = to_project(&doc, &Project::empty()).unwrap();
        let c1 = pipe(&m.project, "C1");
        assert_eq!(c1.shape, "box");
        assert_eq!((c1.rise_ft, c1.span_ft), (3.0, 4.0));
        assert_eq!(c1.invert_up, Some(101.0), "elevation offsets");
        assert_eq!(c1.invert_dn, Some(98.5));
        let c2 = pipe(&m.project, "C2");
        assert_eq!(c2.shape, "arch");
        assert!(m.skipped.iter().any(|s| s.name == "C3" && s.reason.contains("TRIANGULAR")));
        assert!(m.skipped.iter().any(|s| s.kind == "pump" && s.name == "P1"));
        assert_eq!(m.project.tailwater, Some(96.5));
        let j1 = m.project.nodes.iter().find(|n| n.id == "J1").unwrap();
        assert_eq!(j1.kind, "inlet");
        assert!(j1.inlet.sag);
        assert!((j1.inlet.length_ft - 2.0).abs() < 1e-9);
        assert!((j1.inlet.grate_width_ft - 1.5).abs() < 1e-9);
        assert!((j1.rim - 108.0).abs() < 1e-9);
        let net = m.project.to_network();
        let a = net.analyze(&m.project.idf(), &m.project.options()).unwrap();
        assert_eq!(a.pipes.len(), 2);
    }

    #[test]
    fn autosize_batch_is_one_undo_and_writes_only_geom1_for_circulars() {
        let mut doc = fixture("Detention_Pond_Model.inp");
        let before = doc.to_string();
        let m = to_project(&doc, &Project::empty()).unwrap();
        let net = m.project.to_analysis_network();
        let a = net.analyze(&m.project.idf(), &m.project.options()).unwrap();
        // Make every mapped pipe a candidate by pretending it needs a change.
        let mut recs = stormsewer::design::size_network(&net, &a, &stormsewer::design::DesignCriteria::default());
        for r in &mut recs {
            r.outcome = SizeOutcome::Sized;
            r.recommended_diameter_ft = r.current_diameter_ft + 0.25;
        }
        let (cmd, changes) = autosize_command(&doc, &recs);
        assert_eq!(changes.len(), 4);
        assert!(changes.iter().all(|c| (c.after_ft - c.before_ft - 0.25).abs() < 1e-9));
        let Command::Batch(cmds) = &cmd else { panic!() };
        assert_eq!(cmds.len(), 4);
        assert!(cmds.iter().all(|c| matches!(
            c,
            Command::SetField { section, field, .. } if section == "XSECTIONS" && field == "Geom1"
        )));
        doc.apply(cmd).unwrap();
        assert_eq!(doc.undo_depth(), 1);
        assert_eq!(doc.field("XSECTIONS", "C11", "Geom1"), Some("5"));
        assert_eq!(doc.field("XSECTIONS", "C11", "Geom2"), Some("0"));
        assert_eq!(doc.field("CONDUITS", "C11", "Length"), Some("150"));
        // Only [XSECTIONS] lines changed.
        let after = doc.to_string();
        let changed: Vec<&str> = before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a)
            .collect();
        assert_eq!(changed.len(), 4, "{changed:?}");
        assert!(changed.iter().all(|l| l.contains("CIRCULAR")));
        doc.undo();
        assert_eq!(doc.to_string(), before);
        // Recommendations that are adequate or unsolved write nothing.
        for r in &mut recs {
            r.outcome = SizeOutcome::Adequate;
        }
        let (cmd, changes) = autosize_command(&doc, &recs);
        assert!(changes.is_empty());
        assert_eq!(cmd, Command::Batch(vec![]));
    }

    #[test]
    fn alternating_block_reproduces_the_idf_depth() {
        let idf = IdfCurve::new(60.0, 10.0, 0.8);
        let blocks = alternating_block(&idf, 60, 5);
        assert_eq!(blocks.len(), 12);
        assert_eq!(blocks[0].0, 0);
        assert_eq!(blocks[11].0, 55);
        // Total depth equals the 60-min IDF depth.
        let total: f64 = blocks.iter().map(|(_, i)| i * 5.0 / 60.0).sum();
        assert!((total - idf.intensity(60.0)).abs() < 1e-9, "{total}");
        // The peak block sits in the middle and is the 5-min intensity.
        let (peak_t, peak_i) = blocks.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert_eq!(peak_t, 30);
        assert!((peak_i - idf.intensity(5.0)).abs() < 1e-9);
    }

    #[test]
    fn project_round_trips_through_a_model() {
        let mut project = Project::demo();
        project.tailwater = Some(100.5);
        project.pipes[1].invert_up = Some(102.9);
        project.pipes[1].shape = "box".into();
        project.pipes[1].rise_ft = 2.0;
        project.pipes[1].span_ft = 3.0;
        let (cmd, notes) = import_commands(&project, &ImportOptions::default());
        let mut doc = InpDoc::parse(&new_model_text());
        doc.apply(cmd).unwrap();
        assert_eq!(doc.undo_depth(), 1, "import is one step");
        assert!(notes.iter().any(|n| n.contains("alternating-block")));
        assert_eq!(doc.option("FLOW_ROUTING"), Some("DYNWAVE"));
        assert_eq!(doc.option("FLOW_UNITS"), Some("CFS"));
        assert_eq!(doc.field("RAINGAGES", "RG1", "Series"), Some("DesignStorm"));
        assert_eq!(doc.timeseries("DesignStorm").len(), 24);
        assert_eq!(doc.field("OUTFALLS", "OUT", "Type"), Some("FIXED"));
        assert_eq!(doc.field("OUTFALLS", "OUT", "Stage"), Some("100.5"));
        assert_eq!(doc.field("XSECTIONS", "P2", "Shape"), Some("RECT_CLOSED"));
        assert_eq!(doc.field("CONDUITS", "P2", "InOffset"), Some("0.4"));
        assert_eq!(doc.polygon("C1").len(), 3);
        assert_eq!(doc.field("SUBCATCHMENTS", "C1", "Outlet"), Some("N1"));
        assert!(doc.contains("SUBCATCHMENTS", "N1_DA"));
        assert!(doc.contains("SUBAREAS", "N1_DA") && doc.contains("INFILTRATION", "C1"));
        assert!(doc.coordinates("N2").is_some());
        let errors: Vec<_> = doc
            .validate()
            .into_iter()
            .filter(|f| f.severity == crate::doc::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        // The written text parses back to the same document.
        let text = doc.to_string();
        assert_eq!(InpDoc::parse(&text).to_string(), text);

        // ... and back to a project with the same hydraulics.
        let back = to_project(&doc, &project).unwrap();
        assert!(back.skipped.is_empty(), "{:?}", back.skipped);
        assert_eq!(back.project.pipes.len(), project.pipes.len());
        for p in &project.pipes {
            let q = pipe(&back.project, &p.id);
            assert!((q.length - p.length).abs() < 1e-6, "{} length", p.id);
            assert!((q.n - p.n).abs() < 1e-9, "{} n", p.id);
            assert_eq!(q.shape, p.shape);
            if p.shape == "circular" {
                assert!((q.diameter - p.diameter).abs() < 1e-6, "{} diameter", p.id);
            } else {
                assert!((q.rise_ft - p.rise_ft).abs() < 1e-6 && (q.span_ft - p.span_ft).abs() < 1e-6);
            }
            assert_eq!(q.from, p.from);
            assert_eq!(q.to, p.to);
            match p.invert_up {
                Some(v) => assert!((q.invert_up.unwrap() - v).abs() < 1e-6),
                None => assert_eq!(q.invert_up, None),
            }
        }
        for n in &project.nodes {
            let m = back.project.nodes.iter().find(|x| x.id == n.id).unwrap();
            assert!((m.invert - n.invert).abs() < 1e-6, "{} invert", n.id);
            // An outfall has no MaxDepth in SWMM; its rim comes back as the
            // highest connecting crown, which the engine never scores.
            if n.kind != "outfall" {
                assert!((m.rim - n.rim).abs() < 1e-6, "{} rim", n.id);
            }
            assert!((m.x - n.x).abs() < 1e-6 && (m.y - n.y).abs() < 1e-6);
        }
        assert_eq!(back.project.tailwater, Some(100.5));
        // Node N1 carries its own 1 ac plus catchment C1's polygon area.
        let n1 = back.project.nodes.iter().find(|x| x.id == "N1").unwrap();
        let c1_ac = project.catchment_area_ac("C1").unwrap();
        assert!((n1.area_ac - (1.0 + c1_ac)).abs() < 1e-3, "{}", n1.area_ac);
        assert!((n1.c - 0.70).abs() < 0.02, "C survives the %Imperv line: {}", n1.c);
    }

    #[test]
    fn si_projects_import_in_feet() {
        let mut project = Project::demo();
        stormsewer::units::convert_project(&mut project, UnitSystem::Si);
        let (cmd, notes) = import_commands(&project, &ImportOptions::default());
        let mut doc = InpDoc::parse(&new_model_text());
        doc.apply(cmd).unwrap();
        assert!(notes.iter().any(|n| n.contains("SI")));
        let inv: f64 = doc.field("JUNCTIONS", "N1", "Elevation").unwrap().parse().unwrap();
        assert!((inv - 104.0).abs() < 0.01, "{inv}");
        let len: f64 = doc.field("CONDUITS", "P1", "Length").unwrap().parse().unwrap();
        assert!((len - 300.0).abs() < 0.01, "{len}");
    }

    #[test]
    fn user_storm_and_c_line_round_trip() {
        assert!((runoff_c_from_pct_imperv(0.0) - 0.20).abs() < 1e-12);
        assert!((runoff_c_from_pct_imperv(100.0) - 0.95).abs() < 1e-12);
        assert!((pct_imperv_from_runoff_c(runoff_c_from_pct_imperv(40.0)) - 40.0).abs() < 1e-9);
        assert_eq!(pct_imperv_from_runoff_c(0.05), 0.0);
        assert_eq!(pct_imperv_from_runoff_c(1.0), 100.0);
        let opts = ImportOptions {
            storm: DesignStorm::Series(vec![(0, 1.0), (15, 3.0), (30, 0.5)]),
        };
        let (cmd, _) = import_commands(&Project::demo(), &opts);
        let mut doc = InpDoc::parse(&new_model_text());
        doc.apply(cmd).unwrap();
        let ts = doc.timeseries("DesignStorm");
        assert_eq!(ts.len(), 3);
        assert_eq!(ts[1].time, "0:15");
        assert_eq!(doc.field("RAINGAGES", "RG1", "Interval"), Some("0:15"));
        assert_eq!(doc.option("END_TIME"), Some("6:45:00"));
    }
}
