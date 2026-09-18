// SPDX-License-Identifier: GPL-3.0-or-later

//! Reader for the EPA SWMM 5 binary output (`.out`) file.
//!
//! Layout, little-endian throughout:
//!
//! ```text
//! 0    magic (516114522), version, flow-unit code,
//!      n_subcatch, n_nodes, n_links, n_pollutants        7 × i32
//! ...  per-object property blocks
//! id_offset (=28) object IDs: each is i32 length + that many UTF-8 bytes,
//!                 in order subcatchments, nodes, links, pollutants
//!      pollutant concentration unit codes         n_pollutants × i32
//! input_offset    per-object input properties, each block a count of
//!                 properties, that many property codes, then the values:
//!                   subcatchments: 1 (area)            n_subcatch × f32
//!                   nodes: 3 (type, invert, max depth) n_nodes × (i32 + 2 f32)
//!                   links: 5 (type, 2 offsets, max     n_links × (i32 + 4 f32)
//!                          depth, length)
//!      reporting-variable selections: for subcatchments, nodes, links and
//!      the system, a count then that many variable codes (i32 each)
//! output_offset-12  start date (f64 days since 1899-12-30), report step (i32 s)
//! output_offset   n_periods records, each:
//!                   f64 date
//!                   f32 × n_subcatch × (8 + n_pollutants)
//!                   f32 × n_nodes    × (6 + n_pollutants)
//!                   f32 × n_links    × (5 + n_pollutants)
//!                   f32 × 15 system variables
//! size-24  id_offset, input_offset, output_offset, n_periods,
//!          error_code, magic                             6 × i32
//! ```
//!
//! The closing block is read first: it is the only place the period count
//! lives, and a file whose trailing magic is missing is a run that died
//! mid-write, which is worth reporting as such rather than as garbage data.
//!
//! While the engine is still running, the closing block does not exist yet.
//! [`read_metadata_partial`] walks the header forward instead (the layout
//! above is what `output.c` writes, block by block) and counts the whole
//! records present from the file size, which is what live results and a
//! stopped run read.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub const MAGIC: i32 = 516_114_522;

const N_SUBCATCH_VARS_BASE: usize = 8;
const N_NODE_VARS_BASE: usize = 6;
const N_LINK_VARS_BASE: usize = 5;
const N_SYS_VARS: usize = 15;

/// Upper bounds used only to reject corrupt headers before allocating.
const MAX_OBJECTS: i32 = 20_000_000;
const MAX_ID_LEN: i32 = 1024;

/// Subcatchment reporting variables. Index `8 + i` is pollutant `i`.
///
/// `Runoff`'s index was confirmed against EPA SWMM 5.2.4's own arithmetic
/// rather than taken from the manual: running `Site_Drainage_Model` and taking
/// each variable's maximum over all 360 periods, index 4 matched the `.rpt`
/// Subcatchment Runoff Summary peak for all seven subcatchments — 7.35, 8.47,
/// 4.24, 9.70, 11.75, 5.23 and 0.00 CFS — and no other index was close.
/// Index 3 corroborates it: S7, the one subcatchment that infiltrates
/// everything and runs off nothing, holds the largest value there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubcatchVariable {
    Rainfall = 0,
    SnowDepth = 1,
    Evaporation = 2,
    Infiltration = 3,
    Runoff = 4,
    GroundwaterFlow = 5,
    GroundwaterElevation = 6,
    SoilMoisture = 7,
}

/// Node reporting variables. Index `6 + i` is pollutant `i`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeVariable {
    Depth = 0,
    Head = 1,
    Volume = 2,
    LateralInflow = 3,
    TotalInflow = 4,
    Flooding = 5,
}

/// Link reporting variables. Index `5 + i` is pollutant `i`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkVariable {
    Flow = 0,
    Depth = 1,
    Velocity = 2,
    Volume = 3,
    Capacity = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowUnits {
    Cfs,
    Gpm,
    Mgd,
    Cms,
    Lps,
    Mld,
    Unknown(i32),
}

impl FlowUnits {
    fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Cfs,
            1 => Self::Gpm,
            2 => Self::Mgd,
            3 => Self::Cms,
            4 => Self::Lps,
            5 => Self::Mld,
            other => Self::Unknown(other),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Cfs => "CFS".into(),
            Self::Gpm => "GPM".into(),
            Self::Mgd => "MGD".into(),
            Self::Cms => "CMS".into(),
            Self::Lps => "LPS".into(),
            Self::Mld => "MLD".into(),
            Self::Unknown(c) => format!("unit code {c}"),
        }
    }

    /// True for the SI set, so a caller can label axes without a lookup table.
    pub fn is_metric(&self) -> bool {
        matches!(self, Self::Cms | Self::Lps | Self::Mld)
    }
}

/// Header, object names, and closing-block offsets of a `.out` file.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputMetadata {
    pub version: i32,
    pub flow_units: FlowUnits,
    pub n_subcatch: usize,
    pub n_nodes: usize,
    pub n_links: usize,
    pub n_pollutants: usize,
    pub n_periods: usize,
    pub report_step_s: i32,
    /// Simulation start, in days since 1899-12-30 (SWMM's epoch).
    pub start_days: f64,
    pub subcatch_ids: Vec<String>,
    pub node_ids: Vec<String>,
    pub link_ids: Vec<String>,
    pub pollutant_ids: Vec<String>,
    /// Non-zero when the engine recorded a fatal error in this file.
    pub error_code: i32,
    pub output_offset: u64,
}

impl OutputMetadata {
    pub fn n_subcatch_vars(&self) -> usize {
        N_SUBCATCH_VARS_BASE + self.n_pollutants
    }
    pub fn n_node_vars(&self) -> usize {
        N_NODE_VARS_BASE + self.n_pollutants
    }
    pub fn n_link_vars(&self) -> usize {
        N_LINK_VARS_BASE + self.n_pollutants
    }

    pub fn bytes_per_period(&self) -> u64 {
        8 + 4 * (self.n_subcatch * self.n_subcatch_vars()
            + self.n_nodes * self.n_node_vars()
            + self.n_links * self.n_link_vars()
            + N_SYS_VARS) as u64
    }

    /// Seconds from simulation start for reporting period `p` (0-based).
    /// SWMM writes the first period one report step in, not at t = 0.
    pub fn period_seconds(&self, p: usize) -> f64 {
        (p as f64 + 1.0) * self.report_step_s as f64
    }

    pub fn duration_seconds(&self) -> f64 {
        self.n_periods as f64 * self.report_step_s as f64
    }
}

fn read_i32(f: &mut impl Read) -> io::Result<i32> {
    let mut b = [0u8; 4];
    f.read_exact(&mut b)?;
    Ok(i32::from_le_bytes(b))
}

fn read_f64(f: &mut impl Read) -> io::Result<f64> {
    let mut b = [0u8; 8];
    f.read_exact(&mut b)?;
    Ok(f64::from_le_bytes(b))
}

fn read_id(f: &mut File, limit: u64) -> Result<String> {
    let n = read_i32(f)?;
    if !(0..=MAX_ID_LEN).contains(&n) {
        return Err(Error::Format(format!(
            "object name length {n} is out of range — file is corrupt or not a SWMM .out"
        )));
    }
    let pos = f.stream_position()?;
    if pos + n as u64 > limit {
        return Err(Error::Format(
            "object names run past the end of the file".into(),
        ));
    }
    let mut buf = vec![0u8; n as usize];
    f.read_exact(&mut buf)?;
    // SWMM writes whatever bytes the .inp held; don't fail a whole model over
    // one non-UTF-8 name.
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Parse the header, object names, and closing block.
pub fn read_metadata(path: &Path) -> Result<OutputMetadata> {
    let size = std::fs::metadata(path)?.len();
    if size < 52 {
        return Err(Error::Format(format!(
            "{} is {size} bytes — too small to be a SWMM .out file",
            path.display()
        )));
    }
    let mut f = File::open(path)?;

    // Closing block.
    f.seek(SeekFrom::Start(size - 24))?;
    let id_offset = read_i32(&mut f)?;
    let _input_offset = read_i32(&mut f)?;
    let output_offset = read_i32(&mut f)?;
    let n_periods = read_i32(&mut f)?;
    let error_code = read_i32(&mut f)?;
    let magic_end = read_i32(&mut f)?;
    if magic_end != MAGIC {
        return Err(Error::Format(format!(
            "closing magic is {magic_end}, expected {MAGIC} — the run did not finish writing \
             this file, or it is not a SWMM .out"
        )));
    }

    // Opening block.
    f.seek(SeekFrom::Start(0))?;
    let magic_start = read_i32(&mut f)?;
    if magic_start != MAGIC {
        return Err(Error::Format(format!(
            "opening magic is {magic_start}, expected {MAGIC}"
        )));
    }
    let version = read_i32(&mut f)?;
    let flow_units_code = read_i32(&mut f)?;
    let n_subcatch = read_i32(&mut f)?;
    let n_nodes = read_i32(&mut f)?;
    let n_links = read_i32(&mut f)?;
    let n_pollutants = read_i32(&mut f)?;

    for (label, n) in [
        ("subcatchments", n_subcatch),
        ("nodes", n_nodes),
        ("links", n_links),
        ("pollutants", n_pollutants),
    ] {
        if !(0..=MAX_OBJECTS).contains(&n) {
            return Err(Error::Format(format!("{label} count {n} is out of range")));
        }
    }
    if n_periods < 0 {
        return Err(Error::Format(format!("period count {n_periods} is negative")));
    }
    if id_offset < 28 || id_offset as u64 >= size {
        return Err(Error::Format(format!(
            "ID block offset {id_offset} lies outside the file"
        )));
    }
    if output_offset < 12 || output_offset as u64 > size {
        return Err(Error::Format(format!(
            "results offset {output_offset} lies outside the file"
        )));
    }

    f.seek(SeekFrom::Start(id_offset as u64))?;
    let read_ids = |count: i32, f: &mut File| -> Result<Vec<String>> {
        (0..count).map(|_| read_id(f, size)).collect()
    };
    let subcatch_ids = read_ids(n_subcatch, &mut f)?;
    let node_ids = read_ids(n_nodes, &mut f)?;
    let link_ids = read_ids(n_links, &mut f)?;
    let pollutant_ids = read_ids(n_pollutants, &mut f)?;

    // Start date and report step sit immediately before the results section.
    f.seek(SeekFrom::Start(output_offset as u64 - 12))?;
    let start_days = read_f64(&mut f)?;
    let report_step_s = read_i32(&mut f)?;

    let meta = OutputMetadata {
        version,
        flow_units: FlowUnits::from_code(flow_units_code),
        n_subcatch: n_subcatch as usize,
        n_nodes: n_nodes as usize,
        n_links: n_links as usize,
        n_pollutants: n_pollutants as usize,
        n_periods: n_periods as usize,
        report_step_s,
        start_days,
        subcatch_ids,
        node_ids,
        link_ids,
        pollutant_ids,
        error_code,
        output_offset: output_offset as u64,
    };

    // The results section must actually be present. Without this check a
    // truncated file reads as a valid model whose series are silently short.
    let needed = meta.output_offset + meta.bytes_per_period() * meta.n_periods as u64 + 24;
    if size < needed {
        return Err(Error::Format(format!(
            "file is {size} bytes but its header describes {needed} — results are truncated"
        )));
    }

    Ok(meta)
}

/// Parse the header of a `.out` that may still be being written: no
/// closing block is needed. Returns the metadata with `n_periods` set to
/// the number of whole reporting records the file holds right now, and
/// that count again. `error_code` is 0 because the engine has not said.
///
/// The offsets are derived by walking the header block by block, exactly
/// as `output.c` writes it, rather than trusting the closing block. On a
/// finished file the result agrees with [`read_metadata`]; the tests hold
/// the two to that.
pub fn read_metadata_partial(path: &Path) -> Result<(OutputMetadata, usize)> {
    let size = std::fs::metadata(path)?.len();
    if size < 28 {
        return Err(Error::Format(format!(
            "{} is {size} bytes — the engine has not written its header yet",
            path.display()
        )));
    }
    let mut f = File::open(path)?;
    let magic_start = read_i32(&mut f)?;
    if magic_start != MAGIC {
        return Err(Error::Format(format!(
            "opening magic is {magic_start}, expected {MAGIC}"
        )));
    }
    let version = read_i32(&mut f)?;
    let flow_units_code = read_i32(&mut f)?;
    let n_subcatch = read_i32(&mut f)?;
    let n_nodes = read_i32(&mut f)?;
    let n_links = read_i32(&mut f)?;
    let n_pollutants = read_i32(&mut f)?;
    for (label, n) in [
        ("subcatchments", n_subcatch),
        ("nodes", n_nodes),
        ("links", n_links),
        ("pollutants", n_pollutants),
    ] {
        if !(0..=MAX_OBJECTS).contains(&n) {
            return Err(Error::Format(format!("{label} count {n} is out of range")));
        }
    }
    let incomplete = || Error::Format("the engine has not finished writing the header".into());
    let read_ids = |count: i32, f: &mut File| -> Result<Vec<String>> {
        (0..count).map(|_| read_id(f, size)).collect()
    };
    let subcatch_ids = read_ids(n_subcatch, &mut f).map_err(|_| incomplete())?;
    let node_ids = read_ids(n_nodes, &mut f).map_err(|_| incomplete())?;
    let link_ids = read_ids(n_links, &mut f).map_err(|_| incomplete())?;
    let pollutant_ids = read_ids(n_pollutants, &mut f).map_err(|_| incomplete())?;

    // Pollutant unit codes, then the three input-property blocks: a count,
    // that many codes, then one row per object. Each row is 4 bytes per
    // property (the type code is an i32, the rest f32).
    let mut pos = f.stream_position()? + 4 * n_pollutants as u64;
    for n_objects in [n_subcatch, n_nodes, n_links] {
        f.seek(SeekFrom::Start(pos))?;
        let n_props = read_i32(&mut f).map_err(|_| incomplete())?;
        if !(0..=64).contains(&n_props) {
            return Err(Error::Format(format!("input property count {n_props} is out of range")));
        }
        pos += 4 + 4 * n_props as u64 + 4 * n_props as u64 * n_objects as u64;
    }
    // Reporting-variable selections: four (count, codes…) blocks.
    for _ in 0..4 {
        f.seek(SeekFrom::Start(pos))?;
        let n_vars = read_i32(&mut f).map_err(|_| incomplete())?;
        if !(0..=4096).contains(&n_vars) {
            return Err(Error::Format(format!("variable count {n_vars} is out of range")));
        }
        pos += 4 + 4 * n_vars as u64;
    }
    // Start date and report step; the results follow.
    if pos + 12 > size {
        return Err(incomplete());
    }
    f.seek(SeekFrom::Start(pos))?;
    let start_days = read_f64(&mut f)?;
    let report_step_s = read_i32(&mut f)?;
    let output_offset = pos + 12;

    let mut meta = OutputMetadata {
        version,
        flow_units: FlowUnits::from_code(flow_units_code),
        n_subcatch: n_subcatch as usize,
        n_nodes: n_nodes as usize,
        n_links: n_links as usize,
        n_pollutants: n_pollutants as usize,
        n_periods: 0,
        report_step_s,
        start_days,
        subcatch_ids,
        node_ids,
        link_ids,
        pollutant_ids,
        error_code: 0,
        output_offset,
    };
    // Whole records only: the engine may be part-way through writing one,
    // and a finished file's closing block is shorter than a record.
    let available = (size.saturating_sub(output_offset) / meta.bytes_per_period()) as usize;
    meta.n_periods = available;
    Ok((meta, available))
}

/// A single reported variable over the whole simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    /// Seconds from simulation start.
    pub times_s: Vec<f64>,
    pub values: Vec<f64>,
}

impl Series {
    /// Largest value and the time it occurs — the number a design review wants.
    pub fn peak(&self) -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64)> = None;
        for (t, v) in self.times_s.iter().zip(&self.values) {
            let better = match best {
                None => true,
                Some((_, best_v)) => *v > best_v,
            };
            if better {
                best = Some((*t, *v));
            }
        }
        best.map(|(t, v)| (v, t))
    }
}

/// Read one float per reporting period from a fixed offset within each record.
fn read_series(path: &Path, meta: &OutputMetadata, in_period_offset: u64) -> Result<Series> {
    let mut f = File::open(path)?;
    let stride = meta.bytes_per_period();
    let mut values = Vec::with_capacity(meta.n_periods);
    let mut times_s = Vec::with_capacity(meta.n_periods);
    let mut b = [0u8; 4];
    for p in 0..meta.n_periods {
        f.seek(SeekFrom::Start(
            meta.output_offset + p as u64 * stride + in_period_offset,
        ))?;
        f.read_exact(&mut b)?;
        values.push(f32::from_le_bytes(b) as f64);
        times_s.push(meta.period_seconds(p));
    }
    Ok(Series { times_s, values })
}

fn index_of(ids: &[String], name: &str, kind: &str) -> Result<usize> {
    ids.iter().position(|id| id == name).ok_or_else(|| {
        Error::NotFound(format!(
            "{kind} {name:?} is not in this output file ({} {kind}s reported)",
            ids.len()
        ))
    })
}

/// Time series for a node variable. `variable` may be a [`NodeVariable`] or a
/// raw index, where `6 + i` is pollutant `i`.
pub fn node_series(path: &Path, meta: &OutputMetadata, node: &str, variable: usize) -> Result<Series> {
    let idx = index_of(&meta.node_ids, node, "node")?;
    if variable >= meta.n_node_vars() {
        return Err(Error::NotFound(format!(
            "node variable {variable} is out of range (0..{})",
            meta.n_node_vars() - 1
        )));
    }
    let offset = 8 + 4
        * (meta.n_subcatch * meta.n_subcatch_vars() + idx * meta.n_node_vars() + variable) as u64;
    read_series(path, meta, offset)
}

/// Time series for a link variable. `5 + i` is pollutant `i`.
pub fn link_series(path: &Path, meta: &OutputMetadata, link: &str, variable: usize) -> Result<Series> {
    let idx = index_of(&meta.link_ids, link, "link")?;
    if variable >= meta.n_link_vars() {
        return Err(Error::NotFound(format!(
            "link variable {variable} is out of range (0..{})",
            meta.n_link_vars() - 1
        )));
    }
    let offset = 8 + 4
        * (meta.n_subcatch * meta.n_subcatch_vars()
            + meta.n_nodes * meta.n_node_vars()
            + idx * meta.n_link_vars()
            + variable) as u64;
    read_series(path, meta, offset)
}

/// Time series for a subcatchment variable. `variable` may be a
/// [`SubcatchVariable`] or a raw index, where `8 + i` is pollutant `i`.
pub fn subcatch_series(
    path: &Path,
    meta: &OutputMetadata,
    subcatch: &str,
    variable: usize,
) -> Result<Series> {
    let idx = index_of(&meta.subcatch_ids, subcatch, "subcatchment")?;
    if variable >= meta.n_subcatch_vars() {
        return Err(Error::NotFound(format!(
            "subcatchment variable {variable} is out of range (0..{})",
            meta.n_subcatch_vars() - 1
        )));
    }
    // The subcatchment block leads the record, so there is nothing before it
    // but the timestamp.
    let offset = 8 + 4 * (idx * meta.n_subcatch_vars() + variable) as u64;
    read_series(path, meta, offset)
}

/// Peak values for one node across the whole simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct NodePeak {
    pub id: String,
    pub max_depth: f64,
    /// Seconds from start at which the depth peaked.
    pub depth_at_s: f64,
    pub max_total_inflow: f64,
    pub max_flooding: f64,
}

impl NodePeak {
    /// SWMM reports flooding as an overflow rate, so anything above zero means
    /// this node put water out of the system at some point in the run.
    pub fn flooded(&self) -> bool {
        self.max_flooding > 0.0
    }
}

/// Peak values for one link across the whole simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkPeak {
    pub id: String,
    pub max_flow: f64,
    /// Seconds from start at which the flow peaked.
    pub flow_at_s: f64,
    pub max_velocity: f64,
    /// Fraction of the barrel in use; 1.0 means it ran full.
    pub max_capacity: f64,
}

/// Peak values for one subcatchment across the whole simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct SubcatchPeak {
    pub id: String,
    pub max_runoff: f64,
    /// Seconds from start at which the runoff peaked.
    pub runoff_at_s: f64,
    pub max_rainfall: f64,
}

/// Read one float out of a period record, tolerating a short record rather
/// than panicking on a file that slipped past the header checks.
fn f32_at(record: &[u8], offset: usize) -> f64 {
    if offset + 4 > record.len() {
        return 0.0;
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&record[offset..offset + 4]);
    f32::from_le_bytes(bytes) as f64
}

/// Hand every reporting period to `visit`, reading each record once into a
/// reusable buffer.
///
/// The per-object readers above seek once per reporting period. Calling one
/// for every object to build a whole-model summary would therefore seek
/// `objects × periods` times; this reads the results section straight through
/// instead, which is one sequential pass however wide the model is.
fn for_each_period(
    path: &Path,
    meta: &OutputMetadata,
    mut visit: impl FnMut(usize, &[u8]),
) -> Result<()> {
    let mut f = File::open(path)?;
    let mut record = vec![0u8; meta.bytes_per_period() as usize];
    f.seek(SeekFrom::Start(meta.output_offset))?;
    for period in 0..meta.n_periods {
        f.read_exact(&mut record)?;
        visit(period, &record);
    }
    Ok(())
}

/// Replace the "nothing seen yet" sentinel with zero, so a run with no
/// reporting periods reads as zeros rather than negative infinity.
fn settle(value: &mut f64) {
    if !value.is_finite() {
        *value = 0.0;
    }
}

/// Peak runoff and rainfall for every subcatchment, in one pass.
///
/// The subcatchment block leads each record, so unlike the node and link
/// passes there is nothing to skip ahead of it but the timestamp.
pub fn subcatch_peaks(path: &Path, meta: &OutputMetadata) -> Result<Vec<SubcatchPeak>> {
    let sub_vars = meta.n_subcatch_vars();
    let mut peaks: Vec<SubcatchPeak> = meta
        .subcatch_ids
        .iter()
        .map(|id| SubcatchPeak {
            id: id.clone(),
            max_runoff: f64::NEG_INFINITY,
            runoff_at_s: 0.0,
            max_rainfall: f64::NEG_INFINITY,
        })
        .collect();

    for_each_period(path, meta, |period, record| {
        let t = meta.period_seconds(period);
        for (i, peak) in peaks.iter_mut().enumerate() {
            let base = 8 + 4 * (i * sub_vars);
            let runoff = f32_at(record, base + 4 * SubcatchVariable::Runoff as usize);
            if runoff > peak.max_runoff {
                peak.max_runoff = runoff;
                peak.runoff_at_s = t;
            }
            let rainfall = f32_at(record, base + 4 * SubcatchVariable::Rainfall as usize);
            if rainfall > peak.max_rainfall {
                peak.max_rainfall = rainfall;
            }
        }
    })?;

    for peak in &mut peaks {
        settle(&mut peak.max_runoff);
        settle(&mut peak.max_rainfall);
    }
    Ok(peaks)
}

/// Peak depth, total inflow and flooding for every node, in one pass.
pub fn node_peaks(path: &Path, meta: &OutputMetadata) -> Result<Vec<NodePeak>> {
    let subcatch_block = meta.n_subcatch * meta.n_subcatch_vars();
    let node_vars = meta.n_node_vars();
    let mut peaks: Vec<NodePeak> = meta
        .node_ids
        .iter()
        .map(|id| NodePeak {
            id: id.clone(),
            max_depth: f64::NEG_INFINITY,
            depth_at_s: 0.0,
            max_total_inflow: f64::NEG_INFINITY,
            max_flooding: f64::NEG_INFINITY,
        })
        .collect();

    for_each_period(path, meta, |period, record| {
        let t = meta.period_seconds(period);
        for (i, peak) in peaks.iter_mut().enumerate() {
            let base = 8 + 4 * (subcatch_block + i * node_vars);
            let depth = f32_at(record, base + 4 * NodeVariable::Depth as usize);
            if depth > peak.max_depth {
                peak.max_depth = depth;
                peak.depth_at_s = t;
            }
            let inflow = f32_at(record, base + 4 * NodeVariable::TotalInflow as usize);
            if inflow > peak.max_total_inflow {
                peak.max_total_inflow = inflow;
            }
            let flooding = f32_at(record, base + 4 * NodeVariable::Flooding as usize);
            if flooding > peak.max_flooding {
                peak.max_flooding = flooding;
            }
        }
    })?;

    for peak in &mut peaks {
        settle(&mut peak.max_depth);
        settle(&mut peak.max_total_inflow);
        settle(&mut peak.max_flooding);
    }
    Ok(peaks)
}

/// Peak flow, velocity and capacity for every link, in one pass.
pub fn link_peaks(path: &Path, meta: &OutputMetadata) -> Result<Vec<LinkPeak>> {
    let before_links =
        meta.n_subcatch * meta.n_subcatch_vars() + meta.n_nodes * meta.n_node_vars();
    let link_vars = meta.n_link_vars();
    let mut peaks: Vec<LinkPeak> = meta
        .link_ids
        .iter()
        .map(|id| LinkPeak {
            id: id.clone(),
            max_flow: f64::NEG_INFINITY,
            flow_at_s: 0.0,
            max_velocity: f64::NEG_INFINITY,
            max_capacity: f64::NEG_INFINITY,
        })
        .collect();

    for_each_period(path, meta, |period, record| {
        let t = meta.period_seconds(period);
        for (i, peak) in peaks.iter_mut().enumerate() {
            let base = 8 + 4 * (before_links + i * link_vars);
            let flow = f32_at(record, base + 4 * LinkVariable::Flow as usize);
            // Flow is signed: a reversal is not a peak, so compare magnitudes
            // while keeping the value that actually occurred.
            if flow.abs() > peak.max_flow.abs() || !peak.max_flow.is_finite() {
                peak.max_flow = flow;
                peak.flow_at_s = t;
            }
            let velocity = f32_at(record, base + 4 * LinkVariable::Velocity as usize);
            if velocity.abs() > peak.max_velocity.abs() || !peak.max_velocity.is_finite() {
                peak.max_velocity = velocity;
            }
            let capacity = f32_at(record, base + 4 * LinkVariable::Capacity as usize);
            if capacity > peak.max_capacity {
                peak.max_capacity = capacity;
            }
        }
    })?;

    for peak in &mut peaks {
        settle(&mut peak.max_flow);
        settle(&mut peak.max_velocity);
        settle(&mut peak.max_capacity);
    }
    Ok(peaks)
}

/// Read the record's leading timestamp, tolerating a short record for the same
/// reason [`f32_at`] does.
fn f64_at(record: &[u8], offset: usize) -> f64 {
    if offset + 8 > record.len() {
        return 0.0;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&record[offset..offset + 8]);
    f64::from_le_bytes(bytes)
}

/// One subcatchment's state at a single reporting period.
///
/// A subset of the eight reported variables: the ones a map or a hyetograph
/// draws. The rest are reachable through [`subcatch_series`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SubcatchState {
    pub rainfall: f64,
    pub infiltration: f64,
    pub runoff: f64,
}

/// One node's state at a single reporting period.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeState {
    pub depth: f64,
    pub head: f64,
    pub total_inflow: f64,
    pub flooding: f64,
}

impl NodeState {
    /// Flooding is reported as an overflow rate, so anything above zero means
    /// this node is spilling *at this instant* — unlike [`NodePeak::flooded`],
    /// which answers whether it ever did.
    pub fn flooding_now(&self) -> bool {
        self.flooding > 0.0
    }
}

/// One link's state at a single reporting period.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinkState {
    pub flow: f64,
    pub depth: f64,
    pub velocity: f64,
    /// Fraction of the barrel in use; 1.0 means it is running full.
    pub capacity: f64,
}

/// The whole model at one reporting period.
///
/// The per-object readers give one object across all time, and the peak readers
/// give every object's maximum across all time. This is the third cut — every
/// object at one instant — which is what an animated map needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub period: usize,
    /// Seconds from simulation start.
    pub time_s: f64,
    /// The record's own timestamp, in days since 1899-12-30.
    pub date_days: f64,
    /// Indexed to match [`OutputMetadata::subcatch_ids`].
    pub subcatchments: Vec<SubcatchState>,
    /// Indexed to match [`OutputMetadata::node_ids`].
    pub nodes: Vec<NodeState>,
    /// Indexed to match [`OutputMetadata::link_ids`].
    pub links: Vec<LinkState>,
}

/// Read a single reporting period.
///
/// This seeks straight to the record instead of walking the results section,
/// because a time slider jumps around: scrubbing must not cost a full pass over
/// the file per frame. The offset arithmetic is deliberately the same as
/// [`node_peaks`] and [`link_peaks`] use, so the three readers cannot drift
/// apart on the layout.
pub fn read_frame(path: &Path, meta: &OutputMetadata, period: usize) -> Result<Frame> {
    if period >= meta.n_periods {
        return Err(Error::NotFound(format!(
            "reporting period {period} is out of range (this run has {})",
            meta.n_periods
        )));
    }

    let stride = meta.bytes_per_period();
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(meta.output_offset + period as u64 * stride))?;
    let mut record = vec![0u8; stride as usize];
    f.read_exact(&mut record)?;

    let sub_vars = meta.n_subcatch_vars();
    let subcatch_block = meta.n_subcatch * sub_vars;
    let node_vars = meta.n_node_vars();
    let link_vars = meta.n_link_vars();
    let before_links = subcatch_block + meta.n_nodes * node_vars;

    let nodes = (0..meta.n_nodes)
        .map(|i| {
            let base = 8 + 4 * (subcatch_block + i * node_vars);
            NodeState {
                depth: f32_at(&record, base + 4 * NodeVariable::Depth as usize),
                head: f32_at(&record, base + 4 * NodeVariable::Head as usize),
                total_inflow: f32_at(&record, base + 4 * NodeVariable::TotalInflow as usize),
                flooding: f32_at(&record, base + 4 * NodeVariable::Flooding as usize),
            }
        })
        .collect();

    let links = (0..meta.n_links)
        .map(|i| {
            let base = 8 + 4 * (before_links + i * link_vars);
            LinkState {
                flow: f32_at(&record, base + 4 * LinkVariable::Flow as usize),
                depth: f32_at(&record, base + 4 * LinkVariable::Depth as usize),
                velocity: f32_at(&record, base + 4 * LinkVariable::Velocity as usize),
                capacity: f32_at(&record, base + 4 * LinkVariable::Capacity as usize),
            }
        })
        .collect();

    // The subcatchment block leads each record, ahead of the nodes.
    let subcatchments = (0..meta.n_subcatch)
        .map(|i| {
            let base = 8 + 4 * (i * sub_vars);
            SubcatchState {
                rainfall: f32_at(&record, base + 4 * SubcatchVariable::Rainfall as usize),
                infiltration: f32_at(&record, base + 4 * SubcatchVariable::Infiltration as usize),
                runoff: f32_at(&record, base + 4 * SubcatchVariable::Runoff as usize),
            }
        })
        .collect();

    Ok(Frame {
        period,
        time_s: meta.period_seconds(period),
        date_days: f64_at(&record, 0),
        subcatchments,
        nodes,
        links,
    })
}

/// Everything needed to open a result set: the file and its parsed header.
#[derive(Clone, Debug)]
pub struct OutputFile {
    pub path: PathBuf,
    pub meta: OutputMetadata,
    /// Opened with [`OutputFile::open_partial`]: the run had not finished
    /// (or was stopped), `meta.n_periods` is the count of records present
    /// at open time, and the file may since have grown.
    pub partial: bool,
}

impl OutputFile {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let meta = read_metadata(&path)?;
        Ok(Self { path, meta, partial: false })
    }

    /// Open a `.out` the engine is still writing, or left without its
    /// closing block. Every reader on this type then sees the records that
    /// exist; re-open to pick up more.
    pub fn open_partial(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let (meta, _) = read_metadata_partial(&path)?;
        Ok(Self { path, meta, partial: true })
    }

    /// Reporting periods on disk now, for a partial file (cheap: one
    /// `metadata` call).
    pub fn periods_available(&self) -> usize {
        if !self.partial {
            return self.meta.n_periods;
        }
        std::fs::metadata(&self.path)
            .map(|m| (m.len().saturating_sub(self.meta.output_offset) / self.meta.bytes_per_period()) as usize)
            .unwrap_or(self.meta.n_periods)
    }

    pub fn node(&self, node: &str, variable: NodeVariable) -> Result<Series> {
        node_series(&self.path, &self.meta, node, variable as usize)
    }

    pub fn link(&self, link: &str, variable: LinkVariable) -> Result<Series> {
        link_series(&self.path, &self.meta, link, variable as usize)
    }

    pub fn subcatchment(&self, subcatch: &str, variable: SubcatchVariable) -> Result<Series> {
        subcatch_series(&self.path, &self.meta, subcatch, variable as usize)
    }

    /// Every object's state at one reporting period.
    pub fn frame(&self, period: usize) -> Result<Frame> {
        read_frame(&self.path, &self.meta, period)
    }
}

/// Civil date from a count of days since 1970-01-01. Howard Hinnant's
/// `civil_from_days`, which is exact for the whole proleptic Gregorian range.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 1899-12-30 (SWMM's epoch) expressed in days before the Unix epoch.
const SWMM_EPOCH_UNIX_DAYS: i64 = -25_569;

/// Calendar date and time for a SWMM date value, as
/// `(year, month, day, hour, minute, second)`.
pub fn decode_datetime(days: f64) -> (i64, u32, u32, u32, u32, u32) {
    let whole = days.floor();
    let (y, m, d) = civil_from_days(whole as i64 + SWMM_EPOCH_UNIX_DAYS);
    // Round to the nearest second before splitting: SWMM stores times as
    // fractions of a day, so 08:36:27 arrives as 0.3586458333… and truncating
    // shows the second before.
    let secs = ((days - whole) * 86_400.0).round() as u32;
    let secs = secs.min(86_399);
    (y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// Display names of the node variables, in file order: the six fixed ones
/// followed by one per pollutant. The index into this list is the `variable`
/// argument of [`node_series`] and [`RawFrame::node`].
pub fn node_variable_names(meta: &OutputMetadata) -> Vec<String> {
    let mut names: Vec<String> = [
        "Depth",
        "Head",
        "Volume",
        "Lateral inflow",
        "Total inflow",
        "Flooding",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    names.extend(meta.pollutant_ids.iter().cloned());
    names
}

/// Display names of the link variables, in file order (see
/// [`node_variable_names`]).
pub fn link_variable_names(meta: &OutputMetadata) -> Vec<String> {
    let mut names: Vec<String> = ["Flow", "Depth", "Velocity", "Volume", "Capacity"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    names.extend(meta.pollutant_ids.iter().cloned());
    names
}

/// Every reported variable of every object at one reporting period.
///
/// [`Frame`] keeps the four node and four link quantities the map's fixed
/// colouring needs. A variable picker needs all of them, pollutants included,
/// so this holds the record's floats verbatim and indexes into them.
#[derive(Clone, Debug, PartialEq)]
pub struct RawFrame {
    pub period: usize,
    /// Seconds from simulation start.
    pub time_s: f64,
    /// The record's own timestamp, in days since 1899-12-30.
    pub date_days: f64,
    pub n_subcatch_vars: usize,
    pub n_node_vars: usize,
    pub n_link_vars: usize,
    /// `n_subcatch × n_subcatch_vars`, subcatchment-major.
    pub subcatchments: Vec<f64>,
    /// `n_nodes × n_node_vars`, node-major, indexed to [`OutputMetadata::node_ids`].
    pub nodes: Vec<f64>,
    /// `n_links × n_link_vars`, link-major, indexed to [`OutputMetadata::link_ids`].
    pub links: Vec<f64>,
    /// The fifteen system variables.
    pub system: Vec<f64>,
}

impl RawFrame {
    /// Variable `var` of node `i`, or `None` when either index is out of range.
    pub fn node(&self, i: usize, var: usize) -> Option<f64> {
        if var >= self.n_node_vars {
            return None;
        }
        self.nodes.get(i * self.n_node_vars + var).copied()
    }

    /// Variable `var` of link `i`.
    pub fn link(&self, i: usize, var: usize) -> Option<f64> {
        if var >= self.n_link_vars {
            return None;
        }
        self.links.get(i * self.n_link_vars + var).copied()
    }

    /// Variable `var` of subcatchment `i`.
    pub fn subcatchment(&self, i: usize, var: usize) -> Option<f64> {
        if var >= self.n_subcatch_vars {
            return None;
        }
        self.subcatchments
            .get(i * self.n_subcatch_vars + var)
            .copied()
    }
}

/// Read every variable of every object at one reporting period.
///
/// Same seek arithmetic as [`read_frame`]; the record is decoded whole rather
/// than picked over, so a picker can switch variables without another read.
pub fn read_frame_raw(path: &Path, meta: &OutputMetadata, period: usize) -> Result<RawFrame> {
    if period >= meta.n_periods {
        return Err(Error::NotFound(format!(
            "reporting period {period} is out of range (this run has {})",
            meta.n_periods
        )));
    }
    let stride = meta.bytes_per_period();
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(meta.output_offset + period as u64 * stride))?;
    let mut record = vec![0u8; stride as usize];
    f.read_exact(&mut record)?;

    let n_subcatch_vars = meta.n_subcatch_vars();
    let n_node_vars = meta.n_node_vars();
    let n_link_vars = meta.n_link_vars();
    let n_sub = meta.n_subcatch * n_subcatch_vars;
    let n_node = meta.n_nodes * n_node_vars;
    let n_link = meta.n_links * n_link_vars;
    let total = n_sub + n_node + n_link + N_SYS_VARS;
    let floats: Vec<f64> = (0..total).map(|i| f32_at(&record, 8 + 4 * i)).collect();

    Ok(RawFrame {
        period,
        time_s: meta.period_seconds(period),
        date_days: f64_at(&record, 0),
        n_subcatch_vars,
        n_node_vars,
        n_link_vars,
        subcatchments: floats[..n_sub].to_vec(),
        nodes: floats[n_sub..n_sub + n_node].to_vec(),
        links: floats[n_sub + n_node..n_sub + n_node + n_link].to_vec(),
        system: floats[n_sub + n_node + n_link..].to_vec(),
    })
}

/// `2026-05-27 08:36:27`, for report headers and axis labels.
pub fn format_datetime(days: f64) -> String {
    let (y, mo, d, h, mi, s) = decode_datetime(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Day zero of SWMM's epoch must be 1899-12-30, and the offset to the Unix
    /// epoch must land on 1970-01-01. Every timestamp depends on these two.
    #[test]
    fn epoch_is_1899_12_30() {
        assert_eq!(decode_datetime(0.0), (1899, 12, 30, 0, 0, 0));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(decode_datetime(25_569.0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn decodes_time_of_day() {
        // 1900-01-01 12:00:00 is two days and a half into the epoch.
        assert_eq!(decode_datetime(2.5), (1900, 1, 1, 12, 0, 0));
        assert_eq!(format_datetime(2.5), "1900-01-01 12:00:00");
        // A leap day, to catch an off-by-one in the era arithmetic.
        let (y, m, d, ..) = decode_datetime(43_890.0);
        assert_eq!((y, m, d), (2020, 2, 29));
    }

    /// Write a synthetic `.out` in the documented layout so tests can read it
    /// back — the only coverage that runs on CI, where no SWMM install exists.
    ///
    /// `name` must be unique per test. These tests share one scratch directory
    /// and `File::create` truncates, so two tests writing the same filename
    /// race: one empties the file while the other is mid-read, and the reader
    /// fails with an unexpected EOF. Keying the name on `n_periods` alone did
    /// exactly that — two tests both asked for 12 periods — and it surfaced
    /// only once unrelated new tests changed the scheduling.
    fn synthetic_out(dir: &Path, name: &str, n_periods: usize) -> PathBuf {
        synthetic_out_full(dir, name, n_periods, 0)
    }

    /// As `synthetic_out`, but with `n_pol` pollutants, so the
    /// `+ n_pollutants` term in every block width is actually exercised.
    /// Until this existed every fixture had zero pollutants and that
    /// arithmetic was never tested against anything — while a real EPA model
    /// carries one.
    fn synthetic_out_full(dir: &Path, name: &str, n_periods: usize, n_pol: usize) -> PathBuf {
        let (n_sub, n_node, n_link) = (1usize, 2usize, 1usize);
        let n_sub_vars = N_SUBCATCH_VARS_BASE + n_pol;
        let n_node_vars = N_NODE_VARS_BASE + n_pol;
        let n_link_vars = N_LINK_VARS_BASE + n_pol;

        let mut buf: Vec<u8> = Vec::new();
        let push_i32 = |b: &mut Vec<u8>, v: i32| b.extend_from_slice(&v.to_le_bytes());
        let push_f32 = |b: &mut Vec<u8>, v: f32| b.extend_from_slice(&v.to_le_bytes());
        let push_f64 = |b: &mut Vec<u8>, v: f64| b.extend_from_slice(&v.to_le_bytes());
        let push_id = |b: &mut Vec<u8>, s: &str| {
            b.extend_from_slice(&(s.len() as i32).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        };

        push_i32(&mut buf, MAGIC);
        push_i32(&mut buf, 52_004);
        push_i32(&mut buf, 3); // CMS
        push_i32(&mut buf, n_sub as i32);
        push_i32(&mut buf, n_node as i32);
        push_i32(&mut buf, n_link as i32);
        push_i32(&mut buf, n_pol as i32);

        let id_offset = buf.len() as i32;
        push_id(&mut buf, "S1");
        push_id(&mut buf, "JN_Toe");
        push_id(&mut buf, "OF1");
        push_id(&mut buf, "C1");
        for i in 0..n_pol {
            push_id(&mut buf, &format!("P{i}"));
        }

        // Pollutant concentration unit codes.
        for _ in 0..n_pol {
            push_i32(&mut buf, 0);
        }

        let input_offset = buf.len() as i32;
        // Input properties, block by block as output.c writes them: a
        // count, the property codes, then one row per object.
        push_i32(&mut buf, 1);
        push_i32(&mut buf, 1); // INPUT_AREA
        for _ in 0..n_sub {
            push_f32(&mut buf, 4.5);
        }
        push_i32(&mut buf, 3);
        for code in [0, 2, 3] {
            push_i32(&mut buf, code); // type, invert, max depth
        }
        for _ in 0..n_node {
            push_i32(&mut buf, 0);
            push_f32(&mut buf, 100.0);
            push_f32(&mut buf, 5.0);
        }
        push_i32(&mut buf, 5);
        for code in [0, 1, 1, 3, 4] {
            push_i32(&mut buf, code); // type, offset, offset, max depth, length
        }
        for _ in 0..n_link {
            push_i32(&mut buf, 0);
            for _ in 0..4 {
                push_f32(&mut buf, 1.0);
            }
        }
        // Reporting-variable selections: a count and the codes, four times.
        for n_vars in [n_sub_vars, n_node_vars, n_link_vars, N_SYS_VARS] {
            push_i32(&mut buf, n_vars as i32);
            for code in 0..n_vars {
                push_i32(&mut buf, code as i32);
            }
        }

        push_f64(&mut buf, 43_890.5); // start: 2020-02-29 12:00
        push_i32(&mut buf, 300); // 5-minute report step
        let output_offset = buf.len() as i32;

        for p in 0..n_periods {
            push_f64(&mut buf, 43_890.5 + p as f64);
            // Distinct per variable index, so a wrong offset reads a wrong
            // number rather than one zero that happens to match another.
            for s in 0..n_sub {
                for v in 0..n_sub_vars {
                    let value = match v {
                        0 => 2.0,                                        // rainfall, constant
                        3 => p as f32 * 0.25,                            // infiltration ramps
                        4 => {
                            if p == n_periods / 3 {
                                9.0
                            } else {
                                1.0
                            }
                        } // runoff spikes once
                        _ => 0.0,
                    };
                    push_f32(&mut buf, value + s as f32 * 100.0);
                }
            }
            // Node 0 depth ramps; node 1 head is constant. Node 0 total inflow
            // peaks in the middle so `peak()` has something to find.
            for n in 0..n_node {
                for v in 0..n_node_vars {
                    let val = match (n, v) {
                        (0, 0) => p as f32 * 0.5,
                        (0, 4) => if p == n_periods / 2 { 99.0 } else { 1.0 },
                        (1, 1) => 100.0,
                        _ => 0.0,
                    };
                    push_f32(&mut buf, val);
                }
            }
            for l in 0..n_link {
                for v in 0..n_link_vars {
                    push_f32(&mut buf, if (l, v) == (0, 0) { 2.5 } else { 0.0 });
                }
            }
            for _ in 0..N_SYS_VARS {
                push_f32(&mut buf, 0.0);
            }
        }

        push_i32(&mut buf, id_offset);
        push_i32(&mut buf, input_offset);
        push_i32(&mut buf, output_offset);
        push_i32(&mut buf, n_periods as i32);
        push_i32(&mut buf, 0);
        push_i32(&mut buf, MAGIC);

        let path = dir.join(format!("synthetic-{name}-{n_periods}.out"));
        File::create(&path).unwrap().write_all(&buf).unwrap();
        path
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-swmm-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_synthetic_file() {
        let path = synthetic_out(&scratch(), "reads", 12);
        let f = OutputFile::open(&path).unwrap();

        assert_eq!(f.meta.flow_units, FlowUnits::Cms);
        assert!(f.meta.flow_units.is_metric());
        assert_eq!(f.meta.n_periods, 12);
        assert_eq!(f.meta.report_step_s, 300);
        assert_eq!(f.meta.node_ids, vec!["JN_Toe", "OF1"]);
        assert_eq!(f.meta.link_ids, vec!["C1"]);
        assert_eq!(f.meta.subcatch_ids, vec!["S1"]);
        assert_eq!(f.meta.error_code, 0);
        assert_eq!(f.meta.period_seconds(0), 300.0);
        assert_eq!(f.meta.duration_seconds(), 3600.0);
        assert_eq!(format_datetime(f.meta.start_days), "2020-02-29 12:00:00");

        let depth = f.node("JN_Toe", NodeVariable::Depth).unwrap();
        assert_eq!(depth.values.len(), 12);
        assert_eq!(depth.values[0], 0.0);
        assert_eq!(depth.values[4], 2.0);
        assert_eq!(depth.times_s[4], 1500.0);

        let head = f.node("OF1", NodeVariable::Head).unwrap();
        assert!(head.values.iter().all(|v| *v == 100.0));

        let flow = f.link("C1", LinkVariable::Flow).unwrap();
        assert!(flow.values.iter().all(|v| *v == 2.5));
        assert_eq!(f.link("C1", LinkVariable::Depth).unwrap().values[0], 0.0);

        let (peak, at) = f.node("JN_Toe", NodeVariable::TotalInflow).unwrap().peak().unwrap();
        assert_eq!(peak, 99.0);
        assert_eq!(at, 300.0 * 7.0);
    }

    #[test]
    fn unknown_names_are_named_in_the_error() {
        let path = synthetic_out(&scratch(), "unknown-names", 3);
        let f = OutputFile::open(&path).unwrap();
        let err = f.node("NoSuchNode", NodeVariable::Depth).unwrap_err().to_string();
        assert!(err.contains("NoSuchNode"), "{err}");
        assert!(err.contains("2 nodes"), "{err}");
    }

    /// The bulk readers must agree with the per-series readers that were
    /// validated against real files — they walk the same bytes a different
    /// way, so any disagreement means one of them has the layout wrong.
    #[test]
    fn peaks_agree_with_the_series_readers() {
        let path = synthetic_out(&scratch(), "peaks-agree", 12);
        let f = OutputFile::open(&path).unwrap();

        let nodes = node_peaks(&f.path, &f.meta).unwrap();
        assert_eq!(nodes.len(), 2);

        let toe = nodes.iter().find(|n| n.id == "JN_Toe").unwrap();
        // Depth ramps p * 0.5, so the last of 12 periods is the peak.
        assert_eq!(toe.max_depth, 5.5);
        assert_eq!(toe.depth_at_s, 3600.0);
        assert_eq!(toe.max_total_inflow, 99.0);
        assert_eq!(toe.max_flooding, 0.0);
        assert!(!toe.flooded(), "no flooding was written");

        // The same numbers the validated per-series path produces.
        let series = f.node("JN_Toe", NodeVariable::Depth).unwrap();
        let (peak, at) = series.peak().unwrap();
        assert_eq!((toe.max_depth, toe.depth_at_s), (peak, at));

        let outfall = nodes.iter().find(|n| n.id == "OF1").unwrap();
        assert_eq!(outfall.max_depth, 0.0);
        assert_eq!(outfall.max_total_inflow, 0.0);

        let links = link_peaks(&f.path, &f.meta).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, "C1");
        assert_eq!(links[0].max_flow, 2.5);
        // Flow is constant, so the first period holds the peak.
        assert_eq!(links[0].flow_at_s, 300.0);
        assert_eq!(links[0].max_velocity, 0.0);
        assert_eq!(links[0].max_capacity, 0.0);
    }

    /// A single-period run must not leave the "nothing seen yet" sentinels in
    /// the output.
    #[test]
    fn peaks_are_finite_on_a_one_period_run() {
        let path = synthetic_out(&scratch(), "peaks-finite", 1);
        let f = OutputFile::open(&path).unwrap();
        for node in node_peaks(&f.path, &f.meta).unwrap() {
            assert!(node.max_depth.is_finite(), "{}", node.id);
            assert!(node.max_total_inflow.is_finite(), "{}", node.id);
            assert!(node.max_flooding.is_finite(), "{}", node.id);
        }
        for link in link_peaks(&f.path, &f.meta).unwrap() {
            assert!(link.max_flow.is_finite(), "{}", link.id);
            assert!(link.max_velocity.is_finite(), "{}", link.id);
            assert!(link.max_capacity.is_finite(), "{}", link.id);
        }
    }

    /// A frame must agree with the per-series readers at the same instant.
    /// They walk the same bytes a different way, so any disagreement means one
    /// of them has the layout wrong — the same discipline as the peaks tests.
    #[test]
    fn frames_agree_with_the_series_readers() {
        let path = synthetic_out(&scratch(), "frames", 12);
        let f = OutputFile::open(&path).unwrap();

        let depth = f.node("JN_Toe", NodeVariable::Depth).unwrap();
        let inflow = f.node("JN_Toe", NodeVariable::TotalInflow).unwrap();
        let head = f.node("OF1", NodeVariable::Head).unwrap();
        let flow = f.link("C1", LinkVariable::Flow).unwrap();

        for p in 0..f.meta.n_periods {
            let frame = f.frame(p).unwrap();
            assert_eq!(frame.period, p);
            assert_eq!(frame.time_s, depth.times_s[p], "time at period {p}");
            assert_eq!(frame.nodes.len(), 2);
            assert_eq!(frame.links.len(), 1);
            assert_eq!(frame.nodes[0].depth, depth.values[p], "depth at period {p}");
            assert_eq!(
                frame.nodes[0].total_inflow, inflow.values[p],
                "inflow at period {p}"
            );
            assert_eq!(frame.nodes[1].head, head.values[p], "head at period {p}");
            assert_eq!(frame.links[0].flow, flow.values[p], "flow at period {p}");
        }
    }

    /// The one-pass peak reader and the per-variable series reader walk the
    /// same bytes by different routes, so they are checked against each other
    /// rather than against a number written into the test.
    #[test]
    fn subcatchment_peaks_agree_with_the_series() {
        let path = synthetic_out(&scratch(), "sub-peaks", 12);
        let f = OutputFile::open(&path).unwrap();
        let peaks = subcatch_peaks(&f.path, &f.meta).unwrap();
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0].id, "S1");

        let runoff = f.subcatchment("S1", SubcatchVariable::Runoff).unwrap();
        let (value, at_s) = runoff.peak().unwrap();
        assert_eq!(peaks[0].max_runoff, value);
        assert_eq!(peaks[0].runoff_at_s, at_s);
        assert_eq!(peaks[0].max_rainfall, 2.0);
    }

    /// The subcatchment block leads each record. Checked against the series
    /// reader at every period rather than assumed — two paths over the same
    /// bytes, which is what has caught every layout error in this reader.
    #[test]
    fn frames_carry_subcatchment_state() {
        let path = synthetic_out(&scratch(), "sub-frames", 12);
        let f = OutputFile::open(&path).unwrap();
        assert_eq!(f.meta.subcatch_ids, vec!["S1"]);

        let rain = f.subcatchment("S1", SubcatchVariable::Rainfall).unwrap();
        let infil = f.subcatchment("S1", SubcatchVariable::Infiltration).unwrap();
        let runoff = f.subcatchment("S1", SubcatchVariable::Runoff).unwrap();

        for p in 0..f.meta.n_periods {
            let s = f.frame(p).unwrap().subcatchments[0];
            assert_eq!(s.rainfall, rain.values[p], "rainfall at period {p}");
            assert_eq!(s.infiltration, infil.values[p], "infiltration at period {p}");
            assert_eq!(s.runoff, runoff.values[p], "runoff at period {p}");
        }
        assert_eq!(runoff.peak().unwrap().0, 9.0, "the writer spikes runoff once");
    }

    /// The index confirmed against EPA's own report. The named variable and a
    /// raw index 4 must read the same bytes, so the enum cannot drift from
    /// what the file actually holds.
    #[test]
    fn subcatchment_runoff_is_variable_four() {
        assert_eq!(SubcatchVariable::Runoff as usize, 4);
        let path = synthetic_out(&scratch(), "sub-index", 9);
        let f = OutputFile::open(&path).unwrap();
        let named = f.subcatchment("S1", SubcatchVariable::Runoff).unwrap();
        let raw = subcatch_series(&f.path, &f.meta, "S1", 4).unwrap();
        assert_eq!(named, raw);
    }

    /// A pollutant widens every block. No fixture had one before this, so the
    /// `+ n_pollutants` arithmetic went untested while the real EPA model that
    /// prompted it carries exactly one.
    #[test]
    fn a_pollutant_widens_every_block() {
        let path = synthetic_out_full(&scratch(), "sub-pollutant", 10, 1);
        let f = OutputFile::open(&path).unwrap();
        assert_eq!(f.meta.n_pollutants, 1);
        assert_eq!(f.meta.n_subcatch_vars(), 9);
        assert_eq!(f.meta.n_node_vars(), 7);
        assert_eq!(f.meta.n_link_vars(), 6);

        // Every block still decodes despite the wider stride.
        let depth = f.node("JN_Toe", NodeVariable::Depth).unwrap();
        assert_eq!(depth.values[4], 2.0);
        let flow = f.link("C1", LinkVariable::Flow).unwrap();
        assert!(flow.values.iter().all(|v| *v == 2.5));
        let runoff = f.subcatchment("S1", SubcatchVariable::Runoff).unwrap();
        assert_eq!(runoff.peak().unwrap().0, 9.0);

        let frame = f.frame(4).unwrap();
        assert_eq!(frame.subcatchments[0].runoff, runoff.values[4]);
        assert_eq!(frame.nodes[0].depth, depth.values[4]);
        assert_eq!(frame.links[0].flow, flow.values[4]);
    }

    #[test]
    fn an_unknown_subcatchment_is_named_in_the_error() {
        let path = synthetic_out(&scratch(), "sub-unknown", 3);
        let f = OutputFile::open(&path).unwrap();
        let err = f
            .subcatchment("NoSuch", SubcatchVariable::Runoff)
            .unwrap_err()
            .to_string();
        assert!(err.contains("NoSuch"), "{err}");
        assert!(err.contains("subcatchment"), "{err}");
    }

    #[test]
    fn a_subcatchment_variable_out_of_range_is_an_error() {
        let path = synthetic_out(&scratch(), "sub-range", 3);
        let f = OutputFile::open(&path).unwrap();
        // Eight base variables and no pollutants, so 8 is one past the end.
        let err = subcatch_series(&f.path, &f.meta, "S1", 8)
            .unwrap_err()
            .to_string();
        assert!(err.contains("out of range"), "{err}");
    }

    /// Each record carries its own timestamp; the writer stamps 43890.5 + p.
    #[test]
    fn a_frame_carries_the_records_own_date() {
        let path = synthetic_out(&scratch(), "frame-date", 4);
        let f = OutputFile::open(&path).unwrap();
        for p in 0..4 {
            assert_eq!(f.frame(p).unwrap().date_days, 43_890.5 + p as f64);
        }
    }

    #[test]
    fn a_period_past_the_end_is_an_error() {
        let path = synthetic_out(&scratch(), "frame-range", 3);
        let f = OutputFile::open(&path).unwrap();
        assert!(f.frame(2).is_ok(), "the last period is valid");

        let err = f.frame(3).unwrap_err().to_string();
        assert!(err.contains("out of range"), "{err}");
        assert!(
            err.contains('3'),
            "the error should say how long the run is: {err}"
        );
    }

    /// Flooding is an overflow rate. The synthetic file writes none, so no node
    /// may report spilling at any instant.
    #[test]
    fn a_frame_reports_no_flooding_when_none_was_written() {
        let path = synthetic_out(&scratch(), "frame-flood", 5);
        let f = OutputFile::open(&path).unwrap();
        for p in 0..5 {
            for node in f.frame(p).unwrap().nodes {
                assert!(!node.flooding_now(), "period {p}");
            }
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let path = synthetic_out(&scratch(), "bad-magic", 2);
        let mut bytes = std::fs::read(&path).unwrap();
        let n = bytes.len();
        bytes[n - 4..].copy_from_slice(&0i32.to_le_bytes());
        let bad = scratch().join("bad-magic.out");
        std::fs::write(&bad, &bytes).unwrap();
        let err = OutputFile::open(&bad).unwrap_err().to_string();
        assert!(err.contains("closing magic"), "{err}");
    }

    /// A run killed mid-write leaves a header promising more periods than the
    /// file holds. That has to be an error, not short series.
    #[test]
    fn rejects_truncated_results() {
        let path = synthetic_out(&scratch(), "truncated", 10);
        let bytes = std::fs::read(&path).unwrap();
        let mut cut = bytes[..bytes.len() - 24 - 200].to_vec();
        cut.extend_from_slice(&bytes[bytes.len() - 24..]);
        let trunc = scratch().join("truncated.out");
        std::fs::write(&trunc, &cut).unwrap();
        let err = OutputFile::open(&trunc).unwrap_err().to_string();
        assert!(err.contains("truncated"), "{err}");
    }

    /// The forward walk of the header must land exactly where the closing
    /// block says the results start, on a finished file — with and without
    /// pollutants, which widen the header's variable blocks.
    #[test]
    fn partial_read_agrees_with_the_closing_block() {
        for (name, n_pol) in [("partial-plain", 0), ("partial-pol", 2)] {
            let path = synthetic_out_full(&scratch(), name, 7, n_pol);
            let full = read_metadata(&path).unwrap();
            let (part, n) = read_metadata_partial(&path).unwrap();
            assert_eq!(n, 7, "{name}");
            assert_eq!(part.output_offset, full.output_offset, "{name}");
            assert_eq!(part.n_periods, full.n_periods, "{name}");
            assert_eq!(part.node_ids, full.node_ids);
            assert_eq!(part.link_ids, full.link_ids);
            assert_eq!(part.pollutant_ids, full.pollutant_ids);
            assert_eq!(part.start_days, full.start_days);
            assert_eq!(part.report_step_s, full.report_step_s);
            assert_eq!(part.flow_units, full.flow_units);
            let f = OutputFile::open_partial(&path).unwrap();
            assert!(f.partial);
            assert_eq!(f.periods_available(), 7);
            assert_eq!(f.frame(6).unwrap(), OutputFile::open(&path).unwrap().frame(6).unwrap());
        }
    }

    /// The forward walk on bytes EPA's engine wrote, not the synthetic
    /// writer's idea of them.
    #[test]
    fn partial_read_walks_a_real_engine_file() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/results/Detention_Pond_Model.out");
        let full = read_metadata(&path).unwrap();
        let (part, n) = read_metadata_partial(&path).unwrap();
        assert_eq!(part.output_offset, full.output_offset);
        assert_eq!(n, full.n_periods);
        assert_eq!(part.node_ids, full.node_ids);
        assert_eq!(part.report_step_s, full.report_step_s);
        assert_eq!(part.start_days, full.start_days);
    }

    /// A file cut mid-run — no closing block, a record half written —
    /// reads as the whole records it holds, and grows as more arrive.
    #[test]
    fn partial_read_counts_whole_records_in_a_growing_file() {
        let path = synthetic_out(&scratch(), "partial-grow", 10);
        let bytes = std::fs::read(&path).unwrap();
        let full = read_metadata(&path).unwrap();
        let stride = full.bytes_per_period() as usize;
        let start = full.output_offset as usize;
        let live = scratch().join("partial-live.out");

        // Header only: nothing to show yet, but not an error.
        std::fs::write(&live, &bytes[..start]).unwrap();
        let (m, n) = read_metadata_partial(&live).unwrap();
        assert_eq!(n, 0);
        assert_eq!(m.n_periods, 0);
        assert!(OutputFile::open(&live).is_err(), "the strict reader still refuses it");

        // Three records and half of a fourth.
        std::fs::write(&live, &bytes[..start + 3 * stride + stride / 2]).unwrap();
        let f = OutputFile::open_partial(&live).unwrap();
        assert_eq!(f.meta.n_periods, 3);
        let frame = f.frame(2).unwrap();
        assert_eq!(frame.nodes[0].depth, 1.0, "period 2 depth ramps 0.5/period");
        assert!(f.frame(3).is_err());
        assert_eq!(f.node("JN_Toe", NodeVariable::Depth).unwrap().values.len(), 3);
        assert_eq!(node_peaks(&f.path, &f.meta).unwrap()[0].max_depth, 1.0);

        // The file grows under the open handle; the cheap count sees it.
        std::fs::write(&live, &bytes[..start + 8 * stride]).unwrap();
        assert_eq!(f.periods_available(), 8);
        assert_eq!(f.meta.n_periods, 3, "the parsed header is a snapshot");
        let (_, n) = read_metadata_partial(&live).unwrap();
        assert_eq!(n, 8);

        // A header cut short is reported as such rather than as garbage.
        std::fs::write(&live, &bytes[..40]).unwrap();
        let err = read_metadata_partial(&live).unwrap_err().to_string();
        assert!(err.contains("not finished writing"), "{err}");
    }

    #[test]
    fn rejects_too_small() {
        let tiny = scratch().join("tiny.out");
        std::fs::write(&tiny, [0u8; 8]).unwrap();
        let err = OutputFile::open(&tiny).unwrap_err().to_string();
        assert!(err.contains("too small"), "{err}");
    }

    /// Opt-in check against the real PCSWMM-generated fixtures, which cannot
    /// live in this repo. Set STORMSEWER_SWMM_FIXTURES to a directory of
    /// `.out` files to exercise the reader on engine-written bytes.
    #[test]
    fn reads_real_fixtures_when_available() {
        let Ok(dir) = std::env::var("STORMSEWER_SWMM_FIXTURES") else {
            eprintln!("skipped: STORMSEWER_SWMM_FIXTURES not set");
            return;
        };
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("fixture dir") {
            let p = entry.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) != Some("out") {
                continue;
            }
            let f = OutputFile::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            assert!(f.meta.n_periods > 0, "{}: no reporting periods", p.display());
            assert_eq!(
                f.meta.node_ids.len(),
                f.meta.n_nodes,
                "{}: node name count disagrees with the header",
                p.display()
            );
            if let Some(node) = f.meta.node_ids.first() {
                let s = f.node(node, NodeVariable::Depth).unwrap();
                assert_eq!(s.values.len(), f.meta.n_periods);
                assert!(
                    s.values.iter().all(|v| v.is_finite()),
                    "{}: non-finite depth at {node}",
                    p.display()
                );
                // The one-pass reader must reach the same peak on real bytes.
                let peaks = node_peaks(&f.path, &f.meta).unwrap();
                assert_eq!(peaks.len(), f.meta.n_nodes);
                let first = peaks.iter().find(|n| &n.id == node).unwrap();
                assert_eq!(
                    first.max_depth,
                    s.peak().unwrap().0,
                    "{}: bulk and series peaks disagree at {node}",
                    p.display()
                );
            }
            checked += 1;
        }
        assert!(checked > 0, "no .out files in {dir}");
        eprintln!("checked {checked} real .out fixtures");
    }
}
