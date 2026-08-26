# ---- state.rs: profile_pipes field + toggle --------------------------------
s = open("app/src/state.rs", encoding="utf-8").read()

n = s.count("            noaa_paste_open: false,")
assert n == 2, f"constructor sites: {n}"
s = s.replace(
    "            noaa_paste_open: false,",
    "            noaa_paste_open: false,\n            profile_pipes: Vec::new(),",
)

old = "    pub noaa_paste_open: bool,\n    pub noaa_paste_text: String,\n}"
new = """    pub noaa_paste_open: bool,
    pub noaa_paste_text: String,
    /// Pipes chosen for the profile view (Shift-click in Plan), by id, in
    /// click order. Empty = profile the automatic main trunk. Session-only:
    /// cleared on project load, never persisted.
    pub profile_pipes: Vec<String>,
}"""
assert old in s
s = s.replace(old, new, 1)

# clear on project load
old = """    pub fn load_project(&mut self, project: Project, path: Option<std::path::PathBuf>) {"""
probe = s.index(old)
insert_after = s.index("{", probe) + 1
s = s[:insert_after] + "\n        self.profile_pipes.clear();" + s[insert_after:]

# toggle helper next to set_selection
old = """    pub fn clear_selection(&mut self) {"""
new = """    /// Shift-click in the plan view: add or remove a pipe from the
    /// profile run. Selection order is click order; the profile view chains
    /// connected pipes upstream-first regardless.
    pub fn toggle_profile_pipe(&mut self, id: &str) {
        if let Some(i) = self.profile_pipes.iter().position(|p| p == id) {
            self.profile_pipes.remove(i);
        } else {
            self.profile_pipes.push(id.to_owned());
        }
        self.status = match self.profile_pipes.len() {
            0 => "Profile run cleared — Profile shows the main trunk".into(),
            1 => format!(
                "Profile run: {} — Shift-click more pipes, then open Profile",
                self.profile_pipes[0]
            ),
            len => format!(
                "Profile run: {} pipes ({}) — open Profile to view",
                len,
                self.profile_pipes.join(", ")
            ),
        };
    }

    pub fn clear_selection(&mut self) {"""
assert old in s
s = s.replace(old, new, 1)
open("app/src/state.rs", "w", encoding="utf-8", newline="\n").write(s)

# ---- main.rs: shift-click, Esc clear, pass run to draws --------------------
m = open("app/src/main.rs", encoding="utf-8").read()

old = "use edit::{delete_selection, handle_click, move_node, snap_node, Tool};"
new = "use edit::{delete_selection, handle_click, move_node, snap_node, snap_pipe, Tool};"
assert old in m
m = m.replace(old, new, 1)

# Esc chain: clear profile run after drawing cancels, before tc_calc
old = """                } else if !self.state.edit.catchment_vertices.is_empty() {
                    self.state.edit.catchment_vertices.clear();
                    self.state.status = "Catchment drawing cancelled".into();
                } else if self.state.tc_calc.open {"""
new = """                } else if !self.state.edit.catchment_vertices.is_empty() {
                    self.state.edit.catchment_vertices.clear();
                    self.state.status = "Catchment drawing cancelled".into();
                } else if !self.state.profile_pipes.is_empty() {
                    self.state.profile_pipes.clear();
                    self.state.status =
                        "Profile run cleared — Profile shows the main trunk".into();
                } else if self.state.tc_calc.open {"""
assert old in m
m = m.replace(old, new, 1)

# Shift-click: intercept before the normal click handling
old = """            if resp.clicked()
                && self.state.view_tab == ViewTab::Plan
                && self.state.dragging_node.is_none()
            {
                let pos = resp.interact_pointer_pos().unwrap_or(egui::Pos2::ZERO);
                let (wx, wy) = self.state.viewport.screen_to_world(rect, pos);
                if self.state.edit.tool == Tool::DrawCatchment {"""
new = """            if resp.clicked()
                && self.state.view_tab == ViewTab::Plan
                && self.state.dragging_node.is_none()
            {
                let pos = resp.interact_pointer_pos().unwrap_or(egui::Pos2::ZERO);
                let (wx, wy) = self.state.viewport.screen_to_world(rect, pos);
                let shift = ui.input(|i| i.modifiers.shift);
                if shift && self.state.edit.tool == Tool::Select {
                    // Shift-click builds the profile run; it never changes
                    // the ordinary selection and is not an undo-able edit.
                    if let Some(pidx) = snap_pipe(&self.state.project, wx, wy, SNAP_RADIUS) {
                        let id = self.state.project.pipes[pidx].id.clone();
                        self.state.toggle_profile_pipe(&id);
                    }
                } else if self.state.edit.tool == Tool::DrawCatchment {"""
assert old in m
m = m.replace(old, new, 1)

# pass the run into draw_plan and draw_profile
old = """                    Some(self.state.tool.label()),
                    pipe_preview_to,"""
assert old in m
m = m.replace(
    old,
    old + "\n                    &self.state.profile_pipes,",
    1,
)
old = """                ViewTab::Profile => draw_profile(
                    ui,
                    rect,
                    &self.state.project,
                    self.state.analysis.as_ref(),
                ),"""
new = """                ViewTab::Profile => draw_profile(
                    ui,
                    rect,
                    &self.state.project,
                    self.state.analysis.as_ref(),
                    &self.state.profile_pipes,
                ),"""
assert old in m
m = m.replace(old, new, 1)
open("app/src/main.rs", "w", encoding="utf-8", newline="\n").write(m)

# ---- plan.rs: highlight + header note --------------------------------------
p = open("app/src/plan.rs", encoding="utf-8").read()

old = """    tool_label: Option<&str>,
    pipe_preview_to: Option<(f64, f64)>,
    snap_target: Option<usize>,
) {"""
new = """    tool_label: Option<&str>,
    pipe_preview_to: Option<(f64, f64)>,
    snap_target: Option<usize>,
    profile_run: &[String],
) {"""
assert old in p
p = p.replace(old, new, 1)

# find the pipe drawing loop and add the accent underlay
probe = p.index("for pp in d.plan_pipes.iter()")
brace = p.index("{", probe) + 1
underlay = """
            if profile_run.iter().any(|id| id == &pp.id) {
                // Pink underlay: this pipe is part of the chosen profile run
                // (survey-flag pink = temporary marks, cleared with Esc).
                let a = to_screen(pp.x1, pp.y1);
                let b = to_screen(pp.x2, pp.y2);
                painter.line_segment(
                    [a, b],
                    Stroke::new(
                        7.0,
                        Color32::from_rgba_unmultiplied(224, 86, 127, 110),
                    ),
                );
            }
"""
p = p[:brace] + underlay + p[brace:]
open("app/src/plan.rs", "w", encoding="utf-8", newline="\n").write(p)

# ---- profile.rs: draw the selected run -------------------------------------
pr = open("app/src/profile.rs", encoding="utf-8").read()

old = """pub fn draw_profile(
    ui: &mut egui::Ui,
    rect: Rect,
    project: &Project,
    analysis: Option<&Analysis>,
) {"""
new = """pub fn draw_profile(
    ui: &mut egui::Ui,
    rect: Rect,
    project: &Project,
    analysis: Option<&Analysis>,
    profile_run: &[String],
) {"""
assert old in pr
pr = pr.replace(old, new, 1)

old = """    let net = project.to_network();
    crate::theme::draw_sheet_frame(&painter, rect, project);
    let drawing = draw_network(&net, analysis, &DrawConfig::default());"""
new = """    let net = project.to_network();
    crate::theme::draw_sheet_frame(&painter, rect, project);
    let selected_run = !profile_run.is_empty();
    let drawing = if selected_run {
        stormsewer::drawing::draw_profile_run(
            &net,
            analysis,
            &DrawConfig::default(),
            profile_run,
        )
    } else {
        draw_network(&net, analysis, &DrawConfig::default())
    };
    let header = if selected_run {
        format!(
            "Profile · selected run ({}) — Esc clears",
            profile_run.join(" → ")
        )
    } else {
        "Profile · main trunk — Shift-click pipes in Plan to profile a branch"
            .to_owned()
    };
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        header,
        egui::FontId::proportional(13.0),
        palette::MUTED,
    );"""
assert old in pr
pr = pr.replace(old, new, 1)

old = '''            "No profile data",'''
new = '''            "No profile data — the selected pipes were not found",'''
assert old in pr
pr = pr.replace(old, new, 1)
open("app/src/profile.rs", "w", encoding="utf-8", newline="\n").write(pr)

# ---- edit.rs: Select tool hint teaches the gesture -------------------------
e = open("app/src/edit.rs", encoding="utf-8").read()
import re
mo = re.search(r'Tool::Select => "([^"]*)"', e)
assert mo, "select hint"
old_hint = mo.group(0)
new_hint = 'Tool::Select => "Click to select; drag nodes to move; Shift-click pipes to build a profile run"'
e = e.replace(old_hint, new_hint, 1)
open("app/src/edit.rs", "w", encoding="utf-8", newline="\n").write(e)

print("app patched")
