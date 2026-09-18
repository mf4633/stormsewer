// SPDX-License-Identifier: GPL-3.0-or-later

//! Step-by-step control of an EPA SWMM engine through its DLL API.
//!
//! EPA's Windows build of `swmm5.dll` is 32-bit and StormSewer is 64-bit, so
//! the DLL cannot be loaded in-process. A small 32-bit helper executable
//! (`stormsewer-swmm-bridge32.exe`, the `bridge` crate in this workspace)
//! loads it and speaks a line protocol on stdin/stdout; this module is the
//! client. The engine itself is still EPA's, unmodified: the bridge calls
//! `swmm_open`, `swmm_start`, `swmm_step`, `swmm_getValue`, `swmm_setValue`,
//! `swmm_end`, `swmm_report`, `swmm_close` exactly as `runswmm` would.
//!
//! This file is the CONTRACT the 1D-2D coupling compiles against; the
//! engine-control stream fills the bodies and writes the bridge crate.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Where the bridge executable is: beside the app, `STORMSEWER_SWMM_BRIDGE`,
/// or nowhere (None — tight coupling is then unavailable).
pub fn find_bridge() -> Option<PathBuf> {
    None
}

/// The engine DLL that belongs to an engine executable: `swmm5.dll` beside
/// `runswmm.exe`, or None.
pub fn find_dll(engine_exe: &Path) -> Option<PathBuf> {
    let _ = engine_exe;
    None
}

/// A live engine run.
#[derive(Debug)]
pub struct Session {
    /// Simulation length in seconds, from `swmm_open`.
    pub duration_s: f64,
    /// Routing step the engine will use, seconds.
    pub routing_step_s: f64,
    /// Node names in engine index order.
    pub node_names: Vec<String>,
    pub link_names: Vec<String>,
    pub bridge_version: String,
    pub engine_version: String,
}

impl Session {
    /// Start the bridge, load the DLL, open and start the model. The `.rpt`
    /// and `.out` are written by the engine as in a normal run.
    pub fn open(bridge: &Path, dll: &Path, inp: &Path, rpt: &Path, out: &Path) -> Result<Self> {
        let _ = (bridge, dll, inp, rpt, out);
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Advance one routing step. Returns the elapsed simulation time in
    /// seconds, or None when the run is finished.
    pub fn step(&mut self) -> Result<Option<f64>> {
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Advance until `time_s` (inclusive of the step that reaches it).
    pub fn step_until(&mut self, time_s: f64) -> Result<Option<f64>> {
        let _ = time_s;
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Hydraulic head at a node, in the model's length units.
    pub fn node_head(&mut self, node: &str) -> Result<f64> {
        let _ = node;
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    pub fn node_depth(&mut self, node: &str) -> Result<f64> {
        let _ = node;
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Flooding (overflow) rate at a node now, in the model's flow units.
    pub fn node_overflow(&mut self, node: &str) -> Result<f64> {
        let _ = node;
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Set the lateral inflow the engine applies at a node from now on, in
    /// the model's flow units (negative = withdrawal, which the engine caps
    /// at the node's stored volume).
    pub fn set_node_lateral_inflow(&mut self, node: &str, flow: f64) -> Result<()> {
        let _ = (node, flow);
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Flow in a link now.
    pub fn link_flow(&mut self, link: &str) -> Result<f64> {
        let _ = link;
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Set a link's control setting (0..1) — pump on/off, orifice opening.
    pub fn set_link_setting(&mut self, link: &str, setting: f64) -> Result<()> {
        let _ = (link, setting);
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Finish: `swmm_end`, `swmm_report`, `swmm_close`, and the continuity
    /// errors (runoff, flow, quality) in percent.
    pub fn finish(self) -> Result<(f64, f64, f64)> {
        Err(Error::Engine("engine bridge not built yet".into()))
    }

    /// Abort the run and kill the bridge.
    pub fn abort(self) {}
}
