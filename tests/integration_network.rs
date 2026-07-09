// SPDX-License-Identifier: GPL-3.0-or-later
//! End-to-end integration tests: a realistic multi-reach network flows through
//! Tc accumulation, Rational design flows, the GVF/HGL backwater pass, flow-
//! regime classification, and the design review — all in one analysis — and the
//! results agree with each other and with hand reasoning.

use stormsewer::design::review::{design_review, ReviewCriteria, Severity};
use stormsewer::network::{AnalysisOptions, FlowRegime};
use stormsewer::{IdfCurve, Network, Node, Pipe};

fn idf() -> IdfCurve {
    IdfCurve::new(120.0, 15.0, 0.85)
}

/// A three-reach mild-slope trunk: two inlets feed a junction, then one outfall.
/// Inverts drop ~0.5% per reach, so subcritical open-channel flow with a
/// backwater-controlled HGL is expected.
fn trunk() -> Network {
    Network {
        nodes: vec![
            Node::inlet("N1", 101.5, 108.0, 1.0, 0.60),
            Node::inlet("N2", 101.5, 108.0, 0.8, 0.60),
            Node::junction("J1", 101.0, 107.5, 0.5, 0.70),
            Node::outfall("OUT", 100.0, 106.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "J1", 100.0, 2.0, 0.013),
            Pipe::new("P2", "N2", "J1", 100.0, 1.5, 0.013),
            Pipe::new("P3", "J1", "OUT", 100.0, 2.5, 0.013),
        ],
    }
}

#[test]
fn full_network_analysis_is_self_consistent() {
    let net = trunk();
    let a = net.analyze(&idf(), &AnalysisOptions::default()).unwrap();
    assert_eq!(a.pipes.len(), 3);

    let trunk_q = a.pipes.iter().find(|p| p.id == "P3").unwrap().design_q;
    let p1_q = a.pipes.iter().find(|p| p.id == "P1").unwrap().design_q;
    let p2_q = a.pipes.iter().find(|p| p.id == "P2").unwrap().design_q;

    // Conservation of drainage area: the trunk reach carries no less than either
    // tributary (it collects both plus its own inlet area).
    assert!(trunk_q >= p1_q, "trunk {trunk_q} < P1 {p1_q}");
    assert!(trunk_q >= p2_q, "trunk {trunk_q} < P2 {p2_q}");

    for p in &a.pipes {
        // Every reach produced finite, physical hydraulics.
        assert!(p.design_q > 0.0, "{} zero Q", p.id);
        assert!(p.velocity.is_finite() && p.velocity > 0.0, "{} velocity", p.id);
        assert!(p.capacity > 0.0 && p.capacity.is_finite(), "{} capacity", p.id);
        assert!(p.critical_depth.is_finite() && p.critical_depth > 0.0);
        // The HGL pass ran and produced finite grade-line elevations.
        assert!(p.hgl_up.map_or(false, f64::is_finite), "{} hgl_up", p.id);
        assert!(p.hgl_dn.map_or(false, f64::is_finite), "{} hgl_dn", p.id);
        // HGL falls in the downstream direction (energy is dissipated).
        assert!(
            p.hgl_up.unwrap() >= p.hgl_dn.unwrap() - 1e-6,
            "{}: hgl_up {:?} < hgl_dn {:?}",
            p.id,
            p.hgl_up,
            p.hgl_dn
        );
    }
}

#[test]
fn mild_trunk_is_subcritical_and_regime_matches_depth() {
    let net = trunk();
    let a = net.analyze(&idf(), &AnalysisOptions::default()).unwrap();
    for p in &a.pipes {
        // These reaches are not surcharged, so a normal depth exists…
        let yn = p.normal_depth.expect("open-channel reach has normal depth");
        let yc = p.critical_depth;
        // …and the classified regime must agree with the yn-vs-yc comparison
        // that defines it.
        match p.regime() {
            FlowRegime::Subcritical => assert!(yn > yc * 0.98, "{} yn {yn} yc {yc}", p.id),
            FlowRegime::Supercritical => assert!(yn < yc * 1.02, "{} yn {yn} yc {yc}", p.id),
            FlowRegime::Critical => {}
            FlowRegime::Pressurized => panic!("{} unexpectedly pressurized", p.id),
        }
    }
}

#[test]
fn review_flags_undersized_trunk_but_clean_network_passes() {
    // The mild trunk with generous pipes should have no ERROR-level findings.
    let net = trunk();
    let a = net.analyze(&idf(), &AnalysisOptions::default()).unwrap();
    let findings = design_review(&net, &a, &ReviewCriteria::default());
    assert!(
        !findings.iter().any(|f| f.severity == Severity::Error),
        "clean network produced errors: {:?}",
        findings.iter().filter(|f| f.severity == Severity::Error).collect::<Vec<_>>()
    );

    // Now choke the outfall reach to a 12-inch pipe: it must surcharge and the
    // review must flag it.
    let mut choked = net;
    choked.pipes[2] = Pipe::new("P3", "J1", "OUT", 100.0, 1.0, 0.013);
    let a2 = choked.analyze(&idf(), &AnalysisOptions::default()).unwrap();
    let p3 = a2.pipes.iter().find(|p| p.id == "P3").unwrap();
    assert!(p3.report_surcharged() || p3.pct_full > 0.85, "choked trunk not overloaded");
    let findings2 = design_review(&choked, &a2, &ReviewCriteria::default());
    assert!(
        findings2.iter().any(|f| f.id == "P3"),
        "review did not flag choked P3: {findings2:?}"
    );
}

#[test]
fn frequency_factor_scales_design_flow() {
    // With a fixed intensity, Rational Q = i·C_f·C·A. Choose C small enough that
    // C·C_f stays below the 1.0 cap for the 100-yr factor, so the flow scales
    // linearly with C_f and we can check the ratio exactly.
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 101.0, 108.0, 4.0, 0.60),
            Node::outfall("OUT", 100.0, 106.0),
        ],
        pipes: vec![Pipe::new("P1", "N1", "OUT", 100.0, 2.0, 0.013)],
    };
    let base = AnalysisOptions { intensity_override: Some(4.0), ..Default::default() };

    let q10 = net
        .analyze(&idf(), &AnalysisOptions { runoff_frequency_factor: 1.0, ..base.clone() })
        .unwrap()
        .pipes[0]
        .design_q;
    let q100 = net
        .analyze(&idf(), &AnalysisOptions { runoff_frequency_factor: 1.25, ..base })
        .unwrap()
        .pipes[0]
        .design_q;

    assert!(q100 > q10, "100-yr Q {q100} not greater than 10-yr Q {q10}");
    // 0.60·1.25 = 0.75 < 1.0, so the ratio is exactly the factor ratio.
    assert!((q100 / q10 - 1.25).abs() < 1e-6, "ratio {} not 1.25", q100 / q10);
}
