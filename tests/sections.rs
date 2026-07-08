// SPDX-License-Identifier: GPL-3.0-or-later

//! Non-circular conduits are solved on their own geometry through the full
//! network analysis — not approximated by an equal-area circle.

use std::f64::consts::PI;

use stormsewer::hydraulics::{full_flow_capacity, section_full_capacity, Section, K_MANNING_US};
use stormsewer::{Network, Node, Pipe};

const K: f64 = K_MANNING_US;

fn one_pipe(pipe: Pipe) -> Network {
    Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 112.0, 6.0, 0.90),
            Node::outfall("OUT", 99.0, 110.0), // slope 0.01 over 100 ft
        ],
        pipes: vec![pipe],
    }
}

#[test]
fn box_pipe_uses_true_rectangular_geometry() {
    // 2 ft (rise) × 3 ft (span) box.
    let net = one_pipe(Pipe::rectangular("P1", "N1", "OUT", 100.0, 2.0, 3.0, 0.013));
    let a = net.analyze_rational(5.0).unwrap();
    let p = &a[0];

    // The reported capacity is the true rectangular full-flow capacity…
    let sec = Section::Rectangular { rise: 2.0, span: 3.0 };
    let expected = section_full_capacity(&sec, 0.013, 0.01, K);
    assert!((p.capacity - expected).abs() < 1e-6, "box capacity {} vs {}", p.capacity, expected);

    // …and it is materially different from an equal-area circular pipe, proving
    // the geometry is not being collapsed to an equivalent circle.
    let d_eq = (4.0 * 2.0 * 3.0 / PI).sqrt();
    let circ_cap = full_flow_capacity(0.013, 0.01, d_eq, K);
    assert!(
        (p.capacity - circ_cap).abs() > 1.0,
        "box {} should differ from equal-area circle {}",
        p.capacity,
        circ_cap
    );
}

#[test]
fn elliptical_equal_axes_matches_circular_network() {
    // A span==rise ellipse is a circle — the network must produce the same
    // design flow, capacity, and velocity as the circular pipe.
    let ell = one_pipe(Pipe::elliptical("P1", "N1", "OUT", 100.0, 2.0, 2.0, 0.013))
        .analyze_rational(5.0)
        .unwrap();
    let cir = one_pipe(Pipe::new("P1", "N1", "OUT", 100.0, 2.0, 0.013))
        .analyze_rational(5.0)
        .unwrap();

    assert!((ell[0].design_q - cir[0].design_q).abs() < 1e-9);
    assert!((ell[0].capacity - cir[0].capacity).abs() < 0.02, "cap {} vs {}", ell[0].capacity, cir[0].capacity);
    assert!((ell[0].velocity - cir[0].velocity).abs() < 0.02);
}

#[test]
fn arch_pipe_uses_true_arch_geometry() {
    // 3 ft (rise) × 4 ft (span) arch: radius 2, 1-ft walls.
    let net = one_pipe(Pipe::arch("P1", "N1", "OUT", 100.0, 3.0, 4.0, 0.013));
    let a = net.analyze_rational(5.0).unwrap();
    let p = &a[0];

    let sec = Section::Arch { rise: 3.0, span: 4.0 };
    let expected = section_full_capacity(&sec, 0.013, 0.01, K);
    assert!((p.capacity - expected).abs() < 1e-6, "arch capacity {} vs {}", p.capacity, expected);

    // Differs from the equal-area circle → true geometry, not an equivalent pipe.
    let d_eq = (4.0 * sec.full_area() / PI).sqrt();
    let circ_cap = full_flow_capacity(0.013, 0.01, d_eq, K);
    assert!((p.capacity - circ_cap).abs() > 0.5, "arch {} vs equal-area circle {}", p.capacity, circ_cap);
}
