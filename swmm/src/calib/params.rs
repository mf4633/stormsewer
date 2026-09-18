// SPDX-License-Identifier: GPL-3.0-or-later

//! Calibration parameters: a group of objects × a column × a transform ×
//! bounds. A group edits as one factor, so `Width` of every subcatchment
//! tagged `upper` is one parameter, not one per subcatchment.

use serde::{Deserialize, Serialize};

use crate::doc::{schema, Command, InpDoc};

/// Which objects a parameter edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Group {
    /// One object by name.
    Object(String),
    /// Every object of the section carrying this `[TAGS]` tag.
    Tag(String),
    /// A list of names (the map selection when the parameter was made).
    Selection(Vec<String>),
    /// Every object of the section.
    All,
}

impl Group {
    pub fn label(&self) -> String {
        match self {
            Self::Object(n) => n.clone(),
            Self::Tag(t) => format!("tag {t}"),
            Self::Selection(v) => format!("{} selected", v.len()),
            Self::All => "all".into(),
        }
    }
}

/// How the factor changes the base value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transform {
    /// `value = base × x`; bounds are on `x` (e.g. 0.5–2).
    #[default]
    Multiply,
    /// `value = base + x`; bounds are on `x`.
    Offset,
    /// `value = x`; bounds are on the value itself.
    Set,
}

impl Transform {
    pub const ALL: [Transform; 3] = [Transform::Multiply, Transform::Offset, Transform::Set];

    pub fn label(self) -> &'static str {
        match self {
            Self::Multiply => "multiply",
            Self::Offset => "offset",
            Self::Set => "set",
        }
    }

    pub fn apply(self, base: f64, x: f64) -> f64 {
        match self {
            Self::Multiply => base * x,
            Self::Offset => base + x,
            Self::Set => x,
        }
    }

    /// The factor that leaves the base value alone.
    pub fn identity(self, base: f64) -> f64 {
        match self {
            Self::Multiply => 1.0,
            Self::Offset => 0.0,
            Self::Set => base,
        }
    }
}

/// One calibration parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    /// The `.inp` section, upper case.
    pub section: String,
    /// The column name (`schema::columns`) or a decimal index.
    pub column: String,
    pub group: Group,
    pub transform: Transform,
    pub lower: f64,
    pub upper: f64,
}

impl Parameter {
    pub fn new(
        name: &str,
        section: &str,
        column: &str,
        group: Group,
        transform: Transform,
        lower: f64,
        upper: f64,
    ) -> Self {
        Self {
            name: name.into(),
            section: section.to_ascii_uppercase(),
            column: column.into(),
            group,
            transform,
            lower,
            upper,
        }
    }

    pub fn range(&self) -> f64 {
        self.upper - self.lower
    }

    /// The `[TAGS]` keyword for this section's objects.
    pub fn tag_kind(&self) -> Option<&'static str> {
        let s = self.section.as_str();
        if schema::NODE_SECTIONS.contains(&s) {
            Some("Node")
        } else if schema::LINK_SECTIONS.contains(&s) {
            Some("Link")
        } else if matches!(
            s,
            "SUBCATCHMENTS" | "SUBAREAS" | "INFILTRATION" | "GROUNDWATER" | "LID_USAGE"
        ) {
            Some("Subcatch")
        } else if s == "RAINGAGES" {
            Some("Gage")
        } else {
            None
        }
    }

    /// The object names this parameter edits in `doc`. Names the document
    /// no longer has are left out, so a deleted object cannot fail a run.
    pub fn members(&self, doc: &InpDoc) -> Vec<String> {
        match &self.group {
            Group::Object(n) => {
                if doc.contains(&self.section, n) {
                    vec![n.clone()]
                } else {
                    Vec::new()
                }
            }
            Group::Selection(v) => v
                .iter()
                .filter(|n| doc.contains(&self.section, n))
                .cloned()
                .collect(),
            Group::All => doc.names(&self.section),
            Group::Tag(tag) => {
                let Some(kind) = self.tag_kind() else {
                    return Vec::new();
                };
                doc.names(&self.section)
                    .into_iter()
                    .filter(|n| {
                        doc.tag(kind, n)
                            .is_some_and(|t| t.eq_ignore_ascii_case(tag))
                    })
                    .collect()
            }
        }
    }

    /// The current value of the column for `name`.
    pub fn base_value(&self, doc: &InpDoc, name: &str) -> Option<f64> {
        doc.field(&self.section, name, &self.column)?.parse().ok()
    }

    /// The factor at which the parameter reproduces the base model. For
    /// `Set`, the mean of the members' current values.
    pub fn identity(&self, doc: &InpDoc) -> f64 {
        let members = self.members(doc);
        let bases: Vec<f64> = members
            .iter()
            .filter_map(|m| self.base_value(doc, m))
            .collect();
        let base = if bases.is_empty() {
            0.0
        } else {
            bases.iter().sum::<f64>() / bases.len() as f64
        };
        self.transform.identity(base)
    }

    /// The commands that set this parameter to factor `x` on `doc`.
    /// Members with no readable value are skipped.
    pub fn commands(&self, doc: &InpDoc, x: f64) -> Vec<Command> {
        let x = x.clamp(self.lower.min(self.upper), self.upper.max(self.lower));
        let mut out = Vec::new();
        for name in self.members(doc) {
            let Some(base) = self.base_value(doc, &name) else {
                continue;
            };
            let mut v = self.transform.apply(base, x);
            if is_percent(&self.column) {
                v = v.clamp(0.0, 100.0);
            }
            if v < 0.0 && !allows_negative(&self.column) {
                v = 0.0;
            }
            out.push(Command::SetField {
                section: self.section.clone(),
                name,
                field: self.column.clone(),
                value: format_value(v),
            });
        }
        out
    }
}

fn is_percent(column: &str) -> bool {
    matches!(column, "PctImperv" | "PctZero" | "PctRouted" | "PctSlope")
}

fn allows_negative(column: &str) -> bool {
    matches!(
        column,
        "Elevation" | "InOffset" | "OutOffset" | "Z" | "InitDepth"
    )
}

/// Up to six decimals, trailing zeros dropped.
pub fn format_value(v: f64) -> String {
    if !v.is_finite() || v.abs() < 1e-12 {
        return "0".into();
    }
    let s = if v.abs() >= 1000.0 {
        format!("{v:.3}")
    } else {
        format!("{v:.6}")
    };
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// One undo step setting every parameter to its factor.
pub fn apply_set(doc: &InpDoc, params: &[Parameter], x: &[f64]) -> Command {
    let mut cmds = Vec::new();
    for (p, xi) in params.iter().zip(x) {
        cmds.extend(p.commands(doc, *xi));
    }
    Command::Batch(cmds)
}

/// The usual suspects, for the model's infiltration method. Each edits
/// all objects of its section as one multiplier.
pub fn suggestions(doc: &InpDoc) -> Vec<Parameter> {
    let m = |name: &str, section: &str, column: &str, lo: f64, hi: f64| {
        Parameter::new(
            name,
            section,
            column,
            Group::All,
            Transform::Multiply,
            lo,
            hi,
        )
    };
    let mut out = vec![
        m("%Imperv", "SUBCATCHMENTS", "PctImperv", 0.5, 1.5),
        m("Width", "SUBCATCHMENTS", "Width", 0.25, 4.0),
        m("N-Imperv", "SUBAREAS", "NImperv", 0.5, 2.0),
        m("N-Perv", "SUBAREAS", "NPerv", 0.5, 2.0),
        m("S-Imperv", "SUBAREAS", "SImperv", 0.5, 2.0),
        m("S-Perv", "SUBAREAS", "SPerv", 0.5, 2.0),
        m("PctZero", "SUBAREAS", "PctZero", 0.5, 2.0),
    ];
    let method = doc
        .option("INFILTRATION")
        .map(|s| s.to_ascii_uppercase())
        .unwrap_or_else(|| "HORTON".into());
    match method.as_str() {
        "GREEN_AMPT" | "MODIFIED_GREEN_AMPT" => {
            out.push(m("Suction head", "INFILTRATION", "Suction", 0.5, 2.0));
            out.push(m("Ksat", "INFILTRATION", "Ksat", 0.2, 5.0));
            out.push(m("Initial deficit (IMD)", "INFILTRATION", "IMD", 0.5, 1.5));
        }
        "CURVE_NUMBER" => {
            out.push(m("Curve number", "INFILTRATION", "CurveNum", 0.8, 1.2));
        }
        _ => {
            out.push(m("Horton max rate", "INFILTRATION", "MaxRate", 0.5, 2.0));
            out.push(m("Horton min rate", "INFILTRATION", "MinRate", 0.5, 2.0));
            out.push(m("Horton decay", "INFILTRATION", "Decay", 0.5, 2.0));
        }
    }
    out.push(m("Conduit roughness", "CONDUITS", "Roughness", 0.5, 2.0));
    out.push(m("Junction ponded area", "JUNCTIONS", "Aponded", 0.5, 2.0));
    out.push(m("Storage surcharge depth", "STORAGE", "SurDepth", 0.5, 2.0));
    // [AQUIFERS] Name Por WP FC Ksat Kslope Tslope ETu ETs Seep Ebot Egw
    // Umc [ETupat]: Ksat is column 4 (no named table for the section).
    out.push(m("Aquifer conductivity", "AQUIFERS", "4", 0.2, 5.0));
    out
}
