// SPDX-License-Identifier: GPL-3.0-or-later

//! The model report: what a reviewer needs to reproduce and judge a SWMM
//! run — which engine (version and binary hash), which model (name and
//! text hash), the options that decide the answer, the inventory, the
//! engine's own continuity errors and summary tables, the editor's
//! validation findings, a profile station table, and the exact `.inp`
//! text as an appendix.
//!
//! The report is built as a list of [`Block`]s so one assembly serves both
//! the HTML page (styled like the storm-sewer design report) and the PDF
//! the app writes through the engine crate's document writer.

use stormsewer::report_html::report_css;

use crate::doc::{Finding, InpDoc, Severity};
use crate::engine::Run;

/// One piece of the report.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(String),
    Paragraph(String),
    KeyValues(Vec<(String, String)>),
    Table {
        title: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        note: Option<String>,
    },
    /// Tagged lines, e.g. `("ERROR", "...")`.
    List(Vec<(String, String)>),
    /// Verbatim text in a monospace block.
    Preformatted(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelReport {
    pub title: String,
    pub subtitle: String,
    pub blocks: Vec<Block>,
}

/// A profile between two nodes, already tabulated by the app's profile
/// view (station, invert, crown, rim, HGL...).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileTable {
    pub start: String,
    pub end: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

pub struct ReportInputs<'a> {
    pub doc: &'a InpDoc,
    pub run: &'a Run,
    pub findings: &'a [Finding],
    pub profile: Option<&'a ProfileTable>,
    /// The model's file name or title, for the header.
    pub model_name: &'a str,
    /// SHA-256 of the `.inp` text the engine was given.
    pub model_sha256: &'a str,
    /// When the report was generated, as the app formats dates.
    pub generated: &'a str,
}

/// The `[OPTIONS]` keys that decide a run's answer, in the order shown.
pub const OPTIONS_THAT_MATTER: &[&str] = &[
    "FLOW_UNITS",
    "FLOW_ROUTING",
    "INFILTRATION",
    "LINK_OFFSETS",
    "START_DATE",
    "START_TIME",
    "END_DATE",
    "END_TIME",
    "REPORT_START_DATE",
    "REPORT_START_TIME",
    "REPORT_STEP",
    "WET_STEP",
    "DRY_STEP",
    "ROUTING_STEP",
    "INERTIAL_DAMPING",
    "NORMAL_FLOW_LIMITED",
    "FORCE_MAIN_EQUATION",
    "VARIABLE_STEP",
    "MIN_SLOPE",
    "ALLOW_PONDING",
    "SKIP_STEADY_STATE",
    "THREADS",
];

fn count(doc: &InpDoc, section: &str) -> usize {
    doc.names(section).len()
}

/// Assemble the report.
pub fn build(i: &ReportInputs<'_>) -> ModelReport {
    let doc = i.doc;
    let run = i.run;
    let mut blocks = Vec::new();

    let title_text = doc.title();
    if !title_text.trim().is_empty() {
        blocks.push(Block::Heading("Title and notes".into()));
        blocks.push(Block::Preformatted(title_text));
    }

    blocks.push(Block::Heading("Engine and run".into()));
    let mut kv = vec![
        ("Engine".to_string(), format!("EPA SWMM {}", run.engine_version)),
        ("Engine id".to_string(), run.engine_id.clone()),
        ("Engine binary SHA-256".to_string(), run.engine_sha256.clone()),
        (
            "Engine reported version".to_string(),
            run.report
                .engine_version
                .clone()
                .unwrap_or_else(|| "(no banner in the report)".into()),
        ),
        ("Model".to_string(), i.model_name.to_string()),
        ("Model text SHA-256".to_string(), i.model_sha256.to_string()),
        ("Input file".to_string(), run.inp.display().to_string()),
        ("Report file".to_string(), run.rpt.display().to_string()),
        ("Results file".to_string(), run.out.display().to_string()),
        (
            "Run time".to_string(),
            format!("{:.2} s", run.elapsed.as_secs_f64()),
        ),
    ];
    if let Some(begun) = &run.report.analysis_begun {
        kv.push(("Analysis begun".into(), begun.clone()));
    }
    kv.push((
        "Outcome".into(),
        match run.failure_reason() {
            None => format!(
                "succeeded, {} warning(s)",
                run.report.warnings.len()
            ),
            Some(why) => format!("FAILED: {why}"),
        },
    ));
    kv.push(("Generated".into(), i.generated.to_string()));
    blocks.push(Block::KeyValues(kv));
    if !run.report.errors.is_empty() || !run.report.warnings.is_empty() {
        let mut list: Vec<(String, String)> = run
            .report
            .errors
            .iter()
            .map(|e| ("ERROR".to_string(), e.clone()))
            .collect();
        list.extend(
            run.report
                .warnings
                .iter()
                .map(|w| ("WARNING".to_string(), w.clone())),
        );
        blocks.push(Block::List(list));
    }

    blocks.push(Block::Heading("Options".into()));
    let opts: Vec<(String, String)> = OPTIONS_THAT_MATTER
        .iter()
        .filter_map(|k| doc.key_value("OPTIONS", k).map(|v| (k.to_string(), v)))
        .collect();
    if opts.is_empty() {
        blocks.push(Block::Paragraph("The model has no [OPTIONS] section; the engine's defaults applied.".into()));
    } else {
        blocks.push(Block::KeyValues(opts));
    }

    blocks.push(Block::Heading("Inventory".into()));
    let inventory: Vec<(String, String)> = [
        ("Rain gages", "RAINGAGES"),
        ("Subcatchments", "SUBCATCHMENTS"),
        ("Junctions", "JUNCTIONS"),
        ("Outfalls", "OUTFALLS"),
        ("Storage units", "STORAGE"),
        ("Dividers", "DIVIDERS"),
        ("Conduits", "CONDUITS"),
        ("Pumps", "PUMPS"),
        ("Orifices", "ORIFICES"),
        ("Weirs", "WEIRS"),
        ("Outlets", "OUTLETS"),
        ("Time series", "TIMESERIES"),
        ("Curves", "CURVES"),
        ("Patterns", "PATTERNS"),
        ("Pollutants", "POLLUTANTS"),
        ("LID controls", "LID_CONTROLS"),
    ]
    .iter()
    .map(|(label, section)| (label.to_string(), count(doc, section).to_string()))
    .collect();
    blocks.push(Block::KeyValues(inventory));
    let controls = doc
        .section("CONTROLS")
        .map(|s| s.rows().count())
        .unwrap_or(0);
    if controls > 0 {
        blocks.push(Block::Paragraph(format!(
            "[CONTROLS] has {controls} rule line(s)."
        )));
    }

    blocks.push(Block::Heading("Continuity".into()));
    if run.report.continuity.is_empty() {
        blocks.push(Block::Paragraph("The report carries no continuity error lines.".into()));
    } else {
        let rows: Vec<Vec<String>> = run
            .report
            .continuity
            .iter()
            .map(|(section, pct)| vec![section.clone(), format!("{pct:.3}")])
            .collect();
        blocks.push(Block::Table {
            title: "Continuity errors".into(),
            columns: vec!["Section".into(), "Error (%)".into()],
            rows,
            note: run
                .report
                .worst_continuity()
                .map(|(s, p)| format!("Largest: {s} {p:.3} %")),
        });
    }

    blocks.push(Block::Heading("Report summary tables".into()));
    if run.report.tables.is_empty() {
        blocks.push(Block::Paragraph("The report has no summary tables.".into()));
    }
    for t in &run.report.tables {
        blocks.push(Block::Table {
            title: t.title.clone(),
            columns: t.columns.clone(),
            rows: t.rows.clone(),
            note: t.note.clone(),
        });
    }

    blocks.push(Block::Heading("Validation".into()));
    if i.findings.is_empty() {
        blocks.push(Block::Paragraph("The editor's referential checks found nothing.".into()));
    } else {
        blocks.push(Block::List(
            i.findings
                .iter()
                .map(|f| {
                    let tag = match f.severity {
                        Severity::Error => "ERROR",
                        Severity::Warning => "WARNING",
                    };
                    (
                        tag.to_string(),
                        if f.name.is_empty() {
                            format!("[{}] {}", f.section, f.message)
                        } else {
                            format!("[{}] {}: {}", f.section, f.name, f.message)
                        },
                    )
                })
                .collect(),
        ));
    }

    if let Some(p) = i.profile {
        blocks.push(Block::Heading(format!("Profile {} to {}", p.start, p.end)));
        blocks.push(Block::Table {
            title: "Stations".into(),
            columns: p.columns.clone(),
            rows: p.rows.clone(),
            note: None,
        });
    }

    blocks.push(Block::Heading("Appendix: input file".into()));
    blocks.push(Block::Preformatted(doc.to_string()));

    ModelReport {
        title: format!("SWMM model report — {}", i.model_name),
        subtitle: format!(
            "EPA SWMM {} ({}) · {}",
            run.engine_version,
            &run.engine_sha256[..run.engine_sha256.len().min(12)],
            i.generated
        ),
        blocks,
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

impl ModelReport {
    /// A self-contained HTML page in the design report's style.
    pub fn to_html(&self) -> String {
        let mut out = String::new();
        out.push_str("<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"/>");
        out.push_str(r#"<meta name="viewport" content="width=device-width, initial-scale=1"/>"#);
        out.push_str(&format!("<title>{}</title>", esc(&self.title)));
        out.push_str(report_css());
        out.push_str("<style>pre{background:#f4f6f8;border:1px solid #e0e6ed;padding:10px;font-size:0.8rem;overflow:auto;white-space:pre;} .kv td:first-child{font-weight:600;width:28%;} h3{margin-top:18px;} .tag{font-weight:600;margin-right:6px;} .tag-error{color:#b00020;} .tag-warning{color:#8a5a00;}</style>");
        out.push_str("</head><body>");
        out.push_str(&format!("<h1>{}</h1>", esc(&self.title)));
        out.push_str(&format!(r#"<p class="meta">{}</p>"#, esc(&self.subtitle)));
        for b in &self.blocks {
            match b {
                Block::Heading(h) => out.push_str(&format!("<h2>{}</h2>", esc(h))),
                Block::Paragraph(p) => out.push_str(&format!("<p>{}</p>", esc(p))),
                Block::KeyValues(kv) => {
                    out.push_str(r#"<table class="kv"><tbody>"#);
                    for (k, v) in kv {
                        out.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", esc(k), esc(v)));
                    }
                    out.push_str("</tbody></table>");
                }
                Block::Table {
                    title,
                    columns,
                    rows,
                    note,
                } => {
                    out.push_str(&format!("<h3>{}</h3>", esc(title)));
                    if let Some(n) = note {
                        out.push_str(&format!(r#"<p class="meta">{}</p>"#, esc(n)));
                    }
                    if !columns.is_empty() {
                        out.push_str("<table><thead><tr>");
                        for c in columns {
                            out.push_str(&format!("<th>{}</th>", esc(c)));
                        }
                        out.push_str("</tr></thead><tbody>");
                        for r in rows {
                            out.push_str("<tr>");
                            for c in r {
                                out.push_str(&format!("<td>{}</td>", esc(c)));
                            }
                            out.push_str("</tr>");
                        }
                        out.push_str("</tbody></table>");
                    }
                }
                Block::List(items) => {
                    out.push_str("<ul>");
                    for (tag, text) in items {
                        out.push_str(&format!(
                            r#"<li><span class="tag tag-{}">{}</span>{}</li>"#,
                            esc(&tag.to_ascii_lowercase()),
                            esc(tag),
                            esc(text)
                        ));
                    }
                    out.push_str("</ul>");
                }
                Block::Preformatted(text) => {
                    out.push_str(&format!("<pre>{}</pre>", esc(text)));
                }
            }
        }
        out.push_str(r#"<div class="disclaimer">Model report generated by StormSewer from the engine's own report and results files. The engine is EPA's, unmodified; verify the model and its assumptions before relying on the results.</div>"#);
        out.push_str("</body></html>");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpt;
    use std::path::PathBuf;
    use std::time::Duration;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(rel)
    }

    fn pond_run() -> Run {
        let rpt_path = fixture("results/Detention_Pond_Model.rpt");
        Run {
            engine_id: "5.2.4".into(),
            engine_version: "5.2.4".into(),
            engine_sha256: "ab12".repeat(16),
            inp: fixture("epa-samples/Detention_Pond_Model.inp"),
            rpt: rpt_path.clone(),
            out: fixture("results/Detention_Pond_Model.out"),
            exit_code: Some(0),
            elapsed: Duration::from_millis(1234),
            stdout: String::new(),
            stderr: String::new(),
            report: rpt::read(&rpt_path).unwrap(),
        }
    }

    #[test]
    fn report_carries_the_hash_every_table_and_the_input() {
        let doc = InpDoc::read(&fixture("epa-samples/Detention_Pond_Model.inp")).unwrap();
        let run = pond_run();
        let findings = doc.validate();
        let profile = ProfileTable {
            start: "J1".into(),
            end: "O2".into(),
            columns: vec!["Node".into(), "Station".into()],
            rows: vec![vec!["J1".into(), "0".into()], vec!["O2".into(), "500".into()]],
        };
        let r = build(&ReportInputs {
            doc: &doc,
            run: &run,
            findings: &findings,
            profile: Some(&profile),
            model_name: "Detention_Pond_Model.inp",
            model_sha256: "deadbeef",
            generated: "2026-09-17",
        });
        let html = r.to_html();
        assert!(html.contains(&"ab12".repeat(16)), "engine hash");
        assert!(html.contains("deadbeef"), "model hash");
        assert!(html.contains("EPA SWMM 5.2.4"));
        assert!(run.report.tables.len() >= 9);
        for t in &run.report.tables {
            assert!(html.contains(&esc(&t.title)), "table {}", t.title);
        }
        assert!(html.contains("FLOW_ROUTING") && html.contains("KINWAVE"));
        assert!(html.contains("<td>Conduits</td><td>12</td>"));
        assert!(html.contains("<td>Storage units</td><td>1</td>"));
        assert!(html.contains("Continuity errors"));
        assert!(html.contains("Profile J1 to O2"));
        assert!(html.contains("[TITLE]") && html.contains("[XSECTIONS]"), "appendix");
        assert!(html.contains("hc-formula") || html.contains("table{border-collapse"), "shared stylesheet");
        assert!(!html.contains("http://") && !html.contains("https://"), "self-contained");
        assert!(html.contains("1.23 s"));
        assert!(html.contains("A detention pond model."));
    }

    #[test]
    fn a_failed_run_says_so() {
        let doc = InpDoc::parse("[JUNCTIONS]\nJ1 1\n");
        let mut run = pond_run();
        run.report.errors.push("ERROR 200: something".into());
        run.out = fixture("results/absent.out");
        let r = build(&ReportInputs {
            doc: &doc,
            run: &run,
            findings: &[],
            profile: None,
            model_name: "x.inp",
            model_sha256: "0",
            generated: "now",
        });
        let html = r.to_html();
        assert!(html.contains("FAILED: ERROR 200"));
        assert!(html.contains("tag-error"));
        assert!(html.contains("found nothing"));
        assert!(!html.contains("Profile "));
    }
}
