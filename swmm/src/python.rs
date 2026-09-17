// SPDX-License-Identifier: GPL-3.0-or-later

//! A persistent Python session, driven through the kernel in `pykernel.py`.
//!
//! The kernel is compiled into the binary and written to a temp file on first
//! use, so the app stays self-contained — there is no separate script to
//! install or keep in step.
//!
//! Framing is explicit rather than scraped. The alternative, driving
//! `python -i` and watching for `>>> `, means guessing which stream CPython
//! prints prompts to when stdin is not a terminal and whether a continuation
//! prompt is pending. Here the kernel ends every reply with a sentinel line,
//! so "the block finished" is something it *tells* us.
//!
//! The session is useful chiefly because the interpreter the user already has
//! tends to carry the whole post-processing stack — numpy, pandas, swmmio,
//! pyswmm — and ALR itself is Python, so this is the natural place to drive it.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::sha256::sha256_hex;
use crate::{Error, Result};

/// The kernel itself. Compiled in; see the module note.
const KERNEL_SOURCE: &str = include_str!("pykernel.py");

/// Marks the end of a block the caller sends.
pub const END_OF_BLOCK: &str = "<<<STORMSEWER-EXEC>>>";
/// Marks the end of a reply, followed by ` ok` or ` error`.
pub const DONE: &str = "<<<STORMSEWER-DONE>>>";

/// One thing the kernel said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Line(String),
    /// The block finished. `ok` is false when it raised.
    Done { ok: bool },
}

/// What the session should have bound when it starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionEnv {
    pub model: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub rpt: Option<PathBuf>,
    /// Added to `sys.path`, so `import quantum_hydraulics` works.
    pub alr_dir: Option<PathBuf>,
}

/// Where the kernel lives on disk, keyed by its own content.
///
/// Content-addressed so an upgraded app never reuses a stale kernel that a
/// temp directory happened to keep.
fn kernel_path() -> Result<PathBuf> {
    let digest = sha256_hex(KERNEL_SOURCE.as_bytes());
    let path = std::env::temp_dir().join(format!("stormsewer-pykernel-{}.py", &digest[..16]));
    if !path.exists() {
        std::fs::write(&path, KERNEL_SOURCE)?;
    }
    Ok(path)
}

/// The interpreter to use when the user has not named one. Windows puts
/// `python` first, everywhere else `python3` is the safer spelling.
pub fn default_python() -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(windows) {
        &["python", "python3", "py"]
    } else {
        &["python3", "python"]
    };
    for name in candidates {
        let ok = Command::new(name)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(PathBuf::from(name));
        }
    }
    None
}

/// A running Python kernel.
pub struct PythonSession {
    child: Child,
    /// Taken on shutdown: closing the pipe is how the kernel is asked to stop.
    stdin: Option<ChildStdin>,
    rx: Receiver<Output>,
    busy: bool,
}

impl PythonSession {
    /// Launch the kernel. It emits a banner and one `Done` immediately, which
    /// the caller reads like any other reply — that is the ready signal.
    pub fn start(python: &Path, env: &SessionEnv) -> Result<Self> {
        let kernel = kernel_path()?;
        let mut command = Command::new(python);
        command
            .arg("-u") // unbuffered: replies must not wait for a full pipe buffer
            .arg(&kernel)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Belt and braces with the kernel's own reconfigure: the machine's
            // code page must not decide how a model path is encoded.
            .env("PYTHONIOENCODING", "utf-8");

        for (var, value) in [
            ("STORMSEWER_PY_MODEL", env.model.as_ref()),
            ("STORMSEWER_PY_OUT", env.out.as_ref()),
            ("STORMSEWER_PY_RPT", env.rpt.as_ref()),
            ("STORMSEWER_ALR_DIR", env.alr_dir.as_ref()),
        ] {
            if let Some(path) = value {
                command.env(var, path);
            }
        }

        let mut child = command.spawn().map_err(|e| {
            Error::Python(format!("could not start {}: {e}", python.display()))
        })?;

        let stdin = child.stdin.take();
        let (tx, rx) = mpsc::channel();
        // Both pipes feed one channel so ordering stays roughly as emitted and
        // the caller has a single thing to poll. stderr matters even though the
        // kernel folds Python-level stderr into stdout: an interpreter that
        // fails before the kernel runs at all reports there.
        if let Some(stdout) = child.stdout.take() {
            spawn_reader(stdout, tx.clone(), true);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_reader(stderr, tx, false);
        }

        Ok(Self { child, stdin, rx, busy: true })
    }

    /// Send a block for execution.
    pub fn send(&mut self, source: &str) -> Result<()> {
        if source.contains(END_OF_BLOCK) {
            return Err(Error::Python(
                "that code contains the kernel's end-of-block marker".to_string(),
            ));
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| Error::Python("the Python session has been shut down".to_string()))?;
        writeln!(stdin, "{source}\n{END_OF_BLOCK}")
            .and_then(|()| stdin.flush())
            .map_err(|e| Error::Python(format!("could not reach the Python kernel: {e}")))?;
        self.busy = true;
        Ok(())
    }

    /// Whether a block is still running (or the kernel is still starting up).
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Take whatever has arrived, without blocking.
    pub fn poll(&mut self) -> Vec<Output> {
        let mut collected = Vec::new();
        while let Ok(item) = self.rx.try_recv() {
            if matches!(item, Output::Done { .. }) {
                self.busy = false;
            }
            collected.push(item);
        }
        collected
    }

    /// Block until the current block finishes, returning its lines and whether
    /// it succeeded. For tests and non-interactive callers; the UI polls.
    pub fn wait_for_done(&mut self, timeout: Duration) -> Result<(Vec<String>, bool)> {
        let deadline = Instant::now() + timeout;
        let mut lines = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Python("the Python kernel did not reply in time".into()));
            }
            match self.rx.recv_timeout(remaining) {
                Ok(Output::Line(line)) => lines.push(line),
                Ok(Output::Done { ok }) => {
                    self.busy = false;
                    return Ok((lines, ok));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(Error::Python("the Python kernel did not reply in time".into()))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Error::Python(format!(
                        "the Python kernel stopped{}",
                        if lines.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", lines.join(" "))
                        }
                    )))
                }
            }
        }
    }

    /// Close stdin and let the kernel exit; kill it if it will not.
    pub fn shutdown(&mut self) {
        self.stdin = None; // dropping the pipe ends the kernel's read loop
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for PythonSession {
    fn drop(&mut self) {
        // Without this, closing the window would leave a python process behind.
        self.shutdown();
    }
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    stream: R,
    tx: Sender<Output>,
    watch_for_done: bool,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let message = match (watch_for_done, line.strip_prefix(DONE)) {
                (true, Some(status)) => Output::Done { ok: status.trim() == "ok" },
                _ => Output::Line(line),
            };
            if tx.send(message).is_err() {
                break; // the session was dropped
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(30);

    /// The Rust constants and the script are two files that must agree. If one
    /// is edited alone, every session hangs waiting for a marker that never
    /// comes — so assert they match rather than find out at runtime.
    #[test]
    fn sentinels_match_the_kernel_script() {
        assert!(
            KERNEL_SOURCE.contains(&format!("END_OF_BLOCK = \"{END_OF_BLOCK}\"")),
            "pykernel.py does not define END_OF_BLOCK as {END_OF_BLOCK}"
        );
        assert!(
            KERNEL_SOURCE.contains(&format!("DONE = \"{DONE}\"")),
            "pykernel.py does not define DONE as {DONE}"
        );
    }

    #[test]
    fn kernel_is_written_once_and_reused() {
        let first = kernel_path().unwrap();
        assert!(first.is_file());
        let second = kernel_path().unwrap();
        assert_eq!(first, second, "the kernel path must be stable");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), KERNEL_SOURCE);
    }

    fn session() -> Option<PythonSession> {
        let python = default_python()?;
        let mut s = PythonSession::start(&python, &SessionEnv::default()).ok()?;
        // Startup banner, ending in the ready signal.
        let (_banner, ok) = s.wait_for_done(TIMEOUT).ok()?;
        assert!(ok, "the kernel did not start cleanly");
        Some(s)
    }

    #[test]
    fn a_session_evaluates_holds_state_and_survives_errors() {
        let Some(mut s) = session() else {
            eprintln!("skipped: no Python interpreter found");
            return;
        };

        s.send("2 + 2").unwrap();
        let (lines, ok) = s.wait_for_done(TIMEOUT).unwrap();
        assert!(ok);
        assert_eq!(lines, vec!["4".to_string()], "a trailing expression is echoed");

        // State persists across blocks, and only the last expression echoes.
        s.send("x = 21\ny = x * 2\ny").unwrap();
        let (lines, ok) = s.wait_for_done(TIMEOUT).unwrap();
        assert!(ok);
        assert_eq!(lines, vec!["42".to_string()]);

        // An error is reported and the session stays usable.
        s.send("1 / 0").unwrap();
        let (lines, ok) = s.wait_for_done(TIMEOUT).unwrap();
        assert!(!ok, "a raising block must report failure");
        assert!(
            lines.iter().any(|l| l.contains("ZeroDivisionError")),
            "expected a traceback, got {lines:?}"
        );

        s.send("y + 1").unwrap();
        let (lines, ok) = s.wait_for_done(TIMEOUT).unwrap();
        assert!(ok, "the kernel must survive an error");
        assert_eq!(lines, vec!["43".to_string()]);
    }

    /// Regression: a byte-order mark at the head of a block used to reach the
    /// parser and fail with "invalid non-printable character U+FEFF". Pasted
    /// code carries one often enough that the kernel must absorb it.
    #[test]
    fn a_byte_order_mark_does_not_break_a_block() {
        let Some(mut s) = session() else {
            eprintln!("skipped: no Python interpreter found");
            return;
        };
        s.send("\u{feff}6 * 7").unwrap();
        let (lines, ok) = s.wait_for_done(TIMEOUT).unwrap();
        assert!(ok, "a leading BOM must not fail the block, got {lines:?}");
        assert_eq!(lines, vec!["42".to_string()]);
    }

    /// Paths reach the session as environment, never as generated source, so a
    /// path with a backslash or a quote in it cannot break the block.
    #[test]
    fn the_environment_is_bound_in_the_session() {
        let Some(python) = default_python() else {
            eprintln!("skipped: no Python interpreter found");
            return;
        };
        let env = SessionEnv {
            model: Some(PathBuf::from(r#"C:\models\a model's "run".inp"#)),
            ..Default::default()
        };
        let mut s = PythonSession::start(&python, &env).unwrap();
        let (_banner, ok) = s.wait_for_done(TIMEOUT).unwrap();
        assert!(ok);

        // print(), not the bare name: repr() escapes the backslashes and the
        // apostrophe, so asserting against it would test Python's quoting
        // rules rather than the round trip this is here to check.
        s.send("print(model)").unwrap();
        let (lines, ok) = s.wait_for_done(TIMEOUT).unwrap();
        assert!(ok);
        assert_eq!(
            lines,
            vec![r#"C:\models\a model's "run".inp"#.to_string()],
            "the path must survive the environment round trip exactly"
        );
    }

    #[test]
    fn source_carrying_the_marker_is_refused() {
        let Some(mut s) = session() else {
            eprintln!("skipped: no Python interpreter found");
            return;
        };
        let err = s.send(&format!("x = 1\n{END_OF_BLOCK}")).unwrap_err();
        assert!(err.to_string().contains("end-of-block marker"), "{err}");
    }
}
