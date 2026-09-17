// SPDX-License-Identifier: GPL-3.0-or-later

//! The SWMM tab: choose an engine, run a model, and read what came back.
//!
//! Runs happen on a worker thread. A real model takes minutes and the engine
//! is a child process we do nothing but wait on, so running it inline would
//! freeze the window for the whole simulation. This is the only background
//! work in the app, so it is kept deliberately small: one channel, polled
//! once per frame from `ui()`, and no shared mutable state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Ui, Vec2};

use stormsewer_swmm::alr::{Alr, AlrOptions, AlrReport};
use stormsewer_swmm::engine::{Engine, Registry, Run};
use stormsewer_swmm::inp::{InpModel, NodeKind};
use stormsewer_swmm::out::{
    format_datetime, link_peaks, link_series, node_peaks, node_series, LinkPeak, NodePeak,
    OutputFile, Series,
};

use crate::profile::station_tick_step;
use crate::state::AppState;
use crate::theme::palette;
use crate::viewport::Viewport;

/// Which side of the model the plotted series comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlotTarget {
    #[default]
    Node,
    Link,
}

/// Which SWMM view fills the central panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwmmSubView {
    /// The network map. The default: a model is a drawing before it is a
    /// dataset, and seeing it is how a user confirms they opened the right one.
    #[default]
    Map,
    /// One reported series against time.
    Chart,
}

/// Reported variables, in the order SWMM writes them.
fn variable_names(target: PlotTarget) -> &'static [&'static str] {
    match target {
        PlotTarget::Node => &[
            "Depth",
            "Head",
            "Volume",
            "Lateral inflow",
            "Total inflow",
            "Flooding",
        ],
        PlotTarget::Link => &["Flow", "Depth", "Velocity", "Volume", "Capacity"],
    }
}

/// What a finished worker thread hands back. Boxed because a `Run` carries
/// the whole parsed report and dwarfs the error string.
enum RunOutcome {
    Finished(Box<Run>),
    Failed(String),
}

/// Engines found, the model chosen, and the last run's results.
#[derive(Default)]
pub struct SwmmState {
    pub registry: Registry,
    discovered: bool,
    pub engine_id: Option<String>,
    pub model: Option<PathBuf>,
    /// Present only while a run is in flight.
    pending: Option<Receiver<RunOutcome>>,
    pub last_run: Option<Run>,
    pub results: Option<OutputFile>,
    pub alr: Option<AlrReport>,
    /// Whatever the user most needs told: an error, or a note about progress.
    pub log: String,
    pub plot_target: PlotTarget,
    pub plot_id: Option<String>,
    pub plot_var: usize,
    /// The series being plotted, and the selection it was read for. Reading
    /// one seeks once per reporting period, so it is cached rather than
    /// re-read every frame.
    plot_series: Option<Series>,
    plot_key: Option<(PlotTarget, String, usize)>,
    /// Whole-model peaks from the last run, worst first. Computed and sorted
    /// once when the run finishes, since the report panel only reads them.
    pub node_peaks: Vec<NodePeak>,
    pub link_peaks: Vec<LinkPeak>,
    pub sub_view: SwmmSubView,
    /// The parsed `.inp`, for drawing. Kept apart from `model`, which is only
    /// the path handed to the engine: the engine reads the file itself, so a
    /// model this parser cannot draw is still a model that can be run.
    pub model_inp: Option<InpModel>,
    /// Pan/zoom for the map.
    ///
    /// The plan view's viewport cannot be shared. A SWMM model carries its own
    /// coordinates — often a ten-thousand-unit map sheet — so one viewport
    /// serving both would throw the plan view somewhere absurd every time the
    /// user switched tabs.
    pub map_viewport: Viewport,
    /// Fit the map to the model on the next frame, once the canvas rect is
    /// known. Fitting needs a rect, which only the draw call has.
    pub pending_map_fit: bool,
}

impl SwmmState {
    /// Scan for engines the first time the tab is shown. Discovery touches the
    /// filesystem, so it has no business running in `Default`.
    pub fn ensure_discovered(&mut self) {
        if !self.discovered {
            self.rescan();
        }
    }

    pub fn rescan(&mut self) {
        self.registry = Registry::discover();
        self.discovered = true;
        if self.engine_id.is_none() {
            self.engine_id = self.registry.default_engine().map(|e| e.id.clone());
        }
        self.log = match self.registry.engines().len() {
            0 => "No SWMM engine found. Install EPA SWMM, or set \
                  STORMSEWER_SWMM_ENGINE_DIR to the folder holding runswmm."
                .to_string(),
            1 => "Found 1 SWMM engine.".to_string(),
            n => format!("Found {n} SWMM engines."),
        };
    }

    /// Parse the chosen `.inp` so the network can be drawn.
    ///
    /// A model that will not parse is still handed to the engine unchanged, so
    /// this reports the problem and leaves the map empty rather than refusing
    /// to run it.
    pub fn load_model_geometry(&mut self) {
        self.model_inp = None;
        self.pending_map_fit = false;
        let Some(path) = self.model.clone() else {
            return;
        };
        match InpModel::read(&path) {
            Ok(model) => {
                self.log = model.summary();
                self.pending_map_fit = true;
                self.model_inp = Some(model);
            }
            Err(e) => self.log = format!("This model could not be read for drawing: {e}"),
        }
    }

    pub fn engine(&self) -> Option<&Engine> {
        self.engine_id
            .as_deref()
            .and_then(|id| self.registry.by_id(id))
    }

    pub fn is_running(&self) -> bool {
        self.pending.is_some()
    }

    pub fn can_run(&self) -> bool {
        !self.is_running() && self.engine().is_some() && self.model.is_some()
    }

    /// One line for the status bar.
    pub fn status_line(&self) -> String {
        if self.is_running() {
            return "SWMM: running…".to_string();
        }
        match &self.last_run {
            None => "SWMM: idle".to_string(),
            Some(run) => match run.failure_reason() {
                None => format!(
                    "SWMM: finished in {:.1} s, {} warning(s)",
                    run.elapsed.as_secs_f64(),
                    run.report.warnings.len()
                ),
                Some(why) => format!("SWMM: {why}"),
            },
        }
    }

    pub fn start_run(&mut self) {
        let (Some(engine), Some(model)) = (self.engine().cloned(), self.model.clone()) else {
            self.log = "Choose an engine and a model first.".to_string();
            return;
        };
        self.results = None;
        self.alr = None;
        self.last_run = None;
        self.node_peaks.clear();
        self.link_peaks.clear();
        self.log = format!("Running with {}…", engine.label());

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = match engine.run(&model) {
                Ok(run) => RunOutcome::Finished(Box::new(run)),
                Err(e) => RunOutcome::Failed(e.to_string()),
            };
            // The receiver is gone if the window closed mid-run. Nothing to do.
            let _ = tx.send(outcome);
        });
        self.pending = Some(rx);
    }

    /// Collect a finished run. Returns true when something changed, so the
    /// caller knows to refresh the status line.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = self.pending.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                // A worker that panicked would otherwise leave the tab saying
                // "running" for the rest of the session.
                self.pending = None;
                self.log = "The run stopped without reporting a result.".to_string();
                true
            }
            Ok(RunOutcome::Failed(message)) => {
                self.pending = None;
                self.log = message;
                true
            }
            Ok(RunOutcome::Finished(run)) => {
                self.pending = None;
                self.log = String::new();
                if run.succeeded() {
                    match OutputFile::open(&run.out) {
                        Ok(f) => {
                            // Point the chart at something real rather than
                            // opening on an empty plot.
                            self.plot_id = f.meta.node_ids.first().cloned();
                            self.plot_target = PlotTarget::Node;
                            self.plot_var = 0;
                            self.plot_key = None;
                            self.plot_series = None;
                            // One pass over the results, ordered worst first,
                            // so the report panel can simply iterate.
                            match node_peaks(&f.path, &f.meta) {
                                Ok(mut rows) => {
                                    rows.sort_by(|a, b| b.max_depth.total_cmp(&a.max_depth));
                                    self.node_peaks = rows;
                                }
                                Err(e) => self.log = e.to_string(),
                            }
                            match link_peaks(&f.path, &f.meta) {
                                Ok(mut rows) => {
                                    rows.sort_by(|a, b| {
                                        b.max_flow.abs().total_cmp(&a.max_flow.abs())
                                    });
                                    self.link_peaks = rows;
                                }
                                Err(e) => self.log = e.to_string(),
                            }
                            self.results = Some(f);
                        }
                        Err(e) => self.log = format!("Results could not be read: {e}"),
                    }
                }
                self.last_run = Some(*run);
                true
            }
        }
    }

    /// Read the selected series unless the cache already holds it.
    pub fn ensure_series(&mut self) {
        let Some(id) = self.plot_id.clone() else {
            self.plot_series = None;
            self.plot_key = None;
            return;
        };
        let key = (self.plot_target, id.clone(), self.plot_var);
        if self.plot_key.as_ref() == Some(&key) {
            return;
        }
        let Some(file) = self.results.as_ref() else {
            self.plot_series = None;
            self.plot_key = None;
            return;
        };
        // Cloned so the borrow of `results` ends before the fields are set;
        // this runs on a selection change, not every frame.
        let (path, meta) = (file.path.clone(), file.meta.clone());
        let read = match self.plot_target {
            PlotTarget::Node => node_series(&path, &meta, &id, self.plot_var),
            PlotTarget::Link => link_series(&path, &meta, &id, self.plot_var),
        };
        match read {
            Ok(series) => {
                self.plot_series = Some(series);
                self.plot_key = Some(key);
            }
            Err(e) => {
                self.plot_series = None;
                self.plot_key = None;
                self.log = e.to_string();
            }
        }
    }

    pub fn series(&self) -> Option<&Series> {
        self.plot_series.as_ref()
    }

    /// Post-process the last run with ALR. Synchronous: this reads a finished
    /// results file rather than simulating, and returns in well under a second
    /// on the models it is meant for.
    pub fn run_alr(&mut self) {
        let Some(out) = self.last_run.as_ref().map(|r| r.out.clone()) else {
            return;
        };
        let Some(alr) = Alr::from_env() else {
            self.log = "Set STORMSEWER_ALR_SCRIPT to the path of \
                        run_headless_swmm.py to enable ALR checks."
                .to_string();
            return;
        };
        match alr.analyze(&out, &AlrOptions::default()) {
            Ok(report) => {
                self.log = report.summary();
                self.alr = Some(report);
            }
            Err(e) => self.log = e.to_string(),
        }
    }
}

/// Short hash prefix for display, without assuming a length.
fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

/// Pick what the results view plots. Names are collected first so the combo
/// closures can take the state mutably.
fn draw_series_picker(ui: &mut Ui, state: &mut AppState) {
    let names: Vec<String> = match (&state.swmm.results, state.swmm.plot_target) {
        (Some(f), PlotTarget::Node) => f.meta.node_ids.clone(),
        (Some(f), PlotTarget::Link) => f.meta.link_ids.clone(),
        (None, _) => Vec::new(),
    };
    if names.is_empty() {
        return;
    }

    ui.add_space(6.0);
    ui.label(RichText::new("Plot").strong());
    ui.horizontal(|ui| {
        for (target, label) in [(PlotTarget::Node, "Nodes"), (PlotTarget::Link, "Links")] {
            if ui
                .selectable_label(state.swmm.plot_target == target, label)
                .clicked()
                && state.swmm.plot_target != target
            {
                // Variable indices mean different things per target, so the
                // selection cannot carry across.
                state.swmm.plot_target = target;
                state.swmm.plot_var = 0;
                state.swmm.plot_id = None;
            }
        }
    });

    if state.swmm.plot_id.is_none() {
        state.swmm.plot_id = names.first().cloned();
    }
    let current = state.swmm.plot_id.clone().unwrap_or_default();
    egui::ComboBox::from_id_salt("swmm-plot-id")
        .selected_text(current)
        .width(180.0)
        .show_ui(ui, |ui| {
            for name in &names {
                let selected = state.swmm.plot_id.as_deref() == Some(name.as_str());
                if ui.selectable_label(selected, name).clicked() {
                    state.swmm.plot_id = Some(name.clone());
                }
            }
        });

    let variables = variable_names(state.swmm.plot_target);
    let current_var = variables.get(state.swmm.plot_var).copied().unwrap_or("");
    egui::ComboBox::from_id_salt("swmm-plot-var")
        .selected_text(current_var)
        .width(180.0)
        .show_ui(ui, |ui| {
            for (i, label) in variables.iter().enumerate() {
                if ui
                    .selectable_label(state.swmm.plot_var == i, *label)
                    .clicked()
                {
                    state.swmm.plot_var = i;
                }
            }
        });

}

// Chart margins: the left holds value labels, the bottom the time axis.
const PAD_LEFT: f32 = 68.0;
const PAD_RIGHT: f32 = 24.0;
const PAD_TOP: f32 = 44.0;
const PAD_BOTTOM: f32 = 48.0;

/// Units for the selected variable, taken from what the engine reported.
fn unit_label(target: PlotTarget, var: usize, metric: bool) -> &'static str {
    let (length, flow, velocity, volume) = if metric {
        ("m", "m³/s", "m/s", "m³")
    } else {
        ("ft", "cfs", "ft/s", "ft³")
    };
    match (target, var) {
        (PlotTarget::Node, 0 | 1) => length,
        (PlotTarget::Node, 2) => volume,
        (PlotTarget::Node, 3..=5) => flow,
        (PlotTarget::Link, 0) => flow,
        (PlotTarget::Link, 1) => length,
        (PlotTarget::Link, 2) => velocity,
        (PlotTarget::Link, 3) => volume,
        (PlotTarget::Link, 4) => "fraction",
        _ => "",
    }
}

/// The results view: one reported series against time.
pub fn draw_swmm_results(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    state.swmm.ensure_series();

    let dark = ui.visuals().dark_mode;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));

    let empty_state = |line: &str| {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            line,
            egui::FontId::proportional(15.0),
            palette::canvas::muted(dark),
        );
    };

    let Some(series) = state.swmm.series() else {
        empty_state(if state.swmm.results.is_some() {
            "Choose a node or link in the SWMM tab to plot"
        } else {
            "Run a SWMM model to see results here"
        });
        return;
    };
    if series.values.len() < 2 {
        empty_state("This run has too few reporting periods to plot");
        return;
    }

    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD_LEFT, rect.top() + PAD_TOP),
        Pos2::new(rect.right() - PAD_RIGHT, rect.bottom() - PAD_BOTTOM),
    );
    if inner.width() < 40.0 || inner.height() < 40.0 {
        return;
    }

    let hours: Vec<f64> = series.times_s.iter().map(|t| t / 3600.0).collect();
    let (t0, t1) = (hours[0], hours[hours.len() - 1]);
    let mut lo = series.values.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut hi = series
        .values
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    if !(t1 > t0) || !lo.is_finite() || !hi.is_finite() {
        empty_state("This series has no plottable values");
        return;
    }
    if hi - lo < 1e-9 {
        // A flat series still deserves a line rather than a divide by zero.
        lo -= 0.5;
        hi += 0.5;
    } else {
        // Breathing room, rather than a forced zero baseline: heads and
        // elevations are nowhere near zero and would flatten against the top.
        let pad = (hi - lo) * 0.05;
        lo -= pad;
        hi += pad;
    }

    let x_at = |t: f64| inner.left() + ((t - t0) / (t1 - t0)) as f32 * inner.width();
    let y_at = |v: f64| inner.bottom() - ((v - lo) / (hi - lo)) as f32 * inner.height();

    // Value axis.
    let v_step = station_tick_step(hi - lo);
    let mut v = (lo / v_step).ceil() * v_step;
    while v <= hi + v_step * 0.01 {
        let y = y_at(v);
        painter.line_segment(
            [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
            Stroke::new(1.0_f32, palette::canvas::grid(dark)),
        );
        painter.text(
            Pos2::new(inner.left() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{v:.2}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        v += v_step;
    }

    // Time axis.
    let t_step = station_tick_step(t1 - t0);
    let axis_y = inner.bottom();
    painter.line_segment(
        [
            Pos2::new(inner.left(), axis_y),
            Pos2::new(inner.right(), axis_y),
        ],
        Stroke::new(1.0_f32, palette::canvas::line(dark)),
    );
    let mut t = (t0 / t_step).ceil() * t_step;
    while t <= t1 + t_step * 0.01 {
        let x = x_at(t);
        painter.line_segment(
            [Pos2::new(x, axis_y), Pos2::new(x, axis_y + 5.0)],
            Stroke::new(1.0_f32, palette::canvas::line(dark)),
        );
        painter.text(
            Pos2::new(x, axis_y + 8.0),
            egui::Align2::CENTER_TOP,
            format!("{t:.2}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        t += t_step;
    }
    painter.text(
        rect.center_bottom() - egui::Vec2::new(0.0, 4.0),
        egui::Align2::CENTER_BOTTOM,
        "Time (hours from start)",
        egui::FontId::proportional(11.0),
        palette::canvas::muted(dark),
    );

    // The series itself.
    let stroke = Stroke::new(2.0_f32, palette::canvas::hgl(dark));
    for pair in hours.windows(2).zip(series.values.windows(2)) {
        let (ts, vs) = pair;
        painter.line_segment(
            [Pos2::new(x_at(ts[0]), y_at(vs[0])), Pos2::new(x_at(ts[1]), y_at(vs[1]))],
            stroke,
        );
    }

    let metric = state
        .swmm
        .results
        .as_ref()
        .is_some_and(|f| f.meta.flow_units.is_metric());
    let units = unit_label(state.swmm.plot_target, state.swmm.plot_var, metric);
    let variable = variable_names(state.swmm.plot_target)
        .get(state.swmm.plot_var)
        .copied()
        .unwrap_or("");
    let id = state.swmm.plot_id.clone().unwrap_or_default();

    painter.text(
        rect.left_top() + egui::Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        format!("SWMM results · {id} · {variable} ({units})"),
        egui::FontId::proportional(13.0),
        palette::canvas::muted(dark),
    );

    // The peak is the number a reviewer looks for, so mark it.
    if let Some((peak, at_s)) = series.peak() {
        let at_h = at_s / 3600.0;
        let p = Pos2::new(x_at(at_h), y_at(peak));
        painter.circle_stroke(p, 4.0, Stroke::new(1.5_f32, palette::canvas::ink(dark)));
        painter.text(
            p - egui::Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_BOTTOM,
            format!("peak {peak:.3} {units} at {at_h:.2} h"),
            egui::FontId::monospace(11.0),
            palette::canvas::ink(dark),
        );
    }
}

// --- Map ---------------------------------------------------------------------

/// How close a click must land to pick an object, in screen pixels.
const MAP_HIT_RADIUS: f32 = 12.0;

/// Distance from `p` to the segment `a`–`b`, in screen pixels.
fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Click the map to choose what the chart plots.
///
/// Nodes are tested before links: a node is the smaller target, and it is
/// usually what someone reading a model is reaching for.
pub fn map_click(state: &mut AppState, rect: Rect, pos: Pos2) {
    let Some(model) = state.swmm.model_inp.as_ref() else {
        return;
    };
    let vp = &state.swmm.map_viewport;
    let mut best: Option<(f32, PlotTarget, String)> = None;

    for node in &model.nodes {
        let Some((x, y)) = node.pos() else {
            continue;
        };
        let d = (vp.world_to_screen(rect, x, y) - pos).length();
        if d <= MAP_HIT_RADIUS && best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, PlotTarget::Node, node.id.clone()));
        }
    }
    if best.is_none() {
        for link in &model.links {
            for seg in model.link_polyline(link).windows(2) {
                let a = vp.world_to_screen(rect, seg[0].0, seg[0].1);
                let b = vp.world_to_screen(rect, seg[1].0, seg[1].1);
                let d = distance_to_segment(pos, a, b);
                if d <= MAP_HIT_RADIUS && best.as_ref().is_none_or(|x| d < x.0) {
                    best = Some((d, PlotTarget::Link, link.id.clone()));
                }
            }
        }
    }

    if let Some((_, target, id)) = best {
        // A variable index means different things per target — 4 is total
        // inflow for a node and capacity for a link — so it cannot carry over.
        if state.swmm.plot_target != target {
            state.swmm.plot_var = 0;
        }
        state.swmm.plot_target = target;
        state.swmm.plot_id = Some(id);
    }
}

/// Compact colour key for the map.
fn draw_map_legend(painter: &egui::Painter, rect: Rect, dark: bool) {
    // (draw as a line, colour, label)
    let rows: [(bool, Color32, &str); 6] = [
        (true, palette::FLOW_OK, "Link within capacity"),
        (true, palette::WARNING, "Link 85% full or more"),
        (true, palette::ERROR, "Link full / node flooded"),
        (false, palette::NODE_INLET, "Junction"),
        (false, palette::NODE_OUTFALL, "Outfall"),
        (false, palette::NODE_JUNCTION, "Storage / divider"),
    ];

    let pad = 8.0;
    let row_h = 17.0;
    let marker_w = 16.0;
    let box_w = 186.0;
    let box_h = pad * 2.0 + row_h * rows.len() as f32;
    let origin = rect.right_top() + Vec2::new(-box_w - 12.0, 12.0);
    let bg = Rect::from_min_size(origin, Vec2::new(box_w, box_h));

    painter.rect_filled(bg, 5.0, palette::canvas::panel_fill(dark));
    painter.rect_stroke(bg, 5.0, Stroke::new(1.0_f32, palette::canvas::line(dark)));

    for (i, (is_line, color, label)) in rows.iter().enumerate() {
        let cy = bg.top() + pad + row_h * i as f32 + row_h / 2.0;
        let mx = bg.left() + pad;
        if *is_line {
            painter.line_segment(
                [Pos2::new(mx, cy), Pos2::new(mx + marker_w, cy)],
                Stroke::new(3.0_f32, *color),
            );
        } else {
            painter.circle_filled(Pos2::new(mx + marker_w / 2.0, cy), 5.0, *color);
            painter.circle_stroke(
                Pos2::new(mx + marker_w / 2.0, cy),
                5.0,
                Stroke::new(1.0_f32, palette::canvas::ink(dark)),
            );
        }
        painter.text(
            Pos2::new(mx + marker_w + 7.0, cy),
            egui::Align2::LEFT_CENTER,
            *label,
            egui::FontId::proportional(12.0),
            palette::canvas::muted(dark),
        );
    }
}

/// The SWMM network map: subcatchments, links, and nodes, coloured by the last
/// run's peaks once there is a run.
///
/// The colour vocabulary is the plan view's, so both views read the same way.
pub fn draw_swmm_map(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;

    // Fitting needs the canvas rect, which only this call knows.
    if state.swmm.pending_map_fit {
        if let Some((min_x, min_y, max_x, max_y)) =
            state.swmm.model_inp.as_ref().and_then(|m| m.bounds())
        {
            state
                .swmm
                .map_viewport
                .fit_bounds(rect, min_x, min_y, max_x, max_y);
        }
        state.swmm.pending_map_fit = false;
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));

    let empty_state = |line: &str| {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            line,
            egui::FontId::proportional(15.0),
            palette::canvas::muted(dark),
        );
    };

    let Some(model) = state.swmm.model_inp.as_ref() else {
        empty_state("Choose a model (.inp) in the SWMM tab to see its network");
        return;
    };
    let Some((bx0, by0, bx1, by1)) = model.bounds() else {
        empty_state("This model has no coordinates, so there is nothing to draw");
        return;
    };
    let vp = &state.swmm.map_viewport;

    // Results, when the model has been run: flooding for nodes, how full for
    // links. Indexed once rather than searched per object.
    let node_results: HashMap<&str, &NodePeak> = state
        .swmm
        .node_peaks
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let link_results: HashMap<&str, &LinkPeak> = state
        .swmm
        .link_peaks
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();

    // Outlines, not fills: a subcatchment polygon is frequently concave, and a
    // convex-polygon fill would draw those as a bowtie.
    let sub_stroke = Stroke::new(1.0_f32, palette::canvas::grid(dark));
    for sub in &model.subcatchments {
        if sub.polygon.len() < 3 {
            continue;
        }
        let mut pts: Vec<Pos2> = sub
            .polygon
            .iter()
            .map(|&(x, y)| vp.world_to_screen(rect, x, y))
            .collect();
        pts.push(pts[0]);
        painter.add(egui::Shape::line(pts, sub_stroke));
    }

    for link in &model.links {
        let path = model.link_polyline(link);
        if path.len() < 2 {
            continue;
        }
        let selected = state.swmm.plot_target == PlotTarget::Link
            && state.swmm.plot_id.as_deref() == Some(link.id.as_str());
        let color = match link_results.get(link.id.as_str()) {
            Some(p) if p.max_capacity >= 1.0 => palette::ERROR,
            Some(p) if p.max_capacity >= 0.85 => palette::WARNING,
            Some(_) => palette::FLOW_OK,
            // Unrun: draw the network without implying a verdict about it.
            None => palette::canvas::line(dark),
        };
        let (color, width) = if selected {
            (palette::canvas::selection(dark), 5.0_f32)
        } else {
            (color, 3.0_f32)
        };
        let pts: Vec<Pos2> = path
            .iter()
            .map(|&(x, y)| vp.world_to_screen(rect, x, y))
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(width, color)));
    }

    // Labels appear when there is room for them. Judged from the screen space
    // each node gets rather than from zoom alone, so a small site and a large
    // basin behave the same way once fitted.
    let span_px = (bx1 - bx0).max(by1 - by0) as f32 * vp.zoom;
    let per_node = span_px / (model.nodes.len().max(1) as f32).sqrt();
    let show_ids = per_node > 45.0;

    for node in &model.nodes {
        let Some((x, y)) = node.pos() else {
            continue;
        };
        let center = vp.world_to_screen(rect, x, y);
        let selected = state.swmm.plot_target == PlotTarget::Node
            && state.swmm.plot_id.as_deref() == Some(node.id.as_str());
        let mut color = match node.kind {
            NodeKind::Junction => palette::NODE_INLET,
            NodeKind::Outfall => palette::NODE_OUTFALL,
            NodeKind::Storage | NodeKind::Divider => palette::NODE_JUNCTION,
        };
        if node_results
            .get(node.id.as_str())
            .is_some_and(|p| p.flooded())
        {
            color = palette::ERROR;
        }
        let r = if selected { 11.0_f32 } else { 7.0_f32 };
        painter.circle_filled(center, r, color);
        painter.circle_stroke(
            center,
            r,
            Stroke::new(
                1.5_f32,
                if selected {
                    palette::canvas::selection(dark)
                } else {
                    palette::canvas::ink(dark)
                },
            ),
        );
        if show_ids || selected {
            painter.text(
                center + Vec2::new(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                &node.id,
                egui::FontId::proportional(12.0),
                palette::canvas::ink(dark),
            );
        }
    }

    // Rain gages: where the rainfall enters, which is worth seeing on the map.
    for gage in &model.gages {
        if let (Some(x), Some(y)) = (gage.x, gage.y) {
            let c = vp.world_to_screen(rect, x, y);
            painter.circle_filled(c, 4.0, palette::ACCENT);
            if show_ids {
                painter.text(
                    c + Vec2::new(8.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    &gage.id,
                    egui::FontId::proportional(11.0),
                    palette::canvas::muted(dark),
                );
            }
        }
    }

    let name = state
        .swmm
        .model
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let mut header = format!("SWMM map · {name} · {}", model.summary());
    if state.swmm.link_peaks.is_empty() {
        header.push_str("  ·  not run yet");
    }
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        header,
        egui::FontId::proportional(13.0),
        palette::canvas::muted(dark),
    );

    draw_map_legend(&painter, rect, dark);
}

/// The central SWMM view: the network map, or the chart.
pub fn draw_swmm_view(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    match state.swmm.sub_view {
        SwmmSubView::Map => draw_swmm_map(ui, rect, state),
        SwmmSubView::Chart => draw_swmm_results(ui, rect, state),
    }
}

pub fn draw_swmm_tab(ui: &mut Ui, state: &mut AppState) {
    state.swmm.ensure_discovered();

    ui.label(RichText::new("Engine").strong());
    if state.swmm.registry.is_empty() {
        ui.colored_label(egui::Color32::LIGHT_RED, "No SWMM engine found.");
    } else {
        // Collected first: the selection closure needs `state` mutably.
        let engines: Vec<(String, String, bool)> = state
            .swmm
            .registry
            .engines()
            .iter()
            .map(|e| (e.id.clone(), e.label(), e.arch.requires_subprocess_from_x64()))
            .collect();
        for (id, label, out_of_process) in engines {
            let selected = state.swmm.engine_id.as_deref() == Some(id.as_str());
            let text = if out_of_process {
                format!("{label}  ·  runs out-of-process")
            } else {
                label
            };
            if ui.selectable_label(selected, text).clicked() {
                state.swmm.engine_id = Some(id);
            }
        }
    }
    if ui.button("Find Engines").clicked() {
        state.swmm.rescan();
    }

    ui.add_space(6.0);
    ui.separator();
    ui.label(RichText::new("Model").strong());
    let model_text = state
        .swmm
        .model
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "none chosen".to_string());
    ui.label(model_text);
    if ui.button("Choose Model (.inp)…").clicked() {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SWMM Input", &["inp", "INP"])
            .pick_file()
        {
            state.swmm.model = Some(path);
            state.swmm.last_run = None;
            state.swmm.results = None;
            state.swmm.alr = None;
            state.swmm.node_peaks.clear();
            state.swmm.link_peaks.clear();
            state.swmm.log = String::new();
            state.swmm.load_model_geometry();
        }
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        for (view, label) in [(SwmmSubView::Map, "Map"), (SwmmSubView::Chart, "Chart")] {
            if ui
                .selectable_label(state.swmm.sub_view == view, label)
                .clicked()
            {
                state.swmm.sub_view = view;
                state.view_tab = crate::state::ViewTab::Swmm;
            }
        }
        if ui.button("Fit Map").clicked() {
            state.swmm.pending_map_fit = true;
            state.swmm.sub_view = SwmmSubView::Map;
            state.view_tab = crate::state::ViewTab::Swmm;
        }
    });

    ui.add_space(6.0);
    let can_run = state.swmm.can_run();
    if ui
        .add_enabled(can_run, egui::Button::new("Run Model"))
        .clicked()
    {
        state.swmm.start_run();
        state.status = state.swmm.status_line();
    }
    if state.swmm.is_running() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("running…");
        });
    }

    let mut start_alr = false;
    if let Some(run) = &state.swmm.last_run {
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Last run").strong());
        ui.label(format!(
            "engine {} ({})",
            run.engine_version,
            short_hash(&run.engine_sha256)
        ));
        ui.label(format!("{:.1} s", run.elapsed.as_secs_f64()));

        for error in &run.report.errors {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        for warning in run.report.warnings.iter().take(6) {
            ui.colored_label(egui::Color32::GRAY, warning);
        }
        if let Some((section, pct)) = run.report.worst_continuity() {
            ui.label(format!("worst continuity {pct:+.3}% ({section})"));
        }
        if let Some(why) = run.failure_reason() {
            ui.colored_label(egui::Color32::LIGHT_RED, why);
        }

        ui.add_space(4.0);
        if ui
            .add_enabled(run.succeeded(), egui::Button::new("Run ALR Checks"))
            .clicked()
        {
            start_alr = true;
        }
    }
    if start_alr {
        state.swmm.run_alr();
        state.status = state.swmm.status_line();
    }

    if let Some(f) = &state.swmm.results {
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Results").strong());
        ui.label(format!(
            "{} periods every {} s",
            f.meta.n_periods, f.meta.report_step_s
        ));
        ui.label(format!(
            "{} nodes, {} links, {}",
            f.meta.n_nodes,
            f.meta.n_links,
            f.meta.flow_units.label()
        ));
        ui.label(format!("start {}", format_datetime(f.meta.start_days)));
    }
    draw_series_picker(ui, state);

    if let Some(report) = &state.swmm.alr {
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("ALR").strong());
        ui.label(report.summary());
        for check in report.failures() {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!(
                    "{} — {}",
                    check.name,
                    check.detail.clone().unwrap_or_default()
                ),
            );
        }
    }

    if !state.swmm.log.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(state.swmm.log.clone()).monospace());
    }
}
