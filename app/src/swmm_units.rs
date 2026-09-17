// SPDX-License-Identifier: GPL-3.0-or-later

//! The unit-switch wizard and the offsets note in the Options dialog.
//!
//! Changing `FLOW_UNITS` in the Options dialog changes only a keyword:
//! the engine reads every number in the file as if it were already in the
//! new system. The wizard opens after such a change, says so, lists each
//! group of values the engine does not convert with how many rows it would
//! touch, and converts the ticked groups as one undo step
//! (`stormsewer_swmm::doc::units`). Its report can be copied.
//!
//! `LINK_OFFSETS` has the same trap in miniature: switching DEPTH and
//! ELEVATION silently moves every pipe unless the offsets are rewritten
//! with the node inverts. The General tab shows that and offers to convert
//! in the same step as the option change.

use eframe::egui::{self, RichText, Ui};
use stormsewer_swmm::doc::units::{self, Conversion, FlowUnit, OffsetMode, PlanItem};
use stormsewer_swmm::doc::InpDoc;

use crate::state::AppState;
use crate::swmm_dialogs::OptionsDraft;
use crate::swmm_doc::SwmmEditor;
use crate::theme::palette;

#[derive(Clone, Debug, PartialEq)]
pub struct UnitsWizard {
    pub from: FlowUnit,
    pub to: FlowUnit,
    pub items: Vec<PlanItem>,
    /// The account of what the last Convert changed.
    pub report: Option<String>,
    /// The last text put on the clipboard, for tests.
    pub last_copied: Option<String>,
}

/// Open the wizard for a change already applied to the document.
pub fn open(ed: &mut SwmmEditor, from: FlowUnit, to: FlowUnit) {
    let items = units::plan(&ed.doc, from, to);
    ed.units_wizard = Some(UnitsWizard {
        from,
        to,
        items,
        report: None,
        last_copied: None,
    });
}

/// Convert the ticked groups as one undo step. Returns the account.
pub fn apply(ed: &mut SwmmEditor) -> Option<Conversion> {
    let w = ed.units_wizard.as_ref()?;
    let chosen: Vec<_> = w.items.iter().filter(|p| p.on).map(|p| p.item).collect();
    let (from, to) = (w.from, w.to);
    let conv = units::convert(&ed.doc, from, to, &chosen);
    let label = format!("convert units {} → {}", from.keyword(), to.keyword());
    if !ed.apply(conv.command.clone(), &label) {
        return None;
    }
    let title = format!(
        "Unit conversion {} → {} ({} group(s))",
        from.keyword(),
        to.keyword(),
        chosen.len()
    );
    let report = conv.report(&title);
    if let Some(w) = ed.units_wizard.as_mut() {
        w.report = Some(report);
        // Everything ticked is done; a second Convert would double it.
        for p in &mut w.items {
            if p.on {
                p.on = false;
                p.count = 0;
            }
        }
    }
    Some(conv)
}

/// Put `FLOW_UNITS` back to what it was, as its own step.
pub fn revert(ed: &mut SwmmEditor) -> bool {
    let Some(w) = ed.units_wizard.take() else { return false };
    ed.apply(
        stormsewer_swmm::doc::Command::SetOption {
            section: "OPTIONS".into(),
            key: "FLOW_UNITS".into(),
            value: w.from.keyword().into(),
        },
        &format!("revert FLOW_UNITS to {}", w.from.keyword()),
    )
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut w) = state.swmm_doc.units_wizard.clone() else {
        return;
    };
    let mut open = true;
    let mut convert = false;
    let mut revert_now = false;
    let mut copy = false;
    let dark = ctx.style().visuals.dark_mode;
    let system_change = w.from.is_metric() != w.to.is_metric();
    egui::Window::new("Flow units changed")
        .id(egui::Id::new("swmm-units-wizard"))
        .open(&mut open)
        .default_size(egui::vec2(620.0, 520.0))
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!(
                    "FLOW_UNITS is now {} (was {}).",
                    w.to.keyword(),
                    w.from.keyword()
                ))
                .strong(),
            );
            ui.label(
                RichText::new(
                    "SWMM converts nothing in the file when this changes: every number is read as if it were already in the new units. Only its own output changes.",
                )
                .color(palette::warning_text(dark)),
            );
            if system_change {
                ui.label(RichText::new(
                    "This is a change of unit system (US customary ↔ SI), so lengths, areas, depths, rates and coefficients are affected as well as flows. Manning's n is the same in both; orifice Cd is dimensionless; weir Cw is not.",
                ).small());
            } else {
                ui.label(RichText::new(
                    "Same unit system, so only flow-valued fields are affected.",
                ).small());
            }
            ui.separator();
            ui.label("Tick what to convert (one undo step):");
            egui::ScrollArea::vertical()
                .id_salt("swmm-units-items")
                .max_height(260.0)
                .show(ui, |ui| {
                    for p in &mut w.items {
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(p.count > 0, |ui| {
                                ui.checkbox(&mut p.on, format!("{} ({} value(s))", p.item.label(), p.count));
                            });
                        });
                        ui.indent(("swmm-units-item", p.item.label()), |ui| {
                            ui.label(RichText::new(p.item.explanation()).small().weak());
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let any = w.items.iter().any(|p| p.on);
                if ui.add_enabled(any, egui::Button::new("Convert ticked")).clicked() {
                    convert = true;
                }
                if ui
                    .button(format!("Revert to {}", w.from.keyword()))
                    .on_hover_text("Put FLOW_UNITS back; nothing else changes")
                    .clicked()
                {
                    revert_now = true;
                }
                if ui
                    .add_enabled(w.report.is_some(), egui::Button::new("Copy report"))
                    .clicked()
                {
                    copy = true;
                }
            });
            if let Some(r) = &w.report {
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("swmm-units-report")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut r.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                    });
            }
        });
    if copy {
        if let Some(r) = w.report.clone() {
            ctx.output_mut(|o| o.copied_text = r.clone());
            w.last_copied = Some(r);
            state.status = "Conversion report copied".into();
        }
    }
    state.swmm_doc.units_wizard = Some(w);
    if convert {
        if let Some(c) = apply(&mut state.swmm_doc) {
            state.status = format!("Converted {} value(s)", c.changes.len());
        }
    } else if revert_now {
        revert(&mut state.swmm_doc);
        state.status = "FLOW_UNITS reverted".into();
        return;
    }
    if !open {
        state.swmm_doc.units_wizard = None;
    }
}

/// The LINK_OFFSETS explanation and the convert checkbox on the Options
/// General tab, shown under the option grid.
pub fn offsets_note(ui: &mut Ui, doc: &InpDoc, draft: &mut OptionsDraft) {
    let current = OffsetMode::of(doc);
    let chosen = draft
        .values
        .get("LINK_OFFSETS")
        .and_then(|v| OffsetMode::parse(v))
        .unwrap_or(OffsetMode::Depth);
    let dark = ui.visuals().dark_mode;
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "LINK_OFFSETS: DEPTH means each link offset is a height above its node's invert; ELEVATION means it is an absolute elevation. The keyword alone changes what every offset means.",
        )
        .small()
        .weak(),
    );
    if chosen != current {
        ui.label(
            RichText::new(format!(
                "Switching {} → {} without converting moves every pipe.",
                current.keyword(),
                chosen.keyword()
            ))
            .small()
            .color(palette::warning_text(dark)),
        );
        ui.checkbox(
            &mut draft.convert_offsets,
            "Convert existing offsets with the node inverts (same undo step)",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swmm_design::tests::fixture_text;
    use crate::swmm_dialogs::{apply_options, options_draft};
    use crate::swmm_menus;
    use crate::swmm_profile::tests::run_frame;
    use crate::StormSewerApp;
    use stormsewer_swmm::doc::units::Item;

    fn app_with_pond() -> StormSewerApp {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        app.state
            .swmm_doc
            .open_text(&fixture_text("Detention_Pond_Model.inp"), None);
        app
    }

    #[test]
    fn changing_flow_units_opens_the_wizard_and_converts_ticked_groups_as_one_step() {
        let mut app = app_with_pond();
        let original = app.state.swmm_doc.doc.to_string();
        let mut draft = options_draft(&app.state.swmm_doc.doc);
        assert_eq!(draft.values["FLOW_UNITS"], "CFS");
        assert!(app.state.swmm_doc.units_wizard.is_none());
        draft.values.insert("FLOW_UNITS".into(), "CMS".into());
        assert!(apply_options(&mut app.state.swmm_doc, &draft));
        assert_eq!(app.state.swmm_doc.doc.option("FLOW_UNITS"), Some("CMS"));
        let w = app.state.swmm_doc.units_wizard.clone().expect("wizard opened");
        assert_eq!((w.from, w.to), (FlowUnit::Cfs, FlowUnit::Cms));
        let conduits = w.items.iter().find(|p| p.item == Item::Conduits).unwrap();
        assert!(conduits.on && conduits.count > 10, "{conduits:?}");
        let regs = w.items.iter().find(|p| p.item == Item::Regulators).unwrap();
        // W1: CrestHt and Qcoeff; O1's Cd is dimensionless and its offset 0.
        assert!(regs.on && regs.count == 2, "{regs:?}");
        // Nothing in the file changed but the keyword.
        assert_eq!(app.state.swmm_doc.doc.field("CONDUITS", "C1", "Length"), Some("185.00"));
        run_frame(&mut app);
        assert!(app.state.swmm_doc.units_wizard.is_some(), "window drew and stayed");

        // Untick subcatchments, convert the rest: one step.
        let depth = app.state.swmm_doc.undo_depth();
        if let Some(w) = app.state.swmm_doc.units_wizard.as_mut() {
            for p in &mut w.items {
                if p.item == Item::Subcatchments {
                    p.on = false;
                }
            }
        }
        let conv = apply(&mut app.state.swmm_doc).expect("converted");
        assert!(conv.changes.len() > 20);
        assert_eq!(app.state.swmm_doc.undo_depth(), depth + 1, "one undo step");
        assert_eq!(app.state.swmm_doc.undo_label(), Some("convert units CFS → CMS"));
        let doc = &app.state.swmm_doc.doc;
        assert_eq!(doc.field("CONDUITS", "C1", "Length"), Some("56.388"));
        assert_eq!(doc.field("WEIRS", "W1", "Qcoeff"), Some("1.8384"));
        assert_eq!(doc.field("WEIRS", "W1", "CrestHt"), Some("2.4384"));
        assert_eq!(doc.field("ORIFICES", "O1", "Qcoeff"), Some("0.65"), "orifice Cd is dimensionless");
        assert_eq!(doc.field("SUBCATCHMENTS", "S1", "Area"), doc.field("SUBCATCHMENTS", "S1", "Area"));
        let sub_area_before: f64 = InpDoc::parse(&original).field("SUBCATCHMENTS", "S1", "Area").unwrap().parse().unwrap();
        let sub_area_after: f64 = doc.field("SUBCATCHMENTS", "S1", "Area").unwrap().parse().unwrap();
        assert_eq!(sub_area_before, sub_area_after, "unticked group untouched");
        let w = app.state.swmm_doc.units_wizard.clone().unwrap();
        let report = w.report.clone().expect("report");
        assert!(report.starts_with("Unit conversion CFS → CMS"), "{report}");
        assert!(report.contains("[CONDUITS] C1 Length 185.00 → 56.388"), "{report}");
        assert!(w.items.iter().all(|p| !p.on), "ticked groups are spent");
        run_frame(&mut app);

        // Undo restores the text before the conversion; a second undo the
        // option change.
        assert!(app.state.swmm_doc.undo().is_some());
        assert_eq!(app.state.swmm_doc.doc.field("CONDUITS", "C1", "Length"), Some("185.00"));
        assert_eq!(app.state.swmm_doc.doc.option("FLOW_UNITS"), Some("CMS"));
        assert!(app.state.swmm_doc.undo().is_some());
        assert_eq!(app.state.swmm_doc.doc.to_string(), original);

        // Revert puts the keyword back as a step of its own.
        let mut draft = options_draft(&app.state.swmm_doc.doc);
        draft.values.insert("FLOW_UNITS".into(), "GPM".into());
        assert!(apply_options(&mut app.state.swmm_doc, &draft));
        let w = app.state.swmm_doc.units_wizard.clone().unwrap();
        let on: Vec<Item> = w.items.iter().filter(|p| p.on).map(|p| p.item).collect();
        assert!(!on.contains(&Item::Subcatchments), "same system: lengths untouched: {on:?}");
        // Every InitFlow/MaxFlow in this model is 0, so nothing is ticked.
        assert!(on.is_empty(), "{on:?}");
        assert!(revert(&mut app.state.swmm_doc));
        assert_eq!(app.state.swmm_doc.doc.option("FLOW_UNITS"), Some("CFS"));
        assert!(app.state.swmm_doc.units_wizard.is_none());
    }

    #[test]
    fn link_offsets_switch_converts_offsets_in_the_same_step_when_ticked() {
        let mut app = app_with_pond();
        let original = app.state.swmm_doc.doc.to_string();
        let mut draft = options_draft(&app.state.swmm_doc.doc);
        assert!(draft.convert_offsets, "on by default: the safe choice");
        draft.values.insert("LINK_OFFSETS".into(), "ELEVATION".into());
        let depth = app.state.swmm_doc.undo_depth();
        assert!(apply_options(&mut app.state.swmm_doc, &draft));
        assert_eq!(app.state.swmm_doc.undo_depth(), depth + 1, "option + offsets = one step");
        let doc = &app.state.swmm_doc.doc;
        assert_eq!(doc.option("LINK_OFFSETS"), Some("ELEVATION"));
        // Weir crest 8 ft above the pond invert 4956 → 4964; conduit C2's
        // downstream offset 4 above J11.
        assert_eq!(doc.field("WEIRS", "W1", "CrestHt"), Some("4964"));
        let j11: f64 = doc.field("JUNCTIONS", "J11", "Elevation").unwrap().parse().unwrap();
        let c2: f64 = doc.field("CONDUITS", "C2", "OutOffset").unwrap().parse().unwrap();
        assert!((c2 - (j11 + 4.0)).abs() < 1e-9, "{c2} vs {j11} + 4");
        assert!(app.state.swmm_doc.undo().is_some());
        assert_eq!(app.state.swmm_doc.doc.to_string(), original);

        // Unticked: only the keyword changes, and the note says so.
        let mut draft = options_draft(&app.state.swmm_doc.doc);
        draft.values.insert("LINK_OFFSETS".into(), "ELEVATION".into());
        draft.convert_offsets = false;
        assert!(apply_options(&mut app.state.swmm_doc, &draft));
        assert_eq!(app.state.swmm_doc.doc.field("WEIRS", "W1", "CrestHt"), Some("8"));
        assert_eq!(app.state.swmm_doc.doc.option("LINK_OFFSETS"), Some("ELEVATION"));
        // The General tab draws with the note (the dialog open).
        crate::swmm_dialogs::open_options(&mut app.state.swmm_doc);
        if let Some(d) = app.state.swmm_doc.dialogs.options.as_mut() {
            d.values.insert("LINK_OFFSETS".into(), "DEPTH".into());
        }
        run_frame(&mut app);
        assert!(app.state.swmm_doc.dialogs.options.is_some());
    }
}
