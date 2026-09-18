// SPDX-License-Identifier: GPL-3.0-or-later

//! Tools → Scenarios…: named sets of edits on the base model, kept in
//! `<stem>.scenarios.json` beside it, activated into the editor, run as a
//! batch and compared.
//!
//! Activating a scenario swaps the editor's document for base + scenario;
//! the base document is parked here untouched. While a scenario is active
//! every change the user makes is recorded into it: the document's undo
//! history is private, so on every generation change the scenario is
//! recomputed as `scenario::diff(base, working)` and written to the
//! sidecar. Deactivating parks the scenario and puts the base document
//! back, byte for byte, with its own undo history and dirty flag.
//!
//! While a scenario is active the editor's path is
//! `<stem>.scenario.<name>.inp` beside the model, so Ctrl+S cannot write
//! the scenario over the base, the engine's scratch folder is distinct per
//! scenario, and relative `[FILES]` still resolve against the model's
//! folder.
//!
//! Run All runs the base and every scenario on a worker thread, records
//! each result in the compare history (`swmm_compare`, with the scenario
//! label) and tabulates the headline numbers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use eframe::egui::{self, Button, RichText, Ui};
use stormsewer_swmm::doc::InpDoc;
use stormsewer_swmm::engine::{self, Engine, Run};
use stormsewer_swmm::scenario::{
    self, Scenario, ScenarioResult, ScenarioSet, StaleEdit,
};

use crate::state::AppState;
use crate::swmm_compare::record_run_scenario;
use crate::theme::palette;

/// The scenario the editor is working on, with the base parked.
pub struct ActiveScenario {
    pub index: usize,
    pub name: String,
    base_doc: InpDoc,
    base_path: Option<PathBuf>,
    /// The editor path while active, to notice a different model being
    /// opened over it.
    scenario_path: Option<PathBuf>,
    last_gen: u64,
}

enum RunMsg {
    Started(String),
    Done(String, Box<Run>),
    Failed(String, String),
    Finished,
}

struct RunAll {
    rx: Receiver<RunMsg>,
    stop: Arc<AtomicBool>,
    total: usize,
    done: usize,
    current: String,
}

/// Per-editor scenario state.
#[derive(Default)]
pub struct ScenarioState {
    pub open: bool,
    pub set: ScenarioSet,
    /// The model path the set was loaded for (`Some(None)` = an unsaved
    /// model, `None` = not loaded yet).
    loaded_for: Option<Option<PathBuf>>,
    /// The selected list row: `None` is the base model.
    pub selected: Option<usize>,
    pub active: Option<ActiveScenario>,
    pub renaming: bool,
    pub rename_draft: String,
    pub message: String,
    /// Stale edits of the selected scenario against the base.
    pub stale: Vec<StaleEdit>,
    stale_for: Option<(usize, u64)>,
    run: Option<RunAll>,
    pub results: Vec<ScenarioResult>,
    pub results_open: bool,
}

impl ScenarioState {
    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn active_name(&self) -> Option<&str> {
        self.active.as_ref().map(|a| a.name.as_str())
    }

    pub fn is_active(&self, i: usize) -> bool {
        self.active.as_ref().is_some_and(|a| a.index == i)
    }
}

/// The base model's path: the parked one while a scenario is active.
pub fn model_path(state: &AppState) -> Option<PathBuf> {
    match &state.swmm_doc.scenarios.active {
        Some(a) => a.base_path.clone(),
        None => state.swmm_doc.path.clone(),
    }
}

/// Load the sidecar for the open model, once per model.
pub fn ensure_loaded(state: &mut AppState) {
    let path = model_path(state);
    let sc = &mut state.swmm_doc.scenarios;
    if sc.loaded_for.as_ref() == Some(&path) {
        return;
    }
    sc.loaded_for = Some(path.clone());
    sc.selected = None;
    sc.stale.clear();
    sc.stale_for = None;
    sc.results.clear();
    sc.set = match &path {
        Some(p) => match ScenarioSet::load_for(p) {
            Ok(s) => s,
            Err(e) => {
                sc.message = format!("Could not read the scenarios file: {e}");
                ScenarioSet::default()
            }
        },
        None => ScenarioSet::default(),
    };
}

/// Write the sidecar beside the model. Returns whether it could.
pub fn save_set(state: &mut AppState) -> bool {
    let Some(path) = model_path(state) else {
        state.swmm_doc.scenarios.message =
            "Save the model first: scenarios are stored beside it.".into();
        return false;
    };
    match state.swmm_doc.scenarios.set.save_for(&path) {
        Ok(()) => {
            state.swmm_doc.scenarios.message = format!(
                "Saved {}",
                ScenarioSet::sidecar_path(&path).display()
            );
            true
        }
        Err(e) => {
            state.swmm_doc.scenarios.message = format!("Could not save the scenarios: {e}");
            false
        }
    }
}

pub fn open(state: &mut AppState) {
    ensure_loaded(state);
    state.swmm_doc.scenarios.open = true;
}

/// Add a scenario (name made unique) and select it. Returns its index.
pub fn add_scenario(state: &mut AppState, scenario: Scenario) -> usize {
    ensure_loaded(state);
    let i = state.swmm_doc.scenarios.set.add(scenario);
    state.swmm_doc.scenarios.selected = Some(i);
    save_set(state);
    i
}

pub fn new_scenario(state: &mut AppState, name: &str) -> usize {
    add_scenario(state, Scenario::new(name))
}

pub fn duplicate_selected(state: &mut AppState) {
    let Some(i) = state.swmm_doc.scenarios.selected else { return };
    if let Some(j) = state.swmm_doc.scenarios.set.duplicate(i) {
        state.swmm_doc.scenarios.selected = Some(j);
        save_set(state);
    }
}

pub fn rename_selected(state: &mut AppState, name: &str) -> Result<(), String> {
    let Some(i) = state.swmm_doc.scenarios.selected else {
        return Err("select a scenario".into());
    };
    state.swmm_doc.scenarios.set.rename(i, name)?;
    if let Some(a) = state.swmm_doc.scenarios.active.as_mut() {
        if a.index == i {
            a.name = name.trim().to_string();
        }
    }
    save_set(state);
    Ok(())
}

pub fn delete_selected(state: &mut AppState) {
    let Some(i) = state.swmm_doc.scenarios.selected else { return };
    if state.swmm_doc.scenarios.is_active(i) {
        deactivate(state);
    }
    if let Some(a) = state.swmm_doc.scenarios.active.as_mut() {
        if a.index > i {
            a.index -= 1;
        }
    }
    state.swmm_doc.scenarios.set.remove(i);
    state.swmm_doc.scenarios.selected = None;
    state.swmm_doc.scenarios.stale_for = None;
    save_set(state);
}

/// Switch the editor to base + scenario `i`.
pub fn activate(state: &mut AppState, i: usize) -> Result<(), String> {
    ensure_loaded(state);
    if state.swmm_doc.scenarios.active.is_some() {
        deactivate(state);
    }
    let Some(scenario) = state.swmm_doc.scenarios.set.scenarios.get(i).cloned() else {
        return Err("no such scenario".into());
    };
    let doc = scenario::apply(&state.swmm_doc.doc, &scenario).map_err(|e| e.to_string())?;
    let ed = &mut state.swmm_doc;
    let base_doc = std::mem::take(&mut ed.doc);
    let base_path = ed.path.take();
    let scenario_path = base_path
        .as_ref()
        .map(|p| scenario::scenario_model_path(p, &scenario.name));
    ed.replace_document(doc);
    ed.path = scenario_path.clone();
    let last_gen = ed.doc.generation();
    ed.scenarios.active = Some(ActiveScenario {
        index: i,
        name: scenario.name.clone(),
        base_doc,
        base_path,
        scenario_path,
        last_gen,
    });
    ed.scenarios.selected = Some(i);
    ed.scenarios.message = format!("Working on scenario {:?}; edits are recorded into it.", scenario.name);
    state.status = format!("Scenario {:?} active", scenario.name);
    Ok(())
}

/// Record the working document's changes into the active scenario when
/// the document changed since the last look. Returns whether the
/// scenario's edits changed.
pub fn record(state: &mut AppState) -> bool {
    let ed = &mut state.swmm_doc;
    let Some(a) = ed.scenarios.active.as_mut() else {
        return false;
    };
    let gen = ed.doc.generation();
    if gen == a.last_gen {
        return false;
    }
    a.last_gen = gen;
    let edits = scenario::diff(&a.base_doc, &ed.doc);
    let index = a.index;
    let Some(s) = ed.scenarios.set.scenarios.get_mut(index) else {
        return false;
    };
    if s.edits == edits {
        return false;
    }
    s.edits = edits;
    save_set(state);
    true
}

/// Put the base document back. The scenario keeps what was recorded.
pub fn deactivate(state: &mut AppState) {
    record(state);
    let ed = &mut state.swmm_doc;
    let Some(a) = ed.scenarios.active.take() else { return };
    ed.replace_document(a.base_doc);
    ed.path = a.base_path;
    ed.scenarios.message = format!("Back on the base model; scenario {:?} kept.", a.name);
    state.status = "Base model".into();
}

/// A different model was opened over an active scenario: the parked base
/// belongs to the old model, so the activation is dropped.
fn check_model_switch(state: &mut AppState) {
    let dropped = {
        let ed = &state.swmm_doc;
        match &ed.scenarios.active {
            Some(a) => ed.path != a.scenario_path,
            None => false,
        }
    };
    if dropped {
        let a = state.swmm_doc.scenarios.active.take().unwrap();
        state.swmm_doc.scenarios.message = format!(
            "Scenario {:?} was deactivated: another model was opened.",
            a.name
        );
    }
}

/// Recompute the stale-edit list for the selected scenario when the
/// selection or the base changed.
fn refresh_stale(state: &mut AppState) {
    let gen = match &state.swmm_doc.scenarios.active {
        Some(a) => a.base_doc.generation(),
        None => state.swmm_doc.doc.generation(),
    };
    let sc = &state.swmm_doc.scenarios;
    let Some(i) = sc.selected else {
        state.swmm_doc.scenarios.stale.clear();
        state.swmm_doc.scenarios.stale_for = None;
        return;
    };
    if sc.stale_for == Some((i, gen)) {
        return;
    }
    let Some(s) = sc.set.scenarios.get(i) else { return };
    let stale = match &sc.active {
        Some(a) => scenario::validate(&a.base_doc, s),
        None => scenario::validate(&state.swmm_doc.doc, s),
    };
    state.swmm_doc.scenarios.stale = stale;
    state.swmm_doc.scenarios.stale_for = Some((i, gen));
}

/// Run the base and every scenario through `engine` on a worker.
pub fn run_all(state: &mut AppState, engine: &Engine) {
    ensure_loaded(state);
    if state.swmm_doc.scenarios.run.is_some() {
        return;
    }
    let model = model_path(state).unwrap_or_else(|| PathBuf::from(state.swmm_doc.file_name()));
    let base = match &state.swmm_doc.scenarios.active {
        Some(a) => scenario::clone_doc(&a.base_doc),
        None => scenario::clone_doc(&state.swmm_doc.doc),
    };
    let mut jobs: Vec<(String, Result<String, String>)> = vec![("base".to_string(), Ok(base.to_string()))];
    for s in &state.swmm_doc.scenarios.set.scenarios {
        jobs.push((
            s.name.clone(),
            scenario::materialize(&base, s).map_err(|e| e.to_string()),
        ));
    }
    let total = jobs.len();
    let engine = engine.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for (label, text) in jobs {
            if stop_w.load(Ordering::Relaxed) {
                break;
            }
            let _ = tx.send(RunMsg::Started(label.clone()));
            let text = match text {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.send(RunMsg::Failed(label, e));
                    continue;
                }
            };
            let synthetic = scenario::scenario_model_path(&model, &label);
            let outcome = engine::prepare(&synthetic, &text, true)
                .and_then(|p| engine.run_prepared(&p));
            match outcome {
                Ok(run) => {
                    let _ = tx.send(RunMsg::Done(label, Box::new(run)));
                }
                Err(e) => {
                    let _ = tx.send(RunMsg::Failed(label, e.to_string()));
                }
            }
        }
        let _ = tx.send(RunMsg::Finished);
    });
    state.swmm_doc.scenarios.results.clear();
    state.swmm_doc.scenarios.run = Some(RunAll {
        rx,
        stop,
        total,
        done: 0,
        current: String::new(),
    });
    state.status = format!("Running {total} scenario run(s)…");
}

fn poll_run(state: &mut AppState) {
    let label = state.swmm_doc.file_name();
    let mut finished = false;
    loop {
        let Some(run) = state.swmm_doc.scenarios.run.as_mut() else { return };
        match run.rx.try_recv() {
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                finished = true;
                break;
            }
            Ok(RunMsg::Started(name)) => run.current = name,
            Ok(RunMsg::Finished) => {
                finished = true;
                break;
            }
            Ok(RunMsg::Failed(name, e)) => {
                run.done += 1;
                state.swmm_doc.scenarios.results.push(ScenarioResult::failed(&name, e));
            }
            Ok(RunMsg::Done(name, r)) => {
                run.done += 1;
                let mut res = ScenarioResult::from_report(&name, &r.report, r.succeeded());
                res.elapsed_ms = r.elapsed.as_millis();
                if !r.succeeded() {
                    res.error = r.failure_reason();
                }
                state.swmm_doc.scenarios.results.push(res);
                record_run_scenario(&mut state.swmm_doc.run_history, &r, &label, Some(&name));
            }
        }
    }
    if finished {
        let n = state.swmm_doc.scenarios.results.len();
        state.swmm_doc.scenarios.run = None;
        state.swmm_doc.scenarios.results_open = true;
        state.status = format!("{n} scenario run(s) finished; see Results → Compare Runs…");
    }
}

pub fn results_csv(state: &AppState) -> String {
    scenario::results_csv(&state.swmm_doc.scenarios.results)
}

// --- menu -----------------------------------------------------------------------

/// `Tools` menu entries.
pub fn tools_menu_items(ui: &mut Ui, state: &mut AppState) {
    if ui
        .add_enabled(state.swmm_doc.loaded, Button::new("Scenarios…"))
        .on_disabled_hover_text("Open a model first")
        .clicked()
    {
        open(state);
        ui.close_menu();
    }
}

// --- windows --------------------------------------------------------------------

fn draw_main(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.scenarios.open {
        return;
    }
    ensure_loaded(state);
    refresh_stale(state);
    let dark = ctx.style().visuals.dark_mode;
    let mut open = true;
    let mut action: Option<Action> = None;
    egui::Window::new("Scenarios")
        .open(&mut open)
        .default_width(560.0)
        .resizable(true)
        .show(ctx, |ui| {
            let model = model_path(state)
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "unsaved model (save it to keep scenarios)".into());
            ui.label(RichText::new(format!("Model: {model}")).small());
            let sc = &mut state.swmm_doc.scenarios;
            match sc.active_name() {
                Some(n) => {
                    ui.label(
                        RichText::new(format!("Active: {n} — every edit is recorded into it"))
                            .color(palette::accent_text(dark)),
                    );
                }
                None => {
                    ui.label(RichText::new("Active: base model").color(palette::muted_text(dark)));
                }
            }
            ui.separator();
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.label(RichText::new("Scenarios").strong());
                egui::ScrollArea::vertical()
                    .id_salt("swmm-scenario-list")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        let sc = &mut state.swmm_doc.scenarios;
                        if ui.selectable_label(sc.selected.is_none(), "Base model").clicked() {
                            sc.selected = None;
                        }
                        for i in 0..sc.set.scenarios.len() {
                            let mark = if sc.is_active(i) { "● " } else { "" };
                            let n = sc.set.scenarios[i].edits.len();
                            let text = format!("{mark}{} ({n} edit{})", sc.set.scenarios[i].name, if n == 1 { "" } else { "s" });
                            if ui.selectable_label(sc.selected == Some(i), text).clicked() {
                                sc.selected = Some(i);
                                sc.renaming = false;
                            }
                        }
                    });
                let ui = &mut cols[1];
                let sc = &state.swmm_doc.scenarios;
                let has_sel = sc.selected.is_some();
                let busy = sc.is_running();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("New").clicked() {
                        action = Some(Action::New);
                    }
                    if ui.add_enabled(has_sel, Button::new("Duplicate")).clicked() {
                        action = Some(Action::Duplicate);
                    }
                    if ui.add_enabled(has_sel, Button::new("Rename")).clicked() {
                        action = Some(Action::StartRename);
                    }
                    if ui.add_enabled(has_sel, Button::new("Delete")).clicked() {
                        action = Some(Action::Delete);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let sel_active = sc.selected.is_some_and(|i| sc.is_active(i));
                    if ui
                        .add_enabled(has_sel && !sel_active, Button::new("Activate"))
                        .on_hover_text("Work on base + this scenario; edits are recorded into it")
                        .clicked()
                    {
                        action = Some(Action::Activate);
                    }
                    if ui
                        .add_enabled(sc.active.is_some(), Button::new("Deactivate"))
                        .on_hover_text("Back to the base model")
                        .clicked()
                    {
                        action = Some(Action::Deactivate);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let engine_ok = state.swmm.engine().is_some();
                    if ui
                        .add_enabled(engine_ok && !busy, Button::new("Run All"))
                        .on_disabled_hover_text("Choose an engine in the Run menu")
                        .on_hover_text("Run the base and every scenario; results go to Compare Runs")
                        .clicked()
                    {
                        action = Some(Action::RunAll);
                    }
                    if ui.add_enabled(!state.swmm_doc.scenarios.results.is_empty(), Button::new("Results…")).clicked() {
                        action = Some(Action::ShowResults);
                    }
                    if ui.button("Save").on_hover_text("Write the .scenarios.json now").clicked() {
                        action = Some(Action::Save);
                    }
                });
                let sc = &mut state.swmm_doc.scenarios;
                if sc.renaming {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut sc.rename_draft)
                                .id(egui::Id::new("swmm-scenario-rename"))
                                .desired_width(160.0),
                        );
                        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.button("OK").clicked() || enter {
                            action = Some(Action::FinishRename);
                        }
                        if ui.button("Cancel").clicked() {
                            sc.renaming = false;
                        }
                    });
                }
            });
            ui.separator();
            let sc = &mut state.swmm_doc.scenarios;
            match sc.selected {
                None => {
                    ui.label("The base model: the file as saved. Activate a scenario to edit on top of it.");
                }
                Some(i) if i < sc.set.scenarios.len() => {
                    ui.label(RichText::new("Description").strong());
                    let before = sc.set.scenarios[i].description.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut sc.set.scenarios[i].description)
                            .id(egui::Id::new("swmm-scenario-description"))
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                    if sc.set.scenarios[i].description != before {
                        action = Some(Action::Save);
                    }
                    let n = sc.set.scenarios[i].edits.len();
                    let stale = sc.stale.len();
                    ui.label(RichText::new(format!(
                        "Edits: {n}{}",
                        if stale > 0 { format!(" — {stale} stale (the base no longer has what they change)") } else { String::new() }
                    )).strong());
                    egui::ScrollArea::vertical()
                        .id_salt("swmm-scenario-edits")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for (k, e) in sc.set.scenarios[i].edits.iter().enumerate() {
                                let stale = sc.stale.iter().find(|s| s.index == k);
                                let text = e.describe();
                                match stale {
                                    Some(s) => {
                                        ui.label(RichText::new(format!("⚠ {text}")).color(palette::error_text(dark)))
                                            .on_hover_text(&s.reason);
                                    }
                                    None => {
                                        ui.label(text);
                                    }
                                }
                            }
                        });
                }
                Some(_) => {
                    sc.selected = None;
                }
            }
            if !state.swmm_doc.scenarios.message.is_empty() {
                ui.separator();
                ui.label(RichText::new(state.swmm_doc.scenarios.message.clone()).small());
            }
        });
    state.swmm_doc.scenarios.open = open;
    if let Some(a) = action {
        perform(state, a);
    }
}

enum Action {
    New,
    Duplicate,
    StartRename,
    FinishRename,
    Delete,
    Activate,
    Deactivate,
    RunAll,
    ShowResults,
    Save,
}

fn perform(state: &mut AppState, a: Action) {
    match a {
        Action::New => {
            let name = state.swmm_doc.scenarios.set.free_name("Scenario");
            new_scenario(state, &name);
            state.swmm_doc.scenarios.rename_draft = name;
            state.swmm_doc.scenarios.renaming = true;
        }
        Action::Duplicate => duplicate_selected(state),
        Action::StartRename => {
            let sc = &mut state.swmm_doc.scenarios;
            if let Some(s) = sc.selected.and_then(|i| sc.set.scenarios.get(i)) {
                sc.rename_draft = s.name.clone();
                sc.renaming = true;
            }
        }
        Action::FinishRename => {
            let name = state.swmm_doc.scenarios.rename_draft.clone();
            match rename_selected(state, &name) {
                Ok(()) => state.swmm_doc.scenarios.renaming = false,
                Err(e) => state.swmm_doc.scenarios.message = e,
            }
        }
        Action::Delete => delete_selected(state),
        Action::Activate => {
            if let Some(i) = state.swmm_doc.scenarios.selected {
                if let Err(e) = activate(state, i) {
                    state.swmm_doc.scenarios.message = format!("Could not activate: {e}");
                }
            }
        }
        Action::Deactivate => deactivate(state),
        Action::RunAll => {
            if let Some(e) = state.swmm.engine().cloned() {
                run_all(state, &e);
            }
        }
        Action::ShowResults => state.swmm_doc.scenarios.results_open = true,
        Action::Save => {
            save_set(state);
        }
    }
}

fn draw_progress(ctx: &egui::Context, state: &mut AppState) {
    let Some(run) = state.swmm_doc.scenarios.run.as_ref() else { return };
    let (done, total, current) = (run.done, run.total, run.current.clone());
    let stop = run.stop.clone();
    egui::Window::new("Running Scenarios")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{done} of {total} done — running {current}"));
            });
            ui.add(egui::ProgressBar::new(if total == 0 { 0.0 } else { done as f32 / total as f32 }).show_percentage());
            if ui
                .add_enabled(!stop.load(Ordering::Relaxed), Button::new("Stop after this run"))
                .clicked()
            {
                stop.store(true, Ordering::Relaxed);
            }
        });
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map(|v| format!("{v:.*}", digits)).unwrap_or_else(|| "—".into())
}

fn draw_results(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.scenarios.results_open {
        return;
    }
    let dark = ctx.style().visuals.dark_mode;
    let mut open = true;
    let mut export = false;
    egui::Window::new("Scenario Results")
        .open(&mut open)
        .default_width(720.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Peak outfall flow, outfall volume and flooding volume from each run's report; continuity errors in percent.").small());
            });
            if ui.button("Export CSV…").clicked() {
                export = true;
            }
            ui.separator();
            egui::ScrollArea::both().id_salt("swmm-scenario-results").max_height(320.0).show(ui, |ui| {
                egui::Grid::new("swmm-scenario-results-grid").striped(true).show(ui, |ui| {
                    for h in ["Scenario", "OK", "Peak outfall flow", "Outfall volume", "Flooding volume", "Runoff cont. %", "Routing cont. %", "ms", "Error"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in &state.swmm_doc.scenarios.results {
                        ui.label(&r.scenario);
                        if r.succeeded {
                            ui.label(RichText::new("yes").color(palette::ok_text(dark)));
                        } else {
                            ui.label(RichText::new("no").color(palette::error_text(dark)));
                        }
                        ui.monospace(opt(r.peak_outfall_flow, 3));
                        ui.monospace(opt(r.outfall_volume, 4));
                        ui.monospace(opt(r.flooding_volume, 4));
                        ui.monospace(opt(r.runoff_continuity_pct, 3));
                        ui.monospace(opt(r.routing_continuity_pct, 3));
                        ui.monospace(r.elapsed_ms.to_string());
                        ui.label(r.error.clone().unwrap_or_default());
                        ui.end_row();
                    }
                });
            });
            ui.label(RichText::new("Every run is in Results → Compare Runs… with its scenario name.").small());
        });
    state.swmm_doc.scenarios.results_open = open;
    if export {
        let text = results_csv(state);
        if let Some(s) = crate::swmm_export::save_csv("scenarios.csv", &text) {
            state.status = s;
        }
    }
}

/// Windows and dialogs. Also records edits into the active scenario and
/// polls the batch run, once per frame.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    check_model_switch(state);
    record(state);
    poll_run(state);
    if state.swmm_doc.scenarios.is_running() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    draw_main(ctx, state);
    draw_progress(ctx, state);
    draw_results(ctx, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;
    use stormsewer_swmm::doc::Command;

    fn scratch_model(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-app-scenario-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("Pond-{tag}-{}.inp", std::process::id()));
        std::fs::write(&p, fixture_text("Detention_Pond_Model.inp")).unwrap();
        let _ = std::fs::remove_file(ScenarioSet::sidecar_path(&p));
        p
    }

    fn pond_app(path: Option<PathBuf>) -> StormSewerApp {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), path);
        app
    }

    #[test]
    fn scenario_window_records_edits_while_active_and_restores_the_base_exactly() {
        let model = scratch_model("window");
        let mut app = pond_app(Some(model.clone()));
        // The Tools menu entry draws, and opens the window.
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| tools_menu_items(ui, &mut state));
        });
        app.state = state;
        open(&mut app.state);
        assert!(app.state.swmm_doc.scenarios.open);
        run_frame(&mut app);
        let base_text = app.state.swmm_doc.doc.to_string();

        let i = new_scenario(&mut app.state, "Wider S1");
        assert_eq!(i, 0);
        assert!(ScenarioSet::sidecar_path(&model).is_file(), "the sidecar is written on add");
        run_frame(&mut app);
        activate(&mut app.state, 0).unwrap();
        assert_eq!(app.state.swmm_doc.scenarios.active_name(), Some("Wider S1"));
        assert_eq!(
            app.state.swmm_doc.path.as_ref().unwrap().file_name().unwrap().to_string_lossy(),
            format!("Pond-window-{}.scenario.Wider_S1.inp", std::process::id())
        );
        assert!(!app.state.swmm_doc.dirty(), "base + empty scenario starts clean");
        // An edit through the editor is recorded on the next frame.
        assert!(app.state.swmm_doc.apply(
            Command::SetField {
                section: "SUBCATCHMENTS".into(),
                name: "S1".into(),
                field: "Width".into(),
                value: "2500".into(),
            },
            "width",
        ));
        run_frame(&mut app);
        let s = &app.state.swmm_doc.scenarios.set.scenarios[0];
        assert_eq!(s.edits.len(), 1, "{:?}", s.edits);
        assert!(matches!(&s.edits[0], stormsewer_swmm::scenario::ScenarioEdit::SetFields { section, name, fields } if section == "SUBCATCHMENTS" && name == "S1" && fields[5] == "2500"));
        let on_disk = ScenarioSet::load_for(&model).unwrap();
        assert_eq!(on_disk.scenarios[0].edits, s.edits, "recorded to the sidecar as it happens");
        // A second edit and an undo: the recording follows.
        assert!(app.state.swmm_doc.apply(Command::MoveNode { name: "J1".into(), x: 1.0, y: 2.0 }, "move"));
        run_frame(&mut app);
        assert_eq!(app.state.swmm_doc.scenarios.set.scenarios[0].edits.len(), 2);
        app.state.swmm_doc.undo();
        run_frame(&mut app);
        assert_eq!(app.state.swmm_doc.scenarios.set.scenarios[0].edits.len(), 1);
        // Deactivate: the base is back, byte for byte, with its path.
        deactivate(&mut app.state);
        assert!(app.state.swmm_doc.scenarios.active.is_none());
        assert_eq!(app.state.swmm_doc.doc.to_string(), base_text);
        assert_eq!(app.state.swmm_doc.path.as_deref(), Some(model.as_path()));
        assert!(!app.state.swmm_doc.dirty());
        assert_eq!(app.state.swmm_doc.doc.field("SUBCATCHMENTS", "S1", "Width"), Some("1587"));
        // Re-activating replays the recorded edit.
        activate(&mut app.state, 0).unwrap();
        assert_eq!(app.state.swmm_doc.doc.field("SUBCATCHMENTS", "S1", "Width"), Some("2500"));
        run_frame(&mut app);
        // Duplicate, rename, delete.
        duplicate_selected(&mut app.state);
        assert_eq!(app.state.swmm_doc.scenarios.set.scenarios.len(), 2);
        assert_eq!(app.state.swmm_doc.scenarios.selected, Some(1));
        assert!(rename_selected(&mut app.state, "wider s1").is_err(), "name clash");
        rename_selected(&mut app.state, "Copy").unwrap();
        assert_eq!(app.state.swmm_doc.scenarios.set.scenarios[1].name, "Copy");
        app.state.swmm_doc.scenarios.selected = Some(0);
        delete_selected(&mut app.state);
        assert!(app.state.swmm_doc.scenarios.active.is_none(), "deleting the active scenario deactivates");
        assert_eq!(app.state.swmm_doc.doc.to_string(), base_text);
        assert_eq!(app.state.swmm_doc.scenarios.set.scenarios.len(), 1);
        // Stale detection shows in the window: delete S1 from the base.
        app.state.swmm_doc.scenarios.set.scenarios[0].edits = vec![stormsewer_swmm::scenario::ScenarioEdit::SetField {
            section: "SUBCATCHMENTS".into(),
            name: "S1".into(),
            field: "Width".into(),
            value: "1".into(),
        }];
        app.state.swmm_doc.scenarios.selected = Some(0);
        assert!(app.state.swmm_doc.apply(Command::DeleteObject { section: "SUBCATCHMENTS".into(), name: "S1".into() }, "del"));
        run_frame(&mut app);
        assert_eq!(app.state.swmm_doc.scenarios.stale.len(), 1);
        assert!(activate(&mut app.state, 0).is_err(), "a stale scenario does not activate");
        // Renaming flow and the results window draw.
        app.state.swmm_doc.scenarios.renaming = true;
        app.state.swmm_doc.scenarios.results_open = true;
        app.state.swmm_doc.scenarios.results.push(ScenarioResult::failed("x", "boom"));
        run_frame(&mut app);
        assert!(results_csv(&app.state).contains("x,no"));
        // Opening another model drops an activation.
        app.state.swmm_doc.scenarios.set.scenarios[0].edits.clear();
        app.state.swmm_doc.scenarios.stale_for = None;
        activate(&mut app.state, 0).unwrap();
        app.state.swmm_doc.open_text("[TITLE]\nother\n", None);
        run_frame(&mut app);
        assert!(app.state.swmm_doc.scenarios.active.is_none());
        assert!(app.state.swmm_doc.scenarios.message.contains("another model"));
        let _ = std::fs::remove_file(ScenarioSet::sidecar_path(&model));
        let _ = std::fs::remove_file(&model);
    }

    #[test]
    fn unsaved_model_keeps_scenarios_in_memory_only() {
        let mut app = pond_app(None);
        open(&mut app.state);
        new_scenario(&mut app.state, "A");
        assert!(app.state.swmm_doc.scenarios.message.contains("Save the model first"));
        activate(&mut app.state, 0).unwrap();
        assert!(app.state.swmm_doc.path.is_none());
        run_frame(&mut app);
        deactivate(&mut app.state);
        assert!(app.state.swmm_doc.path.is_none());
    }

    /// Run All against a real engine, when one is registered.
    #[test]
    fn run_all_records_base_and_scenarios_when_an_engine_is_present() {
        let model = scratch_model("runall");
        let mut app = pond_app(Some(model.clone()));
        app.state.swmm.ensure_discovered();
        let Some(engine) = app.state.swmm.engine().cloned() else {
            eprintln!("skipped: no SWMM engine discovered");
            return;
        };
        open(&mut app.state);
        new_scenario(&mut app.state, "Rough");
        app.state.swmm_doc.scenarios.set.scenarios[0].edits = vec![stormsewer_swmm::scenario::ScenarioEdit::SetField {
            section: "CONDUITS".into(),
            name: "C1".into(),
            field: "Roughness".into(),
            value: "0.1".into(),
        }];
        run_all(&mut app.state, &engine);
        assert!(app.state.swmm_doc.scenarios.is_running());
        let started = std::time::Instant::now();
        while app.state.swmm_doc.scenarios.is_running() {
            assert!(started.elapsed().as_secs() < 120, "the runs did not finish");
            std::thread::sleep(std::time::Duration::from_millis(100));
            run_frame(&mut app);
        }
        let results = &app.state.swmm_doc.scenarios.results;
        assert_eq!(results.len(), 2, "{results:?}");
        assert!(results.iter().all(|r| r.succeeded), "{results:?}");
        assert_eq!(results[0].scenario, "base");
        assert_eq!(results[1].scenario, "Rough");
        assert!(results[0].peak_outfall_flow.is_some());
        let labels: Vec<Option<String>> = app.state.swmm_doc.run_history.iter().map(|r| r.scenario.clone()).collect();
        assert_eq!(labels, vec![Some("base".into()), Some("Rough".into())]);
        assert!(app.state.swmm_doc.scenarios.results_open);
        crate::swmm_compare::open(&mut app.state);
        assert!(app.state.swmm_compare.diff.is_some());
        run_frame(&mut app);
        let _ = std::fs::remove_file(ScenarioSet::sidecar_path(&model));
        let _ = std::fs::remove_file(&model);
    }
}
