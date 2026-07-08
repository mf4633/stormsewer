// SPDX-License-Identifier: GPL-3.0-or-later

//! Geometry-aware bend loss at a structure. When flow changes direction at a
//! junction, the structure loss gains a term `bend_loss_coeff · (1 − cos Δ)/2 ·
//! V²/2g`, with Δ the deflection between the incoming and outgoing pipe taken
//! from node coordinates. Validated by hand for a 90° turn, and shown to have no
//! effect on a straight run.

use stormsewer::network::AnalysisOptions;
use stormsewer::{IdfCurve, Network, Node, Pipe};

// N1 → MH → OUT, both 18-in reaches surcharged at Q = 18 cfs.
//   V = 18 / (πD²/4) = 10.186 ft/s,  V²/2g = 1.6111 ft.
// A 90° deflection gives (1 − cos 90°)/2 = 0.5, so with bend_loss_coeff = 1.0 the
// extra head at the bend structure is 0.5 · V²/2g = 0.8055 ft.
fn bent_network() -> Network {
    Network {
        nodes: vec![
            Node::inlet("N1", 98.0, 120.0, 4.0, 0.90).at(0.0, 100.0), // C·A = 3.6
            Node::junction("MH", 97.0, 118.0, 0.0, 0.0).at(0.0, 0.0), // 90° turn here
            Node::outfall("OUT", 96.0, 116.0).at(100.0, 0.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "MH", 200.0, 1.5, 0.013),
            Pipe::new("P2", "MH", "OUT", 200.0, 1.5, 0.013),
        ],
    }
}

fn opts(bend: f64) -> AnalysisOptions {
    AnalysisOptions {
        intensity_override: Some(5.0), // Q = 5 · 3.6 = 18 cfs
        tailwater: Some(100.0),
        junction_k: 0.5,
        bend_loss_coeff: bend,
        ..Default::default()
    }
}

fn hgl_n1(net: &Network, bend: f64) -> f64 {
    let a = net.analyze(&IdfCurve::new(0.0, 1.0, 1.0), &opts(bend)).unwrap();
    a.nodes.iter().find(|n| n.id == "N1").unwrap().hgl
}

#[test]
fn ninety_degree_bend_adds_half_velocity_head() {
    let net = bent_network();
    let base = hgl_n1(&net, 0.0); // bend disabled
    let bent = hgl_n1(&net, 1.0); // full bend coefficient

    let v = 18.0 / (std::f64::consts::PI * 1.5 * 1.5 / 4.0);
    let expected = 0.5 * v * v / (2.0 * 32.2); // (1 − cos90)/2 · V²/2g = 0.8055 ft
    assert!((expected - 0.8055).abs() < 0.01, "hand value {expected}");
    assert!(
        ((bent - base) - expected).abs() < 0.01,
        "bend added {} ft at the 90° turn, expected {expected}",
        bent - base
    );
}

#[test]
fn straight_run_has_no_bend_loss() {
    // Collinear N1 → MH → OUT: zero deflection, so bend_loss_coeff is inert.
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 98.0, 120.0, 4.0, 0.90).at(0.0, 0.0),
            Node::junction("MH", 97.0, 118.0, 0.0, 0.0).at(100.0, 0.0),
            Node::outfall("OUT", 96.0, 116.0).at(200.0, 0.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "MH", 100.0, 1.5, 0.013),
            Pipe::new("P2", "MH", "OUT", 100.0, 1.5, 0.013),
        ],
    };
    assert!((hgl_n1(&net, 0.0) - hgl_n1(&net, 1.0)).abs() < 1e-9, "straight run must be unaffected");
}
