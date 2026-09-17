// SPDX-License-Identifier: GPL-3.0-or-later

//! Light referential checks over an [`InpDoc`], for the editor to show live.
//! Nothing here is an error: a finding is a message about a place in the
//! document, and the list is empty for a consistent model.
//!
//! Checked: link endpoints exist; subcatchment outlets (node or subcatchment)
//! and rain gages exist; duplicate names within a section, across the node
//! sections, and across the link sections (case-insensitively, as the engine
//! sees them); `[COORDINATES]`, `[VERTICES]`, `[POLYGONS]`, `[SYMBOLS]` and
//! `[XSECTIONS]` rows for objects that do not exist; nodes without
//! coordinates; conduits without a cross-section; rows shorter than their
//! section allows.

use std::collections::HashMap;

use super::schema::{self, LINK_SECTIONS, NODE_SECTIONS};
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
}

impl Finding {
    fn new(severity: Severity, section: &str, name: &str, message: impl Into<String>) -> Self {
        Self {
            severity,
            section: section.to_string(),
            name: name.to_string(),
            message: message.into(),
        }
    }
}

/// Names defined by a set of sections, folded for lookup, mapped to where
/// they were first seen (as written, section).
type Index = HashMap<String, (String, String)>;

fn index(doc: &InpDoc, sections: &[&str]) -> Index {
    let mut out = HashMap::new();
    for sec in sections {
        for (_, r) in doc.rows(sec) {
            if let Some(n) = r.value(0) {
                out.entry(n.to_ascii_uppercase())
                    .or_insert_with(|| (n.to_string(), sec.to_string()));
            }
        }
    }
    out
}

fn known(index: &Index, name: &str) -> bool {
    index.contains_key(&unquote(name).to_ascii_uppercase())
}

pub fn validate(doc: &InpDoc) -> Vec<Finding> {
    let mut out = Vec::new();
    let nodes = index(doc, NODE_SECTIONS);
    let links = index(doc, LINK_SECTIONS);
    let subs = index(doc, &["SUBCATCHMENTS"]);
    let gages = index(doc, &["RAINGAGES"]);

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
            if let Some(first) = seen.get(&n.to_ascii_uppercase()) {
                out.push(Finding::new(
                    Severity::Error,
                    &s.name,
                    n,
                    format!("duplicate name (first defined as {first:?})"),
                ));
            } else {
                seen.insert(n.to_ascii_uppercase(), n.to_string());
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
                if let Some((_, other)) = family.get(&n.to_ascii_uppercase()) {
                    if other != &s.name {
                        out.push(Finding::new(
                            Severity::Error,
                            &s.name,
                            n,
                            format!("also defined in [{other}]"),
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
            for (i, end) in [(1, "from"), (2, "to")] {
                match r.value(i) {
                    Some(n) if known(&nodes, n) => {}
                    Some(n) => out.push(Finding::new(
                        Severity::Error,
                        sec,
                        name,
                        format!("{end} node {n:?} does not exist"),
                    )),
                    None => out.push(Finding::new(
                        Severity::Error,
                        sec,
                        name,
                        format!("no {end} node"),
                    )),
                }
            }
        }
    }

    // Subcatchment outlet and gage.
    for (_, r) in doc.rows("SUBCATCHMENTS") {
        let name = r.value(0).unwrap_or("");
        match r.value(2) {
            Some(o) if known(&nodes, o) || known(&subs, o) => {}
            Some(o) => out.push(Finding::new(
                Severity::Error,
                "SUBCATCHMENTS",
                name,
                format!("outlet {o:?} is neither a node nor a subcatchment"),
            )),
            None => out.push(Finding::new(
                Severity::Error,
                "SUBCATCHMENTS",
                name,
                "no outlet",
            )),
        }
        match r.value(1) {
            Some(g) if known(&gages, g) => {}
            Some("*") => {}
            Some(g) => out.push(Finding::new(
                Severity::Error,
                "SUBCATCHMENTS",
                name,
                format!("rain gage {g:?} does not exist"),
            )),
            None => {}
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
            if sec == "COORDINATES" && seen.insert(n.to_ascii_uppercase(), ()).is_some() {
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

    out
}
