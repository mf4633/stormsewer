// SPDX-License-Identifier: GPL-3.0-or-later

//! The open SWMM model as the editor sees it: the lossless [`InpDoc`], its
//! file, drawing caches rebuilt whenever the document's text changes, the
//! selection, the clipboard, undo labels, and the file operations.
//!
//! The map is drawn from these caches and the caches come only from the
//! document, so the map and the text can never disagree. Undo and redo go
//! through the document's own history; this type only keeps the
//! human-readable label for each step so the Edit menu can say what
//! Ctrl+Z will do.

use std::path::{Path, PathBuf};

use stormsewer_swmm::doc::build::{self, Clipboard, DeleteImpact, LinkType, NodeType, ObjRef};
use stormsewer_swmm::doc::{Command, Finding, InpDoc, ObjectKind, Severity};

use crate::recent::RecentFiles;
use crate::swmm_tools::{centroid, SwmmEditState, SwmmTool};

/// Where the `.inp` recent-files list is kept, apart from the project list.
const RECENT_INP: &str = "recent-inp.json";

#[derive(Clone, Debug, PartialEq)]
pub struct DrawNode {
    pub name: String,
    pub kind: NodeType,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawLink {
    pub name: String,
    pub kind: LinkType,
    pub from: String,
    pub to: String,
    /// The interior vertices, as in `[VERTICES]`.
    pub vertices: Vec<(f64, f64)>,
    /// Start node, vertices, end node — empty when an end has no position.
    pub path: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawSub {
    pub name: String,
    pub polygon: Vec<(f64, f64)>,
    pub centroid: Option<(f64, f64)>,
    pub outlet: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawGage {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawLabel {
    /// Line index in `[LABELS]`, which is how the row is addressed.
    pub line: usize,
    pub x: f64,
    pub y: f64,
    pub text: String,
}

/// Something the user asked for while the model had unsaved changes; done
/// once they choose Save, Discard, or nothing on Cancel.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    New,
    Open,
    OpenPath(PathBuf),
}

pub struct SwmmEditor {
    /// The SWMM editor is the workspace on screen (else the storm-sewer
    /// design view is).
    pub active: bool,
    pub doc: InpDoc,
    pub path: Option<PathBuf>,
    /// A model is open — new or from a file. Nothing is drawn or editable
    /// until one is.
    pub loaded: bool,
    pub recent: RecentFiles,
    recent_loaded: bool,
    cache_gen: Option<u64>,
    pub nodes: Vec<DrawNode>,
    pub links: Vec<DrawLink>,
    pub subs: Vec<DrawSub>,
    pub gages: Vec<DrawGage>,
    pub labels: Vec<DrawLabel>,
    pub bounds: Option<(f64, f64, f64, f64)>,
    pub findings: Vec<Finding>,
    undo_labels: Vec<String>,
    redo_labels: Vec<String>,
    gesture_label: Option<String>,
    pub selection: Vec<ObjRef>,
    pub clipboard: Clipboard,
    paste_count: u32,
    pub edit: SwmmEditState,
    pub show_labels: bool,
    pub show_arrows: bool,
    pub show_grid: bool,
    pub snap_objects: bool,
    pub snap_grid: bool,
    pub grid_spacing: f64,
    pub pending: Option<PendingAction>,
    /// A delete that takes more than the selection with it, awaiting the
    /// user's yes.
    pub delete_confirm: Option<(Vec<ObjRef>, DeleteImpact)>,
    /// Errors that stopped a run, shown until dismissed.
    pub run_refused: Option<Vec<Finding>>,
    pub show_findings: bool,
    /// The canvas zooms to this object on its next frame.
    pub pending_zoom_to: Option<ObjRef>,
    /// Finished runs this session, oldest first (Results → Compare Runs).
    pub run_history: Vec<crate::swmm_compare::RunRecord>,
    /// The last command that failed, for the status bar.
    pub last_error: Option<String>,
    /// Draw a placeholder property sheet for the selection.
    pub show_properties: bool,
    /// The document generation the runner's `InpModel` inventory was last
    /// parsed from (see `AppState::sync_swmm_editor`).
    pub inventory_gen: Option<u64>,
    /// Which tab the left pane shows: the project browser or the layers.
    pub left_tab: LeftTab,
    /// The property sheet's editing state (draft text, inline error).
    pub sheet: crate::swmm_props::SheetState,
    /// The attribute table window.
    pub grid: crate::swmm_grids::GridState,
    /// The project browser's search text and chosen kind.
    pub browser: crate::swmm_browser::BrowserState,
    /// The project dialogs (Title, Options, Curves, ...) and their drafts.
    pub dialogs: crate::swmm_dialogs::Dialogs,
    /// Profile pick mode: `Some(None)` waits for the start node,
    /// `Some(Some(start))` for the end node.
    pub profile_pick: Option<Option<String>>,
    /// The property sheet focuses its first field on its next frame.
    pub focus_sheet: bool,
    /// Layer visibility, labels, sizes and colours (mirrored to prefs).
    pub layers: crate::swmm_layers::LayerSettings,
    /// A text field had keyboard focus at the end of the last frame, so an
    /// Escape this frame is the field's (egui drops the focus before the
    /// shortcuts run).
    pub had_focus: bool,
    /// The map's fit/label/nudge bookkeeping (see `swmm_canvas`).
    pub canvas: crate::swmm_canvas::CanvasState,
    /// The `[BACKDROP]` image as loaded for drawing.
    pub backdrop: crate::swmm_backdrop::BackdropView,
    /// How the last run's model was laid out for the engine (scratch copy,
    /// copied files, SAVE outputs to copy back). See `run_path`.
    pub run_prep: Option<stormsewer_swmm::engine::Prepared>,
    /// The run status window and the error-code index.
    pub run_panel: crate::swmm_run_panel::RunPanelState,
    /// Run → Check Model and the warnings-before-run prompt.
    pub qa: crate::swmm_qa::QaState,
    /// The unit-switch wizard, while it is open.
    pub units_wizard: Option<crate::swmm_units::UnitsWizard>,
    /// Autosave clock and the restore offer.
    pub recovery: crate::swmm_recovery::RecoveryState,
    pub gis: crate::swmm_gis::GisState,
    pub twod: crate::swmm_twod::TwoDState,
    pub lid: crate::swmm_lid::LidState,
    pub scenarios: crate::swmm_scenarios::ScenarioState,
    pub calib: crate::swmm_calib::CalibState,
    pub live: crate::swmm_live::LiveState,
}

/// The left pane's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeftTab {
    #[default]
    Browser,
    Layers,
}

impl Default for SwmmEditor {
    fn default() -> Self {
        Self {
            active: false,
            doc: InpDoc::default(),
            path: None,
            loaded: false,
            recent: RecentFiles::default(),
            recent_loaded: false,
            cache_gen: None,
            nodes: Vec::new(),
            links: Vec::new(),
            subs: Vec::new(),
            gages: Vec::new(),
            labels: Vec::new(),
            bounds: None,
            findings: Vec::new(),
            undo_labels: Vec::new(),
            redo_labels: Vec::new(),
            gesture_label: None,
            selection: Vec::new(),
            clipboard: Clipboard::default(),
            paste_count: 0,
            edit: SwmmEditState::default(),
            show_labels: true,
            show_arrows: true,
            show_grid: true,
            snap_objects: true,
            snap_grid: false,
            grid_spacing: 10.0,
            pending: None,
            delete_confirm: None,
            run_refused: None,
            show_findings: false,
            pending_zoom_to: None,
            run_history: Vec::new(),
            last_error: None,
            show_properties: true,
            inventory_gen: None,
            left_tab: LeftTab::Browser,
            sheet: Default::default(),
            grid: Default::default(),
            browser: Default::default(),
            dialogs: Default::default(),
            profile_pick: None,
            focus_sheet: false,
            layers: Default::default(),
            had_focus: false,
            canvas: Default::default(),
            backdrop: crate::swmm_backdrop::BackdropView::new(),
            run_prep: None,
            run_panel: Default::default(),
            qa: Default::default(),
            units_wizard: None,
            recovery: Default::default(),
            gis: Default::default(),
            twod: Default::default(),
            lid: Default::default(),
            scenarios: Default::default(),
            calib: Default::default(),
            live: Default::default(),
        }
    }
}

impl SwmmEditor {
    // -- files ---------------------------------------------------------------

    /// The recent list is read from disk the first time it is wanted, not
    /// in `Default`, which tests construct freely.
    pub fn ensure_recent(&mut self) {
        if !self.recent_loaded {
            self.recent = RecentFiles::load_named(RECENT_INP);
            self.recent_loaded = true;
        }
    }

    fn install(&mut self, doc: InpDoc, path: Option<PathBuf>) {
        // Closing the previous model retires its autosave — unless the
        // same file is being reopened, when the snapshot is what the
        // recovery offer is about.
        if self.loaded && self.path != path {
            crate::swmm_recovery::remove_for(self.path.as_deref());
        }
        self.recovery = Default::default();
        self.qa = Default::default();
        self.units_wizard = None;
        self.run_prep = None;
        self.replace_document(doc);
        self.path = path;
    }

    /// Swap in a document for the current path (recovery, and `install`).
    pub fn replace_document(&mut self, doc: InpDoc) {
        self.doc = doc;
        self.loaded = true;
        self.cache_gen = None;
        self.undo_labels.clear();
        self.redo_labels.clear();
        self.gesture_label = None;
        self.selection.clear();
        self.edit.cancel();
        self.edit.active_vertex = None;
        self.paste_count = 0;
        self.last_error = None;
        self.run_refused = None;
        self.delete_confirm = None;
        self.inventory_gen = None;
        self.refresh();
    }

    /// A blank, runnable model.
    pub fn new_model(&mut self) {
        self.install(InpDoc::parse(&build::new_model_text()), None);
    }

    /// Open a model from text, as if read from `path` (no file access).
    #[cfg(test)]
    pub fn open_text(&mut self, text: &str, path: Option<PathBuf>) {
        self.install(InpDoc::parse(text), path);
    }

    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let doc = InpDoc::read(path).map_err(|e| e.to_string())?;
        self.install(doc, Some(path.to_path_buf()));
        self.remember(path);
        crate::swmm_recovery::check_on_open(self, path);
        Ok(())
    }

    /// Save to the model's own path. `Ok(false)` means it has none yet.
    pub fn save(&mut self) -> Result<bool, String> {
        let Some(path) = self.path.clone() else {
            return Ok(false);
        };
        self.save_as(path)?;
        Ok(true)
    }

    pub fn save_as(&mut self, path: PathBuf) -> Result<(), String> {
        self.doc.write(&path).map_err(|e| e.to_string())?;
        self.remember(&path);
        // A clean save supersedes the autosave, under either name.
        crate::swmm_recovery::remove_for(self.path.as_deref());
        crate::swmm_recovery::remove_for(Some(&path));
        self.path = Some(path);
        Ok(())
    }

    /// Add to the recent list — except under test, where every path is a
    /// scratch file and the user's list is not ours to touch.
    fn remember(&mut self, path: &Path) {
        if cfg!(test) {
            return;
        }
        self.ensure_recent();
        self.recent.push(path.to_path_buf());
    }

    pub fn pick_open(&mut self) -> Option<Result<(), String>> {
        let path = rfd::FileDialog::new()
            .add_filter("SWMM Input", &["inp", "INP"])
            .pick_file()?;
        Some(self.open_path(&path))
    }

    pub fn pick_save_as(&mut self) -> Option<Result<(), String>> {
        let mut dlg = rfd::FileDialog::new().add_filter("SWMM Input", &["inp"]);
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled.inp");
        dlg = dlg.set_file_name(name);
        let path = dlg.save_file()?;
        Some(self.save_as(path))
    }

    /// Save, asking for a path when there is none. Returns whether the
    /// model is clean afterwards.
    pub fn save_or_pick(&mut self) -> Result<bool, String> {
        match self.save()? {
            true => Ok(true),
            false => match self.pick_save_as() {
                Some(r) => r.map(|_| true),
                None => Ok(false),
            },
        }
    }

    pub fn dirty(&self) -> bool {
        self.loaded && self.doc.dirty()
    }

    /// Ask for an action that discards the open model; deferred behind the
    /// unsaved-changes prompt when the model is dirty.
    pub fn request(&mut self, action: PendingAction) {
        if self.dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    /// Carry out a deferred action now.
    pub fn perform(&mut self, action: PendingAction) {
        self.pending = None;
        match action {
            PendingAction::New => self.new_model(),
            PendingAction::Open => {
                if let Some(Err(e)) = self.pick_open() {
                    self.last_error = Some(e);
                }
            }
            PendingAction::OpenPath(p) => {
                if let Err(e) = self.open_path(&p) {
                    self.last_error = Some(e);
                }
            }
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| "Untitled.inp".into())
    }

    pub fn window_title(&self) -> String {
        let mut t = self.file_name();
        if self.dirty() {
            t.push('*');
        }
        format!(
            "{t} — SWMM model — StormSewer v{}",
            env!("CARGO_PKG_VERSION")
        )
    }

    /// The file to hand the engine: the model's own file when it is saved,
    /// clean, and on an ASCII path; else a scratch copy of the current text
    /// under `%TEMP%\StormSewer\run\<hash>` with its `[FILES]` and data
    /// files carried along (`stormsewer_swmm::engine::prepare`). The
    /// layout is kept in `run_prep` for the run panel. Refused with the
    /// error findings when the model has any.
    pub fn run_path(&mut self) -> Result<PathBuf, Vec<Finding>> {
        self.refresh();
        let errors: Vec<Finding> = self
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .cloned()
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        let force = self.path.is_none() || self.doc.dirty();
        let model = self
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(self.file_name()));
        match stormsewer_swmm::engine::prepare(&model, &self.doc.to_string(), force) {
            Ok(p) => {
                let inp = p.paths.inp.clone();
                self.run_prep = Some(p);
                Ok(inp)
            }
            Err(e) => Err(vec![Finding {
                severity: Severity::Error,
                section: String::new(),
                name: String::new(),
                message: format!("could not lay the model out for the engine: {e}"),
                column: None,
            }]),
        }
    }

    // -- caches ----------------------------------------------------------------

    /// Rebuild the drawing caches and findings if the document changed since
    /// the last rebuild. Returns whether it did.
    pub fn refresh(&mut self) -> bool {
        if !self.loaded {
            return false;
        }
        let gen = self.doc.generation();
        if self.cache_gen == Some(gen) {
            return false;
        }
        self.cache_gen = Some(gen);
        let doc = &self.doc;

        self.nodes.clear();
        for kind in NodeType::ALL {
            for name in doc.names(kind.section()) {
                if let Some((x, y)) = doc.coordinates(&name) {
                    self.nodes.push(DrawNode { name, kind, x, y });
                }
            }
        }
        self.gages = doc
            .names("RAINGAGES")
            .into_iter()
            .filter_map(|name| doc.symbol(&name).map(|(x, y)| DrawGage { name, x, y }))
            .collect();
        let node_pos = |name: &str| -> Option<(f64, f64)> {
            self.nodes
                .iter()
                .find(|n| n.name.eq_ignore_ascii_case(name))
                .map(|n| (n.x, n.y))
        };
        self.links.clear();
        for kind in LinkType::ALL {
            for (_, row) in doc.rows(kind.section()) {
                let (Some(name), Some(from), Some(to)) = (row.value(0), row.value(1), row.value(2))
                else {
                    continue;
                };
                let vertices = doc.vertices(name);
                let mut path = Vec::new();
                if let (Some(a), Some(b)) = (node_pos(from), node_pos(to)) {
                    path.push(a);
                    path.extend(vertices.iter().copied());
                    path.push(b);
                }
                self.links.push(DrawLink {
                    name: name.to_string(),
                    kind,
                    from: from.to_string(),
                    to: to.to_string(),
                    vertices,
                    path,
                });
            }
        }
        self.subs = doc
            .rows("SUBCATCHMENTS")
            .into_iter()
            .filter_map(|(_, row)| {
                let name = row.value(0)?.to_string();
                let polygon = doc.polygon(&name);
                Some(DrawSub {
                    centroid: centroid(&polygon),
                    outlet: row.value(2).unwrap_or("").to_string(),
                    name,
                    polygon,
                })
            })
            .collect();
        self.labels = doc
            .section("LABELS")
            .map(|s| {
                s.rows()
                    .filter_map(|(line, row)| {
                        Some(DrawLabel {
                            line,
                            x: row.value(0)?.parse().ok()?,
                            y: row.value(1)?.parse().ok()?,
                            text: row.value(2).unwrap_or("").to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut b: Option<(f64, f64, f64, f64)> = None;
        let mut grow = |x: f64, y: f64| {
            b = Some(match b {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        };
        for n in &self.nodes {
            grow(n.x, n.y);
        }
        for g in &self.gages {
            grow(g.x, g.y);
        }
        for l in &self.links {
            for &(x, y) in &l.vertices {
                grow(x, y);
            }
        }
        for s in &self.subs {
            for &(x, y) in &s.polygon {
                grow(x, y);
            }
        }
        for l in &self.labels {
            grow(l.x, l.y);
        }
        self.bounds = b;

        self.findings = doc.validate();
        self.sync_labels();
        // An undo can remove what was selected.
        let doc = &self.doc;
        let labels = &self.labels;
        self.selection.retain(|r| match r {
            ObjRef::Label(li) => labels.iter().any(|l| l.line == *li),
            _ => r
                .kind()
                .zip(r.name())
                .is_some_and(|(k, n)| doc.defining_section(k, n).is_some()),
        });
        if let Some((target, _)) = &self.edit.active_vertex {
            if !self.selection.contains(target) {
                self.edit.active_vertex = None;
            }
        }
        true
    }

    pub fn node(&self, name: &str) -> Option<&DrawNode> {
        self.nodes
            .iter()
            .find(|n| n.name.eq_ignore_ascii_case(name))
    }

    pub fn link(&self, name: &str) -> Option<&DrawLink> {
        self.links
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    pub fn sub(&self, name: &str) -> Option<&DrawSub> {
        self.subs.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn gage(&self, name: &str) -> Option<&DrawGage> {
        self.gages
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(name))
    }

    /// The closest node within `radius` world units.
    pub fn nearest_node(&self, x: f64, y: f64, radius: f64) -> Option<&DrawNode> {
        self.nodes
            .iter()
            .map(|n| (n, ((n.x - x).powi(2) + (n.y - y).powi(2)).sqrt()))
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    }

    /// World bounds of one object, for zoom-to.
    pub fn bounds_of(&self, r: &ObjRef) -> Option<(f64, f64, f64, f64)> {
        let pts: Vec<(f64, f64)> = match r {
            ObjRef::Node(n) => vec![self.node(n).map(|n| (n.x, n.y))?],
            ObjRef::Gage(n) => vec![self.gage(n).map(|g| (g.x, g.y))?],
            ObjRef::Link(n) => self.link(n)?.path.clone(),
            ObjRef::Subcatchment(n) => self.sub(n)?.polygon.clone(),
            ObjRef::Label(li) => vec![self
                .labels
                .iter()
                .find(|l| l.line == *li)
                .map(|l| (l.x, l.y))?],
        };
        let mut it = pts.into_iter();
        let (x, y) = it.next()?;
        Some(it.fold((x, y, x, y), |(x0, y0, x1, y1), (x, y)| {
            (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
        }))
    }

    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }

    /// The map object a finding is about, if it is about one.
    pub fn finding_target(&self, f: &Finding) -> Option<ObjRef> {
        if f.name.is_empty() {
            return None;
        }
        let sec = f.section.as_str();
        let kind = if NodeType::from_section(sec).is_some()
            || matches!(
                sec,
                "COORDINATES" | "INFLOWS" | "DWF" | "RDII" | "TREATMENT"
            ) {
            ObjectKind::Node
        } else if LinkType::from_section(sec).is_some()
            || matches!(sec, "XSECTIONS" | "VERTICES" | "LOSSES")
        {
            ObjectKind::Link
        } else if matches!(
            sec,
            "SUBCATCHMENTS" | "SUBAREAS" | "INFILTRATION" | "POLYGONS" | "LID_USAGE"
        ) {
            ObjectKind::Subcatchment
        } else if matches!(sec, "RAINGAGES" | "SYMBOLS") {
            ObjectKind::Gage
        } else {
            return None;
        };
        self.doc.defining_section(kind, &f.name)?;
        Some(match kind {
            ObjectKind::Node => ObjRef::Node(f.name.clone()),
            ObjectKind::Link => ObjRef::Link(f.name.clone()),
            ObjectKind::Subcatchment => ObjRef::Subcatchment(f.name.clone()),
            _ => ObjRef::Gage(f.name.clone()),
        })
    }

    // -- commands and undo -----------------------------------------------------

    /// Keep the label stacks the same height as the document's history:
    /// the bounded history drops old steps from the bottom, and a gesture
    /// that changed nothing adds no step.
    fn sync_labels(&mut self) {
        let depth = self.doc.undo_depth();
        while self.undo_labels.len() > depth {
            self.undo_labels.remove(0);
        }
        while self.undo_labels.len() < depth {
            self.undo_labels.insert(0, "edit".into());
        }
        if !self.doc.can_redo() {
            self.redo_labels.clear();
        }
    }

    fn note_step(&mut self, label: &str) {
        let depth = self.doc.undo_depth();
        if depth > self.undo_labels.len() {
            self.undo_labels.push(label.to_string());
            self.redo_labels.clear();
        }
        self.sync_labels();
    }

    /// Apply one command as one undo step (or as part of the open
    /// gesture). A failure leaves the document unchanged and is reported in
    /// `last_error` and the return value.
    pub fn apply(&mut self, cmd: Command, label: &str) -> bool {
        match self.doc.apply(cmd) {
            Ok(()) => {
                if self.gesture_label.is_none() {
                    self.note_step(label);
                }
                self.last_error = None;
                true
            }
            Err(e) => {
                self.last_error = Some(format!("{label}: {e}"));
                false
            }
        }
    }

    /// Fold every command until [`Self::end_gesture`] into one step.
    pub fn begin_gesture(&mut self, label: &str) {
        if self.gesture_label.is_none() {
            self.gesture_label = Some(label.to_string());
        }
        self.doc.begin_gesture();
    }

    pub fn end_gesture(&mut self) {
        self.doc.end_gesture();
        if let Some(label) = self.gesture_label.take() {
            self.note_step(&label);
        }
    }

    pub fn can_undo(&self) -> bool {
        self.loaded && self.doc.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.loaded && self.doc.can_redo()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo_labels.last().map(String::as_str)
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo_labels.last().map(String::as_str)
    }

    /// Undo one step; the label of what was undone.
    pub fn undo(&mut self) -> Option<String> {
        if !self.loaded {
            return None;
        }
        if let Some(label) = self.gesture_label.take() {
            // Undo mid-gesture closes it first, as the document does.
            self.doc.end_gesture();
            self.note_step(&label);
        }
        self.edit.cancel();
        if !self.doc.undo() {
            return None;
        }
        let label = self.undo_labels.pop().unwrap_or_else(|| "edit".into());
        self.redo_labels.push(label.clone());
        self.sync_labels();
        Some(label)
    }

    pub fn redo(&mut self) -> Option<String> {
        if !self.loaded || !self.doc.redo() {
            return None;
        }
        let label = self.redo_labels.pop().unwrap_or_else(|| "edit".into());
        self.undo_labels.push(label.clone());
        self.sync_labels();
        Some(label)
    }

    pub fn undo_depth(&self) -> usize {
        self.doc.undo_depth()
    }

    // -- selection ---------------------------------------------------------------

    pub fn is_selected(&self, r: &ObjRef) -> bool {
        self.selection.iter().any(|s| s == r)
    }

    pub fn select_only(&mut self, r: ObjRef) {
        self.selection.clear();
        self.selection.push(r);
        self.edit.active_vertex = None;
    }

    pub fn add_select(&mut self, r: ObjRef) {
        if !self.is_selected(&r) {
            self.selection.push(r);
        }
    }

    pub fn toggle_select(&mut self, r: ObjRef) {
        match self.selection.iter().position(|s| *s == r) {
            Some(i) => {
                self.selection.remove(i);
            }
            None => self.selection.push(r),
        }
        self.edit.active_vertex = None;
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.edit.active_vertex = None;
    }

    /// Replace the selection with `items` (duplicates dropped).
    pub fn select_many(&mut self, items: Vec<ObjRef>) {
        self.selection.clear();
        for r in items {
            self.add_select(r);
        }
        self.edit.active_vertex = None;
    }

    /// The map object a row of `section` stands for, when the section
    /// defines map objects.
    pub fn objref_for(section: &str, name: &str) -> Option<ObjRef> {
        let sec = section.to_ascii_uppercase();
        if NodeType::from_section(&sec).is_some() {
            Some(ObjRef::Node(name.to_string()))
        } else if LinkType::from_section(&sec).is_some() {
            Some(ObjRef::Link(name.to_string()))
        } else if sec == "SUBCATCHMENTS" {
            Some(ObjRef::Subcatchment(name.to_string()))
        } else if sec == "RAINGAGES" {
            Some(ObjRef::Gage(name.to_string()))
        } else {
            None
        }
    }

    /// The defining section and name of every selected named object.
    pub fn selected_rows(&self) -> Vec<(&'static str, String)> {
        self.selection
            .iter()
            .filter_map(|r| {
                let (k, n) = r.kind().zip(r.name())?;
                let sec = self.doc.defining_section(k, n)?;
                Some((sec, n.to_string()))
            })
            .collect()
    }

    /// The selected nodes, in selection order.
    pub fn selected_nodes(&self) -> Vec<String> {
        self.selection
            .iter()
            .filter_map(|r| match r {
                ObjRef::Node(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    /// After a rename, point the selection at the new name.
    pub fn rename_in_selection(&mut self, old: &str, new: &str) {
        for r in &mut self.selection {
            let renamed = match r {
                ObjRef::Node(n) | ObjRef::Link(n) | ObjRef::Subcatchment(n) | ObjRef::Gage(n) => {
                    n.eq_ignore_ascii_case(old).then_some(n)
                }
                ObjRef::Label(_) => None,
            };
            if let Some(n) = renamed {
                *n = new.to_string();
            }
        }
    }

    pub fn select_all(&mut self) {
        self.selection.clear();
        self.selection
            .extend(self.gages.iter().map(|g| ObjRef::Gage(g.name.clone())));
        self.selection.extend(
            self.subs
                .iter()
                .map(|s| ObjRef::Subcatchment(s.name.clone())),
        );
        self.selection
            .extend(self.nodes.iter().map(|n| ObjRef::Node(n.name.clone())));
        self.selection
            .extend(self.links.iter().map(|l| ObjRef::Link(l.name.clone())));
        self.selection
            .extend(self.labels.iter().map(|l| ObjRef::Label(l.line)));
        self.edit.active_vertex = None;
    }

    /// Short description of the selection for the status bar.
    pub fn selection_summary(&self) -> String {
        match self.selection.as_slice() {
            [] => String::new(),
            [one] => describe(one),
            many => format!("{} selected", many.len()),
        }
    }

    pub fn set_tool(&mut self, tool: SwmmTool) {
        self.edit.cancel();
        self.edit.tool = tool;
    }

    // -- structural edits --------------------------------------------------------

    /// Delete the selection. Returns what was deleted, or `None` when a
    /// confirmation is now pending (the cascade takes unselected links) or
    /// nothing was selected.
    pub fn delete_selection(&mut self) -> Option<usize> {
        if self.selection.is_empty() {
            return None;
        }
        let items = self.selection.clone();
        let impact = build::delete_impact(&self.doc, &items);
        if !impact.is_empty() {
            self.delete_confirm = Some((items, impact));
            return None;
        }
        Some(self.delete_items(&items))
    }

    /// Delete `items` with their dependent rows as one step.
    pub fn delete_items(&mut self, items: &[ObjRef]) -> usize {
        let cmd = build::cascade_delete(&self.doc, items);
        let label = match items {
            [one] => format!("delete {}", describe(one)),
            many => format!("delete {} objects", many.len()),
        };
        if self.apply(cmd, &label) {
            self.clear_selection();
            items.len()
        } else {
            0
        }
    }

    /// Carry out the confirmed delete.
    pub fn confirm_delete(&mut self) -> usize {
        match self.delete_confirm.take() {
            Some((items, _)) => self.delete_items(&items),
            None => 0,
        }
    }

    pub fn copy(&mut self) -> usize {
        self.clipboard = build::copy(&self.doc, &self.selection);
        self.paste_count = 0;
        self.clipboard.len()
    }

    pub fn cut(&mut self) -> usize {
        let n = self.copy();
        if n > 0 {
            let items = self.selection.clone();
            self.delete_items(&items);
        }
        n
    }

    /// Paste the clipboard offset by a fraction of the model's extent
    /// (further each time), renamed to free names, and select the copies.
    pub fn paste(&mut self) -> usize {
        if self.clipboard.is_empty() {
            return 0;
        }
        self.paste_count += 1;
        let span = self
            .bounds
            .map(|(x0, y0, x1, y1)| ((x1 - x0).max(y1 - y0)).max(1.0))
            .unwrap_or(100.0);
        let step = (span * 0.05).max(1.0) * self.paste_count as f64;
        let (pasted, cmd) = build::paste(&self.doc, &self.clipboard, step, -step);
        let n = self.clipboard.len();
        let label = format!("paste {n} object(s)");
        if self.apply(cmd, &label) {
            self.selection = pasted;
            self.edit.active_vertex = None;
            n
        } else {
            0
        }
    }

    pub fn reverse_link(&mut self, name: &str) -> bool {
        match build::reverse_link(&self.doc, name) {
            Some(cmd) => self.apply(cmd, &format!("reverse {name}")),
            None => false,
        }
    }

    pub fn convert_node(&mut self, name: &str, to: NodeType) -> bool {
        match build::convert_node(&self.doc, name, to) {
            Some(cmd) => self.apply(cmd, &format!("convert {name} to {}", to.label())),
            None => false,
        }
    }
}

/// A human name for an object reference.
pub fn describe(r: &ObjRef) -> String {
    match r {
        ObjRef::Node(n) => format!("node {n}"),
        ObjRef::Link(n) => format!("link {n}"),
        ObjRef::Subcatchment(n) => format!("subcatchment {n}"),
        ObjRef::Gage(n) => format!("rain gage {n}"),
        ObjRef::Label(_) => "label".into(),
    }
}
