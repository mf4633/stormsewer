// SPDX-License-Identifier: GPL-3.0-or-later

//! EPA SWMM integration for StormSewer: an engine registry that can offer
//! several stock EPA builds side by side, a reader for the binary `.out`
//! results, a parser for the `.rpt` report, and a bridge to the ALR
//! post-processor.
//!
//! # Why engines run as subprocesses
//!
//! EPA ships SWMM 5.2.4 for Windows as a **32-bit** build — `swmm5.dll`,
//! `runswmm.exe`, and `epaswmm5.exe` are all PE32. StormSewer is a 64-bit
//! process, and a 64-bit process cannot load a 32-bit DLL in-process, so
//! binding the toolkit through FFI is not available for the stock engine.
//!
//! Running `runswmm.exe` as a child process is what that constraint leaves,
//! and it is the better design regardless: any number of engine versions can
//! be registered and picked per run, a solver crash cannot take the editor
//! down with it, and every result carries the version and binary hash of the
//! executable that produced it.
//!
//! This crate is native-only and is deliberately not a dependency of the
//! `stormsewer` engine crate, which stays std-only and WASM-ready.

pub mod alr;
pub mod engine;
pub mod out;
pub mod pe;
pub mod python;
pub mod rpt;
pub mod sha256;

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// A file did not match the format it claimed to be.
    Format(String),
    /// A named object, engine, or file was not there.
    NotFound(String),
    /// The engine ran but the run itself failed.
    Engine(String),
    /// The ALR post-processor could not be run or understood.
    Alr(String),
    /// The Python kernel could not be started, reached, or understood.
    Python(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Format(m) => write!(f, "{m}"),
            Self::NotFound(m) => write!(f, "{m}"),
            Self::Engine(m) => write!(f, "{m}"),
            Self::Alr(m) => write!(f, "{m}"),
            Self::Python(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
