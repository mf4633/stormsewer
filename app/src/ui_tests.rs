// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless frontend tests: menu accessibility, full-frame rendering across
//! app states, frontend-edit -> report fidelity, and an end-to-end session.
//!
//! egui is a pure-CPU immediate-mode library, so complete frames (menus,
//! panels, windows, plan/profile canvases) render headlessly through the
//! same `StormSewerApp::ui` the desktop window runs. No file-picker dialog
//! ever opens here: dialog-backed menu items are exercised at the layer
//! beneath the picker (the same functions the dialogs feed a path into).

use crate::edit::{delete_selection, place_pipe, place_structure, Tool};
use crate::help::{open_help, HelpTopic};
use crate::panels::SideTab;
use crate::state::{AppState, ViewTab};
use crate::StormSewerApp;
use eframe::egui;

// --- harness ----------------------------------------------------------------

fn raw_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1400.0, 900.0),
        )),
        ..Default::default()
    }
}

/// Run one complete application frame headlessly.
fn run_frame(app: &mut StormSewerApp) {
    let ctx = egui::Context::default();
    let _ = ctx.run(raw_input(), |ctx| app.ui(ctx));
}

/// Run one frame with extra input events (keyboard shortcuts etc.).
fn run_frame_with_events(app: &mut StormSewerApp, events: Vec<egui::Event>) {
    let ctx = egui::Context::default();
    let mut input = raw_input();
    input.events = events;
    let _ = ctx.run(input, |ctx| app.ui(ctx));
}

fn headless_ctx() -> egui::Context {
    egui::Context::default()
}

fn node_mut<'a>(
    s: &'a mut AppState,
    id: &str,
) -> &'a mut stormsewer::io::ProjectNode {
    s.project.nodes.iter_mut().find(|n| n.id == id).unwrap()
}

/// A network built through the same placement functions the canvas tools
/// call. `Project::empty()` deliberately seeds an "OUT" outfall, so the
/// fixture drains two placed structures into it: N1 (inlet) -> N2
/// (junction) -> OUT, with distinctive values the fidelity tests verify
/// through the computed results (Q = C*i*A).
fn built_state() -> AppState {
    let mut s = AppState::new_empty();
    assert_eq!(s.project.nodes.len(), 1, "empty project seeds exactly OUT");
    let n1 = place_structure(&mut s.project, &mut s.edit, "inlet", 0.0, 600.0);
    let n2 = place_structure(&mut s.project, &mut s.edit, "junction", 0.0, 300.0);
    place_pipe(&mut s.project, &mut s.edit, &n1, &n2).unwrap();
    place_pipe(&mut s.project, &mut s.edit, &n2, "OUT").unwrap();
    {
        let n = node_mut(&mut s, "N1");
        n.area_ac = 1.23;
        n.c = 0.77;
        n.invert = 96.0;
        n.rim = 104.0;
    }
    {
        let n = node_mut(&mut s, "N2");
        n.invert = 94.0;
        n.rim = 102.0;
    }
    {
        let n = node_mut(&mut s, "OUT");
        n.invert = 90.0;
        n.rim = 98.0;
    }
    s
}

fn analyzed_state() -> AppState {
    let mut s = built_state();
    s.run_analysis();
    assert!(s.analysis.is_some(), "fixture must analyze: {}", s.report_text);
    s
}

// --- full-frame rendering across app states ---------------------------------

#[test]
fn full_frame_renders_empty_project() {
    let mut app = StormSewerApp::new_for_test(AppState::new_empty());
    run_frame(&mut app);
    run_frame(&mut app); // second frame: layout settled
}

#[test]
fn full_frame_renders_demo_project() {
    let mut app = StormSewerApp::new_for_test(AppState::new_demo());
    run_frame(&mut app);
}

#[test]
fn full_frame_renders_analyzed_plan_and_profile() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    app.state.view_tab = ViewTab::Plan;
    run_frame(&mut app);
    app.state.view_tab = ViewTab::Profile;
    run_frame(&mut app);
}

#[test]
fn full_frame_renders_every_tool() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    for tool in Tool::all() {
        app.state.set_tool(tool);
        run_frame(&mut app);
    }
}

#[test]
fn full_frame_renders_every_side_tab() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    for tab in [SideTab::Parameters, SideTab::Tables, SideTab::Review] {
        app.state.side_tab = tab;
        run_frame(&mut app);
    }
}

#[test]
fn full_frame_renders_with_selection_and_inspector() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    app.state.set_selection(Some(0), None, None);
    app.state.inspector_open = true;
    run_frame(&mut app);
    app.state.set_selection(None, Some(0), None);
    run_frame(&mut app);
}

#[test]
fn full_frame_renders_all_windows_open() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    app.state.show_global_edit = true;
    app.state.show_report_editor = true;
    app.state.open_tc_calculator();
    app.state.tutorial.open = true;
    app.state.tutorial.step = 0;
    open_help(&mut app.state.help, HelpTopic::GettingStarted);
    app.show_about = true;
    app.state.show_multi_rp = true;
    run_frame(&mut app);
}

#[test]
fn full_frame_renders_si_units() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    app.state.convert_units(stormsewer::units::UnitSystem::Si);
    app.state.run_analysis();
    run_frame(&mut app);
}

#[test]
fn help_window_renders_every_topic() {
    let topics = [
        HelpTopic::GettingStarted,
        HelpTopic::QuickStart,
        HelpTopic::KeyboardShortcuts,
        HelpTopic::DesignWorkflow,
        HelpTopic::DesignCodes,
        HelpTopic::Hydrology,
        HelpTopic::Hydraulics,
        HelpTopic::InletsHeC22,
        HelpTopic::FileIo,
        HelpTopic::Reports,
        HelpTopic::HydraflowMigration,
    ];
    let mut app = StormSewerApp::new_for_test(AppState::new_demo());
    for topic in topics {
        open_help(&mut app.state.help, topic);
        run_frame(&mut app);
        assert!(app.state.help.open);
    }
}

#[test]
fn tutorial_renders_every_step() {
    let mut app = StormSewerApp::new_for_test(AppState::new_demo());
    app.state.tutorial.open = true;
    for step in 0..11 {
        app.state.tutorial.step = step;
        run_frame(&mut app);
    }
}

// --- menu accessibility ------------------------------------------------------

/// Every top-level menu's contents render headlessly (the closure bodies the
/// user sees when the menu opens), against a state where every item is live:
/// a project with results, a selection, and a recent-files entry.
#[test]
fn every_menu_renders_open() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    app.state.set_selection(Some(0), None, None);
    app.state.recent.push(std::env::temp_dir().join("ui-test-recent.ssproj"));
    let ctx = headless_ctx();
    let _ = ctx.run(raw_input(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            app.file_menu(ui, ctx);
            app.edit_menu(ui);
            app.tools_menu(ui);
            app.view_menu(ui, ctx);
            app.help_menu(ui);
        });
    });
}

/// Source-derived inventory: every menu item in main.rs must be one this
/// suite knows about. Adding a menu item without extending the covered list
/// fails this test — coverage can't silently rot.
#[test]
fn menu_inventory_is_covered() {
    let src = include_str!("main.rs");
    let mut labels = vec![];
    for line in src.lines() {
        let l = line.trim();
        for pat in ["ui.button(\"", "egui::Button::new(\"", "menu_button(\""] {
            if let Some(i) = l.find(pat) {
                let rest = &l[i + pat.len()..];
                if let Some(j) = rest.find('"') {
                    labels.push(rest[..j].to_string());
                }
            }
        }
    }
    let covered = [
        "File", "New Project", "New Demo Project", "Open Project…",
        "Recent Projects", "Save Project…", "Import DXF…", "Import LandXML…",
        "Import Hydraflow STM…", "Export DXF…", "Export LandXML…",
        "Load PNG Background…", "Export PDF Report…", "Export HTML Report…",
        "Print Report (Ctrl+P)", "Custom Report (MyReport)",
        "Municipal Summary", "Hydraflow Pipe Table", "Cost Report",
        "Export Custom CSV…", "Export Custom HTML…", "Load Template (.srpt)…",
        "Save Template (.srpt)…", "Edit Columns…",
        "Edit", "Undo", "Redo", "Global Pipe Editing…",
        "Tools", "Tc Calculator…", "Run Diagnostics",
        "View", "Zoom Extents (F)", "Zoom to Selection (G)",
        "Help", "Interactive Tutorial", "Getting Started",
        "Quick Start Tutorial", "Design Workflow", "Computational Methods",
        "File Import & Export", "Hydraflow Migration Guide",
        "Keyboard Shortcuts…", "Troubleshooting", "About StormSewer…",
        "Close",
    ];
    for label in &labels {
        assert!(
            covered.contains(&label.as_str()),
            "menu/button label {label:?} in main.rs is not in the covered \
             inventory — extend ui_tests to exercise it, then add it here"
        );
    }
    // And the inventory can't drift ahead of the source either.
    for c in covered {
        assert!(
            labels.iter().any(|l| l == c),
            "covered label {c:?} no longer exists in main.rs"
        );
    }
}

/// Every non-dialog menu action, invoked through the same state calls the
/// menu handlers make, asserting its user-visible effect.
#[test]
fn menu_actions_have_their_effects() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());

    // File > New Project / New Demo Project
    app.reset_project(AppState::new_empty());
    assert_eq!(app.state.project.nodes.len(), 1, "New Project seeds OUT");
    assert_eq!(app.state.project.nodes[0].id, "OUT");
    app.reset_project(AppState::new_demo());
    assert!(app.state.project.nodes.len() > 1);

    // File > Custom Report templates
    for (tpl, marker) in [
        (stormsewer::io::ReportTemplate::municipal_summary(), "Municipal"),
        (stormsewer::io::ReportTemplate::hydraflow_style(), "Hydraflow"),
        (stormsewer::io::ReportTemplate::cost_report(), "Cost"),
    ] {
        app.state.set_report_template(tpl);
        assert!(
            app.state.report_template.name.contains(marker),
            "template {marker} not applied: {}",
            app.state.report_template.name
        );
    }

    // File > Edit Columns… / Edit > Global Pipe Editing…
    app.state.show_report_editor = true;
    app.state.show_global_edit = true;
    run_frame(&mut app);

    // Edit > Undo / Redo through a real mutation
    let before = app.state.project.nodes[0].invert;
    app.state.checkpoint_undo();
    app.state.project.nodes[0].invert = before - 2.0;
    app.state.undo();
    assert_eq!(app.state.project.nodes[0].invert, before);
    app.state.redo();
    assert_eq!(app.state.project.nodes[0].invert, before - 2.0);
    app.state.undo();

    // Tools > Tc Calculator… / Run Diagnostics
    app.state.open_tc_calculator();
    assert!(app.state.tc_calc.open);
    app.state.diagnostics_text.clear();
    app.state.update_diagnostics();
    assert!(!app.state.diagnostics_text.is_empty());

    // View > Zoom Extents / Zoom to Selection / tab switch
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    app.state.viewport.zoom_to_fit(rect, &app.state.project);
    app.state.set_selection(Some(0), None, None);
    app.state
        .viewport
        .zoom_to_selection(rect, &app.state.project, Some(0), None);
    app.state.view_tab = ViewTab::Profile;
    run_frame(&mut app);
    app.state.view_tab = ViewTab::Plan;

    // View > theme (field only — never prefs.save(), which writes user config)
    app.state.prefs.theme = crate::theme::Theme::Light;
    crate::theme::apply(&headless_ctx(), app.state.prefs.theme);
    run_frame(&mut app);

    // Help > every entry
    app.state.tutorial.open = true;
    app.state.tutorial.step = 0;
    open_help(&mut app.state.help, HelpTopic::KeyboardShortcuts);
    assert!(app.state.help.open);
    app.show_about = true;
    run_frame(&mut app);

    // File > Open report after export toggle
    let was = app.state.open_report_after_export;
    app.state.open_report_after_export = !was;
    assert_ne!(app.state.open_report_after_export, was);
}

#[test]
fn keyboard_shortcuts_dispatch() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());

    // Num2 selects the inlet tool
    run_frame_with_events(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Num2,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.state.tool, Tool::PlaceInlet);

    // Ctrl+Z undoes a checkpointed edit
    let before = app.state.project.nodes[0].rim;
    app.state.checkpoint_undo();
    app.state.project.nodes[0].rim = before + 5.0;
    run_frame_with_events(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
    );
    assert_eq!(app.state.project.nodes[0].rim, before);

    // F5 re-runs analysis after a stale edit
    app.state.analysis = None;
    run_frame_with_events(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::F5,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.state.analysis.is_some());
}

// --- frontend edits -> report fidelity ---------------------------------------

#[test]
fn report_reflects_placed_network_and_values() {
    let s = analyzed_state();
    let r = &s.report_text;
    for id in ["N1", "N2", "OUT", "P1", "P2"] {
        assert!(r.contains(id), "report missing {id}:\n{r}");
    }
    // The frontend-entered area and C must drive the computed hydrology:
    // Q(P1) = C * i(Tc) * A with the project IDF i = a/(t+b)^c at Tc=10.
    let a = s.analysis.as_ref().unwrap();
    let p1 = a.pipes.iter().find(|p| p.id == "P1").unwrap();
    let i10 =
        s.project.idf_a / (10.0_f64 + s.project.idf_b).powf(s.project.idf_c);
    let q_expected = 0.77 * 1.23 * i10;
    assert!(
        (p1.design_q - q_expected).abs() < 1e-6,
        "Q(P1)={} but C*i*A={} — frontend values not driving analysis",
        p1.design_q,
        q_expected
    );
    assert!(!s.multi_rp_text.is_empty(), "multi-RP table empty");
    assert!(!s.review_text.is_empty(), "review text empty");
    assert!(!s.cost_text.is_empty(), "cost text empty");
    assert!(!s.diagnostics_text.is_empty(), "diagnostics empty");
}

#[test]
fn report_updates_when_frontend_edits_change() {
    let mut s = analyzed_state();

    // Global diameter change re-runs the analysis itself; capacity must
    // jump by ~ (D2/D1)^(8/3) and the status line must announce the edit.
    let cap_before = s.analysis.as_ref().unwrap().pipes[0].capacity;
    let report_before = s.report_text.clone();
    s.global_set_pipe_diameter_in(30.0);
    assert!(s.status.contains("30 in"), "status: {}", s.status);
    let cap_after = s.analysis.as_ref().unwrap().pipes[0].capacity;
    assert!(
        cap_after > cap_before * 3.0,
        "capacity {cap_before} -> {cap_after}: diameter edit not applied"
    );
    assert_ne!(s.report_text, report_before, "report unchanged after edit");

    // Node attribute edit through the inspector's binding target: Q must
    // scale exactly with the area, and the stale flag must gate it.
    let q_before = s.analysis.as_ref().unwrap().pipes[0].design_q;
    s.checkpoint_undo();
    node_mut(&mut s, "N1").area_ac = 4.56;
    s.mark_analysis_stale();
    assert!(s.analysis_stale, "edit must mark analysis stale");
    s.run_analysis();
    let q_after = s.analysis.as_ref().unwrap().pipes[0].design_q;
    assert!(
        (q_after / q_before - 4.56 / 1.23).abs() < 1e-9,
        "Q must scale with area: {q_before} -> {q_after}"
    );

    // Undo restores the value; re-analysis restores the result.
    s.undo();
    s.run_analysis();
    let q_undone = s.analysis.as_ref().unwrap().pipes[0].design_q;
    assert!((q_undone - q_before).abs() < 1e-9, "undo not reflected");
}

#[test]
fn report_follows_unit_system_toggle() {
    let mut s = analyzed_state();
    let us_report = s.report_text.clone();
    let q_us = s.analysis.as_ref().unwrap().pipes[0].design_q;

    s.convert_units(stormsewer::units::UnitSystem::Si);
    s.run_analysis();
    assert!(s.analysis.is_some(), "SI analysis failed: {}", s.report_text);
    assert_ne!(s.report_text, us_report, "report identical after SI toggle");

    // Round-trip back: design flow must be preserved (engine guarantees
    // unit invariance; the frontend toggle must not corrupt the project).
    s.convert_units(stormsewer::units::UnitSystem::UsCustomary);
    s.run_analysis();
    let q_back = s.analysis.as_ref().unwrap().pipes[0].design_q;
    assert!(
        (q_back - q_us).abs() < 1e-6,
        "unit round-trip changed design flow: {q_us} -> {q_back}"
    );
}

#[test]
fn custom_report_templates_render_frontend_values() {
    let s = analyzed_state();
    let analysis = s.analysis.as_ref().unwrap();
    for tpl in [
        stormsewer::io::ReportTemplate::municipal_summary(),
        stormsewer::io::ReportTemplate::hydraflow_style(),
        stormsewer::io::ReportTemplate::cost_report(),
    ] {
        let csv = stormsewer::io::render_csv(&s.project, analysis, &tpl);
        assert!(csv.contains("P1"), "{}: CSV missing pipe id\n{csv}", tpl.name);
        let html = stormsewer::io::render_html_table(&s.project, analysis, &tpl);
        assert!(html.contains("P1"), "{}: HTML missing pipe id", tpl.name);
    }
}

#[test]
fn html_report_contains_frontend_values() {
    let s = analyzed_state();
    let dir = std::env::temp_dir().join("stormsewer-ui-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("report.html");
    stormsewer::io::export_html(&s.project, s.analysis.as_ref().unwrap(), &path)
        .unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    for needle in ["N1", "P2", "OUT"] {
        assert!(html.contains(needle), "HTML report missing {needle}");
    }
}

// --- end-to-end session ------------------------------------------------------

#[test]
fn e2e_place_edit_analyze_save_reload_report() {
    let dir = std::env::temp_dir().join("stormsewer-ui-tests");
    std::fs::create_dir_all(&dir).unwrap();

    // Place and edit through the frontend code paths.
    let mut s = built_state();
    s.run_analysis();
    assert!(s.analysis.is_some());
    let report_before = s.report_text.clone();

    // Save the project the way File > Save does beneath the picker.
    let path = dir.join("e2e.ssproj");
    s.project.save(&path).unwrap();

    // Reload into a fresh app the way File > Open does beneath the picker.
    let mut app = StormSewerApp::new_for_test(AppState::new_empty());
    let ctx = headless_ctx();
    app.state.open_project_path(&ctx, path.clone());
    assert_eq!(app.state.project.nodes.len(), 3); // OUT + N1 + N2
    assert_eq!(app.state.project.pipes.len(), 2);
    assert!(
        app.state.recent.paths.iter().any(|p| p == &path),
        "recent files not updated on open"
    );
    app.state.run_analysis();
    assert_eq!(
        app.state.report_text, report_before,
        "reloaded project produces a different report"
    );

    // Delete a structure, re-analyze, verify the report tracks it; undo
    // brings it back.
    let n1_idx = app
        .state
        .project
        .nodes
        .iter()
        .position(|n| n.id == "N1")
        .unwrap();
    app.state.set_selection(Some(n1_idx), None, None);
    app.state.checkpoint_undo();
    delete_selection(&mut app.state.project, Some(n1_idx), None).unwrap();
    app.state.clear_selection();
    app.state.run_analysis();
    assert!(!app.state.report_text.contains("N1"), "deleted node in report");
    app.state.undo();
    app.state.run_analysis();
    assert_eq!(app.state.report_text, report_before, "undo+rerun differs");

    // Exports the File menu offers, at the layer beneath the pickers.
    let analysis = app.state.analysis.clone().unwrap();
    let pdf = dir.join("e2e.pdf");
    stormsewer::io::export_pdf(&app.state.project, &analysis, &pdf, None).unwrap();
    assert!(pdf.metadata().unwrap().len() > 1000, "PDF suspiciously small");
    let dxf = dir.join("e2e.dxf");
    stormsewer::io::export_dxf(&app.state.project, &dxf).unwrap();
    assert!(std::fs::read_to_string(&dxf).unwrap().contains("ENTITIES"));
    let xml = dir.join("e2e.xml");
    stormsewer::io::export_landxml(&app.state.project, &xml).unwrap();
    assert!(std::fs::read_to_string(&xml).unwrap().contains("LandXML"));

    // Full frames still render after the whole session.
    let mut app2 = StormSewerApp::new_for_test(app.state);
    run_frame(&mut app2);
}

#[test]
fn e2e_catchment_drawing_reflects_in_analysis() {
    use crate::catchment_draw::handle_catchment_click;
    let mut s = built_state();
    s.run_analysis();
    let n_catchments = s.project.catchments.len();

    // Draw a square catchment and close it on the first vertex.
    handle_catchment_click(&mut s.project, &mut s.edit, 100.0, 100.0);
    handle_catchment_click(&mut s.project, &mut s.edit, 300.0, 100.0);
    handle_catchment_click(&mut s.project, &mut s.edit, 300.0, 300.0);
    handle_catchment_click(&mut s.project, &mut s.edit, 100.0, 300.0);
    let msg = handle_catchment_click(&mut s.project, &mut s.edit, 100.0, 100.0);
    assert!(
        s.project.catchments.len() == n_catchments + 1,
        "catchment not committed (last msg: {msg:?})"
    );
    s.run_analysis();
    assert!(s.analysis.is_some());
}
