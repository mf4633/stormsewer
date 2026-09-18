// SPDX-License-Identifier: GPL-3.0-or-later

//! Results → Compare Runs… and Compare Engines…
//!
//! Every finished run is copied into a numbered folder under the system
//! temp directory (`StormSewer/runs/<n>/`), because the runner writes each
//! run's `.rpt`/`.out` beside the model and the next run overwrites them.
//! The editor keeps the last [`HISTORY_LEN`] records — paths, engine id /
//! version / binary hash, SHA-256 of the `.inp` text, elapsed time — and
//! the compare window tabulates node and link peak differences between any
//! two of them (`stormsewer_swmm::compare`), sortable by |Δ| or percent,
//! exportable as CSV. The results overlay has no hook for a custom value
//! map, so a row click selects the object on the canvas instead.
//!
//! Compare Engines runs the current model on two registered engines back
//! to back on a worker thread, records both, and opens the comparison.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Button, RichText, Ui};
use stormsewer_swmm::compare::{diff_links, diff_nodes, Diff, LinkMetric, NodeMetric};
use stormsewer_swmm::doc::build::ObjRef;
use stormsewer_swmm::engine::{Engine, Run, RunPaths};
use stormsewer_swmm::out::{link_peaks, node_peaks, LinkPeak, NodePeak, OutputFile};
use stormsewer_swmm::sha256::sha256_file;

use crate::state::AppState;
use crate::theme::palette;

/// How many runs the editor remembers.
pub const HISTORY_LEN: usize = 10;

/// One remembered run.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    /// 1-based, monotonic within the session.
    pub number: usize,
    pub label: String,
    pub inp: PathBuf,
    pub rpt: PathBuf,
    pub out: PathBuf,
    pub engine_id: String,
    pub engine_version: String,
    pub engine_sha256: String,
    /// SHA-256 of the `.inp` text the engine read.
    pub model_sha256: String,
    pub elapsed_ms: u128,
    pub analysis_begun: Option<String>,
    pub succeeded: bool,
    /// The scenario the run was of (`Tools → Scenarios… → Run All`), or
    /// `None` for an ordinary run of the working document.
    pub scenario: Option<String>,
}

impl RunRecord {
    pub fn short_hash(&self) -> &str {
        &self.model_sha256[..self.model_sha256.len().min(8)]
    }

    /// The label with the scenario, when there is one: `pond.inp [Big pipes]`.
    pub fn display_label(&self) -> String {
        match &self.scenario {
            Some(s) => format!("{} [{s}]", self.label),
            None => self.label.clone(),
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "#{} {} — SWMM {} — model {} — {} ms{}",
            self.number,
            self.display_label(),
            self.engine_version,
            self.short_hash(),
            self.elapsed_ms,
            if self.succeeded { "" } else { " (failed)" }
        )
    }
}

fn runs_dir() -> PathBuf {
    std::env::temp_dir().join("StormSewer").join("runs")
}

/// A folder name no other record can claim: this process's id and start
/// time, plus a counter. The run *number* the user sees is per history and
/// restarts at 1 for every model, so two histories in one process (two
/// windows, or two tests running in parallel threads) would otherwise both
/// write `runs/1` and copy over each other's files; that surfaced as a
/// once-in-six flake in the compare test.
fn record_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    static SESSION: OnceLock<String> = OnceLock::new();
    static SLOT: AtomicU64 = AtomicU64::new(0);
    let session = SESSION.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{}-{nanos}", std::process::id())
    });
    let slot = SLOT.fetch_add(1, Ordering::Relaxed);
    runs_dir().join(format!("{session}-{slot}"))
}

/// Copy a run's files into their own folder and describe them. `label` is
/// what the user will see in the picker (the model's file name).
pub fn record_run(history: &mut Vec<RunRecord>, run: &Run, label: &str) -> Option<RunRecord> {
    record_run_scenario(history, run, label, None)
}

/// [`record_run`] with the scenario the run was of.
pub fn record_run_scenario(
    history: &mut Vec<RunRecord>,
    run: &Run,
    label: &str,
    scenario: Option<&str>,
) -> Option<RunRecord> {
    let number = history.iter().map(|r| r.number).max().unwrap_or(0) + 1;
    let dir = record_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let copy = |src: &Path, name: &str| -> Option<PathBuf> {
        let dst = dir.join(name);
        std::fs::copy(src, &dst).ok().map(|_| dst)
    };
    let inp = copy(&run.inp, "model.inp")?;
    let rpt = copy(&run.rpt, "model.rpt").unwrap_or_else(|| run.rpt.clone());
    let out = copy(&run.out, "model.out").unwrap_or_else(|| run.out.clone());
    let rec = RunRecord {
        number,
        label: label.to_string(),
        model_sha256: sha256_file(&inp).unwrap_or_default(),
        inp,
        rpt,
        out,
        engine_id: run.engine_id.clone(),
        engine_version: run.engine_version.clone(),
        engine_sha256: run.engine_sha256.clone(),
        elapsed_ms: run.elapsed.as_millis(),
        analysis_begun: run.report.analysis_begun.clone(),
        succeeded: run.succeeded(),
        scenario: scenario.map(str::to_string),
    };
    history.push(rec.clone());
    while history.len() > HISTORY_LEN {
        let old = history.remove(0);
        if let Some(d) = old.inp.parent() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    Some(rec)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompareSort {
    #[default]
    AbsDelta,
    Percent,
    Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareKind {
    Nodes(NodeMetric),
    Links(LinkMetric),
}

impl Default for CompareKind {
    fn default() -> Self {
        CompareKind::Nodes(NodeMetric::MaxDepth)
    }
}

impl CompareKind {
    fn label(self) -> String {
        match self {
            CompareKind::Nodes(m) => format!("Nodes: {}", m.label()),
            CompareKind::Links(m) => format!("Links: {}", m.label()),
        }
    }
}

/// Peaks read from one recorded run.
struct Loaded {
    number: usize,
    nodes: Vec<NodePeak>,
    links: Vec<LinkPeak>,
}

fn load_peaks(rec: &RunRecord) -> Result<Loaded, String> {
    let f = OutputFile::open(&rec.out).map_err(|e| format!("run #{}: {e}", rec.number))?;
    let nodes = node_peaks(&f.path, &f.meta).map_err(|e| e.to_string())?;
    let links = link_peaks(&f.path, &f.meta).map_err(|e| e.to_string())?;
    Ok(Loaded { number: rec.number, nodes, links })
}

enum EngineOutcome {
    Done(Box<Run>, Box<Run>),
    Failed(String),
}

#[derive(Default)]
pub struct SwmmCompareState {
    pub open: bool,
    /// Run numbers chosen for A and B.
    pub a: Option<usize>,
    pub b: Option<usize>,
    pub kind: CompareKind,
    pub sort: CompareSort,
    pub diff: Option<Diff>,
    pub message: String,
    loaded_a: Option<Loaded>,
    loaded_b: Option<Loaded>,
    /// Compare Engines: the second engine to run against the chosen one.
    pub engines_open: bool,
    pub other_engine: Option<String>,
    engines_rx: Option<Receiver<EngineOutcome>>,
    /// The last run the history has recorded, by its `.out` mtime + engine,
    /// so a run is captured once.
    seen_run: Option<(PathBuf, String, u128)>,
}

impl SwmmCompareState {
    pub fn is_running_engines(&self) -> bool {
        self.engines_rx.is_some()
    }
}

/// Record `swmm.last_run` into the history the first time it is seen.
/// Called once per frame from `draw_windows`.
pub fn capture_last_run(state: &mut AppState) {
    let Some(run) = state.swmm.last_run.as_ref() else { return };
    let key = (run.out.clone(), run.engine_sha256.clone(), run.elapsed.as_nanos());
    if state.swmm_compare.seen_run.as_ref() == Some(&key) {
        return;
    }
    state.swmm_compare.seen_run = Some(key);
    let label = state
        .swmm
        .model
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| state.swmm_doc.file_name());
    record_run(&mut state.swmm_doc.run_history, run, &label);
}

fn find_record(state: &AppState, number: Option<usize>) -> Option<RunRecord> {
    let n = number?;
    state.swmm_doc.run_history.iter().find(|r| r.number == n).cloned()
}

/// Tabulate A against B with the chosen metric and sort.
pub fn recompute(state: &mut AppState) {
    let (Some(ra), Some(rb)) = (find_record(state, state.swmm_compare.a), find_record(state, state.swmm_compare.b)) else {
        state.swmm_compare.diff = None;
        state.swmm_compare.message = "Pick two runs.".into();
        return;
    };
    let c = &mut state.swmm_compare;
    if c.loaded_a.as_ref().map(|l| l.number) != Some(ra.number) {
        match load_peaks(&ra) {
            Ok(l) => c.loaded_a = Some(l),
            Err(e) => {
                c.message = e;
                c.diff = None;
                return;
            }
        }
    }
    if c.loaded_b.as_ref().map(|l| l.number) != Some(rb.number) {
        match load_peaks(&rb) {
            Ok(l) => c.loaded_b = Some(l),
            Err(e) => {
                c.message = e;
                c.diff = None;
                return;
            }
        }
    }
    let (la, lb) = (c.loaded_a.as_ref().unwrap(), c.loaded_b.as_ref().unwrap());
    let mut d = match c.kind {
        CompareKind::Nodes(m) => diff_nodes(&la.nodes, &lb.nodes, m),
        CompareKind::Links(m) => diff_links(&la.links, &lb.links, m),
    };
    match c.sort {
        CompareSort::AbsDelta => d.sort_by_abs_delta(),
        CompareSort::Percent => d.sort_by_percent(),
        CompareSort::Name => d.rows.sort_by(|x, y| x.id.cmp(&y.id)),
    }
    c.message = if d.is_identical(1e-9) {
        format!("{} object(s): identical", d.rows.len())
    } else {
        format!(
            "{} object(s), max |Δ| {:.4}; {} only in A, {} only in B",
            d.rows.len(),
            d.max_abs_delta(),
            d.only_a.len(),
            d.only_b.len()
        )
    };
    c.diff = Some(d);
}

/// Open the window with the two most recent runs chosen.
pub fn open(state: &mut AppState) {
    capture_last_run(state);
    let h = &state.swmm_doc.run_history;
    let n = h.len();
    if state.swmm_compare.a.is_none() && n >= 2 {
        state.swmm_compare.a = Some(h[n - 2].number);
    }
    if state.swmm_compare.b.is_none() && n >= 1 {
        state.swmm_compare.b = Some(h[n - 1].number);
    }
    state.swmm_compare.open = true;
    recompute(state);
}

pub fn csv(state: &AppState) -> Option<String> {
    let d = state.swmm_compare.diff.as_ref()?;
    Some(d.to_csv(&state.swmm_compare.kind.label()))
}

/// Start the same model on `first` then `second` (worker thread).
pub fn start_engines(state: &mut AppState, first: &Engine, second: &Engine) {
    let path = match state.swmm_doc.run_path() {
        Ok(p) => p,
        Err(findings) => {
            state.status = format!("Run refused: {} error(s) to fix first", findings.len());
            state.swmm_doc.run_refused = Some(findings);
            return;
        }
    };
    state.swmm.model = Some(path.clone());
    let (a, b) = (first.clone(), second.clone());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Each engine writes its own .rpt/.out so the first run's results
        // survive the second.
        let run_one = |e: &Engine| -> Result<Run, String> {
            let mut paths = RunPaths::beside(&path).map_err(|e| e.to_string())?;
            let tag = e.id.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
            paths.rpt = paths.rpt.with_extension(format!("{tag}.rpt"));
            paths.out = paths.out.with_extension(format!("{tag}.out"));
            e.run_with(&paths).map_err(|e| e.to_string())
        };
        let outcome = match run_one(&a) {
            Err(e) => EngineOutcome::Failed(format!("{}: {e}", a.label())),
            Ok(ra) => match run_one(&b) {
                Err(e) => EngineOutcome::Failed(format!("{}: {e}", b.label())),
                Ok(rb) => EngineOutcome::Done(Box::new(ra), Box::new(rb)),
            },
        };
        let _ = tx.send(outcome);
    });
    state.swmm_compare.engines_rx = Some(rx);
    state.status = format!("Comparing engines: {} then {}…", first.label(), second.label());
}

/// Two engine runs came back as one pair: record both, make B the
/// current run, and open the comparison.
fn poll_engines(state: &mut AppState) {
    let Some(rx) = state.swmm_compare.engines_rx.as_ref() else { return };
    match rx.try_recv() {
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            state.swmm_compare.engines_rx = None;
            state.status = "The engine comparison stopped without a result.".into();
        }
        Ok(EngineOutcome::Failed(e)) => {
            state.swmm_compare.engines_rx = None;
            state.status = format!("Engine comparison failed: {e}");
        }
        Ok(EngineOutcome::Done(ra, rb)) => {
            state.swmm_compare.engines_rx = None;
            let label = state.swmm_doc.file_name();
            let a = record_run(&mut state.swmm_doc.run_history, &ra, &format!("{label} [{}]", ra.engine_version));
            let b = record_run(&mut state.swmm_doc.run_history, &rb, &format!("{label} [{}]", rb.engine_version));
            state.swmm_compare.a = a.map(|r| r.number);
            state.swmm_compare.b = b.map(|r| r.number);
            state.swmm_compare.seen_run = Some((rb.out.clone(), rb.engine_sha256.clone(), rb.elapsed.as_nanos()));
            state.swmm.last_run = Some(*rb);
            state.swmm_compare.open = true;
            recompute(state);
            state.status = format!("Engines compared: {}", state.swmm_compare.message);
        }
    }
}

// --- menu ---------------------------------------------------------------------

pub fn results_menu_items(ui: &mut Ui, state: &mut AppState) {
    capture_last_run(state);
    let n = state.swmm_doc.run_history.len();
    if ui
        .add_enabled(n >= 1, Button::new("Compare Runs…"))
        .on_disabled_hover_text("Run the model at least once")
        .clicked()
    {
        open(state);
        ui.close_menu();
    }
    state.swmm.ensure_discovered();
    let engines = state.swmm.registry.engines().len();
    if ui
        .add_enabled(engines >= 2 && state.swmm_doc.loaded && !state.swmm_compare.is_running_engines(), Button::new("Compare Engines…"))
        .on_disabled_hover_text("Register a second SWMM engine in the Run menu")
        .clicked()
    {
        state.swmm_compare.engines_open = true;
        ui.close_menu();
    }
}

// --- windows ------------------------------------------------------------------

fn run_picker(ui: &mut Ui, id: &str, chosen: &mut Option<usize>, history: &[RunRecord]) -> bool {
    let mut changed = false;
    let text = chosen
        .and_then(|n| history.iter().find(|r| r.number == n))
        .map(|r| format!("#{} {} ({})", r.number, r.display_label(), r.engine_version))
        .unwrap_or_else(|| "—".into());
    egui::ComboBox::from_id_salt(id).selected_text(text).width(260.0).show_ui(ui, |ui| {
        for r in history {
            if ui.selectable_label(*chosen == Some(r.number), r.summary()).clicked() {
                *chosen = Some(r.number);
                changed = true;
            }
        }
    });
    changed
}

fn draw_compare(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_compare.open {
        return;
    }
    let dark = ctx.style().visuals.dark_mode;
    let mut open = true;
    let mut pick: Option<ObjRef> = None;
    egui::Window::new("Compare Runs")
        .open(&mut open)
        .default_width(640.0)
        .resizable(true)
        .show(ctx, |ui| {
            let history = state.swmm_doc.run_history.clone();
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("A");
                changed |= run_picker(ui, "swmm-compare-a", &mut state.swmm_compare.a, &history);
                ui.label("B");
                changed |= run_picker(ui, "swmm-compare-b", &mut state.swmm_compare.b, &history);
                if ui.button("⇄").on_hover_text("Swap A and B").clicked() {
                    std::mem::swap(&mut state.swmm_compare.a, &mut state.swmm_compare.b);
                    std::mem::swap(&mut state.swmm_compare.loaded_a, &mut state.swmm_compare.loaded_b);
                    changed = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                let current = state.swmm_compare.kind;
                egui::ComboBox::from_id_salt("swmm-compare-kind")
                    .selected_text(current.label())
                    .show_ui(ui, |ui| {
                        for m in NodeMetric::ALL {
                            let k = CompareKind::Nodes(m);
                            if ui.selectable_label(current == k, k.label()).clicked() {
                                state.swmm_compare.kind = k;
                                changed = true;
                            }
                        }
                        for m in LinkMetric::ALL {
                            let k = CompareKind::Links(m);
                            if ui.selectable_label(current == k, k.label()).clicked() {
                                state.swmm_compare.kind = k;
                                changed = true;
                            }
                        }
                    });
                ui.label("Sort");
                for (s, l) in [(CompareSort::AbsDelta, "|Δ|"), (CompareSort::Percent, "%"), (CompareSort::Name, "name")] {
                    if ui.selectable_label(state.swmm_compare.sort == s, l).clicked() {
                        state.swmm_compare.sort = s;
                        changed = true;
                    }
                }
                if ui.add_enabled(state.swmm_compare.diff.is_some(), Button::new("Export CSV…")).clicked() {
                    if let Some(text) = csv(state) {
                        if let Some(s) = crate::swmm_export::save_csv("compare.csv", &text) {
                            state.status = s;
                        }
                    }
                }
            });
            if changed {
                recompute(state);
            }
            if let (Some(a), Some(b)) = (find_record(state, state.swmm_compare.a), find_record(state, state.swmm_compare.b)) {
                ui.label(RichText::new(format!(
                    "A: SWMM {} ({}) model {} · B: SWMM {} ({}) model {}{}",
                    a.engine_version,
                    &a.engine_sha256[..a.engine_sha256.len().min(8)],
                    a.short_hash(),
                    b.engine_version,
                    &b.engine_sha256[..b.engine_sha256.len().min(8)],
                    b.short_hash(),
                    if a.model_sha256 == b.model_sha256 { " (same model)" } else { " (model differs)" }
                )).small());
            }
            ui.label(RichText::new(state.swmm_compare.message.clone()).small());
            ui.separator();
            // Without two runs there is no diff and the closure returns, which
            // used to leave the window empty below the selectors. Say why.
            let Some(diff) = state.swmm_compare.diff.as_ref() else {
                ui.label(
                    RichText::new("Choose two runs in A and B to compare them. Runs are recorded each time the model is run.")
                        .small()
                        .weak(),
                );
                return;
            };
            let is_node = matches!(state.swmm_compare.kind, CompareKind::Nodes(_));
            egui::ScrollArea::vertical().id_salt("swmm-compare-rows").max_height(360.0).show(ui, |ui| {
                egui::Grid::new("swmm-compare-grid").striped(true).show(ui, |ui| {
                    for h in ["Object", "A", "B", "Δ (B−A)", "%"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in &diff.rows {
                        if ui.selectable_label(false, &r.id).on_hover_text("Select on the map").clicked() {
                            pick = Some(if is_node { ObjRef::Node(r.id.clone()) } else { ObjRef::Link(r.id.clone()) });
                        }
                        ui.monospace(format!("{:.3}", r.a));
                        ui.monospace(format!("{:.3}", r.b));
                        let d = r.delta();
                        let color = if d.abs() < 1e-9 {
                            palette::muted_text(dark)
                        } else if d > 0.0 {
                            palette::error_text(dark)
                        } else {
                            palette::ok_text(dark)
                        };
                        ui.label(RichText::new(format!("{d:+.3}")).monospace().color(color));
                        ui.monospace(r.percent().map(|p| format!("{p:+.1}")).unwrap_or_else(|| "—".into()));
                        ui.end_row();
                    }
                    for id in &diff.only_a {
                        ui.label(id);
                        ui.label("only in A");
                        ui.end_row();
                    }
                    for id in &diff.only_b {
                        ui.label(id);
                        ui.label("");
                        ui.label("only in B");
                        ui.end_row();
                    }
                });
            });
        });
    state.swmm_compare.open = open;
    if let Some(r) = pick {
        state.swmm_doc.select_only(r.clone());
        state.swmm_doc.pending_zoom_to = Some(r);
    }
}

fn draw_engines(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_compare.engines_open {
        return;
    }
    let mut open = true;
    let mut start: Option<(Engine, Engine)> = None;
    egui::Window::new("Compare Engines")
        .open(&mut open)
        .collapsible(false)
        .show(ctx, |ui| {
            let engines: Vec<Engine> = state.swmm.registry.engines().to_vec();
            let Some(first) = state.swmm.engine().cloned() else {
                ui.label("Choose an engine in the Run menu first.");
                return;
            };
            ui.label(format!("Run {} on {} and on:", state.swmm_doc.file_name(), first.label()));
            if state.swmm_compare.other_engine.as_deref() == Some(first.id.as_str()) {
                state.swmm_compare.other_engine = None;
            }
            for e in engines.iter().filter(|e| e.id != first.id) {
                ui.radio_value(&mut state.swmm_compare.other_engine, Some(e.id.clone()), e.label());
            }
            ui.horizontal(|ui| {
                let second = state
                    .swmm_compare
                    .other_engine
                    .as_deref()
                    .and_then(|id| engines.iter().find(|e| e.id == id))
                    .cloned();
                let busy = state.swmm_compare.is_running_engines();
                if ui.add_enabled(second.is_some() && !busy, Button::new("Run both")).clicked() {
                    start = Some((first.clone(), second.unwrap()));
                }
                if busy {
                    ui.spinner();
                    ui.label("running…");
                }
            });
        });
    state.swmm_compare.engines_open = open;
    if let Some((a, b)) = start {
        start_engines(state, &a, &b);
        state.swmm_compare.engines_open = false;
    }
}

/// Once per frame: capture finished runs, poll the engine comparison, draw.
pub fn draw_windows(ctx: &egui::Context, state: &mut AppState) {
    capture_last_run(state);
    poll_engines(state);
    if state.swmm_compare.is_running_engines() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    draw_compare(ctx, state);
    draw_engines(ctx, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::swmm_report::tests::fixture_run;
    use crate::StormSewerApp;

    #[test]
    fn runs_are_recorded_once_and_compare_to_themselves_as_identical() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), None);
        app.state.swmm.last_run = Some(fixture_run());
        capture_last_run(&mut app.state);
        capture_last_run(&mut app.state);
        assert_eq!(app.state.swmm_doc.run_history.len(), 1, "one record per run");
        let rec = app.state.swmm_doc.run_history[0].clone();
        assert!(rec.inp.exists() && rec.out.exists() && rec.rpt.exists());
        assert_eq!(rec.model_sha256.len(), 64);
        assert_eq!(rec.engine_version, "5.2.4");
        assert!(rec.summary().contains("1234 ms"));
        assert_eq!(rec.scenario, None);
        assert_eq!(rec.display_label(), rec.label);
        // A scenario run carries its label into the picker text.
        let mut h = Vec::new();
        let s = record_run_scenario(&mut h, &fixture_run(), "pond.inp", Some("Big pipes")).unwrap();
        assert_eq!(s.scenario.as_deref(), Some("Big pipes"));
        assert_eq!(s.display_label(), "pond.inp [Big pipes]");
        assert!(s.summary().starts_with("#1 pond.inp [Big pipes] — SWMM 5.2.4"));
        for r in &h {
            if let Some(d) = r.inp.parent() {
                let _ = std::fs::remove_dir_all(d);
            }
        }

        // A second, distinct run of the same results.
        let mut r2 = fixture_run();
        r2.elapsed = std::time::Duration::from_millis(999);
        app.state.swmm.last_run = Some(r2);
        open(&mut app.state);
        assert_eq!(app.state.swmm_doc.run_history.len(), 2);
        assert_eq!(app.state.swmm_compare.a, Some(1));
        assert_eq!(app.state.swmm_compare.b, Some(2));
        let d = app.state.swmm_compare.diff.as_ref().unwrap();
        assert_eq!(d.rows.len(), 14);
        assert!(d.is_identical(0.0));
        assert!(app.state.swmm_compare.message.contains("identical"));
        for k in [CompareKind::Links(LinkMetric::MaxFlow), CompareKind::Nodes(NodeMetric::MaxFlooding)] {
            app.state.swmm_compare.kind = k;
            for s in [CompareSort::AbsDelta, CompareSort::Percent, CompareSort::Name] {
                app.state.swmm_compare.sort = s;
                recompute(&mut app.state);
                assert!(app.state.swmm_compare.diff.as_ref().unwrap().is_identical(0.0));
                run_frame(&mut app);
            }
        }
        let text = csv(&app.state).unwrap();
        assert!(text.starts_with("Object,Nodes: Max flooding A,"));
        assert_eq!(text.lines().count(), 15);

        // History is bounded.
        for i in 0..HISTORY_LEN + 2 {
            let mut r = fixture_run();
            r.elapsed = std::time::Duration::from_millis(5000 + i as u64);
            app.state.swmm.last_run = Some(r);
            capture_last_run(&mut app.state);
        }
        assert_eq!(app.state.swmm_doc.run_history.len(), HISTORY_LEN);
        assert!(app.state.swmm_doc.run_history[0].number > 1);

        // A missing run falls back gracefully.
        app.state.swmm_compare.a = Some(1);
        recompute(&mut app.state);
        assert!(app.state.swmm_compare.diff.is_none());
        app.state.swmm_compare.engines_open = true;
        run_frame(&mut app);
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| results_menu_items(ui, &mut state));
        });
    }
}
