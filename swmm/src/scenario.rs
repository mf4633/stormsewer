// SPDX-License-Identifier: GPL-3.0-or-later

//! Named scenarios: a base model plus an ordered list of document edits,
//! stored beside the model in `<stem>.scenarios.json` (never in the `.inp`).
//!
//! A [`Scenario`] is a name, a description and a list of [`ScenarioEdit`]s.
//! An edit is a serialisable mirror of [`crate::doc::Command`] — the document
//! command type carries no serde derives, so the mapping here is explicit
//! and lossless in both directions ([`ScenarioEdit::from_command`] /
//! [`ScenarioEdit::to_command`]), and every variant is covered by a test.
//!
//! Applying a scenario never touches the base: [`apply`] parses a fresh copy
//! of the base text and applies the edits to that copy; [`materialize`]
//! returns the resulting `.inp` text. [`validate`] reports which edits no
//! longer fit the base — an object renamed or deleted since the scenario was
//! written — without applying anything for real.
//!
//! [`diff`] turns two documents into the edit list that takes the first to
//! the second (by object name, section by section). The editor uses it to
//! record what the user changed while a scenario was active: the document's
//! undo history is private, so the scenario is recomputed as the difference
//! between the base and the working copy whenever the document changes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::schema;
use crate::doc::{Command, InpDoc, ObjectKind};
use crate::rpt::ReportSummary;
use crate::{Error, Result};

/// Sidecar file format version.
pub const FORMAT_VERSION: u32 = 1;

/// The object kinds a `Rename` edit can name, as stable strings.
fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Node => "Node",
        ObjectKind::Link => "Link",
        ObjectKind::Subcatchment => "Subcatchment",
        ObjectKind::Gage => "Gage",
        ObjectKind::Curve => "Curve",
        ObjectKind::Timeseries => "Timeseries",
        ObjectKind::Pattern => "Pattern",
    }
}

fn kind_from_name(name: &str) -> Option<ObjectKind> {
    Some(match name.to_ascii_lowercase().as_str() {
        "node" => ObjectKind::Node,
        "link" => ObjectKind::Link,
        "subcatchment" | "subcatch" => ObjectKind::Subcatchment,
        "gage" => ObjectKind::Gage,
        "curve" => ObjectKind::Curve,
        "timeseries" => ObjectKind::Timeseries,
        "pattern" => ObjectKind::Pattern,
        _ => return None,
    })
}

/// A serialisable document edit. One variant per [`Command`] variant, with
/// the same fields; `Rename` carries the kind as a string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum ScenarioEdit {
    SetField {
        section: String,
        name: String,
        field: String,
        value: String,
    },
    SetFields {
        section: String,
        name: String,
        fields: Vec<String>,
    },
    SetLine {
        section: String,
        line: usize,
        fields: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },
    SetText {
        section: String,
        line: usize,
        text: String,
    },
    AddRow {
        section: String,
        fields: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },
    InsertText {
        section: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<usize>,
        text: String,
    },
    DeleteLine {
        section: String,
        line: usize,
    },
    DeleteObject {
        section: String,
        name: String,
    },
    DeleteTag {
        kind: String,
        name: String,
    },
    Rename {
        kind: String,
        old: String,
        new: String,
    },
    MoveNode {
        name: String,
        x: f64,
        y: f64,
    },
    MoveGage {
        name: String,
        x: f64,
        y: f64,
    },
    SetVertices {
        link: String,
        points: Vec<(f64, f64)>,
    },
    SetPolygon {
        subcatchment: String,
        points: Vec<(f64, f64)>,
    },
    SetOption {
        section: String,
        key: String,
        value: String,
    },
    SetTitle {
        text: String,
    },
    Batch {
        edits: Vec<ScenarioEdit>,
    },
}

impl ScenarioEdit {
    /// The edit for a command. Total: every command has an edit.
    pub fn from_command(cmd: &Command) -> Self {
        match cmd {
            Command::SetField {
                section,
                name,
                field,
                value,
            } => Self::SetField {
                section: section.clone(),
                name: name.clone(),
                field: field.clone(),
                value: value.clone(),
            },
            Command::SetFields {
                section,
                name,
                fields,
            } => Self::SetFields {
                section: section.clone(),
                name: name.clone(),
                fields: fields.clone(),
            },
            Command::SetLine {
                section,
                line,
                fields,
                comment,
            } => Self::SetLine {
                section: section.clone(),
                line: *line,
                fields: fields.clone(),
                comment: comment.clone(),
            },
            Command::SetText {
                section,
                line,
                text,
            } => Self::SetText {
                section: section.clone(),
                line: *line,
                text: text.clone(),
            },
            Command::AddRow {
                section,
                fields,
                comment,
            } => Self::AddRow {
                section: section.clone(),
                fields: fields.clone(),
                comment: comment.clone(),
            },
            Command::InsertText {
                section,
                line,
                text,
            } => Self::InsertText {
                section: section.clone(),
                line: *line,
                text: text.clone(),
            },
            Command::DeleteLine { section, line } => Self::DeleteLine {
                section: section.clone(),
                line: *line,
            },
            Command::DeleteObject { section, name } => Self::DeleteObject {
                section: section.clone(),
                name: name.clone(),
            },
            Command::DeleteTag { kind, name } => Self::DeleteTag {
                kind: kind.clone(),
                name: name.clone(),
            },
            Command::Rename { kind, old, new } => Self::Rename {
                kind: kind_name(*kind).to_string(),
                old: old.clone(),
                new: new.clone(),
            },
            Command::MoveNode { name, x, y } => Self::MoveNode {
                name: name.clone(),
                x: *x,
                y: *y,
            },
            Command::MoveGage { name, x, y } => Self::MoveGage {
                name: name.clone(),
                x: *x,
                y: *y,
            },
            Command::SetVertices { link, points } => Self::SetVertices {
                link: link.clone(),
                points: points.clone(),
            },
            Command::SetPolygon {
                subcatchment,
                points,
            } => Self::SetPolygon {
                subcatchment: subcatchment.clone(),
                points: points.clone(),
            },
            Command::SetOption {
                section,
                key,
                value,
            } => Self::SetOption {
                section: section.clone(),
                key: key.clone(),
                value: value.clone(),
            },
            Command::SetTitle { text } => Self::SetTitle { text: text.clone() },
            Command::Batch(cmds) => Self::Batch {
                edits: cmds.iter().map(Self::from_command).collect(),
            },
        }
    }

    /// The command for an edit. Fails only for a `Rename` whose kind
    /// string is not one of the seven object kinds.
    pub fn to_command(&self) -> Result<Command> {
        Ok(match self {
            Self::SetField {
                section,
                name,
                field,
                value,
            } => Command::SetField {
                section: section.clone(),
                name: name.clone(),
                field: field.clone(),
                value: value.clone(),
            },
            Self::SetFields {
                section,
                name,
                fields,
            } => Command::SetFields {
                section: section.clone(),
                name: name.clone(),
                fields: fields.clone(),
            },
            Self::SetLine {
                section,
                line,
                fields,
                comment,
            } => Command::SetLine {
                section: section.clone(),
                line: *line,
                fields: fields.clone(),
                comment: comment.clone(),
            },
            Self::SetText {
                section,
                line,
                text,
            } => Command::SetText {
                section: section.clone(),
                line: *line,
                text: text.clone(),
            },
            Self::AddRow {
                section,
                fields,
                comment,
            } => Command::AddRow {
                section: section.clone(),
                fields: fields.clone(),
                comment: comment.clone(),
            },
            Self::InsertText {
                section,
                line,
                text,
            } => Command::InsertText {
                section: section.clone(),
                line: *line,
                text: text.clone(),
            },
            Self::DeleteLine { section, line } => Command::DeleteLine {
                section: section.clone(),
                line: *line,
            },
            Self::DeleteObject { section, name } => Command::DeleteObject {
                section: section.clone(),
                name: name.clone(),
            },
            Self::DeleteTag { kind, name } => Command::DeleteTag {
                kind: kind.clone(),
                name: name.clone(),
            },
            Self::Rename { kind, old, new } => Command::Rename {
                kind: kind_from_name(kind).ok_or_else(|| {
                    Error::Format(format!("rename: unknown object kind {kind:?}"))
                })?,
                old: old.clone(),
                new: new.clone(),
            },
            Self::MoveNode { name, x, y } => Command::MoveNode {
                name: name.clone(),
                x: *x,
                y: *y,
            },
            Self::MoveGage { name, x, y } => Command::MoveGage {
                name: name.clone(),
                x: *x,
                y: *y,
            },
            Self::SetVertices { link, points } => Command::SetVertices {
                link: link.clone(),
                points: points.clone(),
            },
            Self::SetPolygon {
                subcatchment,
                points,
            } => Command::SetPolygon {
                subcatchment: subcatchment.clone(),
                points: points.clone(),
            },
            Self::SetOption {
                section,
                key,
                value,
            } => Command::SetOption {
                section: section.clone(),
                key: key.clone(),
                value: value.clone(),
            },
            Self::SetTitle { text } => Command::SetTitle { text: text.clone() },
            Self::Batch { edits } => Command::Batch(
                edits
                    .iter()
                    .map(Self::to_command)
                    .collect::<Result<Vec<_>>>()?,
            ),
        })
    }

    /// One line for a list: `[SUBAREAS] S1 NImperv = 0.02`.
    pub fn describe(&self) -> String {
        match self {
            Self::SetField {
                section,
                name,
                field,
                value,
            } => format!("[{section}] {name} {field} = {value}"),
            Self::SetFields { section, name, fields } => {
                format!("[{section}] {name} := {}", fields.join(" "))
            }
            Self::SetLine { section, line, fields, .. } => {
                format!("[{section}] line {line} := {}", fields.join(" "))
            }
            Self::SetText { section, line, text } => {
                format!("[{section}] line {line} text := {text}")
            }
            Self::AddRow { section, fields, .. } => {
                format!("[{section}] add {}", fields.join(" "))
            }
            Self::InsertText { section, text, .. } => format!("[{section}] insert {text}"),
            Self::DeleteLine { section, line } => format!("[{section}] delete line {line}"),
            Self::DeleteObject { section, name } => format!("[{section}] delete {name}"),
            Self::DeleteTag { kind, name } => format!("[TAGS] delete {kind} {name}"),
            Self::Rename { kind, old, new } => format!("rename {kind} {old} → {new}"),
            Self::MoveNode { name, x, y } => format!("move node {name} to ({x}, {y})"),
            Self::MoveGage { name, x, y } => format!("move gage {name} to ({x}, {y})"),
            Self::SetVertices { link, points } => {
                format!("vertices of {link}: {} point(s)", points.len())
            }
            Self::SetPolygon { subcatchment, points } => {
                format!("polygon of {subcatchment}: {} point(s)", points.len())
            }
            Self::SetOption { section, key, value } => format!("[{section}] {key} {value}"),
            Self::SetTitle { .. } => "title".to_string(),
            Self::Batch { edits } => format!("batch of {} edit(s)", edits.len()),
        }
    }

    /// Static check: does every object this edit names exist in `doc`?
    /// Returns what is missing, or `None` when the edit fits (or names
    /// nothing that has to exist first).
    fn missing_in(&self, doc: &InpDoc) -> Option<String> {
        let need_row = |section: &str, name: &str| -> Option<String> {
            (!doc.contains(section, name)).then(|| format!("[{section}] has no {name:?}"))
        };
        let need_line = |section: &str, line: usize| -> Option<String> {
            match doc.section(section) {
                None => Some(format!("no section [{section}]")),
                Some(s) if line >= s.lines.len() => Some(format!(
                    "[{section}] has {} line(s), not a line {line}",
                    s.lines.len()
                )),
                _ => None,
            }
        };
        let need_kind = |kind: ObjectKind, name: &str| -> Option<String> {
            (doc.defining_section(kind, name).is_none())
                .then(|| format!("no {} named {name:?}", kind_name(kind).to_ascii_lowercase()))
        };
        match self {
            Self::SetField { section, name, .. } | Self::SetFields { section, name, .. } => {
                need_row(section, name)
            }
            Self::SetLine { section, line, .. } | Self::SetText { section, line, .. } => {
                need_line(section, *line)
            }
            Self::DeleteLine { section, line } => need_line(section, *line),
            Self::DeleteObject { section, name } => need_row(section, name),
            Self::DeleteTag { kind, name } => {
                (doc.tag(kind, name).is_none()).then(|| format!("[TAGS] has no {kind} {name:?}"))
            }
            Self::Rename { kind, old, .. } => match kind_from_name(kind) {
                Some(k) => need_kind(k, old),
                None => Some(format!("unknown object kind {kind:?}")),
            },
            Self::MoveNode { name, .. } => need_kind(ObjectKind::Node, name),
            Self::MoveGage { name, .. } => need_kind(ObjectKind::Gage, name),
            Self::SetVertices { link, .. } => need_kind(ObjectKind::Link, link),
            Self::SetPolygon { subcatchment, .. } => {
                need_kind(ObjectKind::Subcatchment, subcatchment)
            }
            Self::InsertText { section, line, .. } => match (line, doc.section(section)) {
                (Some(l), Some(s)) if *l > s.lines.len() => {
                    Some(format!("[{section}] has no line {l}"))
                }
                _ => None,
            },
            Self::AddRow { .. } | Self::SetOption { .. } | Self::SetTitle { .. } => None,
            Self::Batch { edits } => edits.iter().find_map(|e| e.missing_in(doc)),
        }
    }
}

/// A named set of edits on the base model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub edits: Vec<ScenarioEdit>,
}

impl Scenario {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// The commands, in order. Fails on an unknown rename kind.
    pub fn commands(&self) -> Result<Vec<Command>> {
        self.edits.iter().map(ScenarioEdit::to_command).collect()
    }

    /// A scenario whose edits are one command each.
    pub fn from_commands(name: impl Into<String>, cmds: &[Command]) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            edits: cmds.iter().map(ScenarioEdit::from_command).collect(),
        }
    }
}

/// The scenarios of one model, as stored in the sidecar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenarioSet {
    pub version: u32,
    #[serde(default)]
    pub scenarios: Vec<Scenario>,
}

impl Default for ScenarioSet {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            scenarios: Vec::new(),
        }
    }
}

impl ScenarioSet {
    /// `<folder>/<stem>.scenarios.json` for `<folder>/<stem>.inp`.
    pub fn sidecar_path(model: &Path) -> PathBuf {
        let stem = model
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model".into());
        model
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}.scenarios.json"))
    }

    /// Read the sidecar. A missing file is an empty set, not an error.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| Error::Format(format!("{}: {e}", path.display())))
    }

    /// Read the sidecar beside `model`.
    pub fn load_for(model: &Path) -> Result<Self> {
        Self::load(&Self::sidecar_path(model))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Format(format!("serialise scenarios: {e}")))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn save_for(&self, model: &Path) -> Result<()> {
        self.save(&Self::sidecar_path(model))
    }

    pub fn find(&self, name: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// `base`, then `base 2`, `base 3`… — whichever is free.
    pub fn free_name(&self, base: &str) -> String {
        let base = base.trim();
        let base = if base.is_empty() { "Scenario" } else { base };
        if !self.contains(base) {
            return base.to_string();
        }
        (2u32..)
            .map(|n| format!("{base} {n}"))
            .find(|c| !self.contains(c))
            .expect("a free name exists")
    }

    /// Add with a unique name; returns the index.
    pub fn add(&mut self, mut scenario: Scenario) -> usize {
        scenario.name = self.free_name(&scenario.name);
        self.scenarios.push(scenario);
        self.scenarios.len() - 1
    }

    /// Copy scenario `i` under a `… copy` name; returns the new index.
    pub fn duplicate(&mut self, i: usize) -> Option<usize> {
        let mut copy = self.scenarios.get(i)?.clone();
        copy.name = format!("{} copy", copy.name);
        Some(self.add(copy))
    }

    /// Rename scenario `i`; refused when the name is empty or taken by
    /// another scenario.
    pub fn rename(&mut self, i: usize, name: &str) -> std::result::Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a scenario needs a name".into());
        }
        if self
            .scenarios
            .iter()
            .enumerate()
            .any(|(j, s)| j != i && s.name.eq_ignore_ascii_case(name))
        {
            return Err(format!("a scenario named {name:?} already exists"));
        }
        match self.scenarios.get_mut(i) {
            Some(s) => {
                s.name = name.to_string();
                Ok(())
            }
            None => Err("no such scenario".into()),
        }
    }

    pub fn remove(&mut self, i: usize) -> Option<Scenario> {
        (i < self.scenarios.len()).then(|| self.scenarios.remove(i))
    }
}

/// A fresh document with the same text as `base` (the document type has no
/// `Clone`; its text is its whole state).
pub fn clone_doc(base: &InpDoc) -> InpDoc {
    InpDoc::parse(&base.to_string())
}

/// Base plus scenario, as a new document. The base is untouched. Fails on
/// the first edit that does not apply, naming it.
pub fn apply(base: &InpDoc, scenario: &Scenario) -> Result<InpDoc> {
    let mut doc = clone_doc(base);
    for (i, edit) in scenario.edits.iter().enumerate() {
        let cmd = edit.to_command()?;
        doc.apply(cmd).map_err(|e| {
            Error::Format(format!(
                "scenario {:?}, edit {} ({}): {e}",
                scenario.name,
                i + 1,
                edit.describe()
            ))
        })?;
    }
    Ok(doc)
}

/// The `.inp` text of base plus scenario.
pub fn materialize(base: &InpDoc, scenario: &Scenario) -> Result<String> {
    apply(base, scenario).map(|d| d.to_string())
}

/// An edit that no longer fits the base.
#[derive(Clone, Debug, PartialEq)]
pub struct StaleEdit {
    /// Index into `Scenario::edits`.
    pub index: usize,
    pub edit: ScenarioEdit,
    pub reason: String,
}

/// Which edits are stale against `base`: each edit is checked statically
/// (does what it names exist?) and then applied to a scratch copy, so an
/// edit whose target went missing and an edit that fails for any other
/// reason are both reported. Stale edits are skipped so the ones after
/// them are still judged.
pub fn validate(base: &InpDoc, scenario: &Scenario) -> Vec<StaleEdit> {
    let mut doc = clone_doc(base);
    let mut stale = Vec::new();
    for (index, edit) in scenario.edits.iter().enumerate() {
        if let Some(reason) = edit.missing_in(&doc) {
            stale.push(StaleEdit {
                index,
                edit: edit.clone(),
                reason,
            });
            continue;
        }
        let result = edit.to_command().and_then(|cmd| doc.apply(cmd));
        if let Err(e) = result {
            stale.push(StaleEdit {
                index,
                edit: edit.clone(),
                reason: e.to_string(),
            });
        }
    }
    stale
}

// ---------------------------------------------------------------------------
// Diffing two documents into edits
// ---------------------------------------------------------------------------

/// Sections whose rows are addressed by a typed command rather than by
/// name and fields.
const POINT_SECTIONS: [&str; 4] = ["COORDINATES", "SYMBOLS", "VERTICES", "POLYGONS"];

/// Unquoted, case-folded key of a row in a named section.
fn row_key(section: &str, fields: &[String]) -> Option<String> {
    let ni = schema::name_index(section)?;
    let name = fields.get(ni)?;
    let name = crate::doc::unquote(name).to_ascii_uppercase();
    if section == "TAGS" {
        let kind = fields.first().map(|k| k.to_ascii_uppercase()).unwrap_or_default();
        Some(format!("{kind}\u{1}{name}"))
    } else {
        Some(name)
    }
}

/// Rows of `section` grouped by key, in first-seen order.
fn groups(doc: &InpDoc, section: &str) -> Vec<(String, Vec<Vec<String>>)> {
    let mut out: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    for (_, row) in doc.rows(section) {
        let Some(key) = row_key(section, &row.fields) else { continue };
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, rows)) => rows.push(row.fields.clone()),
            None => out.push((key, vec![row.fields.clone()])),
        }
    }
    out
}

fn points_of(doc: &InpDoc, section: &str, name: &str) -> Vec<(f64, f64)> {
    match section {
        "VERTICES" => doc.vertices(name),
        "POLYGONS" => doc.polygon(name),
        _ => Vec::new(),
    }
}

/// The edits that take `base` to `current`, section by section:
///
/// * named sections: a row group (all rows sharing a name — one for most
///   objects, several for a curve or a series) that changed becomes
///   `SetFields` when both sides have one row, else `DeleteObject` followed
///   by `AddRow` for each current row; groups only in `current` are added,
///   groups only in `base` are deleted;
/// * `[COORDINATES]`, `[SYMBOLS]`, `[VERTICES]`, `[POLYGONS]`: `MoveNode`,
///   `MoveGage`, `SetVertices`, `SetPolygon`;
/// * `[TITLE]`: `SetTitle`;
/// * free-text sections (`[CONTROLS]`, `[LABELS]`, `[FILES]`…): when any
///   data line differs, the old data lines are deleted and the new ones
///   inserted.
///
/// Comment and blank-line changes are not edits. The result applied to
/// `base` yields a document whose rows equal `current`'s, which is what a
/// scenario has to reproduce; the text may differ in spacing.
pub fn diff(base: &InpDoc, current: &InpDoc) -> Vec<ScenarioEdit> {
    let mut edits = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let order: Vec<String> = current
        .sections()
        .iter()
        .chain(base.sections().iter())
        .map(|s| s.name.clone())
        .filter(|n| {
            if seen.iter().any(|s| s == n) {
                false
            } else {
                seen.push(n.clone());
                true
            }
        })
        .collect();
    for section in order {
        let sec = section.as_str();
        if sec == "TITLE" {
            let (a, b) = (base.title(), current.title());
            if a != b {
                edits.push(ScenarioEdit::SetTitle { text: b });
            }
            continue;
        }
        if POINT_SECTIONS.contains(&sec) {
            diff_points(base, current, sec, &mut edits);
            continue;
        }
        if schema::name_index(sec).is_none() {
            diff_text(base, current, sec, &mut edits);
            continue;
        }
        let ga = groups(base, sec);
        let gb = groups(current, sec);
        for (key, rows_a) in &ga {
            match gb.iter().find(|(k, _)| k == key) {
                None => {
                    let name = display_name(sec, &rows_a[0]);
                    edits.push(if sec == "TAGS" {
                        ScenarioEdit::DeleteTag {
                            kind: rows_a[0][0].clone(),
                            name,
                        }
                    } else {
                        ScenarioEdit::DeleteObject {
                            section: section.clone(),
                            name,
                        }
                    });
                }
                Some((_, rows_b)) if rows_a != rows_b => {
                    let name = display_name(sec, &rows_a[0]);
                    if rows_a.len() == 1 && rows_b.len() == 1 && sec != "TAGS" {
                        edits.push(ScenarioEdit::SetFields {
                            section: section.clone(),
                            name,
                            fields: rows_b[0].clone(),
                        });
                    } else if sec == "TAGS" {
                        edits.push(ScenarioEdit::DeleteTag {
                            kind: rows_a[0][0].clone(),
                            name,
                        });
                        for r in rows_b {
                            edits.push(ScenarioEdit::AddRow {
                                section: section.clone(),
                                fields: r.clone(),
                                comment: None,
                            });
                        }
                    } else {
                        edits.push(ScenarioEdit::DeleteObject {
                            section: section.clone(),
                            name,
                        });
                        for r in rows_b {
                            edits.push(ScenarioEdit::AddRow {
                                section: section.clone(),
                                fields: r.clone(),
                                comment: None,
                            });
                        }
                    }
                }
                Some(_) => {}
            }
        }
        for (key, rows_b) in &gb {
            if !ga.iter().any(|(k, _)| k == key) {
                for r in rows_b {
                    edits.push(ScenarioEdit::AddRow {
                        section: section.clone(),
                        fields: r.clone(),
                        comment: None,
                    });
                }
            }
        }
    }
    edits
}

/// The name as written (quotes stripped) of a row in a named section.
fn display_name(section: &str, fields: &[String]) -> String {
    let ni = schema::name_index(section).unwrap_or(0);
    fields
        .get(ni)
        .map(|s| crate::doc::unquote(s).to_string())
        .unwrap_or_default()
}

fn diff_points(base: &InpDoc, current: &InpDoc, section: &str, edits: &mut Vec<ScenarioEdit>) {
    let names_a = current_names(base, section);
    let names_b = current_names(current, section);
    let mut all = names_a.clone();
    for n in &names_b {
        if !all.iter().any(|a| a.eq_ignore_ascii_case(n)) {
            all.push(n.clone());
        }
    }
    for name in all {
        match section {
            "COORDINATES" | "SYMBOLS" => {
                let a = if section == "COORDINATES" { base.coordinates(&name) } else { base.symbol(&name) };
                let b = if section == "COORDINATES" { current.coordinates(&name) } else { current.symbol(&name) };
                match (a, b) {
                    (_, Some((x, y))) if a != b => edits.push(if section == "COORDINATES" {
                        ScenarioEdit::MoveNode { name: name.clone(), x, y }
                    } else {
                        ScenarioEdit::MoveGage { name: name.clone(), x, y }
                    }),
                    (Some(_), None) => edits.push(ScenarioEdit::DeleteObject {
                        section: section.to_string(),
                        name: name.clone(),
                    }),
                    _ => {}
                }
            }
            _ => {
                let a = points_of(base, section, &name);
                let b = points_of(current, section, &name);
                if a != b {
                    edits.push(if section == "VERTICES" {
                        ScenarioEdit::SetVertices { link: name.clone(), points: b }
                    } else {
                        ScenarioEdit::SetPolygon { subcatchment: name.clone(), points: b }
                    });
                }
            }
        }
    }
}

fn current_names(doc: &InpDoc, section: &str) -> Vec<String> {
    doc.names(section)
}

fn diff_text(base: &InpDoc, current: &InpDoc, section: &str, edits: &mut Vec<ScenarioEdit>) {
    let data = |doc: &InpDoc| -> Vec<(usize, String)> {
        doc.section(section)
            .map(|s| {
                s.lines
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.row.is_some())
                    .map(|(i, l)| (i, l.text.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let a = data(base);
    let b = data(current);
    let ta: Vec<&String> = a.iter().map(|(_, t)| t).collect();
    let tb: Vec<&String> = b.iter().map(|(_, t)| t).collect();
    if ta == tb {
        return;
    }
    for (line, _) in a.iter().rev() {
        edits.push(ScenarioEdit::DeleteLine {
            section: section.to_string(),
            line: *line,
        });
    }
    for (_, text) in &b {
        edits.push(ScenarioEdit::InsertText {
            section: section.to_string(),
            line: None,
            text: text.clone(),
        });
    }
}

// ---------------------------------------------------------------------------
// Per-scenario run results
// ---------------------------------------------------------------------------

/// The headline numbers of one scenario's run, read from its report.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario: String,
    pub succeeded: bool,
    /// Largest `Max Flow` in the Outfall Loading Summary, over all
    /// outfalls (the `System` row excluded).
    pub peak_outfall_flow: Option<f64>,
    /// Sum of `Total Flood Volume` in the Node Flooding Summary (0 when
    /// no node flooded).
    pub flooding_volume: Option<f64>,
    /// Sum of `Total Volume` in the Outfall Loading Summary.
    pub outfall_volume: Option<f64>,
    pub runoff_continuity_pct: Option<f64>,
    pub routing_continuity_pct: Option<f64>,
    pub elapsed_ms: u128,
    pub error: Option<String>,
}

impl ScenarioResult {
    /// Read the numbers from a report. `succeeded` is the run's verdict,
    /// which the report alone cannot give (the `.out` must exist too).
    pub fn from_report(scenario: &str, report: &ReportSummary, succeeded: bool) -> Self {
        let table = |title: &str| report.tables.iter().find(|t| t.title == title);
        let col_values = |t: &crate::rpt::SummaryTable, needle: &str| -> Vec<f64> {
            let Some(ci) = t.column(needle) else { return Vec::new() };
            t.rows
                .iter()
                .filter(|r| !r.first().is_some_and(|n| n.eq_ignore_ascii_case("System")))
                .filter_map(|r| r.get(ci).and_then(|v| v.trim().parse::<f64>().ok()))
                .collect()
        };
        let outfalls = table("Outfall Loading Summary");
        let peak_outfall_flow = outfalls.and_then(|t| {
            col_values(t, "Max Flow")
                .into_iter()
                .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
        });
        let outfall_volume = outfalls.map(|t| col_values(t, "Total Volume").iter().sum());
        let flooding_volume = table("Node Flooding Summary").map(|t| {
            if t.rows.is_empty() {
                0.0
            } else {
                col_values(t, "Total Flood Volume").iter().sum()
            }
        });
        let cont = |needle: &str| {
            report
                .continuity
                .iter()
                .find(|(s, _)| s.to_ascii_lowercase().contains(needle))
                .map(|(_, v)| *v)
        };
        Self {
            scenario: scenario.to_string(),
            succeeded,
            peak_outfall_flow,
            flooding_volume,
            outfall_volume,
            runoff_continuity_pct: cont("runoff"),
            routing_continuity_pct: cont("routing"),
            elapsed_ms: 0,
            error: None,
        }
    }

    pub fn failed(scenario: &str, error: impl Into<String>) -> Self {
        Self {
            scenario: scenario.to_string(),
            succeeded: false,
            error: Some(error.into()),
            ..Default::default()
        }
    }
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map(|v| format!("{v:.*}", digits)).unwrap_or_default()
}

/// `Scenario,Succeeded,Peak outfall flow,Outfall volume,Flooding volume,
/// Runoff continuity %,Routing continuity %,Elapsed ms,Error`.
pub fn results_csv(results: &[ScenarioResult]) -> String {
    use crate::rpt::csv_line;
    let mut out = csv_line(&[
        "Scenario",
        "Succeeded",
        "Peak outfall flow",
        "Outfall volume",
        "Flooding volume",
        "Runoff continuity %",
        "Routing continuity %",
        "Elapsed ms",
        "Error",
    ]);
    for r in results {
        out.push_str(&csv_line(&[
            r.scenario.clone(),
            if r.succeeded { "yes".into() } else { "no".into() },
            opt(r.peak_outfall_flow, 3),
            opt(r.outfall_volume, 4),
            opt(r.flooding_volume, 4),
            opt(r.runoff_continuity_pct, 3),
            opt(r.routing_continuity_pct, 3),
            r.elapsed_ms.to_string(),
            r.error.clone().unwrap_or_default(),
        ]));
    }
    out
}

/// The synthetic model path a scenario runs under: `<stem>.scenario.<name>.inp`
/// beside the model, so the engine's scratch folder is distinct per
/// scenario while relative `[FILES]` still resolve against the model's
/// folder. The file itself is only written when the user saves.
pub fn scenario_model_path(model: &Path, scenario: &str) -> PathBuf {
    let stem = model
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    let safe: String = scenario
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    model
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}.scenario.{safe}.inp"))
}
