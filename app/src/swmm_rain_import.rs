// SPDX-License-Identifier: GPL-3.0-or-later

//! Project → Time Series → Import…: a delimited text file or paste (date,
//! time, one value column per series) or a NOAA GHCN-Daily station csv,
//! previewed, then written as `[TIMESERIES]` rows (and, when asked, a
//! `[RAINGAGES]` row per series) in one undo step. And Export to File…:
//! a series written as SWMM's external time-series file, optionally
//! replacing its rows with a `FILE` reference. The parsing is in
//! `stormsewer_swmm::rain`.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Id, RichText, Ui, Vec2};
use stormsewer_swmm::rain::{self, DepthUnit, GageOptions, Import};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;

#[derive(Clone, Debug, PartialEq)]
pub struct ImportDraft {
    pub text: String,
    pub source: Option<PathBuf>,
    pub parsed: Option<Result<Import, String>>,
    pub parsed_for: Option<(String, DepthUnit)>,
    pub unit: DepthUnit,
    pub make_gages: bool,
    pub gage_format: String,
    pub gage_interval: String,
    pub place_gages: bool,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportDraft {
    pub series: String,
    pub replace_with_file: bool,
    pub error: String,
}

/// The unit the model's rainfall is in.
pub fn model_unit(ed: &SwmmEditor) -> DepthUnit {
    match ed
        .doc
        .option("FLOW_UNITS")
        .map(|u| u.to_ascii_uppercase())
        .as_deref()
    {
        Some("CMS" | "LPS" | "MLD") => DepthUnit::Millimetres,
        _ => DepthUnit::Inches,
    }
}

pub fn open_import(ed: &mut SwmmEditor) {
    ed.dialogs.rain_import = Some(ImportDraft {
        text: String::new(),
        source: None,
        parsed: None,
        parsed_for: None,
        unit: model_unit(ed),
        make_gages: false,
        gage_format: "VOLUME".into(),
        gage_interval: "1:00".into(),
        place_gages: true,
        error: String::new(),
    });
}

pub fn open_export(ed: &mut SwmmEditor, series: Option<&str>) {
    let series = series
        .map(str::to_string)
        .or_else(|| ed.dialogs.series.as_ref().and_then(|d| d.name.clone()))
        .or_else(|| ed.doc.names("TIMESERIES").first().cloned())
        .unwrap_or_default();
    ed.dialogs.series_export = Some(ExportDraft {
        series,
        replace_with_file: false,
        error: String::new(),
    });
}

/// Read a file into the draft.
pub fn load_file(d: &mut ImportDraft, path: &Path) {
    match std::fs::read(path) {
        Ok(bytes) => {
            d.text = String::from_utf8_lossy(&bytes).into_owned();
            d.source = Some(path.to_path_buf());
            d.parsed = None;
            d.parsed_for = None;
            d.error.clear();
        }
        Err(e) => d.error = format!("{}: {e}", path.display()),
    }
}

/// Parse when the text or unit changed since the last parse.
pub fn ensure_parsed(ed: &SwmmEditor, d: &mut ImportDraft) {
    let key = (d.text.clone(), d.unit);
    if d.parsed_for.as_ref() == Some(&key) {
        return;
    }
    d.parsed = if d.text.trim().is_empty() {
        None
    } else {
        Some(rain::parse(&d.text, &ed.doc, d.unit))
    };
    d.parsed_for = Some(key);
    // Daily station data reads as daily VOLUME.
    if let Some(Ok(imp)) = &d.parsed {
        if imp.daily {
            d.gage_format = "VOLUME".into();
            d.gage_interval = "24:00".into();
        }
    }
}

/// Where new gage symbols go: along the model's top edge.
fn gage_symbols(ed: &SwmmEditor) -> Option<((f64, f64), f64)> {
    let (x0, _, x1, y1) = ed.bounds?;
    let m = ((x1 - x0) * 0.05).max(10.0);
    Some(((x0 - m + ed.gages.len() as f64 * m * 0.6, y1 + m), m * 0.6))
}

/// Write the parsed import: one undo step. Returns the status line.
pub fn commit(ed: &mut SwmmEditor) -> Option<String> {
    let mut d = ed.dialogs.rain_import.take()?;
    ensure_parsed(ed, &mut d);
    let Some(Ok(imp)) = d.parsed.clone() else {
        d.error = match &d.parsed {
            Some(Err(e)) => e.clone(),
            _ => "nothing to import".into(),
        };
        ed.dialogs.rain_import = Some(d);
        return None;
    };
    let opts = d.make_gages.then(|| GageOptions {
        format: d.gage_format.clone(),
        interval: d.gage_interval.clone(),
        symbols: if d.place_gages {
            gage_symbols(ed)
        } else {
            None
        },
    });
    let (cmd, gages) = rain::commands(&ed.doc, &imp, opts.as_ref());
    let label = format!("import {} time series", imp.series.len());
    if ed.apply(cmd, &label) {
        if let Some(first) = imp.series.first() {
            crate::swmm_dialogs::open_series(ed, Some(&first.name));
        }
        Some(format!(
            "Imported {} series, {} points{}{}",
            imp.series.len(),
            imp.point_count(),
            if gages.is_empty() {
                String::new()
            } else {
                format!(", {} rain gage(s)", gages.len())
            },
            match &imp.factor {
                Some((_, why)) => format!(" ({why})"),
                None => String::new(),
            }
        ))
    } else {
        d.error = ed.last_error.clone().unwrap_or_default();
        ed.dialogs.rain_import = Some(d);
        None
    }
}

/// Write `series` to `path` as an external time-series file; with
/// `replace`, its rows become one `FILE` line (one undo step).
pub fn export_to(
    ed: &mut SwmmEditor,
    series: &str,
    path: &Path,
    replace: bool,
) -> Result<String, String> {
    let pts = ed.doc.timeseries(series);
    if pts.is_empty() {
        return Err(format!(
            "{series}: no rows to export (a FILE series has none here)"
        ));
    }
    let text = rain::external_file_text(series, &pts);
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    let n = pts.len();
    if replace {
        let file = ed
            .path
            .as_deref()
            .and_then(Path::parent)
            .and_then(|dir| path.strip_prefix(dir).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"));
        if !ed.apply(
            rain::externalize_command(&ed.doc, series, &file),
            &format!("{series}: use file"),
        ) {
            return Err(ed.last_error.clone().unwrap_or_default());
        }
        return Ok(format!(
            "Wrote {n} rows of {series} to {} and pointed the series at it",
            path.display()
        ));
    }
    Ok(format!("Wrote {n} rows of {series} to {}", path.display()))
}

fn window<'a>(
    ctx: &egui::Context,
    title: &'a str,
    size: Vec2,
    resizable: bool,
) -> egui::Window<'a> {
    egui::Window::new(title)
        .id(Id::new(("swmm-dialog", title)))
        .collapsible(false)
        .resizable(resizable)
        .default_size(size)
        .default_pos(ctx.screen_rect().center() - size / 2.0)
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    draw_import(ctx, state);
    draw_export(ctx, state);
}

fn preview(ui: &mut Ui, imp: &Import) {
    ui.label(
        RichText::new(format!(
            "{}: {} series, {} points{}{}",
            match imp.layout {
                rain::Layout::Table => "Table",
                rain::Layout::GhcnDaily => "NOAA GHCN-Daily",
            },
            imp.series.len(),
            imp.point_count(),
            if imp.had_header { ", header row" } else { "" },
            if imp.daily { ", daily totals" } else { "" }
        ))
        .strong(),
    );
    if let Some((f, why)) = &imp.factor {
        ui.label(RichText::new(format!("Values × {f:.6}: {why}")).small());
    }
    for w in &imp.warnings {
        ui.label(
            RichText::new(w)
                .small()
                .color(crate::theme::palette::warning_text(ui.visuals().dark_mode)),
        );
    }
    egui::ScrollArea::vertical()
        .id_salt("swmm-import-preview")
        .max_height(180.0)
        .show(ui, |ui| {
            egui::Grid::new("swmm-import-grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("Series").strong());
                    ui.label(RichText::new("From").strong());
                    ui.label(RichText::new("Points").strong());
                    ui.label(RichText::new("First … last").strong());
                    ui.end_row();
                    for s in &imp.series {
                        ui.label(&s.name);
                        ui.label(RichText::new(&s.source).small());
                        ui.label(s.points.len().to_string());
                        let show = |p: &stormsewer_swmm::doc::SeriesPoint| match &p.date {
                            Some(d) => format!("{d} {} {}", p.time, p.value),
                            None => format!("{} {}", p.time, p.value),
                        };
                        ui.label(
                            RichText::new(match (s.points.first(), s.points.last()) {
                                (Some(a), Some(b)) => format!("{} … {}", show(a), show(b)),
                                _ => "—".into(),
                            })
                            .small(),
                        );
                        ui.end_row();
                    }
                });
        });
}

fn draw_import(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.rain_import.clone() else {
        return;
    };
    let mut open = true;
    let mut action: Option<&str> = None;
    window(ctx, "Import Time Series", Vec2::new(680.0, 520.0), true)
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &state.swmm_doc;
            ui.horizontal(|ui| {
                if ui.button("Load file…").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Text tables", &["csv", "txt", "tsv", "dat"])
                        .add_filter("All files", &["*"])
                        .pick_file()
                    {
                        load_file(&mut d, &p);
                    }
                }
                if let Some(p) = &d.source {
                    ui.label(RichText::new(p.display().to_string()).small());
                }
                ui.label("Depth unit:");
                ui.selectable_value(&mut d.unit, DepthUnit::Inches, "in");
                ui.selectable_value(&mut d.unit, DepthUnit::Millimetres, "mm");
            });
            ui.label(
                RichText::new(
                    "Or paste: date time value…  (one series per value column; a NOAA GHCN-Daily csv with STATION, DATE, PRCP is read as daily depths)",
                )
                .small(),
            );
            ui.add(
                egui::TextEdit::multiline(&mut d.text)
                    .id(Id::new("swmm-import-text"))
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            ensure_parsed(ed, &mut d);
            ui.separator();
            match &d.parsed {
                Some(Ok(imp)) => preview(ui, imp),
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(crate::theme::palette::error_text(ui.visuals().dark_mode)));
                }
                None => {
                    ui.label(RichText::new("Nothing to preview yet.").weak());
                }
            }
            ui.separator();
            ui.checkbox(&mut d.make_gages, "Also add a rain gage per series");
            ui.add_enabled_ui(d.make_gages, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Format");
                    for f in ["INTENSITY", "VOLUME", "CUMULATIVE"] {
                        ui.selectable_value(&mut d.gage_format, f.to_string(), f);
                    }
                    ui.label("Interval");
                    ui.add(
                        egui::TextEdit::singleline(&mut d.gage_interval)
                            .id(Id::new("swmm-import-interval"))
                            .desired_width(50.0),
                    );
                    ui.checkbox(&mut d.place_gages, "place on the map");
                });
            });
            if !d.error.is_empty() {
                ui.label(RichText::new(&d.error).color(crate::theme::palette::error_text(ui.visuals().dark_mode)));
            }
            ui.horizontal(|ui| {
                let ok = matches!(d.parsed, Some(Ok(_)));
                if ui.add_enabled(ok, egui::Button::new("Import")).clicked() {
                    action = Some("ok");
                }
                if ui.button("Cancel").clicked() {
                    action = Some("cancel");
                }
                ui.label(RichText::new("One undo step for everything imported.").small());
            });
        });
    let ed = &mut state.swmm_doc;
    match action {
        Some("ok") => {
            ed.dialogs.rain_import = Some(d);
            if let Some(s) = commit(ed) {
                state.status = s;
            }
        }
        Some("cancel") => ed.dialogs.rain_import = None,
        _ => ed.dialogs.rain_import = if open { Some(d) } else { None },
    }
}

fn draw_export(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.series_export.clone() else {
        return;
    };
    let mut open = true;
    let mut action: Option<&str> = None;
    window(ctx, "Export Time Series to File", Vec2::new(420.0, 200.0), false)
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &state.swmm_doc;
            let names = ed.doc.names("TIMESERIES");
            ui.horizontal(|ui| {
                ui.label("Series");
                egui::ComboBox::from_id_salt("swmm-export-series")
                    .selected_text(d.series.clone())
                    .show_ui(ui, |ui| {
                        for n in &names {
                            ui.selectable_value(&mut d.series, n.clone(), n);
                        }
                    });
                let n = ed.doc.timeseries(&d.series).len();
                ui.label(RichText::new(format!("{n} rows")).small());
            });
            ui.checkbox(
                &mut d.replace_with_file,
                "Replace the rows in the model with a FILE reference",
            )
            .on_hover_text("The engine reads the file at run time; the .inp stays small");
            ui.label(
                RichText::new("Format: `;comment` lines, then `date time value` per line (SWMM external time-series file).")
                    .small(),
            );
            if !d.error.is_empty() {
                ui.label(RichText::new(&d.error).color(crate::theme::palette::error_text(ui.visuals().dark_mode)));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!d.series.is_empty(), egui::Button::new("Save As…"))
                    .clicked()
                {
                    action = Some("save");
                }
                if ui.button("Close").clicked() {
                    action = Some("close");
                }
            });
        });
    let ed = &mut state.swmm_doc;
    match action {
        Some("save") => {
            let mut dialog = rfd::FileDialog::new()
                .add_filter("SWMM time series", &["dat", "txt"])
                .set_file_name(format!("{}.dat", d.series));
            if let Some(dir) = ed.path.as_deref().and_then(Path::parent) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(p) = dialog.save_file() {
                match export_to(ed, &d.series, &p, d.replace_with_file) {
                    Ok(s) => {
                        state.status = s;
                        ed.dialogs.series_export = None;
                        return;
                    }
                    Err(e) => d.error = e,
                }
            }
            ed.dialogs.series_export = Some(d);
        }
        Some("close") => ed.dialogs.series_export = None,
        _ => ed.dialogs.series_export = if open { Some(d) } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> SwmmEditor {
        let mut ed = SwmmEditor::default();
        ed.new_model();
        ed.refresh();
        ed
    }

    #[test]
    fn pasted_multi_column_csv_imports_series_and_gages_in_one_step() {
        let mut ed = blank();
        open_import(&mut ed);
        let d = ed.dialogs.rain_import.as_mut().unwrap();
        assert_eq!(d.unit, DepthUnit::Inches);
        d.text = "Date,Time,North,South\n1/1/2024,0:00,0.1,0.2\n1/1/2024,1:00,0.3,0.4\n".into();
        d.make_gages = true;
        d.gage_format = "INTENSITY".into();
        d.gage_interval = "1:00".into();
        ensure_parsed(&blank(), d);
        let imp = d.parsed.clone().unwrap().unwrap();
        assert_eq!(imp.series.len(), 2);
        let depth = ed.undo_depth();
        let status = commit(&mut ed).unwrap();
        assert!(
            status.contains("2 series, 4 points, 2 rain gage(s)"),
            "{status}"
        );
        assert_eq!(ed.undo_depth(), depth + 1);
        assert_eq!(ed.doc.timeseries("North").len(), 2);
        assert_eq!(ed.doc.timeseries("South")[1].value, "0.4");
        assert_eq!(
            ed.doc.field("RAINGAGES", "RG_North", "Format"),
            Some("INTENSITY")
        );
        assert_eq!(
            ed.doc.field("RAINGAGES", "RG_South", "Series"),
            Some("South")
        );
        assert!(ed.dialogs.rain_import.is_none());
        assert_eq!(
            ed.dialogs
                .series
                .as_ref()
                .and_then(|s| s.name.clone())
                .as_deref(),
            Some("North"),
            "the series dialog opens on the import"
        );
        // Bad text stays in the dialog.
        open_import(&mut ed);
        ed.dialogs.rain_import.as_mut().unwrap().text = "x,y\n1,2\n".into();
        assert!(commit(&mut ed).is_none());
        assert!(!ed.dialogs.rain_import.as_ref().unwrap().error.is_empty());
    }

    #[test]
    fn ghcn_file_loads_as_daily_volume_in_model_units() {
        let dir = std::env::temp_dir()
            .join("stormsewer-app-tests")
            .join("ghcn");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("station.csv");
        std::fs::write(
            &path,
            "\"STATION\",\"NAME\",\"DATE\",\"PRCP\"\n\"USC00311234\",\"SOMEWHERE, NC US\",\"2024-02-01\",\"127\"\n\"USC00311234\",\"SOMEWHERE, NC US\",\"2024-02-02\",\"0\"\n",
        )
        .unwrap();
        let mut ed = blank();
        ed.apply(
            stormsewer_swmm::doc::Command::SetOption {
                section: "OPTIONS".into(),
                key: "FLOW_UNITS".into(),
                value: "CMS".into(),
            },
            "si",
        );
        open_import(&mut ed);
        let d = ed.dialogs.rain_import.as_mut().unwrap();
        assert_eq!(d.unit, DepthUnit::Millimetres);
        load_file(d, &path);
        d.make_gages = true;
        ensure_parsed(&blank(), d);
        assert_eq!(d.gage_interval, "24:00");
        assert_eq!(d.gage_format, "VOLUME");
        let status = commit(&mut ed).unwrap();
        assert!(status.contains("tenths of a millimetre"), "{status}");
        let name = ed
            .doc
            .names("TIMESERIES")
            .into_iter()
            .find(|n| n.contains("USC00311234"))
            .unwrap();
        let pts = ed.doc.timeseries(&name);
        assert_eq!(pts[0].value, "12.7");
        assert_eq!(pts[0].date.as_deref(), Some("02/01/2024"));
        assert_eq!(
            ed.doc.field("RAINGAGES", &format!("RG_{name}"), "Interval"),
            Some("24:00")
        );
    }

    #[test]
    fn export_writes_the_dat_file_and_can_point_the_series_at_it() {
        let dir = std::env::temp_dir()
            .join("stormsewer-app-tests")
            .join("export");
        std::fs::create_dir_all(&dir).unwrap();
        let mut ed = SwmmEditor::default();
        ed.open_text(
            "[TIMESERIES]\nTS1 01/01/2024 0:00 0.5\nTS1 01/01/2024 0:15 1.25\n",
            Some(dir.join("model.inp")),
        );
        ed.refresh();
        open_export(&mut ed, None);
        assert_eq!(ed.dialogs.series_export.as_ref().unwrap().series, "TS1");
        let path = dir.join("TS1.dat");
        let s = export_to(&mut ed, "TS1", &path, false).unwrap();
        assert!(s.contains("2 rows"), "{s}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            ";TS1\n;date time value\n01/01/2024 0:00 0.5\n01/01/2024 0:15 1.25\n"
        );
        let depth = ed.undo_depth();
        export_to(&mut ed, "TS1", &path, true).unwrap();
        assert_eq!(ed.undo_depth(), depth + 1);
        let rows = ed.doc.find_all("TIMESERIES", "TS1");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value(1), Some("FILE"));
        assert_eq!(rows[0].value(2), Some("TS1.dat"), "relative to the model");
        assert!(
            export_to(&mut ed, "TS1", &path, false).is_err(),
            "a FILE series has no rows"
        );
    }
}
