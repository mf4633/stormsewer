// SPDX-License-Identifier: GPL-3.0-or-later

//! Recently opened project files (persisted under %APPDATA%/StormSewer).

use std::fs;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 8;

/// Recently opened paths, most recent first. The default list is the
/// `.ssproj` projects; a named list (`load_named`) keeps another file type's
/// recents apart under its own file.
#[derive(Clone, Debug, Default)]
pub struct RecentFiles {
    pub paths: Vec<PathBuf>,
    /// File name under the config directory; empty means `recent.json`.
    file: String,
}

impl RecentFiles {
    pub fn load() -> Self {
        Self::load_named("")
    }

    /// Load the list kept in `file` (e.g. `recent-inp.json`).
    pub fn load_named(file: &str) -> Self {
        let path = config_path(file);
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(paths) = serde_json::from_str::<Vec<PathBuf>>(&data) {
                let paths: Vec<PathBuf> = paths
                    .into_iter()
                    .filter(|p| p.exists())
                    .take(MAX_RECENT)
                    .collect();
                return Self {
                    paths,
                    file: file.to_string(),
                };
            }
        }
        Self {
            paths: Vec::new(),
            file: file.to_string(),
        }
    }

    pub fn save(&self) {
        let path = config_path(&self.file);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.paths) {
            let _ = fs::write(path, json);
        }
    }

    pub fn push(&mut self, path: PathBuf) {
        self.paths.retain(|p| p != &path);
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT);
        self.save();
    }

    pub fn label(&self, path: &Path) -> String {
        path.file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.display().to_string())
    }
}

fn config_path(file: &str) -> PathBuf {
    let file = if file.is_empty() { "recent.json" } else { file };
    std::env::var_os("APPDATA")
        .map(|appdata| PathBuf::from(appdata).join("StormSewer").join(file))
        .unwrap_or_else(|| PathBuf::from(file))
}
