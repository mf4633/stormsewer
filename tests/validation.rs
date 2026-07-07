// SPDX-License-Identifier: GPL-3.0-or-later

//! Analytical validation of the core hydraulics and hydrology methods.
//!
//! Unlike the unit tests (which mostly check internal consistency and ranges),
//! every case here pins an engine result to a **closed-form or hand-derived
//! reference value**, with the derivation shown in the comment. This is the
//! evidence that the numbers are correct, not merely self-consistent — the
//! basis any reviewer needs to trust the engine.
//!
//! Governing methods and references:
//!   * Manning's equation — `Q = (k/n) A R^(2/3) S^(1/2)`, k = 1.49 (US).
//!   * Circular partial-flow geometry — exact central-angle relations
//!     (Brater & King, *Handbook of Hydraulics*; FHWA HDS-5).
//!   * Critical flow — Froude = 1, i.e. `Q^2 T = g A^3`.
//!   * Rational method — `Q = C i A` (peak discharge, US customary acre·in/hr ≈ cfs).
//!   * Time of concentration — Kirpich (1940); NRCS TR-55 (1986) sheet flow.

use stormsewer::hydraulics::{
    circular_geometry, circular_q, critical_depth, full_flow_capacity, max_capacity, normal_depth,
    G_US, K_MANNING_US,
};
use stormsewer::hydrology::{kirpich_minutes, tr55_sheet_flow_minutes};
use stormsewer::{Network, Node, Pipe};

const K: f64 = K_MANNING_US;

/// Manning full-flow capacity of a 24-in pipe.
///
/// D = 2.0 ft, n = 0.013, S = 0.005.
///   A = πD²/4 = π            = 3.141593 ft²
///   R = D/4   = 0.5 ft   →   R^(2/3) = 0.629961
///   Q = (1.49/0.013)·A·R^(2/3)·√0.005
///     = 114.6154 · 3.141593 · 0.629961 · 0.0707107
///     = 16.04 cfs
#[test]
fn manning_full_flow_circular_matches_hand_calc() {
    let q = full_flow_capacity(0.013, 0.005, 2.0, K);
    assert!((q - 16.04).abs() < 0.02, "full-flow Q = {q}, expected 16.04 cfs");
}

/// A circular pipe flowing exactly half full carries exactly half its full-flow
/// discharge — because the hydraulic radius at half depth equals the full-flow
/// radius: R_half = A_half/P_half = (πD²/8)/(πD/2) = D/4 = R_full. With the same
/// R and slope, Q scales with area, so Q_half = ½ Q_full.
#[test]
fn half_full_carries_half_capacity() {
    let (n, s, d) = (0.013, 0.005, 2.0);
    let q_full = full_flow_capacity(n, s, d, K);
    let q_half = circular_q(n, s, d, d / 2.0, K);
    assert!(
        (q_half - 0.5 * q_full).abs() < 1e-6,
        "Q_half = {q_half}, expected {} (½ of full)",
        0.5 * q_full
    );
}

/// Maximum open-channel discharge of a circular conduit occurs near y/D ≈ 0.938
/// and exceeds the just-full value by ≈ 7.6 % (standard partial-flow result,
/// HDS-5 / Brater & King). This guards the non-obvious fact that a pipe conveys
/// *more* than its full-flow capacity just below the crown.
#[test]
fn max_open_channel_capacity_is_1076_of_full() {
    let (n, s, d) = (0.013, 0.005, 2.0);
    let q_full = full_flow_capacity(n, s, d, K);
    let (q_max, y_max) = max_capacity(n, s, d, K);
    let ratio = q_max / q_full;
    assert!((ratio - 1.076).abs() < 0.01, "Q_max/Q_full = {ratio}, expected ≈1.076");
    assert!((y_max / d - 0.938).abs() < 0.01, "y_max/D = {}, expected ≈0.938", y_max / d);
}

/// Critical depth must satisfy the Froude-number-unity condition exactly:
/// `Q² T = g A³`. We solve for y_c, then confirm the defining equation holds
/// (equivalently, Fr = Q√T / √(g A³) = 1) — an analytical self-check independent
/// of how the solver arrives at the depth.
#[test]
fn critical_depth_satisfies_froude_unity() {
    let (q, d) = (10.0, 2.0);
    let yc = critical_depth(q, d, G_US);
    let (a, _p, _r, t) = circular_geometry(yc, d);
    let froude = q * t.sqrt() / (G_US * a * a * a).sqrt();
    assert!((froude - 1.0).abs() < 1e-3, "Froude at y_c = {froude}, expected 1.0");
    assert!(yc > 0.0 && yc < d, "y_c = {yc} out of range");
}

/// Normal (uniform-flow) depth is the inverse of the discharge relation:
/// building Q from a known depth and recovering the depth must round-trip.
#[test]
fn normal_depth_inverts_discharge_exactly() {
    let (n, s, d, y0) = (0.013, 0.01, 2.0, 1.2);
    let q = circular_q(n, s, d, y0, K);
    let y = normal_depth(q, n, s, d, K).expect("below capacity");
    assert!((y - y0).abs() < 1e-3, "recovered y = {y}, expected {y0}");
}

/// Rational-method peak flow accumulates C·A down the network and multiplies by
/// intensity: Q = C i A. Two inlets (C·A = 1.4 and 2.4 ac) under i = 4 in/hr:
///   P1 carries 4 · 1.4 = 5.6 cfs;  P2 carries 4 · (1.4+2.4) = 15.2 cfs.
#[test]
fn rational_peak_flow_matches_ci_a() {
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 105.0, 2.0, 0.7), // C·A = 1.4
            Node::inlet("N2", 99.0, 104.0, 3.0, 0.8),  // C·A = 2.4
            Node::outfall("OUT", 98.0, 103.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "N2", 100.0, 3.0, 0.013),
            Pipe::new("P2", "N2", "OUT", 100.0, 3.0, 0.013),
        ],
    };
    let r = net.analyze_rational(4.0).unwrap();
    let q = |id: &str| r.iter().find(|p| p.id == id).unwrap().design_q;
    assert!((q("P1") - 5.6).abs() < 1e-6, "P1 Q = {}", q("P1"));
    assert!((q("P2") - 15.2).abs() < 1e-6, "P2 Q = {}", q("P2"));
}

/// Kirpich (1940): t_c = 0.0078 · L^0.77 · S^-0.385  (minutes, L in ft).
/// L = 500 ft, S = 0.01:
///   500^0.77 = 119.75 ;  0.01^-0.385 = 5.888
///   t = 0.0078 · 119.75 · 5.888 = 5.50 min
#[test]
fn kirpich_matches_published_formula() {
    let t = kirpich_minutes(500.0, 0.01);
    assert!((t - 5.50).abs() < 0.02, "Kirpich t_c = {t}, expected 5.50 min");
}

/// NRCS TR-55 (1986) sheet flow: t = 0.42·(nL)^0.8 / (P2^0.5 · S^0.4)  (minutes).
/// n = 0.011, L = 200 ft, S = 0.01, P2 = 2.5 in:
///   (nL)^0.8 = 2.2^0.8 = 1.879 ;  P2^0.5 = 1.5811 ;  S^0.4 = 0.15849
///   t = 0.42 · 1.879 / (1.5811 · 0.15849) = 3.15 min
#[test]
fn tr55_sheet_flow_matches_published_formula() {
    let t = tr55_sheet_flow_minutes(200.0, 0.01, 0.011, 2.5);
    assert!((t - 3.15).abs() < 0.03, "TR-55 sheet-flow t = {t}, expected 3.15 min");
}
