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
            self.checkpoint_undo();
            // Fit through the 3-hour row — covers the storm-sewer design range
            // without letting multi-hour depths bias the short-duration fit.
            match self.project.import_noaa_atlas14(&text, 180.0) {
                Ok(n) => {
                    self.status =
                        format!("Imported {n} IDF curves from NOAA Atlas 14: {}", path.display());
                    self.run_analysis();
                    ctx.request_repaint();
                }
                Err(e) => self.status = format!("NOAA import failed: {e}"),
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