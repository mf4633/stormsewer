// SPDX-License-Identifier: GPL-3.0-or-later

//! A lossless, editable EPA SWMM 5 input document with command-based
//! undo/redo — the foundation the SWMM editor sits on.
//!
//! [`crate::inp`] is the *map-and-inventory* reader: it draws the model and
//! drops everything it does not understand, so it can never write a file
//! back. This module is the other half. It keeps every byte of the file and
//! offers typed access over the raw rows for the sections an editor touches
//! first, without ever discarding the rest.
//!
//! # What is preserved
//!
//! `InpDoc::parse(text).to_string() == text`, byte for byte, for every file
//! the tests have seen — including:
//!
//! * every comment line (`;;` header rulers and `;` remarks), in place;
//! * trailing `;` comments on data lines;
//! * blank lines, wherever they fall (`[OPTIONS]` groups its keys with them);
//! * section order, and sections this build has no table for (`[CONTROLS]`,
//!   `[LID_CONTROLS]`, `[STREETS]`, anything a future SWMM adds);
//! * the case of section headers (`[Polygons]` next to `[COORDINATES]`) and
//!   any trailing text on the header line;
//! * CRLF against LF line endings, per line, and a last line without one;
//! * tabs against spaces, and trailing whitespace on a line;
//! * a leading UTF-8 byte-order mark;
//! * text before the first section header.
//!
//! The model: a document is a preamble plus a list of [`Section`]s; a section
//! is its header line plus a list of [`Line`]s; a line keeps its original
//! text and, for data rows, the tokens that text parsed into. A row's text is
//! only ever rewritten when a command edits that row, and then it is
//! re-serialised whitespace-delimited, padded to the column widths of the
//! section's own `;;----` ruler (or its `;;` header, or a default of 16/10
//! characters), with the section's separator (tab if the ruler uses tabs).
//! Unedited rows never change.
//!
//! # What is not preserved
//!
//! * The exact spacing of a row once it has been edited — the values are,
//!   the padding is regenerated.
//! * Nothing else. Unknown sections are not interpreted, but they are kept.
//!
//! # Commands and undo
//!
//! Every change goes through [`InpDoc::apply`] with a [`Command`]. A command
//! expands into primitive line/section edits, each recorded with its inverse,
//! so [`InpDoc::undo`] restores the previous text exactly, including the
//! original spacing of a row the command had rewritten, and
//! [`InpDoc::redo`] reproduces the edited text exactly. A
//! [`Command::Batch`] is one undo step and applies atomically: if any part
//! fails, the parts already applied are rolled back and the document is
//! untouched. A *gesture* ([`InpDoc::begin_gesture`] / [`InpDoc::end_gesture`])
//! folds every command applied while it is open into one step, so a node
//! drag made of forty pointer moves is one Ctrl+Z. History is bounded
//! ([`InpDoc::HISTORY_LIMIT`]); [`InpDoc::mark_saved`] and [`InpDoc::dirty`]
//! track the save point through undo and redo.
//!
//! # Format traps, found while making the EPA samples round-trip
//!
//! * **`;` starts a comment anywhere on a data line, even inside quotes.**
//!   The engine (`input.c`, `getTokens`) truncates the line at the first `;`
//!   *before* it looks for quotes, so a `"label; with semicolon"` in
//!   `[LABELS]` is cut short by SWMM itself. This tokenizer mirrors that.
//! * **Double quotes group a token** (`"Arial"`, `"Site-Post.jpg"`), and the
//!   closing quote ends the token even with no whitespace after it. `""` is a
//!   real, empty field: `[DWF]` rows use it as a placeholder for unused
//!   pattern columns, so it must not be dropped or a later column shifts.
//!   Fields keep their quotes; [`Row::value`] strips them.
//! * **Object names are case-insensitive to the engine** (`hash.c` folds
//!   case), so references are matched case-insensitively and a duplicate
//!   check that compares bytes misses real duplicates.
//! * **Blank lines occur inside sections**, not just between them; the GUI
//!   writes `[OPTIONS]` in groups separated by blank lines.
//! * **Sections can be empty**: `[TAGS]` is routinely a header followed by a
//!   blank line and the next header.
//! * **Header case varies within one file**: `[Polygons]` beside
//!   `[COORDINATES]`. Names are matched uppercased.
//! * **The last line may lack a newline.** Appending after it must first
//!   give it one, and that change is part of the undo step.
//! * **Variable column layouts** decided by a keyword on the row:
//!   `[OUTFALLS]` has no stage-data field for `FREE`/`NORMAL`; `[STORAGE]`
//!   takes one curve name, three functional coefficients, or three geometric
//!   dimensions after the shape; `[DIVIDERS]` takes zero to three parameters
//!   after the type; `[OUTLETS]` takes a curve or a coefficient pair;
//!   `[XSECTIONS]` `CUSTOM`/`IRREGULAR`/`STREET` drop the Geom columns;
//!   `[RAINGAGES]` `FILE` sources carry a station and units.
//! * **`[INFILTRATION]` columns depend on `[OPTIONS] INFILTRATION`**, and
//!   SWMM 5.2 lets a row end with its own method keyword, which wins.
//! * **`[TIMESERIES]` rows are `Name [Date] Time Value`** with the date
//!   optional, so the field count decides the layout — and a row may carry
//!   several time/value pairs. `[CURVES]` and `[PATTERNS]` name their type on
//!   the first row only; continuation rows have one field fewer, and also
//!   may carry several pairs. Named access covers the first pair.
//! * **`[TITLE]` is free text**, but the engine still skips title lines that
//!   begin with `;` and blank lines, so only data rows count as the title.
//! * **`[CONTROLS]` names objects in free text** (`IF NODE J1 DEPTH > 2`);
//!   a rename must patch the token after `NODE`, `LINK`, `PUMP`, `ORIFICE`,
//!   `WEIR`, `OUTLET`, `CONDUIT`, `GAGE`, or the rule silently stops
//!   matching.
//! * **`[SUBCATCHMENTS] Outlet` may name a node or another subcatchment**,
//!   and `[OUTFALLS] RouteTo` names a subcatchment, so renaming either kind
//!   must look there.
//! * **Key/value sections vary key case** (`Units` in `[MAP]`, `UNITS`
//!   elsewhere); keys are matched case-insensitively and the existing
//!   spelling is kept on edit.
//! * **`[LABELS]` rows have no name**, only a position and a quoted label;
//!   the fourth field is an anchor node reference that must follow a rename.
//! * **`[LID_CONTROLS]` mixes row shapes** (`Name Type` then
//!   `Name Layer p1 p2 ...`), as do other multi-row sections; those stay
//!   untyped here and are edited by row.
//! * **Trailing whitespace** on data lines (`ROUTING_STEP 0:00:15 `) and on
//!   comment rulers is common and must survive unedited.

pub mod build;
pub mod schema;
pub mod validate;

use std::fmt;
use std::fs;
use std::path::Path;

use crate::{Error, Result};
pub use schema::ObjectKind;
pub use validate::{Finding, Severity};

/// How a line ended in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    CrLf,
    Lf,
    /// Only the last line of a file can have no ending.
    None,
}

impl Ending {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CrLf => "\r\n",
            Self::Lf => "\n",
            Self::None => "",
        }
    }
}

/// The parsed content of a data line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Whitespace-delimited tokens, quotes kept.
    pub fields: Vec<String>,
    /// The trailing comment, from its `;` to the end of the line, if any.
    pub comment: Option<String>,
}

impl Row {
    /// Field `i` with surrounding double quotes removed.
    pub fn value(&self, i: usize) -> Option<&str> {
        self.fields.get(i).map(|s| unquote(s))
    }

    /// The field with the given column name, given this row's layout.
    pub fn get(&self, columns: &[&str], name: &str) -> Option<&str> {
        schema::field_index(columns, name).and_then(|i| self.value(i))
    }
}

/// One line of a section: the original text, its ending, and the tokens if
/// it is a data row. Comments, blank lines, and rulers have `row == None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The text without its line ending. Verbatim until the line is edited.
    pub text: String,
    pub ending: Ending,
    pub row: Option<Row>,
}

impl Line {
    fn from_text(text: &str, ending: Ending) -> Self {
        let t = text.trim();
        let row = if t.is_empty() || t.starts_with(';') {
            None
        } else {
            let (fields, comment) = tokenize(text);
            Some(Row { fields, comment })
        };
        Self {
            text: text.to_string(),
            ending,
            row,
        }
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn is_comment(&self) -> bool {
        self.text.trim_start().starts_with(';')
    }
}

/// A section: its header line, verbatim, and its lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The header line as written, e.g. `[Polygons]`.
    pub header: String,
    /// The name between the brackets, uppercased.
    pub name: String,
    pub ending: Ending,
    pub lines: Vec<Line>,
}

impl Section {
    /// Data rows with their line indices.
    pub fn rows(&self) -> impl Iterator<Item = (usize, &Row)> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.row.as_ref().map(|r| (i, r)))
    }
}

// ---------------------------------------------------------------------------
// Tokenizer and renderer
// ---------------------------------------------------------------------------

fn is_sep(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// Split a data line the way the engine does: cut at the first `;`, then
/// whitespace-delimited tokens where a token starting with `"` runs to the
/// next `"`. Returns the tokens (quotes kept) and the comment (from its `;`,
/// trailing whitespace trimmed).
pub fn tokenize(text: &str) -> (Vec<String>, Option<String>) {
    let (data, comment) = match text.find(';') {
        Some(i) => (&text[..i], Some(text[i..].trim_end().to_string())),
        None => (text, None),
    };
    let bytes = data.as_bytes();
    let mut fields = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if is_sep(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            while i < bytes.len() && !is_sep(bytes[i]) {
                i += 1;
            }
        }
        fields.push(data[start..i].to_string());
    }
    (fields, comment)
}

/// Strip one pair of surrounding double quotes.
pub fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn same_name(a: &str, b: &str) -> bool {
    unquote(a).eq_ignore_ascii_case(unquote(b))
}

/// Quote a field for output when it would otherwise not survive
/// tokenizing: empty, or containing whitespace.
fn quote_if_needed(s: &str) -> String {
    if s.is_empty() || (!s.starts_with('"') && s.chars().any(char::is_whitespace)) {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

/// Format a coordinate: integers as such, otherwise up to three decimals.
pub fn format_number(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// Column widths and separator for re-serialising a row in a section.
struct Layout {
    widths: Vec<usize>,
    sep: &'static str,
    /// Free-text sections (`[CONTROLS]`, `[TITLE]`): single spaces, no
    /// padding, or `IF NODE J1 DEPTH > 2` turns into a column table.
    compact: bool,
}

impl Layout {
    const DEFAULT_FIRST: usize = 16;
    const DEFAULT_REST: usize = 10;

    fn of(section: &Section) -> Self {
        if schema::name_index(&section.name).is_none() && section.name != "LABELS" {
            return Self {
                widths: Vec::new(),
                sep: " ",
                compact: true,
            };
        }
        // Prefer the ruler: `;;---------------- ---------- ...`.
        for line in &section.lines {
            let t = line.text.trim_end();
            if t.starts_with(";;-") && t[2..].chars().all(|c| c == '-' || c.is_whitespace()) {
                let sep = if t.contains('\t') { "\t" } else { " " };
                let mut widths: Vec<usize> = t[2..].split_whitespace().map(str::len).collect();
                if !widths.is_empty() {
                    widths[0] += 2;
                    return Self {
                        widths,
                        sep,
                        compact: false,
                    };
                }
            }
        }
        // Then a tab-separated `;;Option    \tValue` header.
        for line in &section.lines {
            let t = line.text.trim_end();
            if t.starts_with(";;") && t.contains('\t') {
                let mut widths: Vec<usize> =
                    t[2..].split('\t').map(|s| s.chars().count()).collect();
                if widths.len() >= 2 {
                    widths[0] += 2;
                    return Self {
                        widths,
                        sep: "\t",
                        compact: false,
                    };
                }
            }
        }
        let tabs = section
            .lines
            .iter()
            .any(|l| l.row.is_some() && l.text.contains('\t'));
        Self {
            widths: Vec::new(),
            sep: if tabs { "\t" } else { " " },
            compact: false,
        }
    }

    fn width(&self, i: usize) -> usize {
        self.widths.get(i).copied().unwrap_or(if i == 0 {
            Self::DEFAULT_FIRST
        } else {
            Self::DEFAULT_REST
        })
    }

    fn render(&self, fields: &[String], comment: Option<&str>) -> String {
        let mut out = String::new();
        let n = fields.len();
        for (i, f) in fields.iter().enumerate() {
            let f = quote_if_needed(f);
            out.push_str(&f);
            if i + 1 < n || comment.is_some() {
                if !self.compact {
                    let pad = self.width(i).saturating_sub(f.chars().count());
                    out.extend(std::iter::repeat_n(' ', pad));
                }
                out.push_str(self.sep);
            }
        }
        if let Some(c) = comment {
            out.push_str(c);
        }
        out
    }
}

fn header_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    if !t.starts_with('[') {
        return None;
    }
    let end = t.find(']')?;
    Some(t[1..end].trim().to_ascii_uppercase())
}

fn split_lines(text: &str) -> Vec<(&str, Ending)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(i) => {
                let (line, ending) = if i > 0 && rest.as_bytes()[i - 1] == b'\r' {
                    (&rest[..i - 1], Ending::CrLf)
                } else {
                    (&rest[..i], Ending::Lf)
                };
                out.push((line, ending));
                rest = &rest[i + 1..];
            }
            None => {
                out.push((rest, Ending::None));
                rest = "";
            }
        }
    }
    out
}

fn check_field(s: &str) -> Result<()> {
    if s.contains(';') || s.contains('\n') || s.contains('\r') {
        return Err(Error::Format(format!(
            "field value {s:?} may not contain ';' or a line break"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands and edits
// ---------------------------------------------------------------------------

/// An editing operation. Section names are case-insensitive; object names
/// are matched the way the engine matches them (case-insensitive, quotes
/// ignored). Field names come from [`schema::columns`], or may be a decimal
/// column index.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Set one field of the first row named `name` in `section`. The field
    /// may be the one just past the row's last (appending); further gaps are
    /// an error, since a default for the skipped columns would be a guess.
    SetField {
        section: String,
        name: String,
        field: String,
        value: String,
    },
    /// Replace every field of the first row named `name`.
    SetFields {
        section: String,
        name: String,
        fields: Vec<String>,
    },
    /// Replace the fields of the row at line index `line` in `section`.
    SetLine {
        section: String,
        line: usize,
        fields: Vec<String>,
        comment: Option<String>,
    },
    /// Replace the raw text of the line at `line` — for comments,
    /// `[CONTROLS]` rules, and other free text.
    SetText {
        section: String,
        line: usize,
        text: String,
    },
    /// Append a data row to `section` (after its last non-blank line),
    /// creating the section at the end of the file if it is missing.
    AddRow {
        section: String,
        fields: Vec<String>,
        comment: Option<String>,
    },
    /// Insert a raw text line at `line`, or at the append position if `None`.
    InsertText {
        section: String,
        line: Option<usize>,
        text: String,
    },
    /// Remove the line at `line`.
    DeleteLine { section: String, line: usize },
    /// Remove every row named `name` in `section`. Removing nothing is not
    /// an error.
    DeleteObject { section: String, name: String },
    /// Remove the `[TAGS]` row for `kind` (`Node`, `Link`, `Subcatch`,
    /// `Gage`) and `name`.
    DeleteTag { kind: String, name: String },
    /// Rename an object and every reference to it: its defining row, link
    /// endpoints, subcatchment outlets, inflows, tags, coordinates,
    /// vertices, polygons, curve/series/pattern uses, `[LABELS]` anchors, and
    /// `[CONTROLS]` rules.
    Rename {
        kind: ObjectKind,
        old: String,
        new: String,
    },
    /// Set a node's `[COORDINATES]`, adding the row if it has none.
    MoveNode { name: String, x: f64, y: f64 },
    /// Set a rain gage's `[SYMBOLS]` position, adding the row if it has none.
    MoveGage { name: String, x: f64, y: f64 },
    /// Replace a link's `[VERTICES]` rows. An empty list removes them.
    SetVertices {
        link: String,
        points: Vec<(f64, f64)>,
    },
    /// Replace a subcatchment's `[POLYGONS]` rows. An empty list removes them.
    SetPolygon {
        subcatchment: String,
        points: Vec<(f64, f64)>,
    },
    /// Set a `KEY value...` row in a key/value section (`OPTIONS`, `REPORT`,
    /// `MAP`, ...). `value` is tokenized like a data line. The key's existing
    /// spelling is kept; a missing key is appended.
    SetOption {
        section: String,
        key: String,
        value: String,
    },
    /// Replace the `[TITLE]` text (lines split on `\n`).
    SetTitle { text: String },
    /// Several commands as one undo step, applied atomically.
    Batch(Vec<Command>),
}

/// A primitive, invertible change.
#[derive(Clone, Debug)]
enum Edit {
    ReplaceLine {
        section: usize,
        line: usize,
        old: Line,
        new: Line,
    },
    InsertLine {
        section: usize,
        line: usize,
        new: Line,
    },
    RemoveLine {
        section: usize,
        line: usize,
        old: Line,
    },
    ReplaceHeader {
        section: usize,
        old: (String, Ending),
        new: (String, Ending),
    },
    ReplacePreambleLine {
        line: usize,
        old: Line,
        new: Line,
    },
    InsertSection {
        index: usize,
        section: Section,
    },
    RemoveSection {
        index: usize,
        section: Section,
    },
}

impl Edit {
    fn inverse(&self) -> Edit {
        match self {
            Edit::ReplaceLine {
                section,
                line,
                old,
                new,
            } => Edit::ReplaceLine {
                section: *section,
                line: *line,
                old: new.clone(),
                new: old.clone(),
            },
            Edit::InsertLine { section, line, new } => Edit::RemoveLine {
                section: *section,
                line: *line,
                old: new.clone(),
            },
            Edit::RemoveLine { section, line, old } => Edit::InsertLine {
                section: *section,
                line: *line,
                new: old.clone(),
            },
            Edit::ReplaceHeader { section, old, new } => Edit::ReplaceHeader {
                section: *section,
                old: new.clone(),
                new: old.clone(),
            },
            Edit::ReplacePreambleLine { line, old, new } => Edit::ReplacePreambleLine {
                line: *line,
                old: new.clone(),
                new: old.clone(),
            },
            Edit::InsertSection { index, section } => Edit::RemoveSection {
                index: *index,
                section: section.clone(),
            },
            Edit::RemoveSection { index, section } => Edit::InsertSection {
                index: *index,
                section: section.clone(),
            },
        }
    }
}

#[derive(Debug)]
struct Step {
    id: u64,
    edits: Vec<Edit>,
}

/// A point in a timeseries, fields as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesPoint {
    pub date: Option<String>,
    pub time: String,
    pub value: String,
}

/// A lossless SWMM input document. See the module documentation.
#[derive(Debug)]
pub struct InpDoc {
    bom: bool,
    preamble: Vec<Line>,
    sections: Vec<Section>,
    undo_stack: Vec<Step>,
    redo_stack: Vec<Step>,
    gesture: Option<Vec<Edit>>,
    gesture_depth: usize,
    next_id: u64,
    /// Id of the state under the bottom of the undo stack: 0 for the parsed
    /// text, or the id of the last step dropped by the history bound.
    base_id: u64,
    /// Id of the state that was last saved.
    save_point: u64,
    /// Bumped on every primitive edit, including undo and redo, so a
    /// caller holding derived data (drawing caches) can tell cheaply
    /// whether the text it derived from is still the current text.
    generation: u64,
}

impl Default for InpDoc {
    fn default() -> Self {
        Self::parse("")
    }
}

impl fmt::Display for InpDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bom {
            f.write_str("\u{feff}")?;
        }
        for l in &self.preamble {
            f.write_str(&l.text)?;
            f.write_str(l.ending.as_str())?;
        }
        for s in &self.sections {
            f.write_str(&s.header)?;
            f.write_str(s.ending.as_str())?;
            for l in &s.lines {
                f.write_str(&l.text)?;
                f.write_str(l.ending.as_str())?;
            }
        }
        Ok(())
    }
}

impl InpDoc {
    /// Undo steps kept. Older steps are dropped.
    pub const HISTORY_LIMIT: usize = 500;

    /// Parse `.inp` text. Never fails: text with no section header is a
    /// document of one preamble.
    pub fn parse(text: &str) -> Self {
        let (bom, body) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let mut preamble = Vec::new();
        let mut sections: Vec<Section> = Vec::new();
        for (text, ending) in split_lines(body) {
            if let Some(name) = header_name(text) {
                sections.push(Section {
                    header: text.to_string(),
                    name,
                    ending,
                    lines: Vec::new(),
                });
                continue;
            }
            let line = Line::from_text(text, ending);
            match sections.last_mut() {
                Some(s) => s.lines.push(line),
                None => preamble.push(line),
            }
        }
        Self {
            bom,
            preamble,
            sections,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            gesture: None,
            gesture_depth: 0,
            next_id: 1,
            base_id: 0,
            save_point: 0,
            generation: 0,
        }
    }

    /// A counter that changes whenever the text may have changed (every
    /// applied edit, undo, or redo). Equal values mean identical text; it
    /// never goes backwards, so an undo produces a new value even though it
    /// restores old text.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Read and parse a file.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        let text = String::from_utf8(bytes)
            .map_err(|e| Error::Format(format!("{}: not UTF-8: {e}", path.display())))?;
        Ok(Self::parse(&text))
    }

    /// Write the document and mark the save point.
    pub fn write(&mut self, path: &Path) -> Result<()> {
        fs::write(path, self.to_string())?;
        self.mark_saved();
        Ok(())
    }

    // -- structure ---------------------------------------------------------

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Lines before the first section header.
    pub fn preamble(&self) -> &[Line] {
        &self.preamble
    }

    pub fn has_bom(&self) -> bool {
        self.bom
    }

    fn section_index(&self, name: &str) -> Option<usize> {
        self.sections
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The first section with this name (any case).
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.section_index(name).map(|i| &self.sections[i])
    }

    /// The line ending new lines get: the file's first, else CRLF.
    pub fn newline(&self) -> Ending {
        self.preamble
            .iter()
            .map(|l| l.ending)
            .chain(
                self.sections.iter().flat_map(|s| {
                    std::iter::once(s.ending).chain(s.lines.iter().map(|l| l.ending))
                }),
            )
            .find(|e| *e != Ending::None)
            .unwrap_or(Ending::CrLf)
    }

    // -- typed access -------------------------------------------------------

    /// The `[OPTIONS] INFILTRATION` value, which decides `[INFILTRATION]`
    /// columns.
    fn infiltration_method(&self) -> Option<String> {
        self.option("INFILTRATION").map(|s| s.to_string())
    }

    /// Column names for a row of `section`, resolving the variable layouts.
    pub fn columns(&self, section: &str, row: &Row) -> &'static [&'static str] {
        schema::columns(
            &section.trim().to_ascii_uppercase(),
            &row.fields,
            self.infiltration_method().as_deref(),
        )
    }

    /// Index of `field` (a column name or a decimal index) in `row`.
    fn resolve_field(&self, section: &str, row: &Row, field: &str) -> Result<usize> {
        if let Ok(i) = field.parse::<usize>() {
            return Ok(i);
        }
        let cols = self.columns(section, row);
        schema::field_index(cols, field).ok_or_else(|| {
            Error::NotFound(format!(
                "[{section}] has no field {field:?} for this row (columns: {})",
                cols.join(" ")
            ))
        })
    }

    /// Data rows of `section` with their line indices.
    pub fn rows(&self, section: &str) -> Vec<(usize, &Row)> {
        self.section(section)
            .map(|s| s.rows().collect())
            .unwrap_or_default()
    }

    /// Distinct object names in `section`, first-seen order, as written.
    pub fn names(&self, section: &str) -> Vec<String> {
        let Some(ni) = schema::name_index(&section.trim().to_ascii_uppercase()) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for (_, r) in self.rows(section) {
            if let Some(n) = r.value(ni) {
                if !out.iter().any(|o| same_name(o, n)) {
                    out.push(n.to_string());
                }
            }
        }
        out
    }

    /// Line indices of every row named `name` in `section`.
    fn find_lines(&self, section: &str, name: &str) -> Vec<usize> {
        let upper = section.trim().to_ascii_uppercase();
        let Some(ni) = schema::name_index(&upper) else {
            return Vec::new();
        };
        self.rows(section)
            .into_iter()
            .filter(|(_, r)| r.fields.get(ni).is_some_and(|f| same_name(f, name)))
            .map(|(i, _)| i)
            .collect()
    }

    /// The first row named `name` in `section`, with its line index.
    pub fn find(&self, section: &str, name: &str) -> Option<(usize, &Row)> {
        let li = *self.find_lines(section, name).first()?;
        self.section(section)?.lines[li]
            .row
            .as_ref()
            .map(|r| (li, r))
    }

    /// Every row named `name` in `section` (vertices, series, curves...).
    pub fn find_all(&self, section: &str, name: &str) -> Vec<&Row> {
        let Some(s) = self.section(section) else {
            return Vec::new();
        };
        self.find_lines(section, name)
            .into_iter()
            .filter_map(|li| s.lines[li].row.as_ref())
            .collect()
    }

    /// A named field of the first row named `name`, unquoted.
    pub fn field(&self, section: &str, name: &str, field: &str) -> Option<&str> {
        let (_, row) = self.find(section, name)?;
        let i = self.resolve_field(section, row, field).ok()?;
        row.value(i)
    }

    /// Whether any row in `section` is named `name`.
    pub fn contains(&self, section: &str, name: &str) -> bool {
        !self.find_lines(section, name).is_empty()
    }

    /// Which of the sections defining `kind` holds `name`.
    pub fn defining_section(&self, kind: ObjectKind, name: &str) -> Option<&'static str> {
        kind.defining_sections()
            .iter()
            .copied()
            .find(|s| self.contains(s, name))
    }

    /// A `KEY value...` row's value in a key/value section, joined with
    /// single spaces.
    pub fn key_value(&self, section: &str, key: &str) -> Option<String> {
        let (_, row) = self.find(section, key)?;
        Some(row.fields[1..].join(" "))
    }

    /// An `[OPTIONS]` value.
    pub fn option(&self, key: &str) -> Option<&str> {
        let (_, row) = self.find("OPTIONS", key)?;
        row.value(1)
    }

    /// The `[TITLE]` data lines joined with `\n`.
    pub fn title(&self) -> String {
        self.section("TITLE")
            .map(|s| {
                s.lines
                    .iter()
                    .filter(|l| l.row.is_some())
                    .map(|l| l.text.trim().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    fn xy_of(row: &Row) -> Option<(f64, f64)> {
        let x = row.value(1)?.parse().ok()?;
        let y = row.value(2)?.parse().ok()?;
        Some((x, y))
    }

    /// A node's `[COORDINATES]`.
    pub fn coordinates(&self, node: &str) -> Option<(f64, f64)> {
        self.find("COORDINATES", node)
            .and_then(|(_, r)| Self::xy_of(r))
    }

    /// A rain gage's `[SYMBOLS]` position.
    pub fn symbol(&self, gage: &str) -> Option<(f64, f64)> {
        self.find("SYMBOLS", gage).and_then(|(_, r)| Self::xy_of(r))
    }

    /// A link's `[VERTICES]`, in file order.
    pub fn vertices(&self, link: &str) -> Vec<(f64, f64)> {
        self.find_all("VERTICES", link)
            .into_iter()
            .filter_map(Self::xy_of)
            .collect()
    }

    /// A subcatchment's `[POLYGONS]` outline, in file order.
    pub fn polygon(&self, subcatchment: &str) -> Vec<(f64, f64)> {
        self.find_all("POLYGONS", subcatchment)
            .into_iter()
            .filter_map(Self::xy_of)
            .collect()
    }

    /// The `[TAGS]` tag for an object, by tag kind keyword (`Node`, `Link`,
    /// `Subcatch`, `Gage`).
    pub fn tag(&self, kind: &str, name: &str) -> Option<&str> {
        self.rows("TAGS")
            .into_iter()
            .find(|(_, r)| {
                r.fields
                    .first()
                    .is_some_and(|k| k.eq_ignore_ascii_case(kind))
                    && r.fields.get(1).is_some_and(|n| same_name(n, name))
            })
            .and_then(|(_, r)| r.value(2))
    }

    /// `[INFLOWS]` rows for a node.
    pub fn inflows(&self, node: &str) -> Vec<&Row> {
        self.find_all("INFLOWS", node)
    }

    /// A `[TIMESERIES]` series as points, or an empty list for a `FILE`
    /// series. Rows carrying several time/value pairs are unrolled.
    pub fn timeseries(&self, name: &str) -> Vec<SeriesPoint> {
        let mut out = Vec::new();
        for r in self.find_all("TIMESERIES", name) {
            if r.fields
                .get(1)
                .is_some_and(|f| f.eq_ignore_ascii_case("FILE"))
            {
                continue;
            }
            let mut date: Option<String> = None;
            let mut it = r.fields.iter().skip(1).map(|s| unquote(s));
            while let Some(tok) = it.next() {
                if tok.contains('/') || (tok.contains('-') && !tok.starts_with('-')) {
                    date = Some(tok.to_string());
                    continue;
                }
                let Some(value) = it.next() else { break };
                out.push(SeriesPoint {
                    date: date.clone(),
                    time: tok.to_string(),
                    value: value.to_string(),
                });
            }
        }
        out
    }

    /// A `[CURVES]` curve: its type (from the first row) and its points.
    pub fn curve(&self, name: &str) -> (Option<String>, Vec<(String, String)>) {
        let mut kind = None;
        let mut points = Vec::new();
        for r in self.find_all("CURVES", name) {
            let cols = self.columns("CURVES", r);
            let start = if cols.len() == 4 {
                kind = r.value(1).map(|s| s.to_string());
                2
            } else {
                1
            };
            let vals: Vec<&str> = r.fields.iter().skip(start).map(|s| unquote(s)).collect();
            for pair in vals.chunks(2) {
                if let [x, y] = pair {
                    points.push((x.to_string(), y.to_string()));
                }
            }
        }
        (kind, points)
    }

    /// Referential checks. See [`validate`].
    pub fn validate(&self) -> Vec<Finding> {
        validate::validate(self)
    }

    // -- cascade builders ---------------------------------------------------

    /// The batch that removes a node together with its `[COORDINATES]`,
    /// `[TAGS]`, `[INFLOWS]`, `[DWF]`, `[RDII]`, `[TREATMENT]` rows and every
    /// link attached to it (with that link's own dependent rows). Apply it
    /// with [`InpDoc::apply`]; offered separately so the caller can show what
    /// goes.
    pub fn cascade_delete_node(&self, name: &str) -> Command {
        let mut cmds = Vec::new();
        for sec in schema::LINK_SECTIONS {
            for (_, r) in self.rows(sec) {
                let attached = r.fields.get(1).is_some_and(|f| same_name(f, name))
                    || r.fields.get(2).is_some_and(|f| same_name(f, name));
                if attached {
                    if let Some(link) = r.value(0) {
                        cmds.push(self.cascade_delete_link(link));
                    }
                }
            }
        }
        for sec in schema::NODE_SECTIONS
            .iter()
            .chain(["COORDINATES", "INFLOWS", "DWF", "RDII", "TREATMENT"].iter())
        {
            cmds.push(Command::DeleteObject {
                section: sec.to_string(),
                name: name.to_string(),
            });
        }
        cmds.push(Command::DeleteTag {
            kind: "Node".into(),
            name: name.to_string(),
        });
        Command::Batch(cmds)
    }

    /// The batch that removes a link with its `[XSECTIONS]`, `[LOSSES]`,
    /// `[VERTICES]`, `[INLET_USAGE]` and `[TAGS]` rows.
    pub fn cascade_delete_link(&self, name: &str) -> Command {
        let mut cmds: Vec<Command> = schema::LINK_SECTIONS
            .iter()
            .chain(["XSECTIONS", "LOSSES", "VERTICES", "INLET_USAGE"].iter())
            .map(|sec| Command::DeleteObject {
                section: sec.to_string(),
                name: name.to_string(),
            })
            .collect();
        cmds.push(Command::DeleteTag {
            kind: "Link".into(),
            name: name.to_string(),
        });
        Command::Batch(cmds)
    }

    /// The batch that removes a subcatchment with its `[SUBAREAS]`,
    /// `[INFILTRATION]`, `[GROUNDWATER]`, `[LID_USAGE]`, `[COVERAGES]`,
    /// `[LOADINGS]`, `[POLYGONS]` and `[TAGS]` rows.
    pub fn cascade_delete_subcatchment(&self, name: &str) -> Command {
        let mut cmds: Vec<Command> = [
            "SUBCATCHMENTS",
            "SUBAREAS",
            "INFILTRATION",
            "GROUNDWATER",
            "LID_USAGE",
            "COVERAGES",
            "LOADINGS",
            "POLYGONS",
        ]
        .iter()
        .map(|sec| Command::DeleteObject {
            section: sec.to_string(),
            name: name.to_string(),
        })
        .collect();
        cmds.push(Command::DeleteTag {
            kind: "Subcatch".into(),
            name: name.to_string(),
        });
        Command::Batch(cmds)
    }

    // -- undo / redo --------------------------------------------------------

    /// Apply a command, recording one undo step (or adding to the open
    /// gesture). On error the document is unchanged.
    pub fn apply(&mut self, cmd: Command) -> Result<()> {
        let mut edits = Vec::new();
        if let Err(e) = self.exec(&cmd, &mut edits) {
            for edit in edits.iter().rev() {
                self.perform(&edit.inverse());
            }
            return Err(e);
        }
        if edits.is_empty() {
            return Ok(());
        }
        self.redo_stack.clear();
        match &mut self.gesture {
            Some(g) => g.extend(edits),
            None => self.push_step(edits),
        }
        Ok(())
    }

    fn push_step(&mut self, edits: Vec<Edit>) {
        let id = self.next_id;
        self.next_id += 1;
        self.undo_stack.push(Step { id, edits });
        if self.undo_stack.len() > Self::HISTORY_LIMIT {
            let excess = self.undo_stack.len() - Self::HISTORY_LIMIT;
            if let Some(dropped) = self.undo_stack.drain(..excess).next_back() {
                self.base_id = dropped.id;
            }
        }
    }

    /// Start folding commands into one undo step. Nests; the outermost
    /// [`InpDoc::end_gesture`] closes it.
    pub fn begin_gesture(&mut self) {
        if self.gesture.is_none() {
            self.gesture = Some(Vec::new());
        }
        self.gesture_depth += 1;
    }

    /// Close the gesture; the commands applied since it began become one
    /// step (none, if nothing changed).
    pub fn end_gesture(&mut self) {
        if self.gesture_depth == 0 {
            return;
        }
        self.gesture_depth -= 1;
        if self.gesture_depth == 0 {
            self.close_gesture();
        }
    }

    fn close_gesture(&mut self) {
        self.gesture_depth = 0;
        if let Some(edits) = self.gesture.take() {
            if !edits.is_empty() {
                self.push_step(edits);
            }
        }
    }

    /// Undo one step (closing any open gesture first). Returns whether
    /// anything was undone.
    pub fn undo(&mut self) -> bool {
        self.close_gesture();
        let Some(step) = self.undo_stack.pop() else {
            return false;
        };
        for edit in step.edits.iter().rev() {
            self.perform(&edit.inverse());
        }
        self.redo_stack.push(step);
        true
    }

    /// Redo the last undone step.
    pub fn redo(&mut self) -> bool {
        self.close_gesture();
        let Some(step) = self.redo_stack.pop() else {
            return false;
        };
        for edit in &step.edits {
            self.perform(edit);
        }
        self.undo_stack.push(step);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || self.gesture.as_ref().is_some_and(|g| !g.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Number of undo steps held.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// Id of the current state: the top undo step, or the base.
    fn current_id(&self) -> u64 {
        self.undo_stack.last().map_or(self.base_id, |s| s.id)
    }

    /// Whether the text differs from the last save point. A parsed document
    /// starts clean.
    pub fn dirty(&self) -> bool {
        self.current_id() != self.save_point || self.gesture.as_ref().is_some_and(|g| !g.is_empty())
    }

    /// Record that the current text has been saved (closing any open
    /// gesture).
    pub fn mark_saved(&mut self) {
        self.close_gesture();
        self.save_point = self.current_id();
    }

    fn perform(&mut self, edit: &Edit) {
        self.generation += 1;
        match edit {
            Edit::ReplaceLine {
                section, line, new, ..
            } => self.sections[*section].lines[*line] = new.clone(),
            Edit::InsertLine { section, line, new } => {
                self.sections[*section].lines.insert(*line, new.clone())
            }
            Edit::RemoveLine { section, line, .. } => {
                self.sections[*section].lines.remove(*line);
            }
            Edit::ReplaceHeader { section, new, .. } => {
                let s = &mut self.sections[*section];
                s.header = new.0.clone();
                s.name = header_name(&new.0).unwrap_or_default();
                s.ending = new.1;
            }
            Edit::ReplacePreambleLine { line, new, .. } => self.preamble[*line] = new.clone(),
            Edit::InsertSection { index, section } => self.sections.insert(*index, section.clone()),
            Edit::RemoveSection { index, .. } => {
                self.sections.remove(*index);
            }
        }
    }

    fn do_edit(&mut self, edit: Edit, edits: &mut Vec<Edit>) {
        self.perform(&edit);
        edits.push(edit);
    }

    // -- primitive operations ------------------------------------------------

    fn replace_line(&mut self, si: usize, li: usize, new: Line, edits: &mut Vec<Edit>) {
        let old = self.sections[si].lines[li].clone();
        if old != new {
            self.do_edit(
                Edit::ReplaceLine {
                    section: si,
                    line: li,
                    old,
                    new,
                },
                edits,
            );
        }
    }

    /// Give the line before an insertion point a line ending if it has none.
    fn ensure_ending_before(&mut self, si: usize, li: usize, edits: &mut Vec<Edit>) {
        let nl = self.newline();
        if li == 0 {
            let s = &self.sections[si];
            if s.ending == Ending::None {
                let old = (s.header.clone(), s.ending);
                let new = (s.header.clone(), nl);
                self.do_edit(
                    Edit::ReplaceHeader {
                        section: si,
                        old,
                        new,
                    },
                    edits,
                );
            }
        } else {
            let prev = &self.sections[si].lines[li - 1];
            if prev.ending == Ending::None {
                let mut new = prev.clone();
                new.ending = nl;
                self.replace_line(si, li - 1, new, edits);
            }
        }
    }

    /// Insert a line. A line appended after the file's last line keeps the
    /// file's habit: if that line had no newline, it gets one and the new
    /// line goes without.
    fn insert_line(
        &mut self,
        si: usize,
        li: usize,
        text: String,
        row: Option<Row>,
        edits: &mut Vec<Edit>,
    ) {
        let prev_unterminated = if li == 0 {
            self.sections[si].ending == Ending::None
        } else {
            self.sections[si].lines[li - 1].ending == Ending::None
        };
        self.ensure_ending_before(si, li, edits);
        let ending = if prev_unterminated {
            Ending::None
        } else {
            self.newline()
        };
        self.do_edit(
            Edit::InsertLine {
                section: si,
                line: li,
                new: Line { text, ending, row },
            },
            edits,
        );
    }

    fn remove_line(&mut self, si: usize, li: usize, edits: &mut Vec<Edit>) {
        let old = self.sections[si].lines[li].clone();
        self.do_edit(
            Edit::RemoveLine {
                section: si,
                line: li,
                old,
            },
            edits,
        );
    }

    /// Where a new row goes: after the section's last non-blank line.
    fn append_position(&self, si: usize) -> usize {
        self.sections[si]
            .lines
            .iter()
            .rposition(|l| !l.is_blank())
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// The section's index, creating it at the end of the file if missing.
    fn ensure_section(&mut self, name: &str, edits: &mut Vec<Edit>) -> usize {
        let upper = name.trim().to_ascii_uppercase();
        if let Some(i) = self.section_index(&upper) {
            return i;
        }
        let nl = self.newline();
        // Separate from the previous section with a blank line, and make
        // sure the previous last line ends.
        if let Some(prev) = self.sections.len().checked_sub(1) {
            let n = self.sections[prev].lines.len();
            self.ensure_ending_before(prev, n, edits);
            if n == 0 || !self.sections[prev].lines[n - 1].is_blank() {
                self.do_edit(
                    Edit::InsertLine {
                        section: prev,
                        line: n,
                        new: Line {
                            text: String::new(),
                            ending: nl,
                            row: None,
                        },
                    },
                    edits,
                );
            }
        } else if let Some(last) = self.preamble.last() {
            if last.ending == Ending::None {
                let mut new = last.clone();
                new.ending = nl;
                let line = self.preamble.len() - 1;
                self.do_edit(
                    Edit::ReplacePreambleLine {
                        line,
                        old: last.clone(),
                        new,
                    },
                    edits,
                );
            }
        }
        let section = Section {
            header: format!("[{upper}]"),
            name: upper,
            ending: nl,
            lines: vec![Line {
                text: String::new(),
                ending: nl,
                row: None,
            }],
        };
        let index = self.sections.len();
        self.do_edit(Edit::InsertSection { index, section }, edits);
        index
    }

    fn set_row(
        &mut self,
        si: usize,
        li: usize,
        fields: Vec<String>,
        comment: Option<String>,
        edits: &mut Vec<Edit>,
    ) -> Result<()> {
        for f in &fields {
            check_field(f)?;
        }
        if let Some(c) = &comment {
            if !c.starts_with(';') || c.contains('\n') || c.contains('\r') {
                return Err(Error::Format(
                    "a row comment must start with ';' and be one line".into(),
                ));
            }
        }
        let row = Row { fields, comment };
        // Same values: leave the row's original spacing alone.
        if self.sections[si].lines[li].row.as_ref() == Some(&row) {
            return Ok(());
        }
        let layout = Layout::of(&self.sections[si]);
        let text = layout.render(&row.fields, row.comment.as_deref());
        let ending = self.sections[si].lines[li].ending;
        self.replace_line(
            si,
            li,
            Line {
                text,
                ending,
                row: Some(row),
            },
            edits,
        );
        Ok(())
    }

    fn add_row(
        &mut self,
        section: &str,
        fields: Vec<String>,
        comment: Option<String>,
        at: Option<usize>,
        edits: &mut Vec<Edit>,
    ) -> Result<usize> {
        for f in &fields {
            check_field(f)?;
        }
        if fields.is_empty() {
            return Err(Error::Format("a row needs at least one field".into()));
        }
        let si = self.ensure_section(section, edits);
        let li = at.unwrap_or_else(|| self.append_position(si));
        let layout = Layout::of(&self.sections[si]);
        let text = layout.render(&fields, comment.as_deref());
        self.insert_line(si, li, text, Some(Row { fields, comment }), edits);
        Ok(li)
    }

    fn delete_lines_desc(&mut self, si: usize, mut lines: Vec<usize>, edits: &mut Vec<Edit>) {
        lines.sort_unstable();
        for li in lines.into_iter().rev() {
            self.remove_line(si, li, edits);
        }
    }

    fn set_points(
        &mut self,
        section: &str,
        name: &str,
        points: &[(f64, f64)],
        edits: &mut Vec<Edit>,
    ) -> Result<()> {
        let existing = self.find_lines(section, name);
        let spelled = existing
            .first()
            .and_then(|&li| {
                self.section(section)
                    .and_then(|s| s.lines[li].row.as_ref())
                    .and_then(|r| r.fields.first().cloned())
            })
            .unwrap_or_else(|| name.to_string());
        let mut at = existing.first().copied();
        if let Some(si) = self.section_index(section) {
            self.delete_lines_desc(si, existing, edits);
        }
        if points.is_empty() {
            return Ok(());
        }
        for (x, y) in points {
            let li = self.add_row(
                section,
                vec![spelled.clone(), format_number(*x), format_number(*y)],
                None,
                at,
                edits,
            )?;
            at = Some(li + 1);
        }
        Ok(())
    }

    fn exec(&mut self, cmd: &Command, edits: &mut Vec<Edit>) -> Result<()> {
        match cmd {
            Command::SetField {
                section,
                name,
                field,
                value,
            } => {
                let (li, row) = self.find(section, name).ok_or_else(|| {
                    Error::NotFound(format!("[{section}] has no object {name:?}"))
                })?;
                let idx = self.resolve_field(section, row, field)?;
                let mut fields = row.fields.clone();
                let comment = row.comment.clone();
                if idx > fields.len() {
                    return Err(Error::Format(format!(
                        "[{section}] {name}: field {field:?} is column {idx} but the row has {} fields; set the columns before it first",
                        fields.len()
                    )));
                }
                if idx == fields.len() {
                    fields.push(value.clone());
                } else {
                    fields[idx] = value.clone();
                }
                let si = self.section_index(section).expect("section exists");
                self.set_row(si, li, fields, comment, edits)
            }
            Command::SetFields {
                section,
                name,
                fields,
            } => {
                let (li, row) = self.find(section, name).ok_or_else(|| {
                    Error::NotFound(format!("[{section}] has no object {name:?}"))
                })?;
                let comment = row.comment.clone();
                let si = self.section_index(section).expect("section exists");
                self.set_row(si, li, fields.clone(), comment, edits)
            }
            Command::SetLine {
                section,
                line,
                fields,
                comment,
            } => {
                let si = self
                    .section_index(section)
                    .ok_or_else(|| Error::NotFound(format!("no section [{section}]")))?;
                if *line >= self.sections[si].lines.len() {
                    return Err(Error::NotFound(format!("[{section}] has no line {line}")));
                }
                self.set_row(si, *line, fields.clone(), comment.clone(), edits)
            }
            Command::SetText {
                section,
                line,
                text,
            } => {
                let si = self
                    .section_index(section)
                    .ok_or_else(|| Error::NotFound(format!("no section [{section}]")))?;
                if *line >= self.sections[si].lines.len() {
                    return Err(Error::NotFound(format!("[{section}] has no line {line}")));
                }
                if text.contains('\n') || text.contains('\r') {
                    return Err(Error::Format("SetText takes one line".into()));
                }
                let ending = self.sections[si].lines[*line].ending;
                self.replace_line(si, *line, Line::from_text(text, ending), edits);
                Ok(())
            }
            Command::AddRow {
                section,
                fields,
                comment,
            } => self
                .add_row(section, fields.clone(), comment.clone(), None, edits)
                .map(|_| ()),
            Command::InsertText {
                section,
                line,
                text,
            } => {
                if text.contains('\n') || text.contains('\r') {
                    return Err(Error::Format("InsertText takes one line".into()));
                }
                let si = self.ensure_section(section, edits);
                let li = line.unwrap_or_else(|| self.append_position(si));
                if li > self.sections[si].lines.len() {
                    return Err(Error::NotFound(format!("[{section}] has no line {li}")));
                }
                let l = Line::from_text(text, Ending::None);
                self.insert_line(si, li, l.text, l.row, edits);
                Ok(())
            }
            Command::DeleteLine { section, line } => {
                let si = self
                    .section_index(section)
                    .ok_or_else(|| Error::NotFound(format!("no section [{section}]")))?;
                if *line >= self.sections[si].lines.len() {
                    return Err(Error::NotFound(format!("[{section}] has no line {line}")));
                }
                self.remove_line(si, *line, edits);
                Ok(())
            }
            Command::DeleteObject { section, name } => {
                if let Some(si) = self.section_index(section) {
                    let lines = self.find_lines(section, name);
                    self.delete_lines_desc(si, lines, edits);
                }
                Ok(())
            }
            Command::DeleteTag { kind, name } => {
                if let Some(si) = self.section_index("TAGS") {
                    let lines: Vec<usize> = self.sections[si]
                        .rows()
                        .filter(|(_, r)| {
                            r.fields
                                .first()
                                .is_some_and(|k| k.eq_ignore_ascii_case(kind))
                                && r.fields.get(1).is_some_and(|n| same_name(n, name))
                        })
                        .map(|(i, _)| i)
                        .collect();
                    self.delete_lines_desc(si, lines, edits);
                }
                Ok(())
            }
            Command::Rename { kind, old, new } => self.rename(*kind, old, new, edits),
            Command::MoveNode { name, x, y } => self.set_xy("COORDINATES", name, *x, *y, edits),
            Command::MoveGage { name, x, y } => self.set_xy("SYMBOLS", name, *x, *y, edits),
            Command::SetVertices { link, points } => {
                self.set_points("VERTICES", link, points, edits)
            }
            Command::SetPolygon {
                subcatchment,
                points,
            } => self.set_points("POLYGONS", subcatchment, points, edits),
            Command::SetOption {
                section,
                key,
                value,
            } => {
                check_field(key)?;
                let (values, comment) = tokenize(value);
                if comment.is_some() {
                    return Err(Error::Format("an option value may not contain ';'".into()));
                }
                match self.find(section, key) {
                    Some((li, row)) => {
                        let mut fields = vec![row.fields[0].clone()];
                        fields.extend(values);
                        let comment = row.comment.clone();
                        let si = self.section_index(section).expect("section exists");
                        self.set_row(si, li, fields, comment, edits)
                    }
                    None => {
                        let mut fields = vec![key.clone()];
                        fields.extend(values);
                        self.add_row(section, fields, None, None, edits).map(|_| ())
                    }
                }
            }
            Command::SetTitle { text } => {
                let si = self.ensure_section("TITLE", edits);
                let old: Vec<usize> = self.sections[si].rows().map(|(i, _)| i).collect();
                let start = old
                    .first()
                    .copied()
                    .unwrap_or_else(|| self.append_position(si));
                self.delete_lines_desc(si, old, edits);
                for (at, line) in (start..).zip(text.split('\n')) {
                    let line = line.trim_end_matches('\r');
                    let l = Line::from_text(line, Ending::None);
                    self.insert_line(si, at, l.text, l.row, edits);
                }
                Ok(())
            }
            Command::Batch(cmds) => {
                for c in cmds {
                    self.exec(c, edits)?;
                }
                Ok(())
            }
        }
    }

    fn set_xy(
        &mut self,
        section: &str,
        name: &str,
        x: f64,
        y: f64,
        edits: &mut Vec<Edit>,
    ) -> Result<()> {
        let fx = format_number(x);
        let fy = format_number(y);
        match self.find(section, name) {
            Some((li, row)) => {
                let spelled = row.fields[0].clone();
                let comment = row.comment.clone();
                let si = self.section_index(section).expect("section exists");
                self.set_row(si, li, vec![spelled, fx, fy], comment, edits)
            }
            None => self
                .add_row(section, vec![name.to_string(), fx, fy], None, None, edits)
                .map(|_| ()),
        }
    }

    fn rename(
        &mut self,
        kind: ObjectKind,
        old: &str,
        new: &str,
        edits: &mut Vec<Edit>,
    ) -> Result<()> {
        let new_name = unquote(new);
        if new_name.trim().is_empty() {
            return Err(Error::Format("a name may not be empty".into()));
        }
        check_field(new_name)?;
        if new_name.contains('[') {
            return Err(Error::Format("a name may not contain '['".into()));
        }
        if !same_name(old, new_name) {
            if let Some(sec) = self.defining_section(kind, new_name) {
                return Err(Error::Format(format!(
                    "[{sec}] already has an object named {new_name:?}"
                )));
            }
        }
        let infil = self.infiltration_method();
        let tag_kw = kind.tag_keyword();
        let control_kws = kind.control_keywords();
        for si in 0..self.sections.len() {
            let sec_name = self.sections[si].name.clone();
            let defines = kind.defining_sections().contains(&sec_name.as_str());
            let ref_cols: Vec<&str> = schema::REFERENCES
                .iter()
                .filter(|(s, _, k)| *s == sec_name && *k == kind)
                .map(|(_, c, _)| *c)
                .collect();
            let is_tags = sec_name == "TAGS" && tag_kw.is_some();
            let is_controls = sec_name == "CONTROLS" && !control_kws.is_empty();
            if !defines && ref_cols.is_empty() && !is_tags && !is_controls {
                continue;
            }
            for li in 0..self.sections[si].lines.len() {
                let Some(row) = self.sections[si].lines[li].row.as_ref() else {
                    continue;
                };
                let mut idxs: Vec<usize> = Vec::new();
                if defines {
                    if let Some(ni) = schema::name_index(&sec_name) {
                        idxs.push(ni);
                    }
                }
                if !ref_cols.is_empty() {
                    let cols = schema::columns(&sec_name, &row.fields, infil.as_deref());
                    for c in &ref_cols {
                        if let Some(i) = schema::field_index(cols, c) {
                            idxs.push(i);
                        }
                    }
                }
                if is_tags
                    && row
                        .fields
                        .first()
                        .is_some_and(|k| tag_kw.is_some_and(|t| k.eq_ignore_ascii_case(t)))
                {
                    idxs.push(1);
                }
                if is_controls {
                    for (i, f) in row.fields.iter().enumerate() {
                        if control_kws.iter().any(|k| f.eq_ignore_ascii_case(k)) {
                            idxs.push(i + 1);
                        }
                    }
                }
                let mut fields = row.fields.clone();
                let mut changed = false;
                for i in idxs {
                    if let Some(f) = fields.get_mut(i) {
                        if same_name(f, old) {
                            *f = if f.starts_with('"') {
                                format!("\"{new_name}\"")
                            } else {
                                new_name.to_string()
                            };
                            changed = true;
                        }
                    }
                }
                if changed {
                    let comment = row.comment.clone();
                    self.set_row(si, li, fields, comment, edits)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_mirrors_engine() {
        let (f, c) = tokenize("J1  \t4973 \"Arial Bold\" \"\" x;y ; note");
        assert_eq!(f, vec!["J1", "4973", "\"Arial Bold\"", "\"\"", "x"]);
        assert_eq!(c.as_deref(), Some(";y ; note"));
        let (f, _) = tokenize("\"unterminated label");
        assert_eq!(f, vec!["\"unterminated label"]);
    }

    #[test]
    fn numbers_format_plainly() {
        assert_eq!(format_number(1651.426), "1651.426");
        assert_eq!(format_number(1651.0), "1651");
        assert_eq!(format_number(-0.0001), "0");
        assert_eq!(format_number(0.5), "0.5");
    }

    #[test]
    fn layout_from_ruler() {
        let doc = InpDoc::parse(
            "[JUNCTIONS]\n;;Name           Elev\n;;-------------- ----------\nJ1               1\n",
        );
        let l = Layout::of(&doc.sections[0]);
        assert_eq!(l.widths, vec![16, 10]);
        assert_eq!(l.sep, " ");
        assert_eq!(
            l.render(&["J1".into(), "2".into()], None),
            "J1               2"
        );
    }
}
