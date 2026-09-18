// SPDX-License-Identifier: GPL-3.0-or-later

//! Objective functions against hand-computed values, the Moriasi ratings,
//! interpolation to observed times, and observed-series import.

use stormsewer_swmm::calib::objectives::{interpolate, rating, Metrics, Objective, Paired, Rating};
use stormsewer_swmm::calib::observed::{days_from_civil, parse_observed, ObsTime, ObservedSeries, Variable};
use stormsewer_swmm::out::{decode_datetime, Series};

fn series(t: &[f64], v: &[f64]) -> Series {
    Series {
        times_s: t.to_vec(),
        values: v.to_vec(),
    }
}

fn paired(obs: &[f64], sim: &[f64]) -> Paired {
    let t: Vec<f64> = (0..obs.len()).map(|i| i as f64 * 60.0).collect();
    Paired {
        times: t,
        obs: obs.to_vec(),
        sim: sim.to_vec(),
    }
}

#[test]
fn nse_is_one_for_a_perfect_fit_and_zero_for_the_mean() {
    let obs = [1.0, 3.0, 2.0, 5.0, 4.0];
    let m = Metrics::of(&paired(&obs, &obs));
    assert!((m.nse - 1.0).abs() < 1e-12);
    assert!((m.kge - 1.0).abs() < 1e-12);
    assert!(m.rmse.abs() < 1e-12);
    assert!(m.pbias.abs() < 1e-12);
    assert!(m.rsr.abs() < 1e-12);
    assert!(m.peak_error_pct.abs() < 1e-12);
    assert!(m.volume_error_pct.abs() < 1e-12);
    assert_eq!(m.time_to_peak_error_s, 0.0);
    let mean = [3.0; 5];
    let m = Metrics::of(&paired(&obs, &mean));
    assert!(m.nse.abs() < 1e-12, "predicting the mean scores NSE = 0: {}", m.nse);
    assert!((m.rsr - 1.0).abs() < 1e-12, "RSR = RMSE/σ = 1 for the mean");
    assert!(m.pbias.abs() < 1e-12, "the mean has no bias");
    // Objectives to minimise: perfect fit = 0 for all.
    let perfect = Metrics::of(&paired(&obs, &obs));
    for o in Objective::ALL {
        assert!(o.minimized(&perfect).abs() < 1e-12, "{}", o.label());
    }
}

#[test]
fn pbias_sign_follows_moriasi_positive_means_underestimation() {
    let obs = [10.0, 10.0, 10.0, 10.0];
    let low = [8.0, 8.0, 8.0, 8.0];
    let m = Metrics::of(&paired(&obs, &low));
    // 100·Σ(o−s)/Σo = 100·8/40 = 20 %.
    assert!((m.pbias - 20.0).abs() < 1e-9, "{}", m.pbias);
    assert_eq!(rating(Objective::Pbias, m.pbias), Some(Rating::Satisfactory));
    let high = [12.0, 12.0, 12.0, 12.0];
    let m = Metrics::of(&paired(&obs, &high));
    assert!((m.pbias + 20.0).abs() < 1e-9);
    // Volume and peak errors are (sim − obs)/obs, the opposite sign.
    assert!((m.volume_error_pct - 20.0).abs() < 1e-9);
    assert!((m.peak_error_pct - 20.0).abs() < 1e-9);
    assert!((m.rmse - 2.0).abs() < 1e-12);
}

#[test]
fn kge_components_are_correlation_variability_and_bias() {
    let obs = [1.0, 2.0, 3.0, 4.0, 5.0];
    // sim = 2·obs: r = 1, α = 2, β = 2 → KGE = 1 − √2.
    let sim: Vec<f64> = obs.iter().map(|v| 2.0 * v).collect();
    let m = Metrics::of(&paired(&obs, &sim));
    assert!((m.kge_r - 1.0).abs() < 1e-12);
    assert!((m.kge_alpha - 2.0).abs() < 1e-12);
    assert!((m.kge_beta - 2.0).abs() < 1e-12);
    assert!((m.kge - (1.0 - 2f64.sqrt())).abs() < 1e-12);
    // sim = obs + 3: r = 1, α = 1, β = 2 → KGE = 0.
    let sim: Vec<f64> = obs.iter().map(|v| v + 3.0).collect();
    let m = Metrics::of(&paired(&obs, &sim));
    assert!((m.kge_beta - 2.0).abs() < 1e-12);
    assert!(m.kge.abs() < 1e-12, "{}", m.kge);
    // Reversed: r = −1.
    let sim = [5.0, 4.0, 3.0, 2.0, 1.0];
    let m = Metrics::of(&paired(&obs, &sim));
    assert!((m.kge_r + 1.0).abs() < 1e-12);
    // Time to peak: sim peaks 4 minutes early.
    let obs = [0.0, 1.0, 2.0, 5.0, 3.0, 1.0, 0.0];
    let sim = [1.0, 5.0, 3.0, 2.0, 1.0, 0.0, 0.0];
    let m = Metrics::of(&paired(&obs, &sim));
    assert_eq!(m.time_to_peak_error_s, -120.0);
    assert!((Objective::TimeToPeak.minimized(&m) - 120.0 / 3600.0).abs() < 1e-12);
    // Empty pairing: everything NaN, nothing panics.
    let m = Metrics::of(&Paired::default());
    assert!(m.nse.is_nan() && m.kge.is_nan());
    assert_eq!(rating(Objective::Nse, m.nse), None);
}

#[test]
fn moriasi_ratings_follow_table_4() {
    use Rating::*;
    let cases = [
        (Objective::Nse, 0.80, VeryGood),
        (Objective::Nse, 0.75, Good),
        (Objective::Nse, 0.70, Good),
        (Objective::Nse, 0.65, Satisfactory),
        (Objective::Nse, 0.55, Satisfactory),
        (Objective::Nse, 0.50, Unsatisfactory),
        (Objective::Nse, -1.0, Unsatisfactory),
        (Objective::Rsr, 0.50, VeryGood),
        (Objective::Rsr, 0.55, Good),
        (Objective::Rsr, 0.65, Satisfactory),
        (Objective::Rsr, 0.71, Unsatisfactory),
        (Objective::Pbias, 9.9, VeryGood),
        (Objective::Pbias, -12.0, Good),
        (Objective::Pbias, 20.0, Satisfactory),
        (Objective::Pbias, -25.0, Unsatisfactory),
    ];
    for (o, v, want) in cases {
        assert_eq!(rating(o, v), Some(want), "{} {v}", o.label());
    }
    for o in [Objective::Kge, Objective::Rmse, Objective::PeakError, Objective::VolumeError, Objective::TimeToPeak] {
        assert_eq!(rating(o, 0.5), None, "{} has no Moriasi rating", o.label());
    }
    assert_eq!(Rating::VeryGood.label(), "very good");
}

#[test]
fn interpolation_hits_the_observed_times_and_drops_outsiders() {
    let sim = series(&[300.0, 600.0, 900.0, 1200.0], &[0.0, 10.0, 20.0, 10.0]);
    let got = interpolate(&sim, &[300.0, 450.0, 900.0, 1050.0, 1200.0, 100.0, 2000.0]);
    let vals: Vec<f64> = got.iter().map(|(_, v)| *v).collect();
    assert_eq!(vals[0], 0.0);
    assert!((vals[1] - 5.0).abs() < 1e-12);
    assert_eq!(vals[2], 20.0);
    assert!((vals[3] - 15.0).abs() < 1e-12);
    assert_eq!(vals[4], 10.0);
    assert!(vals[5].is_nan() && vals[6].is_nan());
    let p = Paired::new(&[100.0, 450.0, 2000.0], &[1.0, 2.0, 3.0], &sim);
    assert_eq!(p.len(), 1);
    assert_eq!(p.obs, vec![2.0]);
    assert!((p.sim[0] - 5.0).abs() < 1e-12);
    // Unsorted observed times still pair correctly.
    let p = Paired::new(&[900.0, 450.0], &[1.0, 2.0], &sim);
    assert_eq!(p.sim, vec![20.0, 5.0]);
}

#[test]
fn observed_text_formats_are_detected() {
    // Dated rows, comma, header.
    let imp = parse_observed("datetime,flow\n2007-01-01 00:05,1.5\n2007-01-01 00:10,2.5\n").unwrap();
    assert!(imp.had_header);
    assert_eq!(imp.values, vec![1.5, 2.5]);
    let ObsTime::Absolute(d) = imp.times[0] else { panic!() };
    let (y, mo, dd, h, mi, _) = decode_datetime(d);
    assert_eq!((y, mo, dd, h, mi), (2007, 1, 1, 0, 5));
    // date, time, value with tabs and a slash date.
    let imp = parse_observed("01/01/2007\t0:15\t3\n01/01/2007\t0:20\t4\n").unwrap();
    assert!(!imp.had_header);
    let ObsTime::Absolute(d) = imp.times[1] else { panic!() };
    assert_eq!(decode_datetime(d).4, 20);
    // ISO T separator.
    let imp = parse_observed("2007-01-01T01:00:00,7\n").unwrap();
    let ObsTime::Absolute(d) = imp.times[0] else { panic!() };
    assert_eq!(decode_datetime(d).3, 1);
    // Time-only rows are relative seconds.
    let imp = parse_observed("time;value\n0:05;1\n0:10;2\n").unwrap();
    assert_eq!(imp.times, vec![ObsTime::Relative(300.0), ObsTime::Relative(600.0)]);
    // Numeric first column: seconds by default, hours when the header says.
    let imp = parse_observed("seconds value\n300 1\n600 2\n").unwrap();
    assert_eq!(imp.times[1], ObsTime::Relative(600.0));
    let imp = parse_observed("hours,value\n0.5,1\n1,2\n").unwrap();
    assert_eq!(imp.times[0], ObsTime::Relative(1800.0));
    let imp = parse_observed("minutes,value\n5,1\n").unwrap();
    assert_eq!(imp.times[0], ObsTime::Relative(300.0));
    // Bad rows are counted; comments skipped.
    let imp = parse_observed("; a note\n0:05,1\nabc,def\n0:10,2\n").unwrap();
    assert_eq!(imp.values.len(), 2);
    assert_eq!(imp.warnings.len(), 1);
    assert!(parse_observed("").is_err());
    assert!(parse_observed("a,b\n").is_err());
    assert!(parse_observed("0:05,1\n2007-01-01 00:10,2\n").is_err(), "mixed stamps");
    // Seconds from the model start, both ways.
    let s = ObservedSeries {
        name: "obs".into(),
        id: "O2".into(),
        variable: Variable::SystemInflow,
        times: vec![ObsTime::Absolute(days_from_civil(2007, 1, 1) + 300.0 / 86_400.0), ObsTime::Relative(900.0)],
        values: vec![1.0, 2.0],
        source: String::new(),
    };
    let secs = s.seconds_from(days_from_civil(2007, 1, 1));
    assert!((secs[0] - 300.0).abs() < 1e-6);
    assert_eq!(secs[1], 900.0);
    assert_eq!(s.label(), "O2 · System inflow at outfall");
    assert_eq!(Variable::ALL.len(), 9);
}

#[test]
fn days_from_civil_inverts_decode_datetime() {
    for (y, m, d) in [(1899, 12, 30), (1970, 1, 1), (2007, 1, 1), (2024, 2, 29), (2100, 12, 31), (2000, 3, 1)] {
        let days = days_from_civil(y, m, d);
        let (yy, mm, dd, h, mi, s) = decode_datetime(days);
        assert_eq!((yy, mm, dd, h, mi, s), (y, m, d, 0, 0, 0), "{y}-{m}-{d}");
    }
    assert_eq!(days_from_civil(1899, 12, 30), 0.0);
    assert_eq!(days_from_civil(1970, 1, 1), 25_569.0);
}
