// SPDX-License-Identifier: GPL-3.0-or-later

//! The SWMM tab: choose an engine, run a model, and read what came back.
//!
//! Runs happen on a worker thread. A real model takes minutes and the engine
//! is a child process we do nothing but wait on, so running it inline would
//! freeze the window for the whole simulation. This is the only background
//! work in the app, so it is kept deliberately small: one channel, polled
//! once per frame from `ui()`, and no shared mutable state.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, RichText, Ui};

use stormsewer_swmm::alr::{Alr, AlrOptions, AlrReport};
use stormsewer_swmm::engine::{Engine, Registry, Run};
use stormsewer_swmm::out::{format_datetime, OutputFile};

use crate::state::AppState;

/// What a finished worker thread hands back. Boxed because a `Run` carries
/// the whole parsed report and dwarfs the error string.
enum RunOutcome {
    Finished(Box<Run>),
    Failed(String),
}

/// Engines found, the model chosen, and the last run's results.
#[derive(Default)]
pub struct SwmmState {
    pub registry: Registry,
    discovered: bool,
    pub engine_id: Option<String>,
    pub model: Option<PathBuf>,
    /// Present only while a run is in flight.
    pending: Option<Receiver<RunOutcome>>,
    pub last_run: Option<Run>,
    pub results: Option<OutputFile>,
    pub alr: Option<AlrReport>,
    /// Whatever the user most needs told: an error, or a note about progress.
    pub log: String,
}

impl SwmmState {
    /// Scan for engines the first time the tab is shown. Discovery touches the
    /// filesystem, so it has no business running in `Default`.
    pub fn ensure_discovered(&mut self) {
        if !self.discovered {
            self.rescan();
        }
    }

    pub fn rescan(&mut self) {
        self.registry = Registry::discover();
        self.discovered = true;
        if self.engine_id.is_none() {
            self.engine_id = self.registry.default_engine().map(|e| e.id.clone());
        }
        self.log = match self.registry.engines().len() {
            0 => "No SWMM engine found. Install EPA SWMM, or set \
                  STORMSEWER_SWMM_ENGINE_DIR to the folder holding runswmm."
                .to_string(),
            1 => "Found 1 SWMM engine.".to_string(),
            n => format!("Found {n} SWMM engines."),
        };
    }

    pub fn engine(&self) -> Option<&Engine> {
        self.engine_id
            .as_deref()
            .and_then(|id| self.registry.by_id(id))
    }

    pub fn is_running(&self) -> bool {
        self.pending.is_some()
    }

    pub fn can_run(&self) -> bool {
        !self.is_running() && self.engine().is_some() && self.model.is_some()
    }

    /// One line for the status bar.
    pub fn status_line(&self) -> String {
        if self.is_running() {
            return "SWMM: running…".to_string();
        }
        match &self.last_run {
            None => "SWMM: idle".to_string(),
            Some(run) => match run.failure_reason() {
                None => format!(
                    "SWMM: finished in {:.1} s, {} warning(s)",
                    run.elapsed.as_secs_f64(),
                    run.report.warnings.len()
                ),
                Some(why) => format!("SWMM: {why}"),
            },
        }
    }

    pub fn start_run(&mut self) {
        let (Some(engine), Some(model)) = (self.engine().cloned(), self.model.clone()) else {
            self.log = "Choose an engine and a model first.".to_string();
            return;
        };
        self.results = None;
        self.alr = None;
        self.last_run = None;
        self.log = format!("Running with {}…", engine.label());

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = match engine.run(&model) {
                Ok(run) => RunOutcome::Finished(Box::new(run)),
                Err(e) => RunOutcome::Failed(e.to_string()),
            };
            // The receiver is gone if the window closed mid-run. Nothing to do.
            let _ = tx.send(outcome);
        });
        self.pending = Some(rx);
    }

    /// Collect a finished run. Returns true when something changed, so the
    /// caller knows to refresh the status line.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = self.pending.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                // A worker that panicked would otherwise leave the tab saying
                // "running" for the rest of the session.
                self.pending = None;
                self.log = "The run stopped without reporting a result.".to_string();
                true
            }
            Ok(RunOutcome::Failed(message)) => {
                self.pending = None;
                self.log = message;
                true
            }
            Ok(RunOutcome::Finished(run)) => {
                self.pending = None;
                self.log = String::new();
                if run.succeeded() {
                    match OutputFile::open(&run.out) {
                        Ok(f) => self.results = Some(f),
                        Err(e) => self.log = format!("Results could not be read: {e}"),
                    }
                }
                self.last_run = Some(*run);
                true
            }
        }
    }

    /// Post-process the last run with ALR. Synchronous: this reads a finished
    /// results file rather than simulating, and returns in well under a second
    /// on the models it is meant for.
    pub fn run_alr(&mut self) {
        let Some(out) = self.last_run.as_ref().map(|r| r.out.clone()) else {
            return;
        };
        let Some(alr) = Alr::from_env() else {
            self.log = "Set STORMSEWER_ALR_SCRIPT to the path of \
                        run_headless_swmm.py to enable ALR checks."
                .to_string();
            return;
        };
        match alr.analyze(&out, &AlrOptions::default()) {
            Ok(report) => {
                self.log = report.summary();
                self.alr = Some(report);
            }
            Err(e) => self.log = e.to_string(),
        }
    }
}

/// Short hash prefix for display, without assuming a length.
fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

pub fn draw_swmm_tab(ui: &mut Ui, state: &mut AppState) {
    state.swmm.ensure_discovered();

    ui.label(RichText::new("Engine").strong());
    if state.swmm.registry.is_empty() {
        ui.colored_label(egui::Color32::LIGHT_RED, "No SWMM engine found.");
    } else {
        // Collected first: the selection closure needs `state` mutably.
        let engines: Vec<(String, String, bool)> = state
            .swmm
            .registry
            .engines()
            .iter()
            .map(|e| (e.id.clone(), e.label(), e.arch.requires_subprocess_from_x64()))
            .collect();
        for (id, label, out_of_process) in engines {
            let selected = state.swmm.engine_id.as_deref() == Some(id.as_str());
            let text = if out_of_process {
                format!("{label}  ·  runs out-of-process")
            } else {
                label
            };
            if ui.selectable_label(selected, text).clicked() {
                state.swmm.engine_id = Some(id);
            }
        }
    }
    if ui.button("Find Engines").clicked() {
        state.swmm.rescan();
    }

    ui.add_space(6.0);
    ui.separator();
    ui.label(RichText::new("Model").strong());
    let model_text = state
        .swmm
        .model
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "none chosen".to_string());
    ui.label(model_text);
    if ui.button("Choose Model (.inp)…").clicked() {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SWMM Input", &["inp", "INP"])
            .pick_file()
        {
            state.swmm.model = Some(path);
            state.swmm.last_run = None;
            state.swmm.results = None;
            state.swmm.alr = None;
            state.swmm.log = String::new();
        }
    }

    ui.add_space(6.0);
    let can_run = state.swmm.can_run();
    if ui
        .add_enabled(can_run, egui::Button::new("Run Model"))
        .clicked()
    {
        state.swmm.start_run();
        state.status = state.swmm.status_line();
    }
    if state.swmm.is_running() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("running…");
        });
    }

    let mut start_alr = false;
    if let Some(run) = &state.swmm.last_run {
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Last run").strong());
        ui.label(format!(
            "engine {} ({})",
            run.engine_version,
            short_hash(&run.engine_sha256)
        ));
        ui.label(format!("{:.1} s", run.elapsed.as_secs_f64()));

        for error in &run.report.errors {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        for warning in run.report.warnings.iter().take(6) {
            ui.colored_label(egui::Color32::GRAY, warning);
        }
        if let Some((section, pct)) = run.report.worst_continuity() {
            ui.label(format!("worst continuity {pct:+.3}% ({section})"));
        }
        if let Some(why) = run.failure_reason() {
            ui.colored_label(egui::Color32::LIGHT_RED, why);
        }

        ui.add_space(4.0);
        if ui
            .add_enabled(run.succeeded(), egui::Button::new("Run ALR Checks"))
            .clicked()
        {
            start_alr = true;
        }
    }
    if start_alr {
        state.swmm.run_alr();
        state.status = state.swmm.status_line();
    }

    if let Some(f) = &state.swmm.results {
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Results").strong());
        ui.label(format!(
            "{} periods every {} s",
            f.meta.n_periods, f.meta.report_step_s
        ));
        ui.label(format!(
            "{} nodes, {} links, {}",
            f.meta.n_nodes,
            f.meta.n_links,
            f.meta.flow_units.label()
        ));
        ui.label(format!("start {}", format_datetime(f.meta.start_days)));
    }

    if let Some(report) = &state.swmm.alr {
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("ALR").strong());
        ui.label(report.summary());
        for check in report.failures() {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!(
                    "{} — {}",
                    check.name,
                    check.detail.clone().unwrap_or_default()
                ),
            );
        }
    }

    if !state.swmm.log.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(state.swmm.log.clone()).monospace());
    }
}
