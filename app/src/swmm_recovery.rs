// SPDX-License-Identifier: GPL-3.0-or-later

//! Autosave and crash recovery for the SWMM model editor.
//!
//! While the model is dirty, its text is written every few minutes (the
//! interval is a preference, default 2) to `<model>.inp.autosave` beside
//! the file, or under `%APPDATA%\StormSewer\recovery` for a model that has
//! no file yet. Saving or closing the model removes the snapshot. Opening a
//! file that has a newer snapshot beside it offers Restore / Discard with
//! both timestamps.
//!
//! The clock is egui's frame time (`ctx.input(|i| i.time)`), not the wall
//! clock, so the headless tests drive it by advancing frames.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use stormsewer_swmm::doc::{Command, InpDoc};

use crate::state::AppState;
use crate::swmm_doc::SwmmEditor;

/// The snapshot's suffix: `model.inp` → `model.inp.autosave`.
pub const SUFFIX: &str = ".autosave";

/// Default interval, in minutes.
pub const DEFAULT_MINUTES: u32 = 2;

/// A snapshot found beside a file being opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryOffer {
    pub autosave: PathBuf,
    pub model: PathBuf,
    pub autosave_time: SystemTime,
    pub file_time: SystemTime,
}

#[derive(Clone, Debug, Default)]
pub struct RecoveryState {
    /// Frame time of the last write (or of the first frame seen).
    last_tick: Option<f64>,
    /// Where the last snapshot went, for the status line and tests.
    pub last_write: Option<PathBuf>,
    /// How many snapshots this session wrote.
    pub written: u32,
    pub offer: Option<RecoveryOffer>,
}

/// The folder for snapshots of models with no file: the project autosave
/// override when set (tests), else the per-user data folder.
pub fn recovery_dir() -> PathBuf {
    std::env::var_os("STORMSEWER_AUTOSAVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(crate::prefs::storage_dir)
        .join("recovery")
}

/// Where a model's snapshot lives.
pub fn autosave_path_for(model: Option<&Path>) -> PathBuf {
    match model {
        Some(p) => {
            let mut s = p.as_os_str().to_os_string();
            s.push(SUFFIX);
            PathBuf::from(s)
        }
        None => recovery_dir().join(format!("untitled.inp{SUFFIX}")),
    }
}

/// Remove a model's snapshot, if any.
pub fn remove_for(model: Option<&Path>) {
    let _ = std::fs::remove_file(autosave_path_for(model));
}

/// Write the snapshot now. Returns where it went.
pub fn write_now(ed: &SwmmEditor) -> std::io::Result<PathBuf> {
    let path = autosave_path_for(ed.path.as_deref());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, ed.doc.to_string())?;
    Ok(path)
}

/// Advance the autosave clock to `now` (seconds). Writes a snapshot when
/// the model has been dirty for `interval` seconds since the last write
/// (or since the edit that made it dirty). Returns whether it wrote.
pub fn tick(ed: &mut SwmmEditor, now: f64, interval: f64) -> bool {
    if !ed.dirty() {
        // The clock starts at the first unsaved edit.
        ed.recovery.last_tick = Some(now);
        return false;
    }
    let Some(last) = ed.recovery.last_tick else {
        ed.recovery.last_tick = Some(now);
        return false;
    };
    if now - last < interval {
        return false;
    }
    ed.recovery.last_tick = Some(now);
    match write_now(ed) {
        Ok(p) => {
            ed.recovery.written += 1;
            ed.recovery.last_write = Some(p);
            true
        }
        Err(e) => {
            ed.last_error = Some(format!("autosave failed: {e}"));
            false
        }
    }
}

/// Once per frame from the app: tick with egui's clock and the preference.
pub fn per_frame(ctx: &egui::Context, state: &mut AppState) {
    if !state.swmm_doc.loaded {
        return;
    }
    let minutes = state.prefs.swmm_autosave_minutes;
    if minutes == 0 {
        return;
    }
    let now = ctx.input(|i| i.time);
    if tick(&mut state.swmm_doc, now, minutes as f64 * 60.0) {
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

/// Look for a snapshot newer than `path` and, if there is one, record the
/// offer for the prompt. Older snapshots are stale and are removed.
pub fn check_on_open(ed: &mut SwmmEditor, path: &Path) {
    let autosave = autosave_path_for(Some(path));
    let (Ok(a), Ok(f)) = (std::fs::metadata(&autosave), std::fs::metadata(path)) else {
        return;
    };
    let (Ok(at), Ok(ft)) = (a.modified(), f.modified()) else {
        return;
    };
    if at > ft {
        ed.recovery.offer = Some(RecoveryOffer {
            autosave,
            model: path.to_path_buf(),
            autosave_time: at,
            file_time: ft,
        });
    } else {
        let _ = std::fs::remove_file(&autosave);
    }
}

/// Take the snapshot's text as the open document. The model keeps its
/// path and is left dirty, so the next Save writes the recovered text over
/// the file; the snapshot stays until then.
pub fn restore(ed: &mut SwmmEditor) -> Result<(), String> {
    let Some(offer) = ed.recovery.offer.take() else {
        return Ok(());
    };
    let doc = InpDoc::read(&offer.autosave).map_err(|e| e.to_string())?;
    ed.replace_document(doc);
    // A recovered document differs from the file, so it must read as
    // dirty: one gesture that inserts and removes a comment line changes
    // nothing in the text but moves the document off its save point.
    if let Some(first) = ed.doc.sections().first().map(|s| s.name.clone()) {
        ed.begin_gesture("recover autosave");
        ed.apply(
            Command::InsertText {
                section: first.clone(),
                line: Some(0),
                text: "; recovered".into(),
            },
            "recover autosave",
        );
        ed.apply(
            Command::DeleteLine {
                section: first,
                line: 0,
            },
            "recover autosave",
        );
        ed.end_gesture();
    }
    Ok(())
}

/// Keep the file as it is and drop the snapshot.
pub fn discard(ed: &mut SwmmEditor) {
    if let Some(offer) = ed.recovery.offer.take() {
        let _ = std::fs::remove_file(&offer.autosave);
    }
}

/// `YYYY-MM-DD HH:MM:SS` in UTC, without a date crate.
pub fn format_time(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// The Restore / Discard prompt.
pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(offer) = state.swmm_doc.recovery.offer.clone() else {
        return;
    };
    let mut restore_now = false;
    let mut discard_now = false;
    egui::Window::new("Recover unsaved changes?")
        .id(egui::Id::new("swmm-recovery-prompt"))
        .collapsible(false)
        .resizable(false)
        .default_pos(ctx.screen_rect().center() - egui::vec2(220.0, 80.0))
        .show(ctx, |ui| {
            ui.label(format!(
                "An autosave of \"{}\" is newer than the file.",
                offer
                    .model
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            ui.add_space(4.0);
            egui::Grid::new("swmm-recovery-times").show(ui, |ui| {
                ui.label("Autosave");
                ui.monospace(format_time(offer.autosave_time));
                ui.end_row();
                ui.label("File");
                ui.monospace(format_time(offer.file_time));
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(offer.autosave.display().to_string())
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Restore autosave").clicked() {
                    restore_now = true;
                }
                if ui.button("Discard it, keep the file").clicked() {
                    discard_now = true;
                }
            });
        });
    if restore_now {
        match restore(&mut state.swmm_doc) {
            Ok(()) => state.status = "Recovered the autosave — Save to keep it".into(),
            Err(e) => state.status = format!("Recovery failed: {e}"),
        }
    } else if discard_now {
        discard(&mut state.swmm_doc);
        state.status = "Autosave discarded".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_menus;
    use crate::StormSewerApp;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-app-tests").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct Clock {
        app: StormSewerApp,
        ctx: egui::Context,
        time: f64,
    }

    impl Clock {
        fn new() -> Self {
            let mut app = StormSewerApp::new_for_test(AppState::new_empty());
            swmm_menus::enter_workspace(&mut app.state);
            Self {
                app,
                ctx: egui::Context::default(),
                time: 0.0,
            }
        }

        /// One frame `dt` seconds later.
        fn frame(&mut self, dt: f64) {
            self.time += dt;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(1400.0, 900.0),
                )),
                time: Some(self.time),
                ..Default::default()
            };
            let _ = self.ctx.run(input, |c| self.app.ui(c));
        }
    }

    #[test]
    fn autosave_writes_beside_the_model_when_dirty_and_the_clock_says_so() {
        let dir = scratch("autosave-beside");
        let model = dir.join("site.inp");
        let text = "[JUNCTIONS]\nJ1 0 0\n[COORDINATES]\nJ1 0 0\n";
        std::fs::write(&model, text).unwrap();
        let autosave = autosave_path_for(Some(&model));
        assert_eq!(autosave.file_name().unwrap(), "site.inp.autosave");

        let mut h = Clock::new();
        assert_eq!(h.app.state.prefs.swmm_autosave_minutes, DEFAULT_MINUTES);
        h.app.state.swmm_doc.open_path(&model).unwrap();
        assert!(h.app.state.swmm_doc.recovery.offer.is_none());
        h.frame(0.05);
        // Clean: three minutes pass and nothing is written.
        for _ in 0..6 {
            h.frame(30.0);
        }
        assert!(!autosave.exists());
        assert_eq!(h.app.state.swmm_doc.recovery.written, 0);

        // Dirty: the next full interval writes.
        h.app.state.swmm_doc.apply(
            Command::MoveNode {
                name: "J1".into(),
                x: 5.0,
                y: 5.0,
            },
            "move",
        );
        h.frame(60.0);
        assert!(!autosave.exists(), "only one minute of the two has passed");
        h.frame(61.0);
        assert!(autosave.exists(), "written at two minutes");
        assert_eq!(h.app.state.swmm_doc.recovery.written, 1);
        let snap = std::fs::read_to_string(&autosave).unwrap();
        assert!(snap.contains("J1               5          5") || snap.contains("J1 5 5"), "{snap}");
        assert_eq!(snap, h.app.state.swmm_doc.doc.to_string());

        // Saving removes it.
        assert_eq!(h.app.state.swmm_doc.save(), Ok(true));
        assert!(!autosave.exists());
        // Closing (a new model) removes one written since.
        h.app.state.swmm_doc.apply(
            Command::MoveNode {
                name: "J1".into(),
                x: 6.0,
                y: 6.0,
            },
            "move",
        );
        h.frame(121.0);
        assert!(autosave.exists());
        h.app.state.swmm_doc.new_model();
        assert!(!autosave.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_a_file_with_a_newer_autosave_offers_restore_or_discard() {
        let dir = scratch("autosave-offer");
        let model = dir.join("site.inp");
        let text = "[JUNCTIONS]\nJ1 0 0\n[COORDINATES]\nJ1 0 0\n";
        std::fs::write(&model, text).unwrap();
        let autosave = autosave_path_for(Some(&model));
        let recovered = "[JUNCTIONS]\nJ1 0 0\n[COORDINATES]\nJ1 9 9\n";
        std::fs::write(&autosave, recovered).unwrap();
        // The snapshot must be newer than the file.
        let f = std::fs::File::options().write(true).open(&model).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
        drop(f);

        let mut h = Clock::new();
        h.app.state.swmm_doc.open_path(&model).unwrap();
        let offer = h.app.state.swmm_doc.recovery.offer.clone().expect("an offer");
        assert_eq!(offer.autosave, autosave);
        assert!(offer.autosave_time > offer.file_time);
        assert!(format_time(offer.file_time).ends_with("UTC"));
        // The prompt draws.
        h.frame(0.05);
        assert!(h.app.state.swmm_doc.recovery.offer.is_some());

        // Discard keeps the file's text and drops the snapshot.
        discard(&mut h.app.state.swmm_doc);
        assert!(!autosave.exists());
        assert_eq!(h.app.state.swmm_doc.doc.to_string(), text);

        // Restore takes the snapshot's text, keeps the path, reads dirty.
        std::fs::write(&autosave, recovered).unwrap();
        let f = std::fs::File::options().write(true).open(&model).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
        drop(f);
        h.app.state.swmm_doc.open_path(&model).unwrap();
        assert!(h.app.state.swmm_doc.recovery.offer.is_some());
        restore(&mut h.app.state.swmm_doc).unwrap();
        assert_eq!(h.app.state.swmm_doc.doc.to_string(), recovered);
        assert_eq!(h.app.state.swmm_doc.path.as_deref(), Some(model.as_path()));
        assert!(h.app.state.swmm_doc.dirty(), "recovered text is unsaved");
        assert_eq!(h.app.state.swmm_doc.doc.coordinates("J1"), Some((9.0, 9.0)));
        assert!(autosave.exists(), "kept until the user saves");
        assert_eq!(h.app.state.swmm_doc.save(), Ok(true));
        assert_eq!(std::fs::read_to_string(&model).unwrap(), recovered);
        assert!(!autosave.exists());

        // An older snapshot is stale and is removed on open.
        std::fs::write(&autosave, "stale").unwrap();
        let f = std::fs::File::options().write(true).open(&autosave).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        drop(f);
        h.app.state.swmm_doc.open_path(&model).unwrap();
        assert!(h.app.state.swmm_doc.recovery.offer.is_none());
        assert!(!autosave.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsaved_models_snapshot_under_the_recovery_folder() {
        let p = autosave_path_for(None);
        assert!(p.ends_with("recovery/untitled.inp.autosave") || p.ends_with("recovery\\untitled.inp.autosave"), "{}", p.display());
        assert_eq!(format_time(UNIX_EPOCH + Duration::from_secs(1_700_000_000)), "2023-11-14 22:13:20 UTC");
    }
}
