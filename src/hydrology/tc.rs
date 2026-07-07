// SPDX-License-Identifier: GPL-3.0-or-later

//! Time-of-concentration estimators (minutes).

/// Kirpich (1940) — overland flow over unpaved channel.
/// `l` = flow path length (ft), `s` = average slope (ft/ft, positive).
pub fn kirpich_minutes(l: f64, s: f64) -> f64 {
    if l <= 0.0 || s <= 0.0 {
        return 0.0;
    }
    0.0078 * l.powf(0.77) * s.powf(-0.385)
}

/// TR-55 / NRCS sheet-flow travel time (minutes).
///
/// Implements NRCS TR-55 (1986) Eq. 3-3 converted to minutes:
/// `Tt = 0.42 * (n·L)^0.8 / (P2^0.5 · s^0.4)`
///
/// `l` = flow path (ft), `s` = slope (ft/ft), `n` = Manning roughness,
/// `p2_in` = 2-yr 24-hr rainfall (inches).
pub fn tr55_sheet_flow_minutes(l: f64, s: f64, n: f64, p2_in: f64) -> f64 {
    if l <= 0.0 || s <= 0.0 || n <= 0.0 || p2_in <= 0.0 {
        return 0.0;
    }
    0.42 * (n * l).powf(0.8) / (p2_in.powf(0.5) * s.powf(0.4))
}

/// FAA sheet flow on paved surfaces (TR-55 with n = 0.02).
/// `l` = flow path (ft), `s` = slope (ft/ft), `p2_in` = 2-yr 24-hr rainfall (inches).
pub fn faa_sheet_flow_minutes(l: f64, s: f64, p2_in: f64) -> f64 {
    tr55_sheet_flow_minutes(l, s, 0.02, p2_in)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kirpich_increases_with_length() {
        let short = kirpich_minutes(200.0, 0.02);
        let long = kirpich_minutes(800.0, 0.02);
        assert!(long > short);
    }

    #[test]
    fn faa_reasonable_range() {
        // P2 = 3.0 in (typical mid-Atlantic); expect 5–20 min for 300 ft at 1% slope.
        let t = faa_sheet_flow_minutes(300.0, 0.01, 3.0);
        assert!(t > 5.0 && t < 25.0, "t={t}");
    }

    #[test]
    fn tr55_known_value() {
        // n=0.011 (smooth pavement), L=200 ft, s=0.01, P2=2.5 in
        // Tt(hr) = 0.007*(0.011*200)^0.8 / (2.5^0.5 * 0.01^0.4) = ~0.054 hr -> ~3.2 min
        let t = tr55_sheet_flow_minutes(200.0, 0.01, 0.011, 2.5);
        assert!(t > 2.0 && t < 5.0, "t={t}");
    }
}