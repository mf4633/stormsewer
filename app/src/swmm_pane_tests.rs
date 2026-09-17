// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless egui tests for the SWMM editor's panes and dialogs: the
//! editable property sheet, the attribute grids, the layers pane, the
//! project browser and the project dialogs. Same clock-driven harness as
//! `swmm_ui_tests`: whole frames through `StormSewerApp::ui`.

use std::path::PathBuf;

use eframe::egui::{self, Color32, Event, Key, Modifiers, PointerButton, Pos2};
use stormsewer_swmm::doc::build::{LinkType, NodeType, ObjRef};

use crate::state::AppState;
use crate::swmm_browser::{self, BrowserKind};
use crate::swmm_dialogs;
use crate::swmm_grids::{self, Replace};
use crate::swmm_menus;
use crate::swmm_props::{self, field_id};
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

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../swmm/tests/fixtures")
        .join(rel)
}

struct Harness {
    app: StormSewerApp,
    ctx: egui::Context,
    time: f64,
    modifiers: Modifiers,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Harness {
    fn with_state(state: AppState) -> Self {
        let mut app = StormSewerApp::new_for_test(state);
        swmm_menus::enter_workspace(&mut app.state);
        let mut h = Self {
            app,
            ctx: egui::Context::default(),
            time: 0.0,
            modifiers: Modifiers::NONE,
            shapes: Vec::new(),
        };
        h.frame(vec![], 0.05);
        h.frame(vec![], 0.05);
        assert!(h.app.canvas_rect.width() > 200.0, "canvas laid out");
        h
    }

    fn new() -> Self {
        Self::with_state(AppState::new_empty())
    }

    fn frame(&mut self, events: Vec<Event>, dt: f64) {
        self.time += dt;
        let mut input = raw_input();
        input.time = Some(self.time);
        input.modifiers = self.modifiers;
        input.events = events;
        let out = self.ctx.run(input, |c| self.app.ui(c));
        self.shapes = out.shapes;
    }

    fn ed(&self) -> &crate::swmm_doc::SwmmEditor {
        &self.app.state.swmm_doc
    }

    fn ed_mut(&mut self) -> &mut crate::swmm_doc::SwmmEditor {
        &mut self.app.state.swmm_doc
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

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn click(&mut self, x: f64, y: f64) {
        // A frame first, so a panel that just changed width has settled
        // and the canvas rect the position is computed from is current.
        self.frame(vec![], 1.0);
        let p = self.screen(x, y);
        self.frame(vec![Event::PointerMoved(p)], 0.05);
        self.frame(vec![Self::button(p, true)], 0.05);
        self.frame(vec![Self::button(p, false)], 0.05);
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
        self.modifiers = modifiers;
        self.frame(vec![ev(true)], 0.5);
        self.frame(vec![ev(false)], 0.05);
        self.modifiers = Modifiers::NONE;
    }

    /// Focus a text field, replace its text, and press Enter.
    fn type_into(&mut self, id: egui::Id, text: &str) {
        self.ctx.memory_mut(|m| m.request_focus(id));
        self.frame(vec![], 0.05);
        assert!(
            self.ctx.memory(|m| m.has_focus(id)),
            "the field {id:?} took focus"
        );
        // Real Windows input sets both `ctrl` and `command`.
        self.key(Key::A, Modifiers::CTRL | Modifiers::COMMAND);
        self.frame(vec![Event::Text(text.to_string())], 0.05);
        self.key(Key::Enter, Modifiers::NONE);
        self.frame(vec![], 0.05);
    }

    fn add_node(&mut self, kind: NodeType, x: f64, y: f64) -> String {
        self.app.state.swmm_doc.set_tool(SwmmTool::AddNode(kind));
        let before: Vec<String> = self.ed().nodes.iter().map(|n| n.name.clone()).collect();
        self.click(x, y);
        assert_eq!(self.ed().nodes.len(), before.len() + 1, "node placed");
        self.ed()
            .nodes
            .iter()
            .find(|n| !before.contains(&n.name))
            .unwrap()
            .name
            .clone()
    }

    fn add_conduit(&mut self, from: (f64, f64), to: (f64, f64)) {
        self.app
            .state
            .swmm_doc
            .set_tool(SwmmTool::AddLink(LinkType::Conduit));
        self.click(from.0, from.1);
        self.click(to.0, to.1);
        self.app.state.swmm_doc.set_tool(SwmmTool::Select);
    }

    /// Whether any painted circle has one of `colors` as its fill.
    fn painted_circle_with(&self, colors: &[Color32]) -> bool {
        fn scan(shape: &egui::Shape, colors: &[Color32]) -> bool {
            match shape {
                egui::Shape::Circle(c) => colors.contains(&c.fill),
                egui::Shape::Vec(v) => v.iter().any(|s| scan(s, colors)),
                _ => false,
            }
        }
        self.shapes.iter().any(|c| scan(&c.shape, colors))
    }
}

/// Lines that differ between two texts.
fn changed_lines(a: &str, b: &str) -> Vec<(String, String)> {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();
    assert_eq!(la.len(), lb.len(), "same number of lines");
    la.iter()
        .zip(lb.iter())
        .filter(|(x, y)| x != y)
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect()
}

// --- property sheet -------------------------------------------------------------

#[test]
fn sheet_edit_of_junction_elevation_is_one_undo_step_changing_one_line() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Node("J1".into())]);
    let before = h.doc().to_string();
    let depth = h.ed().undo_depth();

    h.type_into(field_id("JUNCTIONS", "Elevation"), "12.5");

    assert_eq!(h.doc().field("JUNCTIONS", "J1", "Elevation"), Some("12.5"));
    assert_eq!(h.ed().undo_depth(), depth + 1, "one step");
    assert_eq!(h.ed().undo_label(), Some("set Elevation of J1"));
    let diff = changed_lines(&before, &h.doc().to_string());
    assert_eq!(diff.len(), 1, "only the junction row changed: {diff:?}");
    assert!(diff[0].0.starts_with("J1") && diff[0].1.contains("12.5"));
    assert_eq!(h.doc().field("JUNCTIONS", "J1", "MaxDepth"), Some("0"));

    // Escape reverts the field without a step.
    let id = field_id("JUNCTIONS", "MaxDepth");
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.frame(vec![], 0.05);
    h.key(Key::A, Modifiers::CTRL | Modifiers::COMMAND);
    h.frame(vec![Event::Text("99".into())], 0.05);
    assert!(h.ed().sheet.draft.as_ref().is_some_and(|(_, t)| t == "99"));
    h.key(Key::Escape, Modifiers::NONE);
    h.frame(vec![], 0.05);
    assert_eq!(h.doc().field("JUNCTIONS", "J1", "MaxDepth"), Some("0"));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    assert!(h.ed().sheet.draft.is_none());

    // A non-number is refused with an inline message, not written.
    assert!(!swmm_props::commit(
        h.ed_mut(),
        "JUNCTIONS",
        "J1",
        "Elevation",
        "abc"
    ));
    assert_eq!(h.doc().field("JUNCTIONS", "J1", "Elevation"), Some("12.5"));

    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.doc().to_string(), before);
}

#[test]
fn sheet_type_switch_outfall_free_to_fixed_rewrites_the_row() {
    let mut h = Harness::new();
    h.add_node(NodeType::Outfall, 100.0, 100.0);
    let (_, row) = h.doc().find("OUTFALLS", "O1").unwrap();
    assert_eq!(row.fields, vec!["O1", "0", "FREE", "NO"]);
    let depth = h.ed().undo_depth();
    assert!(swmm_props::commit(
        h.ed_mut(),
        "OUTFALLS",
        "O1",
        "Type",
        "FIXED"
    ));
    let (_, row) = h.doc().find("OUTFALLS", "O1").unwrap();
    assert_eq!(row.fields, vec!["O1", "0", "FIXED", "0", "NO"]);
    assert_eq!(h.doc().field("OUTFALLS", "O1", "Stage"), Some("0"));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    // The sheet now shows a Stage field and edits it in place.
    h.frame(vec![], 0.05);
    h.type_into(field_id("OUTFALLS", "Stage"), "3.25");
    assert_eq!(h.doc().field("OUTFALLS", "O1", "Stage"), Some("3.25"));
    assert!(swmm_props::commit(
        h.ed_mut(),
        "OUTFALLS",
        "O1",
        "Type",
        "FREE"
    ));
    let (_, row) = h.doc().find("OUTFALLS", "O1").unwrap();
    assert_eq!(row.fields, vec!["O1", "0", "FREE", "NO"]);
    // Storage: tabular to functional takes the functional defaults.
    h.add_node(NodeType::Storage, 300.0, 100.0);
    assert!(swmm_props::commit(
        h.ed_mut(),
        "STORAGE",
        "ST1",
        "Shape",
        "TABULAR"
    ));
    assert_eq!(h.doc().field("STORAGE", "ST1", "Curve"), Some("*"));
    assert!(swmm_props::commit(
        h.ed_mut(),
        "STORAGE",
        "ST1",
        "Shape",
        "FUNCTIONAL"
    ));
    assert_eq!(h.doc().field("STORAGE", "ST1", "Coeff"), Some("1000"));
    h.frame(vec![], 0.05);
}

#[test]
fn sheet_rename_through_the_name_field_updates_link_ends() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    h.add_node(NodeType::Junction, 300.0, 100.0);
    h.add_conduit((100.0, 100.0), (300.0, 100.0));
    h.click(100.0, 100.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Node("J1".into())]);
    let depth = h.ed().undo_depth();
    h.type_into(field_id("JUNCTIONS", "Name"), "MH-1");
    assert!(h.doc().contains("JUNCTIONS", "MH-1"));
    assert!(!h.doc().contains("JUNCTIONS", "J1"));
    assert_eq!(h.doc().field("CONDUITS", "C1", "FromNode"), Some("MH-1"));
    assert!(h.doc().coordinates("MH-1").is_some());
    assert_eq!(h.ed().undo_depth(), depth + 1);
    assert_eq!(h.ed().undo_label(), Some("rename J1 to MH-1"));
    assert_eq!(
        h.ed().selection,
        vec![ObjRef::Node("MH-1".into())],
        "the selection follows the rename"
    );
    // The link still draws between both ends.
    h.frame(vec![], 0.05);
    assert_eq!(h.ed().links[0].path.len(), 2);
    // A clash is refused.
    assert!(!swmm_props::commit(
        h.ed_mut(),
        "JUNCTIONS",
        "MH-1",
        "Name",
        "J2"
    ));
    assert!(h.ed().last_error.as_deref().unwrap_or("").contains("J2"));
}

#[test]
fn sheet_multi_select_edit_is_one_batch() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    h.add_node(NodeType::Junction, 200.0, 100.0);
    h.add_node(NodeType::Outfall, 300.0, 100.0);
    h.key(Key::A, Modifiers::CTRL);
    assert_eq!(h.ed().selection.len(), 3);
    h.frame(vec![], 0.05);
    let depth = h.ed().undo_depth();
    // The common field is edited through the multi sheet's widget.
    h.type_into(egui::Id::new(("swmm-multi-field", "Elevation")), "5");
    for (sec, name) in [("JUNCTIONS", "J1"), ("JUNCTIONS", "J2"), ("OUTFALLS", "O1")] {
        assert_eq!(h.doc().field(sec, name, "Elevation"), Some("5"), "{name}");
    }
    assert_eq!(h.ed().undo_depth(), depth + 1, "one batch");
    assert_eq!(h.ed().undo_label(), Some("set Elevation of 3 objects"));
    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.doc().field("JUNCTIONS", "J2", "Elevation"), Some("0"));
    assert_eq!(h.doc().field("OUTFALLS", "O1", "Elevation"), Some("0"));
    // MaxDepth is not an outfall column, so it is not a common field.
    let targets = vec![
        ("JUNCTIONS".to_string(), "J1".to_string()),
        ("OUTFALLS".to_string(), "O1".to_string()),
    ];
    assert!(!swmm_props::commit_many(
        h.ed_mut(),
        &targets,
        "MaxDepth",
        "2"
    ));
}

#[test]
fn sheet_sub_sheets_add_inflow_and_edit_cross_section() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    h.add_node(NodeType::Junction, 300.0, 100.0);
    h.add_conduit((100.0, 100.0), (300.0, 100.0));
    // Cross-section through the link's sub-sheet by line.
    let (li, _) = h.doc().find("XSECTIONS", "C1").unwrap();
    assert!(swmm_props::commit_line(
        h.ed_mut(),
        "XSECTIONS",
        li,
        "C1",
        "Geom1",
        "2.5"
    ));
    assert_eq!(h.doc().field("XSECTIONS", "C1", "Geom1"), Some("2.5"));
    assert!(swmm_props::commit_line(
        h.ed_mut(),
        "XSECTIONS",
        li,
        "C1",
        "Shape",
        "IRREGULAR"
    ));
    let (_, row) = h.doc().find("XSECTIONS", "C1").unwrap();
    assert_eq!(row.fields, vec!["C1", "IRREGULAR", "*"]);
    // A tag through the sheet.
    assert!(swmm_props::commit_tag(
        h.ed_mut(),
        stormsewer_swmm::doc::ObjectKind::Link,
        "C1",
        "Culvert"
    ));
    assert_eq!(h.doc().tag("Link", "C1"), Some("Culvert"));
    assert!(swmm_props::commit_tag(
        h.ed_mut(),
        stormsewer_swmm::doc::ObjectKind::Link,
        "C1",
        ""
    ));
    assert_eq!(h.doc().tag("Link", "C1"), None);
    // The sheets render for a node, a link and a subcatchment.
    h.click(100.0, 100.0);
    h.frame(vec![], 0.05);
    h.click(200.0, 100.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Link("C1".into())]);
    h.frame(vec![], 0.05);
    h.app.state.swmm_doc.set_tool(SwmmTool::AddSubcatchment);
    h.click(400.0, 400.0);
    h.click(500.0, 400.0);
    h.click(500.0, 500.0);
    assert_eq!(h.ed().edit.polygon.len(), 3);
    h.key(Key::Enter, Modifiers::NONE);
    assert!(h.doc().contains("SUBCATCHMENTS", "S1"));
    h.frame(vec![], 0.05);
    // Infiltration method switch on the subcatchment's row.
    let (li, _) = h.doc().find("INFILTRATION", "S1").unwrap();
    assert!(swmm_props::commit_line(
        h.ed_mut(),
        "INFILTRATION",
        li,
        "S1",
        "Method",
        "GREEN_AMPT"
    ));
    let (_, row) = h.doc().find("INFILTRATION", "S1").unwrap();
    assert_eq!(row.fields.last().map(String::as_str), Some("GREEN_AMPT"));
    assert_eq!(row.fields.len(), 5);
    h.frame(vec![], 0.05);
}

// --- attribute grids -------------------------------------------------------------

#[test]
fn grid_sorts_filters_edits_and_imports_csv() {
    let mut h = Harness::new();
    h.app
        .state
        .swmm_doc
        .open_path(&fixture("epa-samples/Detention_Pond_Model.inp"))
        .unwrap();
    h.app.state.swmm.pending_map_fit = true;
    h.frame(vec![], 0.05);
    swmm_grids::open(h.ed_mut(), "JUNCTIONS");
    h.frame(vec![], 0.05);
    assert!(h.ed().grid.open);

    let (headers, rows) = swmm_grids::build_grid(h.doc(), "JUNCTIONS");
    assert_eq!(headers[0], "Name");
    assert_eq!(headers[1], "Elevation");
    assert!(rows.len() >= 10, "{}", rows.len());
    // Sort by elevation, both ways.
    h.ed_mut().grid.sort_col = Some(1);
    let asc = swmm_grids::visible_rows(&headers, &rows, &h.ed().grid);
    let elev = |i: usize| rows[i].cell("Elevation").unwrap().parse::<f64>().unwrap();
    assert!(asc.windows(2).all(|w| elev(w[0]) <= elev(w[1])));
    h.ed_mut().grid.sort_desc = true;
    let desc = swmm_grids::visible_rows(&headers, &rows, &h.ed().grid);
    assert!(desc.windows(2).all(|w| elev(w[0]) >= elev(w[1])));
    // Filter.
    h.ed_mut().grid.sort_col = None;
    h.ed_mut().grid.filter = "J1".into();
    let filtered = swmm_grids::visible_rows(&headers, &rows, &h.ed().grid);
    assert!(!filtered.is_empty() && filtered.len() < rows.len());
    assert!(filtered.iter().all(|&i| rows[i].name.contains("J1")));
    h.frame(vec![], 0.05);

    // In-place edit through the grid's cell widget.
    let j1 = rows.iter().find(|r| r.name == "J1").unwrap();
    let depth = h.ed().undo_depth();
    h.type_into(
        egui::Id::new(("swmm-grid-cell", "JUNCTIONS", j1.line, "MaxDepth")),
        "7.5",
    );
    assert_eq!(h.doc().field("JUNCTIONS", "J1", "MaxDepth"), Some("7.5"));
    assert_eq!(h.ed().undo_depth(), depth + 1);

    // Row click selects on the map; the map selection shows in the grid.
    h.ed_mut().select_only(ObjRef::Node("J2".into()));
    h.frame(vec![], 0.05);
    let tsv = swmm_grids::tsv(&headers, &rows, &[0, 1], &Default::default());
    assert!(tsv.starts_with("Name\tElevation\t"));
    assert_eq!(tsv.lines().count(), 3);

    // Import CSV: matched by name, one batch.
    let e1 = h
        .doc()
        .field("JUNCTIONS", "J1", "Elevation")
        .unwrap()
        .to_string();
    let depth = h.ed().undo_depth();
    let csv = "Name,Elevation,Nonsense\nJ1,4999,x\nJ2,4998,y\nNOPE,1,z\n";
    let n = swmm_grids::import_csv(h.ed_mut(), "JUNCTIONS", csv).unwrap();
    assert_eq!(n, 2);
    assert_eq!(h.doc().field("JUNCTIONS", "J1", "Elevation"), Some("4999"));
    assert_eq!(h.doc().field("JUNCTIONS", "J2", "Elevation"), Some("4998"));
    assert_eq!(h.ed().undo_depth(), depth + 1, "one undo step");
    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.doc().field("JUNCTIONS", "J1", "Elevation").unwrap(), e1);
    h.frame(vec![], 0.05);
}

#[test]
fn grid_replace_in_column_sets_and_scales() {
    let mut h = Harness::new();
    for x in [100.0, 200.0, 300.0] {
        h.add_node(NodeType::Junction, x, 100.0);
    }
    for n in ["J1", "J2", "J3"] {
        assert!(swmm_props::commit(
            h.ed_mut(),
            "JUNCTIONS",
            n,
            "MaxDepth",
            "4"
        ));
    }
    let (headers, rows) = swmm_grids::build_grid(h.doc(), "JUNCTIONS");
    let mut st = swmm_grids::GridState {
        filter: "J".into(),
        ..Default::default()
    };
    let which = swmm_grids::visible_rows(&headers, &rows, &st);
    assert_eq!(which.len(), 3);
    let depth = h.ed().undo_depth();
    let n = swmm_grids::replace_in_column(
        h.ed_mut(),
        "JUNCTIONS",
        &rows,
        &which,
        "MaxDepth",
        &Replace::Scale(2.5),
    )
    .unwrap();
    assert_eq!(n, 3);
    for n in ["J1", "J2", "J3"] {
        assert_eq!(h.doc().field("JUNCTIONS", n, "MaxDepth"), Some("10"));
    }
    assert_eq!(h.ed().undo_depth(), depth + 1);
    // Set a value on a subset only.
    let (headers, rows) = swmm_grids::build_grid(h.doc(), "JUNCTIONS");
    st.filter = "J3".into();
    let which = swmm_grids::visible_rows(&headers, &rows, &st);
    let n = swmm_grids::replace_in_column(
        h.ed_mut(),
        "JUNCTIONS",
        &rows,
        &which,
        "InitDepth",
        &Replace::Value("1.5".into()),
    )
    .unwrap();
    assert_eq!(n, 1);
    assert_eq!(h.doc().field("JUNCTIONS", "J3", "InitDepth"), Some("1.5"));
    assert_eq!(h.doc().field("JUNCTIONS", "J1", "InitDepth"), Some("0"));
    // A non-number into a numeric column is refused whole.
    assert!(swmm_grids::replace_in_column(
        h.ed_mut(),
        "JUNCTIONS",
        &rows,
        &which,
        "InitDepth",
        &Replace::Value("abc".into()),
    )
    .is_err());
    h.key(Key::Z, Modifiers::CTRL);
    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.doc().field("JUNCTIONS", "J2", "MaxDepth"), Some("4"));
}

// --- layers ------------------------------------------------------------------------

#[test]
fn layer_toggle_hides_a_kind_from_the_map_and_from_hit_testing() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    h.add_node(NodeType::Outfall, 300.0, 100.0);
    h.app.state.swmm_doc.set_tool(SwmmTool::Select);
    h.click(100.0, 100.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Node("J1".into())]);
    h.ed_mut().clear_selection();

    h.ed_mut().layers.junctions.visible = false;
    h.app.state.swmm_doc.left_tab = crate::swmm_doc::LeftTab::Layers;
    h.frame(vec![], 0.05);
    let junction_fill = crate::theme::palette::NODE_INLET;
    assert!(
        !h.painted_circle_with(&[junction_fill]),
        "no junction symbol painted while the layer is off"
    );
    h.click(100.0, 100.0);
    assert!(
        h.ed().selection.is_empty(),
        "a hidden junction cannot be picked"
    );
    h.click(300.0, 100.0);
    assert_eq!(
        h.ed().selection,
        vec![ObjRef::Node("O1".into())],
        "outfalls stay pickable"
    );

    h.ed_mut().layers.junctions.visible = true;
    h.frame(vec![], 0.05);
    assert!(h.painted_circle_with(&[junction_fill]));
    h.click(100.0, 100.0);
    assert_eq!(h.ed().selection, vec![ObjRef::Node("J1".into())]);
    // A colour override paints with that colour.
    h.ed_mut().layers.junctions.color = Some([10, 20, 30, 255]);
    h.ed_mut().clear_selection();
    h.frame(vec![], 0.05);
    assert!(h.painted_circle_with(&[Color32::from_rgba_unmultiplied(10, 20, 30, 255)]));
}

#[test]
fn results_overlay_draws_on_the_editing_map_when_a_run_exists() {
    let state = crate::swmm_profile::tests::pond_state();
    let mut h = Harness::with_state(state);
    h.app
        .state
        .swmm_doc
        .open_path(&fixture("epa-samples/Detention_Pond_Model.inp"))
        .unwrap();
    h.app.state.swmm.pending_map_fit = true;
    h.frame(vec![], 0.05);
    assert!(h.app.state.swmm.results.is_some());
    assert!(
        !h.painted_circle_with(&crate::swmm_results::RAMP),
        "no result colours before the layer is on"
    );

    h.ed_mut().layers.node_results = true;
    h.ed_mut().layers.link_results = true;
    h.frame(vec![], 0.05);
    assert!(
        h.painted_circle_with(&crate::swmm_results::RAMP),
        "nodes painted in the results ramp on the editing map"
    );
    assert!(h
        .app
        .state
        .swmm
        .results_overlay
        .node_values(false)
        .is_some());

    // Node results alone, from the editor's own geometry.
    h.ed_mut().layers.link_results = false;
    h.frame(vec![], 0.05);
    assert!(h.painted_circle_with(&crate::swmm_results::RAMP));
    // The time slider: a period colours from that instant.
    h.app.state.swmm.set_period(10);
    h.frame(vec![], 0.05);
    assert!(h.app.state.swmm.frame().is_some());
    assert!(h.app.state.swmm.results_overlay.node_values(true).is_some());
    // The layers pane hosts the results controls.
    h.app.state.swmm_doc.left_tab = crate::swmm_doc::LeftTab::Layers;
    h.frame(vec![], 0.05);
    // Off again: back to the map's own colours.
    h.ed_mut().layers.node_results = false;
    h.frame(vec![], 0.05);
    assert!(!h.painted_circle_with(&crate::swmm_results::RAMP));
}

// --- browser ----------------------------------------------------------------------

#[test]
fn browser_search_filters_and_plus_minus_add_and_delete_a_kind() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    h.add_node(NodeType::Junction, 300.0, 100.0);
    h.app.state.swmm_doc.browser.search = "J2".into();
    h.frame(vec![], 0.05);
    assert_eq!(
        swmm_browser::items(h.ed(), BrowserKind::Node(NodeType::Junction)),
        vec!["J1", "J2"]
    );
    h.app.state.swmm_doc.browser.search.clear();

    let depth = h.ed().undo_depth();
    let name = swmm_browser::add(&mut h.app.state, BrowserKind::Node(NodeType::Junction)).unwrap();
    assert_eq!(name, "J3");
    assert!(h.doc().contains("JUNCTIONS", "J3"));
    assert!(h.doc().coordinates("J3").is_some());
    assert_eq!(h.ed().selection, vec![ObjRef::Node("J3".into())]);
    assert_eq!(h.ed().undo_depth(), depth + 1);
    h.frame(vec![], 0.05);
    let n = swmm_browser::delete(&mut h.app.state, BrowserKind::Node(NodeType::Junction));
    assert_eq!(n, 1);
    assert!(!h.doc().contains("JUNCTIONS", "J3"));
    assert_eq!(h.ed().undo_depth(), depth + 2);

    // Non-map kinds: a curve is added, chosen, and deleted by name.
    let name = swmm_browser::add(&mut h.app.state, BrowserKind::Curves).unwrap();
    assert_eq!(name, "Curve1");
    assert!(h.doc().contains("CURVES", "Curve1"));
    assert_eq!(h.ed().browser.item.as_deref(), Some("Curve1"));
    h.frame(vec![], 0.05);
    assert_eq!(
        swmm_browser::delete(&mut h.app.state, BrowserKind::Curves),
        1
    );
    assert!(!h.doc().contains("CURVES", "Curve1"));
    // A subcatchment from the browser gets a polygon around the centre.
    let name = swmm_browser::add(&mut h.app.state, BrowserKind::Subcatchments).unwrap();
    assert_eq!(h.doc().polygon(&name).len(), 4);
    assert_eq!(h.doc().field("SUBCATCHMENTS", &name, "Outlet"), Some("J1"));
    // Links need the map: + arms the tool.
    assert!(swmm_browser::add(&mut h.app.state, BrowserKind::Link(LinkType::Conduit)).is_none());
    assert_eq!(h.ed().edit.tool, SwmmTool::AddLink(LinkType::Conduit));
    // Double-click on a category opens its editor.
    swmm_browser::open_editor(&mut h.app.state, BrowserKind::Curves, None);
    assert!(h.ed().dialogs.curves.is_some());
    swmm_browser::open_editor(&mut h.app.state, BrowserKind::Options, None);
    assert!(h.ed().dialogs.options.is_some());
    h.frame(vec![], 0.05);
}

// --- dialogs ----------------------------------------------------------------------

#[test]
fn options_dialog_ok_is_one_undo_step() {
    let mut h = Harness::new();
    swmm_dialogs::open_options(h.ed_mut());
    h.frame(vec![], 0.05);
    let mut draft = h.ed().dialogs.options.clone().unwrap();
    assert_eq!(draft.values["FLOW_UNITS"], "CFS");
    draft.values.insert("FLOW_UNITS".into(), "CMS".into());
    draft.values.insert("MIN_SLOPE".into(), "0.5".into());
    draft
        .values
        .insert("SURCHARGE_METHOD".into(), "SLOT".into());
    draft.files = "SAVE HOTSTART \"hot.hsf\"".into();
    let depth = h.ed().undo_depth();
    assert!(swmm_dialogs::apply_options(h.ed_mut(), &draft));
    assert_eq!(h.doc().option("FLOW_UNITS"), Some("CMS"));
    assert_eq!(h.doc().option("MIN_SLOPE"), Some("0.5"));
    assert_eq!(h.doc().option("SURCHARGE_METHOD"), Some("SLOT"));
    assert_eq!(h.doc().rows("FILES").len(), 1);
    assert_eq!(h.ed().undo_depth(), depth + 1, "one step for the dialog");
    assert_eq!(h.ed().undo_label(), Some("edit options"));
    h.key(Key::Z, Modifiers::CTRL);
    assert_eq!(h.doc().option("FLOW_UNITS"), Some("CFS"));
    assert_eq!(h.doc().option("SURCHARGE_METHOD"), None);
    // The sheet's unit labels follow FLOW_UNITS.
    h.key(Key::Y, Modifiers::CTRL);
    assert!(swmm_props::units(h.doc()).metric);
    // Every tab renders.
    for tab in 0..5 {
        h.ed_mut().dialogs.options.as_mut().unwrap().tab = tab;
        h.frame(vec![], 0.05);
    }
    // Title dialog: one step too.
    swmm_dialogs::open_title(h.ed_mut());
    h.frame(vec![], 0.05);
    let depth = h.ed().undo_depth();
    assert!(swmm_dialogs::apply_title(
        h.ed_mut(),
        "Pond study\nRevision B"
    ));
    assert_eq!(h.doc().title(), "Pond study\nRevision B");
    assert_eq!(h.ed().undo_depth(), depth + 1);
    // Controls dialog.
    swmm_dialogs::open_controls(h.ed_mut());
    let depth = h.ed().undo_depth();
    assert!(swmm_dialogs::apply_controls(
        h.ed_mut(),
        "RULE R1\nIF NODE J1 DEPTH > 2\nTHEN PUMP P1 STATUS = ON"
    ));
    assert_eq!(h.doc().rows("CONTROLS").len(), 3);
    assert_eq!(h.ed().undo_depth(), depth + 1);
    h.frame(vec![], 0.05);
}

#[test]
fn curves_editor_adds_a_row_and_renders_its_plot() {
    let mut h = Harness::new();
    swmm_dialogs::open_curves(h.ed_mut(), None);
    h.frame(vec![], 0.05);
    assert!(h.ed().dialogs.curves.as_ref().unwrap().name.is_none());
    swmm_browser::add(&mut h.app.state, BrowserKind::Curves);
    swmm_dialogs::open_curves(h.ed_mut(), Some("Curve1"));
    let mut d = h.ed().dialogs.curves.clone().unwrap();
    assert_eq!(d.kind, "STORAGE");
    assert_eq!(d.points.len(), 1);
    d.points.push(("2".into(), "400".into()));
    d.points.push(("4".into(), "900".into()));
    d.kind = "STORAGE".into();
    d.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(swmm_dialogs::apply_curve(h.ed_mut(), &mut d));
    let (kind, pts) = h.doc().curve("Curve1");
    assert_eq!(kind.as_deref(), Some("STORAGE"));
    assert_eq!(pts.len(), 3);
    assert_eq!(pts[2], ("4".to_string(), "900".to_string()));
    assert_eq!(
        h.ed().undo_depth(),
        depth + 1,
        "the whole curve is one step"
    );
    assert!(!d.dirty);
    h.ed_mut().dialogs.curves = Some(d);
    // Past the window's fade-in, the plot's points are the accent colour.
    h.frame(vec![], 0.5);
    h.frame(vec![], 0.5);
    assert!(h.painted_circle_with(&[crate::theme::palette::ACCENT]));
    // A storage unit can pick the curve (the dialog closed, off the map).
    h.ed_mut().dialogs.curves = None;
    h.add_node(NodeType::Storage, 100.0, 100.0);
    assert!(swmm_props::commit(
        h.ed_mut(),
        "STORAGE",
        "ST1",
        "Shape",
        "TABULAR"
    ));
    match swmm_props::spec(h.doc(), "STORAGE", "Curve").kind {
        swmm_props::FieldKind::Reference(names) => assert_eq!(names, vec!["Curve1"]),
        k => panic!("{k:?}"),
    }
    assert!(swmm_props::commit(
        h.ed_mut(),
        "STORAGE",
        "ST1",
        "Curve",
        "Curve1"
    ));
    assert_eq!(h.doc().field("STORAGE", "ST1", "Curve"), Some("Curve1"));
    h.frame(vec![], 0.05);
    // Patterns editor.
    swmm_browser::add(&mut h.app.state, BrowserKind::Patterns);
    swmm_dialogs::open_patterns(h.ed_mut(), Some("Pattern1"));
    let mut p = h.ed().dialogs.patterns.clone().unwrap();
    assert_eq!(p.kind, "HOURLY");
    p.kind = "DAILY".into();
    p.values = (1..=7).map(|i| format!("{}", i as f64 / 7.0)).collect();
    p.dirty = true;
    assert!(swmm_dialogs::apply_pattern(h.ed_mut(), &mut p));
    let (kind, values) = swmm_dialogs::pattern_of(h.doc(), "Pattern1");
    assert_eq!(kind, "DAILY");
    assert_eq!(values.len(), 7);
    h.ed_mut().dialogs.patterns = Some(p);
    h.frame(vec![], 0.05);
}

#[test]
fn time_series_editor_pastes_tsv_rows() {
    let mut h = Harness::new();
    swmm_browser::add(&mut h.app.state, BrowserKind::Timeseries);
    swmm_dialogs::open_series(h.ed_mut(), Some("TS1"));
    h.frame(vec![], 0.05);
    let mut d = h.ed().dialogs.series.clone().unwrap();
    assert_eq!(d.points.len(), 1);
    d.paste = "0:00\t0.5\n0:15\t1.25\n0:30\t0.75\n".into();
    let pts = swmm_dialogs::parse_series_text(&d.paste);
    assert_eq!(pts.len(), 3);
    d.points = pts;
    d.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(swmm_dialogs::apply_series(h.ed_mut(), &mut d));
    let back = h.doc().timeseries("TS1");
    assert_eq!(back.len(), 3);
    assert_eq!(back[1].time, "0:15");
    assert_eq!(back[1].value, "1.25");
    assert_eq!(h.ed().undo_depth(), depth + 1);
    h.ed_mut().dialogs.series = Some(d.clone());
    h.frame(vec![], 0.05);
    // Dated rows and the file option.
    d.paste = "01/01/2007 0:00 0.1\n01/01/2007 1:00 0.2".into();
    d.points = swmm_dialogs::parse_series_text(&d.paste);
    d.dirty = true;
    assert!(swmm_dialogs::apply_series(h.ed_mut(), &mut d));
    let back = h.doc().timeseries("TS1");
    assert_eq!(back[0].date.as_deref(), Some("01/01/2007"));
    d.file = Some("rain.dat".into());
    d.dirty = true;
    assert!(swmm_dialogs::apply_series(h.ed_mut(), &mut d));
    let rows = h.doc().find_all("TIMESERIES", "TS1");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value(1), Some("FILE"));
    assert_eq!(rows[0].value(2), Some("rain.dat"));
    h.ed_mut().dialogs.series = Some(d);
    h.frame(vec![], 0.05);
    // A rain gage can pick the series.
    swmm_dialogs::open_gages(h.ed_mut(), None);
    h.frame(vec![], 0.05);
    let gage = swmm_browser::add(&mut h.app.state, BrowserKind::Gages).unwrap();
    assert_eq!(h.doc().field("RAINGAGES", &gage, "Series"), Some("TS1"));
    h.ed_mut().dialogs.gage = Some(gage);
    h.frame(vec![], 0.05);
}

// --- canvas integrations ---------------------------------------------------------------

#[test]
fn profile_from_selection_and_pick_mode_set_the_path() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    h.add_node(NodeType::Outfall, 300.0, 100.0);
    h.add_conduit((100.0, 100.0), (300.0, 100.0));
    h.ed_mut()
        .select_many(vec![ObjRef::Node("J1".into()), ObjRef::Node("O1".into())]);
    assert!(swmm_menus::profile_from_selection(&mut h.app.state));
    assert_eq!(h.app.state.swmm.profile.start.as_deref(), Some("J1"));
    assert_eq!(h.app.state.swmm.profile.end.as_deref(), Some("O1"));
    assert_eq!(
        h.app.state.swmm.sub_view,
        crate::swmm_panel::SwmmSubView::Profile
    );
    h.frame(vec![], 0.05);
    // Pick mode from the map: start node, end node.
    h.app.state.swmm.sub_view = crate::swmm_panel::SwmmSubView::Map;
    h.ed_mut().profile_pick = Some(None);
    h.frame(vec![], 0.05);
    h.click(300.0, 100.0);
    assert_eq!(h.ed().profile_pick, Some(Some("O1".into())));
    h.click(100.0, 100.0);
    assert!(h.ed().profile_pick.is_none());
    assert_eq!(h.app.state.swmm.profile.start.as_deref(), Some("O1"));
    assert_eq!(h.app.state.swmm.profile.end.as_deref(), Some("J1"));
    assert_eq!(
        h.app.state.swmm.sub_view,
        crate::swmm_panel::SwmmSubView::Profile
    );
    // Escape cancels a pick.
    h.app.state.swmm.sub_view = crate::swmm_panel::SwmmSubView::Map;
    h.ed_mut().profile_pick = Some(None);
    h.key(Key::Escape, Modifiers::NONE);
    assert!(h.ed().profile_pick.is_none());
    // One node is not a path.
    h.ed_mut().select_only(ObjRef::Node("J1".into()));
    assert!(!swmm_menus::profile_from_selection(&mut h.app.state));
}

#[test]
fn every_pane_and_dialog_renders_a_frame() {
    let mut h = Harness::new();
    h.add_node(NodeType::Junction, 100.0, 100.0);
    h.add_node(NodeType::Junction, 300.0, 100.0);
    h.add_conduit((100.0, 100.0), (300.0, 100.0));
    let ed = h.ed_mut();
    swmm_dialogs::open_title(ed);
    swmm_dialogs::open_options(ed);
    swmm_dialogs::open_gages(ed, None);
    swmm_dialogs::open_curves(ed, None);
    swmm_dialogs::open_series(ed, None);
    swmm_dialogs::open_patterns(ed, None);
    swmm_dialogs::open_controls(ed);
    swmm_dialogs::open_pollutants(ed);
    swmm_dialogs::open_landuses(ed);
    swmm_grids::open(ed, "CONDUITS");
    ed.grid.show_columns = true;
    ed.left_tab = crate::swmm_doc::LeftTab::Layers;
    h.frame(vec![], 0.05);
    h.app.state.swmm_doc.left_tab = crate::swmm_doc::LeftTab::Browser;
    h.app.state.swmm_doc.browser.kind = Some(BrowserKind::Curves);
    h.frame(vec![], 0.05);
    // The menus with the new items.
    let mut state = std::mem::replace(&mut h.app.state, AppState::new_empty());
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
    let _ = ctx.run(raw_input(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            swmm_menus::view_menu(ui, &mut state, rect);
            swmm_menus::project_menu(ui, &mut state);
            swmm_menus::draw_left_panel(ui, &mut state);
            swmm_menus::draw_properties_panel(ui, &mut state);
            crate::swmm_grids::draw_grid(ui, &mut state);
        });
    });
}
