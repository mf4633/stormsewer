// SPDX-License-Identifier: GPL-3.0-or-later

//! Native file dialogs for project, DXF, PNG, LandXML, HTML, and PDF I/O.

use std::path::Path;

use eframe::egui;
use eframe::egui::TextureOptions;
use stormsewer::design::design_review;
use stormsewer::io::{
    export_dxf, export_html, export_landxml, export_pdf, import_dxf, import_landxml, import_stm,
    load_template, render_csv, render_html_table, save_template, BackgroundImage, Project,
    ReportTemplate,
};

use crate::state::AppState;

impl AppState {
    pub fn load_background_texture(&mut self, ctx: &egui::Context, path: &str) {
        match image::open(path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.as_flat_samples();
                let color_image =
                    egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
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
            .add_filter("PNG Image", &["png", "PNG"])
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

    pub fn print_report(&mut self) {
        let analysis = match &self.analysis {
            Some(a) => a.clone(),
            None => {
                self.status = "Run analysis before printing".into();
                return;
            }
        };
        let temp = std::env::temp_dir().join("stormsewer-print.pdf");
        let net = self.project.to_network();
        let findings = design_review(&net, &analysis, &self.review_criteria);
        match export_pdf(&self.project, &analysis, &temp, Some(&findings)) {
            Ok(()) => {
                self.status = "Print report generated".into();
                open_in_default_viewer(&temp);
            }
            Err(e) => self.status = e,
        }
    }

    pub fn pick_export_pdf(&mut self) {
        let analysis = match &self.analysis {
            Some(a) => a.clone(),
            None => {
                self.status = "Run analysis before exporting PDF".into();
                return;
            }
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("stormsewer-report.pdf")
            .save_file()
        {
            let net = self.project.to_network();
            let findings = design_review(&net, &analysis, &self.review_criteria);
            match export_pdf(&self.project, &analysis, &path, Some(&findings)) {
                Ok(()) => {
                    self.status = format!("PDF saved: {}", path.display());
                    if self.open_report_after_export {
                        open_in_default_viewer(&path);
                    }
                }
                Err(e) => self.status = e,
            }
        }
    }
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
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
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
                if ui.add_enabled(can, egui::Button::new("Fit & Import")).clicked() {
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