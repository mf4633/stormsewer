// SPDX-License-Identifier: GPL-3.0-or-later

//! SI-unit-path validation. The engine always computes in US customary; the
//! Project layer converts stored SI values (m, mm, ha, mm/hr) back to engine
//! feet/acres/in-hr before analysis. This test pins that the physics is
//! unit-consistent: toggling U.S. ↔ SI must not change the answer.
//!
//! It would catch the two classic unit bugs — a Rational flow off by the metric
//! 1/360 factor (design flows would diverge) or a Manning capacity off by the
//! 1.49 US↔SI coefficient (capacities would shift ~49%).

use stormsewer::io::Project;
use stormsewer::network::AnalysisOptions;
use stormsewer::units::{convert_project, UnitSystem};

// A fixed design intensity removes time-of-concentration coupling (downstream Tc
// depends on pipe travel time → velocity → diameter, and metric diameters snap
// to the catalog). With intensity fixed, design flow is exactly i·ΣCA, so any
// difference across unit systems is a genuine conversion bug, not physics.
const I: f64 = 5.0; // in/hr (engine units) applied identically to both systems

fn flows(p: &Project) -> Vec<(String, f64, f64)> {
    let net = p.to_network();
    let opts = AnalysisOptions {
        intensity_override: Some(I),
        ..p.options()
    };
    let a = net.analyze(&p.idf(), &opts).expect("analyze");
    a.pipes
        .iter()
        .map(|r| (r.id.clone(), r.design_q, r.capacity))
        .collect()
}

#[test]
fn si_toggle_preserves_flows_and_capacity() {
    let mut p = Project::demo();
    assert_eq!(p.units, UnitSystem::UsCustomary);
    let us = flows(&p);
    assert!(!us.is_empty());

    convert_project(&mut p, UnitSystem::Si);
    assert_eq!(p.units, UnitSystem::Si);
    let si = flows(&p);

    assert_eq!(us.len(), si.len(), "pipe count preserved");
    for ((id, q_us, cap_us), (_, q_si, cap_si)) in us.iter().zip(si.iter()) {
        // Design flow (i·ΣCA) must be identical — area converts exactly (ac↔ha),
        // catching e.g. a metric Rational 1/360 factor bug.
        assert!(
            (q_us - q_si).abs() < 1e-6,
            "{id}: design Q differs across units — US {q_us} vs SI {q_si}"
        );
        // Capacity may shift a little as diameters snap to the metric catalog,
        // but nowhere near the 1.49 Manning factor a units bug would introduce.
        let rel = (cap_us - cap_si).abs() / cap_us.max(1e-9);
        assert!(
            rel < 0.15,
            "{id}: capacity differs too much — US {cap_us} vs SI {cap_si} (rel {rel:.3})"
        );
    }

    // Round-trip back to US restores the original values exactly.
    convert_project(&mut p, UnitSystem::UsCustomary);
    let back = flows(&p);
    for ((id, q0, c0), (_, q1, c1)) in us.iter().zip(back.iter()) {
        assert!((q0 - q1).abs() < 1e-6, "{id}: round-trip Q {q0} vs {q1}");
        assert!((c0 - c1).abs() < 1e-6, "{id}: round-trip capacity {c0} vs {c1}");
    }
}
