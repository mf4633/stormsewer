// SPDX-License-Identifier: GPL-3.0-or-later

//! Run → Check Model…: the pre-run QA pass. Runs the document's checks
//! (`stormsewer_swmm::doc::validate`) and lists the findings grouped by
//! severity, each clickable to select the object on the map. The same
//! window doubles as the "warnings before a run" prompt: Run refuses on
//! errors as before, and when there are only warnings it shows them once
//! (per document state) with a Run Anyway button.

use eframe::egui::{self, RichText, Ui};
use stormsewer_swmm::doc::{Finding, Severity};

use crate::state::AppState;
use crate::swmm_run_panel::go_to;
use crate::theme::palette;

#[derive(Clone, Debug, Default)]
pub struct QaState {
    pub open: bool,
    /// The window is asking whether to run despite warnings.
    pub pre_run: bool,
    /// Document generation whose warnings the user has already accepted,
    /// so a repeated F5 does not nag.
    pub warnings_acknowledged: Option<u64>,
    /// Set by Run Anyway; the menu code starts the run and clears it.
    pub run_anyway: bool,
}

/// Findings grouped: errors first, then warnings, each in document order.
pub fn grouped(findings: &[Finding]) -> (Vec<Finding>, Vec<Finding>) {
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .cloned()
        .collect();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .cloned()
        .collect();
    (errors, warnings)
}

/// Run → Check Model: refresh the checks and open the window.
pub fn check_model(state: &mut AppState) {
    state.swmm_doc.refresh();
    state.swmm_doc.qa.pre_run = false;
    state.swmm_doc.qa.open = true;
    let e = state.swmm_doc.error_count();
    let w = state.swmm_doc.warning_count();
    state.status = if e + w == 0 {
        "Check Model: no findings".into()
    } else {
        format!("Check Model: {e} error(s), {w} warning(s)")
    };
}

/// Whether a run may start now. Errors: no. Warnings not yet seen for this
/// document state: opens the prompt and returns false. Otherwise true.
pub fn clear_to_run(state: &mut AppState) -> bool {
    state.swmm_doc.refresh();
    if state.swmm_doc.error_count() > 0 {
        return false;
    }
    if state.swmm_doc.warning_count() == 0 {
        return true;
    }
    let gen = state.swmm_doc.doc.generation();
    if state.swmm_doc.qa.warnings_acknowledged == Some(gen) {
        return true;
    }
    state.swmm_doc.qa.pre_run = true;
    state.swmm_doc.qa.open = true;
    state.status = format!(
        "{} warning(s) to look at before running",
        state.swmm_doc.warning_count()
    );
    false
}

/// Accept the warnings for the current document state.
pub fn acknowledge(state: &mut AppState) {
    state.swmm_doc.qa.warnings_acknowledged = Some(state.swmm_doc.doc.generation());
    state.swmm_doc.qa.pre_run = false;
    state.swmm_doc.qa.open = false;
    state.swmm_doc.qa.run_anyway = true;
}

fn finding_row(ui: &mut Ui, state: &AppState, f: &Finding, dark: bool) -> bool {
    let color = match f.severity {
        Severity::Error => palette::error_text(dark),
        Severity::Warning => palette::warning_text(dark),
    };
    let target = state.swmm_doc.finding_target(f);
    let mut line = format!("[{}] {}", f.section, f.name);
    if let Some(c) = &f.column {
        line.push_str(&format!(" · {c}"));
    }
    line.push_str(&format!(": {}", f.message));
    let selected = target.as_ref().is_some_and(|t| state.swmm_doc.is_selected(t));
    ui.selectable_label(selected, RichText::new(line).color(color).small())
        .on_hover_text(if target.is_some() {
            "Select on the map"
        } else {
            "Not a map object"
        })
        .clicked()
        && target.is_some()
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    if state.swmm_doc.qa.run_anyway {
        // Run Anyway: the warnings are acknowledged for this document
        // state, so the run goes straight through.
        crate::swmm_menus::run_model(state);
    }
    if !state.swmm_doc.qa.open || !state.swmm_doc.loaded {
        return;
    }
    let (errors, warnings) = grouped(&state.swmm_doc.findings);
    let pre_run = state.swmm_doc.qa.pre_run;
    let title = if pre_run { "Warnings before running" } else { "Check Model" };
    let mut open = true;
    let mut go: Option<Finding> = None;
    let mut run_anyway = false;
    let mut cancel = false;
    let dark = ctx.style().visuals.dark_mode;
    egui::Window::new(title)
        .id(egui::Id::new("swmm-qa-window"))
        .open(&mut open)
        .default_size(egui::vec2(600.0, 420.0))
        .resizable(true)
        .show(ctx, |ui| {
            if errors.is_empty() && warnings.is_empty() {
                ui.label(RichText::new("No findings.").color(palette::ok_text(dark)));
            }
            if pre_run {
                ui.label("The model has no errors, but these warnings are worth a look. Run anyway?");
                ui.horizontal(|ui| {
                    if ui.button("Run anyway").clicked() {
                        run_anyway = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
                ui.separator();
            }
            egui::ScrollArea::vertical()
                .id_salt("swmm-qa-scroll")
                .show(ui, |ui| {
                    if !errors.is_empty() {
                        ui.label(
                            RichText::new(format!("Errors ({}) — the engine will refuse or misread the model", errors.len()))
                                .strong()
                                .color(palette::error_text(dark)),
                        );
                        for f in &errors {
                            if finding_row(ui, state, f, dark) {
                                go = Some(f.clone());
                            }
                        }
                        ui.add_space(6.0);
                    }
                    if !warnings.is_empty() {
                        ui.label(
                            RichText::new(format!("Warnings ({}) — the model runs, but check these", warnings.len()))
                                .strong()
                                .color(palette::warning_text(dark)),
                        );
                        for f in &warnings {
                            if finding_row(ui, state, f, dark) {
                                go = Some(f.clone());
                            }
                        }
                    }
                });
        });
    if let Some(f) = go {
        if let Some(t) = state.swmm_doc.finding_target(&f) {
            go_to(state, t);
        }
    }
    if run_anyway {
        acknowledge(state);
    } else if cancel || !open {
        state.swmm_doc.qa.open = false;
        state.swmm_doc.qa.pre_run = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;
    use stormsewer_swmm::doc::build::ObjRef;

    /// A model with one warning (an orphan junction with coordinates) and
    /// no errors.
    const WARN_ONLY: &str = "[JUNCTIONS]\nJ1 100 5\nJ2 100 5\n[OUTFALLS]\nO1 95 FREE NO\n[CONDUITS]\nC1 J1 O1 100 0.013 0 0\n[XSECTIONS]\nC1 CIRCULAR 1 0 0 0\n[COORDINATES]\nJ1 0 0\nJ2 50 50\nO1 100 0\n";

    #[test]
    fn check_model_groups_findings_and_run_lists_warnings_once() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state.swmm_doc.open_text(WARN_ONLY, None);
        check_model(&mut app.state);
        assert!(app.state.swmm_doc.qa.open);
        assert!(!app.state.swmm_doc.qa.pre_run);
        let (errors, warnings) = grouped(&app.state.swmm_doc.findings);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(warnings.iter().any(|w| w.name == "J2" && w.message.contains("no link")), "{warnings:?}");
        assert!(app.state.status.starts_with("Check Model: 0 error(s), 1 warning(s)"), "{}", app.state.status);
        run_frame(&mut app);
        assert!(app.state.swmm_doc.qa.open, "window survives a frame");
        app.state.swmm_doc.qa.open = false;

        // Run: warnings are shown once, then the run proceeds.
        swmm_menus::run_model(&mut app.state);
        assert!(app.state.swmm_doc.qa.open && app.state.swmm_doc.qa.pre_run, "prompted");
        assert!(app.state.swmm.model.is_none(), "not started");
        run_frame(&mut app);
        acknowledge(&mut app.state);
        assert!(app.state.swmm_doc.qa.run_anyway);
        run_frame(&mut app);
        assert!(!app.state.swmm_doc.qa.run_anyway, "the menu consumed it");
        assert!(app.state.swmm.model.is_some(), "the run went ahead (to the engine check)");
        assert!(!app.state.swmm_doc.qa.open);
        // Same document state: no second prompt.
        app.state.swmm.model = None;
        swmm_menus::run_model(&mut app.state);
        assert!(!app.state.swmm_doc.qa.pre_run);
        assert!(app.state.swmm.model.is_some());
        // An edit brings the prompt back.
        app.state.swmm_doc.apply(
            stormsewer_swmm::doc::Command::MoveNode { name: "J2".into(), x: 60.0, y: 60.0 },
            "move",
        );
        app.state.swmm.model = None;
        swmm_menus::run_model(&mut app.state);
        assert!(app.state.swmm_doc.qa.pre_run);
        assert!(app.state.swmm.model.is_none());

        // Errors still refuse outright, without the warning prompt.
        app.state.swmm_doc.qa.open = false;
        app.state.swmm_doc.qa.pre_run = false;
        app.state.swmm_doc.open_text("[JUNCTIONS]\nJ1 0\n[CONDUITS]\nC1 J1 NOPE 10 0.01 0 0\n", None);
        swmm_menus::run_model(&mut app.state);
        assert!(app.state.swmm_doc.run_refused.is_some());
        assert!(!app.state.swmm_doc.qa.pre_run);
        assert!(app.state.status.starts_with("Run refused"), "{}", app.state.status);

        // A finding's target selects and zooms.
        app.state.swmm_doc.open_text(WARN_ONLY, None);
        check_model(&mut app.state);
        let f = app.state.swmm_doc.findings.iter().find(|f| f.name == "J2").cloned().unwrap();
        let t = app.state.swmm_doc.finding_target(&f).unwrap();
        go_to(&mut app.state, t);
        assert_eq!(app.state.swmm_doc.selection, vec![ObjRef::Node("J2".into())]);
        assert!(app.state.swmm_doc.pending_zoom_to.is_some());
    }
}
