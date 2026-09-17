// SPDX-License-Identifier: GPL-3.0-or-later

//! Reader for the EPA SWMM 5 binary output (`.out`) file.
//!
//! Layout, little-endian throughout:
//!
//! ```text
//! 0    magic (516114522), version, flow-unit code,
//!      n_subcatch, n_nodes, n_links, n_pollutants        7 × i32
//! ...  per-object property blocks
//! id_offset       object IDs: each is i32 length + that many UTF-8 bytes,
//!                 in order subcatchments, nodes, links, pollutants
//! ...  reporting-variable selections
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

/// Everything needed to open a result set: the file and its parsed header.
#[derive(Clone, Debug)]
pub struct OutputFile {
    pub path: PathBuf,
    pub meta: OutputMetadata,
}

impl OutputFile {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let meta = read_metadata(&path)?;
        Ok(Self { path, meta })
    }

    pub fn node(&self, node: &str, variable: NodeVariable) -> Result<Series> {
        node_series(&self.path, &self.meta, node, variable as usize)
    }

    pub fn link(&self, link: &str, variable: LinkVariable) -> Result<Series> {
        link_series(&self.path, &self.meta, link, variable as usize)
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

    /// Build a `.out` in the documented layout, then read it back. This is the
    /// only test that can run on CI, where no SWMM install exists.
    fn synthetic_out(dir: &Path, n_periods: usize) -> PathBuf {
        let (n_sub, n_node, n_link, n_pol) = (1usize, 2usize, 1usize, 0usize);
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

        let input_offset = buf.len() as i32;
        push_f64(&mut buf, 43_890.5); // start: 2020-02-29 12:00
        push_i32(&mut buf, 300); // 5-minute report step
        let output_offset = buf.len() as i32;

        for p in 0..n_periods {
            push_f64(&mut buf, 43_890.5 + p as f64);
            for _ in 0..n_sub * n_sub_vars {
                push_f32(&mut buf, 0.0);
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

        let path = dir.join(format!("synthetic-{n_periods}.out"));
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
        let path = synthetic_out(&scratch(), 12);
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
        let path = synthetic_out(&scratch(), 3);
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
        let path = synthetic_out(&scratch(), 12);
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
        let path = synthetic_out(&scratch(), 1);
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

    #[test]
    fn rejects_bad_magic() {
        let path = synthetic_out(&scratch(), 2);
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
        let path = synthetic_out(&scratch(), 10);
        let bytes = std::fs::read(&path).unwrap();
        let mut cut = bytes[..bytes.len() - 24 - 200].to_vec();
        cut.extend_from_slice(&bytes[bytes.len() - 24..]);
        let trunc = scratch().join("truncated.out");
        std::fs::write(&trunc, &cut).unwrap();
        let err = OutputFile::open(&trunc).unwrap_err().to_string();
        assert!(err.contains("truncated"), "{err}");
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
