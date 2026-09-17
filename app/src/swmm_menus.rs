// SPDX-License-Identifier: GPL-3.0-or-later

//! The SWMM editor workspace's chrome: menu bar (File, Edit, View, Project,
//! Run, Results, Tools), toolbar, keyboard shortcuts, dialogs, and the
//! side panels. The storm-sewer design view stays one switch away.
//!
//! Project and Results items that belong to other parts of the editor
//! (the property sheet, the attribute grids, result tables) are here as
//! hooks that say so, so the menu shape is settled before they arrive.

use eframe::egui::{self, Button, Key, Modifiers, RichText, Ui};
use stormsewer_swmm::doc::build::ObjRef;
use stormsewer_swmm::doc::Severity;

use crate::help::{open_help, HelpTopic};
use crate::state::AppState;
use crate::swmm_canvas::{delete_active_vertex, finish_label, finish_polygon, zoom_to};
use crate::swmm_doc::{describe, PendingAction};
use crate::swmm_panel::SwmmSubView;
use crate::swmm_tools::SwmmTool;
use crate::theme::palette;

/// Open the SWMM workspace (with a blank model if none is open).
pub fn enter_workspace(state: &mut AppState) {
    state.swmm_doc.active = true;
    state.swmm_doc.ensure_recent();
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
/// errors; otherwise the current text goes to the chosen engine.
pub fn run_model(state: &mut AppState) {
    state.swmm.ensure_discovered();
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
        state
            .swmm
            .map_viewport
            .zoom_at(canvas_rect, canvas_rect.center(), 1.5);
        ui.close_menu();
    }
    if ui.button("Zoom Out").clicked() {
        state
            .swmm
            .map_viewport
            .zoom_at(canvas_rect, canvas_rect.center(), 1.0 / 1.5);
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
        .clicked()
    {
        if let Some(first) = state.swmm_doc.selection.first().cloned() {
            zoom_to(state, canvas_rect, &first);
        }
        ui.close_menu();
    }
    if ui.button("Pan").clicked() {
        state.swmm_doc.set_tool(SwmmTool::Pan);
        ui.close_menu();
    }
    ui.separator();
    ui.checkbox(&mut state.swmm_doc.show_labels, "Object Labels");
    ui.checkbox(&mut state.swmm_doc.show_arrows, "Flow Arrows");
    ui.checkbox(&mut state.swmm_doc.show_grid, "Grid");
    ui.checkbox(&mut state.swmm_doc.snap_objects, "Snap to Objects");
    ui.checkbox(&mut state.swmm_doc.snap_grid, "Snap to Grid");
    ui.menu_button("Map Layers", |ui| {
        ui.label("Layer toggles arrive with the results-on-map view.");
        ui.checkbox(&mut state.swmm_doc.show_labels, "Labels");
    });
    ui.separator();
    for (view, label) in [(SwmmSubView::Map, "Map"), (SwmmSubView::Chart, "Chart")] {
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

fn not_yet(state: &mut AppState, what: &str) {
    state.status = format!("{what}: not in this build yet — coming with the property sheet");
}

pub fn project_menu(ui: &mut Ui, state: &mut AppState) {
    if ui.button("Title/Notes…").clicked() {
        let title = state.swmm_doc.doc.title();
        state.status = if title.is_empty() {
            "Title: (none) — the property sheet will edit it".into()
        } else {
            format!("Title: {}", title.replace('\n', " / "))
        };
        ui.close_menu();
    }
    if ui.button("Options…").clicked() {
        not_yet(state, "Simulation options");
        ui.close_menu();
    }
    if ui.button("Rain Gages…").clicked() {
        not_yet(state, "Rain gage table");
        ui.close_menu();
    }
    if ui.button("Curves…").clicked() {
        not_yet(state, "Curve editor");
        ui.close_menu();
    }
    if ui.button("Time Series…").clicked() {
        not_yet(state, "Time series editor");
        ui.close_menu();
    }
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
    if ui.button("Find Engines").clicked() {
        state.swmm.rescan();
        ui.close_menu();
    }
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
        state.status = "The engine runs to completion; it cannot be interrupted yet".into();
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
    if ui.button("Python Terminal…").clicked() {
        state.python_term.open = true;
        ui.close_menu();
    }
    if ui.button("Tc Calculator…").clicked() {
        state.open_tc_calculator();
        ui.close_menu();
    }
    ui.separator();
    if ui.button("Storm Sewer Design Workspace").clicked() {
        leave_workspace(state);
        ui.close_menu();
    }
}

/// The whole SWMM menu bar except Help, which the app draws itself.
pub fn draw_menus(ui: &mut Ui, ctx: &egui::Context, state: &mut AppState, canvas_rect: egui::Rect) {
    ui.menu_button("File", |ui| file_menu(ui, ctx, state));
    ui.menu_button("Edit", |ui| edit_menu(ui, state));
    ui.menu_button("View", |ui| view_menu(ui, state, canvas_rect));
    ui.menu_button("Project", |ui| project_menu(ui, state));
    ui.menu_button("Run", |ui| run_menu(ui, state));
    ui.menu_button("Results", |ui| results_menu(ui, state));
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
        ui.separator();
        ui.checkbox(&mut state.swmm_doc.snap_grid, "Grid snap");
        if state.swmm_doc.snap_grid {
            ui.add(
                egui::DragValue::new(&mut state.swmm_doc.grid_spacing)
                    .speed(1.0)
                    .range(0.1..=10000.0),
            );
        }
        ui.checkbox(&mut state.swmm_doc.snap_objects, "Object snap");

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .selectable_label(false, "Storm Sewer")
                .on_hover_text("Switch to the storm sewer design workspace")
                .clicked()
            {
                leave_workspace(state);
            }
            let _ = ui.selectable_label(true, "SWMM");
            ui.separator();
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
    let typing = ctx.wants_keyboard_input();
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
                    if s.swmm_doc.edit.in_progress() {
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
                actions.push(|s| s.swmm.pending_map_fit = true);
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

/// Project browser (objects by kind; click selects and zooms) over the
/// engine runner.
pub fn draw_left_panel(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Project");
    ui.separator();
    let ed = &state.swmm_doc;
    let groups: Vec<(&str, Vec<ObjRef>)> = vec![
        (
            "Rain Gages",
            ed.gages
                .iter()
                .map(|g| ObjRef::Gage(g.name.clone()))
                .collect(),
        ),
        (
            "Subcatchments",
            ed.subs
                .iter()
                .map(|s| ObjRef::Subcatchment(s.name.clone()))
                .collect(),
        ),
        (
            "Nodes",
            ed.nodes
                .iter()
                .map(|n| ObjRef::Node(n.name.clone()))
                .collect(),
        ),
        (
            "Links",
            ed.links
                .iter()
                .map(|l| ObjRef::Link(l.name.clone()))
                .collect(),
        ),
        (
            "Labels",
            ed.labels.iter().map(|l| ObjRef::Label(l.line)).collect(),
        ),
    ];
    let mut pick: Option<ObjRef> = None;
    egui::ScrollArea::vertical()
        .id_salt("swmm-browser")
        .max_height(260.0)
        .show(ui, |ui| {
            for (title, items) in &groups {
                egui::CollapsingHeader::new(format!("{title} ({})", items.len()))
                    .default_open(false)
                    .show(ui, |ui| {
                        for r in items {
                            let label = match r {
                                ObjRef::Label(li) => ed
                                    .labels
                                    .iter()
                                    .find(|l| l.line == *li)
                                    .map(|l| l.text.clone())
                                    .unwrap_or_default(),
                                _ => r.name().unwrap_or("").to_string(),
                            };
                            if ui.selectable_label(ed.is_selected(r), label).clicked() {
                                pick = Some(r.clone());
                            }
                        }
                    });
            }
        });
    if let Some(r) = pick {
        state.swmm_doc.select_only(r.clone());
        state.swmm_doc.pending_zoom_to = Some(r);
    }
    ui.add_space(8.0);
    ui.separator();
    crate::swmm_panel::draw_swmm_tab(ui, state);
}

/// A read-only property sheet for the selection: the object's defining
/// row by column name. In-place editing is the property-sheet stream's.
pub fn draw_properties_panel(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Properties");
    ui.separator();
    let ed = &state.swmm_doc;
    match ed.selection.as_slice() {
        [] => {
            ui.label("Select an object on the map.");
        }
        [one] => {
            ui.label(RichText::new(describe(one)).strong());
            let row = one.kind().zip(one.name()).and_then(|(k, n)| {
                let sec = ed.doc.defining_section(k, n)?;
                let (_, row) = ed.doc.find(sec, n)?;
                Some((sec, row.clone()))
            });
            if let Some((sec, row)) = row {
                ui.label(RichText::new(format!("[{sec}]")).small());
                let cols = ed.doc.columns(sec, &row);
                egui::Grid::new("swmm-props")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for (i, f) in row.fields.iter().enumerate() {
                            let name = cols.get(i).copied().unwrap_or("");
                            ui.label(if name.is_empty() {
                                format!("#{i}")
                            } else {
                                name.to_string()
                            });
                            ui.label(RichText::new(f).monospace());
                            ui.end_row();
                        }
                    });
            } else if let ObjRef::Label(li) = one {
                if let Some(l) = ed.labels.iter().find(|l| l.line == *li) {
                    ui.label(format!("\"{}\" at {:.2}, {:.2}", l.text, l.x, l.y));
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Editing arrives with the property sheet.").small());
        }
        many => {
            ui.label(format!("{} objects selected", many.len()));
        }
    }
}
