// SPDX-License-Identifier: GPL-3.0-or-later

//! A single-band raster on a regular grid: the DEM the 2D solver runs on,
//! the roughness and rainfall grids it reads, and the depth grids it writes.
//!
//! The one type shared between the GIS readers (`gis::geotiff`, this file's
//! ESRI ASCII reader), the 2D engine (`twod`) and the app's overlays, so a
//! grid read by any reader can be handed to any consumer. Keep it plain: a
//! row-major `Vec<f64>` with the top row first, as every GIS format stores
//! it, and a geotransform of the "cell-registered" kind (`x0`, `y0` name the
//! lower-left corner of the lower-left cell, as ESRI ASCII's `xllcorner`).
//!
//! Only square cells. Every DEM this project will meet is square-celled, and
//! a solver on rectangular cells is a different solver.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::{Error, Result};

/// A grid of `f64` samples with a world position.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub ncols: usize,
    pub nrows: usize,
    /// World x of the left edge of the leftmost column.
    pub x0: f64,
    /// World y of the bottom edge of the bottom row.
    pub y0: f64,
    /// Cell width and height, in the raster's own units.
    pub cell: f64,
    /// The value that means "no data"; those cells are stored as NaN.
    pub nodata: Option<f64>,
    /// Row-major, top row first: `data[row * ncols + col]`.
    pub data: Vec<f64>,
}

impl Raster {
    /// A raster of `value` everywhere.
    pub fn filled(ncols: usize, nrows: usize, x0: f64, y0: f64, cell: f64, value: f64) -> Self {
        Self {
            ncols,
            nrows,
            x0,
            y0,
            cell,
            nodata: None,
            data: vec![value; ncols * nrows],
        }
    }

    pub fn get(&self, col: usize, row: usize) -> Option<f64> {
        if col < self.ncols && row < self.nrows {
            Some(self.data[row * self.ncols + col])
        } else {
            None
        }
    }

    pub fn set(&mut self, col: usize, row: usize, v: f64) {
        if col < self.ncols && row < self.nrows {
            self.data[row * self.ncols + col] = v;
        }
    }

    /// World x of the right edge.
    pub fn x1(&self) -> f64 {
        self.x0 + self.cell * self.ncols as f64
    }

    /// World y of the top edge.
    pub fn y1(&self) -> f64 {
        self.y0 + self.cell * self.nrows as f64
    }

    /// World bounds as (xmin, ymin, xmax, ymax).
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.x0, self.y0, self.x1(), self.y1())
    }

    /// The cell containing world point (x, y), or None outside the grid.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        if x < self.x0 || y < self.y0 || x >= self.x1() || y >= self.y1() {
            return None;
        }
        let col = ((x - self.x0) / self.cell).floor() as usize;
        let row = ((self.y1() - y) / self.cell).floor() as usize;
        Some((col.min(self.ncols - 1), row.min(self.nrows - 1)))
    }

    /// World centre of a cell.
    pub fn center(&self, col: usize, row: usize) -> (f64, f64) {
        (
            self.x0 + (col as f64 + 0.5) * self.cell,
            self.y1() - (row as f64 + 0.5) * self.cell,
        )
    }

    /// Nearest-cell sample; None outside the grid or on a no-data cell.
    pub fn sample(&self, x: f64, y: f64) -> Option<f64> {
        let (c, r) = self.cell_at(x, y)?;
        let v = self.data[r * self.ncols + c];
        if v.is_nan() {
            None
        } else {
            Some(v)
        }
    }

    /// Bilinear sample between cell centres; falls back to the nearest cell
    /// at the edges and beside no-data cells.
    pub fn sample_bilinear(&self, x: f64, y: f64) -> Option<f64> {
        let (c, r) = self.cell_at(x, y)?;
        let (cx, cy) = self.center(c, r);
        let fx = (x - cx) / self.cell;
        let fy = (cy - y) / self.cell; // rows go down
        let c2 = if fx >= 0.0 { c + 1 } else { c.wrapping_sub(1) };
        let r2 = if fy >= 0.0 { r + 1 } else { r.wrapping_sub(1) };
        let tx = fx.abs();
        let ty = fy.abs();
        let v00 = self.get(c, r)?;
        let v10 = self.get(c2, r).unwrap_or(v00);
        let v01 = self.get(c, r2).unwrap_or(v00);
        let v11 = self.get(c2, r2).unwrap_or(v10);
        if [v00, v10, v01, v11].iter().any(|v| v.is_nan()) {
            return if v00.is_nan() { None } else { Some(v00) };
        }
        Some((1.0 - ty) * ((1.0 - tx) * v00 + tx * v10) + ty * ((1.0 - tx) * v01 + tx * v11))
    }

    /// Hillshade by Horn's method (the ESRI/GDAL formulation): one value
    /// in 0..1 per cell, top row first, NaN where the cell or a neighbour
    /// it needs has no data. `azimuth` and `altitude` are the sun's
    /// bearing and elevation in degrees; `z_factor` converts elevation
    /// units to the cell unit (1 when they agree, 0.3048 for a DEM in
    /// feet on a metre grid).
    pub fn hillshade(&self, azimuth: f64, altitude: f64, z_factor: f64) -> Vec<f32> {
        let zenith = (90.0 - altitude).to_radians();
        let az = (360.0 - azimuth + 90.0).rem_euclid(360.0).to_radians();
        let (nc, nr) = (self.ncols, self.nrows);
        let at = |c: isize, r: isize| -> f64 {
            let c = c.clamp(0, nc as isize - 1) as usize;
            let r = r.clamp(0, nr as isize - 1) as usize;
            self.data[r * nc + c]
        };
        let mut out = vec![f32::NAN; nc * nr];
        for r in 0..nr as isize {
            for c in 0..nc as isize {
                let (a, b, cc) = (at(c - 1, r - 1), at(c, r - 1), at(c + 1, r - 1));
                let (d, e, f) = (at(c - 1, r), at(c, r), at(c + 1, r));
                let (g, h, i) = (at(c - 1, r + 1), at(c, r + 1), at(c + 1, r + 1));
                if [a, b, cc, d, e, f, g, h, i].iter().any(|v| v.is_nan()) {
                    continue;
                }
                let dzdx = ((cc + 2.0 * f + i) - (a + 2.0 * d + g)) / (8.0 * self.cell);
                let dzdy = ((g + 2.0 * h + i) - (a + 2.0 * b + cc)) / (8.0 * self.cell);
                let slope = (z_factor * (dzdx * dzdx + dzdy * dzdy).sqrt()).atan();
                let aspect = if dzdx != 0.0 || dzdy != 0.0 {
                    let mut asp = dzdy.atan2(-dzdx);
                    if asp < 0.0 {
                        asp += std::f64::consts::TAU;
                    }
                    asp
                } else {
                    0.0
                };
                let shade = zenith.cos() * slope.cos() + zenith.sin() * slope.sin() * (az - aspect).cos();
                out[(r * nc as isize + c) as usize] = shade.clamp(0.0, 1.0) as f32;
            }
        }
        out
    }

    /// Minimum and maximum over cells that hold data.
    pub fn range(&self) -> Option<(f64, f64)> {
        let mut it = self.data.iter().copied().filter(|v| !v.is_nan());
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }

    /// Read an ESRI ASCII grid (`.asc`), the interchange format every GIS
    /// writes. Header keys are case-insensitive; `xllcenter` is converted to
    /// the corner convention.
    pub fn read_asc(path: &Path) -> Result<Self> {
        let f = std::fs::File::open(path)?;
        Self::parse_asc(BufReader::new(f))
    }

    pub fn parse_asc<R: BufRead>(mut reader: R) -> Result<Self> {
        let mut ncols = None;
        let mut nrows = None;
        let mut xll = None;
        let mut yll = None;
        let mut centered = false;
        let mut cell = None;
        let mut nodata = None;
        let mut line = String::new();
        let mut data: Vec<f64> = Vec::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let mut parts = t.split_whitespace();
            let key = parts.next().unwrap_or("");
            let lower = key.to_ascii_lowercase();
            let is_header = matches!(
                lower.as_str(),
                "ncols" | "nrows" | "xllcorner" | "yllcorner" | "xllcenter" | "yllcenter"
                    | "cellsize" | "nodata_value" | "dx" | "dy"
            );
            if is_header && data.is_empty() {
                let v: f64 = parts
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| Error::Format(format!("ASCII grid header {key} has no value")))?;
                match lower.as_str() {
                    "ncols" => ncols = Some(v as usize),
                    "nrows" => nrows = Some(v as usize),
                    "xllcorner" => xll = Some(v),
                    "yllcorner" => yll = Some(v),
                    "xllcenter" => {
                        xll = Some(v);
                        centered = true;
                    }
                    "yllcenter" => {
                        yll = Some(v);
                        centered = true;
                    }
                    "cellsize" | "dx" => cell = Some(v),
                    "dy" => {
                        if cell.is_some_and(|c| (c - v).abs() > 1e-9 * c.abs().max(1.0)) {
                            return Err(Error::Format(
                                "ASCII grid has dx != dy; only square cells are supported".into(),
                            ));
                        }
                    }
                    _ => nodata = Some(v),
                }
                continue;
            }
            for tok in t.split_whitespace() {
                let v: f64 = tok
                    .parse()
                    .map_err(|_| Error::Format(format!("ASCII grid: bad value {tok:?}")))?;
                data.push(v);
            }
        }
        let (Some(ncols), Some(nrows), Some(cell)) = (ncols, nrows, cell) else {
            return Err(Error::Format(
                "ASCII grid header needs ncols, nrows and cellsize".into(),
            ));
        };
        let (Some(mut x0), Some(mut y0)) = (xll, yll) else {
            return Err(Error::Format(
                "ASCII grid header needs xllcorner/yllcorner (or xllcenter/yllcenter)".into(),
            ));
        };
        if centered {
            x0 -= cell / 2.0;
            y0 -= cell / 2.0;
        }
        if data.len() != ncols * nrows {
            return Err(Error::Format(format!(
                "ASCII grid has {} values, header says {ncols} x {nrows} = {}",
                data.len(),
                ncols * nrows
            )));
        }
        if let Some(nd) = nodata {
            for v in &mut data {
                if *v == nd {
                    *v = f64::NAN;
                }
            }
        }
        Ok(Self {
            ncols,
            nrows,
            x0,
            y0,
            cell,
            nodata,
            data,
        })
    }

    /// Write an ESRI ASCII grid. No-data cells are written as the raster's
    /// `nodata` value, or -9999 when it has none.
    pub fn write_asc(&self, path: &Path) -> Result<()> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        self.write_asc_to(&mut f)
    }

    pub fn write_asc_to<W: Write>(&self, w: &mut W) -> Result<()> {
        let nd = self.nodata.unwrap_or(-9999.0);
        writeln!(w, "ncols {}", self.ncols)?;
        writeln!(w, "nrows {}", self.nrows)?;
        writeln!(w, "xllcorner {}", fmt(self.x0))?;
        writeln!(w, "yllcorner {}", fmt(self.y0))?;
        writeln!(w, "cellsize {}", fmt(self.cell))?;
        writeln!(w, "NODATA_value {}", fmt(nd))?;
        for row in 0..self.nrows {
            let mut line = String::with_capacity(self.ncols * 8);
            for col in 0..self.ncols {
                let v = self.data[row * self.ncols + col];
                if col > 0 {
                    line.push(' ');
                }
                line.push_str(&fmt(if v.is_nan() { nd } else { v }));
            }
            writeln!(w, "{line}")?;
        }
        Ok(())
    }
}

/// Numbers without a trailing `.0`, and without exponent noise for the
/// magnitudes a grid holds.
fn fmt(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_by_two() -> Raster {
        // top row: 1 2 3 ; bottom row: 4 5 6
        Raster {
            ncols: 3,
            nrows: 2,
            x0: 100.0,
            y0: 200.0,
            cell: 10.0,
            nodata: Some(-9999.0),
            data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
        }
    }

    #[test]
    fn cell_lookup_follows_the_gis_convention_top_row_first() {
        let r = three_by_two();
        assert_eq!(r.bounds(), (100.0, 200.0, 130.0, 220.0));
        // Lower-left corner is the bottom row, first column.
        assert_eq!(r.cell_at(100.0, 200.0), Some((0, 1)));
        assert_eq!(r.sample(101.0, 201.0), Some(4.0));
        // Upper-right is the top row, last column.
        assert_eq!(r.sample(129.0, 219.0), Some(3.0));
        assert_eq!(r.sample(130.0, 219.0), None);
        assert_eq!(r.center(0, 0), (105.0, 215.0));
    }

    #[test]
    fn bilinear_matches_cell_centres_and_interpolates_between() {
        let r = three_by_two();
        assert_eq!(r.sample_bilinear(105.0, 215.0), Some(1.0));
        assert_eq!(r.sample_bilinear(115.0, 215.0), Some(2.0));
        let mid = r.sample_bilinear(110.0, 215.0).unwrap();
        assert!((mid - 1.5).abs() < 1e-12);
        let mid_v = r.sample_bilinear(105.0, 210.0).unwrap();
        assert!((mid_v - 2.5).abs() < 1e-12);
    }

    #[test]
    fn asc_round_trips_and_maps_nodata_to_nan() {
        let text = "ncols 3\nnrows 2\nxllcorner 100\nyllcorner 200\ncellsize 10\nNODATA_value -9999\n1 2 3\n4 -9999 6\n";
        let r = Raster::parse_asc(text.as_bytes()).unwrap();
        assert_eq!(r.ncols, 3);
        assert!(r.get(1, 1).unwrap().is_nan());
        assert_eq!(r.sample(115.0, 205.0), None);
        assert_eq!(r.range(), Some((1.0, 6.0)));
        let mut out = Vec::new();
        r.write_asc_to(&mut out).unwrap();
        let back = Raster::parse_asc(out.as_slice()).unwrap();
        assert_eq!(back.ncols, r.ncols);
        assert_eq!(back.x0, r.x0);
        assert_eq!(back.get(0, 0), Some(1.0));
        assert!(back.get(1, 1).unwrap().is_nan());
    }

    #[test]
    fn xllcenter_is_shifted_to_the_corner() {
        let text = "ncols 1\nnrows 1\nxllcenter 5\nyllcenter 5\ncellsize 10\n7\n";
        let r = Raster::parse_asc(text.as_bytes()).unwrap();
        assert_eq!(r.x0, 0.0);
        assert_eq!(r.y0, 0.0);
    }

    #[test]
    fn value_count_mismatch_is_an_error() {
        let text = "ncols 2\nnrows 2\nxllcorner 0\nyllcorner 0\ncellsize 1\n1 2 3\n";
        assert!(Raster::parse_asc(text.as_bytes()).is_err());
    }
}
