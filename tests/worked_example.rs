// SPDX-License-Identifier: GPL-3.0-or-later

//! Worked-example validation — the engine reproduces an independent hand
//! calculation of a two-pipe Rational + Manning network, column for column.
//!
//! A constant design intensity decouples the hydrology from the hydraulics, so
//! every reference value below is closed-form and hand-derivable. The full
//! narrative (network, arithmetic, side-by-side table) is in `WORKED_EXAMPLE.md`;
//! this test is its enforceable form. See `examples/worked_example.rs` to print
//! the comparison.
//!
//! Network:  N1 (4.0 ac, C=0.60) → N2 (3.0 ac, C=0.70) → OUT, both pipes 24-in,
//! n=0.013. Inverts 100.0 / 98.0 / 97.4 over 200-ft reaches give S1=0.010,
//! S2=0.003. Design intensity i = 5.0 in/hr.
//!
//! Hand calculation:
//!   C·A:        N1 = 2.4,  through N2 = 2.4 + 2.1 = 4.5
//!   Design Q:   P1 = 5.0·2.4 = 12.00 cfs;  P2 = 5.0·4.5 = 22.50 cfs
//!   Full cap:   A=π=3.141593 ft², R=D/4=0.5, R^(2/3)=0.629961, k/n=114.6154
//!               P1 (S=0.010): 114.6154·3.141593·0.629961·√0.010 = 22.68 cfs
//!               P2 (S=0.003): 114.6154·3.141593·0.629961·√0.003 = 12.42 cfs
//!   Max cap:    ≈ 1.076 · full  →  P1 = 24.40,  P2 = 13.36 cfs
//!   V_full:     Q_full / A  →  P1 = 22.68/π = 7.22,  P2 = 12.42/π = 3.95 ft/s
//!   % full:     P1 = 12.00/22.68 = 52.9 %  (OK)
//!               P2 = 22.50/12.42 = 181 %   (> max cap → surcharges)

use stormsewer::{Network, Node, Pipe};

fn network() -> Network {
    Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 110.0, 4.0, 0.60),
            Node::inlet("N2", 98.0, 108.0, 3.0, 0.70),
            Node::outfall("OUT", 97.4, 106.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "N2", 200.0, 2.0, 0.013),
            Pipe::new("P2", "N2", "OUT", 200.0, 2.0, 0.013),
        ],
    }
}

#[test]
fn engine_reproduces_hand_calculation() {
    let r = network().analyze_rational(5.0).unwrap();
    let p1 = r.iter().find(|p| p.id == "P1").unwrap();
    let p2 = r.iter().find(|p| p.id == "P2").unwrap();

    // Slopes from inverts.
    assert!((p1.slope - 0.010).abs() < 1e-9, "P1 slope {}", p1.slope);
    assert!((p2.slope - 0.003).abs() < 1e-9, "P2 slope {}", p2.slope);

    // Rational design flows Q = i·C·A.
    assert!((p1.design_q - 12.00).abs() < 1e-6, "P1 Q {}", p1.design_q);
    assert!((p2.design_q - 22.50).abs() < 1e-6, "P2 Q {}", p2.design_q);

    // Manning full-flow capacities.
    assert!((p1.capacity - 22.68).abs() < 0.05, "P1 cap {}", p1.capacity);
    assert!((p2.capacity - 12.42).abs() < 0.05, "P2 cap {}", p2.capacity);

    // Full-flow velocities.
    assert!((p1.velocity_full - 7.22).abs() < 0.03, "P1 Vfull {}", p1.velocity_full);
    assert!((p2.velocity_full - 3.95).abs() < 0.03, "P2 Vfull {}", p2.velocity_full);

    // Percent full.
    assert!((p1.pct_full - 0.529).abs() < 0.005, "P1 %full {}", p1.pct_full);

    // Surcharge determination: P1 conveys its flow open-channel, P2 cannot.
    assert!(!p1.surcharged, "P1 should not surcharge");
    assert!(p1.normal_depth.is_some(), "P1 should have a normal depth");
    assert!(p2.surcharged, "P2 should surcharge (Q > max capacity)");
    assert!(p2.normal_depth.is_none(), "P2 has no open-channel normal depth");
}
