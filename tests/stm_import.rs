// SPDX-License-Identifier: GPL-3.0-or-later

//! Import the shipped Hydraflow `.stm` sample and confirm it becomes a correct,
//! analyzable network — the "import your existing project" path end-to-end.

use std::path::PathBuf;

use stormsewer::io::import_stm;
use stormsewer::network::AnalysisOptions;
use stormsewer::IdfCurve;

fn sample_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hydraflow-sample.stm")
}

#[test]
fn imports_hydraflow_sample_to_correct_topology() {
    let project = import_stm(&sample_path()).expect("import sample .stm");

    assert_eq!(project.name, "Riverside Estates - Storm Trunk");
    assert_eq!(project.pipes.len(), 4, "four lines → four pipes");

    // Two named catch-basin inlets, at least one outfall.
    let inlets: Vec<&str> = project
        .nodes
        .iter()
        .filter(|n| n.kind == "inlet")
        .map(|n| n.id.as_str())
        .collect();
    assert!(inlets.contains(&"CB-1"), "CB-1 inlet present, got {inlets:?}");
    assert!(inlets.contains(&"CB-2"), "CB-2 inlet present, got {inlets:?}");
    assert!(
        project.nodes.iter().any(|n| n.kind == "outfall"),
        "an outfall was inferred from the downstream-0 line"
    );

    // Return-Period Index 4 → 10-year design storm; IDF curves carried over.
    assert!((project.design_return_period_years - 10.0).abs() < 1e-6);
    assert!(!project.idf_curves.is_empty(), "IDF curves imported");
}

#[test]
fn imported_sample_analyzes() {
    let project = import_stm(&sample_path()).expect("import sample .stm");
    let net = project.to_network();
    let idf = IdfCurve::new(project.idf_a, project.idf_b, project.idf_c);

    let analysis = net
        .analyze(&idf, &AnalysisOptions::default())
        .expect("imported network analyzes");

    assert_eq!(analysis.pipes.len(), 4);
    // Flow accumulates downstream: the trunk carries more than either lateral.
    let q = |id: &str| analysis.pipes.iter().find(|p| p.id == id).unwrap().design_q;
    assert!(q("P4") >= q("P1"), "trunk P4 {} >= lateral P1 {}", q("P4"), q("P1"));
    assert!(q("P4") > 0.0);
}
