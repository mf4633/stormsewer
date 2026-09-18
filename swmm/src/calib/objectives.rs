// SPDX-License-Identifier: GPL-3.0-or-later

//! Goodness-of-fit statistics between an observed and a simulated series,
//! computed on the simulated values interpolated to the observed times.
//!
//! * NSE — Nash & Sutcliffe (1970), *J. Hydrol.* 10(3), 282–290:
//!   `1 − Σ(o−s)² / Σ(o−ō)²`.
//! * KGE — Gupta, Kling, Yilmaz & Martinez (2009), *J. Hydrol.* 377,
//!   80–91: `1 − √((r−1)² + (α−1)² + (β−1)²)` with `r` the Pearson
//!   correlation, `α = σ_s/σ_o`, `β = μ_s/μ_o`.
//! * PBIAS, RSR and the ratings — Moriasi et al. (2007), *Trans. ASABE*
//!   50(3), 885–900: `PBIAS = 100·Σ(o−s)/Σo` (positive = the model
//!   under-estimates), `RSR = RMSE/σ_o`, and the thresholds of their
//!   Table 4 for streamflow.
//! * Peak, volume and time-to-peak errors are the plain differences a
//!   reviewer asks for; volumes are trapezoidal integrals over time.

use serde::{Deserialize, Serialize};

use crate::out::Series;

/// Simulated values at the observed times; a time outside the simulated
/// span is `NaN` rather than extrapolated.
pub fn interpolate(sim: &Series, times: &[f64]) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(times.len());
    let n = sim.times_s.len();
    if n == 0 {
        return out;
    }
    let mut j = 0usize;
    for &t in times {
        if t < sim.times_s[0] || t > sim.times_s[n - 1] {
            out.push((t, f64::NAN));
            continue;
        }
        while j + 1 < n && sim.times_s[j + 1] < t {
            j += 1;
        }
        // Observed times are usually ascending, but need not be; rewind.
        while j > 0 && sim.times_s[j] > t {
            j -= 1;
        }
        let v = if j + 1 < n {
            let (t0, t1) = (sim.times_s[j], sim.times_s[j + 1]);
            let (v0, v1) = (sim.values[j], sim.values[j + 1]);
            if t1 > t0 {
                v0 + (v1 - v0) * (t - t0) / (t1 - t0)
            } else {
                v0
            }
        } else {
            sim.values[j]
        };
        out.push((t, v));
    }
    out
}

/// Observed and simulated values at common times.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Paired {
    pub times: Vec<f64>,
    pub obs: Vec<f64>,
    pub sim: Vec<f64>,
}

impl Paired {
    /// Pair `obs` (at `times`, seconds) with `sim`; observations outside
    /// the simulated span are left out.
    pub fn new(times: &[f64], obs: &[f64], sim: &Series) -> Self {
        let interp = interpolate(sim, times);
        let mut p = Self::default();
        for ((t, s), o) in interp.into_iter().zip(obs) {
            if s.is_finite() && o.is_finite() {
                p.times.push(t);
                p.obs.push(*o);
                p.sim.push(s);
            }
        }
        p
    }

    pub fn len(&self) -> usize {
        self.obs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.obs.is_empty()
    }
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Population standard deviation.
fn stdev(v: &[f64]) -> f64 {
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

/// Trapezoidal integral of `v` over `t`.
fn integral(t: &[f64], v: &[f64]) -> f64 {
    t.windows(2)
        .zip(v.windows(2))
        .map(|(tw, vw)| (tw[1] - tw[0]) * (vw[0] + vw[1]) / 2.0)
        .sum()
}

fn peak(t: &[f64], v: &[f64]) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    for (tt, vv) in t.iter().zip(v) {
        if best.is_none_or(|(_, b)| *vv > b) {
            best = Some((*tt, *vv));
        }
    }
    best
}

/// All the statistics of one pairing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub n: usize,
    pub nse: f64,
    pub kge: f64,
    /// KGE components: correlation, variability ratio, bias ratio.
    pub kge_r: f64,
    pub kge_alpha: f64,
    pub kge_beta: f64,
    pub rmse: f64,
    pub pbias: f64,
    pub rsr: f64,
    /// `100·(peak_sim − peak_obs)/peak_obs`.
    pub peak_error_pct: f64,
    /// `100·(vol_sim − vol_obs)/vol_obs`.
    pub volume_error_pct: f64,
    /// `t_peak_sim − t_peak_obs`, seconds.
    pub time_to_peak_error_s: f64,
    pub peak_obs: f64,
    pub peak_sim: f64,
}

impl Metrics {
    pub fn of(p: &Paired) -> Self {
        let n = p.len();
        if n == 0 {
            return Self {
                n,
                nse: f64::NAN,
                kge: f64::NAN,
                kge_r: f64::NAN,
                kge_alpha: f64::NAN,
                kge_beta: f64::NAN,
                rmse: f64::NAN,
                pbias: f64::NAN,
                rsr: f64::NAN,
                peak_error_pct: f64::NAN,
                volume_error_pct: f64::NAN,
                time_to_peak_error_s: f64::NAN,
                peak_obs: f64::NAN,
                peak_sim: f64::NAN,
            };
        }
        let (o, s) = (&p.obs, &p.sim);
        let mo = mean(o);
        let ms = mean(s);
        let ss_res: f64 = o.iter().zip(s).map(|(a, b)| (a - b).powi(2)).sum();
        let ss_tot: f64 = o.iter().map(|a| (a - mo).powi(2)).sum();
        let nse = if ss_tot > 0.0 {
            1.0 - ss_res / ss_tot
        } else {
            f64::NAN
        };
        let rmse = (ss_res / n as f64).sqrt();
        let so = stdev(o);
        let ssd = stdev(s);
        let cov: f64 =
            o.iter().zip(s).map(|(a, b)| (a - mo) * (b - ms)).sum::<f64>() / n as f64;
        let r = if so > 0.0 && ssd > 0.0 {
            cov / (so * ssd)
        } else {
            f64::NAN
        };
        let alpha = if so > 0.0 { ssd / so } else { f64::NAN };
        let beta = if mo != 0.0 { ms / mo } else { f64::NAN };
        let kge =
            1.0 - ((r - 1.0).powi(2) + (alpha - 1.0).powi(2) + (beta - 1.0).powi(2)).sqrt();
        let sum_o: f64 = o.iter().sum();
        let pbias = if sum_o != 0.0 {
            100.0 * o.iter().zip(s).map(|(a, b)| a - b).sum::<f64>() / sum_o
        } else {
            f64::NAN
        };
        let rsr = if so > 0.0 { rmse / so } else { f64::NAN };
        let (tpo, po) = peak(&p.times, o).unwrap_or((f64::NAN, f64::NAN));
        let (tps, ps) = peak(&p.times, s).unwrap_or((f64::NAN, f64::NAN));
        let peak_error_pct = if po != 0.0 {
            100.0 * (ps - po) / po
        } else {
            f64::NAN
        };
        let vo = integral(&p.times, o);
        let vs = integral(&p.times, s);
        let volume_error_pct = if vo != 0.0 {
            100.0 * (vs - vo) / vo
        } else {
            f64::NAN
        };
        Self {
            n,
            nse,
            kge,
            kge_r: r,
            kge_alpha: alpha,
            kge_beta: beta,
            rmse,
            pbias,
            rsr,
            peak_error_pct,
            volume_error_pct,
            time_to_peak_error_s: tps - tpo,
            peak_obs: po,
            peak_sim: ps,
        }
    }
}

/// The statistic an optimiser minimises (after [`Objective::minimized`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Objective {
    #[default]
    Nse,
    Kge,
    Rmse,
    Pbias,
    PeakError,
    VolumeError,
    TimeToPeak,
    Rsr,
}

impl Objective {
    pub const ALL: [Objective; 8] = [
        Objective::Nse,
        Objective::Kge,
        Objective::Rmse,
        Objective::Pbias,
        Objective::PeakError,
        Objective::VolumeError,
        Objective::TimeToPeak,
        Objective::Rsr,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Nse => "NSE",
            Self::Kge => "KGE",
            Self::Rmse => "RMSE",
            Self::Pbias => "PBIAS",
            Self::PeakError => "Peak error %",
            Self::VolumeError => "Volume error %",
            Self::TimeToPeak => "Time-to-peak error",
            Self::Rsr => "RSR",
        }
    }

    /// The statistic itself, as reported.
    pub fn value(self, m: &Metrics) -> f64 {
        match self {
            Self::Nse => m.nse,
            Self::Kge => m.kge,
            Self::Rmse => m.rmse,
            Self::Pbias => m.pbias,
            Self::PeakError => m.peak_error_pct,
            Self::VolumeError => m.volume_error_pct,
            Self::TimeToPeak => m.time_to_peak_error_s,
            Self::Rsr => m.rsr,
        }
    }

    /// The number the optimiser drives to zero: `1 − NSE`, `1 − KGE`,
    /// RMSE, |PBIAS|, |peak %|, |volume %|, |Δt_peak| (hours), RSR.
    pub fn minimized(self, m: &Metrics) -> f64 {
        match self {
            Self::Nse => 1.0 - m.nse,
            Self::Kge => 1.0 - m.kge,
            Self::Rmse => m.rmse,
            Self::Pbias => m.pbias.abs(),
            Self::PeakError => m.peak_error_pct.abs(),
            Self::VolumeError => m.volume_error_pct.abs(),
            Self::TimeToPeak => m.time_to_peak_error_s.abs() / 3600.0,
            Self::Rsr => m.rsr,
        }
    }
}

/// Moriasi et al. (2007) Table 4 performance rating.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rating {
    VeryGood,
    Good,
    Satisfactory,
    Unsatisfactory,
}

impl Rating {
    pub fn label(self) -> &'static str {
        match self {
            Self::VeryGood => "very good",
            Self::Good => "good",
            Self::Satisfactory => "satisfactory",
            Self::Unsatisfactory => "unsatisfactory",
        }
    }
}

/// The rating for a statistic, where Moriasi (2007) gives one (NSE, RSR,
/// PBIAS for streamflow); `None` otherwise or for a non-finite value.
pub fn rating(objective: Objective, value: f64) -> Option<Rating> {
    if !value.is_finite() {
        return None;
    }
    Some(match objective {
        Objective::Nse => {
            if value > 0.75 {
                Rating::VeryGood
            } else if value > 0.65 {
                Rating::Good
            } else if value > 0.50 {
                Rating::Satisfactory
            } else {
                Rating::Unsatisfactory
            }
        }
        Objective::Rsr => {
            if value <= 0.50 {
                Rating::VeryGood
            } else if value <= 0.60 {
                Rating::Good
            } else if value <= 0.70 {
                Rating::Satisfactory
            } else {
                Rating::Unsatisfactory
            }
        }
        Objective::Pbias => {
            let a = value.abs();
            if a < 10.0 {
                Rating::VeryGood
            } else if a < 15.0 {
                Rating::Good
            } else if a < 25.0 {
                Rating::Satisfactory
            } else {
                Rating::Unsatisfactory
            }
        }
        _ => return None,
    })
}
