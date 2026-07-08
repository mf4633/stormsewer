// SPDX-License-Identifier: GPL-3.0-or-later

//! Validation against **externally published** worked examples — not the
//! project's own hand calculations. These are standard worked problems from
//! widely used engineering references; the engine independently reproduces the
//! published answers, which is stronger corroboration than an internal check.
//!
//! Sources (public engineering references / continuing-education courses):
//!   * Manning circular partial-flow example — 8-in sewer, S = 0.0033,
//!     n = 0.013, Q = 0.525 cfs → published normal depth 0.433 ft (5.2 in).
//!     (H. Bengtson, "Manning Equation for Open Channels", and equivalent
//!     partially-full-pipe worked examples.)
//!   * Rational method example — downtown business area 35,400 ft² (0.813 ac),
//!     C = 0.85, i = 5.1 in/hr → published peak Q = 3.52 cfs.

use stormsewer::hydraulics::{circular_geometry, normal_depth, K_MANNING_US};
use stormsewer::{Network, Node, Pipe};

const K: f64 = K_MANNING_US;

/// Reproduces the published partially-full circular-pipe result to within the
/// published rounding (0.433 ft). This exercises the iterative normal-depth
/// solver and the exact circular geometry against an independent answer.
#[test]
fn manning_partial_flow_8in_sewer_matches_published() {
    let (d, n, s, q) = (8.0 / 12.0, 0.013, 0.0033, 0.525);
    let yn = normal_depth(q, n, s, d, K).expect("open-channel flow");
    assert!(
        (yn - 0.433).abs() < 0.002,
        "normal depth {yn:.4} ft, published 0.433 ft"
    );
    // Velocity at that depth is a sane sewer value (~2 ft/s).
    let (area, _p, _r, _t) = circular_geometry(yn, d);
    let v = q / area;
    assert!(v > 1.5 && v < 3.0, "velocity {v:.2} ft/s");
}

/// Reproduces the published Rational-method peak flow, including the
/// square-feet → acres conversion, through the network's C·A accumulation.
#[test]
fn rational_business_area_matches_published() {
    let acres = 35_400.0 / 43_560.0; // 0.8127 ac
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 105.0, acres, 0.85),
            Node::outfall("OUT", 99.0, 104.0),
        ],
        pipes: vec![Pipe::new("P1", "N1", "OUT", 100.0, 2.0, 0.013)],
    };
    // Rational analysis at the design intensity i = 5.1 in/hr.
    let q = net.analyze_rational(5.1).unwrap()[0].design_q;
    assert!((q - 3.52).abs() < 0.01, "peak flow {q:.3} cfs, published 3.52 cfs");
}
