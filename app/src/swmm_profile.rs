// SPDX-License-Identifier: GPL-3.0-or-later

//! SWMM profile view: a long-section between two nodes with ground, conduit
//! inverts and crowns, node shafts, the HGL at the animated instant and the
//! maximum-HGL envelope of the run.
//!
//! Reached from the SWMM tab's "Profile" view button. The path is picked from
//! two node dropdowns in the strip above the plot; a map tool can hand a
//! path in through [`SwmmProfileState::set_path`].
//!
//! Geometry comes from `stormsewer_swmm::profile`, read from the model file on
//! disk (the file the engine ran). The instant shown is the reporting period
//! the map animation is on (`SwmmState::period`), so the one slider moves
//! both views.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui, Vec2};

use stormsewer_swmm::doc::InpDoc;
use stormsewer_swmm::profile::{NodeKind, Profile, ProfileNetwork};

// Both: the station axis has the full panel width and wants the plain
// range-derived step; only the elevation axis is squeezed by the single-scale
// rule and needs the pixel-aware one.
use crate::profile::{legible_tick_step, station_tick_step};
use crate::state::AppState;
use crate::swmm_export::{save_csv, ExportState};
use crate::theme::palette;

const STRIP_H: f32 = 34.0;
const PAD_LEFT: f32 = 64.0;
const PAD_RIGHT: f32 = 24.0;
const PAD_TOP: f32 = 40.0;
const PAD_BOTTOM: f32 = 48.0;
/// Node shaft width on screen.
const SHAFT_W: f32 = 10.0;

/// The profile view's own state.
pub struct SwmmProfileState {
    pub start: Option<String>,
    pub end: Option<String>,
    /// Vertical exaggeration: elevation scale ÷ station scale.
    pub v_exag: f32,
    pub show_max: bool,
    net: Option<ProfileNetwork>,
    net_path: Option<PathBuf>,
    profile: Option<Profile>,
    profile_key: Option<(String, String)>,
    pub error: Option<String>,
    pub export: ExportState,
    /// Height the control strip actually needed when it was last drawn.
    ///
    /// The strip wraps, so its height depends on the panel width. Measuring it
    /// and using the measurement on the next frame keeps the plot (and the
    /// title painted at the top of it) clear of a wrapped second row, which
    /// otherwise overprints both.
    pub strip_h: f32,
}

impl Default for SwmmProfileState {
    fn default() -> Self {
        Self {
            start: None,
            end: None,
            v_exag: 5.0,
            show_max: true,
            net: None,
            net_path: None,
            profile: None,
            profile_key: None,
            error: None,
            export: ExportState::default(),
            strip_h: STRIP_H,
        }
    }
}

impl SwmmProfileState {
    /// Choose the path. Entry point for a map tool: pass the two node ids and
    /// the view rebuilds on its next frame.
    pub fn set_path(&mut self, start: impl Into<String>, end: impl Into<String>) {
        self.start = Some(start.into());
        self.end = Some(end.into());
        self.profile = None;
        self.profile_key = None;
        self.error = None;
    }

    pub fn network(&self) -> Option<&ProfileNetwork> {
        self.net.as_ref()
    }

    pub fn profile(&self) -> Option<&Profile> {
        self.profile.as_ref()
    }

    /// Read the model's geometry when the model path changes.
    pub fn ensure_network(&mut self, model: Option<&Path>) {
        if self.net_path.as_deref() == model && (self.net.is_some() || model.is_none()) {
            return;
        }
        self.net_path = model.map(Path::to_path_buf);
        self.net = None;
        self.profile = None;
        self.profile_key = None;
        let Some(path) = model else { return };
        match InpDoc::read(path) {
            Ok(doc) => {
                let net = ProfileNetwork::from_doc(&doc);
                // A model has a natural first path: its first node to the
                // first outfall. Offer it rather than an empty plot.
                if self.start.is_none() || self.end.is_none() {
                    self.start = net.nodes.first().map(|n| n.id.clone());
                    self.end = net
                        .nodes
                        .iter()
                        .find(|n| n.kind == NodeKind::Outfall)
                        .or(net.nodes.last())
                        .map(|n| n.id.clone());
                }
                self.net = Some(net);
            }
            Err(e) => self.error = Some(format!("model could not be read: {e}")),
        }
    }

    /// Build the profile for the chosen path unless it is already built.
    pub fn ensure_profile(&mut self) {
        let (Some(net), Some(start), Some(end)) = (self.net.as_ref(), &self.start, &self.end)
        else {
            self.profile = None;
            return;
        };
        let key = (start.clone(), end.clone());
        if self.profile_key.as_ref() == Some(&key) {
            return;
        }
        self.profile_key = Some(key);
        match net.build(start, end) {
            Ok(p) => {
                self.profile = Some(p);
                self.error = None;
            }
            Err(e) => {
                self.profile = None;
                self.error = Some(e);
            }
        }
    }
}

/// HGL at each profile node for the current frame and the run's peaks.
///
/// Both are `depth + invert`, as the task defines them; `None` where the
/// results file does not report the node.
pub fn hgl_columns(state: &AppState, profile: &Profile) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    let out_index: HashMap<&str, usize> = state
        .swmm
        .results
        .as_ref()
        .map(|f| {
            f.meta
                .node_ids
                .iter()
                .enumerate()
                .map(|(i, s)| (s.as_str(), i))
                .collect()
        })
        .unwrap_or_default();
    let peaks: HashMap<&str, f64> = state
        .swmm
        .node_peaks
        .iter()
        .map(|p| (p.id.as_str(), p.max_depth))
        .collect();
    let frame = state.swmm.frame();
    let now = profile
        .nodes
        .iter()
        .map(|n| {
            let i = *out_index.get(n.node.as_str())?;
            let depth = frame?.nodes.get(i)?.depth;
            Some(n.invert + depth)
        })
        .collect();
    let max = profile
        .nodes
        .iter()
        .map(|n| peaks.get(n.node.as_str()).map(|d| n.invert + d))
        .collect();
    (now, max)
}

/// The station table with both HGL columns, for export.
pub fn profile_csv(state: &AppState) -> Option<String> {
    let p = state.swmm.profile.profile()?;
    let (now, max) = hgl_columns(state, p);
    Some(p.to_csv(&now, &max))
}

fn node_combo(ui: &mut Ui, id: &str, current: &mut Option<String>, names: &[String]) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.clone().unwrap_or_else(|| "—".to_string()))
        .width(120.0)
        .show_ui(ui, |ui| {
            for n in names {
                if ui
                    .selectable_label(current.as_deref() == Some(n.as_str()), n)
                    .clicked()
                {
                    *current = Some(n.clone());
                    changed = true;
                }
            }
        });
    changed
}

/// Controls above the plot. Returns the height the controls actually used,
/// which is more than one row once they wrap.
fn draw_strip(ui: &mut Ui, strip: Rect, plot_rect: Rect, state: &mut AppState) -> f32 {
    let names: Vec<String> = state
        .swmm
        .profile
        .network()
        .map(|n| n.nodes.iter().map(|n| n.id.clone()).collect())
        .unwrap_or_default();
    let mut used = STRIP_H;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(strip), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("From");
            let mut start = state.swmm.profile.start.clone();
            let a = node_combo(ui, "swmm-profile-start", &mut start, &names);
            ui.label("to");
            let mut end = state.swmm.profile.end.clone();
            let b = node_combo(ui, "swmm-profile-end", &mut end, &names);
            if a || b {
                if let (Some(s), Some(e)) = (start, end) {
                    state.swmm.profile.set_path(s, e);
                }
            }
            if ui.small_button("Swap").clicked() {
                let p = &mut state.swmm.profile;
                if let (Some(s), Some(e)) = (p.start.clone(), p.end.clone()) {
                    p.set_path(e, s);
                }
            }
            ui.separator();
            ui.label("V. exag.");
            ui.add(
                egui::Slider::new(&mut state.swmm.profile.v_exag, 1.0..=50.0)
                    .logarithmic(true)
                    .fixed_decimals(0),
            );
            ui.checkbox(&mut state.swmm.profile.show_max, "Max HGL");
            ui.separator();
            if ui
                .add_enabled(
                    !state.swmm.profile.export.is_pending(),
                    egui::Button::new("Export PNG").small(),
                )
                .clicked()
            {
                let name = format!(
                    "profile-{}-{}.png",
                    state.swmm.profile.start.clone().unwrap_or_default(),
                    state.swmm.profile.end.clone().unwrap_or_default()
                );
                state
                    .swmm
                    .profile
                    .export
                    .request_png(ui.ctx(), plot_rect, &name);
            }
            if ui.small_button("Export CSV").clicked() {
                if let Some(csv) = profile_csv(state) {
                    if let Some(msg) = save_csv("profile-stations.csv", &csv) {
                        state.swmm.profile.export.message = msg;
                    }
                } else {
                    state.swmm.profile.export.message = "No profile to export".to_string();
                }
            }
            if !state.swmm.profile.export.message.is_empty() {
                ui.label(state.swmm.profile.export.message.clone());
            }
        });
        used = ui.min_rect().height();
    });
    used.max(STRIP_H)
}

/// Dashed polyline, for the envelope.
fn dashed(painter: &egui::Painter, pts: Vec<Pos2>, stroke: Stroke) {
    if pts.len() >= 2 {
        painter.add(egui::Shape::dashed_line(&pts, stroke, 6.0, 4.0));
    }
}

/// Draw the SWMM profile view into `rect`.
pub fn draw_swmm_profile(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let model = state.swmm.model.clone();
    state.swmm.profile.ensure_network(model.as_deref());
    state.swmm.profile.ensure_profile();
    state.swmm.profile.export.poll(ui.ctx());

    // Last frame's measured height: the strip wraps with the panel width, and
    // laying the plot out under a one-row assumption is what put the wrapped
    // row on top of the title.
    let strip_h = state.swmm.profile.strip_h.max(STRIP_H);
    let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), strip_h));
    let plot_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + strip_h), rect.max);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette::canvas::bg(dark));

    let used = draw_strip(ui, strip, plot_rect, state);
    state.swmm.profile.strip_h = used;

    let empty_state = |line: &str| {
        painter.text(
            plot_rect.center(),
            egui::Align2::CENTER_CENTER,
            line,
            egui::FontId::proportional(15.0),
            palette::canvas::muted(dark),
        );
    };

    if state.swmm.model.is_none() {
        empty_state("Choose a model (.inp) in the SWMM tab to draw a profile");
        return;
    }
    if let Some(err) = state.swmm.profile.error.clone() {
        empty_state(&err);
        return;
    }
    let Some(profile) = state.swmm.profile.profile().cloned() else {
        empty_state("Pick a start and an end node");
        return;
    };
    if profile.links.is_empty() {
        empty_state("The path has no links");
        return;
    }

    let (hgl_now, hgl_max) = hgl_columns(state, &profile);
    let metric = profile.metric;
    let unit = if metric { "m" } else { "ft" };
    let v_exag = state.swmm.profile.v_exag.max(0.1) as f64;

    // Frame: the station span and the elevation span, HGLs included.
    let mut extra: Vec<f64> = hgl_now.iter().flatten().copied().collect();
    if state.swmm.profile.show_max {
        extra.extend(hgl_max.iter().flatten().copied());
    }
    let Some((e_lo, e_hi)) = profile.elevation_range(&extra) else {
        empty_state("The profile has no elevations");
        return;
    };
    let e_pad = ((e_hi - e_lo) * 0.08).max(0.5);
    let (e_lo, e_hi) = (e_lo - e_pad, e_hi + e_pad);
    let length = profile.length().max(1e-6);

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
    // One scale fits the station axis; the elevation axis is that scale times
    // the exaggeration, unless the result would not fit, in which case both
    // shrink together so the exaggeration stays honest.
    let scale =
        (inner.width() as f64 / length).min(inner.height() as f64 / ((e_hi - e_lo) * v_exag));
    let x_at = |st: f64| inner.left() + (st * scale) as f32;
    let y_at = |el: f64| inner.bottom() - ((el - e_lo) * scale * v_exag) as f32;

    // Elevation axis.
    // The step has to clear on screen, not merely in elevation units: the
    // single scale above can leave this axis a couple of dozen pixels.
    let e_step = legible_tick_step((e_hi - e_lo).max(1e-6), scale * v_exag);
    let mut e = (e_lo / e_step).ceil() * e_step;
    while e <= e_hi + e_step * 0.01 {
        let y = y_at(e);
        painter.line_segment(
            [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
            Stroke::new(1.0_f32, palette::canvas::grid(dark)),
        );
        painter.text(
            Pos2::new(inner.left() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{e:.1}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        e += e_step;
    }
    painter.text(
        Pos2::new(plot_rect.left() + 12.0, plot_rect.top() + 22.0),
        egui::Align2::LEFT_TOP,
        format!("Elev ({unit})"),
        egui::FontId::proportional(11.0),
        palette::canvas::muted(dark),
    );

    // Station axis.
    let s_step = station_tick_step(length);
    let axis_y = inner.bottom();
    painter.line_segment(
        [
            Pos2::new(inner.left(), axis_y),
            Pos2::new(x_at(length), axis_y),
        ],
        Stroke::new(1.0_f32, palette::canvas::line(dark)),
    );
    let mut st = 0.0;
    while st <= length + s_step * 0.01 {
        let x = x_at(st);
        painter.line_segment(
            [Pos2::new(x, axis_y), Pos2::new(x, axis_y + 5.0)],
            Stroke::new(1.0_f32, palette::canvas::line(dark)),
        );
        painter.text(
            Pos2::new(x, axis_y + 8.0),
            egui::Align2::CENTER_TOP,
            format!("{st:.0}"),
            egui::FontId::monospace(10.0),
            palette::canvas::muted(dark),
        );
        st += s_step;
    }
    painter.text(
        Pos2::new(inner.center().x, plot_rect.bottom() - 4.0),
        egui::Align2::CENTER_BOTTOM,
        format!("Station ({unit})  ·  vertical exaggeration {v_exag:.0}×"),
        egui::FontId::proportional(11.0),
        palette::canvas::muted(dark),
    );

    // Conduit barrels: filled between invert and crown, then outlined.
    for seg in &profile.links {
        let a = Pos2::new(x_at(seg.from_station), y_at(seg.up_invert));
        let b = Pos2::new(x_at(seg.to_station), y_at(seg.dn_invert));
        let c = Pos2::new(x_at(seg.to_station), y_at(seg.dn_crown));
        let d = Pos2::new(x_at(seg.from_station), y_at(seg.up_crown));
        painter.add(egui::Shape::convex_polygon(
            vec![a, b, c, d],
            palette::canvas::faint_fill(dark),
            Stroke::NONE,
        ));
        let invert_stroke = Stroke::new(2.0_f32, palette::canvas::invert_line(dark));
        painter.line_segment([a, b], invert_stroke);
        painter.line_segment(
            [d, c],
            Stroke::new(1.5_f32, palette::canvas::invert_line(dark)),
        );
    }

    // Surcharge: HGL above the crown along a segment. The HGL runs straight
    // between nodes, so sample the segment and shade the slices above.
    let hgl_at_nodes: Vec<Option<f64>> = hgl_now.clone();
    let surcharge_fill = Color32::from_rgba_unmultiplied(226, 162, 60, 110);
    let flood_fill = Color32::from_rgba_unmultiplied(224, 64, 64, 130);
    for (i, seg) in profile.links.iter().enumerate() {
        let (Some(h0), Some(h1)) = (hgl_at_nodes[i], hgl_at_nodes[i + 1]) else {
            continue;
        };
        const N: usize = 12;
        for k in 0..N {
            let t0 = k as f64 / N as f64;
            let t1 = (k + 1) as f64 / N as f64;
            let st0 = seg.from_station + (seg.to_station - seg.from_station) * t0;
            let st1 = seg.from_station + (seg.to_station - seg.from_station) * t1;
            let hg0 = h0 + (h1 - h0) * t0;
            let hg1 = h0 + (h1 - h0) * t1;
            let cr0 = seg.up_crown + (seg.dn_crown - seg.up_crown) * t0;
            let cr1 = seg.up_crown + (seg.dn_crown - seg.up_crown) * t1;
            if hg0 > cr0 || hg1 > cr1 {
                let top0 = hg0.max(cr0);
                let top1 = hg1.max(cr1);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(x_at(st0), y_at(cr0)),
                        Pos2::new(x_at(st1), y_at(cr1)),
                        Pos2::new(x_at(st1), y_at(top1)),
                        Pos2::new(x_at(st0), y_at(top0)),
                    ],
                    surcharge_fill,
                    Stroke::NONE,
                ));
            }
        }
    }

    // Node shafts from invert to rim, with flooding shaded above the rim.
    for (i, n) in profile.nodes.iter().enumerate() {
        let x = x_at(n.station);
        let shaft = Rect::from_min_max(
            Pos2::new(x - SHAFT_W / 2.0, y_at(n.rim)),
            Pos2::new(x + SHAFT_W / 2.0, y_at(n.invert)),
        );
        painter.rect_filled(shaft, 1.0, palette::canvas::faint_fill(dark));
        painter.rect_stroke(
            shaft,
            1.0,
            Stroke::new(1.0_f32, palette::canvas::line(dark)),
        );
        if let Some(h) = hgl_at_nodes[i] {
            if h > n.rim {
                let flood = Rect::from_min_max(
                    Pos2::new(x - SHAFT_W, y_at(h)),
                    Pos2::new(x + SHAFT_W, y_at(n.rim)),
                );
                painter.rect_filled(flood, 0.0, flood_fill);
            }
        }
        let label_y = (y_at(n.rim) - 4.0).max(plot_rect.top() + PAD_TOP + 12.0);
        painter.text(
            Pos2::new(x, label_y),
            egui::Align2::CENTER_BOTTOM,
            &n.node,
            egui::FontId::monospace(11.0),
            palette::canvas::ink(dark),
        );
    }

    // Ground / rim line across the nodes.
    let ground: Vec<Pos2> = profile
        .nodes
        .iter()
        .map(|n| Pos2::new(x_at(n.station), y_at(n.rim)))
        .collect();
    painter.add(egui::Shape::line(
        ground,
        Stroke::new(2.5_f32, palette::PROFILE_GROUND),
    ));

    // Maximum HGL envelope, dashed.
    if state.swmm.profile.show_max {
        let pts: Vec<Pos2> = profile
            .nodes
            .iter()
            .zip(&hgl_max)
            .filter_map(|(n, h)| h.map(|h| Pos2::new(x_at(n.station), y_at(h))))
            .collect();
        dashed(
            &painter,
            pts,
            Stroke::new(1.5_f32, palette::canvas::egl(dark)),
        );
    }

    // HGL now.
    let hgl_pts: Vec<Pos2> = profile
        .nodes
        .iter()
        .zip(&hgl_now)
        .filter_map(|(n, h)| h.map(|h| Pos2::new(x_at(n.station), y_at(h))))
        .collect();
    if hgl_pts.len() >= 2 {
        painter.add(egui::Shape::line(
            hgl_pts,
            Stroke::new(2.5_f32, palette::canvas::hgl(dark)),
        ));
    }

    // Header and legend.
    let when = match state.swmm.frame() {
        Some(fr) => format!(
            "period {}/{} · {} ({:.2} h)",
            fr.period + 1,
            state.swmm.n_periods(),
            stormsewer_swmm::out::format_datetime(fr.date_days),
            fr.time_s / 3600.0
        ),
        None if state.swmm.results.is_some() => {
            "run peaks (press Play or move the slider for an instant)".to_string()
        }
        None => "not run yet".to_string(),
    };
    painter.text(
        plot_rect.left_top() + Vec2::new(12.0, 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "Profile · {} → {} · {} links · {}",
            profile.nodes.first().map(|n| n.node.as_str()).unwrap_or(""),
            profile.nodes.last().map(|n| n.node.as_str()).unwrap_or(""),
            profile.links.len(),
            when
        ),
        egui::FontId::proportional(13.0),
        palette::canvas::muted(dark),
    );
    let legend = [
        (palette::PROFILE_GROUND, "Ground / rim", false),
        (palette::canvas::invert_line(dark), "Invert / crown", false),
        (palette::canvas::hgl(dark), "HGL now", false),
        (palette::canvas::egl(dark), "Max HGL", true),
        (palette::WARNING, "Surcharge", false),
        (palette::ERROR, "Flooding", false),
    ];
    // Below the title line, not beside it: the title carries the path, the
    // link count and the timestamp, which is easily wide enough to reach the
    // legend column and print straight through it.
    let mut lp = Pos2::new(plot_rect.right() - PAD_RIGHT - 120.0, plot_rect.top() + 30.0);
    for (color, label, is_dashed) in legend {
        let y = lp.y + 6.0;
        if is_dashed {
            dashed(
                &painter,
                vec![Pos2::new(lp.x, y), Pos2::new(lp.x + 24.0, y)],
                Stroke::new(2.0_f32, color),
            );
        } else {
            painter.line_segment(
                [Pos2::new(lp.x, y), Pos2::new(lp.x + 24.0, y)],
                Stroke::new(2.5_f32, color),
            );
        }
        painter.text(
            Pos2::new(lp.x + 30.0, y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            palette::canvas::muted(dark),
        );
        lp.y += 15.0;
    }

    // Hover readout.
    let hover = ui.input(|i| i.pointer.hover_pos());
    if let Some(p) = hover.filter(|p| inner.contains(*p)) {
        let station = ((p.x - inner.left()) as f64 / scale).clamp(0.0, length);
        painter.line_segment(
            [Pos2::new(p.x, inner.top()), Pos2::new(p.x, inner.bottom())],
            Stroke::new(1.0_f32, palette::canvas::selection(dark)),
        );
        let readout = hover_readout(&profile, &hgl_now, station, unit);
        let box_w = 190.0;
        let x = if p.x + 12.0 + box_w > inner.right() {
            p.x - 12.0 - box_w
        } else {
            p.x + 12.0
        };
        let origin = Pos2::new(x, (p.y - 10.0).max(inner.top()));
        let bg = Rect::from_min_size(origin, Vec2::new(box_w, 14.0 * readout.len() as f32 + 10.0));
        painter.rect_filled(bg, 4.0, palette::canvas::panel_fill(dark));
        painter.rect_stroke(bg, 4.0, Stroke::new(1.0_f32, palette::canvas::line(dark)));
        for (i, line) in readout.iter().enumerate() {
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

/// Text lines for the hover box at `station`.
pub fn hover_readout(
    profile: &Profile,
    hgl: &[Option<f64>],
    station: f64,
    unit: &str,
) -> Vec<String> {
    let mut lines = vec![format!("station {station:.1} {unit}")];
    // Segment index covering the station, for the HGL and rim interpolation.
    let seg_i = profile
        .links
        .iter()
        .position(|l| station >= l.from_station && station <= l.to_station);
    if let Some((invert, crown, seg)) = profile.pipe_at(station) {
        lines.push(format!("{} ({})", seg.link, seg.kind.label()));
        lines.push(format!("invert {invert:.2} {unit}"));
        lines.push(format!("crown  {crown:.2} {unit}"));
        if let Some(i) = seg_i {
            let (a, b) = (&profile.nodes[i], &profile.nodes[i + 1]);
            let span = (b.station - a.station).max(1e-9);
            let t = ((station - a.station) / span).clamp(0.0, 1.0);
            let rim = a.rim + (b.rim - a.rim) * t;
            lines.push(format!("rim    {rim:.2} {unit}"));
            if let (Some(ha), Some(hb)) = (
                hgl.get(i).copied().flatten(),
                hgl.get(i + 1).copied().flatten(),
            ) {
                let h = ha + (hb - ha) * t;
                lines.push(format!("HGL    {h:.2} {unit}"));
                lines.push(format!("depth  {:.2} {unit}", h - invert));
                if h > rim {
                    lines.push("flooding".to_string());
                } else if h > crown {
                    lines.push("surcharged".to_string());
                }
            } else {
                lines.push("HGL    —".to_string());
            }
        }
    }
    // Nearest node, for the reviewer who wants the structure's numbers.
    if let Some((i, n)) = profile.nodes.iter().enumerate().min_by(|a, b| {
        (a.1.station - station)
            .abs()
            .total_cmp(&(b.1.station - station).abs())
    }) {
        lines.push(format!(
            "near {} inv {:.2} rim {:.2}",
            n.node, n.invert, n.rim
        ));
        if let Some(h) = hgl.get(i).copied().flatten() {
            lines.push(format!("     HGL {h:.2} depth {:.2}", h - n.invert));
        }
    }
    lines
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::ViewTab;
    use crate::swmm_panel::SwmmSubView;
    use crate::StormSewerApp;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../swmm/tests/fixtures")
            .join(rel)
    }

    /// A state with the Detention Pond model chosen and its committed run
    /// loaded, as if the engine had just finished.
    pub(crate) fn pond_state() -> AppState {
        let mut s = AppState::new_empty();
        s.swmm.model = Some(fixture("epa-samples/Detention_Pond_Model.inp"));
        s.swmm.load_model_geometry();
        let out =
            stormsewer_swmm::out::OutputFile::open(fixture("results/Detention_Pond_Model.out"))
                .unwrap();
        s.swmm.node_peaks = stormsewer_swmm::out::node_peaks(&out.path, &out.meta).unwrap();
        s.swmm.link_peaks = stormsewer_swmm::out::link_peaks(&out.path, &out.meta).unwrap();
        s.swmm.results = Some(out);
        let rpt_path = fixture("results/Detention_Pond_Model.rpt");
        s.swmm.last_run = Some(stormsewer_swmm::engine::Run {
            engine_id: "fixture".into(),
            engine_version: "5.2.4".into(),
            engine_sha256: String::new(),
            inp: fixture("epa-samples/Detention_Pond_Model.inp"),
            rpt: rpt_path.clone(),
            out: fixture("results/Detention_Pond_Model.out"),
            exit_code: Some(0),
            elapsed: std::time::Duration::from_secs(1),
            stdout: String::new(),
            stderr: String::new(),
            report: stormsewer_swmm::rpt::read(&rpt_path).unwrap(),
        });
        s.view_tab = ViewTab::Swmm;
        s
    }

    pub(crate) fn run_frame(app: &mut StormSewerApp) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1400.0, 900.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }

    #[test]
    fn profile_view_renders_before_and_after_a_run() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        app.state.view_tab = ViewTab::Swmm;
        app.state.swmm.sub_view = SwmmSubView::Profile;
        run_frame(&mut app);
        assert!(
            app.state.swmm.profile.profile().is_none(),
            "no model chosen"
        );

        let mut app = StormSewerApp::new_for_test(pond_state());
        app.state.swmm.sub_view = SwmmSubView::Profile;
        run_frame(&mut app);
        // A default path was offered: first node to the outfall.
        let p = app.state.swmm.profile.profile().expect("default profile");
        assert_eq!(p.nodes.first().unwrap().node, "J1");
        assert_eq!(p.nodes.last().unwrap().node, "O2");

        // The slider's instant drives the HGL: after setting a period, the
        // current HGL column is filled from that frame.
        app.state.swmm.set_period(7);
        run_frame(&mut app);
        let p = app.state.swmm.profile.profile().unwrap().clone();
        let (now, max) = hgl_columns(&app.state, &p);
        assert!(now.iter().all(|h| h.is_some()));
        assert!(max.iter().all(|h| h.is_some()));
        for (n, (a, b)) in p.nodes.iter().zip(now.iter().zip(&max)) {
            assert!(a.unwrap() >= n.invert - 1e-9);
            assert!(b.unwrap() >= a.unwrap() - 1e-6, "{}: max below now", n.node);
        }
        let csv = profile_csv(&app.state).unwrap();
        assert_eq!(csv.lines().count(), 1 + p.nodes.len());
        assert!(csv.contains("J11,junction,"));
        run_frame(&mut app);
    }

    #[test]
    fn set_path_rebuilds_and_reports_a_missing_path() {
        let mut app = StormSewerApp::new_for_test(pond_state());
        app.state.swmm.sub_view = SwmmSubView::Profile;
        app.state.swmm.profile.set_path("J2", "J11");
        run_frame(&mut app);
        let p = app.state.swmm.profile.profile().unwrap();
        assert_eq!(p.links.len(), 1);
        assert_eq!(p.links[0].link, "C2");

        app.state.swmm.profile.set_path("J1", "nowhere");
        run_frame(&mut app);
        assert!(app.state.swmm.profile.profile().is_none());
        assert!(app
            .state
            .swmm
            .profile
            .error
            .as_deref()
            .is_some_and(|e| e.contains("nowhere")));
        run_frame(&mut app);
    }

    #[test]
    fn hover_readout_interpolates_along_a_segment() {
        let s = pond_state();
        let doc = InpDoc::read(s.swmm.model.as_ref().unwrap()).unwrap();
        let net = ProfileNetwork::from_doc(&doc);
        let p = net.build("J11", "SU1").unwrap();
        // HGL 4968 at J11 (above C11's 4967.75 crown) and 4958 at SU1.
        let lines = hover_readout(&p, &[Some(4968.0), Some(4958.0)], 0.0, "ft");
        assert!(lines.iter().any(|l| l.starts_with("station 0.0 ft")));
        assert!(lines.iter().any(|l| l == "surcharged"), "{lines:?}");
        let lines = hover_readout(&p, &[Some(4968.0), Some(4958.0)], 150.0, "ft");
        assert!(
            lines.iter().any(|l| l.contains("HGL    4958.00")),
            "{lines:?}"
        );
    }
}
