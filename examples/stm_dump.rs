// Import a real Hydraflow .stm (or .ssproj/.dxf) and print the analysis at full
// precision, for cross-checking against a Storm Sewers report.
// Run: cargo run --example stm_dump -- <file>
use stormsewer::design::inlets::{network_inlet_pass, InletGeometry};
use stormsewer::io::Project;

fn main() {
    let path = std::env::args().nth(1).expect("usage: stm_dump <file>");
    let path = std::path::Path::new(&path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let project = match ext.as_str() {
        "stm" => stormsewer::io::stm::import_stm(path),
        "dxf" => stormsewer::io::dxf::import_dxf(path),
        "xml" => stormsewer::io::landxml::import_landxml(path),
        _ => Project::load(path),
    };
    let project = match project {
        Ok(p) => p,
        Err(e) => {
            eprintln!("IMPORT ERROR: {e}");
            std::process::exit(2);
        }
    };
    println!(
        "name={:?} units={:?} rp={} IDF a={} b={} c={} curves={:?}",
        project.name,
        project.units,
        project.design_return_period_years,
        project.idf_a,
        project.idf_b,
        project.idf_c,
        project
            .idf_curves
            .iter()
            .map(|c| (c.rp_years, c.a, c.b, c.c))
            .collect::<Vec<_>>()
    );
    println!(
        "tailwater={:?} min_tc={} min_slope={} junction_k={} bg_dxf={:?}",
        project.tailwater,
        project.min_tc,
        project.min_slope,
        project.junction_k,
        project.background_dxf.as_ref().map(|b| b.path.clone())
    );
    for n in &project.nodes {
        println!("NODE {:8} kind={:9} x={:.3} y={:.3} inv={:8.3} rim={:8.3} area={:6.3} C={:5.3} tc={:5.2} inlet={:?}",
            n.id, n.kind, n.x, n.y, n.invert, n.rim, n.area_ac, n.c, n.tc_inlet, n.inlet);
    }
    for p in &project.pipes {
        println!(
            "PIPE {:6} {:8}->{:8} L={:8.3} D={:6.4} n={:6.4} shape={} rise={} span={}",
            p.id, p.from, p.to, p.length, p.diameter, p.n, p.shape, p.rise_ft, p.span_ft
        );
    }
    let errs = project.validate();
    if !errs.is_empty() {
        println!("VALIDATION: {errs:?}");
    }
    let net = project.to_analysis_network();
    let idf_set = project.idf_set();
    let a = match net.analyze(idf_set.design_curve(), &project.options()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ANALYSIS ERROR: {e}");
            std::process::exit(3);
        }
    };
    for p in &a.pipes {
        println!("RES  {:6} S={:.5} CA={:.4} tc={:.3} i={:.3} Q={:.3} cap={:.3} pct={:.3} V={:.3} Vfull={:.3} yn={:?} yc={:.3} tt={:.3} hgl_up={:?} hgl_dn={:?} surch={}",
            p.id, p.slope, p.total_ca, p.tc, p.intensity, p.design_q, p.capacity, p.pct_full,
            p.velocity, p.velocity_full, p.normal_depth, p.critical_depth, p.travel_time, p.hgl_up, p.hgl_dn, p.surcharged);
    }
    for n in &a.nodes {
        println!(
            "NODERES {:8} tc={:.3} rim={:.3} hgl={:.4} flood={}",
            n.id, n.tc, n.rim, n.hgl, n.surcharge_to_surface
        );
    }
    let fallback = idf_set.design_curve().intensity(project.min_tc);
    for r in network_inlet_pass(&project, &|_| fallback, &InletGeometry::default()) {
        println!(
            "INLET {:8} local={:.3} co_in={:.3} int={:.3} byp={:.3} spread={:.3} ok={}",
            r.node_id,
            r.local_cfs,
            r.carryover_in_cfs,
            r.intercepted_cfs,
            r.bypass_cfs,
            r.spread_ft,
            r.ok
        );
    }
    print!("{}", stormsewer::report::format_analysis(&a));
}
