// SPDX-License-Identifier: GPL-3.0-or-later

//! Real Civil 3D export formats, checked against the numbers Autodesk's own
//! Hydraflow Storm Sewers printed for the same network.
//!
//! `tests/fixtures/civil3d-2015-storm-sewers.stm` is a "Storm Sewers for
//! AutoCAD Civil 3D 2015" project file for a four-line trunk (18/18/18/12 in,
//! 0.5 % grades, 10-yr Atlas 14 IDF, starting HGL 757.365), with its
//! coordinates moved to a local origin. The reference values below are read
//! from the Storm Sewer Tabulation / HGL Computations pages of the report
//! Hydraflow Storm Sewers v2026.00 printed for that file on 2026-08-16.
//!
//! `tests/fixtures/civil3d-2026-pipenetwork.xml` is the LandXML 1.2 export of
//! the same run from Civil 3D 2026 (structure data in attributes, per-pipe
//! `<Invert>` records, a "null structure" outfall).
//!
//! Before this suite, the STM importer rejected every Civil 3D file outright
//! ("not a Hydraflow Storm Sewers STM file"), and when the signature check was
//! bypassed it read the gutter n as the pipe n, dropped the tailwater, and
//! re-sloped every pipe to the structure inverts (no drops).

use std::path::PathBuf;

use stormsewer::io::{export_landxml, import_landxml, import_stm};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn approx(a: f64, b: f64, tol: f64, what: &str) {
    assert!(
        (a - b).abs() <= tol,
        "{what}: got {a}, expected {b} ± {tol}"
    );
}

#[test]
fn civil3d_2015_stm_imports_what_hydraflow_wrote() {
    let p = import_stm(&fixture("civil3d-2015-storm-sewers.stm"))
        .expect("Civil 3D 2015 'Storm Sewers for AutoCAD' file must import");

    assert_eq!(p.pipes.len(), 4);
    // Return Period Index in the Civil 3D format is the period itself (10 yr),
    // and the one populated IDF slot is the 10-yr column.
    approx(p.design_return_period_years, 10.0, 1e-9, "design storm");
    approx(p.idf_a, 62.50296, 1e-5, "IDF a");
    approx(p.idf_b, 8.799997, 1e-6, "IDF b");
    approx(p.idf_c, 0.7942153, 1e-7, "IDF c");
    approx(p.min_tc, 5.0, 1e-9, "minimum Tc");
    // "Starting HGL" is the outfall tailwater.
    approx(
        p.tailwater.expect("starting HGL becomes tailwater"),
        757.365,
        1e-9,
        "tailwater",
    );
    // Three lines at K = 0.5, the terminal inlet at 1.0 → project K is the mode.
    approx(p.junction_k, 0.5, 1e-9, "junction K");

    // Every line says N-Value = 0.012; the "Gutter N-Value = 0.013" that
    // follows it in each block must not win.
    for pipe in &p.pipes {
        approx(pipe.n, 0.012, 1e-9, &format!("{} Manning n", pipe.id));
    }
    // Rise/Span are feet in this format: 1.5 → 18 in, 1.0 → 12 in.
    let dia = |id: &str| p.pipes.iter().find(|x| x.id == id).unwrap().diameter;
    approx(dia("P1"), 1.5, 1e-9, "P1 diameter ft");
    approx(dia("P4"), 1.0, 1e-9, "P4 diameter ft");

    // Inlets keep their Hydraflow IDs; the outfall has no rim in the file
    // (0) and must not read as 756 ft of negative freeboard.
    for id in ["AI-1", "AI-2", "AI-3", "AI-4"] {
        let n = p
            .nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("{id} present"));
        assert_eq!(n.kind, "inlet");
        approx(n.tc_inlet, 5.0, 1e-9, &format!("{id} inlet time"));
    }
    let out = p
        .nodes
        .iter()
        .find(|n| n.kind == "outfall")
        .expect("outfall");
    approx(out.invert, 756.0, 1e-9, "outfall invert");
    assert!(
        out.rim >= out.invert,
        "outfall rim {} below invert {}",
        out.rim,
        out.invert
    );

    // Line 2 enters AI-4 at 757.03 while AI-4's outlet invert is 756.93: a
    // 0.10 ft drop through the structure, kept as the pipe's own invert.
    let p2 = p.pipes.iter().find(|x| x.id == "P2").unwrap();
    approx(
        p2.invert_dn.expect("P2 keeps its downstream invert"),
        757.03,
        1e-9,
        "P2 invert dn",
    );
    assert!(
        p2.invert_up.is_none(),
        "P2 upstream invert equals the AI-3 invert"
    );
}

#[test]
fn civil3d_2015_stm_analysis_matches_hydraflow_report() {
    let p = import_stm(&fixture("civil3d-2015-storm-sewers.stm")).unwrap();
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    let net = p.to_analysis_network();
    let idf = p.idf_set();
    let a = net
        .analyze(idf.design_curve(), &p.options())
        .expect("analyzes");
    let res = |id: &str| a.pipes.iter().find(|x| x.id == id).unwrap();

    // Storm Sewer Tabulation, lines 1-4 (downstream first).
    // Slope: Hydraflow computes it from the line's own inverts.
    approx(res("P1").slope, 0.00500, 2e-5, "P1 slope");
    approx(
        res("P2").slope,
        0.00497,
        2e-5,
        "P2 slope (with the structure drop)",
    );
    approx(res("P3").slope, 0.00497, 2e-5, "P3 slope");
    approx(res("P4").slope, 0.00502, 2e-5, "P4 slope");

    // Area × C accumulation, acres.
    approx(res("P1").total_ca, 1.50, 5e-3, "P1 total CxA");
    approx(res("P2").total_ca, 1.09, 5e-3, "P2 total CxA");
    approx(res("P3").total_ca, 0.71, 5e-3, "P3 total CxA");
    approx(res("P4").total_ca, 0.38, 5e-3, "P4 total CxA");

    // Full-flow capacity (cfs). Hydraflow uses 1.486 where we use 1.49 — 0.3 %.
    approx(res("P1").capacity, 8.04, 0.05, "P1 capacity");
    approx(res("P2").capacity, 8.02, 0.05, "P2 capacity");
    approx(res("P4").capacity, 2.73, 0.02, "P4 capacity");

    // Design flow (cfs). The top two lines have no upstream travel-time
    // dependence and must match to the reported precision; the lower two
    // differ by the travel-time method (Hydraflow uses the backwater depth,
    // this engine the normal depth) — held to 3 %.
    approx(res("P4").design_q, 2.97, 0.01, "P4 Q");
    approx(res("P3").design_q, 5.28, 0.01, "P3 Q");
    approx(res("P2").design_q, 7.68, 7.68 * 0.03, "P2 Q");
    approx(res("P1").design_q, 10.23, 10.23 * 0.03, "P1 Q");
    approx(res("P4").intensity, 7.77, 0.01, "P4 intensity");

    // Hydraflow flags lines 1 and 4 surcharged (Q > capacity), 2 and 3 not.
    assert!(res("P1").surcharged && res("P4").surcharged);
    assert!(!res("P2").surcharged && !res("P3").surcharged);

    // HGL: outfall sits at the starting HGL; the HGL at AI-4 (line 1 upstream
    // junction, "HGL Junct" 759.08) within the surcharged-reach method gap.
    approx(
        res("P1").hgl_dn.unwrap(),
        757.365,
        1e-6,
        "outfall HGL = tailwater",
    );
    let hgl = |id: &str| a.nodes.iter().find(|n| n.id == id).unwrap().hgl;
    approx(hgl("AI-4"), 759.08, 0.35, "HGL at AI-4");
    approx(hgl("AI-3"), 760.03, 0.40, "HGL at AI-3");
}

#[test]
fn civil3d_2026_landxml_imports_attributes_inverts_and_outfall() {
    let p = import_landxml(&fixture("civil3d-2026-pipenetwork.xml"))
        .expect("Civil 3D 2026 LandXML must import");

    assert_eq!(p.pipes.len(), 4);
    assert_eq!(p.nodes.len(), 5);

    // Civil 3D names: "AI-1 (Storm Sewer)" → AI-1, "Pipe - (2) (Storm Sewer)" → P2.
    let node = |id: &str| {
        p.nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("{id} present"))
    };
    let pipe = |id: &str| {
        p.pipes
            .iter()
            .find(|x| x.id == id)
            .unwrap_or_else(|| panic!("{id} present"))
    };

    // Rim and invert come from elevRim / elevSump attributes, not element text.
    approx(node("AI-1").rim, 762.5167, 1e-3, "AI-1 rim");
    approx(node("AI-1").invert, 759.8457, 1e-3, "AI-1 invert");
    approx(node("AI-4").invert, 756.9303, 1e-3, "AI-4 invert (sump)");
    assert_eq!(
        node("AI-1").kind,
        "inlet",
        "slab-top frame structure is an inlet"
    );

    // The dummy null structure that only receives flow is the outfall.
    let out = p
        .nodes
        .iter()
        .find(|n| n.kind == "outfall")
        .expect("outfall inferred");
    approx(out.invert, 756.0, 1e-9, "outfall invert from its <Invert>");
    assert_eq!(pipe("P5").to, out.id);

    // Diameters in inches → feet; length from the attribute.
    approx(pipe("P2").diameter, 1.0, 1e-9, "P2 diameter");
    approx(pipe("P3").diameter, 1.5, 1e-9, "P3 diameter");
    approx(pipe("P5").length, 186.0721, 1e-3, "P5 length attribute");

    // Per-pipe <Invert flowDir="in"> that differs from the structure invert
    // becomes the pipe's own downstream invert (drops through AI-2/3/4).
    approx(
        pipe("P2").invert_dn.expect("P2 drop"),
        758.97,
        1e-9,
        "P2 invert dn",
    );
    approx(
        pipe("P4").invert_dn.expect("P4 drop"),
        757.03,
        1e-9,
        "P4 invert dn",
    );
    assert!(
        pipe("P2").invert_up.is_none(),
        "upstream invert equals the AI-1 sump"
    );

    // Concrete pipe → n = 0.013.
    approx(pipe("P2").n, 0.013, 1e-9, "RCP n");
}

#[test]
fn landxml_and_stm_exports_of_one_network_agree() {
    let stm = import_stm(&fixture("civil3d-2015-storm-sewers.stm")).unwrap();
    let xml = import_landxml(&fixture("civil3d-2026-pipenetwork.xml")).unwrap();

    for id in ["AI-1", "AI-2", "AI-3", "AI-4"] {
        let a = stm.nodes.iter().find(|n| n.id == id).unwrap();
        let b = xml.nodes.iter().find(|n| n.id == id).unwrap();
        approx(a.invert, b.invert, 0.01, &format!("{id} invert"));
        // Rims are not compared: the STM was saved after AI-4's rim was
        // edited (759.00 vs 759.53 in the LandXML) — source data, not import.
        approx(a.x, b.x, 0.01, &format!("{id} x"));
        approx(a.y, b.y, 0.01, &format!("{id} y"));
    }
    // STM numbers lines from the outfall up (P1 = outfall pipe); LandXML keeps
    // Civil 3D's names (Pipe - (5) is the outfall pipe).
    for (s, x) in [("P1", "P5"), ("P2", "P4"), ("P3", "P3"), ("P4", "P2")] {
        let a = stm.pipes.iter().find(|p| p.id == s).unwrap();
        let b = xml.pipes.iter().find(|p| p.id == x).unwrap();
        approx(a.length, b.length, 0.01, &format!("{s}/{x} length"));
        approx(a.diameter, b.diameter, 1e-9, &format!("{s}/{x} diameter"));
        let net_a = stm.to_network();
        let net_b = xml.to_network();
        let ia = net_a.pipes.iter().position(|p| p.id == s).unwrap();
        let ib = net_b.pipes.iter().position(|p| p.id == x).unwrap();
        let ua = net_a.nodes.iter().position(|n| n.id == a.from).unwrap();
        let da = net_a.nodes.iter().position(|n| n.id == a.to).unwrap();
        let ub = net_b.nodes.iter().position(|n| n.id == b.from).unwrap();
        let db = net_b.nodes.iter().position(|n| n.id == b.to).unwrap();
        let (au, ad) = net_a.pipe_inverts(ia, ua, da);
        let (bu, bd) = net_b.pipe_inverts(ib, ub, db);
        approx(au, bu, 0.01, &format!("{s}/{x} invert up"));
        approx(ad, bd, 0.01, &format!("{s}/{x} invert dn"));
    }
}

#[test]
fn standalone_hydraflow_sample_still_reads_inches() {
    // The pre-existing synthetic sample ("Hydraflow Storm Sewers 2008", Rise
    // in inches, Return Period Index = slot) must keep importing as before.
    let p = import_stm(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hydraflow-sample.stm"),
    )
    .unwrap();
    approx(p.design_return_period_years, 10.0, 1e-9, "slot 4 → 10 yr");
    let d15 = p
        .pipes
        .iter()
        .map(|x| x.diameter)
        .fold(f64::INFINITY, f64::min);
    approx(d15, 1.25, 1e-9, "15 in → 1.25 ft");
}

#[test]
fn profile_draws_the_drops_through_structures() {
    use stormsewer::drawing::{draw_network, DrawConfig, ProfileRole};

    let p = import_stm(&fixture("civil3d-2015-storm-sewers.stm")).unwrap();
    let net = p.to_analysis_network();
    let a = net
        .analyze(p.idf_set().design_curve(), &p.options())
        .unwrap();
    // Unit config: drawing y == elevation - datum, x == station.
    let cfg = DrawConfig {
        profile_origin_x: 0.0,
        profile_origin_y: 0.0,
        h_scale: 1.0,
        v_exag: 1.0,
        ..DrawConfig::default()
    };
    let d = draw_network(&net, &a, &cfg);
    let invert = d
        .profile_lines
        .iter()
        .find(|l| l.role == ProfileRole::Invert)
        .expect("invert polyline");
    // Three 0.10 ft drops (AI-2, AI-3, AI-4): each is a vertical step in the
    // invert line — two consecutive points at one station, 0.10 ft apart.
    let steps: Vec<f64> = invert
        .pts
        .windows(2)
        .filter(|w| (w[0].0 - w[1].0).abs() < 1e-9)
        .map(|w| (w[0].1 - w[1].1).abs())
        .collect();
    assert_eq!(
        steps.len(),
        3,
        "expected three structure drops, got {steps:?} from {:?}",
        invert.pts
    );
    for s in &steps {
        approx(*s, 0.10, 1e-6, "drop height");
    }
    // The line still starts at the top inlet's invert and ends at the outfall.
    let datum = d.profile_datum;
    approx(
        invert.pts.first().unwrap().1 + datum,
        759.85,
        1e-6,
        "profile starts at AI-1 invert",
    );
    approx(
        invert.pts.last().unwrap().1 + datum,
        756.0,
        1e-6,
        "profile ends at the outfall invert",
    );
}

/// The LandXML this program writes has to be a file Civil 3D will take back.
///
/// Getting a network out of Civil 3D and into here was only ever half the
/// trip. The way back is Civil 3D's own LandXML pipe network import, and that
/// import is fussy about things a round-trip through this program's own reader
/// would never catch, because its reader wrote the file.
///
/// Each assertion below is one way a real Civil 3D 2026 export differs from
/// what this program used to emit.
#[test]
fn exported_landxml_is_shaped_like_a_civil3d_file() {
    let project = import_stm(&fixture("civil3d-2015-storm-sewers.stm")).expect("import stm");

    let dir = std::env::temp_dir().join("stormsewer-landxml-shape");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("out.xml");
    export_landxml(&project, &path).expect("export landxml");
    let xml = std::fs::read_to_string(&path).expect("read back");

    // pipeNetType is how Civil 3D picks the parts list. Without it a storm
    // network can come in as sanitary and every part is the wrong family.
    assert!(
        xml.contains(r#"pipeNetType="storm""#),
        "PipeNetwork must declare pipeNetType, got:\n{xml}"
    );

    // <ElevRim> is not a LandXML 1.2 element. A validating consumer can reject
    // the whole file over it, and the elevRim attribute already carries it.
    assert!(
        !xml.contains("<ElevRim>"),
        "ElevRim is not in the schema; the elevRim attribute is the right place"
    );
    assert!(
        xml.contains("elevRim="),
        "structures must still carry their rim as an attribute"
    );

    // Civil 3D writes both, and reads them back.
    assert!(xml.contains("<Project name="), "missing <Project>");
    assert!(xml.contains("<Application name=\"StormSewer\""), "missing <Application>");

    // desc is where Civil 3D records what a structure is. role is this
    // program's own attribute and a third party will ignore it.
    assert!(
        xml.contains(r#"desc="Outfall structure""#),
        "the outfall must be described, not only role-tagged"
    );

    // And it still round-trips through this program unchanged, which is the
    // property the export existed for in the first place.
    let back = import_landxml(&path).expect("re-import");
    assert_eq!(
        back.pipes.len(),
        project.pipes.len(),
        "pipe count survives the round trip"
    );
    assert_eq!(
        back.nodes.len(),
        project.nodes.len(),
        "node count survives the round trip"
    );

    let _ = std::fs::remove_file(&path);
}
