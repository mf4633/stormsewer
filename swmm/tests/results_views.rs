// SPDX-License-Identifier: GPL-3.0-or-later

//! The results views' data layer against a real run: the Detention Pond
//! sample and the `.out`/`.rpt` EPA SWMM 5.2.4 produced from it
//! (`tests/fixtures/results/README.md`).

use std::path::PathBuf;

use stormsewer_swmm::doc::InpDoc;
use stormsewer_swmm::out::{
    link_series, link_variable_names, node_series, node_variable_names, read_frame, read_frame_raw,
    OutputFile,
};
use stormsewer_swmm::profile::{LinkKind, NodeKind, OffsetMode, ProfileNetwork};
use stormsewer_swmm::results::ClassBreaks;
use stormsewer_swmm::rpt;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn pond_doc() -> InpDoc {
    InpDoc::read(&fixture("epa-samples/Detention_Pond_Model.inp")).unwrap()
}

fn pond_out() -> OutputFile {
    OutputFile::open(fixture("results/Detention_Pond_Model.out")).unwrap()
}

// --- .out ---------------------------------------------------------------------

#[test]
fn fixture_run_reads_and_matches_the_report() {
    let out = pond_out();
    let m = &out.meta;
    assert_eq!((m.n_subcatch, m.n_nodes, m.n_links), (8, 14, 14));
    assert_eq!(m.report_step_s, 300);
    assert_eq!(m.n_periods, 144, "12 h at 5 min, first report at 00:05");
    assert_eq!(m.error_code, 0);
    assert!(!m.flow_units.is_metric());

    // The report says J11's reported (report-step) max depth is 4.50 ft.
    let s = node_series(&out.path, m, "J11", 0).unwrap();
    let (peak, _) = s.peak().unwrap();
    assert!((peak - 4.50).abs() < 0.01, "J11 reported max depth {peak}");
    // The report's link maximum (34.96) is over routing steps; the binary
    // holds report-step samples, so its peak is close but never higher.
    let s = link_series(&out.path, m, "C11", 0).unwrap();
    let (peak, _) = s.peak().unwrap();
    assert!(peak > 30.0 && peak <= 34.96 + 1e-6, "C11 peak flow {peak}");
}

#[test]
fn raw_frames_agree_with_frames_and_series() {
    let out = pond_out();
    let m = &out.meta;
    for period in [0, 7, m.n_periods - 1] {
        let raw = read_frame_raw(&out.path, m, period).unwrap();
        let frame = read_frame(&out.path, m, period).unwrap();
        assert_eq!(raw.time_s, frame.time_s);
        assert_eq!(raw.date_days, frame.date_days);
        assert_eq!(raw.system.len(), 15);
        for i in 0..m.n_nodes {
            assert_eq!(raw.node(i, 0), Some(frame.nodes[i].depth));
            assert_eq!(raw.node(i, 1), Some(frame.nodes[i].head));
            assert_eq!(raw.node(i, 4), Some(frame.nodes[i].total_inflow));
            assert_eq!(raw.node(i, 5), Some(frame.nodes[i].flooding));
        }
        for i in 0..m.n_links {
            assert_eq!(raw.link(i, 0), Some(frame.links[i].flow));
            assert_eq!(raw.link(i, 4), Some(frame.links[i].capacity));
        }
        assert_eq!(raw.node(0, 99), None);
        assert_eq!(raw.link(m.n_links, 0), None);
    }
    // Volume (index 2) is not in `Frame`; check it against the series reader.
    let raw = read_frame_raw(&out.path, m, 7).unwrap();
    let j11 = m.node_ids.iter().position(|s| s == "J11").unwrap();
    let vol = node_series(&out.path, m, "J11", 2).unwrap();
    assert_eq!(raw.node(j11, 2), Some(vol.values[7]));
    assert!(read_frame_raw(&out.path, m, m.n_periods).is_err());

    assert_eq!(node_variable_names(m).len(), m.n_node_vars());
    assert_eq!(link_variable_names(m).len(), m.n_link_vars());
    assert_eq!(node_variable_names(m)[5], "Flooding");
    assert_eq!(link_variable_names(m)[4], "Capacity");
}

// --- .rpt summary tables ------------------------------------------------------

#[test]
fn report_summary_tables_parse_from_the_fixture() {
    let rep = rpt::read(&fixture("results/Detention_Pond_Model.rpt")).unwrap();
    assert_eq!(rep.engine_version.as_deref(), Some("5.2.4"));
    assert!(rep.is_clean());
    let titles: Vec<&str> = rep.tables.iter().map(|t| t.title.as_str()).collect();
    for want in [
        "Routing Time Step Summary",
        "Subcatchment Runoff Summary",
        "Node Depth Summary",
        "Node Inflow Summary",
        "Node Flooding Summary",
        "Storage Volume Summary",
        "Outfall Loading Summary",
        "Link Flow Summary",
        "Conduit Surcharge Summary",
    ] {
        assert!(titles.contains(&want), "missing {want} in {titles:?}");
    }

    let depth = rep
        .tables
        .iter()
        .find(|t| t.title == "Node Depth Summary")
        .unwrap();
    assert_eq!(depth.header_lines.len(), 3, "raw heading text is kept");
    assert_eq!(
        depth.columns,
        vec![
            "Node",
            "Type",
            "Average Depth Feet",
            "Maximum Depth Feet",
            "Maximum HGL Feet",
            "Time of Max Occurrence days hr:min",
            "Reported Max Depth Feet",
        ]
    );
    assert_eq!(depth.rows.len(), 14);
    let j11 = depth.rows.iter().find(|r| r[0] == "J11").unwrap();
    assert_eq!(
        j11,
        &["J11", "JUNCTION", "4.05", "4.54", "4967.54", "0 00:36", "4.50"]
    );
    assert_eq!(depth.column("maximum depth"), Some(3));

    // Blank cells stay blank and in place: an orifice reports no velocity.
    let flow = rep
        .tables
        .iter()
        .find(|t| t.title == "Link Flow Summary")
        .unwrap();
    let o1 = flow.rows.iter().find(|r| r[0] == "O1").unwrap();
    assert_eq!(o1, &["O1", "ORIFICE", "3.72", "0 01:15", "", "", "0.00"]);

    // Totals under a rule inside the body belong to the table.
    let outfall = rep
        .tables
        .iter()
        .find(|t| t.title == "Outfall Loading Summary")
        .unwrap();
    assert_eq!(outfall.rows.len(), 2);
    assert_eq!(outfall.rows[1][0], "System");

    let flooding = rep
        .tables
        .iter()
        .find(|t| t.title == "Node Flooding Summary")
        .unwrap();
    assert!(flooding.rows.is_empty());
    assert_eq!(flooding.note.as_deref(), Some("No nodes were flooded."));

    let steps = rep
        .tables
        .iter()
        .find(|t| t.title == "Routing Time Step Summary")
        .unwrap();
    assert_eq!(steps.columns, vec!["Item", "Value"]);
    assert!(steps
        .rows
        .iter()
        .any(|r| r[0] == "Minimum Time Step" && r[1] == "15.00 sec"));

    // Units in the headings survive lossy decoding of the cp1252 byte.
    let storage = rep
        .tables
        .iter()
        .find(|t| t.title == "Storage Volume Summary")
        .unwrap();
    assert!(storage.columns[1].starts_with("Average Volume 1000 ft"));

    let csv = depth.to_csv();
    assert!(csv.starts_with("Node,Type,Average Depth Feet,"));
    assert!(csv.contains("\nJ11,JUNCTION,4.05,4.54,4967.54,0 00:36,4.50\n"));
}

#[test]
fn metric_headings_are_kept_as_written() {
    // The parser must not assume US units: a CMS report says "Meters".
    let text = "\
  ******************
  Node Depth Summary
  ******************

  ---------------------------------------------------------------------------------
                                 Average  Maximum  Maximum  Time of Max    Reported
                                   Depth    Depth      HGL   Occurrence   Max Depth
  Node                 Type       Meters   Meters   Meters  days hr:min      Meters
  ---------------------------------------------------------------------------------
  N1                   JUNCTION     0.03     0.49    12.49     0  00:35        0.49
  Long_Node_Name_Here  OUTFALL      1.50    12.00    24.00     1  13:05       12.00
";
    let tables = rpt::parse_tables(text);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].columns[2], "Average Depth Meters");
    assert_eq!(tables[0].rows[1][0], "Long_Node_Name_Here");
    assert_eq!(tables[0].rows[1][5], "1 13:05");
}

// --- profile geometry ---------------------------------------------------------

#[test]
fn detention_pond_profile_geometry() {
    let net = ProfileNetwork::from_doc(&pond_doc());
    assert_eq!(net.offsets, OffsetMode::Depth);
    assert_eq!(net.flow_units, "CFS");
    assert_eq!(net.nodes.len(), 14);
    assert_eq!(net.links.len(), 14);

    // Directed path from the top of the system to the outfall.
    let p = net.build("J1", "O2").unwrap();
    let ids: Vec<&str> = p.links.iter().map(|l| l.link.as_str()).collect();
    assert_eq!(ids.len(), 9, "{ids:?}");
    assert_eq!(&ids[..7], &["C1", "C5", "C7", "C8", "C9", "C10", "C11"]);
    assert!(ids[7] == "O1" || ids[7] == "W1", "pond outlet: {ids:?}");
    assert_eq!(ids[8], "C_out");
    assert!(p.links.iter().all(|l| l.forward));

    // Stations: conduit lengths, with the orifice given a nominal span.
    let conduits: f64 = [185.0, 207.0, 95.0, 166.0, 320.0, 145.0, 150.0, 100.0]
        .iter()
        .sum();
    assert!((p.length() - (conduits + stormsewer_swmm::profile::NON_CONDUIT_SPAN)).abs() < 1e-9);
    let j5 = p.nodes.iter().find(|n| n.node == "J5").unwrap();
    assert_eq!(j5.station, 185.0);
    assert_eq!(j5.kind, NodeKind::Junction);

    // MaxDepth 0 -> rim from the highest connecting crown. J5 joins C1, C4
    // (trapezoidal, depth 3) and C5: 4969.8 + 3.
    assert!((j5.rim - 4972.8).abs() < 1e-9, "J5 rim {}", j5.rim);
    // Storage has a written depth: 4956 + 10.
    let su1 = p.nodes.iter().find(|n| n.node == "SU1").unwrap();
    assert_eq!(su1.kind, NodeKind::Storage);
    assert!((su1.rim - 4966.0).abs() < 1e-9);

    // C11 is circular 4.75 ft with a 1 ft outlet offset into the pond.
    let c11 = p.links.iter().find(|l| l.link == "C11").unwrap();
    assert_eq!(c11.kind, LinkKind::Conduit);
    assert!((c11.up_invert - 4963.0).abs() < 1e-9);
    assert!((c11.dn_invert - 4957.0).abs() < 1e-9);
    assert!((c11.dn_crown - (4957.0 + 4.75)).abs() < 1e-9);
    assert_eq!(c11.shape, "CIRCULAR");

    // C2 has a 4 ft outlet offset: its downstream invert sits above J11.
    let c2 = net.link("C2").unwrap();
    assert_eq!(c2.out_offset, 4.0);
    let side = net.build("J2", "J11").unwrap();
    assert!((side.links[0].dn_invert - 4967.0).abs() < 1e-9);

    // Hover lookups and CSV.
    let (inv, crown, seg) = p.pipe_at(100.0).unwrap();
    assert_eq!(seg.link, "C1");
    assert!(inv < crown);
    assert!(p.pipe_at(-1.0).is_none());
    let csv = p.to_csv(&[], &[]);
    assert!(csv.starts_with("node,kind,station_ft,invert_ft,rim_ft,"));
    assert_eq!(csv.lines().count(), 1 + p.nodes.len());
}

#[test]
fn profile_hgl_classification() {
    let net = ProfileNetwork::from_doc(&pond_doc());
    let p = net.build("J11", "SU1").unwrap();
    // J11 invert 4963, C11 crown 4967.75 upstream. An HGL of 4968 surcharges
    // C11 but does not flood J11 (rim = crown of C2's outlet, 4967 + 1 = 4968? no:
    // C2's out offset is 4 and its depth 1, so 4963 + 4 + 1 = 4968).
    let j11 = &p.nodes[0];
    assert!((j11.rim - 4968.0).abs() < 1e-9, "J11 rim {}", j11.rim);
    let hgl = [Some(4968.0), Some(4957.5)];
    assert_eq!(p.surcharged(&hgl), vec![0]);
    assert!(p.flooded(&hgl).is_empty());
    let hgl = [Some(4968.5), None];
    assert_eq!(p.flooded(&hgl), vec![0]);
}

#[test]
fn no_path_is_reported_not_invented() {
    let net = ProfileNetwork::from_doc(&pond_doc());
    // Directed: O2 has no outgoing links, so the reverse path must fall back
    // to the undirected search and still be found.
    let back = net.find_path("O2", "J1").unwrap();
    assert!(back.iter().all(|(_, fwd)| !fwd));
    // J2 and J3 are on separate branches that only meet downstream, so a
    // directed path between them does not exist; undirected does.
    let p = net.build("J3", "J2").unwrap();
    assert!(p.links.iter().any(|l| !l.forward));
}

// --- class breaks over real peaks ---------------------------------------------

#[test]
fn class_breaks_colour_the_run_peaks() {
    let out = pond_out();
    let peaks = stormsewer_swmm::out::link_peaks(&out.path, &out.meta).unwrap();
    let flows: Vec<f64> = peaks.iter().map(|p| p.max_flow).collect();
    let lo = flows.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = flows.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let b = ClassBreaks::equal_interval(lo, hi);
    let classes: Vec<usize> = flows.iter().map(|f| b.class_of(*f)).collect();
    assert!(classes.contains(&0) && classes.contains(&4));
    let c11 = peaks.iter().position(|p| p.id == "C11").unwrap();
    assert_eq!(classes[c11], 4, "the largest flow is in the top class");
    let q = ClassBreaks::quantiles(&flows);
    assert!(q.breaks.windows(2).all(|w| w[0] <= w[1]));
}

// --- live engine, opt in ------------------------------------------------------

/// Re-run the fixture through a discovered engine and check the committed
/// results still describe the same run. Gated the same way as the engine
/// crate's own live test.
#[test]
fn live_engine_reproduces_the_fixture_run() {
    let Ok(_) = std::env::var("STORMSEWER_SWMM_ENGINE_DIR") else {
        eprintln!("skipped: STORMSEWER_SWMM_ENGINE_DIR not set");
        return;
    };
    let registry = stormsewer_swmm::engine::Registry::discover();
    let Some(engine) = registry.default_engine() else {
        eprintln!("skipped: no engine discovered");
        return;
    };
    let work = std::env::temp_dir().join("stormsewer-swmm-tests/results-views");
    std::fs::create_dir_all(&work).unwrap();
    let inp = work.join("Detention_Pond_Model.inp");
    std::fs::copy(fixture("epa-samples/Detention_Pond_Model.inp"), &inp).unwrap();
    let run = engine.run(&inp).unwrap();
    assert!(run.succeeded(), "{:?}", run.failure_reason());
    let fresh = OutputFile::open(&run.out).unwrap();
    let committed = pond_out();
    assert_eq!(fresh.meta.n_periods, committed.meta.n_periods);
    assert_eq!(fresh.meta.node_ids, committed.meta.node_ids);
    let a = rpt::read(&run.rpt).unwrap();
    let b = rpt::read(&fixture("results/Detention_Pond_Model.rpt")).unwrap();
    assert_eq!(a.tables.len(), b.tables.len());
    if a.engine_version == b.engine_version {
        let ta = a
            .tables
            .iter()
            .find(|t| t.title == "Node Depth Summary")
            .unwrap();
        let tb = b
            .tables
            .iter()
            .find(|t| t.title == "Node Depth Summary")
            .unwrap();
        assert_eq!(ta.rows, tb.rows);
    }
}
