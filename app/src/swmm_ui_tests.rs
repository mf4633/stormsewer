// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless egui tests for the SWMM map editor: pointer gestures on the
//! canvas, the undo steps they leave, menus, shortcuts, and files. Same
//! harness idea as `ui_tests`: whole frames through `StormSewerApp::ui`,
//! with the clock advanced by hand so egui's click and double-click timing
//! is deterministic.

use std::path::PathBuf;

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use stormsewer_swmm::doc::build::{NodeType, ObjRef};
use stormsewer_swmm::doc::Severity;

use crate::state::AppState;
use crate::swmm_doc::PendingAction;
use crate::swmm_menus;
use crate::swmm_tools::SwmmTool;
use crate::StormSewerApp;

fn raw_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1400.0, 900.0),
        )),
        ..Default::default()
    }
}

fn fixture(name: &str) -> (PathBuf, String) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../swmm/tests/fixtures/epa-samples")
        .join(name);
    let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    (path, text)
}

struct Harness {
    app: StormSewerApp,
    ctx: egui::Context,
    time: f64,
    /// Modifier keys held during the next frames (egui reads them from the
    /// raw input, not from the pointer event).
    modifiers: Modifiers,
}

impl Harness {
    /// The SWMM workspace on a blank model, laid out.
    fn new() -> Self {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        let mut h = Self {
            app,
            ctx: egui::Context::default(),
            time: 0.0,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![], 0.05);
        h.frame(vec![], 0.05);
        assert!(h.app.canvas_rect.width() > 200.0, "canvas laid out");
        h
    }

    fn frame(&mut self, events: Vec<Event>, dt: f64) {
        self.time += dt;
        let mut input = raw_input();
        input.time = Some(self.time);
        input.modifiers = self.modifiers;
        input.events = events;
        let _ = self.ctx.run(input, |c| self.app.ui(c));
    }

    fn ed(&self) -> &crate::swmm_doc::SwmmEditor {
        &self.app.state.swmm_doc
    }

    fn doc(&self) -> &stormsewer_swmm::doc::InpDoc {
        &self.app.state.swmm_doc.doc
    }

    fn screen(&self, x: f64, y: f64) -> Pos2 {
        self.app
            .state
            .swmm
            .map_viewport
            .world_to_screen(self.app.canvas_rect, x, y)
    }

    fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        }
    }

    /// A click: a second since the last one so it is never a double-click.
    fn click_at(&mut self, pos: Pos2, modifiers: Modifiers) {
        self.modifiers = modifiers;
        self.frame(vec![Event::PointerMoved(pos)], 1.0);
        self.frame(vec![Self::button(pos, true, modifiers)], 0.05);
        self.frame(vec![Self::button(pos, false, modifiers)], 0.05);
        self.frame(vec![], 0.05);
        self.modifiers = Modifiers::NONE;
    }

    fn click(&mut self, x: f64, y: f64) {
        let p = self.screen(x, y);
        self.click_at(p, Modifiers::NONE);
    }

    fn double_click(&mut self, x: f64, y: f64) {
        let p = self.screen(x, y);
        self.frame(vec![Event::PointerMoved(p)], 1.0);
        for _ in 0..2 {
            self.frame(vec![Self::button(p, true, Modifiers::NONE)], 0.05);
            self.frame(vec![Self::button(p, false, Modifiers::NONE)], 0.05);
        }
        self.frame(vec![], 0.05);
    }

    /// Press at `from`, move in `steps` pointer events, release at `to`.
    fn drag(&mut self, from: Pos2, to: Pos2, steps: usize, modifiers: Modifiers) {
        self.frame(vec![Event::PointerMoved(from)], 1.0);
        self.frame(vec![Self::button(from, true, modifiers)], 0.05);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let p = from + (to - from) * t;
            self.frame(vec![Event::PointerMoved(p)], 0.05);
        }
        self.frame(vec![Self::button(to, false, modifiers)], 0.05);
        self.frame(vec![], 0.05);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        let ev = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![ev(true)], 0.5);
        self.frame(vec![ev(false)], 0.05);
    }

    fn tool(&mut self, tool: SwmmTool) {
        self.app.state.swmm_doc.set_tool(tool);
    }

    fn add_junction(&mut self, x: f64, y: f64) -> String {
        self.tool(SwmmTool::AddNode(NodeType::Junction));
        let before: Vec<String> = self.ed().nodes.iter().map(|n| n.name.clone()).collect();
        self.click(x, y);
        assert_eq!(
            self.ed().nodes.len(),
            before.len() + 1,
            "junction placed at {x},{y}"
        );
        self.ed()
            .nodes
            .iter()
            .find(|n| !before.contains(&n.name))
            .unwrap()
            .name
            .clone()
    }
}

// --- workspace and chrome ---------------------------------------------------

#[test]
fn workspace_switch_keeps_storm_sewer_reachable() {
    let mut h = Harness::new();
    assert!(h.app.state.swmm_doc.active);
    assert!(h.app.state.swmm_doc.loaded, "entering opens a blank model");
    assert!(h.app.state.window_title().contains("SWMM model"));
    swmm_menus::leave_workspace(&mut h.app.state);
    h.frame(vec![], 0.05);
    assert!(!h.app.state.swmm_doc.active);
    assert!(!h.app.state.window_title().contains("SWMM model"));
    // The model survives the switch.
    assert!(h.app.state.swmm_doc.loaded);
}

#[test]
fn swmm_menus_render_open() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    let mut state = std::mem::replace(&mut h.app.state, AppState::new_empty());
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
    let _ = ctx.run(raw_input(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            swmm_menus::file_menu(ui, ctx, &mut state);
            swmm_menus::edit_menu(ui, &mut state);
            swmm_menus::view_menu(ui, &mut state, rect);
            swmm_menus::project_menu(ui, &mut state);
            swmm_menus::run_menu(ui, &mut state);
            swmm_menus::results_menu(ui, &mut state);
            swmm_menus::tools_menu(ui, &mut state);
            swmm_menus::draw_toolbar(ui, &mut state);
            swmm_menus::draw_left_panel(ui, &mut state);
            swmm_menus::draw_properties_panel(ui, &mut state);
            crate::swmm_canvas::draw_findings_strip(ui, &mut state);
            crate::swmm_canvas::draw_status_bar(ui, &state);
        });
    });
}

#[test]
fn every_swmm_tool_renders_a_frame() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    for tool in SwmmTool::all() {
        h.tool(tool);
        h.frame(vec![], 0.05);
        assert_eq!(h.ed().edit.tool, tool);
    }
    // Dialogs, too.
    h.app.state.swmm_doc.pending = Some(PendingAction::New);
    h.app.state.swmm_doc.run_refused = Some(vec![]);
    h.app.state.swmm_doc.edit.label_prompt = Some(((0.0, 0.0), "x".into()));
    h.app.state.swmm_doc.show_findings = true;
    h.frame(vec![], 0.05);
    for view in [
        crate::swmm_panel::SwmmSubView::Chart,
        crate::swmm_panel::SwmmSubView::Map,
    ] {
        h.app.state.swmm.sub_view = view;
        h.frame(vec![], 0.05);
    }
}

#[test]
fn tool_letter_shortcuts_switch_tools_and_escape_returns_to_select() {
    let mut h = Harness::new();
    h.key(Key::J, Modifiers::NONE);
    assert_eq!(h.ed().edit.tool, SwmmTool::AddNode(NodeType::Junction));
    h.key(Key::C, Modifiers::NONE);
    assert!(matches!(h.ed().edit.tool, SwmmTool::AddLink(_)));
    h.key(Key::A, Modifiers::NONE);
    assert_eq!(h.ed().edit.tool, SwmmTool::AddSubcatchment);
    h.key(Key::H, Modifiers::NONE);
    assert_eq!(h.ed().edit.tool, SwmmTool::Pan);
    h.key(Key::Escape, Modifiers::NONE);
    assert_eq!(h.ed().edit.tool, SwmmTool::Select);
}

// --- gestures -----------------------------------------------------------------

#[test]
fn add_junction_click_adds_row_and_coordinates_one_undo() {
    let mut h = Harness::new();
    let before = h.doc().to_string();
    let name = h.add_junction(120.0, 80.0);
    assert_eq!(name, "J1");
    assert!(h.doc().contains("JUNCTIONS", "J1"));
    let (x, y) = h.doc().coordinates("J1").expect("a COORDINATES row");
    assert!((x - 120.0).abs() < 0.5 && (y - 80.0).abs() < 0.5, "{x},{y}");
    assert_eq!(h.ed().undo_depth(), 1);
    assert_eq!(h.ed().undo_label(), Some("add junction J1"));
    assert!(h.ed().dirty());
    assert_eq!(h.ed().selection, vec![ObjRef::Node("J1".into())]);

    h.key(Key::Z, Modifiers::CTRL);
    assert!(!h.doc().contains("JUNCTIONS", "J1"));
    assert_eq!(h.doc().coordinates("J1"), None);
    assert_eq!(h.doc().to_string(), before);
    assert!(
        h.ed().selection.is_empty(),
        "undo drops a selection it removed"
    );
    assert!(!h.ed().dirty());

    h.key(Key::Y, Modifiers::CTRL);
    assert!(h.doc().contains("JUNCTIONS", "J1"));
    assert_eq!(h.ed().redo_label(), None);
    assert_eq!(h.ed().undo_label(), Some("add junction J1"));
}

#[test]
fn draw_conduit_with_vertex_adds_three_rows_one_undo() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.tool(SwmmTool::AddNode(NodeType::Outfall));
    h.click(400.0, 100.0);
    assert!(h.doc().contains("OUTFALLS", "O1"));
    let before = h.doc().to_string();

    h.tool(SwmmTool::AddLink(
        stormsewer_swmm::doc::build::LinkType::Conduit,
    ));
    h.click(100.0, 100.0);
    assert_eq!(h.ed().edit.link_from.as_deref(), Some("J1"));
    h.click(250.0, 200.0);
    assert_eq!(h.ed().edit.link_vertices.len(), 1);
    h.click(400.0, 100.0);
    assert!(h.ed().edit.link_from.is_none());

    let doc = h.doc();
    assert!(doc.contains("CONDUITS", "C1"));
    assert_eq!(doc.field("CONDUITS", "C1", "FromNode"), Some("J1"));
    assert_eq!(doc.field("CONDUITS", "C1", "ToNode"), Some("O1"));
    let length: f64 = doc
        .field("CONDUITS", "C1", "Length")
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (length - 2.0 * (150f64.powi(2) + 100f64.powi(2)).sqrt()).abs() < 1.0,
        "{length}"
    );
    assert_eq!(doc.field("XSECTIONS", "C1", "Shape"), Some("CIRCULAR"));
    assert_eq!(doc.field("XSECTIONS", "C1", "Geom1"), Some("1"));
    let v = doc.vertices("C1");
    assert_eq!(v.len(), 1);
    assert!(
        (v[0].0 - 250.0).abs() < 0.5 && (v[0].1 - 200.0).abs() < 0.5,
        "{v:?}"
    );
    assert_eq!(h.ed().undo_depth(), 3);
    assert_eq!(h.ed().links.len(), 1);
    assert_eq!(h.ed().links[0].path.len(), 3);
    assert!(h
        .ed()
        .findings
        .iter()
        .all(|f| f.severity != Severity::Error));

    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(
        h.doc().to_string(),
        before,
        "one undo removes all three rows"
    );
}

#[test]
fn node_drag_of_many_pointer_events_is_one_undo_step() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.add_junction(300.0, 100.0);
    h.tool(SwmmTool::AddLink(
        stormsewer_swmm::doc::build::LinkType::Conduit,
    ));
    h.click(100.0, 100.0);
    h.click(300.0, 100.0);
    assert_eq!(h.ed().undo_depth(), 3);
    let before = h.doc().to_string();

    h.tool(SwmmTool::Select);
    let from = h.screen(100.0, 100.0);
    let to = h.screen(160.0, 40.0);
    h.drag(from, to, 12, Modifiers::NONE);

    let (x, y) = h.doc().coordinates("J1").unwrap();
    assert!(
        (x - 160.0).abs() < 1.0 && (y - 40.0).abs() < 1.0,
        "moved to {x},{y}"
    );
    assert_eq!(h.ed().undo_depth(), 4, "twelve pointer events, one step");
    assert_eq!(h.ed().undo_label(), Some("move node J1"));
    // The attached link's drawn path follows the node.
    let path = &h.ed().links[0].path;
    assert!((path[0].0 - 160.0).abs() < 1.0);

    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.doc().to_string(), before);
    assert_eq!(h.ed().undo_depth(), 3);
}

#[test]
fn rubber_band_select_and_delete_cascades() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.add_junction(200.0, 100.0);
    h.add_junction(500.0, 100.0);
    h.tool(SwmmTool::AddLink(
        stormsewer_swmm::doc::build::LinkType::Conduit,
    ));
    h.click(100.0, 100.0);
    h.click(200.0, 100.0);
    h.click(200.0, 100.0);
    h.click(500.0, 100.0);
    assert_eq!(h.ed().links.len(), 2);
    let steps_before = h.ed().undo_depth();

    h.tool(SwmmTool::Select);
    h.app.state.swmm_doc.clear_selection();
    // A window around J1 and J2 (and C1 between them); J3 and C2 stay out.
    let a = h.screen(60.0, 140.0);
    let b = h.screen(240.0, 60.0);
    h.drag(a, b, 6, Modifiers::NONE);
    let sel = &h.ed().selection;
    assert!(sel.contains(&ObjRef::Node("J1".into())), "{sel:?}");
    assert!(sel.contains(&ObjRef::Node("J2".into())), "{sel:?}");
    assert!(sel.contains(&ObjRef::Link("C1".into())), "{sel:?}");
    assert!(!sel.contains(&ObjRef::Node("J3".into())), "{sel:?}");
    assert!(!sel.contains(&ObjRef::Link("C2".into())), "{sel:?}");

    // Deleting J2 takes C2 (unselected) with it: that asks first.
    h.key(Key::Delete, Modifiers::NONE);
    let (_, impact) = h
        .ed()
        .delete_confirm
        .clone()
        .expect("a confirmation is pending");
    assert_eq!(impact.links, vec!["C2"]);
    assert!(h.doc().contains("JUNCTIONS", "J2"), "nothing deleted yet");
    let n = h.app.state.swmm_doc.confirm_delete();
    assert_eq!(n, 3);
    h.frame(vec![], 0.05);
    let doc = h.doc();
    assert!(!doc.contains("JUNCTIONS", "J1"));
    assert!(!doc.contains("JUNCTIONS", "J2"));
    assert!(doc.contains("JUNCTIONS", "J3"));
    assert!(!doc.contains("CONDUITS", "C1"));
    assert!(
        !doc.contains("CONDUITS", "C2"),
        "cascade took the attached link"
    );
    assert!(!doc.contains("XSECTIONS", "C2"));
    assert_eq!(doc.coordinates("J1"), None);
    assert_eq!(
        h.ed().undo_depth(),
        steps_before + 1,
        "the cascade is one step"
    );
    assert!(h.ed().selection.is_empty());
}

#[test]
fn subcatchment_polygon_of_four_clicks() {
    let mut h = Harness::new();
    h.add_junction(300.0, 300.0);
    h.tool(SwmmTool::AddSubcatchment);
    h.click(100.0, 100.0);
    h.click(500.0, 100.0);
    h.click(500.0, 500.0);
    h.click(100.0, 500.0);
    assert_eq!(h.ed().edit.polygon.len(), 4);
    assert!(!h.doc().contains("SUBCATCHMENTS", "S1"), "still drawing");
    h.key(Key::Enter, Modifiers::NONE);
    assert!(h.ed().edit.polygon.is_empty());
    let doc = h.doc();
    assert!(doc.contains("SUBCATCHMENTS", "S1"));
    assert_eq!(doc.polygon("S1").len(), 4);
    assert_eq!(
        doc.field("SUBCATCHMENTS", "S1", "Outlet"),
        Some("J1"),
        "nearest node"
    );
    assert!(doc.contains("SUBAREAS", "S1"));
    let (_, infil) = doc.find("INFILTRATION", "S1").unwrap();
    assert_eq!(infil.fields.len(), 6, "Horton row: {:?}", infil.fields);
    assert_eq!(h.ed().undo_label(), Some("add subcatchment S1"));

    // Double-click closes too.
    h.click(600.0, 100.0);
    h.click(700.0, 100.0);
    h.double_click(700.0, 200.0);
    assert!(h.doc().contains("SUBCATCHMENTS", "S2"));
    assert_eq!(
        h.doc().polygon("S2").len(),
        3,
        "{:?}",
        h.doc().polygon("S2")
    );

    // Clicking the first corner closes as well; two corners cannot close.
    h.click(600.0, 300.0);
    h.click(700.0, 300.0);
    h.click(600.0, 300.0);
    assert!(!h.doc().contains("SUBCATCHMENTS", "S3"));
    assert_eq!(h.ed().edit.polygon.len(), 3);
    h.click(700.0, 400.0);
    h.click(600.0, 300.0);
    assert!(h.doc().contains("SUBCATCHMENTS", "S3"));
    assert_eq!(h.doc().polygon("S3").len(), 4);
    // The subcatchment can be picked by clicking inside it.
    h.tool(SwmmTool::Select);
    h.click(300.0, 300.0);
    assert_eq!(
        h.ed().selection,
        vec![ObjRef::Node("J1".into())],
        "the node wins over the polygon"
    );
    h.click(200.0, 200.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Subcatchment("S1".into())]);
}

#[test]
fn paste_renames_copies() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.add_junction(300.0, 100.0);
    h.tool(SwmmTool::AddLink(
        stormsewer_swmm::doc::build::LinkType::Conduit,
    ));
    h.click(100.0, 100.0);
    h.click(300.0, 100.0);
    h.key(Key::A, Modifiers::CTRL);
    assert_eq!(h.ed().selection.len(), 3);
    h.key(Key::C, Modifiers::CTRL);
    assert_eq!(h.ed().clipboard.len(), 3);
    let depth = h.ed().undo_depth();
    h.key(Key::V, Modifiers::CTRL);
    let doc = h.doc();
    assert!(doc.contains("JUNCTIONS", "J3") && doc.contains("JUNCTIONS", "J4"));
    assert!(doc.contains("CONDUITS", "C2"));
    assert_eq!(doc.field("CONDUITS", "C2", "FromNode"), Some("J3"));
    assert_eq!(doc.field("CONDUITS", "C2", "ToNode"), Some("J4"));
    assert!(doc.contains("XSECTIONS", "C2"));
    let (x3, _) = doc.coordinates("J3").unwrap();
    assert!(x3 > 100.0, "pasted copy is offset: {x3}");
    assert_eq!(h.ed().undo_depth(), depth + 1);
    assert_eq!(h.ed().selection.len(), 3, "the copies are selected");
    assert!(h.ed().selection.contains(&ObjRef::Link("C2".into())));
    assert!(h
        .ed()
        .findings
        .iter()
        .all(|f| f.severity != Severity::Error));
    h.key(Key::V, Modifiers::CTRL);
    assert!(h.doc().contains("JUNCTIONS", "J6") && h.doc().contains("CONDUITS", "C3"));

    // Cut removes and remembers.
    h.key(Key::X, Modifiers::CTRL);
    assert!(!h.doc().contains("CONDUITS", "C3"));
    assert_eq!(h.ed().clipboard.len(), 3);
}

#[test]
fn context_actions_reverse_and_convert_are_single_undo_steps() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.add_junction(300.0, 100.0);
    h.tool(SwmmTool::AddLink(
        stormsewer_swmm::doc::build::LinkType::Conduit,
    ));
    h.click(100.0, 100.0);
    h.click(200.0, 150.0);
    h.click(300.0, 100.0);
    let depth = h.ed().undo_depth();
    assert!(h.app.state.swmm_doc.reverse_link("C1"));
    assert_eq!(h.doc().field("CONDUITS", "C1", "FromNode"), Some("J2"));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    assert!(h.app.state.swmm_doc.convert_node("J2", NodeType::Storage));
    assert!(h.doc().contains("STORAGE", "J2") && !h.doc().contains("JUNCTIONS", "J2"));
    assert_eq!(h.ed().undo_depth(), depth + 2);
    h.frame(vec![], 0.05);
    assert_eq!(h.ed().node("J2").unwrap().kind, NodeType::Storage);
    assert_eq!(
        h.ed().links[0].path.len(),
        3,
        "the converted node still anchors its link"
    );
    h.key(Key::Z, Modifiers::CTRL);
    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.ed().undo_depth(), depth);
    assert_eq!(h.doc().field("CONDUITS", "C1", "FromNode"), Some("J1"));
}

#[test]
fn label_tool_prompts_then_adds_row() {
    let mut h = Harness::new();
    h.tool(SwmmTool::AddLabel);
    h.click(50.0, 60.0);
    assert!(h.ed().edit.label_prompt.is_some());
    assert!(h.doc().section("LABELS").is_none());
    h.app.state.swmm_doc.edit.label_prompt.as_mut().unwrap().1 = "Pond A".into();
    crate::swmm_canvas::finish_label(&mut h.app.state.swmm_doc);
    h.frame(vec![], 0.05);
    assert_eq!(h.ed().labels.len(), 1);
    assert_eq!(h.ed().labels[0].text, "Pond A");
    assert_eq!(h.ed().undo_label(), Some("add label \"Pond A\""));
    // Labels select and delete like anything else.
    h.tool(SwmmTool::Select);
    h.click(50.0, 60.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Label(h.ed().labels[0].line)]);
    h.key(Key::Delete, Modifiers::NONE);
    assert!(h.ed().labels.is_empty());
}

#[test]
fn vertex_double_click_adds_and_delete_removes() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.add_junction(500.0, 100.0);
    h.tool(SwmmTool::AddLink(
        stormsewer_swmm::doc::build::LinkType::Conduit,
    ));
    h.click(100.0, 100.0);
    h.click(500.0, 100.0);
    h.tool(SwmmTool::Select);
    h.click(300.0, 100.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Link("C1".into())]);
    let depth = h.ed().undo_depth();
    h.double_click(300.0, 100.0);
    assert_eq!(
        h.doc().vertices("C1").len(),
        1,
        "double-click on the segment adds a vertex"
    );
    assert_eq!(h.ed().undo_depth(), depth + 1);
    assert!(h.ed().edit.active_vertex.is_some());
    // Drag the vertex.
    let from = h.screen(300.0, 100.0);
    let to = h.screen(300.0, 200.0);
    h.drag(from, to, 5, Modifiers::NONE);
    let v = h.doc().vertices("C1");
    assert!((v[0].1 - 200.0).abs() < 1.0, "{v:?}");
    assert_eq!(h.ed().undo_depth(), depth + 2);
    // Click the handle, Delete removes only the vertex.
    h.click(300.0, 200.0);
    assert!(h.ed().edit.active_vertex.is_some());
    h.key(Key::Delete, Modifiers::NONE);
    assert!(h.doc().vertices("C1").is_empty());
    assert!(h.doc().contains("CONDUITS", "C1"), "the link itself stays");
}

// --- files ------------------------------------------------------------------------

#[test]
fn save_writes_exactly_the_rendered_text_and_clears_dirty() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    assert!(h.ed().dirty());
    let dir = std::env::temp_dir().join("stormsewer-swmm-ui-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("saved-model.inp");
    h.app.state.swmm_doc.save_as(path.clone()).unwrap();
    assert!(!h.ed().dirty());
    assert_eq!(h.ed().path.as_deref(), Some(path.as_path()));
    let on_disk = std::fs::read(&path).unwrap();
    assert_eq!(on_disk, h.doc().to_string().into_bytes());
    // Save with a path needs no dialog and stays clean.
    h.add_junction(200.0, 100.0);
    assert!(h.ed().dirty());
    assert_eq!(h.app.state.swmm_doc.save(), Ok(true));
    assert!(!h.ed().dirty());
    assert!(String::from_utf8(std::fs::read(&path).unwrap())
        .unwrap()
        .contains("J2"));
    // Undo past the save point is dirty again; redo back to it is clean.
    h.key(Key::Z, Modifiers::CTRL);
    assert!(h.ed().dirty());
    h.key(Key::Y, Modifiers::CTRL);
    assert!(!h.ed().dirty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fixture_edit_undo_all_restores_original_bytes() {
    let mut h = Harness::new();
    let (path, original) = fixture("Site_Drainage_Model.inp");
    h.app.state.swmm_doc.open_path(&path).unwrap();
    h.app.state.swmm.pending_map_fit = true;
    h.frame(vec![], 0.05);
    h.frame(vec![], 0.05);
    assert_eq!(h.doc().to_string(), original);
    assert!(!h.ed().nodes.is_empty() && !h.ed().links.is_empty() && !h.ed().subs.is_empty());
    assert!(
        h.app.state.swmm.model_inp.is_some(),
        "the runner's inventory is parsed from the document"
    );

    // Edit through the canvas: place, link, move, delete.
    let (min_x, min_y, max_x, max_y) = h.ed().bounds.unwrap();
    let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    let name = h.add_junction(cx, cy);
    let first = h.ed().nodes[0].clone();
    h.tool(SwmmTool::AddLink(
        stormsewer_swmm::doc::build::LinkType::Conduit,
    ));
    h.click(first.x, first.y);
    h.click(cx, cy);
    assert_eq!(
        h.ed().links.len(),
        h.app.state.swmm.model_inp.as_ref().unwrap().links.len()
    );
    h.tool(SwmmTool::Select);
    let from = h.screen(cx, cy);
    let to = h.screen(cx + (max_x - min_x) * 0.05, cy);
    h.drag(from, to, 8, Modifiers::NONE);
    h.click(cx + (max_x - min_x) * 0.05, cy);
    assert_eq!(h.ed().selection, vec![ObjRef::Node(name.clone())]);
    h.key(Key::Delete, Modifiers::NONE);
    if h.ed().delete_confirm.is_some() {
        h.app.state.swmm_doc.confirm_delete();
    }
    assert!(!h.doc().contains("JUNCTIONS", &name));
    assert_eq!(h.ed().undo_depth(), 4);
    assert!(h.ed().dirty());

    while h.app.state.swmm_doc.can_undo() {
        h.key(Key::Z, Modifiers::CTRL);
    }
    assert_eq!(
        h.doc().to_string(),
        original,
        "undo all restores the file byte for byte"
    );
    assert!(!h.ed().dirty());
}

#[test]
fn unsaved_prompt_defers_new_model_until_discard() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.app.state.swmm_doc.request(PendingAction::New);
    assert_eq!(h.ed().pending, Some(PendingAction::New));
    assert!(
        h.doc().contains("JUNCTIONS", "J1"),
        "deferred behind the prompt"
    );
    h.frame(vec![], 0.05);
    h.app.state.swmm_doc.pending = None;
    assert!(h.doc().contains("JUNCTIONS", "J1"), "cancel keeps editing");
    h.app.state.swmm_doc.request(PendingAction::New);
    h.app.state.swmm_doc.perform(PendingAction::New);
    assert!(!h.doc().contains("JUNCTIONS", "J1"));
    assert!(!h.ed().dirty());
    // A clean model asks nothing.
    h.app.state.swmm_doc.request(PendingAction::New);
    assert!(h.ed().pending.is_none());
}

#[test]
fn dirty_swmm_model_intercepts_close() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    let mut input = raw_input();
    input.time = Some(h.time + 1.0);
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    let out = h.ctx.run(input, |c| h.app.ui(c));
    let cancelled = out
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|v| {
            v.commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::CancelClose))
        })
        .unwrap_or(false);
    assert!(cancelled, "a dirty SWMM model must not close silently");
    assert!(h.app.show_close_confirm);
}

// --- undo routing, validation, run -------------------------------------------

#[test]
fn undo_shortcut_routes_by_workspace() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    // Storm-sewer side: a checkpointed project edit.
    let before = h.app.state.project.nodes[0].invert;
    h.app.state.checkpoint_undo();
    h.app.state.project.nodes[0].invert = before - 2.0;

    // In the SWMM workspace Ctrl+Z undoes the document, not the project.
    h.key(Key::Z, Modifiers::CTRL);
    assert!(!h.doc().contains("JUNCTIONS", "J1"));
    assert_eq!(h.app.state.project.nodes[0].invert, before - 2.0);
    h.key(Key::Z, Modifiers::CTRL | Modifiers::SHIFT);
    assert!(h.doc().contains("JUNCTIONS", "J1"), "Ctrl+Shift+Z redoes");

    // In the design workspace Ctrl+Z undoes the project, not the document.
    swmm_menus::leave_workspace(&mut h.app.state);
    h.frame(vec![], 0.05);
    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.app.state.project.nodes[0].invert, before);
    assert!(h.doc().contains("JUNCTIONS", "J1"));
}

#[test]
fn run_is_refused_with_error_findings() {
    let mut h = Harness::new();
    h.app.state.swmm_doc.open_text(
        "[JUNCTIONS]\nJ1 0\n[CONDUITS]\nC1 J1 NOWHERE 100 0.01 0 0\n[XSECTIONS]\nC1 CIRCULAR 1 0 0 0\n[COORDINATES]\nJ1 0 0\n",
        None,
    );
    h.frame(vec![], 0.05);
    assert_eq!(h.ed().error_count(), 1);
    swmm_menus::run_model(&mut h.app.state);
    let refused = h.ed().run_refused.clone().expect("run refused");
    assert_eq!(refused.len(), 1);
    assert!(refused[0].message.contains("NOWHERE"));
    assert!(
        h.app.state.swmm.model.is_none(),
        "nothing was handed to the engine"
    );
    assert!(h.app.state.status.starts_with("Run refused"));
    h.frame(vec![], 0.05);

    // The finding points at the link; selecting it zooms the map there.
    let target = h.app.state.swmm_doc.finding_target(&refused[0]).unwrap();
    assert_eq!(target, ObjRef::Link("C1".into()));

    // Fix it and the run path is a scratch copy of the current text.
    h.app.state.swmm_doc.run_refused = None;
    h.add_junction(50.0, 50.0);
    h.app.state.swmm_doc.apply(
        stormsewer_swmm::doc::Command::SetField {
            section: "CONDUITS".into(),
            name: "C1".into(),
            field: "ToNode".into(),
            value: "J2".into(),
        },
        "fix",
    );
    let path = h.app.state.swmm_doc.run_path().expect("no errors now");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), h.doc().to_string());
    let _ = std::fs::remove_file(path);
}

#[test]
fn findings_strip_target_selects_and_zooms() {
    let mut h = Harness::new();
    h.app.state.swmm_doc.open_text(
        "[JUNCTIONS]\nJ1 0\nJ2 0\n[COORDINATES]\nJ1 0 0\nJ2 5000 5000\n",
        None,
    );
    h.frame(vec![], 0.05);
    let warnings: Vec<_> = h
        .ed()
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .cloned()
        .collect();
    assert!(warnings.is_empty(), "{warnings:?}");
    h.app
        .state
        .swmm_doc
        .open_text("[JUNCTIONS]\nJ1 0\nJ2 0\n[COORDINATES]\nJ1 0 0\n", None);
    h.frame(vec![], 0.05);
    let f = h
        .ed()
        .findings
        .iter()
        .find(|f| f.name == "J2")
        .cloned()
        .unwrap();
    assert_eq!(f.severity, Severity::Warning);
    let t = h.app.state.swmm_doc.finding_target(&f).unwrap();
    assert_eq!(t, ObjRef::Node("J2".into()));
    // A finding about a node with no coordinates cannot be zoomed to, but
    // one about J1 can: the viewport moves.
    h.app.state.swmm_doc.select_only(ObjRef::Node("J1".into()));
    h.app.state.swmm_doc.pending_zoom_to = Some(ObjRef::Node("J1".into()));
    let pan_before = h.app.state.swmm.map_viewport.pan;
    h.frame(vec![], 0.05);
    assert!(h.ed().pending_zoom_to.is_none());
    assert_ne!(h.app.state.swmm.map_viewport.pan, pan_before);
}

#[test]
fn shift_and_ctrl_click_build_the_selection() {
    let mut h = Harness::new();
    h.add_junction(100.0, 100.0);
    h.add_junction(300.0, 100.0);
    h.add_junction(500.0, 100.0);
    h.tool(SwmmTool::Select);
    h.click(100.0, 100.0);
    let p = h.screen(300.0, 100.0);
    h.click_at(p, Modifiers::SHIFT);
    assert_eq!(h.ed().selection.len(), 2);
    let p = h.screen(500.0, 100.0);
    h.click_at(p, Modifiers::CTRL);
    assert_eq!(h.ed().selection.len(), 3);
    h.click_at(p, Modifiers::CTRL);
    assert_eq!(h.ed().selection.len(), 2, "ctrl-click toggles off");
    h.click(700.0, 300.0);
    assert!(h.ed().selection.is_empty(), "plain click on nothing clears");
    // Moving a multi-selection moves every node.
    h.click(100.0, 100.0);
    let p = h.screen(300.0, 100.0);
    h.click_at(p, Modifiers::SHIFT);
    let from = h.screen(100.0, 100.0);
    let to = h.screen(100.0, 200.0);
    h.drag(from, to, 5, Modifiers::NONE);
    assert!((h.doc().coordinates("J1").unwrap().1 - 200.0).abs() < 1.0);
    assert!((h.doc().coordinates("J2").unwrap().1 - 200.0).abs() < 1.0);
    assert!((h.doc().coordinates("J3").unwrap().1 - 100.0).abs() < 1.0);
}
