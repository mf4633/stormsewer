// SPDX-License-Identifier: GPL-3.0-or-later

//! Small result-analysis pieces the views share: five-class colour breaks
//! for a map legend, and statistics over one reported series.

use crate::out::Series;

/// Four ordered break values dividing a variable into five classes.
///
/// Class `k` holds values `v` with `breaks[k-1] <= v < breaks[k]`; the first
/// class is everything below `breaks[0]`, the last everything at or above
/// `breaks[3]`. The breaks are user-editable, so [`ClassBreaks::normalize`]
/// re-sorts them after an edit rather than trusting the order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassBreaks {
    pub breaks: [f64; 4],
}

impl Default for ClassBreaks {
    fn default() -> Self {
        Self::equal_interval(0.0, 1.0)
    }
}

impl ClassBreaks {
    /// Five equal-width classes between `min` and `max`.
    pub fn equal_interval(min: f64, max: f64) -> Self {
        let (min, max) = if min.is_finite() && max.is_finite() && max > min {
            (min, max)
        } else if min.is_finite() {
            (min, min + 1.0)
        } else {
            (0.0, 1.0)
        };
        let step = (max - min) / 5.0;
        Self {
            breaks: [
                min + step,
                min + 2.0 * step,
                min + 3.0 * step,
                min + 4.0 * step,
            ],
        }
    }

    /// Breaks at the 20/40/60/80 % points of the given values, so each class
    /// holds about a fifth of the objects. Falls back to equal intervals when
    /// there are too few finite values to rank.
    pub fn quantiles(values: &[f64]) -> Self {
        let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
        if v.len() < 5 {
            let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            return Self::equal_interval(lo, hi);
        }
        v.sort_by(|a, b| a.total_cmp(b));
        let at = |q: f64| {
            let pos = q * (v.len() - 1) as f64;
            let i = pos.floor() as usize;
            let t = pos - i as f64;
            let a = v[i];
            let b = v[(i + 1).min(v.len() - 1)];
            a + (b - a) * t
        };
        let mut s = Self {
            breaks: [at(0.2), at(0.4), at(0.6), at(0.8)],
        };
        s.normalize();
        s
    }

    /// Sort the breaks ascending. Called after an edit.
    pub fn normalize(&mut self) {
        self.breaks.sort_by(|a, b| a.total_cmp(b));
    }

    /// The class (0..5) of a value. NaN lands in the first class.
    pub fn class_of(&self, v: f64) -> usize {
        if !v.is_finite() {
            return 0;
        }
        self.breaks.iter().filter(|b| v >= **b).count()
    }

    /// One label per class, e.g. `"< 0.20"`, `"0.20 – 0.40"`, `"≥ 0.80"`.
    pub fn labels(&self, decimals: usize) -> [String; 5] {
        let f = |v: f64| format!("{v:.decimals$}");
        let b = &self.breaks;
        [
            format!("< {}", f(b[0])),
            format!("{} – {}", f(b[0]), f(b[1])),
            format!("{} – {}", f(b[1]), f(b[2])),
            format!("{} – {}", f(b[2]), f(b[3])),
            format!("≥ {}", f(b[3])),
        ]
    }

    /// Decimal places that tell the classes apart for a legend.
    pub fn decimals(&self) -> usize {
        let span = (self.breaks[3] - self.breaks[0]).abs();
        if span >= 100.0 {
            0
        } else if span >= 10.0 {
            1
        } else if span >= 1.0 {
            2
        } else {
            3
        }
    }
}

/// Summary numbers for one series.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeriesStats {
    pub n: usize,
    pub min: f64,
    /// Seconds from start at which the minimum first occurs.
    pub min_at_s: f64,
    pub max: f64,
    pub max_at_s: f64,
    pub mean: f64,
    /// Plain sum of the reported values.
    pub sum: f64,
    /// Trapezoidal integral of value × time, in value-units·seconds. For a
    /// flow this is the volume passed.
    pub integral: f64,
}

/// Statistics over a series, or `None` when it has no finite values.
pub fn stats(series: &Series) -> Option<SeriesStats> {
    let mut n = 0usize;
    let mut min = f64::INFINITY;
    let mut min_at_s = 0.0;
    let mut max = f64::NEG_INFINITY;
    let mut max_at_s = 0.0;
    let mut sum = 0.0;
    for (t, v) in series.times_s.iter().zip(&series.values) {
        if !v.is_finite() {
            continue;
        }
        n += 1;
        sum += v;
        if *v < min {
            min = *v;
            min_at_s = *t;
        }
        if *v > max {
            max = *v;
            max_at_s = *t;
        }
    }
    if n == 0 {
        return None;
    }
    let mut integral = 0.0;
    for (ts, vs) in series.times_s.windows(2).zip(series.values.windows(2)) {
        if vs[0].is_finite() && vs[1].is_finite() {
            integral += 0.5 * (vs[0] + vs[1]) * (ts[1] - ts[0]);
        }
    }
    Some(SeriesStats {
        n,
        min,
        min_at_s,
        max,
        max_at_s,
        mean: sum / n as f64,
        sum,
        integral,
    })
}

/// The `n` largest local maxima of a series as `(time_s, value)`, largest
/// first. A local maximum is a sample no smaller than both neighbours and
/// strictly larger than at least one, so a flat run counts once (at its
/// start) and a constant series has no peaks. The end samples count when they
/// exceed their one neighbour.
pub fn top_peaks(series: &Series, n: usize) -> Vec<(f64, f64)> {
    let v = &series.values;
    let t = &series.times_s;
    let len = v.len().min(t.len());
    let mut peaks: Vec<(f64, f64)> = Vec::new();
    for i in 0..len {
        let cur = v[i];
        if !cur.is_finite() {
            continue;
        }
        let left = if i > 0 { Some(v[i - 1]) } else { None };
        let right = if i + 1 < len { Some(v[i + 1]) } else { None };
        let ge_left = left.is_none_or(|l| cur >= l);
        let ge_right = right.is_none_or(|r| cur >= r);
        let gt_any = left.is_some_and(|l| cur > l) || right.is_some_and(|r| cur > r);
        // Skip the interior of a plateau: only its first sample is the peak.
        let plateau_continuation = left.is_some_and(|l| l == cur);
        if ge_left && ge_right && gt_any && !plateau_continuation {
            peaks.push((t[i], cur));
        }
    }
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
    peaks.truncate(n);
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_interval_classes() {
        let b = ClassBreaks::equal_interval(0.0, 10.0);
        assert_eq!(b.breaks, [2.0, 4.0, 6.0, 8.0]);
        assert_eq!(b.class_of(-1.0), 0);
        assert_eq!(b.class_of(1.99), 0);
        assert_eq!(b.class_of(2.0), 1);
        assert_eq!(b.class_of(7.5), 3);
        assert_eq!(b.class_of(8.0), 4);
        assert_eq!(b.class_of(100.0), 4);
        assert_eq!(b.class_of(f64::NAN), 0);
        assert_eq!(b.labels(0)[0], "< 2");
        assert_eq!(b.labels(1)[4], "≥ 8.0");
    }

    #[test]
    fn degenerate_ranges_still_classify() {
        let b = ClassBreaks::equal_interval(5.0, 5.0);
        assert!(b.breaks[0] > 5.0 && b.breaks[3] < 6.0);
        assert_eq!(b.class_of(5.0), 0);
        let b = ClassBreaks::equal_interval(f64::NAN, f64::NAN);
        assert_eq!(b, ClassBreaks::equal_interval(0.0, 1.0));
    }

    #[test]
    fn quantile_breaks_split_the_values_evenly() {
        let vals: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let b = ClassBreaks::quantiles(&vals);
        let mut counts = [0usize; 5];
        for v in &vals {
            counts[b.class_of(*v)] += 1;
        }
        for c in counts {
            assert!((19..=21).contains(&c), "{counts:?}");
        }
        // Editing a break out of order is repaired.
        let mut b = ClassBreaks::equal_interval(0.0, 10.0);
        b.breaks[0] = 9.0;
        b.normalize();
        assert_eq!(b.breaks, [4.0, 6.0, 8.0, 9.0]);
    }

    #[test]
    fn series_statistics() {
        let s = Series {
            times_s: vec![0.0, 60.0, 120.0, 180.0],
            values: vec![1.0, 3.0, 2.0, f64::NAN],
        };
        let st = stats(&s).unwrap();
        assert_eq!(st.n, 3);
        assert_eq!((st.min, st.min_at_s), (1.0, 0.0));
        assert_eq!((st.max, st.max_at_s), (3.0, 60.0));
        assert!((st.mean - 2.0).abs() < 1e-12);
        assert_eq!(st.sum, 6.0);
        // Trapezoids: (1+3)/2·60 + (3+2)/2·60 = 120 + 150; the NaN segment is skipped.
        assert!((st.integral - 270.0).abs() < 1e-9);
        assert!(stats(&Series {
            times_s: vec![],
            values: vec![]
        })
        .is_none());
    }

    #[test]
    fn peaks_are_local_maxima_largest_first() {
        let s = Series {
            times_s: (0..9).map(|i| i as f64 * 10.0).collect(),
            values: vec![0.0, 2.0, 1.0, 5.0, 5.0, 1.0, 3.0, 0.0, 4.0],
        };
        let p = top_peaks(&s, 10);
        assert_eq!(p, vec![(30.0, 5.0), (80.0, 4.0), (60.0, 3.0), (10.0, 2.0)]);
        assert_eq!(top_peaks(&s, 2).len(), 2);
        let flat = Series {
            times_s: vec![0.0, 1.0, 2.0],
            values: vec![1.0, 1.0, 1.0],
        };
        assert!(top_peaks(&flat, 3).is_empty());
    }
}
