// SPDX-License-Identifier: GPL-3.0-or-later

//! `stormsewer-swmm-bridge32`: drives EPA's `swmm5.dll` one routing step at
//! a time on behalf of StormSewer.
//!
//! EPA ships SWMM for Windows as a 32-bit DLL; StormSewer is a 64-bit
//! process and cannot load it. This helper is built for
//! `i686-pc-windows-msvc`, loads the DLL whose path it is given, and speaks
//! a one-line-per-request protocol on stdin/stdout (see [`proto`] and
//! `docs/23-live-runs.md`). The engine is EPA's, unmodified: the bridge
//! calls `swmm_open`, `swmm_start`, `swmm_step`, `swmm_getValue`,
//! `swmm_setValue`, `swmm_end` and `swmm_close` exactly as `runswmm`
//! would, and the `.rpt` and `.out` the engine writes are the
//! same files a normal run produces.
//!
//! The protocol layer is generic over an [`Engine`] so it is unit-tested
//! with a fake on every platform; only the DLL binding is Windows-specific.
//!
//! Usage: `stormsewer-swmm-bridge32 <path to swmm5.dll>`

mod proto;

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

pub const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");

// Object and property codes from EPA's swmm5.h (5.2).
const OBJ_GAGE: i32 = 0;
const OBJ_SUBCATCH: i32 = 1;
const OBJ_NODE: i32 = 2;
const OBJ_LINK: i32 = 3;

const SYS_STARTDATE: i32 = 0;
const SYS_CURRENTDATE: i32 = 1;
const SYS_ELAPSEDTIME: i32 = 2;
const SYS_ROUTESTEP: i32 = 3;
const SYS_MAXROUTESTEP: i32 = 4;
const SYS_REPORTSTEP: i32 = 5;
const SYS_TOTALSTEPS: i32 = 6;
const SYS_FLOWUNITS: i32 = 8;

const GAGE_RAINFALL: i32 = 100;

const SUBCATCH_AREA: i32 = 200;
const SUBCATCH_RAINFALL: i32 = 202;
const SUBCATCH_EVAP: i32 = 203;
const SUBCATCH_INFIL: i32 = 204;
const SUBCATCH_RUNOFF: i32 = 205;

const NODE_TYPE: i32 = 300;
const NODE_ELEV: i32 = 301;
const NODE_MAXDEPTH: i32 = 302;
const NODE_DEPTH: i32 = 303;
const NODE_HEAD: i32 = 304;
const NODE_VOLUME: i32 = 305;
const NODE_LATFLOW: i32 = 306;
const NODE_INFLOW: i32 = 307;
const NODE_OVERFLOW: i32 = 308;

const LINK_TYPE: i32 = 400;
const LINK_LENGTH: i32 = 403;
const LINK_SLOPE: i32 = 404;
const LINK_FULLDEPTH: i32 = 405;
const LINK_FULLFLOW: i32 = 406;
const LINK_SETTING: i32 = 407;
const LINK_TIMEOPEN: i32 = 408;
const LINK_TIMECLOSED: i32 = 409;
const LINK_FLOW: i32 = 410;
const LINK_DEPTH: i32 = 411;
const LINK_VELOCITY: i32 = 412;
const LINK_TOPWIDTH: i32 = 413;

const SECONDS_PER_DAY: f64 = 86_400.0;

/// An engine error: the code `swmm_getError` reports and its message.
type EngineError = (i32, String);

/// What the protocol needs from an engine. The real one is EPA's DLL; the
/// tests use a fake.
pub trait Engine {
    fn version(&self) -> i32;
    fn open(&mut self, inp: &str, rpt: &str, out: &str) -> Result<(), EngineError>;
    fn start(&mut self) -> Result<(), EngineError>;
    /// One routing step; elapsed simulation time in decimal days, or 0.0
    /// once the run is over (the DLL's own convention).
    fn step(&mut self) -> Result<f64, EngineError>;
    /// `swmm_stride`: advance a fixed number of seconds.
    fn stride(&mut self, seconds: i32) -> Result<f64, EngineError>;
    fn end(&mut self) -> Result<(), EngineError>;
    fn report(&mut self) -> Result<(), EngineError>;
    fn close(&mut self) -> Result<(), EngineError>;
    /// Runoff, flow-routing and quality continuity errors, percent. Only
    /// meaningful between `end` and `close`.
    fn mass_balance(&mut self) -> (f32, f32, f32);
    fn warnings(&self) -> i32;
    fn count(&self, obj: i32) -> i32;
    fn name(&self, obj: i32, index: i32) -> String;
    fn get(&self, property: i32, index: i32) -> f64;
    fn set(&mut self, property: i32, index: i32, value: f64);
}

/// Where the bridge is in a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    /// `swmm_open` + `swmm_start` done; stepping.
    Running,
    /// The engine said the run is over (or failed); `end` is next.
    Stepped,
    /// `swmm_end` done; `report`/`close` remain.
    Ended,
}

/// The protocol state machine over an engine.
pub struct Bridge<E: Engine> {
    engine: E,
    phase: Phase,
    duration_s: f64,
    /// `name → index` for gages, subcatchments, nodes, links.
    index: [HashMap<String, i32>; 4],
    /// The client asked the bridge to exit after this reply.
    pub exit: bool,
    /// Wall-clock spacing of `..` heartbeats during `until`.
    heartbeat: Duration,
}

impl<E: Engine> Bridge<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            phase: Phase::Idle,
            duration_s: 0.0,
            index: Default::default(),
            exit: false,
            heartbeat: Duration::from_secs(2),
        }
    }

    /// The first line the bridge prints, before any request.
    pub fn banner(&self) -> String {
        format!(
            "ready bridge={} engine={} arch={}",
            BRIDGE_VERSION,
            self.engine.version(),
            std::env::consts::ARCH
        )
    }

    /// Answer one request line. Every reply line goes to `out`, flushed, so
    /// a client waiting on a pipe sees it at once.
    pub fn handle(&mut self, line: &str, out: &mut impl Write) -> std::io::Result<()> {
        let reply = match proto::tokens(line) {
            Ok(t) if t.is_empty() => return Ok(()),
            Ok(t) => self.dispatch(&t, out)?,
            Err(e) => format!("err -1 bad request: {e}"),
        };
        // `names` prints its own multi-line reply and hands back nothing.
        if reply.is_empty() {
            return Ok(());
        }
        writeln!(out, "{reply}")?;
        out.flush()
    }

    fn dispatch(&mut self, t: &[String], out: &mut impl Write) -> std::io::Result<String> {
        let args: Vec<&str> = t.iter().map(String::as_str).collect();
        Ok(match args[0] {
            "info" => format!(
                "ok bridge={} engine={} engine_version={} arch={} phase={:?} duration_s={}",
                BRIDGE_VERSION,
                self.engine.version(),
                proto::version_string(self.engine.version()),
                std::env::consts::ARCH,
                self.phase,
                self.duration_s
            ),
            "open" => self.open(&args[1..]),
            "names" => return self.names(&args[1..], out),
            "step" => self.step(),
            "stride" => self.stride(&args[1..]),
            "until" => return self.until(&args[1..], out),
            "get" => self.get(&args[1..]),
            "set" => self.set(&args[1..]),
            "end" => self.end(),
            "report" => self.report(),
            "close" => self.close(),
            "finish" => self.finish(),
            "abort" | "quit" => {
                self.shutdown();
                self.exit = true;
                "ok".to_string()
            }
            other => format!("err -1 unknown command {}", proto::quote(other)),
        })
    }

    fn err(e: EngineError) -> String {
        format!("err {} {}", e.0, proto::quote(&e.1))
    }

    fn open(&mut self, a: &[&str]) -> String {
        if a.len() != 3 {
            return "err -1 usage: open <inp> <rpt> <out>".into();
        }
        if self.phase != Phase::Idle {
            return "err -1 a model is already open".into();
        }
        let (inp, rpt, out) = (a[0], a[1], a[2]);
        let text = match std::fs::read(inp) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => return format!("err -1 {}", proto::quote(&format!("cannot read {inp}: {e}"))),
        };
        self.duration_s = proto::duration_from_inp(&text).unwrap_or(0.0);
        if let Err(e) = self.engine.open(inp, rpt, out) {
            // The engine keeps its error state until closed; release it so
            // the next `open` starts clean.
            let _ = self.engine.close();
            return Self::err(e);
        }
        if let Err(e) = self.engine.start() {
            let _ = self.engine.close();
            return Self::err(e);
        }
        self.phase = Phase::Running;
        for (slot, obj) in [OBJ_GAGE, OBJ_SUBCATCH, OBJ_NODE, OBJ_LINK].into_iter().enumerate() {
            let n = self.engine.count(obj).max(0);
            self.index[slot] = (0..n).map(|i| (self.engine.name(obj, i), i)).collect();
        }
        format!(
            "ok duration_s={} routing_step_s={} report_step_s={} nodes={} links={} subcatch={} gages={} version={} flow_units={} start_days={}",
            self.duration_s,
            self.engine.get(SYS_ROUTESTEP, 0),
            self.engine.get(SYS_REPORTSTEP, 0),
            self.index[OBJ_NODE as usize].len(),
            self.index[OBJ_LINK as usize].len(),
            self.index[OBJ_SUBCATCH as usize].len(),
            self.index[OBJ_GAGE as usize].len(),
            self.engine.version(),
            self.engine.get(SYS_FLOWUNITS, 0) as i32,
            self.engine.get(SYS_STARTDATE, 0),
        )
    }

    fn require_open(&self) -> Result<(), String> {
        if self.phase == Phase::Idle {
            Err("err -1 no model is open".into())
        } else {
            Ok(())
        }
    }

    fn names(&mut self, a: &[&str], out: &mut impl Write) -> std::io::Result<String> {
        if let Err(e) = self.require_open() {
            return Ok(e);
        }
        let Some(obj) = a.first().and_then(|k| object_kind(k)) else {
            return Ok("err -1 usage: names node|link|subcatch|gage".into());
        };
        let n = self.engine.count(obj).max(0);
        writeln!(out, "ok n={n}")?;
        for i in 0..n {
            writeln!(out, "{}", proto::quote(&self.engine.name(obj, i)))?;
        }
        out.flush()?;
        // The list is the reply; nothing more to print.
        Ok(String::new())
    }

    /// Advance one step and translate the engine's answer.
    fn advance(&mut self, r: Result<f64, EngineError>) -> String {
        match r {
            Ok(days) if days > 0.0 => format!("ok t={}", days * SECONDS_PER_DAY),
            Ok(_) => {
                self.phase = Phase::Stepped;
                "done".into()
            }
            Err(e) => {
                self.phase = Phase::Stepped;
                Self::err(e)
            }
        }
    }

    fn step(&mut self) -> String {
        match self.phase {
            Phase::Idle => "err -1 no model is open".into(),
            Phase::Running => {
                let r = self.engine.step();
                self.advance(r)
            }
            _ => "done".into(),
        }
    }

    fn stride(&mut self, a: &[&str]) -> String {
        let Some(secs) = a.first().and_then(|s| s.parse::<f64>().ok()) else {
            return "err -1 usage: stride <seconds>".into();
        };
        match self.phase {
            Phase::Idle => "err -1 no model is open".into(),
            Phase::Running => {
                let r = self.engine.stride(secs.round().max(1.0) as i32);
                self.advance(r)
            }
            _ => "done".into(),
        }
    }

    fn until(&mut self, a: &[&str], out: &mut impl Write) -> std::io::Result<String> {
        let Some(target) = a.first().and_then(|s| s.parse::<f64>().ok()) else {
            return Ok("err -1 usage: until <seconds>".into());
        };
        let mut last_beat = Instant::now();
        loop {
            let reply = self.step();
            let Some(t) = reply.strip_prefix("ok t=").and_then(|v| v.parse::<f64>().ok()) else {
                return Ok(reply);
            };
            if t + 1e-6 >= target {
                return Ok(reply);
            }
            if last_beat.elapsed() >= self.heartbeat {
                writeln!(out, ".. t={t}")?;
                out.flush()?;
                last_beat = Instant::now();
            }
        }
    }

    fn lookup(&self, obj: i32, name: &str) -> Result<i32, String> {
        self.index[obj as usize].get(name).copied().ok_or_else(|| {
            format!(
                "err -1 {}",
                proto::quote(&format!("unknown {} {name}", object_label(obj)))
            )
        })
    }

    fn get(&mut self, a: &[&str]) -> String {
        if let Err(e) = self.require_open() {
            return e;
        }
        if a.len() == 2 && a[0] == "system" {
            let v = match a[1] {
                "elapsed" => self.engine.get(SYS_ELAPSEDTIME, 0) * SECONDS_PER_DAY,
                "start_date" => self.engine.get(SYS_STARTDATE, 0),
                "current_date" => self.engine.get(SYS_CURRENTDATE, 0),
                "route_step" => self.engine.get(SYS_ROUTESTEP, 0),
                "max_route_step" => self.engine.get(SYS_MAXROUTESTEP, 0),
                "report_step" => self.engine.get(SYS_REPORTSTEP, 0),
                "total_steps" => self.engine.get(SYS_TOTALSTEPS, 0),
                "flow_units" => self.engine.get(SYS_FLOWUNITS, 0),
                "duration" => self.duration_s,
                "warnings" => f64::from(self.engine.warnings()),
                other => return format!("err -1 unknown system property {}", proto::quote(other)),
            };
            return format!("ok v={v}");
        }
        if a.len() != 3 {
            return "err -1 usage: get node|link|subcatch|gage|system <name> <property>".into();
        }
        let Some(obj) = object_kind(a[0]) else {
            return format!("err -1 unknown object kind {}", proto::quote(a[0]));
        };
        let Some(prop) = get_property(obj, a[2]) else {
            return format!("err -1 unknown {} property {}", object_label(obj), proto::quote(a[2]));
        };
        match self.lookup(obj, a[1]) {
            Ok(i) => format!("ok v={}", self.engine.get(prop, i)),
            Err(e) => e,
        }
    }

    fn set(&mut self, a: &[&str]) -> String {
        if let Err(e) = self.require_open() {
            return e;
        }
        if a.len() == 3 && a[0] == "system" {
            let Ok(v) = a[2].parse::<f64>() else {
                return format!("err -1 not a number: {}", proto::quote(a[2]));
            };
            return match a[1] {
                "route_step" => {
                    self.engine.set(SYS_ROUTESTEP, 0, v);
                    "ok".into()
                }
                other => format!("err -1 system property {} is not settable", proto::quote(other)),
            };
        }
        if a.len() != 4 {
            return "err -1 usage: set node|link|gage|system <name> <property> <value>".into();
        }
        let Some(obj) = object_kind(a[0]) else {
            return format!("err -1 unknown object kind {}", proto::quote(a[0]));
        };
        let Some(prop) = set_property(obj, a[2]) else {
            return format!("err -1 {} property {} is not settable", object_label(obj), proto::quote(a[2]));
        };
        let Ok(v) = a[3].parse::<f64>() else {
            return format!("err -1 not a number: {}", proto::quote(a[3]));
        };
        match self.lookup(obj, a[1]) {
            Ok(i) => {
                self.engine.set(prop, i, v);
                "ok".into()
            }
            Err(e) => e,
        }
    }

    fn end(&mut self) -> String {
        match self.phase {
            Phase::Idle => "err -1 no model is open".into(),
            Phase::Ended => "err -1 already ended".into(),
            _ => {
                let r = self.engine.end();
                self.phase = Phase::Ended;
                let (runoff, flow, quality) = self.engine.mass_balance();
                match r {
                    Ok(()) => format!(
                        "ok runoff={runoff} flow={flow} quality={quality} warnings={}",
                        self.engine.warnings()
                    ),
                    Err(e) => Self::err(e),
                }
            }
        }
    }

    fn report(&mut self) -> String {
        match self.phase {
            Phase::Ended => match self.engine.report() {
                Ok(()) => "ok".into(),
                Err(e) => Self::err(e),
            },
            Phase::Idle => "err -1 no model is open".into(),
            _ => "err -1 end the run first".into(),
        }
    }

    fn close(&mut self) -> String {
        if self.phase == Phase::Idle {
            return "err -1 no model is open".into();
        }
        if matches!(self.phase, Phase::Running | Phase::Stepped) {
            let _ = self.engine.end();
        }
        let r = self.engine.close();
        self.phase = Phase::Idle;
        self.index = Default::default();
        match r {
            Ok(()) => "ok".into(),
            Err(e) => Self::err(e),
        }
    }

    /// `end` + `close` in one round trip; the reply carries the continuity
    /// errors. An engine error still closes the model.
    ///
    /// `swmm_report` is deliberately not called: `runswmm` only calls it
    /// when no `.out` path was given (`Fout.mode == SCRATCH_FILE`), because
    /// the time-series tables it appends to the `.rpt` duplicate the binary
    /// file. Skipping it is what makes a bridge run's `.rpt` identical to a
    /// `runswmm` run's; `report` is there for a client that wants them.
    fn finish(&mut self) -> String {
        if self.phase == Phase::Idle {
            return "err -1 no model is open".into();
        }
        let ended = if self.phase == Phase::Ended {
            Ok(())
        } else {
            self.engine.end()
        };
        self.phase = Phase::Ended;
        let (runoff, flow, quality) = self.engine.mass_balance();
        let warnings = self.engine.warnings();
        let closed = self.engine.close();
        self.phase = Phase::Idle;
        self.index = Default::default();
        if let Err(e) = ended.and(closed) {
            return Self::err(e);
        }
        format!("ok runoff={runoff} flow={flow} quality={quality} warnings={warnings}")
    }

    /// Leave the engine closed, whatever state it was in.
    pub fn shutdown(&mut self) {
        if self.phase != Phase::Idle {
            let _ = self.close();
        }
    }
}

fn object_kind(word: &str) -> Option<i32> {
    match word {
        "gage" | "raingage" => Some(OBJ_GAGE),
        "subcatch" | "subcatchment" => Some(OBJ_SUBCATCH),
        "node" => Some(OBJ_NODE),
        "link" => Some(OBJ_LINK),
        _ => None,
    }
}

fn object_label(obj: i32) -> &'static str {
    match obj {
        OBJ_GAGE => "gage",
        OBJ_SUBCATCH => "subcatchment",
        OBJ_NODE => "node",
        _ => "link",
    }
}

/// Readable properties, by the words the protocol uses.
fn get_property(obj: i32, word: &str) -> Option<i32> {
    Some(match (obj, word) {
        (OBJ_GAGE, "rainfall") => GAGE_RAINFALL,
        (OBJ_SUBCATCH, "area") => SUBCATCH_AREA,
        (OBJ_SUBCATCH, "rainfall") => SUBCATCH_RAINFALL,
        (OBJ_SUBCATCH, "evap") => SUBCATCH_EVAP,
        (OBJ_SUBCATCH, "infil") => SUBCATCH_INFIL,
        (OBJ_SUBCATCH, "runoff") => SUBCATCH_RUNOFF,
        (OBJ_NODE, "type") => NODE_TYPE,
        (OBJ_NODE, "elev") => NODE_ELEV,
        (OBJ_NODE, "maxdepth") => NODE_MAXDEPTH,
        (OBJ_NODE, "depth") => NODE_DEPTH,
        (OBJ_NODE, "head") => NODE_HEAD,
        (OBJ_NODE, "volume") => NODE_VOLUME,
        (OBJ_NODE, "latflow") => NODE_LATFLOW,
        (OBJ_NODE, "inflow") => NODE_INFLOW,
        (OBJ_NODE, "overflow") => NODE_OVERFLOW,
        (OBJ_LINK, "type") => LINK_TYPE,
        (OBJ_LINK, "length") => LINK_LENGTH,
        (OBJ_LINK, "slope") => LINK_SLOPE,
        (OBJ_LINK, "fulldepth") => LINK_FULLDEPTH,
        (OBJ_LINK, "fullflow") => LINK_FULLFLOW,
        (OBJ_LINK, "setting") => LINK_SETTING,
        (OBJ_LINK, "timeopen") => LINK_TIMEOPEN,
        (OBJ_LINK, "timeclosed") => LINK_TIMECLOSED,
        (OBJ_LINK, "flow") => LINK_FLOW,
        (OBJ_LINK, "depth") => LINK_DEPTH,
        (OBJ_LINK, "velocity") => LINK_VELOCITY,
        (OBJ_LINK, "topwidth") => LINK_TOPWIDTH,
        _ => return None,
    })
}

/// The properties `swmm_setValue` honours while a run is in progress.
fn set_property(obj: i32, word: &str) -> Option<i32> {
    Some(match (obj, word) {
        (OBJ_GAGE, "rainfall") => GAGE_RAINFALL,
        (OBJ_NODE, "latflow") => NODE_LATFLOW,
        // Outfall stage; the engine ignores it for other node types.
        (OBJ_NODE, "head") => NODE_HEAD,
        (OBJ_LINK, "setting") => LINK_SETTING,
        _ => return None,
    })
}

/// Serve requests from `input` until EOF or a `quit`/`abort`.
pub fn serve<E: Engine>(bridge: &mut Bridge<E>, input: impl BufRead, out: &mut impl Write) -> std::io::Result<()> {
    writeln!(out, "{}", bridge.banner())?;
    out.flush()?;
    for line in input.lines() {
        let line = line?;
        bridge.handle(&line, out)?;
        if bridge.exit {
            break;
        }
    }
    bridge.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// EPA's DLL
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod dll {
    //! `swmm5.dll` through `LoadLibraryExW`/`GetProcAddress`. The header
    //! declares every export `__declspec(dllexport) __stdcall`, which is
    //! what `extern "system"` means on 32-bit Windows (and plain C on
    //! 64-bit, where this binary cannot load the DLL anyway).

    use std::ffi::{c_char, c_int, c_void, CString};
    use std::path::Path;

    use super::{Engine, EngineError};

    type Hmodule = *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> Hmodule;
        fn GetProcAddress(module: Hmodule, name: *const c_char) -> *mut c_void;
        fn GetLastError() -> u32;
        fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            wide: *const u16,
            wide_len: c_int,
            out: *mut c_char,
            out_len: c_int,
            default_char: *const c_char,
            used_default: *mut c_int,
        ) -> c_int;
    }

    /// Dependent DLLs (EPA's `vcomp140.dll`) are looked up beside the DLL
    /// itself rather than beside this executable.
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;
    const ERROR_BAD_EXE_FORMAT: u32 = 193;
    const CP_ACP: u32 = 0;
    const WC_NO_BEST_FIT_CHARS: u32 = 0x0000_0400;

    type OpenFn = unsafe extern "system" fn(*const c_char, *const c_char, *const c_char) -> c_int;
    type StartFn = unsafe extern "system" fn(c_int) -> c_int;
    type StepFn = unsafe extern "system" fn(*mut f64) -> c_int;
    type StrideFn = unsafe extern "system" fn(c_int, *mut f64) -> c_int;
    type VoidFn = unsafe extern "system" fn() -> c_int;
    type MassBalFn = unsafe extern "system" fn(*mut f32, *mut f32, *mut f32) -> c_int;
    type GetErrorFn = unsafe extern "system" fn(*mut c_char, c_int) -> c_int;
    type GetCountFn = unsafe extern "system" fn(c_int) -> c_int;
    type GetNameFn = unsafe extern "system" fn(c_int, c_int, *mut c_char, c_int);
    type GetValueFn = unsafe extern "system" fn(c_int, c_int) -> f64;
    type SetValueFn = unsafe extern "system" fn(c_int, c_int, f64);

    pub struct Dll {
        open: OpenFn,
        start: StartFn,
        step: StepFn,
        stride: StrideFn,
        end: VoidFn,
        report: VoidFn,
        close: VoidFn,
        mass_bal: MassBalFn,
        version: VoidFn,
        get_error: GetErrorFn,
        get_warnings: VoidFn,
        get_count: GetCountFn,
        get_name: GetNameFn,
        get_value: GetValueFn,
        set_value: SetValueFn,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A path as the engine's C runtime wants it: bytes in the system
    /// code page. ASCII passes through; anything the code page cannot
    /// hold is refused rather than silently substituted.
    fn ansi(s: &str) -> Result<CString, String> {
        if s.is_ascii() {
            return CString::new(s).map_err(|_| "path holds a NUL byte".into());
        }
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut used: c_int = 0;
        let mut buf = vec![0u8; w.len() * 4 + 1];
        // SAFETY: both buffers are sized and live for the call; the flags
        // ask for a failure marker rather than a best-fit substitute.
        let n = unsafe {
            WideCharToMultiByte(
                CP_ACP,
                WC_NO_BEST_FIT_CHARS,
                w.as_ptr(),
                w.len() as c_int,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as c_int,
                std::ptr::null(),
                &mut used,
            )
        };
        if n <= 0 || used != 0 {
            return Err(format!(
                "the engine opens files through the C runtime and cannot represent {s:?} in the system code page; use an ASCII path"
            ));
        }
        buf.truncate(n as usize);
        CString::new(buf).map_err(|_| "path holds a NUL byte".into())
    }

    impl Dll {
        pub fn load(path: &Path) -> Result<Self, String> {
            let name = wide(&path.to_string_lossy());
            // SAFETY: a NUL-terminated UTF-16 string; no reserved handle.
            let module = unsafe { LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
            if module.is_null() {
                // SAFETY: plain thread-local read.
                let code = unsafe { GetLastError() };
                return Err(if code == ERROR_BAD_EXE_FORMAT {
                    format!(
                        "{} is not a {}-bit DLL. EPA's swmm5.dll is 32-bit; build this bridge for i686-pc-windows-msvc",
                        path.display(),
                        if cfg!(target_pointer_width = "64") { "64" } else { "32" }
                    )
                } else {
                    format!("could not load {} (Windows error {code})", path.display())
                });
            }
            let sym = |name: &str| -> Result<*mut c_void, String> {
                let c = CString::new(name).expect("static name");
                // SAFETY: a live module handle and a NUL-terminated name.
                let p = unsafe { GetProcAddress(module, c.as_ptr()) };
                if p.is_null() {
                    Err(format!("{} does not export {name}", path.display()))
                } else {
                    Ok(p)
                }
            };
            // SAFETY: each pointer came from GetProcAddress for the named
            // export, whose signature swmm5.h fixes; the transmutes only
            // re-type the pointer.
            unsafe {
                Ok(Self {
                    open: std::mem::transmute::<*mut c_void, OpenFn>(sym("swmm_open")?),
                    start: std::mem::transmute::<*mut c_void, StartFn>(sym("swmm_start")?),
                    step: std::mem::transmute::<*mut c_void, StepFn>(sym("swmm_step")?),
                    stride: std::mem::transmute::<*mut c_void, StrideFn>(sym("swmm_stride")?),
                    end: std::mem::transmute::<*mut c_void, VoidFn>(sym("swmm_end")?),
                    report: std::mem::transmute::<*mut c_void, VoidFn>(sym("swmm_report")?),
                    close: std::mem::transmute::<*mut c_void, VoidFn>(sym("swmm_close")?),
                    mass_bal: std::mem::transmute::<*mut c_void, MassBalFn>(sym("swmm_getMassBalErr")?),
                    version: std::mem::transmute::<*mut c_void, VoidFn>(sym("swmm_getVersion")?),
                    get_error: std::mem::transmute::<*mut c_void, GetErrorFn>(sym("swmm_getError")?),
                    get_warnings: std::mem::transmute::<*mut c_void, VoidFn>(sym("swmm_getWarnings")?),
                    get_count: std::mem::transmute::<*mut c_void, GetCountFn>(sym("swmm_getCount")?),
                    get_name: std::mem::transmute::<*mut c_void, GetNameFn>(sym("swmm_getName")?),
                    get_value: std::mem::transmute::<*mut c_void, GetValueFn>(sym("swmm_getValue")?),
                    set_value: std::mem::transmute::<*mut c_void, SetValueFn>(sym("swmm_setValue")?),
                })
            }
        }

        /// Turn a non-zero return code into `(code, message)`.
        fn check(&self, code: c_int) -> Result<(), EngineError> {
            if code == 0 {
                return Ok(());
            }
            let mut buf = vec![0u8; 1024];
            // SAFETY: the buffer is sized and the engine NUL-terminates.
            let reported = unsafe { (self.get_error)(buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let msg = String::from_utf8_lossy(&buf[..end]).trim().to_string();
            let msg = if msg.is_empty() {
                format!("engine error {code}")
            } else {
                msg
            };
            Err((if reported != 0 { reported } else { code }, msg))
        }
    }

    impl Engine for Dll {
        fn version(&self) -> i32 {
            // SAFETY: no arguments, no state.
            unsafe { (self.version)() }
        }

        fn open(&mut self, inp: &str, rpt: &str, out: &str) -> Result<(), EngineError> {
            let (i, r, o) = (ansi(inp).map_err(|m| (-1, m))?, ansi(rpt).map_err(|m| (-1, m))?, ansi(out).map_err(|m| (-1, m))?);
            // SAFETY: three NUL-terminated strings that outlive the call.
            let code = unsafe { (self.open)(i.as_ptr(), r.as_ptr(), o.as_ptr()) };
            self.check(code)
        }

        fn start(&mut self) -> Result<(), EngineError> {
            // SAFETY: saveResults = 1 writes the .out as runswmm does.
            let code = unsafe { (self.start)(1) };
            self.check(code)
        }

        fn step(&mut self) -> Result<f64, EngineError> {
            let mut days = 0.0f64;
            // SAFETY: an out-pointer to a local.
            let code = unsafe { (self.step)(&mut days) };
            self.check(code).map(|()| days)
        }

        fn stride(&mut self, seconds: i32) -> Result<f64, EngineError> {
            let mut days = 0.0f64;
            // SAFETY: an out-pointer to a local.
            let code = unsafe { (self.stride)(seconds, &mut days) };
            self.check(code).map(|()| days)
        }

        fn end(&mut self) -> Result<(), EngineError> {
            // SAFETY: no arguments.
            let code = unsafe { (self.end)() };
            self.check(code)
        }

        fn report(&mut self) -> Result<(), EngineError> {
            // SAFETY: no arguments.
            let code = unsafe { (self.report)() };
            self.check(code)
        }

        fn close(&mut self) -> Result<(), EngineError> {
            // SAFETY: no arguments.
            let code = unsafe { (self.close)() };
            self.check(code)
        }

        fn mass_balance(&mut self) -> (f32, f32, f32) {
            let (mut a, mut b, mut c) = (0.0f32, 0.0f32, 0.0f32);
            // SAFETY: three out-pointers to locals.
            unsafe { (self.mass_bal)(&mut a, &mut b, &mut c) };
            (a, b, c)
        }

        fn warnings(&self) -> i32 {
            // SAFETY: no arguments.
            unsafe { (self.get_warnings)() }
        }

        fn count(&self, obj: i32) -> i32 {
            // SAFETY: an integer argument the engine range-checks.
            unsafe { (self.get_count)(obj) }
        }

        fn name(&self, obj: i32, index: i32) -> String {
            let mut buf = vec![0u8; 256];
            // SAFETY: the buffer is sized; the engine NUL-terminates.
            unsafe { (self.get_name)(obj, index, buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            String::from_utf8_lossy(&buf[..end]).into_owned()
        }

        fn get(&self, property: i32, index: i32) -> f64 {
            // SAFETY: integer arguments the engine range-checks.
            unsafe { (self.get_value)(property, index) }
        }

        fn set(&mut self, property: i32, index: i32, value: f64) {
            // SAFETY: integer arguments the engine range-checks.
            unsafe { (self.set_value)(property, index, value) }
        }
    }
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("stormsewer-swmm-bridge32 {BRIDGE_VERSION}");
            return;
        }
        Some("--help") | None => {
            eprintln!("usage: stormsewer-swmm-bridge32 <path to swmm5.dll>\nSpeaks the StormSewer engine-bridge protocol on stdin/stdout; see docs/23-live-runs.md.");
            std::process::exit(if args.len() > 1 { 0 } else { 2 });
        }
        Some(_) => {}
    }
    let path = std::path::Path::new(&args[1]);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let engine = match dll::Dll::load(path) {
        Ok(e) => e,
        Err(m) => {
            let _ = writeln!(out, "err -1 {}", proto::quote(&m));
            let _ = out.flush();
            std::process::exit(3);
        }
    };
    let mut bridge = Bridge::new(engine);
    let stdin = std::io::stdin();
    if let Err(e) = serve(&mut bridge, stdin.lock(), &mut out) {
        eprintln!("bridge: {e}");
        bridge.shutdown();
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("stormsewer-swmm-bridge32 {BRIDGE_VERSION}");
        return;
    }
    eprintln!("bridge needs Windows: EPA's swmm5.dll is a Windows DLL");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in engine: two nodes, one link, a 60 s run at 10 s steps.
    #[derive(Default)]
    struct Fake {
        opened: bool,
        started: bool,
        steps: i32,
        latflow: Vec<f64>,
        setting: f64,
        fail_open: bool,
    }

    impl Engine for Fake {
        fn version(&self) -> i32 {
            52_004
        }
        fn open(&mut self, _: &str, _: &str, _: &str) -> Result<(), EngineError> {
            if self.fail_open {
                return Err((303, "ERROR 303: cannot open input file.".into()));
            }
            self.opened = true;
            self.latflow = vec![0.0; 2];
            Ok(())
        }
        fn start(&mut self) -> Result<(), EngineError> {
            self.started = true;
            Ok(())
        }
        fn step(&mut self) -> Result<f64, EngineError> {
            self.steps += 1;
            if self.steps >= 6 {
                return Ok(0.0);
            }
            Ok(f64::from(self.steps) * 10.0 / SECONDS_PER_DAY)
        }
        fn stride(&mut self, seconds: i32) -> Result<f64, EngineError> {
            let mut last = 0.0;
            for _ in 0..(seconds / 10).max(1) {
                last = self.step()?;
                if last == 0.0 {
                    break;
                }
            }
            Ok(last)
        }
        fn end(&mut self) -> Result<(), EngineError> {
            self.started = false;
            Ok(())
        }
        fn report(&mut self) -> Result<(), EngineError> {
            Ok(())
        }
        fn close(&mut self) -> Result<(), EngineError> {
            self.opened = false;
            Ok(())
        }
        fn mass_balance(&mut self) -> (f32, f32, f32) {
            (-0.023, 0.092, 0.0)
        }
        fn warnings(&self) -> i32 {
            1
        }
        fn count(&self, obj: i32) -> i32 {
            match obj {
                OBJ_NODE => 2,
                OBJ_LINK => 1,
                _ => 0,
            }
        }
        fn name(&self, obj: i32, index: i32) -> String {
            match (obj, index) {
                (OBJ_NODE, 0) => "J 1".into(),
                (OBJ_NODE, 1) => "OUT".into(),
                _ => "C1".into(),
            }
        }
        fn get(&self, property: i32, index: i32) -> f64 {
            match property {
                SYS_ROUTESTEP => 10.0,
                SYS_REPORTSTEP => 30.0,
                SYS_ELAPSEDTIME => f64::from(self.steps) * 10.0 / SECONDS_PER_DAY,
                NODE_HEAD => 100.5 + f64::from(index),
                NODE_LATFLOW => self.latflow[index as usize],
                LINK_SETTING => self.setting,
                _ => 0.0,
            }
        }
        fn set(&mut self, property: i32, index: i32, value: f64) {
            match property {
                NODE_LATFLOW => self.latflow[index as usize] = value,
                LINK_SETTING => self.setting = value,
                _ => {}
            }
        }
    }

    fn ask(b: &mut Bridge<Fake>, line: &str) -> String {
        let mut out = Vec::new();
        b.handle(line, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// `name` must be unique per test: the tests run in parallel and
    /// `fs::write` truncates, so two tests sharing a file race.
    fn opened(name: &str) -> (Bridge<Fake>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join("stormsewer-bridge-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let inp = dir.join(format!("fake model {name}.inp"));
        std::fs::write(&inp, "[OPTIONS]\nSTART_DATE 01/01/2007\nEND_DATE 01/01/2007\nEND_TIME 00:01:00\n").unwrap();
        let mut b = Bridge::new(Fake::default());
        let reply = ask(&mut b, &format!("open {} x.rpt x.out", proto::quote(&inp.to_string_lossy())));
        assert!(reply.starts_with("ok duration_s=60 routing_step_s=10 report_step_s=30 nodes=2 links=1"), "{reply}");
        (b, inp)
    }

    #[test]
    fn banner_names_both_versions() {
        let b = Bridge::new(Fake::default());
        assert!(b.banner().starts_with(&format!("ready bridge={BRIDGE_VERSION} engine=52004 arch=")));
    }

    #[test]
    fn a_run_steps_to_done_and_finishes_with_continuity() {
        let (mut b, _) = opened("run");
        assert_eq!(ask(&mut b, "names node"), "ok n=2\n\"J 1\"\nOUT\n");
        assert_eq!(ask(&mut b, "names link"), "ok n=1\nC1\n");
        assert_eq!(ask(&mut b, "step"), "ok t=10\n");
        assert_eq!(ask(&mut b, "get node \"J 1\" head"), "ok v=100.5\n");
        assert_eq!(ask(&mut b, "get node OUT head"), "ok v=101.5\n");
        assert_eq!(ask(&mut b, "set node OUT latflow 2.5"), "ok\n");
        assert_eq!(ask(&mut b, "get node OUT latflow"), "ok v=2.5\n");
        assert_eq!(ask(&mut b, "set link C1 setting 0.25"), "ok\n");
        assert_eq!(ask(&mut b, "get link C1 setting"), "ok v=0.25\n");
        assert_eq!(ask(&mut b, "get system elapsed"), "ok v=10\n");
        assert_eq!(ask(&mut b, "until 30"), "ok t=30\n");
        assert_eq!(ask(&mut b, "until 1000"), "done\n");
        assert_eq!(ask(&mut b, "step"), "done\n");
        assert_eq!(ask(&mut b, "finish"), "ok runoff=-0.023 flow=0.092 quality=0 warnings=1\n");
        assert_eq!(ask(&mut b, "step"), "err -1 no model is open\n");
        assert!(!b.engine.opened);
    }

    #[test]
    fn end_report_close_work_one_at_a_time() {
        let (mut b, _) = opened("stages");
        assert_eq!(ask(&mut b, "report"), "err -1 end the run first\n");
        assert_eq!(ask(&mut b, "stride 20"), "ok t=20\n");
        assert_eq!(ask(&mut b, "end"), "ok runoff=-0.023 flow=0.092 quality=0 warnings=1\n");
        assert_eq!(ask(&mut b, "report"), "ok\n");
        assert_eq!(ask(&mut b, "close"), "ok\n");
        assert_eq!(ask(&mut b, "close"), "err -1 no model is open\n");
    }

    #[test]
    fn bad_requests_are_named_and_never_fatal() {
        let (mut b, _) = opened("bad");
        assert_eq!(ask(&mut b, "get node NOPE head"), "err -1 \"unknown node NOPE\"\n");
        assert_eq!(ask(&mut b, "get node OUT colour"), "err -1 unknown node property colour\n");
        assert_eq!(ask(&mut b, "set node OUT head x"), "err -1 not a number: x\n");
        assert_eq!(ask(&mut b, "set link C1 flow 1"), "err -1 link property flow is not settable\n");
        assert_eq!(ask(&mut b, "frobnicate"), "err -1 unknown command frobnicate\n");
        assert_eq!(ask(&mut b, "open a b c"), "err -1 a model is already open\n");
        assert_eq!(ask(&mut b, "\"unterminated"), "err -1 bad request: unterminated quote\n");
        assert_eq!(ask(&mut b, "   "), "");
        assert!(!b.exit);
        assert_eq!(ask(&mut b, "abort"), "ok\n");
        assert!(b.exit);
        assert!(!b.engine.opened, "abort closes the engine");
    }

    #[test]
    fn an_engine_error_on_open_is_relayed_with_its_code() {
        let mut b = Bridge::new(Fake { fail_open: true, ..Default::default() });
        let dir = std::env::temp_dir().join("stormsewer-bridge-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let inp = dir.join("bad.inp");
        std::fs::write(&inp, "[OPTIONS]\n").unwrap();
        let reply = ask(&mut b, &format!("open {} r o", proto::quote(&inp.to_string_lossy())));
        assert_eq!(reply, "err 303 \"ERROR 303: cannot open input file.\"\n");
        assert_eq!(ask(&mut b, "open /no/such/file.inp r o").lines().next().unwrap().split(' ').nth(1), Some("-1"));
    }

    #[test]
    fn serve_prints_the_banner_and_stops_at_quit() {
        let mut b = Bridge::new(Fake::default());
        let input = std::io::Cursor::new(b"info\nquit\nstep\n".to_vec());
        let mut out = Vec::new();
        serve(&mut b, input, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert!(lines[0].starts_with("ready bridge="));
        assert!(lines[1].starts_with("ok bridge="), "{}", lines[1]);
        assert_eq!(lines[2], "ok");
    }
}
