// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure conversions over an [`InpDoc`] that the engine does not do for you.
//!
//! **Offsets.** `[OPTIONS] LINK_OFFSETS` says whether every link offset in
//! the file is a depth above the node invert or an absolute elevation.
//! Flipping the keyword alone changes what every number means, so
//! [`offset_conversion`] rewrites each offset with the node invert (from
//! `[JUNCTIONS]` / `[OUTFALLS]` / `[STORAGE]` / `[DIVIDERS]`) so the
//! geometry keeps its meaning. Conduits use the from-node for `InOffset`
//! and the to-node for `OutOffset`; orifices, weirs and outlets are offset
//! from their inlet (from) node.
//!
//! **Flow units.** `FLOW_UNITS` chooses the unit *system* for the whole
//! file (CFS/GPM/MGD are US customary: feet, acres, inches; CMS/LPS/MLD are
//! SI: metres, hectares, millimetres) as well as the flow unit. The engine
//! converts nothing in the file when it changes: every length, area, depth,
//! rate, coefficient and flow value is read as if it were already in the
//! new system. [`plan`] lists what would change and [`convert`] rewrites the
//! chosen groups as one batch. Manning's n is the same in both systems
//! (the engine carries the 1.49 inside); orifice discharge coefficients are
//! dimensionless; weir coefficients are not (`Cw` in ft^0.5/s versus
//! m^0.5/s, 3.33 against 1.84).
//!
//! Factors: 1 ft = 0.3048 m, 1 acre = 0.40468564 ha, 1 in = 25.4 mm,
//! 1 cfs = 0.028316847 m³/s = 448.83117 gpm = 0.64631689 mgd.

use std::collections::HashMap;

use super::schema::NODE_SECTIONS;
use super::{format_number, unquote, Command, InpDoc, Row};

// ---------------------------------------------------------------------------
// Offsets
// ---------------------------------------------------------------------------

/// The `LINK_OFFSETS` convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetMode {
    Depth,
    Elevation,
}

impl OffsetMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "DEPTH" => Some(Self::Depth),
            "ELEVATION" => Some(Self::Elevation),
            _ => None,
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::Depth => "DEPTH",
            Self::Elevation => "ELEVATION",
        }
    }

    /// The document's convention (`DEPTH` when unset, as the engine does).
    pub fn of(doc: &InpDoc) -> Self {
        doc.option("LINK_OFFSETS")
            .and_then(Self::parse)
            .unwrap_or(Self::Depth)
    }
}

/// The batch a conversion produced, with a human-readable account.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub command: Command,
    /// One line per changed value: `[CONDUITS] C2 OutOffset 4 → 4973`.
    pub changes: Vec<String>,
    /// Values left alone, and why.
    pub skipped: Vec<String>,
}

impl Conversion {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The account as text, for the clipboard.
    pub fn report(&self, title: &str) -> String {
        let mut s = format!("{title}\n{} value(s) changed", self.changes.len());
        if !self.skipped.is_empty() {
            s.push_str(&format!(", {} left alone", self.skipped.len()));
        }
        s.push('\n');
        for c in &self.changes {
            s.push_str("  ");
            s.push_str(c);
            s.push('\n');
        }
        for c in &self.skipped {
            s.push_str("  (skipped) ");
            s.push_str(c);
            s.push('\n');
        }
        s
    }
}

/// Node inverts, folded by name.
fn inverts(doc: &InpDoc) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for sec in NODE_SECTIONS {
        for (_, r) in doc.rows(sec) {
            if let (Some(n), Some(e)) = (r.value(0), r.value(1)) {
                if let Ok(e) = e.trim().parse::<f64>() {
                    out.entry(unquote(n).to_ascii_uppercase()).or_insert(e);
                }
            }
        }
    }
    out
}

/// `(section, offset column, index of the node field it is measured from)`.
const OFFSET_COLUMNS: &[(&str, &str, usize)] = &[
    ("CONDUITS", "InOffset", 1),
    ("CONDUITS", "OutOffset", 2),
    ("ORIFICES", "Offset", 1),
    ("WEIRS", "CrestHt", 1),
    ("OUTLETS", "Offset", 1),
];

/// The batch that switches `LINK_OFFSETS` to `to` and rewrites every offset
/// so each pipe stays where it is. Converting to the mode already in force
/// changes nothing but still sets the option.
pub fn offset_conversion(doc: &InpDoc, to: OffsetMode) -> Conversion {
    let from = OffsetMode::of(doc);
    let inverts = inverts(doc);
    let mut cmds = Vec::new();
    let mut changes = Vec::new();
    let mut skipped = Vec::new();
    if from != to {
        for (sec, col, node_field) in OFFSET_COLUMNS {
            for (_, r) in doc.rows(sec) {
                let cols = doc.columns(sec, r);
                let name = r.value(0).unwrap_or("");
                let Some(raw) = r.get(cols, col) else { continue };
                let Some(node) = r.value(*node_field) else { continue };
                let Some(&invert) = inverts.get(&unquote(node).to_ascii_uppercase()) else {
                    skipped.push(format!(
                        "[{sec}] {name} {col}: node {node:?} has no numeric invert"
                    ));
                    continue;
                };
                let Ok(value) = raw.trim().parse::<f64>() else {
                    skipped.push(format!("[{sec}] {name} {col}: {raw:?} is not a number"));
                    continue;
                };
                let new = match to {
                    OffsetMode::Elevation => value + invert,
                    // An elevation of 0 in ELEVATION mode is below every
                    // real invert; the engine zeroes the negative depth, so
                    // it means depth 0.
                    OffsetMode::Depth => {
                        if value == 0.0 {
                            0.0
                        } else {
                            value - invert
                        }
                    }
                };
                let text = format_number(new);
                if text != raw.trim() {
                    cmds.push(Command::SetField {
                        section: sec.to_string(),
                        name: name.to_string(),
                        field: col.to_string(),
                        value: text.clone(),
                    });
                    changes.push(format!("[{sec}] {name} {col} {raw} → {text}"));
                }
            }
        }
    }
    cmds.push(Command::SetOption {
        section: "OPTIONS".into(),
        key: "LINK_OFFSETS".into(),
        value: to.keyword().into(),
    });
    Conversion {
        command: Command::Batch(cmds),
        changes,
        skipped,
    }
}

// ---------------------------------------------------------------------------
// Flow units
// ---------------------------------------------------------------------------

/// A `FLOW_UNITS` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlowUnit {
    Cfs,
    Gpm,
    Mgd,
    Cms,
    Lps,
    Mld,
}

impl FlowUnit {
    pub const ALL: [FlowUnit; 6] = [
        Self::Cfs,
        Self::Gpm,
        Self::Mgd,
        Self::Cms,
        Self::Lps,
        Self::Mld,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "CFS" => Some(Self::Cfs),
            "GPM" => Some(Self::Gpm),
            "MGD" => Some(Self::Mgd),
            "CMS" => Some(Self::Cms),
            "LPS" => Some(Self::Lps),
            "MLD" => Some(Self::Mld),
            _ => None,
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::Cfs => "CFS",
            Self::Gpm => "GPM",
            Self::Mgd => "MGD",
            Self::Cms => "CMS",
            Self::Lps => "LPS",
            Self::Mld => "MLD",
        }
    }

    pub fn is_metric(self) -> bool {
        matches!(self, Self::Cms | Self::Lps | Self::Mld)
    }

    /// One of this unit, in cubic feet per second.
    pub fn in_cfs(self) -> f64 {
        match self {
            Self::Cfs => 1.0,
            Self::Gpm => 1.0 / 448.831_168_8,
            Self::Mgd => 1.547_228_6,
            Self::Cms => 1.0 / 0.028_316_846_592,
            Self::Lps => 1.0 / 28.316_846_592,
            Self::Mld => 1_000_000.0 / 86_400.0 / 28.316_846_592,
        }
    }

    /// The document's flow unit (`CFS` when unset, as the engine does).
    pub fn of(doc: &InpDoc) -> Self {
        doc.option("FLOW_UNITS")
            .and_then(Self::parse)
            .unwrap_or(Self::Cfs)
    }
}

/// Multipliers that take a value in the `from` system to the `to` system.
/// Every non-flow factor is 1 when both units are in the same system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Factors {
    pub from: FlowUnit,
    pub to: FlowUnit,
    pub flow: f64,
    /// ft ↔ m.
    pub length: f64,
    /// ft² ↔ m².
    pub area: f64,
    /// acre ↔ ha.
    pub big_area: f64,
    /// ft³ ↔ m³.
    pub volume: f64,
    /// in ↔ mm; also in/hr ↔ mm/hr and in/day ↔ mm/day.
    pub depth: f64,
    /// Weir coefficient, ft^0.5/s ↔ m^0.5/s.
    pub weir: f64,
}

impl Factors {
    pub fn between(from: FlowUnit, to: FlowUnit) -> Self {
        const FT: f64 = 0.3048;
        const ACRE_HA: f64 = 0.404_685_642_24;
        const IN_MM: f64 = 25.4;
        let (length, area, big_area, volume, depth, weir) = match (from.is_metric(), to.is_metric()) {
            (false, true) => (FT, FT * FT, ACRE_HA, FT * FT * FT, IN_MM, FT.sqrt()),
            (true, false) => (
                1.0 / FT,
                1.0 / (FT * FT),
                1.0 / ACRE_HA,
                1.0 / (FT * FT * FT),
                1.0 / IN_MM,
                1.0 / FT.sqrt(),
            ),
            _ => (1.0, 1.0, 1.0, 1.0, 1.0, 1.0),
        };
        Self {
            from,
            to,
            flow: from.in_cfs() / to.in_cfs(),
            length,
            area,
            big_area,
            volume,
            depth,
            weir,
        }
    }

    pub fn changes_system(&self) -> bool {
        self.from.is_metric() != self.to.is_metric()
    }
}

/// A group of values the wizard can convert together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Inflows,
    Dwf,
    Curves,
    Storage,
    Nodes,
    Conduits,
    Regulators,
    Pumps,
    Subcatchments,
    Infiltration,
    Evaporation,
    GageSeries,
    Options,
}

impl Item {
    pub const ALL: [Item; 13] = [
        Self::Inflows,
        Self::Dwf,
        Self::Curves,
        Self::Storage,
        Self::Nodes,
        Self::Conduits,
        Self::Regulators,
        Self::Pumps,
        Self::Subcatchments,
        Self::Infiltration,
        Self::Evaporation,
        Self::GageSeries,
        Self::Options,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Inflows => "External inflows",
            Self::Dwf => "Dry-weather flows",
            Self::Curves => "Curves (by type)",
            Self::Storage => "Storage units",
            Self::Nodes => "Node elevations and depths",
            Self::Conduits => "Conduit geometry",
            Self::Regulators => "Weirs, orifices, outlets",
            Self::Pumps => "Pump start/stop depths",
            Self::Subcatchments => "Subcatchment area, width, storage",
            Self::Infiltration => "Infiltration parameters",
            Self::Evaporation => "Evaporation",
            Self::GageSeries => "Rain gage time series",
            Self::Options => "[OPTIONS] tolerances",
        }
    }

    /// What is converted, and what the engine would otherwise do.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Inflows => "[INFLOWS] FLOW rows: Baseline is scaled, and the series scale factor (Sfactor) is scaled so the time series itself need not be edited. Pollutant inflows are left alone.",
            Self::Dwf => "[DWF] FLOW rows: Baseline is scaled. Patterns are multipliers and stay.",
            Self::Curves => "[CURVES]: PUMP1 (volume, flow), PUMP2/PUMP4 (depth, flow), PUMP3/PUMP5 (head, flow), STORAGE (depth, area), RATING (head, flow), TIDAL (hour, stage), DIVERSION (flow, flow), WEIR (head, coefficient). SHAPE and CONTROL curves are dimensionless.",
            Self::Storage => "[STORAGE]: invert, depths, geometric length/width, functional Coeff/Constant (area per depth^Exponent), seepage suction and conductivity.",
            Self::Nodes => "[JUNCTIONS] / [OUTFALLS] / [DIVIDERS]: inverts, depths, ponded areas, fixed outfall stage, divider Qmin/Height/Cd.",
            Self::Conduits => "[CONDUITS] Length, offsets, InitFlow/MaxFlow; [XSECTIONS] the Geom fields that are lengths for each shape (side slopes, exponents and size codes stay); [LOSSES] seepage rate. Manning's n is the same in both systems. Transects are not converted.",
            Self::Regulators => "[WEIRS] CrestHt, RoadWidth, and the coefficients Qcoeff/EndCoeff, which carry units (Cw ≈ 3.33 ft^0.5/s in US, ≈ 1.84 m^0.5/s in SI: factor 0.5521). [ORIFICES] Offset; its Qcoeff is dimensionless and stays. [OUTLETS] Offset; a FUNCTIONAL coefficient C in Q = C·H^n is rescaled by flow/length^n.",
            Self::Pumps => "[PUMPS] Startup and Shutoff depths.",
            Self::Subcatchments => "[SUBCATCHMENTS] Area (acre ↔ ha) and Width; [SUBAREAS] depression storage (in ↔ mm). Slope, imperviousness and Manning's n stay.",
            Self::Infiltration => "[INFILTRATION]: Horton rates (in/hr ↔ mm/hr) and MaxInfil; Green-Ampt suction and Ksat; Curve Number conductivity. [AQUIFERS] and [GROUNDWATER] are not converted.",
            Self::Evaporation => "[EVAPORATION] CONSTANT and MONTHLY values (in/day ↔ mm/day). TIMESERIES and FILE sources are not touched.",
            Self::GageSeries => "[TIMESERIES] read by a TIMESERIES-source rain gage (in ↔ mm, or in/hr ↔ mm/hr). FILE gages keep their Units keyword, which describes the file.",
            Self::Options => "[OPTIONS] MIN_SURFAREA (ft² ↔ m²) and HEAD_TOLERANCE (ft ↔ m).",
        }
    }
}

/// One line of the wizard: an item, how many values it would touch, and
/// whether it is ticked.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanItem {
    pub item: Item,
    pub count: usize,
    pub on: bool,
}

/// Up to four decimals, trailing zeros trimmed.
pub fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// Scale field `i` of `fields` by `k`, recording the change. Non-numeric
/// fields are left alone; a factor of 1 changes nothing.
fn scale(
    fields: &mut [String],
    i: usize,
    k: f64,
    label: &dyn Fn() -> String,
    changes: &mut Vec<String>,
) {
    if k == 1.0 {
        return;
    }
    let Some(f) = fields.get_mut(i) else { return };
    let Ok(v) = unquote(f).trim().parse::<f64>() else { return };
    if v == 0.0 {
        return;
    }
    let new = fmt(v * k);
    if new != f.trim() {
        changes.push(format!("{} {} → {new}", label(), f.trim()));
        *f = new;
    }
}

struct Ctx<'a> {
    doc: &'a InpDoc,
    k: Factors,
    cmds: Vec<Command>,
    changes: Vec<String>,
    skipped: Vec<String>,
}

impl Ctx<'_> {
    /// Visit every row of `section`; `edit` scales fields in place and the
    /// row is rewritten if any changed.
    fn rows(&mut self, section: &str, mut edit: impl FnMut(&Row, &mut Vec<String>, &mut Vec<String>, &mut Vec<String>)) {
        let Some(s) = self.doc.section(section) else { return };
        for (li, r) in s.rows() {
            let mut fields = r.fields.clone();
            let mut changes = Vec::new();
            let mut skipped = Vec::new();
            edit(r, &mut fields, &mut changes, &mut skipped);
            self.skipped.append(&mut skipped);
            if !changes.is_empty() {
                self.cmds.push(Command::SetLine {
                    section: section.to_string(),
                    line: li,
                    fields,
                    comment: r.comment.clone(),
                });
                self.changes.append(&mut changes);
            }
        }
    }

    /// Scale the named columns of every row of `section` by the factors.
    fn columns(&mut self, section: &str, cols: &[(&str, f64)]) {
        let doc = self.doc;
        self.rows(section, |r, fields, changes, _| {
            let layout = doc.columns(section, r);
            let name = r.value(0).unwrap_or("").to_string();
            for (col, k) in cols {
                if let Some(i) = super::schema::field_index(layout, col) {
                    scale(fields, i, *k, &|| format!("[{section}] {name} {col}"), changes);
                }
            }
        });
    }

    fn item(&mut self, item: Item) {
        let k = self.k;
        let doc = self.doc;
        match item {
            Item::Inflows => self.rows("INFLOWS", |r, fields, changes, _| {
                if !r.value(1).is_some_and(|c| c.eq_ignore_ascii_case("FLOW")) {
                    return;
                }
                let name = r.value(0).unwrap_or("").to_string();
                let has_series = r.value(2).is_some_and(|s| !s.trim().is_empty());
                if has_series {
                    // Pad to Sfactor so the series can be scaled without
                    // touching the series itself.
                    let defaults = ["FLOW", "1.0", "1.0"];
                    for (i, d) in (3..6).zip(defaults) {
                        if fields.len() <= i {
                            fields.push(d.to_string());
                        }
                    }
                    scale(fields, 5, k.flow, &|| format!("[INFLOWS] {name} Sfactor"), changes);
                }
                scale(fields, 6, k.flow, &|| format!("[INFLOWS] {name} Baseline"), changes);
            }),
            Item::Dwf => self.rows("DWF", |r, fields, changes, _| {
                if !r.value(1).is_some_and(|c| c.eq_ignore_ascii_case("FLOW")) {
                    return;
                }
                let name = r.value(0).unwrap_or("").to_string();
                scale(fields, 2, k.flow, &|| format!("[DWF] {name} Baseline"), changes);
            }),
            Item::Curves => {
                let mut kinds: HashMap<String, String> = HashMap::new();
                self.rows("CURVES", |r, fields, changes, skipped| {
                    let name = r.value(0).unwrap_or("").to_string();
                    let key = name.to_ascii_uppercase();
                    let cols = doc.columns("CURVES", r);
                    let start = if cols.len() == 4 {
                        kinds.insert(key.clone(), r.value(1).unwrap_or("").to_ascii_uppercase());
                        2
                    } else {
                        1
                    };
                    let kind = kinds.get(&key).cloned().unwrap_or_default();
                    let (kx, ky) = match kind.as_str() {
                        "PUMP1" => (k.volume, k.flow),
                        "PUMP2" | "PUMP3" | "PUMP4" | "PUMP5" => (k.length, k.flow),
                        "STORAGE" => (k.length, k.area),
                        "RATING" => (k.length, k.flow),
                        "TIDAL" => (1.0, k.length),
                        "DIVERSION" => (k.flow, k.flow),
                        "WEIR" => (k.length, k.weir),
                        "SHAPE" | "CONTROL" => (1.0, 1.0),
                        _ => {
                            if start == 2 {
                                skipped.push(format!("[CURVES] {name}: unknown curve type {kind:?}"));
                            }
                            (1.0, 1.0)
                        }
                    };
                    let n = fields.len();
                    let mut i = start;
                    while i + 1 < n {
                        scale(fields, i, kx, &|| format!("[CURVES] {name} X"), changes);
                        scale(fields, i + 1, ky, &|| format!("[CURVES] {name} Y"), changes);
                        i += 2;
                    }
                });
            }
            Item::Storage => self.rows("STORAGE", |r, fields, changes, _| {
                let layout = doc.columns("STORAGE", r);
                let name = r.value(0).unwrap_or("").to_string();
                let lbl = |c: &str| format!("[STORAGE] {name} {c}");
                for c in ["Elevation", "MaxDepth", "InitDepth", "SurDepth", "Length", "Width"] {
                    if let Some(i) = super::schema::field_index(layout, c) {
                        scale(fields, i, k.length, &|| lbl(c), changes);
                    }
                }
                if let (Some(ci), Some(ei)) = (
                    super::schema::field_index(layout, "Coeff"),
                    super::schema::field_index(layout, "Exponent"),
                ) {
                    let b = fields.get(ei).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    scale(fields, ci, k.area / k.length.powf(b), &|| lbl("Coeff"), changes);
                }
                if let Some(i) = super::schema::field_index(layout, "Constant") {
                    scale(fields, i, k.area, &|| lbl("Constant"), changes);
                }
                if let Some(i) = super::schema::field_index(layout, "Psi") {
                    scale(fields, i, k.depth, &|| lbl("Psi"), changes);
                }
                if let Some(i) = super::schema::field_index(layout, "Ksat") {
                    scale(fields, i, k.depth, &|| lbl("Ksat"), changes);
                }
            }),
            Item::Nodes => {
                self.columns(
                    "JUNCTIONS",
                    &[
                        ("Elevation", k.length),
                        ("MaxDepth", k.length),
                        ("InitDepth", k.length),
                        ("SurDepth", k.length),
                        ("Aponded", k.area),
                    ],
                );
                self.columns("OUTFALLS", &[("Elevation", k.length), ("Stage", k.length)]);
                self.columns(
                    "DIVIDERS",
                    &[
                        ("Elevation", k.length),
                        ("Qmin", k.flow),
                        ("Height", k.length),
                        ("Cd", k.weir),
                        ("MaxDepth", k.length),
                        ("InitDepth", k.length),
                        ("SurDepth", k.length),
                        ("Aponded", k.area),
                    ],
                );
            }
            Item::Conduits => {
                self.columns(
                    "CONDUITS",
                    &[
                        ("Length", k.length),
                        ("InOffset", k.length),
                        ("OutOffset", k.length),
                        ("InitFlow", k.flow),
                        ("MaxFlow", k.flow),
                    ],
                );
                let dw = doc
                    .option("FORCE_MAIN_EQUATION")
                    .is_some_and(|v| v.eq_ignore_ascii_case("D-W"));
                self.rows("XSECTIONS", |r, fields, changes, skipped| {
                    let name = r.value(0).unwrap_or("").to_string();
                    let shape = r.value(1).unwrap_or("").to_ascii_uppercase();
                    let geoms: &[usize] = match shape.as_str() {
                        "CIRCULAR" | "EGG" | "HORSESHOE" | "GOTHIC" | "CATENARY"
                        | "SEMIELLIPTICAL" | "BASKETHANDLE" | "SEMICIRCULAR" | "CUSTOM" => &[2],
                        "FILLED_CIRCULAR" | "RECT_CLOSED" | "RECT_OPEN" | "PARABOLIC" | "POWER"
                        | "HORIZ_ELLIPSE" | "VERT_ELLIPSE" | "ARCH" | "TRAPEZOIDAL"
                        | "TRIANGULAR" => &[2, 3],
                        "RECT_TRIANGULAR" | "RECT_ROUND" | "MODBASKETHANDLE" => &[2, 3, 4],
                        "FORCE_MAIN" => &[2],
                        "DUMMY" => &[],
                        "IRREGULAR" | "STREET" => {
                            skipped.push(format!(
                                "[XSECTIONS] {name}: {shape} geometry lives in [TRANSECTS]/[STREETS], not converted"
                            ));
                            &[]
                        }
                        _ => &[],
                    };
                    for &i in geoms {
                        scale(fields, i, k.length, &|| format!("[XSECTIONS] {name} Geom{}", i - 1), changes);
                    }
                    if shape == "FORCE_MAIN" && dw {
                        scale(fields, 3, k.depth, &|| format!("[XSECTIONS] {name} Geom2 (D-W roughness)"), changes);
                    }
                });
                self.columns("LOSSES", &[("Seepage", k.depth)]);
            }
            Item::Regulators => {
                self.columns("ORIFICES", &[("Offset", k.length)]);
                self.columns(
                    "WEIRS",
                    &[
                        ("CrestHt", k.length),
                        ("Qcoeff", k.weir),
                        ("EndCoeff", k.weir),
                        ("RoadWidth", k.length),
                    ],
                );
                self.rows("OUTLETS", |r, fields, changes, _| {
                    let layout = doc.columns("OUTLETS", r);
                    let name = r.value(0).unwrap_or("").to_string();
                    if let Some(i) = super::schema::field_index(layout, "Offset") {
                        scale(fields, i, k.length, &|| format!("[OUTLETS] {name} Offset"), changes);
                    }
                    if let (Some(ci), Some(ni)) = (
                        super::schema::field_index(layout, "Qcoeff"),
                        super::schema::field_index(layout, "Qexpon"),
                    ) {
                        let n = fields.get(ni).and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
                        scale(fields, ci, k.flow / k.length.powf(n), &|| format!("[OUTLETS] {name} Qcoeff"), changes);
                    }
                });
            }
            Item::Pumps => self.columns("PUMPS", &[("Startup", k.length), ("Shutoff", k.length)]),
            Item::Subcatchments => {
                self.columns("SUBCATCHMENTS", &[("Area", k.big_area), ("Width", k.length)]);
                self.columns("SUBAREAS", &[("SImperv", k.depth), ("SPerv", k.depth)]);
            }
            Item::Infiltration => self.columns(
                "INFILTRATION",
                &[
                    ("MaxRate", k.depth),
                    ("MinRate", k.depth),
                    ("MaxInfil", k.depth),
                    ("Suction", k.depth),
                    ("Ksat", k.depth),
                    ("Conductivity", k.depth),
                ],
            ),
            Item::Evaporation => self.rows("EVAPORATION", |r, fields, changes, _| {
                let key = r.value(0).unwrap_or("").to_ascii_uppercase();
                let n = fields.len();
                match key.as_str() {
                    "CONSTANT" | "MONTHLY" => {
                        for i in 1..n {
                            scale(fields, i, k.depth, &|| format!("[EVAPORATION] {key}"), changes);
                        }
                    }
                    _ => {}
                }
            }),
            Item::GageSeries => {
                let mut series: Vec<String> = Vec::new();
                for (_, r) in doc.rows("RAINGAGES") {
                    if r.value(4).is_some_and(|s| s.eq_ignore_ascii_case("TIMESERIES")) {
                        if let Some(name) = r.value(5) {
                            series.push(name.to_ascii_uppercase());
                        }
                    } else if let Some(g) = r.value(0) {
                        self.skipped.push(format!(
                            "[RAINGAGES] {g}: FILE source; the Units keyword describes the file and is left as written"
                        ));
                    }
                }
                self.rows("TIMESERIES", |r, fields, changes, _| {
                    let name = r.value(0).unwrap_or("").to_string();
                    if !series.contains(&name.to_ascii_uppercase()) {
                        return;
                    }
                    if r.value(1).is_some_and(|f| f.eq_ignore_ascii_case("FILE")) {
                        return;
                    }
                    // `Name [Date] Time Value ...`: values follow a time token.
                    let mut i = 1;
                    while i < fields.len() {
                        let tok = unquote(&fields[i]);
                        let is_date = tok.contains('/') || (tok.contains('-') && !tok.starts_with('-'));
                        if is_date {
                            i += 1;
                            continue;
                        }
                        if i + 1 < fields.len() {
                            scale(fields, i + 1, k.depth, &|| format!("[TIMESERIES] {name} value"), changes);
                        }
                        i += 2;
                    }
                });
            }
            Item::Options => {
                self.rows("OPTIONS", |r, fields, changes, _| {
                    let key = r.value(0).unwrap_or("").to_ascii_uppercase();
                    let f = match key.as_str() {
                        "MIN_SURFAREA" => k.area,
                        "HEAD_TOLERANCE" => k.length,
                        _ => return,
                    };
                    scale(fields, 1, f, &|| format!("[OPTIONS] {key}"), changes);
                });
            }
        }
    }
}

/// What switching `from` → `to` would touch, per item, every item ticked
/// that touches anything.
pub fn plan(doc: &InpDoc, from: FlowUnit, to: FlowUnit) -> Vec<PlanItem> {
    Item::ALL
        .iter()
        .map(|&item| {
            let c = convert(doc, from, to, &[item]);
            PlanItem {
                item,
                count: c.changes.len(),
                on: !c.changes.is_empty(),
            }
        })
        .collect()
}

/// The batch that sets `FLOW_UNITS` to `to` and converts the chosen
/// items, with an account of every value changed.
pub fn convert(doc: &InpDoc, from: FlowUnit, to: FlowUnit, items: &[Item]) -> Conversion {
    let mut ctx = Ctx {
        doc,
        k: Factors::between(from, to),
        cmds: Vec::new(),
        changes: Vec::new(),
        skipped: Vec::new(),
    };
    for item in items {
        ctx.item(*item);
    }
    ctx.cmds.push(Command::SetOption {
        section: "OPTIONS".into(),
        key: "FLOW_UNITS".into(),
        value: to.keyword().into(),
    });
    Conversion {
        command: Command::Batch(ctx.cmds),
        changes: ctx.changes,
        skipped: ctx.skipped,
    }
}

/// Every link offset in the document as `(section, name, column, value)`,
/// for tests and the wizard's before/after view.
pub fn offsets(doc: &InpDoc) -> Vec<(String, String, String, f64)> {
    let mut out = Vec::new();
    for (sec, col, _) in OFFSET_COLUMNS {
        for (_, r) in doc.rows(sec) {
            let cols = doc.columns(sec, r);
            if let (Some(n), Some(v)) = (r.value(0), r.get(cols, col)) {
                if let Ok(v) = v.trim().parse::<f64>() {
                    out.push((sec.to_string(), n.to_string(), col.to_string(), v));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> InpDoc {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/epa-samples")
            .join(name);
        InpDoc::read(&p).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 + 1e-6 * b.abs()
    }

    #[test]
    fn offsets_convert_with_node_inverts_and_round_trip() {
        let mut doc = fixture("Detention_Pond_Model.inp");
        assert_eq!(OffsetMode::of(&doc), OffsetMode::Depth);
        let before = offsets(&doc);
        assert!(before.len() >= 12, "{}", before.len());
        let c2_out = before.iter().find(|o| o.1 == "C2" && o.2 == "OutOffset").unwrap().3;
        assert_eq!(c2_out, 4.0);

        let conv = offset_conversion(&doc, OffsetMode::Elevation);
        assert!(!conv.is_empty());
        assert!(conv.changes.iter().any(|c| c.starts_with("[CONDUITS] C2 OutOffset 4 →")), "{:?}", conv.changes);
        doc.apply(conv.command).unwrap();
        assert_eq!(doc.option("LINK_OFFSETS"), Some("ELEVATION"));
        let after = offsets(&doc);
        assert_eq!(after.len(), before.len());
        for ((sec, name, col, b), (_, _, _, a)) in before.iter().zip(&after) {
            let node_field = if col == "OutOffset" { 2 } else { 1 };
            let node = doc.find(sec, name).unwrap().1.value(node_field).unwrap().to_string();
            let invert = inverts(&doc)[&node.to_ascii_uppercase()];
            assert!(close(*a, b + invert), "{sec} {name} {col}: {a} vs {b} + {invert}");
        }
        // The weir crest sits 8 ft above the pond invert of 4956.
        assert!(after.iter().any(|o| o.1 == "W1" && o.3 == 4964.0), "{after:?}");

        // Back again: one undo step per conversion, and the values return.
        let conv = offset_conversion(&doc, OffsetMode::Depth);
        doc.apply(conv.command).unwrap();
        let back = offsets(&doc);
        for (b, r) in before.iter().zip(&back) {
            assert!(close(b.3, r.3), "{b:?} vs {r:?}");
        }
        assert_eq!(doc.option("LINK_OFFSETS"), Some("DEPTH"));
        assert_eq!(doc.undo_depth(), 2);
        assert!(doc.undo() && doc.undo());
        assert_eq!(doc.to_string(), fixture("Detention_Pond_Model.inp").to_string());

        // Same mode: only the option row changes.
        let conv = offset_conversion(&doc, OffsetMode::Depth);
        assert!(conv.is_empty());
    }

    #[test]
    fn elevation_zero_means_depth_zero() {
        let doc = InpDoc::parse(
            "[OPTIONS]\nLINK_OFFSETS ELEVATION\n[JUNCTIONS]\nJ1 100 5\nJ2 90 5\n[CONDUITS]\nC1 J1 J2 10 0.01 0 91.5\nC2 J1 J2 10 0.01 * 0\n",
        );
        let conv = offset_conversion(&doc, OffsetMode::Depth);
        assert!(conv.changes.iter().any(|c| c.contains("C1 OutOffset 91.5 → 1.5")), "{conv:?}");
        assert!(!conv.changes.iter().any(|c| c.contains("C1 InOffset")), "0 stays 0: {conv:?}");
        assert!(conv.skipped.iter().any(|s| s.contains("\"*\" is not a number")), "{conv:?}");
    }

    #[test]
    fn factors_are_the_handbook_values() {
        let k = Factors::between(FlowUnit::Cfs, FlowUnit::Cms);
        assert!(close(k.flow, 0.028_316_846_592));
        assert!(close(k.length, 0.3048));
        assert!(close(k.big_area, 0.404_685_642_24));
        assert!(close(k.depth, 25.4));
        assert!(close(k.weir, 0.552_086_9), "{}", k.weir);
        assert!(k.changes_system());
        let k = Factors::between(FlowUnit::Cfs, FlowUnit::Gpm);
        assert!(close(k.flow, 448.831_168_8));
        assert_eq!(k.length, 1.0);
        assert!(!k.changes_system());
        let k = Factors::between(FlowUnit::Cms, FlowUnit::Lps);
        assert!(close(k.flow, 1000.0));
        let k = Factors::between(FlowUnit::Mld, FlowUnit::Cms);
        assert!(close(k.flow, 1.0 / 86.4));
        let k = Factors::between(FlowUnit::Mgd, FlowUnit::Cfs);
        assert!(close(k.flow, 1.547_228_6));
        assert_eq!(FlowUnit::parse("lps"), Some(FlowUnit::Lps));
        assert_eq!(fmt(56.388), "56.388");
        assert_eq!(fmt(2.0), "2");
        assert_eq!(fmt(0.00001), "0");
    }

    const SMALL: &str = "[OPTIONS]\nFLOW_UNITS CFS\nFORCE_MAIN_EQUATION D-W\nMIN_SURFAREA 12.566\n\
[JUNCTIONS]\nJ1 100 5 0 0 200\n[OUTFALLS]\nO1 95 FIXED 96.5 NO\n\
[STORAGE]\nS1 90 10 0 FUNCTIONAL 1000 1 500 0 0 3 0.5 0.3\n\
[CONDUITS]\nC1 J1 O1 185 0.013 0 0 0 10\nC2 J1 S1 10 0.013 1 0 0 0\n\
[PUMPS]\nP1 S1 J1 PC1 ON 2 1\n\
[ORIFICES]\nOR1 S1 O1 SIDE 0.5 0.65 NO 0\n\
[WEIRS]\nW1 S1 O1 TRANSVERSE 8 3.33 NO 0 0 NO 10\n\
[OUTLETS]\nOU1 S1 O1 1 FUNCTIONAL/DEPTH 2 0.5 NO\n\
[XSECTIONS]\nC1 CIRCULAR 1 0 0 0 1\nC2 TRAPEZOIDAL 2 4 2 2\nOR1 CIRCULAR 0.5 0 0 0\nW1 RECT_OPEN 1 3 0 0\n\
[LOSSES]\nC1 0.5 0.5 0 NO 1\n\
[SUBCATCHMENTS]\nS1 RG1 J1 10 50 400 0.5 0\n[SUBAREAS]\nS1 0.01 0.1 0.05 0.05 25 OUTLET\n\
[INFILTRATION]\nS1 3.0 0.5 4 7 0\n\
[RAINGAGES]\nRG1 INTENSITY 1:00 1.0 TIMESERIES TS1\n\
[TIMESERIES]\nTS1 0:00 1.0 1:00 2.0\nTS2 0:00 5\n\
[INFLOWS]\nJ1 FLOW TS2 FLOW 1.0 1.0 3\nJ1 TSS TS2 CONCEN 1.0 1.0 0\n\
[DWF]\nJ1 FLOW 1 \"\" \"\"\n\
[CURVES]\nPC1 PUMP2 2 10\nPC1 4 20\nSC STORAGE 0 100\nSC 1 200\nRC RATING 1 5\nTC TIDAL 0 2\nDC DIVERSION 1 0.5\n\
[EVAPORATION]\nCONSTANT 0.2\n";

    #[test]
    fn us_to_si_converts_each_group_to_hand_values() {
        let doc = InpDoc::parse(SMALL);
        let plan = plan(&doc, FlowUnit::Cfs, FlowUnit::Cms);
        assert_eq!(plan.len(), Item::ALL.len());
        assert!(plan.iter().all(|p| p.count > 0 && p.on), "{plan:?}");

        let mut doc = InpDoc::parse(SMALL);
        let conv = convert(&doc, FlowUnit::Cfs, FlowUnit::Cms, &Item::ALL);
        doc.apply(conv.command.clone()).unwrap();
        assert_eq!(doc.undo_depth(), 1);
        assert_eq!(doc.option("FLOW_UNITS"), Some("CMS"));
        let f = |sec: &str, name: &str, col: &str| -> f64 {
            doc.field(sec, name, col)
                .unwrap_or_else(|| panic!("[{sec}] {name} {col} missing"))
                .parse()
                .unwrap()
        };
        // Conduit length 185 ft = 56.388 m; max flow 10 cfs = 0.2832 m³/s.
        assert!(close(f("CONDUITS", "C1", "Length"), 56.388));
        assert!(close(f("CONDUITS", "C1", "MaxFlow"), 0.2832));
        assert!(close(f("CONDUITS", "C2", "InOffset"), 0.3048));
        // Junction invert 100 ft = 30.48 m; ponded area 200 ft² = 18.5806 m².
        assert!(close(f("JUNCTIONS", "J1", "Elevation"), 30.48));
        assert!(close(f("JUNCTIONS", "J1", "Aponded"), 18.5806));
        assert!(close(f("OUTFALLS", "O1", "Stage"), 29.4132));
        // Functional storage A = 1000·D^1 + 500: Coeff × 0.0929/0.3048 = 304.8,
        // Constant × 0.0929 = 46.4515; Psi 3 in = 76.2 mm; Ksat 0.5 in/hr = 12.7.
        assert!(close(f("STORAGE", "S1", "Coeff"), 304.8));
        assert!(close(f("STORAGE", "S1", "Constant"), 46.4515));
        assert!(close(f("STORAGE", "S1", "Psi"), 76.2));
        assert!(close(f("STORAGE", "S1", "Ksat"), 12.7));
        // Weir Cw 3.33 × 0.55209 = 1.8384; crest 8 ft → 2.4384 m; road 10 ft → 3.048.
        assert!(close(f("WEIRS", "W1", "Qcoeff"), 1.8384));
        assert!(close(f("WEIRS", "W1", "CrestHt"), 2.4384));
        assert!(close(f("WEIRS", "W1", "RoadWidth"), 3.048));
        // Orifice Cd is dimensionless.
        assert!(close(f("ORIFICES", "OR1", "Qcoeff"), 0.65));
        assert!(close(f("ORIFICES", "OR1", "Offset"), 0.1524));
        // Outlet Q = 2·H^0.5: C × 0.0283168 / 0.3048^0.5 = 0.1026.
        assert!(close(f("OUTLETS", "OU1", "Qcoeff"), 0.1026));
        assert!(close(f("OUTLETS", "OU1", "Qexpon"), 0.5));
        assert!(close(f("PUMPS", "P1", "Startup"), 0.6096));
        // Trapezoid: height and bottom width are lengths, side slopes stay.
        assert_eq!(doc.field("XSECTIONS", "C2", "Geom1"), Some("0.6096"));
        assert_eq!(doc.field("XSECTIONS", "C2", "Geom2"), Some("1.2192"));
        assert_eq!(doc.field("XSECTIONS", "C2", "Geom3"), Some("2"));
        assert_eq!(doc.field("XSECTIONS", "C1", "Geom1"), Some("0.3048"));
        assert_eq!(doc.field("LOSSES", "C1", "Seepage"), Some("25.4"));
        // 10 acres = 4.0469 ha; width 400 ft = 121.92 m; storage 0.05 in = 1.27 mm.
        assert!(close(f("SUBCATCHMENTS", "S1", "Area"), 4.0469));
        assert!(close(f("SUBCATCHMENTS", "S1", "Width"), 121.92));
        assert!(close(f("SUBAREAS", "S1", "SImperv"), 1.27));
        assert!(close(f("INFILTRATION", "S1", "MaxRate"), 76.2));
        assert!(close(f("INFILTRATION", "S1", "MinRate"), 12.7));
        // Inflow: baseline 3 cfs → 0.085; Sfactor 1 → 0.0283, series untouched.
        let inflows = doc.inflows("J1");
        assert_eq!(inflows[0].value(6), Some("0.085"));
        assert_eq!(inflows[0].value(5), Some("0.0283"));
        assert_eq!(inflows[1].value(6), Some("0"), "pollutant inflow untouched");
        assert_eq!(doc.timeseries("TS2")[0].value, "5");
        assert_eq!(doc.field("DWF", "J1", "Baseline"), Some("0.0283"));
        // Curves by type.
        let (_, pts) = doc.curve("PC1");
        assert_eq!(pts, vec![("0.6096".to_string(), "0.2832".to_string()), ("1.2192".into(), "0.5663".into())]);
        let (_, pts) = doc.curve("SC");
        assert_eq!(pts[1], ("0.3048".to_string(), "18.5806".to_string()));
        let (_, pts) = doc.curve("RC");
        assert_eq!(pts[0], ("0.3048".to_string(), "0.1416".to_string()));
        let (_, pts) = doc.curve("TC");
        assert_eq!(pts[0], ("0".to_string(), "0.6096".to_string()), "tidal hour stays");
        let (_, pts) = doc.curve("DC");
        assert_eq!(pts[0], ("0.0283".to_string(), "0.0142".to_string()));
        // Gage series in → mm; evaporation in/day → mm/day; options.
        assert_eq!(doc.timeseries("TS1")[1].value, "50.8");
        assert_eq!(doc.key_value("EVAPORATION", "CONSTANT").as_deref(), Some("5.08"));
        assert_eq!(doc.option("MIN_SURFAREA"), Some("1.1674"));
        assert!(conv.report("CFS → CMS").contains("value(s) changed"));
        assert!(doc.undo());
        assert_eq!(doc.to_string(), SMALL);
    }

    #[test]
    fn within_a_system_only_flows_change() {
        let doc = InpDoc::parse(SMALL);
        let plan = plan(&doc, FlowUnit::Cfs, FlowUnit::Gpm);
        let on: Vec<Item> = plan.iter().filter(|p| p.on).map(|p| p.item).collect();
        assert_eq!(
            on,
            vec![Item::Inflows, Item::Dwf, Item::Curves, Item::Conduits, Item::Regulators],
            "{plan:?}"
        );
        let mut doc = InpDoc::parse(SMALL);
        let conv = convert(&doc, FlowUnit::Cfs, FlowUnit::Gpm, &on);
        doc.apply(conv.command).unwrap();
        assert_eq!(doc.field("DWF", "J1", "Baseline"), Some("448.8312"));
        assert_eq!(doc.field("CONDUITS", "C1", "Length"), Some("185"));
        assert_eq!(doc.field("CONDUITS", "C1", "MaxFlow"), Some("4488.3117"));
        assert_eq!(doc.field("WEIRS", "W1", "Qcoeff"), Some("3.33"));
        // Outlet C scales by flow only when the length factor is 1.
        assert_eq!(doc.field("OUTLETS", "OU1", "Qcoeff"), Some("897.6623"));
    }

    #[test]
    fn si_round_trip_returns_the_epa_sample() {
        let original = fixture("Detention_Pond_Model.inp");
        let mut doc = fixture("Detention_Pond_Model.inp");
        let there = convert(&doc, FlowUnit::Cfs, FlowUnit::Cms, &Item::ALL);
        assert!(there.changes.len() > 50, "{}", there.changes.len());
        doc.apply(there.command).unwrap();
        let back = convert(&doc, FlowUnit::Cms, FlowUnit::Cfs, &Item::ALL);
        doc.apply(back.command).unwrap();
        // Every numeric field comes back within rounding of the original.
        for sec in original.sections() {
            let after = doc.section(&sec.name).unwrap();
            for ((_, a), (_, b)) in sec.rows().zip(after.rows()) {
                assert_eq!(a.fields.len(), b.fields.len(), "[{}] {:?}", sec.name, a.fields);
                for (x, y) in a.fields.iter().zip(&b.fields) {
                    match (x.parse::<f64>(), y.parse::<f64>()) {
                        (Ok(x), Ok(y)) => assert!(
                            (x - y).abs() <= 1e-3 + 1e-4 * x.abs(),
                            "[{}] {x} vs {y}",
                            sec.name
                        ),
                        _ => assert_eq!(x, y, "[{}]", sec.name),
                    }
                }
            }
        }
        assert_eq!(doc.option("FLOW_UNITS"), Some("CFS"));
    }
}
