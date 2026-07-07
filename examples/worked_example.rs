// SPDX-License-Identifier: GPL-3.0-or-later

//! Worked-example validation: a two-pipe Rational + Manning network whose design
//! flows and pipe capacities are all independently hand-derivable (constant
//! intensity decouples the hydrology from the hydraulics). Run with:
//!
//! ```text
//! cargo run --example worked_example
//! ```
//!
//! The hand calculation is documented in `WORKED_EXAMPLE.md`; `tests/worked_example.rs`
//! asserts the engine reproduces it.

use stormsewer::{Network, Node, Pipe};

fn main() {
    // N1 (4.0 ac, C=0.60 → C·A=2.4)  →  N2 (3.0 ac, C=0.70 → C·A=2.1)  →  OUT
    // Inverts give P1 slope 0.010, P2 slope 0.003.
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 110.0, 4.0, 0.60),
            Node::inlet("N2", 98.0, 108.0, 3.0, 0.70),
            Node::outfall("OUT", 97.4, 106.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "N2", 200.0, 2.0, 0.013), // 24", S=0.010
            Pipe::new("P2", "N2", "OUT", 200.0, 2.0, 0.013), // 24", S=0.003
        ],
    };

    // Constant design intensity i = 5.0 in/hr (Rational: Q = i · C·A).
    let results = net.analyze_rational(5.0).unwrap();

    println!("Worked-example validation — i = 5.0 in/hr, n = 0.013\n");
    println!(
        "{:<4} {:>6} {:>6} {:>7} {:>7} {:>7} {:>7} {:>6} {:>10}",
        "Pipe", "S", "Q", "Cap", "Qmax", "%Full", "Vfull", "Vact", "Surcharged"
    );
    println!("{}", "-".repeat(72));
    for p in &results {
        println!(
            "{:<4} {:>6.3} {:>6.2} {:>7.2} {:>7.2} {:>6.1}% {:>7.2} {:>6.2} {:>10}",
            p.id,
            p.slope,
            p.design_q,
            p.capacity,
            p.max_capacity,
            p.pct_full * 100.0,
            p.velocity_full,
            p.velocity,
            p.surcharged,
        );
    }
}
