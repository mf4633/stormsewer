// SPDX-License-Identifier: GPL-3.0-or-later

//! The SWMM editor workspace's chrome: menu bar (File, Edit, View, Project,
//! Run, Results, Tools), toolbar, keyboard shortcuts, dialogs, and the
//! side panels. The storm-sewer design view stays one switch away.
//!
//! Project and Results items that belong to other parts of the editor
//! (the property sheet, the attribute grids, result tables) are here as
//! hooks that say so, so the menu shape is settled before they arrive.

use eframe::egui::{self, Button, Key, Modifiers, RichText, Ui};
use stormsewer_swmm::doc::Severity;

use crate::help::{open_help, HelpTopic};
use crate::state::AppState;
use crate::swmm_canvas::{delete_active_vertex, finish_label, finish_polygon};
use crate::swmm_dialogs;
use crate::swmm_doc::{describe, LeftTab, PendingAction};
use crate::swmm_panel::SwmmSubView;
use crate::swmm_tools::SwmmTool;
use crate::theme::palette;

/// Open the SWMM workspace (with a blank model if none is open).
pub fn enter_workspace(state: &mut AppState) {
    state.swmm_doc.active = true;
    state.swmm_doc.ensure_recent();
    state.swmm_doc.layers = state.prefs.swmm_layers.clone();
    state.swmm.ensure_discovered();
    if !state.swmm_doc.loaded {
        state.swmm_doc.new_model();
    }
    state.swmm.sub_view = SwmmSubView::Map;
    state.swmm.pending_map_fit = true;
    state.status = "SWMM model editor".into();
}

/// Back to the storm-sewer design view; the model stays open.
pub fn leave_workspace(state: &mut AppState) {
    state.swmm_doc.active = false;
    state.status = "Storm sewer design".into();
}

pub fn save_model(state: &mut AppState) {
    match state.swmm_doc.save_or_pick() {
        Ok(true) => state.status = format!("Saved {}", state.swmm_doc.file_name()),
        Ok(false) => state.status = "Save cancelled".into(),
        Err(e) => state.status = format!("Save failed: {e}"),
    }
}

pub fn save_model_as(state: &mut AppState) {
    match state.swmm_doc.pick_save_as() {
        Some(Ok(())) => state.status = format!("Saved {}", state.swmm_doc.file_name()),
        Some(Err(e)) => state.status = format!("Save failed: {e}"),
        None => {}
    }
}

/// Run the open model: refused, with the list, when validation finds
/// errors; warnings are listed once (per document state) before the run
/// goes ahead; otherwise the current text goes to the chosen engine, from
/// a scratch copy when its path or state calls for one.
pub fn run_model(state: &mut AppState) {
    state.swmm.ensure_discovered();
    state.swmm_doc.qa.run_anyway = false;
    state.swmm_doc.refresh();
    if state.swmm_doc.error_count() == 0 && !crate::swmm_qa::clear_to_run(state) {
        return;
    }
    match state.swmm_doc.run_path() {
        Err(findings) => {
            state.status = format!("Run refused: {} error(s) to fix first", findings.len());
            state.swmm_doc.run_refused = Some(findings);
        }
        Ok(path) => {
            state.swmm.model = Some(path);
            if state.swmm.engine().is_none() {
                state.status = "No SWMM engine: choose one in the Run menu".into();
                return;
            }
            state.swmm.start_run();
            state.status = state.swmm.status_line();
        }
    }
}

fn undo(state: &mut AppState) {
    state.status = match state.swmm_doc.undo() {
        Some(label) => format!("Undid {label}"),
        None => "Nothing to undo".into(),
    };
}

fn redo(state: &mut AppState) {
    state.status = match state.swmm_doc.redo() {
        Some(label) => format!("Redid {label}"),
        None => "Nothing to redo".into(),
    };
}

fn delete(state: &mut AppState) {
    if delete_active_vertex(&mut state.swmm_doc) {
        state.status = "Deleted the vertex".into();
        return;
    }
    match state.swmm_doc.delete_selection() {
        Some(n) => state.status = format!("Deleted {n} object(s)"),
        None if state.swmm_doc.delete_confirm.is_some() => {}
        None => state.status = "Nothing selected".into(),
    }
}

fn copy(state: &mut AppState) {
    let n = state.swmm_doc.copy();
    state.status = format!("Copied {n} object(s)");
}

fn cut(state: &mut AppState) {
    let n = state.swmm_doc.cut();
    state.status = format!("Cut {n} object(s)");
}

fn paste(state: &mut AppState) {
    let n = state.swmm_doc.paste();
    state.status = if n == 0 {
        "Nothing to paste".into()
    } else {
        format!(
            "Pasted {n} object(s) as {}",
            state.swmm_doc.selection_summary()
        )
    };
}

// --- menus --------------------------------------------------------------------

pub fn file_menu(ui: &mut Ui, ctx: &egui::Context, state: &mut AppState) {
    if ui.button("New SWMM Model").clicked() {
        state.swmm_doc.request(PendingAction::New);
        state.swmm.pending_map_fit = true;
        ui.close_menu();
    }
    if ui.button("Open .inp…").clicked() {
        state.swmm_doc.request(PendingAction::Open);
        state.swmm.pending_map_fit = true;
        ui.close_menu();
    }
    state.swmm_doc.ensure_recent();
    if !state.swmm_doc.recent.paths.is_empty() {
        ui.menu_button("Recent Models", |ui| {
            let recent: Vec<_> = state.swmm_doc.recent.paths.clone();
            for path in recent {
                let label = state.swmm_doc.recent.label(&path);
                if ui.button(label).clicked() {
                    state.swmm_doc.request(PendingAction::OpenPath(path));
                    state.swmm.pending_map_fit = true;
                    ui.close_menu();
                }
            }
        });
    }
    if ui
        .add_enabled(state.swmm_doc.loaded, Button::new("Save"))
        .clicked()
    {
        save_model(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(state.swmm_doc.loaded, Button::new("Save As…"))
        .clicked()
    {
        save_model_as(state);
        ui.close_menu();
    }
    ui.separator();
    crate::swmm_import::import_menu_items(ui, state);
    crate::swmm_import::export_menu_items(ui, state);
    ui.separator();
    crate::swmm_gis::file_menu_items(ui, state);
    if ui.button("Load PNG Background…").clicked() {
        state.pick_background(ctx);
        ui.close_menu();
    }
    if ui.button("DXF Underlay…").clicked() {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf", "DXF"])
            .pick_file()
        {
            state.set_background_dxf(path);
        }
        ui.close_menu();
    }
    ui.separator();
    if ui.button("Storm Sewer Design Workspace").clicked() {
        leave_workspace(state);
        ui.close_menu();
    }
}

pub fn edit_menu(ui: &mut Ui, state: &mut AppState) {
    let undo_text = match state.swmm_doc.undo_label() {
        Some(l) => format!("Undo {l}"),
        None => "Undo".into(),
    };
    let redo_text = match state.swmm_doc.redo_label() {
        Some(l) => format!("Redo {l}"),
        None => "Redo".into(),
    };
    if ui
        .add_enabled(
            state.swmm_doc.can_undo(),
            Button::new(undo_text).shortcut_text("Ctrl+Z"),
        )
        .clicked()
    {
        undo(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(
            state.swmm_doc.can_redo(),
            Button::new(redo_text).shortcut_text("Ctrl+Y"),
        )
        .clicked()
    {
        redo(state);
        ui.close_menu();
    }
    ui.separator();
    let has_sel = !state.swmm_doc.selection.is_empty();
    if ui
        .add_enabled(has_sel, Button::new("Cut").shortcut_text("Ctrl+X"))
        .clicked()
    {
        cut(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(has_sel, Button::new("Copy").shortcut_text("Ctrl+C"))
        .clicked()
    {
        copy(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(
            !state.swmm_doc.clipboard.is_empty(),
            Button::new("Paste").shortcut_text("Ctrl+V"),
        )
        .clicked()
    {
        paste(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(has_sel, Button::new("Delete").shortcut_text("Del"))
        .clicked()
    {
        delete(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(
            state.swmm_doc.loaded,
            Button::new("Select All").shortcut_text("Ctrl+A"),
        )
        .clicked()
    {
        state.swmm_doc.select_all();
        state.status = format!("{} selected", state.swmm_doc.selection.len());
        ui.close_menu();
    }
}

pub fn view_menu(ui: &mut Ui, state: &mut AppState, canvas_rect: egui::Rect) {
    if ui.button("Zoom In").clicked() {
        crate::swmm_canvas::zoom_at(
            &mut state.swmm.map_viewport,
            canvas_rect,
            canvas_rect.center(),
            1.5,
        );
        ui.close_menu();
    }
    if ui.button("Zoom Out").clicked() {
        crate::swmm_canvas::zoom_at(
            &mut state.swmm.map_viewport,
            canvas_rect,
            canvas_rect.center(),
            1.0 / 1.5,
        );
        ui.close_menu();
    }
    if ui.button("Zoom Extents").clicked() {
        state.swmm.pending_map_fit = true;
        ui.close_menu();
    }
    if ui.button("Zoom Window").clicked() {
        state.swmm_doc.set_tool(SwmmTool::ZoomWindow);
        ui.close_menu();
    }
    if ui
        .add_enabled(
            !state.swmm_doc.selection.is_empty(),
            Button::new("Zoom to Selection"),
        )
        .on_hover_text("F zooms to the selection, or to everything when nothing is selected")
        .clicked()
    {
        state.swmm_doc.canvas.pending_zoom_selection = true;
        ui.close_menu();
    }
    if ui.button("Pan").clicked() {
        state.swmm_doc.set_tool(SwmmTool::Pan);
        ui.close_menu();
    }
    ui.separator();
    crate::swmm_backdrop::view_menu_items(ui, state);
    ui.separator();
    ui.checkbox(&mut state.swmm_doc.show_labels, "Object Labels");
    ui.checkbox(&mut state.swmm_doc.show_arrows, "Flow Arrows");
    ui.checkbox(&mut state.swmm_doc.show_grid, "Grid");
    ui.checkbox(&mut state.swmm_doc.snap_objects, "Snap to Objects");
    ui.checkbox(&mut state.swmm_doc.snap_grid, "Snap to Grid");
    // The spacing lived beside the toolbar's snap checkbox, which the row
    // could not hold. Without it here the grid is stuck at its default.
    if state.swmm_doc.snap_grid {
        ui.horizontal(|ui| {
            ui.label("Grid spacing");
            ui.add(
                egui::DragValue::new(&mut state.swmm_doc.grid_spacing)
                    .speed(1.0)
                    .range(0.1..=10000.0),
            );
        });
    }
    ui.separator();
    if ui.button("Project Browser").clicked() {
        state.swmm_doc.left_tab = LeftTab::Browser;
        ui.close_menu();
    }
    if ui.button("Map Layers").clicked() {
        state.swmm_doc.left_tab = LeftTab::Layers;
        ui.close_menu();
    }
    if ui
        .add_enabled(state.swmm_doc.loaded, Button::new("Attribute Table…"))
        .clicked()
    {
        let section = state
            .swmm_doc
            .selected_rows()
            .first()
            .map(|(s, _)| s.to_string())
            .unwrap_or_else(|| state.swmm_doc.grid.section.clone());
        crate::swmm_grids::open(&mut state.swmm_doc, &section);
        ui.close_menu();
    }
    ui.checkbox(&mut state.swmm_doc.show_properties, "Properties");
    ui.separator();
    let nodes = state.swmm_doc.selected_nodes();
    if ui
        .add_enabled(nodes.len() == 2, Button::new("Profile from Selection"))
        .on_hover_text("Select exactly two nodes")
        .clicked()
    {
        profile_from_selection(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(state.swmm_doc.loaded, Button::new("Pick Profile Path…"))
        .on_hover_text("Click the start node, then the end node; Esc cancels")
        .clicked()
    {
        state.swmm_doc.profile_pick = Some(None);
        state.swmm.sub_view = SwmmSubView::Map;
        state.status = "Profile: click the start node".into();
        ui.close_menu();
    }
    ui.separator();
    for (view, label) in [
        (SwmmSubView::Map, "Map"),
        (SwmmSubView::Chart, "Chart"),
        (SwmmSubView::Profile, "Profile"),
        (SwmmSubView::Results, "Results"),
    ] {
        if ui
            .selectable_label(state.swmm.sub_view == view, label)
            .clicked()
        {
            state.swmm.sub_view = view;
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button("Storm Sewer Design Workspace").clicked() {
        leave_workspace(state);
        ui.close_menu();
    }
}

/// View → Profile from Selection: the two selected nodes become the
/// profile path and the profile view opens.
pub fn profile_from_selection(state: &mut AppState) -> bool {
    let nodes = state.swmm_doc.selected_nodes();
    let [a, b] = nodes.as_slice() else {
        state.status = "Select exactly two nodes for a profile".into();
        return false;
    };
    state.swmm.profile.set_path(a.clone(), b.clone());
    state.swmm.sub_view = SwmmSubView::Profile;
    state.status = format!("Profile {a} to {b}");
    true
}

pub fn project_menu(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.swmm_doc.loaded;
    let ed = &mut state.swmm_doc;
    if ui.add_enabled(loaded, Button::new("Title/Notes…")).clicked() {
        swmm_dialogs::open_title(ed);
        ui.close_menu();
    }
    if ui.add_enabled(loaded, Button::new("Options…")).clicked() {
        swmm_dialogs::open_options(ed);
        ui.close_menu();
    }
    ui.separator();
    if ui.add_enabled(loaded, Button::new("Rain Gages…")).clicked() {
        swmm_dialogs::open_gages(ed, None);
        ui.close_menu();
    }
    if ui.add_enabled(loaded, Button::new("Curves…")).clicked() {
        swmm_dialogs::open_curves(ed, None);
        ui.close_menu();
    }
    ui.add_enabled_ui(loaded, |ui| {
        ui.menu_button("Time Series", |ui| {
            if ui.button("Edit…").clicked() {
                swmm_dialogs::open_series(ed, None);
                ui.close_menu();
            }
            if ui
                .button("Import…")
                .on_hover_text("CSV/TSV date-time-value columns, or a NOAA GHCN-Daily csv")
                .clicked()
            {
                crate::swmm_rain_import::open_import(ed);
                ui.close_menu();
            }
            if ui
                .button("Export to File…")
                .on_hover_text("Write a series as a SWMM external time-series file")
                .clicked()
            {
                crate::swmm_rain_import::open_export(ed, None);
                ui.close_menu();
            }
        });
    });
    if ui.add_enabled(loaded, Button::new("Patterns…")).clicked() {
        swmm_dialogs::open_patterns(ed, None);
        ui.close_menu();
    }
    ui.separator();
    if ui
        .add_enabled(loaded, Button::new("Design Storm…"))
        .on_hover_text("NRCS Type I/IA/II/III, NOAA Atlas 14 regional, alternating block, Chicago, uniform")
        .clicked()
    {
        crate::swmm_storm::open(ed);
        ui.close_menu();
    }
    if ui
        .add_enabled(loaded, Button::new("Compute Conduit Lengths…"))
        .on_hover_text("Geometric length from the map against the stored Length; apply the ticked ones as one step")
        .clicked()
    {
        crate::swmm_lengths::open(ed);
        ui.close_menu();
    }
    ui.separator();
    if ui.add_enabled(loaded, Button::new("Controls…")).clicked() {
        swmm_dialogs::open_controls(ed);
        ui.close_menu();
    }
    ui.separator();
    if ui.add_enabled(loaded, Button::new("Pollutants…")).clicked() {
        swmm_dialogs::open_pollutants(ed);
        ui.close_menu();
    }
    if ui.add_enabled(loaded, Button::new("Land Uses…")).clicked() {
        swmm_dialogs::open_landuses(ed);
        ui.close_menu();
    }
    ui.separator();
    crate::swmm_lid::project_menu_items(ui, state);
    ui.separator();
    if ui.button("Validate Model").clicked() {
        state.swmm_doc.refresh();
        state.swmm_doc.show_findings = true;
        state.status = format!(
            "Validation: {} error(s), {} warning(s)",
            state.swmm_doc.error_count(),
            state.swmm_doc.warning_count()
        );
        ui.close_menu();
    }
}

pub fn run_menu(ui: &mut Ui, state: &mut AppState) {
    state.swmm.ensure_discovered();
    ui.label(RichText::new("Engine").strong());
    let engines: Vec<(String, String)> = state
        .swmm
        .registry
        .engines()
        .iter()
        .map(|e| (e.id.clone(), e.label()))
        .collect();
    if engines.is_empty() {
        ui.label("No SWMM engine found");
    }
    for (id, label) in engines {
        let selected = state.swmm.engine_id.as_deref() == Some(id.as_str());
        if ui.selectable_label(selected, label).clicked() {
            state.swmm.engine_id = Some(id);
        }
    }
    crate::swmm_live::run_menu_items(ui, state);
    if ui.button("Find Engines").clicked() {
        state.swmm.rescan();
        ui.close_menu();
    }
    ui.separator();
    if ui
        .add_enabled(state.swmm_doc.loaded, Button::new("Check Model…"))
        .on_hover_text("Undefined references, orphans, offsets, outfalls, options — before the engine sees them")
        .clicked()
    {
        crate::swmm_qa::check_model(state);
        ui.close_menu();
    }
    crate::swmm_run_panel::help_menu_item(ui, state);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Autosave every");
        if ui
            .add(
                egui::DragValue::new(&mut state.prefs.swmm_autosave_minutes)
                    .range(0..=60)
                    .suffix(" min"),
            )
            .on_hover_text("Snapshot of an unsaved model beside its file (0 = off)")
            .changed()
        {
            state.prefs.save();
        }
    });
    ui.separator();
    let can_run = state.swmm_doc.loaded && !state.swmm.is_running();
    if ui
        .add_enabled(can_run, Button::new("Run").shortcut_text("F5"))
        .clicked()
    {
        run_model(state);
        ui.close_menu();
    }
    if ui
        .add_enabled(state.swmm.is_running(), Button::new("Stop"))
        .clicked()
    {
        state.status = state.swmm.stop();
        ui.close_menu();
    }
}

pub fn results_menu(ui: &mut Ui, state: &mut AppState) {
    let has = state.swmm.results.is_some();
    if ui.add_enabled(has, Button::new("Map (Peaks)")).clicked() {
        state.swmm.show_peaks();
        state.swmm.sub_view = SwmmSubView::Map;
        ui.close_menu();
    }
    if ui.add_enabled(has, Button::new("Chart")).clicked() {
        state.swmm.sub_view = SwmmSubView::Chart;
        ui.close_menu();
    }
    if ui
        .add_enabled(has, Button::new("Report Summary…"))
        .clicked()
    {
        state.status = "Report summary tables arrive with the results stream".into();
        ui.close_menu();
    }
    ui.separator();
    crate::swmm_run_panel::results_menu_item(ui, state);
    crate::swmm_report::results_menu_item(ui, state);
    crate::swmm_compare::results_menu_items(ui, state);
}

pub fn tools_menu(ui: &mut Ui, state: &mut AppState) {
    if ui
        .add_enabled(state.swmm.last_run.is_some(), Button::new("ALR Checks"))
        .clicked()
    {
        state.swmm.run_alr();
        state.status = state.swmm.status_line();
        ui.close_menu();
    }
    crate::swmm_scenarios::tools_menu_items(ui, state);
    crate::swmm_calib::tools_menu_items(ui, state);
    ui.separator();
    if ui.button("Python Terminal…").clicked() {
        state.python_term.open = true;
        ui.close_menu();
    }
    if ui.button("Tc Calculator…").clicked() {
        state.open_tc_calculator();
        ui.close_menu();
    }
    ui.separator();
    crate::swmm_design::tools_menu_items(ui, state);
    ui.separator();
    if ui.button("Storm Sewer Design Workspace").clicked() {
        leave_workspace(state);
        ui.close_menu();
    }
}

/// The workspace switch, right-aligned into the menu bar.
///
/// It lived in the toolbar until v0.10.0, where the row could not fit it and
/// dropped it entirely — leaving the toolbar with no route back to the design
/// workspace. The menu bar has room to spare, and this is navigation used once
/// a session, not a drawing tool used constantly.
pub fn workspace_switch(ui: &mut Ui, state: &mut AppState) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .selectable_label(false, "Storm Sewer")
            .on_hover_text("Switch to the storm sewer design workspace")
            .clicked()
        {
            leave_workspace(state);
        }
        let _ = ui.selectable_label(true, "SWMM");
    });
}

/// The whole SWMM menu bar except Help, which the app draws itself.
pub fn draw_menus(ui: &mut Ui, ctx: &egui::Context, state: &mut AppState, canvas_rect: egui::Rect) {
    ui.menu_button("File", |ui| file_menu(ui, ctx, state));
    ui.menu_button("Edit", |ui| edit_menu(ui, state));
    ui.menu_button("View", |ui| view_menu(ui, state, canvas_rect));
    ui.menu_button("Project", |ui| project_menu(ui, state));
    ui.menu_button("Run", |ui| run_menu(ui, state));
    ui.menu_button("Results", |ui| results_menu(ui, state));
    ui.menu_button("2D", |ui| crate::swmm_twod::menu(ui, state));
    ui.menu_button("Tools", |ui| tools_menu(ui, state));
}

// --- toolbar ------------------------------------------------------------------

pub fn draw_toolbar(ui: &mut Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    ui.horizontal_centered(|ui| {
        for tool in SwmmTool::all() {
            let active = state.swmm_doc.edit.tool == tool;
            let resp = ui
                .selectable_label(active, tool.short())
                .on_hover_text(format!("{} — press {}", tool.label(), tool.shortcut()));
            if resp.clicked() {
                state.swmm_doc.set_tool(tool);
            }
        }
        ui.separator();
        let run =
            Button::new(RichText::new("Run").color(egui::Color32::WHITE)).fill(palette::ACCENT);
        if ui
            .add_enabled(state.swmm_doc.loaded && !state.swmm.is_running(), run)
            .on_hover_text("Run the model with the chosen engine (F5)")
            .clicked()
        {
            run_model(state);
        }
        if ui
            .button("Extents")
            .on_hover_text("Zoom to the whole model (F)")
            .clicked()
        {
            state.swmm.pending_map_fit = true;
        }
        // The snap toggles and the workspace switch used to live here too, and
        // the row could not hold them: 17 labelled tools (~1148px) plus
        // Run/Extents (~140px) leaves ~112px of a 1400px window, against
        // ~405px of snap controls and switch. `horizontal_centered` neither
        // wraps nor clips, so the excess simply overlapped -- "Object snap"
        // and "Storm Sewer" were never painted and "SWMM" was clipped by the
        // window edge. Three attempts to rearrange the overflow failed; the
        // row only fits once things leave it. The snap toggles moved to the
        // View menu and the switch to the menu bar, which had ~840px spare.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.swmm_doc.dirty() {
                ui.label(
                    RichText::new("● Unsaved")
                        .color(palette::accent_text(dark))
                        .small(),
                );
            }
            if state.swmm.is_running() {
                ui.spinner();
            }
        });
    });
}

// --- shortcuts ------------------------------------------------------------------

pub fn handle_shortcuts(ctx: &egui::Context, state: &mut AppState) {
    // Escape in a text field: egui has already dropped the focus by now, so
    // the field's own revert must not become "clear the selection".
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    let typing = ctx.wants_keyboard_input() || (escape && state.swmm_doc.had_focus);
    let ctrl = Modifiers::CTRL;
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    let mut actions: Vec<fn(&mut AppState)> = Vec::new();
    let mut tool: Option<SwmmTool> = None;
    ctx.input_mut(|i| {
        if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl_shift, Key::Z)) {
            actions.push(redo);
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::Z)) {
            actions.push(undo);
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::Y)) {
            actions.push(redo);
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::N)) {
            actions.push(|s| s.swmm_doc.request(PendingAction::New));
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::O)) {
            actions.push(|s| s.swmm_doc.request(PendingAction::Open));
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl_shift, Key::S)) {
            actions.push(save_model_as);
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::S)) {
            actions.push(save_model);
        }
        if !typing {
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::A)) {
                actions.push(|s| s.swmm_doc.select_all());
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::C)) {
                actions.push(copy);
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::X)) {
                actions.push(cut);
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::V)) {
                actions.push(paste);
            }
            if i.key_pressed(Key::Delete) {
                actions.push(delete);
            }
            if i.key_pressed(Key::Escape) {
                actions.push(|s| {
                    if s.swmm_doc.profile_pick.is_some() {
                        s.swmm_doc.profile_pick = None;
                        s.status = "Profile pick cancelled".into();
                    } else if s.swmm_doc.edit.in_progress() {
                        s.swmm_doc.edit.cancel();
                        s.status = "Cancelled".into();
                    } else if !s.swmm_doc.selection.is_empty() {
                        s.swmm_doc.clear_selection();
                        s.status = "Selection cleared".into();
                    } else {
                        s.swmm_doc.set_tool(SwmmTool::Select);
                    }
                });
            }
            if i.key_pressed(Key::Enter) && !state.swmm_doc.edit.polygon.is_empty() {
                actions.push(|s| {
                    if let Some(name) = finish_polygon(&mut s.swmm_doc) {
                        s.status = format!("Added subcatchment {name}");
                    }
                });
            }
            if i.key_pressed(Key::F) {
                actions.push(|s| {
                    if s.swmm_doc.selection.is_empty() {
                        s.swmm.pending_map_fit = true;
                    } else {
                        s.swmm_doc.canvas.pending_zoom_selection = true;
                    }
                });
            }
            if i.key_pressed(Key::F5) {
                actions.push(run_model);
            }
            if i.modifiers.is_none() {
                for t in SwmmTool::all() {
                    let key = match t.shortcut() {
                        "S" => Key::S,
                        "H" => Key::H,
                        "+" => Key::Plus,
                        "-" => Key::Minus,
                        "Z" => Key::Z,
                        "R" => Key::R,
                        "J" => Key::J,
                        "O" => Key::O,
                        "D" => Key::D,
                        "T" => Key::T,
                        "C" => Key::C,
                        "P" => Key::P,
                        "I" => Key::I,
                        "W" => Key::W,
                        "U" => Key::U,
                        "A" => Key::A,
                        "L" => Key::L,
                        _ => continue,
                    };
                    if i.key_pressed(key) {
                        tool = Some(t);
                    }
                }
            }
        }
        if i.key_pressed(Key::F1) {
            actions.push(|s| open_help(&mut s.help, HelpTopic::GettingStarted));
        }
    });
    for a in actions {
        a(state);
    }
    if let Some(t) = tool {
        state.swmm_doc.set_tool(t);
        state.status = t.label();
    }
}

// --- dialogs --------------------------------------------------------------------

pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    swmm_dialogs::draw(ctx, state);
    crate::swmm_gis::draw_dialogs(ctx, state);
    crate::swmm_twod::draw_dialogs(ctx, state);
    crate::swmm_lid::draw_dialogs(ctx, state);
    crate::swmm_scenarios::draw_dialogs(ctx, state);
    crate::swmm_calib::draw_dialogs(ctx, state);
    crate::swmm_live::draw_dialogs(ctx, state);
    crate::swmm_grids::draw_grid_window(ctx, state);
    if let Some(action) = state.swmm_doc.pending.clone() {
        egui::Window::new("Unsaved SWMM model")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(170.0, 60.0))
            .movable(true)
            .show(ctx, |ui| {
                ui.label(format!(
                    "\"{}\" has unsaved changes.",
                    state.swmm_doc.file_name()
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save model…").clicked() {
                        save_model(state);
                        if !state.swmm_doc.dirty() {
                            state.swmm_doc.perform(action.clone());
                        }
                    }
                    if ui.button("Discard changes").clicked() {
                        state.swmm_doc.perform(action.clone());
                    }
                    if ui.button("Keep editing").clicked() {
                        state.swmm_doc.pending = None;
                    }
                });
            });
    }
    if let Some((items, impact)) = state.swmm_doc.delete_confirm.clone() {
        egui::Window::new("Delete takes more with it")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(190.0, 80.0))
            .movable(true)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Deleting {} also removes:",
                    match items.as_slice() {
                        [one] => describe(one),
                        many => format!("{} objects", many.len()),
                    }
                ));
                if !impact.links.is_empty() {
                    ui.label(format!("  links {}", impact.links.join(", ")));
                }
                if !impact.orphaned_subcatchments.is_empty() {
                    ui.label(format!(
                        "  and leaves {} without an outlet",
                        impact.orphaned_subcatchments.join(", ")
                    ));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Delete all of it").clicked() {
                        let n = state.swmm_doc.confirm_delete();
                        state.status = format!("Deleted {n} object(s) and their links");
                    }
                    if ui.button("Keep them").clicked() {
                        state.swmm_doc.delete_confirm = None;
                    }
                });
            });
    }
    if let Some(findings) = state.swmm_doc.run_refused.clone() {
        egui::Window::new("The model has errors")
            .collapsible(false)
            .resizable(true)
            .default_pos(ctx.screen_rect().center() - egui::vec2(220.0, 100.0))
            .movable(true)
            .show(ctx, |ui| {
                ui.label("The engine would refuse or misread this model. Fix these first:");
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for f in findings.iter().filter(|f| f.severity == Severity::Error) {
                            let target = state.swmm_doc.finding_target(f);
                            let line = format!("[{}] {}: {}", f.section, f.name, f.message);
                            if ui.selectable_label(false, line).clicked() {
                                if let Some(t) = target {
                                    state.swmm_doc.select_only(t.clone());
                                    state.swmm_doc.pending_zoom_to = Some(t);
                                }
                            }
                        }
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Show in findings list").clicked() {
                        state.swmm_doc.show_findings = true;
                        state.swmm_doc.run_refused = None;
                    }
                    if ui.button("Dismiss").clicked() {
                        state.swmm_doc.run_refused = None;
                    }
                });
            });
    }
    if state.swmm_doc.edit.label_prompt.is_some() {
        let mut add = false;
        let mut cancel = false;
        egui::Window::new("Label text")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(140.0, 40.0))
            .movable(true)
            .show(ctx, |ui| {
                if let Some((_, text)) = state.swmm_doc.edit.label_prompt.as_mut() {
                    let resp = ui.add(
                        egui::TextEdit::singleline(text)
                            .id(egui::Id::new("swmm_label_text"))
                            .desired_width(240.0),
                    );
                    resp.request_focus();
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        add = true;
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("Add label").clicked() {
                        add = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if add {
            finish_label(&mut state.swmm_doc);
        } else if cancel {
            state.swmm_doc.edit.label_prompt = None;
        }
    }
}

// --- side panels ------------------------------------------------------------------

/// The left pane: the project browser or the layers pane (tabs), over the
/// engine runner.
pub fn draw_left_panel(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        for (tab, label) in [(LeftTab::Browser, "Project"), (LeftTab::Layers, "Layers")] {
            if ui
                .selectable_label(state.swmm_doc.left_tab == tab, RichText::new(label).heading())
                .clicked()
            {
                state.swmm_doc.left_tab = tab;
            }
        }
    });
    ui.separator();
    match state.swmm_doc.left_tab {
        LeftTab::Browser => crate::swmm_browser::draw_browser(ui, state),
        LeftTab::Layers => crate::swmm_layers::draw_layers_pane(ui, state),
    }
    ui.add_space(8.0);
    ui.separator();
    crate::swmm_panel::draw_swmm_tab(ui, state);
}

/// The property sheet for the selection (`swmm_props`).
pub fn draw_properties_panel(ui: &mut Ui, state: &mut AppState) {
    crate::swmm_props::draw_sheet(ui, state);
}
