// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed access, commands, undo/redo, and validation over
//! `stormsewer_swmm::doc::InpDoc`.

use std::fs;
use std::path::PathBuf;

use stormsewer_swmm::doc::{Command, InpDoc, ObjectKind, Severity};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/epa-samples")
        .join(name);
    String::from_utf8(fs::read(&path).unwrap()).unwrap()
}

fn all_fixtures() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/epa-samples");
    let mut out: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "inp"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                String::from_utf8(fs::read(&p).unwrap()).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

fn s(v: &str) -> String {
    v.to_string()
}

// ---------------------------------------------------------------------------
// Typed access
// ---------------------------------------------------------------------------

#[test]
fn options_and_title() {
    let doc = InpDoc::parse(&fixture("Detention_Pond_Model.inp"));
    assert_eq!(doc.option("FLOW_UNITS"), Some("CFS"));
    assert_eq!(
        doc.option("flow_units"),
        Some("CFS"),
        "keys are case-insensitive"
    );
    assert_eq!(doc.option("ROUTING_STEP"), Some("0:00:15"));
    assert_eq!(
        doc.title(),
        "A detention pond model.\nSee Detention_Pond_Model.txt for more details."
    );
    assert_eq!(doc.key_value("MAP", "Units").as_deref(), Some("Feet"));
    assert_eq!(doc.key_value("REPORT", "NODES").as_deref(), Some("ALL"));
}

#[test]
fn variable_layouts_resolve_from_the_row() {
    let doc = InpDoc::parse(&fixture("Detention_Pond_Model.inp"));
    // OUTFALLS FREE: no stage column, so Gated is the fourth field.
    assert_eq!(doc.field("OUTFALLS", "O2", "Type"), Some("FREE"));
    assert_eq!(doc.field("OUTFALLS", "O2", "Gated"), Some("NO"));
    assert_eq!(
        doc.field("OUTFALLS", "O2", "Stage"),
        None,
        "FREE outfalls have no stage"
    );
    // STORAGE PYRAMIDAL: L W Z then SurDepth Fevap.
    assert_eq!(doc.field("STORAGE", "SU1", "Shape"), Some("PYRAMIDAL"));
    assert_eq!(doc.field("STORAGE", "SU1", "Length"), Some("180"));
    assert_eq!(doc.field("STORAGE", "SU1", "Width"), Some("90"));
    assert_eq!(doc.field("STORAGE", "SU1", "Z"), Some("2"));
    assert_eq!(doc.field("STORAGE", "SU1", "SurDepth"), Some("0"));
    // XSECTIONS.
    assert_eq!(doc.field("XSECTIONS", "C1", "Shape"), Some("TRAPEZOIDAL"));
    assert_eq!(doc.field("XSECTIONS", "C1", "Geom2"), Some("5"));
    assert_eq!(doc.field("XSECTIONS", "W1", "Geom1"), Some("2"));
    // INFILTRATION follows [OPTIONS] INFILTRATION = HORTON.
    assert_eq!(doc.field("INFILTRATION", "S1", "MaxRate"), Some("4.5"));
    assert_eq!(doc.field("INFILTRATION", "S1", "DryTime"), Some("7"));
    // Links and subcatchments.
    assert_eq!(doc.field("CONDUITS", "C2", "ToNode"), Some("J11"));
    assert_eq!(doc.field("CONDUITS", "C2", "OutOffset"), Some("4"));
    assert_eq!(doc.field("SUBCATCHMENTS", "S4", "Outlet"), Some("J7"));
    assert_eq!(doc.field("SUBAREAS", "S4", "RouteTo"), Some("OUTLET"));
    assert_eq!(doc.field("ORIFICES", "O1", "Qcoeff"), Some("0.65"));
    assert_eq!(doc.field("WEIRS", "W1", "CrestHt"), Some("8"));
    assert_eq!(doc.field("RAINGAGES", "RainGage", "Series"), Some("2-yr"));
    // Numeric column index works too.
    assert_eq!(doc.field("JUNCTIONS", "J5", "1"), Some("4969.8"));
    assert_eq!(doc.field("JUNCTIONS", "J5", "Elevation"), Some("4969.8"));
}

#[test]
fn infiltration_layout_follows_options_and_row_override() {
    let doc = InpDoc::parse("[OPTIONS]\nINFILTRATION GREEN_AMPT\n[INFILTRATION]\nS1 3.5 0.5 0.26\nS2 80 0.5 7 CURVE_NUMBER\n");
    assert_eq!(doc.field("INFILTRATION", "S1", "Suction"), Some("3.5"));
    assert_eq!(doc.field("INFILTRATION", "S1", "IMD"), Some("0.26"));
    assert_eq!(doc.field("INFILTRATION", "S2", "CurveNum"), Some("80"));
    assert_eq!(
        doc.field("INFILTRATION", "S2", "Method"),
        Some("CURVE_NUMBER")
    );
}

#[test]
fn outfall_and_outlet_layouts() {
    let doc = InpDoc::parse(
        "[OUTFALLS]\nO1 0 FIXED 2.5 YES\nO2 0 TIDAL TC NO S1\nO3 0 TIMESERIES TS NO\n[OUTLETS]\nX1 J1 J2 0 FUNCTIONAL/DEPTH 1.5 0.5 NO\nX2 J1 J2 0 TABULAR/HEAD RC YES\n[DIVIDERS]\nD1 0 C1 WEIR 1 2 3 4 0 0 0\nD2 0 C2 CUTOFF 5 4 0 0 0\n",
    );
    assert_eq!(doc.field("OUTFALLS", "O1", "Stage"), Some("2.5"));
    assert_eq!(doc.field("OUTFALLS", "O1", "Gated"), Some("YES"));
    assert_eq!(doc.field("OUTFALLS", "O2", "Curve"), Some("TC"));
    assert_eq!(doc.field("OUTFALLS", "O2", "RouteTo"), Some("S1"));
    assert_eq!(doc.field("OUTFALLS", "O3", "Series"), Some("TS"));
    assert_eq!(doc.field("OUTLETS", "X1", "Qexpon"), Some("0.5"));
    assert_eq!(doc.field("OUTLETS", "X1", "Gated"), Some("NO"));
    assert_eq!(doc.field("OUTLETS", "X2", "Curve"), Some("RC"));
    assert_eq!(doc.field("OUTLETS", "X2", "Gated"), Some("YES"));
    assert_eq!(doc.field("DIVIDERS", "D1", "Cd"), Some("3"));
    assert_eq!(doc.field("DIVIDERS", "D1", "MaxDepth"), Some("4"));
    assert_eq!(doc.field("DIVIDERS", "D2", "Qmin"), Some("5"));
    assert_eq!(doc.field("DIVIDERS", "D2", "MaxDepth"), Some("4"));
}

#[test]
fn series_curves_geometry_tags() {
    let doc = InpDoc::parse(&fixture("Detention_Pond_Model.inp"));
    let ts = doc.timeseries("2-yr");
    assert_eq!(ts.len(), 24);
    assert_eq!(ts[0].time, "0:00");
    assert_eq!(ts[0].value, "0.29");
    assert_eq!(ts[0].date, None);
    let (kind, pts) = doc.curve("SU1");
    assert_eq!(kind.as_deref(), Some("Storage"));
    assert_eq!(pts, vec![(s("0"), s("21780")), (s("4"), s("21780"))]);
    assert!(doc.coordinates("J1").is_some());
    assert!(doc.symbol("RainGage").is_some());
    assert!(!doc.polygon("S1").is_empty());
    assert_eq!(doc.names("JUNCTIONS").len(), 12);
    assert_eq!(
        doc.defining_section(ObjectKind::Node, "su1"),
        Some("STORAGE")
    );
    assert_eq!(doc.defining_section(ObjectKind::Link, "W1"), Some("WEIRS"));

    let site = InpDoc::parse(&fixture("Site_Drainage_Model.inp"));
    assert_eq!(site.tag("Link", "C3"), Some("Culvert"));
    assert_eq!(site.tag("Node", "C3"), None);

    let dated = InpDoc::parse(
        "[TIMESERIES]\nTS 01/02/2020 0:00 1 0:15 2\nTS 0:30 3\nTS2 FILE \"rain data.dat\"\n",
    );
    let pts = dated.timeseries("TS");
    assert_eq!(pts.len(), 3);
    assert_eq!(pts[1].date.as_deref(), Some("01/02/2020"));
    assert_eq!(pts[1].time, "0:15");
    assert_eq!(pts[2].value, "3");
    assert!(dated.timeseries("TS2").is_empty());
    assert_eq!(
        dated.field("TIMESERIES", "TS2", "File"),
        Some("rain data.dat")
    );
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[test]
fn set_field_rewrites_only_that_row_with_section_alignment() {
    let original = fixture("Detention_Pond_Model.inp");
    let mut doc = InpDoc::parse(&original);
    doc.apply(Command::SetField {
        section: s("JUNCTIONS"),
        name: s("J5"),
        field: s("Elevation"),
        value: s("4970.25"),
    })
    .unwrap();
    assert_eq!(doc.field("JUNCTIONS", "J5", "Elevation"), Some("4970.25"));
    let text = doc.to_string();
    assert!(
        text.contains("J5              \t4970.25   \t0         \t0         \t0         \t0\r\n"),
        "row re-aligned to the ruler with tabs:\n{text}"
    );
    // Everything else is untouched: only one line differs.
    let diffs: Vec<(&str, &str)> = original
        .lines()
        .zip(text.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(diffs.len(), 1, "{diffs:?}");
    assert!(doc.dirty());
    assert!(doc.undo());
    assert_eq!(doc.to_string(), original);
    assert!(!doc.dirty());
    assert!(doc.redo());
    assert_eq!(doc.to_string(), text);
}

#[test]
fn set_field_keeps_trailing_comment_and_appends_one_column() {
    let mut doc = InpDoc::parse("[JUNCTIONS]\nJ1 10 0 ; inlet\n");
    doc.apply(Command::SetField {
        section: s("JUNCTIONS"),
        name: s("J1"),
        field: s("MaxDepth"),
        value: s("3"),
    })
    .unwrap();
    assert_eq!(
        doc.to_string(),
        "[JUNCTIONS]\nJ1               10         3          ; inlet\n"
    );
    // InitDepth is the next column: allowed. SurDepth would skip one: refused.
    doc.apply(Command::SetField {
        section: s("JUNCTIONS"),
        name: s("J1"),
        field: s("InitDepth"),
        value: s("0.5"),
    })
    .unwrap();
    let err = doc
        .apply(Command::SetField {
            section: s("JUNCTIONS"),
            name: s("J1"),
            field: s("Aponded"),
            value: s("1"),
        })
        .unwrap_err();
    assert!(err.to_string().contains("column 5"), "{err}");
    assert_eq!(doc.field("JUNCTIONS", "J1", "InitDepth"), Some("0.5"));
    let err = doc
        .apply(Command::SetField {
            section: s("JUNCTIONS"),
            name: s("J1"),
            field: s("Elevation"),
            value: s("1;2"),
        })
        .unwrap_err();
    assert!(err.to_string().contains("';'"), "{err}");
}

#[test]
fn add_row_into_empty_section_and_missing_section() {
    let mut doc =
        InpDoc::parse("[JUNCTIONS]\r\nJ1 10 0\r\n\r\n[TAGS]\r\n\r\n[MAP]\r\nUnits None\r\n");
    doc.apply(Command::AddRow {
        section: s("TAGS"),
        fields: vec![s("Node"), s("J1"), s("Inlet")],
        comment: None,
    })
    .unwrap();
    doc.apply(Command::AddRow {
        section: s("COORDINATES"),
        fields: vec![s("J1"), s("10"), s("20")],
        comment: Some(s("; new")),
    })
    .unwrap();
    let text = doc.to_string();
    assert_eq!(
        text,
        "[JUNCTIONS]\r\nJ1 10 0\r\n\r\n[TAGS]\r\nNode             J1         Inlet\r\n\r\n[MAP]\r\nUnits None\r\n\r\n[COORDINATES]\r\nJ1               10         20         ; new\r\n\r\n"
    );
    assert_eq!(doc.coordinates("J1"), Some((10.0, 20.0)));
    assert_eq!(doc.tag("Node", "J1"), Some("Inlet"));
    doc.undo();
    doc.undo();
    assert_eq!(
        doc.to_string(),
        "[JUNCTIONS]\r\nJ1 10 0\r\n\r\n[TAGS]\r\n\r\n[MAP]\r\nUnits None\r\n"
    );
}

#[test]
fn appending_after_an_unterminated_last_line() {
    let mut doc = InpDoc::parse("[JUNCTIONS]\nJ1 10 0");
    doc.apply(Command::AddRow {
        section: s("JUNCTIONS"),
        fields: vec![s("J2"), s("9"), s("0")],
        comment: None,
    })
    .unwrap();
    assert_eq!(
        doc.to_string(),
        "[JUNCTIONS]\nJ1 10 0\nJ2               9          0"
    );
    doc.undo();
    assert_eq!(doc.to_string(), "[JUNCTIONS]\nJ1 10 0");

    let mut doc = InpDoc::parse("[TAGS]");
    doc.apply(Command::AddRow {
        section: s("TAGS"),
        fields: vec![s("Node"), s("J1"), s("x")],
        comment: None,
    })
    .unwrap();
    assert_eq!(doc.to_string(), "[TAGS]\r\nNode             J1         x");
    doc.undo();
    assert_eq!(doc.to_string(), "[TAGS]");

    let mut doc = InpDoc::parse("preamble only");
    doc.apply(Command::SetOption {
        section: s("OPTIONS"),
        key: s("FLOW_UNITS"),
        value: s("CFS"),
    })
    .unwrap();
    assert_eq!(
        doc.to_string(),
        "preamble only\r\n[OPTIONS]\r\nFLOW_UNITS       CFS\r\n\r\n"
    );
    doc.undo();
    assert_eq!(doc.to_string(), "preamble only");
}

#[test]
fn rename_node_updates_every_reference() {
    let mut doc = InpDoc::parse(&fixture("Detention_Pond_Model.inp"));
    // J10 is: a junction, C9's to-node, C10's from-node, S5 and S7's outlet,
    // and has coordinates.
    doc.apply(Command::Rename {
        kind: ObjectKind::Node,
        old: s("J10"),
        new: s("MH-10"),
    })
    .unwrap();
    assert!(!doc.contains("JUNCTIONS", "J10"));
    assert!(doc.contains("JUNCTIONS", "MH-10"));
    assert_eq!(doc.field("CONDUITS", "C9", "ToNode"), Some("MH-10"));
    assert_eq!(doc.field("CONDUITS", "C10", "FromNode"), Some("MH-10"));
    assert_eq!(doc.field("SUBCATCHMENTS", "S5", "Outlet"), Some("MH-10"));
    assert_eq!(doc.field("SUBCATCHMENTS", "S7", "Outlet"), Some("MH-10"));
    assert!(doc.coordinates("J10").is_none());
    assert!(doc.coordinates("MH-10").is_some());
    // Nothing else changed: J1, J11 untouched.
    assert!(doc.contains("JUNCTIONS", "J1"));
    assert!(doc.contains("JUNCTIONS", "J11"));
    assert_eq!(doc.field("CONDUITS", "C2", "ToNode"), Some("J11"));
    assert!(
        doc.validate().iter().all(|f| f.severity != Severity::Error),
        "{:?}",
        doc.validate()
    );
    // Duplicate target is refused, case-insensitively.
    let err = doc
        .apply(Command::Rename {
            kind: ObjectKind::Node,
            old: s("J1"),
            new: s("j2"),
        })
        .unwrap_err();
    assert!(err.to_string().contains("already"), "{err}");
}

#[test]
fn rename_reaches_controls_labels_tags_and_inflows() {
    let mut doc = InpDoc::parse(&fixture("Pump_Control_Model.inp"));
    doc.apply(Command::Rename {
        kind: ObjectKind::Node,
        old: s("SU1"),
        new: s("WetWell"),
    })
    .unwrap();
    let text = doc.to_string();
    assert!(
        text.contains("IF NODE WetWell DEPTH >= 4"),
        "controls patched:\n{text}"
    );
    assert!(!text.contains("NODE SU1 "), "no stale rule reference");
    assert_eq!(doc.field("PUMPS", "PUMP1", "FromNode"), Some("WetWell"));
    assert_eq!(
        doc.field("STORAGE", "WetWell", "Shape"),
        Some("CYLINDRICAL"),
        "{:?}",
        doc.find("STORAGE", "WetWell")
    );
    assert_eq!(doc.field("STORAGE", "WetWell", "Length"), Some("6"));
    doc.apply(Command::Rename {
        kind: ObjectKind::Link,
        old: s("PUMP1"),
        new: s("P-1"),
    })
    .unwrap();
    let text = doc.to_string();
    assert!(text.contains("THEN PUMP P-1 status = ON"), "{text}");
    assert!(doc.contains("PUMPS", "P-1"));
    doc.apply(Command::Rename {
        kind: ObjectKind::Curve,
        old: s("PUMP_CURVE1"),
        new: s("PC1"),
    })
    .unwrap();
    assert_eq!(doc.field("PUMPS", "P-1", "Curve"), Some("PC1"));
    assert_eq!(
        doc.find_all("CURVES", "PC1").len(),
        doc.rows("CURVES").len()
    );
    doc.apply(Command::Rename {
        kind: ObjectKind::Node,
        old: s("PSO"),
        new: s("Regulator"),
    })
    .unwrap();
    let label = doc.rows("LABELS")[0].1;
    assert_eq!(
        label.get(doc.columns("LABELS", label), "Anchor"),
        Some("Regulator"),
        "label anchor follows the node: {label:?}"
    );
    assert_eq!(
        label.get(doc.columns("LABELS", label), "Label"),
        Some("Regulator Point")
    );
    doc.apply(Command::Rename {
        kind: ObjectKind::Pattern,
        old: s("DWF"),
        new: s("Diurnal"),
    })
    .unwrap();
    assert_eq!(doc.field("DWF", "KRO3001", "Pattern3"), Some("Diurnal"));
    assert_eq!(doc.names("PATTERNS"), vec!["Diurnal"]);

    let mut site = InpDoc::parse(&fixture("Site_Drainage_Model.inp"));
    site.apply(Command::Rename {
        kind: ObjectKind::Link,
        old: s("C3"),
        new: s("Culv-3"),
    })
    .unwrap();
    assert_eq!(site.tag("Link", "Culv-3"), Some("Culvert"));
    assert!(site.contains("XSECTIONS", "Culv-3"));
    assert!(!site.contains("XSECTIONS", "C3"));
    site.apply(Command::Rename {
        kind: ObjectKind::Subcatchment,
        old: s("S1"),
        new: s("Sub-1"),
    })
    .unwrap();
    assert!(site.contains("SUBAREAS", "Sub-1"));
    assert!(site.contains("INFILTRATION", "Sub-1"));
    assert!(!site.polygon("Sub-1").is_empty());
    assert!(site.polygon("S1").is_empty());
    site.apply(Command::Rename {
        kind: ObjectKind::Gage,
        old: s("RainGage"),
        new: s("RG1"),
    })
    .unwrap();
    assert_eq!(
        site.field("SUBCATCHMENTS", "Sub-1", "RainGage"),
        Some("RG1")
    );
    assert!(site.symbol("RG1").is_some());
    let culvert = fixture("Culvert_Model.inp");
    let mut culvert_doc = InpDoc::parse(&culvert);
    culvert_doc
        .apply(Command::Rename {
            kind: ObjectKind::Timeseries,
            old: s("Inflow"),
            new: s("Hydrograph"),
        })
        .unwrap();
    assert_eq!(
        culvert_doc.field("INFLOWS", "Inlet", "Series"),
        Some("Hydrograph")
    );
    assert!(culvert_doc.timeseries("Hydrograph").len() > 5);
    culvert_doc.undo();
    assert_eq!(culvert_doc.to_string(), culvert);
}

#[test]
fn cascade_delete_node_takes_links_and_dependents() {
    let original = fixture("Detention_Pond_Model.inp");
    let mut doc = InpDoc::parse(&original);
    let cmd = doc.cascade_delete_node("SU1");
    doc.apply(cmd).unwrap();
    assert!(!doc.contains("STORAGE", "SU1"));
    assert!(!doc.contains("CONDUITS", "C11"), "C11 ends at SU1");
    assert!(!doc.contains("ORIFICES", "O1"));
    assert!(!doc.contains("WEIRS", "W1"));
    assert!(!doc.contains("XSECTIONS", "C11"));
    assert!(!doc.contains("XSECTIONS", "O1"));
    assert!(!doc.contains("XSECTIONS", "W1"));
    assert!(doc.coordinates("SU1").is_none());
    assert!(doc.vertices("C11").is_empty());
    assert!(doc.contains("CONDUITS", "C10"));
    assert!(
        doc.validate().iter().all(|f| f.severity != Severity::Error),
        "{:?}",
        doc.validate()
    );
    assert_eq!(doc.undo_depth(), 1, "the cascade is one undo step");
    doc.undo();
    assert_eq!(doc.to_string(), original);
    doc.redo();
    assert!(!doc.contains("STORAGE", "SU1"));
}

#[test]
fn geometry_commands() {
    let original = fixture("Detention_Pond_Model.inp");
    let mut doc = InpDoc::parse(&original);
    doc.apply(Command::MoveNode {
        name: s("J1"),
        x: 100.5,
        y: -20.0,
    })
    .unwrap();
    assert_eq!(doc.coordinates("J1"), Some((100.5, -20.0)));
    doc.apply(Command::MoveNode {
        name: s("NewNode"),
        x: 1.0,
        y: 2.0,
    })
    .unwrap();
    assert_eq!(doc.coordinates("NewNode"), Some((1.0, 2.0)));
    doc.apply(Command::MoveGage {
        name: s("RainGage"),
        x: 0.0,
        y: 0.0,
    })
    .unwrap();
    assert_eq!(doc.symbol("RainGage"), Some((0.0, 0.0)));

    assert_eq!(doc.vertices("C9").len(), 10);
    let first_c9 = doc
        .section("VERTICES")
        .unwrap()
        .rows()
        .find(|(_, r)| r.fields[0] == "C9")
        .map(|(i, _)| i)
        .unwrap();
    doc.apply(Command::SetVertices {
        link: s("C9"),
        points: vec![(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)],
    })
    .unwrap();
    assert_eq!(doc.vertices("C9"), vec![(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
    // Replaced in place: the rows sit where the old ones were, contiguous.
    let lines = doc.section("VERTICES").unwrap();
    let idx: Vec<usize> = lines
        .rows()
        .filter(|(_, r)| r.fields[0] == "C9")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(idx, vec![first_c9, first_c9 + 1, first_c9 + 2]);
    assert_eq!(doc.vertices("C5").len(), 4, "neighbouring links untouched");
    doc.apply(Command::SetVertices {
        link: s("C9"),
        points: vec![],
    })
    .unwrap();
    assert!(doc.vertices("C9").is_empty());
    doc.apply(Command::SetVertices {
        link: s("Brand-New"),
        points: vec![(5.0, 5.0)],
    })
    .unwrap();
    assert_eq!(doc.vertices("Brand-New"), vec![(5.0, 5.0)]);

    doc.apply(Command::SetPolygon {
        subcatchment: s("S1"),
        points: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
    })
    .unwrap();
    assert_eq!(doc.polygon("S1").len(), 3);

    doc.apply(Command::SetOption {
        section: s("OPTIONS"),
        key: s("flow_units"),
        value: s("CMS"),
    })
    .unwrap();
    assert_eq!(doc.option("FLOW_UNITS"), Some("CMS"));
    assert!(
        doc.to_string().contains("FLOW_UNITS          \tCMS\r\n"),
        "key spelling and header alignment kept"
    );
    doc.apply(Command::SetOption {
        section: s("REPORT"),
        key: s("NODES"),
        value: s("J1 J2"),
    })
    .unwrap();
    assert_eq!(doc.key_value("REPORT", "NODES").as_deref(), Some("J1 J2"));
    doc.apply(Command::SetOption {
        section: s("MAP"),
        key: s("DIMENSIONS"),
        value: s("0 0 100 100"),
    })
    .unwrap();
    assert_eq!(doc.field("MAP", "DIMENSIONS", "X2"), Some("100"));

    doc.apply(Command::SetTitle {
        text: s("New title\nsecond line"),
    })
    .unwrap();
    assert_eq!(doc.title(), "New title\nsecond line");
    assert!(
        doc.to_string().contains(
            "[TITLE]\r\n;;Project Title/Notes\r\nNew title\r\nsecond line\r\n\r\n[OPTIONS]"
        ),
        "title replaced in place, comment kept"
    );

    while doc.undo() {}
    assert_eq!(doc.to_string(), original);
    while doc.redo() {}
    assert_eq!(doc.title(), "New title\nsecond line");
    assert_eq!(doc.polygon("S1").len(), 3);
}

#[test]
fn batch_is_atomic() {
    let original = "[JUNCTIONS]\nJ1 10 0\n";
    let mut doc = InpDoc::parse(original);
    let err = doc
        .apply(Command::Batch(vec![
            Command::SetField {
                section: s("JUNCTIONS"),
                name: s("J1"),
                field: s("Elevation"),
                value: s("11"),
            },
            Command::AddRow {
                section: s("JUNCTIONS"),
                fields: vec![s("J2"), s("9"), s("0")],
                comment: None,
            },
            Command::SetField {
                section: s("JUNCTIONS"),
                name: s("Missing"),
                field: s("Elevation"),
                value: s("1"),
            },
        ]))
        .unwrap_err();
    assert!(err.to_string().contains("Missing"), "{err}");
    assert_eq!(doc.to_string(), original, "partial batch rolled back");
    assert!(!doc.can_undo());
    assert!(!doc.dirty());
}

#[test]
fn gesture_folds_a_drag_into_one_step() {
    let original = fixture("Culvert_Model.inp");
    let mut doc = InpDoc::parse(&original);
    let start = doc.coordinates("Inlet").unwrap();
    doc.begin_gesture();
    for i in 1..=40 {
        doc.apply(Command::MoveNode {
            name: s("Inlet"),
            x: start.0 + f64::from(i),
            y: start.1,
        })
        .unwrap();
    }
    assert!(doc.dirty());
    assert!(doc.can_undo());
    doc.end_gesture();
    assert_eq!(doc.undo_depth(), 1);
    assert_eq!(doc.coordinates("Inlet"), Some((start.0 + 40.0, start.1)));
    assert!(doc.undo());
    assert_eq!(doc.to_string(), original);
    assert!(doc.redo());
    assert_eq!(doc.coordinates("Inlet"), Some((start.0 + 40.0, start.1)));
    // An empty gesture leaves no step; undo inside a gesture closes it.
    doc.begin_gesture();
    doc.end_gesture();
    assert_eq!(doc.undo_depth(), 1);
    doc.begin_gesture();
    doc.apply(Command::MoveNode {
        name: s("Inlet"),
        x: 0.0,
        y: 0.0,
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(doc.coordinates("Inlet"), Some((start.0 + 40.0, start.1)));
}

#[test]
fn history_is_bounded_and_save_point_tracks() {
    let mut doc = InpDoc::parse("[JUNCTIONS]\nJ1 10 0\n");
    doc.mark_saved();
    for i in 0..(InpDoc::HISTORY_LIMIT + 50) {
        doc.apply(Command::SetField {
            section: s("JUNCTIONS"),
            name: s("J1"),
            field: s("Elevation"),
            value: i.to_string(),
        })
        .unwrap();
    }
    assert_eq!(doc.undo_depth(), InpDoc::HISTORY_LIMIT);
    let mut undone = 0;
    while doc.undo() {
        undone += 1;
    }
    assert_eq!(undone, InpDoc::HISTORY_LIMIT);
    assert_eq!(
        doc.field("JUNCTIONS", "J1", "Elevation"),
        Some("49"),
        "the oldest 50 steps are gone"
    );
    assert!(
        doc.dirty(),
        "cannot get back to the save point once its step is dropped"
    );

    let mut doc = InpDoc::parse("[JUNCTIONS]\nJ1 10 0\n");
    assert!(!doc.dirty());
    doc.apply(Command::SetField {
        section: s("JUNCTIONS"),
        name: s("J1"),
        field: s("Elevation"),
        value: s("1"),
    })
    .unwrap();
    doc.apply(Command::SetField {
        section: s("JUNCTIONS"),
        name: s("J1"),
        field: s("Elevation"),
        value: s("2"),
    })
    .unwrap();
    doc.mark_saved();
    assert!(!doc.dirty());
    doc.undo();
    assert!(doc.dirty());
    doc.redo();
    assert!(!doc.dirty());
    doc.undo();
    doc.apply(Command::SetField {
        section: s("JUNCTIONS"),
        name: s("J1"),
        field: s("Elevation"),
        value: s("3"),
    })
    .unwrap();
    assert!(doc.dirty());
    assert!(!doc.can_redo());
    // A no-op command is not a step and does not dirty.
    let mut doc = InpDoc::parse("[JUNCTIONS]\nJ1 10 0\n");
    doc.apply(Command::DeleteObject {
        section: s("JUNCTIONS"),
        name: s("nobody"),
    })
    .unwrap();
    doc.apply(Command::SetField {
        section: s("JUNCTIONS"),
        name: s("J1"),
        field: s("Elevation"),
        value: s("10"),
    })
    .unwrap();
    assert!(!doc.dirty());
    assert_eq!(doc.undo_depth(), 0);
}

/// A small deterministic generator.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.next() as usize % items.len()])
        }
    }
}

fn random_command(doc: &InpDoc, rng: &mut Lcg, i: usize) -> Command {
    let nodes: Vec<String> = ["JUNCTIONS", "OUTFALLS", "STORAGE"]
        .iter()
        .flat_map(|sec| doc.names(sec))
        .collect();
    let links: Vec<String> = ["CONDUITS", "PUMPS", "ORIFICES", "WEIRS"]
        .iter()
        .flat_map(|sec| doc.names(sec))
        .collect();
    let subs = doc.names("SUBCATCHMENTS");
    let x = (rng.next() % 10_000) as f64 / 7.0;
    let y = (rng.next() % 10_000) as f64 / 3.0;
    match rng.next() % 12 {
        0 => match rng.pick(&nodes) {
            Some(n) if doc.contains("JUNCTIONS", n) => Command::SetField {
                section: s("JUNCTIONS"),
                name: n.clone(),
                field: s("Elevation"),
                value: format!("{x:.2}"),
            },
            _ => Command::SetOption {
                section: s("OPTIONS"),
                key: s("MIN_SLOPE"),
                value: format!("{}", i),
            },
        },
        1 => match rng.pick(&nodes) {
            Some(n) => Command::MoveNode {
                name: n.clone(),
                x,
                y,
            },
            None => Command::SetTitle {
                text: format!("title {i}"),
            },
        },
        2 => match rng.pick(&links) {
            Some(l) => Command::SetVertices {
                link: l.clone(),
                points: vec![(x, y), (y, x)],
            },
            None => Command::SetTitle {
                text: format!("title {i}"),
            },
        },
        3 => match rng.pick(&nodes) {
            Some(n) => Command::Rename {
                kind: ObjectKind::Node,
                old: n.clone(),
                new: format!("ZN{i}"),
            },
            None => Command::SetTitle {
                text: format!("title {i}"),
            },
        },
        4 => match rng.pick(&links) {
            Some(l) => Command::Rename {
                kind: ObjectKind::Link,
                old: l.clone(),
                new: format!("ZL{i}"),
            },
            None => Command::SetTitle {
                text: format!("title {i}"),
            },
        },
        5 => Command::AddRow {
            section: s("JUNCTIONS"),
            fields: vec![format!("ZJ{i}"), s("1"), s("0"), s("0"), s("0"), s("0")],
            comment: Some(s("; added")),
        },
        6 => match rng.pick(&links) {
            Some(l) => doc.cascade_delete_link(l),
            None => Command::SetTitle {
                text: format!("title {i}"),
            },
        },
        7 => match rng.pick(&subs) {
            Some(sc) => Command::SetPolygon {
                subcatchment: sc.clone(),
                points: vec![(0.0, 0.0), (x, 0.0), (x, y)],
            },
            None => Command::SetOption {
                section: s("REPORT"),
                key: s("LINKS"),
                value: s("NONE"),
            },
        },
        8 => match rng.pick(&nodes) {
            Some(n) if nodes.len() > 3 => doc.cascade_delete_node(n),
            _ => Command::SetTitle {
                text: format!("title {i}"),
            },
        },
        9 => Command::SetOption {
            section: s("OPTIONS"),
            key: s("ROUTING_STEP"),
            value: format!("0:00:{:02}", i % 60),
        },
        10 => Command::Batch(vec![
            Command::AddRow {
                section: s("JUNCTIONS"),
                fields: vec![format!("ZB{i}"), s("2"), s("0")],
                comment: None,
            },
            Command::MoveNode {
                name: format!("ZB{i}"),
                x,
                y,
            },
            Command::AddRow {
                section: s("TAGS"),
                fields: vec![s("Node"), format!("ZB{i}"), s("batch")],
                comment: None,
            },
        ]),
        _ => match rng.pick(&subs) {
            Some(sc) => Command::Rename {
                kind: ObjectKind::Subcatchment,
                old: sc.clone(),
                new: format!("ZS{i}"),
            },
            None => Command::InsertText {
                section: s("OPTIONS"),
                line: None,
                text: s("; a note"),
            },
        },
    }
}

#[test]
fn fifty_random_commands_undo_to_the_original_and_redo_exactly() {
    for (name, original) in all_fixtures() {
        for seed in 1..=3u64 {
            let mut doc = InpDoc::parse(&original);
            let mut rng = Lcg(seed);
            let mut applied = 0;
            for i in 0..50 {
                let cmd = random_command(&doc, &mut rng, i);
                let before = doc.undo_depth();
                doc.apply(cmd.clone())
                    .unwrap_or_else(|e| panic!("{name} seed {seed} step {i}: {cmd:?}: {e}"));
                if doc.undo_depth() > before {
                    applied += 1;
                }
                // The document must still parse to itself at every step.
                let text = doc.to_string();
                assert_eq!(
                    InpDoc::parse(&text).to_string(),
                    text,
                    "{name} seed {seed} step {i}: edited text not stable"
                );
            }
            let edited = doc.to_string();
            assert_ne!(edited, original);
            let mut undone = 0;
            while doc.undo() {
                undone += 1;
            }
            assert_eq!(undone, applied, "{name} seed {seed}");
            assert_eq!(
                doc.to_string(),
                original,
                "{name} seed {seed}: undo did not restore the original"
            );
            let mut redone = 0;
            while doc.redo() {
                redone += 1;
            }
            assert_eq!(redone, applied);
            assert_eq!(
                doc.to_string(),
                edited,
                "{name} seed {seed}: redo did not reproduce the edit"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn samples_validate_without_errors() {
    for (name, text) in all_fixtures() {
        let doc = InpDoc::parse(&text);
        let errors: Vec<_> = doc
            .validate()
            .into_iter()
            .filter(|f| f.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
    }
}

#[test]
fn validation_finds_the_usual_mistakes() {
    let doc = InpDoc::parse(
        "[RAINGAGES]\nRG INTENSITY 0:05 1.0 TIMESERIES TS\n\
         [SUBCATCHMENTS]\nS1 RG J1 1 50 100 1 0\nS2 rg NOWHERE 1 50 100 1 0\nS3 RG2 S1 1 50 100 1 0\n\
         [JUNCTIONS]\nJ1 10 0\nj1 11 0\nJ2 9 0\n\
         [OUTFALLS]\nJ2 0 FREE NO\nO1 0 FREE NO\n\
         [CONDUITS]\nC1 J1 J2 100 0.013 0 0\nC2 J1 GHOST 100 0.013 0 0\nC3 J1\n\
         [XSECTIONS]\nC1 CIRCULAR 1 0 0 0 1\nC9 CIRCULAR 1 0 0 0 1\n\
         [COORDINATES]\nJ1 0 0\nJ1 1 1\nJ2 abc 0\nLOST 5 5\n\
         [VERTICES]\nC1 1 1\nCX 1 1\n[POLYGONS]\nS9 0 0\n[SYMBOLS]\nRGX 0 0\n",
    );
    let findings = doc.validate();
    let has = |section: &str, name: &str, needle: &str| {
        findings
            .iter()
            .any(|f| f.section == section && f.name == name && f.message.contains(needle))
    };
    assert!(has("JUNCTIONS", "j1", "duplicate"), "{findings:?}");
    assert!(has("OUTFALLS", "J2", "also defined in [JUNCTIONS]"));
    assert!(has("CONDUITS", "C2", "to node \"GHOST\" does not exist"));
    assert!(has("CONDUITS", "C3", "no to node"));
    assert!(has("CONDUITS", "C3", "fields"));
    assert!(has("CONDUITS", "C2", "no [XSECTIONS] row"));
    assert!(has("XSECTIONS", "C9", "does not exist"));
    assert!(has("SUBCATCHMENTS", "S2", "outlet \"NOWHERE\""));
    assert!(
        !has("SUBCATCHMENTS", "S3", "outlet"),
        "a subcatchment outlet is fine"
    );
    assert!(has("SUBCATCHMENTS", "S3", "rain gage \"RG2\""));
    assert!(
        !has("SUBCATCHMENTS", "S2", "rain gage"),
        "gage names are case-insensitive"
    );
    assert!(has("COORDINATES", "J1", "more than one"));
    assert!(has("COORDINATES", "J2", "not numbers"));
    assert!(has("COORDINATES", "LOST", "does not exist"));
    assert!(has("OUTFALLS", "O1", "no [COORDINATES]"));
    assert!(has("VERTICES", "CX", "does not exist"));
    assert!(has("POLYGONS", "S9", "does not exist"));
    assert!(has("SYMBOLS", "RGX", "does not exist"));
    assert!(!has("CONDUITS", "C1", "node"));
}
