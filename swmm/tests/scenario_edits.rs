// SPDX-License-Identifier: GPL-3.0-or-later

//! Scenarios: the edit mirror round-trips every command variant through
//! serde and back, apply/materialize leave the base alone, stale edits are
//! named, the base-vs-current diff reproduces the current rows, the
//! sidecar loads and saves, and a report's numbers tabulate.

use std::path::PathBuf;

use stormsewer_swmm::doc::{Command, InpDoc, ObjectKind};
use stormsewer_swmm::scenario::{
    self, Scenario, ScenarioEdit, ScenarioResult, ScenarioSet,
};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn pond() -> InpDoc {
    InpDoc::read(&fixture("epa-samples/Detention_Pond_Model.inp")).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("stormsewer-scenario-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// One command of every variant.
fn every_command() -> Vec<Command> {
    vec![
        Command::SetField {
            section: "SUBCATCHMENTS".into(),
            name: "S1".into(),
            field: "PctImperv".into(),
            value: "60".into(),
        },
        Command::SetFields {
            section: "JUNCTIONS".into(),
            name: "J1".into(),
            fields: vec!["J1".into(), "4973".into(), "5".into(), "0".into(), "0".into(), "0".into()],
        },
        Command::SetLine {
            section: "OPTIONS".into(),
            line: 3,
            fields: vec!["FLOW_ROUTING".into(), "DYNWAVE".into()],
            comment: Some("; changed".into()),
        },
        Command::SetText {
            section: "TITLE".into(),
            line: 1,
            text: "A scenario".into(),
        },
        Command::AddRow {
            section: "JUNCTIONS".into(),
            fields: vec!["J99".into(), "4900".into(), "0".into(), "0".into(), "0".into(), "0".into()],
            comment: None,
        },
        Command::InsertText {
            section: "CONTROLS".into(),
            line: None,
            text: "RULE R1".into(),
        },
        Command::DeleteLine {
            section: "REPORT".into(),
            line: 2,
        },
        Command::DeleteObject {
            section: "CONDUITS".into(),
            name: "C1".into(),
        },
        Command::DeleteTag {
            kind: "Node".into(),
            name: "J1".into(),
        },
        Command::Rename {
            kind: ObjectKind::Node,
            old: "J2".into(),
            new: "J2b".into(),
        },
        Command::MoveNode {
            name: "J3".into(),
            x: 1.5,
            y: -2.25,
        },
        Command::MoveGage {
            name: "RainGage".into(),
            x: 10.0,
            y: 20.0,
        },
        Command::SetVertices {
            link: "C2".into(),
            points: vec![(1.0, 2.0), (3.0, 4.0)],
        },
        Command::SetPolygon {
            subcatchment: "S2".into(),
            points: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
        },
        Command::SetOption {
            section: "OPTIONS".into(),
            key: "ROUTING_STEP".into(),
            value: "0:00:05".into(),
        },
        Command::SetTitle {
            text: "Line one\nLine two".into(),
        },
        Command::Batch(vec![
            Command::SetOption {
                section: "OPTIONS".into(),
                key: "THREADS".into(),
                value: "4".into(),
            },
            Command::DeleteObject {
                section: "WEIRS".into(),
                name: "nothing".into(),
            },
        ]),
    ]
}

#[test]
fn every_command_variant_round_trips_through_the_edit_mirror_and_json() {
    let cmds = every_command();
    assert_eq!(cmds.len(), 17, "one per Command variant");
    for cmd in &cmds {
        let edit = ScenarioEdit::from_command(cmd);
        let back = edit.to_command().unwrap();
        assert_eq!(&back, cmd, "{edit:?}");
        let json = serde_json::to_string(&edit).unwrap();
        assert!(json.contains("\"op\""), "{json}");
        let parsed: ScenarioEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, edit);
        assert_eq!(&parsed.to_command().unwrap(), cmd);
        assert!(!edit.describe().is_empty());
    }
    // A whole scenario, through the set.
    let s = Scenario::from_commands("all", &cmds);
    let mut set = ScenarioSet::default();
    set.add(s.clone());
    let json = serde_json::to_string_pretty(&set).unwrap();
    let back: ScenarioSet = serde_json::from_str(&json).unwrap();
    assert_eq!(back, set);
    assert_eq!(back.scenarios[0].commands().unwrap(), cmds);
    // An unknown rename kind is the one thing that cannot map back.
    let bad = ScenarioEdit::Rename {
        kind: "planet".into(),
        old: "a".into(),
        new: "b".into(),
    };
    assert!(bad.to_command().is_err());
}

#[test]
fn apply_and_materialize_leave_the_base_untouched() {
    let base = pond();
    let before = base.to_string();
    let s = Scenario::from_commands(
        "wider",
        &[
            Command::SetField {
                section: "SUBCATCHMENTS".into(),
                name: "S1".into(),
                field: "Width".into(),
                value: "2000".into(),
            },
            Command::SetOption {
                section: "OPTIONS".into(),
                key: "FLOW_ROUTING".into(),
                value: "DYNWAVE".into(),
            },
        ],
    );
    let doc = scenario::apply(&base, &s).unwrap();
    assert_eq!(doc.field("SUBCATCHMENTS", "S1", "Width"), Some("2000"));
    assert_eq!(doc.option("FLOW_ROUTING"), Some("DYNWAVE"));
    assert_eq!(base.to_string(), before, "the base is not edited");
    assert_eq!(base.field("SUBCATCHMENTS", "S1", "Width"), Some("1587"));
    let text = scenario::materialize(&base, &s).unwrap();
    assert_eq!(text, doc.to_string());
    assert!(text.contains("DYNWAVE"));
    // The base's own generation is untouched — nothing was applied to it.
    assert_eq!(base.generation(), 0);
    // A failing edit names itself.
    let broken = Scenario::from_commands(
        "broken",
        &[Command::SetField {
            section: "SUBCATCHMENTS".into(),
            name: "NOPE".into(),
            field: "Width".into(),
            value: "1".into(),
        }],
    );
    let err = scenario::apply(&base, &broken).unwrap_err().to_string();
    assert!(err.contains("edit 1") && err.contains("NOPE"), "{err}");
}

#[test]
fn stale_edits_are_reported_and_the_rest_still_judged() {
    let base = pond();
    let s = Scenario::from_commands(
        "stale",
        &[
            Command::SetField {
                section: "SUBCATCHMENTS".into(),
                name: "S1".into(),
                field: "Width".into(),
                value: "2000".into(),
            },
            // S99 never existed.
            Command::SetField {
                section: "SUBCATCHMENTS".into(),
                name: "S99".into(),
                field: "Width".into(),
                value: "2000".into(),
            },
            // C1 was deleted from the base.
            Command::DeleteObject {
                section: "CONDUITS".into(),
                name: "Cgone".into(),
            },
            Command::MoveNode {
                name: "Jgone".into(),
                x: 0.0,
                y: 0.0,
            },
            Command::Rename {
                kind: ObjectKind::Link,
                old: "Cgone".into(),
                new: "Cnew".into(),
            },
            Command::SetLine {
                section: "OPTIONS".into(),
                line: 9999,
                fields: vec!["X".into()],
                comment: None,
            },
            // Fine again, after the stale ones.
            Command::SetOption {
                section: "OPTIONS".into(),
                key: "THREADS".into(),
                value: "2".into(),
            },
            Command::SetVertices {
                link: "C2".into(),
                points: vec![(1.0, 1.0)],
            },
        ],
    );
    let stale = scenario::validate(&base, &s);
    let idx: Vec<usize> = stale.iter().map(|e| e.index).collect();
    assert_eq!(idx, vec![1, 2, 3, 4, 5], "{stale:?}");
    assert!(stale[0].reason.contains("S99"));
    assert!(stale[1].reason.contains("Cgone"));
    assert!(stale[2].reason.contains("node"));
    assert!(stale[4].reason.contains("9999") || stale[4].reason.contains("line"));
    assert!(scenario::validate(&base, &Scenario::new("empty")).is_empty());
}

/// Every named section's rows, keyed by section.
fn rows_of(doc: &InpDoc) -> Vec<(String, Vec<Vec<String>>)> {
    doc.sections()
        .iter()
        .map(|s| {
            let rows: Vec<Vec<String>> = s.rows().map(|(_, r)| r.fields.clone()).collect();
            (s.name.clone(), rows)
        })
        .filter(|(_, r)| !r.is_empty())
        .collect()
}

/// Rows per section as a sorted set (the diff may re-order rows within a
/// section, which the engine does not care about).
fn row_sets(doc: &InpDoc) -> Vec<(String, Vec<Vec<String>>)> {
    let mut out = rows_of(doc);
    for (_, rows) in &mut out {
        rows.sort();
    }
    out.sort();
    out
}

#[test]
fn diff_between_base_and_edited_copy_reproduces_the_edited_rows() {
    let base = pond();
    let mut cur = scenario::clone_doc(&base);
    for cmd in [
        Command::SetField {
            section: "SUBCATCHMENTS".into(),
            name: "S1".into(),
            field: "PctImperv".into(),
            value: "70".into(),
        },
        Command::MoveNode {
            name: "J1".into(),
            x: 100.0,
            y: 200.0,
        },
        Command::MoveGage {
            name: "RainGage".into(),
            x: 5.0,
            y: 6.0,
        },
        Command::SetVertices {
            link: "C1".into(),
            points: vec![(1.0, 2.0), (3.0, 4.0)],
        },
        Command::SetPolygon {
            subcatchment: "S7".into(),
            points: vec![],
        },
        Command::AddRow {
            section: "JUNCTIONS".into(),
            fields: vec!["Jnew".into(), "4950".into(), "0".into(), "0".into(), "0".into(), "0".into()],
            comment: None,
        },
        Command::MoveNode {
            name: "Jnew".into(),
            x: 1.0,
            y: 1.0,
        },
        Command::DeleteObject {
            section: "CONDUITS".into(),
            name: "C2".into(),
        },
        Command::DeleteObject {
            section: "XSECTIONS".into(),
            name: "C2".into(),
        },
        Command::SetOption {
            section: "OPTIONS".into(),
            key: "FLOW_ROUTING".into(),
            value: "DYNWAVE".into(),
        },
        Command::SetOption {
            section: "OPTIONS".into(),
            key: "NEW_KEY".into(),
            value: "1".into(),
        },
        Command::SetTitle {
            text: "New title".into(),
        },
        Command::InsertText {
            section: "CONTROLS".into(),
            line: None,
            text: "RULE R1".into(),
        },
        Command::InsertText {
            section: "CONTROLS".into(),
            line: None,
            text: "IF NODE J1 DEPTH > 1".into(),
        },
        // A curve (multi-row group) gains a point.
        Command::AddRow {
            section: "CURVES".into(),
            fields: vec!["Storage".into(), "12".into(), "999".into()],
            comment: None,
        },
        // The pond tags its links; C1 is "Swale".
        Command::DeleteTag {
            kind: "Link".into(),
            name: "C1".into(),
        },
    ] {
        cur.apply(cmd).unwrap();
    }
    let edits = scenario::diff(&base, &cur);
    assert!(!edits.is_empty());
    // The edit kinds we expect to see.
    let has = |f: &dyn Fn(&ScenarioEdit) -> bool| edits.iter().any(f);
    assert!(has(&|e| matches!(e, ScenarioEdit::SetFields { section, name, .. } if section == "SUBCATCHMENTS" && name == "S1")));
    assert!(has(&|e| matches!(e, ScenarioEdit::MoveNode { name, x, y } if name == "J1" && *x == 100.0 && *y == 200.0)));
    assert!(has(&|e| matches!(e, ScenarioEdit::MoveGage { name, .. } if name == "RainGage")));
    assert!(has(&|e| matches!(e, ScenarioEdit::SetVertices { link, points } if link == "C1" && points.len() == 2)));
    assert!(has(&|e| matches!(e, ScenarioEdit::SetPolygon { subcatchment, points } if subcatchment == "S7" && points.is_empty())));
    assert!(has(&|e| matches!(e, ScenarioEdit::AddRow { section, fields, .. } if section == "JUNCTIONS" && fields[0] == "Jnew")));
    assert!(has(&|e| matches!(e, ScenarioEdit::DeleteObject { section, name } if section == "CONDUITS" && name == "C2")));
    assert!(has(&|e| matches!(e, ScenarioEdit::SetTitle { text } if text == "New title")));
    assert!(has(&|e| matches!(e, ScenarioEdit::InsertText { section, text, .. } if section == "CONTROLS" && text == "RULE R1")));
    assert!(has(&|e| matches!(e, ScenarioEdit::DeleteTag { kind, name } if kind.eq_ignore_ascii_case("Link") && name == "C1")));
    // Applying the diff to a fresh base gives the edited rows.
    let s = Scenario {
        name: "diff".into(),
        description: String::new(),
        edits: edits.clone(),
    };
    assert!(scenario::validate(&base, &s).is_empty(), "{:?}", scenario::validate(&base, &s));
    let rebuilt = scenario::apply(&base, &s).unwrap();
    assert_eq!(row_sets(&rebuilt), row_sets(&cur));
    assert_eq!(rebuilt.title(), "New title");
    assert_eq!(rebuilt.coordinates("J1"), Some((100.0, 200.0)));
    assert_eq!(rebuilt.vertices("C1"), vec![(1.0, 2.0), (3.0, 4.0)]);
    assert!(rebuilt.polygon("S7").is_empty());
    assert_eq!(rebuilt.tag("Link", "C1"), None);
    assert_eq!(rebuilt.tag("Link", "C2"), Some("Gutter"));
    // No change, no edits.
    assert!(scenario::diff(&base, &scenario::clone_doc(&base)).is_empty());
    // The diff serialises.
    let json = serde_json::to_string(&s).unwrap();
    let back: Scenario = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn scenario_set_loads_saves_names_and_edits() {
    let model = scratch("Pond.inp");
    std::fs::write(&model, "[TITLE]\n").unwrap();
    let side = ScenarioSet::sidecar_path(&model);
    assert_eq!(side.file_name().unwrap(), "Pond.scenarios.json");
    let _ = std::fs::remove_file(&side);
    // Missing sidecar: empty set.
    let set = ScenarioSet::load_for(&model).unwrap();
    assert!(set.scenarios.is_empty());
    let mut set = set;
    assert_eq!(set.add(Scenario::new("A")), 0);
    assert_eq!(set.add(Scenario::new("A")), 1, "duplicate names are numbered");
    assert_eq!(set.scenarios[1].name, "A 2");
    assert_eq!(set.add(Scenario::new("")), 2);
    assert_eq!(set.scenarios[2].name, "Scenario");
    assert_eq!(set.duplicate(0), Some(3));
    assert_eq!(set.scenarios[3].name, "A copy");
    assert!(set.rename(3, "a").is_err(), "case-insensitive clash");
    assert!(set.rename(3, "").is_err());
    set.rename(3, "B").unwrap();
    assert!(set.contains("b"));
    set.save_for(&model).unwrap();
    let back = ScenarioSet::load_for(&model).unwrap();
    assert_eq!(back, set);
    assert_eq!(back.version, scenario::FORMAT_VERSION);
    let mut back = back;
    assert!(back.remove(9).is_none());
    assert_eq!(back.remove(0).unwrap().name, "A");
    assert_eq!(back.scenarios.len(), 3);
    // A corrupt sidecar is an error naming the file.
    std::fs::write(&side, "{not json").unwrap();
    let err = ScenarioSet::load(&side).unwrap_err().to_string();
    assert!(err.contains("Pond.scenarios.json"), "{err}");
    let _ = std::fs::remove_file(&side);
    // The synthetic per-scenario model path sits beside the model.
    let p = scenario::scenario_model_path(&model, "Big pipes/2");
    assert_eq!(p.parent(), model.parent());
    assert_eq!(p.file_name().unwrap(), "Pond.scenario.Big_pipes_2.inp");
}

#[test]
fn scenario_results_read_the_report_and_export_csv() {
    let report = stormsewer_swmm::rpt::read(&fixture("results/Detention_Pond_Model.rpt")).unwrap();
    let r = ScenarioResult::from_report("base", &report, true);
    assert!(r.succeeded);
    assert!((r.peak_outfall_flow.unwrap() - 3.72).abs() < 1e-9);
    assert!((r.outfall_volume.unwrap() - 0.406).abs() < 1e-9);
    assert_eq!(r.flooding_volume, Some(0.0), "no nodes flooded");
    assert!((r.runoff_continuity_pct.unwrap() + 0.023).abs() < 1e-9);
    assert!((r.routing_continuity_pct.unwrap() - 0.092).abs() < 1e-9);
    let f = ScenarioResult::failed("bad", "ERROR 200");
    assert!(!f.succeeded && f.peak_outfall_flow.is_none());
    let csv = scenario::results_csv(&[r.clone(), f]);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("Scenario,Succeeded,Peak outfall flow"));
    assert!(lines[1].starts_with("base,yes,3.720,0.4060,0.0000,-0.023,0.092,"));
    assert!(lines[2].starts_with("bad,no,,,,,,0,ERROR 200"));
    let json = serde_json::to_string(&r).unwrap();
    let back: ScenarioResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
}
