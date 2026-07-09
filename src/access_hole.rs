// SPDX-License-Identifier: GPL-3.0-or-later

//! FHWA HEC-22 access-hole (manhole / junction) energy-loss coefficient.
//!
//! HEC-22 computes the head loss across an access hole as
//! `H_ah = K_ah · V_o² / 2g`, where `V_o` is the OUTLET-pipe velocity and
//! `K_ah = K_o · C_D · C_d · C_Q · C_p · C_B` is an initial coefficient adjusted
//! by several correction factors.
//!
//! **Scope of this implementation (be honest about it):** the initial
//! coefficient `K_o` (relative-size / deflection-angle), the flow-depth factor
//! `C_d`, the relative-diameter factor `C_D`, and the plunging factor `C_p` are
//! computed from their analytic HEC-22 forms. The relative-flow factor `C_Q` is
//! 1.0 (single inflow per reach in this pass), and benching `C_B` is a supplied
//! input (its HEC-22 Table-7.6 values are grate/agency specific). The method is
//! not yet pinned to a published FHWA worked example. It is opt-in; the default
//! analysis keeps the simple `junction_k` model.
//!
//! Submergence switch: with `d_aho/D_o ≥ 3.2` (pressure flow) `C_d = 1` and the
//! relative-diameter `C_D = (D_o/D_i)^3` applies; below it `C_D = 1` and
//! `C_d = 0.5·(d_aho/D_o)^0.6`.
//!
//! `K_o = 0.1·(b/D_o)·(1 − sinθ) + 1.4·(b/D_o)^0.15·sinθ`, where `b` is the
//! access-hole diameter, `D_o` the outlet-pipe diameter, and `θ` the angle
//! between the inflow and outflow pipes (θ = 180° for straight-through, so
//! `sinθ = 0` and `K_o = 0.1·b/D_o`; a 90° bend gives `sinθ = 1`).

/// Inputs for one access-hole energy-loss computation.
#[derive(Clone, Copy, Debug)]
pub struct AccessHole {
    /// Outlet-pipe velocity `V_o` (ft/s).
    pub v_out: f64,
    /// Outlet-pipe diameter `D_o` (ft).
    pub d_out: f64,
    /// Access-hole (structure) diameter `b` (ft).
    pub access_diam: f64,
    /// Cosine of the flow deflection between inflow and outflow (1 = straight).
    pub deflection_cos: f64,
    /// Water depth in the access hole above the outlet invert, `d_aho` (ft).
    pub water_depth: f64,
    /// Height of a plunging inflow above the outlet invert, `h` (ft); 0 if the
    /// inflow does not plunge.
    pub plunge_height: f64,
    /// Inflow-pipe diameter `D_i` (ft), for the relative-diameter factor `C_D`
    /// under pressure flow; 0 (or ≤ 0) → `C_D = 1`.
    pub d_in: f64,
    /// Benching factor `C_B` (HEC-22 Table 7.6; 1.0 = flat / no credit).
    pub bench_factor: f64,
}

/// Submergence ratio `d_aho / D_o` above which access-hole flow is pressurized.
const SUBMERGED_RATIO: f64 = 3.2;

/// HEC-22 flow-depth correction `C_d = 0.5·(d_aho/D_o)^0.6` for unsubmerged flow
/// (`d_aho/D_o < 3.2`), else 1.0.
pub fn c_depth(water_depth: f64, d_out: f64) -> f64 {
    if d_out <= 0.0 {
        return 1.0;
    }
    let ratio = water_depth / d_out;
    if ratio >= SUBMERGED_RATIO {
        1.0
    } else {
        (0.5 * ratio.max(0.0).powf(0.6)).clamp(0.0, 1.0)
    }
}

/// HEC-22 relative-diameter correction `C_D = (D_o/D_i)^3`, applied only under
/// pressure flow (`d_aho/D_o ≥ 3.2`); otherwise 1.0.
pub fn c_diameter(water_depth: f64, d_out: f64, d_in: f64) -> f64 {
    if d_out <= 0.0 || d_in <= 0.0 || water_depth / d_out < SUBMERGED_RATIO {
        return 1.0;
    }
    (d_out / d_in).powi(3)
}

/// HEC-22 initial access-hole loss coefficient `K_o` (relative size + angle).
pub fn k_o(access_diam: f64, d_out: f64, deflection_cos: f64) -> f64 {
    if d_out <= 0.0 {
        return 0.0;
    }
    let r = access_diam / d_out;
    // θ is the inflow→outflow angle (180° straight); sinθ = sin(deflection δ),
    // and cos δ is the supplied deflection cosine, so sinθ = √(1 − cos²δ).
    let cosd = deflection_cos.clamp(-1.0, 1.0);
    let sin_theta = (1.0 - cosd * cosd).sqrt();
    0.1 * r * (1.0 - sin_theta) + 1.4 * r.powf(0.15) * sin_theta
}

/// HEC-22 plunging-flow correction `C_p`. Applies only when the inflow plunges
/// (its height above the outlet invert exceeds the access-hole water depth).
pub fn c_plunge(plunge_height: f64, water_depth: f64, d_out: f64) -> f64 {
    if d_out <= 0.0 || plunge_height <= water_depth {
        return 1.0;
    }
    1.0 + 0.2 * (plunge_height / d_out) * ((plunge_height - water_depth) / d_out)
}

/// Composite access-hole coefficient `K_ah = K_o·C_D·C_d·C_p·C_B` (`C_Q = 1`).
pub fn k_ah(a: &AccessHole) -> f64 {
    let bench = if a.bench_factor > 0.0 { a.bench_factor } else { 1.0 };
    (k_o(a.access_diam, a.d_out, a.deflection_cos)
        * c_diameter(a.water_depth, a.d_out, a.d_in)
        * c_depth(a.water_depth, a.d_out)
        * c_plunge(a.plunge_height, a.water_depth, a.d_out)
        * bench)
        .max(0.0)
}

/// Access-hole head loss (ft): `H_ah = K_ah · V_o² / 2g`.
pub fn head_loss(a: &AccessHole, g: f64) -> f64 {
    k_ah(a) * a.v_out * a.v_out / (2.0 * g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ko_straight_through_is_relative_size_only() {
        // θ = 180° (sinθ = 0) → Ko = 0.1·(b/Do). b = 4 ft, Do = 1 ft → 0.4.
        let ko = k_o(4.0, 1.0, 1.0); // deflection cos = 1 (straight)
        assert!((ko - 0.4).abs() < 1e-9, "ko={ko}");
    }

    #[test]
    fn ko_ninety_degree_bend_uses_angle_term() {
        // δ = 90° → cos = 0, sinθ = 1 → Ko = 1.4·(b/Do)^0.15. b=4, Do=1 → 1.4·4^0.15.
        let ko = k_o(4.0, 1.0, 0.0);
        let expect = 1.4 * 4.0f64.powf(0.15);
        assert!((ko - expect).abs() < 1e-9, "ko={ko} expect={expect}");
        // A bend loses more head than straight-through.
        assert!(ko > k_o(4.0, 1.0, 1.0));
    }

    #[test]
    fn plunging_increases_loss() {
        assert!((c_plunge(0.0, 1.0, 1.0) - 1.0).abs() < 1e-12, "no plunge → 1");
        assert!(c_plunge(3.0, 1.0, 1.0) > 1.0, "plunging inflow adds loss");
    }

    fn sample(water_depth: f64) -> AccessHole {
        AccessHole {
            v_out: 8.0,
            d_out: 1.5,
            access_diam: 4.0,
            deflection_cos: 1.0,
            water_depth,
            plunge_height: 0.0,
            d_in: 1.5,
            bench_factor: 1.0,
        }
    }

    #[test]
    fn depth_correction_reduces_shallow_loss() {
        // Unsubmerged (d_aho/Do < 3.2): Cd = 0.5·(ratio)^0.6 < 1 → less loss.
        assert!(c_depth(1.5, 1.5) < 1.0); // ratio 1.0 → 0.5
        assert!((c_depth(1.5, 1.5) - 0.5).abs() < 1e-9);
        // Submerged: Cd = 1.
        assert!((c_depth(6.0, 1.5) - 1.0).abs() < 1e-9); // ratio 4.0 ≥ 3.2
    }

    #[test]
    fn diameter_correction_only_when_pressurized() {
        // Unsubmerged → CD = 1 regardless of Di.
        assert!((c_diameter(1.5, 1.5, 1.0) - 1.0).abs() < 1e-9);
        // Submerged with a smaller inflow pipe → CD = (Do/Di)^3 > 1.
        let cd = c_diameter(6.0, 1.5, 1.0);
        assert!((cd - (1.5f64 / 1.0).powi(3)).abs() < 1e-9);
        assert!(cd > 1.0);
    }

    #[test]
    fn head_loss_scales_with_velocity_head() {
        let a = sample(1.5); // ratio 1.0 (unsubmerged) → Cd = 0.5, CD = 1
        let h = head_loss(&a, 32.2);
        let expect = k_o(4.0, 1.5, 1.0) * 0.5 * 8.0 * 8.0 / (2.0 * 32.2);
        assert!((h - expect).abs() < 1e-9, "h={h} expect={expect}");
    }

    #[test]
    fn benching_factor_scales_loss() {
        let mut a = sample(6.0); // submerged
        let full = head_loss(&a, 32.2);
        a.bench_factor = 0.6;
        assert!((head_loss(&a, 32.2) - 0.6 * full).abs() < 1e-9);
    }
}
