// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent application preferences (separate from recent files).

use std::fs;
use std::path::PathBuf;

use crate::theme::Theme;

/// User preferences stored under `%APPDATA%/StormSewer/app_prefs.json`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AppPrefs {
    /// Show Quick Start help once on first launch.
    #[serde(default = "default_true")]
    pub show_quick_start: bool,
    /// Snap new structures to a drawing grid (ft).
    #[serde(default = "default_snap_grid")]
    pub snap_grid_ft: f64,
    /// UI color scheme.
    #[serde(default)]
    pub theme: Theme,
    /// Re-run the analysis automatically whenever an edit marks results
    /// stale (live what-if). F5 still forces a run either way.
    #[serde(default = "default_true")]
    pub auto_analyze: bool,
    /// "Don't ask again" on the support prompt.
    #[serde(default)]
    pub coffee_optout: bool,
    /// Unix time of the last support prompt (0 = never shown).
    #[serde(default)]
    pub coffee_last_epoch: u64,
    /// Set once the user ticks "Don't show on startup" in the interactive
    /// tutorial; until then the tutorial opens on every launch.
    #[serde(default)]
    pub tutorial_done: bool,
    /// When true, manholes dropped while drawing a run start with zero drainage
    /// area — sketch the layout first, assign loads later.
    #[serde(default)]
    pub draw_zero_area: bool,
}

fn default_true() -> bool {
    true
}

fn default_snap_grid() -> f64 {
    10.0
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            show_quick_start: true,
            snap_grid_ft: 10.0,
            theme: Theme::default(),
            auto_analyze: true,
            coffee_optout: false,
            coffee_last_epoch: 0,
            tutorial_done: false,
            draw_zero_area: false,
        }
    }
}

impl AppPrefs {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(prefs) = serde_json::from_str(&data) {
                return prefs;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }
}

/// Is the occasional support prompt due? Deliberately hard to trigger:
/// a real working session (50+ analyses), at least a week since the last
/// prompt, never after opt-out, and never before a first-week grace
/// period (`last_epoch` is stamped at first launch).
pub fn coffee_prompt_due(
    session_analyses: u32,
    last_epoch: u64,
    optout: bool,
    now_epoch: u64,
) -> bool {
    const WEEK: u64 = 7 * 86_400;
    !optout
        && last_epoch != 0
        && session_analyses >= 50
        && now_epoch.saturating_sub(last_epoch) >= WEEK
}

fn config_path() -> PathBuf {
    storage_dir().join("app_prefs.json")
}

/// Per-user StormSewer data directory, following each platform's own
/// convention: `%APPDATA%\StormSewer` on Windows,
/// `~/Library/Application Support/StormSewer` on macOS, and
/// `$XDG_CONFIG_HOME/stormsewer` (or `~/.config/stormsewer`) elsewhere.
///
/// The last-resort `.` only applies when the platform's home variable is
/// missing entirely; a bundled macOS app launched from Finder runs with `/`
/// as its working directory, so falling back there silently discarded
/// preferences and the unsaved-work recovery file.
pub fn storage_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("StormSewer");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/StormSewer");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(dir).join("stormsewer");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config/stormsewer");
        }
    }
    PathBuf::from(".")
}

/// Crash/close recovery file for unsaved work. `STORMSEWER_AUTOSAVE_DIR`
/// overrides the directory (used by tests to stay out of the real one).
pub fn autosave_path() -> PathBuf {
    std::env::var_os("STORMSEWER_AUTOSAVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(storage_dir)
        .join("autosave-recovery.ssproj")
}

#[cfg(test)]
mod headless_tests {
    use super::*;

    #[test]
    fn default_prefs_enable_quick_start_and_grid() {
        let prefs = AppPrefs::default();
        assert!(prefs.show_quick_start);
        assert!((prefs.snap_grid_ft - 10.0).abs() < 1e-9);
    }

    #[test]
    fn prefs_roundtrip_json() {
        let prefs = AppPrefs {
            coffee_optout: false,
            coffee_last_epoch: 0,
            auto_analyze: true,
            show_quick_start: false,
            snap_grid_ft: 25.0,
            theme: Theme::Light,
            tutorial_done: true,
            draw_zero_area: true,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let loaded: AppPrefs = serde_json::from_str(&json).unwrap();
        assert!(!loaded.show_quick_start);
        assert!((loaded.snap_grid_ft - 25.0).abs() < 1e-9);
        assert_eq!(loaded.theme, Theme::Light);
        assert!(loaded.tutorial_done);
        assert!(loaded.draw_zero_area);
    }

    /// Preferences and the recovery file must land in a real per-user
    /// directory on every platform. A relative path means the app would
    /// write next to whatever the working directory happens to be — for a
    /// Finder-launched macOS bundle that is `/`, and the writes vanish.
    #[test]
    fn storage_dir_is_a_real_per_user_location() {
        let dir = storage_dir();
        assert!(
            dir.is_absolute(),
            "storage_dir must be absolute, got {}",
            dir.display()
        );
        assert!(
            dir.to_string_lossy().to_lowercase().contains("stormsewer"),
            "storage_dir must be namespaced to the app, got {}",
            dir.display()
        );
        // The autosave file lives under it (absent the test override).
        assert!(autosave_path().is_absolute() || std::env::var_os("STORMSEWER_AUTOSAVE_DIR").is_some());
    }
}