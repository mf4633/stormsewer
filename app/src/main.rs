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
mod python_term;
mod recent;
mod report_editor;
mod software_gl;
mod state;
mod swmm_backdrop;
// Gap-closing streams (2026-09-18): GIS, 2D, dialogs, scenarios, calibration, live runs.
mod swmm_calib;
mod swmm_gis;
mod swmm_lid;
mod swmm_live;
mod swmm_scenarios;
mod swmm_twod;
mod swmm_browser;
mod swmm_canvas;
mod swmm_compare;
mod swmm_design;
mod swmm_dialogs;
mod swmm_doc;
mod swmm_grids;
mod swmm_import;
mod swmm_layers;
mod swmm_lengths;
mod swmm_menus;
#[cfg(test)]
mod swmm_pane_tests;
mod swmm_props;
mod swmm_panel;
mod swmm_qa;
mod swmm_rain_import;
mod swmm_recovery;
mod swmm_run_panel;
mod swmm_storm;
mod swmm_tools;
mod swmm_units;
// Results views (stream C): profile, map overlay, plots, tables, export.
mod swmm_chart;
mod swmm_export;
mod swmm_profile;
mod swmm_report;
mod swmm_results;
mod swmm_tables;
#[cfg(test)]
mod swmm_ui_tests;
mod tables;
mod tc_calc;
mod theme;
mod toolbar;
mod tutorial;
#[cfg(test)]
mod ui_tests;
mod undo;
mod viewport;

use catchment_draw::handle_catchment_click;
use edit::{
    delete_selection, handle_click, merge_node, nearest_other_node, snap_node, snap_pipe,
    snap_placement, sync_pipe_lengths, ContextTarget, Tool,
};
use eframe::egui::{self, Key, Modifiers, Sense};
use global_edit::draw_global_edit_window;
use help::{draw_help_window, open_help, HelpTopic};
use inspector::draw_inspector;
use menu::draw_context_menu;
use panels::{draw_left_panel, draw_report_panel};
use plan::draw_plan;
use profile::draw_profile;
use report_editor::draw_report_editor_window;
use state::{AppState, ViewTab};
use tc_calc::draw_tc_calc_window;
use toolbar::draw_toolbar;

const SNAP_RADIUS: f64 = 15.0;

/// Window and About-dialog title. Read from the manifest so it cannot drift
/// from the released version, as a hardcoded "v0.8" once did.
fn window_title() -> String {
    format!("StormSewer v{}", env!("CARGO_PKG_VERSION"))
}

/// Support link surfaced in the Help menu and About dialog.
///
/// Stripe, not Buy Me a Coffee: there is no BMAC account, and this constant
/// pointed at a nonexistent one. `client_reference_id` names the surface so
/// the checkout can be attributed.
const SUPPORT_URL: &str =
    "https://buy.stripe.com/14A3cudxo91z1qo0OHdAk00?client_reference_id=stormsewer-app";

/// Warm amber pill for the support button.
const COFFEE_AMBER: egui::Color32 = egui::Color32::from_rgb(255, 221, 0);
const COFFEE_INK: egui::Color32 = egui::Color32::from_rgb(15, 15, 20);

/// Render the "Buy me a coffee" support button. Opens [`SUPPORT_URL`] in the
/// browser when clicked.
fn coffee_button(ui: &mut egui::Ui) {
    let label = egui::RichText::new("☕  Buy me a coffee")
        .color(COFFEE_INK)
        .strong();
    let btn = egui::Button::new(label)
        .fill(COFFEE_AMBER)
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
    /// `--check-renderer`: render this many real frames, then close. Proves a
    /// graphics backend actually works on this machine rather than merely
    /// compiling, which is the one thing the headless suite cannot do.
    selftest_frames: Option<u32>,
    /// `GL_RENDERER` of the Glow context, read on the first self-test frame
    /// (shows whether the bundled Mesa llvmpipe is what actually rendered).
    gl_renderer: Option<String>,
    /// `--screenshot`: draw the SWMM results map, write it to a PNG, and exit.
    /// `None` in normal use.
    screenshot: Option<ScreenshotJob>,
}

/// A pending `--screenshot` capture.
///
/// The window has to be real before the picture is worth anything: fonts
/// install a frame late, the theme settles the frame after that, and the map
/// fits itself only once it knows the canvas rect. So the capture waits a few
/// frames — and for the engine, when `--run` was asked for — before asking
/// egui for the pixels.
#[derive(Clone, Debug, PartialEq)]
struct ScreenshotJob {
    path: std::path::PathBuf,
    /// Run the model first, so the map colours from results instead of
    /// drawing every object unrun.
    run: bool,
    /// Frames still to draw before asking for the image.
    warmup: u32,
    /// The image has been asked for; it arrives on a later frame.
    requested: bool,
    /// Which SWMM view to draw.
    view: CaptureView,
    /// How many images to write, spread across the run's reporting periods.
    /// One means a single image showing the run's peaks, which is what
    /// `--screenshot` has always drawn.
    frames: u32,
    /// How many images have been written so far.
    shot: u32,
    /// Open the model in the editor workspace rather than the results view.
    editor: bool,
    /// One editor dialog to open and photograph, by name.
    dialog: Option<String>,
}

/// Which SWMM view a `--screenshot` capture draws.
///
/// One per `SwmmSubView`: a view that cannot be named here cannot be looked
/// at, and the layout defects this flag exists to find are only visible in
/// pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CaptureView {
    #[default]
    Map,
    Chart,
    Profile,
    Plots,
    Tables,
    Results,
}

/// Arm one of the editor's dialogs so a capture can photograph it.
///
/// The gates are deliberately not uniform, and a flag has to respect that: the
/// 2D, calibration, scenario and live windows are plain bools, while the LID
/// dialogs hold an `Option<Draft>` loaded from the document and so are opened
/// through their own functions. Returns false for a name nothing matches, which
/// the caller reports — a capture that silently draws nothing is worse than no
/// capture, because it looks like evidence.
fn arm_capture_dialog(state: &mut AppState, name: &str) -> bool {
    // Compare and design hang off `AppState`, not the document, so they cannot
    // share the `swmm_doc` borrow the rest take.
    match name {
        "compare" => {
            state.swmm_compare.open = true;
            return true;
        }
        "design" => {
            state.swmm_design.open = true;
            return true;
        }
        _ => {}
    }
    let ed = &mut state.swmm_doc;
    match name {
        "twod-setup" => ed.twod.setup_open = true,
        "twod-interfaces" => ed.twod.interfaces_open = true,
        "twod-sources" => ed.twod.sources_open = true,
        "twod-run" => ed.twod.run_open = true,
        "calib" => ed.calib.open = true,
        "calib-report" => ed.calib.report_open = true,
        "scenarios" => ed.scenarios.open = true,
        "live" => ed.live.open = true,
        "lid-controls" => swmm_lid::open_lid_controls(ed, None),
        "lid-usage" => swmm_lid::open_lid_usage(ed, None),
        _ => return false,
    }
    true
}

/// Every name [`arm_capture_dialog`] accepts, for the usage text and the tests.
const CAPTURE_DIALOGS: [&str; 12] = [
    "twod-setup",
    "twod-interfaces",
    "twod-sources",
    "twod-run",
    "calib",
    "calib-report",
    "scenarios",
    "live",
    "lid-controls",
    "lid-usage",
    "compare",
    "design",
];

/// Where the `n`th image of a `total`-image capture goes.
///
/// A single-image capture keeps the path it was given, so the old behaviour is
/// untouched. A sequence gets a zero-padded index before the extension, which
/// makes the files sort into time order in any file listing.
fn shot_path(base: &std::path::Path, n: u32, total: u32) -> std::path::PathBuf {
    if total <= 1 {
        return base.to_path_buf();
    }
    let stem = base
        .file_stem()
        .map_or_else(|| "shot".to_string(), |s| s.to_string_lossy().into_owned());
    let ext = base
        .extension()
        .map_or_else(|| "png".to_string(), |s| s.to_string_lossy().into_owned());
    base.with_file_name(format!("{stem}-{n:03}.{ext}"))
}

/// The reporting period the `n`th of `total` images should show.
///
/// The first image is the start of the run and the last is its end, so a
/// sequence spans the whole run instead of bunching up at the front.
fn period_for_shot(n: u32, total: u32, n_periods: usize) -> usize {
    if n_periods == 0 || total <= 1 {
        return 0;
    }
    let last = (n_periods - 1) as u64;
    let n = (n as u64).min(total as u64 - 1);
    ((n * last) / (total as u64 - 1)) as usize
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
            selftest_frames: None,
            gl_renderer: None,
            screenshot: None,
        }
    }

    /// Drive a `--screenshot` capture across frames.
    ///
    /// egui hands the pixels back as an input event on a later frame, so this
    /// is a small state machine rather than a call.
    fn drive_screenshot(&mut self, ctx: &egui::Context) {
        let Some(mut job) = self.screenshot.take() else {
            return;
        };

        // The image, once egui has it, arrives as an event.
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let [w, h] = image.size;
            let out = shot_path(&job.path, job.shot, job.frames);
            match image::save_buffer(
                &out,
                image.as_raw(),
                w as u32,
                h as u32,
                image::ExtendedColorType::Rgba8,
            ) {
                Ok(()) => println!("Wrote {} ({w}x{h})", out.display()),
                Err(e) => eprintln!("StormSewer: could not write {}: {e}", out.display()),
            }
            job.shot += 1;
            if job.shot >= job.frames {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            // Move to the next instant, then let the view redraw before asking
            // for pixels again. This function runs after the frame has been
            // composed, so requesting in the same pass would capture the
            // period we just left.
            let period = period_for_shot(job.shot, job.frames, self.state.swmm.n_periods());
            self.state.swmm.set_period(period);
            job.requested = false;
            job.warmup = 2;
            ctx.request_repaint();
            self.screenshot = Some(job);
            return;
        }

        // A run happens on a worker thread; collect it before deciding.
        self.state.swmm.poll();
        // A finished run opens the Run Status window over the map. That is
        // right for a person and wrong for a picture of the map, so the
        // screenshot path closes it again.
        self.state.swmm_doc.run_panel.open = false;
        if job.run && self.state.swmm.is_running() {
            ctx.request_repaint();
            self.screenshot = Some(job);
            return;
        }

        // A sequence opens on its first instant. Done here rather than beside
        // the request below so the warmup frames draw the period we are about
        // to photograph.
        if job.frames > 1 && self.state.swmm.frame().is_none() && self.state.swmm.n_periods() > 0 {
            let period = period_for_shot(0, job.frames, self.state.swmm.n_periods());
            self.state.swmm.set_period(period);
        }

        if job.warmup > 0 {
            job.warmup -= 1;
        } else if !job.requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            job.requested = true;
        }
        ctx.request_repaint();
        self.screenshot = Some(job);
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
            selftest_frames: None,
            gl_renderer: None,
            screenshot: None,
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
                .is_none_or(|t| t.elapsed().as_secs() >= 60);
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
                self.state.status = "Recovered unsaved work — use Save Project… to keep it".into();
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
                        match self.state.bg_calibrate.distance_text.trim().parse::<f64>() {
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
        if (self.state.project_dirty || self.state.swmm_doc.dirty()) && !self.allow_close {
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
                if self.state.project_dirty {
                    ui.label(format!(
                        "\"{}\" has unsaved changes.",
                        self.state.project.name
                    ));
                }
                if self.state.swmm_doc.dirty() {
                    ui.label(format!(
                        "SWMM model \"{}\" has unsaved changes.",
                        self.state.swmm_doc.file_name()
                    ));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save project…").clicked() {
                        if self.state.project_dirty {
                            self.state.pick_save_project();
                        }
                        if self.state.swmm_doc.dirty() {
                            swmm_menus::save_model(&mut self.state);
                        }
                        if !self.state.project_dirty && !self.state.swmm_doc.dirty() {
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
        ui.separator();
        if ui.button("SWMM Runner…").clicked() {
            self.state.side_tab = panels::SideTab::Swmm;
            ui.close_menu();
        }
        if ui.button("Python Terminal…").clicked() {
            self.state.python_term.open = true;
            ui.close_menu();
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
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
        if ui
            .selectable_label(self.state.view_tab == ViewTab::Plan, "Plan")
            .clicked()
        {
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
        if ui
            .selectable_label(self.state.view_tab == ViewTab::Swmm, "SWMM Results")
            .clicked()
        {
            self.state.view_tab = ViewTab::Swmm;
            ui.close_menu();
        }
        if ui
            .button("SWMM Model Editor")
            .on_hover_text("Draw and edit an EPA SWMM model; the design view stays one click away")
            .clicked()
        {
            swmm_menus::enter_workspace(&mut self.state);
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
        swmm_run_panel::help_menu_item(ui, &mut self.state);
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
        if self.state.swmm_doc.active {
            // The SWMM editor has its own bindings; Ctrl+Z there goes to
            // the document's history, not the storm-sewer snapshot stack.
            swmm_menus::handle_shortcuts(ctx, &mut self.state);
            return;
        }
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
                if !self.state.multi_nodes.is_empty() || !self.state.multi_pipes.is_empty() {
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
                } else if !self.state.multi_nodes.is_empty() || !self.state.multi_pipes.is_empty() {
                    self.state.multi_nodes.clear();
                    self.state.multi_pipes.clear();
                    self.state.status = "Selection cleared".into();
                } else if !self.state.profile_pipes.is_empty() {
                    self.state.profile_pipes.clear();
                    self.state.status = "Profile run cleared — Profile shows the main trunk".into();
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
        self.state.sync_swmm_editor();
        // A SWMM run is carried out on a worker thread. Collect it here, and
        // keep the frame clock running while one is in flight — otherwise the
        // window sits frozen until the user happens to move the mouse.
        if self.state.swmm.poll() {
            self.state.status = self.state.swmm.status_line();
        }
        if self.state.swmm.is_running() {
            ctx.request_repaint();
        }
        // Map playback advances on the frame clock, so it needs the clock kept
        // running the same way a live run does.
        if self.state.swmm.playing {
            self.state
                .swmm
                .advance_playback(ctx.input(|i| i.stable_dt));
            ctx.request_repaint();
        }
        // The Python kernel answers on its own thread as well. Poll first —
        // `||` would skip it once the kernel went idle.
        if self.state.python_term.poll() || self.state.python_term.is_busy() {
            ctx.request_repaint();
        }
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
            self.state.window_title(),
        ));

        let swmm_workspace = self.state.swmm_doc.active;
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                if swmm_workspace {
                    swmm_menus::draw_menus(ui, ctx, &mut self.state, self.canvas_rect);
                    ui.menu_button("Help", |ui| self.help_menu(ui));
                    ui.separator();
                    ui.label(self.state.swmm_doc.file_name());
                    swmm_menus::workspace_switch(ui, &mut self.state);
                } else {
                    ui.menu_button("File", |ui| self.file_menu(ui, ctx));
                    ui.menu_button("Edit", |ui| self.edit_menu(ui));
                    ui.menu_button("Tools", |ui| self.tools_menu(ui));
                    ui.menu_button("View", |ui| self.view_menu(ui, ctx));
                    ui.menu_button("Help", |ui| self.help_menu(ui));
                    ui.separator();
                    ui.label(self.state.project.name.clone());
                }
            });
        });

        egui::TopBottomPanel::top("toolbar")
            .exact_height(32.0)
            .show(ctx, |ui| {
                if swmm_workspace {
                    swmm_menus::draw_toolbar(ui, &mut self.state)
                } else {
                    draw_toolbar(ui, &mut self.state, self.canvas_rect)
                }
            });

        self.draw_close_confirm(ctx);
        swmm_menus::draw_dialogs(ctx, &mut self.state);
        swmm_design::draw_windows(ctx, &mut self.state);
        swmm_report::draw_window(ctx, &mut self.state);
        swmm_compare::draw_windows(ctx, &mut self.state);
        swmm_run_panel::draw_windows(ctx, &mut self.state);
        swmm_qa::draw(ctx, &mut self.state);
        swmm_units::draw(ctx, &mut self.state);
        swmm_recovery::per_frame(ctx, &mut self.state);
        swmm_recovery::draw(ctx, &mut self.state);
        self.draw_recovery_prompt(ctx);
        self.draw_bg_scale_dialog(ctx);
        self.draw_coffee_prompt(ctx);
        draw_help_window(ctx, &mut self.state.help);
        draw_global_edit_window(ctx, &mut self.state);
        draw_report_editor_window(ctx, &mut self.state);
        draw_tc_calc_window(ctx, &mut self.state);
        python_term::draw_python_terminal_window(ctx, &mut self.state);
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
                    ui.heading(window_title());
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

        if swmm_workspace {
            egui::SidePanel::left("swmm-project")
                .default_width(240.0)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| swmm_menus::draw_left_panel(ui, &mut self.state));
                });
            egui::SidePanel::right("swmm-properties")
                .default_width(300.0)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| swmm_menus::draw_properties_panel(ui, &mut self.state));
                });
            egui::TopBottomPanel::bottom("swmm-status")
                .exact_height(24.0)
                .show(ctx, |ui| swmm_canvas::draw_status_bar(ui, &self.state));
            egui::TopBottomPanel::bottom("swmm-findings")
                .resizable(false)
                .show(ctx, |ui| swmm_canvas::draw_findings_strip(ui, &mut self.state));
        } else {
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
                .default_height(if self.state.has_selection() {
                    160.0
                } else {
                    72.0
                })
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
        }

        if self.state.pending_zoom_fit && self.canvas_rect != egui::Rect::NOTHING {
            self.state
                .viewport
                .zoom_to_fit(self.canvas_rect, &self.state.project);
            self.state.pending_zoom_fit = false;
        }
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

            if swmm_workspace {
                // The editor owns the canvas: its own tools, viewport, and
                // context menu. Other SWMM views (chart, and whatever the
                // results side adds) draw as they do in the design view.
                if self.state.swmm.sub_view == swmm_panel::SwmmSubView::Map {
                    swmm_canvas::canvas(ui, rect, &resp, &mut self.state);
                } else {
                    swmm_panel::draw_swmm_view(ui, rect, &mut self.state);
                }
                return;
            }

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
                        self.state.edit.context_target = if let Some(i) =
                            snap_node(&self.state.project, wx, wy, SNAP_RADIUS)
                        {
                            self.state.set_selection(Some(i), None, None);
                            Some(ContextTarget::Node(i))
                        } else if let Some(i) = snap_pipe(&self.state.project, wx, wy, SNAP_RADIUS)
                        {
                            self.state.set_selection(None, Some(i), None);
                            Some(ContextTarget::Pipe {
                                idx: i,
                                x: wx,
                                y: wy,
                            })
                        } else {
                            Some(ContextTarget::Empty { x: wx, y: wy })
                        };
                    }
                }
                resp.context_menu(|ui| draw_context_menu(ui, &mut self.state));
            }

            // The SWMM map keeps its own viewport, so the drag has to be routed
            // by view. Sending it to the plan viewport would pan a drawing the
            // user cannot see while the map sat still under the cursor.
            let on_swmm_map = self.state.view_tab == ViewTab::Swmm
                && self.state.swmm.sub_view == swmm_panel::SwmmSubView::Map;
            if on_swmm_map {
                self.state.swmm.map_viewport.handle_pan_zoom(&resp, ui);
            } else if self.state.dragging_node.is_none() {
                self.state.viewport.handle_pan_zoom(&resp, ui);
            }

            // Clicking the map picks what the chart plots.
            if on_swmm_map && resp.clicked() {
                let pos = resp.interact_pointer_pos().unwrap_or(egui::Pos2::ZERO);
                swmm_panel::map_click(&mut self.state, rect, pos);
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
                    if let Some(pidx) = snap_pipe(&self.state.project, wx, wy, SNAP_RADIUS) {
                        let id = self.state.project.pipes[pidx].id.clone();
                        self.state.toggle_profile_pipe(&id);
                    }
                } else if self.state.edit.tool == Tool::DrawCatchment {
                    let closing = self.state.edit.catchment_vertices.len() >= 3 && {
                        let (fx, fy) = self.state.edit.catchment_vertices[0];
                        let dx = wx - fx;
                        let dy = wy - fy;
                        (dx * dx + dy * dy).sqrt() <= 20.0
                    };
                    if closing {
                        self.state.checkpoint_undo();
                    }
                    if let Some(msg) = handle_catchment_click(
                        &mut self.state.project,
                        &mut self.state.edit,
                        wx,
                        wy,
                    ) {
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
                self.state.project.nodes.get(idx).and_then(|n| {
                    nearest_other_node(&self.state.project, n.x, n.y, SNAP_RADIUS, idx)
                })
            } else if self.state.view_tab == ViewTab::Plan && self.state.edit.tool == Tool::DrawPipe
            {
                hover_world.and_then(|(wx, wy)| snap_node(&self.state.project, wx, wy, SNAP_RADIUS))
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
                    painter.circle_stroke(*p, 6.0, egui::Stroke::new(2.0_f32, accent));
                    painter.line_segment(
                        [*p - egui::vec2(9.0, 0.0), *p + egui::vec2(9.0, 0.0)],
                        egui::Stroke::new(1.0_f32, accent),
                    );
                    painter.line_segment(
                        [*p - egui::vec2(0.0, 9.0), *p + egui::vec2(0.0, 9.0)],
                        egui::Stroke::new(1.0_f32, accent),
                    );
                }
                if pts.len() == 2 {
                    painter.line_segment([pts[0], pts[1]], egui::Stroke::new(1.5_f32, accent));
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
                ViewTab::Swmm => swmm_panel::draw_swmm_view(ui, rect, &mut self.state),
            }
        });
    }
}

impl eframe::App for StormSewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Files dropped onto the window open like a double-click would: a
        // model in the editor, a project or drawing in the design view.
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for path in dropped {
            self.state.open_any_path(ctx, path);
        }
        self.ui(ctx);
        self.drive_screenshot(ctx);
        if let Some(left) = self.selftest_frames {
            if self.gl_renderer.is_none() {
                if let Some(gl) = frame.gl() {
                    use eframe::glow::HasContext as _;
                    // SAFETY: a live GL context owned by eframe for this frame.
                    let name = unsafe { gl.get_parameter_string(eframe::glow::RENDERER) };
                    let renderer = if name.is_empty() {
                        "unknown".to_string()
                    } else {
                        name
                    };
                    println!("OpenGL renderer: {renderer}");
                    self.gl_renderer = Some(renderer);
                }
            }
            // Draw a few frames first: fonts install one pass late, and the
            // theme settles the pass after that.
            if left == 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.selftest_frames = Some(left - 1);
                ctx.request_repaint();
            }
        }
    }
}

fn native_options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 860.0])
            .with_title(window_title()),
        renderer,
        ..Default::default()
    }
}

/// Command-line surface. Deliberately tiny: this is a GUI program, and the
/// only reason it takes arguments is so a machine can ask it whether it can
/// actually start.
const USAGE: &str = "StormSewer — storm sewer design and analysis

USAGE:
    StormSewer [OPTIONS] [FILE]

ARGS:
    FILE                A file to open at launch: a .ssproj project, an EPA
                        SWMM .inp model, a Hydraflow / Civil 3D .stm, a
                        LandXML .xml, or a .dxf (a network exported by
                        StormSewer, else a site underlay).

OPTIONS:
    --check-renderer    Start a graphics backend, draw real frames, then exit.
                        Prints which renderer worked (and the OpenGL renderer
                        string). Exit code 0 means StormSewer can run on this
                        machine; 1 means it cannot, and the reason is printed.
                        Useful on remote desktop, virtual desktops, and VMs.

    --screenshot FILE   Open the model given as FILE, draw the SWMM results
                        map, write it to this PNG, and exit. Needs a display.
    --run               With --screenshot, run the model through the engine
                        first, so the map is coloured by results rather than
                        drawn unrun.
    --view WHICH        With --screenshot, which SWMM view to draw: `map` (the
                        default), `chart`, `profile` (the long-section with the
                        HGL), `plots`, `tables` (the .rpt summaries), or
                        `results` (the map coloured by any variable). Anything
                        else falls back to the map. `tables` needs --run, since
                        without a run there are no summaries to show.
    --frames N          With --screenshot, write N images spread evenly across
                        the run's reporting periods instead of one, named
                        FILE-000.png, FILE-001.png and so on. Needs --run,
                        since without results there are no periods to step.
    --editor            With --screenshot, open FILE in the SWMM model editor
                        workspace instead of the results view, so the editor
                        canvas and its overlays are what gets drawn.
    --dialog NAME       With --screenshot, open one editor dialog and draw it:
                        twod-setup, twod-interfaces, twod-sources, twod-run,
                        calib, scenarios, live, lid-controls, lid-usage.
                        Implies --editor and needs FILE: every one of these
                        gates on a loaded model and draws nothing without one.

ENVIRONMENT:
    STORMSEWER_SOFTWARE_GL=1
                        Skip the GPU and use the bundled Mesa llvmpipe software
                        OpenGL (Windows installer builds only). StormSewer does
                        this by itself when no hardware backend starts.
    --version           Print the version and exit.
    --help, -h          Print this message and exit.
";

/// The parsed command line.
#[derive(Clone, Debug, PartialEq)]
struct Cli {
    open: Option<std::path::PathBuf>,
    screenshot: Option<std::path::PathBuf>,
    run: bool,
    view: CaptureView,
    frames: u32,
    editor: bool,
    /// Named `dialog`, not `open`: `open` above is the model path.
    dialog: Option<String>,
}

impl Default for Cli {
    fn default() -> Self {
        // `frames` is a count, not a flag: the derived zero would mean "write
        // no images at all", which is never what anyone asked for.
        Self {
            open: None,
            screenshot: None,
            run: false,
            view: CaptureView::Map,
            frames: 1,
            editor: false,
            dialog: None,
        }
    }
}

/// Parse the argument list.
///
/// This walks the arguments rather than picking "the first one that does not
/// start with `--`": `--screenshot` takes the next argument as its value, and
/// the simpler rule would open the PNG path as the model.
fn parse_cli(args: &[String]) -> Cli {
    let mut cli = Cli::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--screenshot" => cli.screenshot = it.next().map(std::path::PathBuf::from),
            "--run" => cli.run = true,
            "--view" => {
                if let Some(v) = it.next() {
                    cli.view = match v.as_str() {
                        "chart" => CaptureView::Chart,
                        "profile" => CaptureView::Profile,
                        "plots" => CaptureView::Plots,
                        "tables" => CaptureView::Tables,
                        "results" => CaptureView::Results,
                        _ => CaptureView::Map,
                    };
                }
            }
            // A count that will not parse falls back to one image rather than
            // none, so a typo still produces something to look at.
            "--frames" => {
                if let Some(n) = it.next() {
                    cli.frames = n.parse().unwrap_or(1).max(1);
                }
            }
            "--editor" => cli.editor = true,
            "--dialog" => cli.dialog = it.next().cloned(),
            _ if a.starts_with("--") => {}
            _ if cli.open.is_none() => cli.open = Some(std::path::PathBuf::from(a)),
            _ => {}
        }
    }
    cli
}


/// Start the window, trying each renderer in turn.
///
/// wgpu comes first because it reaches Direct3D 12 — and, failing that, a
/// software adapter — on machines with no usable OpenGL driver: remote desktop
/// and Citrix/VDI sessions, plain VMs, and validation sandboxes. OpenGL is the
/// fallback rather than the default because it is the one that goes missing.
fn run(
    selftest_frames: Option<u32>,
    open_path: Option<std::path::PathBuf>,
    screenshot: Option<ScreenshotJob>,
    renderers: &[eframe::Renderer],
) -> Result<eframe::Renderer, Vec<(eframe::Renderer, String)>> {
    let mut failures = Vec::new();
    for &renderer in renderers {
        match eframe::run_native(
            "StormSewer",
            native_options(renderer),
            Box::new({
                let open_path = open_path.clone();
                let screenshot = screenshot.clone();
                move |cc| {
                    let mut app = StormSewerApp::new(cc);
                    app.selftest_frames = selftest_frames;
                    match (screenshot.clone(), open_path.clone()) {
                        // A results-view screenshot deliberately skips
                        // open_any_path: a .inp there enters the model editor,
                        // whose canvas is a different view from this one.
                        // `--editor` asks for that editor on purpose.
                        (Some(job), path) => {
                            app.state.view_tab = ViewTab::Swmm;
                            app.state.tutorial.open = false;
                            let wants_editor = job.editor || job.dialog.is_some();
                            if let Some(p) = path {
                                match stormsewer_swmm::inp::InpModel::read(&p) {
                                    Ok(model) => {
                                        app.state.swmm.model_inp = Some(model);
                                        app.state.swmm.pending_map_fit = true;
                                    }
                                    Err(e) => {
                                        eprintln!("StormSewer: {}: {e}", p.display());
                                    }
                                }
                                app.state.swmm.model = Some(p.clone());
                                // The document is opened *before* entering the
                                // workspace: enter_workspace substitutes a blank
                                // model when nothing is loaded, and a picture of
                                // a blank model is not evidence of anything.
                                if wants_editor {
                                    if let Err(e) = app.state.swmm_doc.open_path(&p) {
                                        eprintln!("StormSewer: {}: {e}", p.display());
                                    }
                                    swmm_menus::enter_workspace(&mut app.state);
                                }
                            } else if wants_editor {
                                eprintln!(
                                    "StormSewer: --editor/--dialog need a model FILE; \
                                     the editor's dialogs draw nothing unloaded"
                                );
                            }
                            // After enter_workspace, which forces the map view
                            // of its own accord and would otherwise silently
                            // override --view.
                            app.state.swmm.sub_view = match job.view {
                                CaptureView::Map => swmm_panel::SwmmSubView::Map,
                                CaptureView::Chart => swmm_panel::SwmmSubView::Chart,
                                CaptureView::Profile => swmm_panel::SwmmSubView::Profile,
                                CaptureView::Plots => swmm_panel::SwmmSubView::Plots,
                                CaptureView::Tables => swmm_panel::SwmmSubView::Tables,
                                CaptureView::Results => swmm_panel::SwmmSubView::Results,
                            };
                            if let Some(name) = job.dialog.as_deref() {
                                if !arm_capture_dialog(&mut app.state, name) {
                                    eprintln!(
                                        "StormSewer: unknown --dialog {name}; known: {}",
                                        CAPTURE_DIALOGS.join(", ")
                                    );
                                }
                            }
                            if job.run {
                                app.state.swmm.ensure_discovered();
                                app.state.swmm.start_run();
                            }
                            app.screenshot = Some(job);
                        }
                        (None, Some(path)) => {
                            app.state.open_any_path(&cc.egui_ctx, path);
                            // A file on the command line is a returning user,
                            // not a first launch: no tutorial over their
                            // network.
                            app.state.tutorial.open = false;
                        }
                        (None, None) => {}
                    }
                    Ok(Box::new(app))
                }
            }),
        ) {
            Ok(()) => return Ok(renderer),
            Err(e) => {
                eprintln!("StormSewer: {renderer:?} renderer failed: {e}");
                failures.push((renderer, e.to_string()));
            }
        }
    }
    Err(failures)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("{}", window_title());
        return;
    }
    // `--check-renderer` runs the real application — real fonts, real theme,
    // real canvas — for a handful of frames and then closes itself.
    let selftest = args.iter().any(|a| a == "--check-renderer");
    let frames = selftest.then_some(5);
    if selftest {
        // eframe logs the adapters it enumerated and why it rejected them.
        // Without a logger installed that diagnosis is simply discarded.
        env_logger::Builder::new()
            // Warn everywhere, except the two modules that report which
            // graphics adapters exist and why each was rejected — the whole
            // point of running this.
            .filter_level(log::LevelFilter::Warn)
            .filter_module("egui_wgpu", log::LevelFilter::Info)
            .filter_module("eframe", log::LevelFilter::Info)
            .format_timestamp(None)
            .init();
    }

    let cli = parse_cli(&args);
    let open_path = cli.open.clone();
    let screenshot = cli.screenshot.clone().map(|path| ScreenshotJob {
        path,
        run: cli.run,
        warmup: 6,
        requested: false,
        view: cli.view,
        frames: cli.frames,
        shot: 0,
        editor: cli.editor,
        dialog: cli.dialog.clone(),
    });

    // Software OpenGL. The fallback process is the copy of this executable
    // in the `mesa` folder (see software_gl.rs); `STORMSEWER_SOFTWARE_GL=1`
    // on the main executable goes straight to it without trying the GPU.
    let software = software_gl::running_from_bundle();
    if !software && software_gl::requested() {
        match software_gl::reexec(&args) {
            Some(status) => std::process::exit(status.code().unwrap_or(1)),
            None => {
                eprintln!(
                    "StormSewer: {} set but this build has no bundled Mesa",
                    software_gl::ENV
                );
                std::process::exit(1);
            }
        }
    }
    let renderers: &[eframe::Renderer] = if software {
        match software_gl::activate() {
            Ok(dll) => {
                if selftest {
                    println!("Using bundled Mesa: {}", dll.display());
                }
            }
            Err(e) => {
                eprintln!("StormSewer: software OpenGL unavailable: {e}");
                std::process::exit(1);
            }
        }
        &[eframe::Renderer::Glow]
    } else {
        &[eframe::Renderer::Wgpu, eframe::Renderer::Glow]
    };

    match run(frames, open_path, screenshot, renderers) {
        Ok(renderer) if selftest => {
            let how = if software {
                " (software OpenGL, bundled Mesa llvmpipe)"
            } else {
                ""
            };
            println!(
                "StormSewer {} started with the {renderer:?} renderer{how}.",
                env!("CARGO_PKG_VERSION")
            );
        }
        Ok(_) => (),
        Err(failures) => {
            // No hardware backend. If this build ships Mesa, try once more in
            // a child process that loads it (see software_gl.rs for why a
            // child and not this process).
            if !software {
                if let Some(dir) = software_gl::bundled_dir() {
                    if selftest {
                        eprintln!(
                            "No hardware graphics backend; retrying with the bundled software OpenGL in {}",
                            dir.display()
                        );
                    }
                    if let Some(status) = software_gl::reexec(&args) {
                        std::process::exit(status.code().unwrap_or(1));
                    }
                }
            }
            if selftest {
                eprintln!(
                    "StormSewer cannot start on this machine — no graphics backend available."
                );
                for (r, e) in &failures {
                    eprintln!("  {r:?}: {e}");
                }
                std::process::exit(1);
            }
            main_no_backend(failures);
        }
    }
}

/// Nothing could start: explain it in a window, because a GUI launch has no
/// console to read.
fn main_no_backend(failures: Vec<(eframe::Renderer, String)>) -> ! {
    let detail = failures
        .iter()
        .map(|(r, e)| format!("{r:?}: {e}"))
        .collect::<Vec<_>>()
        .join(
            "

",
        );
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("StormSewer could not start")
        .set_description(format!(
            "No graphics backend was available.

This usually means the              machine has no GPU driver that StormSewer can use — common on              remote desktop, virtual desktop (Citrix/VDI), and plain virtual              machines.

The browser version needs no graphics driver:              https://mf4633.github.io/stormsewer/

Details:
{detail}"
        ))
        .show();
    std::process::exit(1);
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The mistake this parser exists to avoid: `--screenshot` takes a value,
    /// so a "first non-flag argument" rule opens the PNG as the model.
    #[test]
    fn the_screenshot_path_is_not_mistaken_for_the_model() {
        let cli = parse_cli(&args(&["--screenshot", "out.png", "model.inp"]));
        assert_eq!(cli.screenshot, Some("out.png".into()));
        assert_eq!(cli.open, Some("model.inp".into()));
    }

    #[test]
    fn the_model_may_come_first() {
        let cli = parse_cli(&args(&["model.inp", "--screenshot", "out.png", "--run"]));
        assert_eq!(cli.open, Some("model.inp".into()));
        assert_eq!(cli.screenshot, Some("out.png".into()));
        assert!(cli.run);
    }

    #[test]
    fn a_bare_model_still_opens() {
        let cli = parse_cli(&args(&["model.inp"]));
        assert_eq!(cli.open, Some("model.inp".into()));
        assert!(cli.screenshot.is_none());
        assert!(!cli.run);
    }

    #[test]
    fn other_flags_are_ignored_and_the_first_file_wins() {
        let cli = parse_cli(&args(&["--check-renderer", "a.inp", "b.inp"]));
        assert_eq!(cli.open, Some("a.inp".into()));
        assert!(!cli.run);
    }

    /// `--screenshot` with nothing after it must not panic or eat a file.
    #[test]
    fn a_dangling_screenshot_flag_is_harmless() {
        let cli = parse_cli(&args(&["--screenshot"]));
        assert!(cli.screenshot.is_none());
        assert!(cli.open.is_none());
    }

    #[test]
    fn the_view_defaults_to_the_map_and_one_image() {
        let cli = parse_cli(&args(&["--screenshot", "out.png", "m.inp"]));
        assert_eq!(cli.view, CaptureView::Map);
        assert_eq!(cli.frames, 1);
    }

    #[test]
    fn the_profile_view_can_be_asked_for() {
        let cli = parse_cli(&args(&["--screenshot", "o.png", "--view", "profile"]));
        assert_eq!(cli.view, CaptureView::Profile);
    }

    /// Every sub-view the SWMM tab can show must be nameable, or the capture
    /// tool cannot be pointed at it — and a view nobody can photograph is a
    /// view whose layout nobody has checked.
    #[test]
    fn every_sub_view_is_reachable_by_name() {
        for (name, want) in [
            ("map", CaptureView::Map),
            ("chart", CaptureView::Chart),
            ("profile", CaptureView::Profile),
            ("plots", CaptureView::Plots),
            ("tables", CaptureView::Tables),
            ("results", CaptureView::Results),
        ] {
            assert_eq!(parse_cli(&args(&["--view", name])).view, want, "--view {name}");
        }
    }

    #[test]
    fn the_editor_flag_is_off_unless_asked_for() {
        assert!(!parse_cli(&args(&["--screenshot", "o.png", "m.inp"])).editor);
        assert!(parse_cli(&args(&["--screenshot", "o.png", "--editor", "m.inp"])).editor);
    }

    #[test]
    fn a_dialog_is_read_by_name() {
        let cli = parse_cli(&args(&["--dialog", "twod-setup", "m.inp"]));
        assert_eq!(cli.dialog.as_deref(), Some("twod-setup"));
        assert_eq!(cli.open, Some("m.inp".into()));
    }

    /// `--dialog` takes a value, so the value must not be read as the model,
    /// the same trap `--screenshot` set.
    #[test]
    fn a_dialog_name_is_not_mistaken_for_the_model() {
        let cli = parse_cli(&args(&["--dialog", "calib"]));
        assert_eq!(cli.dialog.as_deref(), Some("calib"));
        assert!(cli.open.is_none());
    }

    #[test]
    fn a_dangling_dialog_flag_is_harmless() {
        let cli = parse_cli(&args(&["--dialog"]));
        assert!(cli.dialog.is_none());
        assert!(cli.open.is_none());
    }

    /// The usage text and the dispatch must not drift. A name offered in
    /// --help but missing from arm_capture_dialog would be caught only at
    /// capture time, by a window that never appears — which looks exactly
    /// like a view with nothing wrong with it.
    #[test]
    fn every_advertised_dialog_is_actually_armed() {
        for name in CAPTURE_DIALOGS {
            let mut state = AppState::new_empty();
            assert!(
                arm_capture_dialog(&mut state, name),
                "{name} is advertised in --help but not armed"
            );
        }
        let mut state = AppState::new_empty();
        assert!(!arm_capture_dialog(&mut state, "no-such-dialog"));
    }

    /// An unknown view falls back to the map rather than refusing to draw.
    #[test]
    fn an_unknown_view_falls_back_to_the_map() {
        let cli = parse_cli(&args(&["--view", "elevation"]));
        assert_eq!(cli.view, CaptureView::Map);
    }

    #[test]
    fn a_frame_count_is_read_and_never_zero() {
        assert_eq!(parse_cli(&args(&["--frames", "12"])).frames, 12);
        // Zero images is never what was meant, and neither is a typo.
        assert_eq!(parse_cli(&args(&["--frames", "0"])).frames, 1);
        assert_eq!(parse_cli(&args(&["--frames", "lots"])).frames, 1);
        assert_eq!(parse_cli(&args(&["--frames"])).frames, 1);
    }

    /// `--view` and `--frames` take values, so neither may be read as the model.
    #[test]
    fn flag_values_are_not_mistaken_for_the_model() {
        let cli = parse_cli(&args(&[
            "--view", "profile", "--frames", "4", "model.inp",
        ]));
        assert_eq!(cli.open, Some("model.inp".into()));
        assert_eq!(cli.frames, 4);
    }

    #[test]
    fn a_single_image_keeps_the_path_it_was_given() {
        let p = std::path::Path::new("shots/out.png");
        assert_eq!(shot_path(p, 0, 1), std::path::PathBuf::from("shots/out.png"));
    }

    /// A sequence indexes before the extension, zero-padded so it sorts.
    #[test]
    fn a_sequence_is_numbered_in_time_order() {
        let p = std::path::Path::new("shots/out.png");
        assert_eq!(shot_path(p, 0, 5), std::path::PathBuf::from("shots/out-000.png"));
        assert_eq!(shot_path(p, 12, 20), std::path::PathBuf::from("shots/out-012.png"));
    }

    /// The span must reach both ends: the first image is the start of the run
    /// and the last is its end, or the animation is not evidence of anything.
    #[test]
    fn a_sequence_spans_the_whole_run() {
        assert_eq!(period_for_shot(0, 5, 101), 0);
        assert_eq!(period_for_shot(4, 5, 101), 100);
        assert_eq!(period_for_shot(2, 5, 101), 50);
    }

    /// Degenerate runs must not divide by zero or index past the end.
    #[test]
    fn period_selection_survives_degenerate_runs() {
        assert_eq!(period_for_shot(0, 1, 0), 0);
        assert_eq!(period_for_shot(3, 4, 0), 0);
        assert_eq!(period_for_shot(0, 4, 1), 0);
        assert_eq!(period_for_shot(3, 4, 1), 0);
        // Asking for more images than there are periods still stays in range.
        assert_eq!(period_for_shot(9, 10, 3), 2);
    }
}
