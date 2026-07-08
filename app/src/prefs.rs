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

fn config_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|appdata| PathBuf::from(appdata).join("StormSewer").join("app_prefs.json"))
        .unwrap_or_else(|| PathBuf::from("app_prefs.json"))
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
}