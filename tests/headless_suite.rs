//! Headless integration tests — full engine workflows without GUI or CAD.

use std::path::PathBuf;

use stormsewer::catchment::{point_in_polygon, shoelace_area_sqft, sqft_to_acres};
use stormsewer::design::{
    apply_sizing_to_network, design_review, recommend_all_pipes, DesignCriteria, ReviewCriteria,
};
use stormsewer::diagnostics::run_diagnostics;
use stormsewer::hydrology::{faa_minutes, tr55_sheet_flow_minutes};
use stormsewer::io::{export_dxf, export_pdf, import_dxf, Project, ProjectCatchment};
use stormsewer::parse::parse_ssn;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn examples_dir() -> PathBuf {
    manifest_dir().join("examples")
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("stormsewer_headless_{name}"))
}

#[test]
fn demo_project_peak_flow_on_p3() {
    let project = Project::demo();
    let net = project.to_network();
    let analysis = net
        .analyze(&project.idf(), &project.options())
        .expect("demo should analyze");
    let p3 = analysis.pipes.iter().find(|p| p.id == "P3").expect("P3");
    assert!(
        (p3.design_q - 8.48).abs() < 0.05,
        "P3 design Q expected ~8.48 cfs, got {:.3}",
        p3.design_q
    );
    assert!(!p3.surcharged);
}

#[test]
fn investor_demo_ssproj_loads_and_matches_engine_demo() {
    let path = examples_dir().join("investor-demo.ssproj");
    if !path.exists() {
        eprintln!("skip: run `cargo run --example export_demo` to create investor-demo.ssproj");
        return;
    }
    let loaded = Project::load(&path).expect("load investor demo");
    let builtin = Project::demo();
    let loaded_net = loaded.to_network();
    let builtin_net = builtin.to_network();
    let loaded_a = loaded_net
        .analyze(&loaded.idf(), &loaded.options())
        .expect("loaded analyze");
    let builtin_a = builtin_net
        .analyze(&builtin.idf(), &builtin.options())
        .expect("builtin analyze");
    assert_eq!(loaded_a.pipes.len(), builtin_a.pipes.len());
    for (a, b) in loaded_a.pipes.iter().zip(builtin_a.pipes.iter()) {
        assert_eq!(a.id, b.id);
        assert!(
            (a.design_q - b.design_q).abs() < 1e-6,
            "{} Q mismatch",
            a.id
        );
    }
    assert!((loaded.p2_rainfall_in - 3.0).abs() < 1e-9);
}

#[test]
fn project_json_roundtrip_preserves_p2_and_topology() {
    let mut project = Project::demo();
    project.p2_rainfall_in = 4.25;
    project.name = "Roundtrip Test".into();

    let path = temp_path("roundtrip.ssproj");
    project.save(&path).expect("save");
    let loaded = Project::load(&path).expect("load");

    assert_eq!(loaded.name, "Roundtrip Test");
    assert!((loaded.p2_rainfall_in - 4.25).abs() < 1e-9);
    assert_eq!(loaded.nodes.len(), project.nodes.len());
    assert_eq!(loaded.pipes.len(), project.pipes.len());
    let _ = std::fs::remove_file(path);
}

#[test]
fn sample_ssn_parse_and_analyze() {
    let path = examples_dir().join("sample.ssn");
    let text = std::fs::read_to_string(&path).expect("read sample.ssn");
    let parsed = parse_ssn(&text).expect("parse sample.ssn");
    let analysis = parsed
        .network
        .analyze(&parsed.idf, &parsed.options)
        .expect("analyze sample");
    let p3 = analysis.pipes.iter().find(|p| p.id == "P3").expect("P3");
    assert!((p3.design_q - 8.477).abs() < 0.05);
}

#[test]
fn sample_ssn_design_review_warns_on_p2_capacity() {
    let path = examples_dir().join("sample.ssn");
    let text = std::fs::read_to_string(&path).expect("read sample.ssn");
    let parsed = parse_ssn(&text).expect("parse");
    let analysis = parsed
        .network
        .analyze(&parsed.idf, &parsed.options)
        .expect("analyze");
    let findings = design_review(&parsed.network, &analysis, &ReviewCriteria::default());
    assert!(findings.iter().any(|f| f.id == "P2" && f.message.contains("capacity")));
}

#[test]
fn sample_ssn_sizing_recommends_upsize_p2() {
    let path = examples_dir().join("sample.ssn");
    let text = std::fs::read_to_string(&path).expect("read sample.ssn");
    let parsed = parse_ssn(&text).expect("parse");
    let analysis = parsed
        .network
        .analyze(&parsed.idf, &parsed.options)
        .expect("analyze");
    let recs = recommend_all_pipes(&parsed.network, &analysis, &DesignCriteria::municipal());
    let p2 = recs.iter().find(|r| r.pipe_id == "P2").expect("P2 rec");
    assert!(p2.recommended_diameter_ft > p2.current_diameter_ft);
}

#[test]
fn demo_diagnostics_report_no_validation_errors() {
    let project = Project::demo();
    let diags = run_diagnostics(&project);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == stormsewer::diagnostics::DiagSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn catchment_merge_changes_inlet_hydrology() {
    let mut project = Project::demo();
    project.catchments.push(ProjectCatchment {
        id: "C9".into(),
        vertices: vec![(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)],
        c: 0.55,
        flow_length_ft: 150.0,
        slope: 0.015,
        inlet_node_id: Some("N1".into()),
    });
    let base_q = {
        let net = project.to_network();
        let a = net.analyze(&project.idf(), &project.options()).unwrap();
        a.pipes.iter().find(|p| p.id == "P1").unwrap().design_q
    };
    let merged_q = {
        let net = project.to_analysis_network();
        let a = net.analyze(&project.idf(), &project.options()).unwrap();
        a.pipes.iter().find(|p| p.id == "P1").unwrap().design_q
    };
    assert!(merged_q > base_q, "catchment should increase P1 flow");
}

#[test]
fn dxf_export_import_roundtrip_headless() {
    let project = Project::demo();
    let path = temp_path("demo.dxf");
    export_dxf(&project, &path).expect("export dxf");
    let imported = import_dxf(&path).expect("import dxf");
    assert_eq!(imported.nodes.len(), project.nodes.len());
    assert_eq!(imported.pipes.len(), project.pipes.len());
    let _ = std::fs::remove_file(path);
}

#[test]
fn pdf_export_writes_file() {
    let project = Project::demo();
    let net = project.to_network();
    let analysis = net.analyze(&project.idf(), &project.options()).unwrap();
    let path = temp_path("report.pdf");
    export_pdf(&project, &analysis, &path, None).expect("export pdf");
    let meta = std::fs::metadata(&path).expect("pdf exists");
    assert!(meta.len() > 500, "pdf should be non-trivial size");
    let _ = std::fs::remove_file(path);
}

#[test]
fn pdf_report_options_control_sections() {
    use stormsewer::design::inlets::{network_inlet_pass, InletGeometry};
    use stormsewer::design::{design_review, ReviewCriteria};
    use stormsewer::io::{export_pdf_with, PdfOptions};

    let project = Project::demo();
    let net = project.to_network();
    let analysis = net.analyze(&project.idf(), &project.options()).unwrap();
    let findings = design_review(&net, &analysis, &ReviewCriteria::default());
    let fallback = project.idf_set().design_curve().intensity(project.min_tc);
    let lookup = |_: &str| fallback;
    let inlet_rows = network_inlet_pass(&project, &lookup, &InletGeometry::default());

    // Full report with metadata date in the header.
    let full = PdfOptions {
        generated_on: "August 26, 2026".into(),
        ..PdfOptions::default()
    };
    let full_path = temp_path("report-full.pdf");
    export_pdf_with(&project, &analysis, &inlet_rows, Some(&findings), &full, &full_path)
        .expect("full export");
    let full_bytes = std::fs::read(&full_path).expect("full pdf exists");
    assert!(full_bytes.starts_with(b"%PDF"), "valid PDF magic");

    // Chrome-only report: every section off must still produce a valid,
    // strictly smaller PDF (header band + footer only).
    let minimal = PdfOptions {
        include_summary: false,
        include_review: false,
        include_plan: false,
        include_profile: false,
        include_pipe_table: false,
        include_structure_table: false,
        include_inlet_table: false,
        generated_on: String::new(),
    };
    let min_path = temp_path("report-min.pdf");
    export_pdf_with(&project, &analysis, &inlet_rows, Some(&findings), &minimal, &min_path)
        .expect("minimal export");
    let min_bytes = std::fs::read(&min_path).expect("minimal pdf exists");
    assert!(min_bytes.starts_with(b"%PDF"));
    assert!(
        min_bytes.len() < full_bytes.len(),
        "section toggles must shrink the report ({} vs {} bytes)",
        min_bytes.len(),
        full_bytes.len()
    );
    let _ = std::fs::remove_file(full_path);
    let _ = std::fs::remove_file(min_path);
}

#[test]
fn pdf_report_paginates_long_schedules() {
    use stormsewer::io::{export_pdf_with, PdfOptions};

    // A 60-pipe straight run must break the pipe schedule across pages:
    // more page objects than the demo's single flowing document.
    let mut project = Project::empty();
    project.name = "Pagination Test".into();
    let n: usize = 60;
    // J0 is the most upstream inlet; the chain drains into the seeded OUT
    // node at the origin (invert 100).
    for i in 0..n {
        let up = (n - i) as f64;
        project.nodes.push(stormsewer::io::ProjectNode {
            id: format!("J{i}"),
            kind: "inlet".into(),
            x: up * 100.0,
            y: 0.0,
            invert: 100.0 + up * 0.4,
            rim: 106.0 + up * 0.4,
            area_ac: 0.4,
            c: 0.6,
            tc_inlet: 8.0,
            inlet: Default::default(),
            bypass_to: None,
            diameter_ft: 4.0,
        });
    }
    for i in 0..n {
        let to = if i + 1 < n {
            format!("J{}", i + 1)
        } else {
            "OUT".into()
        };
        project.pipes.push(stormsewer::io::ProjectPipe::new(
            &format!("P{i}"),
            &format!("J{i}"),
            &to,
            100.0,
            2.0,
            0.013,
        ));
    }
    let net = project.to_network();
    let analysis = net.analyze(&project.idf(), &project.options()).unwrap();
    let opts = PdfOptions::default();
    let path = temp_path("report-paged.pdf");
    export_pdf_with(&project, &analysis, &[], None, &opts, &path).expect("paged export");
    let bytes = std::fs::read(&path).expect("paged pdf exists");
    let count = |needle: &[u8]| {
        bytes
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    };
    // "/Type/Pages" (the page tree) also contains "/Type/Page", so subtract it.
    let pages = count(b"/Type/Page") - count(b"/Type/Pages");
    assert!(
        pages >= 3,
        "a 60-pipe schedule should paginate, found {pages} page objects"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn auto_size_updates_demo_diameters() {
    let project = Project::demo();
    let net = project.to_network();
    let analysis = net.analyze(&project.idf(), &project.options()).unwrap();
    let recs = recommend_all_pipes(&net, &analysis, &DesignCriteria::municipal());
    let sized = apply_sizing_to_network(&net, &recs);
    assert_eq!(sized.pipes.len(), net.pipes.len());
    for sp in &sized.pipes {
        assert!(sp.diameter > 0.0);
    }
}

#[test]
fn p2_rainfall_affects_tr55_sheet_flow_tc() {
    let tc_low = tr55_sheet_flow_minutes(300.0, 0.01, 0.02, 2.0);
    let tc_high = tr55_sheet_flow_minutes(300.0, 0.01, 0.02, 6.0);
    assert!(tc_high < tc_low, "higher P2 should reduce sheet-flow Tc");
    // FAA overland Tc is the real airfield formula: depends on C, not rainfall.
    let faa = faa_minutes(300.0, 0.01, 0.7);
    assert!(faa > 0.0 && faa < 120.0);
    assert!(faa_minutes(300.0, 0.01, 0.9) < faa, "higher C -> shorter FAA Tc");
}

#[test]
fn point_in_polygon_geometry() {
    let square = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
    assert!(point_in_polygon(50.0, 50.0, &square));
    assert!(!point_in_polygon(150.0, 50.0, &square));
    let area_ac = sqft_to_acres(shoelace_area_sqft(&square));
    assert!((area_ac - (10_000.0 / 43_560.0)).abs() < 1e-6);
}

#[test]
fn legacy_project_json_without_p2_defaults_to_three_inches() {
    let json = r#"{"name":"legacy","idf_a":60,"idf_b":10,"idf_c":0.8,"tailwater":null,"min_tc":10,"junction_k":0.5,"design_return_period_years":10,"min_slope":0.001,"nodes":[],"pipes":[]}"#;
    let p: Project = serde_json::from_str(json).unwrap();
    assert!((p.p2_rainfall_in - 3.0).abs() < 1e-9);
}

#[test]
fn empty_project_validates_and_analyzes_single_outfall() {
    let project = Project::empty();
    assert!(project.validate().is_empty());
    let net = project.to_network();
    let analysis = net.analyze(&project.idf(), &project.options()).unwrap();
    assert!(analysis.pipes.is_empty());
}