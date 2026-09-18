// SPDX-License-Identifier: GPL-3.0-or-later

//! The 1D-2D interface bookkeeping: volume leaving one side is exactly
//! the volume arriving on the other, capture never exceeds the weir or
//! orifice limit nor what the cell holds, and bank flow splits to the
//! link's end nodes.

use std::path::Path;

use stormsewer_swmm::gis::raster::Raster;
use stormsewer_swmm::twod::couple::{
    apply_node_exchange, bank_exchange_volumes, node_exchange, NodeState1D,
};
use stormsewer_swmm::twod::interfaces::{inlet_capture, orifice_flow, weir_flow};
use stormsewer_swmm::twod::solver::{Simulator, Sink};
use stormsewer_swmm::twod::{
    self, BankInterface, Config, InterfaceKind, LocatedBank, LocatedNode, NodeInterface, Results,
    Setup,
};

fn plate(n: usize, ground: f64) -> Raster {
    Raster::filled(n, n, 0.0, 0.0, 1.0, ground)
}

fn manhole(name: &str, col: usize, row: usize, ground: f64, lid_open: bool) -> LocatedNode {
    LocatedNode {
        interface: NodeInterface {
            node: name.into(),
            kind: InterfaceKind::Manhole,
            weir_coeff: None,
            lid_open,
        },
        col,
        row,
        ground,
        rim: ground,
    }
}

fn inlet(name: &str, col: usize, row: usize, ground: f64, perimeter: f64, area: f64) -> LocatedNode {
    LocatedNode {
        interface: NodeInterface {
            node: name.into(),
            kind: InterfaceKind::Inlet { perimeter, area },
            weir_coeff: None,
            lid_open: false,
        },
        col,
        row,
        ground,
        rim: ground,
    }
}

// ---------------------------------------------------------------------------
// Node interfaces conserve volume in both directions.
// ---------------------------------------------------------------------------
#[test]
fn node_interface_conserves_volume_both_ways() {
    let bed = plate(11, 100.0);
    let mut sim = Simulator::new(&bed, 0.03, true, 0.003, 0.7);
    let node = manhole("J1", 5, 5, 100.0, true);
    let dt = 5.0;

    // 1. The engine reports flooding: that water is placed on the surface
    //    as it is (CMS → m³/s factor 1).
    let ex = node_exchange(
        &node,
        true,
        9.81,
        0.003,
        0.0,
        NodeState1D {
            head: 100.0,
            overflow: 0.4, ..Default::default()
        },
        1.0,
    );
    assert_eq!(ex.to_surface, 0.4);
    assert_eq!(ex.engine_overflow, 0.4);
    assert_eq!(ex.to_network, 0.0);
    let (vs, vn) = apply_node_exchange(&mut sim, &node, &ex, dt);
    assert!((vs - 2.0).abs() < 1e-12);
    assert_eq!(vn, 0.0);
    assert!((sim.stored() - 2.0).abs() < 1e-12, "the cell holds exactly the 1D loss");
    assert!((sim.balance.inflow - 2.0).abs() < 1e-12);

    // 2. Head above the surface with no flooding reported: weir formula.
    let ex = node_exchange(
        &node,
        true,
        9.81,
        0.003,
        0.0,
        NodeState1D {
            head: 100.5,
            overflow: 0.0, ..Default::default()
        },
        1.0,
    );
    let d = 0.6f64;
    let expected = weir_flow(9.81, 0.6, std::f64::consts::PI * d, 0.5);
    assert!((ex.to_surface - expected).abs() < 1e-12);
    assert_eq!(ex.engine_overflow, 0.0);
    // With the 2 m already on the cell the lid is submerged: orifice.
    let ex_sub = node_exchange(
        &node,
        true,
        9.81,
        0.003,
        sim.depth(5, 5),
        NodeState1D {
            head: 102.5,
            overflow: 0.0, ..Default::default()
        },
        1.0,
    );
    let orif = orifice_flow(9.81, 0.6, std::f64::consts::PI * d * d / 4.0, 0.5);
    assert!((ex_sub.to_surface - orif).abs() < 1e-12);
    // 3. Re-entry through the open lid: what leaves the cell is what the
    //    network is told, and it is capped at the cell's content.
    let before = sim.stored();
    let ex = node_exchange(
        &node,
        true,
        9.81,
        0.003,
        sim.depth(5, 5),
        NodeState1D {
            head: 99.0,
            overflow: 0.0, ..Default::default()
        },
        1.0,
    );
    assert!(ex.to_network > 0.0);
    assert_eq!(ex.to_surface, 0.0);
    let (vs, vn) = apply_node_exchange(&mut sim, &node, &ex, 1000.0);
    assert_eq!(vs, 0.0);
    assert!(vn <= before + 1e-12, "cannot take more than the cell holds");
    assert!((before - sim.stored() - vn).abs() < 1e-12);
    assert!((sim.balance.outflow - vn).abs() < 1e-12);
    // Sealed nodes exchange nothing.
    let mut sealed = node.clone();
    sealed.interface.kind = InterfaceKind::Sealed;
    let ex = node_exchange(&sealed, true, 9.81, 0.003, 1.0, NodeState1D { head: 105.0, overflow: 1.0, ..Default::default() }, 1.0);
    assert_eq!(ex, Default::default());
}

// ---------------------------------------------------------------------------
// An inlet captures no more than the weir/orifice limit.
// ---------------------------------------------------------------------------
#[test]
fn inlet_capture_never_exceeds_the_weir_or_orifice_limit() {
    let g = 32.174;
    let node = inlet("I1", 3, 3, 500.0, 6.0, 1.5);
    for depth in [0.01, 0.05, 0.2, 0.5, 1.0, 2.0] {
        let ex = node_exchange(
            &node,
            false,
            g,
            0.003,
            depth,
            NodeState1D {
                head: 495.0,
                overflow: 0.0, ..Default::default()
            },
            1.0,
        );
        let weir = weir_flow(g, 0.6, 6.0, depth);
        let orifice = orifice_flow(g, 0.6, 1.5, depth);
        assert!(ex.to_network <= weir + 1e-12, "depth {depth}: {} > weir {weir}", ex.to_network);
        assert!(ex.to_network <= orifice + 1e-12, "depth {depth}: {} > orifice {orifice}", ex.to_network);
        assert!((ex.to_network - weir.min(orifice)).abs() < 1e-12);
    }
    // A node whose head is at the surface takes nothing.
    let full = node_exchange(&node, false, g, 0.003, 0.5, NodeState1D { head: 500.5, overflow: 0.0, ..Default::default() }, 1.0);
    assert_eq!(full.to_network, 0.0);
    // Direct check of the regime switch: shallow → weir, deep → orifice.
    assert!((inlet_capture(g, 0.6, 6.0, 1.5, 0.02, 490.0, 500.0) - weir_flow(g, 0.6, 6.0, 0.02)).abs() < 1e-12);
    assert!((inlet_capture(g, 0.6, 6.0, 1.5, 2.0, 490.0, 500.0) - orifice_flow(g, 0.6, 1.5, 2.0)).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// A sink (outfall interface) is bounded by the cell and booked as outflow.
// ---------------------------------------------------------------------------
#[test]
fn outfall_sink_drains_the_cell_and_is_booked_as_outflow() {
    let bed = plate(5, 10.0);
    let mut sim = Simulator::new(&bed, 0.03, true, 0.003, 0.7);
    sim.set_depth(2, 2, 0.5);
    sim.add_sink(Sink {
        col: 2,
        row: 2,
        stage: 8.0,
        perimeter: 100.0,
        area: 100.0,
        coeff: 0.6,
        node: 0,
    });
    let initial = sim.stored();
    sim.advance_to(60.0);
    assert!(sim.sink_volume[0] > 0.0);
    assert!(sim.sink_volume[0] <= initial + 1e-9);
    assert!((sim.balance.outflow - sim.sink_volume[0]).abs() < 1e-12);
    assert!(sim.mass_error_pct().abs() < 1e-9, "{}", sim.mass_error_pct());
}

// ---------------------------------------------------------------------------
// Bank flow: the volume put on (or taken off) the cells equals the split
// handed to the two end nodes.
// ---------------------------------------------------------------------------
#[test]
fn bank_exchange_splits_exactly_between_the_end_nodes() {
    let bed = plate(20, 10.0);
    let mut sim = Simulator::new(&bed, 0.03, true, 0.003, 0.7);
    let bank = LocatedBank {
        interface: BankInterface {
            link: "C1".into(),
            right: true,
            polyline: vec![(0.5, 5.5), (19.5, 5.5)],
            crest: Some(10.2),
            weir_coeff: None,
        },
        cells: (0..20).map(|c| (c, 4, 10.2)).collect(),
    };
    let (total, v_from, v_to) = bank_exchange_volumes(&mut sim, &bank, 11.0, 10.4, 2.0);
    assert!(total > 0.0);
    assert!((v_from + v_to - total).abs() < 1e-12);
    assert!(v_from > v_to, "more spills where the channel is higher");
    assert!((sim.stored() - total).abs() < 1e-12);
    // Reverse: surface above the channel drains back, capped by the cells.
    let (back, bf, bt) = bank_exchange_volumes(&mut sim, &bank, 9.0, 9.0, 1e6);
    assert!(back < 0.0);
    assert!((bf + bt - back).abs() < 1e-12);
    assert!(sim.stored() >= -1e-12);
    assert!((sim.balance.inflow - sim.balance.outflow - sim.stored()).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// A coupled-style run through `run_surface` records node exchange in the
// results file (positive = onto the surface).
// ---------------------------------------------------------------------------
#[test]
fn node_exchange_is_recorded_per_frame() {
    let dir = std::env::temp_dir().join("stormsewer-twod-coupling").join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("m.inp");
    let bed = plate(15, 100.0);
    let config = Config {
        duration_s: Some(60.0),
        output_step_s: 20.0,
        ..Config::default()
    };
    let mut setup = Setup::from_grid(&model, bed, 0.03, true, config);
    setup.nodes.push(manhole("J1", 7, 7, 100.0, false));
    let mut sim = Simulator::from_setup(&setup);
    let mut given = 0.0;
    let summary = twod::solver::run_surface(&setup, &mut sim, 5.0, &mut |_| true, &mut |sim, t, dt, node_vol| {
        if t < 40.0 {
            let v = sim.add_volume(7, 7, 0.25 * dt);
            node_vol[0] += v;
            given += v;
        }
        Ok(())
    })
    .unwrap();
    assert!((given - 10.0).abs() < 1e-9);
    assert!((summary.inflow - given).abs() < 1e-9);
    assert!(summary.mass_error_pct.abs() < 1e-9);
    let r = Results::open(&summary.results).unwrap();
    assert_eq!(r.node_names, vec!["J1".to_string()]);
    let ex = r.node_exchange("J1").unwrap();
    assert_eq!(ex.len(), 4);
    // Frames at 20 and 40 s each carry 5 m³ over 20 s = 0.25 m³/s.
    assert!((ex[1].1 - 0.25).abs() < 1e-6, "{ex:?}");
    assert!((ex[2].1 - 0.25).abs() < 1e-6, "{ex:?}");
    assert!(ex[3].1.abs() < 1e-6);
    let _ = Path::new(&model);
}

// ---------------------------------------------------------------------------
// Formula surcharge needs the head above the node's own rim, and never takes
// more than the network can give (the -667 % continuity regression).
// ---------------------------------------------------------------------------
#[test]
fn formula_surcharge_respects_the_rim_and_the_withdrawal_cap() {
    let g = 9.81;
    // Ground at 100, but the node's rim (engine full depth) is 102.
    let mut node = manhole("J1", 5, 5, 100.0, false);
    node.rim = 102.0;
    let state = |head: f64, cap: Option<f64>| NodeState1D {
        head,
        overflow: 0.0,
        max_withdrawal: cap,
    };
    // Head above the ground but below the rim: the water is still in the
    // manhole, nothing leaves.
    let ex = node_exchange(&node, true, g, 0.003, 0.0, state(101.5, None), 1.0);
    assert_eq!(ex.to_surface, 0.0);
    // Above the rim: a weir over the lid, on the height above the rim.
    let ex = node_exchange(&node, true, g, 0.003, 0.0, state(102.4, None), 1.0);
    let d = 0.6f64;
    let expected = weir_flow(g, 0.6, std::f64::consts::PI * d, 0.4);
    assert!((ex.to_surface - expected).abs() < 1e-12, "{} vs {expected}", ex.to_surface);
    // The cap bounds it exactly.
    let ex = node_exchange(&node, true, g, 0.003, 0.0, state(102.4, Some(0.01)), 1.0);
    assert_eq!(ex.to_surface, 0.01);
    // Iterative mode passes a zero cap: formula surcharge is off, but the
    // engine's own reported flooding still goes onto the surface.
    let ex = node_exchange(&node, true, g, 0.003, 0.0, state(102.4, Some(0.0)), 1.0);
    assert_eq!(ex.to_surface, 0.0);
    let flooded = NodeState1D { head: 102.4, overflow: 0.3, max_withdrawal: Some(0.0) };
    let ex = node_exchange(&node, true, g, 0.003, 0.0, flooded, 1.0);
    assert_eq!(ex.to_surface, 0.3);
    assert_eq!(ex.engine_overflow, 0.3);
}
