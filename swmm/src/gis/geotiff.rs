// SPDX-License-Identifier: GPL-3.0-or-later

//! GeoTIFF DEMs, read from TIFF 6.0 (Adobe, 1992), GeoTIFF 1.1 (OGC
//! 19-008r4) and the GDAL `GDAL_NODATA` tag (42113).
//!
//! Supported: classic TIFF in either byte order; one sample per pixel or
//! the first band of a chunky (PlanarConfiguration 1) multi-band image;
//! unsigned 8/16/32-bit, signed 8/16/32-bit and 32/64-bit float samples;
//! strips or tiles; compression none (1), LZW (5), Deflate (8 and the
//! older 32946), PackBits (32773); predictor 2 (horizontal differencing)
//! and 3 (floating-point differencing); georeferencing from
//! ModelPixelScale + ModelTiepoint or an axis-aligned ModelTransformation;
//! the CRS from the GeoKeyDirectory's EPSG codes (ProjectedCSType,
//! GeographicType) or, for a user-defined projected system, its
//! ProjCoordTrans and parameter keys; linear units from ProjLinearUnits.
//!
//! Not supported, each with its own message: BigTIFF (magic 43), JPEG or
//! CCITT compression, PlanarConfiguration 2 with several bands, rotated
//! or sheared transformations, non-square cells, sub-byte samples.

use std::path::Path;

use crate::gis::crs::{Crs, Datum, Ellipsoid, Projection, Unit};
use crate::gis::raster::Raster;
use crate::gis::{inflate, lzw};
use crate::{Error, Result};

/// A read GeoTIFF: the grid, its CRS if declared, and a one-line
/// description of how it was stored.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoTiff {
    pub raster: Raster,
    pub crs: Option<Crs>,
    pub info: String,
}

#[derive(Clone, Copy, Debug)]
enum Order {
    Le,
    Be,
}

struct Reader<'a> {
    b: &'a [u8],
    order: Order,
}

impl<'a> Reader<'a> {
    fn u16(&self, at: usize) -> Result<u16> {
        let s = self.b.get(at..at + 2).ok_or_else(|| Error::Format("TIFF ends inside a field".into()))?;
        Ok(match self.order {
            Order::Le => u16::from_le_bytes([s[0], s[1]]),
            Order::Be => u16::from_be_bytes([s[0], s[1]]),
        })
    }

    fn u32(&self, at: usize) -> Result<u32> {
        let s = self.b.get(at..at + 4).ok_or_else(|| Error::Format("TIFF ends inside a field".into()))?;
        Ok(match self.order {
            Order::Le => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            Order::Be => u32::from_be_bytes([s[0], s[1], s[2], s[3]]),
        })
    }

    fn f64(&self, at: usize) -> Result<f64> {
        let s = self.b.get(at..at + 8).ok_or_else(|| Error::Format("TIFF ends inside a double".into()))?;
        let a: [u8; 8] = s.try_into().unwrap();
        Ok(match self.order {
            Order::Le => f64::from_le_bytes(a),
            Order::Be => f64::from_be_bytes(a),
        })
    }
}

#[derive(Clone, Debug)]
struct Entry {
    tag: u16,
    kind: u16,
    count: usize,
    /// Offset of the value data (inline in the entry when it fits).
    at: usize,
}

fn type_size(kind: u16) -> usize {
    match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 | 16 | 17 | 18 => 8,
        _ => 0,
    }
}

struct Ifd<'a> {
    r: Reader<'a>,
    entries: Vec<Entry>,
}

impl<'a> Ifd<'a> {
    fn get(&self, tag: u16) -> Option<&Entry> {
        self.entries.iter().find(|e| e.tag == tag)
    }

    /// Integer values of a SHORT/LONG/BYTE tag.
    fn ints(&self, tag: u16) -> Result<Vec<u64>> {
        let Some(e) = self.get(tag) else { return Ok(Vec::new()) };
        let mut out = Vec::with_capacity(e.count);
        for i in 0..e.count {
            let at = e.at + i * type_size(e.kind);
            out.push(match e.kind {
                1 | 7 => *self.r.b.get(at).ok_or_else(|| Error::Format("TIFF tag data cut".into()))? as u64,
                3 => self.r.u16(at)? as u64,
                4 | 13 => self.r.u32(at)? as u64,
                8 => self.r.u16(at)? as i16 as i64 as u64,
                9 => self.r.u32(at)? as i32 as i64 as u64,
                k => return Err(Error::Format(format!("TIFF tag {tag} has type {k}, expected an integer"))),
            });
        }
        Ok(out)
    }

    fn int(&self, tag: u16) -> Result<Option<u64>> {
        Ok(self.ints(tag)?.first().copied())
    }

    fn doubles(&self, tag: u16) -> Result<Vec<f64>> {
        let Some(e) = self.get(tag) else { return Ok(Vec::new()) };
        let mut out = Vec::with_capacity(e.count);
        for i in 0..e.count {
            let at = e.at + i * type_size(e.kind);
            out.push(match e.kind {
                12 => self.r.f64(at)?,
                11 => f32::from_bits(self.r.u32(at)?) as f64,
                3 => self.r.u16(at)? as f64,
                4 => self.r.u32(at)? as f64,
                k => return Err(Error::Format(format!("TIFF tag {tag} has type {k}, expected a double"))),
            });
        }
        Ok(out)
    }

    fn ascii(&self, tag: u16) -> Option<String> {
        let e = self.get(tag)?;
        let s = self.r.b.get(e.at..e.at + e.count)?;
        Some(String::from_utf8_lossy(s).trim_end_matches('\0').to_string())
    }
}

fn parse_ifd(bytes: &[u8]) -> Result<Ifd<'_>> {
    if bytes.len() < 8 {
        return Err(Error::Format("TIFF is shorter than its 8-byte header".into()));
    }
    let order = match &bytes[0..2] {
        b"II" => Order::Le,
        b"MM" => Order::Be,
        _ => return Err(Error::Format("not a TIFF: byte-order mark is neither II nor MM".into())),
    };
    let r = Reader { b: bytes, order };
    let magic = r.u16(2)?;
    if magic == 43 {
        return Err(Error::Format(
            "BigTIFF (magic 43) is not supported; rewrite the DEM as classic TIFF (e.g. gdal_translate -co BIGTIFF=NO)".into(),
        ));
    }
    if magic != 42 {
        return Err(Error::Format(format!("not a TIFF: magic number {magic}, expected 42")));
    }
    let ifd = r.u32(4)? as usize;
    let n = r.u16(ifd)? as usize;
    let mut entries = Vec::with_capacity(n);
    for i in 0..n {
        let at = ifd + 2 + 12 * i;
        let tag = r.u16(at)?;
        let kind = r.u16(at + 2)?;
        let count = r.u32(at + 4)? as usize;
        let size = type_size(kind) * count;
        let data_at = if size <= 4 { at + 8 } else { r.u32(at + 8)? as usize };
        entries.push(Entry {
            tag,
            kind,
            count,
            at: data_at,
        });
    }
    Ok(Ifd { r, entries })
}

/// PackBits (TIFF §9) decode.
fn unpackbits(data: &[u8], expected: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0;
    while i < data.len() && out.len() < expected {
        let n = data[i] as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            let end = (i + len).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i = end;
        } else if n != -128 {
            let len = (-(n as i32)) as usize + 1;
            if let Some(&b) = data.get(i) {
                out.extend(std::iter::repeat(b).take(len));
            }
            i += 1;
        }
    }
    out
}

/// Undo predictor 2 in place on rows of `width` samples of `bps` bytes,
/// `spp` samples per pixel, in the file's byte order.
fn undo_horizontal(buf: &mut [u8], width: usize, rows: usize, spp: usize, bps: usize, order: Order, float: bool) {
    let row_bytes = width * spp * bps;
    let total = buf.len();
    for r in 0..rows {
        let end = ((r + 1) * row_bytes).min(total);
        let row = &mut buf[r * row_bytes..end];
        if row.len() < row_bytes {
            break;
        }
        match (bps, float) {
            (1, _) => {
                for i in spp..row.len() {
                    row[i] = row[i].wrapping_add(row[i - spp]);
                }
            }
            (2, false) => {
                for i in spp..width * spp {
                    let (a, b) = (i * 2, (i - spp) * 2);
                    let prev = match order {
                        Order::Le => u16::from_le_bytes([row[b], row[b + 1]]),
                        Order::Be => u16::from_be_bytes([row[b], row[b + 1]]),
                    };
                    let cur = match order {
                        Order::Le => u16::from_le_bytes([row[a], row[a + 1]]),
                        Order::Be => u16::from_be_bytes([row[a], row[a + 1]]),
                    };
                    let v = cur.wrapping_add(prev);
                    let bytes = match order {
                        Order::Le => v.to_le_bytes(),
                        Order::Be => v.to_be_bytes(),
                    };
                    row[a..a + 2].copy_from_slice(&bytes);
                }
            }
            (4, false) => {
                for i in spp..width * spp {
                    let (a, b) = (i * 4, (i - spp) * 4);
                    let rd = |s: &[u8]| match order {
                        Order::Le => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                        Order::Be => u32::from_be_bytes([s[0], s[1], s[2], s[3]]),
                    };
                    let v = rd(&row[a..a + 4]).wrapping_add(rd(&row[b..b + 4]));
                    let bytes = match order {
                        Order::Le => v.to_le_bytes(),
                        Order::Be => v.to_be_bytes(),
                    };
                    row[a..a + 4].copy_from_slice(&bytes);
                }
            }
            (8, false) => {
                for i in spp..width * spp {
                    let (a, b) = (i * 8, (i - spp) * 8);
                    let rd = |s: &[u8]| {
                        let arr: [u8; 8] = s[..8].try_into().unwrap();
                        match order {
                            Order::Le => u64::from_le_bytes(arr),
                            Order::Be => u64::from_be_bytes(arr),
                        }
                    };
                    let v = rd(&row[a..a + 8]).wrapping_add(rd(&row[b..b + 8]));
                    let bytes = match order {
                        Order::Le => v.to_le_bytes(),
                        Order::Be => v.to_be_bytes(),
                    };
                    row[a..a + 8].copy_from_slice(&bytes);
                }
            }
            _ => {
                // Predictor 2 on a float sample is not defined by the
                // spec (that is predictor 3); treat as bytes, as libtiff
                // does for 8-bit data.
                for i in spp..row.len() {
                    row[i] = row[i].wrapping_add(row[i - spp]);
                }
            }
        }
    }
}

/// Undo predictor 3 (floating-point horizontal differencing, TIFF
/// Technical Note 3): byte-wise accumulation along the row, then the
/// row's byte planes (most significant first) are re-interleaved into
/// big-endian samples.
fn undo_float_predictor(buf: &mut [u8], width: usize, rows: usize, spp: usize, bps: usize) -> Vec<u8> {
    let row_bytes = width * spp * bps;
    let wc = width * spp;
    let mut out = vec![0u8; buf.len()];
    let mut tmp = vec![0u8; row_bytes];
    for r in 0..rows {
        let start = r * row_bytes;
        if start + row_bytes > buf.len() {
            break;
        }
        let row = &mut buf[start..start + row_bytes];
        for i in spp..row_bytes {
            row[i] = row[i].wrapping_add(row[i - spp]);
        }
        tmp.copy_from_slice(row);
        for s in 0..wc {
            for b in 0..bps {
                out[start + s * bps + b] = tmp[b * wc + s];
            }
        }
    }
    out
}

fn sample_to_f64(bytes: &[u8], format: u64, bps: usize, order: Order, big_endian_override: bool) -> f64 {
    let be = big_endian_override || matches!(order, Order::Be);
    match (format, bps) {
        (3, 4) => {
            let a = [bytes[0], bytes[1], bytes[2], bytes[3]];
            (if be { f32::from_be_bytes(a) } else { f32::from_le_bytes(a) }) as f64
        }
        (3, 8) => {
            let a: [u8; 8] = bytes[..8].try_into().unwrap();
            if be { f64::from_be_bytes(a) } else { f64::from_le_bytes(a) }
        }
        (2, 1) => bytes[0] as i8 as f64,
        (2, 2) => {
            let a = [bytes[0], bytes[1]];
            (if be { i16::from_be_bytes(a) } else { i16::from_le_bytes(a) }) as f64
        }
        (2, 4) => {
            let a = [bytes[0], bytes[1], bytes[2], bytes[3]];
            (if be { i32::from_be_bytes(a) } else { i32::from_le_bytes(a) }) as f64
        }
        (_, 1) => bytes[0] as f64,
        (_, 2) => {
            let a = [bytes[0], bytes[1]];
            (if be { u16::from_be_bytes(a) } else { u16::from_le_bytes(a) }) as f64
        }
        (_, 4) => {
            let a = [bytes[0], bytes[1], bytes[2], bytes[3]];
            (if be { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }) as f64
        }
        _ => f64::NAN,
    }
}

/// Parse a GeoTIFF image held in memory.
pub fn parse(bytes: &[u8]) -> Result<GeoTiff> {
    let ifd = parse_ifd(bytes)?;
    let order = ifd.r.order;
    let width = ifd.int(256)?.ok_or_else(|| Error::Format("TIFF has no ImageWidth".into()))? as usize;
    let height = ifd.int(257)?.ok_or_else(|| Error::Format("TIFF has no ImageLength".into()))? as usize;
    if width == 0 || height == 0 {
        return Err(Error::Format("TIFF image is empty".into()));
    }
    let bits = ifd.ints(258)?;
    let bits = bits.first().copied().unwrap_or(1);
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err(Error::Format(format!("{bits}-bit samples are not supported (8, 16, 32 or 64)")));
    }
    let bps = (bits / 8) as usize;
    let spp = ifd.int(277)?.unwrap_or(1) as usize;
    let format = ifd.ints(339)?.first().copied().unwrap_or(1);
    if format == 3 && !matches!(bits, 32 | 64) {
        return Err(Error::Format(format!("{bits}-bit floating point samples are not supported")));
    }
    if format > 3 {
        return Err(Error::Format(format!("SampleFormat {format} is not supported")));
    }
    let compression = ifd.int(259)?.unwrap_or(1);
    let planar = ifd.int(284)?.unwrap_or(1);
    if planar == 2 && spp > 1 {
        return Err(Error::Format("PlanarConfiguration 2 (separate bands) is not supported".into()));
    }
    let predictor = ifd.int(317)?.unwrap_or(1);
    if !matches!(predictor, 1 | 2 | 3) {
        return Err(Error::Format(format!("Predictor {predictor} is not supported")));
    }
    let decompress = |data: &[u8], expected: usize| -> Result<Vec<u8>> {
        Ok(match compression {
            1 => data.to_vec(),
            5 => lzw::decode(data, expected)?,
            8 | 32946 => inflate::inflate(data)?,
            32773 => unpackbits(data, expected),
            7 => return Err(Error::Format("JPEG-compressed TIFF is not supported".into())),
            2 | 3 | 4 => return Err(Error::Format("CCITT-compressed TIFF is not supported".into())),
            c => return Err(Error::Format(format!("TIFF compression {c} is not supported"))),
        })
    };
    // Sample data as big-endian bytes when predictor 3 rebuilt them,
    // else in the file's order.
    let mut samples = vec![0u8; width * height * bps];
    let float_pred = predictor == 3;
    let mut chunk_desc;
    if let Some(tw) = ifd.int(322)? {
        let tw = tw as usize;
        let th = ifd.int(323)?.unwrap_or(height as u64) as usize;
        let offsets = ifd.ints(324)?;
        let counts = ifd.ints(325)?;
        let across = width.div_ceil(tw);
        let down = height.div_ceil(th);
        chunk_desc = format!("{across}x{down} tiles of {tw}x{th}");
        for ty in 0..down {
            for tx in 0..across {
                let i = ty * across + tx;
                let (Some(&off), Some(&cnt)) = (offsets.get(i), counts.get(i)) else {
                    return Err(Error::Format(format!("TIFF tile {i} has no offset")));
                };
                let raw = bytes
                    .get(off as usize..(off + cnt) as usize)
                    .ok_or_else(|| Error::Format(format!("TIFF tile {i} lies past the end of the file")))?;
                let expected = tw * th * spp * bps;
                let mut buf = decompress(raw, expected)?;
                if buf.len() < expected {
                    return Err(Error::Format(format!("TIFF tile {i} decoded to {} bytes, expected {expected}", buf.len())));
                }
                if predictor == 2 {
                    undo_horizontal(&mut buf, tw, th, spp, bps, order, false);
                } else if float_pred {
                    buf = undo_float_predictor(&mut buf, tw, th, spp, bps);
                }
                for r in 0..th {
                    let y = ty * th + r;
                    if y >= height {
                        break;
                    }
                    for c in 0..tw {
                        let x = tx * tw + c;
                        if x >= width {
                            break;
                        }
                        let src = (r * tw + c) * spp * bps;
                        let dst = (y * width + x) * bps;
                        samples[dst..dst + bps].copy_from_slice(&buf[src..src + bps]);
                    }
                }
            }
        }
    } else {
        let offsets = ifd.ints(273)?;
        let counts = ifd.ints(279)?;
        let rps = ifd.int(278)?.map_or(height, |v| (v as usize).min(height)).max(1);
        let nstrips = height.div_ceil(rps);
        chunk_desc = format!("{nstrips} strip(s) of {rps} row(s)");
        if offsets.is_empty() {
            return Err(Error::Format("TIFF has neither strips nor tiles".into()));
        }
        for s in 0..nstrips {
            let (Some(&off), Some(&cnt)) = (offsets.get(s), counts.get(s)) else {
                return Err(Error::Format(format!("TIFF strip {s} has no offset")));
            };
            let raw = bytes
                .get(off as usize..(off + cnt) as usize)
                .ok_or_else(|| Error::Format(format!("TIFF strip {s} lies past the end of the file")))?;
            let rows = rps.min(height - s * rps);
            let expected = rows * width * spp * bps;
            let mut buf = decompress(raw, expected)?;
            if buf.len() < expected {
                return Err(Error::Format(format!("TIFF strip {s} decoded to {} bytes, expected {expected}", buf.len())));
            }
            if predictor == 2 {
                undo_horizontal(&mut buf, width, rows, spp, bps, order, false);
            } else if float_pred {
                buf = undo_float_predictor(&mut buf, width, rows, spp, bps);
            }
            for r in 0..rows {
                let y = s * rps + r;
                for x in 0..width {
                    let src = (r * width + x) * spp * bps;
                    let dst = (y * width + x) * bps;
                    samples[dst..dst + bps].copy_from_slice(&buf[src..src + bps]);
                }
            }
        }
    }
    let nodata = ifd.ascii(42113).and_then(|s| s.trim().parse::<f64>().ok());
    let mut data = Vec::with_capacity(width * height);
    for i in 0..width * height {
        let v = sample_to_f64(&samples[i * bps..(i + 1) * bps], format, bps, order, float_pred);
        data.push(if nodata.is_some_and(|nd| v == nd || (nd.is_nan() && v.is_nan())) { f64::NAN } else { v });
    }
    // Georeferencing.
    let scale = ifd.doubles(33550)?;
    let tie = ifd.doubles(33922)?;
    let xform = ifd.doubles(34264)?;
    let (cell, ox, oy) = if xform.len() >= 16 {
        let (a, b, d, e, f, h) = (xform[0], xform[1], xform[3], xform[4], xform[5], xform[7]);
        if b.abs() > 1e-9 * a.abs().max(1.0) || e.abs() > 1e-9 * f.abs().max(1.0) {
            return Err(Error::Format("rotated or sheared ModelTransformation is not supported".into()));
        }
        if (a.abs() - f.abs()).abs() > 1e-6 * a.abs() {
            return Err(Error::Format(format!("cells are not square ({a} x {f}); only square cells are supported")));
        }
        (a.abs(), d, h)
    } else if scale.len() >= 2 && tie.len() >= 6 {
        let (sx, sy) = (scale[0], scale[1]);
        if (sx.abs() - sy.abs()).abs() > 1e-6 * sx.abs() {
            return Err(Error::Format(format!("cells are not square ({sx} x {sy}); only square cells are supported")));
        }
        // Tiepoint (I, J) -> (X, Y): the raster origin is I pixels left
        // and J pixels up from it.
        let (i, j, x, y) = (tie[0], tie[1], tie[3], tie[4]);
        (sx.abs(), x - i * sx.abs(), y + j * sy.abs())
    } else {
        return Err(Error::Format(
            "TIFF has no georeferencing (ModelPixelScale + ModelTiepoint, or ModelTransformation)".into(),
        ));
    };
    let keys = geokeys(&ifd)?;
    let pixel_is_point = keys.iter().any(|k| k.id == 1025 && k.value == 2);
    let (x0, y1) = if pixel_is_point {
        (ox - cell / 2.0, oy + cell / 2.0)
    } else {
        (ox, oy)
    };
    let raster = Raster {
        ncols: width,
        nrows: height,
        x0,
        y0: y1 - cell * height as f64,
        cell,
        nodata,
        data,
    };
    let crs = crs_from_keys(&keys, &ifd);
    let dtype = match (format, bits) {
        (3, b) => format!("float{b}"),
        (2, b) => format!("int{b}"),
        (_, b) => format!("uint{b}"),
    };
    let comp = match compression {
        1 => "uncompressed",
        5 => "LZW",
        8 | 32946 => "Deflate",
        32773 => "PackBits",
        _ => "?",
    };
    if predictor > 1 {
        chunk_desc.push_str(&format!(", predictor {predictor}"));
    }
    let info = format!("{width}x{height} {dtype}, {comp}, {chunk_desc}, {spp} band(s)");
    Ok(GeoTiff { raster, crs, info })
}

pub fn read(path: &Path) -> Result<GeoTiff> {
    let bytes = std::fs::read(path)?;
    parse(&bytes)
}

// ---------------------------------------------------------------------------
// GeoKeys
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct GeoKey {
    id: u16,
    /// SHORT value when `location == 0`.
    value: u16,
    /// Double values from GeoDoubleParams, or the ASCII text.
    doubles: Vec<f64>,
    text: Option<String>,
}

fn geokeys(ifd: &Ifd) -> Result<Vec<GeoKey>> {
    let dir = ifd.ints(34735)?;
    if dir.len() < 4 {
        return Ok(Vec::new());
    }
    let doubles = ifd.doubles(34736)?;
    let ascii = ifd.ascii(34737).unwrap_or_default();
    let n = dir[3] as usize;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let base = 4 + 4 * k;
        if base + 4 > dir.len() {
            break;
        }
        let (id, loc, count, val) = (dir[base] as u16, dir[base + 1], dir[base + 2] as usize, dir[base + 3] as usize);
        let mut key = GeoKey {
            id,
            value: 0,
            doubles: Vec::new(),
            text: None,
        };
        match loc {
            0 => key.value = val as u16,
            34736 => key.doubles = doubles.get(val..val + count).map(|s| s.to_vec()).unwrap_or_default(),
            34737 => {
                key.text = ascii
                    .get(val..(val + count).min(ascii.len()))
                    .map(|s| s.trim_end_matches('|').trim().to_string())
            }
            _ => {}
        }
        out.push(key);
    }
    Ok(out)
}

fn key_short(keys: &[GeoKey], id: u16) -> Option<u16> {
    keys.iter().find(|k| k.id == id).map(|k| k.value)
}

fn key_double(keys: &[GeoKey], id: u16) -> Option<f64> {
    keys.iter().find(|k| k.id == id).and_then(|k| k.doubles.first().copied())
}

fn unit_from_code(code: u16) -> Option<Unit> {
    match code {
        9001 => Some(Unit::metre()),
        9002 => Some(Unit::intl_foot()),
        9003 => Some(Unit::us_foot()),
        _ => None,
    }
}

/// The CRS the GeoKeys name, if any.
fn crs_from_keys(keys: &[GeoKey], ifd: &Ifd) -> Option<Crs> {
    let _ = ifd;
    let model = key_short(keys, 1024).unwrap_or(0);
    let citation = keys
        .iter()
        .find(|k| k.id == 3073 || k.id == 1026)
        .and_then(|k| k.text.clone());
    if model == 2 {
        let code = key_short(keys, 2048).unwrap_or(4326);
        return match code {
            32767 => {
                let mut c = Crs::wgs84();
                c.epsg = None;
                c.name = citation.unwrap_or_else(|| "user-defined geographic".into());
                Some(c)
            }
            c => Crs::from_epsg(c as u32).ok(),
        };
    }
    if model != 1 {
        return None;
    }
    let pcs = key_short(keys, 3072).unwrap_or(32767);
    let unit = key_short(keys, 3076).and_then(unit_from_code);
    if pcs != 32767 {
        if let Ok(mut c) = Crs::from_epsg(pcs as u32) {
            if let Some(u) = unit {
                if (u.to_metre - c.unit.to_metre).abs() > 1e-9 {
                    c.unit = u;
                    c.epsg = None;
                }
            }
            return Some(c);
        }
    }
    // User-defined projection from the coordinate-transformation keys.
    let gcs = key_short(keys, 2048).unwrap_or(4326);
    let (datum, ellipsoid) = match gcs {
        4269 => (Datum::Nad83, Ellipsoid::grs80()),
        4267 => (Datum::Nad27, Ellipsoid::clarke_1866()),
        _ => (Datum::Wgs84, Ellipsoid::wgs84()),
    };
    let unit = unit.unwrap_or_else(Unit::metre);
    let um = unit.to_metre;
    let g = |id: u16| key_double(keys, id);
    let fe = g(3082).or(g(3086)).unwrap_or(0.0) * um;
    let fn_ = g(3083).or(g(3087)).unwrap_or(0.0) * um;
    let lon0 = g(3080).or(g(3084)).or(g(3088)).unwrap_or(0.0);
    let lat0 = g(3081).or(g(3085)).or(g(3089)).unwrap_or(0.0);
    let k0 = g(3092).or(g(3093)).unwrap_or(1.0);
    let sp1 = g(3078);
    let sp2 = g(3079);
    let projection = match key_short(keys, 3075)? {
        1 => Projection::TransverseMercator { lat0, lon0, k0, fe, fn_ },
        8 => Projection::LambertConformal2Sp {
            lat0,
            lon0,
            sp1: sp1?,
            sp2: sp2.or(sp1)?,
            fe,
            fn_,
        },
        9 => Projection::LambertConformal1Sp { lat0, lon0, k0, fe, fn_ },
        11 => Projection::AlbersEqualArea {
            lat0,
            lon0,
            sp1: sp1?,
            sp2: sp2.or(sp1)?,
            fe,
            fn_,
        },
        7 => Projection::Mercator { lon0, k0, fe, fn_ },
        15 => Projection::PolarStereographic {
            lat_ts: sp1.or(g(3081)).unwrap_or(90.0),
            lon0: g(3095).unwrap_or(lon0),
            fe,
            fn_,
        },
        _ => return None,
    };
    let mut c = Crs {
        name: citation.unwrap_or_else(|| "user-defined projected".into()),
        datum,
        ellipsoid,
        projection,
        unit,
        epsg: None,
        wkt: None,
    };
    c.epsg = crate::gis::stateplane::identify(&c);
    Some(c)
}

// ---------------------------------------------------------------------------
// A minimal writer, for tests and for saving a DEM subset: uncompressed
// or LZW, strips or tiles, with the georeferencing and an EPSG code.
// ---------------------------------------------------------------------------

/// How [`encode`] lays the file out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodeOptions {
    pub compression: u16,
    pub predictor: u16,
    /// Tile size, or None for strips of `rows_per_strip`.
    pub tile: Option<usize>,
    pub rows_per_strip: usize,
    /// SampleFormat: 1 uint, 2 int, 3 float; with `bits` 8/16/32/64.
    pub format: u16,
    pub bits: u16,
    pub big_endian: bool,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            compression: 1,
            predictor: 1,
            tile: None,
            rows_per_strip: 0,
            format: 3,
            bits: 32,
            big_endian: false,
        }
    }
}

fn sample_bytes(v: f64, o: &EncodeOptions) -> Vec<u8> {
    macro_rules! put {
        ($val:expr) => {
            if o.big_endian { $val.to_be_bytes().to_vec() } else { $val.to_le_bytes().to_vec() }
        };
    }
    match (o.format, o.bits) {
        (3, 32) => put!(v as f32),
        (3, 64) => put!(v),
        (2, 8) => vec![v as i8 as u8],
        (2, 16) => put!(v as i16),
        (2, 32) => put!(v as i32),
        (_, 8) => vec![v as u8],
        (_, 16) => put!(v as u16),
        _ => put!(v as u32),
    }
}

fn apply_predictor(buf: &mut [u8], width: usize, rows: usize, bps: usize, o: &EncodeOptions) -> Vec<u8> {
    let row_bytes = width * bps;
    match o.predictor {
        2 => {
            for r in 0..rows {
                let row = &mut buf[r * row_bytes..(r + 1) * row_bytes];
                for x in (1..width).rev() {
                    let (a, b) = (x * bps, (x - 1) * bps);
                    match bps {
                        1 => row[a] = row[a].wrapping_sub(row[b]),
                        2 => {
                            let rd = |s: &[u8]| if o.big_endian { u16::from_be_bytes([s[0], s[1]]) } else { u16::from_le_bytes([s[0], s[1]]) };
                            let v = rd(&row[a..a + 2]).wrapping_sub(rd(&row[b..b + 2]));
                            row[a..a + 2].copy_from_slice(&if o.big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
                        }
                        8 => {
                            let rd = |s: &[u8]| {
                                let arr: [u8; 8] = s[..8].try_into().unwrap();
                                if o.big_endian { u64::from_be_bytes(arr) } else { u64::from_le_bytes(arr) }
                            };
                            let v = rd(&row[a..a + 8]).wrapping_sub(rd(&row[b..b + 8]));
                            row[a..a + 8].copy_from_slice(&if o.big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
                        }
                        _ => {
                            let rd = |s: &[u8]| if o.big_endian { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) };
                            let v = rd(&row[a..a + 4]).wrapping_sub(rd(&row[b..b + 4]));
                            row[a..a + 4].copy_from_slice(&if o.big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
                        }
                    }
                }
            }
            buf.to_vec()
        }
        3 => {
            let mut out = vec![0u8; buf.len()];
            for r in 0..rows {
                let start = r * row_bytes;
                let row = &buf[start..start + row_bytes];
                // Split into big-endian byte planes, then difference.
                let mut planes = vec![0u8; row_bytes];
                for s in 0..width {
                    let be: Vec<u8> = if o.big_endian { row[s * bps..(s + 1) * bps].to_vec() } else { row[s * bps..(s + 1) * bps].iter().rev().copied().collect() };
                    for (b, byte) in be.iter().enumerate() {
                        planes[b * width + s] = *byte;
                    }
                }
                for i in (1..row_bytes).rev() {
                    planes[i] = planes[i].wrapping_sub(planes[i - 1]);
                }
                out[start..start + row_bytes].copy_from_slice(&planes);
            }
            out
        }
        _ => buf.to_vec(),
    }
}

/// Encode a raster as a GeoTIFF with ModelPixelScale/ModelTiepoint, a
/// GDAL_NODATA tag when the raster has one, and a GeoKeyDirectory naming
/// `epsg` (ProjectedCSType for a projected code, GeographicType for
/// 4326/4269/4267) when given.
pub fn encode(r: &Raster, epsg: Option<u32>, o: &EncodeOptions) -> Vec<u8> {
    let bps = (o.bits / 8) as usize;
    let (w, h) = (r.ncols, r.nrows);
    let nd = r.nodata;
    let mut raw = Vec::with_capacity(w * h * bps);
    for &v in &r.data {
        let v = if v.is_nan() { nd.unwrap_or(-9999.0) } else { v };
        raw.extend(sample_bytes(v, o));
    }
    let compress = |chunk: &[u8]| -> Vec<u8> {
        match o.compression {
            5 => lzw::encode(chunk),
            32773 => {
                // PackBits: literal runs only (valid, if not small).
                let mut out = Vec::new();
                for piece in chunk.chunks(128) {
                    out.push((piece.len() - 1) as u8);
                    out.extend_from_slice(piece);
                }
                out
            }
            _ => chunk.to_vec(),
        }
    };
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let (tile_tags, rps) = if let Some(t) = o.tile {
        let across = w.div_ceil(t);
        let down = h.div_ceil(t);
        for ty in 0..down {
            for tx in 0..across {
                let mut buf = vec![0u8; t * t * bps];
                for rr in 0..t {
                    for cc in 0..t {
                        let (x, y) = (tx * t + cc, ty * t + rr);
                        if x < w && y < h {
                            let src = (y * w + x) * bps;
                            let dst = (rr * t + cc) * bps;
                            buf[dst..dst + bps].copy_from_slice(&raw[src..src + bps]);
                        }
                    }
                }
                let buf = apply_predictor(&mut buf, t, t, bps, o);
                chunks.push(compress(&buf));
            }
        }
        (true, 0)
    } else {
        let rps = if o.rows_per_strip == 0 { h } else { o.rows_per_strip.min(h) };
        for s in 0..h.div_ceil(rps) {
            let rows = rps.min(h - s * rps);
            let mut buf = raw[s * rps * w * bps..(s * rps + rows) * w * bps].to_vec();
            let buf = apply_predictor(&mut buf, w, rows, bps, o);
            chunks.push(compress(&buf));
        }
        (false, rps)
    };
    // Layout: header (8), IFD, then tag data blobs, then chunks.
    let be = o.big_endian;
    let u16b = |v: u16| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let u32b = |v: u32| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let f64b = |v: f64| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    struct Tag {
        id: u16,
        kind: u16,
        count: u32,
        data: Vec<u8>,
    }
    let mut tags: Vec<Tag> = Vec::new();
    let short = |id: u16, v: u16| Tag { id, kind: 3, count: 1, data: u16b(v).to_vec() };
    let long = |id: u16, v: u32| Tag { id, kind: 4, count: 1, data: u32b(v).to_vec() };
    tags.push(long(256, w as u32));
    tags.push(long(257, h as u32));
    tags.push(short(258, o.bits));
    tags.push(short(259, o.compression));
    tags.push(short(262, 1));
    tags.push(short(277, 1));
    tags.push(short(284, 1));
    if o.predictor > 1 {
        tags.push(short(317, o.predictor));
    }
    tags.push(short(339, o.format));
    let n = chunks.len() as u32;
    let offsets_tag_index;
    if tile_tags {
        let t = o.tile.unwrap() as u32;
        tags.push(long(322, t));
        tags.push(long(323, t));
        offsets_tag_index = tags.len();
        tags.push(Tag { id: 324, kind: 4, count: n, data: Vec::new() });
        tags.push(Tag { id: 325, kind: 4, count: n, data: chunks.iter().flat_map(|c| u32b(c.len() as u32)).collect() });
    } else {
        offsets_tag_index = tags.len();
        tags.push(Tag { id: 273, kind: 4, count: n, data: Vec::new() });
        tags.push(long(278, rps as u32));
        tags.push(Tag { id: 279, kind: 4, count: n, data: chunks.iter().flat_map(|c| u32b(c.len() as u32)).collect() });
    }
    tags.push(Tag { id: 33550, kind: 12, count: 3, data: [r.cell, r.cell, 0.0].iter().flat_map(|v| f64b(*v)).collect() });
    tags.push(Tag { id: 33922, kind: 12, count: 6, data: [0.0, 0.0, 0.0, r.x0, r.y1(), 0.0].iter().flat_map(|v| f64b(*v)).collect() });
    let mut keys: Vec<u16> = vec![1, 1, 0, 0];
    let mut nkeys = 0u16;
    let mut push_key = |keys: &mut Vec<u16>, id: u16, v: u16| {
        keys.extend_from_slice(&[id, 0, 1, v]);
        nkeys += 1;
    };
    match epsg {
        Some(c @ (4326 | 4269 | 4267)) => {
            push_key(&mut keys, 1024, 2);
            push_key(&mut keys, 1025, 1);
            push_key(&mut keys, 2048, c as u16);
        }
        Some(c) => {
            push_key(&mut keys, 1024, 1);
            push_key(&mut keys, 1025, 1);
            push_key(&mut keys, 3072, c as u16);
            let unit = Crs::from_epsg(c).map(|x| x.unit.to_metre).unwrap_or(1.0);
            push_key(&mut keys, 3076, if (unit - 1.0).abs() < 1e-9 { 9001 } else if (unit - Unit::US_FOOT).abs() < 1e-9 { 9003 } else { 9002 });
        }
        None => {
            push_key(&mut keys, 1025, 1);
        }
    }
    keys[3] = nkeys;
    tags.push(Tag { id: 34735, kind: 3, count: keys.len() as u32, data: keys.iter().flat_map(|v| u16b(*v)).collect() });
    if let Some(nd) = nd {
        let mut s = crate::doc::format_number(nd).into_bytes();
        s.push(0);
        tags.push(Tag { id: 42113, kind: 2, count: s.len() as u32, data: s });
    }
    tags.sort_by_key(|t| t.id);
    let offsets_tag_index = tags.iter().position(|t| t.id == if tile_tags { 324 } else { 273 }).unwrap_or(offsets_tag_index);
    // Compute positions.
    let ifd_at = 8usize;
    let ifd_len = 2 + 12 * tags.len() + 4;
    let mut blob_at = ifd_at + ifd_len;
    let mut blob_positions = Vec::with_capacity(tags.len());
    for t in &tags {
        let size = if t.id == 273 || t.id == 324 { 4 * n as usize } else { t.data.len() };
        if size > 4 {
            blob_positions.push(Some(blob_at));
            blob_at += size + (size & 1);
        } else {
            blob_positions.push(None);
        }
    }
    let mut chunk_at = blob_at;
    let mut chunk_offsets = Vec::with_capacity(chunks.len());
    for c in &chunks {
        chunk_offsets.push(chunk_at as u32);
        chunk_at += c.len() + (c.len() & 1);
    }
    tags[offsets_tag_index].data = chunk_offsets.iter().flat_map(|v| u32b(*v)).collect();
    let mut out = Vec::with_capacity(chunk_at);
    out.extend_from_slice(if be { b"MM" } else { b"II" });
    out.extend_from_slice(&u16b(42));
    out.extend_from_slice(&u32b(ifd_at as u32));
    out.extend_from_slice(&u16b(tags.len() as u16));
    for (t, pos) in tags.iter().zip(&blob_positions) {
        out.extend_from_slice(&u16b(t.id));
        out.extend_from_slice(&u16b(t.kind));
        out.extend_from_slice(&u32b(t.count));
        match pos {
            Some(p) => out.extend_from_slice(&u32b(*p as u32)),
            None => {
                let mut d = t.data.clone();
                d.resize(4, 0);
                out.extend_from_slice(&d);
            }
        }
    }
    out.extend_from_slice(&u32b(0));
    for (t, pos) in tags.iter().zip(&blob_positions) {
        if pos.is_some() {
            out.extend_from_slice(&t.data);
            if t.data.len() & 1 == 1 {
                out.push(0);
            }
        }
    }
    for c in &chunks {
        out.extend_from_slice(c);
        if c.len() & 1 == 1 {
            out.push(0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dem() -> Raster {
        let mut r = Raster::filled(5, 4, 1000.0, 2000.0, 2.5, 0.0);
        for row in 0..4 {
            for col in 0..5 {
                r.set(col, row, 100.0 + row as f64 * 3.0 + col as f64 * 0.5);
            }
        }
        r.nodata = Some(-9999.0);
        r.set(2, 1, f64::NAN);
        r
    }

    fn check(back: &GeoTiff, exact: bool) {
        let r = &back.raster;
        assert_eq!((r.ncols, r.nrows), (5, 4));
        assert!((r.x0 - 1000.0).abs() < 1e-9 && (r.y0 - 2000.0).abs() < 1e-9 && (r.cell - 2.5).abs() < 1e-9, "{:?}", (r.x0, r.y0, r.cell));
        assert!(r.get(2, 1).unwrap().is_nan(), "nodata → NaN");
        // Integer bands truncate 107.5 to 107: allow exactly that.
        let tol = if exact { 1e-9 } else { 0.501 };
        assert!((r.get(0, 0).unwrap() - 100.0).abs() < tol);
        assert!((r.get(4, 3).unwrap() - 111.0).abs() < tol, "{}", r.get(4, 3).unwrap());
        assert!((r.get(3, 2).unwrap() - 107.5).abs() < tol);
    }

    #[test]
    fn every_layout_compression_and_predictor_round_trips() {
        let r = dem();
        let mut cases = Vec::new();
        for &big_endian in &[false, true] {
            for &compression in &[1u16, 5, 32773] {
                for &predictor in &[1u16, 2, 3] {
                    for &tile in &[None, Some(2usize), Some(4)] {
                        for &(format, bits) in &[(3u16, 32u16), (3, 64), (1, 16), (2, 16), (1, 8), (2, 32), (1, 32)] {
                            if predictor == 3 && format != 3 {
                                continue;
                            }
                            cases.push(EncodeOptions { compression, predictor, tile, rows_per_strip: 3, format, bits, big_endian });
                        }
                    }
                }
            }
        }
        assert!(cases.len() > 100);
        for o in cases {
            // An unsigned band cannot hold -9999: such DEMs use 0 (or the
            // type's maximum) as their no-data value.
            let mut r = r.clone();
            if o.format == 1 {
                r.nodata = Some(0.0);
            }
            let bytes = encode(&r, Some(2264), &o);
            let back = parse(&bytes).unwrap_or_else(|e| panic!("{o:?}: {e}"));
            let exact = o.format == 3;
            assert_eq!(back.raster.nodata, r.nodata, "{o:?}");
            check(&back, exact);
            assert_eq!(back.crs.as_ref().and_then(|c| c.epsg), Some(2264), "{o:?}");
            assert_eq!(back.crs.as_ref().unwrap().unit.name, "US survey foot");
            if o.tile.is_some() {
                assert!(back.info.contains("tiles"), "{}", back.info);
            } else {
                assert!(back.info.contains("strip"), "{}", back.info);
            }
        }
    }

    #[test]
    fn geographic_key_pixel_is_point_and_bad_files() {
        let mut r = dem();
        r.nodata = None;
        let bytes = encode(&r, Some(4269), &EncodeOptions::default());
        let back = parse(&bytes).unwrap();
        assert_eq!(back.crs.as_ref().and_then(|c| c.epsg), Some(4269));
        assert!(back.crs.as_ref().unwrap().is_geographic());
        assert_eq!(back.raster.nodata, None);
        // Flip RasterPixelIsArea (1) to IsPoint (2): the grid shifts half a cell.
        let ifd = parse_ifd(&bytes).unwrap();
        let e = ifd.get(34735).unwrap().clone();
        let mut b2 = bytes.clone();
        let dir = ifd.ints(34735).unwrap();
        let k = (0..dir[3] as usize).find(|k| dir[4 + 4 * k] == 1025).unwrap();
        let at = e.at + (4 + 4 * k + 3) * 2;
        b2[at..at + 2].copy_from_slice(&2u16.to_le_bytes());
        let shifted = parse(&b2).unwrap();
        assert!((shifted.raster.x0 - (1000.0 - 1.25)).abs() < 1e-9);
        assert!((shifted.raster.y1() - (2010.0 + 1.25)).abs() < 1e-9);
        // BigTIFF and non-TIFF.
        let mut big = bytes.clone();
        big[2..4].copy_from_slice(&43u16.to_le_bytes());
        assert!(parse(&big).unwrap_err().to_string().contains("BigTIFF"));
        assert!(parse(b"PNG.....").unwrap_err().to_string().contains("not a TIFF"));
        // No georeferencing.
        let mut none = bytes.clone();
        let e = ifd.get(33922).unwrap();
        // Zero the tiepoint tag id so it is not found.
        let tag_pos = (8 + 2..8 + 2 + 12 * ifd.entries.len()).step_by(12).find(|&p| u16::from_le_bytes([none[p], none[p + 1]]) == 33922).unwrap();
        let _ = e;
        none[tag_pos..tag_pos + 2].copy_from_slice(&65000u16.to_le_bytes());
        assert!(parse(&none).unwrap_err().to_string().contains("georeferencing"));
    }

    #[test]
    fn deflate_strips_decode_through_inflate() {
        // Build an uncompressed file, then replace its single strip with a
        // stored-block zlib stream and mark compression 8.
        let mut r = dem();
        r.nodata = None;
        let o = EncodeOptions { format: 1, bits: 8, ..Default::default() };
        let bytes = encode(&r, None, &o);
        let ifd = parse_ifd(&bytes).unwrap();
        let off = ifd.ints(273).unwrap()[0] as usize;
        let cnt = ifd.ints(279).unwrap()[0] as usize;
        let strip = bytes[off..off + cnt].to_vec();
        let mut z = vec![0x78, 0x01, 0x01];
        z.extend_from_slice(&(strip.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(strip.len() as u16)).to_le_bytes());
        z.extend_from_slice(&strip);
        z.extend_from_slice(&[0, 0, 0, 0]);
        let mut out = bytes[..off].to_vec();
        out.extend_from_slice(&z);
        let entries_at = 8 + 2;
        for i in 0..ifd.entries.len() {
            let p = entries_at + 12 * i;
            let tag = u16::from_le_bytes([out[p], out[p + 1]]);
            if tag == 259 {
                out[p + 8..p + 10].copy_from_slice(&8u16.to_le_bytes());
            }
            if tag == 279 {
                out[p + 8..p + 12].copy_from_slice(&(z.len() as u32).to_le_bytes());
            }
        }
        let back = parse(&out).unwrap();
        assert!(back.info.contains("Deflate"));
        assert_eq!(back.raster.get(4, 3), Some(111.0));
    }
}
