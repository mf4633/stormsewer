// SPDX-License-Identifier: GPL-3.0-or-later

//! Observed series: a delimited text table read the way `rain.rs` reads
//! rainfall (delimiter, header and date formats auto-detected), attached
//! to one model object and reported variable.

use serde::{Deserialize, Serialize};

use crate::out::{LinkVariable, NodeVariable, OutputFile, Series, SubcatchVariable};
use crate::rain::{detect_delimiter, normalize_date, normalize_time};

/// What kind of object the observations belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetKind {
    Node,
    Link,
    Subcatchment,
    /// Total inflow at an outfall node — the system outflow.
    System,
}

/// The reported variable an observed series is compared with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Variable {
    NodeDepth,
    NodeHead,
    NodeTotalInflow,
    NodeFlooding,
    LinkFlow,
    LinkDepth,
    LinkVelocity,
    SubcatchRunoff,
    SystemInflow,
}

impl Variable {
    pub const ALL: [Variable; 9] = [
        Variable::NodeDepth,
        Variable::NodeHead,
        Variable::NodeTotalInflow,
        Variable::NodeFlooding,
        Variable::LinkFlow,
        Variable::LinkDepth,
        Variable::LinkVelocity,
        Variable::SubcatchRunoff,
        Variable::SystemInflow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::NodeDepth => "Node depth",
            Self::NodeHead => "Node head",
            Self::NodeTotalInflow => "Node total inflow",
            Self::NodeFlooding => "Node flooding",
            Self::LinkFlow => "Link flow",
            Self::LinkDepth => "Link depth",
            Self::LinkVelocity => "Link velocity",
            Self::SubcatchRunoff => "Subcatchment runoff",
            Self::SystemInflow => "System inflow at outfall",
        }
    }

    pub fn kind(self) -> TargetKind {
        match self {
            Self::NodeDepth | Self::NodeHead | Self::NodeTotalInflow | Self::NodeFlooding => {
                TargetKind::Node
            }
            Self::LinkFlow | Self::LinkDepth | Self::LinkVelocity => TargetKind::Link,
            Self::SubcatchRunoff => TargetKind::Subcatchment,
            Self::SystemInflow => TargetKind::System,
        }
    }

    /// Read this variable for `id` from a results file.
    pub fn read(self, file: &OutputFile, id: &str) -> crate::Result<Series> {
        match self {
            Self::NodeDepth => file.node(id, NodeVariable::Depth),
            Self::NodeHead => file.node(id, NodeVariable::Head),
            Self::NodeTotalInflow | Self::SystemInflow => {
                file.node(id, NodeVariable::TotalInflow)
            }
            Self::NodeFlooding => file.node(id, NodeVariable::Flooding),
            Self::LinkFlow => file.link(id, LinkVariable::Flow),
            Self::LinkDepth => file.link(id, LinkVariable::Depth),
            Self::LinkVelocity => file.link(id, LinkVariable::Velocity),
            Self::SubcatchRunoff => file.subcatchment(id, SubcatchVariable::Runoff),
        }
    }
}

/// When an observation was made.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ObsTime {
    /// Days since 1899-12-30 (SWMM's epoch), from a dated row.
    Absolute(f64),
    /// Seconds from the simulation start, from a time-only or numeric row.
    Relative(f64),
}

/// One observed series, in the model's units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservedSeries {
    pub name: String,
    pub id: String,
    pub variable: Variable,
    pub times: Vec<ObsTime>,
    pub values: Vec<f64>,
    /// Where the data came from, for the report.
    #[serde(default)]
    pub source: String,
}

impl ObservedSeries {
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The observation times as seconds from a simulation that starts at
    /// `start_days` (SWMM days).
    pub fn seconds_from(&self, start_days: f64) -> Vec<f64> {
        self.times
            .iter()
            .map(|t| match t {
                ObsTime::Absolute(d) => (d - start_days) * 86_400.0,
                ObsTime::Relative(s) => *s,
            })
            .collect()
    }

    pub fn label(&self) -> String {
        format!("{} · {}", self.id, self.variable.label())
    }
}

/// Days since 1899-12-30 for a civil date (the inverse of
/// `out::decode_datetime`'s date part). Howard Hinnant's `days_from_civil`.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> f64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let unix_days = era * 146_097 + doe as i64 - 719_468;
    // 1899-12-30 is 25 569 days before 1970-01-01.
    (unix_days + 25_569) as f64
}

/// `MM/DD/YYYY` (as `rain::normalize_date` writes it) to SWMM days.
fn date_days(mdy: &str) -> Option<f64> {
    let mut it = mdy.split('/');
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    let y: i64 = it.next()?.parse().ok()?;
    Some(days_from_civil(y, m, d))
}

/// `H:MM[:SS]` to seconds.
fn time_seconds(hms: &str) -> Option<f64> {
    let parts: Vec<&str> = hms.split(':').collect();
    let h: f64 = parts.first()?.parse().ok()?;
    let m: f64 = parts.get(1)?.parse().ok()?;
    let s: f64 = parts
        .get(2)
        .map(|s| s.parse().unwrap_or(0.0))
        .unwrap_or(0.0);
    Some(h * 3600.0 + m * 60.0 + s)
}

fn split(line: &str, delim: &str) -> Vec<String> {
    if delim == " " {
        return line.split_whitespace().map(str::to_string).collect();
    }
    let d = delim.chars().next().unwrap_or(',');
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if c == d && !quoted {
            cells.push(std::mem::take(&mut cur).trim().to_string());
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

/// What the leading cells of a row mean.
enum Lead {
    /// A dated stamp (SWMM days) and how many leading cells it took.
    Dated(f64, usize),
    /// A relative stamp (seconds) and how many leading cells it took.
    Relative(f64, usize),
}

/// The elapsed-time unit a numeric first column is in, from the header.
#[derive(Clone, Copy)]
enum ElapsedUnit {
    Seconds,
    Minutes,
    Hours,
}

fn lead_of(cells: &[String], elapsed: ElapsedUnit) -> Option<Lead> {
    let c0 = cells.first()?;
    // A date and a time in one cell, separated by a space or a `T`.
    let combined = c0.trim().replace('T', " ");
    if let Some((d, t)) = combined.split_once(' ') {
        if let (Some(d), Some(t)) = (normalize_date(d), normalize_time(t.trim())) {
            return Some(Lead::Dated(
                date_days(&d)? + time_seconds(&t)? / 86_400.0,
                1,
            ));
        }
    }
    if let Some(d) = normalize_date(c0) {
        let days = date_days(&d)?;
        if let Some(t) = cells.get(1).and_then(|t| normalize_time(t)) {
            return Some(Lead::Dated(days + time_seconds(&t)? / 86_400.0, 2));
        }
        return Some(Lead::Dated(days, 1));
    }
    if let Some(t) = normalize_time(c0) {
        return Some(Lead::Relative(time_seconds(&t)?, 1));
    }
    let n: f64 = c0.trim().parse().ok()?;
    let secs = match elapsed {
        ElapsedUnit::Seconds => n,
        ElapsedUnit::Minutes => n * 60.0,
        ElapsedUnit::Hours => n * 3600.0,
    };
    Some(Lead::Relative(secs, 1))
}

/// What a text parsed into.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedImport {
    pub times: Vec<ObsTime>,
    pub values: Vec<f64>,
    pub had_header: bool,
    pub warnings: Vec<String>,
}

/// Read `datetime,value` (or `date,time,value`, `time,value`,
/// `seconds,value`) text. The delimiter is detected from the first line;
/// a header row is any row whose value cell is not a number. A numeric
/// first column is elapsed time in seconds unless the header says
/// `min`/`hour`/`hr`. Values must be in the model's units.
pub fn parse_observed(text: &str) -> Result<ObservedImport, String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with(';') && !t.starts_with('#')
        })
        .collect();
    let Some(first) = lines.first() else {
        return Err("nothing to import".into());
    };
    let delim = detect_delimiter(first);
    let header = split(first, delim);
    let mut elapsed = ElapsedUnit::Seconds;
    let mut had_header = false;
    if let Some(h0) = header.first() {
        let h = h0.to_ascii_lowercase();
        if h.contains("hour") || h.contains("hr") {
            elapsed = ElapsedUnit::Hours;
        } else if h.contains("min") {
            elapsed = ElapsedUnit::Minutes;
        }
    }
    let mut times = Vec::new();
    let mut values = Vec::new();
    let mut warnings = Vec::new();
    let mut bad = 0usize;
    for (i, line) in lines.iter().enumerate() {
        let cells = split(line, delim);
        let Some(lead) = lead_of(&cells, elapsed) else {
            if i == 0 {
                had_header = true;
            } else {
                bad += 1;
            }
            continue;
        };
        let (t, skip) = match lead {
            Lead::Dated(d, n) => (ObsTime::Absolute(d), n),
            Lead::Relative(s, n) => (ObsTime::Relative(s), n),
        };
        let Some(v) = cells.get(skip).and_then(|v| v.trim().parse::<f64>().ok()) else {
            if i == 0 {
                had_header = true;
            } else {
                bad += 1;
            }
            continue;
        };
        times.push(t);
        values.push(v);
    }
    if bad > 0 {
        warnings.push(format!("{bad} row(s) had no readable time and value"));
    }
    if values.is_empty() {
        return Err("no rows with a time and a value".into());
    }
    let mixed = times.iter().any(|t| matches!(t, ObsTime::Absolute(_)))
        && times.iter().any(|t| matches!(t, ObsTime::Relative(_)));
    if mixed {
        return Err("rows mix dated and time-only stamps".into());
    }
    Ok(ObservedImport {
        times,
        values,
        had_header,
        warnings,
    })
}
