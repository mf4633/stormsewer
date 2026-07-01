// SPDX-License-Identifier: GPL-3.0-or-later

//! Recently opened project files (persisted under %APPDATA%/StormSewer).

use std::fs;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 8;

/// Recently opened `.ssproj` paths, most recent first.
#[derive(Clone, Debug, Default)]
pub struct RecentFiles {
    pub paths: Vec<PathBuf>,
}

impl RecentFiles {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(paths) = serde_json::from_str::<Vec<PathBuf>>(&data) {
                let paths: Vec<PathBuf> = paths
                    .into_iter()
                    .filter(|p| p.exists())
                    .take(MAX_RECENT)
                    .collect();
                return Self { paths };
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = config_path();
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

fn config_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|appdata| PathBuf::from(appdata).join("StormSewer").join("recent.json"))
        .unwrap_or_else(|| PathBuf::from("recent.json"))
}