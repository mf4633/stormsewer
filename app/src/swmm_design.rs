// SPDX-License-Identifier: GPL-3.0-or-later

//! Storm-sewer design as a feature of the SWMM model: Tools → Storm Sewer
//! Design maps the open model onto the engine's network (see
//! `stormsewer_swmm::design` for the mapping and its assumptions), runs the
//! Rational / Manning / HGL analysis with a design basis of its own, reviews
//! it against the design criteria, offers auto-sizing as one undoable batch
//! on `[XSECTIONS]`, and writes the same schedules and reports the
//! storm-sewer workspace does — from the SWMM model.
//!
//! The design basis (IDF curve, return period, minimum Tc, junction K,
//! tailwater, minimum slope, submittal block) is a `Project` of its own,
//! seeded from the storm-sewer workspace's project the first time the panel
//! opens and editable in the panel; editing it never touches that project.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Button, RichText, Ui};
use stormsewer::design::{
    design_review, network_inlet_pass_for_project, size_network, DesignCriteria, DesignFinding,
    NetworkInletRow, PipeSizeRecommendation, Severity, SizeOutcome,
};
use stormsewer::io::project::Project;
use stormsewer::network::Analysis;
use stormsewer::params::StormAnalysisParams;
use stormsewer::report_html::{format_analysis_html, HtmlReportMeta};
use stormsewer_swmm::design::{autosize_command, to_project, Mapping, SizeChange};
use stormsewer_swmm::doc::build::ObjRef;
use stormsewer_swmm::doc::{Command, Finding, ObjectKind, Severity as DocSeverity};

use crate::state::AppState;
use crate::theme::palette;

/// One completed design analysis of the open model.
pub struct DesignRun {
    pub mapping: Mapping,
    pub analysis: Analysis,
    pub findings: Vec<DesignFinding>,
    pub recs: Vec<PipeSizeRecommendation>,
    pub inlet_rows: Vec<NetworkInletRow>,
    /// The document generation analysed; the run is stale once it moves.
    pub doc_gen: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DesignTab {
    #[default]
    Pipes,
    Nodes,
    Sizing,
    Inlets,
    Skipped,
    Findings,
    Notes,
}

impl DesignTab {
    const ALL: [DesignTab; 7] = [
        DesignTab::Pipes,
        DesignTab::Nodes,
        DesignTab::Sizing,
        DesignTab::Inlets,
        DesignTab::Skipped,
        DesignTab::Findings,
        DesignTab::Notes,
    ];

    fn label(self) -> &'static str {
        match self {
            DesignTab::Pipes => "Pipes",
            DesignTab::Nodes => "Nodes / HGL",
            DesignTab::Sizing => "Sizing",
            DesignTab::Inlets => "Inlets",
            DesignTab::Skipped => "Not analysed",
            DesignTab::Findings => "Findings",
            DesignTab::Notes => "Assumptions",
        }
    }
}

pub struct SwmmDesignState {
    pub open: bool,
    pub tab: DesignTab,
    /// The design basis; `None` until the panel first opens, when it is
    /// seeded from the storm-sewer workspace's project.
    pub basis: Option<Project>,
    pub run: Option<DesignRun>,
    /// A proposed auto-size batch awaiting the user's Apply.
    pub preview: Option<(Command, Vec<SizeChange>)>,
    pub message: String,
    /// Tailwater checkbox state, so the value survives toggling.
    tailwater_on: bool,
    tailwater_ft: f64,
}

impl Default for SwmmDesignState {
    fn default() -> Self {
        Self {
            open: false,
            tab: DesignTab::Pipes,
            basis: None,
            run: None,
            preview: None,
            message: String::new(),
            tailwater_on: false,
            tailwater_ft: 0.0,
        }
    }
}

impl SwmmDesignState {
    fn stale(&self, gen: u64) -> bool {
        self.run.as_ref().is_some_and(|r| r.doc_gen != gen)
    }
}

/// Seed the basis from the storm-sewer project if it has none yet.
fn ensure_basis(state: &mut AppState) {
    if state.swmm_design.basis.is_none() {
        let mut p = state.project.clone();
        p.nodes.clear();
        p.pipes.clear();
        p.catchments.clear();
        p.background = None;
        p.background_dxf = None;
        state.swmm_design.tailwater_on = p.tailwater.is_some();
        state.swmm_design.tailwater_ft = p.tailwater.unwrap_or(0.0);
        state.swmm_design.basis = Some(p);
    }
}

/// Open the panel (seeding the basis).
pub fn open_panel(state: &mut AppState) {
    ensure_basis(state);
    state.swmm_design.open = true;
}

/// The design findings as editor findings for the findings strip: always
/// warnings (an engine run must not be refused over a surcharged pipe),
/// filed under the object's defining section so a click selects it.
fn strip_findings(state: &AppState, findings: &[DesignFinding]) -> Vec<Finding> {
    let doc = &state.swmm_doc.doc;
    findings
        .iter()
        .map(|f| {
            let section = doc
                .defining_section(ObjectKind::Link, &f.id)
                .or_else(|| doc.defining_section(ObjectKind::Node, &f.id))
                .unwrap_or("CONDUITS");
            let tag = match f.severity {
                Severity::Error => "design error",
                Severity::Warning => "design warning",
            };
            Finding {
                severity: DocSeverity::Warning,
                section: section.to_string(),
                name: f.id.clone(),
                message: format!("[{tag}] {}", f.message),
                column: None,
            }
        })
        .collect()
}

/// Map, analyse, review and size the open model. Returns whether a run
/// exists afterwards; the status is in `swmm_design.message`.
pub fn analyse(state: &mut AppState) -> bool {
    ensure_basis(state);
    state.swmm_doc.refresh();
    if !state.swmm_doc.loaded {
        state.swmm_design.message = "Open or create a SWMM model first.".into();
        return false;
    }
    let basis = state.swmm_design.basis.clone().expect("seeded");
    let mapping = match to_project(&state.swmm_doc.doc, &basis) {
        Ok(m) => m,
        Err(e) => {
            state.swmm_design.message = e;
            state.swmm_design.run = None;
            return false;
        }
    };
    let errors = mapping.project.validate();
    if !errors.is_empty() {
        state.swmm_design.message = format!("Cannot analyse: {}", errors.join("; "));
        state.swmm_design.run = None;
        return false;
    }
    if mapping.project.pipes.is_empty() {
        state.swmm_design.message = format!(
            "No conduit could be analysed ({} listed under Not analysed).",
            mapping.skipped.len()
        );
        state.swmm_design.run = None;
        return false;
    }
    let net = mapping.project.to_analysis_network();
    let analysis = match net.analyze(&mapping.project.idf(), &mapping.project.options()) {
        Ok(a) => a,
        Err(e) => {
            state.swmm_design.message = format!("Analysis failed: {e}");
            state.swmm_design.run = None;
            return false;
        }
    };
    let findings = design_review(&net, &analysis, &state.review_criteria);
    let recs = size_network(&net, &analysis, &DesignCriteria::municipal());
    let inlet_rows = network_inlet_pass_for_project(&mapping.project, &state.inlet_geom);

    // Design findings join the validation findings in the strip until the
    // document changes (the strip rebuilds from validation alone then).
    let strip = strip_findings(state, &findings);
    state
        .swmm_doc
        .findings
        .retain(|f| !f.message.starts_with("[design "));
    state.swmm_doc.findings.extend(strip);

    let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
    let warnings = findings.len() - errors;
    let upsize = recs.iter().filter(|r| r.outcome == SizeOutcome::Sized).count();
    state.swmm_design.message = format!(
        "Analysed {} conduit(s), {} node(s); {} not analysed; review: {errors} error(s), {warnings} warning(s); {upsize} conduit(s) would upsize.",
        mapping.project.pipes.len(),
        mapping.project.nodes.len(),
        mapping.skipped.len()
    );
    state.swmm_design.preview = None;
    state.swmm_design.run = Some(DesignRun {
        mapping,
        analysis,
        findings,
        recs,
        inlet_rows,
        doc_gen: state.swmm_doc.doc.generation(),
    });
    true
}

/// Build the auto-size batch from the last analysis and hold it for the
/// preview. Analyses first when there is no current run.
pub fn preview_autosize(state: &mut AppState) {
    let gen = state.swmm_doc.doc.generation();
    if (state.swmm_design.run.is_none() || state.swmm_design.stale(gen)) && !analyse(state) {
        return;
    }
    let Some(run) = state.swmm_design.run.as_ref() else { return };
    let (cmd, changes) = autosize_command(&state.swmm_doc.doc, &run.recs);
    if changes.is_empty() {
        state.swmm_design.message = "Every analysed conduit already meets the sizing criteria, or has no catalog solution.".into();
        state.swmm_design.preview = None;
        return;
    }
    state.swmm_design.message = format!("{} conduit(s) would change; review and apply.", changes.len());
    state.swmm_design.preview = Some((cmd, changes));
}

/// Apply the previewed batch as one undo step. Returns the number of
/// conduits changed.
pub fn apply_autosize(state: &mut AppState) -> usize {
    let Some((cmd, changes)) = state.swmm_design.preview.take() else {
        return 0;
    };
    let n = changes.len();
    let label = format!("Auto-size {n} conduits");
    if state.swmm_doc.apply(cmd, &label) {
        state.swmm_design.message = format!("{label}: written to [XSECTIONS]; run Analyse to check the result.");
        state.status = label;
        // The analysis no longer matches the document.
        state.swmm_design.run = None;
        n
    } else {
        state.swmm_design.message = state
            .swmm_doc
            .last_error
            .clone()
            .unwrap_or_else(|| "The batch could not be applied.".into());
        0
    }
}

fn params_for(state: &AppState, project: &Project) -> StormAnalysisParams {
    let g = &state.inlet_geom;
    StormAnalysisParams {
        idf: project.idf_set(),
        hydraulics: project.options(),
        sizing: DesignCriteria::municipal(),
        inlet_kind: g.kind,
        inlet_grate_length_ft: g.grate_length_ft,
        inlet_curb_length_ft: g.curb_opening_length_ft,
        inlet_flow_depth_ft: g.sag_ponding_depth_ft,
        inlet_gutter_slope: g.gutter_slope,
    }
}

/// The design report as HTML for the last run.
pub fn design_html(state: &AppState) -> Option<String> {
    let run = state.swmm_design.run.as_ref()?;
    let project = &run.mapping.project;
    let net = project.to_analysis_network();
    let params = params_for(state, project);
    let meta = HtmlReportMeta {
        title: format!("Storm Sewer Design — {}", project.name),
        drawing_name: state.swmm_doc.file_name(),
        generated_utc: crate::files::today_string(),
        project_number: project.report.project_number.clone(),
        engineer: project.report.engineer.clone(),
        firm: project.report.firm.clone(),
        jurisdiction: project.report.jurisdiction.clone(),
    };
    let mut html = format_analysis_html(&net, &run.analysis, &params, &meta);
    // What the engine did not see belongs on the report, not in a status bar.
    if !run.mapping.skipped.is_empty() || !run.mapping.notes.is_empty() {
        let mut extra = String::from("<h2>Mapping from the SWMM model</h2>");
        if !run.mapping.notes.is_empty() {
            extra.push_str("<ul>");
            for n in &run.mapping.notes {
                extra.push_str(&format!("<li>{}</li>", esc(n)));
            }
            extra.push_str("</ul>");
        }
        if !run.mapping.skipped.is_empty() {
            extra.push_str("<h3>Not analysed</h3><table><thead><tr><th>Kind</th><th>Name</th><th>Reason</th></tr></thead><tbody>");
            for s in &run.mapping.skipped {
                extra.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                    esc(&s.kind),
                    esc(&s.name),
                    esc(&s.reason)
                ));
            }
            extra.push_str("</tbody></table>");
        }
        if let Some(i) = html.rfind("<div class=\"disclaimer\">") {
            html.insert_str(i, &extra);
        } else {
            html.push_str(&extra);
        }
    }
    Some(html)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Write the HTML design report to `path`.
pub fn write_design_html(state: &mut AppState, path: &Path) -> Result<(), String> {
    let html = design_html(state).ok_or("Run Analyse first.")?;
    std::fs::write(path, html).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Write the PDF design report (the storm-sewer submittal layout) to `path`.
pub fn write_design_pdf(state: &mut AppState, path: &Path) -> Result<(), String> {
    let run = state.swmm_design.run.as_ref().ok_or("Run Analyse first.")?;
    let mut opts = state.report_options.clone();
    opts.generated_on = crate::files::today_string();
    stormsewer::io::export_pdf_with(
        &run.mapping.project,
        &run.analysis,
        &run.inlet_rows,
        Some(&run.findings),
        &opts,
        path,
    )
}

fn pick_and_write(state: &mut AppState, ext: &str, name: &str, write: fn(&mut AppState, &Path) -> Result<(), String>) {
    if state.swmm_design.run.is_none() && !analyse(state) {
        state.status = state.swmm_design.message.clone();
        return;
    }
    let Some(path) = rfd::FileDialog::new()
        .add_filter(ext.to_ascii_uppercase().as_str(), &[ext])
        .set_file_name(name)
        .save_file()
    else {
        return;
    };
    match write(state, &path) {
        Ok(()) => {
            state.status = format!("Design report saved: {}", path.display());
            if state.open_report_after_export {
                crate::files::open_in_default_viewer(&path);
            }
        }
        Err(e) => state.status = e,
    }
}

fn report_stem(state: &AppState) -> String {
    let stem = PathBuf::from(state.swmm_doc.file_name())
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model")
        .to_string();
    format!("{stem}-design")
}

pub fn save_html_report(state: &mut AppState) {
    let name = format!("{}.html", report_stem(state));
    pick_and_write(state, "html", &name, write_design_html);
}

pub fn save_pdf_report(state: &mut AppState) {
    let name = format!("{}.pdf", report_stem(state));
    pick_and_write(state, "pdf", &name, write_design_pdf);
}

// --- menu -------------------------------------------------------------------

/// The Tools → Storm Sewer Design submenu.
pub fn tools_menu_items(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.swmm_doc.loaded;
    ui.menu_button("Storm Sewer Design", |ui| {
        if ui.add_enabled(loaded, Button::new("Design Panel…")).clicked() {
            open_panel(state);
            ui.close_menu();
        }
        if ui.add_enabled(loaded, Button::new("Analyse")).clicked() {
            open_panel(state);
            analyse(state);
            state.status = state.swmm_design.message.clone();
            ui.close_menu();
        }
        if ui.add_enabled(loaded, Button::new("Auto-size…")).clicked() {
            open_panel(state);
            preview_autosize(state);
            state.status = state.swmm_design.message.clone();
            ui.close_menu();
        }
        if ui.add_enabled(loaded, Button::new("Design Review")).clicked() {
            open_panel(state);
            if analyse(state) {
                state.swmm_design.tab = DesignTab::Findings;
                state.swmm_doc.show_findings = true;
            }
            state.status = state.swmm_design.message.clone();
            ui.close_menu();
        }
        ui.separator();
        if ui
            .add_enabled(loaded, Button::new("Design Report (HTML)…"))
            .clicked()
        {
            save_html_report(state);
            ui.close_menu();
        }
        if ui
            .add_enabled(loaded, Button::new("Design Report (PDF)…"))
            .clicked()
        {
            save_pdf_report(state);
            ui.close_menu();
        }
    });
}

// --- windows ------------------------------------------------------------------

fn f2(v: f64) -> String {
    format!("{v:.2}")
}

fn size_label(p: &stormsewer::io::project::ProjectPipe) -> String {
    match p.shape.as_str() {
        "circular" => format!("{:.0}\"", p.diameter * 12.0),
        other => format!("{other} {:.2}×{:.2} ft", p.rise_ft, p.span_ft),
    }
}

fn basis_editor(ui: &mut Ui, state: &mut AppState) {
    let Some(b) = state.swmm_design.basis.as_mut() else { return };
    egui::Grid::new("swmm-design-basis")
        .num_columns(4)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label("IDF a");
            ui.add(egui::DragValue::new(&mut b.idf_a).speed(0.5).range(0.1..=10000.0));
            ui.label("b");
            ui.add(egui::DragValue::new(&mut b.idf_b).speed(0.1).range(0.0..=200.0));
            ui.end_row();
            ui.label("c");
            ui.add(egui::DragValue::new(&mut b.idf_c).speed(0.01).range(0.1..=2.0));
            ui.label("Return period (yr)");
            ui.add(
                egui::DragValue::new(&mut b.design_return_period_years)
                    .speed(1.0)
                    .range(1.0..=500.0),
            );
            ui.end_row();
            ui.label("Min Tc (min)");
            ui.add(egui::DragValue::new(&mut b.min_tc).speed(0.5).range(1.0..=120.0));
            ui.label("Junction K");
            ui.add(egui::DragValue::new(&mut b.junction_k).speed(0.05).range(0.0..=3.0));
            ui.end_row();
            ui.label("Min slope (ft/ft)");
            ui.add(egui::DragValue::new(&mut b.min_slope).speed(0.0001).range(0.0..=0.1));
            ui.checkbox(&mut state.swmm_design.tailwater_on, "Tailwater (ft)");
            ui.add_enabled(
                state.swmm_design.tailwater_on,
                egui::DragValue::new(&mut state.swmm_design.tailwater_ft).speed(0.1),
            );
            ui.end_row();
        });
    b.tailwater = state
        .swmm_design
        .tailwater_on
        .then_some(state.swmm_design.tailwater_ft);
    let summary = format!(
        "i = {:.1}/(t + {:.1})^{:.2} in/hr, {:.0}-yr",
        b.idf_a, b.idf_b, b.idf_c, b.design_return_period_years
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new(summary).small());
        if ui
            .small_button("Copy from Storm Sewer workspace")
            .on_hover_text("Take the IDF curves, return period and options from the storm-sewer project")
            .clicked()
        {
            state.swmm_design.basis = None;
            ensure_basis(state);
        }
    });
}

fn draw_run(ui: &mut Ui, state: &mut AppState, dark: bool) {
    let Some(run) = state.swmm_design.run.as_ref() else {
        ui.label("Analyse to see pipe hydraulics, HGL, sizing and the design review here.");
        return;
    };
    let project = &run.mapping.project;
    let mut pick: Option<ObjRef> = None;
    ui.horizontal_wrapped(|ui| {
        for t in DesignTab::ALL {
            let n = match t {
                DesignTab::Skipped => run.mapping.skipped.len(),
                DesignTab::Findings => run.findings.len(),
                DesignTab::Notes => run.mapping.notes.len(),
                DesignTab::Sizing => run.recs.iter().filter(|r| r.outcome != SizeOutcome::Adequate).count(),
                DesignTab::Inlets => run.inlet_rows.len(),
                _ => 0,
            };
            let label = if n > 0 { format!("{} ({n})", t.label()) } else { t.label().to_string() };
            ui.selectable_value(&mut state.swmm_design.tab, t, label);
        }
    });
    egui::ScrollArea::both()
        .id_salt("swmm-design-body")
        .max_height(320.0)
        .show(ui, |ui| match state.swmm_design.tab {
            DesignTab::Pipes => {
                egui::Grid::new("swmm-design-pipes").striped(true).show(ui, |ui| {
                    for h in ["Pipe", "From", "To", "Size", "Slope", "Tc min", "i in/hr", "Q cfs", "Cap cfs", "% full", "V ft/s", "HGL up", "HGL dn", "Status"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for pr in &run.analysis.pipes {
                        if ui.selectable_label(false, &pr.id).clicked() {
                            pick = Some(ObjRef::Link(pr.id.clone()));
                        }
                        ui.label(&pr.from);
                        ui.label(&pr.to);
                        ui.label(project.pipes.iter().find(|p| p.id == pr.id).map(size_label).unwrap_or_default());
                        ui.monospace(format!("{:.4}", pr.slope));
                        ui.monospace(format!("{:.1}", pr.tc));
                        ui.monospace(f2(pr.intensity));
                        ui.monospace(f2(pr.design_q));
                        ui.monospace(f2(pr.capacity));
                        ui.monospace(format!("{:.0}", pr.pct_full * 100.0));
                        ui.monospace(f2(pr.velocity));
                        ui.monospace(pr.hgl_up.map(f2).unwrap_or_else(|| "—".into()));
                        ui.monospace(pr.hgl_dn.map(f2).unwrap_or_else(|| "—".into()));
                        let (text, color) = if pr.capacity_unavailable() {
                            (pr.capacity_na_label().to_string(), palette::warning_text(dark))
                        } else if pr.report_surcharged() {
                            ("SURCHARGED".to_string(), palette::error_text(dark))
                        } else {
                            (pr.regime().label().to_string(), palette::ok_text(dark))
                        };
                        ui.label(RichText::new(text).color(color));
                        ui.end_row();
                    }
                });
            }
            DesignTab::Nodes => {
                egui::Grid::new("swmm-design-nodes").striped(true).show(ui, |ui| {
                    for h in ["Node", "Kind", "Area ac", "C", "Tc min", "Invert", "Rim", "HGL", "Freeboard", "Status"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for nr in &run.analysis.nodes {
                        let pn = project.nodes.iter().find(|n| n.id == nr.id);
                        if ui.selectable_label(false, &nr.id).clicked() {
                            pick = Some(ObjRef::Node(nr.id.clone()));
                        }
                        ui.label(pn.map(|n| n.kind.clone()).unwrap_or_default());
                        ui.monospace(pn.map(|n| f2(n.area_ac)).unwrap_or_default());
                        ui.monospace(pn.map(|n| f2(n.c)).unwrap_or_default());
                        ui.monospace(format!("{:.1}", nr.tc));
                        ui.monospace(pn.map(|n| f2(n.invert)).unwrap_or_default());
                        ui.monospace(f2(nr.rim));
                        ui.monospace(f2(nr.hgl));
                        ui.monospace(f2(nr.rim - nr.hgl));
                        if nr.surcharge_to_surface {
                            ui.label(RichText::new("FLOODING").color(palette::error_text(dark)));
                        } else {
                            ui.label(RichText::new("ok").color(palette::ok_text(dark)));
                        }
                        ui.end_row();
                    }
                });
            }
            DesignTab::Sizing => {
                egui::Grid::new("swmm-design-sizing").striped(true).show(ui, |ui| {
                    for h in ["Pipe", "Q cfs", "Slope", "Current", "Recommended", "V ft/s", "% full", "Outcome"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in &run.recs {
                        if ui.selectable_label(false, &r.pipe_id).clicked() {
                            pick = Some(ObjRef::Link(r.pipe_id.clone()));
                        }
                        ui.monospace(f2(r.design_q));
                        ui.monospace(format!("{:.4}", r.slope));
                        ui.monospace(format!("{:.0}\"", r.current_diameter_ft * 12.0));
                        ui.monospace(format!("{:.0}\"", r.recommended_diameter_ft * 12.0));
                        ui.monospace(f2(r.velocity));
                        ui.monospace(format!("{:.0}", r.pct_full * 100.0));
                        let (text, color) = match r.outcome {
                            SizeOutcome::Adequate => ("adequate", palette::ok_text(dark)),
                            SizeOutcome::Sized => ("upsize", palette::warning_text(dark)),
                            SizeOutcome::NoSolution => ("no catalog solution", palette::error_text(dark)),
                        };
                        ui.label(RichText::new(text).color(color)).on_hover_text(&r.note);
                        ui.end_row();
                    }
                });
            }
            DesignTab::Inlets => {
                if run.inlet_rows.is_empty() {
                    ui.label("No inlets: no junction has a subcatchment or an [INLET_USAGE] row.");
                }
                egui::Grid::new("swmm-design-inlets").striped(true).show(ui, |ui| {
                    for h in ["Inlet", "Type", "Local cfs", "Carryover", "Intercepted", "Bypass", "Spread ft", "Status"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in &run.inlet_rows {
                        if ui.selectable_label(false, &r.node_id).clicked() {
                            pick = Some(ObjRef::Node(r.node_id.clone()));
                        }
                        ui.label(r.kind.label());
                        ui.monospace(f2(r.local_cfs));
                        ui.monospace(f2(r.carryover_in_cfs));
                        ui.monospace(f2(r.intercepted_cfs));
                        ui.monospace(f2(r.bypass_cfs));
                        ui.monospace(if r.spread_ft > 0.0 { format!("{:.1}", r.spread_ft) } else { "—".into() });
                        let (text, color) = if !r.ok {
                            ("EXCEEDS", palette::error_text(dark))
                        } else if r.bypass_cfs > 0.005 {
                            ("bypassing", palette::warning_text(dark))
                        } else {
                            ("ok", palette::ok_text(dark))
                        };
                        ui.label(RichText::new(text).color(color));
                        ui.end_row();
                    }
                });
            }
            DesignTab::Skipped => {
                if run.mapping.skipped.is_empty() {
                    ui.label("Every link in the model was analysed.");
                }
                egui::Grid::new("swmm-design-skipped").striped(true).show(ui, |ui| {
                    for h in ["Kind", "Name", "Why"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for s in &run.mapping.skipped {
                        ui.label(&s.kind);
                        if ui.selectable_label(false, &s.name).clicked() {
                            pick = Some(if s.kind == "subcatchment" {
                                ObjRef::Subcatchment(s.name.clone())
                            } else {
                                ObjRef::Link(s.name.clone())
                            });
                        }
                        ui.label(&s.reason);
                        ui.end_row();
                    }
                });
            }
            DesignTab::Findings => {
                if run.findings.is_empty() {
                    ui.label(RichText::new("Design review: no findings.").color(palette::ok_text(dark)));
                }
                for f in &run.findings {
                    let (tag, color) = match f.severity {
                        Severity::Error => ("ERROR", palette::error_text(dark)),
                        Severity::Warning => ("WARN", palette::warning_text(dark)),
                    };
                    if ui
                        .selectable_label(false, RichText::new(format!("[{tag}] {}", f.message)).color(color))
                        .clicked()
                    {
                        let doc = &state.swmm_doc.doc;
                        pick = Some(if doc.defining_section(ObjectKind::Link, &f.id).is_some() {
                            ObjRef::Link(f.id.clone())
                        } else {
                            ObjRef::Node(f.id.clone())
                        });
                    }
                }
            }
            DesignTab::Notes => {
                for n in &run.mapping.notes {
                    ui.label(format!("• {n}"));
                }
                ui.label(format!(
                    "• Flow units {}; {} conduit(s) analysed of {} link(s) in the model.",
                    run.mapping.flow_units,
                    run.mapping.mapped_conduits.len(),
                    run.mapping.mapped_conduits.len() + run.mapping.skipped.iter().filter(|s| s.kind != "subcatchment").count()
                ));
            }
        });
    if let Some(r) = pick {
        state.swmm_doc.select_only(r.clone());
        state.swmm_doc.pending_zoom_to = Some(r);
    }
}

/// The design panel and the auto-size preview.
pub fn draw_windows(ctx: &egui::Context, state: &mut AppState) {
    if state.swmm_design.open {
        ensure_basis(state);
        let dark = ctx.style().visuals.dark_mode;
        let mut open = true;
        egui::Window::new("Storm Sewer Design")
            .open(&mut open)
            .default_width(760.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(RichText::new("Design basis").strong());
                basis_editor(ui, state);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    let loaded = state.swmm_doc.loaded;
                    if ui.add_enabled(loaded, Button::new("Analyse")).clicked() {
                        analyse(state);
                    }
                    if ui.add_enabled(loaded, Button::new("Auto-size…")).clicked() {
                        preview_autosize(state);
                    }
                    let has_run = state.swmm_design.run.is_some();
                    if ui.add_enabled(has_run, Button::new("Report HTML…")).clicked() {
                        save_html_report(state);
                    }
                    if ui.add_enabled(has_run, Button::new("Report PDF…")).clicked() {
                        save_pdf_report(state);
                    }
                    if state.swmm_design.stale(state.swmm_doc.doc.generation()) {
                        ui.label(RichText::new("model edited since — re-run Analyse").color(palette::warning_text(dark)).small());
                    }
                });
                if !state.swmm_design.message.is_empty() {
                    ui.label(RichText::new(state.swmm_design.message.clone()).small());
                }
                ui.separator();
                draw_run(ui, state, dark);
            });
        state.swmm_design.open = open;
    }

    if let Some((_, changes)) = state.swmm_design.preview.as_ref() {
        let changes: Vec<SizeChange> = changes.clone();
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Auto-size preview")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "{} conduit(s) change in [XSECTIONS]; one undo step.",
                    changes.len()
                ));
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    egui::Grid::new("swmm-autosize-preview").striped(true).show(ui, |ui| {
                        for h in ["Conduit", "Shape", "Q cfs", "Before", "After", "Note"] {
                            ui.label(RichText::new(h).strong());
                        }
                        ui.end_row();
                        for c in &changes {
                            ui.label(&c.link);
                            ui.label(&c.shape);
                            ui.monospace(f2(c.design_q));
                            ui.monospace(format!("{:.2} ft ({:.0}\")", c.before_ft, c.before_ft * 12.0));
                            ui.monospace(format!("{:.2} ft ({:.0}\")", c.after_ft, c.after_ft * 12.0));
                            ui.label(&c.note);
                            ui.end_row();
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if ui.button(format!("Apply — Auto-size {} conduits", changes.len())).clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            apply_autosize(state);
        } else if cancel {
            state.swmm_design.preview = None;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;
    use std::path::PathBuf;

    pub(crate) fn fixture_text(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../swmm/tests/fixtures/epa-samples")
            .join(name);
        String::from_utf8(std::fs::read(path).unwrap()).unwrap()
    }

    fn pond_app() -> StormSewerApp {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), None);
        app
    }

    #[test]
    fn analyse_maps_reviews_and_files_findings_without_blocking_runs() {
        let mut app = pond_app();
        assert!(analyse(&mut app.state), "{}", app.state.swmm_design.message);
        let run = app.state.swmm_design.run.as_ref().unwrap();
        assert_eq!(run.analysis.pipes.len(), 4);
        assert_eq!(run.mapping.skipped_of("conduit").count(), 8, "the trapezoids");
        assert!(run.mapping.skipped.iter().any(|s| s.kind == "orifice"));
        assert!(run.mapping.skipped.iter().any(|s| s.kind == "weir"));
        assert!(app.state.swmm_design.basis.is_some());
        // Design findings joined the strip as warnings...
        let strip: Vec<&Finding> = app
            .state
            .swmm_doc
            .findings
            .iter()
            .filter(|f| f.message.starts_with("[design "))
            .collect();
        assert_eq!(strip.len(), run.findings.len());
        assert!(strip.iter().all(|f| f.severity == DocSeverity::Warning));
        assert!(strip.iter().all(|f| f.section == "CONDUITS" || f.section == "JUNCTIONS" || f.section == "STORAGE" || f.section == "OUTFALLS"));
        // ...and an engine run is not refused over them.
        assert!(app.state.swmm_doc.run_path().is_ok());
        // Analysing again does not duplicate them.
        let n = app.state.swmm_doc.findings.len();
        assert!(analyse(&mut app.state));
        assert_eq!(app.state.swmm_doc.findings.len(), n);
        // The HTML report carries the mapping notes and the skipped list.
        let html = design_html(&app.state).unwrap();
        assert!(html.contains("Not analysed") && html.contains("TRAPEZOIDAL"));
        assert!(html.contains("C11"));
        let dir = std::env::temp_dir().join("stormsewer-app-tests");
        std::fs::create_dir_all(&dir).unwrap();
        write_design_html(&mut app.state, &dir.join("swmm-design.html")).unwrap();
        write_design_pdf(&mut app.state, &dir.join("swmm-design.pdf")).unwrap();
        assert!(std::fs::read(dir.join("swmm-design.pdf")).unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn autosize_is_one_undo_step_on_xsections_only() {
        let mut app = pond_app();
        assert!(analyse(&mut app.state));
        // Make the preview certain: pretend every conduit must grow.
        {
            let run = app.state.swmm_design.run.as_mut().unwrap();
            for r in &mut run.recs {
                r.outcome = SizeOutcome::Sized;
                r.recommended_diameter_ft = r.current_diameter_ft + 0.5;
            }
        }
        let before = app.state.swmm_doc.doc.to_string();
        let depth = app.state.swmm_doc.undo_depth();
        preview_autosize(&mut app.state);
        let (_, changes) = app.state.swmm_design.preview.as_ref().unwrap();
        assert_eq!(changes.len(), 4);
        run_frame(&mut app); // the preview window draws
        assert_eq!(apply_autosize(&mut app.state), 4);
        assert_eq!(app.state.swmm_doc.undo_depth(), depth + 1);
        assert_eq!(app.state.swmm_doc.undo_label(), Some("Auto-size 4 conduits"));
        assert!(app.state.swmm_design.preview.is_none());
        assert!(app.state.swmm_design.run.is_none(), "the run is stale after a write");
        let after = app.state.swmm_doc.doc.to_string();
        let changed: Vec<&str> = before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a)
            .collect();
        assert_eq!(changed.len(), 4);
        assert_eq!(app.state.swmm_doc.doc.field("XSECTIONS", "C7", "Geom1"), Some("4"));
        assert!(app.state.swmm_doc.undo().is_some());
        assert_eq!(app.state.swmm_doc.doc.to_string(), before);
        // Nothing to size → no preview.
        assert!(analyse(&mut app.state));
        {
            let run = app.state.swmm_design.run.as_mut().unwrap();
            for r in &mut run.recs {
                r.outcome = SizeOutcome::Adequate;
            }
        }
        preview_autosize(&mut app.state);
        assert!(app.state.swmm_design.preview.is_none());
        assert!(app.state.swmm_design.message.contains("already meets"));
    }

    #[test]
    fn metric_models_are_refused_and_empty_models_say_so() {
        let mut app = pond_app();
        app.state.swmm_doc.open_text("[OPTIONS]\nFLOW_UNITS LPS\n[JUNCTIONS]\nJ1 1\n", None);
        assert!(!analyse(&mut app.state));
        assert!(app.state.swmm_design.message.contains("LPS"));
        app.state.swmm_doc.new_model();
        assert!(!analyse(&mut app.state));
        assert!(app.state.swmm_design.message.contains("no outfall") || app.state.swmm_design.message.contains("No conduit"), "{}", app.state.swmm_design.message);
    }

    #[test]
    fn panel_and_menu_render_every_tab() {
        let mut app = pond_app();
        open_panel(&mut app.state);
        run_frame(&mut app);
        analyse(&mut app.state);
        for t in DesignTab::ALL {
            app.state.swmm_design.tab = t;
            run_frame(&mut app);
        }
        // Edit the model: the panel says the run is stale.
        app.state
            .swmm_doc
            .apply(Command::SetTitle { text: "edited".into() }, "edit title");
        assert!(app.state.swmm_design.stale(app.state.swmm_doc.doc.generation()));
        run_frame(&mut app);
        // The menu draws with a model open.
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                tools_menu_items(ui, &mut state);
            });
        });
    }
}
