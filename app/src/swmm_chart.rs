// SPDX-License-Identifier: GPL-3.0-or-later

//! Time-series plots for SWMM results: several series on one plot, a scatter
//! of one variable against another, a cursor readout, statistics with the
//! top-N peaks, and CSV/PNG export.
//!
//! Reached from the SWMM tab's "Plots" view button. Series are added from the
//! pickers in the strip above the plot, or with "Add selected", which takes
//! whatever the map or chart selection is (`plot_target`, `plot_id`,
//! `plot_var`).

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui, Vec2};

use stormsewer_swmm::out::{
    link_series, link_variable_names, node_series, node_variable_names, OutputFile, Series,
};
use stormsewer_swmm::results::{stats, top_peaks, SeriesStats};

use crate::profile::station_tick_step;
use crate::state::AppState;
use crate::swmm_export::{save_csv, ExportState};
use crate::swmm_panel::PlotTarget;
use crate::swmm_results::variable_unit;
use crate::theme::palette;

const STRIP_H: f32 = 34.0;
const CHIPS_H: f32 = 28.0;
const STATS_W: f32 = 240.0;
const PAD_LEFT: f32 = 68.0;
const PAD_RIGHT: f32 = 20.0;
const PAD_TOP: f32 = 36.0;
const PAD_BOTTOM: f32 = 44.0;

/// Series colours, cycled.
const SERIES_COLORS: [Color32; 8] = [
    Color32::from_rgb(80, 160, 255),
    Color32::from_rgb(224, 86, 127),
    Color32::from_rgb(96, 200, 120),
    Color32::from_rgb(226, 162, 60),
    Color32::from_rgb(180, 120, 255),
    Color32::from_rgb(60, 200, 200),
    Color32::from_rgb(255, 180, 60),
    Color32::from_rgb(200, 200, 200),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartMode {
    #[default]
    Time,
    Scatter,
}

/// One plotted series and where it came from.
#[derive(Clone, Debug)]
pub struct ChartSeries {
    pub target: PlotTarget,
    pub id: String,
    pub var: usize,
    pub label: String,
    pub unit: &'static str,
    pub series: Series,
}

pub struct SwmmChartState {
    pub items: Vec<ChartSeries>,
    pub mode: ChartMode,
    pub scatter_x: usize,
    pub scatter_y: usize,
    /// Which series the statistics column describes.
    pub focus: usize,
    pub top_n: usize,
    pub add_target: PlotTarget,
    pub add_id: Option<String>,
    pub add_var: usize,
    pub export: ExportState,
    pub message: String,
    /// Height the control strip actually needed when it was last drawn.
    ///
    /// The strip wraps with the panel width, and laying the chips band and the
    /// plot out under a one-row assumption is what put the wrapped row on top
    /// of both. Measured on one frame, used on the next.
    pub strip_h: f32,
}

impl Default for SwmmChartState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            mode: ChartMode::Time,
            scatter_x: 0,
            scatter_y: 1,
            focus: 0,
            top_n: 5,
            add_target: PlotTarget::Node,
            add_id: None,
            add_var: 0,
            export: ExportState::default(),
            message: String::new(),
            strip_h: STRIP_H,
        }
    }
}

impl SwmmChartState {
    /// Read a series from the results and add it, unless it is already
    /// plotted. Returns the index either way.
    pub fn add(
        &mut self,
        file: &OutputFile,
        target: PlotTarget,
        id: &str,
        var: usize,
    ) -> Result<usize, String> {
        if let Some(i) = self
            .items
            .iter()
            .position(|s| s.target == target && s.id == id && s.var == var)
        {
            self.focus = i;
            return Ok(i);
        }
        let metric = file.meta.flow_units.is_metric();
        let (series, name, unit) = match target {
            PlotTarget::Node => (
                node_series(&file.path, &file.meta, id, var),
                node_variable_names(&file.meta).get(var).cloned(),
                variable_unit(PlotTarget::Node, var, metric, 6),
            ),
            PlotTarget::Link => (
                link_series(&file.path, &file.meta, id, var),
                link_variable_names(&file.meta).get(var).cloned(),
                variable_unit(PlotTarget::Link, var, metric, 5),
            ),
        };
        let series = series.map_err(|e| e.to_string())?;
        let name = name.ok_or_else(|| format!("variable {var} is not reported"))?;
        self.items.push(ChartSeries {
            target,
            id: id.to_string(),
            var,
            label: format!("{id} · {name}"),
            unit,
            series,
        });
        self.focus = self.items.len() - 1;
        Ok(self.focus)
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
        }
        let n = self.items.len();
        self.focus = self.focus.min(n.saturating_sub(1));
        self.scatter_x = self.scatter_x.min(n.saturating_sub(1));
        self.scatter_y = self.scatter_y.min(n.saturating_sub(1));
    }

    /// Forget everything: the series belong to a run.
    pub fn clear(&mut self) {
        self.items.clear();
        self.focus = 0;
    }

    /// All series as CSV: time columns then one column per series, rows by
    /// reporting period. Series from one run share a time base; a shorter
    /// one leaves blanks.
    pub fn csv(&self) -> String {
        let longest = self
            .items
            .iter()
            .max_by_key(|s| s.series.times_s.len())
            .map(|s| s.series.times_s.clone())
            .unwrap_or_default();
        let mut header = vec!["time_s".to_string(), "time_h".to_string()];
        header.extend(
            self.items
                .iter()
                .map(|s| format!("{} ({})", s.label, s.unit)),
        );
        let mut out = stormsewer_swmm::rpt::csv_line(&header);
        for (k, t) in longest.iter().enumerate() {
            let mut row = vec![format!("{t:.0}"), format!("{:.4}", t / 3600.0)];
            for s in &self.items {
                row.push(
                    s.series
                        .values
                        .get(k)
                        .map(|v| format!("{v:.6}"))
                        .unwrap_or_default(),
                );
            }
            out.push_str(&stormsewer_swmm::rpt::csv_line(&row));
        }
        out
    }

    pub fn focused(&self) -> Option<&ChartSeries> {
        self.items.get(self.focus)
    }
}

fn nice_range(lo: f64, hi: f64) -> (f64, f64) {
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    if hi - lo < 1e-9 {
        (lo - 0.5, hi + 0.5)
    } else {
        let pad = (hi - lo) * 0.05;
        (lo - pad, hi + pad)
    }
}

/// Controls above the plot. Returns the height the controls actually used,
/// which is more than one row once they wrap.
fn draw_strip(ui: &mut Ui, strip: Rect, plot_rect: Rect, state: &mut AppState) -> f32 {
    let (node_ids, link_ids, node_vars, link_vars) = match state.swmm.results.as_ref() {
        Some(f) => (
            f.meta.node_ids.clone(),
            f.meta.link_ids.clone(),
            node_variable_names(&f.meta),
            link_variable_names(&f.meta),
        ),
        None => Default::default(),
    };
    let mut used = STRIP_H;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(strip), |ui| {
        ui.horizontal_wrapped(|ui| {
            let has_results = state.swmm.results.is_some();
            ui.add_enabled_ui(has_results, |ui| {
                let c = &mut state.swmm.chart;
                for (t, label) in [(PlotTarget::Node, "Node"), (PlotTarget::Link, "Link")] {
                    if ui.selectable_label(c.add_target == t, label).clicked() && c.add_target != t
                    {
                        c.add_target = t;
                        c.add_id = None;
                        c.add_var = 0;
                    }
                }
                let (ids, vars) = match c.add_target {
                    PlotTarget::Node => (&node_ids, &node_vars),
                    PlotTarget::Link => (&link_ids, &link_vars),
                };
                if c.add_id.is_none() {
                    c.add_id = ids.first().cloned();
                }
                egui::ComboBox::from_id_salt("swmm-chart-add-id")
                    .selected_text(c.add_id.clone().unwrap_or_default())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for id in ids {
                            if ui
                                .selectable_label(c.add_id.as_deref() == Some(id.as_str()), id)
                                .clicked()
                            {
                                c.add_id = Some(id.clone());
                            }
                        }
                    });
                egui::ComboBox::from_id_salt("swmm-chart-add-var")
                    .selected_text(vars.get(c.add_var).cloned().unwrap_or_default())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for (i, v) in vars.iter().enumerate() {
                            ui.selectable_value(&mut c.add_var, i, v);
                        }
                    });
            });
            let mut to_add: Option<(PlotTarget, String, usize)> = None;
            if ui.small_button("Add").clicked() {
                let c = &state.swmm.chart;
                if let Some(id) = c.add_id.clone() {
                    to_add = Some((c.add_target, id, c.add_var));
                }
            }
            if ui.small_button("Add selected").clicked() {
                if let Some(id) = state.swmm.plot_id.clone() {
                    to_add = Some((state.swmm.plot_target, id, state.swmm.plot_var));
                }
            }
            if let (Some((t, id, var)), Some(file)) = (to_add, state.swmm.results.as_ref()) {
                if let Err(e) = state.swmm.chart.add(file, t, &id, var) {
                    state.swmm.chart.message = e;
                }
            }
            if ui.small_button("Clear").clicked() {
                state.swmm.chart.clear();
            }
            ui.separator();
            let c = &mut state.swmm.chart;
            ui.selectable_value(&mut c.mode, ChartMode::Time, "Time");
            ui.selectable_value(&mut c.mode, ChartMode::Scatter, "Scatter");
            if c.mode == ChartMode::Scatter && c.items.len() >= 2 {
                let labels: Vec<String> = c.items.iter().map(|s| s.label.clone()).collect();
                ui.label("x");
                egui::ComboBox::from_id_salt("swmm-chart-sx")
                    .selected_text(labels.get(c.scatter_x).cloned().unwrap_or_default())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for (i, l) in labels.iter().enumerate() {
                            ui.selectable_value(&mut c.scatter_x, i, l);
                        }
                    });
                ui.label("y");
                egui::ComboBox::from_id_salt("swmm-chart-sy")
                    .selected_text(labels.get(c.scatter_y).cloned().unwrap_or_default())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for (i, l) in labels.iter().enumerate() {
                            ui.selectable_value(&mut c.scatter_y, i, l);
                        }
                    });
            }
            ui.separator();
            if ui
                .add_enabled(
                    !c.export.is_pending(),
                    egui::Button::new("Export PNG").small(),
                )
                .clicked()
            {
                c.export.request_png(ui.ctx(), plot_rect, "swmm-plot.png");
            }
            if ui.small_button("Export CSV").clicked() {
                if c.items.is_empty() {
                    c.message = "Nothing to export".to_string();
                } else if let Some(msg) = save_csv("swmm-series.csv", &c.csv()) {
                    c.message = msg;
                }
            }
            let msg = if c.export.message.is_empty() {
                c.message.clone()
            } else {
                c.export.message.clone()
            };
            if !msg.is_empty() {
                ui.label(msg);
            }
        });
        used = ui.min_rect().height();
    });
    used.max(STRIP_H)
}

fn draw_chips(ui: &mut Ui, chips: Rect, state: &mut AppState) {
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(chips), |ui| {
        ui.horizontal_wrapped(|ui| {
            let c = &mut state.swmm.chart;
            let mut remove: Option<usize> = None;
            for (i, s) in c.items.iter().enumerate() {
                let color = SERIES_COLORS[i % SERIES_COLORS.len()];
                let text = egui::RichText::new(format!("■ {}", s.label)).color(color);
                if ui.selectable_label(c.focus == i, text).clicked() {
                    c.focus = i;
                }
                if ui.small_button("✕").clicked() {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                c.remove(i);
            }
            if c.items.is_empty() {
                ui.label("No series yet — pick a node or link and Add.");
            }
        });
    });
}

fn fmt_h(t_s: f64) -> String {
    format!("{:.2} h", t_s / 3600.0)
}

fn draw_stats(ui: &mut Ui, col: Rect, state: &mut AppState) {
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(col.shrink(6.0)), |ui| {
        let c = &mut state.swmm.chart;
        ui.label(egui::RichText::new("Statistics").strong());
        let Some(s) = c.focused() else {
            ui.label("Select a series.");
            return;
        };
        ui.label(&s.label);
        let Some(st) = series_stats(s) else {
            ui.label("no finite values");
            return;
        };
        let u = s.unit;
        egui::Grid::new("swmm-chart-stats")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("samples");
                ui.label(format!("{}", st.n));
                ui.end_row();
                ui.label("min");
                ui.label(format!("{:.4} {u} at {}", st.min, fmt_h(st.min_at_s)));
                ui.end_row();
                ui.label("mean");
                ui.label(format!("{:.4} {u}", st.mean));
                ui.end_row();
                ui.label("max");
                ui.label(format!("{:.4} {u} at {}", st.max, fmt_h(st.max_at_s)));
                ui.end_row();
                ui.label("total (Σ)");
                ui.label(format!("{:.4} {u}", st.sum));
                ui.end_row();
                ui.label("∫ v·dt");
                ui.label(format!("{:.1} {u}·s", st.integral));
                ui.end_row();
            });
        ui.horizontal(|ui| {
            ui.label("Top peaks");
            ui.add(egui::DragValue::new(&mut c.top_n).range(1..=50));
        });
        let s = c.focused().expect("focused series exists");
        let peaks = top_peaks(&s.series, c.top_n);
        if peaks.is_empty() {
            ui.label("no local maxima");
        }
        for (k, (t, v)) in peaks.iter().enumerate() {
            ui.label(format!("{}. {v:.4} {u} at {}", k + 1, fmt_h(*t)));
        }
    });
}

/// Statistics for a series, for callers that want the numbers without the
/// widgets (tests, reports).
pub fn series_stats(s: &ChartSeries) -> Option<SeriesStats> {
    stats(&s.series)
}

/// Draw the plots view.
pub fn draw_swmm_plots(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    state.swmm.chart.export.poll(ui.ctx());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));

    // Last frame's measured height. The chips band sits between the strip and
    // the plot, so a wrapped strip laid out as one row prints over the chips
    // and the empty-state text alike; all three rects move together.
    let strip_h = state.swmm.chart.strip_h.max(STRIP_H);
    let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), strip_h));
    let chips = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + strip_h),
        Vec2::new(rect.width(), CHIPS_H),
    );
    let body_top = rect.top() + strip_h + CHIPS_H;
    let stats_col = Rect::from_min_max(Pos2::new(rect.right() - STATS_W, body_top), rect.max);
    let plot_rect = Rect::from_min_max(
        Pos2::new(rect.left(), body_top),
        Pos2::new(stats_col.left(), rect.bottom()),
    );

    let used = draw_strip(ui, strip, plot_rect, state);
    state.swmm.chart.strip_h = used;
    draw_chips(ui, chips, state);
    draw_stats(ui, stats_col, state);

    let empty_state = |line: &str| {
        painter.text(
            plot_rect.center(),
            egui::Align2::CENTER_CENTER,
            line,
            egui::FontId::proportional(15.0),
            palette::canvas::muted(dark),
        );
    };
    if state.swmm.results.is_none() {
        empty_state("Run a SWMM model to plot its results");
        return;
    }
    let c = &state.swmm.chart;
    if c.items.is_empty() {
        empty_state("Add a series above");
        return;
    }
    let inner = Rect::from_min_max(
        Pos2::new(plot_rect.left() + PAD_LEFT, plot_rect.top() + PAD_TOP),
        Pos2::new(
            plot_rect.right() - PAD_RIGHT,
            plot_rect.bottom() - PAD_BOTTOM,
        ),
    );
    if inner.width() < 40.0 || inner.height() < 40.0 {
        return;
    }
    match c.mode {
        ChartMode::Time => draw_time_plot(ui, &painter, plot_rect, inner, c, dark),
        ChartMode::Scatter => draw_scatter(ui, &painter, plot_rect, inner, c, dark),
    }
}

/// Ranges and titles of a plot's two axes.
struct AxisSpec<'a> {
    x: (f64, f64),
    y: (f64, f64),
    x_title: &'a str,
    y_title: &'a str,
}

fn axis(painter: &egui::Painter, inner: Rect, dark: bool, spec: AxisSpec<'_>, plot_rect: Rect) {
    let AxisSpec {
        x: (x0, x1),
        y: (y0, y1),
        x_title,
        y_title,
    } = spec;
    let x_at = |v: f64| inner.left() + ((v - x0) / (x1 - x0)) as f32 * inner.width();
    let y_at = |v: f64| inner.bottom() - ((v - y0) / (y1 - y0)) as f32 * inner.height();
    let y_step = station_tick_step(y1 - y0);
    let mut v = (y0 / y_step).ceil() * y_step;
    while v <= y1 + y_step * 0.01 {
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
        v += y_step;
    }
    let x_step = station_tick_step(x1 - x0);
    painter.line_segment(
        [
            Pos2::new(inner.left(), inner.bottom()),
            Pos2::new(inner.right(), inner.bottom()),
        ],
        Stroke::new(1.0_f32, palette::canvas::line(dark)),
    );
    let mut t = (x0 / x_step).ceil() * x_step;
    while t <= x1 + x_step * 0.01 {
        let x = x_at(t);
        painter.line_segment(
            [
                Pos2::new(x, inner.bottom()),
                Pos2::new(x, inner.bottom() + 5.0),
            ],
            Stroke::new(1.0_f32, palette::canvas::line(dark)),
        );
        painter.text(
            Pos2::new(x, inner.bottom() + 8.0),
            egui::Align2::CENTER_TOP,
            format!("{t:.2}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        t += x_step;
    }
    painter.text(
        Pos2::new(inner.center().x, plot_rect.bottom() - 4.0),
        egui::Align2::CENTER_BOTTOM,
        x_title,
        egui::FontId::proportional(11.0),
        palette::canvas::muted(dark),
    );
    painter.text(
        Pos2::new(plot_rect.left() + 10.0, plot_rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        y_title,
        egui::FontId::proportional(11.0),
        palette::canvas::muted(dark),
    );
}

fn draw_time_plot(
    ui: &Ui,
    painter: &egui::Painter,
    plot_rect: Rect,
    inner: Rect,
    c: &SwmmChartState,
    dark: bool,
) {
    let mut t0 = f64::INFINITY;
    let mut t1 = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in &c.items {
        for (t, v) in s.series.times_s.iter().zip(&s.series.values) {
            let h = t / 3600.0;
            t0 = t0.min(h);
            t1 = t1.max(h);
            if v.is_finite() {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
    }
    if t1 <= t0 || !t1.is_finite() || !t0.is_finite() || !lo.is_finite() {
        return;
    }
    let (lo, hi) = nice_range(lo, hi);
    let units: Vec<&str> = {
        let mut u: Vec<&str> = c.items.iter().map(|s| s.unit).collect();
        u.dedup();
        u
    };
    axis(
        painter,
        inner,
        dark,
        AxisSpec {
            x: (t0, t1),
            y: (lo, hi),
            x_title: "Time (hours from start)",
            y_title: &format!("Value ({})", units.join(", ")),
        },
        plot_rect,
    );
    let x_at = |h: f64| inner.left() + ((h - t0) / (t1 - t0)) as f32 * inner.width();
    let y_at = |v: f64| inner.bottom() - ((v - lo) / (hi - lo)) as f32 * inner.height();

    for (i, s) in c.items.iter().enumerate() {
        let color = SERIES_COLORS[i % SERIES_COLORS.len()];
        let width = if i == c.focus { 2.5_f32 } else { 1.5_f32 };
        let pts: Vec<Pos2> = s
            .series
            .times_s
            .iter()
            .zip(&s.series.values)
            .map(|(t, v)| Pos2::new(x_at(t / 3600.0), y_at(*v)))
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(width, color)));
    }

    // Legend, top-right inside the plot.
    let mut lp = Pos2::new(inner.right() - 200.0, inner.top() + 6.0);
    for (i, s) in c.items.iter().enumerate() {
        let color = SERIES_COLORS[i % SERIES_COLORS.len()];
        painter.line_segment(
            [
                Pos2::new(lp.x, lp.y + 6.0),
                Pos2::new(lp.x + 20.0, lp.y + 6.0),
            ],
            Stroke::new(2.5_f32, color),
        );
        painter.text(
            Pos2::new(lp.x + 26.0, lp.y + 6.0),
            egui::Align2::LEFT_CENTER,
            format!("{} ({})", s.label, s.unit),
            egui::FontId::proportional(11.0),
            palette::canvas::ink(dark),
        );
        lp.y += 15.0;
    }

    // Cursor readout.
    let hover = ui.input(|i| i.pointer.hover_pos());
    if let Some(p) = hover.filter(|p| inner.contains(*p)) {
        let h = t0 + (p.x - inner.left()) as f64 / inner.width() as f64 * (t1 - t0);
        painter.line_segment(
            [Pos2::new(p.x, inner.top()), Pos2::new(p.x, inner.bottom())],
            Stroke::new(1.0_f32, palette::canvas::selection(dark)),
        );
        let mut lines = vec![format!("t = {h:.3} h")];
        for s in &c.items {
            if let Some((_, v)) = nearest_sample(&s.series, h * 3600.0) {
                lines.push(format!("{}: {v:.4} {}", s.label, s.unit));
            }
        }
        let box_w = 240.0;
        let x = if p.x + 12.0 + box_w > inner.right() {
            p.x - 12.0 - box_w
        } else {
            p.x + 12.0
        };
        let origin = Pos2::new(x, (p.y - 10.0).max(inner.top()));
        let bg = Rect::from_min_size(origin, Vec2::new(box_w, 14.0 * lines.len() as f32 + 10.0));
        painter.rect_filled(bg, 4.0, palette::canvas::panel_fill(dark));
        painter.rect_stroke(bg, 4.0, Stroke::new(1.0_f32, palette::canvas::line(dark)));
        for (i, line) in lines.iter().enumerate() {
            painter.text(
                origin + Vec2::new(6.0, 5.0 + 14.0 * i as f32),
                egui::Align2::LEFT_TOP,
                line,
                egui::FontId::monospace(11.0),
                palette::canvas::ink(dark),
            );
        }
    }
}

/// The sample nearest to `t_s`, as `(time_s, value)`.
pub fn nearest_sample(s: &Series, t_s: f64) -> Option<(f64, f64)> {
    let i = s
        .times_s
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - t_s).abs().total_cmp(&(b.1 - t_s).abs()))?
        .0;
    Some((s.times_s[i], *s.values.get(i)?))
}

fn draw_scatter(
    ui: &Ui,
    painter: &egui::Painter,
    plot_rect: Rect,
    inner: Rect,
    c: &SwmmChartState,
    dark: bool,
) {
    let (Some(sx), Some(sy)) = (c.items.get(c.scatter_x), c.items.get(c.scatter_y)) else {
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            "Add two series to scatter one against the other",
            egui::FontId::proportional(14.0),
            palette::canvas::muted(dark),
        );
        return;
    };
    let n = sx.series.values.len().min(sy.series.values.len());
    let pairs: Vec<(f64, f64)> = (0..n)
        .map(|i| (sx.series.values[i], sy.series.values[i]))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .collect();
    if pairs.is_empty() {
        return;
    }
    let (x0, x1) = nice_range(
        pairs.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        pairs.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
    );
    let (y0, y1) = nice_range(
        pairs.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        pairs.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    );
    axis(
        painter,
        inner,
        dark,
        AxisSpec {
            x: (x0, x1),
            y: (y0, y1),
            x_title: &format!("{} ({})", sx.label, sx.unit),
            y_title: &format!("{} ({})", sy.label, sy.unit),
        },
        plot_rect,
    );
    let x_at = |v: f64| inner.left() + ((v - x0) / (x1 - x0)) as f32 * inner.width();
    let y_at = |v: f64| inner.bottom() - ((v - y0) / (y1 - y0)) as f32 * inner.height();
    let color = SERIES_COLORS[c.scatter_y % SERIES_COLORS.len()];
    for (a, b) in &pairs {
        painter.circle_filled(Pos2::new(x_at(*a), y_at(*b)), 2.5, color);
    }
    // Cursor: nearest point.
    let hover = ui.input(|i| i.pointer.hover_pos());
    if let Some(p) = hover.filter(|p| inner.contains(*p)) {
        if let Some((i, (a, b))) = pairs.iter().enumerate().min_by(|u, v| {
            let du = (Pos2::new(x_at(u.1 .0), y_at(u.1 .1)) - p).length();
            let dv = (Pos2::new(x_at(v.1 .0), y_at(v.1 .1)) - p).length();
            du.total_cmp(&dv)
        }) {
            let q = Pos2::new(x_at(*a), y_at(*b));
            painter.circle_stroke(
                q,
                5.0,
                Stroke::new(1.5_f32, palette::canvas::selection(dark)),
            );
            let t = sx.series.times_s.get(i).copied().unwrap_or(0.0);
            painter.text(
                q + Vec2::new(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                format!("({a:.4}, {b:.4}) at {}", fmt_h(t)),
                egui::FontId::monospace(11.0),
                palette::canvas::ink(dark),
            );
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
    fn plots_view_renders_with_mixed_series_and_exports_csv() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        app.state.view_tab = crate::state::ViewTab::Swmm;
        app.state.swmm.sub_view = SwmmSubView::Plots;
        run_frame(&mut app);

        let mut app = StormSewerApp::new_for_test(pond_state());
        app.state.swmm.sub_view = SwmmSubView::Plots;
        run_frame(&mut app);
        {
            let file = app.state.swmm.results.clone().unwrap();
            let c = &mut app.state.swmm.chart;
            c.add(&file, PlotTarget::Node, "J11", 0).unwrap();
            c.add(&file, PlotTarget::Link, "C11", 0).unwrap();
            // Adding the same series again does not duplicate it.
            assert_eq!(c.add(&file, PlotTarget::Node, "J11", 0).unwrap(), 0);
            assert_eq!(c.items.len(), 2);
            assert!(c.add(&file, PlotTarget::Node, "nope", 0).is_err());
        }
        run_frame(&mut app);
        app.state.swmm.chart.mode = ChartMode::Scatter;
        run_frame(&mut app);

        let c = &app.state.swmm.chart;
        let csv = c.csv();
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "time_s,time_h,J11 · Depth (ft),C11 · Flow (cfs)"
        );
        assert_eq!(csv.lines().count(), 1 + 144);

        let st = series_stats(&c.items[0]).unwrap();
        assert!((st.max - 4.50).abs() < 0.01, "J11 max depth {}", st.max);
        assert!(st.min >= 0.0 && st.mean > 0.0);
        let peaks = top_peaks(&c.items[1].series, 3);
        assert!(!peaks.is_empty());
        assert!(peaks[0].1 > 30.0);
        assert_eq!(nearest_sample(&c.items[0].series, 0.0).unwrap().0, 300.0);

        app.state.swmm.chart.remove(0);
        assert_eq!(app.state.swmm.chart.items.len(), 1);
        run_frame(&mut app);
        app.state.swmm.chart.clear();
        run_frame(&mut app);
    }
}
