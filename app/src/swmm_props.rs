// SPDX-License-Identifier: GPL-3.0-or-later

//! The editable property sheet, and the field model it shares with the
//! attribute grids and the project dialogs.
//!
//! Every field on the sheet comes from [`schema::columns`] for the object's
//! row, so a column the document knows is a field the sheet shows, and
//! nothing else is. What each column *is* — a number with a unit, a keyword
//! from a fixed list, a yes/no flag, a reference to another object — is a
//! table lookup here ([`spec`]). Committing a field is one command and one
//! undo step labelled `Set <field> of <name>`; the name field renames through
//! [`Command::Rename`] so every reference follows; a type column that changes
//! the row's layout rewrites the row with the new layout's defaults
//! ([`switched_row`]).

use eframe::egui::{self, Id, Key, RichText, TextEdit, Ui};
use stormsewer_swmm::doc::build::{LinkType, NodeType, ObjRef};
use stormsewer_swmm::doc::schema::{self, ObjectKind, INFILTRATION_METHODS, REFERENCES};
use stormsewer_swmm::doc::{Command, InpDoc, Row};

use crate::state::AppState;
use crate::swmm_doc::{describe, SwmmEditor};
use crate::theme::palette;

// --- the field model ----------------------------------------------------------

pub const OUTFALL_TYPES: &[&str] = &["FREE", "NORMAL", "FIXED", "TIDAL", "TIMESERIES"];
pub const STORAGE_SHAPES: &[&str] = &[
    "FUNCTIONAL",
    "TABULAR",
    "CYLINDRICAL",
    "CONICAL",
    "PARABOLOID",
    "PYRAMIDAL",
];
pub const DIVIDER_TYPES: &[&str] = &["OVERFLOW", "CUTOFF", "TABULAR", "WEIR"];
pub const ORIFICE_TYPES: &[&str] = &["SIDE", "BOTTOM"];
pub const WEIR_TYPES: &[&str] = &[
    "TRANSVERSE",
    "SIDEFLOW",
    "V-NOTCH",
    "TRAPEZOIDAL",
    "ROADWAY",
];
pub const OUTLET_TYPES: &[&str] = &[
    "TABULAR/DEPTH",
    "TABULAR/HEAD",
    "FUNCTIONAL/DEPTH",
    "FUNCTIONAL/HEAD",
];
pub const XSECTION_SHAPES: &[&str] = &[
    "CIRCULAR",
    "FORCE_MAIN",
    "FILLED_CIRCULAR",
    "RECT_CLOSED",
    "RECT_OPEN",
    "TRAPEZOIDAL",
    "TRIANGULAR",
    "HORIZ_ELLIPSE",
    "VERT_ELLIPSE",
    "ARCH",
    "PARABOLIC",
    "POWER",
    "RECT_TRIANGULAR",
    "RECT_ROUND",
    "MODBASKETHANDLE",
    "EGG",
    "HORSESHOE",
    "GOTHIC",
    "CATENARY",
    "SEMIELLIPTICAL",
    "BASKETHANDLE",
    "SEMICIRCULAR",
    "CUSTOM",
    "IRREGULAR",
    "STREET",
];
pub const CURVE_TYPES: &[&str] = &[
    "STORAGE",
    "SHAPE",
    "DIVERSION",
    "TIDAL",
    "PUMP1",
    "PUMP2",
    "PUMP3",
    "PUMP4",
    "PUMP5",
    "RATING",
    "CONTROL",
    "WEIR",
];
pub const PATTERN_TYPES: &[&str] = &["MONTHLY", "DAILY", "HOURLY", "WEEKEND"];
const YES_NO: &[&str] = &["YES", "NO"];

/// Columns whose value picks the row's layout (see `schema::columns`).
const LAYOUT_COLUMNS: &[(&str, &str)] = &[
    ("OUTFALLS", "Type"),
    ("STORAGE", "Shape"),
    ("DIVIDERS", "Type"),
    ("OUTLETS", "Type"),
    ("XSECTIONS", "Shape"),
    ("RAINGAGES", "Source"),
    ("INFILTRATION", "Method"),
];

/// Columns that hold a number.
const NUMERIC: &[&str] = &[
    "Elevation",
    "MaxDepth",
    "InitDepth",
    "SurDepth",
    "Aponded",
    "Stage",
    "Coeff",
    "Exponent",
    "Constant",
    "Length",
    "Width",
    "Z",
    "Fevap",
    "Psi",
    "Ksat",
    "IMD",
    "Qmin",
    "Height",
    "Cd",
    "Roughness",
    "InOffset",
    "OutOffset",
    "InitFlow",
    "MaxFlow",
    "Startup",
    "Shutoff",
    "Offset",
    "Qcoeff",
    "CloseTime",
    "CrestHt",
    "EndCon",
    "EndCoeff",
    "RoadWidth",
    "Qexpon",
    "Geom1",
    "Geom2",
    "Geom3",
    "Geom4",
    "Barrels",
    "Culvert",
    "Kentry",
    "Kexit",
    "Kavg",
    "Seepage",
    "Mfactor",
    "Sfactor",
    "Baseline",
    "SewerArea",
    "X",
    "Y",
    "Size",
    "Bold",
    "Italic",
    "Area",
    "PctImperv",
    "PctSlope",
    "CurbLen",
    "NImperv",
    "NPerv",
    "SImperv",
    "SPerv",
    "PctZero",
    "PctRouted",
    "MaxRate",
    "MinRate",
    "Decay",
    "DryTime",
    "MaxInfil",
    "Suction",
    "CurveNum",
    "Conductivity",
    "SCF",
    "Number",
    "InitSat",
    "FromImp",
    "ToPerv",
    "FromPerv",
    "Percent",
    "Buildup",
    "PctClogged",
    "Qmax",
    "aLocal",
    "wLocal",
    "Esurf",
    "A1",
    "B1",
    "A2",
    "B2",
    "A3",
    "Dsw",
    "Egwt",
    "Ebot",
    "Wgr",
    "Umc",
    "Multiplier",
    "Value",
    "Crain",
    "Cgw",
    "Crdii",
    "Kdecay",
    "CoFrac",
    "Cdwf",
    "Cinit",
    "SweepInterval",
    "Availability",
];

/// References beyond `schema::REFERENCES`: `(section, column, section that
/// defines the names)`.
const EXTRA_REFS: &[(&str, &str, &str)] = &[
    ("SUBCATCHMENTS", "SnowPack", "SNOWPACKS"),
    ("GROUNDWATER", "Aquifer", "AQUIFERS"),
    ("LID_USAGE", "LID", "LID_CONTROLS"),
    ("COVERAGES", "LandUse", "LANDUSES"),
    ("LOADINGS", "Pollutant", "POLLUTANTS"),
    ("TREATMENT", "Pollutant", "POLLUTANTS"),
    ("XSECTIONS", "Street", "STREETS"),
    ("INLET_USAGE", "Inlet", "INLETS"),
    ("RDII", "UnitHydrograph", "HYDROGRAPHS"),
    ("INFLOWS", "Constituent", "POLLUTANTS"),
    ("DWF", "Constituent", "POLLUTANTS"),
    ("POLLUTANTS", "CoPollutant", "POLLUTANTS"),
];

/// What a column holds, which decides the widget.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Text,
    Number,
    /// One of a fixed keyword list.
    Enum(&'static [&'static str]),
    /// The name of another object: the choices are what the model has.
    Reference(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldSpec {
    pub kind: FieldKind,
    pub unit: Option<&'static str>,
    /// Changing it rewrites the row's layout.
    pub layout: bool,
}

/// The model's unit system from `[OPTIONS] FLOW_UNITS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Units {
    pub metric: bool,
    pub flow: &'static str,
}

pub fn units(doc: &InpDoc) -> Units {
    let flow = doc
        .option("FLOW_UNITS")
        .unwrap_or("CFS")
        .to_ascii_uppercase();
    let (metric, flow) = match flow.as_str() {
        "CMS" => (true, "m³/s"),
        "LPS" => (true, "L/s"),
        "MLD" => (true, "ML/d"),
        "GPM" => (false, "gpm"),
        "MGD" => (false, "mgd"),
        _ => (false, "cfs"),
    };
    Units { metric, flow }
}

/// The unit label for a column, if it has one.
pub fn unit_label(section: &str, column: &str, u: Units) -> Option<&'static str> {
    let (len, small, rate, area, big_area) = if u.metric {
        ("m", "mm", "mm/hr", "m²", "ha")
    } else {
        ("ft", "in", "in/hr", "ft²", "acres")
    };
    let c = column;
    let sec = section;
    Some(match c {
        "Elevation" | "MaxDepth" | "InitDepth" | "SurDepth" | "Stage" | "CrestHt" | "Offset"
        | "InOffset" | "OutOffset" | "Height" | "Startup" | "Shutoff" | "Length" | "Z"
        | "Esurf" | "Egwt" | "Ebot" | "Dsw" | "Ksat" | "Suction" | "Geom1" | "Geom2" | "Geom3"
        | "Geom4" | "RoadWidth" | "CurbLen"
            if !(sec == "INFILTRATION" && matches!(c, "Ksat" | "Suction")) =>
        {
            len
        }
        "Width" if sec == "SUBCATCHMENTS" || sec == "STORAGE" || sec == "LID_USAGE" => len,
        "Aponded" | "Constant" => area,
        "Area" if sec == "SUBCATCHMENTS" => big_area,
        "Area" if sec == "LID_USAGE" => area,
        "Coeff" if sec == "STORAGE" => area,
        "InitFlow" | "MaxFlow" | "Qmin" | "Qmax" | "Baseline" if sec != "DWF" => u.flow,
        "Baseline" => u.flow,
        "PctImperv" | "PctSlope" | "PctZero" | "PctRouted" | "Percent" | "PctClogged"
        | "FromImp" | "ToPerv" | "FromPerv" | "InitSat" => "%",
        "SImperv" | "SPerv" | "MaxInfil" => small,
        "MaxRate" | "MinRate" | "Conductivity" => rate,
        "Ksat" | "Suction" if sec == "INFILTRATION" => {
            if c == "Ksat" {
                rate
            } else {
                small
            }
        }
        "Decay" => "1/hr",
        "DryTime" => "days",
        "Interval" => "hh:mm",
        "CloseTime" => "s",
        "Time" => "hh:mm",
        _ => return None,
    })
}

fn kinds_for(section: &str, column: &str) -> Vec<ObjectKind> {
    REFERENCES
        .iter()
        .filter(|(s, c, _)| s.eq_ignore_ascii_case(section) && c.eq_ignore_ascii_case(column))
        .map(|(_, _, k)| *k)
        .collect()
}

/// The names a reference column may hold, from the model. `None` when the
/// column is not a reference.
pub fn reference_options(doc: &InpDoc, section: &str, column: &str) -> Option<Vec<String>> {
    // The name column of a defining section is the object itself.
    let mut out: Vec<String> = Vec::new();
    let mut is_ref = false;
    for kind in kinds_for(section, column) {
        is_ref = true;
        for sec in kind.defining_sections() {
            out.extend(doc.names(sec));
        }
    }
    for (s, c, def) in EXTRA_REFS {
        if s.eq_ignore_ascii_case(section) && c.eq_ignore_ascii_case(column) {
            is_ref = true;
            out.extend(doc.names(def));
        }
    }
    if !is_ref {
        return None;
    }
    if column.eq_ignore_ascii_case("Constituent") {
        out.insert(0, "FLOW".into());
    }
    if column.eq_ignore_ascii_case("DivertLink") && out.is_empty() {
        out.push("*".into());
    }
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    Some(out)
}

fn enum_options(section: &str, column: &str) -> Option<&'static [&'static str]> {
    let sec = section.to_ascii_uppercase();
    let c = column;
    Some(match (sec.as_str(), c) {
        ("OUTFALLS", "Type") => OUTFALL_TYPES,
        ("STORAGE", "Shape") => STORAGE_SHAPES,
        ("DIVIDERS", "Type") => DIVIDER_TYPES,
        ("ORIFICES", "Type") => ORIFICE_TYPES,
        ("WEIRS", "Type") => WEIR_TYPES,
        ("WEIRS", "RoadSurf") => &["PAVED", "GRAVEL"],
        ("OUTLETS", "Type") => OUTLET_TYPES,
        ("XSECTIONS", "Shape") => XSECTION_SHAPES,
        ("PUMPS", "Status") => &["ON", "OFF"],
        ("RAINGAGES", "Format") => &["INTENSITY", "VOLUME", "CUMULATIVE"],
        ("RAINGAGES", "Source") => &["TIMESERIES", "FILE"],
        ("RAINGAGES", "Units") => &["IN", "MM"],
        ("SUBAREAS", "RouteTo") => &["OUTLET", "IMPERVIOUS", "PERVIOUS"],
        ("INFILTRATION", "Method") => INFILTRATION_METHODS,
        ("INFLOWS", "Type") => &["FLOW", "CONCEN", "MASS"],
        ("CURVES", "Type") => CURVE_TYPES,
        ("PATTERNS", "Type") => PATTERN_TYPES,
        ("TIMESERIES", "Source") => &["FILE"],
        ("POLLUTANTS", "Units") => &["MG/L", "UG/L", "#/L"],
        ("TAGS", "Kind") => &["Node", "Link", "Subcatch", "Gage"],
        (_, "Gated" | "FlapGate" | "Surcharge" | "SnowOnly") => YES_NO,
        _ => return None,
    })
}

/// Whether `(section, column)` decides the row's layout.
pub fn is_layout_column(section: &str, column: &str) -> bool {
    LAYOUT_COLUMNS
        .iter()
        .any(|(s, c)| s.eq_ignore_ascii_case(section) && c.eq_ignore_ascii_case(column))
}

/// The field model for one column of `section`.
pub fn spec(doc: &InpDoc, section: &str, column: &str) -> FieldSpec {
    let u = units(doc);
    let layout = is_layout_column(section, column);
    let kind = if let Some(opts) = enum_options(section, column) {
        FieldKind::Enum(opts)
    } else if let Some(names) = reference_options(doc, section, column) {
        FieldKind::Reference(names)
    } else if NUMERIC.iter().any(|n| n.eq_ignore_ascii_case(column)) {
        FieldKind::Number
    } else {
        FieldKind::Text
    };
    FieldSpec {
        kind,
        unit: unit_label(&section.to_ascii_uppercase(), column, u),
        layout,
    }
}

/// The value a column takes when a layout switch or a gap fill needs one.
pub fn default_for(section: &str, column: &str) -> String {
    let sec = section.to_ascii_uppercase();
    match column {
        "Gated" | "FlapGate" | "Surcharge" | "SnowOnly" => "NO",
        "Coeff" if sec == "STORAGE" => "1000",
        "Barrels" => "1",
        "Geom1" => "1",
        "Qcoeff" if sec == "OUTLETS" => "10",
        "Qexpon" => "0.5",
        "Exponent" => "0",
        "Status" => "ON",
        "Type" if sec == "INFLOWS" => "FLOW",
        "Constituent" => "FLOW",
        "RouteTo" if sec == "SUBAREAS" => "OUTLET",
        "Format" => "INTENSITY",
        "Source" => "TIMESERIES",
        "Interval" => "1:00",
        "SCF" => "1.0",
        "Curve" | "Series" | "Transect" | "Street" | "DivertLink" | "Pattern" | "File"
        | "Station" | "Aquifer" | "LID" | "LandUse" | "Pollutant" | "Inlet" | "UnitHydrograph" => {
            "*"
        }
        "Units" if sec == "RAINGAGES" => "IN",
        "Units" if sec == "POLLUTANTS" => "MG/L",
        c if NUMERIC.iter().any(|n| n.eq_ignore_ascii_case(c)) => "0",
        _ => "",
    }
    .to_string()
}

/// The row after `column` (a layout column) takes `value`: the new layout's
/// columns, filled from the old row where the names match and with defaults
/// where the new layout adds columns. Trailing optional columns the old row
/// left out stay out. `None` when the column is not a layout column.
pub fn switched_row(
    doc: &InpDoc,
    section: &str,
    fields: &[String],
    column: &str,
    value: &str,
) -> Option<Vec<String>> {
    if !is_layout_column(section, column) {
        return None;
    }
    let sec = section.to_ascii_uppercase();
    let old = Row {
        fields: fields.to_vec(),
        comment: None,
    };
    let old_cols = doc.columns(&sec, &old);
    let idx = schema::field_index(old_cols, column)?;
    let mut probe = fields.to_vec();
    if sec == "INFILTRATION" {
        probe = vec![
            fields.first().cloned().unwrap_or_default(),
            value.to_string(),
        ];
    } else if idx < probe.len() {
        probe[idx] = value.to_string();
    } else {
        probe.resize(idx, String::new());
        probe.push(value.to_string());
    }
    let new_cols = doc.columns(
        &sec,
        &Row {
            fields: probe,
            comment: None,
        },
    );
    let mut out = Vec::new();
    for col in new_cols {
        if col.eq_ignore_ascii_case(column) {
            out.push(value.to_string());
            continue;
        }
        match schema::field_index(old_cols, col) {
            Some(i) if i < fields.len() => out.push(fields[i].clone()),
            Some(_) => break, // an optional trailing column the row left out
            None => out.push(default_for(&sec, col)),
        }
    }
    if sec == "INFILTRATION" {
        // The method keyword trails the row; put it last.
        if let Some(pos) = out.iter().position(|v| v.eq_ignore_ascii_case(value)) {
            let v = out.remove(pos);
            out.push(v);
        }
    }
    Some(out)
}

/// `fields` with column `idx` set to `value`, gaps before it filled with the
/// column defaults.
fn with_field(
    section: &str,
    cols: &[&str],
    fields: &[String],
    idx: usize,
    value: &str,
) -> Vec<String> {
    let mut out = fields.to_vec();
    while out.len() < idx {
        let col = cols.get(out.len()).copied().unwrap_or("");
        out.push(default_for(section, col));
    }
    if idx < out.len() {
        out[idx] = value.to_string();
    } else {
        out.push(value.to_string());
    }
    out
}

/// The kind an object in `section` is, for renames.
pub fn kind_of_section(section: &str) -> Option<ObjectKind> {
    let sec = section.to_ascii_uppercase();
    if NodeType::from_section(&sec).is_some() {
        Some(ObjectKind::Node)
    } else if LinkType::from_section(&sec).is_some() {
        Some(ObjectKind::Link)
    } else {
        match sec.as_str() {
            "SUBCATCHMENTS" => Some(ObjectKind::Subcatchment),
            "RAINGAGES" => Some(ObjectKind::Gage),
            "CURVES" => Some(ObjectKind::Curve),
            "TIMESERIES" => Some(ObjectKind::Timeseries),
            "PATTERNS" => Some(ObjectKind::Pattern),
            _ => None,
        }
    }
}

/// The command that sets `column` of the object `name` in `section` to
/// `value`: a rename for the name column, a layout rewrite for a layout
/// column, else a field set (gaps before the column filled with defaults).
pub fn set_command(
    doc: &InpDoc,
    section: &str,
    name: &str,
    column: &str,
    value: &str,
) -> Result<(Command, String), String> {
    let sec = section.to_ascii_uppercase();
    let (_, row) = doc
        .find(&sec, name)
        .ok_or_else(|| format!("[{sec}] has no object {name}"))?;
    let cols = doc.columns(&sec, row);
    let idx = schema::field_index(cols, column).ok_or_else(|| format!("no column {column}"))?;
    if idx == 0 && schema::name_index(&sec) == Some(0) {
        if let Some(kind) = kind_of_section(&sec) {
            return Ok((
                Command::Rename {
                    kind,
                    old: name.to_string(),
                    new: value.to_string(),
                },
                format!("rename {name} to {value}"),
            ));
        }
    }
    let label = format!("set {column} of {name}");
    if let Some(fields) = switched_row(doc, &sec, &row.fields, column, value) {
        return Ok((
            Command::SetFields {
                section: sec,
                name: name.to_string(),
                fields,
            },
            label,
        ));
    }
    let fields = with_field(&sec, cols, &row.fields, idx, value);
    Ok((
        Command::SetFields {
            section: sec,
            name: name.to_string(),
            fields,
        },
        label,
    ))
}

/// The fields of the row at `line` in `section` after `column` takes
/// `value` (layout switches included).
pub fn line_fields(
    doc: &InpDoc,
    section: &str,
    line: usize,
    column: &str,
    value: &str,
) -> Result<Vec<String>, String> {
    let sec = section.to_ascii_uppercase();
    let row = doc
        .section(&sec)
        .and_then(|s| s.lines.get(line))
        .and_then(|l| l.row.as_ref())
        .ok_or_else(|| format!("[{sec}] has no row at line {line}"))?;
    if let Some(fields) = switched_row(doc, &sec, &row.fields, column, value) {
        return Ok(fields);
    }
    let cols = doc.columns(&sec, row);
    let idx = match schema::field_index(cols, column) {
        Some(i) => i,
        None => column
            .parse::<usize>()
            .map_err(|_| format!("no column {column}"))?,
    };
    Ok(with_field(&sec, cols, &row.fields, idx, value))
}

fn check_value(spec: &FieldSpec, value: &str) -> Result<(), String> {
    if spec.kind == FieldKind::Number && !value.is_empty() && value.parse::<f64>().is_err() {
        return Err(format!("{value:?} is not a number"));
    }
    Ok(())
}

/// Set one field of one named object as one undo step.
pub fn commit(ed: &mut SwmmEditor, section: &str, name: &str, column: &str, value: &str) -> bool {
    let value = value.trim();
    let fs = spec(&ed.doc, section, column);
    if let Err(e) = check_value(&fs, value) {
        ed.last_error = Some(e);
        return false;
    }
    let (cmd, label) = match set_command(&ed.doc, section, name, column, value) {
        Ok(x) => x,
        Err(e) => {
            ed.last_error = Some(e);
            return false;
        }
    };
    let renamed = matches!(cmd, Command::Rename { .. });
    let ok = ed.apply(cmd, &label);
    if ok && renamed {
        ed.rename_in_selection(name, value);
    }
    ok
}

/// Set one field of the row at `line` as one undo step.
pub fn commit_line(
    ed: &mut SwmmEditor,
    section: &str,
    line: usize,
    name: &str,
    column: &str,
    value: &str,
) -> bool {
    let value = value.trim();
    let fs = spec(&ed.doc, section, column);
    if let Err(e) = check_value(&fs, value) {
        ed.last_error = Some(e);
        return false;
    }
    let fields = match line_fields(&ed.doc, section, line, column, value) {
        Ok(f) => f,
        Err(e) => {
            ed.last_error = Some(e);
            return false;
        }
    };
    let comment = ed
        .doc
        .section(section)
        .and_then(|s| s.lines.get(line))
        .and_then(|l| l.row.as_ref())
        .and_then(|r| r.comment.clone());
    ed.apply(
        Command::SetLine {
            section: section.to_ascii_uppercase(),
            line,
            fields,
            comment,
        },
        &format!("set {column} of {name}"),
    )
}

/// Set one field of several objects as one undo step (one batch).
pub fn commit_many(
    ed: &mut SwmmEditor,
    targets: &[(String, String)],
    column: &str,
    value: &str,
) -> bool {
    let value = value.trim();
    let mut cmds = Vec::new();
    for (sec, name) in targets {
        let fs = spec(&ed.doc, sec, column);
        if let Err(e) = check_value(&fs, value) {
            ed.last_error = Some(e);
            return false;
        }
        match set_command(&ed.doc, sec, name, column, value) {
            Ok((Command::Rename { .. }, _)) => {
                ed.last_error = Some("several objects cannot share a name".into());
                return false;
            }
            Ok((cmd, _)) => cmds.push(cmd),
            Err(e) => {
                ed.last_error = Some(e);
                return false;
            }
        }
    }
    let label = format!("set {column} of {} objects", targets.len());
    ed.apply(Command::Batch(cmds), &label)
}

/// Set (or clear) an object's `[TAGS]` tag as one step.
pub fn commit_tag(ed: &mut SwmmEditor, kind: ObjectKind, name: &str, tag: &str) -> bool {
    let Some(kw) = kind.tag_keyword() else {
        return false;
    };
    let tag = tag.trim();
    let label = format!("set Tag of {name}");
    let existing = ed.doc.rows("TAGS").into_iter().find(|(_, r)| {
        r.fields.first().is_some_and(|k| k.eq_ignore_ascii_case(kw))
            && r.value(1).is_some_and(|n| n.eq_ignore_ascii_case(name))
    });
    let cmd = match (existing, tag.is_empty()) {
        (_, true) => Command::DeleteTag {
            kind: kw.into(),
            name: name.into(),
        },
        (Some((line, r)), false) => Command::SetLine {
            section: "TAGS".into(),
            line,
            fields: vec![r.fields[0].clone(), r.fields[1].clone(), tag.to_string()],
            comment: r.comment.clone(),
        },
        (None, false) => Command::AddRow {
            section: "TAGS".into(),
            fields: vec![kw.into(), name.into(), tag.into()],
            comment: None,
        },
    };
    ed.apply(cmd, &label)
}

// --- widgets ------------------------------------------------------------------

/// Draft text of the field being typed into, and the last inline error.
#[derive(Clone, Debug, Default)]
pub struct SheetState {
    pub draft: Option<(Id, String)>,
    pub error: Option<(Id, String)>,
}

/// The id of a sheet field, so a test can focus it.
pub fn field_id(section: &str, column: &str) -> Id {
    Id::new((
        "swmm-sheet",
        section.to_ascii_uppercase(),
        column.to_string(),
    ))
}

/// A single-line text field with a draft kept across frames. Returns the
/// new text when the user commits (focus leaves other than by Escape) and
/// the text changed.
pub fn text_field(
    ui: &mut Ui,
    id: Id,
    value: &str,
    draft: &mut Option<(Id, String)>,
    width: f32,
) -> Option<String> {
    let mut text = match draft {
        Some((d, t)) if *d == id => t.clone(),
        _ => value.to_string(),
    };
    let resp = ui.add(TextEdit::singleline(&mut text).id(id).desired_width(width));
    let mut out = None;
    if resp.lost_focus() {
        let escaped = ui.input(|i| i.key_pressed(Key::Escape));
        if !escaped && text != value {
            out = Some(text);
        }
        if draft.as_ref().is_some_and(|(d, _)| *d == id) {
            *draft = None;
        }
    } else if resp.has_focus() {
        *draft = Some((id, text));
    }
    out
}

/// A dropdown over `options` (plus the current value when it is not one of
/// them). Returns the choice when it changes.
pub fn choice_field(
    ui: &mut Ui,
    id: Id,
    value: &str,
    options: &[String],
    width: f32,
) -> Option<String> {
    let mut chosen: Option<String> = None;
    let shown = if value.is_empty() { "(none)" } else { value };
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(width)
        .show_ui(ui, |ui| {
            let listed = options.iter().any(|o| o.eq_ignore_ascii_case(value));
            if !listed && !value.is_empty() {
                let _ = ui.selectable_label(true, value);
            }
            for o in options {
                if ui
                    .selectable_label(o.eq_ignore_ascii_case(value), o)
                    .clicked()
                    && !o.eq_ignore_ascii_case(value)
                {
                    chosen = Some(o.clone());
                }
            }
        });
    chosen
}

/// The widget for one field. Returns the value to commit, if the user
/// changed it this frame.
pub fn field_widget(
    ui: &mut Ui,
    id: Id,
    spec: &FieldSpec,
    value: &str,
    draft: &mut Option<(Id, String)>,
    width: f32,
) -> Option<String> {
    match &spec.kind {
        FieldKind::Text | FieldKind::Number => text_field(ui, id, value, draft, width),
        FieldKind::Enum(opts) => {
            let options: Vec<String> = opts.iter().map(|s| s.to_string()).collect();
            choice_field(ui, id, value, &options, width)
        }
        FieldKind::Reference(names) => {
            let mut options = names.clone();
            if !options.iter().any(|o| o == "*") {
                options.push("*".into());
            }
            choice_field(ui, id, value, &options, width)
        }
    }
}

fn column_label(column: &str, unit: Option<&str>) -> String {
    match unit {
        Some(u) => format!("{column} ({u})"),
        None => column.to_string(),
    }
}

/// Every row of `section` named `name`, with its line index.
fn rows_named(doc: &InpDoc, section: &str, name: &str) -> Vec<(usize, Row)> {
    let Some(ni) = schema::name_index(section) else {
        return Vec::new();
    };
    doc.rows(section)
        .into_iter()
        .filter(|(_, r)| r.value(ni).is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .map(|(li, r)| (li, r.clone()))
        .collect()
}

/// A sub-sheet: the rows of `section` that belong to `name`, each editable
/// by line, with add and delete. `defaults` are the fields of a new row
/// after the name; `None` means rows cannot be added here.
fn draw_rows_sheet(
    ui: &mut Ui,
    ed: &mut SwmmEditor,
    section: &str,
    name: &str,
    title: &str,
    defaults: Option<Vec<String>>,
    single: bool,
) {
    let rows = rows_named(&ed.doc, section, name);
    let header = if rows.is_empty() {
        title.to_string()
    } else {
        format!("{title} ({})", rows.len())
    };
    egui::CollapsingHeader::new(header)
        .id_salt(("swmm-subsheet", section, name))
        .default_open(single)
        .show(ui, |ui| {
            let mut pending: Option<(usize, String, String)> = None;
            let mut delete: Option<usize> = None;
            for (li, row) in &rows {
                let cols = ed.doc.columns(section, row);
                egui::Grid::new(("swmm-subgrid", section, li))
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for (i, col) in cols.iter().enumerate().skip(1) {
                            let fs = spec(&ed.doc, section, col);
                            ui.label(column_label(col, fs.unit));
                            let value = row.value(i).unwrap_or("");
                            let id = Id::new(("swmm-subfield", section, *li, *col));
                            if let Some(v) =
                                field_widget(ui, id, &fs, value, &mut ed.sheet.draft, 150.0)
                            {
                                pending = Some((*li, col.to_string(), v));
                            }
                            ui.end_row();
                        }
                    });
                if !single && ui.small_button("Remove row").clicked() {
                    delete = Some(*li);
                }
                ui.add_space(4.0);
            }
            if let Some((li, col, v)) = pending {
                commit_line(ed, section, li, name, &col, &v);
            }
            if let Some(li) = delete {
                ed.apply(
                    Command::DeleteLine {
                        section: section.into(),
                        line: li,
                    },
                    &format!("remove {} row of {name}", title.to_lowercase()),
                );
            }
            if let Some(defaults) = defaults {
                if (!single || rows.is_empty()) && ui.small_button(format!("Add {title}")).clicked()
                {
                    let mut fields = vec![name.to_string()];
                    fields.extend(defaults);
                    ed.apply(
                        Command::AddRow {
                            section: section.into(),
                            fields,
                            comment: None,
                        },
                        &format!("add {} to {name}", title.to_lowercase()),
                    );
                }
            }
        });
}

fn draw_single_sheet(ui: &mut Ui, state: &mut AppState, target: &ObjRef) {
    let dark = ui.visuals().dark_mode;
    let Some((kind, name)) = target.kind().zip(target.name().map(str::to_string)) else {
        if let ObjRef::Label(li) = target {
            draw_label_sheet(ui, &mut state.swmm_doc, *li);
        }
        return;
    };
    let ed = &mut state.swmm_doc;
    let Some(sec) = ed.doc.defining_section(kind, &name) else {
        ui.label("The object is gone.");
        return;
    };
    let Some((_, row)) = ed.doc.find(sec, &name).map(|(li, r)| (li, r.clone())) else {
        return;
    };
    ui.label(RichText::new(describe(target)).strong());
    ui.label(RichText::new(format!("[{sec}]")).small());
    let cols = ed.doc.columns(sec, &row);
    let mut pending: Option<(String, String)> = None;
    let focus_first = std::mem::take(&mut ed.focus_sheet);
    egui::Grid::new("swmm-props")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (i, col) in cols.iter().enumerate() {
                let fs = spec(&ed.doc, sec, col);
                ui.label(column_label(col, fs.unit));
                let value = row.value(i).unwrap_or("");
                let id = field_id(sec, col);
                if i == 0 && focus_first {
                    ui.memory_mut(|m| m.request_focus(id));
                }
                if let Some(v) = field_widget(ui, id, &fs, value, &mut ed.sheet.draft, 170.0) {
                    pending = Some((col.to_string(), v));
                }
                ui.end_row();
                if let Some((eid, msg)) = &ed.sheet.error {
                    if *eid == id {
                        ui.label("");
                        ui.label(RichText::new(msg).color(palette::error_text(dark)).small());
                        ui.end_row();
                    }
                }
            }
            // Fields past the row's layout (extra tokens) stay visible.
            for (i, f) in row.fields.iter().enumerate().skip(cols.len()) {
                ui.label(format!("#{i}"));
                ui.label(RichText::new(f).monospace());
                ui.end_row();
            }
            // The tag.
            if let Some(kw) = kind.tag_keyword() {
                ui.label("Tag");
                let tag = ed.doc.tag(kw, &name).unwrap_or("").to_string();
                let id = field_id(sec, "Tag");
                if let Some(v) = text_field(ui, id, &tag, &mut ed.sheet.draft, 170.0) {
                    pending = Some(("Tag".into(), v));
                }
                ui.end_row();
            }
        });
    if let Some((col, v)) = pending {
        let id = field_id(sec, &col);
        let ok = if col == "Tag" {
            commit_tag(ed, kind, &name, &v)
        } else {
            commit(ed, sec, &name, &col, &v)
        };
        ed.sheet.error = if ok {
            None
        } else {
            ed.last_error.take().map(|e| (id, e))
        };
    }

    // Per-kind extras.
    ui.add_space(6.0);
    let s = |v: &str| v.to_string();
    match kind {
        ObjectKind::Node => {
            draw_rows_sheet(
                ui,
                ed,
                "INFLOWS",
                &name,
                "Inflow",
                Some(vec![
                    s("FLOW"),
                    s("\"\""),
                    s("FLOW"),
                    s("1.0"),
                    s("1.0"),
                    s("0"),
                ]),
                false,
            );
            draw_rows_sheet(
                ui,
                ed,
                "DWF",
                &name,
                "Dry Weather Flow",
                Some(vec![s("FLOW"), s("0")]),
                false,
            );
            if NodeType::from_section(sec) == Some(NodeType::Storage) {
                let shape = ed
                    .doc
                    .field(sec, &name, "Shape")
                    .unwrap_or("")
                    .to_ascii_uppercase();
                if shape == "TABULAR" {
                    ui.label(
                        RichText::new("Storage curve: pick it in the Curve field above; Project → Curves… edits it.")
                            .small(),
                    );
                }
            }
        }
        ObjectKind::Link => {
            let ltype = LinkType::from_section(sec);
            if matches!(
                ltype,
                Some(LinkType::Conduit | LinkType::Orifice | LinkType::Weir)
            ) {
                let defaults = match ltype {
                    Some(LinkType::Weir) => vec![s("RECT_OPEN"), s("1"), s("1"), s("0"), s("0")],
                    _ => vec![s("CIRCULAR"), s("1"), s("0"), s("0"), s("0"), s("1")],
                };
                draw_rows_sheet(
                    ui,
                    ed,
                    "XSECTIONS",
                    &name,
                    "Cross-Section",
                    Some(defaults),
                    true,
                );
            }
            if ltype == Some(LinkType::Conduit) {
                draw_rows_sheet(
                    ui,
                    ed,
                    "LOSSES",
                    &name,
                    "Losses",
                    Some(vec![s("0"), s("0"), s("0"), s("NO"), s("0")]),
                    true,
                );
            }
        }
        ObjectKind::Subcatchment => {
            draw_rows_sheet(
                ui,
                ed,
                "SUBAREAS",
                &name,
                "Subarea",
                Some(vec![
                    s("0.01"),
                    s("0.1"),
                    s("0.05"),
                    s("0.05"),
                    s("25"),
                    s("OUTLET"),
                ]),
                true,
            );
            let infil = stormsewer_swmm::doc::build::infiltration_defaults(&ed.doc);
            draw_rows_sheet(
                ui,
                ed,
                "INFILTRATION",
                &name,
                "Infiltration",
                Some(infil),
                true,
            );
            draw_rows_sheet(
                ui,
                ed,
                "LID_USAGE",
                &name,
                "LID Usage",
                Some(vec![s("*"), s("1"), s("0"), s("0"), s("0"), s("0"), s("0")]),
                false,
            );
            draw_rows_sheet(ui, ed, "GROUNDWATER", &name, "Groundwater", None, true);
            draw_rows_sheet(ui, ed, "COVERAGES", &name, "Land Use Coverage", None, false);
        }
        _ => {}
    }

    // Findings about this object.
    let findings: Vec<String> = ed
        .findings
        .iter()
        .filter(|f| f.name.eq_ignore_ascii_case(&name))
        .map(|f| format!("[{}] {}", f.section, f.message))
        .collect();
    if !findings.is_empty() {
        ui.add_space(4.0);
        for f in findings {
            ui.label(RichText::new(f).color(palette::warning_text(dark)).small());
        }
    }
}

fn draw_label_sheet(ui: &mut Ui, ed: &mut SwmmEditor, line: usize) {
    let Some(row) = ed
        .doc
        .section("LABELS")
        .and_then(|s| s.lines.get(line))
        .and_then(|l| l.row.clone())
    else {
        return;
    };
    ui.label(RichText::new("label").strong());
    let cols = ed.doc.columns("LABELS", &row);
    let mut pending: Option<(String, String)> = None;
    egui::Grid::new("swmm-props-label")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (i, col) in cols.iter().enumerate() {
                let fs = spec(&ed.doc, "LABELS", col);
                ui.label(column_label(col, fs.unit));
                let value = row.value(i).unwrap_or("");
                let id = Id::new(("swmm-label-field", line, *col));
                if let Some(v) = field_widget(ui, id, &fs, value, &mut ed.sheet.draft, 170.0) {
                    pending = Some((col.to_string(), v));
                }
                ui.end_row();
            }
        });
    if let Some((col, v)) = pending {
        commit_line(ed, "LABELS", line, "label", &col, &v);
    }
}

fn draw_multi_sheet(ui: &mut Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let ed = &mut state.swmm_doc;
    let targets = ed.selected_rows();
    ui.label(format!("{} objects selected", ed.selection.len()));
    if targets.is_empty() {
        return;
    }
    // Columns every selected row has, in the first row's order.
    let mut rows: Vec<(String, String, Row, &'static [&'static str])> = Vec::new();
    for (sec, name) in &targets {
        if let Some((_, r)) = ed.doc.find(sec, name) {
            let cols = ed.doc.columns(sec, r);
            rows.push((sec.to_string(), name.clone(), r.clone(), cols));
        }
    }
    let Some(first) = rows.first() else { return };
    let common: Vec<&'static str> = first
        .3
        .iter()
        .copied()
        .skip(1)
        .filter(|c| {
            rows.iter()
                .all(|(_, _, _, cols)| schema::field_index(cols, c).is_some())
        })
        .collect();
    ui.label(RichText::new("Common fields — a change applies to all as one undo step").small());
    let mut pending: Option<(String, String)> = None;
    egui::Grid::new("swmm-props-multi")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for col in &common {
                let fs = spec(&ed.doc, &first.0, col);
                ui.label(column_label(col, fs.unit));
                let values: Vec<&str> = rows
                    .iter()
                    .map(|(_, _, r, cols)| r.get(cols, col).unwrap_or(""))
                    .collect();
                let same = values.windows(2).all(|w| w[0] == w[1]);
                let shown = if same {
                    values[0].to_string()
                } else {
                    "(varies)".to_string()
                };
                let id = Id::new(("swmm-multi-field", *col));
                if let Some(v) = field_widget(ui, id, &fs, &shown, &mut ed.sheet.draft, 170.0) {
                    if v != "(varies)" {
                        pending = Some((col.to_string(), v));
                    }
                }
                ui.end_row();
            }
        });
    if let Some((col, v)) = pending {
        let targets: Vec<(String, String)> = rows
            .iter()
            .map(|(s, n, _, _)| (s.clone(), n.clone()))
            .collect();
        if !commit_many(ed, &targets, &col, &v) {
            if let Some(e) = ed.last_error.take() {
                ui.label(RichText::new(e).color(palette::error_text(dark)).small());
            }
        }
    }
}

/// The property sheet for the selection: editable fields for one object,
/// the common fields for several.
pub fn draw_sheet(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Properties");
    ui.separator();
    if !state.swmm_doc.loaded {
        ui.label("No model is open.");
        return;
    }
    let selection = state.swmm_doc.selection.clone();
    match selection.as_slice() {
        [] => {
            ui.label("Select an object on the map or in the browser.");
        }
        [one] => draw_single_sheet(ui, state, one),
        _ => draw_multi_sheet(ui, state),
    }
    if let Some(e) = state.swmm_doc.last_error.clone() {
        let dark = ui.visuals().dark_mode;
        ui.label(RichText::new(e).color(palette::error_text(dark)).small());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outfall_free_to_fixed_gains_a_stage_and_keeps_gated() {
        let doc = InpDoc::parse("[OUTFALLS]\nO1 10 FREE NO\n");
        let (_, row) = doc.find("OUTFALLS", "O1").unwrap();
        let f = switched_row(&doc, "OUTFALLS", &row.fields, "Type", "FIXED").unwrap();
        assert_eq!(f, vec!["O1", "10", "FIXED", "0", "NO"]);
        let f = switched_row(&doc, "OUTFALLS", &f, "Type", "TIDAL").unwrap();
        assert_eq!(f, vec!["O1", "10", "TIDAL", "*", "NO"]);
        let f = switched_row(&doc, "OUTFALLS", &f, "Type", "FREE").unwrap();
        assert_eq!(f, vec!["O1", "10", "FREE", "NO"]);
    }

    #[test]
    fn storage_tabular_to_functional_takes_defaults() {
        let doc = InpDoc::parse("[STORAGE]\nST1 0 10 0 TABULAR SC1 0 0\n");
        let (_, row) = doc.find("STORAGE", "ST1").unwrap();
        let f = switched_row(&doc, "STORAGE", &row.fields, "Shape", "FUNCTIONAL").unwrap();
        assert_eq!(
            f,
            vec![
                "ST1",
                "0",
                "10",
                "0",
                "FUNCTIONAL",
                "1000",
                "0",
                "0",
                "0",
                "0"
            ]
        );
    }

    #[test]
    fn infiltration_method_switch_puts_keyword_last() {
        let doc = InpDoc::parse("[OPTIONS]\nINFILTRATION HORTON\n[INFILTRATION]\nS1 3 0.5 4 7 0\n");
        let (_, row) = doc.find("INFILTRATION", "S1").unwrap();
        let f = switched_row(&doc, "INFILTRATION", &row.fields, "Method", "GREEN_AMPT").unwrap();
        assert_eq!(f, vec!["S1", "0", "0", "0", "GREEN_AMPT"]);
    }

    #[test]
    fn units_follow_flow_units() {
        let doc = InpDoc::parse("[OPTIONS]\nFLOW_UNITS CMS\n");
        let u = units(&doc);
        assert!(u.metric);
        assert_eq!(unit_label("JUNCTIONS", "Elevation", u), Some("m"));
        assert_eq!(unit_label("SUBCATCHMENTS", "Area", u), Some("ha"));
        assert_eq!(unit_label("CONDUITS", "Roughness", u), None);
        let us = units(&InpDoc::parse(""));
        assert_eq!(unit_label("CONDUITS", "MaxFlow", us), Some("cfs"));
    }

    #[test]
    fn spec_types_columns() {
        let doc = InpDoc::parse("[CURVES]\nSC1 STORAGE 0 1\n[JUNCTIONS]\nJ1 0\n");
        assert_eq!(spec(&doc, "JUNCTIONS", "Elevation").kind, FieldKind::Number);
        assert!(matches!(
            spec(&doc, "OUTFALLS", "Type").kind,
            FieldKind::Enum(_)
        ));
        assert!(spec(&doc, "OUTFALLS", "Type").layout);
        match spec(&doc, "STORAGE", "Curve").kind {
            FieldKind::Reference(names) => assert_eq!(names, vec!["SC1"]),
            k => panic!("{k:?}"),
        }
        match spec(&doc, "CONDUITS", "FromNode").kind {
            FieldKind::Reference(names) => assert_eq!(names, vec!["J1"]),
            k => panic!("{k:?}"),
        }
    }
}
