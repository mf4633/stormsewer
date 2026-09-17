// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser for the EPA SWMM 5 text report (`.rpt`).
//!
//! **`runswmm.exe` exits 0 no matter what happens** — a successful run, a
//! fatal input error, and `runswmm` with no arguments at all all return 0
//! (verified against EPA SWMM 5.2.4). The exit status is therefore useless as
//! a success signal, and the report is the only place the engine says whether
//! the run was any good. Everything downstream of a run keys off this parse.
//!
//! The lines that carry meaning:
//!
//! ```text
//!   EPA STORM WATER MANAGEMENT MODEL - VERSION 5.2 (Build 5.2.4)
//!   ERROR 211: invalid number ...
//!   WARNING 09: time series interval greater than recording interval ...
//!   Continuity Error (%) .....        -0.034
//! ```

use std::path::Path;

use crate::{Error, Result};

/// What the engine reported about a run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportSummary {
    /// Build string, e.g. `5.2.4`, taken from the banner.
    pub engine_version: Option<String>,
    /// Fatal errors. A non-empty list means the run failed.
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// Continuity errors as `(section, percent)`, e.g.
    /// `("Flow Routing Continuity", -0.034)`.
    pub continuity: Vec<(String, f64)>,
    pub analysis_begun: Option<String>,
    /// The summary tables (`Node Depth Summary`, `Link Flow Summary`, …), in
    /// report order. See [`SummaryTable`].
    pub tables: Vec<SummaryTable>,
    /// The diagnostic lists the engine prints after the continuity
    /// sections, every entry as written (the engine itself caps them, but
    /// nothing here does). See [`Diagnostics`].
    pub diagnostics: Diagnostics,
}

/// One `Node J1 (5.23%)` / `Link C3 (12)` line from a diagnostic list.
#[derive(Clone, Debug, PartialEq)]
pub struct RankedEntry {
    /// `Node` or `Link`.
    pub what: String,
    pub name: String,
    /// The number in parentheses, sign kept.
    pub value: f64,
    /// The unit that followed it: `%` or nothing.
    pub unit: String,
}

/// The four ranked lists the engine prints, in the report's order, plus
/// the sentence it prints instead when a list is empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagnostics {
    /// `Highest Continuity Errors`: nodes, percent of the node's inflow.
    pub continuity_by_node: Vec<RankedEntry>,
    pub continuity_note: Option<String>,
    /// `Time-Step Critical Elements`: links (and nodes), percent of steps
    /// where that element set the time step.
    pub critical_elements: Vec<RankedEntry>,
    pub critical_note: Option<String>,
    /// `Highest Flow Instability Indexes`: links, count of flow-direction
    /// reversals per period (0-150).
    pub instability: Vec<RankedEntry>,
    pub instability_note: Option<String>,
    /// `Most Frequent Nonconverging Nodes`: nodes, percent of steps.
    pub nonconverging: Vec<RankedEntry>,
    pub nonconverging_note: Option<String>,
}

impl Diagnostics {
    pub fn is_empty(&self) -> bool {
        self.continuity_by_node.is_empty()
            && self.critical_elements.is_empty()
            && self.instability.is_empty()
            && self.nonconverging.is_empty()
            && self.continuity_note.is_none()
            && self.critical_note.is_none()
            && self.instability_note.is_none()
            && self.nonconverging_note.is_none()
    }
}

/// `Node J1 (5.23%)` → a [`RankedEntry`]; anything else is `None`.
fn ranked_entry(line: &str) -> Option<RankedEntry> {
    let t = line.trim();
    let (what, rest) = t.split_once(char::is_whitespace)?;
    if !(what.eq_ignore_ascii_case("node") || what.eq_ignore_ascii_case("link")) {
        return None;
    }
    let open = rest.rfind('(')?;
    let close = rest.rfind(')')?;
    if close < open {
        return None;
    }
    let name = rest[..open].trim();
    let inner = rest[open + 1..close].trim();
    let digits_end = inner
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .map(|(i, _)| i)
        .unwrap_or(inner.len());
    let value: f64 = inner[..digits_end].parse().ok()?;
    if name.is_empty() {
        return None;
    }
    Some(RankedEntry {
        what: what.to_string(),
        name: name.to_string(),
        value,
        unit: inner[digits_end..].trim().to_string(),
    })
}

/// Read the four diagnostic lists. Each is a star-boxed title followed by
/// entry lines (or one sentence), ending at a blank line or the next rule.
pub fn parse_diagnostics(text: &str) -> Diagnostics {
    let lines: Vec<&str> = text.lines().collect();
    let is_star_rule = |s: &str| s.len() >= 4 && s.chars().all(|c| c == '*');
    let mut d = Diagnostics::default();
    let mut i = 0;
    while i + 2 < lines.len() {
        let title = lines[i + 1].trim();
        if !(is_star_rule(lines[i].trim()) && is_star_rule(lines[i + 2].trim())) {
            i += 1;
            continue;
        }
        let slot: Option<(&mut Vec<RankedEntry>, &mut Option<String>)> = match title {
            "Highest Continuity Errors" => Some((&mut d.continuity_by_node, &mut d.continuity_note)),
            "Time-Step Critical Elements" => Some((&mut d.critical_elements, &mut d.critical_note)),
            "Highest Flow Instability Indexes" => Some((&mut d.instability, &mut d.instability_note)),
            "Most Frequent Nonconverging Nodes" => {
                Some((&mut d.nonconverging, &mut d.nonconverging_note))
            }
            _ => None,
        };
        let Some((entries, note)) = slot else {
            i += 3;
            continue;
        };
        let mut k = i + 3;
        while k < lines.len() {
            let t = lines[k].trim();
            if t.is_empty() || is_star_rule(t) {
                break;
            }
            match ranked_entry(t) {
                Some(e) => entries.push(e),
                None => {
                    if note.is_none() {
                        *note = Some(t.to_string());
                    }
                }
            }
            k += 1;
        }
        i = k;
    }
    d
}

impl ReportSummary {
    /// The `Routing Time Step Summary` key/value table, if the run got
    /// that far.
    pub fn routing_time_step(&self) -> Option<&SummaryTable> {
        self.tables
            .iter()
            .find(|t| t.title == "Routing Time Step Summary")
    }

    /// The engine found nothing fatal. Whether the *run* as a whole succeeded
    /// also depends on the results file existing — see [`crate::engine::Run`].
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    /// Largest absolute continuity error across all sections, which is the
    /// single number a reviewer looks at first.
    pub fn worst_continuity(&self) -> Option<(&str, f64)> {
        let mut worst: Option<(&str, f64)> = None;
        for (section, pct) in &self.continuity {
            let better = match worst {
                None => true,
                Some((_, w)) => pct.abs() > w.abs(),
            };
            if better {
                worst = Some((section.as_str(), *pct));
            }
        }
        worst
    }
}

/// Parse report text. Takes a string rather than a path so it can be tested
/// without fixtures and reused on a report already in memory.
pub fn parse(text: &str) -> ReportSummary {
    let mut summary = ReportSummary::default();
    // Section titles arrive boxed in asterisks, in two shapes. The plain one:
    //     ****************
    //     Analysis Options
    //     ****************
    // and the tabular one, where the rules carry the column headings and the
    // title line carries the column units:
    //     **************************        Volume        Volume
    //     Flow Routing Continuity        hectare-m      10^6 ltr
    //     **************************     ---------     ---------
    // So a rule is a line that *starts* with a run of asterisks rather than
    // one made only of them, and the width of that run marks off the heading:
    // the title is the first `width` characters of the following line. The
    // closing rule must also not arm the line after it, or the section's first
    // real line is eaten as a title and its values are never read.
    let rule_width = |s: &str| {
        let stars = s.chars().take_while(|c| *c == '*').count();
        (stars >= 4).then_some(stars)
    };
    let mut armed: Option<usize> = None;
    let mut just_titled = false;
    let mut section = String::new();

    for raw in text.lines() {
        let line = raw.trim();

        if let Some(width) = rule_width(line) {
            armed = (!just_titled).then_some(width);
            just_titled = false;
            continue;
        }
        if let Some(width) = armed {
            if !line.is_empty() {
                section = line.chars().take(width).collect::<String>().trim().to_string();
                armed = None;
                just_titled = true;
                continue;
            }
        }
        armed = None;
        just_titled = false;

        if let Some(rest) = line.strip_prefix("ERROR") {
            summary.errors.push(format!("ERROR{rest}").trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("WARNING") {
            summary.warnings.push(format!("WARNING{rest}").trim().to_string());
            continue;
        }

        if summary.engine_version.is_none() {
            if let Some(build) = line.split_once("(Build ").and_then(|(_, r)| r.split_once(')')) {
                summary.engine_version = Some(build.0.trim().to_string());
                continue;
            }
        }

        if let Some(rest) = line.strip_prefix("Analysis begun on:") {
            summary.analysis_begun = Some(rest.trim().to_string());
            continue;
        }

        if line.starts_with("Continuity Error (%)") {
            if let Some(value) = line.split_whitespace().last().and_then(|t| t.parse::<f64>().ok()) {
                let label = if section.is_empty() {
                    "Continuity".to_string()
                } else {
                    section.clone()
                };
                summary.continuity.push((label, value));
            }
        }
    }

    summary.tables = parse_tables(text);
    summary.diagnostics = parse_diagnostics(text);
    summary
}

/// Read and parse a report file.
pub fn read(path: &Path) -> Result<ReportSummary> {
    let bytes = std::fs::read(path).map_err(|e| {
        Error::NotFound(format!("could not read the report {}: {e}", path.display()))
    })?;
    // Reports are ASCII in practice, but a model with an odd character in a
    // title should not sink the parse.
    Ok(parse(&String::from_utf8_lossy(&bytes)))
}

/// One summary table from the report, kept close to the text.
///
/// The engine prints each table as a title boxed in asterisks, a dashed rule,
/// two to four lines of right-aligned column headings whose last line carries
/// the units (which change with `FLOW_UNITS`), another rule, and the rows.
/// The heading text is kept verbatim in `header_lines`; `columns` stacks the
/// words of each column top to bottom so "Maximum / Depth / Feet" reads as one
/// label. A table the engine replaced with a sentence ("No nodes were
/// flooded.") has no rows and carries that sentence in `note`. Key/value
/// summaries (`Routing Time Step Summary`) become two columns, `Item` and
/// `Value`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SummaryTable {
    pub title: String,
    pub header_lines: Vec<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub note: Option<String>,
}

impl SummaryTable {
    /// Column index whose label contains `needle`, case-insensitively.
    pub fn column(&self, needle: &str) -> Option<usize> {
        let n = needle.to_ascii_lowercase();
        self.columns
            .iter()
            .position(|c| c.to_ascii_lowercase().contains(&n))
    }

    /// The object name in each row: always the first column.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.rows
            .iter()
            .filter_map(|r| r.first().map(String::as_str))
    }

    /// Comma-separated text with the column labels as the first line. Cells
    /// holding a comma or a quote are quoted.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        out.push_str(&csv_line(&self.columns));
        for row in &self.rows {
            out.push_str(&csv_line(row));
        }
        out
    }
}

/// One CSV record with a trailing newline, quoting cells that need it.
pub fn csv_line<S: AsRef<str>>(cells: &[S]) -> String {
    let mut line = cells
        .iter()
        .map(|c| {
            let s = c.as_ref();
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    line.push('\n');
    line
}

/// `(start, end)` char spans of the whitespace-delimited tokens of a line.
fn token_spans(line: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    let mut i = 0;
    for c in line.chars() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                spans.push((s, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
        i += 1;
    }
    if let Some(s) = start {
        spans.push((s, i));
    }
    spans
}

/// Spans of heading tokens: words separated by a single space belong to one
/// heading ("Storage Unit", "Time of Max", "1000 ft³"); two or more spaces
/// separate columns.
fn heading_spans(line: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            let next_is_space = chars.get(i + 1).is_none_or(|n| n.is_whitespace());
            if next_is_space {
                if let Some(s) = start.take() {
                    spans.push((s, i));
                }
            }
        } else if start.is_none() {
            start = Some(i);
        }
        i += 1;
    }
    if let Some(s) = start {
        spans.push((s, chars.len()));
    }
    spans
}

fn slice_chars(line: &str, start: usize, end: usize) -> String {
    line.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn is_dashed_rule(line: &str) -> bool {
    line.len() >= 4 && line.chars().all(|c| c == '-')
}

/// Column spans from the stacked heading lines: tokens on different lines
/// that overlap in position belong to the same column, since the engine
/// right-aligns each column's words over one another.
fn header_columns(header_lines: &[String]) -> Vec<(usize, usize, String)> {
    // Each entry: (start, end, words in reading order).
    let mut cols: Vec<(usize, usize, Vec<String>)> = Vec::new();
    for line in header_lines {
        for (s, e) in heading_spans(line) {
            let word = slice_chars(line, s, e);
            // Everything this token touches gets merged into one column.
            let touching: Vec<usize> = cols
                .iter()
                .enumerate()
                .filter(|(_, c)| s < c.1 && c.0 < e)
                .map(|(i, _)| i)
                .collect();
            if touching.is_empty() {
                cols.push((s, e, vec![word]));
            } else {
                let mut merged = (s, e, vec![word]);
                for &i in touching.iter().rev() {
                    let c = cols.remove(i);
                    merged.0 = merged.0.min(c.0);
                    merged.1 = merged.1.max(c.1);
                    let mut words = c.2;
                    words.append(&mut merged.2);
                    merged.2 = words;
                }
                cols.push(merged);
            }
        }
    }
    cols.sort_by_key(|c| c.0);
    cols.into_iter()
        .map(|(s, e, words)| (s, e, words.join(" ")))
        .collect()
}

/// Split one data line into the table's columns by token position.
fn split_row(line: &str, cols: &[(usize, usize, String)]) -> Vec<String> {
    let mut cells: Vec<Vec<String>> = vec![Vec::new(); cols.len()];
    for (s, e) in token_spans(line) {
        let text = slice_chars(line, s, e);
        // The column this token overlaps most; failing that, the one whose
        // right edge is nearest, since numbers are right-aligned.
        let mut best: Option<(usize, usize)> = None;
        for (i, c) in cols.iter().enumerate() {
            let overlap = e.min(c.1).saturating_sub(s.max(c.0));
            if overlap > 0 && best.is_none_or(|b| overlap > b.1) {
                best = Some((i, overlap));
            }
        }
        let idx = match best {
            Some((i, _)) => i,
            None => cols
                .iter()
                .enumerate()
                .min_by_key(|(_, c)| (c.1 as i64 - e as i64).abs())
                .map(|(i, _)| i)
                .unwrap_or(0),
        };
        cells[idx].push(text);
    }
    cells.into_iter().map(|c| c.join(" ")).collect()
}

/// Every summary table in the report, in order. Tables are found by their
/// asterisk-boxed titles; anything not shaped like a table is skipped.
pub fn parse_tables(text: &str) -> Vec<SummaryTable> {
    let lines: Vec<&str> = text.lines().collect();
    let is_star_rule = |s: &str| s.len() >= 4 && s.chars().all(|c| c == '*');
    let mut tables = Vec::new();
    let mut i = 0;
    while i + 2 < lines.len() {
        let a = lines[i].trim();
        let title = lines[i + 1].trim();
        let c = lines[i + 2].trim();
        if !(is_star_rule(a) && is_star_rule(c) && !title.is_empty()) {
            i += 1;
            continue;
        }
        let title = title.to_string();
        // Skip blank lines after the title.
        let mut j = i + 3;
        while j < lines.len() && lines[j].trim().is_empty() {
            j += 1;
        }
        if j >= lines.len() {
            break;
        }
        let first = lines[j].trim();
        if is_dashed_rule(first) {
            // Tabular: headings up to the next rule, then rows.
            let mut header_lines = Vec::new();
            let mut k = j + 1;
            while k < lines.len() && !is_dashed_rule(lines[k].trim()) {
                if lines[k].trim().is_empty() {
                    break;
                }
                header_lines.push(lines[k].to_string());
                k += 1;
            }
            let cols = header_columns(&header_lines);
            let mut rows = Vec::new();
            k += 1;
            while k < lines.len() {
                let raw = lines[k];
                let t = raw.trim();
                if t.is_empty() || is_star_rule(t) {
                    break;
                }
                // A rule inside the body separates totals ("System") from
                // the objects; the totals still belong to the table.
                if !is_dashed_rule(t) && !cols.is_empty() {
                    rows.push(split_row(raw, &cols));
                }
                k += 1;
            }
            tables.push(SummaryTable {
                title,
                header_lines,
                columns: cols.into_iter().map(|c| c.2).collect(),
                rows,
                note: None,
            });
            i = k;
        } else if title.ends_with("Summary") && first.contains(" : ") {
            // Key/value summary.
            let mut rows = Vec::new();
            let mut k = j;
            while k < lines.len() {
                let t = lines[k].trim();
                if t.is_empty() || is_star_rule(t) {
                    break;
                }
                if let Some((key, value)) = t.split_once(':') {
                    rows.push(vec![key.trim().to_string(), value.trim().to_string()]);
                }
                k += 1;
            }
            tables.push(SummaryTable {
                title,
                header_lines: Vec::new(),
                columns: vec!["Item".to_string(), "Value".to_string()],
                rows,
                note: None,
            });
            i = k;
        } else if title.ends_with("Summary") && !is_star_rule(first) {
            // "No nodes were flooded." — the table exists, with nothing in it.
            tables.push(SummaryTable {
                title,
                header_lines: Vec::new(),
                columns: Vec::new(),
                rows: Vec::new(),
                note: Some(first.to_string()),
            });
            i = j + 1;
        } else {
            i += 3;
        }
    }
    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written to the shape of a real report rather than copied from one.
    const SAMPLE: &str = "\
  EPA STORM WATER MANAGEMENT MODEL - VERSION 5.2 (Build 5.2.4)
  ------------------------------------------------------------

  Example model for the parser test.

  ****************
  Analysis Options
  ****************
  Flow Units ............... CMS
  Flow Routing Method ...... DYNWAVE

  WARNING 09: time series interval greater than recording interval for Rain Gage RG1

  **************************        Volume        Volume
  Flow Routing Continuity        hectare-m      10^6 ltr
  **************************     ---------     ---------
  Initial Stored Volume ....         0.005         0.055
  Final Stored Volume ......        52.870       528.701
  Continuity Error (%) .....        -0.034

  **************************        Volume         Depth
  Runoff Quantity Continuity     hectare-m            mm
  **************************     ---------     ---------
  Continuity Error (%) .....         1.250

  Analysis begun on:  Wed May 27 08:36:27 2026
";

    #[test]
    fn reads_version_warnings_and_continuity() {
        let s = parse(SAMPLE);
        assert_eq!(s.engine_version.as_deref(), Some("5.2.4"));
        assert!(s.is_clean());
        assert_eq!(s.warnings.len(), 1);
        assert!(s.warnings[0].starts_with("WARNING 09:"));
        assert!(s.warnings[0].contains("Rain Gage RG1"));
        assert_eq!(s.analysis_begun.as_deref(), Some("Wed May 27 08:36:27 2026"));

        assert_eq!(
            s.continuity,
            vec![
                ("Flow Routing Continuity".to_string(), -0.034),
                ("Runoff Quantity Continuity".to_string(), 1.25),
            ]
        );
        // The worst error is by magnitude, not by sign.
        assert_eq!(s.worst_continuity(), Some(("Runoff Quantity Continuity", 1.25)));
    }

    #[test]
    fn errors_make_a_report_unclean() {
        let s = parse(
            "  EPA STORM WATER MANAGEMENT MODEL - VERSION 5.2 (Build 5.1.015)\n\
             \n  ERROR 211: invalid number 4.x at line 88 of [CONDUITS]\n\
             \n  ERROR 303: cannot solve network\n",
        );
        assert!(!s.is_clean());
        assert_eq!(s.errors.len(), 2);
        assert!(s.errors[0].contains("ERROR 211"));
        assert_eq!(s.engine_version.as_deref(), Some("5.1.015"));
        assert!(s.warnings.is_empty());
    }

    /// Shaped like the engine's diagnostic lists, with more entries than
    /// the five the GUI shows so the count proves nothing is dropped.
    const DIAGNOSTICS: &str = "\
  *************************
  Highest Continuity Errors
  *************************
  Node J1 (5.23%)
  Node J2 (-1.02%)
  Node SU1 (0.75%)
  Node J7 (0.40%)
  Node J8 (0.31%)
  Node J9 (0.30%)
  Node J10 (0.28%)

  ***************************
  Time-Step Critical Elements
  ***************************
  Link C3 (45.20%)
  Link C1 (12.00%)

  ********************************
  Highest Flow Instability Indexes
  ********************************
  All links are stable.

  *********************************
  Most Frequent Nonconverging Nodes
  *********************************
  Convergence obtained at all time steps.

  *************************
  Routing Time Step Summary
  *************************
  Minimum Time Step           :     0.50 sec
  Average Time Step           :    14.10 sec
  Maximum Time Step           :    15.00 sec
  % of Time in Steady State   :     0.00
  Average Iterations per Step :     2.05
  % of Steps Not Converging   :     0.10

";

    #[test]
    fn diagnostic_lists_are_read_in_full() {
        let s = parse(DIAGNOSTICS);
        let d = &s.diagnostics;
        assert_eq!(d.continuity_by_node.len(), 7, "{d:?}");
        assert_eq!(d.continuity_by_node[0].name, "J1");
        assert_eq!(d.continuity_by_node[0].what, "Node");
        assert!((d.continuity_by_node[0].value - 5.23).abs() < 1e-9);
        assert_eq!(d.continuity_by_node[0].unit, "%");
        assert!((d.continuity_by_node[1].value + 1.02).abs() < 1e-9);
        assert_eq!(d.continuity_by_node[6].name, "J10");
        assert!(d.continuity_note.is_none());
        assert_eq!(d.critical_elements.len(), 2);
        assert_eq!(d.critical_elements[0].name, "C3");
        assert!(d.instability.is_empty());
        assert_eq!(d.instability_note.as_deref(), Some("All links are stable."));
        assert!(d.nonconverging.is_empty());
        assert_eq!(
            d.nonconverging_note.as_deref(),
            Some("Convergence obtained at all time steps.")
        );
        let step = s.routing_time_step().unwrap();
        assert_eq!(step.rows.len(), 6);
        assert_eq!(step.rows[5][0], "% of Steps Not Converging");
        assert_eq!(step.rows[5][1], "0.10");
        assert!(!d.is_empty());
        assert!(parse("").diagnostics.is_empty());

        // An instability index has no unit.
        let e = ranked_entry("  Link C3 (12)").unwrap();
        assert_eq!(e.value, 12.0);
        assert_eq!(e.unit, "");
        assert!(ranked_entry("All links are stable.").is_none());
        assert!(ranked_entry("Node (5%)").is_none());
    }

    /// A run that died before writing anything leaves an empty or partial
    /// report. That must parse to "nothing reported", not panic.
    #[test]
    fn empty_report_is_survivable() {
        let s = parse("");
        assert_eq!(s, ReportSummary::default());
        assert!(s.is_clean());
        assert_eq!(s.worst_continuity(), None);
    }

    #[test]
    fn missing_report_file_is_named() {
        let missing = std::env::temp_dir().join("stormsewer-swmm-tests/definitely-absent.rpt");
        let err = read(&missing).unwrap_err().to_string();
        assert!(err.contains("could not read the report"), "{err}");
    }

    /// Opt-in pass over real reports, which cannot be committed here.
    #[test]
    fn parses_real_reports_when_available() {
        let Ok(dir) = std::env::var("STORMSEWER_SWMM_FIXTURES") else {
            eprintln!("skipped: STORMSEWER_SWMM_FIXTURES not set");
            return;
        };
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("fixture dir") {
            let p = entry.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) != Some("rpt") {
                continue;
            }
            let s = read(&p).unwrap();
            assert!(
                s.engine_version.is_some(),
                "{}: no version banner found",
                p.display()
            );
            // Continuity belongs to a continuity section. Attributing it to
            // "Analysis Options" means the banner walker never advanced past
            // the first section — the exact failure the tabular banner caused.
            for (section, _) in &s.continuity {
                assert!(
                    section.contains("Continuity"),
                    "{}: continuity attributed to section {section:?}",
                    p.display()
                );
            }
            checked += 1;
        }
        assert!(checked > 0, "no .rpt files in {dir}");
        eprintln!("checked {checked} real .rpt fixtures");
    }
}
