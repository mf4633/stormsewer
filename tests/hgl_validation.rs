// SPDX-License-Identifier: GPL-3.0-or-later

//! HGL / backwater validation — the engine reproduces a hand-derived
//! hydraulic-grade-line calculation for a surcharged pipe under tailwater.
//!
//! This covers the iterative/energy piece that `worked_example.rs` deliberately
//! omits. A single 18-inch pipe carries 20 cfs from N1 to a free outfall held at
//! a tailwater of 100.0 ft; the flow exceeds the pipe's capacity, so it flows
//! full and the HGL is driven by friction plus the structure loss.
//!
//! Given: D = 1.5 ft, n = 0.013, L = 300 ft, Q = 20.0 cfs (= i·C·A = 5.0·4.0),
//! inverts N1 = 100.0 / OUT = 96.0 → S = 4/300 = 0.013333, tailwater = 100.0,
//! junction K = 0.5.
//!
//! Hand calculation of HGL at N1 (pressurized reach):
//!   A_full   = πD²/4 = 1.767146 ft²        R = D/4 = 0.375, R^(2/3) = 0.520014
//!   K_full   = (1.49/0.013)·A·R^(2/3) = 105.33   (so Q_full = K_full·√S)
//!   S_f      = (Q / K_full)² = (20 / 105.33)² = 0.036058
//!   h_f      = S_f · L = 0.036058 · 300 = 10.817 ft
//!   WS_dn    = max(tailwater, crown) = max(100.0, 96.0+1.5) = 100.0 ft
//!   V        = Q / A_full = 20 / 1.767146 = 11.317 ft/s
//!   h_j      = K·V²/2g = 0.5·11.317²/64.4 = 0.994 ft
//!   HGL(N1)  = WS_dn + h_f + h_j = 100.0 + 10.817 + 0.994 = 111.81 ft
//!
//! The rim at N1 is 108.0 ft, so HGL (111.81) is above the rim → surface
//! flooding, which the engine must also flag.

use stormsewer::network::AnalysisOptions;
use stormsewer::{IdfCurve, Network, Node, Pipe};

#[test]
fn hgl_matches_hand_backwater() {
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 108.0, 5.0, 0.80), // C·A = 4.0
            Node::outfall("OUT", 96.0, 106.0),
        ],
        pipes: vec![Pipe::new("P1", "N1", "OUT", 300.0, 1.5, 0.013)],
    };
    let opts = AnalysisOptions {
        intensity_override: Some(5.0), // Q = 5.0 · 4.0 = 20.0 cfs
        tailwater: Some(100.0),
        junction_k: 0.5,
        ..Default::default()
    };
    let a = net.analyze(&IdfCurve::new(0.0, 1.0, 1.0), &opts).unwrap();

    let p1 = &a.pipes[0];
    assert!(p1.surcharged, "P1 should surcharge (20 cfs > capacity)");
    assert!((p1.design_q - 20.0).abs() < 1e-6, "Q = {}", p1.design_q);

    // Downstream water surface is the tailwater; upstream HGL is the hand value.
    assert!(
        (p1.hgl_dn.unwrap() - 100.0).abs() < 1e-6,
        "HGL dn = {:?}",
        p1.hgl_dn
    );
    assert!(
        (p1.hgl_up.unwrap() - 111.81).abs() < 0.05,
        "HGL up = {:?}, expected 111.81 ft",
        p1.hgl_up
    );

    // Node HGL and the surface-flooding flag (HGL 111.81 > rim 108.0).
    let n1 = a.nodes.iter().find(|n| n.id == "N1").unwrap();
    assert!((n1.hgl - 111.81).abs() < 0.05, "N1 HGL = {}", n1.hgl);
    assert!(n1.surcharge_to_surface, "N1 should flag surface flooding");
}
