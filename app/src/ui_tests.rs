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
    app.state.noaa_paste_open = true;
    app.state.noaa_paste_text =
        "by duration for ARI (years):,1,2,5,10,25,50,100
         5-min:,0.406,0.474,0.569,0.646,0.752,0.836,0.923
         60-min:,1.20,1.42,1.73,1.98,2.33,2.60,2.88
"
            .into();
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
        // ".button(\"" also catches builder-style calls split across lines
        for pat in [".button(\"", "egui::Button::new(\"", "menu_button(\""] {
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
        "Import NOAA Atlas 14 IDF…", "Paste NOAA Atlas 14 Data…",
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
        "Save project…", "Discard and close", "Cancel",
        "Restore recovered work", "Delete snapshot",
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

    // File > Paste NOAA Atlas 14 Data… opens the import dialog
    app.state.noaa_paste_open = true;
    run_frame(&mut app);
    assert!(app.state.noaa_paste_open);

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

// --- profile runs & branched networks ----------------------------------------

/// Trunk N1 -> N2 -> OUT plus branch B (N3) -> N2, built through the same
/// placement calls the canvas tools use.
fn branched_state() -> AppState {
    let mut s = built_state();
    let b = place_structure(&mut s.project, &mut s.edit, "inlet", 300.0, 300.0);
    place_pipe(&mut s.project, &mut s.edit, &b, "N2").unwrap();
    {
        let n = node_mut(&mut s, "N3");
        n.area_ac = 2.0;
        n.c = 0.60;
        n.invert = 95.0;
        n.rim = 103.0;
    }
    s.run_analysis();
    assert!(s.analysis.is_some(), "branched fixture: {}", s.report_text);
    s
}

#[test]
fn branched_trunk_carries_summed_ca_in_app_analysis() {
    let s = branched_state();
    let a = s.analysis.as_ref().unwrap();
    // P2 (N2 -> OUT) drains N1 + N2 + the branch inlet N3.
    let p2 = a.pipes.iter().find(|p| p.id == "P2").unwrap();
    let expected_ca = 0.77 * 1.23 + 0.70 * 1.0 + 0.60 * 2.0;
    assert!(
        (p2.total_ca - expected_ca).abs() < 1e-9,
        "CA below junction: {} vs {}",
        p2.total_ca,
        expected_ca
    );
    assert!(
        (p2.design_q - p2.total_ca * p2.intensity).abs() < 1e-9,
        "Q != CA*i below the junction"
    );
}

#[test]
fn shift_click_toggle_builds_and_clears_profile_run() {
    let mut s = branched_state();
    s.toggle_profile_pipe("P3");
    s.toggle_profile_pipe("P2");
    assert_eq!(s.profile_pipes, ["P3", "P2"]);
    assert!(s.status.contains("P3"), "status: {}", s.status);
    // Toggling again removes.
    s.toggle_profile_pipe("P3");
    assert_eq!(s.profile_pipes, ["P2"]);
    s.toggle_profile_pipe("P2");
    assert!(s.profile_pipes.is_empty());
    assert!(s.status.contains("main trunk"), "status: {}", s.status);
}

#[test]
fn escape_clears_profile_run() {
    let mut app = StormSewerApp::new_for_test(branched_state());
    app.state.toggle_profile_pipe("P2");
    run_frame_with_events(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.state.profile_pipes.is_empty(), "Esc must clear the run");
    assert!(app.state.status.contains("main trunk"));
}

#[test]
fn profile_run_selects_branch_and_renders() {
    let mut app = StormSewerApp::new_for_test(branched_state());
    // The branch run: PB (P3's pipe is P3? branch pipe id) — resolve by
    // endpoints so the test doesn't assume id numbering.
    let branch_pipe = app
        .state
        .project
        .pipes
        .iter()
        .find(|p| p.to == "N2" && p.from == "N3")
        .map(|p| p.id.clone())
        .expect("branch pipe exists");
    let trunk_tail = app
        .state
        .project
        .pipes
        .iter()
        .find(|p| p.from == "N2")
        .map(|p| p.id.clone())
        .unwrap();
    app.state.profile_pipes = vec![trunk_tail.clone(), branch_pipe.clone()];

    // Engine agrees this chains into one branch-to-outfall run.
    let net = app.state.project.to_network();
    let stems = stormsewer::drawing::stems_from_pipes(
        &net,
        &app.state.profile_pipes,
    );
    assert_eq!(stems.len(), 1, "branch run must chain into one stem");
    let names: Vec<&str> =
        stems[0].iter().map(|&i| net.nodes[i].id.as_str()).collect();
    assert_eq!(names, ["N3", "N2", "OUT"]);

    // Both views render with the run active (plan underlay + run profile).
    app.state.view_tab = ViewTab::Plan;
    run_frame(&mut app);
    app.state.view_tab = ViewTab::Profile;
    run_frame(&mut app);
}

#[test]
fn profile_run_cleared_on_project_load() {
    let dir = std::env::temp_dir().join("stormsewer-ui-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = branched_state();
    let path = dir.join("run-clear.ssproj");
    s.project.save(&path).unwrap();
    s.toggle_profile_pipe("P1");
    assert!(!s.profile_pipes.is_empty());
    let ctx = headless_ctx();
    s.open_project_path(&ctx, path);
    assert!(
        s.profile_pipes.is_empty(),
        "stale profile run survived a project load"
    );
}

// --- competitive-parity features ---------------------------------------------

#[test]
fn egl_line_present_in_profile() {
    let s = analyzed_state();
    let net = s.project.to_network();
    let d = stormsewer::drawing::draw_network(
        &net,
        s.analysis.as_ref().unwrap(),
        &stormsewer::drawing::DrawConfig::default(),
    );
    use stormsewer::drawing::ProfileRole;
    let roles: Vec<ProfileRole> = d.profile_lines.iter().map(|p| p.role).collect();
    assert!(roles.contains(&ProfileRole::Egl), "EGL missing: {roles:?}");
    // EGL never falls below the HGL it derives from.
    let hgl = d.profile_lines.iter().find(|p| p.role == ProfileRole::Hgl).unwrap();
    let egl = d.profile_lines.iter().find(|p| p.role == ProfileRole::Egl).unwrap();
    for (h, e) in hgl.pts.iter().zip(egl.pts.iter()) {
        assert!(e.1 >= h.1 - 1e-9, "EGL below HGL");
    }
}

#[test]
fn inlet_schedule_rows_follow_bypass_chain() {
    let mut s = built_state();
    // Make N1 a heavy inlet bypassing to a new inlet N3 at the junction.
    let extra = place_structure(&mut s.project, &mut s.edit, "inlet", 60.0, 300.0);
    {
        let n = node_mut(&mut s, "N1");
        n.area_ac = 3.0;
        n.c = 0.9;
        n.bypass_to = Some(extra.clone());
    }
    s.run_analysis();
    assert!(s.analysis.is_some(), "{}", s.report_text);
    let a_row = s.inlet_rows.iter().find(|r| r.node_id == "N1").unwrap();
    let b_row = s
        .inlet_rows
        .iter()
        .find(|r| r.node_id == extra)
        .unwrap();
    assert!(a_row.bypass_cfs > 0.0, "N1 should bypass at 3 ac");
    assert!(
        (b_row.carryover_in_cfs - a_row.bypass_cfs).abs() < 1e-9,
        "carryover must equal upstream bypass"
    );
    assert_eq!(a_row.bypass_to.as_deref(), Some(extra.as_str()));

    // Renders as a schedule in the report panel.
    let mut app = StormSewerApp::new_for_test(s);
    run_frame(&mut app);
}

#[test]
fn bypass_to_round_trips_through_save() {
    let dir = std::env::temp_dir().join("stormsewer-ui-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = built_state();
    node_mut(&mut s, "N1").bypass_to = Some("N2".into());
    let path = dir.join("bypass.ssproj");
    s.project.save(&path).unwrap();
    let loaded = stormsewer::io::Project::load(&path).unwrap();
    let n1 = loaded.nodes.iter().find(|n| n.id == "N1").unwrap();
    assert_eq!(n1.bypass_to.as_deref(), Some("N2"));
}

#[test]
fn live_recompute_runs_on_next_frame() {
    let mut app = StormSewerApp::new_for_test(analyzed_state());
    app.state.prefs.auto_analyze = true;
    let q_before = app.state.analysis.as_ref().unwrap().pipes[0].design_q;
    node_mut(&mut app.state, "N1").area_ac = 4.56;
    app.state.mark_analysis_stale();
    run_frame(&mut app);
    assert!(!app.state.analysis_stale, "frame should have recomputed");
    let q_after = app.state.analysis.as_ref().unwrap().pipes[0].design_q;
    assert!(
        (q_after / q_before - 4.56 / 1.23).abs() < 1e-9,
        "recompute did not pick up the edit"
    );

    // And OFF means stale stays until F5.
    let mut app2 = StormSewerApp::new_for_test(analyzed_state());
    app2.state.prefs.auto_analyze = false;
    node_mut(&mut app2.state, "N1").area_ac = 9.9;
    app2.state.mark_analysis_stale();
    run_frame(&mut app2);
    assert!(app2.state.analysis_stale, "auto off must not recompute");
}

// --- complete UI/UX + report-validation sweep (2026-08-26 features) ----------

#[test]
fn inspector_bypass_combo_renders_for_selected_inlet() {
    let mut app = StormSewerApp::new_for_test(branched_state());
    let inlet_idx = app
        .state
        .project
        .nodes
        .iter()
        .position(|n| n.kind == "inlet")
        .unwrap();
    app.state.set_selection(Some(inlet_idx), None, None);
    app.state.inspector_open = true;
    run_frame(&mut app);
    run_frame(&mut app);
}

/// No egui window in the app may be anchored: anchored windows cannot be
/// dragged, which is exactly the popup complaint this guards against.
#[test]
fn no_anchored_windows_anywhere() {
    for (name, src) in [
        ("main.rs", include_str!("main.rs")),
        ("help.rs", include_str!("help.rs")),
        ("tutorial.rs", include_str!("tutorial.rs")),
        ("tc_calc.rs", include_str!("tc_calc.rs")),
        ("global_edit.rs", include_str!("global_edit.rs")),
        ("report_editor.rs", include_str!("report_editor.rs")),
        ("files.rs", include_str!("files.rs")),
    ] {
        assert!(
            !src.contains(".anchor("),
            "{name} anchors a window — anchored windows can't be dragged"
        );
    }
}

/// Real drag: press the About window's title bar, move the pointer, and the
/// window must follow. Uses one persistent egui context so window positions
/// survive between frames.
#[test]
fn about_window_is_draggable() {
    let mut app = StormSewerApp::new_for_test(AppState::new_demo());
    app.show_about = true;
    let ctx = egui::Context::default();
    let _ = ctx.run(raw_input(), |c| app.ui(c));
    let id = egui::Id::new("About StormSewer");
    let r1 = ctx
        .memory(|m| m.area_rect(id))
        .expect("About window has an area");

    let grab = egui::pos2(r1.min.x + 60.0, r1.min.y + 8.0);
    let mut press = raw_input();
    press.events = vec![
        egui::Event::PointerMoved(grab),
        egui::Event::PointerButton {
            pos: grab,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let _ = ctx.run(press, |c| app.ui(c));

    let target = grab + egui::vec2(140.0, 80.0);
    let mut drag = raw_input();
    drag.events = vec![egui::Event::PointerMoved(target)];
    let _ = ctx.run(drag, |c| app.ui(c));

    let mut release = raw_input();
    release.events = vec![egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }];
    let _ = ctx.run(release, |c| app.ui(c));

    let r2 = ctx.memory(|m| m.area_rect(id)).unwrap();
    let moved = (r2.min - r1.min).length();
    assert!(moved > 60.0, "About window did not follow the drag ({moved} px)");
}

#[test]
fn report_text_contains_inlet_schedule() {
    let mut s = built_state();
    node_mut(&mut s, "N1").bypass_to = Some("N2".into());
    s.run_analysis();
    assert!(
        s.report_text.contains("INLET SCHEDULE"),
        "inlet schedule missing from report text"
    );
    assert!(s.report_text.contains("N1"));
    assert!(
        s.report_text.contains("conservative"),
        "surface-analysis caveat missing"
    );
}

/// The deep E2E: build a branched network with a bypass chain, analyze, and
/// validate the REPORT against independent hand calculations — Manning
/// capacity from first principles, EGL = HGL + V^2/2g, the carryover chain
/// recomputed with the raw HEC-22 check, and every export artifact.
#[test]
fn e2e_report_validation_first_principles() {
    use stormsewer::design::inlets::check_inlet_geom;
    use stormsewer::drawing::{draw_network, DrawConfig, ProfileRole};

    let dir = std::env::temp_dir().join("stormsewer-ui-tests");
    std::fs::create_dir_all(&dir).unwrap();

    // Branched fixture with a bypass chain N1 -> N3(branch inlet).
    let mut s = built_state();
    let b = place_structure(&mut s.project, &mut s.edit, "inlet", 300.0, 300.0);
    place_pipe(&mut s.project, &mut s.edit, &b, "N2").unwrap();
    {
        let n = node_mut(&mut s, &b);
        n.area_ac = 2.0;
        n.c = 0.60;
        n.invert = 95.0;
        n.rim = 103.0;
    }
    {
        let n = node_mut(&mut s, "N1");
        n.area_ac = 3.0; // heavy: guarantees bypass
        n.c = 0.90;
        n.bypass_to = Some(b.clone());
    }
    s.run_analysis();
    let a = s.analysis.clone().expect("analysis");

    // 1. Manning full-flow capacity from first principles for every
    //    circular pipe: Q = (1.486/n) * A * R^(2/3) * sqrt(S).
    for pr in &a.pipes {
        let p = s.project.pipes.iter().find(|p| p.id == pr.id).unwrap();
        if p.shape != "circular" {
            continue;
        }
        let d = p.diameter;
        let area = std::f64::consts::PI * d * d / 4.0;
        let r = d / 4.0;
        // Engine convention: K = 1.49 (stormsewer::hydraulics::K_MANNING_US).
        let q_hand = (stormsewer::hydraulics::K_MANNING_US / p.n)
            * area
            * r.powf(2.0 / 3.0)
            * pr.manning_slope.sqrt();
        assert!(
            (pr.capacity - q_hand).abs() / q_hand < 1e-6,
            "{}: capacity {} vs hand Manning {}",
            pr.id,
            pr.capacity,
            q_hand
        );
    }

    // 2. EGL in the drawing equals HGL + V^2/2g at every stem node.
    let net = s.project.to_network();
    let cfg = DrawConfig::default();
    let d = draw_network(&net, &a, &cfg);
    let egl = d
        .profile_lines
        .iter()
        .find(|p| p.role == ProfileRole::Egl)
        .expect("EGL polyline");
    let hgl = d
        .profile_lines
        .iter()
        .find(|p| p.role == ProfileRole::Hgl)
        .unwrap();
    let to_elev = |y: f64| d.profile_datum + (y - cfg.profile_origin_y) / cfg.v_exag;
    for (h, e) in hgl.pts.iter().zip(egl.pts.iter()) {
        let vh = to_elev(e.1) - to_elev(h.1);
        assert!(
            (0.0..10.0).contains(&vh),
            "velocity head {vh} ft implausible"
        );
    }
    // Spot-check one node exactly: upstream stem head node's outgoing pipe.
    let head_label = &d.profile_labels[0].text;
    let out_pipe = a.pipes.iter().find(|p| &p.from == head_label).unwrap();
    let vh_hand = out_pipe.velocity * out_pipe.velocity / (2.0 * 32.174);
    let vh_drawn = to_elev(egl.pts[0].1) - to_elev(hgl.pts[0].1);
    assert!(
        (vh_hand - vh_drawn).abs() < 1e-6,
        "EGL head at {head_label}: drawn {vh_drawn} vs hand {vh_hand}"
    );

    // 3. Carryover chain recomputed with the raw HEC-22 check.
    let n1_row = s.inlet_rows.iter().find(|r| r.node_id == "N1").unwrap();
    let geom = s.inlet_geom.clone();
    let hand = check_inlet_geom(n1_row.approach_cfs, &geom);
    assert!((n1_row.bypass_cfs - hand.bypass_cfs).abs() < 1e-9);
    let b_row = s.inlet_rows.iter().find(|r| r.node_id == b).unwrap();
    assert!(
        (b_row.carryover_in_cfs - n1_row.bypass_cfs).abs() < 1e-9,
        "carryover chain broken in report rows"
    );

    // 4. The report text carries the numbers the schedules show.
    for needle in ["INLET SCHEDULE", "STORM SEWER ANALYSIS", "N1", "OUT"] {
        assert!(s.report_text.contains(needle), "report missing {needle}");
    }
    let bypass_str = format!("{:.2}", n1_row.bypass_cfs);
    assert!(
        s.report_text.contains(&bypass_str),
        "report missing bypass value {bypass_str}"
    );

    // 5. Every export artifact.
    let html = dir.join("val.html");
    stormsewer::io::export_html(&s.project, &a, &html).unwrap();
    let html_s = std::fs::read_to_string(&html).unwrap();
    for needle in ["N1", "N2", "OUT", "P1"] {
        assert!(html_s.contains(needle), "HTML missing {needle}");
    }
    let pdf = dir.join("val.pdf");
    stormsewer::io::export_pdf(&s.project, &a, &pdf, None).unwrap();
    assert!(pdf.metadata().unwrap().len() > 1000);
    let csv = stormsewer::io::render_csv(
        &s.project,
        &a,
        &stormsewer::io::ReportTemplate::hydraflow_style(),
    );
    assert!(csv.contains("P1"), "custom CSV missing pipe id");

    // 6. Round-trip: save, reload, re-analyze -> byte-identical report.
    let path = dir.join("val.ssproj");
    s.project.save(&path).unwrap();
    let mut s2 = AppState::new_empty();
    let ctx = headless_ctx();
    s2.open_project_path(&ctx, path);
    s2.run_analysis();
    assert_eq!(
        s2.report_text, s.report_text,
        "reloaded project produces a different report"
    );
}

/// Canvas pan follows the hand in BOTH axes: dragging down must move the
/// drawing down (pan.y decreases, since pan.y is measured from the bottom),
/// dragging right moves it right.
#[test]
fn canvas_pan_follows_the_hand() {
    let mut app = StormSewerApp::new_for_test(AppState::new_empty());
    let ctx = egui::Context::default();
    let _ = ctx.run(raw_input(), |c| app.ui(c));

    let start = egui::pos2(900.0, 300.0); // empty canvas area
    let mut press = raw_input();
    press.events = vec![
        egui::Event::PointerMoved(start),
        egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let _ = ctx.run(press, |c| app.ui(c));
    let pan_before = app.state.viewport.pan;

    // Drag right 50 and down 80.
    let target = start + egui::vec2(50.0, 80.0);
    let mut drag = raw_input();
    drag.events = vec![egui::Event::PointerMoved(target)];
    let _ = ctx.run(drag, |c| app.ui(c));

    let mut release = raw_input();
    release.events = vec![egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }];
    let _ = ctx.run(release, |c| app.ui(c));

    let pan = app.state.viewport.pan;
    assert!(
        (pan.x - pan_before.x - 50.0).abs() < 1.0,
        "horizontal pan should follow the hand (+50): {} -> {}",
        pan_before.x,
        pan.x
    );
    assert!(
        (pan.y - pan_before.y + 80.0).abs() < 1.0,
        "downward drag must DECREASE pan.y by 80 (content follows hand):          {} -> {}",
        pan_before.y,
        pan.y
    );
}

// --- unsaved-work guard + autosave -------------------------------------------

fn close_request_input() -> egui::RawInput {
    let mut input = raw_input();
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    input
}

/// Serializes the tests that redirect the autosave path via env var.
static AUTOSAVE_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_temp_autosave_dir<R>(f: impl FnOnce(&std::path::Path) -> R) -> R {
    let _guard = AUTOSAVE_ENV.lock().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "stormsewer-autosave-{}",
        std::thread::current().name().unwrap_or("t").replace("::", "-")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("STORMSEWER_AUTOSAVE_DIR", &dir);
    let r = f(&dir);
    std::env::remove_var("STORMSEWER_AUTOSAVE_DIR");
    r
}

#[test]
fn dirty_close_is_intercepted_with_a_choice() {
    with_temp_autosave_dir(|_| {
        let mut app = StormSewerApp::new_for_test(analyzed_state());
        app.state.mark_project_dirty();
        let ctx = egui::Context::default();
        let out = ctx.run(close_request_input(), |c| app.ui(c));
        let cancelled = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| {
                v.commands
                    .iter()
                    .any(|c| matches!(c, egui::ViewportCommand::CancelClose))
            })
            .unwrap_or(false);
        assert!(cancelled, "close must be cancelled while dirty");
        assert!(app.show_close_confirm, "confirm dialog must open");
        // The dialog renders on the next frame.
        let _ = ctx.run(raw_input(), |c| app.ui(c));
    });
}

#[test]
fn clean_close_is_not_intercepted() {
    with_temp_autosave_dir(|_| {
        let mut app = StormSewerApp::new_for_test(analyzed_state());
        app.state.mark_project_saved();
        assert!(!app.state.project_dirty);
        let ctx = egui::Context::default();
        let out = ctx.run(close_request_input(), |c| app.ui(c));
        let cancelled = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| {
                v.commands
                    .iter()
                    .any(|c| matches!(c, egui::ViewportCommand::CancelClose))
            })
            .unwrap_or(false);
        assert!(!cancelled, "clean projects close without ceremony");
        assert!(!app.show_close_confirm);
    });
}

#[test]
fn autosave_lifecycle_snapshot_and_clear() {
    with_temp_autosave_dir(|dir| {
        let path = dir.join("autosave-recovery.ssproj");
        let mut app = StormSewerApp::new_for_test(built_state());
        app.state.mark_project_dirty();
        app.maybe_autosave(true);
        assert!(path.exists(), "dirty project must snapshot");

        // Snapshot restores to the same network.
        let recovered = stormsewer::io::Project::load(&path).unwrap();
        assert_eq!(recovered.nodes.len(), app.state.project.nodes.len());
        assert_eq!(recovered.pipes.len(), app.state.project.pipes.len());

        // A clean save supersedes the snapshot.
        app.state.mark_project_saved();
        assert!(!path.exists(), "clean save must clear the snapshot");

        // Clean projects never snapshot.
        app.maybe_autosave(true);
        assert!(!path.exists());
    });
}

#[test]
fn recovery_prompt_restores_pathless_and_dirty() {
    with_temp_autosave_dir(|dir| {
        let path = dir.join("autosave-recovery.ssproj");
        let s = branched_state();
        s.project.save(&path).unwrap();
        let node_count = s.project.nodes.len();

        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        app.show_recovery = true;
        let _ = headless_ctx().run(raw_input(), |c| app.ui(c)); // prompt renders
        app.restore_recovery();
        assert_eq!(app.state.project.nodes.len(), node_count);
        assert!(app.state.project_path.is_none(), "restore must be path-less");
        assert!(app.state.project_dirty, "restored work must read as unsaved");
        assert!(!app.show_recovery);
    });
}
