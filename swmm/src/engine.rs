// SPDX-License-Identifier: GPL-3.0-or-later

//! Registry of EPA SWMM engines, and running one.
//!
//! An engine is a `runswmm.exe` (or `runswmm`) on disk. Several may be
//! registered at once — 5.1.015 beside 5.2.4, say — and the user picks which
//! one runs a given model. Each is identified by the version it reports and
//! by the SHA-256 of the executable itself, both of which are stamped into
//! every [`Run`], so a report produced today can be tied to an exact binary
//! later.
//!
//! Two behaviours of `runswmm` shape this module:
//!
//! 1. **It always exits 0.** Success, fatal input error, and no arguments at
//!    all all give status 0. Success is therefore decided by reading the
//!    report and checking that results were written — never by exit status.
//! 2. **It appends to an existing report.** Stale `.rpt` and `.out` files from
//!    a previous run are deleted before launching, so a run that dies without
//!    writing cannot be mistaken for one that succeeded.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::pe::{arch_of, Arch};
use crate::rpt::{self, ReportSummary};
use crate::sha256::sha256_file;
use crate::{Error, Result};

/// The executable name EPA ships for command-line runs.
#[cfg(windows)]
pub const RUNNER_EXE: &str = "runswmm.exe";
#[cfg(not(windows))]
pub const RUNNER_EXE: &str = "runswmm";

/// One registered SWMM engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    /// Stable key for persisting a choice, e.g. `5.2.4` or `5.2.4-x86`.
    pub id: String,
    /// Version as the engine reports it, or `unknown` if it would not say.
    pub version: String,
    pub exe: PathBuf,
    pub arch: Arch,
    pub sha256: String,
}

impl Engine {
    /// Probe an executable and describe it. Fails only if the file cannot be
    /// read; an engine too old to answer `--version` still registers.
    pub fn probe(exe: impl Into<PathBuf>) -> Result<Self> {
        let exe = exe.into();
        if !exe.is_file() {
            return Err(Error::NotFound(format!(
                "no SWMM runner at {}",
                exe.display()
            )));
        }
        let arch = arch_of(&exe).unwrap_or(Arch::NotPe);
        let sha256 = sha256_file(&exe)?;
        let version = probe_version(&exe).unwrap_or_else(|| "unknown".to_string());

        // Two installs can report the same version while differing in
        // architecture; keep both addressable.
        let id = match arch {
            Arch::X86 => format!("{version}-x86"),
            _ => version.clone(),
        };

        Ok(Self { id, version, exe, arch, sha256 })
    }

    /// A one-line description for a menu: `EPA SWMM 5.2.4 — 32-bit (x86)`.
    pub fn label(&self) -> String {
        format!("EPA SWMM {} — {}", self.version, self.arch.label())
    }

    /// Run a model. `inp` must exist; the report and results land beside it
    /// unless [`RunPaths`] says otherwise.
    pub fn run(&self, inp: &Path) -> Result<Run> {
        self.run_with(&RunPaths::beside(inp)?)
    }

    pub fn run_with(&self, paths: &RunPaths) -> Result<Run> {
        if !paths.inp.is_file() {
            return Err(Error::NotFound(format!(
                "no input file at {}",
                paths.inp.display()
            )));
        }
        for out_path in [&paths.rpt, &paths.out] {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // runswmm appends to an existing report, and leaves an old results
            // file untouched when it fails early. Either would let a failed run
            // read as a successful one.
            if out_path.exists() {
                std::fs::remove_file(out_path)?;
            }
        }

        let started = Instant::now();
        let output = Command::new(&self.exe)
            .arg(&paths.inp)
            .arg(&paths.rpt)
            .arg(&paths.out)
            .output()
            .map_err(|e| {
                Error::Engine(format!("could not start {}: {e}", self.exe.display()))
            })?;
        let elapsed = started.elapsed();

        // Absent report: the engine never got far enough to write one.
        let report = if paths.rpt.is_file() {
            rpt::read(&paths.rpt)?
        } else {
            ReportSummary::default()
        };

        Ok(Run {
            engine_id: self.id.clone(),
            engine_version: self.version.clone(),
            engine_sha256: self.sha256.clone(),
            inp: paths.inp.clone(),
            rpt: paths.rpt.clone(),
            out: paths.out.clone(),
            exit_code: output.status.code(),
            elapsed,
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            report,
        })
    }
}

/// Ask an engine its version. Returns `None` when the output does not look
/// like one — `runswmm` exits 0 even when it does not understand the flag, so
/// the text itself is the only test.
fn probe_version(exe: &Path) -> Option<String> {
    let output = Command::new(exe).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let token = text.split_whitespace().next()?.trim();
    let looks_like_version =
        token.starts_with(|c: char| c.is_ascii_digit()) && token.contains('.');
    looks_like_version.then(|| token.to_string())
}

/// Where a run reads from and writes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunPaths {
    pub inp: PathBuf,
    pub rpt: PathBuf,
    pub out: PathBuf,
}

impl RunPaths {
    /// Report and results beside the input, named after it — EPA's own
    /// convention, and what the ALR bridge expects since it needs the `.inp`
    /// next to the `.out`.
    pub fn beside(inp: &Path) -> Result<Self> {
        let stem = inp
            .file_stem()
            .ok_or_else(|| Error::NotFound(format!("{} has no file name", inp.display())))?;
        let dir = inp.parent().unwrap_or_else(|| Path::new("."));
        Ok(Self {
            inp: inp.to_path_buf(),
            rpt: dir.join(stem).with_extension("rpt"),
            out: dir.join(stem).with_extension("out"),
        })
    }
}

/// The outcome of one engine run.
#[derive(Clone, Debug)]
pub struct Run {
    pub engine_id: String,
    pub engine_version: String,
    /// SHA-256 of the executable that produced this result.
    pub engine_sha256: String,
    pub inp: PathBuf,
    pub rpt: PathBuf,
    pub out: PathBuf,
    /// Recorded for the log only. **Not** a success signal: see the module
    /// documentation.
    pub exit_code: Option<i32>,
    pub elapsed: Duration,
    pub stdout: String,
    pub stderr: String,
    pub report: ReportSummary,
}

impl Run {
    /// The run finished and produced results worth reading: the report is free
    /// of fatal errors and a non-empty results file exists.
    pub fn succeeded(&self) -> bool {
        self.report.is_clean() && self.wrote_results()
    }

    pub fn wrote_results(&self) -> bool {
        std::fs::metadata(&self.out).map(|m| m.len() > 0).unwrap_or(false)
    }

    /// Why the run failed, in one line, or `None` if it did not.
    pub fn failure_reason(&self) -> Option<String> {
        if self.succeeded() {
            return None;
        }
        if let Some(first) = self.report.errors.first() {
            return Some(first.clone());
        }
        if !self.rpt.is_file() {
            return Some(format!(
                "the engine wrote no report — it may not have started (exit status {})",
                self.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into())
            ));
        }
        if !self.wrote_results() {
            return Some(
                "the engine reported no errors but wrote no results file".to_string(),
            );
        }
        Some("the run failed for an unrecorded reason".to_string())
    }
}

/// The set of engines available to run models.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    engines: Vec<Engine>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Search the usual install locations plus `PATH`. Never fails: an empty
    /// registry is the normal result on a machine with no SWMM installed, and
    /// the user can always register a folder by hand.
    pub fn discover() -> Self {
        let mut registry = Self::new();
        for dir in search_roots() {
            let candidate = dir.join(RUNNER_EXE);
            if candidate.is_file() {
                let _ = registry.register(&candidate);
            }
        }
        registry
    }

    /// Add an engine by the path to its runner, or to the folder holding it.
    /// Re-registering a binary already present replaces it rather than
    /// duplicating it.
    pub fn register(&mut self, path: &Path) -> Result<Engine> {
        let exe = if path.is_dir() { path.join(RUNNER_EXE) } else { path.to_path_buf() };
        let engine = Engine::probe(exe)?;
        self.engines.retain(|e| e.sha256 != engine.sha256);
        self.engines.push(engine.clone());
        // Newest version first, so the default choice is the current engine.
        self.engines.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(engine)
    }

    pub fn engines(&self) -> &[Engine] {
        &self.engines
    }

    pub fn is_empty(&self) -> bool {
        self.engines.is_empty()
    }

    pub fn by_id(&self, id: &str) -> Option<&Engine> {
        self.engines.iter().find(|e| e.id == id)
    }

    /// The engine to use when the user has not chosen one.
    pub fn default_engine(&self) -> Option<&Engine> {
        self.engines.first()
    }
}

/// Directories that may hold a SWMM runner.
fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    // An explicit override always wins, and is how CI or a portable install
    // points at an engine.
    if let Ok(dir) = std::env::var("STORMSEWER_SWMM_ENGINE_DIR") {
        roots.push(PathBuf::from(dir));
    }

    // EPA installs to "EPA SWMM 5.2.4" under Program Files; the 32-bit build
    // lands in the x86 tree even on 64-bit Windows.
    for var in ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"] {
        let Ok(base) = std::env::var(var) else { continue };
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("EPA SWMM") || name.starts_with("SWMM") {
                roots.push(entry.path());
            }
        }
    }

    if let Some(path) = std::env::var_os("PATH") {
        roots.extend(std::env::split_paths(&path));
    }

    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-swmm-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn run_paths_sit_beside_the_input() {
        let paths = RunPaths::beside(Path::new("/models/teton.inp")).unwrap();
        assert_eq!(paths.rpt.file_name().unwrap(), "teton.rpt");
        assert_eq!(paths.out.file_name().unwrap(), "teton.out");
        assert_eq!(paths.rpt.parent(), paths.inp.parent());
    }

    #[test]
    fn missing_runner_is_named() {
        let err = Engine::probe(scratch().join("no-such-runswmm.exe"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no SWMM runner"), "{err}");
    }

    /// Discovery must be safe on a machine with nothing installed — this is
    /// what CI exercises.
    #[test]
    fn discovery_never_panics() {
        let registry = Registry::discover();
        for engine in registry.engines() {
            assert!(!engine.sha256.is_empty());
            assert!(!engine.label().is_empty());
        }
    }

    #[test]
    fn registering_the_same_binary_twice_does_not_duplicate_it() {
        // A file that is not a real engine still registers: probing only needs
        // to read it, and version falls back to "unknown".
        let fake = scratch().join(RUNNER_EXE);
        std::fs::write(&fake, b"not really an engine").unwrap();

        let mut registry = Registry::new();
        registry.register(&fake).unwrap();
        registry.register(&fake).unwrap();
        assert_eq!(registry.engines().len(), 1);
        assert!(!registry.is_empty());

        let engine = &registry.engines()[0];
        assert_eq!(engine.version, "unknown");
        assert_eq!(engine.arch, Arch::NotPe);
        assert_eq!(registry.default_engine().unwrap().id, engine.id);
        assert!(registry.by_id(&engine.id).is_some());
        assert!(registry.by_id("5.2.4").is_none());

        // Registering the containing folder resolves to the same binary.
        let mut by_dir = Registry::new();
        by_dir.register(&scratch()).unwrap();
        assert_eq!(by_dir.engines().len(), 1);
    }

    #[test]
    fn a_run_without_results_reports_why() {
        let run = Run {
            engine_id: "5.2.4".into(),
            engine_version: "5.2.4".into(),
            engine_sha256: "0".repeat(64),
            inp: scratch().join("model.inp"),
            rpt: scratch().join("absent.rpt"),
            out: scratch().join("absent.out"),
            exit_code: Some(0),
            elapsed: Duration::from_millis(5),
            stdout: String::new(),
            stderr: String::new(),
            report: ReportSummary::default(),
        };
        // Exit code 0 with no report is exactly the case that makes exit
        // status unusable.
        assert!(!run.succeeded());
        assert!(run.failure_reason().unwrap().contains("no report"));
    }

    /// Opt-in end-to-end run against a real engine. Set
    /// STORMSEWER_SWMM_ENGINE_DIR and STORMSEWER_SWMM_FIXTURES to exercise it.
    #[test]
    fn runs_a_real_model_when_an_engine_is_available() {
        let (Ok(_), Ok(fixtures)) = (
            std::env::var("STORMSEWER_SWMM_ENGINE_DIR"),
            std::env::var("STORMSEWER_SWMM_FIXTURES"),
        ) else {
            eprintln!("skipped: engine dir or fixtures not set");
            return;
        };
        let registry = Registry::discover();
        let Some(engine) = registry.default_engine() else {
            eprintln!("skipped: no engine discovered");
            return;
        };

        let source = std::fs::read_dir(&fixtures)
            .expect("fixture dir")
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("inp"))
            .expect("an .inp fixture");

        // Copy into scratch so the run cannot disturb the fixtures.
        let work = scratch().join("live-run");
        std::fs::create_dir_all(&work).unwrap();
        let inp = work.join(source.file_name().unwrap());
        std::fs::copy(&source, &inp).unwrap();

        let run = engine.run(&inp).unwrap();
        eprintln!(
            "{} -> succeeded={} exit={:?} errors={} warnings={} in {:?}",
            engine.label(),
            run.succeeded(),
            run.exit_code,
            run.report.errors.len(),
            run.report.warnings.len(),
            run.elapsed
        );
        assert!(run.succeeded(), "{:?}", run.failure_reason());
        assert_eq!(run.report.engine_version.as_deref(), Some(engine.version.as_str()));
        assert_eq!(run.engine_sha256.len(), 64);

        // The results must be readable by our own reader — this is the real
        // end-to-end check that the .out parser matches what the engine writes.
        let results = crate::out::OutputFile::open(&run.out).unwrap();
        assert!(results.meta.n_periods > 0);
    }
}
