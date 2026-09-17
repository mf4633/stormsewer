// SPDX-License-Identifier: GPL-3.0-or-later

//! Attribute tables: one grid per section with the section's typed columns,
//! in-place editing through the same commands as the property sheet, sort
//! by column, a filter box, a column chooser, row selection kept in step
//! with the map selection both ways, Copy as TSV, Export and Import CSV,
//! and Replace in Column — the bulk edit people otherwise script.
//!
//! Rows are addressed by line index, so the multi-row sections (vertices,
//! series, curves) show every row and edit the one you clicked.

use std::collections::HashSet;

use eframe::egui::{self, Id, RichText, Ui};
use stormsewer_swmm::doc::schema;
use stormsewer_swmm::doc::{Command, InpDoc};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;
use crate::swmm_props::{self, field_widget, spec, FieldKind};
use crate::theme::palette;

/// A bulk edit of one column.
#[derive(Clone, Debug, PartialEq)]
pub enum Replace {
    Value(String),
    Scale(f64),
}

#[derive(Clone, Debug)]
pub struct GridState {
    pub open: bool,
    pub section: String,
    pub sort_col: Option<usize>,
    pub sort_desc: bool,
    pub filter: String,
    pub hidden: HashSet<String>,
    pub show_columns: bool,
    pub draft: Option<(Id, String)>,
    pub replace_col: usize,
    pub replace_value: String,
    pub replace_factor: String,
    pub message: String,
}

impl Default for GridState {
    fn default() -> Self {
        Self {
            open: false,
            section: "JUNCTIONS".into(),
            sort_col: None,
            sort_desc: false,
            filter: String::new(),
            hidden: HashSet::new(),
            show_columns: false,
            draft: None,
            replace_col: 1,
            replace_value: String::new(),
            replace_factor: "1.0".into(),
            message: String::new(),
        }
    }
}

/// One row of a grid.
#[derive(Clone, Debug, PartialEq)]
pub struct GridRow {
    pub line: usize,
    pub name: String,
    pub fields: Vec<String>,
    pub cols: &'static [&'static str],
}

impl GridRow {
    /// The value under `header`, if this row's layout has that column.
    pub fn cell(&self, header: &str) -> Option<&str> {
        let i = schema::field_index(self.cols, header)?;
        self.fields.get(i).map(String::as_str)
    }
}

/// The sections a grid can show: those with named rows and a column table.
pub fn grid_sections(doc: &InpDoc) -> Vec<String> {
    const ORDER: &[&str] = &[
        "RAINGAGES",
        "SUBCATCHMENTS",
        "SUBAREAS",
        "INFILTRATION",
        "LID_USAGE",
        "JUNCTIONS",
        "OUTFALLS",
        "DIVIDERS",
        "STORAGE",
        "CONDUITS",
        "PUMPS",
        "ORIFICES",
        "WEIRS",
        "OUTLETS",
        "XSECTIONS",
        "LOSSES",
        "INFLOWS",
        "DWF",
        "POLLUTANTS",
        "LANDUSES",
        "COVERAGES",
        "LOADINGS",
        "CURVES",
        "TIMESERIES",
        "PATTERNS",
        "COORDINATES",
        "VERTICES",
        "POLYGONS",
        "SYMBOLS",
        "TAGS",
        "GROUNDWATER",
        "INLET_USAGE",
    ];
    let mut out: Vec<String> = ORDER.iter().map(|s| s.to_string()).collect();
    for s in doc.sections() {
        if schema::name_index(&s.name).is_some()
            && !out.contains(&s.name)
            && !schema::KEY_VALUE_SECTIONS.contains(&s.name.as_str())
        {
            out.push(s.name.clone());
        }
    }
    out
}

/// The headers (the union of the rows' layouts, first-seen order) and rows
/// of `section`.
pub fn build_grid(doc: &InpDoc, section: &str) -> (Vec<String>, Vec<GridRow>) {
    let sec = section.to_ascii_uppercase();
    let ni = schema::name_index(&sec).unwrap_or(0);
    let mut headers: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for (line, row) in doc.rows(&sec) {
        let cols = doc.columns(&sec, row);
        for c in cols {
            if !headers.iter().any(|h| h.eq_ignore_ascii_case(c)) {
                headers.push(c.to_string());
            }
        }
        let extra = row.fields.len().saturating_sub(cols.len());
        for i in 0..extra {
            let h = format!("#{}", cols.len() + i);
            if !headers.contains(&h) {
                headers.push(h);
            }
        }
        rows.push(GridRow {
            line,
            name: row.value(ni).unwrap_or("").to_string(),
            fields: row
                .fields
                .iter()
                .map(|f| stormsewer_swmm::doc::unquote(f).to_string())
                .collect(),
            cols,
        });
    }
    if headers.is_empty() {
        headers.push("#0".into());
    }
    (headers, rows)
}

fn cell_of<'a>(row: &'a GridRow, header: &str) -> &'a str {
    if let Some(v) = row.cell(header) {
        return v;
    }
    header
        .strip_prefix('#')
        .and_then(|i| i.parse::<usize>().ok())
        .and_then(|i| row.fields.get(i))
        .map(String::as_str)
        .unwrap_or("")
}

/// Row indices after the filter and sort.
pub fn visible_rows(headers: &[String], rows: &[GridRow], state: &GridState) -> Vec<usize> {
    let needle = state.filter.trim().to_ascii_lowercase();
    let mut idx: Vec<usize> = (0..rows.len())
        .filter(|&i| {
            needle.is_empty()
                || rows[i]
                    .fields
                    .iter()
                    .any(|f| f.to_ascii_lowercase().contains(&needle))
        })
        .collect();
    if let Some(c) = state.sort_col {
        if let Some(h) = headers.get(c) {
            let numeric = idx.iter().all(|&i| {
                cell_of(&rows[i], h).parse::<f64>().is_ok() || cell_of(&rows[i], h).is_empty()
            });
            idx.sort_by(|&a, &b| {
                let va = cell_of(&rows[a], h);
                let vb = cell_of(&rows[b], h);
                let ord = if numeric {
                    let fa = va.parse::<f64>().unwrap_or(f64::NEG_INFINITY);
                    let fb = vb.parse::<f64>().unwrap_or(f64::NEG_INFINITY);
                    fa.total_cmp(&fb)
                } else {
                    va.to_ascii_lowercase().cmp(&vb.to_ascii_lowercase())
                };
                if state.sort_desc {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }
    }
    idx
}

/// Tab-separated text of the given rows with a header line.
pub fn tsv(
    headers: &[String],
    rows: &[GridRow],
    which: &[usize],
    hidden: &HashSet<String>,
) -> String {
    let shown: Vec<&String> = headers.iter().filter(|h| !hidden.contains(*h)).collect();
    let mut out = shown
        .iter()
        .map(|h| h.as_str())
        .collect::<Vec<_>>()
        .join("\t");
    out.push('\n');
    for &i in which {
        let line: Vec<&str> = shown.iter().map(|h| cell_of(&rows[i], h)).collect();
        out.push_str(&line.join("\t"));
        out.push('\n');
    }
    out
}

fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Comma-separated text of the given rows with a header line.
pub fn csv(
    headers: &[String],
    rows: &[GridRow],
    which: &[usize],
    hidden: &HashSet<String>,
) -> String {
    let shown: Vec<&String> = headers.iter().filter(|h| !hidden.contains(*h)).collect();
    let mut out = shown
        .iter()
        .map(|h| csv_quote(h))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for &i in which {
        let line: Vec<String> = shown
            .iter()
            .map(|h| csv_quote(cell_of(&rows[i], h)))
            .collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// Split one CSV (or TSV) line, honouring double quotes.
pub fn split_delimited(line: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out.iter().map(|s| s.trim().to_string()).collect()
}

/// Update rows of `section` from CSV or TSV text: the first line names the
/// columns, the name column (or the first column) picks the row, and every
/// other column that the row's layout has is set. One undo step.
pub fn import_csv(ed: &mut SwmmEditor, section: &str, text: &str) -> Result<usize, String> {
    let sec = section.to_ascii_uppercase();
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines.next().ok_or("the text is empty")?;
    let sep = if header_line.contains('\t') {
        '\t'
    } else {
        ','
    };
    let headers = split_delimited(header_line, sep);
    let ni = schema::name_index(&sec).unwrap_or(0);
    let (grid_headers, rows) = build_grid(&ed.doc, &sec);
    let name_col = headers
        .iter()
        .position(|h| {
            grid_headers
                .get(ni)
                .is_some_and(|g| g.eq_ignore_ascii_case(h))
        })
        .unwrap_or(0);
    let mut cmds = Vec::new();
    let mut touched = 0usize;
    for line in lines {
        let values = split_delimited(line, sep);
        let Some(name) = values.get(name_col) else {
            continue;
        };
        let Some(row) = rows.iter().find(|r| r.name.eq_ignore_ascii_case(name)) else {
            continue;
        };
        let mut fields = row.fields.clone();
        let mut changed = false;
        for (h, v) in headers.iter().zip(values.iter()) {
            if h.eq_ignore_ascii_case(&grid_headers[ni]) {
                continue;
            }
            let Some(i) = schema::field_index(row.cols, h) else {
                continue;
            };
            if i >= fields.len() {
                continue;
            }
            if swmm_props::is_layout_column(&sec, h) {
                if let Some(f) = swmm_props::switched_row(&ed.doc, &sec, &fields, h, v) {
                    if f != fields {
                        fields = f;
                        changed = true;
                    }
                }
                continue;
            }
            if fields[i] != *v {
                fields[i] = v.clone();
                changed = true;
            }
        }
        if changed {
            let comment = ed
                .doc
                .section(&sec)
                .and_then(|s| s.lines.get(row.line))
                .and_then(|l| l.row.as_ref())
                .and_then(|r| r.comment.clone());
            cmds.push(Command::SetLine {
                section: sec.clone(),
                line: row.line,
                fields,
                comment,
            });
            touched += 1;
        }
    }
    if cmds.is_empty() {
        return Ok(0);
    }
    if ed.apply(
        Command::Batch(cmds),
        &format!("import {touched} rows into [{sec}]"),
    ) {
        Ok(touched)
    } else {
        Err(ed.last_error.clone().unwrap_or_default())
    }
}

/// Set `header` of every listed row to a value, or scale it by a factor.
/// One undo step.
pub fn replace_in_column(
    ed: &mut SwmmEditor,
    section: &str,
    rows: &[GridRow],
    which: &[usize],
    header: &str,
    op: &Replace,
) -> Result<usize, String> {
    let sec = section.to_ascii_uppercase();
    let fs = spec(&ed.doc, &sec, header);
    let mut cmds = Vec::new();
    for &i in which {
        let row = &rows[i];
        let Some(idx) = schema::field_index(row.cols, header) else {
            continue;
        };
        if idx >= row.fields.len() {
            continue;
        }
        let new = match op {
            Replace::Value(v) => v.trim().to_string(),
            Replace::Scale(k) => {
                let Ok(cur) = row.fields[idx].parse::<f64>() else {
                    continue;
                };
                stormsewer_swmm::doc::format_number(cur * k)
            }
        };
        if fs.kind == FieldKind::Number && !new.is_empty() && new.parse::<f64>().is_err() {
            return Err(format!("{new:?} is not a number"));
        }
        if new == row.fields[idx] {
            continue;
        }
        let fields = match swmm_props::switched_row(&ed.doc, &sec, &row.fields, header, &new) {
            Some(f) => f,
            None => {
                let mut f = row.fields.clone();
                f[idx] = new;
                f
            }
        };
        let comment = ed
            .doc
            .section(&sec)
            .and_then(|s| s.lines.get(row.line))
            .and_then(|l| l.row.as_ref())
            .and_then(|r| r.comment.clone());
        cmds.push(Command::SetLine {
            section: sec.clone(),
            line: row.line,
            fields,
            comment,
        });
    }
    let n = cmds.len();
    if n == 0 {
        return Ok(0);
    }
    let label = match op {
        Replace::Value(v) => format!("set {header} of {n} rows to {v}"),
        Replace::Scale(k) => format!("scale {header} of {n} rows by {k}"),
    };
    if ed.apply(Command::Batch(cmds), &label) {
        Ok(n)
    } else {
        Err(ed.last_error.clone().unwrap_or_default())
    }
}

/// Open the table on `section`.
pub fn open(ed: &mut SwmmEditor, section: &str) {
    ed.grid.open = true;
    ed.grid.section = section.to_ascii_uppercase();
    ed.grid.sort_col = None;
    ed.grid.replace_col = 1;
}

/// The attribute table window.
pub fn draw_grid_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.grid.open || !state.swmm_doc.loaded {
        return;
    }
    let mut open = true;
    egui::Window::new("Attribute Table")
        .id(Id::new("swmm-attribute-table"))
        .open(&mut open)
        .default_size(egui::vec2(760.0, 420.0))
        .resizable(true)
        .show(ctx, |ui| draw_grid(ui, state));
    if !open {
        state.swmm_doc.grid.open = false;
    }
}

/// The grid itself, for the window or any other host.
pub fn draw_grid(ui: &mut Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let ed = &mut state.swmm_doc;
    let sections = grid_sections(&ed.doc);
    if !sections.contains(&ed.grid.section) {
        ed.grid.section = sections
            .first()
            .cloned()
            .unwrap_or_else(|| "JUNCTIONS".into());
    }
    let section = ed.grid.section.clone();
    let (headers, rows) = build_grid(&ed.doc, &section);

    // -- toolbar ----------------------------------------------------------------
    ui.horizontal_wrapped(|ui| {
        ui.label("Section");
        egui::ComboBox::from_id_salt("swmm-grid-section")
            .selected_text(&section)
            .width(150.0)
            .show_ui(ui, |ui| {
                for s in &sections {
                    let n = ed.doc.rows(s).len();
                    if ui
                        .selectable_label(*s == section, format!("{s} ({n})"))
                        .clicked()
                    {
                        open(ed, s);
                    }
                }
            });
        ui.label("Filter");
        ui.add(
            egui::TextEdit::singleline(&mut ed.grid.filter)
                .id(Id::new("swmm-grid-filter"))
                .desired_width(120.0),
        );
        if ui
            .small_button("×")
            .on_hover_text("Clear the filter")
            .clicked()
        {
            ed.grid.filter.clear();
        }
        ui.toggle_value(&mut ed.grid.show_columns, "Columns…");
    });
    if ed.grid.show_columns {
        ui.horizontal_wrapped(|ui| {
            for h in &headers {
                let mut shown = !ed.grid.hidden.contains(h);
                if ui.checkbox(&mut shown, h).changed() {
                    if shown {
                        ed.grid.hidden.remove(h);
                    } else {
                        ed.grid.hidden.insert(h.clone());
                    }
                }
            }
        });
    }
    let visible = visible_rows(&headers, &rows, &ed.grid);
    let selected_visible: Vec<usize> = visible
        .iter()
        .copied()
        .filter(|&i| {
            SwmmEditor::objref_for(&section, &rows[i].name).is_some_and(|r| ed.is_selected(&r))
        })
        .collect();
    let export_rows: Vec<usize> = if selected_visible.is_empty() {
        visible.clone()
    } else {
        selected_visible.clone()
    };

    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{} of {} rows", visible.len(), rows.len()));
        if !selected_visible.is_empty() {
            ui.label(format!("· {} selected", selected_visible.len()));
        }
        ui.separator();
        if ui.button("Copy as TSV").clicked() {
            let text = tsv(&headers, &rows, &export_rows, &ed.grid.hidden);
            ui.output_mut(|o| o.copied_text = text);
            ed.grid.message = format!("Copied {} rows", export_rows.len());
        }
        if ui.button("Export CSV…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name(format!("{}.csv", section.to_lowercase()))
                .save_file()
            {
                let text = csv(&headers, &rows, &export_rows, &ed.grid.hidden);
                ed.grid.message = match std::fs::write(&path, text) {
                    Ok(()) => format!("Wrote {}", path.display()),
                    Err(e) => format!("Export failed: {e}"),
                };
            }
        }
        if ui.button("Import CSV…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV or TSV", &["csv", "tsv", "txt"])
                .pick_file()
            {
                ed.grid.message = match std::fs::read_to_string(&path) {
                    Ok(text) => match import_csv(ed, &section, &text) {
                        Ok(n) => format!("Updated {n} rows from {}", path.display()),
                        Err(e) => format!("Import failed: {e}"),
                    },
                    Err(e) => format!("Import failed: {e}"),
                };
            }
        }
        ui.separator();
        let has_sel = !selected_visible.is_empty();
        if ui
            .add_enabled(has_sel, egui::Button::new("Zoom to"))
            .clicked()
        {
            if let Some(&i) = selected_visible.first() {
                ed.pending_zoom_to = SwmmEditor::objref_for(&section, &rows[i].name);
            }
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("Properties"))
            .clicked()
        {
            ed.show_properties = true;
            ed.focus_sheet = true;
        }
    });

    // -- replace in column ----------------------------------------------------------
    ui.horizontal_wrapped(|ui| {
        ui.label("Replace in column");
        let col = ed.grid.replace_col.min(headers.len().saturating_sub(1));
        ed.grid.replace_col = col;
        egui::ComboBox::from_id_salt("swmm-grid-replace-col")
            .selected_text(headers.get(col).cloned().unwrap_or_default())
            .width(110.0)
            .show_ui(ui, |ui| {
                for (i, h) in headers.iter().enumerate() {
                    ui.selectable_value(&mut ed.grid.replace_col, i, h);
                }
            });
        let header = headers
            .get(ed.grid.replace_col)
            .cloned()
            .unwrap_or_default();
        ui.label("to");
        ui.add(
            egui::TextEdit::singleline(&mut ed.grid.replace_value)
                .id(Id::new("swmm-grid-replace-value"))
                .desired_width(90.0),
        );
        if ui
            .button("Set all")
            .on_hover_text("Every filtered row (or the selected rows)")
            .clicked()
        {
            let op = Replace::Value(ed.grid.replace_value.clone());
            ed.grid.message =
                match replace_in_column(ed, &section, &rows, &export_rows, &header, &op) {
                    Ok(n) => format!("Set {header} on {n} rows"),
                    Err(e) => e,
                };
        }
        ui.label("or scale by");
        ui.add(
            egui::TextEdit::singleline(&mut ed.grid.replace_factor)
                .id(Id::new("swmm-grid-replace-factor"))
                .desired_width(50.0),
        );
        if ui.button("Scale").clicked() {
            match ed.grid.replace_factor.trim().parse::<f64>() {
                Ok(k) => {
                    let op = Replace::Scale(k);
                    ed.grid.message =
                        match replace_in_column(ed, &section, &rows, &export_rows, &header, &op) {
                            Ok(n) => format!("Scaled {header} on {n} rows"),
                            Err(e) => e,
                        };
                }
                Err(_) => ed.grid.message = "The factor is not a number".into(),
            }
        }
    });
    if !ed.grid.message.is_empty() {
        ui.label(RichText::new(&ed.grid.message).small());
    }
    ui.separator();

    // -- the table ------------------------------------------------------------------
    let shown: Vec<(usize, String)> = headers
        .iter()
        .enumerate()
        .filter(|(_, h)| !ed.grid.hidden.contains(*h))
        .map(|(i, h)| (i, h.clone()))
        .collect();
    let mut pending: Option<(usize, String, String, String)> = None;
    let mut pick: Option<(usize, egui::Modifiers)> = None;
    let sel_color = palette::canvas::selection(dark);
    egui::ScrollArea::both()
        .id_salt("swmm-grid-scroll")
        .max_height(ui.available_height() - 4.0)
        .show(ui, |ui| {
            egui::Grid::new(("swmm-grid", &section))
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("#").strong());
                    for (i, h) in &shown {
                        let arrow = match (ed.grid.sort_col, ed.grid.sort_desc) {
                            (Some(c), false) if c == *i => " ▲",
                            (Some(c), true) if c == *i => " ▼",
                            _ => "",
                        };
                        let fs = spec(&ed.doc, &section, h);
                        let text = match fs.unit {
                            Some(u) => format!("{h} ({u}){arrow}"),
                            None => format!("{h}{arrow}"),
                        };
                        if ui
                            .selectable_label(
                                ed.grid.sort_col == Some(*i),
                                RichText::new(text).strong(),
                            )
                            .clicked()
                        {
                            if ed.grid.sort_col == Some(*i) {
                                if ed.grid.sort_desc {
                                    ed.grid.sort_col = None;
                                    ed.grid.sort_desc = false;
                                } else {
                                    ed.grid.sort_desc = true;
                                }
                            } else {
                                ed.grid.sort_col = Some(*i);
                                ed.grid.sort_desc = false;
                            }
                        }
                    }
                    ui.end_row();
                    for &ri in &visible {
                        let row = &rows[ri];
                        let objref = SwmmEditor::objref_for(&section, &row.name);
                        let selected = objref.as_ref().is_some_and(|r| ed.is_selected(r));
                        let mark = if selected {
                            RichText::new(format!("{}", row.line))
                                .color(sel_color)
                                .strong()
                        } else {
                            RichText::new(format!("{}", row.line))
                        };
                        let resp = ui.selectable_label(selected, mark);
                        if resp.clicked() {
                            pick = Some((ri, ui.input(|i| i.modifiers)));
                        }
                        if resp.double_clicked() {
                            ed.pending_zoom_to = objref.clone();
                        }
                        for (_, h) in &shown {
                            let idx = schema::field_index(row.cols, h)
                                .or_else(|| h.strip_prefix('#').and_then(|n| n.parse().ok()));
                            match idx {
                                Some(i) if i < row.fields.len() => {
                                    let fs = spec(&ed.doc, &section, h);
                                    let id =
                                        Id::new(("swmm-grid-cell", &section, row.line, h.as_str()));
                                    let value = row.fields[i].as_str();
                                    if let Some(v) =
                                        field_widget(ui, id, &fs, value, &mut ed.grid.draft, 90.0)
                                    {
                                        pending = Some((row.line, row.name.clone(), h.clone(), v));
                                    }
                                }
                                _ => {
                                    ui.label(RichText::new("—").weak());
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    if let Some((line, name, col, v)) = pending {
        let is_name = schema::name_index(&section) == Some(0)
            && headers
                .first()
                .is_some_and(|h| h.eq_ignore_ascii_case(&col))
            && swmm_props::kind_of_section(&section).is_some();
        let ok = if is_name {
            swmm_props::commit(ed, &section, &name, &col, &v)
        } else {
            swmm_props::commit_line(ed, &section, line, &name, &col, &v)
        };
        ed.grid.message = if ok {
            format!("Set {col} of {name}")
        } else {
            ed.last_error.take().unwrap_or_default()
        };
    }
    if let Some((ri, mods)) = pick {
        if let Some(r) = SwmmEditor::objref_for(&section, &rows[ri].name) {
            if mods.ctrl {
                ed.toggle_select(r);
            } else if mods.shift {
                ed.add_select(r);
            } else {
                ed.select_only(r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_handles_quotes_and_tabs() {
        assert_eq!(split_delimited("a,\"b,c\",d", ','), vec!["a", "b,c", "d"]);
        assert_eq!(split_delimited("a\tb\t", '\t'), vec!["a", "b", ""]);
        assert_eq!(split_delimited("\"x \"\"y\"\"\"", ','), vec!["x \"y\""]);
    }

    #[test]
    fn grid_headers_are_the_union_of_layouts() {
        let doc = InpDoc::parse("[OUTFALLS]\nO1 1 FREE NO\nO2 2 FIXED 3 NO\n");
        let (h, rows) = build_grid(&doc, "OUTFALLS");
        assert_eq!(
            h,
            vec!["Name", "Elevation", "Type", "Gated", "RouteTo", "Stage"]
        );
        assert_eq!(rows[0].cell("Stage"), None);
        assert_eq!(rows[1].cell("Stage"), Some("3"));
    }

    #[test]
    fn sort_is_numeric_when_the_column_is() {
        let doc = InpDoc::parse("[JUNCTIONS]\nJ1 10\nJ2 9\nJ3 100\n");
        let (h, rows) = build_grid(&doc, "JUNCTIONS");
        let mut st = GridState {
            sort_col: Some(1),
            ..Default::default()
        };
        assert_eq!(visible_rows(&h, &rows, &st), vec![1, 0, 2]);
        st.sort_desc = true;
        assert_eq!(visible_rows(&h, &rows, &st), vec![2, 0, 1]);
        st.sort_col = None;
        st.filter = "j3".into();
        assert_eq!(visible_rows(&h, &rows, &st), vec![2]);
    }
}
