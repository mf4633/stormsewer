// SPDX-License-Identifier: GPL-3.0-or-later

//! Step-by-step control of an EPA SWMM engine through its DLL API.
//!
//! EPA's Windows build of `swmm5.dll` is 32-bit and StormSewer is 64-bit, so
//! the DLL cannot be loaded in-process. A small 32-bit helper executable
//! (`stormsewer-swmm-bridge32.exe`, the `bridge` crate in this workspace)
//! loads it and speaks a line protocol on stdin/stdout; this module is the
//! client. The engine itself is still EPA's, unmodified: the bridge calls
//! `swmm_open`, `swmm_start`, `swmm_step`, `swmm_getValue`, `swmm_setValue`,
//! `swmm_end` and `swmm_close` exactly as `runswmm` would, and the `.rpt`
//! and `.out` it leaves behind are byte-for-byte what a `runswmm` run of
//! the same model writes.
//!
//! # Protocol
//!
//! One request per line, one reply per line. Tokens are space-separated; a
//! token with a space, quote or backslash is double-quoted with `\"` and
//! `\\` escapes ([`quote`], [`tokens`]). The bridge prints
//! `ready bridge=<v> engine=<int> arch=<a>` once at start-up, then answers
//! each request with `ok [k=v ...]`, `done` (the run is over), or
//! `err <code> <message>` where `code` is the engine's error code (`-1` for
//! a protocol error). `names <kind>` answers `ok n=N` followed by N lines,
//! one quoted name each. During a long `until` the bridge prints a
//! heartbeat `.. t=<s>` every couple of seconds so a client's silence
//! timer does not fire. The full request list is in `docs/23-live-runs.md`.
//!
//! # Hangs
//!
//! Every reply is awaited with a [`READ_TIMEOUT`] guard. A bridge that goes
//! quiet for that long (an engine stuck in a step, a crashed helper) is
//! killed and the call fails, rather than freezing the app; dropping a
//! [`Session`] kills the helper too.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::{Error, Result};

/// The helper's file name beside the app.
#[cfg(windows)]
pub const BRIDGE_EXE: &str = "stormsewer-swmm-bridge32.exe";
#[cfg(not(windows))]
pub const BRIDGE_EXE: &str = "stormsewer-swmm-bridge32";

/// Environment variable naming the bridge executable explicitly.
pub const BRIDGE_ENV: &str = "STORMSEWER_SWMM_BRIDGE";

/// How long a single reply may take before the bridge is presumed wedged.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Where the bridge executable is: `STORMSEWER_SWMM_BRIDGE`, beside the
/// running executable, or (in a dev build only) the i686 target folder of
/// this workspace. `None` means tight coupling is unavailable.
pub fn find_bridge() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(BRIDGE_ENV) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join(BRIDGE_EXE);
            if beside.is_file() {
                return Some(beside);
            }
        }
    }
    #[cfg(debug_assertions)]
    {
        // Running from a dev tree: the bridge is wherever
        // `cargo build -p stormsewer-swmm-bridge --target i686-pc-windows-msvc`
        // (or scripts/build-bridge.ps1) left it.
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for profile in ["release", "debug"] {
            let p = workspace
                .join("target")
                .join("i686-pc-windows-msvc")
                .join(profile)
                .join(BRIDGE_EXE);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The engine DLL that belongs to an engine executable: `swmm5.dll` beside
/// `runswmm.exe`, or None.
pub fn find_dll(engine_exe: &Path) -> Option<PathBuf> {
    let dll = engine_exe.parent()?.join("swmm5.dll");
    dll.is_file().then_some(dll)
}

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

/// Split a protocol line into tokens, honouring double quotes.
pub fn tokens(line: &str) -> std::result::Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quoted = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '\\') => match chars.next() {
                Some(e @ ('"' | '\\')) => cur.push(e),
                Some(other) => {
                    cur.push('\\');
                    cur.push(other);
                }
                None => return Err("dangling backslash".into()),
            },
            (true, '"') => quoted = false,
            (true, c) => cur.push(c),
            (false, '"') => {
                quoted = true;
                in_token = true;
            }
            (false, c) if c.is_whitespace() => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            (false, c) => {
                cur.push(c);
                in_token = true;
            }
        }
    }
    if quoted {
        return Err("unterminated quote".into());
    }
    if in_token {
        out.push(cur);
    }
    Ok(out)
}

/// A token as it goes on the wire: quoted only when it has to be.
pub fn quote(s: &str) -> String {
    let plain = !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\\');
    if plain {
        return s.to_string();
    }
    let mut q = String::with_capacity(s.len() + 2);
    q.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q
}

/// One parsed reply line.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// `ok` with its `key=value` fields in order.
    Ok(Vec<(String, String)>),
    /// The run is over.
    Done,
    /// An engine (`code >= 0`) or protocol (`-1`) error.
    Err { code: i32, message: String },
}

impl Reply {
    /// Parse a reply line. Anything unrecognised is reported as a protocol
    /// error so the caller never mistakes noise for a value.
    pub fn parse(line: &str) -> Reply {
        let t = match tokens(line) {
            Ok(t) => t,
            Err(e) => {
                return Reply::Err { code: -1, message: format!("unparseable reply {line:?}: {e}") }
            }
        };
        match t.first().map(String::as_str) {
            // `ready` is the start-up banner, shaped like an `ok`.
            Some("ok" | "ready") => Reply::Ok(
                t[1..]
                    .iter()
                    .map(|kv| match kv.split_once('=') {
                        Some((k, v)) => (k.to_string(), v.to_string()),
                        None => (kv.clone(), String::new()),
                    })
                    .collect(),
            ),
            Some("done") => Reply::Done,
            Some("err") => Reply::Err {
                code: t.get(1).and_then(|c| c.parse().ok()).unwrap_or(-1),
                message: t.get(2..).map(|m| m.join(" ")).unwrap_or_default(),
            },
            _ => Reply::Err { code: -1, message: format!("unexpected reply {line:?}") },
        }
    }

    /// The value of field `key` in an `ok` reply.
    pub fn field(&self, key: &str) -> Option<&str> {
        match self {
            Reply::Ok(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    fn number(&self, key: &str) -> Result<f64> {
        let raw = self
            .field(key)
            .ok_or_else(|| Error::Engine(format!("bridge reply lacks {key}")))?;
        raw.parse()
            .map_err(|_| Error::Engine(format!("bridge reply {key}={raw:?} is not a number")))
    }

    /// Turn an error reply into an [`Error`]; pass an `ok`/`done` through.
    fn into_result(self) -> Result<Reply> {
        match self {
            Reply::Err { code, message } if code >= 0 => {
                Err(Error::Engine(format!("engine error {code}: {message}")))
            }
            Reply::Err { message, .. } => Err(Error::Engine(format!("engine bridge: {message}"))),
            ok => Ok(ok),
        }
    }
}

/// `52004` → `5.2.4`, the engine's `xyzzz` version packing.
pub fn version_string(v: i32) -> String {
    if v <= 0 {
        return "unknown".into();
    }
    format!("{}.{}.{}", v / 10_000, (v / 1_000) % 10, v % 1_000)
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// A live engine run.
#[derive(Debug)]
pub struct Session {
    /// Simulation length in seconds, from `swmm_open`.
    pub duration_s: f64,
    /// Routing step the engine will use, seconds.
    pub routing_step_s: f64,
    /// Node names in engine index order.
    pub node_names: Vec<String>,
    pub link_names: Vec<String>,
    pub bridge_version: String,
    pub engine_version: String,
    /// Reporting step, seconds.
    pub report_step_s: f64,
    /// Simulation start as SWMM stores it: days since 1899-12-30.
    pub start_days: f64,
    /// Elapsed simulation time after the last step, seconds.
    pub elapsed_s: f64,
    /// The run reached its end (or failed); only `finish`/`abort` remain.
    pub finished: bool,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<std::io::Result<String>>,
    _reader: std::thread::JoinHandle<()>,
}

impl Session {
    /// Start the bridge, load the DLL, open and start the model. The `.rpt`
    /// and `.out` are written by the engine as in a normal run.
    pub fn open(bridge: &Path, dll: &Path, inp: &Path, rpt: &Path, out: &Path) -> Result<Self> {
        if !inp.is_file() {
            return Err(Error::NotFound(format!("no input file at {}", inp.display())));
        }
        if !dll.is_file() {
            return Err(Error::NotFound(format!("no engine DLL at {}", dll.display())));
        }
        for path in [rpt, out] {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // As engine::run_with: a stale file must not pass for this run's.
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        let mut child = Command::new(bridge)
            .arg(dll)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Engine(format!("could not start {}: {e}", bridge.display())))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, lines) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let failed = line.is_err();
                if tx.send(line).is_err() || failed {
                    break;
                }
            }
        });
        let mut s = Self {
            duration_s: 0.0,
            routing_step_s: 0.0,
            node_names: Vec::new(),
            link_names: Vec::new(),
            bridge_version: String::new(),
            engine_version: String::new(),
            report_step_s: 0.0,
            start_days: 0.0,
            elapsed_s: 0.0,
            finished: false,
            child,
            stdin,
            lines,
            _reader: reader,
        };

        // The banner: `ready bridge=… engine=… arch=…`, or an `err` when the
        // DLL would not load (a 64-bit build of the helper, say).
        let banner = s.read_reply()?;
        let banner = match banner {
            Reply::Err { message, .. } => {
                return Err(Error::Engine(format!("engine bridge could not start: {message}")))
            }
            other => other,
        };
        s.bridge_version = banner.field("bridge").unwrap_or("unknown").to_string();
        let engine_int: i32 = banner.field("engine").and_then(|v| v.parse().ok()).unwrap_or(0);
        s.engine_version = version_string(engine_int);

        let line = format!(
            "open {} {} {}",
            quote(&inp.to_string_lossy()),
            quote(&rpt.to_string_lossy()),
            quote(&out.to_string_lossy())
        );
        let opened = s.request(&line)?;
        s.duration_s = opened.number("duration_s")?;
        s.routing_step_s = opened.number("routing_step_s")?;
        s.report_step_s = opened.number("report_step_s").unwrap_or(0.0);
        s.start_days = opened.number("start_days").unwrap_or(0.0);
        s.node_names = s.names("node")?;
        s.link_names = s.names("link")?;
        Ok(s)
    }

    /// The next line from the bridge, minus heartbeats, within the timeout.
    fn read_line(&mut self) -> Result<String> {
        let deadline = Instant::now() + READ_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(Ok(line)) => {
                    if line.starts_with("..") {
                        // Heartbeat: the engine is working; keep waiting.
                        continue;
                    }
                    return Ok(line);
                }
                Ok(Err(e)) => {
                    let _ = self.child.kill();
                    return Err(Error::Engine(format!("engine bridge pipe failed: {e}")));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let status = self.child.try_wait().ok().flatten();
                    return Err(Error::Engine(match status {
                        Some(st) => format!("engine bridge exited ({st}) before answering"),
                        None => "engine bridge closed its output".to_string(),
                    }));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.child.kill();
                    return Err(Error::Engine(format!(
                        "engine bridge did not answer within {} s; it was stopped",
                        READ_TIMEOUT.as_secs()
                    )));
                }
            }
        }
    }

    fn read_reply(&mut self) -> Result<Reply> {
        let line = self.read_line()?;
        Ok(Reply::parse(&line))
    }

    fn send(&mut self, line: &str) -> Result<()> {
        writeln!(self.stdin, "{line}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| Error::Engine(format!("engine bridge went away: {e}")))
    }

    /// One request, one reply; an error reply becomes an `Err`.
    pub fn request(&mut self, line: &str) -> Result<Reply> {
        self.send(line)?;
        self.read_reply()?.into_result()
    }

    /// `names <kind>`: the list the bridge prints after `ok n=N`.
    pub fn names(&mut self, kind: &str) -> Result<Vec<String>> {
        let head = self.request(&format!("names {kind}"))?;
        let n = head.number("n")? as usize;
        let mut names = Vec::with_capacity(n);
        for _ in 0..n {
            let line = self.read_line()?;
            let mut t = tokens(&line).map_err(Error::Engine)?;
            names.push(t.pop().unwrap_or_default());
        }
        Ok(names)
    }

    /// Interpret a stepping reply, tracking elapsed time and the end.
    fn stepped(&mut self, reply: Reply) -> Result<Option<f64>> {
        match reply {
            Reply::Done => {
                self.finished = true;
                Ok(None)
            }
            ok @ Reply::Ok(_) => {
                let t = ok.number("t")?;
                self.elapsed_s = t;
                Ok(Some(t))
            }
            Reply::Err { .. } => unreachable!("request() turns err replies into Err"),
        }
    }

    /// Advance one routing step. Returns the elapsed simulation time in
    /// seconds, or None when the run is finished.
    pub fn step(&mut self) -> Result<Option<f64>> {
        if self.finished {
            return Ok(None);
        }
        let r = self.request("step").inspect_err(|_| self.finished = true)?;
        self.stepped(r)
    }

    /// Advance until `time_s` (inclusive of the step that reaches it).
    pub fn step_until(&mut self, time_s: f64) -> Result<Option<f64>> {
        if self.finished {
            return Ok(None);
        }
        let r = self
            .request(&format!("until {time_s}"))
            .inspect_err(|_| self.finished = true)?;
        self.stepped(r)
    }

    /// `swmm_stride`: advance by a fixed number of seconds (the engine
    /// shortens its routing step to land on it).
    pub fn stride(&mut self, seconds: f64) -> Result<Option<f64>> {
        if self.finished {
            return Ok(None);
        }
        let r = self
            .request(&format!("stride {seconds}"))
            .inspect_err(|_| self.finished = true)?;
        self.stepped(r)
    }

    /// `get <kind> <name> <property>` as a number.
    pub fn get(&mut self, kind: &str, name: &str, property: &str) -> Result<f64> {
        self.request(&format!("get {kind} {} {property}", quote(name)))?
            .number("v")
    }

    /// `set <kind> <name> <property> <value>`.
    pub fn set(&mut self, kind: &str, name: &str, property: &str, value: f64) -> Result<()> {
        self.request(&format!("set {kind} {} {property} {value}", quote(name)))
            .map(|_| ())
    }

    /// Hydraulic head at a node, in the model's length units.
    pub fn node_head(&mut self, node: &str) -> Result<f64> {
        self.get("node", node, "head")
    }

    pub fn node_depth(&mut self, node: &str) -> Result<f64> {
        self.get("node", node, "depth")
    }

    /// Flooding (overflow) rate at a node now, in the model's flow units.
    pub fn node_overflow(&mut self, node: &str) -> Result<f64> {
        self.get("node", node, "overflow")
    }

    /// Total inflow to a node now.
    pub fn node_inflow(&mut self, node: &str) -> Result<f64> {
        self.get("node", node, "inflow")
    }

    /// Stored volume at a node now.
    pub fn node_volume(&mut self, node: &str) -> Result<f64> {
        self.get("node", node, "volume")
    }

    /// Set the lateral inflow the engine applies at a node from now on, in
    /// the model's flow units (negative = withdrawal, which the engine caps
    /// at the node's stored volume).
    pub fn set_node_lateral_inflow(&mut self, node: &str, flow: f64) -> Result<()> {
        self.set("node", node, "latflow", flow)
    }

    /// Set an outfall's stage (head) from now on.
    pub fn set_outfall_stage(&mut self, node: &str, head: f64) -> Result<()> {
        self.set("node", node, "head", head)
    }

    /// Flow in a link now.
    pub fn link_flow(&mut self, link: &str) -> Result<f64> {
        self.get("link", link, "flow")
    }

    pub fn link_depth(&mut self, link: &str) -> Result<f64> {
        self.get("link", link, "depth")
    }

    pub fn link_setting(&mut self, link: &str) -> Result<f64> {
        self.get("link", link, "setting")
    }

    /// Set a link's control setting (0..1) — pump on/off, orifice opening.
    pub fn set_link_setting(&mut self, link: &str, setting: f64) -> Result<()> {
        self.set("link", link, "setting", setting)
    }

    /// Override a rain gage's rainfall from now on (intensity in the gage's
    /// units).
    pub fn set_gage_rainfall(&mut self, gage: &str, rainfall: f64) -> Result<()> {
        self.set("gage", gage, "rainfall", rainfall)
    }

    /// Finish: `swmm_end`, `swmm_close`, and the continuity errors (runoff,
    /// flow, quality) in percent. `swmm_report` is skipped exactly as
    /// `runswmm` skips it when an `.out` path is given, so the `.rpt`
    /// matches a normal run's.
    pub fn finish(mut self) -> Result<(f64, f64, f64)> {
        let r = self.request("finish")?;
        let errors = (r.number("runoff")?, r.number("flow")?, r.number("quality")?);
        let _ = self.request("quit");
        let _ = self.child.wait();
        Ok(errors)
    }

    /// Abort the run and kill the bridge. The engine is asked to close its
    /// files first so a partial `.rpt` is left readable.
    pub fn abort(mut self) {
        if self.send("abort").is_ok() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A session that was finished or aborted has already exited; a
        // dropped live one must not outlive the app.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_round_trip_quoted_paths() {
        let path = r#"C:\Models\my pond\site "A".inp"#;
        let line = format!("open {} r.rpt o.out", quote(path));
        let t = tokens(&line).unwrap();
        assert_eq!(t, vec!["open", path, "r.rpt", "o.out"]);
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("two words"), "\"two words\"");
        assert!(tokens("\"open").is_err());
    }

    #[test]
    fn ok_replies_parse_into_fields() {
        let r = Reply::parse("ok duration_s=43200 routing_step_s=15 nodes=14 version=52004");
        assert_eq!(r.field("duration_s"), Some("43200"));
        assert_eq!(r.number("routing_step_s").unwrap(), 15.0);
        assert_eq!(r.field("nodes"), Some("14"));
        assert!(r.field("links").is_none());
        assert!(r.number("nodes").is_ok());
        assert_eq!(Reply::parse("done"), Reply::Done);
        assert_eq!(Reply::parse("ready bridge=0.9.8 engine=52004 arch=x86").field("engine"), Some("52004"));
        assert_eq!(Reply::parse("ok"), Reply::Ok(vec![]));
        assert_eq!(Reply::parse("ok t=15"), Reply::Ok(vec![("t".into(), "15".into())]));
    }

    #[test]
    fn err_replies_carry_code_and_message() {
        let r = Reply::parse("err 303 \"ERROR 303: cannot open input file.\"");
        assert_eq!(r, Reply::Err { code: 303, message: "ERROR 303: cannot open input file.".into() });
        let e = r.into_result().unwrap_err().to_string();
        assert!(e.contains("engine error 303"), "{e}");
        let p = Reply::parse("err -1 unknown command frobnicate").into_result().unwrap_err().to_string();
        assert!(p.contains("engine bridge: unknown command"), "{p}");
        let noise = Reply::parse("garbage line");
        assert!(matches!(noise, Reply::Err { code: -1, .. }));
        assert_eq!(version_string(52_004), "5.2.4");
        assert_eq!(version_string(51_015), "5.1.15");
    }

    #[test]
    fn find_dll_wants_swmm5_beside_runswmm() {
        let dir = std::env::temp_dir().join("stormsewer-bridge-dll-test");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(crate::engine::RUNNER_EXE);
        std::fs::write(&exe, b"x").unwrap();
        let _ = std::fs::remove_file(dir.join("swmm5.dll"));
        assert_eq!(find_dll(&exe), None);
        std::fs::write(dir.join("swmm5.dll"), b"x").unwrap();
        assert_eq!(find_dll(&exe), Some(dir.join("swmm5.dll")));
    }

    /// End to end against EPA's DLL. Needs the bridge built for i686 (see
    /// scripts/build-bridge.ps1) and an EPA SWMM install; skips with a note
    /// otherwise. The pond model is run to the end through `step_until`,
    /// then the continuity errors the engine reports through
    /// `swmm_getMassBalErr` are checked against a `runswmm` run of the same
    /// model, and the `.out` files must be identical.
    #[test]
    fn pond_model_runs_to_the_end_through_the_bridge() {
        let Some(bridge) = find_bridge() else {
            eprintln!("skipped: bridge executable not built (cargo build -p stormsewer-swmm-bridge --release --target i686-pc-windows-msvc)");
            return;
        };
        let registry = crate::engine::Registry::discover();
        let Some(engine) = registry
            .engines()
            .iter()
            .find(|e| find_dll(&e.exe).is_some())
            .cloned()
        else {
            eprintln!("skipped: no EPA SWMM engine with swmm5.dll beside it");
            return;
        };
        let dll = find_dll(&engine.exe).unwrap();

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/epa-samples/Detention_Pond_Model.inp");
        let work = std::env::temp_dir().join("stormsewer-swmm-tests").join("bridge-pond");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).unwrap();
        let inp = work.join("pond.inp");
        std::fs::copy(&fixture, &inp).unwrap();
        let rpt = work.join("pond.rpt");
        let out = work.join("pond.out");

        let mut s = Session::open(&bridge, &dll, &inp, &rpt, &out).unwrap();
        assert_eq!(s.duration_s, 43_200.0, "12 h model");
        assert_eq!(s.routing_step_s, 15.0);
        assert_eq!(s.report_step_s, 300.0);
        assert_eq!(s.engine_version, engine.version);
        assert!(s.node_names.iter().any(|n| n == "J1"), "{:?}", s.node_names);
        assert!(s.link_names.iter().any(|n| n == "C1"), "{:?}", s.link_names);

        let t = s.step().unwrap().unwrap();
        assert_eq!(t, 15.0);
        let head = s.node_head("J1").unwrap();
        assert!(head.is_finite() && head > 4000.0, "J1 head {head}");
        assert!(s.node_head("NoSuchNode").is_err());

        // Half way, and the pond has filled somewhat.
        let t = s.step_until(21_600.0).unwrap().unwrap();
        assert_eq!(t, 21_600.0);
        assert!(s.node_depth("SU1").unwrap() > 0.0);
        assert!(s.link_flow("C1").unwrap().is_finite());
        // The control API answers too.
        s.set_node_lateral_inflow("J1", 0.0).unwrap();

        assert_eq!(s.step_until(1e9).unwrap(), None);
        assert!(s.finished);
        assert_eq!(s.step().unwrap(), None);
        let (runoff, flow, quality) = s.finish().unwrap();
        assert!(runoff.is_finite() && flow.is_finite() && quality.is_finite());

        // The same model through runswmm.
        let cmp = work.join("runswmm");
        std::fs::create_dir_all(&cmp).unwrap();
        let ref_inp = cmp.join("pond.inp");
        std::fs::copy(&fixture, &ref_inp).unwrap();
        let reference = engine.run(&ref_inp).unwrap();
        assert!(reference.succeeded(), "{:?}", reference.failure_reason());
        let expect = |section: &str| -> f64 {
            reference
                .report
                .continuity
                .iter()
                .find(|(s, _)| s.contains(section))
                .map(|(_, v)| *v)
                .unwrap_or_else(|| panic!("{section} continuity in {:?}", reference.report.continuity))
        };
        // The .rpt prints three decimals, so 0.01 percentage points is the
        // agreement the text can show.
        let runoff_ref = expect("Runoff");
        let flow_ref = expect("Flow");
        eprintln!("continuity bridge runoff={runoff} flow={flow} quality={quality}; runswmm runoff={runoff_ref} flow={flow_ref}");
        assert!((runoff - runoff_ref).abs() <= 0.01, "runoff {runoff} vs {runoff_ref}");
        assert!((flow - flow_ref).abs() <= 0.01, "flow {flow} vs {flow_ref}");

        // And the bridge's own report and results are the engine's usual ones.
        let ours = crate::rpt::read(&rpt).unwrap();
        assert_eq!(ours.continuity, reference.report.continuity);
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&reference.out).unwrap(), "identical .out");
        let results = crate::out::OutputFile::open(&out).unwrap();
        assert_eq!(results.meta.n_periods, 144);
    }
}
