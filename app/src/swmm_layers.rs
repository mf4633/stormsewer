// SPDX-License-Identifier: GPL-3.0-or-later

//! The layers pane: visibility and label toggles per object kind, symbol
//! size and colour overrides (persisted in the preferences), flow arrows,
//! the grid, the PNG/DXF underlay with its opacity, and the results layers
//! that put the coloured run on the editing map with the legend and the
//! playback controls of the results stream.
//!
//! The map reads `SwmmEditor::layers`; the pane writes it and mirrors it
//! into the preferences so the next session opens the same way.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Ui};
use stormsewer_swmm::doc::build::{LinkType, NodeType};

use crate::state::AppState;
use crate::swmm_results;
use crate::theme::palette;

/// One drawable kind's settings.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerStyle {
    pub visible: bool,
    pub labels: bool,
    /// Symbol size multiplier (1 = the map's default).
    pub size: f32,
    /// Colour override as RGBA, or the map's own colour.
    pub color: Option<[u8; 4]>,
}

impl Default for LayerStyle {
    fn default() -> Self {
        Self {
            visible: true,
            labels: true,
            size: 1.0,
            color: None,
        }
    }
}

impl LayerStyle {
    pub fn color32(&self) -> Option<Color32> {
        self.color
            .map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
    }
}

/// Every kind the map draws, in the order the pane lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayerKind {
    Gages,
    Subcatchments,
    Junctions,
    Outfalls,
    Dividers,
    Storage,
    Conduits,
    Pumps,
    Orifices,
    Weirs,
    Outlets,
    Labels,
    Vertices,
}

impl LayerKind {
    pub const ALL: [LayerKind; 13] = [
        LayerKind::Gages,
        LayerKind::Subcatchments,
        LayerKind::Junctions,
        LayerKind::Outfalls,
        LayerKind::Dividers,
        LayerKind::Storage,
        LayerKind::Conduits,
        LayerKind::Pumps,
        LayerKind::Orifices,
        LayerKind::Weirs,
        LayerKind::Outlets,
        LayerKind::Labels,
        LayerKind::Vertices,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Gages => "Rain Gages",
            Self::Subcatchments => "Subcatchments",
            Self::Junctions => "Junctions",
            Self::Outfalls => "Outfalls",
            Self::Dividers => "Dividers",
            Self::Storage => "Storage Units",
            Self::Conduits => "Conduits",
            Self::Pumps => "Pumps",
            Self::Orifices => "Orifices",
            Self::Weirs => "Weirs",
            Self::Outlets => "Outlets",
            Self::Labels => "Map Labels",
            Self::Vertices => "Vertices",
        }
    }

    pub fn of_node(kind: NodeType) -> Self {
        match kind {
            NodeType::Junction => Self::Junctions,
            NodeType::Outfall => Self::Outfalls,
            NodeType::Divider => Self::Dividers,
            NodeType::Storage => Self::Storage,
        }
    }

    pub fn of_link(kind: LinkType) -> Self {
        match kind {
            LinkType::Conduit => Self::Conduits,
            LinkType::Pump => Self::Pumps,
            LinkType::Orifice => Self::Orifices,
            LinkType::Weir => Self::Weirs,
            LinkType::Outlet => Self::Outlets,
        }
    }

    /// Whether the kind has a text label on the map.
    pub fn has_labels(self) -> bool {
        !matches!(self, Self::Labels | Self::Vertices)
    }
}

/// The whole pane's state, persisted in the preferences.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerSettings {
    #[serde(default)]
    pub gages: LayerStyle,
    #[serde(default)]
    pub subcatchments: LayerStyle,
    #[serde(default)]
    pub junctions: LayerStyle,
    #[serde(default)]
    pub outfalls: LayerStyle,
    #[serde(default)]
    pub dividers: LayerStyle,
    #[serde(default)]
    pub storage: LayerStyle,
    #[serde(default)]
    pub conduits: LayerStyle,
    #[serde(default)]
    pub pumps: LayerStyle,
    #[serde(default)]
    pub orifices: LayerStyle,
    #[serde(default)]
    pub weirs: LayerStyle,
    #[serde(default)]
    pub outlets: LayerStyle,
    #[serde(default)]
    pub labels: LayerStyle,
    #[serde(default)]
    pub vertices: LayerStyle,
    /// Colour nodes by the run's node variable on the editing map.
    #[serde(default)]
    pub node_results: bool,
    /// Colour links by the run's link variable on the editing map.
    #[serde(default)]
    pub link_results: bool,
    #[serde(default = "default_true")]
    pub legend: bool,
}

fn default_true() -> bool {
    true
}

impl Default for LayerSettings {
    fn default() -> Self {
        Self {
            gages: LayerStyle::default(),
            subcatchments: LayerStyle::default(),
            junctions: LayerStyle::default(),
            outfalls: LayerStyle::default(),
            dividers: LayerStyle::default(),
            storage: LayerStyle::default(),
            conduits: LayerStyle::default(),
            pumps: LayerStyle::default(),
            orifices: LayerStyle::default(),
            weirs: LayerStyle::default(),
            outlets: LayerStyle::default(),
            labels: LayerStyle::default(),
            vertices: LayerStyle::default(),
            node_results: false,
            link_results: false,
            legend: true,
        }
    }
}

impl LayerSettings {
    pub fn style(&self, kind: LayerKind) -> &LayerStyle {
        match kind {
            LayerKind::Gages => &self.gages,
            LayerKind::Subcatchments => &self.subcatchments,
            LayerKind::Junctions => &self.junctions,
            LayerKind::Outfalls => &self.outfalls,
            LayerKind::Dividers => &self.dividers,
            LayerKind::Storage => &self.storage,
            LayerKind::Conduits => &self.conduits,
            LayerKind::Pumps => &self.pumps,
            LayerKind::Orifices => &self.orifices,
            LayerKind::Weirs => &self.weirs,
            LayerKind::Outlets => &self.outlets,
            LayerKind::Labels => &self.labels,
            LayerKind::Vertices => &self.vertices,
        }
    }

    pub fn style_mut(&mut self, kind: LayerKind) -> &mut LayerStyle {
        match kind {
            LayerKind::Gages => &mut self.gages,
            LayerKind::Subcatchments => &mut self.subcatchments,
            LayerKind::Junctions => &mut self.junctions,
            LayerKind::Outfalls => &mut self.outfalls,
            LayerKind::Dividers => &mut self.dividers,
            LayerKind::Storage => &mut self.storage,
            LayerKind::Conduits => &mut self.conduits,
            LayerKind::Orifices => &mut self.orifices,
            LayerKind::Pumps => &mut self.pumps,
            LayerKind::Weirs => &mut self.weirs,
            LayerKind::Outlets => &mut self.outlets,
            LayerKind::Labels => &mut self.labels,
            LayerKind::Vertices => &mut self.vertices,
        }
    }

    pub fn node(&self, kind: NodeType) -> &LayerStyle {
        self.style(LayerKind::of_node(kind))
    }

    pub fn link(&self, kind: LinkType) -> &LayerStyle {
        self.style(LayerKind::of_link(kind))
    }

    pub fn results_on(&self) -> bool {
        self.node_results || self.link_results
    }
}

/// Persist the editor's layer settings (not under test, where the user's
/// preferences are not ours to write).
fn persist(state: &mut AppState) {
    state.prefs.swmm_layers = state.swmm_doc.layers.clone();
    if !cfg!(test) {
        state.prefs.save();
    }
}

fn style_row(ui: &mut Ui, kind: LayerKind, style: &mut LayerStyle) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut style.visible, kind.label()).changed();
    });
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        if kind.has_labels() {
            changed |= ui
                .add_enabled(
                    style.visible,
                    egui::Checkbox::new(&mut style.labels, "labels"),
                )
                .changed();
        }
        changed |= ui
            .add_enabled(
                style.visible,
                egui::DragValue::new(&mut style.size)
                    .speed(0.05)
                    .range(0.25..=4.0)
                    .prefix("×")
                    .fixed_decimals(2),
            )
            .on_hover_text("Symbol size")
            .changed();
        let mut c = style.color32().unwrap_or(Color32::TRANSPARENT);
        let had = style.color.is_some();
        if ui
            .add_enabled_ui(style.visible, |ui| {
                egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut c,
                    egui::color_picker::Alpha::OnlyBlend,
                )
            })
            .inner
            .changed()
        {
            style.color = Some(c.to_array());
            changed = true;
        }
        if had
            && ui
                .small_button("reset")
                .on_hover_text("Map colour")
                .clicked()
        {
            style.color = None;
            changed = true;
        }
    });
    changed
}

/// The layers pane.
pub fn draw_layers_pane(ui: &mut Ui, state: &mut AppState) {
    let dark = ui.visuals().dark_mode;
    let mut changed = false;
    ui.label(RichText::new("Objects").strong());
    egui::ScrollArea::vertical()
        .id_salt("swmm-layers-objects")
        .max_height(320.0)
        .show(ui, |ui| {
            for kind in LayerKind::ALL {
                let style = state.swmm_doc.layers.style_mut(kind);
                changed |= style_row(ui, kind, style);
            }
        });
    ui.horizontal(|ui| {
        if ui.small_button("All on").clicked() {
            for kind in LayerKind::ALL {
                state.swmm_doc.layers.style_mut(kind).visible = true;
            }
            changed = true;
        }
        if ui.small_button("All off").clicked() {
            for kind in LayerKind::ALL {
                state.swmm_doc.layers.style_mut(kind).visible = false;
            }
            changed = true;
        }
        if ui.small_button("Defaults").clicked() {
            state.swmm_doc.layers = LayerSettings::default();
            changed = true;
        }
    });
    ui.separator();
    ui.label(RichText::new("Map").strong());
    ui.checkbox(&mut state.swmm_doc.show_arrows, "Flow arrows");
    ui.checkbox(&mut state.swmm_doc.show_grid, "Grid");
    ui.checkbox(&mut state.swmm_doc.show_labels, "Object labels (all)");

    ui.separator();
    ui.label(RichText::new("Underlay").strong());
    if let Some(bg) = state.project.background.as_mut() {
        ui.label(RichText::new(format!("PNG: {}", bg.path)).small());
        ui.add(egui::Slider::new(&mut bg.opacity, 0.0..=1.0).text("opacity"));
        if ui.small_button("Remove PNG").clicked() {
            state.project.background = None;
            state.bg_texture = None;
        }
    } else if ui.small_button("Load PNG…").clicked() {
        let ctx = ui.ctx().clone();
        state.pick_background(&ctx);
    }
    if let Some(dxf) = state.project.background_dxf.as_mut() {
        ui.label(RichText::new(format!("DXF: {}", dxf.path)).small());
        ui.add(egui::Slider::new(&mut dxf.opacity, 0.0..=1.0).text("opacity"));
        if ui.small_button("Remove DXF").clicked() {
            state.project.background_dxf = None;
            state.dxf_underlay.clear();
        }
    } else if ui.small_button("Load DXF…").clicked() {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf", "DXF"])
            .pick_file()
        {
            state.set_background_dxf(path);
        }
    }

    ui.separator();
    ui.label(RichText::new("Results").strong());
    let has_results = state.swmm.results.is_some();
    if !has_results {
        ui.label(RichText::new("Run the model to colour the map by its results.").small());
    }
    let ls = &mut state.swmm_doc.layers;
    changed |= ui
        .add_enabled(
            has_results,
            egui::Checkbox::new(&mut ls.node_results, "Node results"),
        )
        .changed();
    changed |= ui
        .add_enabled(
            has_results,
            egui::Checkbox::new(&mut ls.link_results, "Link results"),
        )
        .changed();
    changed |= ui
        .add_enabled(has_results, egui::Checkbox::new(&mut ls.legend, "Legend"))
        .changed();
    if has_results && state.swmm_doc.layers.results_on() {
        ui.add_space(4.0);
        swmm_results::draw_results_controls(ui, state);
    }
    if changed {
        persist(state);
    }
    if !has_results {
        ui.label(
            RichText::new("Tip: View → Attribute Table lists every object of a kind.")
                .color(palette::muted_text(dark))
                .small(),
        );
    }
}

/// Paint the run's colours on the editing map according to the results
/// layer toggles: the results stream's whole overlay when both layers are
/// on; only the nodes or only the links, from the same caches, otherwise.
/// Then the legend. Draws nothing without a run.
pub fn draw_results_layers(painter: &egui::Painter, rect: Rect, state: &mut AppState, dark: bool) {
    let ls = state.swmm_doc.layers.clone();
    if !ls.results_on() || state.swmm.results.is_none() {
        return;
    }
    if ls.node_results && ls.link_results {
        swmm_results::draw_results_overlay(painter, rect, state, dark);
    } else {
        let showing_frame = state.swmm.frame().is_some();
        let period = state.swmm.period;
        {
            let results = state.swmm.results.as_ref();
            state
                .swmm
                .results_overlay
                .refresh(results, showing_frame, period);
        }
        let Some(file) = state.swmm.results.as_ref() else {
            return;
        };
        let ov = &state.swmm.results_overlay;
        let vp = &state.swmm.map_viewport;
        let ed = &state.swmm_doc;
        let ink = palette::canvas::ink(dark);
        if ls.link_results {
            if let Some(vals) = ov.link_values(showing_frame) {
                let idx: HashMap<String, usize> = file
                    .meta
                    .link_ids
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (s.to_ascii_uppercase(), i))
                    .collect();
                for l in &ed.links {
                    if l.path.len() < 2 || !ls.link(l.kind).visible {
                        continue;
                    }
                    let Some(&i) = idx.get(&l.name.to_ascii_uppercase()) else {
                        continue;
                    };
                    let Some(&v) = vals.get(i) else { continue };
                    let color = swmm_results::class_color(ov.link_breaks.class_of(v));
                    let pts: Vec<Pos2> = l
                        .path
                        .iter()
                        .map(|&(x, y)| vp.world_to_screen(rect, x, y))
                        .collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(3.0_f32, color)));
                }
            }
        }
        if ls.node_results {
            if let Some(vals) = ov.node_values(showing_frame) {
                let idx: HashMap<String, usize> = file
                    .meta
                    .node_ids
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (s.to_ascii_uppercase(), i))
                    .collect();
                for n in &ed.nodes {
                    if !ls.node(n.kind).visible {
                        continue;
                    }
                    let Some(&i) = idx.get(&n.name.to_ascii_uppercase()) else {
                        continue;
                    };
                    let Some(&v) = vals.get(i) else { continue };
                    let color = swmm_results::class_color(ov.node_breaks.class_of(v));
                    let c = vp.world_to_screen(rect, n.x, n.y);
                    let r = 7.0 * ls.node(n.kind).size;
                    painter.circle_filled(c, r, color);
                    painter.circle_stroke(c, r, Stroke::new(1.5_f32, ink));
                }
            }
        }
    }
    if ls.legend {
        swmm_results::legend(painter, rect, state, dark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_default_visible() {
        let mut s = LayerSettings::default();
        assert!(LayerKind::ALL.iter().all(|k| s.style(*k).visible));
        s.style_mut(LayerKind::Junctions).visible = false;
        s.conduits.color = Some([1, 2, 3, 255]);
        let json = serde_json::to_string(&s).unwrap();
        let back: LayerSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert!(!back.node(NodeType::Junction).visible);
        assert_eq!(
            back.link(LinkType::Conduit).color32(),
            Some(Color32::from_rgba_unmultiplied(1, 2, 3, 255))
        );
        // Older preference files without the field still load.
        let old: LayerSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, LayerSettings::default());
    }
}
