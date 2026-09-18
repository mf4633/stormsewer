// SPDX-License-Identifier: GPL-3.0-or-later

//! Tools → Calibration…: observed series, the parameters the optimiser may
//! change, the objective, a sensitivity screen, and DDS over batch engine
//! runs — with a progress window, Stop, Apply Best as one undo step, Save
//! as Scenario, and a report. The setup lives in `<stem>.calib.json`
//! beside the model (`stormsewer_swmm::calib::CalibSetup`).
//!
//! Every engine evaluation runs on a worker thread in its own scratch
//! folder, so `threads` evaluations proceed at once; the window polls a
//! channel once per frame.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use eframe::egui::{self, Button, RichText, Ui};
use stormsewer_swmm::calib::observed::{parse_observed, ObsTime};
use stormsewer_swmm::calib::params::{apply_set, suggestions};
use stormsewer_swmm::calib::{
    self, rating, CalibResult, CalibSetup, EngineEvaluator, Evaluator, Group, MorrisRow,
    Objective, ObservedSeries, Parameter, Progress, TornadoRow, Transform, Variable,
};
use stormsewer_swmm::scenario::Scenario;

use crate::state::AppState;
use crate::theme::palette;

/// Which runner a worker is executing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    Dds,
    Oat,
    Morris,
}

impl RunKind {
    fn label(self) -> &'static str {
        match self {
            Self::Dds => "DDS calibration",
            Self::Oat => "one-at-a-time sensitivity",
            Self::Morris => "Morris screening",
        }
    }
}

struct CalibRun {
    rx: Receiver<Progress>,
    stop: Arc<AtomicBool>,
    kind: RunKind,
    done: usize,
    total: usize,
    best_objective: f64,
    best_x: Vec<f64>,
    history: Vec<(usize, f64)>,
    failed: usize,
    last_error: Option<String>,
    started: std::time::Instant,
}

/// How a draft parameter picks its objects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroupKind {
    #[default]
    All,
    Selection,
    Tag,
    Object,
}

/// The parameter being composed in the window.
#[derive(Clone, Debug)]
pub struct ParamDraft {
    pub name: String,
    pub section: String,
    pub column: String,
    pub group: GroupKind,
    pub group_text: String,
    pub transform: Transform,
    pub lower: f64,
    pub upper: f64,
}

impl Default for ParamDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            section: "SUBCATCHMENTS".into(),
            column: "Width".into(),
            group: GroupKind::All,
            group_text: String::new(),
            transform: Transform::Multiply,
            lower: 0.5,
            upper: 2.0,
        }
    }
}

/// Sections a parameter may edit, offered in the section picker.
pub const SECTIONS: [&str; 9] = [
    "SUBCATCHMENTS",
    "SUBAREAS",
    "INFILTRATION",
    "JUNCTIONS",
    "STORAGE",
    "OUTFALLS",
    "CONDUITS",
    "AQUIFERS",
    "GROUNDWATER",
];

/// Per-editor calibration state.
pub struct CalibState {
    pub open: bool,
    pub setup: CalibSetup,
    loaded_for: Option<Option<PathBuf>>,
    pub import_text: String,
    pub import_id: String,
    pub import_variable: Variable,
    pub import_name: String,
    pub draft: ParamDraft,
    pub suggestion: usize,
    run: Option<CalibRun>,
    pub result: Option<CalibResult>,
    pub tornado: Vec<TornadoRow>,
    pub morris: Vec<MorrisRow>,
    pub progress_open: bool,
    pub report_open: bool,
    pub report_text: String,
    pub message: String,
}

impl Default for CalibState {
    fn default() -> Self {
        Self {
            open: false,
            setup: CalibSetup::default(),
            loaded_for: None,
            import_text: String::new(),
            import_id: String::new(),
            import_variable: Variable::SystemInflow,
            import_name: String::new(),
            draft: ParamDraft::default(),
            suggestion: 0,
            run: None,
            result: None,
            tornado: Vec::new(),
            morris: Vec::new(),
            progress_open: false,
            report_open: false,
            report_text: String::new(),
            message: String::new(),
        }
    }
}

impl CalibState {
    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }
}

/// Load the sidecar for the open model, once per model.
pub fn ensure_loaded(state: &mut AppState) {
    let path = crate::swmm_scenarios::model_path(state);
    let c = &mut state.swmm_doc.calib;
    if c.loaded_for.as_ref() == Some(&path) {
        return;
    }
    c.loaded_for = Some(path.clone());
    c.result = None;
    c.tornado.clear();
    c.morris.clear();
    c.setup = match &path {
        Some(p) => match CalibSetup::load_for(p) {
            Ok(s) => s,
            Err(e) => {
                c.message = format!("Could not read the calibration file: {e}");
                CalibSetup::default()
            }
        },
        None => CalibSetup::default(),
    };
}

/// Write the sidecar beside the model. Returns whether it could.
pub fn save_setup(state: &mut AppState) -> bool {
    let Some(path) = crate::swmm_scenarios::model_path(state) else {
        state.swmm_doc.calib.message =
            "Save the model first: the calibration setup is stored beside it.".into();
        return false;
    };
    match state.swmm_doc.calib.setup.save_for(&path) {
        Ok(()) => {
            state.swmm_doc.calib.message =
                format!("Saved {}", CalibSetup::sidecar_path(&path).display());
            true
        }
        Err(e) => {
            state.swmm_doc.calib.message = format!("Could not save the setup: {e}");
            false
        }
    }
}

pub fn open(state: &mut AppState) {
    ensure_loaded(state);
    state.swmm_doc.calib.open = true;
}

/// Parse the pasted text and attach it to the chosen object.
pub fn add_observed_from_text(state: &mut AppState) -> Result<(), String> {
    let c = &mut state.swmm_doc.calib;
    let id = c.import_id.trim().to_string();
    if id.is_empty() {
        return Err("name the model object the observations belong to".into());
    }
    let imp = parse_observed(&c.import_text)?;
    let name = if c.import_name.trim().is_empty() {
        format!("{id} observed")
    } else {
        c.import_name.trim().to_string()
    };
    let n = imp.values.len();
    c.setup.observed.push(ObservedSeries {
        name,
        id,
        variable: c.import_variable,
        times: imp.times,
        values: imp.values,
        source: "pasted / loaded text".into(),
    });
    c.message = format!(
        "Added {n} observation(s){}",
        if imp.warnings.is_empty() {
            String::new()
        } else {
            format!("; {}", imp.warnings.join("; "))
        }
    );
    c.import_text.clear();
    Ok(())
}

/// Read the chosen object and variable from the last run's results as an
/// observed series (for synthetic tests and tutorials).
pub fn add_observed_from_last_run(state: &mut AppState) -> Result<(), String> {
    let Some(file) = state.swmm.results.as_ref() else {
        return Err("run the model first".into());
    };
    let c = &mut state.swmm_doc.calib;
    let id = c.import_id.trim().to_string();
    if id.is_empty() {
        return Err("name the model object".into());
    }
    let series = c.import_variable.read(file, &id).map_err(|e| e.to_string())?;
    let start = file.meta.start_days;
    let name = if c.import_name.trim().is_empty() {
        format!("{id} from run")
    } else {
        c.import_name.trim().to_string()
    };
    c.setup.observed.push(ObservedSeries {
        name,
        id,
        variable: c.import_variable,
        times: series
            .times_s
            .iter()
            .map(|t| ObsTime::Absolute(start + t / 86_400.0))
            .collect(),
        values: series.values,
        source: "last run".into(),
    });
    c.message = "Added the last run's series as observed".into();
    Ok(())
}

/// Turn the draft into a parameter.
pub fn draft_parameter(state: &AppState) -> Result<Parameter, String> {
    let d = &state.swmm_doc.calib.draft;
    if d.column.trim().is_empty() {
        return Err("name the column".into());
    }
    if !(d.lower.is_finite() && d.upper.is_finite()) || d.lower >= d.upper {
        return Err("lower must be below upper".into());
    }
    let group = match d.group {
        GroupKind::All => Group::All,
        GroupKind::Tag => {
            if d.group_text.trim().is_empty() {
                return Err("give the tag".into());
            }
            Group::Tag(d.group_text.trim().to_string())
        }
        GroupKind::Object => {
            if d.group_text.trim().is_empty() {
                return Err("name the object".into());
            }
            Group::Object(d.group_text.trim().to_string())
        }
        GroupKind::Selection => {
            let names: Vec<String> = state
                .swmm_doc
                .selected_rows()
                .into_iter()
                .filter(|(sec, _)| sec.eq_ignore_ascii_case(&d.section) || section_object_match(sec, &d.section))
                .map(|(_, n)| n)
                .collect();
            if names.is_empty() {
                return Err(format!("select objects of [{}] on the map first", d.section));
            }
            Group::Selection(names)
        }
    };
    let name = if d.name.trim().is_empty() {
        format!("{} {} ({})", d.section, d.column, group.label())
    } else {
        d.name.trim().to_string()
    };
    Ok(Parameter::new(
        &name,
        &d.section,
        d.column.trim(),
        group,
        d.transform,
        d.lower,
        d.upper,
    ))
}

/// A selected subcatchment also serves `[SUBAREAS]`/`[INFILTRATION]`.
fn section_object_match(defining: &str, target: &str) -> bool {
    defining == "SUBCATCHMENTS"
        && matches!(target, "SUBAREAS" | "INFILTRATION" | "GROUNDWATER" | "LID_USAGE")
}

pub fn add_draft_parameter(state: &mut AppState) -> Result<(), String> {
    let p = draft_parameter(state)?;
    let members = p.members(&state.swmm_doc.doc).len();
    if members == 0 {
        return Err(format!("no objects of [{}] match", p.section));
    }
    state.swmm_doc.calib.setup.parameters.push(p);
    state.swmm_doc.calib.message = format!("Added a parameter over {members} object(s)");
    Ok(())
}

fn evaluator(state: &AppState) -> Result<(Arc<EngineEvaluator>, (String, String)), String> {
    let engine = state
        .swmm
        .engine()
        .cloned()
        .ok_or_else(|| "choose an engine in the Run menu".to_string())?;
    let c = &state.swmm_doc.calib;
    if c.setup.observed.is_empty() {
        return Err("add at least one observed series".into());
    }
    if c.setup.parameters.is_empty() {
        return Err("add at least one parameter".into());
    }
    if state.swmm_doc.error_count() > 0 {
        return Err(format!(
            "the model has {} error(s); fix them first (Run → Check Model)",
            state.swmm_doc.error_count()
        ));
    }
    let model = crate::swmm_scenarios::model_path(state)
        .unwrap_or_else(|| PathBuf::from(state.swmm_doc.file_name()));
    let ev = EngineEvaluator::new(
        engine.clone(),
        model,
        state.swmm_doc.doc.to_string(),
        c.setup.parameters.clone(),
        c.setup.observed.clone(),
        c.setup.objective,
    );
    Ok((Arc::new(ev), (engine.version, engine.sha256)))
}

fn begin(state: &mut AppState, kind: RunKind, total: usize, rx: Receiver<Progress>, stop: Arc<AtomicBool>) {
    state.swmm_doc.calib.run = Some(CalibRun {
        rx,
        stop,
        kind,
        done: 0,
        total,
        best_objective: f64::NAN,
        best_x: Vec::new(),
        history: Vec::new(),
        failed: 0,
        last_error: None,
        started: std::time::Instant::now(),
    });
    state.swmm_doc.calib.progress_open = true;
    state.status = format!("Started {}", kind.label());
}

/// Start DDS on a worker thread.
pub fn start_dds(state: &mut AppState) -> Result<(), String> {
    if state.swmm_doc.calib.is_running() {
        return Err("a run is in progress".into());
    }
    let (ev, engine) = evaluator(state)?;
    let setup = state.swmm_doc.calib.setup.clone();
    let x0 = setup.identity(&state.swmm_doc.doc);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let stop_w = stop.clone();
    let ev: Arc<dyn Evaluator> = ev;
    std::thread::spawn(move || {
        calib::run_dds(
            ev,
            setup.objective,
            setup.parameters,
            x0,
            setup.max_evals.max(1),
            setup.seed,
            setup.threads.max(1),
            engine,
            stop_w,
            tx,
        );
    });
    let total = state.swmm_doc.calib.setup.max_evals.max(1);
    begin(state, RunKind::Dds, total, rx, stop);
    Ok(())
}

/// Start the one-at-a-time screen on a worker thread.
pub fn start_oat(state: &mut AppState) -> Result<(), String> {
    if state.swmm_doc.calib.is_running() {
        return Err("a run is in progress".into());
    }
    let (ev, _) = evaluator(state)?;
    let setup = state.swmm_doc.calib.setup.clone();
    let x0 = setup.identity(&state.swmm_doc.doc);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let stop_w = stop.clone();
    let ev: Arc<dyn Evaluator> = ev;
    let total = 2 * setup.parameters.len();
    std::thread::spawn(move || {
        calib::run_oat(ev, &setup.parameters, &x0, setup.oat_pct, setup.threads.max(1), stop_w, tx);
    });
    begin(state, RunKind::Oat, total, rx, stop);
    Ok(())
}

/// Start Morris screening on a worker thread.
pub fn start_morris(state: &mut AppState) -> Result<(), String> {
    if state.swmm_doc.calib.is_running() {
        return Err("a run is in progress".into());
    }
    let (ev, _) = evaluator(state)?;
    let setup = state.swmm_doc.calib.setup.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let stop_w = stop.clone();
    let ev: Arc<dyn Evaluator> = ev;
    let total = setup.morris_r.max(1) * (setup.parameters.len() + 1);
    std::thread::spawn(move || {
        calib::run_morris(
            ev,
            &setup.parameters,
            setup.morris_r.max(1),
            setup.morris_p.max(2),
            setup.seed,
            setup.threads.max(1),
            stop_w,
            tx,
        );
    });
    begin(state, RunKind::Morris, total, rx, stop);
    Ok(())
}

pub fn stop(state: &mut AppState) {
    if let Some(r) = state.swmm_doc.calib.run.as_ref() {
        r.stop.store(true, Ordering::Relaxed);
        state.status = "Stopping after the current evaluations…".into();
    }
}

fn poll(state: &mut AppState) {
    let model = state.swmm_doc.file_name();
    let mut finished = false;
    loop {
        let Some(run) = state.swmm_doc.calib.run.as_mut() else { return };
        match run.rx.try_recv() {
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                finished = true;
                break;
            }
            Ok(Progress::Evaluated { done, total, best_objective, best_x, error, .. }) => {
                run.done = done;
                run.total = total;
                if error.is_some() {
                    run.failed += 1;
                    run.last_error = error;
                }
                if run.kind == RunKind::Dds {
                    run.best_objective = best_objective;
                    run.best_x = best_x;
                    run.history.push((done, best_objective));
                }
            }
            Ok(Progress::Finished(r)) => {
                state.swmm_doc.calib.report_text = calib::report_markdown(&r, &model);
                state.swmm_doc.calib.result = Some(*r);
                finished = true;
                break;
            }
            Ok(Progress::Tornado(rows)) => {
                state.swmm_doc.calib.tornado = rows;
                finished = true;
                break;
            }
            Ok(Progress::Morris(rows)) => {
                state.swmm_doc.calib.morris = rows;
                finished = true;
                break;
            }
            Ok(Progress::Failed(e)) => {
                state.swmm_doc.calib.message = e;
                finished = true;
                break;
            }
        }
    }
    if finished {
        let c = &mut state.swmm_doc.calib;
        let kind = c.run.as_ref().map(|r| r.kind);
        let failed = c.run.as_ref().map(|r| r.failed).unwrap_or(0);
        c.run = None;
        c.progress_open = false;
        state.status = match kind {
            Some(k) => format!(
                "{} finished{}",
                k.label(),
                if failed > 0 { format!(" ({failed} evaluation(s) failed)") } else { String::new() }
            ),
            None => "Calibration finished".into(),
        };
    }
}

/// The best parameter set as one undo step on the working document.
pub fn apply_best(state: &mut AppState) -> bool {
    let Some(r) = state.swmm_doc.calib.result.clone() else {
        return false;
    };
    let batch = apply_set(&state.swmm_doc.doc, &r.parameters, &r.best_x);
    let ok = state.swmm_doc.apply(batch, "apply calibration");
    if ok {
        state.status = format!("Applied the calibrated parameters ({} = {:.4}); Ctrl+Z undoes them", r.objective.label(), r.best_objective);
    }
    ok
}

/// The best parameter set as a new scenario on the base model.
pub fn save_as_scenario(state: &mut AppState) -> Option<usize> {
    let r = state.swmm_doc.calib.result.clone()?;
    let batch = apply_set(&state.swmm_doc.doc, &r.parameters, &r.best_x);
    let mut s = Scenario::from_commands("Calibrated", &[batch]);
    s.description = format!(
        "DDS calibration: {} = {:.4} after {} evaluations (seed {}), EPA SWMM {}",
        r.objective.label(),
        r.best_objective,
        r.evaluations,
        r.seed,
        r.engine_version
    );
    let i = crate::swmm_scenarios::add_scenario(state, s);
    state.status = format!("Saved scenario {:?}", state.swmm_doc.scenarios.set.scenarios[i].name);
    Some(i)
}

// --- menu -----------------------------------------------------------------------

/// `Tools` menu entries.
pub fn tools_menu_items(ui: &mut Ui, state: &mut AppState) {
    if ui
        .add_enabled(state.swmm_doc.loaded, Button::new("Calibration…"))
        .on_disabled_hover_text("Open a model first")
        .clicked()
    {
        open(state);
        ui.close_menu();
    }
}

// --- windows --------------------------------------------------------------------

fn fmt(v: f64) -> String {
    if v.is_finite() {
        format!("{v:.4}")
    } else {
        "—".into()
    }
}

enum Action {
    AddObserved,
    LoadCsv,
    FromLastRun,
    RemoveObserved(usize),
    AddSuggestion,
    AddDraft,
    RemoveParam(usize),
    Oat,
    Morris,
    Dds,
    SaveSetup,
    ApplyBest,
    SaveScenario,
    Report,
    ExportTornado,
}

fn draw_observed(ui: &mut Ui, state: &mut AppState, action: &mut Option<Action>) {
    let c = &mut state.swmm_doc.calib;
    if c.setup.observed.is_empty() {
        ui.label(RichText::new("No observed series yet.").small());
    }
    let mut remove = None;
    egui::Grid::new("swmm-calib-observed").striped(true).show(ui, |ui| {
        for (i, o) in c.setup.observed.iter().enumerate() {
            ui.label(&o.name);
            ui.label(o.label());
            ui.label(format!("{} points", o.len()));
            if ui.button("Remove").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        *action = Some(Action::RemoveObserved(i));
    }
    ui.separator();
    ui.label(RichText::new("Add observed data").strong());
    ui.horizontal(|ui| {
        ui.label("Object");
        ui.add(egui::TextEdit::singleline(&mut c.import_id).id(egui::Id::new("swmm-calib-obs-id")).desired_width(100.0));
        ui.label("Variable");
        egui::ComboBox::from_id_salt("swmm-calib-obs-var")
            .selected_text(c.import_variable.label())
            .show_ui(ui, |ui| {
                for v in Variable::ALL {
                    ui.selectable_value(&mut c.import_variable, v, v.label());
                }
            });
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut c.import_name).id(egui::Id::new("swmm-calib-obs-name")).desired_width(120.0));
    });
    ui.label(RichText::new("Paste datetime,value rows (or date,time,value; time,value; seconds,value) in the model's units:").small());
    ui.add(
        egui::TextEdit::multiline(&mut c.import_text)
            .id(egui::Id::new("swmm-calib-obs-text"))
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .font(egui::TextStyle::Monospace),
    );
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!c.import_text.trim().is_empty(), Button::new("Add observed series")).clicked() {
            *action = Some(Action::AddObserved);
        }
        if ui.button("Load CSV…").clicked() {
            *action = Some(Action::LoadCsv);
        }
        if ui
            .add_enabled(state.swmm.results.is_some(), Button::new("From last run"))
            .on_hover_text("Use the last run's series for this object as the observations (synthetic tests)")
            .clicked()
        {
            *action = Some(Action::FromLastRun);
        }
    });
}

fn draw_parameters(ui: &mut Ui, state: &mut AppState, action: &mut Option<Action>) {
    let doc_suggestions = suggestions(&state.swmm_doc.doc);
    let c = &mut state.swmm_doc.calib;
    if c.setup.parameters.is_empty() {
        ui.label(RichText::new("No parameters yet: add a suggestion or compose one below.").small());
    }
    let mut remove = None;
    egui::ScrollArea::vertical().id_salt("swmm-calib-params").max_height(200.0).show(ui, |ui| {
        egui::Grid::new("swmm-calib-param-grid").striped(true).show(ui, |ui| {
            for h in ["Parameter", "Field", "Objects", "Transform", "Lower", "Upper", ""] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (i, p) in c.setup.parameters.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(120.0));
                ui.label(format!("[{}] {}", p.section, p.column));
                ui.label(p.group.label());
                egui::ComboBox::from_id_salt(("swmm-calib-transform", i))
                    .selected_text(p.transform.label())
                    .show_ui(ui, |ui| {
                        for t in Transform::ALL {
                            ui.selectable_value(&mut p.transform, t, t.label());
                        }
                    });
                ui.add(egui::DragValue::new(&mut p.lower).speed(0.01));
                ui.add(egui::DragValue::new(&mut p.upper).speed(0.01));
                if ui.button("Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = remove {
        *action = Some(Action::RemoveParam(i));
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label("Suggested");
        if c.suggestion >= doc_suggestions.len() {
            c.suggestion = 0;
        }
        egui::ComboBox::from_id_salt("swmm-calib-suggestion")
            .selected_text(doc_suggestions.get(c.suggestion).map(|p| p.name.as_str()).unwrap_or("—"))
            .width(200.0)
            .show_ui(ui, |ui| {
                for (i, p) in doc_suggestions.iter().enumerate() {
                    ui.selectable_value(&mut c.suggestion, i, format!("{} — [{}] {}", p.name, p.section, p.column));
                }
            });
        if ui.button("Add suggestion").clicked() {
            *action = Some(Action::AddSuggestion);
        }
    });
    ui.label(RichText::new("Compose a parameter").strong());
    let d = &mut c.draft;
    ui.horizontal_wrapped(|ui| {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut d.name).id(egui::Id::new("swmm-calib-draft-name")).desired_width(120.0));
        ui.label("Section");
        egui::ComboBox::from_id_salt("swmm-calib-draft-section")
            .selected_text(d.section.clone())
            .show_ui(ui, |ui| {
                for s in SECTIONS {
                    ui.selectable_value(&mut d.section, s.to_string(), s);
                }
            });
        ui.label("Column");
        ui.add(egui::TextEdit::singleline(&mut d.column).id(egui::Id::new("swmm-calib-draft-column")).desired_width(90.0))
            .on_hover_text("A column name from the property sheet (Width, NPerv, Roughness…) or a 0-based index");
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Objects");
        for (k, l) in [(GroupKind::All, "All"), (GroupKind::Selection, "Selection"), (GroupKind::Tag, "Tag"), (GroupKind::Object, "Object")] {
            if ui.selectable_label(d.group == k, l).clicked() {
                d.group = k;
            }
        }
        if matches!(d.group, GroupKind::Tag | GroupKind::Object) {
            ui.add(egui::TextEdit::singleline(&mut d.group_text).id(egui::Id::new("swmm-calib-draft-group")).desired_width(100.0));
        }
        ui.label("Transform");
        egui::ComboBox::from_id_salt("swmm-calib-draft-transform")
            .selected_text(d.transform.label())
            .show_ui(ui, |ui| {
                for t in Transform::ALL {
                    ui.selectable_value(&mut d.transform, t, t.label());
                }
            });
        ui.label("Lower");
        ui.add(egui::DragValue::new(&mut d.lower).speed(0.01));
        ui.label("Upper");
        ui.add(egui::DragValue::new(&mut d.upper).speed(0.01));
        if ui.button("Add parameter").clicked() {
            *action = Some(Action::AddDraft);
        }
    });
}

fn draw_settings(ui: &mut Ui, state: &mut AppState) {
    let c = &mut state.swmm_doc.calib;
    ui.horizontal_wrapped(|ui| {
        ui.label("Objective");
        egui::ComboBox::from_id_salt("swmm-calib-objective")
            .selected_text(c.setup.objective.label())
            .show_ui(ui, |ui| {
                for o in Objective::ALL {
                    ui.selectable_value(&mut c.setup.objective, o, o.label());
                }
            });
        ui.label("Evaluations");
        ui.add(egui::DragValue::new(&mut c.setup.max_evals).range(1..=100_000));
        ui.label("Seed");
        ui.add(egui::DragValue::new(&mut c.setup.seed));
        ui.label("Threads");
        ui.add(egui::DragValue::new(&mut c.setup.threads).range(1..=64));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("OAT swing %");
        ui.add(egui::DragValue::new(&mut c.setup.oat_pct).range(0.1..=100.0).speed(0.5));
        ui.label("Morris trajectories");
        ui.add(egui::DragValue::new(&mut c.setup.morris_r).range(1..=100));
        ui.label("levels");
        ui.add(egui::DragValue::new(&mut c.setup.morris_p).range(2..=20));
    });
}

fn draw_result(ui: &mut Ui, state: &mut AppState, action: &mut Option<Action>, dark: bool) {
    let c = &state.swmm_doc.calib;
    if !c.tornado.is_empty() {
        ui.label(RichText::new("Sensitivity (one at a time)").strong());
        egui::Grid::new("swmm-calib-tornado").striped(true).show(ui, |ui| {
            for h in ["Parameter", "x low", "x high", "f(low)", "f(high)", "Swing"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for r in &c.tornado {
                ui.label(&r.name);
                ui.monospace(fmt(r.x_low));
                ui.monospace(fmt(r.x_high));
                ui.monospace(fmt(r.f_low));
                ui.monospace(fmt(r.f_high));
                ui.monospace(fmt(r.swing));
                ui.end_row();
            }
        });
        if ui.button("Export tornado CSV…").clicked() {
            *action = Some(Action::ExportTornado);
        }
    }
    if !c.morris.is_empty() {
        ui.label(RichText::new("Morris elementary effects").strong());
        egui::Grid::new("swmm-calib-morris").striped(true).show(ui, |ui| {
            for h in ["Parameter", "μ*", "μ", "σ", "n"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for r in &c.morris {
                ui.label(&r.name);
                ui.monospace(fmt(r.mu_star));
                ui.monospace(fmt(r.mu));
                ui.monospace(fmt(r.sigma));
                ui.monospace(r.n_effects.to_string());
                ui.end_row();
            }
        });
    }
    let Some(r) = c.result.as_ref() else { return };
    ui.label(RichText::new(format!(
        "Best {} objective {} after {} evaluation(s){} — EPA SWMM {}",
        r.objective.label(),
        fmt(r.best_objective),
        r.evaluations,
        if r.stopped_early { " (stopped)" } else { "" },
        r.engine_version
    )).strong());
    egui::Grid::new("swmm-calib-best").striped(true).show(ui, |ui| {
        for h in ["Parameter", "Start", "Best"] {
            ui.label(RichText::new(h).strong());
        }
        ui.end_row();
        for (i, p) in r.parameters.iter().enumerate() {
            ui.label(&p.name);
            ui.monospace(r.x0.get(i).copied().map(fmt).unwrap_or_default());
            ui.monospace(r.best_x.get(i).copied().map(fmt).unwrap_or_default());
            ui.end_row();
        }
    });
    for fit in &r.fits {
        ui.label(RichText::new(&fit.name).strong());
        egui::Grid::new(("swmm-calib-fit", &fit.name)).striped(true).show(ui, |ui| {
            for o in Objective::ALL {
                let v = o.value(&fit.metrics);
                ui.label(o.label());
                ui.monospace(fmt(v));
                match rating(o, v) {
                    Some(rt) => {
                        let color = match rt {
                            calib::Rating::VeryGood | calib::Rating::Good => palette::ok_text(dark),
                            calib::Rating::Satisfactory => palette::warning_text(dark),
                            calib::Rating::Unsatisfactory => palette::error_text(dark),
                        };
                        ui.label(RichText::new(rt.label()).color(color));
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("Apply Best").on_hover_text("Set the working document to the best parameters as one undo step").clicked() {
            *action = Some(Action::ApplyBest);
        }
        if ui.button("Save as Scenario").clicked() {
            *action = Some(Action::SaveScenario);
        }
        if ui.button("Report…").clicked() {
            *action = Some(Action::Report);
        }
    });
}

fn draw_main(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.calib.open {
        return;
    }
    ensure_loaded(state);
    let dark = ctx.style().visuals.dark_mode;
    let mut open = true;
    let mut action: Option<Action> = None;
    egui::Window::new("Calibration")
        .open(&mut open)
        .default_width(640.0)
        .default_height(620.0)
        .resizable(true)
        .show(ctx, |ui| {
            // Size the body to the space the window actually has, and leave room
            // for the button row below it. A fixed cap larger than the window
            // never overflows, so it never scrolls, and everything below the
            // frame becomes unreachable.
            let body_h = (ui.available_height() - 48.0).max(120.0);
            egui::ScrollArea::vertical().id_salt("swmm-calib-main").max_height(body_h).show(ui, |ui| {
                ui.label(RichText::new(format!("Model: {}", state.swmm_doc.file_name())).small());
                egui::CollapsingHeader::new("Observed data").default_open(true).show(ui, |ui| draw_observed(ui, state, &mut action));
                egui::CollapsingHeader::new("Parameters").default_open(true).show(ui, |ui| draw_parameters(ui, state, &mut action));
                egui::CollapsingHeader::new("Objective and budget").default_open(true).show(ui, |ui| draw_settings(ui, state));
                if !state.swmm_doc.calib.message.is_empty() {
                    ui.label(RichText::new(state.swmm_doc.calib.message.clone()).small());
                }
                ui.separator();
                draw_result(ui, state, &mut action, dark);
            });
            // The run buttons stay out of the scrolling body: they are why the
            // window is open, so they must not scroll off the bottom.
            ui.separator();
            let busy = state.swmm_doc.calib.is_running();
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!busy, Button::new("Sensitivity (OAT)")).on_hover_text("Each parameter ±swing % around its current value").clicked() {
                    action = Some(Action::Oat);
                }
                if ui.add_enabled(!busy, Button::new("Morris screening")).clicked() {
                    action = Some(Action::Morris);
                }
                if ui.add_enabled(!busy, Button::new("Calibrate (DDS)")).clicked() {
                    action = Some(Action::Dds);
                }
                if ui.button("Save setup").on_hover_text("Write the .calib.json now").clicked() {
                    action = Some(Action::SaveSetup);
                }
            });
        });
    state.swmm_doc.calib.open = open;
    if let Some(a) = action {
        perform(state, a);
    }
}

fn perform(state: &mut AppState, a: Action) {
    let outcome: Result<(), String> = match a {
        Action::AddObserved => add_observed_from_text(state).map(|_| {
            save_setup(state);
        }),
        Action::LoadCsv => {
            match rfd::FileDialog::new().add_filter("Text", &["csv", "txt", "dat"]).pick_file() {
                Some(p) => match std::fs::read_to_string(&p) {
                    Ok(t) => {
                        state.swmm_doc.calib.import_text = t;
                        if state.swmm_doc.calib.import_name.trim().is_empty() {
                            state.swmm_doc.calib.import_name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        }
                        Ok(())
                    }
                    Err(e) => Err(format!("{}: {e}", p.display())),
                },
                None => Ok(()),
            }
        }
        Action::FromLastRun => add_observed_from_last_run(state).map(|_| {
            save_setup(state);
        }),
        Action::RemoveObserved(i) => {
            if i < state.swmm_doc.calib.setup.observed.len() {
                state.swmm_doc.calib.setup.observed.remove(i);
            }
            Ok(())
        }
        Action::AddSuggestion => {
            let s = suggestions(&state.swmm_doc.doc);
            match s.get(state.swmm_doc.calib.suggestion) {
                Some(p) => {
                    state.swmm_doc.calib.setup.parameters.push(p.clone());
                    Ok(())
                }
                None => Err("no suggestion chosen".into()),
            }
        }
        Action::AddDraft => add_draft_parameter(state),
        Action::RemoveParam(i) => {
            if i < state.swmm_doc.calib.setup.parameters.len() {
                state.swmm_doc.calib.setup.parameters.remove(i);
            }
            Ok(())
        }
        Action::Oat => start_oat(state),
        Action::Morris => start_morris(state),
        Action::Dds => start_dds(state),
        Action::SaveSetup => {
            save_setup(state);
            Ok(())
        }
        Action::ApplyBest => {
            if apply_best(state) {
                Ok(())
            } else {
                Err(state.swmm_doc.last_error.clone().unwrap_or_else(|| "nothing to apply".into()))
            }
        }
        Action::SaveScenario => save_as_scenario(state).map(|_| ()).ok_or_else(|| "no result yet".to_string()),
        Action::Report => {
            state.swmm_doc.calib.report_open = true;
            Ok(())
        }
        Action::ExportTornado => {
            let text = calib::tornado_csv(&state.swmm_doc.calib.tornado);
            if let Some(s) = crate::swmm_export::save_csv("sensitivity.csv", &text) {
                state.status = s;
            }
            Ok(())
        }
    };
    if let Err(e) = outcome {
        state.swmm_doc.calib.message = e;
    }
}

fn draw_progress(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.calib.progress_open {
        return;
    }
    let Some(run) = state.swmm_doc.calib.run.as_ref() else {
        state.swmm_doc.calib.progress_open = false;
        return;
    };
    let names: Vec<String> = state.swmm_doc.calib.setup.parameter_names();
    let (kind, done, total, best, best_x, failed, last_error, elapsed) = (
        run.kind,
        run.done,
        run.total,
        run.best_objective,
        run.best_x.clone(),
        run.failed,
        run.last_error.clone(),
        run.started.elapsed().as_secs(),
    );
    let pts: Vec<(f64, f64)> = run.history.iter().filter(|(_, f)| f.is_finite()).map(|(n, f)| (*n as f64, *f)).collect();
    let mut stop_now = false;
    egui::Window::new("Calibration Progress")
        .collapsible(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{}: {done} of {total} evaluations, {elapsed} s", kind.label()));
            });
            ui.add(egui::ProgressBar::new(if total == 0 { 0.0 } else { done as f32 / total as f32 }).show_percentage());
            if kind == RunKind::Dds {
                ui.label(format!("Best objective so far: {}", fmt(best)));
                for (n, x) in names.iter().zip(&best_x) {
                    ui.monospace(format!("{n} = {}", fmt(*x)));
                }
                ui.label(RichText::new("Convergence (best objective by evaluation)").small());
                crate::swmm_dialogs::plot(ui, egui::Id::new("swmm-calib-convergence"), &pts, 120.0, false);
            }
            if failed > 0 {
                ui.label(RichText::new(format!("{failed} evaluation(s) failed: {}", last_error.unwrap_or_default())).small());
            }
            if ui.button("Stop").on_hover_text("Finish the evaluations in flight, then stop").clicked() {
                stop_now = true;
            }
        });
    if stop_now {
        stop(state);
    }
}

fn draw_report(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.calib.report_open {
        return;
    }
    let mut open = true;
    let mut save: Option<&'static str> = None;
    egui::Window::new("Calibration Report")
        .open(&mut open)
        .default_width(640.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save Markdown…").clicked() {
                    save = Some("md");
                }
                if ui.button("Save HTML…").clicked() {
                    save = Some("html");
                }
                if ui.button("Export series CSV…").clicked() {
                    save = Some("csv");
                }
            });
            egui::ScrollArea::vertical().id_salt("swmm-calib-report").max_height(480.0).show(ui, |ui| {
                let mut text = state.swmm_doc.calib.report_text.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });
        });
    state.swmm_doc.calib.report_open = open;
    if let Some(kind) = save {
        let Some(r) = state.swmm_doc.calib.result.clone() else { return };
        let model = state.swmm_doc.file_name();
        let (name, text, filter) = match kind {
            "md" => ("calibration.md".to_string(), calib::report_markdown(&r, &model), "Markdown"),
            "html" => ("calibration.html".to_string(), calib::report_html(&r, &model), "HTML"),
            _ => ("calibration-series.csv".to_string(), calib::fits_csv(&r.fits), "CSV"),
        };
        if let Some(path) = rfd::FileDialog::new().add_filter(filter, &[kind]).set_file_name(&name).save_file() {
            state.status = match std::fs::write(&path, text) {
                Ok(()) => format!("Saved {}", path.display()),
                Err(e) => format!("Could not save: {e}"),
            };
        }
    }
}

/// Windows and dialogs; also polls the worker once per frame.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    poll(state);
    if state.swmm_doc.calib.is_running() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    draw_main(ctx, state);
    draw_progress(ctx, state);
    draw_report(ctx, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::swmm_report::tests::fixture_run;
    use crate::StormSewerApp;
    use stormsewer_swmm::out::OutputFile;

    fn scratch_model(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-app-calib-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("Pond-{tag}-{}.inp", std::process::id()));
        std::fs::write(&p, fixture_text("Detention_Pond_Model.inp")).unwrap();
        let _ = std::fs::remove_file(CalibSetup::sidecar_path(&p));
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
    fn calibration_window_builds_a_setup_and_persists_it() {
        let model = scratch_model("window");
        let mut app = pond_app(Some(model.clone()));
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| tools_menu_items(ui, &mut state));
        });
        app.state = state;
        open(&mut app.state);
        run_frame(&mut app);
        // Observed data from pasted text.
        assert!(add_observed_from_text(&mut app.state).is_err(), "needs an object");
        app.state.swmm_doc.calib.import_id = "O2".into();
        app.state.swmm_doc.calib.import_text = "datetime,flow\n2007-01-01 00:05,1\n2007-01-01 00:10,2\n".into();
        add_observed_from_text(&mut app.state).unwrap();
        assert_eq!(app.state.swmm_doc.calib.setup.observed.len(), 1);
        assert_eq!(app.state.swmm_doc.calib.setup.observed[0].name, "O2 observed");
        assert!(app.state.swmm_doc.calib.import_text.is_empty());
        // Observed data from the last run's results.
        assert!(add_observed_from_last_run(&mut app.state).is_err(), "no run yet");
        let run = fixture_run();
        app.state.swmm.results = Some(OutputFile::open(&run.out).unwrap());
        app.state.swmm_doc.calib.import_name = "synthetic".into();
        add_observed_from_last_run(&mut app.state).unwrap();
        assert_eq!(app.state.swmm_doc.calib.setup.observed[1].len(), 144, "12 h at 5 min");
        assert!(matches!(app.state.swmm_doc.calib.setup.observed[1].times[0], ObsTime::Absolute(_)));
        // Parameters: a suggestion, then a composed one over the selection.
        app.state.swmm_doc.calib.suggestion = 1;
        perform(&mut app.state, Action::AddSuggestion);
        assert_eq!(app.state.swmm_doc.calib.setup.parameters[0].name, "Width");
        app.state.swmm_doc.calib.draft = ParamDraft {
            name: String::new(),
            section: "CONDUITS".into(),
            column: "Roughness".into(),
            group: GroupKind::Selection,
            group_text: String::new(),
            transform: Transform::Multiply,
            lower: 0.5,
            upper: 2.0,
        };
        assert!(add_draft_parameter(&mut app.state).is_err(), "nothing selected");
        app.state.swmm_doc.select_many(vec![
            stormsewer_swmm::doc::build::ObjRef::Link("C1".into()),
            stormsewer_swmm::doc::build::ObjRef::Link("C3".into()),
            stormsewer_swmm::doc::build::ObjRef::Node("J1".into()),
        ]);
        add_draft_parameter(&mut app.state).unwrap();
        let p = &app.state.swmm_doc.calib.setup.parameters[1];
        assert_eq!(p.group, Group::Selection(vec!["C1".into(), "C3".into()]));
        assert_eq!(p.name, "CONDUITS Roughness (2 selected)");
        app.state.swmm_doc.calib.draft.group = GroupKind::Tag;
        app.state.swmm_doc.calib.draft.group_text = "Swale".into();
        add_draft_parameter(&mut app.state).unwrap();
        assert_eq!(app.state.swmm_doc.calib.setup.parameters[2].members(&app.state.swmm_doc.doc).len(), 7);
        app.state.swmm_doc.calib.draft.group = GroupKind::Object;
        app.state.swmm_doc.calib.draft.group_text = "Cnone".into();
        assert!(add_draft_parameter(&mut app.state).is_err(), "no such object");
        app.state.swmm_doc.calib.draft.lower = 3.0;
        assert!(add_draft_parameter(&mut app.state).is_err(), "bounds");
        // Persist and reload.
        assert!(save_setup(&mut app.state));
        assert!(CalibSetup::sidecar_path(&model).is_file());
        let back = CalibSetup::load_for(&model).unwrap();
        let setup = &app.state.swmm_doc.calib.setup;
        assert_eq!(back.parameters, setup.parameters);
        assert_eq!(back.objective, setup.objective);
        assert_eq!(back.observed.len(), setup.observed.len());
        for (a, b) in back.observed.iter().zip(&setup.observed) {
            assert_eq!((&a.name, &a.id, a.variable, a.len()), (&b.name, &b.id, b.variable, b.len()));
            // serde_json's default float parser is not bit-exact.
            assert!(a.values.iter().zip(&b.values).all(|(x, y)| (x - y).abs() < 1e-9));
        }
        // A run cannot start without an engine chosen; the message says so.
        app.state.swmm.engine_id = None;
        let err = start_dds(&mut app.state).unwrap_err();
        assert!(err.contains("engine"), "{err}");
        // The windows draw in every state.
        app.state.swmm_doc.calib.progress_open = true;
        app.state.swmm_doc.calib.report_open = true;
        run_frame(&mut app);
        assert!(!app.state.swmm_doc.calib.progress_open, "closes when nothing runs");
        perform(&mut app.state, Action::RemoveObserved(0));
        perform(&mut app.state, Action::RemoveParam(0));
        assert_eq!(app.state.swmm_doc.calib.setup.observed.len(), 1);
        assert_eq!(app.state.swmm_doc.calib.setup.parameters.len(), 2);
        run_frame(&mut app);
        // Apply Best on a fake result is one undo step; Save as Scenario adds one.
        let r = CalibResult {
            objective: Objective::Nse,
            parameters: vec![app.state.swmm_doc.calib.setup.parameters[0].clone()],
            x0: vec![1.0],
            best_x: vec![1.5],
            best_objective: 0.1,
            evaluations: 3,
            max_evals: 3,
            seed: 1,
            history: vec![(1, 0.5), (2, 0.2), (3, 0.1)],
            fits: Vec::new(),
            initial_fits: Vec::new(),
            engine_version: "5.2.4".into(),
            engine_sha256: "x".into(),
            stopped_early: false,
            failed_evaluations: 0,
        };
        app.state.swmm_doc.calib.result = Some(r);
        let depth = app.state.swmm_doc.undo_depth();
        assert!(apply_best(&mut app.state));
        assert_eq!(app.state.swmm_doc.undo_depth(), depth + 1);
        assert_eq!(app.state.swmm_doc.doc.field("CONDUITS", "C1", "Roughness"), Some("0.075"));
        assert_eq!(app.state.swmm_doc.doc.field("CONDUITS", "C2", "Roughness"), Some("0.016"));
        assert_eq!(app.state.swmm_doc.undo_label(), Some("apply calibration"));
        app.state.swmm_doc.undo();
        assert_eq!(app.state.swmm_doc.doc.field("CONDUITS", "C1", "Roughness"), Some("0.05"));
        let i = save_as_scenario(&mut app.state).unwrap();
        assert_eq!(app.state.swmm_doc.scenarios.set.scenarios[i].name, "Calibrated");
        assert_eq!(app.state.swmm_doc.scenarios.set.scenarios[i].edits.len(), 1);
        run_frame(&mut app);
        let _ = std::fs::remove_file(CalibSetup::sidecar_path(&model));
        let _ = std::fs::remove_file(stormsewer_swmm::scenario::ScenarioSet::sidecar_path(&model));
        let _ = std::fs::remove_file(&model);
    }

    /// The whole loop against a real engine: synthetic observations from
    /// a perturbed run, a short DDS, the result applied.
    #[test]
    fn dds_runs_from_the_window_when_an_engine_is_present() {
        let model = scratch_model("dds");
        let mut app = pond_app(Some(model.clone()));
        app.state.swmm.ensure_discovered();
        let Some(engine) = app.state.swmm.engine().cloned() else {
            eprintln!("skipped: no SWMM engine discovered");
            return;
        };
        open(&mut app.state);
        // Observed: the base run's O2 inflow with widths × 1.5 as the truth.
        let mut truth = stormsewer_swmm::doc::InpDoc::parse(&app.state.swmm_doc.doc.to_string());
        let width = Parameter::new("Width", "SUBCATCHMENTS", "Width", Group::All, Transform::Multiply, 0.5, 2.0);
        truth.apply(apply_set(&truth, std::slice::from_ref(&width), &[1.5])).unwrap();
        let truth_path = model.with_extension("truth.inp");
        std::fs::write(&truth_path, truth.to_string()).unwrap();
        let run = engine.run(&truth_path).unwrap();
        assert!(run.succeeded());
        app.state.swmm.results = Some(OutputFile::open(&run.out).unwrap());
        app.state.swmm_doc.calib.import_id = "O2".into();
        app.state.swmm_doc.calib.import_variable = Variable::SystemInflow;
        add_observed_from_last_run(&mut app.state).unwrap();
        app.state.swmm_doc.calib.setup.parameters = vec![width];
        app.state.swmm_doc.calib.setup.max_evals = 6;
        app.state.swmm_doc.calib.setup.threads = 2;
        app.state.swmm_doc.calib.setup.seed = 5;
        start_oat(&mut app.state).unwrap();
        assert!(start_dds(&mut app.state).is_err(), "one run at a time");
        let started = std::time::Instant::now();
        while app.state.swmm_doc.calib.is_running() {
            assert!(started.elapsed().as_secs() < 180);
            std::thread::sleep(std::time::Duration::from_millis(100));
            run_frame(&mut app);
        }
        assert_eq!(app.state.swmm_doc.calib.tornado.len(), 1);
        start_dds(&mut app.state).unwrap();
        assert!(app.state.swmm_doc.calib.progress_open);
        let started = std::time::Instant::now();
        while app.state.swmm_doc.calib.is_running() {
            assert!(started.elapsed().as_secs() < 300);
            std::thread::sleep(std::time::Duration::from_millis(100));
            run_frame(&mut app);
        }
        let r = app.state.swmm_doc.calib.result.clone().expect("a result");
        assert_eq!(r.evaluations, 6);
        assert!(r.best_objective <= r.history[0].1, "{r:?}");
        assert!(r.fits.len() == 1 && r.fits[0].metrics.n > 100);
        assert!(app.state.swmm_doc.calib.report_text.contains(&engine.sha256));
        eprintln!("window DDS: best 1−NSE {:.4} at width × {:.3}", r.best_objective, r.best_x[0]);
        assert!(apply_best(&mut app.state));
        run_frame(&mut app);
        let _ = std::fs::remove_file(&truth_path);
        let _ = std::fs::remove_file(CalibSetup::sidecar_path(&model));
        let _ = std::fs::remove_file(&model);
    }
}
