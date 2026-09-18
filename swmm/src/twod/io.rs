// SPDX-License-Identifier: GPL-3.0-or-later

//! The `<model>.2d.out` results file: a little-endian binary written
//! frame by frame so a run that is stopped, or crashes, still leaves a
//! readable file.
//!
//! ```text
//! header   "SS2D" u32 version(1) u32 ncols u32 nrows f64 x0 f64 y0 f64 cell
//!          u8 metric f64 frame_step_s u32 n_frames f64 dry_depth
//!          u32 n_nodes { u16 len, utf-8 name }*
//! frame*   f64 time_s  f32 depth[n]  f32 vx[n]  f32 vy[n]  f32 node[n_nodes]
//! trailer  f32 max_depth[n] f32 max_speed[n] f32 max_hazard[n] f32 arrival[n]
//!          u32 n_frames "2DSS"
//! ```
//!
//! `n_frames` in the header is 0 until the file is closed; a reader that
//! finds no `2DSS` tail counts frames from the file size and computes the
//! maxima from the frames.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::gis::raster::Raster;
use crate::{Error, Result};

use super::{Frame, Results};

const MAGIC: &[u8; 4] = b"SS2D";
const TAIL: &[u8; 4] = b"2DSS";
const VERSION: u32 = 1;
/// Byte offset of `n_frames` in the header.
const N_FRAMES_OFFSET: u64 = 4 + 4 + 4 + 4 + 8 + 8 + 8 + 1 + 8;

/// Incremental writer.
pub struct Writer {
    f: BufWriter<File>,
    ncells: usize,
    nnodes: usize,
    frames: u32,
}

impl Writer {
    pub fn create(
        path: &Path,
        grid: &Raster,
        metric: bool,
        frame_step_s: f64,
        dry_depth: f64,
        node_names: &[String],
    ) -> Result<Self> {
        let mut f = BufWriter::new(File::create(path)?);
        f.write_all(MAGIC)?;
        f.write_all(&VERSION.to_le_bytes())?;
        f.write_all(&(grid.ncols as u32).to_le_bytes())?;
        f.write_all(&(grid.nrows as u32).to_le_bytes())?;
        f.write_all(&grid.x0.to_le_bytes())?;
        f.write_all(&grid.y0.to_le_bytes())?;
        f.write_all(&grid.cell.to_le_bytes())?;
        f.write_all(&[u8::from(metric)])?;
        f.write_all(&frame_step_s.to_le_bytes())?;
        f.write_all(&0u32.to_le_bytes())?;
        f.write_all(&dry_depth.to_le_bytes())?;
        f.write_all(&(node_names.len() as u32).to_le_bytes())?;
        for name in node_names {
            let bytes = name.as_bytes();
            let len = bytes.len().min(u16::MAX as usize);
            f.write_all(&(len as u16).to_le_bytes())?;
            f.write_all(&bytes[..len])?;
        }
        f.flush()?;
        Ok(Self {
            f,
            ncells: grid.ncols * grid.nrows,
            nnodes: node_names.len(),
            frames: 0,
        })
    }

    pub fn frame(&mut self, time_s: f64, depth: &[f32], vx: &[f32], vy: &[f32], node: &[f32]) -> Result<()> {
        if depth.len() != self.ncells || vx.len() != self.ncells || vy.len() != self.ncells {
            return Err(Error::Format("2D frame has the wrong number of cells".into()));
        }
        if node.len() != self.nnodes {
            return Err(Error::Format("2D frame has the wrong number of node values".into()));
        }
        self.f.write_all(&time_s.to_le_bytes())?;
        write_f32s(&mut self.f, depth)?;
        write_f32s(&mut self.f, vx)?;
        write_f32s(&mut self.f, vy)?;
        write_f32s(&mut self.f, node)?;
        self.f.flush()?;
        self.frames += 1;
        Ok(())
    }

    pub fn frames(&self) -> usize {
        self.frames as usize
    }

    /// Write the maxima trailer, patch the frame count, and seal the file.
    pub fn finish(mut self, max_depth: &[f32], max_speed: &[f32], max_hazard: &[f32], arrival: &[f32]) -> Result<()> {
        for block in [max_depth, max_speed, max_hazard, arrival] {
            if block.len() != self.ncells {
                return Err(Error::Format("2D maxima block has the wrong number of cells".into()));
            }
            write_f32s(&mut self.f, block)?;
        }
        self.f.write_all(&self.frames.to_le_bytes())?;
        self.f.write_all(TAIL)?;
        self.f.flush()?;
        let file = self.f.get_mut();
        file.seek(SeekFrom::Start(N_FRAMES_OFFSET))?;
        file.write_all(&self.frames.to_le_bytes())?;
        file.flush()?;
        Ok(())
    }
}

fn write_f32s<W: Write>(w: &mut W, values: &[f32]) -> Result<()> {
    let mut buf = Vec::with_capacity(values.len() * 4);
    for v in values {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    w.write_all(&buf)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

struct Layout {
    ncols: usize,
    nrows: usize,
    x0: f64,
    y0: f64,
    cell: f64,
    metric: bool,
    frame_step_s: f64,
    dry_depth: f64,
    names: Vec<String>,
    header_len: u64,
    n_frames: usize,
    /// Byte offset of the maxima trailer when the file was sealed.
    trailer: Option<u64>,
}

impl Layout {
    fn ncells(&self) -> usize {
        self.ncols * self.nrows
    }

    fn frame_len(&self) -> u64 {
        8 + 4 * (3 * self.ncells() + self.names.len()) as u64
    }

    fn frame_offset(&self, i: usize) -> u64 {
        self.header_len + i as u64 * self.frame_len()
    }
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_f64<R: Read>(r: &mut R) -> Result<f64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(f64::from_le_bytes(b))
}

fn read_f32s<R: Read>(r: &mut R, n: usize) -> Result<Vec<f32>> {
    let mut buf = vec![0u8; n * 4];
    r.read_exact(&mut buf)?;
    let (chunks, _) = buf.as_chunks::<4>();
    Ok(chunks.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

fn read_layout(path: &Path) -> Result<Layout> {
    let mut f = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)
        .map_err(|_| Error::Format(format!("{}: not a 2D results file", path.display())))?;
    if &magic != MAGIC {
        return Err(Error::Format(format!(
            "{}: not a StormSewer 2D results file",
            path.display()
        )));
    }
    let version = read_u32(&mut f)?;
    if version != VERSION {
        return Err(Error::Format(format!(
            "{}: 2D results version {version} is not readable by this build",
            path.display()
        )));
    }
    let ncols = read_u32(&mut f)? as usize;
    let nrows = read_u32(&mut f)? as usize;
    let x0 = read_f64(&mut f)?;
    let y0 = read_f64(&mut f)?;
    let cell = read_f64(&mut f)?;
    let mut m = [0u8; 1];
    f.read_exact(&mut m)?;
    let frame_step_s = read_f64(&mut f)?;
    let header_frames = read_u32(&mut f)? as usize;
    let dry_depth = read_f64(&mut f)?;
    let n_nodes = read_u32(&mut f)? as usize;
    let mut names = Vec::with_capacity(n_nodes);
    let mut header_len = N_FRAMES_OFFSET + 4 + 8 + 4;
    for _ in 0..n_nodes {
        let mut l = [0u8; 2];
        f.read_exact(&mut l)?;
        let len = u16::from_le_bytes(l) as usize;
        let mut bytes = vec![0u8; len];
        f.read_exact(&mut bytes)?;
        names.push(String::from_utf8_lossy(&bytes).into_owned());
        header_len += 2 + len as u64;
    }
    let mut layout = Layout {
        ncols,
        nrows,
        x0,
        y0,
        cell,
        metric: m[0] != 0,
        frame_step_s,
        dry_depth,
        names,
        header_len,
        n_frames: 0,
        trailer: None,
    };
    let size = std::fs::metadata(path)?.len();
    let frame_len = layout.frame_len();
    let trailer_len = 16 * layout.ncells() as u64 + 8;
    // Sealed file: the tail names the frame count and the trailer sits
    // just before it.
    let mut sealed = false;
    if size >= header_len + trailer_len {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(size - 8))?;
        let mut tail = [0u8; 8];
        file.read_exact(&mut tail)?;
        if &tail[4..] == TAIL {
            let n = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]) as usize;
            if header_len + n as u64 * frame_len + trailer_len == size {
                layout.n_frames = n;
                layout.trailer = Some(size - trailer_len);
                sealed = true;
            }
        }
    }
    if !sealed {
        let _ = header_frames;
        layout.n_frames = ((size.saturating_sub(header_len)) / frame_len) as usize;
    }
    Ok(layout)
}

impl Results {
    pub fn open(path: &Path) -> Result<Self> {
        let l = read_layout(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            ncols: l.ncols,
            nrows: l.nrows,
            x0: l.x0,
            y0: l.y0,
            cell: l.cell,
            metric: l.metric,
            n_frames: l.n_frames,
            frame_step_s: l.frame_step_s,
            node_names: l.names,
        })
    }

    fn layout(&self) -> Result<Layout> {
        read_layout(&self.path)
    }

    fn raster(&self, data: Vec<f64>) -> Raster {
        Raster {
            ncols: self.ncols,
            nrows: self.nrows,
            x0: self.x0,
            y0: self.y0,
            cell: self.cell,
            nodata: Some(-9999.0),
            data,
        }
    }

    /// Whether the run wrote its maxima trailer (a stopped run has none).
    pub fn is_sealed(&self) -> bool {
        self.layout().map(|l| l.trailer.is_some()).unwrap_or(false)
    }

    /// The dry-depth threshold the run used.
    pub fn dry_depth(&self) -> Result<f64> {
        Ok(self.layout()?.dry_depth)
    }

    pub fn frame(&self, i: usize) -> Result<Frame> {
        let l = self.layout()?;
        if i >= l.n_frames {
            return Err(Error::NotFound(format!(
                "2D frame {i} is out of range (0..{})",
                l.n_frames
            )));
        }
        let mut f = BufReader::new(File::open(&self.path)?);
        f.seek(SeekFrom::Start(l.frame_offset(i)))?;
        let time_s = read_f64(&mut f)?;
        let n = l.ncells();
        let depth = read_f32s(&mut f, n)?;
        let vx = read_f32s(&mut f, n)?;
        let vy = read_f32s(&mut f, n)?;
        Ok(Frame {
            time_s,
            depth,
            vx,
            vy,
        })
    }

    /// The four maxima blocks: from the trailer when sealed, else scanned
    /// from the frames.
    fn maxima(&self) -> Result<[Vec<f32>; 4]> {
        let l = self.layout()?;
        let n = l.ncells();
        if let Some(off) = l.trailer {
            let mut f = BufReader::new(File::open(&self.path)?);
            f.seek(SeekFrom::Start(off))?;
            let a = read_f32s(&mut f, n)?;
            let b = read_f32s(&mut f, n)?;
            let c = read_f32s(&mut f, n)?;
            let d = read_f32s(&mut f, n)?;
            return Ok([a, b, c, d]);
        }
        let mut md = vec![0.0f32; n];
        let mut ms = vec![0.0f32; n];
        let mut mh = vec![0.0f32; n];
        let mut ar = vec![f32::NAN; n];
        for i in 0..l.n_frames {
            let fr = self.frame(i)?;
            for k in 0..n {
                let h = fr.depth[k];
                if h.is_nan() {
                    md[k] = f32::NAN;
                    continue;
                }
                let s = (fr.vx[k] * fr.vx[k] + fr.vy[k] * fr.vy[k]).sqrt();
                md[k] = md[k].max(h);
                ms[k] = ms[k].max(s);
                mh[k] = mh[k].max(h * s);
                if ar[k].is_nan() && h as f64 > l.dry_depth {
                    ar[k] = fr.time_s as f32;
                }
            }
        }
        Ok([md, ms, mh, ar])
    }

    fn maxima_raster(&self, which: usize) -> Result<Raster> {
        let blocks = self.maxima()?;
        let data = blocks[which].iter().map(|v| *v as f64).collect();
        Ok(self.raster(data))
    }

    pub fn max_depth(&self) -> Result<Raster> {
        self.maxima_raster(0)
    }

    pub fn max_velocity(&self) -> Result<Raster> {
        self.maxima_raster(1)
    }

    pub fn max_hazard(&self) -> Result<Raster> {
        self.maxima_raster(2)
    }

    pub fn arrival(&self) -> Result<Raster> {
        self.maxima_raster(3)
    }

    pub fn node_exchange(&self, node: &str) -> Result<Vec<(f64, f64)>> {
        let l = self.layout()?;
        let idx = l
            .names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(node))
            .ok_or_else(|| Error::NotFound(format!("2D results have no node interface {node}")))?;
        let mut f = File::open(&self.path)?;
        let mut out = Vec::with_capacity(l.n_frames);
        let node_off = 8 + 4 * 3 * l.ncells() as u64 + 4 * idx as u64;
        for i in 0..l.n_frames {
            let base = l.frame_offset(i);
            f.seek(SeekFrom::Start(base))?;
            let t = read_f64(&mut f)?;
            f.seek(SeekFrom::Start(base + node_off))?;
            let mut b = [0u8; 4];
            f.read_exact(&mut b)?;
            out.push((t, f32::from_le_bytes(b) as f64));
        }
        Ok(out)
    }

    pub fn sample(&self, frame: &Frame, x: f64, y: f64) -> Option<(f64, f64, f64)> {
        let (col, row) = self.raster(Vec::new()).cell_at(x, y)?;
        let i = row * self.ncols + col;
        let d = *frame.depth.get(i)?;
        if d.is_nan() {
            return None;
        }
        Some((d as f64, frame.vx[i] as f64, frame.vy[i] as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-twod-io-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{name}", std::process::id()))
    }

    #[test]
    fn round_trip_sealed_and_unsealed() {
        let grid = Raster::filled(3, 2, 10.0, 20.0, 5.0, 0.0);
        let path = tmp("rt.2d.out");
        let names = vec!["J1".to_string(), "J2".to_string()];
        let mut w = Writer::create(&path, &grid, true, 60.0, 0.01, &names).unwrap();
        let d0 = vec![0.0f32; 6];
        let v0 = vec![0.0f32; 6];
        w.frame(0.0, &d0, &v0, &v0, &[0.0, 0.0]).unwrap();
        let d1 = vec![0.0, 0.5, 0.0, 1.0, 0.0, 0.0];
        let vx = vec![0.0, 2.0, 0.0, 0.0, 0.0, 0.0];
        w.frame(60.0, &d1, &vx, &v0, &[1.5, -0.5]).unwrap();

        // Before finish: readable from the file size, maxima from frames.
        let r = Results::open(&path).unwrap();
        assert_eq!(r.n_frames, 2);
        assert!(!r.is_sealed());
        let md = r.max_depth().unwrap();
        assert_eq!(md.get(1, 0), Some(0.5));
        assert_eq!(md.get(0, 1), Some(1.0));
        let mv = r.max_velocity().unwrap();
        assert_eq!(mv.get(1, 0), Some(2.0));
        let ar = r.arrival().unwrap();
        assert_eq!(ar.get(1, 0), Some(60.0));
        assert!(ar.get(0, 0).unwrap().is_nan());

        let maxd = vec![0.0, 0.5, 0.0, 1.0, 0.0, 0.0];
        let maxs = vec![0.0, 2.0, 0.0, 0.0, 0.0, 0.0];
        let maxh = vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let arr = vec![f32::NAN, 60.0, f32::NAN, 60.0, f32::NAN, f32::NAN];
        w.finish(&maxd, &maxs, &maxh, &arr).unwrap();

        let r = Results::open(&path).unwrap();
        assert!(r.is_sealed());
        assert_eq!(r.n_frames, 2);
        assert_eq!(r.node_names, names);
        assert_eq!(r.frame_step_s, 60.0);
        assert!(r.metric);
        let f1 = r.frame(1).unwrap();
        assert_eq!(f1.time_s, 60.0);
        assert_eq!(f1.depth[3], 1.0);
        assert_eq!(r.max_hazard().unwrap().get(1, 0), Some(1.0));
        let ex = r.node_exchange("j2").unwrap();
        assert_eq!(ex, vec![(0.0, 0.0), (60.0, -0.5)]);
        // Cell (1,0) is x 15..20, y 25..30.
        assert_eq!(r.sample(&f1, 17.0, 27.0), Some((0.5, 2.0, 0.0)));
        assert_eq!(r.sample(&f1, 0.0, 0.0), None);
        assert!(r.frame(2).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
