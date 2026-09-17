// SPDX-License-Identifier: GPL-3.0-or-later

//! The Project menu's dialogs: Title/Notes, Options (grouped as the EPA
//! GUI groups them), Rain Gages, Curves, Time Series, Patterns, Controls,
//! Pollutants and Land Uses. Every OK or Apply is one command and one undo
//! step; Cancel or closing the window discards the draft.
//!
//! Curves, series and patterns are edited as a draft (a table and a plot
//! drawn from it) and written back as a whole, so a curve of twelve points
//! is one undo step and its rows keep their place in the file.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, Id, Pos2, Rect, RichText, Stroke, Ui, Vec2};
use stormsewer_swmm::doc::build::{self, ObjRef};
use stormsewer_swmm::doc::{Command, InpDoc, ObjectKind, SeriesPoint};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;
use crate::swmm_props::{self, field_widget, spec, text_field, CURVE_TYPES, PATTERN_TYPES};
use crate::theme::palette;

// --- row rewriting ------------------------------------------------------------

fn render_line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| {
            if f.is_empty() || f.chars().any(char::is_whitespace) {
                format!("\"{f}\"")
            } else {
                f.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// The batch that replaces every row named `name` in `section` with `rows`
/// (each with the name as its first field). Existing rows keep their line
/// and comment; extra rows go in after the last; surplus rows go.
pub fn replace_rows(doc: &InpDoc, section: &str, name: &str, rows: &[Vec<String>]) -> Command {
    let sec = section.to_ascii_uppercase();
    let existing: Vec<(usize, Option<String>)> = doc
        .rows(&sec)
        .into_iter()
        .filter(|(_, r)| r.value(0).is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .map(|(li, r)| (li, r.comment.clone()))
        .collect();
    let mut cmds = Vec::new();
    let n = existing.len().min(rows.len());
    for (i, (li, comment)) in existing.iter().enumerate().take(n) {
        cmds.push(Command::SetLine {
            section: sec.clone(),
            line: *li,
            fields: rows[i].clone(),
            comment: comment.clone(),
        });
    }
    if rows.len() > existing.len() {
        match existing.last() {
            Some((last, _)) => {
                for (k, row) in rows[existing.len()..].iter().enumerate() {
                    cmds.push(Command::InsertText {
                        section: sec.clone(),
                        line: Some(last + 1 + k),
                        text: render_line(row),
                    });
                }
            }
            None => {
                for row in &rows[existing.len()..] {
                    cmds.push(Command::AddRow {
                        section: sec.clone(),
                        fields: row.clone(),
                        comment: None,
                    });
                }
            }
        }
    }
    for (li, _) in existing.iter().skip(rows.len()).rev() {
        cmds.push(Command::DeleteLine {
            section: sec.clone(),
            line: *li,
        });
    }
    Command::Batch(cmds)
}

/// The rows of a curve: the type on the first row only, as the EPA GUI
/// writes it.
pub fn curve_rows(name: &str, kind: &str, points: &[(String, String)]) -> Vec<Vec<String>> {
    points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            let mut r = vec![name.to_string()];
            if i == 0 && !kind.trim().is_empty() {
                r.push(kind.trim().to_string());
            }
            r.push(x.trim().to_string());
            r.push(y.trim().to_string());
            r
        })
        .collect()
}

/// The rows of a time series: dated when any point has a date; or the
/// single `FILE` row.
pub fn series_rows(name: &str, points: &[SeriesPoint], file: Option<&str>) -> Vec<Vec<String>> {
    if let Some(f) = file {
        return vec![vec![name.to_string(), "FILE".into(), f.to_string()]];
    }
    points
        .iter()
        .map(|p| {
            let mut r = vec![name.to_string()];
            if let Some(d) = &p.date {
                r.push(d.clone());
            }
            r.push(p.time.trim().to_string());
            r.push(p.value.trim().to_string());
            r
        })
        .collect()
}

/// The rows of a pattern: the type on the first row, six multipliers per
/// row.
pub fn pattern_rows(name: &str, kind: &str, values: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for (i, chunk) in values.chunks(6).enumerate() {
        let mut r = vec![name.to_string()];
        if i == 0 {
            r.push(kind.to_string());
        }
        r.extend(chunk.iter().map(|v| v.trim().to_string()));
        out.push(r);
    }
    if out.is_empty() {
        out.push(vec![name.to_string(), kind.to_string()]);
    }
    out
}

/// Parse pasted rows of `date time value` or `time value` (tabs, commas or
/// spaces between). Lines that do not parse are skipped.
pub fn parse_series_text(text: &str) -> Vec<SeriesPoint> {
    let mut out = Vec::new();
    for line in text.lines() {
        let toks: Vec<&str> = line
            .split(|c: char| c == '\t' || c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .collect();
        match toks.as_slice() {
            [date, time, value] if value.parse::<f64>().is_ok() => out.push(SeriesPoint {
                date: Some(date.to_string()),
                time: time.to_string(),
                value: value.to_string(),
            }),
            [time, value] if value.parse::<f64>().is_ok() => out.push(SeriesPoint {
                date: None,
                time: time.to_string(),
                value: value.to_string(),
            }),
            _ => {}
        }
    }
    out
}

/// How many multipliers a pattern type has.
pub fn pattern_len(kind: &str) -> usize {
    match kind.to_ascii_uppercase().as_str() {
        "MONTHLY" => 12,
        "DAILY" => 7,
        _ => 24,
    }
}

/// The multipliers of a pattern, in order, and its type.
pub fn pattern_of(doc: &InpDoc, name: &str) -> (String, Vec<String>) {
    let mut kind = String::new();
    let mut values = Vec::new();
    for r in doc.find_all("PATTERNS", name) {
        let cols = doc.columns("PATTERNS", r);
        let start = if cols.len() == 3 {
            kind = r.value(1).unwrap_or("").to_string();
            2
        } else {
            1
        };
        values.extend(r.fields.iter().skip(start).cloned());
    }
    (kind, values)
}

/// A free-text section's lines (comments included) joined with `\n`.
pub fn section_text(doc: &InpDoc, section: &str) -> String {
    doc.section(section)
        .map(|s| {
            s.lines
                .iter()
                .map(|l| l.text.clone())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// The batch that makes a free-text section read `text`, line for line.
pub fn set_section_text(doc: &InpDoc, section: &str, text: &str) -> Command {
    let sec = section.to_ascii_uppercase();
    let old: Vec<String> = doc
        .section(&sec)
        .map(|s| s.lines.iter().map(|l| l.text.clone()).collect())
        .unwrap_or_default();
    let new: Vec<&str> = if text.is_empty() {
        Vec::new()
    } else {
        text.lines().collect()
    };
    let mut cmds = Vec::new();
    let n = old.len().min(new.len());
    for (i, line) in new.iter().enumerate().take(n) {
        if old[i] != *line {
            cmds.push(Command::SetText {
                section: sec.clone(),
                line: i,
                text: line.to_string(),
            });
        }
    }
    for (k, line) in new.iter().enumerate().skip(n) {
        cmds.push(Command::InsertText {
            section: sec.clone(),
            line: Some(k),
            text: line.to_string(),
        });
    }
    for i in (new.len()..old.len()).rev() {
        cmds.push(Command::DeleteLine {
            section: sec.clone(),
            line: i,
        });
    }
    Command::Batch(cmds)
}

// --- options -----------------------------------------------------------------------

#[derive(Clone, Copy)]
enum OptKind {
    Enum(&'static [&'static str]),
    YesNo,
    Text(&'static str),
}

const FLOW_UNITS: &[&str] = &["CFS", "GPM", "MGD", "CMS", "LPS", "MLD"];
const ROUTING: &[&str] = &["STEADY", "KINWAVE", "DYNWAVE"];

const GENERAL: &[(&str, OptKind)] = &[
    ("FLOW_UNITS", OptKind::Enum(FLOW_UNITS)),
    (
        "INFILTRATION",
        OptKind::Enum(stormsewer_swmm::doc::schema::INFILTRATION_METHODS),
    ),
    ("FLOW_ROUTING", OptKind::Enum(ROUTING)),
    ("LINK_OFFSETS", OptKind::Enum(&["DEPTH", "ELEVATION"])),
    ("FORCE_MAIN_EQUATION", OptKind::Enum(&["H-W", "D-W"])),
    ("MIN_SLOPE", OptKind::Text("%")),
    ("ALLOW_PONDING", OptKind::YesNo),
    ("SKIP_STEADY_STATE", OptKind::YesNo),
    ("IGNORE_RAINFALL", OptKind::YesNo),
    ("IGNORE_SNOWMELT", OptKind::YesNo),
    ("IGNORE_GROUNDWATER", OptKind::YesNo),
    ("IGNORE_RDII", OptKind::YesNo),
    ("IGNORE_ROUTING", OptKind::YesNo),
    ("IGNORE_QUALITY", OptKind::YesNo),
];
const DATES: &[(&str, OptKind)] = &[
    ("START_DATE", OptKind::Text("mm/dd/yyyy")),
    ("START_TIME", OptKind::Text("hh:mm:ss")),
    ("REPORT_START_DATE", OptKind::Text("mm/dd/yyyy")),
    ("REPORT_START_TIME", OptKind::Text("hh:mm:ss")),
    ("END_DATE", OptKind::Text("mm/dd/yyyy")),
    ("END_TIME", OptKind::Text("hh:mm:ss")),
    ("SWEEP_START", OptKind::Text("mm/dd")),
    ("SWEEP_END", OptKind::Text("mm/dd")),
    ("DRY_DAYS", OptKind::Text("days")),
];
const STEPS: &[(&str, OptKind)] = &[
    ("REPORT_STEP", OptKind::Text("hh:mm:ss")),
    ("WET_STEP", OptKind::Text("hh:mm:ss")),
    ("DRY_STEP", OptKind::Text("hh:mm:ss")),
    ("ROUTING_STEP", OptKind::Text("hh:mm:ss or s")),
    ("RULE_STEP", OptKind::Text("hh:mm:ss")),
    ("LENGTHENING_STEP", OptKind::Text("s")),
    ("VARIABLE_STEP", OptKind::Text("0–2, Courant factor")),
    ("MINIMUM_STEP", OptKind::Text("s")),
];
const DYNWAVE: &[(&str, OptKind)] = &[
    (
        "INERTIAL_DAMPING",
        OptKind::Enum(&["NONE", "PARTIAL", "FULL"]),
    ),
    (
        "NORMAL_FLOW_LIMITED",
        OptKind::Enum(&["SLOPE", "FROUDE", "BOTH"]),
    ),
    ("SURCHARGE_METHOD", OptKind::Enum(&["EXTRAN", "SLOT"])),
    ("MIN_SURFAREA", OptKind::Text("ft² or m²")),
    ("MAX_TRIALS", OptKind::Text("")),
    ("HEAD_TOLERANCE", OptKind::Text("ft or m")),
    ("SYS_FLOW_TOL", OptKind::Text("%")),
    ("LAT_FLOW_TOL", OptKind::Text("%")),
    ("THREADS", OptKind::Text("")),
];

/// The known option keys, every group.
pub fn option_keys() -> Vec<&'static str> {
    GENERAL
        .iter()
        .chain(DATES)
        .chain(STEPS)
        .chain(DYNWAVE)
        .map(|(k, _)| *k)
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OptionsDraft {
    pub values: BTreeMap<String, String>,
    pub files: String,
    pub tab: usize,
}

/// The current option values (known keys and any others the file has).
pub fn options_draft(doc: &InpDoc) -> OptionsDraft {
    let mut values = BTreeMap::new();
    for k in option_keys() {
        values.insert(
            k.to_string(),
            doc.key_value("OPTIONS", k).unwrap_or_default(),
        );
    }
    for (_, r) in doc.rows("OPTIONS") {
        if let Some(k) = r.value(0) {
            let k = k.to_ascii_uppercase();
            values.entry(k).or_insert_with(|| r.fields[1..].join(" "));
        }
    }
    OptionsDraft {
        values,
        files: section_text(doc, "FILES"),
        tab: 0,
    }
}

/// The batch that applies a draft: one `SetOption` per changed key (a
/// cleared key is removed), and the `[FILES]` text.
pub fn options_command(doc: &InpDoc, draft: &OptionsDraft) -> Command {
    let mut cmds = Vec::new();
    for (k, v) in &draft.values {
        let v = v.trim();
        let cur = doc.key_value("OPTIONS", k);
        match (cur, v.is_empty()) {
            (Some(c), false) if c.eq_ignore_ascii_case(v) => {}
            (None, true) => {}
            (Some(_), true) => cmds.push(Command::DeleteObject {
                section: "OPTIONS".into(),
                name: k.clone(),
            }),
            _ => cmds.push(Command::SetOption {
                section: "OPTIONS".into(),
                key: k.clone(),
                value: v.to_string(),
            }),
        }
    }
    if draft.files != section_text(doc, "FILES") {
        cmds.push(set_section_text(doc, "FILES", &draft.files));
    }
    Command::Batch(cmds)
}

// --- dialog state -------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurvesDraft {
    pub name: Option<String>,
    pub kind: String,
    pub points: Vec<(String, String)>,
    /// The generation the draft was read at; a doc change elsewhere reloads.
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeriesDraft {
    pub name: Option<String>,
    pub points: Vec<SeriesPoint>,
    pub file: Option<String>,
    pub paste: String,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PatternsDraft {
    pub name: Option<String>,
    pub kind: String,
    pub values: Vec<String>,
    pub loaded_gen: Option<u64>,
    pub dirty: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Dialogs {
    pub title: Option<String>,
    pub options: Option<OptionsDraft>,
    pub gages: bool,
    pub gage: Option<String>,
    pub curves: Option<CurvesDraft>,
    pub series: Option<SeriesDraft>,
    pub patterns: Option<PatternsDraft>,
    pub controls: Option<String>,
    pub pollutants: bool,
    pub landuses: bool,
    /// Text draft for the immediate-commit editors (gages, tables).
    pub draft: Option<(Id, String)>,
    pub message: String,
}

pub fn open_title(ed: &mut SwmmEditor) {
    ed.dialogs.title = Some(ed.doc.title());
}

pub fn open_options(ed: &mut SwmmEditor) {
    ed.dialogs.options = Some(options_draft(&ed.doc));
}

pub fn open_gages(ed: &mut SwmmEditor, item: Option<&str>) {
    ed.dialogs.gages = true;
    ed.dialogs.gage = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("RAINGAGES").first().cloned());
}

pub fn open_curves(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("CURVES").first().cloned());
    let mut d = CurvesDraft::default();
    load_curve(&ed.doc, &mut d, name);
    ed.dialogs.curves = Some(d);
}

pub fn open_series(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("TIMESERIES").first().cloned());
    let mut d = SeriesDraft::default();
    load_series(&ed.doc, &mut d, name);
    ed.dialogs.series = Some(d);
}

pub fn open_patterns(ed: &mut SwmmEditor, item: Option<&str>) {
    let name = item
        .map(str::to_string)
        .or_else(|| ed.doc.names("PATTERNS").first().cloned());
    let mut d = PatternsDraft::default();
    load_pattern(&ed.doc, &mut d, name);
    ed.dialogs.patterns = Some(d);
}

pub fn open_controls(ed: &mut SwmmEditor) {
    ed.dialogs.controls = Some(section_text(&ed.doc, "CONTROLS"));
}

pub fn open_pollutants(ed: &mut SwmmEditor) {
    ed.dialogs.pollutants = true;
}

pub fn open_landuses(ed: &mut SwmmEditor) {
    ed.dialogs.landuses = true;
}

pub fn load_curve(doc: &InpDoc, d: &mut CurvesDraft, name: Option<String>) {
    d.name = name;
    d.kind.clear();
    d.points.clear();
    if let Some(n) = &d.name {
        let (kind, pts) = doc.curve(n);
        d.kind = kind.unwrap_or_default();
        d.points = pts;
    }
    d.loaded_gen = Some(doc.generation());
    d.dirty = false;
}

pub fn load_series(doc: &InpDoc, d: &mut SeriesDraft, name: Option<String>) {
    d.name = name;
    d.points.clear();
    d.file = None;
    if let Some(n) = &d.name {
        d.points = doc.timeseries(n);
        if let Some(r) = doc
            .find_all("TIMESERIES", n)
            .into_iter()
            .find(|r| r.value(1).is_some_and(|f| f.eq_ignore_ascii_case("FILE")))
        {
            d.file = Some(r.value(2).unwrap_or("").to_string());
        }
    }
    d.loaded_gen = Some(doc.generation());
    d.dirty = false;
}

pub fn load_pattern(doc: &InpDoc, d: &mut PatternsDraft, name: Option<String>) {
    d.name = name;
    d.kind = "HOURLY".into();
    d.values.clear();
    if let Some(n) = &d.name {
        let (kind, values) = pattern_of(doc, n);
        if !kind.is_empty() {
            d.kind = kind;
        }
        d.values = values;
    }
    d.loaded_gen = Some(doc.generation());
    d.dirty = false;
}

/// Write the curves draft back as one step.
pub fn apply_curve(ed: &mut SwmmEditor, d: &mut CurvesDraft) -> bool {
    let Some(name) = d.name.clone() else {
        return false;
    };
    let rows = curve_rows(&name, &d.kind, &d.points);
    let cmd = replace_rows(&ed.doc, "CURVES", &name, &rows);
    let ok = ed.apply(cmd, &format!("edit curve {name}"));
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

pub fn apply_series(ed: &mut SwmmEditor, d: &mut SeriesDraft) -> bool {
    let Some(name) = d.name.clone() else {
        return false;
    };
    let rows = series_rows(&name, &d.points, d.file.as_deref());
    let cmd = replace_rows(&ed.doc, "TIMESERIES", &name, &rows);
    let ok = ed.apply(cmd, &format!("edit time series {name}"));
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

pub fn apply_pattern(ed: &mut SwmmEditor, d: &mut PatternsDraft) -> bool {
    let Some(name) = d.name.clone() else {
        return false;
    };
    let rows = pattern_rows(&name, &d.kind, &d.values);
    let cmd = replace_rows(&ed.doc, "PATTERNS", &name, &rows);
    let ok = ed.apply(cmd, &format!("edit pattern {name}"));
    if ok {
        d.loaded_gen = Some(ed.doc.generation());
        d.dirty = false;
    }
    ok
}

/// Apply the Title dialog: one step.
pub fn apply_title(ed: &mut SwmmEditor, text: &str) -> bool {
    ed.apply(
        Command::SetTitle {
            text: text.trim_end().to_string(),
        },
        "edit title",
    )
}

/// Apply the Options dialog: one step.
pub fn apply_options(ed: &mut SwmmEditor, draft: &OptionsDraft) -> bool {
    let cmd = options_command(&ed.doc, draft);
    ed.apply(cmd, "edit options")
}

/// Apply the Controls dialog: one step.
pub fn apply_controls(ed: &mut SwmmEditor, text: &str) -> bool {
    let cmd = set_section_text(&ed.doc, "CONTROLS", text);
    ed.apply(cmd, "edit controls")
}

// --- plots ----------------------------------------------------------------------------

/// A small line (or bar) plot of `pts`, painted in place.
pub fn plot(ui: &mut Ui, id: Id, pts: &[(f64, f64)], height: f32, bars: bool) {
    let dark = ui.visuals().dark_mode;
    let width = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, palette::canvas::bg(dark));
    let inner = rect.shrink2(Vec2::new(36.0, 14.0));
    let ink = palette::canvas::ink(dark);
    let muted = palette::canvas::muted(dark);
    painter.line_segment(
        [inner.left_bottom(), inner.right_bottom()],
        Stroke::new(1.0_f32, muted),
    );
    painter.line_segment(
        [inner.left_top(), inner.left_bottom()],
        Stroke::new(1.0_f32, muted),
    );
    let finite: Vec<(f64, f64)> = pts
        .iter()
        .copied()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect();
    if finite.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no points",
            egui::FontId::proportional(12.0),
            muted,
        );
        return;
    }
    let (mut x0, mut x1) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut y0, mut y1) = (0.0_f64, f64::NEG_INFINITY);
    for (x, y) in &finite {
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        y0 = y0.min(*y);
        y1 = y1.max(*y);
    }
    if bars {
        x0 = -0.5;
        x1 = finite.len() as f64 - 0.5;
    }
    if (x1 - x0).abs() < 1e-12 {
        x1 = x0 + 1.0;
    }
    if (y1 - y0).abs() < 1e-12 {
        y1 = y0 + 1.0;
    }
    let to = |x: f64, y: f64| -> Pos2 {
        Pos2::new(
            inner.left() + ((x - x0) / (x1 - x0)) as f32 * inner.width(),
            inner.bottom() - ((y - y0) / (y1 - y0)) as f32 * inner.height(),
        )
    };
    let font = egui::FontId::proportional(10.0);
    painter.text(
        inner.left_bottom() + Vec2::new(-4.0, 2.0),
        egui::Align2::RIGHT_TOP,
        stormsewer_swmm::doc::format_number(y0),
        font.clone(),
        ink,
    );
    painter.text(
        inner.left_top() + Vec2::new(-4.0, 0.0),
        egui::Align2::RIGHT_TOP,
        stormsewer_swmm::doc::format_number(y1),
        font.clone(),
        ink,
    );
    if !bars {
        painter.text(
            inner.left_bottom() + Vec2::new(0.0, 2.0),
            egui::Align2::LEFT_TOP,
            stormsewer_swmm::doc::format_number(x0),
            font.clone(),
            ink,
        );
        painter.text(
            inner.right_bottom() + Vec2::new(0.0, 2.0),
            egui::Align2::RIGHT_TOP,
            stormsewer_swmm::doc::format_number(x1),
            font,
            ink,
        );
    }
    let _ = id;
    if bars {
        let w = (inner.width() / finite.len() as f32 * 0.8).max(1.0);
        for (i, (_, y)) in finite.iter().enumerate() {
            let top = to(i as f64, *y);
            let base = to(i as f64, y0.max(0.0).min(y1));
            let r = Rect::from_min_max(
                Pos2::new(top.x - w / 2.0, top.y.min(base.y)),
                Pos2::new(top.x + w / 2.0, top.y.max(base.y)),
            );
            painter.rect_filled(r, 1.0, palette::FLOW_OK);
        }
    } else {
        let screen: Vec<Pos2> = finite.iter().map(|(x, y)| to(*x, *y)).collect();
        if screen.len() >= 2 {
            painter.add(egui::Shape::line(
                screen.clone(),
                Stroke::new(2.0_f32, palette::FLOW_OK),
            ));
        }
        for p in &screen {
            painter.circle_filled(*p, 2.5, palette::ACCENT);
        }
    }
}

// --- windows ------------------------------------------------------------------------------

fn window<'a>(ctx: &egui::Context, title: &'a str, size: Vec2) -> egui::Window<'a> {
    egui::Window::new(title)
        .id(Id::new(("swmm-dialog", title)))
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .default_pos(ctx.screen_rect().center() - size / 2.0)
}

/// A list of names with add and delete, returning the pick.
fn name_list(
    ui: &mut Ui,
    id: &str,
    names: &[String],
    current: Option<&str>,
    height: f32,
) -> Option<String> {
    let mut pick = None;
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .show(ui, |ui| {
            for n in names {
                if ui
                    .selectable_label(current.is_some_and(|c| c.eq_ignore_ascii_case(n)), n)
                    .clicked()
                {
                    pick = Some(n.clone());
                }
            }
            if names.is_empty() {
                ui.label(RichText::new("(none)").weak());
            }
        });
    pick
}

fn draw_title(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut text) = state.swmm_doc.dialogs.title.clone() else {
        return;
    };
    let mut open = true;
    let mut done = false;
    window(ctx, "Title / Notes", Vec2::new(460.0, 300.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("The [TITLE] section: a project title and notes, one line each.");
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .id(Id::new("swmm-title-text"))
                    .desired_rows(10)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    if apply_title(&mut state.swmm_doc, &text) {
                        state.status = "Title updated".into();
                    }
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
        });
    if done || !open {
        state.swmm_doc.dialogs.title = None;
    } else {
        state.swmm_doc.dialogs.title = Some(text);
    }
}

fn option_widget(ui: &mut Ui, key: &str, kind: OptKind, values: &mut BTreeMap<String, String>) {
    let v = values.entry(key.to_string()).or_default();
    match kind {
        OptKind::Enum(opts) => {
            egui::ComboBox::from_id_salt(("swmm-opt", key))
                .selected_text(if v.is_empty() { "(unset)" } else { v.as_str() })
                .width(150.0)
                .show_ui(ui, |ui| {
                    for o in opts {
                        ui.selectable_value(v, o.to_string(), *o);
                    }
                });
        }
        OptKind::YesNo => {
            let mut on = v.eq_ignore_ascii_case("YES");
            if ui.checkbox(&mut on, "").changed() {
                *v = if on { "YES" } else { "NO" }.to_string();
            }
        }
        OptKind::Text(unit) => {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(v)
                        .id(Id::new(("swmm-opt", key)))
                        .desired_width(120.0),
                );
                if !unit.is_empty() {
                    ui.label(RichText::new(unit).weak().small());
                }
            });
        }
    }
}

fn option_group(ui: &mut Ui, group: &[(&str, OptKind)], values: &mut BTreeMap<String, String>) {
    egui::Grid::new(("swmm-opt-grid", group.len(), group[0].0))
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (key, kind) in group {
                ui.label(*key);
                option_widget(ui, key, *kind, values);
                ui.end_row();
            }
        });
}

fn draw_options(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut draft) = state.swmm_doc.dialogs.options.clone() else {
        return;
    };
    let mut open = true;
    let mut done = false;
    let units = swmm_props::units(&state.swmm_doc.doc);
    window(ctx, "Simulation Options", Vec2::new(520.0, 460.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in ["General", "Dates", "Time Steps", "Dynamic Wave", "Files"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut draft.tab, i, *name);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("swmm-options-scroll")
                .max_height(330.0)
                .show(ui, |ui| match draft.tab {
                    0 => option_group(ui, GENERAL, &mut draft.values),
                    1 => option_group(ui, DATES, &mut draft.values),
                    2 => option_group(ui, STEPS, &mut draft.values),
                    3 => {
                        ui.label(
                            RichText::new(format!(
                                "Lengths in {}; areas in {}.",
                                if units.metric { "m" } else { "ft" },
                                if units.metric { "m²" } else { "ft²" }
                            ))
                            .small(),
                        );
                        option_group(ui, DYNWAVE, &mut draft.values);
                    }
                    _ => {
                        ui.label("[FILES] — USE or SAVE RAINFALL / RUNOFF / HOTSTART / RDII / INFLOWS / OUTFLOWS \"file\"");
                        ui.add(
                            egui::TextEdit::multiline(&mut draft.files)
                                .id(Id::new("swmm-options-files"))
                                .desired_rows(8)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    if apply_options(&mut state.swmm_doc, &draft) {
                        state.status = "Options updated".into();
                    }
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
        });
    if done || !open {
        state.swmm_doc.dialogs.options = None;
    } else {
        state.swmm_doc.dialogs.options = Some(draft);
    }
}

fn draw_gages(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.dialogs.gages {
        return;
    }
    let mut open = true;
    window(ctx, "Rain Gages", Vec2::new(520.0, 360.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            let names = ed.doc.names("RAINGAGES");
            let current = ed.dialogs.gage.clone();
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("Add a rain gage").clicked() {
                        let ((cx, cy), span) = match ed.bounds {
                            Some((x0, y0, x1, y1)) => (
                                ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                                (x1 - x0).max(10.0) * 0.05,
                            ),
                            None => ((0.0, 0.0), 100.0),
                        };
                        let o = build::new_gage(&ed.doc, cx - span, cy + span);
                        let n = o.name.clone();
                        if ed.apply(o.command, &format!("add rain gage {n}")) {
                            ed.dialogs.gage = Some(n);
                        }
                    }
                    if ui
                        .add_enabled(current.is_some(), egui::Button::new("−"))
                        .on_hover_text("Delete the chosen gage")
                        .clicked()
                    {
                        if let Some(n) = current.clone() {
                            if ed.apply(
                                build::cascade_delete_gage(&n),
                                &format!("delete rain gage {n}"),
                            ) {
                                ed.dialogs.gage = None;
                            }
                        }
                    }
                });
                if let Some(pick) =
                    name_list(ui, "swmm-gage-list", &names, current.as_deref(), 260.0)
                {
                    ed.dialogs.gage = Some(pick.clone());
                    ed.select_only(ObjRef::Gage(pick));
                }
                let ui = &mut cols[1];
                let Some(name) = ed.dialogs.gage.clone() else {
                    ui.label("Choose a gage, or + to add one.");
                    return;
                };
                let Some((_, row)) = ed
                    .doc
                    .find("RAINGAGES", &name)
                    .map(|(li, r)| (li, r.clone()))
                else {
                    ui.label("The gage is gone.");
                    return;
                };
                let cols = ed.doc.columns("RAINGAGES", &row);
                let mut pending: Option<(String, String)> = None;
                egui::Grid::new("swmm-gage-grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for (i, col) in cols.iter().enumerate() {
                            let fs = spec(&ed.doc, "RAINGAGES", col);
                            ui.label(match fs.unit {
                                Some(u) => format!("{col} ({u})"),
                                None => col.to_string(),
                            });
                            let value = row.value(i).unwrap_or("");
                            let id = Id::new(("swmm-gage-field", *col));
                            if let Some(v) =
                                field_widget(ui, id, &fs, value, &mut ed.dialogs.draft, 160.0)
                            {
                                pending = Some((col.to_string(), v));
                            }
                            ui.end_row();
                        }
                    });
                if let Some((col, v)) = pending {
                    if swmm_props::commit(ed, "RAINGAGES", &name, &col, &v) {
                        if col == "Name" {
                            ed.dialogs.gage = Some(v);
                        }
                    } else {
                        ed.dialogs.message = ed.last_error.take().unwrap_or_default();
                    }
                }
                if !ed.dialogs.message.is_empty() {
                    ui.label(RichText::new(&ed.dialogs.message).small());
                }
                ui.label(
                    RichText::new(
                        "Each change is one undo step. Time series: Project → Time Series…",
                    )
                    .small(),
                );
            });
        });
    if !open {
        state.swmm_doc.dialogs.gages = false;
    }
}

fn draw_curves(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.curves.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    window(ctx, "Curves", Vec2::new(620.0, 440.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            if !d.dirty && d.loaded_gen != Some(ed.doc.generation()) {
                let n = d.name.clone();
                load_curve(&ed.doc, &mut d, n);
            }
            let names = ed.doc.names("CURVES");
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("Add a curve").clicked() {
                        let n = build::unique_name(&ed.doc, ObjectKind::Curve, "Curve");
                        let cmd = Command::AddRow {
                            section: "CURVES".into(),
                            fields: vec![n.clone(), "STORAGE".into(), "0".into(), "0".into()],
                            comment: None,
                        };
                        if ed.apply(cmd, &format!("add curve {n}")) {
                            load_curve(&ed.doc, &mut d, Some(n));
                        }
                    }
                    if ui
                        .add_enabled(d.name.is_some(), egui::Button::new("−"))
                        .on_hover_text("Delete the chosen curve")
                        .clicked()
                    {
                        if let Some(n) = d.name.clone() {
                            if ed.apply(
                                Command::DeleteObject {
                                    section: "CURVES".into(),
                                    name: n.clone(),
                                },
                                &format!("delete curve {n}"),
                            ) {
                                load_curve(&ed.doc, &mut d, None);
                            }
                        }
                    }
                });
                if let Some(pick) =
                    name_list(ui, "swmm-curve-list", &names, d.name.as_deref(), 320.0)
                {
                    if d.dirty {
                        apply_curve(ed, &mut d);
                    }
                    load_curve(&ed.doc, &mut d, Some(pick));
                }
                let ui = &mut cols[1];
                let Some(name) = d.name.clone() else {
                    ui.label("Choose a curve, or + to add one.");
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let id = Id::new("swmm-curve-name");
                    if let Some(v) = text_field(ui, id, &name, &mut ed.dialogs.draft, 120.0) {
                        if swmm_props::commit(ed, "CURVES", &name, "Name", &v) {
                            d.name = Some(v);
                        }
                    }
                    ui.label("Type");
                    egui::ComboBox::from_id_salt("swmm-curve-type")
                        .selected_text(if d.kind.is_empty() {
                            "(none)"
                        } else {
                            d.kind.as_str()
                        })
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for t in CURVE_TYPES {
                                if ui
                                    .selectable_label(d.kind.eq_ignore_ascii_case(t), *t)
                                    .clicked()
                                {
                                    d.kind = t.to_string();
                                    d.dirty = true;
                                }
                            }
                        });
                });
                let mut remove: Option<usize> = None;
                egui::ScrollArea::vertical()
                    .id_salt("swmm-curve-points")
                    .max_height(150.0)
                    .show(ui, |ui| {
                        egui::Grid::new("swmm-curve-grid")
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label(RichText::new("X").strong());
                                ui.label(RichText::new("Y").strong());
                                ui.label("");
                                ui.end_row();
                                for i in 0..d.points.len() {
                                    let (x, y) = &mut d.points[i];
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(x)
                                                .id(Id::new(("swmm-curve-x", i)))
                                                .desired_width(80.0),
                                        )
                                        .changed()
                                    {
                                        d.dirty = true;
                                    }
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(y)
                                                .id(Id::new(("swmm-curve-y", i)))
                                                .desired_width(80.0),
                                        )
                                        .changed()
                                    {
                                        d.dirty = true;
                                    }
                                    if ui.small_button("×").clicked() {
                                        remove = Some(i);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                if let Some(i) = remove {
                    d.points.remove(i);
                    d.dirty = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("Add row").clicked() {
                        let last = d.points.last().cloned().unwrap_or(("0".into(), "0".into()));
                        let x: f64 = last.0.parse().unwrap_or(0.0);
                        d.points
                            .push((stormsewer_swmm::doc::format_number(x + 1.0), last.1));
                        d.dirty = true;
                    }
                    if ui
                        .add_enabled(d.dirty, egui::Button::new("Apply"))
                        .clicked()
                        && apply_curve(ed, &mut d)
                    {
                        state.status = format!("Curve {name} updated");
                    }
                    if ui
                        .add_enabled(d.dirty, egui::Button::new("Revert"))
                        .clicked()
                    {
                        load_curve(&ed.doc, &mut d, Some(name.clone()));
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                let pts: Vec<(f64, f64)> = d
                    .points
                    .iter()
                    .filter_map(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .collect();
                plot(ui, Id::new("swmm-curve-plot"), &pts, 120.0, false);
            });
        });
    if close || !open {
        state.swmm_doc.dialogs.curves = None;
    } else {
        state.swmm_doc.dialogs.curves = Some(d);
    }
}

fn draw_series(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.series.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    window(ctx, "Time Series", Vec2::new(680.0, 480.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            if !d.dirty && d.loaded_gen != Some(ed.doc.generation()) {
                let n = d.name.clone();
                load_series(&ed.doc, &mut d, n);
            }
            let names = ed.doc.names("TIMESERIES");
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("Add a time series").clicked() {
                        let n = build::unique_name(&ed.doc, ObjectKind::Timeseries, "TS");
                        let cmd = Command::AddRow {
                            section: "TIMESERIES".into(),
                            fields: vec![n.clone(), "0:00".into(), "0".into()],
                            comment: None,
                        };
                        if ed.apply(cmd, &format!("add time series {n}")) {
                            load_series(&ed.doc, &mut d, Some(n));
                        }
                    }
                    if ui
                        .add_enabled(d.name.is_some(), egui::Button::new("−"))
                        .clicked()
                    {
                        if let Some(n) = d.name.clone() {
                            if ed.apply(
                                Command::DeleteObject {
                                    section: "TIMESERIES".into(),
                                    name: n.clone(),
                                },
                                &format!("delete time series {n}"),
                            ) {
                                load_series(&ed.doc, &mut d, None);
                            }
                        }
                    }
                });
                if let Some(pick) =
                    name_list(ui, "swmm-series-list", &names, d.name.as_deref(), 200.0)
                {
                    if d.dirty {
                        apply_series(ed, &mut d);
                    }
                    load_series(&ed.doc, &mut d, Some(pick));
                }
                ui.separator();
                ui.label(RichText::new("Paste rows (date time value, or time value):").small());
                ui.add(
                    egui::TextEdit::multiline(&mut d.paste)
                        .id(Id::new("swmm-series-paste"))
                        .desired_rows(5)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(d.name.is_some(), egui::Button::new("Replace with pasted"))
                        .clicked()
                    {
                        let pts = parse_series_text(&d.paste);
                        if !pts.is_empty() {
                            d.points = pts;
                            d.file = None;
                            d.dirty = true;
                        }
                    }
                    if ui
                        .add_enabled(d.name.is_some(), egui::Button::new("Append pasted"))
                        .clicked()
                    {
                        let pts = parse_series_text(&d.paste);
                        if !pts.is_empty() {
                            d.points.extend(pts);
                            d.file = None;
                            d.dirty = true;
                        }
                    }
                });
                let ui = &mut cols[1];
                let Some(name) = d.name.clone() else {
                    ui.label("Choose a series, or + to add one.");
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let id = Id::new("swmm-series-name");
                    if let Some(v) = text_field(ui, id, &name, &mut ed.dialogs.draft, 120.0) {
                        if swmm_props::commit(ed, "TIMESERIES", &name, "Name", &v) {
                            d.name = Some(v);
                        }
                    }
                });
                let mut from_file = d.file.is_some();
                if ui
                    .checkbox(&mut from_file, "From an external file")
                    .changed()
                {
                    d.file = if from_file { Some(String::new()) } else { None };
                    d.dirty = true;
                }
                if let Some(f) = d.file.as_mut() {
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::TextEdit::singleline(f)
                                    .id(Id::new("swmm-series-file"))
                                    .desired_width(200.0),
                            )
                            .changed()
                        {
                            d.dirty = true;
                        }
                        if ui.button("…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                *f = p.display().to_string();
                                d.dirty = true;
                            }
                        }
                    });
                } else {
                    let mut remove: Option<usize> = None;
                    egui::ScrollArea::vertical()
                        .id_salt("swmm-series-points")
                        .max_height(170.0)
                        .show(ui, |ui| {
                            egui::Grid::new("swmm-series-grid")
                                .num_columns(4)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label(RichText::new("Date").strong());
                                    ui.label(RichText::new("Time").strong());
                                    ui.label(RichText::new("Value").strong());
                                    ui.label("");
                                    ui.end_row();
                                    for i in 0..d.points.len() {
                                        let p = &mut d.points[i];
                                        let mut date = p.date.clone().unwrap_or_default();
                                        if ui
                                            .add(
                                                egui::TextEdit::singleline(&mut date)
                                                    .id(Id::new(("swmm-series-d", i)))
                                                    .desired_width(80.0),
                                            )
                                            .changed()
                                        {
                                            p.date = if date.trim().is_empty() {
                                                None
                                            } else {
                                                Some(date)
                                            };
                                            d.dirty = true;
                                        }
                                        if ui
                                            .add(
                                                egui::TextEdit::singleline(&mut p.time)
                                                    .id(Id::new(("swmm-series-t", i)))
                                                    .desired_width(60.0),
                                            )
                                            .changed()
                                        {
                                            d.dirty = true;
                                        }
                                        if ui
                                            .add(
                                                egui::TextEdit::singleline(&mut p.value)
                                                    .id(Id::new(("swmm-series-v", i)))
                                                    .desired_width(60.0),
                                            )
                                            .changed()
                                        {
                                            d.dirty = true;
                                        }
                                        if ui.small_button("×").clicked() {
                                            remove = Some(i);
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                    if let Some(i) = remove {
                        d.points.remove(i);
                        d.dirty = true;
                    }
                }
                ui.horizontal(|ui| {
                    if d.file.is_none() && ui.button("Add row").clicked() {
                        let last = d.points.last().cloned();
                        d.points.push(SeriesPoint {
                            date: last.as_ref().and_then(|p| p.date.clone()),
                            time: "0:00".into(),
                            value: "0".into(),
                        });
                        d.dirty = true;
                    }
                    if ui
                        .add_enabled(d.dirty, egui::Button::new("Apply"))
                        .clicked()
                        && apply_series(ed, &mut d)
                    {
                        state.status = format!("Time series {name} updated");
                    }
                    if ui
                        .add_enabled(d.dirty, egui::Button::new("Revert"))
                        .clicked()
                    {
                        load_series(&ed.doc, &mut d, Some(name.clone()));
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                if d.file.is_none() {
                    let pts: Vec<(f64, f64)> = d
                        .points
                        .iter()
                        .enumerate()
                        .filter_map(|(i, p)| {
                            Some((
                                hours_of(&p.time).unwrap_or(i as f64),
                                p.value.trim().parse().ok()?,
                            ))
                        })
                        .collect();
                    plot(ui, Id::new("swmm-series-plot"), &pts, 120.0, false);
                }
            });
        });
    if close || !open {
        state.swmm_doc.dialogs.series = None;
    } else {
        state.swmm_doc.dialogs.series = Some(d);
    }
}

/// `h:mm` or decimal hours as hours.
pub fn hours_of(time: &str) -> Option<f64> {
    let t = time.trim();
    if let Some((h, m)) = t.split_once(':') {
        let h: f64 = h.parse().ok()?;
        let (m, s) = match m.split_once(':') {
            Some((m, s)) => (m.parse::<f64>().ok()?, s.parse::<f64>().unwrap_or(0.0)),
            None => (m.parse::<f64>().ok()?, 0.0),
        };
        Some(h + m / 60.0 + s / 3600.0)
    } else {
        t.parse().ok()
    }
}

fn draw_patterns(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.patterns.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    window(ctx, "Time Patterns", Vec2::new(620.0, 420.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &mut state.swmm_doc;
            if !d.dirty && d.loaded_gen != Some(ed.doc.generation()) {
                let n = d.name.clone();
                load_pattern(&ed.doc, &mut d, n);
            }
            let names = ed.doc.names("PATTERNS");
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("Add a pattern").clicked() {
                        let n = build::unique_name(&ed.doc, ObjectKind::Pattern, "Pattern");
                        let mut fields = vec![n.clone(), "HOURLY".into()];
                        fields.extend(std::iter::repeat_n("1.0".to_string(), 6));
                        let cmd = Command::AddRow {
                            section: "PATTERNS".into(),
                            fields,
                            comment: None,
                        };
                        if ed.apply(cmd, &format!("add pattern {n}")) {
                            load_pattern(&ed.doc, &mut d, Some(n));
                        }
                    }
                    if ui
                        .add_enabled(d.name.is_some(), egui::Button::new("−"))
                        .clicked()
                    {
                        if let Some(n) = d.name.clone() {
                            if ed.apply(
                                Command::DeleteObject {
                                    section: "PATTERNS".into(),
                                    name: n.clone(),
                                },
                                &format!("delete pattern {n}"),
                            ) {
                                load_pattern(&ed.doc, &mut d, None);
                            }
                        }
                    }
                });
                if let Some(pick) =
                    name_list(ui, "swmm-pattern-list", &names, d.name.as_deref(), 300.0)
                {
                    if d.dirty {
                        apply_pattern(ed, &mut d);
                    }
                    load_pattern(&ed.doc, &mut d, Some(pick));
                }
                let ui = &mut cols[1];
                let Some(name) = d.name.clone() else {
                    ui.label("Choose a pattern, or + to add one.");
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let id = Id::new("swmm-pattern-name");
                    if let Some(v) = text_field(ui, id, &name, &mut ed.dialogs.draft, 110.0) {
                        if swmm_props::commit(ed, "PATTERNS", &name, "Name", &v) {
                            d.name = Some(v);
                        }
                    }
                    ui.label("Type");
                    egui::ComboBox::from_id_salt("swmm-pattern-type")
                        .selected_text(d.kind.clone())
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for t in PATTERN_TYPES {
                                if ui
                                    .selectable_label(d.kind.eq_ignore_ascii_case(t), *t)
                                    .clicked()
                                {
                                    d.kind = t.to_string();
                                    d.values.resize(pattern_len(t), "1.0".into());
                                    d.dirty = true;
                                }
                            }
                        });
                });
                let n = pattern_len(&d.kind);
                if d.values.len() != n && ui.small_button(format!("Resize to {n} values")).clicked()
                {
                    d.values.resize(n, "1.0".into());
                    d.dirty = true;
                }
                egui::ScrollArea::vertical()
                    .id_salt("swmm-pattern-values")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        egui::Grid::new("swmm-pattern-grid")
                            .num_columns(6)
                            .striped(true)
                            .show(ui, |ui| {
                                for i in 0..d.values.len() {
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut d.values[i])
                                                .id(Id::new(("swmm-pattern-v", i)))
                                                .desired_width(48.0),
                                        )
                                        .changed()
                                    {
                                        d.dirty = true;
                                    }
                                    if (i + 1) % 6 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(d.dirty, egui::Button::new("Apply"))
                        .clicked()
                        && apply_pattern(ed, &mut d)
                    {
                        state.status = format!("Pattern {name} updated");
                    }
                    if ui
                        .add_enabled(d.dirty, egui::Button::new("Revert"))
                        .clicked()
                    {
                        load_pattern(&ed.doc, &mut d, Some(name.clone()));
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                let pts: Vec<(f64, f64)> = d
                    .values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i as f64, v.trim().parse().unwrap_or(0.0)))
                    .collect();
                plot(ui, Id::new("swmm-pattern-plot"), &pts, 110.0, true);
            });
        });
    if close || !open {
        state.swmm_doc.dialogs.patterns = None;
    } else {
        state.swmm_doc.dialogs.patterns = Some(d);
    }
}

fn draw_controls(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut text) = state.swmm_doc.dialogs.controls.clone() else {
        return;
    };
    let mut open = true;
    let mut done = false;
    window(ctx, "Control Rules", Vec2::new(560.0, 380.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("The [CONTROLS] section as text: RULE name / IF … / THEN … / PRIORITY n.");
            egui::ScrollArea::vertical()
                .id_salt("swmm-controls-scroll")
                .max_height(260.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .id(Id::new("swmm-controls-text"))
                            .desired_rows(14)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    if apply_controls(&mut state.swmm_doc, &text) {
                        state.status = "Controls updated".into();
                    }
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
        });
    if done || !open {
        state.swmm_doc.dialogs.controls = None;
    } else {
        state.swmm_doc.dialogs.controls = Some(text);
    }
}

/// A basic table editor for a single-row-per-name section: every row's
/// fields in place, add a row with defaults, delete a row. Each change is
/// one step.
fn simple_table(ui: &mut Ui, ed: &mut SwmmEditor, section: &str, prefix: &str, defaults: &[&str]) {
    let rows: Vec<(usize, stormsewer_swmm::doc::Row)> = ed
        .doc
        .rows(section)
        .into_iter()
        .map(|(li, r)| (li, r.clone()))
        .collect();
    let mut pending: Option<(usize, String, String, String)> = None;
    let mut delete: Option<(usize, String)> = None;
    egui::ScrollArea::both()
        .id_salt(("swmm-simple-table", section))
        .max_height(260.0)
        .show(ui, |ui| {
            egui::Grid::new(("swmm-simple-grid", section))
                .striped(true)
                .show(ui, |ui| {
                    let header_cols = rows
                        .first()
                        .map(|(_, r)| ed.doc.columns(section, r))
                        .unwrap_or_else(|| {
                            ed.doc.columns(
                                section,
                                &stormsewer_swmm::doc::Row {
                                    fields: vec![String::new()],
                                    comment: None,
                                },
                            )
                        });
                    for c in header_cols {
                        ui.label(RichText::new(*c).strong());
                    }
                    ui.label("");
                    ui.end_row();
                    for (li, row) in &rows {
                        let cols = ed.doc.columns(section, row);
                        let name = row.value(0).unwrap_or("").to_string();
                        for (i, col) in cols.iter().enumerate() {
                            let fs = spec(&ed.doc, section, col);
                            let value = row.value(i).unwrap_or("");
                            let id = Id::new(("swmm-simple-cell", section, *li, *col));
                            if let Some(v) =
                                field_widget(ui, id, &fs, value, &mut ed.dialogs.draft, 80.0)
                            {
                                pending = Some((*li, name.clone(), col.to_string(), v));
                            }
                        }
                        if ui.small_button("×").clicked() {
                            delete = Some((*li, name.clone()));
                        }
                        ui.end_row();
                    }
                });
        });
    if let Some((li, name, col, v)) = pending {
        if !swmm_props::commit_line(ed, section, li, &name, &col, &v) {
            ed.dialogs.message = ed.last_error.take().unwrap_or_default();
        }
    }
    if let Some((li, name)) = delete {
        ed.apply(
            Command::DeleteLine {
                section: section.into(),
                line: li,
            },
            &format!("delete {} {name}", prefix.to_lowercase()),
        );
    }
    if ui.button(format!("Add {prefix}")).clicked() {
        let name = (1u64..)
            .map(|n| format!("{prefix}{n}"))
            .find(|c| !ed.doc.contains(section, c))
            .expect("a free name exists");
        let mut fields = vec![name.clone()];
        fields.extend(defaults.iter().map(|s| s.to_string()));
        ed.apply(
            Command::AddRow {
                section: section.into(),
                fields,
                comment: None,
            },
            &format!("add {} {name}", prefix.to_lowercase()),
        );
    }
}

fn draw_pollutants(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.dialogs.pollutants {
        return;
    }
    let mut open = true;
    window(ctx, "Pollutants", Vec2::new(760.0, 340.0))
        .open(&mut open)
        .show(ctx, |ui| {
            simple_table(
                ui,
                &mut state.swmm_doc,
                "POLLUTANTS",
                "Pollutant",
                &["MG/L", "0", "0", "0", "0", "NO", "*", "0", "0", "0"],
            );
        });
    if !open {
        state.swmm_doc.dialogs.pollutants = false;
    }
}

fn draw_landuses(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.dialogs.landuses {
        return;
    }
    let mut open = true;
    window(ctx, "Land Uses", Vec2::new(520.0, 300.0))
        .open(&mut open)
        .show(ctx, |ui| {
            simple_table(
                ui,
                &mut state.swmm_doc,
                "LANDUSES",
                "LandUse",
                &["0", "0", "0"],
            );
            ui.label(RichText::new("Buildup and washoff rows: View → Attribute Table.").small());
        });
    if !open {
        state.swmm_doc.dialogs.landuses = false;
    }
}

/// Every project dialog that is open.
pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.loaded {
        return;
    }
    draw_title(ctx, state);
    draw_options(ctx, state);
    draw_gages(ctx, state);
    draw_curves(ctx, state);
    draw_series(ctx, state);
    draw_patterns(ctx, state);
    draw_controls(ctx, state);
    draw_pollutants(ctx, state);
    draw_landuses(ctx, state);
    let _ = Color32::TRANSPARENT;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_rows_keeps_place_and_trims() {
        let mut doc = InpDoc::parse("[CURVES]\n;WQCV\nSU1 Storage 0 100\nSU1 4 200\nOTHER 0 1\n");
        let rows = curve_rows("SU1", "STORAGE", &[("0".into(), "5".into())]);
        doc.apply(replace_rows(&doc, "CURVES", "SU1", &rows))
            .unwrap();
        assert_eq!(
            doc.to_string(),
            "[CURVES]\n;WQCV\nSU1              STORAGE    0          5\nOTHER 0 1\n"
        );
        let rows = curve_rows(
            "SU1",
            "STORAGE",
            &[
                ("0".into(), "5".into()),
                ("1".into(), "6".into()),
                ("2".into(), "7".into()),
            ],
        );
        doc.apply(replace_rows(&doc, "CURVES", "SU1", &rows))
            .unwrap();
        let (kind, pts) = doc.curve("SU1");
        assert_eq!(kind.as_deref(), Some("STORAGE"));
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[2], ("2".to_string(), "7".to_string()));
        assert!(
            doc.to_string().contains("\nOTHER 0 1\n"),
            "{}",
            doc.to_string()
        );
        assert!(doc.undo());
        assert!(doc.undo());
        assert!(doc.to_string().contains("SU1 Storage 0 100\nSU1 4 200\n"));
    }

    #[test]
    fn series_paste_parses_dated_and_plain() {
        let pts = parse_series_text("01/01/2007\t0:00\t0.5\n0:15 1.0\njunk\n,0:30,2");
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0].date.as_deref(), Some("01/01/2007"));
        assert_eq!(pts[1].time, "0:15");
        assert_eq!(pts[2].value, "2");
        assert_eq!(hours_of("1:30"), Some(1.5));
        assert_eq!(hours_of("2"), Some(2.0));
    }

    #[test]
    fn options_command_only_touches_changed_keys() {
        let doc = InpDoc::parse("[OPTIONS]\nFLOW_UNITS CFS\nMIN_SLOPE 0\n");
        let mut d = options_draft(&doc);
        assert_eq!(d.values["FLOW_UNITS"], "CFS");
        d.values.insert("FLOW_UNITS".into(), "CMS".into());
        d.values.insert("MIN_SLOPE".into(), String::new());
        d.values.insert("THREADS".into(), "4".into());
        let Command::Batch(cmds) = options_command(&doc, &d) else {
            panic!()
        };
        assert_eq!(cmds.len(), 3, "{cmds:?}");
        let mut doc = doc;
        doc.apply(Command::Batch(cmds)).unwrap();
        assert_eq!(doc.option("FLOW_UNITS"), Some("CMS"));
        assert_eq!(doc.option("MIN_SLOPE"), None);
        assert_eq!(doc.option("THREADS"), Some("4"));
    }

    #[test]
    fn section_text_roundtrips_controls() {
        let doc =
            InpDoc::parse("[CONTROLS]\nRULE R1\nIF NODE J1 DEPTH > 2\nTHEN PUMP P1 STATUS = ON\n");
        let mut doc = doc;
        let text = "RULE R1\nIF NODE J1 DEPTH > 3\nTHEN PUMP P1 STATUS = ON\nPRIORITY 1";
        doc.apply(set_section_text(&doc, "CONTROLS", text)).unwrap();
        assert_eq!(section_text(&doc, "CONTROLS"), text);
        doc.apply(set_section_text(&doc, "CONTROLS", "")).unwrap();
        assert_eq!(section_text(&doc, "CONTROLS"), "");
    }

    #[test]
    fn pattern_rows_split_by_six() {
        let values: Vec<String> = (0..12).map(|i| format!("{i}")).collect();
        let rows = pattern_rows("P", "MONTHLY", &values);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 8);
        assert_eq!(rows[1].len(), 7);
        let mut doc = InpDoc::parse("");
        doc.apply(replace_rows(&doc, "PATTERNS", "P", &rows))
            .unwrap();
        let (kind, back) = pattern_of(&doc, "P");
        assert_eq!(kind, "MONTHLY");
        assert_eq!(back, values);
    }
}
