// SPDX-License-Identifier: GPL-3.0-or-later

//! Known-point checks for the coordinate systems.
//!
//! Reference values are from PROJ 9.5.1 (via pyproj 3.7.1), computed on
//! this machine on 2026-09-18 with
//! `Transformer.from_crs("EPSG:4326", "EPSG:<code>", always_xy=True)`;
//! PROJ's own conformance suite is checked against the EPSG Guidance
//! Note 7-2 worked examples, so agreement to a millimetre here is
//! agreement with the published formulas. The transverse Mercator is
//! additionally cross-checked against an independent implementation of
//! Snyder (1987) eqs. 8-9 to 8-15 (the USGS series) written in this test.

use stormsewer_swmm::gis::crs::{transform, Crs, Datum};

fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    (a.0 - b.0).abs() < tol && (a.1 - b.1).abs() < tol
}

/// Snyder (1987) transverse Mercator forward, eqs. 8-9 .. 8-15, for the
/// ellipsoid — the USGS series, independent of the Krüger series the
/// crate uses. Returns (x, y) in metres including the false origin.
fn snyder_tm(a: f64, inv_f: f64, k0: f64, lon0: f64, fe: f64, fn_: f64, lon: f64, lat: f64) -> (f64, f64) {
    let f = 1.0 / inv_f;
    let e2 = 2.0 * f - f * f;
    let ep2 = e2 / (1.0 - e2);
    let phi = lat.to_radians();
    let dl = (lon - lon0).to_radians();
    let n = a / (1.0 - e2 * phi.sin().powi(2)).sqrt();
    let t = phi.tan().powi(2);
    let c = ep2 * phi.cos().powi(2);
    let a_ = dl * phi.cos();
    let m = |phi: f64| {
        a * ((1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0 - 5.0 * e2.powi(3) / 256.0) * phi
            - (3.0 * e2 / 8.0 + 3.0 * e2 * e2 / 32.0 + 45.0 * e2.powi(3) / 1024.0) * (2.0 * phi).sin()
            + (15.0 * e2 * e2 / 256.0 + 45.0 * e2.powi(3) / 1024.0) * (4.0 * phi).sin()
            - (35.0 * e2.powi(3) / 3072.0) * (6.0 * phi).sin())
    };
    let x = k0 * n * (a_ + (1.0 - t + c) * a_.powi(3) / 6.0 + (5.0 - 18.0 * t + t * t + 72.0 * c - 58.0 * ep2) * a_.powi(5) / 120.0);
    let y = k0
        * (m(phi) - m(0.0)
            + n * phi.tan()
                * (a_.powi(2) / 2.0
                    + (5.0 - t + 9.0 * c + 4.0 * c * c) * a_.powi(4) / 24.0
                    + (61.0 - 58.0 * t + t * t + 600.0 * c - 330.0 * ep2) * a_.powi(6) / 720.0));
    (x + fe, y + fn_)
}

const CHARLOTTE: (f64, f64) = (-80.8431, 35.2271);

#[test]
fn utm_17n_charlotte_matches_proj_and_the_snyder_series() {
    let utm = Crs::from_epsg(32617).unwrap();
    let (x, y) = utm.forward(CHARLOTTE.0, CHARLOTTE.1).unwrap();
    // PROJ 9.5.1: 514277.7208, 3898239.3400
    assert!(close((x, y), (514277.7208, 3898239.3400), 0.001), "{x} {y}");
    let s = snyder_tm(6378137.0, 298.257223563, 0.9996, -81.0, 500000.0, 0.0, CHARLOTTE.0, CHARLOTTE.1);
    assert!(close((x, y), s, 0.002), "Snyder series: {s:?} vs {x} {y}");
    // Points 3° either side of the central meridian, where the USGS
    // series is within a couple of millimetres of the exact projection.
    for (lon, lat, px, py) in [(-78.0, 36.5, 768701.9968, 4043594.6356), (-83.9, 33.5, 230587.1192, 3710484.3254)] {
        let (x, y) = utm.forward(lon, lat).unwrap();
        assert!(close((x, y), (px, py), 0.001), "{lon},{lat}: {x} {y}");
        let s = snyder_tm(6378137.0, 298.257223563, 0.9996, -81.0, 500000.0, 0.0, lon, lat);
        assert!(close((x, y), s, 0.005), "Snyder series at {lon}: {s:?} vs {x} {y}");
    }
    // Southern hemisphere: the 10,000 km false northing.
    let south = Crs::from_epsg(32717).unwrap();
    let (x, y) = south.forward(CHARLOTTE.0, -CHARLOTTE.1).unwrap();
    assert!(close((x, y), (514277.7208, 6101760.6600), 0.001), "{x} {y}");
    let (lon, lat) = south.inverse(x, y).unwrap();
    assert!(close((lon, lat), (CHARLOTTE.0, -CHARLOTTE.1), 1e-9));
}

#[test]
fn north_carolina_state_plane_in_feet_and_metres() {
    let ft = Crs::from_epsg(2264).unwrap();
    let (x, y) = ft.forward(CHARLOTTE.0, CHARLOTTE.1).unwrap();
    // PROJ 9.5.1 EPSG:2264 (US survey feet): 1449620.3963, 542689.0712.
    // No NGS-published coordinate for this exact point is cited; the
    // check is against PROJ, plus the round trip below.
    assert!(close((x, y), (1449620.3963, 542689.0712), 0.002), "{x} {y}");
    let m = Crs::from_epsg(32119).unwrap();
    let (xm, ym) = m.forward(CHARLOTTE.0, CHARLOTTE.1).unwrap();
    // PROJ 9.5.1 EPSG:32119 (metres): 441845.1813, 165411.9597
    assert!(close((xm, ym), (441845.1813, 165411.9597), 0.001), "{xm} {ym}");
    assert!((x * 1200.0 / 3937.0 - xm).abs() < 0.001, "feet and metres agree");
    let (lon, lat) = ft.inverse(x, y).unwrap();
    assert!(close((lon, lat), CHARLOTTE, 1e-9), "{lon} {lat}");
    // Between the two forms directly.
    let (x2, y2) = transform(&m, &ft, xm, ym).unwrap();
    assert!(close((x2, y2), (x, y), 1e-6 / 0.3048));
}

#[test]
fn other_projections_match_proj() {
    let cases: [(u32, f64, f64, f64, f64); 8] = [
        (3857, CHARLOTTE.0, CHARLOTTE.1, -8999412.7261, 4194786.1142),
        (26931, -134.4, 58.3, 775672.4620, 720098.3296),
        (26931, -131.0, 55.0, 989301.7079, 355740.9154),
        (3031, CHARLOTTE.0, -75.0, -1617899.0016, 260793.6411),
        (3413, -45.0, 80.0, 0.0, -1085920.2974),
        (5070, CHARLOTTE.0, CHARLOTTE.1, 1362114.7671, 1461065.9222),
        (26948, -110.0, 33.0, 228933.9872, 221763.8792),
        (32137, -101.0, 35.0, 245641.0288, 1111052.4898),
    ];
    for (code, lon, lat, px, py) in cases {
        let crs = Crs::from_epsg(code).unwrap();
        let (x, y) = crs.forward(lon, lat).unwrap();
        assert!(close((x, y), (px, py), 0.002), "EPSG:{code} ({lon},{lat}): got {x} {y}, PROJ {px} {py}");
        let (lon2, lat2) = crs.inverse(x, y).unwrap();
        assert!(close((lon2, lat2), (lon, lat), 1e-9), "EPSG:{code} inverse: {lon2} {lat2}");
    }
    // California zone 1 in US feet (EPSG 2225): PROJ 6423664.6281, 2247969.6983.
    let ca = Crs::from_epsg(2225).unwrap();
    let (x, y) = ca.forward(-122.5, 41.0).unwrap();
    assert!(close((x, y), (6423664.6281, 2247969.6983), 0.005), "{x} {y}");
}

#[test]
fn transform_between_systems_and_datum_rules() {
    let utm = Crs::from_epsg(32617).unwrap();
    let nc = Crs::from_epsg(2264).unwrap();
    let (e, n) = transform(&Crs::wgs84(), &utm, CHARLOTTE.0, CHARLOTTE.1).unwrap();
    let (x, y) = transform(&utm, &nc, e, n).unwrap();
    assert!(close((x, y), (1449620.3963, 542689.0712), 0.003), "{x} {y}");
    // NAD83 geographic to WGS 84 UTM: allowed (one datum, ~1-2 m caveat).
    let nad83 = Crs::nad83();
    assert_eq!(nad83.datum, Datum::Nad83);
    let (e2, n2) = transform(&nad83, &utm, CHARLOTTE.0, CHARLOTTE.1).unwrap();
    assert!(close((e2, n2), (e, n), 1e-9));
    // NAD27 anything: refused with the reason.
    let nad27 = Crs::from_epsg(26717).unwrap();
    let err = transform(&nad27, &utm, e, n).unwrap_err().to_string();
    assert!(err.contains("unsupported datum shift"), "{err}");
    // Web Mercator (sphere) to and from WGS 84 is permitted.
    let wm = Crs::from_epsg(3857).unwrap();
    let (lon, lat) = transform(&wm, &Crs::wgs84(), -8999412.7261, 4194786.1142).unwrap();
    assert!(close((lon, lat), CHARLOTTE, 1e-7), "{lon} {lat}");
}
