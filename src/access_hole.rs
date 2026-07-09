// SPDX-License-Identifier: GPL-3.0-or-later

//! FHWA HEC-22 access-hole (manhole / junction) energy-loss coefficient.
//!
//! HEC-22 computes the head loss across an access hole as
//! `H_ah = K_ah · V_o² / 2g`, where `V_o` is the OUTLET-pipe velocity and
//! `K_ah = K_o · C_D · C_d · C_Q · C_p · C_B` is an initial coefficient adjusted
//! by several correction factors.
//!
//! **Scope of this implementation (be honest about it):** only the initial
//! coefficient `K_o` — the well-established relative-size / deflection-angle term
//! — and the plunging-flow factor `C_p` are computed here. The relative-diameter
//! (`C_D`), flow-depth (`C_d`), relative-flow (`C_Q`), and benching (`C_B`)
//! corrections are NOT yet implemented (treated as 1.0), and the method is not
//! yet pinned to a published FHWA worked example. It is exposed as an opt-in
//! analysis mode; the default analysis keeps the simple `junction_k` model.
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

/// Access-hole head loss (ft): `H_ah = K_ah · V_o² / 2g` with
/// `K_ah = K_o · C_p` (the corrections not yet implemented are 1.0).
pub fn head_loss(a: &AccessHole, g: f64) -> f64 {
    let kah = k_o(a.access_diam, a.d_out, a.deflection_cos)
        * c_plunge(a.plunge_height, a.water_depth, a.d_out);
    kah.max(0.0) * a.v_out * a.v_out / (2.0 * g)
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

    #[test]
    fn head_loss_scales_with_velocity_head() {
        let a = AccessHole {
            v_out: 8.0,
            d_out: 1.5,
            access_diam: 4.0,
            deflection_cos: 1.0,
            water_depth: 1.0,
            plunge_height: 0.0,
        };
        let h = head_loss(&a, 32.2);
        let expect = k_o(4.0, 1.5, 1.0) * 8.0 * 8.0 / (2.0 * 32.2);
        assert!((h - expect).abs() < 1e-9);
    }
}
