// SPDX-License-Identifier: GPL-3.0-or-later
//! Degenerate / edge-case networks must be handled without panicking: the
//! analysis should return a clean result or a `NetworkError`, never crash.

use stormsewer::network::AnalysisOptions;
use stormsewer::{IdfCurve, Network, Node, Pipe};

fn idf() -> IdfCurve {
    IdfCurve::new(60.0, 10.0, 0.8)
}

#[test]
fn single_outfall_no_pipes_does_not_panic() {
    let net = Network {
        nodes: vec![Node::outfall("OUT", 100.0, 106.0)],
        pipes: vec![],
    };
    let a = net.analyze(&idf(), &AnalysisOptions::default());
    // Either an empty analysis or a topology error — just not a panic.
    if let Ok(a) = a {
        assert!(a.pipes.is_empty());
    }
}

#[test]
fn zero_area_network_gives_zero_design_flow() {
    // No contributing area anywhere → Rational Q = i·ΣCA = 0.
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 108.0, 0.0, 0.0),
            Node::outfall("OUT", 99.0, 107.0),
        ],
        pipes: vec![Pipe::new("P1", "N1", "OUT", 100.0, 1.5, 0.013)],
    };
    let a = net.analyze(&idf(), &AnalysisOptions::default()).unwrap();
    assert_eq!(a.pipes.len(), 1);
    assert!(a.pipes[0].design_q.abs() < 1e-9, "Q = {}", a.pipes[0].design_q);
    assert!(!a.pipes[0].surcharged);
    // HGL/regime must still be finite (no NaN/inf) even at zero flow.
    for p in &a.pipes {
        if let Some(h) = p.hgl_up {
            assert!(h.is_finite(), "hgl_up not finite");
        }
    }
}

#[test]
fn all_adverse_slopes_do_not_panic_and_flag_errors() {
    // Every pipe runs uphill (invert rises downstream).
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 96.0, 110.0, 2.0, 0.8),
            Node::junction("N2", 98.0, 110.0, 1.0, 0.8),
            Node::outfall("OUT", 100.0, 110.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "N2", 100.0, 1.5, 0.013),
            Pipe::new("P2", "N2", "OUT", 100.0, 1.5, 0.013),
        ],
    };
    let a = net.analyze(&idf(), &AnalysisOptions::default()).unwrap();
    for p in &a.pipes {
        assert!(p.slope < 0.0, "{} slope {}", p.id, p.slope);
        // Adverse pipes report capacity unavailable, not a bogus finite capacity.
        assert!(p.capacity_unavailable());
        // HGL values stay finite.
        assert!(p.hgl_dn.map_or(true, f64::is_finite));
    }
}

#[test]
fn steep_network_is_supercritical_without_panic() {
    use stormsewer::network::FlowRegime;
    // 20% slopes → supercritical open-channel flow throughout.
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 140.0, 150.0, 0.5, 0.6),
            Node::junction("N2", 120.0, 130.0, 0.5, 0.6),
            Node::outfall("OUT", 100.0, 110.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "N2", 100.0, 1.5, 0.013),
            Pipe::new("P2", "N2", "OUT", 100.0, 1.5, 0.013),
        ],
    };
    let opts = AnalysisOptions { intensity_override: Some(2.0), ..Default::default() };
    let a = net.analyze(&idf(), &opts).unwrap();
    assert!(a.pipes.iter().all(|p| p.regime() == FlowRegime::Supercritical));
    assert!(a.pipes.iter().all(|p| p.hgl_up.map_or(true, f64::is_finite)));
}

#[test]
fn disconnected_node_does_not_crash() {
    // An inlet with no pipe out of it plus a normal reach.
    let net = Network {
        nodes: vec![
            Node::inlet("LONELY", 105.0, 112.0, 1.0, 0.7),
            Node::inlet("N1", 100.0, 108.0, 2.0, 0.8),
            Node::outfall("OUT", 99.0, 107.0),
        ],
        pipes: vec![Pipe::new("P1", "N1", "OUT", 100.0, 1.5, 0.013)],
    };
    // Must not panic; the connected reach still analyzes.
    if let Ok(a) = net.analyze(&idf(), &AnalysisOptions::default()) {
        assert!(a.pipes.iter().any(|p| p.id == "P1"));
    }
}
