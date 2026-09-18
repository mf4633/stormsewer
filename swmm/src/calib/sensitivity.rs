// SPDX-License-Identifier: GPL-3.0-or-later

//! Sensitivity screening before an optimisation: one-at-a-time swings
//! around the current values (a tornado table), and Morris's elementary
//! effects — Morris, M. D. (1991), "Factorial sampling plans for
//! preliminary computational experiments", *Technometrics* 33(2), 161–174,
//! with the `μ*` (mean absolute effect) of Campolongo, Cariboni & Saltelli
//! (2007) reported beside Morris's `μ` and `σ`.

use serde::{Deserialize, Serialize};

use super::dds::{Bounds, Rng};

/// One parameter's one-at-a-time swing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TornadoRow {
    pub name: String,
    pub x_low: f64,
    pub x_high: f64,
    pub f_low: f64,
    pub f_high: f64,
    /// `f_high − f_low`.
    pub swing: f64,
}

impl TornadoRow {
    pub fn abs_swing(&self) -> f64 {
        if self.swing.is_finite() {
            self.swing.abs()
        } else {
            0.0
        }
    }
}

/// The points OAT needs: for each parameter, `x0` with that parameter at
/// `x0_j·(1 ∓ pct/100)` — or `x0_j ∓ pct/100·range` when `x0_j` is zero
/// — clamped to its bounds. Returned as `(param index, is_high, point)`
/// so the caller can evaluate them in any order or in parallel.
pub fn oat_points(bounds: &[Bounds], x0: &[f64], pct: f64) -> Vec<(usize, bool, Vec<f64>)> {
    let mut out = Vec::new();
    for (j, b) in bounds.iter().enumerate() {
        let delta = if x0[j].abs() > 1e-12 {
            x0[j].abs() * pct / 100.0
        } else {
            b.range() * pct / 100.0
        };
        for (is_high, sign) in [(false, -1.0), (true, 1.0)] {
            let mut x = x0.to_vec();
            x[j] = b.clamp(x0[j] + sign * delta);
            out.push((j, is_high, x));
        }
    }
    out
}

/// Assemble the tornado from evaluated OAT points, largest |swing| first.
pub fn tornado(
    names: &[String],
    points: &[(usize, bool, Vec<f64>)],
    values: &[f64],
) -> Vec<TornadoRow> {
    let mut rows: Vec<TornadoRow> = names
        .iter()
        .map(|n| TornadoRow {
            name: n.clone(),
            x_low: f64::NAN,
            x_high: f64::NAN,
            f_low: f64::NAN,
            f_high: f64::NAN,
            swing: f64::NAN,
        })
        .collect();
    for ((j, is_high, x), f) in points.iter().zip(values) {
        let Some(row) = rows.get_mut(*j) else { continue };
        if *is_high {
            row.x_high = x[*j];
            row.f_high = *f;
        } else {
            row.x_low = x[*j];
            row.f_low = *f;
        }
    }
    for r in &mut rows {
        r.swing = r.f_high - r.f_low;
    }
    rows.sort_by(|a, b| b.abs_swing().total_cmp(&a.abs_swing()));
    rows
}

/// One parameter's Morris statistics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MorrisRow {
    pub name: String,
    /// Mean elementary effect.
    pub mu: f64,
    /// Mean absolute elementary effect (Campolongo et al. 2007).
    pub mu_star: f64,
    /// Standard deviation of the elementary effects: interaction and
    /// non-linearity.
    pub sigma: f64,
    pub n_effects: usize,
}

/// One Morris trajectory: its `k + 1` points (already mapped to the
/// bounds) and, per step, `(variable index, signed Δ)`.
pub type Trajectory = (Vec<Vec<f64>>, Vec<(usize, f64)>);

/// `r` Morris trajectories: `k + 1` points where each step changes one
/// variable by `±Δ` on a `p`-level grid over `[0, 1]`, mapped to the
/// bounds.
pub fn morris_trajectories(bounds: &[Bounds], r: usize, p: usize, seed: u64) -> Vec<Trajectory> {
    let k = bounds.len();
    let p = p.max(2);
    let delta = p as f64 / (2.0 * (p as f64 - 1.0));
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for _ in 0..r {
        // Base point on the grid, so that x ± Δ stays in [0, 1].
        let mut u: Vec<f64> = (0..k)
            .map(|_| {
                let levels = p / 2;
                rng.below(levels.max(1)) as f64 / (p as f64 - 1.0)
            })
            .collect();
        // Random order of variables, random direction each.
        let mut order: Vec<usize> = (0..k).collect();
        for i in (1..k).rev() {
            let j = rng.below(i + 1);
            order.swap(i, j);
        }
        let mut points = vec![u.clone()];
        let mut steps = Vec::new();
        for &j in &order {
            let up = u[j] + delta <= 1.0 + 1e-12;
            let down = u[j] - delta >= -1e-12;
            let sign = match (up, down) {
                (true, true) => {
                    if rng.uniform() < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                (true, false) => 1.0,
                _ => -1.0,
            };
            u[j] = (u[j] + sign * delta).clamp(0.0, 1.0);
            points.push(u.clone());
            steps.push((j, sign * delta));
        }
        let mapped: Vec<Vec<f64>> = points
            .iter()
            .map(|u| {
                u.iter()
                    .zip(bounds)
                    .map(|(ui, b)| b.lower + ui * b.range())
                    .collect()
            })
            .collect();
        out.push((mapped, steps));
    }
    out
}

/// Elementary effects from evaluated trajectories: `values[t]` holds the
/// objective at each point of trajectory `t`, in order. Effects are per
/// unit of the normalised `[0, 1]` range, so parameters compare directly.
pub fn morris(
    names: &[String],
    trajectories: &[Trajectory],
    values: &[Vec<f64>],
) -> Vec<MorrisRow> {
    let k = names.len();
    let mut effects: Vec<Vec<f64>> = vec![Vec::new(); k];
    for ((_, steps), vals) in trajectories.iter().zip(values) {
        for (s, (j, d)) in steps.iter().enumerate() {
            let (Some(a), Some(b)) = (vals.get(s), vals.get(s + 1)) else {
                continue;
            };
            if a.is_finite() && b.is_finite() && *d != 0.0 {
                effects[*j].push((b - a) / d);
            }
        }
    }
    let mut rows: Vec<MorrisRow> = names
        .iter()
        .zip(&effects)
        .map(|(n, e)| {
            let m = e.len();
            let (mu, mu_star, sigma) = if m == 0 {
                (f64::NAN, f64::NAN, f64::NAN)
            } else {
                let mu = e.iter().sum::<f64>() / m as f64;
                let mu_star = e.iter().map(|x| x.abs()).sum::<f64>() / m as f64;
                let var = e.iter().map(|x| (x - mu).powi(2)).sum::<f64>() / m as f64;
                (mu, mu_star, var.sqrt())
            };
            MorrisRow {
                name: n.clone(),
                mu,
                mu_star,
                sigma,
                n_effects: m,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        let fa = if a.mu_star.is_finite() { a.mu_star } else { -1.0 };
        let fb = if b.mu_star.is_finite() { b.mu_star } else { -1.0 };
        fb.total_cmp(&fa)
    });
    rows
}
