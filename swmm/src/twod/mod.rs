// SPDX-License-Identifier: GPL-3.0-or-later

//! 2D overland flow on a DEM grid, and its interfaces to the 1D SWMM network.
//!
//! This file is the CONTRACT between the solver (`solver`, `couple`), the
//! app's 2D views (`app/src/swmm_twod.rs`) and the CLI. The types and
//! signatures below are what the app compiles against; the bodies are filled
//! by the 2D engine stream. Change a signature here only together with every
//! caller.
//!
//! # What lives where
//!
//! - [`Config`]: everything a 2D run needs that is not in the `.inp`, kept
//!   in a sidecar text file `<model>.2d` beside the model (the `.inp` stays
//!   EPA's format; an unknown section would make the engine refuse it).
//! - [`Setup`]: the config resolved against a model — DEM loaded, roughness
//!   and rainfall grids built, interfaces located on the grid.
//! - [`run`]: the 2D-only run, results to `<model>.2d.out`.
//! - `couple::run_coupled`: 1D-2D, tight through the engine bridge or
//!   iterative through `runswmm`.
//! - [`Results`]: reader for `<model>.2d.out` (frames of depth and unit
//!   discharge, plus the maxima and arrival-time grids).

use std::path::{Path, PathBuf};

use crate::doc::InpDoc;
use crate::gis::raster::Raster;
use crate::{Error, Result};

pub mod couple;
pub mod solver;

/// How a boundary edge of the grid behaves.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Boundary {
    /// Wall: no flow across the edge.
    #[default]
    Closed,
    /// Free outflow: water leaves at the local normal-depth gradient.
    Open,
    /// Fixed water-surface elevation (a receiving water body).
    FixedHead(f64),
}

/// Roughness for the grid.
#[derive(Clone, Debug, PartialEq)]
pub enum Roughness {
    Uniform(f64),
    /// A raster of Manning's n on the DEM's grid (or resampled to it).
    Grid(PathBuf),
    /// Manning's n from a land-cover raster and a lookup of class → n.
    Classes { raster: PathBuf, table: Vec<(i64, f64)> },
}

impl Default for Roughness {
    fn default() -> Self {
        Self::Uniform(0.05)
    }
}

/// Rain falling directly on the grid.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum RainOnGrid {
    /// No direct rainfall: water reaches the surface only through the
    /// interfaces (surcharging nodes) and sources.
    #[default]
    None,
    /// The same gage as the model's first rain gage (its time series).
    Gage(String),
    /// A constant intensity, in the model's rain units (in/hr or mm/hr).
    Constant(f64),
}

/// Infiltration from the 2D surface.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Infiltration {
    #[default]
    None,
    /// Constant loss rate, in the model's rain units.
    Constant(f64),
    /// Horton decay from `f0` to `fc` with rate `k` (1/hr).
    Horton { f0: f64, fc: f64, k: f64 },
}

/// A point inflow to the surface that is not a network node.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub name: String,
    pub x: f64,
    pub y: f64,
    /// Name of a `[TIMESERIES]` in the model, flow in the model's flow units.
    pub series: String,
}

/// How a node exchanges water with the cell it sits in.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum InterfaceKind {
    /// A manhole: surcharge flows out when the node head exceeds ground;
    /// ponded water re-enters through the lid only if `lid_open` is set.
    #[default]
    Manhole,
    /// An inlet: captures surface water by weir/orifice with the given
    /// perimeter (ft or m) and clear opening area (ft² or m²).
    Inlet { perimeter: f64, area: f64 },
    /// No exchange (the node is sealed).
    Sealed,
}

/// The 1D-2D interface at a node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeInterface {
    pub node: String,
    pub kind: InterfaceKind,
    /// Weir coefficient for surcharge/capture (dimensionless, default 0.6 of
    /// the unit-system constant). `None` = default.
    pub weir_coeff: Option<f64>,
    /// Open lid: water can also flow back INTO a manhole.
    pub lid_open: bool,
}

/// A bank line: an open-channel conduit exchanging with the cells along a
/// polyline over a crest at `crest` elevation (or the DEM where None).
#[derive(Clone, Debug, PartialEq)]
pub struct BankInterface {
    pub link: String,
    /// Left or right bank, as seen looking downstream.
    pub right: bool,
    pub polyline: Vec<(f64, f64)>,
    pub crest: Option<f64>,
    pub weir_coeff: Option<f64>,
}

/// Everything a 2D run needs that is not in the `.inp`.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// DEM path, `.asc` or `.tif`, in the model's map units.
    pub dem: Option<PathBuf>,
    /// Resample the DEM to this cell size (None = use the DEM's).
    pub cell: Option<f64>,
    /// Restrict the grid to this window (xmin, ymin, xmax, ymax) of the DEM.
    pub window: Option<(f64, f64, f64, f64)>,
    pub roughness: Roughness,
    pub rain: RainOnGrid,
    pub infiltration: Infiltration,
    pub boundary: Boundary,
    /// Simulation length in seconds (None = the model's own duration).
    pub duration_s: Option<f64>,
    /// Interval between saved frames, seconds.
    pub output_step_s: f64,
    /// Depth below which a cell is dry, in the DEM's units.
    pub dry_depth: f64,
    /// Courant number for the adaptive time step (0.7 is the usual choice).
    pub courant: f64,
    /// Explicit time-step cap, seconds (None = CFL only).
    pub max_dt_s: Option<f64>,
    pub sources: Vec<Source>,
    /// Node interfaces. Nodes not listed get the default (`Manhole`).
    pub nodes: Vec<NodeInterface>,
    pub banks: Vec<BankInterface>,
    /// Nodes to exclude from automatic interfacing.
    pub sealed: Vec<String>,
    /// Number of worker threads (0 = all cores).
    pub threads: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dem: None,
            cell: None,
            window: None,
            roughness: Roughness::default(),
            rain: RainOnGrid::default(),
            infiltration: Infiltration::default(),
            boundary: Boundary::default(),
            duration_s: None,
            output_step_s: 300.0,
            dry_depth: 0.003,
            courant: 0.7,
            max_dt_s: None,
            sources: Vec::new(),
            nodes: Vec::new(),
            banks: Vec::new(),
            sealed: Vec::new(),
            threads: 0,
        }
    }
}

impl Config {
    /// The sidecar path for a model: `<model>.2d` beside `<model>.inp`.
    pub fn sidecar_path(model: &Path) -> PathBuf {
        model.with_extension("2d")
    }

    /// Read the sidecar; a missing file is the default config.
    pub fn read(path: &Path) -> Result<Self> {
        let _ = path;
        Err(Error::Format("2D engine not built yet".into()))
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let _ = path;
        Err(Error::Format("2D engine not built yet".into()))
    }

    /// Parse the sidecar text (the format is documented in the manual).
    pub fn parse(text: &str) -> Result<Self> {
        let _ = text;
        Err(Error::Format("2D engine not built yet".into()))
    }

    pub fn to_text(&self) -> String {
        String::new()
    }
}

/// A node interface located on the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatedNode {
    pub interface: NodeInterface,
    pub col: usize,
    pub row: usize,
    /// Ground elevation at the cell (DEM), in the DEM's units.
    pub ground: f64,
    /// The node's own rim (invert + max depth) from the model.
    pub rim: f64,
}

/// A bank interface rasterised onto cells.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatedBank {
    pub interface: BankInterface,
    /// Cells along the line with the crest elevation used at each.
    pub cells: Vec<(usize, usize, f64)>,
}

/// The config resolved against a model.
#[derive(Clone, Debug)]
pub struct Setup {
    pub model: PathBuf,
    pub config: Config,
    pub dem: Raster,
    pub manning: Raster,
    pub nodes: Vec<LocatedNode>,
    pub banks: Vec<LocatedBank>,
    /// Whether the model is metric (CMS/LPS/MLD) — governs g and Manning's
    /// unit constant.
    pub metric: bool,
    pub warnings: Vec<String>,
}

impl Setup {
    /// Load the DEM, build the roughness grid, locate every node with
    /// coordinates on the grid, rasterise the bank lines. Nodes outside the
    /// DEM become warnings, not errors.
    pub fn build(model: &Path, doc: &InpDoc, config: &Config) -> Result<Self> {
        let _ = (model, doc, config);
        Err(Error::Format("2D engine not built yet".into()))
    }

    pub fn results_path(&self) -> PathBuf {
        results_path(&self.model)
    }
}

/// `<model>.2d.out` beside `<model>.inp`.
pub fn results_path(model: &Path) -> PathBuf {
    let mut p = model.to_path_buf();
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    p.set_file_name(format!("{stem}.2d.out"));
    p
}

/// Progress report from a run; the callback returns `false` to stop.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub time_s: f64,
    pub duration_s: f64,
    pub dt_s: f64,
    pub wet_cells: usize,
    /// Volume on the surface now.
    pub volume: f64,
    /// Running mass-balance error, percent.
    pub mass_error_pct: f64,
}

/// What a finished run reports.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSummary {
    pub results: PathBuf,
    pub steps: usize,
    pub frames: usize,
    pub elapsed_s: f64,
    /// Final mass-balance error, percent of total inflow.
    pub mass_error_pct: f64,
    pub peak_depth: f64,
    pub wet_area_max: f64,
    /// Total volumes, in the DEM's cubic units.
    pub inflow: f64,
    pub outflow: f64,
    pub infiltrated: f64,
    pub stored: f64,
    pub warnings: Vec<String>,
}

/// Run the surface alone: interfaces feed from nothing (no 1D), sources and
/// rain-on-grid still apply. Writes `setup.results_path()`.
pub fn run(setup: &Setup, progress: &mut dyn FnMut(&Progress) -> bool) -> Result<RunSummary> {
    let _ = (setup, progress);
    Err(Error::Format("2D engine not built yet".into()))
}

/// One saved frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub time_s: f64,
    /// Depth per cell, row-major top row first, NaN where the DEM is no-data.
    pub depth: Vec<f32>,
    /// Velocity components per cell (m/s or ft/s).
    pub vx: Vec<f32>,
    pub vy: Vec<f32>,
}

/// Reader for `<model>.2d.out`.
#[derive(Debug)]
pub struct Results {
    pub path: PathBuf,
    pub ncols: usize,
    pub nrows: usize,
    pub x0: f64,
    pub y0: f64,
    pub cell: f64,
    pub metric: bool,
    pub n_frames: usize,
    pub frame_step_s: f64,
    /// Node interfaces recorded in the file, with their per-frame exchange
    /// flows (positive = 1D → 2D), for the profile and the run panel.
    pub node_names: Vec<String>,
}

impl Results {
    pub fn open(path: &Path) -> Result<Self> {
        let _ = path;
        Err(Error::Format("2D engine not built yet".into()))
    }

    pub fn frame(&self, i: usize) -> Result<Frame> {
        let _ = i;
        Err(Error::Format("2D engine not built yet".into()))
    }

    /// Maximum depth over the run.
    pub fn max_depth(&self) -> Result<Raster> {
        Err(Error::Format("2D engine not built yet".into()))
    }

    /// Maximum velocity magnitude over the run.
    pub fn max_velocity(&self) -> Result<Raster> {
        Err(Error::Format("2D engine not built yet".into()))
    }

    /// Maximum depth × velocity (the usual hazard index).
    pub fn max_hazard(&self) -> Result<Raster> {
        Err(Error::Format("2D engine not built yet".into()))
    }

    /// Seconds from the start until each cell first exceeds `dry_depth`;
    /// NaN where it never does.
    pub fn arrival(&self) -> Result<Raster> {
        Err(Error::Format("2D engine not built yet".into()))
    }

    /// Exchange flow at a node interface per frame (positive = out of the
    /// network onto the surface).
    pub fn node_exchange(&self, node: &str) -> Result<Vec<(f64, f64)>> {
        let _ = node;
        Err(Error::Format("2D engine not built yet".into()))
    }

    /// Depth and velocity at a world point in a frame.
    pub fn sample(&self, frame: &Frame, x: f64, y: f64) -> Option<(f64, f64, f64)> {
        let _ = (frame, x, y);
        None
    }
}
