// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolving a [`Config`] against a model: loading and resampling the DEM,
//! building the roughness grid, locating nodes and bank lines on the grid,
//! and turning the model's `[TIMESERIES]` / `[RAINGAGES]` into the
//! time-varying inputs the solver samples.

use std::path::Path;

use crate::doc::InpDoc;
use crate::gis::raster::Raster;
use crate::{Error, Result};

use super::{
    BankInterface, Config, InterfaceKind, LocatedBank, LocatedNode, NodeInterface, RainOnGrid,
    Roughness, Setup,
};

/// `v > 0`, false for NaN (spelled out so the intent survives clippy).
fn positive(v: f64) -> bool {
    v > 0.0
}

/// Diameter of the default manhole lid used by a `Manhole` interface that
/// gives no dimensions: 0.6 m (metric) or 2 ft (US).
pub fn default_lid_diameter(metric: bool) -> f64 {
    if metric {
        0.6
    } else {
        2.0
    }
}

/// A time series in seconds from the model start, values as given.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TimeSeries {
    pub t: Vec<f64>,
    pub v: Vec<f64>,
}

impl TimeSeries {
    /// Piecewise-constant lookup: the value of the last point at or before
    /// `t`, held for `hold_s` seconds (or forever when `hold_s` is None);
    /// 0 before the first point.
    pub fn step_at(&self, t: f64, hold_s: Option<f64>) -> f64 {
        if self.t.is_empty() || t < self.t[0] {
            return 0.0;
        }
        let i = match self.t.binary_search_by(|x| x.partial_cmp(&t).unwrap()) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        match hold_s {
            Some(h) if t >= self.t[i] + h => 0.0,
            _ => self.v[i],
        }
    }

    /// Linear interpolation, as the engine does for inflow series; 0
    /// outside the series.
    pub fn linear_at(&self, t: f64) -> f64 {
        let n = self.t.len();
        if n == 0 || t < self.t[0] || t > self.t[n - 1] {
            return 0.0;
        }
        if n == 1 {
            return self.v[0];
        }
        let i = match self.t.binary_search_by(|x| x.partial_cmp(&t).unwrap()) {
            Ok(i) => return self.v[i],
            Err(i) => i,
        };
        let (t0, t1) = (self.t[i - 1], self.t[i]);
        let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
        self.v[i - 1] + f * (self.v[i] - self.v[i - 1])
    }

    /// Volume under the curve between `a` and `b`, treating the series as
    /// linear (trapezoids).
    pub fn integral_linear(&self, a: f64, b: f64) -> f64 {
        let n = self.t.len();
        if n < 2 || b <= a {
            return 0.0;
        }
        let mut total = 0.0;
        for i in 1..n {
            let (t0, t1) = (self.t[i - 1], self.t[i]);
            let lo = t0.max(a);
            let hi = t1.min(b);
            if hi <= lo {
                continue;
            }
            total += 0.5 * (self.linear_at(lo) + self.linear_at(hi)) * (hi - lo);
        }
        total
    }
}

/// Rain-on-grid as the solver samples it: a rate in depth/s in the DEM's
/// length units, piecewise constant.
#[derive(Clone, Debug, PartialEq)]
pub enum RainInput {
    None,
    Constant(f64),
    /// Series of rates, each held for `interval_s`.
    Series { series: TimeSeries, interval_s: f64 },
}

impl RainInput {
    pub fn rate_at(&self, t: f64) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Constant(r) => *r,
            Self::Series { series, interval_s } => series.step_at(t, Some(*interval_s)),
        }
    }
}

/// A point source resolved onto a cell.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatedSource {
    pub name: String,
    pub col: usize,
    pub row: usize,
    /// Flow in the DEM's cubic units per second.
    pub series: TimeSeries,
}

/// An outfall that can take water from the surface at a stage.
#[derive(Clone, Debug, PartialEq)]
pub struct OutfallStage {
    /// Index into `Setup::nodes`.
    pub node: usize,
    /// Fixed stage, or the invert for a free outfall.
    pub stage: f64,
}

/// The time-varying and unit inputs resolved from the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    /// Model start as days since 1899-12-30 (the engine's epoch).
    pub start_days: f64,
    /// `END_DATE/END_TIME - START_DATE/START_TIME`, seconds.
    pub model_duration_s: f64,
    pub rain: RainInput,
    pub sources: Vec<LocatedSource>,
    pub outfalls: Vec<OutfallStage>,
    /// Multiply a flow in the model's flow units to get DEM cubic units/s.
    pub flow_to_cubic: f64,
    /// Multiply a rate in the model's rain units (in/hr or mm/hr) to get
    /// DEM length units per second.
    pub rain_to_rate: f64,
    /// Infiltration as a rate in DEM length units per second.
    pub infiltration: InfiltrationInput,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InfiltrationInput {
    None,
    Constant(f64),
    /// `f0`, `fc` in length/s, `k` in 1/s.
    Horton { f0: f64, fc: f64, k: f64 },
}

impl Inputs {
    /// Inputs for a model-less run (tests): no rain, no sources, unit
    /// factors of one.
    pub fn empty() -> Self {
        Self {
            start_days: 0.0,
            model_duration_s: 0.0,
            rain: RainInput::None,
            sources: Vec::new(),
            outfalls: Vec::new(),
            flow_to_cubic: 1.0,
            rain_to_rate: 1.0,
            infiltration: InfiltrationInput::None,
        }
    }
}

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

/// Whether the model's `FLOW_UNITS` is metric.
pub fn is_metric(doc: &InpDoc) -> bool {
    matches!(
        doc.option("FLOW_UNITS")
            .map(|s| s.to_ascii_uppercase())
            .as_deref(),
        Some("CMS") | Some("LPS") | Some("MLD")
    )
}

/// Factor from the model's flow units to cubic length units per second.
pub fn flow_to_cubic(doc: &InpDoc) -> f64 {
    match doc
        .option("FLOW_UNITS")
        .map(|s| s.to_ascii_uppercase())
        .as_deref()
    {
        Some("GPM") => 1.0 / 448.831_169,
        Some("MGD") => 1.547_229_2,
        Some("LPS") => 1e-3,
        Some("MLD") => 1e6 / 1e3 / 86_400.0,
        _ => 1.0, // CFS, CMS
    }
}

/// Factor from in/hr (US) or mm/hr (metric) to length units per second.
pub fn rain_to_rate(metric: bool) -> f64 {
    if metric {
        1e-3 / 3600.0
    } else {
        1.0 / 12.0 / 3600.0
    }
}

// ---------------------------------------------------------------------------
// Dates and times
// ---------------------------------------------------------------------------

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// 1899-12-30 expressed in days before the Unix epoch.
const SWMM_EPOCH_UNIX_DAYS: i64 = -25_569;

/// `MM/DD/YYYY` (or `M/D/YY`, or with `-`) → days since 1899-12-30.
pub fn parse_date_days(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(['/', '-']).collect();
    if parts.len() != 3 {
        return None;
    }
    let (m, d, mut y): (u32, u32, i64) =
        (parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?);
    if y < 100 {
        y += if y < 70 { 2000 } else { 1900 };
    }
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((days_from_civil(y, m, d) - SWMM_EPOCH_UNIX_DAYS) as f64)
}

/// `H:MM`, `H:MM:SS` or decimal hours → seconds.
pub fn parse_time_seconds(s: &str) -> Option<f64> {
    if s.contains(':') {
        let mut parts = s.split(':');
        let h: f64 = parts.next()?.trim().parse().ok()?;
        let m: f64 = parts.next().map(|p| p.trim().parse().ok()).unwrap_or(Some(0.0))?;
        let sec: f64 = parts.next().map(|p| p.trim().parse().ok()).unwrap_or(Some(0.0))?;
        Some(h * 3600.0 + m * 60.0 + sec)
    } else {
        s.trim().parse::<f64>().ok().map(|h| h * 3600.0)
    }
}

/// Model start as days since 1899-12-30, and the model duration in
/// seconds. Missing dates default to the engine's own defaults (today is
/// not knowable here, so 01/01/2000 stands in; only differences matter).
pub fn model_clock(doc: &InpDoc) -> (f64, f64) {
    let date = |key: &str| doc.option(key).and_then(parse_date_days);
    let time = |key: &str| doc.option(key).and_then(parse_time_seconds);
    let default_day = parse_date_days("01/01/2000").unwrap_or(0.0);
    let start_days = date("START_DATE").unwrap_or(default_day);
    let start = start_days * 86_400.0 + time("START_TIME").unwrap_or(0.0);
    let end = date("END_DATE").unwrap_or(start_days) * 86_400.0 + time("END_TIME").unwrap_or(0.0);
    (start_days + time("START_TIME").unwrap_or(0.0) / 86_400.0, (end - start).max(0.0))
}

/// A `[TIMESERIES]` as seconds from the model start. Dated points are
/// placed on the model calendar; undated ones are relative to the start.
pub fn timeseries(doc: &InpDoc, name: &str, start_days: f64) -> Result<TimeSeries> {
    let points = doc.timeseries(name);
    if points.is_empty() {
        return Err(Error::NotFound(format!(
            "[TIMESERIES] {name} is missing or is a FILE series (not supported on the 2D grid)"
        )));
    }
    let mut t = Vec::with_capacity(points.len());
    let mut v = Vec::with_capacity(points.len());
    for p in &points {
        let secs = parse_time_seconds(&p.time)
            .ok_or_else(|| Error::Format(format!("[TIMESERIES] {name}: bad time {:?}", p.time)))?;
        let value: f64 = p
            .value
            .parse()
            .map_err(|_| Error::Format(format!("[TIMESERIES] {name}: bad value {:?}", p.value)))?;
        let at = match &p.date {
            Some(d) => {
                let days = parse_date_days(d)
                    .ok_or_else(|| Error::Format(format!("[TIMESERIES] {name}: bad date {d:?}")))?;
                (days - start_days) * 86_400.0 + secs
            }
            None => secs,
        };
        t.push(at);
        v.push(value);
    }
    // Keep the series sorted; the engine requires it too, but be forgiving.
    let mut idx: Vec<usize> = (0..t.len()).collect();
    idx.sort_by(|a, b| t[*a].partial_cmp(&t[*b]).unwrap());
    Ok(TimeSeries {
        t: idx.iter().map(|&i| t[i]).collect(),
        v: idx.iter().map(|&i| v[i]).collect(),
    })
}

/// A rain gage's series as intensities (in/hr or mm/hr) held for the gage
/// interval, from its `[RAINGAGES]` row (`INTENSITY`, `VOLUME` or
/// `CUMULATIVE` format, `SCF` applied).
pub fn gage_rain(doc: &InpDoc, gage: &str, start_days: f64) -> Result<(TimeSeries, f64)> {
    let (_, row) = doc
        .find("RAINGAGES", gage)
        .ok_or_else(|| Error::NotFound(format!("[RAINGAGES] has no gage {gage}")))?;
    let format = row.value(1).unwrap_or("INTENSITY").to_ascii_uppercase();
    let interval_s = row
        .value(2)
        .and_then(parse_time_seconds)
        .filter(|s| *s > 0.0)
        .ok_or_else(|| Error::Format(format!("[RAINGAGES] {gage}: bad interval")))?;
    let scf: f64 = row.value(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let source = row.value(4).unwrap_or("").to_ascii_uppercase();
    if source != "TIMESERIES" {
        return Err(Error::Format(format!(
            "[RAINGAGES] {gage} reads a FILE; rain-on-grid needs a TIMESERIES gage"
        )));
    }
    let series_name = row
        .value(5)
        .ok_or_else(|| Error::Format(format!("[RAINGAGES] {gage} names no series")))?;
    let raw = timeseries(doc, series_name, start_days)?;
    let hours = interval_s / 3600.0;
    let mut v = Vec::with_capacity(raw.v.len());
    let mut prev = 0.0;
    for &x in &raw.v {
        let rate = match format.as_str() {
            "VOLUME" => x / hours,
            "CUMULATIVE" => {
                let r = (x - prev).max(0.0) / hours;
                prev = x;
                r
            }
            _ => x,
        };
        v.push(rate * scf);
    }
    Ok((TimeSeries { t: raw.t, v }, interval_s))
}

// ---------------------------------------------------------------------------
// Rasters
// ---------------------------------------------------------------------------

/// Read a DEM or attribute raster by extension. GeoTIFF goes through the
/// GIS reader when it exists.
pub fn read_raster(path: &Path) -> Result<Raster> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "tif" | "tiff" => read_geotiff(path),
        _ => Raster::read_asc(path),
    }
}

fn read_geotiff(path: &Path) -> Result<Raster> {
    Ok(crate::gis::geotiff::read(path)?.raster)
}

/// Crop a raster to a world window (cells whose centre lies inside).
pub fn crop(r: &Raster, window: (f64, f64, f64, f64)) -> Result<Raster> {
    let (xmin, ymin, xmax, ymax) = window;
    let c0 = (((xmin - r.x0) / r.cell).floor().max(0.0)) as usize;
    let c1 = (((xmax - r.x0) / r.cell).ceil().min(r.ncols as f64)) as usize;
    let r0 = (((r.y1() - ymax) / r.cell).floor().max(0.0)) as usize;
    let r1 = (((r.y1() - ymin) / r.cell).ceil().min(r.nrows as f64)) as usize;
    if c1 <= c0 || r1 <= r0 {
        return Err(Error::Format(format!(
            "the 2D window ({xmin}, {ymin}, {xmax}, {ymax}) does not overlap the DEM ({:?})",
            r.bounds()
        )));
    }
    let ncols = c1 - c0;
    let nrows = r1 - r0;
    let mut data = Vec::with_capacity(ncols * nrows);
    for row in r0..r1 {
        data.extend_from_slice(&r.data[row * r.ncols + c0..row * r.ncols + c1]);
    }
    Ok(Raster {
        ncols,
        nrows,
        x0: r.x0 + c0 as f64 * r.cell,
        y0: r.y1() - r1 as f64 * r.cell,
        cell: r.cell,
        nodata: r.nodata,
        data,
    })
}

/// Resample onto a grid of `cell` covering the same extent (bilinear for
/// continuous fields, nearest for classes).
pub fn resample(r: &Raster, cell: f64, bilinear: bool) -> Result<Raster> {
    if !positive(cell) {
        return Err(Error::Format("2D cell size must be positive".into()));
    }
    if (cell - r.cell).abs() < 1e-9 * r.cell {
        return Ok(r.clone());
    }
    let ncols = ((r.x1() - r.x0) / cell).round().max(1.0) as usize;
    let nrows = ((r.y1() - r.y0) / cell).round().max(1.0) as usize;
    let mut out = Raster::filled(ncols, nrows, r.x0, r.y0, cell, f64::NAN);
    out.nodata = r.nodata;
    for row in 0..nrows {
        for col in 0..ncols {
            let (x, y) = out.center(col, row);
            let v = if bilinear {
                r.sample_bilinear(x, y)
            } else {
                r.sample(x, y)
            };
            out.data[row * ncols + col] = v.unwrap_or(f64::NAN);
        }
    }
    Ok(out)
}

/// Sample `src` onto `target`'s grid (nearest cell).
pub fn onto_grid(src: &Raster, target: &Raster) -> Raster {
    let mut out = Raster::filled(
        target.ncols,
        target.nrows,
        target.x0,
        target.y0,
        target.cell,
        f64::NAN,
    );
    for row in 0..target.nrows {
        for col in 0..target.ncols {
            let (x, y) = target.center(col, row);
            out.data[row * target.ncols + col] = src.sample(x, y).unwrap_or(f64::NAN);
        }
    }
    out
}

/// Build the Manning grid for a DEM.
pub fn manning_grid(dem: &Raster, roughness: &Roughness, base: &Path) -> Result<(Raster, Vec<String>)> {
    let mut warnings = Vec::new();
    let resolve = |p: &Path| if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    let grid = match roughness {
        Roughness::Uniform(n) => {
            if !positive(*n) {
                return Err(Error::Format("Manning's n must be positive".into()));
            }
            Raster::filled(dem.ncols, dem.nrows, dem.x0, dem.y0, dem.cell, *n)
        }
        Roughness::Grid(path) => {
            let src = read_raster(&resolve(path))?;
            let mut g = onto_grid(&src, dem);
            let mut missing = 0usize;
            for (i, v) in g.data.iter_mut().enumerate() {
                if v.is_nan() || *v <= 0.0 {
                    if !dem.data[i].is_nan() {
                        missing += 1;
                    }
                    *v = 0.05;
                }
            }
            if missing > 0 {
                warnings.push(format!(
                    "roughness grid has no value at {missing} DEM cells; n = 0.05 used there"
                ));
            }
            g
        }
        Roughness::Classes { raster, table } => {
            let src = read_raster(&resolve(raster))?;
            let classes = onto_grid(&src, dem);
            let mut g = Raster::filled(dem.ncols, dem.nrows, dem.x0, dem.y0, dem.cell, 0.05);
            let mut unknown = 0usize;
            for (i, c) in classes.data.iter().enumerate() {
                if c.is_nan() {
                    continue;
                }
                let key = c.round() as i64;
                match table.iter().find(|(k, _)| *k == key) {
                    Some((_, n)) if *n > 0.0 => g.data[i] = *n,
                    _ => unknown += 1,
                }
            }
            if unknown > 0 {
                warnings.push(format!(
                    "{unknown} cells have a land-cover class not in the table; n = 0.05 used"
                ));
            }
            g
        }
    };
    Ok((grid, warnings))
}

// ---------------------------------------------------------------------------
// Lines on the grid
// ---------------------------------------------------------------------------

/// Clip a world segment to the raster bounds (Liang–Barsky); None when it
/// lies wholly outside.
fn clip_segment(r: &Raster, a: (f64, f64), b: (f64, f64)) -> Option<((f64, f64), (f64, f64))> {
    let (xmin, ymin, xmax, ymax) = r.bounds();
    let eps = r.cell * 1e-6;
    let (xmax, ymax) = (xmax - eps, ymax - eps);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for (p, q) in [
        (-dx, a.0 - xmin),
        (dx, xmax - a.0),
        (-dy, a.1 - ymin),
        (dy, ymax - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some((
        (a.0 + t0 * dx, a.1 + t0 * dy),
        (a.0 + t1 * dx, a.1 + t1 * dy),
    ))
}

/// Cells under a polyline, in order, by Bresenham's line algorithm between
/// the cells of consecutive vertices (segments clipped to the grid).
pub fn rasterise_polyline(r: &Raster, points: &[(f64, f64)]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let push = |c: (usize, usize), out: &mut Vec<(usize, usize)>| {
        if out.last() != Some(&c) && !out.contains(&c) {
            out.push(c);
        }
    };
    if points.len() == 1 {
        if let Some(c) = r.cell_at(points[0].0, points[0].1) {
            push(c, &mut out);
        }
        return out;
    }
    for w in points.windows(2) {
        let Some((a, b)) = clip_segment(r, w[0], w[1]) else { continue };
        let (Some(ca), Some(cb)) = (r.cell_at(a.0, a.1), r.cell_at(b.0, b.1)) else {
            continue;
        };
        let (mut x0, mut y0) = (ca.0 as i64, ca.1 as i64);
        let (x1, y1) = (cb.0 as i64, cb.1 as i64);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            push((x0 as usize, y0 as usize), &mut out);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

fn node_rim(doc: &InpDoc, name: &str) -> Option<(f64, &'static str)> {
    for section in ["JUNCTIONS", "STORAGE", "OUTFALLS", "DIVIDERS"] {
        if let Some((_, row)) = doc.find(section, name) {
            let invert: f64 = row.value(1)?.parse().ok()?;
            let max_depth: f64 = match section {
                "OUTFALLS" => 0.0,
                "DIVIDERS" => row.value(3).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                _ => row.value(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
            };
            return Some((invert + max_depth, section));
        }
    }
    None
}

/// Stage an outfall holds: `FIXED` stage, else its invert.
fn outfall_stage(doc: &InpDoc, name: &str) -> Option<f64> {
    let (_, row) = doc.find("OUTFALLS", name)?;
    let invert: f64 = row.value(1)?.parse().ok()?;
    let kind = row.value(2).unwrap_or("FREE").to_ascii_uppercase();
    if kind == "FIXED" {
        row.value(3).and_then(|s| s.parse().ok()).or(Some(invert))
    } else {
        Some(invert)
    }
}

pub fn build(model: &Path, doc: &InpDoc, config: &Config) -> Result<Setup> {
    let mut warnings = Vec::new();
    let base = model.parent().unwrap_or_else(|| Path::new("."));
    let resolve = |p: &Path| if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    let dem_path = config
        .dem
        .as_ref()
        .ok_or_else(|| Error::Format("no DEM: set [GRID] DEM in the 2D sidecar".into()))?;
    let mut dem = read_raster(&resolve(dem_path))?;
    if let Some(w) = config.window {
        dem = crop(&dem, w)?;
    }
    if let Some(cell) = config.cell {
        dem = resample(&dem, cell, true)?;
    }
    if dem.ncols < 2 || dem.nrows < 2 {
        return Err(Error::Format("the 2D grid needs at least 2 x 2 cells".into()));
    }
    if !positive(config.dry_depth) || !positive(config.courant) || config.courant > 1.0 {
        return Err(Error::Format(
            "2D run settings: DRY_DEPTH must be positive and COURANT in (0, 1]".into(),
        ));
    }
    if !positive(config.output_step_s) {
        return Err(Error::Format("2D OUTPUT_STEP must be positive".into()));
    }
    let (manning, w) = manning_grid(&dem, &config.roughness, base)?;
    warnings.extend(w);
    let metric = is_metric(doc);

    // Nodes.
    let mut nodes = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for section in ["JUNCTIONS", "STORAGE", "OUTFALLS", "DIVIDERS"] {
        names.extend(doc.names(section));
    }
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    for name in &names {
        let Some((x, y)) = doc.coordinates(name) else { continue };
        let interface = config
            .nodes
            .iter()
            .find(|n| same(&n.node, name))
            .cloned()
            .unwrap_or_else(|| NodeInterface {
                node: name.clone(),
                kind: if config.sealed.iter().any(|s| same(s, name)) {
                    InterfaceKind::Sealed
                } else {
                    InterfaceKind::Manhole
                },
                weir_coeff: None,
                lid_open: false,
            });
        let Some((col, row)) = dem.cell_at(x, y) else {
            warnings.push(format!("node {name} is outside the DEM; no 2D interface"));
            continue;
        };
        let ground = dem.data[row * dem.ncols + col];
        if ground.is_nan() {
            warnings.push(format!("node {name} sits on a DEM no-data cell; no 2D interface"));
            continue;
        }
        let Some((rim, _)) = node_rim(doc, name) else {
            warnings.push(format!("node {name} has no elevation; no 2D interface"));
            continue;
        };
        nodes.push(LocatedNode {
            interface,
            col,
            row,
            ground,
            rim,
        });
    }
    for n in &config.nodes {
        if !names.iter().any(|m| same(m, &n.node)) {
            warnings.push(format!("[NODES] {}: no such node in the model", n.node));
        }
    }

    // Banks.
    let mut banks = Vec::new();
    for b in &config.banks {
        if !doc.contains("CONDUITS", &b.link) {
            warnings.push(format!("[BANKS] {}: no such conduit in the model", b.link));
            continue;
        }
        let polyline: Vec<(f64, f64)> = if b.polyline.is_empty() {
            link_polyline(doc, &b.link)
        } else {
            b.polyline.clone()
        };
        let cells: Vec<(usize, usize, f64)> = rasterise_polyline(&dem, &polyline)
            .into_iter()
            .filter_map(|(c, r)| {
                let z = dem.data[r * dem.ncols + c];
                if z.is_nan() {
                    None
                } else {
                    Some((c, r, b.crest.unwrap_or(z)))
                }
            })
            .collect();
        if cells.is_empty() {
            warnings.push(format!("[BANKS] {}: the bank line crosses no DEM cell", b.link));
            continue;
        }
        banks.push(LocatedBank {
            interface: BankInterface {
                polyline,
                ..b.clone()
            },
            cells,
        });
    }

    // Time-varying inputs.
    let (start_days, model_duration_s) = model_clock(doc);
    let rain_factor = rain_to_rate(metric);
    let rain = match &config.rain {
        RainOnGrid::None => RainInput::None,
        RainOnGrid::Constant(i) => RainInput::Constant(i * rain_factor),
        RainOnGrid::Gage(g) => {
            let (mut series, interval_s) = gage_rain(doc, g, start_days)?;
            for v in &mut series.v {
                *v *= rain_factor;
            }
            RainInput::Series { series, interval_s }
        }
    };
    let flow_factor = flow_to_cubic(doc);
    let mut sources = Vec::new();
    for s in &config.sources {
        let Some((col, row)) = dem.cell_at(s.x, s.y) else {
            warnings.push(format!("[SOURCES] {}: outside the DEM", s.name));
            continue;
        };
        if dem.data[row * dem.ncols + col].is_nan() {
            warnings.push(format!("[SOURCES] {}: on a no-data cell", s.name));
            continue;
        }
        let mut series = timeseries(doc, &s.series, start_days)?;
        for v in &mut series.v {
            *v *= flow_factor;
        }
        sources.push(LocatedSource {
            name: s.name.clone(),
            col,
            row,
            series,
        });
    }
    let outfalls = nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            outfall_stage(doc, &n.interface.node).map(|stage| OutfallStage { node: i, stage })
        })
        .collect();
    let infiltration = match config.infiltration {
        super::Infiltration::None => InfiltrationInput::None,
        super::Infiltration::Constant(f) => InfiltrationInput::Constant(f * rain_factor),
        super::Infiltration::Horton { f0, fc, k } => InfiltrationInput::Horton {
            f0: f0 * rain_factor,
            fc: fc * rain_factor,
            k: k / 3600.0,
        },
    };

    Ok(Setup {
        model: model.to_path_buf(),
        config: config.clone(),
        dem,
        manning,
        nodes,
        banks,
        metric,
        warnings,
        inputs: Inputs {
            start_days,
            model_duration_s,
            rain,
            sources,
            outfalls,
            flow_to_cubic: flow_factor,
            rain_to_rate: rain_factor,
            infiltration,
        },
    })
}

/// A conduit's centreline: from-node, vertices, to-node.
pub fn link_polyline(doc: &InpDoc, link: &str) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    let from = doc.field("CONDUITS", link, "FromNode");
    let to = doc.field("CONDUITS", link, "ToNode");
    if let Some(p) = from.and_then(|n| doc.coordinates(n)) {
        pts.push(p);
    }
    pts.extend(doc.vertices(link));
    if let Some(p) = to.and_then(|n| doc.coordinates(n)) {
        pts.push(p);
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_dates_parse() {
        assert_eq!(parse_time_seconds("1:30"), Some(5400.0));
        assert_eq!(parse_time_seconds("0:05:30"), Some(330.0));
        assert_eq!(parse_time_seconds("2.5"), Some(9000.0));
        // 1899-12-30 is day 0; 1900-01-01 is day 2.
        assert_eq!(parse_date_days("01/01/1900"), Some(2.0));
        assert_eq!(parse_date_days("1/1/2000"), Some(36526.0));
    }

    #[test]
    fn step_and_linear_lookup() {
        let s = TimeSeries {
            t: vec![0.0, 60.0, 120.0],
            v: vec![1.0, 3.0, 0.0],
        };
        assert_eq!(s.step_at(30.0, None), 1.0);
        assert_eq!(s.step_at(30.0, Some(10.0)), 0.0);
        assert_eq!(s.step_at(200.0, None), 0.0);
        assert!((s.linear_at(30.0) - 2.0).abs() < 1e-12);
        assert_eq!(s.linear_at(-1.0), 0.0);
        assert!((s.integral_linear(0.0, 120.0) - (120.0 + 90.0)).abs() < 1e-9);
    }

    #[test]
    fn bresenham_covers_a_diagonal() {
        let r = Raster::filled(10, 10, 0.0, 0.0, 1.0, 0.0);
        let cells = rasterise_polyline(&r, &[(0.5, 0.5), (9.5, 9.5)]);
        assert_eq!(cells.len(), 10);
        assert_eq!(cells[0], (0, 9));
        assert_eq!(cells[9], (9, 0));
        // A segment leaving the grid is clipped, not dropped.
        let cells = rasterise_polyline(&r, &[(5.5, 5.5), (25.0, 5.5)]);
        assert_eq!(cells.len(), 5);
    }

    #[test]
    fn crop_and_resample_keep_georeference() {
        let mut r = Raster::filled(10, 10, 100.0, 200.0, 2.0, 1.0);
        r.data[0] = 9.0; // top-left
        let c = crop(&r, (104.0, 204.0, 112.0, 216.0)).unwrap();
        assert_eq!((c.ncols, c.nrows), (4, 6));
        assert_eq!(c.x0, 104.0);
        assert_eq!(c.y0, 204.0);
        let s = resample(&r, 4.0, true).unwrap();
        assert_eq!((s.ncols, s.nrows), (5, 5));
        assert_eq!(s.x1(), r.x1());
    }
}
