// SPDX-License-Identifier: GPL-3.0-or-later
// wasm-bindgen's macro references an internal cfg that isn't set on host builds.
#![allow(unexpected_cfgs)]

//! WebAssembly bindings for the StormSewer engine.
//!
//! Thin `wasm-bindgen` wrappers over the pure engine so the same validated
//! hydrology/hydraulics can run in a browser with no server. Build with:
//!
//! ```text
//! wasm-pack build wasm --target web --out-dir pkg
//! # or, without wasm-pack:
//! cargo build -p stormsewer-wasm --target wasm32-unknown-unknown --release
//! wasm-bindgen target/wasm32-unknown-unknown/release/stormsewer_wasm.wasm \
//!     --target web --out-dir wasm/pkg
//! ```
//!
//! See `wasm/index.html` for a page that loads the result.

use stormsewer::hydraulics::{critical_depth, full_flow_capacity, normal_depth, G_US, K_MANNING_US};
use stormsewer::hydrology::{kirpich_minutes, tr55_sheet_flow_minutes};
use stormsewer::parse::parse_ssn;
use stormsewer::report::format_analysis;
use wasm_bindgen::prelude::*;

/// Rational-method peak flow, `Q = C·i·A` (cfs).
#[wasm_bindgen]
pub fn rational_peak(c: f64, i: f64, area_ac: f64) -> f64 {
    c * i * area_ac
}

/// Manning full-flow capacity of a circular pipe (cfs), US customary.
#[wasm_bindgen]
pub fn manning_full_flow_circular(n: f64, slope: f64, diameter_ft: f64) -> f64 {
    full_flow_capacity(n, slope, diameter_ft, K_MANNING_US)
}

/// Normal (uniform-flow) depth in a circular pipe (ft); `NaN` if the pipe would
/// surcharge (flow exceeds open-channel capacity).
#[wasm_bindgen]
pub fn normal_depth_circular(q: f64, n: f64, slope: f64, diameter_ft: f64) -> f64 {
    normal_depth(q, n, slope, diameter_ft, K_MANNING_US).unwrap_or(f64::NAN)
}

/// Critical depth in a circular pipe (ft).
#[wasm_bindgen]
pub fn critical_depth_circular(q: f64, diameter_ft: f64) -> f64 {
    critical_depth(q, diameter_ft, G_US)
}

/// Kirpich time of concentration (minutes).
#[wasm_bindgen]
pub fn kirpich_tc(length_ft: f64, slope: f64) -> f64 {
    kirpich_minutes(length_ft, slope)
}

/// NRCS TR-55 sheet-flow travel time (minutes).
#[wasm_bindgen]
pub fn tr55_sheet_flow(length_ft: f64, slope: f64, n: f64, p2_in: f64) -> f64 {
    tr55_sheet_flow_minutes(length_ft, slope, n, p2_in)
}

/// Analyze a `.ssn` network file and return the formatted text report, or an
/// `error: …` string on parse/analysis failure. This is the whole engine — the
/// same code path as the CLI — running in the browser.
#[wasm_bindgen]
pub fn analyze_ssn(text: &str) -> String {
    let parsed = match parse_ssn(text) {
        Ok(p) => p,
        Err(e) => return format!("error: {e}"),
    };
    match parsed.network.analyze(&parsed.idf, &parsed.options) {
        Ok(a) => format_analysis(&a),
        Err(e) => format!("error: {e}"),
    }
}
