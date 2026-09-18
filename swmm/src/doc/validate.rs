// SPDX-License-Identifier: GPL-3.0-or-later

//! Referential and sanity checks over an [`InpDoc`], for the editor to show
//! live and for Run → Check Model. A finding is a message about a place in
//! the document; the list is empty for a consistent model.
//!
//! Checked:
//!
//! * every reference in [`schema::REFERENCES`] resolves (link endpoints,
//!   subcatchment outlets and gages, curves, series, patterns, links named
//!   by dividers/losses/inlets, subcatchments named by their detail rows,
//!   `[LABELS]` anchors);
//! * duplicate names within a section and across the node and link
//!   families, case-insensitively as the engine sees them, with a separate
//!   message when two spellings differ only by case;
//! * geometry rows for objects that do not exist; nodes without
//!   coordinates; links with an endpoint that has none;
//! * orphan nodes (no link attached, once the model has any links);
//! * a junction whose only outgoing links are weirs or orifices;
//! * conduits with no `[XSECTIONS]` row; zero or negative conduit length;
//! * negative offsets (the engine zeroes them) and offsets that put a pipe
//!   invert above the node rim (the engine raises the rim);
//! * an outfall with more than one link, or with both incoming and outgoing
//!   links;
//! * subcatchments with zero area or width; a rain gage nothing uses; a
//!   time series that is named but has no points;
//! * `[OPTIONS]`: END before START, REPORT_STEP shorter than ROUTING_STEP,
//!   DRY_STEP shorter than WET_STEP;
//! * rows shorter than their section allows;
//! * the LID, groundwater, snow, RDII and water-quality sections: names
//!   that exist, layers a LID type needs, LID area within the
//!   subcatchment, coverages within 100 %, expression variables the
//!   engine knows (see `validate_hydrology_quality`).

use std::collections::{HashMap, HashSet};

use super::build;
use super::schema::{self, ObjectKind, LINK_SECTIONS, NODE_SECTIONS};
use super::{unquote, InpDoc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The engine will refuse or misread the model.
    Error,
    /// Something is off but the model runs (a node that cannot be drawn).
    Warning,
}

/// One thing the checks noticed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// Section name, uppercased.
    pub section: String,
    /// Object name, as written, or empty for a section-level finding.
    pub name: String,
    pub message: String,
    /// The column at fault, when one field is (a schema column name).
    pub column: Option<String>,
}

impl Finding {
    pub fn new(severity: Severity, section: &str, name: &str, message: impl Into<String>) -> Self {
        Self {
            severity,
            section: section.to_string(),
            name: name.to_string(),
            message: message.into(),
            column: None,
        }
    }

    pub fn column(mut self, column: &str) -> Self {
        self.column = Some(column.to_string());
        self
    }
}

/// Names defined by a set of sections, folded for lookup, mapped to where
/// they were first seen (as written, section).
type Index = HashMap<String, (String, String)>;

fn fold(s: &str) -> String {
    unquote(s).to_ascii_uppercase()
}

fn index(doc: &InpDoc, sections: &[&str]) -> Index {
    let mut out = HashMap::new();
    for sec in sections {
        for (_, r) in doc.rows(sec) {
            if let Some(n) = r.value(0) {
                out.entry(fold(n))
                    .or_insert_with(|| (n.to_string(), sec.to_string()));
            }
        }
    }
    out
}

fn known(index: &Index, name: &str) -> bool {
    index.contains_key(&fold(name))
}

fn num(s: Option<&str>) -> Option<f64> {
    s?.trim().parse::<f64>().ok()
}

/// `hh:mm:ss`, `h:mm`, or a plain number of seconds, as seconds.
pub fn step_seconds(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return (v >= 0.0).then_some(v);
    }
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut secs = 0.0;
    for p in &parts {
        secs = secs * 60.0 + p.trim().parse::<f64>().ok()?;
    }
    if parts.len() == 2 {
        secs *= 60.0;
    }
    Some(secs)
}

/// `mm/dd/yyyy` + `hh:mm:ss` as a comparable `(y, m, d, seconds)`.
fn datetime(date: Option<&str>, time: Option<&str>) -> Option<(i64, i64, i64, f64)> {
    let d = date?.trim();
    let parts: Vec<&str> = d.split(['/', '-']).collect();
    if parts.len() != 3 {
        return None;
    }
    let m: i64 = parts[0].parse().ok()?;
    let dd: i64 = parts[1].parse().ok()?;
    let mut y: i64 = parts[2].parse().ok()?;
    if y < 100 {
        y += if y < 50 { 2000 } else { 1900 };
    }
    let secs = time.map(step_seconds).unwrap_or(Some(0.0)).unwrap_or(0.0);
    Some((y, m, dd, secs))
}

/// What the checks know about one node.
struct NodeInfo {
    section: String,
    invert: Option<f64>,
    /// `MaxDepth` (junction/storage/divider). `None` for outfalls; `Some(0)`
    /// means "up to the highest connected crown", which the rim check
    /// cannot evaluate.
    max_depth: Option<f64>,
}

fn node_table(doc: &InpDoc) -> HashMap<String, NodeInfo> {
    let mut out = HashMap::new();
    for sec in NODE_SECTIONS {
        for (_, r) in doc.rows(sec) {
            let Some(n) = r.value(0) else { continue };
            let cols = doc.columns(sec, r);
            let invert = num(r.get(cols, "Elevation"));
            let max_depth = num(r.get(cols, "MaxDepth"));
            out.entry(fold(n)).or_insert(NodeInfo {
                section: sec.to_string(),
                invert,
                max_depth,
            });
        }
    }
    out
}

/// One link's endpoints, by section.
struct LinkInfo {
    section: &'static str,
    name: String,
    from: String,
    to: String,
}

fn link_table(doc: &InpDoc) -> Vec<LinkInfo> {
    let mut out = Vec::new();
    for sec in LINK_SECTIONS {
        for (_, r) in doc.rows(sec) {
            let (Some(n), Some(a), Some(b)) = (r.value(0), r.value(1), r.value(2)) else {
                continue;
            };
            out.push(LinkInfo {
                section: sec,
                name: n.to_string(),
                from: fold(a),
                to: fold(b),
            });
        }
    }
    out
}

/// References the dedicated checks below already cover.
const HANDLED_REFERENCES: &[(&str, &str)] = &[
    ("CONDUITS", "FromNode"),
    ("CONDUITS", "ToNode"),
    ("PUMPS", "FromNode"),
    ("PUMPS", "ToNode"),
    ("ORIFICES", "FromNode"),
    ("ORIFICES", "ToNode"),
    ("WEIRS", "FromNode"),
    ("WEIRS", "ToNode"),
    ("OUTLETS", "FromNode"),
    ("OUTLETS", "ToNode"),
    ("SUBCATCHMENTS", "Outlet"),
    ("SUBCATCHMENTS", "RainGage"),
    ("COORDINATES", "Node"),
    ("VERTICES", "Link"),
    ("POLYGONS", "Subcatchment"),
    ("SYMBOLS", "Gage"),
    ("XSECTIONS", "Link"),
];

fn kind_word(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Node => "node",
        ObjectKind::Link => "link",
        ObjectKind::Subcatchment => "subcatchment",
        ObjectKind::Gage => "rain gage",
        ObjectKind::Curve => "curve",
        ObjectKind::Timeseries => "time series",
        ObjectKind::Pattern => "pattern",
    }
}

pub fn validate(doc: &InpDoc) -> Vec<Finding> {
    let mut out = Vec::new();
    let nodes = index(doc, NODE_SECTIONS);
    let links = index(doc, LINK_SECTIONS);
    let subs = index(doc, &["SUBCATCHMENTS"]);
    let gages = index(doc, &["RAINGAGES"]);
    let curves = index(doc, &["CURVES"]);
    let series = index(doc, &["TIMESERIES"]);
    let patterns = index(doc, &["PATTERNS"]);
    let index_for = |kind: ObjectKind| -> &Index {
        match kind {
            ObjectKind::Node => &nodes,
            ObjectKind::Link => &links,
            ObjectKind::Subcatchment => &subs,
            ObjectKind::Gage => &gages,
            ObjectKind::Curve => &curves,
            ObjectKind::Timeseries => &series,
            ObjectKind::Pattern => &patterns,
        }
    };

    // Duplicates within a section, and across node/link sections.
    for s in doc.sections() {
        if schema::MULTI_ROW_SECTIONS.contains(&s.name.as_str())
            || schema::KEY_VALUE_SECTIONS.contains(&s.name.as_str())
        {
            continue;
        }
        let Some(ni) = schema::name_index(&s.name) else {
            continue;
        };
        let mut seen: HashMap<String, String> = HashMap::new();
        for (_, r) in s.rows() {
            let Some(n) = r.value(ni) else { continue };
            if let Some(first) = seen.get(&fold(n)) {
                let message = if first == n {
                    format!("duplicate name (first defined as {first:?})")
                } else {
                    format!(
                        "duplicate name: differs only by case from {first:?}, and the engine ignores case"
                    )
                };
                out.push(Finding::new(Severity::Error, &s.name, n, message));
            } else {
                seen.insert(fold(n), n.to_string());
            }
        }
        if NODE_SECTIONS.contains(&s.name.as_str()) || LINK_SECTIONS.contains(&s.name.as_str()) {
            let family = if NODE_SECTIONS.contains(&s.name.as_str()) {
                &nodes
            } else {
                &links
            };
            for (_, r) in s.rows() {
                let Some(n) = r.value(0) else { continue };
                if let Some((first, other)) = family.get(&fold(n)) {
                    if other != &s.name {
                        let case = if first != n {
                            " (differing only by case, which the engine ignores)"
                        } else {
                            ""
                        };
                        out.push(Finding::new(
                            Severity::Error,
                            &s.name,
                            n,
                            format!("also defined in [{other}]{case}"),
                        ));
                    }
                }
            }
        }
    }

    // Link endpoints.
    for sec in LINK_SECTIONS {
        for (_, r) in doc.rows(sec) {
            let name = r.value(0).unwrap_or("");
            for (i, end, col) in [(1, "from", "FromNode"), (2, "to", "ToNode")] {
                match r.value(i) {
                    Some(n) if known(&nodes, n) => {}
                    Some(n) => out.push(
                        Finding::new(
                            Severity::Error,
                            sec,
                            name,
                            format!("{end} node {n:?} does not exist"),
                        )
                        .column(col),
                    ),
                    None => out.push(
                        Finding::new(Severity::Error, sec, name, format!("no {end} node"))
                            .column(col),
                    ),
                }
            }
        }
    }

    // Subcatchment outlet, gage, area and width.
    let mut gages_used: HashSet<String> = HashSet::new();
    for (_, r) in doc.rows("SUBCATCHMENTS") {
        let name = r.value(0).unwrap_or("");
        match r.value(2) {
            Some(o) if known(&nodes, o) || known(&subs, o) => {}
            Some(o) => out.push(
                Finding::new(
                    Severity::Error,
                    "SUBCATCHMENTS",
                    name,
                    format!("outlet {o:?} is neither a node nor a subcatchment"),
                )
                .column("Outlet"),
            ),
            None => out.push(
                Finding::new(Severity::Error, "SUBCATCHMENTS", name, "no outlet").column("Outlet"),
            ),
        }
        match r.value(1) {
            Some(g) if known(&gages, g) => {
                gages_used.insert(fold(g));
            }
            Some("*") | None => {}
            Some(g) => out.push(
                Finding::new(
                    Severity::Error,
                    "SUBCATCHMENTS",
                    name,
                    format!("rain gage {g:?} does not exist"),
                )
                .column("RainGage"),
            ),
        }
        if let Some(a) = num(r.value(3)) {
            if a <= 0.0 {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "SUBCATCHMENTS",
                        name,
                        "area is zero or negative; the engine rejects it (ERROR 211)",
                    )
                    .column("Area"),
                );
            }
        }
        if let Some(w) = num(r.value(5)) {
            if w <= 0.0 {
                out.push(
                    Finding::new(
                        Severity::Warning,
                        "SUBCATCHMENTS",
                        name,
                        "width is zero, so no runoff leaves it (overland flow needs a width)",
                    )
                    .column("Width"),
                );
            }
        }
    }
    for (_, r) in doc.rows("RAINGAGES") {
        if let Some(g) = r.value(0) {
            if !gages_used.contains(&fold(g)) {
                out.push(Finding::new(
                    Severity::Warning,
                    "RAINGAGES",
                    g,
                    "no subcatchment uses this rain gage",
                ));
            }
        }
    }

    // Every other reference in the schema table.
    for (sec, col, kind) in schema::REFERENCES {
        if HANDLED_REFERENCES.contains(&(*sec, *col)) {
            continue;
        }
        let ni = schema::name_index(sec);
        let idx = index_for(*kind);
        for (_, r) in doc.rows(sec) {
            let cols = doc.columns(sec, r);
            let Some(v) = r.get(cols, col) else { continue };
            let v = v.trim();
            if v.is_empty() || v == "*" || known(idx, v) {
                continue;
            }
            // `[SUBCATCHMENTS] Outlet` may name either kind; here a column
            // names exactly one, so a miss is a miss.
            let name = ni.and_then(|i| r.value(i)).unwrap_or("");
            out.push(
                Finding::new(
                    Severity::Error,
                    sec,
                    name,
                    format!("{col} names {} {v:?}, which does not exist", kind_word(*kind)),
                )
                .column(col),
            );
        }
    }

    // Geometry rows against objects, and objects against geometry.
    let geometry: [(&str, &Index, &str); 4] = [
        ("COORDINATES", &nodes, "node"),
        ("VERTICES", &links, "link"),
        ("POLYGONS", &subs, "subcatchment"),
        ("SYMBOLS", &gages, "rain gage"),
    ];
    for (sec, idx, what) in geometry {
        let mut seen: HashMap<String, ()> = HashMap::new();
        for (_, r) in doc.rows(sec) {
            let Some(n) = r.value(0) else { continue };
            if !known(idx, n) {
                out.push(Finding::new(
                    Severity::Warning,
                    sec,
                    n,
                    format!("{what} {n:?} does not exist"),
                ));
            }
            if sec == "COORDINATES" && seen.insert(fold(n), ()).is_some() {
                out.push(Finding::new(
                    Severity::Warning,
                    sec,
                    n,
                    "more than one coordinate row",
                ));
            }
            if r.fields.len() >= 3
                && (r.value(1).unwrap_or("").parse::<f64>().is_err()
                    || r.value(2).unwrap_or("").parse::<f64>().is_err())
            {
                out.push(Finding::new(
                    Severity::Error,
                    sec,
                    n,
                    "coordinates are not numbers",
                ));
            }
        }
    }
    let coords = index(doc, &["COORDINATES"]);
    for sec in NODE_SECTIONS {
        for (_, r) in doc.rows(sec) {
            if let Some(n) = r.value(0) {
                if !known(&coords, n) {
                    out.push(Finding::new(
                        Severity::Warning,
                        sec,
                        n,
                        "no [COORDINATES] row, so it cannot be drawn",
                    ));
                }
            }
        }
    }

    // Connectivity: orphan nodes, undrawable links, outfall links,
    // junctions that drain only through a weir or orifice.
    let link_infos = link_table(doc);
    let node_infos = node_table(doc);
    let mut attached: HashSet<String> = HashSet::new();
    let mut incoming: HashMap<String, Vec<&LinkInfo>> = HashMap::new();
    let mut outgoing: HashMap<String, Vec<&LinkInfo>> = HashMap::new();
    for l in &link_infos {
        attached.insert(l.from.clone());
        attached.insert(l.to.clone());
        outgoing.entry(l.from.clone()).or_default().push(l);
        incoming.entry(l.to.clone()).or_default().push(l);
        for (end, col) in [(&l.from, "FromNode"), (&l.to, "ToNode")] {
            if nodes.contains_key(end) && !coords.contains_key(end) {
                out.push(
                    Finding::new(
                        Severity::Warning,
                        l.section,
                        &l.name,
                        format!(
                            "{col} {:?} has no coordinates, so the link cannot be drawn",
                            nodes[end].0
                        ),
                    )
                    .column(col),
                );
            }
        }
    }
    for sec in NODE_SECTIONS {
        for (_, r) in doc.rows(sec) {
            let Some(n) = r.value(0) else { continue };
            let key = fold(n);
            // A model with no links yet is just points; every node would be
            // an orphan, so the check waits until something is connected.
            if !link_infos.is_empty() && !attached.contains(&key) {
                out.push(Finding::new(
                    Severity::Warning,
                    sec,
                    n,
                    "no link connects to this node",
                ));
            }
            if *sec == "OUTFALLS" {
                let n_in = incoming.get(&key).map_or(0, Vec::len);
                let n_out = outgoing.get(&key).map_or(0, Vec::len);
                if n_in > 0 && n_out > 0 {
                    out.push(Finding::new(
                        Severity::Error,
                        sec,
                        n,
                        format!(
                            "outfall has {n_in} incoming and {n_out} outgoing link(s); an outfall takes one incoming link and none out (ERROR 141)"
                        ),
                    ));
                } else if n_in + n_out > 1 {
                    out.push(Finding::new(
                        Severity::Error,
                        sec,
                        n,
                        format!(
                            "outfall has {} links; an outfall takes exactly one (ERROR 141)",
                            n_in + n_out
                        ),
                    ));
                }
            }
            if *sec == "JUNCTIONS" {
                if let Some(outs) = outgoing.get(&key) {
                    if !outs.is_empty()
                        && outs
                            .iter()
                            .all(|l| l.section == "WEIRS" || l.section == "ORIFICES")
                    {
                        let names: Vec<&str> = outs.iter().map(|l| l.name.as_str()).collect();
                        out.push(Finding::new(
                            Severity::Warning,
                            sec,
                            n,
                            format!(
                                "its only outgoing link(s) {} are weirs/orifices: nothing leaves below the crest or offset, so the junction fills and floods at low flow",
                                names.join(", ")
                            ),
                        ));
                    }
                }
            }
        }
    }

    // Offsets and conduit length.
    let elevation_mode = doc
        .option("LINK_OFFSETS")
        .is_some_and(|v| v.eq_ignore_ascii_case("ELEVATION"));
    let offset_columns: [(&str, &[(&str, usize)]); 4] = [
        ("CONDUITS", &[("InOffset", 1), ("OutOffset", 2)]),
        ("ORIFICES", &[("Offset", 1)]),
        ("WEIRS", &[("CrestHt", 1)]),
        ("OUTLETS", &[("Offset", 1)]),
    ];
    for (sec, cols) in offset_columns {
        for (_, r) in doc.rows(sec) {
            let name = r.value(0).unwrap_or("");
            let layout = doc.columns(sec, r);
            if sec == "CONDUITS" {
                if let Some(len) = num(r.get(layout, "Length")) {
                    if len <= 0.0 {
                        out.push(
                            Finding::new(
                                Severity::Error,
                                sec,
                                name,
                                "length is zero or negative (ERROR 111)",
                            )
                            .column("Length"),
                        );
                    }
                }
            }
            for (col, end_field) in cols {
                let Some(raw) = r.get(layout, col) else { continue };
                let Some(value) = num(Some(raw)) else { continue };
                let Some(end) = r.value(*end_field) else { continue };
                let Some(node) = node_infos.get(&fold(end)) else { continue };
                let Some(invert) = node.invert else { continue };
                let depth = if elevation_mode {
                    if value == 0.0 {
                        continue;
                    }
                    value - invert
                } else {
                    value
                };
                if depth < -1e-9 {
                    let how = if elevation_mode {
                        format!("elevation {value} is below the invert {invert} of node {end}")
                    } else {
                        format!("offset {value} is negative")
                    };
                    out.push(
                        Finding::new(
                            Severity::Warning,
                            sec,
                            name,
                            format!(
                                "{col}: {how}; the engine silently uses 0 (pipe invert = node invert) and raises the rim if it must (WARNING 03)"
                            ),
                        )
                        .column(col),
                    );
                    continue;
                }
                if let Some(max_depth) = node.max_depth {
                    if max_depth > 0.0 && depth > max_depth + 1e-9 {
                        out.push(
                            Finding::new(
                                Severity::Warning,
                                sec,
                                name,
                                format!(
                                    "{col}: invert at {} is above the rim of {} {end} (invert {invert} + MaxDepth {max_depth} = {}); the engine will raise the node's MaxDepth to fit (WARNING 02)",
                                    invert + depth,
                                    node.section.to_ascii_lowercase().trim_end_matches('s'),
                                    invert + max_depth
                                ),
                            )
                            .column(col),
                        );
                    }
                }
            }
        }
    }

    // Cross-sections.
    let xsections = index(doc, &["XSECTIONS"]);
    for (_, r) in doc.rows("XSECTIONS") {
        if let Some(n) = r.value(0) {
            if !known(&links, n) {
                out.push(Finding::new(
                    Severity::Warning,
                    "XSECTIONS",
                    n,
                    format!("link {n:?} does not exist"),
                ));
            }
        }
    }
    for sec in ["CONDUITS", "ORIFICES", "WEIRS"] {
        for (_, r) in doc.rows(sec) {
            if let Some(n) = r.value(0) {
                if !known(&xsections, n) {
                    out.push(Finding::new(Severity::Error, sec, n, "no [XSECTIONS] row"));
                }
            }
        }
    }

    // Time series named but empty.
    for name in doc.names("TIMESERIES") {
        let rows = doc.find_all("TIMESERIES", &name);
        let is_file = rows
            .iter()
            .any(|r| r.fields.get(1).is_some_and(|f| f.eq_ignore_ascii_case("FILE")));
        if !is_file && doc.timeseries(&name).is_empty() {
            out.push(Finding::new(
                Severity::Warning,
                "TIMESERIES",
                &name,
                "has no points; anything that reads it gets nothing",
            ));
        }
    }

    // [OPTIONS] sanity.
    let opt = |k: &str| doc.option(k);
    if let (Some(start), Some(end)) = (
        datetime(opt("START_DATE"), opt("START_TIME")),
        datetime(opt("END_DATE"), opt("END_TIME")),
    ) {
        if end <= start {
            out.push(
                Finding::new(
                    Severity::Error,
                    "OPTIONS",
                    "END_DATE",
                    "simulation ends before (or when) it starts (ERROR 191)",
                )
                .column("Value"),
            );
        }
    }
    if let (Some(routing), Some(report)) = (
        opt("ROUTING_STEP").and_then(step_seconds),
        opt("REPORT_STEP").and_then(step_seconds),
    ) {
        if routing > report {
            out.push(
                Finding::new(
                    Severity::Error,
                    "OPTIONS",
                    "REPORT_STEP",
                    format!(
                        "REPORT_STEP ({report} s) is shorter than ROUTING_STEP ({routing} s) (ERROR 195)"
                    ),
                )
                .column("Value"),
            );
        }
    }
    if let (Some(wet), Some(dry)) = (
        opt("WET_STEP").and_then(step_seconds),
        opt("DRY_STEP").and_then(step_seconds),
    ) {
        if wet > dry {
            out.push(
                Finding::new(
                    Severity::Warning,
                    "OPTIONS",
                    "DRY_STEP",
                    format!(
                        "DRY_STEP ({dry} s) is shorter than WET_STEP ({wet} s); the engine raises it to WET_STEP (WARNING 06)"
                    ),
                )
                .column("Value"),
            );
        }
    }

    // Short rows.
    for s in doc.sections() {
        let Some(min) = schema::min_fields(&s.name) else {
            continue;
        };
        for (li, r) in s.rows() {
            if r.fields.len() < min {
                let name = r
                    .value(schema::name_index(&s.name).unwrap_or(0))
                    .unwrap_or("");
                out.push(Finding::new(
                    Severity::Error,
                    &s.name,
                    name,
                    format!(
                        "line {} has {} fields; at least {min} are needed",
                        li + 1,
                        r.fields.len()
                    ),
                ));
            }
        }
    }

    validate_hydrology_quality(doc, &mut out);

    out
}

// --- hydrology and water-quality sections ------------------------------------------
//
// LID controls and usage, aquifers and groundwater, snow packs, buildup,
// washoff, coverages, loadings, treatment, unit hydrographs and RDII. The
// rules are the engine's own readers and validators (`lid.c`, `gwater.c`,
// `snow.c`, `landuse.c`, `subcatch.c`, `treatmnt.c`, `rdii.c`) and the
// column meanings of the SWMM 5.2 User's Manual, Appendix D.

/// The variables a `[GWF]` expression may use (`gwater.c` `GWVarWords`):
/// water table height, surface water height, channel bottom height, ground
/// surface height, saturated and unsaturated conductivity, upper-zone
/// moisture, porosity, surface infiltration, upper-zone percolation, area.
pub const GW_VARIABLES: &[&str] = &[
    "HGW", "HSW", "HCB", "HGS", "KS", "K", "THETA", "PHI", "FI", "FU", "A",
];

/// The process variables a `[TREATMENT]` expression may use (`treatmnt.c`
/// `ProcessVarWords`): hydraulic residence time, time step, flow, depth,
/// surface area. Pollutant names and `R_<pollutant>` are the others.
pub const TREATMENT_VARIABLES: &[&str] = &["HRT", "DT", "FLOW", "DEPTH", "AREA"];

/// The functions the engine's expression parser (`mathexpr.c`) knows.
pub const MATH_FUNCTIONS: &[&str] = &[
    "COS", "SIN", "TAN", "COT", "ABS", "SGN", "SQRT", "LOG", "EXP", "ASIN", "ACOS", "ATAN",
    "ACOT", "SINH", "COSH", "TANH", "COTH", "LOG10", "STEP",
];

/// The identifiers in an expression: runs of letters, digits, `_` and `.`
/// that start with a letter or `_`. Numbers (`2`, `0.5`, `1e-3`) are not
/// identifiers; operators and parentheses are skipped.
pub fn expression_identifiers(expr: &str) -> Vec<String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
        } else if c.is_ascii_digit() || c == '.' {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

fn is_word(list: &[&str], s: &str) -> bool {
    list.iter().any(|w| w.eq_ignore_ascii_case(s))
}

/// The identifiers of a `[GWF]` expression the engine does not know.
pub fn unknown_gwf_identifiers(expr: &str) -> Vec<String> {
    expression_identifiers(expr)
        .into_iter()
        .filter(|id| !is_word(GW_VARIABLES, id) && !is_word(MATH_FUNCTIONS, id))
        .collect()
}

/// What is wrong with a `[TREATMENT]` expression, if anything: it must
/// begin with `R` (removal fraction) or `C` (outlet concentration), have an
/// `=`, and use only process variables, pollutant names, `R_<pollutant>`
/// and the parser's functions (`treatmnt.c` `treatmnt_readExpression`,
/// `getVariableIndex`).
pub fn check_treatment_expression(expr: &str, pollutants: &[String]) -> Result<(), String> {
    let e = expr.trim();
    let Some(first) = e.chars().next() else {
        return Err("the expression is empty".into());
    };
    if !matches!(first.to_ascii_uppercase(), 'R' | 'C') {
        return Err(format!(
            "must start with R (removal fraction) or C (outlet concentration), not {first:?}"
        ));
    }
    let Some((_, rhs)) = e.split_once('=') else {
        return Err("has no '=' (write R = ... or C = ...)".into());
    };
    if rhs.trim().is_empty() {
        return Err("nothing follows the '='".into());
    }
    let is_pollutant = |s: &str| pollutants.iter().any(|p| p.eq_ignore_ascii_case(s));
    let unknown: Vec<String> = expression_identifiers(rhs)
        .into_iter()
        .filter(|id| {
            !(is_word(TREATMENT_VARIABLES, id)
                || is_word(MATH_FUNCTIONS, id)
                || is_pollutant(id)
                || (id.len() > 2
                    && id[..2].eq_ignore_ascii_case("R_")
                    && is_pollutant(&id[2..])))
        })
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "unknown variable(s) {}; allowed: HRT, DT, FLOW, DEPTH, AREA, a pollutant name, R_<pollutant>",
            unknown.join(", ")
        ))
    }
}

fn not_number(fs: &mut Vec<Finding>, sec: &str, name: &str, col: &str, v: &str) {
    fs.push(
        Finding::new(
            Severity::Error,
            sec,
            name,
            format!("{col} {v:?} is not a number"),
        )
        .column(col),
    );
}

/// Every field of the row's layout from column `from` on that the layout
/// names must be a number; reports the ones that are not.
fn numeric_from(
    fs: &mut Vec<Finding>,
    doc: &InpDoc,
    sec: &str,
    name: &str,
    r: &super::Row,
    from: usize,
    skip: &[&str],
) {
    let cols = doc.columns(sec, r);
    for (i, col) in cols.iter().enumerate().skip(from) {
        if skip.contains(col) {
            continue;
        }
        let Some(v) = r.value(i) else { break };
        if v.trim() == "*" && i >= 10 {
            continue; // optional [GROUNDWATER] elevations left unset
        }
        if v.trim().parse::<f64>().is_err() {
            not_number(fs, sec, name, col, v);
        }
    }
}

fn validate_hydrology_quality(doc: &InpDoc, out: &mut Vec<Finding>) {
    let subs = index(doc, &["SUBCATCHMENTS"]);
    let nodes = index(doc, NODE_SECTIONS);
    let series = index(doc, &["TIMESERIES"]);
    let pollutants = index(doc, &["POLLUTANTS"]);
    let landuses = index(doc, &["LANDUSES"]);
    let aquifers = index(doc, &["AQUIFERS"]);
    let snowpacks = index(doc, &["SNOWPACKS"]);
    let hydrographs = index(doc, &["HYDROGRAPHS"]);
    let pollutant_names = doc.names("POLLUTANTS");

    // [LID_CONTROLS]: a type row per process, known layer keywords, the
    // fields each reader needs, the layers the type requires.
    let mut lid_types: HashMap<String, String> = HashMap::new();
    for name in doc.names("LID_CONTROLS") {
        let rows = doc.find_all("LID_CONTROLS", &name);
        let kind = rows
            .iter()
            .find(|r| r.fields.len() == 2)
            .and_then(|r| r.value(1))
            .map(|k| k.to_ascii_uppercase());
        match &kind {
            None => out.push(Finding::new(
                Severity::Error,
                "LID_CONTROLS",
                &name,
                "no `Name Type` row: the engine cannot tell what kind of LID this is (ERROR 183)",
            )),
            Some(k) if !is_word(schema::LID_TYPES, k) => out.push(
                Finding::new(
                    Severity::Error,
                    "LID_CONTROLS",
                    &name,
                    format!(
                        "unknown LID type {k:?}; one of {}",
                        schema::LID_TYPES.join(", ")
                    ),
                )
                .column("Type"),
            ),
            Some(k) => {
                lid_types.insert(fold(&name), k.clone());
            }
        }
        let kind = kind.filter(|k| is_word(schema::LID_TYPES, k));
        let mut layers_present: Vec<String> = Vec::new();
        for r in rows.iter().filter(|r| r.fields.len() != 2) {
            let layer = r.value(1).unwrap_or("").to_ascii_uppercase();
            if !is_word(schema::LID_LAYERS, &layer) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "LID_CONTROLS",
                        &name,
                        format!(
                            "unknown layer keyword {layer:?}; one of {}",
                            schema::LID_LAYERS.join(", ")
                        ),
                    )
                    .column("Layer"),
                );
                continue;
            }
            layers_present.push(layer.clone());
            let min = build::lid_layer_min_fields(&layer);
            if r.fields.len() < min {
                out.push(Finding::new(
                    Severity::Error,
                    "LID_CONTROLS",
                    &name,
                    format!(
                        "{layer} row has {} fields; the engine reads at least {min}",
                        r.fields.len()
                    ),
                ));
                continue;
            }
            if let Some(k) = &kind {
                if !build::lid_layers_for(k).iter().any(|(l, _)| *l == layer) {
                    out.push(Finding::new(
                        Severity::Warning,
                        "LID_CONTROLS",
                        &name,
                        format!(
                            "a {layer} layer is not used by a {} ({k}); the engine ignores it",
                            build::lid_type_label(k)
                        ),
                    ));
                }
            }
            match layer.as_str() {
                "REMOVALS" => {
                    for pair in r.fields[2..].chunks(2) {
                        let p = unquote(&pair[0]);
                        if !known(&pollutants, p) {
                            out.push(
                                Finding::new(
                                    Severity::Error,
                                    "LID_CONTROLS",
                                    &name,
                                    format!("REMOVALS names pollutant {p:?}, which does not exist"),
                                )
                                .column("Pollutant"),
                            );
                        }
                        match pair.get(1).map(|s| unquote(s)) {
                            Some(v) => match v.trim().parse::<f64>() {
                                Ok(x) if (0.0..=100.0).contains(&x) => {}
                                Ok(_) => out.push(
                                    Finding::new(
                                        Severity::Error,
                                        "LID_CONTROLS",
                                        &name,
                                        format!("REMOVALS: {v} % for {p} is not between 0 and 100"),
                                    )
                                    .column("Removal"),
                                ),
                                Err(_) => not_number(out, "LID_CONTROLS", &name, "Removal", v),
                            },
                            None => out.push(Finding::new(
                                Severity::Error,
                                "LID_CONTROLS",
                                &name,
                                format!("REMOVALS: no removal percentage after {p}"),
                            )),
                        }
                    }
                }
                "STORAGE" => numeric_from(out, doc, "LID_CONTROLS", &name, r, 2, &["Covrd"]),
                "DRAIN" => numeric_from(out, doc, "LID_CONTROLS", &name, r, 2, &["Qcurve"]),
                _ => numeric_from(out, doc, "LID_CONTROLS", &name, r, 2, &[]),
            }
            if layer == "SURFACE" {
                if let Some(v) = num(r.value(3)) {
                    if v >= 1.0 {
                        out.push(
                            Finding::new(
                                Severity::Error,
                                "LID_CONTROLS",
                                &name,
                                "SURFACE VegFrac (vegetation volume fraction) must be below 1 (ERROR 185)",
                            )
                            .column("VegFrac"),
                        );
                    }
                }
            }
        }
        if let Some(k) = &kind {
            for (layer, required) in build::lid_layers_for(k) {
                if *required && !layers_present.iter().any(|l| l == layer) {
                    out.push(Finding::new(
                        Severity::Error,
                        "LID_CONTROLS",
                        &name,
                        format!(
                            "a {} ({k}) needs a {layer} layer (ERROR 184)",
                            build::lid_type_label(k)
                        ),
                    ));
                }
            }
        }
    }

    // [LID_USAGE]: known LID, sane numbers, a drain target that exists,
    // and the LID area within the subcatchment.
    for (_, r) in doc.rows("LID_USAGE") {
        let name = r.value(0).unwrap_or("");
        match r.value(1) {
            Some(l) if lid_types.contains_key(&fold(l)) => {}
            Some(l) => out.push(
                Finding::new(
                    Severity::Error,
                    "LID_USAGE",
                    name,
                    format!("LID names LID control {l:?}, which does not exist"),
                )
                .column("LID"),
            ),
            None => {}
        }
        numeric_from(out, doc, "LID_USAGE", name, r, 2, &["RptFile", "DrainTo"]);
        for (i, col) in [(5, "InitSat"), (6, "FromImp"), (10, "FromPerv")] {
            if let Some(v) = num(r.value(i)) {
                if !(0.0..=100.0).contains(&v) {
                    out.push(
                        Finding::new(
                            Severity::Error,
                            "LID_USAGE",
                            name,
                            format!("{col} is a percentage (0 to 100), not {v}"),
                        )
                        .column(col),
                    );
                }
            }
        }
        if let Some(d) = r.value(9) {
            let d = d.trim();
            if !d.is_empty() && d != "*" && !known(&nodes, d) && !known(&subs, d) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "LID_USAGE",
                        name,
                        format!("DrainTo names {d:?}, which is neither a node nor a subcatchment"),
                    )
                    .column("DrainTo"),
                );
            }
        }
    }
    for name in doc.names("LID_USAGE") {
        if let Some((area, total)) = build::lid_area_check(doc, &name) {
            if total > area * (1.0 + 1e-6) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "LID_USAGE",
                        &name,
                        format!(
                            "LID units cover {} but the subcatchment is {} (Number × Area, in the LID area unit); the engine refuses it (ERROR 187)",
                            super::format_number(total),
                            super::format_number(area)
                        ),
                    )
                    .column("Area"),
                );
            }
        }
    }

    // [AQUIFERS]: numbers, and the relations gwater.c checks (ERROR 109).
    for (_, r) in doc.rows("AQUIFERS") {
        let name = r.value(0).unwrap_or("");
        numeric_from(out, doc, "AQUIFERS", name, r, 1, &["ETupat"]);
        let (por, wp, fc, ksat) = (num(r.value(1)), num(r.value(2)), num(r.value(3)), num(r.value(4)));
        if let (Some(por), Some(wp), Some(fc), Some(ksat)) = (por, wp, fc, ksat) {
            let bad = if por <= 0.0 {
                Some("porosity must be positive")
            } else if fc >= por {
                Some("field capacity must be below the porosity")
            } else if wp >= fc {
                Some("wilting point must be below the field capacity")
            } else if ksat <= 0.0 {
                Some("conductivity must be positive")
            } else {
                None
            };
            if let Some(why) = bad {
                out.push(Finding::new(
                    Severity::Error,
                    "AQUIFERS",
                    name,
                    format!("{why} (ERROR 109)"),
                ));
            }
        }
    }

    // [GROUNDWATER]: a known aquifer, numbers.
    for (_, r) in doc.rows("GROUNDWATER") {
        let name = r.value(0).unwrap_or("");
        match r.value(1) {
            Some(a) if known(&aquifers, a) => {}
            Some(a) => out.push(
                Finding::new(
                    Severity::Error,
                    "GROUNDWATER",
                    name,
                    format!("Aquifer names aquifer {a:?}, which does not exist"),
                )
                .column("Aquifer"),
            ),
            None => {}
        }
        numeric_from(out, doc, "GROUNDWATER", name, r, 3, &[]);
    }

    // [GWF]: LATERAL or DEEP, and an expression over the engine's variables.
    for (_, r) in doc.rows("GWF") {
        let name = r.value(0).unwrap_or("");
        let kind = r.value(1).unwrap_or("").to_ascii_uppercase();
        if !(kind.starts_with("LAT") || kind == "DEEP") {
            out.push(
                Finding::new(
                    Severity::Error,
                    "GWF",
                    name,
                    format!("Type must be LATERAL or DEEP, not {kind:?}"),
                )
                .column("Type"),
            );
        }
        let expr = build::expression_of(&r.fields);
        if expr.trim().is_empty() {
            out.push(Finding::new(Severity::Error, "GWF", name, "the expression is empty (ERROR 233)"));
        } else {
            let unknown = unknown_gwf_identifiers(&expr);
            if !unknown.is_empty() {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "GWF",
                        name,
                        format!(
                            "unknown variable(s) {} in the {} expression; allowed: {} (ERROR 233)",
                            unknown.join(", "),
                            kind,
                            GW_VARIABLES.join(", ")
                        ),
                    )
                    .column("Expression"),
                );
            }
        }
    }

    // [SNOWPACKS]: keyword rows with their numbers; subcatchments name a
    // pack that exists.
    for (_, r) in doc.rows("SNOWPACKS") {
        let name = r.value(0).unwrap_or("");
        let layer = r.value(1).unwrap_or("").to_ascii_uppercase();
        if !is_word(schema::SNOWPACK_LAYERS, &layer) {
            out.push(
                Finding::new(
                    Severity::Error,
                    "SNOWPACKS",
                    name,
                    format!(
                        "unknown keyword {layer:?}; one of {}",
                        schema::SNOWPACK_LAYERS.join(", ")
                    ),
                )
                .column("Layer"),
            );
            continue;
        }
        if layer != "REMOVAL" && r.fields.len() < 9 {
            out.push(Finding::new(
                Severity::Error,
                "SNOWPACKS",
                name,
                format!(
                    "{layer} row has {} fields; the engine reads 9 (name, keyword, Cmin, Cmax, Tbase, FWF, SD0, FW0, {})",
                    r.fields.len(),
                    if layer == "PLOWABLE" { "SNN0" } else { "SD100" }
                ),
            ));
            continue;
        }
        numeric_from(out, doc, "SNOWPACKS", name, r, 2, &["Scatch"]);
    }
    for (_, r) in doc.rows("SUBCATCHMENTS") {
        if let Some(p) = r.value(8) {
            let p = p.trim();
            if !p.is_empty() && p != "*" && !known(&snowpacks, p) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "SUBCATCHMENTS",
                        r.value(0).unwrap_or(""),
                        format!("SnowPack names snow pack {p:?}, which does not exist"),
                    )
                    .column("SnowPack"),
                );
            }
        }
    }

    // [HYDROGRAPHS]: a gage row per set, known months and responses,
    // ratios that sum to at most 1 per month (rdii.c validateRdii).
    for name in doc.names("HYDROGRAPHS") {
        let rows = doc.find_all("HYDROGRAPHS", &name);
        if !rows.iter().any(|r| r.fields.len() == 2) {
            out.push(Finding::new(
                Severity::Error,
                "HYDROGRAPHS",
                &name,
                "no `Name RainGage` row: the set has no rain gage",
            ));
        }
        let mut r_sum: HashMap<String, f64> = HashMap::new();
        for r in rows.iter().filter(|r| r.fields.len() != 2) {
            let month = r.value(1).unwrap_or("").to_ascii_uppercase();
            let month_ok = month == "ALL"
                || schema::UH_MONTHS
                    .iter()
                    .any(|m| *m != "ALL" && month.starts_with(*m));
            if !month_ok {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "HYDROGRAPHS",
                        &name,
                        format!("Month must be ALL or a month name, not {month:?}"),
                    )
                    .column("Month"),
                );
            }
            let resp = r.value(2).unwrap_or("").to_ascii_uppercase();
            if !is_word(schema::UH_RESPONSES, &resp) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "HYDROGRAPHS",
                        &name,
                        format!("Response must be SHORT, MEDIUM or LONG, not {resp:?}"),
                    )
                    .column("Response"),
                );
            }
            if r.fields.len() < 6 {
                out.push(Finding::new(
                    Severity::Error,
                    "HYDROGRAPHS",
                    &name,
                    format!("row has {} fields; the engine reads at least 6 (name, month, response, R, T, K)", r.fields.len()),
                ));
                continue;
            }
            numeric_from(out, doc, "HYDROGRAPHS", &name, r, 3, &[]);
            if let Some(rv) = num(r.value(3)) {
                if !(0.0..=1.0).contains(&rv) {
                    out.push(
                        Finding::new(
                            Severity::Error,
                            "HYDROGRAPHS",
                            &name,
                            format!("R (response ratio) must be between 0 and 1, not {rv} (ERROR 153)"),
                        )
                        .column("R"),
                    );
                } else {
                    *r_sum.entry(month.clone()).or_insert(0.0) += rv;
                }
            }
            if let Some(t) = num(r.value(4)) {
                if t < 0.0 {
                    out.push(
                        Finding::new(
                            Severity::Error,
                            "HYDROGRAPHS",
                            &name,
                            "T (time to peak) cannot be negative (ERROR 151)",
                        )
                        .column("T"),
                    );
                }
            }
        }
        for (month, sum) in r_sum {
            if sum > 1.01 {
                out.push(Finding::new(
                    Severity::Error,
                    "HYDROGRAPHS",
                    &name,
                    format!(
                        "the SHORT + MEDIUM + LONG response ratios for {month} sum to {}; at most 1 (ERROR 153)",
                        super::format_number(sum)
                    ),
                ));
            }
        }
    }

    // [RDII]: a known unit hydrograph set and a sewer area.
    for (_, r) in doc.rows("RDII") {
        let name = r.value(0).unwrap_or("");
        match r.value(1) {
            Some(u) if known(&hydrographs, u) => {}
            Some(u) => out.push(
                Finding::new(
                    Severity::Error,
                    "RDII",
                    name,
                    format!("UnitHydrograph names unit hydrograph set {u:?}, which does not exist"),
                )
                .column("UnitHydrograph"),
            ),
            None => {}
        }
        if let Some(a) = r.value(2) {
            match a.trim().parse::<f64>() {
                Ok(v) if v < 0.0 => out.push(
                    Finding::new(
                        Severity::Error,
                        "RDII",
                        name,
                        "SewerArea cannot be negative (ERROR 155)",
                    )
                    .column("SewerArea"),
                ),
                Ok(_) => {}
                Err(_) => not_number(out, "RDII", name, "SewerArea", a),
            }
        }
    }

    // [BUILDUP] and [WASHOFF]: known land use and pollutant, known
    // function, the fields the function needs.
    for (sec, functions) in [
        ("BUILDUP", schema::BUILDUP_FUNCTIONS),
        ("WASHOFF", schema::WASHOFF_FUNCTIONS),
    ] {
        for (_, r) in doc.rows(sec) {
            let name = r.value(0).unwrap_or("");
            if !known(&landuses, name) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        sec,
                        name,
                        format!("LandUse names land use {name:?}, which does not exist"),
                    )
                    .column("LandUse"),
                );
            }
            match r.value(1) {
                Some(p) if known(&pollutants, p) => {}
                Some(p) => out.push(
                    Finding::new(
                        Severity::Error,
                        sec,
                        name,
                        format!("Pollutant names pollutant {p:?}, which does not exist"),
                    )
                    .column("Pollutant"),
                ),
                None => {}
            }
            let func = r.value(2).unwrap_or("NONE").to_ascii_uppercase();
            if !is_word(functions, &func) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        sec,
                        name,
                        format!("unknown function {func:?}; one of {}", functions.join(", ")),
                    )
                    .column("Function"),
                );
                continue;
            }
            if func == "NONE" {
                continue;
            }
            let need = if sec == "BUILDUP" { 7 } else { 5 };
            if r.fields.len() < need {
                out.push(Finding::new(
                    Severity::Error,
                    sec,
                    name,
                    format!(
                        "{func} row has {} fields; the engine reads {need}",
                        r.fields.len()
                    ),
                ));
                continue;
            }
            if sec == "BUILDUP" {
                let skip: &[&str] = if func == "EXT" { &["Coeff3", "PerUnit"] } else { &["PerUnit"] };
                numeric_from(out, doc, sec, name, r, 3, skip);
                let unit = r.value(6).unwrap_or("").to_ascii_uppercase();
                if !is_word(schema::BUILDUP_NORMALIZERS, &unit) {
                    out.push(
                        Finding::new(
                            Severity::Error,
                            sec,
                            name,
                            format!("PerUnit must be AREA or CURB, not {unit:?}"),
                        )
                        .column("PerUnit"),
                    );
                }
                if func == "EXT" {
                    let ts = r.value(5).unwrap_or("");
                    if !known(&series, ts) {
                        out.push(
                            Finding::new(
                                Severity::Error,
                                sec,
                                name,
                                format!("EXT buildup names time series {ts:?}, which does not exist"),
                            )
                            .column("Coeff3"),
                        );
                    }
                }
            } else {
                numeric_from(out, doc, sec, name, r, 3, &[]);
                for (i, col) in [(5, "SweepRmvl"), (6, "BmpRmvl")] {
                    if let Some(v) = num(r.value(i)) {
                        if !(0.0..=100.0).contains(&v) {
                            out.push(
                                Finding::new(
                                    Severity::Error,
                                    sec,
                                    name,
                                    format!("{col} is a percentage (0 to 100), not {v}"),
                                )
                                .column(col),
                            );
                        }
                    }
                }
            }
        }
    }

    // [COVERAGES]: known land uses, percentages, at most 100 % in all.
    for (_, r) in doc.rows("COVERAGES") {
        let name = r.value(0).unwrap_or("");
        for pair in r.fields[1.min(r.fields.len())..].chunks(2) {
            let lu = unquote(&pair[0]);
            if !known(&landuses, lu) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "COVERAGES",
                        name,
                        format!("LandUse names land use {lu:?}, which does not exist"),
                    )
                    .column("LandUse"),
                );
            }
            match pair.get(1).map(|s| unquote(s)) {
                Some(p) if p.trim().parse::<f64>().is_err() => {
                    not_number(out, "COVERAGES", name, "Percent", p)
                }
                Some(_) => {}
                None => out.push(Finding::new(
                    Severity::Error,
                    "COVERAGES",
                    name,
                    format!("no percentage after land use {lu}"),
                )),
            }
        }
    }
    for name in doc.names("COVERAGES") {
        let total = build::coverage_total(doc, &name);
        if total > 100.0 + 1e-6 {
            out.push(
                Finding::new(
                    Severity::Warning,
                    "COVERAGES",
                    &name,
                    format!(
                        "land uses cover {} % of the subcatchment; more than 100 % over-counts buildup and washoff",
                        super::format_number(total)
                    ),
                )
                .column("Percent"),
            );
        }
    }

    // [LOADINGS]: known pollutants, numbers.
    for (_, r) in doc.rows("LOADINGS") {
        let name = r.value(0).unwrap_or("");
        for pair in r.fields[1.min(r.fields.len())..].chunks(2) {
            let p = unquote(&pair[0]);
            if !known(&pollutants, p) {
                out.push(
                    Finding::new(
                        Severity::Error,
                        "LOADINGS",
                        name,
                        format!("Pollutant names pollutant {p:?}, which does not exist"),
                    )
                    .column("Pollutant"),
                );
            }
            if let Some(b) = pair.get(1).map(|s| unquote(s)) {
                if b.trim().parse::<f64>().is_err() {
                    not_number(out, "LOADINGS", name, "Buildup", b);
                }
            }
        }
    }

    // [TREATMENT]: known pollutant, a well-formed expression.
    for (_, r) in doc.rows("TREATMENT") {
        let name = r.value(0).unwrap_or("");
        match r.value(1) {
            Some(p) if known(&pollutants, p) => {}
            Some(p) => out.push(
                Finding::new(
                    Severity::Error,
                    "TREATMENT",
                    name,
                    format!("Pollutant names pollutant {p:?}, which does not exist"),
                )
                .column("Pollutant"),
            ),
            None => {}
        }
        let expr = build::expression_of(&r.fields);
        if let Err(e) = check_treatment_expression(&expr, &pollutant_names) {
            out.push(
                Finding::new(
                    Severity::Error,
                    "TREATMENT",
                    name,
                    format!("expression for {}: {e} (ERROR 233)", r.value(1).unwrap_or("")),
                )
                .column("Expression"),
            );
        }
    }

    // [INFLOWS] and [DWF]: a constituent is FLOW or a pollutant.
    for sec in ["INFLOWS", "DWF"] {
        for (_, r) in doc.rows(sec) {
            let name = r.value(0).unwrap_or("");
            if let Some(c) = r.value(1) {
                if !c.eq_ignore_ascii_case("FLOW") && !known(&pollutants, c) {
                    out.push(
                        Finding::new(
                            Severity::Error,
                            sec,
                            name,
                            format!("Constituent {c:?} is neither FLOW nor a pollutant"),
                        )
                        .column("Constituent"),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(text: &str) -> Vec<Finding> {
        validate(&InpDoc::parse(text))
    }

    fn has(fs: &[Finding], section: &str, name: &str, needle: &str) -> bool {
        fs.iter()
            .any(|f| f.section == section && f.name == name && f.message.contains(needle))
    }

    /// `rows` added at the top of `[sec]` (a document has one section of
    /// each name; a second header would be ignored by `rows()`), or a new
    /// section at the end.
    fn with(text: &str, sec: &str, rows: &str) -> String {
        let header = format!("[{sec}]\n");
        if text.contains(&header) {
            text.replacen(&header, &format!("{header}{rows}\n"), 1)
        } else {
            format!("{text}{header}{rows}\n")
        }
    }

    /// Two junctions joined by one pipe with a cross-section, coordinates
    /// and an outfall: the base every negative test edits.
    const CLEAN: &str = "[OPTIONS]\nFLOW_UNITS CFS\nLINK_OFFSETS DEPTH\nSTART_DATE 01/01/2020\nSTART_TIME 00:00:00\nEND_DATE 01/02/2020\nEND_TIME 00:00:00\nREPORT_STEP 00:05:00\nWET_STEP 00:01:00\nDRY_STEP 01:00:00\nROUTING_STEP 0:00:15\n\
[JUNCTIONS]\nJ1 100 5 0 0 0\n[OUTFALLS]\nO1 95 FREE NO\n[CONDUITS]\nC1 J1 O1 100 0.013 0 0 0 0\n[XSECTIONS]\nC1 CIRCULAR 1 0 0 0 1\n[COORDINATES]\nJ1 0 0\nO1 100 0\n";

    #[test]
    fn a_clean_model_has_no_findings() {
        let fs = findings(CLEAN);
        assert!(fs.is_empty(), "{fs:?}");
    }

    #[test]
    fn references_of_every_kind_are_checked() {
        let mut text = with(CLEAN, "INFLOWS", "J1 FLOW TS9 FLOW 1.0 1.0 0 PAT9");
        text = with(&text, "DWF", "J1 FLOW 2 \"\" PATX");
        text = with(&text, "STORAGE", "S1 90 10 0 TABULAR CRV9 0 0");
        text = with(&text, "COORDINATES", "S1 5 5");
        text = with(&text, "CONDUITS", "C2 J1 S1 10 0.01 0 0");
        text = with(&text, "XSECTIONS", "C2 CIRCULAR 1 0 0 0");
        text = with(&text, "LOSSES", "CX 0 0 0 NO 0");
        text = with(&text, "LABELS", "1 2 \"hi\" JX");
        text = with(&text, "SUBAREAS", "SX 0.01 0.1 0.05 0.05 25 OUTLET");
        let fs = findings(&text);
        assert!(has(&fs, "INFLOWS", "J1", "Series names time series \"TS9\""), "{fs:?}");
        assert!(has(&fs, "INFLOWS", "J1", "Pattern names pattern \"PAT9\""), "{fs:?}");
        assert!(has(&fs, "DWF", "J1", "Pattern2 names pattern \"PATX\""), "{fs:?}");
        assert!(!has(&fs, "DWF", "J1", "Pattern1"), "empty \"\" is not a reference: {fs:?}");
        assert!(has(&fs, "STORAGE", "S1", "Curve names curve \"CRV9\""), "{fs:?}");
        assert!(has(&fs, "LOSSES", "CX", "Link names link \"CX\""), "{fs:?}");
        assert!(has(&fs, "LABELS", "", "Anchor names node \"JX\""), "{fs:?}");
        assert!(
            has(&fs, "SUBAREAS", "SX", "Subcatchment names subcatchment \"SX\""),
            "{fs:?}"
        );
        let f = fs
            .iter()
            .find(|f| f.section == "INFLOWS" && f.message.contains("TS9"))
            .unwrap();
        assert_eq!(f.column.as_deref(), Some("Series"));
        assert_eq!(f.severity, Severity::Error);
    }

    #[test]
    fn orphan_nodes_and_undrawable_links_are_warnings() {
        let text = with(&with(CLEAN, "JUNCTIONS", "J2 100 5"), "COORDINATES", "J2 1 1");
        let fs = findings(&text);
        assert!(has(&fs, "JUNCTIONS", "J2", "no link connects"), "{fs:?}");
        assert!(!has(&fs, "JUNCTIONS", "J1", "no link connects"));

        let text = CLEAN.replace("O1 100 0\n", "");
        let fs = findings(&text);
        assert!(has(&fs, "OUTFALLS", "O1", "no [COORDINATES]"), "{fs:?}");
        let f = fs
            .iter()
            .find(|f| f.section == "CONDUITS" && f.name == "C1")
            .expect("link finding");
        assert!(f.message.contains("has no coordinates"), "{f:?}");
        assert_eq!(f.column.as_deref(), Some("ToNode"));
        assert_eq!(f.severity, Severity::Warning);
    }

    #[test]
    fn names_differing_only_by_case_are_duplicates() {
        let text = with(&with(CLEAN, "JUNCTIONS", "j1 101 5"), "COORDINATES", "j1 2 2");
        let fs = findings(&text);
        // `with` puts j1 first, so J1 is the one reported as the duplicate.
        assert!(has(&fs, "JUNCTIONS", "J1", "differs only by case from \"j1\""), "{fs:?}");
        let text = with(CLEAN, "STORAGE", "j1 101 5 0 TABULAR X 0 0");
        let fs = findings(&text);
        assert!(
            has(&fs, "STORAGE", "j1", "also defined in [JUNCTIONS] (differing only by case"),
            "{fs:?}"
        );
    }

    #[test]
    fn a_junction_draining_only_through_a_weir_is_flagged() {
        let mut text = with(CLEAN, "JUNCTIONS", "J2 100 5");
        text = with(&text, "WEIRS", "W1 J2 O1 TRANSVERSE 1 3.33 NO 0 0");
        text = with(&text, "XSECTIONS", "W1 RECT_OPEN 1 2 0 0");
        text = with(&text, "COORDINATES", "J2 50 50");
        let fs = findings(&text);
        assert!(
            has(&fs, "JUNCTIONS", "J2", "only outgoing link(s) W1 are weirs/orifices"),
            "{fs:?}"
        );
        assert!(!has(&fs, "JUNCTIONS", "J1", "weirs/orifices"));
        // Two links at the outfall now: that is its own error.
        assert!(has(&fs, "OUTFALLS", "O1", "outfall has 2 links"), "{fs:?}");
    }

    #[test]
    fn outfall_with_incoming_and_outgoing_links_is_an_error() {
        let mut text = with(CLEAN, "JUNCTIONS", "J2 90 5");
        text = with(&text, "CONDUITS", "C2 O1 J2 10 0.013 0 0");
        text = with(&text, "XSECTIONS", "C2 CIRCULAR 1 0 0 0");
        text = with(&text, "COORDINATES", "J2 200 0");
        let fs = findings(&text);
        assert!(has(&fs, "OUTFALLS", "O1", "1 incoming and 1 outgoing"), "{fs:?}");
        assert_eq!(
            fs.iter().find(|f| f.section == "OUTFALLS").unwrap().severity,
            Severity::Error
        );
    }

    #[test]
    fn offsets_negative_or_above_rim_say_what_the_engine_does() {
        let text = CLEAN.replace("C1 J1 O1 100 0.013 0 0 0 0", "C1 J1 O1 100 0.013 -0.5 0 0 0");
        let fs = findings(&text);
        let f = fs.iter().find(|f| f.section == "CONDUITS").unwrap();
        assert!(f.message.contains("offset -0.5 is negative"), "{f:?}");
        assert!(f.message.contains("silently uses 0"), "{f:?}");
        assert_eq!(f.column.as_deref(), Some("InOffset"));

        // Invert 100 + offset 6 is above rim 100 + 5.
        let text = CLEAN.replace("C1 J1 O1 100 0.013 0 0 0 0", "C1 J1 O1 100 0.013 6 0 0 0");
        let fs = findings(&text);
        let f = fs.iter().find(|f| f.section == "CONDUITS").unwrap();
        assert!(
            f.message.contains("invert at 106 is above the rim of junction J1"),
            "{f:?}"
        );
        assert!(f.message.contains("raise the node's MaxDepth"), "{f:?}");

        // In ELEVATION mode an offset below the node invert is the same trap.
        let text = CLEAN
            .replace("LINK_OFFSETS DEPTH", "LINK_OFFSETS ELEVATION")
            .replace("C1 J1 O1 100 0.013 0 0 0 0", "C1 J1 O1 100 0.013 99 95 0 0");
        let fs = findings(&text);
        let f = fs.iter().find(|f| f.section == "CONDUITS").unwrap();
        assert!(f.message.contains("elevation 99 is below the invert 100"), "{f:?}");
        // A conforming ELEVATION-mode model is clean.
        let text = CLEAN
            .replace("LINK_OFFSETS DEPTH", "LINK_OFFSETS ELEVATION")
            .replace("C1 J1 O1 100 0.013 0 0 0 0", "C1 J1 O1 100 0.013 100.5 95 0 0");
        assert!(findings(&text).is_empty(), "{:?}", findings(&text));
        // MaxDepth 0 means "to the highest crown": no rim to check.
        let text = CLEAN
            .replace("J1 100 5 0 0 0", "J1 100 0 0 0 0")
            .replace("C1 J1 O1 100 0.013 0 0 0 0", "C1 J1 O1 100 0.013 6 0 0 0");
        assert!(findings(&text).is_empty(), "{:?}", findings(&text));
    }

    #[test]
    fn zero_length_conduit_is_an_error() {
        let text = CLEAN.replace("C1 J1 O1 100 0.013", "C1 J1 O1 0 0.013");
        let fs = findings(&text);
        assert!(has(&fs, "CONDUITS", "C1", "length is zero"), "{fs:?}");
    }

    #[test]
    fn subcatchment_area_width_gage_use_and_empty_series() {
        let mut text = with(
            CLEAN,
            "RAINGAGES",
            "RG1 INTENSITY 1:00 1.0 TIMESERIES TS1\nRG2 INTENSITY 1:00 1.0 TIMESERIES TS2",
        );
        text = with(&text, "SUBCATCHMENTS", "S1 RG1 J1 0 50 0 0.5 0");
        text = with(&text, "SUBAREAS", "S1 0.01 0.1 0.05 0.05 25 OUTLET");
        text = with(
            &text,
            "TIMESERIES",
            "TS1 0:00 1.0\nTS2 FILE \"rain.dat\"\nTS3 0:00",
        );
        text = with(&text, "POLYGONS", "S1 0 0");
        let fs = findings(&text);
        assert!(has(&fs, "SUBCATCHMENTS", "S1", "area is zero"), "{fs:?}");
        assert!(has(&fs, "SUBCATCHMENTS", "S1", "width is zero"), "{fs:?}");
        assert!(has(&fs, "RAINGAGES", "RG2", "no subcatchment uses"), "{fs:?}");
        assert!(!has(&fs, "RAINGAGES", "RG1", "no subcatchment uses"));
        assert!(has(&fs, "TIMESERIES", "TS3", "has no points"), "{fs:?}");
        assert!(
            !has(&fs, "TIMESERIES", "TS2", "has no points"),
            "a FILE series has no inline points: {fs:?}"
        );
        assert!(!has(&fs, "TIMESERIES", "TS1", "has no points"));
    }

    #[test]
    fn options_sanity() {
        let text = CLEAN.replace("END_DATE 01/02/2020", "END_DATE 12/31/2019");
        let fs = findings(&text);
        assert!(has(&fs, "OPTIONS", "END_DATE", "ends before"), "{fs:?}");
        let text = CLEAN.replace("ROUTING_STEP 0:00:15", "ROUTING_STEP 600");
        let fs = findings(&text);
        assert!(
            has(&fs, "OPTIONS", "REPORT_STEP", "shorter than ROUTING_STEP (600 s)"),
            "{fs:?}"
        );
        let text = CLEAN.replace("DRY_STEP 01:00:00", "DRY_STEP 00:00:30");
        let fs = findings(&text);
        assert!(has(&fs, "OPTIONS", "DRY_STEP", "shorter than WET_STEP"), "{fs:?}");
        assert_eq!(step_seconds("00:05:00"), Some(300.0));
        assert_eq!(step_seconds("1:30"), Some(5400.0));
        assert_eq!(step_seconds("15"), Some(15.0));
        assert_eq!(step_seconds("x"), None);
    }

    #[test]
    fn epa_samples_carry_no_errors() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/epa-samples");
        let mut n = 0;
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("inp") {
                continue;
            }
            let doc = InpDoc::read(&p).unwrap();
            let errors: Vec<Finding> = validate(&doc)
                .into_iter()
                .filter(|f| f.severity == Severity::Error)
                .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", p.display());
            n += 1;
        }
        assert!(n >= 5);
    }
}

#[cfg(test)]
mod chapter21_tests {
    use super::*;

    fn findings(text: &str) -> Vec<Finding> {
        validate(&InpDoc::parse(text))
    }

    fn has(fs: &[Finding], section: &str, name: &str, needle: &str) -> bool {
        fs.iter()
            .any(|f| f.section == section && f.name == name && f.message.contains(needle))
    }

    const BASE: &str = "[OPTIONS]\nFLOW_UNITS CFS\n[RAINGAGES]\nRG1 INTENSITY 1:00 1.0 TIMESERIES TS\n[TIMESERIES]\nTS 0:00 1\n[JUNCTIONS]\nJ1 0\n[OUTFALLS]\nO1 0 FREE NO\n[CONDUITS]\nC1 J1 O1 100 0.01 0 0\n[XSECTIONS]\nC1 CIRCULAR 1 0 0 0\n[COORDINATES]\nJ1 0 0\nO1 1 1\n[SUBCATCHMENTS]\nS1 RG1 J1 2 50 100 1 0\n[SUBAREAS]\nS1 0.01 0.1 0.05 0.05 25 OUTLET\n[POLLUTANTS]\nTSS MG/L 0 0 0 0\n[LANDUSES]\nRes\n";

    #[test]
    fn expression_identifiers_skip_numbers_and_operators() {
        assert_eq!(
            expression_identifiers("1e-3*Hgw + exp(-0.5*Ks) - 2.5"),
            vec!["Hgw", "exp", "Ks"]
        );
        assert!(unknown_gwf_identifiers("0.001*(HGW-HCB)*A").is_empty());
        assert_eq!(unknown_gwf_identifiers("Hgw*Bogus"), vec!["Bogus"]);
        assert!(check_treatment_expression("R = 0.5 * HRT", &[]).is_ok());
        assert!(check_treatment_expression("c = TSS * 0.5 + R_TSS", &["TSS".into()]).is_ok());
        assert!(check_treatment_expression("R = R_BOD", &["TSS".into()]).is_err());
        assert!(check_treatment_expression("0.5", &[]).is_err());
        assert!(check_treatment_expression("R 0.5", &[]).is_err());
        assert!(check_treatment_expression("", &[]).is_err());
    }

    #[test]
    fn lid_controls_and_usage_findings() {
        let text = format!(
            "{BASE}[LID_CONTROLS]\nBC1 BC\nBC1 SURFACE 0 1.5 0.1 1 5\nBC1 DRAINMAT 3 0.5 0.1\nNoType SOIL 3 0.5 0.2 0.1 0.5 10 3.5\nBad ZZ\nShort IT\nShort STORAGE 12 0.75\n[LID_USAGE]\nS1 BC1 4 30000 0 0 150 0 * Nowhere 0\nS1 Ghost 1 1 0 0 0 0\n"
        );
        let fs = findings(&text);
        assert!(has(&fs, "LID_CONTROLS", "BC1", "needs a SOIL layer (ERROR 184)"), "{fs:?}");
        assert!(has(&fs, "LID_CONTROLS", "BC1", "DRAINMAT layer is not used"), "{fs:?}");
        assert!(has(&fs, "LID_CONTROLS", "BC1", "VegFrac"), "{fs:?}");
        assert!(has(&fs, "LID_CONTROLS", "NoType", "ERROR 183"), "{fs:?}");
        assert!(has(&fs, "LID_CONTROLS", "Bad", "unknown LID type"), "{fs:?}");
        assert!(has(&fs, "LID_CONTROLS", "Short", "STORAGE row has 4 fields"), "{fs:?}");
        assert!(has(&fs, "LID_USAGE", "S1", "ERROR 187"), "{fs:?}");
        assert!(has(&fs, "LID_USAGE", "S1", "FromImp is a percentage"), "{fs:?}");
        assert!(has(&fs, "LID_USAGE", "S1", "DrainTo names \"Nowhere\""), "{fs:?}");
        assert!(has(&fs, "LID_USAGE", "S1", "LID control \"Ghost\""), "{fs:?}");
        let f = fs.iter().find(|f| f.message.contains("ERROR 187")).unwrap();
        assert_eq!(f.severity, Severity::Error);
        assert_eq!(f.column.as_deref(), Some("Area"));
    }

    #[test]
    fn groundwater_snow_and_rdii_findings() {
        let text = format!(
            "{BASE}[AQUIFERS]\nAq 0.5 0.4 0.3 0.1 12 15 0.35 14 0.002 0 3.5 0.4\n[GROUNDWATER]\nS1 Ghost J1 6 0.1 1 0 0 0 0\n[GWF]\nS1 LATERAL 0.001*(Hgw-Hcb)*Bogus\nS1 SIDEWAYS Hgw\n[SNOWPACKS]\nSP PLOWABLE 0.001 0.001 32 0.1 0 0\nSP IMPERVIOUS 0.001 0.001 32 0.1 0 0 1\nSP FROZEN 1 2 3 4 5 6 7\n[HYDROGRAPHS]\nUH RG1\nUH ALL SHORT 0.6 1 2\nUH ALL MEDIUM 0.6 4 2\nUH ALL LONG 0 -1 2\nUH SOMETIME QUICK 0 1 2\nNoGage ALL SHORT 0 1 2\n[RDII]\nJ1 Ghost -5\n"
        );
        let text = text.replace("S1 RG1 J1 2 50 100 1 0\n", "S1 RG1 J1 2 50 100 1 0 Nope\n");
        let fs = findings(&text);
        assert!(has(&fs, "AQUIFERS", "Aq", "wilting point must be below the field capacity (ERROR 109)"), "{fs:?}");
        assert!(has(&fs, "GROUNDWATER", "S1", "aquifer \"Ghost\""), "{fs:?}");
        assert!(has(&fs, "GWF", "S1", "unknown variable(s) Bogus"), "{fs:?}");
        assert!(has(&fs, "GWF", "S1", "Type must be LATERAL or DEEP"), "{fs:?}");
        assert!(has(&fs, "SNOWPACKS", "SP", "PLOWABLE row has 8 fields"), "{fs:?}");
        assert!(has(&fs, "SNOWPACKS", "SP", "unknown keyword \"FROZEN\""), "{fs:?}");
        assert!(has(&fs, "SUBCATCHMENTS", "S1", "snow pack \"Nope\""), "{fs:?}");
        assert!(has(&fs, "HYDROGRAPHS", "UH", "sum to 1.2"), "{fs:?}");
        assert!(has(&fs, "HYDROGRAPHS", "UH", "ERROR 151"), "{fs:?}");
        assert!(has(&fs, "HYDROGRAPHS", "UH", "Month must be ALL"), "{fs:?}");
        assert!(has(&fs, "HYDROGRAPHS", "UH", "Response must be"), "{fs:?}");
        assert!(has(&fs, "HYDROGRAPHS", "NoGage", "no rain gage"), "{fs:?}");
        assert!(has(&fs, "RDII", "J1", "unit hydrograph set \"Ghost\""), "{fs:?}");
        assert!(has(&fs, "RDII", "J1", "ERROR 155"), "{fs:?}");
    }

    #[test]
    fn water_quality_findings() {
        let text = format!(
            "{BASE}[BUILDUP]\nRes TSS POW 50 1\nRes BOD EXP 1 1 0 AREA\nRes TSS EXT 1 1 NoSeries AREA\nRes TSS GROW 1 1 1 AREA\nCom TSS NONE 0 0 0 AREA\n[WASHOFF]\nRes TSS EXP 1\nRes TSS EMC 100 0 150 0\n[COVERAGES]\nS1 Res 60 Ind 50\n[LOADINGS]\nS1 BOD 5\n[TREATMENT]\nJ1 TSS R = 0.5 * FOO\nJ1 BOD C = 1\nO1 TSS 0.5\n[INFLOWS]\nJ1 Lead TS FLOW\n[DWF]\nJ1 BOD 1\n"
        );
        let fs = findings(&text);
        assert!(has(&fs, "BUILDUP", "Res", "POW row has 5 fields"), "{fs:?}");
        assert!(has(&fs, "BUILDUP", "Res", "pollutant \"BOD\""), "{fs:?}");
        assert!(has(&fs, "BUILDUP", "Res", "time series \"NoSeries\""), "{fs:?}");
        assert!(has(&fs, "BUILDUP", "Res", "unknown function \"GROW\""), "{fs:?}");
        assert!(has(&fs, "BUILDUP", "Com", "land use \"Com\""), "{fs:?}");
        assert!(has(&fs, "WASHOFF", "Res", "EXP row has 4 fields"), "{fs:?}");
        assert!(has(&fs, "WASHOFF", "Res", "SweepRmvl is a percentage"), "{fs:?}");
        assert!(has(&fs, "COVERAGES", "S1", "land use \"Ind\""), "{fs:?}");
        assert!(has(&fs, "COVERAGES", "S1", "cover 110 %"), "{fs:?}");
        assert_eq!(
            fs.iter().find(|f| f.message.contains("cover 110")).unwrap().severity,
            Severity::Warning
        );
        assert!(has(&fs, "LOADINGS", "S1", "pollutant \"BOD\""), "{fs:?}");
        assert!(has(&fs, "TREATMENT", "J1", "unknown variable(s) FOO"), "{fs:?}");
        assert!(has(&fs, "TREATMENT", "J1", "pollutant \"BOD\""), "{fs:?}");
        assert!(has(&fs, "TREATMENT", "O1", "must start with R"), "{fs:?}");
        assert!(has(&fs, "INFLOWS", "J1", "Constituent \"Lead\""), "{fs:?}");
        assert!(has(&fs, "DWF", "J1", "Constituent \"BOD\""), "{fs:?}");
        // A clean quality model has none of these.
        let text = format!(
            "{BASE}[BUILDUP]\nRes TSS SAT 50 10 0 AREA\n[WASHOFF]\nRes TSS EMC 100 0\n[COVERAGES]\nS1 Res 60\n[LOADINGS]\nS1 TSS 5\n[TREATMENT]\nJ1 TSS R = 1 - exp(-0.5*HRT)\n"
        );
        let fs = findings(&text);
        assert!(fs.iter().all(|f| f.severity != Severity::Error), "{fs:?}");
    }
}
