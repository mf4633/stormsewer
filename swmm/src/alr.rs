// SPDX-License-Identifier: GPL-3.0-or-later

//! Bridge to the ALR (Adaptive Lagrangian Refinement) post-processor.
//!
//! ALR does not replace the SWMM engine — it reads a finished run and checks
//! the result against conditions SWMM's own report does not evaluate. It is a
//! Python package, invoked here through its headless CLI:
//!
//! ```text
//! python run_headless_swmm.py <model.out> --json
//! ```
//!
//! It needs the model's `.inp` **beside** the `.out` to recover geometry, so
//! that is checked before launching rather than left to a Python traceback.
//!
//! The JSON is parsed leniently: every field is optional and unknown keys are
//! kept, so a newer ALR that reports more can still be read by this build.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::{Error, Result};

/// A configured ALR installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alr {
    /// Interpreter to run, e.g. `python` or a virtualenv's binary.
    pub python: PathBuf,
    /// Path to `run_headless_swmm.py`.
    pub script: PathBuf,
}

impl Alr {
    pub fn new(python: impl Into<PathBuf>, script: impl Into<PathBuf>) -> Self {
        Self { python: python.into(), script: script.into() }
    }

    /// Read the configuration from the environment:
    /// `STORMSEWER_ALR_SCRIPT`, and `STORMSEWER_ALR_PYTHON` if the
    /// interpreter is not simply `python`.
    pub fn from_env() -> Option<Self> {
        let script = std::env::var("STORMSEWER_ALR_SCRIPT").ok()?;
        let python =
            std::env::var("STORMSEWER_ALR_PYTHON").unwrap_or_else(|_| "python".to_string());
        Some(Self::new(python, script))
    }

    pub fn is_available(&self) -> bool {
        self.script.is_file()
    }

    /// Analyse a finished run.
    pub fn analyze(&self, out_file: &Path, options: &AlrOptions) -> Result<AlrReport> {
        if !self.script.is_file() {
            return Err(Error::Alr(format!(
                "ALR script not found at {} — set STORMSEWER_ALR_SCRIPT",
                self.script.display()
            )));
        }
        if !out_file.is_file() {
            return Err(Error::Alr(format!(
                "no results file at {}",
                out_file.display()
            )));
        }
        // ALR reads geometry from the .inp of the same name; without it the
        // run fails deep inside Python with a much less useful message.
        let inp = out_file.with_extension("inp");
        if !inp.is_file() {
            return Err(Error::Alr(format!(
                "ALR needs the model input beside the results, but {} is missing",
                inp.display()
            )));
        }

        let mut command = Command::new(&self.python);
        command.arg(&self.script).arg(out_file).arg("--json");
        if let Some(top) = options.top {
            command.arg("--top").arg(top.to_string());
        }
        if options.all {
            command.arg("--all");
        }
        if !options.nodes.is_empty() {
            command.arg("--nodes").arg(options.nodes.join(","));
        }

        let output = command.output().map_err(|e| {
            Error::Alr(format!(
                "could not start ALR with {}: {e}",
                self.python.display()
            ))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json = extract_json(&stdout).ok_or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            Error::Alr(format!("ALR produced no JSON report. It said: {detail}"))
        })?;

        serde_json::from_str(json)
            .map_err(|e| Error::Alr(format!("could not read ALR's JSON report: {e}")))
    }
}

/// Which parts of a model ALR should look at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlrOptions {
    /// Restrict the scan to these node names.
    pub nodes: Vec<String>,
    /// Only the worst `n`.
    pub top: Option<u32>,
    /// Report every check, not only failures.
    pub all: bool,
}

/// ALR's verdict on a run. Unknown fields are preserved in `extra` so a report
/// from a newer ALR is never silently truncated.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AlrReport {
    #[serde(default)]
    pub all_pass: bool,
    #[serde(default)]
    pub passed: u32,
    #[serde(default)]
    pub failed: u32,
    #[serde(default)]
    pub checks: Vec<AlrCheck>,
    #[serde(default)]
    pub scan: serde_json::Value,
    #[serde(default)]
    pub peak_conditions: serde_json::Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl AlrReport {
    pub fn failures(&self) -> impl Iterator<Item = &AlrCheck> {
        self.checks.iter().filter(|c| !c.passed)
    }

    /// One line for a status bar.
    pub fn summary(&self) -> String {
        // The counts carry meaning even when the detail array is absent, so
        // only a report with neither is genuinely empty.
        if self.checks.is_empty() && self.passed == 0 && self.failed == 0 {
            return "ALR reported no checks".to_string();
        }
        if self.all_pass {
            format!("ALR: all {} checks passed", self.passed)
        } else {
            format!("ALR: {} passed, {} failed", self.passed, self.failed)
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AlrCheck {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Pull the JSON object out of stdout. ALR may log before it prints, so the
/// report is taken as the text from the first `{` to the last `}`.
fn extract_json(stdout: &str) -> Option<&str> {
    let start = stdout.find('{')?;
    let end = stdout.rfind('}')?;
    (end > start).then(|| &stdout[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_json_after_log_noise() {
        let text = "loading model...\nscanning 42 nodes\n{\"all_pass\": true}\n";
        assert_eq!(extract_json(text), Some("{\"all_pass\": true}"));
        assert_eq!(extract_json("no json here"), None);
        assert_eq!(extract_json(""), None);
    }

    #[test]
    fn parses_a_report() {
        let json = r#"{
            "scan": {"analyzed": 42},
            "passed": 3,
            "failed": 1,
            "all_pass": false,
            "peak_conditions": {"node": "JN_Toe", "depth": 4.1},
            "checks": [
                {"name": "froude", "passed": true, "detail": "subcritical"},
                {"name": "surcharge", "passed": false, "detail": "JN_Toe exceeds crown",
                 "margin": 0.42}
            ]
        }"#;
        let report: AlrReport = serde_json::from_str(json).unwrap();
        assert!(!report.all_pass);
        assert_eq!(report.passed, 3);
        assert_eq!(report.failed, 1);
        assert_eq!(report.summary(), "ALR: 3 passed, 1 failed");

        let failures: Vec<_> = report.failures().collect();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "surcharge");
        assert_eq!(failures[0].detail.as_deref(), Some("JN_Toe exceeds crown"));
        // A field this build does not know about survives the round trip.
        assert_eq!(
            failures[0].extra.get("margin").and_then(|v| v.as_f64()),
            Some(0.42)
        );
        assert_eq!(report.scan.get("analyzed").and_then(|v| v.as_u64()), Some(42));
    }

    /// A report from a newer ALR carrying keys this build has never seen must
    /// still parse.
    #[test]
    fn tolerates_unknown_fields() {
        let report: AlrReport =
            serde_json::from_str(r#"{"all_pass": true, "passed": 7, "vorticity_index": 1.5}"#)
                .unwrap();
        assert!(report.all_pass);
        assert_eq!(report.summary(), "ALR: all 7 checks passed");
        assert!(report.extra.contains_key("vorticity_index"));
    }

    #[test]
    fn empty_report_is_survivable() {
        let report: AlrReport = serde_json::from_str("{}").unwrap();
        assert_eq!(report.summary(), "ALR reported no checks");
    }

    #[test]
    fn missing_script_is_named() {
        let alr = Alr::new("python", std::env::temp_dir().join("no-such-alr.py"));
        assert!(!alr.is_available());
        let err = alr
            .analyze(Path::new("whatever.out"), &AlrOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("STORMSEWER_ALR_SCRIPT"), "{err}");
    }

    /// The .inp-beside-the-.out requirement is checked before Python starts,
    /// so the message names the real problem.
    #[test]
    fn missing_input_beside_results_is_explained() {
        let dir = std::env::temp_dir().join("stormsewer-swmm-tests/alr");
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("run_headless_swmm.py");
        std::fs::write(&script, b"# stand-in").unwrap();
        let out = dir.join("lonely.out");
        std::fs::write(&out, b"results").unwrap();
        let _ = std::fs::remove_file(dir.join("lonely.inp"));

        let err = Alr::new("python", &script)
            .analyze(&out, &AlrOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs the model input beside"), "{err}");
        assert!(err.contains("lonely.inp"), "{err}");
    }
}
