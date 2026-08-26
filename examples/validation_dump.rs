// Prints the reference-network numbers that VALIDATION.md documents, at full
// precision. Run: cargo run --example validation_dump
use stormsewer::design::inlets::{network_inlet_pass, InletGeometry};
use stormsewer::io::Project;

fn main() {
    let project = Project::demo();
    let net = project.to_network();
    let a = net.analyze(&project.idf(), &project.options()).expect("analyze");

    println!("IDF a={} b={} c={}", project.idf_a, project.idf_b, project.idf_c);
    println!("tailwater={:?} min_tc={}", project.tailwater, project.min_tc);
    println!("junction_k={:?}", project.options().junction_k);
    println!();
    for n in &project.nodes {
        println!(
            "NODE {:4} kind={:9} inv={:8.3} rim={:8.3} area={:6.3} C={:5.3} tc={:6.3}",
            n.id, n.kind, n.invert, n.rim, n.area_ac, n.c, n.tc_inlet
        );
    }
    println!();
    for p in &project.pipes {
        println!(
            "PIPE {:4} {:4}->{:4} L={:8.3} D={:6.4} n={:6.4}",
            p.id, p.from, p.to, p.length, p.diameter, p.n
        );
    }
    println!();
    for p in &a.pipes {
        println!(
            "RES  {:4} S={:.8} tc={:.6} i={:.6} Q={:.6} cap={:.6} pct={:.6} V={:.6} yn={:?} hgl_up={:?} hgl_dn={:?} surch={}",
            p.id, p.manning_slope, p.tc, p.intensity, p.design_q, p.capacity,
            p.pct_full, p.velocity, p.normal_depth, p.hgl_up, p.hgl_dn, p.surcharged
        );
    }
    println!();
    for n in &a.nodes {
        println!(
            "NODERES {:4} tc={:.6} rim={:.3} hgl={:.6} flood={}",
            n.id, n.tc, n.rim, n.hgl, n.surcharge_to_surface
        );
    }
    println!();
    let fallback = project.idf_set().design_curve().intensity(project.min_tc);
    for r in network_inlet_pass(&project, &|_| fallback, &InletGeometry::default()) {
        println!(
            "INLET {:4} local={:.6} co_in={:.6} intercepted={:.6} bypass={:.6} spread={:.6} ok={}",
            r.node_id, r.local_cfs, r.carryover_in_cfs, r.intercepted_cfs, r.bypass_cfs, r.spread_ft, r.ok
        );
    }
}
