// SPDX-License-Identifier: GPL-3.0-or-later

//! Results on the SWMM map: nodes and links coloured by any reported
//! variable, a five-class legend with editable breaks, link width by flow,
//! flow-direction arrows, playback controls, and a query tool.
//!
//! # Hooks for the map view
//!
//! The map canvas is drawn elsewhere. It reaches this module through three
//! functions, each taking the whole `AppState` so the caller passes nothing
//! but what it already has:
//!
//! - [`draw_results_overlay`]`(painter, rect, state, dark)` — after the map's
//!   own links and nodes: repaints them in the chosen variables' colours at
//!   the current reporting period (or the run peaks when no period is
//!   shown), adds flow arrows and the query highlight, and updates the query
//!   count. Draws nothing until a run has results.
//! - [`legend`]`(painter, rect, state, dark)` — the class legend and the
//!   frame timestamp, anchored bottom-left so it clears the map's own key.
//! - [`draw_results_controls`]`(ui, state)` — the pickers, breaks, playback
//!   and query widgets, for a side panel or a strip.
//!
//! Until the map view calls them, the "Results" sub-view
//! ([`draw_swmm_results_view`]) shows the map with all three applied.
//!
//! Playback state lives in `SwmmState` (`period`, `playing`, `frame()`); this
//! module only adds the speed factor and the widgets.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui, Vec2};

use stormsewer_swmm::inp::NodeKind;
use stormsewer_swmm::out::{
    format_datetime, link_variable_names, node_variable_names, read_frame_raw, OutputFile, RawFrame,
};
use stormsewer_swmm::results::ClassBreaks;

use crate::state::AppState;
use crate::swmm_panel::PlotTarget;
use crate::theme::palette;

/// Five-class sequential ramp, light to dark, readable on either background.
pub const RAMP: [Color32; 5] = [
    Color32::from_rgb(255, 245, 180),
    Color32::from_rgb(254, 196, 79),
    Color32::from_rgb(251, 128, 42),
    Color32::from_rgb(220, 50, 32),
    Color32::from_rgb(128, 0, 38),
];

pub fn class_color(class: usize) -> Color32 {
    RAMP[class.min(4)]
}

/// Highlight objects above or below a value at the current instant.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub enabled: bool,
    pub target: PlotTarget,
    pub above: bool,
    pub value: f64,
    /// Objects matching at the last drawn frame.
    pub count: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            enabled: false,
            target: PlotTarget::Node,
            above: true,
            value: 0.0,
            count: 0,
        }
    }
}

impl Query {
    pub fn matches(&self, v: f64) -> bool {
        if self.above {
            v > self.value
        } else {
            v < self.value
        }
    }
}

/// Run-wide maxima of the chosen variables, for stable class breaks.
struct PeakCache {
    path: PathBuf,
    node_var: usize,
    link_var: usize,
    nodes: Vec<f64>,
    links: Vec<f64>,
    /// Largest |flow| over the run, for the width scale.
    max_abs_flow: f64,
}

/// Overlay settings and caches.
pub struct ResultsOverlayState {
    pub node_var: usize,
    pub link_var: usize,
    pub node_breaks: ClassBreaks,
    pub link_breaks: ClassBreaks,
    /// Recompute breaks from the run's range when the variable changes.
    pub auto_breaks: bool,
    pub width_by_flow: bool,
    pub arrows: bool,
    pub query: Query,
    raw: Option<RawFrame>,
    raw_key: Option<(PathBuf, usize)>,
    peaks: Option<PeakCache>,
    breaks_key: Option<(PathBuf, usize, usize)>,
}

impl Default for ResultsOverlayState {
    fn default() -> Self {
        Self {
            node_var: 0,
            link_var: 0,
            node_breaks: ClassBreaks::default(),
            link_breaks: ClassBreaks::default(),
            auto_breaks: true,
            width_by_flow: false,
            arrows: true,
            query: Query::default(),
            raw: None,
            raw_key: None,
            peaks: None,
            breaks_key: None,
        }
    }
}

impl ResultsOverlayState {
    /// Values per `.out` node index at the current instant, or the run maxima
    /// when no instant is shown.
    pub fn node_values(&self, showing_frame: bool) -> Option<Vec<f64>> {
        if showing_frame {
            let raw = self.raw.as_ref()?;
            let n = raw.nodes.len() / raw.n_node_vars.max(1);
            Some((0..n).filter_map(|i| raw.node(i, self.node_var)).collect())
        } else {
            self.peaks.as_ref().map(|p| p.nodes.clone())
        }
    }

    pub fn link_values(&self, showing_frame: bool) -> Option<Vec<f64>> {
        if showing_frame {
            let raw = self.raw.as_ref()?;
            let n = raw.links.len() / raw.n_link_vars.max(1);
            Some((0..n).filter_map(|i| raw.link(i, self.link_var)).collect())
        } else {
            self.peaks.as_ref().map(|p| p.links.clone())
        }
    }

    /// Flow (link variable 0) per link at the current instant, for arrows and
    /// widths. `None` outside an instant.
    fn flows(&self) -> Option<Vec<f64>> {
        let raw = self.raw.as_ref()?;
        let n = raw.links.len() / raw.n_link_vars.max(1);
        Some((0..n).filter_map(|i| raw.link(i, 0)).collect())
    }

    /// Bring the caches up to date with the results file and the period.
    pub fn refresh(&mut self, results: Option<&OutputFile>, showing_frame: bool, period: usize) {
        let Some(file) = results else {
            self.raw = None;
            self.raw_key = None;
            self.peaks = None;
            self.breaks_key = None;
            return;
        };
        let meta = &file.meta;
        self.node_var = self.node_var.min(meta.n_node_vars().saturating_sub(1));
        self.link_var = self.link_var.min(meta.n_link_vars().saturating_sub(1));

        if showing_frame {
            let key = (file.path.clone(), period);
            if self.raw_key.as_ref() != Some(&key) {
                self.raw = read_frame_raw(&file.path, meta, period).ok();
                self.raw_key = self.raw.as_ref().map(|_| key);
            }
        }

        let stale = self.peaks.as_ref().is_none_or(|p| {
            p.path != file.path || p.node_var != self.node_var || p.link_var != self.link_var
        });
        if stale {
            // One pass over every period: a seek per record, so scrubbing
            // afterwards costs nothing more.
            let mut nodes = vec![f64::NEG_INFINITY; meta.n_nodes];
            let mut links = vec![f64::NEG_INFINITY; meta.n_links];
            let mut max_abs_flow = 0.0_f64;
            for p in 0..meta.n_periods {
                let Ok(raw) = read_frame_raw(&file.path, meta, p) else {
                    break;
                };
                for (i, slot) in nodes.iter_mut().enumerate() {
                    if let Some(v) = raw.node(i, self.node_var) {
                        *slot = slot.max(v);
                    }
                }
                for (i, slot) in links.iter_mut().enumerate() {
                    if let Some(v) = raw.link(i, self.link_var) {
                        *slot = slot.max(v);
                    }
                    if let Some(q) = raw.link(i, 0) {
                        max_abs_flow = max_abs_flow.max(q.abs());
                    }
                }
            }
            for v in nodes.iter_mut().chain(links.iter_mut()) {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
            self.peaks = Some(PeakCache {
                path: file.path.clone(),
                node_var: self.node_var,
                link_var: self.link_var,
                nodes,
                links,
                max_abs_flow,
            });
        }

        let key = (file.path.clone(), self.node_var, self.link_var);
        if self.auto_breaks && self.breaks_key.as_ref() != Some(&key) {
            if let Some(p) = self.peaks.as_ref() {
                self.node_breaks = range_breaks(&p.nodes);
                self.link_breaks = range_breaks(&p.links);
            }
            self.breaks_key = Some(key);
        }
    }

    /// Recompute the breaks from the run now, whatever `auto_breaks` says.
    pub fn reset_breaks(&mut self, quantile: bool) {
        if let Some(p) = self.peaks.as_ref() {
            if quantile {
                self.node_breaks = ClassBreaks::quantiles(&p.nodes);
                self.link_breaks = ClassBreaks::quantiles(&p.links);
            } else {
                self.node_breaks = range_breaks(&p.nodes);
                self.link_breaks = range_breaks(&p.links);
            }
        }
    }
}

/// Equal-interval breaks from zero (or the minimum, when negative) to the max.
fn range_breaks(values: &[f64]) -> ClassBreaks {
    let lo = values
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
        .min(0.0);
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    ClassBreaks::equal_interval(lo, hi)
}

/// Unit label for a reported variable.
pub fn variable_unit(target: PlotTarget, var: usize, metric: bool, n_fixed: usize) -> &'static str {
    let (length, flow, velocity, volume) = if metric {
        ("m", "m³/s", "m/s", "m³")
    } else {
        ("ft", "cfs", "ft/s", "ft³")
    };
    if var >= n_fixed {
        return "conc.";
    }
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

/// Index of every `.out` object by id, since the `.inp` order differs.
fn out_indices(file: &OutputFile) -> (HashMap<&str, usize>, HashMap<&str, usize>) {
    (
        file.meta
            .node_ids
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_str(), i))
            .collect(),
        file.meta
            .link_ids
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_str(), i))
            .collect(),
    )
}

/// Arrow head at the middle of a polyline, pointing along it (or against it).
fn draw_arrow(painter: &egui::Painter, pts: &[Pos2], flip: bool, color: Color32) {
    if pts.len() < 2 {
        return;
    }
    let total: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    if total < 14.0 {
        return;
    }
    let mut remaining = total / 2.0;
    for w in pts.windows(2) {
        let seg = w[1] - w[0];
        let len = seg.length();
        if len >= remaining {
            let mid = w[0] + seg * (remaining / len.max(1e-6));
            let mut dir = seg / len.max(1e-6);
            if flip {
                dir = -dir;
            }
            let normal = Vec2::new(-dir.y, dir.x);
            let tip = mid + dir * 6.0;
            let base = mid - dir * 4.0;
            painter.add(egui::Shape::convex_polygon(
                vec![tip, base + normal * 4.5, base - normal * 4.5],
                color,
                Stroke::new(1.0_f32, Color32::from_black_alpha(160)),
            ));
            return;
        }
        remaining -= len;
    }
}

/// Repaint the map's links and nodes in result colours. See the module
/// documentation for where this is called from.
pub fn draw_results_overlay(painter: &egui::Painter, rect: Rect, state: &mut AppState, dark: bool) {
    let showing_frame = state.swmm.frame().is_some();
    let period = state.swmm.period;
    {
        let results = state.swmm.results.as_ref();
        state
            .swmm
            .results_overlay
            .refresh(results, showing_frame, period);
    }
    let (Some(model), Some(file)) = (state.swmm.model_inp.as_ref(), state.swmm.results.as_ref())
    else {
        return;
    };
    let ov = &state.swmm.results_overlay;
    let (Some(node_vals), Some(link_vals)) =
        (ov.node_values(showing_frame), ov.link_values(showing_frame))
    else {
        return;
    };
    let flows = ov.flows();
    let max_abs_flow = ov.peaks.as_ref().map_or(0.0, |p| p.max_abs_flow);
    let (node_idx, link_idx) = out_indices(file);
    let vp = &state.swmm.map_viewport;
    let selection = palette::canvas::selection(dark);
    let mut count = 0usize;

    for link in &model.links {
        let path = model.link_polyline(link);
        if path.len() < 2 {
            continue;
        }
        let Some(&i) = link_idx.get(link.id.as_str()) else {
            continue;
        };
        let Some(&v) = link_vals.get(i) else { continue };
        let color = class_color(ov.link_breaks.class_of(v));
        let flow = flows.as_ref().and_then(|f| f.get(i).copied());
        let width = if ov.width_by_flow && max_abs_flow > 0.0 {
            let q = flow.unwrap_or(v).abs();
            2.0 + 6.0 * (q / max_abs_flow).clamp(0.0, 1.0) as f32
        } else {
            3.0
        };
        let pts: Vec<Pos2> = path
            .iter()
            .map(|&(x, y)| vp.world_to_screen(rect, x, y))
            .collect();
        let hit = ov.query.enabled && ov.query.target == PlotTarget::Link && ov.query.matches(v);
        if hit {
            count += 1;
            painter.add(egui::Shape::line(
                pts.clone(),
                Stroke::new(width + 5.0, selection),
            ));
        }
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(width, color)));
        if ov.arrows {
            let flip = flow.is_some_and(|q| q < 0.0);
            draw_arrow(painter, &pts, flip, color);
        }
    }

    for node in &model.nodes {
        let Some((x, y)) = node.pos() else { continue };
        let Some(&i) = node_idx.get(node.id.as_str()) else {
            continue;
        };
        let Some(&v) = node_vals.get(i) else { continue };
        let center = vp.world_to_screen(rect, x, y);
        let color = class_color(ov.node_breaks.class_of(v));
        let r = match node.kind {
            NodeKind::Storage => 9.0_f32,
            _ => 7.0,
        };
        let hit = ov.query.enabled && ov.query.target == PlotTarget::Node && ov.query.matches(v);
        if hit {
            count += 1;
            painter.circle_stroke(center, r + 4.0, Stroke::new(3.0_f32, selection));
        }
        painter.circle_filled(center, r, color);
        painter.circle_stroke(center, r, Stroke::new(1.5_f32, palette::canvas::ink(dark)));
    }
    state.swmm.results_overlay.query.count = count;
}

/// The class legend and frame timestamp, bottom-left of `rect`.
pub fn legend(painter: &egui::Painter, rect: Rect, state: &AppState, dark: bool) {
    let Some(file) = state.swmm.results.as_ref() else {
        return;
    };
    let ov = &state.swmm.results_overlay;
    let metric = file.meta.flow_units.is_metric();
    let node_names = node_variable_names(&file.meta);
    let link_names = link_variable_names(&file.meta);
    let node_label = format!(
        "Nodes · {} ({})",
        node_names
            .get(ov.node_var)
            .map(String::as_str)
            .unwrap_or("?"),
        variable_unit(PlotTarget::Node, ov.node_var, metric, 6)
    );
    let link_label = format!(
        "Links · {} ({})",
        link_names
            .get(ov.link_var)
            .map(String::as_str)
            .unwrap_or("?"),
        variable_unit(PlotTarget::Link, ov.link_var, metric, 5)
    );
    let when = match state.swmm.frame() {
        Some(fr) => format!(
            "{}  ·  {:.2} h  ·  period {}/{}",
            format_datetime(fr.date_days),
            fr.time_s / 3600.0,
            fr.period + 1,
            state.swmm.n_periods()
        ),
        None => "run maxima".to_string(),
    };

    let pad = 8.0;
    let row_h = 15.0;
    let box_w = 230.0;
    let rows = 2 + 5 + 5 + 1 + if ov.query.enabled { 1 } else { 0 };
    let box_h = pad * 2.0 + row_h * rows as f32 + 8.0;
    let origin = Pos2::new(rect.left() + 12.0, rect.bottom() - 12.0 - box_h);
    let bg = Rect::from_min_size(origin, Vec2::new(box_w, box_h));
    painter.rect_filled(bg, 5.0, palette::canvas::panel_fill(dark));
    painter.rect_stroke(bg, 5.0, Stroke::new(1.0_f32, palette::canvas::line(dark)));

    let mut y = bg.top() + pad;
    let text = |painter: &egui::Painter, y: f32, s: &str, strong: bool| {
        painter.text(
            Pos2::new(bg.left() + pad, y),
            egui::Align2::LEFT_TOP,
            s,
            egui::FontId::proportional(if strong { 12.0 } else { 11.0 }),
            if strong {
                palette::canvas::ink(dark)
            } else {
                palette::canvas::muted(dark)
            },
        );
    };
    for (label, breaks, is_line) in [
        (node_label, &ov.node_breaks, false),
        (link_label, &ov.link_breaks, true),
    ] {
        text(painter, y, &label, true);
        y += row_h;
        let labels = breaks.labels(breaks.decimals());
        for (k, l) in labels.iter().enumerate() {
            let cy = y + row_h / 2.0;
            let mx = bg.left() + pad;
            if is_line {
                painter.line_segment(
                    [Pos2::new(mx, cy), Pos2::new(mx + 16.0, cy)],
                    Stroke::new(4.0_f32, class_color(k)),
                );
            } else {
                painter.circle_filled(Pos2::new(mx + 8.0, cy), 5.0, class_color(k));
            }
            painter.text(
                Pos2::new(mx + 24.0, cy),
                egui::Align2::LEFT_CENTER,
                l,
                egui::FontId::monospace(11.0),
                palette::canvas::muted(dark),
            );
            y += row_h;
        }
        y += 4.0;
    }
    text(painter, y, &when, false);
    y += row_h;
    if ov.query.enabled {
        let q = &ov.query;
        text(
            painter,
            y,
            &format!(
                "query: {} {} {:.3} → {} match",
                match q.target {
                    PlotTarget::Node => "nodes",
                    PlotTarget::Link => "links",
                },
                if q.above { ">" } else { "<" },
                q.value,
                q.count
            ),
            true,
        );
    }
}

fn breaks_editor(ui: &mut Ui, breaks: &mut ClassBreaks, enabled: bool) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            for (k, b) in breaks.breaks.iter_mut().enumerate() {
                let r = ui.add(
                    egui::DragValue::new(b)
                        .speed(0.01)
                        .max_decimals(3)
                        .prefix(if k == 0 { "" } else { "| " }),
                );
                changed |= r.changed();
            }
        });
    });
    if changed {
        breaks.normalize();
    }
    changed
}

/// Pickers, breaks, playback and query widgets.
pub fn draw_results_controls(ui: &mut Ui, state: &mut AppState) {
    let Some(file) = state.swmm.results.as_ref() else {
        ui.label("Run a model to colour the map by its results.");
        return;
    };
    let node_names = node_variable_names(&file.meta);
    let link_names = link_variable_names(&file.meta);
    let n_periods = file.meta.n_periods;

    ui.horizontal_wrapped(|ui| {
        let ov = &mut state.swmm.results_overlay;
        ui.label("Nodes");
        egui::ComboBox::from_id_salt("swmm-results-node-var")
            .selected_text(node_names.get(ov.node_var).cloned().unwrap_or_default())
            .width(120.0)
            .show_ui(ui, |ui| {
                for (i, n) in node_names.iter().enumerate() {
                    ui.selectable_value(&mut ov.node_var, i, n);
                }
            });
        ui.label("Links");
        egui::ComboBox::from_id_salt("swmm-results-link-var")
            .selected_text(link_names.get(ov.link_var).cloned().unwrap_or_default())
            .width(120.0)
            .show_ui(ui, |ui| {
                for (i, n) in link_names.iter().enumerate() {
                    ui.selectable_value(&mut ov.link_var, i, n);
                }
            });
        ui.checkbox(&mut ov.width_by_flow, "Width by flow");
        ui.checkbox(&mut ov.arrows, "Arrows");
        ui.separator();
        ui.checkbox(&mut ov.auto_breaks, "Auto breaks");
        if ui.small_button("Equal").clicked() {
            ov.reset_breaks(false);
        }
        if ui.small_button("Quantile").clicked() {
            ov.reset_breaks(true);
        }
    });
    ui.horizontal_wrapped(|ui| {
        let ov = &mut state.swmm.results_overlay;
        let editable = !ov.auto_breaks;
        ui.label("Node breaks");
        if breaks_editor(ui, &mut ov.node_breaks, editable) {
            ov.auto_breaks = false;
        }
        ui.label("Link breaks");
        if breaks_editor(ui, &mut ov.link_breaks, editable) {
            ov.auto_breaks = false;
        }
    });

    // Playback: the same state the SWMM tab drives, plus speed.
    if n_periods > 0 {
        ui.horizontal_wrapped(|ui| {
            let mut period = state.swmm.period;
            if ui
                .add(egui::Slider::new(&mut period, 0..=n_periods - 1).text("period"))
                .changed()
            {
                state.swmm.playing = false;
                state.swmm.set_period(period);
            }
            if state.swmm.playing {
                if ui.small_button("Pause").clicked() {
                    state.swmm.playing = false;
                }
            } else if ui.small_button("Play").clicked() {
                state.swmm.toggle_play();
            }
            if ui.small_button("◀").clicked() {
                state.swmm.playing = false;
                state.swmm.step(-1);
            }
            if ui.small_button("▶").clicked() {
                state.swmm.playing = false;
                state.swmm.step(1);
            }
            if ui.small_button("Peaks").clicked() {
                state.swmm.show_peaks();
            }
            ui.label("speed");
            let mut speed = state.swmm.speed_factor();
            if ui
                .add(
                    egui::Slider::new(&mut speed, 0.25..=8.0)
                        .logarithmic(true)
                        .suffix("×")
                        .fixed_decimals(2),
                )
                .changed()
            {
                state.swmm.play_speed = speed;
            }
            let when = match state.swmm.frame() {
                Some(fr) => format!(
                    "{}  ·  {:.2} h",
                    format_datetime(fr.date_days),
                    fr.time_s / 3600.0
                ),
                None => "showing run maxima".to_string(),
            };
            ui.label(when);
        });
    }

    ui.horizontal_wrapped(|ui| {
        let q = &mut state.swmm.results_overlay.query;
        ui.checkbox(&mut q.enabled, "Query");
        ui.add_enabled_ui(q.enabled, |ui| {
            ui.selectable_value(&mut q.target, PlotTarget::Node, "nodes");
            ui.selectable_value(&mut q.target, PlotTarget::Link, "links");
            ui.selectable_value(&mut q.above, true, ">");
            ui.selectable_value(&mut q.above, false, "<");
            ui.add(
                egui::DragValue::new(&mut q.value)
                    .speed(0.01)
                    .max_decimals(3),
            );
            ui.label(format!("{} match", q.count));
        });
    });
}

const CONTROLS_H: f32 = 96.0;

/// The "Results" sub-view: the map with the overlay, legend and controls.
pub fn draw_swmm_results_view(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), CONTROLS_H));
    let map_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + CONTROLS_H), rect.max);

    ui.painter_at(rect)
        .rect_filled(rect, 4.0, palette::canvas::bg(dark));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(strip.shrink(6.0)), |ui| {
        draw_results_controls(ui, state);
    });

    // The map's own pan and zoom are routed by sub-view elsewhere, so this
    // view takes the gestures itself, on the same viewport.
    let resp = ui.interact(
        map_rect,
        ui.id().with("swmm-results-map"),
        egui::Sense::click_and_drag(),
    );
    state.swmm.map_viewport.handle_pan_zoom(&resp, ui);
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            crate::swmm_panel::map_click(state, map_rect, pos);
        }
    }

    crate::swmm_panel::draw_swmm_map(ui, map_rect, state);
    let painter = ui.painter_at(map_rect);
    draw_results_overlay(&painter, map_rect, state, dark);
    legend(&painter, map_rect, state, dark);
}

impl crate::swmm_panel::SwmmState {
    /// Playback speed multiplier, 1× when unset.
    pub fn speed_factor(&self) -> f32 {
        if self.play_speed > 0.0 && self.play_speed.is_finite() {
            self.play_speed
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_panel::SwmmSubView;
    use crate::swmm_profile::tests::{pond_state, run_frame};
    use crate::StormSewerApp;

    #[test]
    fn results_view_renders_and_queries() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        app.state.view_tab = crate::state::ViewTab::Swmm;
        app.state.swmm.sub_view = SwmmSubView::Results;
        run_frame(&mut app);

        let mut app = StormSewerApp::new_for_test(pond_state());
        app.state.swmm.sub_view = SwmmSubView::Results;
        run_frame(&mut app);
        // Peaks mode: every node has a value and the breaks span the run.
        let ov = &app.state.swmm.results_overlay;
        let vals = ov.node_values(false).expect("peak values");
        assert_eq!(vals.len(), 14);
        assert!(ov.node_breaks.breaks[3] > ov.node_breaks.breaks[0]);

        // Query: nodes deeper than 1 ft at the run peak. The report lists
        // J9, J10, J11 and SU1 above 1 ft.
        app.state.swmm.results_overlay.query = Query {
            enabled: true,
            target: PlotTarget::Node,
            above: true,
            value: 1.0,
            count: 0,
        };
        run_frame(&mut app);
        assert_eq!(app.state.swmm.results_overlay.query.count, 4);

        // An instant: the frame's values, and the count changes with it.
        app.state.swmm.set_period(0);
        run_frame(&mut app);
        // J11 sits behind a 4 ft outlet offset and is deep from the start,
        // so the first period is not dry everywhere, but fewer nodes match
        // than at the run peak.
        let first = app.state.swmm.results_overlay.query.count;
        assert!(first < 4, "{first} nodes over 1 ft at the first period");
        let vals = app.state.swmm.results_overlay.link_values(true).unwrap();
        assert_eq!(vals.len(), 14);

        // Switching variables re-derives the breaks automatically.
        let before = app.state.swmm.results_overlay.link_breaks;
        app.state.swmm.results_overlay.link_var = 4; // capacity, 0..1
        run_frame(&mut app);
        let after = app.state.swmm.results_overlay.link_breaks;
        assert_ne!(before, after);
        assert!(after.breaks[3] <= 1.0);
        app.state.swmm.results_overlay.width_by_flow = true;
        app.state.swmm.results_overlay.arrows = true;
        run_frame(&mut app);
    }

    #[test]
    fn class_colours_and_units() {
        assert_eq!(class_color(0), RAMP[0]);
        assert_eq!(class_color(99), RAMP[4]);
        assert_eq!(variable_unit(PlotTarget::Link, 0, false, 5), "cfs");
        assert_eq!(variable_unit(PlotTarget::Link, 0, true, 5), "m³/s");
        assert_eq!(variable_unit(PlotTarget::Node, 6, true, 6), "conc.");
        let q = Query {
            enabled: true,
            target: PlotTarget::Link,
            above: false,
            value: 2.0,
            count: 0,
        };
        assert!(q.matches(1.0) && !q.matches(2.0));
    }
}
