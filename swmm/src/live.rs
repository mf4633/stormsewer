// SPDX-License-Identifier: GPL-3.0-or-later

//! Live results while the engine runs, and stopping it.
//!
//! Two things happen while `runswmm` is busy. The `.out` grows by one
//! record per reporting period (see the layout in [`crate::out`]), so a
//! reader that does not need the closing block can show where the run is
//! and what the network looks like at the latest period — that is
//! [`LivePoller`]. And the child process can be killed: [`CancelFlag`] is
//! the switch the UI flips and the worker thread watches through
//! [`crate::engine::Engine::run_with_cancel`].
//!
//! The engine writes through the C runtime's buffered `fwrite`, so records
//! reach the file in 4 KB bursts rather than one at a time: on a small
//! model the count can jump several periods at once and lag a little
//! behind the engine. It always catches up at the end.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::out::{self, Frame, OutputMetadata};
use crate::Result;

/// The switch a Stop flips. Shared between the UI and the run's worker
/// thread; the engine child process is killed as soon as it is set.
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask for the run to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// The flag the worker polls.
    pub fn atomic(&self) -> &AtomicBool {
        &self.0
    }
}

/// What the latest look at a growing `.out` found.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveSnapshot {
    /// Whole reporting periods on disk.
    pub periods: usize,
    /// Simulation time reached, seconds from the start: the last period's.
    pub sim_time_s: f64,
    /// Reporting step, seconds.
    pub report_step_s: f64,
    /// Nodes spilling at the latest period.
    pub flooding_nodes: usize,
    /// The deepest node at the latest period, and how deep.
    pub worst_node: Option<(String, f64)>,
    /// Peak link capacity fraction at the latest period.
    pub fullest_link: Option<(String, f64)>,
    /// The latest period's date, days since 1899-12-30.
    pub date_days: f64,
    pub file_size: u64,
}

/// Periodic reader of a `.out` that is still being written.
///
/// [`LivePoller::poll`] is cheap to call every frame: it looks at the file
/// size no more often than `interval` and only re-reads when the size grew.
/// The header is parsed once it is complete and kept; each new period costs
/// one `read_frame`.
#[derive(Debug)]
pub struct LivePoller {
    path: PathBuf,
    interval: Duration,
    last_poll: Option<Instant>,
    last_size: u64,
    meta: Option<OutputMetadata>,
    snapshot: Option<LiveSnapshot>,
    /// The last frame read, for a map that wants to draw it.
    frame: Option<Frame>,
    error: Option<String>,
}

/// How often the UI's poller looks at the file.
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(500);

impl LivePoller {
    pub fn new(out: impl Into<PathBuf>) -> Self {
        Self::with_interval(out, DEFAULT_INTERVAL)
    }

    pub fn with_interval(out: impl Into<PathBuf>, interval: Duration) -> Self {
        Self {
            path: out.into(),
            interval,
            last_poll: None,
            last_size: 0,
            meta: None,
            snapshot: None,
            frame: None,
            error: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn snapshot(&self) -> Option<&LiveSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn frame(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }

    /// The parsed header, once the engine has written it whole.
    pub fn meta(&self) -> Option<&OutputMetadata> {
        self.meta.as_ref()
    }

    /// Why the last read failed, if it did. A header not yet complete is
    /// not an error, just "nothing yet".
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Look at the file if `interval` has passed. Returns true when the
    /// snapshot changed.
    pub fn poll(&mut self) -> bool {
        if let Some(t) = self.last_poll {
            if t.elapsed() < self.interval {
                return false;
            }
        }
        self.last_poll = Some(Instant::now());
        self.poll_now()
    }

    /// Look at the file right now. Returns true when the snapshot changed.
    pub fn poll_now(&mut self) -> bool {
        let size = match std::fs::metadata(&self.path) {
            Ok(m) => m.len(),
            Err(_) => return false, // not created yet
        };
        if size == self.last_size {
            return false;
        }
        self.last_size = size;
        match self.read(size) {
            Ok(Some(snapshot)) => {
                let changed = self.snapshot.as_ref() != Some(&snapshot);
                self.snapshot = Some(snapshot);
                self.error = None;
                changed
            }
            Ok(None) => false,
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }

    fn read(&mut self, size: u64) -> Result<Option<LiveSnapshot>> {
        let meta = match self.meta.as_mut() {
            Some(m) => m,
            None => match out::read_metadata_partial(&self.path) {
                Ok((m, _)) => self.meta.insert(m),
                // The engine is still writing the header; try again later.
                Err(crate::Error::Format(_)) => return Ok(None),
                Err(e) => return Err(e),
            },
        };
        let periods = (size.saturating_sub(meta.output_offset) / meta.bytes_per_period()) as usize;
        meta.n_periods = periods;
        if periods == 0 {
            return Ok(Some(LiveSnapshot {
                periods: 0,
                sim_time_s: 0.0,
                report_step_s: f64::from(meta.report_step_s),
                flooding_nodes: 0,
                worst_node: None,
                fullest_link: None,
                date_days: meta.start_days,
                file_size: size,
            }));
        }
        let frame = out::read_frame(&self.path, meta, periods - 1)?;
        let flooding_nodes = frame.nodes.iter().filter(|n| n.flooding_now()).count();
        let worst_node = frame
            .nodes
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.depth.total_cmp(&b.1.depth))
            .map(|(i, n)| (meta.node_ids[i].clone(), n.depth));
        let fullest_link = frame
            .links
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.capacity.total_cmp(&b.1.capacity))
            .map(|(i, l)| (meta.link_ids[i].clone(), l.capacity));
        let snapshot = LiveSnapshot {
            periods,
            sim_time_s: frame.time_s,
            report_step_s: f64::from(meta.report_step_s),
            flooding_nodes,
            worst_node,
            fullest_link,
            date_days: frame.date_days,
            file_size: size,
        };
        self.frame = Some(frame);
        Ok(Some(snapshot))
    }
}

/// Days since 1899-12-30 for a civil date, SWMM's epoch.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468 + 25_569
}

fn parse_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split(['/', '-']).collect();
    if parts.len() != 3 {
        return None;
    }
    let m: u32 = parts[0].trim().parse().ok()?;
    let d: u32 = parts[1].trim().parse().ok()?;
    let y: i64 = parts[2].trim().parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn parse_time(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut secs = 0.0;
    for (p, m) in parts.iter().zip([3600.0, 60.0, 1.0]) {
        let v: f64 = p.trim().parse().ok()?;
        secs += v * m;
    }
    Some(secs)
}

/// Simulation length in seconds from a model's `[OPTIONS]`
/// (`START_DATE`/`START_TIME` to `END_DATE`/`END_TIME`), as the engine
/// computes `TotalDuration`. The `.out` header does not carry it, so a
/// progress bar has to read the model. Missing options take the engine's
/// defaults; a value that will not parse gives `None`.
pub fn duration_from_inp(text: &str) -> Option<f64> {
    let mut in_options = false;
    let (mut sd, mut st, mut ed, mut et) = (None, None, None, None);
    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_options = line.to_ascii_uppercase().starts_with("[OPTIONS]");
            continue;
        }
        if !in_options {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(key), Some(value)) = (it.next(), it.next()) else { continue };
        match key.to_ascii_uppercase().as_str() {
            "START_DATE" => sd = Some(parse_date(value)?),
            "START_TIME" => st = Some(parse_time(value)?),
            "END_DATE" => ed = Some(parse_date(value)?),
            "END_TIME" => et = Some(parse_time(value)?),
            _ => {}
        }
    }
    let sd = sd.unwrap_or_else(|| days_from_civil(2004, 1, 1));
    let start = sd as f64 * 86_400.0 + st.unwrap_or(0.0);
    let end = ed.unwrap_or(sd) as f64 * 86_400.0 + et.unwrap_or(0.0);
    Some((end - start).floor().max(0.0))
}

/// `1 h 05 min`, `12 h`, `45 s` — for a progress line.
pub fn format_hms(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h} h {m:02} min")
    } else if m > 0 {
        format!("{m} min {sec:02} s")
    } else {
        format!("{sec} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/results/Detention_Pond_Model.out")
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-swmm-tests").join("live");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The pond model's `.out`, replayed as the engine would write it:
    /// header first, then record by record, the closing block last. The
    /// poller must follow it, and at the end agree with the strict reader.
    #[test]
    fn poller_follows_a_growing_out_file() {
        let bytes = std::fs::read(fixture()).unwrap();
        let full = out::read_metadata(&fixture()).unwrap();
        let start = full.output_offset as usize;
        let stride = full.bytes_per_period() as usize;
        let live = scratch().join("growing.out");
        let _ = std::fs::remove_file(&live);

        let mut p = LivePoller::with_interval(&live, Duration::ZERO);
        assert!(!p.poll(), "no file yet");
        assert!(p.snapshot().is_none());

        // Half a header: not an error, nothing to show.
        std::fs::write(&live, &bytes[..start / 2]).unwrap();
        assert!(!p.poll());
        assert!(p.error().is_none(), "{:?}", p.error());
        assert!(p.meta().is_none());

        // Header complete, no records.
        std::fs::write(&live, &bytes[..start]).unwrap();
        assert!(p.poll());
        assert_eq!(p.snapshot().unwrap().periods, 0);
        assert!(p.meta().is_some());

        // Ten records and a bit.
        std::fs::write(&live, &bytes[..start + 10 * stride + 7]).unwrap();
        assert!(p.poll());
        let s = p.snapshot().unwrap().clone();
        assert_eq!(s.periods, 10);
        assert_eq!(s.sim_time_s, 10.0 * 300.0);
        assert_eq!(s.report_step_s, 300.0);
        assert!(s.worst_node.is_some());
        assert_eq!(p.frame().unwrap().period, 9);
        // The same size again is not a change.
        assert!(!p.poll());

        // Whole file, closing block included: the count is the real one.
        std::fs::write(&live, &bytes).unwrap();
        assert!(p.poll());
        let s = p.snapshot().unwrap();
        assert_eq!(s.periods, full.n_periods);
        let strict = out::OutputFile::open(&live).unwrap();
        let last = strict.frame(full.n_periods - 1).unwrap();
        assert_eq!(p.frame(), Some(&last));
        let deepest = last
            .nodes
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.depth.total_cmp(&b.1.depth))
            .map(|(i, n)| (full.node_ids[i].clone(), n.depth));
        assert_eq!(s.worst_node, deepest);
        assert_eq!(s.flooding_nodes, last.nodes.iter().filter(|n| n.flooding_now()).count());
    }

    #[test]
    fn poll_respects_its_interval() {
        let live = scratch().join("interval.out");
        std::fs::write(&live, std::fs::read(fixture()).unwrap()).unwrap();
        let mut p = LivePoller::with_interval(&live, Duration::from_secs(3600));
        assert!(p.poll(), "the first look always happens");
        std::fs::write(&live, b"changed").unwrap();
        assert!(!p.poll(), "too soon to look again");
        assert!(p.snapshot().is_some(), "the earlier snapshot stands");
    }

    #[test]
    fn cancel_flag_is_shared() {
        let a = CancelFlag::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
        assert!(b.atomic().load(Ordering::SeqCst));
    }

    #[test]
    fn duration_comes_from_the_options_block() {
        let inp = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/epa-samples/Detention_Pond_Model.inp"),
        )
        .unwrap();
        assert_eq!(duration_from_inp(&inp), Some(43_200.0));
        assert_eq!(
            duration_from_inp("[OPTIONS]\nSTART_DATE 12/31/2019\nSTART_TIME 6\nEND_DATE 01/02/2020\nEND_TIME 06:30\n"),
            Some(2.0 * 86_400.0 + 1_800.0)
        );
        assert_eq!(duration_from_inp("[OPTIONS]\nFLOW_UNITS CFS\n"), Some(0.0));
        assert_eq!(duration_from_inp("[OPTIONS]\nEND_TIME later\n"), None);
        assert_eq!(days_from_civil(1899, 12, 30), 0);
        assert_eq!(days_from_civil(2007, 1, 1), 39_083, "the pond model's start, as the engine stores it");
    }

    #[test]
    fn hms_formats_for_a_progress_line() {
        assert_eq!(format_hms(0.0), "0 s");
        assert_eq!(format_hms(45.0), "45 s");
        assert_eq!(format_hms(125.0), "2 min 05 s");
        assert_eq!(format_hms(3900.0), "1 h 05 min");
        assert_eq!(format_hms(43_200.0), "12 h 00 min");
    }
}
