// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless tests for the chapter-21 dialogs: open, edit, OK, one undo;
//! Cancel changes nothing; and the lossless promise on the EPA samples —
//! open every dialog on a real model, press OK without edits, and the
//! file is byte for byte what it was.

use std::path::PathBuf;

use eframe::egui::{self, Event, Key, Modifiers, Pos2};
use stormsewer_swmm::doc::build::{self, ObjRef};
use stormsewer_swmm::doc::{schema, validate, InpDoc, Severity};

use super::gw::{self, GroundwaterDraft, NamedDraft, SnowDraft};
use super::quality::{self, PairsDraft, QualityDraft, TreatmentDraft};
use super::{apply_lid_controls, apply_rows, LidControlsDraft, RowsDraft};
use crate::state::AppState;
use crate::swmm_menus;
use crate::swmm_props;
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
        .join("../swmm/tests/fixtures/epa-samples")
        .join(rel)
}

struct Harness {
    app: StormSewerApp,
    ctx: egui::Context,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        let mut app = StormSewerApp::new_for_test(AppState::new_empty());
        swmm_menus::enter_workspace(&mut app.state);
        let mut h = Self {
            app,
            ctx: egui::Context::default(),
            time: 0.0,
        };
        h.frame();
        h.frame();
        h
    }

    fn open(rel: &str) -> Self {
        let mut h = Self::new();
        h.app.state.swmm_doc.open_path(&fixture(rel)).unwrap();
        h.app.state.swmm.pending_map_fit = true;
        h.frame();
        h
    }

    fn frame(&mut self) {
        self.time += 0.05;
        let mut input = raw_input();
        input.time = Some(self.time);
        let _ = self.ctx.run(input, |c| self.app.ui(c));
    }

    fn ed(&self) -> &crate::swmm_doc::SwmmEditor {
        &self.app.state.swmm_doc
    }

    fn ed_mut(&mut self) -> &mut crate::swmm_doc::SwmmEditor {
        &mut self.app.state.swmm_doc
    }

    fn doc(&self) -> &InpDoc {
        &self.app.state.swmm_doc.doc
    }

    fn undo(&mut self) {
        let ev = |pressed| Event::Key {
            key: Key::Z,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::CTRL,
        };
        self.time += 0.5;
        let mut input = raw_input();
        input.time = Some(self.time);
        input.modifiers = Modifiers::CTRL;
        input.events = vec![ev(true)];
        let _ = self.ctx.run(input, |c| self.app.ui(c));
        let mut input = raw_input();
        input.time = Some(self.time + 0.05);
        input.events = vec![ev(false)];
        let _ = self.ctx.run(input, |c| self.app.ui(c));
        self.frame();
    }

    /// Open every chapter-21 dialog on the model, draw a frame, and press
    /// OK on each without edits.
    fn ok_every_dialog_unedited(&mut self) {
        let ed = self.ed_mut();
        super::open_lid_controls(ed, None);
        super::open_lid_usage(ed, None);
        gw::open_aquifers(ed, None);
        gw::open_groundwater(ed, None);
        gw::open_snowpacks(ed, None);
        quality::open_quality(ed, None);
        quality::open_coverages(ed, None);
        quality::open_loadings(ed, None);
        quality::open_treatment(ed, None);
        quality::open_hydrographs(ed, None);
        quality::open_rdii(ed, None);
        self.frame();
        self.frame();
        let ed = self.ed_mut();
        if let Some(mut d) = ed.lid.lid_controls.take() {
            apply_lid_controls(ed, &mut d);
        }
        if let Some(mut d) = ed.lid.lid_usage.take() {
            apply_rows(ed, &mut d, "edit LID usage");
        }
        if let Some(mut d) = ed.lid.aquifers.take() {
            gw::apply_named(ed, &mut d, "edit aquifer");
        }
        if let Some(mut d) = ed.lid.groundwater.take() {
            gw::apply_groundwater(ed, &mut d);
        }
        if let Some(mut d) = ed.lid.snowpacks.take() {
            gw::apply_snowpacks(ed, &mut d);
        }
        if let Some(mut d) = ed.lid.quality.take() {
            quality::apply_quality(ed, &mut d);
        }
        if let Some(mut d) = ed.lid.coverages.take() {
            quality::apply_pairs(ed, &mut d, "edit coverages");
        }
        if let Some(mut d) = ed.lid.loadings.take() {
            quality::apply_pairs(ed, &mut d, "edit loadings");
        }
        if let Some(mut d) = ed.lid.treatment.take() {
            quality::apply_treatment(ed, &mut d);
        }
        if let Some(mut d) = ed.lid.hydrographs.take() {
            gw::apply_named(ed, &mut d, "edit unit hydrographs");
        }
        if let Some(mut d) = ed.lid.rdii.take() {
            apply_rows(ed, &mut d, "edit RDII");
        }
        self.frame();
    }
}

fn errors(doc: &InpDoc) -> Vec<String> {
    doc.validate()
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| format!("[{}] {}: {}", f.section, f.name, f.message))
        .collect()
}

// --- lossless ------------------------------------------------------------------------

#[test]
fn ok_without_edits_leaves_every_epa_sample_byte_for_byte() {
    for name in [
        "LID_Model.inp",
        "Groundwater_Model.inp",
        "Site_Drainage_Model.inp",
        "Detention_Pond_Model.inp",
    ] {
        let original = std::fs::read_to_string(fixture(name)).unwrap();
        let mut h = Harness::open(name);
        assert_eq!(h.doc().to_string(), original, "{name} parses losslessly");
        let depth = h.ed().undo_depth();
        h.ok_every_dialog_unedited();
        // Every named object, not just the first one the dialogs opened on.
        for lid in h.doc().names("LID_CONTROLS") {
            let mut d = LidControlsDraft::load(h.doc(), Some(lid));
            apply_lid_controls(h.ed_mut(), &mut d);
        }
        for sub in h.doc().names("SUBCATCHMENTS") {
            let mut d = RowsDraft::load(h.doc(), "LID_USAGE", &sub);
            apply_rows(h.ed_mut(), &mut d, "lid");
            let mut d = GroundwaterDraft::load(h.doc(), &sub);
            gw::apply_groundwater(h.ed_mut(), &mut d);
            let mut d = PairsDraft::load(h.doc(), "COVERAGES", &sub);
            quality::apply_pairs(h.ed_mut(), &mut d, "cov");
            let mut d = PairsDraft::load(h.doc(), "LOADINGS", &sub);
            quality::apply_pairs(h.ed_mut(), &mut d, "load");
        }
        for lu in h.doc().names("LANDUSES") {
            let mut d = QualityDraft::load(h.doc(), Some(lu));
            quality::apply_quality(h.ed_mut(), &mut d);
        }
        for node in super::node_names(h.doc()) {
            let mut d = TreatmentDraft::load(h.doc(), &node);
            quality::apply_treatment(h.ed_mut(), &mut d);
            let mut d = RowsDraft::load(h.doc(), "RDII", &node);
            apply_rows(h.ed_mut(), &mut d, "rdii");
        }
        for aq in h.doc().names("AQUIFERS") {
            let mut d = NamedDraft::load(h.doc(), "AQUIFERS", Some(aq));
            gw::apply_named(h.ed_mut(), &mut d, "aq");
        }
        assert_eq!(h.ed().undo_depth(), depth, "{name}: no undo step was added");
        assert!(!h.ed().dirty(), "{name}: the model is still clean");
        assert_eq!(h.doc().to_string(), original, "{name}: byte for byte");
        assert!(errors(h.doc()).is_empty(), "{name}: {:?}", errors(h.doc()));
    }
}

// --- LID controls and usage ------------------------------------------------------------

#[test]
fn lid_controls_edit_is_one_step_and_cancel_changes_nothing() {
    let mut h = Harness::open("LID_Model.inp");
    let original = h.doc().to_string();
    super::open_lid_controls(h.ed_mut(), Some("Planters"));
    h.frame();
    let mut d = h.ed().lid.lid_controls.clone().unwrap();
    assert_eq!(d.kind(), "BC");
    assert!(d.layer_row("SOIL").is_some() && d.layer_row("DRAINMAT").is_none());
    // Cancel: an edited draft that is dropped writes nothing.
    d.add_layer("REMOVALS");
    h.ed_mut().lid.lid_controls = None;
    h.frame();
    assert_eq!(h.doc().to_string(), original);

    // OK: the soil thickness and a new pollutant removal, one step.
    super::open_lid_controls(h.ed_mut(), Some("Planters"));
    let mut d = h.ed().lid.lid_controls.clone().unwrap();
    let soil = d.layer_row("SOIL").unwrap();
    let cols = super::row_columns(h.doc(), "LID_CONTROLS", &d.rows[soil]);
    assert_eq!(cols[2], "Thick");
    super::set_cell(&mut d.rows[soil], cols, "LID_CONTROLS", 2, "18");
    d.add_layer("REMOVALS");
    let rm = d.layer_row("REMOVALS").unwrap();
    d.rows[rm].extend(["TSS".to_string(), "40".to_string()]);
    d.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(apply_lid_controls(h.ed_mut(), &mut d));
    assert_eq!(h.ed().undo_depth(), depth + 1, "one step");
    assert_eq!(h.ed().undo_label(), Some("edit LID control Planters"));
    let rows = build::rows_of(h.doc(), "LID_CONTROLS", "Planters");
    assert_eq!(rows.len(), 6, "{rows:?}");
    assert_eq!(rows[2][2], "18");
    assert_eq!(rows[5], vec!["Planters", "REMOVALS", "TSS", "40"]);
    // The engine-facing text re-parses with the Appendix D columns.
    let again = InpDoc::parse(&h.doc().to_string());
    let (_, r) = again.find("LID_CONTROLS", "Planters").unwrap();
    assert_eq!(again.columns("LID_CONTROLS", r), &["Name", "Type"]);
    let soil_row = again
        .find_all("LID_CONTROLS", "Planters")
        .into_iter()
        .find(|r| r.value(1) == Some("SOIL"))
        .unwrap();
    assert_eq!(
        again.columns("LID_CONTROLS", soil_row),
        &["Name", "Layer", "Thick", "Por", "FC", "WP", "Ksat", "Kslope", "Suct"]
    );
    assert_eq!(soil_row.get(again.columns("LID_CONTROLS", soil_row), "Thick"), Some("18"));
    // Other LID controls kept their exact text.
    assert!(h.doc().to_string().contains("GreenRoof       \tSOIL      \t3         \t0.5"));
    h.ed_mut().lid.lid_controls = Some(d);
    h.frame();
    h.undo();
    assert_eq!(h.doc().to_string(), original, "one Ctrl+Z restores the file");
}

#[test]
fn lid_controls_add_rename_duplicate_delete_and_type_switch() {
    let mut h = Harness::open("LID_Model.inp");
    let o = build::new_lid_control(h.doc(), "GR");
    assert_eq!(o.name, "LID1");
    assert!(h.ed_mut().apply(o.command, "add"));
    let rows = build::rows_of(h.doc(), "LID_CONTROLS", "LID1");
    let layers: Vec<&str> = rows.iter().map(|r| r[1].as_str()).collect();
    assert_eq!(layers, vec!["GR", "SURFACE", "SOIL", "DRAINMAT"]);
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    // Rename follows into [LID_USAGE].
    assert!(!super::rename_lid_control(h.ed_mut(), "Planters", "Planter Boxes"));
    assert!(!super::rename_lid_control(h.ed_mut(), "Planters", "GreenRoof"));
    assert!(super::rename_lid_control(h.ed_mut(), "Planters", "Boxes"));
    assert_eq!(h.doc().field("LID_USAGE", "S4", "LID"), Some("Boxes"));
    assert_eq!(build::lid_type_of(h.doc(), "Boxes").as_deref(), Some("BC"));
    assert_eq!(super::lid_uses(h.doc(), "Boxes"), 1);
    // Delete takes the usage rows with it.
    let before = h.doc().rows("LID_USAGE").len();
    let del = super::delete_lid_control(h.doc(), "Boxes");
    assert!(h.ed_mut().apply(del, "del"));
    assert!(!h.doc().contains("LID_CONTROLS", "Boxes"));
    assert_eq!(h.doc().rows("LID_USAGE").len(), before - 1);
    // A type switch adds the layers the new type insists on.
    let mut d = LidControlsDraft::load(h.doc(), Some("Swale".into()));
    d.set_kind("PP");
    assert!(d.layer_row("PAVEMENT").is_some());
    assert!(d.layer_row("SURFACE").is_some());
    assert!(apply_lid_controls(h.ed_mut(), &mut d));
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    h.ed_mut().lid.lid_controls = Some(d);
    h.frame();
}

#[test]
fn lid_usage_dialog_checks_area_and_undoes_in_one_step() {
    let mut h = Harness::open("LID_Model.inp");
    let original = h.doc().to_string();
    h.ed_mut().select_only(ObjRef::Subcatchment("S2".into()));
    super::open_lid_usage(h.ed_mut(), None);
    h.frame();
    let mut d = h.ed().lid.lid_usage.clone().unwrap();
    assert_eq!(d.name, "S2");
    assert!(d.rows.is_empty());
    let mut r = vec!["S2".to_string()];
    r.extend(build::lid_usage_defaults("Planters"));
    d.rows.push(r);
    let cols = super::row_columns(h.doc(), "LID_USAGE", &d.rows[0]);
    assert_eq!(cols, schema::columns("LID_USAGE", &[], None));
    super::set_cell(&mut d.rows[0], cols, "LID_USAGE", 3, "500");
    super::set_cell(&mut d.rows[0], cols, "LID_USAGE", 6, "50");
    d.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(apply_rows(h.ed_mut(), &mut d, "edit LID usage of S2"));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    let (_, row) = h.doc().find("LID_USAGE", "S2").unwrap();
    assert_eq!(row.fields, vec!["S2", "Planters", "1", "500", "0", "0", "50", "0", "*", "*", "0"]);
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    // The sheet count.
    let extras = swmm_props::subcatchment_extras(h.doc(), "S2");
    assert_eq!(extras[0], ("LID Usage…", "1 LID unit".to_string()));
    // Too much area is an error the validator names.
    d.rows[0][3] = "300000".into();
    d.dirty = true;
    assert!(apply_rows(h.ed_mut(), &mut d, "edit LID usage of S2"));
    let (area, total) = build::lid_area_check(h.doc(), "S2").unwrap();
    assert!((area - 4.74 * 43560.0).abs() < 1e-6);
    assert_eq!(total, 300000.0);
    assert!(errors(h.doc()).iter().any(|e| e.contains("ERROR 187")), "{:?}", errors(h.doc()));
    h.ed_mut().lid.lid_usage = Some(d);
    h.frame();
    h.undo();
    h.undo();
    assert_eq!(h.doc().to_string(), original);
}

// --- aquifers and groundwater ------------------------------------------------------------

#[test]
fn groundwater_dialog_writes_gwf_expressions_and_checks_variables() {
    let mut h = Harness::open("Groundwater_Model.inp");
    let original = h.doc().to_string();
    gw::open_aquifers(h.ed_mut(), None);
    gw::open_groundwater(h.ed_mut(), Some("1"));
    h.frame();
    let mut d = h.ed().lid.groundwater.clone().unwrap();
    assert_eq!(d.gw.len(), 1);
    assert_eq!(d.gw[0][1], "1");
    d.lateral = "0.001 * (Hgw - Hcb) * A".into();
    d.deep = "0.002 * Hgw".into();
    let cols = super::row_columns(h.doc(), "GROUNDWATER", &d.gw[0]);
    super::set_cell(&mut d.gw[0], cols, "GROUNDWATER", 12, "2.5"); // Wgr, filling Ebot with *
    d.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(gw::apply_groundwater(h.ed_mut(), &mut d));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    let gwf = h.doc().find_all("GWF", "1");
    assert_eq!(gwf.len(), 2);
    assert_eq!(build::expression_of(&gwf[0].fields), "0.001 * (Hgw - Hcb) * A");
    assert_eq!(gwf[0].value(1), Some("LATERAL"));
    assert_eq!(gwf[1].value(1), Some("DEEP"));
    let (_, row) = h.doc().find("GROUNDWATER", "1").unwrap();
    assert_eq!(row.fields.len(), 13);
    assert_eq!(row.fields[11], "*");
    assert_eq!(row.fields[12], "2.5");
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    // Re-parse: the columns are Appendix D's.
    let again = InpDoc::parse(&h.doc().to_string());
    let (_, r) = again.find("GROUNDWATER", "1").unwrap();
    assert_eq!(r.get(again.columns("GROUNDWATER", r), "Wgr"), Some("2.5"));
    assert_eq!(again.columns("GWF", gwf[0]), &["Subcatchment", "Type", "Expression"]);
    // A bad variable is a finding.
    let mut d2 = GroundwaterDraft::load(h.doc(), "1");
    d2.deep = "Hgw * Bogus".into();
    d2.dirty = true;
    assert!(gw::apply_groundwater(h.ed_mut(), &mut d2));
    assert!(errors(h.doc()).iter().any(|e| e.contains("Bogus")), "{:?}", errors(h.doc()));
    assert_eq!(validate::unknown_gwf_identifiers("exp(-0.5*Hgw) + 1e-3*Ks + Bogus"), vec!["Bogus"]);
    h.ed_mut().lid.groundwater = Some(d2);
    h.frame();
    h.undo();
    h.undo();
    assert_eq!(h.doc().to_string(), original);
    // Aquifer rename follows into [GROUNDWATER]; the aquifers dialog draws.
    let mut a = h.ed().lid.aquifers.clone().unwrap();
    assert_eq!(a.name.as_deref(), Some("1"));
    assert!(gw::rename_named(h.ed_mut(), "AQUIFERS", &[("AQUIFERS", 0), ("GROUNDWATER", 1)], "1", "Sand"));
    assert_eq!(h.doc().field("GROUNDWATER", "1", "Aquifer"), Some("Sand"));
    a = NamedDraft::load(h.doc(), "AQUIFERS", Some("Sand".into()));
    let cols = super::row_columns(h.doc(), "AQUIFERS", &a.rows[0]);
    assert_eq!(cols.len(), 14);
    super::set_cell(&mut a.rows[0], cols, "AQUIFERS", 2, "0.5"); // WP above FC
    a.dirty = true;
    assert!(gw::apply_named(h.ed_mut(), &mut a, "edit aquifer"));
    assert!(errors(h.doc()).iter().any(|e| e.contains("ERROR 109")), "{:?}", errors(h.doc()));
    h.ed_mut().lid.aquifers = Some(a);
    h.frame();
}

#[test]
fn snow_packs_dialog_adds_a_pack_and_renames_it_on_the_subcatchment() {
    let mut h = Harness::open("LID_Model.inp");
    gw::open_snowpacks(h.ed_mut(), None);
    h.frame();
    let o = build::new_snowpack(h.doc());
    assert!(h.ed_mut().apply(o.command, "add"));
    assert_eq!(h.doc().find_all("SNOWPACKS", "SnowPack1").len(), 4);
    assert!(swmm_props::commit(h.ed_mut(), "SUBCATCHMENTS", "S1", "SnowPack", "SnowPack1"));
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    assert!(gw::rename_named(h.ed_mut(), "SNOWPACKS", &[("SNOWPACKS", 0), ("SUBCATCHMENTS", 8)], "SnowPack1", "Plowed"));
    assert_eq!(h.doc().field("SUBCATCHMENTS", "S1", "SnowPack"), Some("Plowed"));
    assert_eq!(h.doc().find_all("SNOWPACKS", "Plowed").len(), 4);
    let mut d = SnowDraft::load(h.doc(), Some("Plowed".into()));
    d.temperature = "SNOWMELT 34 0.5 0.6 0 50 0".into();
    let depth = h.ed().undo_depth();
    assert!(gw::apply_snowpacks(h.ed_mut(), &mut d));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    assert_eq!(h.doc().rows("TEMPERATURE").len(), 1);
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    h.ed_mut().lid.snowpacks = Some(d);
    h.frame();
    h.undo();
    assert_eq!(h.doc().rows("TEMPERATURE").len(), 0);
}

// --- water quality ----------------------------------------------------------------------

#[test]
fn quality_dialogs_edit_buildup_coverages_loadings_and_treatment() {
    let mut h = Harness::open("Site_Drainage_Model.inp");
    let original = h.doc().to_string();
    quality::open_quality(h.ed_mut(), Some("Undeveloped"));
    h.frame();
    let mut d = h.ed().lid.quality.clone().unwrap();
    assert_eq!(d.buildup.len(), 1);
    let cols = super::row_columns(h.doc(), "BUILDUP", &d.buildup[0]);
    assert_eq!(cols, &["LandUse", "Pollutant", "Function", "Coeff1", "Coeff2", "Coeff3", "PerUnit"]);
    super::set_cell(&mut d.buildup[0], cols, "BUILDUP", 2, "SAT");
    super::set_cell(&mut d.buildup[0], cols, "BUILDUP", 3, "50");
    super::set_cell(&mut d.buildup[0], cols, "BUILDUP", 4, "10");
    d.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(quality::apply_quality(h.ed_mut(), &mut d));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    let (_, r) = h.doc().find("BUILDUP", "Undeveloped").unwrap();
    assert_eq!(r.fields, vec!["Undeveloped", "TSS", "SAT", "50", "10", "0.0", "AREA"]);
    h.ed_mut().lid.quality = Some(d);
    h.frame();

    quality::open_coverages(h.ed_mut(), Some("S3"));
    quality::open_loadings(h.ed_mut(), Some("S3"));
    h.frame();
    let mut c = h.ed().lid.coverages.clone().unwrap();
    assert_eq!(c.pairs.len(), 2);
    c.pairs.push(("Undeveloped".into(), "50".into()));
    c.dirty = true;
    assert!(c.total() > 100.0);
    assert!(quality::apply_pairs(h.ed_mut(), &mut c, "edit coverages of S3"));
    assert_eq!(build::pairs_of(h.doc(), "COVERAGES", "S3").len(), 3);
    assert!(h
        .doc()
        .validate()
        .iter()
        .any(|f| f.section == "COVERAGES" && f.name == "S3" && f.message.contains("109 %")));
    let mut l = h.ed().lid.loadings.clone().unwrap();
    assert!(l.pairs.is_empty());
    l.pairs.push(("TSS".into(), "12.5".into()));
    l.dirty = true;
    assert!(quality::apply_pairs(h.ed_mut(), &mut l, "edit loadings of S3"));
    assert_eq!(h.doc().field("LOADINGS", "S3", "Buildup"), Some("12.5"));
    let extras = swmm_props::subcatchment_extras(h.doc(), "S3");
    assert_eq!(extras[2].1, "3 land use(s), 109 %");
    assert_eq!(extras[3].1, "1 pollutant(s)");
    h.ed_mut().lid.coverages = Some(c);
    h.ed_mut().lid.loadings = Some(l);
    h.frame();

    quality::open_treatment(h.ed_mut(), Some("O1"));
    h.frame();
    let mut t = h.ed().lid.treatment.clone().unwrap();
    assert_eq!(t.name, "O1");
    t.pairs.push(("TSS".into(), "R = 1 - exp(-0.2 * HRT)".into()));
    t.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(quality::apply_treatment(h.ed_mut(), &mut t));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    let rows = h.doc().find_all("TREATMENT", "O1");
    assert_eq!(rows.len(), 1);
    assert_eq!(build::expression_of(&rows[0].fields), "R = 1 - exp(-0.2 * HRT)");
    assert_eq!(swmm_props::node_extras(h.doc(), "O1")[0].1, "1 expression(s)");
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    // A bad expression is refused by the check and flagged by the validator.
    assert!(validate::check_treatment_expression("R = 0.5 * FOO", &["TSS".into()]).is_err());
    assert!(validate::check_treatment_expression("0.5 * HRT", &[]).is_err());
    assert!(validate::check_treatment_expression("C = 0.5 * TSS + R_TSS", &["TSS".into()]).is_ok());
    t.pairs[0].1 = "R = 0.5 * FOO".into();
    t.dirty = true;
    assert!(quality::apply_treatment(h.ed_mut(), &mut t));
    assert!(errors(h.doc()).iter().any(|e| e.contains("FOO")), "{:?}", errors(h.doc()));
    h.ed_mut().lid.treatment = Some(t);
    h.frame();
    for _ in 0..6 {
        h.undo();
    }
    assert_eq!(h.doc().to_string(), original);
}

// --- RDII -----------------------------------------------------------------------------------

#[test]
fn hydrographs_and_rdii_dialogs_write_valid_rows_and_rename_together() {
    let mut h = Harness::open("Detention_Pond_Model.inp");
    quality::open_hydrographs(h.ed_mut(), None);
    h.frame();
    let o = build::new_hydrograph(h.doc());
    assert_eq!(o.name, "UH1");
    assert!(h.ed_mut().apply(o.command, "add"));
    let rows = build::rows_of(h.doc(), "HYDROGRAPHS", "UH1");
    assert_eq!(rows.len(), 4);
    assert_eq!(build::hydrograph_gage(h.doc(), "UH1").as_deref(), h.doc().names("RAINGAGES").first().map(String::as_str));
    let mut d = NamedDraft::load(h.doc(), "HYDROGRAPHS", Some("UH1".into()));
    let cols = super::row_columns(h.doc(), "HYDROGRAPHS", &d.rows[1]);
    assert_eq!(cols, &["Name", "Month", "Response", "R", "T", "K", "Dmax", "Drec", "D0"]);
    super::set_cell(&mut d.rows[1], cols, "HYDROGRAPHS", 3, "0.6");
    super::set_cell(&mut d.rows[2], cols, "HYDROGRAPHS", 3, "0.5");
    super::set_cell(&mut d.rows[1], cols, "HYDROGRAPHS", 6, "0.1"); // Dmax; Drec, D0 left out
    d.dirty = true;
    assert!(gw::apply_named(h.ed_mut(), &mut d, "edit unit hydrographs"));
    assert!(errors(h.doc()).iter().any(|e| e.contains("ERROR 153")), "{:?}", errors(h.doc()));
    assert_eq!(quality::ratio_totals(&d.rows), vec![("ALL".to_string(), 1.1)]);
    super::set_cell(&mut d.rows[2], cols, "HYDROGRAPHS", 3, "0.3");
    d.dirty = true;
    assert!(gw::apply_named(h.ed_mut(), &mut d, "edit unit hydrographs"));
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    h.ed_mut().lid.hydrographs = Some(d);
    h.frame();

    let node = h.doc().names("JUNCTIONS")[0].clone();
    quality::open_rdii(h.ed_mut(), Some(&node));
    h.frame();
    let mut r = h.ed().lid.rdii.clone().unwrap();
    assert!(r.rows.is_empty());
    r.rows.push(vec![node.clone(), "UH1".into(), "12".into()]);
    r.dirty = true;
    let depth = h.ed().undo_depth();
    assert!(apply_rows(h.ed_mut(), &mut r, "edit RDII"));
    assert_eq!(h.ed().undo_depth(), depth + 1);
    assert_eq!(h.doc().field("RDII", &node, "SewerArea"), Some("12"));
    assert_eq!(swmm_props::node_extras(h.doc(), &node)[1].1, "set UH1");
    assert!(gw::rename_named(h.ed_mut(), "HYDROGRAPHS", &[("HYDROGRAPHS", 0), ("RDII", 1)], "UH1", "Sewershed"));
    assert_eq!(h.doc().field("RDII", &node, "UnitHydrograph"), Some("Sewershed"));
    assert!(errors(h.doc()).is_empty(), "{:?}", errors(h.doc()));
    h.ed_mut().lid.rdii = Some(r);
    h.frame();
    h.undo();
    h.undo();
    assert!(!h.doc().contains("RDII", &node));
}

// --- menu and sheet ----------------------------------------------------------------------

#[test]
fn project_menu_and_sheets_render_with_the_dialogs_open() {
    let mut h = Harness::open("LID_Model.inp");
    h.ed_mut().select_only(ObjRef::Subcatchment("S1".into()));
    h.frame();
    h.ok_every_dialog_unedited();
    let ed = h.ed_mut();
    super::open_lid_controls(ed, None);
    super::open_lid_usage(ed, None);
    gw::open_aquifers(ed, None);
    gw::open_groundwater(ed, None);
    gw::open_snowpacks(ed, None);
    quality::open_quality(ed, None);
    quality::open_coverages(ed, None);
    quality::open_loadings(ed, None);
    quality::open_treatment(ed, None);
    quality::open_hydrographs(ed, None);
    quality::open_rdii(ed, None);
    h.frame();
    h.frame();
    assert!(h.ed().lid.lid_controls.is_some());
    assert!(h.ed().lid.lid_usage.as_ref().is_some_and(|d| d.name == "S1"));
    let mut state = std::mem::replace(&mut h.app.state, AppState::new_empty());
    let ctx = egui::Context::default();
    let _ = ctx.run(raw_input(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.menu_button("Project", |ui| super::project_menu_items(ui, &mut state));
            swmm_menus::draw_properties_panel(ui, &mut state);
        });
    });
    let _ = Pos2::ZERO;
}

// --- every dialog: OK is one step, Ctrl+Z restores, Cancel writes nothing ----------------

impl Harness {
    /// Draw a few frames with an edited draft open: nothing may be written
    /// until OK. Then drop the draft (what Cancel does) and check the file.
    fn assert_cancel_writes_nothing(&mut self, original: &str, depth: usize, what: &str) {
        self.frame();
        self.frame();
        assert_eq!(self.doc().to_string(), original, "{what}: an open draft writes nothing");
        self.ed_mut().lid = Default::default();
        self.frame();
        assert_eq!(self.ed().undo_depth(), depth, "{what}: Cancel adds no step");
        assert_eq!(self.doc().to_string(), original, "{what}: Cancel changes nothing");
    }

    /// After an OK: exactly one step, the text changed, one Ctrl+Z restores it.
    fn assert_one_step_then_undo(&mut self, original: &str, depth: usize, what: &str) {
        assert_eq!(self.ed().undo_depth(), depth + 1, "{what}: OK is one step");
        assert_ne!(self.doc().to_string(), original, "{what}: OK wrote the edit");
        self.ed_mut().lid = Default::default();
        self.undo();
        assert_eq!(self.doc().to_string(), original, "{what}: one Ctrl+Z restores the file");
        assert_eq!(self.ed().undo_depth(), depth, "{what}: back to the start");
    }
}

/// The rows `cmd` would add for `name` in `section`, applied to a scratch
/// copy of `text`.
fn rows_after(text: &str, cmd: stormsewer_swmm::doc::Command, section: &str, name: &str) -> Vec<Vec<String>> {
    let mut tmp = InpDoc::parse(text);
    tmp.apply(cmd).unwrap();
    build::rows_of(&tmp, section, name)
}

type OpenFn = fn(&mut crate::swmm_doc::SwmmEditor, Option<&str>);

#[test]
fn every_dialog_ok_is_one_undo_step_and_cancel_writes_nothing() {
    // LID Controls (LID_Model).
    let mut h = Harness::open("LID_Model.inp");
    let original = h.doc().to_string();
    let depth = h.ed().undo_depth();
    let edit_lid = |d: &mut LidControlsDraft| {
        d.set_kind("RG");
        d.remove_layer("DRAIN");
        d.dirty = true;
    };
    super::open_lid_controls(h.ed_mut(), Some("Planters"));
    edit_lid(h.ed_mut().lid.lid_controls.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "LID Controls");
    super::open_lid_controls(h.ed_mut(), Some("Planters"));
    let mut d = h.ed().lid.lid_controls.clone().unwrap();
    edit_lid(&mut d);
    assert!(apply_lid_controls(h.ed_mut(), &mut d));
    assert_eq!(build::lid_type_of(h.doc(), "Planters").as_deref(), Some("RG"));
    h.assert_one_step_then_undo(&original, depth, "LID Controls");

    // LID Usage.
    let edit_usage = |d: &mut RowsDraft| {
        d.rows[0][2] = "2".into();
        d.dirty = true;
    };
    super::open_lid_usage(h.ed_mut(), Some("S4"));
    edit_usage(h.ed_mut().lid.lid_usage.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "LID Usage");
    super::open_lid_usage(h.ed_mut(), Some("S4"));
    let mut d = h.ed().lid.lid_usage.clone().unwrap();
    edit_usage(&mut d);
    assert!(apply_rows(h.ed_mut(), &mut d, "edit LID usage of S4"));
    assert_eq!(h.doc().field("LID_USAGE", "S4", "Number"), Some("2"));
    h.assert_one_step_then_undo(&original, depth, "LID Usage");

    // Snow Packs: a pack's rows and the [TEMPERATURE] text in one step.
    let o = build::new_snowpack(h.doc());
    let pack = o.name.clone();
    let pack_rows = rows_after(&original, o.command, "SNOWPACKS", &pack);
    assert_eq!(pack_rows.len(), 4);
    let edit_snow = |d: &mut SnowDraft| {
        d.packs.name = Some(pack.clone());
        d.packs.rows = pack_rows.clone();
        d.packs.dirty = true;
        d.temperature = "SNOWMELT 34 0.5 0.6 0 50 0".into();
    };
    gw::open_snowpacks(h.ed_mut(), None);
    edit_snow(h.ed_mut().lid.snowpacks.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "Snow Packs");
    gw::open_snowpacks(h.ed_mut(), None);
    let mut d = h.ed().lid.snowpacks.clone().unwrap();
    edit_snow(&mut d);
    assert!(gw::apply_snowpacks(h.ed_mut(), &mut d));
    assert_eq!(h.doc().find_all("SNOWPACKS", &pack).len(), 4);
    assert_eq!(h.doc().rows("TEMPERATURE").len(), 1);
    h.assert_one_step_then_undo(&original, depth, "Snow Packs");

    // Aquifers and Groundwater (Groundwater_Model).
    let mut h = Harness::open("Groundwater_Model.inp");
    let original = h.doc().to_string();
    let depth = h.ed().undo_depth();
    let edit_aq = |d: &mut NamedDraft| {
        d.rows[0][4] = "0.2".into(); // Ksat
        d.dirty = true;
    };
    gw::open_aquifers(h.ed_mut(), None);
    edit_aq(h.ed_mut().lid.aquifers.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "Aquifers");
    gw::open_aquifers(h.ed_mut(), None);
    let mut d = h.ed().lid.aquifers.clone().unwrap();
    edit_aq(&mut d);
    assert!(gw::apply_named(h.ed_mut(), &mut d, "edit aquifer"));
    assert_eq!(h.doc().field("AQUIFERS", "1", "Ksat"), Some("0.2"));
    h.assert_one_step_then_undo(&original, depth, "Aquifers");

    let edit_gw = |d: &mut GroundwaterDraft| {
        d.gw[0][3] = "7".into(); // Esurf
        d.lateral = "0.001 * Hgw".into();
        d.dirty = true;
    };
    gw::open_groundwater(h.ed_mut(), Some("1"));
    edit_gw(h.ed_mut().lid.groundwater.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "Groundwater");
    gw::open_groundwater(h.ed_mut(), Some("1"));
    let mut d = h.ed().lid.groundwater.clone().unwrap();
    edit_gw(&mut d);
    assert!(gw::apply_groundwater(h.ed_mut(), &mut d));
    assert_eq!(h.doc().field("GROUNDWATER", "1", "Esurf"), Some("7"));
    assert_eq!(h.doc().find_all("GWF", "1").len(), 1);
    h.assert_one_step_then_undo(&original, depth, "Groundwater");

    // Water quality (Site_Drainage_Model).
    let mut h = Harness::open("Site_Drainage_Model.inp");
    let original = h.doc().to_string();
    let depth = h.ed().undo_depth();
    let edit_bw = |d: &mut QualityDraft| {
        d.washoff[0][3] = "0.5".into(); // Coeff1
        d.dirty = true;
    };
    quality::open_quality(h.ed_mut(), Some("Residential_1"));
    edit_bw(h.ed_mut().lid.quality.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "Buildup / Washoff");
    quality::open_quality(h.ed_mut(), Some("Residential_1"));
    let mut d = h.ed().lid.quality.clone().unwrap();
    edit_bw(&mut d);
    assert!(quality::apply_quality(h.ed_mut(), &mut d));
    assert_eq!(h.doc().find("WASHOFF", "Residential_1").unwrap().1.value(3), Some("0.5"));
    h.assert_one_step_then_undo(&original, depth, "Buildup / Washoff");

    let pair_dialogs: [(&str, OpenFn, &str); 2] = [
        ("COVERAGES", quality::open_coverages, "Undeveloped"),
        ("LOADINGS", quality::open_loadings, "TSS"),
    ];
    for (section, open, add) in pair_dialogs {
        let edit = |d: &mut PairsDraft| {
            d.pairs.push((add.to_string(), "5".into()));
            d.dirty = true;
        };
        fn slot<'a>(h: &'a mut Harness, section: &str) -> &'a mut Option<PairsDraft> {
            if section == "COVERAGES" {
                &mut h.app.state.swmm_doc.lid.coverages
            } else {
                &mut h.app.state.swmm_doc.lid.loadings
            }
        }
        open(h.ed_mut(), Some("S1"));
        edit(slot(&mut h, section).as_mut().unwrap());
        h.assert_cancel_writes_nothing(&original, depth, section);
        open(h.ed_mut(), Some("S1"));
        let mut d = slot(&mut h, section).clone().unwrap();
        edit(&mut d);
        assert!(quality::apply_pairs(h.ed_mut(), &mut d, section));
        assert!(build::pairs_of(h.doc(), section, "S1").contains(&(add.to_string(), "5".to_string())));
        h.assert_one_step_then_undo(&original, depth, section);
    }

    let edit_t = |d: &mut TreatmentDraft| {
        d.pairs.push(("TSS".into(), "R = 0.3".into()));
        d.dirty = true;
    };
    quality::open_treatment(h.ed_mut(), Some("O1"));
    edit_t(h.ed_mut().lid.treatment.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "Treatment");
    quality::open_treatment(h.ed_mut(), Some("O1"));
    let mut d = h.ed().lid.treatment.clone().unwrap();
    edit_t(&mut d);
    assert!(quality::apply_treatment(h.ed_mut(), &mut d));
    assert_eq!(h.doc().find_all("TREATMENT", "O1").len(), 1);
    h.assert_one_step_then_undo(&original, depth, "Treatment");

    // Unit Hydrographs and RDII Inflow (Site_Drainage has a gage and nodes).
    let o = build::new_hydrograph(h.doc());
    let uh = o.name.clone();
    let uh_rows = rows_after(&original, o.command, "HYDROGRAPHS", &uh);
    let edit_uh = |d: &mut NamedDraft| {
        d.name = Some(uh.clone());
        d.rows = uh_rows.clone();
        d.dirty = true;
    };
    quality::open_hydrographs(h.ed_mut(), None);
    edit_uh(h.ed_mut().lid.hydrographs.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "Unit Hydrographs");
    quality::open_hydrographs(h.ed_mut(), None);
    let mut d = h.ed().lid.hydrographs.clone().unwrap();
    edit_uh(&mut d);
    assert!(gw::apply_named(h.ed_mut(), &mut d, "edit unit hydrographs"));
    assert_eq!(build::rows_of(h.doc(), "HYDROGRAPHS", &uh).len(), uh_rows.len());
    h.assert_one_step_then_undo(&original, depth, "Unit Hydrographs");

    let node = h.doc().names("JUNCTIONS")[0].clone();
    let edit_rdii = |d: &mut RowsDraft| {
        d.rows = vec![vec![node.clone(), "UH1".into(), "3".into()]];
        d.dirty = true;
    };
    quality::open_rdii(h.ed_mut(), Some(&node));
    edit_rdii(h.ed_mut().lid.rdii.as_mut().unwrap());
    h.assert_cancel_writes_nothing(&original, depth, "RDII Inflow");
    quality::open_rdii(h.ed_mut(), Some(&node));
    let mut d = h.ed().lid.rdii.clone().unwrap();
    edit_rdii(&mut d);
    assert!(apply_rows(h.ed_mut(), &mut d, "edit RDII"));
    assert_eq!(h.doc().field("RDII", &node, "SewerArea"), Some("3"));
    h.assert_one_step_then_undo(&original, depth, "RDII Inflow");
}
