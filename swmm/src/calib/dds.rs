// SPDX-License-Identifier: GPL-3.0-or-later

//! Dynamically dimensioned search — Tolson, B. A., and C. A. Shoemaker
//! (2007), "Dynamically dimensioned search algorithm for computationally
//! efficient watershed model calibration", *Water Resour. Res.* 43, W01413.
//!
//! The algorithm, as the paper gives it (their Section 2):
//!
//! 1. Start from `x_best = x0` with its objective `F_best`; `m` is the
//!    evaluation budget.
//! 2. At evaluation `i`, every decision variable is included in the
//!    neighbourhood with probability `P(i) = 1 − ln(i)/ln(m)`; if none is
//!    chosen, one is picked at random. The search thus narrows from global
//!    to local as the budget runs out.
//! 3. Each chosen variable `j` is perturbed by `r·(x_max_j − x_min_j)·N(0,1)`
//!    with `r = 0.2`; a value past a bound is reflected about that bound,
//!    and if the reflection crosses the other bound the variable is set to
//!    the bound it first crossed.
//! 4. The candidate is evaluated; it replaces the best when
//!    `F_new ≤ F_best` (greedy). There is no other state.
//!
//! [`Dds`] is written as a proposer so evaluations can run anywhere:
//! [`Dds::propose`] gives the next candidates, [`Dds::report`] takes their
//! objectives. With one candidate per round it is exactly the paper;
//! asking for `k` candidates per round evaluates `k` perturbations of the
//! current best concurrently and keeps the best of them, which is what the
//! parallel runner does. [`minimize`] wraps the loop for a plain function.

/// A small deterministic generator (SplitMix64 output feeding
/// xorshift64*), so a run with a seed repeats exactly. No dependency.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // SplitMix64 to spread a small seed over the state.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self {
            state: if z == 0 { 0x2545_F491_4F6C_DD1D } else { z },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal, by Box–Muller.
    pub fn normal(&mut self) -> f64 {
        let u1 = loop {
            let u = self.uniform();
            if u > 0.0 {
                break u;
            }
        };
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }

    /// Uniform integer in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.uniform() * n as f64) as usize % n
        }
    }
}

/// One variable's bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub lower: f64,
    pub upper: f64,
}

impl Bounds {
    pub fn new(lower: f64, upper: f64) -> Self {
        Self {
            lower: lower.min(upper),
            upper: upper.max(lower),
        }
    }

    pub fn range(self) -> f64 {
        self.upper - self.lower
    }

    pub fn clamp(self, x: f64) -> f64 {
        x.clamp(self.lower, self.upper)
    }
}

/// The DDS state: the best point so far and the evaluation counter.
#[derive(Clone, Debug)]
pub struct Dds {
    bounds: Vec<Bounds>,
    /// Perturbation size as a fraction of each range (the paper's `r`).
    pub r: f64,
    /// The evaluation budget `m`.
    pub max_evals: usize,
    rng: Rng,
    best_x: Vec<f64>,
    best_f: f64,
    /// Evaluations reported so far (the paper's `i`, once the start point
    /// is in).
    evals: usize,
    /// Candidates handed out and not yet reported.
    pending: usize,
    history: Vec<(usize, f64)>,
}

impl Dds {
    /// `x0` is evaluated first (as the paper's step 1); `max_evals` counts
    /// it. `r = 0.2` is the paper's recommendation.
    pub fn new(bounds: Vec<Bounds>, x0: Vec<f64>, max_evals: usize, seed: u64) -> Self {
        assert_eq!(bounds.len(), x0.len(), "one bound per variable");
        let x0: Vec<f64> = bounds.iter().zip(&x0).map(|(b, x)| b.clamp(*x)).collect();
        Self {
            bounds,
            r: 0.2,
            max_evals: max_evals.max(1),
            rng: Rng::new(seed),
            best_x: x0,
            best_f: f64::INFINITY,
            evals: 0,
            pending: 0,
            history: Vec::new(),
        }
    }

    pub fn dims(&self) -> usize {
        self.bounds.len()
    }

    pub fn best(&self) -> (&[f64], f64) {
        (&self.best_x, self.best_f)
    }

    pub fn evaluations(&self) -> usize {
        self.evals
    }

    /// `(evaluation number, best objective after it)`, one entry per
    /// reported evaluation — the convergence curve.
    pub fn history(&self) -> &[(usize, f64)] {
        &self.history
    }

    /// The budget is spent (counting candidates handed out).
    pub fn done(&self) -> bool {
        self.evals + self.pending >= self.max_evals
    }

    /// Inclusion probability at evaluation `i` (1-based).
    fn inclusion_probability(&self, i: usize) -> f64 {
        if self.max_evals <= 1 || i == 0 {
            return 1.0;
        }
        1.0 - (i as f64).ln() / (self.max_evals as f64).ln()
    }

    /// Reflect `x` into `b` the paper's way.
    fn reflect(b: Bounds, x: f64) -> f64 {
        if x < b.lower {
            let r = b.lower + (b.lower - x);
            if r > b.upper {
                b.lower
            } else {
                r
            }
        } else if x > b.upper {
            let r = b.upper - (x - b.upper);
            if r < b.lower {
                b.upper
            } else {
                r
            }
        } else {
            x
        }
    }

    /// One candidate around the current best for evaluation number `i`.
    fn perturb(&mut self, i: usize) -> Vec<f64> {
        let p = self.inclusion_probability(i);
        let n = self.dims();
        let mut chosen: Vec<bool> = (0..n).map(|_| self.rng.uniform() < p).collect();
        if !chosen.iter().any(|c| *c) && n > 0 {
            let j = self.rng.below(n);
            chosen[j] = true;
        }
        let mut x = self.best_x.clone();
        for j in 0..n {
            if !chosen[j] {
                continue;
            }
            let b = self.bounds[j];
            let step = self.r * b.range() * self.rng.normal();
            x[j] = Self::reflect(b, x[j] + step);
        }
        x
    }

    /// The next `k` candidates (fewer when the budget is nearly spent,
    /// none when it is). The very first call yields the start point.
    pub fn propose(&mut self, k: usize) -> Vec<Vec<f64>> {
        let mut out = Vec::new();
        for _ in 0..k.max(1) {
            if self.done() {
                break;
            }
            let i = self.evals + self.pending;
            let x = if i == 0 {
                self.best_x.clone()
            } else {
                self.perturb(i)
            };
            self.pending += 1;
            out.push(x);
        }
        out
    }

    /// Take the objective of a proposed candidate. A non-finite objective
    /// (a failed run) counts against the budget and never becomes best.
    pub fn report(&mut self, x: &[f64], f: f64) {
        self.pending = self.pending.saturating_sub(1);
        self.evals += 1;
        if f.is_finite() && f <= self.best_f {
            self.best_f = f;
            self.best_x = x.to_vec();
        }
        self.history.push((self.evals, self.best_f));
    }
}

/// Run DDS to completion on a plain function.
pub fn minimize(
    bounds: Vec<Bounds>,
    x0: Vec<f64>,
    max_evals: usize,
    seed: u64,
    mut f: impl FnMut(&[f64]) -> f64,
) -> Dds {
    let mut dds = Dds::new(bounds, x0, max_evals, seed);
    while !dds.done() {
        for x in dds.propose(1) {
            let v = f(&x);
            dds.report(&x, v);
        }
    }
    dds
}
