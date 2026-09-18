// SPDX-License-Identifier: GPL-3.0-or-later

//! The shallow-water solver: an explicit finite-volume scheme on the DEM
//! grid using the local-inertial (partial-inertia) form of the momentum
//! equation — Bates, Horritt & Fewtrell (2010), *J. Hydrol.* 387:33–45 —
//! with the q-centred θ-weighting of de Almeida, Bates, Freer & Souvignet
//! (2012), *Water Resour. Res.* 48, W05528. The equations, their
//! discretisation and the validation cases are in the manual's 2D methods
//! chapter (`docs/20b-2d-methods.md`); the comments here point at the
//! formulas by name.
//!
//! State lives on a staggered grid: depth `h` and bed `z` at cell centres,
//! unit discharge `qx` on the vertical faces and `qy` on the horizontal
//! faces. Rows run top to bottom as the raster stores them, so `qy > 0`
//! means flow towards increasing row index (south); the frames written to
//! the results file convert to world `+y`.
//!
//! One time step is four passes, each parallel over row bands with
//! `std::thread::scope` and each writing a disjoint slice:
//! 1. face fluxes (Bates eq. 11 with de Almeida's q-centred numerator),
//! 2. a per-cell outflow limiter so no cell can lose more than it holds,
//! 3. the continuity update with rain, infiltration, maxima and accounting,
//! 4. the limiter applied to the face fluxes themselves, so the next
//!    step's momentum starts from the flux that actually moved.

use std::thread;

use crate::gis::raster::Raster;
use crate::{Error, Result};

use super::grid::{InfiltrationInput, Inputs, LocatedSource, RainInput};
use super::interfaces::inlet_capture;
use super::io::Writer;
use super::{Boundary, Progress, RunSummary, Setup};

/// Gravitational acceleration and Manning's unit constant per system.
pub fn gravity(metric: bool) -> f64 {
    if metric {
        9.81
    } else {
        32.174
    }
}

pub fn manning_k(metric: bool) -> f64 {
    if metric {
        1.0
    } else {
        1.486
    }
}

/// Grids smaller than this run on one thread: spawning costs more than
/// the work.
const PARALLEL_MIN_CELLS: usize = 40_000;

/// Time step used while the whole grid is dry (nothing sets a CFL limit),
/// before the run's own caps.
const DRY_DT_S: f64 = 1.0;

/// Volumes in and out of the surface, in cubic DEM units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassBalance {
    /// Rain, sources, boundary inflow and exchange onto the surface.
    pub inflow: f64,
    /// Boundary outflow, sinks and exchange off the surface.
    pub outflow: f64,
    pub infiltrated: f64,
    /// Water on the grid at the start.
    pub initial: f64,
}

impl MassBalance {
    /// `(in - out - infiltrated - Δstored) / max(in, initial)` in percent;
    /// zero when nothing ever entered.
    pub fn error_pct(&self, stored: f64) -> f64 {
        let reference = self.inflow.max(self.initial);
        if reference <= 0.0 {
            return 0.0;
        }
        (self.inflow - self.outflow - self.infiltrated - (stored - self.initial)) / reference * 100.0
    }
}

/// Which momentum equation the solver integrates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Scheme {
    /// Bates et al. (2010) / de Almeida et al. (2012): gravity, pressure
    /// and friction on a staggered grid, no convective acceleration. Fast
    /// and robust for flood spreading, valid for low Froude numbers.
    #[default]
    LocalInertial,
    /// Full shallow-water equations: first-order Godunov with the HLL
    /// flux (Toro 2001) and the hydrostatic reconstruction of Audusse et
    /// al. (2004) for the bed slope, so it is well balanced and keeps
    /// depths non-negative. Use for dam breaks and supercritical flow.
    Hll,
}

/// One face's HLL flux: mass, x-momentum, y-momentum (in the face's own
/// axes: normal first), and the reconstructed depths on its two sides for
/// the bed-slope source term.
#[derive(Clone, Copy, Debug, Default)]
struct FaceFlux {
    h: f64,
    normal: f64,
    transverse: f64,
    left: f64,
    right: f64,
}

/// A cell that drains the surface at a stage (an outfall's 2D interface).
#[derive(Clone, Debug, PartialEq)]
pub struct Sink {
    pub col: usize,
    pub row: usize,
    /// Water-surface elevation the sink holds.
    pub stage: f64,
    pub perimeter: f64,
    pub area: f64,
    pub coeff: f64,
    /// Index into `Setup::nodes`, for the exchange record.
    pub node: usize,
}

/// The solver state for one grid.
#[derive(Clone, Debug)]
pub struct Simulator {
    pub ncols: usize,
    pub nrows: usize,
    pub dx: f64,
    pub g: f64,
    /// Manning's unit constant (1 metric, 1.486 US).
    pub kn: f64,
    pub dry_depth: f64,
    /// de Almeida's θ: 1 is Bates' original scheme, 0.7 the usual choice.
    pub theta: f64,
    pub courant: f64,
    pub max_dt: f64,
    pub boundary: Boundary,
    pub scheme: Scheme,
    pub time: f64,
    pub steps: usize,
    pub last_dt: f64,
    pub balance: MassBalance,
    pub rain: RainInput,
    pub infiltration: InfiltrationInput,
    pub sources: Vec<LocatedSource>,
    pub sinks: Vec<Sink>,
    /// Cumulative volume each sink has taken.
    pub sink_volume: Vec<f64>,
    threads: usize,
    z: Vec<f64>,
    h: Vec<f64>,
    n: Vec<f64>,
    qx: Vec<f64>,
    qy: Vec<f64>,
    qx2: Vec<f64>,
    qy2: Vec<f64>,
    /// Cell-centred momentum for the HLL scheme (`hv` positive towards
    /// increasing row, like `qy`).
    hu: Vec<f64>,
    hv: Vec<f64>,
    fx: Vec<FaceFlux>,
    fy: Vec<FaceFlux>,
    scale: Vec<f64>,
    wet_time: Vec<f64>,
    hmax: f64,
    /// Largest `|u| + √(g h)` on the grid, for the HLL time step.
    wave_max: f64,
    max_depth: Vec<f32>,
    max_speed: Vec<f32>,
    max_hazard: Vec<f32>,
    arrival: Vec<f32>,
}

/// What one band of the update pass reports back.
#[derive(Clone, Copy, Debug, Default)]
struct BandReport {
    rain: f64,
    infiltrated: f64,
    boundary_in: f64,
    boundary_out: f64,
    hmax: f64,
    wave_max: f64,
    wet: usize,
}

impl Simulator {
    /// A solver on a bed with uniform roughness, no rain or sources — the
    /// form the validation tests use.
    pub fn new(bed: &Raster, manning: f64, metric: bool, dry_depth: f64, courant: f64) -> Self {
        let manning = Raster::filled(bed.ncols, bed.nrows, bed.x0, bed.y0, bed.cell, manning);
        Self::with_grids(bed, &manning, metric, dry_depth, courant, 0)
    }

    pub fn with_grids(
        bed: &Raster,
        manning: &Raster,
        metric: bool,
        dry_depth: f64,
        courant: f64,
        threads: usize,
    ) -> Self {
        let n = bed.ncols * bed.nrows;
        let threads = if threads == 0 {
            thread::available_parallelism().map(|p| p.get()).unwrap_or(1)
        } else {
            threads
        };
        Self {
            ncols: bed.ncols,
            nrows: bed.nrows,
            dx: bed.cell,
            g: gravity(metric),
            kn: manning_k(metric),
            dry_depth,
            theta: 0.7,
            courant,
            max_dt: f64::INFINITY,
            boundary: Boundary::Closed,
            scheme: Scheme::LocalInertial,
            time: 0.0,
            steps: 0,
            last_dt: 0.0,
            balance: MassBalance::default(),
            rain: RainInput::None,
            infiltration: InfiltrationInput::None,
            sources: Vec::new(),
            sinks: Vec::new(),
            sink_volume: Vec::new(),
            threads,
            z: bed.data.clone(),
            h: vec![0.0; n],
            n: manning.data.clone(),
            qx: vec![0.0; (bed.ncols + 1) * bed.nrows],
            qy: vec![0.0; (bed.nrows + 1) * bed.ncols],
            qx2: vec![0.0; (bed.ncols + 1) * bed.nrows],
            qy2: vec![0.0; (bed.nrows + 1) * bed.ncols],
            hu: Vec::new(),
            hv: Vec::new(),
            fx: Vec::new(),
            fy: Vec::new(),
            scale: vec![1.0; n],
            wet_time: vec![0.0; n],
            hmax: 0.0,
            wave_max: 0.0,
            max_depth: vec![0.0; n],
            max_speed: vec![0.0; n],
            max_hazard: vec![0.0; n],
            arrival: vec![f32::NAN; n],
        }
    }

    /// The solver a resolved setup describes, with its rain, infiltration,
    /// sources and outfall sinks attached.
    pub fn from_setup(setup: &Setup) -> Self {
        let cfg = &setup.config;
        let mut sim = Self::with_grids(
            &setup.dem,
            &setup.manning,
            setup.metric,
            cfg.dry_depth,
            cfg.courant,
            cfg.threads,
        );
        sim.max_dt = cfg.max_dt_s.unwrap_or(f64::INFINITY);
        sim.boundary = cfg.boundary;
        sim.attach_inputs(&setup.inputs);
        for o in &setup.inputs.outfalls {
            let node = &setup.nodes[o.node];
            let (perimeter, area) = super::interfaces::opening(&node.interface, setup.metric);
            if perimeter <= 0.0 {
                continue;
            }
            sim.add_sink(Sink {
                col: node.col,
                row: node.row,
                stage: o.stage,
                perimeter,
                area,
                coeff: super::interfaces::coeff(&node.interface),
                node: o.node,
            });
        }
        sim
    }

    pub fn attach_inputs(&mut self, inputs: &Inputs) {
        self.rain = inputs.rain.clone();
        self.infiltration = inputs.infiltration;
        self.sources = inputs.sources.clone();
    }

    pub fn add_sink(&mut self, sink: Sink) {
        self.sinks.push(sink);
        self.sink_volume.push(0.0);
    }

    pub fn cells(&self) -> usize {
        self.ncols * self.nrows
    }

    fn idx(&self, col: usize, row: usize) -> usize {
        row * self.ncols + col
    }

    pub fn depth(&self, col: usize, row: usize) -> f64 {
        self.h[self.idx(col, row)]
    }

    pub fn bed(&self, col: usize, row: usize) -> f64 {
        self.z[self.idx(col, row)]
    }

    /// Water-surface elevation of a cell.
    pub fn surface(&self, col: usize, row: usize) -> f64 {
        let i = self.idx(col, row);
        self.z[i] + self.h[i]
    }

    pub fn depths(&self) -> &[f64] {
        &self.h
    }

    /// Set the depth everywhere (initial condition); recomputes the
    /// initial storage for the balance.
    pub fn set_depths(&mut self, depths: &[f64]) {
        assert_eq!(depths.len(), self.h.len());
        for (i, d) in depths.iter().enumerate() {
            self.h[i] = if self.z[i].is_nan() { 0.0 } else { d.max(0.0) };
        }
        self.hu.iter_mut().for_each(|v| *v = 0.0);
        self.hv.iter_mut().for_each(|v| *v = 0.0);
        self.qx.iter_mut().for_each(|v| *v = 0.0);
        self.qy.iter_mut().for_each(|v| *v = 0.0);
        self.balance.initial = self.stored();
        self.hmax = self.h.iter().copied().fold(0.0, f64::max);
        self.wave_max = 0.0;
    }

    /// Fill to a water-surface elevation (still water).
    pub fn fill_to(&mut self, wse: f64) {
        let depths: Vec<f64> = self.z.iter().map(|z| (wse - z).max(0.0)).collect();
        self.set_depths(&depths);
    }

    pub fn set_depth(&mut self, col: usize, row: usize, depth: f64) {
        let i = self.idx(col, row);
        if !self.z[i].is_nan() {
            self.h[i] = depth.max(0.0);
        }
        self.balance.initial = self.stored();
        self.hmax = self.hmax.max(depth);
    }

    /// Volume on the grid now.
    pub fn stored(&self) -> f64 {
        let a = self.dx * self.dx;
        self.h.iter().filter(|v| !v.is_nan()).sum::<f64>() * a
    }

    pub fn wet_cells(&self) -> usize {
        self.h.iter().filter(|&&v| v > self.dry_depth).count()
    }

    pub fn mass_error_pct(&self) -> f64 {
        self.balance.error_pct(self.stored())
    }

    /// Cell velocity components in world axes (x east, y north).
    pub fn velocity(&self, col: usize, row: usize) -> (f64, f64) {
        let i = self.idx(col, row);
        let h = self.h[i];
        if h <= self.dry_depth {
            return (0.0, 0.0);
        }
        if self.scheme == Scheme::Hll && !self.hu.is_empty() {
            return (self.hu[i] / h, -self.hv[i] / h);
        }
        let w = self.ncols + 1;
        let qw = self.qx[row * w + col];
        let qe = self.qx[row * w + col + 1];
        let qn = self.qy[row * self.ncols + col];
        let qs = self.qy[(row + 1) * self.ncols + col];
        (0.5 * (qw + qe) / h, -0.5 * (qn + qs) / h)
    }

    /// Unit discharge on the face between `(col-1,row)` and `(col,row)`.
    pub fn qx_face(&self, col: usize, row: usize) -> f64 {
        self.qx[row * (self.ncols + 1) + col]
    }

    /// Add (or, negative, take) a volume at a cell; a withdrawal is capped
    /// at what the cell holds. Returns the volume actually moved (signed)
    /// and books it as exchange inflow/outflow.
    pub fn add_volume(&mut self, col: usize, row: usize, volume: f64) -> f64 {
        let i = self.idx(col, row);
        if self.z[i].is_nan() || volume == 0.0 {
            return 0.0;
        }
        let a = self.dx * self.dx;
        let actual = if volume < 0.0 {
            -((-volume).min(self.h[i] * a))
        } else {
            volume
        };
        self.h[i] += actual / a;
        if actual > 0.0 {
            self.balance.inflow += actual;
            self.hmax = self.hmax.max(self.h[i]);
        } else {
            self.balance.outflow -= actual;
        }
        actual
    }

    /// Volume a cell holds.
    pub fn cell_volume(&self, col: usize, row: usize) -> f64 {
        self.h[self.idx(col, row)] * self.dx * self.dx
    }

    /// The CFL time step for the current state: `α Δx / √(g h_max)`.
    pub fn cfl_dt(&self) -> f64 {
        let dt = if self.hmax > self.dry_depth {
            match self.scheme {
                Scheme::LocalInertial => self.courant * self.dx / (self.g * self.hmax).sqrt(),
                // Full dynamics: the fastest characteristic is |u| + √(g h),
                // and the unsplit 2D update is stable for ν_x + ν_y ≤ 1, so
                // the 1D Courant number is halved.
                Scheme::Hll => {
                    let w = self.wave_max.max((self.g * self.hmax).sqrt());
                    0.5 * self.courant * self.dx / w
                }
            }
        } else {
            DRY_DT_S
        };
        dt.min(self.max_dt)
    }

    /// Maxima so far: depth, speed, depth × speed, arrival time (s, NaN
    /// where never wet).
    pub fn maxima(&self) -> (&[f32], &[f32], &[f32], &[f32]) {
        (&self.max_depth, &self.max_speed, &self.max_hazard, &self.arrival)
    }

    /// Depth and world-axis velocity per cell as `f32` frames; NaN depth
    /// on no-data cells.
    pub fn frame(&self) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let n = self.cells();
        let mut d = Vec::with_capacity(n);
        let mut vx = Vec::with_capacity(n);
        let mut vy = Vec::with_capacity(n);
        for row in 0..self.nrows {
            for col in 0..self.ncols {
                let i = row * self.ncols + col;
                if self.z[i].is_nan() {
                    d.push(f32::NAN);
                    vx.push(0.0);
                    vy.push(0.0);
                } else {
                    let (u, v) = self.velocity(col, row);
                    d.push(self.h[i] as f32);
                    vx.push(u as f32);
                    vy.push(v as f32);
                }
            }
        }
        (d, vx, vy)
    }

    fn band_threads(&self) -> usize {
        if self.cells() < PARALLEL_MIN_CELLS {
            1
        } else {
            self.threads.max(1).min(self.nrows)
        }
    }

    /// Advance one step of at most `dt_cap` seconds (the CFL step when
    /// smaller). Returns the step taken.
    pub fn step(&mut self, dt_cap: f64) -> f64 {
        let dt = self.cfl_dt().min(dt_cap).max(0.0);
        if dt <= 0.0 {
            return 0.0;
        }
        let reports = match self.scheme {
            Scheme::LocalInertial => self.step_inertial(dt),
            Scheme::Hll => self.step_hll(dt),
        };
        self.finish_step(reports, dt);
        dt
    }

    /// Sources, sinks, the balance and the clock, after either scheme's
    /// grid passes.
    fn finish_step(&mut self, reports: Vec<BandReport>, dt: f64) {
        let ncols = self.ncols;
        let mut hmax: f64 = 0.0;
        let mut wave_max: f64 = 0.0;
        for r in &reports {
            self.balance.inflow += r.rain + r.boundary_in;
            self.balance.outflow += r.boundary_out;
            self.balance.infiltrated += r.infiltrated;
            hmax = hmax.max(r.hmax);
            wave_max = wave_max.max(r.wave_max);
        }
        let t0 = self.time;
        let t1 = t0 + dt;
        let a = self.dx * self.dx;
        for si in 0..self.sources.len() {
            let vol = self.sources[si].series.integral_linear(t0, t1);
            if vol > 0.0 {
                let (c, r) = (self.sources[si].col, self.sources[si].row);
                let i = r * ncols + c;
                self.h[i] += vol / a;
                self.balance.inflow += vol;
                hmax = hmax.max(self.h[i]);
            }
        }
        for si in 0..self.sinks.len() {
            let s = &self.sinks[si];
            let i = s.row * ncols + s.col;
            let h = self.h[i];
            if h <= self.dry_depth {
                continue;
            }
            let q = inlet_capture(self.g, s.coeff, s.perimeter, s.area, h, s.stage, self.z[i]);
            let vol = (q * dt).min(h * a);
            if vol > 0.0 {
                self.h[i] -= vol / a;
                self.balance.outflow += vol;
                self.sink_volume[si] += vol;
            }
        }
        self.hmax = hmax;
        self.wave_max = wave_max;
        self.time = t1;
        self.steps += 1;
        self.last_dt = dt;
    }

    /// The local-inertial step: the four passes described in the module
    /// doc. Returns the per-band reports.
    fn step_inertial(&mut self, dt: f64) -> Vec<BandReport> {
        let ncols = self.ncols;
        let nrows = self.nrows;
        let threads = self.band_threads();
        let band_rows = nrows.div_ceil(threads).max(1);

        // ---- pass 1: face fluxes -------------------------------------
        {
            let p = FluxParams {
                ncols,
                nrows,
                dx: self.dx,
                g: self.g,
                kn: self.kn,
                dry: self.dry_depth,
                theta: self.theta,
                dt,
                boundary: self.boundary,
            };
            let (z, h, n, qx, qy) = (&self.z, &self.h, &self.n, &self.qx, &self.qy);
            let (qx2, qy2) = (&mut self.qx2, &mut self.qy2);
            let w = ncols + 1;
            thread::scope(|s| {
                for (b, chunk) in qx2.chunks_mut(band_rows * w).enumerate() {
                    let r0 = b * band_rows;
                    s.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let row = r0 + k / w;
                            let col = k % w;
                            *out = flux_x(&p, z, h, n, qx, row, col);
                        }
                    });
                }
                for (b, chunk) in qy2.chunks_mut(band_rows * ncols).enumerate() {
                    let f0 = b * band_rows;
                    s.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let frow = f0 + k / ncols;
                            let col = k % ncols;
                            *out = flux_y(&p, z, h, n, qy, frow, col);
                        }
                    });
                }
            });
        }

        // ---- pass 2: outflow limiter per cell -------------------------
        {
            let (h, qx2, qy2) = (&self.h, &self.qx2, &self.qy2);
            let scale = &mut self.scale;
            let w = ncols + 1;
            let f = dt / self.dx;
            thread::scope(|s| {
                for (b, chunk) in scale.chunks_mut(band_rows * ncols).enumerate() {
                    let r0 = b * band_rows;
                    s.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let row = r0 + k / ncols;
                            let col = k % ncols;
                            let i = row * ncols + col;
                            let qw = qx2[row * w + col];
                            let qe = qx2[row * w + col + 1];
                            let qn = qy2[row * ncols + col];
                            let qs = qy2[(row + 1) * ncols + col];
                            let outflow = f * (qe.max(0.0) - qw.min(0.0) + qs.max(0.0) - qn.min(0.0));
                            *out = if outflow > h[i] && outflow > 0.0 {
                                (h[i] / outflow).max(0.0)
                            } else {
                                1.0
                            };
                        }
                    });
                }
            });
        }

        // ---- pass 3: continuity, rain, infiltration, maxima ------------
        let reports: Vec<BandReport> = {
            let (z, n, qx2, qy2, scale) = (&self.z, &self.n, &self.qx2, &self.qy2, &self.scale);
            let _ = n;
            let rain_rate = self.rain.rate_at(self.time);
            let infiltration = self.infiltration;
            let dry = self.dry_depth;
            let dx = self.dx;
            let time_end = (self.time + dt) as f32;
            let w = ncols + 1;
            let f = dt / dx;
            let cells_per_band = band_rows * ncols;
            let mut reports = vec![BandReport::default(); nrows.div_ceil(band_rows)];
            thread::scope(|s| {
                let bands = self
                    .h
                    .chunks_mut(cells_per_band)
                    .zip(self.wet_time.chunks_mut(cells_per_band))
                    .zip(self.max_depth.chunks_mut(cells_per_band))
                    .zip(self.max_speed.chunks_mut(cells_per_band))
                    .zip(self.max_hazard.chunks_mut(cells_per_band))
                    .zip(self.arrival.chunks_mut(cells_per_band))
                    .zip(reports.iter_mut())
                    .enumerate();
                for (b, ((((((h, wet), maxd), maxs), maxh), arr), report)) in bands {
                    let r0 = b * band_rows;
                    s.spawn(move || {
                        let mut rep = BandReport::default();
                        for k in 0..h.len() {
                            let row = r0 + k / ncols;
                            let col = k % ncols;
                            let i = row * ncols + col;
                            if z[i].is_nan() {
                                continue;
                            }
                            // Effective (limited) face fluxes: each face is
                            // scaled by its upwind cell's limiter.
                            let qw = {
                                let q = qx2[row * w + col];
                                q * if q > 0.0 {
                                    if col > 0 { scale[i - 1] } else { 1.0 }
                                } else {
                                    scale[i]
                                }
                            };
                            let qe = {
                                let q = qx2[row * w + col + 1];
                                q * if q > 0.0 {
                                    scale[i]
                                } else if col + 1 < ncols {
                                    scale[i + 1]
                                } else {
                                    1.0
                                }
                            };
                            let qn = {
                                let q = qy2[row * ncols + col];
                                q * if q > 0.0 {
                                    if row > 0 { scale[i - ncols] } else { 1.0 }
                                } else {
                                    scale[i]
                                }
                            };
                            let qs = {
                                let q = qy2[(row + 1) * ncols + col];
                                q * if q > 0.0 {
                                    scale[i]
                                } else if row + 1 < nrows {
                                    scale[i + ncols]
                                } else {
                                    1.0
                                }
                            };
                            let mut hn = h[k] + f * (qw - qe + qn - qs);
                            // Boundary accounting: edge faces are the grid's
                            // inflow and outflow.
                            let mut book = |q_in: f64| {
                                let v = q_in * dt * dx;
                                if v > 0.0 {
                                    rep.boundary_in += v;
                                } else {
                                    rep.boundary_out -= v;
                                }
                            };
                            if col == 0 {
                                book(qw);
                            }
                            if col + 1 == ncols {
                                book(-qe);
                            }
                            if row == 0 {
                                book(qn);
                            }
                            if row + 1 == nrows {
                                book(-qs);
                            }
                            if rain_rate > 0.0 {
                                hn += rain_rate * dt;
                                rep.rain += rain_rate * dt * dx * dx;
                            }
                            let rate = match infiltration {
                                InfiltrationInput::None => 0.0,
                                InfiltrationInput::Constant(r) => r,
                                InfiltrationInput::Horton { f0, fc, k: decay } => {
                                    fc + (f0 - fc) * (-decay * wet[k]).exp()
                                }
                            };
                            if rate > 0.0 && hn > 0.0 {
                                let loss = (rate * dt).min(hn);
                                hn -= loss;
                                rep.infiltrated += loss * dx * dx;
                            }
                            if hn < 0.0 {
                                hn = 0.0;
                            }
                            h[k] = hn;
                            if hn > dry {
                                wet[k] += dt;
                                rep.wet += 1;
                                let u = 0.5 * (qw + qe) / hn;
                                let v = 0.5 * (qn + qs) / hn;
                                let speed = (u * u + v * v).sqrt() as f32;
                                if speed > maxs[k] {
                                    maxs[k] = speed;
                                }
                                let hz = (hn as f32) * speed;
                                if hz > maxh[k] {
                                    maxh[k] = hz;
                                }
                                if arr[k].is_nan() {
                                    arr[k] = time_end;
                                }
                            }
                            if hn as f32 > maxd[k] {
                                maxd[k] = hn as f32;
                            }
                            if hn > rep.hmax {
                                rep.hmax = hn;
                            }
                        }
                        *report = rep;
                    });
                }
            });
            reports
        };

        // ---- pass 4: the limiter applied to the fluxes themselves ------
        {
            let scale = &self.scale;
            let w = ncols + 1;
            thread::scope(|s| {
                for (b, chunk) in self.qx2.chunks_mut(band_rows * w).enumerate() {
                    let r0 = b * band_rows;
                    s.spawn(move || {
                        for (k, q) in chunk.iter_mut().enumerate() {
                            let row = r0 + k / w;
                            let col = k % w;
                            let up = if *q > 0.0 {
                                if col > 0 { Some(row * ncols + col - 1) } else { None }
                            } else if col < ncols {
                                Some(row * ncols + col)
                            } else {
                                None
                            };
                            if let Some(i) = up {
                                *q *= scale[i];
                            }
                        }
                    });
                }
                for (b, chunk) in self.qy2.chunks_mut(band_rows * ncols).enumerate() {
                    let f0 = b * band_rows;
                    s.spawn(move || {
                        for (k, q) in chunk.iter_mut().enumerate() {
                            let frow = f0 + k / ncols;
                            let col = k % ncols;
                            let up = if *q > 0.0 {
                                if frow > 0 { Some((frow - 1) * ncols + col) } else { None }
                            } else if frow < nrows {
                                Some(frow * ncols + col)
                            } else {
                                None
                            };
                            if let Some(i) = up {
                                *q *= scale[i];
                            }
                        }
                    });
                }
            });
        }
        std::mem::swap(&mut self.qx, &mut self.qx2);
        std::mem::swap(&mut self.qy, &mut self.qy2);
        reports
    }

    /// The full-dynamic step: HLL fluxes on every face with hydrostatic
    /// reconstruction, the same outflow limiter, then the conserved update
    /// with the bed-slope source, semi-implicit friction, rain and
    /// infiltration.
    fn step_hll(&mut self, dt: f64) -> Vec<BandReport> {
        let ncols = self.ncols;
        let nrows = self.nrows;
        let n_cells = ncols * nrows;
        if self.hu.len() != n_cells {
            self.hu = vec![0.0; n_cells];
            self.hv = vec![0.0; n_cells];
            self.fx = vec![FaceFlux::default(); (ncols + 1) * nrows];
            self.fy = vec![FaceFlux::default(); (nrows + 1) * ncols];
        }
        let threads = self.band_threads();
        let band_rows = nrows.div_ceil(threads).max(1);
        let g = self.g;
        let dry = self.dry_depth;
        let boundary = self.boundary;

        // ---- pass 1: HLL fluxes on every face ---------------------------
        {
            let (z, h, hu, hv) = (&self.z, &self.h, &self.hu, &self.hv);
            let w = ncols + 1;
            let state = |i: usize| -> (f64, f64, f64, f64) {
                let d = h[i];
                if d > dry {
                    (z[i], d, hu[i] / d, hv[i] / d)
                } else {
                    (z[i], d.max(0.0), 0.0, 0.0)
                }
            };
            thread::scope(|s| {
                for (b, chunk) in self.fx.chunks_mut(band_rows * w).enumerate() {
                    let r0 = b * band_rows;
                    s.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let row = r0 + k / w;
                            let col = k % w;
                            let cell = |c: usize| row * ncols + c;
                            *out = if col == 0 || col == ncols {
                                let ci = if col == 0 { cell(0) } else { cell(ncols - 1) };
                                let (zc, hc, uc, vc) = state(ci);
                                edge_flux_hll(g, boundary, zc, hc, uc, vc, col == 0)
                            } else {
                                let (zl, hl, ul, vl) = state(cell(col - 1));
                                let (zr, hr, ur, vr) = state(cell(col));
                                face_flux_hll(g, zl, hl, ul, vl, zr, hr, ur, vr)
                            };
                        }
                    });
                }
                for (b, chunk) in self.fy.chunks_mut(band_rows * ncols).enumerate() {
                    let f0 = b * band_rows;
                    s.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let frow = f0 + k / ncols;
                            let col = k % ncols;
                            let cell = |r: usize| r * ncols + col;
                            // Along y the normal velocity is v (row-wise)
                            // and the transverse one is u.
                            *out = if frow == 0 || frow == nrows {
                                let ci = if frow == 0 { cell(0) } else { cell(nrows - 1) };
                                let (zc, hc, uc, vc) = state(ci);
                                edge_flux_hll(g, boundary, zc, hc, vc, uc, frow == 0)
                            } else {
                                let (zu, hu_, uu, vu) = state(cell(frow - 1));
                                let (zd, hd, ud, vd) = state(cell(frow));
                                face_flux_hll(g, zu, hu_, vu, uu, zd, hd, vd, ud)
                            };
                        }
                    });
                }
            });
        }

        // ---- pass 2: outflow limiter ------------------------------------
        {
            let (h, fx, fy) = (&self.h, &self.fx, &self.fy);
            let w = ncols + 1;
            let f = dt / self.dx;
            thread::scope(|s| {
                for (b, chunk) in self.scale.chunks_mut(band_rows * ncols).enumerate() {
                    let r0 = b * band_rows;
                    s.spawn(move || {
                        for (k, out) in chunk.iter_mut().enumerate() {
                            let row = r0 + k / ncols;
                            let col = k % ncols;
                            let i = row * ncols + col;
                            let qw = fx[row * w + col].h;
                            let qe = fx[row * w + col + 1].h;
                            let qn = fy[row * ncols + col].h;
                            let qs = fy[(row + 1) * ncols + col].h;
                            let outflow = f * (qe.max(0.0) - qw.min(0.0) + qs.max(0.0) - qn.min(0.0));
                            *out = if outflow > h[i] && outflow > 0.0 {
                                (h[i] / outflow).max(0.0)
                            } else {
                                1.0
                            };
                        }
                    });
                }
            });
        }

        // ---- pass 3: conserved update -----------------------------------
        let (z, n, fx, fy, scale) = (&self.z, &self.n, &self.fx, &self.fy, &self.scale);
        let rain_rate = self.rain.rate_at(self.time);
        let infiltration = self.infiltration;
        let dx = self.dx;
        let kn = self.kn;
        let time_end = (self.time + dt) as f32;
        let w = ncols + 1;
        let f = dt / dx;
        let cells_per_band = band_rows * ncols;
        let mut reports = vec![BandReport::default(); nrows.div_ceil(band_rows)];
        thread::scope(|s| {
            let bands = self
                .h
                .chunks_mut(cells_per_band)
                .zip(self.hu.chunks_mut(cells_per_band))
                .zip(self.hv.chunks_mut(cells_per_band))
                .zip(self.wet_time.chunks_mut(cells_per_band))
                .zip(self.max_depth.chunks_mut(cells_per_band))
                .zip(self.max_speed.chunks_mut(cells_per_band))
                .zip(self.max_hazard.chunks_mut(cells_per_band))
                .zip(self.arrival.chunks_mut(cells_per_band))
                .zip(reports.iter_mut())
                .enumerate();
            for (b, ((((((((h, hu), hv), wet), maxd), maxs), maxh), arr), report)) in bands {
                let r0 = b * band_rows;
                s.spawn(move || {
                    let mut rep = BandReport::default();
                    for k in 0..h.len() {
                        let row = r0 + k / ncols;
                        let col = k % ncols;
                        let i = row * ncols + col;
                        if z[i].is_nan() {
                            continue;
                        }
                        // Each face scaled by the limiter of its upwind
                        // cell: this one when the water leaves it, else
                        // the neighbour's (1 across an edge).
                        let sc = |neighbour: Option<usize>, leaving: bool| -> f64 {
                            if leaving {
                                scale[i]
                            } else {
                                neighbour.map(|j| scale[j]).unwrap_or(1.0)
                            }
                        };
                        let fw = fx[row * w + col];
                        let sw = sc(if col > 0 { Some(i - 1) } else { None }, fw.h < 0.0);
                        let fe = fx[row * w + col + 1];
                        let se = sc(if col + 1 < ncols { Some(i + 1) } else { None }, fe.h > 0.0);
                        let fn_ = fy[row * ncols + col];
                        let sn = sc(if row > 0 { Some(i - ncols) } else { None }, fn_.h < 0.0);
                        let fs = fy[(row + 1) * ncols + col];
                        let ss = sc(if row + 1 < nrows { Some(i + ncols) } else { None }, fs.h > 0.0);
                        let (qw, qe, qn, qs) = (fw.h * sw, fe.h * se, fn_.h * sn, fs.h * ss);
                        let mut hn = h[k] + f * (qw - qe + qn - qs);
                        // Momentum: flux differences plus the hydrostatic
                        // reconstruction's bed-slope term (Audusse 2004).
                        let mut hun = hu[k]
                            + f * (fw.normal * sw - fe.normal * se + fn_.transverse * sn - fs.transverse * ss)
                            + f * 0.5 * g * (fe.left * fe.left - fw.right * fw.right);
                        let mut hvn = hv[k]
                            + f * (fw.transverse * sw - fe.transverse * se + fn_.normal * sn - fs.normal * ss)
                            + f * 0.5 * g * (fs.left * fs.left - fn_.right * fn_.right);
                        let mut book = |q_in: f64| {
                            let v = q_in * dt * dx;
                            if v > 0.0 {
                                rep.boundary_in += v;
                            } else {
                                rep.boundary_out -= v;
                            }
                        };
                        if col == 0 {
                            book(qw);
                        }
                        if col + 1 == ncols {
                            book(-qe);
                        }
                        if row == 0 {
                            book(qn);
                        }
                        if row + 1 == nrows {
                            book(-qs);
                        }
                        if rain_rate > 0.0 {
                            hn += rain_rate * dt;
                            rep.rain += rain_rate * dt * dx * dx;
                        }
                        let rate = match infiltration {
                            InfiltrationInput::None => 0.0,
                            InfiltrationInput::Constant(r) => r,
                            InfiltrationInput::Horton { f0, fc, k: decay } => {
                                fc + (f0 - fc) * (-decay * wet[k]).exp()
                            }
                        };
                        if rate > 0.0 && hn > 0.0 {
                            let loss = (rate * dt).min(hn);
                            hn -= loss;
                            rep.infiltrated += loss * dx * dx;
                        }
                        if hn < 0.0 {
                            hn = 0.0;
                        }
                        if hn > dry {
                            // Semi-implicit Manning friction on the momentum.
                            let u = hun / hn;
                            let v = hvn / hn;
                            let speed = (u * u + v * v).sqrt();
                            let nn = n[i];
                            let fric = 1.0 + dt * g * nn * nn * speed / (kn * kn * hn.powf(4.0 / 3.0));
                            hun /= fric;
                            hvn /= fric;
                            let u = hun / hn;
                            let v = hvn / hn;
                            let speed = (u * u + v * v).sqrt();
                            wet[k] += dt;
                            rep.wet += 1;
                            let c = (g * hn).sqrt();
                            rep.wave_max = rep.wave_max.max(u.abs() + c).max(v.abs() + c);
                            let sp = speed as f32;
                            if sp > maxs[k] {
                                maxs[k] = sp;
                            }
                            let hz = (hn as f32) * sp;
                            if hz > maxh[k] {
                                maxh[k] = hz;
                            }
                            if arr[k].is_nan() {
                                arr[k] = time_end;
                            }
                        } else {
                            hun = 0.0;
                            hvn = 0.0;
                        }
                        h[k] = hn;
                        hu[k] = hun;
                        hv[k] = hvn;
                        if hn as f32 > maxd[k] {
                            maxd[k] = hn as f32;
                        }
                        if hn > rep.hmax {
                            rep.hmax = hn;
                        }
                    }
                    *report = rep;
                });
            }
        });
        reports
    }

    /// Step until `t` (exactly).
    pub fn advance_to(&mut self, t: f64) {
        let mut guard = 0usize;
        while self.time < t - 1e-9 {
            let dt = self.step(t - self.time);
            if dt <= 0.0 {
                break;
            }
            guard += 1;
            if guard > 50_000_000 {
                break;
            }
        }
        if (self.time - t).abs() < 1e-6 {
            self.time = t;
        }
    }
}

#[derive(Clone, Copy)]
struct FluxParams {
    ncols: usize,
    nrows: usize,
    dx: f64,
    g: f64,
    kn: f64,
    dry: f64,
    theta: f64,
    dt: f64,
    boundary: Boundary,
}

/// Bates et al. (2010) eq. 11 with de Almeida et al. (2012) eq. 12: the
/// new unit discharge across a face from the old ones on it and its two
/// neighbours along the same axis, the free-surface slope and an implicit
/// Manning friction term. `left` is the cell on the negative side.
#[inline]
#[allow(clippy::too_many_arguments)]
fn face_update(
    p: &FluxParams,
    q_old: f64,
    q_prev: f64,
    q_next: f64,
    z_l: f64,
    h_l: f64,
    z_r: f64,
    h_r: f64,
    n_l: f64,
    n_r: f64,
) -> f64 {
    if z_l.is_nan() || z_r.is_nan() {
        return 0.0;
    }
    let eta_l = z_l + h_l;
    let eta_r = z_r + h_r;
    // Effective flow depth at the face (Bates eq. 12).
    let hf = eta_l.max(eta_r) - z_l.max(z_r);
    if hf <= p.dry {
        return 0.0;
    }
    let slope = (eta_r - eta_l) / p.dx;
    let n = 0.5 * (n_l + n_r);
    let q_c = p.theta * q_old + 0.5 * (1.0 - p.theta) * (q_prev + q_next);
    let numerator = q_c - p.g * hf * p.dt * slope;
    let denominator = 1.0 + p.g * p.dt * n * n * q_old.abs() / (p.kn * p.kn * hf.powf(7.0 / 3.0));
    numerator / denominator
}

/// Flux leaving the grid through an edge face at normal depth, for an
/// `Open` boundary: `q = (k/n) h^{5/3} S^{1/2}` with `S` the larger of the
/// bed and water-surface slopes towards the edge (never negative), so
/// still water against a level edge stays put.
#[inline]
fn open_edge(p: &FluxParams, z: f64, h: f64, n: f64, z_in: f64, h_in: f64) -> f64 {
    if z.is_nan() || z_in.is_nan() || h <= p.dry {
        return 0.0;
    }
    let s_bed = (z_in - z) / p.dx;
    let s_wse = ((z_in + h_in) - (z + h)) / p.dx;
    let s = s_bed.max(s_wse).max(0.0);
    p.kn / n * h.powf(5.0 / 3.0) * s.sqrt()
}

/// Unit discharge across the vertical face on the west side of `(col,row)`
/// (`col == ncols` is the east edge of the last column). Positive = +x.
fn flux_x(
    p: &FluxParams,
    z: &[f64],
    h: &[f64],
    n: &[f64],
    qx: &[f64],
    row: usize,
    col: usize,
) -> f64 {
    let w = p.ncols + 1;
    let q_old = qx[row * w + col];
    let cell = |c: usize| row * p.ncols + c;
    if col == 0 || col == p.ncols {
        // Edge face.
        let (ci, inner, outward) = if col == 0 {
            (cell(0), cell(1), -1.0)
        } else {
            (cell(p.ncols - 1), cell(p.ncols - 2), 1.0)
        };
        return match p.boundary {
            Boundary::Closed => 0.0,
            Boundary::Open => outward * open_edge(p, z[ci], h[ci], n[ci], z[inner], h[inner]),
            Boundary::FixedHead(head) => {
                let zg = z[ci];
                let hg = (head - zg).max(0.0);
                if col == 0 {
                    face_update(p, q_old, q_old, q_old, zg, hg, z[ci], h[ci], n[ci], n[ci])
                } else {
                    face_update(p, q_old, q_old, q_old, z[ci], h[ci], zg, hg, n[ci], n[ci])
                }
            }
        };
    }
    let l = cell(col - 1);
    let r = cell(col);
    let q_prev = qx[row * w + col - 1];
    let q_next = qx[row * w + col + 1];
    face_update(p, q_old, q_prev, q_next, z[l], h[l], z[r], h[r], n[l], n[r])
}

/// Unit discharge across the horizontal face on the north side of row
/// `frow` (`frow == nrows` is the south edge). Positive = towards
/// increasing row (south).
fn flux_y(
    p: &FluxParams,
    z: &[f64],
    h: &[f64],
    n: &[f64],
    qy: &[f64],
    frow: usize,
    col: usize,
) -> f64 {
    let q_old = qy[frow * p.ncols + col];
    let cell = |r: usize| r * p.ncols + col;
    if frow == 0 || frow == p.nrows {
        let (ci, inner, outward) = if frow == 0 {
            (cell(0), cell(1), -1.0)
        } else {
            (cell(p.nrows - 1), cell(p.nrows - 2), 1.0)
        };
        return match p.boundary {
            Boundary::Closed => 0.0,
            Boundary::Open => outward * open_edge(p, z[ci], h[ci], n[ci], z[inner], h[inner]),
            Boundary::FixedHead(head) => {
                let zg = z[ci];
                let hg = (head - zg).max(0.0);
                if frow == 0 {
                    face_update(p, q_old, q_old, q_old, zg, hg, z[ci], h[ci], n[ci], n[ci])
                } else {
                    face_update(p, q_old, q_old, q_old, z[ci], h[ci], zg, hg, n[ci], n[ci])
                }
            }
        };
    }
    let u = cell(frow - 1);
    let d = cell(frow);
    let q_prev = qy[(frow - 1) * p.ncols + col];
    let q_next = qy[(frow + 1) * p.ncols + col];
    face_update(p, q_old, q_prev, q_next, z[u], h[u], z[d], h[d], n[u], n[d])
}

// ---------------------------------------------------------------------------
// HLL fluxes (Toro 2001, ch. 10) with hydrostatic reconstruction
// ---------------------------------------------------------------------------

/// The HLL approximate Riemann flux for the 1D shallow-water system
/// between states `(hl, ul)` and `(hr, ur)`, with the transverse momentum
/// carried by the mass flux at the upwind side's velocity. Wave speeds are
/// the Davis/Toro estimates, with Toro's dry-bed forms when one side is
/// dry. Returns `(mass, normal momentum, transverse momentum)`.
fn hll(g: f64, hl: f64, ul: f64, vl: f64, hr: f64, ur: f64, vr: f64) -> (f64, f64, f64) {
    if hl <= 0.0 && hr <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let cl = (g * hl.max(0.0)).sqrt();
    let cr = (g * hr.max(0.0)).sqrt();
    let (sl, sr) = if hl <= 0.0 {
        (ur - 2.0 * cr, ur + cr)
    } else if hr <= 0.0 {
        (ul - cl, ul + 2.0 * cl)
    } else {
        ((ul - cl).min(ur - cr), (ul + cl).max(ur + cr))
    };
    let fl = (hl * ul, hl * ul * ul + 0.5 * g * hl * hl);
    let fr = (hr * ur, hr * ur * ur + 0.5 * g * hr * hr);
    let (fh, fm) = if sl >= 0.0 {
        fl
    } else if sr <= 0.0 {
        fr
    } else {
        let d = sr - sl;
        (
            (sr * fl.0 - sl * fr.0 + sl * sr * (hr - hl)) / d,
            (sr * fl.1 - sl * fr.1 + sl * sr * (hr * ur - hl * ul)) / d,
        )
    };
    let ft = fh * if fh >= 0.0 { vl } else { vr };
    (fh, fm, ft)
}

/// Flux across an interior face. The bed step is handled by the
/// hydrostatic reconstruction of Audusse et al. (2004): both depths are
/// measured above the higher of the two beds, and the reconstructed
/// depths are kept for the cells' bed-slope source terms.
#[allow(clippy::too_many_arguments)]
fn face_flux_hll(g: f64, zl: f64, hl: f64, ul: f64, vl: f64, zr: f64, hr: f64, ur: f64, vr: f64) -> FaceFlux {
    if zl.is_nan() && zr.is_nan() {
        return FaceFlux::default();
    }
    if zl.is_nan() {
        return wall_flux(g, hr);
    }
    if zr.is_nan() {
        return wall_flux(g, hl);
    }
    let zf = zl.max(zr);
    let hls = (zl + hl - zf).max(0.0);
    let hrs = (zr + hr - zf).max(0.0);
    let (h, normal, transverse) = hll(g, hls, ul, vl, hrs, ur, vr);
    FaceFlux {
        h,
        normal,
        transverse,
        left: hls,
        right: hrs,
    }
}

/// A reflecting wall: no mass, hydrostatic pressure only, balanced
/// against the cell's own source term.
fn wall_flux(g: f64, h: f64) -> FaceFlux {
    FaceFlux {
        h: 0.0,
        normal: 0.5 * g * h * h,
        transverse: 0.0,
        left: h,
        right: h,
    }
}

/// Flux across a grid-edge face for the cell state `(z, h, un, ut)`,
/// where `un` is the velocity normal to the face (positive along the
/// axis) and `cell_is_right` says the cell is on the positive side.
fn edge_flux_hll(g: f64, boundary: Boundary, z: f64, h: f64, un: f64, ut: f64, cell_is_right: bool) -> FaceFlux {
    if z.is_nan() {
        return FaceFlux::default();
    }
    match boundary {
        Boundary::Closed => wall_flux(g, h),
        // Zero gradient for outflow: the ghost carries the cell's own
        // state. Flow pointing into the grid is blocked (a wall), or the
        // edge would manufacture water.
        Boundary::Open => {
            let outward = if cell_is_right { un < 0.0 } else { un > 0.0 };
            if outward {
                FaceFlux {
                    h: h * un,
                    normal: h * un * un + 0.5 * g * h * h,
                    transverse: h * un * ut,
                    left: h,
                    right: h,
                }
            } else {
                wall_flux(g, h)
            }
        }
        Boundary::FixedHead(head) => {
            let hg = (head - z).max(0.0);
            if cell_is_right {
                face_flux_hll(g, z, hg, 0.0, 0.0, z, h, un, ut)
            } else {
                face_flux_hll(g, z, h, un, ut, z, hg, 0.0, 0.0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The run driver
// ---------------------------------------------------------------------------

/// Exchange hook called at the start of every synchronisation interval
/// with the simulator at time `t`, the interval length, and the per-node
/// volume accumulators (positive = onto the surface) the results file
/// records.
pub type Exchange<'a> = dyn FnMut(&mut Simulator, f64, f64, &mut [f64]) -> Result<()> + 'a;

/// Drive a simulator over the setup's duration, writing frames every
/// `output_step_s` and calling `exchange` every `sync_s`.
pub fn run_surface(
    setup: &Setup,
    sim: &mut Simulator,
    sync_s: f64,
    progress: &mut dyn FnMut(&Progress) -> bool,
    exchange: &mut Exchange<'_>,
) -> Result<RunSummary> {
    let started = std::time::Instant::now();
    let duration = setup
        .config
        .duration_s
        .unwrap_or(setup.inputs.model_duration_s);
    if duration.is_nan() || duration <= 0.0 {
        return Err(Error::Format(
            "2D run duration is zero: set [RUN] DURATION or the model's END_DATE/END_TIME".into(),
        ));
    }
    let frame_step = setup.config.output_step_s.min(duration);
    let sync = if sync_s > 0.0 { sync_s.min(frame_step) } else { frame_step };
    let node_names: Vec<String> = setup.nodes.iter().map(|n| n.interface.node.clone()).collect();
    let path = setup.results_path();
    let mut writer = Writer::create(
        &path,
        &setup.dem,
        setup.metric,
        frame_step,
        setup.config.dry_depth,
        &node_names,
    )?;
    let mut node_vol = vec![0.0f64; setup.nodes.len()];
    let mut sink_seen = vec![0.0f64; sim.sinks.len()];
    let mut frames = 0usize;
    let mut warnings = setup.warnings.clone();
    let mut peak_depth: f64 = 0.0;
    let mut wet_area_max: f64 = 0.0;
    let area = sim.dx * sim.dx;

    let write_frame = |sim: &mut Simulator,
                       writer: &mut Writer,
                       node_vol: &mut [f64],
                       sink_seen: &mut [f64],
                       dt_frame: f64|
     -> Result<()> {
        for (si, s) in sim.sinks.iter().enumerate() {
            let taken = sim.sink_volume[si] - sink_seen[si];
            sink_seen[si] = sim.sink_volume[si];
            if let Some(v) = node_vol.get_mut(s.node) {
                *v -= taken;
            }
        }
        let rates: Vec<f32> = node_vol
            .iter()
            .map(|v| if dt_frame > 0.0 { (*v / dt_frame) as f32 } else { 0.0 })
            .collect();
        let (d, vx, vy) = sim.frame();
        writer.frame(sim.time, &d, &vx, &vy, &rates)?;
        node_vol.iter_mut().for_each(|v| *v = 0.0);
        Ok(())
    };

    write_frame(sim, &mut writer, &mut node_vol, &mut sink_seen, 0.0)?;
    frames += 1;
    let mut next_frame = frame_step;
    let mut last_frame_t = 0.0;
    let mut stopped = false;
    while sim.time < duration - 1e-9 {
        let t = sim.time;
        let t_sync = (t + sync).min(next_frame).min(duration);
        let dt_sync = t_sync - t;
        exchange(sim, t, dt_sync, &mut node_vol)?;
        sim.advance_to(t_sync);
        peak_depth = peak_depth.max(sim.hmax);
        let wet = sim.wet_cells();
        wet_area_max = wet_area_max.max(wet as f64 * area);
        if sim.time >= next_frame - 1e-9 || sim.time >= duration - 1e-9 {
            write_frame(sim, &mut writer, &mut node_vol, &mut sink_seen, sim.time - last_frame_t)?;
            frames += 1;
            last_frame_t = sim.time;
            next_frame = (next_frame + frame_step).min(duration);
            let p = Progress {
                time_s: sim.time,
                duration_s: duration,
                dt_s: sim.last_dt,
                wet_cells: wet,
                volume: sim.stored(),
                mass_error_pct: sim.mass_error_pct(),
            };
            if !progress(&p) {
                stopped = true;
                warnings.push(format!("2D run stopped by the caller at {:.0} s", sim.time));
                break;
            }
        }
    }
    let (md, ms, mh, ar) = sim.maxima();
    writer.finish(md, ms, mh, ar)?;
    let stored = sim.stored();
    let _ = stopped;
    Ok(RunSummary {
        results: path,
        steps: sim.steps,
        frames,
        elapsed_s: started.elapsed().as_secs_f64(),
        mass_error_pct: sim.balance.error_pct(stored),
        peak_depth,
        wet_area_max,
        inflow: sim.balance.inflow,
        outflow: sim.balance.outflow,
        infiltrated: sim.balance.infiltrated,
        stored,
        warnings,
    })
}

/// The surface alone: rain, sources and outfall sinks, no network.
pub fn run(setup: &Setup, progress: &mut dyn FnMut(&Progress) -> bool) -> Result<RunSummary> {
    run_with_scheme(setup, Scheme::LocalInertial, progress)
}

/// The surface alone with a chosen momentum scheme.
pub fn run_with_scheme(
    setup: &Setup,
    scheme: Scheme,
    progress: &mut dyn FnMut(&Progress) -> bool,
) -> Result<RunSummary> {
    let mut sim = Simulator::from_setup(setup);
    sim.scheme = scheme;
    let sync = setup.config.output_step_s;
    run_surface(setup, &mut sim, sync, progress, &mut |_, _, _, _| Ok(()))
}
