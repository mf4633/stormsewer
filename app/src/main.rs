// SPDX-License-Identifier: GPL-3.0-or-later

//! StormSewer — standalone desktop storm sewer design application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catchment_draw;
mod edit;
mod files;
mod global_edit;
mod help;
mod inspector;
mod menu;
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
#[cfg(test)]
mod ui_tests;

use eframe::egui::{self, Key, Modifiers, Sense};
use catchment_draw::handle_catchment_click;
use edit::{
    delete_selection, handle_click, merge_node, nearest_other_node, snap_node, snap_pipe,
    snap_placement, sync_pipe_lengths, ContextTarget, Tool,
};
use global_edit::draw_global_edit_window;
use help::{draw_help_window, open_help, HelpTopic};
use inspector::draw_inspector;
use menu::draw_context_menu;
use tc_calc::draw_tc_calc_window;
use panels::{draw_left_panel, draw_report_panel};
use toolbar::draw_toolbar;
use report_editor::draw_report_editor_window;
use plan::draw_plan;
use profile::draw_profile;
use state::{AppState, ViewTab};

const SNAP_RADIUS: f64 = 15.0;

/// Support / donation link surfaced in the Help menu and About dialog.
const SUPPORT_URL: &str = "https://buymeacoffee.com/mf4633";

/// Buy Me a Coffee brand yellow (#FFDD00) and dark ink used on the button.
const BMC_YELLOW: egui::Color32 = egui::Color32::from_rgb(255, 221, 0);
const BMC_INK: egui::Color32 = egui::Color32::from_rgb(15, 15, 20);

/// Render the branded "Buy me a coffee" button. Opens [`SUPPORT_URL`] in the
/// browser when clicked. Styled to echo the Buy Me a Coffee button (yellow pill,
/// dark text) so it reads as the familiar widget rather than a plain link.
fn coffee_button(ui: &mut egui::Ui) {
    let label = egui::RichText::new("☕  Buy me a coffee").color(BMC_INK).strong();
    let btn = egui::Button::new(label)
        .fill(BMC_YELLOW)
        .stroke(egui::Stroke::NONE)
        .rounding(egui::Rounding::same(6.0))
        .min_size(egui::vec2(0.0, 30.0));
    let resp = ui
        .add(btn)
        .on_hover_text("Support continued development — thank you!");
    if resp.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(SUPPORT_URL));
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

struct StormSewerApp {
    state: AppState,
    show_about: bool,
    canvas_rect: egui::Rect,
    /// Unsaved-changes dialog raised by an intercepted close request.
    show_close_confirm: bool,
    /// Set once the user chooses to close (with or without saving).
    allow_close: bool,
    /// Offer to restore a crash-recovery autosave found at startup.
    show_recovery: bool,
    last_autosave: Option<std::time::Instant>,
    /// Last dark/light resolution applied to the egui style.
    applied_dark: Option<bool>,
    /// The rare "support this project" prompt.
    show_coffee: bool,
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
        let show_recovery = crate::prefs::autosave_path().exists();
        Self {
            state,
            show_about: false,
            canvas_rect: egui::Rect::NOTHING,
            show_close_confirm: false,
            allow_close: false,
            show_recovery,
            // Start the clock now: the first snapshot lands a full interval
            // after launch, not on the first frame after the first edit.
            last_autosave: Some(std::time::Instant::now()),
            applied_dark: None,
            show_coffee: false,
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.state.set_tool(tool);
    }
    #[cfg(test)]
    pub(crate) fn new_for_test(state: AppState) -> Self {
        Self {
            state,
            show_about: false,
            canvas_rect: egui::Rect::NOTHING,
            show_close_confirm: false,
            allow_close: false,
            show_recovery: false,
            last_autosave: Some(std::time::Instant::now()),
            applied_dark: None,
            show_coffee: false,
        }
    }

    /// Write the crash-recovery snapshot when the project is dirty; every
    /// 60 s in normal use, immediately when `force` is set (tests).
    fn maybe_autosave(&mut self, force: bool) {
        if !self.state.project_dirty {
            return;
        }
        let due = force
            || self
                .last_autosave
                .map_or(true, |t| t.elapsed().as_secs() >= 60);
        if !due {
            return;
        }
        let path = crate::prefs::autosave_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if self.state.project.save(&path).is_ok() {
            self.last_autosave = Some(std::time::Instant::now());
        }
    }

    fn clear_autosave() {
        let _ = std::fs::remove_file(crate::prefs::autosave_path());
    }

    /// Restore the crash-recovery snapshot into the app (path-less and
    /// dirty, so the user decides where it lives).
    fn restore_recovery(&mut self) {
        let path = crate::prefs::autosave_path();
        match stormsewer::io::Project::load(&path) {
            Ok(project) => {
                self.state.load_project(project, None);
                self.state.mark_project_dirty();
                self.state.status =
                    "Recovered unsaved work — use Save Project… to keep it".into();
            }
            Err(e) => self.state.status = format!("Recovery failed: {e}"),
        }
        self.show_recovery = false;
    }

    /// The rare support prompt: friendly, movable, one-click gone.
    fn draw_coffee_prompt(&mut self, ctx: &egui::Context) {
        if !self.show_coffee {
            return;
        }
        egui::Window::new("Enjoying StormSewer?")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(180.0, 80.0))
            .movable(true)
            .show(ctx, |ui| {
                ui.label(
                    "StormSewer is free and stays free. If it's earning its \
                     keep on your projects, you can support the work:",
                );
                ui.add_space(6.0);
                ui.hyperlink_to(
                    "\u{2615} Buy me a coffee",
                    "https://buy.stripe.com/14A3cudxo91z1qo0OHdAk00?client_reference_id=stormsewer-nag",
                );
                ui.hyperlink_to(
                    "Custom features & firm support — support@hydrocomplete.com",
                    "mailto:support@hydrocomplete.com?subject=StormSewer",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Maybe later").clicked() {
                        self.show_coffee = false;
                    }
                    if ui.button("Don't ask again").clicked() {
                        self.state.prefs.coffee_optout = true;
                        self.state.prefs.save();
                        self.show_coffee = false;
                    }
                });
            });
    }

    /// Distance entry for two-point background calibration.
    fn draw_bg_scale_dialog(&mut self, ctx: &egui::Context) {
        let ready = self.state.bg_calibrate.active
            && self.state.bg_calibrate.point_a.is_some()
            && self.state.bg_calibrate.point_b.is_some();
        if !ready {
            return;
        }
        let units = match self.state.project.units {
            stormsewer::units::UnitSystem::UsCustomary => "ft",
            stormsewer::units::UnitSystem::Si => "m",
        };
        egui::Window::new("Set background scale")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(160.0, 60.0))
            .movable(true)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Real distance between the two clicked points ({units}):"
                ));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.state.bg_calibrate.distance_text)
                        .id(egui::Id::new("bg_scale_distance"))
                        .desired_width(120.0),
                );
                resp.request_focus();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let submit = ui.button("Set scale").clicked()
                        || ui.input(|inp| inp.key_pressed(egui::Key::Enter));
                    if submit {
                        match self.state.bg_calibrate.distance_text.trim().parse::<f64>()
                        {
                            Ok(d) => {
                                if let Err(e) = self.state.apply_bg_calibration(d) {
                                    self.state.status = e;
                                }
                            }
                            Err(_) => {
                                self.state.status =
                                    "Enter the distance as a number, e.g. 250".into();
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.state.cancel_bg_calibration();
                    }
                });
            });
    }

    /// Intercept window close: dirty projects get a Save / Discard /
    /// Cancel choice instead of silent data loss.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.state.project_dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_close_confirm = true;
        } else {
            Self::clear_autosave();
        }
    }

    fn draw_close_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_close_confirm {
            return;
        }
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(170.0, 60.0))
            .movable(true)
            .show(ctx, |ui| {
                ui.label(format!(
                    "\"{}\" has unsaved changes.",
                    self.state.project.name
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save project…").clicked() {
                        self.state.pick_save_project();
                        if !self.state.project_dirty {
                            self.allow_close = true;
                            self.show_close_confirm = false;
                            Self::clear_autosave();
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    if ui.button("Discard and close").clicked() {
                        self.allow_close = true;
                        self.show_close_confirm = false;
                        Self::clear_autosave();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_close_confirm = false;
                    }
                });
            });
    }

    fn draw_recovery_prompt(&mut self, ctx: &egui::Context) {
        if !self.show_recovery {
            return;
        }
        egui::Window::new("Recovered work found")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(190.0, 70.0))
            .movable(true)
            .show(ctx, |ui| {
                ui.label(
                    "StormSewer closed with unsaved changes last time. \
                     An automatic recovery snapshot is available.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Restore recovered work").clicked() {
                        self.restore_recovery();
                    }
                    if ui.button("Delete snapshot").clicked() {
                        Self::clear_autosave();
                        self.show_recovery = false;
                    }
                });
            });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
                    if ui
                        .button("Import NOAA Atlas 14 IDF…")
                        .on_hover_text("Fit a/b/c IDF curves from a NOAA PFDS precipitation CSV")
                        .clicked()
                    {
                        self.state.pick_import_noaa(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .button("Paste NOAA Atlas 14 Data…")
                        .on_hover_text("Paste NOAA PFDS CSV text directly and fit IDF curves")
                        .clicked()
                    {
                        self.state.noaa_paste_open = true;
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
                        self.state.open_report_options();
                        ui.close_menu();
                    }
                    if ui.button("Export HTML Report…").clicked() {
                        self.state.pick_export_html();
                        ui.close_menu();
                    }
                    if ui.button("Print Report (Ctrl+P)").clicked() {
                        self.state.open_report_options();
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
                    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
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
                    }

    fn tools_menu(&mut self, ui: &mut egui::Ui) {
                    if ui.button("Tc Calculator…").clicked() {
                        self.state.open_tc_calculator();
                        ui.close_menu();
                    }
                    if ui.button("Run Diagnostics").clicked() {
                        self.state.update_diagnostics();
                        self.state.side_tab = panels::SideTab::Review;
                        ui.close_menu();
                    }
                    }

    fn view_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
                    for (label, choice) in [
                        ("Dark theme", theme::Theme::Dark),
                        ("Light theme", theme::Theme::Light),
                        ("Follow system theme", theme::Theme::System),
                    ] {
                        if ui
                            .selectable_label(self.state.prefs.theme == choice, label)
                            .clicked()
                        {
                            self.state.prefs.theme = choice;
                            self.state.prefs.save();
                            ui.close_menu();
                        }
                    }
                    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
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
                    ui.separator();
                    ui.hyperlink_to("☕ Support StormSewer", SUPPORT_URL)
                        .on_hover_text("Buy me a coffee — support continued development");
                    if ui.button("Support & Custom Work…").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(
                            "mailto:support@hydrocomplete.com?subject=StormSewer%20support",
                        ));
                        ui.close_menu();
                    }
                    if ui.button("About StormSewer…").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
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
                if !self.state.multi_nodes.is_empty()
                    || !self.state.multi_pipes.is_empty()
                {
                    self.state.delete_multi();
                } else {
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
                if self.state.bg_calibrate.active {
                    self.state.cancel_bg_calibration();
                } else if self.state.edit.pipe_from.is_some() {
                    self.state.edit.pipe_from = None;
                    self.state.status = "Pipe drawing cancelled".into();
                } else if !self.state.edit.catchment_vertices.is_empty() {
                    self.state.edit.catchment_vertices.clear();
                    self.state.status = "Catchment drawing cancelled".into();
                } else if !self.state.multi_nodes.is_empty()
                    || !self.state.multi_pipes.is_empty()
                {
                    self.state.multi_nodes.clear();
                    self.state.multi_pipes.clear();
                    self.state.status = "Selection cleared".into();
                } else if !self.state.profile_pipes.is_empty() {
                    self.state.profile_pipes.clear();
                    self.state.status =
                        "Profile run cleared — Profile shows the main trunk".into();
                } else if self.state.tc_calc.open {
                    self.state.tc_calc.open = false;
                }
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(ctrl, Key::P)) {
                self.state.open_report_options();
            }
        });
    }
}

impl StormSewerApp {
    /// Full per-frame UI, extracted from `eframe::App::update` so headless
    /// tests can drive complete frames without an eframe window.
    fn ui(&mut self, ctx: &egui::Context) {
        // Theme resolution: prefs (Dark / Light / System) against the OS
        // preference, re-applied only when the answer changes.
        let system_dark = ctx
            .input(|i| i.raw.system_theme)
            .map(|t| t == egui::Theme::Dark);
        let dark = self.state.prefs.theme.resolve(system_dark);
        if self.applied_dark != Some(dark) {
            // Latch only once the full type scale is live, so a context
            // whose fonts activate next pass gets one more apply.
            if theme::apply_resolved(ctx, dark) {
                self.applied_dark = Some(dark);
            }
        }
        self.handle_close_request(ctx);
        self.maybe_autosave(false);
        // Rare support prompt — real sessions only, a week apart, opt-out
        // respected. Skipped entirely under test so suites never touch the
        // user's prefs file.
        #[cfg(not(test))]
        if !self.show_coffee && !self.state.prefs.coffee_optout {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if self.state.prefs.coffee_last_epoch == 0 {
                // First launch: start the one-week grace period.
                self.state.prefs.coffee_last_epoch = now;
                self.state.prefs.save();
            } else if crate::prefs::coffee_prompt_due(
                self.state.session_analyses,
                self.state.prefs.coffee_last_epoch,
                self.state.prefs.coffee_optout,
                now,
            ) {
                self.show_coffee = true;
                self.state.prefs.coffee_last_epoch = now;
                self.state.prefs.save();
            }
        }
        self.handle_shortcuts(ctx);
        // Live what-if: any edit that marks the analysis stale recomputes on
        // the next frame (never mid-drag; F5 stays as the manual trigger).
        if self.state.prefs.auto_analyze
            && self.state.analysis_stale
            && self.state.dragging_node.is_none()
            && !self.state.project.pipes.is_empty()
        {
            self.state.run_analysis();
            self.state.update_inlet_check();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            self.state.window_title().into(),
        ));

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| self.file_menu(ui, ctx));
                ui.menu_button("Edit", |ui| self.edit_menu(ui));
                ui.menu_button("Tools", |ui| self.tools_menu(ui));
                ui.menu_button("View", |ui| self.view_menu(ui, ctx));
                ui.menu_button("Help", |ui| self.help_menu(ui));
                ui.separator();
                ui.label(self.state.project.name.clone());
            });
        });

        egui::TopBottomPanel::top("toolbar")
            .exact_height(32.0)
            .show(ctx, |ui| draw_toolbar(ui, &mut self.state, self.canvas_rect));

        self.draw_close_confirm(ctx);
        self.draw_recovery_prompt(ctx);
        self.draw_bg_scale_dialog(ctx);
        self.draw_coffee_prompt(ctx);
        draw_help_window(ctx, &mut self.state.help);
        draw_global_edit_window(ctx, &mut self.state);
        draw_report_editor_window(ctx, &mut self.state);
        draw_tc_calc_window(ctx, &mut self.state);
        files::draw_noaa_paste_window(ctx, &mut self.state);
        files::draw_report_options_window(ctx, &mut self.state);
        tutorial::draw_tutorial(ctx, &mut self.state);

        if self.show_about {
            egui::Window::new("About StormSewer")
                .collapsible(false)
                .resizable(false)
                .default_pos(ctx.screen_rect().center() - egui::vec2(170.0, 90.0))
                .movable(true)
                .show(ctx, |ui| {
                    ui.heading("StormSewer v0.9");
                    ui.label("Standalone storm sewer design desktop application.");
                    ui.label("Rational method hydrology, Manning hydraulics, HGL backwater.");
                    ui.label("HEC-22 inlet analysis, DXF/LandXML exchange, PDF/HTML reports.");
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label("Free and open source. If it helped, you can");
                    ui.add_space(4.0);
                    coffee_button(ui);
                    ui.add_space(8.0);
                    ui.label(
                        "Need a feature, a DOT report template, or help                          fitting StormSewer into your firm's workflow?",
                    );
                    ui.hyperlink_to(
                        "support@hydrocomplete.com — support & custom work",
                        "mailto:support@hydrocomplete.com?subject=StormSewer%20support",
                    );
                    ui.add_space(4.0);
                    ui.hyperlink_to(
                        "hydrocomplete.com — more tools by the same author",
                        "https://hydrocomplete.com",
                    );
                    ui.hyperlink_to(
                        "\u{2615} Buy me a coffee",
                        "https://buy.stripe.com/14A3cudxo91z1qo0OHdAk00?client_reference_id=stormsewer-app",
                    );
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

                // Right-click a structure or pipe to open a context menu on it.
                if resp.secondary_clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let (wx, wy) = self.state.viewport.screen_to_world(rect, pos);
                        self.state.edit.context_target =
                            if let Some(i) = snap_node(&self.state.project, wx, wy, SNAP_RADIUS) {
                                self.state.set_selection(Some(i), None, None);
                                Some(ContextTarget::Node(i))
                            } else if let Some(i) = snap_pipe(&self.state.project, wx, wy, SNAP_RADIUS)
                            {
                                self.state.set_selection(None, Some(i), None);
                                Some(ContextTarget::Pipe { idx: i, x: wx, y: wy })
                            } else {
                                Some(ContextTarget::Empty { x: wx, y: wy })
                            };
                    }
                }
                resp.context_menu(|ui| draw_context_menu(ui, &mut self.state));
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
                let shift = ui.input(|i| i.modifiers.shift);
                let ctrl = ui.input(|i| i.modifiers.ctrl);
                if self.state.bg_calibrate.active {
                    // Calibration owns the canvas until done or cancelled.
                    self.state.bg_calibration_click(wx, wy);
                } else if ctrl && self.state.edit.tool == Tool::Select {
                    // Ctrl-click builds the multi-selection for deletion.
                    let node = snap_node(&self.state.project, wx, wy, SNAP_RADIUS);
                    let pipe = if node.is_none() {
                        snap_pipe(&self.state.project, wx, wy, SNAP_RADIUS)
                    } else {
                        None
                    };
                    if node.is_some() || pipe.is_some() {
                        self.state.toggle_multi(node, pipe);
                    }
                } else if shift && self.state.edit.tool == Tool::Select {
                    // Shift-click builds the profile run; it never changes
                    // the ordinary selection and is not an undo-able edit.
                    if let Some(pidx) =
                        snap_pipe(&self.state.project, wx, wy, SNAP_RADIUS)
                    {
                        let id = self.state.project.pipes[pidx].id.clone();
                        self.state.toggle_profile_pipe(&id);
                    }
                } else if self.state.edit.tool == Tool::DrawCatchment {
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

            if self.state.bg_calibrate.active && self.state.view_tab == ViewTab::Plan {
                let painter = ui.painter_at(rect);
                let accent = egui::Color32::from_rgb(224, 86, 127);
                let mut pts = vec![];
                if let Some(a) = self.state.bg_calibrate.point_a {
                    pts.push(self.state.viewport.world_to_screen(rect, a.0, a.1));
                }
                if let Some(b) = self.state.bg_calibrate.point_b {
                    pts.push(self.state.viewport.world_to_screen(rect, b.0, b.1));
                } else if let (Some(a), Some(hover)) =
                    (self.state.bg_calibrate.point_a, resp.hover_pos())
                {
                    let _ = a;
                    pts.push(hover);
                }
                for p in &pts {
                    painter.circle_stroke(*p, 6.0, egui::Stroke::new(2.0, accent));
                    painter.line_segment(
                        [*p - egui::vec2(9.0, 0.0), *p + egui::vec2(9.0, 0.0)],
                        egui::Stroke::new(1.0, accent),
                    );
                    painter.line_segment(
                        [*p - egui::vec2(0.0, 9.0), *p + egui::vec2(0.0, 9.0)],
                        egui::Stroke::new(1.0, accent),
                    );
                }
                if pts.len() == 2 {
                    painter.line_segment([pts[0], pts[1]], egui::Stroke::new(1.5, accent));
                }
            }

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
                    &self.state.profile_pipes,
                    &self.state.multi_nodes,
                    &self.state.multi_pipes,
                ),
                ViewTab::Profile => draw_profile(
                    ui,
                    rect,
                    &self.state.project,
                    self.state.analysis.as_ref(),
                    &self.state.profile_pipes,
                ),
            }
        });
    }
}

impl eframe::App for StormSewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 860.0])
            .with_title("StormSewer v0.9"),
        ..Default::default()
    };
    eframe::run_native(
        "StormSewer",
        options,
        Box::new(|cc| Ok(Box::new(StormSewerApp::new(cc)))),
    )
}