// SPDX-License-Identifier: GPL-3.0-or-later

//! Rain and flow time-series import: a delimited text table with date,
//! time and one or more value columns (one `[TIMESERIES]` per column, so
//! a radar grid of a hundred gages is one paste), and NOAA GHCN-Daily
//! station csv (`STATION, DATE, PRCP` in tenths of a millimetre) as daily
//! `VOLUME` series. Plus the writer for SWMM's external time-series file
//! (`date time value` lines, `;` comments), for series too long to keep in
//! the `.inp`.
//!
//! Formats accepted (auto-detected, no options to set):
//!
//! * delimiter: comma, tab, semicolon, or runs of spaces;
//! * an optional header row (any row whose value cells are not numbers);
//! * dates `M/D/YYYY`, `MM/DD/YYYY`, `YYYY-MM-DD`, `YYYYMMDD`, `D.M.YYYY`;
//! * times `H:MM`, `HH:MM:SS`, or a combined `date time` cell;
//! * a table with no date column (`time value …`) is a relative series.
//!
//! SWMM object names are limited (the GUI's limit, and the reason a GHCN
//! station's 40-character name plus its id will not fit): names longer
//! than [`MAX_NAME`] are cut and suffixed, and the import says so.

use std::collections::BTreeMap;

use crate::doc::build::{self};
use crate::doc::{Command, InpDoc, ObjectKind, SeriesPoint};

/// The longest name written; longer ones are cut and numbered.
pub const MAX_NAME: usize = 50;

/// The unit rainfall depths are wanted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthUnit {
    Inches,
    Millimetres,
}

impl DepthUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inches => "in",
            Self::Millimetres => "mm",
        }
    }
}

/// One series read from the text.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedSeries {
    /// The name it will be written under (already cut to [`MAX_NAME`] and
    /// made safe for a `.inp`).
    pub name: String,
    /// Where it came from: the column header or the station.
    pub source: String,
    pub points: Vec<SeriesPoint>,
}

/// What kind of table was recognised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `date time value…` or `time value…`.
    Table,
    /// NOAA GHCN-Daily station csv.
    GhcnDaily,
}

/// The result of reading a text.
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    pub layout: Layout,
    pub delimiter: &'static str,
    pub had_header: bool,
    pub series: Vec<ImportedSeries>,
    /// Rows that did not parse, and names that were cut.
    pub warnings: Vec<String>,
    /// The factor applied to every value, and why (GHCN only).
    pub factor: Option<(f64, String)>,
    /// Whether the series are daily totals (VOLUME, 24-hour interval).
    pub daily: bool,
}

impl Import {
    pub fn point_count(&self) -> usize {
        self.series.iter().map(|s| s.points.len()).sum()
    }
}

// ---------------------------------------------------------------------------
// Cells
// ---------------------------------------------------------------------------

/// The delimiter a line uses: the most frequent of tab, comma, semicolon;
/// else runs of whitespace.
pub fn detect_delimiter(line: &str) -> &'static str {
    let tabs = line.matches('\t').count();
    let commas = line.matches(',').count();
    let semis = line.matches(';').count();
    if tabs > 0 && tabs >= commas && tabs >= semis {
        "\t"
    } else if commas > 0 && commas >= semis {
        ","
    } else if semis > 0 {
        ";"
    } else {
        " "
    }
}

/// Split a line on `delim`, honouring double quotes (a GHCN station name
/// is `"CHARLOTTE DOUGLAS AIRPORT, NC US"`, comma and all).
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

fn month_days(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn valid_date(y: i64, m: i64, d: i64) -> bool {
    (1900..=2200).contains(&y) && (1..=12).contains(&m) && d >= 1 && d <= month_days(y, m)
}

/// A date cell as SWMM's `MM/DD/YYYY`, or `None` when it is not a date.
pub fn normalize_date(s: &str) -> Option<String> {
    let s = s.trim();
    let parts: Vec<&str> = if s.contains('/') {
        s.split('/').collect()
    } else if s.contains('-') {
        s.split('-').collect()
    } else if s.contains('.') {
        s.split('.').collect()
    } else if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
        vec![&s[..4], &s[4..6], &s[6..]]
    } else {
        return None;
    };
    if parts.len() != 3 {
        return None;
    }
    let n: Vec<i64> = parts
        .iter()
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let (y, m, d) = if parts[0].trim().len() == 4 {
        // YYYY-MM-DD
        (n[0], n[1], n[2])
    } else if s.contains('.') {
        // D.M.YYYY
        (n[2], n[1], n[0])
    } else {
        // M/D/YYYY (two-digit years are 2000-based)
        let y = if parts[2].trim().len() <= 2 {
            n[2] + 2000
        } else {
            n[2]
        };
        (y, n[0], n[1])
    };
    valid_date(y, m, d).then(|| format!("{m:02}/{d:02}/{y:04}"))
}

/// A time cell as `H:MM` or `H:MM:SS`, or `None`.
pub fn normalize_time(s: &str) -> Option<String> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let h: u32 = parts[0].trim().parse().ok()?;
    let m: u32 = parts[1].trim().parse().ok()?;
    if m >= 60 || h > 48 {
        return None;
    }
    match parts.get(2) {
        Some(sec) => {
            let sec: f64 = sec.trim().parse().ok()?;
            if !(0.0..60.0).contains(&sec) {
                return None;
            }
            if sec == 0.0 {
                Some(format!("{h}:{m:02}"))
            } else {
                Some(format!("{h}:{m:02}:{:02}", sec as u32))
            }
        }
        None => Some(format!("{h}:{m:02}")),
    }
}

/// A `date time` cell split in two, when it is one.
fn split_datetime(s: &str) -> Option<(String, String)> {
    let s = s.trim().replace('T', " ");
    let (d, t) = s.split_once(' ')?;
    Some((normalize_date(d)?, normalize_time(t.trim())?))
}

fn is_number(s: &str) -> bool {
    !s.trim().is_empty() && s.trim().parse::<f64>().is_ok()
}

fn format_value(v: f64) -> String {
    if v.abs() < 1e-12 {
        return "0".into();
    }
    let s = format!("{v:.5}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A name that survives the `.inp`: no whitespace, quotes, `;` or `[`, at
/// most [`MAX_NAME`] characters (cut ones get `~n`), unique among `taken`
/// and the document's series. Returns the name and whether it was cut.
pub fn safe_name(raw: &str, doc: &InpDoc, taken: &[String]) -> (String, bool) {
    let mut s: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_whitespace() || c == '"' || c == ';' || c == '[' || c == ']' {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.is_empty() {
        s = "Series".into();
    }
    let cut = s.chars().count() > MAX_NAME;
    if cut {
        s = s.chars().take(MAX_NAME - 3).collect();
    }
    let free = |c: &str| {
        !doc.contains("TIMESERIES", c) && !taken.iter().any(|t| t.eq_ignore_ascii_case(c))
    };
    if !cut && free(&s) {
        return (s, false);
    }
    let name = (1u32..)
        .map(|n| format!("{s}~{n}"))
        .find(|c| free(c))
        .expect("a free name exists");
    (name, cut)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Read a pasted or loaded text. `unit` is the unit the model's rainfall
/// is in (GHCN values are converted to it; table values pass through).
pub fn parse(text: &str, doc: &InpDoc, unit: DepthUnit) -> Result<Import, String> {
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
    let upper: Vec<String> = header.iter().map(|h| h.to_ascii_uppercase()).collect();
    let col = |name: &str| upper.iter().position(|h| h == name);
    if let (Some(si), Some(di), Some(pi)) = (col("STATION"), col("DATE"), col("PRCP")) {
        return parse_ghcn(&lines[1..], delim, si, col("NAME"), di, pi, doc, unit);
    }
    parse_table(&lines, delim, doc)
}

#[allow(clippy::too_many_arguments)]
fn parse_ghcn(
    rows: &[&str],
    delim: &'static str,
    station: usize,
    name: Option<usize>,
    date: usize,
    prcp: usize,
    doc: &InpDoc,
    unit: DepthUnit,
) -> Result<Import, String> {
    let (factor, why) = match unit {
        DepthUnit::Millimetres => (
            0.1,
            "GHCN PRCP is tenths of a millimetre: × 0.1 → mm".to_string(),
        ),
        DepthUnit::Inches => (
            0.1 / 25.4,
            "GHCN PRCP is tenths of a millimetre: × 0.1 / 25.4 → inches".to_string(),
        ),
    };
    let mut by_station: BTreeMap<String, (String, Vec<SeriesPoint>)> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut bad = 0;
    for line in rows {
        let cells = split(line, delim);
        let (Some(id), Some(d), Some(v)) = (cells.get(station), cells.get(date), cells.get(prcp))
        else {
            bad += 1;
            continue;
        };
        let (Some(d), Ok(v)) = (normalize_date(d), v.trim().parse::<f64>()) else {
            if v.trim().is_empty() {
                // A missing PRCP is a gap, not an error.
                continue;
            }
            bad += 1;
            continue;
        };
        let label = name
            .and_then(|i| cells.get(i))
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| id.clone());
        let entry = by_station
            .entry(id.clone())
            .or_insert_with(|| (label, Vec::new()));
        entry.1.push(SeriesPoint {
            date: Some(d),
            time: "0:00".into(),
            value: format_value(v * factor),
        });
    }
    if bad > 0 {
        warnings.push(format!("{bad} row(s) had no readable STATION/DATE/PRCP"));
    }
    if by_station.is_empty() {
        return Err("no GHCN rows with a date and a PRCP value".into());
    }
    let mut taken: Vec<String> = Vec::new();
    let mut series = Vec::new();
    for (id, (label, points)) in by_station {
        // The id leads so a cut name still says which station it is.
        let raw = if label == id {
            id.clone()
        } else {
            format!("{id} {label}")
        };
        let (n, cut) = safe_name(&raw, doc, &taken);
        if cut {
            warnings.push(format!(
                "\"{raw}\" is longer than {MAX_NAME} characters (SWMM's name limit); written as {n}"
            ));
        }
        taken.push(n.clone());
        series.push(ImportedSeries {
            name: n,
            source: format!("{label} ({id})"),
            points,
        });
    }
    Ok(Import {
        layout: Layout::GhcnDaily,
        delimiter: delim,
        had_header: true,
        series,
        warnings,
        factor: Some((factor, why)),
        daily: true,
    })
}

/// How the leading columns of a table row are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lead {
    DateTime,
    Combined,
    TimeOnly,
}

fn lead_of(cells: &[String]) -> Option<(Lead, usize)> {
    let c0 = cells.first()?;
    if split_datetime(c0).is_some() {
        return Some((Lead::Combined, 1));
    }
    if normalize_date(c0).is_some() {
        let c1 = cells.get(1)?;
        if normalize_time(c1).is_some() {
            return Some((Lead::DateTime, 2));
        }
        // A date with no time: a daily value.
        return Some((Lead::DateTime, 1));
    }
    if normalize_time(c0).is_some() {
        return Some((Lead::TimeOnly, 1));
    }
    None
}

fn parse_table(lines: &[&str], delim: &'static str, doc: &InpDoc) -> Result<Import, String> {
    let first = split(lines[0], delim);
    let had_header = lead_of(&first).is_none() || first.iter().skip(1).all(|c| !is_number(c));
    let data_start = usize::from(had_header);
    let Some(probe) = lines.get(data_start).map(|l| split(l, delim)) else {
        return Err("a header but no data rows".into());
    };
    let Some((lead, skip)) = lead_of(&probe) else {
        return Err(format!(
            "the first data row does not start with a date and time, or a time: {:?}",
            lines[data_start]
        ));
    };
    let n_values = probe.len().saturating_sub(skip);
    if n_values == 0 {
        return Err("no value columns after the date and time".into());
    }
    let mut names: Vec<String> = (0..n_values)
        .map(|i| {
            if had_header {
                first.get(skip + i).cloned().filter(|h| !h.is_empty())
            } else {
                None
            }
            .unwrap_or_else(|| format!("Series{}", i + 1))
        })
        .collect();
    let mut warnings = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    for n in &mut names {
        let (safe, cut) = safe_name(n, doc, &taken);
        if cut {
            warnings.push(format!(
                "column \"{n}\" is longer than {MAX_NAME} characters; written as {safe}"
            ));
        }
        taken.push(safe.clone());
        *n = safe;
    }
    let mut series: Vec<ImportedSeries> = names
        .iter()
        .enumerate()
        .map(|(i, n)| ImportedSeries {
            name: n.clone(),
            source: if had_header {
                first.get(skip + i).cloned().unwrap_or_default()
            } else {
                format!("column {}", skip + i + 1)
            },
            points: Vec::new(),
        })
        .collect();
    let mut bad = 0;
    let mut daily = lead == Lead::DateTime && skip == 1;
    for line in &lines[data_start..] {
        let cells = split(line, delim);
        let (date, time, vals) = match lead {
            Lead::Combined => match cells.first().and_then(|c| split_datetime(c)) {
                Some((d, t)) => (Some(d), t, &cells[1..]),
                None => {
                    bad += 1;
                    continue;
                }
            },
            Lead::DateTime => {
                let Some(d) = cells.first().and_then(|c| normalize_date(c)) else {
                    bad += 1;
                    continue;
                };
                if skip == 2 {
                    let Some(t) = cells.get(1).and_then(|c| normalize_time(c)) else {
                        bad += 1;
                        continue;
                    };
                    (Some(d), t, &cells[2..])
                } else {
                    (Some(d), "0:00".to_string(), &cells[1..])
                }
            }
            Lead::TimeOnly => {
                let Some(t) = cells.first().and_then(|c| normalize_time(c)) else {
                    bad += 1;
                    continue;
                };
                daily = false;
                (None, t, &cells[1..])
            }
        };
        if vals.len() < n_values {
            bad += 1;
            continue;
        }
        for (i, s) in series.iter_mut().enumerate() {
            let v = vals[i].trim();
            if v.is_empty() {
                continue;
            }
            let Ok(x) = v.parse::<f64>() else {
                bad += 1;
                break;
            };
            s.points.push(SeriesPoint {
                date: date.clone(),
                time: time.clone(),
                value: format_value(x),
            });
        }
    }
    if bad > 0 {
        warnings.push(format!("{bad} row(s) skipped: not `date time value…`"));
    }
    if series.iter().all(|s| s.points.is_empty()) {
        return Err("no rows parsed".into());
    }
    Ok(Import {
        layout: Layout::Table,
        delimiter: delim,
        had_header,
        series,
        warnings,
        factor: None,
        daily,
    })
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Rain gages to create for the imported series.
#[derive(Clone, Debug, PartialEq)]
pub struct GageOptions {
    /// `INTENSITY`, `VOLUME` or `CUMULATIVE`.
    pub format: String,
    /// `h:mm` recording interval.
    pub interval: String,
    /// Where the gages' symbols go: the first, and the spacing between
    /// them, in map units (`None` leaves them off the map).
    pub symbols: Option<((f64, f64), f64)>,
}

/// The one-step batch that adds every series (rows `name [date] time
/// value`) and, when asked, a `[RAINGAGES]` row per series named after
/// it (`RG_<series>`, made unique).
pub fn commands(
    doc: &InpDoc,
    import: &Import,
    gages: Option<&GageOptions>,
) -> (Command, Vec<String>) {
    let mut cmds = Vec::new();
    let mut gage_names = Vec::new();
    let mut reserved: Vec<String> = Vec::new();
    for (k, s) in import.series.iter().enumerate() {
        for p in &s.points {
            let mut fields = vec![s.name.clone()];
            if let Some(d) = &p.date {
                fields.push(d.clone());
            }
            fields.push(p.time.clone());
            fields.push(p.value.clone());
            cmds.push(Command::AddRow {
                section: "TIMESERIES".into(),
                fields,
                comment: None,
            });
        }
        if let Some(g) = gages {
            let base = format!("RG_{}", s.name);
            let base: String = base.chars().take(MAX_NAME).collect();
            let gage = build::unique_name_from(doc, ObjectKind::Gage, &base, &reserved);
            reserved.push(gage.clone());
            cmds.push(Command::AddRow {
                section: "RAINGAGES".into(),
                fields: vec![
                    gage.clone(),
                    g.format.to_ascii_uppercase(),
                    g.interval.clone(),
                    "1.0".into(),
                    "TIMESERIES".into(),
                    s.name.clone(),
                ],
                comment: None,
            });
            if let Some(((x, y), dx)) = g.symbols {
                cmds.push(Command::MoveGage {
                    name: gage.clone(),
                    x: x + dx * k as f64,
                    y,
                });
            }
            gage_names.push(gage);
        }
    }
    (Command::Batch(cmds), gage_names)
}

/// The text of a SWMM external time-series file for `points`: a comment
/// header, then `date time value` (or `time value`) per line, which the
/// engine reads through `Name FILE "path"`.
pub fn external_file_text(name: &str, points: &[SeriesPoint]) -> String {
    let mut out = format!(";{name}\n;date time value\n");
    for p in points {
        match &p.date {
            Some(d) => out.push_str(&format!("{d} {} {}\n", p.time, p.value)),
            None => out.push_str(&format!("{} {}\n", p.time, p.value)),
        }
    }
    out
}

/// The batch that turns an in-file series into a `FILE` reference: every
/// data row replaced by `Name FILE "path"`.
pub fn externalize_command(doc: &InpDoc, name: &str, path: &str) -> Command {
    let mut cmds = vec![Command::DeleteObject {
        section: "TIMESERIES".into(),
        name: name.into(),
    }];
    let spelled = doc
        .find("TIMESERIES", name)
        .map(|(_, r)| r.fields[0].clone())
        .unwrap_or_else(|| name.to_string());
    cmds.push(Command::AddRow {
        section: "TIMESERIES".into(),
        fields: vec![
            spelled,
            "FILE".into(),
            format!("\"{}\"", path.replace('"', "")),
        ],
        comment: None,
    });
    Command::Batch(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> InpDoc {
        InpDoc::parse("")
    }

    #[test]
    fn dates_times_and_delimiters_normalise() {
        assert_eq!(normalize_date("1/5/2024").as_deref(), Some("01/05/2024"));
        assert_eq!(normalize_date("2024-01-05").as_deref(), Some("01/05/2024"));
        assert_eq!(normalize_date("20240105").as_deref(), Some("01/05/2024"));
        assert_eq!(normalize_date("5.1.2024").as_deref(), Some("01/05/2024"));
        assert_eq!(normalize_date("1/5/24").as_deref(), Some("01/05/2024"));
        assert_eq!(normalize_date("13/5/2024"), None);
        assert_eq!(normalize_date("2/30/2024"), None);
        assert_eq!(normalize_date("0.5"), None);
        assert_eq!(normalize_time("9:05").as_deref(), Some("9:05"));
        assert_eq!(normalize_time("09:05:00").as_deref(), Some("9:05"));
        assert_eq!(normalize_time("09:05:30").as_deref(), Some("9:05:30"));
        assert_eq!(normalize_time("9:65"), None);
        assert_eq!(normalize_time("1.5"), None);
        assert_eq!(detect_delimiter("a,b,c"), ",");
        assert_eq!(detect_delimiter("a\tb\tc"), "\t");
        assert_eq!(detect_delimiter("a;b"), ";");
        assert_eq!(detect_delimiter("a b  c"), " ");
        let (n, cut) = safe_name(
            "USW00013881 CHARLOTTE DOUGLAS AIRPORT, NC US",
            &blank(),
            &[],
        );
        assert!(!cut && n.len() <= MAX_NAME && !n.contains(' '), "{n}");
        let long = "A".repeat(80);
        let (n, cut) = safe_name(&long, &blank(), &[]);
        assert!(cut && n.chars().count() <= MAX_NAME, "{n}");
        assert!(n.ends_with("~1"));
        let (n2, _) = safe_name(&long, &blank(), std::slice::from_ref(&n));
        assert!(n2.ends_with("~2"));
    }

    #[test]
    fn single_column_csv_with_header_and_dates() {
        let text = "Date,Time,Rain (in)\n1/1/2024,0:00,0.00\n1/1/2024,0:15,0.12\n1/1/2024,0:30,0.35\njunk,row,here\n";
        let imp = parse(text, &blank(), DepthUnit::Inches).unwrap();
        assert_eq!(imp.layout, Layout::Table);
        assert!(imp.had_header);
        assert_eq!(imp.delimiter, ",");
        assert_eq!(imp.series.len(), 1);
        assert_eq!(imp.series[0].name, "Rain_(in)");
        assert_eq!(imp.series[0].points.len(), 3);
        assert_eq!(imp.series[0].points[1].date.as_deref(), Some("01/01/2024"));
        assert_eq!(imp.series[0].points[1].time, "0:15");
        assert_eq!(imp.series[0].points[1].value, "0.12");
        assert_eq!(imp.warnings.len(), 1, "{:?}", imp.warnings);
        assert!(!imp.daily);
        // Tab-separated, no header, time only, ISO date+time in one cell.
        let imp = parse("0:00\t0.5\n0:05\t1.25\n", &blank(), DepthUnit::Inches).unwrap();
        assert!(!imp.had_header);
        assert_eq!(imp.series[0].name, "Series1");
        assert_eq!(imp.series[0].points[1].date, None);
        let imp = parse(
            "datetime,value\n2024-03-01 06:00:00,1.5\n2024-03-01T07:00:00,2\n",
            &blank(),
            DepthUnit::Inches,
        )
        .unwrap();
        assert_eq!(imp.series[0].points.len(), 2);
        assert_eq!(imp.series[0].points[0].date.as_deref(), Some("03/01/2024"));
        assert_eq!(imp.series[0].points[0].time, "6:00");
        assert!(parse("", &blank(), DepthUnit::Inches).is_err());
        assert!(parse("x,y\n1,2\n", &blank(), DepthUnit::Inches).is_err());
    }

    #[test]
    fn multi_column_csv_becomes_one_series_per_gage_with_gages_in_one_step() {
        let text =
            "Date\tTime\tG1\tG2\tG3\n03/01/2024\t0:00\t0\t0.1\t0.2\n03/01/2024\t1:00\t0.3\t\t0.5\n";
        let doc = InpDoc::parse(
            "[TIMESERIES]\nG2 0:00 1\n[RAINGAGES]\nRG_G1 INTENSITY 1:00 1.0 TIMESERIES G1\n",
        );
        let imp = parse(text, &doc, DepthUnit::Inches).unwrap();
        assert_eq!(imp.series.len(), 3);
        assert_eq!(
            imp.series[1].name, "G2~1",
            "clashes with the existing series"
        );
        assert_eq!(imp.series[1].points.len(), 1, "the blank cell is a gap");
        assert_eq!(imp.point_count(), 5);
        let opts = GageOptions {
            format: "intensity".into(),
            interval: "1:00".into(),
            symbols: Some(((100.0, 200.0), 50.0)),
        };
        let mut doc = doc;
        let (cmd, gages) = commands(&doc, &imp, Some(&opts));
        assert_eq!(gages, vec!["RG_G2", "RG_G2~1", "RG_G3"]);
        doc.apply(cmd).unwrap();
        assert_eq!(doc.undo_depth(), 1);
        assert_eq!(doc.timeseries("G1").len(), 2);
        assert_eq!(doc.timeseries("G3")[1].value, "0.5");
        assert_eq!(doc.field("RAINGAGES", "RG_G3", "Format"), Some("INTENSITY"));
        assert_eq!(doc.field("RAINGAGES", "RG_G3", "Series"), Some("G3"));
        assert_eq!(doc.symbol("RG_G3"), Some((200.0, 200.0)));
        assert!(doc
            .validate()
            .iter()
            .all(|f| f.severity != crate::doc::Severity::Error));
        // Without gages: only series.
        let (cmd, gages) = commands(&doc, &imp, None);
        assert!(gages.is_empty());
        let Command::Batch(c) = cmd else { panic!() };
        assert_eq!(c.len(), 5);
    }

    #[test]
    fn ghcn_daily_converts_tenths_of_mm_and_cuts_long_names() {
        let text = "\"STATION\",\"NAME\",\"DATE\",\"PRCP\",\"TMAX\"\n\"USW00013881\",\"CHARLOTTE DOUGLAS INTERNATIONAL AIRPORT, NC US\",\"2024-01-01\",\"0\",\"55\"\n\"USW00013881\",\"CHARLOTTE DOUGLAS INTERNATIONAL AIRPORT, NC US\",\"2024-01-02\",\"254\",\"60\"\n\"USW00013881\",\"CHARLOTTE DOUGLAS INTERNATIONAL AIRPORT, NC US\",\"2024-01-03\",\"\",\"61\"\n";
        let imp = parse(text, &blank(), DepthUnit::Inches).unwrap();
        assert_eq!(imp.layout, Layout::GhcnDaily);
        assert!(imp.daily);
        assert_eq!(imp.series.len(), 1);
        let s = &imp.series[0];
        assert!(s.name.chars().count() <= MAX_NAME, "{}", s.name);
        assert!(s.name.contains("USW00013881"), "{}", s.name);
        assert!(
            imp.warnings.iter().any(|w| w.contains("longer than")),
            "{:?}",
            imp.warnings
        );
        assert_eq!(s.points.len(), 2, "the blank PRCP is a gap");
        assert_eq!(s.points[1].date.as_deref(), Some("01/02/2024"));
        assert_eq!(s.points[1].value, "1"); // 254 tenths of mm = 25.4 mm = 1 in
        assert!(imp.factor.as_ref().unwrap().1.contains("25.4"));
        let imp = parse(text, &blank(), DepthUnit::Millimetres).unwrap();
        assert_eq!(imp.series[0].points[1].value, "25.4");
        // The name it actually writes must not clash with one already there.
        let name = imp.series[0].name.clone();
        let doc = InpDoc::parse(&format!("[TIMESERIES]\n{name} 01/01/2024 0:00 1\n"));
        let imp = parse(text, &doc, DepthUnit::Millimetres).unwrap();
        assert_ne!(imp.series[0].name, name);
    }

    #[test]
    fn external_file_round_trips_through_the_dat_format() {
        let pts = vec![
            SeriesPoint {
                date: Some("01/01/2024".into()),
                time: "0:00".into(),
                value: "0.5".into(),
            },
            SeriesPoint {
                date: Some("01/01/2024".into()),
                time: "0:15".into(),
                value: "1.25".into(),
            },
        ];
        let text = external_file_text("TS1", &pts);
        assert!(
            text.starts_with(";TS1\n;date time value\n01/01/2024 0:00 0.5\n01/01/2024 0:15 1.25\n")
        );
        let back = parse(&text, &blank(), DepthUnit::Inches).unwrap();
        assert_eq!(back.series[0].points, pts);
        let mut doc = InpDoc::parse(
            "[TIMESERIES]\nTS1 01/01/2024 0:00 0.5\nTS1 01/01/2024 0:15 1.25\nOther 0:00 1\n",
        );
        doc.apply(externalize_command(&doc, "TS1", "C:/rain/TS1.dat"))
            .unwrap();
        let rows = doc.find_all("TIMESERIES", "TS1");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value(1), Some("FILE"));
        assert_eq!(rows[0].value(2), Some("C:/rain/TS1.dat"));
        assert_eq!(doc.timeseries("Other").len(), 1);
        assert_eq!(doc.undo_depth(), 1);
    }
}
