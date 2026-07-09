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
fn elliptical_full_perimeter_matches_ramanujan() {
    // Independent check of the Simpson-integrated elliptical perimeter against
    // Ramanujan's ellipse-circumference approximation C ≈ π[3(a+b) −
    // √((3a+b)(a+3b))], a = span/2, b = rise/2. For a 4-wide × 3-tall ellipse
    // (a=2, b=1.5): C ≈ π[10.5 − √48.75] = 11.051 ft.
    let sec = Section::Elliptical { rise: 3.0, span: 4.0 };
    let (a, b) = (4.0_f64 / 2.0, 3.0_f64 / 2.0);
    let ramanujan = PI * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).sqrt());
    let p = sec.full_perimeter();
    let rel = (p - ramanujan).abs() / ramanujan;
    assert!(rel < 0.01, "ellipse perimeter {p} vs Ramanujan {ramanujan} (rel {rel:.4})");

    // A circle is the degenerate ellipse: perimeter = πD.
    let circle = Section::Elliptical { rise: 2.0, span: 2.0 };
    assert!((circle.full_perimeter() - PI * 2.0).abs() < 0.02);

    // Full area is exact: π·span·rise/4.
    assert!((sec.full_area() - PI * 4.0 * 3.0 / 4.0).abs() < 1e-9);
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
