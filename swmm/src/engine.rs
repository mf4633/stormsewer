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
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// A run in progress: the engine child process and what it was given. Made
/// by [`Engine::spawn`]; finished by [`RunHandle::wait`], polled by
/// [`RunHandle::try_finish`], or ended early by [`RunHandle::kill`], which
/// still collects whatever partial `.rpt` the engine had written.
#[derive(Debug)]
pub struct RunHandle {
    engine: Engine,
    paths: RunPaths,
    child: Child,
    started: Instant,
    /// Drained on threads: `runswmm` prints a progress line per simulated
    /// hour, and a long run left unread would fill the pipe and stall.
    stdout: Option<std::thread::JoinHandle<Vec<u8>>>,
    stderr: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl RunHandle {
    pub fn paths(&self) -> &RunPaths {
        &self.paths
    }

    pub fn started(&self) -> Instant {
        self.started
    }

    /// Wall-clock time since the engine was launched.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Gather the run. `grace` bounds how long to wait for the engine's
    /// stdout/stderr to reach end-of-file: `None` after a normal exit (the
    /// pipes close with the process), a short limit after a kill, because a
    /// child the engine started — a shell wrapper's `sleep`, say — can hold
    /// the pipe open long after the engine itself is gone. Output not in by
    /// then is left behind rather than making Stop wait for it.
    fn collect(mut self, exit_code: Option<i32>, grace: Option<Duration>) -> Result<Run> {
        let elapsed = self.started.elapsed();
        let deadline = grace.map(|g| Instant::now() + g);
        let drain = |h: Option<std::thread::JoinHandle<Vec<u8>>>| -> Vec<u8> {
            let Some(h) = h else { return Vec::new() };
            if let Some(deadline) = deadline {
                while !h.is_finished() {
                    if Instant::now() >= deadline {
                        return Vec::new();
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            h.join().unwrap_or_default()
        };
        let stdout = drain(self.stdout.take());
        let stderr = drain(self.stderr.take());
        // A report cut short by a kill is still a report: `rpt::read`
        // parses whatever lines are there.
        let report = if self.paths.rpt.is_file() {
            rpt::read(&self.paths.rpt)?
        } else {
            ReportSummary::default()
        };
        Ok(Run {
            engine_id: self.engine.id.clone(),
            engine_version: self.engine.version.clone(),
            engine_sha256: self.engine.sha256.clone(),
            inp: self.paths.inp.clone(),
            rpt: self.paths.rpt.clone(),
            out: self.paths.out.clone(),
            exit_code,
            elapsed,
            stdout: String::from_utf8_lossy(&stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
            report,
        })
    }

    /// Block until the engine exits.
    pub fn wait(mut self) -> Result<Run> {
        let status = self
            .child
            .wait()
            .map_err(|e| Error::Engine(format!("waiting for the engine failed: {e}")))?;
        self.collect(status.code(), None)
    }

    /// The finished run once the engine has exited, or the handle back
    /// while it is still running.
    pub fn try_finish(self) -> Result<std::result::Result<Run, RunHandle>> {
        let mut this = self;
        match this.child.try_wait() {
            Ok(Some(status)) => this.collect(status.code(), None).map(Ok),
            Ok(None) => Ok(Err(this)),
            Err(e) => Err(Error::Engine(format!("polling the engine failed: {e}"))),
        }
    }

    /// Stop the engine now and collect the partial run. The `.rpt` holds
    /// whatever the engine had flushed; the `.out` is missing its closing
    /// block and needs [`crate::out::OutputFile::open_partial`].
    pub fn kill(mut self) -> Result<Run> {
        let _ = self.child.kill();
        let status = self.child.wait().ok();
        self.collect(status.and_then(|s| s.code()), Some(Duration::from_secs(2)))
    }

    /// Wait for the engine, checking `cancel` every `poll`; when it is set
    /// the engine is killed. Returns the run and whether it was cancelled.
    pub fn wait_or_cancel(self, cancel: &AtomicBool, poll: Duration) -> Result<(Run, bool)> {
        let mut handle = self;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return handle.kill().map(|run| (run, true));
            }
            match handle.try_finish()? {
                Ok(run) => return Ok((run, false)),
                Err(h) => handle = h,
            }
            std::thread::sleep(poll);
        }
    }
}

impl Engine {
    /// Launch a run without waiting for it. Stale `.rpt`/`.out` files are
    /// removed first, as in [`Engine::run_with`].
    pub fn spawn(&self, paths: &RunPaths) -> Result<RunHandle> {
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
            if out_path.exists() {
                std::fs::remove_file(out_path)?;
            }
        }
        let started = Instant::now();
        let mut child = Command::new(&self.exe)
            .arg(&paths.inp)
            .arg(&paths.rpt)
            .arg(&paths.out)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::Engine(format!("could not start {}: {e}", self.exe.display()))
            })?;
        fn drain(r: Option<impl std::io::Read + Send + 'static>) -> Option<std::thread::JoinHandle<Vec<u8>>> {
            r.map(|mut r| {
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    let _ = r.read_to_end(&mut buf);
                    buf
                })
            })
        }
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        Ok(RunHandle {
            engine: self.clone(),
            paths: paths.clone(),
            child,
            started,
            stdout,
            stderr,
        })
    }

    /// Run a model, killing the engine as soon as `cancel` is set. Returns
    /// the run (partial when cancelled) and whether it was cancelled.
    pub fn run_with_cancel(&self, paths: &RunPaths, cancel: &AtomicBool) -> Result<(Run, bool)> {
        self.spawn(paths)?
            .wait_or_cancel(cancel, Duration::from_millis(25))
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

// ---------------------------------------------------------------------------
// Scratch copies: non-ASCII paths, dirty documents, and [FILES] references
// ---------------------------------------------------------------------------

/// Whether every character of the path is ASCII. The stock EPA engines
/// open files with the C runtime's narrow-character calls, so a path with
/// `õ`, `ü` or a CJK character fails with ERROR 303/305/307 even though
/// the file is there.
pub fn is_ascii_path(path: &Path) -> bool {
    path.to_string_lossy().is_ascii()
}

/// The folder scratch runs go under: `%TEMP%\StormSewer\run`, or, when the
/// temp folder itself has a non-ASCII name (a user called `Jörg`), the
/// first of `%ProgramData%\StormSewer\run`, `%SystemDrive%\StormSewer\run`
/// and `/tmp/StormSewer/run` that is ASCII.
pub fn scratch_root() -> PathBuf {
    let mut candidates = vec![std::env::temp_dir()];
    if let Some(p) = std::env::var_os("ProgramData") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(p) = std::env::var_os("SystemDrive") {
        candidates.push(PathBuf::from(format!("{}\\", p.to_string_lossy())));
    }
    candidates.push(PathBuf::from("/tmp"));
    candidates
        .into_iter()
        .find(|p| is_ascii_path(p))
        .unwrap_or_else(std::env::temp_dir)
        .join("StormSewer")
        .join("run")
}

/// Name of the marker file each scratch folder carries; its modified time
/// is the folder's age.
const STAMP: &str = ".stormsewer-run";

/// Remove scratch folders whose stamp is older than `max_age`. Returns how
/// many were removed. Never fails: a folder in use is simply left.
pub fn clean_stale_scratch(max_age: Duration) -> usize {
    let root = scratch_root();
    let Ok(entries) = std::fs::read_dir(&root) else { return 0 };
    let now = std::time::SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let stamp = dir.join(STAMP);
        let Ok(meta) = std::fs::metadata(&stamp) else { continue };
        let Ok(modified) = meta.modified() else { continue };
        let stale = now.duration_since(modified).is_ok_and(|age| age > max_age);
        if stale && std::fs::remove_dir_all(&dir).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// A file name with every non-ASCII character replaced.
fn ascii_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii() { c } else { '_' })
        .collect()
}

/// A model laid out for the engine, possibly in a scratch folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub paths: RunPaths,
    /// Why the run reads a scratch copy, when it does.
    pub note: Option<String>,
    /// `[FILES]` and other external inputs copied beside the scratch model:
    /// `original → copy`.
    pub copied: Vec<(PathBuf, PathBuf)>,
    /// `[FILES] SAVE` outputs redirected into the scratch folder, to be
    /// copied back to where the model wanted them after the run.
    pub save_back: Vec<(PathBuf, PathBuf)>,
}

impl Prepared {
    /// The run reads a scratch copy, not the model's own file.
    pub fn is_scratch(&self) -> bool {
        self.note.is_some()
    }

    /// After a run, put every redirected `SAVE` file where the model
    /// asked for it. Returns the destinations written.
    pub fn copy_back(&self) -> Vec<PathBuf> {
        let mut done = Vec::new();
        for (scratch, original) in &self.save_back {
            if !scratch.is_file() {
                continue;
            }
            if let Some(parent) = original.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::copy(scratch, original).is_ok() {
                done.push(original.clone());
            }
        }
        done
    }
}

/// `[FILES]` interface kinds and the extension a copy gets.
fn files_extension(kind: &str) -> &'static str {
    match kind.to_ascii_uppercase().as_str() {
        "HOTSTART" => "hsf",
        "RAINFALL" => "rff",
        "RUNOFF" => "rof",
        "RDII" => "rdi",
        "INFLOWS" | "OUTFLOWS" => "txt",
        _ => "dat",
    }
}

/// Lay `text` (the model as the editor holds it) out for a run. When the
/// model's own path and folder are ASCII and `force_scratch` is false, the
/// run happens beside the file. Otherwise the text goes to
/// `scratch_root()/<hash of the model path>/<ascii name>.inp`, every
/// `[FILES]` row and every external file named by `[RAINGAGES]`,
/// `[TIMESERIES]` and `[TEMPERATURE]` is rewritten to its absolute path so
/// the engine finds it from the new folder. A file whose own path is not
/// ASCII is copied beside the scratch model (with an ASCII name) instead,
/// and a `SAVE` target like that is written there and copied back after
/// the run (see [`Prepared::copy_back`]).
pub fn prepare(model: &Path, text: &str, force_scratch: bool) -> Result<Prepared> {
    let ascii = is_ascii_path(model);
    if ascii && !force_scratch {
        return Ok(Prepared {
            paths: RunPaths::beside(model)?,
            note: None,
            copied: Vec::new(),
            save_back: Vec::new(),
        });
    }
    let base = model
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let key = crate::sha256::sha256_hex(model.to_string_lossy().as_bytes());
    let dir = scratch_root().join(&key[..16]);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(STAMP), b"")?;
    let stem = model
        .file_name()
        .map(|s| ascii_name(&s.to_string_lossy()))
        .unwrap_or_else(|| "model.inp".into());
    let inp = dir.join(stem).with_extension("inp");

    let mut doc = crate::doc::InpDoc::parse(text);
    let mut copied = Vec::new();
    let mut save_back = Vec::new();
    let mut n = 0usize;
    // A path as the model names it, resolved against its folder; an
    // absolute ASCII path is used as is, anything else is copied beside
    // the scratch model.
    let mut relocate = |raw: &str, kind: &str, copied: &mut Vec<(PathBuf, PathBuf)>| -> String {
        let original = {
            let p = PathBuf::from(raw);
            if p.is_absolute() { p } else { base.join(p) }
        };
        // An ASCII file is reachable from anywhere by its absolute path;
        // only a non-ASCII one has to travel.
        if is_ascii_path(&original) || !original.is_file() {
            return original.to_string_lossy().into_owned();
        }
        n += 1;
        let ext = original
            .extension()
            .map(|e| ascii_name(&e.to_string_lossy()))
            .unwrap_or_else(|| files_extension(kind).into());
        let copy = dir.join(format!("use_{}_{n}.{ext}", kind.to_ascii_lowercase()));
        if std::fs::copy(&original, &copy).is_ok() {
            copied.push((original, copy.clone()));
            copy.to_string_lossy().into_owned()
        } else {
            original.to_string_lossy().into_owned()
        }
    };
    let mut cmds = Vec::new();
    if let Some(files) = doc.section("FILES") {
        for (li, row) in files.rows() {
            let (Some(verb), Some(kind), Some(path)) = (row.value(0), row.value(1), row.value(2))
            else {
                continue;
            };
            let new = if verb.eq_ignore_ascii_case("USE") {
                relocate(path, kind, &mut copied)
            } else if verb.eq_ignore_ascii_case("SAVE") {
                let original = {
                    let p = PathBuf::from(path);
                    if p.is_absolute() { p } else { base.join(p) }
                };
                if is_ascii_path(&original) {
                    original.to_string_lossy().into_owned()
                } else {
                    let ext = original
                        .extension()
                        .map(|e| ascii_name(&e.to_string_lossy()))
                        .unwrap_or_else(|| files_extension(kind).into());
                    let target = dir.join(format!("save_{}.{ext}", kind.to_ascii_lowercase()));
                    save_back.push((target.clone(), original));
                    target.to_string_lossy().into_owned()
                }
            } else {
                continue;
            };
            if new != path {
                let mut fields = row.fields.clone();
                fields[2] = new;
                cmds.push(crate::doc::Command::SetLine {
                    section: "FILES".into(),
                    line: li,
                    fields,
                    comment: row.comment.clone(),
                });
            }
        }
    }
    // External data files named elsewhere: `[RAINGAGES] ... FILE "x" sta
    // units`, `[TIMESERIES] name FILE "x"`, `[TEMPERATURE] FILE "x"`.
    let externals: [(&str, usize, usize); 3] = [("RAINGAGES", 4, 5), ("TIMESERIES", 1, 2), ("TEMPERATURE", 0, 1)];
    for (section, kw_field, path_field) in externals {
        if let Some(s) = doc.section(section) {
            for (li, row) in s.rows() {
                if !row.value(kw_field).is_some_and(|k| k.eq_ignore_ascii_case("FILE")) {
                    continue;
                }
                let Some(path) = row.value(path_field) else { continue };
                let new = relocate(path, "data", &mut copied);
                if new != path {
                    let mut fields = row.fields.clone();
                    fields[path_field] = new;
                    cmds.push(crate::doc::Command::SetLine {
                        section: section.into(),
                        line: li,
                        fields,
                        comment: row.comment.clone(),
                    });
                }
            }
        }
    }
    if !cmds.is_empty() {
        doc.apply(crate::doc::Command::Batch(cmds))?;
    }
    std::fs::write(&inp, doc.to_string())?;

    let mut why = Vec::new();
    if !ascii {
        why.push("the model's path has characters outside ASCII, which the stock engines cannot open".to_string());
    }
    if force_scratch {
        why.push("the model has unsaved changes (or no file yet)".to_string());
    }
    let mut note = format!("Ran a scratch copy at {} because {}.", inp.display(), why.join(" and "));
    if !copied.is_empty() {
        note.push_str(&format!(" {} referenced file(s) were copied beside it.", copied.len()));
    }
    if !save_back.is_empty() {
        note.push_str(&format!(
            " {} [FILES] SAVE output(s) are copied back to the model's folder after the run.",
            save_back.len()
        ));
    }
    Ok(Prepared {
        paths: RunPaths::beside(&inp)?,
        note: Some(note),
        copied,
        save_back,
    })
}

impl Engine {
    /// Run a prepared model and copy any redirected `SAVE` files back.
    pub fn run_prepared(&self, prepared: &Prepared) -> Result<Run> {
        let run = self.run_with(&prepared.paths)?;
        prepared.copy_back();
        Ok(run)
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

    #[test]
    fn non_ascii_models_run_from_an_ascii_scratch_copy_with_their_files() {
        let work = scratch().join("modèle-ü");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(work.join("data")).unwrap();
        let model = work.join("réseau.inp");
        std::fs::write(work.join("data/warm.hsf"), b"hot").unwrap();
        std::fs::write(work.join("rain.dat"), b"rain").unwrap();
        let text = "[OPTIONS]\nFLOW_UNITS CFS\n[FILES]\nUSE HOTSTART \"data/warm.hsf\"\nSAVE HOTSTART out.hsf\n[RAINGAGES]\nRG1 VOLUME 1:00 1.0 FILE \"rain.dat\" STA1 IN\n[JUNCTIONS]\nJ1 0\n";
        std::fs::write(&model, text).unwrap();
        assert!(!is_ascii_path(&model));

        let p = prepare(&model, text, false).unwrap();
        assert!(p.is_scratch());
        assert!(is_ascii_path(&p.paths.inp), "{}", p.paths.inp.display());
        assert!(p.paths.inp.starts_with(scratch_root()));
        assert_eq!(p.paths.inp.file_name().unwrap(), "r_seau.inp");
        assert!(p.paths.inp.parent().unwrap().join(STAMP).is_file());
        let note = p.note.clone().unwrap();
        assert!(note.contains("outside ASCII"), "{note}");
        assert!(note.contains("2 referenced file(s)"), "{note}");
        // The hotstart and rain files travelled, with ASCII names, and the
        // scratch model names the copies.
        assert_eq!(p.copied.len(), 2);
        let written = std::fs::read_to_string(&p.paths.inp).unwrap();
        for (orig, copy) in &p.copied {
            assert!(copy.is_file());
            assert!(is_ascii_path(copy));
            assert_eq!(std::fs::read(orig).unwrap(), std::fs::read(copy).unwrap());
            assert!(written.contains(&copy.to_string_lossy().to_string()), "{written}");
        }
        assert!(!written.contains("data/warm.hsf"), "{written}");
        // SAVE is redirected and copied back after the run.
        assert_eq!(p.save_back.len(), 1);
        let (target, original) = p.save_back[0].clone();
        assert_eq!(original, work.join("out.hsf"));
        assert!(written.contains(&target.to_string_lossy().to_string()));
        std::fs::write(&target, b"saved").unwrap();
        assert_eq!(p.copy_back(), vec![original.clone()]);
        assert_eq!(std::fs::read(&original).unwrap(), b"saved");
        // Everything else in the text survived the rewrite.
        assert!(written.contains("J1 0"));

        // An ASCII model with nothing forcing a copy runs in place.
        let plain = scratch().join("plain.inp");
        std::fs::write(&plain, text).unwrap();
        let p = prepare(&plain, text, false).unwrap();
        assert!(!p.is_scratch());
        assert_eq!(p.paths, RunPaths::beside(&plain).unwrap());
        // Forced (dirty document): scratch, with relative files resolved
        // against the model's folder.
        let p = prepare(&plain, text, true).unwrap();
        assert!(p.is_scratch());
        assert!(p.note.as_deref().unwrap().contains("unsaved changes"));
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn stale_scratch_folders_are_removed_by_age() {
        let root = scratch_root();
        std::fs::create_dir_all(&root).unwrap();
        let old = root.join("test-stale-old");
        let new = root.join("test-stale-new");
        for d in [&old, &new] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join(STAMP), b"").unwrap();
        }
        let f = std::fs::File::options().write(true).open(old.join(STAMP)).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(10 * 86_400))
            .unwrap();
        drop(f);
        let removed = clean_stale_scratch(Duration::from_secs(7 * 86_400));
        assert!(removed >= 1, "{removed}");
        assert!(!old.exists(), "the 10-day-old folder goes");
        assert!(new.exists(), "the fresh one stays");
        let _ = std::fs::remove_dir_all(&new);
        assert!(is_ascii_path(&scratch_root()));
    }

    /// A cancelled run must come back promptly, flagged as cancelled, with
    /// whatever files exist. Without a real engine, a stand-in process that
    /// would run for a minute plays the part.
    #[test]
    fn a_cancelled_run_is_killed_and_reported_as_such() {
        let work = scratch().join("cancel-run");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).unwrap();
        let inp = work.join("model.inp");
        std::fs::write(&inp, "[OPTIONS]\n").unwrap();
        #[cfg(windows)]
        let fake = {
            let cmd = work.join("runswmm.cmd");
            std::fs::write(&cmd, "@echo running\r\n@ping -n 60 127.0.0.1 > nul\r\n").unwrap();
            cmd
        };
        #[cfg(not(windows))]
        let fake = {
            let sh = work.join("runswmm");
            std::fs::write(&sh, "#!/bin/sh\necho running\nsleep 60\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
            sh
        };
        let engine = Engine {
            id: "fake".into(),
            version: "unknown".into(),
            exe: fake,
            arch: Arch::NotPe,
            sha256: "0".repeat(64),
        };
        let paths = RunPaths::beside(&inp).unwrap();
        std::fs::write(&paths.rpt, "stale").unwrap();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let started = Instant::now();
        let worker = std::thread::spawn(move || engine.run_with_cancel(&paths, &flag));
        std::thread::sleep(Duration::from_millis(300));
        cancel.store(true, Ordering::SeqCst);
        let (run, cancelled) = worker.join().unwrap().unwrap();
        assert!(cancelled);
        assert!(started.elapsed() < Duration::from_secs(20), "killed, not waited out");
        assert!(!run.rpt.exists(), "the stale report was removed before launch");
        assert!(!run.succeeded());
        let _ = std::fs::remove_dir_all(&work);
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
