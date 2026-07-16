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

use stormsewer::network::{AnalysisOptions, FlowRegime};
use stormsewer::{IdfCurve, Network, Node, Pipe};

/// Open-channel backwater physics: on a long mild reach held by an elevated
/// tailwater, the upstream HGL must fall BELOW a constant-depth translation of
/// the downstream stage — i.e. a real M1 profile that relaxes toward normal
/// depth — yet remain above normal depth. The old model translated the
/// downstream stage upstream at constant depth and over-predicted the stage.
#[test]
fn subcritical_backwater_relaxes_toward_normal_depth() {
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 96.6, 110.0, 2.0, 0.5), // inv_u = 96.6, C·A = 1.0
            Node::outfall("OUT", 96.0, 110.0),        // inv_d = 96.0 → S = 0.6/300 = 0.002 (mild)
        ],
        pipes: vec![Pipe::new("P1", "N1", "OUT", 300.0, 1.5, 0.013)],
    };
    let opts = AnalysisOptions {
        intensity_override: Some(3.0), // Q = i·C·A = 3.0 · 1.0 = 3.0 cfs (open channel)
        tailwater: Some(97.2),         // deep tailwater, ~1.2 ft over the invert
        junction_k: 0.0,               // isolate the backwater from structure loss
        ..Default::default()
    };
    let a = net.analyze(&IdfCurve::new(0.0, 1.0, 1.0), &opts).unwrap();
    let p1 = &a.pipes[0];

    assert!(!p1.surcharged, "reach should be open-channel");
    assert_eq!(p1.regime(), FlowRegime::Subcritical, "mild reach is subcritical");

    let hgl_up = p1.hgl_up.unwrap();
    let hgl_dn = p1.hgl_dn.unwrap();
    assert!((hgl_dn - 97.2).abs() < 1e-6, "hgl_dn = {hgl_dn}");

    // Old behaviour: hgl_up == hgl_dn + bed drop (0.6). The real backwater is
    // strictly below that as the profile draws down toward normal depth.
    let constant_depth_translate = hgl_dn + 0.6;
    assert!(
        hgl_up < constant_depth_translate - 0.02,
        "backwater should relax below the constant-depth translate: up={hgl_up}, translate={constant_depth_translate}"
    );
    // But it stays above normal depth (M1 curve), not below it.
    let yn = p1.normal_depth.unwrap();
    assert!(
        hgl_up > 96.6 + yn - 0.05,
        "M1 profile stays above normal depth: up={hgl_up}, normal surface={}",
        96.6 + yn
    );
}

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

/// Opt-in HEC-22 access-hole loss: with `hec22_structure_loss` enabled the
/// structure loss uses the initial coefficient Ko (relative size + angle) and
/// the outlet velocity, replacing the constant junction-K model.
#[test]
fn hec22_access_hole_loss_uses_ko_coefficient() {
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 100.0, 108.0, 5.0, 0.80), // C·A = 4.0
            Node::outfall("OUT", 96.0, 106.0),
        ],
        pipes: vec![Pipe::new("P1", "N1", "OUT", 300.0, 1.5, 0.013)],
    };
    let opts = AnalysisOptions {
        intensity_override: Some(5.0), // Q = 20 cfs (surcharges the 18-in pipe)
        tailwater: Some(100.0),
        hec22_structure_loss: true,
        access_hole_diam_ft: 4.0,
        ..Default::default()
    };
    let a = net.analyze(&IdfCurve::new(0.0, 1.0, 1.0), &opts).unwrap();
    let p1 = &a.pipes[0];

    // N1 is a straight-through headwater: Ko = 0.1·(b/Do) = 0.1·(4/1.5) = 0.2667.
    let v = p1.velocity;
    let ko = 0.1 * (4.0 / 1.5);
    let expected = 110.817 + ko * v * v / 64.4; // pressurized HGL_us + Ko·V²/2g
    assert!(
        (p1.hgl_up.unwrap() - expected).abs() < 0.03,
        "hec22 hgl_up = {}, expected ~{}",
        p1.hgl_up.unwrap(),
        expected
    );
    // Ko (0.267) here is below the default junction K (0.5), so the HEC-22 loss
    // is smaller than the constant-K result (111.81 ft from the hand test).
    assert!(p1.hgl_up.unwrap() < 111.81, "Ko loss should be below the K=0.5 loss");
}

/// Supercritical control: on a steep reach the flow is controlled from upstream,
/// so a downstream tailwater BELOW the pipe crown must NOT raise the upstream
/// HGL — the upstream stage equals the reach's own normal depth. But once the
/// tailwater rises ABOVE the crown the outlet is drowned and the reach is backed
/// up (the reported HGL must then reflect the submergence — ignoring it would be
/// unconservative and hide flooding).
#[test]
fn supercritical_reach_backed_up_only_when_outlet_drowns() {
    // 1.5-ft pipe, inv_d = 100 → crown at 101.5 ft.
    let analyze = |tw: f64| {
        let net = Network {
            nodes: vec![
                Node::inlet("N1", 105.0, 120.0, 1.0, 0.6), // inv_u = 105
                Node::outfall("OUT", 100.0, 120.0),        // inv_d = 100 → S = 5/100 = 0.05 (steep)
            ],
            pipes: vec![Pipe::new("P1", "N1", "OUT", 100.0, 1.5, 0.013)],
        };
        let opts = AnalysisOptions {
            intensity_override: Some(2.0), // Q = 2.0 · 0.6 = 1.2 cfs (open channel)
            tailwater: Some(tw),
            junction_k: 0.5,
            ..Default::default()
        };
        net.analyze(&IdfCurve::new(0.0, 1.0, 1.0), &opts).unwrap()
    };
    let free = analyze(100.0); // tailwater at the invert
    let low_tw = analyze(101.0); // still below the 101.5-ft crown
    let drowned = analyze(103.0); // 1.5 ft over the crown → outlet submerged

    let p_free = &free.pipes[0];
    let p_low = &low_tw.pipes[0];
    let p_drowned = &drowned.pipes[0];

    // The reach's own flow is supercritical in every case (regime ignores TW).
    assert_eq!(p_free.regime(), FlowRegime::Supercritical, "steep reach is supercritical");

    let up_free = p_free.hgl_up.unwrap();
    let up_low = p_low.hgl_up.unwrap();
    let up_drowned = p_drowned.hgl_up.unwrap();
    let yn = p_free.normal_depth.unwrap();

    // Below the crown: upstream HGL is the reach's normal depth, tailwater-independent.
    assert!((up_free - (105.0 + yn)).abs() < 1e-6, "upstream HGL = inv_u + normal depth");
    assert!(
        (up_free - up_low).abs() < 1e-6,
        "below-crown tailwater must not back up a supercritical reach: {up_free} vs {up_low}"
    );

    // Above the crown: the outlet is drowned and the upstream HGL rises to reflect it.
    assert!(
        up_drowned > up_free + 0.5,
        "drowned outlet must raise the upstream HGL: drowned {up_drowned} vs free {up_free}"
    );
    assert!(up_drowned.is_finite());
}

/// Multi-structure HGL: two surcharged 18-in reaches in series through a
/// junction manhole, hand-derived cumulatively from the outfall. This covers the
/// case a single-reach test cannot — the HGL climbing through *two* friction
/// segments and *two* structure losses.
///
/// N1 (inv 98) → MH (inv 97) → OUT (inv 96), both pipes D=1.5, n=0.013, L=200,
/// S=0.005, tailwater 100.0, junction K=0.5. Only N1 has area (C·A=3.6), so both
/// reaches carry Q = 5.0·3.6 = 18.0 cfs.
///
/// Full-flow conveyance K_f = (1.49/0.013)·(πD²/4)·(D/4)^(2/3) = 105.33, so
///   S_f  = (18/105.33)² = 0.029204,  h_f = S_f·200 = 5.841 ft
///   V    = 18/(πD²/4) = 10.186 ft/s,  h_j = 0.5·V²/2g = 0.806 ft
///
/// Cumulative from tailwater 100.0:
///   HGL(MH) = 100.0 + h_f + h_j                     = 106.65 ft
///   HGL(N1) = HGL(MH) + h_f + h_j                   = 113.29 ft
#[test]
fn multi_structure_hgl_matches_hand_backwater() {
    let net = Network {
        nodes: vec![
            Node::inlet("N1", 98.0, 115.0, 4.0, 0.90), // C·A = 3.6
            Node::junction("MH", 97.0, 108.0, 0.0, 0.0),
            Node::outfall("OUT", 96.0, 110.0),
        ],
        pipes: vec![
            Pipe::new("P1", "N1", "MH", 200.0, 1.5, 0.013),
            Pipe::new("P2", "MH", "OUT", 200.0, 1.5, 0.013),
        ],
    };
    let opts = AnalysisOptions {
        intensity_override: Some(5.0),
        tailwater: Some(100.0),
        junction_k: 0.5,
        ..Default::default()
    };
    let a = net.analyze(&IdfCurve::new(0.0, 1.0, 1.0), &opts).unwrap();

    let hgl = |id: &str| a.nodes.iter().find(|n| n.id == id).unwrap().hgl;
    for p in &a.pipes {
        assert!(p.surcharged, "{} should surcharge (18 cfs)", p.id);
        assert!((p.design_q - 18.0).abs() < 1e-6, "{} Q {}", p.id, p.design_q);
    }
    assert!((hgl("OUT") - 100.0).abs() < 1e-6, "OUT {}", hgl("OUT"));
    assert!((hgl("MH") - 106.65).abs() < 0.03, "MH HGL {}", hgl("MH"));
    assert!((hgl("N1") - 113.29).abs() < 0.03, "N1 HGL {}", hgl("N1"));
    // HGL rises monotonically upstream.
    assert!(hgl("N1") > hgl("MH") && hgl("MH") > hgl("OUT"));
}
