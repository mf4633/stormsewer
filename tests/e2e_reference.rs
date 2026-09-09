// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end: real Civil 3D project files in, every output channel out,
//! each checked against what Hydraflow Storm Sewers v2026.00 printed for
//! the same files.
//!
//! Inputs (tests/fixtures, coordinates moved to a local origin, hydraulics
//! untouched):
//!   civil3d-2015-storm-sewers.stm       "Run 1": 4-line trunk, 18/18/18/12 in
//!   civil3d-2015-storm-sewers-run2.stm  "Run 2": same layout, 24/18/18/12 in
//!   civil3d-2012-storm-layout.stm       both runs as laid out, no areas, 6 storms
//!   civil3d-2026-pipenetwork.xml        Run 1 as a Civil 3D 2026 LandXML export
//!
//! Reference: the Storm Sewer Tabulation, HGL Computations, and inlet pages of
//! the two Hydraflow reports (run date 2026-08-16). Run 2's report was printed
//! with a 757.62 starting HGL and the file was later re-saved with 758.618;
//! the test reproduces the printed run by setting that tailwater, and checks
//! the file's own stored HGLs separately.
//!
//! Stages per run: import → validate → engine → inlet pass → text report →
//! HTML → PDF → save/reload (.ssproj) → LandXML export/import → DXF
//! export/import → CLI subprocess. A stage passes when its numbers match the
//! reference within the documented method tolerances (VALIDATION.md §8), and
//! every round-trip must reproduce the engine's own report byte for byte.

use std::path::{Path, PathBuf};
use std::process::Command;

use stormsewer::design::inlets::{
    format_inlet_rows, network_inlet_pass_for_project, InletGeometry,
};
use stormsewer::design::{design_review, ReviewCriteria};
use stormsewer::io::{
    export_dxf, export_html, export_landxml, export_pdf_with, import_dxf, import_landxml,
    import_stm, PdfOptions, Project,
};
use stormsewer::network::Analysis;
use stormsewer::report::format_analysis;

// ───────────────────────── reference data ─────────────────────────

/// One Hydraflow line, downstream-first (line 1 = outfall pipe = our P1).
struct RefLine {
    slope: f64,
    ca: f64,
    tc: f64,
    i: f64,
    q: f64,
    cap: f64,
    /// Velocity, full-pipe lines only (elsewhere Hydraflow uses the
    /// backwater depth and this engine the normal depth — not comparable).
    v_full: Option<f64>,
    surcharged: bool,
    hgl_dn: f64,
    hgl_up: f64,
    /// HGL at the upstream structure after the junction loss.
    hgl_junct: f64,
}

struct RefRun {
    name: &'static str,
    fixture: &'static str,
    tailwater: f64,
    lines: [RefLine; 4],
    /// Inlet id → local (captured) flow, cfs; all at 100 % efficiency.
    inlets: [(&'static str, f64); 4],
}

const RUN1: RefRun = RefRun {
    name: "Run 1",
    fixture: "civil3d-2015-storm-sewers.stm",
    tailwater: 757.365,
    lines: [
        RefLine {
            slope: 0.00500,
            ca: 1.50,
            tc: 7.4,
            i: 6.83,
            q: 10.23,
            cap: 8.04,
            v_full: Some(5.79),
            surcharged: true,
            hgl_dn: 757.37,
            hgl_up: 758.82,
            hgl_junct: 759.08,
        },
        RefLine {
            slope: 0.00497,
            ca: 1.09,
            tc: 6.8,
            i: 7.07,
            q: 7.68,
            cap: 8.02,
            v_full: None,
            surcharged: false,
            hgl_dn: 759.08,
            hgl_up: 759.88,
            hgl_junct: 760.03,
        },
        RefLine {
            slope: 0.00497,
            ca: 0.71,
            tc: 5.8,
            i: 7.44,
            q: 5.28,
            cap: 8.02,
            v_full: None,
            surcharged: false,
            hgl_dn: 760.03,
            hgl_up: 760.37,
            hgl_junct: 760.44,
        },
        RefLine {
            slope: 0.00502,
            ca: 0.38,
            tc: 5.0,
            i: 7.77,
            q: 2.97,
            cap: 2.73,
            v_full: Some(3.78),
            surcharged: true,
            hgl_dn: 760.44,
            hgl_up: 761.47,
            hgl_junct: 761.69,
        },
    ],
    inlets: [
        ("AI-4", 3.19),
        ("AI-3", 2.93),
        ("AI-2", 2.55),
        ("AI-1", 2.97),
    ],
};

const RUN2: RefRun = RefRun {
    name: "Run 2",
    fixture: "civil3d-2015-storm-sewers-run2.stm",
    tailwater: 757.62,
    lines: [
        RefLine {
            slope: 0.00496,
            ca: 1.70,
            tc: 7.0,
            i: 6.98,
            q: 11.90,
            cap: 17.25,
            v_full: None,
            surcharged: false,
            hgl_dn: 757.62,
            hgl_up: 758.16,
            hgl_junct: 758.16,
        },
        RefLine {
            slope: 0.00496,
            ca: 1.28,
            tc: 6.4,
            i: 7.19,
            q: 9.20,
            cap: 8.01,
            v_full: Some(5.20),
            surcharged: true,
            hgl_dn: 758.52,
            hgl_up: 759.67,
            hgl_junct: 759.88,
        },
        RefLine {
            slope: 0.00497,
            ca: 0.86,
            tc: 5.6,
            i: 7.51,
            q: 6.49,
            cap: 8.02,
            v_full: None,
            surcharged: false,
            hgl_dn: 759.88,
            hgl_up: 760.45,
            hgl_junct: 760.55,
        },
        RefLine {
            slope: 0.00495,
            ca: 0.48,
            tc: 5.0,
            i: 7.77,
            q: 3.73,
            cap: 2.71,
            v_full: Some(4.75),
            surcharged: true,
            hgl_dn: 760.55,
            hgl_up: 762.18,
            hgl_junct: 762.53,
        },
    ],
    inlets: [
        ("AI-8", 3.30),
        ("AI-7", 3.23),
        ("AI-6", 2.98),
        ("AI-5", 3.73),
    ],
};

/// Tolerances = the method differences documented in VALIDATION.md §8.
const TOL_Q_TERMINAL: f64 = 0.01; // cfs, lines with no upstream travel time
const TOL_Q_REL: f64 = 0.03; // travel-time method (backwater vs normal depth)
const TOL_TC: f64 = 0.55; // min, same cause
const TOL_I_REL: f64 = 0.03; // in/hr, same cause
const TOL_CAP_REL: f64 = 0.004; // two-decimal inverts move √S; the Manning constant now matches Hydraflow
const TOL_HGL: f64 = 0.60; // ft, surcharged-reach friction + per-structure K
const TOL_V_FULL: f64 = 0.03; // ft/s, Q/A_full: only Q differs

// ───────────────────────── helpers ─────────────────────────

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("stormsewer-e2e-reference");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn approx(a: f64, b: f64, tol: f64, what: &str) {
    assert!(
        (a - b).abs() <= tol,
        "{what}: got {a:.4}, reference {b:.4} ± {tol:.4}"
    );
}

fn analyze(p: &Project) -> Analysis {
    let errors = p.validate();
    assert!(errors.is_empty(), "project does not validate: {errors:?}");
    p.to_analysis_network()
        .analyze(p.idf_set().design_curve(), &p.options())
        .expect("engine run")
}

fn full_report(p: &Project, a: &Analysis) -> String {
    let rows = network_inlet_pass_for_project(p, &InletGeometry::default());
    format!("{}{}", format_analysis(a), format_inlet_rows(&rows))
}

/// Every engine number against the Hydraflow tabulation for one run.
fn check_engine(run: &RefRun, p: &Project, a: &Analysis, stage: &str) {
    let ids = ["P1", "P2", "P3", "P4"];
    for (k, (id, r)) in ids.iter().zip(run.lines.iter()).enumerate() {
        let pr = a
            .pipes
            .iter()
            .find(|x| &x.id == id)
            .unwrap_or_else(|| panic!("{stage}: {id} missing"));
        let w = |q: &str| format!("{} [{stage}] {id} {q}", run.name);
        // 4e-5 covers the two-decimal inverts in the .stm against the
        // full-precision ones in the LandXML (757.90 vs 757.905 over 175 ft).
        approx(pr.slope, r.slope, 4e-5, &w("slope"));
        approx(pr.total_ca, r.ca, 0.006, &w("ΣCA"));
        approx(pr.tc, r.tc, TOL_TC, &w("Tc"));
        approx(pr.intensity, r.i, r.i * TOL_I_REL, &w("intensity"));
        let q_tol = if k >= 2 {
            TOL_Q_TERMINAL
        } else {
            r.q * TOL_Q_REL
        };
        approx(pr.design_q, r.q, q_tol, &w("Q"));
        approx(pr.capacity, r.cap, r.cap * TOL_CAP_REL, &w("capacity"));
        assert_eq!(pr.surcharged, r.surcharged, "{}", w("surcharge call"));
        if let Some(v) = r.v_full {
            // Full pipe: V = Q / A_full, so it inherits the Q tolerance.
            approx(
                pr.velocity,
                v,
                TOL_V_FULL.max(v * TOL_Q_REL),
                &w("full-pipe velocity"),
            );
            let a_full = std::f64::consts::PI * pr_diameter(p, id).powi(2) / 4.0;
            approx(
                pr.velocity,
                pr.design_q / a_full,
                1e-6,
                &w("V = Q / A_full"),
            );
        }
        let hgl_dn = pr.hgl_dn.expect("HGL pass ran");
        let hgl_up = pr.hgl_up.expect("HGL pass ran");
        if k == 0 {
            approx(
                hgl_dn,
                run.tailwater,
                1e-6,
                &w("outfall HGL = starting HGL"),
            );
        } else {
            approx(hgl_dn, r.hgl_dn, TOL_HGL, &w("HGL dn"));
        }
        // Engine hgl_up includes the structure loss → Hydraflow "HGL Junct".
        approx(
            hgl_up,
            r.hgl_junct,
            TOL_HGL,
            &w("HGL at upstream structure"),
        );
        assert!(
            hgl_up >= r.hgl_up - 0.05,
            "{}: {hgl_up} below Hydraflow's pipe-end HGL {}",
            w("HGL up"),
            r.hgl_up
        );
    }
    // Project-level inputs the importer must have carried.
    approx(
        p.tailwater.unwrap_or(f64::NAN),
        run.tailwater,
        1e-6,
        &format!("{} [{stage}] tailwater", run.name),
    );
    approx(p.min_tc, 5.0, 1e-9, "min Tc");
    for pipe in &p.pipes {
        approx(
            pipe.n,
            0.012,
            1e-9,
            &format!("{} [{stage}] {} n", run.name, pipe.id),
        );
    }
}

/// Inlet schedule: 4 × 4 sag grates capture everything at the design depth.
fn check_inlets(run: &RefRun, p: &Project, stage: &str) {
    let rows = network_inlet_pass_for_project(p, &InletGeometry::default());
    for (id, local) in run.inlets {
        let row = rows
            .iter()
            .find(|r| r.node_id == id)
            .unwrap_or_else(|| panic!("{stage}: inlet {id} missing"));
        let w = |q: &str| format!("{} [{stage}] inlet {id} {q}", run.name);
        approx(row.local_cfs, local, 0.01, &w("local Q"));
        approx(row.intercepted_cfs, local, 0.01, &w("captured Q (100 %)"));
        approx(row.bypass_cfs, 0.0, 1e-6, &w("bypass"));
        assert!(row.ok, "{}", w("ok"));
    }
}

fn pr_diameter(p: &Project, id: &str) -> f64 {
    p.pipes.iter().find(|x| x.id == id).unwrap().diameter
}

fn load_run(run: &RefRun) -> Project {
    let mut p = import_stm(&fixture(run.fixture)).expect("STM import");
    // Run 2's file holds a later starting HGL than the printed report.
    p.tailwater = Some(run.tailwater);
    p
}

fn hex_upper(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02X}")).collect()
}

// ───────────────────────── stages ─────────────────────────

fn pipeline(run: &RefRun) {
    let tag = run.name.replace(' ', "_").to_ascii_lowercase();

    // 1. Import + engine.
    let p = load_run(run);
    let a = analyze(&p);
    check_engine(run, &p, &a, "engine");
    check_inlets(run, &p, "engine");
    let report = full_report(&p, &a);

    // 2. Text report carries the schedule the app shows.
    for id in ["P1", "P2", "P3", "P4"] {
        assert!(report.contains(id), "{}: text report lacks {id}", run.name);
    }
    for (id, _) in run.inlets {
        assert!(
            report.contains(id),
            "{}: text report lacks inlet {id}",
            run.name
        );
    }
    assert!(report.contains("INLET SCHEDULE"));
    let q1 = format!(
        "{:.2}",
        a.pipes.iter().find(|x| x.id == "P1").unwrap().design_q
    );
    assert!(
        report.contains(&q1),
        "{}: report lacks P1 design Q {q1}",
        run.name
    );
    for (id, r) in ["P1", "P2", "P3", "P4"].iter().zip(run.lines.iter()) {
        let line = report.lines().find(|l| l.starts_with(id)).unwrap();
        assert_eq!(
            line.contains("SURCHARGED"),
            r.surcharged,
            "{}: {id} status line: {line}",
            run.name
        );
    }

    // 3. HTML.
    let html = scratch(&format!("{tag}.html"));
    export_html(&p, &a, &html).unwrap();
    let html_text = std::fs::read_to_string(&html).unwrap();
    for id in ["P1", "P4", run.inlets[0].0] {
        assert!(html_text.contains(id), "{}: HTML lacks {id}", run.name);
    }

    // 4. PDF with every section, including the inlet table.
    let rows = network_inlet_pass_for_project(&p, &InletGeometry::default());
    let net = p.to_analysis_network();
    let findings = design_review(&net, &a, &ReviewCriteria::default());
    let opts = PdfOptions {
        generated_on: "2026-09-08".into(),
        ..PdfOptions::default()
    };
    let pdf = scratch(&format!("{tag}.pdf"));
    export_pdf_with(&p, &a, &rows, Some(&findings), &opts, &pdf).unwrap();
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(
        bytes.len() > 10_000,
        "{}: PDF is {} bytes",
        run.name,
        bytes.len()
    );
    let pdf_text = String::from_utf8_lossy(&bytes).into_owned();
    // printpdf writes text operands as uppercase hex strings.
    for needle in ["P1", "P4", run.inlets[0].0, &q1] {
        assert!(
            pdf_text.contains(&hex_upper(needle)),
            "{}: PDF lacks {needle}",
            run.name
        );
    }
    let pages = pdf_text.matches("/Type/Page").count() - pdf_text.matches("/Type/Pages").count();
    assert!(pages >= 2, "{}: PDF has {pages} page(s)", run.name);

    // 5. Save / reload: byte-identical report, drops and tailwater intact.
    let proj = scratch(&format!("{tag}.ssproj"));
    p.save(&proj).unwrap();
    let p2 = Project::load(&proj).unwrap();
    assert_eq!(
        p2.pipes
            .iter()
            .map(|x| (x.invert_up, x.invert_dn))
            .collect::<Vec<_>>(),
        p.pipes
            .iter()
            .map(|x| (x.invert_up, x.invert_dn))
            .collect::<Vec<_>>(),
        "{}: per-pipe inverts lost in .ssproj",
        run.name
    );
    assert!(
        p2.pipes.iter().any(|x| x.invert_dn.is_some()),
        "{}: no drop survived",
        run.name
    );
    let a2 = analyze(&p2);
    assert_eq!(
        full_report(&p2, &a2),
        report,
        "{}: reloaded project reports differently",
        run.name
    );

    // 6. LandXML export → import through the Civil 3D parser.
    let xml = scratch(&format!("{tag}.xml"));
    export_landxml(&p, &xml).unwrap();
    let mut p3 = import_landxml(&xml).unwrap();
    // LandXML carries geometry, not the storm or tailwater: restore those.
    p3.idf_curves = p.idf_curves.clone();
    p3.idf_a = p.idf_a;
    p3.idf_b = p.idf_b;
    p3.idf_c = p.idf_c;
    p3.design_return_period_years = p.design_return_period_years;
    p3.tailwater = p.tailwater;
    p3.min_tc = p.min_tc;
    p3.junction_k = p.junction_k;
    for pipe in &mut p3.pipes {
        pipe.n = 0.012; // material is not in our export
    }
    for n in &mut p3.nodes {
        // Areas ride on the structure, and Hydraflow's inlet times are 5 min.
        if let Some(src) = p.nodes.iter().find(|s| s.id == n.id) {
            n.area_ac = src.area_ac;
            n.c = src.c;
            n.tc_inlet = src.tc_inlet;
            n.inlet = src.inlet.clone();
            n.kind = src.kind.clone();
        }
    }
    let a3 = analyze(&p3);
    check_engine(run, &p3, &a3, "landxml round-trip");
    for (x, y) in p.pipes.iter().zip(p3.pipes.iter()) {
        assert_eq!(x.id, y.id);
        approx(
            y.length,
            x.length,
            1e-3,
            &format!("{} landxml {} length", run.name, x.id),
        );
        assert_eq!(
            x.invert_dn.map(|v| (v * 1000.0).round()),
            y.invert_dn.map(|v| (v * 1000.0).round()),
            "{}: landxml lost the drop on {}",
            run.name,
            x.id
        );
    }

    // 7. DXF network export → import.
    let dxf = scratch(&format!("{tag}.dxf"));
    export_dxf(&p, &dxf).unwrap();
    let mut p4 = import_dxf(&dxf).unwrap();
    p4.idf_curves = p.idf_curves.clone();
    p4.idf_a = p.idf_a;
    p4.idf_b = p.idf_b;
    p4.idf_c = p.idf_c;
    p4.design_return_period_years = p.design_return_period_years;
    p4.tailwater = p.tailwater;
    p4.min_tc = p.min_tc;
    p4.junction_k = p.junction_k;
    for n in &mut p4.nodes {
        if let Some(src) = p.nodes.iter().find(|s| s.id == n.id) {
            n.inlet = src.inlet.clone();
        }
    }
    // DXF pipes are renumbered in drawing order; match them by endpoints.
    for src in &p.pipes {
        let got = p4
            .pipes
            .iter_mut()
            .find(|x| x.from == src.from && x.to == src.to)
            .unwrap_or_else(|| panic!("{}: DXF lost pipe {}→{}", run.name, src.from, src.to));
        got.id = src.id.clone();
        approx(got.diameter, src.diameter, 1e-9, "dxf diameter");
        approx(got.n, src.n, 1e-9, "dxf n");
        assert_eq!(
            got.invert_dn.map(|v| (v * 1e6).round()),
            src.invert_dn.map(|v| (v * 1e6).round()),
            "{}: DXF lost the drop on {}",
            run.name,
            src.id
        );
        // DXF has no length record; the drawn length is the plan distance.
        got.length = src.length;
    }
    let a4 = analyze(&p4);
    check_engine(run, &p4, &a4, "dxf round-trip");
}

#[test]
fn run1_full_pipeline_against_hydraflow_report() {
    pipeline(&RUN1);
}

#[test]
fn run2_full_pipeline_against_hydraflow_report() {
    pipeline(&RUN2);
}

/// The Run 2 file was re-saved after printing with a 758.618 starting HGL
/// and stores Hydraflow's HGLs for that state; check against those too.
#[test]
fn run2_file_state_matches_the_hgls_hydraflow_stored_in_it() {
    let p = import_stm(&fixture(RUN2.fixture)).unwrap();
    approx(p.tailwater.unwrap(), 758.618, 1e-6, "file starting HGL");
    let a = analyze(&p);
    // "HGL Junct" per line from the .stm (line 1 → P1 …).
    let stored = [759.1578, 760.8777, 761.5511, 763.5285];
    for (id, h) in ["P1", "P2", "P3", "P4"].iter().zip(stored) {
        let pr = a.pipes.iter().find(|x| &x.id == id).unwrap();
        approx(
            pr.hgl_up.unwrap(),
            h,
            TOL_HGL,
            &format!("Run 2 (file state) {id} HGL at structure"),
        );
    }
    approx(a.pipes[0].hgl_dn.unwrap(), 758.618, 1e-6, "outfall HGL");
}

#[test]
fn layout_2012_imports_both_runs_and_all_six_storms() {
    let p = import_stm(&fixture("civil3d-2012-storm-layout.stm")).expect("2012 format imports");
    assert_eq!(p.pipes.len(), 8);
    assert_eq!(
        p.nodes.iter().filter(|n| n.kind == "outfall").count(),
        2,
        "two runs, two outfalls"
    );
    for id in [
        "AI-1", "AI-2", "AI-3", "AI-4", "AI-5", "AI-6", "AI-7", "AI-8",
    ] {
        assert!(p.nodes.iter().any(|n| n.id == id), "{id} present");
    }
    // FHA IDF table: 2/5/10/25/50/100-yr columns populated, 2-yr selected.
    let rps: Vec<u32> = p.idf_curves.iter().map(|c| c.rp_years).collect();
    assert_eq!(rps, vec![2, 5, 10, 25, 50, 100]);
    approx(
        p.design_return_period_years,
        2.0,
        1e-9,
        "design storm (years, not slot)",
    );
    approx(p.idf_a, 69.87033, 1e-5, "2-yr a");
    let c100 = p.idf_curves.iter().find(|c| c.rp_years == 100).unwrap();
    approx(c100.a, 127.1596, 1e-4, "100-yr a");
    // Layout stage: no areas yet, inlet time 0 → the file's 5-min minimum.
    for n in p.nodes.iter().filter(|n| n.kind == "inlet") {
        approx(n.tc_inlet, 5.0, 1e-9, &format!("{} inlet time", n.id));
        approx(n.area_ac, 0.0, 1e-9, "no drainage area yet");
    }
    for pipe in &p.pipes {
        approx(pipe.n, 0.013, 1e-9, "2012 file n");
    }
    approx(
        p.junction_k,
        0.5,
        1e-9,
        "all-zero (auto) K falls back to 0.5",
    );
    assert!(p.tailwater.is_none(), "Starting HGL = 0 means none");
    // Runs for every storm, monotone in return period once areas exist.
    let mut q = p.clone();
    for n in q.nodes.iter_mut().filter(|n| n.kind == "inlet") {
        n.area_ac = 0.5;
        n.c = 0.7;
    }
    let net = q.to_analysis_network();
    let all = net.analyze_all_rps(&q.idf_set(), &q.options()).unwrap();
    assert_eq!(all.len(), 6);
    let outfall_q: Vec<f64> = all
        .iter()
        .map(|(_, a)| a.pipes.iter().find(|x| x.id == "P1").unwrap().design_q)
        .collect();
    for w in outfall_q.windows(2) {
        assert!(w[1] > w[0], "Q must grow with return period: {outfall_q:?}");
    }
}

/// The LandXML export of Run 1 from Civil 3D 2026 analyzes to the same
/// numbers as the .stm once the storm, tailwater, and areas are supplied.
#[test]
fn civil3d_landxml_of_run1_analyzes_like_the_stm() {
    let stm = load_run(&RUN1);
    let mut xml = import_landxml(&fixture("civil3d-2026-pipenetwork.xml")).unwrap();
    xml.idf_curves = stm.idf_curves.clone();
    xml.idf_a = stm.idf_a;
    xml.idf_b = stm.idf_b;
    xml.idf_c = stm.idf_c;
    xml.design_return_period_years = stm.design_return_period_years;
    xml.tailwater = stm.tailwater;
    xml.min_tc = stm.min_tc;
    xml.junction_k = stm.junction_k;
    for pipe in &mut xml.pipes {
        pipe.n = 0.012;
    }
    for n in &mut xml.nodes {
        if let Some(src) = stm.nodes.iter().find(|s| s.id == n.id) {
            n.area_ac = src.area_ac;
            n.c = src.c;
            n.tc_inlet = src.tc_inlet;
            n.inlet = src.inlet.clone();
            n.rim = src.rim; // AI-4's rim was edited between the two exports
        }
    }
    // Civil 3D names pipes from the top; renumber to Hydraflow's line order.
    for (from, to) in [("P5", "P1"), ("P4", "P2"), ("P3", "P3"), ("P2", "P4")] {
        if let Some(pipe) = xml.pipes.iter_mut().find(|x| x.id == from) {
            pipe.id = format!("tmp-{to}");
        }
    }
    for pipe in &mut xml.pipes {
        pipe.id = pipe.id.trim_start_matches("tmp-").to_string();
    }
    let a = analyze(&xml);
    check_engine(&RUN1, &xml, &a, "civil3d landxml");
    check_inlets(&RUN1, &xml, "civil3d landxml");
}

// ───────────────────────── CLI ─────────────────────────

fn cli_bin() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("cargo")
        .args(["build", "--bin", "stormsewer-cli", "--quiet"])
        .current_dir(&root)
        .status()
        .expect("cargo build cli");
    assert!(status.success());
    root.join("target").join("debug").join(if cfg!(windows) {
        "stormsewer-cli.exe"
    } else {
        "stormsewer-cli"
    })
}

fn cli(path: &Path) -> String {
    let out = Command::new(cli_bin()).arg(path).output().expect("run cli");
    assert!(
        out.status.success(),
        "cli failed on {}: {}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn cli_analyzes_every_civil3d_input_like_the_app() {
    // STM: the CLI's text must equal the library's report for the same file.
    let p = import_stm(&fixture(RUN1.fixture)).unwrap();
    let a = analyze(&p);
    let expected = full_report(&p, &a);
    let out = cli(&fixture(RUN1.fixture));
    assert_eq!(
        out.trim_end(),
        expected.trim_end(),
        "CLI and library reports differ for Run 1"
    );
    assert!(out.contains("AI-4") && out.contains("INLET SCHEDULE"));

    // Run 2, LandXML, and a saved project all run.
    assert!(cli(&fixture(RUN2.fixture)).contains("AI-8"));
    assert!(cli(&fixture("civil3d-2026-pipenetwork.xml")).contains("AI-1"));
    let proj = scratch("cli-run1.ssproj");
    p.save(&proj).unwrap();
    assert_eq!(
        cli(&proj).trim_end(),
        expected.trim_end(),
        "CLI on .ssproj differs from .stm"
    );

    // Legacy .ssn path is untouched.
    let ssn = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/sample.ssn");
    assert!(cli(&ssn).contains("STORM SEWER ANALYSIS"));

    // And an unknown extension fails loudly.
    let bad = scratch("network.txt");
    std::fs::write(&bad, "nothing").unwrap();
    let out = Command::new(cli_bin()).arg(&bad).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unsupported file type"));
}
