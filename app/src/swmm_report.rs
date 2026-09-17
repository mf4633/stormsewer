// SPDX-License-Identifier: GPL-3.0-or-later

//! Results → Model Report…: one document about the open model and its last
//! run — engine and binary hash, the options that matter, inventory,
//! continuity, every summary table the engine printed, validation
//! findings, a profile table, and the exact `.inp` text — assembled by
//! `stormsewer_swmm::model_report` and written as HTML or PDF.

use std::path::Path;

use eframe::egui::{self, Button, RichText, Ui};
use stormsewer::io::{export_document_pdf, PdfBlock};
use stormsewer_swmm::model_report::{self, Block, ModelReport, ProfileTable, ReportInputs};
use stormsewer_swmm::sha256::sha256_hex;

use crate::state::AppState;

#[derive(Default)]
pub struct SwmmReportState {
    pub open: bool,
    pub report: Option<ModelReport>,
    /// Report title and notes typed by the user; empty means the defaults.
    pub title: String,
    pub notes: String,
    pub message: String,
}

/// The profile view's current table, if a profile is drawn, as the
/// report's station table.
fn profile_table(state: &AppState) -> Option<ProfileTable> {
    let csv = crate::swmm_profile::profile_csv(state)?;
    let mut lines = csv.lines();
    let columns: Vec<String> = lines.next()?.split(',').map(str::to_string).collect();
    let rows: Vec<Vec<String>> = lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(',').map(str::to_string).collect())
        .collect();
    let start = state.swmm.profile.start.clone().unwrap_or_default();
    let end = state.swmm.profile.end.clone().unwrap_or_default();
    Some(ProfileTable { start, end, columns, rows })
}

/// Build the report from the open model and the last run.
pub fn build(state: &mut AppState) -> Result<ModelReport, String> {
    state.swmm_doc.refresh();
    if !state.swmm_doc.loaded {
        return Err("Open a SWMM model first.".into());
    }
    let run = state
        .swmm
        .last_run
        .as_ref()
        .ok_or("Run the model first: the report is about a run.")?;
    let text = state.swmm_doc.doc.to_string();
    let sha = sha256_hex(text.as_bytes());
    let profile = profile_table(state);
    let generated = crate::files::today_string();
    let name = state.swmm_doc.file_name();
    let mut report = model_report::build(&ReportInputs {
        doc: &state.swmm_doc.doc,
        run,
        findings: &state.swmm_doc.findings,
        profile: profile.as_ref(),
        model_name: &name,
        model_sha256: &sha,
        generated: &generated,
    });
    let title = state.swmm_report.title.trim();
    if !title.is_empty() {
        report.title = title.to_string();
    }
    let notes = state.swmm_report.notes.trim();
    if !notes.is_empty() {
        let at = report
            .blocks
            .iter()
            .position(|b| matches!(b, Block::Heading(_)))
            .unwrap_or(0);
        report.blocks.insert(at, Block::Paragraph(notes.to_string()));
    }
    Ok(report)
}

/// Open the window and (re)build the report.
pub fn open(state: &mut AppState) {
    state.swmm_report.open = true;
    match build(state) {
        Ok(r) => {
            state.swmm_report.message = format!("{} section(s)", r.blocks.len());
            state.swmm_report.report = Some(r);
        }
        Err(e) => {
            state.swmm_report.message = e;
            state.swmm_report.report = None;
        }
    }
}

fn to_pdf_blocks(blocks: &[Block]) -> Vec<PdfBlock> {
    blocks
        .iter()
        .map(|b| match b {
            Block::Heading(s) => PdfBlock::Heading(s.clone()),
            Block::Paragraph(s) => PdfBlock::Paragraph(s.clone()),
            Block::KeyValues(kv) => PdfBlock::KeyValues(kv.clone()),
            Block::Table { title, columns, rows, note } => PdfBlock::Table {
                title: title.clone(),
                columns: columns.clone(),
                rows: rows.clone(),
                note: note.clone(),
            },
            Block::List(items) => PdfBlock::List(items.clone()),
            Block::Preformatted(s) => PdfBlock::Mono(s.clone()),
        })
        .collect()
}

pub fn write_html(report: &ModelReport, path: &Path) -> Result<(), String> {
    std::fs::write(path, report.to_html()).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn write_pdf(report: &ModelReport, path: &Path) -> Result<(), String> {
    export_document_pdf(&report.title, &report.subtitle, &to_pdf_blocks(&report.blocks), path)
}

fn stem(state: &AppState) -> String {
    std::path::PathBuf::from(state.swmm_doc.file_name())
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model")
        .to_string()
}

fn save(state: &mut AppState, ext: &str, write: fn(&ModelReport, &Path) -> Result<(), String>) {
    let Some(report) = state.swmm_report.report.clone() else {
        state.swmm_report.message = "Nothing to save yet.".into();
        return;
    };
    let Some(path) = rfd::FileDialog::new()
        .add_filter(ext.to_ascii_uppercase().as_str(), &[ext])
        .set_file_name(format!("{}-report.{ext}", stem(state)))
        .save_file()
    else {
        return;
    };
    match write(&report, &path) {
        Ok(()) => {
            state.status = format!("Model report saved: {}", path.display());
            state.swmm_report.message = state.status.clone();
            if state.open_report_after_export {
                crate::files::open_in_default_viewer(&path);
            }
        }
        Err(e) => {
            state.status = e.clone();
            state.swmm_report.message = e;
        }
    }
}

/// Results → Model Report… menu item.
pub fn results_menu_item(ui: &mut Ui, state: &mut AppState) {
    if ui
        .add_enabled(state.swmm.last_run.is_some(), Button::new("Model Report…"))
        .on_disabled_hover_text("Run the model first")
        .clicked()
    {
        open(state);
        ui.close_menu();
    }
}

fn draw_block(ui: &mut Ui, b: &Block) {
    match b {
        Block::Heading(s) => {
            ui.add_space(6.0);
            ui.label(RichText::new(s).strong().size(15.0));
        }
        Block::Paragraph(s) => {
            ui.label(s);
        }
        Block::KeyValues(kv) => {
            egui::Grid::new(ui.next_auto_id()).num_columns(2).show(ui, |ui| {
                for (k, v) in kv {
                    ui.label(RichText::new(k).strong());
                    ui.label(v);
                    ui.end_row();
                }
            });
        }
        Block::Table { title, columns, rows, note } => {
            ui.label(RichText::new(title).strong());
            egui::ScrollArea::horizontal().id_salt(ui.next_auto_id()).show(ui, |ui| {
                egui::Grid::new(ui.next_auto_id()).striped(true).show(ui, |ui| {
                    for c in columns {
                        ui.label(RichText::new(c).strong().small());
                    }
                    ui.end_row();
                    for r in rows.iter().take(60) {
                        for v in r {
                            ui.monospace(RichText::new(v).small());
                        }
                        ui.end_row();
                    }
                });
            });
            if rows.len() > 60 {
                ui.label(RichText::new(format!("… {} more row(s) in the saved report", rows.len() - 60)).small());
            }
            if let Some(n) = note {
                ui.label(RichText::new(n).small().italics());
            }
        }
        Block::List(items) => {
            for (tag, text) in items {
                ui.label(format!("[{tag}] {text}"));
            }
        }
        Block::Preformatted(s) => {
            let shown: String = s.lines().take(40).collect::<Vec<_>>().join("\n");
            ui.monospace(RichText::new(shown).small());
            if s.lines().count() > 40 {
                ui.label(RichText::new("… full text in the saved report").small());
            }
        }
    }
}

pub fn draw_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_report.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Model Report")
        .open(&mut open)
        .default_width(720.0)
        .default_height(560.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Title");
                ui.add(egui::TextEdit::singleline(&mut state.swmm_report.title).hint_text("Model Report — <model>").desired_width(260.0));
                if ui.button("Rebuild").clicked() {
                    self::open(state);
                }
                let has = state.swmm_report.report.is_some();
                if ui.add_enabled(has, Button::new("Save HTML…")).clicked() {
                    save(state, "html", write_html);
                }
                if ui.add_enabled(has, Button::new("Save PDF…")).clicked() {
                    save(state, "pdf", write_pdf);
                }
                ui.checkbox(&mut state.open_report_after_export, "Open after saving");
            });
            ui.horizontal(|ui| {
                ui.label("Notes");
                ui.add(egui::TextEdit::multiline(&mut state.swmm_report.notes).desired_rows(2).desired_width(f32::INFINITY));
            });
            if !state.swmm_report.message.is_empty() {
                ui.label(RichText::new(state.swmm_report.message.clone()).small());
            }
            ui.separator();
            if let Some(report) = state.swmm_report.report.as_ref() {
                ui.label(RichText::new(&report.title).strong().size(18.0));
                ui.label(RichText::new(&report.subtitle).small());
                egui::ScrollArea::vertical().id_salt("swmm-report-body").show(ui, |ui| {
                    for b in &report.blocks {
                        draw_block(ui, b);
                    }
                });
            } else {
                ui.label("Run the model, then rebuild.");
            }
        });
    state.swmm_report.open = open;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;
    use std::path::PathBuf;
    use stormsewer_swmm::engine::Run;
    use stormsewer_swmm::rpt;

    /// A run record pointing at the real fixture results.
    pub(crate) fn fixture_run() -> Run {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../swmm/tests/fixtures");
        let rpt_path = dir.join("results/Detention_Pond_Model.rpt");
        Run {
            engine_id: "5.2.4".into(),
            engine_version: "5.2.4".into(),
            engine_sha256: "deadbeef".repeat(8),
            inp: dir.join("epa-samples/Detention_Pond_Model.inp"),
            rpt: rpt_path.clone(),
            out: dir.join("results/Detention_Pond_Model.out"),
            exit_code: Some(0),
            elapsed: std::time::Duration::from_millis(1234),
            stdout: String::new(),
            stderr: String::new(),
            report: rpt::read(&rpt_path).unwrap(),
        }
    }

    #[test]
    fn report_needs_a_run_then_carries_hash_tables_and_profile() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), None);
        assert!(build(&mut app.state).is_err(), "no run yet");
        open(&mut app.state);
        assert!(app.state.swmm_report.report.is_none());
        run_frame(&mut app);

        app.state.swmm.last_run = Some(fixture_run());
        app.state.swmm_report.title = "Pond QA".into();
        app.state.swmm_report.notes = "Reviewed for the record.".into();
        // A profile for the station table.
        let inp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../swmm/tests/fixtures/epa-samples/Detention_Pond_Model.inp");
        app.state.swmm.profile.ensure_network(Some(&inp));
        app.state.swmm.profile.start = Some("J1".into());
        app.state.swmm.profile.end = Some("O2".into());
        app.state.swmm.profile.ensure_profile();
        open(&mut app.state);
        let report = app.state.swmm_report.report.clone().unwrap();
        assert_eq!(report.title, "Pond QA");
        let html = report.to_html();
        assert!(html.contains(&"deadbeef".repeat(8)));
        assert!(html.contains("Reviewed for the record."));
        let sha = sha256_hex(app.state.swmm_doc.doc.to_string().as_bytes());
        assert!(html.contains(&sha));
        for t in &fixture_run().report.tables {
            assert!(html.contains(&t.title), "{}", t.title);
        }
        assert!(html.contains("J1") && html.contains("O2"));
        assert!(html.contains("[TITLE]"), "the appendix carries the input");
        run_frame(&mut app);

        let dir = std::env::temp_dir().join("stormsewer-app-tests");
        std::fs::create_dir_all(&dir).unwrap();
        write_html(&report, &dir.join("swmm-model-report.html")).unwrap();
        write_pdf(&report, &dir.join("swmm-model-report.pdf")).unwrap();
        assert!(std::fs::read(dir.join("swmm-model-report.pdf")).unwrap().starts_with(b"%PDF"));

        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| results_menu_item(ui, &mut state));
        });
    }
}
