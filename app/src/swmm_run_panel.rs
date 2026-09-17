// SPDX-License-Identifier: GPL-3.0-or-later

//! Results → Run Status…: the whole story of the last run in one window,
//! opened by itself when a run finishes. Continuity errors with the
//! colour thresholds on screen, every diagnostic list the report carries
//! (all entries, not the GUI's five), every WARNING and ERROR line with
//! what the code means and the object it names, engine version and binary
//! hash, elapsed time, and where the engine actually read the model from
//! (a scratch copy, when the path forced one). Every entry that names a
//! map object selects and zooms to it. A plain-text summary can be copied.
//!
//! Help → SWMM Error Codes… lists the whole index with a search box; the
//! window lives here too.

use std::path::PathBuf;

use eframe::egui::{self, Button, Color32, RichText, Ui};
use stormsewer_swmm::doc::build::ObjRef;
use stormsewer_swmm::doc::ObjectKind;
use stormsewer_swmm::engine::Run;
use stormsewer_swmm::errors::{self, Kind, ParsedLine};
use stormsewer_swmm::rpt::RankedEntry;

use crate::state::AppState;
use crate::swmm_panel::SwmmSubView;
use crate::theme::palette;

/// Continuity thresholds, in percent of absolute error.
pub const CONTINUITY_AMBER: f64 = 1.0;
pub const CONTINUITY_RED: f64 = 10.0;

#[derive(Clone, Debug, Default)]
pub struct RunPanelState {
    pub open: bool,
    /// Identity of the run the panel last opened for.
    seen: Option<(PathBuf, String, u128)>,
    pub codes_open: bool,
    pub codes_query: String,
    /// The last text put on the clipboard, for tests.
    pub last_copied: Option<String>,
    /// Stale scratch folders were cleaned this session.
    cleaned: bool,
}

/// Green / amber / red for a continuity error.
pub fn continuity_color(pct: f64, dark: bool) -> Color32 {
    let a = pct.abs();
    if a >= CONTINUITY_RED {
        palette::error_text(dark)
    } else if a >= CONTINUITY_AMBER {
        palette::warning_text(dark)
    } else {
        palette::ok_text(dark)
    }
}

/// The map object a report line or diagnostic entry names, if the open
/// model has it.
pub fn resolve_object(state: &AppState, what: &str, name: &str) -> Option<ObjRef> {
    let doc = &state.swmm_doc.doc;
    let kinds: &[ObjectKind] = match what.to_ascii_lowercase().as_str() {
        "node" | "junction" | "outfall" | "storage" | "divider" => &[ObjectKind::Node],
        "link" | "conduit" | "pump" | "orifice" | "weir" | "outlet" | "regulator" => {
            &[ObjectKind::Link]
        }
        "subcatchment" => &[ObjectKind::Subcatchment],
        "rain gage" | "gage" => &[ObjectKind::Gage],
        "object" | "name" => &[
            ObjectKind::Node,
            ObjectKind::Link,
            ObjectKind::Subcatchment,
            ObjectKind::Gage,
        ],
        _ => return None,
    };
    for k in kinds {
        if doc.defining_section(*k, name).is_some() {
            return Some(match k {
                ObjectKind::Node => ObjRef::Node(name.to_string()),
                ObjectKind::Link => ObjRef::Link(name.to_string()),
                ObjectKind::Subcatchment => ObjRef::Subcatchment(name.to_string()),
                _ => ObjRef::Gage(name.to_string()),
            });
        }
    }
    None
}

/// Select `target` on the map and zoom to it.
pub fn go_to(state: &mut AppState, target: ObjRef) {
    state.swmm_doc.select_only(target.clone());
    state.swmm_doc.pending_zoom_to = Some(target);
    state.swmm.sub_view = SwmmSubView::Map;
}

/// A report line with its index entry, for display.
fn explain(line: &str) -> (ParsedLine, Option<&'static errors::Code>) {
    let p = errors::parse_line(line);
    let e = p.entry();
    (p, e)
}

/// The panel as plain text.
pub fn summary_text(run: &Run, scratch_note: Option<&str>) -> String {
    let r = &run.report;
    let mut s = String::new();
    s.push_str(&format!(
        "SWMM run status\nEngine: EPA SWMM {} (sha256 {})\nModel: {}\nElapsed: {:.1} s\n",
        run.engine_version,
        run.engine_sha256,
        run.inp.display(),
        run.elapsed.as_secs_f64()
    ));
    if let Some(n) = scratch_note {
        s.push_str(n);
        s.push('\n');
    }
    match run.failure_reason() {
        None => s.push_str("Result: finished; results file written\n"),
        Some(why) => s.push_str(&format!("Result: FAILED — {why}\n")),
    }
    s.push_str(&format!(
        "\nContinuity (green < {CONTINUITY_AMBER}%, amber {CONTINUITY_AMBER}–{CONTINUITY_RED}%, red > {CONTINUITY_RED}%)\n"
    ));
    if r.continuity.is_empty() {
        s.push_str("  (none reported)\n");
    }
    for (section, pct) in &r.continuity {
        let band = if pct.abs() >= CONTINUITY_RED {
            "RED"
        } else if pct.abs() >= CONTINUITY_AMBER {
            "amber"
        } else {
            "green"
        };
        s.push_str(&format!("  {section}: {pct:+.3}% [{band}]\n"));
    }
    let lists: [(&str, &[RankedEntry], Option<&String>); 4] = [
        ("Highest continuity errors by node", &r.diagnostics.continuity_by_node, r.diagnostics.continuity_note.as_ref()),
        ("Time-step critical elements", &r.diagnostics.critical_elements, r.diagnostics.critical_note.as_ref()),
        ("Highest flow instability indexes", &r.diagnostics.instability, r.diagnostics.instability_note.as_ref()),
        ("Most frequent non-converging nodes", &r.diagnostics.nonconverging, r.diagnostics.nonconverging_note.as_ref()),
    ];
    for (title, entries, note) in lists {
        s.push_str(&format!("\n{title} ({})\n", entries.len()));
        for e in entries {
            s.push_str(&format!("  {} {} ({}{})\n", e.what, e.name, e.value, e.unit));
        }
        if let Some(n) = note {
            s.push_str(&format!("  {n}\n"));
        }
    }
    if let Some(t) = r.routing_time_step() {
        s.push_str("\nRouting time step summary\n");
        for row in &t.rows {
            if row.len() >= 2 {
                s.push_str(&format!("  {}: {}\n", row[0], row[1]));
            }
        }
    }
    s.push_str(&format!("\nErrors ({})\n", r.errors.len()));
    for line in &r.errors {
        let (_, e) = explain(line);
        s.push_str(&format!("  {line}\n"));
        if let Some(e) = e {
            s.push_str(&format!("    = {}. {} Fix: {}\n", e.meaning, e.cause, e.fix));
        }
    }
    s.push_str(&format!("\nWarnings ({})\n", r.warnings.len()));
    for line in &r.warnings {
        let (_, e) = explain(line);
        s.push_str(&format!("  {line}\n"));
        if let Some(e) = e {
            s.push_str(&format!("    = {}. {} Fix: {}\n", e.meaning, e.cause, e.fix));
        }
    }
    s
}

/// The status-bar line for a finished run, with the first error or
/// warning explained.
pub fn status_line(run: &Run) -> String {
    let base = match run.failure_reason() {
        None => format!(
            "SWMM: finished in {:.1} s, {} warning(s)",
            run.elapsed.as_secs_f64(),
            run.report.warnings.len()
        ),
        Some(why) => format!("SWMM: {why}"),
    };
    let first = run
        .report
        .errors
        .first()
        .or_else(|| run.report.warnings.first());
    match first.and_then(|l| errors::parse_line(l).entry()) {
        Some(e) => format!("{base} — {}: {}", e.label(), e.meaning),
        None => base,
    }
}

/// Once per frame: open the panel for a newly finished run, copy any
/// redirected SAVE files back, put the explanation on the status line,
/// clean stale scratch folders once, then draw.
pub fn draw_windows(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.run_panel.cleaned {
        state.swmm_doc.run_panel.cleaned = true;
        let _ = stormsewer_swmm::engine::clean_stale_scratch(std::time::Duration::from_secs(7 * 86_400));
    }
    if let Some(run) = state.swmm.last_run.as_ref() {
        let key = (run.out.clone(), run.engine_sha256.clone(), run.elapsed.as_nanos());
        if state.swmm_doc.run_panel.seen.as_ref() != Some(&key) {
            state.swmm_doc.run_panel.seen = Some(key);
            state.swmm_doc.run_panel.open = true;
            state.status = status_line(run);
            if let Some(prep) = state.swmm_doc.run_prep.as_ref() {
                let back = prep.copy_back();
                if !back.is_empty() {
                    state.status.push_str(&format!(" — {} SAVE file(s) copied back", back.len()));
                }
            }
        }
    }
    draw_run_status(ctx, state);
    draw_error_codes(ctx, state);
}

fn draw_run_status(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.run_panel.open {
        return;
    }
    let Some(run) = state.swmm.last_run.clone() else {
        state.swmm_doc.run_panel.open = false;
        return;
    };
    let scratch_note = state
        .swmm_doc
        .run_prep
        .as_ref()
        .and_then(|p| p.note.clone());
    let mut open = true;
    let mut go: Option<ObjRef> = None;
    let mut copy = false;
    let mut codes = false;
    let dark = ctx.style().visuals.dark_mode;
    egui::Window::new("Run Status")
        .id(egui::Id::new("swmm-run-status"))
        .open(&mut open)
        .default_size(egui::vec2(640.0, 560.0))
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Copy summary").clicked() {
                    copy = true;
                }
                if ui.button("Error codes…").clicked() {
                    codes = true;
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("swmm-run-status-scroll")
                .show(ui, |ui| {
                    header(ui, &run, scratch_note.as_deref(), dark);
                    continuity(ui, &run, dark);
                    messages(ui, state, &run, dark, &mut go);
                    diagnostics(ui, state, &run, &mut go);
                    routing_step(ui, &run);
                });
        });
    if copy {
        let text = summary_text(&run, scratch_note.as_deref());
        ctx.output_mut(|o| o.copied_text = text.clone());
        state.swmm_doc.run_panel.last_copied = Some(text);
        state.status = "Run summary copied".into();
    }
    if codes {
        state.swmm_doc.run_panel.codes_open = true;
    }
    if let Some(t) = go {
        go_to(state, t);
    }
    state.swmm_doc.run_panel.open = open;
}

fn header(ui: &mut Ui, run: &Run, scratch_note: Option<&str>, dark: bool) {
    match run.failure_reason() {
        None => {
            ui.label(
                RichText::new("Run finished and wrote results")
                    .strong()
                    .color(palette::ok_text(dark)),
            );
        }
        Some(why) => {
            ui.label(
                RichText::new(format!("Run failed: {why}"))
                    .strong()
                    .color(palette::error_text(dark)),
            );
        }
    }
    egui::Grid::new("swmm-run-header").num_columns(2).show(ui, |ui| {
        ui.label("Engine");
        ui.label(format!("EPA SWMM {}", run.engine_version));
        ui.end_row();
        ui.label("Binary sha256");
        ui.monospace(RichText::new(&run.engine_sha256).small())
            .on_hover_text("SHA-256 of the runswmm executable that produced this run");
        ui.end_row();
        ui.label("Elapsed");
        ui.label(format!("{:.2} s", run.elapsed.as_secs_f64()));
        ui.end_row();
        ui.label("Model read");
        ui.label(RichText::new(run.inp.display().to_string()).small());
        ui.end_row();
        ui.label("Report");
        ui.label(RichText::new(run.rpt.display().to_string()).small());
        ui.end_row();
    });
    if let Some(n) = scratch_note {
        ui.label(RichText::new(n).small().color(palette::warning_text(dark)));
    }
}

fn continuity(ui: &mut Ui, run: &Run, dark: bool) {
    ui.add_space(8.0);
    ui.label(RichText::new("Continuity").strong());
    ui.label(
        RichText::new(format!(
            "green below {CONTINUITY_AMBER}%, amber {CONTINUITY_AMBER}–{CONTINUITY_RED}%, red above {CONTINUITY_RED}% (absolute)"
        ))
        .small()
        .weak(),
    );
    if run.report.continuity.is_empty() {
        ui.label("No continuity section: the run did not get that far.");
    }
    for (section, pct) in &run.report.continuity {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{pct:+.3}%"))
                    .monospace()
                    .color(continuity_color(*pct, dark)),
            );
            ui.label(section);
        });
    }
}

fn messages(ui: &mut Ui, state: &AppState, run: &Run, dark: bool, go: &mut Option<ObjRef>) {
    let groups: [(&str, &[String], Color32); 2] = [
        ("Errors", &run.report.errors, palette::error_text(dark)),
        ("Warnings", &run.report.warnings, palette::warning_text(dark)),
    ];
    for (title, lines, color) in groups {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{title} ({})", lines.len())).strong());
        if lines.is_empty() {
            ui.label(RichText::new("none").weak());
        }
        for line in lines {
            let (p, entry) = explain(line);
            let target = p
                .object
                .as_ref()
                .and_then(|(what, name)| resolve_object(state, what, name));
            let text = RichText::new(line).color(color);
            if let Some(t) = target {
                if ui
                    .selectable_label(state.swmm_doc.is_selected(&t), text)
                    .on_hover_text("Select on the map")
                    .clicked()
                {
                    *go = Some(t);
                }
            } else {
                ui.label(text);
            }
            if let Some(e) = entry {
                ui.indent(("swmm-run-msg", line), |ui| {
                    ui.label(RichText::new(e.meaning).small());
                    ui.label(RichText::new(format!("{} Fix: {}", e.cause, e.fix)).small().weak());
                });
            }
        }
    }
}

fn diagnostics(ui: &mut Ui, state: &AppState, run: &Run, go: &mut Option<ObjRef>) {
    let d = &run.report.diagnostics;
    let lists: [(&str, &[RankedEntry], Option<&String>, &str); 4] = [
        ("Highest continuity errors by node", &d.continuity_by_node, d.continuity_note.as_ref(), "% of the node's inflow volume"),
        ("Time-step critical elements", &d.critical_elements, d.critical_note.as_ref(), "% of steps this element set the time step"),
        ("Highest flow instability indexes", &d.instability, d.instability_note.as_ref(), "flow reversals per reporting period, 0–150"),
        ("Most frequent non-converging nodes", &d.nonconverging, d.nonconverging_note.as_ref(), "% of steps the node did not converge"),
    ];
    for (title, entries, note, unit_hint) in lists {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{title} ({})", entries.len())).strong())
            .on_hover_text(unit_hint);
        if let Some(n) = note {
            ui.label(RichText::new(n).weak());
        }
        for e in entries {
            let target = resolve_object(state, &e.what, &e.name);
            let text = format!("{} {}  {}{}", e.what, e.name, e.value, e.unit);
            match target {
                Some(t) => {
                    if ui
                        .selectable_label(state.swmm_doc.is_selected(&t), RichText::new(text).monospace())
                        .clicked()
                    {
                        *go = Some(t);
                    }
                }
                None => {
                    ui.label(RichText::new(text).monospace());
                }
            }
        }
    }
}

fn routing_step(ui: &mut Ui, run: &Run) {
    let Some(t) = run.report.routing_time_step() else { return };
    ui.add_space(8.0);
    ui.label(RichText::new("Routing time step summary").strong());
    egui::Grid::new("swmm-run-step").num_columns(2).striped(true).show(ui, |ui| {
        for row in &t.rows {
            if row.len() >= 2 {
                ui.label(&row[0]);
                ui.monospace(&row[1]);
                ui.end_row();
            }
        }
    });
}

/// Results → Run Status… and Help → SWMM Error Codes….
pub fn results_menu_item(ui: &mut Ui, state: &mut AppState) {
    if ui
        .add_enabled(state.swmm.last_run.is_some(), Button::new("Run Status…"))
        .on_disabled_hover_text("Run the model first")
        .clicked()
    {
        state.swmm_doc.run_panel.open = true;
        ui.close_menu();
    }
}

pub fn help_menu_item(ui: &mut Ui, state: &mut AppState) {
    if ui.button("SWMM Error Codes…").clicked() {
        state.swmm_doc.run_panel.codes_open = true;
        ui.close_menu();
    }
}

fn draw_error_codes(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.run_panel.codes_open {
        return;
    }
    let mut open = true;
    let dark = ctx.style().visuals.dark_mode;
    egui::Window::new("SWMM Error Codes")
        .id(egui::Id::new("swmm-error-codes"))
        .open(&mut open)
        .default_size(egui::vec2(620.0, 520.0))
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(
                    egui::TextEdit::singleline(&mut state.swmm_doc.run_panel.codes_query)
                        .id(egui::Id::new("swmm-error-codes-search"))
                        .hint_text("code, word, or section")
                        .desired_width(260.0),
                );
            });
            ui.label(
                RichText::new("Codes from the EPA SWMM 5.2 User's Manual, Appendix E; the causes and fixes are what the forum threads show people needed.")
                    .small()
                    .weak(),
            );
            ui.separator();
            let hits = errors::search(&state.swmm_doc.run_panel.codes_query);
            ui.label(RichText::new(format!("{} of {} codes", hits.len(), errors::ALL.len())).small());
            egui::ScrollArea::vertical()
                .id_salt("swmm-error-codes-scroll")
                .show(ui, |ui| {
                    for c in hits {
                        let color = match c.kind {
                            Kind::Error => palette::error_text(dark),
                            Kind::Warning => palette::warning_text(dark),
                        };
                        ui.label(RichText::new(format!("{} — {}", c.label(), c.meaning)).strong().color(color));
                        ui.indent(("swmm-code", c.kind == Kind::Error, c.code), |ui| {
                            ui.label(RichText::new(c.cause).small());
                            ui.label(RichText::new(format!("Fix: {}", c.fix)).small().weak());
                        });
                        ui.add_space(4.0);
                    }
                });
        });
    state.swmm_doc.run_panel.codes_open = open;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::swmm_report::tests::fixture_run;
    use crate::StormSewerApp;
    use stormsewer_swmm::rpt;

    /// A report shaped like a bad run: errors, warnings naming objects,
    /// a red continuity error, and full diagnostic lists.
    fn troubled_run() -> Run {
        let mut run = fixture_run();
        let text = [
            "  EPA STORM WATER MANAGEMENT MODEL - VERSION 5.2 (Build 5.2.4)",
            "",
            "  WARNING 03: negative offset ignored for Link C2",
            "  WARNING 02: max depth increased for Node J1",
            "",
            "  **************************        Volume        Volume",
            "  Flow Routing Continuity        acre-feet      10^6 gal",
            "  **************************     ---------     ---------",
            "  Continuity Error (%) .....        12.500",
            "",
            "  **************************        Volume         Depth",
            "  Runoff Quantity Continuity     acre-feet        inches",
            "  **************************     ---------     ---------",
            "  Continuity Error (%) .....         2.000",
            "",
            "  *************************",
            "  Highest Continuity Errors",
            "  *************************",
            "  Node J1 (5.23%)",
            "  Node SU1 (1.00%)",
            "  Node J2 (0.90%)",
            "  Node J3 (0.80%)",
            "  Node J4 (0.70%)",
            "  Node J5 (0.60%)",
            "  Node NOPE (0.50%)",
            "",
            "  ***************************",
            "  Time-Step Critical Elements",
            "  ***************************",
            "  Link C3 (45.20%)",
            "",
            "  ********************************",
            "  Highest Flow Instability Indexes",
            "  ********************************",
            "  Link C1 (12)",
            "",
            "  *********************************",
            "  Most Frequent Nonconverging Nodes",
            "  *********************************",
            "  Convergence obtained at all time steps.",
            "",
            "  *************************",
            "  Routing Time Step Summary",
            "  *************************",
            "  Minimum Time Step           :     0.50 sec",
            "  Average Iterations per Step :     2.05",
            "",
        ]
        .join("
");
        run.report = rpt::parse(&text);
        run
    }

    #[test]
    fn panel_opens_when_a_run_finishes_and_entries_resolve_to_map_objects() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), None);
        assert!(!app.state.swmm_doc.run_panel.open);
        app.state.swmm.last_run = Some(troubled_run());
        run_frame(&mut app);
        assert!(app.state.swmm_doc.run_panel.open, "opened by itself");
        assert!(
            app.state.status.contains("WARNING 03: A negative link offset was set to zero"),
            "status explains the code: {}",
            app.state.status
        );
        // A second frame does not reopen a closed panel for the same run.
        app.state.swmm_doc.run_panel.open = false;
        run_frame(&mut app);
        assert!(!app.state.swmm_doc.run_panel.open);

        // Entries resolve to the model's objects, and only to real ones.
        let run = app.state.swmm.last_run.clone().unwrap();
        let d = &run.report.diagnostics;
        assert_eq!(d.continuity_by_node.len(), 7, "all seven, not five");
        assert_eq!(resolve_object(&app.state, "Node", "J1"), Some(ObjRef::Node("J1".into())));
        assert_eq!(resolve_object(&app.state, "Node", "SU1"), Some(ObjRef::Node("SU1".into())));
        assert_eq!(resolve_object(&app.state, "Link", "C3"), Some(ObjRef::Link("C3".into())));
        assert_eq!(resolve_object(&app.state, "Node", "NOPE"), None);
        assert_eq!(resolve_object(&app.state, "object", "S1"), Some(ObjRef::Subcatchment("S1".into())));
        let p = errors::parse_line(&run.report.warnings[0]);
        let (what, name) = p.object.unwrap();
        let t = resolve_object(&app.state, &what, &name).unwrap();
        assert_eq!(t, ObjRef::Link("C2".into()));
        go_to(&mut app.state, t.clone());
        assert_eq!(app.state.swmm_doc.selection, vec![t.clone()]);
        assert_eq!(app.state.swmm_doc.pending_zoom_to, Some(t));
        assert_eq!(app.state.swmm.sub_view, SwmmSubView::Map);

        // The plain-text summary carries everything.
        let text = summary_text(&run, Some("Ran a scratch copy"));
        assert!(text.contains("EPA SWMM 5.2.4"));
        assert!(text.contains(&"deadbeef".repeat(8)));
        assert!(text.contains("Flow Routing Continuity: +12.500% [RED]"), "{text}");
        assert!(text.contains("Runoff Quantity Continuity: +2.000% [amber]"), "{text}");
        assert!(text.contains("Highest continuity errors by node (7)"));
        assert!(text.contains("Node NOPE (0.5%)"));
        assert!(text.contains("Link C1 (12)"));
        assert!(text.contains("Convergence obtained at all time steps."));
        assert!(text.contains("Average Iterations per Step: 2.05"));
        assert!(text.contains("WARNING 03: negative offset ignored for Link C2"));
        assert!(text.contains("= A negative link offset was set to zero."));
        assert!(text.contains("Ran a scratch copy"));
        assert!(text.contains("green < 1%, amber 1–10%, red > 10%"));

        // Colours follow the thresholds.
        assert_eq!(continuity_color(0.5, false), palette::ok_text(false));
        assert_eq!(continuity_color(-1.0, false), palette::warning_text(false));
        assert_eq!(continuity_color(12.5, false), palette::error_text(false));

        // The window draws with the panel open, and the error index too.
        app.state.swmm_doc.run_panel.open = true;
        app.state.swmm_doc.run_panel.codes_open = true;
        app.state.swmm_doc.run_panel.codes_query = "hotstart".into();
        run_frame(&mut app);
        assert!(app.state.swmm_doc.run_panel.open);
        assert!(app.state.swmm_doc.run_panel.codes_open);
        assert!(errors::search("hotstart").iter().any(|c| c.code == 335));
        // Menu items draw.
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                results_menu_item(ui, &mut state);
                help_menu_item(ui, &mut state);
            });
        });
    }

    #[test]
    fn a_clean_run_reads_as_finished_with_a_green_continuity() {
        let run = fixture_run();
        let line = status_line(&run);
        assert!(line.starts_with("SWMM: finished in 1.2 s"), "{line}");
        let text = summary_text(&run, None);
        assert!(text.contains("Result: finished; results file written"));
        assert!(text.contains("[green]"), "{text}");
        assert!(text.contains("Highest flow instability indexes (0)"));
        assert!(text.contains("All links are stable."));
        assert!(text.contains("Errors (0)"));
        let mut failed = fixture_run();
        failed.report.errors.push("ERROR 209: undefined object RG9 at line 40 of [SUBCATCHMENTS]".into());
        let line = status_line(&failed);
        assert!(line.contains("ERROR 209: Undefined object"), "{line}");
        assert!(summary_text(&failed, None).contains("Result: FAILED — ERROR 209"));
    }
}
