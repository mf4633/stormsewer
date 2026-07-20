// SPDX-License-Identifier: GPL-3.0-or-later

//! Application state: project, analysis results, viewport, and editing session.

use std::path::PathBuf;

use eframe::egui::TextureHandle;
use stormsewer::design::cost::{default_cost_table, estimate_network_cost, format_cost_summary};
use stormsewer::design::criteria::DesignCriteria;
use stormsewer::design::review::{design_review, DesignFinding, ReviewCriteria, Severity};
use stormsewer::design::sizing::{apply_sizing_to_network, recommend_all_pipes, PipeSizeRecommendation};
use stormsewer::diagnostics::{format_diagnostics, run_diagnostics};
use stormsewer::io::{import_dxf_underlay, DxfUnderlaySegment, Project, ReportTemplate};
use stormsewer::network::Analysis;
use stormsewer::report::format_analysis;
use stormsewer::units::convert_project;

use stormsewer::design::inlets::InletGeometry;

use crate::edit::{EditState, Tool};
use crate::panels::SideTab;
use crate::prefs::AppPrefs;
use crate::recent::RecentFiles;
use crate::undo::UndoStack;
use crate::viewport::Viewport;

/// Active central-panel view tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewTab {
    #[default]
    Plan,
    Profile,
}

/// Full StormSewer desktop application state.
pub struct AppState {
    pub project: Project,
    pub project_path: Option<PathBuf>,
    pub analysis: Option<Analysis>,
    pub report_text: String,
    pub multi_rp_text: String,
    pub show_multi_rp: bool,
    pub review_text: String,
    pub status: String,
    pub viewport: Viewport,
    pub view_tab: ViewTab,
    pub edit: EditState,
    pub tool: Tool,
    pub bg_texture: Option<TextureHandle>,
    pub sizing_text: String,
    pub findings: Vec<DesignFinding>,
    pub selected_node: Option<usize>,
    pub selected_pipe: Option<usize>,
    pub dragging_node: Option<usize>,
    pub side_tab: SideTab,
    pub inspector_open: bool,
    pub help: crate::help::HelpState,
    pub undo: UndoStack,
    pub recent: RecentFiles,
    pub review_criteria: ReviewCriteria,
    pub inlet_geom: InletGeometry,
    pub inlet_check_text: String,
    /// Open PDF/HTML in the default viewer after export.
    pub open_report_after_export: bool,
    pub cost_text: String,
    pub diagnostics_text: String,
    pub tc_calc: crate::tc_calc::TcCalcState,
    pub show_global_edit: bool,
    pub global_edit_n: f64,
    pub global_edit_dia_in: f64,
    pub report_template: ReportTemplate,
    pub show_report_editor: bool,
    pub dxf_underlay: Vec<DxfUnderlaySegment>,
    /// True when project parameters changed since the last successful analysis.
    pub analysis_stale: bool,
    /// Set by review navigation; main loop zooms to the current selection once.
    pub pending_zoom_selection: bool,
    pub project_dirty: bool,
    pub prefs: AppPrefs,
    pub tutorial: crate::tutorial::TutorialState,
    /// NOAA Atlas 14 paste-import dialog: open flag and pasted CSV text.
    pub noaa_paste_open: bool,
    pub noaa_paste_text: String,
}

impl AppState {
    /// Switch the active editing tool, keeping [`EditState::tool`] in sync and
    /// cancelling any pipe-in-progress when leaving the pipe tool.
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.edit.tool = tool;
        if tool != Tool::DrawPipe {
            self.edit.pipe_from = None;
        }
    }

    /// Load the built-in demo project and run an initial analysis.
    pub fn new_demo() -> Self {
        let project = Project::demo();
        let edit = init_edit_counters(&project);
        let mut state = Self {
            project,
            project_path: None,
            analysis: None,
            report_text: "Click Analyze to run hydraulic calculations.".into(),
            multi_rp_text: String::new(),
            show_multi_rp: false,
            review_text: String::new(),
            status: "Ready — demo project loaded".into(),
            viewport: Viewport::default(),
            view_tab: ViewTab::Plan,
            edit,
            tool: Tool::Select,
            bg_texture: None,
            sizing_text: String::new(),
            findings: Vec::new(),
            selected_node: None,
            selected_pipe: None,
            dragging_node: None,
            side_tab: SideTab::Parameters,
            inspector_open: true,
            help: crate::help::HelpState::default(),
            undo: UndoStack::default(),
            recent: RecentFiles::load(),
            review_criteria: ReviewCriteria::default(),
            inlet_geom: InletGeometry::default(),
            inlet_check_text: String::new(),
            open_report_after_export: true,
            cost_text: String::new(),
            diagnostics_text: String::new(),
            tc_calc: crate::tc_calc::TcCalcState::default(),
            show_global_edit: false,
            global_edit_n: 0.013,
            global_edit_dia_in: 18.0,
            report_template: ReportTemplate::default(),
            show_report_editor: false,
            dxf_underlay: Vec::new(),
            analysis_stale: false,
            pending_zoom_selection: false,
            project_dirty: false,
            prefs: AppPrefs::load(),
            tutorial: crate::tutorial::TutorialState::default(),
            noaa_paste_open: false,
            noaa_paste_text: String::new(),
        };
        state.run_analysis();
        state.update_inlet_check();
        state.update_cost();
        state.update_diagnostics();
        state
    }

    /// Load a blank project with default parameters and no network elements.
    pub fn new_empty() -> Self {
        let project = Project::empty();
        let edit = init_edit_counters(&project);
        Self {
            project,
            project_path: None,
            analysis: None,
            report_text: "Click Analyze to run hydraulic calculations.".into(),
            multi_rp_text: String::new(),
            show_multi_rp: false,
            review_text: String::new(),
            status: "Ready — new empty project".into(),
            viewport: Viewport::default(),
            view_tab: ViewTab::Plan,
            edit,
            tool: Tool::Select,
            bg_texture: None,
            sizing_text: String::new(),
            findings: Vec::new(),
            selected_node: None,
            selected_pipe: None,
            dragging_node: None,
            side_tab: SideTab::Parameters,
            inspector_open: true,
            help: crate::help::HelpState::default(),
            undo: UndoStack::default(),
            recent: RecentFiles::load(),
            review_criteria: ReviewCriteria::default(),
            inlet_geom: InletGeometry::default(),
            inlet_check_text: String::new(),
            open_report_after_export: true,
            cost_text: String::new(),
            diagnostics_text: String::new(),
            tc_calc: crate::tc_calc::TcCalcState::default(),
            show_global_edit: false,
            global_edit_n: 0.013,
            global_edit_dia_in: 18.0,
            report_template: ReportTemplate::default(),
            show_report_editor: false,
            dxf_underlay: Vec::new(),
            analysis_stale: true,
            pending_zoom_selection: false,
            project_dirty: false,
            prefs: AppPrefs::load(),
            tutorial: crate::tutorial::TutorialState::default(),
            noaa_paste_open: false,
            noaa_paste_text: String::new(),
        }
    }

    /// Title-bar / menu label reflecting save state and stale analysis.
    pub fn window_title(&self) -> String {
        let mut title = self.project.name.clone();
        if self.project_dirty {
            title.push('*');
        }
        if self.analysis_stale {
            title.push_str(" [stale]");
        }
        if let Some(path) = &self.project_path {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                title.push_str(&format!(" — {name}"));
            }
        }
        format!("{title} — StormSewer v0.8")
    }

    /// Count design-review errors and warnings from the last analysis.
    pub fn review_counts(&self) -> (usize, usize) {
        let errors = self
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count();
        let warnings = self
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count();
        (errors, warnings)
    }

    /// Mark the project as modified since last save.
    pub fn mark_project_dirty(&mut self) {
        self.project_dirty = true;
    }

    /// Mark the project as saved to disk.
    pub fn mark_project_saved(&mut self) {
        self.project_dirty = false;
    }

    /// Load DXF reference geometry for the plan underlay (from `background_dxf`).
    pub fn reload_dxf_underlay(&mut self) {
        self.dxf_underlay.clear();
        let Some(ref bg) = self.project.background_dxf else {
            return;
        };
        match import_dxf_underlay(std::path::Path::new(&bg.path)) {
            Ok(segs) => {
                self.dxf_underlay = segs;
                self.status = format!("DXF underlay loaded: {}", bg.path);
            }
            Err(e) => self.status = format!("DXF underlay: {e}"),
        }
    }

    /// Recompute pipe construction cost estimate.
    pub fn update_cost(&mut self) {
        let summary = estimate_network_cost(&self.project, &default_cost_table());
        self.cost_text = format_cost_summary(&summary);
    }

    /// Recompute network topology diagnostics.
    pub fn update_diagnostics(&mut self) {
        let diags = run_diagnostics(&self.project);
        self.diagnostics_text = format_diagnostics(&diags);
    }

    /// Convert project units (Hydraflow Options → Units).
    pub fn convert_units(&mut self, to: stormsewer::units::UnitSystem) {
        if self.project.units == to {
            return;
        }
        self.checkpoint_undo();
        convert_project(&mut self.project, to);
        self.run_analysis();
        self.update_cost();
        self.update_diagnostics();
        self.status = format!("Converted to {}", to.label());
    }

    /// Seed Tc calculator from project and current selection before opening.
    pub fn open_tc_calculator(&mut self) {
        self.tc_calc.p2_in = self.project.p2_rainfall_in;
        for seg in &mut self.tc_calc.segments {
            if seg.kind == stormsewer::hydrology::Tr55SegmentKind::Sheet {
                seg.p2_in = self.project.p2_rainfall_in;
            }
        }
        if let Some(idx) = self.edit.selected_catchment {
            if let Some(catchment) = self.project.catchments.get(idx) {
                self.tc_calc.length = catchment.flow_length_ft;
                self.tc_calc.slope = catchment.slope;
            }
        } else if let Some(idx) = self.selected_node {
            if let Some(node) = self.project.nodes.get(idx) {
                self.tc_calc.length = 300.0;
                let rim_drop = (node.rim - node.invert).max(1.0);
                self.tc_calc.slope = (rim_drop / 300.0).clamp(0.0001, 0.5);
            }
        }
        self.tc_calc.open = true;
    }

    /// Apply Tc from calculator to the selected structure or linked inlet.
    /// Also syncs P2 rainfall back to the project. Returns false if nothing was selected.
    pub fn apply_tc_minutes(&mut self, tc: f64) -> bool {
        self.project.p2_rainfall_in = self.tc_calc.p2_in;
        self.checkpoint_undo();

        if let Some(idx) = self.selected_node {
            if idx < self.project.nodes.len() {
                let id = self.project.nodes[idx].id.clone();
                self.project.nodes[idx].tc_inlet = tc;
                self.status = format!("Set {id} Tc = {tc:.2} min");
                self.run_analysis();
                return true;
            }
        }

        if let Some(idx) = self.edit.selected_catchment {
            if let Some(catchment) = self.project.catchments.get_mut(idx) {
                catchment.flow_length_ft = self.tc_calc.length;
                let id = catchment.id.clone();
                if let Some(ref inlet_id) = catchment.inlet_node_id {
                    if let Some(node) = self.project.nodes.iter_mut().find(|n| &n.id == inlet_id) {
                        node.tc_inlet = tc;
                        self.status = format!(
                            "Set catchment {id} flow length = {:.0} ft; {inlet_id} Tc = {tc:.2} min",
                            self.tc_calc.length
                        );
                        self.run_analysis();
                        return true;
                    }
                }
                self.status = format!(
                    "Set catchment {id} flow length = {:.0} ft — link an inlet to apply Tc",
                    self.tc_calc.length
                );
                self.run_analysis();
                return true;
            }
        }

        self.status = "Select a structure or catchment on the plan (or in Tables) to apply Tc".into();
        false
    }

    /// Unified selection across plan view, inspector, and tables.
    pub fn set_selection(
        &mut self,
        node: Option<usize>,
        pipe: Option<usize>,
        catchment: Option<usize>,
    ) {
        self.selected_node = node;
        self.selected_pipe = pipe;
        self.edit.selected_node = node;
        self.edit.selected_pipe = pipe;
        self.edit.selected_catchment = catchment;
        if node.is_some() || pipe.is_some() || catchment.is_some() {
            self.inspector_open = true;
        }
    }

    /// Select a structure or pipe by id (used by design review navigation).
    pub fn select_by_id(&mut self, id: &str) -> bool {
        if let Some(i) = self.project.nodes.iter().position(|n| n.id == id) {
            self.set_selection(Some(i), None, None);
            self.pending_zoom_selection = true;
            return true;
        }
        if let Some(i) = self.project.pipes.iter().position(|p| p.id == id) {
            self.set_selection(None, Some(i), None);
            self.pending_zoom_selection = true;
            return true;
        }
        false
    }

    /// Human-readable label for the current selection, if any.
    pub fn selection_label(&self) -> Option<String> {
        if let Some(idx) = self.selected_node {
            if let Some(node) = self.project.nodes.get(idx) {
                return Some(format!("{} ({})", node.id, node.kind));
            }
        }
        if let Some(idx) = self.selected_pipe {
            if let Some(pipe) = self.project.pipes.get(idx) {
                return Some(format!("{} (pipe)", pipe.id));
            }
        }
        if let Some(idx) = self.edit.selected_catchment {
            if let Some(catchment) = self.project.catchments.get(idx) {
                return Some(format!("{} (catchment)", catchment.id));
            }
        }
        None
    }

    /// Mark results out of date after parameter edits.
    pub fn mark_analysis_stale(&mut self) {
        self.analysis_stale = true;
        self.mark_project_dirty();
    }

    /// Set Manning n on all pipes (Hydraflow global editing).
    pub fn global_set_pipe_n(&mut self, n: f64) {
        self.checkpoint_undo();
        for pipe in &mut self.project.pipes {
            pipe.n = n;
        }
        self.status = format!("Set all pipe n = {n:.3}");
        self.run_analysis();
    }

    /// Set diameter on all circular pipes.
    pub fn global_set_pipe_diameter_in(&mut self, dia_in: f64) {
        self.checkpoint_undo();
        let dia_ft = dia_in / 12.0;
        for pipe in &mut self.project.pipes {
            if pipe.shape == "circular" {
                pipe.diameter = dia_ft;
            }
        }
        self.status = format!("Set all circular pipes to {dia_in:.0} in");
        self.run_analysis();
        self.update_cost();
    }

    /// Record undo checkpoint before mutating the project.
    pub fn checkpoint_undo(&mut self) {
        self.undo.checkpoint(&self.project);
        self.mark_project_dirty();
    }

    /// Record a snapshot taken before an immediate-mode UI edit.
    pub fn record_undo_snapshot(&mut self, previous: Project) {
        self.undo.record_previous(previous);
        self.mark_project_dirty();
    }

    /// Undo the last edit and re-run analysis.
    pub fn undo(&mut self) {
        if let Some(project) = self.undo.undo(&self.project) {
            self.restore_project(project);
            self.status = "Undo".into();
        }
    }

    /// Redo a previously undone edit.
    pub fn redo(&mut self) {
        if let Some(project) = self.undo.redo(&self.project) {
            self.restore_project(project);
            self.status = "Redo".into();
        }
    }

    /// Replace project document and refresh derived state.
    pub fn restore_project(&mut self, project: Project) {
        self.project = project;
        self.edit = init_edit_counters(&self.project);
        self.clear_selection();
        self.reload_dxf_underlay();
        self.run_analysis();
        self.update_inlet_check();
    }

    /// Load a project from disk (clears undo history).
    pub fn load_project(&mut self, project: Project, path: Option<std::path::PathBuf>) {
        self.undo.clear();
        self.bg_texture = None;
        self.project = project;
        self.project_path = path.clone();
        self.project_dirty = false;
        self.analysis_stale = false;
        self.edit = init_edit_counters(&self.project);
        self.clear_selection();
        if let Some(ref p) = path {
            self.recent.push(p.clone());
        }
        self.reload_dxf_underlay();
        self.run_analysis();
        self.update_inlet_check();
    }

    /// HEC-22 inlet capacity check for the selected inlet structure.
    pub fn update_inlet_check(&mut self) {
        use stormsewer::design::inlets::{check_inlet_geom, inlet_geometry_for_node, InletKind};

        let Some(idx) = self.selected_node else {
            self.inlet_check_text.clear();
            return;
        };
        let Some(node) = self.project.nodes.get(idx) else {
            self.inlet_check_text.clear();
            return;
        };
        if node.kind != "inlet" {
            self.inlet_check_text = "Select an inlet to run HEC-22 capacity check.".into();
            return;
        }

        // Approach flow is the LOCAL gutter runoff (C·A·i) at this inlet, not the
        // outgoing pipe's accumulated design flow.
        let intensity = self
            .analysis
            .as_ref()
            .map(|a| {
                a.pipes
                    .iter()
                    .filter(|pr| {
                        self.project
                            .pipes
                            .iter()
                            .any(|p| p.id == pr.id && p.from == node.id)
                    })
                    .map(|pr| pr.intensity)
                    .fold(0.0_f64, f64::max)
            })
            .unwrap_or(0.0);
        let design_q = node.c * self.project.area_to_engine_ac(node.area_ac) * intensity;

        let geom = inlet_geometry_for_node(
            &self.inlet_geom,
            node.inlet.length_ft,
            node.inlet.gutter_slope,
            node.inlet.sag,
        );
        let check = check_inlet_geom(design_q, &geom);
        let status = if check.ok { "OK" } else { "UNDERSIZED" };
        self.inlet_check_text = format!(
            "Inlet {} — {}\nDesign Q: {:.2} cfs\nCapacity: {:.2} cfs ({})",
            node.id,
            geom.kind.label(),
            check.design_q_cfs,
            check.capacity_cfs,
            status
        );

        if !check.ok && geom.kind == InletKind::GrateOnGrade {
            self.inlet_check_text.push_str("\nTip: try combination inlet or increase grate length.");
        }
    }

    /// Run hydraulic analysis on the current project network.
    pub fn run_analysis(&mut self) {
        let validation_errors = self.project.validate();
        if !validation_errors.is_empty() {
            self.report_text = format!(
                "Validation errors:\n{}",
                validation_errors.join("\n")
            );
            self.multi_rp_text.clear();
            self.analysis = None;
            self.findings.clear();
            self.status = format!(
                "Validation failed ({} issue(s))",
                validation_errors.len()
            );
            self.run_review();
            return;
        }

        let net = self.project.to_analysis_network();
        let idf_set = self.project.idf_set();
        match net.analyze(idf_set.design_curve(), &self.project.options()) {
            Ok(a) => {
                self.report_text = format_analysis(&a);
                self.analysis = Some(a);
                self.status = "Analysis complete".into();
                self.analysis_stale = false;

                match net.analyze_all_rps(&idf_set, &self.project.options()) {
                    Ok(results) => self.multi_rp_text = format_multi_rp_table(&results),
                    Err(e) => self.multi_rp_text = format!("Multi-RP error: {e}"),
                }
            }
            Err(e) => {
                self.report_text = format!("Analysis error: {e}");
                self.multi_rp_text.clear();
                self.analysis = None;
                self.findings.clear();
                self.status = format!("Analysis failed: {e}");
            }
        }
        self.run_review();
        self.update_cost();
        self.update_diagnostics();
    }

    /// Run design-standard review and format findings into `review_text`.
    pub fn run_review(&mut self) {
        let Some(ref analysis) = self.analysis else {
            self.findings.clear();
            self.review_text = "No analysis — run Analyze to enable design review.".into();
            return;
        };

        let net = self.project.to_network();
        self.findings = design_review(&net, analysis, &self.review_criteria);

        if self.findings.is_empty() {
            self.review_text = "Design review: no issues found.".into();
            return;
        }

        let mut text = String::from("=== DESIGN REVIEW ===\n\n");
        for finding in &self.findings {
            let tag = match finding.severity {
                Severity::Error => "ERROR",
                Severity::Warning => "WARN",
            };
            text.push_str(&format!("[{tag}] {}: {}\n", finding.id, finding.message));
        }
        self.review_text = text;
    }

    /// Clear the current selection.
    pub fn clear_selection(&mut self) {
        self.set_selection(None, None, None);
    }

    /// Whether something is selected in the inspector.
    pub fn has_selection(&self) -> bool {
        self.selected_node.is_some()
            || self.selected_pipe.is_some()
            || self.edit.selected_catchment.is_some()
    }

    /// Recommend pipe diameters from the latest analysis and apply them to the project.
    pub fn apply_sizing(&mut self) {
        let Some(analysis) = self.analysis.clone() else {
            self.status = "Run analysis before auto-sizing".into();
            self.sizing_text = "No analysis — run Analyze first.".into();
            return;
        };
        self.checkpoint_undo();

        let criteria = DesignCriteria::municipal();
        let net = self.project.to_network();
        let recs = recommend_all_pipes(&net, &analysis, &criteria);
        self.sizing_text = format_sizing_recommendations(&recs);

        let sized = apply_sizing_to_network(&net, &recs);
        for pipe in &mut self.project.pipes {
            if let Some(sp) = sized.pipes.iter().find(|p| p.id == pipe.id) {
                let new_dia = sp.diameter;
                pipe.diameter = new_dia;
                match pipe.shape.as_str() {
                    "box" => {
                        pipe.rise_ft = new_dia;
                        pipe.span_ft = new_dia;
                    }
                    "elliptical" => {
                        pipe.rise_ft = new_dia;
                        pipe.span_ft = new_dia * 1.5;
                    }
                    _ => {}
                }
            }
        }

        let changed = recs.iter().filter(|r| !r.meets_criteria).count();
        self.status = if changed == 0 {
            "All pipes meet municipal criteria".into()
        } else {
            format!("Auto-sized {changed} pipe(s) — re-running analysis")
        };
        self.run_analysis();
    }
}

fn init_edit_counters(project: &Project) -> EditState {
    let mut edit = EditState::default();
    for n in &project.nodes {
        if let Some(num) = n.id.strip_prefix('N').and_then(|s| s.parse::<u32>().ok()) {
            edit.next_node_id = edit.next_node_id.max(num + 1);
        }
    }
    for p in &project.pipes {
        if let Some(num) = p.id.strip_prefix('P').and_then(|s| s.parse::<u32>().ok()) {
            edit.next_pipe_id = edit.next_pipe_id.max(num + 1);
        }
    }
    for c in &project.catchments {
        if let Some(num) = c.id.strip_prefix('C').and_then(|s| s.parse::<u32>().ok()) {
            edit.next_catchment_id = edit.next_catchment_id.max(num + 1);
        }
    }
    if edit.next_node_id == 0 {
        edit.next_node_id = 1;
    }
    if edit.next_pipe_id == 0 {
        edit.next_pipe_id = 1;
    }
    if edit.next_catchment_id == 0 {
        edit.next_catchment_id = 1;
    }
    edit
}

fn format_multi_rp_table(results: &[(u32, Analysis)]) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut s = String::from("=== MULTI-RP PEAK Q (cfs) ===\n\n");
    let rps: Vec<u32> = results.iter().map(|(rp, _)| *rp).collect();
    let pipe_ids: Vec<String> = results[0].1.pipes.iter().map(|p| p.id.clone()).collect();

    s.push_str(&format!("{:<6}", "Pipe"));
    for rp in &rps {
        s.push_str(&format!(" {:>6}yr", rp));
    }
    s.push('\n');
    s.push_str(&"-".repeat(6 + 7 * rps.len()));
    s.push('\n');

    for pipe_id in &pipe_ids {
        s.push_str(&format!("{:<6}", pipe_id));
        for (_, analysis) in results {
            let q = analysis
                .pipes
                .iter()
                .find(|p| &p.id == pipe_id)
                .map(|p| p.design_q)
                .unwrap_or(0.0);
            s.push_str(&format!(" {:>8.2}", q));
        }
        s.push('\n');
    }
    s
}

fn format_sizing_recommendations(recs: &[PipeSizeRecommendation]) -> String {
    let mut s = String::from("=== PIPE SIZING (municipal criteria) ===\n\n");
    if recs.is_empty() {
        s.push_str("No pipes in network.\n");
        return s;
    }
    for r in recs {
        s.push_str(&r.note);
        s.push('\n');
    }
    let adequate = recs.iter().filter(|r| r.meets_criteria).count();
    s.push_str(&format!(
        "\n{adequate}/{} pipes already meet criteria.\n",
        recs.len()
    ));
    s
}