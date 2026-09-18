// SPDX-License-Identifier: GPL-3.0-or-later

//! 1D-2D coupling: the SWMM network and the surface exchange water at the
//! node and bank interfaces while both advance in time.
//!
//! Two modes. **Tight**: the engine is driven step by step through the
//! bridge (`crate::bridge`), node heads are read and lateral inflows set
//! every synchronisation interval, so surcharge and re-entry are resolved
//! within the same time step. **Iterative**: `runswmm` runs the whole 1D
//! model, its node overflow hydrographs drive the surface, the surface's
//! captured flows are written back as `[INFLOWS]` time series into a
//! scratch copy, and the pair is re-run until the exchange volumes stop
//! changing. Iterative works with any engine and no bridge; tight is the
//! one to use when the surface feeds the network back.

use std::path::PathBuf;

use crate::engine::Engine;
use crate::{Error, Result};

use super::{Progress, RunSummary, Setup};

/// Which coupling to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Step the engine through the bridge executable at `bridge` with the
    /// engine DLL at `dll`, exchanging every `sync_s` seconds.
    Tight {
        bridge: PathBuf,
        dll: PathBuf,
        sync_s: f64,
    },
    /// Re-run `runswmm` up to `iterations` times, stopping early when the
    /// total exchanged volume changes by less than `tolerance` (fraction).
    Iterative { iterations: usize, tolerance: f64 },
}

/// What a coupled run reports beyond the surface summary.
#[derive(Clone, Debug, PartialEq)]
pub struct CoupledSummary {
    pub surface: RunSummary,
    /// The 1D run's files (the scratch copy for iterative mode).
    pub inp: PathBuf,
    pub rpt: PathBuf,
    pub out: PathBuf,
    /// Iterations actually run (1 for tight).
    pub iterations: usize,
    /// Volume that left the network onto the surface, and that came back.
    pub surcharged: f64,
    pub captured: f64,
    pub warnings: Vec<String>,
}

/// Run the surface and the network together. Writes `setup.results_path()`
/// and the 1D results; the progress callback returns `false` to stop.
pub fn run_coupled(
    setup: &Setup,
    engine: &Engine,
    mode: &Mode,
    progress: &mut dyn FnMut(&Progress) -> bool,
) -> Result<CoupledSummary> {
    let _ = (setup, engine, mode, progress);
    Err(Error::Format("1D-2D coupling not built yet".into()))
}
