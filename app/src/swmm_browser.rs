// SPDX-License-Identifier: GPL-3.0-or-later

//! The project browser: the model as a tree in the EPA GUI's order —
//! Title/Notes, Options, Climatology, Hydrology, Hydraulics, Quality,
//! Curves, Time Series, Patterns, Controls, Map — with counts per node, a
//! search box that filters the tree, click to select and zoom,
//! double-click to open the sheet or the editor, and + / − buttons that
//! add or delete objects of the chosen kind through `doc/build.rs`.

use eframe::egui::{self, RichText, Ui};
use stormsewer_swmm::doc::build::{self, LinkType, NodeType, ObjRef};
use stormsewer_swmm::doc::{Command, ObjectKind};

use crate::state::AppState;
use crate::swmm_dialogs;
use crate::swmm_doc::SwmmEditor;
use crate::swmm_grids;
use crate::swmm_tools::SwmmTool;
use crate::theme::palette;

/// A node of the tree that stands for a kind of thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserKind {
    Title,
    Options,
    Evaporation,
    Temperature,
    Adjustments,
    Gages,
    Subcatchments,
    Aquifers,
    Snowpacks,
    Hydrographs,
    LidControls,
    Node(NodeType),
    Link(LinkType),
    Pollutants,
    LandUses,
    Curves,
    Timeseries,
    Patterns,
    Controls,
    Labels,
    Backdrop,
}

impl BrowserKind {
    pub fn section(self) -> &'static str {
        match self {
            Self::Title => "TITLE",
            Self::Options => "OPTIONS",
            Self::Evaporation => "EVAPORATION",
            Self::Temperature => "TEMPERATURE",
            Self::Adjustments => "ADJUSTMENTS",
            Self::Gages => "RAINGAGES",
            Self::Subcatchments => "SUBCATCHMENTS",
            Self::Aquifers => "AQUIFERS",
            Self::Snowpacks => "SNOWPACKS",
            Self::Hydrographs => "HYDROGRAPHS",
            Self::LidControls => "LID_CONTROLS",
            Self::Node(k) => k.section(),
            Self::Link(k) => k.section(),
            Self::Pollutants => "POLLUTANTS",
            Self::LandUses => "LANDUSES",
            Self::Curves => "CURVES",
            Self::Timeseries => "TIMESERIES",
            Self::Patterns => "PATTERNS",
            Self::Controls => "CONTROLS",
            Self::Labels => "LABELS",
            Self::Backdrop => "BACKDROP",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title/Notes",
            Self::Options => "Options",
            Self::Evaporation => "Evaporation",
            Self::Temperature => "Temperature",
            Self::Adjustments => "Adjustments",
            Self::Gages => "Rain Gages",
            Self::Subcatchments => "Subcatchments",
            Self::Aquifers => "Aquifers",
            Self::Snowpacks => "Snow Packs",
            Self::Hydrographs => "Unit Hydrographs",
            Self::LidControls => "LID Controls",
            Self::Node(NodeType::Junction) => "Junctions",
            Self::Node(NodeType::Outfall) => "Outfalls",
            Self::Node(NodeType::Divider) => "Dividers",
            Self::Node(NodeType::Storage) => "Storage Units",
            Self::Link(LinkType::Conduit) => "Conduits",
            Self::Link(LinkType::Pump) => "Pumps",
            Self::Link(LinkType::Orifice) => "Orifices",
            Self::Link(LinkType::Weir) => "Weirs",
            Self::Link(LinkType::Outlet) => "Outlets",
            Self::Pollutants => "Pollutants",
            Self::LandUses => "Land Uses",
            Self::Curves => "Curves",
            Self::Timeseries => "Time Series",
            Self::Patterns => "Patterns",
            Self::Controls => "Controls",
            Self::Labels => "Labels",
            Self::Backdrop => "Backdrop",
        }
    }

    /// The kind an object of this node is, for the map selection.
    pub fn object_kind(self) -> Option<ObjectKind> {
        match self {
            Self::Gages => Some(ObjectKind::Gage),
            Self::Subcatchments => Some(ObjectKind::Subcatchment),
            Self::Node(_) => Some(ObjectKind::Node),
            Self::Link(_) => Some(ObjectKind::Link),
            Self::Curves => Some(ObjectKind::Curve),
            Self::Timeseries => Some(ObjectKind::Timeseries),
            Self::Patterns => Some(ObjectKind::Pattern),
            _ => None,
        }
    }

    /// Whether the node lists named objects.
    pub fn has_items(self) -> bool {
        !matches!(
            self,
            Self::Title
                | Self::Options
                | Self::Evaporation
                | Self::Temperature
                | Self::Adjustments
                | Self::Controls
                | Self::Backdrop
        )
    }

    /// Whether `+` can make one here.
    pub fn can_add(self) -> bool {
        matches!(
            self,
            Self::Gages
                | Self::Subcatchments
                | Self::Node(_)
                | Self::Link(_)
                | Self::Pollutants
                | Self::LandUses
                | Self::Curves
                | Self::Timeseries
                | Self::Patterns
                | Self::Labels
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct BrowserState {
    pub search: String,
    pub kind: Option<BrowserKind>,
    /// The named item chosen in a non-map list (curves, series, ...).
    pub item: Option<String>,
}

/// Names under a browser node.
pub fn items(ed: &SwmmEditor, kind: BrowserKind) -> Vec<String> {
    match kind {
        BrowserKind::Labels => ed.labels.iter().map(|l| l.text.clone()).collect(),
        k if k.has_items() => ed.doc.names(k.section()),
        _ => Vec::new(),
    }
}

/// The map object a browser item is, if it is one.
fn objref(kind: BrowserKind, ed: &SwmmEditor, name: &str, index: usize) -> Option<ObjRef> {
    match kind {
        BrowserKind::Labels => ed.labels.get(index).map(|l| ObjRef::Label(l.line)),
        _ => SwmmEditor::objref_for(kind.section(), name),
    }
}

/// Where a new object goes when it is made from the browser rather than
/// the map: the model's centre, or the origin of an empty model.
fn placement(ed: &SwmmEditor) -> ((f64, f64), f64) {
    match ed.bounds {
        Some((x0, y0, x1, y1)) => (
            ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
            ((x1 - x0).max(y1 - y0) * 0.05).max(10.0),
        ),
        None => ((0.0, 0.0), 100.0),
    }
}

/// Make one object of `kind` (a node, gage, subcatchment, curve, series,
/// pattern, pollutant, land use or label) as one undo step, and select it.
/// Links need two nodes, so `+` on a link kind arms the draw tool instead.
pub fn add(state: &mut AppState, kind: BrowserKind) -> Option<String> {
    let ed = &mut state.swmm_doc;
    let ((cx, cy), span) = placement(ed);
    let s = |v: &str| v.to_string();
    let (obj, label, name, select): (Command, String, String, Option<ObjRef>) = match kind {
        BrowserKind::Node(k) => {
            let o = build::new_node(&ed.doc, k, cx, cy);
            let n = o.name.clone();
            (
                o.command,
                format!("add {} {n}", k.label().to_lowercase()),
                n.clone(),
                Some(ObjRef::Node(n)),
            )
        }
        BrowserKind::Gages => {
            let o = build::new_gage(&ed.doc, cx - span, cy + span);
            let n = o.name.clone();
            (
                o.command,
                format!("add rain gage {n}"),
                n.clone(),
                Some(ObjRef::Gage(n)),
            )
        }
        BrowserKind::Subcatchments => {
            let poly = vec![
                (cx - span, cy - span),
                (cx + span, cy - span),
                (cx + span, cy + span),
                (cx - span, cy + span),
            ];
            let outlet = ed
                .nearest_node(cx, cy, f64::INFINITY)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| "*".into());
            let o = build::new_subcatchment(&ed.doc, &poly, &outlet);
            let n = o.name.clone();
            (
                o.command,
                format!("add subcatchment {n}"),
                n.clone(),
                Some(ObjRef::Subcatchment(n)),
            )
        }
        BrowserKind::Link(k) => {
            ed.set_tool(SwmmTool::AddLink(k));
            state.status = format!("{}: click the start node on the map", k.label());
            return None;
        }
        BrowserKind::Labels => {
            let text = build::unique_name(&ed.doc, ObjectKind::Node, "Label");
            (
                build::new_label(cx, cy + span, &text),
                format!("add label \"{text}\""),
                text.clone(),
                None,
            )
        }
        BrowserKind::Curves => {
            let n = build::unique_name(&ed.doc, ObjectKind::Curve, "Curve");
            (
                Command::AddRow {
                    section: s("CURVES"),
                    fields: vec![n.clone(), s("STORAGE"), s("0"), s("0")],
                    comment: None,
                },
                format!("add curve {n}"),
                n.clone(),
                None,
            )
        }
        BrowserKind::Timeseries => {
            let n = build::unique_name(&ed.doc, ObjectKind::Timeseries, "TS");
            (
                Command::AddRow {
                    section: s("TIMESERIES"),
                    fields: vec![n.clone(), s("0:00"), s("0")],
                    comment: None,
                },
                format!("add time series {n}"),
                n.clone(),
                None,
            )
        }
        BrowserKind::Patterns => {
            let n = build::unique_name(&ed.doc, ObjectKind::Pattern, "Pattern");
            let mut fields = vec![n.clone(), s("HOURLY")];
            fields.extend(std::iter::repeat_n(s("1.0"), 6));
            (
                Command::AddRow {
                    section: s("PATTERNS"),
                    fields,
                    comment: None,
                },
                format!("add pattern {n}"),
                n.clone(),
                None,
            )
        }
        BrowserKind::Pollutants => {
            let n = unique_in(ed, "POLLUTANTS", "Pollutant");
            (
                Command::AddRow {
                    section: s("POLLUTANTS"),
                    fields: vec![
                        n.clone(),
                        s("MG/L"),
                        s("0"),
                        s("0"),
                        s("0"),
                        s("0"),
                        s("NO"),
                        s("*"),
                        s("0"),
                        s("0"),
                        s("0"),
                    ],
                    comment: None,
                },
                format!("add pollutant {n}"),
                n.clone(),
                None,
            )
        }
        BrowserKind::LandUses => {
            let n = unique_in(ed, "LANDUSES", "LandUse");
            (
                Command::AddRow {
                    section: s("LANDUSES"),
                    fields: vec![n.clone(), s("0"), s("0"), s("0")],
                    comment: None,
                },
                format!("add land use {n}"),
                n.clone(),
                None,
            )
        }
        _ => return None,
    };
    if !ed.apply(obj, &label) {
        return None;
    }
    if let Some(r) = select {
        ed.select_only(r.clone());
        ed.pending_zoom_to = Some(r);
    } else {
        ed.browser.item = Some(name.clone());
    }
    ed.browser.kind = Some(kind);
    state.status = format!("Added {name}");
    Some(name)
}

/// `prefix1`, `prefix2`, ... not yet a row name in `section`.
fn unique_in(ed: &SwmmEditor, section: &str, prefix: &str) -> String {
    (1u64..)
        .map(|n| format!("{prefix}{n}"))
        .find(|c| !ed.doc.contains(section, c))
        .expect("a free name exists")
}

/// Delete what the browser has chosen of `kind`: the selected map objects
/// of that kind (with the usual cascade and confirmation), or the chosen
/// named item of a non-map list.
pub fn delete(state: &mut AppState, kind: BrowserKind) -> usize {
    let ed = &mut state.swmm_doc;
    if let Some(ok) = kind.object_kind() {
        if matches!(
            ok,
            ObjectKind::Curve | ObjectKind::Timeseries | ObjectKind::Pattern
        ) {
            let Some(name) = ed.browser.item.clone() else {
                return 0;
            };
            let ok = ed.apply(
                Command::DeleteObject {
                    section: kind.section().into(),
                    name: name.clone(),
                },
                &format!("delete {} {name}", kind.label().to_lowercase()),
            );
            if ok {
                ed.browser.item = None;
                state.status = format!("Deleted {name}");
                return 1;
            }
            return 0;
        }
        let items: Vec<ObjRef> = ed
            .selection
            .iter()
            .filter(|r| r.kind() == Some(ok))
            .cloned()
            .collect();
        if items.is_empty() {
            state.status = format!("Select a {} first", kind.label().to_lowercase());
            return 0;
        }
        ed.select_many(items);
        return match ed.delete_selection() {
            Some(n) => {
                state.status = format!("Deleted {n} object(s)");
                n
            }
            None => 0,
        };
    }
    match kind {
        BrowserKind::Labels => {
            let items: Vec<ObjRef> = ed
                .selection
                .iter()
                .filter(|r| matches!(r, ObjRef::Label(_)))
                .cloned()
                .collect();
            if items.is_empty() {
                return 0;
            }
            ed.delete_items(&items)
        }
        BrowserKind::Pollutants | BrowserKind::LandUses => {
            let Some(name) = ed.browser.item.clone() else {
                return 0;
            };
            let ok = ed.apply(
                Command::DeleteObject {
                    section: kind.section().into(),
                    name: name.clone(),
                },
                &format!("delete {} {name}", kind.label().to_lowercase()),
            );
            if ok {
                ed.browser.item = None;
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Open the editor a browser node stands for.
pub fn open_editor(state: &mut AppState, kind: BrowserKind, item: Option<&str>) {
    match kind {
        BrowserKind::Title => swmm_dialogs::open_title(&mut state.swmm_doc),
        BrowserKind::Options => swmm_dialogs::open_options(&mut state.swmm_doc),
        BrowserKind::Gages => swmm_dialogs::open_gages(&mut state.swmm_doc, item),
        BrowserKind::Curves => swmm_dialogs::open_curves(&mut state.swmm_doc, item),
        BrowserKind::Timeseries => swmm_dialogs::open_series(&mut state.swmm_doc, item),
        BrowserKind::Patterns => swmm_dialogs::open_patterns(&mut state.swmm_doc, item),
        BrowserKind::Controls => swmm_dialogs::open_controls(&mut state.swmm_doc),
        BrowserKind::Pollutants => swmm_dialogs::open_pollutants(&mut state.swmm_doc),
        BrowserKind::LandUses => swmm_dialogs::open_landuses(&mut state.swmm_doc),
        BrowserKind::Subcatchments
        | BrowserKind::Node(_)
        | BrowserKind::Link(_)
        | BrowserKind::Labels => {
            state.swmm_doc.show_properties = true;
            state.swmm_doc.focus_sheet = true;
        }
        BrowserKind::Backdrop => {
            state.swmm_doc.left_tab = crate::swmm_doc::LeftTab::Layers;
        }
        _ => swmm_grids::open(&mut state.swmm_doc, kind.section()),
    }
}

/// What was clicked: the kind, the item (index and name) if one, and
/// whether it was a double-click.
type Pick = (BrowserKind, Option<(usize, String)>, bool);

struct Tree {
    pick: Option<Pick>,
}

fn matches(search: &str, name: &str) -> bool {
    search.is_empty() || name.to_ascii_lowercase().contains(search)
}

/// One node: the header with its count, and the items when it has any.
fn node(ui: &mut Ui, ed: &SwmmEditor, tree: &mut Tree, kind: BrowserKind, search: &str) {
    let dark = ui.visuals().dark_mode;
    let all = items(ed, kind);
    let shown: Vec<(usize, &String)> = all
        .iter()
        .enumerate()
        .filter(|(_, n)| matches(search, n))
        .collect();
    if !search.is_empty() && shown.is_empty() && !matches(search, kind.label()) {
        return;
    }
    let count = match kind {
        k if k.has_items() => all.len(),
        BrowserKind::Controls => ed.doc.rows("CONTROLS").len(),
        _ => ed.doc.rows(kind.section()).len(),
    };
    let chosen = ed.browser.kind == Some(kind);
    let title = if kind.has_items() || count > 0 {
        format!("{} ({count})", kind.label())
    } else {
        kind.label().to_string()
    };
    let title = if chosen {
        RichText::new(title)
            .color(palette::accent_text(dark))
            .strong()
    } else {
        RichText::new(title)
    };
    if !kind.has_items() {
        let resp = ui.selectable_label(chosen, title);
        if resp.clicked() {
            tree.pick = Some((kind, None, resp.double_clicked()));
        }
        if resp.double_clicked() {
            tree.pick = Some((kind, None, true));
        }
        return;
    }
    let resp = egui::CollapsingHeader::new(title)
        .id_salt(("swmm-browser-node", kind.section()))
        .default_open(!search.is_empty())
        .show(ui, |ui| {
            for (i, name) in &shown {
                let selected = objref(kind, ed, name, *i).is_some_and(|r| ed.is_selected(&r))
                    || (chosen && ed.browser.item.as_deref() == Some(name.as_str()));
                let r = ui.selectable_label(selected, name.as_str());
                if r.clicked() {
                    tree.pick = Some((kind, Some((*i, (*name).clone())), false));
                }
                if r.double_clicked() {
                    tree.pick = Some((kind, Some((*i, (*name).clone())), true));
                }
            }
        });
    if resp.header_response.clicked() {
        tree.pick = Some((kind, None, false));
    }
    if resp.header_response.double_clicked() {
        tree.pick = Some((kind, None, true));
    }
}

fn group(ui: &mut Ui, title: &str, search: &str, body: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt(("swmm-browser-group", title))
        .default_open(true)
        .show(ui, |ui| {
            let _ = search;
            body(ui);
        });
}

/// The browser pane.
pub fn draw_browser(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.label("Search");
        if ui.small_button("×").on_hover_text("Clear").clicked() {
            state.swmm_doc.browser.search.clear();
        }
        // A fixed width: sizing to the pane would widen the pane to fit
        // the field, which widens the field, every frame.
        ui.add(
            egui::TextEdit::singleline(&mut state.swmm_doc.browser.search)
                .id(egui::Id::new("swmm-browser-search"))
                .desired_width(150.0),
        );
    });
    let search = state.swmm_doc.browser.search.trim().to_ascii_lowercase();
    let kind = state.swmm_doc.browser.kind;
    ui.horizontal(|ui| {
        let can_add = kind.is_some_and(|k| k.can_add());
        if ui
            .add_enabled(can_add, egui::Button::new("+"))
            .on_hover_text("Add one of the chosen kind")
            .clicked()
        {
            if let Some(k) = kind {
                add(state, k);
            }
        }
        if ui
            .add_enabled(can_add, egui::Button::new("−"))
            .on_hover_text("Delete the chosen object(s) of that kind")
            .clicked()
        {
            if let Some(k) = kind {
                delete(state, k);
            }
        }
        let can_table = kind.is_some_and(|k| k.has_items());
        if ui
            .add_enabled(can_table, egui::Button::new("Table"))
            .on_hover_text("Attribute table for the chosen kind")
            .clicked()
        {
            if let Some(k) = kind {
                swmm_grids::open(&mut state.swmm_doc, k.section());
            }
        }
        if ui
            .add_enabled(kind.is_some(), egui::Button::new("Edit…"))
            .on_hover_text("Open the editor for the chosen kind")
            .clicked()
        {
            if let Some(k) = kind {
                let item = state.swmm_doc.browser.item.clone();
                open_editor(state, k, item.as_deref());
            }
        }
    });
    ui.separator();

    let mut tree = Tree { pick: None };
    {
        let ed = &state.swmm_doc;
        node(ui, ed, &mut tree, BrowserKind::Title, &search);
        node(ui, ed, &mut tree, BrowserKind::Options, &search);
        group(ui, "Climatology", &search, |ui| {
            node(ui, ed, &mut tree, BrowserKind::Evaporation, &search);
            node(ui, ed, &mut tree, BrowserKind::Temperature, &search);
            node(ui, ed, &mut tree, BrowserKind::Adjustments, &search);
        });
        group(ui, "Hydrology", &search, |ui| {
            node(ui, ed, &mut tree, BrowserKind::Gages, &search);
            node(ui, ed, &mut tree, BrowserKind::Subcatchments, &search);
            for k in [
                BrowserKind::Aquifers,
                BrowserKind::Snowpacks,
                BrowserKind::Hydrographs,
                BrowserKind::LidControls,
            ] {
                if ed.doc.section(k.section()).is_some() {
                    node(ui, ed, &mut tree, k, &search);
                }
            }
        });
        group(ui, "Hydraulics", &search, |ui| {
            ui.label(RichText::new("Nodes").small());
            for k in NodeType::ALL {
                node(ui, ed, &mut tree, BrowserKind::Node(k), &search);
            }
            ui.label(RichText::new("Links").small());
            for k in LinkType::ALL {
                node(ui, ed, &mut tree, BrowserKind::Link(k), &search);
            }
        });
        group(ui, "Quality", &search, |ui| {
            node(ui, ed, &mut tree, BrowserKind::Pollutants, &search);
            node(ui, ed, &mut tree, BrowserKind::LandUses, &search);
        });
        node(ui, ed, &mut tree, BrowserKind::Curves, &search);
        node(ui, ed, &mut tree, BrowserKind::Timeseries, &search);
        node(ui, ed, &mut tree, BrowserKind::Patterns, &search);
        node(ui, ed, &mut tree, BrowserKind::Controls, &search);
        group(ui, "Map", &search, |ui| {
            node(ui, ed, &mut tree, BrowserKind::Labels, &search);
            node(ui, ed, &mut tree, BrowserKind::Backdrop, &search);
        });
    }

    if let Some((kind, item, open)) = tree.pick {
        let ed = &mut state.swmm_doc;
        ed.browser.kind = Some(kind);
        let mut name: Option<String> = None;
        if let Some((i, n)) = item {
            match objref(kind, ed, &n, i) {
                Some(r) => {
                    ed.select_only(r.clone());
                    ed.pending_zoom_to = Some(r);
                    ed.browser.item = None;
                }
                None => ed.browser.item = Some(n.clone()),
            }
            name = Some(n);
        }
        if open {
            open_editor(state, kind, name.as_deref());
        }
    }
}
