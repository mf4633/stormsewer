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
}

impl ReportSummary {
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
