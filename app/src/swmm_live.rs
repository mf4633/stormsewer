// SPDX-License-Identifier: GPL-3.0-or-later

//! Live results while the engine runs, and a Stop that works.
//!
//! While `runswmm` is busy the `.out` grows by one record per reporting
//! period. `stormsewer_swmm::live::LivePoller` reads it every half second
//! without the closing block, and this window shows where the run is:
//! simulation time against the model's duration, periods written, how many
//! nodes are flooding right now and the deepest node in the latest frame.
//! With **Follow** on, the map's period is moved to the latest one as it
//! arrives, so the network animates as the engine works. The window opens
//! by itself when a run starts (Run → **Live Results** toggles that) and
//! closes when the run ends; the SWMM panel then shows the finished results
//! as usual.

use std::path::PathBuf;

use eframe::egui::{self, RichText, Ui};
use stormsewer_swmm::live::{duration_from_inp, format_hms, LivePoller, LiveSnapshot};
use stormsewer_swmm::out::format_datetime;

use crate::state::AppState;
use crate::theme::palette;

/// Per-editor live-run state.
pub struct LiveState {
    /// Run → Live Results: open the window and follow the file during runs.
    pub enabled: bool,
    /// Move the map to the newest period as it arrives.
    pub follow: bool,
    /// The window is showing.
    pub open: bool,
    /// The run being watched, while one is.
    poller: Option<LivePoller>,
    /// Simulation length of the model being run, from its `[OPTIONS]`.
    duration_s: Option<f64>,
    /// The latest look at the file, kept after the run ends for the tests
    /// and for a last glance.
    pub snapshot: Option<LiveSnapshot>,
    /// Periods the map was last told about.
    shown_periods: usize,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            enabled: true,
            follow: true,
            open: false,
            poller: None,
            duration_s: None,
            snapshot: None,
            shown_periods: 0,
        }
    }
}

impl LiveState {
    /// The `.out` being watched.
    pub fn watching(&self) -> Option<PathBuf> {
        self.poller.as_ref().map(|p| p.path().to_path_buf())
    }
}

/// `Run` menu entries (live results toggle and the like).
pub fn run_menu_items(ui: &mut Ui, state: &mut AppState) {
    ui.checkbox(&mut state.swmm_doc.live.enabled, "Live Results")
        .on_hover_text("Show the run's progress and the map's newest period while the engine works");
}

/// Windows and dialogs.
pub fn draw_dialogs(ctx: &egui::Context, state: &mut AppState) {
    follow_run(state);
    if state.swmm_doc.live.open {
        draw_window(ctx, state);
    }
}

/// Start watching when a run starts, read the file while it runs, and let
/// go when it ends.
fn follow_run(state: &mut AppState) {
    let running_out = state.swmm.is_running().then(|| state.swmm.run_out().map(|p| p.to_path_buf())).flatten();
    let Some(out) = running_out else {
        // Run over (or none): the panel's own poll shows the final results.
        if state.swmm_doc.live.poller.take().is_some() {
            state.swmm_doc.live.open = false;
        }
        return;
    };
    let live = &mut state.swmm_doc.live;
    if live.poller.as_ref().is_none_or(|p| p.path() != out) {
        live.poller = Some(LivePoller::new(&out));
        live.duration_s = duration_from_inp(&state.swmm_doc.doc.to_string());
        live.snapshot = None;
        live.shown_periods = 0;
        live.open = live.enabled;
    }
    if !live.enabled {
        return;
    }
    let Some(poller) = live.poller.as_mut() else { return };
    if !poller.poll() {
        return;
    }
    let Some(snapshot) = poller.snapshot().cloned() else { return };
    let periods = snapshot.periods;
    live.snapshot = Some(snapshot);
    if periods > live.shown_periods {
        live.shown_periods = periods;
        let follow = live.follow;
        if let Some(n) = state.swmm.refresh_partial_results(&out) {
            if follow && n > 0 {
                state.swmm.playing = false;
                state.swmm.set_period(n - 1);
            }
        }
    }
}

fn draw_window(ctx: &egui::Context, state: &mut AppState) {
    let mut open = true;
    let mut stop = false;
    let dark = ctx.style().visuals.dark_mode;
    let running = state.swmm.is_running();
    let stopping = state.swmm.is_stopping();
    let wall = state.swmm.running_for().unwrap_or_default();
    let live = &mut state.swmm_doc.live;
    egui::Window::new("Live Results")
        .id(egui::Id::new("swmm-live-results"))
        .open(&mut open)
        .default_size(egui::vec2(360.0, 220.0))
        .resizable(true)
        .show(ctx, |ui| {
            let duration = live.duration_s.unwrap_or(0.0);
            match &live.snapshot {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Waiting for the engine to write its first reporting period…");
                    });
                    if duration > 0.0 {
                        ui.add(egui::ProgressBar::new(0.0).text(format!("0 of {}", format_hms(duration))));
                    }
                }
                Some(s) => {
                    let fraction = if duration > 0.0 { (s.sim_time_s / duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
                    let text = if duration > 0.0 {
                        format!("{} of {}", format_hms(s.sim_time_s), format_hms(duration))
                    } else {
                        format_hms(s.sim_time_s)
                    };
                    ui.add(egui::ProgressBar::new(fraction).text(text).animate(running));
                    egui::Grid::new("swmm-live-grid").num_columns(2).show(ui, |ui| {
                        ui.label("Simulation time");
                        ui.label(if s.periods > 0 { format_datetime(s.date_days) } else { "—".to_string() });
                        ui.end_row();
                        ui.label("Periods written");
                        ui.label(format!("{} (every {} s)", s.periods, s.report_step_s));
                        ui.end_row();
                        ui.label("Flooding now");
                        let flooding = format!("{} node(s)", s.flooding_nodes);
                        if s.flooding_nodes > 0 {
                            ui.label(RichText::new(flooding).color(palette::error_text(dark)));
                        } else {
                            ui.label(flooding);
                        }
                        ui.end_row();
                        ui.label("Deepest node");
                        match &s.worst_node {
                            Some((id, depth)) => ui.label(format!("{id}  {depth:.3}")),
                            None => ui.label("—"),
                        };
                        ui.end_row();
                        ui.label("Fullest link");
                        match &s.fullest_link {
                            Some((id, cap)) => ui.label(format!("{id}  {:.0}% of capacity", cap * 100.0)),
                            None => ui.label("—"),
                        };
                        ui.end_row();
                        ui.label("Wall clock");
                        ui.label(format!("{:.1} s", wall.as_secs_f64()));
                        ui.end_row();
                    });
                }
            }
            if let Some(e) = live.poller.as_ref().and_then(|p| p.error()) {
                ui.label(RichText::new(e).small().color(palette::warning_text(dark)));
            }
            if let Some(path) = live.watching() {
                ui.label(RichText::new(path.display().to_string()).small().weak());
            }
            ui.label(
                RichText::new("Records reach the file in bursts as the engine flushes; the count can lag a few periods.")
                    .small()
                    .weak(),
            );
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut live.follow, "Follow")
                    .on_hover_text("Move the map to the newest reporting period as it arrives");
                if stopping {
                    ui.spinner();
                    ui.label("stopping…");
                } else if ui
                    .add_enabled(running, egui::Button::new("Stop"))
                    .on_hover_text("Kill the engine; keep what it wrote so far")
                    .clicked()
                {
                    stop = true;
                }
            });
        });
    live.open = open;
    if stop {
        state.status = state.swmm.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;

    /// A run in flight writing the pond model's `.out` record by record:
    /// the window opens by itself, the snapshot follows the file, Follow
    /// moves the map to the newest period, and the window closes when the
    /// run ends.
    #[test]
    fn live_window_follows_a_run_and_moves_the_map() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../swmm/tests/fixtures/results/Detention_Pond_Model.out");
        let bytes = std::fs::read(&fixture).unwrap();
        let meta = stormsewer_swmm::out::read_metadata(&fixture).unwrap();
        let start = meta.output_offset as usize;
        let stride = meta.bytes_per_period() as usize;
        let dir = std::env::temp_dir().join("stormsewer-swmm-tests").join("live-window");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("pond.out");
        let _ = std::fs::remove_file(&out);

        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), None);
        assert!(app.state.swmm_doc.live.enabled, "on by default");
        assert!(!app.state.swmm_doc.live.open);

        let tx = app.state.swmm.fake_run_for_test(out.clone());
        run_frame(&mut app);
        let live = &app.state.swmm_doc.live;
        assert!(live.open, "opens when a run starts");
        assert_eq!(live.watching(), Some(out.clone()));
        assert_eq!(live.duration_s, Some(43_200.0), "from the model's [OPTIONS]");
        assert!(live.snapshot.is_none(), "no file yet");

        // The engine writes twelve periods. The poller looks at most every
        // half second, so wait it out before the next frame.
        std::fs::write(&out, &bytes[..start + 12 * stride]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(600));
        run_frame(&mut app);
        let s = app.state.swmm_doc.live.snapshot.clone().expect("snapshot");
        assert_eq!(s.periods, 12);
        assert_eq!(s.sim_time_s, 12.0 * 300.0);
        assert_eq!(app.state.swmm.n_periods(), 12);
        assert!(app.state.swmm.results.as_ref().unwrap().partial);
        assert_eq!(app.state.swmm.period, 11, "Follow moved the map to the newest period");
        assert_eq!(app.state.swmm.frame().unwrap().period, 11);

        // Follow off: the map stays put while the file grows.
        app.state.swmm_doc.live.follow = false;
        app.state.swmm.set_period(3);
        std::fs::write(&out, &bytes[..start + 30 * stride]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(600));
        run_frame(&mut app);
        assert_eq!(app.state.swmm_doc.live.snapshot.as_ref().unwrap().periods, 30);
        assert_eq!(app.state.swmm.n_periods(), 30);
        assert_eq!(app.state.swmm.period, 3);

        // The window can be closed and the run keeps being followed.
        app.state.swmm_doc.live.open = false;
        run_frame(&mut app);
        assert!(app.state.swmm_doc.live.watching().is_some());

        // The run ends: the poller lets go.
        drop(tx);
        run_frame(&mut app);
        assert!(!app.state.swmm.is_running());
        run_frame(&mut app);
        assert!(app.state.swmm_doc.live.watching().is_none());
        assert!(!app.state.swmm_doc.live.open);
        assert_eq!(app.state.swmm_doc.live.snapshot.as_ref().unwrap().periods, 30, "the last look is kept");
    }

    /// With the toggle off nothing is read and no window opens.
    #[test]
    fn live_results_can_be_switched_off() {
        let dir = std::env::temp_dir().join("stormsewer-swmm-tests").join("live-off");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("pond.out");
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../swmm/tests/fixtures/results/Detention_Pond_Model.out"),
            &out,
        )
        .unwrap();
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state.swmm_doc.live.enabled = false;
        let _tx = app.state.swmm.fake_run_for_test(out.clone());
        run_frame(&mut app);
        assert!(!app.state.swmm_doc.live.open);
        assert!(app.state.swmm_doc.live.snapshot.is_none());
        assert!(app.state.swmm.results.is_none());
        // The menu item draws.
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| run_menu_items(ui, &mut state));
        });
    }
}
