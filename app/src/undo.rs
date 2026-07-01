// SPDX-License-Identifier: GPL-3.0-or-later

//! Undo/redo history via project snapshots (Hydraflow Edit → Undo parity).

use stormsewer::io::Project;

const MAX_HISTORY: usize = 50;

/// Snapshot-based undo/redo stack for the active project document.
#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    undo: Vec<Project>,
    redo: Vec<Project>,
}

impl UndoStack {
    /// Push a checkpoint of the current project before a mutating edit.
    pub fn checkpoint(&mut self, project: &Project) {
        self.record_previous(project.clone());
    }

    /// Record a pre-edit snapshot (for immediate-mode UI where widgets mutate before `.changed()`).
    pub fn record_previous(&mut self, previous: Project) {
        self.undo.push(previous);
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Restore the previous project state, returning it if available.
    pub fn undo(&mut self, current: &Project) -> Option<Project> {
        let prev = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(prev)
    }

    /// Re-apply a undone edit, returning the project if available.
    pub fn redo(&mut self, current: &Project) -> Option<Project> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Clear history after loading a new file or starting a blank project.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}