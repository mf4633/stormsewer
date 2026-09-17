// SPDX-License-Identifier: GPL-3.0-or-later

//! The Python terminal window.
//!
//! A persistent kernel (see `stormsewer_swmm::python`) runs as a child
//! process with the current model, results, and report already bound as
//! `model`, `out`, and `rpt`. This is where a run gets post-processed: the
//! interpreter an engineer already has usually carries numpy, pandas, swmmio
//! and pyswmm, and ALR is itself Python, so it can be driven from here
//! directly.

use std::path::PathBuf;

use eframe::egui::{self, RichText};

use stormsewer_swmm::python::{default_python, Output, PythonSession, SessionEnv};

use crate::state::AppState;

/// Terminal window state. The session is dropped — and so shut down — when
/// this is replaced or the app closes.
#[derive(Default)]
pub struct PythonTermState {
    pub open: bool,
    session: Option<PythonSession>,
    pub input: String,
    /// The transcript exactly as shown.
    pub log: String,
    pub status: String,
}

impl PythonTermState {
    pub fn is_busy(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.is_busy())
    }

    pub fn is_started(&self) -> bool {
        self.session.is_some()
    }

    /// Start the kernel unless one is already running.
    pub fn start(&mut self, env: SessionEnv) {
        if self.session.is_some() {
            return;
        }
        let Some(python) = default_python() else {
            self.status =
                "No Python interpreter found on PATH — install Python to use this.".to_string();
            return;
        };
        match PythonSession::start(&python, &env) {
            Ok(session) => {
                self.session = Some(session);
                self.status = format!("kernel: {}", python.display());
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    pub fn restart(&mut self, env: SessionEnv) {
        // Dropping the old session closes its stdin and reaps the child.
        self.session = None;
        self.log.push_str("\n--- kernel restarted ---\n");
        self.start(env);
    }

    /// Send a block, echoing it into the transcript the way a prompt would.
    pub fn submit(&mut self, source: &str) {
        // trim(), not trim_end(): whitespace-only input is nothing to run, and
        // should not echo a prompt or raise a "kernel not running" complaint.
        if source.trim().is_empty() {
            return;
        }
        let trimmed = source.trim_end();
        if self.session.is_none() {
            self.status = "The kernel is not running.".to_string();
            return;
        }
        for line in trimmed.lines() {
            self.log.push_str(">>> ");
            self.log.push_str(line);
            self.log.push('\n');
        }
        if let Some(session) = self.session.as_mut() {
            if let Err(e) = session.send(trimmed) {
                self.log.push_str(&format!("{e}\n"));
            }
        }
    }

    /// Drain whatever the kernel has said. True when anything arrived, so the
    /// caller knows to keep the frame clock running.
    pub fn poll(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let items = session.poll();
        if items.is_empty() {
            return false;
        }
        for item in items {
            match item {
                Output::Line(line) => {
                    self.log.push_str(&line);
                    self.log.push('\n');
                }
                Output::Done { ok } => {
                    self.status = if ok {
                        "ready".to_string()
                    } else {
                        "the last block raised".to_string()
                    };
                }
            }
        }
        true
    }
}

/// Bind the current model and results into a new session.
fn session_env(app: &AppState) -> SessionEnv {
    let run = app.swmm.last_run.as_ref();
    SessionEnv {
        model: app.swmm.model.clone(),
        out: run.map(|r| r.out.clone()),
        rpt: run.map(|r| r.rpt.clone()),
        alr_dir: alr_dir(),
    }
}

/// Where ALR lives, so `import quantum_hydraulics` works in the session.
/// Accepts the script path the ALR bridge already uses, and falls back to it
/// rather than making the user configure the same thing twice.
fn alr_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("STORMSEWER_ALR_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("STORMSEWER_ALR_SCRIPT")
        .map(PathBuf::from)
        .and_then(|script| script.parent().map(|dir| dir.to_path_buf()))
}

pub fn draw_python_terminal_window(ctx: &egui::Context, app: &mut AppState) {
    if !app.python_term.open {
        return;
    }
    if !app.python_term.is_started() {
        let env = session_env(app);
        app.python_term.start(env);
    }

    let busy = app.python_term.is_busy();
    let (mut run, mut restart, mut clear, mut close) = (false, false, false, false);

    let mut open = app.python_term.open;
    egui::Window::new("Python Terminal")
        .collapsible(false)
        .resizable(true)
        .default_width(680.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(RichText::new(&app.python_term.status).monospace().size(11.0));

            egui::ScrollArea::vertical()
                .max_height(320.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    // A read-only TextEdit rather than a label, so the
                    // transcript can be selected and copied out.
                    let mut transcript = app.python_term.log.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut transcript)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });

            ui.separator();
            ui.add(
                egui::TextEdit::multiline(&mut app.python_term.input)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("Python — Ctrl+Enter to run"),
            );

            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Run")).clicked() {
                    run = true;
                }
                if ui.button("Restart Kernel").clicked() {
                    restart = true;
                }
                if ui.button("Clear Output").clicked() {
                    clear = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
                if busy {
                    ui.spinner();
                }
            });

            let ctrl_enter =
                ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
            if ctrl_enter && !busy {
                run = true;
            }
        });
    app.python_term.open = open;

    if run {
        let source = std::mem::take(&mut app.python_term.input);
        app.python_term.submit(&source);
    }
    if restart {
        let env = session_env(app);
        app.python_term.restart(env);
    }
    if clear {
        app.python_term.log.clear();
    }
    if close {
        app.python_term.open = false;
    }
}
