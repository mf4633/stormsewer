// SPDX-License-Identifier: GPL-3.0-or-later

//! Open-channel & partial-flow hydraulics for circular closed conduits.
//!
//! US customary units throughout (feet, seconds, cfs). Pass `K_MANNING_SI`
//! and `G_SI` for metric. All geometry is exact (no table lookups).

use std::f64::consts::PI;

/// Manning conversion factor, US customary (1.486 rounded to 1.49 by convention).
pub const K_MANNING_US: f64 = 1.49;
/// Manning conversion factor, SI.
pub const K_MANNING_SI: f64 = 1.0;
/// Gravitational acceleration, US customary (ft/s^2).
pub const G_US: f64 = 32.2;
/// Gravitational acceleration, SI (m/s^2).
pub const G_SI: f64 = 9.81;

/// Central angle (radians) subtended at the pipe centre by the water surface,
/// for flow depth `y` in a circular pipe of diameter `d`.
///
/// `0` when empty, `2*PI` when full.
pub fn circular_angle(y: f64, d: f64) -> f64 {
    let y = y.clamp(0.0, d);
    2.0 * (1.0 - 2.0 * y / d).acos()
}

/// Flow geometry at depth `y` in a circular pipe of diameter `d`.
///
/// Returns `(area, wetted_perimeter, hydraulic_radius, top_width)`.
pub fn circular_geometry(y: f64, d: f64) -> (f64, f64, f64, f64) {
    let theta = circular_angle(y, d);
    let area = d * d / 8.0 * (theta - theta.sin());
    let perim = d * theta / 2.0;
    let radius = if perim > 0.0 { area / perim } else { 0.0 };
    let top = d * (theta / 2.0).sin();
    (area, perim, radius, top)
}

/// Full cross-sectional area of a circular pipe.
pub fn full_area(d: f64) -> f64 {
    PI * d * d / 4.0
}

/// Manning discharge from flow area and hydraulic radius.
///
/// `Q = (k/n) * A * R^(2/3) * S^(1/2)`
pub fn manning_q(n: f64, s: f64, area: f64, radius: f64, k: f64) -> f64 {
    if n <= 0.0 {
        return 0.0;
    }
    k / n * area * radius.powf(2.0 / 3.0) * s.max(0.0).sqrt()
}

/// Just-full (geometrically full, free-surface) capacity of a circular pipe.
///
/// Note: the *maximum* open-channel discharge actually occurs near `0.94 d`
/// and slightly exceeds this value — see [`max_capacity`].
pub fn full_flow_capacity(n: f64, s: f64, d: f64, k: f64) -> f64 {
    manning_q(n, s, full_area(d), d / 4.0, k)
}

/// Discharge at partial depth `y` in a circular pipe.
pub fn circular_q(n: f64, s: f64, d: f64, y: f64, k: f64) -> f64 {
    let (area, _p, radius, _t) = circular_geometry(y, d);
    manning_q(n, s, area, radius, k)
}

/// Maximum open-channel discharge of a circular pipe and the depth at which it
/// occurs (~`0.938 d`). Returns `(q_max, y_at_max)`.
pub fn max_capacity(n: f64, s: f64, d: f64, k: f64) -> (f64, f64) {
    // Q(y) is smooth with a single interior maximum; a fine scan is exact
    // enough and cheap for the depths involved.
    let steps = 2000;
    let mut best_q = 0.0;
    let mut best_y = 0.0;
    for i in 1..=steps {
        let y = d * i as f64 / steps as f64;
        let q = circular_q(n, s, d, y, k);
        if q > best_q {
            best_q = q;
            best_y = y;
        }
    }
    (best_q, best_y)
}

/// Normal (uniform-flow) depth for a target discharge in a circular pipe.
///
/// Returns `None` when `q_target` exceeds the pipe's maximum open-channel
/// capacity — i.e. the pipe must surcharge / flow under pressure.
pub fn normal_depth(q_target: f64, n: f64, s: f64, d: f64, k: f64) -> Option<f64> {
    if q_target <= 0.0 {
        return Some(0.0);
    }
    let (q_max, y_max) = max_capacity(n, s, d, k);
    if q_target > q_max {
        return None;
    }
    // Q is monotonic increasing on (0, y_max]; bisect there.
    let (mut lo, mut hi) = (0.0_f64, y_max);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if circular_q(n, s, d, mid, k) > q_target {
            hi = mid;
        } else {
            lo = mid;
        }
        if hi - lo < 1e-7 {
            break;
        }
    }
    Some(0.5 * (lo + hi))
}

/// Critical depth for a target discharge in a circular pipe.
///
/// Solves `Q^2 * T = g * A^3` (Froude = 1). Valid for `0 < y < d`.
pub fn critical_depth(q: f64, d: f64, g: f64) -> f64 {
    if q <= 0.0 {
        return 0.0;
    }
    let resid = |y: f64| {
        let (area, _p, _r, top) = circular_geometry(y, d);
        if area <= 0.0 || top <= 0.0 {
            return -q * q;
        }
        q * q * top - g * area * area * area
    };
    // resid > 0 for small y, < 0 as y grows: bracket the sign change.
    let (mut lo, mut hi) = (1e-6 * d, 0.999 * d);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if resid(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-7 {
            break;
        }
    }
    0.5 * (lo + hi)
}

// ─────────────────────────────────────────────────────────────────────────────
// Non-circular sections
//
// A closed-conduit cross-section solved on its own geometry — not approximated
// by an equivalent circle. Depth `y` is measured from the invert (0) to the
// crown ([`Section::height`]); for `y < height` the water surface is open (the
// crown is not wetted), matching the free-surface convention used for circular
// pipes above.
// ─────────────────────────────────────────────────────────────────────────────

/// A closed-conduit cross-section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Section {
    /// Circular pipe of diameter `d`.
    Circular { d: f64 },
    /// Rectangular (box) conduit: `rise` (height) by `span` (width).
    Rectangular { rise: f64, span: f64 },
    /// Horizontal-elliptical pipe: vertical `rise` by horizontal `span`.
    Elliptical { rise: f64, span: f64 },
    /// Arch conduit: vertical walls up to the springline with a semicircular top
    /// of radius `span/2` (arch-culvert shape). Requires `rise >= span/2`.
    Arch { rise: f64, span: f64 },
}

impl Section {
    /// Circular section constructor.
    pub fn circular(d: f64) -> Self {
        Section::Circular { d }
    }

    /// Crown height — the maximum flow depth.
    pub fn height(&self) -> f64 {
        match *self {
            Section::Circular { d } => d,
            Section::Rectangular { rise, .. }
            | Section::Elliptical { rise, .. }
            | Section::Arch { rise, .. } => rise,
        }
    }

    /// Flow geometry at depth `y`: `(area, wetted_perimeter, hydraulic_radius,
    /// top_width)` with an open water surface for `y < height`.
    pub fn geometry(&self, y: f64) -> (f64, f64, f64, f64) {
        match *self {
            Section::Circular { d } => circular_geometry(y, d),
            Section::Rectangular { rise, span } => {
                let y = y.clamp(0.0, rise);
                let area = span * y;
                let perim = span + 2.0 * y; // bottom + two sides (open top)
                let r = if perim > 0.0 { area / perim } else { 0.0 };
                (area, perim, r, span)
            }
            Section::Elliptical { rise, span } => {
                let y = y.clamp(0.0, rise);
                // An ellipse is a circle stretched horizontally by span/rise, so
                // area and top width scale exactly from the vertical circle of
                // diameter `rise`. The wetted perimeter does not scale and is
                // integrated numerically.
                let (a_c, _p, _r, t_c) = circular_geometry(y, rise);
                let scale = span / rise;
                let area = scale * a_c;
                let top = scale * t_c;
                let perim = elliptical_wetted_perimeter(y, rise, span);
                let r = if perim > 0.0 { area / perim } else { 0.0 };
                (area, perim, r, top)
            }
            Section::Arch { rise, span } => {
                let y = y.clamp(0.0, rise);
                let rad = span / 2.0;
                let wall = (rise - rad).max(0.0); // vertical wall height (to springline)
                let (area, perim, top) = if y <= wall {
                    // Rectangular (below the springline).
                    (span * y, span + 2.0 * y, span)
                } else {
                    // Full rectangle + a circular zone of the semicircular top.
                    let hs = (y - wall).min(rad); // height into the semicircle
                    let root = (rad * rad - hs * hs).max(0.0).sqrt();
                    let phi = (hs / rad).clamp(-1.0, 1.0).asin();
                    let seg_area = hs * root + rad * rad * phi;
                    let area = span * wall + seg_area;
                    let perim = span + 2.0 * wall + 2.0 * rad * phi;
                    (area, perim, 2.0 * root)
                };
                let r = if perim > 0.0 { area / perim } else { 0.0 };
                (area, perim, r, top)
            }
        }
    }

    /// Full cross-sectional area.
    pub fn full_area(&self) -> f64 {
        match *self {
            Section::Circular { d } => full_area(d),
            Section::Rectangular { rise, span } => rise * span,
            Section::Elliptical { rise, span } => PI * span * rise / 4.0,
            Section::Arch { rise, span } => {
                let rad = span / 2.0;
                let wall = (rise - rad).max(0.0);
                span * wall + PI * rad * rad / 2.0 // walls + semicircle
            }
        }
    }

    /// Fully-wetted perimeter (entire boundary), for just-full capacity.
    pub fn full_perimeter(&self) -> f64 {
        match *self {
            Section::Circular { d } => PI * d,
            Section::Rectangular { rise, span } => 2.0 * (rise + span),
            Section::Elliptical { rise, span } => elliptical_wetted_perimeter(rise, rise, span),
            Section::Arch { rise, span } => {
                let rad = span / 2.0;
                let wall = (rise - rad).max(0.0);
                span + 2.0 * wall + PI * rad // bottom + walls + semicircle arc
            }
        }
    }

    /// Hydraulic radius at just-full (entire boundary wetted).
    pub fn full_hydraulic_radius(&self) -> f64 {
        let p = self.full_perimeter();
        if p > 0.0 {
            self.full_area() / p
        } else {
            0.0
        }
    }
}

/// Wetted perimeter of a horizontal ellipse (`rise` × `span`) filled to depth
/// `y`, by Simpson integration of arc length — the ellipse has no closed-form
/// perimeter. Reduces exactly to the circular value when `span == rise`.
fn elliptical_wetted_perimeter(y: f64, rise: f64, span: f64) -> f64 {
    let a = span / 2.0; // horizontal semi-axis
    let b = rise / 2.0; // vertical semi-axis
    let y = y.clamp(0.0, rise);
    if b <= 0.0 {
        return 0.0;
    }
    // Parametrize z = -b cos t (bottom at t=0), so depth = b(1 - cos t).
    let cos_tw = (1.0 - y / b).clamp(-1.0, 1.0);
    let theta_w = cos_tw.acos();
    let n = 256usize; // even, for Simpson
    let h = theta_w / n as f64;
    if h <= 0.0 {
        return 0.0;
    }
    let f = |t: f64| (a * a * t.cos() * t.cos() + b * b * t.sin() * t.sin()).sqrt();
    let mut sum = f(0.0) + f(theta_w);
    for i in 1..n {
        let t = i as f64 * h;
        sum += if i % 2 == 1 { 4.0 } else { 2.0 } * f(t);
    }
    // Two symmetric sides.
    2.0 * (h / 3.0) * sum
}

/// Discharge at partial depth `y` in any [`Section`].
pub fn section_q(sec: &Section, n: f64, s: f64, y: f64, k: f64) -> f64 {
    let (area, _p, radius, _t) = sec.geometry(y);
    manning_q(n, s, area, radius, k)
}

/// Just-full (entire-boundary-wetted) capacity of any [`Section`].
pub fn section_full_capacity(sec: &Section, n: f64, s: f64, k: f64) -> f64 {
    manning_q(n, s, sec.full_area(), sec.full_hydraulic_radius(), k)
}

/// Maximum open-channel discharge and the depth at which it occurs, for any
/// [`Section`]. Returns `(q_max, y_at_max)`.
pub fn section_max_capacity(sec: &Section, n: f64, s: f64, k: f64) -> (f64, f64) {
    let h = sec.height();
    let steps = 2000;
    let (mut best_q, mut best_y) = (0.0, 0.0);
    for i in 1..=steps {
        let y = h * i as f64 / steps as f64;
        let q = section_q(sec, n, s, y, k);
        if q > best_q {
            best_q = q;
            best_y = y;
        }
    }
    (best_q, best_y)
}

/// Normal (uniform-flow) depth for a target discharge in any [`Section`].
/// `None` when the flow exceeds the section's maximum open-channel capacity.
pub fn section_normal_depth(sec: &Section, q_target: f64, n: f64, s: f64, k: f64) -> Option<f64> {
    if q_target <= 0.0 {
        return Some(0.0);
    }
    let (q_max, y_max) = section_max_capacity(sec, n, s, k);
    if q_target > q_max {
        return None;
    }
    let (mut lo, mut hi) = (0.0_f64, y_max);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if section_q(sec, n, s, mid, k) > q_target {
            hi = mid;
        } else {
            lo = mid;
        }
        if hi - lo < 1e-7 {
            break;
        }
    }
    Some(0.5 * (lo + hi))
}

/// Critical depth for a target discharge in any [`Section`] (solves `Q^2 T = g A^3`).
pub fn section_critical_depth(sec: &Section, q: f64, g: f64) -> f64 {
    if q <= 0.0 {
        return 0.0;
    }
    let h = sec.height();
    let resid = |y: f64| {
        let (area, _p, _r, top) = sec.geometry(y);
        if area <= 0.0 || top <= 0.0 {
            return -q * q;
        }
        q * q * top - g * area * area * area
    };
    let (mut lo, mut hi) = (1e-6 * h, 0.999 * h);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if resid(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-7 {
            break;
        }
    }
    0.5 * (lo + hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: f64 = K_MANNING_US;

    #[test]
    fn full_area_matches_geometry() {
        // Geometry at y = d must equal the closed-form full area.
        let d = 2.0;
        let (area, perim, r, _t) = circular_geometry(d, d);
        assert!((area - full_area(d)).abs() < 1e-9);
        assert!((perim - PI * d).abs() < 1e-9); // full wetted perimeter = pi*d
        assert!((r - d / 4.0).abs() < 1e-9); // hydraulic radius full = d/4
    }

    #[test]
    fn half_depth_is_half_area() {
        // A circle is exactly half-full at half-diameter depth.
        let d = 3.0;
        let (area, _p, _r, _t) = circular_geometry(d / 2.0, d);
        assert!((area - full_area(d) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn manning_capacity_known_value() {
        // 2-ft pipe, n=0.013, s=0.005  ->  ~16.0 cfs full-flow (hand calc).
        let q = full_flow_capacity(0.013, 0.005, 2.0, K);
        assert!((q - 16.0).abs() < 0.2, "got {q}");
    }

    #[test]
    fn normal_depth_round_trip() {
        // Build a discharge from a depth, then recover the depth.
        let (n, s, d) = (0.013, 0.01, 2.0);
        let y0 = 1.2;
        let q = circular_q(n, s, d, y0, K);
        let y = normal_depth(q, n, s, d, K).expect("below capacity");
        assert!((y - y0).abs() < 1e-3, "got {y}");
    }

    #[test]
    fn surcharge_returns_none() {
        // A tiny pipe cannot pass a huge flow as open channel.
        let y = normal_depth(500.0, 0.013, 0.005, 1.0, K);
        assert!(y.is_none());
    }

    #[test]
    fn max_capacity_exceeds_full() {
        // Peak open-channel flow occurs near 0.94 d and beats just-full.
        let (qmax, ymax) = max_capacity(0.013, 0.005, 2.0, K);
        let qfull = full_flow_capacity(0.013, 0.005, 2.0, K);
        assert!(qmax > qfull);
        assert!(ymax > 1.8 && ymax < 2.0, "y_at_max = {ymax}");
    }

    #[test]
    fn critical_depth_in_range() {
        let yc = critical_depth(10.0, 2.0, G_US);
        assert!(yc > 0.0 && yc < 2.0, "yc = {yc}");
    }

    // ── Non-circular sections ────────────────────────────────────────────────

    #[test]
    fn rectangular_partial_flow_hand_calc() {
        // 4-ft wide box, flow depth 2 ft: A = 8, P = 4 + 2·2 = 8, R = 1.0.
        // Q = (1.49/0.013)·8·1.0^(2/3)·√0.01 = 114.6154·8·0.1 = 91.69 cfs.
        let sec = Section::Rectangular { rise: 3.0, span: 4.0 };
        let (a, p, r, t) = sec.geometry(2.0);
        assert!((a - 8.0).abs() < 1e-9 && (p - 8.0).abs() < 1e-9 && (r - 1.0).abs() < 1e-9);
        assert!((t - 4.0).abs() < 1e-9, "top width = span");
        let q = section_q(&sec, 0.013, 0.01, 2.0, K);
        assert!((q - 91.69).abs() < 0.05, "box Q = {q}, expected 91.69");
    }

    #[test]
    fn rectangular_critical_depth_closed_form() {
        // For a rectangle, y_c = (Q²/(g·b²))^(1/3). Q=50, b=4:
        // (2500/(32.2·16))^(1/3) = 4.8524^(1/3) = 1.693 ft.
        let sec = Section::Rectangular { rise: 3.0, span: 4.0 };
        let yc = section_critical_depth(&sec, 50.0, G_US);
        let closed = (50.0_f64.powi(2) / (G_US * 16.0)).powf(1.0 / 3.0);
        assert!((yc - closed).abs() < 1e-3, "yc {yc} vs closed form {closed}");
        assert!((closed - 1.693).abs() < 0.01);
    }

    #[test]
    fn elliptical_reduces_to_circular_when_axes_equal() {
        // A span==rise ellipse IS a circle — geometry and capacity must match the
        // exact circular results (perimeter is numerically integrated → ~1e-3).
        let d = 2.0;
        let ell = Section::Elliptical { rise: d, span: d };
        let cir = Section::Circular { d };
        for &y in &[0.4, 1.0, 1.6, 2.0] {
            let (ae, pe, re, te) = ell.geometry(y);
            let (ac, pc, rc, tc) = cir.geometry(y);
            assert!((ae - ac).abs() < 1e-6, "area @ {y}: {ae} vs {ac}");
            assert!((te - tc).abs() < 1e-6, "top @ {y}: {te} vs {tc}");
            assert!((pe - pc).abs() < 1e-3, "perimeter @ {y}: {pe} vs {pc}");
            assert!((re - rc).abs() < 1e-3, "radius @ {y}: {re} vs {rc}");
        }
        let qe = section_full_capacity(&ell, 0.013, 0.005, K);
        let qc = full_flow_capacity(0.013, 0.005, d, K);
        assert!((qe - qc).abs() < 0.02, "full capacity: {qe} vs {qc}");
    }

    #[test]
    fn elliptical_full_area_is_pi_ab() {
        // Full elliptical area = π·(span/2)·(rise/2) = π·span·rise/4.
        let sec = Section::Elliptical { rise: 2.0, span: 3.0 };
        assert!((sec.full_area() - PI * 3.0 * 2.0 / 4.0).abs() < 1e-9);
    }

    #[test]
    fn section_normal_depth_round_trip_rectangular() {
        let sec = Section::Rectangular { rise: 3.0, span: 4.0 };
        let (n, s, y0) = (0.013, 0.01, 1.5);
        let q = section_q(&sec, n, s, y0, K);
        let y = section_normal_depth(&sec, q, n, s, K).expect("below capacity");
        assert!((y - y0).abs() < 1e-3, "recovered {y} vs {y0}");
    }

    #[test]
    fn arch_full_area_is_walls_plus_semicircle() {
        // span 4, rise 3 → radius 2, wall 1. Full area = 4·1 + π·2²/2 = 4 + 2π.
        let sec = Section::Arch { rise: 3.0, span: 4.0 };
        assert!((sec.full_area() - (4.0 + 2.0 * PI)).abs() < 1e-9);
        // Full perimeter = span + 2·wall + π·r = 4 + 2 + 2π.
        assert!((sec.full_perimeter() - (6.0 + 2.0 * PI)).abs() < 1e-9);
    }

    #[test]
    fn arch_is_continuous_at_springline() {
        // Just below and just above the springline must agree (no geometry jump).
        let sec = Section::Arch { rise: 3.0, span: 4.0 }; // wall = 1.0
        let (a_lo, p_lo, _, t_lo) = sec.geometry(1.0 - 1e-6);
        let (a_hi, p_hi, _, t_hi) = sec.geometry(1.0 + 1e-6);
        assert!((a_lo - a_hi).abs() < 1e-4, "area {a_lo} vs {a_hi}");
        assert!((p_lo - p_hi).abs() < 1e-4, "perimeter {p_lo} vs {p_hi}");
        assert!((t_lo - t_hi).abs() < 1e-4, "top {t_lo} vs {t_hi}");
        // At the springline the top width equals the span.
        assert!((sec.geometry(1.0).3 - 4.0).abs() < 1e-6);
    }

    #[test]
    fn arch_with_no_walls_is_a_half_circle() {
        // rise = span/2 → no vertical walls → a semicircular dome of radius span/2.
        let span = 4.0;
        let sec = Section::Arch { rise: span / 2.0, span };
        assert!((sec.full_area() - PI * (span / 2.0).powi(2) / 2.0).abs() < 1e-9);
        // Crown top width collapses to a point.
        assert!(sec.geometry(span / 2.0).3.abs() < 1e-6);
    }

    #[test]
    fn section_normal_depth_round_trip_arch() {
        let sec = Section::Arch { rise: 3.0, span: 4.0 };
        let (n, s, y0) = (0.013, 0.01, 2.2); // above the springline
        let q = section_q(&sec, n, s, y0, K);
        let y = section_normal_depth(&sec, q, n, s, K).expect("below capacity");
        assert!((y - y0).abs() < 2e-3, "recovered {y} vs {y0}");
    }
}
