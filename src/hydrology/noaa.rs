// SPDX-License-Identifier: GPL-3.0-or-later

//! NOAA Atlas 14 precipitation-frequency ingestion.
//!
//! The NOAA Hydrometeorological Design Studies Center publishes point
//! precipitation-frequency estimates (the PFDS "csv" export) as a table of
//! rainfall *depth* (inches) by storm *duration* and *average recurrence
//! interval* (return period). Storm-sewer design instead needs *intensity*
//! (in/hr) as a smooth function of duration, in the three-parameter form
//!
//! ```text
//! i = a / (t + b)^c        (t in minutes, i in in/hr)
//! ```
//!
//! This module parses the NOAA CSV into a [`NoaaTable`], converts depth to
//! intensity, and fits `(a, b, c)` per return period with an ordinary
//! least-squares regression — so a user can paste their site's NOAA data and
//! get a full IDF set without hand-entering coefficients.

use crate::idf::IdfCurve;

/// Parsed NOAA Atlas 14 precipitation-frequency table (depths in inches).
#[derive(Clone, Debug, PartialEq)]
pub struct NoaaTable {
    /// Return periods / recurrence intervals (years), left-to-right as in the file.
    pub return_periods: Vec<u32>,
    /// Storm durations (minutes), top-to-bottom as in the file.
    pub durations_min: Vec<f64>,
    /// Rainfall depth (inches): `depth_in[duration_index][rp_index]`.
    pub depth_in: Vec<Vec<f64>>,
}

impl NoaaTable {
    /// Intensity (in/hr) for a duration row: depth / duration-in-hours.
    fn intensity_row(&self, dur_idx: usize) -> Vec<f64> {
        let hr = self.durations_min[dur_idx] / 60.0;
        self.depth_in[dur_idx].iter().map(|d| d / hr).collect()
    }
}

/// Parse a duration label such as `"5-min"`, `"60-min"`, `"2-hr"`, or `"24-hr"`
/// (optionally with a trailing `:`) into minutes. Returns `None` if it is not a
/// recognizable duration token.
fn parse_duration_minutes(label: &str) -> Option<f64> {
    let s = label.trim().trim_end_matches(':').trim().to_ascii_lowercase();
    let (num, unit) = s.split_once('-')?;
    let value: f64 = num.trim().parse().ok()?;
    let factor = if unit.starts_with("min") {
        1.0
    } else if unit.starts_with("hr") || unit.starts_with("hour") {
        60.0
    } else if unit.starts_with("day") {
        1440.0
    } else {
        return None;
    };
    Some(value * factor)
}

/// Split one CSV line into trimmed, unquoted fields.
fn fields(line: &str) -> Vec<String> {
    line.split(',')
        .map(|f| f.trim().trim_matches('"').trim().to_string())
        .collect()
}

/// Parse a NOAA Atlas 14 PFDS precipitation-frequency CSV export.
///
/// Tolerant of the surrounding metadata lines: it locates the first
/// `by duration for ARI (years):` header (however capitalized), reads the
/// return periods from it, then consumes the duration rows that follow until a
/// non-duration line ends the block. Only the first estimates block is read
/// (the upper/lower 90% confidence blocks that follow are ignored).
pub fn parse_noaa_atlas14_csv(text: &str) -> Result<NoaaTable, String> {
    let mut lines = text.lines().peekable();

    // Find the header row that carries the return periods.
    let mut return_periods: Vec<u32> = Vec::new();
    let mut found_header = false;
    for line in lines.by_ref() {
        let low = line.to_ascii_lowercase();
        if low.contains("by duration for") {
            return_periods = fields(line)
                .into_iter()
                .filter_map(|f| f.parse::<f64>().ok())
                .map(|y| y.round() as u32)
                .collect();
            found_header = true;
            break;
        }
    }
    if !found_header {
        return Err("no 'by duration for ARI (years):' header found — is this a NOAA Atlas 14 CSV?".into());
    }
    if return_periods.is_empty() {
        return Err("could not read return periods from the NOAA header row".into());
    }

    // Consume the duration rows immediately following the header.
    let mut durations_min = Vec::new();
    let mut depth_in = Vec::new();
    for line in lines.by_ref() {
        let cells = fields(line);
        if cells.is_empty() {
            break;
        }
        let Some(dur) = parse_duration_minutes(&cells[0]) else {
            // First non-duration row ends the estimates block.
            if durations_min.is_empty() {
                continue; // tolerate a blank/units line between header and data
            }
            break;
        };
        let depths: Vec<f64> = cells[1..].iter().filter_map(|f| f.parse::<f64>().ok()).collect();
        if depths.len() < return_periods.len() {
            return Err(format!(
                "duration row '{}' has {} values but {} return periods",
                cells[0],
                depths.len(),
                return_periods.len()
            ));
        }
        durations_min.push(dur);
        depth_in.push(depths[..return_periods.len()].to_vec());
    }

    if durations_min.len() < 3 {
        return Err("need at least 3 duration rows to fit an IDF curve".into());
    }
    Ok(NoaaTable { return_periods, durations_min, depth_in })
}

/// Fit `i = a/(t+b)^c` to duration/intensity pairs by ordinary least squares.
///
/// For a fixed `b`, taking logs makes the model *linear*:
/// `ln i = ln a − c·ln(t+b)`, solved in closed form. We scan `b` over a
/// physical range and keep the value giving the smallest residual sum of
/// squares in log space. Requires ≥ 3 points; panics otherwise (callers parse
/// tables that already guarantee this).
pub fn fit_idf_curve(durations_min: &[f64], intensity_in_hr: &[f64]) -> IdfCurve {
    assert!(durations_min.len() >= 3, "need ≥3 points to fit an IDF curve");
    assert_eq!(durations_min.len(), intensity_in_hr.len());

    let ln_i: Vec<f64> = intensity_in_hr.iter().map(|&i| i.max(1e-9).ln()).collect();
    let n = durations_min.len() as f64;

    // Closed-form log-linear fit for a given b; returns (a, c, rss).
    let fit_for_b = |b: f64| -> (f64, f64, f64) {
        let x: Vec<f64> = durations_min.iter().map(|&t| (t + b).max(1e-9).ln()).collect();
        let sx: f64 = x.iter().sum();
        let sy: f64 = ln_i.iter().sum();
        let sxx: f64 = x.iter().map(|v| v * v).sum();
        let sxy: f64 = x.iter().zip(&ln_i).map(|(a, b)| a * b).sum();
        let denom = n * sxx - sx * sx;
        if denom.abs() < 1e-12 {
            return (0.0, 0.0, f64::INFINITY);
        }
        let slope = (n * sxy - sx * sy) / denom; // = −c
        let intercept = (sy - slope * sx) / n; //  = ln a
        let c = -slope;
        let a = intercept.exp();
        let rss: f64 = x
            .iter()
            .zip(&ln_i)
            .map(|(xi, yi)| {
                let pred = intercept + slope * xi;
                (yi - pred).powi(2)
            })
            .sum();
        (a, c, rss)
    };

    // Coarse scan over b, then a local refinement around the best coarse value.
    let mut best = (0.0_f64, f64::INFINITY, 0.0_f64, 0.0_f64); // (b, rss, a, c)
    let mut b = 0.0;
    while b <= 40.0 {
        let (a, c, rss) = fit_for_b(b);
        if rss < best.1 {
            best = (b, rss, a, c);
        }
        b += 0.25;
    }
    let mut lo = (best.0 - 0.25).max(0.0);
    let mut hi = best.0 + 0.25;
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        let (_, _, rss_lo) = fit_for_b(0.5 * (lo + mid));
        let (_, _, rss_hi) = fit_for_b(0.5 * (mid + hi));
        if rss_lo < rss_hi {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let bb = 0.5 * (lo + hi);
    let (a, c, _) = fit_for_b(bb);
    IdfCurve::new(a, bb, c)
}

/// Convert a parsed NOAA table into fitted IDF curves, one per return period.
///
/// Only durations `≤ max_duration_min` are used for the fit; the short-duration
/// end dominates storm-sewer design, and including multi-hour rows would bias
/// the three-parameter form. Pass e.g. `180.0` to fit through the 3-hour row.
pub fn noaa_to_idf_curves(table: &NoaaTable, max_duration_min: f64) -> Vec<(u32, IdfCurve)> {
    let use_idx: Vec<usize> = (0..table.durations_min.len())
        .filter(|&i| table.durations_min[i] <= max_duration_min)
        .collect();
    let durs: Vec<f64> = use_idx.iter().map(|&i| table.durations_min[i]).collect();

    table
        .return_periods
        .iter()
        .enumerate()
        .map(|(rp_idx, &rp)| {
            let intens: Vec<f64> = use_idx
                .iter()
                .map(|&di| table.intensity_row(di)[rp_idx])
                .collect();
            (rp, fit_idf_curve(&durs, &intens))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed but realistic PFDS export (estimates block only).
    const SAMPLE: &str = "\
Point precipitation frequency estimates (inches) - NOAA Atlas 14
Data type: Precipitation depth
by duration for ARI (years):,1,2,5,10,25,50,100
5-min:,0.276,0.330,0.410,0.475,0.564,0.635,0.708
10-min:,0.404,0.483,0.601,0.696,0.826,0.930,1.036
15-min:,0.500,0.597,0.744,0.862,1.024,1.153,1.284
30-min:,0.673,0.804,1.001,1.160,1.379,1.552,1.729
60-min:,0.834,0.997,1.241,1.438,1.709,1.924,2.143
2-hr:,0.977,1.168,1.454,1.685,2.003,2.255,2.512
3-hr:,1.068,1.277,1.590,1.842,2.190,2.465,2.746
6-hr:,1.303,1.558,1.940,2.248,2.673,3.009,3.352
Upper bound of the 90% confidence interval
by duration for ARI (years):,1,2,5,10,25,50,100
5-min:,0.300,0.360,0.450,0.520,0.620,0.700,0.780
";

    #[test]
    fn parses_durations_and_return_periods() {
        let t = parse_noaa_atlas14_csv(SAMPLE).unwrap();
        assert_eq!(t.return_periods, vec![1, 2, 5, 10, 25, 50, 100]);
        // Only the estimates block — the confidence block after it is ignored.
        assert_eq!(t.durations_min, vec![5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 180.0, 360.0]);
        assert_eq!(t.depth_in.len(), 8);
        assert!((t.depth_in[0][0] - 0.276).abs() < 1e-9);
        assert!((t.depth_in[4][3] - 1.438).abs() < 1e-9); // 60-min, 10-yr
    }

    #[test]
    fn duration_label_parsing() {
        assert_eq!(parse_duration_minutes("5-min:"), Some(5.0));
        assert_eq!(parse_duration_minutes("60-min"), Some(60.0));
        assert_eq!(parse_duration_minutes("2-hr:"), Some(120.0));
        assert_eq!(parse_duration_minutes("24-hr"), Some(1440.0));
        assert_eq!(parse_duration_minutes("2-day:"), Some(2880.0));
        assert_eq!(parse_duration_minutes("total"), None);
        assert_eq!(parse_duration_minutes(""), None);
    }

    #[test]
    fn fit_recovers_known_curve() {
        // Generate intensities from a known curve, then fit and recover it.
        let a0 = 96.0;
        let b0 = 12.0;
        let c0 = 0.82;
        let true_curve = IdfCurve::new(a0, b0, c0);
        let durs = [5.0, 10.0, 15.0, 30.0, 60.0, 120.0];
        let intens: Vec<f64> = durs.iter().map(|&t| true_curve.intensity(t)).collect();

        let fit = fit_idf_curve(&durs, &intens);
        // Recovered coefficients should be very close (noise-free data).
        assert!((fit.a - a0).abs() / a0 < 0.02, "a {} vs {a0}", fit.a);
        assert!((fit.b - b0).abs() < 0.5, "b {} vs {b0}", fit.b);
        assert!((fit.c - c0).abs() < 0.02, "c {} vs {c0}", fit.c);
    }

    #[test]
    fn fitted_curve_reproduces_intensities() {
        // The fitted curve should reproduce each NOAA intensity within a few
        // percent across the fitted duration range.
        let t = parse_noaa_atlas14_csv(SAMPLE).unwrap();
        let curves = noaa_to_idf_curves(&t, 180.0);
        assert_eq!(curves.len(), 7);

        // Check the 10-yr curve (index 3) against the source depths.
        let (rp, curve) = &curves[3];
        assert_eq!(*rp, 10);
        for (di, &dur) in t.durations_min.iter().enumerate() {
            if dur > 180.0 {
                continue;
            }
            let src_intensity = t.depth_in[di][3] / (dur / 60.0);
            let fit_intensity = curve.intensity(dur);
            let rel = (fit_intensity - src_intensity).abs() / src_intensity;
            assert!(rel < 0.06, "dur {dur}: fit {fit_intensity} vs {src_intensity} (rel {rel:.3})");
        }
    }

    #[test]
    fn intensity_increases_with_return_period() {
        let t = parse_noaa_atlas14_csv(SAMPLE).unwrap();
        let curves = noaa_to_idf_curves(&t, 120.0);
        // At a fixed 15-min duration, rarer storms are more intense.
        let i_2yr = curves.iter().find(|(rp, _)| *rp == 2).unwrap().1.intensity(15.0);
        let i_100yr = curves.iter().find(|(rp, _)| *rp == 100).unwrap().1.intensity(15.0);
        assert!(i_100yr > i_2yr, "100-yr {i_100yr} !> 2-yr {i_2yr}");
    }

    #[test]
    fn rejects_non_noaa_text() {
        assert!(parse_noaa_atlas14_csv("hello,world\n1,2,3").is_err());
    }
}
