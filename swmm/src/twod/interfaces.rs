// SPDX-License-Identifier: GPL-3.0-or-later

//! The 1D-2D exchange formulas: weir and orifice flow at manholes and
//! inlets, lateral weir flow over a bank. Pure functions of the two water
//! levels, so they can be checked one at a time.
//!
//! The forms are the ones in Chen, Djordjević, Leandro & Savić (2007),
//! "The urban inundation model with bidirectional flow interaction between
//! 2D overland surface and 1D sewer networks", NOVATECH 2007, and Leandro,
//! Chen, Djordjević & Savić (2009), *J. Hydraul. Eng.* 135(6):495–504:
//!
//! - free weir      `Q = c · (2/3) · √(2g) · L · Δh^{3/2}`
//! - orifice        `Q = c · A · √(2g Δh)`
//! - submerged weir: the free-weir flow times Villemonte's (1947) factor
//!   `(1 − (h₂/h₁)^{3/2})^{0.385}`.
//!
//! `c` is the dimensionless discharge coefficient (0.6 unless the
//! interface sets one); with the `(2/3)√(2g)` written out the same `c`
//! serves both unit systems (in metric it makes the familiar 1.77 L h^1.5).

use std::f64::consts::PI;

use super::grid::default_lid_diameter;
use super::{InterfaceKind, NodeInterface};

/// Default discharge coefficient.
pub const DEFAULT_COEFF: f64 = 0.6;

pub fn coeff(iface: &NodeInterface) -> f64 {
    iface.weir_coeff.filter(|c| *c > 0.0).unwrap_or(DEFAULT_COEFF)
}

/// Perimeter and clear area of the opening an interface exchanges
/// through: the inlet's own, a default round lid for a manhole, nothing
/// for a sealed node.
pub fn opening(iface: &NodeInterface, metric: bool) -> (f64, f64) {
    match iface.kind {
        InterfaceKind::Manhole => {
            let d = default_lid_diameter(metric);
            (PI * d, PI * d * d / 4.0)
        }
        InterfaceKind::Inlet { perimeter, area } => (perimeter.max(0.0), area.max(0.0)),
        InterfaceKind::Sealed => (0.0, 0.0),
    }
}

/// Free weir flow over length `length` under head `head`.
pub fn weir_flow(g: f64, c: f64, length: f64, head: f64) -> f64 {
    if head <= 0.0 || length <= 0.0 {
        return 0.0;
    }
    c * (2.0 / 3.0) * (2.0 * g).sqrt() * length * head.powf(1.5)
}

/// Orifice flow through `area` under head `head`.
pub fn orifice_flow(g: f64, c: f64, area: f64, head: f64) -> f64 {
    if head <= 0.0 || area <= 0.0 {
        return 0.0;
    }
    c * area * (2.0 * g * head).sqrt()
}

/// Villemonte's submergence factor for a weir with upstream head `h1`
/// and downstream head `h2` (both above the crest).
pub fn villemonte(h1: f64, h2: f64) -> f64 {
    if h2 <= 0.0 || h1 <= 0.0 {
        return 1.0;
    }
    let r = (h2 / h1).min(1.0);
    (1.0 - r.powf(1.5)).max(0.0).powf(0.385)
}

/// Flow captured from the surface into a node (≥ 0): weir flow over the
/// opening's perimeter under the surface depth, capped by orifice flow
/// through its area under the head difference to the node, and zero when
/// the node's head is at or above the surface.
pub fn inlet_capture(
    g: f64,
    c: f64,
    perimeter: f64,
    area: f64,
    depth_2d: f64,
    head_1d: f64,
    ground: f64,
) -> f64 {
    if depth_2d <= 0.0 {
        return 0.0;
    }
    let eta = ground + depth_2d;
    if head_1d >= eta {
        return 0.0;
    }
    // Head driving the orifice: the full surface depth when the node is
    // below ground, else the difference between the two surfaces.
    let dh = if head_1d <= ground { depth_2d } else { eta - head_1d };
    let weir = weir_flow(g, c, perimeter, depth_2d);
    let orifice = orifice_flow(g, c, area, dh);
    if area > 0.0 {
        weir.min(orifice)
    } else {
        weir
    }
}

/// Flow surcharging out of a node onto the surface (≥ 0) when its head is
/// above the water surface at its cell: free weir over the lid perimeter
/// while the surface is dry, orifice through the lid area once ponded
/// water submerges it.
#[allow(clippy::too_many_arguments)]
pub fn manhole_surcharge(
    g: f64,
    c: f64,
    perimeter: f64,
    area: f64,
    head_1d: f64,
    ground: f64,
    depth_2d: f64,
    dry_depth: f64,
) -> f64 {
    let eta = ground + depth_2d.max(0.0);
    if head_1d <= eta {
        return 0.0;
    }
    if depth_2d <= dry_depth {
        weir_flow(g, c, perimeter, head_1d - ground)
    } else {
        orifice_flow(g, c, area, head_1d - eta)
    }
}

/// Lateral weir flow over a bank crest between a channel surface `eta_1d`
/// and a cell surface `eta_2d`, positive from the channel to the surface.
/// Free weir when the lower side is below the crest, Villemonte-reduced
/// when both sides are above it.
pub fn bank_exchange(g: f64, c: f64, length: f64, crest: f64, eta_1d: f64, eta_2d: f64) -> f64 {
    let hi = eta_1d.max(eta_2d) - crest;
    if hi <= 0.0 {
        return 0.0;
    }
    let lo = eta_1d.min(eta_2d) - crest;
    let q = weir_flow(g, c, length, hi) * villemonte(hi, lo);
    if eta_1d >= eta_2d {
        q
    } else {
        -q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_weir_reduces_to_the_textbook_constant() {
        // c (2/3) √(2g) with c = 0.6, g = 9.81 is 1.771.
        let q = weir_flow(9.81, 0.6, 1.0, 1.0);
        assert!((q - 1.7713).abs() < 1e-3, "{q}");
    }

    #[test]
    fn capture_is_limited_by_the_orifice_and_stops_when_the_node_is_full() {
        let g = 9.81;
        let deep = inlet_capture(g, 0.6, 2.0, 0.1, 0.5, -1.0, 0.0);
        assert!((deep - orifice_flow(g, 0.6, 0.1, 0.5)).abs() < 1e-12);
        let shallow = inlet_capture(g, 0.6, 2.0, 10.0, 0.01, -1.0, 0.0);
        assert!((shallow - weir_flow(g, 0.6, 2.0, 0.01)).abs() < 1e-12);
        assert_eq!(inlet_capture(g, 0.6, 2.0, 0.1, 0.5, 0.5, 0.0), 0.0);
        assert_eq!(inlet_capture(g, 0.6, 2.0, 0.1, 0.0, -1.0, 0.0), 0.0);
    }

    #[test]
    fn surcharge_switches_from_weir_to_orifice_when_ponded() {
        let g = 32.174;
        let free = manhole_surcharge(g, 0.6, 6.25, 3.125, 101.0, 100.0, 0.0, 0.003);
        assert!((free - weir_flow(g, 0.6, 6.25, 1.0)).abs() < 1e-12);
        let ponded = manhole_surcharge(g, 0.6, 6.25, 3.125, 101.0, 100.0, 0.5, 0.003);
        assert!((ponded - orifice_flow(g, 0.6, 3.125, 0.5)).abs() < 1e-12);
        assert_eq!(manhole_surcharge(g, 0.6, 6.25, 3.125, 100.2, 100.0, 0.5, 0.003), 0.0);
    }

    #[test]
    fn bank_flow_is_antisymmetric_and_submergence_reduces_it() {
        let a = bank_exchange(9.81, 0.6, 5.0, 10.0, 11.0, 9.0);
        let b = bank_exchange(9.81, 0.6, 5.0, 10.0, 9.0, 11.0);
        assert!((a + b).abs() < 1e-12);
        let sub = bank_exchange(9.81, 0.6, 5.0, 10.0, 11.0, 10.5);
        assert!(sub > 0.0 && sub < a);
        assert_eq!(bank_exchange(9.81, 0.6, 5.0, 10.0, 9.5, 9.0), 0.0);
    }
}
