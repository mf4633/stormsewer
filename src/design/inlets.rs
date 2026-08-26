// SPDX-License-Identifier: GPL-3.0-or-later

//! FHWA HEC-22 inlet interception (US customary).
//!
//! On-grade inlets are analysed by the HEC-22 Chapter-4 gutter-spread method:
//! the triangular-gutter spread `T` is found from the approach flow with
//! Izzard's equation, then the interception efficiency `E` from the frontal /
//! side-flow split. Sag inlets use the weir↔orifice transition. Interception on
//! grade DEPENDS on the approach flow, so the capacity checks take it as input.
//!
//! Well-established forms are implemented directly; the grate-specific
//! splash-over velocity and the clogging fraction are exposed as inputs (they
//! are grate-model / agency specific) rather than hard-coded.

/// Gravitational constant (ft/s²) used by the sag orifice relations.
const G: f64 = crate::hydraulics::G_US;

/// Inlet configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InletKind {
    /// Grate inlet on grade.
    #[default]
    GrateOnGrade,
    /// Curb-opening inlet on grade.
    CurbOpening,
    /// Grate + curb opening at the same location (on grade).
    Combination,
    /// Sag (low-point) grate.
    SagGrate,
    /// Sag (low-point) curb opening.
    SagCurb,
}

impl InletKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::GrateOnGrade => "grate on grade",
            Self::CurbOpening => "curb opening",
            Self::Combination => "combination (grate + curb)",
            Self::SagGrate => "sag grate",
            Self::SagCurb => "sag curb opening",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "grate" | "grateongrade" | "grate_on_grade" | "g" => Some(Self::GrateOnGrade),
            "curb" | "curbopening" | "curb_opening" | "c" => Some(Self::CurbOpening),
            "combo" | "combination" | "comb" => Some(Self::Combination),
            "sag" | "saggrate" | "sag_grate" | "s" => Some(Self::SagGrate),
            "sagcurb" | "sag_curb" => Some(Self::SagCurb),
            _ => None,
        }
    }

    pub fn is_sag(self) -> bool {
        matches!(self, Self::SagGrate | Self::SagCurb)
    }
}

/// Gutter / inlet geometry for HEC-22 capacity checks (ft, ft/ft).
#[derive(Clone, Debug, PartialEq)]
pub struct InletGeometry {
    pub kind: InletKind,
    /// Grate length along the flow (ft).
    pub grate_length_ft: f64,
    /// Grate width across the flow, `W` (ft).
    pub grate_width_ft: f64,
    /// Curb-opening length, `L` (ft).
    pub curb_opening_length_ft: f64,
    /// Longitudinal gutter slope, `S_L` (ft/ft).
    pub gutter_slope: f64,
    /// Gutter cross (transverse) slope, `S_x` (ft/ft).
    pub cross_slope: f64,
    /// Gutter Manning roughness, `n`.
    pub gutter_n: f64,
    /// Grate splash-over velocity, `V_o` (ft/s): frontal flow is fully captured
    /// below it (grate-model specific — HEC-22 Chart 5).
    pub splash_over_velocity_fps: f64,
    /// Allowable gutter spread `T` (ft); on-grade inlets pass if spread ≤ this.
    pub allowable_spread_ft: f64,
    /// Clogging fraction (0–1) applied to sag capacity (agency specific).
    pub clogging_fraction: f64,
    /// Ponding depth at a sag (ft) — the head available at the low point.
    pub sag_ponding_depth_ft: f64,
    /// Curb-opening height for sag orifice flow (ft).
    pub curb_opening_height_ft: f64,
}

impl Default for InletGeometry {
    fn default() -> Self {
        Self {
            kind: InletKind::GrateOnGrade,
            grate_length_ft: 2.0,
            grate_width_ft: 1.5,
            curb_opening_length_ft: 4.0,
            gutter_slope: 0.01,
            cross_slope: 0.02,
            gutter_n: 0.016,
            splash_over_velocity_fps: 5.0,
            allowable_spread_ft: 10.0,
            clogging_fraction: 0.0,
            sag_ponding_depth_ft: 0.3,
            curb_opening_height_ft: 0.5,
        }
    }
}

/// Triangular-gutter spread `T` (ft) from Izzard's equation:
/// `Q = (0.56/n)·S_x^1.67·S_L^0.5·T^2.67` → `T = [Q·n/(0.56·S_x^1.67·S_L^0.5)]^(1/2.67)`.
/// The inverse exponent is exactly `1/2.67` so it round-trips the forward form.
pub fn gutter_spread_ft(q: f64, n: f64, cross_slope: f64, gutter_slope: f64) -> f64 {
    if q <= 0.0 || n <= 0.0 || cross_slope <= 0.0 || gutter_slope <= 0.0 {
        return 0.0;
    }
    (q * n / (0.56 * cross_slope.powf(1.67) * gutter_slope.sqrt())).powf(1.0 / 2.67)
}

/// Average gutter velocity (ft/s) for a triangular section: `V = Q / A`,
/// `A = ½·S_x·T²`.
pub fn gutter_velocity_fps(q: f64, cross_slope: f64, spread_ft: f64) -> f64 {
    let a = 0.5 * cross_slope * spread_ft * spread_ft;
    if a > 0.0 {
        q / a
    } else {
        0.0
    }
}

/// Grate-on-grade interception efficiency `E = R_f·E_o + R_s·(1−E_o)` (HEC-22).
pub fn grate_efficiency(q: f64, g: &InletGeometry) -> f64 {
    let t = gutter_spread_ft(q, g.gutter_n, g.cross_slope, g.gutter_slope);
    if t <= 0.0 {
        return 0.0;
    }
    let v = gutter_velocity_fps(q, g.cross_slope, t);
    // Frontal-flow ratio E_o = 1 − (1 − W/T)^2.67 (all flow is frontal if W ≥ T).
    let eo = if g.grate_width_ft >= t {
        1.0
    } else {
        1.0 - (1.0 - g.grate_width_ft / t).powf(2.67)
    };
    // Frontal efficiency R_f = 1 − 0.09·(V − V_o), capped to [0, 1].
    let rf = (1.0 - 0.09 * (v - g.splash_over_velocity_fps)).clamp(0.0, 1.0);
    // Side-flow efficiency R_s = 1 / (1 + 0.15·V^1.8 / (S_x·L^2.3)).
    let rs = if g.cross_slope > 0.0 && g.grate_length_ft > 0.0 {
        1.0 / (1.0 + 0.15 * v.powf(1.8) / (g.cross_slope * g.grate_length_ft.powf(2.3)))
    } else {
        0.0
    };
    (rf * eo + rs * (1.0 - eo)).clamp(0.0, 1.0)
}

/// Curb-opening length for total interception `L_T` (ft):
/// `L_T = 0.6·Q^0.42·S_L^0.3·(1/(n·S_x))^0.6`.
pub fn curb_length_full_interception_ft(q: f64, g: &InletGeometry) -> f64 {
    if q <= 0.0 || g.gutter_n <= 0.0 || g.cross_slope <= 0.0 || g.gutter_slope <= 0.0 {
        return 0.0;
    }
    0.6 * q.powf(0.42) * g.gutter_slope.powf(0.3) * (1.0 / (g.gutter_n * g.cross_slope)).powf(0.6)
}

/// Curb-opening-on-grade interception efficiency: `E = 1 − (1 − L/L_T)^1.8`,
/// or 1 when the opening is at least the full-interception length.
pub fn curb_efficiency(q: f64, g: &InletGeometry) -> f64 {
    let lt = curb_length_full_interception_ft(q, g);
    if lt <= 0.0 {
        return 0.0;
    }
    if g.curb_opening_length_ft >= lt {
        1.0
    } else {
        (1.0 - (1.0 - g.curb_opening_length_ft / lt).powf(1.8)).clamp(0.0, 1.0)
    }
}

/// On-grade interception efficiency for the configured inlet kind. Combination
/// (equal-length grate + curb) is taken as the grate efficiency per HEC-22 — the
/// curb opening does not add capacity on grade, so summing them (as the old code
/// did) double-counts the same gutter flow.
pub fn on_grade_efficiency(q: f64, g: &InletGeometry) -> f64 {
    match g.kind {
        InletKind::CurbOpening => curb_efficiency(q, g),
        _ => grate_efficiency(q, g),
    }
}

/// Sag grate capacity (cfs): weir `Q = C_w·P·d^1.5` (C_w≈3.0) transitioning to
/// orifice `Q = C_o·A·√(2g·d)` (C_o≈0.67), governed by the smaller at the
/// ponding depth. The grate sits against the curb, so per HEC-22 the weir
/// perimeter excludes the curb-side length: `P = L + 2W` (not the full
/// `2(L+W)`). A clogging fraction reduces open area and perimeter.
pub fn sag_grate_capacity_cfs(g: &InletGeometry) -> f64 {
    let d = g.sag_ponding_depth_ft;
    if d <= 0.0 || g.grate_length_ft <= 0.0 || g.grate_width_ft <= 0.0 {
        return 0.0;
    }
    let clog = g.clogging_fraction.clamp(0.0, 0.95);
    // Curb-adjacent grate: one length side is against the curb and excluded.
    let perimeter = (g.grate_length_ft + 2.0 * g.grate_width_ft) * (1.0 - clog);
    let area = g.grate_length_ft * g.grate_width_ft * (1.0 - clog);
    let weir = 3.0 * perimeter * d.powf(1.5);
    let orifice = 0.67 * area * (2.0 * G * d).sqrt();
    weir.min(orifice)
}

/// Sag curb-opening capacity (cfs): weir `Q = C_w·(L + 1.8·W)·d^1.5` (C_w≈2.3)
/// for `d ≤ h`, transitioning to orifice `Q = C_o·h·L·√(2g·(d − h/2))` (C_o≈0.67)
/// when submerged, governed by the smaller.
pub fn sag_curb_capacity_cfs(g: &InletGeometry) -> f64 {
    let d = g.sag_ponding_depth_ft;
    let l = g.curb_opening_length_ft;
    let h = g.curb_opening_height_ft.max(1e-3);
    if d <= 0.0 || l <= 0.0 {
        return 0.0;
    }
    let weir = 2.3 * (l + 1.8 * g.grate_width_ft) * d.powf(1.5);
    if d <= h {
        weir
    } else {
        let orifice = 0.67 * h * l * (2.0 * G * (d - h / 2.0)).sqrt();
        weir.min(orifice)
    }
}

/// Result of an inlet capacity / interception check.
#[derive(Clone, Debug, PartialEq)]
pub struct InletCheck {
    pub kind: InletKind,
    pub design_q_cfs: f64,
    /// Intercepted flow (cfs): `E·Q` on grade, or the sag capacity capped at `Q`.
    pub capacity_cfs: f64,
    /// On-grade interception efficiency (0–1); 1.0 for sag inlets.
    pub efficiency: f64,
    /// Gutter spread `T` (ft) at the design flow (on grade; 0 for sag).
    pub spread_ft: f64,
    /// Flow bypassing the inlet (cfs).
    pub bypass_cfs: f64,
    /// On grade: spread within allowable. Sag: capacity ≥ design flow.
    pub ok: bool,
}

/// HEC-22 inlet check for the approach `design_q_cfs`.
pub fn check_inlet_geom(design_q_cfs: f64, geom: &InletGeometry) -> InletCheck {
    if geom.kind.is_sag() {
        let cap = match geom.kind {
            InletKind::SagCurb => sag_curb_capacity_cfs(geom),
            _ => sag_grate_capacity_cfs(geom),
        };
        return InletCheck {
            kind: geom.kind,
            design_q_cfs,
            capacity_cfs: cap,
            efficiency: 1.0,
            spread_ft: 0.0,
            bypass_cfs: (design_q_cfs - cap).max(0.0),
            ok: cap >= design_q_cfs,
        };
    }
    let e = on_grade_efficiency(design_q_cfs, geom);
    let t = gutter_spread_ft(design_q_cfs, geom.gutter_n, geom.cross_slope, geom.gutter_slope);
    let intercepted = e * design_q_cfs;
    InletCheck {
        kind: geom.kind,
        design_q_cfs,
        capacity_cfs: intercepted,
        efficiency: e,
        spread_ft: t,
        bypass_cfs: (design_q_cfs - intercepted).max(0.0),
        // On grade the design criterion is the allowable gutter spread.
        ok: t <= geom.allowable_spread_ft,
    }
}

/// Merge per-node STM/Hydraflow overrides into the app-wide inlet defaults.
pub fn inlet_geometry_for_node(
    defaults: &InletGeometry,
    length_ft: f64,
    gutter_slope: f64,
    sag: bool,
) -> InletGeometry {
    let mut geom = defaults.clone();
    if length_ft > 0.0 {
        geom.curb_opening_length_ft = length_ft;
        geom.grate_length_ft = length_ft;
    }
    if gutter_slope > 0.0 {
        geom.gutter_slope = gutter_slope;
    }
    if sag {
        geom.kind = InletKind::SagGrate;
    }
    geom
}

/// Legacy grate-only check (backward compatible) using default gutter geometry.
pub fn check_inlet(
    design_q_cfs: f64,
    grate_length_ft: f64,
    _flow_depth_ft: f64,
    gutter_slope: f64,
) -> InletCheck {
    let geom = InletGeometry {
        kind: InletKind::GrateOnGrade,
        grate_length_ft,
        gutter_slope,
        ..InletGeometry::default()
    };
    check_inlet_geom(design_q_cfs, &geom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_inlet_project(bypass_a_to_b: bool) -> crate::io::Project {
        let mut p = crate::io::Project::empty();
        let mk = |id: &str, area: f64, c: f64| crate::io::ProjectNode {
            id: id.into(),
            kind: "inlet".into(),
            x: 0.0,
            y: 0.0,
            invert: 95.0,
            rim: 100.0,
            area_ac: area,
            c,
            tc_inlet: 10.0,
            inlet: Default::default(),
            bypass_to: None,
            diameter_ft: 4.0,
        };
        let mut a = mk("A", 3.0, 0.9); // big flow: guaranteed bypass
        if bypass_a_to_b {
            a.bypass_to = Some("B".into());
        }
        p.nodes.push(a);
        p.nodes.push(mk("B", 0.5, 0.5));
        p
    }

    #[test]
    fn bypass_carryover_reaches_target_inlet() {
        let p = two_inlet_project(true);
        let g = InletGeometry::default();
        let i_of = |_: &str| 5.0f64; // constant intensity
        let rows = network_inlet_pass(&p, &i_of, &g);
        let a = rows.iter().find(|r| r.node_id == "A").unwrap();
        let b = rows.iter().find(|r| r.node_id == "B").unwrap();
        assert!(a.bypass_cfs > 0.0, "fixture must bypass at A");
        assert!((a.local_cfs - 3.0 * 0.9 * 5.0).abs() < 1e-9);
        assert!((b.carryover_in_cfs - a.bypass_cfs).abs() < 1e-9);
        assert!((b.approach_cfs - (b.local_cfs + a.bypass_cfs)).abs() < 1e-9);
        // And B's interception matches a direct check at that approach.
        let direct = check_inlet_geom(b.approach_cfs, &g);
        assert!((b.intercepted_cfs - direct.capacity_cfs.min(b.approach_cfs)).abs() < 1e-9);
        assert!((b.spread_ft - direct.spread_ft).abs() < 1e-9);
    }

    #[test]
    fn off_system_bypass_carries_nothing() {
        let p = two_inlet_project(false);
        let g = InletGeometry::default();
        let rows = network_inlet_pass(&p, &|_| 5.0, &g);
        for r in &rows {
            assert_eq!(r.carryover_in_cfs, 0.0);
            assert!(r.bypass_to.is_none());
        }
    }

    #[test]
    fn bypass_cycle_is_broken_not_infinite() {
        let mut p = two_inlet_project(true);
        // (index 0 is the seeded OUT outfall — mutate by id)
        p.nodes.iter_mut().find(|n| n.id == "B").unwrap().bypass_to =
            Some("A".into()); // A -> B -> A
        let g = InletGeometry::default();
        let rows = network_inlet_pass(&p, &|_| 5.0, &g);
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter().any(|r| r.cycle_broken),
            "cycle must be flagged"
        );
    }

    #[test]
    fn bypass_to_unknown_target_leaves_system() {
        let mut p = two_inlet_project(true);
        p.nodes.iter_mut().find(|n| n.id == "A").unwrap().bypass_to =
            Some("NOPE".into());
        let g = InletGeometry::default();
        let rows = network_inlet_pass(&p, &|_| 5.0, &g);
        let b = rows.iter().find(|r| r.node_id == "B").unwrap();
        assert_eq!(b.carryover_in_cfs, 0.0);
    }

    #[test]
    fn gutter_spread_matches_izzard() {
        // Q=3 cfs, n=0.016, Sx=0.02, SL=0.01: 0.02^1.67=0.001452, 0.01^0.5=0.1,
        // denom=0.56·0.001452·0.1=8.13e-5; 3·0.016/8.13e-5=590; 590^0.375≈10.9 ft.
        let t = gutter_spread_ft(3.0, 0.016, 0.02, 0.01);
        assert!((t - 10.94).abs() < 0.3, "T={t}");
        // Spread grows with flow and shrinks with steeper longitudinal slope.
        assert!(gutter_spread_ft(6.0, 0.016, 0.02, 0.01) > t);
        assert!(gutter_spread_ft(3.0, 0.016, 0.02, 0.05) < t);
    }

    #[test]
    fn grate_efficiency_is_a_bounded_fraction() {
        // Unlike the old surrogate (an unbounded capacity that grew with slope),
        // the real method returns an interception EFFICIENCY in [0, 1].
        let g = InletGeometry::default();
        for q in [0.5, 2.0, 6.0, 20.0] {
            let e = grate_efficiency(q, &g);
            assert!((0.0..=1.0).contains(&e), "E={e} out of range at Q={q}");
        }
    }

    #[test]
    fn splash_over_reduces_frontal_capture() {
        // With everything else fixed, a lower splash-over velocity means the
        // grate loses more frontal flow to splash-over → lower efficiency. This
        // is the real slope/velocity effect the old surrogate had backwards.
        let base = InletGeometry::default();
        let high_vo = grate_efficiency(6.0, &InletGeometry { splash_over_velocity_fps: 12.0, ..base.clone() });
        let low_vo = grate_efficiency(6.0, &InletGeometry { splash_over_velocity_fps: 1.0, ..base.clone() });
        assert!(low_vo < high_vo, "low Vo {low_vo} should be < high Vo {high_vo}");
    }

    #[test]
    fn curb_efficiency_full_when_long_enough() {
        let g = InletGeometry {
            kind: InletKind::CurbOpening,
            curb_opening_length_ft: 100.0, // very long → full interception
            ..InletGeometry::default()
        };
        assert!((curb_efficiency(2.0, &g) - 1.0).abs() < 1e-9);
        let short = InletGeometry { curb_opening_length_ft: 2.0, ..g.clone() };
        assert!(curb_efficiency(2.0, &short) < 1.0);
    }

    #[test]
    fn combination_is_grate_not_sum() {
        // Fixes the double-count: on grade a combination inlet intercepts the
        // grate efficiency, not grate + curb of the same gutter flow.
        let g = InletGeometry { kind: InletKind::Combination, ..InletGeometry::default() };
        let e_comb = on_grade_efficiency(4.0, &g);
        let e_grate =
            grate_efficiency(4.0, &InletGeometry { kind: InletKind::GrateOnGrade, ..g.clone() });
        assert!((e_comb - e_grate).abs() < 1e-9);
        assert!(e_comb <= 1.0);
    }

    #[test]
    fn sag_grate_weir_then_orifice() {
        // Shallow ponding → weir governs; deep ponding → orifice governs and the
        // capacity is smaller than extrapolating the weir would give.
        let shallow = InletGeometry {
            kind: InletKind::SagGrate,
            sag_ponding_depth_ft: 0.2,
            ..InletGeometry::default()
        };
        let deep = InletGeometry { sag_ponding_depth_ft: 1.5, ..shallow.clone() };
        assert!(sag_grate_capacity_cfs(&deep) > sag_grate_capacity_cfs(&shallow));
        // Orifice governs at depth: capacity below the pure-weir extrapolation.
        let d = 1.5_f64;
        // Curb-adjacent weir perimeter is L + 2W (curb-side length excluded).
        let weir_only = 3.0 * (deep.grate_length_ft + 2.0 * deep.grate_width_ft) * d.powf(1.5);
        assert!(sag_grate_capacity_cfs(&deep) <= weir_only);

        // Shallow (weir-governed) capacity matches the L+2W perimeter exactly.
        let ds = 0.2_f64;
        let expect = 3.0 * (shallow.grate_length_ft + 2.0 * shallow.grate_width_ft) * ds.powf(1.5);
        assert!((sag_grate_capacity_cfs(&shallow) - expect).abs() < 1e-9);
    }

    #[test]
    fn on_grade_check_flags_excess_spread() {
        // A large flow on a flat, high-n gutter spreads beyond the allowable.
        let g = InletGeometry { allowable_spread_ft: 8.0, ..InletGeometry::default() };
        let small = check_inlet_geom(1.0, &g);
        let big = check_inlet_geom(12.0, &g);
        assert!(small.ok && small.spread_ft <= 8.0);
        assert!(!big.ok && big.spread_ft > 8.0, "spread {}", big.spread_ft);
        // Bypass is the un-intercepted fraction.
        assert!(big.bypass_cfs > 0.0 && big.efficiency < 1.0);
    }

    #[test]
    fn kind_from_str() {
        assert_eq!(InletKind::from_str_loose("combo"), Some(InletKind::Combination));
        assert_eq!(InletKind::from_str_loose("SAG"), Some(InletKind::SagGrate));
    }
}

/// Plain-text inlet schedule for the report (same column story as the GUI).
pub fn format_inlet_rows(rows: &[NetworkInletRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("
=== INLET SCHEDULE (HEC-22, with bypass carryover) ===

");
    out.push_str(&format!(
        "{:<8}{:>10}{:>10}{:>10}{:>10}  {:<8}{:>10}  Status
",
        "Inlet", "Local", "C/O in", "Interc", "Bypass", "To", "Spread"
    ));
    out.push_str(&format!(
        "{:<8}{:>10}{:>10}{:>10}{:>10}  {:<8}{:>10}
",
        "", "cfs", "cfs", "cfs", "cfs", "", "ft"
    ));
    out.push_str(&"-".repeat(78));
    out.push('\n');
    for r in rows {
        let status = if r.cycle_broken {
            "CYCLE BROKEN"
        } else if !r.ok {
            "EXCEEDS ALLOWABLE"
        } else if r.bypass_cfs > 0.005 {
            "bypassing"
        } else {
            "ok"
        };
        out.push_str(&format!(
            "{:<8}{:>10.2}{:>10.2}{:>10.2}{:>10.2}  {:<8}{:>10}  {}
",
            r.node_id,
            r.local_cfs,
            r.carryover_in_cfs,
            r.intercepted_cfs,
            r.bypass_cfs,
            r.bypass_to.as_deref().unwrap_or("(off)"),
            if r.spread_ft > 0.0 {
                format!("{:.1}", r.spread_ft)
            } else {
                "-".into()
            },
            status
        ));
    }
    out.push_str(
        "
Pipe design flows remain full Rational C*A (conservative); this
         schedule checks surface capture and spread.
",
    );
    out
}

/// One inlet's line in a network-wide HEC-22 pass, with carryover routing.
#[derive(Clone, Debug)]
pub struct NetworkInletRow {
    pub node_id: String,
    pub kind: InletKind,
    /// Local gutter runoff C·i·A at this inlet (cfs).
    pub local_cfs: f64,
    /// Bypass arriving from upstream inlets that target this one (cfs).
    pub carryover_in_cfs: f64,
    /// Total approach flow = local + carryover (cfs).
    pub approach_cfs: f64,
    pub intercepted_cfs: f64,
    pub bypass_cfs: f64,
    /// Where the bypass goes (`None` = leaves the system).
    pub bypass_to: Option<String>,
    pub spread_ft: f64,
    pub efficiency: f64,
    /// On grade: spread ≤ allowable. Sag: capacity ≥ approach.
    pub ok: bool,
    /// True if this inlet sits on a bypass cycle that had to be broken.
    pub cycle_broken: bool,
}

/// Network-wide inlet interception with bypass carryover: each inlet's
/// approach flow is its local gutter runoff plus the bypass of every
/// upstream inlet that routes here, evaluated in bypass-graph order.
///
/// This is a surface-flow adequacy analysis: pipe design flows remain the
/// full Rational C·A accumulation (conservative — pipes are sized as if
/// every drop is captured), while this pass answers "does each inlet, with
/// its carryover, still intercept within the allowable spread?"
///
/// Cycles in the bypass graph (A→B→A) are broken at the first node in
/// project order; that node is evaluated without the cyclic carryover and
/// flagged `cycle_broken`.
pub fn network_inlet_pass(
    project: &crate::io::Project,
    intensity_for_node: &dyn Fn(&str) -> f64,
    defaults: &InletGeometry,
) -> Vec<NetworkInletRow> {
    use std::collections::HashMap;

    let inlets: Vec<&crate::io::ProjectNode> = project
        .nodes
        .iter()
        .filter(|n| n.kind == "inlet")
        .collect();
    let idx: HashMap<&str, usize> = inlets
        .iter()
        .enumerate()
        .map(|(k, n)| (n.id.as_str(), k))
        .collect();

    // Bypass edges among inlets only; a target that isn't an inlet (or
    // doesn't exist) means the flow leaves the system.
    let target: Vec<Option<usize>> = inlets
        .iter()
        .map(|n| {
            n.bypass_to
                .as_deref()
                .and_then(|t| idx.get(t).copied())
                .filter(|&t| inlets[t].id != n.id)
        })
        .collect();

    let mut indegree = vec![0usize; inlets.len()];
    for t in target.iter().flatten() {
        indegree[*t] += 1;
    }

    // Kahn over the bypass graph; cycles broken in project order.
    let mut order = Vec::with_capacity(inlets.len());
    let mut ready: Vec<usize> = (0..inlets.len()).filter(|&k| indegree[k] == 0).collect();
    let mut done = vec![false; inlets.len()];
    let mut cycle_broken = vec![false; inlets.len()];
    loop {
        while let Some(k) = ready.pop() {
            if done[k] {
                continue;
            }
            done[k] = true;
            order.push(k);
            if let Some(t) = target[k] {
                indegree[t] = indegree[t].saturating_sub(1);
                if indegree[t] == 0 && !done[t] {
                    ready.push(t);
                }
            }
        }
        match (0..inlets.len()).find(|&k| !done[k]) {
            Some(k) => {
                cycle_broken[k] = true;
                ready.push(k);
            }
            None => break,
        }
    }

    let mut carryover = vec![0.0f64; inlets.len()];
    let mut rows: Vec<Option<NetworkInletRow>> = vec![None; inlets.len()];
    for &k in &order {
        let n = inlets[k];
        let local = (n.c * n.area_ac * intensity_for_node(&n.id)).max(0.0);
        let approach = local + carryover[k];
        let geom = inlet_geometry_for_node(
            defaults,
            n.inlet.length_ft,
            n.inlet.gutter_slope,
            n.inlet.sag,
        );
        let check = check_inlet_geom(approach, &geom);
        if let Some(t) = target[k] {
            carryover[t] += check.bypass_cfs;
        }
        rows[k] = Some(NetworkInletRow {
            node_id: n.id.clone(),
            kind: check.kind,
            local_cfs: local,
            carryover_in_cfs: carryover[k],
            approach_cfs: approach,
            intercepted_cfs: check.capacity_cfs.min(approach),
            bypass_cfs: check.bypass_cfs,
            bypass_to: target[k].map(|t| inlets[t].id.clone()),
            spread_ft: check.spread_ft,
            efficiency: check.efficiency,
            ok: check.ok,
            cycle_broken: cycle_broken[k],
        });
    }
    rows.into_iter().flatten().collect()
}
