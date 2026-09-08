// SPDX-License-Identifier: GPL-3.0-or-later

//! Native file dialogs for project, DXF, PNG, LandXML, HTML, and PDF I/O.

use std::path::Path;

use eframe::egui;
use eframe::egui::TextureOptions;
use stormsewer::design::design_review;
use stormsewer::io::{
    export_dxf, export_html, export_landxml, export_pdf_with, import_dxf, import_landxml,
    import_stm, load_template, render_csv, render_html_table, save_template, BackgroundImage,
    Project, ReportTemplate,
};

use crate::state::AppState;

impl AppState {
    pub fn load_background_texture(&mut self, ctx: &egui::Context, path: &str) {
        match image::open(path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.as_flat_samples();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
                self.bg_texture = Some(ctx.load_texture(
                    format!("bg-{path}"),
                    color_image,
                    TextureOptions::LINEAR,
                ));
                self.status = format!("Background loaded: {path}");
            }
            Err(e) => self.status = format!("Cannot load image: {e}"),
        }
    }

    pub fn open_project_path(&mut self, ctx: &egui::Context, path: std::path::PathBuf) {
        match Project::load(&path) {
            Ok(p) => {
                self.load_project(p, Some(path));
                if let Some(bg) = self.project.background.clone() {
                    self.load_background_texture(ctx, &bg.path);
                }
                self.status = "Project opened".into();
            }
            Err(e) => self.status = e,
        }
    }

    /// Open any file StormSewer understands, chosen by extension: `.ssproj`
    /// projects, Hydraflow / Civil 3D `.stm`, LandXML `.xml`, and `.dxf`
    /// (a network exported by this app, or otherwise a site underlay). Used
    /// by the command-line argument and file association; the menu items
    /// keep their own typed pickers.
    pub fn open_any_path(&mut self, ctx: &egui::Context, path: std::path::PathBuf) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let imported = match ext.as_str() {
            "ssproj" => {
                self.open_project_path(ctx, path);
                return;
            }
            "stm" => import_stm(&path).map(|p| (p, "Imported STM")),
            "xml" => import_landxml(&path).map(|p| (p, "Imported LandXML")),
            "dxf" => match import_dxf(&path) {
                Ok(p) => Ok((p, "Imported DXF network")),
                // Not one of ours: bring it in as the site underlay instead.
                // On a fresh launch (untouched demo/empty project) start from
                // an empty project so the drawing, not the demo, is what
                // the user sees and draws on.
                Err(_) => {
                    if !self.project_dirty && self.project_path.is_none() {
                        let mut empty = Project::empty();
                        empty.name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Site")
                            .to_string();
                        self.load_project(empty, None);
                        // Project::empty() seeds an "OUT" outfall; a site
                        // drawing has no network yet.
                        self.project.nodes.clear();
                        self.project.pipes.clear();
                        self.run_analysis();
                    }
                    self.set_background_dxf(path.clone());
                    self.pending_zoom_fit = true;
                    self.status = format!("DXF underlay: {}", path.display());
                    ctx.request_repaint();
                    return;
                }
            },
            _ => Err(format!(
                "cannot open {}: not a .ssproj, .stm, .xml, or .dxf",
                path.display()
            )),
        };
        match imported {
            Ok((p, what)) => {
                self.bg_texture = None;
                self.load_project(p, None);
                self.status = format!("{what}: {}", path.display());
                ctx.request_repaint();
            }
            Err(e) => self.status = e,
        }
    }

    /// Attach a reference DXF as the plan underlay (bounds from its geometry).
    pub fn set_background_dxf(&mut self, path: std::path::PathBuf) {
        let mut bg = stormsewer::io::BackgroundDxf {
            path: path.display().to_string(),
            min_x: 0.0,
            min_y: 0.0,
            max_x: 1000.0,
            max_y: 1000.0,
            opacity: 0.45,
        };
        if let Ok(segs) = stormsewer::io::import_dxf_underlay(&path) {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for s in &segs {
                for (x, y) in [(s.x1, s.y1), (s.x2, s.y2)] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
            if x0 < x1 && y0 < y1 {
                bg.min_x = x0;
                bg.min_y = y0;
                bg.max_x = x1;
                bg.max_y = y1;
            }
        }
        self.checkpoint_undo();
        self.project.background_dxf = Some(bg);
        self.project_dirty = true;
        self.reload_dxf_underlay();
    }

    pub fn pick_open_project(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("StormSewer Project", &["ssproj"])
            .pick_file()
        {
            self.open_project_path(ctx, path);
        }
    }

    pub fn pick_save_project(&mut self) {
        let mut dlg = rfd::FileDialog::new().add_filter("StormSewer Project", &["ssproj"]);
        if let Some(p) = &self.project_path {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                dlg = dlg.set_file_name(name);
            }
        }
        let path = dlg.save_file().or_else(|| self.project_path.clone());
        if let Some(path) = path {
            match self.project.save(&path) {
                Ok(()) => {
                    self.project_path = Some(path.clone());
                    self.recent.push(path);
                    self.mark_project_saved();
                    self.status = "Project saved".into();
                }
                Err(e) => self.status = e,
            }
        }
    }

    pub fn pick_background(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Image", &["png", "PNG", "jpg", "jpeg", "JPG", "JPEG"])
            .pick_file()
        {
            self.checkpoint_undo();
            let path_str = path.display().to_string();
            self.project.background = Some(BackgroundImage {
                path: path_str.clone(),
                origin_x: 0.0,
                origin_y: 0.0,
                width: 800.0,
                opacity: 0.65,
            });
            self.load_background_texture(ctx, &path_str);
        }
    }

    pub fn pick_import_dxf(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf", "DXF"])
            .pick_file()
        {
            match import_dxf(&path) {
                Ok(p) => {
                    self.load_project(p, None);
                    self.status = format!("Imported {}", path.display());
                    ctx.request_repaint();
                }
                Err(e) => self.status = e,
            }
        }
    }

    pub fn pick_export_dxf(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf"])
            .set_file_name("network.dxf")
            .save_file()
        {
            match export_dxf(&self.project, &path) {
                Ok(()) => self.status = format!("Exported {}", path.display()),
                Err(e) => self.status = e,
            }
        }
    }

    pub fn pick_import_stm(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Hydraflow STM", &["stm", "STM"])
            .pick_file()
        {
            match import_stm(&path) {
                Ok(p) => {
                    self.load_project(p, None);
                    self.status = format!("Imported STM: {}", path.display());
                    ctx.request_repaint();
                }
                Err(e) => self.status = e,
            }
        }
    }

    pub fn pick_import_noaa(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("NOAA Atlas 14 CSV", &["csv", "CSV"])
            .pick_file()
        {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    self.status = format!("Read failed: {e}");
                    return;
                }
            };
            self.import_noaa_text(&text, Some(&path.display().to_string()));
            ctx.request_repaint();
        }
    }

    /// Fit and apply NOAA Atlas 14 IDF curves from raw CSV text (shared by the
    /// file picker and the paste dialog). `source` is an optional label for the
    /// status line. Returns true on success.
    pub fn import_noaa_text(&mut self, text: &str, source: Option<&str>) -> bool {
        self.checkpoint_undo();
        // Fit through the 3-hour row — covers the storm-sewer design range
        // without letting multi-hour depths bias the short-duration fit.
        match self.project.import_noaa_atlas14(text, 180.0) {
            Ok(n) => {
                self.status = match source {
                    Some(s) => format!("Imported {n} IDF curves from NOAA Atlas 14: {s}"),
                    None => format!("Imported {n} IDF curves from pasted NOAA Atlas 14 data"),
                };
                self.run_analysis();
                true
            }
            Err(e) => {
                self.status = format!("NOAA import failed: {e}");
                false
            }
        }
    }

    pub fn pick_import_landxml(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("LandXML", &["xml", "XML"])
            .pick_file()
        {
            match import_landxml(&path) {
                Ok(p) => {
                    self.bg_texture = None;
                    self.load_project(p, None);
                    self.status = format!("Imported LandXML: {}", path.display());
                    ctx.request_repaint();
                }
                Err(e) => self.status = e,
            }
        }
    }

    pub fn pick_export_landxml(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("LandXML", &["xml"])
            .set_file_name("network.xml")
            .save_file()
        {
            match export_landxml(&self.project, &path) {
                Ok(()) => self.status = format!("LandXML exported: {}", path.display()),
                Err(e) => self.status = e,
            }
        }
    }

    pub fn pick_export_custom_csv(&mut self) {
        let analysis = match &self.analysis {
            Some(a) => a.clone(),
            None => {
                self.status = "Run analysis before custom report export".into();
                return;
            }
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV Report", &["csv"])
            .set_file_name("stormsewer-custom-report.csv")
            .save_file()
        {
            let csv = render_csv(&self.project, &analysis, &self.report_template);
            match std::fs::write(&path, csv) {
                Ok(()) => self.status = format!("Custom CSV saved: {}", path.display()),
                Err(e) => self.status = format!("Write failed: {e}"),
            }
        }
    }

    pub fn pick_export_custom_html(&mut self) {
        let analysis = match &self.analysis {
            Some(a) => a.clone(),
            None => {
                self.status = "Run analysis before custom report export".into();
                return;
            }
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("HTML Report", &["html", "htm"])
            .set_file_name("stormsewer-custom-report.html")
            .save_file()
        {
            let html = render_html_table(&self.project, &analysis, &self.report_template);
            match std::fs::write(&path, html) {
                Ok(()) => {
                    self.status = format!("Custom HTML saved: {}", path.display());
                    if self.open_report_after_export {
                        open_in_default_viewer(&path);
                    }
                }
                Err(e) => self.status = format!("Write failed: {e}"),
            }
        }
    }

    pub fn pick_load_report_template(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("StormSewer Report Template", &["srpt"])
            .pick_file()
        {
            match load_template(&path) {
                Ok(t) => {
                    self.report_template = t;
                    self.status = "Report template loaded".into();
                }
                Err(e) => self.status = e,
            }
        }
    }

    pub fn pick_save_report_template(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("StormSewer Report Template", &["srpt"])
            .set_file_name("my-report.srpt")
            .save_file()
        {
            match save_template(&self.report_template, &path) {
                Ok(()) => self.status = format!("Template saved: {}", path.display()),
                Err(e) => self.status = e,
            }
        }
    }

    pub fn set_report_template(&mut self, template: ReportTemplate) {
        self.report_template = template;
        self.status = format!("Report template: {}", self.report_template.name);
    }

    pub fn pick_export_html(&mut self) {
        let analysis = match &self.analysis {
            Some(a) => a.clone(),
            None => {
                self.status = "Run analysis before exporting HTML report".into();
                return;
            }
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("HTML Report", &["html", "htm"])
            .set_file_name("stormsewer-report.html")
            .save_file()
        {
            match export_html(&self.project, &analysis, &path) {
                Ok(()) => {
                    self.status = format!("HTML report saved: {}", path.display());
                    if self.open_report_after_export {
                        open_in_default_viewer(&path);
                    }
                }
                Err(e) => self.status = e,
            }
        }
    }

    /// Open the report options dialog (sections, title block, preview/save).
    /// Both File → Export PDF Report and Print (Ctrl+P) route through it.
    pub fn open_report_options(&mut self) {
        if self.analysis.is_none() {
            self.status = "Run analysis before exporting a report".into();
            return;
        }
        self.report_options_open = true;
    }

    /// Write the PDF report to `path` with the session's report options.
    /// Returns true on success.
    pub fn write_pdf_report(&mut self, path: &Path) -> bool {
        let analysis = match &self.analysis {
            Some(a) => a.clone(),
            None => {
                self.status = "Run analysis before exporting a report".into();
                return false;
            }
        };
        let net = self.project.to_network();
        let findings = design_review(&net, &analysis, &self.review_criteria);
        let mut opts = self.report_options.clone();
        opts.generated_on = today_string();
        match export_pdf_with(
            &self.project,
            &analysis,
            &self.inlet_rows,
            Some(&findings),
            &opts,
            path,
        ) {
            Ok(()) => true,
            Err(e) => {
                self.status = e;
                false
            }
        }
    }

    /// Render the report to a temp file and open it in the PDF viewer.
    pub fn report_preview(&mut self) {
        let temp = std::env::temp_dir().join("stormsewer-preview.pdf");
        if self.write_pdf_report(&temp) {
            self.status = "Report preview opened".into();
            open_in_default_viewer(&temp);
        }
    }

    /// Ask where to save, then write the report. Returns true when saved.
    pub fn report_save_pdf(&mut self) -> bool {
        let mut name = self
            .project
            .name
            .trim()
            .to_lowercase()
            .replace(|c: char| !c.is_ascii_alphanumeric(), "-");
        while name.contains("--") {
            name = name.replace("--", "-");
        }
        let name = name.trim_matches('-');
        let file_name = if name.is_empty() {
            "stormsewer-report.pdf".to_string()
        } else {
            format!("{name}-report.pdf")
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name(&file_name)
            .save_file()
        else {
            return false;
        };
        if self.write_pdf_report(&path) {
            self.status = format!("PDF saved: {}", path.display());
            if self.open_report_after_export {
                open_in_default_viewer(&path);
            }
            true
        } else {
            false
        }
    }
}

/// Today's date as "Month D, YYYY" from the system clock (civil-from-days,
/// Gregorian; no external time crate needed).
fn today_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{} {}, {}", MONTHS[(m - 1) as usize], d, y)
}

/// Modal dialog: choose report sections and title-block fields, then
/// preview or save the PDF report.
pub fn draw_report_options_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.report_options_open {
        return;
    }
    let edit_snapshot = state.project.clone();
    let mut open = state.report_options_open;
    let mut do_preview = false;
    let mut do_save = false;
    let mut close = false;
    egui::Window::new("Report Options")
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.strong("Sections");
            let o = &mut state.report_options;
            ui.checkbox(&mut o.include_summary, "Design basis & summary");
            ui.checkbox(&mut o.include_review, "Design review findings");
            ui.checkbox(&mut o.include_plan, "Plan schematic");
            ui.checkbox(&mut o.include_pipe_table, "Pipe schedule");
            ui.checkbox(&mut o.include_structure_table, "Structure schedule");
            ui.checkbox(&mut o.include_inlet_table, "Inlet schedule (HEC-22)");
            ui.checkbox(&mut o.include_profile, "Profile");
            ui.add_space(6.0);
            ui.strong("Title block");
            egui::Grid::new("report-title-block")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Project");
                    ui.text_edit_singleline(&mut state.project.name);
                    ui.end_row();
                    ui.label("Project no.");
                    ui.text_edit_singleline(&mut state.project.report.project_number);
                    ui.end_row();
                    ui.label("Engineer");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.project.report.engineer)
                            .id(egui::Id::new("report-engineer")),
                    );
                    ui.end_row();
                    ui.label("Firm");
                    ui.text_edit_singleline(&mut state.project.report.firm);
                    ui.end_row();
                    ui.label("Jurisdiction");
                    ui.text_edit_singleline(&mut state.project.report.jurisdiction);
                    ui.end_row();
                });
            ui.add_space(8.0);
            let ready = state.analysis.is_some();
            if !ready {
                ui.label("Run analysis first — the report needs results.");
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(ready, egui::Button::new("Preview"))
                    .clicked()
                {
                    do_preview = true;
                }
                if ui
                    .add_enabled(ready, egui::Button::new("Save PDF…"))
                    .clicked()
                {
                    do_save = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });

    if state.project != edit_snapshot {
        let gesture_active =
            ctx.input(|inp| inp.pointer.any_down()) || ctx.memory(|m| m.focused().is_some());
        if !state.undo_gesture_active {
            state.undo.record_previous(edit_snapshot);
        }
        state.undo_gesture_active = gesture_active;
        state.mark_project_dirty();
    }
    if do_preview {
        state.report_preview();
    }
    if do_save && state.report_save_pdf() {
        close = true;
    }
    if close {
        open = false;
    }
    state.report_options_open = open;
}

/// Modal dialog to paste NOAA Atlas 14 PFDS CSV text and fit IDF curves from it.
pub fn draw_noaa_paste_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.noaa_paste_open {
        return;
    }
    let mut open = state.noaa_paste_open;
    let mut do_import = false;
    let mut close = false;
    egui::Window::new("Import NOAA Atlas 14 IDF")
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                "Paste the NOAA Atlas 14 PFDS precipitation-frequency CSV (English \"depth\" \
                 export, inches). StormSewer fits a/(t+b)^c coefficients for every return period.",
            );
            ui.hyperlink_to(
                "Get data → NOAA PFDS (hdsc.nws.noaa.gov/pfds)",
                "https://hdsc.nws.noaa.gov/pfds/",
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut state.noaa_paste_text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(12)
                            .code_editor()
                            .hint_text(
                                "by duration for ARI (years):,1,2,5,10,25,50,100\n\
                             5-min:,0.276,0.330,0.410,0.475,0.564,0.635,0.708\n\
                             10-min:,...",
                            ),
                    );
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let can = !state.noaa_paste_text.trim().is_empty();
                if ui
                    .add_enabled(can, egui::Button::new("Fit & Import"))
                    .clicked()
                {
                    do_import = true;
                }
                if ui.button("Clear").clicked() {
                    state.noaa_paste_text.clear();
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });

    if do_import {
        let text = state.noaa_paste_text.clone();
        if state.import_noaa_text(&text, None) {
            close = true;
        }
    }
    if close {
        open = false;
    }
    state.noaa_paste_open = open;
}

fn open_in_default_viewer(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.display().to_string()])
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}
