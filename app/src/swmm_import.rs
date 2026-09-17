// SPDX-License-Identifier: GPL-3.0-or-later

//! File → Import / Export for the SWMM model editor: a StormSewer project
//! (`.ssproj`, or the older `.ssn` text network) becomes a new SWMM model,
//! the open model becomes a StormSewer project, and the network goes out as
//! CSV or GeoJSON. The mapping in both directions and its assumptions live
//! in `stormsewer_swmm::design`; the CSV/GeoJSON writers in
//! `stormsewer_swmm::net_export`.

use std::path::{Path, PathBuf};

use eframe::egui::{Button, Ui};
use stormsewer::io::project::Project;
use stormsewer::parse::parse_ssn;
use stormsewer_swmm::design::{import_commands, to_project, ImportOptions};
use stormsewer_swmm::net_export;

use crate::state::AppState;

/// Read a `.ssproj` (JSON) or `.ssn` (text) file as a project.
pub fn load_project_file(path: &Path) -> Result<Project, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "ssn" {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let parsed = parse_ssn(&text)?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("network");
        Ok(Project::from_network(
            &parsed.network,
            name,
            &parsed.idf,
            &parsed.options,
        ))
    } else {
        Project::load(path)
    }
}

/// Replace the open model with a new one built from `project`. One undo
/// step ("import project"); the model has no path until saved. Returns the
/// import notes.
pub fn import_project(state: &mut AppState, project: &Project, opts: &ImportOptions) -> Result<Vec<String>, String> {
    let (cmd, notes) = import_commands(project, opts);
    state.swmm_doc.new_model();
    if !state.swmm_doc.apply(cmd, "import project") {
        return Err(state
            .swmm_doc
            .last_error
            .clone()
            .unwrap_or_else(|| "import failed".into()));
    }
    state.swmm_doc.refresh();
    state.swmm.pending_map_fit = true;
    state.swmm_doc.pending_zoom_to = None;
    Ok(notes)
}

/// Import a project file into a new model and report in the status line.
pub fn import_project_file(state: &mut AppState, path: &Path) {
    match load_project_file(path).and_then(|p| import_project(state, &p, &ImportOptions::default())) {
        Ok(notes) => {
            state.status = format!(
                "Imported {} into a new SWMM model ({} node(s), {} link(s)); {}",
                path.display(),
                state.swmm_doc.nodes.len(),
                state.swmm_doc.links.len(),
                notes.first().cloned().unwrap_or_default()
            );
        }
        Err(e) => state.status = format!("Import failed: {e}"),
    }
}

/// Write the open model as a StormSewer project. Returns the mapping's
/// skipped list so the caller can say what did not make it across.
pub fn export_project_file(state: &AppState, path: &Path) -> Result<Vec<String>, String> {
    let mapping = to_project(&state.swmm_doc.doc, &state.project)?;
    let mut project = mapping.project;
    if project.name.is_empty() || project.name == state.project.name {
        project.name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("model")
            .to_string();
    }
    project.save(path)?;
    Ok(mapping
        .skipped
        .iter()
        .map(|s| format!("{} {}: {}", s.kind, s.name, s.reason))
        .collect())
}

/// `<stem>-nodes.csv` and `<stem>-links.csv` beside the chosen path.
pub fn export_network_csv(state: &AppState, path: &Path) -> Result<(PathBuf, PathBuf), String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("network")
        .trim_end_matches("-nodes")
        .trim_end_matches("-links")
        .to_string();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let nodes = dir.join(format!("{stem}-nodes.csv"));
    let links = dir.join(format!("{stem}-links.csv"));
    std::fs::write(&nodes, net_export::nodes_csv(&state.swmm_doc.doc))
        .map_err(|e| format!("cannot write {}: {e}", nodes.display()))?;
    std::fs::write(&links, net_export::links_csv(&state.swmm_doc.doc))
        .map_err(|e| format!("cannot write {}: {e}", links.display()))?;
    Ok((nodes, links))
}

pub fn export_geojson(state: &AppState, path: &Path) -> Result<(), String> {
    std::fs::write(path, net_export::geojson(&state.swmm_doc.doc))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn model_stem(state: &AppState) -> String {
    PathBuf::from(state.swmm_doc.file_name())
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model")
        .to_string()
}

fn save_dialog(state: &AppState, suffix: &str, filter: &str, exts: &[&str]) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(filter, exts)
        .set_file_name(format!("{}{suffix}", model_stem(state)))
        .save_file()
}

/// The File → Import submenu.
pub fn import_menu_items(ui: &mut Ui, state: &mut AppState) {
    ui.menu_button("Import", |ui| {
        if ui
            .button("StormSewer Project (.ssproj / .ssn)…")
            .on_hover_text("New SWMM model from a storm-sewer project: nodes, conduits, catchments, and a design storm from its IDF curve")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("StormSewer project", &["ssproj", "ssn"])
                .pick_file()
            {
                import_project_file(state, &path);
            }
            ui.close_menu();
        }
    });
}

/// The File → Export submenu.
pub fn export_menu_items(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.swmm_doc.loaded;
    ui.menu_button("Export", |ui| {
        if ui
            .add_enabled(loaded, Button::new("StormSewer Project (.ssproj)…"))
            .clicked()
        {
            if let Some(path) = save_dialog(state, ".ssproj", "StormSewer project", &["ssproj"]) {
                match export_project_file(state, &path) {
                    Ok(skipped) if skipped.is_empty() => {
                        state.status = format!("Project saved: {}", path.display());
                    }
                    Ok(skipped) => {
                        state.status = format!(
                            "Project saved: {} — {} object(s) not exported: {}",
                            path.display(),
                            skipped.len(),
                            skipped.join("; ")
                        );
                    }
                    Err(e) => state.status = format!("Export failed: {e}"),
                }
            }
            ui.close_menu();
        }
        if ui
            .add_enabled(loaded, Button::new("Network CSV (nodes, links)…"))
            .clicked()
        {
            if let Some(path) = save_dialog(state, "-nodes.csv", "CSV", &["csv"]) {
                match export_network_csv(state, &path) {
                    Ok((n, l)) => state.status = format!("CSV saved: {} and {}", n.display(), l.display()),
                    Err(e) => state.status = format!("Export failed: {e}"),
                }
            }
            ui.close_menu();
        }
        if ui
            .add_enabled(loaded, Button::new("GeoJSON (map units, no CRS)…"))
            .clicked()
        {
            if let Some(path) = save_dialog(state, ".geojson", "GeoJSON", &["geojson", "json"]) {
                match export_geojson(state, &path) {
                    Ok(()) => state.status = format!("GeoJSON saved: {}", path.display()),
                    Err(e) => state.status = format!("Export failed: {e}"),
                }
            }
            ui.close_menu();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-app-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_project_becomes_a_model_and_comes_back_with_its_pipes() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        let project = Project::demo();
        assert!(!project.pipes.is_empty(), "the demo project has pipes");
        let path = temp_dir().join("swmm-import-sample.ssproj");
        project.save(&path).unwrap();

        import_project_file(&mut app.state, &path);
        assert!(app.state.status.starts_with("Imported"), "{}", app.state.status);
        assert!(app.state.swmm_doc.loaded);
        assert!(app.state.swmm_doc.path.is_none());
        assert_eq!(app.state.swmm_doc.undo_label(), Some("import project"));
        assert_eq!(app.state.swmm_doc.undo_depth(), 1);
        assert_eq!(app.state.swmm_doc.links.len(), project.pipes.len());
        assert_eq!(app.state.swmm_doc.nodes.len(), project.nodes.len());
        assert!(app.state.swmm_doc.doc.rows("TIMESERIES").len() > 10);
        run_frame(&mut app);

        // Export the model as a project: every pipe's geometry survives.
        let back = temp_dir().join("swmm-export-sample.ssproj");
        let skipped = export_project_file(&app.state, &back).unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        let p2 = Project::load(&back).unwrap();
        assert_eq!(p2.pipes.len(), project.pipes.len());
        for a in &project.pipes {
            let b = p2.pipes.iter().find(|p| p.id == a.id).unwrap();
            assert!((a.length - b.length).abs() < 0.01, "{}", a.id);
            assert!((a.diameter - b.diameter).abs() < 1e-6, "{}", a.id);
            assert!((a.n - b.n).abs() < 1e-6, "{}", a.id);
            if let (Some(x), Some(y)) = (a.invert_up, b.invert_up) {
                assert!((x - y).abs() < 0.01, "{}", a.id);
            }
        }
    }

    #[test]
    fn ssn_networks_import_too() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        let path = temp_dir().join("swmm-import-sample.ssn");
        std::fs::write(
            &path,
            "IDF 120 10 0.8\nNODE A inlet 0 0 100 108 1.0 0.8 10\nNODE B inlet 200 0 99 107 0.5 0.7 10\nNODE OUT outfall 350 0 97 100\nPIPE P1 A B 200 1.5 0.013\nPIPE P2 B OUT 150 1.5 0.013\n",
        )
        .unwrap();
        let p = load_project_file(&path).unwrap();
        assert_eq!(p.pipes.len(), 2);
        import_project_file(&mut app.state, &path);
        assert!(app.state.status.starts_with("Imported"), "{}", app.state.status);
        assert_eq!(app.state.swmm_doc.links.len(), 2);
        assert_eq!(app.state.swmm_doc.nodes.len(), 3);
        // A bad file reaches the status line, not a panic.
        let bad = temp_dir().join("swmm-import-bad.ssn");
        std::fs::write(&bad, "PIPE P1 A B\n").unwrap();
        import_project_file(&mut app.state, &bad);
        assert!(app.state.status.starts_with("Import failed"), "{}", app.state.status);
    }

    #[test]
    fn csv_and_geojson_exports_write_files_for_the_pond() {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), None);
        let (n, l) = export_network_csv(&app.state, &temp_dir().join("pond-nodes.csv")).unwrap();
        assert!(n.ends_with("pond-nodes.csv") && l.ends_with("pond-links.csv"));
        let nodes = std::fs::read_to_string(&n).unwrap();
        assert_eq!(nodes.lines().count(), 15, "header + 14 nodes");
        let links = std::fs::read_to_string(&l).unwrap();
        assert_eq!(links.lines().count(), 15, "header + 14 links");
        let g = temp_dir().join("pond.geojson");
        export_geojson(&app.state, &g).unwrap();
        let text = std::fs::read_to_string(&g).unwrap();
        assert!(text.contains("\"FeatureCollection\"") && text.contains("coordinate reference system"));
        // The menus draw with a model open.
        let mut state = std::mem::replace(&mut app.state, AppState::new_empty());
        let ctx = eframe::egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                import_menu_items(ui, &mut state);
                export_menu_items(ui, &mut state);
            });
        });
    }
}
