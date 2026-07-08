// SPDX-License-Identifier: GPL-3.0-or-later

//! StormSewer — standalone desktop storm sewer design application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catchment_draw;
mod edit;
mod files;
mod global_edit;
mod help;
mod inspector;
mod panels;
mod plan;
mod prefs;
mod profile;
mod recent;
mod report_editor;
mod state;
mod tables;
mod tc_calc;
mod theme;
mod toolbar;
mod tutorial;
mod undo;
mod viewport;

use eframe::egui::{self, Key, Modifiers, Sense};
use catchment_draw::handle_catchment_click;
use edit::{
    delete_selection, handle_click, merge_node, nearest_other_node, snap_node, snap_placement,
    sync_pipe_lengths, Tool,
};
use global_edit::draw_global_edit_window;
use help::{draw_help_window, open_help, HelpTopic};
use inspector::draw_inspector;
use tc_calc::draw_tc_calc_window;
use panels::{draw_left_panel, draw_report_panel};
use toolbar::draw_toolbar;
use report_editor::draw_report_editor_window;
use plan::draw_plan;
use profile::draw_profile;
use state::{AppState, ViewTab};

const SNAP_RADIUS: f64 = 15.0;

struct StormSewerApp {
    state: AppState,
    show_about: bool,
    canvas_rect: egui::Rect,
}

impl StormSewerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = AppState::new_demo();
        theme::apply(&cc.egui_ctx, state.prefs.theme);
        // The interactive tutorial opens on every launch until the user opts out.
        if !state.prefs.tutorial_done {
            state.tutorial.open = true;
            state.tutorial.step = 0;
        }
        Self {
            state,
            show_about: false,
            canvas_rect: egui::Rect::NOTHING,
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.state.set_tool(tool);
    }

    fn reset_project(&mut self, state: AppState) {
        self.state = state;
        self.state.bg_texture = None;
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl = Modifiers::CTRL;

        ctx.input_mut(|i| {
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::Z)) {
                self.state.undo();
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::Y)) {
                self.state.redo();
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::N)) {
                self.reset_project(AppState::new_empty());
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::O)) {
                self.state.pick_open_project(ctx);
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::S)) {
                self.state.pick_save_project();
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::A)) {
                self.state.run_analysis();
            }
            if i.key_pressed(Key::F5) {
                self.state.run_analysis();
            }
            if i.key_pressed(Key::Delete) {
                self.state.checkpoint_undo();
                if let Some(msg) = delete_selection(
                    &mut self.state.project,
                    self.state.selected_node,
                    self.state.selected_pipe,
                ) {
                    self.state.status = msg;
                    self.state.clear_selection();
                    self.state.run_analysis();
                    self.state.update_inlet_check();
                }
            }
            if i.key_pressed(Key::Num1) {
                self.set_tool(Tool::Select);
            }
            if i.key_pressed(Key::Num2) {
                self.set_tool(Tool::PlaceInlet);
            }
            if i.key_pressed(Key::Num3) {
                self.set_tool(Tool::PlaceJunction);
            }
            if i.key_pressed(Key::Num4) {
                self.set_tool(Tool::PlaceOutfall);
            }
            if i.key_pressed(Key::Num5) {
                self.set_tool(Tool::DrawPipe);
            }
            if i.key_pressed(Key::Num6) {
                self.set_tool(Tool::DrawCatchment);
            }
            if i.key_pressed(Key::F) {
                self.state
                    .viewport
                    .zoom_to_fit(self.canvas_rect, &self.state.project);
            }
            if i.key_pressed(Key::G) {
                self.state.viewport.zoom_to_selection(
                    self.canvas_rect,
                    &self.state.project,
                    self.state.selected_node,
                    self.state.selected_pipe,
                );
            }
            if i.key_pressed(Key::F1) {
                open_help(&mut self.state.help, HelpTopic::GettingStarted);
            }
            if i.key_pressed(Key::Escape) {
                if self.state.edit.pipe_from.is_some() {
                    self.state.edit.pipe_from = None;
                    self.state.status = "Pipe drawing cancelled".into();
                } else if !self.state.edit.catchment_vertices.is_empty() {
                    self.state.edit.catchment_vertices.clear();
                    self.state.status = "Catchment drawing cancelled".into();
                } else if self.state.tc_calc.open {
                    self.state.tc_calc.open = false;
                }
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::P)) {
                self.state.print_report();
            }
        });
    }
}

impl eframe::App for StormSewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            self.state.window_title().into(),
        ));

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        self.reset_project(AppState::new_empty());
                        ui.close_menu();
                    }
                    if ui.button("New Demo Project").clicked() {
                        let help = self.state.help.clone();
                        self.reset_project(AppState::new_demo());
                        self.state.help = help;
                        ui.close_menu();
                    }
                    if ui.button("Open Project…").clicked() {
                        self.state.pick_open_project(ctx);
                        ui.close_menu();
                    }
                    if !self.state.recent.paths.is_empty() {
                        ui.menu_button("Recent Projects", |ui| {
                            let recent: Vec<_> = self.state.recent.paths.clone();
                            for path in recent {
                                let label = self.state.recent.label(&path);
                                if ui.button(label).clicked() {
                                    self.state.open_project_path(ctx, path);
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    if ui.button("Save Project…").clicked() {
                        self.state.pick_save_project();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Import DXF…").clicked() {
                        self.state.pick_import_dxf(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Import LandXML…").clicked() {
                        self.state.pick_import_landxml(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Import Hydraflow STM…").clicked() {
                        self.state.pick_import_stm(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Export DXF…").clicked() {
                        self.state.pick_export_dxf();
                        ui.close_menu();
                    }
                    if ui.button("Export LandXML…").clicked() {
                        self.state.pick_export_landxml();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Load PNG Background…").clicked() {
                        self.state.pick_background(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Export PDF Report…").clicked() {
                        self.state.pick_export_pdf();
                        ui.close_menu();
                    }
                    if ui.button("Export HTML Report…").clicked() {
                        self.state.pick_export_html();
                        ui.close_menu();
                    }
                    if ui.button("Print Report (Ctrl+P)").clicked() {
                        self.state.print_report();
                        ui.close_menu();
                    }
                    ui.menu_button("Custom Report (MyReport)", |ui| {
                        if ui.button("Municipal Summary").clicked() {
                            self.state
                                .set_report_template(stormsewer::io::ReportTemplate::municipal_summary());
                            ui.close_menu();
                        }
                        if ui.button("Hydraflow Pipe Table").clicked() {
                            self.state
                                .set_report_template(stormsewer::io::ReportTemplate::hydraflow_style());
                            ui.close_menu();
                        }
                        if ui.button("Cost Report").clicked() {
                            self.state
                                .set_report_template(stormsewer::io::ReportTemplate::cost_report());
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Export Custom CSV…").clicked() {
                            self.state.pick_export_custom_csv();
                            ui.close_menu();
                        }
                        if ui.button("Export Custom HTML…").clicked() {
                            self.state.pick_export_custom_html();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Load Template (.srpt)…").clicked() {
                            self.state.pick_load_report_template();
                            ui.close_menu();
                        }
                        if ui.button("Save Template (.srpt)…").clicked() {
                            self.state.pick_save_report_template();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Edit Columns…").clicked() {
                            self.state.show_report_editor = true;
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    ui.checkbox(
                        &mut self.state.open_report_after_export,
                        "Open report after export",
                    );
                });
                ui.menu_button("Edit", |ui| {
                    let can_undo = self.state.undo.can_undo();
                    let can_redo = self.state.undo.can_redo();
                    if ui
                        .add_enabled(can_undo, egui::Button::new("Undo"))
                        .clicked()
                    {
                        self.state.undo();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(can_redo, egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.state.redo();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Global Pipe Editing…").clicked() {
                        self.state.show_global_edit = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Tools", |ui| {
                    if ui.button("Tc Calculator…").clicked() {
                        self.state.open_tc_calculator();
                        ui.close_menu();
                    }
                    if ui.button("Run Diagnostics").clicked() {
                        self.state.update_diagnostics();
                        self.state.side_tab = panels::SideTab::Review;
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Zoom Extents (F)").clicked() {
                        self.state
                            .viewport
                            .zoom_to_fit(self.canvas_rect, &self.state.project);
                        ui.close_menu();
                    }
                    if ui.button("Zoom to Selection (G)").clicked() {
                        self.state.viewport.zoom_to_selection(
                            self.canvas_rect,
                            &self.state.project,
                            self.state.selected_node,
                            self.state.selected_pipe,
                        );
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.selectable_label(self.state.view_tab == ViewTab::Plan, "Plan").clicked() {
                        self.state.view_tab = ViewTab::Plan;
                        ui.close_menu();
                    }
                    if ui
                        .selectable_label(self.state.view_tab == ViewTab::Profile, "Profile")
                        .clicked()
                    {
                        self.state.view_tab = ViewTab::Profile;
                        ui.close_menu();
                    }
                    ui.separator();
                    let mut light = self.state.prefs.theme == theme::Theme::Light;
                    if ui.checkbox(&mut light, "Light theme").clicked() {
                        self.state.prefs.theme =
                            if light { theme::Theme::Light } else { theme::Theme::Dark };
                        self.state.prefs.save();
                        theme::apply(ctx, self.state.prefs.theme);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Interactive Tutorial").clicked() {
                        self.state.tutorial.open = true;
                        self.state.tutorial.step = 0;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Getting Started").clicked() {
                        open_help(&mut self.state.help, HelpTopic::GettingStarted);
                        ui.close_menu();
                    }
                    if ui.button("Quick Start Tutorial").clicked() {
                        open_help(&mut self.state.help, HelpTopic::QuickStart);
                        ui.close_menu();
                    }
                    if ui.button("Design Workflow").clicked() {
                        open_help(&mut self.state.help, HelpTopic::DesignWorkflow);
                        ui.close_menu();
                    }
                    if ui.button("Computational Methods").clicked() {
                        open_help(&mut self.state.help, HelpTopic::Hydrology);
                        ui.close_menu();
                    }
                    if ui.button("File Import & Export").clicked() {
                        open_help(&mut self.state.help, HelpTopic::FileIo);
                        ui.close_menu();
                    }
                    if ui.button("Hydraflow Migration Guide").clicked() {
                        open_help(&mut self.state.help, HelpTopic::HydraflowMigration);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Keyboard Shortcuts…").clicked() {
                        open_help(&mut self.state.help, HelpTopic::KeyboardShortcuts);
                        ui.close_menu();
                    }
                    if ui.button("Troubleshooting").clicked() {
                        open_help(&mut self.state.help, HelpTopic::Troubleshooting);
                        ui.close_menu();
                    }
                    if ui.button("About StormSewer…").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
                ui.separator();
                ui.label(self.state.project.name.clone());
            });
        });

        egui::TopBottomPanel::top("toolbar")
            .exact_height(32.0)
            .show(ctx, |ui| draw_toolbar(ui, &mut self.state, self.canvas_rect));

        draw_help_window(ctx, &mut self.state.help);
        draw_global_edit_window(ctx, &mut self.state);
        draw_report_editor_window(ctx, &mut self.state);
        draw_tc_calc_window(ctx, &mut self.state);
        tutorial::draw_tutorial(ctx, &mut self.state);

        if self.show_about {
            egui::Window::new("About StormSewer")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("StormSewer v0.7");
                    ui.label("Standalone storm sewer design desktop application.");
                    ui.label("Rational method hydrology, Manning hydraulics, HGL backwater.");
                    ui.label("HEC-22 inlet analysis, DXF/LandXML exchange, PDF/HTML reports.");
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.show_about = false;
                    }
                });
        }

        egui::SidePanel::left("params")
            .default_width(240.0)
            .resizable(true)
            .show(ctx, |ui| draw_left_panel(ui, &mut self.state));

        egui::SidePanel::right("report")
            .default_width(360.0)
            .resizable(true)
            .show(ctx, |ui| draw_report_panel(ui, &self.state));

        egui::TopBottomPanel::bottom("inspector")
            .resizable(true)
            .default_height(if self.state.has_selection() { 160.0 } else { 72.0 })
            .show(ctx, |ui| {
                egui::CollapsingHeader::new("Inspector")
                    .default_open(self.state.inspector_open)
                    .show(ui, |ui| {
                        self.state.inspector_open = true;
                        draw_inspector(ui, &mut self.state);
                    });
            });

        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Tool: {} ({})",
                        self.state.tool.label(),
                        self.state.tool.shortcut()
                    ));
                    ui.separator();
                    ui.label(self.state.tool.hint());
                    ui.separator();
                    ui.label(&self.state.status);
                });
            });

        if self.state.pending_zoom_selection {
            self.state.viewport.zoom_to_selection(
                self.canvas_rect,
                &self.state.project,
                self.state.selected_node,
                self.state.selected_pipe,
            );
            self.state.pending_zoom_selection = false;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
            self.canvas_rect = rect;

            if self.state.view_tab == ViewTab::Plan && self.state.tool == Tool::Select {
                if resp.drag_started() {
                    let pos = resp.interact_pointer_pos().unwrap_or(egui::Pos2::ZERO);
                    let (wx, wy) = self.state.viewport.screen_to_world(rect, pos);
                    if let Some(idx) = snap_node(&self.state.project, wx, wy, SNAP_RADIUS) {
                        self.state.checkpoint_undo();
                        self.state.dragging_node = Some(idx);
                        self.state.set_selection(Some(idx), None, None);
                    }
                }

                if let Some(idx) = self.state.dragging_node {
                    if resp.dragged() {
                        // Position the node at the cursor, snapped to the drawing
                        // grid — live grid feedback instead of free-floating drag.
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let (wx, wy) = self.state.viewport.screen_to_world(rect, pos);
                            let (sx, sy) = snap_placement(wx, wy, self.state.prefs.snap_grid_ft);
                            if idx < self.state.project.nodes.len() {
                                self.state.project.nodes[idx].x = sx;
                                self.state.project.nodes[idx].y = sy;
                                sync_pipe_lengths(&mut self.state.project);
                            }
                        }
                    }
                    if resp.drag_stopped() {
                        // Released over another node? Merge the dragged one into it.
                        let merged = {
                            let project = &mut self.state.project;
                            let (nx, ny) = (project.nodes[idx].x, project.nodes[idx].y);
                            match nearest_other_node(project, nx, ny, SNAP_RADIUS, idx) {
                                Some(t) => {
                                    let to_id = project.nodes[t].id.clone();
                                    merge_node(project, idx, &to_id).map(|msg| (msg, to_id))
                                }
                                None => None,
                            }
                        };
                        if let Some((msg, to_id)) = merged {
                            self.state.status = msg;
                            let ni = self.state.project.nodes.iter().position(|n| n.id == to_id);
                            self.state.set_selection(ni, None, None);
                        }
                        self.state.dragging_node = None;
                        self.state.run_analysis();
                        self.state.update_inlet_check();
                        ui.ctx().request_repaint();
                    }
                }
            }

            if self.state.dragging_node.is_none() {
                self.state.viewport.handle_pan_zoom(&resp, ui);
            }

            if resp.clicked()
                && self.state.view_tab == ViewTab::Plan
                && self.state.dragging_node.is_none()
            {
                let pos = resp.interact_pointer_pos().unwrap_or(egui::Pos2::ZERO);
                let (wx, wy) = self.state.viewport.screen_to_world(rect, pos);
                if self.state.edit.tool == Tool::DrawCatchment {
                    let closing = self.state.edit.catchment_vertices.len() >= 3
                        && {
                            let (fx, fy) = self.state.edit.catchment_vertices[0];
                            let dx = wx - fx;
                            let dy = wy - fy;
                            (dx * dx + dy * dy).sqrt() <= 20.0
                        };
                    if closing {
                        self.state.checkpoint_undo();
                    }
                    if let Some(msg) =
                        handle_catchment_click(&mut self.state.project, &mut self.state.edit, wx, wy)
                    {
                        self.state.status = msg.clone();
                        if msg.starts_with("Added catchment") {
                            self.state.run_analysis();
                        }
                    }
                } else {
                    let should_checkpoint = match self.state.edit.tool {
                        Tool::Select | Tool::DrawCatchment => false,
                        // Every DrawPipe click now mutates: it drops a manhole
                        // and/or links a pipe, so each is an undo step.
                        Tool::DrawPipe => true,
                        _ => true,
                    };
                    if should_checkpoint {
                        self.state.checkpoint_undo();
                    }
                    let grid_ft = self.state.prefs.snap_grid_ft;
                    self.state.edit.zero_area_nodes = self.state.prefs.draw_zero_area;
                    let result = handle_click(
                        &mut self.state.project,
                        &mut self.state.edit,
                        wx,
                        wy,
                        grid_ft,
                    );
                    if let Some(msg) = result.status {
                        self.state.status = msg;
                    }
                    if result.selected_node.is_some()
                        || result.selected_pipe.is_some()
                        || result.selected_catchment.is_some()
                    {
                        self.state.set_selection(
                            result.selected_node,
                            result.selected_pipe,
                            result.selected_catchment,
                        );
                        self.state.update_inlet_check();
                    } else if self.state.edit.tool == Tool::Select {
                        self.state.clear_selection();
                    }
                    if result.needs_analysis {
                        self.state.run_analysis();
                    }
                }
                // The inspector/status panels above were already laid out earlier
                // this frame, so repaint once more to reflect the new selection
                // immediately instead of waiting for the next input event.
                ui.ctx().request_repaint();
            }

            // Right-click or double-click finishes a pipe run (same as Esc), matching
            // CAD polyline muscle memory. The double-click's first click already
            // dropped the final manhole via the block above; this just ends the run.
            if self.state.view_tab == ViewTab::Plan
                && self.state.edit.tool == Tool::DrawPipe
                && self.state.edit.pipe_from.is_some()
                && (resp.secondary_clicked() || resp.double_clicked())
            {
                self.state.edit.pipe_from = None;
                self.state.status = "Run finished".into();
            }

            // In Draw Pipe mode, find the node the cursor would snap to. It both
            // highlights the tie-in target and ends the rubber-band preview cleanly
            // on that node instead of the raw cursor position.
            let hover_world = resp
                .hover_pos()
                .map(|pos| self.state.viewport.screen_to_world(rect, pos));
            let snap_target = if let Some(idx) = self.state.dragging_node {
                // While dragging, ring the node the dragged one would merge into.
                self.state
                    .project
                    .nodes
                    .get(idx)
                    .and_then(|n| nearest_other_node(&self.state.project, n.x, n.y, SNAP_RADIUS, idx))
            } else if self.state.view_tab == ViewTab::Plan
                && self.state.edit.tool == Tool::DrawPipe
            {
                hover_world
                    .and_then(|(wx, wy)| snap_node(&self.state.project, wx, wy, SNAP_RADIUS))
            } else {
                None
            };
            let pipe_preview_to = if self.state.edit.pipe_from.is_some() {
                match snap_target.and_then(|i| self.state.project.nodes.get(i)) {
                    Some(n) => Some((n.x, n.y)),
                    None => hover_world,
                }
            } else {
                None
            };

            match self.state.view_tab {
                ViewTab::Plan => draw_plan(
                    ui,
                    rect,
                    &self.state.project,
                    self.state.analysis.as_ref(),
                    &self.state.viewport,
                    self.state.bg_texture.as_ref(),
                    &self.state.dxf_underlay,
                    &self.state.edit,
                    self.state.selected_node,
                    self.state.selected_pipe,
                    &self.state.findings,
                    Some(self.state.tool.label()),
                    pipe_preview_to,
                    snap_target,
                ),
                ViewTab::Profile => draw_profile(
                    ui,
                    rect,
                    &self.state.project,
                    self.state.analysis.as_ref(),
                ),
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 860.0])
            .with_title("StormSewer v0.7"),
        ..Default::default()
    };
    eframe::run_native(
        "StormSewer",
        options,
        Box::new(|cc| Ok(Box::new(StormSewerApp::new(cc)))),
    )
}